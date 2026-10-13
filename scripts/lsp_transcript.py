#!/usr/bin/env python3
"""Run a scripted language server session and print what the server sent.

    scripts/lsp_transcript.py SCRIPT.json -- SERVER COMMAND...

The script is {"answers": {method: result}, "steps": [step, ...]}; a step is

    {"send": message}       send a JSON-RPC message
    {"raw": text}           send text as one message's content
    {"wait": id}            read until the response with this id
    {"waitMethod": method}  read until a server message with this method
    {"close": true}         close the server's input

Server requests are answered from "answers" (null otherwise) as they arrive.
After the steps the output is read to its end. The transcript is each
message's JSON text as the server wrote it, one per line, then `EXIT <code>`
and `STDERR <JSON string>`; `--normalize` replaces handler durations with
`<time>` and Go's randomized "unable to" with "cannot".

With tsgo as the server (`target/typescript7/bin/tsc-<commit> --lsp --stdio`),
this records the transcripts in crates/lsp/tests/fixtures/ the server tests
replay.
"""

import json
import queue
import re
import subprocess
import sys
import threading


def frame(data):
    return b"Content-Length: %d\r\n\r\n" % len(data) + data


def read_messages(stream, out):
    while True:
        length = None
        while True:
            line = stream.readline()
            if not line:
                out.put(None)
                return
            if line == b"\r\n":
                break
            key, _, value = line.partition(b":")
            if key == b"Content-Length":
                length = int(value.strip())
        out.put(stream.read(length).decode())


def normalize(text):
    text = re.sub(r" in [0-9.]+(ns|µs|ms|s)\"", ' in <time>"', text)
    text = re.sub(r" in [0-9.]+(ns|\\u00b5s|ms|s)\"", ' in <time>"', text)
    return text.replace("unable to unmarshal", "cannot unmarshal")


def main():
    args = sys.argv[1:]
    normalized = "--normalize" in args
    args = [arg for arg in args if arg != "--normalize"]
    separator = args.index("--")
    script = json.load(open(args[0]))
    command = args[separator + 1 :]
    server = subprocess.Popen(
        command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    messages = queue.Queue()
    threading.Thread(target=read_messages, args=(server.stdout, messages), daemon=True).start()
    answers = script.get("answers", {})
    lines = []

    def receive(predicate=None, timeout=30):
        while True:
            try:
                text = messages.get(timeout=timeout)
            except queue.Empty:
                sys.exit("timed out waiting for the server")
            if text is None:
                return False
            lines.append(normalize(text) if normalized else text)
            message = json.loads(text)
            if "method" in message and "id" in message:
                answer = {"jsonrpc": "2.0", "id": message["id"], "result": answers.get(message["method"])}
                server.stdin.write(frame(json.dumps(answer, separators=(",", ":")).encode()))
                server.stdin.flush()
            if predicate and predicate(message):
                return True

    for step in script["steps"]:
        if "send" in step:
            server.stdin.write(frame(json.dumps(step["send"], separators=(",", ":")).encode()))
            server.stdin.flush()
        elif "raw" in step:
            server.stdin.write(frame(step["raw"].encode()))
            server.stdin.flush()
        elif "wait" in step:
            receive(lambda m: m.get("id") == step["wait"] and "method" not in m)
        elif "waitMethod" in step:
            receive(lambda m: m.get("method") == step["waitMethod"])
        elif "close" in step:
            server.stdin.close()
    receive()
    code = server.wait(timeout=30)
    for line in lines:
        print(line)
    print("EXIT", code)
    print("STDERR", json.dumps(server.stderr.read().decode()))


if __name__ == "__main__":
    main()
