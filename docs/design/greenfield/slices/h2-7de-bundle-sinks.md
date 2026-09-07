# H2.7d/e Bundle sink and listing observations

This packet freezes ten complete ordinary Program command observations from
pinned TypeScript 6.0.3. Each repeats on a fresh Program and empty in-memory
sink. The two unchanged script texts are `const a = 1;` and `const b = 2;`.
Their explicit roots, ES2015/AMD options, outFile, sourceMap, declaration,
declarationMap and listEmittedFiles survive in every input. The separate
25-case Bundle declaration fixture is unchanged.

The cases are a normal write, each of the four outputs skipped individually,
all four callbacks reporting onError, and a direct throw at each callback
position. This yields 34 callback attempts and 22 materialized files per
repetition, with six returned results and four direct exceptions. The full
callback bytes, BOM, sourceFiles, metadata before/after, actual materialized
bytes and indices, diagnostic order, complete map JSON, list, status strings
and exit/null result are retained. Source order and ordered libraries are
also recorded. No options diagnostic is suppressed: successful command
returns still report TS5101 and TS5107 and exit with code 2.

The observed asymmetry is in TS6 `_tsc.js:116634-116638` and
`116692-116712`. Callback order is JS map, JS, DTS map, DTS. Listing order is
JS, JS map, DTS, DTS map. Both text callbacks have data and can receive the
skippedDtsWrite mutation. Only skipped DTS text is removed from the list;
JS and both maps remain listed after a skip. Map callbacks have no data.
Both emitted map observations remain present after any skip or onError.

The all-onError control calls all four callbacks, materializes no files,
reports four TS5033 diagnostics plus the two option diagnostics, returns
emitSkipped=false and retains all four listing entries. Text callback
metadata.diagnostics remains an empty array. Throws at successive callback
positions preserve zero, one, two or three materialized files. The command
then has no result or exit code; its report and status buffers remain empty
because reporting runs after emit. These are direct callback exceptions,
separate from onError, and do not claim new System write-retry coverage.

The fixed native source 1f70213d4922b434345f639b441681e470c7cfc1,
`tsc/internal/compiler/emitter.go:359-390`, appends map paths on successful
writes and conditions text paths on write errors and skippedDtsWrite. Its
shared write architecture does not supply the TS6 Bundle listing contract.
The TypeScript result is the compatibility authority here.

`scripts/observe-bundle-sinks.mjs --write` and `--check` both reproduce the
complete ten-case observations twice. The fixture contains input and
observer/source hashes and remains marked TypeScript-reference-only with
runtime_admitted=0.

The separate draft `target/h2-7d-bundle-sinks-comparison.patch` adds a compiler
test using the real OutputSink interface and the parent's
ProgramSession::emit_command_for_harness command wrapper. It follows the
existing M5 controlled sink adapter: Err becomes onError/TS5033, controlled
panic becomes the exact direct exception, and SkippedUnchanged carries the
text mutation observation while runtime code determines listing. Unknown
errors and panics fail the comparison. Full tuples are compared without
dropping fields. Prepared input source order and ordered libraries are
checked independently against the same fixture.

The draft has passed formatting and patch applicability checks only. It has
not been compiled or executed against the still-separate production outFile
packet. Runtime connection, JS listing correction, complete Rust comparison,
canary/profile admission and closure remain parent integration work. This
packet adds no runtime guard changes or success claims for later owners.
