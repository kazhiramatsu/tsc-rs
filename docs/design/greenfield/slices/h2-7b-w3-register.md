# h2-7b-w3 — register (values recorded as minted; `__X__` = TO-FILL at the named step)

## Design gate
- Packet rev 0 (2026-09-05, integrator-authored during the w2 gate from the W3-X analyses): sol cross-review r1 (20:15, 9 min) = REVISE — B1 D1's owner set includes `elaboration.rs`, absent from W3-A's fixed allowed set → D1 WITHDRAWN from the wave (the lane-local ticket copy carries the withdrawal note; allowed sets stay fixed); B2 the register's verify line lacked the E1 sha argument → fixed; B3 criterion 4 had exempted the automatic normal rerun → the inherited w2 measurement restored (a rerun counts) and the gate runs in the foreground band with `nice` when the machine is idle; B4 the register said > 23 rows / 8 h 30 m → > 40 rows / 8 h 33 m; non-blocking: W3-E placeholders removed, rev metadata, T7/J10 labels, the A3 count → rev 1 (20:25) → r2 (20:25, 6 min) = AGREE — B1-B4 RESOLVED, N4 (the A3 summary count) applied after ratification — **rev 1 RATIFIED 20:31 JST**.

## Preflight (the w3 operating rules)
- Launch check (`preflight.py SPEC-W3-A.md SPEC-W3-B.md` at E1 b06c6575): OK — 2 disjoint sets (17 + 9 paths), 0 single-writer hits, 0 missing files.
- Verify before integration (`preflight.py --verify <worktree> <SPEC> <E1 sha>` — the launch base is the operator's explicit argument): W3-A __VERIFY_A__; W3-B __VERIFY_B__.

## Lanes (the staffing cell)
- W3-A (gpt-5.6-sol xhigh): launched __LAUNCH_A__; STATUS __STATUS_A__ (rows fixed / left / out of scope: __ROWS_A__); full battery __FULL_A__.
- W3-B (gpt-5.6-sol xhigh): launched __LAUNCH_B__; STATUS __STATUS_B__ (rows fixed / left / out of scope: __ROWS_B__); full battery __FULL_B__.
- Analysis (gpt-6-astra xhigh, docs-only, when the integrator assigns it): __ASTRA__.
- Integrator: E1 (packet draft, register skeleton, the two registered stubs, the 5g shadow registration) landed 2026-09-05 20:14 JST as b06c6575 on `h2/7b-w3` from `main @c0e21f32`; checkpoints __CHECKPOINTS__.

## Timeline (JST)
- lane launch 20:15 (A and B from E1 b06c6575; time box: stop 23:45, full battery 23:50, STATUS 00:45) → handoff __T_HANDOFF__ → lanes merged __T_MERGED__ → train battery green __T_BATTERY__ → re-mint __T_REMINT__ → walk __T_WALK_START__ … cert __WALK__ → gate __T_GATE_START__ … exit __T_GATE_END__ → merge __T_MERGE__ (wall lane launch → merge: __WALL__; w2 reference 11 h 04 m, the target ≤ 8 h 33 m).

## First w3 sweep / re-mint — TO-FILL
- exact __EXACT__ / diverging __DIVERGING__ (rows __ROWS__; owner stays `h2-7b-m-2-divergence-closure`) — measured on the merged train before the canonical re-mint; closed rows __CLOSED__ (list, each with its §3 mechanism); byte-identical __EQUAL__; strict subsets __SUBSETS__; added elements 0; new rows 0.
- adjacent bands (merged train, workers=2): 6c __6C__ (base 639 / 4 / 643), 5h __5H__ (824 / 64 / 44), 6a __6A__ (171 / 4 / 2), 6b __6B__ (6 / 0 / 0), full 5g __5G__ (9,027 / 8,511); whole suites: compiler contracts __CC__, checker __CHK__, xtask __XT__, emitter contracts __EC__; clippy; fmt; ledger __LEDGER__ (base 3,787 / 0 / 0).

## Walk / gate / PR — recorded in the PR body at the final head
- walk cert __WALK__ (launches: __WALK_LAUNCHES__); gate __GATE_LINE__ (reruns: __GATE_RERUNS__); hosted __HOSTED__; PR __PR__.

## Success criteria (packet §7) — recorded at close
| # | criterion | value | pass |
|---|---|---|---|
| 1 | one walk launch | __C1__ | __P1__ |
| 2 | zero train-battery-first regressions | __C2__ | __P2__ |
| 3 | zero allowed-path STOPs | __C3__ | __P3__ |
| 4 | zero perf-contention reruns | __C4__ | __P4__ |
| 5 | > 40 rows in ≤ 8 h 33 m | __C5__ | __P5__ |
| 6 | zero lane conflicts / multi-writer pins | __C6__ | __P6__ |

## Implementation-time amendments
- (none yet)
