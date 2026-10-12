#!/usr/bin/env python3
"""Record the README demo as an asciicast v2 file from real twin output.

The demo host is synthetic (fake /proc, systemd units and Docker fixture), so the
recording never contains the recording machine's hostname, addresses or services:

    nginx (0.0.0.0:443) -> api (127.0.0.1:8000) -> postgresql (127.0.0.1:5432)
    docker.service running the Compose container shop-web-1 (live-restore disabled)

Usage: tools/record_demo.py path/to/twin docs/demo.cast
"""
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time

TCP_HEADER = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode"
COMMANDS = [
    "twin brief",
    'twin preflight "sudo systemctl restart postgresql"',
    'twin preflight "sudo systemctl restart docker"',
    'twin preflight "cargo test && git push"',
]
WIDTH, HEIGHT = 110, 34


def endpoint(ip, port):
    a, b, c, d = (int(x) for x in ip.split("."))
    return f"{d:02X}{c:02X}{b:02X}{a:02X}:{port:04X}"


def build_host(root):
    proc = root / "proc"
    units = root / "units"
    docker = root / "docker"
    host = root / "host"
    for directory in (proc / "net", proc / "sys/kernel", units, docker / "containers", host):
        directory.mkdir(parents=True, exist_ok=True)
    (proc / "sys/kernel/random").mkdir(parents=True, exist_ok=True)
    (proc / "sys/kernel/random/boot_id").write_text("5f0c9a1e-2b7d-4c3e-9f10-6a2b8c4d1e00\n")
    (proc / "self/ns").mkdir(parents=True, exist_ok=True)
    os.symlink("net:[4026531840]", proc / "self/ns/net")
    (proc / "sys/kernel/hostname").write_text("web-1\n")
    (proc / "sys/kernel/osrelease").write_text("6.8.0-45-generic\n")
    units.joinpath("nginx.service").write_text("[Unit]\nWants=network-online.target\n[Service]\nExecStart=/usr/sbin/nginx\n")
    units.joinpath("api.service").write_text("[Unit]\nRequires=postgresql.service\nAfter=postgresql.service\n[Service]\nExecStart=/usr/bin/api\n")
    units.joinpath("postgresql.service").write_text("[Service]\nExecStart=/usr/lib/postgresql/16/bin/postgres\n")
    units.joinpath("docker.service").write_text("[Unit]\nWants=network-online.target\n[Service]\nExecStart=/usr/bin/dockerd\n")

    processes = [
        (1, "systemd", None),
        (812, "nginx", "nginx.service"),
        (1204, "api", "api.service"),
        (1530, "postgres", "postgresql.service"),
        (944, "dockerd", "docker.service"),
    ]
    for pid, name, unit in processes:
        directory = proc / str(pid)
        (directory / "fd").mkdir(parents=True, exist_ok=True)
        (directory / "ns").mkdir(parents=True, exist_ok=True)
        os.symlink("net:[4026531840]", directory / "ns/net")
        os.symlink(f"/usr/bin/{name}", directory / "exe")
        (directory / "stat").write_text(f"{pid} ({name}) S 1 {pid} {pid} 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 100 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n")
        (directory / "status").write_text("Uid:\t0\t0\t0\t0\n")
        (directory / "cmdline").write_bytes(f"/usr/bin/{name}\0".encode())
        cgroup = f"0::/system.slice/{unit}\n" if unit else "0::/init.scope\n"
        (directory / "cgroup").write_text(cgroup)

    sockets = [
        # (pid, fd, inode, local, remote, state)
        (812, 6, 5001, ("0.0.0.0", 443), ("0.0.0.0", 0), "0A"),
        (1204, 7, 5002, ("127.0.0.1", 8000), ("0.0.0.0", 0), "0A"),
        (1530, 5, 5003, ("127.0.0.1", 5432), ("0.0.0.0", 0), "0A"),
        (812, 9, 5004, ("127.0.0.1", 41822), ("127.0.0.1", 8000), "01"),
        (1204, 11, 5005, ("127.0.0.1", 50114), ("127.0.0.1", 5432), "01"),
        (1204, 12, 5006, ("127.0.0.1", 50116), ("127.0.0.1", 5432), "01"),
    ]
    lines = [TCP_HEADER]
    for index, (pid, fd, inode, local, remote, state) in enumerate(sockets):
        lines.append(f"{index:>4}: {endpoint(*local)} {endpoint(*remote)} {state} 00000000:00000000 00:00000000 00000000     0        0 {inode} 1 0000000000000000 100 0 0 10 0")
        os.symlink(f"socket:[{inode}]", proc / str(pid) / "fd" / str(fd))
    (proc / "net/tcp").write_text("\n".join(lines) + "\n")
    (proc / "net/tcp6").write_text(TCP_HEADER + "\n")
    udp_header = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode ref pointer drops"
    (proc / "net/udp").write_text(udp_header + "\n")
    (proc / "net/udp6").write_text(udp_header + "\n")
    (proc / "net/unix").write_text("Num       RefCount Protocol Flags    Type St Inode Path\n")

    container_id = "4f1c2b9e7d6a"
    labels = {"com.docker.compose.project": "shop", "com.docker.compose.service": "web"}
    (docker / "containers/list.json").write_text(json.dumps([{
        "Id": container_id, "Names": ["/shop-web-1"], "Image": "shop/web:2.3", "State": "running",
        "Labels": labels, "Ports": [{"IP": "0.0.0.0", "PrivatePort": 3000, "PublicPort": 3000, "Type": "tcp"}],
        "Mounts": [],
    }]))
    (docker / f"containers/{container_id}.json").write_text(json.dumps({
        "Id": container_id, "Name": "/shop-web-1", "Config": {"Image": "shop/web:2.3", "Labels": labels},
        "State": {"Status": "running", "Running": True, "Pid": 0},
        "NetworkSettings": {"Ports": {"3000/tcp": [{"HostIp": "0.0.0.0", "HostPort": "3000"}]}},
        "Mounts": [],
    }))
    (docker / "info.json").write_text(json.dumps({"LiveRestoreEnabled": False}))
    return proc, units, docker, host


