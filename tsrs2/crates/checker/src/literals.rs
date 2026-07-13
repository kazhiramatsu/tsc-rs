//! M4 5.5c: the literals band (extraction doc §5) — checkArrayLiteral
//! / checkObjectLiteral + the spread-type family + computed-name
//! checking + object-literal index synthesis. Codes owned here:
//! 2464, 2698, 2590, 2783(+related 2785), and the REVERSE excess check
//! 2353 (L74197 — contextual-pattern-lacks-member; the relation-side
//! EPC head lives in relate.rs since M3).
//!
//! Freshness plumbing (risk #3): object literals are born
//! FreshLiteral(8192); array literal types are born via the
//! createArrayLiteralType clone. The strip positions
//! (mutable-location, checkExpressionWithContextualType tail) landed
//! at 5.5b; assertion/object-level regular conversion land 5.5e/5.6.
//!
//! Stage escapes: [ITER] — non-array-like spreads escape to 5.5f
//! (checkIteratedTypeOrElementType); the generic-mapped tuple-context
//! disjunct escapes to M8 (currently dead — is_generic_mapped_type is
//! constant false until mapped types construct); object-literal
//! accessors defer to the 5.8-DECL escape arm in check_deferred_node;
//! grammar walks (checkGrammarObjectLiteralExpression 89637,
//! checkGrammarMethod 89943) are elided slices — 1117-family FN
//! documented by pin fixtures until they land.

use tsrs2_binder::{node_util, SymbolId, SymbolTable};
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    AccessFlags, CheckFlags, CheckMode, ContextFlags, ElementFlags, ObjectFlags, SymbolFlags,
    TypeData, TypeFlags, TypeId, UnionReduction,
};

use crate::state::{CheckResult2, CheckerState, IndexInfo, Unsupported};

/// The per-literal accumulator checkArrayLiteral threads through its
/// element loop while the cached contextual scope is pushed; the exits
/// consume it after the pop (tsc pops before the exits, 74020).
struct ArrayLiteralScan {
    element_types: Vec<TypeId>,
    element_flags: Vec<ElementFlags>,
    in_destructuring_pattern: bool,
    in_const_context: bool,
    contextual_type: Option<TypeId>,
    in_tuple_context: bool,
}

impl<'a> CheckerState<'a> {
    /// tsc-port: isSpreadIntoCallOrNew @6.0.3
    /// tsc-hash: 6d3c782aabde636ca8c97a9748cdf590f6340d59939db21f73474693d39ebe10
    /// tsc-span: _tsc.js:73952-73955
    fn is_spread_into_call_or_new(&self, node: NodeId) -> bool {
        let mut parent = self.parent_of(node);
        // walkUpParenthesizedExpressions (19359).
        while let Some(p) = parent {
            if self.kind_of(p) != SyntaxKind::ParenthesizedExpression {
                break;
            }
            parent = self.parent_of(p);
        }
        let Some(parent) = parent else { return false };
        if self.kind_of(parent) != SyntaxKind::SpreadElement {
            return false;
        }
        self.parent_of(parent).is_some_and(|grand| {
            matches!(
                self.kind_of(grand),
                SyntaxKind::CallExpression | SyntaxKind::NewExpression
            )
        })
    }

