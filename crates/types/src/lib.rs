#![forbid(unsafe_code)]

pub mod flags;
pub mod options;
pub mod tables;
pub mod ty;
mod version;

pub use flags::*;
pub use options::{CompilerOptionNumber, CompilerOptions, ModuleSuffix};
pub use tables::{
    js_number_to_string, IntersectionFlags, Intrinsics, TupleTargetFlags, TypeTables,
    UnionReduction,
};
pub use ty::{
    ConditionalRootData, ConditionalRootId, ConditionalTypeData, LiteralValue, MappedTypeData,
    MappedTypeModifiers, MapperId, PseudoBigInt, ReverseMappedTypeData, SubstitutionTypeData,
    SymbolId, TemplateText, TupleTargetData, Type, TypeData, TypeId,
};
pub use version::compiler_version_satisfies;

pub fn is_scaffolded() -> bool {
    true
}
