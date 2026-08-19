# H2.5h-a / CS-5 — contextless and dual API deletion (DRAFT)

Design-gate packet for the fourth comment-scope production packet,
under the mandatory implementation-ready design gate
(../post-h1-completion-slices.md). **DRAFT front-run status:** authored
in the `h2/5h-a-cs5` worktree on the CS-4 front-run head; trusted
base, authority hashes, and §12 re-pin at the CS-5 train's design-gate
pass. Machine check (once the envelope exists):
`node .github/ci/slice-readiness.mjs --check h2-5h-a-cs-5`.

## 1. Identity, purpose, and boundary

- **Slice ID:** `h2-5h-a-cs-5`. **Kind:** `foundation` — a purely
  subtractive packet: the transitional contextless emission surface is
  deleted now that CS-4 threaded every route. Zero behavior change of
  any kind; the emitter suite and the corpus ratchet must not move a
  byte.
- **Purpose:** delete the five caller-less contextless shims
  (`emit_required_node`, `emit_node_id`, `emit_identifier_name`,
  `emit_required_identifier_name`, `emit_child_after_token`), their
  five CS-4 `#[allow(dead_code)]` annotations, and the
  `EmitContext::detached_transitional()` constructor, leaving
  `EmitContext::file_root()` as the printer's SINGLE zero-scope entry
  — the promise the CS-2 architecture row made
  ("single root-only zero-scope entry") becomes structurally true.
- **Non-goals (owned later):** the artifact-driven full-pipeline
  fixture suite and the permanent zero-contextless audit plus the
  E-COMMENT-SCOPE-H / E-PRINTER-BASE / E-PRINTER-G / E-COMMENTS-G
  requalification at the final validation ref (CS-6); any
  ES2015/Generators production work.
- **Prerequisites:** CS-4 merged with its envelope `ready`.
- **Trusted base:** re-pin at train start (DRAFT: authored at
  `7f120422` on the CS-4 branch).
- **Activation state:** before — five dual pairs
  (plain + `_with_context`) exist with the plain halves caller-less
  and annotated; `detached_transitional` has zero production
  constructions outside those shims. After — the pairs collapse to the
  threaded forms only, `detached_transitional` does not exist, and the
  `EmitContext` documentation names `file_root` as the sole zero-scope
  constructor. Architecture rows unchanged (all requalify at CS-6).
- **Next owner:** CS-6.

## 2. Required-reference table

| Row | Lifecycle before → after | Current Rust symbols | Role here |
|---|---|---|---|
| `E-COMMENT-SCOPE-H` | `active-unqualified` (unchanged; CS-5 lands the route fifth) | `CommentEmissionScope`, `EmitContext` | the row under implementation; its "four named detached_transitional entries" sentence retires with the code |
| `E-PRINTER-BASE` / `E-PRINTER-G` / `E-COMMENTS-G` | `active-unqualified` (unchanged) | pipeline and cursor machinery | untouched premises; deletion cannot alter phase order |
| gap-matrix `comment-scope-threading` | `partial` (note update only, §8) | per-side producer anchors (no shim references — measured) | the matrix note advances CS-5 → CS-6-remaining |

## 3. Pinned upstream map

No new TypeScript spans: deletion ports nothing. The governing
authority is the already-pinned printer-scope-state span
(`_tsc.js:116957-116959`) — tsc has exactly one ambient scope whose
zero state exists once per printer; after this packet the port's
constructor census matches that shape structurally
(`file_root` = createPrinter's `-1/-1/-1`, and every other context is
derived by threading).

## 4. Rust semantic map (the exact deletion inventory, measured at 7f120422)

| Item | Action |
|---|---|
| `fn emit_required_node` + its CS-4 dead-code annotation | delete |
| `fn emit_node_id` + annotation | delete |
| `fn emit_identifier_name` + annotation | delete |
| `fn emit_required_identifier_name` + annotation | delete |
| `fn emit_child_after_token` + annotation | delete |
| `const fn detached_transitional` + its doc comment | delete |
| `EmitContext` struct doc line "…and the named transitional entries below" (printer.rs:546) | reword to name `file_root` as the sole zero-scope constructor |
| `comment_cursor.rs` scope-struct and `empty()` doc lines naming "named transitional (detached) entries" (:74, :89) | reword — the entries no longer exist (review-caught: the fence must include these two doc lines) |
| `#[cfg(test)] contract_scope` on `CommentEmissionScope` | KEEP — unit-contract hook, not a production constructor |
| `_with_context` names | **KEEP** (decided): the packet stays purely subtractive; renaming ~200 call sites adds review surface with zero semantic value, and the compound variants (`_with_context_and_source_extent`, `_and_source_comments`) keep the family lexically coherent. Cosmetic consolidation, if ever, is post-H2.9 refactor territory. |

Measured guarantees: the five shims and the constructor are
printer-private with zero references outside `printer.rs`; the dual
census is exactly these five pairs (no sixth pair exists); the ten
`detached_transitional` occurrences are the definition, the four shim
constructions, and five annotation comments.

