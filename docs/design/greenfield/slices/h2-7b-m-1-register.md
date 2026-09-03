# h2-7b-m-1 — Phase-E/P register (values recorded as minted; `__X__` = TO-FILL at the named step)

## Design gate
- Packet rev 3 RATIFIED 2026-09-03 (sol r1 REVISE 13+1 → r2 REVISE 5 partial → r3 AGREE). Authority file: `docs/design/greenfield/slices/h2-7b-m-1.md`.
- Frozen order: the 6c repair train (`fix/h2-6c-acceptance-wiring`) → this rung → m-2. Trusted base for the train: the repair-train merge sha `a67fdd37`.
- Registration base at the train cut: 21 contracts (19 + the two 6c pairs); this rung → 22.

## The band (measured at authoring 2026-09-03; asserted at every build)
GLOBAL 2,456 (compiler 1,034 / conformance 861 / project 528 / transpile 33); CANDIDATE 1,593 (921 / 476 / 196 / 0); FROZEN_NEXT_SLICE 1 (`compiler/modulePreserve4.ts#default`); dispositions `cases` roll `ed0036eb…`.
First-cut census (settings + matrix, pre virtual-config merge): compiler deferred ≥ 25 (isolatedDeclarations 21, noEmitOnError 2, stripInternal 1, standalone emitDeclarationOnly 1) + explicit-false declarationMap 10 (admitted) + removeComments 60 (4 true / 56 false, admitted facet); conformance deferred ≥ 1 (stripInternal); project: 22 config-driven rows TO-VERIFY; six `.d.ts`-only project rows = admitted `no-emit-eligible-source` controls.

## Machine (first mint) — TO-FILL
- `ADMITTED_H2_7B_ROWS` / `DEFERRED_H2_7B_ROWS` per suite: compiler `896/25`, conformance `475/1`, project `196/0`, transpile `0/0` (total `1567/26`); `typescript_runs` (= 2 × admitted): `3134`; declaration writes: `2410`; emit_refused: `34`; first_deferred_slices: `H2.7c=26`
- generator sha256 `f1750148475eeb0d3b87353c3c209a2997e6af9d7804956c74b0e8b14f09ed90`; contract sha256 `0d8c2484b0effd82b7c928f971ebbcf9e655e0405ef6357ed0cb1f3550207ad9`; `qualification_fingerprint_sha256` `773506218ac5f655e0017502b7501adaa29d0602fc233ece3fd1f88132481316`; check receipt minted: yes, before the stored-artifact byte comparison; second `--check` hit: yes, adopted `1567` in `1.121s`
- resource measurements at `--preflight` / `--probe project:3` / `--probe compiler:3`: wall `2.729s / 1.128s / 1.511s`; preflight per-process RSS `630652928 / 962314240` bytes (explicit probes `550617088 / 859652096`; ceilings 4 GB / 12 GB); `--write` wall `263.26s` (STOP 40 min; `25` atomic checkpoint snapshots, final checkpoint deleted)
- owner_arms amendment: h2-transition outputs regenerated (`owner_roots` 50 / `owner_arms` 4): yes; the H2.7a inventory span pin :448-467 byte-identical + line numbers verified: yes (`25632b51bf9ea161a1b472e97f66b8d46f8b9e92b980e4effd4c0b1472d6cdd`); h2-7a-close re-minted pin-only: no, `--check` refused `h2-7a-close: close input pins changed`; 6c pass count in the walk: `0` in this lane (walk stopped on stale pin surface)

## Walk / gate / PR — recorded in the PR body at the final head

## Operator review (train `h2/7b-m1`, 2026-09-04)

- Lane: codex luna max on SPEC-M1 (82 min; STATUS archived at
  `target/session-notes/7b/lanes/STATUS-m1-lane.md`); commit 12730fa0.
- Operator battery (independent re-run, demoted): `node --check` ×2;
  `h2-transition.mjs --check` fresh (owners 50 / arms 4; dispositions `cases`
  roll `ed0036eb…` unchanged); `qualification.mjs check` 22 contracts;
  `qualification.test.mjs` 38/38; pin-index clean (64 consumers / 255 sites);
  walk-preflight = exactly the walk-owned harness row (`h2_1a_profile`) stale;
  planner coverage + topology 71/71; `--preflight` census reproduced
  (896/25, 475/1, 196/0; floor 1567/1567; negative facts 0); the frozen
  :448-467 span sha `25632b51…` byte-identical and the three pre-448 edits
  line-count-neutral (verified per hunk); `git diff --check` clean.
- Receipt ordering verified in code: both observation paths mint the receipt
  before the stored-artifact byte comparison (sharded and single-process);
  the receipt-hit path compares without minting (no mint is needed there).
- Independent receipt proof (the lane receipt moved away, canonical-equivalent
  4-shard `--check`, then `--check` again): check 1 (receipt absent) = the full 4-shard pass, observations 1567/1567, `check_receipt=minted`, 557 s wall demoted (the background band runs on efficiency cores; the lane measured 89 s at normal priority); check 2 = `receipt: hit; adopted 1567 stored observations` in 8 s; the artifact byte-identical (sha256 `d4a979e3…`, `git status` clean).
- Accepted deviations (operator judgment, packet §3/§3.6):
  1. `GLOBAL_DISPOSITIONS_CASES_SHA256` is a frozen 64-hex literal in the
     machine, classified `unmatched` in the pin-index — the packet's
     registration bullet said "grammar-D pin; no frozen literal". Accepted:
     the same era-frozen roll is a literal in `h2-7a-close.mjs:26`, the
     H2.7a close artifact asserts it, and a fail-closed assertion at every
     build is the §2 contract ("asserted at preflight and every build").
  2. The receipt's `census_disposition_roll_sha256` is the census-authority
     `cases` roll (the dispositions input), not a roll over the 1,593 census
     records' dispositions. Sound: the census is a pure function of the
     generator hash (in the key) and the projected inputs (in the key); the
     dropped whole-file dispositions hash is replaced by this roll, so a
     cases change misses and a pin-only change hits — the §3.6 intent.
  3. The lane ran every node/python command under `nice -n 15` (the sandbox
     denies `setpriority`); the operator battery ran demoted.
- Mid-train fence amendment (packet §9.6): the `h2-7b-oracle` local-CI
  phase in `crates/xtask/src/main.rs` + the hosted policy pin re-record —
  the repair guard's mechanical consequence; the walk restarted from the
  amended tree (the first walk, run 20260904-011132-75241, was stopped by
  the operator at h2-1a-qualification).
