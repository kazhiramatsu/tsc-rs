# UTF-16 adjacent repairs: design for cross-review

Status (2026-09-13): native before measurement complete; the requested design
review and §10 follow-up corrections have been accepted. A's representation
implementation has started (§11). **The 23-probe repair is not yet qualified.** The user extended
the scope to all 23 previously recorded adjacent probes, then requested
cross-review before proceeding if the checker repair is not local. The
latest reviewer assignment is **fable-5.1, reasoning effort max**, replacing
the earlier proposed Claude review. Inspection confirms a binder/checker
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
predicates inspect different masks on the same field. Only
`ContainsInvalidEscape` can arise on template fragments; the other bits of
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
