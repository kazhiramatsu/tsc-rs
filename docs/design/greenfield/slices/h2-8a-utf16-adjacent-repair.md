# UTF-16 adjacent repairs: design for cross-review

Status (2026-09-13): native before measurement complete; design cross-review
pending. **No
production repair in this packet has been implemented.** The user extended
the scope to all 23 previously recorded adjacent probes, then requested
cross-review before proceeding if the checker repair is not local. The
latest reviewer assignment is **fable-5.1, reasoning effort max**, replacing
the earlier proposed Claude review. Inspection confirms a binder/checker
identity boundary; this packet
is the concrete proposal for that review, not a signed implementation gate.

The integration worktree is
`/Users/hiramatsu/dev/tsc-rs-declaration-comment-design`, branch
`work/h2-8a-declaration-comment-ranges`, combined before head
`ed6d8073ae0f4f4d307f21a37c21a483e0a41bf0`. It includes the
[declaration-comment repair](h2-8a-declaration-comment-ranges.md) and Claude's
[literal-value repair](h2-5h-utf16-literals.md) at
`816b31c2711a144ac379fd0af2db4f02b7aa0525`. The trusted main base remains
`f406f12009cd476e28f9e0dcfb3ae4030563fa67`.

## 1. Scope and measurements

The immutable input and TypeScript observation files are
`crates/compiler/tests/fixtures/utf16-literals-adjacent-probes-inputs.json`
and `utf16-literals-adjacent-probes.json`. Their SHA-256 hashes are,
respectively:

```
44672cd7192c2897f25cf93f0c04892a2c3bcabd00589de83ac32bfff2ee6b69
6cc7c405903740308ed765aa0c3d1e72a51521d9207058c50b40391cd9e8a963
```

Keep all 23 commands, including declaration, declarationMap, sourceMap,
diagnostic reporting, write order/metadata and exit status. Do not remove
declaration options, normalize output, or replace the failure fixtures.

| Population | Commands | First known owner; additional dependencies |
| --- | ---: | --- |
| const initializer, object member names, class collisions, object collisions; ES5 and ES2015 | 8 | A: literal values and symbol identity together |
| octal, decimal escape 8, malformed hex, unterminated extended escape, out-of-range extended escape, unterminated string, template octal; ES5 and ES2015 | 14 | B: parse-diagnostic emit refusal; recheck A and all emitted products after reaching emit |
| tagged invalid cooked at ES2015 | 1 | C: ES2018 `LiftRestriction` consumer |

Claude's before record is 0/23 exact. The new combined-head measurement also
finds **0/23 exact, with all 23 failures identical across two captures**.
The [immutable before receipt](../../../../ratchets/h2-8a-utf16-adjacent-before.v1.json)
retains both raw captures for every gap and separates the existing controls.
Runner:
`target/declaration-comment-ranges-runs/run-utf16-adjacent-before.py`;
run directory marker: `target/utf16-adjacent-before-run.txt`. It records the
head, tracked crate input hashes, binaries, real command exit codes, logs,
and repeated captures. It also reruns Claude's 4 original rows and
64 promoted controls, the 41 declaration-comment controls, both required
originals and the still-failing shared G5c original. A result at a previous
head is not qualification of this combined tree. The completed run is
`target/declaration-comment-ranges-runs/utf16-adjacent-before-20260913-005302`.
All three builds passed and all tracked crate inputs and the head stayed
unchanged through measurement.

| Combined before check | Result and evidence boundary |
| --- | --- |
| 23 adjacent commands | 8 A divergences, 14 B typed refusals, 1 C divergence; test exit 101; all twice identical |
| 64 existing UTF-16 controls | Full captured tuples exact twice; test exit 0 |
| 41 declaration-comment controls | Full captured tuples exact twice; all three test commands exit 0 |
| Required originals G4a/G4b | Strict primary comparisons pass twice; each test exits 0; the loader captures failures only |
| Shared G5c original | Strict exit 101 and repeated declaration-output mismatch; remains outside this repair |
| 4 original H2.5h rows | Existing typed comparator passes twice; test exit 0; its raw JSON is a projection, as explained below |

Evidence audit found that the 4-row target stores only selected native
fields: its expected JSON includes additional write metadata and status
fields absent from its actual JSON. The typed comparator tests its defined
fields and passes; generic JSON equality between these different schemas is
inapplicable. Do not treat the old projected captures as full command
captures or mistake this audit mismatch for a compiler regression. Section 8
now supplies an independent complete-command observation for the same four
inputs without weakening the existing comparator. The 23 requested probes
and 64+41 controls already use full command captures.

