# H2.8a G4a/G4b: declaration comment range selection

Design frozen on 2026-09-12 at trusted base
`f406f12009cd476e28f9e0dcfb3ae4030563fa67` (main after PR #519).
This is the next Codex slice selected by the user. It repairs the two
declaration serializers' choice of **source location**, which supplies
declaration comments and mapping provenance. Production remains unmodified.
The source decisions, Rust representation, file ownership and implementation
steps are fixed below. Seventeen source commands and their internal selector
traces are observed twice. Native before/after execution remains an explicit
implementation gate, not a result of this design exercise.

The design worktree is `/Users/hiramatsu/dev/tsc-rs-declaration-comment-design`,
branch `docs/h2-8a-declaration-comment-ranges`. Continue on this isolated
checkout, or create a runtime branch from the same base and carry these docs.
Do not implement against the older `work/h2-8-output-matrix` checkout.
Revalidate the pins and the parallel file boundary if the base changes.

## 1. Purpose, scope and predecessors

The frozen [A6-37 original matrix](../../../../ratchets/h2-8a-global-after-a6-37.v1.json)
and [cause map](../../../../ratchets/h2-8a-convergence-causes.v1.json) identify:

| Cause | Exact original case ID | Scope of this slice |
| --- | --- | --- |
| G4a | `typescript-6.0.3/compiler/jsDeclarationEmitExportAssignedFunctionWithExtraTypedefsMembers.ts#default` | Wrong assignment location on a serialized function; required complete repair |
| G4b | `typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsClassMethod.ts#default` | Missing prototype-assignment location on a serialized method; required complete repair |
| G4a + G5c | `typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsFunctionsCjs.ts#default` | G4a source-location correction plus mandatory complete observation; its separately recorded return-annotation defect remains G5c |

These are historical failed commands, not a freshly measured three-case
baseline. No repair count is inferred from them. The old 755/769 result is
immutable and is not incremented by focused successes.

The runtime change owns `get_signature_text_range_location` and the
non-omitted method/function signature loop in `make_serialize_property_symbol`,
both private methods of `StatementSerializer` in
`crates/checker/src/node_builder/statements.rs`. It uses the existing binder
assignment classifier. It adds no checker query, syntax type, emitter metadata,
shared comment algorithm, source-text scan or output rewrite.

JSDoc return/type reuse (G5a/G5c), link formatting (G6), declaration specifier
options (G5d / PR #516), class-expression containers (G5b), literal values,
new API admission and the full H2.8 close remain their existing owners. A new
failure is classified from a complete tuple before changing this boundary.

## 2. Parallel ownership

The current Claude assignment is
`target/next-slices-20260912/claude.md` in the canonical checkout:
**H2.5h ca-2a-r2 UTF-16 literal value fidelity**, not the earlier decorator
handoff. Its worktree is `/Users/hiramatsu/dev/tsc-rs-utf16-literals`.
Its current execution document is
`docs/design/greenfield/slices/h2-5h-utf16-literals.md` in that worktree.

For this *new* slice, the user's approved narrower boundary is the two
statement serializer locations above. The previous map-projection ticket's
blanket exclusion of checker files applied to that completed Codex slice;
it does not assign this new declaration-location work to Claude. The actual
UTF-16 implementation at `5b8673122` changes no checker production file.
Its declaration literal-type and symbol-name representation gaps are also
explicitly outside this slice. No checker-wide ownership is inferred here.

| Writer | Fixed paths / operations |
| --- | --- |
| Codex production | `crates/checker/src/node_builder/statements.rs`: the two location decisions and the import of the existing assignment enum/function only |
| Codex tests | `crates/checker/tests/unit/node_builder_statements/tests.rs`: uniquely prefixed `declaration_comment_range_…` controls; new standalone `crates/compiler/tests/h2_8a_declaration_comment_ranges.rs` |
| Codex evidence | New `crates/compiler/tests/fixtures/declaration-comment-ranges.json`, `declaration-comment-range-traces.json`; this packet and its observation notebook; new `ratchets/h2-8a-declaration-comment-ranges-{before,after}.v1.json` execution receipts |
| Claude, excluded here | `crates/syntax/**`, `crates/emitter/**`, every `utf16-literals-*` fixture/observer/test, H2.5h manifest and shrink receipt; checker literal/type/name changes remain outside this ticket |
| Other existing work, excluded here | Checker `lib.rs`, `node_builder/specifier.rs`, compiler `src/lib.rs`, and declaration-specifier fixtures/report (PR #516); H2.8b config work |
| Shared surfaces, read only | Binder classifier; checker `node_builder/{chains,type_nodes,signatures}.rs`; all existing compiler comparators/loaders, `contracts.rs`, xtask acceptance readers, global ratchets, CI and oracle generators |

Compiler tests include the existing helper modules with `#[path = …]`, as the
standalone `h2_8a_require_rewrite.rs` target does. Cargo discovers the new test
target automatically. No shared test registration or comparator edit is needed.
Any newly required production path ends this ticket's implementation scope:
record the evidence, amend the design and check file disjointness before editing.

On this Mac, use one heavy local job, a dedicated target directory, background
priority, `CARGO_BUILD_JOBS=2`, and one test thread. Claude's compiler-contract
run was active during design; no new Rust build or native test was launched.

## 3. Exact source decisions

Authority is vendored TypeScript 6.0.3 `_tsc.js`, whole SHA-256
`1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`.
The whole-function spans and hashes are in §10. `typescript.js`, used for
observations, is separately pinned; changing either compiler invalidates the
source freeze.

### 3.1 G4a: function declaration source location

Call order is `serializeAsFunctionNamespaceMerge` →
`signatureToSignatureDeclarationHelper` → `getSignatureTextRangeLocation` →
`setTextRange2` → `addResult`. Signature/type construction occurs **before**
location selection. Preserve that order and all existing error propagation.

| Ordered branch | TypeScript location | Current Rust gap | Required action |
| --- | --- | --- | --- |
| Signature has no declaration | absent | Already returns `None` | Preserve |
| Declaration has no parent | declaration | Early `?` returns `None` | Keep the declaration as the default |
| Parent is BinaryExpression and `getAssignmentDeclarationKind(parent) == Property` (5) | parent assignment | Accepts every BinaryExpression | Apply the existing enum predicate |
| Parent is VariableDeclaration and has a parent | that parent, normally VariableDeclarationList | Already selects it | Preserve; do not climb to VariableStatement |
| VariableDeclaration has no parent | original signature declaration | Falls back to VariableDeclaration | Fall through to the declaration |
| Every other parent | signature declaration | Binary parents are overaccepted | Preserve the actual declaration; do not skip parentheses |

`ExportsProperty` (1), `ModuleExports` (2), `PrototypeProperty` (3), and
`ThisProperty` (4) are distinct from `Property` (5). This is not a general
"assignment has comments" test. Read the classifier with the owning parsed
source from `checker.binder.source_of_node(parent)`, not the serializer's
target source. Its JavaScript-file filter matters: the direct controls show
Property=5 in JS and TS, but the JS special assignments become None=0 in TS.

Implementation shape, using existing types and names:

```rust
let declaration = self.checker.signature_of(signature).declaration?;
if let Some(parent) = self.checker.parent_of(declaration) {
    if self.checker.kind_of(parent) == SyntaxKind::BinaryExpression
        && get_assignment_declaration_kind(
            self.checker.binder.source_of_node(parent), parent,
        ) == AssignmentDeclarationKind::Property
    {
        return Some(parent);
    }
    if self.checker.kind_of(parent) == SyntaxKind::VariableDeclaration {
        if let Some(list) = self.checker.parent_of(parent) {
            return Some(list);
        }
    }
}
Some(declaration)
```

### 3.2 G4b: method declaration source location

Within the existing `METHOD | FUNCTION` branch, after `omit_type` has been
handled, TypeScript constructs each method signature first. It then uses
`sig.declaration.parent` only when that parent is a BinaryExpression classified
as `PrototypeProperty` (3). All other cases use `sig.declaration` itself.

The Rust loop currently uses `signature.declaration.or(first_property_like)`.
Replace only this loop's location selection. Preserve the other
`first_property_like` fallbacks in the setter, getter, ordinary property and
`omit_type` branches; those are separate upstream decisions.

```rust
let signature_declaration = self.checker.signature_of(signature).declaration;
let location = signature_declaration.map(|declaration| {
    self.checker.parent_of(declaration)
        .filter(|&parent| {
            self.checker.kind_of(parent) == SyntaxKind::BinaryExpression
                && get_assignment_declaration_kind(
                    self.checker.binder.source_of_node(parent), parent,
                ) == AssignmentDeclarationKind::PrototypeProperty
        })
        .unwrap_or(declaration)
});
nodes.push(self.range_member(declaration, location)?);
```

Here the existing `declaration` passed to `range_member` is the newly built
`TransformNode`; `signature_declaration` is an `Option<NodeId>` in the checker
source arena. Keep those identities separate. For a missing signature
declaration, pass `None`; do not invent an anchor from the property's other
declarations. Do not move the `omit_type` branch into this loop.

`isPrototypePropertyAssignment` is exactly BinaryExpression plus enum value 3;
the public binder classifier already implements that dependency. No new
prototype parser/helper or string comparison belongs in this patch.

The G4b upstream predicate assumes that a present parsed signature declaration
has its parent; unlike G4a, it does not explicitly guard a parentless synthetic
declaration. This packet's G4b domain is the ordinary parsed Program route,
whose observed method/function declarations have parents, plus an absent
signature declaration. The Rust `Option` fallback above is defensive handling,
not a claimed TypeScript observation for arbitrary parentless synthetic method
signatures. The four parent/sentinel observations in §6 exercise **G4a only**.
If a native G4b trace reaches a present declaration with no parent, preserve
that evidence and amend this domain before claiming parity; do not silently
count it as one of the observed "other parent" branches.

### 3.3 Classifier and downstream dependency boundaries

For these callers, the BinaryExpression guard excludes the classifier's
Object.defineProperty call arm. Its binary path checks `=`, an access-expression
left side, the rightmost assigned value's `void 0` exclusion, prototype object
initializers, static name/access eligibility and assignment category. Reuse
all of it. Do not replace it with a three-token `C.prototype.m` recognizer.

`range_member`/the G4a caller projects the selected checker `NodeId` with
`project_parse_node`, then calls the existing `set_text_range2`.
That helper clones where necessary, preserves an already-containing original
chain, attaches original provenance **before** assigning a same-enclosing-file
text range, and leaves foreign-source coordinates uncopied. An absent location
still passes through the helper's existing clone decision. No source positions
or original links are set directly by either new predicate.

The binder classifier and these downstream helpers are read-only integration
boundaries, not additional repair work. A failing identity/source control
requires evidence and a separate amendment; it does not authorize editing
`chains.rs`, the factory or printer.

## 4. Rust state and failure semantics

| Upstream state | Existing Rust representation | Producer / owner / consumer / lifetime |
| --- | --- | --- |
| signature and optional declaration | `SignatureId`, `Signature::declaration: Option<NodeId>` | Checker signature table; borrowed by the serializer for this declaration request; no cached location |
| declaration parent and source | Checker/binder `NodeId`, `parent_of`, `source_of_node` | Parsed source topology, read only; location is not a transformed parent or original-node guess |
| assignment category | `tsc_binder::assignment::AssignmentDeclarationKind` | Existing pure source classifier; temporary enum value; no new flags or symbol facts |
| selected location / absent sentinel | `Option<NodeId>` then `BuildResult<Option<TransformNode>>` | Local selector → existing parse projection; source identity travels with `TransformNode` |
| synthesized declaration | `TransformNode` in the request's `TransformArena` | Existing signature factory → location projection → `set_text_range2` → existing ordered statement/member list |
| original and raw range | Existing original side table plus node range | Existing `set_text_range2`; downstream resolver/comment/map consumers; no change to clone/update/dispose ownership |
| allocation / tracker / errors | Existing `BuildResult`, `EmitResolverError::Factory` and checker-abort conversion | Construction and projection retain their original order and `?` boundaries; no new suppression, retry or recovery |

No new cache, lifetime, callback, sink operation, lexical environment,
receiver replacement, generated binding, comment cursor or transform pass is
introduced. Neither decision changes the signature construction order or
recomputes node flags. Source/array flags and name finalization remain with
their existing factories. Comment and map differences follow only from the
source anchor; their bytes must still be compared independently.

## 5. Architecture and evidence references

Read [the architecture](../emitter-architecture.md) and the
[mandatory design gate](../post-h1-completion-slices.md#11-mandatory-implementation-ready-design-gate)
before runtime edits. This packet records a source audit at `f406f1200` and
does not relabel the architecture's historical qualification dates.

| Concern / published status | Concrete current boundary | This slice's disposition and evidence |
| --- | --- | --- |
| E-PROTOCOL / active-qualified, `0653e10d`, 2026-08-17 | Public `EmitResolver`, `OutputSink`; `CheckerSession` borrow | Premise unchanged; complete command tuple and blocked-semantic control retain the resolver/write boundary |
| E-CHECKER-FACTS-BASE / active-qualified, `0653e10d`, 2026-08-17 | `CheckerState`, binder source/parent topology; private `StatementSerializer` | Public ownership unchanged; **local selector behavior requires new qualification** by this packet, not inheritance from the old row |
| E-RESOLVER-IDENTITY-G / active-qualified, `0653e10d`, 2026-08-17 | `project_parse_node`, public arena parse-tree projection | Premise unchanged; selected source identity and same/foreign-source controls are required |
| E-METADATA-BASE / active-qualified, original `0653e10d`, A6-28-7 extension retained | Existing original links and range storage; private `set_text_range2` | Representation unchanged; keep provenance-before-range ordering; the new range producer is unqualified until native controls pass |
| E-PRINTER-BASE, E-COMMENTS-G, E-COMMENT-SCOPE-H / active-qualified, `6acd5d43`, 2026-08-21 plus recorded extensions | Public printer and private immutable comment scope | Premise unchanged; no checker dependency or comment-state edits; compare declaration bytes, maps, repeated commands and removeComments |
| E-DECL-SESSION / active-unqualified, audit 2026-09-07 at `4b4d89dc` | Ordinary `ProgramSession` / declaration request lifetime | Research input only; execute the ordinary command route directly, without claiming getter/forced-emit qualification |
| E-STRINGS / historical active-qualified, `0653e10d`; current UTF-16 work separate | Claude's scanner/literal/factory/printing paths | No premise of broad literal parity; no literal-value/source-spelling edits or UTF-16 promotion in this slice |
| E-NAMES-BASE and E-ORDER-H | Existing generated-name finalization and transform registration | Unchanged allocation/order; ES2015/ESNext and expando controls retain the actual output, with no new runtime activation |

The local gap inventory has two runtime owners (G4a/G4b), six ordered G4a
decisions, the G4b prototype/other/absent decisions, one shared classifier and
two source-projection/range seams. `preserve_comments_on`/JSDoc link synthesis
is not on this location-selection edge and retains G6. The known G5c owner
is an explicit shared-case limitation, not an unresolved design decision
delegated to the implementer.

## 6. Frozen witnesses and what they actually reach

The [executable observation notebook](h2-8a-declaration-comment-range-observations.md)
contains all input text and the observation recipe. It produces a complete
fixture without changing any old input or expectation. The unmodified TS
command is the oracle; instrumentation only records source decisions and
must produce the same complete tuple.

| Suffix under `declaration-comment-range/` | Observed selector / expected source ownership |
| --- | --- |
| module-exports | G4a, ModuleExports=2 → FunctionExpression; the assignment's JSDoc is not replayed on the generated function |
| exports-property; module-exports-property | G4a, ExportsProperty=1 → FunctionExpression; the second also retains its expando namespace |
| ordinary-property | G4a, Property=5 → BinaryExpression; declaration output retains the property-owner JSDoc |
| variable | G4a → VariableDeclarationList, not VariableStatement |
| function-declaration | G4a → original FunctionDeclaration |
| prototype-dot; prototype-element | G4b, PrototypeProperty=3 → BinaryExpression; method output retains its JSDoc |
| static-method | Observed **G4a** Property=5 in an expando namespace, not a G4b negative; retain that real route |
| this-property | Adjacent property serializer; no G4b event; retain its existing this-property output |
| ordinary-method | G4b → original MethodDeclaration |
| prototype-parenthesized | Adjacent property serializer; no G4b event; outer comment and function-valued property are preserved, with no parenthesis skipping |
| remove-comments | G4b still selects the assignment; printer suppresses comments |
| declaration-only | G4b still selects the assignment; only declaration products are emitted |
| lf-bom-listings | G4b plus LF, BOM and emitted-file reporting; retain bytes and callback metadata separately |
| esnext | G4b at the adjacent retained target |
| blocked-semantic | TS2322, exit 1, no writes and no selector calls; noEmitOnError blocks before transformation |

All 17 ordinary Programs ran twice. The 16 unblocked cases had no reported
diagnostics and exit 0. Instrumented Programs also ran twice and were exactly
equal to their ordinary tuples; traces repeated identically. Twenty-four
separate JS/TS classifier controls cover property, exports, module exports,
prototype dot/element, this, plain/compound assignment, dynamic receiver,
void-zero and parenthesized access keys. Four direct source-selector controls
freeze missing declaration, missing parent, variable without list and variable
with list. Four further range controls freeze same-source location, foreign
source, absent location and parsed-input cloning at the unchanged
`setTextRange2` seam. Direct controls are not additional full Programs or new
API admission.

The full JSON files were regenerated from the final notebook with actual exit 0:

| Artifact | SHA-256 |
| --- | --- |
| commands.json | `d5b0d846188dbe37061e852d62b15e510a26edb252bfcceab3333f0c2daf6b9f` |
| traces.json | `97c5f2d5ad5395fc157fde243fa487db1f8ea3f66a35644efde64104ca942b08` |
| authority.json | `1e01f1b151b182ed2dd2ac806a19f0414ab47ea28085344a4313eb670858bed3` |

Retained run directory:
`/var/folders/b7/j_jl1trx4hx0khkxvb84d_jc0000gn/T/tsc-rs-declaration-comments-frozen-lv9osfpn`.
The notebook is the durable reproduction source; the temporary directory's
continued existence is not a readiness premise. Native execution count: **0**.
No native repair, new global total, native map parity or performance result
is claimed by these source observations.

## 7. Implementation sequence and acceptance

1. **Reproduce the source freeze.** Extract and execute the notebook, then run
   §9's validation. Promote `commands.json` and `traces.json` byte-for-byte to
   the two new fixture paths in §2; verify their SHA-256 values. Add the
   independent compiler test target and uniquely named checker tests. No
   production selector may change during this stage.
2. **Capture the native before state.** At the pinned production base, compare
   all 17 fresh commands and each of the three original IDs, with complete
   captures, original fixtures/options and actual process exits. Obtain two
   actual executions per failed command, not two copies of its panic. The
   existing `assert_cases_with_inspection` exits its inner two-run loop on the
   first failure: run a second independent before job for failed rows, and
   record successful extra runs separately. Build the test target once and
   retain its binary hash. Do not run both before jobs concurrently.
3. **G4a edit.** Import the existing `get_assignment_declaration_kind` and
   `AssignmentDeclarationKind`; apply §3.1 verbatim in semantics. Keep absent
   signature distinct from parentless declaration and parentless variable.
   Run G4a witnesses and the relevant source-identity tests on these bytes.
4. **G4b edit.** Apply §3.2 only in the non-omitted method/function loop.
   Preserve the other property/accessor/omitType arms and signature order.
   Run the prototype and adjacent property/method witnesses.
5. **Final focused gate.** All 17 fresh complete commands must match TS twice,
   including all JS/map/declaration/map writes, not just the comment text.
   The first two original cases in §1 must match completely twice. The third
   original case remains an unconditional strict report: save both complete
   tuples, all differences and its actual exit. If G5c remains, report it as
   failed, verify the intended source-location change independently, and
   retain all unrelated fields against before; do not mark it exact or modify
   its expectation. If any of the two required originals has a second owner,
   re-slice from captured evidence before claiming this gate passed.
6. **Regressions and handoff.** Run the existing checker statement and chain
   suites plus the named original JS export/require/declaration controls.
   Preserve every before-positive tuple. Record any pre-existing unrelated
   failure with the pinned base proof; do not silently allowlist a new one.
   Retain per-cause commits, final production/test/input hashes, source→Rust→
   witness mapping, raw captures and exact command exits in the new receipts.
   Integrate runtime changes only after the existing hosted acceptance.

The standalone compiler target reuses, without modifying,
`h2_7c_declaration_blocking::assert_cases_with_inspection` with command reporting
enabled and `h2_7b_w4a_controls` for full comparison. It mounts the same pinned
libraries and preserves every option in the notebook. Include
`h2_7d_original_corpus_shared` for original-ID projections. Capture the complete
command even on failure before running the existing fail-fast comparator;
the supplemental capture shape in `h2_8a_require_rewrite.rs` is the current
reference. Use a slice-specific capture directory and distinguish these extra
capture emissions from the comparator's primary emissions.

Checker controls inspect actual serialized `original` identity and raw
`pos/end` for the parsed witness inputs, not a duplicated boolean expression.
Compare signature types/parameters and output ordering alongside locations.
Use the existing `with_declaration_statements` and `StatementSerializer::new`
test helpers. The 24 classifier observations also test the shared predicate's
JS-file behavior without modifying binder code. Direct missing-declaration /
parent controls remain separately labelled synthetic/internal tests. The
existing `set_text_range2_replaces_provenance_before_copying_the_range` is an
unchanged regression. Reproduce all four frozen range controls in the owned
checker tests using real mounted sources: same-source copies the local range;
foreign-source retains synthesized coordinates while updating provenance;
absent location keeps the existing original; parsed input is cloned. Compare
identity, original source and both endpoints with `traces.json`. Do not bypass
the projection or write raw coordinates to make them pass.

No new effect/failure edge is introduced, so this patch does not add a new
sink/host fault matrix. Existing arena/projection `Result` behavior remains
tested at its owner; blocked-semantic is the explicit no-write control here.
Source type construction precedes the edited decisions, so diagnostic
accepted-set changes are not intended. Any observed diagnostic change must
be investigated and invokes the schedule's snapshot/verify discipline before
claiming semantic parity.

Expected native commands, after the independent target/fixtures exist:

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/declaration-comment-ranges taskpolicy -b nice -n 15 cargo test -p tsc-rs-checker --lib node_builder::statements::tests -- --test-threads=1
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/declaration-comment-ranges taskpolicy -b nice -n 15 cargo test -p tsc-rs-checker --lib node_builder::chains::tests -- --test-threads=1
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/declaration-comment-ranges taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test h2_8a_declaration_comment_ranges -- --test-threads=1
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/declaration-comment-ranges taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test h2_8a_original_corpus original_export_assignment_annotations_match_complete_commands -- --exact --test-threads=1
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/declaration-comment-ranges taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test h2_8a_original_corpus original_require_alias_declarations_match_complete_commands -- --exact --test-threads=1
```

The new target's strict shared-G5c report must remain callable by an exact
test name, `original_shared_g5c_complete_command`, independently of the two
required original repairs. It is never counted as passing from a failed exit.
The current lightweight workflow is the schedule header's focused loop plus
hosted acceptance; do not restore historical walks or full developer CI.
No PR, acceptance run or runtime modification is part of this design delivery.

## 8. Readiness and remaining execution work

The source design has no unresolved choice about either selector, the binder
predicate, location identity, missing-parent behavior, edit files or test
route. The native before observation and test adapters are **not yet executed
or implemented**. They are mandatory steps 1–2, with explicit failure criteria,
before the design may authorize production edits under schedule §1.1.

| Gate | Status / exact completion condition |
| --- | --- |
| Source/Rust decision map and file boundary | Fixed in §§2–4; no extra production owner |
| Source freeze | 17 commands ×2; instrumented equality ×2; 24 predicate, 4 sentinel and 4 range controls ×2; hashes in §6 |
| Baseline and architecture pins | Check §9 at the fixed base; fresh source audit does not imply a native qualification |
| Frozen fixture materialization and test target | Pending step 1; copied fixture hashes must equal §6 |
| Native pre-edit comparisons | Pending step 2; complete captures and actual repeated failures at the pinned production source |
| Runtime implementation and qualification | Pending steps 3–6; no production file changed in this delivery |

This distinction is intentional: source semantics can be fixed before paying
for a native build, while the native evidence gate still prevents speculative
implementation. There is no user approval dependency for the specified work.
The implementer must resolve new evidence by the amendment rule rather than
assuming unknown failures belong to G5c or to Claude.

## 9. Source-design validation

After running the notebook, export its output directory as `DECL_COMMENT_OUT`
and execute the Python block below from this repository root. It checks base
identity, all pinned files, the exact source spans, the actual frozen outputs,
case/trace coverage and the absence of production edits. It validates the
**source design only** and explicitly reports that native readiness is pending.

```python
import hashlib, json, os, subprocess
from pathlib import Path

base = 'f406f12009cd476e28f9e0dcfb3ae4030563fa67'
subprocess.run(['git', 'merge-base', '--is-ancestor', base, 'HEAD'], check=True)
out = Path(os.environ['DECL_COMMENT_OUT'])
expected = {
    'commands.json': 'd5b0d846188dbe37061e852d62b15e510a26edb252bfcceab3333f0c2daf6b9f',
    'traces.json': '97c5f2d5ad5395fc157fde243fa487db1f8ea3f66a35644efde64104ca942b08',
    'authority.json': '1e01f1b151b182ed2dd2ac806a19f0414ab47ea28085344a4313eb670858bed3',
}
sha = lambda b: hashlib.sha256(b).hexdigest()
for name, digest in expected.items():
    assert sha((out / name).read_bytes()) == digest, name
authority = json.loads((out / 'authority.json').read_text())
for entry in authority['files']:
    assert sha(Path(entry['path']).read_bytes()) == entry['sha256'], entry['path']
lines = Path('vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
for entry in authority['spans']:
    assert sha(b''.join(lines[entry['start']-1:entry['end']])) == entry['sha256'], entry['name']
commands = json.loads((out / 'commands.json').read_text())
traces = json.loads((out / 'traces.json').read_text())
assert commands['repetitions'] == traces['repetitions'] == 2
assert len(commands['cases']) == len(traces['cases']) == 17
ids = [c['case_id'] for c in commands['cases']]
assert len(set(ids)) == 17
assert ids == [c['case_id'] for c in traces['cases']]
assert len(traces['predicates']) == 24 and len(traces['sentinels']) == len(traces['ranges']) == 4
assert {t['owner'] for c in traces['cases'] for t in c['trace']} == {'G4a', 'G4b'}
assert subprocess.check_output(['git', 'diff', base, '--', 'crates', 'scripts', 'ratchets']) == b''
print('Source design validated; native pre-edit gate PENDING; production unchanged.')
```

## 10. Pinned upstream spans

Hashes cover whole lines including the final newline. These are the changed
owners, the existing classifier path and the unchanged provenance/range seam;
the full compiler hashes also pin primitive SyntaxKind/factory utilities.
No per-span hash is obtained from a historical document without checking the
current source. The notebook regenerates this table's identities from the
vendored AST and verifies every requested function is present exactly once.

| Function | `_tsc.js` lines | SHA-256 |
| --- | --- | --- |
| `getOriginalNode` | 11400–11410 | `e6e639e966314faf444b9b68796893745ffb06eb0adcf1180d6935332d8797a3` |
| `getSourceFileOfNode` | 12867–12872 | `58e6f8a806053a8ef08f49d786151da6e7ea6e103c40de9a930afaa37519a8a6` |
| `isInJSFile` | 14886–14888 | `c5f0db66356c51537ce1e7c91692c0775f3db5764d39100f22ad85b89e0ec9a1` |
| `getRightMostAssignedExpression` | 15036–15045 | `45c4435222711651c5ac4c6b5349f0b2c4f13ee387715fd94d30719b7b1f0b37` |
| `isExportsIdentifier` | 15046–15048 | `1895084a734024e56d73452ed872d5c03a87471f0aa59d8c25dcd2c9e9b71560` |
| `isModuleIdentifier` | 15049–15051 | `9cc485146388304721bcedcfc69736735c384b0bf051ea84f321c5b56fff49cd` |
| `isModuleExportsAccessExpression` | 15052–15054 | `a868c0d25d139d6dd4e6ea935a17e4fe03908764961fbb0ecfe786317f6b2f71` |
| `getAssignmentDeclarationKind` | 15055–15058 | `86ed418c050973f93d14271122ba9f948961e1e038e6c338eb6df19543402bd2` |
| `isLiteralLikeElementAccess` | 15069–15071 | `cb97911b45367e4eddd8dea54ccfc3f03633fd001a17ab20787d826e6f8b77a7` |
| `isBindableStaticAccessExpression` | 15072–15078 | `0acbfc71d1713d467f90502d134ef80bebc2c1d332555c6d62fce3d2b115f43b` |
| `isBindableStaticElementAccessExpression` | 15079–15085 | `2756da3de8512199488f0a6e83d033fbb05385a5457e7d197a0c2e6df7409159` |
| `isBindableStaticNameExpression` | 15086–15088 | `53acbb025bc0548bdb246e8d1665df9cada206e07d887c1612ef1486dfe2b6c0` |
| `getAssignmentDeclarationKindWorker` | 15095–15120 | `748a8d0ff34b41c4230a22f31b31e87e5752191e17183fafd67e39f4e2773d51` |
| `isVoidZero` | 15121–15123 | `1b207e5df1d0a22cc59ec52cb09c1fd3a3992aac066e44c1958b07c41ea923e6` |
| `getElementOrPropertyAccessArgumentExpressionOrName` | 15124–15133 | `059bdc96ee87ab80992b12ac6fcb6f2047f0e15ea995ea349110711e9efa84b2` |
| `getElementOrPropertyAccessName` | 15134–15145 | `964cec0cedef54bfe2ad2b52ba716f594d192800367162a153a69bc54c2161d7` |
| `getAssignmentDeclarationPropertyAccessKind` | 15146–15177 | `f64dadd48b2ad9f13cc1a2b1dc4ff7ebaa6d6a59b2f3453d3db544767e9e5a81` |
| `getInitializerOfBinaryExpression` | 15178–15183 | `ccf8aa2e13439a470de0fc54c775b3428e54c2a12b51ad3e6a1efb7e58b11f32` |
| `isPrototypePropertyAssignment` | 15184–15186 | `1c180874c282d2b400931fdebe6f1e25fe6534f99fc5767e1ba41984fc2c5e17` |
| `isDynamicName` | 15854–15860 | `8f6cc6444b97a25155fab32522270e0343d00cc5850bad4b862fdeae66bd5459` |
| `isPrototypeAccess` | 17171–17173 | `9c0c61850f471f2a7d134843986afa7cdd74134c790a68ac80414a15f2ac61e7` |
| `setTextRange` | 28256–28258 | `b4484231223d27d5a3bd94103656b0f7ee336d397d3f1af742f67f893bd1cc08` |
| `setTextRange2` | 51102–51121 | `07b8fa38d2f39bb231e53741f7b348d56af10b2d2a73fe778b415eb2622c5c00` |
| `serializeAsFunctionNamespaceMerge` | 54458–54476 | `31d4121617baea5146ed7516eaccff1fd2139d7f938bf5008f5b3516dfccd97e` |
| `getSignatureTextRangeLocation` | 54483–54493 | `73ccdbcf6a52d23d85ad0136f48e14849f39d1d31ed25842fb665495d0f1f4dc` |
| `makeSerializePropertySymbol` | 55099–55229 | `fab03df95bee26e871b1bc817932eb99b58a9c262f37009266cc1a5291f817a7` |

Whole-file baseline pins used by §9 (the machine-readable copy is `authority.json`):

| Path | SHA-256 |
| --- | --- |
| `vendor/typescript-6.0.3/lib/_tsc.js` | `1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3` |
| `vendor/typescript-6.0.3/lib/typescript.js` | `569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39` |
| `scripts/observe-jsdoc-block-scope-container.mjs` | `cee190bee2cdca35a5fc9c6702e98cd5a089a53dd43c542723b588ce8fc2201f` |
| `crates/oracle/vfs-directory-overlay.mjs` | `2868391e75941127f8eb3a352232190fd98c6d794235913e288432e5626aab76` |
| `crates/checker/src/node_builder/statements.rs` | `dac1ba74abd1f67303272bf428f3da9b43ac9ca663eb52b42337d6ba39965f28` |
| `crates/checker/src/node_builder/chains.rs` | `3dc45639617727687c8ce6c12afe913c8986f7d54528e6a0a9e17037d0a64d5d` |
| `crates/binder/src/assignment.rs` | `3589abfd9c9de29468e06d3582419e34c01f1ec58c0b88b62ebd350918a3afcc` |
| `crates/compiler/tests/integration/h2_7c_declaration_blocking.rs` | `4fe4dd0719993cd60564a53aab780d63200fc9ed390570889d48e42afbe0391b` |
| `crates/compiler/tests/integration/h2_7b_w4a_controls.rs` | `c9332037a63761ae27dfe88c90d865197dbc01b2aa7f7fe6fa11999b9392cd6b` |
| `crates/compiler/tests/integration/h2_7d_original_corpus_shared.rs` | `90aff72e27fac502da3ff3f06980c55c4923e35554c3e64800a60213cb1b04ba` |
| `docs/design/greenfield/emitter-architecture.md` | `87866477f1e3ee90371f0600264540a9688a98b51517e0689926d37ca0f6bfb2` |
| `docs/design/greenfield/post-h1-completion-slices.md` | `df4e630f4fe348119e43714261d1193a880453fa6847b42c7bf52d086bd840fb` |
| `ratchets/h2-8a-global-after-a6-37.v1.json` | `528440d00cf5b3c57c7bf815bb9e236fcf1cffc7e19c2756b48cc8c5cd4fee5e` |
| `ratchets/h2-8a-convergence-causes.v1.json` | `9d5b2a26bb605fd9907b2bff57982ea0c0e6d256b66ccd7e812a2d5c5b82796c` |
