#!/usr/bin/env python3
"""Record tsgo's language server transcripts from its fourslash tests.

usage: lsp_transcripts.py [--profile <name>] [--run <regexp>] [--output <path>]
                          [--from-raw] [--check]

tsgo's fourslash harness (internal/fourslash) drives an in-process server
through pipes with real JSON-RPC. This script copies the harness, its tests
and their util package, and internal/testutil/lsptestutil into
internal/tsrsfourslash within the pinned checkout (scripts/typescript7.py
setup), points their imports at the copies, drops the known-failing skips
(the transcripts record what tsgo answers, failing verification or not),
adds scripts/lsp_transcripts_dump/tsrs_record.go and runs the tests with the
recorder on: each harness's files and options, the messages both ways and
the harness's file operations, in order. Every failing test must be a known
failing one. The copy is removed afterwards.

The full transcripts go to target/lsp-transcripts/<profile>/raw.jsonl (a
check's to raw-check.jsonl), for looking at differences. The vendored scenarios,
vendor/typescript-native/<profile>/fourslash-transcripts.jsonl.gz (JSON
lines, gzip-compressed: the messages repeat a lot), keep per harness the
files, the inferred projects' options, the client's messages and the
harness's file operations in order, the harness's answers to
workspace/configuration and, on each request, the digest of tsgo's response
(the first 16 hex digits of the SHA-256 of the canonical JSON of its result
or error: keys sorted, no spaces, text as is; the arrays whose order tsgo
does not keep, UNORDERED, sorted by their elements' canonical JSON).

Two runs of the tests differ in order: tsgo's order of the UNORDERED arrays
changes from run to run (Go map iteration, concurrent searches), and some
tests send their requests and rename files in a Go map's order (in a few,
NONDETERMINISTIC, that order changes what tsgo answers). --check records
again and compares each harness's steps as a multiset, except for the
NONDETERMINISTIC tests. --from-raw rebuilds the scenarios from the last
raw transcripts without recording; --run limits the tests (and writes only
--output).
"""

import argparse
from collections import Counter
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import typescript7  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
RECORDER = ROOT / "scripts/lsp_transcripts_dump/tsrs_record.go"
DEFAULT_PROFILE = "7.1.0-dev-aa814927"
MODULE = "github.com/microsoft/TypeScript/tsc/internal/"
COPY = "tsrsfourslash"
SKIP = '\tt.Skip("Known failing fourslash test")\n'
IMPORTS = {
    f'"{MODULE}fourslash"': f'"{MODULE}{COPY}"',
    f'"{MODULE}fourslash/tests/util"': f'"{MODULE}{COPY}/tests/util"',
    f'"{MODULE}testutil/lsptestutil"': f'"{MODULE}{COPY}/lsptestutil"',
}
# (file, old, new): each old text must occur once.
REPLACEMENTS = [
    ("lsptestutil/lspclient.go",
     "func newClientTransport() clientTransport {\n",
     "func newClientTransport(rec *TsrsRecording) clientTransport {\n"),
    ("lsptestutil/lspclient.go",
     "\t\tserverOut:      lsp.ToWriter(serverToClientWriter),\n",
     "\t\tserverOut:      lsp.ToWriter(rec.wrap(serverToClientWriter, \"s\")),\n"),
    ("lsptestutil/lspclient.go",
     "\t\tclientOut:      lsp.ToWriter(clientToServerWriter),\n",
     "\t\tclientOut:      lsp.ToWriter(rec.wrap(clientToServerWriter, \"c\")),\n"),
    ("lsptestutil/lspclient.go",
     "onServerRequest ServerRequestHandler) (*LSPClient, func() error) {\n"
     "\ttransport := newClientTransport()\n",
     "onServerRequest ServerRequestHandler, rec *TsrsRecording) (*LSPClient, func() error) {\n"
     "\ttransport := newClientTransport(rec)\n"),
    ("lsptestutil/lspclient.go",
     "\t\tif err := g.Wait(); err != nil && !errors.Is(err, context.Canceled) {\n",
     "\t\terr := g.Wait()\n\t\trec.TsrsEnd()\n"
     "\t\tif err != nil && !errors.Is(err, context.Canceled) {\n"),
    ("fourslash.go",
     "\tcapabilities   *lsproto.ClientCapabilities\n",
     "\tcapabilities   *lsproto.ClientCapabilities\n\ttsrs           *lsptestutil.TsrsRecording\n"),
    ("fourslash.go",
     "\tclient, closeClient := lsptestutil.NewLSPClient(t, serverOpts, f.handleServerRequest)\n",
     "\tf.tsrs = lsptestutil.TsrsBegin(t, testfs, harnessOptions.CaseSensitivity == tspath.CaseSensitive, compilerOptions)\n"
     "\tclient, closeClient := lsptestutil.NewLSPClient(t, serverOpts, f.handleServerRequest, f.tsrs)\n"),
    ("fourslash.go",
     "\tif err := f.vfs.WriteFile(fileName, script.content); err != nil {\n",
     "\tf.tsrs.TsrsWrite(fileName.AsString(), script.content)\n"
     "\tif err := f.vfs.WriteFile(fileName, script.content); err != nil {\n"),
    ("fourslash.go",
     "\t\tif err := f.vfs.WriteFile(renamedFileName, content); err != nil {\n",
     "\t\tf.tsrs.TsrsWrite(renamedFileName.AsString(), content)\n"
     "\t\tif err := f.vfs.WriteFile(renamedFileName, content); err != nil {\n"),
    ("fourslash.go",
     "\tif err := f.vfs.Remove(oldPath); err != nil {\n",
     "\tf.tsrs.TsrsRemove(oldPath.AsString())\n"
     "\tif err := f.vfs.Remove(oldPath); err != nil {\n"),
]


