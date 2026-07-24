//! M4 5.5f + 5.7c: the JSX band — eager element/fragment arms, JSX
//! grammar (2633/2639/17001/17000/18007) and preconditions (17004)
//! landed at 5.5f; 5.7c adds everything past the old
//! `getResolvedSignature` boundary: getIntrinsicTagSymbol +
//! intrinsic/fragment/value-tag resolution (7026/2339/2604/2879/
//! 2558), the attributes worker (2698/2710/2608), effective-first-arg
//! (2607) and the post-resolution tail (2786-family). Attributes-vs-
//! props relation FAILURES largely contain: elaborateJsxComponents is
//! elementwise elaboration (T2 machinery) and the anonymous
//! attributes-type display rides the T2 curtain.
//!
//! Namespace machinery includes jsxFactory-family options, leading
//! @jsx pragmas, and react-jsx implicit runtime imports.

use tsrs2_binder::{SymbolId, SymbolTable};
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    CheckMode, ContextFlags, IntersectionFlags, JsxFlags, ObjectFlags, SymbolFlags, TypeData,
    TypeFlags, TypeId,
};

use crate::structural::SignatureKind;

use crate::links::LinkSlot;
use crate::state::{CheckResult2, CheckerState, SignatureId, Unsupported};
use tsrs2_diags::gen as diagnostics;
use tsrs2_diags::MessageChain;

/// JsxNames (90915): the JSX.* well-known member names this slice
/// consults.
const JSX_NAMESPACE_NAME: &str = "JSX";
const JSX_ELEMENT: &str = "Element";
const JSX_INTRINSIC_ELEMENTS: &str = "IntrinsicElements";
const JSX_ELEMENT_CLASS: &str = "ElementClass";
const JSX_ELEMENT_ATTRIBUTES_PROPERTY_NAME_CONTAINER: &str = "ElementAttributesProperty";
const JSX_ELEMENT_CHILDREN_ATTRIBUTE_NAME_CONTAINER: &str = "ElementChildrenAttribute";
const JSX_ELEMENT_TYPE: &str = "ElementType";
const JSX_INTRINSIC_ATTRIBUTES: &str = "IntrinsicAttributes";
const JSX_INTRINSIC_CLASS_ATTRIBUTES: &str = "IntrinsicClassAttributes";
const JSX_LIBRARY_MANAGED_ATTRIBUTES: &str = "LibraryManagedAttributes";
/// ReactNames (90927).
const REACT_FRAGMENT: &str = "Fragment";

#[derive(Clone, Debug, Default)]
struct JsxPragmaSettings {
    factory: Option<String>,
    fragment_factory: Option<String>,
    import_source: Option<String>,
    runtime: Option<String>,
}

fn leading_jsx_pragmas(text: &str) -> JsxPragmaSettings {
    fn collect(comment: &str, settings: &mut JsxPragmaSettings) {
        // tsc's multiLinePragmaRegEx: one non-whitespace pragma name
        // and a required argument extending to the end of that line.
        // JSX pragmas are MultiLine-only; `// @jsx` is deliberately
        // not collected.
        for line in comment.split(['\n', '\r', '\u{2028}', '\u{2029}']) {
            let Some(at) = line.find('@') else { continue };
            let tail = &line[at + 1..];
            let name_end = tail.find(char::is_whitespace).unwrap_or(tail.len());
            let name = tail[..name_end].to_ascii_lowercase();
            let value = tail[name_end..].trim().to_owned();
            if !value.is_empty() {
                match name.as_str() {
                    "jsx" if settings.factory.is_none() => settings.factory = Some(value),
                    "jsxfrag" if settings.fragment_factory.is_none() => {
                        settings.fragment_factory = Some(value)
                    }
                    "jsximportsource" => settings.import_source = Some(value),
                    "jsxruntime" => settings.runtime = Some(value),
                    _ => {}
                }
            }
        }
    }

    let mut settings = JsxPragmaSettings::default();
    let mut offset = if text.starts_with("#!") {
        text.find(['\n', '\r', '\u{2028}', '\u{2029}'])
            .unwrap_or(text.len())
    } else {
        0
    };
    loop {
        while let Some(character) = text[offset..].chars().next() {
            if character.is_whitespace() || character == '\u{FEFF}' {
                offset += character.len_utf8();
            } else {
                break;
            }
        }
        let rest = &text[offset..];
        if let Some(comment) = rest.strip_prefix("//") {
            let end = comment
                .find(['\n', '\r', '\u{2028}', '\u{2029}'])
                .unwrap_or(comment.len());
            offset += 2 + end;
            continue;
        }
        if let Some(comment) = rest.strip_prefix("/*") {
            let end = comment.find("*/").unwrap_or(comment.len());
            collect(&comment[..end], &mut settings);
            offset += 2 + end + usize::from(end < comment.len()) * 2;
            continue;
        }
        break;
    }
    settings
}

fn first_entity_identifier(entity: &str) -> Option<String> {
    let valid_identifier = |part: &str| {
        !part.is_empty()
            && part.chars().next().is_some_and(|character| {
                character == '_' || character == '$' || character.is_alphabetic()
            })
            && part.chars().all(|character| {
                character == '_' || character == '$' || character.is_alphanumeric()
            })
    };
    let mut parts = entity.split('.').map(str::trim);
    let first = parts.next()?;
    if !valid_identifier(first) || !parts.all(valid_identifier) {
        return None;
    }
    Some(first.to_owned())
}

/// tsc JsxReferenceKind (getJsxReferenceKind 76075).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JsxReferenceKind {
    Component,
    Function,
    Mixed,
}

impl<'a> CheckerState<'a> {
    // ---- eager worker arms ----

    /// tsc-port: checkJsxSelfClosingElement @6.0.3
    /// tsc-hash: e3207ce198d2810ef11c6b79962641dc40d79b7d3447ed72f9baa6b4a9adaf40
    /// tsc-span: _tsc.js:74307-74310
    pub(crate) fn check_jsx_self_closing_element(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        self.check_node_deferred(node);
        // `getJsxElementTypeAt(node) || anyType`: getJsxType answers
        // errorType (truthy) when JSX.Element is missing — the anyType
        // fallback is defensively dead; errorType IS the result.
        self.get_jsx_element_type_at(node)
    }

    /// tsc-port: checkJsxElement @6.0.3
    /// tsc-hash: 60686da722fc562e5cc36c0bd134a14ade1e9f6a32853f38c676fe9606be5427
    /// tsc-span: _tsc.js:74320-74323
    pub(crate) fn check_jsx_element(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        self.check_node_deferred(node);
        // Same `|| anyType` note as the self-closing arm.
        self.get_jsx_element_type_at(node)
    }

    /// tsc-port: checkJsxFragment @6.0.3
    /// tsc-hash: 8295e0ce6f62141e10f2947fcd1c218f0745c4f3ff4e852c7061c41e12d2def8
    /// tsc-span: _tsc.js:74324-74336
    ///
    /// The 17016/17017 pragma-factory errors read jsxFactory/pragmas.
    pub(crate) fn check_jsx_fragment(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let opening_fragment = match self.data_of(node) {
            NodeData::JsxFragment(data) => data.opening_fragment,
            _ => None,
        };
        if let Some(opening_fragment) = opening_fragment {
            self.check_jsx_opening_like_element_or_opening_fragment(opening_fragment)?;
        }
        let pragmas = leading_jsx_pragmas(&self.binder.source_of_node(node).text);
        if matches!(self.options.jsx, Some(2) | Some(4) | Some(5))
            && (self.options.jsx_factory.is_some() || pragmas.factory.is_some())
            && self.options.jsx_fragment_factory.is_none()
            && pragmas.fragment_factory.is_none()
        {
            let message = if self.options.jsx_factory.is_some() {
                &diagnostics::The_jsxFragmentFactory_compiler_option_must_be_provided_to_use_JSX_fragments_with_the_jsxFactory_compiler_option
            } else {
                &diagnostics::An_jsxFrag_pragma_is_required_when_using_an_jsx_pragma_with_JSX_fragments
            };
            self.error_at(Some(node), message, &[]);
        }
        self.check_jsx_children(node, tsrs2_types::CheckMode::NORMAL)?;
        let element_type = self.get_jsx_element_type_at(node)?;
        Ok(if self.tables.is_error_type(element_type) {
            self.tables.intrinsics.any
        } else {
            element_type
        })
    }

