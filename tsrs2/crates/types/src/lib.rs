#![forbid(unsafe_code)]

pub mod flags;
pub mod options;
pub mod tables;
pub mod ty;

pub use flags::*;
pub use options::CompilerOptions;
pub use tables::{Intrinsics, M4Dependency, TypeTables, UnionReduction};
pub use ty::{LiteralValue, PseudoBigInt, SymbolId, TupleTargetData, Type, TypeData, TypeId};

pub fn is_scaffolded() -> bool {
    true
}