## 2. Why a literal-type-only fix is insufficient

These are current source facts, not a prediction based on test names:

1. `crates/types/src/ty.rs::TemplateText` and
   `tables.rs::get_string_literal_type_from_utf16` already retain arbitrary
   UTF-16 units. The literal type interner itself needs no new encoding.
2. `crates/checker/src/expr.rs::check_expression_worker` constructs ordinary
   string/no-substitution-template literal types from lossy `data.text`.
   `declaration_emit.rs::emit_literal_type_to_node` already writes string
   values through `create_string_literal_from_code_units`; changing that
   writer would repair the wrong owner.
3. `crates/binder/src/symbols.rs` defines
   `SymbolTable = IndexMap<String, SymbolId>` and `Symbol.escaped_name: String`.
   `declare.rs::get_declaration_name` and
   `node_util.rs::get_escaped_text_of_identifier_or_literal` derive literal
   property keys from the same lossy `data.text`. Distinct isolated
   surrogates therefore enter the same symbol-table slot.
4. `crates/checker/src/indexed.rs::property_name_from_type` and
   `flow.rs::try_get_name_from_type` convert `TemplateText` with `to_utf8()`.
   For an isolated surrogate that conversion returns `None`: switching
   only expression typing to UTF-16 makes those lookup paths lose their
   property name. Mapped types, narrowing and structural lookup consume
   these paths too.

For example, declaration and lookup must agree for both properties below,
and must distinguish both from U+FFFD and the six literal characters
backslash-u-D-8-0-0:

```ts
export const o = { "\u{D800}": 1, ["\u{DC00}"]: 2 };
export const a = o["\uD800"];
export const b = o[`\uDC00`];
```

## 3. A: proposed identity design, pending review

The user's clarified criterion is **correct representation, then the same
operation semantics as TypeScript on that representation**. Current Rust
behavior is not the semantic oracle. Surrogate collisions and dropped
property names are incorrect behaviors to remove, not compatibility to
preserve. Review every operation against its upstream meaning; retaining
old lossy behavior behind an adapter is not a valid migration strategy.

Use an explicit lossless **escaped symbol name** type for identity-bearing
keys and `Symbol.escaped_name`. Its equality and hashing must be based on
UTF-16 units after the existing leading-underscore escape. Put the value
type below binder/checker in the dependency graph; retain `TemplateText`
for literal type values. Conversions from UTF-8 names must produce the same
key as the equivalent UTF-16 input. Ordered symbol-table iteration remains
unchanged.

Do not mechanically convert every Rust `String`: source paths, diagnostic
rendering and semantic symbol identity have different purposes. Audit
identity-bearing caches and helper return types from the changed entry
points. In particular, a string display fallback is not a property key.

The minimum atomic behavior change includes:

- A canonical lossless literal-value API usable by binder and checker.
  Review must settle where that value is owned from scanner/parser onward,
  including synthesized values. The existing qualified raw decoder can be
  reused, but a source-reconstruction helper alone does not establish that
  all identity consumers use the correct representation. A parsed-source
  view must prove token boundaries and owning-source identity; synthesized
  nodes must carry their value explicitly. Remaining `data.text` reads need
  classification by semantic use, not an implicit lossy fallback. Binder
  must not depend on emitter's transform arena just to read a parsed name.
- Binder declaration/computed-property keys, assignment names and duplicate
  detection; checker ordinary literal typing and literal type annotations.
- Both literal-type-to-property-name paths, direct element-access names,
  member/export lookup and identity-bearing caches; mapped/`keyof` results
  and narrowing must keep that same identity.
- Declaration member-name synthesis and diagnostic display at explicit
  output boundaries. Preserve internal names, user names beginning with
  `__`, private names, unique-symbol names and numeric-key behavior.

A UTF-8 fast path is compatible with this proposal, but its equality/hash
contract must be proved first. In particular, derived enum hashing plus
`Borrow<str>` is not automatically correct. Do not use escaped display
strings or an unproved sentinel encoding as surrogate keys.

Alternative for review: a single lossless name interner with typed IDs.
It would also work only if parsed declarations, checker-created symbols,
cross-file merges and caches share its lifetime/identity. It is not a local
side table attached only to the failing nodes. The proposed value type
avoids introducing that additional interner-lifetime change in this repair.

Before editing, the reviewer and implementer should settle the concrete
type/API, canonical value ownership and an operation-by-operation migration
map. Enumerate callers whose signatures change and identity caches whose
keys change. An unchanged test result alone does not prove equivalent
operations on the new representation. Preserve separate
cause commits; do not publish a literal-type-only intermediate state as a
qualified runtime fix.