    /// tsc-port: isArrayLikeType @6.0.3
    /// tsc-hash: d4052c871dd48f9db83cbe239eb03664a38aed8f521ab81203bcf22345e6972e
    /// tsc-span: _tsc.js:67680-67682
    pub(crate) fn is_array_like_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if self.is_array_type(ty)? {
            return Ok(true);
        }
        if self.tables.flags_of(ty).intersects(TypeFlags::NULLABLE) {
            return Ok(false);
        }
        let any_readonly_array = self.any_readonly_array_type()?;
        self.is_type_assignable_to(ty, any_readonly_array)
    }

    /// tsc-port: isMutableArrayLikeType @6.0.3
    /// tsc-hash: 84f59dc973996df4a4a92ebe41bf5d0e9f65062dc4fba30c805914771da10094
    /// tsc-span: _tsc.js:67683-67685
    pub(crate) fn is_mutable_array_like_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if self.is_mutable_array_or_tuple(ty)? {
            return Ok(true);
        }
        if self
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::ANY | TypeFlags::NULLABLE)
        {
            return Ok(false);
        }
        let any_array = self.any_array_type()?;
        self.is_type_assignable_to(ty, any_array)
    }

    /// tsc-port: isTupleLikeType @6.0.3
    /// tsc-hash: 549fdab955d308326e6dd5f37054b777704e4c593cb88da89f86386c51e0ed7a
    /// tsc-span: _tsc.js:67722-67725
    pub(crate) fn is_tuple_like_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if self.tables.is_tuple_type(ty) {
            return Ok(true);
        }
        if self.get_property_of_type_full(ty, "0")?.is_some() {
            return Ok(true);
        }
        if !self.is_array_like_type(ty)? {
            return Ok(false);
        }
        let Some(length_type) = self.get_type_of_property_of_type(ty, "length")? else {
            return Ok(false);
        };
        Ok(self.every_type(length_type, |state, t| {
            state
                .tables
                .flags_of(t)
                .intersects(TypeFlags::NUMBER_LITERAL)
        }))
    }

    /// The inTupleContext contextual disjunct (73969):
    /// `isTupleLikeType(t) || isGenericMappedType(t) && !t.nameType &&
    /// getHomomorphicTypeVariable(...)` — the mapped disjunct is an M8
    /// escape (dead while is_generic_mapped_type is constant false).
    fn is_tuple_context_constituent(&mut self, t: TypeId) -> CheckResult2<bool> {
        if self.is_generic_mapped_type_state(t) {
            return Err(Unsupported::new(
                "inTupleContext generic-mapped disjunct (getHomomorphicTypeVariable, M8)",
            ));
        }
        self.is_tuple_like_type(t)
    }

    /// tsc-port: checkArrayLiteral @6.0.3
    /// tsc-hash: c9548a5ffbc73ebc39448b27acad5fe94e216419bc688d0f785ab9a0b6d18fd9
    /// tsc-span: _tsc.js:73956-74033
    ///
    /// The languageVersion emit-helper gate is dead at target ES2025
    /// (LanguageFeatureMinimumTarget.SpreadElements = ES2015); the
    /// [INFER] intra-expression site is behind the Inferential
    /// checkMode bit no M4 producer sets. Non-array-like spreads take
    /// the [ITER → 5.5f] escape (checkIteratedTypeOrElementType); the
    /// silent destructuring variant escapes the same way rather than
    /// inventing unknownType.
    pub(crate) fn check_array_literal(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
        force_tuple: bool,
    ) -> CheckResult2<TypeId> {
        self.push_cached_contextual_type(node)?;
        let scan = (|state: &mut Self| -> CheckResult2<ArrayLiteralScan> {
            let elements: Vec<NodeId> = match state.data_of(node) {
                NodeData::ArrayLiteralExpression(data) => state.nodes_of(data.elements),
                _ => Vec::new(),
            };
            let source = state.binder.source_of_node(node);
            let in_destructuring_pattern = node_util::is_assignment_target(source, node);
            let in_const_context = state.is_const_context(node)?;
            let contextual_type =
                state.get_apparent_type_of_contextual_type(node, ContextFlags::NONE)?;
            let in_tuple_context = state.is_spread_into_call_or_new(node)
                || match contextual_type {
                    Some(contextual) => state.some_type_result(contextual, |state, t| {
                        state.is_tuple_context_constituent(t)
                    })?,
                    None => false,
                };
            let mut element_types: Vec<TypeId> = Vec::with_capacity(elements.len());
            let mut element_flags: Vec<ElementFlags> = Vec::with_capacity(elements.len());
            let mut has_omitted_expression = false;
            for e in elements {
                if state.kind_of(e) == SyntaxKind::SpreadElement {
                    // languageVersion < SpreadElements:
                    // checkExternalEmitHelpers — dead at ES2025.
                    let expression = match state.data_of(e) {
                        NodeData::SpreadElement(data) => data.expression,
                        _ => None,
                    };
                    let expression = expression.ok_or_else(|| {
                        Unsupported::new("spread element without expression (parse recovery)")
                    })?;
                    let spread_type = state.check_expression_with_force_tuple(
                        expression,
                        check_mode,
                        force_tuple,
                    )?;
                    if state.is_array_like_type(spread_type)? {
                        element_types.push(spread_type);
                        element_flags.push(ElementFlags::VARIADIC);
                    } else if in_destructuring_pattern {
                        let number = state.tables.intrinsics.number;
                        match state.get_index_type_of_type(spread_type, number)? {
                            Some(rest) => {
                                element_types.push(rest);
                                element_flags.push(ElementFlags::REST);
                            }
                            None => {
                                // getIteratedTypeOrElementType(Destructuring,
                                // …, /*errorNode*/ undefined) ‖ unknownType
                                // — the silent [ITER] probe escapes to 5.5f
                                // rather than fabricating unknown.
                                return Err(Unsupported::new(
                                    "array destructuring rest over a non-array-like \
                                     (getIteratedTypeOrElementType, [ITER] 5.5f)",
                                ));
                            }
                        }
                    } else {
                        // checkIteratedTypeOrElementType(Spread, …,
                        // e.expression) — [ITER → 5.5f].
                        return Err(Unsupported::new(
                            "array spread over a non-array-like \
                             (checkIteratedTypeOrElementType, [ITER] 5.5f)",
                        ));
                    }
                } else if state.tables.exact_optional_property_types
                    && state.kind_of(e) == SyntaxKind::OmittedExpression
                {
                    has_omitted_expression = true;
                    element_types.push(state.tables.intrinsics.undefined_or_missing);
                    element_flags.push(ElementFlags::OPTIONAL);
                } else {
                    let ty =
                        state.check_expression_for_mutable_location(e, check_mode, force_tuple)?;
                    let with_optionality = state.tables.add_optionality(
                        ty,
                        /*is_property*/ true,
                        has_omitted_expression,
                    );
                    element_types.push(with_optionality);
                    element_flags.push(if has_omitted_expression {
                        ElementFlags::OPTIONAL
                    } else {
                        ElementFlags::REQUIRED
                    });
                    // inTupleContext && checkMode & Inferential && …:
                    // addIntraExpressionInferenceSite — [INFER] dead
                    // (no M4 producer sets the Inferential bit).
                    if in_tuple_context
                        && !check_mode.is_empty()
                        && check_mode.intersects(CheckMode::INFERENTIAL)
                        && !check_mode.intersects(CheckMode::SKIP_CONTEXT_SENSITIVE)
                        && state.is_context_sensitive(e)
                    {
                        return Err(Unsupported::new(
                            "addIntraExpressionInferenceSite (inference contexts, M6)",
                        ));
                    }
                }
            }
            Ok(ArrayLiteralScan {
                element_types,
                element_flags,
                in_destructuring_pattern,
                in_const_context,
                contextual_type,
                in_tuple_context,
            })
        })(self);
        self.pop_contextual_type();
        let scan = scan?;
        if scan.in_destructuring_pattern {
            // createTupleType RAW — no ArrayLiteral stamp (74021).
            return self
                .tables
                .create_tuple_type(&scan.element_types, Some(&scan.element_flags), false, None)
                .map_err(Self::unsupported_m4);
        }
        if force_tuple || scan.in_const_context || scan.in_tuple_context {
            let readonly = scan.in_const_context
                && !match scan.contextual_type {
                    Some(contextual) => self.some_type_result(contextual, |state, t| {
                        state.is_mutable_array_like_type(t)
                    })?,
                    None => false,
                };
            let tuple = self
                .tables
                .create_tuple_type(
                    &scan.element_types,
                    Some(&scan.element_flags),
                    readonly,
                    None,
                )
                .map_err(Self::unsupported_m4)?;
            return self.create_array_literal_type(tuple);
        }
        let element_union = if scan.element_types.is_empty() {
            if self.tables.strict_null_checks {
                self.tables.intrinsics.implicit_never
            } else {
                self.tables.intrinsics.undefined_widening
            }
        } else {
            let mut unwrapped = Vec::with_capacity(scan.element_types.len());
            for (i, &t) in scan.element_types.iter().enumerate() {
                if scan.element_flags[i].intersects(ElementFlags::VARIADIC) {
                    let number = self.tables.intrinsics.number;
                    let indexed = self
                        .get_indexed_access_type_or_undefined(
                            t,
                            number,
                            AccessFlags::NONE,
                            None,
                            None,
                            None,
                        )?
                        .unwrap_or(self.tables.intrinsics.any);
                    unwrapped.push(indexed);
                } else {
                    unwrapped.push(t);
                }
            }
            self.get_union_type_ex(&unwrapped, UnionReduction::Subtype)?
        };
        let array = self.create_array_type(element_union, scan.in_const_context)?;
        self.create_array_literal_type(array)
    }

    /// tsc-port: createArrayLiteralType @6.0.3
    /// tsc-hash: cf81899a7aafe0954eb10991a5494d2fe4da21f95384b231c35f014c6e1bdd12
    /// tsc-span: _tsc.js:74034-74044
    fn create_array_literal_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
        {
            return Ok(ty);
        }
        if let Some(literal) = self.links.ty(ty).literal_type {
            return Ok(literal);
        }
        let literal = self.tables.clone_type_reference(ty);
        let flags = self.tables.object_flags_of(literal)
            | ObjectFlags::ARRAY_LITERAL
            | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL;
        self.tables.type_mut(literal).object_flags = flags;
        self.links
            .set_type_literal_type(self.speculation_depth, ty, literal);
        Ok(literal)
    }

    /// tsc-port: isNumericName @6.0.3
    /// tsc-hash: eefe1bebc12b86e536cd952d515773fc40e546f371526d1708f5e3806fc7d994
    /// tsc-span: _tsc.js:74045-74057
    fn is_numeric_name(&mut self, name: NodeId) -> CheckResult2<bool> {
        Ok(match self.kind_of(name) {
            SyntaxKind::ComputedPropertyName => self.is_numeric_computed_name(name)?,
            SyntaxKind::Identifier => crate::indexed::is_numeric_literal_name(
                self.identifier_text_of(name).unwrap_or_default(),
            ),
            SyntaxKind::NumericLiteral | SyntaxKind::StringLiteral => {
                crate::indexed::is_numeric_literal_name(&self.literal_text_of(name))
            }
            _ => false,
        })
    }

    /// tsc-port: isNumericComputedName @6.0.3
    /// tsc-hash: 2c710679e57e3f5aa5207439bef6de1301b8dfb284d09d5a1c15941aa810013a
    /// tsc-span: _tsc.js:74058-74060
    fn is_numeric_computed_name(&mut self, name: NodeId) -> CheckResult2<bool> {
        let ty = self.check_computed_property_name(name)?;
        self.is_type_assignable_to_kind(ty, TypeFlags::NUMBER_LIKE, false)
    }

    /// tsc-port: checkComputedPropertyName @6.0.3
    /// tsc-hash: 9f89c9f08a99020384ee9ddada6d5172dd05aee82d8d3bc8aa6be2499f0bc7e3
    /// tsc-span: _tsc.js:74061-74082
    ///
    /// The result caches on the EXPRESSION node's resolvedType slot
    /// (74062). The class-expression-property loop-capture side-effect
    /// (LoopWithCapturedBlockScopedBinding/BlockScopedBindingInLoop
    /// nodeCheckFlags, 74067-74074) is emit-marking — elided, no
    /// diagnostic reads those bits. The cache write is guarded like
    /// checkExpressionCached: a re-entrant inner fill wins the slot.
    pub(crate) fn check_computed_property_name(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let expression = match self.data_of(node) {
            NodeData::ComputedPropertyName(data) => data.expression,
            _ => None,
        };
        let expression = expression.ok_or_else(|| {
            Unsupported::new("computed property name without expression (parse recovery)")
        })?;
        if let Some(cached) = self.links.node(expression).resolved_type.resolved() {
            return Ok(cached);
        }
        // The `[K in T]` member parse-recovery arm: a type-literal/
        // class/interface member whose computed name is an `in`
        // binary expression resolves to errorType silently.
        let parent = self.parent_of(node);
        let grandparent = parent.and_then(|p| self.parent_of(p));
        let in_recovery_container = grandparent.is_some_and(|g| {
            matches!(
                self.kind_of(g),
                SyntaxKind::TypeLiteral
                    | SyntaxKind::InterfaceDeclaration
                    | SyntaxKind::ClassDeclaration
                    | SyntaxKind::ClassExpression
            )
        });
        let is_in_binary = matches!(self.data_of(expression), NodeData::BinaryExpression(data)
            if data.operator_token.is_some_and(|op| self.kind_of(op) == SyntaxKind::InKeyword));
        let parent_is_accessor = parent.is_some_and(|p| {
            matches!(
                self.kind_of(p),
                SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
            )
        });
        if in_recovery_container && is_in_binary && !parent_is_accessor {
            let error = self.tables.intrinsics.error;
            self.links.set_node_resolved_type(
                self.speculation_depth,
                expression,
                crate::links::LinkSlot::Resolved(error),
            );
            return Ok(error);
        }
        let ty = self.check_expression(expression, CheckMode::NORMAL)?;
        if self
            .links
            .node(expression)
            .resolved_type
            .resolved()
            .is_none()
        {
            self.links.set_node_resolved_type(
                self.speculation_depth,
                expression,
                crate::links::LinkSlot::Resolved(ty),
            );
        }
        let nullable = self.tables.flags_of(ty).intersects(TypeFlags::NULLABLE);
        let legal = if nullable {
            false
        } else if self.is_type_assignable_to_kind(
            ty,
            TypeFlags::STRING_LIKE | TypeFlags::NUMBER_LIKE | TypeFlags::ES_SYMBOL_LIKE,
            false,
        )? {
            true
        } else {
            let string_number_symbol = self.tables.intrinsics.string_number_symbol;
            self.is_type_assignable_to(ty, string_number_symbol)?
        };
        if !legal {
            self.error_at(
                Some(node),
                &diagnostics::A_computed_property_name_must_be_of_type_string_number_symbol_or_any,
                &[],
            );
        }
        Ok(self
            .links
            .node(expression)
            .resolved_type
            .resolved()
            .unwrap_or(ty))
    }

    /// tsc-port: isSymbolWithNumericName @6.0.3
    /// tsc-hash: beb742e6e73179136a6e9921ecf8187897b5a23baa9ba1161f7233396e71ab31
    /// tsc-span: _tsc.js:74083-74087
    fn is_symbol_with_numeric_name(&mut self, symbol: SymbolId) -> CheckResult2<bool> {
        if crate::indexed::is_numeric_literal_name(&self.binder.symbol(symbol).escaped_name) {
            return Ok(true);
        }
        let Some(first_decl) = self.binder.symbol(symbol).declarations.first().copied() else {
            return Ok(false);
        };
        match self.name_of_named_declaration(first_decl) {
            Some(name) => self.is_numeric_name(name),
            None => Ok(false),
        }
    }

    /// tsc-port: isSymbolWithSymbolName @6.0.3
    /// tsc-hash: fa5a1b163b9aaf0d9d2d7e15ee191225b8e92a24fd926e812ddafc1e62a8edfa
    /// tsc-span: _tsc.js:74088-74092
    ///
    /// isKnownSymbol (19295) inlines as the "__@" escaped-name prefix.
    fn is_symbol_with_symbol_name(&mut self, symbol: SymbolId) -> CheckResult2<bool> {
        if self.binder.symbol(symbol).escaped_name.starts_with("__@") {
            return Ok(true);
        }
        let Some(first_decl) = self.binder.symbol(symbol).declarations.first().copied() else {
            return Ok(false);
        };
        let Some(name) = self.name_of_named_declaration(first_decl) else {
            return Ok(false);
        };
        if self.kind_of(name) != SyntaxKind::ComputedPropertyName {
            return Ok(false);
        }
        let ty = self.check_computed_property_name(name)?;
        self.is_type_assignable_to_kind(ty, TypeFlags::ES_SYMBOL, false)
    }

    /// tsc-port: isSymbolWithComputedName @6.0.3
    /// tsc-hash: 07d5fc9d3c094c8199150039808dee0fc58b029acbc3517ebfe727516b919a86
    /// tsc-span: _tsc.js:74093-74097
    fn is_symbol_with_computed_name(&self, symbol: SymbolId) -> bool {
        let Some(first_decl) = self.binder.symbol(symbol).declarations.first().copied() else {
            return false;
        };
        self.name_of_named_declaration(first_decl)
            .is_some_and(|name| self.kind_of(name) == SyntaxKind::ComputedPropertyName)
    }

    /// tsc-port: getObjectLiteralIndexInfo @6.0.3
    /// tsc-hash: d08c845384a5c83bbb6a6223743e814894596219d421b832e79f7d4f7761ddb3
    /// tsc-span: _tsc.js:74098-74120
    fn get_object_literal_index_info(
        &mut self,
        is_readonly: bool,
        offset: usize,
        properties: &[SymbolId],
        key_type: TypeId,
    ) -> CheckResult2<IndexInfo> {
        let string = self.tables.intrinsics.string;
        let number = self.tables.intrinsics.number;
        let es_symbol = self.tables.intrinsics.es_symbol;
        let mut prop_types: Vec<TypeId> = Vec::new();
        let mut components: Option<Vec<NodeId>> = None;
        for &prop in &properties[offset.min(properties.len())..] {
            let selected = if key_type == string {
                !self.is_symbol_with_symbol_name(prop)?
            } else if key_type == number {
                self.is_symbol_with_numeric_name(prop)?
            } else if key_type == es_symbol {
                self.is_symbol_with_symbol_name(prop)?
            } else {
                false
            };
            if selected {
                prop_types.push(self.get_type_of_symbol(prop)?);
                if self.is_symbol_with_computed_name(prop) {
                    if let Some(&decl) = self.binder.symbol(prop).declarations.first() {
                        components.get_or_insert_with(Vec::new).push(decl);
                    }
                }
            }
        }
        let value_type = if prop_types.is_empty() {
            self.tables.intrinsics.undefined
        } else {
            self.get_union_type_ex(&prop_types, UnionReduction::Subtype)?
        };
        Ok(IndexInfo {
            key_type,
            value_type,
            is_readonly,
            declaration: None,
            components,
        })
    }

    /// The `declaration.name` read behind isNamedDeclaration guards
    /// (74086): the member-declaration kinds the literals band meets.
    fn name_of_named_declaration(&self, declaration: NodeId) -> Option<NodeId> {
        match self.data_of(declaration) {
            NodeData::PropertyAssignment(data) => data.name,
            NodeData::ShorthandPropertyAssignment(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            NodeData::PropertyDeclaration(data) => data.name,
            NodeData::PropertySignature(data) => data.name,
            NodeData::MethodSignature(data) => data.name,
            _ => None,
        }
    }

    /// The NumericLiteral/StringLiteral `.text` read behind
    /// isNumericName's literal arms.
    fn literal_text_of(&self, node: NodeId) -> String {
        match self.data_of(node) {
            NodeData::NumericLiteral(data) => data.text.clone(),
            NodeData::StringLiteral(data) => data.text.clone(),
            _ => String::new(),
        }
    }
}

