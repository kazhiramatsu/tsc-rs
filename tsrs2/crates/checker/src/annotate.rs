//! The MINIMAL type-from-annotation path (m3-types-relations-steps.md
//! stage 4.1) — an explicitly scoped slice of M4 5.1/5.3, each fn a
//! ledgered (partial) port. Everything a TypeMapper would touch is
//! Unsupported by construction; M4 5.1 replaces this module's dispatch
//! with the full getTypeFromTypeNode port.

use tsrs2_binder::{InternalSymbolName, SymbolId};
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{NodeArrayId, NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    CheckFlags, ElementFlags, IntersectionFlags, M4Dependency, ObjectFlags, PseudoBigInt,
    SignatureFlags, SymbolFlags, TypeData, TypeFlags, TypeId, UnionReduction,
};

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

    fn identifier_text(&self, node: NodeId) -> Option<&str> {
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
                Err(Unsupported::new("this types (M4 5.3)"))
            }
            SyntaxKind::TypeQuery => self.get_type_from_type_query_node(node),
            SyntaxKind::IndexedAccessType => self.get_type_from_indexed_access_type_node(node),
            SyntaxKind::MappedType => Err(Unsupported::new("mapped types (M4 5.2)")),
            SyntaxKind::ConditionalType => Err(Unsupported::new("conditional types (M4 5.2)")),
            SyntaxKind::InferType => Err(Unsupported::new("infer types (M4 5.2)")),
            SyntaxKind::ImportType => Err(Unsupported::new("import types (M4)")),
            SyntaxKind::ExpressionWithTypeArguments => {
                Err(Unsupported::new("expression type references (M4)"))
            }
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
                    let element = data.element_type.ok_or_else(|| {
                        Unsupported::new("array type with missing element type")
                    })?;
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
            let element_flags = match &self.tables.type_of(target).data {
                TypeData::TupleTarget(data) => data.element_flags.to_vec(),
                _ => unreachable!("TUPLE object flag implies a tuple target"),
            };
            for (index, &argument) in type_arguments.iter().enumerate() {
                if element_flags[index].intersects(ElementFlags::VARIADIC)
                    && self.tables.is_tuple_type(argument)
                {
                    self.get_type_arguments(argument)?;
                }
            }
        }
        self.tables
            .create_normalized_type_reference(target, type_arguments)
            .map_err(Self::unsupported_m4)
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
        if elements
            .iter()
            .any(|&element| self.is_named_tuple_member(element))
        {
            return Err(Unsupported::new(
                "labeled tuple elements (deferred with tuple property synthesis)",
            ));
        }
        let element_flags: Vec<ElementFlags> = elements
            .iter()
            .map(|&element| self.get_tuple_element_flags(element))
            .collect();
        self.tables
            .get_tuple_target_type(&element_flags, readonly)
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
            SyntaxKind::UniqueKeyword => Err(Unsupported::new("unique symbol types (M4)")),
            other => Err(Unsupported::new(format!(
                "type operator {other:?} outside the M3 slice"
            ))),
        }
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
        let resolved = match symbol {
            None => self.empty_type_literal_type,
            Some(symbol)
                if self.symbol_members(symbol).is_empty() && alias_symbol.is_none() =>
            {
                self.empty_type_literal_type
            }
            Some(symbol) => {
                let id = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
                let alias_type_arguments = self.get_type_arguments_for_alias_symbol(alias_symbol);
                let ty = self.tables.type_mut(id);
                ty.object_flags = ObjectFlags::ANONYMOUS;
                ty.symbol = Some(symbol);
                ty.alias_symbol = alias_symbol;
                ty.alias_type_arguments =
                    alias_type_arguments.map(Vec::into_boxed_slice);
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
    /// (60587) is skipped — nothing reads it on reference nodes yet.
    fn get_type_from_type_reference(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let NodeData::TypeReference(data) = self.data_of(node) else {
            unreachable!("TypeReference kind implies payload");
        };
        let type_name = data
            .type_name
            .ok_or_else(|| Unsupported::new("type reference with missing name"))?;
        let Some(symbol) = self.resolve_entity_name(
            type_name,
            SymbolFlags::TYPE,
            /*ignore_errors*/ false,
            None,
        ) else {
            return Err(Unsupported::new(
                "unresolved type name (unknownSymbol -> errorType, observable at M4 5.4)",
            ));
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
            if flags.intersects(SymbolFlags::CLASS) {
                // Class references escape until class MEMBERS resolve
                // (5.3) — the declared types exist since 5.2b, but
                // every relation against them would dead-end.
                return Err(Unsupported::new("class declared types (M4 5.3)"));
            }
            self.get_type_from_class_or_interface_reference(node, symbol)?
        } else if flags.intersects(SymbolFlags::TYPE_ALIAS) {
            self.get_type_from_type_alias_reference(node, symbol)?
        } else if flags.intersects(SymbolFlags::REGULAR_ENUM | SymbolFlags::CONST_ENUM) {
            return Err(Unsupported::new("enum declared types (M4 5.3b)"));
        } else {
            return Err(Unsupported::new(format!(
                "type reference to symbol flags {flags:?} (M4)"
            )));
        };
        self.links.set_node_resolved_type(
            self.speculation_depth,
            node,
            LinkSlot::Resolved(resolved),
        );
        Ok(resolved)
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
            let local_alias_type_arguments =
                self.get_type_arguments_for_alias_symbol(alias_symbol);
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
                    let element = data.element_type.ok_or_else(|| {
                        Unsupported::new("array type with missing element type")
                    })?;
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
                self.tables.set_resolved_type_arguments_if_vacant(ty, resolved);
            }
        } else {
            let fallback = self.error_filled_type_arguments(ty);
            self.tables.set_resolved_type_arguments_if_vacant(ty, fallback);
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
                arguments.extend(
                    std::iter::repeat(error)
                        .take(type_parameters.len() - outer_type_parameter_count),
                );
                arguments
            }
            TypeData::TupleTarget(data) => vec![error; data.type_parameters.len()],
            _ => Vec::new(),
        }
    }

    /// tsc-port: getEffectiveTypeArguments @6.0.3
    /// tsc-hash: 6c12eff78b7503813dedde829e82b7ada2fbdded78d792dcc7da0591fe9498a2
    /// tsc-span: _tsc.js:81679-81681
    fn get_effective_type_arguments(
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
            let mut resolved_arguments: Vec<TypeId> =
                Vec::with_capacity(node_type_arguments.len());
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
                _ => Vec::new(),
            };
            let num_type_arguments = node_type_arguments.len();
            let min_type_argument_count =
                self.get_min_type_argument_count(Some(&type_parameters));
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
            let mut resolved_arguments: Vec<TypeId> =
                Vec::with_capacity(node_type_arguments.len());
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
        if let Some(&instantiation) = self.links.alias_instantiations.get(&(symbol, id_key.clone()))
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
            self.error_at(
                Some(node),
                &diagnostics::Type_0_is_not_generic,
                &[&display],
            );
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
            self.node_symbol(host)
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
                    self.symbol_flags(symbol).intersects(SymbolFlags::TYPE_ALIAS)
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
    /// getWidenedType (5.6) is identity here: annotation-derived
    /// symbol types never carry fresh literals or widening-context
    /// object literals (initializer typing, their source, is 5.5 —
    /// symbols typed from initializers unwind as Unsupported first).
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
        let Some(symbol) = self.resolve_entity_name(
            expr_name,
            SymbolFlags::VALUE,
            /*ignore_errors*/ false,
            None,
        ) else {
            return Err(Unsupported::new(
                "unresolved typeof entity name (unknownSymbol -> errorType, observable at 5.4)",
            ));
        };
        let ty = self.get_type_of_symbol(symbol)?;
        let resolved = self.tables.get_regular_type_of_literal_type(ty);
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
                self.links.alias_instantiations.insert((symbol, list_id), ty);
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
            generic_type.object_flags = ObjectFlags::from_bits(
                kind.bits() | ObjectFlags::REFERENCE.bits(),
            );
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
                .find(|&declaration| {
                    self.kind_of(declaration) == SyntaxKind::InterfaceDeclaration
                })
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
    fn is_entity_name_expression(&self, node: NodeId) -> bool {
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
    /// M3 dispatch: Anonymous + non-generic ClassOrInterface only.
    /// Reference members (resolveTypeReferenceMembers — instantiation),
    /// ReverseMapped/Mapped and union/intersection member resolution
    /// are M4/4.5 rows.
    pub fn resolve_structured_type_members(&mut self, ty: TypeId) -> CheckResult2<MembersId> {
        if let Some(members) = self.links.ty(ty).resolved_members.resolved() {
            return Ok(members);
        }
        let flags = self.tables.flags_of(ty);
        if !flags.intersects(TypeFlags::OBJECT) {
            return Err(Unsupported::new(
                "member resolution outside Object types (getApparentType, M4 5.3)",
            ));
        }
        let object_flags = self.tables.object_flags_of(ty);
        if object_flags.intersects(ObjectFlags::REFERENCE) {
            return Err(Unsupported::new(
                "type reference member instantiation (M4 5.3)",
            ));
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
    ///
    /// tsc-port: resolveDeclaredMembers @6.0.3
    /// tsc-hash: 26214e56476509650c70cc07871cd14e249f549efc1bffc1fc84e33349b0a7e0
    /// tsc-span: _tsc.js:57602-57615
    ///
    /// Fused for the thisless slice: with no type parameters and no
    /// base types, resolveObjectTypeMembers (57796) copies the declared
    /// members through unchanged, so the declared members ARE the
    /// resolved members.
    fn resolve_class_or_interface_members(&mut self, ty: TypeId) -> CheckResult2<MembersId> {
        let symbol = self
            .tables
            .type_of(ty)
            .symbol
            .expect("interface types carry their declaring symbol");
        // Interfaces with base types resolve through getBaseTypes +
        // heritage merge (resolveInterfaceMembers 58305, M4 5.3); the
        // M3 slice reads own members only — thisless interfaces WITH
        // heritage reach here since the declared-type worker stopped
        // escaping them (5.2 follow-up).
        let declarations = self.binder.symbol(symbol).declarations.clone();
        for declaration in declarations {
            let heritage = match self.data_of(declaration) {
                NodeData::InterfaceDeclaration(data) => data.heritage_clauses,
                NodeData::ClassDeclaration(data) => data.heritage_clauses,
                NodeData::ClassExpression(data) => data.heritage_clauses,
                _ => None,
            };
            if heritage.is_some_and(|list| !self.binder.node_array(list).nodes.is_empty()) {
                return Err(Unsupported::new("interface heritage/base types (M4 5.3)"));
            }
        }
        let members = self.symbol_members(symbol).clone();
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
            .set_type_members(self.speculation_depth, ty, LinkSlot::Resolved(id));
        Ok(id)
    }

    /// tsc-port: resolveAnonymousTypeMembers @6.0.3
    /// tsc-hash: 5da860e7aee705f29431b2726015d0564a56aeddbe32d2653253ad09aab4f93f
    /// tsc-span: _tsc.js:58316-58407
    ///
    /// M3 slices: the TypeLiteral-symbol branch (58332-58340) and the
    /// function/method value branch (58341/58355: empty members + call
    /// signatures from the symbol's declarations). Instantiated
    /// targets, classes, enums, modules and globalThis are M4.
    fn resolve_anonymous_type_members(&mut self, ty: TypeId) -> CheckResult2<MembersId> {
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::INSTANTIATED)
        {
            // 58317-58321: instantiated shells resolve through the
            // target's members under type.mapper — 5.3's instantiated-
            // reference member tables; without them the symbol-table
            // read below would surface UNMAPPED member types.
            return Err(Unsupported::new(
                "instantiated anonymous type members (resolveAnonymousTypeMembers \
                 target/mapper, M4 5.3)",
            ));
        }
        let symbol = self
            .tables
            .type_of(ty)
            .symbol
            .expect("anonymous member resolution requires a symbol");
        let flags = self.symbol_flags(symbol);
        if flags.intersects(SymbolFlags::TYPE_LITERAL) {
            let members = self.symbol_members(symbol).clone();
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
                .set_type_members(self.speculation_depth, ty, LinkSlot::Resolved(id));
            return Ok(id);
        }
        if flags.intersects(SymbolFlags::FUNCTION | SymbolFlags::METHOD) {
            let call_signatures = self.get_signatures_of_symbol(Some(symbol))?;
            let id = self.alloc_members(ResolvedMembers {
                call_signatures,
                ..ResolvedMembers::default()
            });
            self.links
                .set_type_members(self.speculation_depth, ty, LinkSlot::Resolved(id));
            return Ok(id);
        }
        Err(Unsupported::new(format!(
            "anonymous members for symbol flags {flags:?} (M4)"
        )))
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
        let index_symbol = self
            .symbol_members(symbol)
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
                        declaration,
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
            return Err(Unsupported::new("mapped symbols (getTypeOfMappedSymbol, M8)"));
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
        if flags.intersects(SymbolFlags::FUNCTION | SymbolFlags::METHOD) {
            return self.get_type_of_func_class_enum_module(symbol);
        }
        Err(Unsupported::new(format!(
            "getTypeOfSymbol for symbol flags {flags:?} (M4)"
        )))
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
            let (annotation, is_property, is_optional) =
                state.variable_like_annotation(declaration)?;
            match annotation {
                Some(annotation) => {
                    let declared = state.get_type_from_type_node(annotation)?;
                    Ok(state
                        .tables
                        .add_optionality(declared, is_property, is_optional))
                }
                None => Ok(state.tables.intrinsics.any),
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
        let id = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
        self.tables.type_mut(id).object_flags = ObjectFlags::ANONYMOUS;
        self.tables.type_mut(id).symbol = Some(symbol);
        self.links
            .set_symbol_type(self.speculation_depth, symbol, LinkSlot::Resolved(id));
        Ok(id)
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
        for declaration in declarations {
            if !is_m3_signature_declaration_kind(self.kind_of(declaration)) {
                return Err(Unsupported::new(format!(
                    "signature declaration kind {:?} (M4)",
                    self.kind_of(declaration)
                )));
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
            _ => {
                return Err(Unsupported::new(format!(
                    "signature declaration kind {:?} (M4)",
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
        let mut this_parameter = None;
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
            // non-initialized, non-rest parameter.
            let is_optional_parameter = data.question_token.is_some()
                || data.initializer.is_some()
                || data.dot_dot_dot_token.is_some();
            if !is_optional_parameter {
                min_argument_count = parameters.len() as u32;
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
            declaration,
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
        let annotation = match self.data_of(declaration) {
            NodeData::FunctionType(data) => data.r#type,
            NodeData::ConstructorType(data) => data.r#type,
            NodeData::CallSignature(data) => data.r#type,
            NodeData::ConstructSignature(data) => data.r#type,
            NodeData::MethodSignature(data) => data.r#type,
            _ => None,
        };
        let target = self.signature_of(id).target;
        let computed = match target {
            // 59815: signature.target → instantiate the target's
            // return type through signature.mapper.
            Some(target) => {
                let mapper = self.signature_of(id).mapper;
                self.get_return_type_of_signature(target)
                    .and_then(|target_return| self.instantiate_type(target_return, mapper))
            }
            None => match annotation {
                Some(annotation) => self.get_type_from_type_node(annotation),
                // Annotation-context signatures are bodyless: anyType.
                None => Ok(self.tables.intrinsics.any),
            },
        };
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
                let name = self.name_of_node(declaration);
                match name {
                    Some(name) => {
                        let display = tsrs2_binder::node_util::declaration_name_to_string(
                            self.binder.source_of_node(declaration),
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
                            Some(declaration),
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
fn is_reserved_member_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.first() == Some(&b'_')
        && bytes.get(1) == Some(&b'_')
        && bytes.get(2) != Some(&b'_')
        && bytes.get(2) != Some(&b'@')
        && bytes.get(2) != Some(&b'#')
}

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
        let mut state = CheckerState::new(&source, binder, &options);
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
                for (name, needle) in [
                    ("b", "conditional"),
                    ("c", "unresolved type name"),
                ] {
                    let annotation =
                        find_probe_annotation(state.binder.source(0), name).expect("annotation");
                    let err = state
                        .get_type_from_type_node(annotation)
                        .expect_err("out-of-slice shape must be Unsupported");
                    assert!(
                        err.reason.contains(needle),
                        "{name}: {} should mention {needle}",
                        err.reason
                    );
                }
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
                let members = state.resolve_structured_type_members(declared);
                let reason = members.expect_err("heritage members escape to 5.3").reason;
                assert!(reason.contains("M4 5.3"), "{reason}");
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
                // Reference MEMBERS resolve at 5.3 — relations between
                // instantiated references dead-end honestly for now.
                let related = state.is_type_assignable_to(narrow, wide);
                assert!(related.is_err(), "reference members are a 5.3 row");
                assert!(state
                    .tables
                    .flags_of(narrow)
                    .intersects(TypeFlags::OBJECT));
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
                    state.tables.type_of(instantiated).alias_type_arguments.as_deref(),
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
                assert!(reason.contains("instantiateSignatureInContextOf"), "{reason}");
            },
        );
    }
}