## 4. B: parse recovery is a separate policy decision

`crates/emitter/src/builtins.rs::preflight_source` currently refuses every
source with parse diagnostics using `ParseDiagnosticsDeferred { owner_slice:
"H2.9" }`. TypeScript's command driver still calls `program.emit` after
collecting syntactic diagnostics; `noEmitOnError` is a distinct option.
The fourteen probes currently stop before their emitted products can be
compared.

Do not delete the guard wholesale or admit individual fixture IDs/diagnostic
codes. First determine whether the parser can identify recovery confined to
literal token values while the surrounding AST remains complete. If that
boundary can be represented and witnessed, admit it explicitly and keep
unqualified structural recovery deferred. If it cannot, review a broader
H2.9 recovery design before changing the refusal contract. This question is
not settled by the scanner decoder's existing unit tests.

Required comparisons include malformed literal emission, diagnostics and
exit status with `noEmitOnError` both off and on, declaration-only/noEmit
controls, and a structural parse failure outside the admitted boundary.
After lifting a refusal, compare JS, declarations, both maps and callback
metadata; the initial refusal may conceal additional producer differences.

## 5. C: restore the ES2018 tagged-template consumer

The ES2018 transform already exists. Its visitor has no tagged-template
case, while `builtins/tagged_template.rs` accepts only `Es2015Visitor` and
leaves `ProcessLevel::LiftRestriction` unused. This is a missing shared
consumer, not a request to activate the whole ES2018 transform.

Extract the shared algorithm behind a host interface for the ES2015 and
ES2018 visitors. The ES2018 host must own its visitation, helper requests,
temporary allocation and source-file trailing declarations. Preserve the
upstream visit order, cooked `void 0`, normalized raw strings, module
template-object caching and retained valid templates. A second copied
implementation would leave two algorithms to keep consistent.

Witness invalid/valid tagged templates at ES5, ES2015, ES2017 and retained
ES2018, nested tags and substitutions, script/module temp behavior, helper
deduplication and maps. Keep the original ES5 invalid-cooked control and
Claude's 64 promoted controls exact. This implementation can be assigned
separately once concrete non-overlapping file ownership is agreed.

## 6. Upstream anchors and validation gate

Authority: vendored TypeScript 6.0.3 `_tsc.js` SHA-256
`1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`;
`typescript.js` SHA-256
`569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39`.
The table hashes the indicated inclusive line excerpts, including their
original LF bytes; multi-function excerpts are not individual ledger hashes.

| Boundary | `_tsc.js` lines | Excerpt SHA-256 |
| --- | --- | --- |
| ordinary literal expression arm | 81031–81035 | `cac07b86b50c60f4eb94490a9248973e10fd46ab80c57423037f7d01129bb719` |
| binder declaration name | 42534–42598 | `d2af29f322058fe2e4f4a1064734eea28f25a726f6cbbbd5d8e19bf6d8dbd4bd` |
| property name from type | 19351–19362 | `d7603a1a910389c7918a0a9f5e86222fb9d2a3ec13180bd36288d0b0a49c6802` |
| flow name conversion and direct access | 69509–69514 | `3d6f16c7d159ef70173491497e59b3b5dbf51bfd50740c0a143ad776f5a1e8ce` |
| declaration const value | 88506–88509 | `aed30591a56b896560cdc11531e90bd746b037ffa64fa9d884cd9e384048ee53` |
| shared tagged template | 93972–94033 | `e72e23fbba59ef9ae11be42e95119840e52cea0d422691a986117da145e0f69e` |
| ES2018 source-file tail and consumer | 102022–102056 | `c55d86cf21bd36122cd8388875ab1a672c0c400b093bc39e432192c3e1239d44` |
| command diagnostics/emit driver | 129412–129467 | `9dc0128691c9a1bee5aeae85524cc8e2679b3905a4416a41095452e509951a8d` |

Review must add any newly reached upstream producer before implementing it.
Additional A controls must distinguish D800, D801, DC00, FFFD, literal
backslash-u spelling and a valid surrogate pair; prove escape-spelling
equivalence, real duplicate diagnostics, successful indexed access,
`keyof`/mapped/union properties, narrowing and cross-file names. Include
ordinary ASCII, non-BMP and leading-underscore/internal-name controls.

