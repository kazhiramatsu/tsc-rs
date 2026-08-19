# H2.5h-a / CS-6 — witness-driven fixture gate, permanent audit, requalification (DRAFT)

Design-gate packet for the fifth and final comment-scope production
packet, under the mandatory implementation-ready design gate
(../post-h1-completion-slices.md). **DRAFT front-run status:** authored
in the `h2/5h-a-cs6` worktree on the CS-5 front-run head; trusted
base, authority hashes, and §12 re-pin at the CS-6 train's design-gate
pass. Machine check (once the envelope exists):
`node .github/ci/slice-readiness.mjs --check h2-5h-a-cs-6`.

## 1. Identity, purpose, and boundary

- **Slice ID:** `h2-5h-a-cs-6`. **Kind:** `closure` — the
  E-COMMENT-SCOPE-H sub-packet's fixture/audit gate. Three outcomes:
  1. **The witness-driven full-pipeline fixture suite**: all 30 frozen
     comment-scope cases (10 families × positive /
     remove-comments-control / adjacent-negative-control) drive the
     port end to end — decode the stored input bytes, parse, apply the
     case's transform as a BEFORE-transformer (mirroring the oracle's
     `program.emit(..., { before: [transform] })`), run the ES2016
     script-transformer pipeline, print with the case options — and
     the output must BYTE-EQUAL the stored oracle bytes
     (`observation.writes[0].callback_utf8_base64`, full text, not
     hash-only). No expected value is authored anywhere: the frozen
     artifact is the entire expectation.
  2. **The permanent zero-contextless audit**: a workspace-audit rule
     (in `crates/xtask/src/workspace_maintenance.rs` — measured
     policy-pin-free, unlike main.rs) denying the retired identifiers
     in `crates/emitter/src`: `detached_transitional` anywhere, and
     definitions of the five deleted shim names (word-boundary,
     not-followed-by `_`, so the `_with_context` family stays legal).
     CS-5's grep-level acceptance gains its named permanent successor.
  3. **Requalification**: `E-COMMENT-SCOPE-H`, `E-PRINTER-BASE`,
     `E-PRINTER-G`, `E-COMMENTS-G` return to `active-qualified` at the
     CS-6 final validation ref; gap-matrix `comment-scope-threading`
     flips `partial` → `exists`. **This closes E-COMMENT-SCOPE-H and
     unblocks the H2.5h-b ES2015/Generators production packets (B-1+),**
     per the standing prohibition "no ES2015/Generators production
     work may precede it".
- **Non-goals:** any H2.5h-b production work (unblocked, not started);
  witness-set extension (a fixture failure is fixed in PRODUCTION
  under the frozen bytes' authority, never by amending the witness —
  the inverse of the CS-3 §7 rule).
- **Prerequisites:** CS-5 merged with its envelope `ready`; gate-tax 2
  merged (the walk re-mints ride adoption).
- **Trusted base:** re-pin at train start (DRAFT: authored at
  `7bbbd79f` on the CS-5 branch).
- **Activation state:** before — the witness artifact is frozen
  evidence consumed only by marker/topology assertions; the four rows
  are `active-unqualified`; zero-contextless holds by grep only.
  After — every frozen byte is reproduced by the port under test on
  every gate run; the audit is a permanent workspace rule; the four
  rows are `active-qualified` at the recorded ref.
- **Next owner:** H2.5h-b (B-1+ implementation packets, single joint
  runtime slice per the step-4 SCC decision).

## 2. Required-reference table

| Row | Lifecycle before → after | Role here |
|---|---|---|
| `E-COMMENT-SCOPE-H` | `active-unqualified` → **`active-qualified` @ CS-6 final ref** | the closing row; symbol list gains the fixture suite and drops nothing |
| `E-PRINTER-BASE` / `E-PRINTER-G` | `active-unqualified` → **`active-qualified` @ CS-6 final ref** | reshaped by CS-2..5; the fixture suite plus their retained focused contracts are the requalification evidence |
| `E-COMMENTS-G` | `active-unqualified` (modified-requalify) → **`active-qualified` @ CS-6 final ref** | re-expressed projections revalidated by the suite's list/topology families |
| gap-matrix `comment-scope-threading` | `partial` → **`exists`** | producer anchors complete; capabilities counts move 3/7/3 → 4/6/3 |
| dispositions manifest | counts unchanged | note records the requalification ref (the classification is historical) |

## 3. Pinned upstream map

