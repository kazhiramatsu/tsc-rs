# Retired standalone CI prototype

The user retired the standalone root `new-ci/` project on 2026-09-21 during
repository cleanup. It was outside the Cargo workspace and was not used by
the current hosted acceptance or witness workflows. Its Rust source, Cargo
files and planner coverage adapter have been removed. The optional planner
invocation and its duplicate-order check were removed from the legacy
chain-walk driver; the driver's own registry and topology checks remain.

Current CI uses [.github/ci/replay.py](../../../../.github/ci/replay.py) and
[scripts/witness.py](../../../../scripts/witness.py). This retirement does
not change their selection, coverage, limits or comparisons, and does not
restart the retired reference-hash chain. The existing acceptance impact
classifier still recognizes the historical `new-ci/` path; that string
does not load or execute the removed prototype. Python pin-index tooling
retains its independently implemented extraction logic and provenance notes.

## Preserved documents and evidence

These seven files retain their original bytes:

- [prototype-readme.md](prototype-readme.md): original README and limitations.
- [SPEC.md](SPEC.md): original substrate and shadow reporter mission.
- [SPEC2.md](SPEC2.md): subsequent implementation mission.
- [SPEC-M3.md](SPEC-M3.md): transaction, lease and status hardening mission.
- [STATUS.md](STATUS.md): recorded milestone outcomes.
- [LEGACY-FCI-DISPOSITION.md](LEGACY-FCI-DISPOSITION.md): prior FCI retirement rationale.
- [plan-report.md](plan-report.md): previously ignored report copied from the
  primary worktree. It predicts changes from commit `399c62bace578d8d0a54a664f01a2525bcb5aab1`
  to a then-current `HEAD`; that `HEAD` is not independently identified in the
  report. It is historical output, not a qualification of today's tree.

The [evidence-DAG design](../../greenfield/new-ci-evidence-dag.md) and
[Phase 0 packet](../../greenfield/new-ci-phase-0.md) remain available as
reference. Commands and old paths inside the archived files describe their
original context and do not authorize execution now.

## Recovering the implementation

The last complete source is preserved in Git at
`fc50535913946b0537d3292b6d66062c58e9ec9d`, under `new-ci/` and
`scripts/walk-planner-coverage.py`. Inspect that commit or export those paths
when studying the prototype. Existing frozen records retain their old paths
and hashes; they have not been regenerated or re-signed.
