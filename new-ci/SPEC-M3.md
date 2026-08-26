# new-ci M3-only mission (parallel worker, 2026-08-26 night)

Same hard boundaries as SPEC.md/SPEC2.md: ONLY new-ci/ changes,
repository read-only, no root-workspace cargo, no network. Commit when
done (or progress + new-ci/STATUS.md if you must stop). Another agent
is working on M1/M2 on a DIFFERENT branch — do not worry about them;
implement M3 as self-contained new library modules with minimal edits
to existing files (module declarations only) so the morning merge is
trivial.

## M3 — substrate hardening
 — substrate hardening (design §3/§7/§8 into code)

Implement in the library what DESIGN (now at
docs/design/greenfield/new-ci-evidence-dag.md, identical content to
your draft) specifies but the first slice stubbed:

- transaction manifest + immutable root-generation promotion with the
  two crash windows recoverable (before-close, after-close);
- advisory leases with owner tokens and fencing epochs; stale-owner
  CAS loss; reclamation only via higher epoch;
- typed status receipts (success/failed/cancelled/timed-out/
  diagnostic) with only verified success eligible for HIT;
- adversarial tests: kill-simulation for both promotion crash windows,
  concurrent CAS mint race (threads), lease fencing (stale owner loses
  even with a wrong clock), status-eligibility.


## Review contract

Morning review re-runs everything independently; claims without a
runnable demonstration are treated as absent. cargo clippy clean
inside new-ci/.
