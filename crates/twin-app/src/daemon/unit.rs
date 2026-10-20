use std::path::Path;

/// The hardened unit `twin daemon install-unit --print` shows. twin never installs it.
///
/// The daemon runs as root only to hold four read capabilities: reading other users'
/// files and `/proc` entries, and loading eBPF tracepoints. Everything that could change
/// the host is removed from the bounding set or filtered at the syscall level, and the
/// only writable paths are its own state and runtime directories.
pub fn systemd_unit(twin: &Path) -> String {
    format!(
        "\
[Unit]
Description=twin read-only operational twin
Documentation=https://github.com/KM-Alee/twin-ops
After=network-online.target docker.service containerd.service
Wants=network-online.target

[Service]
Type=simple
ExecStart={twin} daemon run --state-dir /var/lib/twin --socket /run/twin/twin.sock
Restart=on-failure
RestartSec=5s

# Clients in group twin may query the socket; nobody else can reach it.
Group=twin
UMask=0027
StateDirectory=twin
StateDirectoryMode=0700
RuntimeDirectory=twin
RuntimeDirectoryMode=0750

# Read-only host access: only these capabilities exist inside the service.
CapabilityBoundingSet=CAP_DAC_READ_SEARCH CAP_SYS_PTRACE CAP_BPF CAP_PERFMON
AmbientCapabilities=CAP_DAC_READ_SEARCH CAP_SYS_PTRACE CAP_BPF CAP_PERFMON
NoNewPrivileges=yes

ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=yes
PrivateDevices=yes
DevicePolicy=closed
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
ProtectClock=yes
ProtectHostname=yes
RestrictNamespaces=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
RemoveIPC=yes
KeyringMode=private

# Unix sockets for the client socket, Docker and containerd; netlink for socket
# diagnostics; inet for the local Kubernetes API and DNS answers.
RestrictAddressFamilies=AF_UNIX AF_NETLINK AF_INET AF_INET6
SystemCallArchitectures=native
SystemCallFilter=@system-service bpf perf_event_open
SystemCallFilter=~@resources @mount @reboot @swap @module @raw-io @clock @cpu-emulation @obsolete
SystemCallErrorNumber=EPERM

[Install]
WantedBy=multi-user.target
",
        twin = twin.display()
    )
}
