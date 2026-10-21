#!/usr/bin/env python3
"""Build or verify bundled read-only probes without installing a toolchain."""
import argparse
import hashlib
import pathlib
import subprocess
import tempfile

root = pathlib.Path(__file__).resolve().parents[1]
source = root / "crates/twin-ebpf/probes/twin.bpf.c"
output = source.with_suffix(".o")
parser = argparse.ArgumentParser()
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
with tempfile.TemporaryDirectory(prefix="twin-bpf-") as directory:
    candidate = pathlib.Path(directory) / "twin.bpf.o"
    subprocess.run(["clang", "-target", "bpfel", "-O2", "-g", "-fno-ident", "-ffile-prefix-map=" + str(root) + "=.", "-c", str(source), "-o", str(candidate)], check=True, cwd=root)
    compiler = subprocess.run(["clang", "--version"], check=True, text=True, capture_output=True).stdout.splitlines()[0]
    manifest = "compiler " + compiler + "\nsource " + hashlib.sha256(source.read_bytes()).hexdigest() + "\nobject " + hashlib.sha256(candidate.read_bytes()).hexdigest() + "\n"
    if args.check:
        recorded = source.with_suffix(".sha256").read_text().splitlines()
        if recorded and recorded[0] != "compiler " + compiler:
            raise SystemExit("cannot verify byte-for-byte: the bundled object was built with " + recorded[0].removeprefix("compiler ") + ", this host has " + compiler)
        if candidate.read_bytes() != output.read_bytes() or source.with_suffix(".sha256").read_text() != manifest:
            raise SystemExit("bundled eBPF object or source manifest is stale; rebuild with tools/build_ebpf.py")
    else:
        output.write_bytes(candidate.read_bytes())
        source.with_suffix(".sha256").write_text(manifest)
print(output.relative_to(root))
