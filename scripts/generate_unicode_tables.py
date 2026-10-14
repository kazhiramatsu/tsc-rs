#!/usr/bin/env python3
"""Generate crates/syntax/src/go_unicode/tables.rs, the Unicode tables of
Go's packages tsgo reads (unicode and golang.org/x/text/unicode/norm) for
the organize imports comparers, the auto-import index and vfsmatch.

usage: generate_unicode_tables.py [--check]

The tool (scripts/unicode_tables_dump/main.go) is copied into the pinned
checkout (scripts/typescript7.py setup) as tsc/cmd/tsrsunicodetables, built
with the checkout's toolchain and modules, and removed again. --check
generates into a temporary file and compares instead of writing.
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
TOOL = ROOT / "scripts/unicode_tables_dump/main.go"
OUTPUT = ROOT / "crates/syntax/src/go_unicode/tables.rs"


def generate(output):
    typescript7.check_source()
    upstream = typescript7.UPSTREAM.resolve()
    command_dir = upstream / "tsc/cmd/tsrsunicodetables"
    if command_dir.exists():
        shutil.rmtree(command_dir)
    command_dir.mkdir()
    try:
        shutil.copyfile(TOOL, command_dir / "main.go")
        command = ["go", "-C", str(upstream / "tsc"), "run", "./cmd/tsrsunicodetables", str(output)]
        if sys.platform == "darwin" and shutil.which("taskpolicy"):
            command = ["taskpolicy", "-b", "nice", "-n", "15", *command]
        env = typescript7.environment()
        # Go compares module paths after resolving links: use the real path.
        env["GOWORK"] = str(upstream / "go.work")
        subprocess.run(command, env=env, check=True)
    finally:
        shutil.rmtree(command_dir, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if not args.check:
        generate(OUTPUT)
        return 0
    with tempfile.TemporaryDirectory() as scratch:
        generated = Path(scratch) / "unicode_tables.rs"
        generate(generated)
        if generated.read_bytes() != OUTPUT.read_bytes():
            print(f"{OUTPUT.relative_to(ROOT)} differs from the generated tables", file=sys.stderr)
            return 1
    print("unicode tables up to date")
    return 0


if __name__ == "__main__":
    sys.exit(main())
