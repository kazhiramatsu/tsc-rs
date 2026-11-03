#!/usr/bin/env python3
"""Fingerprint the parser's and binder's output over the test corpora, or
compare two fingerprint runs.

usage: syntax_fingerprint.py [--corpus conformance|bench|all] [--out FILE]
                             [--binary PATH] [--bench-root DIR] [--profile NAME]
                             [--compare BASELINE [--against CURRENT]] [--list]

Runs crates/binder/examples/syntax_fingerprint.rs (build it first:
`nice -n 20 cargo build --release -j 1 -p tsc-rs-binder --example syntax_fingerprint`)
over (a) the vendored conformance corpus (the bundled libraries, the test
library and every compiler and conformance case file, each read whole) and (b)
the source files of the eight bench projects under --bench-root (their
directories, without node_modules and hidden directories), one process at a
time, and writes one TSV: a `#` header, then per input and JSDoc mode
`path TAB mode TAB parse_hash TAB bind_hash TAB node_count TAB diagnostics_hash`.

--compare BASELINE runs the same corpora as the baseline recorded (unless
--corpus says otherwise), writes the run to --out and prints every input
whose row differs from the baseline's, or is missing from either; it exits 1
on any difference. With --against CURRENT it compares two existing files and
runs nothing. --list prints the inputs per corpus and runs nothing.
"""

import argparse
import datetime
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PROFILE = "7.1.0-dev-aa814927"
DEFAULT_BINARY = ROOT / "target/release/examples/syntax_fingerprint"
DEFAULT_OUT = ROOT / "target/syntax-fingerprint/fingerprints.tsv"
DEFAULT_BENCH_ROOT = Path.home() / "dev/bench-corpora"
# The eight comparison projects: the directory each is walked from.
BENCH_PROJECTS = [
    "hono",
    "zod",
    "effect.noindex/packages/effect",
    "TypeScript-6.0/src/compiler",
    "next.js/packages/next",
    "vscode/src",
    "playwright",
    "vue",
]
# The vendored profile's directories the binary's --conformance walks.
PROFILE_DIRECTORIES = [
    "tsc/internal/bundled/libs",
    "tsc/testdata/tests/lib",
    "tsc/testdata/tests/cases/compiler",
    "tsc/testdata/tests/cases/conformance",
]
SOURCE_EXTENSIONS = (".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs")
COLUMNS = ("parse_hash", "bind_hash", "node_count", "diagnostics_hash")


def bench_files(project_root):
    """The project's source files, sorted, without node_modules and hidden
    directories."""
    files = []
    for directory, subdirectories, names in os.walk(project_root):
        subdirectories[:] = sorted(
            name for name in subdirectories if name != "node_modules" and not name.startswith(".")
        )
        files += [
            os.path.join(directory, name) for name in names if name.lower().endswith(SOURCE_EXTENSIONS)
        ]
    return sorted(files)


def run_binary(binary, arguments, stdin_text=None):
    """The rows the fingerprint binary prints, run at the lowest priority."""
    command = ["nice", "-n", "20", str(binary), *arguments]
    result = subprocess.run(
        command, input=stdin_text.encode() if stdin_text is not None else None,
        stdout=subprocess.PIPE, cwd=ROOT,
    )
    if result.returncode != 0:
        raise SystemExit(f"{' '.join(command)} exited with {result.returncode}")
    return result.stdout.decode().splitlines()


def run(args, corpora):
    if not args.binary.exists():
        raise SystemExit(
            f"{args.binary} does not exist: "
            "nice -n 20 cargo build --release -j 1 -p tsc-rs-binder --example syntax_fingerprint"
        )
    rows = []
    if "conformance" in corpora:
        print("conformance ...", file=sys.stderr, flush=True)
        rows += run_binary(args.binary, ["--profile", args.profile, "--conformance"])
    if "bench" in corpora:
        for project in BENCH_PROJECTS:
            files = bench_files(args.bench_root / project)
            print(f"bench {project}: {len(files)} files ...", file=sys.stderr, flush=True)
            output = run_binary(
                args.binary, ["--files", "-", "--root", str(args.bench_root)], "\n".join(files) + "\n"
            )
            rows += [f"bench/{row}" for row in output]
    return rows


