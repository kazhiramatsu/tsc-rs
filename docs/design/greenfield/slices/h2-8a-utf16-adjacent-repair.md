# UTF-16 adjacent repairs: design for cross-review

Status (2026-09-13): **implementation incomplete; handed off for Claude to
continue at the user's request.** See the
[implementation handoff](h2-8a-utf16-adjacent-handoff.md) for the frozen dirty
state, 40 remaining all-target compile errors, evidence and restart steps.
The design review and §10 follow-up corrections were accepted. A/B/C have
implementation changes, and **the original 23 complete commands were exact
twice on an intermediate snapshot (§31)**. Final consumer classification,
corpus admission audit, regression qualification and review remain open.
The user extended
the scope to all 23 previously recorded adjacent probes, then requested
cross-review before proceeding if the checker repair is not local. The
design review was delivered by **fable-5.1, reasoning effort max**. The user
will request **Claude** to review the completed implementation; the
[implementation review request](h2-8a-utf16-adjacent-implementation-review.md)
is being prepared and will be frozen after implementation and validation.
Inspection confirms a binder/checker
identity boundary; this packet
records the proposal and subsequent review decisions, not a qualified repair
or a completed implementation gate.

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

Historical invocation status: the explicitly requested `fable-5.1` / `max` delegation
was attempted after the user selected it. The session's agent tool returned
`Unknown model: fable-5.1`; no reviewer agent was created. No substitute model
has reviewed or approved this packet. Its available model overrides are
`gpt-6-astra`, `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna` and `gpt-5.5`.
The handoff for an environment that supports the requested model is
`/Users/hiramatsu/dev/tsc-rs/target/next-slices-20260913/fable-5.1-max-utf16-adjacent-review.md`.
The requested review was subsequently delivered through that handoff; see §10.
The invocation failure above is historical, not the current review status.

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

## 9. Operation and representation audit for the pending review

Read-only source audit at `1effc9100`; production remains `ed6d8073a`.
This makes the migration questions concrete. It is not an exhaustive caller
inventory or a signed file-ownership gate. In particular, semantic use of a
value decides whether its representation must change; Rust's type name
`String` alone does not decide that question.

### 9.1. Additional identity/value consumers verified in source

| Operation | Current producer/consumer | Required semantic relation |
| --- | --- | --- |
| Expression and annotation literal values | `checker/src/expr.rs::check_expression_worker`; separately `annotate.rs::get_type_from_literal_type_node` → `check_literal_expression` | Both construct the same regular/fresh literal types from the same UTF-16 value; the annotation path independently reads lossy `data.text` today |
| Property name → literal type | `checker/src/indexed.rs::get_literal_type_from_property_name`, `get_literal_type_from_property` | `keyof`, contextual typing and mapped/inferred properties must round-trip the key value. Direct literal names still use `data.text`; the no-declaration fallback currently constructs a literal type from `symbol_display_name` |
| Syntax name → escaped key | `binder/src/declare.rs::get_declaration_name`, `node_util.rs::get_escaped_text_of_identifier_or_literal`; `checker/src/evaluate.rs::try_get_text_of_property_name` | Declared, computed and assigned names with equivalent spellings must form one canonical escaped key, independently of source spelling |
| Literal type → member lookup | `checker/src/indexed.rs::property_name_from_type`; `flow.rs::try_get_name_from_type`; `mapped.rs` mapped-member construction | Every usable string literal type has a property name. The current `to_utf8()` → `None` path violates that domain; mapped-member callers even `expect` a name after the usable-type check |
| Union/intersection property cache | `checker/src/structural.rs::get_union_or_intersection_property`; `links.rs::union_property` / `set_union_property` | Cache key `(TypeId, name, skip)` must use the same name identity as member lookup; retain the meaning of `skip` and the existing speculative publication protocol |
| Discriminant-property exclusions | `checker/src/structural.rs` excluded-property sets in discriminant relation and property comparison | Set membership uses property identity; distinct surrogate keys must not exclude one another |
| Export and type-only membership | `checker/src/modules.rs::visit_module_exports` and `get_export_of_module`; `links.rs::type_only_export_star_map`; `modules.rs::ExportLookupTable` and `non_type_only_names` | The auxiliary maps/sets are keyed by exported names, just like `Symbol.exports`. The first field of `ExportLookupTable` is a member name; its stored specifier text is a separate semantic field |
| Unique-symbol generated name | `checker/src/annotate.rs` `__@{escaped_name}@{symbol_id}` construction; `types/src/ty.rs::TypeData::UniqueESSymbol` and `tables.rs::create_unique_es_symbol_type` | Prefix/name/id concatenation is a JavaScript-string operation; do not route it through a lossy diagnostic `Display` implementation after introducing a typed name |
| Constant evaluation and enum literal values | `checker/src/evaluate.rs::EvalValue`, literal arms, `+` concatenation and `evaluate_template_expression`; `annotate.rs` enum value → literal type | `EvalValue::Str(String)` and concatenation currently lose units independently of expression typing. Enum literal interning already accepts `TemplateText`; its input producer must preserve the value too |
| Declaration property name construction | `checker/src/node_builder/chains.rs::chains_get_property_name_node_for_symbol`, `get_property_name_node_for_symbol_from_name_type`, `create_property_name_for_identifier_or_literal` | The name-type branch already uses `create_string_literal_from_code_units` for non-scalar values; the raw symbol-name fallback still takes `&str`. Both must implement the same identifier/numeric/string selection on the new representation |

Verified counterexamples to a blanket cache-key rewrite:

- `types/src/tables.rs::string_literal_types` already uses `TemplateText` as
  its canonical key. `utf8_string_literal_types` is an exact scalar-only
  lookup mirror, populated only when lossless `to_utf8()` succeeds.
- `types/src/tables.rs::template_literal_types` has a Rust `String` key,
  but constructs it from type IDs, fragment UTF-16 lengths and fixed-width
  hex for every code unit. It does not collapse surrogate values.
- `links.rs` alias and conditional instantiation keys come from type-list
  IDs and alias IDs (`annotate.rs::get_type_alias_instantiation`,
  `conditional.rs` instantiation). Those fields do not store property names.
- Despite its name, `NodeLinks::non_existent_prop_check_cache` is keyed by
  containing type ID and the unchecked-JS boolean within a property-node ID
  (`access.rs::report_nonexistent_property`). Its speculation rollback log
  mirrors that same key. It is not a string-name cache.

The reviewer should extend this map through each actual caller and any
remaining `data.text` use before signing implementation ownership. Preserve
the upstream operation's meaning, including freshness, cache identity,
order, speculation and output selection; do not retain an incorrect old
result merely because it was previously observed.

### 9.2. Diagnostic values also need an explicit representation boundary

The independent design probe
`scripts/observe-utf16-diagnostic-values.mjs` calls TypeScript 6.0.3
`getPreEmitDiagnostics` and `flattenDiagnosticMessageText` twice for each
of three tiny inputs. Its
[record](../../../../ratchets/h2-8a-utf16-diagnostic-values-design.v1.json)
stores every message as UTF-16 units, its JSON spelling, and the separate
Node UTF-8 byte conversion. These are six diagnostic API observations,
**not** complete-command or native qualification and not new admissions.

| Input | Diagnostic | Surrogates in flattened message value |
| --- | --- | --- |
| `export class C { "\u{D800}"=1; "\uD800"=2; }` | 2300 | None; this producer uses escaped source-name spelling |
| `export const o = {}; export const x = o["\uD800"];` | 7053 | Two D800 units, both unpaired |
| `export const x: "\uD800" = "\uDC00";` | 2322 | One DC00 and one D800 unit, both unpaired |

`crates/diagnostics/src/lib.rs::MessageChain.text` is currently a Rust
`String`. That type cannot hold the latter two TypeScript diagnostic values.
Escaping the value to printable `\\uD800` or replacing it with U+FFFD inside
the semantic message does not reproduce that API. Conversely, a UTF-8 output
boundary has its own encoding behavior; it must not be confused with the
internal diagnostic's JavaScript-string value. The class-duplicate control
also shows why one blanket escaping policy for all diagnostics is wrong.

The review must therefore settle canonical diagnostic-value ownership and
the explicit API/rendering/encoding boundaries along with symbol identity.
For source-observed controls containing these values, the observation
transport must retain code units: a plain `serde_json::Value::String` field
cannot represent an isolated surrogate. The design probe uses an explicit
UTF-16 array instead of silently changing the expected message. Existing
fixtures and comparators remain unchanged; no diagnostic representation or
test-protocol migration has been implemented here.

### 9.3. The existing parser error bit does not prove literal-only recovery

`syntax/src/parser.rs::drain_scanner_errors` and
`parse_error_at_position` both feed the same diagnostic list and set the
same `parse_error_before_next_finished_node` boolean. `finish_node_at`
consumes that boolean by marking the next finished node with
`THIS_NODE_HAS_ERROR`. `push_parse_diagnostic_with_index` also deduplicates
at the same diagnostic start. The shared bit/list does not carry the
lexical-versus-structural recovery provenance required by the proposed B
decision. Untagged invalid templates explicitly rescan with reporting before
feeding this same mechanism.

Consequently, testing that error bit on a literal, checking a diagnostic
position, or selecting diagnostic codes is not by itself proof that all
recovery is confined to a literal value. The reviewer must determine a
parser-owned representation/proof for the recovered structure or recommend
the broader recovery design. This audit does not change the existing emit
refusal or presume that a narrower guard is correct.

## 10. Delivered review and Codex incorporation — 2026-09-13

The user delivered the 647-line fable-5.1 max response at
`/Users/hiramatsu/dev/tsc-rs/target/next-slices-20260913/fable-5.1-max-utf16-adjacent-review-response.md`.
Its SHA-256 is
`c62b04d60dad64bea70057e54fe70a1b724ce9896f7f728a7f9f2127b1277236`.
The review request now points to the completed response. Preserve both files;
the corrections below are an implementer addendum, not edits to the review.

The incorporation source is integration head `f0aaa2de2`. Its production
sources under `crates/*/src` are unchanged from `ed6d8073a`; the intervening
original-row fixture/test supplement is described in §8. Both frozen adjacent
fixture hashes still match §1. The integration worktree was clean before this
documentation update. No Cargo, native compiler, CI, implementation writer
agent, or merge was run during incorporation. The only executions were small
in-memory observations of the pinned TypeScript API, bounded below.

### 10.1. Accepted direction and representation corrections

Accept A's canonical WTF-8 owned JavaScript-string value and branded
`EscapedName`, with scanner token values and the five string/template literal
`text` fields owning the value. Keep `TemplateText` for literal type values.
Accept the producer, lookup, cache and output inventory in response §2 as an
extension of §9.1, with compiler-enforced classification still outstanding.
Include lossless diagnostic message ownership when adopting the name-bearing
diagnostic controls; display spelling is not an identity adapter.

Two details of the suggested Rust API must change before implementation:

1. `crates/diagnostics/src/lib.rs:1` and `crates/types/src/lib.rs:1` both
   `forbid(unsafe_code)`. Reinterpreting `&str`/`&[u8]` as a custom unsized
   `&JsStr` ordinarily needs an unsafe reference cast. The review does not
   supply an implementation compatible with these crate constraints. Use a
   safe borrowed view by value, `JsStr<'a> { bytes: &'a [u8] }`, with private
   fields and `from_str(&str)`/owned-value accessors as its public constructors.
   This retains a zero-copy UTF-8 view without changing either unsafe policy.
   The owned `JsString(Vec<u8>)` exposes no unchecked mutable byte access.
2. Canonical byte equality is UTF-16 value equality; byte **ordering** is not
   JavaScript string ordering. U+E000 is greater than U+10000 under UTF-16
   lexicographic comparison (`E000 > D800`), but its UTF-8 bytes sort before
   U+10000's bytes (`EE < F0`). Response R-A1's byte-derived `Ord` cannot
   stand in for TypeScript's `compareStringsCaseSensitive`/`compareValues`
   (`_tsc.js:910–935`). Keep `Ord` in byte order to satisfy `Borrow<[u8]>`;
   document explicitly that this is not JavaScript ordering. Provide
   `cmp_utf16()` over `code_units()` for explicit upstream string comparers,
   including on the branded name. Test that byte `Ord` and `cmp_utf16()`
   differ for this counterexample. Do not introduce identity tables using
   `BTreeMap<EscapedName, _>` or rely on default sorting for JS semantics.

Concrete proposed lookup contract: owned values and `EscapedName` implement
`Borrow<[u8]>`, with `Hash` explicitly delegated to the same byte-slice hash.
Public lookup APIs accept canonical `JsStr` views or already escaped `&str`
queries, and perform `as_bytes()` internally. Do not expose arbitrary bytes
as a public query: a noncanonical six-byte surrogate pair would silently
miss its canonical four-byte key. Both `IndexMap` and existing `HashMap` caches
with a single name key can use this contract without an unsafe cast or a new
interner. Tuple keys such as `(TypeId, EscapedName, bool)` cannot borrow
`(TypeId, &[u8], bool)` this way; retain their owned tuple lookup instead of
refactoring caches just to obtain heterogeneous borrowing. The view's
`Hash` delegates to that slice too. Do not mix Rust `str` hashing with slice
hashing or claim `Borrow<str>` for a value containing non-UTF-8 bytes. Distinct
UTF-16 values have distinct canonical bytes; this is an encoding property,
not a claim that a finite hash has no collisions.

Place `JsString`/the borrowed view in `diagnostics/src/js_string.rs` and
re-export through `tsc-types`; place `EscapedName` in `tsc-types`, where
`TypeData::UniqueESSymbol` can use it. This settles the dependency direction
without making diagnostics depend on types. Preserve explicit underscore
escape/unescape and internal/identifier constructors. Do not implement a
lossy `Display`, `Deref<Target = str>`, or identity fallback on failed UTF-8
conversion. The report's round-trip, junction concatenation, exhaustive
single-unit and selected-pair proofs remain required, now using the byte
query's actual `Borrow`/hash contract. At design-review time no such Rust
proof had run; §11 records the subsequent foundation tests.

### 10.2. B: independent recovery events, diagnostic origins and rollback

Accept admission by parser-owned facts: no committed structural recovery,
and lexical recovery restricted to string/template token kinds outside
trivia. Retain typed deferral for other recovery and preserve `noEmit` and
`noEmitOnError` ownership in the command driver. B follows A because its
declaration values depend on A.

Do not implement R-B1's proposed equality
`parse_diagnostics.len() == lexical.len() + structural_events`:

- `parser.rs::push_parse_diagnostic_with_index` (9774) suppresses consecutive
  diagnostics at the same start and returns `None`. A structural event must
  still prohibit admission when its diagnostic was suppressed.
- `create_missing_node` (1196) can create a recovery node without emitting a
  diagnostic. Conversely, wrapper calls such as `parse_expected` and
  `parse_error_at_current_token` can describe the same recovery. Count at
  defined producer boundaries, not at every layer of a reporting call chain.
- `process_leading_reference_directives` (9692) pushes diagnostics directly;
  the proposed scanner/error-wrapper inventory alone does not classify them.
  Keep these non-literal origins deferred.

Store a diagnostic-origin entry for each *retained* diagnostic and a separate
record of committed recovery events, including events with no retained
diagnostic index. The coverage invariant is
`diagnostic_origins.len() == parse_diagnostics.len()`, with valid optional
indices from events into that list. It is independent of event counts.
Admission examines all committed recovery events and all retained origins;
an unclassified origin must not pass. Ordinary optional grammar constructs
must not be mistaken for structural error recovery.

Pass origin explicitly through `push_parse_diagnostic_with_index`, the common
funnel for all three reporting callers. Only `drain_scanner_errors` constructs
lexical origins (all eleven drain callers share that path). This gives
retained diagnostics their parallel origin entry structurally at insertion.
The invariant covers `parse_diagnostics`, the list inspected by
`preflight_source` (`builtins.rs:15722`); `js_doc_diagnostics` is a separate
list outside this admission check.

`try_parse` (1273) rolls state back on failure and `look_ahead` (1293) always
rolls it back. New provenance vectors and counts must participate in both
transactions; failed speculative recovery must not contaminate the final
parse. Cover successful speculation, failed speculation, same-start
lexical/structural deduplication, silent missing nodes, reference-directive
errors, untagged template rescans and incremental parsing. Preserve the
existing separate behavior of source flags, which intentionally survive
speculation; provenance is not another source flag.
`Scanner::restore` already truncates `errors` to `error_len` (scanner.rs:514),
so provenance carried on `ScanError` follows that rollback automatically.
The parser's retained origins and committed event records still require
their own corresponding save/truncate state.

Scanner trivia origin must be recorded explicitly at the producer. The
suggestion to derive it from `pos < token_start` during `error_at` is not a
general proof: `Scanner::scan` sets `token_start` at each trivia/token loop
iteration (`scanner.rs:269`), and `error_at_with_args` (2148) can report within
that current trivia. Associate non-trivia scanner events with the completed
scan/rescan token kind and retain their original positions. Do not attach
trivia errors to the subsequent literal merely because they drain together.

### 10.3. C: observed double visit and declaration-driven numbering

Accept the shared host, parser-owned template flags, invalid-escape
`CONTAINS_ES_2018` propagation, non-hoisted numbered binding, and per-source
tail declarations. Preserve the separate upstream predicates:
`hasInvalidEscape` checks `ContainsInvalidEscape` (`_tsc.js:16267–16271`),
while `createTemplateCooked` tests the broader `IsInvalid` mask (94018).
Transport scanner template flags without reconstructing them; these two
predicates inspect different masks on the same field. Of the `IsInvalid`
bits, only `ContainsInvalidEscape` can arise on template fragments; the other bits of
`IsInvalid` are numeric-literal flags. The current Rust `create_template_cooked`
and `has_invalid_escape` (`tagged_template.rs:152/216`) share the raw-text
predicate `template_cooked_is_invalid`, so this is an actual owner/predicate
migration, not the addition of two raw-text reconstruction functions.

Reuse `Es2018Visitor::allocate_local_numbered_binding` (es2018.rs:1189) and
ES2015's `allocate_numbered_binding` (es2015.rs:1153): both already allocate
without hoisting. Only `allocate_hoisted_numbered_binding` hoists. C adds the
shared host and tail record; no new allocator is required.

The new reproducible observer and JSON receipt are beside the review:

- `codex-utf16-review-followup-probes.mjs`
- `codex-utf16-review-followup-probes.json`, SHA-256
  `92fbb23eafa07bfbf8c33de981453f32ee9c7abe8589886f288a3819c35537ca`

Run the script with `--check` to verify its saved observations. It pins both
TypeScript file hashes from §6 and records all source strings/options and
two identical `transpileModule` outputs per case. The three cases show:

| Source after `export {};` | Target | Observed output fact |
| --- | --- | --- |
| `({...o}).f` followed by a valid tagged template | ES2017 | `Object.assign` tag lowering, no extra `_a` or template-object declaration; this particular example does not witness the review's suggested temp side effect |
| `a` tagged with invalid `\unicode`, itself the tag of a valid `ok` template | ES2017 | The call uses `templateObject_2`; the tail declares `var templateObject_1, templateObject_2;` |
| Valid `first`, invalid `\unicode`, valid `last` tagged calls | ES5 | Call bindings occur as `2, 1, 3`; tails are `var templateObject_1;` then `var templateObject_2, templateObject_3;` |

The nested case closes the *upstream observability question* U-C1: both tag
visits' declaration side effects matter. Rust implementation equivalence is
still unproved. Do not let `Es2018Visitor::visit_with_value_use`'s NodeId memo
collapse those effects; choose a bounded revisit mechanism after auditing
all affected visitor state, with the nested case as its witness. Clearing
memo entries alone is a proposal, not yet a proved restoration protocol.

The §10 reviewer also supplied a positive *hoisted-temp* witness at ES2017:
`export {}; ({a, ...r} = f(), r).f` followed by a valid tagged `ok` template
emits `var _a, _b;` and uses only `_b`; an invalid `\unicode` tagged template
or the untagged expression emits only `var _a;`. Record these three controls
separately in the immutable v2 observer/receipt, retaining v1's SHA. Revisit
must preserve both tail records and `hoist_variable_declaration` effects,
including bypassing the memo-hit fast path at es2018.rs:355 for the
preliminary visit. Promote these controls to complete-command evidence.

Correct R-C6's generalization that names follow first textual use. The
printer's `emitSourceFileWorker` calls `generateNames` on source statements
*before* printing them (`_tsc.js:119753–119769`). `generateNames`
(120515–120593) traverses variable declarations, including these tails, and
`generateName` (120624) allocates/caches their names. That explains why an
ES2018 tail binding becomes number 1 even when its call is printed second.
Rust already has this pre-scan for function bodies in
`target_bindings.rs::collect_function_body_declaration_name_events`.
`collect_binding_name_events` currently skips it on SourceFile roots:
the scope branches at 1085/1149/1214/1255 require `!scope_root`, leaving root
to the child-order traversal at 1349. The ModuleBlock arm at 1255 also omits
the pre-scan, whereas upstream `emitModuleBlock` runs it (_tsc.js:119167).
Numbered names are subsequently ordered only by `(moment_path, sequence)`
(target_bindings.rs:713). Add the same declaration pre-scan at SourceFile
root and ModuleBlock in the shared collector, preserving function behavior.
Both ES2015's finalizer (es2015.rs:345) and print finalization
(transform.rs:986) use this collector; do not patch the consumers separately.
The predicted pre-repair result after C connection is calls `1,2,3` with
tails `var templateObject_2;` / `var templateObject_1, templateObject_3;`.
That prediction is not a native measurement. Use the nested and ES5 mixed
v1 cases to witness the source-root fix; also cover ModuleBlock behavior.

These six saved API executions establish neither full-command tuples nor
Rust, declaration, callback, source-map or diagnostic equivalence. The
ordering control is a JavaScript operation observation, not a Rust WTF-8
implementation test. Promote relevant cases into separate complete-command
controls before final qualification; do not change the original 23 fixtures.

### 10.4. Implementation ownership and remaining gate

Codex remains the sole implementation writer. The response's lanes are
cause groups, not authorization to delegate or concurrently edit shared
files. Use the following sequential ownership boundaries:

| Stage | Files and ownership constraint |
| --- | --- |
| A representation proofs | `diagnostics/src/js_string.rs`, diagnostics/types re-exports, `types` escaped-name type and focused tests; keep unsafe policies intact |
| A owned values and identity closure | Scanner/parser literal values; xtask generation and generated nodes/observable fields; binder symbol tables; all checker producers/lookups/caches; factory and declaration/property-name outputs; classify every reached consumer |
| A diagnostic values | Diagnostics message construction, checker display producers and explicit UTF-8/UTF-16 output transport; same writer where checker identity and display share files |
| B | Scanner provenance, parser transactions/source-file records, emitter preflight/error variants, command gate controls; revisit A-owned scanner/parser files sequentially |
| C | Parser template flags and their generated schema, transform-flag row, shared tagged-template algorithm, ES2018/ES2015 hosts, target binding finalization; same writer owns parser, codegen and `builtins.rs` overlap |

Keep cause commits reviewable without publishing an incomplete identity
migration as qualified. The full roughly 300-site escaped-name and literal
consumer classification remains an implementation prerequisite; the table
above is not a claim that an exhaustive file map has been completed.

Remaining evidence is response §5 plus the corrections here: canonical
encoding/concatenation/hash/ordering proofs; complete consumer classification;
parser transaction/diagnostic coverage controls; declaration literal-type
`NoAsciiEscaping` callback units versus emitted bytes; full C command/map
controls; original 23 exact twice; prior 64 + 41 and both original-four
evidence routes; G4a/G4b and the separately recorded G5c limit. Observe the
raw UTF-16-source lone-surrogate boundary as a separately scoped limitation.
No fixture, comparator, manifest, ratchet or CI policy changes are authorized
as a way to erase a gap. The review delivery is complete; implementation,
qualification and the final diff review remain open.

### 10.5. §10 review incorporated; implementation entry

The user delivered the §10 review against `f0aaa2de2` and explicitly judged
implementation ready once its six corrections were reflected here. All six
are now incorporated: byte `Ord`/explicit UTF-16 comparer and canonical query
boundary (including the tuple exception); existing finalizer pre-scan gaps;
the hoisted-temp witness; reuse of the existing local allocator; diagnostic
origin funnels and rollback/list scope; and the two masks over owned flags.
This satisfies the stated design-entry condition. Codex starts the single-
writer implementation with A's representation and contract proofs, followed
by the full consumer migration, B and C. This agreement does not certify
implementation behavior or waive any of the evidence requirements above.

## 11. A representation implementation and additional observations

The accepted §10 review is followed by an additive representation foundation:

- `diagnostics/src/js_string.rs`: canonical WTF-8 `JsString`, safe borrowed
  `JsStr`, UTF-16 iteration, junction-normalizing concatenation, byte `Ord`,
  explicit `cmp_utf16`, and byte-slice `Borrow`/hash. Explicit lossy output
  replaces each unpaired surrogate once; debugging preserves its value.
- `types/src/escaped_name.rs`: branded underscore escape/unescape, internal
  and already escaped identifier constructors, matching byte lookup/hash
  and explicit UTF-16 comparison. `TemplateText` gains lossless bridges.
