//! Stable-address records with compact eight-byte-unit handles.
//!
//! # Lifetime and publication contract
//!
//! This crate deliberately exposes the legacy store's caller-managed lifetime
//! contract. A handle is dereferenced only by a holder of its arena: an
//! `Arc<FrozenArena>` for shared records, or the owning thread's `LocalArena`.
//! The holder keeps the arena alive throughout **every use of the returned
//! reference**, not just the call to `get`. Publish shared records through an
//! Arc only after freezing. Records never move and are not freed individually.
//! A mutable view requires that all overlapping shared views have ended.
//!
//! Dropping the whole arena zeros its blocks and returns them for reuse by any
//! owner. A stale scalar read can observe zeros or another owner's bytes, all
//! valid record bit patterns. Exhaustive kind decoding must handle those values.
//! Debug reads additionally reject free/unissued blocks. This diagnostic is
//! **not** a liveness or provenance proof: a reused block can have a new owner.
//! Stale reads are logic errors and are outside the lifetime contract. In
//! particular, all-bit-pattern validity does not legalize concurrent stale
//! accesses, retained references across reuse, or stale list lengths.
//! Records must have no padding, including inside fields, so typed writes
//! initialize every stored byte. This does not relax the holder contract.
//!
//! # Limits of the legacy safe facade
//!
//! The specified safe `get`/`get_mut`/`from_raw` API cannot enforce these rules
//! in Rust's type system. Forging handles, keeping views across mutation/drop,
//! or bypassing publication can cause undefined behavior. This is not a sound
//! general-purpose safe Rust allocator. Consumer integration requires auditing
//! the holder discipline above; the unsafe code stays isolated in this crate.
//!
//! # Threads
//!
//! `Ref<T>` is Send + Sync exactly when `T: Sync`. `ArenaCell` is not Sync, so
//! cell-bearing handles stay on their owner thread. [`Owned`] is the audited
//! escape hatch for moving an exclusively owned checker, including its handles.
//! Call [`LocalArena::adopt_current_thread`] on re-entry before using its cells.
//! Address ordering never represents creation ordering; store dense ids in records.

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(any(all(unix, target_pointer_width = "64"), target_arch = "wasm32")))]
compile_error!("arena supports 64-bit Unix and wasm32");
#[cfg(all(feature = "arena-shuffle", not(debug_assertions)))]
compile_error!("arena-shuffle is debug/test-harness instrumentation only");

mod arenas;
mod handles;
mod memory;
mod scratch;

pub use arenas::{report, ArenaCell, ArenaStats, FrozenArena, LocalArena, Owned, SharedArena};
pub use handles::{Bytes, List, ListHeader, Ref, Span};
pub use memory::{owner_of, BlockAllocator, Reservation, BLOCK_BYTES, RESERVE_BYTES};
pub use scratch::{ScratchArena, ScratchCell, ScratchScope, ScratchVec};

/// Plain record bytes, with no destructor, references, or invalid bit patterns.
///
/// # Safety
///
/// Implementors must have stable C/transparent layout, alignment at most eight,
/// no Drop glue or padding (including inside fields), and accept **every** byte
/// pattern. Every byte of a value must be initialized. They must support bitwise
/// copying, including from a shared borrow. Fields containing ownership,
/// references, bools, enums with invalid discriminants, or nonzero integers are
/// forbidden. `Option<Ref<T>>` is allowed; bare `Ref<T>` is not (zero is invalid).
/// Records containing cells must not claim Sync. Use [`record!`] to define
/// checked field compositions without unsafe code in consumer crates.
pub unsafe trait Record: Sized + 'static {}

macro_rules! primitive_records {
    ($($t:ty),* $(,)?) => { $(
        // SAFETY: these scalars accept every bit pattern and have no Drop glue.
        unsafe impl Record for $t {}
    )* };
}
primitive_records!(
    u8,
    i8,
    u16,
    i16,
    u32,
    i32,
    u64,
    i64,
    usize,
    isize,
    f32,
    f64,
    ()
);
// SAFETY: arrays preserve element validity and layout, without Drop glue.
unsafe impl<T: Record, const N: usize> Record for [T; N] {}
// SAFETY: a phantom marker has no bytes, validity constraints or Drop glue.
unsafe impl<T: 'static> Record for std::marker::PhantomData<T> {}

/// Define a C-layout record whose fields already satisfy [`Record`].
///
/// ```
/// tsc_arena::record! {
///     #[derive(Clone, Copy)]
///     pub struct Header { pub kind: u32, pub dense_id: u32 }
/// }
/// ```
///
/// String/Drop fields fail the field bound. Adding a custom Drop implementation
/// also fails the compile-time `needs_drop` assertion. Interior and trailing
/// padding fail the size-versus-field-sum assertion; use explicit integer spare
/// fields where needed. Manual unsafe Record
/// implementations remain an audited responsibility.
#[macro_export]
macro_rules! record {
    ($(#[$attr:meta])* $vis:vis struct $name:ident { $($(#[$fa:meta])* $fv:vis $field:ident: $ty:ty),* $(,)? }) => {
        $(#[$attr])*
        #[repr(C)]
        $vis struct $name { $($(#[$fa])* $fv $field: $ty),* }
        const _: () = {
            fn check<T: $crate::Record>() {}
            fn fields() { $(check::<$ty>();)* }
            let _ = fields;
            assert!(!::std::mem::needs_drop::<$name>(), "Record must not have Drop glue");
            assert!(::std::mem::align_of::<$name>() <= 8, "Record alignment exceeds eight");
            assert!(::std::mem::size_of::<$name>() == 0usize $(+ ::std::mem::size_of::<$ty>())*,
                "Record must not have padding");
        };
        // SAFETY: Record fields, C layout and the assertions above establish
        // all-bit-pattern validity, layout, duplication, no padding and no Drop.
        unsafe impl $crate::Record for $name {}
    };
}