Qualification requires all original 23 full tuples exact repeatedly plus
the added source-observed controls and relevant binder/checker/emitter
regressions. Retain the prior 4+64 literal cases and 41 declaration-comment
cases. The shared G5c return-inference mismatch remains a separate explicit
failure unless independently repaired and qualified. The historical A37
count and existing before/after receipts are immutable snapshots.

Use the current schedule's focused-observation workflow and a fresh hosted
acceptance at the final combined head. No local full CI or certificate walk
is claimed or reintroduced. The older PR #520 hosted run measures
`e8b8f44a6`, which predates Claude integration and this extension; its merge
watcher is paused and it must not merge this unfinished work. That old
[run 34701741388](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34701741388)
completed successfully on 2026-09-13 JST (updated 16:00:40 UTC on September
12). It is evidence for the old head only; it does not satisfy the final
combined candidate's acceptance requirement.

## 7. Requested cross-review decision

The requested reviewer, fable-5.1 max, should review A's representation
invariant, operation semantics and
type/API choice first, then identify missed consumers or a coherent
alternative. Scope reduction is acceptable only if it retains the full
semantic closure; minimizing diff size is not the design criterion.
For B, decide whether literal-only recovery has a defensible parser-owned
boundary. For C, review the shared host and per-source temporary lifetime.
Return concrete source references, required changes and any unproven
branch; do not count a proposed design as a completed repair.

Codex will incorporate the review, freeze concrete file ownership and
additional observations, implement, then request review of the resulting
diff and repeated evidence. At this checkpoint the implementation writer
remains Codex; another implementation lane requires an explicit ticket so
two agents do not edit the same symbol representation concurrently.

Invocation status: the explicitly requested `fable-5.1` / `max` delegation
was attempted after the user selected it. The session's agent tool returned
`Unknown model: fable-5.1`; no reviewer agent was created. No substitute model
has reviewed or approved this packet. Its available model overrides are
`gpt-6-astra`, `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna` and `gpt-5.5`.
The handoff for an environment that supports the requested model is
`/Users/hiramatsu/dev/tsc-rs/target/next-slices-20260913/fable-5.1-max-utf16-adjacent-review.md`.
Production implementation remains pending the requested design review.

## 8. Completed original-row evidence supplement — 2026-09-13

While the user passes the design review to fable-5.1 max, the original four
rows received a separate complete-command check. This adds evidence and
tests only; no binder, checker, syntax, emitter or compiler production source
changed from combined production head `ed6d8073a`.

`scripts/observe-utf16-original-rows-complete.mjs` selects the four exact IDs
and validates their file hashes, roots, `/.src` working directory, settings,
absence of virtual config and symlinks against the immutable H2.5h input.
It reconstructs the existing qualification's option defaults and VFS host
policy, then invokes TypeScript's full command twice per row. Every field
present in the older frozen observation must match with no value
normalization. The separate fixture adds callback data presence/keys,
source-map URL position and diagnostics, related diagnostic information,
materialized bytes, command status and the complete emit result. The initial
write and a subsequent `--check` both succeeded.

New fixture: `crates/compiler/tests/fixtures/utf16-original-rows-complete.json`,
SHA-256 `0aad290c08c7825cee90bc5bcd96fc7258f020f0ba6f496b7c400be4e825b217`.
The original qualification artifact, typed comparator, 23 adjacent fixtures
and earlier before receipts are unchanged.

`crates/compiler/tests/h2_5h_utf16_original_rows_complete.rs` uses the same
qualified-VFS input loader and `Established` option floor, then invokes
`emit_command_for_harness` and compares the entire captured value. This
observes actual command status rather than reconstructing it from emitted
diagnostics. The older bounded library-bundle/typed-comparator execution
remains independently tested; its old captures are not retroactively relabeled.

Native run
`target/declaration-comment-ranges-runs/utf16-original-complete-20260913-011624`
used base head `ee4a093a2` plus the new hashed test/fixture/observer files.
The new target built successfully; two separate test jobs passed, each
executing every case twice. All **16 primary complete captures are exact**,
with four identical commands per original case. The
[supplement receipt](../../../../ratchets/h2-8a-utf16-original-complete.v1.json)
contains all raw captures, source/binary/log hashes and actual exits. It also
retains the preceding test-build attempt: an E0382 borrow error in the new
capture helper was corrected before the successful build. No compiler
behavior failure was hidden by that test-only correction.

This closes the measurement limitation for the current production tree.
After the requested representation/recovery/ES2018 repairs, rerun this
complete target alongside the old typed target and the original 23 cases.
It does not establish that those 23 gaps have been repaired or that the
pending representation design has been reviewed.
