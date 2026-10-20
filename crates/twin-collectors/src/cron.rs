use std::fmt;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

const MAX_FILE_BYTES: u64 = 64 * 1024;
const MAX_FILES: usize = 256;
const MAX_ENTRIES: usize = 1024;
const RUN_PARTS: [(&str, &str); 4] = [
    ("etc/cron.hourly", "@hourly"),
    ("etc/cron.daily", "@daily"),
    ("etc/cron.weekly", "@weekly"),
    ("etc/cron.monthly", "@monthly"),
];
const SCHEDULE_KEYWORDS: [&str; 8] = [
    "@reboot",
    "@yearly",
    "@annually",
    "@monthly",
    "@weekly",
    "@daily",
    "@midnight",
    "@hourly",
];

/// One scheduled command. The command text is kept only for the caller to classify
/// transiently; it can carry credentials and must never be stored or shown.
#[derive(Clone)]
pub struct CronEntry {
    path: PathBuf,
    line: usize,
    user: Option<String>,
    schedule: String,
    command: String,
}

impl CronEntry {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }

    pub fn schedule(&self) -> &str {
        &self.schedule
    }

    pub fn command(&self) -> &str {
        &self.command
    }
}

impl fmt::Debug for CronEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CronEntry")
            .field("path", &self.path)
            .field("line", &self.line)
            .field("user", &self.user)
            .field("schedule", &self.schedule)
            .field("command", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CronWarningKind {
    Unreadable,
    Oversized,
    Malformed,
    Truncated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronWarning {
    kind: CronWarningKind,
    path: PathBuf,
}

impl CronWarning {
    pub fn kind(&self) -> CronWarningKind {
        self.kind
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn detail(&self) -> &'static str {
        match self.kind {
            CronWarningKind::Unreadable => "cron source unreadable; its jobs are unknown",
            CronWarningKind::Oversized => "crontab larger than 64 KiB was not read",
            CronWarningKind::Malformed => "malformed crontab entry skipped",
            CronWarningKind::Truncated => "cron job limit reached; remaining jobs not read",
        }
    }
}

#[derive(Debug, Default)]
pub struct CronBatch {
    entries: Vec<CronEntry>,
    warnings: Vec<CronWarning>,
}

impl CronBatch {
    pub fn entries(&self) -> &[CronEntry] {
        &self.entries
    }

    pub fn warnings(&self) -> &[CronWarning] {
        &self.warnings
    }

    /// Whether every cron source that exists was read in full, so a job that is missing
    /// now was really removed.
    pub fn is_complete(&self) -> bool {
        !self.warnings.iter().any(|warning| {
            matches!(
                warning.kind,
                CronWarningKind::Unreadable
                    | CronWarningKind::Oversized
                    | CronWarningKind::Truncated
            )
        })
    }

    fn warn(&mut self, kind: CronWarningKind, path: &Path) {
        self.warnings.push(CronWarning {
            kind,
            path: path.to_path_buf(),
        });
    }

    fn truncate(&mut self, path: &Path) {
        if !self
            .warnings
            .iter()
            .any(|warning| warning.kind == CronWarningKind::Truncated)
        {
            self.warn(CronWarningKind::Truncated, path);
        }
    }

    fn is_full(&mut self, path: &Path) -> bool {
        let full = self.entries.len() >= MAX_ENTRIES;
        if full {
            self.truncate(path);
        }
        full
    }
}

/// Reads the system crontab, `/etc/cron.d`, user spools and the run-parts directories
/// under `root`. Missing sources are normal; unreadable ones are coverage gaps.
pub fn collect_cron(root: &Path) -> CronBatch {
    let mut batch = CronBatch::default();
    let mut files = 0usize;
    read_crontab(
        &mut batch,
        &mut files,
        &root.join("etc/crontab"),
        Format::System,
    );
    for file in list_files(&mut batch, &root.join("etc/cron.d")) {
        read_crontab(&mut batch, &mut files, &file, Format::System);
    }
    for spool in ["var/spool/cron/crontabs", "var/spool/cron"] {
        for file in list_files(&mut batch, &root.join(spool)) {
            let Some(user) = file
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
            else {
                continue;
            };
            read_crontab(&mut batch, &mut files, &file, Format::User(user));
        }
    }
    for (directory, schedule) in RUN_PARTS {
        let directory = root.join(directory);
        for script in list_files(&mut batch, &directory) {
            if batch.is_full(&directory) {
                return batch;
            }
            let Some(name) = script.file_name() else {
                continue;
            };
            let host_path = Path::new("/").join(
                directory
                    .strip_prefix(root)
                    .unwrap_or(&directory)
                    .join(name),
            );
            batch.entries.push(CronEntry {
                path: script.clone(),
                line: 0,
                user: Some("root".into()),
                schedule: schedule.into(),
                command: host_path.display().to_string(),
            });
        }
    }
    batch
}

enum Format {
    System,
    User(String),
}

/// Regular files cron itself would read: run-parts skips names with dots and backups.
fn list_files(batch: &mut CronBatch, directory: &Path) -> Vec<PathBuf> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Vec::new(),
        Err(_) => {
            batch.warn(CronWarningKind::Unreadable, directory);
            return Vec::new();
        }
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy())
                .is_some_and(|name| !name.contains('.') && !name.ends_with('~'))
        })
        .collect();
    files.sort();
    files
}