    /// tsc-port: checkJsxExpression @6.0.3
    /// tsc-hash: 0cbbb4729f6068be0372dc86ca60643a2454d1588f8b1395380e8867a2a26dfd
    /// tsc-span: _tsc.js:74847-74858
    pub(crate) fn check_jsx_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        self.check_grammar_jsx_expression(node);
        let NodeData::JsxExpression(data) = self.data_of(node).clone() else {
            return Ok(self.tables.intrinsics.error);
        };
        let Some(expression) = data.expression else {
            return Ok(self.tables.intrinsics.error);
        };
        // 74494 threads checkMode into the inner expression — a
        // NORMAL hardcode here dropped SkipContextSensitive during
        // 7.4's inference trials, so context-sensitive attribute
        // functions assigned their parameters through the FIXING
        // mapper mid-pass-1 and pinned type parameters before any
        // candidate landed (the intraExpressionInferencesJsx 18046
        // face).
        let ty = self.check_expression(expression, check_mode)?;
        if data.dot_dot_dot_token.is_some()
            && ty != self.tables.intrinsics.any
            && !self.is_array_type(ty)?
        {
            self.error_at(
                Some(node),
                &diagnostics::JSX_spread_child_must_be_an_array_type,
                &[],
            );
        }
        Ok(ty)
    }

    /// tsc-port: checkJsxAttributes @6.0.3
    /// tsc-hash: ceed171a5b09ff0e3d5ee2df7229d3afa2e83ed0c8bd34c36f8f3006a32c0c8a
    /// tsc-span: _tsc.js:74522-74524
    pub(crate) fn check_jsx_attributes(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let parent = self.parent_of(node).ok_or_else(|| {
            Unsupported::new("JSX attributes without an element (parse recovery)")
        })?;
        self.create_jsx_attributes_type_from_attributes_property(parent, check_mode)
    }

    /// tsc-port: checkJsxAttribute @6.0.3
    /// tsc-hash: 91408c9840bf8a783d7d1ced8d401c91a4ece4e3cd97ffefcc87f31685bf5852
    /// tsc-span: _tsc.js:74343-74345
    pub(crate) fn check_jsx_attribute(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let initializer = match self.data_of(node) {
            NodeData::JsxAttribute(data) => data.initializer,
            _ => None,
        };
        match initializer {
            Some(initializer) => {
                self.check_expression_for_mutable_location(initializer, check_mode, false)
            }
            None => Ok(self.tables.intrinsics.true_fresh),
        }
    }

    /// tsc-port: createJsxAttributesTypeFromAttributesProperty @6.0.3
    /// tsc-hash: 0a9b4693e940d88eb7c1f44bf68f18be234122787ee12bdd1cee1f6250e83715
    /// tsc-span: _tsc.js:74346-74490
    ///
    /// Divergences held to the T2/M6 lines: the deprecation-suggestion
    /// probe (74381-74386) is suggestion-band (unmodeled JSDoc) — the
    /// access.rs precedent; addIntraExpressionInferenceSite requires a
    /// live inference context (Inferential never set at M4 — escape
    /// mirrors literals.rs); the synthesized children property carries
    /// NO fabricated PropertySignature valueDeclaration (node
    /// fabrication is unavailable — its consumers are display/related-
    /// span side, T2).
    pub(crate) fn create_jsx_attributes_type_from_attributes_property(
        &mut self,
        opening_like_element: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let strict_null_checks = self.tables.strict_null_checks;
        let mut all_attributes_table = strict_null_checks.then(SymbolTable::default);
        let mut attributes_table = SymbolTable::default();
        let mut spread = self.empty_jsx_object_type;
        let mut has_spread_any_type = false;
        let mut type_to_intersect: Option<TypeId> = None;
        let mut explicitly_specify_children_attribute = false;
        let mut object_flags = ObjectFlags::JSX_ATTRIBUTES;
        let jsx_namespace = self.get_jsx_namespace_at(opening_like_element)?;
        let jsx_children_property_name =
            self.get_jsx_element_children_property_name(jsx_namespace)?;
        let is_jsx_open_fragment =
            self.kind_of(opening_like_element) == SyntaxKind::JsxOpeningFragment;
        let mut attributes_symbol: Option<SymbolId> = None;
        let mut attribute_parent = opening_like_element;
        if !is_jsx_open_fragment {
            let attributes = match self.data_of(opening_like_element) {
                NodeData::JsxOpeningElement(data) => data.attributes,
                NodeData::JsxSelfClosingElement(data) => data.attributes,
                _ => None,
            }
            .ok_or_else(|| {
                Unsupported::new("JSX opening element without attributes (parse recovery)")
            })?;
            attributes_symbol = self.node_symbol(attributes);
            attribute_parent = attributes;
            let contextual_type = self.get_contextual_type(attributes, ContextFlags::NONE)?;
            let properties = match self.data_of(attributes) {
                NodeData::JsxAttributes(data) => data.properties,
                _ => None,
            };
            for attribute_decl in self.nodes_of(properties) {
                if self.kind_of(attribute_decl) == SyntaxKind::JsxAttribute {
                    let member = self.node_symbol(attribute_decl).ok_or_else(|| {
                        Unsupported::new("JSX attribute without a bound symbol (parse recovery)")
                    })?;
                    let expr_type = self.check_jsx_attribute(attribute_decl, check_mode)?;
                    object_flags |=
                        self.tables.object_flags_of(expr_type) & ObjectFlags::PROPAGATING_FLAGS;
                    let member_flags = self.binder.symbol(member).flags;
                    let escaped_name = self.binder.symbol(member).escaped_name.clone();
                    let attribute_symbol = self
                        .binder
                        .create_symbol(SymbolFlags::PROPERTY | member_flags, escaped_name.clone());
                    let declarations = self.binder.symbol(member).declarations.clone();
                    let parent = self.binder.symbol(member).parent;
                    let value_declaration = self.binder.symbol(member).value_declaration;
                    {
                        let symbol = self.binder.symbol_mut(attribute_symbol);
                        symbol.declarations = declarations;
                        symbol.parent = parent;
                        if let Some(value_declaration) = value_declaration {
                            symbol.value_declaration = Some(value_declaration);
                        }
                    }
                    self.links
                        .set_fresh_symbol_type(attribute_symbol, LinkSlot::Resolved(expr_type));
                    self.links
                        .set_symbol_target(self.speculation_depth, attribute_symbol, member);
                    attributes_table.insert(escaped_name.clone(), attribute_symbol);
                    if let Some(all) = &mut all_attributes_table {
                        all.insert(escaped_name, attribute_symbol);
                    }
                    let name_text = match self.data_of(attribute_decl) {
                        NodeData::JsxAttribute(data) => {
                            data.name.map(|name| self.jsx_attribute_name_text(name))
                        }
                        _ => None,
                    };
                    if let (Some(name_text), Some(children_name)) =
                        (&name_text, &jsx_children_property_name)
                    {
                        if name_text == children_name {
                            explicitly_specify_children_attribute = true;
                        }
                    }
                    // 74381-74386: addDeprecatedSuggestion — the
                    // suggestion band rides JSDoc @deprecated
                    // (unmodeled); elided like access.rs.
                    // 74387-74392: the intra-expression inference-site
                    // record (7.4) — the attribute INITIALIZER'S
                    // expression, against the ATTRIBUTES context node.
                    if contextual_type.is_some()
                        && check_mode.intersects(CheckMode::INFERENTIAL)
                        && !check_mode.intersects(CheckMode::SKIP_CONTEXT_SENSITIVE)
                        && self.is_context_sensitive(attribute_decl)
                    {
                        let inference_context = self
                            .get_inference_context(attributes)
                            .expect("Inferential check mode implies an inference context (74389)");
                        let initializer_expression = match self.data_of(attribute_decl) {
                            NodeData::JsxAttribute(data) => {
                                data.initializer.and_then(|initializer| {
                                    match self.data_of(initializer) {
                                        NodeData::JsxExpression(data) => data.expression,
                                        _ => None,
                                    }
                                })
                            }
                            _ => None,
                        }
                        .ok_or_else(|| {
                            Unsupported::new(
                                "JSX attribute inference site without an initializer expression (parse recovery)",
                            )
                        })?;
                        self.add_intra_expression_inference_site(
                            inference_context,
                            initializer_expression,
                            expr_type,
                        );
                    }
                } else {
                    // Debug.assert(JsxSpreadAttribute) — recovery kinds
                    // take a named escape instead of a panic (risk #6).
                    if self.kind_of(attribute_decl) != SyntaxKind::JsxSpreadAttribute {
                        return Err(Unsupported::new(
                            "unexpected JSX attribute kind (parse recovery)",
                        ));
                    }
                    if !attributes_table.is_empty() {
                        let segment = self.create_jsx_attributes_segment(
                            &mut object_flags,
                            attributes_symbol,
                            &attributes_table,
                        );
                        spread = self.get_spread_type(
                            spread,
                            segment,
                            attributes_symbol,
                            object_flags,
                            /*readonly*/ false,
                        )?;
                        attributes_table = SymbolTable::default();
                    }
                    let expression = match self.data_of(attribute_decl) {
                        NodeData::JsxSpreadAttribute(data) => data.expression,
                        _ => None,
                    }
                    .ok_or_else(|| {
                        Unsupported::new("JSX spread without an expression (parse recovery)")
                    })?;
                    let inner_mode =
                        CheckMode::from_bits(check_mode.bits() & CheckMode::INFERENTIAL.bits());
                    let raw = self.check_expression(expression, inner_mode)?;
                    let expr_type = self.get_reduced_type(raw)?;
                    if self.tables.flags_of(expr_type).intersects(TypeFlags::ANY) {
                        has_spread_any_type = true;
                    }
                    if self.is_valid_spread_type(expr_type)? {
                        spread = self.get_spread_type(
                            spread,
                            expr_type,
                            attributes_symbol,
                            object_flags,
                            /*readonly*/ false,
                        )?;
                        if let Some(all) = &all_attributes_table {
                            let all = all.clone();
                            self.check_spread_prop_overrides(expr_type, &all, attribute_decl)?;
                        }
                    } else {
                        self.error_at(
                            Some(expression),
                            &diagnostics::Spread_types_may_only_be_created_from_object_types,
                            &[],
                        );
                        type_to_intersect = Some(match type_to_intersect {
                            Some(existing) => self.get_intersection_type(
                                &[existing, expr_type],
                                IntersectionFlags::NONE,
                            )?,
                            None => expr_type,
                        });
                    }
                }
            }
            if !has_spread_any_type && !attributes_table.is_empty() {
                let segment = self.create_jsx_attributes_segment(
                    &mut object_flags,
                    attributes_symbol,
                    &attributes_table,
                );
                spread = self.get_spread_type(
                    spread,
                    segment,
                    attributes_symbol,
                    object_flags,
                    /*readonly*/ false,
                )?;
            }
        }
        // 74441-74478: the children property synthesis.
        let parent = self.parent_of(opening_like_element);
        let element_with_children = parent.filter(|&parent| match self.data_of(parent) {
            NodeData::JsxElement(data) => data.opening_element == Some(opening_like_element),
            NodeData::JsxFragment(data) => data.opening_fragment == Some(opening_like_element),
            _ => false,
        });
        if let Some(element) = element_with_children {
            let children = match self.data_of(element) {
                NodeData::JsxElement(data) => data.children,
                NodeData::JsxFragment(data) => data.children,
                _ => None,
            };
            let semantic_children = self
                .nodes_of(children)
                .into_iter()
                .filter(|&child| self.is_semantic_jsx_child(child))
                .count();
            if semantic_children > 0 {
                let children_types = self.check_jsx_children(element, check_mode)?;
                if let Some(children_name) = jsx_children_property_name
                    .as_ref()
                    .filter(|name| !has_spread_any_type && !name.is_empty())
                {
                    if explicitly_specify_children_attribute {
                        let display = tsrs2_binder::unescape_leading_underscores(children_name);
                        self.error_at(
                            Some(attribute_parent),
                            &diagnostics::_0_are_specified_twice_The_attribute_named_0_will_be_overwritten,
                            &[display],
                        );
                    }
                    let contextual_type =
                        if self.kind_of(opening_like_element) == SyntaxKind::JsxOpeningElement {
                            self.get_apparent_type_of_contextual_type(
                                attribute_parent,
                                ContextFlags::NONE,
                            )?
                        } else {
                            None
                        };
                    let children_contextual_type = match contextual_type {
                        Some(contextual_type) => self.get_type_of_property_of_contextual_type(
                            contextual_type,
                            children_name,
                            None,
                        )?,
                        None => None,
                    };
                    let children_prop_symbol = self
                        .binder
                        .create_symbol(SymbolFlags::PROPERTY, children_name.clone());
                    let children_prop_type = if children_types.len() == 1 {
                        children_types[0]
                    } else if self
                        .contextual_children_type_is_tuple_like(children_contextual_type)?
                    {
                        self.create_tuple_type_forced(&children_types, None, false, None)?
                    } else {
                        let union = self.get_union_type_ex(
                            &children_types,
                            tsrs2_types::UnionReduction::Literal,
                        )?;
                        self.create_array_type(union, /*readonly*/ false)?
                    };
                    self.links.set_fresh_symbol_type(
                        children_prop_symbol,
                        LinkSlot::Resolved(children_prop_type),
                    );
                    // The fabricated PropertySignature valueDeclaration
                    // (74456-74466) is elided — see the fn header.
                    let mut child_prop_map = SymbolTable::default();
                    child_prop_map.insert(children_name.clone(), children_prop_symbol);
                    let child_type = self.make_resolved_anonymous_type(
                        attributes_symbol,
                        child_prop_map,
                        vec![children_prop_symbol],
                        Vec::new(),
                        ObjectFlags::ANONYMOUS,
                    );
                    let mut propagating = ObjectFlags::from_bits(0);
                    for &child in &children_types {
                        propagating |=
                            self.tables.object_flags_of(child) & ObjectFlags::PROPAGATING_FLAGS;
                    }
                    spread = self.get_spread_type(
                        spread,
                        child_type,
                        attributes_symbol,
                        object_flags | propagating,
                        /*readonly*/ false,
                    )?;
                }
            }
        }
        if has_spread_any_type {
            return Ok(self.tables.intrinsics.any);
        }
        if let Some(type_to_intersect) = type_to_intersect {
            if spread != self.empty_jsx_object_type {
                return self
                    .get_intersection_type(&[type_to_intersect, spread], IntersectionFlags::NONE);
            }
            return Ok(type_to_intersect);
        }
        if spread == self.empty_jsx_object_type {
            return Ok(self.create_jsx_attributes_segment(
                &mut object_flags,
                attributes_symbol,
                &attributes_table,
            ));
        }
        Ok(spread)
    }

    /// The createJsxAttributesTypeHelper closure (74486-74489) +
    /// createJsxAttributesType (74491-74495): a FRESH anonymous type
    /// over the current segment table; the helper mutates the shared
    /// objectFlags word (FreshLiteral accumulates).
    fn create_jsx_attributes_segment(
        &mut self,
        object_flags: &mut ObjectFlags,
        attributes_symbol: Option<SymbolId>,
        attributes_table: &SymbolTable,
    ) -> TypeId {
        *object_flags |= ObjectFlags::FRESH_LITERAL;
        let flags = ObjectFlags::ANONYMOUS
            | *object_flags
            | ObjectFlags::FRESH_LITERAL
            | ObjectFlags::OBJECT_LITERAL
            | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL;
        let properties: Vec<SymbolId> = attributes_table.values().copied().collect();
        self.make_resolved_anonymous_type(
            attributes_symbol,
            attributes_table.clone(),
            properties,
            Vec::new(),
            flags,
        )
    }

    /// someType(childrenContextualType, isTupleLikeType) (74455) —
    /// the union-distributing probe, result-carrying.
    fn contextual_children_type_is_tuple_like(
        &mut self,
        children_contextual_type: Option<TypeId>,
    ) -> CheckResult2<bool> {
        let Some(ty) = children_contextual_type else {
            return Ok(false);
        };
        let constituents: Vec<TypeId> = if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } => types.to_vec(),
                _ => vec![ty],
            }
        } else {
            vec![ty]
        };
        for constituent in constituents {
            if self.is_tuple_like_type(constituent)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsc-port: getSemanticJsxChildren @6.0.3 (the per-child predicate)
    /// tsc-hash: 6f3f6ec374677f5ec862f5979a10f36bbed5b161dd1f0ba688e0a2003f12bc8d
    /// tsc-span: _tsc.js:16187-16200
    pub(crate) fn is_semantic_jsx_child(&self, child: NodeId) -> bool {
        match self.data_of(child) {
            NodeData::JsxExpression(data) => data.expression.is_some(),
            NodeData::JsxText(data) => !data.contains_only_trivia_white_spaces,
            _ => true,
        }
    }

    // ---- deferred arms ----

    /// tsc-port: checkJsxElementDeferred @6.0.3
    /// tsc-hash: 2068fd98f6f9b417a668c1ce64f5e2eddbad0fbfac7de3c282bb47c997bc6776
    /// tsc-span: _tsc.js:74311-74319
    pub(crate) fn check_jsx_element_deferred(&mut self, node: NodeId) -> CheckResult2<()> {
        let (opening_element, closing_element) = match self.data_of(node) {
            NodeData::JsxElement(data) => (data.opening_element, data.closing_element),
            _ => (None, None),
        };
        if let Some(opening_element) = opening_element {
            self.check_jsx_opening_like_element_or_opening_fragment(opening_element)?;
        }
        if let Some(closing_element) = closing_element {
            let tag_name = match self.data_of(closing_element) {
                NodeData::JsxClosingElement(data) => data.tag_name,
                _ => None,
            };
            if let Some(tag_name) = tag_name {
                if self.is_jsx_intrinsic_tag_name(tag_name) {
                    self.get_intrinsic_tag_symbol(closing_element)?;
                } else {
                    self.check_expression(tag_name, CheckMode::NORMAL)?;
                }
            }
        }
        self.check_jsx_children(node, CheckMode::NORMAL)?;
        Ok(())
    }

    /// tsc-port: checkJsxSelfClosingElementDeferred @6.0.3
    /// tsc-hash: f6f5be939a71796a6fcd510f53e32f83dbe02ba475d5e6bf13e63302298ac7df
    /// tsc-span: _tsc.js:74304-74306
    pub(crate) fn check_jsx_self_closing_element_deferred(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<()> {
        self.check_jsx_opening_like_element_or_opening_fragment(node)
    }

    /// tsc-port: checkJsxOpeningLikeElementOrOpeningFragment @6.0.3
    /// tsc-hash: 2f04a1ed5759553f45561927e599df29785b0fc79d910ce8e4e72c25b3c1d0a9
    /// tsc-span: _tsc.js:74797-74825
    ///
    /// markJsxAliasReferenced (71787) is emit/alias bookkeeping — no-op
    /// hook. checkDeprecatedSignature is a no-op like the call worker's
    /// (the Deprecated flag only comes from JSDoc `@deprecated`,
    /// unmodeled — the 5.7b flag audit).
    pub(crate) fn check_jsx_opening_like_element_or_opening_fragment(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<()> {
        let is_opening_like = matches!(
            self.kind_of(node),
            SyntaxKind::JsxOpeningElement | SyntaxKind::JsxSelfClosingElement
        );
        if is_opening_like {
            self.check_grammar_jsx_element(node);
        }
        self.check_jsx_preconditions(node)?;
        self.mark_jsx_alias_referenced(node)?;
        let signature = self.get_resolved_signature(node, CheckMode::NORMAL)?;
        // checkDeprecatedSignature: no-op (see the fn header).
        if is_opening_like {
            let element_type_constraint = self.get_jsx_element_type_type_at(node)?;
            let tag_name = match self.data_of(node) {
                NodeData::JsxOpeningElement(data) => data.tag_name,
                NodeData::JsxSelfClosingElement(data) => data.tag_name,
                _ => None,
            }
            .ok_or_else(|| {
                Unsupported::new("JSX opening element without a tag name (parse recovery)")
            })?;
            if let Some(constraint) = element_type_constraint {
                let tag_type = if self.is_jsx_intrinsic_tag_name(tag_name) {
                    let text = self.intrinsic_tag_name_to_string(tag_name)?;
                    self.tables.get_string_literal_type(&text)
                } else {
                    self.check_expression(tag_name, CheckMode::NORMAL)?
                };
                self.check_jsx_bound_relation(
                    tag_type,
                    constraint,
                    tag_name,
                    &diagnostics::Its_type_0_is_not_a_valid_JSX_element_type,
                )?;
            } else {
                let ref_kind = self.get_jsx_reference_kind(node)?;
                let instance_type = self.get_return_type_of_signature(signature)?;
                self.check_jsx_return_assignable_to_appropriate_bound(
                    ref_kind,
                    instance_type,
                    node,
                    tag_name,
                )?;
            }
        }
        Ok(())
    }

    /// tsc-port: checkJsxReturnAssignableToAppropriateBound @6.0.3
    /// tsc-hash: f2e804bf0cd9a84cfa9cbb8d440df8b5b28180ac83781710e7ce17d31a43fe19
    /// tsc-span: _tsc.js:74698-74727
    fn check_jsx_return_assignable_to_appropriate_bound(
        &mut self,
        ref_kind: JsxReferenceKind,
        elem_instance_type: TypeId,
        opening_like_element: NodeId,
        tag_name: NodeId,
    ) -> CheckResult2<()> {
        match ref_kind {
            JsxReferenceKind::Function => {
                let constraint = self.get_jsx_stateless_element_type_at(opening_like_element)?;
                if let Some(constraint) = constraint {
                    self.check_jsx_bound_relation(
                        elem_instance_type,
                        constraint,
                        tag_name,
                        &diagnostics::Its_return_type_0_is_not_a_valid_JSX_element,
                    )?;
                }
            }
            JsxReferenceKind::Component => {
                let constraint = self.get_jsx_element_class_type_at(opening_like_element)?;
                if let Some(constraint) = constraint {
                    self.check_jsx_bound_relation(
                        elem_instance_type,
                        constraint,
                        tag_name,
                        &diagnostics::Its_instance_type_0_is_not_a_valid_JSX_element,
                    )?;
                }
            }
            JsxReferenceKind::Mixed => {
                let sfc = self.get_jsx_stateless_element_type_at(opening_like_element)?;
                let class = self.get_jsx_element_class_type_at(opening_like_element)?;
                let (Some(sfc), Some(class)) = (sfc, class) else {
                    return Ok(());
                };
                let combined =
                    self.get_union_type_ex(&[sfc, class], tsrs2_types::UnionReduction::Literal)?;
                self.check_jsx_bound_relation(
                    elem_instance_type,
                    combined,
                    tag_name,
                    &diagnostics::Its_element_type_0_is_not_a_valid_JSX_element,
                )?;
            }
        }
        Ok(())
    }

    /// checkTypeRelatedTo(source, target, assignable, errorNode=tagName,
    /// headMessage, generateInitialErrorChain) — the 2786-family
    /// reporter: the OUTER chain (and diagnostic code) is 2786
    /// `_0_cannot_be_used_as_a_JSX_component` over the flavor head; the
    /// relation-detail tail elides (T2). Display failures unwind
    /// Unsupported per the house discipline.
    fn check_jsx_bound_relation(
        &mut self,
        source: TypeId,
        target: TypeId,
        tag_name: NodeId,
        head: &'static tsrs2_diags::DiagnosticMessage,
    ) -> CheckResult2<()> {
        if self.is_type_assignable_to(source, target)? {
            return Ok(());
        }
        let source_text = self.type_to_string_slice(source)?;
        let target_text = self.type_to_string_slice(target)?;
        let component_name = self.text_of_node(tag_name)?;
        let chain = MessageChain::new(
            &diagnostics::_0_cannot_be_used_as_a_JSX_component,
            &[component_name],
        )
        .with_next(vec![MessageChain::new(head, &[source_text, target_text])]);
        let span = self.diag_span_of_node(tag_name);
        let diagnostic = self.diagnostic_at_span(&span, chain);
        self.push_error_diagnostic(diagnostic);
        Ok(())
    }

    /// tsc-port: checkJsxPreconditions @6.0.3
    /// tsc-hash: a6d8c793e2d659fbd7e3a02445f9a3d8a1eda5d6f6b35e043eb0b45e6045c00d
    /// tsc-span: _tsc.js:74787-74796
    ///
    /// The 2602 arm compares getJsxElementTypeAt against undefined —
    /// getJsxType never returns undefined (errorType stands in), so
    /// the arm is DEAD in 6.0.3 (oracle-verified: no 2602 next to
    /// 17004); transcription keeps only the live 17004 row.
    fn check_jsx_preconditions(&mut self, error_node: NodeId) -> CheckResult2<()> {
        if self.options.jsx.unwrap_or(0) == 0 {
            self.error_at(
                Some(error_node),
                &diagnostics::Cannot_use_JSX_unless_the_jsx_flag_is_provided,
                &[],
            );
        }
        // getJsxElementTypeAt(errorNode) === undefined → 2602: dead.
        Ok(())
    }

    /// tsc-port: markJsxAliasReferenced @6.0.3
    /// tsc-hash: 0eada6ca95ca451acb3957d1c93cab747727a4e7e2681109206f406b65eb9efa
    /// tsc-span: _tsc.js:71787-71827
    ///
    /// The REFERENCE bookkeeping (isReferenced, alias marking) is
    /// M7/alias-band and stays inert; the T0 face is the factory
    /// resolveName probe whose not-found message (2874) fires under
    /// jsx===React. For classic fragments the second factory probe
    /// shares the same first identifier and dedupes when appropriate.
    fn mark_jsx_alias_referenced(&mut self, node: NodeId) -> CheckResult2<()> {
        if self
            .get_jsx_namespace_container_for_implicit_import(node)?
            .is_some()
        {
            return Ok(());
        }
        let jsx_factory_ref_err = (self.options.jsx == Some(2)).then_some(
            &diagnostics::This_JSX_tag_requires_0_to_be_in_scope_but_it_could_not_be_found,
        );
        let jsx_factory_namespace = self.get_jsx_namespace_name(node);
        let jsx_factory_location = match self.data_of(node) {
            NodeData::JsxOpeningElement(data) => data.tag_name.unwrap_or(node),
            NodeData::JsxSelfClosingElement(data) => data.tag_name.unwrap_or(node),
            _ => node,
        };
        // shouldFactoryRefErr: jsx !== Preserve && jsx !== ReactNative.
        let should_factory_ref_err = !matches!(self.options.jsx, Some(1) | Some(3));
        let meaning = if should_factory_ref_err {
            SymbolFlags::VALUE
        } else {
            SymbolFlags::from_bits(SymbolFlags::VALUE.bits() & !SymbolFlags::ENUM.bits())
        };
        let is_fragment = self.kind_of(node) == SyntaxKind::JsxOpeningFragment;
        if !(is_fragment && jsx_factory_namespace == "null") {
            let symbol = self.resolve_name(
                Some(jsx_factory_location),
                &jsx_factory_namespace,
                meaning,
                jsx_factory_ref_err,
                /*is_use*/ true,
                /*exclude_globals*/ false,
            );
            // isReferenced/alias marking: M7/alias bookkeeping, inert.
            let _ = symbol;
        }
        if is_fragment {
            let factory_namespace = self.get_jsx_factory_namespace_name(node);
            self.resolve_name(
                Some(jsx_factory_location),
                &factory_namespace,
                meaning,
                jsx_factory_ref_err,
                /*is_use*/ true,
                /*exclude_globals*/ false,
            )?;
        }
        Ok(())
    }

    /// tsc-port: checkJsxChildren @6.0.3
    /// tsc-hash: 4278af460ef6a9ddc82ec13a9987a5a5680260214fdf98c2be57a7f478751355
    /// tsc-span: _tsc.js:74496-74510
    fn check_jsx_children(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<Vec<TypeId>> {
        let children = match self.data_of(node) {
            NodeData::JsxElement(data) => data.children,
            NodeData::JsxFragment(data) => data.children,
            _ => None,
        };
        let mut children_types = Vec::new();
        for child in self.nodes_of(children) {
            match self.kind_of(child) {
                SyntaxKind::JsxText => {
                    // containsOnlyTriviaWhiteSpaces: inline-only
                    // whitespace is a semantic string child; only
                    // line-break-carrying whitespace is trivia.
                    let is_trivia = match self.data_of(child) {
                        NodeData::JsxText(data) => data.contains_only_trivia_white_spaces,
                        _ => false,
                    };
                    if !is_trivia {
                        children_types.push(self.tables.intrinsics.string);
                    }
                }
                SyntaxKind::JsxExpression
                    if matches!(
                        self.data_of(child),
                        NodeData::JsxExpression(data) if data.expression.is_none()
                    ) => {}
                _ => {
                    children_types.push(
                        self.check_expression_for_mutable_location(child, check_mode, false)?,
                    );
                }
            }
        }
        Ok(children_types)
    }

    // ---- namespace / JSX.* lookups ----

    /// tsc-port: getJsxElementTypeAt @6.0.3
    /// tsc-hash: a1d8bc8f8435cf258dae4bee967ba846210bafdfdd34defc3b7b2eeb8aca6e3f
    /// tsc-span: _tsc.js:74750-74752
    ///
    fn get_jsx_element_type_at(&mut self, location: NodeId) -> CheckResult2<TypeId> {
        self.get_jsx_type(JSX_ELEMENT, location)
    }

    /// tsc-port: getJsxType @6.0.3
    /// tsc-hash: 7c6f27e0e16484dad8149ee8100c711537a059f5cd79e2202a96590e10b11ace
    /// tsc-span: _tsc.js:74525-74530
    ///
    fn get_jsx_type(&mut self, name: &str, location: NodeId) -> CheckResult2<TypeId> {
        let namespace = self.get_jsx_namespace_at(location)?;
        let Some(namespace) = namespace else {
            return Ok(self.tables.intrinsics.error);
        };
        // getSymbol(exports, name, Type): merged-symbol chase + the
        // alias escape (an aliased JSX.* type member would otherwise
        // read as "missing" — review find).
        let Some(symbol) = self.jsx_namespace_export(namespace, name, SymbolFlags::TYPE)? else {
            return Ok(self.tables.intrinsics.error);
        };
        self.get_declared_type_of_symbol_slice(symbol)
    }

    /// getSymbol(getExportsOfSymbol(namespace), name, meaning) (50791)
    /// over the JSX namespace: the shared getExportsOfSymbol worker
    /// (late-bound statics + the export-star walk) feeding the
    /// faithful getSymbol lookup (alias arm included, M4 5.9d).
    fn jsx_namespace_export(
        &mut self,
        namespace: SymbolId,
        name: &str,
        meaning: SymbolFlags,
    ) -> CheckResult2<Option<SymbolId>> {
        let exports = self.get_exports_of_symbol(namespace)?;
        self.get_symbol_in_table(&exports, name, meaning)
    }

    // ---- 5.7c: intrinsic tag resolution ----

    /// tsc-port: isJsxIntrinsicTagName @6.0.3
    /// tsc-hash: 12579b4f2aea0333c0ef3472df620269b6209d96f0b9344fb1a4c0603f4a52c8
    /// tsc-span: _tsc.js:74340-74342
    pub(crate) fn is_jsx_intrinsic_tag_name(&self, tag_name: NodeId) -> bool {
        match self.data_of(tag_name) {
            NodeData::Identifier(data) => is_intrinsic_jsx_name(&data.escaped_text),
            NodeData::JsxNamespacedName(_) => true,
            _ => false,
        }
    }

    /// The escaped property name an intrinsic tag looks up
    /// (tagName.escapedText ‖ getEscapedTextOfJsxNamespacedName).
    fn intrinsic_tag_property_name(&self, tag_name: NodeId) -> CheckResult2<String> {
        match self.data_of(tag_name) {
            NodeData::Identifier(data) => Ok(data.escaped_text.clone()),
            NodeData::JsxNamespacedName(_) => Ok(self.jsx_attribute_name_text(tag_name)),
            _ => Err(Unsupported::new(
                "intrinsic tag with a non-identifier name (parse recovery)",
            )),
        }
    }

    /// tsc-port: intrinsicTagNameToString @6.0.3
    /// tsc-hash: 2664b54c05cf94a2a019c82065356c1348fec112c02379f734d79d8b7a488aa4
    /// tsc-span: _tsc.js:19348-19350
    ///
    /// idText flavors: the DISPLAY form (unescaped).
    fn intrinsic_tag_name_to_string(&self, tag_name: NodeId) -> CheckResult2<String> {
        let escaped = self.intrinsic_tag_property_name(tag_name)?;
        Ok(tsrs2_binder::unescape_leading_underscores(&escaped).to_owned())
    }

    /// tsc-port: getIntrinsicTagSymbol @6.0.3
    /// tsc-hash: b9cbc750759c56623f8eedde30346c88f2d5482af2d9800ac332796eafc4253f
    /// tsc-span: _tsc.js:74531-74562
    ///
    /// The getApplicableIndexSymbol arm (74543-74547) and the
    /// propName-typed re-probe (74548-74551) merge here: both answer
    /// IntrinsicIndexedElement, and the memoized symbol identity
    /// (a filtered `__index` copy vs the container symbol) is
    /// services-only — T0 reads jsxFlags and the error rows.
    pub(crate) fn get_intrinsic_tag_symbol(&mut self, node: NodeId) -> CheckResult2<SymbolId> {
        if let Some(cached) = self.links.node(node).resolved_symbol.resolved() {
            return Ok(cached);
        }
        let tag_name = match self.data_of(node) {
            NodeData::JsxOpeningElement(data) => data.tag_name,
            NodeData::JsxSelfClosingElement(data) => data.tag_name,
            NodeData::JsxClosingElement(data) => data.tag_name,
            _ => None,
        }
        .ok_or_else(|| Unsupported::new("JSX element without a tag name (parse recovery)"))?;
        let intrinsic_elements_type = self.get_jsx_type(JSX_INTRINSIC_ELEMENTS, node)?;
        let symbol;
        if !self.tables.is_error_type(intrinsic_elements_type) {
            let prop_name = self.intrinsic_tag_property_name(tag_name)?;
            let intrinsic_prop =
                self.get_property_of_type_full(intrinsic_elements_type, &prop_name)?;
            if let Some(intrinsic_prop) = intrinsic_prop {
                self.links.add_node_jsx_flags(
                    self.speculation_depth,
                    node,
                    JsxFlags::INTRINSIC_NAMED_ELEMENT,
                );
                symbol = intrinsic_prop;
            } else if self
                .get_applicable_index_info_for_name(intrinsic_elements_type, &prop_name)?
                .is_some()
            {
                self.links.add_node_jsx_flags(
                    self.speculation_depth,
                    node,
                    JsxFlags::INTRINSIC_INDEXED_ELEMENT,
                );
                let members = self.resolve_structured_type_members(intrinsic_elements_type)?;
                let index_symbol = self
                    .members_of(members)
                    .members
                    .get(tsrs2_binder::InternalSymbolName::INDEX)
                    .copied();
                // tsc stores `intrinsicElementsType.symbol` here, which
                // an alias-declared IntrinsicElements leaves undefined:
                // the memo then never fills and no T0 path reads the
                // identity (flags are already published; the INDEXED
                // attribute type reads the index info, not the symbol).
                // unknownSymbol is the identity-free stand-in.
                symbol = match index_symbol {
                    Some(index_symbol) => index_symbol,
                    None => self
                        .tables
                        .type_of(intrinsic_elements_type)
                        .symbol
                        .unwrap_or(self.unknown_symbol),
                };
            } else {
                let display = self.intrinsic_tag_name_to_string(tag_name)?;
                let container = format!("JSX.{JSX_INTRINSIC_ELEMENTS}");
                self.error_at(
                    Some(node),
                    &diagnostics::Property_0_does_not_exist_on_type_1,
                    &[&display, &container],
                );
                symbol = self.unknown_symbol;
            }
        } else {
            // (5.8d) `declare global` JSX.IntrinsicElements surfaces
            // now MERGE (merge_module_augmentations pass 1) — the old
            // undecidability containment retired with the resolver
            // failure-band gate.
            if self
                .options
                .strict_option_value(self.options.no_implicit_any)
            {
                self.error_at(
                    Some(node),
                    &diagnostics::JSX_element_implicitly_has_type_any_because_no_interface_JSX_0_exists,
                    &[JSX_INTRINSIC_ELEMENTS],
                );
            }
            symbol = self.unknown_symbol;
        }
        self.links
            .set_node_resolved_symbol(self.speculation_depth, node, symbol);
        Ok(symbol)
    }

    /// tsc-port: getIntrinsicAttributesTypeFromJsxOpeningLikeElement @6.0.3
    /// tsc-hash: 677b24cfa7578ef6b8b502ea20f8478b4b09153eea27dbcc8cee8260eaacd929
    /// tsc-span: _tsc.js:74728-74744
    pub(crate) fn get_intrinsic_attributes_type_from_jsx_opening_like_element(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_jsx_element_attributes_type {
            return Ok(cached);
        }
        let symbol = self.get_intrinsic_tag_symbol(node)?;
        let jsx_flags = self.links.node(node).jsx_flags;
        let result = if jsx_flags.intersects(JsxFlags::INTRINSIC_NAMED_ELEMENT) {
            self.get_type_of_symbol(symbol)?
        } else if jsx_flags.intersects(JsxFlags::INTRINSIC_INDEXED_ELEMENT) {
            let tag_name = match self.data_of(node) {
                NodeData::JsxOpeningElement(data) => data.tag_name,
                NodeData::JsxSelfClosingElement(data) => data.tag_name,
                _ => None,
            }
            .ok_or_else(|| {
                Unsupported::new("JSX opening element without a tag name (parse recovery)")
            })?;
            let prop_name = self.intrinsic_tag_property_name(tag_name)?;
            let intrinsic_elements_type = self.get_jsx_type(JSX_INTRINSIC_ELEMENTS, node)?;
            self.get_applicable_index_info_for_name(intrinsic_elements_type, &prop_name)?
                .unwrap_or(self.tables.intrinsics.error)
        } else {
            self.tables.intrinsics.error
        };
        self.links.set_node_resolved_jsx_element_attributes_type(
            self.speculation_depth,
            node,
            result,
        );
        Ok(result)
    }

    /// tsc-port: getIntrinsicAttributesTypeFromStringLiteralType @6.0.3
    /// tsc-hash: 233443405f32b17a7d94d92f8328c3312488cd4c184541eebaea9c8410cafa1b
    /// tsc-span: _tsc.js:74682-74697
    fn get_intrinsic_attributes_type_from_string_literal_type(
        &mut self,
        ty: TypeId,
        location: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let intrinsic_elements_type = self.get_jsx_type(JSX_INTRINSIC_ELEMENTS, location)?;
        if self.tables.is_error_type(intrinsic_elements_type) {
            return Ok(Some(self.tables.intrinsics.any));
        }
        let value = self.string_literal_type_value(ty)?;
        let escaped = tsrs2_binder::escape_leading_underscores(&value);
        if let Some(prop) =
            self.get_property_of_type_full(intrinsic_elements_type, escaped.as_ref())?
        {
            return Ok(Some(self.get_type_of_symbol(prop)?));
        }
        let string = self.tables.intrinsics.string;
        if let Some(info) = self.get_index_info_of_type(intrinsic_elements_type, string)? {
            return Ok(Some(info.value_type));
        }
        Ok(None)
    }

    /// The StringLiteral payload read.
    fn string_literal_type_value(&self, ty: TypeId) -> CheckResult2<String> {
        match &self.tables.type_of(ty).data {
            TypeData::Literal {
                value: tsrs2_types::LiteralValue::String(value),
            } => Ok(value.clone()),
            _ => unreachable!("STRING_LITERAL types always carry Literal string data"),
        }
    }

    // ---- 5.7c: signatures for JSX resolution ----

    /// tsc-port: createSignatureForJSXIntrinsic @6.0.3
    /// tsc-hash: f98bc97eeb008dd1fe5c95999ea48780a5263535f6bfd184069bb8b92de9fd33
    /// tsc-span: _tsc.js:77332-77371
    ///
    /// DECLARATION-LESS: tsc fabricates a FunctionTypeNode purely for
    /// signature DISPLAY (nodeBuilder) — T2 side; the T0 signature is
    /// one `props` parameter typed by the intrinsic attributes type,
    /// returning the JSX.Element declared type ‖ errorType, minArg 1.
    pub(crate) fn create_signature_for_jsx_intrinsic(
        &mut self,
        node: NodeId,
        result: TypeId,
    ) -> CheckResult2<SignatureId> {
        let namespace = self.get_jsx_namespace_at(node)?;
        let type_symbol = match namespace {
            Some(namespace) => {
                self.jsx_namespace_export(namespace, JSX_ELEMENT, SymbolFlags::TYPE)?
            }
            None => None,
        };
        let return_type = match type_symbol {
            Some(type_symbol) => self.get_declared_type_of_symbol_slice(type_symbol)?,
            None => self.tables.intrinsics.error,
        };
        let props = self
            .binder
            .create_symbol(SymbolFlags::FUNCTION_SCOPED_VARIABLE, "props".to_owned());
        self.links
            .set_fresh_symbol_type(props, LinkSlot::Resolved(result));
        Ok(self.alloc_signature(crate::state::Signature {
            declaration: None,
            flags: tsrs2_types::SignatureFlags::NONE,
            type_parameters: None,
            parameters: vec![props],
            this_parameter: None,
            min_argument_count: 1,
            resolved_return_type: LinkSlot::Resolved(return_type),
            from_method: false,
            target: None,
            mapper: None,
            instantiations: std::collections::HashMap::new(),
            erased_signature_cache: None,
            canonical_signature_cache: None,
            base_signature_cache: None,
            composite_kind: None,
            composite_signatures: None,
            optional_call_signature_cache: (None, None),
            isolated_signature_kind: Some(SignatureKind::Construct),
            isolated_signature_type: None,
        }))
    }

    /// tsc-port: getOrCreateTypeFromSignature @6.0.3
    /// tsc-hash: 226def909910a74af356148d136d42297aba460f51ea90cf8ffe0c769bf08ec9
    /// tsc-span: _tsc.js:59968-59982
    ///
    /// An undefined declaration kind counts as a CONSTRUCTOR (59972).
    /// Declaration-elided synthetic signatures carry that kind
    /// explicitly: JSX fakes land in constructSignatures, while tsc's
    /// fabricated FunctionTypeNode call signatures remain CALL (both
    /// carry no symbol: `signature.declaration?.symbol` is undefined).
    pub(crate) fn get_or_create_type_from_signature(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.signature_of(signature).isolated_signature_type {
            return Ok(cached);
        }
        let is_constructor = match self.signature_of(signature).isolated_signature_kind {
            Some(SignatureKind::Call) => false,
            Some(SignatureKind::Construct) => true,
            None => match self.signature_of(signature).declaration {
                None => true,
                Some(declaration) => matches!(
                    self.kind_of(declaration),
                    SyntaxKind::Constructor
                        | SyntaxKind::ConstructSignature
                        | SyntaxKind::ConstructorType
                ),
            },
        };
        let id = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
        self.tables.type_mut(id).object_flags =
            ObjectFlags::ANONYMOUS | ObjectFlags::SINGLE_SIGNATURE_TYPE;
        let members = self.alloc_members(crate::state::ResolvedMembers {
            members: SymbolTable::default(),
            properties: Vec::new(),
            call_signatures: if is_constructor {
                Vec::new()
            } else {
                vec![signature]
            },
            construct_signatures: if is_constructor {
                vec![signature]
            } else {
                Vec::new()
            },
            index_infos: Vec::new(),
        });
        self.links
            .set_fresh_type_members(id, LinkSlot::Resolved(members));
        // The anonymous type and its members are one semantic object,
        // so construction is safe inside a candidate trial. The
        // signature memo is a cold permanent cache, however: a failed
        // candidate must not publish its trial-local TypeId.
        if self.speculation_depth == 0 {
            self.signature_mut(signature).isolated_signature_type = Some(id);
        }
        Ok(id)
    }

    /// tsc-port: getUninstantiatedJsxSignaturesOfType @6.0.3
    /// tsc-hash: 3faa3a5b89bb9582c3d52fd7fc87289777fae435a5796fa918dc9f1800ad7909
    /// tsc-span: _tsc.js:74659-74681
    pub(crate) fn get_uninstantiated_jsx_signatures_of_type(
        &mut self,
        element_type: TypeId,
        caller: NodeId,
    ) -> CheckResult2<Vec<SignatureId>> {
        let flags = self.tables.flags_of(element_type);
        if flags.intersects(TypeFlags::STRING) && !flags.intersects(TypeFlags::STRING_LITERAL) {
            return Ok(vec![self.any_signature]);
        }
        if flags.intersects(TypeFlags::STRING_LITERAL) {
            let intrinsic_type =
                self.get_intrinsic_attributes_type_from_string_literal_type(element_type, caller)?;
            let Some(intrinsic_type) = intrinsic_type else {
                let value = self.string_literal_type_value(element_type)?;
                let container = format!("JSX.{JSX_INTRINSIC_ELEMENTS}");
                self.error_at(
                    Some(caller),
                    &diagnostics::Property_0_does_not_exist_on_type_1,
                    &[&value, &container],
                );
                return Ok(Vec::new());
            };
            let fake_signature = self.create_signature_for_jsx_intrinsic(caller, intrinsic_type)?;
            return Ok(vec![fake_signature]);
        }
        let apparent_elem_type = self.get_apparent_type(element_type)?;
        let mut signatures =
            self.get_signatures_of_type(apparent_elem_type, SignatureKind::Construct)?;
        if signatures.is_empty() {
            signatures = self.get_signatures_of_type(apparent_elem_type, SignatureKind::Call)?;
        }
        if signatures.is_empty()
            && self
                .tables
                .flags_of(apparent_elem_type)
                .intersects(TypeFlags::UNION)
        {
            let constituents: Vec<TypeId> = match &self.tables.type_of(apparent_elem_type).data {
                TypeData::Union { types, .. } => types.to_vec(),
                _ => Vec::new(),
            };
            let mut lists: Vec<Vec<SignatureId>> = Vec::with_capacity(constituents.len());
            for constituent in constituents {
                lists.push(self.get_uninstantiated_jsx_signatures_of_type(constituent, caller)?);
            }
            signatures = self.get_union_signatures(&lists)?;
        }
        Ok(signatures)
    }

    /// tsc-port: getJSXFragmentType @6.0.3
    /// tsc-hash: 7a885aaa15a865e701ecab621cb35e8cc1e9f33b5962ab7bae27d992ee703452
    /// tsc-span: _tsc.js:77372-77396
    ///
    pub(crate) fn get_jsx_fragment_type(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let root = self.binder.source_of_node(node).root;
        if let Some(cached) = self.links.node(root).jsx_fragment_type {
            return Ok(cached);
        }
        let fragment_factory_name = self.get_jsx_namespace_name(node);
        let should_resolve_factory_reference = (self.options.jsx == Some(2)
            || self.options.jsx_fragment_factory.is_some())
            && fragment_factory_name != "null";
        if !should_resolve_factory_reference {
            let any = self.tables.intrinsics.any;
            self.links
                .set_node_jsx_fragment_type(self.speculation_depth, root, any);
            return Ok(any);
        }
        let implicit = self.get_jsx_namespace_container_for_implicit_import(node)?;
        // getJsxNamespaceContainerForImplicitImport: None (the guard
        // escaped the react-jsx flavors). shouldModuleRefErr is true at
        // jsx===React, so the meaning is plain VALUE.
        let factory_symbol = match implicit {
            Some(symbol) => Some(symbol),
            None => self.resolve_name(
                Some(node),
                &fragment_factory_name,
                SymbolFlags::VALUE,
                Some(&diagnostics::Using_JSX_fragments_requires_fragment_factory_0_to_be_in_scope_but_it_could_not_be_found),
                /*is_use*/ true,
                /*exclude_globals*/ false,
            )?,
        };
        let Some(factory_symbol) = factory_symbol else {
            let error = self.tables.intrinsics.error;
            self.links
                .set_node_jsx_fragment_type(self.speculation_depth, root, error);
            return Ok(error);
        };
        if self.binder.symbol(factory_symbol).escaped_name == REACT_FRAGMENT {
            let ty = self.get_type_of_symbol(factory_symbol)?;
            self.links
                .set_node_jsx_fragment_type(self.speculation_depth, root, ty);
            return Ok(ty);
        }
        let resolved_alias = if self
            .symbol_flags(factory_symbol)
            .intersects(SymbolFlags::ALIAS)
        {
            self.resolve_alias(factory_symbol)?
        } else {
            factory_symbol
        };
        let exports = self.get_exports_of_symbol(resolved_alias)?;
        let type_symbol =
            self.get_symbol_in_table(&exports, REACT_FRAGMENT, SymbolFlags::BLOCK_SCOPED_VARIABLE)?;
        let ty = match type_symbol {
            Some(type_symbol) => self.get_type_of_symbol(type_symbol)?,
            None => self.tables.intrinsics.error,
        };
        self.links
            .set_node_jsx_fragment_type(self.speculation_depth, root, ty);
        Ok(ty)
    }

    // ---- 5.7c: reference kind + effective first argument ----

    /// tsc-port: getJsxReferenceKind @6.0.3
    /// tsc-hash: 2c7cc3306ba04d73d4dc86ab318c011deda247f0aacaed1ff0499e02415d4c06
    /// tsc-span: _tsc.js:76075-76087
    pub(crate) fn get_jsx_reference_kind(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<JsxReferenceKind> {
        let tag_name = match self.data_of(node) {
            NodeData::JsxOpeningElement(data) => data.tag_name,
            NodeData::JsxSelfClosingElement(data) => data.tag_name,
            _ => None,
        }
        .ok_or_else(|| {
            Unsupported::new("JSX opening element without a tag name (parse recovery)")
        })?;
        if self.is_jsx_intrinsic_tag_name(tag_name) {
            return Ok(JsxReferenceKind::Mixed);
        }
        let checked = self.check_expression(tag_name, CheckMode::NORMAL)?;
        let tag_type = self.get_apparent_type(checked)?;
        if !self
            .get_signatures_of_type(tag_type, SignatureKind::Construct)?
            .is_empty()
        {
            return Ok(JsxReferenceKind::Component);
        }
        if !self
            .get_signatures_of_type(tag_type, SignatureKind::Call)?
            .is_empty()
        {
            return Ok(JsxReferenceKind::Function);
        }
        Ok(JsxReferenceKind::Mixed)
    }

    /// tsc-port: getEffectiveFirstArgumentForJsxSignature @6.0.3
    /// tsc-hash: 625ad2f2732a3c7c2ed3a22ae6e0ec29b410073c53d989874648717245c98573
    /// tsc-span: _tsc.js:73648-73650
    pub(crate) fn get_effective_first_argument_for_jsx_signature(
        &mut self,
        signature: SignatureId,
        node: NodeId,
    ) -> CheckResult2<TypeId> {
        let is_fragment = self.kind_of(node) == SyntaxKind::JsxOpeningFragment;
        if is_fragment || self.get_jsx_reference_kind(node)? != JsxReferenceKind::Component {
            self.get_jsx_props_type_from_call_signature(signature, node)
        } else {
            self.get_jsx_props_type_from_class_type(signature, node)
        }
    }

    /// tsc-port: getJsxPropsTypeFromCallSignature @6.0.3
    /// tsc-hash: 33e5c609300535f18be8be847ad477103464f14b66e441fef6b3381a0eac35d8
    /// tsc-span: _tsc.js:73651-73659
    fn get_jsx_props_type_from_call_signature(
        &mut self,
        signature: SignatureId,
        context: NodeId,
    ) -> CheckResult2<TypeId> {
        // getTypeOfFirstParameterOfSignatureWithFallback(sig, unknown).
        let mut props_type = if self.signature_of(signature).parameters.is_empty() {
            self.tables.intrinsics.unknown
        } else {
            self.get_type_at_position(signature, 0)?
        };
        let namespace = self.get_jsx_namespace_at(context)?;
        props_type = self
            .get_jsx_managed_attributes_from_located_attributes(context, namespace, props_type)?;
        let intrinsic_attribs = self.get_jsx_type(JSX_INTRINSIC_ATTRIBUTES, context)?;
        if !self.tables.is_error_type(intrinsic_attribs) {
            props_type = self
                .get_intersection_type(&[intrinsic_attribs, props_type], IntersectionFlags::NONE)?;
        }
        Ok(props_type)
    }

    /// tsc-port: getJsxPropsTypeForSignatureFromMember @6.0.3
    /// tsc-hash: a52282550d0eb07807f3c0eed685e822c1db097a5cbe7cbdf8928b26fadf168e
    /// tsc-span: _tsc.js:73660-73678
    fn get_jsx_props_type_for_signature_from_member(
        &mut self,
        signature: SignatureId,
        forced_lookup_location: &str,
    ) -> CheckResult2<Option<TypeId>> {
        if let Some(composites) = self.signature_of(signature).composite_signatures.clone() {
            let mut results: Vec<TypeId> = Vec::with_capacity(composites.len());
            for composite in composites {
                let instance = self.get_return_type_of_signature(composite)?;
                if self.tables.flags_of(instance).intersects(TypeFlags::ANY) {
                    return Ok(Some(instance));
                }
                let Some(prop_type) =
                    self.get_type_of_property_of_type(instance, forced_lookup_location)?
                else {
                    return Ok(None);
                };
                results.push(prop_type);
            }
            return Ok(Some(
                self.get_intersection_type(&results, IntersectionFlags::NONE)?,
            ));
        }
        let instance_type = self.get_return_type_of_signature(signature)?;
        if self
            .tables
            .flags_of(instance_type)
            .intersects(TypeFlags::ANY)
        {
            return Ok(Some(instance_type));
        }
        self.get_type_of_property_of_type(instance_type, forced_lookup_location)
    }

    /// tsc-port: getStaticTypeOfReferencedJsxConstructor @6.0.3
    /// tsc-hash: c473cec1cbbe45817fcdeb6cfd76ed4f4ce0fd98927235f5724a2dde0eff8823
    /// tsc-span: _tsc.js:73679-73696
    fn get_static_type_of_referenced_jsx_constructor(
        &mut self,
        context: NodeId,
    ) -> CheckResult2<TypeId> {
        if self.kind_of(context) == SyntaxKind::JsxOpeningFragment {
            return self.get_jsx_fragment_type(context);
        }
        let tag_name = match self.data_of(context) {
            NodeData::JsxOpeningElement(data) => data.tag_name,
            NodeData::JsxSelfClosingElement(data) => data.tag_name,
            _ => None,
        }
        .ok_or_else(|| {
            Unsupported::new("JSX opening element without a tag name (parse recovery)")
        })?;
        if self.is_jsx_intrinsic_tag_name(tag_name) {
            let result =
                self.get_intrinsic_attributes_type_from_jsx_opening_like_element(context)?;
            let fake_signature = self.create_signature_for_jsx_intrinsic(context, result)?;
            return self.get_or_create_type_from_signature(fake_signature);
        }
        let tag_type = self.check_expression_cached(tag_name, CheckMode::NORMAL)?;
        if self
            .tables
            .flags_of(tag_type)
            .intersects(TypeFlags::STRING_LITERAL)
        {
            let Some(result) =
                self.get_intrinsic_attributes_type_from_string_literal_type(tag_type, context)?
            else {
                return Ok(self.tables.intrinsics.error);
            };
            let fake_signature = self.create_signature_for_jsx_intrinsic(context, result)?;
            return self.get_or_create_type_from_signature(fake_signature);
        }
        Ok(tag_type)
    }

    /// tsc-port: getJsxManagedAttributesFromLocatedAttributes @6.0.3
    /// tsc-hash: eecb5b27501de81d5e4bc7c7fc14ab0b9f5e528c09b233bea9b20f2332030113
    /// tsc-span: _tsc.js:73697-73707
    fn get_jsx_managed_attributes_from_located_attributes(
        &mut self,
        context: NodeId,
        namespace: Option<SymbolId>,
        attributes_type: TypeId,
    ) -> CheckResult2<TypeId> {
        let managed_sym = match namespace {
            Some(namespace) => self.jsx_namespace_export(
                namespace,
                JSX_LIBRARY_MANAGED_ATTRIBUTES,
                SymbolFlags::TYPE,
            )?,
            None => None,
        };
        if let Some(managed_sym) = managed_sym {
            let ctor_type = self.get_static_type_of_referenced_jsx_constructor(context)?;
            if let Some(result) = self.instantiate_alias_or_interface_with_defaults(
                managed_sym,
                self.is_in_js_file(context),
                &[ctor_type, attributes_type],
            )? {
                return Ok(result);
            }
        }
        Ok(attributes_type)
    }

    /// tsc-port: getJsxPropsTypeFromClassType @6.0.3
    /// tsc-hash: 182e71e720a8189ea641031ab9f51dee87cee594b1677967b9df3665dffcea81
    /// tsc-span: _tsc.js:73708-73740
    fn get_jsx_props_type_from_class_type(
        &mut self,
        signature: SignatureId,
        context: NodeId,
    ) -> CheckResult2<TypeId> {
        let namespace = self.get_jsx_namespace_at(context)?;
        let forced_lookup_location = self.get_jsx_element_properties_name(namespace)?;
        let attributes_type: Option<TypeId> = match &forced_lookup_location {
            None => Some(if self.signature_of(signature).parameters.is_empty() {
                self.tables.intrinsics.unknown
            } else {
                self.get_type_at_position(signature, 0)?
            }),
            Some(location) if location.is_empty() => {
                Some(self.get_return_type_of_signature(signature)?)
            }
            Some(location) => {
                let location = location.clone();
                self.get_jsx_props_type_for_signature_from_member(signature, &location)?
            }
        };
        let Some(mut attributes_type) = attributes_type else {
            if let Some(location) = forced_lookup_location.as_ref().filter(|l| !l.is_empty()) {
                let has_properties = match self.data_of(context) {
                    NodeData::JsxOpeningElement(data) => data.attributes,
                    NodeData::JsxSelfClosingElement(data) => data.attributes,
                    _ => None,
                }
                .is_some_and(|attributes| match self.data_of(attributes) {
                    NodeData::JsxAttributes(data) => !self.nodes_of(data.properties).is_empty(),
                    _ => false,
                });
                if has_properties {
                    let display = tsrs2_binder::unescape_leading_underscores(location);
                    self.error_at(
                        Some(context),
                        &diagnostics::JSX_element_class_does_not_support_attributes_because_it_does_not_have_a_0_property,
                        &[display],
                    );
                }
            }
            return Ok(self.tables.intrinsics.unknown);
        };
        attributes_type = self.get_jsx_managed_attributes_from_located_attributes(
            context,
            namespace,
            attributes_type,
        )?;
        if self
            .tables
            .flags_of(attributes_type)
            .intersects(TypeFlags::ANY)
        {
            return Ok(attributes_type);
        }
        let mut apparent_attributes_type = attributes_type;
        let intrinsic_class_attribs = self.get_jsx_type(JSX_INTRINSIC_CLASS_ATTRIBUTES, context)?;
        if !self.tables.is_error_type(intrinsic_class_attribs) {
            let class_symbol = self.tables.type_of(intrinsic_class_attribs).symbol;
            let type_params = match class_symbol {
                Some(symbol) => {
                    let params =
                        self.get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
                    (!params.is_empty()).then_some(params)
                }
                None => None,
            };
            let host_class_type = self.get_return_type_of_signature(signature)?;
            let library_managed_attribute_type = if let Some(type_params) = type_params {
                let min = self.get_min_type_argument_count(Some(&type_params));
                let inferred_args = self
                    .fill_missing_type_arguments(
                        Some(&[host_class_type]),
                        Some(&type_params),
                        min,
                        self.is_in_js_file(context),
                    )?
                    .expect("Some input yields Some");
                let mapper = self.create_type_mapper(type_params, Some(inferred_args));
                self.instantiate_type(intrinsic_class_attribs, Some(mapper))?
            } else {
                intrinsic_class_attribs
            };
            apparent_attributes_type = self.get_intersection_type(
                &[library_managed_attribute_type, apparent_attributes_type],
                IntersectionFlags::NONE,
            )?;
        }
        let intrinsic_attribs = self.get_jsx_type(JSX_INTRINSIC_ATTRIBUTES, context)?;
        if !self.tables.is_error_type(intrinsic_attribs) {
            apparent_attributes_type = self.get_intersection_type(
                &[intrinsic_attribs, apparent_attributes_type],
                IntersectionFlags::NONE,
            )?;
        }
        Ok(apparent_attributes_type)
    }

    /// tsc-port: instantiateAliasOrInterfaceWithDefaults @6.0.3
    /// tsc-hash: 6671afcf7448a7c82e3862d888997dcf5f4acdddf9a83c3930a97d42814086d7
    /// tsc-span: _tsc.js:74768-74782
    fn instantiate_alias_or_interface_with_defaults(
        &mut self,
        managed_sym: SymbolId,
        in_js: bool,
        type_arguments: &[TypeId],
    ) -> CheckResult2<Option<TypeId>> {
        let declared_managed_type = self.get_declared_type_of_symbol_slice(managed_sym)?;
        if self
            .symbol_flags(managed_sym)
            .intersects(SymbolFlags::TYPE_ALIAS)
        {
            let params = self.links.symbol(managed_sym).type_parameters.clone();
            if params.as_ref().map_or(0, Vec::len) >= type_arguments.len() {
                let args = self
                    .fill_missing_type_arguments(
                        Some(type_arguments),
                        params.as_deref(),
                        type_arguments.len(),
                        in_js,
                    )?
                    .unwrap_or_default();
                return Ok(Some(if args.is_empty() {
                    declared_managed_type
                } else {
                    self.get_type_alias_instantiation(managed_sym, Some(&args), None, None)?
                }));
            }
        }
        let declared_params = self.interface_type_parameters(declared_managed_type);
        if declared_params.as_ref().map_or(0, Vec::len) >= type_arguments.len() {
            if declared_params.is_none() {
                // tsc's createTypeReference over a non-generic declared
                // type reads an undefined `instantiations` map and
                // throws TypeError — a reachable tsc crash (thisless
                // non-generic interface as JSX.ElementType), so no
                // golden can exist; permanent guard, crash-guard
                // family (m4-end-sweep-steps.md).
                return Err(Unsupported::new(
                    "createTypeReference over a non-generic interface \
                     (tsc TypeError crash guard, parse-recovery-class permanent containment)",
                ));
            }
            let args = self
                .fill_missing_type_arguments(
                    Some(type_arguments),
                    declared_params.as_deref(),
                    type_arguments.len(),
                    in_js,
                )?
                .unwrap_or_default();
            return Ok(Some(
                self.tables
                    .create_type_reference(declared_managed_type, &args),
            ));
        }
        Ok(None)
    }

    /// InterfaceType.typeParameters (outer+local): the GenericType
    /// payload; every other declared-type shape reads tsc's undefined
    /// (`length(undefined) = 0` in the caller's arity compare).
    fn interface_type_parameters(&self, ty: TypeId) -> Option<Vec<TypeId>> {
        match &self.tables.type_of(ty).data {
            TypeData::GenericType {
                type_parameters, ..
            } => Some(type_parameters.to_vec()),
            _ => None,
        }
    }

    // ---- 5.7c: JSX.* container names ----

    /// tsc-port: getNameFromJsxElementAttributesContainer @6.0.3
    /// tsc-hash: bbc6bda45fdd402c4219619a669313f010d5116d72b0a07ec882689f542f006e
    /// tsc-span: _tsc.js:74629-74643
    fn get_name_from_jsx_element_attributes_container(
        &mut self,
        name_of_attrib_prop_container: &str,
        jsx_namespace: Option<SymbolId>,
    ) -> CheckResult2<Option<String>> {
        let Some(jsx_namespace) = jsx_namespace else {
            return Ok(None);
        };
        let container_sym = self.jsx_namespace_export(
            jsx_namespace,
            name_of_attrib_prop_container,
            SymbolFlags::TYPE,
        )?;
        let Some(container_sym) = container_sym else {
            return Ok(None);
        };
        let container_type = self.get_declared_type_of_symbol_slice(container_sym)?;
        let properties = self.get_properties_of_type(container_type)?;
        if properties.is_empty() {
            return Ok(Some(String::new()));
        }
        if properties.len() == 1 {
            return Ok(Some(self.binder.symbol(properties[0]).escaped_name.clone()));
        }
        let first_declaration = self
            .binder
            .symbol(container_sym)
            .declarations
            .first()
            .copied();
        if let Some(declaration) = first_declaration {
            let display = tsrs2_binder::unescape_leading_underscores(name_of_attrib_prop_container);
            self.error_at(
                Some(declaration),
                &diagnostics::The_global_type_JSX_0_may_not_have_more_than_one_property,
                &[display],
            );
        }
        Ok(None)
    }

    /// tsc-port: getJsxElementPropertiesName @6.0.3
    /// tsc-hash: 55d3c72e591c4eb119250d751ad968064e49c90244fcf9c9b424995e594df6cd
    /// tsc-span: _tsc.js:74650-74652
    fn get_jsx_element_properties_name(
        &mut self,
        jsx_namespace: Option<SymbolId>,
    ) -> CheckResult2<Option<String>> {
        self.get_name_from_jsx_element_attributes_container(
            JSX_ELEMENT_ATTRIBUTES_PROPERTY_NAME_CONTAINER,
            jsx_namespace,
        )
    }

    /// tsc-port: getJsxElementChildrenPropertyName @6.0.3
    /// tsc-hash: 81212cb84e0b2a445d123c8a4b26aaa5cacebd72ab98c37cbf504245212a9d40
    /// tsc-span: _tsc.js:74653-74658
    ///
    /// The react-jsx/react-jsxdev arm is dead behind the namespace
    /// entity guard (jsx 4/5 escape) — transcribed anyway.
    pub(crate) fn get_jsx_element_children_property_name(
        &mut self,
        jsx_namespace: Option<SymbolId>,
    ) -> CheckResult2<Option<String>> {
        if matches!(self.options.jsx, Some(4) | Some(5)) {
            return Ok(Some("children".to_owned()));
        }
        self.get_name_from_jsx_element_attributes_container(
            JSX_ELEMENT_CHILDREN_ATTRIBUTE_NAME_CONTAINER,
            jsx_namespace,
        )
    }

    /// tsc-port: getJsxElementClassTypeAt @6.0.3
    /// tsc-hash: 31d63d85b4f693d040d29b074963ce0ef9ae73d19806e78b1d280057e1836fa3
    /// tsc-span: _tsc.js:74745-74749
    fn get_jsx_element_class_type_at(&mut self, location: NodeId) -> CheckResult2<Option<TypeId>> {
        let ty = self.get_jsx_type(JSX_ELEMENT_CLASS, location)?;
        if self.tables.is_error_type(ty) {
            return Ok(None);
        }
        Ok(Some(ty))
    }

    /// tsc-port: getJsxStatelessElementTypeAt @6.0.3
    /// tsc-hash: c2de6c99befedb5ba48ecdbeacb67038bef469c76e05d90dd33f8ac616aff58a
    /// tsc-span: _tsc.js:74753-74758
    ///
    /// getJsxElementTypeAt answers errorType when JSX.Element is
    /// missing — truthy in tsc, so the union with nullType still
    /// forms (relations against it succeed through the error member).
    fn get_jsx_stateless_element_type_at(
        &mut self,
        location: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let jsx_element_type = self.get_jsx_element_type_at(location)?;
        let null = self.tables.intrinsics.null;
        Ok(Some(self.get_union_type_ex(
            &[jsx_element_type, null],
            tsrs2_types::UnionReduction::Literal,
        )?))
    }

    /// tsc-port: getJsxElementTypeTypeAt @6.0.3
    /// tsc-hash: 030d14a4dfacaa6c26e2e15f7df9dddcf85ea21f6d086780a054e57e8a25f96c
    /// tsc-span: _tsc.js:74759-74767
    fn get_jsx_element_type_type_at(&mut self, location: NodeId) -> CheckResult2<Option<TypeId>> {
        let Some(namespace) = self.get_jsx_namespace_at(location)? else {
            return Ok(None);
        };
        let Some(sym) =
            self.jsx_namespace_export(namespace, JSX_ELEMENT_TYPE, SymbolFlags::TYPE)?
        else {
            return Ok(None);
        };
        let in_js = self.is_in_js_file(location);
        let Some(ty) = self.instantiate_alias_or_interface_with_defaults(sym, in_js, &[])? else {
            return Ok(None);
        };
        if self.tables.is_error_type(ty) {
            return Ok(None);
        }
        Ok(Some(ty))
    }

    /// tsc-port: getJsxNamespaceAt @6.0.3
    /// tsc-hash: 56cd592146ecb83505d526562a9ebe446de69ccd680f8f2418766b0fc36d9cba
    /// tsc-span: _tsc.js:74586-74628
    ///
    /// The links.jsxNamespace memo is elided (pure resolution — no
    /// observable beyond repeated lookups).
    pub(crate) fn get_jsx_namespace_at(
        &mut self,
        location: NodeId,
    ) -> CheckResult2<Option<SymbolId>> {
        let resolved_namespace =
            match self.get_jsx_namespace_container_for_implicit_import(location)? {
                Some(container) => Some(container),
                None => {
                    let namespace_name = self.get_jsx_namespace_name(location);
                    self.resolve_name(
                        Some(location),
                        &namespace_name,
                        SymbolFlags::NAMESPACE,
                        None,
                        /*is_use*/ false,
                        /*exclude_globals*/ false,
                    )?
                }
            };
        if let Some(resolved_namespace) = resolved_namespace {
            // resolveSymbol(getSymbol(getExportsOfSymbol(
            //   resolveSymbol(resolvedNamespace)), JSX, Namespace)):
            // the faithful composition — alias namespaces resolve
            // through, and the getSymbol alias arm answers aliased
            // JSX members by target flags (M4 5.9d).
            let resolved = self.resolve_symbol_ex(Some(resolved_namespace), false)?;
            let candidate = match resolved {
                Some(resolved) => {
                    let exports = self.get_exports_of_symbol(resolved)?;
                    self.get_symbol_in_table(&exports, JSX_NAMESPACE_NAME, SymbolFlags::NAMESPACE)?
                }
                None => None,
            };
            let candidate = self.resolve_symbol_ex(candidate, false)?;
            if let Some(candidate) = candidate {
                if candidate != self.unknown_symbol {
                    return Ok(Some(candidate));
                }
            }
        }
        let global = self.get_global_symbol(
            JSX_NAMESPACE_NAME,
            SymbolFlags::NAMESPACE,
            /*diagnostic*/ None,
        );
        let global = global.map(|symbol| self.get_merged_symbol(symbol));
        match self.resolve_symbol_ex(global, false)? {
            Some(symbol) if symbol != self.unknown_symbol => Ok(Some(symbol)),
            _ => Ok(None),
        }
    }

    /// tsc-port: getJsxNamespaceContainerForImplicitImport @6.0.3
    /// tsc-hash: 16500826e40cbc991b27656d3a45fccf52785730d7e32550d1a46c56698a822c
    /// tsc-span: _tsc.js:74563-74585
    ///
    /// Non-None requires getJSXImplicitImportBase — jsx react-jsx/
    /// react-jsxdev or an @jsxImportSource pragma — both escape in
    /// the entity guard, so the survivors always answer None (2792/
    /// 2875 module-resolution rows ride 5.8).
    fn get_jsx_namespace_container_for_implicit_import(
        &mut self,
        location: NodeId,
    ) -> CheckResult2<Option<SymbolId>> {
        let file_index = self.binder.file_index_of_node(location);
        if let Some(cached) = self.jsx_implicit_import_containers.get(&file_index) {
            return Ok(*cached);
        }
        let pragmas = leading_jsx_pragmas(&self.binder.source(file_index).text);
        let base = if pragmas.runtime.as_deref() == Some("classic") {
            None
        } else if matches!(self.options.jsx, Some(4) | Some(5))
            || self.options.jsx_import_source.is_some()
            || pragmas.import_source.is_some()
            || pragmas.runtime.as_deref() == Some("automatic")
        {
            Some(
                pragmas
                    .import_source
                    .or_else(|| self.options.jsx_import_source.clone())
                    .unwrap_or_else(|| "react".to_owned()),
            )
        } else {
            None
        };
        let Some(base) = base else {
            self.jsx_implicit_import_containers.insert(file_index, None);
            return Ok(None);
        };
        let runtime = format!(
            "{base}/{}",
            if self.options.jsx == Some(5) {
                "jsx-dev-runtime"
            } else {
                "jsx-runtime"
            }
        );
        let error_message = if self.options.emit_module_resolution_kind() == 1 {
            &diagnostics::Cannot_find_module_0_Did_you_mean_to_set_the_moduleResolution_option_to_nodenext_or_to_add_aliases_to_the_paths_option
        } else {
            &diagnostics::This_JSX_tag_requires_the_module_path_0_to_exist_but_none_could_be_found_Make_sure_you_have_types_for_the_appropriate_package_installed
        };
        let module = self.resolve_external_module(
            location,
            &runtime,
            Some(error_message),
            Some(location),
            /*is_for_augmentation*/ false,
        )?;
        let resolved = match module {
            Some(module) if module != self.unknown_symbol => self
                .resolve_symbol_ex(Some(module), false)?
                .map(|symbol| self.get_merged_symbol(symbol)),
            _ => None,
        };
        self.jsx_implicit_import_containers
            .insert(file_index, resolved);
        Ok(resolved)
    }

    /// tsc-port: getJsxNamespace @6.0.3
    /// tsc-hash: 8ff29aa0c80ee1a5faf4121789c901358721fcc508acdf6030ee1d23e096462a
    /// tsc-span: _tsc.js:47491-47537
    ///
    pub(crate) fn get_jsx_namespace_name(&self, location: NodeId) -> String {
        let pragmas = leading_jsx_pragmas(&self.binder.source_of_node(location).text);
        if matches!(
            self.kind_of(location),
            SyntaxKind::JsxOpeningFragment | SyntaxKind::JsxFragment
        ) {
            // An invalid local jsxfrag shadows the compiler option in
            // getJsxFragmentFactoryEntity, then falls through to the
            // ordinary JSX namespace.
            if let Some(local) = pragmas.fragment_factory {
                return first_entity_identifier(&local)
                    .unwrap_or_else(|| self.global_jsx_namespace_name());
            }
            if let Some(option) = self.options.jsx_fragment_factory.as_deref() {
                return first_entity_identifier(option)
                    .unwrap_or_else(|| self.global_jsx_namespace_name());
            }
            return self.global_jsx_namespace_name();
        }
        if let Some(local) = pragmas.factory {
            if let Some(namespace) = first_entity_identifier(&local) {
                return namespace;
            }
        }
        self.global_jsx_namespace_name()
    }

    fn get_jsx_factory_namespace_name(&self, location: NodeId) -> String {
        let pragmas = leading_jsx_pragmas(&self.binder.source_of_node(location).text);
        pragmas
            .factory
            .as_deref()
            .and_then(first_entity_identifier)
            .unwrap_or_else(|| self.global_jsx_namespace_name())
    }

    fn global_jsx_namespace_name(&self) -> String {
        match self.options.jsx_factory.as_deref() {
            Some(factory) => first_entity_identifier(factory).unwrap_or_else(|| "React".to_owned()),
            None => self
                .options
                .react_namespace
                .clone()
                .unwrap_or_else(|| "React".to_owned()),
        }
    }

    // ---- grammar ----

    /// tsc-port: checkGrammarJsxElement @6.0.3
    /// tsc-hash: de32567b8f0e77ec4a738e463f04b7d52b0d8c4dbe6e5d09a65bec2270a00981
    /// tsc-span: _tsc.js:89728-89747
    fn check_grammar_jsx_element(&mut self, node: NodeId) -> bool {
        let (tag_name, type_arguments, attributes) = match self.data_of(node) {
            NodeData::JsxOpeningElement(data) => {
                (data.tag_name, data.type_arguments, data.attributes)
            }
            NodeData::JsxSelfClosingElement(data) => {
                (data.tag_name, data.type_arguments, data.attributes)
            }
            _ => (None, None, None),
        };
        if let Some(tag_name) = tag_name {
            self.check_grammar_jsx_name(tag_name);
        }
        self.check_grammar_type_arguments(node, type_arguments);
        let properties = attributes.and_then(|attributes| match self.data_of(attributes) {
            NodeData::JsxAttributes(data) => data.properties,
            _ => None,
        });
        let mut seen: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
        for attr in self.nodes_of(properties) {
            if self.kind_of(attr) == SyntaxKind::JsxSpreadAttribute {
                continue;
            }
            let NodeData::JsxAttribute(data) = self.data_of(attr).clone() else {
                continue;
            };
            let Some(name) = data.name else { continue };
            let escaped_text = self.jsx_attribute_name_text(name);
            if seen.insert(escaped_text, true).is_some() {
                return self.grammar_error_on_node(
                    name,
                    &diagnostics::JSX_elements_cannot_have_multiple_attributes_with_the_same_name,
                    &[],
                );
            }
            if let Some(initializer) = data.initializer {
                if self.kind_of(initializer) == SyntaxKind::JsxExpression
                    && matches!(
                        self.data_of(initializer),
                        NodeData::JsxExpression(expr) if expr.expression.is_none()
                    )
                {
                    return self.grammar_error_on_node(
                        initializer,
                        &diagnostics::JSX_attributes_must_only_be_assigned_a_non_empty_expression,
                        &[],
                    );
                }
            }
        }
        false
    }

    /// tsc-port: checkGrammarJsxName @6.0.3
    /// tsc-hash: fc54050a5171b0fcf12ec185a0c99b47e6db9e8c2bdbbafc6f1cf28c8ee8601d
    /// tsc-span: _tsc.js:89748-89755
    fn check_grammar_jsx_name(&mut self, node: NodeId) -> bool {
        if self.kind_of(node) == SyntaxKind::PropertyAccessExpression {
            if let NodeData::PropertyAccessExpression(data) = self.data_of(node) {
                if let Some(expression) = data.expression {
                    if self.kind_of(expression) == SyntaxKind::JsxNamespacedName {
                        return self.grammar_error_on_node(
                            expression,
                            &diagnostics::JSX_property_access_expressions_cannot_include_JSX_namespace_names,
                            &[],
                        );
                    }
                }
            }
        }
        if self.kind_of(node) == SyntaxKind::JsxNamespacedName {
            // getJSXTransformEnabled: jsx react/react-jsx/react-jsxdev.
            let transform_enabled = matches!(self.options.jsx, Some(2) | Some(4) | Some(5));
            if transform_enabled {
                let namespace_intrinsic = match self.data_of(node) {
                    NodeData::JsxNamespacedName(data) => data
                        .namespace
                        .and_then(|namespace| self.identifier_text(namespace))
                        .is_some_and(is_intrinsic_jsx_name),
                    _ => false,
                };
                if !namespace_intrinsic {
                    return self.grammar_error_on_node(
                        node,
                        &diagnostics::React_components_cannot_include_JSX_namespace_names,
                        &[],
                    );
                }
            }
        }
        false
    }

    /// tsc-port: checkGrammarJsxExpression @6.0.3
    /// tsc-hash: 00f4c03686f682f746da8112cbe21d43ba2169bf5a6f0da7f06927c992b00871
    /// tsc-span: _tsc.js:89756-89760
    fn check_grammar_jsx_expression(&mut self, node: NodeId) -> bool {
        let expression = match self.data_of(node) {
            NodeData::JsxExpression(data) => data.expression,
            _ => None,
        };
        let Some(expression) = expression else {
            return false;
        };
        // isCommaSequence: a comma BinaryExpression (possibly nested).
        let is_comma = self.kind_of(expression) == SyntaxKind::BinaryExpression
            && matches!(
                self.data_of(expression),
                NodeData::BinaryExpression(data)
                    if data.operator_token.is_some_and(
                        |token| self.kind_of(token) == SyntaxKind::CommaToken
                    )
            );
        if is_comma {
            return self.grammar_error_on_node(
                expression,
                &diagnostics::JSX_expressions_may_not_use_the_comma_operator_Did_you_mean_to_write_an_array,
                &[],
            );
        }
        false
    }

    /// getEscapedTextOfJsxAttributeName: identifier text or
    /// "namespace:name".
    pub(crate) fn jsx_attribute_name_text(&self, name: NodeId) -> String {
        match self.data_of(name) {
            NodeData::Identifier(data) => data.escaped_text.clone(),
            NodeData::JsxNamespacedName(data) => {
                let namespace = data
                    .namespace
                    .and_then(|n| self.identifier_text(n))
                    .unwrap_or_default();
                let member = data
                    .name
                    .and_then(|n| self.identifier_text(n))
                    .unwrap_or_default();
                format!("{namespace}:{member}")
            }
            _ => String::new(),
        }
    }
}

/// tsc-port: isIntrinsicJsxName @6.0.3
/// tsc-hash: d8f235d3962904d9679df95b374a3fd3bcf2334b1b8d7a6d1a8b5a495d224202
/// tsc-span: _tsc.js:16350-16353
fn is_intrinsic_jsx_name(name: &str) -> bool {
    name.as_bytes()
        .first()
        .is_some_and(|&b| b.is_ascii_lowercase())
        || name.contains('-')
}

#[cfg(test)]
mod tests {
    use tsrs2_types::CompilerOptions;

    use super::leading_jsx_pragmas;
    use crate::state::test_support::with_program_state;

    /// Driver-level fixture check — oracle-pinned rows (tsc 6.0.3,
    /// noLib, .tsx, options per test) — scratchpad j*.tsx probes,
    /// 2026-07-13.
    fn checked_rows_with(text: &str, options: &CompilerOptions) -> Vec<(u32, u32, u32)> {
        with_program_state(&[("a.tsx", text)], options, |state| {
            state.check_source_file(0);
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
        })
    }

    fn jsx(value: i32) -> CompilerOptions {
        CompilerOptions {
            jsx: Some(value),
            ..CompilerOptions::default()
        }
    }

    #[test]
    fn jsx_attribute_elaboration_uses_2322_at_the_attribute_name() {
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element {} interface IntrinsicElements { x: { n: number } } }\n\
                 (<x n=\"s\" />);\n\
                 (<x q={1} />);\n",
                &jsx(1),
            ),
            [(2322, 100, 1), (2322, 115, 1)]
        );
    }

    #[test]
    fn multiple_jsx_children_elaborate_one_row_per_child() {
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element {} interface ElementChildrenAttribute { children: any } }\n\
                 declare function Comp(p: { children: [string, string] }): JSX.Element;\n\
                 (<Comp>{1}{2}</Comp>);\n",
                &jsx(1),
            ),
            [(2322, 178, 3), (2322, 181, 3)]
        );
    }

    #[test]
    fn required_intrinsic_attribute_selects_the_missing_property_head() {
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element {} interface ElementClass { render: any } interface IntrinsicAttributes { key: string } interface IntrinsicClassAttributes<T> { ref: T } interface IntrinsicElements {} }\n\
                 interface I { new(n: string): { x: number; render(): void } }\n\
                 declare var E: I;\n\
                 (<E x={10} />);\n",
                &jsx(1),
            ),
            [(2741, 294, 1)]
        );
    }

    #[test]
    fn jsx_text_inside_a_string_is_not_a_pragma() {
        let rows = checked_rows_with(
            "declare namespace JSX { interface Element {} interface IntrinsicElements { div: { id: string } } }\n\
             declare var React: any;\n\
             const marker = \"@jsx\";\n\
             (<div id={1} />);\n",
            &jsx(1),
        );
        assert!(rows.iter().any(|row| row.0 == 2322), "{rows:?}");
    }

    #[test]
    fn jsx_pragma_collection_matches_multiline_and_precedence_rules() {
        let pragmas = leading_jsx_pragmas(
            "// @jsx Ignored.h\n\
             /** @jsx First.h */\n\
             /** @jsx Second.h\n\
                 @jsxfrag First.Fragment\n\
                 @jsxfrag Second.Fragment\n\
                 @jsximportsource first\n\
                 @jsximportsource second\n\
                 @jsxruntime classic\n\
                 @jsxruntime automatic */\n\
             const value = 1;\n\
             /** @jsx TooLate.h */",
        );
        assert_eq!(pragmas.factory.as_deref(), Some("First.h"));
        assert_eq!(pragmas.fragment_factory.as_deref(), Some("First.Fragment"));
        assert_eq!(pragmas.import_source.as_deref(), Some("second"));
        assert_eq!(pragmas.runtime.as_deref(), Some("automatic"));
    }

    #[test]
    fn jsx_factory_option_selects_its_namespace() {
        let rows = checked_rows_with(
            "declare namespace Preact { namespace JSX { interface Element {} interface IntrinsicElements { div: { id: string } } } function h(): any; }\n\
             (<div id={1} />);\n",
            &CompilerOptions {
                jsx: Some(2),
                jsx_factory: Some("Preact.h".to_owned()),
                ..CompilerOptions::default()
            },
        );
        assert!(rows.iter().any(|row| row.0 == 2322), "{rows:?}");
        assert!(!rows.iter().any(|row| row.0 == 2874), "{rows:?}");
    }

    #[test]
    fn invalid_jsx_factory_option_falls_back_to_react_namespace() {
        let rows = checked_rows_with(
            "declare namespace React { namespace JSX { interface Element {} interface IntrinsicElements { div: { id: string } } } }\n\
             declare var React: any;\n\
             (<div id={1} />);\n",
            &CompilerOptions {
                jsx: Some(2),
                jsx_factory: Some("Preact.!".to_owned()),
                ..CompilerOptions::default()
            },
        );
        assert!(rows.iter().any(|row| row.0 == 2322), "{rows:?}");
        assert!(!rows.iter().any(|row| row.0 == 2874), "{rows:?}");
    }

    #[test]
    fn jsx_pragma_selects_its_namespace() {
        let rows = checked_rows_with(
            "/** @jsx Preact.h */\n\
             declare namespace Preact { namespace JSX { interface Element {} interface IntrinsicElements { div: { id: string } } } function h(): any; }\n\
             (<div id={1} />);\n",
            &jsx(2),
        );
        assert!(rows.iter().any(|row| row.0 == 2322), "{rows:?}");
        assert!(!rows.iter().any(|row| row.0 == 2874), "{rows:?}");
    }

    #[test]
    fn automatic_jsx_runtime_reports_a_missing_runtime_module_once() {
        let rows = checked_rows_with("(<div />);\n(<span />);\n", &jsx(4));
        assert_eq!(
            rows.iter().filter(|row| row.0 == 2875).count(),
            1,
            "{rows:?}"
        );
        assert!(rows.iter().any(|row| row.0 == 7026), "{rows:?}");
    }

    #[test]
    fn automatic_jsx_runtime_uses_exported_jsx_namespace() {
        let rows = checked_rows_with(
            "declare module \"react/jsx-runtime\" {\n\
               export namespace JSX { interface Element {} interface IntrinsicElements { div: { id: string } } }\n\
             }\n\
             (<div id={1} />);\n",
            &jsx(4),
        );
        assert!(!rows.iter().any(|row| row.0 == 2875), "{rows:?}");
        assert!(rows.iter().any(|row| row.0 == 2322), "{rows:?}");
    }

    #[test]
    fn no_jsx_option_reports_17004_and_empty_initializer_17000() {
        // Oracle (scratchpad j58.tsx probe, 2026-07-14): the FULL
        // 11-row set — 5.8a's checkVariableDeclaration recovered the
        // const-statement rows (7026/17004/17001/2695/18007) that the
        // 5.7c pin recorded as demand-caveat FN (risk §14.9 flip).
        assert_eq!(
            checked_rows_with(
                "declare var React: any;\nconst a = <div id=\"x\" id=\"y\" />;\nconst b = <span>{1, 2}</span>;\n(<p attr={} />);\n",
                &CompilerOptions::default(),
            ),
            [
                (17001, 46, 2),
                (17004, 34, 21),
                (7026, 34, 21),
                (17004, 67, 6),
                (7026, 67, 6),
                (18007, 74, 4),
                (2695, 74, 1),
                (7026, 79, 7),
                (17000, 97, 2),
                (17004, 89, 13),
                (7026, 89, 13),
            ]
        );
    }

    #[test]
    fn duplicate_attribute_reports_17001_in_expression_position() {
        // Oracle: 7026 @25+21 + 17004 @25+21 + 17001 @37+2 — the FULL
        // oracle set (5.7c recovered the 7026 row).
        assert_eq!(
            checked_rows_with(
                "declare var React: any;\n(<div id=\"x\" id=\"y\" />);\n",
                &CompilerOptions::default(),
            ),
            [(17001, 37, 2), (17004, 25, 21), (7026, 25, 21)]
        );
    }

    #[test]
    fn value_tag_without_signatures_reports_2604() {
        // Oracle (jsx: preserve, noLib + hand-declared Function so the
        // isUntypedFunctionCall globalFunctionType probe stays honest —
        // the degenerate noLib Function absorbs every object type):
        // 2604 @147+4 at the tag name.
        assert_eq!(
            checked_rows_with(
                "interface Function { $brand: 1 }\ndeclare namespace JSX { interface Element { e: 1 } }\ndeclare var React: any;\ndeclare const Comp: { x: number };\n(<Comp />);\n",
                &jsx(1),
            ),
            [(2604, 147, 4)]
        );
    }

    #[test]
    fn intrinsic_type_arguments_report_2558() {
        // Oracle (jsx: preserve): 2558 @136+6 on the typeArguments
        // range (the intrinsic fake signature expects 0).
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } interface IntrinsicElements { div: { id?: string } } }\ndeclare var React: any;\n(<div<string> id=\"a\" />);\n",
                &jsx(1),
            ),
            [(2558, 136, 6)]
        );
    }

    #[test]
    fn react_fragment_with_untyped_react_stays_silent() {
        // Oracle (jsx: react): NO rows — React resolves as a value,
        // its exports carry no Fragment, the fragment type is
        // errorType and resolveErrorCall stays silent.
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } }\ndeclare var React: any;\n(<>x</>);\n",
                &jsx(2),
            ),
            []
        );
    }

    #[test]
    fn react_fragment_without_react_reports_2874_and_2879() {
        // Oracle (jsx: react): 2874 @54+2 (markJsxAliasReferenced's
        // factory probe) + 2879 @54+2 (getJSXFragmentType's resolve).
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } }\n(<>x</>);\n",
                &jsx(2),
            ),
            [(2874, 54, 2), (2879, 54, 2)]
        );
    }

    #[test]
    fn sfc_wrong_return_type_reports_2786() {
        // Oracle (jsx: preserve): 2786 @171+1 at the tag name (chain:
        // "Its return type 'number' is not a valid JSX element").
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } interface ElementClass { render(): void } }\ndeclare var React: any;\ndeclare function F(props: { a: string }): number;\n(<F a=\"x\" />);\n",
                &jsx(1),
            ),
            [(2786, 171, 1)]
        );
    }

    #[test]
    fn class_component_wrong_instance_type_reports_2786() {
        // Oracle (jsx: preserve): 2786 @213+1 at the tag name (chain:
        // "Its instance type 'C' is not a valid JSX element") — the
        // Component ref kind + ElementAttributesProperty props path.
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } interface ElementClass { render(): void } interface ElementAttributesProperty { props: {} } }\ndeclare var React: any;\ndeclare class C { props: { a: string }; }\n(<C a=\"x\" />);\n",
                &jsx(1),
            ),
            [(2786, 213, 1)]
        );
    }

    #[test]
    fn children_specified_twice_reports_2710() {
        // Oracle (jsx: preserve): 2710 @194+12 at the attributes node
        // (explicit `children` attribute + semantic children).
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } interface ElementChildrenAttribute { children: {} } interface IntrinsicElements { div: { children?: string } } }\ndeclare var React: any;\n(<div children=\"a\">text</div>);\n",
                &jsx(1),
            ),
            [(2710, 194, 12)]
        );
    }

    #[test]
    fn non_object_jsx_spread_reports_2698() {
        // Oracle (jsx: preserve): 2698 @165+1 at the spread expression
        // + 2559 @157+3 at the tag. The 2559 head is the headless
        // reportRelationError face — T2-contained here (recorded FN);
        // the 2698 row emitted by the attributes worker survives the
        // containment.
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } interface IntrinsicElements { div: { id?: string } } }\ndeclare var React: any;\ndeclare const n: number;\n(<div {...n} />);\n",
                &jsx(1),
            ),
            [(2698, 165, 1)]
        );
    }

    #[test]
    fn unknown_intrinsic_tag_reports_2339_on_opening_and_closing() {
        // Oracle (jsx: preserve): 2339 @118+5 (opening `<foo>`) +
        // 2339 @124+6 (closing `</foo>`) vs JSX.IntrinsicElements.
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } interface IntrinsicElements { div: {} } }\ndeclare var React: any;\n(<foo>x</foo>);\n",
                &jsx(1),
            ),
            [(2339, 118, 5), (2339, 124, 6)]
        );
    }

    #[test]
    fn inline_whitespace_child_is_semantic_and_fires_2710() {
        // Oracle (jsx: preserve): 2710 @194+12 — `<div children="a"> `
        // has an INLINE-SPACE text child (no line break), which is a
        // SEMANTIC string child (scanJsxToken keeps firstNonWhitespace
        // at 0, NOT -1) — the children synthesis runs and the explicit
        // `children` attribute reports as overwritten.
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } interface ElementChildrenAttribute { children: {} } interface IntrinsicElements { div: { children?: string } } }\ndeclare var React: any;\n(<div children=\"a\"> </div>);\n",
                &jsx(1),
            ),
            [(2710, 194, 12)]
        );
    }

    #[test]
    fn line_break_whitespace_child_is_trivia_and_stays_silent() {
        // Oracle (jsx: preserve): NO rows — the same shape with a
        // LINE-BREAK whitespace child is JsxTextAllWhiteSpaces
        // (non-semantic), so no children synthesis and no 2710.
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } interface ElementChildrenAttribute { children: {} } interface IntrinsicElements { div: { children?: string } } }\ndeclare var React: any;\n(<div children=\"a\">\n</div>);\n",
                &jsx(1),
            ),
            []
        );
    }

    #[test]
    fn aliased_react_jsx_namespace_reports_2339() {
        // Oracle (jsx: preserve): 2339 @143+7 — tsc resolveSymbol()s
        // the `export import JSX = Inner` alias to the real container
        // and reports the unknown intrinsic. LIVE since 5.9d's
        // getSymbol alias arm (previously a recorded FN).
        assert_eq!(
            checked_rows_with(
                "declare namespace React { namespace Inner { interface Element { e: 1 } interface IntrinsicElements { div: {} } } export import JSX = Inner; }\n(<foo />);\n",
                &jsx(1),
            ),
            [(2339, 143, 7)]
        );
    }

    #[test]
    fn factory_arity_reports_6229() {
        // Oracle (jsx: preserve): 6229 @229+4 at the tag name — Comp
        // requires 3 arguments but React.createElement's first-param
        // signatures provide at most 2.
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } }\ndeclare namespace React { function createElement(tag: (a: string, b: string) => any, props: any): any; }\ndeclare function Comp(a: string, b: string, c: string): JSX.Element;\n(<Comp />);\n",
                &jsx(1),
            ),
            [(6229, 229, 4)]
        );
    }

    #[test]
    fn class_without_props_property_reports_2607() {
        // Oracle (jsx: preserve): 2607 @159+11 at the opening element —
        // ElementAttributesProperty forces a `props` lookup the class
        // lacks, and attributes are present.
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } interface ElementAttributesProperty { props: {} } }\ndeclare var React: any;\ndeclare class C { m(): void; }\n(<C a=\"x\" />);\n",
                &jsx(1),
            ),
            [(2607, 159, 11)]
        );
    }

    #[test]
    fn multi_property_children_container_reports_2608() {
        // Oracle (jsx: preserve): 2608 @61+24 at the container's NAME
        // (the ElementChildrenAttribute interface declaration).
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } interface ElementChildrenAttribute { children: {}; kids: {} } interface IntrinsicElements { div: {} } }\ndeclare var React: any;\n(<div>text</div>);\n",
                &jsx(1),
            ),
            [(2608, 61, 24)]
        );
    }

    #[test]
    fn element_type_constraint_reports_2786() {
        // Oracle (jsx: preserve): 2786 @155+4 at the tag name — the
        // JSX.ElementType alias constrains tags to "div"; the "span"
        // string-literal tag type fails (chain: Its type '"span"' is
        // not a valid JSX element type).
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { e: 1 } type ElementType = \"div\"; interface IntrinsicElements { div: {}; span: {} } }\ndeclare var React: any;\n(<span />);\n",
                &jsx(1),
            ),
            [(2786, 155, 4)]
        );
    }

    #[test]
    fn library_managed_attributes_drive_contextual_typing() {
        // Oracle (jsx: preserve): 2339 @237+3 — LibraryManagedAttributes
        // REPLACES the props type, so the callback parameter is
        // contextually typed V and `v.bad` misses. (The oracle's 6205
        // unused-type-parameters row is suggestion-band, M7 FN.)
        assert_eq!(
            checked_rows_with(
                "interface V { m: number }\ndeclare namespace JSX { interface Element { e: 1 } type LibraryManagedAttributes<C, P> = { cb?: (v: V) => void }; }\ndeclare var React: any;\ndeclare function F(props: { a?: string }): JSX.Element;\n(<F cb={v => v.bad} />);\n",
                &jsx(1),
            ),
            [(2339, 237, 3)]
        );
    }

    #[test]
    fn jsx_attribute_callback_is_contextually_typed() {
        // Oracle (jsx: preserve): 2339 @183+3 — the attribute value's
        // arrow parameter is contextually typed from the props member
        // (`v: V`), so `v.bad` misses.
        assert_eq!(
            checked_rows_with(
                "interface V { m: number }\ndeclare namespace JSX { interface Element { e: 1 } }\ndeclare var React: any;\ndeclare function F(props: { cb?: (v: V) => void }): JSX.Element;\n(<F cb={v => v.bad} />);\n",
                &jsx(1),
            ),
            [(2339, 183, 3)]
        );
    }

    #[test]
    fn declared_jsx_namespace_with_jsx_option_reports_no_intrinsics_7026() {
        // Oracle (jsx: preserve): 7026 @83+13 + 2339 @117+3 — the FULL
        // oracle set: the namespace resolves (no 17004), the div tag
        // misses JSX.IntrinsicElements (7026), the fragment rides the
        // untyped-call path (anyType at jsx=preserve) cleanly.
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { x: number } }\ndeclare var React: any;\n(<div a=\"1\" />);\n(<>text</>);\n(\"x\".bad);\n",
                &jsx(1),
            ),
            [(2339, 117, 3), (7026, 83, 13)]
        );
    }
}
