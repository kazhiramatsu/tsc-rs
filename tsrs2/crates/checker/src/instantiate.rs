//! Type instantiation (M4 5.2) — instantiateType + the TypeMapper
//! machinery (checker-foundations §6).
//!
//! TypeMapper is the CLOSED enum of tsc's six TypeMapKind shapes
//! (flags.rs TypeMapKind) — never a HashMap. Mappers are arena-
//! allocated on CheckerState so MapperId equality IS tsc's mapper
//! object identity (findActiveMapper 73616 compares `===`); every
//! make* call allocates a fresh id exactly like tsc object creation.
//!
//! StringMapping goes live here too: getStringMappingType + the
//! generic-operand interning (tables) — `Uppercase<T>` reaches it via
//! getTypeAliasInstantiation (5.2 follow-up); relation arms flipped in
//! structural.rs/unions.rs pin at 5.3b.

use tsrs2_binder::SymbolId;
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{for_each_child, NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    CheckFlags, IntersectionFlags, ObjectFlags, SignatureFlags, SymbolFlags, TypeData, TypeFlags,
    TypeId, UnionReduction,
};

use crate::links::LinkSlot;
use crate::state::{CheckResult2, CheckerState, Signature, SignatureId, Unsupported};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MapperId(pub u32);

/// tsc-port: makeDeferredTypeMapper @6.0.3
/// tsc-hash: 11cf19c83b088bf7ae4ff74f09b4c0c2aae1d17efdd20ebee18049a409183c7c
/// tsc-span: _tsc.js:63368-63370
///
/// Deferred mapper thunks — every tsc constructor site is inference-
/// context machinery (makeFixingMapperForContext 68259 /
/// makeNonFixingMapperForContext 68272, M6) or infer-type constraint
/// resolution (getInferredTypeParameterConstraint 60074, M8). The enum
/// is deliberately UNINHABITED until an owner lands: the Deferred
/// mapper kind exists in the closed TypeMapper enum but cannot be
/// constructed.
#[derive(Clone, Copy, Debug)]
pub enum DeferredMapperTargets {}

/// tsc-port: makeFunctionTypeMapper @6.0.3
/// tsc-hash: 8b8b3a8e91724e911f8633efe97f52806c560c58307f03995886eb93b37185fb
/// tsc-span: _tsc.js:63365-63367
///
/// tsc's function mappers are a closed set of checker singletons; the
/// enum names them instead of storing closures. The inference fixing
/// mappers land with M6.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FunctionMapper {
    /// 47104: `t => t.flags & TypeParameter ? wildcardType : t`.
    Permissive,
    /// 47103: `t => t.flags & TypeParameter ?
    /// getRestrictiveTypeParameter(t) : t`.
    Restrictive,
    /// 47112: `t => t.flags & TypeParameter ? uniqueLiteralType : t`
    /// (isReducibleIntersection's probe mapper).
    UniqueLiteral,
    /// 47123-47131: fires the out-of-band variance handler with
    /// onlyUnreliable=false when t is one of the three marker type
    /// parameters; identity otherwise (M4 5.3b).
    ReportsUnmeasurable,
    /// 47114-47122: fires the handler with onlyUnreliable=true on the
    /// markers; identity otherwise.
    ReportsUnreliable,
}

/// tsc TypeMapper — the six TypeMapKind shapes.
#[derive(Clone, Debug)]
pub enum TypeMapper {
    /// tsc-port: makeUnaryTypeMapper @6.0.3
    /// tsc-hash: f16e43be81b2a0ab46054c6a62c222608c47514ebef557e8fdbe5fc2b022cb63
    /// tsc-span: _tsc.js:63359-63361
    Simple {
        source: TypeId,
        target: TypeId,
    },
    /// tsc-port: makeArrayTypeMapper @6.0.3
    /// tsc-hash: e011653188a06047cc1bd6668dc7b584e0756d94eb7e8a572b0cc9fe88695602
    /// tsc-span: _tsc.js:63362-63364
    ///
    /// `targets: None` is the type-eraser form (targets → anyType).
    Array {
        sources: Vec<TypeId>,
        targets: Option<Vec<TypeId>>,
    },
    Deferred(DeferredMapperTargets),
    Function(FunctionMapper),
    /// tsc-port: makeCompositeTypeMapper @6.0.3
    /// tsc-hash: c6357cfb91719fe9cb439c4d6b9743da0095e3f28e047b98a4e46654c02c3196
    /// tsc-span: _tsc.js:63371-63373
    Composite {
        mapper1: MapperId,
        mapper2: MapperId,
    },
    Merged {
        mapper1: MapperId,
        mapper2: MapperId,
    },
}

/// tsc intrinsicTypeKinds (46408-46414).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IntrinsicTypeKind {
    Uppercase,
    Lowercase,
    Capitalize,
    Uncapitalize,
    NoInfer,
}

pub(crate) fn intrinsic_type_kind(name: &str) -> Option<IntrinsicTypeKind> {
    match name {
        "Uppercase" => Some(IntrinsicTypeKind::Uppercase),
        "Lowercase" => Some(IntrinsicTypeKind::Lowercase),
        "Capitalize" => Some(IntrinsicTypeKind::Capitalize),
        "Uncapitalize" => Some(IntrinsicTypeKind::Uncapitalize),
        "NoInfer" => Some(IntrinsicTypeKind::NoInfer),
        _ => None,
    }
}

/// JS `str.toUpperCase()` — Unicode Default Case Conversion; Rust's
/// str::to_uppercase implements the same tables (including one-to-many
/// expansions like ß→SS).
fn js_to_upper_case(s: &str) -> String {
    s.to_uppercase()
}

/// JS `str.toLowerCase()` — including the Final_Sigma condition, which
/// Rust's str::to_lowercase also implements.
fn js_to_lower_case(s: &str) -> String {
    s.to_lowercase()
}

/// JS `str.charAt(0).toUpperCase() + str.slice(1)`: charAt(0) is one
/// UTF-16 CODE UNIT, so an astral first character contributes a lone
/// surrogate whose case conversion is the identity — Capitalize/
/// Uncapitalize are no-ops on astral-initial strings.
fn js_capitalize(s: &str, upper: bool) -> String {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    if (first as u32) > 0xFFFF {
        return s.to_owned();
    }
    let mapped = if upper {
        first.to_uppercase().to_string()
    } else {
        first.to_lowercase().to_string()
    };
    format!("{mapped}{}", chars.as_str())
}

impl<'a> CheckerState<'a> {
    pub(crate) fn mapper(&self, id: MapperId) -> &TypeMapper {
        &self.mappers[id.0 as usize]
    }

    pub(crate) fn alloc_mapper(&mut self, mapper: TypeMapper) -> MapperId {
        let id = MapperId(self.mappers.len() as u32);
        self.mappers.push(mapper);
        id
    }

    pub fn make_unary_type_mapper(&mut self, source: TypeId, target: TypeId) -> MapperId {
        self.alloc_mapper(TypeMapper::Simple { source, target })
    }

    pub fn make_array_type_mapper(
        &mut self,
        sources: Vec<TypeId>,
        targets: Option<Vec<TypeId>>,
    ) -> MapperId {
        self.alloc_mapper(TypeMapper::Array { sources, targets })
    }

    fn make_composite_type_mapper(
        &mut self,
        merged: bool,
        mapper1: MapperId,
        mapper2: MapperId,
    ) -> MapperId {
        self.alloc_mapper(if merged {
            TypeMapper::Merged { mapper1, mapper2 }
        } else {
            TypeMapper::Composite { mapper1, mapper2 }
        })
    }

    /// tsc-port: createTypeMapper @6.0.3
    /// tsc-hash: 8e994c2bdc2b579611b4c91889ed540cb31e94e362240a01f2a3bbdd9c9bcc06
    /// tsc-span: _tsc.js:63324-63326
    pub fn create_type_mapper(
        &mut self,
        sources: Vec<TypeId>,
        targets: Option<Vec<TypeId>>,
    ) -> MapperId {
        if sources.len() == 1 {
            let target = match &targets {
                Some(targets) => targets[0],
                None => self.tables.intrinsics.any,
            };
            self.make_unary_type_mapper(sources[0], target)
        } else {
            self.make_array_type_mapper(sources, targets)
        }
    }

    /// tsc-port: createTypeEraser @6.0.3
    /// tsc-hash: 014992554244c1b0a4881215b101a857c3e6aa9b7b7a79832eff6a1f6935da8d
    /// tsc-span: _tsc.js:63374-63380
    pub fn create_type_eraser(&mut self, sources: Vec<TypeId>) -> MapperId {
        self.create_type_mapper(sources, None)
    }

    /// tsc-port: getMappedType @6.0.3
    /// tsc-hash: 1de145bdc6a4fc936f2d547c707305081eede73870da72023bc151fa83e65252
    /// tsc-span: _tsc.js:63327-63358
    pub fn get_mapped_type(&mut self, ty: TypeId, mapper: MapperId) -> CheckResult2<TypeId> {
        match self.mapper(mapper).clone() {
            TypeMapper::Simple { source, target } => Ok(if ty == source { target } else { ty }),
            TypeMapper::Array { sources, targets } => {
                for (i, &source) in sources.iter().enumerate() {
                    if ty == source {
                        return Ok(match &targets {
                            Some(targets) => targets[i],
                            None => self.tables.intrinsics.any,
                        });
                    }
                }
                Ok(ty)
            }
            TypeMapper::Deferred(targets) => match targets {},
            TypeMapper::Function(kind) => {
                if self
                    .tables
                    .flags_of(ty)
                    .intersects(TypeFlags::TYPE_PARAMETER)
                {
                    Ok(match kind {
                        FunctionMapper::Permissive => self.tables.intrinsics.wildcard,
                        FunctionMapper::Restrictive => self.get_restrictive_type_parameter(ty),
                        FunctionMapper::UniqueLiteral => self.tables.intrinsics.unique_literal,
                        FunctionMapper::ReportsUnmeasurable => {
                            self.fire_variance_marker_if_marker(ty, /*only_unreliable*/ false);
                            ty
                        }
                        FunctionMapper::ReportsUnreliable => {
                            self.fire_variance_marker_if_marker(ty, /*only_unreliable*/ true);
                            ty
                        }
                    })
                } else {
                    Ok(ty)
                }
            }
            TypeMapper::Composite { mapper1, mapper2 } => {
                let t1 = self.get_mapped_type(ty, mapper1)?;
                if t1 != ty {
                    self.instantiate_type(t1, Some(mapper2))
                } else {
                    self.get_mapped_type(t1, mapper2)
                }
            }
            TypeMapper::Merged { mapper1, mapper2 } => {
                let t1 = self.get_mapped_type(ty, mapper1)?;
                self.get_mapped_type(t1, mapper2)
            }
        }
    }

    /// tsc-port: combineTypeMappers @6.0.3
    /// tsc-hash: afd8901717a4640c86d0d1d4c35109c36299437179ae750f488332fbf369d447
    /// tsc-span: _tsc.js:63388-63390
    pub fn combine_type_mappers(
        &mut self,
        mapper1: Option<MapperId>,
        mapper2: MapperId,
    ) -> MapperId {
        match mapper1 {
            Some(mapper1) => self.make_composite_type_mapper(false, mapper1, mapper2),
            None => mapper2,
        }
    }

    /// tsc-port: mergeTypeMappers @6.0.3
    /// tsc-hash: a0a8887e865c76ba68e96a6afc6cf0e7936b3269cf9844f78846d137571acb16
    /// tsc-span: _tsc.js:63391-63393
    pub fn merge_type_mappers(&mut self, mapper1: Option<MapperId>, mapper2: MapperId) -> MapperId {
        match mapper1 {
            Some(mapper1) => self.make_composite_type_mapper(true, mapper1, mapper2),
            None => mapper2,
        }
    }

    /// tsc-port: prependTypeMapping @6.0.3
    /// tsc-hash: aab8559e2938956f9e9fbc2ff25806323c347936f444603d2d4b55be36238652
    /// tsc-span: _tsc.js:63394-63396
    pub fn prepend_type_mapping(
        &mut self,
        source: TypeId,
        target: TypeId,
        mapper: Option<MapperId>,
    ) -> MapperId {
        match mapper {
            None => self.make_unary_type_mapper(source, target),
            Some(mapper) => {
                let unary = self.make_unary_type_mapper(source, target);
                self.make_composite_type_mapper(true, unary, mapper)
            }
        }
    }

    /// tsc-port: appendTypeMapping @6.0.3
    /// tsc-hash: e13fb7a80b5ebb373bb478fed8bed0df0ce7d9ec9df7bb4fed127c989674ff95
    /// tsc-span: _tsc.js:63397-63399
    pub fn append_type_mapping(
        &mut self,
        mapper: Option<MapperId>,
        source: TypeId,
        target: TypeId,
    ) -> MapperId {
        match mapper {
            None => self.make_unary_type_mapper(source, target),
            Some(mapper) => {
                let unary = self.make_unary_type_mapper(source, target);
                self.make_composite_type_mapper(true, mapper, unary)
            }
        }
    }

    /// The raw `TypeParameter.constraint` field read: the inline
    /// constraint of tables-synthesized markers, else the lazy links
    /// slot (unresolved = tsc's undefined).
    fn type_parameter_constraint_raw(&self, tp: TypeId) -> Option<TypeId> {
        if let TypeData::TypeParameter {
            constraint: Some(inline),
            ..
        } = self.tables.type_of(tp).data
        {
            return Some(inline);
        }
        self.links.ty(tp).type_parameter_constraint.resolved()
    }

