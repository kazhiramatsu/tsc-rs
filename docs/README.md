# Documentation

This directory contains project notes that should be tracked with the repository.

- [design/](design/README.md): the implementation-ready design set for the
  remaining conformance work — priority roadmap, the mandatory working
  protocol (0-NEW_FP gate, probe/classifier recipes), per-workstream designs
  and numbered step guides, the knowledge base of pinned facts, the stall
  playbook (architectural ceilings + refactor house style), and the greenfield
  rebuild design. **Start any handoff at `design/README.md`.**
- [determinism-design.md](determinism-design.md): why the checker is
  single-threaded per program and how the CFG flow resolver replaced the
  fact stack.
- [phase1-status.md](phase1-status.md): current Phase 1 status, remaining
  diagnostic differences, and recommended next steps.
- [setup.md](setup.md): bootstrap requirements, generated fixtures, and common
  verification commands.

Use `../scripts/bootstrap.sh` to prepare local oracle, corpus, and `/tmp`
fixtures needed by `../verify.sh`.

Keep transient outputs, local golden files, and generated scratch data outside
this directory unless they are intentionally being promoted to project docs.