- The initial focused tests cover all 65,536 single units, every Unicode
  scalar's exact UTF-8 representation, selected boundary pairs and triples,
  4,096 deterministic sequence controls, concatenation associativity, borrowed
  hash input equality/lookup, order differences and escaped-name round trips.

The first run (before import/initializer formatting only) built both library
test binaries and passed all 10 string and 3 escaped-name tests. Its receipt
is `target/declaration-comment-ranges-runs/utf16-a-representation-20260913-102128/receipt.json`;
it records source hashes, binaries, command exits and logs. No scanner,
parser, binder/checker identity or emitter behavior has been migrated by
this foundation; it is not evidence that any of the 23 gaps is fixed.

After formatting and adding the explicit `TemplateText`/escaped-name value
bridge control, the current foundation passed **14 focused tests (10 + 4)**
at `target/declaration-comment-ranges-runs/utf16-a-representation-20260913-102731/receipt.json`.
The saved runner is `target/declaration-comment-ranges-runs/run-utf16-a-representation.py`.
Both build and both test commands exited 0; the recorded source inputs were
unchanged throughout. Runs used `taskpolicy -b`, `nice -n 15`, two Cargo jobs,
and one test thread. Formatting and `git diff --check` also passed.

The next migration's search inventory is
`target/next-slices-20260913/utf16-escaped-name-consumers-before.json` (245
textual occurrences across binder/types/checker, including declarations and
imports). It explicitly marks classification pending; a nearest-function
textual hint is not semantic classification. Scanner/parser ownership,
identity lookup/cache migration, diagnostic values, B, C and native complete
comparisons remain open. No full CI or 23-probe rerun is claimed here.

The separately added `codex-utf16-review-followup-probes-v2.mjs` and
`codex-utf16-review-followup-probes-v2.json` sit beside the immutable v1
observer. The v2 JSON SHA-256 is
`28b126f461adf71aa07a199d9cfb21713a78d4e297a7094806f767aba69d0df9`.
All three rest-assignment controls from §10.3 were observed twice through
`transpileModule` **and** twice through the complete command driver: 6 direct
and 6 complete-command executions, exact on repetition. The latter include
JS, declarations, both maps, write callbacks/data, diagnostic values as UTF-16
units, status, emit result and actual exit. The sources intentionally retain
the reviewer's unresolved identifiers; their diagnostics are captured, not
filtered. The same one-temp/two-temp distinction is present in full-command
JS output. `--check` passed; v1's SHA remains unchanged. These are upstream
observations awaiting native comparison, not Rust qualification.

The owned-value migration also reaches two existing scanner diagnostic
producers. In the same pinned `_tsc.js`, `scanEscapeSequence` at 9094–9118
passes a padded hexadecimal replacement (`\\x01`, for example) for octal
escapes and the actual raw escape (`\\8` or `\\9`) for disallowed decimal
escapes. Rust's `scan_octal_escape` and `scan_escape_sequence` currently
omit these arguments. Carry them through `error_at_with_args`; the separate
27-input parser observation records their complete diagnostic UTF-16 values
alongside literal values. This corrects the producer, without rewriting the
original command fixtures or filtering diagnostic data.

## 12. Owned-value migration in progress

The scanner now owns `JsString`, and xtask generates that type for the five
string/template literal `text` payloads and their observable-field variant.
Diagnostic message chains, canonical heads and formatted diagnostic values
also retain JavaScript units. The binder now stores `EscapedName` in symbols,
ordered symbol tables and classifiable names. `SymbolTable` wraps its private
`IndexMap`: queries accept canonical `JsStr`/UTF-8 views and perform byte
borrowing internally; insertion requires an explicitly branded key.

The following operation classifications have been implemented in the lower
layers. This table is partial; it does not certify closure of checker,
program, emitter, adapters or the original §11 search inventory.

| Owner / consumer | Value meaning and operation |
| --- | --- |
| scanner string/escape/template producers | Arbitrary code units, canonicalized at construction and concatenation; scanner flags and error recovery choices remain separate |
| scanner numeric producers and `BigIntStringScan` | ASCII numeric spellings; private `numeric_token_value` asserts that domain before scalar numeric processing |
| parser `current_token_text` | Identifier/private-identifier, numeric/bigint and regexp grammar consumers; scalar-only conversion is justified by those producers |
| parser invalid-name diagnostic arguments | `current_token_value` preserves an arbitrary offending literal token; no scalar projection |
| JSDoc names/comment text and JSX text | Grammar identifiers or raw UTF-8 source slices; their stored scalar domain is retained |
| binder `literal_text_of`, raw text/name helpers | Borrowed `JsStr` or owned `JsString`; raw literal values stay distinct from escaped keys |
| binder declaration/access-name helpers | Escape raw literal values once; preserve parser-owned escaped identifiers, explicit internal names, private-name prefixes, numeric signs and quoted module keys |
| binder declaration diagnostics | Source spelling remains a UTF-8 source slice; synthesized-name fallback and message arguments remain `JsString` |
| binder private serial relocation | Parse only the ASCII serial, preserve the full canonical suffix, reconstruct the branded name and keep table order |
| ambient-module patterns | Split on ASCII `*` while retaining `JsString` prefixes/suffixes; do not project them to paths or display strings |
| `TypeTables::get_string_literal_type` | Canonical JavaScript query; retain the scalar cache fast path and use the existing `TemplateText` interner for non-scalars |

Two independent API observers have been added, without touching the original
23 fixtures. `scripts/observe-utf16-owned-literal-values.mjs` records the 23
original inputs plus four ownership controls (27 cases, 54 executions);
`scripts/observe-utf16-binder-names.mjs` records seven name-table controls in
fresh processes (14 executions, including process-local private serials).
The latter covers distinct surrogates, equivalent pair spellings, computed
keys, internal/user underscores, ambient module patterns, export names,
private names and signed/canonical numeric spellings. Its generated Rust
fixture is `crates/binder/tests/fixtures/utf16-binder-names.rs`, SHA-256
`3e88d20bf3821ab0ad6ab4b69f877849cb74515db49f01f45682c17bd8d7e150`.
The parser fixture is `crates/syntax/tests/fixtures/utf16-owned-literal-values.json`,
SHA-256 `1423f66adc3cec19c4afe5952b12395128869d8aa4cd1f62432c210549e14107`.
Both native comparison tests passed before formatting; the corresponding
library runs passed 42 diagnostics, 35 types and 165 syntax tests.

Checker migration has begun but has not compiled or qualified. It removes
raw-text reconstruction from template type/evaluation inputs, retains
`JsString` in constant evaluation, and brands literal-type/property-name
producers and union-property tuple keys. The reached shared
`getApplicableIndexInfoForName` operation is `_tsc.js:59479–59481`: a late
name queries `esSymbolType`; other names query the string literal type of
`unescapeLeadingUnderscores(name)`. Both existing Rust approximations are
being replaced by that shared operation; add numeric, escaped-underscore
and template index-signature controls before qualifying it. Computed-name
diagnostic display follows `getNameOfSymbolFromNameType` (55523–55540) and
`escapeString` (16273–16314): that regex does not escape lone surrogates.
Those diagnostic values therefore stay JavaScript strings, rather than
silently substituting a source-style `\\uXXXX` value.

The migration intentionally remains incomplete. A focused emitter build
currently stops at 21 program-layer type errors involving JSON/config value
transport and module-request keys. Those are real consumers to classify and
migrate, not grounds for introducing lossy conversion, dropping non-scalar
values, or adding a new refusal merely to make compilation pass. No program
transport fallback has been introduced. Checker call sites/caches, emitter
and factory ownership, diagnostic output sinks, B provenance and C transforms
still require implementation and verification. The 23-command repair and
the original final-review gate remain open.

After formatting, the owned-value runner rebuilt the five selected test
targets and ran all **244 test functions twice**, with every command exiting
0. It also checked both repeated TypeScript observations and node generation.
Receipt:
`target/declaration-comment-ranges-runs/utf16-a-owned-values-20260913-112126/receipt.json`;
runner: `target/declaration-comment-ranges-runs/run-utf16-a-owned-values.py`.
The receipt records source/fixture/observer and binary hashes, exact commands,
logs and real exits; its input hashes and head were unchanged through the
run. The five targets are diagnostics/types/syntax library tests, the
27-case parser integration comparison and the seven-case binder integration
comparison. This remains lower-layer evidence: it includes neither checker
nor command emission, and does not yet include the binder's older unit-test
target (whose scalar test adapters still need migration).

The subsequent run includes those migrated binder unit-test adapters and the
canonical-query `EscapedNameSet` boundary: **315 test functions (244 + 71),
each twice**, all commands exit 0, at
`target/declaration-comment-ranges-runs/utf16-a-owned-values-20260913-113135/receipt.json`.
The original scalar assertions and expected diagnostics were retained; name
constructors now explicitly produce `EscapedName`. This remains evidence for
the six selected lower-layer targets, not for the compiler command pipeline.

## 13. Program values and module-name migration in progress

The reached JSON boundary cannot transport arbitrary JavaScript strings in
`serde_json::Value::String`, including object keys. The program now has
`JsonValue`/`JsonObject` in `program/src/json_value.rs`; values own `JsString`
and the object keeps insertion order with a private canonical-key hash index.
Queries accept `JsStr`/UTF-8 views. The strict scalar serde fast path converts
into that representation, and the syntax converter produces it directly.
There is no sentinel encoding for surrogate identity and no new refusal or
lossy fallback for non-scalar JSON. The existing separate JSONC prototype
marker is retained, including its escape for user keys beginning with NUL.

The upstream owners are the already pinned `readJsonOrUndefined`/`readJson`
(`_tsc.js:17261–17275`) and `convertToJson` (38521–38600). A strict JSON input
rejected by serde because of a lone surrogate must still retain JSON.parse's
own `__proto__` property behavior. JSONC conversion uses JavaScript property
assignment and can set the prototype instead. Both routes now retain the
actual key/value units. `append_json_quoted` is an explicit JSON text sink:
well-formed JSON.stringify escapes lone surrogates and leaves valid scalar
characters intact. Its output is never a compiler identity key.

New independent observers and fixtures, each checked against two TypeScript
observations, are:

| Observer / fixture | Coverage | Fixture SHA-256 |
| --- | --- | --- |
| `observe-utf16-json-values.mjs` / `program/tests/fixtures/utf16-json-values.json` | Eight `readJson` inputs: distinct keys/values, equivalent pairs, numeric key enumeration, strict/JSONC prototypes, escaped marker, paths values and an unrelated surrogate description beside valid package fields; ten JSON string-quoting controls | `973c3dc348eea623f43612bbc17dc6cb0f731401780e4bd19c2eff9e1e101df2` |
| `observe-utf16-config-names.mjs` / `program/tests/fixtures/utf16-config-names.json` | Ten exact option-name and `getSpellingSuggestion` API inputs, including lone-surrogate names, literal escape spelling and a paired non-BMP value | `41b8082870bc62eca87bc8cadc3077af25a7fb1c2506268cb8a8988971151f68` |

The isolated actual JSON modules passed their eight unit tests twice at
`target/declaration-comment-ranges-runs/utf16-json-values-20260913-114336/receipt.json`.
After adding the option-name API migration, the isolated actual
`json.rs`/`json_value.rs`/`config_options.rs` modules passed **ten tests twice**,
including the unchanged full option-catalogue comparison, at
`target/declaration-comment-ranges-runs/utf16-program-value-modules-20260913-115623/receipt.json`.
The latter runner is
`target/declaration-comment-ranges-runs/run-utf16-program-value-modules.py`;
it rebuilds syntax dependencies, compiles a harness importing those actual
production modules, and records exact commands, real exits, dependency and
binary hashes. It also archives the selected input sources and fixtures,
the harness/runner and the tracked worktree patch. This is deliberately
isolated module evidence: the program library and command pipeline still do
not compile. A later iterator `Clone` bound exposes the existing cloneable
iterator to resolver consumers; it is outside that recorded source hash.

Additional operation classifications now being carried through program are:

| Owner / consumer | Representation and operation |
| --- | --- |
| `ResolutionKey`, `TypeReferenceResolutionKey`, missing-resolution records | Preserve original module/type-name units as `JsString`; source host paths remain a separate field; internal table order is byte order |
| `ProgramConfigFile` name/value location indices | Canonical JS keys and queries, including automatic type names and written string values |
| `PathMapping`, raw paths violations and selected package captures | Owned JS patterns/substitutions; star offsets and captured bounds are UTF-16 positions |
| `matching_paths`, `compare_pattern_keys`, `select_package_map_target` | JS prefix/suffix comparisons and substring semantics, including pair-splitting bounds and upstream overlap behavior; target expansion/host probing still needs migration |
| `ConfigOptionBag` raw names, typed object properties and raw syntax names | Preserve arbitrary keys; declared typed option names remain scalar only after exact lookup in the ASCII schema |
| Config option cache identity, JSON ToString and path diagnostic values | JS concatenation through nested values; JSON quoting occurs only at serialization |
| Config option declaration/spelling lookup | Exact canonical query; the existing UTF-16 edit-distance algorithm consumes the owned units. Non-scalar names are still eligible for suggestions |
| Root/relative predicates, star counting and numeric object-key enumeration | ASCII grammar operations over canonical values; numeric parsing is scalar only because valid indices have an ASCII decimal spelling |

The name/pattern migration requires comparisons which can match the lead or
trail unit of a canonically encoded pair. `JsStr::starts_with_js`,
`ends_with_js` and `substring` therefore operate on UTF-16 units; scalar-only
prefix/suffix operations retain the byte fast path. Two new contract tests
cover selected triples, every substring boundary (including clamping and
swapping), mismatches, pair-half matching and scalar separators around lone
surrogates. All **44 diagnostics library tests passed twice** at
`target/declaration-comment-ranges-runs/utf16-a-owned-values-20260913-115226/receipt.json`
(`selected_targets: [diagnostics-lib]`). This is not a rerun of the other five
owned-value targets at that source version.

The whole-program check at this point recorded **103 remaining type errors**
in `target/next-slices-20260913/utf16-program-check-4.stderr`. The subsequent
check-5 recorded 73, before the later package-name and replacement-operation
changes below. Neither count is a current clean-build claim.
Remaining work includes config/path normalization and option projections,
module/package target expansion, loader consumers, checker/emitter/factory
closure, diagnostic output adapters, B, C and their full command controls.
Neither the JSON observations nor the new string-operation tests qualify
module resolution. The original 23 input/expected hashes are unchanged; no
comparator, manifest, ratchet or CI-policy adjustment has been made. The full
repair and final review remain open.


### 13.1. Replacement operations, package values and the host boundary

`program/src/js_string_ops.rs` now owns the existing first-star/all-star
replacement operations. `$&`, `$$`, prefix/suffix substitution and unknown
replacement tokens operate on canonical JavaScript values. The output-byte
preflight counts canonicalization at fragment joins before allocation;
`diagnostics::JsStringByteLength` shares the same lead/trail seam contract.
Scalar inputs retain the existing linear preflight, and the existing native
resource limit and error messages are unchanged. `resolution_error.rs` is
an unchanged extraction of the resolver's error types, allowing the actual
replacement module to be tested independently of unfinished host consumers.

The added observer `observe-utf16-string-replacement.mjs` records 14 cases
with first and global replacement, twice. Its fixture is
`program/tests/fixtures/utf16-string-replacement.json`, SHA-256
`cc440934216089384ff42a976385f81f3c7ad44bd869e9ecb673801112bb5950`.
The actual JSON, option, replacement and error modules passed **12 tests
twice**, with every command exiting 0, at
`target/declaration-comment-ranges-runs/utf16-program-value-modules-20260913-121808/receipt.json`.
The expanded canonical byte-length and split-boundary contracts passed
**45 diagnostics tests twice** at
`target/declaration-comment-ranges-runs/utf16-a-owned-values-20260913-121923/receipt.json`
(selected target: diagnostics-lib). Each receipt retains unchanged input
hashes/head; these runs remain isolated evidence.

Resolver JSON ToString, array joining and `typesVersions` iteration now
retain JavaScript values. The latter iterates individual UTF-16 units, so a
pair split by JavaScript array-like indexing is not replaced by U+FFFD.
Package metadata names/versions and PackageId components now own JsString.
Finite ASCII option/condition/version grammars classify non-scalars as
nonmatching grammar values while retaining the original value elsewhere.
Module target expansion, config/path projection and downstream PackageId
consumers remain unfinished.

A real-host probe against the pinned TypeScript found that `./\uD800`,
`./\uD801` and `./\uFFFD` can read the same physical replacement-character
file while retaining three different `resolvedFileName` values. A `paths`
substitution that erases the wildcard also resolves a non-scalar request to
an ordinary scalar `mapped.ts`. Thus neither a lossy module key nor early
NotFound before substitutions is correct. The current ProgramPath/host API
accepts Unicode scalar paths; the scope question about extending that host
boundary was still pending at this checkpoint (the subsequent assumption and
implementation are recorded in §23). No new lossy adapter or refusal had been added.
Independent parser work continues while that boundary is unresolved.


### 13.2. Package-name cache and diagnostic consumers

The loader's package-name facts now use canonical JsString keys, and type
reference inclusion/error context retains the original JS name. The shared
located-diagnostic producer formats JavaScript arguments directly. The
internal `pathsBasePath` getter keeps its scalar host-directory type, with an
explicit assertion justified by its sole producer; arbitrary raw options do
not pass through that projection.

`js_string_ops::types_package_name` follows getTypesPackageName and
mangleScopedPackageName (`_tsc.js:42070–42080`), including their first-slash
replacement and unchanged `@scope` when no slash exists. Its separate
12-case observer/fixture is `observe-utf16-package-names.mjs` /
`program/tests/fixtures/utf16-package-names.json`, SHA-256
`74ca399bb5b7a230361210b25b88cf7235bc694bb50b553e35f41ee96392343c`.
It covers distinct surrogates, scoped names, extra slashes, valid pairs and
literal escape spelling without invoking host resolution. The expanded
actual-module harness passed **13 tests twice**, every command exit 0, at
`target/declaration-comment-ranges-runs/utf16-program-value-modules-20260913-124233/receipt.json`.
Input sources/fixtures and binaries are archived and hashed as before.

The subsequent whole-program checks remain unsuccessful: check-6 has 67
errors, check-7 has 49. The latter is
`target/next-slices-20260913/utf16-program-check-7.stderr`, recorded before the
final package-helper extraction and borrowed-condition correction. Remaining
config/path and module-target transport is not covered by the isolated
harness. The whole compiler, A closure, B activation, C and the original
full-command/final-review obligations all remain open.


### 13.3. Lexical paths retain JavaScript values before the host

`program/src/js_path.rs` now supplies the shared getRootLength,
combinePaths and getNormalizedAbsolutePath operations on canonical values
(`_tsc.js:5349–5390`, 5474–5487, 5493–5567). The existing scalar resolver
normalizer delegates to this implementation and retains its host validation;
its scalar result assertion is justified by scalar input segments and ASCII
syntax removal. Raw config file-path option/list values use the canonical
entry directly. Config-dir template matching/substitution and both option
value phases are shared pure operations in the same module. They preserve
TypeScript's case-insensitive marker recognition followed by case-sensitive
replacement, including the dotless-i example. No OS conversion happens here.

The independent path observation exposed a required UTF-16 operation in
`normalizedUpTo - 2`: a first relative segment consisting of one surrogate
unit has three bytes but only one JavaScript unit. TypeScript's short-prefix
branch retains that segment in this case. The implementation now performs
that arithmetic and last-slash bound in UTF-16; ordinary delimiter searches
and canonical substring boundaries remain byte operations. The failing native
observation was retained, and the expected fixture was not changed.
`JsString::truncate_bytes` checks whole WTF-8 boundaries before mutation; it
cannot split an encoded pair and leaves invalid offsets unchanged.

New repeated observations are:

| Observer / fixture | Scope | SHA-256 |
| --- | --- | --- |
| `observe-utf16-lexical-paths.mjs` / `program/tests/fixtures/utf16-lexical-paths.json` | 231 path/base combinations; roots, combination and normalization; relative, disk, UNC, URL, mixed separators, NUL, pair and distinct surrogate values | `f834de406197aa7b41f864d2d54461b53ab776d3ed9ddaefb8e6edec15dbcae7` |
| `observe-utf16-config-path-values.mjs` / `program/tests/fixtures/utf16-config-path-values.json` | 51 parseJsonConfigFileContent outDir value projections; both raw option and configDir finalization phases, with no host/output-plan qualification | `8749ec55605333c71013563931bc0601b00af8ab3b3cee122c245d9dfaa5a13b` |

The actual-module harness passed **14 tests twice** after the path arithmetic
correction at
`target/declaration-comment-ranges-runs/utf16-program-value-modules-20260913-125235/receipt.json`.
With the shared config-dir value functions, it passed **15 tests twice**, all
commands exit 0, at
`target/declaration-comment-ranges-runs/utf16-program-value-modules-20260913-125459/receipt.json`.
Both runs archive the actual production module sources, inputs and binaries;
none compiles the full config parser, program or command pipeline.
The canonical truncation contract and existing diagnostics library tests also
passed **46 tests twice**, all commands exit 0, at
`target/declaration-comment-ranges-runs/utf16-a-owned-values-20260913-125657/receipt.json`
(selected target: diagnostics-lib). This is not a rerun of the other owned
value targets.

The last program check, check-8, still has **40 type errors** in
`target/next-slices-20260913/utf16-program-check-8.stderr`. Subsequent producer
migration keeps runtime-dependency JSON keys and selected package entry values
as JsString/JsStr. Config project-reference path/originalPath projections also
retain JavaScript values; execution's existing project-reference limitation
is unchanged. These later glue changes have not been checked as a whole.
Raw config specs, config option projections, module target/host transport,
checker/factory/emitter consumers, B activation and C remain unfinished. The
file-name/host-scope question from §13.1 remains pending; this pure-value work
does not decide it or qualify host resolution.

## 14. B parser facts before the preflight connection in §19

`syntax/src/recovery.rs` now defines retained diagnostic origins and separate
committed recovery events. `push_parse_diagnostic_with_index` takes an
explicit origin and records every reporting attempt, including attempts
suppressed at the same diagnostic start. A missing node with a message uses
that reporting event; a silent missing node records one separate structural
event. Ordinary optional grammar creates neither event. Only scanner drain
constructs lexical origins; the unterminated-comment producer marks its
trivia kind explicitly, and other scan/rescan errors use the completed token
kind. Scanner restoration retains its existing error truncation behavior.

Parser and JSDoc transactions save/truncate the origin and event records with
their diagnostics. Moving JSDoc diagnostics to js_doc_diagnostics removes
these temporary syntactic facts too. Top-level await reparsing required a
further owner: it keeps the same old diagnostic ranges, copies origins and
remaps event indices. Suppressed/silent structural events use their producing
source element, since an error position can point at the next statement.
Incremental reuse also rejects a range containing committed recovery,
including silent events whose node has no diagnostic error flag; fresh and
incremental comparisons now include the recovery facts.

SourceFile exposes read-only recovery facts and `has_only_literal_recovery`.
The predicate requires complete retained-origin coverage and accepts only
string/template lexical events, with no diagnostic-code or fixture allowlist.
The original 23 inputs all fit that parser boundary. At this checkpoint the emitter preflight
was **not yet switched** (the later source change is in §19): A's declaration values, B's command gate controls,
`noEmit`/`noEmitOnError`, and complete-command comparisons remain prerequisites
for qualifying the admission change.

New syntax observations are at
`scripts/observe-utf16-recovery-boundary.mjs` and
`syntax/tests/fixtures/utf16-recovery-boundary.json` (20 cases, 40 executions),
SHA-256 `9f7f3e1c40df92fb3b968c8044654ce884447e599a23cb747d9f35a3a5576462`.
The fixture explicitly distinguishes observed TypeScript diagnostics from
reviewed Rust admission policy. Its native comparison passed all 20 cases,
including all template fragment rescans, numeric/identifier/regexp/trivia/JSX
exclusions, mixed structural/lexical recovery, await reparsing, reference
errors and separate JSDoc diagnostics. Direct private-state tests additionally
cover both deduplication orders, silent nodes, successful/failed speculation,
lookahead and scanner rollback. The existing syntax tests also pass with
recovery equality added to incremental comparisons. The subsequent archived run passed **247 test functions twice** (173 syntax
library, one 27-case owned-value comparison, one 20-case recovery comparison,
71 binder library and one seven-case name comparison), with all commands
exiting 0. Receipt:
`target/declaration-comment-ranges-runs/utf16-recovery-provenance-20260913-123601/receipt.json`;
runner: `target/declaration-comment-ranges-runs/run-utf16-recovery-provenance.py`.
It archives inputs, runner and worktree patch, hashes the actual binaries,
and verifies unchanged input hashes/head. Three repeated TypeScript observers,
node generation and diff checks also pass. No command-level qualification is
implied here.

## 15. C template flag transport in progress

The scanner now exposes its flags masked by `TemplateLiteralLikeFlags`
(7176). Both parser fragment entry points save that word before advancing the
scanner, including untagged invalid-template rescans. The generated Node
header owns `template_flags`; other node kinds initialize it to zero.
Subtree copying and both emitter factory clone paths retain it. The factory
update path already goes through clone_node. Fresh/incremental syntax
comparison includes the word, and a new edit/reuse test covers valid and
invalid head/middle/tail escape changes.

