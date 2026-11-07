# SPEC-0b.6 — the thin vertical pilot: type lists and instantiated symbols on the checker arena, measured

Track: [ts71-arch-arena](../README.md), phase 0b ([SPEC-0b](SPEC-0b.md) §0b.6, reviews R15/R17/M4). This is phase A's first real
step run as a **measured pilot on a branch** before the freeze: it exercises the ownership model the design rests on — `'static`
views from a checker-owned arena inside recursive `&mut self` checking, the speculation journal, instantiated symbols that read
through an origin — on the real compiler, and produces a native A/B number for one record family. Depends on `tsc-rs-arena`
(PR #914); start when it is on main. Lane rules as [SPEC-arena](SPEC-arena.md); time box **one week**; builds allowed: `cargo
check --workspace`, `cargo test -p tsc-rs-checker` (focused modules, `--test-threads=1`), one dev build of the CLI. No release
build, no measurement: the integrator measures (`serial.py` 1 thread, `diffout.py` 8 projects, `conformance_ts71.py --checkers 4`,
Types/Symbols/Instantiations rows).

## Part 1 — type lists as arena spans (families: `TypeListId`, union/intersection members, type arguments, alias arguments)

- `TypeTables` gets a `LocalArena` (owner = checker index). `intern_type_list` stores the list as `Ref<List<TypeId>>` in the arena
  and keeps the content-hash intern table mapping to that `Ref`; `TypeListId` becomes a newtype of the `Ref` (keep the name).
  `list(id) -> &'static [TypeId]` returns the arena slice (no `&self` borrow); `list_copy`/`ListCursor` are deleted where callers
  can now hold the slice across `&mut self` calls (the B class of the review's copy sample: `annotate.rs:4544/5881`, `engine.rs:4607`,
  `inference.rs:624/997`, `instantiate.rs:923/1951`, `structural.rs:5417/9280` and their siblings). S-class sites (independent
  snapshots, builders) keep their `Vec`s.
- Lists are immutable once interned (today's property). The arena is dropped with the checker. Keep `TypeTables::len()` and
  every count that feeds the statistics rows unchanged.
- Record in `STATUS.md`: the number of `to_vec()`/`list_copy` call sites removed, by file, and the sites deliberately kept with
  their class (B/S/I/O).

## Part 2 — instantiated transient symbols with an origin (family: `instantiate_symbol`'s products only)

- A new record in the checker `LocalArena` for symbols created by `instantiate_symbol` (`CheckFlags::INSTANTIATED`):
  `{flags, origin: SymbolId, check_flags, target, mapper, type_of_symbol: ArenaCell, declared_type: ArenaCell, name_type}` (32 B,
  gen2 `TransientSymbol`), reads of name / declarations / value_declaration / parent go through `origin`. Every other transient
  symbol keeps today's path. The transient `SymbolId` space stays as today (high bit), so `symbol(id)` dispatches on a per-id tag
  to the record kind; API handles and ordering ids are unchanged.
- Before changing code, run the mutation census the review asked for (M4/Codex): list every site that mutates a transient symbol
  after creation (`symbol_mut`, `set_declarations*`, `push_declaration`, `extend_declarations`, parent and value-declaration
  assignments) and classify whether it can reach an INSTANTIATED symbol (`merge.rs:330`, `structural.rs:5677`, `annotate.rs:5530-5568`
  are the known non-initialization cases). Where it can, either keep that symbol on today's path (allocate the full form) or
  implement the write explicitly on the new record; never forward a write to the origin. Debug-assert the rule.

## Gates the integrator runs on the pilot (same as phase A's, scaled)

`cargo test -p tsc-rs-checker` (1,891), `conformance_ts71.py --workers 1 --checkers 4` identical to main's 4-checker report,
`diffout.py` on the 8 projects (1 and 4 threads), Types/Symbols/Instantiations rows unchanged, `serial.py` instructions and peak
footprint on the 8 projects against the main binary of the same base, the shadow comparison of the speculation paths (existing
`flow-memo-shadow`-style strict verifier where applicable).

## What the pilot decides

- If the measured instruction delta of parts 1+2 is at or above the per-family forecast in the design's component ledger (to
  be filled from B1), phase A proceeds with the rest of the families on this mechanism.
- If it is well below, the ownership model is not the lever the design claims; the owner decides before any freeze.
Either way the pilot's code is the first phase-A PR candidate, not throwaway.
