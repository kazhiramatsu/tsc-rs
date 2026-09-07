# H2.7d bundle declaration observations

This packet supplies TypeScript 6.0.3 expectations for the bundle declaration
visitor. It adds no Rust admission, canary, profile or closure change. Run
`node scripts/observe-bundle-declarations.mjs --check` with the pinned Node
version to reproduce `crates/emitter/tests/fixtures/bundle-declarations.json`.

The 25 main sequences comprise 18 new controls, four unchanged inputs from
`bundle-plan.json`, and the three unchanged D/E compound corpus inputs. The
existing 55 bundle-plan witnesses and 24 module-identity Programs remain their
own evidence; module identities are referenced by artifact hash, not counted
again as new controls. Two additional sequences retain H2.9 noEmit and H2.8d
ordinary targeted emit as references. All inputs and the observer are hashed.

| New controls | Purpose |
| --- | --- |
| `references/path-*` (5) | Shared and duplicate path references, root reversal, relocated output/BOM/CRLF, excluded external module, missing target |
| `references/type-lib-*`, `references/amd-no-default-lib` (3) | Preserved type/lib references, resolution modes, order, AMD directives and no-default-lib |
| `wrappers/*` (4) | Global/TS/JS/JSON mixture, AMD/System, ambient augmentation and stripInternal erasure |
| `state/*` (3) | Late-painted local declarations and scope-marker isolation across sources |
| `diagnostics/*` (3) | Multiple declaration errors, noEmitOnError and a semantic-only early gate |

New controls use `ignoreDeprecations: "6.0"` to isolate the declaration branches.
The four reused main plan inputs, noEmit reference, and three corpus inputs
retain their original options, roots, files and deprecation diagnostics. The
compound rows retain `/.src` paths and original input hashes. Their first
ordinary command tuple must equal the complete frozen census observation;
the reused plan rows must equal the existing emit result and write records.
These joins never turn an altered corpus input into an apparent success.

Each of the 27 sequences runs twice on fresh Programs. Calls within a sequence
share the same Program and checker: ordinary command, declaration getters
(whole/target/repeated), and forced whole/target emit where useful. There are
102 calls per repetition. Diagnostic snapshots follow the calls to avoid
warming the first operation. Each call preserves ordered writes, complete
callback and materialized UTF-8 bytes, BOM, source files, callback metadata,
diagnostics, emitSkipped, sourceMaps, emittedFiles, command status/exit code,
resolver requests, and nullable exception/result fields. Absence is distinct
from an empty list. All observed exceptions in this packet are null; the
existing bundle-plan forced declarationMap/declaration-disabled Debug Failure
remains an upstream exception witness and is not reclassified as success.

Each input also has two separate fresh-Program probes using a forced identity
`afterDeclarations` hook. These 54 supplementary Programs are explicitly API1
references; the 54 main Programs have no custom transformer. Probes preserve
Bundle synthetic references, their positions and attributes, transformed source
flags, statement positions, modifiers and nested module wrappers. Wherever the
main sequence contains forced whole emit, the probe's full write/result tuple
must equal that call. Input source facts separately preserve external-module,
CommonJS, JavaScript, JSON and no-default-lib flags.

The relevant observed behavior is:

- Internal emitted TS path references disappear; external declaration references
  remain relative to the bundle declaration output directory. The normal shared
  case retains `shared, shared, shared, other`; reversing roots retains
  `shared, other, shared, shared`. Synthetic reference positions are `-1/-1`.
  An external TS source excluded by module=None instead retains its planned
  `../src/external.d.ts` reference. A missing target reports TS6053 and its
  reference disappears. Relocating outFile changes relative references and
  preserves BOM/CRLF; its incompatible declarationDir still reports TS5053.
- Preserved type/lib duplicates remain in source order, including import/require
  resolution modes. Non-preserved references disappear. no-default-lib is false
  on transformed sources. AMD directives remain immediately before their own
  ambient module wrappers, after the combined reference header.
- Global declarations receive `declare`; external TS and CommonJS/JSDoc JS
  receive ambient wrappers. CommonJS source facts have a CommonJS indicator
  even when `isExternalModule` is false. JSON is marked both JavaScript and JSON;
  ordinary bundle declarations exclude it, while forced whole and forced JSON
  target emit include its wrapper and the other bundle sources. System's
  resolveJsonModule option diagnostic is preserved. Ambient augmentations use
  the rewritten bundled module name. stripInternal can leave an empty external
  wrapper while erasing a global source completely.
- Late-painted local types remain with the source that needs them. Explicit
  empty export markers do not leak to neighboring wrappers. Reversing source
  order changes bundle order without transferring this state between sources.
- Multiple TS4094 errors block the entire ordinary declaration bundle while
  leaving JavaScript output when noEmitOnError is false. Whole getters sort both
  diagnostics; target getters return the selected source's diagnostics. Forced
  whole/target calls still write the whole declaration bundle with emitSkipped
  true. noEmitOnError suppresses all ordinary writes. A semantic-only TS2322
  gate has no declaration diagnostics and forced emitSkipped is false.
- Empty and declaration-only reused inputs retain absent lists and no writes.
  A declaration collision can leave ordinary JavaScript only; forced emit still
  writes the declaration path with emitSkipped true. The noEmit and ordinary
  targeted variants remain owned by H2.9 and H2.8d respectively.

All three original D/E compound cases have identical declaration text between
ordinary and forced calls, but different declaration-map mappings around the
inline object parameter of `doThing`. Ordinary mappings contain
`OAAO,CAAC,GAAG`; forced mappings contain `OAAO,CAAC,CAAC,EAAE` at that location.
Forced whole, forced target and the independent tree probe agree. Consumers
must compare each call's stored map bytes and metadata rather than reuse the
ordinary map for forced emit. These are TS6 expectations; no native-7 behavior
is substituted and no compound original is admitted by this packet.

Validation: the observer's `--write` and `--check` each run both repetitions
and all unchanged-input joins. Node syntax, input and dependency hashes, unique
IDs, byte lengths, nullable fields and `git diff --check` are also checked.
