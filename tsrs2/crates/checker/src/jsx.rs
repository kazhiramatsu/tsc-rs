//! M4 5.5f: the §9 JSX attribute slice — eager element/fragment arms,
//! the deferred opening check up to THE 5.7 BOUNDARY (L74804
//! `getResolvedSignature`), JSX grammar (2633/2639/17001/17000/18007)
//! and preconditions (17004). Everything past the boundary
//! (closing-tag checks, children, attributes-vs-props relations,
//! 2604/2605/18053/2786-2789) arrives with call resolution at 5.7.
//!
//! Namespace machinery scope: the `jsx` option is modeled; fixtures
//! that customize the namespace ENTITY (jsxFactory-family options,
//! @jsx pragma comments, react-jsx implicit imports) ESCAPE — pragma
//! collection and module resolution are unported (5.8), and a wrong
//! namespace would resolve the wrong JSX.* container (FP shape), so
//! containment wins.

use tsrs2_binder::SymbolId;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{SymbolFlags, TypeId};

use crate::state::{CheckResult2, CheckerState, Unsupported};
use tsrs2_diags::gen as diagnostics;

/// JsxNames (90915): the JSX.* well-known member names this slice
/// consults.
const JSX_NAMESPACE_NAME: &str = "JSX";
const JSX_ELEMENT: &str = "Element";

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
    /// The 17016/17017 pragma-factory errors read jsxFactory/pragmas —
    /// both are escape triggers in get_jsx_namespace_entity_guard, so
    /// the gate reduces to constant-false here. The tail past the
    /// opening check is dead at 5.5f (the opening check escapes at the
    /// getResolvedSignature boundary) — it lands live with 5.7.
    pub(crate) fn check_jsx_fragment(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let opening_fragment = match self.data_of(node) {
            NodeData::JsxFragment(data) => data.opening_fragment,
            _ => None,
        };
        if let Some(opening_fragment) = opening_fragment {
            self.check_jsx_opening_like_element_or_opening_fragment(opening_fragment)?;
        }
        self.check_jsx_children(node)?;
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
    pub(crate) fn check_jsx_expression(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        self.check_grammar_jsx_expression(node);
        let NodeData::JsxExpression(data) = self.data_of(node).clone() else {
            return Ok(self.tables.intrinsics.error);
        };
        let Some(expression) = data.expression else {
            return Ok(self.tables.intrinsics.error);
        };
        let ty = self.check_expression(expression, tsrs2_types::CheckMode::NORMAL)?;
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

    /// The JsxAttributes worker arm: createJsxAttributesTypeFromAttributesProperty
    /// (74346) builds the attributes object type — its observable
    /// consumers (props relations, contextual attribute types) all sit
    /// behind the resolveJsxOpeningLikeElement boundary. Escape with
    /// the 5.7 band rather than half-build the type (children
    /// synthesis fabricates a PropertySignature declaration).
    pub(crate) fn check_jsx_attributes_stub(&self) -> CheckResult2<TypeId> {
        Err(Unsupported::new(
            "checkJsxAttributes (attributes type construction, 5.7)",
        ))
    }

    // ---- deferred arms ----

    /// tsc-port: checkJsxElementDeferred @6.0.3
    /// tsc-hash: 2068fd98f6f9b417a668c1ce64f5e2eddbad0fbfac7de3c282bb47c997bc6776
    /// tsc-span: _tsc.js:74311-74319
    ///
    /// The closing-tag check and checkJsxChildren sit AFTER the
    /// opening check, whose getResolvedSignature boundary escapes —
    /// they arrive with 5.7.
    pub(crate) fn check_jsx_element_deferred(&mut self, node: NodeId) -> CheckResult2<()> {
        let opening_element = match self.data_of(node) {
            NodeData::JsxElement(data) => data.opening_element,
            _ => None,
        };
        if let Some(opening_element) = opening_element {
            self.check_jsx_opening_like_element_or_opening_fragment(opening_element)?;
        }
        // Unreachable until 5.7 (the opening check escaped above):
        // intrinsic closing tags → getIntrinsicTagSymbol; value tags →
        // checkExpression(tagName); then checkJsxChildren.
        unreachable!("the getResolvedSignature boundary escapes until 5.7");
    }

    /// tsc-port: checkJsxSelfClosingElementDeferred @6.0.3
    /// tsc-hash: f6f5be939a71796a6fcd510f53e32f83dbe02ba475d5e6bf13e63302298ac7df
    /// tsc-span: _tsc.js:74304-74306
    pub(crate) fn check_jsx_self_closing_element_deferred(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<()> {
        self.check_jsx_opening_like_element_or_opening_fragment(node)?;
        unreachable!("the getResolvedSignature boundary escapes until 5.7");
    }

    /// tsc-port: checkJsxOpeningLikeElementOrOpeningFragment @6.0.3
    /// tsc-hash: 2f04a1ed5759553f45561927e599df29785b0fc79d910ce8e4e72c25b3c1d0a9
    /// tsc-span: _tsc.js:74797-74825
    ///
    /// markJsxAliasReferenced (71787) is emit/alias bookkeeping — no-op
    /// hook. L74804 `getResolvedSignature` is THE 5.7 line: everything
    /// from there (checkDeprecatedSignature, the elementTypeConstraint
    /// relation with 2786-family, checkJsxReturnAssignableToAppropriateBound)
    /// escapes.
    fn check_jsx_opening_like_element_or_opening_fragment(
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
        // markJsxAliasReferenced: no-op hook (emit bookkeeping).
        Err(Unsupported::new(
            "getResolvedSignature over a JSX opening-like element (call resolution, 5.7)",
        ))
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

    /// tsc-port: checkJsxChildren @6.0.3
    /// tsc-hash: 4278af460ef6a9ddc82ec13a9987a5a5680260214fdf98c2be57a7f478751355
    /// tsc-span: _tsc.js:74496-74510
    fn check_jsx_children(&mut self, node: NodeId) -> CheckResult2<Vec<TypeId>> {
        let children = match self.data_of(node) {
            NodeData::JsxElement(data) => data.children,
            NodeData::JsxFragment(data) => data.children,
            _ => None,
        };
        let mut children_types = Vec::new();
        for child in self.nodes_of(children) {
            match self.kind_of(child) {
                SyntaxKind::JsxText => {
                    // containsOnlyTriviaWhiteSpaces: the scanner marks
                    // whitespace-only JSX text.
                    let is_trivia = match self.data_of(child) {
                        NodeData::JsxText(data) => {
                            data.text.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
                        }
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
                    children_types.push(self.check_expression_for_mutable_location(
                        child,
                        tsrs2_types::CheckMode::NORMAL,
                        false,
                    )?);
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
        let exports = self.get_exports_of_jsx_namespace(namespace)?;
        let Some(symbol) = exports.get(name).copied() else {
            return Ok(self.tables.intrinsics.error);
        };
        if !self
            .symbol_flags(symbol)
            .intersects(SymbolFlags::TYPE)
        {
            return Ok(self.tables.intrinsics.error);
        }
        self.get_declared_type_of_symbol_slice(symbol)
    }

    /// getExportsOfSymbol over the JSX namespace container: plain
    /// namespaces read their exports table; anything needing the
    /// export-star walk rides the 5.8 module band (annotate.rs
    /// get_exports_of_symbol escape) — here the JSX namespace is by
    /// construction a namespace symbol, so read the table directly.
    fn get_exports_of_jsx_namespace(
        &mut self,
        namespace: SymbolId,
    ) -> CheckResult2<tsrs2_binder::SymbolTable> {
        Ok(self.binder.symbol(namespace).exports.clone())
    }

    /// tsc-port: getJsxNamespaceAt @6.0.3
    /// tsc-hash: 56cd592146ecb83505d526562a9ebe446de69ccd680f8f2418766b0fc36d9cba
    /// tsc-span: _tsc.js:74586-74628
    ///
    /// The links.jsxNamespace memo is elided (pure resolution — no
    /// observable beyond repeated lookups). resolveSymbol over the
    /// resolved names is the alias-free face (alias resolution is
    /// 5.8; an aliased React/JSX container escapes below).
    fn get_jsx_namespace_at(&mut self, location: NodeId) -> CheckResult2<Option<SymbolId>> {
        self.jsx_namespace_entity_guard(location)?;
        if let Some(container) = self.get_jsx_namespace_container_for_implicit_import(location)? {
            let _ = container;
            unreachable!("the implicit-import arm escapes in the guard");
        }
        let namespace_name = self.get_jsx_namespace_name(location);
        let resolved_namespace = self.resolve_name(
            Some(location),
            &namespace_name,
            SymbolFlags::NAMESPACE,
            None,
            /*is_use*/ false,
            /*exclude_globals*/ false,
        );
        if let Some(resolved_namespace) = resolved_namespace {
            if self
                .symbol_flags(resolved_namespace)
                .intersects(SymbolFlags::ALIAS)
            {
                return Err(Unsupported::new(
                    "aliased JSX factory namespace (alias resolution, 5.8)",
                ));
            }
            let exports = self.binder.symbol(resolved_namespace).exports.clone();
            if let Some(&candidate) = exports.get(JSX_NAMESPACE_NAME) {
                if self
                    .symbol_flags(candidate)
                    .intersects(SymbolFlags::NAMESPACE_MODULE | SymbolFlags::VALUE_MODULE)
                    && candidate != self.unknown_symbol
                {
                    if self
                        .symbol_flags(candidate)
                        .intersects(SymbolFlags::ALIAS)
                    {
                        return Err(Unsupported::new(
                            "aliased JSX namespace member (alias resolution, 5.8)",
                        ));
                    }
                    return Ok(Some(self.get_merged_symbol(candidate)));
                }
            }
        }
        let global = self.get_global_symbol(
            JSX_NAMESPACE_NAME,
            SymbolFlags::NAMESPACE,
            /*diagnostic*/ None,
        );
        match global {
            Some(symbol) if symbol != self.unknown_symbol => {
                if self.symbol_flags(symbol).intersects(SymbolFlags::ALIAS) {
                    return Err(Unsupported::new(
                        "aliased global JSX namespace (alias resolution, 5.8)",
                    ));
                }
                Ok(Some(self.get_merged_symbol(symbol)))
            }
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
        _location: NodeId,
    ) -> CheckResult2<Option<SymbolId>> {
        Ok(None)
    }

    /// tsc-port: getJsxNamespace @6.0.3
    /// tsc-hash: 8ff29aa0c80ee1a5faf4121789c901358721fcc508acdf6030ee1d23e096462a
    /// tsc-span: _tsc.js:47491-47537
    ///
    /// The surviving face after the entity guard: reactNamespace ‖
    /// "React" (pragma/jsxFactory shapes escaped).
    fn get_jsx_namespace_name(&self, _location: NodeId) -> String {
        self.options
            .react_namespace
            .clone()
            .unwrap_or_else(|| "React".to_owned())
    }

    /// The FP guard for namespace-entity customization: @jsx-family
    /// pragma comments (collected by tsc's processPragmasIntoFields —
    /// unported) and the jsxFactory-family options change WHICH
    /// entity resolves the JSX namespace; jsx react-jsx/react-jsxdev
    /// adds the implicit-import module resolution (5.8). All escape.
    fn jsx_namespace_entity_guard(&self, location: NodeId) -> CheckResult2<()> {
        if self.options.jsx_factory.is_some()
            || self.options.jsx_fragment_factory.is_some()
            || self.options.jsx_import_source.is_some()
        {
            return Err(Unsupported::new(
                "jsxFactory-family options (JSX namespace entity parsing, 5.8)",
            ));
        }
        if matches!(self.options.jsx, Some(4) | Some(5)) {
            return Err(Unsupported::new(
                "react-jsx implicit import container (module resolution, 5.8)",
            ));
        }
        let source = self.binder.source_of_node(location);
        let text_lower = source.text.to_ascii_lowercase();
        if text_lower.contains("@jsx") {
            // Over-approximates tsc's pragma scan (comment-position
            // aware) — a stray "@jsx" in a string literal contains the
            // file's JSX checks (FN), never flips a namespace (FP).
            return Err(Unsupported::new(
                "@jsx-family pragma comment (pragma collection, 5.8)",
            ));
        }
        Ok(())
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
    fn jsx_attribute_name_text(&self, name: NodeId) -> String {
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
    fn no_jsx_option_reports_17004_and_empty_initializer_17000() {
        // Oracle: 17004 @89+13 + 17000 @97+2 (7026 rows ride 5.7's
        // resolveJsxOpeningLikeElement).
        assert_eq!(
            checked_rows_with(
                "declare var React: any;\nconst a = <div id=\"x\" id=\"y\" />;\nconst b = <span>{1, 2}</span>;\n(<p attr={} />);\n",
                &CompilerOptions::default(),
            ),
            [(17000, 97, 2), (17004, 89, 13)]
        );
    }

    #[test]
    fn duplicate_attribute_reports_17001_in_expression_position() {
        // Oracle: 17004 @25+21 + 17001 @37+2 (7026 rides 5.7).
        assert_eq!(
            checked_rows_with(
                "declare var React: any;\n(<div id=\"x\" id=\"y\" />);\n",
                &CompilerOptions::default(),
            ),
            [(17001, 37, 2), (17004, 25, 21)]
        );
    }

    #[test]
    fn declared_jsx_namespace_with_jsx_option_stays_silent() {
        // Oracle (jsx: preserve): only the 7026 rows (5.7 FN) — the
        // namespace resolves, no 17004, fragment/element statements
        // contain at the getResolvedSignature boundary.
        assert_eq!(
            checked_rows_with(
                "declare namespace JSX { interface Element { x: number } }\ndeclare var React: any;\n(<div a=\"1\" />);\n(<>text</>);\n(\"x\".bad);\n",
                &jsx(1),
            ),
            [(2339, 117, 3)]
        );
    }
}
