# Documentation

This directory contains project notes that should be tracked with the repository.

- [phase1-status.md](phase1-status.md): current Phase 1 status, remaining
  diagnostic differences, and recommended next steps.
- [setup.md](setup.md): bootstrap requirements, generated fixtures, and common
  verification commands.

Use `../scripts/bootstrap.sh` to prepare local oracle, corpus, and `/tmp`
fixtures needed by `../verify.sh`.

Keep transient outputs, local golden files, and generated scratch data outside
this directory unless they are intentionally being promoted to project docs.