fn read_crontab(batch: &mut CronBatch, files: &mut usize, path: &Path, format: Format) {
    if *files >= MAX_FILES {
        return batch.truncate(path);
    }
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return,
        Err(_) => return batch.warn(CronWarningKind::Unreadable, path),
    };
    *files += 1;
    let mut bytes = Vec::new();
    if file
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return batch.warn(CronWarningKind::Unreadable, path);
    }
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return batch.warn(CronWarningKind::Oversized, path);
    }
    let text = String::from_utf8_lossy(&bytes);
    for (index, raw) in text.lines().enumerate() {
        if batch.is_full(path) {
            return;
        }
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || is_assignment(line) {
            continue;
        }
        match parse_entry(line, &format) {
            Some((schedule, user, command)) => batch.entries.push(CronEntry {
                path: path.to_path_buf(),
                line: index + 1,
                user,
                schedule,
                command,
            }),
            None => batch.warn(CronWarningKind::Malformed, path),
        }
    }
}

/// `NAME=value` lines set the job environment; their values are never read.
fn is_assignment(line: &str) -> bool {
    let name = line.split('=').next().unwrap_or_default().trim();
    line.contains('=')
        && !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && !name.starts_with(|ch: char| ch.is_ascii_digit())
}

fn parse_entry(line: &str, format: &Format) -> Option<(String, Option<String>, String)> {
    let mut rest = line;
    let schedule = if line.starts_with('@') {
        let (keyword, after) = split_word(rest)?;
        if !SCHEDULE_KEYWORDS.contains(&keyword) {
            return None;
        }
        rest = after;
        keyword.to_string()
    } else {
        let mut fields = Vec::with_capacity(5);
        for _ in 0..5 {
            let (field, after) = split_word(rest)?;
            if !field
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '*' | ',' | '/' | '-'))
            {
                return None;
            }
            fields.push(field);
            rest = after;
        }
        fields.join(" ")
    };
    let user = match format {
        Format::System => {
            let (user, after) = split_word(rest)?;
            if !user
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
            {
                return None;
            }
            rest = after;
            user.to_string()
        }
        Format::User(user) => user.clone(),
    };
    let command = until_unescaped_percent(rest).trim().to_string();
    (!command.is_empty()).then_some((schedule, Some(user), command))
}

fn split_word(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start();
    if text.is_empty() {
        return None;
    }
    Some(match text.find(char::is_whitespace) {
        Some(end) => (&text[..end], &text[end..]),
        None => (text, ""),
    })
}

/// Cron turns the first unescaped `%` into a newline and feeds the rest to stdin.
fn until_unescaped_percent(command: &str) -> &str {
    let bytes = command.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'%' && (index == 0 || bytes[index - 1] != b'\\') {
            return &command[..index];
        }
    }
    command
}
