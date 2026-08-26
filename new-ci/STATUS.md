# new-ci status (2026-08-27)

All three overnight milestones complete and independently verified:
- M1 pin-index generator (luna): 27,255 pins / 389 files, 0 unclassified
  oracle literals, incident validation e8e32f61 1124/1124 + e1957f77 6/6
  with 0 misses.
- M2 prospective plan (luna): gate-tax-4 landing acceptance 55/61 stale
  rungs (matches the measured walk), 6/6 surfaces, precision/recall
  1.000/1.000.
- M3 substrate hardening (sol): transaction promotion + fencing leases +
  status receipts, 19 tests incl. adversarial (crash windows, CAS race,
  stale-owner fencing).

Agents cannot git-commit in linked worktrees (index outside sandbox);
all work committed at review.
