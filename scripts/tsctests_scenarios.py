#!/usr/bin/env python3
"""Record the inputs of TypeScript 7.1's tsc and tsc -b tests.

usage: tsctests_scenarios.py [--profile <name>] [--check]

tsgo's internal/execute/tsctests package declares each test as Go values
and edit closures, and its baselines are what one run of them writes. This
script copies the package within the pinned checkout (scripts/typescript7.py
setup) as internal/execute/tsrsdump, adds scripts/tsctests_dump/
tsrs_dump_test.go, and runs the one-shot tests (watch mode excluded) with
the recorder on: every scenario's files, arguments, environment and terminal,
and every edit's TestSys file operations, in order. The tests still compare
their baselines with the reference, so a recording is written only from a
run that reproduced all of them. The result is
vendor/typescript-native/<profile>/tsctests-scenarios.json; --check records
again and compares the bytes. The copy is removed afterwards.
"""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import typescript7  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
OVERLAY = ROOT / "scripts/tsctests_dump/tsrs_dump_test.go"
DEFAULT_PROFILE = "7.1.0-dev-19dadef8"
# The one-shot tests; watch mode is recorded with its own runner later.
RUN = "^(TestTsc|TestBuild|TestShowConfig|TestForceConsistentCasingInFileNames|TestGenerateTrace|TestTypeAcquisition)"
SKIP = "Watch"

# (file, anchor, text inserted after the anchor). Each anchor must occur once.
INSERTIONS = [
    ("runner.go", "\t\tsys := newTestSys(test, false)\n",
     "\t\tdump := tsrsDumpBegin(t, test, scenario)\n"),
]
REPLACEMENTS = [
    ("runner.go",
     "\t\t\t\tif do.edit != nil {\n\t\t\t\t\tdo.edit(sys)\n\t\t\t\t}\n",
     "\t\t\t\ttsrsDumpEdit(dump, sys, do)\n"),
    ("runner.go",
     "\t\t\t\tfor i := range index + 1 {\n\t\t\t\t\tif test.edits[i].edit != nil {\n"
     "\t\t\t\t\t\ttest.edits[i].edit(nonIncrementalSys)\n\t\t\t\t\t}\n\t\t\t\t}\n",
     "\t\t\t\tfor i := range index + 1 {\n"
     "\t\t\t\t\ttsrsDumpNonIncrementalEdit(dump, index, i, nonIncrementalSys, test.edits[i])\n"
     "\t\t\t\t}\n"),
    ("runner.go",
     "\t\tbaseline.Run(t, strings.ReplaceAll(test.subScenario",
     "\t\ttsrsDumpEnd(dump)\n\t\tbaseline.Run(t, strings.ReplaceAll(test.subScenario"),
]
HELPERS = {
    "func (s *TestSys) writeFileNoError(path string, content string) {\n":
        'tsrsOp{Op: "write", Path: path, tsrsText: tsrsTextOf(content)}',
    "func (s *TestSys) removeNoError(path string) {\n":
        'tsrsOp{Op: "remove", Path: path}',
    "func (s *TestSys) renameFileNoError(oldPath string, newPath string) {\n":
        'tsrsOp{Op: "rename", Path: oldPath, To: newPath}',
    "func (s *TestSys) replaceFileText(path string, oldText string, newText string) {\n":
        'tsrsOp{Op: "replace", Path: path, Old: tsrsString(oldText), New: tsrsString(newText)}',
    "func (s *TestSys) replaceFileTextAll(path string, oldText string, newText string) {\n":
        'tsrsOp{Op: "replaceAll", Path: path, Old: tsrsString(oldText), New: tsrsString(newText)}',
    "func (s *TestSys) appendFile(path string, text string) {\n":
        'tsrsOp{Op: "append", Path: path, tsrsText: tsrsTextOf(text)}',
    "func (s *TestSys) prependFile(path string, text string) {\n":
        'tsrsOp{Op: "prepend", Path: path, tsrsText: tsrsTextOf(text)}',
}


def patch(directory):
    def edit(name, old, new):
        path = directory / name
        text = path.read_text()
        if text.count(old) != 1:
            raise SystemExit(f"{name}: expected one {old!r}, found {text.count(old)}")
        path.write_text(text.replace(old, new))

    for name, anchor, insertion in INSERTIONS:
        edit(name, anchor, anchor + insertion)
    for name, old, new in REPLACEMENTS:
        edit(name, old, new)
    for signature, op in HELPERS.items():
        edit("sys.go", signature, signature + f"\tdefer tsrsRecord(s, {op})()\n")


def record(output):
    typescript7.check_source()
    # Go compares module paths after resolving links: use the real path.
    upstream = typescript7.UPSTREAM.resolve()
    source = upstream / "tsc/internal/execute/tsctests"
    copy = upstream / "tsc/internal/execute/tsrsdump"
    if copy.exists():
        shutil.rmtree(copy)
    copy.mkdir()
    try:
        for path in sorted(source.glob("*.go")):
            if path.name != "testmain_test.go":
                shutil.copyfile(path, copy / path.name)
        shutil.copyfile(OVERLAY, copy / OVERLAY.name)
        patch(copy)
        env = typescript7.environment()
        env["GOWORK"] = str(upstream / "go.work")
        env["TSRS_TSCTESTS_DUMP"] = str(output)
        command = ["go", "-C", str(upstream / "tsc"), "test",
                   "./internal/execute/tsrsdump", "-count=1", "-run", RUN, "-skip", SKIP]
        if sys.platform == "darwin" and shutil.which("taskpolicy"):
            command = ["taskpolicy", "-b", "nice", "-n", "15", *command]
        result = subprocess.run(command, env=env)
        if result.returncode != 0 or not output.is_file():
            raise SystemExit("the recording run failed")
    finally:
        shutil.rmtree(copy, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", default=DEFAULT_PROFILE)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    target = ROOT / "vendor/typescript-native" / args.profile / "tsctests-scenarios.json"
    with tempfile.TemporaryDirectory() as scratch:
        output = Path(scratch) / "scenarios.json"
        record(output)
        recorded = output.read_bytes()
    if args.check:
        if not target.is_file() or target.read_bytes() != recorded:
            raise SystemExit(f"{target}: differs from a new recording")
        print(f"{target}: matches a new recording")
        return 0
    target.write_bytes(recorded)
    print(f"{target}: {len(recorded)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