The mask also retains valid Unicode, extended-Unicode and hex-escape bits.
Only the intersection with **IsInvalid** is restricted to ContainsInvalidEscape;
that restriction does not describe the entire template word. TypeScript's
`getTransformFlagsOfTemplateLiteralLike` (`_tsc.js:22862–22868`) sets ES2018
whenever the word is nonzero. The parsed-tree transform-flag row now follows
that condition. The emitter has not been compiled or qualified at this stage.
Its shared tagged-template host, raw-predicate replacement, ES2018 tail
records/revisit and SourceFile/ModuleBlock finalizer changes remain open.

The new observer `scripts/observe-utf16-template-flags.mjs` records 43 cases
twice (86 parses) against the pinned TypeScript, including valid/invalid
escapes, distinct fragment positions, trivia, unterminated tokens, untagged
rescans, type arguments, await reparsing and non-template flag isolation.
The fixture `crates/syntax/tests/fixtures/utf16-template-flags.json` has
SHA-256 `65d617b0deb84330b6fb9bc4e03ad1e5bc2afff4bf2833286c52e8661e3edcb3`.
It separates syntax values/flags/diagnostics from transform observations;
the native syntax comparison qualifies only the former. No transform flag
or command comparison is inferred from its success.

The archived run passed **175 test functions twice** (174 syntax library
tests and the one 43-case native template comparison), all commands exit 0:
`target/declaration-comment-ranges-runs/utf16-template-flags-20260913-131248/receipt.json`.
Runner: `target/declaration-comment-ranges-runs/run-utf16-template-flags.py`.
The run archives source/fixture inputs, runner and worktree patch, hashes
actual binaries, verifies unchanged inputs/head, and checks the four relevant
TypeScript observers plus node generation and diff whitespace.

Node generation now lives in the production module
`crates/xtask/src/node_codegen.rs`, shared with the normal xtask entry points;
four unchanged input/output helpers are in `codegen_common.rs`. This allows
the actual generator to compile and run independently while the wider
compiler migration prevents building all xtask dependencies. The scoped
runner `target/declaration-comment-ranges-runs/run-node-codegen.py` imports
those actual modules, archives and hashes its sources/harness/serde library
and binary, and records generation/check outputs. It does not copy generator
logic or claim a whole-xtask build. The first generation and check passed at
`target/declaration-comment-ranges-runs/node-codegen-20260913-131012/receipt.json`;
the syntax receipt links the subsequent check after formatting. Existing
node-generation outputs other than the Node header's new word are unchanged
by this C step. The old saved xtask binary cannot check the new generator;
future scoped runners now invoke the shared-module runner instead. Existing
receipts remain untouched.

The user will request Claude's final implementation review. The separate
[review request](h2-8a-utf16-adjacent-implementation-review.md) is prepared as
a draft with the A/B/C checks, immutable inputs and required evidence. Its
target commits, complete diff and final receipts remain explicitly unfrozen
until implementation and qualification finish. This document preparation
does not close A, B activation, C, the original 23-command gate or final review.

## 16. C lowering connected; native qualification remains blocked by A compilation

`tagged_template::TaggedTemplateHost` now separates the shared algorithm
from each transformer's visit operations and binding/declaration lifetime.
ES2015 uses All; ES2018 has a TaggedTemplateExpression arm using
LiftRestriction. The common cooked producer reads parser-owned JsString
and tests `template_flags & IsInvalid`. The admission predicate reads
`ContainsInvalidEscape` on the same fragment fields. The raw scanner
reconstruction has been removed. Raw text uses the existing explicit
factory raw-value channel when present, otherwise the parsed raw text;
CR/CRLF normalization preserves other UTF-16 units. The ordinary string
factory and code-unit string factory now write canonical owning text.
Other A factory/printer consumers remain unfinished.

ES2018 reuses its existing `allocate_local_numbered_binding`, records plain
declarations in the visitor's source-owned tail vector, and appends one tail
variable statement after the source lexical environment has been merged.
The statements array preserves its range through update_node_array. Each
source creates a fresh visitor, so the tail vector's lifetime is one source.
These are implementation facts, not a successful native-output comparison.

The bounded revisit now disables **both reads and writes** of the ES2018
node and array memos while the shared tagged-template algorithm runs. Its
ordinary first tag visit and the valid-path visitEachChild therefore execute
the real visitor twice; no hoist/tail effects are replayed manually. Nested
tagged-template calls retain the disabled state, and both success and error
restore the previous memo mode. The surrounding normal visit may cache the
final tagged node result after this scope exits. Existing memo entries are
neither consumed nor overwritten during the bounded scope.

No transformation state is rolled back between those two visits. Allocator
counters, helper requests, hoisted declarations, source tail records and
context metadata retain their ordinary effects. value_use, function_stack,
async-generator super-capture stacks and iteration depth use the existing
visitor's entry/exit handling on each pass. The function/arrow and deeper
nested controls below are intended to test those interactions; that state
audit alone does not establish equivalence.

The shared target-binding collector now runs its existing declaration
pre-scan at SourceFile root and ModuleBlock, including a directly printed
ModuleBlock root. Its declaration walker accepts SourceFile/ModuleBlock
statement arrays. Both transformer and printer finalization use this
collector; the function-body pre-scan remains. This should preserve the
declaration-driven 2,1,3 numbering through subsequent ES2015 and print
finalizers, but the native witness has not executed yet.

### 16.1. Added observations and native checks awaiting compilation

The original v1/v2 review JSON files are copied byte for byte into separate
test fixtures `utf16-tagged-template-review-v1.json` and
`utf16-tagged-template-review-v2.json` under `crates/compiler/tests/fixtures`.
Their original SHA-256 values remain `92fbb23e…` and `28b126f4…`; neither
original observer nor receipt was edited.

`scripts/observe-utf16-tagged-template-controls.mjs` promotes all six parent
cases and adds ten controls: function-local rest hoists, a function-local
nested tag with source tail records, ES2015 mixed tags, ES2018 retention,
script-mode non-caching, cooked surrogate values, an escaped valid tag,
four nested visits, a function-expression tag and namespace composition.
Each of the **16 cases** has two direct API observations and two complete
commands (32 of each). The v1 direct outputs agree exactly with the parent;
the v2 direct and complete observations agree exactly with their parent.
All callback values/data, declarations, maps, reported/emit diagnostics,
status and exit fields are retained; unresolved identifiers are not removed.

The new fixture is `compiler/tests/fixtures/utf16-tagged-template-controls.json`,
SHA-256 `e8d6d1fa07d16d1887df1cb26dbbd51c150f42407a73e495239f7ccfd2a62688`.
Initial generation and --check both exited 0; receipt:
`target/declaration-comment-ranges-runs/utf16-c-controls-observation-20260913-133220/receipt.json`.
The independent native target `h2_8a_utf16_tagged_template_controls.rs`
compares the full saved command object twice and can archive both actual
captures using `TSC_RS_UTF16_TAGGED_CONTROLS_CAPTURE_DIR`. Diagnostic strings
retain UTF-16 as well as their UTF-8 encoding. The callback strings in these
specific controls are scalar; this does not prove arbitrary callback-value
transport. **This native target has not compiled or run.**

Namespace command lowering does not by itself exercise a retained ModuleBlock
printer arm. A separate printer observer,
`scripts/observe-utf16-generated-declaration-order.mjs`, marks two generated
identities and prints uses in reverse order before their tail declarations.
Its SourceFile, unlowered ModuleBlock and function-body cases all produce
calls 2 then 1 and declarations 1 then 2, twice. The fixture
`emitter/tests/fixtures/utf16-generated-declaration-order.json` has SHA-256
`91c849e197418b466bffb3569f6b0a80bd1ce87f7ca2eedf4d4cc8db3d955dd3`.
The corresponding actual-printer test is in `target_bindings_tests.rs`.
An additional emitter test uses §15's 43-case fixture to check fragment
transform flags, the root ES2018 gate, and ordinary/cross-source factory
clones. **Both emitter tests await compilation and execution.**

The five existing raw escape decision-table tests moved from the deleted
transformer reconstruction to `syntax/tests/template_escape_flags.rs`.
Their input/expected decisions are retained and now query the parser-owned
flags. All five passed twice, all runner commands exit 0, at
`target/declaration-comment-ranges-runs/utf16-template-flags-20260913-132931/receipt.json`.
That selected run is syntax evidence only; it does not run the new emitter
or command tests.

### 16.2. Current build boundary

The normal offline emitter check exited **101**, stopping in program with
**34 errors** from the unfinished A value migration. It did not type-check
the emitter changes. Receipt, archived source inputs and full compiler log:
`target/declaration-comment-ranges-runs/utf16-c-emitter-check-20260913-132524/receipt.json`.
This replaces check-8's older 40-error count as the last ordinary build
observation; the reduction comes from the already recorded program producer
changes. Config option/spec projections, package-target expansion and the
module/path host boundary still prevent reaching the emitter. No fallback
or new refusal was added to bypass those errors. Checker closure, B command
activation, C native tests, all original 23 commands and the final review
remain open.

## 17. Config option/name consumers; the original three repairs remain unqualified

The user re-confirmed the three concrete targets during implementation:
(1) distinct unpaired surrogates must not collapse into duplicate symbol
names (TS2300/TS1117), (2) H2.9 literal-only parse recovery must permit emit,
and (3) the ES2015-target invalid-cooked tagged-template case must reach the
ES2018 LiftRestriction lane. Their status is unchanged as a qualification
claim: binder identity controls pass, but complete A command comparison is
pending; at this checkpoint B recorded parser facts but its emit gate was not activated (see §19 for the subsequent source change); C is
connected in source but has not yet passed the ordinary emitter build or
native command controls. Scanner decoder tests do not qualify B admission.

The current owned-value migration includes CompilerOptions string values,
ModuleSuffix entries, ProgramOptions.types, retained ConfigSpec/extends
values, config discovery option values and wildcard-directory names.
Scalar `lib` catalogue values remain String: the private converted-list
producer chooses from the fixed ASCII catalogue and invalid entries become
Undefined. Arbitrary raw lib strings do not pass through that projection.
Automatic type names, inclusion/option diagnostics and JSX implicit runtime
import construction now retain JsString. Path normalization, config-dir
substitution and inherited spec rebasing retain canonical values before a
host operation; inherited raw strings are indexed by UTF-16 unit.
This exposes downstream path/output consumers which still require migration.
It does not establish whole-program compilation or resolve the pending host
boundary scope question recorded in §13.1.

The public config wildcard matcher now accepts canonical JsStr or scalar
strings and compiles UTF-16 units. Its existing DP, non-Unicode RegExp case
rules and package/dot/min.js exclusions are preserved. Delimiter splitting
uses `JsStr::split_ascii`, which cannot cut a WTF-8 code point. Filename
casing applies the existing TypeScript host profile to scalar runs and
retains unpaired surrogates as context boundaries; it performs no host I/O.

A scalar-only validity test would be wrong for JSX entity-name option
values: TypeScript accepts `a/*\uD800*/` with a raw unpaired unit in the
comment. `syntax::is_entity_name_js_text` keeps the original owner unchanged
and delegates grammar validation to the normal parser, replacing unpaired
units only in comments in its temporary scanner input. Such units outside
comments cannot form a valid entity name. Delimiters, line breaks, escapes,
qualification and unfinished-comment validation remain parser-owned. The
projected input never becomes a symbol name or diagnostic value.

| Added observation | Scope | Fixture SHA-256 |
| --- | --- | --- |
| `observe-utf16-entity-names.mjs` / `syntax/tests/fixtures/utf16-entity-names.json` | 129 entity/identifier validity cases, three targets, each observed twice; raw surrogate comments, line terminators, qualification, escapes and pairs | `f5c4314d8da980e1744e7e2d7f7f0bc5503580217b724db16f52618ac50edaee` |
| `observe-utf16-config-matching.mjs` / `program/tests/fixtures/utf16-config-matching.json` | 924 files-pattern comparisons and 17 filename casing values, each observed twice; distinct surrogates, replacement characters, pairs, wildcard unit width and casing context | `224f6640c510355b9d24891bb223a658ae3f9a343f520287ce95e2fc3ae693bf` |

Native syntax library tests (174) and the 129-case entity predicate test
passed twice at
`target/declaration-comment-ranges-runs/utf16-template-flags-20260913-140615/receipt.json`.
The actual program module harness, now including config_matcher, passed
24 unit tests twice at
`target/declaration-comment-ranges-runs/utf16-program-value-modules-20260913-140818/receipt.json`.
The latter includes both new pattern/casing tests and existing matcher
regressions. Its first attempt failed on a new test's relative include path;
the path was corrected without changing expected data, and the failed run
was retained. These are isolated production-module tests, not program,
resolver, checker, emitter or complete-command qualification.

The ordinary Cargo check at
`target/declaration-comment-ranges-runs/utf16-program-options-check-20260913-135505/receipt.json`
failed with 114 program errors after the option type transition. After the
consumer work above, the ordinary emitter check at
`target/declaration-comment-ranges-runs/utf16-program-check-20260913-140934/receipt.json`
still exited 101 with 55 program errors. Source inputs were unchanged during
both checks and are archived. A missing import and canonical comparison
were then corrected; remaining current-source build evidence must supersede
that receipt. Error-count changes are migration feedback only. All original
23 command tuples, B activation, C native checks, final classification,
regressions and final review remain required.


## 18. Factory/printer cooked-value ownership in progress

The factory's legacy cooked-value reconstruction has been removed:
`TransformArena::literal_value` borrows the five canonical payload values,
and `literal_code_units` is only their UTF-16 transport. A missing source
range, original link or raw spelling no longer means the value is unknown.
The separate `LiteralNodeProperties.javascript_string_value` field and its
getter/setter were removed from all production and test uses. Quote choice,
textSourceNode and explicit factory raw-template spelling remain separate
properties. `set_literal_value` updates only the cooked payload of a string
literal/template fragment, after checking node identity and kind.

String/template factory construction from UTF-16 units now owns canonical
JsString. Enum constants, JSX attribute synthesis, ES2015 string/template
lowering and bundle prologue equality read or construct that owner directly.
The printer's synthetic string and cooked-template branches pass the owned
UTF-16 units into the existing quote/escape functions and UTF-16 TextWriter.
Original-token copying and raw-template selection retain their separate
upstream spelling rules. No NoAsciiEscaping byte-output success is claimed.

Module export names now own JsString; their scalar Deref/AsRef interface
was removed. Existing-node publication literals copy this owned value and
retain textSourceNode solely for lexical spelling. Only the branches proven
to contain identifier nodes or a scalar synthesized-identifier constructor
project to &str. Downstream export lookup/cache consumers still need migration
and type checking. Generated module basenames were subsequently migrated as recorded below.

Native literal capture tests now inspect the actual owned payload instead
of injecting/reconstructing a lossless side value. Their oracle fixtures and
comparisons are unchanged. Factory seam assertions now require values even
without source provenance, and distinguish a new U+FFFD from a surrogate.
The lifecycle test uses an actual literal node for cooked-value cloning;
its statement node continues to exercise emit metadata. These test changes
have not yet been compiled or run. Normal emitter validation remains behind
program compilation: the latest ordinary check at
`target/declaration-comment-ranges-runs/utf16-program-check-20260913-142551/receipt.json`
exited 101 with 52 program errors and unchanged archived source inputs. Later
helper signature edits are not covered by that receipt. Nothing here closes
A, B, C, the original commands, regressions, final classification or review.

The later basename projection uses `program::base_file_name` on canonical
JS values and `builtins::generated_module_name` applies the upstream ASCII
`/\W/g` replacement to UTF-16 units. A scalar pair therefore contributes two
underscores to the generated base. This projection produces an ASCII base
for the generated-binding owner; it is not a replacement for module identity.
The 33 repeated TypeScript observations are in
`program/tests/fixtures/utf16-generated-module-names.json`, SHA-256
`2dc87e508a7ae717d386c8799f80da171ebb8beb2d26ec1e66fd405d58ddb7bd`.
The actual program module harness passed 25 tests twice at
`target/declaration-comment-ranges-runs/utf16-program-value-modules-20260913-142948/receipt.json`.
Only the basename half has native execution evidence; the emitter generated
base test remains uncompiled.

## 19. B preflight connected in source; native admission evidence pending

`builtins::preflight_source` now calls `SourceFile::has_only_literal_recovery`
unconditionally. It retains `ParseDiagnosticsDeferred` for a rejected source.
Empty retained diagnostics do not bypass suppressed structural reports,
silent missing nodes or incomplete retained-origin coverage. The preflight
does not erase diagnostics or parser facts, and JSDoc remains a separate list.
The shared 20-case recovery fixture drives an emitter preflight test with the
same JS/TSX parser options; a second test clears retained messages from a
structurally recovered source and requires refusal even with count zero.
Neither new emitter test has been compiled or run.

This is a source-level connection, not qualification of B. A's declaration
values, noEmit/noEmitOnError, the complete commands and output/exit evidence
remain mandatory. One existing legacy-decorator test adapter explicitly
clears parse_diagnostics before passing a recovered tree through the full
script pipeline (`active_transform_contract.rs::transform_and_print_legacy_decorator_recovery_at_target`).
The new invariant exposes that adapter's bypass; its stage-isolation
contract needs review when running existing regressions. Do not weaken the
production predicate or change expected outputs to preserve the bypass.

## 20. Checker name/diagnostic consumers and resolver transport in progress

Flow query and loop-cache keys now own JsString. Synthetic and parent element
access chains use EscapedName. Composite keys append canonical name values
between the same upstream ASCII separators; private-name description cuts
use ASCII delimiters, retaining the escaped description as an EscapedName.
The tuple-key and name-key caches retain their respective lookup contracts.

`class::property_name_for_property_name_node` and its effective-name consumer
return EscapedName. Object literal, class and inherited-property collision
maps retain those keys. Interface/type-literal duplicate property checks
retain raw JsString values and pass the name to canonical diagnostic
construction. Numeric-name recognition projects only values that can equal
the scalar output of NumberToString. Literal numeric-name reads borrow the
owned AST text rather than copying or reconstructing it.

The checker diagnostic family now has explicit borrowed-JS entry points:
`diagnostic_for_node_js`, `create_error_js`, `error_at_js`,
`error_at_with_related_js`, `lookup_or_issue_error_js`, and related-info
adapters. Scalar entry points retain their callers while sharing construction
where applicable. `MessageChain::new_js_parts` formats borrowed canonical
parts directly into the owned message. Duplicate declaration reporting and
the amalgamated cross-file name map retain canonical names, including the
multi-name message and its related information. Class member messages and
quoted missing-member lists retain their JS values. Spelling-suggestion
wrappers feed canonical name units into the existing UTF-16 distance engine;
the distance algorithm and tie ordering are unchanged.

A new checker test requires three different lone-surrogate/replacement
message values to occupy distinct diagnostic entries even though their UTF-8
outputs coincide. Repeated canonical queries and the scalar U+FFFD query
must find the appropriate existing entry. Checker code and this test remain
uncompiled. These edits are not a completed escaped-name-site inventory.

Diagnostics library tests passed 46 tests twice after the borrowed-message
constructor change, with all scoped commands exit zero:
`target/declaration-comment-ranges-runs/utf16-a-owned-values-20260913-144710/receipt.json`
(selected target: diagnostics-lib). This receipt does not cover checker or
emitter changes.

Resolver imports/exports target expansion and legacy target normalization
now return canonical JsString; the existing scalar normalization wrapper
delegates to a JS-value worker. Package grammar checks, slash joining,
replacement and trailing-separator preservation happen before a host-path
conversion. The existing infrastructure error has a native path display
only for scalar spellings; successful values never use that projection.
The ProgramPath/host interface itself is unchanged and the scope question
in §13.1 remains pending. Downstream target transport is still incomplete.

The ordinary emitter Cargo check at
`target/declaration-comment-ranges-runs/utf16-program-check-20260913-145618/receipt.json`
exited 101 with 56 program type errors and unchanged archived source inputs.
The added errors expose downstream consumers of the newly canonical target
results. The emitter and checker were not type-checked. All final commands,
regressions, classification, byte-output controls and final diff review remain
open; the Claude request remains a draft.

The next transport edit corrected a code-unit/byte mismatch at
`ModuleResolver::resolve_using_optional_settings`: `matching_paths` returns
UTF-16 capture bounds, but its consumer still sliced a Rust str with those
numbers. Captures now use canonical `substring`, including a capture of
only the trailing unit of a pair. Optional-target normalization retains the
result as JsString. This does not decide the host-path boundary: the added
three resolver examples all end at scalar host filenames, including a
wildcard substitution with no `*` that erases the non-scalar capture.

The pinned TypeScript resolveModuleName API observed those three examples
twice with identical result and callback records, exit zero, at
`target/declaration-comment-ranges-runs/utf16-path-capture-observation-20260913-150529/receipt.json`.
Observation stdout SHA-256:
`9c985b8e5c85887b9add102eea90677390a43dafa9218de618318172e1e3d92b`.
The new native test
`module_resolution_contract::paths_captures_are_utf16_ranges_including_a_split_surrogate_pair`
is uncompiled. This is resolver API evidence, not complete-command evidence.
The subsequent ordinary emitter check, with unchanged archived inputs,
exited 101 with 59 program errors:
`target/declaration-comment-ranges-runs/utf16-program-check-20260913-150705/receipt.json`.
It supersedes the prior build observation, but still does not reach emitter
or checker type checking. The original two fixture hashes were rechecked
and remain exactly the §1 values.

Declaration property-name generation is also being connected to the owner:
`node_builder/chains::create_property_name_for_identifier_or_literal` accepts
canonical values, selects identifiers/numerics only after scalar grammar
validation, and otherwise creates an owned StringLiteral. The nameType path
converts TemplateText directly to JsString and shares that selection with
the raw escaped-symbol fallback. A new checker test compares both routes
for two lone surrogates, U+FFFD and a pair, inspecting the actual AST owner.
Synthetic-scope name lookup now borrows canonical names as well.

The same file's length/first-character operations now consume UTF-16 units.
`strip_symbol_name_quotes` retains canonical units and follows `stripQuotes`
plus `/\\./g` (_tsc.js:16340, :53369). This also corrects its previous
mismatched-quote stripping, dropped final backslash and treatment of line
terminators as regexp-dot matches. A focused unit test covers those rules,
escaped lone units and a pair. These checker tests are uncompiled/unrun;
there is no declaration-byte or NoAsciiEscaping qualification from them.
Entity-name display producers, module specifiers, and other downstream
checker/builder consumers still require migration and ordinary type checking.

## 21. Canonical name-resolution and diagnostic consumers (unqualified source work)

`resolve::resolve_name` and its suggestion entry now accept canonical borrowed
names. The private scope walk, exact lookup, spelling pass and failed-name
diagnostics retain that view. Identifier-only source-spelling comparisons
borrow their scalar text as JsStr; primitive/constructor checks and static lib
metadata remain finite-name comparisons. Canonical diagnostic construction
also covers the suggestion's plain canonical head and related information,
UMD-global and type-only alias reports. A new resolver unit test distinguishes
D800, D801, DC00 and FFFD through the real globals lookup and exclude-globals
branch. This is a synthetic name-resolution API test, not a source grammar or
complete-command witness.

`check::entity_symbol_name_as_written_slice` and its nameType producer now
return JsString. String/number nameType values use the existing canonical
double-quote escape operation shared with `merge::symbol_name_as_written_slice`.
This follows `escapeString` at _tsc.js:16311–16314 and the nameType caller at
:55523–55539: isolated surrogates remain units in the result. Quoting and
unique-symbol/negative-number brackets append canonical values. The builder
property-name test also inspects these written-name values; downstream
entity-name synthesis and the separate type-display String pipeline remain
unfinished and must not be inferred to preserve arbitrary values yet.

The enum element-access evaluator now looks up its EscapedName as a canonical
view. Enum comparison's known string and its diagnostic arguments retain
JsString through the relation error stack, using the pinned escapeString
operation at _tsc.js:64709–64722. The new JS argument funnel preserves the
existing incompatible-stack flush, elision, parent-skip and revision behavior.
An unrun unit test inspects the escaped enum-value diagnostic units before
UTF-8 output, including a quote, newline and distinct lone units.
Access-control property names, uninitialized-property reports and missing-member
suggestion chains also use the JS diagnostic constructors. The unresolved
augmentation index's container path is a Vec<EscapedName>, matching its export
table producer and receiver-symbol path; filesystem source identities are
unchanged.

The property-access spelling predicate reads UTF-16 units directly. ToNumber
accepts canonical input and rejects non-scalar strings because StringNumericLiteral
and its surrounding whitespace contain no isolated surrogate. Its scalar
trim now uses the existing `syntax::is_js_whitespace` table, correcting Rust
trim's U+0085/U+FEFF difference. A focused unit test covers those characters,
U+200B and lone units. Numeric-name round-trip equality compares the canonical
input with the scalar NumberToString result.

Only rustfmt and `git diff --check` have run on this source work. No new Cargo
check or native checker test ran: the latest ordinary build observation remains
the 59-error program failure at §20, and repeating it without program edits
would not reach checker or emitter. The ProgramPath/host scope question in
§13.1 is still unanswered and those interfaces were not changed. The original
23 commands, downstream migration, regression checks and final review remain
open. The Claude request remains **準備中・未提出**.

## 22. Package-map subpath transport; ordinary build still fails