/// checkObjectLiteral's accumulator — the locals of L74138-74158 the
/// member loop mutates and the createObjectLiteralType closure reads.
struct ObjectLiteralAcc {
    all_properties_table: Option<SymbolTable>,
    properties_table: SymbolTable,
    properties_array: Vec<SymbolId>,
    spread: TypeId,
    contextual_type: Option<TypeId>,
    contextual_type_has_pattern: bool,
    in_const_context: bool,
    check_flags: CheckFlags,
    object_flags: ObjectFlags,
    pattern_with_computed_properties: bool,
    has_computed_string_property: bool,
    has_computed_number_property: bool,
    has_computed_symbol_property: bool,
    offset: usize,
    in_destructuring_pattern: bool,
}

impl<'a> CheckerState<'a> {
    /// tsc-port: checkObjectLiteral @6.0.3
    /// tsc-hash: c78231c4fb699497a2e5eaf135a9a290add889608f13083434a3d68a31b03d78
    /// tsc-span: _tsc.js:74135-74299
    ///
    /// Elided/dead arms: checkGrammarObjectLiteralExpression (89637)
    /// is an elided slice (1117-family FN, pinned);
    /// isInJavascript/enumTag/jsDocType/JSLiteral ride [JSDOC] (TS
    /// files answer false — plain-JS files gate earlier); the
    /// languageVersion ObjectAssign emit-helper gate is dead at
    /// ES2025; the Inferential intra-expression site is a dead gate
    /// (no M4 producer). The result is FRESH per call — no node-links
    /// caching, matching tsc.
    pub(crate) fn check_object_literal(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let source = self.binder.source_of_node(node);
        let in_destructuring_pattern = node_util::is_assignment_target(source, node);
        // checkGrammarObjectLiteralExpression(node, inDestructuringPattern):
        // elided slice.
        let strict_null_checks = self.tables.strict_null_checks;
        let mut acc = ObjectLiteralAcc {
            all_properties_table: strict_null_checks.then(SymbolTable::default),
            properties_table: SymbolTable::default(),
            properties_array: Vec::new(),
            spread: self.empty_object_type,
            contextual_type: None,
            contextual_type_has_pattern: false,
            in_const_context: false,
            check_flags: CheckFlags::from_bits(0),
            object_flags: ObjectFlags::FRESH_LITERAL,
            pattern_with_computed_properties: false,
            has_computed_string_property: false,
            has_computed_number_property: false,
            has_computed_symbol_property: false,
            offset: 0,
            in_destructuring_pattern,
        };
        self.push_cached_contextual_type(node)?;
        let walked = self.check_object_literal_members(node, check_mode, &mut acc);
        self.pop_contextual_type();
        walked?;
        let error = self.tables.intrinsics.error;
        if acc.spread == error {
            return Ok(error);
        }
        if acc.spread != self.empty_object_type {
            if !acc.properties_array.is_empty() {
                let segment = self.create_object_literal_segment(node, &acc)?;
                let raw_symbol = self.node_symbol(node);
                acc.spread = self.get_spread_type(
                    acc.spread,
                    segment,
                    raw_symbol,
                    acc.object_flags,
                    acc.in_const_context,
                )?;
                acc.properties_array = Vec::new();
                acc.properties_table = SymbolTable::default();
                acc.has_computed_string_property = false;
                acc.has_computed_number_property = false;
                // hasComputedSymbolProperty survives the FINAL flush
                // (74270-74276 resets only string+number) — the
                // asymmetry is transcribed as-is.
            }
            let spread = acc.spread;
            return self.map_type_result(spread, |state, t| {
                if t == state.empty_object_type {
                    state.create_object_literal_segment(node, &acc)
                } else {
                    Ok(t)
                }
            });
        }
        self.create_object_literal_segment(node, &acc)
    }