def run(twin, command, env, cwd):
    argv = [twin] + json.loads(json.dumps(command_argv(command)))
    result = subprocess.run(argv, env=env, cwd=cwd, capture_output=True, text=True)
    return result.stdout


def command_argv(command):
    import shlex
    return shlex.split(command)[1:]


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    twin = str(pathlib.Path(sys.argv[1]).resolve())
    output = pathlib.Path(sys.argv[2])
    with tempfile.TemporaryDirectory(prefix="twin-demo-") as directory:
        root = pathlib.Path(directory)
        proc, units, docker, host = build_host(root)
        env = {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "HOME": str(root / "home"),
            "TWIN_PROC_ROOT": str(proc),
            "TWIN_SYSTEMD_UNIT_ROOT": str(units),
            "TWIN_DOCKER_FIXTURE": str(docker),
            "TWIN_HOST_ROOT": str(host),
            "COLUMNS": str(WIDTH),
        }
        subprocess.run([twin, "init"], env=env, cwd=root, capture_output=True, check=True)
        subprocess.run([twin, "scan"], env=env, cwd=root, capture_output=True, check=True)
        events = []
        clock = 0.5
        for command in COMMANDS:
            events.append([round(clock, 3), "o", "\x1b[1;32m$\x1b[0m "])
            for char in command:
                clock += 0.035
                events.append([round(clock, 3), "o", char])
            clock += 0.4
            events.append([round(clock, 3), "o", "\r\n"])
            text = run(twin, command, env, root)
            for line in text.rstrip("\n").split("\n"):
                clock += 0.03
                events.append([round(clock, 3), "o", line + "\r\n"])
            clock += 2.2
        header = {
            "version": 2,
            "width": WIDTH,
            "height": HEIGHT,
            "timestamp": int(time.time()),
            "title": "twin: know what breaks before you touch a Linux box",
            "env": {"SHELL": "/bin/bash", "TERM": "xterm-256color"},
        }
        output.parent.mkdir(parents=True, exist_ok=True)
        with output.open("w") as handle:
            handle.write(json.dumps(header) + "\n")
            for event in events:
                handle.write(json.dumps(event) + "\n")
        print(f"{output}: {len(events)} events, {clock:.1f}s")


if __name__ == "__main__":
    main()
