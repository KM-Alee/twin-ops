//! `twin daemon run`: the privileged watch loop plus a Unix socket that serves the same
//! read-only JSON-RPC handler as `twin mcp` to authorized local users.

mod client;

use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
use twin_app::daemon::AccessPolicy;
use twin_app::{AppError, TwinLayout, WatchCommand, WatchResult, WatchSink};

use crate::mcp::{error_response, McpServer, ServeMode};

pub use client::{DaemonClient, DaemonUnavailable};

/// Connections served at once; further clients get an error and fall back locally.
pub const MAX_CLIENTS: usize = 32;
const MAX_LINE_BYTES: u64 = 256 * 1024;
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_millis(500);
const SOCKET_MODE: u32 = 0o660;
const MAX_ACCEPT_FAILURES: u32 = 120;

#[derive(Debug)]
pub enum DaemonError {
    Io { context: String, source: io::Error },
    App(AppError),
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { context, source } => write!(f, "{context}: {source}"),
            Self::App(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for DaemonError {}

impl From<AppError> for DaemonError {
    fn from(error: AppError) -> Self {
        Self::App(error)
    }
}

fn io_error(context: impl Into<String>) -> impl FnOnce(io::Error) -> DaemonError {
    let context = context.into();
    move |source| DaemonError::Io { context, source }
}

/// The daemon's listening socket. The file is removed when this is dropped.
pub struct DaemonSocket {
    listener: UnixListener,
    path: PathBuf,
}

impl DaemonSocket {
    /// Binds `path` with mode 0660. A socket left by a stopped daemon is replaced; a live
    /// daemon or any other kind of file there is an error.
    pub fn bind(path: &Path) -> Result<Self, DaemonError> {
        if let Ok(metadata) = std::fs::symlink_metadata(path) {
            if !metadata.file_type().is_socket() {
                return Err(DaemonError::Io {
                    context: format!("{} exists and is not a socket", path.display()),
                    source: io::Error::from(ErrorKind::AlreadyExists),
                });
            }
            if UnixStream::connect(path).is_ok() {
                return Err(DaemonError::Io {
                    context: format!("a twin daemon is already serving {}", path.display()),
                    source: io::Error::from(ErrorKind::AddrInUse),
                });
            }
            std::fs::remove_file(path)
                .map_err(io_error(format!("remove stale socket {}", path.display())))?;
        }
        let listener =
            UnixListener::bind(path).map_err(io_error(format!("bind {}", path.display())))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(SOCKET_MODE))
            .map_err(io_error(format!("set mode on {}", path.display())))?;
        Ok(Self {
            listener,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DaemonSocket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Unblocks a daemon waiting in `accept` so it can notice its stop flag.
pub fn wake(socket: &Path) {
    let _ = UnixStream::connect(socket);
}

/// Serves the handler in daemon mode to every peer the policy allows.
pub struct DaemonServer {
    handler: McpServer,
    policy: AccessPolicy,
    active: AtomicUsize,
}

impl DaemonServer {
    pub fn new(handler: McpServer, policy: AccessPolicy) -> Self {
        Self {
            handler: handler.with_mode(ServeMode::Daemon),
            policy,
            active: AtomicUsize::new(0),
        }
    }

    /// Accepts clients until `stop` is set and [`wake`] is called. Open sessions end
    /// within half a second of the stop.
    pub fn serve(&self, socket: &DaemonSocket, stop: &AtomicBool) -> Result<(), DaemonError> {
        std::thread::scope(|scope| {
            let mut failures = 0u32;
            for stream in socket.listener.incoming() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                // Accept errors on a bound socket are transient (a client that gave up,
                // descriptor or buffer exhaustion); only a persistent run of them is fatal.
                let stream = match stream {
                    Ok(stream) => {
                        failures = 0;
                        stream
                    }
                    Err(error) if failures >= MAX_ACCEPT_FAILURES => {
                        return Err(io_error("accept client")(error))
                    }
                    Err(_) => {
                        failures += 1;
                        std::thread::sleep(POLL);
                        continue;
                    }
                };
                if self.active.fetch_add(1, Ordering::SeqCst) >= MAX_CLIENTS {
                    self.active.fetch_sub(1, Ordering::SeqCst);
                    refuse(&stream, "the twin daemon is serving too many clients");
                    continue;
                }
                scope.spawn(move || {
                    self.session(stream, stop);
                    self.active.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Ok(())
        })
    }

    fn session(&self, stream: UnixStream, stop: &AtomicBool) {
        let Ok(peer) = twin_app::daemon::peer_credentials(&stream) else {
            return;
        };
        if !self.policy.allows(&peer) {
            refuse(
                &stream,
                &format!(
                    "uid {} may not query the twin daemon; an administrator can add it to the daemon's group (gid {})",
                    peer.uid(),
                    self.policy.gid()
                ),
            );
            return;
        }
        if stream.set_read_timeout(Some(POLL)).is_err() {
            return;
        }
        let Ok(mut writer) = stream.try_clone() else {
            return;
        };
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        let mut idle_since = Instant::now();
        while !stop.load(Ordering::SeqCst) {
            let room = MAX_LINE_BYTES.saturating_sub(line.len() as u64);
            match (&mut reader).take(room).read_until(b'\n', &mut line) {
                Ok(0) if line.is_empty() => return,
                Ok(_) if line.last() == Some(&b'\n') => {
                    let text = String::from_utf8_lossy(&line).to_string();
                    line.clear();
                    idle_since = Instant::now();
                    if let Some(response) = self.handler.handle_line(text.trim()) {
                        if writeln!(writer, "{response}")
                            .and_then(|()| writer.flush())
                            .is_err()
                        {
                            return;
                        }
                    }
                }
                Ok(_) if line.len() as u64 >= MAX_LINE_BYTES => {
                    refuse(&writer, "request larger than 256 KiB");
                    return;
                }
                Ok(_) => return,
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    if idle_since.elapsed() > IDLE_TIMEOUT {
                        return;
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => return,
            }
        }
    }
}

fn refuse(mut stream: &UnixStream, message: &str) {
    let _ = writeln!(stream, "{}", error_response(Value::Null, -32001, message));
}

const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// `twin brief` through the daemon, refused when the daemon's graph is older than the
/// caller accepts.
pub fn brief(
    socket: &Path,
    request: &twin_app::BriefRequest,
) -> Result<twin_app::BriefResult, DaemonUnavailable> {
    let mut client = DaemonClient::connect(socket, CLIENT_TIMEOUT)?;
    let result: twin_app::BriefResult = client.call_typed(
        "twin/brief",
        &serde_json::json!({ "target": request.target, "since": request.changes_since }),
    )?;
    fresh_enough(Some(result.graph_age_secs), request.max_graph_age_secs)?;
    Ok(result)
}

/// `twin preflight` through the daemon. Commands that run a script or makefile are
/// refused here: reading them must use the caller's own permissions.
pub fn preflight(
    socket: &Path,
    request: &twin_app::PreflightRequest,
) -> Result<twin_app::PreflightResult, DaemonUnavailable> {
    if twin_app::preflight_reads_files(&request.command) {
        return Err(DaemonUnavailable::new(
            "the command runs a script, which is read with your own permissions",
        ));
    }
    let mut client = DaemonClient::connect(socket, CLIENT_TIMEOUT)?;
    let result: twin_app::PreflightResult = client.call_typed(
        "twin/preflight",
        &serde_json::json!({
            "command": request.command,
            "cwd": request.cwd,
            "file_changes": request.file_changes,
        }),
    )?;
    fresh_enough(result.graph_age_secs, request.max_graph_age_secs)?;
    Ok(result)
}

fn fresh_enough(age: Option<u64>, limit: u64) -> Result<(), DaemonUnavailable> {
    match age {
        Some(age) if age > limit => Err(DaemonUnavailable::new(format!(
            "its graph is {age} s old, past the {limit} s limit"
        ))),
        _ => Ok(()),
    }
}

/// What `twin daemon status` reports.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DaemonStatus {
    pub socket: String,
    pub is_reachable: bool,
    pub version: Option<String>,
    pub graph_age_secs: Option<u64>,
    pub reason: Option<String>,
}

pub fn status(socket: &Path) -> DaemonStatus {
    let answer = DaemonClient::connect(socket, CLIENT_TIMEOUT)
        .and_then(|mut client| client.call("twin/status", &serde_json::json!({})));
    match answer {
        Ok(status) => DaemonStatus {
            socket: socket.display().to_string(),
            is_reachable: true,
            version: status
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_string),
            graph_age_secs: status.get("graph_age_secs").and_then(Value::as_u64),
            reason: None,
        },
        Err(unavailable) => DaemonStatus {
            socket: socket.display().to_string(),
            is_reachable: false,
            version: None,
            graph_age_secs: None,
            reason: Some(unavailable.reason().to_string()),
        },
    }
}

pub struct DaemonOptions {
    pub layout: TwinLayout,
    pub config: Option<PathBuf>,
    pub proc_root: PathBuf,
    pub socket: PathBuf,
    pub interval: String,
    /// `Ok` attaches live TCP tracing; `Err` records the reason as an eBPF coverage gap.
    /// `None` leaves eBPF off without a gap.
    pub ebpf: Option<Result<(), String>>,
}

/// Binds the socket, scans once, then runs the watch loop and the socket server until
/// `stop` is set (the caller also calls [`wake`]) or the watch loop fails.
pub fn run<S: WatchSink + Send>(
    options: DaemonOptions,
    stop: &AtomicBool,
    sink: &mut S,
) -> Result<WatchResult, DaemonError> {
    let mut watch = twin_app::parse_watch(&WatchCommand {
        config_override: options.config.clone(),
        interval: options.interval.clone(),
        duration: None,
        ticks: None,
        ebpf: matches!(options.ebpf, Some(Ok(()))),
        events: matches!(options.ebpf, Some(Ok(()))).then(|| "tcp".to_string()),
    })?;
    if let Some(Err(reason)) = &options.ebpf {
        watch.ebpf = twin_app::WatchEbpf::Unavailable {
            reason: reason.clone(),
        };
    }
    options
        .layout
        .ensure_dirs()
        .map_err(io_error("create the daemon's state directory"))?;
    let socket = DaemonSocket::bind(&options.socket)?;
    twin_app::ensure_initialized_in(&options.layout, options.config.as_deref())?;
    twin_app::scan_in(
        &options.layout,
        twin_app::ScanRequest::default_with_config(options.config.clone()),
        &options.proc_root,
    )?;
    let policy = AccessPolicy::current().map_err(io_error("read the daemon's own identity"))?;
    let server = DaemonServer::new(
        McpServer::new(
            options.layout.clone(),
            options.config.clone(),
            options.proc_root.clone(),
            u64::MAX,
        ),
        policy,
    );
    std::thread::scope(|scope| {
        let watcher = scope.spawn(|| {
            let result = twin_app::watch_in(&options.layout, watch, &options.proc_root, stop, sink);
            stop.store(true, Ordering::SeqCst);
            wake(socket.path());
            result
        });
        let served = server.serve(&socket, stop);
        stop.store(true, Ordering::SeqCst);
        let watched = watcher.join().map_err(|_| DaemonError::Io {
            context: "the watch loop stopped unexpectedly".to_string(),
            source: io::Error::other("watch thread panicked"),
        })?;
        served?;
        Ok(watched?)
    })
}
