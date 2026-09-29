#![forbid(unsafe_code)]

pub mod escaped_name;
pub mod flags;
pub mod identity;
pub mod options;
pub mod perf;
pub mod tables;
pub mod trace;
pub mod ty;
mod version;

pub use escaped_name::EscapedName;
pub use flags::*;
pub use identity::{
    IdentityAllocationPolicy, IdentityDomain, IdentityDomainStats, IdentityError, IdentityLease,
    IdentityLimits, IdentityRange, IdentityReservation, IdentitySpace, IdentitySpaceStats,
    TRANSIENT_SYMBOL_BIT,
};
pub use options::{CompilerOptionNumber, CompilerOptions, ModuleSuffix};
pub use tables::{
    js_number_to_string, InstantiationKey, IntersectionFlags, Intrinsics, TupleTargetFlags,
    TypeListId, TypeTables, TypesMemory, UnionReduction,
};
pub use tsc_diagnostics::{JsStr, JsString};
pub use ty::{
    ConditionalRootData, ConditionalRootId, ConditionalTypeData, LiteralValue, MappedTypeData,
    MappedTypeModifiers, MapperId, PseudoBigInt, ReverseMappedTypeData, SubstitutionTypeData,
    SymbolId, TemplateText, TupleTargetData, Type, TypeData, TypeId,
};
pub use version::compiler_version_satisfies;
