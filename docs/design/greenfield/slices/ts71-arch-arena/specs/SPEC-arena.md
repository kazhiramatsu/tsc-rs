# SPEC-arena — `crates/arena`, the one `unsafe` crate of the arena model (phase 0.1 + 0.2)

Revision 2 (2026-11-07, after the Opus review B1/M8): 16 KiB blocks, reuse-after-drop policy, `Ref: Send + Sync` iff `T: Sync`,
`Owned<T>` escape hatch, no `contiguous` arena, no parser `mark/reset`, dense ids in records for ordering.

Track: [ts71-arch-arena](../README.md) (owner approved 2026-11-07). This lane builds the memory substrate every later phase
stands on. It migrates **no consumer**: the existing `crates/store` and all its users stay as they are until phase 1.

## Lane rules (mandatory; the integrator verifies every claim independently)

- Work only in this clone. The sandbox cannot commit: after each milestone save a numbered patch
  `target/lane-arena/patches/NN-<topic>.patch` (`git diff <base-commit>`), and keep `STATUS.md` at the clone root current
  (done / verified with the command and its output / open / blocked). The integrator commits from outside.
- Load: every cargo command is `nice -n 20 cargo … -j 2` (the `setpriority` warning is harmless), tests run with
  `-- --test-threads=1`, one cargo command at a time. Build and test **only** `-p tsc-rs-arena`; never the workspace, never a
  release build of the CLI, no benchmarks. A process that gets no CPU for a while may have been paused by the integrator: wait.
- No network. Changes only under `crates/arena/`, plus the one `members` line in the root `Cargo.toml` and `Cargo.lock`.
- Every claim needs a runnable demonstration (a test, or a command and its quoted output in `STATUS.md`). No performance claims.
- Time box: two working days. If blocked or at the box, stop and write `STATUS.md`.

## What to read first

`crates/store/src/lib.rs` (the reservation, `get`, zero-on-drop, the contract text in the module doc), `claimed.rs`, `frozen.rs`,
`recycle.rs` (primitives landed without consumers: reuse their mmap/zeroing/mark code where it fits), `crates/types/src/id.rs`
(`id_type!`: ids are already `NonZeroU32` with `new(index) = index + 1`), the design document §0 and §2 (段 0.1 / 0.2).

## Deliverable 1: the crate

`crates/arena`, package `tsc-rs-arena`, lib name `tsc_arena`, `#![deny(unsafe_op_in_unsafe_fn)]`, no dependencies beyond
`libc` (unix) and the workspace's existing `libmimalloc-sys` if you need its stats (optional). Everything below is the contract;
names may be adjusted only if `STATUS.md` says why.

### Reservation and blocks

- `Reservation`: one process-wide range, created lazily on first use. Native 64-bit: `mmap(PROT_NONE, MAP_PRIVATE|MAP_ANON)` of
  `RESERVE_BYTES = 32 GiB` — the whole space a `u32` handle at 8-byte granularity can name (a `const`; a build-time `cfg` may lower it
  for CI machines, no runtime flag). Pages are committed by `mprotect(PROT_READ|PROT_WRITE)` per block on first use. wasm32: no
  reservation; a block is `std::alloc::alloc_zeroed`, and handles are byte addresses / 8 (wasm32 addresses are below 4 GiB). Keep both
  arms compiling; the wasm32 arm may be untested if the target is not installed (say so in `STATUS.md`).
- Handle unit: 8 bytes. `raw: u32` → address `base + (raw as usize) << 3`. Handle 0 is never issued.
- `BlockAllocator`: blocks of `BLOCK_BYTES = 16 KiB` (the page size of this machine; small files then waste at most one page),
  issued from a free list first and by an atomic bump otherwise. `block_owner`: one `AtomicU32` per block (`32 GiB / 16 KiB =
  2,097,152` entries, 8 MiB, allocated with `mmap` so untouched entries cost nothing) holding `owner + 1` (0 = unissued,
  `u32::MAX` = free). `pub fn owner_of(raw: u32) -> Option<u32>` is one load: `block_owner[(raw as usize) >> 11]`.
- **Release and reuse policy (review B1, decided 2026-11-07).** When an arena is dropped (a `FrozenArena`'s last `Arc`, a
  `LocalArena`), its blocks are zeroed (`madvise(MADV_FREE)` for whole blocks; the kernel supplies zero pages on next touch — verify
  on macOS that a `MADV_FREE`d page reads as zero after reuse, otherwise zero explicitly) and returned to the free list for any
  later owner. This is today's `crates/store` contract (ranges are reused after their lease drops; "a stale identity reads an
  empty record rather than a dangling one"): a stale `Ref` reads zeros, or a live record of some other owner. Every `Record` is
  valid for every bit pattern (see below), the per-kind decoders match exhaustively on `kind`, so a stale read is a logic error,
  never undefined behaviour. The holders of `Ref`s are the same holders as today (a Program's documents, a checker's state), and
  they drop with their arenas. Document this in the module doc; debug builds additionally assert in `Ref::get` that the block
  is issued (`owner_of(raw).is_some()`).