The imports rewrite continuation's Target/Sequence subpaths now retain the
JsString supplied by `select_package_map_target`. `resolve_selected_export`
borrows that value canonically through array/condition recursion. The raw
package target's suffix predicate and input-remapping diagnostic entry also
retain JS views; the ambiguous-root diagnostic uses `new_js_parts`. Concrete
target paths and rewritten bare specifiers still have downstream scalar
consumers. No host adapter, lookup refusal or lossy identity conversion was
introduced to satisfy those consumers.

An ordinary offline emitter check ran after this program edit, with jobs=2
and unchanged archived inputs/head. It exited 101 with **53 program errors**:
`target/declaration-comment-ranges-runs/utf16-program-check-20260913-154424/receipt.json`.
This supersedes §20's 59-error build observation; it still does not typecheck
emitter or checker and provides no native qualification for §21. No full CI
ran. The host/path interfaces and original adjacent fixtures remain unchanged.

## 23. JS path ownership and explicit host boundaries

The optional §13.1 question had not received an answer. On continuation,
Codex told the user it would proceed on the assumption that preserving module
identity includes this path/host boundary. This is an implementation assumption,
not a newly received user approval or part of the earlier reviewer verdict.
The scope is no longer being left unresolved while other consumers are edited.

`ProgramPath` now owns its display spelling as JsString, and `CanonicalPath`
owns the canonical JS value. The new JS constructors validate empty/NUL paths
without rejecting lone surrogates. Existing native constructors still reject
unrepresentable native OS paths. `display()` and `as_js()` expose canonical
views; implicit Display and AsRef<Path> conversion of the identity were removed.
`make_program_path` accepts JS values and applies the shared filename case
profile before creating that owner. An unrun path-identity integration test
distinguishes three compiler paths with identical UTF-8 projections and checks
scalar constructor equivalence. Its existing expectations were adapted only
to the new value API.

`CompilerHost` has seven required JS query methods, so every host adapter must
choose its implementation explicitly. Existing native query methods remain
for scalar native callers. The filesystem host performs UTF-8 replacement at
the actual I/O boundary, after compiler normalization/substitution. Returned
directory entries retain the requested JS parent spelling; realpath returns
the observed physical spelling. I/O errors retain both native context and the
requested JS path. The memory host instead keys files, directories, realpaths
and injected failures by canonical JS values. Its JS builder accepts those
values without filesystem conversion. Its native result adapters require a
scalar result; compiler callers handling arbitrary paths must use the JS API.

The filename case operation moved from program's pure path module to host,
without changing its scalar-run/lone-unit behavior; program delegates to it.
For native component grammar in the memory host, a temporary scalar spelling
replaces each three-byte lone unit by an ordinary three-byte scalar. std::path
only supplies borrowed component offsets; the stored parent/child values are
sliced from the original canonical value. That temporary spelling is never a
query, key, diagnostic or returned filename. The added memory control includes
both the temporary scalar U+E000 and lone units to catch accidental collisions.
The host dependency on diagnostics is explicit in Cargo.toml and Cargo.lock;
no unsafe code was introduced.

Native host evidence (not compiler command evidence):

- `utf16-host-boundary-20260913-155716/receipt.json`: all 24 host tests passed
  twice at the initial host edit, unchanged archived sources/head.
- `utf16-host-boundary-20260913-160132/receipt.json`: all **25 host tests passed
  twice** after retaining JS paths on I/O errors and adding its control. The
  two integration controls cover distinct lone units/pairs/replacement/private-use
  values, casing, listing order, failure identities, aliases, and actual file
  reads/listings through the UTF-8 I/O boundary. Existing scalar host tests also
  pass. Inputs/head remained unchanged and diff check passed.

Both directories are under `target/declaration-comment-ranges-runs/`; the
runner is `run-utf16-host-boundary.py` and invokes real offline cargo test on
all host targets with jobs=2 and test threads=1. These are native tests on the
current platform; no Windows run or end-to-end module resolution is implied.

The subsequent ordinary emitter check now exposes the callers requiring path
classification: **265 program errors**, exit 101, unchanged archived inputs,
`target/declaration-comment-ranges-runs/utf16-program-check-20260913-160232/receipt.json`.
That supersedes §22's 53-error observation. The additional errors follow the
intentional owner/interface change, and are unfinished migration, not repaired
behavior. ConfigCompilerHostAdapter, preparation/loader/resolver callers,
diagnostic source-name transport, checker and emitter still need completion.
No original adjacent fixture, comparator, manifest, ratchet or CI policy was
changed. No full CI ran; the 23-command qualification and Claude submission
remain open.

## 24. Prepared path consumers and extension matching in progress

Prepared source/auxiliary/package display aliases now own JsString. Their
deduplication compares canonical views. Extensionless-root candidate matching
uses UTF-16 prefix/substring operations and canonical concatenation of the
recognized suffix. Declaration-file suffix classification and diagnostic-name
slash comparisons no longer require scalar paths. ProgramConfigFile retains
its diagnostic filename/path as JS values; the downstream Diagnostic filename
fields still need migration.

PreparationError now retains a JS path alongside its native/display error
context. Prepared validation and ProgramPath's JS validation use that owner;
the original native constructor still preserves invalid native OS-path errors.
Error-message formatting is an explicit UTF-8 display operation and does not
create lookup keys. The unrun path-identity test also checks the original
surrogate/NUL path on a validation error.

ModuleExtension's current logical extension representation is unchanged, but
path matching and moduleSuffixes reconstruction now accept canonical JS paths
and suffix values. The prior scalar-host-path assumption was removed. Case
folding uses the shared JS filename profile and suffix comparisons use JS
code-unit semantics. The existing JsString fallible reservation method retains
the previous allocation-failure branch. Arbitrary logical extensions and other
resolver consumers still need classification; this is not full path support.

Intermediate ordinary check `utf16-program-check-20260913-161159` stopped on
an accidentally duplicated reservation method; it was removed in favor of the
existing method. `utf16-program-check-20260913-161232` then reported 211 program
errors. After resolving the remaining preparation-error display sites,
`target/declaration-comment-ranges-runs/utf16-program-check-20260913-161442/receipt.json`
exited 101 with **201 program errors**, unchanged archived inputs. No errors
were reported for prepared.rs or resolution.rs in that observation; the
program crate still failed, so those files and their new tests remain
unqualified. The subsequent JS validation-error owner/test edit is unrun.
The latest host receipt's archived inputs were rechecked against the current
host/diagnostics sources and still match. No compiler command or full CI ran.


## 25. Source/diagnostic filenames and binder module identity

Diagnostic.file_name, Diagnostic.file_path, RelatedInfo.file_name and
syntax::SourceFile.file_name now own JsString. Parser construction, JSON
parsing and incremental propagation retain that value. The existing scalar
Diagnostic constructor delegates to a canonical constructor; canonical
producers use new_js directly. Borrowed String/JsString inputs to owned
constructors copy the same value, without a scalar projection.

Diagnostic comparison uses explicit UTF-16 ordering for file paths and
related filenames. FormatDiagnosticsHost additionally accepts JS-keyed source
maps and a JS current directory. Lookup, slash normalization, path reduction,
relative location text and final formatting retain JS units. A legacy
scalar-keyed source map can only match scalar keys; no non-scalar query is
converted to a replacement character to search that map. The added test
selects three different source texts for D800, D801 and FFFD filenames,
including related information and an alternative slash spelling.

Native evidence, with exact source archives and unchanged-input receipts:

- `target/declaration-comment-ranges-runs/utf16-source-paths-20260913-163046/receipt.json`:
  diagnostics **48** and syntax **184** tests pass twice. This includes all
  syntax targets, the existing recovery/template/scanner controls and a new
  TS/JSON filename preservation test. The 101 existing parser-test filename
  arguments were converted from String construction to JsString construction;
  their values and expected results were not changed.
- `target/declaration-comment-ranges-runs/utf16-binder-paths-20260913-164210/receipt.json`:
  binder **73** tests pass twice. External source-module names now remove the
  known extension from the JS filename and quote that same value into an
  EscapedName. The new control distinguishes D800/D801/FFFD module names and
  checks the original filenames on duplicate-declaration diagnostics.
- Initial source-path run `162919` failed on those 101 old argument types;
  initial binder-path run `164126` failed on the new test's temporary options
  lifetime. Both failed receipts remain on disk; the later runs above contain
  the fixes. No failed run is counted as a pass.

Program-side source work also carries JS paths through library-directory
containment, output/common-directory calculations, config diagnostic
locations, source inclusion reasons and rootDir/module-format diagnostics.
The pure output-directory functions now accept/return JS values; their
consumers must retain those values through the actual output boundary.
ResolutionError and config infrastructure errors retain canonical requested
paths. Native-only error display context remains an output/error context,
never a successful resolved identity. JSON manifest fallback parsing receives
the original JS filename. New output-directory and normalization-error tests
are present but remain **unrun** while program fails to compile.

The latest ordinary emitter dependency check is
`target/declaration-comment-ranges-runs/utf16-program-check-20260913-164340/receipt.json`:
exit **101**, unchanged archived inputs, **171 program errors** (loader 86,
module_resolution 71, config 14). The preceding `163522` check had 174.
There are no current errors reported for prepared.rs, output_directories.rs,
module_requests.rs, library.rs or json.rs, but a failed crate build does not
qualify these files. ConfigParseHost/its resolver adapter, loader error and
path owners, and resolver request/cache/host consumers still need migration.

The two original 23-probe fixture hashes were rechecked and still match §1.
No repaired full-command comparison, prior compiler regression run, final diff
review or full CI has completed. A/B/C remain unqualified at the compiler
boundary, and the Claude review request remains a draft.


## 26. Config, loader and resolver JS path transport

The §23 scope assumption now reaches the ordinary program dependency build.
ConfigParseHost accepts canonical JS queries directly, and CompilerConfigHost
uses the CompilerHost JS callbacks for discovery, reads, directory checks and
realpath. Include/exclude buckets, files-before-directories traversal, case
profiles and error precedence retain their existing control flow. Discovery
sorting is explicit UTF-16 order. ConfigCompilerHostAdapter retains the
previous optional-callback behavior while its required reads/existence probes
carry the original JS path.

ConfigSourceText, ConfigRootPlanRequest, ConfigRootPlan, extended-config cache
keys and source names, the active extends stack, option/spec defining bases,
and pathsBasePath now own JS values. Relative spec rebasing, configDir
substitution, extends probes, resolved config paths and circularity diagnostics
retain those units. ProgramPathMappings shares the declaring JS base with its
paths table. Include-pattern root reasons retain a JS config filename instead
of formatting that name into a scalar string.

The loader accepts JS roots from config and preserves its native PathBuf root
entry points with validation at their existing input-processing position.
Source/type/library queries, wildcard automatic type discovery, package probes,
case-insensitive keys and source inclusion diagnostics now use JS values.
ProgramLoadError and limit failures retain a canonical js_path alongside the
native display/error context. No host failure was reclassified as a lookup
miss. The native library-catalog input remains an explicit native boundary;
its normalized program path and subsequent library probes are JS values.

ModuleResolver now carries JS containing files, directories, module/type
specifiers, active request/continuation state, package roots and cache keys.
It issues JS host queries, including package reads and realpath. baseUrl,
rootDirs and default/custom type-root traversal share that domain. The
ordinary suffix-free tryFile hit still borrows its candidate; only suffix
expansion owns a new value. Package-name parsing and extension-family slicing
use ASCII delimiter boundaries, while rootDirs longest-prefix selection and
withPackageId/input-output reverse mapping use UTF-16 lengths and substrings.
The latter two previously used from_utf16_lossy after slicing; they now keep
the sliced JS value. Arbitrary logical extensions also own JsString, and
ModuleExtension::as_js forces consumers to classify that value. Case matching
and moduleSuffixes reconstruction retain the arbitrary extension units.

Native evidence (all offline, at most two build jobs, one heavy command at a
time, exact source archives and unchanged-input receipts):

- Latest combined program receipt:
  `target/declaration-comment-ranges-runs/utf16-program-paths-20260913-172834/receipt.json`.
  **42 library tests + 2 config-path tests + 3 module-path tests pass twice**;
  both real cargo commands exit 0. This is the actual production program
  crate and its dependencies, without stubs or extracted substitutes.
- New `crates/program/tests/utf16_config_paths.rs` controls distinguish D800
  and D801 extended config sources/cache entries and inherited option origins,
  preserve the effective pathsBasePath/outDir, and retain D800 in a circular
  extends diagnostic. `utf16_module_paths.rs` controls distinguish D800,
  D801 and FFFD package queries/cache entries under a surrogate cwd, preserve
  suffix candidate versus realpath, retain an arbitrary extension, and carry
  the original path on a typed host failure.
- The library run includes the frozen JSON/config-matching/path controls,
  added discovery test, output-directory tests, library priorities and option
  validation. Existing scalar test values and expectations were preserved;
  their construction/call sites were migrated to the new value types.
- Earlier native receipt `utf16-program-paths-20260913-172426` passed the five
  new integration tests twice. Library receipt `172554` failed to compile 47
  old test arguments; after preserving their values in the new types,
  `utf16-program-lib-20260913-172648` passed 42 tests twice. The combined
  `172834` receipt above supersedes those source snapshots for this subset.
- Ordinary emitter dependency checks `170030`, `171057`, `171329`, `171609`
  and `172129` retained their failures during the owner migration. The latest
  ordinary check,
  `target/declaration-comment-ranges-runs/utf16-program-check-20260913-172231/receipt.json`,
  completed program checking and reached emitter, then exited **101 with 198
  emitter errors**. It does not build checker. Subsequent program cleanup,
  formatting and test argument changes are covered by `172834`; emitter has
  not been rechecked after that cleanup.

These 47 tests are **not all program integration targets**. Existing custom
hosts and integration callers in program, harness, conformance and compiler
still need their canonical API migrations and regression execution. The
emitter/checker migrations, exact 23 complete commands twice, prior compiler
regressions and final diff review remain open. The original two 23-probe
fixture hashes were rechecked and still match §1. No expected outputs,
comparators, ratchets or CI policy were changed. No full CI ran. All three
user-reconfirmed defects remain unqualified at the full compiler boundary.
The Claude request is still a draft for the user to submit after completion.


## 27. Emitter name consumers: initial connection

Class-field lowering's AssignedClassName::Literal now owns JsString through
__setFunctionName construction. Its generated-identifier selection remains a
scalar identifier grammar check: an isolated surrogate cannot satisfy
isIdentifierText, but it remains intact in the distinct runtime name value.
The ES2015 and class-field prologue comparison values now own JsString, and
ES2018's property-name-to-string-literal path takes a canonical JS slice.
The property-access identifier branch explicitly reads identifier text.
Factory's observable-field traversal recognizes the new JsString field
variant as a value rather than a child reference.

ES2015 accessor pairing now uses EscapedName identity. The vendored
getPropertyNameForPropertyNameNode (_tsc.js:15861–15887) escapes leading
underscores for literal names as well as carrying already-escaped identifier
names. The previous Rust literal branch omitted that operation. Both raw
string-literal and computed string-literal branches now preserve the JS value
and apply the same name escape.

Added focused controls:

- `scripts/observe-utf16-accessor-pair-names.mjs` uses the pinned TypeScript
  createProgram emitter, with ES5 JavaScript output only. It observes a mixed
  identifier/quoted `__x` accessor pair and separate D800/D801/FFFD accessor
  pairs, twice each. `--write` and `--check` both passed.
- New fixture `crates/emitter/tests/fixtures/utf16-accessor-pair-names.json`:
  SHA-256 `2359fda0cb20343a913ff6dd39368eb4fc198439d9222fc13e5ba1b27bef920d`.
  It is separate from the original immutable 23 commands. This observation
  excludes diagnostics/declarations and is not complete-command evidence.
- The real ES2015/generator projection test in
  `crates/emitter/tests/unit/es2015/tests.rs` compares both entire JavaScript
  outputs, but remains **uncompiled/unrun** until emitter builds.

The latest ordinary check is now
`target/declaration-comment-ranges-runs/utf16-program-check-20260913-173627/receipt.json`:
exit **101**, unchanged archived inputs, **183 emitter errors**, with program
passing. The earlier `173322` check had 185 errors, including two caused by an
incorrect match-pattern edit; that edit was corrected before `173627`.
No errors are currently reported for these ES2015/ES2018/class-field/factory
connections, but the failed emitter crate does not qualify them. Program's
47-test `172834` snapshot is unchanged by these emitter-only edits. Checker,
source-map/output path consumers, remaining emitter name consumers, existing
integration callers and all final compiler comparisons remain open.


## 28. Module publication values and further emitter consumers

The ordinary emitter check now reaches **60 emitter errors** (exit 101), down
from the 183 recorded in §27. This remains a failed crate check and does not
qualify any emitter output or checker behavior. No original 23-command after
capture has been produced.

Ownership and consumer classification in this step:

- `builtins.rs::CommonJsModuleInfo` stores local/export lookup keys and the
  export-location tuple in canonical JsString. `unique_exports` and these
  tables are HashSet/HashMap; output order still comes from the existing
  vectors and source traversal. Their private borrowed lookups use canonical
  bytes. The tuple lookup owns both components. ModuleExportName comparison
  uses its JS value while preserving its syntax provenance for emission.
- `builtins.rs::ImportBinding` retains the JS property value and original
  property node. An original quoted name remains an element access. Only the
  internally synthesized ASCII `default` property lacks a source node and
  takes the scalar identifier constructor. AMD dependency path values own JS
  strings; GeneratedModuleNameAllocator still generates scalar identifier
  spellings using the existing UTF-16-based module-name projection.
- `builtins.rs::identifier_text_owned` explicitly accepts Identifier nodes
  for declaration/local/hoisted names and identifier property access. Literal
  property, export and module values continue through their JS-valued helper.
  This is syntax classification, not scalar conversion or replacement of a
  literal value. Extended identifier-escape normalization reads identifier
  text only after the identifier-specific predicate succeeds.
- `builtins/system.rs::SystemModuleInfo::collect` groups canonical module
  specifiers through a HashMap into the original insertion-ordered dependency
  vector. Export-star exclusion values and object-rest exclusion values own
  JS strings through string-literal construction. Hoisted local bindings stay
  in the identifier domain. Resolved external-module names still depend on
  the unfinished external-module/output-path migration.
- `builtins/relative_imports.rs::ImportCallRewrites::append` consumes the
  scanner-owned literal JS value directly. `rewrite_relative_module_specifier`
  removes only an ASCII extension at a proven canonical byte boundary and
  preserves the prefix. The existing extension/admission policy is unchanged.
- `builtins/generators.rs::property_name_text` and legacy decorators'
  `AccessorPropertyNameIdentity::Static` now use EscapedName, including their
  literal escape and already-escaped identifier branches. Their literal
  constructors and `flatten_destructuring.rs::create_string_literal_from_property_name`
  preserve the JS value. The ESNext constant string constructor was migrated.
- JSON/JavaScript routing in builtins, printer and declaration orchestration
  examines only ASCII suffixes. It does not project the whole source filename
  into a scalar string. Declaration diagnostics carry the original JS filename
  through Diagnostic::new_js.

New controls:

- `scripts/observe-utf16-module-name-values.mjs` captures the pinned TypeScript
  createProgram JavaScript emit for 12 cases (CommonJS, AMD and System; quoted
  re-export identities, D800/D801/FFFD dependency grouping, local export
  aliases, and relative extension rewriting including DC00). Each result was
  observed twice; separate `--write` and `--check` invocations both passed.
- New fixture `crates/emitter/tests/fixtures/utf16-module-name-values.json`:
  SHA-256 `76d67e1afec5e0b3ae6950204e6e1954a0eccd19a6a8f0c5d3f047ae29854771`.
  It records JavaScript output only, excluding diagnostics/declarations and
  complete-command evidence. Existing immutable fixtures were not edited.
- `tests/unit/builtins/tests.rs::module_transforms_preserve_js_name_values`
  compares those entire outputs through the real type/module transforms and
  printer with the existing unit-suite resolver projection. It is not a
  checker/full-compiler test and is **uncompiled/unrun**.
- `tests/unit/lib/tests.rs::relative_module_rewrite_keeps_utf16_name_identity`
  adds D800, D801, DC00, FFFD and a surrogate-pair control. The prior scalar
  rewrite test keeps the same values/expectations with its Option adapter
  migrated. These native tests also remain **uncompiled/unrun**.

Archived ordinary checks, all exit 101 with unchanged inputs:

| Receipt under target/declaration-comment-ranges-runs | Emitter errors |
| --- | ---: |
| utf16-program-check-20260913-174741/receipt.json | 110 |
| utf16-program-check-20260913-175016/receipt.json | 67 |
| utf16-program-check-20260913-175244/receipt.json | 66 |
| utf16-program-check-20260913-175518/receipt.json | 60 |

The last check archives the current production sources, checks the ordinary
emitter crate and its dependencies without substitutes, and does not include
checker. The earlier 47-test program receipt `172834` still matches every
archived input; this emitter-only step did not rerun or change that subset.
The original 23 input/expected fixture SHA-256 values were rechecked and still
match §1. No full CI, comparator, ratchet or CI-policy change occurred.

Remaining emitter errors are in JSX/standard-decorator name owners and
external-module, output, declaration and source-map path consumers. In
particular, JSX's raw reactNamespace recovery spelling must not silently
become React when it is non-scalar. Standard decorators' helper-variable
selection must classify names by the vendored getHelperVariableName
(_tsc.js:99214–99221): a non-identifier literal uses `member`; runtime context
and assigned-class names retain the literal JS value. These observations are
next-step requirements, not claims that these paths have been implemented.
Checker migration, all existing affected integration callers, native emitter
controls, the original 23 commands twice, prior compiler regressions and the
final diff review remain open. The three user-reconfirmed defects remain
unqualified at the full compiler boundary; the Claude packet is still a draft.


## 29. Output-path, source-map, JSX and decorator value owners

The ordinary emitter crate and its real dependencies now **pass cargo check**:
`utf16-program-check-20260913-182617/receipt.json`, exit 0, unchanged inputs.
This command does not check binder/checker or run emitter tests. The original
23 complete-command after captures remain absent; their qualification gate is
unchanged. All paths below are relative to `target/declaration-comment-ranges-runs`.

Changes since §28:

- `host.rs::EmitHost` / `EmitSource`, `artifact.rs::EmitArtifact`,
  `outcome.rs::SourceMapObservation`, the emit filesystem/sink protocol and
  output/declaration plans now carry canonical JsString/JsStr filenames.
  External-module names, declaration reference paths, module-specifier host
  queries, callback filenames and source-map input-source lists use the same
  values. Native filesystem/CLI callers still require migration and validation;
  the emitter-only check does not prove those boundaries.
- `source_map.rs::SourceMapGenerator` owns JS-valued file, sourceRoot,
  directories, sources, source contents and names. Its private borrowed table
  lookup uses canonical bytes. JSON encoding uses `program::append_json_quoted`
  only at serialization. Path-component operations retain JS values; ASCII
  delimiters supply canonical byte boundaries, while prefix removal at a JS
  string length uses UTF-16 substring. Unicode root case comparison uses full
  uppercase, preserving unpaired units, and filename case folding preserves
  the existing TypeScript special-character profile.
- `execute.rs::encode_uri` retains the original JS value in
  `EmitFailure::MalformedSourceMapUrl` when JS encodeURI rejects a lone unit.
  The error displays `URI malformed`. A pinned TypeScript createProgram
  source-map emit also throws URIError for `/project/D800.ts` (D800 denotes
  the actual unpaired code unit). This Rust compiler error route remains
  unqualified. Inline JSON source maps bypass this URI conversion as before.
- `syntax::FileReference`, `TypeReferenceDirective`, `AmdDependency.path` and
  program's planned path/lib references own JS values. The parser's raw source
  pragma producers convert their scalar text at this ownership boundary.
  Library suggestion/edit-distance calculations retain the original JS name;
  only finite catalog lookup projects its grammar to scalar text. Triple-slash
  reference and AMD-dependency comments use the UTF-16 comment writer.
  This does not establish support for raw unpaired units in source text.
- JSX option/runtime-module/file-name owners and entity decoding retain JS
  values. `syntax::parse_entity_name_components` shares the isolated parser
  with option validation and returns parsed identifier values, including
  escaped identifiers and comments containing lone units. JSX's raw invalid
  reactNamespace remains observable in emit. A synthetic non-scalar recovery
  identifier stores the original value in `EmitMetadata::recovery_identifier_text`,
  preserved by metadata merge and written as UTF-16; its scalar payload is
  empty and no lexical resolver query is made. Final review must trace this
  recovery node through all later transforms and confirm that the empty
  payload never becomes a semantic name or a generated-binding identity.
- Standard-decorator runtime property/assigned-class names own JS values.
  `decorator_helper_stem` separately implements getHelperVariableName's
  identifier grammar: numeric, computed and non-identifier string names use
  `member`. Generated names do not donate an identifier stem. Assigned runtime
  values are no longer carried as potential lexical static-accessor receivers;
  the only expansion caller is a private decorated accessor with a descriptor,
  whose forwarding getter/setter calls that descriptor with `this`.

New immutable observations (each TypeScript observation twice, separate
`--write` and `--check` both exit 0):

