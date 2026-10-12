#!/usr/bin/env python3
"""Summarize RSS windows from a completed watch measurement; never infer a leak cause."""
import argparse
import hashlib
import json
import statistics
from pathlib import Path


def analyze(path, window):
    raw = path.read_bytes()
    report = json.loads(raw)
    samples = report["rss_kib_by_elapsed_seconds"]
    if window <= 0 or not samples:
        raise ValueError("positive window and nonempty samples required")
    windows = []
    cardinalities = report.get("database_cardinality_by_elapsed_seconds", [])
    duration = report["duration_seconds"]
    for start in range(0, int(duration), window):
        values = [rss for elapsed, rss in samples if start <= elapsed < start + window]
        if values:
            windows.append({"start_seconds": start, "end_seconds": min(start + window, duration),
                            "samples": len(values), "median_rss_kib": statistics.median(values),
                            "minimum_rss_kib": min(values), "maximum_rss_kib": max(values)})
    for interval in windows:
        observed = [sample for sample in cardinalities
                    if sample.get("available") and interval["start_seconds"] <= sample["elapsed_seconds"]
                    and (sample["elapsed_seconds"] < interval["end_seconds"]
                         or interval["end_seconds"] == duration == sample["elapsed_seconds"])]
        interval["database_samples"] = len(observed)
        if observed:
            interval["database_first"] = observed[0]
            interval["database_last"] = observed[-1]
    return {"source": str(path), "source_sha256": hashlib.sha256(raw).hexdigest(),
            "binary_sha256": report.get("binary_sha256"), "duration_seconds": duration,
            "exit_code": report["exit_code"], "ticks_completed": report["ticks_completed"],
            "retained_ticks": report["retained_ticks"], "windows": windows,
            "interpretation": "Observed RSS windows only. Growth may reflect workload, retained evidence or allocator behavior; these measurements do not establish a leak or its absence."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("measurement", type=Path)
    parser.add_argument("--window-seconds", type=int, default=900)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        result = analyze(args.measurement, args.window_seconds)
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.error(str(error))
    output = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(output)
    else:
        print(output, end="")


if __name__ == "__main__":
    main()
