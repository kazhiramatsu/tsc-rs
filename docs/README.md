# Documentation

This directory contains project notes that should be tracked with the
repository. Active development targets `tsrs2/` (the greenfield
rewrite); the paused v1 codebase is preserved at tag `v1-final`.

- [design/](design/README.md): the design set. Durable architecture
  docs plus `design/greenfield/` — the authoritative milestone step
  guides and convergence plan for the active rebuild. **Start any
  handoff at `design/greenfield/README.md`.**
- [setup.md](setup.md): requirements (pinned Rust/Node toolchains) and
  the verification commands.
- [NOTES-m1.md](NOTES-m1.md): M1 final-gate triage — the one-line
  classification of every residual parser mismatch at M1 close.
- [NOTES-m4.md](NOTES-m4.md): M4 close notes — close-state record,
  manual stub audit, and top one-sided codes seeding M5/M6 work.
- [design/archive/](design/archive/README.md): v1-era roadmaps,
  workstreams, and operating instructions, kept for provenance.

Keep transient outputs, local golden files, and generated scratch data
outside this directory unless they are intentionally being promoted to
project docs.
