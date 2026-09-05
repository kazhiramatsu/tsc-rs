# h2-7b-w4 — register (values recorded as minted; `__X__` = TO-FILL at the named step)

Packet: `h2-7b-w4.md` (rev 1 draft at E1; ratification after the sol review r2). Trusted base `main @399c62ba`. Train `h2/7b-w4`.

## E1 (2026-09-06 JST)
- 06:53 `h2/7b-w4` cut from main 399c62ba; 06:54 the ahead lane W4-A0 merged (2b293d59; `lane/w4-a0` 9f8f6ff5, verified against its base c2075797; 11 rows incl. `exportDefaultNamespace`); 06:56 the integrator's S2 + P1 merged (9cd8f79c; `lane/w4-i` 203816a7 on d0cd9a0b; 10 rows; program 467+24 / harness 83+32 tests, clippy, fmt clean in the worktree); stubs `h2_7b_w4a_controls.rs` / `h2_7b_w4b_controls.rs` registered; launch scratch `target/session-notes/7b/lanes/w4/launch-scratch.json` (expected 34 live divergences of 55; closed 21) __SCRATCH__.
- Launch SHA __LAUNCH_SHA__; lanes launched __T_LAUNCH__ (W4-A gpt-5.6-sol xhigh `tsc-rs-w4-a`; W4-B gpt-5.6-sol xhigh `tsc-rs-w4-b`); preflight __PREFLIGHT__.

## Lanes — TO-FILL at handoff (rows fixed / left / out of scope; battery tails; verify)

## Timeline (JST) — TO-FILL
- lane launch __T_LAUNCH__ → handoff A __T_A__ / B __T_B__ → train battery → re-mint → walk → gate → merge __T_MERGE__ (wall __WALL__).

## Walk / gate / PR — TO-FILL
- walk cert __CERT__ (launches __LAUNCHES__); gate __GATE_LINE__; hosted __HOSTED__; PR __PR__.

## Success criteria (packet §7) — recorded at close
| # | criterion | value | pass |
|---|---|---|---|
| 1 | one walk launch | __C1__ | __P1__ |
| 2 | zero train-battery-first / gate-first regressions | __C2__ | __P2__ |
| 3 | zero allowed-path STOPs | __C3__ | __P3__ |
| 4 | zero perf-contention reruns | __C4__ | __P4__ |
| 5 | ≥ 35 rows in ≤ 8 h 33 m | __C5__ | __P5__ |
| 6 | zero lane conflicts / multi-writer pins | __C6__ | __P6__ |

## Implementation-time amendments — TO-FILL