- Feature `arena-shuffle` (off by default; test builds only): `BlockAllocator` issues blocks in a seeded pseudo-random order
  (`TSRS_ARENA_SHUFFLE_SEED`, read once; a test-harness knob, not a compiler mode). The integrator's determinism job compares
  outputs across two seeds.

### Records and handles

```rust
/// A record stored in an arena: `#[repr(C)]`, plain old data, EVERY byte pattern a valid value (not only zeros: a stale handle may
/// read another owner's record), no `Drop`, no references, alignment ≤ 8.
pub unsafe trait Record: Sized + 'static {}
pub struct Ref<T: Record>(NonZeroU32, PhantomData<&'static T>);   // Copy, Eq, Ord (by raw), Hash; Option<Ref<T>> is 4 bytes (assert)
// `PhantomData<&'static T>` makes `Ref<T>: Send + Sync` exactly when `T: Sync`: a handle to a record with cells cannot leave its thread.
impl<T: Record> Ref<T> {
    pub fn raw(self) -> u32; pub fn from_raw(raw: u32) -> Option<Self>;
    pub fn get(self) -> &'static T;    // see "lifetime contract"
}
pub struct Span<T: Record> { start: Ref<T>, len: u32 }   // 8 bytes; `as_slice(self) -> &'static [T]`; empty span = len 0
pub struct ListHeader { len: u32, extra: u32 }            // a length-prefixed list: `Ref<List<T>>` reads the header then `len` items
pub struct List<T: Record>(PhantomData<T>);              // `Ref<List<T>>::items() -> &'static [T]`, `::header() -> &ListHeader`
pub struct Bytes;                                        // `Ref<Bytes>`: len-prefixed byte string (for literal texts); `as_bytes()`
```

- `Ref::get` is `#[inline(always)]`, one load after the shift-add. Debug builds assert `owner_of(raw).is_some()` and not
  released.
- **Lifetime contract** (module doc; the integrator reviews the wording): a `Ref` is dereferenced only by a holder of its arena
  (an `Arc<FrozenArena>` for shared arenas; the owning thread for local arenas). Records never move and are never freed
  individually; blocks are zeroed and reused only after the whole arena drops (policy above). Same contract as today's store.
- **Thread contract**: `Ref<T>` is `Send + Sync` iff `T: Sync`. Records of local arenas (checker, scratch, emit) hold
  `ArenaCell`s and are therefore `!Sync`, so their handles are `!Send`. A checker is still moved between threads as a whole
  (the checker pool): provide `pub struct Owned<T>(T)` with `unsafe impl<T> Send for Owned<T>` and the documented contract
  "exclusive access, one thread at a time; the wrapped value's handles are used only through this exclusive access" — this is
  the one audited escape hatch, and the consumer crates stay `forbid(unsafe_code)`. `ArenaCell::get/set` debug-assert that the
  current thread is the arena's recorded owner thread (`LocalArena` records `thread::current().id()` at creation and on
  `Owned` re-entry through `LocalArena::adopt_current_thread()`).

### Arenas

Two arena types, distinguished by what they may hold:

```rust
/// Written by one thread, then frozen and shared read-only (file documents).
pub struct SharedArena { … }      // Send, !Sync while unfrozen
impl SharedArena {
    pub fn new(owner: u32) -> Self;
    pub fn alloc<T: Record + Sync>(&self, value: T) -> Ref<T>;             // `&self`: the bump pointer is a Cell
    pub fn alloc_slice<T: Record + Sync>(&self, values: &[T]) -> Span<T>;
    pub fn alloc_list<T: Record + Sync>(&self, values: &[T]) -> Ref<List<T>>;
    pub fn alloc_bytes(&self, bytes: &[u8]) -> Ref<Bytes>;
    pub fn get_mut<T: Record>(&mut self, r: Ref<T>) -> &mut T;             // exclusive writes before freezing (parser/binder)
    pub fn freeze(self) -> FrozenArena;                                    // Send + Sync
    pub fn used_bytes(&self) -> usize; pub fn committed_bytes(&self) -> usize;
}
pub struct FrozenArena { … }     // read-only; `release(self)` (MADV_FREE + mark released); stats
/// Owned and written by one thread for its whole life (checker, scratch, emit).
pub struct LocalArena { … }       // Send, !Sync
impl LocalArena {
    pub fn new(owner: u32) -> Self;
    pub fn alloc<T: Record>(&self, value: T) -> Ref<T>;             // records may contain ArenaCell
    pub fn alloc_slice / alloc_list / alloc_bytes as above;
    pub fn adopt_current_thread(&self);                              // after an `Owned` move (debug bookkeeping only)
}
// Dropping a `LocalArena` or the last `Arc<FrozenArena>` releases its blocks (policy above); no explicit `release()`.
pub struct ArenaCell<T: Copy>(UnsafeCell<T>);   // get/set through &self; !Sync, so a record holding one cannot enter a SharedArena
```

