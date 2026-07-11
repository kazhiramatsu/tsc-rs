//! The MINIMAL type-from-annotation path (m3-types-relations-steps.md
//! stage 4.1) — an explicitly scoped slice of M4 5.1/5.3, each fn a
//! ledgered (partial) port. Everything a TypeMapper would touch is
//! Unsupported by construction; M4 5.1 replaces this module's dispatch
//! with the full getTypeFromTypeNode port.

use tsrs2_binder::{InternalSymbolName, SymbolId};
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{NodeArrayId, NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    ElementFlags, IntersectionFlags, M4Dependency, ObjectFlags, PseudoBigInt, SignatureFlags,
    SymbolFlags, TypeData, TypeFlags, TypeId, UnionReduction,
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

    fn unsupported_m4(err: M4Dependency) -> Unsupported {
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
            SyntaxKind::TypeQuery => Err(Unsupported::new("typeof types (M4 5.1)")),
            SyntaxKind::IndexedAccessType => Err(Unsupported::new("indexed access types (M4 5.2)")),
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
        let elements = self.nodes_of(data.types);
        let mut types = Vec::with_capacity(elements.len());
        for element in elements {
            types.push(self.get_type_from_type_node(element)?);
        }
        let union = self.get_union_type_ex(&types, UnionReduction::Literal)?;
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
        let intersection = self.get_intersection_type(
            &types,
            if no_supertype_reduction {
                IntersectionFlags::NO_SUPERTYPE_REDUCTION
            } else {
                IntersectionFlags::NONE
            },
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
    ///
    /// isDeferredTypeReferenceNode (61068) is constant-false in M3 —
    /// deferral requires type aliases — so only the eager branch is
    /// live; the deferred branch returns with M4 5.1.
    fn get_type_from_array_or_tuple_type_node(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let target = self.get_array_or_tuple_target_type(node)?;
        let elements = match self.data_of(node) {
            NodeData::TupleType(data) => self.nodes_of(data.elements),
            NodeData::ArrayType(_) => {
                unreachable!("array types resolve to the global Array target (M4)")
            }
            _ => unreachable!("array/tuple kind implies payload"),
        };
        let mut element_types = Vec::with_capacity(elements.len());
        for element in elements {
            element_types.push(self.get_type_from_type_node(element)?);
        }
        let resolved = self
            .tables
            .create_normalized_type_reference(target, &element_types)
            .map_err(Self::unsupported_m4)?;
        self.links.set_node_resolved_type(
            self.speculation_depth,
            node,
            LinkSlot::Resolved(resolved),
        );
        Ok(resolved)
    }

    /// tsc-port: getArrayOrTupleTargetType @6.0.3
    /// tsc-hash: 4cf2f8c3a8e8ac36305166ae9a3424a26f2d685e453bd521e3f32be9bf76892e
    /// tsc-span: _tsc.js:61056-61064
    fn get_array_or_tuple_target_type(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let readonly = self
            .parent_of(node)
            .is_some_and(|parent| self.is_readonly_type_operator(parent));
        if self.get_array_element_type_node(node).is_some() {
            return Err(Unsupported::new(
                "globalArrayType/globalReadonlyArrayType targets (M4 5.3)",
            ));
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
            SyntaxKind::KeyOfKeyword => Err(Unsupported::new("keyof types (M4 5.2)")),
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
        let resolved = match symbol {
            None => self.empty_type_literal_type,
            Some(symbol) if self.symbol_members(symbol).is_empty() => self.empty_type_literal_type,
            Some(symbol) => {
                let id = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
                self.tables.type_mut(id).object_flags = ObjectFlags::ANONYMOUS;
                self.tables.type_mut(id).symbol = Some(symbol);
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
    /// getTypeReferenceType (60380-60405): non-generic class/interface
    /// dispatch only. Type aliases, enums and type arguments are M4
    /// 5.1b/5.3b rows. An unresolved name is tsc's unknownSymbol →
    /// errorType; the probe keeps the Unsupported channel until the
    /// 5.4 driver makes errorType observable through diagnostics.
    fn get_type_from_type_reference(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let NodeData::TypeReference(data) = self.data_of(node) else {
            unreachable!("TypeReference kind implies payload");
        };
        if data.type_arguments.is_some() {
            return Err(Unsupported::new("generic type references (M4 5.1)"));
        }
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
            // getTypeReferenceType → getDeclaredTypeOfSymbol's type-
            // parameter arm; a type-argument list on a type-parameter
            // reference is the 2315 family (M4 5.1 grammar row —
            // type_arguments already escaped above).
            self.get_declared_type_of_type_parameter(symbol)
        } else if flags.intersects(SymbolFlags::INTERFACE) {
            self.get_declared_type_of_class_or_interface(symbol)?
        } else if flags.intersects(SymbolFlags::CLASS) {
            return Err(Unsupported::new("class declared types (M4 5.3)"));
        } else if flags.intersects(SymbolFlags::TYPE_ALIAS) {
            return Err(Unsupported::new("type aliases (M4 5.1)"));
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

    /// tsc-port: getDeclaredTypeOfClassOrInterface @6.0.3
    /// tsc-hash: b159a970fade450a929f147df283c2d536e3a3459c66ac6b6e9b9675173ef57c
    /// tsc-span: _tsc.js:57375-57403
    ///
    /// M3 slice: the pure non-generic THISLESS interface branch — a
    /// plain InterfaceType (ObjectFlags::Interface, no Reference/
    /// thisType/typeParameters). Everything that makes the 57383 guard
    /// true (type parameters, classes, `this` usage, heritage — per
    /// isThislessInterface 57346-57374) is an M4 row.
    pub(crate) fn get_declared_type_of_class_or_interface(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.symbol(symbol).declared_type.resolved() {
            return Ok(cached);
        }
        let declarations = self.binder.symbol(symbol).declarations.clone();
        for declaration in &declarations {
            if self.kind_of(*declaration) != SyntaxKind::InterfaceDeclaration {
                continue;
            }
            let NodeData::InterfaceDeclaration(data) = self.data_of(*declaration) else {
                unreachable!("InterfaceDeclaration kind implies payload");
            };
            if data
                .type_parameters
                .is_some_and(|list| !self.binder.node_array(list).nodes.is_empty())
            {
                return Err(Unsupported::new("generic interfaces (M4 5.1)"));
            }
            if data
                .heritage_clauses
                .is_some_and(|list| !self.binder.node_array(list).nodes.is_empty())
            {
                return Err(Unsupported::new("interface heritage/base types (M4 5.3)"));
            }
            // isThislessInterface: any declaration with ContainsThis
            // makes the interface a GenericType with a thisType.
            if self.node_flags(*declaration) & tsrs2_types::NodeFlags::CONTAINS_THIS.bits() != 0 {
                return Err(Unsupported::new(
                    "interfaces referencing `this` (M4 5.3 thisType)",
                ));
            }
        }
        let id = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
        self.tables.type_mut(id).object_flags = ObjectFlags::INTERFACE;
        self.tables.type_mut(id).symbol = Some(symbol);
        self.links
            .set_symbol_declared_type(self.speculation_depth, symbol, LinkSlot::Resolved(id));
        Ok(id)
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
    /// M3 dispatch: variable/property symbols (annotation-typed) and
    /// function/method symbols. CheckFlags-dispatched transients,
    /// accessors, classes, enums, modules and aliases are M4.
    pub fn get_type_of_symbol(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
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
    /// M3 slice: annotation-only signatures. IIFE/JS/JSDoc branches and
    /// accessor this-borrowing are dead here; generic signatures
    /// (typeParameters, 59628-59630) are M4 rows.
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
        if type_parameters.is_some_and(|list| !self.binder.node_array(list).nodes.is_empty()) {
            return Err(Unsupported::new("generic signatures (M4 5.1)"));
        }
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
            parameters,
            this_parameter,
            min_argument_count,
            resolved_return_type: LinkSlot::Vacant,
            from_method: self.kind_of(declaration) == SyntaxKind::MethodSignature,
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
    /// Slice: the annotation branch + the bodyless anyType fallback
    /// (59815: nodeIsMissing(body) → anyType). Instantiation targets,
    /// composites, call-chain optionality and body inference are
    /// 5.2/5.5/M6. Cycles run on the resolution stack (59812/59821);
    /// an Err unwind pops the stack and leaves the slot Vacant
    /// (M3-review Resolving-dangling fix).
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
        let computed = match annotation {
            Some(annotation) => self.get_type_from_type_node(annotation),
            // Annotation-context signatures are bodyless: anyType.
            None => Ok(self.tables.intrinsics.any),
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
fn parse_numeric_literal_text(text: &str) -> CheckResult2<f64> {
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
            "declare var a: number[];\ndeclare var b: keyof { x: 1 };\ndeclare var c: Missing;\n",
            |state| {
                for (name, needle) in [
                    ("a", "globalArrayType"),
                    ("b", "keyof"),
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