    /// The member loop of checkObjectLiteral (74159-74268), run while
    /// the cached contextual scope is pushed.
    fn check_object_literal_members(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
        acc: &mut ObjectLiteralAcc,
    ) -> CheckResult2<()> {
        let properties: Vec<NodeId> = match self.data_of(node) {
            NodeData::ObjectLiteralExpression(data) => self.nodes_of(data.properties),
            _ => Vec::new(),
        };
        acc.contextual_type =
            self.get_apparent_type_of_contextual_type(node, ContextFlags::NONE)?;
        acc.contextual_type_has_pattern = acc.contextual_type.is_some_and(|contextual| {
            self.links.ty(contextual).pattern.is_some_and(|pattern| {
                matches!(
                    self.kind_of(pattern),
                    SyntaxKind::ObjectBindingPattern | SyntaxKind::ObjectLiteralExpression
                )
            })
        });
        acc.in_const_context = self.is_const_context(node)?;
        acc.check_flags = if acc.in_const_context {
            CheckFlags::READONLY
        } else {
            CheckFlags::from_bits(0)
        };
        // isInJavascript / enumTag / isJSObjectLiteral: [JSDOC] — TS
        // files answer false throughout.
        // Pre-pass: force every computed name (74159-74163).
        for &member_decl in &properties {
            if let Some(name) = self.name_of_named_declaration(member_decl) {
                if self.kind_of(name) == SyntaxKind::ComputedPropertyName {
                    self.check_computed_property_name(name)?;
                }
            }
        }
        for member_decl in properties {
            let member_symbol = self
                .node_symbol(member_decl)
                .map(|symbol| self.get_merged_symbol(symbol));
            let computed_name_type = match self.name_of_named_declaration(member_decl) {
                Some(name) if self.kind_of(name) == SyntaxKind::ComputedPropertyName => {
                    Some(self.check_computed_property_name(name)?)
                }
                _ => None,
            };
            let kind = self.kind_of(member_decl);
            let member: SymbolId;
            if matches!(
                kind,
                SyntaxKind::PropertyAssignment | SyntaxKind::ShorthandPropertyAssignment
            ) || self.is_object_literal_method(member_decl)
            {
                let member_sym = member_symbol.ok_or_else(|| {
                    Unsupported::new("object member without a bound symbol (parse recovery)")
                })?;
                let ty = match kind {
                    SyntaxKind::PropertyAssignment => {
                        self.check_property_assignment(member_decl, check_mode)?
                    }
                    SyntaxKind::ShorthandPropertyAssignment => {
                        // The objectAssignmentInitializer only outside
                        // destructuring — error-recovery semantics for
                        // `{ a = 100 }` (74173-74176).
                        let (name, initializer) = match self.data_of(member_decl) {
                            NodeData::ShorthandPropertyAssignment(data) => {
                                (data.name, data.object_assignment_initializer)
                            }
                            _ => (None, None),
                        };
                        let target = match initializer {
                            Some(initializer) if !acc.in_destructuring_pattern => initializer,
                            _ => name.ok_or_else(|| {
                                Unsupported::new(
                                    "shorthand property without a name (parse recovery)",
                                )
                            })?,
                        };
                        self.check_expression_for_mutable_location(target, check_mode, false)?
                    }
                    _ => self.check_object_literal_method(member_decl, check_mode)?,
                };
                // isInJavascript jsDocType/enumTag arms — [JSDOC] dead.
                acc.object_flags |=
                    self.tables.object_flags_of(ty) & ObjectFlags::PROPAGATING_FLAGS;
                let name_type = computed_name_type
                    .filter(|&t| self.property_name_from_type_usable(t).is_some());
                let member_flags = self.binder.symbol(member_sym).flags;
                let prop = match name_type {
                    Some(name_type) => {
                        let name = self
                            .property_name_from_type_usable(name_type)
                            .expect("filtered above");
                        let prop = self
                            .binder
                            .create_symbol(SymbolFlags::PROPERTY | member_flags, name);
                        self.links.set_symbol_check_flags(
                            self.speculation_depth,
                            prop,
                            acc.check_flags | CheckFlags::LATE,
                        );
                        self.links.set_symbol_name_type(
                            self.speculation_depth,
                            prop,
                            Some(name_type),
                        );
                        prop
                    }
                    None => {
                        let name = self.binder.symbol(member_sym).escaped_name.clone();
                        let prop = self
                            .binder
                            .create_symbol(SymbolFlags::PROPERTY | member_flags, name);
                        if !acc.check_flags.is_empty() {
                            self.links.set_symbol_check_flags(
                                self.speculation_depth,
                                prop,
                                acc.check_flags,
                            );
                        }
                        prop
                    }
                };
                if acc.in_destructuring_pattern && self.has_default_value(member_decl) {
                    self.binder.symbol_mut(prop).flags |= SymbolFlags::OPTIONAL;
                } else if acc.contextual_type_has_pattern
                    && !self
                        .tables
                        .object_flags_of(acc.contextual_type.expect("pattern implies contextual"))
                        .intersects(ObjectFlags::OBJECT_LITERAL_PATTERN_WITH_COMPUTED_PROPERTIES)
                {
                    let contextual = acc.contextual_type.expect("pattern implies contextual");
                    let member_name = self.binder.symbol(member_sym).escaped_name.clone();
                    let implied_prop = self.get_property_of_type_full(contextual, &member_name)?;
                    match implied_prop {
                        Some(implied) => {
                            let optional =
                                self.binder.symbol(implied).flags & SymbolFlags::OPTIONAL;
                            self.binder.symbol_mut(prop).flags |= optional;
                        }
                        None => {
                            let string = self.tables.intrinsics.string;
                            if self.get_index_info_of_type(contextual, string)?.is_none() {
                                let member_display = self.symbol_display_name(member_sym);
                                let contextual_display = self.type_to_string_slice(contextual)?;
                                let error_node = self
                                    .name_of_named_declaration(member_decl)
                                    .or(Some(member_decl));
                                self.error_at(
                                    error_node,
                                    &diagnostics::Object_literal_may_only_specify_known_properties_and_0_does_not_exist_in_type_1,
                                    &[&member_display, &contextual_display],
                                );
                            }
                        }
                    }
                }
                let declarations = self.binder.symbol(member_sym).declarations.clone();
                let parent = self.binder.symbol(member_sym).parent;
                let value_declaration = self.binder.symbol(member_sym).value_declaration;
                {
                    let prop_symbol = self.binder.symbol_mut(prop);
                    prop_symbol.declarations = declarations;
                    prop_symbol.parent = parent;
                    if let Some(value_declaration) = value_declaration {
                        prop_symbol.value_declaration = Some(value_declaration);
                    }
                }
                self.links.set_symbol_type(
                    self.speculation_depth,
                    prop,
                    crate::links::LinkSlot::Resolved(ty),
                );
                self.links
                    .set_symbol_target(self.speculation_depth, prop, member_sym);
                member = prop;
                if let Some(all) = &mut acc.all_properties_table {
                    let name = self.binder.symbol(prop).escaped_name.clone();
                    all.insert(name, prop);
                }
                // contextualType && checkMode & Inferential && …:
                // addIntraExpressionInferenceSite — [INFER] dead (no
                // M4 producer sets the Inferential bit).
                if acc.contextual_type.is_some()
                    && check_mode.intersects(CheckMode::INFERENTIAL)
                    && !check_mode.intersects(CheckMode::SKIP_CONTEXT_SENSITIVE)
                    && matches!(
                        kind,
                        SyntaxKind::PropertyAssignment | SyntaxKind::MethodDeclaration
                    )
                    && self.is_context_sensitive(member_decl)
                {
                    return Err(Unsupported::new(
                        "addIntraExpressionInferenceSite (inference contexts, M6)",
                    ));
                }
            } else if kind == SyntaxKind::SpreadAssignment {
                // languageVersion < ObjectAssign:
                // checkExternalEmitHelpers — dead at ES2025.
                if !acc.properties_array.is_empty() {
                    let segment = self.create_object_literal_segment(node, acc)?;
                    let raw_symbol = self.node_symbol(node);
                    acc.spread = self.get_spread_type(
                        acc.spread,
                        segment,
                        raw_symbol,
                        acc.object_flags,
                        acc.in_const_context,
                    )?;
                    acc.properties_array = Vec::new();
                    acc.properties_table = SymbolTable::default();
                    acc.has_computed_string_property = false;
                    acc.has_computed_number_property = false;
                    acc.has_computed_symbol_property = false;
                }
                let expression = match self.data_of(member_decl) {
                    NodeData::SpreadAssignment(data) => data.expression,
                    _ => None,
                };
                let expression = expression.ok_or_else(|| {
                    Unsupported::new("spread assignment without expression (parse recovery)")
                })?;
                let inner_mode =
                    CheckMode::from_bits(check_mode.bits() & CheckMode::INFERENTIAL.bits());
                let raw = self.check_expression(expression, inner_mode)?;
                let ty = self.get_reduced_type(raw)?;
                if self.is_valid_spread_type(ty)? {
                    let merged = self.try_merge_union_of_object_type_and_empty_object(
                        ty,
                        acc.in_const_context,
                    )?;
                    if let Some(all) = &acc.all_properties_table {
                        let all = all.clone();
                        self.check_spread_prop_overrides(merged, &all, member_decl)?;
                    }
                    acc.offset = acc.properties_array.len();
                    if acc.spread == self.tables.intrinsics.error {
                        continue;
                    }
                    let raw_symbol = self.node_symbol(node);
                    acc.spread = self.get_spread_type(
                        acc.spread,
                        merged,
                        raw_symbol,
                        acc.object_flags,
                        acc.in_const_context,
                    )?;
                } else {
                    self.error_at(
                        Some(member_decl),
                        &diagnostics::Spread_types_may_only_be_created_from_object_types,
                        &[],
                    );
                    acc.spread = self.tables.intrinsics.error;
                }
                continue;
            } else {
                // Debug.assert(Get/SetAccessor) — recovery kinds take
                // a named escape instead of a panic (risk #6).
                if !matches!(kind, SyntaxKind::GetAccessor | SyntaxKind::SetAccessor) {
                    return Err(Unsupported::new(
                        "unexpected object-literal member kind (parse recovery)",
                    ));
                }
                self.check_node_deferred(member_decl);
                member = member_symbol.ok_or_else(|| {
                    Unsupported::new("object accessor without a bound symbol (parse recovery)")
                })?;
            }
            if let Some(computed_name_type) = computed_name_type.filter(|&t| {
                !self
                    .tables
                    .flags_of(t)
                    .intersects(TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE)
            }) {
                let string_number_symbol = self.tables.intrinsics.string_number_symbol;
                if self.is_type_assignable_to(computed_name_type, string_number_symbol)? {
                    let number = self.tables.intrinsics.number;
                    let es_symbol = self.tables.intrinsics.es_symbol;
                    if self.is_type_assignable_to(computed_name_type, number)? {
                        acc.has_computed_number_property = true;
                    } else if self.is_type_assignable_to(computed_name_type, es_symbol)? {
                        acc.has_computed_symbol_property = true;
                    } else {
                        acc.has_computed_string_property = true;
                    }
                    if acc.in_destructuring_pattern {
                        acc.pattern_with_computed_properties = true;
                    }
                }
            } else {
                let name = self.binder.symbol(member).escaped_name.clone();
                acc.properties_table.insert(name, member);
            }
            acc.properties_array.push(member);
        }
        Ok(())
    }

