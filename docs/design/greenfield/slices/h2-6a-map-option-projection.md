# H2.6a map option projection

Owner: `h2-6a-r7-b6-option-inheritance`. Start:
`3757da2f6cb78a52531db293bba2f104b0975e02` (merged #518).

## Scope and evidence requirements

The three `optionsSourcemapInlineSources*.ts#default` rows retain their
original qualification inputs, routes, and TypeScript observations. The
target is removal of precisely those three divergence entries after complete
native comparison. The destructuring diagnostic row remains with its owner.
No syntax, checker, emitter, output comparator, CI, runtime activation, or other
band's qualification/manifest is part of this change.

The independent compiler test target is `h2_6a_map_option_projection`.
It exposes the unchanged `source_map_band_probe::probe_one_band_row` entry
and separately records complete command observations and PreparedProgram map
options under `TSRS_MAP_OPTION_CAPTURE`. The baseline test exercises each
original recorded compiler plan twice on `SourceMap`, then twice on the
existing `MapFamily` route. Only exact native results on the latter can
establish that this is input reconstruction rather than a map generator fix.

## Input inventory

`h2-6a-map-option-inventory.json` records all 177 qualification inputs, the
qualification SHA-256, routes, dispositions, original directive map settings,
virtual config presence, and frozen TypeScript repetition fingerprints.
Regenerate the read-only inventory with:

```sh
node crates/oracle/h2-6a-map-option-projection.mjs --inventory
```

There are 150 recorded compiler plans and 27 qualified VFS inputs, 175
admitted and two deferred. Every input carries `sourceMap=true`; precisely
the selected three also carry `inlineSources=true`, with `mapRoot=local`
and `sourceRoot=local` on one row each. No input has a virtual config.
These are input facts, not evidence of native equality.

## Projection and reader qualification

The separate `SourceMapWithOptions` floor preserves `sourceMap`,
`inlineSourceMap`, `inlineSources`, `sourceRoot`, and `mapRoot`. Keep the
historical `SourceMap` enum contract and all its other callers intact.
The H2.6a reader selects the new floor on both existing routes without
case-ID exceptions. The new floor does not admit the broader MapFamily
floor's BOM, declaration-map, or bundle output options.

Config and directive values use the same floor, preserving absent,
false, empty-string, and relative-path values. Config parsing stays with
the program config planner and directives remain the higher option layer.
The recorded plan route already inherits parsed config options. The
historical qualified VFS route reads virtual config options only for
DeclarationFamily; the new floor has an explicit config path to be a
general reconstruction contract. Earlier floors must retain their existing
config behavior.

The fixed-head native baseline reproduced precisely 1/2/1 differing writes
and differing emit results on `SourceMap`, with matching diagnostics. Each
of the same three recorded plans was completely exact twice on `MapFamily`.
The change is therefore input reconstruction and reader qualification; it
does not fix a missing native map implementation.

The first full-band attempt after projection failed at row 15 with
`unadmitted H2.6b activity`. This is a second part of the reader mismatch:
`emitter/src/execute.rs` already records embedded sources and root handling
as H2.6b. The H2.6a reader now qualifies that existing path only when the
PreparedProgram requests an external source map, no declaration, and
embedded sources or a root setting. It still rejects H2.6b activity on
ordinary map inputs, inline-map inputs, and later map owners. Independent
controls check that only the three selected frozen rows acquire H2.6b
activity. No production admission profile or runtime counter changes.

The frozen qualification's prose says "no inline/root variants", which
contradicts its three actual recorded inputs. The artifact is retained
byte-for-byte; the inventory, full tuples, and this reader migration make
that discrepancy explicit rather than rewriting historical observations.
After this qualification correction the full 177-case comparison reported
exactly the three selected stale entries, with no new/facet-changed rows.
Only then were those entries deleted.

## Independent TypeScript witnesses

`crates/oracle/h2-6a-map-option-projection.mjs` observes 31 new cases twice
before any production edits and writes only the independent fixture
`crates/compiler/tests/fixtures/h2-6a-map-option-projection.json`.
It never changes qualification inputs or their expected observations.

The cases cover external maps, inline maps, embedded sources, false
booleans, empty roots, relative roots, directive overrides of config values,
and option errors TS5051/TS5053/TS5069. Both direct directives and parsed
virtual configs are observed. In particular, conflicting config options
produce separate diagnostics at their option spans, so checking only codes
or diagnostic count would be insufficient.

Each observation includes ordered writes, callback bytes/hashes and lengths,
BOM and materialized-byte hashes/lengths, source provenance, callback data
presence/URL position/diagnostic count, full reported and emit diagnostics,
emitSkipped, emittedFiles (absent on these commands), raw sourceMaps,
status writes, and exit code. The native harness's sink contract supplies
the on-error callback; it has no separately exposed callback-presence field.
No frozen H2.6a input requests listEmittedFiles, and all new witnesses keep
that same command mode; the comparisons verify emittedFiles absence rather
than claiming a new listing-mode admission.

All 31 witnesses match twice through each native route under the new floor
(124 emissions). Before the change, all 31 recorded-plan witnesses and all
15 directive witnesses on qualified VFS matched the existing MapFamily
route. Fifteen qualified-VFS config witnesses differed because that route
ignored config options; the all-false control already matched. Both the
before and after complete observations are retained locally under
`target/map-option-evidence`.

## Execution constraints and completion gates

Use `CARGO_TARGET_DIR=target/map-option-build CARGO_BUILD_JOBS=2` and
`nice -n 10`. Run one heavy local process at a time; inspect live process
state before starting, including Claude's UTF-16 worktree.

The original ignored probe ran twice per selected row at the fixed start
head with no production diff. Complete baseline comparisons and the 62
existing-route witness combinations were then captured before production
changes. The permanent test target passes all three tests: historical floor
divergence/existing native parity, selected rows plus two adjacent controls,
and all directive/config witnesses. The 14 harness execution tests pass,
including all six floors' directive and config projection contracts.

The final local band runs passed:

| Band | Candidates | Exact | Known diverging | Deferred | Repetitions |
| --- | ---: | ---: | ---: | ---: | ---: |
| H2.6a | 177 | 174 | 1 | 2 | 2 |
| H2.6b | 6 | 6 | 0 | 0 | 2 |
| H2.6c | 643 | 631 | 8 | 4 | 2 |

`h2-6a-map-option-receipt.json` preserves complete selected-row baseline and
projected tuples, repetition fingerprints, all 62 witness-route comparisons,
the three stale-entry messages, unchanged qualification/comparator hashes,
passing command summaries, and the pure manifest deletion (4 to 1, zero
added or modified entries). Witness freshness and workspace formatting also
passed. Local raw captures and command logs remain under
`target/map-option-evidence`.

Integration is separately established by the PR's full
`cargo xtask acceptance` check and merge state; this is a local execution
receipt. No broader developer CI, walk, or chain-walk was added.