    /// tsc-port: getRestrictiveTypeParameter @6.0.3
    /// tsc-hash: c2334c93091b888b0c243802f8a3d5f93ea4d609adc1b3fcca6b40ec4b4c0009
    /// tsc-span: _tsc.js:63400-63402
    fn get_restrictive_type_parameter(&mut self, tp: TypeId) -> TypeId {
        let raw_constraint = self.type_parameter_constraint_raw(tp);
        let unconstrained = (raw_constraint.is_none()
            && self.get_constraint_declaration(tp).is_none())
            || raw_constraint == Some(self.no_constraint_type);
        if unconstrained {
            return tp;
        }
        if let Some(cached) = self.links.ty(tp).restrictive_instantiation.resolved() {
            return cached;
        }
        let symbol = self.tables.type_of(tp).symbol;
        let restrictive = self.tables.create_type(
            TypeFlags::TYPE_PARAMETER,
            TypeData::TypeParameter {
                is_this_type: false,
                constraint: None,
            },
        );
        self.tables.type_mut(restrictive).symbol = symbol;
        let no_constraint = self.no_constraint_type;
        self.links.set_type_parameter_constraint(
            self.speculation_depth,
            restrictive,
            no_constraint,
        );
        self.links
            .set_type_restrictive_instantiation(self.speculation_depth, tp, restrictive);
        restrictive
    }

    /// tsc-port: cloneTypeParameter @6.0.3
    /// tsc-hash: 58e94c72a583540b7fe44cf0ef3d87bba03057b16a1dd4601531b4a1294ed633
    /// tsc-span: _tsc.js:63403-63407
    fn clone_type_parameter(&mut self, type_parameter: TypeId) -> TypeId {
        let symbol = self.tables.type_of(type_parameter).symbol;
        let result = self.tables.create_type(
            TypeFlags::TYPE_PARAMETER,
            TypeData::TypeParameter {
                is_this_type: false,
                constraint: None,
            },
        );
        self.tables.type_mut(result).symbol = symbol;
        self.links
            .set_type_parameter_target(self.speculation_depth, result, type_parameter);
        result
    }

    /// tsc-port: instantiateSignature @6.0.3
    /// tsc-hash: 1019f1062e6618574f9157950b306ca478b25c9add481fbc52a3513576d9ef08
    /// tsc-span: _tsc.js:63411-63435
    pub fn instantiate_signature(
        &mut self,
        signature: SignatureId,
        mapper: MapperId,
        erase_type_parameters: bool,
    ) -> CheckResult2<SignatureId> {
        let source = self.signature_of(signature).clone();
        let mut mapper = mapper;
        let mut fresh_type_parameters: Option<Vec<TypeId>> = None;
        if let Some(type_parameters) = &source.type_parameters {
            if !erase_type_parameters {
                let fresh: Vec<TypeId> = type_parameters
                    .iter()
                    .map(|&tp| self.clone_type_parameter(tp))
                    .collect();
                let fresh_mapper =
                    self.create_type_mapper(type_parameters.clone(), Some(fresh.clone()));
                mapper = self.combine_type_mappers(Some(fresh_mapper), mapper);
                for &tp in &fresh {
                    self.links
                        .set_type_parameter_mapper(self.speculation_depth, tp, mapper);
                }
                fresh_type_parameters = Some(fresh);
            }
        }
        let this_parameter = source
            .this_parameter
            .map(|this| self.instantiate_symbol(this, mapper));
        let parameters: Vec<SymbolId> = source
            .parameters
            .iter()
            .map(|&parameter| self.instantiate_symbol(parameter, mapper))
            .collect();
        let result = Signature {
            declaration: source.declaration,
            flags: source.flags & SignatureFlags::PROPAGATING_FLAGS,
            type_parameters: fresh_type_parameters,
            parameters,
            this_parameter,
            min_argument_count: source.min_argument_count,
            resolved_return_type: LinkSlot::Vacant,
            from_method: source.from_method,
            target: Some(signature),
            mapper: Some(mapper),
            instantiations: std::collections::HashMap::new(),
            erased_signature_cache: None,
            composite_kind: None,
            composite_signatures: None,
            optional_call_signature_cache: (None, None),
        };
        Ok(self.alloc_signature(result))
    }

    /// tsc-port: instantiateSymbol @6.0.3
    /// tsc-hash: 3151a0198339a37ffdc3a8585ade0d5da549d926ba9a7fe013bd08c3919ba27a
    /// tsc-span: _tsc.js:63436-63462
    ///
    /// The SetAccessor writeType fast-path read (63440-63445) sees
    /// links.writeType as always-unset — that slot lands with the
    /// accessors port (getTypeOfAccessors, 5.1 residual) — so the early
    /// return is correctly NOT taken for set-accessor symbols, exactly
    /// as tsc behaves while writeType is undefined.
    pub fn instantiate_symbol(&mut self, symbol: SymbolId, mapper: MapperId) -> SymbolId {
        let mut symbol = symbol;
        let mut mapper = mapper;
        let links = self.links.symbol(symbol);
        if let Some(cached_type) = links.type_of_symbol.resolved() {
            if !self.could_contain_type_variables(cached_type) {
                if !self
                    .symbol_flags(symbol)
                    .intersects(SymbolFlags::SET_ACCESSOR)
                {
                    return symbol;
                }
                // 63442-63444: set accessors also need a var-free
                // writeType before the fast path applies.
                if let Some(write_type) = self.links.symbol(symbol).write_type.resolved() {
                    if !self.could_contain_type_variables(write_type) {
                        return symbol;
                    }
                }
            }
        }
        let links = self.links.symbol(symbol);
        if links.check_flags.intersects(CheckFlags::INSTANTIATED) {
            let target = links
                .target
                .expect("Instantiated check flag implies links.target");
            let target_mapper = links.mapper;
            symbol = target;
            mapper = self.combine_type_mappers(target_mapper, mapper);
        }
        let source = self.binder.symbol(symbol);
        let source_flags = source.flags;
        let escaped_name = source.escaped_name.clone();
        let declarations = source.declarations.clone();
        let parent = source.parent;
        let value_declaration = source.value_declaration;
        let check_flags = CheckFlags::INSTANTIATED
            | CheckFlags::from_bits(
                self.links.symbol(symbol).check_flags.bits()
                    & (CheckFlags::READONLY
                        | CheckFlags::LATE
                        | CheckFlags::OPTIONAL_PARAMETER
                        | CheckFlags::REST_PARAMETER)
                        .bits(),
            );
        let name_type = self.links.symbol(symbol).name_type;
        let result = self.binder.create_symbol(source_flags, escaped_name);
        self.links
            .set_symbol_check_flags(self.speculation_depth, result, check_flags);
        {
            let transient = self.binder.symbol_mut(result);
            transient.declarations = declarations;
            transient.parent = parent;
            transient.value_declaration = value_declaration;
        }
        self.links.set_symbol_instantiation_links(
            self.speculation_depth,
            result,
            symbol,
            mapper,
            name_type,
        );
        result
    }

