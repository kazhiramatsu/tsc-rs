# JSDoc corpus

Small JavaScript (and a few TypeScript) projects whose answers tsgo's test
suite does not cover: the shapes real projects use — nested `@type` casts,
`@template` on arrows, `@import`, `@typedef` conditional types, `@overload`,
`@property` without a type — collected while comparing tsc-rs's language
server with tsgo's on webpack, svelte, SvelteKit, ESLint and prettier
(docs/design/greenfield/slices/ts71-real-projects/README.md). Each case
stores tsgo's own answers, so the check needs no tsgo.

## Layout

```
tests/jsdoc-corpus/
  initialize.json            the client capabilities both servers are initialized with
  <case>/
    tsconfig.json, *.js, …   the project (no node_modules; `types: []`)
    requests.jsonl           the language-server requests to replay, one per line
    expected/cli.txt         tsgo's `tsc -p . --pretty false` output and exit code
    expected/lsp.jsonl       tsgo's answer per request id (normalized, canonical JSON)
    known.txt                request ids (or `cli`) allowed to differ, each with a reason
```

A request is `{"id": …, "file": "a.js", "method": "textDocument/hover",
"params": {"position": {"line": 11, "character": 51}}}`; the runner adds
`textDocument` for the file, opens each file once (in the order the requests
first use it) and never closes it, so both servers see the same session.
Positions are UTF-16 (the client asks for `utf-16`). An answer is normalized
as the differential recorder does: `{"result": …}` or `{"error": message}`,
the arrays tsgo does not order (references, definitions, highlights, …)
sorted, the case's absolute directory replaced by `${ROOT}`, keys sorted,
compact JSON.

Every source file of a case must belong to its `tsconfig.json` (`update`
checks this with `--listFiles`): a file outside the project opens as an
inferred project, whose automatic type acquisition makes the answers depend
on the machine's type cache. Every case sets `types: []` for the same
reason.

## Checking

`cargo test -p tsc-rs-cli --test jsdoc_corpus` (part of every crate's tests
and of the hosted `rust` job) runs tsc-rs's command line and language server
on every case and compares with `expected/`. A difference whose id is not in
`known.txt` fails; so does a `known.txt` id whose answer now matches (remove
it — the ratchet moves only forward). `JSDOC_CORPUS_CASE=<name>` runs one
case; `JSDOC_CORPUS_DUMP=<dir>` writes tsc-rs's answers next to tsgo's.

## Adding or refreshing a case

```
python3 scripts/jsdoc_corpus.py init <case>      # samples requests.jsonl from the case's files
python3 scripts/jsdoc_corpus.py update [<case>…] # regenerates expected/ from the vendored tsgo
python3 scripts/jsdoc_corpus.py check [<case>…]  # the same comparison as the test, from Python
```

`update` needs the vendored tsgo (`target/typescript7/bin/tsc-<commit>`,
`scripts/typescript7.py`); it is run locally and the baselines are committed.
Edit `requests.jsonl` by hand to add the positions a difference was found
at (keep the ids stable: `<file>:<line>:<character>:<method>`).
