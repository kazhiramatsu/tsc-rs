# h2-7b-w4 — register (values recorded as minted; `__X__` = TO-FILL at the named step)

Packet: `h2-7b-w4.md` (rev 1 draft at E1; ratification after the sol review r2). Trusted base `main @399c62ba`. Train `h2/7b-w4`.

## E1 (2026-09-06 JST)
- 06:53 `h2/7b-w4` cut from main 399c62ba; 06:54 the ahead lane W4-A0 merged (2b293d59; `lane/w4-a0` 9f8f6ff5, verified against its base c2075797; 11 rows incl. `exportDefaultNamespace`); 06:56 the integrator's S2 + P1 merged (9cd8f79c; `lane/w4-i` 203816a7 on d0cd9a0b; 10 rows; program 467+24 / harness 83+32 tests, clippy, fmt clean in the worktree); stubs `h2_7b_w4a_controls.rs` / `h2_7b_w4b_controls.rs` registered; launch scratch `target/session-notes/7b/lanes/w4/launch-scratch.json` (expected 34 live divergences of 55; closed 21) → verdict OK: scratch 34 live / canonical 55, closed 21, equal 33, strict subsets 1 (the inherited jsDeclarationsUniqueSymbolUsage reduction), problems 0 (07:04).
- Launch SHA c16f2d2e (E1c: the packet at rev 2 after the sol reviews r1/r2; E1b bc175dcd = stubs + packet + register; E1 2b293d59 / 9cd8f79c = the A0 and integrator merges); lanes launched 07:05 (W4-A gpt-5.6-sol xhigh `tsc-rs-w4-a`; W4-B gpt-5.6-sol xhigh `tsc-rs-w4-b`); preflight OK (SPEC-W4-A 11 allowed paths, SPEC-W4-B 9, disjoint; every file present at the launch SHA); the E1 train battery started 07:05 on the canonical checkout (jobs 2, 1 sweep worker).

## Lanes — TO-FILL at handoff (rows fixed / left / out of scope; battery tails; verify)

## Timeline (JST) — TO-FILL
- lane launch 07:05 → handoff A __T_A__ / B __T_B__ → train battery → re-mint → walk → gate → merge __T_MERGE__ (wall __WALL__).

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

## Implementation-time amendments
- 08:22 hosted `gates` on the E1 heads is RED by design (run 33996594489, 40m43s): `cargo xtask acceptance`'s H2.7b suite check reports the 21 closed rows as stale manifest entries ("is exact now (shrink the manifest)") plus the inherited `jsDeclarationsUniqueSymbolUsage` facet reduction; it turns green after the E2 re-mint, as in w3 (hosted passed on E4/E5/E6 there). No code defect: the compiler suite 458/10 exact/known and the project suite 196/0.
- 07:39 the user reported high CPU load (the E1 train battery's compiler-contracts test at ~200% + the two lanes' cargo work): the integrator stopped the E1 train battery after its bands had all matched w3 (7b 1523/34, 6c 321/318/4, 5h 830/58/44, 6a 171/4/2, 6b 6/0/0, 5g 8511 exact) — its unit-test / clippy / fmt / ledger tail runs on the merged train — and demoted both lane process trees to `nice 20` (priority only; the foreground band stays — the E-core lesson). The kill of the canonical `xtask h2-7b-acceptance` may have caught a lane's in-flight scratch sweep (a relative `target/debug/xtask` command line is indistinguishable from the canonical one by path): if a lane STATUS reports one aborted sweep around 07:39, that is the integrator's interference, not a lane fault.