The upstream IS the frozen witness artifact
(`ratchets/h2-5h-a-comment-scope-witnesses.v1.json`): its
`typescript` record pins the oracle, its `generator` sha pins the
frozen TRANSFORMS table this packet mirrors, and every case row
carries the complete input (`input.files[].utf8_base64`, serialized
compiler options `{module: 99 ESNext, newLine: 1 LF, removeComments,
target: 3 ES2016}`) and the complete output
(`writes[0].callback_utf8_base64` + sha + byte count). Measured: the
oracle applied each case transform as a BEFORE-transformer
(generator :1040-1045), so downlevel/erasure ran AFTER the transform —
the Rust drive must preserve that order.

## 4. Rust semantic map

| Item | Target |
|---|---|
| fixture harness | new `crates/emitter/tests/integration/comment_scope_witness_contract.rs`: `include_bytes!` the artifact (h2_1a_profile precedent), serde_json parse, one `#[test]` iterating all 30 cases with per-case panic messages carrying `case_id` and a byte-diff excerpt |
| per-case drive | the measured house precedent is `transform_and_print_at_target_with_resolver_and_mode` (active_transform_contract.rs:200): parse → `TransformArena` → `transform_nodes` with the case builder PREPENDED to `get_script_transformers(&options, &NoConstantValueResolver)` → print under `SourceFileTextMode::Canonical` (the oracle-byte-parity mode those contracts qualified) with `with_remove_comments(case)` → byte-compare against the decoded stored output |
| compiler options | **built from the stored serialized record, never from `bootstrap_options()` defaults** — the frozen record is exactly the four keys `{module: 99 ESNext, newLine: 1 LF, removeComments, target: 3 ES2016}` (measured across all 30 cases); an unknown stored key fails closed. Review-caught: the precedent's bootstrap defaults (e.g. `always_strict`) would alter prologue emission and break byte parity |
| structural guards | measured invariants asserted per case before comparing: exactly one input file and one root, exactly one write, `emit_skipped == false`, stored reported/emit diagnostics empty (30/30 today) |
| transform builder: `identity` | pass-through |
| `wrap-expression-statements-in-synthetic-arrow` | the one `x;` expression statement wraps as `factory` arrow `() => { x; }` in a fresh expression statement (createArrowFunction/createBlock/createToken equivalents on `NodeFactory`) |
| `apply-comment-emit-flags` | `suppressLead()`/`suppressTrail()` call statements gain `EmitFlags::NO_LEADING_COMMENTS`/`NO_TRAILING_COMMENTS`; the one `Block` gains `EmitFlags::NO_NESTED_COMMENTS` (exists, metadata.rs:32) |
| `add-synthetic-comments-to-marked` | the `markMe()` statement gains multi-line `SyntheticComment`s " SYN-LEAD " (leading) and " SYN-TRAIL " (trailing), no trailing newline flags |
| `append-synthetic-statement-with-synthetic-comments` | append a synthesized `syntheticMarker()` expression statement carrying the same two synthetic comments |
| `replace-not-emitted-and-zero-width` | `dropMe()` → `NotEmittedStatement(original)`; `shrinkMe()` → fresh `shrunkMarker()` statement with its text range set to `(pos, pos)` of the original (zero width) |
| audit rule | `workspace_maintenance.rs` gains a forbidden-identifier table for `crates/emitter/src`: token `detached_transitional`; regex `fn emit_(required_node|node_id|identifier_name|required_identifier_name|child_after_token)\(` — with its own unit test (positive canary on a fixture string, not a repo file) |
| requalification | `emitter-architecture.md` four-row status flip @ final ref; handoff amendment; gap-matrix generator state/count change |

All builder ingredients exist today (factory node construction,
`EmitFlags` incl. `NO_NESTED_COMMENTS`, `SyntheticComment`, `NotEmittedStatement`,
zero-width text ranges, `with_remove_comments` — measured).

## 5. Current local-gap matrix

| Surface | Today | State | Owner |
|---|---|---|---|
| full-pipeline byte reproduction of the 30 frozen cases | marker/topology assertions only | `missing` | this packet, step 1 |
| permanent zero-contextless enforcement | CS-5 grep acceptance | `missing` | this packet, step 2 |
| four-row qualification | `active-unqualified` | `partial-or-stale` | this packet, step 3 |
| unknown production gaps the suite may surface | none known; the scope machinery is fully landed (CS-2..5) | `unknown-bounded` — every failure is bounded by a frozen case with full byte evidence | this packet, step 1 protocol (§6) |

## 6. Implementation sequence

Fence: the new fixture test file, `workspace_maintenance.rs` (+ its
unit tests), `crates/emitter` production fixes ONLY where a fixture
case is red (each fix cites its `case_id`), and the §8 evidence set.

1. **Fixture suite.** Land the harness with the six builders; iterate
   to 30/30. Protocol for a red case: the frozen bytes are the
   authority — fix production, cite the `case_id` in the fix commit,
   never touch the witness. If a red reveals a semantics the landed
   machinery cannot express (not expected after CS-2..5), STOP per the
   design-gate amend rule.
