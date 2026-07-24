#![forbid(unsafe_code)]

pub mod flags;
pub mod options;
pub mod tables;
pub mod ty;

pub use flags::*;
pub use options::CompilerOptions;
pub use tables::{
    js_number_to_string, IntersectionFlags, Intrinsics, TupleTargetFlags, TypeTables,
    UnionReduction,
};
pub use ty::{
    LiteralValue, MappedTypeData, MappedTypeModifiers, MapperId, PseudoBigInt, SymbolId,
    TemplateText, TupleTargetData, Type, TypeData, TypeId,
};

pub fn is_scaffolded() -> bool {
    true
}
