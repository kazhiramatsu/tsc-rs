# h2-7a-m-4 — Phase-E register (E2 values recorded 2026-09-03; E3/E4 rows filled when minted)

## Owner-inventory succession (E2 generator edit: the dormant m-4 ANCHOR table + `summary.m4_anchors`)
- generator sha256: 836ed16d… → c2bbc20f… (`crates/oracle/h2-7a-owner-inventory.mjs`; `M4_ANCHORS` keyed `<name>@<start line>` because the owner nests two `cleanup` functions :115238/:115687)
- contract sha256: 0c009f2a… → ab509215… (`summary.m4_anchors { rows: const 68, anchored 0..68, header_verified 0..68 }` added; required)
- inventory whole-file fingerprint: 25cc3683… → 94f1e111… — rows changed 0 / added 0 / removed 0; `summary.audit` 308/0/0 unchanged; `summary.m4_anchors` = { rows 68, anchored 0, header_verified 0 } (dormant until P5)
- `--check` exit 0 twice after `--write`; walk-preflight + pin-audit clean; the m-3.5 controls and the m-2 partition-projection tests re-run green (E2 targeted check)

## L1/L2 domain table (E2, `target/session-notes/m4/m4-e2-census.py` over the frozen artifacts)
| class | cases | owner |
| --- | --- | --- |
| eligible | 116 | m-4 |
| `outFile` — `h2-7a/F6/references-first`, `h2-7a/S2/entityname-1`, `h2-7a/S3/typeofexpr-1` | 3 | H2.7d |
| `isolatedDeclarations` — `h2-7a/S2/latebound-1` | 1 | H2.7c |
| `declarationMap` / `stripInternal` | 0 | H2.7e / H2.7c |