    /// The createObjectLiteralType closure (74277-74298): a FRESH
    /// anonymous type per call over the CURRENT segment state.
    fn create_object_literal_segment(
        &mut self,
        node: NodeId,
        acc: &ObjectLiteralAcc,
    ) -> CheckResult2<TypeId> {
        let mut index_infos: Vec<IndexInfo> = Vec::new();
        let is_readonly = self.is_const_context(node)?;
        if acc.has_computed_string_property {
            let string = self.tables.intrinsics.string;
            index_infos.push(self.get_object_literal_index_info(
                is_readonly,
                acc.offset,
                &acc.properties_array,
                string,
            )?);
        }
        if acc.has_computed_number_property {
            let number = self.tables.intrinsics.number;
            index_infos.push(self.get_object_literal_index_info(
                is_readonly,
                acc.offset,
                &acc.properties_array,
                number,
            )?);
        }
        if acc.has_computed_symbol_property {
            let es_symbol = self.tables.intrinsics.es_symbol;
            index_infos.push(self.get_object_literal_index_info(
                is_readonly,
                acc.offset,
                &acc.properties_array,
                es_symbol,
            )?);
        }
        let symbol = self.node_symbol(node);
        let mut object_flags = ObjectFlags::ANONYMOUS
            | acc.object_flags
            | ObjectFlags::OBJECT_LITERAL
            | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL;
        // isJSObjectLiteral → JSLiteral: [JSDOC] dead in TS files.
        if acc.pattern_with_computed_properties {
            object_flags |= ObjectFlags::OBJECT_LITERAL_PATTERN_WITH_COMPUTED_PROPERTIES;
        }
        let id = self.make_resolved_anonymous_type(
            symbol,
            acc.properties_table.clone(),
            acc.properties_array.clone(),
            index_infos,
            object_flags,
        );
        if acc.in_destructuring_pattern {
            self.links
                .set_type_pattern(self.speculation_depth, id, node);
        }
        Ok(id)
    }

    /// createAnonymousType (50208) over already-resolved members —
    /// the literals-band constructor (fresh TypeId per call).
    pub(crate) fn make_resolved_anonymous_type(
        &mut self,
        symbol: Option<SymbolId>,
        members: SymbolTable,
        properties: Vec<SymbolId>,
        index_infos: Vec<IndexInfo>,
        object_flags: ObjectFlags,
    ) -> TypeId {
        let id = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
        self.tables.type_mut(id).object_flags = object_flags;
        self.tables.type_mut(id).symbol = symbol;
        let members_id = self.alloc_members(crate::state::ResolvedMembers {
            members,
            properties,
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            index_infos,
        });
        self.links.set_type_members(
            self.speculation_depth,
            id,
            crate::links::LinkSlot::Resolved(members_id),
        );
        id
    }

