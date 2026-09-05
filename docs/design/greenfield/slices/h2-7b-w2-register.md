# h2-7b-w2 — register (values recorded as minted; `__X__` = TO-FILL at the named step)

## Design gate
- Packet rev 1 (2026-09-05, integrator-authored draft from the w1 census + the verified owners; landed at E1 1f777680): sol cross-review r1 (launched 09:07, worktree `tsc-rs-w2-r`) = __R1__ → rev 2 → r2 = __R2__.

## Preflight (the w2 operating rules)
- Launch check (`preflight.py SPEC-W2-A.md SPEC-W2-B.md SPEC-W2-E.md` at E1): OK at E1 1f777680 — SPEC-W2-A 12 paths, SPEC-W2-B 9, SPEC-W2-E 1; all three intersections empty; 0 single-writer hits; 0 missing files.
- Verify before integration (`preflight.py --verify <worktree> <SPEC>`): W2-A __VERIFY_A__; W2-B __VERIFY_B__; W2-E __VERIFY_E__.

## Lanes (the staffing cell)
- W2-A (gpt-5.6-sol xhigh): launched 09:07 JST from E1 1f777680 (time box: stop 11:57, full battery 12:02, STATUS 12:42); STATUS __STATUS_A__ (rows fixed / left / out of scope: __ROWS_A__); full battery __FULL_A__.
- W2-B (gpt-5.6-sol xhigh): launched 09:07 JST from E1 1f777680 (same time box); STATUS __STATUS_B__ (rows fixed / left / out of scope: __ROWS_B__); full battery __FULL_B__.
- W2-E (gpt-5.6-luna max): launched 09:07 JST from E1 1f777680 (STATUS 12:42); STATUS __STATUS_E__ (census rows: 34; residual owners; w3 seed).
- Integrator: E1 (packet draft, register skeleton, the three registered stubs, the 5g shadow registration) landed 2026-09-05 09:06 JST on `h2/7b-w2` from `main @709fb8fa`; checkpoints __CHECKPOINTS__.

## Timeline (JST)
- lane launch 09:07 → handoff __T_HANDOFF__ → lanes merged __T_MERGED__ → train battery green __T_BATTERY__ → re-mint __T_REMINT__ → walk __T_WALK_START__ … cert __WALK__ → gate __T_GATE_START__ … exit __T_GATE_END__ → merge __T_MERGE__ (wall lane launch → merge: __WALL__; w1 reference 8 h 30 m).

## First w2 sweep / re-mint — TO-FILL
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
| 5 | > 23 rows in ≤ 8 h 30 m | __C5__ | __P5__ |
| 6 | zero lane conflicts / multi-writer pins | __C6__ | __P6__ |

## Implementation-time amendments
- (none yet)
