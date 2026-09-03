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

## Receipt-miss root cause (completed on the train, 2026-09-03)

The lane's `inputs` projection is a valid consistency repair (the key now
matches the 5g/6a/6b idiom), but the PERMANENT miss had a second, decisive
cause found by the train's walk and gate: `h2-6c-census.mjs:148` and
`h2-6c-qualification.mjs:62` minted their receipts (kinds
`h2-6c-census-check-receipt` / `h2-6c-qualification-check-receipt`) into the
SAME file `target/h2-6c/check-receipt.v1.json`, overwriting each other.
Every ordered `census --check` → `qualification --check` (the chain-walk
ORDER and the `h2-6c-oracle` phase) therefore found the other machine's
receipt, failed the `kind` guard as `stale`, and re-observed — a ping-pong
that also re-censused on the next census check. Consecutive qualification
checks with nothing in between hit (the four post-walk manual runs), which
is what exposed the collision. Fix: the qualification receipt moves to
`target/h2-6c/qualification-check-receipt.v1.json` (the census keeps its
path); every other oracle script already owns a distinct receipt path
(verified by grep over `crates/oracle/*.mjs`). Proof: the train's second
walk (round 2 census AND qualification receipt hits in seconds) and the
`h2-6c-oracle` phase of the gate at the final head.

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

The lane's full command tails are archived at
`target/session-notes/repair/STATUS-6c-lane.md` (session notes, untracked);
the train's walk cert, gate summary, and hosted result are recorded in the
PR #502 body at the final head.

## Deviations and open questions

- The §0 standalone pre-step and the chain walk/full gate remain operator
  responsibilities and were not run in this linked worktree.
- The lane could not commit in its linked worktree; the operator committed
  its diff on `lane/6c-repair` (60c48b64) after the battery above and merged
  it into the train `fix/h2-6c-acceptance-wiring` (e429a19d).