    /// tsc-port: isValidSpreadType @6.0.3
    /// tsc-hash: 74590af5838441dbda040c02216967ca483c35f1588d4e2e00cae5978bc22c0c
    /// tsc-span: _tsc.js:74300-74303
    pub(crate) fn is_valid_spread_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let constrained =
            self.map_type_result(ty, |state, t| state.get_base_constraint_or_type(t))?;
        let t = self.remove_definitely_falsy_types(constrained)?;
        let flags = self.tables.flags_of(t);
        if flags.intersects(
            TypeFlags::ANY
                | TypeFlags::NON_PRIMITIVE
                | TypeFlags::OBJECT
                | TypeFlags::INSTANTIABLE_NON_PRIMITIVE,
        ) {
            return Ok(true);
        }
        if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            let members: Vec<TypeId> = match &self.tables.type_of(t).data {
                TypeData::Union { types, .. } => types.to_vec(),
                TypeData::Intersection { types } => types.to_vec(),
                _ => return Ok(false),
            };
            for member in members {
                if !self.is_valid_spread_type(member)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// tsc-port: checkSpreadPropOverrides @6.0.3
    /// tsc-hash: d7b214c0f5be33bdc704b964aa3564298a490c7a2420eb052f06418bbab6679e
    /// tsc-span: _tsc.js:74511-74521
    fn check_spread_prop_overrides(
        &mut self,
        ty: TypeId,
        props: &SymbolTable,
        spread: NodeId,
    ) -> CheckResult2<()> {
        for right in self.get_properties_of_type(ty)? {
            let right_flags = self.binder.symbol(right).flags;
            if right_flags.intersects(SymbolFlags::OPTIONAL) {
                continue;
            }
            if self.get_check_flags(right).intersects(CheckFlags::PARTIAL) {
                continue;
            }
            let name = self.binder.symbol(right).escaped_name.clone();
            let Some(&left) = props.get(&name) else {
                continue;
            };
            let display = self.symbol_display_name(left);
            let related = self.related_for_node(
                spread,
                &diagnostics::This_spread_always_overwrites_this_property,
                &[],
            );
            let left_declaration = self.binder.symbol(left).value_declaration;
            self.error_at_with_related(
                left_declaration,
                &diagnostics::_0_is_specified_more_than_once_so_this_usage_will_be_overwritten,
                &[&display],
                vec![related],
            );
        }
        Ok(())
    }

    /// tsc-port: isEmptyObjectTypeOrSpreadsIntoEmptyObject @6.0.3
    /// tsc-hash: c936035e3c75254a2f9524cae682dd8d8b03f53fd2575f0ccbd35f58a76d5a0d
    /// tsc-span: _tsc.js:62921-62923
    fn is_empty_object_type_or_spreads_into_empty_object(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<bool> {
        if self.is_empty_object_type(ty)? {
            return Ok(true);
        }
        Ok(self.tables.flags_of(ty).intersects(
            TypeFlags::NULL
                | TypeFlags::UNDEFINED
                | TypeFlags::BOOLEAN_LIKE
                | TypeFlags::NUMBER_LIKE
                | TypeFlags::BIG_INT_LIKE
                | TypeFlags::STRING_LIKE
                | TypeFlags::ENUM_LIKE
                | TypeFlags::NON_PRIMITIVE
                | TypeFlags::INDEX,
        ))
    }

    /// tsc-port: tryMergeUnionOfObjectTypeAndEmptyObject @6.0.3
    /// tsc-hash: f8575980a61d1210d63cd653af3b1f5922957866d362ce7743c2aaf7583b89ca
    /// tsc-span: _tsc.js:62924-62963
    fn try_merge_union_of_object_type_and_empty_object(
        &mut self,
        ty: TypeId,
        readonly: bool,
    ) -> CheckResult2<TypeId> {
        if !self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            return Ok(ty);
        }
        let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
            TypeData::Union { types, .. } => types.to_vec(),
            _ => unreachable!("union flag implies union data"),
        };
        let mut all_empty_or_spread = true;
        for &member in &members {
            if !self.is_empty_object_type_or_spreads_into_empty_object(member)? {
                all_empty_or_spread = false;
                break;
            }
        }
        if all_empty_or_spread {
            for &member in &members {
                if self.is_empty_object_type(member)? {
                    return Ok(member);
                }
            }
            return Ok(self.empty_object_type);
        }
        let mut first_type: Option<TypeId> = None;
        for &member in &members {
            if !self.is_empty_object_type_or_spreads_into_empty_object(member)? {
                first_type = Some(member);
                break;
            }
        }
        let Some(first_type) = first_type else {
            return Ok(ty);
        };
        for &member in &members {
            if member != first_type
                && !self.is_empty_object_type_or_spreads_into_empty_object(member)?
            {
                // A second non-empty constituent: no merge.
                return Ok(ty);
            }
        }
        self.get_anonymous_partial_type(first_type, readonly)
    }

    /// getAnonymousPartialType — the inner closure of
    /// tryMergeUnionOfObjectTypeAndEmptyObject (62940-62962).
    fn get_anonymous_partial_type(&mut self, ty: TypeId, readonly: bool) -> CheckResult2<TypeId> {
        let mut members = SymbolTable::default();
        let mut properties: Vec<SymbolId> = Vec::new();
        for prop in self.get_properties_of_type(ty)? {
            let modifiers = self.get_declaration_modifier_flags_from_symbol(prop);
            if modifiers.intersects(
                tsrs2_types::ModifierFlags::PRIVATE | tsrs2_types::ModifierFlags::PROTECTED,
            ) {
                // Skipped entirely (62943).
            } else if self.is_spreadable_property(prop) {
                let prop_flags = self.binder.symbol(prop).flags;
                let is_setonly_accessor = prop_flags.intersects(SymbolFlags::SET_ACCESSOR)
                    && !prop_flags.intersects(SymbolFlags::GET_ACCESSOR);
                let name = self.binder.symbol(prop).escaped_name.clone();
                let result = self
                    .binder
                    .create_symbol(SymbolFlags::PROPERTY | SymbolFlags::OPTIONAL, name.clone());
                let late = self.get_check_flags(prop) & CheckFlags::LATE;
                let check_flags = late
                    | if readonly {
                        CheckFlags::READONLY
                    } else {
                        CheckFlags::from_bits(0)
                    };
                if !check_flags.is_empty() {
                    self.links
                        .set_symbol_check_flags(self.speculation_depth, result, check_flags);
                }
                let result_type = if is_setonly_accessor {
                    self.tables.intrinsics.undefined
                } else {
                    let prop_type = self.get_type_of_symbol(prop)?;
                    self.tables
                        .add_optionality(prop_type, /*is_property*/ true, true)
                };
                self.links.set_symbol_type(
                    self.speculation_depth,
                    result,
                    crate::links::LinkSlot::Resolved(result_type),
                );
                let declarations = self.binder.symbol(prop).declarations.clone();
                self.binder.symbol_mut(result).declarations = declarations;
                let name_type = self.links.symbol(prop).name_type;
                self.links
                    .set_symbol_name_type(self.speculation_depth, result, name_type);
                self.links
                    .set_symbol_synthetic_origin(self.speculation_depth, result, prop);
                members.insert(name, result);
                properties.push(result);
            }
        }
        let index_infos = self.get_index_infos_of_type(ty)?;
        let symbol = self.tables.type_of(ty).symbol;
        Ok(self.make_resolved_anonymous_type(
            symbol,
            members,
            properties,
            index_infos,
            ObjectFlags::ANONYMOUS
                | ObjectFlags::OBJECT_LITERAL
                | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL,
        ))
    }

    /// tsc-port: checkCrossProductUnion @6.0.3
    /// tsc-hash: c916448698615d4762a0b12b7b0759c757fa2c19dedd842bc9448d9731fe9da1
    /// tsc-span: _tsc.js:61874-61883
    ///
    /// The error-reporting variant getSpreadType consumes (2590 at
    /// currentNode); the intersection path keeps its silent guard
    /// (intersect.rs) with the diagnostic deferred.
    fn check_cross_product_union(&mut self, types: &[TypeId]) -> bool {
        let size = self.cross_product_union_size(types);
        if size >= 100_000 {
            self.error_at(
                self.current_node,
                &diagnostics::Expression_produces_a_union_type_that_is_too_complex_to_represent,
                &[],
            );
            return false;
        }
        true
    }