Frozen gating denominators over the eligible domain: transform windows (`probe.transformSeed`) 202 = 199 unblocked windows with exactly one expected `declaration` write + 3 blocked windows; expected declaration writes 199; eligible emit diagnostics 3 (the corpus-wide 13 include `S2/latebound-1`'s 10); `declarations.transformTopLevelDeclaration.changed` 742; `declarations.visitDeclarationSubtree.changed` 496; `declarations.declBlocked` 202; `tracker.trackSymbol` 533; `tracker.reportInferenceFallback` 362; `tracker.reportInaccessibleUniqueSymbolError` 1; `tracker.reportLikelyUnsafeImportRequiredError` 1; depth-1 resolver root entries 3,092 (incl. 53 `resolver.hasGlobalName.entry` — all OUTSIDE every window, see below).

Window definition (measured): a window is the event span from a `probe.transformSeed` to the following `declarations.declBlocked`, inclusive; the printer-time `resolver.hasGlobalName.*` events fall outside every window (36 before the first seed, 10 after the last declarations event, 9 between windows) — E2 asserts zero `hasGlobalName` events inside any window (the L1 full-consumption rule is scoped to the window span).

`emit_skipped` is a CASE-level flag: `S2/expando-1` and `S2/latebound-3` carry one declaration write each while `emit_skipped == true` (the other file of the case is blocked); `F6/references-second` and `S2/latebound-1` carry none.

## Factory-face census (E2; `e2-census.txt`)
67 distinct `factory2.*` faces called by the module; 37 present in `crates/emitter/src/factory.rs` under the snake-case name; 30 absent by name: the 28 `update*` faces listed in the packet §3.7, `getGeneratedNameForNode`, and `createBundle` (refused bundle arm — not needed). The Rust factory carries 22 `update_*` and 97 `create_*` faces. Rule (packet §3.7): each absent `update*` face lands additively in P2 as the upstream pattern `identity when every child is identical, else update(create_x(children), original)` over the existing typed create face; `get_generated_name_for_node` lands additively; nothing routes through the generic `update_node`.

## Reached-helper closure (E2; `e2-reached-helpers.txt`)
116 reached rows in the declarations module; 40 have a Rust fn of the same snake-case name (non-test), 76 do not by name — most are `isX` predicates that Rust expresses as `NodeData`/kind matches; the packet §3.5 names the declaration-only helpers ported here (`canProduceDiagnostics`, `createGetSymbolAccessibilityDiagnosticForNode(Name)`, `isLateVisibilityPaintedStatement`, `createEmptyExports`, `getResolutionModeOverride`) and the later-owned ones; P1/P2 record each row's disposition (`ported-here` / `reused` / `owned-later(slice)`) in the ledger headers.

## Message-name lookup (E2)
131 distinct `Diagnostics.*` names referenced by the module (:114249-115873) and `declarations/diagnostics.ts` (:113795-114247); missing in `crates/diagnostics/src/gen.rs`: 0.

## Counting-grammar note (E2)
Non-row items ported under the owner header: `pushErrorFallbackNode` :114292-114300, `popErrorFallbackNode` :114301-114303, `throwDiagnostic` :114266, `restoreFallbackNode` :114279, the three `getSymbolAccessibilityDiagnostic` lambdas :114433/:115289/:115617.

## Witness artifact succession (E3 — parent-packet pin move)
- whole-file fingerprint before / after: edda8e69… / 012a8087… (parent_packet pin ceadfb54… → 56191ae3…; 120 cases / 240 oracle runs / 202 declaration writes)
- case-manifest fingerprint 81d5b13a639f5cc8e74bc82dea62aef8c2b54c70419cd1c575225a0ec24f3ab1 — reproduced: YES (byte-identical)
- observation-content roll 4d2e6f6dc52cf5e43356d3e8fd707bf4926638057d60ae98c49581cfb6a42cad — reproduced: YES (byte-identical)
- `--check` exit 0 twice: YES (check_receipt=hit); walk-preflight + pin-audit clean after the mint

## Probe artifact succession (E3 cascade — the witness pin moved)
- `node crates/oracle/h2-7a-probe-traces.mjs --check` refused after the witness re-mint (stale `witnesses` input pin) → `--write` (instrumented fresh-process double observation, 120 cases); trace-content roll ca24d47c… REPRODUCED, printed-results roll 7554535b… REPRODUCED, events 16,925 / printed_results 647 unchanged; witness pin dbbe7688… → 24c84397…; whole-file fingerprint fe62bebc… → 35f66909…; `--check` exit 0 twice (receipt hit); walk-preflight clean. The packet §1/§9 now record this cascade (the r5 text said "the probe does not re-mint" — corrected 2026-09-03).

## Resolver partition succession (P0 landing, measured)
The inventory partition stays m_3_head 7 / m_2 12: the three m-4 resolver additions (`create_type_of_declaration_in_expando_scope`, `is_last_bodiless_overload_of_symbol`, `is_first_declaration_of_symbol`) are NOT rows of the frozen m-1 consumed-member subset (`RESOLVER_MEMBER_SPECS` unchanged); they are recorded here and in the ledger headers. The inventory re-minted for the moved `inputs.rust_evidence` pin on resolver.rs: rows changed 0, fingerprint 94f1e111… → 2766bf1e…; `--check` exit 0 twice. (The packet's "7 → 10" forecast is superseded by this measurement.)

## E4 surprise-trigger assessment (operator, 2026-09-03)
No trigger fired → Phase P proceeds on this record (the m-2 §9.E4 rule); the one TO-VERIFY(P1) row is a measurement the P1 lane reports, not a fired trigger.
| trigger | fired? | disposition |
| --- | --- | --- |
| witness identity not reproduced | no | — |
| L2 domain ≠ 116 − exclusions | no (116 = 120 − 4) | |
| a `.d.ts`-bearing case with an option outside the §8 classes | no | |
| a factory face the m-3.5 faces cannot express additively | no (30 absent faces, all additive over existing create faces) | |
| an `EmitSource`/`EmitHost` fact with no additive accessor | TO-VERIFY(P1) | |
| a message name missing from gen.rs | no (0 missing) | |

## P4 convergence record (2026-09-03)
Trajectory (sol lane, `target/session-notes/m4/lanes/p4-lane.log`): divergences 4,084 → 4,080 → 3,270 → 3,082 → 3,048 → 901 → 433 → 169 → 117 → 80 → 7 → 6; byte mismatches 382 → 110 → 66 → 54 → 40 → 32 → 30 → 15 → 12 → 8; roots 187 → 201 → 202 with the frozen 199/3 split reached. Three harness-defect repairs in the gating test, each justified against the probe runtime: output-path key normalization (the probe records the writeFile callback path verbatim, Rust preflight presents an absolute path with a `.` component — compared in one lexical current-directory-relative domain; frozen paths/bytes/denominators untouched), boundary-site attribution from the probe's function wrappers (not AST ancestry), and the `__h27aEntryArgs` defaulted-third-argument projection (:141). Remaining two classes → the mid-train fence amendment (packet §9 P4): the chains.rs custom-host short-circuit (6 L1 rows + 3 byte diffs in `F1/alias-inlining` w2 and `S3/literalconst-4` w1/w2) and the ModuleBlock name-generation scope (`S2/expando-4` w0: `_b` vs upstream's namespace-local `_a`).
