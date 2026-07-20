//! M4 5.5b: the literal-level widening carve-out (extraction doc §0
//! [WIDEN]) — pure type→type classifiers with no widening context.
//! M4 5.6: the object-level widening machinery (getWidenedTypeWithContext
//! 68020 + widening contexts + getWidenedTypeOfObjectLiteral 67997) and
//! the implicit-any reporting family (reportWideningErrorsInType 68052,
//! reportImplicitAny 68089, reportErrorsFromWidening 68187).
//! getRegularTypeOfObjectLiteral (67923) lives in engine.rs.

use tsrs2_binder::{node_util, SymbolId};
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{ObjectFlags, SymbolFlags, TypeFlags, TypeId};

use crate::state::{CheckResult2, CheckerState, WideningContext, WideningContextId};

impl<'a> CheckerState<'a> {
    /// tsc-port: getBaseTypeOfLiteralTypeForComparison @6.0.3
    /// tsc-hash: bd554f80bd0a6cab1d2af095a19a79fe0e7cd393ac2bc946ff4c28e353b40f72
    /// tsc-span: _tsc.js:67762-67764
    ///
    /// NO enum-like arm — Enum (65536) maps to number (the extraction
    /// doc calls this out against the getBaseTypeOfLiteralType shape).
    #[allow(dead_code)] // consumer: the relational-operator band (5.5e)
    pub(crate) fn get_base_type_of_literal_type_for_comparison(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(
            TypeFlags::STRING_LITERAL | TypeFlags::TEMPLATE_LITERAL | TypeFlags::STRING_MAPPING,
        ) {
            Ok(self.tables.intrinsics.string)
        } else if flags.intersects(TypeFlags::NUMBER_LITERAL | TypeFlags::ENUM) {
            Ok(self.tables.intrinsics.number)
        } else if flags.intersects(TypeFlags::BIG_INT_LITERAL) {
            Ok(self.tables.intrinsics.bigint)
        } else if flags.intersects(TypeFlags::BOOLEAN_LITERAL) {
            Ok(self.tables.intrinsics.boolean)
        } else if flags.intersects(TypeFlags::UNION) {
            Ok(self
                .map_type(
                    ty,
                    &mut |state, t| {
                        state
                            .get_base_type_of_literal_type_for_comparison(t)
                            .map(Some)
                    },
                    false,
                )?
                .expect("mapper is total"))
        } else {
            Ok(ty)
        }
    }