| Observer / fixture stem | Scope and SHA-256 |
| --- | --- |
| `utf16-source-map-values` | 7 generator cases, 7 URI values, 1 compiler URIError witness; `1d4c81bf2d6fa6570d7a66a428d426c40bb92bc8192f4fa37ce168bdb8c75ad4` |
| `utf16-jsx-name-values` | 11 JSX emits: raw namespace identity, parsed factory comments/escapes, runtime import values, development filename, attribute entities; `f15007f3091e011380b08afb917978aa1c2cee1b5605805891f208db3a98ded4` |
| `utf16-decorator-name-values` | 6 ES2021/ES2022 emits: literal member names/helper stems, assigned decorated class names, static private accessors; `4eb9cbc95bd40a51ef91ec3e9b5196dcaa05b0b51766172d947690d2b9164b32` |

Observers are `scripts/observe-<stem>.mjs`; fixtures are in
`crates/emitter/tests/fixtures/<stem>.json`. These are focused API/JavaScript
observations, not full command tuples. Native replay tests were added to the
real emitter library suite; their execution is still pending at this entry.
The JSX replay compares printed UTF-16 units, not replacement-character text.

Updated native lower-crate evidence:
`utf16-program-paths-20260913-182806/receipt.json` runs program and syntax
library targets plus `entity_names`, `utf16_module_paths` and
`utf16_config_paths`. Each repetition passes 42 program library + 5 program
path + 175 syntax library + 2 entity tests = **224 tests, twice exit 0**.
Inputs and head remain unchanged; diff check exits 0. This supersedes the
previous 47-test snapshot for the files changed in this step, but is not a
claim that all program/syntax integration targets ran.

Intermediate check receipts are retained: `180432` (126 emitter errors),
`180727` (74), `181056` (4 syntax producer errors), `181141` (2 program errors),
`181304` (2 program errors), `181341` (24 emitter errors), `182331` (7), then
`182617` (0), all with prefix `utf16-program-check-20260913-` and unchanged
inputs. Initial lower test build `utf16-program-paths-20260913-182723` failed
on two old AMD-dependency test constructors; only their type adapters changed
before the passing 182806 run.

The first ordinary emitter library-test build,
`utf16-emitter-lib-build-20260913-183126/receipt.json`, exits 101 with
111 type errors in existing test hosts/projections (including cfg(test)
parsed_metadata). Its production dependencies build; no library tests ran.
Those test adapters are being migrated without changing frozen expectations.
Checker, compiler callers, emitter native replays, the original 23 commands
twice, previous regressions and final diff review remain open. Claude's packet
is still **準備中・未提出**.


### 29.1. First native emitter execution and identified differences

`utf16-emitter-library-20260913-183618/receipt.json` builds and executes the
ordinary emitter library target: **499 passed, 5 failed**, exit 101,
unchanged inputs/head and diff check 0. This is the first native emitter
execution after the ownership migration. Both new source-map/URI replay test
functions passed (7 generator and 7 URI cases). Existing library regressions
passed; all five failures are added review controls, not comparator changes.

- ES2015 accessor identity is distinct, but emitted literal spelling differed
  in escape-letter case. `createExpressionForPropertyName` explicitly restores
  the cloned literal's original parent upstream (_tsc.js:27339–27347).
  Rust now copies that parent in this worker, without changing general
  cloneNode parent semantics. The computed branch also uses its expression's
  original range/parent, as upstream does.
- The generated-declaration-order test had only attached IDs to parsed nodes.
  The TypeScript witness actually constructs synthesized identifiers. The test
  now supplies synthesized flags/ranges before printing; the same expected
  source/module/function ordering remains fixed.
- The new JSX projection's resolver lacked the existing undefined-factory-root
  query implementation. It now uses the library's LegacyScriptJsxResolver for
  these fixtures, whose source has no lexical factory imports. This remains a
  transform/printer projection, not checker evidence.
- AMD dependency arrays were incorrectly passing through relative-extension
  rewriting. collectAsynchronousDependencies (_tsc.js:110461) takes
  getExternalModuleNameLiteral directly; the extra rewrite was removed from
  that producer. CommonJS require/expression rewriting remains its own path.
- Standard decorator literal names need computed access in the context object
  even when the source member has a non-computed literal name
  (_tsc.js:99835 onward). The existing context literal carrier now covers both
  kinds, preserving the member's original syntax. The same control also found
  a target-only private-helper reorder in the printer: it moved `in` behind
  get/set at ES2021 despite this TypeScript witness requesting `in` first.
  That extra reorder was removed; equal-priority helpers preserve request order.
  Earlier compiler regressions must still verify this change's broader effect.

The changes above are under repeat native execution; they are not yet recorded
as passing. Additional failed library build `183410` had six remaining test
adapter errors, followed by the executable `183618` snapshot. No original
fixture, ratchet, manifest or comparator changed; both original fixture hashes
were rechecked against §1. The checker fallback module-specifier-host delegate
has also been migrated to the JS path protocol, but is **not compiled** yet.

### 29.2. Native emitter library passes twice

`utf16-emitter-library-20260913-185507/receipt.json` records the ordinary
emitter library suite: **504 passed, twice exit 0**, unchanged inputs/head,
diff check 0. Source-map/URI (7 + 7 cases), JSX (11), standard decorators (6),
module names (12), ES2015 accessors (2), and generated declaration order
(SourceFile / ModuleBlock / function, each printed twice) all pass in this
native suite. Existing emitter library tests pass on the same snapshot.
This does not qualify checker, compiler, or the original 23 complete commands.

Intermediate receipts `184544` and `184942` each retain 503 passes and one
failure. The first showed the witness was reusing parsed identifiers as
generated names: the second print then treated generated spellings as source
collisions. The final witness creates fresh synthetic nodes, shared across
uses just as TypeScript does, leaving parsed nodes immutable. The second
failure exposed the default printer's dormant ModuleDeclaration gate. The
direct-printer witness now explicitly enables declaration syntax, allowing
the retained namespace just as the upstream printer does. Expected output
and production admission gates are unchanged by these test corrections.

Checker compilation is now the next integration boundary. The source-map
compiler URIError witness, final raw UTF-16 file/input/output boundaries,
original 23 commands twice, older compiler regressions and final diff review
remain open; the three reported defects are not yet fully qualified.

## 30. Checker integration: diagnostic arguments and name-key owners

Ordinary `cargo check --offline -p tsc-rs-checker` now reaches the real checker
after building its production binder/emitter dependencies. Intermediate
receipts, all under `target/declaration-comment-ranges-runs/`, preserve failed
builds and source snapshots rather than treating an error-count decrease as
test evidence:

| Receipt stem | Result |
| --- | --- |
| `utf16-checker-check-20260913-185542` | 623 checker errors, exit 101, inputs unchanged |
| `utf16-checker-check-20260913-190415` | 509 checker errors, exit 101, inputs unchanged |
| `utf16-checker-check-20260913-190836` | 428 checker errors, exit 101, inputs unchanged |
| `utf16-checker-check-20260913-191144` | 388 checker errors, exit 101, inputs unchanged |
| `utf16-checker-check-20260913-191652` | 366 checker errors, exit 101, inputs unchanged |
| `utf16-checker-check-20260913-192057` | 326 checker errors, exit 101, inputs unchanged |
| `utf16-checker-check-20260913-192632` | 271 checker errors, exit 101, inputs unchanged |

Implementation in this step:

- Name-bearing node diagnostic arguments use the existing JS-value entry
  points, including deprecation suggestions and the argument lists retained
  by relation errors. The deferred incompatible-property stack and its
  constructed property path retain `JsString` through the diagnostic boundary.
- Structural exclusion sets, union key-property caches, contextual
  discriminators and combined-signature parameter identities retain
  `EscapedName`. Internal symbol constants enter through `internal`, while
  actual identifier payloads use `from_identifier_escaped_text`. Numeric
  tuple-position queries borrow canonical decimal bytes internally.
- `getElementOrPropertyAccessName` and `getNameFromImportAttribute` now
  escape literal values at their producer, as `_tsc.js:15134–15145` and
  `19376–19378` require. Identifier payloads already carry escaped text.
  The former Rust branches copied literal values without the leading-underscore
  escape. This change therefore needs controls for leading `__` as well as
  distinct surrogate values when checker execution is available.
- Scalar IdentifierData consumers use syntax's scalar unescape worker.
  This is a grammar classification, without converting arbitrary symbol or
  literal values to scalar text. `getUnresolvedSymbolForEntityName` retains
  its scalar entity-chain key because every link is an identifier produced
  by that worker. Other module/symbol identity paths remain JS-owned work.
- Display serialization of synthesized string names and template text now
  accepts canonical JS views and applies the existing UTF-16 escaping worker.
  This is an actual source-text serialization boundary; it does not qualify
  literal type nodes carrying `NoAsciiEscaping`, whose final byte output is
  still an open obligation.

The parameter-name identity consumer re-escapes the tuple label display
worker's already-unescaped result exactly once, restoring upstream's escaped
symbol key. Verify ordinary, tuple-rest and leading-underscore parameter
controls together; the return type alone is not behavioral evidence.

Checker compilation, remaining module/path and declaration adapters, checker
execution and the complete compiler commands are still pending. The passing
224 lower and 504 emitter-library snapshots in §29 remain scoped to their
recorded sources and targets. Claude's document is **準備中・未提出**.

### 30.1. Additional name consumers and the output-only Identifier boundary

`createEmptyObjectTypeFromStringLiteral` still discarded a literal whenever
`TemplateText::to_utf8()` failed. The owner now creates an `EscapedName` from
`to_js_string()` without dropping the property. Add reverse mapped/keyof
controls to verify distinct lone surrogates and the paired-spelling alias;
this checker change is not executed yet. Widening-context property keys and
first-position/last-value maps now use the same identity type.

Statement serialization's `getNameCandidateWorker` sanitizes rejected names
one UTF-16 unit at a time, as the upstream non-Unicode regular expression does
(`_tsc.js:55390–55437`). A supplementary character that requires sanitization
must contribute two underscores. Generated local names remain scalar after
this explicit sanitization; verbatim export names do not pass through it.

The TS direct factory accepts a lone surrogate in an Identifier used as an
export name and prints that raw UTF-16 unit (`createExportSpecifier` / `asName`,
`_tsc.js:23657–23668`, `24966`). The separate immutable observer
`scripts/observe-utf16-synthetic-export-names.mjs` captures seven values with and
without an identifier clone, twice each; a separate `--check` also exits 0.
Fixture SHA-256: `3618f7fc32be479c7954fe5ddf732364c9f4cfd19d07f1103b61abda60ee644f`.
These are factory/printer controls, not complete declaration emit commands.

`NodeFactory::create_unchecked_identifier` extends the existing JSX recovery
carrier to this output-only boundary. A non-scalar spelling lives in
`EmitMetadata::unchecked_identifier_text`; clones retain it and the printer
writes the UTF-16 value. Its empty scalar IdentifierData payload must never
be a lookup/cache key. The checker calls this entry after symbol lookup for
serialized export/entity/expression names. Ordinary lexical identifiers and
sanitized generated bindings retain the scalar factory entry.

`utf16-emitter-library-20260913-193542/receipt.json` records **505 tests passed
twice**, unchanged inputs/head, diff check 0, including the 14 new observations
(each printed twice). The preceding `193420` build failure used a serde derive
without a direct crate dependency; the test now reads the existing serde_json
value schema and adds no dependency. Later resolver protocol edits are outside
this passing snapshot and require renewed verification.

The callback's function property name is now `EscapedName`, while a tracker
symbol description retains the canonical JS spelling. Remaining callbacks,
recording JSON, module paths, checker compilation and the original compiler
commands are still open. No original fixture or comparator has been changed.

### 30.2. Diagnostic renderer owns JavaScript text

The ordinary `194100` checker receipt reaches the checker with 230 errors
(exit 101, inputs unchanged); `193925` stopped in emitter with four errors
from the newly branded expando-property protocol. Those expando consumers
now unescape the key before their lexical identifier filter, matching
`_tsc.js:115419–115438`. Only values admitted by that grammar become the
scalar generated declaration name. Leading-underscore expando controls
remain required when compiler execution is available.

The diagnostic renderer's `SliceTypeNodeFace` and its source/type-text return
values now retain `JsString`. Explicit `concat_js` / `join_js_texts` combine
raw JS values and scalar syntax text; the numeric interpolation implementations
format only numeric counters. No Display implementation was added to
`JsString` or `EscapedName`. This closes a downstream ownership gap that
previous symbol-type changes exposed; it is not a textual escape workaround.
`194847` records the first compile-time classification pass (481 checker
errors, exit 101, inputs unchanged), after 155 interpolations and the owning
return types were changed. These intermediate error counts are not tests.

`string_literal_type_display_text`'s NoAsciiEscaping branch previously wrote
lone surrogates as six-character escape spellings. It now retains the unit
while continuing to escape the actual control/quote/backslash characters
required by escapeString. Final diagnostic and declaration byte sinks still
need full-command verification; source/name identity never uses a sink
projection. The expression renderer and node builder share the same
stripQuotes + `/\\./g` UTF-16 worker, preserving matching-quote rules,
line terminators and trailing backslashes without a second decoder.


### 30.3. Literal type display observations and remaining renderer owners

The separate immutable observer
`scripts/observe-utf16-literal-type-display.mjs` records eight values through
TypeScript `typeToString`, a synthetic literal type printed with
`NoAsciiEscaping`, and semantic diagnostics. Each observation runs twice;
the independent `--check` exits 0. Fixture SHA-256:
`83438e83bcef8a6e8ff237ac11d3fb03a877ef0340d38d093191d9a45bb579f1`.
For the D800 case the type display is `[34, 55296, 34]`, retaining the raw
lone-surrogate unit. These are TypeScript API observations, not Rust results
or proof of final declaration bytes. The paired Rust test
`checker/tests/unit/check/tests.rs::literal_type_displays_and_diagnostics_retain_typescript_utf16_values`
is added but uncompiled and unexecuted pending checker integration.

Subsequent ordinary checker checks all exit 101 with unchanged captured inputs:
`195137`: 327 errors; JSON receipts `195443`: 296, `195659`: 234,
`200018`: 189, `200457`: 176. Receipt names have the prefix
`utf16-checker-json-check-20260913-` for the JSON runs. These are compile-time
classification passes. The next migration carries clone-display expression,
body and module text in `JsString`; JSX attribute entity escaping must preserve
raw non-ASCII code units, including lone surrogates, until the output sink.
Module paths, diagnostic recording protocols and compiler qualification remain
open. The original 23 controls and all frozen receipts remain unchanged.


### 30.4. Module export identity, display closure and path adapters

Clone-display expressions, bodies and modules now own `JsString`, including
JSX attribute entity escaping. After lifting scalar token/indent fragments,
`201344` reached 149 errors with none in the three clone-display modules or
`check.rs`. It followed the ownership-transition `201151` pass (197 errors).
Module exports now retain `EscapedName` in export-star collision tables,
type-only export name maps/sets, and the type-only alias rollback journal.
`module_export_name_text_escaped` returns the brand; its unescaped output
returns `JsString`. The 92 module diagnostic calls now use canonical views.
Ambient-pattern comparisons use UTF-16 prefix/suffix operations and unit
lengths, including a prefix that ends inside a surrogate pair.
Subsequent JSON checker receipts: `201616` 136, `201801` 118, `202016` 107
errors. All exit 101 with inputs unchanged; no checker test is qualified.

Document registry paths and per-file diagnostic output paths now own JS text.
The module-specifier fallback host stores canonical path keys, and its package
scope lookup uses canonical borrowed directory views. The shared
`JsStr::rsplit_once` uses a scalar separator whose byte match always lands at
whole code-point boundaries; it preserves both borrowed canonical components.
It is distinct from arbitrary-JS pattern matching, which must still use UTF-16
unit bounds. The new test covers scalar differential cases and lone-surrogate
path components around repeated slash / supplementary separators.
`utf16-diagnostic-paths-20260913-202213/receipt.json` records **49 native tests
passed twice**, input/head unchanged and diff check 0. This lower-crate result
supersedes the historical 48-test diagnostics snapshot only for its own scope.
Later checker changes do not alter those diagnostics inputs.

The checker module-format path index and package-type path map now use JS keys.
Remaining legacy resolver paths, input-host metadata, package JSON values,
module-specifier synthesis and tracker recording still need ownership closure.
In particular, package JSON conversion must account for the program layer's
encoded NUL keys / JSONC prototype marker rather than blindly serializing its
internal object representation. No frozen expected output or receipt is revised.


### 30.5. Input paths and original host text

The `202412` JSON checker receipt reaches 93 errors. Removing the remaining
scalar return from `normalize_program_path` then exposes its legacy callers:
`202644` has 200 errors, `202937` 169, and `203501` 140. These checks all
exit 101 with unchanged captured inputs. No compiler command or checker test
has passed as a result of these intermediate type counts.

`InputFile.name`, authoritative source/resolver metadata and failure requests,
source/module path indexes, package path maps, library-cache path keys and
fixture-shadowing lookups now retain JS values. Shadowing uses a canonical
name key directly: converting a non-scalar path to `Option<&str>` would make
distinct invalid scalar projections collide at `None` even where Rust type
inference still permitted the lookup. The three per-file diagnostic selectors
also compare original JS paths. Infrastructure error Display projects only the
stored containing-file text at that final UTF-8 sink; stored requests remain
canonical, and debug specifier output retains each surrogate spelling.

The fallback module-specifier host now retains every original input snapshot
through `CheckerState::host_input_snapshots`. Its readFile results come from
that text owner, including host-only package manifests overriding equal-path
Program sources; binder/source membership is unchanged. It no longer rebuilds
host input with `serde_json::Value::to_string`. This removes the serialization
hazard noted in §30.4 at this particular read boundary. Lossless parsing of
package values and the separate tracker recording protocol are still pending.

Extension candidate probes, extension suggestions and relative output paths
now use canonical values. ASCII delimiters justify byte offsets only at an
explicit `split_at_byte` boundary; arbitrary JS wildcard/prefix matching still
requires UTF-16 positions. The 14 module-reference input methods now accept JS
views as their native input. Their remaining resolver/path callers are being
closed in subsequent snapshots. Complete A/B/C qualification is still open.


### 30.6. Shared package JSON values and resolver closure

The checker now parses package manifests through the program layer's
`read_package_json_object_from_snapshot`, retaining the original input snapshot.
Root property reads use the shared inherited-property accessor; exact mapping
lookups use the own-property accessor. Enumeration uses the existing program
worker, which decodes escaped NUL keys, omits prototype metadata, orders array
index keys numerically, then preserves insertion order for other keys.
Wildcard captures and replacements preserve UTF-16 units. No package value is
converted through a scalar-only JSON serializer for host readFile.

Package `name` is retained verbatim, including whitespace, matching the existing
TypeScript self-name and package-ID reads. Scoped `@types` mangling now delegates
to the existing program helper: only the first slash is replaced and a slashless
scope is unchanged. Added qualification obligations are whitespace/lone-surrogate
package names, slashless/nested scoped names, JSONC inherited versus own keys,
NUL keys, numeric key order, and wildcard captures at surrogate-pair boundaries.
These are obligations, not claimed passing checker controls.

`utf16-program-lib-20260913-204855/receipt.json` records **42 native tests passed
twice**, unchanged inputs/head, identical stdout, and diff check 0. Its snapshot
includes the shared snapshot reader but predates consolidating the public own
entry helper with the existing numeric-order worker and exposing the own-property
and scoped-name helpers. It therefore does not qualify the current final source.

Later ordinary JSON checker checks (`utf16-checker-json-check-20260913-` prefix)
record 203649: 140, 203823: 149, 204215: 135, 204611: 119, 205058: 123,
205341: 111, and **205742: 62** errors. Each exits 101 with inputs unchanged.
The last snapshot has no errors in `modules.rs`; 47 remain in declaration
module-specifier synthesis, with the remainder in tracking/recording and two
path consumers. This is ownership closure in progress. No original complete
command has an after-capture; A/B/C and the final Claude review remain open.


### 30.7. Declaration module specifiers and package accessor controls

Declaration module-specifier synthesis now owns JS paths, preferences derived
from path values, mapping keys/patterns, package conditions, synthesized results
and its emit-only cache. ASCII delimiter offsets use a local checked byte-slice
helper. Arbitrary-JS wildcard prefixes/suffixes/captures use UTF-16 unit bounds;
the missing-star target branch implements JS `slice(0, -1)` on units. Filename
casing delegates to the existing host worker through program. Its package JSON
reads use the same shared reader and property/own-entry accessors as the checker
resolver. The bounded regexp exclusion matcher now consumes units without `u`
and decoded points with `u`, retaining lone units in both modes; full RegExp
support is not claimed and the pre-existing unsupported-pattern route remains.

Intermediate checker JSON receipts: `210803` exits 101 with 102 errors after
changing specifier owners; `211132` has 23; `211310` has 19, with none in
`specifier.rs`. These are compile-time migration snapshots, not test results.
The subsequent tracker/accessibility value ownership change (`211553`, 21
errors) compiles ordinary emitter dependencies but leaves checker recording and
accessibility consumers open. The replay JSON tree is now being migrated to the
existing program `JsonValue`; general serde serialization of a JS value is not
introduced. A canonical request/result observer seam must retain the existing
wire JSON shape, and scalar compatibility must be a fallible boundary adapter.

New observer `scripts/observe-utf16-package-accessors.mjs` records seven package
objects, seven scoped names and two filename-case values, each twice. `--check`
exits 0. Fixture `program/tests/fixtures/utf16-package-accessors.json` SHA-256:
`9694152d1bbede65f3ff82c65183403b3d07f0371e3dd964566f018672ba13d2`.
The native package comparison checks exact own-key order and the string-field
projection used by package consumers, including own/inherited values and absent
or non-string fields. It does not assert general Object.prototype reflection or
callable builtin values. The scoped-name and case controls compare full units.

`utf16-program-lib-20260913-211836/receipt.json` records **45 native tests passed
twice**, inputs/head unchanged, diff check 0. The stdout files differ only in
elapsed test time (0.07s versus 0.08s). This includes the consolidated accessor
exports and the three new tests, superseding `204855` for the program library
scope. Checker and emitter tests, raw UTF-16 source input, the original 23
complete-command after-captures and the final Claude packet remain outstanding.


### 30.8. First ordinary checker build and replay JSON boundary

`utf16-checker-json-check-20260913-212441/receipt.json` is the first successful
ordinary checker cargo check in this ownership closure: **exit 0**, captured
inputs unchanged. It supersedes the intermediate failing `212148` snapshot
(42 replay/accessibility errors), not the outstanding native tests or compiler
qualification. Compiler production adapters are now being checked separately.

The declaration symbolToString adapter now returns the printer's UTF-16 value;
accessibility result/error records, deferred tracker callbacks, declaration
materialized diagnostic arguments, and syntactic module-specifier overrides
retain JS text. Replay decisions and callback payloads use the existing canonical
`JsonValue` tree. An explicit local JSON construction/encoding boundary preserves
string units, ordinary key order and the existing wire shape. It does not add
Serialize/Display to JsString or EscapedName. Replay comparison keys that were
already JSON text now use explicit JSON encoding. Canonical symbol keys remain
EscapedName; serialized projection is confined to the replay observer.

`with_declaration_emit_replay_js_observer_for_harness` accepts/returns canonical
JSON values. The existing serde observer is retained as a scalar compatibility
adapter for the frozen harness: it converts input once, then converts its report
through well-formed JSON text and a fallible serde parse. A non-scalar report
requires the JS observer API; there is no replacement or sentinel name. The JS
observer has no such scalar restriction. The request hook remains inert in
production when no request is installed. Native controls for nested replay JSON,
JS observer comparisons, and the unchanged frozen replay comparators remain
required before qualifying this boundary.


### 30.9. Compiler and CLI production build

The ordinary compiler JSON check receipts (`utf16-compiler-json-check-20260913-`)
record `212543`: 96 errors, `212836`: 34, `213253`: four, and **213418: exit 0**.
Every snapshot has unchanged captured inputs. The last check compiles the
compiler library and CLI plus their real production dependencies. Native test
adapters and full-command comparisons are the next boundary, not implied by
this successful build. The original witness target's test build is now started.

Prepared/checked emit hosts, authoritative resolver projections, diagnostic path
ownership queries and checker cwd inputs now carry canonical JS paths. Old
scalar Unicode checks were removed from already-owned prepared JS values;
actual native path constructors retain their native-domain contracts. Missing
source/incomplete-check error payloads retain JS names until their error Display.
The source-map harness callback/result paths and command status records retain
JS values as well. The CLI host implements all seven explicit JS queries;
only exact scalar virtual-library names are intercepted, and other requests
reach the filesystem host unchanged.

Symlink facts contained a remaining lossy conversion even after the surrounding
path owners compiled. It is removed: file/directory pairs and dedupe keys now
retain JS values and use the host filename case profile. A new native symlink
control distinguishes D800/D801 tails and retains different parent names when
the filename tail matches. This changes program sources after `211836`; the
45-test result is now a historical snapshot and the current program library
must be rerun. The new symlink control has not executed yet.

