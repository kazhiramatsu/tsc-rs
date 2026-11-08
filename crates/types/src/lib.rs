#![forbid(unsafe_code)]

pub mod escaped_name;
pub mod flags;
pub mod goslices;
mod id;
pub mod identity;
pub mod names;
pub mod options;
pub mod perf;
pub mod tables;
pub mod trace;
pub mod tracing;
pub mod ty;
pub mod type_order;
mod version;

pub use escaped_name::EscapedName;
pub use flags::*;
pub use identity::{
    IdentityClaim, IdentityDomain, IdentityDomainStats, IdentityError, IdentityLease,
    IdentityLeaseWatch, IdentityLimits, IdentityRange, IdentitySpace, IdentitySpaceStats,
    TRANSIENT_SYMBOL_BIT,
};
pub use options::{CompilerOptionNumber, CompilerOptions, ModuleSuffix};
pub use tables::{
    js_number_to_string, push_decimal, AliasKey, IndexedAccessKey, InstantiationKey,
    IntersectionAlias, IntersectionFlags, IntersectionKey, Intrinsics, ListCursor,
    TemplateLiteralTooLarge, TupleTargetFlags, TypeListCopy, TypeListId, TypeTables, TypesMemory,
    UnionKey, UnionOfUnionKey, UnionOriginKey, UnionReduction,
};
pub use tsc_diagnostics::{JsStr, JsString};
pub use ty::{
    AliasId, BigIntId, ConditionalDataId, ConditionalRootData, ConditionalRootId,
    ConditionalTypeData, IntrinsicData, IntrinsicId, LiteralData, LiteralTextId, LiteralValue,
    LiteralValueOwned, MappedTypeData, MappedTypeModifiers, MapperId, NumberBits, PseudoBigInt,
    ReverseMappedTypeData, SubstitutionTypeData, SymbolId, TemplateLiteralData, TemplateLiteralId,
    TemplateText, TupleTargetData, TupleTargetId, Type, TypeAlias, TypeData, TypeId,
};
pub use type_order::{sort_func_like_go, TypeOrder, TypeOrderContext};
pub use version::{
    compiler_version_satisfies, Version, TYPESCRIPT_VERSION, TYPESCRIPT_VERSION_MAJOR_MINOR,
};

#[cfg(feature = "phase-ledger")]
pub mod phase_ledger;
