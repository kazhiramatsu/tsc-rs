#!/usr/bin/env python3
"""The JSDoc corpus (tests/jsdoc-corpus): sample a case's requests, refresh its
baselines from the vendored tsgo, or compare a server with them.

usage: jsdoc_corpus.py init <case> [--positions N]
       jsdoc_corpus.py update [<case> ...] [--tsgo <binary>]
       jsdoc_corpus.py check [<case> ...] [--server <binary>] [--dump <dir>]

`init` writes <case>/requests.jsonl from the case's sources: per file the
diagnostics, document symbols and semantic tokens, and at a spread of
identifiers hover, definition, type definition and document highlights, at
fewer of them references, after `(` signature help (the differential
recorder's sampling, lsp-compare.py, without completions). Edit the file by
hand to add the positions a difference was found at; the ids are
`<file>:<line>:<character>:<method>`.

`update` runs tsgo (default target/typescript7/bin/tsc-<commit> of
scripts/typescript7.py's pin) on each case — `tsc -p . --pretty false` into
expected/cli.txt, the requests into expected/lsp.jsonl — and is the only
step that needs tsgo. `check` runs another server (default
target/release/tsc-rs) the same way and reports the ids whose answers
differ, honouring <case>/known.txt exactly as the Rust test
crates/cli/tests/jsdoc_corpus.rs does (that test is the gate; this is the
same comparison from Python, for a quick look or with --dump).

An answer is normalized as the recorder does: {"result": …} or {"error":
message}, the arrays tsgo does not order sorted by their canonical JSON, the
case's absolute directory replaced by ${ROOT}, keys sorted, compact JSON.
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "tests" / "jsdoc-corpus"
ENCODING = "utf-16"

UNORDERED = {
    "textDocument/completion": [("items",)],
    "textDocument/references": [()],
    "textDocument/implementation": [()],
    "textDocument/rename": [("documentChanges",), ("changes", "*")],
    "callHierarchy/incomingCalls": [()],
    "codeLens/resolve": [("command", "arguments", 2)],
    "workspace/willRenameFiles": [("documentChanges",), ("changes", "*")],
    "textDocument/definition": [()],
    "textDocument/typeDefinition": [()],
    "textDocument/documentHighlight": [()],
}


def tsgo_default():
    text = (ROOT / "scripts" / "typescript7.py").read_text()
    for line in text.splitlines():
        if line.startswith("PIN = "):
            pin = line.split('"')[1]
            return ROOT / "target" / "typescript7" / "bin" / f"tsc-{pin[:8]}"
    raise SystemExit("scripts/typescript7.py has no PIN")


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def sort_arrays(value, path):
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


def normalize(method, response, root):
    if "error" in response:
        normalized = {"error": response["error"].get("message")}
    else:
        result = response.get("result")
        for path in UNORDERED.get(method, []):
            result = sort_arrays(result, path)
        normalized = {"result": result}
    return canonical(normalized).replace(file_uri(root), "file://${ROOT}").replace(str(root), "${ROOT}")


def file_uri(path):
    return "file://" + quote(str(path))


def language_id(name):
    name = str(name)
    if name.endswith((".js", ".cjs", ".mjs", ".jsx")):
        return "javascript"
    if name.endswith(".json"):
        return "json"
    return "typescript"


class Server:
    """A language server over stdio, driven as the recorder drives one."""

    def __init__(self, binary, root):
        self.proc = subprocess.Popen(
            [str(binary), "--lsp", "--stdio"], cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL
        )
        self.next_id = 1

    def send(self, message):
        if message.get("params", 0) is None:
            message = {key: value for key, value in message.items() if key != "params"}
        body = json.dumps(message).encode()
        self.proc.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
        self.proc.stdin.flush()

    def read(self):
        length = None
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise EOFError("the server closed its output")
            line = line.strip()
            if not line:
                break
            name, _, value = line.decode().partition(":")
            if name.lower() == "content-length":
                length = int(value.strip())
        return json.loads(self.proc.stdout.read(length))

    def answer(self, message):
        if "id" not in message:
            return
        if message.get("method") == "workspace/configuration":
            result = [None] * len(message["params"].get("items", []))
        else:
            result = None
        self.send({"jsonrpc": "2.0", "id": message["id"], "result": result})

    def request(self, method, params):
        request_id = self.next_id
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        while True:
            message = self.read()
            if "method" in message:
                self.answer(message)
            elif message.get("id") == request_id:
                return message

    def notify(self, method, params):
        self.send({"jsonrpc": "2.0", "method": method, "params": params})

    def close(self):
        try:
            self.request("shutdown", None)
            self.notify("exit", None)
            self.proc.stdin.close()
        except (EOFError, BrokenPipeError):
            pass
        try:
            self.proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.proc.kill()


def initialize_params(root):
    params = json.load(open(CORPUS / "initialize.json"))
    params["capabilities"]["general"]["positionEncodings"] = [ENCODING]
    params["capabilities"]["workspace"]["workspaceFolders"] = True
    params["rootUri"] = file_uri(root)
    params["workspaceFolders"] = [{"uri": file_uri(root), "name": root.name}]
    params["processId"] = os.getpid()
    return params


def scan(text):
    """Identifier starts and `(` call positions outside comments and strings."""
    identifiers, calls = [], []
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c == "/" and text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
            continue
        if c == "/" and text.startswith("/*", i):
            j = text.find("*/", i + 2)
            i = n if j < 0 else j + 2
            continue
        if c in "'\"`":
            j = i + 1
            while j < n and text[j] != c and (c == "`" or text[j] != "\n"):
                j += 2 if text[j] == "\\" else 1
            i = j + 1
            continue
        if c.isalpha() or c in "_$":
            j = i + 1
            while j < n and (text[j].isalnum() or text[j] in "_$"):
                j += 1
            identifiers.append(i)
            if j < n and text[j] == "(":
                calls.append(j + 1)
            i = j
            continue
        i += 1
    return identifiers, calls


def spread(items, count):
    if count <= 0 or not items:
        return []
    if len(items) <= count:
        return items
    step = len(items) / count
    return [items[int(k * step)] for k in range(count)]


class Positions:
    def __init__(self, text):
        self.text = text
        self.line_starts = [0]
        for index, ch in enumerate(text):
            if ch == "\n":
                self.line_starts.append(index + 1)

    def at(self, offset):
        import bisect

        line = bisect.bisect_right(self.line_starts, offset) - 1
        segment = self.text[self.line_starts[line]:offset]
        return {"line": line, "character": len(segment.encode("utf-16-le")) // 2}


def case_files(case):
    """The case's source files, in a stable order, tsconfig.json first."""
    names = sorted(
        str(path.relative_to(case))
        for path in case.rglob("*")
        if path.is_file()
        and path.suffix in (".js", ".cjs", ".mjs", ".jsx", ".ts", ".cts", ".mts", ".tsx")
        and "node_modules" not in path.parts
        and "expected" not in path.parts
    )
    return names