## 5. Current local-gap matrix

| Surface | Today | State | Owner |
|---|---|---|---|
| contextless shims | caller-less, annotated | `obsolete` | this packet |
| `detached_transitional` constructor | shim-only | `obsolete` | this packet |
| permanent zero-contextless enforcement | grep-level acceptance only | `missing` | CS-6 (the audit gate) |
| full-pipeline fixture suite | witness artifact frozen, not yet driven end-to-end | `missing` | CS-6 |

## 6. Implementation sequence

Fence: `crates/emitter/src/printer.rs` and the two stale
`comment_cursor.rs` doc lines (§4), plus the §8 evidence set.

1. Delete the five shims with their annotations; delete
   `detached_transitional`; reword the `EmitContext` struct doc.
   Check: `cargo check -p tsc-rs-emitter` (the compiler proves
   caller-lessness), then the full emitter suite — zero
   expected-string changes.
2. Acceptance greps (§9) plus fmt/clippy.
3. Chain walk (adoption — seconds), §8 amendments,
   envelope/bootstrap/index, packet checker, full local gate with the
   static `--lane rust` prefix run BEFORE the walk.

Error behavior: nothing to add — deletions cannot introduce variants.

## 7. Frozen witnesses and the output-change rule

No family is consumed: a subtractive packet claims no new semantics.
All ten families are adjacent controls and must stay byte-stable; the
corpus ratchet enforces global byte-identity. The CS-3 §7
output-change rule applies vacuously (any focused-output change
aborts the packet — there is no witness that could license one).

## 8. Evidence, ratchet, and documentation amendments

1. **Gap matrix**: note text records CS-5 landed, CS-6 remaining.
   Review-measured: the generator anchors never referenced the shim
   names or `detached_transitional` (zero matches across the three
   5h-a generators), so NO anchor change is needed — the artifact
   re-mints on the walk from the handoff-pin cascade alone.
2. **Dispositions**: no row moves; manifest note records the landing.
3. **Architecture map**: `E-COMMENT-SCOPE-H` symbol list drops "the
   four named `EmitContext::detached_transitional` entries" clause;
   status text records CS-5.
4. **Handoff** `h2-5h-a.md`: one amendment sentence → envelope re-pin
   + doc-pinning witness re-mints (adoption: seconds).
5. **Chain walk**: CS-2 §8.5 verbatim with the extended const-sync
   table; printer.rs byte-final first.
6. **Readiness**: new envelope `h2-5h-a-cs-5` (`ready`, predecessor
   `h2-5h-a-cs-4`), bootstrap `allowedPacketIds += h2-5h-a-cs-5`,
   index row.

## 9. Acceptance

- `grep -rn "detached_transitional" crates` returns nothing.
- `grep -rnE "fn emit_(required_node|node_id|identifier_name|required_identifier_name|child_after_token)\(" crates` returns nothing (the `_with_context` family and compound variants remain).
- `cargo test -p tsc-rs-emitter` fully green with zero expected-string
  changes; fmt + clippy `-D warnings` green.
- Packet checker: the eight `h2-5h-a.md` commands + slice-readiness
  for cs-2..cs-5, individually.
- Complete local gate green at the final head; conformance
  49024/49024 all bands FP=0; no ratchet regression.
- Complete when green at one head, PR carries the gate summary, merge
  commit lands; CS-6 opens against this document.

## 10. Traceability

| Invariant | Target | Test | Evidence |
|---|---|---|---|
| single zero-scope constructor | `EmitContext::file_root` | compiler (no other constructor exists) + CS-6 audit | printer-scope-state span |
| no contextless emission entry | deleted shims | acceptance greps | gap-matrix anchors |
| zero behavior change | whole printer | emitter suite + corpus ratchet | adjacent-control witness families |

## 11. Prohibitions

As CS-4 §11, minus what no longer exists; additionally — no renames
(§4's decided KEEP), no new `EmitContext` constructors, no
`declaration_list_container_end` producer beyond the CS-4 one, and no
CS-6 scope (the permanent audit and fixture suite are not started
here even opportunistically).

## 12. Unresolved items (DRAFT — close before the envelope flips ready)

1. Trusted base + authority hashes: re-pin after CS-4 merges.
2. Confirm at the re-pinned base that no NEW caller of the five shims
   appeared between the front-run head and the train (one grep).
3. CS-6 handshake: agree the audit's enforcement vehicle (a unit test
   pinning the constructor census vs a workspace-audit rule) so CS-5's
   grep-level acceptance has a named permanent successor.

## 13. Readiness summary (draft)

Deletion inventory: 5 shims + 1 constructor + 1 doc line (measured,
§4). Gap rows: 4 (§5) — 2 owned here, 2 CS-6. Witness rows: 0
consumed + 10 adjacent controls. Architecture impact: symbol-list
trim on `E-COMMENT-SCOPE-H` only. Undispositioned 0. Unresolved: §12
(3 items, all mechanical at train start).
