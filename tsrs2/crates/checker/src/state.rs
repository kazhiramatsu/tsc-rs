//! CheckerState — the per-program checker context (M3 slice).
//!
//! Owns the binder run (symbol arena + tables), the TypeTables, the
//! links tables, and the signature arena. This is the seed of M4's
//! createTypeChecker; M3 uses it only through the relpin probe bridge.

use tsrs2_binder::{Binder, InternalSymbolName, SymbolId, SymbolTable};
use tsrs2_syntax::{NodeId, SourceFile};
use tsrs2_types::{
    CompilerOptions, ObjectFlags, SignatureFlags, SymbolFlags, TypeData, TypeFlags, TypeId,
    TypeTables,
};

use crate::links::{LinkSlot, LinksTables};
use crate::relate::RelationCaches;

/// A query the M3 slice cannot answer yet; carries the blocking
/// machinery's name so relpin failures read as scoping facts, not bugs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Unsupported {
    pub reason: String,
}

impl Unsupported {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

pub type CheckResult2<T> = Result<T, Unsupported>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SignatureId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MembersId(pub u32);

/// tsc Signature (core-interfaces §4) — the M3 subset: annotation-only
/// signatures, no type parameters, no instantiation target/mapper.
#[derive(Clone, Debug)]
pub struct Signature {
    pub declaration: NodeId,
    pub flags: SignatureFlags,
    /// Parameter symbols in declaration order, `this` excluded.
    pub parameters: Vec<SymbolId>,
    pub this_parameter: Option<SymbolId>,
    pub min_argument_count: u32,
    /// Lazy return type with tsc's resolving sentinel
    /// (getReturnTypeOfSignature 59810 / pushTypeResolution).
    pub resolved_return_type: LinkSlot<TypeId>,
    /// strictFunctionTypes variance keys on the DECLARATION kind
    /// (method bivariance — core-interfaces §4 from_method).
    pub from_method: bool,
}

/// tsc IndexInfo (createIndexInfo 59989).
#[derive(Clone, Debug)]
pub struct IndexInfo {
    pub key_type: TypeId,
    pub value_type: TypeId,
    pub is_readonly: bool,
    pub declaration: NodeId,
}

/// tsc resolved structured-type members (setStructuredTypeMembers
/// 50198): members table + named properties + signatures + index infos.
#[derive(Clone, Debug, Default)]
pub struct ResolvedMembers {
    pub members: SymbolTable,
    pub properties: Vec<SymbolId>,
    pub call_signatures: Vec<SignatureId>,
    pub construct_signatures: Vec<SignatureId>,
    pub index_infos: Vec<IndexInfo>,
}

pub struct CheckerState<'a> {
    pub binder: Binder<'a>,
    pub source: &'a SourceFile,
    pub options: &'a CompilerOptions,
    pub tables: TypeTables,
    /// tsc strictFunctionTypes via getStrictOptionValue.
    pub strict_function_types: bool,
    pub links: LinksTables,
    pub signatures: Vec<Signature>,
    pub members: Vec<ResolvedMembers>,
    /// checker-key §1.5: five per-relation caches + enumRelation.
    pub relations: RelationCaches,
    /// tsc subtypeReductionCache (47000), list-id keyed.
    pub subtype_reduction_cache: std::collections::HashMap<String, Vec<tsrs2_types::TypeId>>,
    /// greenfield §4.3: all links writes assert this is zero.
    pub speculation_depth: u32,
    /// createAnonymousType(undefined, emptySymbols, ...) (_tsc.js 47132).
    pub empty_object_type: TypeId,
    /// createAnonymousType(emptyTypeLiteralSymbol, ...) (47160).
    pub empty_type_literal_type: TypeId,
}

impl<'a> CheckerState<'a> {
    pub fn new(source: &'a SourceFile, binder: Binder<'a>, options: &'a CompilerOptions) -> Self {
        let strict_null_checks = options.strict_option_value(options.strict_null_checks);
        let strict_function_types = options.strict_option_value(options.strict_function_types);
        let exact_optional = options.exact_optional_property_types.unwrap_or(false);
        let tables = TypeTables::new(strict_null_checks, exact_optional);
        let mut state = Self {
            binder,
            source,
            options,
            tables,
            strict_function_types,
            links: LinksTables::default(),
            signatures: Vec::new(),
            members: Vec::new(),
            relations: RelationCaches::default(),
            subtype_reduction_cache: std::collections::HashMap::new(),
            speculation_depth: 0,
            empty_object_type: TypeId(0),
            empty_type_literal_type: TypeId(0),
        };
        // The empty anonymous types from the checker init block
        // (47132/47160): resolved-empty from birth.
        state.empty_object_type = state.create_resolved_empty_anonymous_type(None);
        let empty_type_literal_symbol = state.binder.symbols.alloc(
            SymbolFlags::TYPE_LITERAL,
            InternalSymbolName::TYPE.to_owned(),
        );
        state.empty_type_literal_type =
            state.create_resolved_empty_anonymous_type(Some(empty_type_literal_symbol));
        state
    }

    /// tsc-port: createAnonymousType @6.0.3
    /// tsc-hash: 801cde8bdea7de88d9052f5f01d296c15ec067902d478f857925edd1106efb93
    /// tsc-span: _tsc.js:50208-50210
    fn create_resolved_empty_anonymous_type(&mut self, symbol: Option<SymbolId>) -> TypeId {
        let id = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
        self.tables.type_mut(id).object_flags = ObjectFlags::ANONYMOUS;
        self.tables.type_mut(id).symbol = symbol;
        let members = self.alloc_members(ResolvedMembers::default());
        self.links
            .set_type_members(self.speculation_depth, id, LinkSlot::Resolved(members));
        id
    }

    pub fn alloc_members(&mut self, members: ResolvedMembers) -> MembersId {
        let id = MembersId(self.members.len() as u32);
        self.members.push(members);
        id
    }

    pub fn members_of(&self, id: MembersId) -> &ResolvedMembers {
        &self.members[id.0 as usize]
    }

    pub fn alloc_signature(&mut self, signature: Signature) -> SignatureId {
        let id = SignatureId(self.signatures.len() as u32);
        self.signatures.push(signature);
        id
    }

    pub fn signature_of(&self, id: SignatureId) -> &Signature {
        &self.signatures[id.0 as usize]
    }

    /// Empty member table shared by symbols that never had one.
    pub fn symbol_members(&self, symbol: SymbolId) -> &SymbolTable {
        &self.binder.symbols.symbol(symbol).members
    }

    /// File-scope name resolution for the relpin scratch program — the
    /// M3 slice of resolveEntityName: one flat scope (the source file's
    /// locals), meaning-filtered. Full lexical walking arrives with M4.
    pub fn resolve_file_scope_name(&self, name: &str, meaning: SymbolFlags) -> Option<SymbolId> {
        let locals = self.binder.locals.get(&self.source.root)?;
        let &symbol = locals.get(name)?;
        let flags = self.binder.symbols.symbol(symbol).flags;
        flags.intersects(meaning).then_some(symbol)
    }

    pub fn symbol_flags(&self, symbol: SymbolId) -> SymbolFlags {
        self.binder.symbols.symbol(symbol).flags
    }

    pub fn node_symbol(&self, node: NodeId) -> Option<SymbolId> {
        self.binder.node_symbol.get(&node).copied()
    }

    /// The binder's mutable node-flags view (ContainsThis etc.).
    pub fn node_flags(&self, node: NodeId) -> i32 {
        self.binder.flags_of(node).bits()
    }
}