    /// tsc-port: getObjectTypeInstantiation @6.0.3
    /// tsc-hash: fb17fb2693f1b9664336314c08f11b85b999e573f3a4b070da39a7b35e99a791
    /// tsc-span: _tsc.js:63463-63517
    ///
    /// The Reference arms are live (deferred references, 5.2g); the
    /// InstantiationExpressionType `type.node` read stays dead until
    /// M6 instantiation expressions. The JS-constructor template-tag
    /// parameters (63474-63477) ride on JSDoc binding and are elided
    /// with it (project-wide).
    fn get_object_type_instantiation(
        &mut self,
        ty: TypeId,
        mapper: MapperId,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<TypeId> {
        let object_flags = self.tables.object_flags_of(ty);
        let is_reference = object_flags.intersects(ObjectFlags::REFERENCE);
        let declaration = if is_reference {
            // 63464: deferred references carry their node; the
            // InstantiationExpressionType node arm is M6.
            self.links
                .ty(ty)
                .deferred_node
                .expect("References here are deferred (worker !node gate)")
        } else {
            let symbol = self
                .tables
                .type_of(ty)
                .symbol
                .expect("anonymous type instantiation requires a symbol");
            *self
                .binder
                .symbol(symbol)
                .declarations
                .first()
                .expect("couldContainTypeVariables demands declarations")
        };
        let target = if is_reference {
            // 63466: the canonical deferred reference cached on the
            // node hosts the instantiations map.
            self.links
                .node(declaration)
                .resolved_type
                .resolved()
                .expect("deferred references are node-cached before instantiation")
        } else if object_flags.intersects(ObjectFlags::INSTANTIATED) {
            self.links
                .ty(ty)
                .instantiated_target
                .expect("Instantiated object flag implies links target")
        } else {
            ty
        };
        let type_parameters = match self
            .links
            .node(declaration)
            .outer_type_parameters
            .resolved()
        {
            Some(cached) => cached.to_vec(),
            None => {
                let outer = self
                    .get_outer_type_parameters(declaration, /*include_this_types*/ true)?
                    .unwrap_or_default();
                // 63481: `target.objectFlags & (Reference |
                // InstantiationExpressionType) || target.symbol.flags &
                // Method || ... & TypeLiteral` — the symbol read is
                // short-circuited away for references (tuple-target
                // deferred references have no symbol).
                let filter_applies =
                    (self.tables.object_flags_of(target).intersects(
                        ObjectFlags::REFERENCE | ObjectFlags::INSTANTIATION_EXPRESSION_TYPE,
                    ) || {
                        let target_symbol = self
                            .tables
                            .type_of(target)
                            .symbol
                            .expect("non-reference instantiation targets carry their symbol");
                        self.symbol_flags(target_symbol)
                            .intersects(SymbolFlags::METHOD)
                            || self
                                .symbol_flags(target_symbol)
                                .intersects(SymbolFlags::TYPE_LITERAL)
                    }) && self.tables.type_of(target).alias_type_arguments.is_none();
                let all_declarations: Vec<NodeId> = if object_flags
                    .intersects(ObjectFlags::REFERENCE | ObjectFlags::INSTANTIATION_EXPRESSION_TYPE)
                {
                    vec![declaration]
                } else {
                    let symbol = self
                        .tables
                        .type_of(ty)
                        .symbol
                        .expect("anonymous type instantiation requires a symbol");
                    self.binder.symbol(symbol).declarations.clone()
                };
                let filtered = if filter_applies {
                    let mut kept: Vec<TypeId> = Vec::new();
                    for tp in outer {
                        let mut referenced = false;
                        for &candidate in &all_declarations {
                            if self.is_type_parameter_possibly_referenced(tp, candidate)? {
                                referenced = true;
                                break;
                            }
                        }
                        if referenced {
                            kept.push(tp);
                        }
                    }
                    kept
                } else {
                    outer
                };
                self.links.set_node_outer_type_parameters(
                    self.speculation_depth,
                    declaration,
                    filtered.clone().into_boxed_slice(),
                );
                filtered
            }
        };
        if type_parameters.is_empty() {
            return Ok(ty);
        }
        // `type.mapper` (63485): the deferred-reference mapper for
        // references, the instantiation mapper for anonymous shells.
        let type_mapper = if is_reference {
            self.links.ty(ty).deferred_mapper
        } else {
            self.links.ty(ty).instantiated_mapper
        };
        let combined_mapper = self.combine_type_mappers(type_mapper, mapper);
        let mut type_arguments: Vec<TypeId> = Vec::with_capacity(type_parameters.len());
        for &tp in &type_parameters {
            type_arguments.push(self.get_mapped_type(tp, combined_mapper)?);
        }
        let new_alias_symbol = alias_symbol.or(self.tables.type_of(ty).alias_symbol);
        let new_alias_type_arguments = if alias_symbol.is_some() {
            alias_type_arguments.map(<[TypeId]>::to_vec)
        } else {
            match self.tables.type_of(ty).alias_type_arguments.clone() {
                Some(arguments) => Some(self.instantiate_types(&arguments, mapper)?),
                None => None,
            }
        };
        let id_key = format!(
            "{}{}",
            self.tables.get_type_list_id(&type_arguments),
            self.tables
                .get_alias_id(new_alias_symbol, new_alias_type_arguments.as_deref())
        );
        // 63489-63492: the target's instantiations map is seeded with
        // itself under its own type-parameter list id.
        let target_alias_symbol = self.tables.type_of(target).alias_symbol;
        let target_alias_arguments = self.tables.type_of(target).alias_type_arguments.clone();
        let self_key = format!(
            "{}{}",
            self.tables.get_type_list_id(&type_parameters),
            self.tables
                .get_alias_id(target_alias_symbol, target_alias_arguments.as_deref())
        );
        if self.tables.instantiation_get(target, &self_key).is_none() {
            self.tables.instantiation_insert(target, self_key, target);
        }
        if let Some(existing) = self.tables.instantiation_get(target, &id_key) {
            return Ok(existing);
        }
        let mut new_mapper =
            self.create_type_mapper(type_parameters.clone(), Some(type_arguments.clone()));
        // SingleSignatureType targets (63495-63497) are unconstructible
        // before M6 instantiation expressions.
        assert!(
            !self
                .tables
                .object_flags_of(target)
                .intersects(ObjectFlags::SINGLE_SIGNATURE_TYPE),
            "SingleSignatureType is unconstructible before M6"
        );
        let _ = &mut new_mapper;
        let target_object_flags = self.tables.object_flags_of(target);
        let result = if target_object_flags.intersects(ObjectFlags::REFERENCE) {
            // 63499: a fresh deferred reference over the SAME node with
            // the instantiation mapper — `type.target`/`type.node`, not
            // the canonical target's.
            let reference_target = self.tables.reference_target(ty);
            self.create_deferred_type_reference(
                reference_target,
                declaration,
                Some(new_mapper),
                new_alias_symbol,
                new_alias_type_arguments.as_deref(),
            )?
        } else if target_object_flags.intersects(ObjectFlags::MAPPED) {
            return Err(Unsupported::new("mapped type instantiation (M8)"));
        } else {
            self.instantiate_anonymous_type(
                target,
                new_mapper,
                new_alias_symbol,
                new_alias_type_arguments.as_deref(),
            )?
        };
        self.tables.instantiation_insert(target, id_key, result);
        // 63503-63514: propagate couldContainTypeVariables over the
        // type arguments onto the result's memo bits.
        let result_object_flags = self.tables.object_flags_of(result);
        if self
            .tables
            .flags_of(result)
            .intersects(TypeFlags::OBJECT_FLAGS_TYPE)
            && !result_object_flags.intersects(ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES_COMPUTED)
        {
            let mut result_could_contain = false;
            for &argument in &type_arguments {
                if self.could_contain_type_variables(argument) {
                    result_could_contain = true;
                    break;
                }
            }
            let latest = self.tables.object_flags_of(result);
            if !latest.intersects(ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES_COMPUTED) {
                let updated = if latest.intersects(
                    ObjectFlags::MAPPED | ObjectFlags::ANONYMOUS | ObjectFlags::REFERENCE,
                ) {
                    latest.bits()
                        | ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES_COMPUTED.bits()
                        | if result_could_contain {
                            ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES.bits()
                        } else {
                            0
                        }
                } else if !result_could_contain {
                    latest.bits() | ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES_COMPUTED.bits()
                } else {
                    latest.bits()
                };
                self.tables.type_mut(result).object_flags = ObjectFlags::from_bits(updated);
            }
        }
        Ok(result)
    }

    /// tsc-port: maybeTypeParameterReference @6.0.3
    /// tsc-hash: 8b10b6ebcc2a9a9cfe7c3f4febb9da66c043ae20805930219affb8e2856bfaa9
    /// tsc-span: _tsc.js:63518-63520
    fn maybe_type_parameter_reference(&self, node: NodeId) -> bool {
        let Some(parent) = self.parent_of(node) else {
            return true;
        };
        match self.data_of(parent) {
            NodeData::TypeReference(data) if self.kind_of(parent) == SyntaxKind::TypeReference => {
                !(data.type_arguments.is_some() && data.type_name == Some(node))
            }
            NodeData::ImportType(data) if self.kind_of(parent) == SyntaxKind::ImportType => {
                !(data.type_arguments.is_some() && data.qualifier == Some(node))
            }
            _ => true,
        }
    }

    /// tsc-port: isTypeParameterPossiblyReferenced @6.0.3
    /// tsc-hash: d81c8461734d2002d8f9c657319d9f1aa8c55019b3278723ea0de45f6c917f1f
    /// tsc-span: _tsc.js:63521-63562
    fn is_type_parameter_possibly_referenced(
        &mut self,
        tp: TypeId,
        node: NodeId,
    ) -> CheckResult2<bool> {
        let symbol = self.tables.type_of(tp).symbol;
        let declarations = symbol
            .map(|symbol| self.binder.symbol(symbol).declarations.clone())
            .unwrap_or_default();
        if declarations.len() != 1 {
            return Ok(true);
        }
        let container = self.parent_of(declarations[0]);
        let mut n = Some(node);
        while n != container {
            let Some(current) = n else {
                return Ok(true);
            };
            if self.kind_of(current) == SyntaxKind::Block {
                return Ok(true);
            }
            if self.kind_of(current) == SyntaxKind::ConditionalType {
                let NodeData::ConditionalType(data) = self.data_of(current) else {
                    unreachable!("ConditionalType kind implies payload");
                };
                if let Some(extends_type) = data.extends_type {
                    if self.contains_reference(tp, extends_type)? {
                        return Ok(true);
                    }
                }
            }
            n = self.parent_of(current);
        }
        self.contains_reference(tp, node)
    }

    /// The containsReference walker inside isTypeParameterPossiblyReferenced
    /// (63536-63561).
    fn contains_reference(&mut self, tp: TypeId, node: NodeId) -> CheckResult2<bool> {
        match self.kind_of(node) {
            SyntaxKind::ThisType => Ok(matches!(
                self.tables.type_of(tp).data,
                TypeData::TypeParameter {
                    is_this_type: true,
                    ..
                }
            )),
            SyntaxKind::Identifier => {
                let is_this_type = matches!(
                    self.tables.type_of(tp).data,
                    TypeData::TypeParameter {
                        is_this_type: true,
                        ..
                    }
                );
                if is_this_type
                    || !self.is_part_of_type_node(node)
                    || !self.maybe_type_parameter_reference(node)
                {
                    return Ok(false);
                }
                // getTypeFromTypeNodeWorker(identifier) (63290-63293)
                // resolves the name and takes its DECLARED type; a type
                // parameter type is per-symbol memoized, so `=== tp`
                // reduces to "resolves to tp's symbol". Resolution is
                // quiet (ignore_errors), like getSymbolAtLocation.
                let resolved = self.resolve_entity_name(
                    node,
                    SymbolFlags::TYPE,
                    /*ignore_errors*/ true,
                    None,
                );
                match resolved {
                    Some(candidate)
                        if self
                            .symbol_flags(candidate)
                            .intersects(SymbolFlags::TYPE_PARAMETER) =>
                    {
                        let declared = self.get_declared_type_of_type_parameter(candidate);
                        Ok(declared == tp)
                    }
                    _ => Ok(false),
                }
            }
            SyntaxKind::TypeQuery => {
                let NodeData::TypeQuery(data) = self.data_of(node) else {
                    unreachable!("TypeQuery kind implies payload");
                };
                let type_arguments = data.type_arguments;
                let Some(entity_name) = data.expr_name else {
                    return Ok(true);
                };
                let first_identifier = self.first_identifier(entity_name);
                if !self.is_this_identifier(first_identifier) {
                    let first_identifier_symbol = self.get_resolved_symbol(first_identifier);
                    let tp_symbol = self.tables.type_of(tp).symbol;
                    let tp_declaration = tp_symbol.and_then(|symbol| {
                        self.binder.symbol(symbol).declarations.first().copied()
                    });
                    let tp_is_this = matches!(
                        self.tables.type_of(tp).data,
                        TypeData::TypeParameter {
                            is_this_type: true,
                            ..
                        }
                    );
                    let tp_scope = match tp_declaration {
                        Some(declaration)
                            if self.kind_of(declaration) == SyntaxKind::TypeParameter =>
                        {
                            self.parent_of(declaration)
                        }
                        Some(declaration) if tp_is_this => Some(declaration),
                        _ => None,
                    };
                    let symbol_declarations = first_identifier_symbol
                        .map(|symbol| self.binder.symbol(symbol).declarations.clone());
                    if let (Some(declarations), Some(scope)) = (symbol_declarations, tp_scope) {
                        if !declarations.is_empty() {
                            for declaration in declarations {
                                if self.is_node_descendant_of(declaration, scope) {
                                    return Ok(true);
                                }
                            }
                            for argument in self.nodes_of(type_arguments) {
                                if self.contains_reference(tp, argument)? {
                                    return Ok(true);
                                }
                            }
                            return Ok(false);
                        }
                    }
                }
                Ok(true)
            }
            SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature => {
                let (r#type, body, type_parameters, parameters) = match self.data_of(node) {
                    NodeData::MethodDeclaration(data) => (
                        data.r#type,
                        data.body,
                        data.type_parameters,
                        data.parameters,
                    ),
                    NodeData::MethodSignature(data) => {
                        (data.r#type, None, data.type_parameters, data.parameters)
                    }
                    _ => unreachable!("method kind implies payload"),
                };
                if r#type.is_none() && body.is_some() {
                    return Ok(true);
                }
                for child in self.nodes_of(type_parameters) {
                    if self.contains_reference(tp, child)? {
                        return Ok(true);
                    }
                }
                for child in self.nodes_of(parameters) {
                    if self.contains_reference(tp, child)? {
                        return Ok(true);
                    }
                }
                match r#type {
                    Some(annotation) => self.contains_reference(tp, annotation),
                    None => Ok(false),
                }
            }
            _ => {
                for child in self.children_of(node) {
                    if self.contains_reference(tp, child)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }

    /// tsc-port: instantiateAnonymousType @6.0.3
    /// tsc-hash: bde1b975a7540a89787ebe33ba5a6687d2d56e37688858dd76a35ab126603da9
    /// tsc-span: _tsc.js:63637-63657
    ///
    /// The Mapped arm (63640-63647) is unreachable — mapped targets
    /// route to instantiateMappedType (M8) before this call — and the
    /// InstantiationExpressionType node copy (63648-63650) is dead
    /// until M6; both asserted, not elided.
    fn instantiate_anonymous_type(
        &mut self,
        ty: TypeId,
        mapper: MapperId,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<TypeId> {
        let source = self.tables.type_of(ty);
        debug_assert!(
            source.symbol.is_some(),
            "anonymous type must have symbol to be instantiated"
        );
        let source_symbol = source.symbol;
        let source_alias_symbol = source.alias_symbol;
        let source_alias_type_arguments = source.alias_type_arguments.clone();
        let source_object_flags = self.tables.object_flags_of(ty);
        assert!(
            !source_object_flags
                .intersects(ObjectFlags::MAPPED | ObjectFlags::INSTANTIATION_EXPRESSION_TYPE),
            "mapped/instantiation-expression types are unconstructible before M8/M6"
        );
        let result = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
        let mut object_flags = (source_object_flags.bits()
            & !(ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES_COMPUTED.bits()
                | ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES.bits()))
            | ObjectFlags::INSTANTIATED.bits();
        self.tables.type_mut(result).symbol = source_symbol;
        self.links
            .set_type_instantiation_links(self.speculation_depth, result, ty, mapper);
        let new_alias_symbol = alias_symbol.or(source_alias_symbol);
        let new_alias_type_arguments = if alias_symbol.is_some() {
            alias_type_arguments.map(<[TypeId]>::to_vec)
        } else {
            match source_alias_type_arguments {
                Some(arguments) => Some(self.instantiate_types(&arguments, mapper)?),
                None => None,
            }
        };
        if let Some(arguments) = &new_alias_type_arguments {
            object_flags |= self
                .tables
                .get_propagating_flags_of_types(arguments, TypeFlags::from_bits(0))
                .bits();
        }
        let result_type = self.tables.type_mut(result);
        result_type.alias_symbol = new_alias_symbol;
        result_type.alias_type_arguments =
            new_alias_type_arguments.map(|arguments| arguments.into_boxed_slice());
        result_type.object_flags = ObjectFlags::from_bits(object_flags);
        Ok(result)
    }

    /// tsc-port: instantiateType @6.0.3
    /// tsc-hash: deb287805c3917727bbdf4f8437e557278ef62a92382a2ef9a1cf780c501e181
    /// tsc-span: _tsc.js:63675-63684
    pub fn instantiate_type(
        &mut self,
        ty: TypeId,
        mapper: Option<MapperId>,
    ) -> CheckResult2<TypeId> {
        match mapper {
            Some(mapper) => self.instantiate_type_with_alias(ty, mapper, None, None),
            None => Ok(ty),
        }
    }

    /// tsc-port: instantiateTypeWithAlias @6.0.3
    /// tsc-hash: 7b89d170da7ed964a1fd3c86df71375fa36abc08195649e149cb30d6f255ece9
    /// tsc-span: _tsc.js:63685-63716
    pub(crate) fn instantiate_type_with_alias(
        &mut self,
        ty: TypeId,
        mapper: MapperId,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<TypeId> {
        if !self.could_contain_type_variables(ty) {
            return Ok(ty);
        }
        if self.instantiation_depth == 100 || self.instantiation_count >= 5_000_000 {
            // error(currentNode, 2589): currentNode is the driver's
            // element cursor (5.4); queries outside the driver (probe
            // entries, relpin) still emit file-less.
            let current_node = self.current_node;
            self.error_at(
                current_node,
                &diagnostics::Type_instantiation_is_excessively_deep_and_possibly_infinite,
                &[],
            );
            return Ok(self.tables.intrinsics.error);
        }
        let index = self.find_active_mapper(mapper);
        if index.is_none() {
            self.push_active_mapper(mapper);
        }
        let key = format!(
            "{}{}",
            ty.0,
            self.tables.get_alias_id(alias_symbol, alias_type_arguments)
        );
        let cache_index = index.unwrap_or(self.active_type_mappers.len() - 1);
        if let Some(&cached) = self.active_type_mappers_caches[cache_index].get(&key) {
            return Ok(cached);
        }
        self.total_instantiation_count += 1;
        self.instantiation_count += 1;
        self.instantiation_depth += 1;
        let result = self.instantiate_type_worker(ty, mapper, alias_symbol, alias_type_arguments);
        match (&result, index) {
            (_, None) => self.pop_active_mapper(),
            (Ok(value), Some(active)) => {
                self.active_type_mappers_caches[active].insert(key, *value);
            }
            (Err(_), Some(_)) => {}
        }
        self.instantiation_depth -= 1;
        result
    }

    /// tsc-port: instantiateTypeWorker @6.0.3
    /// tsc-hash: b13e58cd032228486ee4b1aaf036cfd363ea106831c16d6bc6c3e4f2d537b0e9
    /// tsc-span: _tsc.js:63717-63795
    ///
    /// Unsupported escapes name their owners: Index/IndexedAccess
    /// (keyof + indexed access, 5.2 follow-up), ReverseMapped/
    /// Conditional/Substitution (M8). Each of those TypeFlags is
    /// unconstructible today — the escape fires if one ever appears
    /// rather than mis-computing.
    fn instantiate_type_worker(
        &mut self,
        ty: TypeId,
        mapper: MapperId,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::TYPE_PARAMETER) {
            return self.get_mapped_type(ty, mapper);
        }
        if flags.intersects(TypeFlags::OBJECT) {
            let object_flags = self.tables.object_flags_of(ty);
            if object_flags
                .intersects(ObjectFlags::REFERENCE | ObjectFlags::ANONYMOUS | ObjectFlags::MAPPED)
            {
                if object_flags.intersects(ObjectFlags::REFERENCE)
                    && self.links.ty(ty).deferred_node.is_none()
                {
                    // The !type.node fast path (63725-63729); deferred
                    // (node-carrying) references fall through to
                    // getObjectTypeInstantiation.
                    let target = self.tables.reference_target(ty);
                    let resolved: Vec<TypeId> = self.tables.type_arguments(ty).to_vec();
                    let new_type_arguments = self.instantiate_types(&resolved, mapper)?;
                    return if new_type_arguments != resolved {
                        self.create_normalized_type_reference_forced(target, &new_type_arguments)
                    } else {
                        Ok(ty)
                    };
                }
                if object_flags.intersects(ObjectFlags::REVERSE_MAPPED) {
                    return Err(Unsupported::new("reverse-mapped type instantiation (M8)"));
                }
                return self.get_object_type_instantiation(
                    ty,
                    mapper,
                    alias_symbol,
                    alias_type_arguments,
                );
            }
            return Ok(ty);
        }
        if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            let origin = match &self.tables.type_of(ty).data {
                TypeData::Union { origin, .. } if flags.intersects(TypeFlags::UNION) => *origin,
                _ => None,
            };
            let source_types: Vec<TypeId> = match origin {
                Some(origin)
                    if self
                        .tables
                        .flags_of(origin)
                        .intersects(TypeFlags::UNION_OR_INTERSECTION) =>
                {
                    match &self.tables.type_of(origin).data {
                        TypeData::Union { types, .. } => types.to_vec(),
                        TypeData::Intersection { types } => types.to_vec(),
                        _ => unreachable!("origin union/intersection flag implies member data"),
                    }
                }
                _ => match &self.tables.type_of(ty).data {
                    TypeData::Union { types, .. } => types.to_vec(),
                    TypeData::Intersection { types } => types.to_vec(),
                    _ => unreachable!("union/intersection flag implies member data"),
                },
            };
            let new_types = self.instantiate_types(&source_types, mapper)?;
            if new_types == source_types && alias_symbol == self.tables.type_of(ty).alias_symbol {
                return Ok(ty);
            }
            let new_alias_symbol = alias_symbol.or(self.tables.type_of(ty).alias_symbol);
            let new_alias_type_arguments = if alias_symbol.is_some() {
                alias_type_arguments.map(<[TypeId]>::to_vec)
            } else {
                match self.tables.type_of(ty).alias_type_arguments.clone() {
                    Some(arguments) => Some(self.instantiate_types(&arguments, mapper)?),
                    None => None,
                }
            };
            let origin_is_intersection = origin.is_some_and(|origin| {
                self.tables
                    .flags_of(origin)
                    .intersects(TypeFlags::INTERSECTION)
            });
            return if flags.intersects(TypeFlags::INTERSECTION) || origin_is_intersection {
                self.get_intersection_type_ex(
                    &new_types,
                    IntersectionFlags::NONE,
                    new_alias_symbol,
                    new_alias_type_arguments.as_deref(),
                )
            } else {
                self.get_union_type_ex_with_origin(
                    &new_types,
                    UnionReduction::Literal,
                    new_alias_symbol,
                    new_alias_type_arguments.as_deref(),
                    None,
                )
            };
        }
        if flags.intersects(TypeFlags::INDEX) {
            // 63749: the StringsOnly bit deliberately drops on
            // instantiation (getIndexType default flags).
            let TypeData::Index { ty: inner, .. } = self.tables.type_of(ty).data else {
                unreachable!("index flag implies index data");
            };
            let new_inner = self.instantiate_type(inner, Some(mapper))?;
            return self.get_index_type(new_inner, tsrs2_types::IndexFlags::NONE);
        }
        if flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
            let (texts, types) = match &self.tables.type_of(ty).data {
                TypeData::TemplateLiteral { texts, types } => (texts.to_vec(), types.to_vec()),
                _ => unreachable!("template flag implies template data"),
            };
            let new_types = self.instantiate_types(&types, mapper)?;
            return Ok(self.tables.get_template_literal_type(&texts, &new_types));
        }
        if flags.intersects(TypeFlags::STRING_MAPPING) {
            let TypeData::StringMapping { ty: inner } = self.tables.type_of(ty).data else {
                unreachable!("string-mapping flag implies string-mapping data");
            };
            let symbol = self
                .tables
                .type_of(ty)
                .symbol
                .expect("string-mapping types carry the intrinsic symbol");
            let new_inner = self.instantiate_type(inner, Some(mapper))?;
            return self.get_string_mapping_type(symbol, new_inner);
        }
        if flags.intersects(TypeFlags::INDEXED_ACCESS) {
            let new_alias_symbol = alias_symbol.or(self.tables.type_of(ty).alias_symbol);
            let new_alias_type_arguments = if alias_symbol.is_some() {
                alias_type_arguments.map(<[TypeId]>::to_vec)
            } else {
                match self.tables.type_of(ty).alias_type_arguments.clone() {
                    Some(arguments) => Some(self.instantiate_types(&arguments, mapper)?),
                    None => None,
                }
            };
            let TypeData::IndexedAccess {
                object_type,
                index_type,
                access_flags,
            } = self.tables.type_of(ty).data
            else {
                unreachable!("indexed-access flag implies indexed-access data");
            };
            let new_object = self.instantiate_type(object_type, Some(mapper))?;
            let new_index = self.instantiate_type(index_type, Some(mapper))?;
            return self.get_indexed_access_type(
                new_object,
                new_index,
                access_flags,
                /*access_node*/ None,
                new_alias_symbol,
                new_alias_type_arguments.as_deref(),
            );
        }
        if flags.intersects(TypeFlags::CONDITIONAL) {
            return Err(Unsupported::new(
                "conditional-type instantiation (getConditionalTypeInstantiation, M8)",
            ));
        }
        if flags.intersects(TypeFlags::SUBSTITUTION) {
            return Err(Unsupported::new("substitution-type instantiation (M8)"));
        }
        Ok(ty)
    }

    /// instantiateList + instantiateTypes (63298-63317): tsc returns
    /// the same array when nothing changed — callers compare slices for
    /// the same identity answer.
    pub fn instantiate_types(
        &mut self,
        types: &[TypeId],
        mapper: MapperId,
    ) -> CheckResult2<Vec<TypeId>> {
        let mut result = Vec::with_capacity(types.len());
        for &ty in types {
            result.push(self.instantiate_type(ty, Some(mapper))?);
        }
        Ok(result)
    }

    /// tsc-port: instantiateIndexInfo @6.0.3
    /// tsc-hash: 3b4b664a5bbf383ab52e2a181219942ffc8a604bf3484fb8fba14a5cff5e8a99
    /// tsc-span: _tsc.js:63829-63831
    ///
    /// `components` is not modeled (index-signature declaration lists,
    /// 5.3).
    pub fn instantiate_index_info(
        &mut self,
        info: &crate::state::IndexInfo,
        mapper: MapperId,
    ) -> CheckResult2<crate::state::IndexInfo> {
        let value_type = self.instantiate_type(info.value_type, Some(mapper))?;
        Ok(crate::state::IndexInfo {
            key_type: info.key_type,
            value_type,
            is_readonly: info.is_readonly,
            declaration: info.declaration,
            components: None,
        })
    }

    /// tsc-port: getPermissiveInstantiation @6.0.3
    /// tsc-hash: b215e803aced2e65175ffa5a5f69c5bc14075e5898b9e01e29edf4d45182474b
    /// tsc-span: _tsc.js:63815-63817
    pub fn get_permissive_instantiation(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if self
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::PRIMITIVE | TypeFlags::ANY_OR_UNKNOWN | TypeFlags::NEVER)
        {
            return Ok(ty);
        }
        if let Some(cached) = self.links.ty(ty).permissive_instantiation.resolved() {
            return Ok(cached);
        }
        let mapper = self.permissive_mapper;
        let result = self.instantiate_type(ty, Some(mapper))?;
        self.links
            .set_type_permissive_instantiation(self.speculation_depth, ty, result);
        Ok(result)
    }

    /// tsc-port: getRestrictiveInstantiation @6.0.3
    /// tsc-hash: d450bbffe7bd10c6ad0d93eb8bacb16be8a7b92303aee9a7565e3ff43c328716
    /// tsc-span: _tsc.js:63818-63828
    pub fn get_restrictive_instantiation(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if self
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::PRIMITIVE | TypeFlags::ANY_OR_UNKNOWN | TypeFlags::NEVER)
        {
            return Ok(ty);
        }
        if let Some(cached) = self.links.ty(ty).restrictive_instantiation.resolved() {
            return Ok(cached);
        }
        let mapper = self.restrictive_mapper;
        let result = self.instantiate_type(ty, Some(mapper))?;
        self.links
            .set_type_restrictive_instantiation(self.speculation_depth, ty, result);
        self.links
            .set_type_restrictive_instantiation(self.speculation_depth, result, result);
        Ok(result)
    }