Native output writes convert the requested JS path at the actual filesystem
call; stable I/O diagnostic messages retain the requested JS spelling. CLI
source maps, filename grouping and relative display names retain JS text.
The multi-file pretty summary orders names with explicit `cmp_utf16`, and
stdout conversion occurs after rendering/grouping. Command status writes are
JS API values; the terminal output remains encoded text. Newly added replay
JSON unit tests cover nested values, paired/lone units and distinct object keys
versus literal escape spellings; they are not executed by ordinary cargo check.
Raw UTF-16 source files retain the separately scoped limitation in §10.4;
observing that boundary and all final qualification obligations remain open.

### 30.10. Native witness build, harness queries and raw-source observation

The native original witness target now builds with actual production dependencies:
`utf16-original-test-build-20260913-215623/receipt.json` has **exit 0 and unchanged
captured inputs**. Earlier attempts record 65 harness errors (`213559`), three
config source-text adapter errors (`215130`), 31 test adapters (`215253`), and
four test adapters (`215514`). The `215130` inputs-unchanged field is **false**:
Cargo updated only `Cargo.lock` for the harness's explicit diagnostics dependency.
This is a build attempt, not qualification; subsequent snapshots include that lock.

Compiler-suite and mounted-project hosts now forward JS queries through all seven
CompilerHost methods. Library mount routing compares canonical components before
filesystem I/O. Virtual mount lookup and compiler fixture config queries retain
JS paths, including case folding, wildcard matching, enumeration and query logs.
Project config root arrays remain JS values through new `load_program_js` and
`load_emitting_program_js` entry points; native root APIs keep their original
contracts. Config file contents remain the admitted scalar source-text domain.
The corpus's scalar serde log adapter converts only after host execution and
returns an explicit observer error if a JS result is unrepresentable.

Original comparison adapters lift scalar fixture options at entry and require
exact scalar representability at legacy JSON output boundaries. Unexpected lone
units cannot become U+FFFD and match a scalar expected value. Diagnostic ordering,
write count/order, callback/materialized byte comparisons, maps, status and exit
checks remain active. Callback path equality now compares JS text directly; it
still does not normalize dot components. Original input and expected fixture
hashes remain those in §1. The full 23-case run is now started, with primary
comparisons and separate supplemental command captures; no result is claimed yet.

The agreed raw-source requirement is §10.4's **separately scoped limitation**, not
an instruction to migrate TextSnapshot/source-file storage. The independent
`observe-utf16-raw-source-boundary.mjs` observer records six LE/BE cases twice:
two distinct raw high surrogates, a raw low surrogate and a valid pair. TypeScript
sys.readFile retains their units and all parse without diagnostics. Fixture SHA:
`c91bfe347d853e618cccea13ff5518efc107710a0ab533537a4d50a631136e0a`.
Its `--check` succeeds. `utf16_raw_source_boundary.rs` records the Rust decoder/
loader distinction (explicit rejection for raw lone units; accepted pairs) but
has not executed yet. This observation is separate from escaped literal values
in the original 23 repair commands.

## 31. Original 23 complete commands: first exact native result

`target/declaration-comment-ranges-runs/utf16-adjacent-complete-20260913-215727/receipt.json`
records **exit 0, all captured inputs unchanged**. The unchanged original input
and expected fixture hashes are checked against §1 before running. The native
comparator reports all **23 cases EXACT x2** (46 primary complete commands).
The separate 46 supplemental captures also satisfy full `actual == expected`
JSON equality, with both actuals identical for each case. See `capture-summary.json`
and `captures/` under that receipt directory. The test reports one test passed,
nine unrelated tests filtered, in 34.58 seconds.

This is the first complete command evidence for all three requested failures:
eight A commands (declaration values and class/object name collisions), fourteen
B commands (literal lexical recovery, including octal/decimal/hex/extended and
unterminated literals), and one C command (ES2015 tagged invalid cooked). The
comparison includes declarations, callback/materialized bytes, diagnostic chains,
map data, emit listing/refusal, status writes and exit code. No expected fixture,
ratchet, manifest or CI policy was changed to obtain this result.

The prior 64 commands are now running against the same production snapshot.
The goal is **not yet complete**: added controls, current native unit/integration
regressions, prior 41, both original-four routes, G4a/G4b, complete consumer
classification and the final review packet still need closure. Raw UTF-16 source
files keep the separately scoped §10.4 limit; their decoder/loader observation
has been prepared but not yet executed.

### 31.1. Prior 64 exact; C control input correction

`utf16-prior64-complete-20260913-215852/receipt.json` records **exit 0, captured
inputs unchanged**. All prior 64 primary complete-command cases are exact twice;
all 128 supplemental captures equal their full expected JSON and repeat
identically. This uses the same production snapshot as the first original 23
success. An earlier launcher attempt stopped in its Python environment setup before
Cargo or any test invocation; no result is claimed for it.

The first native 16-case C control run (`utf16-tagged-controls-complete-20260913-220128`)
recorded deterministic differences in all 16 cases because its test's
`EmitOptionFloor::Established` input builder dropped sourceMap/declarationMap and
outDir. It emitted two files under `/project`, while the independently observed
command requested four writes under `/project/out`. The test is corrected to
construct the complete frozen options directly through MemoryCompilerHost and
`load_emitting_program`, without a corpus option floor. The observed fixture and
comparison fields remain unchanged. This rerun is in progress; the initial
incorrect-input result is retained rather than counted as production qualification.

### 31.2. C controls exact and current program tests

`utf16-tagged-controls-complete-20260913-220326/receipt.json` records **exit 0,
captured inputs unchanged**. All **16 complete C cases are exact twice** (32
captures), including review v2's positive hoist witness, valid/invalid rest tags,
nested revisits, SourceFile/ModuleBlock numbering, function scope and raw/cooked
values. Every actual capture equals the entire pinned expected JSON. The input
builder now retains the observer's map and output-directory options (§31.1).

A subsequent name-consumer audit found two checker wildcard substitution helpers
still doing literal replacement. TypeScript `replaceFirstStar` (`_tsc.js:19373`)
and package-map global replacement interpret `$$`, `$&`, prefix and suffix tokens.
The checker now calls shared program value builders that use the same piece
walker as the existing budgeted host resolver helpers. Checker string builders
retain ordinary allocation; the resolver's separate budget and typed allocation
errors remain unchanged. Both value builders are compared against all 14 pinned
replacement observations, including canonical surrogate concatenation.

`utf16-program-lib-20260913-220517/receipt.json` records **46 native program tests
passed twice**, identical stdout, unchanged captured inputs/head and diff-check 0.
This includes the new symlink distinction and shared replacement-value checks.
It changes production sources after the 23/64/C command receipts above, so those
receipts identify the earlier snapshot and final complete-command verification
must include this later name-operation change.

### 31.3. Prior 41, original-four routes and G4 regressions

`utf16-prior41-complete-20260913-221129/receipt.json` records **exit 0, captured
inputs unchanged**: all 41 declaration-comment cases are exact twice. All 82
supplemental full captures equal the expected JSON and repeat identically.
This snapshot includes the later shared wildcard value operations in §31.2.

`utf16-original-routes-regression-20260913-221422/receipt.json` records unchanged
captured inputs and exit 0 for both original-four routes (typed projection and
supplemental complete commands) and for the G4a/G4b exact comparisons. Each
comparison repeats twice internally. The separate strict G5c command exits 101;
its two full actual captures and expected values are **identical** to the
corresponding original before captures under `utf16-adjacent-before-20260913-005302`.
Only `writes` differs from expected, preserving the known separate declaration
return-inference limitation. It is not counted as passing or silently waived.

`utf16-raw-source-boundary-20260913-220711/receipt.json` records the six raw UTF-16
source decoder/loader observations passing twice, unchanged captured inputs/head
and diff-check 0. Earlier raw-target build attempts failed on a test-only serde
import; the standalone test now reads the pinned numeric arrays through the
existing serde_json dependency. Source decode behavior was not altered.

Additional explicit value controls are prepared for the checker exclusion regex
subset (12 cases, fixture SHA
`872598abd8abdce43d72f1732fd89b3fefbbc786224299b729c1f331810c8baa`) and declaration
literal-type printing with/without NoAsciiEscaping (8 cases, SHA
`8f52ca2100a8c145d0e6ee62b44095740ea4a9e1a1ffb0ceb240524517f50a34`). Both independent
observers repeat and their `--check` commands pass. Their Rust tests have not run
yet. The latter directly compares printer UTF-16 callback units and UTF-8 bytes
under a synthetic `.d.ts` type alias; it supplements the complete original A
commands and does not claim to be another full compiler-command observation.

The first current checker library test build is running. A full current native
checker/emitter regression result, remaining consumer audit, final source freeze,
and final command verification/review packet remain open.

### 31.4. Checker library and declaration literal printer qualification

The ordinary checker library now builds with the actual production graph.
`utf16-checker-lib-build-20260913-223336/receipt.json` records exit 0 and
unchanged input files. Legacy scalar test observers explicitly require
`as_str().expect(...)` after semantic processing; no replacement conversion
or equality-field removal was used to make the tests compile. New UTF-16
controls compare code units directly. The first execution,
`utf16-checker-library-20260913-223801/receipt.json`, is **1735 passed / 1 failed**.
The one failure is the new standalone LiteralType printer call, which lacked
`PrinterOptions::with_declaration_syntax(true)`. That test input is now fixed;
checker library requalification is pending. The existing checker tests and
new canonical resolution, replay JSON and supported RegExp controls passed
in that first execution; this does not qualify the corrected final snapshot.

`utf16-emitter-literal-suite-20260913-224049/receipt.json` records the current
**505 emitter library tests plus 2 literal integration test functions,
each passing twice**, exit 0, unchanged inputs/head and diff check 0. The
integration functions cover the existing 288 escaping observations and the
new 8 declaration literal-type observations (each internally repeated),
including `NoAsciiEscaping`, callback UTF-16 units and final UTF-8 bytes.
The preceding build at `223858` stopped on two legacy test input fields
following the resolver's JS-string protocol change; those two scalar inputs
were lifted to `JsString`, without changing expected values.

The additional A1–A10 / B1–B6 command matrix and the remaining C1 target/span
controls are now separately observed in
`scripts/observe-utf16-identity-recovery-controls.mjs` and
`crates/compiler/tests/fixtures/utf16-identity-recovery-controls.json`:
**65 complete commands, two identical upstream observations each**, SHA-256
`0605b71cafcffa559630bf891158a18441a5d00b84e359f9897407248c25e343`.
Nine B negative controls require the existing typed H2.9 refusal, with the
upstream command tuple retained separately; a refusal is not labeled exact.
Native comparisons have started and are not yet qualified. The fixture also
records a repeated upstream internal error for duplicate instance/static
private fields at ES5. The A8 private-name command uses ES2022; the original
object/internal/numeric/unique-symbol controls retain ES5 and ES2015.
No original 23/64/41 fixture, expected comparator or CI policy was changed.

### 31.5. Additional identity/recovery commands and noEmit command connection

`utf16-identity-recovery-complete-20260913-224314/receipt.json` first captured
49 exact commands twice, then stopped because the test had sent `noEmit:true`
to the separately typed emitting loader. The observer was connected to
`load_program` / `ProgramSession::run` for that option. The next run,
`utf16-identity-recovery-complete-20260913-224725/receipt.json`, captured all
65 cases twice: **55 complete tuples exact, 9 required typed recovery refusals,
1 noEmit observer mismatch**, unchanged inputs. All repeat captures agree.
The A1–A10 controls (35 commands), B1's 14 `noEmitOnError` commands, both B6
EOF-backslash cases, B5 and C1's additional target/span cases are exact.
B3/B4 all preserve `ParseDiagnosticsDeferred { owner_slice: "H2.9", .. }`
and no partial writes. No fixture was weakened to admit those negative cases.

The noEmit mismatch was in the test's fabricated emit-result projection:
zero writes do **not** imply `emitSkipped:true`. Vendored `handleNoEmitOptions`
(_tsc.js:125636–125640) calls `program.emitBuildInfo` for a whole Program;
with incremental/composite disabled the initial `emitFiles` collections are
returned: `emitSkipped:false`, and an empty `sourceMaps` list if a map option
is enabled. The attempted filesystem CLI observer also hit the pre-existing
H0 config scope restriction for emitting-only option names, and therefore
was not evidence of successful diagnostic command execution. Its stderr is
retained in that failed capture. No CLI scope rule or exit-code rule changed.

The command observer now uses the production command result throughout:
`ProgramSession::emit_command_for_harness` accepts its existing Emit route and
also a NoEmit prepared program. The latter executes the ordinary typed H0
`run`, then obtains the empty whole-program build-info result from
`EmitOutcome::no_emit_without_build_info`. This constructor is value-only;
it rejects incremental/composite output, requires `noEmit:true`, and creates
no resolver, output plan, writer, artifact or sink call. Reporting uses the
same `CliEmitSessionOutcome::into_reported` and CLI status producer as Emit.
The test retains a separate native zero-activity assertion and no longer
manufactures emit metadata or takes exit status from the failed CLI route.
The unchanged 65-case full comparison has been restarted; its result remains
pending until the receipt completes. H0 filesystem config scope is unchanged.

`utf16-checker-library-20260913-224923/receipt.json` records **1736 tests
passing twice**, with unchanged inputs/head and diff check 0, after correcting
the standalone declaration printer option. This snapshot predates the
value-only noEmit outcome addition above. The source changes must be included
in final qualification and the final review packet.

The restarted `utf16-identity-recovery-complete-20260913-225202/receipt.json`
now passes: **56 complete tuples exact twice and 9 typed recovery refusals
twice**, 65/65 controls satisfy their distinct contracts, exit 0 and unchanged
archived inputs. In particular the NoEmit command's actual production outcome,
not a test-manufactured tuple, now agrees with all upstream fields. Additional
map/list option combinations are being observed separately before freezing
that result's scope. The unchanged original 23/64/41 and C16 need their final
rerun after the final production snapshot, including this new command branch.

### 31.6. NoEmit option matrix; remaining workspace adapter errors

`utf16-noemit-command-complete-20260913-225604/receipt.json` passes the
additional **14 complete NoEmit commands twice**, exit 0 and unchanged input
snapshot. The immutable observation is
`crates/compiler/tests/fixtures/utf16-noemit-command-controls.json`, SHA-256
`240b8d67c357b4268574d29031263a7984956cc6bc1fd7b675be0eaf82c577a0`;
`scripts/observe-utf16-noemit-command-controls.mjs --check` also succeeds.
Valid and invalid literal sources cover no maps, external JS maps, inline JS
maps, enabled declaration maps, inactive declaration maps, emitted-file
listing, and noEmitOnError precedence. Both incremental and composite requests
remain typed `UnsupportedEmitFeature::BuildInfo` at the added empty-result
constructor. The tests retain the original 65-case comparison in the same
source and share its unchanged field-by-field capture logic.

The first ordinary whole-workspace check,
`utf16-workspace-check-20260913-225431/receipt.json`, reports 50 adapter errors
in fuzz/conformance (exit 101, unchanged inputs), beyond the already compiled
compiler/harness graph. The conformance H0 memory adapter is being migrated
to JS-owned display/canonical paths and package keys. Its existing strict
POSIX normalization, above-root rejection and source insertion order stay
explicit; lookup queries do not cross native Path or scalar conversion.
That adapter work and the remaining wire/renderer observations are not yet
qualified. No whole-workspace check or final review completion is claimed.

The H0 memory adapter's ordinary production compile now succeeds. Fuzz and
conformance wire adapters explicitly preserve their existing scalar-string
observation schema: a nonrepresentable diagnostic/rendered value returns an
observation error, never replacement text, an escaped alternate value or a
successful comparison. Compiler names remain JS values through source lookup;
scalar corpus maps are searched with complete JS equality before the wire
boundary. This restriction already exists in M9's
`validUnicodeString` checks (`crates/fuzz/vectors/verify-canonical-class.mjs`)
and the oracle/golden Rust `String` deserialization. It does not qualify these
legacy observers for the new arbitrary-UTF-16 diagnostic controls; those use
the separate complete-command UTF-16 array schema. No comparator equality,
expected fixture or schema version was changed by the adapter updates.

The next workspace check, `utf16-workspace-check-20260913-230212/receipt.json`,
reaches xtask and reports 122 remaining compiler errors in its observation and
acceptance adapters. Fuzz/conformance library production compiled in this
pass; their native tests and xtask adapter migration remain open. The run
has exit 101 and unchanged archived inputs.

### 31.7. Workspace production check and remaining test-target migration

`utf16-workspace-check-20260913-230918/receipt.json` records the first
successful ordinary offline `cargo check --workspace`: exit 0, unchanged
archived inputs. This is a production type check, not hosted acceptance or
completion of all native test targets.

The subsequent `--all-targets --keep-going` check,
`utf16-workspace-all-targets-check-20260913-231623/receipt.json`, exposes
1724 errors in previously unbuilt legacy integration-test/example observers
and fixture inputs. It exits 101 with unchanged archived inputs. Most are
scalar input values needing the new owner type, native Path observations of
JS paths, JSON String observations of JS/config values, and test hosts missing
the canonical-query trait methods. These are real outstanding compile gates;
the earlier 23/64/41 and focused library successes do not close them.

Legacy observation adapters retain their old scalar fixture domain, Path
comparisons, JSON fields, null presence, array order, diagnostic contents and
Number values. Shared **test-only**, explicitly invoked helpers in
`crates/host/tests/support/{scalar_path,scalar_query_bridge}.rs` and
`crates/program/tests/support/scalar_json.rs` reject a non-scalar value instead
of replacing it. The host bridge calls each mock's existing native method,
so fault injection and probe order remain observed. This bridge is only for
those existing scalar fixture mocks, not a production host or a claim of
arbitrary-UTF-16 coverage. The new UTF-16 controls use full JS queries/arrays.
No expected fixture or existing equality comparator was changed by these
adapters. Native execution of these targets remains pending.

The consumer audit found one further semantic loss in
`checker::jsx::get_intrinsic_attributes_type_from_string_literal_type`:
`TemplateText::to_utf8()` skipped the property lookup for a lone surrogate.
It now bridges to `JsString`, escapes once, and queries the same complete key
as the IntrinsicElements declaration. Independent TS observation
`scripts/observe-utf16-jsx-intrinsic-identity.mjs` pins four distinct tag types
(high D800, high D801, low DC00, and U+FFFD), valid attributes and three
incorrect-attribute diagnostics. Both TS executions and `--check` agree;
fixture SHA-256 is
`fd1d5536defa8648aa8e91f9fd148a90574f47360c4b710cd93cff82c00c31c2`.
The new native checker test compares full diagnostic UTF-16 messages and
ranges; its build/run and the final complete-command reruns remain pending.
The final ordinary all-targets check before handoff is recorded below.

### 31.8. User-requested implementation handoff; incomplete

The user requested preparation of a Claude handoff. Further implementation
changes stopped. `utf16-workspace-all-targets-check-20260913-233912/receipt.json`
finished with exit 101, 40 compiler-message errors and unchanged archived
inputs. The remaining reported errors are in existing test input/observation
adapters; no all-target success or final regression qualification is claimed.
The latest JSX test and new scalar-wire/path boundary tests still need native
execution, and the semantic consumer audit and B admission corpus delta remain
open. Earlier successes are evidence for their own snapshots only.

The [handoff document](h2-8a-utf16-adjacent-handoff.md) is the continuation entry
point. `target/declaration-comment-ranges-runs/utf16-claude-handoff-20260913/`
contains the latest raw errors and logs, receipt copies, tracked patch,
untracked archive, source snapshot including examples, manifests and runner
copies. The HEAD remains `b652451f0ec4e6aba47aba3f4fd345c1b9168cdc`; most
implementation changes are uncommitted. This is a freeze for continuation,
not an implementation-completion or final-review approval.

## 32. Continuation after the handoff (2026-09-14)

Continuation of the [handoff](h2-8a-utf16-adjacent-handoff.md) §6 steps on the
same dirty worktree (HEAD `b652451f0`, uncommitted migration). No CI, no
hosted acceptance, no `cargo xtask acceptance` was run. Native jobs stayed
serial with `CARGO_BUILD_JOBS=2`, `--test-threads=1`, offline, in the dedicated
`target/declaration-comment-ranges` directory.

### 32.1. The 40 remaining test-adapter errors

All 40 compiler-message errors were in existing test observers, not in
production. They were closed with the test-only scalar adapters already
introduced for this repair (`program/tests/support/scalar_json.rs`,
`host/tests/support/scalar_path.rs`), the changed `JsStr`/`JsString`
signatures of `finish_declaration_bundle_map`, `source_mapping_url`,
`source_map_recording_inputs_for`, `canonical_emit_path` and the harness
source-map selector, `PathMapping::new`'s `Vec<JsString>` substitutions, the
`ProgramLoadError::InvalidInput { js_path }` field, the `EmitIoError`
`message()`/`path()` `JsStr` accessors, and the module-suffixes oracle test's
missing `scalar_json` wiring. No production Serialize shortcut and no expected
value changed. `cargo check --offline --workspace --all-targets --keep-going`
then finished with exit 0, zero errors and zero warnings
(`utf16-workspace-all-targets-check-20260914-001527`, `inputs_unchanged=true`).
The warnings removed on the way were an unused `Path` import in
`compiler/src/lib.rs`, the never-used `String`-argument wrappers
`report_error_direct`/`report_error_unelided` in `checker/src/engine.rs`
(their `_js` variants are the callers), an unused `json` import in the
`replay_json` unit tests, an unused `Path` import in
`host_text_decode_contract.rs`, and redundant parentheses in
`declaration_transformer_replay_decision_equal.rs`.

`cargo fmt --all` reformatted the migrated files (1,603 rustfmt diffs; only
files already dirty relative to HEAD changed) and `git diff --check` is clean.

### 32.2. Native test queue incidents

The focused native tests run through
`target/declaration-comment-ranges-runs/run-utf16-focused-native-tests.py`
(serial commands, one receipt with the crate source snapshot). Two launches
failed at link time with undefined anonymous symbols from `libtsc_syntax` and
`libtsc_emitter` rlibs: artifacts corrupted by an earlier build that was killed
mid-compile, not a source defect. `cargo clean --offline -p <every workspace
crate>` in the dedicated target directory (11.4 GiB) removed them; the third
launch built cleanly. The partial run directories of the two failed launches
are kept as they are.

### 32.3. Consumer audit (handoff §6 step 3)

The pre-migration 245-row search inventory was replaced by a classified,
function-level audit of every production consumer of `escaped_name`,
`.text.as_str(`, `as_str()`, `to_utf8()` and the lossy conversions
(`to_string_lossy`, `from_utf8_lossy`, `from_utf16_lossy`): 1,253 rows in
`crates/*/src` outside `cfg(test)` modules, every row classified exactly once
by seven grouped read-only reviews and validated by file:line key. The result
is [`h2-8a-utf16-adjacent-consumer-audit.md`](h2-8a-utf16-adjacent-consumer-audit.md)
with the evidence directory `utf16-consumer-audit-20260914-004515`.

Totals: identity 350, grammar-scalar 147, display 64, native-io 17,
scalar-observer 105, not-js-value 564, suspect 6. The six suspects, each
re-verified against the source and vendored `_tsc.js` before any change:

1. `syntax/src/parser.rs::parse_new_expression_stub` called `parse_identifier()`
   after `new` `.`, so a string or template token whose value carries an
   unpaired surrogate (`new."\uD800"`) reached `current_token_text`'s scalar-only
   `expect` and panicked. tsc `parseNewExpressionOrNewDotTarget` calls
   `parseIdentifierName()`: keywords are accepted, any other token yields a
   missing identifier plus TS1003. Fix: `parse_identifier_name(None)`, with a
   tsc-minted fixture (`scripts/observe-utf16-new-meta-property-name.mjs`,
   `crates/syntax/tests/fixtures/utf16-new-meta-property-name.json`) and the
   test `crates/syntax/tests/new_meta_property_name.rs`. The same change removes
   the pre-existing divergence where `new."x"` fabricated an identifier from
   the literal value and `new.if` reported TS1003.
2.–6. Five fail-open observers in xtask kept a PathBuf-era `to_string_lossy()`
   on values that became `JsString`: `h1_emit_acceptance::assert_outcome`
   (emittedFiles) and `assert_writes` (source provenance),
   `h2_2c_acceptance::source_maps_value`, and the two symbol-audit line
   renderers. They now fail closed with `as_str().expect(..)` like their
   sibling sites. Exposure was bounded because the expected side is serde JSON,
   which cannot carry a lone surrogate.

Observations recorded but not changed: the harness `decode_utf16` UTF-16-BOM
replacement (unreachable on the pinned corpus), the emitter sink projection
point, the `/node_modules/` substring test through a lossy copy, the Windows
native-name boundary, and four scalar-side fidelity notes outside this repair
(engine `report_incompatible_stack` identifier test, System `{ 0: a }` binding
element, JSX `decode_entities` edge cases, the builtins activity canary's
lowercase path). The JSX intrinsic fix from §31 stays the only checker change
found by the audit.

### 32.4. Checker JSX test: observation, not semantics

The new checker test `utf16_intrinsic_literal_tag_names_retain_distinct_attribute_types`
failed natively (1736 passed, 1 failed) because it compared only the head
`message_text()` while the tsc fixture holds
`flattenDiagnosticMessageText(messageText, "\n")`, i.e. the head plus the
elaboration `Property 'b' does not exist on type '{ a: number; }'.` on an
indented second line. The three 2322 diagnostics agree in code, position and
head text, which is the identity the fixture exists to witness. The test now
flattens the chain the same way; the checker was not changed for it.

