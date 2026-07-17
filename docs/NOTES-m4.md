# M4 close notes

Per m4-checker-skeleton-steps.md "Final gate" and m4-end-sweep-steps.md
5.9e: the close-state record, the manual stub audit, and the top
one-sided codes with owner guesses — this file seeds M5/M6
verification and the M8 backlog.

## Close state (2026-07-18, m4/5.9e-close)

Measured under the oracle-correction epoch truth (PR #19, goldens
regenerated under the pinned producer, Node v25.2.1):

- **All band: T0 42.7403% (20,953/49,024), FP 0, FN 28,071**
- 2xxx band: 52.9714% (11,151/21,051), FP 0
- Syntactic band: 99.8219% (2,242/2,246), FP 0
- Shadow tiers: T1 42.7342% / T2 39.8988% / T3 38.6627%
- Escapes: 227 sites, **0 untagged**, **0 stale at STAGE 5.9**,
  112 recovery-class
- relpin 415/415; invariants (idempotence) green; ledger green;
  accepted-set ratchet (A1) green vs trusted base

## Go/no-go vs definition-of-done

definition-of-done.md M4-close row: T0 ≥ 35% (All band), untagged
escapes 0, stale 0 — first real go/no-go.

| Criterion | Required | Measured | Verdict |
|---|---|---|---|
| T0 All band | ≥ 35% | 42.7403% | pass |
| Untagged escapes | 0 | 0 | pass |
| Stale escapes at 5.9 | 0 | 0 | pass |

**M4 close: GO** (recorded 2026-07-18). The M6 checkpoint
(speculation transaction) and M8 checkpoints (T1–T3 shadow gates,
emitter inventory, fuzzer) from the 2026-07-14 external review
remain conditions on THOSE milestones, not on this close.

## Manual stub audit (final-gate step)

`grep -rn "M[5-8]-stub" crates/` — 40 sites, classes:

| Class | Sites | Allowed by final gate |
|---|---:|---|
| M6-stub | 8 | yes (inference surfaces per 5.7) |
| M7-stub | 16 | yes (late-bound members option taken in 5.3) |
| M8-stub | 16 | yes (conditional/mapped type nodes per 5.1) |
| M5-stub | 0 | (flow stubs live behind escapes, not stub markers) |
| M3/M4-stub | 0 | correct — M3 normalization stubs un-stubbed in 5.3 |

No disallowed residual classes.

## Top 10 one-sided codes (owner guesses)

FP = 0 corpus-wide, so every one-sided code is oracle-only (FN).
Counts from the full All-band run at close
(`target/conformance/m4-close.json`). Owners follow
non-2xxx-first-order.md's family map where the code is non-2xxx;
2xxx rows carry the sweep's attribution. These are guesses to seed
verification, not adjudicated gates — the A5 rollup pins the real
(code, pass) split.

| # | Code | FN | Owner guess | Mechanism |
|---:|---|---:|---|---|
| 1 | 6133 | 12,936 | M7 (8.3 error + 8.4 suggestion) | "declared but its value is never read" — the unused family alone is ~79% of non-2xxx FN mass |
| 2 | 2454 | 3,962 | M5 | "used before being assigned" — definite-assignment analysis over the flow graph |
| 3 | 2322 | 1,362 | M5 + M6 (residual T2/M8 display) | assignability where the source type needs narrowing or inference to match |
| 4 | 6196 | 1,334 | M7 (8.3 + 8.4) | "declared but never used" |
| 5 | 2304 | 1,003 | M5 | cannot-find-name reached via flow-sensitive paths (2304-via-flow attribution from the sweep) |
| 6 | 2339 | 525 | M5 (+ M8 residue) | property miss on narrowable receivers; small residue behind the Suppressed-augmentation M8 escape |
| 7 | 18050 | 354 | M5 | "value cannot be used here" (null/undefined) — strict-nullability flow family |
| 8 | 2345 | 349 | M6 (+ M5) | argument assignability under the inferTypeArguments stub |
| 9 | 2307 | 289 | M8 (resolver backlog) | cannot-find-module residue — package-exports/JSON resolution incl. the 1543-family |
| 10 | 80007 | 239 | M7 8.4 | "'await' has no effect" — await/bignum suggestions row |

Next tier (11–20, same sources): 7044 (220, M7 8.4
infer-from-usage), 6198 (187, M7 unused), 18048 (175, M5
nullability), 2365 (157, M5/M6 operator applicability), 1479 (143,
M8 module-semantics interop), 6053 (142, M7 8.5 program/options),
1206 (129, M7 8.1 checker grammar), 7032 (127, M7 8.4), 2834 (120,
M8 resolver), 7005 (120, adjudicate M6-adjacent vs M8 implicit-any).

## FN evidence split (seeds the verification order)

Of 28,071 FN: **11,799 carry partial-boundary evidence** (the
checker reached the construct and recorded why it stopped) and
16,272 have none (mostly the M7 unused band, which has no M4-side
boundary to record). Top recorded reasons:

| FN | Boundary reason (owner) |
|---:|---|
| 4,190 | flow-sensitive use-before-assignment (M5) |
| 1,601 | overload band over a parse-recovery tree (recovery) |
| 1,454 | typeToString beyond the 5.4 display slice (T2/M8) |
| 1,353 | instantiateSignatureInContextOf (M6) |
| 940 | inferTypeArguments stub — generic call w/o explicit args (M6) |
| 2,186 | [FLOW M5] family: nullable receiver 741, failed initializer 558, implicit-return 405, guarded property miss 333, union RHS 118, ctor property 31 |
| 327 | declaration without a bound symbol (recovery) |
| 325 | JS expando analysis ([JSDOC] M8) |
| 421 | mapped 289 + conditional 132 (M8-stub families) |
| 342 | display tails: tuple renderer 140, UseFullyQualifiedType 39, 2507 ctor-function 36, bigint template 35, misc (M6/T2 M8) |

Reading: the M5 flow slice (use-before-assignment + FLOW-M5 family
+ 18048/18050) is the single biggest evidenced mass (~6.7k), and
the M6 inference pair (instantiateSignatureInContextOf +
inferTypeArguments) is next (~2.3k) — those two milestones start
from these boundaries, not from zero. The unevidenced mass is
dominated by M7 unused (14.6k), which needs its own pass, not
checker boundaries.