    /// tsc-port: getSpreadType @6.0.3
    /// tsc-hash: d634ea9b62362ca445f014e39bab81d0f54845ad4efc867fdb130837387749b6
    /// tsc-span: _tsc.js:62964-63039
    fn get_spread_type(
        &mut self,
        left: TypeId,
        right: TypeId,
        symbol: Option<SymbolId>,
        object_flags: ObjectFlags,
        readonly: bool,
    ) -> CheckResult2<TypeId> {
        let left_flags = self.tables.flags_of(left);
        let right_flags = self.tables.flags_of(right);
        if left_flags.intersects(TypeFlags::ANY) || right_flags.intersects(TypeFlags::ANY) {
            return Ok(self.tables.intrinsics.any);
        }
        if left_flags.intersects(TypeFlags::UNKNOWN) || right_flags.intersects(TypeFlags::UNKNOWN) {
            return Ok(self.tables.intrinsics.unknown);
        }
        if left_flags.intersects(TypeFlags::NEVER) {
            return Ok(right);
        }
        if right_flags.intersects(TypeFlags::NEVER) {
            return Ok(left);
        }
        let left = self.try_merge_union_of_object_type_and_empty_object(left, readonly)?;
        if self.tables.flags_of(left).intersects(TypeFlags::UNION) {
            if !self.check_cross_product_union(&[left, right]) {
                return Ok(self.tables.intrinsics.error);
            }
            return self.map_type_result(left, |state, t| {
                state.get_spread_type(t, right, symbol, object_flags, readonly)
            });
        }
        let right = self.try_merge_union_of_object_type_and_empty_object(right, readonly)?;
        if self.tables.flags_of(right).intersects(TypeFlags::UNION) {
            if !self.check_cross_product_union(&[left, right]) {
                return Ok(self.tables.intrinsics.error);
            }
            return self.map_type_result(right, |state, t| {
                state.get_spread_type(left, t, symbol, object_flags, readonly)
            });
        }
        if self.tables.flags_of(right).intersects(
            TypeFlags::BOOLEAN_LIKE
                | TypeFlags::NUMBER_LIKE
                | TypeFlags::BIG_INT_LIKE
                | TypeFlags::STRING_LIKE
                | TypeFlags::ENUM_LIKE
                | TypeFlags::NON_PRIMITIVE
                | TypeFlags::INDEX,
        ) {
            return Ok(left);
        }
        let left_generic = self.tables.is_generic_object_type(left);
        let right_generic = self.tables.is_generic_object_type(right);
        if left_generic || right_generic {
            if self.is_empty_object_type(left)? {
                return Ok(right);
            }
            if self
                .tables
                .flags_of(left)
                .intersects(TypeFlags::INTERSECTION)
            {
                let types: Vec<TypeId> = match &self.tables.type_of(left).data {
                    TypeData::Intersection { types } => types.to_vec(),
                    _ => unreachable!("intersection flag implies intersection data"),
                };
                let last_left = *types.last().expect("intersections are non-empty");
                if self.is_non_generic_object_type(last_left)
                    && self.is_non_generic_object_type(right)
                {
                    let folded =
                        self.get_spread_type(last_left, right, symbol, object_flags, readonly)?;
                    let mut constituents = types[..types.len() - 1].to_vec();
                    constituents.push(folded);
                    return self.get_intersection_type(
                        &constituents,
                        tsrs2_types::IntersectionFlags::NONE,
                    );
                }
            }
            return self
                .get_intersection_type(&[left, right], tsrs2_types::IntersectionFlags::NONE);
        }
        let mut members = SymbolTable::default();
        let mut skipped_private_members: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let index_infos = if left == self.empty_object_type {
            self.get_index_infos_of_type(right)?
        } else {
            self.get_union_index_infos(&[left, right])?
        };
        for right_prop in self.get_properties_of_type(right)? {
            let name = self.binder.symbol(right_prop).escaped_name.clone();
            let modifiers = self.get_declaration_modifier_flags_from_symbol(right_prop);
            if modifiers.intersects(
                tsrs2_types::ModifierFlags::PRIVATE | tsrs2_types::ModifierFlags::PROTECTED,
            ) {
                skipped_private_members.insert(name);
            } else if self.is_spreadable_property(right_prop) {
                let spread_symbol = self.get_spread_symbol(right_prop, readonly)?;
                members.insert(name, spread_symbol);
            }
        }
        for left_prop in self.get_properties_of_type(left)? {
            let name = self.binder.symbol(left_prop).escaped_name.clone();
            if skipped_private_members.contains(&name) || !self.is_spreadable_property(left_prop) {
                continue;
            }
            if let Some(&right_prop) = members.get(&name) {
                let right_type = self.get_type_of_symbol(right_prop)?;
                if self
                    .binder
                    .symbol(right_prop)
                    .flags
                    .intersects(SymbolFlags::OPTIONAL)
                {
                    let left_declarations = self.binder.symbol(left_prop).declarations.clone();
                    let right_declarations = self.binder.symbol(right_prop).declarations.clone();
                    let flags = SymbolFlags::PROPERTY
                        | (self.binder.symbol(left_prop).flags & SymbolFlags::OPTIONAL);
                    let result = self.binder.create_symbol(flags, name.clone());
                    let left_type = self.get_type_of_symbol(left_prop)?;
                    let left_without_undefined =
                        self.remove_missing_or_undefined_type(left_type)?;
                    let right_without_undefined =
                        self.remove_missing_or_undefined_type(right_type)?;
                    let result_type = if left_without_undefined == right_without_undefined {
                        left_type
                    } else {
                        self.get_union_type_ex(
                            &[left_type, right_without_undefined],
                            UnionReduction::Subtype,
                        )?
                    };
                    self.links.set_symbol_type(
                        self.speculation_depth,
                        result,
                        crate::links::LinkSlot::Resolved(result_type),
                    );
                    self.links.set_symbol_spread_pair(
                        self.speculation_depth,
                        result,
                        left_prop,
                        right_prop,
                    );
                    let mut declarations = left_declarations;
                    declarations.extend(right_declarations);
                    self.binder.symbol_mut(result).declarations = declarations;
                    let name_type = self.links.symbol(left_prop).name_type;
                    self.links
                        .set_symbol_name_type(self.speculation_depth, result, name_type);
                    members.insert(name, result);
                }
            } else {
                let spread_symbol = self.get_spread_symbol(left_prop, readonly)?;
                members.insert(name, spread_symbol);
            }
        }
        let index_infos: Vec<IndexInfo> = index_infos
            .into_iter()
            .map(|info| Self::get_index_info_with_readonly(info, readonly))
            .collect();
        let properties: Vec<SymbolId> = members.values().copied().collect();
        let spread = self.make_resolved_anonymous_type(
            symbol,
            members,
            properties,
            index_infos,
            ObjectFlags::ANONYMOUS
                | ObjectFlags::OBJECT_LITERAL
                | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL
                | ObjectFlags::CONTAINS_SPREAD
                | object_flags,
        );
        Ok(spread)
    }