2. **Audit rule** + unit test (canary proves the rule fires; the live
   tree proves it passes).
3. **Requalification amendments** (§8), chain walk (adoption), new
   envelope `h2-5h-a-cs-6` (`ready`, predecessor `h2-5h-a-cs-5`),
   bootstrap + index.
4. Static `--lane rust` BEFORE the walk; then the packet checker
   (now cs-2..cs-6) and the full local gate at the final candidate;
   the merge commit's gate summary IS the requalification ref record.

## 7. Frozen witnesses and the output-change rule

ALL TEN families are consumed — the suite drives every case. The
output-change rule reaches its terminal form: there are no authored
expected values to change; production moves to the frozen bytes,
never the reverse. The witness artifact may change ONLY through
pin-line re-mints (adoption keeps observations byte-identical); any
observation drift aborts the packet.

## 8. Evidence, ratchet, and documentation amendments

1. **Gap matrix generator**: `comment-scope-threading` state
   `partial` → `exists` (counts 3/7/3 → 4/6/3), anchors gain the
   fixture-suite path, note records the sub-packet closure.
2. **Dispositions**: note records the requalification ref; counts
   unchanged.
3. **Architecture map**: the four rows flip `active-qualified` with
   the CS-6 final validation ref; `E-COMMENT-SCOPE-H` text replaces
   the packet-ladder narrative with the closed state.
4. **Handoff** `h2-5h-a.md`: the E-COMMENT-SCOPE-H mandatory
   sub-packet is recorded CLOSED and H2.5h-b (B-1+) recorded
   unblocked → envelope re-pin + doc-pinning witness re-mints
   (adoption: seconds).
5. **Chain walk**: CS-2 §8.5 verbatim with the extended const-sync
   table; harness-pin full-sweep audit before the gate (the
   playbook's multi-attempt-walk rule).
6. **Readiness**: envelope `h2-5h-a-cs-6`, bootstrap
   `allowedPacketIds += h2-5h-a-cs-6`, index row.

## 9. Acceptance

- Fixture suite 30/30 green — every case byte-equal to the frozen
  oracle output, both `removeComments` polarities, all six
  transforms exercised.
- Audit rule: canary unit test red-proves the rule; the live tree
  passes it; CS-5's acceptance greps remain zero.
- `cargo test -p tsc-rs-emitter` fully green; any changed focused
  expected value carries its `case_id` citation (production fixes
  only).
- Packet checker: the eight `h2-5h-a.md` commands + slice-readiness
  cs-2..cs-6, individually.
- Complete local gate green at the final head; conformance
  49024/49024 all bands FP=0; no ratchet regression.
- The four architecture rows read `active-qualified` at exactly the
  final candidate ref recorded in the PR gate summary.
- Complete when merged; **H2.5h-b B-1 opens against the frozen
  graph/matrix/witnesses and this closure.**

## 10. Traceability

| Invariant | Target | Test | Evidence |
|---|---|---|---|
| oracle-captured bytes reproduced end to end | full pipeline | the 30-case suite | witness artifact (sole authority) |
| before-transformer order | `transform_nodes` prepend | suite (order-sensitive cases: wrapper, synthetic append) | generator :1040-1045 |
| zero-contextless permanence | workspace-audit rule | rule canary + live pass | CS-5 deletion + this rule |
| requalification honesty | architecture map @ ref | packet checker + gate summary | merged head |

## 11. Prohibitions

No witness amendment for a red fixture; no H2.5h-b production work;
no new `EmitContext` constructors; no audit-rule scope beyond
`crates/emitter/src` (widening the deny-list to other crates is a
separate reviewed decision); the CS-3/4/5 prohibitions remain.

## 12. Unresolved items (DRAFT — close before the envelope flips ready)

1. Trusted base + authority hashes: re-pin after CS-5 merges.
2. The exact Rust spelling of the six builders against the factory
   API (mechanical; the ingredients are measured present).
3. The virtual `/project/input.ts` path — expected byte-irrelevant
   (source maps are off and the precedent drive parses under a plain
   name with an arena `SourceFileId`), confirm on the first fixture
   run.
4. The audit rule's exact table shape inside workspace_maintenance.rs
   (follow the inline-tests rule's structure).

## 13. Readiness summary (draft)

Upstream: the frozen witness artifact end to end (30 cases, full
input/output bytes — measured). Rust-map rows: 10 (§4), all
ingredients present. Gap rows: 4 (§5). Witnesses: 10/10 consumed —
the terminal form. Architecture impact: the four-row requalification
and the sub-packet closure that unblocks H2.5h-b. Undispositioned 0.
Unresolved: §12 (4 items, mechanical).
