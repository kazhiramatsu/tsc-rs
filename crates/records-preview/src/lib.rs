//! Phase 0.3 isolated layout proof; no live compiler consumers.
#![deny(unsafe_op_in_unsafe_fn)]
pub mod arena_stub;
pub mod nodes;
pub mod records;
pub mod support;
pub mod syntax_kind;
pub mod tables;
pub use tsc_types::{NodeFlags, ObjectFlags, SignatureFlags, SymbolFlags, TypeFlags};

/// Shared records cannot hold checker cells.
/// ```compile_fail
/// use tsc_records_preview::{arena_stub::SharedArena, records::NodeLinks};
/// SharedArena::new(0).alloc(NodeLinks::default());
/// ```
/// Type header handles remain local even though their common words are immutable.
/// ```compile_fail
/// use tsc_records_preview::{arena_stub::Ref, records::Type};
/// fn require_send<T: Send>() {}
/// require_send::<Ref<Type>>();
/// ```
/// Tagged mapper payloads can retain local types even though their words are u32.
/// ```compile_fail
/// use tsc_records_preview::{arena_stub::Ref, records::Mapper};
/// fn require_send<T: Send>() {}
/// require_send::<Ref<Mapper>>();
/// ```
pub struct ThreadContract;