    /// tsc-port: getWidenedLiteralType @6.0.3
    /// tsc-hash: 34e9ce1ae0d68d982398871f0aa07073f045e652899c902b0c4a97d64dd04f9a
    /// tsc-span: _tsc.js:67765-67767
    ///
    /// Only FRESH literals widen; regular literals pass through.
    pub(crate) fn get_widened_literal_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        let fresh = self.tables.is_fresh_literal_type(ty);
        if flags.intersects(TypeFlags::ENUM_LIKE) && fresh {
            self.get_base_type_of_enum_like_type(ty)
        } else if flags.intersects(TypeFlags::STRING_LITERAL) && fresh {
            Ok(self.tables.intrinsics.string)
        } else if flags.intersects(TypeFlags::NUMBER_LITERAL) && fresh {
            Ok(self.tables.intrinsics.number)
        } else if flags.intersects(TypeFlags::BIG_INT_LITERAL) && fresh {
            Ok(self.tables.intrinsics.bigint)
        } else if flags.intersects(TypeFlags::BOOLEAN_LITERAL) && fresh {
            Ok(self.tables.intrinsics.boolean)
        } else if flags.intersects(TypeFlags::UNION) {
            Ok(self
                .map_type(
                    ty,
                    &mut |state, t| state.get_widened_literal_type(t).map(Some),
                    false,
                )?
                .expect("mapper is total"))
        } else {
            Ok(ty)
        }
    }

    /// tsc-port: getWidenedUniqueESSymbolType @6.0.3
    /// tsc-hash: 004e6feb812db03248e01232736667a491d945d662999742b4b85398a051d86a
    /// tsc-span: _tsc.js:67768-67770
    pub(crate) fn get_widened_unique_es_symbol_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNIQUE_ES_SYMBOL) {
            Ok(self.tables.intrinsics.es_symbol)
        } else if flags.intersects(TypeFlags::UNION) {
            Ok(self
                .map_type(
                    ty,
                    &mut |state, t| state.get_widened_unique_es_symbol_type(t).map(Some),
                    false,
                )?
                .expect("mapper is total"))
        } else {
            Ok(ty)
        }
    }

    /// tsc-port: getWidenedLiteralLikeTypeForContextualType @6.0.3
    /// tsc-hash: e37987d1c869b101752178cb673a0723ddca1f24e403363c9eba0b8238ba7107
    /// tsc-span: _tsc.js:67771-67776
    pub(crate) fn get_widened_literal_like_type_for_contextual_type(
        &mut self,
        ty: TypeId,
        contextual_type: Option<TypeId>,
    ) -> CheckResult2<TypeId> {
        let mut ty = ty;
        if !self.is_literal_of_contextual_type(ty, contextual_type)? {
            let widened = self.get_widened_literal_type(ty)?;
            ty = self.get_widened_unique_es_symbol_type(widened)?;
        }
        Ok(self.tables.get_regular_type_of_literal_type(ty))
    }

    /// tsc-port: getWidenedLiteralLikeTypeForContextualReturnTypeIfNeeded @6.0.3
    /// tsc-hash: e4a1b137182f82fa0678d1ae3be9c2b587f29bdc5a8011d40f409f55fadf4e28
    /// tsc-span: _tsc.js:67777-67783
    #[allow(dead_code)]
    pub(crate) fn get_widened_literal_like_type_for_contextual_return_type_if_needed(
        &mut self,
        ty: Option<TypeId>,
        contextual_signature_return_type: Option<TypeId>,
        is_async: bool,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(current) = ty else {
            return Ok(ty);
        };
        if !self.is_unit_type(current) {
            return Ok(ty);
        }
        let contextual_type = match contextual_signature_return_type {
            None => None,
            // 67779: async contexts compare against the PROMISED type.
            Some(signature_return) if is_async => {
                self.get_promised_type_of_promise(signature_return)?
            }
            Some(signature_return) => Some(signature_return),
        };
        Ok(Some(
            self.get_widened_literal_like_type_for_contextual_type(current, contextual_type)?,
        ))
    }

    /// tsc-port: getWidenedLiteralLikeTypeForContextualIterationTypeIfNeeded @6.0.3
    /// tsc-hash: bf2483d08e235cdcd45b14bb2336905208671b3533c617ac30218cc53188328e
    /// tsc-span: _tsc.js:67784-67790
    pub(crate) fn get_widened_literal_like_type_for_contextual_iteration_type_if_needed(
        &mut self,
        ty: Option<TypeId>,
        contextual_signature_return_type: Option<TypeId>,
        kind: tsrs2_types::IterationTypeKind,
        is_async_generator: bool,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(current) = ty else {
            return Ok(ty);
        };
        if !self.is_unit_type(current) {
            return Ok(ty);
        }
        let contextual_type = match contextual_signature_return_type {
            Some(contextual) => self.get_iteration_type_of_generator_function_return_type(
                kind,
                contextual,
                is_async_generator,
            )?,
            None => None,
        };
        Ok(Some(
            self.get_widened_literal_like_type_for_contextual_type(current, contextual_type)?,
        ))
    }
    // ---- M4 5.6: widening contexts + object-level widening ----

    /// tsc-port: createWideningContext @6.0.3
    /// tsc-hash: 45090097a709c1c9f72722e2fbd4bc8281837f4b16d6d769e70215a9a94c65c4
    /// tsc-span: _tsc.js:67939-67941
    fn create_widening_context(
        &mut self,
        parent: Option<WideningContextId>,
        property_name: Option<String>,
        siblings: Option<Vec<TypeId>>,
    ) -> WideningContextId {
        self.widening_contexts.push(WideningContext {
            parent,
            property_name,
            siblings,
            resolved_properties: None,
        });
        self.widening_contexts.len() - 1
    }

    /// tsc-port: getSiblingsOfContext @6.0.3
    /// tsc-hash: 3dac07f8bf252c880bfaa9d9f2adec940e39b141952124924689aec63158811c
    /// tsc-span: _tsc.js:67942-67958
    ///
    /// Root contexts are created WITH siblings (the union arm), so the
    /// lazy fill only ever walks a parent that exists.
    fn get_siblings_of_context(&mut self, context: WideningContextId) -> CheckResult2<Vec<TypeId>> {
        if let Some(siblings) = &self.widening_contexts[context].siblings {
            return Ok(siblings.clone());
        }
        let parent = self.widening_contexts[context]
            .parent
            .expect("sibling-less context has a parent (createWideningContext callers)");
        let property_name = self.widening_contexts[context]
            .property_name
            .clone()
            .expect("child contexts carry a property name");
        let mut siblings = Vec::new();
        for ty in self.get_siblings_of_context(parent)? {
            if !self.is_object_literal_type(ty) {
                continue;
            }
            let Some(prop) = self.get_property_of_object_type(ty, &property_name)? else {
                continue;
            };
            let prop_type = self.get_type_of_symbol(prop)?;
            self.for_each_type_members(prop_type, &mut siblings);
        }
        self.widening_contexts[context].siblings = Some(siblings.clone());
        Ok(siblings)
    }

    /// tsc forEachType over a possibly-union type, collecting members.
    fn for_each_type_members(&self, ty: TypeId, out: &mut Vec<TypeId>) {
        if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            match &self.tables.type_of(ty).data {
                tsrs2_types::TypeData::Union { types, .. } => out.extend(types.iter().copied()),
                _ => unreachable!("union flag implies union data"),
            }
        } else {
            out.push(ty);
        }
    }

    /// tsc-port: getPropertiesOfContext @6.0.3
    /// tsc-hash: 649a6b3c66e70770397ff8327344865ee0da59b7b5c0168c022e79cf4a50835c
    /// tsc-span: _tsc.js:67959-67972
    ///
    /// The JS Map keeps the FIRST insertion position and the LAST
    /// value per name — reproduced with an index map over a Vec.
    fn get_properties_of_context(
        &mut self,
        context: WideningContextId,
    ) -> CheckResult2<Vec<SymbolId>> {
        if let Some(resolved) = &self.widening_contexts[context].resolved_properties {
            return Ok(resolved.clone());
        }
        let mut names: Vec<SymbolId> = Vec::new();
        let mut index_of: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for t in self.get_siblings_of_context(context)? {
            if !self.is_object_literal_type(t)
                || self
                    .tables
                    .object_flags_of(t)
                    .intersects(ObjectFlags::CONTAINS_SPREAD)
            {
                continue;
            }
            for prop in self.get_properties_of_type_full(t)? {
                let name = self.binder.symbol(prop).escaped_name.clone();
                match index_of.get(&name) {
                    Some(&slot) => names[slot] = prop,
                    None => {
                        index_of.insert(name, names.len());
                        names.push(prop);
                    }
                }
            }
        }
        self.widening_contexts[context].resolved_properties = Some(names.clone());
        Ok(names)
    }

    /// tsc-port: getWidenedProperty @6.0.3
    /// tsc-hash: 368585e7c70a54a58d6242bc084b55605482901f6d65631476aed3599355241a
    /// tsc-span: _tsc.js:67973-67986
    fn get_widened_property(
        &mut self,
        prop: SymbolId,
        context: Option<WideningContextId>,
    ) -> CheckResult2<SymbolId> {
        if !self.symbol_flags(prop).intersects(SymbolFlags::PROPERTY) {
            // Since get_type_of_symbol is a lazily attached full-face
            // accessor in tsrs2 too, methods and accessors pass
            // through unwidened exactly like tsc's early return.
            return Ok(prop);
        }
        let original = self.get_type_of_symbol(prop)?;
        let prop_context = context.map(|parent| {
            let name = self.binder.symbol(prop).escaped_name.clone();
            self.create_widening_context(Some(parent), Some(name), /*siblings*/ None)
        });
        let widened = self.get_widened_type_with_context(original, prop_context)?;
        Ok(if widened == original {
            prop
        } else {
            self.create_symbol_with_type(prop, Some(widened))
        })
    }

    /// tsc-port: getUndefinedProperty @6.0.3
    /// tsc-hash: 496749dd0e04e2129dd7349f4799f3f91892cca966e406f1ac715496704e4071
    /// tsc-span: _tsc.js:67987-67996
    ///
    /// undefinedOrMissingType, like the sister consumers
    /// (literals.rs tuple elements, facts.rs) — the missing flavor is
    /// what keeps a widened absent property assignable under
    /// exactOptionalPropertyTypes (m4-review A13; the old
    /// "eOPT is unmodeled" justification was false — options.rs
    /// carries it and tables.rs computes the intrinsic from it).
    fn get_undefined_property(&mut self, prop: SymbolId) -> SymbolId {
        let name = self.binder.symbol(prop).escaped_name.clone();
        if let Some(&cached) = self.undefined_properties.get(&name) {
            return cached;
        }
        let undefined_or_missing = self.tables.intrinsics.undefined_or_missing;
        let result = self.create_symbol_with_type(prop, Some(undefined_or_missing));
        self.binder.symbol_mut(result).flags |= SymbolFlags::OPTIONAL;
        self.undefined_properties.insert(name, result);
        result
    }

    /// tsc-port: getWidenedTypeOfObjectLiteral @6.0.3
    /// tsc-hash: d819a87ed648eac431abbfa6176b9ed9e32712747412f28fd5b755bef724277c
    /// tsc-span: _tsc.js:67997-68012
    fn get_widened_type_of_object_literal(
        &mut self,
        ty: TypeId,
        context: Option<WideningContextId>,
    ) -> CheckResult2<TypeId> {
        let mut members = tsrs2_binder::SymbolTable::default();
        let mut properties: Vec<SymbolId> = Vec::new();
        for prop in self.get_properties_of_object_type_owned(ty)? {
            let widened = self.get_widened_property(prop, context)?;
            let name = self.binder.symbol(widened).escaped_name.clone();
            members.insert(name, widened);
            properties.push(widened);
        }
        if let Some(context) = context {
            for prop in self.get_properties_of_context(context)? {
                let name = self.binder.symbol(prop).escaped_name.clone();
                if members.get(&name).is_none() {
                    let undefined_prop = self.get_undefined_property(prop);
                    members.insert(name, undefined_prop);
                    properties.push(undefined_prop);
                }
            }
        }
        let mut index_infos = self.get_index_infos_of_type(ty)?;
        for info in &mut index_infos {
            info.value_type = self.get_widened_type(info.value_type)?;
        }
        let symbol = self.tables.type_of(ty).symbol;
        let result = self
            .tables
            .create_type(TypeFlags::OBJECT, tsrs2_types::TypeData::Object);
        let carried = self.tables.object_flags_of(ty).bits()
            & (ObjectFlags::JS_LITERAL.bits() | ObjectFlags::NON_INFERRABLE_TYPE.bits());
        self.tables.type_mut(result).object_flags =
            ObjectFlags::from_bits(ObjectFlags::ANONYMOUS.bits() | carried);
        self.tables.type_mut(result).symbol = symbol;
        let members_id = self.alloc_members(crate::state::ResolvedMembers {
            members,
            properties,
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            index_infos,
        });
        self.links.set_type_members(
            self.speculation_depth,
            result,
            crate::links::LinkSlot::Resolved(members_id),
        );
        Ok(result)
    }

    /// tsc-port: getWidenedType @6.0.3
    /// tsc-hash: f2b817e75f05ad2b8275ab420eaf4c7a47af72ffd405ec668a7ef85f33732149
    /// tsc-span: _tsc.js:68013-68019
    pub(crate) fn get_widened_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        self.get_widened_type_with_context(ty, /*context*/ None)
    }

    /// tsc-port: getWidenedTypeWithContext @6.0.3
    /// tsc-hash: 41e001be4631d04bbb61f4c0862b64be3385aa5cd6d2afb30a9c5157fcd43a00
    /// tsc-span: _tsc.js:68020-68051
    pub(crate) fn get_widened_type_with_context(
        &mut self,
        ty: TypeId,
        context: Option<WideningContextId>,
    ) -> CheckResult2<TypeId> {
        const REQUIRES_WIDENING: i32 = ObjectFlags::CONTAINS_WIDENING_TYPE.bits()
            | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL.bits();
        if self.tables.object_flags_of(ty).bits() & REQUIRES_WIDENING == 0 {
            return Ok(ty);
        }
        if context.is_none() {
            if let Some(widened) = self.links.ty(ty).widened {
                return Ok(widened);
            }
        }
        let flags = self.tables.flags_of(ty);
        let mut result: Option<TypeId> = None;
        if flags.intersects(TypeFlags::ANY | TypeFlags::NULLABLE) {
            result = Some(self.tables.intrinsics.any);
        } else if self.is_object_literal_type(ty) {
            result = Some(self.get_widened_type_of_object_literal(ty, context)?);
        } else if flags.intersects(TypeFlags::UNION) {
            let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                tsrs2_types::TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            };
            let union_context = match context {
                Some(context) => context,
                None => self.create_widening_context(
                    /*parent*/ None,
                    /*property_name*/ None,
                    Some(members.clone()),
                ),
            };
            let mut widened = Vec::with_capacity(members.len());
            for member in &members {
                let next = if self
                    .tables
                    .flags_of(*member)
                    .intersects(TypeFlags::NULLABLE)
                {
                    *member
                } else {
                    self.get_widened_type_with_context(*member, Some(union_context))?
                };
                widened.push(next);
            }
            let mut any_empty_object = false;
            for member in &widened {
                if self.is_empty_object_type(*member)? {
                    any_empty_object = true;
                    break;
                }
            }
            let reduction = if any_empty_object {
                tsrs2_types::UnionReduction::Subtype
            } else {
                tsrs2_types::UnionReduction::Literal
            };
            result = Some(self.get_union_type_ex(&widened, reduction)?);
        } else if flags.intersects(TypeFlags::INTERSECTION) {
            let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                tsrs2_types::TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("intersection flag implies intersection data"),
            };
            let mut widened = Vec::with_capacity(members.len());
            for member in members {
                widened.push(self.get_widened_type(member)?);
            }
            result =
                Some(self.get_intersection_type(&widened, tsrs2_types::IntersectionFlags::NONE)?);
        } else if self.is_array_type(ty)? || self.tables.is_tuple_type(ty) {
            let arguments = self.get_type_arguments(ty)?;
            let mut widened = Vec::with_capacity(arguments.len());
            for argument in arguments {
                widened.push(self.get_widened_type(argument)?);
            }
            let target = self.tables.reference_target(ty);
            result = Some(self.tables.create_type_reference(target, &widened));
        }
        if let Some(result) = result {
            if context.is_none() {
                self.links
                    .set_type_widened(self.speculation_depth, ty, result);
            }
        }
        Ok(result.unwrap_or(ty))
    }

    // ---- M4 5.6: implicit-any reporting ----

    /// tsc-port: reportWideningErrorsInType @6.0.3
    /// tsc-hash: 4507e2924a947f4e005f74a926638c423e0b8b7aaf893b32f32fd5b4f562b041
    /// tsc-span: _tsc.js:68052-68088
    ///
    /// Returns whether an error was reported (the innermost widening
    /// property wins; reportImplicitAny only fires when nothing here
    /// found a property to blame).
    fn report_widening_errors_in_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let mut error_reported = false;
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::CONTAINS_WIDENING_TYPE)
        {
            return Ok(false);
        }
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNION) {
            let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                tsrs2_types::TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            };
            let mut any_empty_object = false;
            for member in &members {
                if self.is_empty_object_type(*member)? {
                    any_empty_object = true;
                    break;
                }
            }
            if any_empty_object {
                error_reported = true;
            } else {
                for member in members {
                    if !error_reported {
                        error_reported = self.report_widening_errors_in_type(member)?;
                    }
                }
            }
        } else if self.is_array_type(ty)? || self.tables.is_tuple_type(ty) {
            for argument in self.get_type_arguments(ty)? {
                if !error_reported {
                    error_reported = self.report_widening_errors_in_type(argument)?;
                }
            }
        } else if self.is_object_literal_type(ty) {
            for p in self.get_properties_of_object_type_owned(ty)? {
                let t = self.get_type_of_symbol(p)?;
                if !self
                    .tables
                    .object_flags_of(t)
                    .intersects(ObjectFlags::CONTAINS_WIDENING_TYPE)
                {
                    continue;
                }
                error_reported = self.report_widening_errors_in_type(t)?;
                if !error_reported {
                    let type_symbol_value_declaration = self
                        .tables
                        .type_of(ty)
                        .symbol
                        .and_then(|symbol| self.binder.symbol(symbol).value_declaration);
                    let declarations = self.binder.symbol(p).declarations.clone();
                    let value_declaration = declarations.into_iter().find(|&d| {
                        self.binder
                            .node_symbol(d)
                            .and_then(|symbol| self.binder.symbol(symbol).value_declaration)
                            .and_then(|value_declaration| self.parent_of(value_declaration))
                            == type_symbol_value_declaration
                    });
                    if let Some(value_declaration) = value_declaration {
                        let widened = self.get_widened_type(t)?;
                        let type_string = self.type_to_string_slice(widened)?;
                        let symbol_string = self.symbol_display_name(p);
                        self.error_at(
                            Some(value_declaration),
                            &diagnostics::Object_literal_s_property_0_implicitly_has_an_1_type,
                            &[&symbol_string, &type_string],
                        );
                        error_reported = true;
                    }
                }
            }
        }
        Ok(error_reported)
    }

    /// tsc-port: reportImplicitAny @6.0.3
    /// tsc-hash: 9fc2b1e4bb5bb2fc5cd60b54b69c437744b338cf28b94704ac6ce447d1115f08
    /// tsc-span: _tsc.js:68089-68162
    ///
    /// errorOrSuggestion is error_at both ways: the !noImplicitAny
    /// constants (7043-7050) are Suggestion-category in gen.rs, so the
    /// category rides the message like tsc's addErrorOrSuggestion.
    /// The JSDocFunctionType/JSDocSignature arms escape ([JSDOC]).
    pub(crate) fn report_implicit_any(
        &mut self,
        declaration: NodeId,
        ty: TypeId,
        widening_kind: Option<tsrs2_types::WideningKind>,
    ) -> CheckResult2<()> {
        use tsrs2_types::WideningKind;
        let widened = self.get_widened_type(ty)?;
        let type_as_string = self.type_to_string_slice(widened)?;
        if self.is_in_js_file(declaration) && self.options.check_js != Some(true) {
            // isCheckJsEnabledForFile: checkJs pragmas are unmodeled,
            // so only the option can enable JS-file reports.
            return Ok(());
        }
        let no_implicit_any = self
            .options
            .strict_option_value(self.options.no_implicit_any);
        let source = self.binder.source_of_node(declaration);
        let diagnostic: &'static tsrs2_diags::DiagnosticMessage = match self.kind_of(declaration) {
            SyntaxKind::BinaryExpression
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature => {
                if no_implicit_any {
                    &diagnostics::Member_0_implicitly_has_an_1_type
                } else {
                    &diagnostics::Member_0_implicitly_has_an_1_type_but_a_better_type_may_be_inferred_from_usage
                }
            }
            SyntaxKind::Parameter => {
                let (name, dot_dot_dot) = match self.data_of(declaration) {
                    NodeData::Parameter(data) => (data.name, data.dot_dot_dot_token.is_some()),
                    _ => (None, false),
                };
                if let Some(name) = name {
                    if self.kind_of(name) == SyntaxKind::Identifier {
                        let name_text = self.text_of_node(name)?;
                        let parent = self.parent_of(declaration);
                        let parent_kind = parent.map(|parent| self.kind_of(parent));
                        let in_signature_parent = matches!(
                            parent_kind,
                            Some(
                                SyntaxKind::CallSignature
                                    | SyntaxKind::MethodSignature
                                    | SyntaxKind::FunctionType
                            )
                        );
                        if in_signature_parent {
                            let parent = parent.expect("kind implies parent");
                            let parameters = self.parameter_nodes_of_signature_like(parent);
                            let index = parameters.iter().position(|&p| p == declaration);
                            let resolves_as_type = self
                                .resolve_name(
                                    Some(declaration),
                                    &tsrs2_binder::escape_leading_underscores(&name_text),
                                    SymbolFlags::TYPE,
                                    /*name_not_found_message*/ None,
                                    /*is_use*/ true,
                                    /*exclude_globals*/ false,
                                )?
                                .is_some();
                            let keyword_is_type_node = tsrs2_syntax::keyword_kind(&name_text)
                                .is_some_and(|kind| self.is_type_node_kind(kind));
                            if let Some(index) = index {
                                if resolves_as_type || keyword_is_type_node {
                                    let new_name = format!("arg{index}");
                                    let type_name = format!(
                                        "{}{}",
                                        name_text,
                                        if dot_dot_dot { "[]" } else { "" }
                                    );
                                    self.error_at(
                                        Some(declaration),
                                        &diagnostics::Parameter_has_a_name_but_no_type_Did_you_mean_0_1,
                                        &[&new_name, &type_name],
                                    );
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
                if dot_dot_dot {
                    if no_implicit_any {
                        &diagnostics::Rest_parameter_0_implicitly_has_an_any_type
                    } else {
                        &diagnostics::Rest_parameter_0_implicitly_has_an_any_type_but_a_better_type_may_be_inferred_from_usage
                    }
                } else if no_implicit_any {
                    &diagnostics::Parameter_0_implicitly_has_an_1_type
                } else {
                    &diagnostics::Parameter_0_implicitly_has_an_1_type_but_a_better_type_may_be_inferred_from_usage
                }
            }
            SyntaxKind::BindingElement => {
                if !no_implicit_any {
                    return Ok(());
                }
                &diagnostics::Binding_element_0_implicitly_has_an_1_type
            }
            SyntaxKind::JSDocFunctionType | SyntaxKind::JSDocSignature => {
                return Err(crate::state::Unsupported::new(
                    "reportImplicitAny JSDoc arms ([JSDOC] M8)",
                ));
            }
            SyntaxKind::FunctionDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction => {
                // `!declaration.name` reads the RAW name field — an
                // anonymous fn expression stays anonymous here even
                // though getNameOfDeclaration would surface the
                // ASSIGNED name (oracle pin: 7011, not 7010-with-'h').
                let name = self.name_of_node(declaration);
                if no_implicit_any && name.is_none() {
                    let message = if widening_kind == Some(WideningKind::GENERATOR_YIELD) {
                        &diagnostics::Generator_implicitly_has_yield_type_0_Consider_supplying_a_return_type_annotation
                    } else {
                        &diagnostics::Function_expression_which_lacks_return_type_annotation_implicitly_has_an_0_return_type
                    };
                    self.error_at(Some(declaration), message, &[&type_as_string]);
                    return Ok(());
                }
                if !no_implicit_any {
                    &diagnostics::_0_implicitly_has_an_1_return_type_but_a_better_type_may_be_inferred_from_usage
                } else if widening_kind == Some(WideningKind::GENERATOR_YIELD) {
                    &diagnostics::_0_which_lacks_return_type_annotation_implicitly_has_an_1_yield_type
                } else {
                    &diagnostics::_0_which_lacks_return_type_annotation_implicitly_has_an_1_return_type
                }
            }
            SyntaxKind::MappedType => {
                if no_implicit_any {
                    self.error_at(
                        Some(declaration),
                        &diagnostics::Mapped_object_type_implicitly_has_an_any_template_type,
                        &[],
                    );
                }
                return Ok(());
            }
            _ => {
                if no_implicit_any {
                    &diagnostics::Variable_0_implicitly_has_an_1_type
                } else {
                    &diagnostics::Variable_0_implicitly_has_an_1_type_but_a_better_type_may_be_inferred_from_usage
                }
            }
        };
        let name = node_util::get_name_of_declaration(source, declaration);
        let name_string = match name {
            Some(name) => self.text_of_node(name)?,
            None => "(Missing)".to_owned(),
        };
        self.error_at(
            Some(declaration),
            diagnostic,
            &[&name_string, &type_as_string],
        );
        Ok(())
    }

    /// The Parameter NodeArray of a signature-like parent (the 7051
    /// `param.parent.parameters.includes(param)` read).
    fn parameter_nodes_of_signature_like(&self, node: NodeId) -> Vec<NodeId> {
        let parameters = match self.data_of(node) {
            NodeData::CallSignature(data) => data.parameters,
            NodeData::MethodSignature(data) => data.parameters,
            NodeData::FunctionType(data) => data.parameters,
            _ => None,
        };
        self.nodes_of(parameters)
    }

    /// tsc-port: shouldReportErrorsFromWideningWithContextualSignature @6.0.3
    /// tsc-hash: 63e7cd6e79236567f70332dbf955020fe94571a9bd466c4e58de94b575cc819f
    /// tsc-span: _tsc.js:68163-68186
    fn should_report_errors_from_widening_with_contextual_signature(
        &mut self,
        declaration: NodeId,
        widening_kind: tsrs2_types::WideningKind,
    ) -> CheckResult2<bool> {
        use tsrs2_types::WideningKind;
        let Some(signature) =
            self.get_contextual_signature_for_function_like_declaration(declaration)?
        else {
            return Ok(true);
        };
        let mut return_type = self.get_return_type_of_signature(signature)?;
        let flags = self.get_function_flags(declaration);
        let is_async = flags & crate::functions::FUNCTION_FLAGS_ASYNC != 0;
        match widening_kind {
            WideningKind::FUNCTION_RETURN => {
                if flags & crate::functions::FUNCTION_FLAGS_GENERATOR != 0 {
                    if let Some(iteration) = self
                        .get_iteration_type_of_generator_function_return_type(
                            tsrs2_types::IterationTypeKind::RETURN,
                            return_type,
                            is_async,
                        )?
                    {
                        return_type = iteration;
                    }
                } else if is_async {
                    if let Some(awaited) =
                        self.get_awaited_type_no_alias(return_type, /*error_info*/ None)?
                    {
                        return_type = awaited;
                    }
                }
                Ok(self.tables.is_generic_type(return_type))
            }
            WideningKind::GENERATOR_YIELD => {
                let yield_type = self.get_iteration_type_of_generator_function_return_type(
                    tsrs2_types::IterationTypeKind::YIELD,
                    return_type,
                    is_async,
                )?;
                Ok(match yield_type {
                    Some(yield_type) => self.tables.is_generic_type(yield_type),
                    None => false,
                })
            }
            WideningKind::GENERATOR_NEXT => {
                let next_type = self.get_iteration_type_of_generator_function_return_type(
                    tsrs2_types::IterationTypeKind::NEXT,
                    return_type,
                    is_async,
                )?;
                Ok(match next_type {
                    Some(next_type) => self.tables.is_generic_type(next_type),
                    None => false,
                })
            }
            _ => Ok(false),
        }
    }

    /// tsc-port: reportErrorsFromWidening @6.0.3
    /// tsc-hash: 3f660239a8c5b20636c27182b6a777a91da8080b7b0715db44996cd5c05bbe1e
    /// tsc-span: _tsc.js:68187-68197
    ///
    /// addLazyDiagnostic = eager identity (5.4 driver decision;
    /// sort_and_dedupe owns final order).
    pub(crate) fn report_errors_from_widening(
        &mut self,
        declaration: NodeId,
        ty: TypeId,
        widening_kind: Option<tsrs2_types::WideningKind>,
    ) -> CheckResult2<()> {
        let no_implicit_any = self
            .options
            .strict_option_value(self.options.no_implicit_any);
        if !(no_implicit_any
            && self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::CONTAINS_WIDENING_TYPE))
        {
            return Ok(());
        }
        let should_report = match widening_kind {
            None => true,
            Some(kind) => {
                node_util::is_function_like_declaration_kind(self.kind_of(declaration))
                    && self.should_report_errors_from_widening_with_contextual_signature(
                        declaration,
                        kind,
                    )?
            }
        };
        if should_report && !self.report_widening_errors_in_type(ty)? {
            self.report_implicit_any(declaration, ty, widening_kind)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::CompilerOptions;

    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    /// Driver-level fixture check (operators.rs idiom): oracle-pinned
    /// rows (tsc 6.0.3, noLib, options per test) — scratchpad
    /// pin56_{a..e}.ts probes, 2026-07-13.
    fn checked_rows_with(text: &str, options: &CompilerOptions) -> Vec<(u32, u32, u32)> {
        with_program_state(&[("a.ts", text)], options, |state| {
            state.check_source_file(0);
            rows(state)
        })
    }

    fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
        checked_rows_with(text, &CompilerOptions::default())
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

    // ---- the auto-type family (getTypeForVariableLikeDeclaration
    // auto arm — flow-evolved, live since 6.2/6.6) ----

    #[test]
    fn auto_family_renders_no_false_relations() {
        // Oracle rows: 2339 toFixed-on-number (flow-evolved — LIVE
        // since the 6.6f gate retirement), 7053 on c[0] (LIVE since
        // M6 7.5's "{}" display arm — the evolving never[] index
        // renders '{}' under the noLib Array miss; re-probed
        // probe75d.mjs), 6133 ×2 (M7), 7005 ×2 (live). The pin still
        // asserts the FP face: NO 2322 from `b = 5` against a
        // null-typed b.
        assert_eq!(
            checked_rows(
                "let b = null;\nb = 5;\nb.toFixed();\nlet c = [];\nc[0] = 1;\nexport let v1;\nv1;\ndeclare let d1;\nd1;\n"
            ),
            [(2339, 23, 7), (7053, 46, 4), (7005, 67, 2), (7005, 87, 2)]
        );
    }

    #[test]
    fn const_null_keeps_the_null_type_and_reports_implicit_any_bands() {
        // 6133 rows are M7 FN.
        assert_eq!(
            checked_rows(
                "const b2 = null;\ndeclare let n1: number;\nn1 = b2;\nconst f = function (x) { return 1; };\nf;\nconst fb = function ({ c }, [d]) { return 1; };\nfb;\n"
            ),
            [(2322, 41, 2), (7006, 70, 1), (7031, 114, 1), (7031, 120, 1)]
        );
    }

    // ---- sibling-context widening (getWidenedTypeOfObjectLiteral +
    // getUndefinedProperty) and fresh/regular round-trip ----

    #[test]
    fn union_widening_synthesizes_optional_undefined_siblings() {
        // ABSENCE pin (oracle: no diagnostics): without the sibling
        // context (getUndefinedProperty), `t.b` / `nested.p.y` would
        // render 2339 on the arm that lacks the property — the FP
        // shape the context machinery exists to prevent. The positive
        // faces (2322 `number | undefined`, 2741/2353 displays) sit
        // behind the M5 narrowable-union assignment gate and the T2
        // anonymous display slice.
        assert_eq!(
            checked_rows(
                "declare const cond: boolean;\nconst t = cond ? { a: 1 } : { b: 2 };\nt.a;\nt.b;\nconst nested = cond ? { p: { x: 1 } } : { p: { y: \"s\" } };\nnested.p.x;\nnested.p.y;\n"
            ),
            []
        );
    }

    // ---- reportImplicitAny suggestion band (!noImplicitAny) ----

    #[test]
    fn loose_mode_reports_suggestion_variants() {
        // 6133 is M7 FN; 7043/7044 are Suggestion-category rows that
        // ride the same T0 key space.
        let options = CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        };
        assert_eq!(
            checked_rows_with(
                "let a;\na = 1;\nconst f = function (x) { return 1; };\nf;\n",
                &options
            ),
            [(7043, 4, 1), (7044, 34, 1)]
        );
    }

    // ---- reportWideningErrorsInType / reportErrorsFromWidening under
    // noImplicitAny + strictNullChecks:false (nullWideningType) ----

    #[test]
    fn null_widening_reports_7018_and_7011() {
        let options = CompilerOptions {
            no_implicit_any: Some(true),
            strict_null_checks: Some(false),
            ..CompilerOptions::default()
        };
        // The arr row is an ABSENCE pin: under noLib the oracle emits
        // nothing for `const arr = [null]` (no 7005).
        assert_eq!(
            checked_rows_with(
                "const o1 = { a: null };\no1;\nconst h = function () { return null; };\nh;\nconst k = function () { return { a: null }; };\nk;\nconst arr = [null];\narr;\n",
                &options
            ),
            [(7018, 13, 7), (7011, 38, 8), (7018, 104, 7)]
        );
    }
}
