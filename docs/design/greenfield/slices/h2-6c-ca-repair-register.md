# H2.6c ca repair register — acceptance wiring and receipt reuse

## Identity and correction

This register records the repair of the two false completion claims in
`h2-6c-ca.md:12` and `h2-6c-ca.md:168`. The signed close packet is left
unchanged. At the repair base (`46e3e487`), `run_h2_6c` existed (landed by
`859a0fea`) and the standalone `h2-6c-acceptance` command was reachable, but
the hosted `fn acceptance` boundary stopped at `run_h2_6b`; the local CI
oracle phase and the two 6c schema registrations were also absent.

The repair adds the missing hosted call and `h2-6c-oracle` phase, registers
the census and qualification contracts, and adds guards over the acceptance
surface, oracle phases, ORDER/schema registry, and registry ordering. No
signed design packet, ratchet, or manifest was edited by this lane.

## Receipt-miss correction

The receipt miss was inside `global_records_sha256`; the generator, Node,
workspace, and observation-content terms were otherwise valid. The
asymmetric sub-term was the `inputs` envelope: the check side built it from
fresh inputs while the mint side reconstructed it from the stored artifact.
Its pin-only `owner_inventory` member was not an observation input, so a
pin rebind changed the envelope shape/value without changing any observed
case. The related `global_candidate_dispositions` member is projected out as
well when present.

The fix follows the established `observationInputs` projection used by the
H2.5g/H2.6a/H2.6b receipt machines. The receipt key now hashes
`observation_inputs`, and stored-artifact reuse applies the same projection;
owner closure, census/project-mount fingerprints, execution contract,
library inventory, and every per-case identity remain guarded.

## Evidence

Before the repair, the direct 6c qualification check completed the full
643-case observation and minted a local receipt, but repeated checks could
not establish a stable receipt hit. The required debug run printed the
check-side global terms and reproduced the pin-carrying `inputs.owner_inventory`
term. After the projection fix, the check again observed 643 cases but failed
closed on the frozen artifact's raw generator hash; the operator walk must
re-mint that recorded field before a receipt hit can be demonstrated. The
complete command tails and this stop are recorded in the lane root
`STATUS.md`.

The frozen qualification artifact remained byte-identical throughout; no
`ratchets/*.json` file was hand-edited. The 6c contract registry contains 21
entries after this repair (the two new entries are H2.6c census and H2.6c
qualification).

## Verification record

The runnable verification record is maintained in the lane's root
`STATUS.md`, including command result tails, the pre-step deviation, and the
operator-only walk/full-gate exclusions.

## Deviations and open questions

- The §0 standalone pre-step and the chain walk/full gate remain operator
  responsibilities and were not run in this linked worktree.
- No repair commit was created, as required for this linked worktree; the
  implementation is the working-tree diff from `46e3e487`.