def read_requests(case):
    requests = []
    for line in (case / "requests.jsonl").read_text().splitlines():
        if line.strip():
            requests.append(json.loads(line))
    return requests


def init_case(case, positions_per_file):
    lines = []
    for relative in case_files(case):
        text = (case / relative).read_bytes().decode("utf-8-sig")
        positions = Positions(text)

        def add(id_suffix, method, params):
            lines.append(json.dumps({"id": f"{relative}:{id_suffix}", "file": relative, "method": method, "params": params}, ensure_ascii=False))

        add("diagnostic", "textDocument/diagnostic", {})
        add("documentSymbol", "textDocument/documentSymbol", {})
        add("semanticTokens", "textDocument/semanticTokens/full", {})
        identifiers, calls = scan(text)
        for offset in spread(identifiers, positions_per_file):
            position = positions.at(offset)
            where = f"{position['line']}:{position['character']}"
            for method in ("hover", "definition", "typeDefinition", "documentHighlight"):
                add(f"{where}:{method}", f"textDocument/{method}", {"position": position})
        for offset in spread(identifiers, max(1, positions_per_file // 5)):
            position = positions.at(offset)
            add(f"{position['line']}:{position['character']}:references", "textDocument/references", {"position": position, "context": {"includeDeclaration": True}})
        for offset in spread(calls, max(1, positions_per_file // 4)):
            position = positions.at(offset)
            add(f"{position['line']}:{position['character']}:signatureHelp", "textDocument/signatureHelp", {"position": position, "context": {"triggerKind": 1, "isRetrigger": False}})
    (case / "requests.jsonl").write_text("\n".join(lines) + "\n")
    print(f"{case.name}: {len(lines)} requests")


def run_case(binary, case):
    """tsc -p . --pretty false and every request: (cli text, {id: normalized answer})."""
    root = case.resolve()
    cli = subprocess.run([str(binary), "-p", ".", "--pretty", "false"], cwd=root, capture_output=True, text=True)
    cli_text = (cli.stdout + cli.stderr).replace("\r\n", "\n").replace(str(root), "${ROOT}")
    cli_text = cli_text.rstrip("\n") + f"\nexit {cli.returncode}\n"
    server = Server(binary, root)
    server.request("initialize", initialize_params(root))
    server.notify("initialized", {})
    opened = set()
    answers = {}
    for request in read_requests(case):
        path = root / request["file"]
        if request["file"] not in opened:
            opened.add(request["file"])
            server.notify(
                "textDocument/didOpen",
                {"textDocument": {"uri": file_uri(path), "languageId": language_id(path), "version": 1, "text": path.read_bytes().decode("utf-8-sig")}},
            )
        params = dict(request.get("params") or {})
        params.setdefault("textDocument", {"uri": file_uri(path)})
        response = server.request(request["method"], params)
        answers[request["id"]] = normalize(request["method"], response, root)
    server.close()
    return cli_text, answers


def read_known(case):
    known = {}
    path = case / "known.txt"
    if not path.exists():
        return known
    for line in path.read_text().splitlines():
        body = line.split("#", 1)[0].strip()
        if body:
            known[body] = line.split("#", 1)[1].strip() if "#" in line else ""
    return known


def cases_of(names):
    if names:
        return [CORPUS / name for name in names]
    return sorted(path for path in CORPUS.iterdir() if path.is_dir() and (path / "requests.jsonl").exists())


def check_membership(binary, case):
    """Every source file of the case is in its project (an outside file would
    open as an inferred project, whose automatic type acquisition makes the
    answers depend on the machine)."""
    root = case.resolve()
    listed = subprocess.run([str(binary), "-p", ".", "--noEmit", "--listFiles"], cwd=root, capture_output=True, text=True).stdout
    in_project = {line.strip() for line in listed.splitlines() if line.startswith(str(root) + "/")}
    outside = [name for name in case_files(case) if str(root / name) not in in_project]
    if outside:
        raise SystemExit(f"{case.name}: not in the project (add them to tsconfig.json or remove them): {' '.join(outside)}")


def update(args):
    binary = Path(args.tsgo).resolve() if args.tsgo else tsgo_default()
    if not binary.exists():
        raise SystemExit(f"tsgo binary not found: {binary} (scripts/typescript7.py builds it)")
    for case in cases_of(args.cases):
        check_membership(binary, case)
        cli_text, answers = run_case(binary, case)
        (case / "expected").mkdir(exist_ok=True)
        (case / "expected" / "cli.txt").write_text(cli_text)
        with open(case / "expected" / "lsp.jsonl", "w") as out:
            for request in read_requests(case):
                out.write(json.dumps({"id": request["id"], "answer": answers[request["id"]]}, ensure_ascii=False) + "\n")
        print(f"{case.name}: {len(answers)} answers, cli exit {cli_text.rstrip().rsplit(' ', 1)[-1]}")


def check(args):
    binary = Path(args.server).resolve() if args.server else ROOT / "target" / "release" / "tsc-rs"
    failures = 0
    for case in cases_of(args.cases):
        expected_cli = (case / "expected" / "cli.txt").read_text()
        expected = {}
        for line in (case / "expected" / "lsp.jsonl").read_text().splitlines():
            if line.strip():
                row = json.loads(line)
                expected[row["id"]] = row["answer"]
        cli_text, answers = run_case(binary, case)
        known = read_known(case)
        differing = [id_ for id_, answer in answers.items() if expected.get(id_) != answer]
        if cli_text != expected_cli:
            differing.insert(0, "cli")
        unexpected = [id_ for id_ in differing if id_ not in known]
        stale = [id_ for id_ in known if id_ not in differing]
        status = "ok" if not unexpected and not stale else "FAIL"
        print(f"{case.name}: {len(answers)} answers, {len(differing)} differ ({len(known)} known) {status}")
        for id_ in unexpected:
            print(f"  differs: {id_}")
            if id_ == "cli":
                print("    tsgo  : " + expected_cli[:300].replace("\n", " | "))
                print("    server: " + cli_text[:300].replace("\n", " | "))
            else:
                print("    tsgo  : " + (expected.get(id_) or "(no baseline)")[:300])
                print("    server: " + answers[id_][:300])
        for id_ in stale:
            print(f"  now matches, remove from known.txt: {id_}")
        if args.dump:
            dump = Path(args.dump) / case.name
            dump.mkdir(parents=True, exist_ok=True)
            (dump / "cli.txt").write_text(cli_text)
            with open(dump / "lsp.jsonl", "w") as out:
                for id_, answer in answers.items():
                    out.write(json.dumps({"id": id_, "answer": answer}, ensure_ascii=False) + "\n")
        failures += bool(unexpected or stale)
    sys.exit(1 if failures else 0)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("init")
    p.add_argument("case")
    p.add_argument("--positions", type=int, default=40, help="identifier positions per file (default 40)")
    p = sub.add_parser("update")
    p.add_argument("cases", nargs="*")
    p.add_argument("--tsgo")
    p = sub.add_parser("check")
    p.add_argument("cases", nargs="*")
    p.add_argument("--server")
    p.add_argument("--dump")
    args = parser.parse_args()
    if args.command == "init":
        init_case(CORPUS / args.case, args.positions)
    elif args.command == "update":
        update(args)
    else:
        check(args)


if __name__ == "__main__":
    main()