    // ---- the active-mapper instantiation cache stack ----

    /// tsc-port: pushActiveMapper @6.0.3
    /// tsc-hash: 2427ee73f242d95460bbfce1cdf64a5dc8ca97c820c33f7dad5bcb5f7b96b500
    /// tsc-span: _tsc.js:73606-73610
    fn push_active_mapper(&mut self, mapper: MapperId) {
        self.active_type_mappers.push(mapper);
        self.active_type_mappers_caches
            .push(std::collections::HashMap::new());
    }

    /// tsc-port: popActiveMapper @6.0.3
    /// tsc-hash: 13a2f9c435dc40c4e65324e250a5929c9bc93e9c3505415df1bac1904a774b07
    /// tsc-span: _tsc.js:73611-73615
    fn pop_active_mapper(&mut self) {
        self.active_type_mappers.pop();
        self.active_type_mappers_caches.pop();
    }

    /// tsc-port: findActiveMapper @6.0.3
    /// tsc-hash: ebb0d31c2e466f9a5a5171f23e8516ff8ac51e11ab41c262d6f5017d57d94485
    /// tsc-span: _tsc.js:73616-73623
    fn find_active_mapper(&self, mapper: MapperId) -> Option<usize> {
        self.active_type_mappers
            .iter()
            .rposition(|&active| active == mapper)
    }

    // ---- couldContainTypeVariables ----