def copy_packages(upstream):
    """The copies under internal/tsrsfourslash; the known failing tests."""
    internal = upstream / "tsc/internal"
    copy = internal / COPY
    if copy.exists():
        shutil.rmtree(copy)
    sources = [
        (internal / "fourslash", copy),
        (internal / "fourslash/tests", copy / "tests"),
        (internal / "fourslash/tests/util", copy / "tests/util"),
        (internal / "testutil/lsptestutil", copy / "lsptestutil"),
    ]
    known_failing = set()
    for source, target in sources:
        target.mkdir(parents=True)
        for path in sorted(source.glob("*.go")):
            text = path.read_text()
            for old, new in IMPORTS.items():
                text = text.replace(old, new)
            if SKIP in text:
                for match in re.finditer(r"func (Test\w+)\(t \*testing\.T\) \{\n" + re.escape(SKIP), text):
                    known_failing.add(match.group(1))
                text = text.replace(SKIP, "")
            (target / path.name).write_text(text)
    shutil.copyfile(RECORDER, copy / "lsptestutil" / RECORDER.name)
    for name, old, new in REPLACEMENTS:
        path = copy / name
        text = path.read_text()
        if text.count(old) != 1:
            raise SystemExit(f"{name}: expected one {old!r}, found {text.count(old)}")
        path.write_text(text.replace(old, new))
    return copy, known_failing


def record(raw, run):
    typescript7.check_source()
    # Go compares module paths after resolving links: use the real path.
    upstream = typescript7.UPSTREAM.resolve()
    copy, known_failing = copy_packages(upstream)
    try:
        env = typescript7.environment()
        env["TSRS_LSP_TRANSCRIPTS"] = str(raw)
        command = ["go", "-C", str(upstream / "tsc"), "test", f"./internal/{COPY}/tests",
                   "-count=1", "-json", "-timeout", "120m", "-parallel", "2"]
        if run:
            command += ["-run", run]
        if sys.platform == "darwin" and shutil.which("taskpolicy"):
            command = ["taskpolicy", "-b", "nice", "-n", "15", *command]
        result = subprocess.run(command, env=env, stdout=subprocess.PIPE, text=True)
    finally:
        shutil.rmtree(copy, ignore_errors=True)
    failed = set()
    build_output = []
    for line in result.stdout.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            build_output.append(line)
            continue
        if event.get("Action") == "fail" and event.get("Test"):
            failed.add(event["Test"].split("/")[0])
        if event.get("Action") == "output" and not event.get("Test"):
            build_output.append(event.get("Output", "").rstrip("\n"))
    unexpected = sorted(failed - known_failing)
    if unexpected:
        raise SystemExit("tests failed that tsgo does not skip: " + ", ".join(unexpected[:20]))
    if not raw.is_file():
        print("\n".join(build_output[-40:]), file=sys.stderr)
        raise SystemExit("the recording run wrote nothing")
    return known_failing, failed


# The arrays of a response whose order tsgo does not keep, by request
# method: paths from the response's result ("*" for each value of an
# object). Completions come from symbol tables (Go maps), and the searches
# over several projects (crossproject.go) combine the projects' results in
# a sync map's order.
UNORDERED = {
    "textDocument/completion": [("items",)],
    "textDocument/references": [()],
    "textDocument/implementation": [()],
    "textDocument/rename": [("documentChanges",), ("changes", "*")],
    "callHierarchy/incomingCalls": [()],
    "codeLens/resolve": [("command", "arguments", 2)],
    "workspace/willRenameFiles": [("documentChanges",), ("changes", "*")],
}
# Tests whose requests depend on the order the harness iterates a Go map:
# --check does not compare them.
NONDETERMINISTIC = {
    # The harness resolves the first completion of a name that two exports
    # share; their order changes.
    "TestCompletionsImport_jsModuleExportsAssignment",
    # The harness pulls the files' diagnostics in a map's order, and what
    # checking finds in these JavaScript files depends on that order.
    "TestFindAllRefs_importType_js1",
    "TestFindAllRefs_importType_js2",
    # Two module files share a path completion's name (repeated.ts and
    # repeated.d.ts); which one's extension tsgo keeps changes.
    "TestCompletionForStringLiteralNonrelativeImport2",
}


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def sort_arrays(value, path):
    """`value` with the arrays at `path` sorted by canonical JSON."""
    if not path:
        return sorted(value, key=canonical) if isinstance(value, list) else value
    part, rest = path[0], path[1:]
    if part == "*" and isinstance(value, dict):
        return {key: sort_arrays(item, rest) for key, item in value.items()}
    if isinstance(part, str) and isinstance(value, dict) and part in value:
        return {**value, part: sort_arrays(value[part], rest)}
    if isinstance(part, int) and isinstance(value, list) and part < len(value):
        return [sort_arrays(item, rest) if index == part else item for index, item in enumerate(value)]
    return value


