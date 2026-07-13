//! The MINIMAL type-from-annotation path (m3-types-relations-steps.md
//! stage 4.1) — an explicitly scoped slice of M4 5.1/5.3, each fn a
//! ledgered (partial) port. Everything a TypeMapper would touch is
//! Unsupported by construction; M4 5.1 replaces this module's dispatch
//! with the full getTypeFromTypeNode port.

use tsrs2_binder::{node_util, InternalSymbolName, SymbolId};
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{NodeArrayId, NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    CheckFlags, CheckMode, ElementFlags, IntersectionFlags, LiteralValue, M4Dependency,
    ModifierFlags, ObjectFlags, PseudoBigInt, SignatureFlags, SymbolFlags, TypeData, TypeFlags,
    TypeId, UnionReduction,
};

use crate::evaluate::EvalValue;

use crate::links::LinkSlot;
use crate::state::{
    CheckResult2, CheckerState, IndexInfo, MembersId, ResolvedMembers, Signature, SignatureId,
    Unsupported,
};

impl<'a> CheckerState<'a> {
    // ---- node helpers ----

    pub(crate) fn kind_of(&self, node: NodeId) -> SyntaxKind {
        self.binder.source_of_node(node).arena.node(node).kind
    }

    pub(crate) fn data_of(&self, node: NodeId) -> &'a NodeData {
        &self.binder.source_of_node(node).arena.node(node).data
    }

    pub(crate) fn parent_of(&self, node: NodeId) -> Option<NodeId> {
        self.binder.source_of_node(node).arena.node(node).parent
    }

    pub(crate) fn nodes_of(&self, array: Option<NodeArrayId>) -> Vec<NodeId> {
        match array {
            Some(array) => self.binder.node_array(array).nodes.clone(),
            None => Vec::new(),
        }
    }

    pub(crate) fn identifier_text(&self, node: NodeId) -> Option<&str> {
        match self.data_of(node) {
            NodeData::Identifier(data) => Some(&data.escaped_text),
            _ => None,
        }
    }

    pub(crate) fn unsupported_m4(err: M4Dependency) -> Unsupported {
        Unsupported::new(err.0)
    }

    // ---- the annotation entry ----

    /// tsc-port: getTypeFromTypeNode @6.0.3
    /// tsc-hash: 5d4a798af65bf23738c21df6d7142d44f9ac093ea314f620267fde2a974f3004
    /// tsc-span: _tsc.js:63196-63198
    ///
    /// getConditionalFlowTypeOfType (60454) is identity without
    /// conditional types; it returns with M4 5.2.
    ///
    /// tsc-port: getTypeFromTypeNodeWorker @6.0.3
    /// tsc-hash: 5de45dfdb59c76a72c1b56d2d18859eae20ca9e9db0ff6aa6c4d6aeea0eaf912
    /// tsc-span: _tsc.js:63199-63297
    pub fn get_type_from_type_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        match self.kind_of(node) {
            SyntaxKind::AnyKeyword => Ok(self.tables.intrinsics.any),
            SyntaxKind::UnknownKeyword => Ok(self.tables.intrinsics.unknown),
            SyntaxKind::StringKeyword => Ok(self.tables.intrinsics.string),
            SyntaxKind::NumberKeyword => Ok(self.tables.intrinsics.number),
            SyntaxKind::BigIntKeyword => Ok(self.tables.intrinsics.bigint),
            SyntaxKind::BooleanKeyword => Ok(self.tables.intrinsics.boolean),
            SyntaxKind::SymbolKeyword => Ok(self.tables.intrinsics.es_symbol),
            SyntaxKind::VoidKeyword => Ok(self.tables.intrinsics.void),
            SyntaxKind::UndefinedKeyword => Ok(self.tables.intrinsics.undefined),
            SyntaxKind::NullKeyword => Ok(self.tables.intrinsics.null),
            SyntaxKind::NeverKeyword => Ok(self.tables.intrinsics.never),
            // The JS-file-without-noImplicitAny branch (63225) is
            // unreachable: the probe never checks JS files.
            SyntaxKind::ObjectKeyword => Ok(self.tables.intrinsics.non_primitive),
            SyntaxKind::IntrinsicKeyword => Ok(self.tables.intrinsics.intrinsic_marker),
            SyntaxKind::LiteralType => self.get_type_from_literal_type_node(node),
            SyntaxKind::TypeReference => self.get_type_from_type_reference(node),
            SyntaxKind::TypePredicate => {
                let NodeData::TypePredicate(data) = self.data_of(node) else {
                    unreachable!("TypePredicate kind implies payload");
                };
                Ok(if data.asserts_modifier.is_some() {
                    self.tables.intrinsics.void
                } else {
                    self.tables.intrinsics.boolean
                })
            }
            SyntaxKind::ArrayType | SyntaxKind::TupleType => {
                self.get_type_from_array_or_tuple_type_node(node)
            }
            SyntaxKind::OptionalType => self.get_type_from_optional_type_node(node),
            SyntaxKind::UnionType => self.get_type_from_union_type_node(node),
            SyntaxKind::IntersectionType => self.get_type_from_intersection_type_node(node),
            SyntaxKind::NamedTupleMember => self.get_type_from_named_tuple_type_node(node),
            SyntaxKind::ParenthesizedType => {
                let NodeData::ParenthesizedType(data) = self.data_of(node) else {
                    unreachable!("ParenthesizedType kind implies payload");
                };
                let inner = data
                    .r#type
                    .ok_or_else(|| Unsupported::new("parenthesized type with missing operand"))?;
                self.get_type_from_type_node(inner)
            }
            SyntaxKind::RestType => self.get_type_from_rest_type_node(node),
            SyntaxKind::FunctionType | SyntaxKind::ConstructorType | SyntaxKind::TypeLiteral => {
                self.get_type_from_type_literal_or_fn_ctor_node(node)
            }
            SyntaxKind::TypeOperator => self.get_type_from_type_operator_node(node),
            SyntaxKind::TemplateLiteralType => self.get_type_from_template_type_node(node),
            SyntaxKind::ThisType | SyntaxKind::ThisKeyword => {
                self.get_type_from_this_type_node(node)
            }
            SyntaxKind::TypeQuery => self.get_type_from_type_query_node(node),
            SyntaxKind::IndexedAccessType => self.get_type_from_indexed_access_type_node(node),
            SyntaxKind::MappedType => Err(Unsupported::new(
                "mapped types (unported family, M4-end sweep 5.8)",
            )),
            SyntaxKind::ConditionalType => Err(Unsupported::new(
                "conditional types (unported family, M4-end sweep 5.8)",
            )),
            SyntaxKind::InferType => Err(Unsupported::new(
                "infer types (unported family, M4-end sweep 5.8)",
            )),
            SyntaxKind::ImportType => Err(Unsupported::new("import types (M4)")),
            // 63207: heritage ExpressionWithTypeArguments routes
            // through the same type-reference worker (getTypeReferenceName
            // reads the entity-name expression).
            SyntaxKind::ExpressionWithTypeArguments => self.get_type_from_type_reference(node),
            other => Err(Unsupported::new(format!(
                "type node kind {other:?} outside the M3 annotation slice"
            ))),
        }
    }

    // ---- literal types ----

    /// tsc-port: getTypeFromLiteralTypeNode @6.0.3
    /// tsc-hash: 32d41b6c0209245ea57edd770f01ade757ee22fd27e9c490c3db76c8af46d281
    /// tsc-span: _tsc.js:63102-63111
    fn get_type_from_literal_type_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let NodeData::LiteralType(data) = self.data_of(node) else {
            unreachable!("LiteralType kind implies payload");
        };
        let literal = data
            .literal
            .ok_or_else(|| Unsupported::new("literal type with missing literal"))?;
        if self.kind_of(literal) == SyntaxKind::NullKeyword {
            return Ok(self.tables.intrinsics.null);
        }
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let fresh = self.check_literal_expression(literal)?;
        let regular = self.tables.get_regular_type_of_literal_type(fresh);
        self.links.set_node_resolved_type(
            self.speculation_depth,
            node,
            LinkSlot::Resolved(regular),
        );
        Ok(regular)
    }

    /// The checkExpression slice getTypeFromLiteralTypeNode delegates
    /// to (63106): literal expressions produce FRESH literal types.
    /// Scoped to the literal kinds a LiteralTypeNode can hold; the full
    /// checkExpression is M4/M6.
    fn check_literal_expression(&mut self, literal: NodeId) -> CheckResult2<TypeId> {
        match self.data_of(literal).clone() {
            NodeData::StringLiteral(data) => {
                let regular = self.tables.get_string_literal_type(&data.text);
                Ok(self.tables.get_fresh_type_of_literal_type(regular))
            }
            NodeData::NoSubstitutionTemplateLiteral(data) => {
                let regular = self.tables.get_string_literal_type(&data.text);
                Ok(self.tables.get_fresh_type_of_literal_type(regular))
            }
            NodeData::NumericLiteral(data) => {
                let value = parse_numeric_literal_text(&data.text)?;
                let regular = self.tables.get_number_literal_type(value);
                Ok(self.tables.get_fresh_type_of_literal_type(regular))
            }
            NodeData::BigIntLiteral(data) => {
                let value = parse_pseudo_bigint_text(&data.text, /*negative*/ false)?;
                let regular = self.tables.get_bigint_literal_type(value);
                Ok(self.tables.get_fresh_type_of_literal_type(regular))
            }
            NodeData::PrefixUnaryExpression(data) => {
                if data.operator != SyntaxKind::MinusToken {
                    return Err(Unsupported::new(
                        "non-minus prefix operator in literal type",
                    ));
                }
                let operand = data
                    .operand
                    .ok_or_else(|| Unsupported::new("prefix literal with missing operand"))?;
                match self.data_of(operand).clone() {
                    NodeData::NumericLiteral(data) => {
                        let value = -parse_numeric_literal_text(&data.text)?;
                        let regular = self.tables.get_number_literal_type(value);
                        Ok(self.tables.get_fresh_type_of_literal_type(regular))
                    }
                    NodeData::BigIntLiteral(data) => {
                        let value = parse_pseudo_bigint_text(&data.text, /*negative*/ true)?;
                        let regular = self.tables.get_bigint_literal_type(value);
                        Ok(self.tables.get_fresh_type_of_literal_type(regular))
                    }
                    _ => Err(Unsupported::new("negated non-numeric literal type")),
                }
            }
            _ if self.kind_of(literal) == SyntaxKind::TrueKeyword => {
                Ok(self.tables.intrinsics.true_fresh)
            }
            _ if self.kind_of(literal) == SyntaxKind::FalseKeyword => {
                Ok(self.tables.intrinsics.false_fresh)
            }
            _ => Err(Unsupported::new(format!(
                "literal type literal kind {:?} outside the M3 slice",
                self.kind_of(literal)
            ))),
        }
    }

    // ---- unions / intersections ----

    /// tsc-port: getTypeFromUnionTypeNode @6.0.3
    /// tsc-hash: 66ed5227fe5516899d57e5edcff3c0978642f6dc68d83d883238cfb66c1b5c97
    /// tsc-span: _tsc.js:61642-61649
    ///
    /// Alias symbols are M4; UnionReduction::Literal replaces the
    /// interim constructor in stage 4.2. Routes through the checker
    /// twin (twin rule, unions.rs) so `"abc" | \`a${string}\``
    /// annotations run the template-literal reduction like tsc.
    fn get_type_from_union_type_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let NodeData::UnionType(data) = self.data_of(node) else {
            unreachable!("UnionType kind implies payload");
        };
        let alias_symbol = self.get_alias_symbol_for_type_node(node);
        let elements = self.nodes_of(data.types);
        let mut types = Vec::with_capacity(elements.len());
        for element in elements {
            types.push(self.get_type_from_type_node(element)?);
        }
        let alias_type_arguments = self.get_type_arguments_for_alias_symbol(alias_symbol);
        let union = self.get_union_type_ex_with_origin(
            &types,
            UnionReduction::Literal,
            alias_symbol,
            alias_type_arguments.as_deref(),
            None,
        )?;
        self.links
            .set_node_resolved_type(self.speculation_depth, node, LinkSlot::Resolved(union));
        Ok(union)
    }

    /// tsc-port: getTypeFromIntersectionTypeNode @6.0.3
    /// tsc-hash: 3253ede7a3b7ff3f66b870ca76d327633426b9b6b5e6ca1b4b7747499cf6c744
    /// tsc-span: _tsc.js:61909-61920
    ///
    /// Alias symbols are M4. The noSupertypeReduction flag fires for a
    /// 2-member `{} & T` where T is string/number/bigint-flavored or a
    /// pattern template literal (the NonNullable-style trick).
    fn get_type_from_intersection_type_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let NodeData::IntersectionType(data) = self.data_of(node) else {
            unreachable!("IntersectionType kind implies payload");
        };
        let alias_symbol = self.get_alias_symbol_for_type_node(node);
        let elements = self.nodes_of(data.types);
        let mut types = Vec::with_capacity(elements.len());
        for element in elements {
            types.push(self.get_type_from_type_node(element)?);
        }
        let empty_index = if types.len() == 2 {
            types
                .iter()
                .position(|&t| t == self.empty_type_literal_type)
        } else {
            None
        };
        let t = match empty_index {
            Some(index) => types[1 - index],
            None => self.tables.intrinsics.unknown,
        };
        let no_supertype_reduction = self.tables.flags_of(t).intersects(TypeFlags::from_bits(
            TypeFlags::STRING.bits() | TypeFlags::NUMBER.bits() | TypeFlags::BIG_INT.bits(),
        )) || (self
            .tables
            .flags_of(t)
            .intersects(TypeFlags::TEMPLATE_LITERAL)
            && self.tables.is_pattern_literal_type(t));
        let alias_type_arguments = self.get_type_arguments_for_alias_symbol(alias_symbol);
        let intersection = self.get_intersection_type_ex(
            &types,
            if no_supertype_reduction {
                IntersectionFlags::NO_SUPERTYPE_REDUCTION
            } else {
                IntersectionFlags::NONE
            },
            alias_symbol,
            alias_type_arguments.as_deref(),
        )?;
        self.links.set_node_resolved_type(
            self.speculation_depth,
            node,
            LinkSlot::Resolved(intersection),
        );
        Ok(intersection)
    }

    // ---- template literal types ----

    /// tsc-port: getTypeFromTemplateTypeNode @6.0.3
    /// tsc-hash: 1a50ce55b0562f5f6b0a82d1b28d1943f8601f6172b9db8c00ba3634922d87b7
    /// tsc-span: _tsc.js:62047-62056
    fn get_type_from_template_type_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let NodeData::TemplateLiteralType(data) = self.data_of(node) else {
            unreachable!("TemplateLiteralType kind implies payload");
        };
        let head = data
            .head
            .ok_or_else(|| Unsupported::new("template literal type with missing head"))?;
        let spans = self.nodes_of(data.template_spans);
        let NodeData::TemplateHead(head_data) = self.data_of(head) else {
            return Err(Unsupported::new("template literal head payload"));
        };
        let mut texts = vec![head_data.text.clone()];
        let mut types = Vec::with_capacity(spans.len());
        for span in spans {
            let NodeData::TemplateLiteralTypeSpan(span_data) = self.data_of(span).clone() else {
                return Err(Unsupported::new("template literal span payload"));
            };
            let span_type = span_data
                .r#type
                .ok_or_else(|| Unsupported::new("template span with missing type"))?;
            let literal = span_data
                .literal
                .ok_or_else(|| Unsupported::new("template span with missing literal"))?;
            let text = match self.data_of(literal) {
                NodeData::TemplateMiddle(data) => data.text.clone(),
                NodeData::TemplateTail(data) => data.text.clone(),
                _ => return Err(Unsupported::new("template span literal payload")),
            };
            types.push(self.get_type_from_type_node(span_type)?);
            texts.push(text);
        }
        let template = self.tables.get_template_literal_type(&texts, &types);
        self.links.set_node_resolved_type(
            self.speculation_depth,
            node,
            LinkSlot::Resolved(template),
        );
        Ok(template)
    }

    // ---- tuples ----

    /// tsc-port: getTypeFromArrayOrTupleTypeNode @6.0.3
    /// tsc-hash: fbfa13c985f4372427e82b3bcb4fbdcc8bba690945422a60571d8ca75d8e5301
    /// tsc-span: _tsc.js:61118-61137
    fn get_type_from_array_or_tuple_type_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let target = self.get_array_or_tuple_target_type(node)?;
        let is_tuple = self.kind_of(node) == SyntaxKind::TupleType;
        let elements = match self.data_of(node) {
            NodeData::TupleType(data) => self.nodes_of(data.elements),
            NodeData::ArrayType(_) => Vec::new(),
            _ => unreachable!("array/tuple kind implies payload"),
        };
        let resolved = if target == self.empty_generic_type {
            // A failed global Array/ReadonlyArray lookup (61122).
            self.empty_object_type
        } else {
            let has_variadic_element = is_tuple
                && elements.iter().any(|&element| {
                    self.get_tuple_element_flags(element)
                        .intersects(ElementFlags::VARIADIC)
                });
            if !has_variadic_element && self.is_deferred_type_reference_node(node, false)? {
                if is_tuple && elements.is_empty() {
                    target
                } else {
                    self.create_deferred_type_reference(target, node, None, None, None)?
                }
            } else {
                let element_types = if is_tuple {
                    let mut element_types = Vec::with_capacity(elements.len());
                    for element in &elements {
                        element_types.push(self.get_type_from_type_node(*element)?);
                    }
                    element_types
                } else {
                    let NodeData::ArrayType(data) = self.data_of(node) else {
                        unreachable!("non-tuple node here is an array");
                    };
                    let element = data
                        .element_type
                        .ok_or_else(|| Unsupported::new("array type with missing element type"))?;
                    vec![self.get_type_from_type_node(element)?]
                };
                self.create_normalized_type_reference_forced(target, &element_types)?
            }
        };
        self.links.set_node_resolved_type(
            self.speculation_depth,
            node,
            LinkSlot::Resolved(resolved),
        );
        Ok(resolved)
    }

    /// createNormalizedTypeReference forces variadic tuple elements'
    /// type arguments lazily THROUGH getTypeArguments (61240 inside
    /// createNormalizedTupleType); tables cannot reach the checker, so
    /// checker callers pre-force here — element order matches tsc's
    /// left-to-right expansion, and the forced resolution has no other
    /// observable effects.
    /// tsc-port: createNormalizedTypeReference @6.0.3
    /// tsc-hash: 32b91334e6762e8ea63ac6a9be5f6689a4d112aa1db2d59986c736ac6735e143
    /// tsc-span: _tsc.js:61210-61212
    ///
    /// The checker twin: tuple targets run the FULL
    /// createNormalizedTupleType below (the tables twin keeps its
    /// M4Dependency escapes for tables-internal callers only).
    pub(crate) fn create_normalized_type_reference_forced(
        &mut self,
        target: TypeId,
        type_arguments: &[TypeId],
    ) -> CheckResult2<TypeId> {
        if self
            .tables
            .object_flags_of(target)
            .intersects(ObjectFlags::TUPLE)
        {
            self.create_normalized_tuple_type_full(target, type_arguments)
        } else {
            Ok(self.tables.create_type_reference(target, type_arguments))
        }
    }

    /// tsc indexes target.elementFlags past the element list for the
    /// tuple-this append (getTypeWithThisArgument): `undefined`
    /// coerces to 0 under JS bitwise reads, i.e. zero element flags.
    fn element_flag_at(flags: &[ElementFlags], index: usize) -> ElementFlags {
        flags
            .get(index)
            .copied()
            .unwrap_or(ElementFlags::from_bits(0))
    }

    /// tsc-port: createNormalizedTupleType @6.0.3
    /// tsc-hash: 5b7968f648c63d88544746d841015ff7800b723dbc071b96fb4d6f7ae0b18154
    /// tsc-span: _tsc.js:61213-61287
    ///
    /// Checker twin of the tables port — the three former M4Dependency
    /// arms run live here: union/never variadic distribution (mapType
    /// with checkCrossProductUnion 2590 at currentNode), array-like
    /// variadic collapse (getIndexTypeOfType), and the variadic-in-
    /// rest-window collapse (getIndexedAccessType); the 10k-element
    /// guard reports 2799/2800 at currentNode. mapType here is the
    /// no-origin form (union constituents are flattened at creation,
    /// so the origin-walk arm has nothing extra to see).
    fn create_normalized_tuple_type_full(
        &mut self,
        target: TypeId,
        element_types: &[TypeId],
    ) -> CheckResult2<TypeId> {
        let TypeData::TupleTarget(data) = self.tables.type_of(target).data.clone() else {
            unreachable!("createNormalizedTupleType requires a tuple target");
        };
        if !data.combined_flags.intersects(ElementFlags::NON_REQUIRED) {
            // No non-required elements: plain reference (61215-61217).
            return Ok(self.tables.create_type_reference(target, element_types));
        }
        if data.combined_flags.intersects(ElementFlags::VARIADIC) {
            // Union/never variadic distribution (61218-61223).
            let union_index = (0..element_types.len()).find(|&i| {
                Self::element_flag_at(&data.element_flags, i).intersects(ElementFlags::VARIADIC)
                    && self
                        .tables
                        .flags_of(element_types[i])
                        .intersects(TypeFlags::from_bits(
                            TypeFlags::NEVER.bits() | TypeFlags::UNION.bits(),
                        ))
            });
            if let Some(union_index) = union_index {
                let cross: Vec<TypeId> = element_types
                    .iter()
                    .enumerate()
                    .map(|(i, &t)| {
                        if Self::element_flag_at(&data.element_flags, i)
                            .intersects(ElementFlags::VARIADIC)
                        {
                            t
                        } else {
                            self.tables.intrinsics.unknown
                        }
                    })
                    .collect();
                if !self.check_cross_product_union(&cross) {
                    return Ok(self.tables.intrinsics.error);
                }
                let outer = element_types.to_vec();
                return self.map_type_result(outer[union_index], move |state, t| {
                    let mut replaced = outer.clone();
                    replaced[union_index] = t;
                    state.create_normalized_tuple_type_full(target, &replaced)
                });
            }
        }
        let mut expanded_types: Vec<TypeId> = Vec::new();
        let mut expanded_flags: Vec<ElementFlags> = Vec::new();
        let mut expanded_declarations: Vec<Option<u32>> = Vec::new();
        let outer_labels = data.labeled_element_declarations.clone();
        let outer_declaration = move |i: usize| -> Option<u32> {
            outer_labels
                .as_ref()
                .and_then(|declarations| declarations.get(i).copied())
                .flatten()
        };
        let mut last_required_index: isize = -1;
        let mut first_rest_index: isize = -1;
        let mut last_optional_or_rest_index: isize = -1;
        {
            let mut add_element = |state: &mut Self,
                                   expanded_types: &mut Vec<TypeId>,
                                   expanded_flags: &mut Vec<ElementFlags>,
                                   expanded_declarations: &mut Vec<Option<u32>>,
                                   ty: TypeId,
                                   flags: ElementFlags,
                                   declaration: Option<u32>| {
                if flags.intersects(ElementFlags::REQUIRED) {
                    last_required_index = expanded_flags.len() as isize;
                }
                if flags.intersects(ElementFlags::REST) && first_rest_index < 0 {
                    first_rest_index = expanded_flags.len() as isize;
                }
                if flags.intersects(ElementFlags::from_bits(
                    ElementFlags::OPTIONAL.bits() | ElementFlags::REST.bits(),
                )) {
                    last_optional_or_rest_index = expanded_flags.len() as isize;
                }
                let pushed = if flags.intersects(ElementFlags::OPTIONAL) {
                    state.tables.add_optionality(ty, /*is_property*/ true, true)
                } else {
                    ty
                };
                expanded_types.push(pushed);
                expanded_flags.push(flags);
                expanded_declarations.push(declaration);
            };

            for (i, &element_type) in element_types.iter().enumerate() {
                let flags = Self::element_flag_at(&data.element_flags, i);
                if flags.intersects(ElementFlags::VARIADIC) {
                    if self
                        .tables
                        .flags_of(element_type)
                        .intersects(TypeFlags::ANY)
                    {
                        add_element(
                            self,
                            &mut expanded_types,
                            &mut expanded_flags,
                            &mut expanded_declarations,
                            element_type,
                            ElementFlags::REST,
                            outer_declaration(i),
                        );
                    } else if self
                        .tables
                        .flags_of(element_type)
                        .intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE)
                        || self.is_generic_mapped_type_state(element_type)
                    {
                        add_element(
                            self,
                            &mut expanded_types,
                            &mut expanded_flags,
                            &mut expanded_declarations,
                            element_type,
                            ElementFlags::VARIADIC,
                            outer_declaration(i),
                        );
                    } else if self.tables.is_tuple_type(element_type) {
                        let inner_args = self.get_type_arguments(element_type)?;
                        if inner_args.len() + expanded_types.len() >= 10_000 {
                            // 61240-61246: 2799 in type positions,
                            // 2800 in expression positions.
                            let message = if self
                                .current_node
                                .is_some_and(|node| self.is_part_of_type_node(node))
                            {
                                &diagnostics::Type_produces_a_tuple_type_that_is_too_large_to_represent
                            } else {
                                &diagnostics::Expression_produces_a_tuple_type_that_is_too_large_to_represent
                            };
                            self.error_at(self.current_node, message, &[]);
                            return Ok(self.tables.intrinsics.error);
                        }
                        let inner_target = self.tables.reference_target(element_type);
                        let TypeData::TupleTarget(inner) =
                            self.tables.type_of(inner_target).data.clone()
                        else {
                            unreachable!("tuple type targets a tuple target");
                        };
                        for (n, &inner_type) in inner_args.iter().enumerate() {
                            let inner_declaration = inner
                                .labeled_element_declarations
                                .as_ref()
                                .and_then(|declarations| declarations.get(n).copied())
                                .flatten();
                            add_element(
                                self,
                                &mut expanded_types,
                                &mut expanded_flags,
                                &mut expanded_declarations,
                                inner_type,
                                inner.element_flags[n],
                                inner_declaration,
                            );
                        }
                    } else {
                        // 61252: `isArrayLikeType(type) &&
                        // getIndexTypeOfType(type, numberType) ||
                        // errorType` as a Rest element.
                        let index_type = if self.is_array_like_type(element_type)? {
                            self.get_index_type_of_type(
                                element_type,
                                self.tables.intrinsics.number,
                            )?
                        } else {
                            None
                        };
                        add_element(
                            self,
                            &mut expanded_types,
                            &mut expanded_flags,
                            &mut expanded_declarations,
                            index_type.unwrap_or(self.tables.intrinsics.error),
                            ElementFlags::REST,
                            outer_declaration(i),
                        );
                    }
                } else {
                    add_element(
                        self,
                        &mut expanded_types,
                        &mut expanded_flags,
                        &mut expanded_declarations,
                        element_type,
                        flags,
                        outer_declaration(i),
                    );
                }
            }
        }
        // Optional elements before the last required one become
        // required (61258-61260).
        for flags in expanded_flags
            .iter_mut()
            .take(last_required_index.max(0) as usize)
        {
            if flags.intersects(ElementFlags::OPTIONAL) {
                *flags = ElementFlags::REQUIRED;
            }
        }
        // Collapse everything from the first rest element through the
        // last optional/rest element into a single rest union
        // (61261-61266); variadic window members read their number
        // index via getIndexedAccessType.
        if first_rest_index >= 0 && first_rest_index < last_optional_or_rest_index {
            let first = first_rest_index as usize;
            let last = last_optional_or_rest_index as usize;
            let window: Vec<TypeId> = expanded_types[first..=last].to_vec();
            let mut mapped = Vec::with_capacity(window.len());
            for (offset, &t) in window.iter().enumerate() {
                let member = if expanded_flags[first + offset].intersects(ElementFlags::VARIADIC) {
                    self.get_indexed_access_type(
                        t,
                        self.tables.intrinsics.number,
                        tsrs2_types::AccessFlags::NONE,
                        None,
                        None,
                        None,
                    )?
                } else {
                    t
                };
                mapped.push(member);
            }
            expanded_types[first] = self.get_union_type_ex(&mapped, UnionReduction::Literal)?;
            expanded_types.drain(first + 1..=last);
            expanded_flags.drain(first + 1..=last);
            expanded_declarations.drain(first + 1..=last);
        }
        // getTupleTargetType's single-rest collapse (61146-61148) needs
        // the checker-owned global array targets, so it lives here like
        // create_tuple_type_forced's copy.
        let tuple_target = if expanded_flags.len() == 1
            && expanded_flags[0].intersects(ElementFlags::REST)
        {
            if data.readonly {
                self.global_readonly_array_type()?
            } else {
                self.global_array_type()?
            }
        } else {
            self.tables
                .get_tuple_target_type(&expanded_flags, data.readonly, Some(&expanded_declarations))
                .map_err(Self::unsupported_m4)?
        };
        Ok(if tuple_target == self.empty_generic_type {
            self.empty_object_type
        } else if !expanded_flags.is_empty() {
            self.tables
                .create_type_reference(tuple_target, &expanded_types)
        } else {
            tuple_target
        })
    }

    /// tsc-port: getArrayOrTupleTargetType @6.0.3
    /// tsc-hash: 4cf2f8c3a8e8ac36305166ae9a3424a26f2d685e453bd521e3f32be9bf76892e
    /// tsc-span: _tsc.js:61056-61064
    ///
    /// The single-rest tuple `[...T[]]` reaches the Array target
    /// through getArrayElementTypeNode's unwrap — the tables
    /// get_tuple_target_type collapse escape stays for synthesized
    /// createTupleType callers only.
    fn get_array_or_tuple_target_type(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let readonly = self
            .parent_of(node)
            .is_some_and(|parent| self.is_readonly_type_operator(parent));
        if self.get_array_element_type_node(node).is_some() {
            return if readonly {
                self.global_readonly_array_type()
            } else {
                self.global_array_type()
            };
        }
        let NodeData::TupleType(data) = self.data_of(node) else {
            unreachable!("non-array node here is a tuple");
        };
        let elements = self.nodes_of(data.elements);
        let element_flags: Vec<ElementFlags> = elements
            .iter()
            .map(|&element| self.get_tuple_element_flags(element))
            .collect();
        // 61063: named member declarations — per-element
        // NamedTupleMember nodes (raw ids; the tables key + target data
        // are NodeId-free).
        let named: Vec<Option<u32>> = elements
            .iter()
            .map(|&element| self.is_named_tuple_member(element).then_some(element.0))
            .collect();
        self.tables
            .get_tuple_target_type(&element_flags, readonly, Some(&named))
            .map_err(Self::unsupported_m4)
    }

    fn is_named_tuple_member(&self, node: NodeId) -> bool {
        self.kind_of(node) == SyntaxKind::NamedTupleMember
    }

    /// tsc isReadonlyTypeOperator (61138-61140).
    fn is_readonly_type_operator(&self, node: NodeId) -> bool {
        matches!(
            self.data_of(node),
            NodeData::TypeOperator(data) if data.operator == SyntaxKind::ReadonlyKeyword
        )
    }

    /// tsc-port: getTupleElementFlags @6.0.3
    /// tsc-hash: 20a58237c33ac7f48b75470a5b7ff6badfc7c8190624917b6bb38a95fad11224
    /// tsc-span: _tsc.js:61041-61052
    fn get_tuple_element_flags(&self, node: NodeId) -> ElementFlags {
        match self.data_of(node) {
            NodeData::OptionalType(_) => ElementFlags::OPTIONAL,
            NodeData::RestType(data) => self.get_rest_type_element_flags(data.r#type),
            NodeData::NamedTupleMember(data) => {
                if data.question_token.is_some() {
                    ElementFlags::OPTIONAL
                } else if data.dot_dot_dot_token.is_some() {
                    self.get_rest_type_element_flags(data.r#type)
                } else {
                    ElementFlags::REQUIRED
                }
            }
            _ => ElementFlags::REQUIRED,
        }
    }

    /// tsc-port: getRestTypeElementFlags @6.0.3
    /// tsc-hash: adc5eb6dd9ecf2f36b7c529c4a742b8c286fec672e9163db8353034673949181
    /// tsc-span: _tsc.js:61053-61055
    fn get_rest_type_element_flags(&self, inner: Option<NodeId>) -> ElementFlags {
        match inner {
            Some(inner) if self.get_array_element_type_node(inner).is_some() => ElementFlags::REST,
            _ => ElementFlags::VARIADIC,
        }
    }

    /// tsc-port: getArrayElementTypeNode @6.0.3
    /// tsc-hash: e7d0bc18a05ab539c84dab523668931ea9d6c9d40fa454b9029ba7e36ab20d7a
    /// tsc-span: _tsc.js:63170-63186
    fn get_array_element_type_node(&self, node: NodeId) -> Option<NodeId> {
        match self.data_of(node) {
            NodeData::ParenthesizedType(data) => data
                .r#type
                .and_then(|inner| self.get_array_element_type_node(inner)),
            NodeData::TupleType(data) => {
                let elements = self.nodes_of(data.elements);
                if elements.len() == 1 {
                    let element = elements[0];
                    match self.data_of(element) {
                        NodeData::RestType(data) => {
                            return data
                                .r#type
                                .and_then(|inner| self.get_array_element_type_node(inner));
                        }
                        NodeData::NamedTupleMember(data) if data.dot_dot_dot_token.is_some() => {
                            return data
                                .r#type
                                .and_then(|inner| self.get_array_element_type_node(inner));
                        }
                        _ => {}
                    }
                }
                None
            }
            NodeData::ArrayType(data) => data.element_type,
            _ => None,
        }
    }

    /// tsc-port: getTypeFromOptionalTypeNode @6.0.3
    /// tsc-hash: 59ecf05dbac008065e30a599ef231d99430997ea5f02e0e8fa22403f36c272b8
    /// tsc-span: _tsc.js:61317-61323
    fn get_type_from_optional_type_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let NodeData::OptionalType(data) = self.data_of(node) else {
            unreachable!("OptionalType kind implies payload");
        };
        let inner = data
            .r#type
            .ok_or_else(|| Unsupported::new("optional type with missing operand"))?;
        let inner_type = self.get_type_from_type_node(inner)?;
        Ok(self
            .tables
            .add_optionality(inner_type, /*is_property*/ true, true))
    }

    /// tsc-port: getTypeFromRestTypeNode @6.0.3
    /// tsc-hash: ac5c2df0a5261dbe26796cdc3b31ab0eb6b4958511857d46f68fe049197608ba
    /// tsc-span: _tsc.js:63167-63169
    fn get_type_from_rest_type_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let NodeData::RestType(data) = self.data_of(node) else {
            unreachable!("RestType kind implies payload");
        };
        let inner = data
            .r#type
            .ok_or_else(|| Unsupported::new("rest type with missing operand"))?;
        let unwrapped = self.get_array_element_type_node(inner).unwrap_or(inner);
        self.get_type_from_type_node(unwrapped)
    }

    /// tsc-port: getTypeFromNamedTupleTypeNode @6.0.3
    /// tsc-hash: 94643dc28cefdc9e689c4db019c77b39683a0c1d4379adfb81b8745b243bd474
    /// tsc-span: _tsc.js:63187-63195
    fn get_type_from_named_tuple_type_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let NodeData::NamedTupleMember(data) = self.data_of(node).clone() else {
            unreachable!("NamedTupleMember kind implies payload");
        };
        let inner = data
            .r#type
            .ok_or_else(|| Unsupported::new("named tuple member with missing type"))?;
        let resolved = if data.dot_dot_dot_token.is_some() {
            let unwrapped = self.get_array_element_type_node(inner).unwrap_or(inner);
            self.get_type_from_type_node(unwrapped)?
        } else {
            let inner_type = self.get_type_from_type_node(inner)?;
            self.tables.add_optionality(
                inner_type,
                /*is_property*/ true,
                data.question_token.is_some(),
            )
        };
        self.links.set_node_resolved_type(
            self.speculation_depth,
            node,
            LinkSlot::Resolved(resolved),
        );
        Ok(resolved)
    }

    /// tsc-port: getTypeFromTypeOperatorNode @6.0.3
    /// tsc-hash: 7bad7b2a41d1aa311c75b425dfb27261472876c904bca19627c209e416008579
    /// tsc-span: _tsc.js:62028-62046
    fn get_type_from_type_operator_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let NodeData::TypeOperator(data) = self.data_of(node) else {
            unreachable!("TypeOperator kind implies payload");
        };
        let operator = data.operator;
        let inner = data
            .r#type
            .ok_or_else(|| Unsupported::new("type operator with missing operand"))?;
        match operator {
            // The readonly-ness itself was consumed by
            // getArrayOrTupleTargetType through the parent check.
            SyntaxKind::ReadonlyKeyword => self.get_type_from_type_node(inner),
            SyntaxKind::KeyOfKeyword => {
                let operand = self.get_type_from_type_node(inner)?;
                self.get_index_type(operand, tsrs2_types::IndexFlags::NONE)
            }
            SyntaxKind::UniqueKeyword => {
                // 62035-62037: `unique symbol` resolves through the
                // OWNING declaration (walkUpParenthesizedTypes on the
                // operator's parent); non-`symbol` operands answer
                // errorType.
                if self.kind_of(inner) != SyntaxKind::SymbolKeyword {
                    return Ok(self.tables.intrinsics.error);
                }
                let mut parent = self.parent_of(node).ok_or_else(|| {
                    Unsupported::new("type operator without a parent (parse recovery)")
                })?;
                while self.kind_of(parent) == SyntaxKind::ParenthesizedType {
                    match self.parent_of(parent) {
                        Some(next) => parent = next,
                        None => break,
                    }
                }
                self.get_es_symbol_like_type_for_node(parent)
            }
            other => Err(Unsupported::new(format!(
                "type operator {other:?} outside the M3 slice"
            ))),
        }
    }

    /// tsc-port: getESSymbolLikeTypeForNode @6.0.3
    /// tsc-hash: cfffbfaec274ec3d0403dfece197ea736c208fd8698405ab6a3696e5f41d915b
    /// tsc-span: _tsc.js:63117-63132
    ///
    /// The JSDoc host hop and isCommonJsExportPropertyAssignment are
    /// JS-only (elided). getSymbolOfNode tolerates unbound nodes — an
    /// invalid position falls through to the plain `symbol` intrinsic
    /// (the 1332-family grammar rows are parser-emitted).
    pub(crate) fn get_es_symbol_like_type_for_node(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<TypeId> {
        if self.is_valid_es_symbol_declaration(node) {
            let symbol = self.node_symbol(node).map(|s| self.get_merged_symbol(s));
            if let Some(symbol) = symbol {
                if let Some(cached) = self.links.symbol(symbol).unique_es_symbol_type {
                    return Ok(cached);
                }
                let escaped_name = format!(
                    "__@{}@{}",
                    self.binder.symbol(symbol).escaped_name,
                    symbol.0
                );
                let ty = self
                    .tables
                    .create_unique_es_symbol_type(symbol, escaped_name);
                self.links
                    .set_symbol_unique_es_symbol_type(self.speculation_depth, symbol, ty);
                return Ok(ty);
            }
        }
        Ok(self.tables.intrinsics.es_symbol)
    }

    /// tsc-port: isValidESSymbolDeclaration @6.0.3
    /// tsc-hash: 667a26eb7c294b84d739b1e9b57d758772ff062767d05c4cfabd873d99eac28c
    /// tsc-span: _tsc.js:14377-14379
    ///
    /// isCommonJsExportPropertyAssignment is JS-only (constant false).
    fn is_valid_es_symbol_declaration(&self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        match self.data_of(node) {
            NodeData::VariableDeclaration(data) => {
                // isVarConst: (combined & BlockScoped) == Const.
                let combined = node_util::get_combined_node_flags(source, node);
                let block_scoped = tsrs2_types::NodeFlags::from_bits(
                    tsrs2_types::NodeFlags::LET.bits()
                        | tsrs2_types::NodeFlags::CONST.bits()
                        | tsrs2_types::NodeFlags::USING.bits(),
                );
                let is_const =
                    combined.bits() & block_scoped.bits() == tsrs2_types::NodeFlags::CONST.bits();
                is_const
                    && data
                        .name
                        .is_some_and(|name| self.kind_of(name) == SyntaxKind::Identifier)
                    && self.is_variable_declaration_in_variable_statement(node)
            }
            NodeData::PropertyDeclaration(_) => {
                node_util::has_syntactic_modifier(
                    source,
                    node,
                    tsrs2_types::ModifierFlags::READONLY,
                ) && self.has_static_modifier(node)
            }
            NodeData::PropertySignature(_) => node_util::has_syntactic_modifier(
                source,
                node,
                tsrs2_types::ModifierFlags::READONLY,
            ),
            _ => false,
        }
    }

    /// tsc isVariableDeclarationInVariableStatement (14384):
    /// declaration → VariableDeclarationList → VariableStatement.
    fn is_variable_declaration_in_variable_statement(&self, node: NodeId) -> bool {
        let Some(list) = self.parent_of(node) else {
            return false;
        };
        self.kind_of(list) == SyntaxKind::VariableDeclarationList
            && self
                .parent_of(list)
                .is_some_and(|statement| self.kind_of(statement) == SyntaxKind::VariableStatement)
    }

    // ---- type literals / function / constructor types ----

    /// tsc-port: getTypeFromTypeLiteralOrFunctionOrConstructorTypeNode @6.0.3
    /// tsc-hash: fd62d5bd39d73cc252a89075d1572e1a4d7d8c684e4f31313844ae52995a337f
    /// tsc-span: _tsc.js:62890-62907
    ///
    /// Alias symbols (getAliasSymbolForTypeNode) are M4; the JSDoc
    /// array-type wrap is JS-only.
    fn get_type_from_type_literal_or_fn_ctor_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let symbol = self.node_symbol(node);
        let alias_symbol = self.get_alias_symbol_for_type_node(node);
        let members_empty = match symbol {
            // 62894: getMembersOfSymbol — the RESOLVED (late-bound)
            // table decides emptiness; a type literal whose only
            // member is computed-named must NOT collapse to
            // emptyTypeLiteralType (its early table is empty — the
            // binder parks dynamic names off-table).
            Some(symbol) => self.get_members_of_symbol(symbol)?.is_empty(),
            None => true,
        };
        let resolved = match symbol {
            None => self.empty_type_literal_type,
            Some(_) if members_empty && alias_symbol.is_none() => self.empty_type_literal_type,
            Some(symbol) => {
                let id = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
                let alias_type_arguments = self.get_type_arguments_for_alias_symbol(alias_symbol);
                let ty = self.tables.type_mut(id);
                ty.object_flags = ObjectFlags::ANONYMOUS;
                ty.symbol = Some(symbol);
                ty.alias_symbol = alias_symbol;
                ty.alias_type_arguments = alias_type_arguments.map(Vec::into_boxed_slice);
                id
            }
        };
        self.links.set_node_resolved_type(
            self.speculation_depth,
            node,
            LinkSlot::Resolved(resolved),
        );
        Ok(resolved)
    }

    // ---- interface references ----

    /// tsc-port: getTypeFromTypeReference @6.0.3
    /// tsc-hash: d850bffcaf58ba26258dd2c696ae5f925b00c2314d7d08f0ac4c33f9a22d753a
    /// tsc-span: _tsc.js:60557-60592
    ///
    /// Combined with the slices of resolveTypeReferenceName
    /// (60372-60379, the real resolveEntityName from M4 5.1a) and
    /// getTypeReferenceType (60380-60405). Generic ALIAS references
    /// ride on getTypeAliasInstantiation (next commit); enums are 5.3b;
    /// class references wait for class members (5.3). An unresolved
    /// name is tsc's unknownSymbol → errorType; the probe keeps the
    /// Unsupported channel until the 5.4 driver makes errorType
    /// observable through diagnostics. The links.resolvedSymbol write
    /// (60587) lands with the 5.4 driver (checkTypeReferenceOrImport
    /// reads it for type-argument constraint checking).
    pub(crate) fn get_type_from_type_reference(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        // getTypeReferenceName (60364-60371): the entity name for
        // TypeReference nodes, the (entity-name) expression for heritage
        // ExpressionWithTypeArguments.
        let type_name = match self.data_of(node) {
            NodeData::TypeReference(data) => data.type_name,
            NodeData::ExpressionWithTypeArguments(data) => data
                .expression
                .filter(|&expression| self.is_entity_name_expression(expression)),
            _ => unreachable!("type reference kinds imply payloads"),
        };
        let type_name =
            type_name.ok_or_else(|| Unsupported::new("type reference with missing name"))?;
        // resolveEntityName reports (2304 family) and yields
        // unknownSymbol; the reference then types as errorType.
        let Some(symbol) = self.resolve_entity_name(
            type_name,
            SymbolFlags::TYPE,
            /*ignore_errors*/ false,
            None,
        ) else {
            return Ok(self.tables.intrinsics.error);
        };
        let flags = self.symbol_flags(symbol);
        let resolved = if flags.intersects(SymbolFlags::TYPE_PARAMETER) {
            // getTypeReferenceType's tryGetDeclaredTypeOfSymbol arm
            // (60400-60403): a type-argument list on a non-generic
            // reference is the 2315 family via checkNoTypeArguments.
            let declared = self.get_declared_type_of_type_parameter(symbol);
            if !self.check_no_type_arguments(node, Some(symbol)) {
                self.tables.intrinsics.error
            } else {
                self.tables.get_regular_type_of_literal_type(declared)
            }
        } else if flags.intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE) {
            self.get_type_from_class_or_interface_reference(node, symbol)?
        } else if flags.intersects(SymbolFlags::TYPE_ALIAS) {
            self.get_type_from_type_alias_reference(node, symbol)?
        } else if flags.intersects(SymbolFlags::REGULAR_ENUM | SymbolFlags::CONST_ENUM) {
            // tryGetDeclaredTypeOfSymbol arm (60391-60394): enums flow
            // through the same checkNoTypeArguments +
            // getRegularTypeOfLiteralType tail as type parameters.
            let declared = self.get_declared_type_of_enum(symbol)?;
            if !self.check_no_type_arguments(node, Some(symbol)) {
                self.tables.intrinsics.error
            } else {
                self.tables.get_regular_type_of_literal_type(declared)
            }
        } else if flags.intersects(SymbolFlags::ENUM_MEMBER) {
            let declared = self.get_declared_type_of_enum_member(symbol)?;
            if !self.check_no_type_arguments(node, Some(symbol)) {
                self.tables.intrinsics.error
            } else {
                self.tables.get_regular_type_of_literal_type(declared)
            }
        } else {
            return Err(Unsupported::new(format!(
                "type reference to symbol flags {flags:?} (M4)"
            )));
        };
        // links.resolvedSymbol + links.resolvedType (60587-60588):
        // written together, and deliberately OVERWRITE-capable — the
        // type-parameter-default recursion can complete an inner
        // computation of this same node first (see
        // overwrite_type_reference_resolution).
        self.links.overwrite_type_reference_resolution(
            self.speculation_depth,
            node,
            symbol,
            resolved,
        );
        Ok(resolved)
    }

    /// tsc-port: getTypeFromThisTypeNode @6.0.3
    /// tsc-hash: 5f298805f1bf4351822f0b77399acba5c31dff7ee616d8f1efed35ea03d4c9da
    /// tsc-span: _tsc.js:63160-63166
    fn get_type_from_this_type_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let resolved = self.get_this_type(node)?;
        self.links.set_node_resolved_type(
            self.speculation_depth,
            node,
            LinkSlot::Resolved(resolved),
        );
        Ok(resolved)
    }

    /// tsc-port: getThisType @6.0.3
    /// tsc-hash: aa0c087eb26daf6a716a057c5006108dd99b1f00b86becb8add0490dab33d15d
    /// tsc-span: _tsc.js:63133-63159
    ///
    /// The JS prototype-assignment and JSDoc host arms are elided
    /// project-wide; isJSConstructor is constant-false in TS files.
    fn get_this_type(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let source = self.binder.source_of_node(node);
        let container = tsrs2_binder::node_util::get_this_container(
            source, node, /*include_arrow_functions*/ false,
        );
        let parent = container.and_then(|container| self.parent_of(container));
        if let (Some(container), Some(parent)) = (container, parent) {
            let parent_kind = self.kind_of(parent);
            if matches!(
                parent_kind,
                SyntaxKind::ClassDeclaration
                    | SyntaxKind::ClassExpression
                    | SyntaxKind::InterfaceDeclaration
            ) {
                let is_constructor = self.kind_of(container) == SyntaxKind::Constructor;
                let descendant_of_body = is_constructor && {
                    let body = match self.data_of(container) {
                        NodeData::Constructor(data) => data.body,
                        _ => None,
                    };
                    body.is_some_and(|body| self.is_node_descendant_of(node, body))
                };
                if !self.has_static_modifier(container) && (!is_constructor || descendant_of_body) {
                    let symbol = self
                        .node_symbol(parent)
                        .expect("class/interface declarations bind symbols");
                    // getSymbolOfDeclaration (getThisType's parent read).
                    let symbol = self.get_merged_symbol(symbol);
                    let declared = self.get_declared_type_of_class_or_interface(symbol)?;
                    return match &self.tables.type_of(declared).data {
                        TypeData::GenericType { this_type, .. } => Ok(*this_type),
                        // A `this` inside the interface sets ContainsThis,
                        // which forces the GenericType shape — a plain
                        // Object here means the declared type is still
                        // mid-construction (cyclic heritage shell).
                        _ => Err(Unsupported::new(
                            "this type of a mid-cycle declared-type shell",
                        )),
                    };
                }
            }
        }
        self.error_at(
            Some(node),
            &diagnostics::A_this_type_is_available_only_in_a_non_static_member_of_a_class_or_interface,
            &[],
        );
        Ok(self.tables.intrinsics.error)
    }

    // ---- deferred references ----

    /// tsc-port: createDeferredTypeReference @6.0.3
    /// tsc-hash: e141b08db3abcdba6f098fdfaaedcad4db64d3d3b4ddd116208aa03ddb8cf97c
    /// tsc-span: _tsc.js:60188-60201
    ///
    /// Returns a FRESH type per call (no interning): identity comes
    /// from the callers' caches — node links.resolvedType for the
    /// canonical reference, target.instantiations for mapper-carrying
    /// instances (getObjectTypeInstantiation 63499).
    pub(crate) fn create_deferred_type_reference(
        &mut self,
        target: TypeId,
        node: NodeId,
        mapper: Option<crate::instantiate::MapperId>,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<TypeId> {
        let (alias_symbol, alias_type_arguments) = if alias_symbol.is_none() {
            let alias_symbol = self.get_alias_symbol_for_type_node(node);
            let local_alias_type_arguments = self.get_type_arguments_for_alias_symbol(alias_symbol);
            let alias_type_arguments = match (mapper, local_alias_type_arguments) {
                (Some(mapper), Some(local)) => Some(self.instantiate_types(&local, mapper)?),
                (_, local) => local,
            };
            (alias_symbol, alias_type_arguments)
        } else {
            (alias_symbol, alias_type_arguments.map(<[TypeId]>::to_vec))
        };
        let ty = self.tables.create_deferred_reference_shell(target);
        self.links
            .set_type_deferred_reference_links(self.speculation_depth, ty, node, mapper);
        let type_object = self.tables.type_mut(ty);
        type_object.alias_symbol = alias_symbol;
        type_object.alias_type_arguments = alias_type_arguments.map(Vec::into_boxed_slice);
        Ok(ty)
    }

    /// tsc-port: getTypeArguments @6.0.3
    /// tsc-hash: 3b8ca1bff64a4f4d6f3cc3397e58af16e5a44427df7f4bf4a9cf4937b366ac1d
    /// tsc-span: _tsc.js:60202-60222
    ///
    /// The lazy branch is reachable only through deferred references,
    /// whose node is ever-present — the `type.node || currentNode`
    /// error location is always the node. An Err unwind pops the
    /// resolution stack and leaves the slot vacant, so a later query
    /// re-resolves (the 5.1b unwind discipline). The pop-failure 4109/
    /// 4110 arms need an argument-forcing consumer INSIDE argument
    /// resolution — every such forcer pre-5.3 (indexed access over
    /// tuples, member access) escapes, so the arms sit unexercised
    /// until 5.3 (pin then).
    pub(crate) fn get_type_arguments(&mut self, ty: TypeId) -> CheckResult2<Vec<TypeId>> {
        if let Some(resolved) = self.tables.try_type_arguments(ty) {
            return Ok(resolved.to_vec());
        }
        if !self.push_type_resolution(
            crate::state::ResolutionTarget::Type(ty),
            tsrs2_types::TypeSystemPropertyName::RESOLVED_TYPE_ARGUMENTS,
        ) {
            // Mid-cycle read: errorType-filled arguments WITHOUT
            // caching (60206) — the outermost frame reports.
            return Ok(self.error_filled_type_arguments(ty));
        }
        let node = self
            .links
            .ty(ty)
            .deferred_node
            .expect("unresolved references are deferred (node-carrying)");
        let computed = (|state: &mut Self| -> CheckResult2<Vec<TypeId>> {
            match state.data_of(node) {
                NodeData::TypeReference(_) => {
                    let target = state.tables.reference_target(ty);
                    let TypeData::GenericType {
                        type_parameters,
                        outer_type_parameter_count,
                        ..
                    } = &state.tables.type_of(target).data
                    else {
                        unreachable!("TypeReference-node deferrals target class/interface types");
                    };
                    let outer_count = *outer_type_parameter_count;
                    let type_parameters = type_parameters.to_vec();
                    let effective = state
                        .get_effective_type_arguments(node, &type_parameters[outer_count..])?;
                    let mut arguments = type_parameters[..outer_count].to_vec();
                    arguments.extend(effective);
                    Ok(arguments)
                }
                NodeData::ArrayType(data) => {
                    let element = data
                        .element_type
                        .ok_or_else(|| Unsupported::new("array type with missing element type"))?;
                    Ok(vec![state.get_type_from_type_node(element)?])
                }
                NodeData::TupleType(data) => {
                    let elements = state.nodes_of(data.elements);
                    let mut arguments = Vec::with_capacity(elements.len());
                    for element in elements {
                        arguments.push(state.get_type_from_type_node(element)?);
                    }
                    Ok(arguments)
                }
                _ => unreachable!(
                    "deferred references carry TypeReference/ArrayType/TupleType nodes"
                ),
            }
        })(self);
        let type_arguments = match computed {
            Ok(arguments) => arguments,
            Err(err) => {
                self.pop_type_resolution();
                return Err(err);
            }
        };
        if self.pop_type_resolution() {
            // `??=` short-circuits: a slot filled during the recursive
            // resolution skips the mapper application entirely (60211).
            if self.tables.try_type_arguments(ty).is_none() {
                let resolved = match self.links.ty(ty).deferred_mapper {
                    // An Err below unwinds with the slot still vacant —
                    // nothing cached, re-queryable.
                    Some(mapper) => self.instantiate_types(&type_arguments, mapper)?,
                    None => type_arguments,
                };
                self.tables
                    .set_resolved_type_arguments_if_vacant(ty, resolved);
            }
        } else {
            let fallback = self.error_filled_type_arguments(ty);
            self.tables
                .set_resolved_type_arguments_if_vacant(ty, fallback);
            let target = self.tables.reference_target(ty);
            match self.tables.type_of(target).symbol {
                Some(symbol) => {
                    let name = self.symbol_display_name(symbol);
                    self.error_at(
                        Some(node),
                        &diagnostics::Type_arguments_for_0_circularly_reference_themselves,
                        &[&name],
                    );
                }
                None => {
                    self.error_at(
                        Some(node),
                        &diagnostics::Tuple_type_arguments_circularly_reference_themselves,
                        &[],
                    );
                }
            }
        }
        Ok(self.tables.type_arguments(ty).to_vec())
    }

    /// The push/pop-failure filler (60206/60213): outer type parameters
    /// stay themselves, local ones become errorType —
    /// `concatenate(outerTypeParameters, localTypeParameters?.map(() =>
    /// errorType)) || emptyArray`. Tuple targets treat every parameter
    /// as local (createTupleTargetType 61186-61188).
    fn error_filled_type_arguments(&self, ty: TypeId) -> Vec<TypeId> {
        let target = self.tables.reference_target(ty);
        let error = self.tables.intrinsics.error;
        match &self.tables.type_of(target).data {
            TypeData::GenericType {
                type_parameters,
                outer_type_parameter_count,
                ..
            } => {
                let mut arguments: Vec<TypeId> =
                    type_parameters[..*outer_type_parameter_count].to_vec();
                arguments.extend(std::iter::repeat_n(
                    error,
                    type_parameters.len() - outer_type_parameter_count,
                ));
                arguments
            }
            TypeData::TupleTarget(data) => vec![error; data.type_parameters.len()],
            _ => Vec::new(),
        }
    }

    /// tsc-port: getEffectiveTypeArguments @6.0.3
    /// tsc-hash: 6c12eff78b7503813dedde829e82b7ada2fbdded78d792dcc7da0591fe9498a2
    /// tsc-span: _tsc.js:81679-81681
    pub(crate) fn get_effective_type_arguments(
        &mut self,
        node: NodeId,
        type_parameters: &[TypeId],
    ) -> CheckResult2<Vec<TypeId>> {
        let argument_nodes = match self.data_of(node) {
            NodeData::TypeReference(data) => self.nodes_of(data.type_arguments),
            _ => unreachable!("getEffectiveTypeArguments reads TypeReference nodes here"),
        };
        let mut resolved = Vec::with_capacity(argument_nodes.len());
        for argument in argument_nodes {
            resolved.push(self.get_type_from_type_node(argument)?);
        }
        let min_type_argument_count = self.get_min_type_argument_count(Some(type_parameters));
        let is_js = self.is_in_js_file(node);
        Ok(self
            .fill_missing_type_arguments(
                Some(&resolved),
                Some(type_parameters),
                min_type_argument_count,
                is_js,
            )?
            .unwrap_or_default())
    }

    /// tsc-port: getTypeFromClassOrInterfaceReference @6.0.3
    /// tsc-hash: f342ce01f970d999b75075be7cad3c36a4b6defd82cd81b155a1ae78498d449b
    /// tsc-span: _tsc.js:60226-60262
    ///
    /// The missingAugmentsTag message variants are JSDoc-only (elided).
    fn get_type_from_class_or_interface_reference(
        &mut self,
        node: NodeId,
        symbol: SymbolId,
    ) -> CheckResult2<TypeId> {
        let merged = self.get_merged_symbol(symbol);
        let ty = self.get_declared_type_of_class_or_interface(merged)?;
        let (type_parameters, outer_count) = match &self.tables.type_of(ty).data {
            TypeData::GenericType {
                type_parameters,
                outer_type_parameter_count,
                ..
            } => (type_parameters.to_vec(), *outer_type_parameter_count),
            _ => (Vec::new(), 0),
        };
        let local_type_parameters = &type_parameters[outer_count..];
        if !local_type_parameters.is_empty() {
            let node_type_arguments = match self.data_of(node) {
                NodeData::TypeReference(data) => self.nodes_of(data.type_arguments),
                NodeData::ExpressionWithTypeArguments(data) => self.nodes_of(data.type_arguments),
                _ => Vec::new(),
            };
            let num_type_arguments = node_type_arguments.len();
            let min_type_argument_count =
                self.get_min_type_argument_count(Some(local_type_parameters));
            let is_js = self.is_in_js_file(node);
            let is_js_implicit_any = !self
                .options
                .strict_option_value(self.options.no_implicit_any)
                && is_js;
            if !is_js_implicit_any
                && (num_type_arguments < min_type_argument_count
                    || num_type_arguments > local_type_parameters.len())
            {
                let type_str = self.generic_type_display(ty);
                if min_type_argument_count == local_type_parameters.len() {
                    self.error_at(
                        Some(node),
                        &diagnostics::Generic_type_0_requires_1_type_argument_s,
                        &[&type_str, &min_type_argument_count.to_string()],
                    );
                } else {
                    self.error_at(
                        Some(node),
                        &diagnostics::Generic_type_0_requires_between_1_and_2_type_arguments,
                        &[
                            &type_str,
                            &min_type_argument_count.to_string(),
                            &local_type_parameters.len().to_string(),
                        ],
                    );
                }
                if !is_js {
                    return Ok(self.tables.intrinsics.error);
                }
            }
            if self.kind_of(node) == SyntaxKind::TypeReference
                && self.is_deferred_type_reference_node(
                    node,
                    num_type_arguments != local_type_parameters.len(),
                )?
            {
                return self.create_deferred_type_reference(ty, node, None, None, None);
            }
            let mut resolved_arguments: Vec<TypeId> = Vec::with_capacity(node_type_arguments.len());
            for argument in node_type_arguments {
                resolved_arguments.push(self.get_type_from_type_node(argument)?);
            }
            let local_type_parameters = local_type_parameters.to_vec();
            let filled = self
                .fill_missing_type_arguments(
                    Some(&resolved_arguments),
                    Some(&local_type_parameters),
                    min_type_argument_count,
                    is_js,
                )?
                .unwrap_or_default();
            let mut type_arguments: Vec<TypeId> = type_parameters[..outer_count].to_vec();
            type_arguments.extend(filled);
            return self
                .tables
                .create_normalized_type_reference(ty, &type_arguments)
                .map_err(Self::unsupported_m4);
        }
        Ok(if self.check_no_type_arguments(node, Some(symbol)) {
            ty
        } else {
            self.tables.intrinsics.error
        })
    }

    /// tsc-port: getTypeFromIndexedAccessTypeNode @6.0.3
    /// tsc-hash: bfdb8d46e15236842742a4ae54bf26a85b7605b13304de4118efae469dfbed94
    /// tsc-span: _tsc.js:62612-62621
    fn get_type_from_indexed_access_type_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let NodeData::IndexedAccessType(data) = self.data_of(node) else {
            unreachable!("IndexedAccessType kind implies payload");
        };
        let object_node = data
            .object_type
            .ok_or_else(|| Unsupported::new("indexed access with missing object type"))?;
        let index_node = data
            .index_type
            .ok_or_else(|| Unsupported::new("indexed access with missing index type"))?;
        let object_type = self.get_type_from_type_node(object_node)?;
        let index_type = self.get_type_from_type_node(index_node)?;
        let potential_alias = self.get_alias_symbol_for_type_node(node);
        let alias_type_arguments = self.get_type_arguments_for_alias_symbol(potential_alias);
        let resolved = self.get_indexed_access_type(
            object_type,
            index_type,
            tsrs2_types::AccessFlags::NONE,
            Some(node),
            potential_alias,
            alias_type_arguments.as_deref(),
        )?;
        self.links.set_node_resolved_type(
            self.speculation_depth,
            node,
            LinkSlot::Resolved(resolved),
        );
        Ok(resolved)
    }

    /// tsc-port: getTypeArgumentsForAliasSymbol @6.0.3
    /// tsc-hash: 3515d46635004f0f184c6e32860b51099bc66259e42e5af8f1777adc0f086061
    /// tsc-span: _tsc.js:62915-62917
    fn get_type_arguments_for_alias_symbol(
        &mut self,
        symbol: Option<SymbolId>,
    ) -> Option<Vec<TypeId>> {
        let symbol = symbol?;
        let type_parameters =
            self.get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
        (!type_parameters.is_empty()).then_some(type_parameters)
    }

    /// tsc-port: getTypeFromTypeAliasReference @6.0.3
    /// tsc-hash: 4117b012190268bec69ab226d5b25d0561d7bd3630fae40819022c04e2b1f3dc
    /// tsc-span: _tsc.js:60278-60335
    ///
    /// Elisions, each owned by a later stage: the Unresolved-check-flag
    /// error-alias arm (60279-60296 — unresolvedSymbols are
    /// unconstructible while unresolved names escape at resolution) and
    /// the import-alias re-resolution arm (60313-60329, resolveAlias =
    /// 5.8) — an alias REFERENCED THROUGH an import keeps aliasSymbol
    /// None, an alias-identity FN only.
    fn get_type_from_type_alias_reference(
        &mut self,
        node: NodeId,
        symbol: SymbolId,
    ) -> CheckResult2<TypeId> {
        let ty = self.get_declared_type_of_type_alias(symbol)?;
        let type_parameters = self.links.symbol(symbol).type_parameters.clone();
        if let Some(type_parameters) = type_parameters {
            let node_type_arguments = match self.data_of(node) {
                NodeData::TypeReference(data) => self.nodes_of(data.type_arguments),
                NodeData::ExpressionWithTypeArguments(data) => self.nodes_of(data.type_arguments),
                _ => Vec::new(),
            };
            let num_type_arguments = node_type_arguments.len();
            let min_type_argument_count = self.get_min_type_argument_count(Some(&type_parameters));
            if num_type_arguments < min_type_argument_count
                || num_type_arguments > type_parameters.len()
            {
                // Alias arity errors display the PLAIN symbol name
                // (symbolToString), unlike the class/interface
                // typeToString form — oracle-pinned.
                let display = self.symbol_display_name(symbol);
                if min_type_argument_count == type_parameters.len() {
                    self.error_at(
                        Some(node),
                        &diagnostics::Generic_type_0_requires_1_type_argument_s,
                        &[&display, &min_type_argument_count.to_string()],
                    );
                } else {
                    self.error_at(
                        Some(node),
                        &diagnostics::Generic_type_0_requires_between_1_and_2_type_arguments,
                        &[
                            &display,
                            &min_type_argument_count.to_string(),
                            &type_parameters.len().to_string(),
                        ],
                    );
                }
                return Ok(self.tables.intrinsics.error);
            }
            let alias_symbol = self.get_alias_symbol_for_type_node(node);
            let new_alias_symbol = alias_symbol.filter(|&alias| {
                self.is_local_type_alias(symbol) || !self.is_local_type_alias(alias)
            });
            let alias_type_arguments = self.get_type_arguments_for_alias_symbol(new_alias_symbol);
            let mut resolved_arguments: Vec<TypeId> = Vec::with_capacity(node_type_arguments.len());
            for argument in node_type_arguments {
                resolved_arguments.push(self.get_type_from_type_node(argument)?);
            }
            let type_arguments = (num_type_arguments > 0).then_some(resolved_arguments);
            return self.get_type_alias_instantiation(
                symbol,
                type_arguments.as_deref(),
                new_alias_symbol,
                alias_type_arguments.as_deref(),
            );
        }
        Ok(if self.check_no_type_arguments(node, Some(symbol)) {
            ty
        } else {
            self.tables.intrinsics.error
        })
    }

    /// tsc-port: getTypeAliasInstantiation @6.0.3
    /// tsc-hash: 8aafbc240586103fe0d9771544e0eea8d9057c8726f39b56fe9f613add9aeb45
    /// tsc-span: _tsc.js:60263-60277
    ///
    /// The NoInfer intrinsic escapes (getNoInferType mints Substitution
    /// types, M8); Uppercase/Lowercase/Capitalize/Uncapitalize route to
    /// getStringMappingType.
    pub(crate) fn get_type_alias_instantiation(
        &mut self,
        symbol: SymbolId,
        type_arguments: Option<&[TypeId]>,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<TypeId> {
        let ty = self.get_declared_type_of_type_alias(symbol)?;
        if ty == self.tables.intrinsics.intrinsic_marker {
            let name = self.binder.symbol(symbol).escaped_name.clone();
            if let Some(kind) = crate::instantiate::intrinsic_type_kind(&name) {
                if let Some(arguments) = type_arguments {
                    if arguments.len() == 1 {
                        return if kind == crate::instantiate::IntrinsicTypeKind::NoInfer {
                            Err(Unsupported::new(
                                "NoInfer intrinsic (getNoInferType — Substitution types, M8)",
                            ))
                        } else {
                            self.get_string_mapping_type(symbol, arguments[0])
                        };
                    }
                }
            }
        }
        let type_parameters = self
            .links
            .symbol(symbol)
            .type_parameters
            .clone()
            .expect("getTypeAliasInstantiation callers gate on typeParameters");
        let id_key = format!(
            "{}{}",
            self.tables.get_type_list_id(type_arguments.unwrap_or(&[])),
            self.tables.get_alias_id(alias_symbol, alias_type_arguments)
        );
        if let Some(&instantiation) = self
            .links
            .alias_instantiations
            .get(&(symbol, id_key.clone()))
        {
            return Ok(instantiation);
        }
        let min_type_argument_count = self.get_min_type_argument_count(Some(&type_parameters));
        let is_js = self
            .binder
            .symbol(symbol)
            .value_declaration
            .is_some_and(|declaration| self.is_in_js_file(declaration));
        let filled = self.fill_missing_type_arguments(
            type_arguments,
            Some(&type_parameters),
            min_type_argument_count,
            is_js,
        )?;
        let mapper = self.create_type_mapper(type_parameters, filled);
        let instantiation =
            self.instantiate_type_with_alias(ty, mapper, alias_symbol, alias_type_arguments)?;
        self.links
            .alias_instantiations
            .insert((symbol, id_key), instantiation);
        Ok(instantiation)
    }

    /// tsc-port: isLocalTypeAlias @6.0.3
    /// tsc-hash: db92e7ec2cc0c83423b0931394ac73aabadd9028b9f2a3a8a98024bcaef6f4f7
    /// tsc-span: _tsc.js:60336-60340
    fn is_local_type_alias(&self, symbol: SymbolId) -> bool {
        let declaration = self
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| self.kind_of(declaration) == SyntaxKind::TypeAliasDeclaration);
        declaration.is_some_and(|declaration| {
            let mut current = self.parent_of(declaration);
            while let Some(node) = current {
                if tsrs2_binder::node_util::is_function_like_kind(self.kind_of(node)) {
                    return true;
                }
                current = self.parent_of(node);
            }
            false
        })
    }

    /// tsc-port: checkNoTypeArguments @6.0.3
    /// tsc-hash: 0468ee396427d4a338fbca17bb95e2f10429f69271d64bb787682f2631661408
    /// tsc-span: _tsc.js:60486-60492
    fn check_no_type_arguments(&mut self, node: NodeId, symbol: Option<SymbolId>) -> bool {
        let type_arguments = match self.data_of(node) {
            NodeData::TypeReference(data) => data.type_arguments,
            NodeData::ExpressionWithTypeArguments(data) => data.type_arguments,
            _ => None,
        };
        if type_arguments.is_some() {
            let display = match symbol {
                Some(symbol) => self.symbol_display_name(symbol),
                None => match self.data_of(node) {
                    NodeData::TypeReference(data) => data
                        .type_name
                        .map(|name| {
                            tsrs2_binder::node_util::declaration_name_to_string(
                                self.binder.source_of_node(name),
                                Some(name),
                            )
                        })
                        .unwrap_or_else(|| "(anonymous)".to_owned()),
                    _ => "(anonymous)".to_owned(),
                },
            };
            self.error_at(Some(node), &diagnostics::Type_0_is_not_generic, &[&display]);
            return false;
        }
        true
    }

    /// typeToString slice for the 2314/2707 family: a class/interface
    /// GenericType displays as `Name<L1, L2>` over its LOCAL type
    /// parameters only — the nodeBuilder's typeReferenceToTypeNode
    /// splits outer parameters into enclosing-declaration
    /// qualification, which drops without an enclosing declaration
    /// (oracle-pinned: `I<U>` for a fn-scoped `interface I<U>` with
    /// outer `T`).
    fn generic_type_display(&self, ty: TypeId) -> String {
        let symbol = self
            .tables
            .type_of(ty)
            .symbol
            .expect("declared types carry their symbol");
        let name = self.symbol_display_name(symbol);
        match &self.tables.type_of(ty).data {
            TypeData::GenericType {
                type_parameters,
                outer_type_parameter_count,
                ..
            } if type_parameters.len() > *outer_type_parameter_count => {
                let locals: Vec<String> = type_parameters[*outer_type_parameter_count..]
                    .iter()
                    .map(|&parameter| {
                        self.tables
                            .type_of(parameter)
                            .symbol
                            .map(|s| self.symbol_display_name(s))
                            .unwrap_or_default()
                    })
                    .collect();
                format!("{name}<{}>", locals.join(", "))
            }
            _ => name,
        }
    }

    /// tsc-port: getAliasSymbolForTypeNode @6.0.3
    /// tsc-hash: 210d8f6d63e8913008a038e4878d54e360b1173134393fb2f189b9d1d7e88f97
    /// tsc-span: _tsc.js:62908-62914
    ///
    /// JSDoc type-expression hosts are elided.
    pub(crate) fn get_alias_symbol_for_type_node(&self, node: NodeId) -> Option<SymbolId> {
        let mut host = self.parent_of(node)?;
        loop {
            let hop = match self.data_of(host) {
                NodeData::ParenthesizedType(_) => true,
                NodeData::TypeOperator(data) => data.operator == SyntaxKind::ReadonlyKeyword,
                _ => false,
            };
            if !hop {
                break;
            }
            host = self.parent_of(host)?;
        }
        if self.kind_of(host) == SyntaxKind::TypeAliasDeclaration {
            // getSymbolOfDeclaration (62913).
            self.node_symbol(host).map(|s| self.get_merged_symbol(s))
        } else {
            None
        }
    }

    /// tsc-port: isDeferredTypeReferenceNode @6.0.3
    /// tsc-hash: 391c3bed6841ebc32f348473977f7496fa9807d15f2929dde920d4c94a758105
    /// tsc-span: _tsc.js:61068-61072
    fn is_deferred_type_reference_node(
        &mut self,
        node: NodeId,
        has_default_type_arguments: bool,
    ) -> CheckResult2<bool> {
        if self.get_alias_symbol_for_type_node(node).is_some() {
            return Ok(true);
        }
        if !self.is_resolved_by_type_alias(node) {
            return Ok(false);
        }
        match self.data_of(node) {
            NodeData::ArrayType(data) => match data.element_type {
                Some(element) => self.may_resolve_type_alias(element),
                None => Ok(false),
            },
            NodeData::TupleType(data) => {
                for element in self.nodes_of(data.elements) {
                    if self.may_resolve_type_alias(element)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => {
                if has_default_type_arguments {
                    return Ok(true);
                }
                let type_arguments = match self.data_of(node) {
                    NodeData::TypeReference(data) => data.type_arguments,
                    _ => None,
                };
                for argument in self.nodes_of(type_arguments) {
                    if self.may_resolve_type_alias(argument)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }

    /// tsc-port: isResolvedByTypeAlias @6.0.3
    /// tsc-hash: 5e91bb14024740ab93e30c8ef652e2c28e784de7229b690ce4ba41e56ebfb452
    /// tsc-span: _tsc.js:61073-61089
    fn is_resolved_by_type_alias(&self, node: NodeId) -> bool {
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        match self.kind_of(parent) {
            SyntaxKind::ParenthesizedType
            | SyntaxKind::NamedTupleMember
            | SyntaxKind::TypeReference
            | SyntaxKind::UnionType
            | SyntaxKind::IntersectionType
            | SyntaxKind::IndexedAccessType
            | SyntaxKind::ConditionalType
            | SyntaxKind::TypeOperator
            | SyntaxKind::ArrayType
            | SyntaxKind::TupleType => self.is_resolved_by_type_alias(parent),
            SyntaxKind::TypeAliasDeclaration => true,
            _ => false,
        }
    }

    /// tsc-port: mayResolveTypeAlias @6.0.3
    /// tsc-hash: 845e0c512d1edd3015e228151cb17d5e05504158f1d0597577340b3bc79f73bf
    /// tsc-span: _tsc.js:61090-61114
    ///
    /// The TypeReference arm resolves the name with tsc's ERROR-
    /// emitting resolution (resolveTypeReferenceName without
    /// ignoreErrors); JSDoc kinds are elided.
    fn may_resolve_type_alias(&mut self, node: NodeId) -> CheckResult2<bool> {
        match self.data_of(node) {
            NodeData::TypeReference(data) => {
                let Some(type_name) = data.type_name else {
                    return Ok(false);
                };
                let symbol = self.resolve_entity_name(
                    type_name,
                    SymbolFlags::TYPE,
                    /*ignore_errors*/ false,
                    None,
                );
                Ok(symbol.is_some_and(|symbol| {
                    self.symbol_flags(symbol)
                        .intersects(SymbolFlags::TYPE_ALIAS)
                }))
            }
            NodeData::TypeQuery(_) => Ok(true),
            NodeData::TypeOperator(data) => {
                if data.operator == SyntaxKind::UniqueKeyword {
                    return Ok(false);
                }
                match data.r#type {
                    Some(inner) => self.may_resolve_type_alias(inner),
                    None => Ok(false),
                }
            }
            NodeData::ParenthesizedType(data) => match data.r#type {
                Some(inner) => self.may_resolve_type_alias(inner),
                None => Ok(false),
            },
            NodeData::OptionalType(data) => match data.r#type {
                Some(inner) => self.may_resolve_type_alias(inner),
                None => Ok(false),
            },
            NodeData::NamedTupleMember(data) => match data.r#type {
                Some(inner) => self.may_resolve_type_alias(inner),
                None => Ok(false),
            },
            NodeData::RestType(data) => {
                let Some(inner) = data.r#type else {
                    return Ok(false);
                };
                match self.data_of(inner) {
                    NodeData::ArrayType(array) => match array.element_type {
                        Some(element) => self.may_resolve_type_alias(element),
                        None => Ok(false),
                    },
                    _ => Ok(true),
                }
            }
            NodeData::UnionType(data) => {
                for member in self.nodes_of(data.types) {
                    if self.may_resolve_type_alias(member)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            NodeData::IntersectionType(data) => {
                for member in self.nodes_of(data.types) {
                    if self.may_resolve_type_alias(member)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            NodeData::IndexedAccessType(data) => {
                if let Some(object) = data.object_type {
                    if self.may_resolve_type_alias(object)? {
                        return Ok(true);
                    }
                }
                match data.index_type {
                    Some(index) => self.may_resolve_type_alias(index),
                    None => Ok(false),
                }
            }
            NodeData::ConditionalType(data) => {
                for part in [
                    data.check_type,
                    data.extends_type,
                    data.true_type,
                    data.false_type,
                ]
                .into_iter()
                .flatten()
                {
                    if self.may_resolve_type_alias(part)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    /// tsc-port: getTypeFromTypeQueryNode @6.0.3
    /// tsc-hash: d8e9b4a2ea79ce1b11bdaebf9b475b2b7175e9b653c0e8c0f87925ab8908f7c6
    /// tsc-span: _tsc.js:60596-60603
    ///
    /// Slice: entity-name exprName over resolveEntityName +
    /// getTypeOfSymbol — the checkExpressionWithTypeArguments route
    /// (77963) with checkExpression's identifier/qualified-name arms
    /// collapses to exactly this while identifiers carry their
    /// declared types (flow narrowing is M5; type arguments on typeof
    /// are `typeof f<...>` instantiation expressions, 5.2/M6).
    fn get_type_from_type_query_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let NodeData::TypeQuery(data) = self.data_of(node) else {
            unreachable!("TypeQuery kind implies payload");
        };
        if data.type_arguments.is_some() {
            return Err(Unsupported::new(
                "typeof with type arguments (instantiation expressions, M4 5.2/M6)",
            ));
        }
        let expr_name = data
            .expr_name
            .ok_or_else(|| Unsupported::new("typeof with missing entity name"))?;
        // resolveEntityName reports (2304 family) and yields
        // unknownSymbol; typeof then types as errorType.
        let ty = match self.resolve_entity_name(
            expr_name,
            SymbolFlags::VALUE,
            /*ignore_errors*/ false,
            None,
        ) {
            Some(symbol) => self.get_type_of_symbol(symbol)?,
            None => self.tables.intrinsics.error,
        };
        let widened = self.get_widened_type(ty)?;
        let resolved = self.tables.get_regular_type_of_literal_type(widened);
        self.links.set_node_resolved_type(
            self.speculation_depth,
            node,
            LinkSlot::Resolved(resolved),
        );
        Ok(resolved)
    }

    /// tsc-port: getDeclaredTypeOfTypeAlias @6.0.3
    /// tsc-hash: 65be838227a2b645257234d352a6b1a615000a261692235cd9be50c2672cb6d6
    /// tsc-span: _tsc.js:57404-57435
    ///
    /// Slice notes: the links.typeParameters/instantiations
    /// bookkeeping is 5.2's (generic alias REFERENCES are Unsupported
    /// at the reference arm, so skipping it is verdict-neutral); the
    /// JSDoc type-alias arms are elided; the BuiltinIteratorReturn
    /// intrinsic-marker swap needs iterator globals (5.8 iteration
    /// protocol) and unwinds as Unsupported.
    pub(crate) fn get_declared_type_of_type_alias(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.symbol(symbol).declared_type.resolved() {
            return Ok(cached);
        }
        if !self.push_type_resolution(
            crate::state::ResolutionTarget::Symbol(symbol),
            tsrs2_types::TypeSystemPropertyName::DECLARED_TYPE,
        ) {
            return Ok(self.tables.intrinsics.error);
        }
        let declaration = self
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| self.kind_of(declaration) == SyntaxKind::TypeAliasDeclaration);
        let computed = (|state: &mut Self| -> CheckResult2<TypeId> {
            let Some(declaration) = declaration else {
                return Err(Unsupported::new(
                    "type alias symbol without a TypeAliasDeclaration (JSDoc aliases unmodeled)",
                ));
            };
            let NodeData::TypeAliasDeclaration(data) = state.data_of(declaration) else {
                unreachable!("TypeAliasDeclaration kind implies payload");
            };
            match data.r#type {
                Some(type_node) => state.get_type_from_type_node(type_node),
                None => Ok(state.tables.intrinsics.error),
            }
        })(self);
        let ty = match computed {
            Ok(ty) => ty,
            Err(err) => {
                self.pop_type_resolution();
                return Err(err);
            }
        };
        let ty = if self.pop_type_resolution() {
            // 57415-57419: generic aliases stamp their local type
            // parameters + seed the instantiations map with the
            // uninstantiated declared type.
            let type_parameters =
                self.get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
            if !type_parameters.is_empty() {
                let list_id = self.tables.get_type_list_id(&type_parameters);
                self.links.set_symbol_type_parameters(
                    self.speculation_depth,
                    symbol,
                    type_parameters,
                );
                self.links
                    .alias_instantiations
                    .insert((symbol, list_id), ty);
            }
            if ty == self.tables.intrinsics.intrinsic_marker
                && self.binder.symbol(symbol).escaped_name == "BuiltinIteratorReturn"
            {
                return Err(Unsupported::new(
                    "BuiltinIteratorReturn intrinsic alias (iterator globals, M4 5.8)",
                ));
            }
            ty
        } else {
            // 57426-57432: the cycle came from a deeper frame.
            let error_node = declaration
                .and_then(|declaration| self.name_of_node(declaration).or(Some(declaration)));
            let name = self.symbol_display_name(symbol);
            self.error_at(
                error_node,
                &diagnostics::Type_alias_0_circularly_references_itself,
                &[&name],
            );
            self.tables.intrinsics.error
        };
        self.links
            .set_symbol_declared_type(self.speculation_depth, symbol, LinkSlot::Resolved(ty));
        Ok(ty)
    }

    /// tsc-port: getDeclaredTypeOfClassOrInterface @6.0.3
    /// tsc-hash: b159a970fade450a929f147df283c2d536e3a3459c66ac6b6e9b9675173ef57c
    /// tsc-span: _tsc.js:57375-57403
    ///
    /// The JS merge arm (mergeJSSymbols + getAssignedClassSymbol,
    /// 57380-57384) is elided with JS Assignment binding (M2 3.4c
    /// residual). tsc writes the shell into the links BEFORE computing
    /// type parameters and thisless-ness, so cyclic heritage reads a
    /// thisType-less shell mid-computation; here the slot is written on
    /// success only (Err unwinds stay re-queryable) and the in-progress
    /// set reproduces the same mid-cycle observable for the ONLY
    /// mid-cycle reader, isThislessInterface's base walk.
    pub(crate) fn get_declared_type_of_class_or_interface(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.symbol(symbol).declared_type.resolved() {
            return Ok(cached);
        }
        assert!(
            !self.class_interface_declared_in_progress.contains(&symbol),
            "re-entrant declared-type computation must route through the in-progress set"
        );
        self.class_interface_declared_in_progress.push(symbol);
        let computed = self.compute_declared_type_of_class_or_interface(symbol);
        self.class_interface_declared_in_progress.pop();
        let id = computed?;
        self.links
            .set_symbol_declared_type(self.speculation_depth, symbol, LinkSlot::Resolved(id));
        Ok(id)
    }

    fn compute_declared_type_of_class_or_interface(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<TypeId> {
        let is_class = self.symbol_flags(symbol).intersects(SymbolFlags::CLASS);
        let kind = if is_class {
            ObjectFlags::CLASS
        } else {
            ObjectFlags::INTERFACE
        };
        let outer_type_parameters = self
            .get_outer_type_parameters_of_class_or_interface(symbol)?
            .unwrap_or_default();
        let local_type_parameters =
            self.get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
        let generic = !outer_type_parameters.is_empty()
            || !local_type_parameters.is_empty()
            || is_class
            || !self.is_thisless_interface(symbol)?;
        let id = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
        self.tables.type_mut(id).object_flags = kind;
        self.tables.type_mut(id).symbol = Some(symbol);
        if generic {
            let outer_count = outer_type_parameters.len();
            let mut type_parameters = outer_type_parameters;
            type_parameters.extend(local_type_parameters);
            // 57392-57393: the instantiations map is seeded with the
            // target under its own type-parameter list id (the shared
            // tables map that createTypeReference consults).
            let list_id = self.tables.get_type_list_id(&type_parameters);
            self.tables.instantiation_insert(id, list_id, id);
            // 57400-57402: thisType — isThisType, constraint = target
            // (the inline constraint slot, like tuple this types).
            let this_type = self.tables.create_type(
                TypeFlags::TYPE_PARAMETER,
                TypeData::TypeParameter {
                    is_this_type: true,
                    constraint: Some(id),
                },
            );
            self.tables.type_mut(this_type).symbol = Some(symbol);
            let generic_type = self.tables.type_mut(id);
            generic_type.object_flags =
                ObjectFlags::from_bits(kind.bits() | ObjectFlags::REFERENCE.bits());
            generic_type.data = TypeData::GenericType {
                type_parameters: type_parameters.into_boxed_slice(),
                outer_type_parameter_count: outer_count,
                this_type,
            };
        }
        Ok(id)
    }

    /// tsc-port: getOuterTypeParametersOfClassOrInterface @6.0.3
    /// tsc-hash: d96d0807f547cc2abbdc6f299657709def45973f5656cbb5629f9235ca88be6b
    /// tsc-span: _tsc.js:57080-57094
    ///
    /// The variable-declaration-with-function-initializer arm is the JS
    /// constructor pattern — elided with JS binding. The single-argument
    /// getOuterTypeParameters call means includeThisTypes = false.
    fn get_outer_type_parameters_of_class_or_interface(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<Option<Vec<TypeId>>> {
        let flags = self.symbol_flags(symbol);
        let declaration = if flags.intersects(SymbolFlags::CLASS | SymbolFlags::FUNCTION) {
            self.binder.symbol(symbol).value_declaration
        } else {
            self.binder
                .symbol(symbol)
                .declarations
                .iter()
                .copied()
                .find(|&declaration| self.kind_of(declaration) == SyntaxKind::InterfaceDeclaration)
        };
        let declaration = declaration.expect(
            "Class was missing valueDeclaration -OR- non-class had no interface declarations",
        );
        self.get_outer_type_parameters(declaration, /*include_this_types*/ false)
    }

    /// tsc-port: getLocalTypeParametersOfClassOrInterfaceOrTypeAlias @6.0.3
    /// tsc-hash: de3a9b63c89901776c9fc681db222f2b987853ff368fa18a8735bf1a83867f9c
    /// tsc-span: _tsc.js:57095-57107
    ///
    /// isJSConstructor and the JSDoc typedef/callback alias kinds are
    /// elided project-wide.
    pub(crate) fn get_local_type_parameters_of_class_or_interface_or_type_alias(
        &mut self,
        symbol: SymbolId,
    ) -> Vec<TypeId> {
        let declarations = self.binder.symbol(symbol).declarations.clone();
        let mut result: Vec<TypeId> = Vec::new();
        for node in declarations {
            if matches!(
                self.kind_of(node),
                SyntaxKind::InterfaceDeclaration
                    | SyntaxKind::ClassDeclaration
                    | SyntaxKind::ClassExpression
                    | SyntaxKind::TypeAliasDeclaration
            ) {
                let parameter_declarations = self.type_parameter_declarations_of(node);
                result = self.append_type_parameters(result, &parameter_declarations);
            }
        }
        result
    }

    /// tsc-port: isThislessInterface @6.0.3
    /// tsc-hash: e55eea0f7b249c2868dbb9574c61319f21bc708bb79efac5e44adbe8cf2a3221
    /// tsc-span: _tsc.js:57346-57374
    ///
    /// An in-progress base (cyclic heritage) reads as tsc's eagerly
    /// written shell: no thisType yet — the check passes.
    fn is_thisless_interface(&mut self, symbol: SymbolId) -> CheckResult2<bool> {
        let declarations = self.binder.symbol(symbol).declarations.clone();
        for declaration in declarations {
            if self.kind_of(declaration) != SyntaxKind::InterfaceDeclaration {
                continue;
            }
            if self.node_flags(declaration) & tsrs2_types::NodeFlags::CONTAINS_THIS.bits() != 0 {
                return Ok(false);
            }
            for base_node in self.interface_base_type_nodes(declaration) {
                let NodeData::ExpressionWithTypeArguments(data) = self.data_of(base_node) else {
                    continue;
                };
                let Some(expression) = data.expression else {
                    continue;
                };
                if !self.is_entity_name_expression(expression) {
                    continue;
                }
                let base_symbol = self.resolve_entity_name(
                    expression,
                    SymbolFlags::TYPE,
                    /*ignore_errors*/ true,
                    None,
                );
                let Some(base_symbol) = base_symbol else {
                    return Ok(false);
                };
                if !self
                    .symbol_flags(base_symbol)
                    .intersects(SymbolFlags::INTERFACE)
                {
                    return Ok(false);
                }
                if self
                    .class_interface_declared_in_progress
                    .contains(&base_symbol)
                {
                    // Mid-cycle: the base's (eager) shell carries no
                    // thisType yet.
                    continue;
                }
                let base_declared = self.get_declared_type_of_class_or_interface(base_symbol)?;
                if matches!(
                    self.tables.type_of(base_declared).data,
                    TypeData::GenericType { .. }
                ) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// tsc-port: getInterfaceBaseTypeNodes @6.0.3
    /// tsc-hash: 452f64003ee12e280cae2b3c6c074e8a3b46f3d7f75c107eb8a7de1a126836d4
    /// tsc-span: _tsc.js:15764-15767
    ///
    /// The extends heritage clause's types (token recovered from source
    /// text, like every heritage read).
    fn interface_base_type_nodes(&self, declaration: NodeId) -> Vec<NodeId> {
        let NodeData::InterfaceDeclaration(data) = self.data_of(declaration) else {
            return Vec::new();
        };
        for clause in self.nodes_of(data.heritage_clauses) {
            if self.heritage_clause_is_extends(clause) {
                let NodeData::HeritageClause(clause_data) = self.data_of(clause) else {
                    continue;
                };
                return self.nodes_of(clause_data.types);
            }
        }
        Vec::new()
    }

    /// tsc-port: isEntityNameExpression @6.0.3
    /// tsc-hash: 2e7694f05260a41567e84db34bfbfd9ec77c27e3c37116b2a9cf88f0ddccfeee
    /// tsc-span: _tsc.js:17128-17130
    pub(crate) fn is_entity_name_expression(&self, node: NodeId) -> bool {
        match self.data_of(node) {
            NodeData::Identifier(_) => true,
            NodeData::PropertyAccessExpression(data) => {
                data.name
                    .is_some_and(|name| self.kind_of(name) == SyntaxKind::Identifier)
                    && data
                        .expression
                        .is_some_and(|expression| self.is_entity_name_expression(expression))
            }
            _ => false,
        }
    }

    // ---- structured member resolution ----

    /// tsc-port: resolveStructuredTypeMembers @6.0.3
    /// tsc-hash: 07b72758470ff7a70755a9aebe3dda44c543f90521b873fda21f7e03be3793a1
    /// tsc-span: _tsc.js:58679-58704
    ///
    /// ReverseMapped/Mapped member resolution is M8; union and
    /// intersection member synthesis is 5.3d.
    pub fn resolve_structured_type_members(&mut self, ty: TypeId) -> CheckResult2<MembersId> {
        if let Some(members) = self.links.ty(ty).resolved_members.resolved() {
            return Ok(members);
        }
        let flags = self.tables.flags_of(ty);
        if !flags.intersects(TypeFlags::OBJECT) {
            if flags.intersects(TypeFlags::UNION) {
                return self.resolve_union_type_members(ty);
            }
            if flags.intersects(TypeFlags::INTERSECTION) {
                return self.resolve_intersection_type_members(ty);
            }
            unreachable!("resolveStructuredTypeMembers takes structured types");
        }
        let object_flags = self.tables.object_flags_of(ty);
        if object_flags.intersects(ObjectFlags::REFERENCE) {
            return self.resolve_type_reference_members(ty);
        }
        if object_flags.intersects(ObjectFlags::CLASS_OR_INTERFACE) {
            return self.resolve_class_or_interface_members(ty);
        }
        if object_flags.intersects(ObjectFlags::ANONYMOUS) {
            return self.resolve_anonymous_type_members(ty);
        }
        Err(Unsupported::new(format!(
            "member resolution for object flags {object_flags:?} (M4)"
        )))
    }

    /// tsc-port: resolveClassOrInterfaceMembers @6.0.3
    /// tsc-hash: dc755164dcb68d5a89257563a1788b16d318f635ea42cb45362471caab22073b
    /// tsc-span: _tsc.js:57842-57844
    fn resolve_class_or_interface_members(&mut self, ty: TypeId) -> CheckResult2<MembersId> {
        let source = self.resolve_declared_members(ty)?;
        self.resolve_object_type_members(ty, ty, source, &[], &[])
    }

    /// tsc-port: resolveTypeReferenceMembers @6.0.3
    /// tsc-hash: 3761f294b677fb0961124508846997f613d16efd53c91352cd4fbd0053548734
    /// tsc-span: _tsc.js:57845-57852
    fn resolve_type_reference_members(&mut self, ty: TypeId) -> CheckResult2<MembersId> {
        let target = self.tables.reference_target(ty);
        let source = self.resolve_declared_members(target)?;
        let (source_type_parameters, this_type) = match &self.tables.type_of(target).data {
            TypeData::GenericType {
                type_parameters,
                this_type,
                ..
            } => (type_parameters.to_vec(), *this_type),
            TypeData::TupleTarget(data) => (data.type_parameters.to_vec(), data.this_type),
            _ => unreachable!("reference targets are generic or tuple targets"),
        };
        let mut type_parameters = source_type_parameters;
        type_parameters.push(this_type);
        let type_arguments = self.get_type_arguments(ty)?;
        let padded_type_arguments = if type_arguments.len() == type_parameters.len() {
            type_arguments
        } else {
            let mut padded = type_arguments;
            padded.push(ty);
            padded
        };
        self.resolve_object_type_members(
            ty,
            target,
            source,
            &type_parameters,
            &padded_type_arguments,
        )
    }

    /// tsc-port: resolveDeclaredMembers @6.0.3
    /// tsc-hash: 26214e56476509650c70cc07871cd14e249f549efc1bffc1fc84e33349b0a7e0
    /// tsc-span: _tsc.js:57602-57615
    ///
    /// The declared (OWN) members of a class/interface target —
    /// tsc's type.declaredProperties/declaredCallSignatures/
    /// declaredConstructSignatures/declaredIndexInfos, stored as one
    /// ResolvedMembers in TypeLinks.declared_members. Tuple targets
    /// synthesize their declared members at creation in tsc
    /// (61160-61185) — that synthesis is 5.3c.
    fn resolve_declared_members(&mut self, target: TypeId) -> CheckResult2<MembersId> {
        if let Some(declared) = self.links.ty(target).declared_members.resolved() {
            return Ok(declared);
        }
        if let TypeData::TupleTarget(data) = self.tables.type_of(target).data.clone() {
            // createTupleTargetType's property synthesis (61160-61185),
            // deferred from creation to first read: per-index props for
            // positions before the first Variable element (links.type =
            // the marker parameter), then the length property (number
            // with a rest element, else the minLength..=arity literal
            // union). Call/construct/index lists are empty (61198-61200).
            let mut properties: Vec<SymbolId> = Vec::new();
            let mut combined = ElementFlags::from_bits(0);
            for (i, &type_parameter) in data.type_parameters.iter().enumerate() {
                let flags = data.element_flags[i];
                combined |= flags;
                if !combined.intersects(ElementFlags::VARIABLE) {
                    let mut symbol_flags = SymbolFlags::PROPERTY;
                    if flags.intersects(ElementFlags::OPTIONAL) {
                        symbol_flags |= SymbolFlags::OPTIONAL;
                    }
                    let property = self.binder.create_symbol(symbol_flags, i.to_string());
                    if data.readonly {
                        self.links.set_symbol_check_flags(
                            self.speculation_depth,
                            property,
                            CheckFlags::READONLY,
                        );
                    }
                    let label = data
                        .labeled_element_declarations
                        .as_ref()
                        .and_then(|declarations| declarations.get(i).copied())
                        .flatten();
                    if let Some(label) = label {
                        self.links.set_symbol_tuple_label_declaration(
                            self.speculation_depth,
                            property,
                            NodeId(label),
                        );
                    }
                    self.links.set_symbol_type(
                        self.speculation_depth,
                        property,
                        LinkSlot::Resolved(type_parameter),
                    );
                    properties.push(property);
                }
            }
            let length_symbol = self
                .binder
                .create_symbol(SymbolFlags::PROPERTY, "length".to_owned());
            if data.readonly {
                self.links.set_symbol_check_flags(
                    self.speculation_depth,
                    length_symbol,
                    CheckFlags::READONLY,
                );
            }
            let length_type = if combined.intersects(ElementFlags::VARIABLE) {
                self.tables.intrinsics.number
            } else {
                let literals: Vec<TypeId> = (data.min_length..=data.type_parameters.len())
                    .map(|length| self.tables.get_number_literal_type(length as f64))
                    .collect();
                self.get_union_type_ex(&literals, UnionReduction::Literal)?
            };
            self.links.set_symbol_type(
                self.speculation_depth,
                length_symbol,
                LinkSlot::Resolved(length_type),
            );
            properties.push(length_symbol);
            let members = self.symbol_list_to_table(&properties);
            let id = self.alloc_members(ResolvedMembers {
                members,
                properties,
                ..ResolvedMembers::default()
            });
            self.links
                .set_type_declared_members(self.speculation_depth, target, id);
            return Ok(id);
        }
        let symbol = self
            .tables
            .type_of(target)
            .symbol
            .expect("class/interface targets carry their declaring symbol");
        let members = self.get_members_of_symbol(symbol)?;
        let properties = self.get_named_members(&members);
        let call_signatures =
            self.get_signatures_of_symbol(members.get(InternalSymbolName::CALL).copied())?;
        let construct_signatures =
            self.get_signatures_of_symbol(members.get(InternalSymbolName::NEW).copied())?;
        let index_infos = self.get_index_infos_of_symbol(symbol)?;
        let id = self.alloc_members(ResolvedMembers {
            members,
            properties,
            call_signatures,
            construct_signatures,
            index_infos,
        });
        self.links
            .set_type_declared_members(self.speculation_depth, target, id);
        Ok(id)
    }

    /// tsc-port: resolveObjectTypeMembers @6.0.3
    /// tsc-hash: fe670f3b254fb8e6ba9ef3f70ea39509b72d60ac7e4c6c16220bdacc105fa67e
    /// tsc-span: _tsc.js:57796-57841
    ///
    /// `source` is the declared-members carrier (`source_type` its
    /// owner). The early setStructuredTypeMembers (57829) is ported as
    /// an early slot write whose contents are completed in place at the
    /// end: mid-cycle readers observe the pre-inheritance table (tsc
    /// readers additionally observe the loop's incremental mutations,
    /// but every such re-entry requires a heritage cycle that
    /// getBaseTypes has already cut). An Err unwind retracts the slot —
    /// tsc has no failure mode, so partial tables must not persist.
    fn resolve_object_type_members(
        &mut self,
        ty: TypeId,
        source_type: TypeId,
        source: MembersId,
        type_parameters: &[TypeId],
        type_arguments: &[TypeId],
    ) -> CheckResult2<MembersId> {
        let mut mapper: Option<crate::instantiate::MapperId> = None;
        let mut members: tsrs2_binder::SymbolTable;
        let mut call_signatures: Vec<SignatureId>;
        let mut construct_signatures: Vec<SignatureId>;
        let mut index_infos: Vec<IndexInfo>;
        let range_equal = type_arguments[..type_parameters.len().min(type_arguments.len())]
            == *type_parameters
            && type_arguments.len() >= type_parameters.len();
        let source_symbol = self.tables.type_of(source_type).symbol;
        let mut members_are_live_table = false;
        if range_equal {
            members = match source_symbol {
                Some(symbol) => {
                    members_are_live_table = true;
                    self.get_members_of_symbol(symbol)?
                }
                None => {
                    let declared = self.members_of(source).properties.clone();
                    self.symbol_list_to_table(&declared)
                }
            };
            call_signatures = self.members_of(source).call_signatures.clone();
            construct_signatures = self.members_of(source).construct_signatures.clone();
            index_infos = self.members_of(source).index_infos.clone();
        } else {
            let type_mapper =
                self.create_type_mapper(type_parameters.to_vec(), Some(type_arguments.to_vec()));
            mapper = Some(type_mapper);
            let declared_properties = self.members_of(source).properties.clone();
            members = self.create_instantiated_symbol_table(
                &declared_properties,
                type_mapper,
                /*mapping_this_only*/ type_parameters.len() == 1,
            )?;
            let declared_calls = self.members_of(source).call_signatures.clone();
            call_signatures = self.instantiate_signature_list(&declared_calls, type_mapper)?;
            let declared_constructs = self.members_of(source).construct_signatures.clone();
            construct_signatures =
                self.instantiate_signature_list(&declared_constructs, type_mapper)?;
            let declared_index_infos = self.members_of(source).index_infos.clone();
            index_infos = self.instantiate_index_info_list(&declared_index_infos, type_mapper)?;
        }
        let base_types = self.get_base_types(source_type)?;
        let early_id = if !base_types.is_empty() {
            if members_are_live_table {
                // 57821-57828: copy the declared properties (+ the
                // index symbol) before inheriting — the symbol's own
                // table must not absorb base members.
                let declared_properties = self.members_of(source).properties.clone();
                let mut table = self.symbol_list_to_table(&declared_properties);
                let source_index = source_symbol.and_then(|symbol| {
                    self.symbol_members(symbol)
                        .get(InternalSymbolName::INDEX)
                        .copied()
                });
                if let Some(index_symbol) = source_index {
                    table.insert(InternalSymbolName::INDEX.to_owned(), index_symbol);
                }
                members = table;
            }
            // Early write (57829): partial members become observable.
            let properties = self.get_named_members(&members);
            let id = self.alloc_members(ResolvedMembers {
                members: members.clone(),
                properties,
                call_signatures: call_signatures.clone(),
                construct_signatures: construct_signatures.clone(),
                index_infos: index_infos.clone(),
            });
            self.links
                .set_type_members(self.speculation_depth, ty, LinkSlot::Resolved(id));
            let this_argument = type_arguments.last().copied();
            let inherited = (|state: &mut Self| -> CheckResult2<()> {
                for &base_type in &base_types {
                    let instantiated_base_type = match this_argument {
                        Some(this_argument) => {
                            let instantiated = state.instantiate_type(base_type, mapper)?;
                            state.get_type_with_this_argument(
                                instantiated,
                                Some(this_argument),
                                /*need_apparent_type*/ false,
                            )?
                        }
                        None => base_type,
                    };
                    let base_properties =
                        state.get_properties_of_type_full(instantiated_base_type)?;
                    state.add_inherited_members(&mut members, &base_properties);
                    call_signatures.extend(state.get_signatures_of_type(
                        instantiated_base_type,
                        crate::structural::SignatureKind::Call,
                    )?);
                    construct_signatures.extend(state.get_signatures_of_type(
                        instantiated_base_type,
                        crate::structural::SignatureKind::Construct,
                    )?);
                    let inherited_index_infos =
                        if instantiated_base_type != state.tables.intrinsics.any {
                            state.get_index_infos_of_type(instantiated_base_type)?
                        } else {
                            vec![IndexInfo {
                                key_type: state.tables.intrinsics.string,
                                value_type: state.tables.intrinsics.any,
                                is_readonly: false,
                                declaration: None,
                                components: None,
                            }]
                        };
                    for info in inherited_index_infos {
                        if !index_infos
                            .iter()
                            .any(|existing| existing.key_type == info.key_type)
                        {
                            index_infos.push(info);
                        }
                    }
                }
                Ok(())
            })(self);
            if let Err(err) = inherited {
                self.links.retract_type_members(ty);
                return Err(err);
            }
            Some(id)
        } else {
            None
        };
        let properties = self.get_named_members(&members);
        let resolved = ResolvedMembers {
            members,
            properties,
            call_signatures,
            construct_signatures,
            index_infos,
        };
        match early_id {
            Some(id) => {
                // Final setStructuredTypeMembers (57840): complete the
                // early table in place.
                *self.members_mut(id) = resolved;
                Ok(id)
            }
            None => {
                let id = self.alloc_members(resolved);
                self.links
                    .set_type_members(self.speculation_depth, ty, LinkSlot::Resolved(id));
                Ok(id)
            }
        }
    }

    /// createSymbolTable(symbols) (50128): a table keyed by escaped
    /// name, insertion order preserved.
    fn symbol_list_to_table(&self, symbols: &[SymbolId]) -> tsrs2_binder::SymbolTable {
        let mut table = tsrs2_binder::SymbolTable::default();
        for &symbol in symbols {
            table.insert(self.binder.symbol(symbol).escaped_name.clone(), symbol);
        }
        table
    }

    /// tsc-port: getMembersOfSymbol @6.0.3
    /// tsc-hash: 0f11dedba730036e86b603fd44caf3e18114365b5b104823548dd7fb97466631
    /// tsc-span: _tsc.js:57767-57769
    pub(crate) fn get_members_of_symbol(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<tsrs2_binder::SymbolTable> {
        if self
            .symbol_flags(symbol)
            .intersects(SymbolFlags::LATE_BINDING_CONTAINER)
        {
            return self.get_resolved_members_of_symbol(symbol);
        }
        Ok(self.symbol_members(symbol).clone())
    }

    /// tsc-port: getResolvedMembersOrExportsOfSymbol @6.0.3
    /// tsc-hash: 658bb9b2cdee7a906c4edf27c0be7a544b0be92d2d4bfb14dcc52675429f1304
    /// tsc-span: _tsc.js:57712-57766
    ///
    /// Both resolution kinds route here (is_static selects the
    /// resolvedExports flavor). The EARLY table parks in the links
    /// slot before binding (the 57717 re-entrancy guard) and the
    /// combined table rewrites it; an Err unwind reverts to Vacant.
    /// The JS assignment-declaration and cjsExportMerged blocks are
    /// elided project-wide. An early/late NAME COLLISION would need
    /// mergeSymbol across the tables — escape (5.8 merge surface).
    fn get_resolved_members_or_exports_of_symbol(
        &mut self,
        symbol: SymbolId,
        is_static: bool,
    ) -> CheckResult2<tsrs2_binder::SymbolTable> {
        let cached = if is_static {
            self.links.symbol(symbol).resolved_exports.resolved()
        } else {
            self.links.symbol(symbol).resolved_members.resolved()
        };
        if let Some(resolved) = cached {
            return Ok(resolved);
        }
        let early = if is_static {
            self.binder.symbol(symbol).exports.clone()
        } else {
            self.symbol_members(symbol).clone()
        };
        if is_static {
            self.links.set_symbol_resolved_exports_late_bind(
                self.speculation_depth,
                symbol,
                early.clone(),
            );
        } else {
            self.links.set_symbol_resolved_members_late_bind(
                self.speculation_depth,
                symbol,
                early.clone(),
            );
        }
        // Members whose lateBindMember pre-write happens in THIS frame
        // — an Err unwind must revert their node/lateSymbol memos too,
        // or the retry's memo-hits would silently DROP them from the
        // rebuilt late table (review round #2).
        let mut freshly_bound: Vec<NodeId> = Vec::new();
        let result = (|state: &mut Self,
                       freshly_bound: &mut Vec<NodeId>|
         -> CheckResult2<tsrs2_binder::SymbolTable> {
            let mut late = tsrs2_binder::SymbolTable::default();
            let declarations = state.binder.symbol(symbol).declarations.clone();
            for declaration in declarations {
                for member in state.members_of_declaration(declaration) {
                    if is_static != state.has_static_modifier(member) {
                        continue;
                    }
                    if !state.has_late_bindable_ast_name(member) {
                        continue;
                    }
                    // INTERFACE containers keep the M7-stub containment:
                    // un-containing the lib interfaces (String/Array/
                    // Function carry `[Symbol.iterator]`-family members
                    // at ES2015+) unmasks [FLOW M5], comment-directive
                    // and declare-global-augment divergences corpus-wide
                    // (5.7b review round, FP find). Type literals,
                    // classes and object literals late-bind for real.
                    if state
                        .symbol_flags(symbol)
                        .intersects(SymbolFlags::INTERFACE)
                    {
                        return Err(Unsupported::new(
                            "late-bound INTERFACE members (lib well-known-symbol surface, M7-stub)",
                        ));
                    }
                    let name = state
                        .name_of_named_declaration(member)
                        .expect("late-bindable AST implies a computed name");
                    // The silent pre-flight: checkComputedPropertyName
                    // EMITS resolution misses (2304/2339) — where OUR
                    // resolution diverges from tsc's (declare-global
                    // augments, module band), the emission itself would
                    // be the FP. A chain that does not resolve contains
                    // the container like the old stub — no diagnostic.
                    if !state.computed_name_chain_resolves(name)? {
                        return Err(Unsupported::new(
                            "late-bound member name resolution miss \
                             (declare-global augment / module band, 5.8)",
                        ));
                    }
                    // hasLateBindableName / hasLateBindableIndexSignature
                    // (57635-57642) dispatch on the CHECKED name type:
                    // property-usable → member; string/number/symbol
                    // assignable → index signature; neither → skip
                    // (checkComputedPropertyName memoizes the type).
                    let name_type = state.check_computed_property_name(name)?;
                    if state.property_name_from_type_usable(name_type).is_some() {
                        if state
                            .links
                            .node(member)
                            .resolved_symbol
                            .resolved()
                            .is_none()
                        {
                            freshly_bound.push(member);
                        }
                        state.late_bind_member(symbol, &early, &mut late, member)?;
                    } else {
                        let string_number_symbol = state.tables.intrinsics.string_number_symbol;
                        if state.is_type_assignable_to(name_type, string_number_symbol)? {
                            state.late_bind_index_signature(&early, &mut late, member)?;
                        }
                    }
                }
            }
            // combineSymbolTables (47810): plain inserts when the key
            // sets are disjoint; a collision needs mergeSymbol (5.8).
            let mut resolved = early.clone();
            for (name, member) in late.iter() {
                if resolved.get(name).is_some() {
                    return Err(Unsupported::new(
                        "early/late member table merge (combineSymbolTables → mergeSymbol, 5.8)",
                    ));
                }
                resolved.insert(name.clone(), *member);
            }
            Ok(resolved)
        })(self, &mut freshly_bound);
        match result {
            Ok(resolved) => {
                if is_static {
                    self.links.set_symbol_resolved_exports_late_bind(
                        self.speculation_depth,
                        symbol,
                        resolved.clone(),
                    );
                } else {
                    self.links.set_symbol_resolved_members_late_bind(
                        self.speculation_depth,
                        symbol,
                        resolved.clone(),
                    );
                }
                Ok(resolved)
            }
            Err(err) => {
                if is_static {
                    self.links.revert_symbol_resolved_exports(symbol);
                } else {
                    self.links.revert_symbol_resolved_members(symbol);
                }
                for member in freshly_bound {
                    self.links.revert_node_resolved_symbol_late_bind(member);
                    if let Some(member_symbol) = self.node_symbol(member) {
                        self.links.clear_symbol_late_symbol(member_symbol);
                    }
                }
                Err(err)
            }
        }
    }

    fn get_resolved_members_of_symbol(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<tsrs2_binder::SymbolTable> {
        self.get_resolved_members_or_exports_of_symbol(symbol, /*is_static*/ false)
    }

    /// tsc-port: lateBindMember @6.0.3
    /// tsc-hash: 27f7f740dbb3500e1dc1cd5612f22f37c1e285c4ed662ea8910f8be5044be367
    /// tsc-span: _tsc.js:57662-57693
    ///
    /// The BinaryExpression/element-access declName arms are JS
    /// (expando assignments) — TS members always carry a
    /// ComputedPropertyName. The member's node links.resolvedSymbol
    /// parks its own binder symbol first (re-entrancy guard), then the
    /// late symbol replaces it (the dedicated protocol setter).
    fn late_bind_member(
        &mut self,
        parent: SymbolId,
        early: &tsrs2_binder::SymbolTable,
        late: &mut tsrs2_binder::SymbolTable,
        decl: NodeId,
    ) -> CheckResult2<Option<SymbolId>> {
        let decl_symbol = self
            .node_symbol(decl)
            .expect("the member is expected to have a symbol");
        if let Some(resolved) = self.links.node(decl).resolved_symbol.resolved() {
            return Ok(Some(resolved));
        }
        self.links
            .set_node_resolved_symbol_late_bind(self.speculation_depth, decl, decl_symbol);
        let decl_name = self
            .name_of_named_declaration(decl)
            .expect("late-bindable AST implies a computed name");
        let name_type = self.check_computed_property_name(decl_name)?;
        let Some(member_name) = self.property_name_from_type_usable(name_type) else {
            return Ok(Some(decl_symbol));
        };
        let symbol_flags = self.binder.symbol(decl_symbol).flags;
        let mut late_symbol = match late.get(&member_name) {
            Some(&existing) => existing,
            None => {
                let created = self
                    .binder
                    .create_symbol(SymbolFlags::NONE, member_name.clone());
                self.links.set_symbol_check_flags(
                    self.speculation_depth,
                    created,
                    tsrs2_types::CheckFlags::LATE,
                );
                late.insert(member_name.clone(), created);
                created
            }
        };
        let early_symbol = early.get(&member_name).copied();
        let parent_is_class = self.symbol_flags(parent).intersects(SymbolFlags::CLASS);
        let excluded = get_excluded_symbol_flags(symbol_flags);
        if !parent_is_class && self.binder.symbol(late_symbol).flags.intersects(excluded) {
            // 57676-57681: duplicate late-bound member.
            let declarations: Vec<NodeId> = early_symbol
                .map(|s| self.binder.symbol(s).declarations.clone())
                .unwrap_or_default()
                .into_iter()
                .chain(self.binder.symbol(late_symbol).declarations.clone())
                .collect();
            let display = if !self
                .tables
                .flags_of(name_type)
                .intersects(TypeFlags::UNIQUE_ES_SYMBOL)
            {
                tsrs2_binder::unescape_leading_underscores(&member_name).to_owned()
            } else {
                self.text_of_node(decl_name)?
            };
            for declaration in declarations {
                let error_node = self
                    .name_of_named_declaration(declaration)
                    .unwrap_or(declaration);
                self.error_at(
                    Some(error_node),
                    &diagnostics::Property_0_was_also_declared_here,
                    &[&display],
                );
            }
            self.error_at(
                Some(decl_name),
                &diagnostics::Duplicate_property_0,
                &[&display],
            );
            let fresh = self
                .binder
                .create_symbol(SymbolFlags::NONE, member_name.clone());
            self.links.set_symbol_check_flags(
                self.speculation_depth,
                fresh,
                tsrs2_types::CheckFlags::LATE,
            );
            late.insert(member_name.clone(), fresh);
            late_symbol = fresh;
        }
        self.links
            .set_symbol_name_type(self.speculation_depth, late_symbol, Some(name_type));
        self.add_declaration_to_late_bound_symbol(late_symbol, decl, symbol_flags);
        let existing_parent = self.binder.symbol(late_symbol).parent;
        match existing_parent {
            Some(existing) => debug_assert_eq!(
                existing, parent,
                "Existing symbol parent should match new one"
            ),
            None => self.binder.symbol_mut(late_symbol).parent = Some(parent),
        }
        self.links
            .set_node_resolved_symbol_late_bind(self.speculation_depth, decl, late_symbol);
        Ok(Some(late_symbol))
    }

    /// tsc-port: addDeclarationToLateBoundSymbol @6.0.3
    /// tsc-hash: 5902dcfdc7b047251415cfe2d48d2423eaa85135ceaeeee2427343e3c4716661
    /// tsc-span: _tsc.js:57649-57661
    ///
    /// isReplaceableByMethod is a JS object-literal flag (always
    /// false); setValueDeclaration reduces to the first-Value-wins
    /// write for TS member shapes.
    fn add_declaration_to_late_bound_symbol(
        &mut self,
        late_symbol: SymbolId,
        member: NodeId,
        symbol_flags: SymbolFlags,
    ) {
        let member_symbol = self
            .node_symbol(member)
            .expect("late-bound member has a symbol");
        self.binder.symbol_mut(late_symbol).flags |= symbol_flags;
        self.links
            .set_symbol_late_symbol(self.speculation_depth, member_symbol, late_symbol);
        self.binder
            .symbol_mut(late_symbol)
            .declarations
            .push(member);
        if symbol_flags.intersects(SymbolFlags::VALUE)
            && self.binder.symbol(late_symbol).value_declaration.is_none()
        {
            self.binder.symbol_mut(late_symbol).value_declaration = Some(member);
        }
    }

    /// tsc-port: lateBindIndexSignature @6.0.3
    /// tsc-hash: e4a21cba13d27d2990eb3880e9c33d4ed3b06acc26c66109960ba95799663165
    /// tsc-span: _tsc.js:57694-57711
    ///
    /// cloneSymbol of an early __index (both computed AND declared
    /// index members on one container) is a merge surface — escape;
    /// the pure-late shape allocates the fresh __index symbol. The
    /// consumer (getIndexInfosOfIndexSymbol) still escapes on
    /// non-IndexSignature declarations, so these members stay FN
    /// (honest containment at the read).
    fn late_bind_index_signature(
        &mut self,
        early: &tsrs2_binder::SymbolTable,
        late: &mut tsrs2_binder::SymbolTable,
        decl: NodeId,
    ) -> CheckResult2<()> {
        let index_symbol = match late.get(InternalSymbolName::INDEX) {
            Some(&existing) => existing,
            None => {
                if early.get(InternalSymbolName::INDEX).is_some() {
                    return Err(Unsupported::new(
                        "late index signature over an early __index (cloneSymbol merge, 5.8)",
                    ));
                }
                let created = self
                    .binder
                    .create_symbol(SymbolFlags::NONE, InternalSymbolName::INDEX.to_owned());
                self.links.set_symbol_check_flags(
                    self.speculation_depth,
                    created,
                    tsrs2_types::CheckFlags::LATE,
                );
                late.insert(InternalSymbolName::INDEX.to_owned(), created);
                created
            }
        };
        self.binder.symbol_mut(index_symbol).declarations.push(decl);
        Ok(())
    }

    /// The static `resolvedExports` kind — getExportsOfSymbol's
    /// late-binding-container route. Module symbols need
    /// getExportsOfModuleWorker's export-star walk (5.8).
    fn get_exports_of_symbol(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<tsrs2_binder::SymbolTable> {
        if self.symbol_flags(symbol).intersects(SymbolFlags::MODULE) {
            return Err(Unsupported::new(
                "module exports (getExportsOfModuleWorker export-star, M4 5.8)",
            ));
        }
        if !self
            .symbol_flags(symbol)
            .intersects(SymbolFlags::LATE_BINDING_CONTAINER)
        {
            return Ok(self.binder.symbol(symbol).exports.clone());
        }
        self.get_resolved_members_or_exports_of_symbol(symbol, /*is_static*/ true)
    }

    /// getMembersOfDeclaration (19010-ish): the member lists a
    /// late-binding container declaration carries.
    fn members_of_declaration(&self, declaration: NodeId) -> Vec<NodeId> {
        match self.data_of(declaration) {
            NodeData::InterfaceDeclaration(data) => self.nodes_of(data.members),
            NodeData::ClassDeclaration(data) => self.nodes_of(data.members),
            NodeData::ClassExpression(data) => self.nodes_of(data.members),
            NodeData::TypeLiteral(data) => self.nodes_of(data.members),
            NodeData::ObjectLiteralExpression(data) => self.nodes_of(data.properties),
            _ => Vec::new(),
        }
    }

    /// hasLateBindableName's AST half (isLateBindableAST 57622-57628):
    /// a computed name over an entity-name expression. The TYPE half
    /// (property-name vs index-signature usability) dispatches at the
    /// late-binding loop.
    fn has_late_bindable_ast_name(&self, member: NodeId) -> bool {
        let name = match self.data_of(member) {
            NodeData::PropertySignature(data) => data.name,
            NodeData::PropertyDeclaration(data) => data.name,
            NodeData::MethodSignature(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            NodeData::PropertyAssignment(data) => data.name,
            NodeData::ShorthandPropertyAssignment(data) => data.name,
            _ => None,
        };
        let Some(name) = name else {
            return false;
        };
        let NodeData::ComputedPropertyName(data) = self.data_of(name) else {
            return false;
        };
        data.expression
            .is_some_and(|expression| self.is_entity_name_expression(expression))
    }

    /// The silent pre-flight over a computed name's entity chain
    /// (Identifier / dotted PropertyAccess): the leftmost name
    /// resolves with NO diagnostic and each property hop looks up
    /// silently. False = our resolution diverges from tsc's (a
    /// declare-global augment / module-band member we cannot merge) —
    /// the late-binding loop then contains WITHOUT emitting the miss
    /// checkComputedPropertyName would fabricate.
    fn computed_name_chain_resolves(&mut self, name: NodeId) -> CheckResult2<bool> {
        let NodeData::ComputedPropertyName(data) = self.data_of(name) else {
            return Ok(false);
        };
        let Some(expression) = data.expression else {
            return Ok(false);
        };
        let mut hops: Vec<String> = Vec::new();
        let mut current = expression;
        while let NodeData::PropertyAccessExpression(access) = self.data_of(current) {
            let Some(prop) = access
                .name
                .and_then(|n| self.identifier_text_of(n))
                .map(str::to_owned)
            else {
                return Ok(false);
            };
            hops.push(prop);
            let Some(inner) = access.expression else {
                return Ok(false);
            };
            current = inner;
        }
        if self.kind_of(current) != SyntaxKind::Identifier {
            return Ok(false);
        }
        let Some(root_text) = self.identifier_text_of(current).map(str::to_owned) else {
            return Ok(false);
        };
        let Some(root) = self.resolve_name(
            Some(current),
            &root_text,
            SymbolFlags::VALUE,
            None,
            false,
            false,
        ) else {
            return Ok(false);
        };
        let mut ty = self.get_type_of_symbol(root)?;
        for hop in hops.iter().rev() {
            let Some(prop) = self.get_property_of_type_full(ty, hop)? else {
                return Ok(false);
            };
            ty = self.get_type_of_symbol(prop)?;
        }
        Ok(true)
    }

    pub(crate) fn has_static_modifier(&self, member: NodeId) -> bool {
        let modifiers = match self.data_of(member) {
            NodeData::PropertyDeclaration(data) => data.modifiers,
            NodeData::MethodDeclaration(data) => data.modifiers,
            NodeData::GetAccessor(data) => data.modifiers,
            NodeData::SetAccessor(data) => data.modifiers,
            _ => None,
        };
        self.nodes_of(modifiers)
            .iter()
            .any(|&modifier| self.kind_of(modifier) == SyntaxKind::StaticKeyword)
    }

    // ---- base types ----

    /// tsc-port: getBaseTypes @6.0.3
    /// tsc-hash: e943ec4fd5c8bdba4a95b723e41065f4d56060b919a718c9405ee6de21bb62df
    /// tsc-span: _tsc.js:57218-57247
    ///
    /// A mid-cycle re-entry (pushTypeResolution false) freezes
    /// baseTypesResolved with whatever partial list the outer frame has
    /// built — ported exactly; the outer frame's pop then reports 2310
    /// per class/interface declaration. Class bases (resolveBaseTypesOfClass
    /// — base constructor types) are 5.3e.
    pub(crate) fn get_base_types(&mut self, ty: TypeId) -> CheckResult2<Vec<TypeId>> {
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::CLASS_OR_INTERFACE | ObjectFlags::REFERENCE)
        {
            return Ok(Vec::new());
        }
        if !self.links.ty(ty).base_types_resolved {
            if self.push_type_resolution(
                crate::state::ResolutionTarget::Type(ty),
                tsrs2_types::TypeSystemPropertyName::RESOLVED_BASE_TYPES,
            ) {
                let resolved = (|state: &mut Self| -> CheckResult2<()> {
                    if state
                        .tables
                        .object_flags_of(ty)
                        .intersects(ObjectFlags::TUPLE)
                    {
                        let base = state.get_tuple_base_type(ty)?;
                        state.links.set_type_resolved_base_types(
                            state.speculation_depth,
                            ty,
                            vec![base],
                        );
                    } else {
                        let symbol = state
                            .tables
                            .type_of(ty)
                            .symbol
                            .expect("type must be class or interface");
                        let flags = state.symbol_flags(symbol);
                        if flags.intersects(SymbolFlags::CLASS) {
                            state.resolve_base_types_of_class(ty)?;
                        }
                        if flags.intersects(SymbolFlags::INTERFACE) {
                            state.resolve_base_types_of_interface(ty, symbol)?;
                        }
                        assert!(
                            flags.intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE),
                            "type must be class or interface"
                        );
                    }
                    Ok(())
                })(self);
                if let Err(err) = resolved {
                    self.pop_type_resolution();
                    return Err(err);
                }
                if !self.pop_type_resolution() {
                    let symbol = self.tables.type_of(ty).symbol;
                    let declarations = symbol
                        .map(|symbol| self.binder.symbol(symbol).declarations.clone())
                        .unwrap_or_default();
                    for declaration in declarations {
                        if matches!(
                            self.kind_of(declaration),
                            SyntaxKind::ClassDeclaration | SyntaxKind::InterfaceDeclaration
                        ) {
                            self.report_circular_base_type(declaration, ty);
                        }
                    }
                }
            }
            self.links
                .set_type_base_types_resolved(self.speculation_depth, ty);
        }
        Ok(self
            .links
            .ty(ty)
            .resolved_base_types
            .clone()
            .unwrap_or_default())
    }

    /// tsc-port: getTupleBaseType @6.0.3
    /// tsc-hash: b528c842896a958ebf1d8e80b191f9ab6e92a2903a555b6c21b4de6133d03dcf
    /// tsc-span: _tsc.js:57248-57251
    fn get_tuple_base_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let TypeData::TupleTarget(data) = self.tables.type_of(ty).data.clone() else {
            unreachable!("TUPLE object flag implies a tuple target");
        };
        let mut element_types = Vec::with_capacity(data.type_parameters.len());
        for (i, &tp) in data.type_parameters.iter().enumerate() {
            if data.element_flags[i].intersects(ElementFlags::VARIADIC) {
                element_types.push(self.get_indexed_access_type(
                    tp,
                    self.tables.intrinsics.number,
                    tsrs2_types::AccessFlags::NONE,
                    None,
                    None,
                    None,
                )?);
            } else {
                element_types.push(tp);
            }
        }
        let union = self.get_union_type_ex(&element_types, UnionReduction::Literal)?;
        self.create_array_type(union, data.readonly)
    }

    /// tsc-port: resolveBaseTypesOfInterface @6.0.3
    /// tsc-hash: d0faa71bac757aefc2c1d0fc974e8661aa7c45e990e5d5902e44753c58d6dc3e
    /// tsc-span: _tsc.js:57319-57345
    fn resolve_base_types_of_interface(
        &mut self,
        ty: TypeId,
        symbol: SymbolId,
    ) -> CheckResult2<()> {
        let mut resolved = self
            .links
            .ty(ty)
            .resolved_base_types
            .clone()
            .unwrap_or_default();
        self.links
            .set_type_resolved_base_types(self.speculation_depth, ty, resolved.clone());
        let declarations = self.binder.symbol(symbol).declarations.clone();
        for declaration in declarations {
            if self.kind_of(declaration) != SyntaxKind::InterfaceDeclaration {
                continue;
            }
            for node in self.interface_base_type_nodes(declaration) {
                let from_node = self.get_type_from_type_node(node)?;
                let base_type = self.get_reduced_type(from_node)?;
                if base_type == self.tables.intrinsics.error {
                    continue;
                }
                if self.is_valid_base_type(base_type)? {
                    if ty != base_type && !self.has_base_type(base_type, ty)? {
                        resolved.push(base_type);
                        self.links.set_type_resolved_base_types(
                            self.speculation_depth,
                            ty,
                            resolved.clone(),
                        );
                    } else {
                        self.report_circular_base_type(declaration, ty);
                    }
                } else {
                    self.error_at(
                        Some(node),
                        &diagnostics::An_interface_can_only_extend_an_object_type_or_intersection_of_object_types_with_statically_known_members,
                        &[],
                    );
                }
            }
        }
        Ok(())
    }

    /// tsc-port: isValidBaseType @6.0.3
    /// tsc-hash: 4efebd5c35e02f1f53bfcd54aa9955ee5eb856bb8adf1f3eb97fd73ea5c2e397
    /// tsc-span: _tsc.js:57310-57318
    ///
    /// isGenericMappedType is constant-false before M8 mapped types.
    fn is_valid_base_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::TYPE_PARAMETER) {
            if let Some(constraint) = self.get_base_constraint_of_type(ty)? {
                return self.is_valid_base_type(constraint);
            }
        }
        if flags.intersects(TypeFlags::OBJECT | TypeFlags::NON_PRIMITIVE | TypeFlags::ANY) {
            return Ok(true);
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } = self.tables.type_of(ty).data.clone() else {
                unreachable!("intersection flag implies intersection data");
            };
            for t in types.iter() {
                if !self.is_valid_base_type(*t)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// tsc-port: hasBaseType @6.0.3
    /// tsc-hash: 4be36907403570c20f53afc9305703585ad832eac42b2d7bba4f94d43f95c211
    /// tsc-span: _tsc.js:56996-57007
    pub(crate) fn has_base_type(&mut self, ty: TypeId, check_base: TypeId) -> CheckResult2<bool> {
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::CLASS_OR_INTERFACE | ObjectFlags::REFERENCE)
        {
            let target = self.get_target_type(ty);
            if target == check_base {
                return Ok(true);
            }
            for base in self.get_base_types(target)? {
                if self.has_base_type(base, check_base)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if self.tables.flags_of(ty).intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } = self.tables.type_of(ty).data.clone() else {
                unreachable!("intersection flag implies intersection data");
            };
            for t in types.iter() {
                if self.has_base_type(*t, check_base)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// tsc getTargetType (56993-56995).
    pub(crate) fn get_target_type(&self, ty: TypeId) -> TypeId {
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
        {
            self.tables.reference_target(ty)
        } else {
            ty
        }
    }

    /// tsc-port: reportCircularBaseType @6.0.3
    /// tsc-hash: 92e019c6189cd151d9c754edb3112a3909753f31f356bd541e55b6423eaf42dd
    /// tsc-span: _tsc.js:57210-57217
    fn report_circular_base_type(&mut self, node: NodeId, ty: TypeId) {
        let display = self.generic_type_display(ty);
        self.error_at(
            Some(node),
            &diagnostics::Type_0_recursively_references_itself_as_a_base_type,
            &[&display],
        );
    }

    /// tsc-port: getTypeWithThisArgument @6.0.3
    /// tsc-hash: 71e6ed1bfe5ec5f0a9375446ad398c7047d86c374968622e3df5deeefcbc123e
    /// tsc-span: _tsc.js:57785-57795
    ///
    /// needApparentType=true routes non-reference constituents through
    /// the full getApparentType chain (the intersection-apparent path).
    pub(crate) fn get_type_with_this_argument(
        &mut self,
        ty: TypeId,
        this_argument: Option<TypeId>,
        need_apparent_type: bool,
    ) -> CheckResult2<TypeId> {
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
        {
            let target = self.tables.reference_target(ty);
            let target_parameters: Vec<TypeId> = match &self.tables.type_of(target).data {
                TypeData::GenericType {
                    type_parameters, ..
                } => type_parameters.to_vec(),
                TypeData::TupleTarget(data) => data.type_parameters.to_vec(),
                _ => Vec::new(),
            };
            let type_arguments = self.get_type_arguments(ty)?;
            return if target_parameters.len() == type_arguments.len() {
                // PLAIN createTypeReference (57789) — NOT the
                // normalized path: the this slot appends past the
                // element list on the SAME target (a tuple target's
                // arity/flags must not change here). Downstream
                // elementFlags[i] reads see `undefined` in tsc, whose
                // bitwise coercion is 0 — the normalization twin
                // mirrors that with element_flag_at's zero flags when
                // INSTANTIATION later re-normalizes this reference.
                let this_type = match &self.tables.type_of(target).data {
                    TypeData::GenericType { this_type, .. } => *this_type,
                    TypeData::TupleTarget(data) => data.this_type,
                    _ => unreachable!("references target generic or tuple targets"),
                };
                let mut arguments = type_arguments;
                arguments.push(this_argument.unwrap_or(this_type));
                Ok(self.tables.create_type_reference(target, &arguments))
            } else {
                Ok(ty)
            };
        }
        if self.tables.flags_of(ty).intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } = self.tables.type_of(ty).data.clone() else {
                unreachable!("intersection flag implies intersection data");
            };
            let mut new_types = Vec::with_capacity(types.len());
            let mut changed = false;
            for &t in types.iter() {
                let mapped =
                    self.get_type_with_this_argument(t, this_argument, need_apparent_type)?;
                changed |= mapped != t;
                new_types.push(mapped);
            }
            return if changed {
                self.get_intersection_type(&new_types, tsrs2_types::IntersectionFlags::NONE)
            } else {
                Ok(ty)
            };
        }
        if need_apparent_type {
            return self.get_apparent_type(ty);
        }
        Ok(ty)
    }

    // ---- class bases (5.3e) ----

    /// tsc-port: getBaseTypeNodeOfClass @6.0.3
    /// tsc-hash: e309e0dd7cc0f96feefd2940dc8f124c609d09c48e1bd4cef7b477d4ded13004
    /// tsc-span: _tsc.js:57132-57135
    pub(crate) fn get_base_type_node_of_class(&mut self, ty: TypeId) -> Option<NodeId> {
        let symbol = self.tables.type_of(ty).symbol?;
        let declarations = self.binder.symbol(symbol).declarations.clone();
        for declaration in declarations {
            let heritage = match self.data_of(declaration) {
                NodeData::ClassDeclaration(data) => data.heritage_clauses,
                NodeData::ClassExpression(data) => data.heritage_clauses,
                _ => continue,
            };
            for clause in self.nodes_of(heritage) {
                if self.heritage_clause_is_extends(clause) {
                    let NodeData::HeritageClause(clause_data) = self.data_of(clause) else {
                        continue;
                    };
                    return self.nodes_of(clause_data.types).first().copied();
                }
            }
        }
        None
    }

    /// The checkExpression read for extends clauses (tsc 57156 calls
    /// plain checkExpression): mixin factory calls and other
    /// non-entity-name shapes ride the full expression checker — the
    /// 5.5-era entity-name-only slice is retired.
    fn check_base_type_expression(&mut self, expression: NodeId) -> CheckResult2<TypeId> {
        self.check_expression(expression, tsrs2_types::CheckMode::NORMAL)
    }

    /// tsc-port: getBaseConstructorTypeOfClass @6.0.3
    /// tsc-hash: c549cc3f19bfdfe04b2ffb3048bfcd3bb3009d45bda972944127fd55a2dcb800
    /// tsc-span: _tsc.js:57146-57190
    ///
    /// The JSDoc extended-tag double-check (57160-57163) is elided;
    /// the 2507 not-a-constructor error renders the base type through
    /// typeToString — that arm unwinds as Unsupported (display T2/M8).
    pub(crate) fn get_base_constructor_type_of_class(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<TypeId> {
        if let Some(resolved) = self.links.ty(ty).resolved_base_constructor_type.resolved() {
            return Ok(resolved);
        }
        let Some(base_type_node) = self.get_base_type_node_of_class(ty) else {
            let undefined = self.tables.intrinsics.undefined;
            self.links.set_type_resolved_base_constructor_type(
                self.speculation_depth,
                ty,
                undefined,
            );
            return Ok(undefined);
        };
        if !self.push_type_resolution(
            crate::state::ResolutionTarget::Type(ty),
            tsrs2_types::TypeSystemPropertyName::RESOLVED_BASE_CONSTRUCTOR_TYPE,
        ) {
            return Ok(self.tables.intrinsics.error);
        }
        let computed = (|state: &mut Self| -> CheckResult2<TypeId> {
            let NodeData::ExpressionWithTypeArguments(data) = state.data_of(base_type_node) else {
                unreachable!("heritage clause types are ExpressionWithTypeArguments");
            };
            let expression = data
                .expression
                .ok_or_else(|| Unsupported::new("extends clause with missing expression"))?;
            let base_constructor_type = state.check_base_type_expression(expression)?;
            if state
                .tables
                .flags_of(base_constructor_type)
                .intersects(TypeFlags::OBJECT | TypeFlags::INTERSECTION)
            {
                state.resolve_structured_type_members(base_constructor_type)?;
            }
            Ok(base_constructor_type)
        })(self);
        let base_constructor_type = match computed {
            Ok(resolved) => resolved,
            Err(err) => {
                self.pop_type_resolution();
                return Err(err);
            }
        };
        if !self.pop_type_resolution() {
            let symbol = self
                .tables
                .type_of(ty)
                .symbol
                .expect("class types carry their symbol");
            let declaration = self.binder.symbol(symbol).value_declaration;
            let name = self.symbol_display_name(symbol);
            self.error_at(
                declaration,
                &diagnostics::_0_is_referenced_directly_or_indirectly_in_its_own_base_expression,
                &[&name],
            );
            let error = self.tables.intrinsics.error;
            if self
                .links
                .ty(ty)
                .resolved_base_constructor_type
                .resolved()
                .is_none()
            {
                self.links.set_type_resolved_base_constructor_type(
                    self.speculation_depth,
                    ty,
                    error,
                );
            }
            return Ok(self
                .links
                .ty(ty)
                .resolved_base_constructor_type
                .resolved()
                .expect("just filled"));
        }
        let null_widening = self.tables.intrinsics.null;
        if !self
            .tables
            .flags_of(base_constructor_type)
            .intersects(TypeFlags::ANY)
            && base_constructor_type != null_widening
            && !self.is_constructor_type(base_constructor_type)?
        {
            // 57170-57185: 2507 + the type-parameter constraint
            // elaboration — both render through typeToString.
            return Err(Unsupported::new(
                "Type_0_is_not_a_constructor_function_type display (2507, T2/M8)",
            ));
        }
        if self
            .links
            .ty(ty)
            .resolved_base_constructor_type
            .resolved()
            .is_none()
        {
            self.links.set_type_resolved_base_constructor_type(
                self.speculation_depth,
                ty,
                base_constructor_type,
            );
        }
        Ok(self
            .links
            .ty(ty)
            .resolved_base_constructor_type
            .resolved()
            .expect("just filled"))
    }

    /// tsc-port: isConstructorType @6.0.3
    /// tsc-hash: 246d9586e6eb03f6969ff17e449006f018091ef62e8ff4ca713648fd174be9e5
    /// tsc-span: _tsc.js:57122-57131
    fn is_constructor_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if !self
            .get_signatures_of_type(ty, crate::structural::SignatureKind::Construct)?
            .is_empty()
        {
            return Ok(true);
        }
        if self
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::TYPE_VARIABLE)
        {
            if let Some(constraint) = self.get_base_constraint_of_type(ty)? {
                return self.is_mixin_constructor_type(constraint);
            }
        }
        Ok(false)
    }

    /// tsc-port: resolveBaseTypesOfClass @6.0.3
    /// tsc-hash: 0b5de636d21d6bcdd2c80a6df7db47bcd7cfcf5407d6c0911d32bda948ee0fe7
    /// tsc-span: _tsc.js:57252-57291
    ///
    /// The resolvingEmptyArray sentinel is the entry write; the
    /// members reset at the tail (57297-57298) retracts a members
    /// table that resolved mid-flight against the empty sentinel. The
    /// 2508 no-base-constructor-type-arguments and 2509 invalid-base
    /// errors render through typeToString — those arms unwind as
    /// Unsupported.
    fn resolve_base_types_of_class(&mut self, ty: TypeId) -> CheckResult2<()> {
        self.links
            .set_type_resolved_base_types(self.speculation_depth, ty, Vec::new());
        let raw_base_constructor = self.get_base_constructor_type_of_class(ty)?;
        let base_constructor_type = self.get_apparent_type(raw_base_constructor)?;
        if !self
            .tables
            .flags_of(base_constructor_type)
            .intersects(TypeFlags::OBJECT | TypeFlags::INTERSECTION | TypeFlags::ANY)
        {
            return Ok(());
        }
        let base_type_node = self
            .get_base_type_node_of_class(ty)
            .expect("a base constructor implies an extends clause");
        let original_base_type = match self.tables.type_of(base_constructor_type).symbol {
            Some(symbol) => Some(self.get_declared_type_of_symbol_slice(symbol)?),
            None => None,
        };
        let base_type;
        let base_symbol = self.tables.type_of(base_constructor_type).symbol;
        let base_is_applied_class = match (base_symbol, original_base_type) {
            (Some(symbol), Some(original))
                if self.symbol_flags(symbol).intersects(SymbolFlags::CLASS) =>
            {
                self.are_all_outer_type_parameters_applied(original)?
            }
            _ => false,
        };
        if base_is_applied_class {
            base_type = self.get_type_from_class_or_interface_reference(
                base_type_node,
                base_symbol.expect("checked above"),
            )?;
        } else if self
            .tables
            .flags_of(base_constructor_type)
            .intersects(TypeFlags::ANY)
        {
            base_type = base_constructor_type;
        } else {
            // 57266-57272: constructor-signature selection by type
            // argument count.
            let constructors = self.get_instantiated_constructors_for_type_arguments(
                base_constructor_type,
                base_type_node,
            )?;
            if constructors.is_empty() {
                self.error_at(
                    Some(base_type_node),
                    &diagnostics::No_base_constructor_has_the_specified_number_of_type_arguments,
                    &[],
                );
                return Ok(());
            }
            base_type = self.get_return_type_of_signature(constructors[0])?;
        }
        if base_type == self.tables.intrinsics.error {
            return Ok(());
        }
        let reduced_base_type = self.get_reduced_type(base_type)?;
        if !self.is_valid_base_type(reduced_base_type)? {
            return Err(Unsupported::new(
                "Base_constructor_return_type_0_is_not_an_object_type display (2509, T2/M8)",
            ));
        }
        if ty == reduced_base_type || self.has_base_type(reduced_base_type, ty)? {
            let symbol = self
                .tables
                .type_of(ty)
                .symbol
                .expect("class types carry their symbol");
            let declaration = self.binder.symbol(symbol).value_declaration;
            let display = self.generic_type_display(ty);
            self.error_at(
                declaration,
                &diagnostics::Type_0_recursively_references_itself_as_a_base_type,
                &[&display],
            );
            return Ok(());
        }
        // 57297-57298: members resolved against the mid-flight empty
        // sentinel recompute with the base in place.
        if self.links.ty(ty).resolved_members.resolved().is_some() {
            self.links.retract_type_members(ty);
        }
        self.links.set_type_resolved_base_types(
            self.speculation_depth,
            ty,
            vec![reduced_base_type],
        );
        Ok(())
    }

    /// tsc-port: tryGetDeclaredTypeOfSymbol @6.0.3
    /// tsc-hash: 28a2c468c08ad14478832fbe5bbeaa107945fc9314bbf768156d2668101141af
    /// tsc-span: _tsc.js:57505-57525
    ///
    /// Covers the getDeclaredTypeOfSymbol wrapper too (57502-57504):
    /// a symbol matching no arm — e.g. a TypeLiteral in mixin base
    /// position — is errorType, not a failure. The Alias arm needs
    /// resolveAlias (import semantics, 5.8) and stays an escape.
    pub(crate) fn get_declared_type_of_symbol_slice(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<TypeId> {
        let flags = self.symbol_flags(symbol);
        if flags.intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE) {
            return self.get_declared_type_of_class_or_interface(symbol);
        }
        if flags.intersects(SymbolFlags::TYPE_ALIAS) {
            return self.get_declared_type_of_type_alias(symbol);
        }
        if flags.intersects(SymbolFlags::TYPE_PARAMETER) {
            return Ok(self.get_declared_type_of_type_parameter(symbol));
        }
        if flags.intersects(SymbolFlags::ENUM) {
            return self.get_declared_type_of_enum(symbol);
        }
        if flags.intersects(SymbolFlags::ENUM_MEMBER) {
            return self.get_declared_type_of_enum_member(symbol);
        }
        if flags.intersects(SymbolFlags::ALIAS) {
            return Err(Unsupported::new(
                "getDeclaredTypeOfAlias (resolveAlias, 5.8 modules)",
            ));
        }
        Ok(self.tables.intrinsics.error)
    }

    /// tsc-port: areAllOuterTypeParametersApplied @6.0.3
    /// tsc-hash: ff7b7190a50dfe9f5f4476721e4151a40231bc66f2e503f4e687f7d1d030eda4
    /// tsc-span: _tsc.js:57292-57299
    fn are_all_outer_type_parameters_applied(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let (outer_count, type_parameters) = match &self.tables.type_of(ty).data {
            TypeData::GenericType {
                type_parameters,
                outer_type_parameter_count,
                ..
            } => (*outer_type_parameter_count, type_parameters.to_vec()),
            _ => return Ok(true),
        };
        if outer_count == 0 {
            return Ok(true);
        }
        let last = outer_count - 1;
        let type_arguments = self.get_type_arguments(ty)?;
        Ok(self.tables.type_of(type_parameters[last]).symbol
            != self.tables.type_of(type_arguments[last]).symbol)
    }

    /// tsc-port: getConstructorsForTypeArguments @6.0.3
    /// tsc-hash: 60d15d3dba29ddf3da5216985843e8d47fffa7f5dd580c60a999eb48a8f00ccc
    /// tsc-span: _tsc.js:57136-57140
    ///
    /// tsc-port: getInstantiatedConstructorsForTypeArguments @6.0.3
    /// tsc-hash: 459642b770f0c17bad05b48f8b92b89e167d4cb27c16608acdcdf8f8cdddb358
    /// tsc-span: _tsc.js:57141-57145
    fn get_instantiated_constructors_for_type_arguments(
        &mut self,
        ty: TypeId,
        node: NodeId,
    ) -> CheckResult2<Vec<SignatureId>> {
        let argument_nodes = match self.data_of(node) {
            NodeData::ExpressionWithTypeArguments(data) => self.nodes_of(data.type_arguments),
            NodeData::TypeReference(data) => self.nodes_of(data.type_arguments),
            _ => Vec::new(),
        };
        let type_arg_count = argument_nodes.len();
        let all = self.get_signatures_of_type(ty, crate::structural::SignatureKind::Construct)?;
        let mut signatures = Vec::new();
        for signature in all {
            let type_parameters = self.signature_of(signature).type_parameters.clone();
            let min = self.get_min_type_argument_count(type_parameters.as_deref());
            let max = type_parameters.as_ref().map_or(0, Vec::len);
            if type_arg_count >= min && type_arg_count <= max {
                signatures.push(signature);
            }
        }
        let mut type_arguments = Vec::with_capacity(argument_nodes.len());
        for argument in argument_nodes {
            type_arguments.push(self.get_type_from_type_node(argument)?);
        }
        let mut result = Vec::with_capacity(signatures.len());
        for signature in signatures {
            let is_generic = self
                .signature_of(signature)
                .type_parameters
                .as_ref()
                .is_some_and(|params| !params.is_empty());
            result.push(if is_generic {
                self.get_signature_instantiation(
                    signature,
                    Some(&type_arguments),
                    /*is_javascript*/ false,
                    None,
                )?
            } else {
                signature
            });
        }
        Ok(result)
    }

    /// tsc-port: getDefaultConstructSignatures @6.0.3
    /// tsc-hash: bc0610c29c6b19cb9c38f77780b74b34dc555c7696df12c218a82eaf3ba5ea06
    /// tsc-span: _tsc.js:57961-57998
    fn get_default_construct_signatures(
        &mut self,
        class_type: TypeId,
    ) -> CheckResult2<Vec<SignatureId>> {
        let base_constructor_type = self.get_base_constructor_type_of_class(class_type)?;
        let base_signatures = self.get_signatures_of_type(
            base_constructor_type,
            crate::structural::SignatureKind::Construct,
        )?;
        let symbol = self
            .tables
            .type_of(class_type)
            .symbol
            .expect("class types carry their symbol");
        let declaration = self.class_like_declaration_of_symbol(symbol);
        let is_abstract = declaration.is_some_and(|declaration| {
            let modifiers = match self.data_of(declaration) {
                NodeData::ClassDeclaration(data) => data.modifiers,
                NodeData::ClassExpression(data) => data.modifiers,
                _ => None,
            };
            self.nodes_of(modifiers)
                .iter()
                .any(|&modifier| self.kind_of(modifier) == SyntaxKind::AbstractKeyword)
        });
        let local_type_parameters: Option<Vec<TypeId>> = match &self.tables.type_of(class_type).data
        {
            TypeData::GenericType {
                type_parameters,
                outer_type_parameter_count,
                ..
            } if type_parameters.len() > *outer_type_parameter_count => {
                Some(type_parameters[*outer_type_parameter_count..].to_vec())
            }
            _ => None,
        };
        if base_signatures.is_empty() {
            let signature = Signature {
                declaration: None,
                flags: if is_abstract {
                    tsrs2_types::SignatureFlags::ABSTRACT
                } else {
                    tsrs2_types::SignatureFlags::from_bits(0)
                },
                type_parameters: local_type_parameters,
                parameters: Vec::new(),
                this_parameter: None,
                min_argument_count: 0,
                resolved_return_type: LinkSlot::Resolved(class_type),
                from_method: false,
                target: None,
                mapper: None,
                instantiations: std::collections::HashMap::new(),
                erased_signature_cache: None,
                composite_kind: None,
                composite_signatures: None,
                optional_call_signature_cache: (None, None),
            };
            return Ok(vec![self.alloc_signature(signature)]);
        }
        let base_type_node = self
            .get_base_type_node_of_class(class_type)
            .expect("base signatures imply an extends clause");
        let argument_nodes = match self.data_of(base_type_node) {
            NodeData::ExpressionWithTypeArguments(data) => self.nodes_of(data.type_arguments),
            _ => Vec::new(),
        };
        let mut type_arguments = Vec::with_capacity(argument_nodes.len());
        for argument in argument_nodes {
            type_arguments.push(self.get_type_from_type_node(argument)?);
        }
        let type_arg_count = type_arguments.len();
        let mut result = Vec::new();
        for base_signature in base_signatures {
            let base_type_parameters = self.signature_of(base_signature).type_parameters.clone();
            let min = self.get_min_type_argument_count(base_type_parameters.as_deref());
            let max = base_type_parameters.as_ref().map_or(0, Vec::len);
            if type_arg_count < min || type_arg_count > max {
                continue;
            }
            let signature = if max > 0 {
                let filled = self
                    .fill_missing_type_arguments(
                        Some(&type_arguments),
                        base_type_parameters.as_deref(),
                        min,
                        /*is_javascript*/ false,
                    )?
                    .unwrap_or_default();
                self.create_signature_instantiation(base_signature, Some(&filled))?
            } else {
                self.clone_signature(base_signature)
            };
            let data = self.signature_mut(signature);
            data.type_parameters = local_type_parameters.clone();
            data.resolved_return_type = LinkSlot::Resolved(class_type);
            data.flags = if is_abstract {
                tsrs2_types::SignatureFlags::from_bits(
                    data.flags.bits() | tsrs2_types::SignatureFlags::ABSTRACT.bits(),
                )
            } else {
                tsrs2_types::SignatureFlags::from_bits(
                    data.flags.bits() & !tsrs2_types::SignatureFlags::ABSTRACT.bits(),
                )
            };
            result.push(signature);
        }
        Ok(result)
    }

    /// getClassLikeDeclarationOfSymbol (14400-region).
    fn class_like_declaration_of_symbol(&self, symbol: SymbolId) -> Option<NodeId> {
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| {
                matches!(
                    self.kind_of(declaration),
                    SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                )
            })
    }

    /// tsc-port: getBaseTypeVariableOfClass @6.0.3
    /// tsc-hash: 4c17d2c29383954876ca8e8b980b1f4ea472d166adcbde14083b75ccfab8bca3
    /// tsc-span: _tsc.js:56804-56807
    fn get_base_type_variable_of_class(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<Option<TypeId>> {
        let class_type = self.get_declared_type_of_class_or_interface(symbol)?;
        let base_constructor_type = self.get_base_constructor_type_of_class(class_type)?;
        let flags = self.tables.flags_of(base_constructor_type);
        if flags.intersects(TypeFlags::TYPE_VARIABLE) {
            return Ok(Some(base_constructor_type));
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } =
                self.tables.type_of(base_constructor_type).data.clone()
            else {
                unreachable!("intersection flag implies intersection data");
            };
            return Ok(types
                .iter()
                .copied()
                .find(|&t| self.tables.flags_of(t).intersects(TypeFlags::TYPE_VARIABLE)));
        }
        Ok(None)
    }

    /// tsc-port: getTypeOfAccessors @6.0.3
    /// tsc-hash: 02e993d3e444bf2bef90e03977fbd24eb389b56578fb5a170fa8ae14660ba119
    /// tsc-span: _tsc.js:56746-56786
    ///
    /// Un-annotated getter bodies infer via getReturnTypeFromBody
    /// (live since 5.5f); auto-accessor property declarations ride the
    /// `accessor` modifier (M7 class checking) — that arm escapes; the
    /// annotated slice plus the noImplicitAny arms are live.
    fn get_type_of_accessors(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.symbol(symbol).type_of_symbol.resolved() {
            return Ok(cached);
        }
        if !self.push_type_resolution(
            crate::state::ResolutionTarget::Symbol(symbol),
            tsrs2_types::TypeSystemPropertyName::TYPE,
        ) {
            return Ok(self.tables.intrinsics.error);
        }
        let getter = self.declaration_of_kind(symbol, SyntaxKind::GetAccessor);
        let setter = self.declaration_of_kind(symbol, SyntaxKind::SetAccessor);
        let computed = (|state: &mut Self| -> CheckResult2<Option<TypeId>> {
            let getter_annotation = state.annotated_accessor_type_node(getter);
            let setter_annotation = state.annotated_accessor_type_node(setter);
            if let Some(annotation) = getter_annotation.or(setter_annotation) {
                return Ok(Some(state.get_type_from_type_node(annotation)?));
            }
            if let Some(getter) = getter {
                let body = match state.data_of(getter) {
                    NodeData::GetAccessor(data) => data.body,
                    _ => None,
                };
                if body.is_some() {
                    // 56756: getter && getter.body →
                    // getReturnTypeFromBody(getter) — live since 5.5f.
                    return Ok(Some(state.get_return_type_from_body(
                        getter,
                        tsrs2_types::CheckMode::NORMAL,
                    )?));
                }
            }
            Ok(None)
        })(self);
        let ty = match computed {
            Ok(ty) => ty,
            Err(err) => {
                self.pop_type_resolution();
                return Err(err);
            }
        };
        let ty = match ty {
            Some(ty) => ty,
            None => {
                // 56761-56769: the noImplicitAny suggestions.
                if self
                    .options
                    .strict_option_value(self.options.no_implicit_any)
                {
                    let name = self.symbol_display_name(symbol);
                    if let Some(setter) = setter {
                        self.error_at(
                            Some(setter),
                            &diagnostics::Property_0_implicitly_has_type_any_because_its_set_accessor_lacks_a_parameter_type_annotation,
                            &[&name],
                        );
                    } else if let Some(getter) = getter {
                        self.error_at(
                            Some(getter),
                            &diagnostics::Property_0_implicitly_has_type_any_because_its_get_accessor_lacks_a_return_type_annotation,
                            &[&name],
                        );
                    }
                }
                self.tables.intrinsics.any
            }
        };
        let resolved = if self.pop_type_resolution() {
            ty
        } else {
            // 56771-56783: circular accessor annotations.
            let name = self.symbol_display_name(symbol);
            let location = self
                .annotated_accessor_type_node(getter)
                .map(|_| getter.expect("annotated getter"))
                .or_else(|| {
                    self.annotated_accessor_type_node(setter)
                        .map(|_| setter.expect("annotated setter"))
                });
            if let Some(location) = location {
                self.error_at(
                    Some(location),
                    &diagnostics::_0_is_referenced_directly_or_indirectly_in_its_own_type_annotation,
                    &[&name],
                );
            }
            self.tables.intrinsics.any
        };
        if self
            .links
            .symbol(symbol)
            .type_of_symbol
            .resolved()
            .is_none()
        {
            self.links.set_symbol_type(
                self.speculation_depth,
                symbol,
                LinkSlot::Resolved(resolved),
            );
        }
        Ok(self
            .links
            .symbol(symbol)
            .type_of_symbol
            .resolved()
            .expect("just filled"))
    }

    /// tsc-port: getWriteTypeOfAccessors @6.0.3
    /// tsc-hash: e244812d509db78f1218b344fc0737b9f010d62f752928c7740aaa30990c8a88
    /// tsc-span: _tsc.js:56787-56803
    pub(crate) fn get_write_type_of_accessors(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.symbol(symbol).write_type.resolved() {
            return Ok(cached);
        }
        if !self.push_type_resolution(
            crate::state::ResolutionTarget::Symbol(symbol),
            tsrs2_types::TypeSystemPropertyName::WRITE_TYPE,
        ) {
            return Ok(self.tables.intrinsics.error);
        }
        let setter = self.declaration_of_kind(symbol, SyntaxKind::SetAccessor);
        let annotation = self.annotated_accessor_type_node(setter);
        let computed = match annotation {
            Some(annotation) => self.get_type_from_type_node(annotation).map(Some),
            None => Ok(None),
        };
        let write_type = match computed {
            Ok(write_type) => write_type,
            Err(err) => {
                self.pop_type_resolution();
                return Err(err);
            }
        };
        let write_type = if self.pop_type_resolution() {
            match write_type {
                Some(write_type) => write_type,
                None => self.get_type_of_accessors(symbol)?,
            }
        } else {
            if annotation.is_some() {
                let name = self.symbol_display_name(symbol);
                self.error_at(
                    setter,
                    &diagnostics::_0_is_referenced_directly_or_indirectly_in_its_own_type_annotation,
                    &[&name],
                );
            }
            self.tables.intrinsics.any
        };
        if self.links.symbol(symbol).write_type.resolved().is_none() {
            self.links
                .set_symbol_write_type(self.speculation_depth, symbol, write_type);
        }
        Ok(self
            .links
            .symbol(symbol)
            .write_type
            .resolved()
            .expect("just filled"))
    }

    /// tsc getAnnotatedAccessorTypeNode (56718-56734): getter return
    /// annotation / setter first-parameter annotation (the
    /// auto-accessor PropertyDeclaration arm is M7).
    fn annotated_accessor_type_node(&self, accessor: Option<NodeId>) -> Option<NodeId> {
        let accessor = accessor?;
        match self.data_of(accessor) {
            NodeData::GetAccessor(data) => data.r#type,
            NodeData::SetAccessor(data) => {
                let parameters = self.nodes_of(data.parameters);
                let parameter = parameters.first().copied()?;
                match self.data_of(parameter) {
                    NodeData::Parameter(data) => data.r#type,
                    _ => None,
                }
            }
            _ => None,
        }
    }

    // ---- member instantiation ----

    /// tsc-port: createInstantiatedSymbolTable @6.0.3
    /// tsc-hash: 93ee5d54e22d588ad60edb5da39cc9b435088befbaeec8ff3d8acbcefe9edee5
    /// tsc-span: _tsc.js:57580-57586
    fn create_instantiated_symbol_table(
        &mut self,
        symbols: &[SymbolId],
        mapper: crate::instantiate::MapperId,
        mapping_this_only: bool,
    ) -> CheckResult2<tsrs2_binder::SymbolTable> {
        let mut result = tsrs2_binder::SymbolTable::default();
        for &symbol in symbols {
            let value = if mapping_this_only && self.is_thisless(symbol) {
                symbol
            } else {
                self.instantiate_symbol(symbol, mapper)
            };
            result.insert(self.binder.symbol(symbol).escaped_name.clone(), value);
        }
        Ok(result)
    }

    /// tsc-port: addInheritedMembers @6.0.3
    /// tsc-hash: cea31f588695a7b112fd7d5e97a0fa381eb26f00ed6d678a8efed4707344bde2
    /// tsc-span: _tsc.js:57587-57596
    ///
    /// The derived-is-JS-assignment override arm (isBinaryExpression
    /// valueDeclaration) rides on JS assignment binding — elided
    /// project-wide; isStaticPrivateIdentifierProperty is class-only
    /// machinery live from 5.3e (private identifiers cannot appear on
    /// interfaces).
    fn add_inherited_members(
        &mut self,
        symbols: &mut tsrs2_binder::SymbolTable,
        base_symbols: &[SymbolId],
    ) {
        for &base in base_symbols {
            let name = self.binder.symbol(base).escaped_name.clone();
            if !symbols.contains_key(&name) {
                symbols.insert(name, base);
            }
        }
    }

    /// tsc-port: isThisless @6.0.3
    /// tsc-hash: ba4574791e8b6cfcc0698b11a4a7bd78d67b08c1f80d3a66b0af8f24e1680405
    /// tsc-span: _tsc.js:57561-57579
    fn is_thisless(&self, symbol: SymbolId) -> bool {
        let declarations = &self.binder.symbol(symbol).declarations;
        if declarations.len() != 1 {
            return false;
        }
        let declaration = declarations[0];
        match self.kind_of(declaration) {
            SyntaxKind::PropertyDeclaration | SyntaxKind::PropertySignature => {
                self.is_thisless_variable_like_declaration(declaration)
            }
            SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::Constructor
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor => self.is_thisless_function_like_declaration(declaration),
            _ => false,
        }
    }

    /// tsc isThislessType/isThislessVariableLikeDeclaration/
    /// isThislessFunctionLikeDeclaration/isThislessTypeParameter
    /// (57515-57560, one tsc-span for the family).
    /// tsc-hash: 751b178605bf587488fd82360737a52b1329e2352a4b746869ca7dbd2dac6f4e
    /// tsc-span: _tsc.js:57515-57560
    fn is_thisless_type(&self, node: NodeId) -> bool {
        match self.kind_of(node) {
            SyntaxKind::AnyKeyword
            | SyntaxKind::UnknownKeyword
            | SyntaxKind::StringKeyword
            | SyntaxKind::NumberKeyword
            | SyntaxKind::BigIntKeyword
            | SyntaxKind::BooleanKeyword
            | SyntaxKind::SymbolKeyword
            | SyntaxKind::ObjectKeyword
            | SyntaxKind::VoidKeyword
            | SyntaxKind::UndefinedKeyword
            | SyntaxKind::NeverKeyword
            | SyntaxKind::LiteralType => true,
            SyntaxKind::ArrayType => match self.data_of(node) {
                NodeData::ArrayType(data) => data
                    .element_type
                    .is_some_and(|element| self.is_thisless_type(element)),
                _ => false,
            },
            SyntaxKind::TypeReference => match self.data_of(node) {
                NodeData::TypeReference(data) => self
                    .nodes_of(data.type_arguments)
                    .iter()
                    .all(|&argument| self.is_thisless_type(argument)),
                _ => false,
            },
            _ => false,
        }
    }

    fn is_thisless_type_parameter(&self, node: NodeId) -> bool {
        let constraint = match self.data_of(node) {
            NodeData::TypeParameter(data) => data.constraint,
            _ => None,
        };
        constraint.is_none_or(|constraint| self.is_thisless_type(constraint))
    }

    fn is_thisless_variable_like_declaration(&self, node: NodeId) -> bool {
        let (annotation, initializer) = match self.data_of(node) {
            NodeData::PropertyDeclaration(data) => (data.r#type, data.initializer),
            NodeData::PropertySignature(data) => (data.r#type, data.initializer),
            NodeData::Parameter(data) => (data.r#type, data.initializer),
            _ => (None, None),
        };
        match annotation {
            Some(annotation) => self.is_thisless_type(annotation),
            None => initializer.is_none(),
        }
    }

    fn is_thisless_function_like_declaration(&self, node: NodeId) -> bool {
        let (kind, return_type, parameters, type_parameters) = match self.data_of(node) {
            NodeData::MethodDeclaration(data) => (
                SyntaxKind::MethodDeclaration,
                data.r#type,
                data.parameters,
                data.type_parameters,
            ),
            NodeData::MethodSignature(data) => (
                SyntaxKind::MethodSignature,
                data.r#type,
                data.parameters,
                data.type_parameters,
            ),
            NodeData::Constructor(data) => (
                SyntaxKind::Constructor,
                data.r#type,
                data.parameters,
                data.type_parameters,
            ),
            NodeData::GetAccessor(data) => (
                SyntaxKind::GetAccessor,
                data.r#type,
                data.parameters,
                data.type_parameters,
            ),
            NodeData::SetAccessor(data) => (
                SyntaxKind::SetAccessor,
                data.r#type,
                data.parameters,
                data.type_parameters,
            ),
            _ => return false,
        };
        let return_ok = kind == SyntaxKind::Constructor
            || return_type.is_some_and(|annotation| self.is_thisless_type(annotation));
        return_ok
            && self
                .nodes_of(parameters)
                .iter()
                .all(|&parameter| self.is_thisless_variable_like_declaration(parameter))
            && self
                .nodes_of(type_parameters)
                .iter()
                .all(|&parameter| self.is_thisless_type_parameter(parameter))
    }

    /// instantiateSignatures (63824-63826): map instantiate_signature
    /// without erasure.
    fn instantiate_signature_list(
        &mut self,
        signatures: &[SignatureId],
        mapper: crate::instantiate::MapperId,
    ) -> CheckResult2<Vec<SignatureId>> {
        let mut result = Vec::with_capacity(signatures.len());
        for &signature in signatures {
            result.push(
                self.instantiate_signature(
                    signature, mapper, /*erase_type_parameters*/ false,
                )?,
            );
        }
        Ok(result)
    }

    /// instantiateIndexInfos (63827-63828): map instantiate_index_info.
    fn instantiate_index_info_list(
        &mut self,
        index_infos: &[IndexInfo],
        mapper: crate::instantiate::MapperId,
    ) -> CheckResult2<Vec<IndexInfo>> {
        let mut result = Vec::with_capacity(index_infos.len());
        for info in index_infos {
            result.push(self.instantiate_index_info(info, mapper)?);
        }
        Ok(result)
    }

    /// tsc-port: resolveAnonymousTypeMembers @6.0.3
    /// tsc-hash: 5da860e7aee705f29431b2726015d0564a56aeddbe32d2653253ad09aab4f93f
    /// tsc-span: _tsc.js:58316-58407
    ///
    /// Live arms: instantiated targets (58317-58330), TypeLiteral
    /// symbols (58332-58340), function/method value types with no
    /// exports. Classes, enums, modules, globalThis and export-carrying
    /// functions (namespace merges) are 5.3e value-side rows. Every arm
    /// writes the EMPTY table early (tsc 58318/58333/58354) so a
    /// re-entrant read mid-resolution observes empty members instead of
    /// recursing — the write is completed in place, or retracted on an
    /// Err unwind.
    fn resolve_anonymous_type_members(&mut self, ty: TypeId) -> CheckResult2<MembersId> {
        let early_id = self.alloc_members(ResolvedMembers::default());
        self.links
            .set_type_members(self.speculation_depth, ty, LinkSlot::Resolved(early_id));
        let resolved = (|state: &mut Self| -> CheckResult2<ResolvedMembers> {
            if state
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::INSTANTIATED)
            {
                // 58317-58330: the target's members under type.mapper.
                let target = state
                    .links
                    .ty(ty)
                    .instantiated_target
                    .expect("Instantiated object flag implies links target");
                let mapper = state
                    .links
                    .ty(ty)
                    .instantiated_mapper
                    .expect("Instantiated object flag implies links mapper");
                let target_properties = state.get_properties_of_object_type_owned(target)?;
                let members = state.create_instantiated_symbol_table(
                    &target_properties,
                    mapper,
                    /*mapping_this_only*/ false,
                )?;
                let target_calls =
                    state.get_signatures_of_type(target, crate::structural::SignatureKind::Call)?;
                let call_signatures = state.instantiate_signature_list(&target_calls, mapper)?;
                let target_constructs = state
                    .get_signatures_of_type(target, crate::structural::SignatureKind::Construct)?;
                let construct_signatures =
                    state.instantiate_signature_list(&target_constructs, mapper)?;
                let target_index_infos = state.get_index_infos_of_type(target)?;
                let index_infos = state.instantiate_index_info_list(&target_index_infos, mapper)?;
                let properties = state.get_named_members(&members);
                return Ok(ResolvedMembers {
                    members,
                    properties,
                    call_signatures,
                    construct_signatures,
                    index_infos,
                });
            }
            let symbol = state
                .tables
                .type_of(ty)
                .symbol
                .expect("anonymous member resolution requires a symbol");
            let flags = state.symbol_flags(symbol);
            if flags.intersects(SymbolFlags::TYPE_LITERAL) {
                let members = state.get_members_of_symbol(symbol)?;
                let properties = state.get_named_members(&members);
                let call_signatures = state
                    .get_signatures_of_symbol(members.get(InternalSymbolName::CALL).copied())?;
                let construct_signatures = state
                    .get_signatures_of_symbol(members.get(InternalSymbolName::NEW).copied())?;
                let index_infos = state.get_index_infos_of_symbol(symbol)?;
                return Ok(ResolvedMembers {
                    members,
                    properties,
                    call_signatures,
                    construct_signatures,
                    index_infos,
                });
            }
            if flags.intersects(SymbolFlags::FUNCTION | SymbolFlags::METHOD | SymbolFlags::CLASS) {
                // 58341-58407: the value-side tail — exports as
                // members, static base inheritance for classes,
                // call/construct signatures.
                let mut members = state.get_exports_of_symbol(symbol)?;
                let mut base_constructor_index_info: Option<IndexInfo> = None;
                if flags.intersects(SymbolFlags::CLASS) {
                    let class_type = state.get_declared_type_of_class_or_interface(symbol)?;
                    let base_constructor_type =
                        state.get_base_constructor_type_of_class(class_type)?;
                    if state.tables.flags_of(base_constructor_type).intersects(
                        TypeFlags::OBJECT | TypeFlags::INTERSECTION | TypeFlags::TYPE_VARIABLE,
                    ) {
                        // 58359-58360: copy named+index members, then
                        // inherit the base's STATIC side.
                        let named = state.get_named_members(&members);
                        let mut table = state.symbol_list_to_table(&named);
                        if let Some(index_symbol) = members.get(InternalSymbolName::INDEX).copied()
                        {
                            table.insert(InternalSymbolName::INDEX.to_owned(), index_symbol);
                        }
                        members = table;
                        let base_properties =
                            state.get_properties_of_type_full(base_constructor_type)?;
                        state.add_inherited_members(&mut members, &base_properties);
                    } else if state
                        .tables
                        .flags_of(base_constructor_type)
                        .intersects(TypeFlags::ANY)
                    {
                        base_constructor_index_info = Some(IndexInfo {
                            key_type: state.tables.intrinsics.string,
                            value_type: state.tables.intrinsics.any,
                            is_readonly: false,
                            declaration: None,
                            components: None,
                        });
                    }
                }
                let index_symbol = members.get(InternalSymbolName::INDEX).copied();
                let mut index_infos = match index_symbol {
                    Some(index_symbol) => state.get_index_infos_of_index_symbol(index_symbol)?,
                    None => match base_constructor_index_info {
                        Some(info) => vec![info],
                        // The enum number-index arm (58372-58374) rides
                        // on enum declared types (5.3b-doc stage);
                        // enums do not take this path yet.
                        None => Vec::new(),
                    },
                };
                if index_symbol.is_some() {
                    // tsc computes infos from the index symbol only;
                    // the base fallback is the None arm above.
                    let _ = &mut index_infos;
                }
                let call_signatures =
                    if flags.intersects(SymbolFlags::FUNCTION | SymbolFlags::METHOD) {
                        state.get_signatures_of_symbol(Some(symbol))?
                    } else {
                        Vec::new()
                    };
                let mut construct_signatures = Vec::new();
                if flags.intersects(SymbolFlags::CLASS) {
                    let constructor = state
                        .symbol_members(symbol)
                        .get(InternalSymbolName::CONSTRUCTOR)
                        .copied();
                    construct_signatures = state.get_signatures_of_symbol(constructor)?;
                    if construct_signatures.is_empty() {
                        let class_type = state.get_declared_type_of_class_or_interface(symbol)?;
                        construct_signatures =
                            state.get_default_construct_signatures(class_type)?;
                    }
                }
                let properties = state.get_named_members(&members);
                return Ok(ResolvedMembers {
                    members,
                    properties,
                    call_signatures,
                    construct_signatures,
                    index_infos,
                });
            }
            Err(Unsupported::new(format!(
                "anonymous members for symbol flags {flags:?} (M4 5.3e/5.8)"
            )))
        })(self);
        match resolved {
            Ok(resolved) => {
                *self.members_mut(early_id) = resolved;
                Ok(early_id)
            }
            Err(err) => {
                self.links.retract_type_members(ty);
                Err(err)
            }
        }
    }

    /// tsc-port: getNamedMembers @6.0.3
    /// tsc-hash: 0322ebcde783f23fab98e8cfe109540e322461b73e05839efd7bb5802db9abc2
    /// tsc-span: _tsc.js:50145-50189
    ///
    /// tsc-port: isNamedMember @6.0.3
    /// tsc-hash: a5f7a9c67fc0a7fc6fdd4fad34d8a8553232aa4d800b44c90fd00f48682b8964
    /// tsc-span: _tsc.js:50190-50192
    ///
    /// stableTypeOrdering is off by default: insertion order (the
    /// binder's IndexMap order) is the observable order. symbolIsValue's
    /// alias-resolution branch is M4; members here are value members.
    fn get_named_members(&self, members: &tsrs2_binder::SymbolTable) -> Vec<SymbolId> {
        members
            .iter()
            .filter(|(name, &symbol)| {
                !is_reserved_member_name(name)
                    && self.symbol_flags(symbol).intersects(SymbolFlags::VALUE)
            })
            .map(|(_, &symbol)| symbol)
            .collect()
    }

    // ---- index signatures ----

    /// tsc-port: getIndexInfosOfSymbol @6.0.3
    /// tsc-hash: 74b6395ea69a06dcf65b424906f74f8a4781ed280de38c1afe1973a85931804e
    /// tsc-span: _tsc.js:59992-59995
    ///
    /// tsc-port: getIndexSymbol @6.0.3
    /// tsc-hash: 2dffc568392757cc14418934e18e93bb1927d8ad61ab7932fb91c1e8a0cfb768
    /// tsc-span: _tsc.js:59983-59985
    fn get_index_infos_of_symbol(&mut self, symbol: SymbolId) -> CheckResult2<Vec<IndexInfo>> {
        // getIndexSymbol reads getMembersOfSymbol (the late-bound
        // table) — a computed-name index member surfaces here and the
        // declaration reader below keeps its honest escape.
        let index_symbol = self
            .get_members_of_symbol(symbol)?
            .get(InternalSymbolName::INDEX)
            .copied();
        match index_symbol {
            Some(index_symbol) => self.get_index_infos_of_index_symbol(index_symbol),
            None => Ok(Vec::new()),
        }
    }

    /// tsc-port: getIndexInfosOfIndexSymbol @6.0.3
    /// tsc-hash: 860af0bebe06ec9b601dc9788cd32f2ae7a2705471665cf26e917ab689fe15a5
    /// tsc-span: _tsc.js:59996-60052
    ///
    /// M3 slice: the isIndexSignatureDeclaration arm (60007-60017).
    /// Late-bound computed-name index signatures (60018-60049) are M4.
    fn get_index_infos_of_index_symbol(
        &mut self,
        index_symbol: SymbolId,
    ) -> CheckResult2<Vec<IndexInfo>> {
        let declarations = self.binder.symbol(index_symbol).declarations.clone();
        let mut index_infos: Vec<IndexInfo> = Vec::new();
        for declaration in declarations {
            let NodeData::IndexSignature(data) = self.data_of(declaration).clone() else {
                return Err(Unsupported::new(
                    "late-bound computed-name index signatures (M4)",
                ));
            };
            let parameters = self.nodes_of(data.parameters);
            if parameters.len() != 1 {
                continue;
            }
            let NodeData::Parameter(parameter) = self.data_of(parameters[0]).clone() else {
                continue;
            };
            let Some(parameter_type) = parameter.r#type else {
                continue;
            };
            let key_type = self.get_type_from_type_node(parameter_type)?;
            let value_type = match data.r#type {
                Some(annotation) => self.get_type_from_type_node(annotation)?,
                None => self.tables.intrinsics.any,
            };
            let is_readonly = self.has_readonly_modifier(data.modifiers);
            // forEachType: union key types split into one info per
            // constituent (60011).
            let key_types: Vec<TypeId> =
                if self.tables.flags_of(key_type).intersects(TypeFlags::UNION) {
                    match &self.tables.type_of(key_type).data {
                        TypeData::Union { types, .. } => types.to_vec(),
                        _ => vec![key_type],
                    }
                } else {
                    vec![key_type]
                };
            for key_type in key_types {
                if self.is_valid_index_key_type(key_type)
                    && !index_infos.iter().any(|info| info.key_type == key_type)
                {
                    index_infos.push(IndexInfo {
                        key_type,
                        value_type,
                        is_readonly,
                        declaration: Some(declaration),
                        components: None,
                    });
                }
            }
        }
        Ok(index_infos)
    }

    /// tsc-port: isValidIndexKeyType @6.0.3
    /// tsc-hash: af4172bba24054af84a69a5df992b6bcffaab2b93b0faa85a33bdf84430b543a
    /// tsc-span: _tsc.js:60053-60055
    ///
    /// The isGenericType exclusion in the intersection arm is
    /// constant-false in M3 (no type variables).
    fn is_valid_index_key_type(&self, key_type: TypeId) -> bool {
        let flags = self.tables.flags_of(key_type);
        if flags.intersects(TypeFlags::STRING | TypeFlags::NUMBER | TypeFlags::ES_SYMBOL) {
            return true;
        }
        if self.tables.is_pattern_literal_type(key_type) {
            return true;
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            if let TypeData::Intersection { types } = &self.tables.type_of(key_type).data {
                return types.iter().any(|&t| self.is_valid_index_key_type(t));
            }
        }
        false
    }

    fn has_readonly_modifier(&self, modifiers: Option<NodeArrayId>) -> bool {
        self.nodes_of(modifiers)
            .iter()
            .any(|&modifier| self.kind_of(modifier) == SyntaxKind::ReadonlyKeyword)
    }

    // ---- symbol types ----

    /// tsc-port: getTypeOfSymbol @6.0.3
    /// tsc-hash: 36123c37428ab9dcfb6c89ba1c42dbf1a5461becdfdade097ed545e21b50bfd7
    /// tsc-span: _tsc.js:56945-56975
    ///
    /// Dispatch slice: the Instantiated check-flag arm (5.2),
    /// variable/property symbols (annotation-typed) and function/method
    /// symbols. DeferredType transients are M6 machinery,
    /// Mapped/ReverseMapped M8; accessors, classes, enums, modules and
    /// aliases keep their owning-stage escapes.
    pub fn get_type_of_symbol(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        let check_flags = self.links.symbol(symbol).check_flags;
        if check_flags.intersects(CheckFlags::DEFERRED_TYPE) {
            return Err(Unsupported::new(
                "DeferredType symbols (getTypeOfSymbolWithDeferredType, M6)",
            ));
        }
        if check_flags.intersects(CheckFlags::INSTANTIATED) {
            return self.get_type_of_instantiated_symbol(symbol);
        }
        if check_flags.intersects(CheckFlags::MAPPED) {
            return Err(Unsupported::new(
                "mapped symbols (getTypeOfMappedSymbol, M8)",
            ));
        }
        if check_flags.intersects(CheckFlags::REVERSE_MAPPED) {
            return Err(Unsupported::new(
                "reverse-mapped symbols (getTypeOfReverseMappedSymbol, M8)",
            ));
        }
        let flags = self.symbol_flags(symbol);
        if flags.intersects(SymbolFlags::VARIABLE | SymbolFlags::PROPERTY) {
            return self.get_type_of_variable_or_parameter_or_property(symbol);
        }
        if flags.intersects(
            SymbolFlags::FUNCTION | SymbolFlags::METHOD | SymbolFlags::CLASS | SymbolFlags::ENUM,
        ) {
            return self.get_type_of_func_class_enum_module(symbol);
        }
        if flags.intersects(SymbolFlags::VALUE_MODULE) {
            return Err(Unsupported::new("module value types (M4 5.8)"));
        }
        if flags.intersects(SymbolFlags::ENUM_MEMBER) {
            return self.get_type_of_enum_member(symbol);
        }
        if flags.intersects(SymbolFlags::ACCESSOR) {
            return self.get_type_of_accessors(symbol);
        }
        if flags.intersects(SymbolFlags::ALIAS) {
            // getTypeOfAlias needs alias resolution (M4 5.8).
            return Err(Unsupported::new("alias value types (getTypeOfAlias, 5.8)"));
        }
        // tsc's tail (56974): symbols with NO value arm — TypeLiteral,
        // Interface, TypeAlias, TypeParameter shells — are errorType
        // in tsc too (typeHasStaticProperty probes hit this for
        // `{ ... }` __type receivers).
        Ok(self.tables.intrinsics.error)
    }

    /// tsc-port: getTypeOfInstantiatedSymbol @6.0.3
    /// tsc-hash: 00fcbdb7ebbb16d38d4a3e87f4221eaba62ee68a83e3d1f842b1340d457bb476
    /// tsc-span: _tsc.js:56885-56888
    fn get_type_of_instantiated_symbol(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.symbol(symbol).type_of_symbol.resolved() {
            return Ok(cached);
        }
        let target = self
            .links
            .symbol(symbol)
            .target
            .expect("Instantiated check flag implies links.target");
        let mapper = self.links.symbol(symbol).mapper;
        let target_type = self.get_type_of_symbol(target)?;
        let instantiated = self.instantiate_type(target_type, mapper)?;
        self.links.set_symbol_type(
            self.speculation_depth,
            symbol,
            LinkSlot::Resolved(instantiated),
        );
        Ok(instantiated)
    }

    /// tsc-port: getTypeOfVariableOrParameterOrProperty @6.0.3
    /// tsc-hash: 3401237074b42af69c2ceace5255cc0e405c373c4ab621f0c3e9f253791356bd
    /// tsc-span: _tsc.js:56631-56641
    ///
    /// tsc-port: getTypeForVariableLikeDeclaration @6.0.3
    /// tsc-hash: c0e8266ebc58c3f705777885e0cbce9e9a3452ce61f033c5e075f8f739ef624e
    /// tsc-span: _tsc.js:56032-56141
    ///
    /// M3 slice: the declared-annotation branch (56050/56057 —
    /// tryGetTypeFromEffectiveTypeNode + addOptionality). Initializer
    /// inference, binding patterns, widening and reportImplicitAny are
    /// M4/M6; the no-annotation fallback is anyType with the implicit-
    /// any diagnostic deferred.
    fn get_type_of_variable_or_parameter_or_property(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.symbol(symbol).type_of_symbol.resolved() {
            return Ok(cached);
        }
        // The (target, kind) cycle detector (56673); an Err unwind pops
        // the stack and leaves the slot Vacant, so a later query
        // re-resolves instead of fabricating a type (M3-review
        // Resolving-dangling fix — the Resolving slot state is retired
        // for this site).
        if !self.push_type_resolution(
            crate::state::ResolutionTarget::Symbol(symbol),
            tsrs2_types::TypeSystemPropertyName::TYPE,
        ) {
            let resolved = self.report_circularity_error(symbol);
            self.links.set_symbol_type(
                self.speculation_depth,
                symbol,
                LinkSlot::Resolved(resolved),
            );
            return Ok(resolved);
        }
        let declaration = self.binder.symbol(symbol).value_declaration;
        let computed = (|state: &mut Self| -> CheckResult2<TypeId> {
            let declaration = declaration.ok_or_else(|| {
                Unsupported::new("symbol without value declaration (M4 synthesis)")
            })?;
            // getTypeOfVariableOrParameterOrPropertyWorker dispatch
            // (56680-56711): the Prototype/requireSymbol/ModuleExports
            // heads and the JSON-source-file arm precede the resolution
            // stack in tsc; those symbol shapes never take this route
            // in the slice (modules 5.8, JS [JSDOC]).
            match state.kind_of(declaration) {
                SyntaxKind::ExportAssignment => {
                    Err(Unsupported::new("export= value type (5.8 modules)"))
                }
                SyntaxKind::BinaryExpression
                | SyntaxKind::PropertyAccessExpression
                | SyntaxKind::ElementAccessExpression
                | SyntaxKind::CallExpression
                | SyntaxKind::Identifier
                | SyntaxKind::StringLiteral
                | SyntaxKind::NumericLiteral
                | SyntaxKind::SourceFile => Err(Unsupported::new(
                    "assignment-declaration value type \
                     (getWidenedTypeForAssignmentDeclaration [JSDOC])",
                )),
                SyntaxKind::PropertyAssignment => {
                    match state.try_get_type_from_effective_type_node(declaration)? {
                        Some(declared) => Ok(declared),
                        None => state.check_property_assignment(declaration, CheckMode::NORMAL),
                    }
                }
                SyntaxKind::JsxAttribute => {
                    match state.try_get_type_from_effective_type_node(declaration)? {
                        Some(declared) => Ok(declared),
                        None => Err(Unsupported::new("checkJsxAttribute (5.7)")),
                    }
                }
                SyntaxKind::ShorthandPropertyAssignment => {
                    match state.try_get_type_from_effective_type_node(declaration)? {
                        Some(declared) => Ok(declared),
                        None => {
                            let name = match state.data_of(declaration) {
                                NodeData::ShorthandPropertyAssignment(data) => data.name,
                                _ => None,
                            }
                            .ok_or_else(|| {
                                Unsupported::new("shorthand without a name (parse recovery)")
                            })?;
                            state.check_expression_for_mutable_location(
                                name,
                                CheckMode::NORMAL,
                                /*force_tuple*/ false,
                            )
                        }
                    }
                }
                SyntaxKind::MethodDeclaration => {
                    match state.try_get_type_from_effective_type_node(declaration)? {
                        Some(declared) => Ok(declared),
                        None => state.check_object_literal_method(declaration, CheckMode::NORMAL),
                    }
                }
                SyntaxKind::Parameter
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::PropertySignature
                | SyntaxKind::VariableDeclaration
                | SyntaxKind::BindingElement => state
                    .get_widened_type_for_variable_like_declaration(
                        declaration,
                        /*report_errors*/ true,
                    ),
                other => Err(Unsupported::new(format!(
                    "worker declaration kind {other:?} (M4)"
                ))),
            }
        })(self);
        let resolved = match computed {
            Ok(resolved) => resolved,
            Err(err) => {
                self.pop_type_resolution();
                return Err(err);
            }
        };
        let resolved = if self.pop_type_resolution() {
            resolved
        } else {
            // A deeper frame flagged the cycle: the computed type is
            // garbage (56710-56715; the ValueModule arm is dead —
            // module symbols do not take this worker in the slice).
            self.report_circularity_error(symbol)
        };
        self.links
            .set_symbol_type(self.speculation_depth, symbol, LinkSlot::Resolved(resolved));
        Ok(resolved)
    }

    /// tsc-port: reportCircularityError @6.0.3
    /// tsc-hash: adf5723b96f6db25f0049b2c3df010cc591925e84ed5d87252a8da4b4ef5cffa
    /// tsc-span: _tsc.js:56893-56910
    ///
    /// The Alias arm (Circular_definition_of_import_alias_0) waits on
    /// alias declarations (M4 5.8).
    fn report_circularity_error(&mut self, symbol: SymbolId) -> TypeId {
        let Some(declaration) = self.binder.symbol(symbol).value_declaration else {
            return self.tables.intrinsics.any;
        };
        let annotation = self
            .variable_like_annotation(declaration)
            .ok()
            .and_then(|(annotation, _, _)| annotation);
        let name = self.symbol_display_name(symbol);
        if annotation.is_some() {
            self.error_at(
                Some(declaration),
                &diagnostics::_0_is_referenced_directly_or_indirectly_in_its_own_type_annotation,
                &[&name],
            );
            return self.tables.intrinsics.error;
        }
        let no_implicit_any = self
            .options
            .strict_option_value(self.options.no_implicit_any);
        let has_initializer = matches!(
            self.data_of(declaration),
            NodeData::Parameter(data) if data.initializer.is_some()
        );
        if no_implicit_any
            && (self.kind_of(declaration) != SyntaxKind::Parameter || has_initializer)
        {
            self.error_at(
                Some(declaration),
                &diagnostics::_0_implicitly_has_type_any_because_it_does_not_have_a_type_annotation_and_is_referenced_directly_or_indirectly_in_its_own_initializer,
                &[&name],
            );
        }
        self.tables.intrinsics.any
    }

    /// tsc-port: getWidenedTypeForVariableLikeDeclaration @6.0.3
    /// tsc-hash: 825d1f13aef6988c4eedf9b267afb2f03a735450f7a1cf228655fa5820bed83d
    /// tsc-span: _tsc.js:56552-56559
    pub(crate) fn get_widened_type_for_variable_like_declaration(
        &mut self,
        declaration: NodeId,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        let ty = self.get_type_for_variable_like_declaration(
            declaration,
            /*include_optionality*/ true,
            CheckMode::NORMAL,
        )?;
        self.widen_type_for_variable_like_declaration(ty, declaration, report_errors)
    }

    /// tsc-port: widenTypeForVariableLikeDeclaration @6.0.3
    /// tsc-hash: 6fde6424a18e58f7812383933306b669f058a47225dc182ab1978618ce527a36
    /// tsc-span: _tsc.js:56586-56606
    ///
    /// The ESSymbol/isGlobalSymbolConstructor arm escapes: an ESSymbol
    /// initializer type only arrives through Symbol() calls
    /// (getResolvedSignature, 5.7), so the arm is dormant until then.
    fn widen_type_for_variable_like_declaration(
        &mut self,
        ty: Option<TypeId>,
        declaration: NodeId,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if let Some(mut ty) = ty {
            if self.tables.flags_of(ty).intersects(TypeFlags::ES_SYMBOL) {
                // isGlobalSymbolConstructor(declaration.parent): only a
                // parent whose (merged) symbol IS the global
                // SymbolConstructor type symbol takes
                // getESSymbolLikeTypeForNode. Everything else (ordinary
                // `: symbol` annotations) falls through.
                if self.is_in_js_file(declaration) {
                    return Err(Unsupported::new(
                        "widenTypeForVariableLikeDeclaration JS ESSymbol arm ([JSDOC])",
                    ));
                }
                let parent_symbol = self
                    .parent_of(declaration)
                    .and_then(|parent| self.binder.node_symbol(parent))
                    .map(|symbol| self.get_merged_symbol(symbol));
                if let Some(parent_symbol) = parent_symbol {
                    // getGlobalESSymbolConstructorTypeSymbol(false).
                    let global = self.get_global_type_symbol("SymbolConstructor", false);
                    if global == Some(parent_symbol) {
                        ty = self.get_es_symbol_like_type_for_node(declaration)?;
                    }
                }
            }
            if report_errors {
                self.report_errors_from_widening(declaration, ty, /*widening_kind*/ None)?;
            }
            if self
                .tables
                .flags_of(ty)
                .intersects(TypeFlags::UNIQUE_ES_SYMBOL)
                && (self.kind_of(declaration) == SyntaxKind::BindingElement
                    || self.effective_type_annotation_node(declaration).is_none())
            {
                let declaration_symbol = self.get_symbol_of_declaration(declaration)?;
                if self.tables.type_of(ty).symbol != Some(declaration_symbol) {
                    ty = self.tables.intrinsics.es_symbol;
                }
            }
            return self.get_widened_type(ty);
        }
        let is_rest_parameter = matches!(
            self.data_of(declaration),
            NodeData::Parameter(data) if data.dot_dot_dot_token.is_some()
        );
        let ty = if is_rest_parameter {
            self.any_array_type()?
        } else {
            self.tables.intrinsics.any
        };
        if report_errors && !self.declaration_belongs_to_private_ambient_member(declaration) {
            self.report_implicit_any(declaration, ty, /*widening_kind*/ None)?;
        }
        Ok(ty)
    }

    /// tsc-port: declarationBelongsToPrivateAmbientMember @6.0.3
    /// tsc-hash: 4d59a6942967180c236c14b747702f0260d949bf4fceed6c2fa9903c2de4d9eb
    /// tsc-span: _tsc.js:56607-56611
    ///
    /// isPrivateWithinAmbient (18580-18582): private modifier (or
    /// #-name) inside an Ambient-flagged subtree.
    fn declaration_belongs_to_private_ambient_member(&self, declaration: NodeId) -> bool {
        let source = self.binder.source_of_node(declaration);
        let root = node_util::get_root_declaration(source, declaration);
        let member = if self.kind_of(root) == SyntaxKind::Parameter {
            self.parent_of(root).unwrap_or(root)
        } else {
            root
        };
        let private = node_util::get_combined_modifier_flags(source, member)
            .intersects(ModifierFlags::PRIVATE)
            || self
                .name_of_node(member)
                .is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier);
        private && self.node_flags(member) & tsrs2_types::NodeFlags::AMBIENT.bits() != 0
    }

    /// tsc-port: tryGetTypeFromEffectiveTypeNode @6.0.3
    /// tsc-hash: 0d815a919b7406dc0fa9e388f2915ef9ddc5c43afa9421c92b7d339ec0e2579d
    /// tsc-span: _tsc.js:56612-56617
    pub(crate) fn try_get_type_from_effective_type_node(
        &mut self,
        declaration: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        match self.effective_type_annotation_node(declaration) {
            Some(annotation) => Ok(Some(self.get_type_from_type_node(annotation)?)),
            None => Ok(None),
        }
    }

    /// tsc-port: getTypeForVariableLikeDeclaration @6.0.3
    /// tsc-hash: 109205c923753ffa4729ddda5d05745808a37dba0474c79e15e7da5a9e0bb8df
    /// tsc-span: _tsc.js:56032-56144
    ///
    /// Escaped arms: for-in/for-of variables (5.8 statements /
    /// [ITER]), the JSDoc/JS container arms ([JSDOC]), the property-
    /// declaration constructor/static-block flow arms ([FLOW M5]) and
    /// its ambient getTypeOfPropertyInBaseClass tail (5.8).
    ///
    /// The AUTO ARM ([FLOW M5]): tsc returns autoType/autoArrayType and
    /// lets control-flow analysis evolve the type. Oracle-pinned
    /// (2026-07-13): `let x;` / `let x = null;` / `let x = [];` /
    /// `const x = [];` all check clean at uses under strict — the M4
    /// stand-in is anyType (falling through to the initializer arm
    /// would render null/never[] relations tsc never sees: FP), with
    /// the flow rows (18048/2454/7034) recorded FN until M5.
    fn get_type_for_variable_like_declaration(
        &mut self,
        declaration: NodeId,
        include_optionality: bool,
        check_mode: CheckMode,
    ) -> CheckResult2<Option<TypeId>> {
        let source = self.binder.source_of_node(declaration);
        let kind = self.kind_of(declaration);
        let parent = self.parent_of(declaration);
        if kind == SyntaxKind::VariableDeclaration {
            let grand = parent.and_then(|parent| self.parent_of(parent));
            match grand.map(|grand| self.kind_of(grand)) {
                Some(SyntaxKind::ForInStatement) => {
                    return Err(Unsupported::new("for-in variable type (5.8 statements)"));
                }
                Some(SyntaxKind::ForOfStatement) => {
                    return Err(Unsupported::new(
                        "for-of variable type (checkRightHandSideOfForOf [ITER] 5.8)",
                    ));
                }
                _ => {}
            }
        }
        if parent.is_some_and(|parent| {
            matches!(
                self.kind_of(parent),
                SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
            )
        }) {
            return self.get_type_for_binding_element(declaration);
        }
        let is_property = (kind == SyntaxKind::PropertyDeclaration
            && !node_util::has_syntactic_modifier(source, declaration, ModifierFlags::ACCESSOR))
            || kind == SyntaxKind::PropertySignature;
        let is_optional = include_optionality && self.is_optional_declaration(declaration);
        let declared_type = self.try_get_type_from_effective_type_node(declaration)?;
        if self.is_catch_clause_variable_declaration_or_binding_element(declaration) {
            if let Some(declared) = declared_type {
                let flags = self.tables.flags_of(declared);
                return Ok(Some(
                    if flags.intersects(TypeFlags::ANY)
                        || declared == self.tables.intrinsics.unknown
                    {
                        declared
                    } else {
                        self.tables.intrinsics.error
                    },
                ));
            }
            let use_unknown = self
                .options
                .strict_option_value(self.options.use_unknown_in_catch_variables);
            return Ok(Some(if use_unknown {
                self.tables.intrinsics.unknown
            } else {
                self.tables.intrinsics.any
            }));
        }
        if let Some(declared) = declared_type {
            return Ok(Some(self.tables.add_optionality(
                declared,
                is_property,
                is_optional,
            )));
        }
        let no_implicit_any = self
            .options
            .strict_option_value(self.options.no_implicit_any);
        if (no_implicit_any || self.is_in_js_file(declaration))
            && kind == SyntaxKind::VariableDeclaration
        {
            let name_is_binding_pattern = self.name_of_node(declaration).is_some_and(|name| {
                matches!(
                    self.kind_of(name),
                    SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
                )
            });
            let exported = node_util::get_combined_modifier_flags(source, declaration)
                .intersects(ModifierFlags::EXPORT);
            let ambient =
                self.node_flags(declaration) & tsrs2_types::NodeFlags::AMBIENT.bits() != 0;
            if !name_is_binding_pattern && !exported && !ambient {
                let constant = node_util::get_combined_node_flags(source, declaration).bits()
                    & tsrs2_types::NodeFlags::CONSTANT.bits()
                    != 0;
                let initializer = self.initializer_of(declaration);
                let null_or_undefined_initializer = match initializer {
                    None => true,
                    Some(initializer) => self.is_null_or_undefined_expr(initializer),
                };
                if !constant && null_or_undefined_initializer {
                    // tsc: autoType ([FLOW M5]) — anyType stand-in.
                    return Ok(Some(self.tables.intrinsics.any));
                }
                if initializer
                    .is_some_and(|initializer| self.is_empty_array_literal_expr(initializer))
                {
                    // tsc: autoArrayType ([FLOW M5]) — anyType stand-in.
                    return Ok(Some(self.tables.intrinsics.any));
                }
            }
        }
        if kind == SyntaxKind::Parameter {
            if self.binder.node_symbol(declaration).is_none() {
                return Ok(None);
            }
            let func = parent.expect("parameter has a parent");
            if self.kind_of(func) == SyntaxKind::SetAccessor
                && !self.has_late_bindable_ast_name(func)
            {
                let accessor_symbol = self.get_symbol_of_declaration(func)?;
                let getter = self.get_declaration_of_kind(accessor_symbol, SyntaxKind::GetAccessor);
                if let Some(getter) = getter {
                    let is_this_parameter = matches!(
                        self.data_of(declaration),
                        NodeData::Parameter(data)
                            if data.name.is_some_and(|name| {
                                self.kind_of(name) == SyntaxKind::Identifier
                                    && self
                                        .text_of_node(name)
                                        .is_ok_and(|text| text == "this")
                            })
                    );
                    if is_this_parameter {
                        return Err(Unsupported::new(
                            "accessor this-parameter type (getAccessorThisParameter, 5.8)",
                        ));
                    }
                    let getter_signature = self.get_signature_from_declaration(getter)?;
                    return Ok(Some(self.get_return_type_of_signature(getter_signature)?));
                }
            }
            // getParameterTypeOfTypeTag: [JSDOC] — no-op outside JS.
            let symbol = self.binder.node_symbol(declaration);
            let is_this =
                symbol.is_some_and(|symbol| self.binder.symbol(symbol).escaped_name == "this");
            let contextual = if is_this {
                self.get_contextual_this_parameter_type(func)?
            } else {
                self.get_contextually_typed_parameter_type(declaration)?
            };
            if let Some(contextual) = contextual {
                return Ok(Some(self.tables.add_optionality(
                    contextual,
                    /*is_property*/ false,
                    is_optional,
                )));
            }
        }
        let has_expression_initializer = matches!(
            kind,
            SyntaxKind::VariableDeclaration
                | SyntaxKind::Parameter
                | SyntaxKind::BindingElement
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::PropertyAssignment
                | SyntaxKind::EnumMember
        ) && self.initializer_of(declaration).is_some();
        if has_expression_initializer {
            if self.is_in_js_file(declaration) && kind != SyntaxKind::Parameter {
                return Err(Unsupported::new(
                    "JS container object type (getJSContainerObjectType [JSDOC])",
                ));
            }
            let initializer_type =
                self.check_declaration_initializer(declaration, check_mode, None)?;
            let widened =
                self.widen_type_inferred_from_initializer(declaration, initializer_type)?;
            return Ok(Some(self.tables.add_optionality(
                widened,
                is_property,
                is_optional,
            )));
        }
        if kind == SyntaxKind::PropertyDeclaration
            && (no_implicit_any || self.is_in_js_file(declaration))
        {
            let class = parent.expect("property declaration has a parent");
            let ambient_member = node_util::get_combined_modifier_flags(source, declaration)
                .intersects(ModifierFlags::AMBIENT);
            if !self.has_static_modifier(declaration) {
                if self.find_constructor_declaration(class).is_some() {
                    return Err(Unsupported::new(
                        "constructor-assigned property type (getFlowTypeInConstructor [FLOW M5])",
                    ));
                }
                if ambient_member {
                    return Err(Unsupported::new(
                        "ambient property base-class type (getTypeOfPropertyInBaseClass, 5.8)",
                    ));
                }
            } else {
                let has_static_blocks = matches!(
                    self.data_of(class),
                    NodeData::ClassDeclaration(data)
                        if self.nodes_of(data.members).iter().any(|&member| {
                            self.kind_of(member) == SyntaxKind::ClassStaticBlockDeclaration
                        })
                ) || matches!(
                    self.data_of(class),
                    NodeData::ClassExpression(data)
                        if self.nodes_of(data.members).iter().any(|&member| {
                            self.kind_of(member) == SyntaxKind::ClassStaticBlockDeclaration
                        })
                );
                if has_static_blocks {
                    return Err(Unsupported::new(
                        "static-block-assigned property type (getFlowTypeInStaticBlocks [FLOW M5])",
                    ));
                }
                if ambient_member {
                    return Err(Unsupported::new(
                        "ambient property base-class type (getTypeOfPropertyInBaseClass, 5.8)",
                    ));
                }
            }
        }
        if kind == SyntaxKind::JsxAttribute {
            return Ok(Some(self.tables.intrinsics.true_regular));
        }
        if let Some(name) = self.name_of_node(declaration) {
            if matches!(
                self.kind_of(name),
                SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
            ) {
                return Ok(Some(self.get_type_from_binding_pattern(
                    name, /*include_pattern_in_type*/ false, /*report_errors*/ true,
                )?));
            }
        }
        Ok(None)
    }

    /// tsc-port: getTypeForBindingElement @6.0.3
    /// tsc-hash: db39bb9df5d65526e7574373b9c37507d764c97897bfeb52630915930f6291ba
    /// tsc-span: _tsc.js:55942-55951
    ///
    /// tsc-port: getTypeForBindingElementParent @6.0.3
    /// tsc-hash: 08b0e4f2dd355e6594b9f063fb6aa6f72c5b0ce69241893358080cd4e8a01994
    /// tsc-span: _tsc.js:55824-55840
    fn get_type_for_binding_element(
        &mut self,
        declaration: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let is_rest = matches!(
            self.data_of(declaration),
            NodeData::BindingElement(data) if data.dot_dot_dot_token.is_some()
        );
        let check_mode = if is_rest {
            CheckMode::REST_BINDING_ELEMENT
        } else {
            CheckMode::NORMAL
        };
        let pattern = self
            .parent_of(declaration)
            .expect("binding element has a pattern");
        let parent_declaration = self
            .parent_of(pattern)
            .expect("binding pattern has a declaration");
        let parent_type =
            self.get_type_for_binding_element_parent(parent_declaration, check_mode)?;
        match parent_type {
            Some(parent_type) => Ok(Some(self.get_binding_element_type_from_parent_type(
                declaration,
                parent_type,
                /*no_tuple_bounds_check*/ false,
            )?)),
            None => Ok(None),
        }
    }

    fn get_type_for_binding_element_parent(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<Option<TypeId>> {
        if check_mode != CheckMode::NORMAL {
            return self.get_type_for_variable_like_declaration(
                node, /*include_optionality*/ false, check_mode,
            );
        }
        let symbol = self.get_symbol_of_declaration(node)?;
        if let Some(cached) = self.links.symbol(symbol).type_of_symbol.resolved() {
            return Ok(Some(cached));
        }
        self.get_type_for_variable_like_declaration(
            node, /*include_optionality*/ false, check_mode,
        )
    }

    /// tsc-port: isNullOrUndefined @6.0.3 (the checker-local
    /// isNullOrUndefined2)
    /// tsc-hash: 134b4ea51c0e63244ba9e3640b567e1455ff00b981dee40817ca46e89ef520cd
    /// tsc-span: _tsc.js:56013-56020
    fn is_null_or_undefined_expr(&mut self, node: NodeId) -> bool {
        let expr = self.skip_parentheses(node);
        match self.kind_of(expr) {
            SyntaxKind::NullKeyword => true,
            SyntaxKind::Identifier => self.get_resolved_symbol(expr) == Some(self.undefined_symbol),
            _ => false,
        }
    }

    /// tsc-port: isEmptyArrayLiteral @6.0.3 (the checker-local
    /// isEmptyArrayLiteral2)
    /// tsc-hash: aec39287153052d13f54374113ffbec58c92de043b2c6f6ff1bad85399baf420
    /// tsc-span: _tsc.js:56021-56028
    fn is_empty_array_literal_expr(&self, node: NodeId) -> bool {
        let expr = self.skip_parentheses(node);
        matches!(
            self.data_of(expr),
            NodeData::ArrayLiteralExpression(data)
                if self.nodes_of(data.elements).is_empty()
        )
    }

    /// tsc-port: isCatchClauseVariableDeclarationOrBindingElement @6.0.3
    /// tsc-hash: 621724a8fdb0fa42184253babb0c36ecf2e7a3862a4216921af939fec7741262
    /// tsc-span: _tsc.js:13709-13712
    fn is_catch_clause_variable_declaration_or_binding_element(&self, declaration: NodeId) -> bool {
        let source = self.binder.source_of_node(declaration);
        let root = node_util::get_root_declaration(source, declaration);
        self.kind_of(root) == SyntaxKind::VariableDeclaration
            && self
                .parent_of(root)
                .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::CatchClause)
    }

    /// isOptionalDeclaration (19304): questionToken presence on
    /// parameter/property shapes.
    fn is_optional_declaration(&self, declaration: NodeId) -> bool {
        match self.data_of(declaration) {
            NodeData::Parameter(data) => data.question_token.is_some(),
            NodeData::PropertyDeclaration(data) => data.question_token.is_some(),
            NodeData::PropertySignature(data) => data.question_token.is_some(),
            _ => false,
        }
    }

    /// The declaration shapes the M3 slice can type: property
    /// signatures, parameters, variable declarations. Returns
    /// (annotation, isProperty, isOptional) for addOptionality —
    /// isOptionalDeclaration (19304): questionToken presence.
    fn variable_like_annotation(
        &self,
        declaration: NodeId,
    ) -> CheckResult2<(Option<NodeId>, bool, bool)> {
        match self.data_of(declaration) {
            NodeData::PropertySignature(data) => Ok((
                data.r#type,
                /*is_property*/ true,
                data.question_token.is_some(),
            )),
            NodeData::PropertyDeclaration(data) => Ok((
                data.r#type,
                /*is_property*/ true,
                data.question_token.is_some(),
            )),
            NodeData::Parameter(data) => Ok((
                data.r#type,
                /*is_property*/ false,
                data.question_token.is_some(),
            )),
            NodeData::VariableDeclaration(data) => Ok((data.r#type, false, false)),
            _ => Err(Unsupported::new(format!(
                "variable-like declaration kind {:?} (M4)",
                self.kind_of(declaration)
            ))),
        }
    }

    /// tsc-port: getTypeOfFuncClassEnumModule @6.0.3
    /// tsc-hash: 079629bbc8a29f3e85c4f2c38c64b0c6ecd7f8e5253a87f56bef8c1749dc8dfa
    /// tsc-span: _tsc.js:56808-56827
    ///
    /// M3 slice: function/method symbols get a lazily-membered
    /// anonymous type; the class/enum/module worker paths are M4.
    fn get_type_of_func_class_enum_module(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.symbol(symbol).type_of_symbol.resolved() {
            return Ok(cached);
        }
        // getTypeOfFuncClassEnumModuleWorker (56828-56860): the JS
        // assignment/expando and commonJS arms are elided project-wide;
        // shorthand-ambient-module anyType is 5.8.
        let id = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
        self.tables.type_mut(id).object_flags = ObjectFlags::ANONYMOUS;
        self.tables.type_mut(id).symbol = Some(symbol);
        let resolved = if self.symbol_flags(symbol).intersects(SymbolFlags::CLASS) {
            // 56849-56852: mixin-extending classes intersect with the
            // base type variable.
            match self.get_base_type_variable_of_class(symbol)? {
                Some(base_type_variable) => {
                    self.get_intersection_type(&[id, base_type_variable], IntersectionFlags::NONE)?
                }
                None => id,
            }
        } else {
            // The optional-symbol getOptionalType arm (56854-56858) is
            // property machinery — value module/function symbols are
            // never OPTIONAL here.
            id
        };
        self.links
            .set_symbol_type(self.speculation_depth, symbol, LinkSlot::Resolved(resolved));
        Ok(resolved)
    }

    // ---- enum declared types (M4 5.3b) ----

    /// tsc-port: getDeclaredTypeOfEnum @6.0.3
    /// tsc-hash: f77e4529a1ec2fd69a2d6f2ff3749a16327d69ef9d535ff814989aadd98d6f1e
    /// tsc-span: _tsc.js:57439-57474
    ///
    /// hasBindableName (57448) splits into the engine's late-binding
    /// shape: late-bindable AST names escape (5.5), other dynamic
    /// names are skipped. tsc's unconditional member-links write is a
    /// vacant-guarded write here (LinkSlot discipline): merged enums
    /// that redeclare a member would make tsc's LAST write win where
    /// ours keeps the FIRST — those fixtures are 2300-family errors.
    pub(crate) fn get_declared_type_of_enum(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        if let Some(declared) = self.links.symbol(symbol).declared_type.resolved() {
            return Ok(declared);
        }
        let mut member_type_list: Vec<TypeId> = Vec::new();
        let declarations = self.binder.symbol(symbol).declarations.clone();
        for declaration in declarations {
            let NodeData::EnumDeclaration(data) = self.data_of(declaration) else {
                continue;
            };
            for member in self.nodes_of(data.members) {
                if self.has_late_bindable_ast_name(member) {
                    return Err(Unsupported::new(
                        "late-bound enum member name (lateBindMember 57662, M7-stub)",
                    ));
                }
                if node_util::has_dynamic_name(self.binder.source_of_node(member), member) {
                    continue;
                }
                let member_symbol = self
                    .node_symbol(member)
                    .expect("bound enum members carry symbols");
                // getSymbolOfDeclaration (57448).
                let member_symbol = self.get_merged_symbol(member_symbol);
                let value = self.get_enum_member_value(member)?.value;
                let base = match value {
                    Some(EvalValue::Str(text)) => self.tables.get_enum_literal_type(
                        LiteralValue::String(text),
                        symbol,
                        member_symbol,
                    ),
                    Some(EvalValue::Num(number)) => self.tables.get_enum_literal_type(
                        LiteralValue::Number(number),
                        symbol,
                        member_symbol,
                    ),
                    None => self.tables.create_computed_enum_type(member_symbol),
                };
                let member_type = self.tables.get_fresh_type_of_literal_type(base);
                if self
                    .links
                    .symbol(member_symbol)
                    .declared_type
                    .resolved()
                    .is_none()
                {
                    self.links.set_symbol_declared_type(
                        self.speculation_depth,
                        member_symbol,
                        LinkSlot::Resolved(member_type),
                    );
                }
                member_type_list.push(self.tables.get_regular_type_of_literal_type(member_type));
            }
        }
        let enum_type = if !member_type_list.is_empty() {
            let union = self.get_union_type_ex_with_origin(
                &member_type_list,
                UnionReduction::Literal,
                Some(symbol),
                /*alias_type_arguments*/ None,
                /*origin*/ None,
            )?;
            if self.tables.flags_of(union).intersects(TypeFlags::UNION) {
                // 57466-57469: the enum union is stamped EnumLiteral and
                // takes the enum symbol; the intern key already carries
                // the enum symbol as alias id, so the mutation cannot
                // leak into structurally identical bare unions.
                let ty = self.tables.type_mut(union);
                ty.flags = TypeFlags::from_bits(ty.flags.bits() | TypeFlags::ENUM_LITERAL.bits());
                ty.symbol = Some(symbol);
            }
            union
        } else {
            self.tables.create_computed_enum_type(symbol)
        };
        if let Some(declared) = self.links.symbol(symbol).declared_type.resolved() {
            return Ok(declared);
        }
        self.links.set_symbol_declared_type(
            self.speculation_depth,
            symbol,
            LinkSlot::Resolved(enum_type),
        );
        Ok(enum_type)
    }

    /// tsc-port: getDeclaredTypeOfEnumMember @6.0.3
    /// tsc-hash: 55c65eb5da7d98f13bba95bf12368b43ce3d2ed59a2e04edafe5ac1777f90fef
    /// tsc-span: _tsc.js:57484-57493
    ///
    /// The inner re-check is load-bearing: forcing the parent enum
    /// fills bindable members' slots as a side effect, and only the
    /// leftovers (non-bindable members) take the whole-enum type.
    pub(crate) fn get_declared_type_of_enum_member(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<TypeId> {
        if let Some(declared) = self.links.symbol(symbol).declared_type.resolved() {
            return Ok(declared);
        }
        let parent = self
            .get_parent_of_symbol(symbol)
            .expect("enum member symbols have enum parents");
        let enum_type = self.get_declared_type_of_enum(parent)?;
        if let Some(declared) = self.links.symbol(symbol).declared_type.resolved() {
            return Ok(declared);
        }
        self.links.set_symbol_declared_type(
            self.speculation_depth,
            symbol,
            LinkSlot::Resolved(enum_type),
        );
        Ok(enum_type)
    }

    /// tsc-port: getTypeOfEnumMember @6.0.3
    /// tsc-hash: 192449a6e0e94c96d5c45accd89b12ed9f2f371748ec65e920acd099eecace29
    /// tsc-span: _tsc.js:56860-56863
    fn get_type_of_enum_member(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.symbol(symbol).type_of_symbol.resolved() {
            return Ok(cached);
        }
        let declared = self.get_declared_type_of_enum_member(symbol)?;
        self.links
            .set_symbol_type(self.speculation_depth, symbol, LinkSlot::Resolved(declared));
        Ok(declared)
    }

    /// tsc-port: getParentOfSymbol @6.0.3
    /// tsc-hash: 780aba46e2063ad2b64047a5d9ae0fc705fedf00e2d63d633c4fb8fc8e53a088
    /// tsc-span: _tsc.js:49942-49944
    ///
    /// getLateBoundSymbol is the identity until 5.5 late binding.
    pub(crate) fn get_parent_of_symbol(&self, symbol: SymbolId) -> Option<SymbolId> {
        let parent = self.binder.symbol(symbol).parent?;
        Some(self.get_merged_symbol(parent))
    }

    /// tsc-port: getTypeOfParameter @6.0.3
    /// tsc-hash: 94d4e1585e05140cd7efeea51c3ce5d865e1405c5c3e290d5d6fec5cc3af1171
    /// tsc-span: _tsc.js:78111-78120
    pub fn get_type_of_parameter(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        let declared = self.get_type_of_symbol(symbol)?;
        let declaration = self.binder.symbol(symbol).value_declaration;
        let is_optional = declaration.is_some_and(|declaration| {
            matches!(
                self.data_of(declaration),
                NodeData::Parameter(data)
                    if data.question_token.is_some() || data.initializer.is_some()
            )
        });
        Ok(self
            .tables
            .add_optionality(declared, /*is_property*/ false, is_optional))
    }

    // ---- signatures ----

    /// tsc-port: getSignaturesOfSymbol @6.0.3
    /// tsc-hash: 99c125ca31b9dfdc2433ad7853d7ec76162048bb302bac7f515e034837d14886
    /// tsc-span: _tsc.js:59719-59749
    ///
    /// The overload-implementation skip (59725-59730) never fires in
    /// M3: annotation-context signatures are bodyless. JSDoc @type/
    /// @overload branches are JS-only.
    pub fn get_signatures_of_symbol(
        &mut self,
        symbol: Option<SymbolId>,
    ) -> CheckResult2<Vec<SignatureId>> {
        let Some(symbol) = symbol else {
            return Ok(Vec::new());
        };
        let declarations = self.binder.symbol(symbol).declarations.clone();
        let mut result = Vec::new();
        for (i, &declaration) in declarations.iter().enumerate() {
            if !node_util::is_function_like_kind(self.kind_of(declaration)) {
                continue;
            }
            // 59725-59730: an overload IMPLEMENTATION immediately
            // following its final overload signature contributes no
            // signature of its own.
            if i > 0
                && node_util::body_of(self.binder.source_of_node(declaration), declaration)
                    .is_some()
            {
                let previous = declarations[i - 1];
                let same_parent = self.parent_of(declaration) == self.parent_of(previous);
                let same_kind = self.kind_of(declaration) == self.kind_of(previous);
                let source = self.binder.source_of_node(declaration);
                let adjacent = source.arena.node(declaration).pos
                    == self
                        .binder
                        .source_of_node(previous)
                        .arena
                        .node(previous)
                        .end;
                if same_parent && same_kind && adjacent {
                    continue;
                }
            }
            result.push(self.get_signature_from_declaration(declaration)?);
        }
        Ok(result)
    }

    /// tsc-port: getSignatureFromDeclaration @6.0.3
    /// tsc-hash: f74d65ac24febb2f8dc4dfe2c0cc3fc26cfd01a4f004385457085decf95922cd
    /// tsc-span: _tsc.js:59569-59651
    ///
    /// tsc-port: createSignature @6.0.3
    /// tsc-hash: 5dd5f4c5474933718e431fe1de3bb7f541e50391206aae67eb4f97fc1b7d036a
    /// tsc-span: _tsc.js:57852-57867
    ///
    /// M3 slice + 5.2e generics: annotation-only signatures with
    /// typeParameters (getTypeParametersFromDeclaration, 59630).
    /// IIFE/JS/JSDoc branches and accessor this-borrowing are dead
    /// here; the Constructor classType arm rides on class members
    /// (5.3, kinds gate below).
    pub fn get_signature_from_declaration(
        &mut self,
        declaration: NodeId,
    ) -> CheckResult2<SignatureId> {
        if let Some(cached) = self.links.node(declaration).resolved_signature.resolved() {
            return Ok(cached);
        }
        let (type_parameters, parameter_list, modifiers) = match self.data_of(declaration) {
            NodeData::FunctionType(data) => (data.type_parameters, data.parameters, None),
            NodeData::ConstructorType(data) => {
                (data.type_parameters, data.parameters, data.modifiers)
            }
            NodeData::CallSignature(data) => (data.type_parameters, data.parameters, None),
            NodeData::ConstructSignature(data) => (data.type_parameters, data.parameters, None),
            NodeData::MethodSignature(data) => (data.type_parameters, data.parameters, None),
            // 5.5f: value function-likes (body-carrying kinds). The
            // Constructor classType/parameter-property arms are 5.8
            // (class declaration band).
            NodeData::FunctionDeclaration(data) => (data.type_parameters, data.parameters, None),
            NodeData::FunctionExpression(data) => (data.type_parameters, data.parameters, None),
            NodeData::ArrowFunction(data) => (data.type_parameters, data.parameters, None),
            NodeData::MethodDeclaration(data) => (data.type_parameters, data.parameters, None),
            NodeData::GetAccessor(data) => (data.type_parameters, data.parameters, None),
            NodeData::SetAccessor(data) => (data.type_parameters, data.parameters, None),
            _ => {
                return Err(Unsupported::new(format!(
                    "signature declaration kind {:?} (M4 5.8)",
                    self.kind_of(declaration)
                )))
            }
        };
        let type_parameters = {
            let _ = type_parameters;
            let declarations = self.type_parameter_declarations_of(declaration);
            let parameters = self.append_type_parameters(Vec::new(), &declarations);
            (!parameters.is_empty()).then_some(parameters)
        };
        let mut flags = SignatureFlags::from_bits(0);
        let mut parameters: Vec<SymbolId> = Vec::new();
        let mut this_parameter: Option<SymbolId> = None;
        let mut min_argument_count = 0u32;
        for (i, &parameter) in self.nodes_of(parameter_list).iter().enumerate() {
            let NodeData::Parameter(data) = self.data_of(parameter).clone() else {
                return Err(Unsupported::new("malformed signature parameter"));
            };
            let Some(parameter_symbol) = self.node_symbol(parameter) else {
                return Err(Unsupported::new("unbound signature parameter symbol"));
            };
            let is_this =
                i == 0 && data.name.and_then(|name| self.identifier_text(name)) == Some("this");
            if is_this {
                this_parameter = Some(parameter_symbol);
            } else {
                parameters.push(parameter_symbol);
            }
            if data
                .r#type
                .is_some_and(|annotation| self.kind_of(annotation) == SyntaxKind::LiteralType)
            {
                flags |= SignatureFlags::HAS_LITERAL_TYPES;
            }
            // minArgumentCount (59613-59616): last non-optional,
            // non-initialized, non-rest parameter. The IIFE arm
            // (59614: over-declared parameters of an immediately
            // invoked function with fewer arguments and no annotation
            // are optional) rides the node_util walk.
            let iife_optional = data.r#type.is_none()
                && node_util::get_immediately_invoked_function_expression(
                    self.binder.source_of_node(declaration),
                    declaration,
                )
                .is_some_and(|iife| {
                    let argument_count = match self.data_of(iife) {
                        NodeData::CallExpression(call) => self.nodes_of(call.arguments).len(),
                        _ => 0,
                    };
                    parameters.len() > argument_count
                });
            let is_optional_parameter = data.question_token.is_some()
                || data.initializer.is_some()
                || data.dot_dot_dot_token.is_some()
                || iife_optional;
            if !is_optional_parameter {
                min_argument_count = parameters.len() as u32;
            }
        }
        // 59619-59626: accessors with a bindable name borrow the OTHER
        // accessor's annotated this-parameter when they lack their own.
        if matches!(
            self.kind_of(declaration),
            SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
        ) && this_parameter.is_none()
            && !self.has_late_bindable_ast_name(declaration)
            && !node_util::has_dynamic_name(self.binder.source_of_node(declaration), declaration)
        {
            let other_kind = if self.kind_of(declaration) == SyntaxKind::GetAccessor {
                SyntaxKind::SetAccessor
            } else {
                SyntaxKind::GetAccessor
            };
            let symbol = self.get_symbol_of_declaration(declaration)?;
            let other = self
                .binder
                .symbol(symbol)
                .declarations
                .iter()
                .copied()
                .find(|&d| self.kind_of(d) == other_kind);
            if let Some(other) = other {
                // getAnnotatedAccessorThisParameter: the other
                // accessor's `this` parameter symbol, when declared.
                let other_parameters = match self.data_of(other) {
                    NodeData::GetAccessor(data) => data.parameters,
                    NodeData::SetAccessor(data) => data.parameters,
                    _ => None,
                };
                if let Some(&first) = self.nodes_of(other_parameters).first() {
                    let is_this = matches!(
                        self.data_of(first),
                        NodeData::Parameter(data)
                            if data.name.and_then(|name| self.identifier_text(name))
                                == Some("this")
                    );
                    if is_this {
                        this_parameter = self.node_symbol(first);
                    }
                }
            }
        }
        let last_is_rest = self
            .nodes_of(parameter_list)
            .last()
            .is_some_and(|&parameter| {
                matches!(
                    self.data_of(parameter),
                    NodeData::Parameter(data) if data.dot_dot_dot_token.is_some()
                )
            });
        if last_is_rest {
            flags |= SignatureFlags::HAS_REST_PARAMETER;
        }
        if self.kind_of(declaration) == SyntaxKind::ConstructorType
            && self
                .nodes_of(modifiers)
                .iter()
                .any(|&modifier| self.kind_of(modifier) == SyntaxKind::AbstractKeyword)
        {
            flags |= SignatureFlags::ABSTRACT;
        }
        let signature = Signature {
            declaration: Some(declaration),
            flags,
            type_parameters,
            parameters,
            this_parameter,
            min_argument_count,
            resolved_return_type: LinkSlot::Vacant,
            from_method: self.kind_of(declaration) == SyntaxKind::MethodSignature,
            target: None,
            mapper: None,
            instantiations: std::collections::HashMap::new(),
            erased_signature_cache: None,
            composite_kind: None,
            composite_signatures: None,
            optional_call_signature_cache: (None, None),
        };
        let id = self.alloc_signature(signature);
        self.links.set_node_resolved_signature(
            self.speculation_depth,
            declaration,
            LinkSlot::Resolved(id),
        );
        Ok(id)
    }

    /// tsc-port: getReturnTypeOfSignature @6.0.3
    /// tsc-hash: e265945ed88a31c9a144fbc92e35e37e90cc63ce624fec90da4fc7326b56a644
    /// tsc-span: _tsc.js:59810-59841
    ///
    /// tsc-port: getReturnTypeFromAnnotation @6.0.3
    /// tsc-hash: 59a361ad1f8c3e47696f66ca558f78d32f511ea571477db96df219a130d55c5a
    /// tsc-span: _tsc.js:59842-59871
    ///
    /// Slice: the instantiation-target arm (5.2) + the annotation
    /// branch + the bodyless anyType fallback (59815: nodeIsMissing(body)
    /// → anyType). Composite signatures ride on 5.3 union members,
    /// call-chain optionality and body inference on 5.5/M6. Cycles run
    /// on the resolution stack (59812/59821); an Err unwind pops the
    /// stack and leaves the slot Vacant (M3-review Resolving-dangling
    /// fix).
    pub fn get_return_type_of_signature(&mut self, id: SignatureId) -> CheckResult2<TypeId> {
        if let Some(resolved) = self.signature_of(id).resolved_return_type.resolved() {
            return Ok(resolved);
        }
        if !self.push_type_resolution(
            crate::state::ResolutionTarget::Signature(id),
            tsrs2_types::TypeSystemPropertyName::RESOLVED_RETURN_TYPE,
        ) {
            return Ok(self.tables.intrinsics.error);
        }
        let declaration = self.signature_of(id).declaration;
        let annotation = declaration.and_then(|declaration| match self.data_of(declaration) {
            NodeData::FunctionType(data) => data.r#type,
            NodeData::ConstructorType(data) => data.r#type,
            NodeData::CallSignature(data) => data.r#type,
            NodeData::ConstructSignature(data) => data.r#type,
            NodeData::MethodSignature(data) => data.r#type,
            NodeData::FunctionDeclaration(data) => data.r#type,
            NodeData::FunctionExpression(data) => data.r#type,
            NodeData::ArrowFunction(data) => data.r#type,
            NodeData::MethodDeclaration(data) => data.r#type,
            NodeData::GetAccessor(data) => data.r#type,
            NodeData::SetAccessor(_) => None,
            _ => None,
        });
        let target = self.signature_of(id).target;
        let composite = self.signature_of(id).composite_signatures.clone();
        let computed = match (target, composite) {
            // 59815: signature.target → instantiate the target's
            // return type through signature.mapper.
            (Some(target), _) => {
                let mapper = self.signature_of(id).mapper;
                self.get_return_type_of_signature(target)
                    .and_then(|target_return| self.instantiate_type(target_return, mapper))
            }
            // 59815: composite signatures — the union/intersection of
            // the member returns (Subtype reduction on the union arm),
            // instantiated through signature.mapper.
            (None, Some(composite)) => (|state: &mut Self| {
                let kind = state.signature_of(id).composite_kind;
                let mut returns = Vec::with_capacity(composite.len());
                for &member in &composite {
                    returns.push(state.get_return_type_of_signature(member)?);
                }
                let combined = if kind.is_some_and(|kind| kind.intersects(TypeFlags::INTERSECTION))
                {
                    state.get_intersection_type(&returns, IntersectionFlags::NONE)?
                } else {
                    state.get_union_type_ex(&returns, UnionReduction::Subtype)?
                };
                let mapper = state.signature_of(id).mapper;
                state.instantiate_type(combined, mapper)
            })(self),
            // 59815 tail: getReturnTypeFromAnnotation(declaration) ||
            // (nodeIsMissing(body) ? anyType : getReturnTypeFromBody).
            // getReturnTypeFromAnnotation also covers the Constructor
            // class-type arm and the getter's setter-annotation borrow.
            (None, None) => (|state: &mut Self| {
                let Some(declaration) = declaration else {
                    // Synthetic signatures (returnOnly/unknown) carry a
                    // pre-seeded resolvedReturnType and never get here;
                    // annotation-context bodyless shapes answer any.
                    return Ok(state.tables.intrinsics.any);
                };
                if let Some(annotated) = state.get_return_type_from_annotation(declaration)? {
                    return Ok(annotated);
                }
                // The annotation-context kinds (FunctionType/
                // CallSignature/...) sit outside getReturnTypeFrom-
                // Annotation's declaration match — their typeNode read
                // is the `annotation` extraction above.
                if let Some(annotation) = annotation {
                    return state.get_type_from_type_node(annotation);
                }
                let body =
                    node_util::body_of(state.binder.source_of_node(declaration), declaration);
                match body {
                    None => Ok(state.tables.intrinsics.any),
                    Some(_) => {
                        state.get_return_type_from_body(declaration, tsrs2_types::CheckMode::NORMAL)
                    }
                }
            })(self),
        };
        let resolved = match computed {
            Ok(resolved) => resolved,
            Err(err) => {
                self.pop_type_resolution();
                return Err(err);
            }
        };
        // 59816-59820: call-chain signatures adjust return
        // optionality (getOptionalCallSignature's flagged clones,
        // M4 5.7).
        let chain_flags = self.signature_of(id).flags;
        let adjusted = if chain_flags.intersects(SignatureFlags::IS_INNER_CALL_CHAIN) {
            self.add_optional_type_marker(resolved)
        } else if chain_flags.intersects(SignatureFlags::IS_OUTER_CALL_CHAIN) {
            self.get_optional_type(resolved, /*is_property*/ false)
        } else {
            Ok(resolved)
        };
        let resolved = match adjusted {
            Ok(resolved) => resolved,
            Err(err) => {
                self.pop_type_resolution();
                return Err(err);
            }
        };
        let resolved = if self.pop_type_resolution() {
            resolved
        } else {
            // 59821-59836: a deeper frame flagged the cycle.
            if let Some(type_node) = annotation {
                self.error_at(
                    Some(type_node),
                    &diagnostics::Return_type_annotation_circularly_references_itself,
                    &[],
                );
            } else if self
                .options
                .strict_option_value(self.options.no_implicit_any)
            {
                let name = declaration.and_then(|declaration| self.name_of_node(declaration));
                match name {
                    Some(name) => {
                        let display = tsrs2_binder::node_util::declaration_name_to_string(
                            self.binder
                                .source_of_node(declaration.expect("named implies declared")),
                            Some(name),
                        );
                        self.error_at(
                            Some(name),
                            &diagnostics::_0_implicitly_has_return_type_any_because_it_does_not_have_a_return_type_annotation_and_is_referenced_directly_or_indirectly_in_one_of_its_return_expressions,
                            &[&display],
                        );
                    }
                    None => {
                        self.error_at(
                            declaration,
                            &diagnostics::Function_implicitly_has_return_type_any_because_it_does_not_have_a_return_type_annotation_and_is_referenced_directly_or_indirectly_in_one_of_its_return_expressions,
                            &[],
                        );
                    }
                }
            }
            self.tables.intrinsics.any
        };
        self.signatures[id.0 as usize].resolved_return_type = LinkSlot::Resolved(resolved);
        Ok(resolved)
    }
}

/// tsc-port: isReservedMemberName @6.0.3
/// tsc-hash: 6e93c419462cea22e393d89e2df487745553e2aab4363501e4c436f1d5a13b84
/// tsc-span: _tsc.js:50142-50144
/// tsc-port: getExcludedSymbolFlags @6.0.3
/// tsc-hash: 44af025b45ba77e5268ef6b5eb0d490623d4607c7085362e5f1f0f0436f2da41
/// tsc-span: _tsc.js:47669-47688
fn get_excluded_symbol_flags(flags: SymbolFlags) -> SymbolFlags {
    let mut result = 0i32;
    if flags.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE) {
        result |= SymbolFlags::BLOCK_SCOPED_VARIABLE_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::FUNCTION_SCOPED_VARIABLE) {
        result |= SymbolFlags::FUNCTION_SCOPED_VARIABLE_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::PROPERTY) {
        result |= SymbolFlags::PROPERTY_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::ENUM_MEMBER) {
        result |= SymbolFlags::ENUM_MEMBER_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::FUNCTION) {
        result |= SymbolFlags::FUNCTION_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::CLASS) {
        result |= SymbolFlags::CLASS_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::INTERFACE) {
        result |= SymbolFlags::INTERFACE_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::REGULAR_ENUM) {
        result |= SymbolFlags::REGULAR_ENUM_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::CONST_ENUM) {
        result |= SymbolFlags::CONST_ENUM_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::VALUE_MODULE) {
        result |= SymbolFlags::VALUE_MODULE_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::METHOD) {
        result |= SymbolFlags::METHOD_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::GET_ACCESSOR) {
        result |= SymbolFlags::GET_ACCESSOR_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::SET_ACCESSOR) {
        result |= SymbolFlags::SET_ACCESSOR_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::TYPE_PARAMETER) {
        result |= SymbolFlags::TYPE_PARAMETER_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::TYPE_ALIAS) {
        result |= SymbolFlags::TYPE_ALIAS_EXCLUDES.bits();
    }
    if flags.intersects(SymbolFlags::ALIAS) {
        result |= SymbolFlags::ALIAS_EXCLUDES.bits();
    }
    SymbolFlags::from_bits(result)
}

fn is_reserved_member_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.first() == Some(&b'_')
        && bytes.get(1) == Some(&b'_')
        && bytes.get(2) != Some(&b'_')
        && bytes.get(2) != Some(&b'@')
        && bytes.get(2) != Some(&b'#')
}

#[allow(dead_code)] // retired by the 5.5f isFunctionLike filter
fn is_m3_signature_declaration_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::CallSignature
            | SyntaxKind::ConstructSignature
            | SyntaxKind::MethodSignature
            | SyntaxKind::FunctionType
            | SyntaxKind::ConstructorType
    )
}

/// The scanner already normalized numeric literal text to its value
/// form (tsc node.text = token value); this parses that value string.
pub(crate) fn parse_numeric_literal_text(text: &str) -> CheckResult2<f64> {
    text.parse::<f64>()
        .map_err(|_| Unsupported::new(format!("unparsable numeric literal text {text:?}")))
}

/// The decimal slice of parsePseudoBigInt (18909-18964): annotation
/// literals reach the checker in decimal form; other radixes arrive
/// with expression checking (M6).
fn parse_pseudo_bigint_text(text: &str, negative: bool) -> CheckResult2<PseudoBigInt> {
    let digits = text.strip_suffix('n').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Unsupported::new(format!(
            "non-decimal bigint literal text {text:?} (parsePseudoBigInt radix support, M6)"
        )));
    }
    let trimmed = digits.trim_start_matches('0');
    let base10_value = if trimmed.is_empty() { "0" } else { trimmed };
    Ok(PseudoBigInt {
        negative: negative && base10_value != "0",
        base10_value: base10_value.to_owned(),
    })
}

// ---- M4 5.5b: binding-pattern types (L56468-56552) ----

impl<'a> CheckerState<'a> {
    /// tsc-port: getTypeFromBindingElement @6.0.3
    /// tsc-hash: 5aed861556c2cfa2ed5aa01177ce6375ddaa04f810aeadc7d48c8673cd09d2d9
    /// tsc-span: _tsc.js:56468-56486
    ///
    pub(crate) fn get_type_from_binding_element(
        &mut self,
        element: NodeId,
        include_pattern_in_type: bool,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        let NodeData::BindingElement(data) = self.data_of(element) else {
            return Err(Unsupported::new(
                "getTypeFromBindingElement over a non-binding-element (parse recovery)",
            ));
        };
        let (initializer, name) = (data.initializer, data.name);
        let source = self.binder.source_of_node(element);
        if initializer.is_some() {
            let contextual_type = match name {
                Some(name) if node_util::is_binding_pattern(source, name) => self
                    .get_type_from_binding_pattern(
                        name, /*include_pattern_in_type*/ true, /*report_errors*/ false,
                    )?,
                _ => self.tables.intrinsics.unknown,
            };
            let initializer_type = self.check_declaration_initializer(
                element,
                tsrs2_types::CheckMode::NORMAL,
                Some(contextual_type),
            )?;
            let widened =
                self.get_widened_literal_type_for_initializer(element, initializer_type)?;
            return Ok(self.tables.add_optionality(
                widened, /*is_property*/ false, /*is_optional*/ true,
            ));
        }
        if let Some(name) = name {
            if node_util::is_binding_pattern(source, name) {
                return self.get_type_from_binding_pattern(
                    name,
                    include_pattern_in_type,
                    report_errors,
                );
            }
        }
        if report_errors && !self.declaration_belongs_to_private_ambient_member(element) {
            let any = self.tables.intrinsics.any;
            self.report_implicit_any(element, any, /*widening_kind*/ None)?;
        }
        Ok(if include_pattern_in_type {
            self.tables.intrinsics.non_inferrable_any
        } else {
            self.tables.intrinsics.any
        })
    }

    /// tsc-port: getTypeFromObjectBindingPattern @6.0.3
    /// tsc-hash: 10ec9571c60f25c26106990c44782d1d2407ed3aaaf0bc3e4e5e8a551e844072
    /// tsc-span: _tsc.js:56487-56527
    fn get_type_from_object_binding_pattern(
        &mut self,
        pattern: NodeId,
        include_pattern_in_type: bool,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        let elements: Vec<NodeId> = match self.data_of(pattern) {
            NodeData::ObjectBindingPattern(data) => self.nodes_of(data.elements),
            _ => Vec::new(),
        };
        let mut members = tsrs2_binder::SymbolTable::default();
        let mut properties: Vec<SymbolId> = Vec::new();
        let mut string_index_info: Option<crate::state::IndexInfo> = None;
        let mut object_flags =
            ObjectFlags::OBJECT_LITERAL | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL;
        for e in elements {
            let NodeData::BindingElement(data) = self.data_of(e) else {
                continue;
            };
            let (dot_dot_dot, property_name, element_name, initializer) = (
                data.dot_dot_dot_token.is_some(),
                data.property_name,
                data.name,
                data.initializer,
            );
            let Some(name) = property_name.or(element_name) else {
                continue;
            };
            if dot_dot_dot {
                string_index_info = Some(crate::state::IndexInfo {
                    key_type: self.tables.intrinsics.string,
                    value_type: self.tables.intrinsics.any,
                    is_readonly: false,
                    declaration: None,
                    components: None,
                });
                continue;
            }
            let expr_type = self.get_literal_type_from_property_name(name)?;
            let Some(text) = self.property_name_from_type_usable(expr_type) else {
                object_flags |= ObjectFlags::OBJECT_LITERAL_PATTERN_WITH_COMPUTED_PROPERTIES;
                continue;
            };
            let flags = SymbolFlags::PROPERTY
                | if initializer.is_some() {
                    SymbolFlags::OPTIONAL
                } else {
                    SymbolFlags::from_bits(0)
                };
            let symbol = self.binder.create_symbol(flags, text.clone());
            let element_type =
                self.get_type_from_binding_element(e, include_pattern_in_type, report_errors)?;
            self.links.set_symbol_type(
                self.speculation_depth,
                symbol,
                crate::links::LinkSlot::Resolved(element_type),
            );
            members.insert(text, symbol);
            properties.push(symbol);
        }
        let id = self
            .tables
            .create_type(TypeFlags::OBJECT, tsrs2_types::TypeData::Object);
        self.tables.type_mut(id).object_flags = ObjectFlags::ANONYMOUS | object_flags;
        let members_id = self.alloc_members(crate::state::ResolvedMembers {
            members,
            properties,
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            index_infos: string_index_info.into_iter().collect(),
        });
        self.links.set_type_members(
            self.speculation_depth,
            id,
            crate::links::LinkSlot::Resolved(members_id),
        );
        if include_pattern_in_type {
            self.links
                .set_type_pattern(self.speculation_depth, id, pattern);
            // objectFlags |= ContainsObjectOrArrayLiteral (already set).
        }
        Ok(id)
    }

    /// tsc-port: getTypeFromArrayBindingPattern @6.0.3
    /// tsc-hash: f2e5cfbc78da5b22bdf82cddbae73e99f2e64febd149ed62eaea4623eb01638d
    /// tsc-span: _tsc.js:56528-56545
    fn get_type_from_array_binding_pattern(
        &mut self,
        pattern: NodeId,
        include_pattern_in_type: bool,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        let elements: Vec<NodeId> = match self.data_of(pattern) {
            NodeData::ArrayBindingPattern(data) => self.nodes_of(data.elements),
            _ => Vec::new(),
        };
        let rest_element = elements.last().copied().filter(|&last| {
            matches!(
                self.data_of(last),
                NodeData::BindingElement(data) if data.dot_dot_dot_token.is_some()
            )
        });
        if elements.is_empty() || (elements.len() == 1 && rest_element.is_some()) {
            return if self.options.emit_script_target() >= tsrs2_types::ScriptTarget::ES2015 {
                self.create_iterable_type(self.tables.intrinsics.any)
            } else {
                self.any_array_type()
            };
        }
        let mut element_types: Vec<TypeId> = Vec::with_capacity(elements.len());
        for &e in &elements {
            element_types.push(if self.kind_of(e) == SyntaxKind::OmittedExpression {
                self.tables.intrinsics.any
            } else {
                self.get_type_from_binding_element(e, include_pattern_in_type, report_errors)?
            });
        }
        let min_length = elements
            .iter()
            .rposition(|&e| {
                !(Some(e) == rest_element
                    || self.kind_of(e) == SyntaxKind::OmittedExpression
                    || self.binding_element_has_default_value(e))
            })
            .map_or(0, |index| index + 1);
        let element_flags: Vec<tsrs2_types::ElementFlags> = elements
            .iter()
            .enumerate()
            .map(|(i, &e)| {
                if Some(e) == rest_element {
                    tsrs2_types::ElementFlags::REST
                } else if i >= min_length {
                    tsrs2_types::ElementFlags::OPTIONAL
                } else {
                    tsrs2_types::ElementFlags::REQUIRED
                }
            })
            .collect();
        let mut result =
            self.create_tuple_type_forced(&element_types, Some(&element_flags), false, None)?;
        if include_pattern_in_type {
            result = self.tables.clone_type_reference(result);
            self.links
                .set_type_pattern(self.speculation_depth, result, pattern);
            let with_literal_flag =
                self.tables.object_flags_of(result) | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL;
            self.tables.type_mut(result).object_flags = with_literal_flag;
        }
        Ok(result)
    }

    /// hasDefaultValue's binding-element half (the expression halves
    /// live on the driver band).
    fn binding_element_has_default_value(&self, e: NodeId) -> bool {
        matches!(
            self.data_of(e),
            NodeData::BindingElement(data) if data.initializer.is_some()
        )
    }

    /// tsc-port: getTypeFromBindingPattern @6.0.3
    /// tsc-hash: 494f309ef98ab15a4a03bcd5949aa78ad57237d71845da6aa8ca17cf9374fa4e
    /// tsc-span: _tsc.js:56546-56552
    ///
    /// The includePatternInType push feeds checkIdentifier's
    /// nonInferrableAnyType circularity arm (contextualBindingPatterns
    /// membership) — live since 5.5a, populated from here.
    pub(crate) fn get_type_from_binding_pattern(
        &mut self,
        pattern: NodeId,
        include_pattern_in_type: bool,
        report_errors: bool,
    ) -> CheckResult2<TypeId> {
        if include_pattern_in_type {
            self.contextual_binding_patterns.push(pattern);
        }
        let result = if self.kind_of(pattern) == SyntaxKind::ObjectBindingPattern {
            self.get_type_from_object_binding_pattern(
                pattern,
                include_pattern_in_type,
                report_errors,
            )
        } else {
            self.get_type_from_array_binding_pattern(
                pattern,
                include_pattern_in_type,
                report_errors,
            )
        };
        if include_pattern_in_type {
            self.contextual_binding_patterns.pop();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_binder::bind_source_file;
    use tsrs2_syntax::{parse_source_file, LanguageVariant, ParseOptions, SourceFile};
    use tsrs2_types::{
        CompilerOptions, ElementFlags, ObjectFlags, SignatureFlags, TypeData, TypeFlags, TypeId,
    };

    use crate::relpin::find_probe_annotation;
    use crate::state::CheckerState;

    fn parse(text: &str) -> SourceFile {
        let source = parse_source_file(
            "annotate-test.ts".to_owned(),
            text.to_owned(),
            ParseOptions {
                language_variant: LanguageVariant::Standard,
                javascript_file: false,
                ..ParseOptions::default()
            },
            None,
        );
        assert!(
            source.parse_diagnostics.is_empty(),
            "test source must parse cleanly: {:?}",
            source.parse_diagnostics
        );
        source
    }

    fn annotation_type(state: &mut CheckerState, name: &str) -> TypeId {
        let annotation = find_probe_annotation(state.binder.source(0), name)
            .expect("declared var with annotation");
        state
            .get_type_from_type_node(annotation)
            .expect("annotation resolves in the M3 slice")
    }

    fn with_state<R>(text: &str, run: impl FnOnce(&mut CheckerState) -> R) -> R {
        let options = CompilerOptions::default();
        let source = parse(text);
        let binder = bind_source_file(&source, &options);
        let mut state = CheckerState::new(&source, &binder, &options);
        run(&mut state)
    }

    #[test]
    fn union_annotation_reduces_string_literals_matched_by_templates() {
        with_state(
            "declare var a: \"abc\" | `a${string}`;\ndeclare var b: `a${string}`;\n",
            |state| {
                let a = annotation_type(state, "a");
                let b = annotation_type(state, "b");
                // The annotation path routes through the checker union
                // twin, so removeStringLiteralsMatchedByTemplateLiterals
                // collapses the union to the template itself, like
                // getUnionTypeWorker (61547-61549) does.
                assert_eq!(a, b);
            },
        );
    }

    #[test]
    fn distinct_type_literals_are_distinct_types() {
        with_state(
            "declare var a: { x: number };\ndeclare var b: { x: number };\n",
            |state| {
                let a = annotation_type(state, "a");
                let b = annotation_type(state, "b");
                assert_ne!(a, b, "anonymous object types never intern structurally");
                // Re-reading the SAME node returns the cached type.
                assert_eq!(annotation_type(state, "a"), a);
            },
        );
    }

    #[test]
    fn literal_type_nodes_yield_regular_literals() {
        with_state("declare var a: 1;\ndeclare var b: \"x\";\n", |state| {
            let one = annotation_type(state, "a");
            assert!(!state.tables.is_fresh_literal_type(one));
            assert_eq!(state.tables.get_regular_type_of_literal_type(one), one);
            let x = annotation_type(state, "b");
            assert!(state
                .tables
                .flags_of(x)
                .intersects(TypeFlags::STRING_LITERAL));
        });
    }

    #[test]
    fn union_annotations_intern_by_member_set() {
        with_state("declare var a: 1 | 2;\ndeclare var b: 2 | 1;\n", |state| {
            let a = annotation_type(state, "a");
            let b = annotation_type(state, "b");
            assert_eq!(a, b);
            assert!(state.tables.flags_of(a).intersects(TypeFlags::UNION));
        });
    }

    #[test]
    fn tuple_annotations_build_normalized_references() {
        with_state(
            "declare var a: [number, string?];\ndeclare var b: readonly [number];\ndeclare var c: [number, ...[string, boolean]];\ndeclare var d: [number, string, boolean];\n",
            |state| {
                let a = annotation_type(state, "a");
                assert!(state.tables.is_tuple_type(a));
                let target = state.tables.reference_target(a);
                let TypeData::TupleTarget(data) = &state.tables.type_of(target).data else {
                    panic!("tuple reference targets a tuple target");
                };
                assert_eq!(
                    data.element_flags.as_ref(),
                    [ElementFlags::REQUIRED, ElementFlags::OPTIONAL]
                );
                assert_eq!(data.min_length, 1);
                assert!(!data.readonly);
                // Optional element type widened with undefined (strict).
                let args = state.tables.type_arguments(a).to_vec();
                assert!(state.tables.flags_of(args[1]).intersects(TypeFlags::UNION));

                let b = annotation_type(state, "b");
                let b_target = state.tables.reference_target(b);
                let TypeData::TupleTarget(data) = &state.tables.type_of(b_target).data else {
                    panic!("tuple reference targets a tuple target");
                };
                assert!(data.readonly);

                // Variadic tuple spread normalizes to the flat tuple.
                assert_eq!(annotation_type(state, "c"), annotation_type(state, "d"));
            },
        );
    }

    #[test]
    fn recursive_interfaces_resolve_declared_types_and_members() {
        with_state(
            "interface A { next: B }\ninterface B { next: A }\ndeclare var a: A;\ndeclare var b: B;\n",
            |state| {
                let a = annotation_type(state, "a");
                let b = annotation_type(state, "b");
                assert_ne!(a, b);
                assert!(state
                    .tables
                    .object_flags_of(a)
                    .intersects(ObjectFlags::INTERFACE));
                let members = state
                    .resolve_structured_type_members(a)
                    .expect("interface members resolve");
                let members = state.members_of(members).clone();
                assert_eq!(members.properties.len(), 1);
                let next = members.properties[0];
                let next_type = state.get_type_of_symbol(next).expect("property type");
                assert_eq!(next_type, b, "A.next is B's declared type");
            },
        );
    }

    #[test]
    fn method_members_get_anonymous_types_with_call_signatures() {
        with_state(
            "declare var a: { m(x: 1): void, p: (x: number) => void };\n",
            |state| {
                let a = annotation_type(state, "a");
                let members_id = state
                    .resolve_structured_type_members(a)
                    .expect("type literal members resolve");
                let members = state.members_of(members_id).clone();
                assert_eq!(members.properties.len(), 2);

                let method_type = state
                    .get_type_of_symbol(members.properties[0])
                    .expect("method type");
                let method_members_id = state
                    .resolve_structured_type_members(method_type)
                    .expect("method members resolve");
                let method_members = state.members_of(method_members_id).clone();
                assert_eq!(method_members.call_signatures.len(), 1);
                let signature = state
                    .signature_of(method_members.call_signatures[0])
                    .clone();
                assert!(signature.from_method);
                assert!(signature.flags.contains(SignatureFlags::HAS_LITERAL_TYPES));
                assert_eq!(signature.min_argument_count, 1);

                let property_type = state
                    .get_type_of_symbol(members.properties[1])
                    .expect("function property type");
                let property_members_id = state
                    .resolve_structured_type_members(property_type)
                    .expect("function type members resolve");
                let property_members = state.members_of(property_members_id).clone();
                assert_eq!(property_members.call_signatures.len(), 1);
                assert!(
                    !state
                        .signature_of(property_members.call_signatures[0])
                        .from_method
                );
            },
        );
    }

    #[test]
    fn index_signatures_produce_index_infos() {
        with_state(
            "declare var a: { readonly [k: string]: number };\ndeclare var b: { [k: symbol]: number };\n",
            |state| {
                for (name, key) in [("a", TypeFlags::STRING), ("b", TypeFlags::ES_SYMBOL)] {
                    let ty = annotation_type(state, name);
                    let members_id = state
                        .resolve_structured_type_members(ty)
                        .expect("index members resolve");
                    let members = state.members_of(members_id).clone();
                    assert_eq!(members.index_infos.len(), 1);
                    assert!(state
                        .tables
                        .flags_of(members.index_infos[0].key_type)
                        .intersects(key));
                    assert_eq!(members.index_infos[0].is_readonly, name == "a");
                }
            },
        );
    }

    #[test]
    fn template_annotations_fold_literal_spans() {
        with_state(
            "declare var a: `a${string}`;\ndeclare var b: `a${\"b\"}c`;\n",
            |state| {
                let a = annotation_type(state, "a");
                assert!(state
                    .tables
                    .flags_of(a)
                    .intersects(TypeFlags::TEMPLATE_LITERAL));
                let b = annotation_type(state, "b");
                assert_eq!(b, state.tables.get_string_literal_type("abc"));
            },
        );
    }

    #[test]
    fn intersection_normalization_matches_tsc() {
        with_state(
            concat!(
                "declare var a: string & number;\n",
                "declare var b: 1 & 2;\n",
                "declare var c: \"a\" & string;\n",
                "declare var d: string & {};\n",
                "declare var e: unknown & string;\n",
                "declare var f: (\"a\" | \"b\") & string;\n",
                "declare var g: (string | undefined) & (number | undefined);\n",
                "declare var h: boolean & true;\n",
                "declare var i: null & number;\n",
            ),
            |state| {
                let never = state.tables.intrinsics.never;
                // DisjointDomains: string & number = never (step 2).
                assert_eq!(annotation_type(state, "a"), never);
                // Unit ∧ Unit quirk: 1 & 2 = never.
                assert_eq!(annotation_type(state, "b"), never);
                // Supertype reduction: "a" & string = "a".
                let c = annotation_type(state, "c");
                assert_eq!(c, state.tables.get_string_literal_type("a"));
                // string & {} keeps both members (noSupertypeReduction).
                let d = annotation_type(state, "d");
                let TypeData::Intersection { types } = &state.tables.type_of(d).data else {
                    panic!("string & {{}} stays an intersection");
                };
                assert_eq!(types.len(), 2);
                // unknown vanishes from intersections.
                assert_eq!(annotation_type(state, "e"), state.tables.intrinsics.string);
                // Union distribution: ("a"|"b") & string = "a" | "b".
                let f = annotation_type(state, "f");
                let a_lit = state.tables.get_string_literal_type("a");
                let b_lit = state.tables.get_string_literal_type("b");
                let expected = state
                    .tables
                    .get_union_type(&[a_lit, b_lit], tsrs2_types::UnionReduction::Literal);
                assert_eq!(f, expected);
                // The undefined pull-out: (string|undefined) & (number|undefined)
                // = (string & number) | undefined = undefined.
                assert_eq!(
                    annotation_type(state, "g"),
                    state.tables.intrinsics.undefined
                );
                // Cross product over the boolean primitive union.
                assert_eq!(
                    annotation_type(state, "h"),
                    state.tables.intrinsics.true_regular
                );
                // strictNullChecks default-on: null & number is never
                // via the nullable∧NumberLike disjoint check.
                assert_eq!(annotation_type(state, "i"), never);
            },
        );
    }

    #[test]
    fn intersections_are_insertion_order_sensitive_and_never_structural() {
        with_state(
            concat!(
                "declare var a: { x: number } & { y: string };\n",
                "declare var b: { y: string } & { x: number };\n",
                "declare var c: { x: number } & { x: number };\n",
            ),
            |state| {
                // Member order is identity: A & B differs from B & A.
                assert_ne!(annotation_type(state, "a"), annotation_type(state, "b"));
                // Structurally identical anonymous literals never dedup:
                // both members survive (the typeMembershipMap is
                // identity-keyed — the steps-doc 4.3 pin).
                let c = annotation_type(state, "c");
                let TypeData::Intersection { types } = &state.tables.type_of(c).data else {
                    panic!("distinct {{x}} literals stay an intersection");
                };
                assert_eq!(types.len(), 2);
                assert_ne!(types[0], types[1]);
            },
        );
    }

    #[test]
    fn m4_shapes_report_unsupported_not_wrong_types() {
        with_state(
            "declare var b: number extends string ? 1 : 2;\ndeclare var c: Missing;\n",
            |state| {
                let annotation =
                    find_probe_annotation(state.binder.source(0), "b").expect("annotation");
                let err = state
                    .get_type_from_type_node(annotation)
                    .expect_err("out-of-slice shape must be Unsupported");
                assert!(
                    err.reason.contains("conditional"),
                    "b: {} should mention conditional",
                    err.reason
                );
                // Unresolved names are in-slice: resolveEntityName
                // reports 2304 and the reference types as errorType.
                let annotation =
                    find_probe_annotation(state.binder.source(0), "c").expect("annotation");
                let ty = state
                    .get_type_from_type_node(annotation)
                    .expect("unresolved names type as errorType");
                assert_eq!(ty, state.tables.intrinsics.error);
            },
        );
    }

    #[test]
    #[should_panic(expected = "links writes are forbidden during speculation")]
    fn links_writes_assert_zero_speculation_depth() {
        with_state("declare var a: 1 | 2;\n", |state| {
            state.speculation_depth = 1;
            let _ = annotation_type(state, "a");
        });
    }
}

#[cfg(test)]
mod alias_and_typeof_tests {
    use tsrs2_types::{CompilerOptions, TypeFlags};

    use crate::relpin::find_probe_annotation;
    use crate::state::test_support::with_program_state;

    #[test]
    fn non_generic_type_alias_resolves_to_aliased_type() {
        with_program_state(
            &[("a.ts", "type A = string | number;\ndeclare var v: A;\n")],
            &CompilerOptions::default(),
            |state| {
                let annotation =
                    find_probe_annotation(state.binder.source(0), "v").expect("annotation");
                let resolved = state
                    .get_type_from_type_node(annotation)
                    .expect("alias resolves");
                assert!(state.tables.flags_of(resolved).intersects(TypeFlags::UNION));
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn circular_type_alias_reports_2456_and_yields_error_type() {
        with_program_state(
            &[("a.ts", "type A = A;\ndeclare var v: A;\n")],
            &CompilerOptions::default(),
            |state| {
                let annotation =
                    find_probe_annotation(state.binder.source(0), "v").expect("annotation");
                let resolved = state
                    .get_type_from_type_node(annotation)
                    .expect("circular alias resolves to errorType");
                assert!(state.tables.is_error_type(resolved));
                let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
                assert_eq!(codes, [2456]);
            },
        );
    }

    #[test]
    fn typeof_annotated_var_resolves_to_declared_type() {
        with_program_state(
            &[(
                "a.ts",
                "declare var w: \"lit\";\ndeclare var v: typeof w;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let annotation =
                    find_probe_annotation(state.binder.source(0), "v").expect("annotation");
                let resolved = state
                    .get_type_from_type_node(annotation)
                    .expect("typeof resolves");
                // Regular (non-fresh) literal type, like tsc's
                // getRegularTypeOfLiteralType tail.
                assert!(state
                    .tables
                    .flags_of(resolved)
                    .intersects(TypeFlags::STRING_LITERAL));
                assert_eq!(
                    state.tables.get_regular_type_of_literal_type(resolved),
                    resolved
                );
            },
        );
    }

    #[test]
    fn typeof_namespace_member_resolves_through_exports() {
        with_program_state(
            &[(
                "a.ts",
                "namespace N { export const K: number = 1; }\ndeclare var v: typeof N.K;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let annotation =
                    find_probe_annotation(state.binder.source(0), "v").expect("annotation");
                let resolved = state
                    .get_type_from_type_node(annotation)
                    .expect("qualified typeof resolves");
                assert_eq!(resolved, state.tables.intrinsics.number);
            },
        );
    }
}

#[cfg(test)]
mod generic_declared_type_tests {
    use tsrs2_types::{CompilerOptions, ObjectFlags, SymbolFlags, TypeData, TypeFlags};

    use crate::relpin::find_probe_annotation;
    use crate::state::test_support::with_program_state;

    #[test]
    fn generic_interface_declared_type_is_a_generic_type_target() {
        with_program_state(
            &[("a.ts", "interface I<T> { a: T }\n")],
            &CompilerOptions::default(),
            |state| {
                let symbol = state
                    .resolve_file_scope_name("I", SymbolFlags::INTERFACE)
                    .expect("I resolves");
                let declared = state
                    .get_declared_type_of_class_or_interface(symbol)
                    .expect("declared type in slice");
                let TypeData::GenericType {
                    type_parameters,
                    outer_type_parameter_count,
                    this_type,
                } = state.tables.type_of(declared).data.clone()
                else {
                    panic!("generic interfaces declare GenericType targets");
                };
                assert_eq!(type_parameters.len(), 1);
                assert_eq!(outer_type_parameter_count, 0);
                assert!(state
                    .tables
                    .object_flags_of(declared)
                    .intersects(ObjectFlags::REFERENCE));
                assert!(matches!(
                    state.tables.type_of(this_type).data,
                    TypeData::TypeParameter {
                        is_this_type: true,
                        constraint: Some(constraint),
                    } if constraint == declared
                ));
                // The instantiations map is seeded with the target:
                // referencing it with its own parameters IS the target.
                let reference = state
                    .tables
                    .create_type_reference(declared, &type_parameters);
                assert_eq!(reference, declared);
                assert!(state.could_contain_type_variables(declared));
            },
        );
    }

    #[test]
    fn thisful_interface_declares_a_generic_type_without_parameters() {
        with_program_state(
            &[("a.ts", "interface I { m(): this }\n")],
            &CompilerOptions::default(),
            |state| {
                let symbol = state
                    .resolve_file_scope_name("I", SymbolFlags::INTERFACE)
                    .expect("I resolves");
                let declared = state
                    .get_declared_type_of_class_or_interface(symbol)
                    .expect("declared type in slice");
                assert!(matches!(
                    state.tables.type_of(declared).data,
                    TypeData::GenericType {
                        ref type_parameters,
                        ..
                    } if type_parameters.is_empty()
                ));
                assert!(
                    !state.could_contain_type_variables(declared),
                    "no type arguments to contain variables"
                );
            },
        );
    }

    #[test]
    fn thisless_heritage_interface_stays_plain_but_members_escape() {
        with_program_state(
            &[(
                "a.ts",
                "interface A { a: string }\ninterface B extends A { b: string }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let symbol = state
                    .resolve_file_scope_name("B", SymbolFlags::INTERFACE)
                    .expect("B resolves");
                let declared = state
                    .get_declared_type_of_class_or_interface(symbol)
                    .expect("declared type in slice");
                assert!(
                    matches!(state.tables.type_of(declared).data, TypeData::Object),
                    "thisless heritage interfaces stay plain InterfaceTypes"
                );
                // 5.3a: heritage members merge through getBaseTypes —
                // B sees its own `b` plus the inherited `a`.
                let members = state
                    .resolve_structured_type_members(declared)
                    .expect("heritage members resolve");
                let names: Vec<String> = state
                    .members_of(members)
                    .properties
                    .iter()
                    .map(|&p| state.binder.symbol(p).escaped_name.clone())
                    .collect();
                assert_eq!(names, ["b", "a"], "own members first, inherited appended");
            },
        );
    }

    #[test]
    fn cyclic_heritage_reads_the_thisless_shell() {
        with_program_state(
            &[(
                "a.ts",
                "interface A extends B { }\ninterface B extends A { }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let a = state
                    .resolve_file_scope_name("A", SymbolFlags::INTERFACE)
                    .expect("A resolves");
                let b = state
                    .resolve_file_scope_name("B", SymbolFlags::INTERFACE)
                    .expect("B resolves");
                let declared_a = state
                    .get_declared_type_of_class_or_interface(a)
                    .expect("A declared");
                let declared_b = state
                    .get_declared_type_of_class_or_interface(b)
                    .expect("B declared");
                // tsc's eagerly written shells observe "no thisType yet"
                // mid-cycle: both stay thisless.
                assert!(matches!(
                    state.tables.type_of(declared_a).data,
                    TypeData::Object
                ));
                assert!(matches!(
                    state.tables.type_of(declared_b).data,
                    TypeData::Object
                ));
            },
        );
    }

    #[test]
    fn bare_reference_to_generic_interface_reports_2314() {
        with_program_state(
            &[("a.ts", "interface I<T> { a: T }\ndeclare var v: I;\n")],
            &CompilerOptions::default(),
            |state| {
                let annotation = find_probe_annotation(state.binder.source(0), "v")
                    .expect("var with annotation");
                let resolved = state
                    .get_type_from_type_node(annotation)
                    .expect("errorType flows");
                assert!(state.tables.is_error_type(resolved));
                let rendered: Vec<(u32, String)> = state
                    .diagnostics
                    .iter()
                    .map(|d| (d.code(), d.message_text().to_owned()))
                    .collect();
                assert_eq!(
                    rendered,
                    [(
                        2314,
                        "Generic type 'I<T>' requires 1 type argument(s).".to_owned()
                    )]
                );
            },
        );
    }

    #[test]
    fn class_declared_types_are_generic_type_targets() {
        with_program_state(
            &[("a.ts", "class C<T> { }\nclass D { }\n")],
            &CompilerOptions::default(),
            |state| {
                let c = state
                    .resolve_file_scope_name("C", SymbolFlags::CLASS)
                    .expect("C resolves");
                let d = state
                    .resolve_file_scope_name("D", SymbolFlags::CLASS)
                    .expect("D resolves");
                let declared_c = state
                    .get_declared_type_of_class_or_interface(c)
                    .expect("C declared");
                let declared_d = state
                    .get_declared_type_of_class_or_interface(d)
                    .expect("D declared");
                assert!(matches!(
                    state.tables.type_of(declared_c).data,
                    TypeData::GenericType { ref type_parameters, .. } if type_parameters.len() == 1
                ));
                assert!(state
                    .tables
                    .object_flags_of(declared_c)
                    .intersects(ObjectFlags::CLASS | ObjectFlags::REFERENCE));
                // kind === Class forces the GenericType shape even with
                // no parameters (57387).
                assert!(matches!(
                    state.tables.type_of(declared_d).data,
                    TypeData::GenericType { ref type_parameters, .. } if type_parameters.is_empty()
                ));
                assert!(!state.could_contain_type_variables(declared_d));
                assert!(state
                    .tables
                    .flags_of(declared_c)
                    .intersects(TypeFlags::OBJECT));
            },
        );
    }
}

#[cfg(test)]
mod generic_reference_tests {
    use tsrs2_types::{CompilerOptions, TypeData, TypeFlags};

    use crate::relpin::find_probe_annotation;
    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    fn annotation_of(state: &CheckerState, name: &str) -> tsrs2_syntax::NodeId {
        find_probe_annotation(state.binder.source(0), name).expect("var with annotation")
    }

    #[test]
    fn generic_reference_instantiates_and_interns() {
        with_program_state(
            &[(
                "a.ts",
                "interface I<T> { a: T }\ndeclare var v: I<string>;\ndeclare var w: I<string>;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let reference = state.get_type_from_type_node(v).expect("I<string>");
                assert!(matches!(
                    state.tables.type_of(reference).data,
                    TypeData::Reference { .. }
                ));
                assert_eq!(
                    state.tables.type_arguments(reference),
                    &[state.tables.intrinsics.string]
                );
                let w = annotation_of(state, "w");
                let again = state.get_type_from_type_node(w).expect("I<string>");
                assert_eq!(again, reference, "reference interning by target+list");
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn bare_generic_reference_reports_2314_with_local_parameter_display() {
        with_program_state(
            &[(
                "a.ts",
                "function f<T>() { interface I<U> { a: [T, U] } var v: I; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let resolved = state.get_type_from_type_node(v).expect("errorType flows");
                assert!(state.tables.is_error_type(resolved));
                let rendered: Vec<(u32, String)> = state
                    .diagnostics
                    .iter()
                    .map(|d| (d.code(), d.message_text().to_owned()))
                    .collect();
                assert_eq!(
                    rendered,
                    [(
                        2314,
                        "Generic type 'I<U>' requires 1 type argument(s).".to_owned()
                    )],
                    "oracle-pinned local-parameters-only display"
                );
            },
        );
    }

    #[test]
    fn arity_range_reports_2707() {
        with_program_state(
            &[(
                "a.ts",
                "interface K<T, U = string> { }\ndeclare var v: K;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let resolved = state.get_type_from_type_node(v).expect("errorType flows");
                assert!(state.tables.is_error_type(resolved));
                let rendered: Vec<(u32, String)> = state
                    .diagnostics
                    .iter()
                    .map(|d| (d.code(), d.message_text().to_owned()))
                    .collect();
                assert_eq!(
                    rendered,
                    [(
                        2707,
                        "Generic type 'K<T, U>' requires between 1 and 2 type arguments."
                            .to_owned()
                    )]
                );
            },
        );
    }

    #[test]
    fn type_parameter_defaults_fill_missing_arguments() {
        with_program_state(
            &[(
                "a.ts",
                "interface K<T, U = string> { }\ninterface L<T, U = T> { }\n\
                 declare var v: K<number>;\ndeclare var w: L<number>;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let reference = state.get_type_from_type_node(v).expect("K<number>");
                assert_eq!(
                    state.tables.type_arguments(reference),
                    &[
                        state.tables.intrinsics.number,
                        state.tables.intrinsics.string
                    ]
                );
                // U = T instantiates the default through the partially
                // filled argument list.
                let w = annotation_of(state, "w");
                let reference = state.get_type_from_type_node(w).expect("L<number>");
                assert_eq!(
                    state.tables.type_arguments(reference),
                    &[
                        state.tables.intrinsics.number,
                        state.tables.intrinsics.number
                    ]
                );
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn mutually_circular_defaults_resolve_silently_via_the_sentinel() {
        with_program_state(
            &[(
                "a.ts",
                "interface P<T = Q> { }\ninterface Q<U = P> { }\ndeclare var v: P;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let reference = state.get_type_from_type_node(v).expect("P resolves");
                // P<Q<P<unknown>>>: the re-entrant default stamps the
                // circular sentinel, which reads as "no default" and
                // falls back to unknownType (2716 is a 5.8 declaration
                // check, not a reference-site diagnostic).
                assert!(matches!(
                    state.tables.type_of(reference).data,
                    TypeData::Reference { .. }
                ));
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
                // The re-entrant stamp survives (tsc keeps the circular
                // sentinel over the successfully computed default), so
                // T's default reads as none -> unknownType.
                let args = state.tables.type_arguments(reference).to_vec();
                assert_eq!(args, [state.tables.intrinsics.unknown]);
            },
        );
    }

    #[test]
    fn stray_type_arguments_report_2315() {
        with_program_state(
            &[(
                "a.ts",
                "type A = string;\ndeclare var v: A<number>;\nfunction f<T>() { var w: T<string>; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let resolved = state.get_type_from_type_node(v).expect("errorType flows");
                assert!(state.tables.is_error_type(resolved));
                let w = annotation_of(state, "w");
                let resolved = state.get_type_from_type_node(w).expect("errorType flows");
                assert!(state.tables.is_error_type(resolved));
                let rendered: Vec<(u32, String)> = state
                    .diagnostics
                    .iter()
                    .map(|d| (d.code(), d.message_text().to_owned()))
                    .collect();
                assert_eq!(
                    rendered,
                    [
                        (2315, "Type 'A' is not generic.".to_owned()),
                        (2315, "Type 'T' is not generic.".to_owned()),
                    ]
                );
            },
        );
    }

    #[test]
    fn alias_hosted_generic_references_defer_and_resolve_lazily() {
        with_program_state(
            &[(
                "a.ts",
                "interface I<T> { a: T }\ntype X = I<number>;\ndeclare var v: X;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let deferred = state.get_type_from_type_node(v).expect("alias RHS defers");
                // The deferred shell: Reference object flags, a node,
                // the alias stamp, and NO resolved arguments yet.
                assert!(matches!(
                    state.tables.type_of(deferred).data,
                    TypeData::Reference {
                        resolved_type_arguments: None,
                        ..
                    }
                ));
                assert!(state.links.ty(deferred).deferred_node.is_some());
                assert!(state.tables.type_of(deferred).alias_symbol.is_some());
                // Forcing reads the node lazily.
                let arguments = state.get_type_arguments(deferred).expect("forcible");
                assert_eq!(arguments, [state.tables.intrinsics.number]);
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn self_referential_deferred_aliases_resolve_without_circularity() {
        with_program_state(
            &[("a.ts", "type A = [A];\ndeclare var v: A;\n")],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let deferred = state.get_type_from_type_node(v).expect("tuple RHS defers");
                // `type A = [A]` is LEGAL through deferral (the eager
                // path would 2456): the argument list is the deferred
                // reference itself.
                let arguments = state.get_type_arguments(deferred).expect("forcible");
                assert_eq!(arguments, [deferred]);
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn alias_hosted_array_nodes_defer_over_the_global_array_target() {
        with_program_state(
            &[(
                "a.ts",
                "interface Array<T> { length: number }\ntype A = string[];\ndeclare var v: A;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let deferred = state.get_type_from_type_node(v).expect("array RHS defers");
                assert!(state.links.ty(deferred).deferred_node.is_some());
                let target = state.tables.reference_target(deferred);
                assert!(matches!(
                    state.tables.type_of(target).data,
                    TypeData::GenericType { .. }
                ));
                let arguments = state.get_type_arguments(deferred).expect("forcible");
                assert_eq!(arguments, [state.tables.intrinsics.string]);
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn plain_array_annotations_resolve_eagerly_against_the_array_global() {
        with_program_state(
            &[(
                "a.ts",
                "interface Array<T> { length: number }\ndeclare var v: number[];\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let reference = state.get_type_from_type_node(v).expect("arrays construct");
                // No alias host, no alias-resolvable elements: the
                // eager arm builds a plain resolved reference.
                assert!(state.links.ty(reference).deferred_node.is_none());
                assert_eq!(
                    state.tables.type_arguments(reference),
                    [state.tables.intrinsics.number]
                );
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn missing_array_global_reports_2318_and_empty_object_type() {
        with_program_state(
            &[("a.ts", "declare var v: number[];\n")],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let resolved = state.get_type_from_type_node(v).expect("fallback resolves");
                // getArrayOrTupleTargetType finds emptyGenericType (the
                // memoized getGlobalType failure) -> emptyObjectType
                // (61122-61123), with the one-shot 2318.
                assert_eq!(resolved, state.empty_object_type);
                assert_eq!(state.diagnostics.len(), 1, "{:?}", state.diagnostics);
                assert_eq!(state.diagnostics[0].code(), 2318);
            },
        );
    }

    #[test]
    fn empty_tuple_aliases_resolve_to_the_tuple_target() {
        with_program_state(
            &[("a.ts", "type E = [];\ndeclare var v: E;\n")],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let resolved = state.get_type_from_type_node(v).expect("empty tuple");
                // 61124: zero-element deferrable tuples return the
                // TARGET itself, not a deferred reference.
                assert!(matches!(
                    state.tables.type_of(resolved).data,
                    TypeData::TupleTarget(_)
                ));
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn heritage_with_type_arguments_instantiates_inherited_members() {
        with_program_state(
            &[(
                "a.ts",
                "interface A<T> { a: T }\ninterface B extends A<string> { b: number }\n\
                 declare var v: B;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let b = state.get_type_from_type_node(v).expect("B resolves");
                let a_property = state
                    .get_property_of_type_full(b, "a")
                    .expect("members resolve")
                    .expect("inherited property");
                let a_type = state
                    .get_type_of_symbol(a_property)
                    .expect("inherited property type");
                assert_eq!(a_type, state.tables.intrinsics.string);
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn generic_heritage_chains_map_members_through_the_reference() {
        with_program_state(
            &[(
                "a.ts",
                "interface A<T> { a: T }\ninterface B<U> extends A<U> { b: U }\n\
                 declare var v: B<number>;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let b = state.get_type_from_type_node(v).expect("B<number>");
                for name in ["a", "b"] {
                    let property = state
                        .get_property_of_type_full(b, name)
                        .expect("members resolve")
                        .expect("property present");
                    let property_type = state.get_type_of_symbol(property).expect("property type");
                    assert_eq!(
                        property_type, state.tables.intrinsics.number,
                        "{name} instantiates through the heritage mapper"
                    );
                }
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn deferred_reference_members_force_arguments_lazily() {
        with_program_state(
            &[(
                "a.ts",
                "interface Box<T> { value: T }\ntype A = Box<number>;\ndeclare var v: A;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let deferred = state.get_type_from_type_node(v).expect("alias RHS defers");
                assert!(state.links.ty(deferred).deferred_node.is_some());
                let value = state
                    .get_property_of_type_full(deferred, "value")
                    .expect("deferred members resolve")
                    .expect("value property");
                let value_type = state.get_type_of_symbol(value).expect("property type");
                assert_eq!(value_type, state.tables.intrinsics.number);
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn circular_heritage_reports_one_2310_per_interface() {
        with_program_state(
            &[(
                "a.ts",
                "interface A extends B { }\ninterface B extends A { }\ndeclare var v: A;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let a = state.get_type_from_type_node(v).expect("A resolves");
                let members = state
                    .resolve_structured_type_members(a)
                    .expect("cycle-cut members resolve");
                assert!(state.members_of(members).properties.is_empty());
                // Oracle-pinned (with-lib CLI): exactly one 2310 per
                // interface — the duplicate report on A collapses in
                // tsc's diagnostics.add equality dedupe.
                let codes: Vec<u32> = state
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.code())
                    .collect();
                assert_eq!(codes, [2310, 2310], "{:?}", state.diagnostics);
                assert_ne!(
                    state.diagnostics[0].start, state.diagnostics[1].start,
                    "one per declaration"
                );
            },
        );
    }

    #[test]
    fn thisful_interface_members_substitute_the_reference_for_this() {
        with_program_state(
            &[(
                "a.ts",
                "interface C { self: this; tag: string }\ndeclare var v: C;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let c = state.get_type_from_type_node(v).expect("C resolves");
                // this-ful interfaces are GenericType targets; the
                // annotation resolves to the declared type itself.
                let self_property = state
                    .get_property_of_type_full(c, "self")
                    .expect("members resolve")
                    .expect("self property");
                let self_type = state.get_type_of_symbol(self_property).expect("self type");
                assert_eq!(
                    self_type, c,
                    "this maps to the reference through the this-argument mapper"
                );
                // Thisless members skip instantiation entirely
                // (mappingThisOnly): `tag` keeps the ORIGINAL symbol.
                let tag_property = state
                    .get_property_of_type_full(c, "tag")
                    .expect("members resolve")
                    .expect("tag property");
                assert!(
                    !state
                        .links
                        .symbol(tag_property)
                        .check_flags
                        .intersects(tsrs2_types::CheckFlags::INSTANTIATED),
                    "thisless member symbols pass through uninstantiated"
                );
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn primitive_apparent_types_read_the_wrapper_globals() {
        with_program_state(
            &[(
                "a.ts",
                "interface String { length: number }\ndeclare var v: \"abc\";\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let literal = state.get_type_from_type_node(v).expect("literal type");
                let length = state
                    .get_property_of_type_full(literal, "length")
                    .expect("apparent members resolve")
                    .expect("length property via globalStringType");
                let length_type = state.get_type_of_symbol(length).expect("length type");
                assert_eq!(length_type, state.tables.intrinsics.number);
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn intersection_apparent_substitutes_this_across_constituents() {
        with_program_state(
            &[(
                "a.ts",
                "interface C { self: this }\ntype X = C & { x: number };\ndeclare var v: X;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let x = state.get_type_from_type_node(v).expect("X resolves");
                // getApparentTypeOfIntersectionType: this maps to the
                // WHOLE intersection before the property lookup.
                let self_property = state
                    .get_property_of_type_full(x, "self")
                    .expect("intersection apparent resolves")
                    .expect("self property");
                let self_type = state.get_type_of_symbol(self_property).expect("self type");
                assert_eq!(self_type, x, "this-argument = the intersection");
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn empty_subinterfaces_normalize_to_their_single_base() {
        with_program_state(
            &[(
                "a.ts",
                "interface A { self: this; a: number }\ninterface J extends A { }\n\
                 declare var v: J;\ndeclare var w: A;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                // A is this-ful, so the empty J is this-ful too — both
                // are GenericType references, the shape getSingleBase
                // requires.
                let v = annotation_of(state, "v");
                let j = state.get_type_from_type_node(v).expect("J resolves");
                let w = annotation_of(state, "w");
                let a = state.get_type_from_type_node(w).expect("A resolves");
                let normalized = state
                    .get_normalized_type(j, /*writing*/ false)
                    .expect("single-base collapse");
                assert_eq!(normalized, a, "empty J collapses to its single base A");
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn generic_subinterfaces_do_not_collapse_their_single_base() {
        with_program_state(
            &[(
                "a.ts",
                "interface I<T> { a: T }\ninterface J<T> extends I<T> { }\n\
                 declare var v: J<number>;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                // The type parameter T lives in J's symbol MEMBERS
                // (binder parity with tsc), so the non-augmenting
                // collapse's `getMembersOfSymbol(symbol).size` gate
                // rejects generic subinterfaces.
                let v = annotation_of(state, "v");
                let j = state.get_type_from_type_node(v).expect("J<number>");
                let single = state
                    .get_single_base_for_non_augmenting_subtype(j)
                    .expect("computes");
                assert_eq!(single, None);
                let normalized = state
                    .get_normalized_type(j, /*writing*/ false)
                    .expect("normalizes");
                assert_eq!(normalized, j);
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn circular_tuple_type_arguments_report_4110() {
        with_program_state(
            &[(
                "a.ts",
                "interface Array<T> { length: number }\ntype A = [A[0]];\ndeclare var v: A;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let deferred = state.get_type_from_type_node(v).expect("tuple RHS defers");
                // Forcing the arguments resolves A[0], whose property
                // lookup re-enters getTypeArguments on the same
                // reference — the pop-failure arm fills errorType and
                // reports 4110 at the tuple node (oracle-pinned).
                let arguments = state.get_type_arguments(deferred).expect("forcible");
                assert_eq!(arguments, [state.tables.intrinsics.error]);
                let codes: Vec<u32> = state
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.code())
                    .collect();
                assert_eq!(codes, [4110], "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn circular_interface_type_arguments_report_4109() {
        with_program_state(
            &[(
                "a.ts",
                "interface I<T> { a: T }\ntype B = I<B[\"a\"]>;\ndeclare var w: B;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let w = annotation_of(state, "w");
                let deferred = state.get_type_from_type_node(w).expect("alias RHS defers");
                let arguments = state.get_type_arguments(deferred).expect("forcible");
                assert_eq!(arguments, [state.tables.intrinsics.error]);
                let codes: Vec<u32> = state
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.code())
                    .collect();
                assert_eq!(codes, [4109], "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn labeled_tuples_synthesize_named_index_members() {
        with_program_state(
            &[(
                "a.ts",
                "interface Array<T> { length: number }\n\
                 type P = [x: number, y?: string];\ndeclare var v: P[0];\n",
            )],
            &CompilerOptions::default(),
            |state| {
                // Labeled tuple targets intern with the node-id key
                // segment and carry tupleLabelDeclaration on the
                // synthesized properties.
                let v = annotation_of(state, "v");
                let resolved = state.get_type_from_type_node(v).expect("P[0]");
                assert_eq!(resolved, state.tables.intrinsics.number);
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn rest_parameter_arity_reads_tuple_rest_types() {
        with_program_state(
            &[(
                "a.ts",
                "interface Array<T> { length: number }\n\
                 declare var f: (...args: [number, string?]) => void;\n\
                 declare var g: (a: number, b?: string) => void;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                // Tuple rest parameters expand for arity: f accepts
                // (number, string?) exactly like g — assignable both
                // ways through the signature arity machinery.
                let f_node = annotation_of(state, "f");
                let f = state.get_type_from_type_node(f_node).expect("f resolves");
                let g_node = annotation_of(state, "g");
                let g = state.get_type_from_type_node(g_node).expect("g resolves");
                assert_eq!(state.is_type_assignable_to(f, g), Ok(true));
                assert_eq!(state.is_type_assignable_to(g, f), Ok(true));
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn union_members_synthesize_combined_call_signatures() {
        with_program_state(
            &[(
                "a.ts",
                // The Array interface stands in for the lib global:
                // the 5.3b array-target relation arm probes
                // global(Readonly)ArrayType on object-object pairs,
                // and the no-lib one-shot 2318 would dirty the
                // asserted-empty diagnostics.
                "interface Array<T> { length: number }\n\
                 interface ReadonlyArray<T> { length: number }\n\
                 type F = (() => number) | (() => string);\ndeclare var v: F;\n\
                 declare var w: () => number | string;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let f = state.get_type_from_type_node(v).expect("F resolves");
                let signatures = state
                    .get_signatures_of_type(f, crate::structural::SignatureKind::Call)
                    .expect("union call signatures synthesize");
                assert_eq!(signatures.len(), 1, "matching arities combine to one");
                // The composite return is the Subtype-reduced union.
                let w = annotation_of(state, "w");
                let expected = state.get_type_from_type_node(w).expect("w resolves");
                assert_eq!(state.is_type_assignable_to(f, expected), Ok(true));
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn union_index_infos_intersect_across_constituents() {
        with_program_state(
            &[(
                "a.ts",
                "interface Array<T> { length: number }\n\
                 interface ReadonlyArray<T> { length: number }\n\
                 type U = { [k: string]: number } | { [k: string]: string };\n\
                 declare var v: U;\ndeclare var w: { [k: string]: number | string };\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let u = state.get_type_from_type_node(v).expect("U resolves");
                let infos = state
                    .get_index_infos_of_type(u)
                    .expect("union index infos synthesize");
                assert_eq!(infos.len(), 1);
                let w = annotation_of(state, "w");
                let expected = state.get_type_from_type_node(w).expect("w resolves");
                assert_eq!(state.is_type_assignable_to(u, expected), Ok(true));
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn class_instance_members_resolve_with_heritage() {
        with_program_state(
            &[(
                "a.ts",
                "declare class B { b: string }\ndeclare class C extends B { c: number }\n\
                 declare var v: C;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let c = state.get_type_from_type_node(v).expect("C resolves");
                for (name, expected) in [
                    ("c", state.tables.intrinsics.number),
                    ("b", state.tables.intrinsics.string),
                ] {
                    let property = state
                        .get_property_of_type_full(c, name)
                        .expect("class members resolve")
                        .expect("property present");
                    let property_type = state.get_type_of_symbol(property).expect("property type");
                    assert_eq!(property_type, expected, "{name}");
                }
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn generic_class_references_instantiate_members() {
        with_program_state(
            &[(
                "a.ts",
                "declare class Box<T> { value: T }\ndeclare var v: Box<string>;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let boxed = state.get_type_from_type_node(v).expect("Box<string>");
                let value = state
                    .get_property_of_type_full(boxed, "value")
                    .expect("members resolve")
                    .expect("value property");
                let value_type = state.get_type_of_symbol(value).expect("value type");
                assert_eq!(value_type, state.tables.intrinsics.string);
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn accessor_properties_read_getter_and_setter_annotations() {
        with_program_state(
            &[(
                "a.ts",
                "declare class A { get x(): number; set x(value: number); }\n\
                 declare var v: A;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let a = state.get_type_from_type_node(v).expect("A resolves");
                let x = state
                    .get_property_of_type_full(a, "x")
                    .expect("members resolve")
                    .expect("x property");
                let x_type = state.get_type_of_symbol(x).expect("accessor type");
                assert_eq!(x_type, state.tables.intrinsics.number);
                let write_type = state
                    .get_write_type_of_accessors(x)
                    .expect("setter write type");
                assert_eq!(write_type, state.tables.intrinsics.number);
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn own_base_expression_circularity_reports_2506() {
        with_program_state(
            &[("a.ts", "declare class C extends C { }\ndeclare var v: C;\n")],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let c = state.get_type_from_type_node(v).expect("C resolves");
                let members = state
                    .resolve_structured_type_members(c)
                    .expect("cycle-cut members resolve");
                assert!(state.members_of(members).properties.is_empty());
                let codes: Vec<u32> = state
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.code())
                    .collect();
                assert_eq!(codes, [2506], "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn deferred_references_instantiate_through_the_canonical_node_cache() {
        with_program_state(
            &[(
                "a.ts",
                "interface I<T> { a: T }\ntype A<U> = I<U>;\n\
                 declare var v: A<string>;\ndeclare var w: A<string>;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let instance = state
                    .get_type_from_type_node(v)
                    .expect("alias instantiation over a deferred RHS");
                // getObjectTypeInstantiation minted a fresh deferred
                // reference carrying the U->string mapper.
                assert!(state.links.ty(instance).deferred_node.is_some());
                assert!(state.links.ty(instance).deferred_mapper.is_some());
                let arguments = state.get_type_arguments(instance).expect("forcible");
                assert_eq!(arguments, [state.tables.intrinsics.string]);
                // The canonical node reference hosts the instantiations
                // map: the same argument list reuses the instance.
                let w = annotation_of(state, "w");
                let again = state.get_type_from_type_node(w).expect("cached");
                assert_eq!(again, instance);
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn variadic_expansion_pre_forces_deferred_tuple_elements() {
        with_program_state(
            &[(
                "a.ts",
                "type B = [number];\ntype A = [...B, string];\ndeclare var v: A;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                // A has a variadic element, so it resolves EAGERLY; the
                // spread forces B's (deferred) arguments through the
                // pre-force wrapper.
                let resolved = state.get_type_from_type_node(v).expect("variadic expands");
                assert!(state.links.ty(resolved).deferred_node.is_none());
                assert_eq!(
                    state.tables.type_arguments(resolved),
                    [
                        state.tables.intrinsics.number,
                        state.tables.intrinsics.string
                    ]
                );
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn generic_reference_relations_flow_through_instantiated_arguments() {
        with_program_state(
            &[(
                "a.ts",
                "interface I<T> { a: T }\ndeclare var v: I<\"x\">;\ndeclare var w: I<string>;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let narrow = state.get_type_from_type_node(v).expect("I<\"x\">");
                let w = annotation_of(state, "w");
                let wide = state.get_type_from_type_node(w).expect("I<string>");
                assert_ne!(narrow, wide);
                // Reference MEMBERS resolve since 5.3a: the relation
                // flows through the instantiated `a` property.
                assert_eq!(state.is_type_assignable_to(narrow, wide), Ok(true));
                assert_eq!(state.is_type_assignable_to(wide, narrow), Ok(false));
                assert!(state.tables.flags_of(narrow).intersects(TypeFlags::OBJECT));
            },
        );
    }
}

#[cfg(test)]
mod alias_instantiation_tests {
    use tsrs2_types::{CompilerOptions, TypeData, TypeFlags};

    use crate::relpin::find_probe_annotation;
    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    fn annotation_of(state: &CheckerState, name: &str) -> tsrs2_syntax::NodeId {
        find_probe_annotation(state.binder.source(0), name).expect("var with annotation")
    }

    #[test]
    fn generic_alias_instantiates_with_alias_stamping_and_interning() {
        with_program_state(
            &[(
                "a.ts",
                "type A<T> = T | null;\ndeclare var v: A<string>;\ndeclare var w: A<string>;\ndeclare var u: string | null;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let a = state
                    .resolve_file_scope_name("A", tsrs2_types::SymbolFlags::TYPE_ALIAS)
                    .expect("A resolves");
                let v = annotation_of(state, "v");
                let instantiated = state.get_type_from_type_node(v).expect("A<string>");
                assert!(state
                    .tables
                    .flags_of(instantiated)
                    .intersects(TypeFlags::UNION));
                assert_eq!(state.tables.type_of(instantiated).alias_symbol, Some(a));
                assert_eq!(
                    state.tables.type_of(instantiated).alias_type_arguments.as_deref(),
                    Some(&[state.tables.intrinsics.string][..])
                );
                let w = annotation_of(state, "w");
                let again = state.get_type_from_type_node(w).expect("A<string>");
                assert_eq!(again, instantiated, "alias instantiations intern");
                // The alias id participates in the union intern key: the
                // bare structural twin is a DISTINCT type, like tsc.
                let u = annotation_of(state, "u");
                let bare = state.get_type_from_type_node(u).expect("string | null");
                assert_ne!(bare, instantiated);
                // ...but relations see them as the same shape.
                assert_eq!(state.is_type_assignable_to(bare, instantiated), Ok(true));
                assert_eq!(state.is_type_assignable_to(instantiated, bare), Ok(true));
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn alias_of_alias_restamps_the_outer_alias() {
        with_program_state(
            &[(
                "a.ts",
                "type A<T> = T | null;\ntype B = A<string>;\ndeclare var v: B;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let b = state
                    .resolve_file_scope_name("B", tsrs2_types::SymbolFlags::TYPE_ALIAS)
                    .expect("B resolves");
                let v = annotation_of(state, "v");
                let declared = state.get_type_from_type_node(v).expect("B resolves");
                assert!(state.tables.flags_of(declared).intersects(TypeFlags::UNION));
                assert_eq!(
                    state.tables.type_of(declared).alias_symbol,
                    Some(b),
                    "the outer alias reference stamps ITS symbol on the instantiation"
                );
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn declared_alias_union_carries_the_alias_with_parameter_arguments() {
        with_program_state(
            &[(
                "a.ts",
                "function f() { type L<T> = T | null; var v: L<string>; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                // No alias host on the annotation: the instantiation
                // inherits the DECLARED union's alias (L with its own
                // parameters) and instantiates the alias arguments.
                let v = annotation_of(state, "v");
                let instantiated = state.get_type_from_type_node(v).expect("L<string>");
                let alias = state
                    .tables
                    .type_of(instantiated)
                    .alias_symbol
                    .expect("inherited alias symbol");
                assert_eq!(state.binder.symbol(alias).escaped_name, "L");
                assert_eq!(
                    state
                        .tables
                        .type_of(instantiated)
                        .alias_type_arguments
                        .as_deref(),
                    Some(&[state.tables.intrinsics.string][..])
                );
            },
        );
    }

    #[test]
    fn bare_generic_alias_reference_reports_2314_with_plain_display() {
        with_program_state(
            &[("a.ts", "type A<T> = T;\ndeclare var v: A;\n")],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let resolved = state.get_type_from_type_node(v).expect("errorType flows");
                assert!(state.tables.is_error_type(resolved));
                let rendered: Vec<(u32, String)> = state
                    .diagnostics
                    .iter()
                    .map(|d| (d.code(), d.message_text().to_owned()))
                    .collect();
                assert_eq!(
                    rendered,
                    [(
                        2314,
                        "Generic type 'A' requires 1 type argument(s).".to_owned()
                    )],
                    "alias arity errors use the plain symbol display"
                );
            },
        );
    }

    #[test]
    fn intrinsic_string_mapping_aliases_route_to_get_string_mapping_type() {
        with_program_state(
            &[(
                "a.ts",
                "type Uppercase<S extends string> = intrinsic;\ndeclare var v: Uppercase<\"abc\">;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let mapped = state.get_type_from_type_node(v).expect("Uppercase<\"abc\">");
                assert_eq!(mapped, state.tables.get_string_literal_type("ABC"));
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn self_referential_generic_alias_reports_2456() {
        with_program_state(
            &[("a.ts", "type A<T> = A<T>;\ndeclare var v: A<string>;\n")],
            &CompilerOptions::default(),
            |state| {
                let v = annotation_of(state, "v");
                let resolved = state.get_type_from_type_node(v).expect("errorType flows");
                assert!(state.tables.is_error_type(resolved));
                // Oracle-pinned: tsc emits 2456 at the declaration
                // plus 2315 at BOTH references (the mid-cycle declared
                // type is errorType with no typeParameters, so each
                // argument list trips checkNoTypeArguments).
                let mut codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
                codes.sort_unstable();
                assert_eq!(codes, [2315, 2315, 2456]);
            },
        );
    }

    #[test]
    fn generic_alias_of_type_literal_stamps_the_anonymous_type() {
        with_program_state(
            &[(
                "a.ts",
                "type Box<T> = { value: T };\ndeclare var v: Box<string>;\ndeclare var w: Box<string>;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let box_symbol = state
                    .resolve_file_scope_name("Box", tsrs2_types::SymbolFlags::TYPE_ALIAS)
                    .expect("Box resolves");
                let v = annotation_of(state, "v");
                let instantiated = state.get_type_from_type_node(v).expect("Box<string>");
                // The RHS type literal becomes an instantiated anonymous
                // shell carrying the alias.
                assert!(matches!(
                    state.tables.type_of(instantiated).data,
                    TypeData::Object
                ));
                assert_eq!(
                    state.tables.type_of(instantiated).alias_symbol,
                    Some(box_symbol)
                );
                let w = annotation_of(state, "w");
                let again = state.get_type_from_type_node(w).expect("Box<string>");
                assert_eq!(again, instantiated, "instantiation interning");
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }
}

#[cfg(test)]
mod generic_signature_tests {
    use tsrs2_types::CompilerOptions;

    use crate::relpin::find_probe_annotation;
    use crate::state::test_support::with_program_state;

    #[test]
    fn function_type_annotations_construct_generic_signatures() {
        with_program_state(
            &[("a.ts", "declare var v: <T extends string>(x: T) => T;\n")],
            &CompilerOptions::default(),
            |state| {
                let annotation = find_probe_annotation(state.binder.source(0), "v")
                    .expect("var with annotation");
                let signature = state
                    .get_signature_from_declaration(annotation)
                    .expect("generic signature");
                let type_parameters = state
                    .signature_of(signature)
                    .type_parameters
                    .clone()
                    .expect("typeParameters");
                assert_eq!(type_parameters.len(), 1);
                let constraint = state
                    .get_constraint_from_type_parameter(type_parameters[0])
                    .expect("constraint");
                assert_eq!(constraint, Some(state.tables.intrinsics.string));
                // Erasure maps the parameter and return to any.
                let erased = state.get_erased_signature(signature).expect("erased");
                let erased_return = state
                    .get_return_type_of_signature(erased)
                    .expect("erased return");
                assert_eq!(erased_return, state.tables.intrinsics.any);
            },
        );
    }

    #[test]
    fn generic_signature_relations_escape_to_inference() {
        with_program_state(
            &[(
                "a.ts",
                "declare var v: <T>(x: T) => T;\ndeclare var w: <U>(x: U) => U;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let v = find_probe_annotation(state.binder.source(0), "v").expect("v");
                let w = find_probe_annotation(state.binder.source(0), "w").expect("w");
                let source = state.get_type_from_type_node(v).expect("v type");
                let target = state.get_type_from_type_node(w).expect("w type");
                let related = state.is_type_assignable_to(source, target);
                let reason = related.expect_err("generic relations are M6").reason;
                assert!(
                    reason.contains("instantiateSignatureInContextOf"),
                    "{reason}"
                );
            },
        );
    }
}

// ---- enum declared types + values (M4 5.3b) ----
#[cfg(test)]
mod enum_tests {
    use tsrs2_syntax::NodeId;
    use tsrs2_types::{CompilerOptions, TypeData, TypeFlags, TypeId};

    use crate::relpin::find_probe_annotation;
    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    fn with_state<R>(text: &str, run: impl FnOnce(&mut CheckerState) -> R) -> R {
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), run)
    }

    fn annotation_type(state: &mut CheckerState, name: &str) -> TypeId {
        let annotation: NodeId = find_probe_annotation(state.binder.source(0), name)
            .expect("declared var with annotation");
        state
            .get_type_from_type_node(annotation)
            .expect("annotation resolves")
    }

    fn literal_number(state: &CheckerState, ty: TypeId) -> f64 {
        match &state.tables.type_of(ty).data {
            TypeData::Literal {
                value: tsrs2_types::LiteralValue::Number(value),
            } => *value,
            other => panic!("expected number literal, got {other:?}"),
        }
    }

    #[test]
    fn enum_declared_type_is_a_stamped_literal_union() {
        with_state(
            "enum E { A, B }\ndeclare var e: E;\ndeclare var a: E.A;\n",
            |state| {
                let e = annotation_type(state, "e");
                let flags = state.tables.flags_of(e);
                // 57466-57469: the member union takes EnumLiteral and
                // the enum symbol.
                assert!(flags.intersects(TypeFlags::UNION));
                assert!(flags.intersects(TypeFlags::ENUM_LITERAL));
                assert!(state.tables.type_of(e).symbol.is_some());
                let TypeData::Union { types, .. } = &state.tables.type_of(e).data else {
                    panic!("two-member enums declare unions");
                };
                let members: Vec<TypeId> = types.to_vec();
                assert_eq!(members.len(), 2);
                assert_eq!(literal_number(state, members[0]), 0.0);
                assert_eq!(literal_number(state, members[1]), 1.0);
                // E.A resolves to the member's REGULAR literal type.
                let a = annotation_type(state, "a");
                assert_eq!(a, members[0]);
                assert!(state.tables.flags_of(a).intersects(TypeFlags::ENUM_LITERAL));
            },
        );
    }

    #[test]
    fn enum_values_evaluate_auto_and_constant_expressions() {
        with_state(
            "enum E { A = 3, B, C = (A | B) * 2, D = \"x\" + \"y\", E2 = `a${\"b\"}c` }\n\
             declare var c: E.C;\ndeclare var d: E.D;\n",
            |state| {
                let c = annotation_type(state, "c");
                // A|B = 3|4 = 7, *2 = 14.
                assert_eq!(literal_number(state, c), 14.0);
                let d = annotation_type(state, "d");
                match &state.tables.type_of(d).data {
                    TypeData::Literal {
                        value: tsrs2_types::LiteralValue::String(text),
                    } => assert_eq!(text, "xy"),
                    other => panic!("expected string literal, got {other:?}"),
                }
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn single_member_enum_declares_the_literal_itself() {
        with_state("enum One { A }\ndeclare var v: One;\n", |state| {
            let one = annotation_type(state, "v");
            let flags = state.tables.flags_of(one);
            // getUnionType over one literal returns the literal — no
            // union to stamp, so the symbol stays the MEMBER's.
            assert!(!flags.intersects(TypeFlags::UNION));
            assert!(flags.intersects(TypeFlags::NUMBER_LITERAL));
            assert!(flags.intersects(TypeFlags::ENUM_LITERAL));
            assert_eq!(literal_number(state, one), 0.0);
        });
    }

    #[test]
    fn ambient_uninitialized_members_get_computed_enum_types() {
        with_state("declare enum A { X }\ndeclare var v: A;\n", |state| {
            let a = annotation_type(state, "v");
            let flags = state.tables.flags_of(a);
            assert!(flags.intersects(TypeFlags::ENUM), "{flags:?}");
            assert!(!flags.intersects(TypeFlags::UNION));
            assert!(matches!(state.tables.type_of(a).data, TypeData::Enum));
        });
    }

    #[test]
    fn enum_forward_reference_reports_2651_and_yields_zero() {
        with_state("enum E { A = B, B = 1 }\ndeclare var a: E.A;\n", |state| {
            let a = annotation_type(state, "a");
            assert_eq!(literal_number(state, a), 0.0);
            let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            assert_eq!(codes, vec![2651]);
        });
    }

    #[test]
    fn enum_self_reference_reports_2565_then_checks_the_initializer_expression() {
        with_state("enum E { A = A }\ndeclare var a: E.A;\n", |state| {
            let annotation = find_probe_annotation(state.binder.source(0), "a")
                .expect("declared var with annotation");
            // The self-reference evaluates to no value, so tsc falls
            // into checkExpression + checkTypeAssignableTo (85654) —
            // live since 5.5e. The member type is number-based, so the
            // assignable check passes and the oracle total is the one
            // 2565 (oracle-pinned 2026-07-13).
            state
                .get_type_from_type_node(annotation)
                .expect("computed enum member checks its initializer since 5.5e");
            let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            assert_eq!(codes, vec![2565]);
            // Recompute is idempotent (the 2565 dedupes).
            state
                .get_type_from_type_node(annotation)
                .expect("recompute stays clean");
            let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            assert_eq!(codes, vec![2565]);
        });
    }

    #[test]
    fn enum_member_referencing_earlier_const_evaluates() {
        with_state(
            "const x = 3;\nenum E { A = x, B = First.A + 1 }\nenum First { A = 1 }\n\
             declare var a: E.A;\ndeclare var b: E.B;\n",
            |state| {
                let a = annotation_type(state, "a");
                assert_eq!(literal_number(state, a), 3.0);
                // Cross-enum references force the OTHER enum's values;
                // First is declared after E, which 2651 only forbids
                // for members, not whole enums declared later? No —
                // 2651 covers members declared after the referencing
                // initializer INCLUDING other enums' members, so B
                // reports and evaluates to 0.
                let b = annotation_type(state, "b");
                assert_eq!(literal_number(state, b), 1.0);
                let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
                assert_eq!(codes, vec![2651]);
            },
        );
    }

    #[test]
    fn enum_relations_route_through_the_enum_relation_cache() {
        with_state(
            "enum E { A, B }\nenum F { A, B }\nconst enum C { A }\n\
             declare var e: E;\ndeclare var f: F;\ndeclare var ea: E.A;\n\
             declare var n: number;\ndeclare var c: C;\n",
            |state| {
                let e = annotation_type(state, "e");
                let f = annotation_type(state, "f");
                let ea = annotation_type(state, "ea");
                let n = annotation_type(state, "n");
                let c = annotation_type(state, "c");
                // Different enums never relate (names differ).
                assert!(!state.is_type_assignable_to(e, f).expect("e->f"));
                assert!(!state.is_type_assignable_to(f, e).expect("f->e"));
                // Members relate to their own enum and to number.
                assert!(state.is_type_assignable_to(ea, e).expect("ea->e"));
                assert!(state.is_type_assignable_to(ea, n).expect("ea->n"));
                assert!(!state.is_type_assignable_to(e, ea).expect("e->ea"));
                // number → numeric enum under assignable (64754-64755).
                assert!(state.is_type_assignable_to(n, e).expect("n->e"));
                assert!(state.is_type_assignable_to(n, ea).expect("n->ea"));
                // const enums still take numbers (Enum flag rules, not
                // RegularEnum): single member C.A is a numeric enum
                // literal.
                assert!(state.is_type_assignable_to(n, c).expect("n->c"));
                assert!(!state.is_type_assignable_to(c, e).expect("c->e"));
            },
        );
    }
    #[test]
    fn tuple_this_append_keeps_the_target() {
        with_state("declare var t: [number, string?];\n", |state| {
            let tuple = annotation_type(state, "t");
            let target = state.tables.reference_target(tuple);
            let with_this = state
                .get_type_with_this_argument(tuple, None, false)
                .expect("tuple-this append is in-slice");
            // tsc 57789 = PLAIN createTypeReference: the SAME tuple
            // target with one extra (this) argument — arity, length
            // and element flags must not change.
            assert_eq!(state.tables.reference_target(with_this), target);
            let arguments = state
                .tables
                .try_type_arguments(with_this)
                .expect("plain references carry resolved arguments")
                .to_vec();
            let TypeData::TupleTarget(data) = &state.tables.type_of(target).data else {
                panic!("tuple annotations target a tuple target");
            };
            assert_eq!(arguments.len(), data.type_parameters.len() + 1);
            assert_eq!(data.element_flags.len(), data.type_parameters.len());
        });
    }
}

#[cfg(test)]
mod unique_symbol_tests {
    use tsrs2_types::{CompilerOptions, SymbolFlags, TypeData, TypeFlags};

    use crate::state::test_support::with_program_state;

    /// 5.7b review round: the unique-symbol type identity contract —
    /// one type per declaration (SymbolLinks.uniqueESSymbolType memo),
    /// UNIQUE_ES_SYMBOL flagged, distinct across declarations, and
    /// widening collapses to the plain `symbol` intrinsic.
    #[test]
    fn unique_symbol_types_are_per_declaration_memoized_and_widen() {
        with_program_state(
            &[(
                "a.ts",
                "declare const u: unique symbol;\ndeclare const v: unique symbol;\nlet l: unique symbol;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let sym = |state: &mut crate::state::CheckerState, name: &str| {
                    state
                        .get_global_symbol(name, SymbolFlags::VALUE, None)
                        .expect("fixture declares the name")
                };
                let u = sym(state, "u");
                let v = sym(state, "v");
                let u_type = state.get_type_of_symbol(u).expect("u types");
                let v_type = state.get_type_of_symbol(v).expect("v types");
                assert!(state
                    .tables
                    .flags_of(u_type)
                    .intersects(TypeFlags::UNIQUE_ES_SYMBOL));
                assert!(state
                    .tables
                    .flags_of(v_type)
                    .intersects(TypeFlags::UNIQUE_ES_SYMBOL));
                assert_ne!(
                    u_type, v_type,
                    "distinct declarations mint distinct unique types"
                );
                let name_of = |state: &crate::state::CheckerState, ty| {
                    match &state.tables.type_of(ty).data {
                        TypeData::UniqueESSymbol { escaped_name } => escaped_name.clone(),
                        other => panic!("expected a unique symbol, got {other:?}"),
                    }
                };
                let u_name = name_of(state, u_type);
                let v_name = name_of(state, v_type);
                assert!(u_name.starts_with("__@u@"), "{u_name}");
                assert!(v_name.starts_with("__@v@"), "{v_name}");
                assert_ne!(u_name, v_name);
                // The per-declaration memo: re-resolving the same
                // declaration answers the SAME TypeId.
                let u_decl = state.binder.symbol(u).declarations[0];
                let first = state
                    .get_es_symbol_like_type_for_node(u_decl)
                    .expect("resolves");
                let second = state
                    .get_es_symbol_like_type_for_node(u_decl)
                    .expect("resolves");
                assert_eq!(first, second, "SymbolLinks.uniqueESSymbolType memoizes");
                assert_eq!(first, u_type);
                // An INVALID position (a `let`) answers the plain
                // `symbol` intrinsic, not a unique type.
                let l = sym(state, "l");
                let l_decl = state.binder.symbol(l).declarations[0];
                let l_type = state
                    .get_es_symbol_like_type_for_node(l_decl)
                    .expect("resolves");
                assert_eq!(l_type, state.tables.intrinsics.es_symbol);
                // Widening collapses unique → symbol.
                let widened = state
                    .get_widened_unique_es_symbol_type(u_type)
                    .expect("widens");
                assert_eq!(widened, state.tables.intrinsics.es_symbol);
            },
        );
    }
}

#[cfg(test)]
mod late_binding_tests {
    use tsrs2_syntax::SyntaxKind;
    use tsrs2_types::CompilerOptions;

    use crate::state::test_support::with_program_state;

    /// 5.7b review round #2: an Err unwind mid-late-binding must leave
    /// every touched member re-bindable — asking the same question
    /// twice answers the same thing. Without the member-side rollback,
    /// the retry memo-hits past lateBindMember and SUCCEEDS with the
    /// colliding late member silently dropped from the table.
    #[test]
    fn late_binding_err_unwind_is_idempotent() {
        with_program_state(
            &[(
                "a.ts",
                "const k = \"x\";\ntype T = { x: number; [k]: string };\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let source = state.binder.source(0);
                let type_literal = source
                    .arena
                    .node_ids()
                    .find(|&id| {
                        tsrs2_binder::node_util::kind_of(source, id) == SyntaxKind::TypeLiteral
                    })
                    .expect("fixture contains a type literal");
                let symbol = state
                    .binder
                    .node_symbol(type_literal)
                    .expect("type literal binds a symbol");
                let first = state.get_members_of_symbol(symbol);
                let second = state.get_members_of_symbol(symbol);
                let first_reason = first.expect_err("early/late collision escapes").reason;
                let second_reason = second
                    .expect_err("the retry must escape the same way")
                    .reason;
                assert_eq!(first_reason, second_reason);
                assert!(
                    first_reason.contains("combineSymbolTables"),
                    "{first_reason}"
                );
            },
        );
    }
}
