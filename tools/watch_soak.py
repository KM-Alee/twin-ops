#!/usr/bin/env python3
"""Measure live read-only watch RSS using an isolated temporary twin store."""
import argparse
from contextlib import closing
import json
import hashlib
import os
from pathlib import Path
import selectors
import sqlite3
import statistics
import subprocess
import tempfile
import time


def database_cardinality(database, elapsed):
    sample = {"elapsed_seconds": round(elapsed, 3), "available": False}
    try:
        with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True, timeout=0.1)) as connection:
            connection.execute("BEGIN")
            counts = {}
            for table in ("nodes", "edges", "observations", "edge_observations", "node_history",
                          "edge_history", "collector_runs", "snapshots", "snapshot_nodes",
                          "snapshot_edges", "snapshot_edge_observations", "test_runs"):
                counts[table] = connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
            for table in ("nodes", "edges"):
                counts[f"{table}_by_state"] = {
                    state: connection.execute(f"SELECT COUNT(*) FROM {table} WHERE state = ?", (state,)).fetchone()[0]
                    for state in ("active", "stale", "gone")
                }
            connection.rollback()
        sizes = {}
        for label, suffix in (("database", ""), ("wal", "-wal"), ("shm", "-shm")):
            path = Path(str(database) + suffix)
            sizes[label] = path.stat().st_size if path.exists() else 0
        sample.update(available=True, row_counts=counts, bytes=sizes)
    except (sqlite3.Error, OSError):
        pass
    return sample


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/twin"))
    parser.add_argument("--seconds", type=int, default=600)
    parser.add_argument("--output", type=Path, default=Path("bench-results/watch-soak.json"))
    args = parser.parse_args()
    if args.seconds < 1:
        parser.error("--seconds must be positive")
    binary = args.binary.resolve()
    binary_sha256 = hashlib.sha256(binary.read_bytes()).hexdigest()
    samples = []
    cardinality_samples = []
    phases = {}
    ticks = 0
    retained_ticks = None
    def consume(line):
        nonlocal ticks, retained_ticks
        event = json.loads(line)
        payload = event.get("data", event)
        if event.get("event") == "tick":
            ticks += 1
            for name, elapsed in payload["collector_timings_ms"].items():
                phases.setdefault(name, []).append(elapsed)
        if isinstance(payload, dict) and "ticks_completed" in payload:
            retained_ticks = len(payload["ticks"])
            ticks = payload["ticks_completed"]

    with tempfile.TemporaryDirectory(prefix="twin-watch-soak-") as directory:
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith("TWIN_") and key not in ("SUDO_UID", "SUDO_USER")}
        for kind in ("DATA", "CONFIG", "STATE"):
            environment[f"XDG_{kind}_HOME"] = str(Path(directory) / kind.lower())
        subprocess.run([str(binary), "init"], env=environment, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        database = Path(directory) / "data" / "twin" / "twin.db"
        started = time.monotonic()
        next_cardinality = 0.0
        child = subprocess.Popen([str(binary), "watch", "--json", "--interval", "1s",
                                  "--duration", f"{args.seconds}s"], env=environment,
                                 stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        selector = selectors.DefaultSelector()
        selector.register(child.stdout, selectors.EVENT_READ)
        pending = b""
        while child.poll() is None:
            elapsed = time.monotonic() - started
            if elapsed >= next_cardinality:
                cardinality_samples.append(database_cardinality(database, elapsed))
                next_cardinality = elapsed + 60
            try:
                status = Path(f"/proc/{child.pid}/status").read_text()
                rss = next(int(line.split()[1]) for line in status.splitlines()
                           if line.startswith("VmRSS:"))
                samples.append((round(time.monotonic() - started, 3), rss))
            except (FileNotFoundError, ProcessLookupError, StopIteration):
                pass
            for key, _ in selector.select(timeout=1):
                chunk = os.read(key.fd, 65536)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                pending += chunk
                while b"\n" in pending:
                    line, pending = pending.split(b"\n", 1)
                    consume(line)
            if not selector.get_map():
                child.wait()
        # The final report is emitted after the last tick.
        pending += child.stdout.read()
        for line in pending.splitlines():
            consume(line)
        child.stdout.close()
        selector.close()
        elapsed = round(time.monotonic() - started, 3)
        cardinality_samples.append(database_cardinality(database, elapsed))
        return_code = child.returncode
    steady = [rss for seconds, rss in samples if seconds >= min(60, args.seconds / 4)]
    window = max(1, len(steady) // 4)
    report = {
        "binary_sha256": binary_sha256,
        "duration_seconds": elapsed,
        "exit_code": return_code,
        "ticks_completed": ticks,
        "retained_ticks": retained_ticks,
        "rss_samples": len(samples),
        "database_cardinality_by_elapsed_seconds": cardinality_samples,
        "peak_rss_kib": max((rss for _, rss in samples), default=0),
        "steady_first_quarter_median_kib": statistics.median(steady[:window]) if steady else None,
        "steady_last_quarter_median_kib": statistics.median(steady[-window:]) if steady else None,
        "collector_timings_ms": {name: {"median": statistics.median(values), "max": max(values)}
                                 for name, values in sorted(phases.items())},
        "rss_kib_by_elapsed_seconds": samples,
        "scope": "live unprivileged host polling; isolated temporary XDG store; no eBPF attachment",
        "interpretation": "A bounded soak measures this workload; it does not prove absence of leaks.",
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: value for key, value in report.items()
                      if key not in ("rss_kib_by_elapsed_seconds", "database_cardinality_by_elapsed_seconds")}))
    return return_code


if __name__ == "__main__":
    raise SystemExit(main())