### 32.5. B admission corpus delta: census design

The refusal `ParseDiagnosticsDeferred` is counted, never executed, by the H2
profile runners (`h2_2c_acceptance` counts `Deferred(H2_9)` rows), and every
executed profile row carries empty `source_facts`; therefore the admission
change cannot alter any executed acceptance row, and the delta is a candidate
inventory question. The new xtask subcommand `utf16-literal-recovery-census`
evaluates `SourceFile::has_only_literal_recovery` with the acceptance's own
loaders and parse projection (`h2_2c_acceptance::parse_prepared_source`) over
every corpus row with recorded inputs: the 7,908 recorded execution plans
(compiler and project suites) and every qualification / candidate-input
artifact that embeds its VFS (`h2-1a` … `h2-7b` qualification rows,
`h2-7de`/`h2-8a` candidate inputs), deduplicated by case id. Each row gets a
verdict: `newly-admitted` (a unit retained diagnostics, every unit
literal-only), `newly-refused` (no retained diagnostic but a structural or
silent-missing event), `still-refused`, `unchanged-admitted`, `no-recovery`;
the report also cross-checks Rust diagnostic presence and codes against the
oracle's `source_facts.parse_diagnostic_units` where recorded. Results are in
§32.6.

### 32.6. Focused native tests after the repair (handoff §6 step 2)

Receipt `utf16-focused-native-tests-20260914-003327` (serial, one run each,
`inputs_unchanged=true`, `head_unchanged=true`):

| command | result |
| --- | --- |
| checker `--lib` (1,737 tests) | 1736 passed, 1 failed: the new JSX test (observation flatten, §32.4) |
| fuzz `scalar_wire_boundary_distinguishes_replacement_text_from_unpaired_units` | passed |
| conformance `golden_message_boundary_preserves_scalars_and_refuses_unpaired_units` | passed |
| harness `virtual_normalization_keeps_scalar_corpus_semantics_and_utf16_identity` | passed |
| compiler `h2_7e_declaration_map_apis`, `h2_7d_module_identities`, `h2_7d_declaration_bundles`, `h2_6a_map_option_projection` | 3 + 3 + 4 + 3 passed (2 ignored probes) |
| compiler `h2_7e_declaration_maps` | 6 passed, 2 failed (inherited, below) |
| compiler `contracts` (the five changed modules) | 56 passed, 2 failed (inherited, below) |
| harness `contracts` (`h2_5h_project_emit`, `module_suffixes_oracle_contract`) | 3 passed |
| program `h2_7d_bundle_source_facts` | passed |
| program `contracts` (`config_diagnostics_oracle_contract`, `host_text_decode_contract`) | 7 passed, 1 failed (expectation corrected, below) |
| emitter `contracts` `artifact_sink_contract` | 8 passed |
| `xtask codegen nodes-check`, `xtask schema-audit` (handoff §6 step 5) | exit 0, exit 0 |

The two earlier launches of this queue are the corrupted-artifact incidents
of §32.2 (`utf16-focused-native-tests-20260914-002029` and `-002744`, kept as
they are).

**Four failures inherited from `origin/main`, not caused by the migration.**
All four are "typed boundary" expectations that main's H2.7d/H2.8a admission
commits made stale after the local gate was retired (2026-09-09); the
migration diff of these tests is type-only (`.into()`, `scalar_test_path()`),
no branch-local commit touches `emitter/src/execute.rs`, `plan.rs`,
`compiler/src/lib.rs` or `program/src/loader.rs`, and every relevant
production commit (`d6beaefa8` bundles → production emission 2026-09-07,
`56e666af5` output directories 2026-09-08, `4b0f4d75b` root paths and TS5011
2026-09-08, `57e202e50` config option relations 2026-09-12) is on
`origin/main` inside HEAD's lineage. They are re-run at the merge-base
`f406f1200` in the separate worktree `tsc-rs-main-baseline-f406f120` (its own
target directory) to confirm; see §32.7.

1. `h2_7e_bundle_maps_keep_typed_boundary`: expects `outFile` + declaration
   maps to be refused with `UnsupportedCompilerOption { option: "outFile" }`;
   the runtime now emits `bundle.d.ts` and `bundle.d.ts.map`.
2. `h2_7e_cli_status_and_exit_match_every_typescript_observation`, case
   `options/declaration-dir`: the CLI comparison rebases the fixture's
   API-route observation (`types/a.d.ts`, exit 0) onto a `-p tsconfig.json`
   run. tsc 6.0.3's `getCommonSourceDirectory` uses the config directory when
   `configFilePath` is set and `rootDir` is not, so the real compiler prints
   TS5011 and `types/src/a.d.ts` with exit 2 on the same tree (ground truth
   `tsc-cli-ts5011-groundtruth-20260914-005619`), exactly what the Rust CLI
   prints. The rebased expectation, not the CLI, is what disagrees.
3. `emit_session_contract::h2_3a_narrow_out_dir_and_source_family_boundary_fails_closed`:
   "TypeScript-only outDir remains H2.8a" — `outDir` is admitted since
   `56e666af5`.
4. `emit_session_contract::unsupported_options_and_unadmitted_extensions_fail_before_the_first_sink_call`:
   "AMD outFile remains owned by a later bundle slice" — bundles are admitted
   since `d6beaefa8`.

None of the four expectations was changed in this train; they belong to the
owners of those admissions and are listed for the review as pre-existing.

**One expectation corrected as part of A.**
`host_text_decode_contract::raw_unpaired_utf16_is_invalid_but_escaped_surrogates_follow_read_json`
asserted that an escaped `\ud800` in a package.json `name` reads back as
`p\u{FFFD}kg`, i.e. the pre-repair replacement. tsc's `parseJsonText` +
`convertToObject` (and `JSON.parse`) keep the lone surrogate — code units
`p, D800, k, g`, observed from the vendored compiler — and the package name is
an identity (package ids, resolution caches), so the owned-value repair keeps
it too. The assertion now compares the UTF-16 units. The raw UTF-16 half of
the test (an unpaired unit in the file bytes fails closed with `InvalidData`)
is unchanged and still passes.

### 32.7. Baseline confirmation of the inherited failures

`main-baseline-h2-7e-20260914-012637`: a pristine detached worktree at the
`origin/main` merge-base `f406f12009cd476e28f9e0dcfb3ae4030563fa67`
(`~/dev/tsc-rs-main-baseline-f406f120`, target directory `target/baseline`)
ran `cargo test -p tsc-rs-compiler --test h2_7e_declaration_maps` and the two
`emit_session_contract` tests. Results: `h2_7e_declaration_maps` 6 passed /
2 failed (`h2_7e_bundle_maps_keep_typed_boundary`,
`h2_7e_cli_status_and_exit_match_every_typescript_observation`) and the
contract pair 0 passed / 2 failed — the same four failures as §32.6, on a
tree that contains none of this branch's commits or the uncommitted
migration. They are pre-existing on `main`.

### 32.8. Parser repair and second test round

`utf16-round2-native-tests-20260914-012217` (`inputs_unchanged=true`):
`crates/syntax/tests/new_meta_property_name.rs` passes against the tsc-minted
fixture (10 cases: lone-surrogate string/template names, a pair, a scalar
string, a keyword, `new.target`, `new.1` which lexes `.1` as a numeric literal
and is a `NewExpression`, `.js`/`.tsx` variants, EOF after the dot); the
whole syntax crate passes (175 unit tests plus the integration targets); the
checker JSX test passes after the flatten correction (1 passed, 1,736
filtered). The xtask commands of that round failed to compile on a one-line
type slip in the new census module (`Vec<u8> + &[u8; 1]`), corrected before
round 3; no other source changed between the rounds.

### 32.9. B admission census results and the complete-command linkage (handoff §6 step 4)

`cargo xtask utf16-literal-recovery-census` ran in
`utf16-round3-native-tests-20260914-012217` (exit 0, 87 minutes at the
demoted priority) and wrote
`target/declaration-comment-ranges-runs/utf16-literal-recovery-census.json`
(SHA-256 `16fafabaac5e46d99e0caebd37f215d387a2199dbd36f0473f9fbd678cb78efb`):

| fact | value |
| --- | --- |
| rows evaluated (unique case ids) | 14,219 (14,329 claimed; 110 could not be loaded) |
| `no-recovery` | 13,598 |
| `still-refused` (a structural or silent-missing recovery remains) | 571 |
| **`newly-admitted`** (retained diagnostics, every unit literal-only) | **50** |
| `newly-refused` (a row admitted before that the predicate now refuses) | **0** |
| `unchanged-admitted` (zero-diagnostic literal events only) | 0 |
| load failures | 110: 108 project descriptors with `resolveMapRoot` / `resolveSourceRoot` / `declarationDir` / `emittedFiles`, 2 qualified fixtures refused by option validation — all rows that a typed refusal stops before any preflight |
| TypeScript parity mismatches | 5 (below) |

The 50 newly admitted rows are exactly the literal family: unterminated
string and template literals (1002, 1160, 1126 at end of text), invalid
hexadecimal/unicode escapes (1125, 1198, 1199) — `stringLiteralsErrors`,
`taggedTemplatesWithIncompleteNoSubstitutionTemplate1/2`,
`unterminatedStringLiteralWithBackslash1`, the
`templateStringUnterminated1–5` pairs, `unicodeExtendedEscapesInStrings` /
`InTemplates` 07/12/14/17/19/20/21/22/24/25 for es5 and es6,
`invalidTaggedTemplateEscapeSequences` for es5/es2015/esnext, the
30-unit `parser.numericSeparators.unicodeEscape` row, and two scanner rows.
The still-refused code histogram is structural (1005, 17006, 1109, 1128,
1003, 1121, 1487, …); no row moved from admitted to refused, so the
predicate cannot regress any executed acceptance row.

Parity: the oracle's `source_facts.parse_diagnostic_units` agree with the
Rust parser on every compared row except (a) three `requireOfJsonFile*`
rows whose only recorded units are `.json` sources — JSON takes the JSON
text route and is never preflighted, so the census now skips `.json` units
in the parity comparison — and (b) `declarationEmitUnknownImport2.ts`
(es5 and es2015), where the oracle records one TS1005 and the Rust parser
retains two; both rows are `still-refused` either way. That duplicate
TS1005 is a parser parity observation outside this repair, recorded for the
syntactic band's owner.

Correction (fix round, §33.11): the previous sentence is wrong. The oracle
records a unit's codes as a `Set`, and the pinned parser also reports TS1005
twice for that row; the census now compares code presence, and the re-run
in §33.14 records the parity result on the corrected comparison.

Linkage to complete commands: `scripts/observe-utf16-literal-recovery-corpus.mjs`
reads the census, rebuilds each newly admitted row's qualified VFS from the
same artifact the census loaded (the four recorded-plan rows resolve to their
`h2-5g`/`h2-5h` embedded inputs), projects the harness directives with the
established floor exactly as `load_qualified_compiler_emit_with_symlinks`
does, runs the complete tsc command on a hermetic VFS host twice, and wrote
`crates/compiler/tests/fixtures/utf16-literal-recovery-corpus.json`
(SHA-256 `8974a5d03595a5d9cea728556467ee70e187a5295a57e12ecdb71eb7c0d2e87c`;
50 cases, 0 skipped, 100 executions, all exit 2 with the literal-family
diagnostics reported and every write still emitted). The native comparison
is `crates/compiler/tests/h2_8a_utf16_literal_recovery_corpus.rs`: the same
qualified loader at the established floor, `emit_command_for_harness`, and
the full tuple (writes with callback/materialized bytes and metadata,
reported diagnostics, status writes, exit code, emit result) compared twice
against the upstream runs. Its result is recorded in §32.10.

### 32.10. The one corpus divergence: prologue-only files and the detached prefix

`utf16-round4-native-tests-20260914-030624`: the native comparison of the 50
newly admitted rows matched 49 complete tuples exactly and diverged on
`conformance/scanner/ecmascript5/scannerS7.8.4_A7.1_T4.ts#default` in the
emitted JavaScript only (diagnostics, status and exit identical): tsc's
callback text ends with the file's two detached copyright lines repeated
after `"\u000G";`, tsc-rs's does not.

The source has no trailing comments. The mechanism is upstream emission
order, not the recovery: `emitPrologueDirectivesIfNeeded` prints the
string-literal prologue (with its leading comments) before the source file,
then `emitSourceFile` sees a synthesized `"use strict"` at `statements[0]`
and runs `emitBodyWithDetachedComments`, whose
`emitDetachedCommentsAndUpdateCommentsInfo` writes the detached prefix
again. tsc-rs's `write_transformed_source_file` emits that prefix (and the
helpers) when the statement loop reaches the first non-prologue statement,
so a file whose statements are all prologue directives never reached it.
Reproduced with a valid escape (`// A\n// B\n\n"custom";`), so the gap is
independent of B and was hidden only because such rows were refused before.

Fix: after the loop, when body statements exist and every statement was a
prologue, emit the detached prefix, the helpers and the declaration-file
triple-slash directives exactly as the first-body-statement path does
(`crates/emitter/src/printer.rs`, `statement_count == helper_offset`).
Witness: `scripts/observe-prologue-only-detached-comments.mjs` minted
`crates/compiler/tests/fixtures/prologue-only-detached-comments.json`
(SHA-256 `65ceacc91bd3fbb4f3260d9f1d4d8fff2971437fa35f0814adc0f80b8f9a27f4`,
8 cases, 2 identical repetitions) covering the two-line and one-line
detached headers, a block-comment header, two custom prologues, a custom
prologue followed by a statement (the already-correct path), an attached
header (no repetition), an original `"use strict"` only (no repetition,
`statements[0]` is not synthesized) and the invalid-escape variant;
`crates/compiler/tests/h2_8a_prologue_only_detached_comments.rs` compares
the emitted text, exit code and diagnostic codes twice. Results of the
native runs after this fix are in §32.11.

### 32.11. Final native rounds on the frozen source

`utf16-workspace-all-targets-check-20260914-032439`: the complete
`--workspace --all-targets --keep-going` type check is exit 0 with the parser
repair, the printer repair, the census subcommand, the fail-closed observers,
the new tests and fixtures (`inputs_unchanged=true`).

`utf16-round5-native-tests-20260914-032*` (`inputs_unchanged=true`):
`h2_8a_prologue_only_detached_comments` 1 passed (8 cases × 2),
`h2_8a_utf16_literal_recovery_corpus` 1 passed — all 50 newly admitted
corpus rows now equal tsc's complete tuples twice — and the emitter library
suite 505 passed. Together with §32.8 this closes handoff §6 steps 2, 4 and
5 on the final source.

Source identity for the final re-verification:
`utf16-final-source-freeze-20260914-033140` (HEAD `b652451f0`, tracked patch
`369 files changed, 26213 insertions(+), 17280 deletions(-)`, SHA-256
`424d4a565e57c63b9f96905a9c13e0db4fd467c083f149cc3bf2dde91c9ae953`, 100
untracked files archived, 1,204 hashed inputs). Documents under
`docs/design/greenfield/slices/` are listed in that manifest but continue to
change until the review packet is final; a closing freeze is recorded in
§32.12 together with the re-verification results.

### 32.12. Final re-verification on the frozen source (handoff §6 step 6)

`utf16-final-reverification-20260914-033109.json` sequenced every saved
runner on the frozen tree of §32.11; each runner kept its own input snapshot
(`inputs_unchanged=true` everywhere) and repetition rule:

| scope | result | receipt |
| --- | --- | --- |
| original adjacent 23 | complete exact, captured twice, supplemental captures identical | `utf16-adjacent-complete-20260914-033109` |
| existing UTF-16 64 | complete exact ×2 | `utf16-prior64-complete-20260914-033206` |
| C controls 16 | complete exact ×2 | `utf16-tagged-controls-complete-20260914-033426` |
| A/B/C1 65 | 56 complete exact ×2, 9 typed recovery refusals ×2, no partial writes | `utf16-identity-recovery-complete-20260914-033500` |
| noEmit 14 | complete exact ×2 | `utf16-noemit-command-complete-20260914-033653` |
| declaration-comment 41 | 3 test functions passed (exact ×2) | `utf16-prior41-complete-20260914-033735` |
| original H2.5h 4 (typed and complete), G4a/G4b | required comparisons passed; G5c strict observation still exit 101 with the same pre-repair declaration return-inference mismatch | `utf16-original-routes-regression-20260914-033913` |
| checker library | 1,737 tests ×2 (1,736 plus the JSX intrinsic identity test) | `utf16-checker-library-20260914-034034` |
| emitter library and literal integration | 505 + 2 ×2 | `utf16-emitter-literal-suite-20260914-034303` |
| program library | 46 ×2 | `utf16-program-lib-20260914-034311` |
| raw UTF-16 source boundary | ×2, unpaired raw units still rejected by the decoder, pairs identical | `utf16-raw-source-boundary-20260914-034320` |

Added on this continuation and green on the same tree: `new_meta_property_name`
(10 cases ×2, §32.8), `h2_8a_utf16_literal_recovery_corpus` (50 rows ×2,
§32.10/§32.11), `h2_8a_prologue_only_detached_comments` (8 cases ×2), the
syntax crate, the changed harness/program/emitter/compiler test targets of
§32.6, `xtask codegen nodes-check` and `xtask schema-audit`.

`main-baseline-xtask-6c-manifest-20260914-0343*`: the one xtask unit test
that failed in `utf16-round4-native-tests-20260914-030624`
(`h2_2c_acceptance::h2_7b_tests::h2_6c_current_manifest_keeps_only_unclosed_historical_refusals`,
which expects the H2.6c known-divergences manifest to still carry 130
`outDir` and 4 `rootDir` refusals) fails identically at the pristine
`origin/main` merge-base: main shrank that manifest on 2026-09-08 without
updating the test. Inherited, unchanged here; the other 39 selected xtask
tests pass.

Not run, by directive: full local CI, the oracle chain walk, hosted
acceptance and `cargo xtask acceptance` (the user runs acceptance at merge).
The profile artifacts under `ratchets/` and the oracle scripts under
`crates/oracle/` are untouched; the 50 newly admitted rows stay
`deferred-to-slices` in their profiles until those profiles are re-minted,
which is why the admission change cannot alter a hosted result today.

Closing freeze after the documents were finished: `utf16-closing-source-freeze-20260914-034941` (code inputs
only — crates, Cargo files, scripts — identical to the
`utf16-final-source-freeze-20260914-033140` code manifest:
`True`; tracked patch SHA-256
`35f01c2643f65d45f121f7086bb579762e80669d3ba2a8337205ab38ed0aee09`, 369 files changed, 26278 insertions(+), 17280 deletions(-),
100 untracked files archived).

## 33. Implementation-review fix round (2026-09-14)

The independent review (`h2-8a-utf16-adjacent-implementation-review-response.md`)
left three must-fix items (M-1…M-3), the full-target evidence item (M-4) and
a set of follow-ups. This section records, per finding, what changed, why,
and how it was verified. Starting point: the same uncommitted diff in
`~/dev/tsc-rs-declaration-comment-design` at HEAD `b652451f0`. The §32
freezes and receipts are untouched; every post-fix receipt lives in a new
directory (§33.14). Hosted acceptance remains the user's.

Rules applied throughout: no existing expected value was changed to make a
test pass (the one corrected unit-test expectation in §33.4 is backed by a
pinned-compiler observation that contradicts the old value); every
behavioural change carries a control minted twice from the pinned compiler
(`scripts/observe-utf16-review-fix-controls.mjs` →
`crates/compiler/tests/fixtures/utf16-review-fix-controls.json`, 25 complete
commands ×2, SHA-256 `2d55c50860edb446eb8fe9165431906adadf07f6ec49f6fdf0530bd4eddd5e52`;
`scripts/observe-utf16-scanner-escape-diagnostics.mjs` →
`crates/syntax/tests/fixtures/utf16-scanner-escape-diagnostics.json`, 23
inputs ×2, SHA-256 `aad3487634bee584d9d38969e4450e44a8426cb30d9931a3b7464f8551f5586b`);
Serialize paths never route through a lossy conversion; the production
admission predicate (`preflight_source`) is not weakened.

### 33.1. M-3 / A-1: `output_directories_preserve_distinct_js_components` expected `"/work/"`

- Change (test only): the test derives the common directory through
  `common_source_directory(&CompilerOptions::default(), None, &paths, "/work", true)`
  instead of `inferred_common_source_directory(...)`.
- Reason: tsc's `getCommonSourceDirectory` (`_tsc.js:116460-116475`) is the
  function that appends the trailing separator;
  `computeCommonSourceDirectoryOfFilenames` (`121909-121939`, the port
  `inferred_common_source_directory`) returns `/work` without it, and
  `getSourceFilePathInNewDir` needs the separator to strip the prefix. Both
  production functions were correct; the test had paired the wrong tsc
  function with the expectation.
- Verification: `fix-program-path-identity` in
  `utf16-fix-round-focused-native-tests-20260914-082732`: 8 passed
  (`path_identity_contract`, previously 480/481 in the review run).

### 33.2. M-1 / B-1: the legacy-decorator recovery adapter

- Change: `SourceFile::discard_parse_recovery_for_harness()` (`#[doc(hidden)]`,
  `crates/syntax/src/lib.rs`) clears the retained diagnostics together with
  the committed recovery record; the adapter
  `transform_and_print_legacy_decorator_recovery_at_target` calls it instead
  of `parse_diagnostics.clear()`.
- Reason: the two contract tests deliberately drive the transform pipeline
  over erroneous trees that upstream still emits (decorator placement rules,
  `this[#x]`); `preflight_source` must keep refusing a tree whose recovery
  record is non-empty, so the bypass is explicit on the test side rather
  than a weaker predicate. Clearing only the diagnostic list is exactly the
  mismatch the predicate exists to catch
  (`clearing_retained_messages_does_not_erase_structural_recovery`, which
  now also asserts `recovery_events > 0`, §33.12).
- Verification: `fix-emitter-active-transform-legacy` (46 passed, including
  `legacy_explicit_this_rules_keep_admission_serialization_and_runtime_distinct`
  and `legacy_private_expression_flag_is_scanned_before_runtime_admission`,
  the two that failed in the review run) and `fix-emitter-builtins-preflight`
  (2 passed) in `utf16-fix-round-focused-native-tests-20260914-082732`.

### 33.3. M-2 / B-2: declaration diagnostics in the harness NoEmit command

- Change (production, harness route): `emit_command_for_harness` clones the
  prepared program before the consuming `run()` when
  `noEmit && getEmitDeclarations(options)` (`declaration || composite`,
  `_tsc.js:18151-18156`, ported as `get_emit_declarations`), and after the
  semantic pass opens a second session
  `ProgramSession::new(prepared).get_declaration_diagnostics(EmitSelection::WholeProgram)`
  only while the syntactic, options, global and semantic streams are all
  empty. `CliEmitSessionOutcome` gains a `declaration_diagnostics` stream
  that `into_reported` appends after the semantic stream under the same
  gate (`emitFilesAndReportErrors`, `_tsc.js:129433-129440`); the emit-mode
  constructor leaves it empty (an Emit-mode program can never carry
  `noEmit`).
- Reason: `run()` keeps H0's separate no-emitter contract and its typed
  zero-activity proof, so the declaration getter is not folded into it; the
  public getter already accepts either prepared mode. The second checker
  costs one extra check only in the gated configuration and computes the
  declaration diagnostics tsc computes through `getEmitResolver`.
  `composite` + `noEmit` is still refused earlier by
  `no_emit_without_build_info` (unsupported build info), so the gate is
  reachable through `declaration: true` only.
- Controls (`utf16-review-fix-controls.json`): `m2-noemit-declaration-ts4094`
  (TS4094, exit 2), `-clean` (exit 0), `-semantic-first` (TS2322 only: the
  gate stays shut), `-syntactic-first` (TS1109 only), `-without-declaration`
  (exit 0), `-two-files` (three TS4094 rows in program order).
- Verification: `review_fix_controls_match_complete_commands_twice`, 25/25
  exact ×2 (capture `capture-review-fix-2`, and the
  `compiler-utf16-top-level-tests` label of the full run, §33.14). The
  existing noEmit controls (`no_emit_controls_match_complete_commands_twice`,
  14 ×2) and the H2.7c getter tests (`fix-compiler-h2_7c-getters`, 24 passed)
  are unchanged.

### 33.4. A-2: "Did you mean" candidates and values use tsc `symbolName`

- Change: `CheckerState::symbol_name` (`merge.rs`, port of `symbolName`,
  `_tsc.js:11452-11457`: a private class member renders its `#name` source
  text, everything else unescapes) replaces the raw
  `escaped_name.unescape()` candidate namer in
  `get_spelling_suggestion_for_name`, the written face in
  `get_suggestion_for_nonexistent_property` (`spell.rs`; element access and
  object-literal excess property flavors), and the raw unescape in the
  property-access flavor (`access.rs`, `reportNonexistentProperty`,
  `_tsc.js:75452`; the related "is declared here" row uses the same value).
  The JSX excess-property branch keeps `symbol_name_as_written_slice`: tsc
  renders `symbolToString(suggestionSymbol)` there (`_tsc.js:65373-65374`).