def digest(method, response):
    """The digest of a response's result or error."""
    value = {key: response[key] for key in ("result", "error") if key in response}
    if "result" in value:
        for path in UNORDERED.get(method, []):
            value["result"] = sort_arrays(value["result"], path)
    return hashlib.sha256(canonical(value).encode()).hexdigest()[:16]


def scenario(recording):
    """The vendored scenario of one raw transcript."""
    steps = []
    configuration = []
    pending = {}
    server_requests = {}
    for event in recording["events"]:
        if "c" in event:
            message = event["c"]
            if "method" in message:
                step = {"send": message}
                if "id" in message:
                    pending[json.dumps(message["id"])] = step
                steps.append(step)
            else:
                method = server_requests.pop(json.dumps(message.get("id")), None)
                if method == "workspace/configuration":
                    configuration.append(message.get("result"))
        elif "s" in event:
            message = event["s"]
            if "method" in message:
                if "id" in message:
                    server_requests[json.dumps(message["id"])] = message["method"]
            elif "id" in message:
                step = pending.pop(json.dumps(message["id"]), None)
                if step is not None:
                    step["expect"] = digest(step["send"]["method"], message)
        else:
            steps.append(event)
    return {
        "name": recording["name"],
        "caseSensitive": recording["caseSensitive"],
        "files": recording["files"],
        "inferredOptions": recording["inferredOptions"],
        "steps": steps,
        "configuration": configuration,
    }


def comparable(line):
    """A scenario as --check compares it: its steps as a multiset, the
    requests without their IDs and a watch notification's changes in any
    order (the harness renames files in a map's order)."""
    scenario = json.loads(line)
    steps = Counter()
    for step in scenario.pop("steps"):
        if "send" in step:
            message = dict(step["send"])
            message.pop("id", None)
            if message["method"] == "workspace/didChangeWatchedFiles":
                changes = sorted(message["params"]["changes"], key=canonical)
                message["params"] = {**message["params"], "changes": changes}
            step = {**step, "send": message}
        steps[canonical(step)] += 1
    return canonical(scenario), steps


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", default=DEFAULT_PROFILE)
    parser.add_argument("--run")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--from-raw", action="store_true")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    target = args.output or ROOT / "vendor/typescript-native" / args.profile / "fourslash-transcripts.jsonl.gz"
    if args.run and not args.output:
        raise SystemExit("--run writes only --output")
    raw_target = ROOT / "target/lsp-transcripts" / args.profile / "raw.jsonl"
    if args.from_raw:
        recordings = [json.loads(line) for line in raw_target.read_text().splitlines()]
    else:
        with tempfile.TemporaryDirectory() as scratch:
            raw = Path(scratch) / "raw.jsonl"
            known_failing, failed = record(raw, args.run)
            recordings = [json.loads(line) for line in raw.read_text().splitlines()]
            # A check keeps its transcripts beside the last ones.
            kept = raw_target.with_name("raw-check.jsonl") if args.check else raw_target
            kept.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(raw, kept)
        print(f"{len(recordings)} transcripts; {len(failed)} known failing tests failed "
              f"({len(known_failing)} known failing)")
    recordings.sort(key=lambda recording: recording["name"])
    lines = [json.dumps(scenario(recording), sort_keys=True, separators=(",", ":"), ensure_ascii=False)
             for recording in recordings]
    recorded = ("\n".join(lines) + "\n").encode()
    if args.check:
        if not target.is_file():
            raise SystemExit(f"{target}: missing")
        vendored = gzip.decompress(target.read_bytes()).decode().splitlines()
        if len(vendored) != len(lines):
            raise SystemExit(f"{target}: {len(vendored)} scenarios, a new recording has {len(lines)}")
        differing = [json.loads(line)["name"] for line, new in zip(vendored, lines)
                     if json.loads(line)["name"] not in NONDETERMINISTIC
                     and comparable(line) != comparable(new)]
        if differing:
            raise SystemExit(f"{target}: {len(differing)} scenarios differ from a new recording: "
                             + ", ".join(differing[:20]))
        print(f"{target}: matches a new recording")
        return 0
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(gzip.compress(recorded, compresslevel=9, mtime=0))
    print(f"{target}: {len(recorded)} bytes, {target.stat().st_size} compressed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