- The `Record + Sync` bound on `SharedArena::alloc` is the type-level proof that shared (file) records hold no cells; the
  binder writes them through `get_mut` before `freeze`. The `!Sync` of `ArenaCell` is what enforces it. Add a compile-fail test
  (`trybuild`) that a record with an `ArenaCell` field is rejected by `SharedArena::alloc`.
- No `contiguous` arena and no `mark/reset` on `SharedArena` (review B1/M8, 2026-11-07): ordering on the trust path uses dense
  per-checker ids stored in the records (the Type header), never addresses; the parser keeps failed-speculation nodes as tsgo does.

### Scratch

```rust
pub struct ScratchArena { … }                       // one per checker thread; !Sync
impl ScratchArena { pub fn scope(&mut self) -> ScratchScope<'_>; }
pub struct ScratchScope<'s> { … }                   // resets the bump pointer on drop
impl<'s> ScratchScope<'s> {
    pub fn alloc<T: Copy>(&self, value: T) -> &'s mut T;         // hmm: return `&'s mut T` from `&self` is unsound; use `&'s ScratchCell<T>` or return by `&mut self` — decide and document
    pub fn vec<T: Copy>(&self) -> ScratchVec<'s, T>;            // push/extend/as_slice/clear, grows by moving to a larger scratch block
    pub fn child(&self) -> ScratchScope<'_>;                     // nested scopes, stack discipline
}
```
The point of `'s`: a value allocated in a scope cannot be stored in a `Record` (no `'static`), so it cannot escape into a
permanent arena without an explicit copy. Add a compile-fail test that storing a `ScratchVec` slice into a struct that outlives
the scope is rejected. Resolve the `alloc` signature question above in favor of soundness and say which you chose.

### Memory report

`pub fn report() -> Vec<ArenaStats { owner: u32, kind: &'static str, used: usize, committed: usize, released: bool }>` over all
live arenas (a global registry guarded by a mutex, updated on create/freeze/release). The integrator wires it into
`TSRS_MEMORY_REPORT` in phase 1.

## Deliverable 2: tests (all under `crates/arena/tests/` and unit tests)

1. Round trips: `alloc`/`get` for a few record shapes (8, 24, 40 B), `alloc_slice`/`alloc_list`/`alloc_bytes`; alignment of every
   returned address is 8.
2. `size_of::<Option<Ref<T>>>() == 4`, `size_of::<Span<T>>() == 8` (const asserts).
3. `owner_of` for handles in the first, a middle and the last block of an arena; a released arena's handle reports released.
4. Drop and reuse: dropping an arena returns its blocks; a new arena receives one of them and reads zeros before writing; a stale
   `Ref` into the dropped arena reads a valid (zero or other-owner) record in release and panics in debug when the block is free.
5. `size_of::<Ref<T>>() == 4`; `Ref<SharedRecord>: Send + Sync` and `Ref<CellRecord>: !Send` (static assertions with a helper
   trait); `Owned<LocalThing>: Send`.
6. `SharedArena::freeze` → `Arc<FrozenArena>` read from 4 threads concurrently (handles produced before the freeze).
7. `arena-shuffle`: with two seeds the same allocation sequence yields different raw handles; `owner_of` stays correct.
8. Compile-fail (`trybuild` as a dev-dependency): (a) `ArenaCell` record into `SharedArena::alloc`; (b) a `Drop` type as a
   `Record` (the trait is unsafe to implement; provide a `#[macro]` or doc that forbids it, and a test that a `String` field does
   not compile through the provided derive/macro if you add one); (c) a scratch value escaping its scope; (d) sending a
   `Ref<CellRecord>` to another thread.
9. `cargo clippy -p tsc-rs-arena -- -D warnings` clean; `cargo doc -p tsc-rs-arena` without warnings.

## Out of scope (do not do)

Migrating any consumer; touching `crates/store`, `crates/syntax`, `crates/binder`, `crates/types`; benchmarks; a recycler of
records inside a live arena; page-return heuristics beyond the drop policy; LSP/incremental ownership; any runtime mode flag.

## Acceptance (integrator)

`nice -n 20 cargo test -p tsc-rs-arena -j 2 -- --test-threads=1` green, clippy and doc clean, the compile-fail tests present and
passing, `STATUS.md` with the commands and outputs, patches numbered. The integrator then reviews the lifetime contract wording
and the `ArenaCell`/`SharedArena` bound before phase 1 depends on them.
