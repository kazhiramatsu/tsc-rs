# H2.7d/e internal Bundle map recording

This packet connects one source-map recorder to the shared Bundle writer.
Public outFile orchestration remains separate integration work. The compiler
comparison uses the E-owned Bundle path/artifact worker. The semantic baseline is TypeScript 6.0.3 at
`050880ce59e30b356b686bd3144efe24f875ebc8`.

## Recorder lifetime and source identity

`writeBundle` (`_tsc.js:117058-117073`) installs one writer and generator
before the shebang, prologues, helpers, and synthetic reference header. It
prints every member through that writer, then resets once. Each source does
not start a new generated line/column counter or map generator.

The source registration order is observable. `emitPrologueDirectives`
(`119788-119810`) calls `setSourceFile` only when that source first contributes
a prologue that has not already been emitted. `emitPrologueDirectivesIfNeeded`
(`119811-119824`) visits all sources before the body pass. `print` and
`setSourceFile` (`117099-117116`) then switch to each body source, even if it
is empty or all its statements were erased. A later source's unique prologue
can therefore make it the first `sources` entry, while the callback's
`sourceFiles` still follows Bundle source order.

The internal printer uses the existing `SourceMapRecordingInputs` and
`SourceMapRecording` types. It installs the recorder before the header,
switches at the first emitted prologue for each source and before every body,
and takes the generator only after the complete Bundle. The ordinary source
printer uses the same source-switch helper at its existing registration point.
Comment positions continue to use the current source; node/token ranges keep
their explicit source identity. Synthetic headers and helpers retain their
existing mapping policy rather than receiving reconstructed mappings from
output text.

The pinned native source at `1f70213d4922b434345f639b441681e470c7cfc1` was
reviewed at `printer/printer.go:5065-5096` and `5816-5843`. Its ordinary writer
and source-switch state retain the same registration, JSON exclusion, and
inline-content concerns, but it has no Bundle writer arm. That absence does
not replace the 6.0.3 Bundle semantics.

## Complete references and internal comparison

`scripts/observe-bundle-maps.mjs` records nine new controls and three unchanged
original compound inputs, each on two fresh ordinary Programs. The new
controls cover prologue registration order, duplicate-only and empty sources,
erased sources, Unicode comments with CRLF/BOM, shared synthetic references,
empty declaration-only output, inline sources, inline maps, and an ES5 helper
shared by multiple sources.

The three original `declarationMapsOutFile.ts`, `declarationMapsOutFile2.ts`,
and `declarationMapsWithSourceMap.ts` inputs retain every file, root, option,
and directory/case setting from the frozen declaration observations. Their
ordinary callback and emit-result tuples are deep-equal to those earlier
observations. No forced/getter output substitutes for an ordinary map.

Every reference retains callback order, paths, complete callback and
materialized bytes, BOM, source association, optional metadata, all diagnostic
streams, `emitSkipped`, and absent-versus-empty lists/maps. There are 43
callbacks per repetition. `--write` and `--check` both reproduce the complete
observations; this does not change corpus membership or runtime admission.

The compiler comparison borrows the production prepared Program's real host
and resolver. It transforms and prints JavaScript before declarations in the
same ordinary checker lifetime, with a fresh transform arena and recorder
for each output. The JavaScript arena snapshots direct parse-node metadata
after printing and before disposal; the declaration arena restores it before
transformation. It compares the complete generated map JSON, raw source
order, inline contents, full URL-bearing text bytes, UTF-16 URL offset, and
materialized BOM bytes. It retains the earlier 19-case declaration visitor
comparison. Callback invocation, fault order, cold API sequences, and command
acceptance remain beyond this internal printer facet.

These inputs have no `sourceRoot` or `mapRoot`. The JavaScript comparison
asserts their absence before borrowing the source-file recording/URL helper:
in that bounded branch its source argument is not consumed. Declarations use
`declaration_bundle_map_recording_inputs_for` and
`finish_declaration_bundle_map`, which model `sourceFile = undefined` and
retain the complete transformed Bundle source list independently of generator
`raw_sources`. Map and declaration artifacts retain complete bytes, BOM,
source association and optional metadata. Sink invocation and callback fault
ordering remain executor work. No input option is removed; additional
root-option owners and JavaScript JSON Bundle printing remain separate.

## Ordinary metadata lifetime and producer corrections

The separate `metadata_lifetime_references` field records six inputs on two
fresh ordinary and two fresh forced Programs each. Four inputs reuse the
three original compounds and ES5 helper control; two exercise parameter
properties at ES5/ES2015. Identity-only after/afterDeclarations hooks retain
the direct parsed-node metadata and full output tuples. They are API1 tree
references, not public custom-transformer admission. The ordinary tuples of
the four reused inputs equal the unmodified main observations.

TypeScript `visitParameter` (`_tsc.js:95029-95051`) sets flag 64 on an
unchanged binding name when the parameter updates. Ordinary declaration
printing sees that original-node mutation; fresh forced printing sees flag
zero. Rust's JavaScript parameter name remains isolated for parameter-property
projection, and now records the same flag on the original parse name at that
exact producer. Parameter `typeNode` remains on the JavaScript clone only:
the complete metadata probe rejects copying it to the original name.
`visitVariableDeclaration` (`95088-95108`) does leave `typeNode` on an
unchanged original variable name, so the handoff preserves that distinct case.

`ParsedEmitMetadata` keys records by Program source token and parsed NodeId,
validates original text-snapshot identity, parse lease and node range against
the live host, and remaps source identities when mount order changes.
Restore validates the complete snapshot before changing an empty target.
Only metadata directly attached to parsed nodes is captured; synthetic
`original` links are not walked. This bounded packet supports observed
flags/typeNode metadata and refuses other fields or synthetic references
explicitly. Fresh forced/getter operations do not restore an ordinary snapshot.
Constant-value and other additional metadata require subsequent observation
and portable ownership work before broader executor acceptance.

The ES2018 object-spread producer also returned an extra original/text range
on its synthetic assign-helper call. TypeScript's
`visitObjectLiteralExpression` (`102002-102017`) returns the helper call
directly, with no such location. Removing those two producer-level range
assignments removes the extra JavaScript map segment while retaining helper
deduplication. The generic original/range relationship is unchanged.

The fixed native `tstransforms/legacydecorators.go:127-147` likewise updates
the retained parameter name's flag through its emit context. Native Bundle
absence and transformer layout do not replace the TS6 ordinary/fresh-forced
lifetime observations.

```sh
node scripts/observe-bundle-maps.mjs --check
cargo test -p tsc-rs-compiler --test h2_7d_declaration_bundles
```
