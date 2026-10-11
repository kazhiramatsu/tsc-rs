#!/usr/bin/env python3
"""Encode source files with TypeScript 7.1's API encoder, for tsc-rs's tests.

usage: api_encoder_dump.py fixtures [--check]
       api_encoder_dump.py corpus <out dir> [--profile <name>]

fixtures: encodes each source of crates/api/tests/fixtures/tsgo/ under the
name `/<file name>` and writes tsgo's bytes as `<file name>.hex` beside it
(crates/api/tests/tsgo_fixtures.rs compares tsc-rs's encoding with them);
--check encodes again and compares instead of writing.

corpus: encodes every compiler and conformance case file of the vendored
profile, each as one file named `/<suite>/<path>`, into
<out dir>/<index>.bin with the list <out dir>/corpus.tsv, the input of
crates/api/tests/tsgo_corpus.rs.

The tool (scripts/api_encoder_dump/main.go) is copied into the pinned
checkout (scripts/typescript7.py setup) as tsc/cmd/tsrsencdump, built with
the checkout's toolchain, and removed again.
"""

import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import typescript7  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
TOOL = ROOT / "scripts/api_encoder_dump/main.go"
FIXTURES = ROOT / "crates/api/tests/fixtures/tsgo"
DEFAULT_PROFILE = "7.1.0-dev-aa814927"
HEX_LINE = 32


def build(scratch):
    typescript7.check_source()
    upstream = typescript7.UPSTREAM.resolve()
    command_dir = upstream / "tsc/cmd/tsrsencdump"
    if command_dir.exists():
        shutil.rmtree(command_dir)
    command_dir.mkdir()
    binary = scratch / "tsrsencdump"
    try:
        shutil.copyfile(TOOL, command_dir / "main.go")
        command = ["go", "-C", str(upstream / "tsc"), "build", "-o", str(binary), "./cmd/tsrsencdump"]
        if sys.platform == "darwin" and shutil.which("taskpolicy"):
            command = ["taskpolicy", "-b", "nice", "-n", "15", *command]
        env = typescript7.environment()
        # Go compares module paths after resolving links: use the real path.
        env["GOWORK"] = str(upstream / "go.work")
        subprocess.run(command, env=env, check=True)
    finally:
        shutil.rmtree(command_dir, ignore_errors=True)
    return binary


def encode(binary, rows, out):
    """rows: (index, file name, path on disk)."""
    out.mkdir(parents=True, exist_ok=True)
    listing = out / "corpus.tsv"
    listing.write_text("".join(f"{index}\t{name}\t{path}\n" for index, name, path in rows))
    subprocess.run([str(binary), str(listing), str(out)], check=True)
    return listing


def to_hex(data):
    text = data.hex()
    width = HEX_LINE * 2
    return "".join(text[i:i + width] + "\n" for i in range(0, len(text), width))


def fixtures(check):
    sources = sorted(path for path in FIXTURES.iterdir()
                     if path.is_file() and path.suffix != ".hex" and path.name != "README.md")
    with tempfile.TemporaryDirectory() as scratch:
        scratch = Path(scratch)
        binary = build(scratch)
        rows = [(index, f"/{path.name}", path) for index, path in enumerate(sources)]
        encode(binary, rows, scratch / "out")
        stale = []
        for index, path in enumerate(sources):
            expected = to_hex((scratch / "out" / f"{index}.bin").read_bytes())
            target = path.with_name(path.name + ".hex")
            if check:
                if not target.exists() or target.read_text() != expected:
                    stale.append(target.name)
            else:
                target.write_text(expected)
        if stale:
            raise SystemExit(f"fixtures differ from tsgo's encoding: {stale}")
    print(f"{len(sources)} fixtures {'match' if check else 'written'}")


def corpus(out, profile):
    cases = ROOT / "vendor/typescript-native" / profile / "upstream/tsc/testdata/tests/cases"
    rows = []
    for suite in ("compiler", "conformance"):
        for path in sorted((cases / suite).rglob("*")):
            if path.is_file():
                rows.append((f"/{path.relative_to(cases)}", path.resolve()))
    rows.sort()
    with tempfile.TemporaryDirectory() as scratch:
        binary = build(Path(scratch))
        encode(binary, [(index, name, path) for index, (name, path) in enumerate(rows)], out)
    print(f"{len(rows)} files encoded into {out}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    fixture_parser = commands.add_parser("fixtures")
    fixture_parser.add_argument("--check", action="store_true")
    corpus_parser = commands.add_parser("corpus")
    corpus_parser.add_argument("out", type=Path)
    corpus_parser.add_argument("--profile", default=DEFAULT_PROFILE)
    args = parser.parse_args()
    if args.command == "fixtures":
        fixtures(args.check)
    else:
        corpus(args.out.resolve(), args.profile)


if __name__ == "__main__":
    sys.exit(main())