    /// tsc-port: couldContainTypeVariables @6.0.3
    /// tsc-hash: 69d9595fd8e7e94d39069abfae52d4c64f24df7bcc018db02a0ed42f60cf744d
    /// tsc-span: _tsc.js:68331-68343
    ///
    /// The Reference `type.node` read is constant false (no deferred
    /// references before M8); getTypeArguments is the eager tables
    /// accessor for the same reason.
    pub fn could_contain_type_variables(&mut self, ty: TypeId) -> bool {
        let object_flags = self.tables.object_flags_of(ty);
        if object_flags.intersects(ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES_COMPUTED) {
            return object_flags.intersects(ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES);
        }
        let flags = self.tables.flags_of(ty);
        let result = flags.intersects(TypeFlags::INSTANTIABLE)
            || (flags.intersects(TypeFlags::OBJECT)
                && !self.is_non_generic_top_level_type(ty)
                && ((object_flags.intersects(ObjectFlags::REFERENCE) && {
                    // `type.node || some(getTypeArguments(type), ...)`
                    // (68336): node-carrying references short-circuit
                    // true without forcing their arguments.
                    self.links.ty(ty).deferred_node.is_some() || {
                        let arguments: Vec<TypeId> = self.tables.type_arguments(ty).to_vec();
                        arguments
                            .into_iter()
                            .any(|argument| self.could_contain_type_variables(argument))
                    }
                }) || (object_flags.intersects(ObjectFlags::ANONYMOUS)
                    && self.tables.type_of(ty).symbol.is_some_and(|symbol| {
                        self.binder.symbol(symbol).flags.intersects(
                            SymbolFlags::FUNCTION
                                | SymbolFlags::METHOD
                                | SymbolFlags::CLASS
                                | SymbolFlags::TYPE_LITERAL
                                | SymbolFlags::OBJECT_LITERAL,
                        ) && !self.binder.symbol(symbol).declarations.is_empty()
                    }))
                    || object_flags.intersects(
                        ObjectFlags::MAPPED
                            | ObjectFlags::REVERSE_MAPPED
                            | ObjectFlags::OBJECT_REST_TYPE
                            | ObjectFlags::INSTANTIATION_EXPRESSION_TYPE,
                    )))
            || (flags.intersects(TypeFlags::UNION_OR_INTERSECTION)
                && !flags.intersects(TypeFlags::ENUM_LITERAL)
                && !self.is_non_generic_top_level_type(ty)
                && {
                    let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                        TypeData::Union { types, .. } => types.to_vec(),
                        TypeData::Intersection { types } => types.to_vec(),
                        _ => unreachable!("union/intersection flag implies member data"),
                    };
                    members
                        .into_iter()
                        .any(|member| self.could_contain_type_variables(member))
                });
        if flags.intersects(TypeFlags::OBJECT_FLAGS_TYPE) {
            let updated = self.tables.object_flags_of(ty).bits()
                | ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES_COMPUTED.bits()
                | if result {
                    ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES.bits()
                } else {
                    0
                };
            self.tables.type_mut(ty).object_flags = ObjectFlags::from_bits(updated);
        }
        result
    }

    /// tsc-port: isNonGenericTopLevelType @6.0.3
    /// tsc-hash: 9a8529c79d22d005a3e0866a1e80befc1fdd042ea2a2bab0d5dd5659f758d565
    /// tsc-span: _tsc.js:68344-68350
    fn is_non_generic_top_level_type(&self, ty: TypeId) -> bool {
        let source = self.tables.type_of(ty);
        if let (Some(alias_symbol), None) = (source.alias_symbol, &source.alias_type_arguments) {
            let declaration = self
                .binder
                .symbol(alias_symbol)
                .declarations
                .iter()
                .copied()
                .find(|&declaration| self.kind_of(declaration) == SyntaxKind::TypeAliasDeclaration);
            // findAncestor(declaration.parent, SourceFile → true,
            // ModuleDeclaration → false, else "quit"): only a DIRECT
            // source-file parent answers true.
            return declaration.is_some_and(|declaration| {
                self.parent_of(declaration)
                    .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::SourceFile)
            });
        }
        false
    }

    // ---- outer type parameters ----

    /// tsc-port: appendTypeParameters @6.0.3
    /// tsc-hash: d3bbd01ed1f4fccff64ff602130783b4b417f479b4f8fde6fe37604eb6c092f6
    /// tsc-span: _tsc.js:57008-57013
    pub(crate) fn append_type_parameters(
        &mut self,
        mut type_parameters: Vec<TypeId>,
        declarations: &[NodeId],
    ) -> Vec<TypeId> {
        for &declaration in declarations {
            let Some(symbol) = self.node_symbol(declaration) else {
                continue;
            };
            // getSymbolOfDeclaration (49936) chases getMergedSymbol:
            // same-named type parameters of MERGED interface
            // declarations (lib Promise across es2015.promise +
            // symbol.wellknown) are one merged symbol — without the
            // chase each declaration minted its own parameter type and
            // `Promise<T, T>` mis-reported arity 2 (lib-loading L2
            // find; getLateBoundSymbol stays elided with late binding).
            let symbol = self.get_merged_symbol(symbol);
            let declared = self.get_declared_type_of_type_parameter(symbol);
            if !type_parameters.contains(&declared) {
                type_parameters.push(declared);
            }
        }
        type_parameters
    }

    /// getEffectiveTypeParameterDeclarations, TS-declaration slice
    /// (JSDoc template tags are elided project-wide).
    pub(crate) fn type_parameter_declarations_of(&self, node: NodeId) -> Vec<NodeId> {
        let list = match self.data_of(node) {
            NodeData::ClassDeclaration(data) => data.type_parameters,
            NodeData::ClassExpression(data) => data.type_parameters,
            NodeData::InterfaceDeclaration(data) => data.type_parameters,
            NodeData::TypeAliasDeclaration(data) => data.type_parameters,
            NodeData::FunctionDeclaration(data) => data.type_parameters,
            NodeData::MethodDeclaration(data) => data.type_parameters,
            NodeData::MethodSignature(data) => data.type_parameters,
            NodeData::CallSignature(data) => data.type_parameters,
            NodeData::ConstructSignature(data) => data.type_parameters,
            NodeData::FunctionType(data) => data.type_parameters,
            NodeData::ConstructorType(data) => data.type_parameters,
            NodeData::FunctionExpression(data) => data.type_parameters,
            NodeData::ArrowFunction(data) => data.type_parameters,
            _ => None,
        };
        self.nodes_of(list)
    }

    /// tsc-port: getOuterTypeParameters @6.0.3
    /// tsc-hash: c803db747a9b456f3fdf5ab1b1374d0b5426c33155735e4150eeeee52e3a6128
    /// tsc-span: _tsc.js:57014-57079
    ///
    /// Elisions/escapes: the BinaryExpression prototype-assignment hop
    /// (57016-57024) rides on JS Assignment binding (M2 3.4c residual);
    /// JSDoc kinds (JSDocFunctionType/template/typedef/enum/callback
    /// tags, the JSDoc parameter/comment arms 57067-57078) are elided
    /// project-wide. The context-sensitive function-expression replay
    /// (57052-57057) is live since 5.7b; conditional-type containers
    /// are live via getInferTypeParameters.
    pub(crate) fn get_outer_type_parameters(
        &mut self,
        node: NodeId,
        include_this_types: bool,
    ) -> CheckResult2<Option<Vec<TypeId>>> {
        let mut node = node;
        loop {
            let Some(next) = self.parent_of(node) else {
                return Ok(None);
            };
            node = next;
            let kind = self.kind_of(node);
            match kind {
                SyntaxKind::ClassDeclaration
                | SyntaxKind::ClassExpression
                | SyntaxKind::InterfaceDeclaration
                | SyntaxKind::CallSignature
                | SyntaxKind::ConstructSignature
                | SyntaxKind::MethodSignature
                | SyntaxKind::FunctionType
                | SyntaxKind::ConstructorType
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
                | SyntaxKind::TypeAliasDeclaration
                | SyntaxKind::MappedType
                | SyntaxKind::ConditionalType => {
                    let outer = self
                        .get_outer_type_parameters(node, include_this_types)?
                        .unwrap_or_default();
                    if (matches!(
                        kind,
                        SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
                    ) || self.is_object_literal_method(node))
                        && self.is_context_sensitive(node)
                    {
                        // 57052-57057: context-sensitive function
                        // expressions replay their CHECKED signature's
                        // type parameters (the contextual assignment
                        // may have instantiated them); others fall
                        // through to the declared-type-parameter
                        // append.
                        let symbol = self.get_symbol_of_declaration(node)?;
                        let ty = self.get_type_of_symbol(symbol)?;
                        let signatures = self
                            .get_signatures_of_type(ty, crate::structural::SignatureKind::Call)?;
                        if let Some(&first) = signatures.first() {
                            if let Some(type_parameters) =
                                self.signature_of(first).type_parameters.clone()
                            {
                                let mut result = outer;
                                result.extend(type_parameters.iter().copied());
                                return Ok(Some(result));
                            }
                        }
                    }
                    if kind == SyntaxKind::MappedType {
                        let NodeData::MappedType(data) = self.data_of(node) else {
                            unreachable!("MappedType kind implies payload");
                        };
                        let type_parameter = data.type_parameter.ok_or_else(|| {
                            Unsupported::new("mapped type with missing type parameter")
                        })?;
                        let symbol = self.node_symbol(type_parameter).ok_or_else(|| {
                            Unsupported::new("unbound mapped-type type parameter")
                        })?;
                        // getSymbolOfDeclaration (57051).
                        let symbol = self.get_merged_symbol(symbol);
                        let declared = self.get_declared_type_of_type_parameter(symbol);
                        let mut result = outer;
                        result.push(declared);
                        return Ok(Some(result));
                    }
                    if kind == SyntaxKind::ConditionalType {
                        let infer_parameters = self.get_infer_type_parameters(node);
                        let mut result = outer;
                        result.extend(infer_parameters);
                        return Ok(Some(result));
                    }
                    let declarations = self.type_parameter_declarations_of(node);
                    let mut outer_and_own = self.append_type_parameters(outer, &declarations);
                    if include_this_types
                        && matches!(
                            kind,
                            SyntaxKind::ClassDeclaration
                                | SyntaxKind::ClassExpression
                                | SyntaxKind::InterfaceDeclaration
                        )
                    {
                        // 57063-57066: append the declared type's
                        // thisType (GenericType shapes only — plain
                        // thisless interfaces carry none).
                        let symbol = self.node_symbol(node).ok_or_else(|| {
                            Unsupported::new("unbound class/interface declaration")
                        })?;
                        // getSymbolOfDeclaration (57065): the MERGED
                        // symbol owns the one true declared type.
                        let symbol = self.get_merged_symbol(symbol);
                        let declared = self.get_declared_type_of_class_or_interface(symbol)?;
                        if let TypeData::GenericType { this_type, .. } =
                            self.tables.type_of(declared).data
                        {
                            outer_and_own.push(this_type);
                        }
                    }
                    return Ok(Some(outer_and_own));
                }
                _ => {}
            }
        }
    }

    /// tsc-port: getInferTypeParameters @6.0.3
    /// tsc-hash: a727c630d4ac1b86e85eeef5f93c891ff28ac96696c47f2c4a0cf72b0552dd42
    /// tsc-span: _tsc.js:62756-62766
    fn get_infer_type_parameters(&mut self, node: NodeId) -> Vec<TypeId> {
        let Some(locals) = self.binder.locals_of(node) else {
            return Vec::new();
        };
        let symbols: Vec<SymbolId> = locals.values().copied().collect();
        let mut result = Vec::new();
        for symbol in symbols {
            if self
                .symbol_flags(symbol)
                .intersects(SymbolFlags::TYPE_PARAMETER)
            {
                result.push(self.get_declared_type_of_type_parameter(symbol));
            }
        }
        result
    }

    // ---- signature instantiation ----

    /// tsc-port: getSignatureInstantiation @6.0.3
    /// tsc-hash: 524fd5c11b79aba2d61c5d34ecf53ed5cad179d6d26bdbeed44c4a3bfcbee4a1
    /// tsc-span: _tsc.js:59886-59901
    ///
    /// `inferred_type_parameters` (instantiation-expression signatures)
    /// unwinds as Unsupported — its only producers are M6 inference
    /// contexts.
    pub fn get_signature_instantiation(
        &mut self,
        signature: SignatureId,
        type_arguments: Option<&[TypeId]>,
        is_javascript: bool,
        inferred_type_parameters: Option<&[TypeId]>,
    ) -> CheckResult2<SignatureId> {
        let type_parameters = self.signature_of(signature).type_parameters.clone();
        let min_type_argument_count = self.get_min_type_argument_count(type_parameters.as_deref());
        let filled = self.fill_missing_type_arguments(
            type_arguments,
            type_parameters.as_deref(),
            min_type_argument_count,
            is_javascript,
        )?;
        let instantiated = self.get_signature_instantiation_without_filling_in_type_arguments(
            signature,
            filled.as_deref(),
        )?;
        if inferred_type_parameters.is_some() {
            return Err(Unsupported::new(
                "instantiation-expression signatures (inferredTypeParameters, M6)",
            ));
        }
        Ok(instantiated)
    }

    /// tsc-port: getSignatureInstantiationWithoutFillingInTypeArguments @6.0.3
    /// tsc-hash: 03362d45cfd590dff1985e328708931e66df1b4d97c7473c41386b432b539c97
    /// tsc-span: _tsc.js:59902-59911
    pub fn get_signature_instantiation_without_filling_in_type_arguments(
        &mut self,
        signature: SignatureId,
        type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<SignatureId> {
        let key = self.tables.get_type_list_id(type_arguments.unwrap_or(&[]));
        if let Some(&existing) = self.signature_of(signature).instantiations.get(&key) {
            return Ok(existing);
        }
        let instantiation = self.create_signature_instantiation(signature, type_arguments)?;
        self.signatures[signature.0 as usize]
            .instantiations
            .insert(key, instantiation);
        Ok(instantiation)
    }

    /// tsc-port: createSignatureInstantiation @6.0.3
    /// tsc-hash: 30a64ba4b8cfa38a06629dff2b93e973e1665a3893b75df6827460068e5a6685
    /// tsc-span: _tsc.js:59912-59919
    pub(crate) fn create_signature_instantiation(
        &mut self,
        signature: SignatureId,
        type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<SignatureId> {
        let mapper = self.create_signature_type_mapper(signature, type_arguments)?;
        self.instantiate_signature(signature, mapper, /*erase_type_parameters*/ true)
    }

    /// tsc-port: getTypeParametersForMapper @6.0.3
    /// tsc-hash: cd7f932bd92c945acc5b3de55c7bc5fd2a29b02864a8bf93b0c535c4e955212f
    /// tsc-span: _tsc.js:59920-59922
    fn get_type_parameters_for_mapper(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult2<Vec<TypeId>> {
        let type_parameters = self
            .signature_of(signature)
            .type_parameters
            .clone()
            .unwrap_or_default();
        let mut result = Vec::with_capacity(type_parameters.len());
        for tp in type_parameters {
            let mapper = self.links.ty(tp).type_parameter_mapper;
            result.push(self.instantiate_type(tp, mapper)?);
        }
        Ok(result)
    }

    /// tsc-port: createSignatureTypeMapper @6.0.3
    /// tsc-hash: 2167072b0631c8334655915c6cfd0df93e0aae46f1287a790f9c77d7390c8f4b
    /// tsc-span: _tsc.js:59923-59925
    fn create_signature_type_mapper(
        &mut self,
        signature: SignatureId,
        type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<MapperId> {
        let sources = self.get_type_parameters_for_mapper(signature)?;
        Ok(self.create_type_mapper(sources, type_arguments.map(<[TypeId]>::to_vec)))
    }

    /// tsc-port: getErasedSignature @6.0.3
    /// tsc-hash: c99518238cd42ff436e62cf150c82253ed618397b6471b3ab1af5a9b2dcb1fb5
    /// tsc-span: _tsc.js:59926-59928
    pub fn get_erased_signature(&mut self, signature: SignatureId) -> CheckResult2<SignatureId> {
        if self.signature_of(signature).type_parameters.is_none() {
            return Ok(signature);
        }
        if let Some(cached) = self.signature_of(signature).erased_signature_cache {
            return Ok(cached);
        }
        let erased = self.create_erased_signature(signature)?;
        self.signatures[signature.0 as usize].erased_signature_cache = Some(erased);
        Ok(erased)
    }

    /// tsc-port: createErasedSignature @6.0.3
    /// tsc-hash: dde837ead02ad3156d3060c7b3fe69207380e9bd55d1739c4c8d45437d62e74c
    /// tsc-span: _tsc.js:59929-59936
    fn create_erased_signature(&mut self, signature: SignatureId) -> CheckResult2<SignatureId> {
        let type_parameters = self
            .signature_of(signature)
            .type_parameters
            .clone()
            .expect("getErasedSignature gates on typeParameters");
        let eraser = self.create_type_eraser(type_parameters);
        self.instantiate_signature(signature, eraser, /*erase_type_parameters*/ true)
    }

    /// tsc-port: getMinTypeArgumentCount @6.0.3
    /// tsc-hash: dfce3f35d224b2fe0221f8f2c43a41b8588d33695d4fe8a3185a89b6c92db63a
    /// tsc-span: _tsc.js:59534-59544
    pub fn get_min_type_argument_count(&self, type_parameters: Option<&[TypeId]>) -> usize {
        let mut min_type_argument_count = 0;
        if let Some(type_parameters) = type_parameters {
            for (i, &tp) in type_parameters.iter().enumerate() {
                if !self.has_type_parameter_default(tp) {
                    min_type_argument_count = i + 1;
                }
            }
        }
        min_type_argument_count
    }

    /// tsc-port: hasTypeParameterDefault @6.0.3
    /// tsc-hash: f35b775fc1102e338bea994785e68aed68d84ba53644e507c0b7bbac3ebc48c5
    /// tsc-span: _tsc.js:59068-59070
    fn has_type_parameter_default(&self, tp: TypeId) -> bool {
        let Some(symbol) = self.tables.type_of(tp).symbol else {
            return false;
        };
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .any(|&declaration| {
                matches!(
                    self.data_of(declaration),
                    NodeData::TypeParameter(data)
                        if self.kind_of(declaration) == SyntaxKind::TypeParameter
                            && data.r#default.is_some()
                )
            })
    }

    /// tsc-port: fillMissingTypeArguments @6.0.3
    /// tsc-hash: 351c698c8fa3fccd898708b4e8b00822609109bfa95b5dc7f7203adec3c8fe90
    /// tsc-span: _tsc.js:59545-59568
    pub fn fill_missing_type_arguments(
        &mut self,
        type_arguments: Option<&[TypeId]>,
        type_parameters: Option<&[TypeId]>,
        min_type_argument_count: usize,
        is_javascript_implicit_any: bool,
    ) -> CheckResult2<Option<Vec<TypeId>>> {
        let num_type_parameters = type_parameters.map_or(0, <[TypeId]>::len);
        if num_type_parameters == 0 {
            return Ok(Some(Vec::new()));
        }
        let num_type_arguments = type_arguments.map_or(0, <[TypeId]>::len);
        if is_javascript_implicit_any
            || (num_type_arguments >= min_type_argument_count
                && num_type_arguments <= num_type_parameters)
        {
            let type_parameters = type_parameters.expect("non-zero length implies a list");
            let mut result: Vec<TypeId> = type_arguments.map(<[_]>::to_vec).unwrap_or_default();
            result.resize(num_type_parameters, self.tables.intrinsics.error);
            let base_default_type = self.get_default_type_argument_type(is_javascript_implicit_any);
            for i in num_type_arguments..num_type_parameters {
                let mut default_type = self.get_default_from_type_parameter(type_parameters[i])?;
                if is_javascript_implicit_any {
                    if let Some(default) = default_type {
                        let unknown = self.tables.intrinsics.unknown;
                        let empty_object = self.empty_object_type;
                        if self.is_type_related_to(
                            default,
                            unknown,
                            crate::relate::RelationKind::Identity,
                        )? || self.is_type_related_to(
                            default,
                            empty_object,
                            crate::relate::RelationKind::Identity,
                        )? {
                            default_type = Some(self.tables.intrinsics.any);
                        }
                    }
                }
                result[i] = match default_type {
                    Some(default) => {
                        let mapper =
                            self.create_type_mapper(type_parameters.to_vec(), Some(result.clone()));
                        self.instantiate_type(default, Some(mapper))?
                    }
                    None => base_default_type,
                };
            }
            result.truncate(num_type_parameters);
            return Ok(Some(result));
        }
        Ok(type_arguments.map(<[TypeId]>::to_vec))
    }

    /// tsc-port: getDefaultTypeArgumentType @6.0.3
    /// tsc-hash: 9fc8c6ef773571fb9228871cebd6ec4a0b7769bc7f6c55dd8ada3443b8d81687
    /// tsc-span: _tsc.js:69314-69316
    fn get_default_type_argument_type(&self, is_in_javascript_file: bool) -> TypeId {
        if is_in_javascript_file {
            self.tables.intrinsics.any
        } else {
            self.tables.intrinsics.unknown
        }
    }

    /// tsc-port: getResolvedTypeParameterDefault @6.0.3
    /// tsc-hash: f4e1b3c6ebb2cf0add27d57636d43c98124be4590317dee655d4cdbcb2811af0
    /// tsc-span: _tsc.js:59043-59062
    ///
    /// tsc's resolvingDefaultType in-flight sentinel is the checker's
    /// in-progress set: re-entry stamps circularConstraintType into the
    /// links slot (permanently, like tsc), the outer frame keeps a
    /// re-entry stamp over its own result, and an Err unwind leaves the
    /// slot Vacant (re-queryable).
    pub(crate) fn get_resolved_type_parameter_default(
        &mut self,
        tp: TypeId,
    ) -> CheckResult2<TypeId> {
        if let Some(resolved) = self.links.ty(tp).type_parameter_default.resolved() {
            return Ok(resolved);
        }
        if self.type_parameter_defaults_in_progress.contains(&tp) {
            let circular = self.circular_constraint_type;
            self.links
                .set_type_parameter_default(self.speculation_depth, tp, circular);
            return Ok(circular);
        }
        if let Some(target) = self.links.ty(tp).type_parameter_target {
            let target_default = self.get_resolved_type_parameter_default(target)?;
            // tsc instantiates the sentinel types too — they carry no
            // type variables, so this is the identity for them.
            let mapper = self.links.ty(tp).type_parameter_mapper;
            let default = self.instantiate_type(target_default, mapper)?;
            self.links
                .set_type_parameter_default(self.speculation_depth, tp, default);
            return Ok(default);
        }
        self.type_parameter_defaults_in_progress.push(tp);
        let default_declaration = self.tables.type_of(tp).symbol.and_then(|symbol| {
            self.binder
                .symbol(symbol)
                .declarations
                .clone()
                .into_iter()
                .find_map(|declaration| match self.data_of(declaration) {
                    NodeData::TypeParameter(data)
                        if self.kind_of(declaration) == SyntaxKind::TypeParameter =>
                    {
                        data.r#default
                    }
                    _ => None,
                })
        });
        let computed = match default_declaration {
            Some(declaration) => self.get_type_from_type_node(declaration),
            None => Ok(self.no_constraint_type),
        };
        self.type_parameter_defaults_in_progress.pop();
        let default_type = computed?;
        // A re-entry may have stamped the circular sentinel while the
        // default resolved — tsc keeps the stamp (59057-59059).
        if let Some(stamped) = self.links.ty(tp).type_parameter_default.resolved() {
            return Ok(stamped);
        }
        self.links
            .set_type_parameter_default(self.speculation_depth, tp, default_type);
        Ok(default_type)
    }

    /// tsc-port: getDefaultFromTypeParameter @6.0.3
    /// tsc-hash: bf3dd4b9c8399461bc5fdc2eb38cf01bedd9d4d55503d48e6a33c378b8f0cbf7
    /// tsc-span: _tsc.js:59061-59064
    pub(crate) fn get_default_from_type_parameter(
        &mut self,
        tp: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        let default_type = self.get_resolved_type_parameter_default(tp)?;
        Ok((default_type != self.no_constraint_type
            && default_type != self.circular_constraint_type)
            .then_some(default_type))
    }

    // ---- mapType + string mapping ----

    /// tsc-port: mapType @6.0.3
    /// tsc-hash: 60df9dff52551306badb91844d59d835b9533cb48b538b3f9135664dfa87c3ad
    /// tsc-span: _tsc.js:70028-70051
    pub fn map_type(
        &mut self,
        ty: TypeId,
        mapper: &mut dyn FnMut(&mut Self, TypeId) -> CheckResult2<Option<TypeId>>,
        no_reductions: bool,
    ) -> CheckResult2<Option<TypeId>> {
        if self.tables.flags_of(ty).intersects(TypeFlags::NEVER) {
            return Ok(Some(ty));
        }
        if !self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            return mapper(self, ty);
        }
        let (origin, own_types) = match &self.tables.type_of(ty).data {
            TypeData::Union { types, origin } => (*origin, types.to_vec()),
            _ => unreachable!("union flag implies union data"),
        };
        let types: Vec<TypeId> = match origin {
            Some(origin) if self.tables.flags_of(origin).intersects(TypeFlags::UNION) => {
                match &self.tables.type_of(origin).data {
                    TypeData::Union { types, .. } => types.to_vec(),
                    _ => unreachable!("union flag implies union data"),
                }
            }
            _ => own_types,
        };
        let mut mapped_types: Vec<TypeId> = Vec::new();
        let mut changed = false;
        for t in types {
            let mapped = if self.tables.flags_of(t).intersects(TypeFlags::UNION) {
                self.map_type(t, mapper, no_reductions)?
            } else {
                mapper(self, t)?
            };
            changed = changed || mapped != Some(t);
            if let Some(mapped) = mapped {
                mapped_types.push(mapped);
            }
        }
        if !changed {
            return Ok(Some(ty));
        }
        if mapped_types.is_empty() {
            return Ok(None);
        }
        let reduction = if no_reductions {
            UnionReduction::None
        } else {
            UnionReduction::Literal
        };
        Ok(Some(self.get_union_type_ex(&mapped_types, reduction)?))
    }

    /// tsc-port: getStringMappingType @6.0.3
    /// tsc-hash: 2a62cdc3f0f0656ad0c8eb91482fd7c48208dd263861d26d297dd9634a534857
    /// tsc-span: _tsc.js:62119-62128
    pub fn get_string_mapping_type(
        &mut self,
        symbol: SymbolId,
        ty: TypeId,
    ) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNION | TypeFlags::NEVER) {
            let mapped = self.map_type(
                ty,
                &mut |state, t| state.get_string_mapping_type(symbol, t).map(Some),
                /*no_reductions*/ false,
            )?;
            return Ok(mapped.expect("string mapping over unions never drops members"));
        }
        if flags.intersects(TypeFlags::STRING_LITERAL) {
            let value = match &self.tables.type_of(ty).data {
                TypeData::Literal {
                    value: tsrs2_types::LiteralValue::String(value),
                } => value.clone(),
                _ => unreachable!("string-literal flag implies string payload"),
            };
            let name = self.binder.symbol(symbol).escaped_name.clone();
            let mapped = apply_string_mapping(intrinsic_type_kind(&name), &value);
            return Ok(self.tables.get_string_literal_type(&mapped));
        }
        if flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
            let (texts, types) = match &self.tables.type_of(ty).data {
                TypeData::TemplateLiteral { texts, types } => (texts.to_vec(), types.to_vec()),
                _ => unreachable!("template flag implies template data"),
            };
            let (new_texts, new_types) =
                self.apply_template_string_mapping(symbol, texts, types)?;
            return Ok(self
                .tables
                .get_template_literal_type(&new_texts, &new_types));
        }
        // Mapping<Mapping<T>> === Mapping<T>
        if flags.intersects(TypeFlags::STRING_MAPPING)
            && Some(symbol) == self.tables.type_of(ty).symbol
        {
            return Ok(ty);
        }
        if flags.intersects(TypeFlags::ANY | TypeFlags::STRING | TypeFlags::STRING_MAPPING)
            || self.tables.is_generic_index_type(ty)
        {
            return Ok(self
                .tables
                .get_string_mapping_type_for_generic_type(symbol, ty));
        }
        // Mapping<`${number}`> / Mapping<`${bigint}`>
        if self.tables.is_pattern_literal_placeholder_type(ty) {
            let template = self
                .tables
                .get_template_literal_type(&[String::new(), String::new()], &[ty]);
            return Ok(self
                .tables
                .get_string_mapping_type_for_generic_type(symbol, template));
        }
        Ok(ty)
    }

    /// tsc-port: applyTemplateStringMapping @6.0.3
    /// tsc-hash: 4f34fe8f83e6b5fc210446cb6f574580374cef057031552a0e64c407a372f2c5
    /// tsc-span: _tsc.js:62142-62153
    fn apply_template_string_mapping(
        &mut self,
        symbol: SymbolId,
        texts: Vec<String>,
        types: Vec<TypeId>,
    ) -> CheckResult2<(Vec<String>, Vec<TypeId>)> {
        let name = self.binder.symbol(symbol).escaped_name.clone();
        match intrinsic_type_kind(&name) {
            Some(IntrinsicTypeKind::Uppercase) | Some(IntrinsicTypeKind::Lowercase) => {
                let upper = intrinsic_type_kind(&name) == Some(IntrinsicTypeKind::Uppercase);
                let new_texts = texts
                    .iter()
                    .map(|text| {
                        if upper {
                            js_to_upper_case(text)
                        } else {
                            js_to_lower_case(text)
                        }
                    })
                    .collect();
                let mut new_types = Vec::with_capacity(types.len());
                for t in types {
                    new_types.push(self.get_string_mapping_type(symbol, t)?);
                }
                Ok((new_texts, new_types))
            }
            Some(IntrinsicTypeKind::Capitalize) | Some(IntrinsicTypeKind::Uncapitalize) => {
                let upper = intrinsic_type_kind(&name) == Some(IntrinsicTypeKind::Capitalize);
                if texts[0].is_empty() {
                    let mut new_types = types.clone();
                    new_types[0] = self.get_string_mapping_type(symbol, types[0])?;
                    Ok((texts, new_types))
                } else {
                    let mut new_texts = texts.clone();
                    new_texts[0] = js_capitalize(&texts[0], upper);
                    Ok((new_texts, types))
                }
            }
            _ => Ok((texts, types)),
        }
    }

    /// tsc-port: isTypeMatchedByTemplateLiteralOrStringMapping @6.0.3
    /// tsc-hash: 17932b7f92e200b7fde380e72dfcd2adb1dcd7b62e47544505c9153217a00149
    /// tsc-span: _tsc.js:61447-61449
    pub(crate) fn is_type_matched_by_template_literal_or_string_mapping(
        &mut self,
        ty: TypeId,
        template: TypeId,
    ) -> CheckResult2<bool> {
        if self
            .tables
            .flags_of(template)
            .intersects(TypeFlags::TEMPLATE_LITERAL)
        {
            self.is_type_matched_by_template_literal_type(ty, template)
        } else {
            self.is_member_of_string_mapping(ty, template)
        }
    }

    /// tsc-port: isMemberOfStringMapping @6.0.3
    /// tsc-hash: 2bb2682a95b2fa40480413f2a14136954a0774c53c23b8a95402e776550ccc42
    /// tsc-span: _tsc.js:68532-68549
    pub(crate) fn is_member_of_string_mapping(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<bool> {
        let target_flags = self.tables.flags_of(target);
        if target_flags.intersects(TypeFlags::ANY) {
            return Ok(true);
        }
        if target_flags.intersects(TypeFlags::STRING | TypeFlags::TEMPLATE_LITERAL) {
            return self.is_type_assignable_to(source, target);
        }
        if target_flags.intersects(TypeFlags::STRING_MAPPING) {
            let mut mapping_stack: Vec<SymbolId> = Vec::new();
            let mut inner = target;
            while self
                .tables
                .flags_of(inner)
                .intersects(TypeFlags::STRING_MAPPING)
            {
                let symbol = self
                    .tables
                    .type_of(inner)
                    .symbol
                    .expect("string-mapping types carry the intrinsic symbol");
                mapping_stack.insert(0, symbol);
                let TypeData::StringMapping { ty } = self.tables.type_of(inner).data else {
                    unreachable!("string-mapping flag implies string-mapping data");
                };
                inner = ty;
            }
            let mut mapped_source = source;
            for symbol in mapping_stack {
                mapped_source = self.get_string_mapping_type(symbol, mapped_source)?;
            }
            return Ok(mapped_source == source && self.is_member_of_string_mapping(source, inner)?);
        }
        Ok(false)
    }

    // ---- node utilities for the containsReference walker ----

    pub(crate) fn children_of(&self, node: NodeId) -> Vec<NodeId> {
        let source = self.binder.source_of_node(node);
        let mut children = Vec::new();
        for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        children
    }

    /// tsc-port: isThisIdentifier @6.0.3
    /// tsc-hash: 3ce9be1698a352dc46a5450b942102356b37b37b66e694fb3b39e6eea72302f7
    /// tsc-span: _tsc.js:16698-16700
    pub(crate) fn is_this_identifier(&self, node: NodeId) -> bool {
        self.kind_of(node) == SyntaxKind::Identifier
            && self.identifier_text_of(node) == Some("this")
    }

    /// tsc-port: isNodeDescendantOf @6.0.3
    /// tsc-hash: 4fecae1da46379a5f34e9780afd471333c5b1f528f936788b1b0e19b4c85b026
    /// tsc-span: _tsc.js:15672-15678
    pub(crate) fn is_node_descendant_of(&self, node: NodeId, ancestor: NodeId) -> bool {
        let mut current = Some(node);
        while let Some(node) = current {
            if node == ancestor {
                return true;
            }
            current = self.parent_of(node);
        }
        false
    }

    /// tsc-port: isPartOfTypeNode @6.0.3
    /// tsc-hash: 55a9d46da576728e248042fc0ca94da1daed14019326a3d28b945c2fcfdea441
    /// tsc-span: _tsc.js:14190-14271
    ///
    /// JSDoc arms (JSDocTemplateTag constraint, the implements/augments
    /// tag checks inside isPartOfTypeExpressionWithTypeArguments) are
    /// elided project-wide; the heritage-clause check keeps the
    /// non-extends-expression semantics.
    pub(crate) fn is_part_of_type_node(&self, node: NodeId) -> bool {
        let kind = self.kind_of(node);
        if SyntaxKind::FirstTypeNode <= kind && kind <= SyntaxKind::LastTypeNode {
            return true;
        }
        match kind {
            SyntaxKind::AnyKeyword
            | SyntaxKind::UnknownKeyword
            | SyntaxKind::NumberKeyword
            | SyntaxKind::BigIntKeyword
            | SyntaxKind::StringKeyword
            | SyntaxKind::BooleanKeyword
            | SyntaxKind::SymbolKeyword
            | SyntaxKind::ObjectKeyword
            | SyntaxKind::UndefinedKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::NeverKeyword => true,
            SyntaxKind::VoidKeyword => self
                .parent_of(node)
                .is_some_and(|parent| self.kind_of(parent) != SyntaxKind::VoidExpression),
            SyntaxKind::ExpressionWithTypeArguments => {
                self.is_part_of_type_expression_with_type_arguments(node)
            }
            SyntaxKind::TypeParameter => self.parent_of(node).is_some_and(|parent| {
                matches!(
                    self.kind_of(parent),
                    SyntaxKind::MappedType | SyntaxKind::InferType
                )
            }),
            SyntaxKind::Identifier
            | SyntaxKind::QualifiedName
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ThisKeyword => {
                let mut node = node;
                if kind == SyntaxKind::Identifier {
                    if let Some(parent) = self.parent_of(node) {
                        match self.data_of(parent) {
                            NodeData::QualifiedName(data) if data.right == Some(node) => {
                                node = parent;
                            }
                            NodeData::PropertyAccessExpression(data) if data.name == Some(node) => {
                                node = parent;
                            }
                            _ => {}
                        }
                    }
                }
                let Some(parent) = self.parent_of(node) else {
                    return false;
                };
                let parent_kind = self.kind_of(parent);
                if parent_kind == SyntaxKind::TypeQuery {
                    return false;
                }
                if parent_kind == SyntaxKind::ImportType {
                    // The parser keeps no isTypeOf marker (ImportTypeData
                    // has no home for it) — recover it from source text
                    // like the heritage-token recovery in resolveName.
                    return !self.import_type_is_type_of(parent);
                }
                if SyntaxKind::FirstTypeNode <= parent_kind
                    && parent_kind <= SyntaxKind::LastTypeNode
                {
                    return true;
                }
                match parent_kind {
                    SyntaxKind::ExpressionWithTypeArguments => {
                        self.is_part_of_type_expression_with_type_arguments(parent)
                    }
                    SyntaxKind::TypeParameter => {
                        matches!(
                            self.data_of(parent),
                            NodeData::TypeParameter(data) if data.constraint == Some(node)
                        )
                    }
                    SyntaxKind::PropertyDeclaration
                    | SyntaxKind::PropertySignature
                    | SyntaxKind::Parameter
                    | SyntaxKind::VariableDeclaration
                    | SyntaxKind::FunctionDeclaration
                    | SyntaxKind::FunctionExpression
                    | SyntaxKind::ArrowFunction
                    | SyntaxKind::Constructor
                    | SyntaxKind::MethodDeclaration
                    | SyntaxKind::MethodSignature
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor
                    | SyntaxKind::CallSignature
                    | SyntaxKind::ConstructSignature
                    | SyntaxKind::IndexSignature
                    | SyntaxKind::TypeAssertionExpression => {
                        self.type_annotation_of(parent) == Some(node)
                    }
                    SyntaxKind::CallExpression
                    | SyntaxKind::NewExpression
                    | SyntaxKind::TaggedTemplateExpression => {
                        let type_arguments = match self.data_of(parent) {
                            NodeData::CallExpression(data) => data.type_arguments,
                            NodeData::NewExpression(data) => data.type_arguments,
                            NodeData::TaggedTemplateExpression(data) => data.type_arguments,
                            _ => None,
                        };
                        self.nodes_of(type_arguments).contains(&node)
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// isPartOfTypeExpressionWithTypeArguments (14272-14274), JSDoc
    /// arms elided: a heritage-clause member that is NOT the extends
    /// expression of a class.
    fn is_part_of_type_expression_with_type_arguments(&self, node: NodeId) -> bool {
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        if self.kind_of(parent) != SyntaxKind::HeritageClause {
            return false;
        }
        !self.is_expression_with_type_arguments_in_class_extends_clause(node)
    }

    /// isExpressionWithTypeArgumentsInClassExtendsClause (utilities):
    /// heritage token recovered from source text (HeritageClauseData
    /// stores no token — same recovery as resolveName's heritage walk).
    fn is_expression_with_type_arguments_in_class_extends_clause(&self, node: NodeId) -> bool {
        let Some(clause) = self.parent_of(node) else {
            return false;
        };
        if self.kind_of(clause) != SyntaxKind::HeritageClause {
            return false;
        }
        let Some(container) = self.parent_of(clause) else {
            return false;
        };
        let is_class = matches!(
            self.kind_of(container),
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
        );
        is_class && self.heritage_clause_is_extends(clause)
    }

    fn import_type_is_type_of(&self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        let node = source.arena.node(node);
        source.text[node.pos as usize..node.end as usize]
            .trim_start()
            .starts_with("typeof")
    }
}

/// tsc-port: applyStringMapping @6.0.3
/// tsc-hash: 03303706c2ff1cce6253350ab983e924aec70581fee678721dfdeeaf6e680e72
/// tsc-span: _tsc.js:62129-62141
fn apply_string_mapping(kind: Option<IntrinsicTypeKind>, value: &str) -> String {
    match kind {
        Some(IntrinsicTypeKind::Uppercase) => js_to_upper_case(value),
        Some(IntrinsicTypeKind::Lowercase) => js_to_lower_case(value),
        Some(IntrinsicTypeKind::Capitalize) => js_capitalize(value, true),
        Some(IntrinsicTypeKind::Uncapitalize) => js_capitalize(value, false),
        _ => value.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::{CompilerOptions, ObjectFlags, SymbolFlags, TypeFlags, UnionReduction};

    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    fn annotation_of_var(state: &CheckerState, name: &str) -> tsrs2_syntax::NodeId {
        crate::relpin::find_probe_annotation(state.binder.source(0), name)
            .expect("var with annotation")
    }

    fn declared_type_parameter_at(
        state: &mut CheckerState,
        inside: tsrs2_syntax::NodeId,
        name: &str,
    ) -> tsrs2_types::TypeId {
        let symbol = state
            .resolve_name(
                Some(inside),
                name,
                SymbolFlags::TYPE_PARAMETER,
                None,
                false,
                false,
            )
            .expect("type parameter resolves");
        state.get_declared_type_of_type_parameter(symbol)
    }

    fn declared_type_parameter(state: &mut CheckerState, name: &str) -> tsrs2_types::TypeId {
        let source = state.binder.source(0);
        let inside = source
            .arena
            .node_ids()
            .find(|&id| source.arena.node(id).kind == tsrs2_syntax::SyntaxKind::VariableDeclaration)
            .expect("var declaration");
        declared_type_parameter_at(state, inside, name)
    }

    #[test]
    fn union_instantiation_maps_parameters_and_keeps_identity() {
        with_program_state(
            &[(
                "a.ts",
                "function f<T, U>() { var v: T | number; var w: string | number; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let annotation = annotation_of_var(state, "v");
                let union = state.get_type_from_type_node(annotation).expect("union");
                let t = declared_type_parameter(state, "T");
                let u = declared_type_parameter(state, "U");
                assert!(state.could_contain_type_variables(union));
                let string = state.tables.intrinsics.string;
                let mapper = state.create_type_mapper(vec![t], Some(vec![string]));
                let mapped = state
                    .instantiate_type(union, Some(mapper))
                    .expect("instantiation in slice");
                let expected_annotation = annotation_of_var(state, "w");
                let expected = state
                    .get_type_from_type_node(expected_annotation)
                    .expect("string | number");
                assert_eq!(mapped, expected, "T|number [T:=string] is string|number");
                // A mapper over an unreferenced parameter maps nothing:
                // tsc returns the SAME type object.
                let unrelated = state.create_type_mapper(vec![u], Some(vec![string]));
                let unchanged = state
                    .instantiate_type(union, Some(unrelated))
                    .expect("instantiation in slice");
                assert_eq!(unchanged, union);
            },
        );
    }

    #[test]
    fn template_literal_over_type_parameter_interns_and_instantiates() {
        with_program_state(
            &[(
                "a.ts",
                "function f<T extends string>() { var v: `a${T}`; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let annotation = annotation_of_var(state, "v");
                let template = state.get_type_from_type_node(annotation).expect("template");
                // Regression: the tables isGenericIndexType stub used to
                // collapse `a${T}` to string.
                assert!(
                    state
                        .tables
                        .flags_of(template)
                        .intersects(TypeFlags::TEMPLATE_LITERAL),
                    "generic span keeps the template literal shape"
                );
                let t = declared_type_parameter(state, "T");
                let x = state.tables.get_string_literal_type("x");
                let mapper = state.create_type_mapper(vec![t], Some(vec![x]));
                let mapped = state
                    .instantiate_type(template, Some(mapper))
                    .expect("instantiation in slice");
                let expected = state.tables.get_string_literal_type("ax");
                assert_eq!(mapped, expected);
            },
        );
    }

    #[test]
    fn tuple_reference_instantiation_reuses_the_interned_reference() {
        with_program_state(
            &[(
                "a.ts",
                "function f<T>() { var v: [T, string]; var w: [number, string]; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let annotation = annotation_of_var(state, "v");
                let tuple = state.get_type_from_type_node(annotation).expect("tuple");
                let t = declared_type_parameter(state, "T");
                let number = state.tables.intrinsics.number;
                let mapper = state.create_type_mapper(vec![t], Some(vec![number]));
                let mapped = state
                    .instantiate_type(tuple, Some(mapper))
                    .expect("instantiation in slice");
                let expected_annotation = annotation_of_var(state, "w");
                let expected = state
                    .get_type_from_type_node(expected_annotation)
                    .expect("[number, string]");
                assert_eq!(mapped, expected);
            },
        );
    }

    #[test]
    fn anonymous_type_instantiation_creates_a_cached_shell() {
        with_program_state(
            &[("a.ts", "function f<T, U>() { var v: { a: T }; }\n")],
            &CompilerOptions::default(),
            |state| {
                let annotation = annotation_of_var(state, "v");
                let anonymous = state
                    .get_type_from_type_node(annotation)
                    .expect("type literal");
                let t = declared_type_parameter(state, "T");
                let u = declared_type_parameter(state, "U");
                let string = state.tables.intrinsics.string;
                let mapper = state.create_type_mapper(vec![t], Some(vec![string]));
                let shell = state
                    .instantiate_type(anonymous, Some(mapper))
                    .expect("instantiation in slice");
                assert_ne!(shell, anonymous);
                assert!(state
                    .tables
                    .object_flags_of(shell)
                    .intersects(ObjectFlags::INSTANTIATED));
                assert_eq!(state.links.ty(shell).instantiated_target, Some(anonymous));
                // Same type arguments -> the interned instantiation.
                let mapper2 = state.create_type_mapper(vec![t], Some(vec![string]));
                let again = state
                    .instantiate_type(anonymous, Some(mapper2))
                    .expect("instantiation in slice");
                assert_eq!(again, shell);
                // A Block sits between the type literal and the type-
                // parameter container, so isTypeParameterPossiblyReferenced
                // answers true for U as well (63527-63529): a U-only
                // mapper still mints a (distinct) instantiation, exactly
                // like tsc.
                let unrelated = state.create_type_mapper(vec![u], Some(vec![string]));
                let u_shell = state
                    .instantiate_type(anonymous, Some(unrelated))
                    .expect("instantiation in slice");
                assert_ne!(u_shell, anonymous);
                assert_ne!(u_shell, shell);
                // Member resolution of the shell reads the target's
                // properties through the mapper (5.3a): `a: T` lands as
                // `a: string`.
                let members = state
                    .resolve_structured_type_members(shell)
                    .expect("instantiated members resolve");
                let properties = state.members_of(members).properties.clone();
                assert_eq!(properties.len(), 1);
                let property_type = state
                    .get_type_of_symbol(properties[0])
                    .expect("instantiated property type");
                assert_eq!(property_type, string);
            },
        );
    }

    #[test]
    fn unreferenced_outer_parameters_are_filtered_without_a_block() {
        with_program_state(
            &[("a.ts", "declare function f<T, U>(): { a: T };\n")],
            &CompilerOptions::default(),
            |state| {
                let source = state.binder.source(0);
                let literal_node = source
                    .arena
                    .node_ids()
                    .find(|&id| source.arena.node(id).kind == tsrs2_syntax::SyntaxKind::TypeLiteral)
                    .expect("type literal");
                let anonymous = state
                    .get_type_from_type_node(literal_node)
                    .expect("type literal type");
                let t = declared_type_parameter_at(state, literal_node, "T");
                let u = declared_type_parameter_at(state, literal_node, "U");
                let string = state.tables.intrinsics.string;
                // No Block intervenes: containsReference filters U out,
                // so a U-only mapper hits the seeded self entry.
                let unrelated = state.create_type_mapper(vec![u], Some(vec![string]));
                let unchanged = state
                    .instantiate_type(anonymous, Some(unrelated))
                    .expect("instantiation in slice");
                assert_eq!(unchanged, anonymous);
                // T stays: a T mapper mints the shell.
                let mapper = state.create_type_mapper(vec![t], Some(vec![string]));
                let shell = state
                    .instantiate_type(anonymous, Some(mapper))
                    .expect("instantiation in slice");
                assert_ne!(shell, anonymous);
            },
        );
    }

    #[test]
    fn erased_and_argument_instantiated_signatures_map_lazily() {
        with_program_state(
            &[("a.ts", "function f<T>() { var v: (x: T) => T; }\n")],
            &CompilerOptions::default(),
            |state| {
                let annotation = annotation_of_var(state, "v");
                let base = state
                    .get_signature_from_declaration(annotation)
                    .expect("function-type signature");
                let t = declared_type_parameter(state, "T");
                // Promote to a generic signature by hand — generic
                // getSignatureFromDeclaration lands with the follow-up.
                let mut generic = state.signature_of(base).clone();
                generic.type_parameters = Some(vec![t]);
                let generic = state.alloc_signature(generic);

                let erased = state.get_erased_signature(generic).expect("erased");
                assert_ne!(erased, generic);
                let erased_return = state
                    .get_return_type_of_signature(erased)
                    .expect("erased return");
                assert_eq!(erased_return, state.tables.intrinsics.any);
                let erased_parameter = state.signature_of(erased).parameters[0];
                let erased_parameter_type = state
                    .get_type_of_symbol(erased_parameter)
                    .expect("instantiated parameter type");
                assert_eq!(erased_parameter_type, state.tables.intrinsics.any);
                // The erased signature is cached.
                assert_eq!(state.get_erased_signature(generic).expect("cached"), erased);

                let string = state.tables.intrinsics.string;
                let instantiated = state
                    .get_signature_instantiation(generic, Some(&[string]), false, None)
                    .expect("signature instantiation");
                let instantiated_return = state
                    .get_return_type_of_signature(instantiated)
                    .expect("instantiated return");
                assert_eq!(instantiated_return, string);
                let parameter = state.signature_of(instantiated).parameters[0];
                let parameter_type = state
                    .get_type_of_symbol(parameter)
                    .expect("instantiated parameter type");
                assert_eq!(parameter_type, string);
                // Interned per type-argument list.
                let again = state
                    .get_signature_instantiation(generic, Some(&[string]), false, None)
                    .expect("signature instantiation");
                assert_eq!(again, instantiated);
            },
        );
    }

    #[test]
    fn string_mapping_applies_to_literals_unions_and_generics() {
        with_program_state(
            &[("a.ts", "function f<T extends string>() { var v: T; }\n")],
            &CompilerOptions::default(),
            |state| {
                let uppercase = state
                    .binder
                    .create_symbol(SymbolFlags::TYPE_ALIAS, "Uppercase".to_owned());
                let capitalize = state
                    .binder
                    .create_symbol(SymbolFlags::TYPE_ALIAS, "Capitalize".to_owned());
                let abc = state.tables.get_string_literal_type("abc");
                let mapped = state
                    .get_string_mapping_type(uppercase, abc)
                    .expect("literal mapping");
                assert_eq!(mapped, state.tables.get_string_literal_type("ABC"));
                // charAt(0)-faithful Capitalize: ß expands to SS.
                let eszett = state.tables.get_string_literal_type("ßoo");
                let capitalized = state
                    .get_string_mapping_type(capitalize, eszett)
                    .expect("literal mapping");
                assert_eq!(capitalized, state.tables.get_string_literal_type("SSoo"));
                // Unions map member-wise.
                let a = state.tables.get_string_literal_type("a");
                let b = state.tables.get_string_literal_type("b");
                let union = state
                    .get_union_type_ex(&[a, b], UnionReduction::Literal)
                    .expect("union");
                let mapped_union = state
                    .get_string_mapping_type(uppercase, union)
                    .expect("union mapping");
                let upper_a = state.tables.get_string_literal_type("A");
                let upper_b = state.tables.get_string_literal_type("B");
                let expected = state
                    .get_union_type_ex(&[upper_a, upper_b], UnionReduction::Literal)
                    .expect("union");
                assert_eq!(mapped_union, expected);
                // Generic operands intern a StringMapping type;
                // instantiation maps through it.
                let t = declared_type_parameter(state, "T");
                let generic = state
                    .get_string_mapping_type(uppercase, t)
                    .expect("generic mapping");
                assert!(state
                    .tables
                    .flags_of(generic)
                    .intersects(TypeFlags::STRING_MAPPING));
                let again = state
                    .get_string_mapping_type(uppercase, t)
                    .expect("generic mapping");
                assert_eq!(again, generic, "stringMappingTypes interning");
                let foo = state.tables.get_string_literal_type("foo");
                let mapper = state.create_type_mapper(vec![t], Some(vec![foo]));
                let instantiated = state
                    .instantiate_type(generic, Some(mapper))
                    .expect("instantiation in slice");
                assert_eq!(instantiated, state.tables.get_string_literal_type("FOO"));
                // Mapping<Mapping<T>> === Mapping<T>.
                let doubled = state
                    .get_string_mapping_type(uppercase, generic)
                    .expect("idempotent mapping");
                assert_eq!(doubled, generic);
            },
        );
    }

    #[test]
    fn string_mapping_relations_and_constraints() {
        with_program_state(
            &[("a.ts", "function f<T extends string>() { var v: T; }\n")],
            &CompilerOptions::default(),
            |state| {
                let uppercase = state
                    .binder
                    .create_symbol(SymbolFlags::TYPE_ALIAS, "Uppercase".to_owned());
                let lowercase = state
                    .binder
                    .create_symbol(SymbolFlags::TYPE_ALIAS, "Lowercase".to_owned());
                let string = state.tables.intrinsics.string;
                let upper_string = state
                    .get_string_mapping_type(uppercase, string)
                    .expect("Uppercase<string>");
                let lower_string = state
                    .get_string_mapping_type(lowercase, string)
                    .expect("Lowercase<string>");
                let foo_upper = state.tables.get_string_literal_type("FOO");
                let foo_lower = state.tables.get_string_literal_type("foo");
                assert_eq!(
                    state.is_type_assignable_to(foo_upper, upper_string),
                    Ok(true),
                    "\"FOO\" is a member of Uppercase<string>"
                );
                assert_eq!(
                    state.is_member_of_string_mapping(foo_lower, upper_string),
                    Ok(false),
                    "\"foo\" is not a member of Uppercase<string>"
                );
                assert_eq!(
                    state.is_type_assignable_to(upper_string, string),
                    Ok(true),
                    "Uppercase<string> relates through its base constraint"
                );
                assert_eq!(
                    state.is_type_assignable_to(upper_string, lower_string),
                    Ok(false),
                    "different intrinsics are unrelated"
                );
                // computeBaseConstraint: Uppercase<T> -> Uppercase<string>.
                let t = declared_type_parameter(state, "T");
                let upper_t = state
                    .get_string_mapping_type(uppercase, t)
                    .expect("Uppercase<T>");
                let constraint = state
                    .get_base_constraint_of_type(upper_t)
                    .expect("constraint in slice");
                assert_eq!(constraint, Some(upper_string));
                // Union reduction drops literals matched by mappings.
                let union = state
                    .get_union_type_ex(&[foo_upper, upper_string], UnionReduction::Literal)
                    .expect("union");
                assert_eq!(union, upper_string, "\"FOO\" | Uppercase<string> reduces");
            },
        );
    }

    #[test]
    fn permissive_and_restrictive_instantiations() {
        with_program_state(
            &[("a.ts", "function f<T extends string, U>() { var v: T; }\n")],
            &CompilerOptions::default(),
            |state| {
                let t = declared_type_parameter(state, "T");
                let u = declared_type_parameter(state, "U");
                let permissive = state
                    .get_permissive_instantiation(t)
                    .expect("permissive in slice");
                assert_eq!(permissive, state.tables.intrinsics.wildcard);
                let restrictive = state
                    .get_restrictive_instantiation(t)
                    .expect("restrictive in slice");
                assert_ne!(restrictive, t, "constrained parameters get a fresh twin");
                assert_eq!(
                    state.get_constraint_from_type_parameter(restrictive),
                    Ok(None),
                    "the twin's constraint is the noConstraint sentinel"
                );
                let restrictive_again = state
                    .get_restrictive_instantiation(t)
                    .expect("restrictive cached");
                assert_eq!(restrictive_again, restrictive);
                // Unconstrained parameters ARE their restrictive form.
                let u_restrictive = state
                    .get_restrictive_instantiation(u)
                    .expect("restrictive in slice");
                assert_eq!(u_restrictive, u);
            },
        );
    }

    #[test]
    fn cloned_type_parameters_instantiate_their_target_constraint() {
        with_program_state(
            &[(
                "a.ts",
                "function f<T extends string>() { var v: (x: T) => T; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let annotation = annotation_of_var(state, "v");
                let base = state
                    .get_signature_from_declaration(annotation)
                    .expect("function-type signature");
                let t = declared_type_parameter(state, "T");
                let mut generic = state.signature_of(base).clone();
                generic.type_parameters = Some(vec![t]);
                let generic = state.alloc_signature(generic);
                // A non-erasing instantiation clones the parameters.
                let identity = state.create_type_mapper(vec![t], Some(vec![t]));
                let cloned_signature = state
                    .instantiate_signature(generic, identity, /*erase*/ false)
                    .expect("instantiation in slice");
                let fresh = state
                    .signature_of(cloned_signature)
                    .type_parameters
                    .clone()
                    .expect("fresh type parameters")[0];
                assert_ne!(fresh, t);
                assert_eq!(state.links.ty(fresh).type_parameter_target, Some(t));
                let constraint = state
                    .get_constraint_from_type_parameter(fresh)
                    .expect("constraint in slice");
                assert_eq!(constraint, Some(state.tables.intrinsics.string));
            },
        );
    }
}

#[cfg(test)]
mod class_container_tests {
    use tsrs2_types::{CompilerOptions, ObjectFlags, SymbolFlags, TypeData};

    use crate::state::test_support::with_program_state;

    #[test]
    fn type_literals_inside_class_bodies_instantiate_with_this_type_filtered() {
        with_program_state(
            &[("a.ts", "class C<T> { p: { a: T } }\n")],
            &CompilerOptions::default(),
            |state| {
                let source = state.binder.source(0);
                let literal_node = source
                    .arena
                    .node_ids()
                    .find(|&id| source.arena.node(id).kind == tsrs2_syntax::SyntaxKind::TypeLiteral)
                    .expect("type literal");
                let anonymous = state
                    .get_type_from_type_node(literal_node)
                    .expect("type literal type");
                let t = {
                    let symbol = state
                        .resolve_name(
                            Some(literal_node),
                            "T",
                            SymbolFlags::TYPE_PARAMETER,
                            None,
                            false,
                            false,
                        )
                        .expect("T resolves");
                    state.get_declared_type_of_type_parameter(symbol)
                };
                let string = state.tables.intrinsics.string;
                let mapper = state.create_type_mapper(vec![t], Some(vec![string]));
                // The walk crosses the ClassDeclaration container: its
                // thisType joins the outer parameters and containsReference
                // filters it back out (no `this` in the literal).
                let shell = state
                    .instantiate_type(anonymous, Some(mapper))
                    .expect("instantiation crosses class containers since the GenericType port");
                assert_ne!(shell, anonymous);
                assert!(state
                    .tables
                    .object_flags_of(shell)
                    .intersects(ObjectFlags::INSTANTIATED));
                // The class declared type itself instantiates through the
                // reference arm.
                let c = state
                    .resolve_file_scope_name("C", SymbolFlags::CLASS)
                    .expect("C resolves");
                let declared = state
                    .get_declared_type_of_class_or_interface(c)
                    .expect("C declared");
                let mapper = state.create_type_mapper(vec![t], Some(vec![string]));
                let instantiated = state
                    .instantiate_type(declared, Some(mapper))
                    .expect("declared-type instantiation");
                assert_ne!(instantiated, declared);
                assert!(matches!(
                    state.tables.type_of(instantiated).data,
                    TypeData::Reference { target, .. } if target == declared
                ));
            },
        );
    }
}