def header(args, corpora):
    head = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL
    ).stdout.decode().strip()
    dirty = subprocess.run(
        ["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT, stdout=subprocess.PIPE
    ).stdout.decode().strip()
    return [
        f"# syntax fingerprints; corpora={','.join(corpora)}; profile={args.profile}",
        f"# source={head}{' (with uncommitted changes)' if dirty else ''}; "
        f"written={datetime.datetime.now().isoformat(timespec='seconds')}",
        "# path\tmode\t" + "\t".join(COLUMNS),
    ]


def read_rows(path):
    """(corpora from the header, {(path, mode): columns}) of a fingerprint file."""
    corpora, rows = None, {}
    for line in Path(path).read_text().splitlines():
        if line.startswith("#"):
            if "corpora=" in line:
                corpora = line.split("corpora=", 1)[1].split(";", 1)[0].split(",")
            continue
        fields = line.split("\t")
        if len(fields) != 2 + len(COLUMNS):
            raise SystemExit(f"{path}: malformed row {line!r}")
        rows[(fields[0], fields[1])] = tuple(fields[2:])
    return corpora, rows


def compare(baseline_path, current_path):
    """Print the differences; whether there were none."""
    _, baseline = read_rows(baseline_path)
    _, current = read_rows(current_path)
    differing = []
    for key in sorted(baseline.keys() | current.keys()):
        old, new = baseline.get(key), current.get(key)
        if old == new:
            continue
        if old is None:
            differing.append(f"{key[0]}\t{key[1]}\tonly in {current_path}")
        elif new is None:
            differing.append(f"{key[0]}\t{key[1]}\tonly in {baseline_path}")
        else:
            columns = [
                f"{name} {a} -> {b}" for name, a, b in zip(COLUMNS, old, new) if a != b
            ]
            differing.append(f"{key[0]}\t{key[1]}\t{'; '.join(columns)}")
    for line in differing:
        print(line)
    files = len({line.split("\t", 1)[0] for line in differing})
    print(
        f"{len(differing)} differing rows in {files} inputs; {len(baseline)} baseline rows, {len(current)} current rows",
        file=sys.stderr,
    )
    return not differing


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--corpus", choices=("conformance", "bench", "all"))
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY)
    parser.add_argument("--bench-root", type=Path, default=DEFAULT_BENCH_ROOT)
    parser.add_argument("--profile", default=DEFAULT_PROFILE)
    parser.add_argument("--compare", type=Path, metavar="BASELINE")
    parser.add_argument("--against", type=Path, metavar="CURRENT")
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()

    if args.against and not args.compare:
        raise SystemExit("--against needs --compare")
    if args.compare and args.against:
        return 0 if compare(args.compare, args.against) else 1

    if args.corpus:
        corpora = ["conformance", "bench"] if args.corpus == "all" else [args.corpus]
    elif args.compare:
        corpora = read_rows(args.compare)[0] or ["conformance", "bench"]
    else:
        corpora = ["conformance", "bench"]

    if args.list:
        if "bench" in corpora:
            for project in BENCH_PROJECTS:
                files = bench_files(args.bench_root / project)
                print(f"bench/{project}\t{len(files)} files")
        if "conformance" in corpora:
            profile_root = ROOT / "vendor/typescript-native" / args.profile / "upstream"
            for directory in PROFILE_DIRECTORIES:
                print(f"conformance/{directory}\t{len(bench_files(profile_root / directory))} files")
        return 0

    rows = run(args, corpora)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("\n".join(header(args, corpora) + rows) + "\n")
    panics = sum(1 for row in rows if row.endswith("\tpanic"))
    print(f"wrote {args.out}: {len(rows)} rows, {panics} panics", file=sys.stderr)
    if args.compare:
        return 0 if compare(args.compare, args.out) else 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