- Reason: tsc renders these suggestions with `symbolName`, never with the
  written face; the written face quoted computed names (`'"abcd-"'`), and
  the raw candidate namer offered `__#1@#hidden`, which the length filter of
  `getSpellingSuggestion` rejected, so `this.hidden` → `#hidden` could not
  fire (once the candidate fired, the property-access flavor still printed
  the escaped key; the `access.rs` change closes that).
- Corrected expectation: `check::tests::excess_property_suggestion_uses_the_written_string_literal_name`
  expected `Did you mean to write '"ns:attribute"'?`; the pinned compiler
  prints `'ns:attribute'` (probe 2026-09-14: `declare let t: { "ns:attribute": string }; t = { attribute: "x" };`
  → TS2561 with `'ns:attribute'`, and the same for a computed `["ns:attribute"]`
  target), so the test is renamed
  `excess_property_suggestion_uses_the_unescaped_symbol_name` with the tsc
  value. `jsx::tests::namespaced_jsx_attribute_suggestion_uses_symbol_to_string_face`
  (`'"ns:attribute"'`) stays as it is and matches the probe of the JSX
  branch.
- Controls: `a2-element-access-quoted-candidate`, `a2-property-access-quoted-candidate`,
  `a2-excess-property-quoted-candidate`, `a2-private-name-candidate`
  (`Did you mean '#hidden'?` plus the `'#hidden' is declared here.` row),
  `a2-leading-underscore-candidates` (tsc's own escaped first argument
  `'___protoy'` in the element-access flavor is reproduced), `a2-lone-surrogate-candidate`.
- Verification: the first focused run reported exactly one divergence
  (`a2-private-name-candidate`: `__#1@#hidden`), fixed by the `access.rs`
  change; the re-run is 25/25 exact ×2; `fix-checker-lib` 1735 passed / 2
  failed in the first run (the two unit tests above), both green after the
  correction and the JSX revert; the full `checker-lib` run of §33.14
  covers the final source.

### 33.5. A-3: declarationless symbols render their nameType face

- Change: `symbol_name_as_written_slice` falls back to
  `symbol_name_from_name_type_slice` (now `pub(crate)`) and then to
  `symbol_name` when no declaration carries a name.
- Reason: `getNameOfSymbolAsWritten` (`_tsc.js:55586-55588`) tries
  `getNameOfSymbolFromNameType` before `symbolName`; mapped-type property
  symbols (`Record<"a-b", number>`) have no declarations, so TS2741 printed
  `Property 'a-b'` where tsc prints `Property '"a-b"'` (and `[-1]` for a
  negative numeric name, `"1"` → `1`, `"ab"` → `ab`).
- Controls: `a3-record-quoted-name`, `a3-record-lone-surrogate-name`,
  `a3-record-identifier-name`, `a3-record-negative-numeric-name`,
  `a3-record-numeric-name` (all exact ×2).

### 33.6. A-4: the write callback keeps the JavaScript string

- Change: `EmitArtifact` stores an `EmitCallbackText` (the writer's
  `GeneratedText`: raw UTF-16 units plus the UTF-8 sink projection);
  `callback_text()` / `callback_bytes()` / `materialized_bytes()` keep
  returning the projection, `callback_units()` returns the raw units. The
  three producers (`execute.rs` JavaScript with and without the source-map
  URL suffix, `declaration_map.rs`, `declarations/orchestration.rs`) pass
  the printed `GeneratedText` through instead of `printed.text().to_owned()`.
- Reason: tsc's `writeFile` callback receives the raw string
  (`_tsc.js:16644-16650`) and only `sys.writeFile` (`5164-5182`) projects an
  unpaired unit to U+FFFD; a `.d.ts` literal type built by `typeToTypeNode`
  carries the raw unit (NoAsciiEscaping), so the two faces differ at the API
  boundary while the file bytes agree.
- Controls: `a4-declaration-callback-lone-unit` (ES2015 and ES5): the
  `.d.ts` callback for `k2` carries unit `D800` while `k` (declaration
  reused from the `as const` assertion) keeps the `\uD800` spelling; the
  identity-recovery test's `captured_write` now asserts that its frozen
  callbacks are scalar (fail-closed) instead of projecting silently.
- Verification: the new control test (25/25 ×2), `fix-emitter-artifact-sink`
  (8 passed), `fix-emitter-utf16-escaping-writer` (2+2 passed),
  `fix-compiler-identity-recovery-controls` (2 passed: 65 + 14 commands ×2
  with the fail-closed assertion).

### 33.7. A-5: `impl DiagnosticArgument for EscapedName` removed

- Change: the impl in `crates/types/src/escaped_name.rs` is deleted.
- Reason: it offered an escaped spelling as a diagnostic value; no caller
  existed, and diagnostic names must always pass through
  `unescapeLeadingUnderscores` (tsc `symbolName` / `idText`).
- Verification: workspace all-targets check
  `utf16-workspace-all-targets-check-20260914-082507` (exit 0, 0 errors,
  inputs unchanged).

### 33.8. A-6: `/node_modules/` membership on the JavaScript string

- Change: `resolve_runtime_dependency_symlinks` (`loader.rs`) uses
  `display_path_contains_node_modules(JsStr)`, a byte-window test accepting
  either separator on both sides, with unit tests (ASCII, backslashes, a
  lone surrogate before and inside the component).
- Reason: the previous `to_string_lossy().replace('\\', "/").contains(...)`
  was result-equivalent (U+FFFD never equals an ASCII byte) but was the only
  production path predicate evaluated on a lossy copy.
- Verification: `fix-program-lib-node-modules` (2 passed).

### 33.9. A-7: scanner escape diagnostics (lane A2 F1–F3)

- F1: `scan_escape_sequence` reports `Unexpected_end_of_text` unconditionally
  (`_tsc.js:9066-9072`); a tagged template ending in `\` (report flag off)
  previously lost TS1126 and reported TS1160 instead. The parser's
  same-position rule then drops the following TS1160 exactly as tsc's
  `parseErrorAtPosition` does (the fixture shows one TS1126 for tagged,
  untagged and string forms alike).
- F2: `scan_extended_unicode_escape` parses an overflowing digit string as a
  value above 0x10FFFF (TS1198 from `escapedStart`, `9221-9226`) instead of
  "no digits" (TS1125 at the end).
- F3: `scan_unicode_escape` returns the escape flag together with the
  character and resets `pos` on overflow; the identifier consumers (JSX
  identifier parts, identifier start, identifier parts, private names)
  insert the flag only when they accept the character
  (`scanIdentifierParts`, `9285-9305`; the backslash arm of `scan`,
  `9751-9770`). Previously a rejected escape left `UNICODE_ESCAPE` on the
  preceding token (`if\u0020(x) {}` gained a TS1260) and an overflow left
  `pos` past the digits (`a\u{FFFFFFFFFF}` produced a different token
  sequence).
- Fixture: 23 inputs (parse diagnostics with message units, identifier
  texts and positions in source order, statement counts), test
  `crates/syntax/tests/scanner_escape_diagnostics.rs`.
- Verification: `fix-syntax-scanner-escape-diagnostics` (1 passed, 23 ×2
  inputs), `fix-syntax-lib` (175 passed), the syntax fixture tests
  (`new_meta_property_name`, `template_escape_flags`, `template_flags`,
  `recovery_provenance`, `owned_literal_values`: all passed), the b6
  EOF-backslash rows of the identity-recovery controls (exact ×2).

### 33.10. C-1: generated call callees are parenthesized

- Change: `NodeFactory::parenthesize_left_side_of_access` is `pub(crate)`;
  both tagged-template hosts' `create_call` (`es2018.rs`, `es2015.rs`) apply
  it to the callee before building the `CallExpression`
  (`createCallExpression`, `_tsc.js:22579-22585` → `20466-20471`).
- Reason: an optional-chain tag is lowered to a conditional before the
  tagged-template lowering runs; without the rule the ES2018/ES2015 output
  read `… ? void 0 : a.b(templateObject_1 || …)` and changed meaning.
  Helper callees (identifiers, property accesses, parenthesized nodes) are
  left-hand-side expressions and pass through unchanged.
- Controls: `c1-es2017-optional-property-invalid`, `c1-es5-optional-property-valid`,
  `c1-es2015-optional-element-invalid`, `c1-es2015-optional-call-invalid`,
  `c1-es2017-optional-chain-long-invalid`, `c1-es2015-parenthesized-tag-valid`
  (all exact ×2, TS1358 reported by the checker where tsc reports it).
- Verification: the control test plus `fix-compiler-tagged-template-controls`
  (1 passed) and `fix-compiler-literal-recovery-corpus` (50/50 ×2).

### 33.11. B-4: the duplicate TS1005 note and the census parity comparison

- Correction to §32.9: the `declarationEmitUnknownImport2` mismatch was not
  a parser parity observation. The h2-7b oracle records
  `parse_diagnostic_units[].codes` as a `Set`; the pinned parser also
  reports TS1005 twice for that row. The census now compares code presence
  (sorted, deduplicated) on both sides.
- Verification: the census re-run (`utf16-fix-round-census-native-tests-20260914-091507`,
  exit 0, inputs unchanged, 4,924 s on its own target directory concurrently
  with the full run) wrote `utf16-literal-recovery-census-fixround.json`
  (the §32.9 artifact is kept): 14,219 rows, verdicts unchanged
  (newly-admitted 50, no-recovery 13,598, still-refused 571, 110 load
  failures), TypeScript parity compared 2,605 rows with **0 mismatches**
  (§32.9 recorded 5: the three `.json` units now skipped and the two
  `declarationEmitUnknownImport2` rows now compared as sets).

### 33.12. B-5: the refusal message carries the recovery event count

- Change: `TransformError::ParseDiagnosticsDeferred` gains `recovery_events`
  (committed events, including silent missing nodes and suppressed reporting
  attempts); `preflight_source` fills it from `parse_recovery().events().len()`;
  the display reads "emit recovery for {count} parse diagnostics
  ({recovery_events} recovery events) is deferred to {owner_slice}".
- Verification: `clearing_retained_messages_does_not_erase_structural_recovery`
  asserts `count: 0` with `recovery_events > 0` (`fix-emitter-builtins-preflight`);
  the identity-recovery negative controls still match the typed refusal
  (`b3-b4-rejected/*`).

### 33.13. Recorded, not implemented

- B-3 conflict-marker trivia: outside this repair's admission boundary; the
  producer must set `ScanError.trivia_kind` when implemented (review §3.2).
- B-6 `current_node` reuse scan: performance only; the tree is identical.
- A-9 `encodeURI` failure exit code (tsc: uncaught `URIError`, exit 1):
  policy left to the CLI owner; no writes either way.
- A-10 escape/unescape and `escapeString` duplicates: refactor only, no
  behavioural difference recorded.
- C-2 (printer wraps a parsed optional-chain tag), C-3 (ES2018 parameter
  object-rest initializer), C-4 (`for await` / async generator `super.x`
  mappings): outside C; recorded for the printer, ES2018 and source-map
  owners.
- Observed while porting `symbolName`: the "Cannot find name" family
  (`class.rs`, `resolve.rs`, `modules.rs`) renders suggestions with
  `symbol_display_name` (unescape) where tsc uses `symbolToString`; not
  probed, not changed, recorded for the checker owner.

### 33.14. M-4: full-target results, inherited failures, evidence directories

The review's compiler-contracts run finished after the review was saved:
`utf16-review-evidence-native-tests-20260914-045703` `compiler-contracts-full`
416 passed / 15 failed / 16 ignored (11,733 s, inputs unchanged). Two of
the 15 are the known main-inherited `emit_session_contract` rows (§32.7).
The other 13 were re-run at the last pre-WTF-8 commit of this branch
(`f0aaa2de2` = `b652451f0^`, worktree `~/dev/tsc-rs-pre-wtf8-f0aaa2de2`,
own target directory, receipt `pre-wtf8-baseline-20260914-081736`): the
same 13 tests fail there with the same failing row sets (class-field alias
map positions 28, hoisted declaration export ranges 4, static initializer
map ranges 4, ellipsis comment owners 4, export name syntax maps 8,
import helpers 3, token comment phases 1, plus `h2_7a_m4_controls` ×3,
`programmatic_node_module_resolution_relationships_keep_exact_module_names`
and the two `source_map_emit_witness_contract` outDir rows). They predate
the WTF-8 migration and this fix round; none is caused or changed by
either. Notable for B: `export-name-syntax-maps` `*/es5/direct-extended-unicode`
(`export const \u{10400} = 1;` at ES5, three parse diagnostics matching
tsc's TS1127 + TS1005 ×2) and `token-comment-phases/recovery/import`
(`export /* export */ import value from './dep'`) were refused by the
pre-migration predicate (any parse diagnostic) exactly as they are by the
literal-only predicate now; the census claim "newly-refused 0" (§32.9)
is about the recorded corpus and ratchet universes, and these fixture rows
sit outside it.

Evidence directories of this round (all new, under
`target/declaration-comment-ranges-runs/`):

| directory | scope | result |
| --- | --- | --- |
| `utf16-review-evidence-native-tests-20260914-045703` | pre-fix source: diagnostics / types / binder / host / program (all targets), emitter `contracts`, compiler `contracts` | program 480/481 (M-3), emitter 450/452 (M-1), compiler 416/15/16 (all 15 inherited, above); inputs unchanged |
| `pre-wtf8-baseline-20260914-081736` | `f0aaa2de2`, the 13 unexplained compiler-contracts tests | the same 13 fail with the same row sets (4 passed / 13 failed / 430 filtered) |
| `pre-wtf8-baseline-emitter-20260914-091946`, `pre-wtf8-baseline-emitter-nff-20260914-092844` | `f0aaa2de2`, every emitter test target (`--no-fail-fast` in the second) | only `list_comment_flags_contract` fails (the two `call-*/ParentNoNestedComments/retained` rows); every other emitter target passes |
| `utf16-workspace-all-targets-check-20260914-082507` | post-fix `cargo check --workspace --all-targets` | exit 0, 0 errors, inputs unchanged |
| `utf16-fix-round-focused-native-tests-20260914-082732` | post-fix focused targets (18 labels, §33.1–§33.12) | 16 green; `fix-compiler-review-fix-controls` 1 divergence and `fix-checker-lib` 2 unit tests, both closed by the §33.4 corrections; codegen nodes-check and schema-audit exit 0; inputs unchanged |
| `capture-review-fix-2` (scratch capture of the control test after the corrections) | `review_fix_controls_match_complete_commands_twice` | 25/25 exact ×2 |
| `utf16-fix-round-full-native-tests-20260914-091505` | final source: `cargo fmt --check`, diagnostics / types / syntax / binder / host / program (all targets), checker lib, emitter (all targets), compiler UTF-16 top-level tests, compiler `contracts` (all modules) | fmt clean; diagnostics 49, types 35+3, syntax 175+fixture tests (the new scanner fixture included), binder 71+2, host 3+14+8, program 47+481+…, checker 1737, emitter 505 lib + 452 contracts + top-level tests up to the inherited `list_comment_flags_contract` stop; compiler top-level tests up to the inherited h2_7e stop; compiler `contracts` 416 passed / 15 failed / 16 ignored, the identical inherited set (11,130 s); inputs unchanged |
| `utf16-fix-round-census-native-tests-20260914-091507` | census re-run on its own target directory (`run-utf16-focused-native-tests-alt-target.py`, `TSRS_RUN_TARGET_DIR`) | §33.11: 14,219 rows, verdicts unchanged, parity 0 mismatches / 2,605 compared |
| `utf16-fix-round-tail-native-tests-20260914-123352` | final source, `--no-fail-fast`: every emitter test target, the ten compiler UTF-16 / H2.7 top-level test targets, `codegen nodes-check`, `schema-audit` | emitter: 18 targets green, the inherited `list_comment_flags_contract` alone red; compiler: the two inherited h2_7e rows alone red, `h2_8a_utf16_review_fix_controls` 25 ×2, identity-recovery 65+14 ×2, literal-recovery corpus 50 ×2, tagged-template controls, prologue-only detached comments, h2_7d / h2_6a all green; codegen and schema checks exit 0; inputs unchanged |
| `utf16-final-reverification-20260914-125111.json` (+ the eleven `utf16-*-20260914-1251xx…1300xx` runner directories) | final source, the eleven saved complete-command / library runners of §32.12, each with its own input snapshot and two repetitions | all eleven exit 0, inputs unchanged (adjacent 23, prior 64, tagged controls, identity/recovery 65+14, noEmit 14, prior 41, original routes, checker library, emitter literal suite, program lib, raw source boundary) |
| `utf16-fix-round-source-freeze-20260914-130226` | source freeze after the runs above: `git-head.txt` (`b652451f0`), `git-status.txt`, `source-manifest.json` (477 tracked-modified and untracked files with SHA-256), `tracked-head.patch` (`git diff --binary HEAD`, SHA-256 `e5181491a4ea639e…`), `untracked-files.tar.gz` (107 files), `freeze.py` | the bytes every receipt in this table was taken on; a closing freeze `utf16-fix-round-closing-source-freeze-<stamp>` is taken after this table is written and differs from it only in the documentation files |

The §32 freezes (`utf16-final-source-freeze-20260914-033140`,
`utf16-closing-source-freeze-20260914-034941`) and every §32 receipt are
unchanged.

### 33.15. A-8: the controls-to-findings correspondence table

Copied from the review response §0.2 (the review's requested "対応表"):

| 要求（設計レビュー回答 §5） | 対照 fixture / case id | 最終結果 |
| --- | --- | --- |
| A-1 区別（D800/D801/DC00/FFFD/`\\uD800`/pair/`\u{1F600}`） | `utf16-identity-recovery-controls.json` `a1-distinct-and-pair/target-{1,2}` | exact ×2 |
| A-2 綴り同値（object 1117 / class 2300,2393 / interface 2300,2717） | `a2-object-equivalent`, `a2-class-equivalent`, `a2-interface-equivalent` ×2 targets | exact ×2 |
| A-3 lookup 両端・keyof / Pick / Omit / Record / mapped / narrowing / contextual / reverse-mapped | `a3-lookup-mapped`, `a3-narrowing-context`, `a3-reverse-mapped` ×2 targets | exact ×2 |
| A-4 late-bound | `a4-late-bound` ×2 | exact ×2 |
| A-5 enum 逆写像・const enum・pair 結合 | `a5-enum-pair` ×2 | exact ×2 |
| A-6 template literal type / Uppercase / `a${string}` | `a6-template-types` ×2 | exact ×2 |
| A-7 cross-file / string export-import names / ambient module `"\uD800"` | `a7-cross-file/module-{1,99}`, `a7-ambient-module/module-{1,99}` | exact ×2 |
| A-8 名前種別（`__proto__`, `__call`, `___x`, numeric, private static/instance） | `a8-name-kinds`, `a8-leading-expando` ×2, `a8-private-static-instance`（ES5 は upstream 内部エラーとして `upstream_limitations` に記録） | exact ×2 |
| A-9 診断値（7053 / 2322 / 2353 / 2551-2552） | `a9-diagnostic-values`, `a9-spelling-suggestion` ×2；`ratchets/h2-8a-utf16-diagnostic-values-design.v1.json` | exact ×2 |
| A-10 `.d.ts` literal 型ノードと NoAsciiEscaping の出力バイト | `a10-literal-type` ×2；`crates/emitter/tests/fixtures/utf16-declaration-literal-printer.json`, `utf16-literal-escaping.json`（288 cases）, `utf16-writer.json`（48） | exact ×2 / suites 緑 |
| B-1 noEmitOnError 14 | `b1-noEmitOnError/<row>` 14 | exact ×2 |
| B-2 noEmit + declaration | `b2-noEmit-declaration`；`utf16-noemit-command-controls.json` 14 | exact ×2 |
| B-3 構造的 recovery 負対照 | `b3-b4-rejected/{missing-initializer,missing-class,mixed-literal-structure}` | typed refusal ×2、部分書き込みなし |
| B-4 admitted 外の lexical 負対照 | `b3-b4-rejected/{numeric-separator,hex-digits,regexp-eof,comment-eof,conflict-marker,keyword-escape}` | typed refusal ×2 |
| B-5 tagged invalid は診断なし（C の領域） | `b5-tagged-no-diagnostic`（target 4） | exact ×2 |
| B-6 EOF backslash | `b6-eof-backslash`, `b6-value-eof-backslash` | exact ×2 |
| B corpus 差分（拒否→一致 / 拒否→新規不一致） | `utf16-literal-recovery-census.json`（newly-admitted 50 / newly-refused 0 / still-refused 571）；`utf16-literal-recovery-corpus.json` 50 行 | 50/50 exact ×2（printer 修正後；修正前は 49/50） |
| C-1 ES2015/16/17 invalid cooked、nested、複数 substitution、script 版 | `utf16-tagged-template-controls.json` `es2015-mixed`, `nested-invalid-tag`, `raw-cooked-units`, `script-invalid`；`c1-multiple-spans/target-{2,3,4}` | exact ×2 |
| C-2 ES5 混在の採番と tail 順 | `es5-valid-invalid-valid`；`namespace-mixed`（ModuleBlock） | exact ×2 |
| C-3 二重 visit（object spread tag、nested invalid tag、hoisted temp） | `object-spread-tag`, `nested-four-visits`, `rest-assignment-{valid,invalid,no}-tag`, `function-rest-valid`, `function-nested-invalid`, `escaped-valid-rest-tag`, `function-expression-tag` | exact ×2 |
| C-4 ES2018 retained | `es2018-retained-invalid` | exact ×2 |
| C-5 既存 ES5 控除と 64 件 exact 維持 | `utf16-literals-template-literals.json`（26）, `-string-literals.json`（36）, `-bundle-prologues.json`（2） | exact ×2 |
| v1 / v2 receipt の保存 | `utf16-tagged-template-review-v1.json` SHA `92fbb23e…`（§10.3 の値と一致、test が pin）, `-v2.json` | 維持 |


## 34. Combined PR and the first hosted acceptance (2026-09-14)

The user requested a combined PR including every unmerged PR and hosted CI.
[PR #521](https://github.com/kazhiramatsu/tsc-rs/pull/521) includes the original
heads of #520 and #516 as ancestors. Those two PRs were closed as superseded;
their branches and commit histories are retained. The reviewed dirty tree
was reconstructed in a separate worktree, split into a coordinated ownership
migration and per-finding repair commits, and checked against the closing
freeze: all 477 recorded files match at `af13581fa`. The original review
worktree remains unchanged.

The #516 merge at `a90a02948` adds its Program-owned declaration-specifier
option projection and unchanged fixtures. Its integration adapter retains
`pathsBasePath`, `rootDirs`, and the config path as JavaScript values; the
existing scalar test adapter rejects non-scalar observations explicitly.
The combined workspace all-targets type check passes (exit 0, inputs
unchanged; `target/integration-evidence/focused-20260914-132427/receipt.json`
in `~/dev/tsc-rs-utf16-integration`).

[The first hosted run, 34805824585](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34805824585),
on `a90a02948`, builds successfully and passes diagnostic conformance with
49,024/49,024 matches, FP=0 and FN=0, then H1. It fails at H2.1a because
`invalidTaggedTemplateEscapeSequences.ts#target%3Desnext` now emits instead
of satisfying the historical source-deferred refusal expectation.

This corrects the blanket statement in sections 32–33 that all 50 newly
admitted corpus rows are disconnected from hosted execution until a profile
re-mint. One of those rows is actively checked for refusal by H2.1a. The other
49 occur only in the H2.5a/H2.5g/H2.5h deferred projections and remain deferred
there. Their profiles are not re-minted by this integration.

Use H2.1a's existing current-source-promotion path for that one row, with
its exact case ID, fingerprint
`fb4e084b24b8ab291c29e50a94b555fe1db39d802b301ee58c80a6d740794eb0`,
and original required slice `H2.9`. The historical qualification and both
TypeScript observations stay unchanged. The promoted row must match its
one output write, all three TS1125 diagnostics and command observables twice;
other source deferrals retain their existing checks. The promotion guard
pins the required slice separately for each row, including the existing
`commentsAfterSpread` promotion. The added focused test compares both rows
and rejects changed fingerprints or owners. Combined-test receipts and the
final hosted result are recorded on PR #521.

### 34.1. H2.5h shrink after the second hosted run

[Run 34806679260](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34806679260)
on `dc1e8b091` passes diagnostic conformance (49,024/49,024, FP=0,
FN=0), H1, the corrected H2.1a promotion and every subsequent slice through
H2.5g. H2.5g retains 8,511 exact cases and its existing 516 deferrals.
H2.5h executes all 932 cases before its ordered ratchet join reports exactly
one manifest difference: `optionalChainingInLoop.ts#target%3Des5` is now
exact. There are no new divergences or changed divergence facets in that
run's aggregate report.

Remove only that case from `ratchets/h2-5h-known-divergences.v1.json`,
shrinking 17 entries to 16. The remaining entries, owners and facets are
unchanged. Its SHA-256 changes from
`644c7adc8e173f73cb3e10fb26a838fb236c614bab146422856e3500622b1bf9`
to `720e1822ae7f21fec19f238c4f4aeee1ba35b585568cd9b2f476b25e18a86926`.
The qualification, TypeScript observations, execution denominator and
comparison rules are unchanged. This is the existing shrink-only ratchet's
required retirement of a repaired row, separate from the 49 corpus rows
that remain deferred in their historical profiles.