    /// tsc-port: isSpreadableProperty @6.0.3
    /// tsc-hash: 252c8c781a944cd84d471f21fdd7ed2cd01c47b19a28548a94ad017c8dbdb2a1
    /// tsc-span: _tsc.js:63040-63043
    pub(crate) fn is_spreadable_property(&self, prop: SymbolId) -> bool {
        let symbol = self.binder.symbol(prop);
        let has_private_identifier = symbol.declarations.iter().any(|&decl| {
            // isPrivateIdentifierClassElementDeclaration (16472):
            // a class-element declaration named by a PrivateIdentifier.
            self.name_of_named_declaration(decl)
                .is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier)
                && self.parent_of(decl).is_some_and(|p| {
                    matches!(
                        self.kind_of(p),
                        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                    )
                })
        });
        if has_private_identifier {
            return false;
        }
        if !symbol
            .flags
            .intersects(SymbolFlags::METHOD | SymbolFlags::GET_ACCESSOR | SymbolFlags::SET_ACCESSOR)
        {
            return true;
        }
        !symbol.declarations.iter().any(|&decl| {
            self.parent_of(decl).is_some_and(|parent| {
                matches!(
                    self.kind_of(parent),
                    SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                )
            })
        })
    }

    /// tsc-port: getSpreadSymbol @6.0.3
    /// tsc-hash: 081b8438c5ab6cb37771a452a7f336cc67ee020d921994fcd1e6cc09100e97f9
    /// tsc-span: _tsc.js:63044-63056
    pub(crate) fn get_spread_symbol(
        &mut self,
        prop: SymbolId,
        readonly: bool,
    ) -> CheckResult2<SymbolId> {
        let prop_flags = self.binder.symbol(prop).flags;
        let is_setonly_accessor = prop_flags.intersects(SymbolFlags::SET_ACCESSOR)
            && !prop_flags.intersects(SymbolFlags::GET_ACCESSOR);
        if !is_setonly_accessor && readonly == self.is_readonly_symbol(prop) {
            return Ok(prop);
        }
        let flags = SymbolFlags::PROPERTY | (prop_flags & SymbolFlags::OPTIONAL);
        let name = self.binder.symbol(prop).escaped_name.clone();
        let result = self.binder.create_symbol(flags, name);
        let late = self.get_check_flags(prop) & CheckFlags::LATE;
        let check_flags = late
            | if readonly {
                CheckFlags::READONLY
            } else {
                CheckFlags::from_bits(0)
            };
        if !check_flags.is_empty() {
            self.links
                .set_symbol_check_flags(self.speculation_depth, result, check_flags);
        }
        let result_type = if is_setonly_accessor {
            self.tables.intrinsics.undefined
        } else {
            self.get_type_of_symbol(prop)?
        };
        self.links.set_symbol_type(
            self.speculation_depth,
            result,
            crate::links::LinkSlot::Resolved(result_type),
        );
        let declarations = self.binder.symbol(prop).declarations.clone();
        self.binder.symbol_mut(result).declarations = declarations;
        let name_type = self.links.symbol(prop).name_type;
        self.links
            .set_symbol_name_type(self.speculation_depth, result, name_type);
        self.links
            .set_symbol_synthetic_origin(self.speculation_depth, result, prop);
        Ok(result)
    }

    /// tsc-port: getIndexInfoWithReadonly @6.0.3
    /// tsc-hash: 25cdf96c54f3c95907ea861c50cd5c09ac5e12dd339b0139a4bb1be7849c5280
    /// tsc-span: _tsc.js:63057-63059
    fn get_index_info_with_readonly(info: IndexInfo, readonly: bool) -> IndexInfo {
        if info.is_readonly != readonly {
            IndexInfo {
                is_readonly: readonly,
                ..info
            }
        } else {
            info
        }
    }

    /// tsc isNonGenericObjectType (62918-62920).
    pub(crate) fn is_non_generic_object_type(&self, ty: TypeId) -> bool {
        self.tables.flags_of(ty).intersects(TypeFlags::OBJECT)
            && !self.is_generic_mapped_type_state(ty)
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::CompilerOptions;

    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    /// Driver-level fixture check (expr.rs idiom): full
    /// check_source_file, checker-sink rows as (code, start, length).
    /// Every expectation below is oracle-pinned (tsc 6.0.3, noLib,
    /// options {}) — scratchpad pins55c matrix, 2026-07-12.
    fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            rows(state)
        })
    }

    fn rows(state: &CheckerState) -> Vec<(u32, u32, u32)> {
        state
            .diagnostics
            .iter()
            .filter(|diag| diag.file_name.is_some())
            .map(|diag| {
                (
                    diag.code(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                )
            })
            .collect()
    }

    // ---- computed names: the 2464 legality band ----

    #[test]
    fn computed_name_of_object_type_reports_2464() {
        assert_eq!(
            checked_rows("declare const o: object;\n({ [o]: 1 });\n"),
            [(2464, 28, 3)]
        );
    }

    #[test]
    fn computed_name_of_boolean_reports_2464() {
        assert_eq!(
            checked_rows("declare const b: boolean;\n({ [b]: 1 });\n"),
            [(2464, 29, 3)]
        );
    }

    #[test]
    fn computed_name_of_null_reports_2464() {
        assert_eq!(
            checked_rows("declare const u: null;\n({ [u]: 1 });\n"),
            [(2464, 26, 3)]
        );
    }

    #[test]
    fn computed_name_of_undefined_reports_2464() {
        assert_eq!(
            checked_rows("declare const u: undefined;\n({ [u]: 1 });\n"),
            [(2464, 31, 3)]
        );
    }

    #[test]
    fn nullable_first_wins_over_string_part_in_2464() {
        // `string | undefined` — the Nullable test fires BEFORE the
        // StringLike kind test (74076).
        assert_eq!(
            checked_rows("declare const su: string | undefined;\n({ [su]: 1 });\n"),
            [(2464, 41, 4)]
        );
    }

    #[test]
    fn legal_computed_names_are_silent() {
        assert_eq!(
            checked_rows("declare const n: number;\n({ [n]: 1 });\n"),
            []
        );
        assert_eq!(checked_rows("declare const a: any;\n({ [a]: 1 });\n"), []);
        assert_eq!(
            checked_rows("declare const sym: unique symbol;\n({ [sym]: 1 });\n"),
            []
        );
        assert_eq!(
            checked_rows("declare enum E { A }\ndeclare const e: E;\n({ [e]: 1 });\n"),
            []
        );
    }

    // ---- spreads: the 2698 validity band ----

    #[test]
    fn spreading_a_number_reports_2698() {
        // Error node = the SpreadAssignment (74240).
        assert_eq!(checked_rows("({ ...42 });\n"), [(2698, 3, 5)]);
    }

    #[test]
    fn spreading_a_boolean_reports_2698() {
        assert_eq!(checked_rows("({ ...true });\n"), [(2698, 3, 7)]);
    }

    #[test]
    fn spreading_a_string_reports_2698() {
        assert_eq!(
            checked_rows("declare const s: string;\n({ ...s });\n"),
            [(2698, 28, 4)]
        );
    }

    #[test]
    fn spreading_null_and_undefined_reports_2698() {
        // The falsy-strip leaves never — INVALID, not silent (oracle
        // pin f08; the strip matters the other way: nullable UNIONS
        // spread cleanly, below).
        assert_eq!(
            checked_rows("({ ...null });\n({ ...undefined });\n"),
            [(2698, 3, 7), (2698, 18, 12)]
        );
    }

    #[test]
    fn spreading_never_and_unknown_report_2698() {
        assert_eq!(
            checked_rows("declare const nv: never;\n({ ...nv });\n"),
            [(2698, 28, 5)]
        );
        assert_eq!(
            checked_rows("declare const uk: unknown;\n({ ...uk });\n"),
            [(2698, 30, 5)]
        );
    }

    #[test]
    fn spreading_a_nullable_object_union_is_silent() {
        // THE risk-#4-adjacent verdict pin: removeDefinitelyFalsyTypes
        // strips `null` from the union before the validity test — an
        // identity stub would emit a spurious 2698 here.
        assert_eq!(
            checked_rows("declare const x: { a: number } | null;\n({ ...x });\n"),
            []
        );
    }

    #[test]
    fn spreading_an_object_union_is_silent() {
        assert_eq!(
            checked_rows("declare const u2: { a: number } | { b: string };\n({ ...u2 });\n"),
            []
        );
    }

    // ---- spread overrides: 2783 + related 2785 ----

    #[test]
    fn spread_overriding_a_property_reports_2783_with_related_2785() {
        let text = "({ a: 1, ...{ a: 2 } });\n";
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            assert_eq!(rows(state), [(2783, 3, 4)]);
            let related = &state.diagnostics[0].related;
            assert_eq!(related.len(), 1);
            assert_eq!(related[0].message.code, 2785);
            assert_eq!(related[0].start, Some(9));
            assert_eq!(related[0].length, Some(11));
        });
    }

    #[test]
    fn spread_of_required_declared_prop_reports_2783() {
        assert_eq!(
            checked_rows("declare const src: { a: number };\n({ a: 1, ...src });\n"),
            [(2783, 37, 4)]
        );
    }

    #[test]
    fn optional_spread_props_do_not_override() {
        assert_eq!(
            checked_rows("declare const src2: { a?: number };\n({ a: 1, ...src2 });\n"),
            []
        );
    }

    #[test]
    fn spread_before_the_property_is_silent() {
        assert_eq!(checked_rows("({ ...{ a: 2 }, a: 1 });\n"), []);
    }

    // ---- containment lift: literals now surface inner diagnostics ----

    #[test]
    fn array_literal_elements_are_forced() {
        assert_eq!(checked_rows("[missingName];\n"), [(2304, 1, 11)]);
    }

    #[test]
    fn object_literal_initializers_are_forced() {
        assert_eq!(checked_rows("({ a: missingName });\n"), [(2304, 6, 11)]);
    }

    #[test]
    fn clean_literals_are_silent() {
        assert_eq!(checked_rows("[1, 2, 3];\n"), []);
        assert_eq!(checked_rows("[, 1];\n"), []);
        assert_eq!(checked_rows("[...[1, 2]];\n"), []);
        assert_eq!(checked_rows("declare const a: number;\n({ a });\n"), []);
    }

    // ---- documented FN at 5.5c (oracle rows exist; ours stay []) ----

    #[test]
    fn duplicate_object_props_are_fn_until_the_grammar_slice() {
        // Oracle: 1117 (9,1) — checkGrammarObjectLiteralExpression is
        // an elided slice.
        assert_eq!(checked_rows("({ a: 1, a: 2 });\n"), []);
    }

    #[test]
    fn non_array_spread_in_array_literal_is_contained() {
        // Oracle (noLib) is ALSO silent here; lib-loaded tsc reports
        // the 2488 band — the [ITER → 5.5f] escape contains the
        // statement either way.
        assert_eq!(checked_rows("[...42];\n"), []);
    }

    #[test]
    fn object_literal_method_bodies_check_since_5_5f() {
        // Oracle: 2304 @16+11 — the deferred body pass drives the
        // method's return expression through checkExpression.
        assert_eq!(
            checked_rows("({ m() { return missingName; } });\n"),
            [(2304, 16, 11)]
        );
    }
}
