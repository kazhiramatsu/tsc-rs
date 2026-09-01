//! Checker-owned declaration-emit visibility and accessibility workers.
//!
//! These workers intentionally do not participate in the bounded display
//! slices in `check.rs`. The display path keeps its existing read-only
//! decisions, while this module owns declaration-emit memoization, alias
//! painting, and result-shaped accessibility.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use tsc_binder::{node_util, SymbolId};
use tsc_emitter::{
    EmitFunctionProperty, EmitResolverNode, EmitResolverSymbol, EmitSymbolAccessibility,
    EmitSymbolAccessibilityResult, EmitSymbolMeaning,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{CheckFlags, CompilerOptions, ModifierFlags, NodeFlags, SymbolFlags};

use crate::state::{CheckResult, CheckerState};

/// Narrow bridge to the two already-exact display prerequisites. The impl
/// stays beside their private bodies in `check.rs`; declaration-emit owns all
/// result shaping and mutation in this module.
pub(crate) trait DeclarationEmitAccessibilityPrimitives {
    fn declaration_emit_accessible_symbol_chain(
        &mut self,
        symbol: SymbolId,
        meaning: SymbolFlags,
        enclosing: Option<NodeId>,
    ) -> CheckResult<Option<Vec<SymbolId>>>;

    fn declaration_emit_containers_of_symbol(
        &mut self,
        symbol: SymbolId,
        enclosing: Option<NodeId>,
        meaning: SymbolFlags,
    ) -> CheckResult<Vec<SymbolId>>;
}

/// tsc-port: getEmitDeclarations @6.0.3
/// tsc-hash: b592b5f4c632a60784ba25e59679201356befc86d16c5a441be6439b53a5150c
/// tsc-span: _tsc.js:18151-18155
pub(crate) fn emit_declarations(options: &CompilerOptions) -> bool {
    options.declaration == Some(true) || options.composite == Some(true)
}

impl CheckerState<'_> {
    /// tsc-port: isDefinitelyReferenceToGlobalSymbolObject @6.0.3
    /// tsc-hash: 0a9f99b8eb62eb0a85b6019c76e13f9934dd0be091a0ebebf82f54a888ea237e
    /// tsc-span: _tsc.js:47469-47483
    pub(crate) fn emit_is_definitely_reference_to_global_symbol_object(
        &mut self,
        node: NodeId,
    ) -> CheckResult<bool> {
        let NodeData::PropertyAccessExpression(data) = self.data_of(node) else {
            return Ok(false);
        };
        let Some(name) = data.name else {
            return Ok(false);
        };
        if self.kind_of(name) != SyntaxKind::Identifier {
            return Ok(false);
        }
        let Some(expression) = data.expression else {
            return Ok(false);
        };

        match self.data_of(expression) {
            NodeData::Identifier(_) => {
                if self.identifier_text_of(expression) != Some("Symbol") {
                    return Ok(false);
                }
                let resolved = self
                    .get_resolved_symbol(expression)?
                    .unwrap_or(self.unknown_symbol);
                let global = self
                    .get_global_symbol(
                        "Symbol",
                        SymbolFlags::VALUE | SymbolFlags::EXPORT_VALUE,
                        None,
                    )?
                    .unwrap_or(self.unknown_symbol);
                Ok(resolved == global)
            }
            NodeData::PropertyAccessExpression(expression_data) => {
                let Some(global_this) = expression_data.expression else {
                    return Ok(false);
                };
                let Some(property_name) = expression_data.name else {
                    return Ok(false);
                };
                if self.kind_of(global_this) != SyntaxKind::Identifier
                    || self.identifier_text_of(global_this) != Some("globalThis")
                    || self.identifier_text_of(property_name) != Some("Symbol")
                {
                    return Ok(false);
                }
                Ok(self
                    .get_resolved_symbol(global_this)?
                    .unwrap_or(self.unknown_symbol)
                    == self.global_this_symbol)
            }
            _ => Ok(false),
        }
    }

    /// tsc-port: isOptionalParameter @6.0.3
    /// tsc-hash: 230cc8ce09e27fc4b9b6e370079e26817941e278127f592eca3c51ecb55ac67b
    /// tsc-span: _tsc.js:59509-59527
    ///
    /// The effective-question-token test deliberately precedes the
    /// Parameter-kind guard. The display slice retains its recorded
    /// post-guard order and is not routed through this worker.
    ///
    /// tsc's Debug.assert(parameterIndex >= 0) is a binder invariant. A
    /// malformed/recovery tree cannot carry that invariant across this
    /// typed boundary, so the Rust worker fails closed with `false`.
    pub(crate) fn emit_is_optional_parameter(&mut self, node: NodeId) -> CheckResult<bool> {
        let _replay_call = DeclarationReplayCallGuard::enter("resolver.isOptionalParameter");
        if self.has_question_token(node) || self.is_optional_declaration(node) {
            return Ok(true);
        }
        let NodeData::Parameter(data) = self.data_of(node) else {
            return Ok(false);
        };
        let Some(parent) = self.parent_of(node) else {
            return Ok(false);
        };
        if data.initializer.is_some() {
            let signature = self.get_signature_from_declaration(parent)?;
            let parameters = self.parameters_of_function(parent);
            let Some(parameter_index) = parameters.iter().position(|&parameter| parameter == node)
            else {
                return Ok(false);
            };
            return Ok(parameter_index >= self.min_argument_count_without_void_trimming(signature)?);
        }
        let Some(iife) = self.get_immediately_invoked_function_expression(parent) else {
            return Ok(false);
        };
        let parameters = self.parameters_of_function(parent);
        let Some(parameter_index) = parameters.iter().position(|&parameter| parameter == node)
        else {
            return Ok(false);
        };
        Ok(data.r#type.is_none()
            && data.dot_dot_dot_token.is_none()
            && parameter_index >= self.get_effective_call_arguments(iife)?.len())
    }

    /// tsc-port: isImplementationOfOverload @6.0.3
    /// tsc-hash: 8e84478797279cd09461d21f45d61335f27c12c7711fac1678ef91e806cfd378
    /// tsc-span: _tsc.js:88055-88068
    pub(crate) fn emit_is_implementation_of_overload(&mut self, node: NodeId) -> CheckResult<bool> {
        let source = self.binder.source_of_node(node);
        let body_is_present = node_util::body_of(source, node)
            .is_some_and(|body| !node_util::node_is_missing(source, Some(body)));
        if !body_is_present
            || matches!(
                self.kind_of(node),
                SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
            )
        {
            return Ok(false);
        }
        let Some(_) = self.node_symbol(node) else {
            return Ok(false);
        };
        let symbol = self.get_symbol_of_declaration(node)?;
        if symbol == self.unknown_symbol {
            return Ok(false);
        }
        let signatures = self.get_signatures_of_symbol(Some(symbol))?;
        Ok(signatures.len() > 1
            || signatures.len() == 1 && self.signature_of(signatures[0]).declaration != Some(node))
    }

    /// tsc-port: requiresAddingImplicitUndefined @6.0.3
    /// tsc-hash: 520f7a6ffc45898f262773b00042189eaf39127a40ed129daa103f6ce663e2d7
    /// tsc-span: _tsc.js:88075-88077
    ///
    /// The optional enclosing declaration is retained for the parameter
    /// property/function-like gate. The declared-type check uses the
    /// nonlocal effective annotation and keeps the `isErrorType` disjunct.
    pub(crate) fn emit_requires_adding_implicit_undefined(
        &mut self,
        parameter: NodeId,
        enclosing_declaration: Option<NodeId>,
    ) -> CheckResult<bool> {
        let _replay_call =
            DeclarationReplayCallGuard::enter("resolver.requiresAddingImplicitUndefined");
        let required =
            self.emit_is_required_initialized_parameter(parameter, enclosing_declaration)?;
        let optional_property =
            self.emit_is_optional_uninitialized_parameter_property(parameter)?;
        if !(required || optional_property) {
            return Ok(false);
        }
        Ok(!self.emit_declared_parameter_type_contains_undefined(parameter)?)
    }

    /// tsc-port: isRequiredInitializedParameter @6.0.3
    /// tsc-hash: ae6983141d7e5b9362ccdc291c29334c9e591a7946b073dcbcd36db38215ebe7
    /// tsc-span: _tsc.js:88078-88085
    fn emit_is_required_initialized_parameter(
        &mut self,
        parameter: NodeId,
        enclosing_declaration: Option<NodeId>,
    ) -> CheckResult<bool> {
        if !self
            .options
            .strict_option_value(self.options.strict_null_checks)
        {
            return Ok(false);
        }
        if self.emit_is_optional_parameter(parameter)?
            || self.kind_of(parameter) == SyntaxKind::JSDocParameterTag
        {
            return Ok(false);
        }
        let has_initializer = matches!(
            self.data_of(parameter),
            NodeData::Parameter(data) if data.initializer.is_some()
        );
        if !has_initializer {
            return Ok(false);
        }
        let source = self.binder.source_of_node(parameter);
        if node_util::has_syntactic_modifier(
            source,
            parameter,
            ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
        ) {
            return Ok(enclosing_declaration.is_some_and(|enclosing| {
                node_util::is_function_like_declaration_kind(self.kind_of(enclosing))
            }));
        }
        Ok(true)
    }

    /// tsc-port: isOptionalUninitializedParameterProperty @6.0.3
    /// tsc-hash: 19516da065e7ffd2f46cc49e81f3bab132778d45a8898827c93c870187dd10cb
    /// tsc-span: _tsc.js:88086-88089
    fn emit_is_optional_uninitialized_parameter_property(
        &mut self,
        parameter: NodeId,
    ) -> CheckResult<bool> {
        if !self
            .options
            .strict_option_value(self.options.strict_null_checks)
            || !self.emit_is_optional_parameter(parameter)?
        {
            return Ok(false);
        }
        let is_jsdoc_parameter = self.kind_of(parameter) == SyntaxKind::JSDocParameterTag;
        let has_initializer = matches!(
            self.data_of(parameter),
            NodeData::Parameter(data) if data.initializer.is_some()
        );
        if !is_jsdoc_parameter && has_initializer {
            return Ok(false);
        }
        Ok(node_util::has_syntactic_modifier(
            self.binder.source_of_node(parameter),
            parameter,
            ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
        ))
    }

    /// tsc-port: getNonlocalEffectiveTypeAnnotationNode @6.0.3
    /// tsc-hash: b538286bbef1aab02c6fac684de28e6c03ab0dd7768a651831492609e8cd2561
    /// tsc-span: _tsc.js:88532-88542
    /// (h2-7a-m-3 widening: NodeBuilder declaration-annotation lookup.)
    pub(crate) fn emit_nonlocal_effective_type_annotation_node(
        &mut self,
        parameter: NodeId,
    ) -> CheckResult<Option<NodeId>> {
        if let Some(annotation) = self.effective_type_annotation_node(parameter) {
            return Ok(Some(annotation));
        }
        if self.kind_of(parameter) != SyntaxKind::Parameter {
            return Ok(None);
        }
        let Some(setter) = self.parent_of(parameter) else {
            return Ok(None);
        };
        if self.kind_of(setter) != SyntaxKind::SetAccessor {
            return Ok(None);
        }
        let symbol = self.get_symbol_of_declaration(setter)?;
        if symbol == self.unknown_symbol {
            return Ok(None);
        }
        let getter = self
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| self.kind_of(declaration) == SyntaxKind::GetAccessor);
        Ok(getter.and_then(|getter| self.effective_return_type_node(getter)))
    }

    /// tsc-port: declaredParameterTypeContainsUndefined @6.0.3
    /// tsc-hash: ae4d909ece83865a74023d1c5e686c3373d3e94c91cc204c7b2e3af1a89eb274
    /// tsc-span: _tsc.js:88068-88074
    fn emit_declared_parameter_type_contains_undefined(
        &mut self,
        parameter: NodeId,
    ) -> CheckResult<bool> {
        let Some(annotation) = self.emit_nonlocal_effective_type_annotation_node(parameter)? else {
            return Ok(false);
        };
        let ty = self.get_type_from_type_node(annotation)?;
        Ok(self.tables.is_error_type(ty) || self.contains_undefined_type(ty))
    }

    /// tsc-port: isExpandoPropertyDeclaration @6.0.3
    /// tsc-hash: e024c7cc528f204e53cddc596e1c0868a39641128c07a23886290b97d61bd188
    /// tsc-span: _tsc.js:19363-19367
    fn emit_is_expando_property_declaration(&self, declaration: Option<NodeId>) -> bool {
        declaration.is_some_and(|declaration| {
            matches!(
                self.kind_of(declaration),
                SyntaxKind::PropertyAccessExpression
                    | SyntaxKind::ElementAccessExpression
                    | SyntaxKind::BinaryExpression
            )
        })
    }

    /// tsc-port: isExpandoFunctionDeclaration @6.0.3
    /// tsc-hash: ed2afab33ef4b7bc2c878b2943e881dedafba9030fa884dbf9155c3578fe9554
    /// tsc-span: _tsc.js:88090-88112
    pub(crate) fn emit_is_expando_function_declaration(
        &mut self,
        node: NodeId,
    ) -> CheckResult<bool> {
        if !self.emit_is_parse_tree_node(node) {
            return Ok(false);
        }
        let declaration = match self.kind_of(node) {
            SyntaxKind::FunctionDeclaration | SyntaxKind::VariableDeclaration => node,
            _ => return Ok(false),
        };
        let symbol = if self.kind_of(declaration) == SyntaxKind::VariableDeclaration {
            let source = self.binder.source_of_node(declaration);
            let (has_type, initializer) = match self.data_of(declaration) {
                NodeData::VariableDeclaration(data) => (
                    data.r#type.is_some(),
                    tsc_binder::assignment::get_declared_expando_initializer(source, declaration),
                ),
                _ => (false, None),
            };
            if has_type
                || (!self.is_in_js_file(declaration) && !self.is_var_const_like(declaration))
            {
                return Ok(false);
            }
            let Some(initializer) = initializer else {
                return Ok(false);
            };
            if self.node_symbol(initializer).is_none() {
                return Ok(false);
            }
            self.get_symbol_of_declaration(initializer)?
        } else {
            if self.node_symbol(declaration).is_none() {
                return Ok(false);
            }
            self.get_symbol_of_declaration(declaration)?
        };
        if symbol == self.unknown_symbol
            || !self
                .symbol_flags(symbol)
                .intersects(SymbolFlags::FUNCTION | SymbolFlags::VARIABLE)
        {
            return Ok(false);
        }
        let exports = self.get_exports_of_symbol(symbol)?;
        for &property in exports.values() {
            let property_data = self.binder.symbol(property);
            if property_data.flags.intersects(SymbolFlags::VALUE)
                && self.emit_is_expando_property_declaration(property_data.value_declaration)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsc-port: getPropertiesOfContainerFunction @6.0.3
    /// tsc-hash: a194d2629e8413c8dd13b4a67567fdb2dba8a6a09f563e8b705d9488c0e588a3
    /// tsc-span: _tsc.js:88113-88120
    pub(crate) fn emit_get_properties_of_container_function(
        &mut self,
        node: NodeId,
        session_token: u64,
    ) -> CheckResult<Vec<EmitFunctionProperty>> {
        if !self.emit_is_parse_tree_node(node)
            || self.kind_of(node) != SyntaxKind::FunctionDeclaration
        {
            return Ok(Vec::new());
        }
        let Some(_) = self.node_symbol(node) else {
            return Ok(Vec::new());
        };
        let container_symbol = self.get_symbol_of_declaration(node)?;
        if container_symbol == self.unknown_symbol {
            return Ok(Vec::new());
        }
        let ty = self.get_type_of_symbol(container_symbol)?;
        let properties = self.get_properties_of_type(ty)?;
        let parent = EmitResolverSymbol {
            session_token,
            symbol_index: container_symbol.0,
        };
        let mut result = Vec::with_capacity(properties.len());
        for property in properties {
            let property_data = self.binder.symbol(property);
            let value_declaration = property_data
                .value_declaration
                .filter(|&declaration| self.emit_is_parse_tree_node(declaration))
                .map(|declaration| self.declaration_emit_resolver_node(declaration));
            result.push(EmitFunctionProperty {
                name: property_data.escaped_name.clone(),
                symbol: EmitResolverSymbol {
                    session_token,
                    symbol_index: property.0,
                },
                parent,
                value_declaration,
            });
        }
        Ok(result)
    }

    /// tsc-port: isLiteralConstDeclaration @6.0.3
    /// tsc-hash: 1c1cef46271c6fce5e62c4307e8471561e2f10782682ef7dfff2a10013b1f1d6
    /// tsc-span: _tsc.js:88485-88490
    pub(crate) fn emit_is_literal_const_declaration(&mut self, node: NodeId) -> CheckResult<bool> {
        let is_literal_const = self.is_declaration_readonly(node)
            || self.kind_of(node) == SyntaxKind::VariableDeclaration
                && self.is_var_const_like(node);
        if !is_literal_const {
            return Ok(false);
        }
        let symbol = self.get_symbol_of_declaration(node)?;
        if symbol == self.unknown_symbol {
            return Ok(false);
        }
        let ty = self.get_type_of_symbol(symbol)?;
        Ok(self.tables.is_fresh_literal_type(ty))
    }

    /// tsc-port: isLateBound @6.0.3
    /// tsc-hash: d11842db30b0440c571390f2deed480c5a03d4e45ef954902841c3178c938112
    /// tsc-span: _tsc.js:88600-88604
    pub(crate) fn emit_is_late_bound(&mut self, node: NodeId) -> CheckResult<bool> {
        if !self.emit_is_parse_tree_node(node)
            || !node_util::is_declaration(self.binder.source_of_node(node), node)
        {
            return Ok(false);
        }
        let Some(_) = self.node_symbol(node) else {
            return Ok(false);
        };
        let symbol = self.get_symbol_of_declaration(node)?;
        Ok(symbol != self.unknown_symbol
            && self.get_check_flags(symbol).intersects(CheckFlags::LATE))
    }

    /// tsc-port: isImportRequiredByAugmentation @6.0.3
    /// tsc-hash: 7498ec7545df67711e0cdeb1967852804859c42964ae9f0f61444d3ca2c3124c
    /// tsc-span: _tsc.js:88696-88717
    pub(crate) fn emit_is_import_required_by_augmentation(
        &mut self,
        node: NodeId,
    ) -> CheckResult<bool> {
        let source_root = self.binder.source_of_node(node).root;
        let Some(file_symbol) = self.node_symbol(source_root) else {
            return Ok(false);
        };
        let Some(import_target) = self.get_external_module_file_from_declaration(node)? else {
            return Ok(false);
        };
        if import_target == source_root {
            return Ok(false);
        }
        let exports = self.get_exports_of_module(file_symbol)?;
        for &exported in exports.values() {
            // tsc's `s.mergeId` is represented by membership in the
            // merged-symbol source-key table. Do not treat a merge target
            // alone as a recorded source key.
            if !self.merged_symbols.contains_key(&exported) {
                continue;
            }
            let merged = self.get_merged_symbol(exported);
            let declarations = self.binder.symbol(merged).declarations.clone();
            if declarations
                .into_iter()
                .any(|declaration| self.binder.source_of_node(declaration).root == import_target)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn emit_is_parse_tree_node(&self, node: NodeId) -> bool {
        !node_util::node_flags(self.binder.source_of_node(node), node)
            .intersects(NodeFlags::SYNTHESIZED)
    }

    /// tsc-port: isDeclarationVisible @6.0.3
    /// tsc-hash: b569e8243cf2db9de0dbec7462f29fa1e70f4b94405adb5a134b6571d4c8fbeb
    /// tsc-span: _tsc.js:55589-55674
    ///
    /// This emit-only kind walk recurses through this memoizing worker so
    /// every declaration visited by the decision gets its own
    /// NodeLinks.isVisible write. The display slice remains read-only.
    pub(crate) fn emit_is_declaration_visible(&mut self, declaration: NodeId) -> CheckResult<bool> {
        let _replay_call = DeclarationReplayCallGuard::enter("resolver.isDeclarationVisible");
        if let Some(visible) = self.links.node(declaration).is_visible {
            return Ok(visible);
        }
        let visible = self.emit_determine_declaration_is_visible(declaration)?;
        self.links
            .set_node_is_visible(self.speculation_depth, declaration, visible);
        record_declaration_replay_visibility_write(declaration, visible);
        Ok(self.links.node(declaration).is_visible.unwrap_or(visible))
    }

    fn emit_determine_declaration_is_visible(&mut self, declaration: NodeId) -> CheckResult<bool> {
        match self.kind_of(declaration) {
            SyntaxKind::JSDocCallbackTag
            | SyntaxKind::JSDocTypedefTag
            | SyntaxKind::JSDocEnumTag => Ok(self
                .parent_of(declaration)
                .and_then(|parent| self.parent_of(parent))
                .and_then(|parent| self.parent_of(parent))
                .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::SourceFile)),
            SyntaxKind::BindingElement => {
                let parent = self
                    .parent_of(declaration)
                    .and_then(|parent| self.parent_of(parent));
                match parent {
                    Some(parent) => self.emit_is_declaration_visible(parent),
                    None => Ok(false),
                }
            }
            SyntaxKind::VariableDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::ImportEqualsDeclaration => {
                let source = self.binder.source_of_node(declaration);
                if self.kind_of(declaration) == SyntaxKind::VariableDeclaration {
                    let empty_pattern = match self.data_of(declaration) {
                        NodeData::VariableDeclaration(data) => {
                            data.name.is_some_and(|name| match self.data_of(name) {
                                NodeData::ObjectBindingPattern(data) => {
                                    self.nodes_of(data.elements).is_empty()
                                }
                                NodeData::ArrayBindingPattern(data) => {
                                    self.nodes_of(data.elements).is_empty()
                                }
                                _ => false,
                            })
                        }
                        _ => false,
                    };
                    if empty_pattern {
                        return Ok(false);
                    }
                }
                if node_util::is_ambient_module(source, declaration)
                    && node_util::is_module_augmentation_external(source, declaration)
                {
                    return Ok(true);
                }
                let Some(container) = self.declaration_emit_declaration_container(declaration)
                else {
                    return Ok(false);
                };
                let exported = node_util::get_combined_modifier_flags(source, declaration)
                    .intersects(ModifierFlags::EXPORT);
                let ambient_nested = self.kind_of(declaration)
                    != SyntaxKind::ImportEqualsDeclaration
                    && self.kind_of(container) != SyntaxKind::SourceFile
                    && self.node_flags(container) & NodeFlags::AMBIENT.bits() != 0;
                if !exported && !ambient_nested {
                    return Ok(self.kind_of(container) == SyntaxKind::SourceFile
                        && !self
                            .binder
                            .is_external_or_common_js_module_of_node(container));
                }
                self.emit_is_declaration_visible(container)
            }
            SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature => {
                let source = self.binder.source_of_node(declaration);
                if node_util::get_effective_modifier_flags(source, declaration)
                    .intersects(ModifierFlags::PRIVATE | ModifierFlags::PROTECTED)
                {
                    return Ok(false);
                }
                match self.parent_of(declaration) {
                    Some(parent) => self.emit_is_declaration_visible(parent),
                    None => Ok(false),
                }
            }
            SyntaxKind::Constructor
            | SyntaxKind::ConstructSignature
            | SyntaxKind::CallSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::Parameter
            | SyntaxKind::ModuleBlock
            | SyntaxKind::FunctionType
            | SyntaxKind::ConstructorType
            | SyntaxKind::TypeLiteral
            | SyntaxKind::TypeReference
            | SyntaxKind::ArrayType
            | SyntaxKind::TupleType
            | SyntaxKind::UnionType
            | SyntaxKind::IntersectionType
            | SyntaxKind::ParenthesizedType
            | SyntaxKind::NamedTupleMember => match self.parent_of(declaration) {
                Some(parent) => self.emit_is_declaration_visible(parent),
                None => Ok(false),
            },
            SyntaxKind::TypeParameter
            | SyntaxKind::SourceFile
            | SyntaxKind::NamespaceExportDeclaration => Ok(true),
            SyntaxKind::ImportClause
            | SyntaxKind::NamespaceImport
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::ExportAssignment => Ok(false),
            _ => Ok(false),
        }
    }

    /// tsc-port: getDeclarationContainer @6.0.3
    /// tsc-hash: 3d4b993da842ea191877ffad47fb0c8045a3d1086066350235a4992e74413283
    /// tsc-span: _tsc.js:55784-55798
    fn declaration_emit_declaration_container(&self, declaration: NodeId) -> Option<NodeId> {
        let source = self.binder.source_of_node(declaration);
        let root = node_util::get_root_declaration(source, declaration);
        let mut current = Some(root);
        while let Some(node) = current {
            match self.kind_of(node) {
                SyntaxKind::VariableDeclaration
                | SyntaxKind::VariableDeclarationList
                | SyntaxKind::ImportSpecifier
                | SyntaxKind::NamedImports
                | SyntaxKind::NamespaceImport
                | SyntaxKind::ImportClause => current = self.parent_of(node),
                _ => return self.parent_of(node),
            }
        }
        None
    }

    /// tsc-port: hasVisibleDeclarations @6.0.3
    /// tsc-hash: 3a5941173ae711a2e4bd9bf466cb674b521a0cb7cbd8c97fffdd7ac817dc4a6b
    /// tsc-span: _tsc.js:50544-50594
    pub(crate) fn has_visible_declarations_with_aliases(
        &mut self,
        symbol: SymbolId,
        should_compute_aliases_to_make_visible: bool,
    ) -> CheckResult<Option<EmitSymbolAccessibilityResult>> {
        let symbol_flags = self.binder.symbol(symbol).flags;
        let declarations = self.binder.symbol(symbol).declarations.clone();
        let mut aliases_to_make_visible = Vec::new();
        for declaration in declarations {
            if self.kind_of(declaration) == SyntaxKind::Identifier {
                continue;
            }
            if !self.declaration_is_visible_or_alias(
                symbol_flags,
                declaration,
                should_compute_aliases_to_make_visible,
                &mut aliases_to_make_visible,
            )? {
                return Ok(None);
            }
        }
        // The upstream object literal always owns the
        // `aliasesToMakeVisible` property on this success path. When no
        // alias was appended its value is `undefined`; schema-2 records that
        // present property as a present empty array, distinct from result
        // shapes which omit the property entirely.
        Ok(Some(
            self.accessibility_result(
                EmitSymbolAccessibility::Accessible,
                Some(
                    aliases_to_make_visible
                        .into_iter()
                        .map(|node| self.declaration_emit_resolver_node(node))
                        .collect(),
                ),
                None,
                None,
                None,
            ),
        ))
    }

    fn declaration_is_visible_or_alias(
        &mut self,
        symbol_flags: SymbolFlags,
        declaration: NodeId,
        should_compute_aliases_to_make_visible: bool,
        aliases_to_make_visible: &mut Vec<NodeId>,
    ) -> CheckResult<bool> {
        if self.emit_is_declaration_visible(declaration)? {
            return Ok(true);
        }

        let source = self.binder.source_of_node(declaration);
        if let Some(import_syntax) = self.declaration_emit_any_import_syntax(declaration) {
            if !node_util::has_syntactic_modifier(source, import_syntax, ModifierFlags::EXPORT) {
                if let Some(parent) = self.parent_of(import_syntax) {
                    if self.emit_is_declaration_visible(parent)? {
                        self.add_visible_alias(
                            declaration,
                            import_syntax,
                            should_compute_aliases_to_make_visible,
                            aliases_to_make_visible,
                        );
                        return Ok(true);
                    }
                }
            }
        }

        if self.kind_of(declaration) == SyntaxKind::VariableDeclaration {
            let variable_statement = self
                .parent_of(declaration)
                .and_then(|parent| self.parent_of(parent));
            if let Some(variable_statement) = variable_statement {
                if self.kind_of(variable_statement) == SyntaxKind::VariableStatement
                    && !node_util::has_syntactic_modifier(
                        source,
                        variable_statement,
                        ModifierFlags::EXPORT,
                    )
                {
                    if let Some(parent) = self.parent_of(variable_statement) {
                        if self.emit_is_declaration_visible(parent)? {
                            self.add_visible_alias(
                                declaration,
                                variable_statement,
                                should_compute_aliases_to_make_visible,
                                aliases_to_make_visible,
                            );
                            return Ok(true);
                        }
                    }
                }
            }
        }

        if self.declaration_emit_is_late_visibility_painted_statement(declaration)
            && !node_util::has_syntactic_modifier(source, declaration, ModifierFlags::EXPORT)
        {
            if let Some(parent) = self.parent_of(declaration) {
                if self.emit_is_declaration_visible(parent)? {
                    self.add_visible_alias(
                        declaration,
                        declaration,
                        should_compute_aliases_to_make_visible,
                        aliases_to_make_visible,
                    );
                    return Ok(true);
                }
            }
        }

        if self.kind_of(declaration) != SyntaxKind::BindingElement {
            return Ok(false);
        }

        if symbol_flags.intersects(SymbolFlags::ALIAS) && self.is_in_js_file(declaration) {
            let variable_declaration = self
                .parent_of(declaration)
                .and_then(|parent| self.parent_of(parent));
            let variable_statement = variable_declaration.and_then(|variable| {
                self.parent_of(variable)
                    .and_then(|parent| self.parent_of(parent))
            });
            let js_alias_shape = variable_declaration
                .is_some_and(|variable| self.kind_of(variable) == SyntaxKind::VariableDeclaration)
                && variable_statement.is_some_and(|statement| {
                    self.kind_of(statement) == SyntaxKind::VariableStatement
                        && !node_util::has_syntactic_modifier(
                            source,
                            statement,
                            ModifierFlags::EXPORT,
                        )
                });
            if js_alias_shape {
                if let Some(parent) =
                    variable_statement.and_then(|statement| self.parent_of(statement))
                {
                    if self.emit_is_declaration_visible(parent)? {
                        self.add_visible_alias(
                            declaration,
                            variable_statement.expect("guarded variable statement"),
                            should_compute_aliases_to_make_visible,
                            aliases_to_make_visible,
                        );
                        return Ok(true);
                    }
                }
            }
        }

        if symbol_flags.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE) {
            let Some(root) = node_util::walk_up_binding_elements_and_patterns(source, declaration)
            else {
                return Ok(false);
            };
            if self.kind_of(root) == SyntaxKind::Parameter {
                return Ok(false);
            }
            let Some(variable_statement) = self
                .parent_of(root)
                .and_then(|parent| self.parent_of(parent))
            else {
                return Ok(false);
            };
            if self.kind_of(variable_statement) != SyntaxKind::VariableStatement {
                return Ok(false);
            }
            if node_util::has_syntactic_modifier(source, variable_statement, ModifierFlags::EXPORT)
            {
                return Ok(true);
            }
            let Some(parent) = self.parent_of(variable_statement) else {
                return Ok(false);
            };
            if !self.emit_is_declaration_visible(parent)? {
                return Ok(false);
            }
            self.add_visible_alias(
                declaration,
                variable_statement,
                should_compute_aliases_to_make_visible,
                aliases_to_make_visible,
            );
            return Ok(true);
        }

        Ok(false)
    }

    /// tsc-port: addVisibleAlias @6.0.3
    /// tsc-hash: 2b3863dfe35f81d08fb5ccc664c99206931b2b8dd37e469e73ec7ec6ee499c9c
    /// tsc-span: _tsc.js:50587-50593
    fn add_visible_alias(
        &mut self,
        declaration: NodeId,
        aliasing_statement: NodeId,
        should_compute_aliases_to_make_visible: bool,
        aliases_to_make_visible: &mut Vec<NodeId>,
    ) {
        if !should_compute_aliases_to_make_visible {
            return;
        }
        self.links
            .set_node_is_visible(self.speculation_depth, declaration, true);
        record_declaration_replay_visibility_write(declaration, true);
        if !aliases_to_make_visible.contains(&aliasing_statement) {
            aliases_to_make_visible.push(aliasing_statement);
        }
    }

    /// tsc-port: isAnySymbolAccessible @6.0.3
    /// tsc-hash: 196ddf5926730f5e6f16ff4f2a7d59e1abf506c39cfc64d9ff90bd1a065f6cb1
    /// tsc-span: _tsc.js:50450-50498
    pub(crate) fn is_any_symbol_accessible(
        &mut self,
        symbols: &[SymbolId],
        enclosing_declaration: NodeId,
        initial_symbol: SymbolId,
        meaning: SymbolFlags,
        should_compute_aliases_to_make_visible: bool,
        allow_modules: bool,
    ) -> CheckResult<Option<EmitSymbolAccessibilityResult>> {
        if symbols.is_empty() {
            return Ok(None);
        }
        let mut had_accessible_chain = None;
        let mut early_module_bail = false;
        for &symbol in symbols {
            // JSDoc template parameters are lexical declarations in tsc's
            // class/interface Type-member scope. The shared display slice
            // intentionally omits that table, so recover the exact direct
            // chain through the ordinary enclosing-name lookup before using
            // the shared chain worker. Without this arm the container walk
            // incorrectly substitutes the owning class and paints it.
            let symbol_flags = self.binder.symbol(symbol).flags;
            let directly_accessible_type_parameter =
                if symbol_flags.intersects(SymbolFlags::TYPE_PARAMETER) {
                    let name = self.binder.symbol(symbol).escaped_name.clone();
                    self.resolve_name(
                        Some(enclosing_declaration),
                        &name,
                        meaning,
                        /*name_not_found_message*/ None,
                        /*is_use*/ false,
                        /*exclude_globals*/ false,
                    )?
                    .is_some_and(|resolved| self.get_merged_symbol(resolved) == symbol)
                } else {
                    false
                };
            let accessible_symbol_chain = if directly_accessible_type_parameter {
                Some(vec![symbol])
            } else {
                self.declaration_emit_accessible_symbol_chain(
                    symbol,
                    meaning,
                    Some(enclosing_declaration),
                )?
            };
            if let Some(accessible_symbol_chain) = accessible_symbol_chain {
                had_accessible_chain = Some(symbol);
                if let Some(result) = self.has_visible_declarations_with_aliases(
                    accessible_symbol_chain[0],
                    should_compute_aliases_to_make_visible,
                )? {
                    return Ok(Some(result));
                }
            }

            if allow_modules
                && self
                    .binder
                    .symbol(symbol)
                    .declarations
                    .iter()
                    .copied()
                    .any(|declaration| {
                        self.has_non_global_augmentation_external_module_symbol(declaration)
                    })
            {
                if should_compute_aliases_to_make_visible {
                    early_module_bail = true;
                    continue;
                }
                return Ok(Some(self.accessible_result()));
            }

            let containers = self.declaration_emit_containers_of_symbol(
                symbol,
                Some(enclosing_declaration),
                meaning,
            )?;
            let parent_meaning = if initial_symbol == symbol {
                Self::declaration_emit_qualified_left_meaning(meaning)
            } else {
                meaning
            };
            if let Some(result) = self.is_any_symbol_accessible(
                &containers,
                enclosing_declaration,
                initial_symbol,
                parent_meaning,
                should_compute_aliases_to_make_visible,
                allow_modules,
            )? {
                return Ok(Some(result));
            }
        }

        if early_module_bail {
            return Ok(Some(self.accessible_result()));
        }
        if let Some(had_accessible_chain) = had_accessible_chain {
            return Ok(Some(
                self.accessibility_result(
                    EmitSymbolAccessibility::NotAccessible,
                    None,
                    Some(self.symbol_display_name(initial_symbol)),
                    (had_accessible_chain != initial_symbol)
                        .then(|| self.symbol_display_name(had_accessible_chain)),
                    None,
                ),
            ));
        }
        Ok(None)
    }

    /// tsc-port: isSymbolAccessible @6.0.3
    /// tsc-hash: 9235fa70af1dbdc19b0a98214131caf0ac9eb80fa208e9af814c4740abb1fd6f
    /// tsc-span: _tsc.js:50499-50508
    pub(crate) fn emit_is_symbol_accessible(
        &mut self,
        symbol: SymbolId,
        enclosing_declaration: NodeId,
        meaning: EmitSymbolMeaning,
        should_compute_aliases_to_make_visible: bool,
    ) -> CheckResult<EmitSymbolAccessibilityResult> {
        let _replay_call = DeclarationReplayCallGuard::enter("resolver.isSymbolAccessible");
        self.is_symbol_accessible_worker(
            Some(symbol),
            Some(enclosing_declaration),
            Self::symbol_flags_from_emit_meaning(meaning),
            should_compute_aliases_to_make_visible,
            /*allow_modules*/ true,
        )
    }

    /// tsc-port: isSymbolAccessibleWorker @6.0.3
    /// tsc-hash: 4fee32d2060129fdfc29d3a6fa609ff0833ad395201fe036f155bd6b73df5a6b
    /// tsc-span: _tsc.js:50509-50533
    ///
    /// Error names intentionally use merge.rs's simple symbol display. They
    /// are SHADOW semantics until the m-3.5 full symbolToString byte gate;
    /// accessibility, alias ordering, module identity, and error-node
    /// selection are authoritative here.
    pub(crate) fn is_symbol_accessible_worker(
        &mut self,
        symbol: Option<SymbolId>,
        enclosing_declaration: Option<NodeId>,
        meaning: SymbolFlags,
        should_compute_aliases_to_make_visible: bool,
        allow_modules: bool,
    ) -> CheckResult<EmitSymbolAccessibilityResult> {
        let (Some(symbol), Some(enclosing_declaration)) = (symbol, enclosing_declaration) else {
            return Ok(self.accessible_result());
        };
        if let Some(result) = self.is_any_symbol_accessible(
            &[symbol],
            enclosing_declaration,
            symbol,
            meaning,
            should_compute_aliases_to_make_visible,
            allow_modules,
        )? {
            return Ok(result);
        }

        let declarations = self.binder.symbol(symbol).declarations.clone();
        let mut symbol_external_module = None;
        for declaration in declarations {
            if let Some(container) = self.get_external_module_container(declaration)? {
                symbol_external_module = Some(container);
                break;
            }
        }
        if let Some(symbol_external_module) = symbol_external_module {
            let enclosing_external_module =
                self.get_external_module_container(enclosing_declaration)?;
            if Some(symbol_external_module) != enclosing_external_module {
                return Ok(self.accessibility_result(
                    EmitSymbolAccessibility::CannotBeNamed,
                    None,
                    Some(self.symbol_display_name(symbol)),
                    Some(self.symbol_display_name(symbol_external_module)),
                    self.is_in_js_file(enclosing_declaration)
                        .then(|| self.declaration_emit_resolver_node(enclosing_declaration)),
                ));
            }
        }

        Ok(self.accessibility_result(
            EmitSymbolAccessibility::NotAccessible,
            None,
            Some(self.symbol_display_name(symbol)),
            None,
            None,
        ))
    }

    /// tsc-port: getExternalModuleContainer @6.0.3
    /// tsc-hash: bf570958a67d853f0096fccf9878ce00e4ea91aaa57d1e775d8c280d858b5bcf
    /// tsc-span: _tsc.js:50534-50543
    pub(crate) fn get_external_module_container(
        &mut self,
        declaration: NodeId,
    ) -> CheckResult<Option<SymbolId>> {
        let mut current = Some(declaration);
        while let Some(node) = current {
            if self.has_external_module_symbol(node) {
                if self.node_symbol(node).is_none() {
                    return Ok(None);
                }
                return self.get_symbol_of_declaration(node).map(Some);
            }
            current = self.parent_of(node);
        }
        Ok(None)
    }

    fn has_external_module_symbol(&self, declaration: NodeId) -> bool {
        node_util::is_ambient_module(self.binder.source_of_node(declaration), declaration)
            || self.kind_of(declaration) == SyntaxKind::SourceFile
                && self
                    .binder
                    .is_external_or_common_js_module_of_node(declaration)
    }

    fn has_non_global_augmentation_external_module_symbol(&self, declaration: NodeId) -> bool {
        match self.data_of(declaration) {
            NodeData::ModuleDeclaration(data) => data
                .name
                .is_some_and(|name| self.kind_of(name) == SyntaxKind::StringLiteral),
            NodeData::SourceFile(_) => self
                .binder
                .is_external_or_common_js_module_of_node(declaration),
            _ => false,
        }
    }

    /// tsc-port: getMeaningOfEntityNameReference @6.0.3
    /// tsc-hash: 8784944e5eb1e33d5e3df2aa701fc3275e8eb6f0b0608b0ebf355177a2e6f8e2
    /// tsc-span: _tsc.js:50595-50605
    pub(crate) fn get_meaning_of_entity_name_reference(
        &self,
        entity_name: NodeId,
    ) -> EmitSymbolMeaning {
        let parent = self.parent_of(entity_name);
        let value_meaning = parent.is_some_and(|parent| {
            self.kind_of(parent) == SyntaxKind::TypeQuery
                || self.kind_of(parent) == SyntaxKind::ExpressionWithTypeArguments
                    && !self.is_part_of_type_node(parent)
                || self.kind_of(parent) == SyntaxKind::ComputedPropertyName
                || matches!(
                    self.data_of(parent),
                    NodeData::TypePredicate(data)
                        if data.parameter_name == Some(entity_name)
                )
        });
        if value_meaning {
            return EmitSymbolMeaning::VALUE_EXPORT_VALUE;
        }

        let namespace_meaning = matches!(
            self.kind_of(entity_name),
            SyntaxKind::QualifiedName | SyntaxKind::PropertyAccessExpression
        ) || parent.is_some_and(|parent| {
            self.kind_of(parent) == SyntaxKind::ImportEqualsDeclaration
                || matches!(
                    self.data_of(parent),
                    NodeData::QualifiedName(data) if data.left == Some(entity_name)
                )
                || matches!(
                    self.data_of(parent),
                    NodeData::PropertyAccessExpression(data)
                        if data.expression == Some(entity_name)
                )
                || matches!(
                    self.data_of(parent),
                    NodeData::ElementAccessExpression(data)
                        if data.expression == Some(entity_name)
                )
        });
        if namespace_meaning {
            EmitSymbolMeaning::NAMESPACE
        } else {
            EmitSymbolMeaning::TYPE
        }
    }

    /// tsc-port: isEntityNameVisible @6.0.3
    /// tsc-hash: 060c123c45cc5190b222fb3e1170d371492ca672405b04b4283dca7f7b5d8369
    /// tsc-span: _tsc.js:50606-50648
    pub(crate) fn emit_is_entity_name_visible(
        &mut self,
        entity_name: NodeId,
        enclosing_declaration: NodeId,
        should_compute_aliases_to_make_visible: bool,
    ) -> CheckResult<EmitSymbolAccessibilityResult> {
        let _replay_call = DeclarationReplayCallGuard::enter("resolver.isEntityNameVisible");
        let emit_meaning = self.get_meaning_of_entity_name_reference(entity_name);
        let meaning = Self::symbol_flags_from_emit_meaning(emit_meaning);
        let first_identifier = self.first_identifier(entity_name);
        let resolution_name = match self.identifier_text_of(first_identifier) {
            Some(name) => name.to_owned(),
            None => self.text_of_node(first_identifier)?,
        };
        let symbol = self.resolve_name(
            Some(enclosing_declaration),
            &resolution_name,
            meaning,
            /*name_not_found_message*/ None,
            /*is_use*/ false,
            /*exclude_globals*/ false,
        )?;

        if symbol.is_some_and(|symbol| {
            self.binder
                .symbol(symbol)
                .flags
                .intersects(SymbolFlags::TYPE_PARAMETER)
                && meaning.intersects(SymbolFlags::TYPE)
        }) {
            return Ok(self.accessible_result());
        }

        if symbol.is_none() && self.is_this_identifier(first_identifier) {
            let container = crate::expr::get_this_container_full(
                self,
                first_identifier,
                /*include_arrow_functions*/ false,
                /*include_class_computed_property_name*/ false,
            );
            let container_symbol = if let Some(container) = container {
                if self.node_symbol(container).is_some() {
                    Some(self.get_symbol_of_declaration(container)?)
                } else {
                    None
                }
            } else {
                None
            };
            if self
                .is_symbol_accessible_worker(
                    container_symbol,
                    Some(first_identifier),
                    meaning,
                    /*should_compute_aliases_to_make_visible*/ false,
                    /*allow_modules*/ true,
                )?
                .accessibility
                == EmitSymbolAccessibility::Accessible
            {
                return Ok(self.accessible_result());
            }
        }

        let Some(symbol) = symbol else {
            let error_name = self.text_of_node(first_identifier)?;
            return Ok(self.accessibility_result(
                EmitSymbolAccessibility::NotResolved,
                None,
                Some(error_name),
                None,
                Some(self.declaration_emit_resolver_node(first_identifier)),
            ));
        };

        if let Some(result) = self
            .has_visible_declarations_with_aliases(symbol, should_compute_aliases_to_make_visible)?
        {
            return Ok(result);
        }
        let error_name = self.text_of_node(first_identifier)?;
        Ok(self.accessibility_result(
            EmitSymbolAccessibility::NotAccessible,
            None,
            Some(error_name),
            None,
            Some(self.declaration_emit_resolver_node(first_identifier)),
        ))
    }

    /// tsc-port: collectLinkedAliases @6.0.3
    /// tsc-hash: 8fe011e257a2763196e5bd485d330cf0df070bbdf96d1d78fd9edf54c0f391c5
    /// tsc-span: _tsc.js:55675-55727
    ///
    /// `set_visibility=true` is used by the two check-phase callers. The
    /// ordered collection branch is deliberately production-dormant until
    /// H2.7b, but remains complete and crate-visible for focused replay.
    pub(crate) fn collect_linked_aliases(
        &mut self,
        node: NodeId,
        set_visibility: bool,
    ) -> CheckResult<Option<Vec<NodeId>>> {
        let parent = self.parent_of(node);
        let export_symbol = if self.kind_of(node) != SyntaxKind::StringLiteral
            && parent.is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ExportAssignment)
        {
            let name = match self.identifier_text_of(node) {
                Some(name) => name.to_owned(),
                None => self.text_of_node(node)?,
            };
            self.resolve_name(
                Some(node),
                &name,
                Self::symbol_flags_from_emit_meaning(EmitSymbolMeaning::ALIAS_RESOLVE),
                /*name_not_found_message*/ None,
                /*is_use*/ false,
                /*exclude_globals*/ false,
            )?
        } else if parent.is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ExportSpecifier) {
            self.get_target_of_export_specifier(
                parent.expect("guarded export specifier"),
                Self::symbol_flags_from_emit_meaning(EmitSymbolMeaning::ALIAS_RESOLVE),
                /*dont_resolve_alias*/ false,
            )?
        } else {
            None
        };

        let Some(export_symbol) = export_symbol else {
            return Ok(None);
        };
        let mut visited = HashSet::new();
        visited.insert(export_symbol);
        let mut result = None;
        let declarations = self.binder.symbol(export_symbol).declarations.clone();
        self.build_visible_node_list(&declarations, set_visibility, &mut visited, &mut result)?;
        Ok(result)
    }

    fn build_visible_node_list(
        &mut self,
        declarations: &[NodeId],
        set_visibility: bool,
        visited: &mut HashSet<SymbolId>,
        result: &mut Option<Vec<NodeId>>,
    ) -> CheckResult<()> {
        for &declaration in declarations {
            let result_node = self
                .declaration_emit_any_import_syntax(declaration)
                .unwrap_or(declaration);
            if set_visibility {
                self.links
                    .set_node_is_visible(self.speculation_depth, declaration, true);
                record_declaration_replay_visibility_write(declaration, true);
            } else {
                let result = result.get_or_insert_with(Vec::new);
                if !result.contains(&result_node) {
                    result.push(result_node);
                }
            }

            if !self.is_internal_module_import_equals_declaration(declaration) {
                continue;
            }
            let module_reference = match self.data_of(declaration) {
                NodeData::ImportEqualsDeclaration(data) => data.module_reference,
                _ => None,
            };
            let Some(module_reference) = module_reference else {
                continue;
            };
            let first_identifier = self.first_identifier(module_reference);
            let name = match self.identifier_text_of(first_identifier) {
                Some(name) => name.to_owned(),
                None => self.text_of_node(first_identifier)?,
            };
            let import_symbol = self.resolve_name(
                Some(declaration),
                &name,
                Self::symbol_flags_from_emit_meaning(EmitSymbolMeaning::IMPORT_EQUALS_RESOLVE),
                /*name_not_found_message*/ None,
                /*is_use*/ false,
                /*exclude_globals*/ false,
            )?;
            if let Some(import_symbol) = import_symbol {
                if visited.insert(import_symbol) {
                    let declarations = self.binder.symbol(import_symbol).declarations.clone();
                    self.build_visible_node_list(&declarations, set_visibility, visited, result)?;
                }
            }
        }
        Ok(())
    }

    fn declaration_emit_any_import_syntax(&self, declaration: NodeId) -> Option<NodeId> {
        match self.kind_of(declaration) {
            SyntaxKind::ImportEqualsDeclaration => Some(declaration),
            SyntaxKind::ImportClause => self.parent_of(declaration),
            SyntaxKind::NamespaceImport => self
                .parent_of(declaration)
                .and_then(|parent| self.parent_of(parent)),
            SyntaxKind::ImportSpecifier => self
                .parent_of(declaration)
                .and_then(|parent| self.parent_of(parent))
                .and_then(|parent| self.parent_of(parent)),
            _ => None,
        }
    }

    fn declaration_emit_is_late_visibility_painted_statement(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::ImportDeclaration
                | SyntaxKind::ImportEqualsDeclaration
                | SyntaxKind::VariableStatement
                | SyntaxKind::ClassDeclaration
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::ModuleDeclaration
                | SyntaxKind::TypeAliasDeclaration
                | SyntaxKind::InterfaceDeclaration
                | SyntaxKind::EnumDeclaration
        )
    }

    fn declaration_emit_qualified_left_meaning(meaning: SymbolFlags) -> SymbolFlags {
        if meaning == SymbolFlags::VALUE {
            SymbolFlags::VALUE
        } else {
            SymbolFlags::NAMESPACE
        }
    }

    fn symbol_flags_from_emit_meaning(meaning: EmitSymbolMeaning) -> SymbolFlags {
        SymbolFlags::from_bits(meaning.0 as i32)
    }

    fn accessible_result(&self) -> EmitSymbolAccessibilityResult {
        self.accessibility_result(EmitSymbolAccessibility::Accessible, None, None, None, None)
    }

    fn accessibility_result(
        &self,
        accessibility: EmitSymbolAccessibility,
        aliases_to_make_visible: Option<Vec<EmitResolverNode>>,
        error_symbol_name: Option<String>,
        error_module_name: Option<String>,
        error_node: Option<EmitResolverNode>,
    ) -> EmitSymbolAccessibilityResult {
        EmitSymbolAccessibilityResult {
            accessibility,
            aliases_to_make_visible,
            error_symbol_name,
            error_module_name,
            error_node,
        }
    }

    fn declaration_emit_resolver_node(&self, node: NodeId) -> EmitResolverNode {
        let file_index = self.binder.file_index_of_node(node);
        let source = if self.authoritative_source_tokens.is_empty() {
            u32::try_from(file_index).expect("checker file index exceeds SourceFileId")
        } else {
            self.authoritative_source_tokens
                .get(file_index)
                .expect("authoritative metadata covers every checker file")
                .0
        };
        EmitResolverNode::from_raw_source(source, node)
    }
}

// The P4 replay lives in the compiler integration target, where `cfg(test)`
// is not set for this dependency. Keep the hook inert unless that target
// installs one owned request on its current thread. The production checker
// takes the same path and observes only the empty TLS slot.
thread_local! {
    static DECLARATION_REPLAY_PENDING: RefCell<Option<DeclarationReplayPending>> = const { RefCell::new(None) };
    static DECLARATION_REPLAY_CAPTURE: RefCell<Option<DeclarationReplayCapture>> = const { RefCell::new(None) };
}

struct DeclarationReplayPending {
    request: Option<serde_json::Value>,
    report: Option<Result<serde_json::Value, String>>,
}

#[derive(Default)]
struct DeclarationReplayCapture {
    calls: Vec<&'static str>,
    edges: BTreeMap<String, u64>,
    visibility_writes: Vec<(NodeId, bool)>,
}

struct DeclarationReplayCallGuard {
    active: bool,
}

impl DeclarationReplayCallGuard {
    fn enter(member: &'static str) -> Self {
        let active = DECLARATION_REPLAY_CAPTURE.with(|slot| {
            let mut slot = slot.borrow_mut();
            let Some(capture) = slot.as_mut() else {
                return false;
            };
            if let Some(parent) = capture.calls.last() {
                *capture
                    .edges
                    .entry(format!("{parent} -> {member}"))
                    .or_default() += 1;
            }
            capture.calls.push(member);
            true
        });
        Self { active }
    }
}

impl Drop for DeclarationReplayCallGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        DECLARATION_REPLAY_CAPTURE.with(|slot| {
            let popped = slot
                .borrow_mut()
                .as_mut()
                .and_then(|capture| capture.calls.pop());
            debug_assert!(popped.is_some());
        });
    }
}

fn record_declaration_replay_visibility_write(node: NodeId, value: bool) {
    DECLARATION_REPLAY_CAPTURE.with(|slot| {
        if let Some(capture) = slot.borrow_mut().as_mut() {
            capture.visibility_writes.push((node, value));
        }
    });
}

struct DeclarationReplayPendingGuard {
    armed: bool,
}

impl Drop for DeclarationReplayPendingGuard {
    fn drop(&mut self) {
        if self.armed {
            DECLARATION_REPLAY_PENDING.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DeclarationReplayCoordinate {
    file_tag: usize,
    kind: u16,
    pos: u32,
    end: u32,
}

impl DeclarationReplayCoordinate {
    fn json(self) -> serde_json::Value {
        serde_json::json!([self.file_tag, self.kind, self.pos, self.end])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeclarationReplayFileClass {
    Source,
    Library,
}

#[derive(Clone, Copy, Debug)]
struct DeclarationReplayFile {
    class: DeclarationReplayFileClass,
    program_index: usize,
}

struct DeclarationReplayFileMap {
    files: Vec<DeclarationReplayFile>,
    tag_by_program_index: BTreeMap<usize, usize>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum DeclarationReplayExclusion {
    LibTarget,
    SyntheticWithoutOriginal,
    AmbiguousSymbol,
    ZeroDeclarationSymbol,
}

impl DeclarationReplayExclusion {
    const fn name(self) -> &'static str {
        match self {
            Self::LibTarget => "lib-target",
            Self::SyntheticWithoutOriginal => "synthetic-without-original",
            Self::AmbiguousSymbol => "ambiguous-symbol",
            Self::ZeroDeclarationSymbol => "zero-declaration-symbol",
        }
    }
}

enum DeclarationReplayResolutionError {
    Excluded(DeclarationReplayExclusion),
    Invalid(String),
}

impl From<String> for DeclarationReplayResolutionError {
    fn from(value: String) -> Self {
        Self::Invalid(value)
    }
}

struct DeclarationReplayFrame {
    call_id: i64,
    member: String,
    entry: serde_json::Value,
    maximal_domain_call: Option<i64>,
    root: bool,
}

struct DeclarationReplayRoot {
    member: String,
    entry: serde_json::Value,
    result: serde_json::Value,
    result_sequence: u64,
    visibility_writes: Vec<(serde_json::Value, bool)>,
    nested_edges: BTreeMap<String, u64>,
    /// h2-7a-m-3 §6.2: the ordered decision-lane events inside this root's
    /// span (withContext exits, tracker callbacks, syntactic frames,
    /// specifier-override arms) for the serialization replay comparison.
    nested_decision_events: Vec<serde_json::Value>,
}

#[derive(Default)]
struct DeclarationReplayMemberCounts {
    replayed: u64,
    excluded: BTreeMap<&'static str, u64>,
    shadow: u64,
}

enum DeclarationReplayInvocation {
    Unary {
        member: String,
        node: NodeId,
    },
    SymbolAccessible {
        symbol: SymbolId,
        enclosing: NodeId,
        meaning: EmitSymbolMeaning,
        should_compute_aliases: bool,
    },
    EntityNameVisible {
        entity_name: NodeId,
        enclosing: NodeId,
        should_compute_aliases: bool,
    },
    RequiresImplicitUndefined {
        parameter: NodeId,
        enclosing: Option<NodeId>,
    },
    CollectLinkedAliases {
        node: NodeId,
        set_visibility: bool,
    },
    /// h2-7a-m-3: one of the six traced serialization members.
    Serialization {
        member: String,
        node: NodeId,
        enclosing: Option<NodeId>,
        no_syntactic_printer: bool,
    },
}

enum DeclarationReplayDecision {
    Boolean(bool),
    Accessibility(EmitSymbolAccessibilityResult),
    Properties(Vec<EmitFunctionProperty>),
    Enum(crate::evaluate::EvaluatorResult),
    Void,
    /// h2-7a-m-3: a serialization member's outcome — the §6.3 produced
    /// class plus the ordered sink events recorded during the call.
    Serialized {
        produced: crate::node_builder::replay_sink::ProducedClass,
        events: Vec<crate::node_builder::replay_sink::DecisionEvent>,
    },
}

impl CheckerState<'_> {
    /// Run one compiler-integration operation with an owned, thread-scoped
    /// declaration replay request. The request is consumed by the live
    /// CheckerState after the ordinary check pass and before that state is
    /// released. Nothing is injected into NodeLinks.
    /// tsrs-native: harness-only replay observer seam (h2-7a-m-2 §7;
    /// production-inert — no request, no effect).
    #[doc(hidden)]
    pub fn with_declaration_emit_replay_observer_for_harness<T>(
        request: serde_json::Value,
        operation: impl FnOnce() -> T,
    ) -> Result<(T, serde_json::Value), String> {
        DECLARATION_REPLAY_PENDING.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_some() {
                return Err(
                    "a declaration replay request is already active on this thread".to_owned(),
                );
            }
            *slot = Some(DeclarationReplayPending {
                request: Some(request),
                report: None,
            });
            Ok(())
        })?;
        let mut guard = DeclarationReplayPendingGuard { armed: true };
        let output = operation();
        let pending = DECLARATION_REPLAY_PENDING.with(|slot| slot.borrow_mut().take());
        guard.armed = false;
        let pending = pending.ok_or_else(|| "declaration replay slot disappeared".to_owned())?;
        let report = pending
            .report
            .ok_or_else(|| "checker did not consume the declaration replay request".to_owned())??;
        Ok((output, report))
    }

    /// tsrs-native: harness-only replay driver behind the observer
    /// seam (h2-7a-m-2 §7).
    pub(crate) fn run_declaration_emit_replay_observer_for_harness(&mut self) {
        let request = DECLARATION_REPLAY_PENDING.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .and_then(|pending| pending.request.take())
        });
        let Some(request) = request else {
            return;
        };
        let report = self.declaration_emit_replay_case(&request);
        DECLARATION_REPLAY_PENDING.with(|slot| {
            let mut slot = slot.borrow_mut();
            let pending = slot
                .as_mut()
                .expect("declaration replay pending slot remains installed");
            pending.report = Some(report);
        });
    }

    fn declaration_emit_replay_case(
        &mut self,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let case_id = replay_string_field(request, "case_id")?;
        let file_map = self.declaration_replay_file_map(request)?;
        let events = replay_array_field(request, "trace_events")?;
        let (roots, traced_nested_edges) = declaration_replay_roots(events, case_id)?;
        let roots_by_sequence = roots
            .into_iter()
            .map(|root| (root.result_sequence, root))
            .collect::<BTreeMap<_, _>>();

        let mut member_counts = declaration_replay_domain_members()
            .iter()
            .map(|member| {
                (
                    (*member).to_owned(),
                    DeclarationReplayMemberCounts::default(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut gating_mismatches = Vec::new();
        let mut seed_checks = 0_u64;
        let mut replayed_nested_edges = BTreeMap::new();
        let mut rust_nested_edges = BTreeMap::new();

        for event in events {
            let sequence = replay_u64_field(event, "event_seq")?;
            let site = replay_string_field(event, "site_id")?;
            if matches!(site, "probe.checkSeed" | "probe.transformSeed") {
                seed_checks += 1;
                if let Err(detail) = self.declaration_replay_compare_seed(event, &file_map) {
                    gating_mismatches.push(format!("{case_id} event {sequence} {site}: {detail}"));
                }
            }
            let Some(root) = roots_by_sequence.get(&sequence) else {
                continue;
            };
            let counts = member_counts
                .get_mut(&root.member)
                .ok_or_else(|| format!("{case_id}: missing count row for {}", root.member))?;
            match self.declaration_replay_root(root, &file_map) {
                Ok(DeclarationReplayRootOutcome::Excluded(class)) => {
                    *counts.excluded.entry(class.name()).or_default() += 1;
                }
                Ok(DeclarationReplayRootOutcome::Replayed {
                    gating_mismatch,
                    shadow_divergences,
                    expected_nested_edges,
                    actual_nested_edges,
                }) => {
                    counts.replayed += 1;
                    counts.shadow += shadow_divergences;
                    if let Some(detail) = gating_mismatch {
                        gating_mismatches.push(format!(
                            "{case_id} event {sequence} {}: {detail}",
                            root.member
                        ));
                    }
                    merge_replay_counts(&mut replayed_nested_edges, &expected_nested_edges);
                    merge_replay_counts(&mut rust_nested_edges, &actual_nested_edges);
                }
                Err(detail) => gating_mismatches.push(format!(
                    "{case_id} event {sequence} {}: {detail}",
                    root.member
                )),
            }
        }

        let nested_topology_divergences =
            replay_count_distance(&replayed_nested_edges, &rust_nested_edges);
        let counts_json = member_counts
            .into_iter()
            .map(|(member, counts)| {
                let excluded = declaration_replay_exclusion_names()
                    .iter()
                    .map(|class| {
                        (
                            (*class).to_owned(),
                            serde_json::json!(counts.excluded.get(class).copied().unwrap_or(0)),
                        )
                    })
                    .collect::<serde_json::Map<_, _>>();
                (
                    member,
                    serde_json::json!({
                        "replayed": counts.replayed,
                        "excluded": excluded,
                        "shadow": counts.shadow,
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>();

        Ok(serde_json::json!({
            "case_id": case_id,
            "seed_checks": seed_checks,
            "member_counts": counts_json,
            "gating_mismatches": gating_mismatches,
            "traced_nested_edges": replay_counts_json(&traced_nested_edges),
            "replayed_nested_edges": replay_counts_json(&replayed_nested_edges),
            "rust_nested_edges": replay_counts_json(&rust_nested_edges),
            "nested_topology_divergences": nested_topology_divergences,
        }))
    }
}

enum DeclarationReplayRootOutcome {
    Excluded(DeclarationReplayExclusion),
    Replayed {
        gating_mismatch: Option<String>,
        shadow_divergences: u64,
        expected_nested_edges: BTreeMap<String, u64>,
        actual_nested_edges: BTreeMap<String, u64>,
    },
}

fn declaration_replay_domain_members() -> &'static [&'static str] {
    &[
        "resolver.collectLinkedAliases",
        "resolver.createLateBoundIndexSignatures",
        "resolver.createLiteralConstValue",
        "resolver.createReturnTypeOfSignatureDeclaration",
        "resolver.createTypeOfDeclaration",
        "resolver.createTypeOfExpression",
        "resolver.getDeclarationStatementsForSourceFile",
        "resolver.getEnumMemberValue",
        "resolver.getPropertiesOfContainerFunction",
        "resolver.isDeclarationVisible",
        "resolver.isDefinitelyReferenceToGlobalSymbolObject",
        "resolver.isEntityNameVisible",
        "resolver.isExpandoFunctionDeclaration",
        "resolver.isImplementationOfOverload",
        "resolver.isImportRequiredByAugmentation",
        "resolver.isLateBound",
        "resolver.isLiteralConstDeclaration",
        "resolver.isOptionalParameter",
        "resolver.isSymbolAccessible",
        "resolver.requiresAddingImplicitUndefined",
    ]
}

fn declaration_replay_exclusion_names() -> &'static [&'static str] {
    &[
        "lib-target",
        "synthetic-without-original",
        "ambiguous-symbol",
        "zero-declaration-symbol",
    ]
}

fn declaration_replay_is_domain_member(member: &str) -> bool {
    declaration_replay_domain_members().contains(&member)
}

fn merge_replay_counts(target: &mut BTreeMap<String, u64>, source: &BTreeMap<String, u64>) {
    for (key, value) in source {
        *target.entry(key.clone()).or_default() += value;
    }
}

fn replay_count_distance(left: &BTreeMap<String, u64>, right: &BTreeMap<String, u64>) -> u64 {
    left.keys()
        .chain(right.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|key| {
            left.get(key)
                .copied()
                .unwrap_or(0)
                .abs_diff(right.get(key).copied().unwrap_or(0))
        })
        .sum()
}

fn replay_counts_json(counts: &BTreeMap<String, u64>) -> serde_json::Value {
    serde_json::Value::Object(
        counts
            .iter()
            .map(|(key, value)| (key.clone(), serde_json::json!(value)))
            .collect(),
    )
}

fn replay_string_field<'a>(value: &'a serde_json::Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("missing string field {field:?}"))
}

fn replay_u64_field(value: &serde_json::Value, field: &str) -> Result<u64, String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("missing unsigned field {field:?}"))
}

fn replay_i64_field(value: &serde_json::Value, field: &str) -> Result<i64, String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| format!("missing integer field {field:?}"))
}

fn replay_array_field<'a>(
    value: &'a serde_json::Value,
    field: &str,
) -> Result<&'a [serde_json::Value], String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("missing array field {field:?}"))
}

/// h2-7a-m-3 §6.2: the harness tracker. Records every callback into the
/// decision sink with the probe payload projection and answers trackSymbol
/// with the transformer's issued-diagnostic protocol (accessible → false;
/// otherwise a diagnostic would be issued → true), so SymbolTrackerImpl
/// bookkeeping runs the production arms.
struct DeclarationReplayRecordingTracker;

impl DeclarationReplayRecordingTracker {
    fn raw_ref(
        access: &mut dyn tsc_emitter::EmitTrackerAccess,
        node: Option<tsc_emitter::EmitTrackerNode>,
    ) -> serde_json::Value {
        match node {
            None => serde_json::Value::Null,
            Some(node) => {
                let description = access.describe_node(node);
                match (description.parse, description.original) {
                    (Some(parse), _) => {
                        serde_json::json!([parse.source().raw(), parse.node().0])
                    }
                    (None, Some(original)) => {
                        serde_json::json!([original.source().raw(), original.node().0])
                    }
                    (None, None) => serde_json::json!("opaque"),
                }
            }
        }
    }
}

impl tsc_emitter::EmitSymbolTracker for DeclarationReplayRecordingTracker {
    fn can_track_symbol(&self) -> bool {
        true
    }

    fn track_symbol(
        &mut self,
        access: &mut dyn tsc_emitter::EmitTrackerAccess,
        symbol: tsc_emitter::EmitTrackerSymbol,
        enclosing_declaration: Option<tsc_emitter::EmitTrackerNode>,
        meaning: EmitSymbolMeaning,
    ) -> Result<bool, tsc_emitter::EmitResolverError> {
        let description = access.describe_symbol(symbol);
        let node_payload = Self::raw_ref(access, enclosing_declaration);
        let payload = serde_json::json!({
            "name": description.escaped_name,
            "node": node_payload,
            "meaning": meaning.0,
        });
        crate::node_builder::replay_sink::record(move || {
            crate::node_builder::replay_sink::DecisionEvent::Tracker {
                site: "tracker.trackSymbol",
                payload,
            }
        });
        // tsc-port: trackSymbol @6.0.3 (:114360-114369) — the transformer
        // resolves accessibility and reports handleSymbolAccessibilityError's
        // issued-diagnostic verdict.
        let verdict = access.is_symbol_accessible(symbol, enclosing_declaration, meaning, true)?;
        Ok(verdict.accessibility != EmitSymbolAccessibility::Accessible)
    }

    fn report_inference_fallback(
        &mut self,
        access: &mut dyn tsc_emitter::EmitTrackerAccess,
        node: tsc_emitter::EmitTrackerNode,
    ) -> Result<(), tsc_emitter::EmitResolverError> {
        let payload = Self::raw_ref(access, Some(node));
        crate::node_builder::replay_sink::record(move || {
            crate::node_builder::replay_sink::DecisionEvent::Tracker {
                site: "tracker.reportInferenceFallback",
                payload,
            }
        });
        Ok(())
    }

    fn report_private_in_base_of_class_expression(&mut self, property_name: &str) {
        let payload = serde_json::json!(property_name);
        crate::node_builder::replay_sink::record(move || {
            crate::node_builder::replay_sink::DecisionEvent::Tracker {
                site: "tracker.reportPrivateInBaseOfClassExpression",
                payload,
            }
        });
    }

    fn report_inaccessible_unique_symbol_error(&mut self) {
        crate::node_builder::replay_sink::record(|| {
            crate::node_builder::replay_sink::DecisionEvent::Tracker {
                site: "tracker.reportInaccessibleUniqueSymbolError",
                payload: serde_json::Value::Null,
            }
        });
    }

    fn report_cyclic_structure_error(&mut self) {
        crate::node_builder::replay_sink::record(|| {
            crate::node_builder::replay_sink::DecisionEvent::Tracker {
                site: "tracker.reportCyclicStructureError",
                payload: serde_json::Value::Null,
            }
        });
    }

    fn report_inaccessible_this_error(&mut self) {
        crate::node_builder::replay_sink::record(|| {
            crate::node_builder::replay_sink::DecisionEvent::Tracker {
                site: "tracker.reportInaccessibleThisError",
                payload: serde_json::Value::Null,
            }
        });
    }

    fn report_likely_unsafe_import_required_error(
        &mut self,
        specifier: &str,
        symbol_name: Option<&str>,
    ) {
        // The frozen probe projection is LOSSY (§6.2): is-string,
        // slash-component count, symbol name.
        let payload = serde_json::json!([
            true,
            specifier.split('/').count(),
            symbol_name.unwrap_or(""),
        ]);
        crate::node_builder::replay_sink::record(move || {
            crate::node_builder::replay_sink::DecisionEvent::Tracker {
                site: "tracker.reportLikelyUnsafeImportRequiredError",
                payload,
            }
        });
    }

    fn report_truncation_error(&mut self) {
        crate::node_builder::replay_sink::record(|| {
            crate::node_builder::replay_sink::DecisionEvent::Tracker {
                site: "tracker.reportTruncationError",
                payload: serde_json::Value::Null,
            }
        });
    }

    fn report_nonlocal_augmentation(
        &mut self,
        _containing_file: tsc_emitter::EmitTrackerNode,
        _parent_symbol: tsc_emitter::EmitTrackerSymbol,
        _augmenting_symbol: tsc_emitter::EmitTrackerSymbol,
    ) {
        crate::node_builder::replay_sink::record(|| {
            crate::node_builder::replay_sink::DecisionEvent::Tracker {
                site: "tracker.reportNonlocalAugmentation",
                payload: serde_json::Value::Null,
            }
        });
    }

    fn report_non_serializable_property(&mut self, property_name: &str) {
        let payload = serde_json::json!(property_name);
        crate::node_builder::replay_sink::record(move || {
            crate::node_builder::replay_sink::DecisionEvent::Tracker {
                site: "tracker.reportNonSerializableProperty",
                payload,
            }
        });
    }
}

fn declaration_replay_roots(
    events: &[serde_json::Value],
    case_id: &str,
) -> Result<(Vec<DeclarationReplayRoot>, BTreeMap<String, u64>), String> {
    let mut stack: Vec<DeclarationReplayFrame> = Vec::new();
    let mut writes: BTreeMap<i64, Vec<(serde_json::Value, bool)>> = BTreeMap::new();
    let mut nested_by_root: BTreeMap<i64, BTreeMap<String, u64>> = BTreeMap::new();
    let mut decision_by_root: BTreeMap<i64, Vec<serde_json::Value>> = BTreeMap::new();
    let mut all_nested = BTreeMap::new();
    let mut roots = Vec::new();
    let is_decision_lane = |site: &str| {
        site == "nodebuilder.withContext.result"
            || site == "nodebuilder.withContext.decision"
            || site.starts_with("nodebuilder.moduleSpecifierOverride")
            || site.starts_with("tracker.")
            || site.starts_with("syntactic.")
    };

    for event in events {
        let site = replay_string_field(event, "site_id")?;
        let call_id = replay_i64_field(event, "call_id")?;
        if call_id >= 0 && site.ends_with(".entry") {
            let member = site.trim_end_matches(".entry").to_owned();
            let parent_domain = stack
                .iter()
                .rev()
                .find(|frame| declaration_replay_is_domain_member(&frame.member));
            let inherited_root = stack.last().and_then(|frame| frame.maximal_domain_call);
            let is_domain = declaration_replay_is_domain_member(&member);
            let root = is_domain && inherited_root.is_none();
            let maximal_domain_call = if root { Some(call_id) } else { inherited_root };
            if is_domain {
                if let Some(parent) = parent_domain {
                    let edge = format!("{} -> {member}", parent.member);
                    *all_nested.entry(edge.clone()).or_default() += 1;
                    if let Some(root_id) = maximal_domain_call {
                        *nested_by_root
                            .entry(root_id)
                            .or_default()
                            .entry(edge)
                            .or_default() += 1;
                    }
                }
            }
            if is_decision_lane(site) {
                if let Some(root_id) = maximal_domain_call {
                    decision_by_root
                        .entry(root_id)
                        .or_default()
                        .push(event.clone());
                }
            }
            stack.push(DeclarationReplayFrame {
                call_id,
                member,
                entry: event.clone(),
                maximal_domain_call,
                root,
            });
            continue;
        }
        if call_id >= 0 && site.ends_with(".result") {
            let frame = stack
                .pop()
                .ok_or_else(|| format!("{case_id}: result {site} call {call_id} has no entry"))?;
            if frame.call_id != call_id || frame.member != site.trim_end_matches(".result") {
                return Err(format!(
                    "{case_id}: call stack mismatch at {site} call {call_id}"
                ));
            }
            if is_decision_lane(site) {
                if let Some(root_id) = frame.maximal_domain_call {
                    decision_by_root
                        .entry(root_id)
                        .or_default()
                        .push(event.clone());
                }
            }
            if frame.root {
                roots.push(DeclarationReplayRoot {
                    member: frame.member,
                    entry: frame.entry,
                    result: event.clone(),
                    result_sequence: replay_u64_field(event, "event_seq")?,
                    visibility_writes: writes.remove(&call_id).unwrap_or_default(),
                    nested_edges: nested_by_root.remove(&call_id).unwrap_or_default(),
                    nested_decision_events: decision_by_root.remove(&call_id).unwrap_or_default(),
                });
            }
            continue;
        }
        if call_id < 0 && is_decision_lane(site) {
            if let Some(root_id) = stack.last().and_then(|frame| frame.maximal_domain_call) {
                decision_by_root
                    .entry(root_id)
                    .or_default()
                    .push(event.clone());
            }
        }
        if site.starts_with("isVisible.") {
            let Some(root_id) = stack.last().and_then(|frame| frame.maximal_domain_call) else {
                continue;
            };
            let args = replay_array_field(event, "args")?;
            let node = args
                .get(1)
                .ok_or_else(|| format!("{case_id}: writer {site} lacks node"))?
                .clone();
            let value = args
                .get(2)
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| format!("{case_id}: writer {site} lacks value"))?;
            writes.entry(root_id).or_default().push((node, value));
        }
    }
    if !stack.is_empty() {
        return Err(format!("{case_id}: trace ended with open call frames"));
    }
    Ok((roots, all_nested))
}

impl CheckerState<'_> {
    fn declaration_replay_file_map(
        &self,
        request: &serde_json::Value,
    ) -> Result<DeclarationReplayFileMap, String> {
        let source_paths = replay_array_field(request, "source_paths")?;
        let file_table = replay_array_field(request, "file_table")?;
        let mut files = Vec::with_capacity(file_table.len());
        let mut tag_by_program_index = BTreeMap::new();

        for (file_tag, row) in file_table.iter().enumerate() {
            let row = row
                .as_array()
                .ok_or_else(|| format!("fileTable row {file_tag} is not an array"))?;
            if row.len() != 2 {
                return Err(format!("fileTable row {file_tag} has wrong arity"));
            }
            let class = row[0]
                .as_str()
                .ok_or_else(|| format!("fileTable row {file_tag} lacks a class"))?;
            let (class, program_index) = match class {
                "src" => {
                    let source_index = row[1]
                        .as_u64()
                        .and_then(|index| usize::try_from(index).ok())
                        .ok_or_else(|| {
                            format!("fileTable row {file_tag} has invalid source index")
                        })?;
                    let source_path = source_paths
                        .get(source_index)
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            format!("fileTable row {file_tag} source index is out of range")
                        })?;
                    let program_index = self
                        .declaration_replay_find_source(source_path, false)
                        .ok_or_else(|| {
                            format!(
                                "fileTable row {file_tag} source {source_path:?} is absent from the Program"
                            )
                        })?;
                    (DeclarationReplayFileClass::Source, program_index)
                }
                "lib" => {
                    let basename = row[1]
                        .as_str()
                        .ok_or_else(|| format!("fileTable row {file_tag} lacks a lib name"))?;
                    let program_index = self
                        .declaration_replay_find_source(basename, true)
                        .ok_or_else(|| {
                            format!(
                                "fileTable row {file_tag} library {basename:?} is absent from the Program"
                            )
                        })?;
                    (DeclarationReplayFileClass::Library, program_index)
                }
                other => return Err(format!("unknown fileTable class {other:?}")),
            };
            if tag_by_program_index
                .insert(program_index, file_tag)
                .is_some()
            {
                return Err(format!(
                    "fileTable rows alias Program source index {program_index}"
                ));
            }
            files.push(DeclarationReplayFile {
                class,
                program_index,
            });
        }
        Ok(DeclarationReplayFileMap {
            files,
            tag_by_program_index,
        })
    }

    fn declaration_replay_find_source(&self, expected: &str, basename_only: bool) -> Option<usize> {
        let expected = expected.replace('\\', "/");
        let matches = (0..self.binder.file_count())
            .filter(|&index| {
                let actual = self.binder.source(index).file_name.replace('\\', "/");
                if basename_only {
                    actual.rsplit('/').next() == Some(expected.as_str())
                } else {
                    actual == expected
                }
            })
            .collect::<Vec<_>>();
        (matches.len() == 1).then_some(matches[0])
    }

    fn declaration_replay_compare_seed(
        &self,
        event: &serde_json::Value,
        file_map: &DeclarationReplayFileMap,
    ) -> Result<(), String> {
        let args = replay_array_field(event, "args")?;
        let rows = args
            .get(1)
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "seed event lacks rows".to_owned())?;
        let mut expected = Vec::with_capacity(rows.len());
        for row in rows {
            let row = row
                .as_array()
                .filter(|row| row.len() == 2)
                .ok_or_else(|| "seed row is malformed".to_owned())?;
            // Seed dumps cover every defined isVisible slot, including
            // default-library nodes.  Library targeting is an invocation
            // exclusion only; it must not erase seed state.
            let coordinate = declaration_replay_coordinate(&row[0], file_map, false)
                .map_err(declaration_replay_resolution_message)?
                .ok_or_else(|| "seed row uses a sentinel node".to_owned())?;
            let value = row[1]
                .as_bool()
                .ok_or_else(|| "seed row lacks a boolean value".to_owned())?;
            expected.push((coordinate, value));
        }
        declaration_replay_sort_visibility_rows(&mut expected);

        let mut actual = Vec::new();
        for file_index in 0..self.binder.file_count() {
            let source = self.binder.source(file_index);
            for node in source.arena.node_ids() {
                let Some(value) = self.links.node(node).is_visible else {
                    continue;
                };
                let coordinate = self.declaration_replay_project_node(node, file_map)?;
                actual.push((coordinate, value));
            }
        }
        declaration_replay_sort_visibility_rows(&mut actual);
        if actual == expected {
            Ok(())
        } else {
            Err(format!(
                "visibility seed differs: expected {}, actual {}",
                declaration_replay_visibility_json(&expected),
                declaration_replay_visibility_json(&actual)
            ))
        }
    }

    fn declaration_replay_root(
        &mut self,
        root: &DeclarationReplayRoot,
        file_map: &DeclarationReplayFileMap,
    ) -> Result<DeclarationReplayRootOutcome, String> {
        let preparation = match self.declaration_replay_prepare_root(root, file_map) {
            Ok(preparation) => preparation,
            Err(DeclarationReplayResolutionError::Excluded(class)) => {
                return Ok(DeclarationReplayRootOutcome::Excluded(class));
            }
            Err(DeclarationReplayResolutionError::Invalid(detail)) => return Err(detail),
        };

        DECLARATION_REPLAY_CAPTURE.with(|slot| {
            let mut slot = slot.borrow_mut();
            assert!(slot.is_none(), "declaration replay capture cannot nest");
            *slot = Some(DeclarationReplayCapture::default());
        });
        let decision = self.declaration_replay_invoke(preparation.invocation);
        let capture = DECLARATION_REPLAY_CAPTURE.with(|slot| {
            slot.borrow_mut()
                .take()
                .expect("declaration replay capture remains installed")
        });
        if !capture.calls.is_empty() {
            return Err("Rust replay ended with open call frames".to_owned());
        }
        let decision = decision?;
        let actual = self.declaration_replay_project_decision(&decision, file_map)?;
        let actual_paint = capture
            .visibility_writes
            .into_iter()
            .map(|(node, value)| {
                self.declaration_replay_project_node(node, file_map)
                    .map(|coordinate| (coordinate, value))
            })
            .collect::<Result<BTreeSet<_>, _>>()?;

        let mut mismatches = Vec::new();
        if actual != preparation.expected {
            mismatches.push(format!(
                "result differs: expected {}, actual {}",
                preparation.expected, actual
            ));
        }
        if actual_paint != preparation.expected_paint {
            mismatches.push(format!(
                "paint set differs: expected {}, actual {}",
                declaration_replay_visibility_set_json(&preparation.expected_paint),
                declaration_replay_visibility_set_json(&actual_paint)
            ));
        }
        mismatches.extend(preparation.input_mismatches);
        let shadow_divergences = declaration_replay_shadow_divergences(
            &decision,
            &preparation.expected_shadow_symbol,
            &preparation.expected_shadow_module,
        );

        Ok(DeclarationReplayRootOutcome::Replayed {
            gating_mismatch: (!mismatches.is_empty()).then(|| mismatches.join("; ")),
            shadow_divergences,
            expected_nested_edges: root.nested_edges.clone(),
            actual_nested_edges: capture.edges,
        })
    }
}

struct DeclarationReplayPreparation {
    invocation: DeclarationReplayInvocation,
    expected: serde_json::Value,
    expected_paint: BTreeSet<(DeclarationReplayCoordinate, bool)>,
    expected_shadow_symbol: Option<String>,
    expected_shadow_module: Option<Option<String>>,
    input_mismatches: Vec<String>,
}

type DeclarationReplayExpectedDecision =
    (serde_json::Value, Option<String>, Option<Option<String>>);

fn declaration_replay_sort_visibility_rows(rows: &mut [(DeclarationReplayCoordinate, bool)]) {
    rows.sort_by_key(|(coordinate, value)| {
        (
            coordinate.file_tag,
            coordinate.pos,
            coordinate.end,
            coordinate.kind,
            *value,
        )
    });
}

fn declaration_replay_visibility_json(
    rows: &[(DeclarationReplayCoordinate, bool)],
) -> serde_json::Value {
    serde_json::Value::Array(
        rows.iter()
            .map(|(coordinate, value)| serde_json::json!([coordinate.json(), value]))
            .collect(),
    )
}

fn declaration_replay_visibility_set_json(
    rows: &BTreeSet<(DeclarationReplayCoordinate, bool)>,
) -> serde_json::Value {
    declaration_replay_visibility_json(&rows.iter().copied().collect::<Vec<_>>())
}

fn declaration_replay_resolution_message(error: DeclarationReplayResolutionError) -> String {
    match error {
        DeclarationReplayResolutionError::Excluded(class) => {
            format!("reference belongs to excluded class {}", class.name())
        }
        DeclarationReplayResolutionError::Invalid(detail) => detail,
    }
}

fn declaration_replay_coordinate(
    value: &serde_json::Value,
    file_map: &DeclarationReplayFileMap,
    reject_library: bool,
) -> Result<Option<DeclarationReplayCoordinate>, DeclarationReplayResolutionError> {
    let values = value
        .as_array()
        .filter(|values| values.len() == 8)
        .ok_or_else(|| "node reference is not an eight-element array".to_owned())?;
    let numbers = values
        .iter()
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| "node reference contains a non-integer".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let coordinate = if numbers[0] >= 0 {
        &numbers[0..4]
    } else if numbers[4] >= 0 {
        &numbers[4..8]
    } else {
        return Ok(None);
    };
    let file_tag = usize::try_from(coordinate[0])
        .map_err(|_| "node reference file tag is negative".to_owned())?;
    let file = file_map
        .files
        .get(file_tag)
        .ok_or_else(|| "node reference file tag is out of range".to_owned())?;
    if reject_library && file.class == DeclarationReplayFileClass::Library {
        return Err(DeclarationReplayResolutionError::Excluded(
            DeclarationReplayExclusion::LibTarget,
        ));
    }
    Ok(Some(DeclarationReplayCoordinate {
        file_tag,
        kind: u16::try_from(coordinate[1])
            .map_err(|_| "node reference kind is out of range".to_owned())?,
        pos: u32::try_from(coordinate[2])
            .map_err(|_| "node reference pos is out of range".to_owned())?,
        end: u32::try_from(coordinate[3])
            .map_err(|_| "node reference end is out of range".to_owned())?,
    }))
}

impl CheckerState<'_> {
    fn declaration_replay_resolve_node(
        &self,
        value: &serde_json::Value,
        file_map: &DeclarationReplayFileMap,
    ) -> Result<NodeId, DeclarationReplayResolutionError> {
        let coordinate = declaration_replay_coordinate(value, file_map, true)?.ok_or(
            DeclarationReplayResolutionError::Excluded(
                DeclarationReplayExclusion::SyntheticWithoutOriginal,
            ),
        )?;
        let file = file_map.files[coordinate.file_tag];
        let source = self.binder.source(file.program_index);
        let matches = source
            .arena
            .node_ids()
            .filter(|&node| {
                let data = source.arena.node(node);
                (node == source.root || data.parent.is_some())
                    && data.kind as u16 == coordinate.kind
                    && data.pos == coordinate.pos
                    && data.end == coordinate.end
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [node] => Ok(*node),
            [] => {
                let same_kind = source
                    .arena
                    .node_ids()
                    .filter_map(|node| {
                        let data = source.arena.node(node);
                        (data.kind as u16 == coordinate.kind).then_some((data.pos, data.end))
                    })
                    .take(8)
                    .collect::<Vec<_>>();
                let same_span = source
                    .arena
                    .node_ids()
                    .filter_map(|node| {
                        let data = source.arena.node(node);
                        (data.pos == coordinate.pos && data.end == coordinate.end)
                            .then_some(data.kind as u16)
                    })
                    .take(8)
                    .collect::<Vec<_>>();
                let sample = source
                    .arena
                    .node_ids()
                    .take(12)
                    .map(|node| {
                        let data = source.arena.node(node);
                        (data.kind as u16, data.pos, data.end)
                    })
                    .collect::<Vec<_>>();
                Err(DeclarationReplayResolutionError::Invalid(format!(
                    "node {:?} has no Rust parse-tree match (file {:?}, same-kind spans {:?}, same-span kinds {:?}, sample {:?})",
                    coordinate, source.file_name, same_kind, same_span, sample
                )))
            }
            _ => Err(DeclarationReplayResolutionError::Invalid(format!(
                "node {:?} has {} Rust parse-tree matches",
                coordinate,
                matches.len()
            ))),
        }
    }

    fn declaration_replay_project_node(
        &self,
        node: NodeId,
        file_map: &DeclarationReplayFileMap,
    ) -> Result<DeclarationReplayCoordinate, String> {
        let program_index = self.binder.file_index_of_node(node);
        let file_tag = file_map
            .tag_by_program_index
            .get(&program_index)
            .copied()
            .ok_or_else(|| {
                format!(
                    "Rust node {:?} belongs to Program file {:?}, absent from fileTable",
                    node,
                    self.binder.source(program_index).file_name
                )
            })?;
        let data = self.binder.source(program_index).arena.node(node);
        Ok(DeclarationReplayCoordinate {
            file_tag,
            kind: data.kind as u16,
            pos: data.pos,
            end: data.end,
        })
    }

    fn declaration_replay_prepare_root(
        &mut self,
        root: &DeclarationReplayRoot,
        file_map: &DeclarationReplayFileMap,
    ) -> Result<DeclarationReplayPreparation, DeclarationReplayResolutionError> {
        let entry_args = replay_array_field(&root.entry, "args")?;
        let result_args = replay_array_field(&root.result, "args")?;
        let mut input_mismatches = Vec::new();
        let invocation = match root.member.as_str() {
            "resolver.createTypeOfDeclaration"
            | "resolver.createReturnTypeOfSignatureDeclaration"
            | "resolver.createTypeOfExpression"
            | "resolver.createLiteralConstValue"
            | "resolver.getDeclarationStatementsForSourceFile"
            | "resolver.createLateBoundIndexSignatures" => {
                // Generic entry tail: [site, argc, name(first), nodeRef(first),
                // nodeRef(second), scalar(first), scalar(second)].
                let node = self.declaration_replay_resolve_node(
                    entry_args
                        .get(3)
                        .ok_or_else(|| "serialization entry lacks a node".to_owned())?,
                    file_map,
                )?;
                let takes_enclosing = matches!(
                    root.member.as_str(),
                    "resolver.createTypeOfDeclaration"
                        | "resolver.createReturnTypeOfSignatureDeclaration"
                        | "resolver.createTypeOfExpression"
                        | "resolver.createLateBoundIndexSignatures"
                );
                let enclosing = if takes_enclosing {
                    Some(self.declaration_replay_resolve_node(
                        entry_args.get(4).ok_or_else(|| {
                            "serialization entry lacks an enclosing node".to_owned()
                        })?,
                        file_map,
                    )?)
                } else {
                    None
                };
                // The :115425 fakespace variant is recovered from the traced
                // withContext internal words and asserted consistent.
                let mut no_syntactic_printer = false;
                for event in &root.nested_decision_events {
                    if replay_string_field(event, "site_id")? != "nodebuilder.withContext.result" {
                        continue;
                    }
                    let args = replay_array_field(event, "args")?;
                    let internal = args
                        .get(3)
                        .and_then(serde_json::Value::as_u64)
                        .ok_or_else(|| "withContext result lacks internal flags".to_owned())?;
                    if internal & 2 != 0 {
                        no_syntactic_printer = true;
                    }
                }
                DeclarationReplayInvocation::Serialization {
                    member: root.member.clone(),
                    node,
                    enclosing,
                    no_syntactic_printer,
                }
            }
            "resolver.isSymbolAccessible" => {
                let symbol_ref = entry_args
                    .get(2)
                    .ok_or_else(|| "isSymbolAccessible entry lacks symbol".to_owned())?;
                let (symbol, mismatch) =
                    self.declaration_replay_resolve_symbol(symbol_ref, file_map)?;
                if let Some(mismatch) = mismatch {
                    input_mismatches.push(mismatch);
                }
                let enclosing = self.declaration_replay_resolve_node(
                    entry_args.get(3).ok_or_else(|| {
                        "isSymbolAccessible entry lacks enclosing node".to_owned()
                    })?,
                    file_map,
                )?;
                let meaning = entry_args
                    .get(4)
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| "isSymbolAccessible entry has invalid meaning".to_owned())?;
                let should_compute_aliases = entry_args
                    .get(5)
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| {
                        "isSymbolAccessible entry lacks shouldComputeAliases".to_owned()
                    })?;
                DeclarationReplayInvocation::SymbolAccessible {
                    symbol,
                    enclosing,
                    meaning: EmitSymbolMeaning(meaning),
                    should_compute_aliases,
                }
            }
            "resolver.isEntityNameVisible" => {
                let entity_name = self.declaration_replay_resolve_node(
                    entry_args
                        .get(2)
                        .ok_or_else(|| "isEntityNameVisible entry lacks entity name".to_owned())?,
                    file_map,
                )?;
                let enclosing = self.declaration_replay_resolve_node(
                    entry_args.get(3).ok_or_else(|| {
                        "isEntityNameVisible entry lacks enclosing node".to_owned()
                    })?,
                    file_map,
                )?;
                let should_compute_aliases = entry_args
                    .get(4)
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| {
                        "isEntityNameVisible entry lacks shouldComputeAliases".to_owned()
                    })?;
                DeclarationReplayInvocation::EntityNameVisible {
                    entity_name,
                    enclosing,
                    should_compute_aliases,
                }
            }
            "resolver.requiresAddingImplicitUndefined" => {
                let parameter = self.declaration_replay_generic_first_node(entry_args, file_map)?;
                let arity = entry_args
                    .get(1)
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| "generic entry lacks arity".to_owned())?;
                let enclosing = if arity >= 2 {
                    Some(
                        self.declaration_replay_resolve_node(
                            entry_args
                                .get(4)
                                .ok_or_else(|| "generic entry lacks second node".to_owned())?,
                            file_map,
                        )?,
                    )
                } else {
                    None
                };
                DeclarationReplayInvocation::RequiresImplicitUndefined {
                    parameter,
                    enclosing,
                }
            }
            "resolver.collectLinkedAliases" => {
                let node = self.declaration_replay_resolve_node(
                    entry_args
                        .get(1)
                        .ok_or_else(|| "collectLinkedAliases entry lacks node".to_owned())?,
                    file_map,
                )?;
                let set_visibility = entry_args
                    .get(2)
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| "collectLinkedAliases entry lacks setVisibility".to_owned())?;
                if !set_visibility {
                    return Err(DeclarationReplayResolutionError::Invalid(
                        "replay domain admits only collectLinkedAliases(true)".to_owned(),
                    ));
                }
                DeclarationReplayInvocation::CollectLinkedAliases {
                    node,
                    set_visibility,
                }
            }
            member => DeclarationReplayInvocation::Unary {
                member: member.to_owned(),
                node: self.declaration_replay_generic_first_node(entry_args, file_map)?,
            },
        };

        let (expected, expected_shadow_symbol, expected_shadow_module) = if matches!(
            invocation,
            DeclarationReplayInvocation::Serialization { .. }
        ) {
            (
                self.declaration_replay_expected_serialized(root, file_map)
                    .map_err(DeclarationReplayResolutionError::Invalid)?,
                None,
                None,
            )
        } else {
            self.declaration_replay_expected_decision(&root.member, result_args, file_map)?
        };
        let expected_paint = root
            .visibility_writes
            .iter()
            .map(|(node, value)| {
                let coordinate = declaration_replay_coordinate(node, file_map, true)?.ok_or(
                    DeclarationReplayResolutionError::Excluded(
                        DeclarationReplayExclusion::SyntheticWithoutOriginal,
                    ),
                )?;
                Ok((coordinate, *value))
            })
            .collect::<Result<BTreeSet<_>, DeclarationReplayResolutionError>>()?;

        Ok(DeclarationReplayPreparation {
            invocation,
            expected,
            expected_paint,
            expected_shadow_symbol,
            expected_shadow_module,
            input_mismatches,
        })
    }

    fn declaration_replay_generic_first_node(
        &self,
        args: &[serde_json::Value],
        file_map: &DeclarationReplayFileMap,
    ) -> Result<NodeId, DeclarationReplayResolutionError> {
        self.declaration_replay_resolve_node(
            args.get(3)
                .ok_or_else(|| "generic entry lacks first node".to_owned())?,
            file_map,
        )
    }

    fn declaration_replay_expected_decision(
        &self,
        member: &str,
        args: &[serde_json::Value],
        file_map: &DeclarationReplayFileMap,
    ) -> Result<DeclarationReplayExpectedDecision, DeclarationReplayResolutionError> {
        match member {
            "resolver.isSymbolAccessible" | "resolver.isEntityNameVisible" => {
                let accessibility = args
                    .get(1)
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| "accessibility result lacks accessibility".to_owned())?;
                let error_symbol = args
                    .get(2)
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "accessibility result lacks errorSymbolName".to_owned())?
                    .to_owned();
                let error_module = match args.get(3) {
                    Some(serde_json::Value::Null) => None,
                    Some(value) => Some(
                        value
                            .as_str()
                            .ok_or_else(|| {
                                "accessibility result has invalid errorModuleName".to_owned()
                            })?
                            .to_owned(),
                    ),
                    None => {
                        return Err(DeclarationReplayResolutionError::Invalid(
                            "accessibility result lacks errorModuleName".to_owned(),
                        ));
                    }
                };
                let error_node = declaration_replay_expected_optional_node(
                    args.get(4)
                        .ok_or_else(|| "accessibility result lacks errorNode".to_owned())?,
                    file_map,
                )?;
                let aliases = match args.get(5) {
                    Some(serde_json::Value::Null) => serde_json::Value::Null,
                    Some(serde_json::Value::Array(values)) => serde_json::Value::Array(
                        values
                            .iter()
                            .map(|value| {
                                declaration_replay_expected_required_node(value, file_map)
                                    .map(DeclarationReplayCoordinate::json)
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                    _ => {
                        return Err(DeclarationReplayResolutionError::Invalid(
                            "accessibility result has invalid aliases".to_owned(),
                        ));
                    }
                };
                Ok((
                    serde_json::json!({
                        "kind": "accessibility",
                        "accessibility": accessibility,
                        "error_node": error_node.map_or(serde_json::Value::Null, DeclarationReplayCoordinate::json),
                        "aliases": aliases,
                    }),
                    Some(error_symbol),
                    Some(error_module),
                ))
            }
            "resolver.getPropertiesOfContainerFunction" => {
                let rows = args
                    .get(1)
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| "property result lacks rows".to_owned())?;
                let rows = rows
                    .iter()
                    .map(|row| {
                        let row = row
                            .as_array()
                            .filter(|row| row.len() == 3)
                            .ok_or_else(|| "property row is malformed".to_owned())?;
                        let name = row[0]
                            .as_str()
                            .ok_or_else(|| "property row lacks name".to_owned())?;
                        let parent = declaration_replay_expected_symbol(&row[1], file_map)?;
                        let value_declaration = match &row[2] {
                            serde_json::Value::Null => serde_json::Value::Null,
                            value => {
                                declaration_replay_expected_required_node(value, file_map)?.json()
                            }
                        };
                        Ok(serde_json::json!([name, parent, value_declaration]))
                    })
                    .collect::<Result<Vec<_>, DeclarationReplayResolutionError>>()?;
                Ok((
                    serde_json::json!({"kind": "properties", "rows": rows}),
                    None,
                    None,
                ))
            }
            "resolver.getEnumMemberValue" => {
                let value_type = args
                    .get(1)
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "enum result lacks value type".to_owned())?;
                let value = args
                    .get(2)
                    .cloned()
                    .ok_or_else(|| "enum result lacks value".to_owned())?;
                let syntactically_string = args
                    .get(3)
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| "enum result lacks string marker".to_owned())?;
                Ok((
                    serde_json::json!({
                        "kind": "enum",
                        "value_type": value_type,
                        "value": value,
                        "is_syntactically_string": syntactically_string,
                    }),
                    None,
                    None,
                ))
            }
            "resolver.collectLinkedAliases" => {
                Ok((serde_json::json!({"kind": "void"}), None, None))
            }
            _ => {
                let value = args
                    .get(1)
                    .and_then(serde_json::Value::as_array)
                    .and_then(|scalar| scalar.get(3))
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| "boolean result lacks scalar value".to_owned())?;
                Ok((
                    serde_json::json!({"kind": "boolean", "value": value}),
                    None,
                    None,
                ))
            }
        }
    }

    fn declaration_replay_invoke(
        &mut self,
        invocation: DeclarationReplayInvocation,
    ) -> Result<DeclarationReplayDecision, String> {
        let result = match invocation {
            DeclarationReplayInvocation::SymbolAccessible {
                symbol,
                enclosing,
                meaning,
                should_compute_aliases,
            } => self
                .emit_is_symbol_accessible(symbol, enclosing, meaning, should_compute_aliases)
                .map(DeclarationReplayDecision::Accessibility),
            DeclarationReplayInvocation::EntityNameVisible {
                entity_name,
                enclosing,
                should_compute_aliases,
            } => self
                .emit_is_entity_name_visible(entity_name, enclosing, should_compute_aliases)
                .map(DeclarationReplayDecision::Accessibility),
            DeclarationReplayInvocation::RequiresImplicitUndefined {
                parameter,
                enclosing,
            } => self
                .emit_requires_adding_implicit_undefined(parameter, enclosing)
                .map(DeclarationReplayDecision::Boolean),
            DeclarationReplayInvocation::CollectLinkedAliases {
                node,
                set_visibility,
            } => self
                .collect_linked_aliases(node, set_visibility)
                .map(|_| DeclarationReplayDecision::Void),
            DeclarationReplayInvocation::Serialization {
                member,
                node,
                enclosing,
                no_syntactic_printer,
            } => {
                return self.declaration_replay_invoke_serialization(
                    &member,
                    node,
                    enclosing,
                    no_syntactic_printer,
                )
            }
            DeclarationReplayInvocation::Unary { member, node } => match member.as_str() {
                "resolver.isDefinitelyReferenceToGlobalSymbolObject" => self
                    .emit_is_definitely_reference_to_global_symbol_object(node)
                    .map(DeclarationReplayDecision::Boolean),
                "resolver.isDeclarationVisible" => self
                    .emit_is_declaration_visible(node)
                    .map(DeclarationReplayDecision::Boolean),
                "resolver.isOptionalParameter" => self
                    .emit_is_optional_parameter(node)
                    .map(DeclarationReplayDecision::Boolean),
                "resolver.isImplementationOfOverload" => self
                    .emit_is_implementation_of_overload(node)
                    .map(DeclarationReplayDecision::Boolean),
                "resolver.isExpandoFunctionDeclaration" => self
                    .emit_is_expando_function_declaration(node)
                    .map(DeclarationReplayDecision::Boolean),
                "resolver.getPropertiesOfContainerFunction" => self
                    .emit_get_properties_of_container_function(node, 0)
                    .map(DeclarationReplayDecision::Properties),
                "resolver.getEnumMemberValue" => self
                    .get_enum_member_value(node)
                    .map(DeclarationReplayDecision::Enum),
                "resolver.isLiteralConstDeclaration" => self
                    .emit_is_literal_const_declaration(node)
                    .map(DeclarationReplayDecision::Boolean),
                "resolver.isLateBound" => self
                    .emit_is_late_bound(node)
                    .map(DeclarationReplayDecision::Boolean),
                "resolver.isImportRequiredByAugmentation" => self
                    .emit_is_import_required_by_augmentation(node)
                    .map(DeclarationReplayDecision::Boolean),
                other => return Err(format!("unsupported replay member {other}")),
            },
        };
        result.map_err(|abort| format!("checker aborted: {}", abort.description()))
    }

    /// h2-7a-m-3 §6: invoke one traced serialization root against the live
    /// foundation. The arena mounts every Program source so parse-node
    /// provenance projects; the sink records the decision lanes; the five
    /// transformer-driven members run under the pinned declaration-emit
    /// words (with the :115425 internal variant when the trace recovered
    /// it); createLiteralConstValue takes no flag words.
    /// h2-7a-m-3 §6.2-6.4: decode the traced serialization root — its
    /// result tail and every nested decision-lane event — into the same
    /// JSON shape the actual-side projector emits.
    fn declaration_replay_expected_serialized(
        &self,
        root: &DeclarationReplayRoot,
        file_map: &DeclarationReplayFileMap,
    ) -> Result<serde_json::Value, String> {
        let result_args = replay_array_field(&root.result, "args")?;
        let is_null = result_args
            .get(2)
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| "serialization result lacks the null marker".to_owned())?;
        let array_len = result_args
            .get(4)
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| "serialization result lacks the array length".to_owned())?;
        let produced = if is_null {
            serde_json::json!({"class": "absent"})
        } else if array_len >= 0 {
            serde_json::json!({"class": "container", "length": array_len})
        } else {
            let reference = result_args
                .get(3)
                .ok_or_else(|| "serialization result lacks the node reference".to_owned())?;
            declaration_replay_trace_ref_json(reference, file_map, false)?
        };
        let mut events = Vec::new();
        for event in &root.nested_decision_events {
            let site = replay_string_field(event, "site_id")?;
            let args = replay_array_field(event, "args")?;
            if site == "nodebuilder.withContext.result" {
                events.push(serde_json::json!({
                    "site": site,
                    "status": args.get(1).cloned().unwrap_or(serde_json::Value::Null),
                    "flags": args.get(2).cloned().unwrap_or(serde_json::Value::Null),
                    "internal_flags": args.get(3).cloned().unwrap_or(serde_json::Value::Null),
                    "approximate_length": args.get(4).cloned().unwrap_or(serde_json::Value::Null),
                    "type_stack_len": args.get(5).cloned().unwrap_or(serde_json::Value::Null),
                    "truncating": args.get(6).cloned().unwrap_or(serde_json::Value::Null),
                    "out_truncated": args.get(7).cloned().unwrap_or(serde_json::Value::Null),
                    "encountered_error": args.get(8).cloned().unwrap_or(serde_json::Value::Null),
                    "produced": if args.get(1).and_then(serde_json::Value::as_str)
                        == Some("fallback-undefined")
                    {
                        serde_json::json!({"class": "absent"})
                    } else {
                        declaration_replay_trace_ref_json(
                            args.get(9)
                                .ok_or_else(|| "withContext result lacks a node".to_owned())?,
                            file_map,
                            true,
                        )?
                    },
                }));
                continue;
            }
            if site == "syntactic.serializeTypeOfDeclaration.entry"
                || site == "syntactic.serializeReturnTypeForSignature.entry"
            {
                continue;
            }
            if let Some(base) = site.strip_suffix(".result") {
                if base.starts_with("syntactic.") {
                    events.push(serde_json::json!({
                        "site": base,
                        "frame": true,
                        "fallback": args.get(2).cloned().unwrap_or(serde_json::Value::Null),
                        "produced": declaration_replay_trace_ref_json(
                            args.get(3)
                                .ok_or_else(|| "syntactic frame lacks a node".to_owned())?,
                            file_map,
                            true,
                        )?,
                    }));
                    continue;
                }
            }
            if site.ends_with(".checkerFallback") {
                events.push(serde_json::json!({
                    "site": site,
                    "report_fallback": args.get(1).cloned().unwrap_or(serde_json::Value::Null),
                }));
                continue;
            }
            if site == "tracker.trackSymbol" {
                // Traced shape (:414): [site, name(symbol), nodeRef(enclosing),
                // meaning] — the transformer probe records the NAME string.
                events.push(serde_json::json!({
                    "site": site,
                    "payload": {
                        "name": args.get(1).cloned().unwrap_or(serde_json::Value::Null),
                        "node": declaration_replay_trace_ref_coordinate_json(
                            args.get(2)
                                .ok_or_else(|| "trackSymbol lacks a node".to_owned())?,
                            file_map,
                        )?,
                        "meaning": args.get(3).cloned().unwrap_or(serde_json::Value::Null),
                    },
                }));
                continue;
            }
            if site == "tracker.reportInferenceFallback" {
                events.push(serde_json::json!({
                    "site": site,
                    "payload": declaration_replay_trace_ref_coordinate_json(
                        args.get(1)
                            .ok_or_else(|| "reportInferenceFallback lacks a node".to_owned())?,
                        file_map,
                    )?,
                }));
                continue;
            }
            if let Some(rest) = site.strip_prefix("tracker.") {
                let _ = rest;
                let payload = match args.len() {
                    1 => serde_json::Value::Null,
                    2 => args[1].clone(),
                    _ => serde_json::Value::Array(args[1..].to_vec()),
                };
                events.push(serde_json::json!({"site": site, "payload": payload}));
                continue;
            }
            // Expected-zero lanes (§6.4): a traced event on a zero lane is
            // carried verbatim so the actual side reds against it.
            events.push(serde_json::json!({"site": site, "raw": args[1..].to_vec()}));
        }
        Ok(serde_json::json!({
            "kind": "serialized",
            "produced": produced,
            "events": events,
        }))
    }

    fn declaration_replay_invoke_serialization(
        &mut self,
        member: &str,
        node: NodeId,
        enclosing: Option<NodeId>,
        no_syntactic_printer: bool,
    ) -> Result<DeclarationReplayDecision, String> {
        let mut arena = tsc_emitter::TransformArena::new();
        let mut target = None;
        let node_file = self.binder.file_index_of_node(node);
        for index in 0..self.binder.file_count() {
            let source_id = arena.add_source(
                self.binder.source(index),
                Some(tsc_program::SourceFileId::from_raw(
                    u32::try_from(index).map_err(|_| "source index overflow".to_owned())?,
                )),
            );
            if index == node_file {
                target = Some(source_id);
            }
        }
        let target = target.ok_or_else(|| "root node file is not mounted".to_owned())?;
        let flags = tsc_emitter::EmitNodeBuilderFlags::DECLARATION_EMIT;
        let internal = if no_syntactic_printer {
            tsc_emitter::EmitInternalNodeBuilderFlags::DECLARATION_EMIT
                .union(tsc_emitter::EmitInternalNodeBuilderFlags::NO_SYNTACTIC_PRINTER)
        } else {
            tsc_emitter::EmitInternalNodeBuilderFlags::DECLARATION_EMIT
        };
        let mut tracker = DeclarationReplayRecordingTracker;
        crate::node_builder::replay_sink::arm();
        let outcome: Result<
            crate::node_builder::replay_sink::ProducedClass,
            tsc_emitter::EmitResolverError,
        > = {
            use crate::node_builder::replay_sink::ProducedClass;
            let enclosing_or_root = enclosing.unwrap_or_else(|| self.binder.source(node_file).root);
            match member {
                "resolver.createTypeOfDeclaration" => self
                    .emit_create_type_of_declaration(
                        &mut arena,
                        target,
                        node,
                        enclosing_or_root,
                        flags,
                        internal,
                        &mut tracker,
                    )
                    .map(|produced| match produced {
                        None => ProducedClass::Absent,
                        Some(node) => crate::node_builder::transform_node_class(&arena, node),
                    }),
                "resolver.createReturnTypeOfSignatureDeclaration" => self
                    .emit_create_return_type_of_signature_declaration(
                        &mut arena,
                        target,
                        node,
                        enclosing_or_root,
                        flags,
                        internal,
                        &mut tracker,
                    )
                    .map(|produced| match produced {
                        None => ProducedClass::Absent,
                        Some(node) => crate::node_builder::transform_node_class(&arena, node),
                    }),
                "resolver.createTypeOfExpression" => self
                    .emit_create_type_of_expression(
                        &mut arena,
                        target,
                        node,
                        enclosing_or_root,
                        flags,
                        internal,
                        &mut tracker,
                    )
                    .map(|produced| match produced {
                        None => ProducedClass::Absent,
                        Some(node) => crate::node_builder::transform_node_class(&arena, node),
                    }),
                "resolver.createLiteralConstValue" => self
                    .emit_create_literal_const_value(&mut arena, target, node, &mut tracker)
                    .map(|node| crate::node_builder::transform_node_class(&arena, node)),
                "resolver.getDeclarationStatementsForSourceFile" => self
                    .emit_get_declaration_statements_for_source_file(
                        &mut arena,
                        target,
                        node,
                        flags,
                        internal,
                        &mut tracker,
                    )
                    .map(|produced| match produced {
                        None => ProducedClass::Absent,
                        Some(nodes) => ProducedClass::Container {
                            length: nodes.len(),
                        },
                    }),
                "resolver.createLateBoundIndexSignatures" => self
                    .emit_create_late_bound_index_signatures(
                        &mut arena,
                        target,
                        node,
                        enclosing_or_root,
                        flags,
                        internal,
                        &mut tracker,
                    )
                    .map(|produced| match produced {
                        None => ProducedClass::Absent,
                        Some(nodes) => ProducedClass::Container {
                            length: nodes.len(),
                        },
                    }),
                other => Err(tsc_emitter::EmitResolverError::CheckerAborted {
                    method: tsc_emitter::EmitResolverMethod::CreateTypeOfDeclaration,
                    node: EmitResolverNode::from_raw_source(
                        u32::try_from(node_file).unwrap_or(0),
                        node,
                    ),
                    reason: if other.is_empty() {
                        "empty member"
                    } else {
                        "unsupported serialization member"
                    },
                }),
            }
        };
        let events = crate::node_builder::replay_sink::disarm();
        match outcome {
            Ok(produced) => Ok(DeclarationReplayDecision::Serialized { produced, events }),
            Err(error) => Err(format!("serialization member failed: {error}")),
        }
    }

    fn declaration_replay_project_decision(
        &self,
        decision: &DeclarationReplayDecision,
        file_map: &DeclarationReplayFileMap,
    ) -> Result<serde_json::Value, String> {
        match decision {
            DeclarationReplayDecision::Boolean(value) => {
                Ok(serde_json::json!({"kind": "boolean", "value": value}))
            }
            DeclarationReplayDecision::Void => Ok(serde_json::json!({"kind": "void"})),
            DeclarationReplayDecision::Serialized { produced, events } => {
                let events = events
                    .iter()
                    .map(|event| self.declaration_replay_actual_event_json(event, file_map))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(serde_json::json!({
                    "kind": "serialized",
                    "produced": self.declaration_replay_actual_produced_json(
                        produced, file_map, false,
                    )?,
                    "events": events,
                }))
            }
            DeclarationReplayDecision::Enum(result) => {
                let (value_type, value) = match &result.value {
                    Some(crate::evaluate::EvalValue::Str(value)) => {
                        ("string", serde_json::json!(value))
                    }
                    Some(crate::evaluate::EvalValue::Num(value)) => {
                        let number = serde_json::Number::from_f64(*value)
                            .ok_or_else(|| "Rust enum value is not JSON-finite".to_owned())?;
                        ("number", serde_json::Value::Number(number))
                    }
                    None => ("undefined", serde_json::Value::Null),
                };
                Ok(serde_json::json!({
                    "kind": "enum",
                    "value_type": value_type,
                    "value": value,
                    "is_syntactically_string": result.is_syntactically_string,
                }))
            }
            DeclarationReplayDecision::Accessibility(result) => {
                let error_node = result
                    .error_node
                    .map(|node| self.declaration_replay_project_node(node.node(), file_map))
                    .transpose()?
                    .map_or(serde_json::Value::Null, DeclarationReplayCoordinate::json);
                let aliases = match &result.aliases_to_make_visible {
                    Some(aliases) => serde_json::Value::Array(
                        aliases
                            .iter()
                            .map(|node| {
                                self.declaration_replay_project_node(node.node(), file_map)
                                    .map(DeclarationReplayCoordinate::json)
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                    None => serde_json::Value::Null,
                };
                Ok(serde_json::json!({
                    "kind": "accessibility",
                    "accessibility": result.accessibility as u8,
                    "error_node": error_node,
                    "aliases": aliases,
                }))
            }
            DeclarationReplayDecision::Properties(properties) => {
                let rows = properties
                    .iter()
                    .map(|property| {
                        let parent = self.declaration_replay_project_symbol(
                            SymbolId(property.parent.symbol_index),
                            file_map,
                        )?;
                        let value_declaration = property
                            .value_declaration
                            .map(|node| self.declaration_replay_project_node(node.node(), file_map))
                            .transpose()?
                            .map_or(serde_json::Value::Null, DeclarationReplayCoordinate::json);
                        Ok(serde_json::json!([
                            property.name,
                            parent,
                            value_declaration
                        ]))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(serde_json::json!({"kind": "properties", "rows": rows}))
            }
        }
    }

    fn declaration_replay_resolve_symbol(
        &mut self,
        value: &serde_json::Value,
        file_map: &DeclarationReplayFileMap,
    ) -> Result<(SymbolId, Option<String>), DeclarationReplayResolutionError> {
        let values = value
            .as_array()
            .filter(|values| values.len() == 3)
            .ok_or_else(|| "symbol reference is malformed".to_owned())?;
        let declaration_count = values[1]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| "symbol reference has invalid declaration count".to_owned())?;
        if declaration_count == 0 {
            return Err(DeclarationReplayResolutionError::Excluded(
                DeclarationReplayExclusion::ZeroDeclarationSymbol,
            ));
        }
        let declarations = values[2]
            .as_array()
            .ok_or_else(|| "symbol reference lacks declaration prefix".to_owned())?
            .iter()
            .map(|node| self.declaration_replay_resolve_node(node, file_map))
            .collect::<Result<Vec<_>, _>>()?;
        let mut candidates = BTreeSet::new();
        for declaration in declarations.iter().copied() {
            if let Some(symbol) = self.node_symbol(declaration) {
                candidates.insert(self.get_merged_symbol(symbol));
            }
            let symbol = self
                .get_symbol_of_declaration(declaration)
                .map_err(|abort| format!("symbol resolution aborted: {}", abort.description()))?;
            if symbol != self.unknown_symbol {
                candidates.insert(symbol);
            }
        }
        if candidates.is_empty() {
            return Err(DeclarationReplayResolutionError::Invalid(
                "symbol declaration prefix resolves to no Rust symbol".to_owned(),
            ));
        }
        let expected = declaration_replay_expected_symbol(value, file_map)?;
        let exact = candidates
            .iter()
            .copied()
            .filter(|&symbol| {
                self.declaration_replay_project_symbol(symbol, file_map)
                    .is_ok_and(|actual| actual == expected)
            })
            .collect::<Vec<_>>();
        if exact.len() > 1 {
            return Err(DeclarationReplayResolutionError::Excluded(
                DeclarationReplayExclusion::AmbiguousSymbol,
            ));
        }
        if let Some(symbol) = exact.first() {
            return Ok((*symbol, None));
        }
        if candidates.len() > 1 {
            return Err(DeclarationReplayResolutionError::Excluded(
                DeclarationReplayExclusion::AmbiguousSymbol,
            ));
        }
        let symbol = *candidates.iter().next().expect("nonempty candidates");
        let actual = self.declaration_replay_project_symbol(symbol, file_map)?;
        Ok((
            symbol,
            Some(format!(
                "symbol input differs: expected {expected}, actual {actual}"
            )),
        ))
    }

    fn declaration_replay_project_symbol(
        &self,
        symbol: SymbolId,
        file_map: &DeclarationReplayFileMap,
    ) -> Result<serde_json::Value, String> {
        let data = self.binder.symbol(symbol);
        let declarations = data
            .declarations
            .iter()
            .take(8)
            .map(|&node| {
                self.declaration_replay_project_node(node, file_map)
                    .map(DeclarationReplayCoordinate::json)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(serde_json::json!([
            data.escaped_name,
            data.declarations.len(),
            declarations
        ]))
    }
}

fn declaration_replay_expected_required_node(
    value: &serde_json::Value,
    file_map: &DeclarationReplayFileMap,
) -> Result<DeclarationReplayCoordinate, DeclarationReplayResolutionError> {
    declaration_replay_coordinate(value, file_map, true)?.ok_or(
        DeclarationReplayResolutionError::Excluded(
            DeclarationReplayExclusion::SyntheticWithoutOriginal,
        ),
    )
}

fn declaration_replay_expected_optional_node(
    value: &serde_json::Value,
    file_map: &DeclarationReplayFileMap,
) -> Result<Option<DeclarationReplayCoordinate>, DeclarationReplayResolutionError> {
    declaration_replay_coordinate(value, file_map, true)
}

fn declaration_replay_expected_symbol(
    value: &serde_json::Value,
    file_map: &DeclarationReplayFileMap,
) -> Result<serde_json::Value, DeclarationReplayResolutionError> {
    let values = value
        .as_array()
        .filter(|values| values.len() == 3)
        .ok_or_else(|| "symbol reference is malformed".to_owned())?;
    let name = values[0]
        .as_str()
        .ok_or_else(|| "symbol reference lacks name".to_owned())?;
    let declaration_count = values[1]
        .as_u64()
        .ok_or_else(|| "symbol reference lacks declaration count".to_owned())?;
    if declaration_count == 0 {
        return Err(DeclarationReplayResolutionError::Excluded(
            DeclarationReplayExclusion::ZeroDeclarationSymbol,
        ));
    }
    let declarations = values[2]
        .as_array()
        .ok_or_else(|| "symbol reference lacks declarations".to_owned())?
        .iter()
        .map(|value| {
            declaration_replay_expected_required_node(value, file_map)
                .map(DeclarationReplayCoordinate::json)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(serde_json::json!([name, declaration_count, declarations]))
}

fn declaration_replay_shadow_divergences(
    decision: &DeclarationReplayDecision,
    expected_symbol: &Option<String>,
    expected_module: &Option<Option<String>>,
) -> u64 {
    let DeclarationReplayDecision::Accessibility(result) = decision else {
        return 0;
    };
    let mut divergences = 0;
    if let Some(expected) = expected_symbol {
        if result.error_symbol_name.as_deref().unwrap_or("") != expected {
            divergences += 1;
        }
    }
    if let Some(expected) = expected_module {
        if &result.error_module_name != expected {
            divergences += 1;
        }
    }
    divergences
}

// ---------------------------------------------------------------------------
// h2-7a-m-3 P4: the seven NodeBuilder-backed resolver member workers.
// ---------------------------------------------------------------------------

/// tsc-port: hasInferredType @6.0.3
/// tsc-hash: cced5328b76bdeff714b0e84710f4bf95169d5fe52f969c0e517480515dd3c22
/// tsc-span: _tsc.js:19921-19942
fn has_inferred_type(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Parameter
            | SyntaxKind::PropertySignature
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::BindingElement
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::BinaryExpression
            | SyntaxKind::VariableDeclaration
            | SyntaxKind::ExportAssignment
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::JSDocParameterTag
            | SyntaxKind::JSDocPropertyTag
    )
}

fn node_builder_abort_error(
    checker: &CheckerState<'_>,
    method: tsc_emitter::EmitResolverMethod,
    node: NodeId,
    abort: crate::state::CheckAbort,
) -> tsc_emitter::EmitResolverError {
    let source = u32::try_from(checker.binder.file_index_of_node(node)).unwrap_or(0);
    tsc_emitter::EmitResolverError::CheckerAborted {
        method,
        node: EmitResolverNode::from_raw_source(source, node),
        reason: abort.description(),
    }
}

fn node_builder_factory_error(
    method: tsc_emitter::EmitResolverMethod,
    error: tsc_emitter::TransformError,
) -> tsc_emitter::EmitResolverError {
    tsc_emitter::EmitResolverError::Factory {
        method,
        error: Box::new(error),
    }
}

fn any_keyword_fallback(
    arena: &mut tsc_emitter::TransformArena,
    target: tsc_emitter::TransformSourceId,
    method: tsc_emitter::EmitResolverMethod,
) -> Result<tsc_emitter::TransformNode, tsc_emitter::EmitResolverError> {
    arena
        .factory()
        .create_token(
            target,
            SyntaxKind::AnyKeyword,
            tsc_emitter::TransformFlags::NONE,
        )
        .map_err(|error| node_builder_factory_error(method, error))
}

impl<'a> CheckerState<'a> {
    /// tsc-port: createTypeOfDeclaration @6.0.3
    /// tsc-hash: e7208311f25e05e9154f95d68bf3d6c1e0a2fa42f4e8197d11a2dc83a462c624
    /// tsc-span: _tsc.js:88359-88366
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_create_type_of_declaration(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        declaration: NodeId,
        enclosing_declaration: NodeId,
        flags: tsc_emitter::EmitNodeBuilderFlags,
        internal_flags: tsc_emitter::EmitInternalNodeBuilderFlags,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<Option<tsc_emitter::TransformNode>, tsc_emitter::EmitResolverError> {
        let method = tsc_emitter::EmitResolverMethod::CreateTypeOfDeclaration;
        // getParseTreeNode(declarationIn, hasInferredType): the resolver
        // boundary already guarantees parse-tree identity; the kind filter
        // remains.
        if !has_inferred_type(self.kind_of(declaration)) {
            return any_keyword_fallback(arena, target, method).map(Some);
        }
        let symbol = self
            .get_symbol_of_declaration(declaration)
            .map_err(|abort| node_builder_abort_error(self, method, declaration, abort))?;
        crate::node_builder::serialize_type_for_declaration(
            self,
            arena,
            target,
            declaration,
            symbol,
            Some(enclosing_declaration),
            Some(flags.union(tsc_emitter::EmitNodeBuilderFlags::MULTILINE_OBJECT_LITERALS)),
            Some(internal_flags),
            Some(tracker),
        )
    }

    /// tsc-port: createReturnTypeOfSignatureDeclaration @6.0.3
    /// tsc-hash: afee5b310b2c60519f7fdfe73b676da237a0a34b6f3ae97a60a3674b892406b6
    /// tsc-span: _tsc.js:88382-88388
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_create_return_type_of_signature_declaration(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        signature_declaration: NodeId,
        enclosing_declaration: NodeId,
        flags: tsc_emitter::EmitNodeBuilderFlags,
        internal_flags: tsc_emitter::EmitInternalNodeBuilderFlags,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<Option<tsc_emitter::TransformNode>, tsc_emitter::EmitResolverError> {
        let method = tsc_emitter::EmitResolverMethod::CreateReturnTypeOfSignatureDeclaration;
        if !node_util::is_function_like_kind(self.kind_of(signature_declaration)) {
            return any_keyword_fallback(arena, target, method).map(Some);
        }
        crate::node_builder::serialize_return_type_for_signature(
            self,
            arena,
            target,
            signature_declaration,
            Some(enclosing_declaration),
            Some(flags.union(tsc_emitter::EmitNodeBuilderFlags::MULTILINE_OBJECT_LITERALS)),
            Some(internal_flags),
            Some(tracker),
        )
    }

    /// tsc-port: createTypeOfExpression @6.0.3
    /// tsc-hash: dd314f61d3160f871fe3d2568358c718dbca65cc107f1668ef3d0f6f79611fb4
    /// tsc-span: _tsc.js:88389-88395
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_create_type_of_expression(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        expression: NodeId,
        enclosing_declaration: NodeId,
        flags: tsc_emitter::EmitNodeBuilderFlags,
        internal_flags: tsc_emitter::EmitInternalNodeBuilderFlags,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<Option<tsc_emitter::TransformNode>, tsc_emitter::EmitResolverError> {
        let method = tsc_emitter::EmitResolverMethod::CreateTypeOfExpression;
        let source = self.binder.source_of_node(expression);
        if !node_util::is_expression_node(source, expression) {
            return any_keyword_fallback(arena, target, method).map(Some);
        }
        crate::node_builder::serialize_type_for_expression(
            self,
            arena,
            target,
            expression,
            Some(enclosing_declaration),
            Some(flags.union(tsc_emitter::EmitNodeBuilderFlags::MULTILINE_OBJECT_LITERALS)),
            Some(internal_flags),
            Some(tracker),
        )
    }
}

impl<'a> CheckerState<'a> {
    /// tsc-port: literalTypeToNode @6.0.3
    /// tsc-hash: f572c2d3b26803a05c3a3512e3fd391bea5b5a98505ebaff350e23f974280e0c
    /// tsc-span: _tsc.js:88491-88505
    #[allow(clippy::too_many_arguments)]
    fn emit_literal_type_to_node(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        literal_type: tsc_types::TypeId,
        enclosing_declaration: NodeId,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<tsc_emitter::TransformNode, tsc_emitter::EmitResolverError> {
        use tsc_types::{LiteralValue, TypeData, TypeFlags};
        let method = tsc_emitter::EmitResolverMethod::CreateLiteralConstValue;
        let flags = self.tables.flags_of(literal_type);
        if flags.contains(TypeFlags::ENUM_LIKE) {
            let symbol = self.tables.type_of(literal_type).symbol.ok_or(
                tsc_emitter::EmitResolverError::CheckerAborted {
                    method,
                    node: EmitResolverNode::from_raw_source(
                        u32::try_from(self.binder.file_index_of_node(enclosing_declaration))
                            .unwrap_or(0),
                        enclosing_declaration,
                    ),
                    reason: "enum-like literal type has no symbol",
                },
            )?;
            let node = crate::node_builder::with_context(
                self,
                arena,
                target,
                Some(enclosing_declaration),
                None,
                None,
                Some(tracker),
                None,
                None,
                |checker, arena, target, context| {
                    crate::node_builder::chains_symbol_to_expression(
                        checker,
                        arena,
                        target,
                        context,
                        symbol,
                        tsc_emitter::EmitSymbolMeaning(111_551),
                    )
                    .map(Some)
                },
                None,
            )?
            .flatten();
            if let Some(node) = node {
                return Ok(node);
            }
            // encounteredError inside the enum arm falls through to the
            // literal-value arms exactly as upstream's falsy `enumResult`.
        }
        if flags.contains(TypeFlags::BOOLEAN_LITERAL) {
            let is_true = matches!(
                &self.tables.type_of(literal_type).data,
                TypeData::Intrinsic { name: "true", .. }
            );
            let kind = if is_true {
                SyntaxKind::TrueKeyword
            } else {
                SyntaxKind::FalseKeyword
            };
            return arena
                .factory()
                .create_token(target, kind, tsc_emitter::TransformFlags::NONE)
                .map_err(|error| node_builder_factory_error(method, error));
        }
        let value = match &self.tables.type_of(literal_type).data {
            TypeData::Literal { value } => value.clone(),
            _ => {
                return Err(tsc_emitter::EmitResolverError::CheckerAborted {
                    method,
                    node: EmitResolverNode::from_raw_source(
                        u32::try_from(self.binder.file_index_of_node(enclosing_declaration))
                            .unwrap_or(0),
                        enclosing_declaration,
                    ),
                    reason: "literal-const type is not a literal",
                })
            }
        };
        let mut factory = arena.factory();
        let map_factory = |error| node_builder_factory_error(method, error);
        match value {
            LiteralValue::BigInt(pseudo) => factory
                .create_node(
                    target,
                    NodeData::BigIntLiteral(tsc_syntax::nodes::BigIntLiteralData {
                        text: format!("{}n", pseudo.to_base10_string()),
                    }),
                    tsc_emitter::TransformFlags::NONE,
                )
                .map_err(map_factory),
            LiteralValue::String(text) => {
                let text = text.to_utf8().ok_or({
                    // The lane-BD convention: unpaired UTF-16 payloads are a
                    // typed m-3.5 factory-face residue, never lossy storage.
                    tsc_emitter::EmitResolverError::CheckerAborted {
                        method,
                        node: EmitResolverNode::from_raw_source(
                            u32::try_from(self.binder.file_index_of_node(enclosing_declaration))
                                .unwrap_or(0),
                            enclosing_declaration,
                        ),
                        reason:
                            "unpaired UTF-16 string literal requires the m-3.5 UTF-16 factory face",
                    }
                })?;
                factory
                    .create_node(
                        target,
                        NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
                            text,
                            has_extended_unicode_escape: None,
                        }),
                        tsc_emitter::TransformFlags::NONE,
                    )
                    .map_err(map_factory)
            }
            LiteralValue::Number(number) if number < 0.0 => {
                let operand = factory
                    .create_node(
                        target,
                        NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                            text: tsc_types::js_number_to_string(-number),
                        }),
                        tsc_emitter::TransformFlags::NONE,
                    )
                    .map_err(map_factory)?;
                factory
                    .create_node(
                        target,
                        NodeData::PrefixUnaryExpression(
                            tsc_syntax::nodes::PrefixUnaryExpressionData {
                                operator: SyntaxKind::MinusToken,
                                operand: Some(operand.node()),
                            },
                        ),
                        tsc_emitter::TransformFlags::NONE,
                    )
                    .map_err(map_factory)
            }
            LiteralValue::Number(number) => factory
                .create_node(
                    target,
                    NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                        text: tsc_types::js_number_to_string(number),
                    }),
                    tsc_emitter::TransformFlags::NONE,
                )
                .map_err(map_factory),
        }
    }

    /// tsc-port: createLiteralConstValue @6.0.3
    /// tsc-hash: aed30591a56b896560cdc11531e90bd746b037ffa64fa9d884cd9e384048ee53
    /// tsc-span: _tsc.js:88506-88509
    pub(crate) fn emit_create_literal_const_value(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        node: NodeId,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<tsc_emitter::TransformNode, tsc_emitter::EmitResolverError> {
        let method = tsc_emitter::EmitResolverMethod::CreateLiteralConstValue;
        let symbol = self
            .get_symbol_of_declaration(node)
            .map_err(|abort| node_builder_abort_error(self, method, node, abort))?;
        let literal_type = self
            .get_type_of_symbol(symbol)
            .map_err(|abort| node_builder_abort_error(self, method, node, abort))?;
        self.emit_literal_type_to_node(arena, target, literal_type, node, tracker)
    }
}

impl<'a> CheckerState<'a> {
    /// tsc-port: createLateBoundIndexSignatures @6.0.3 (resolver member)
    /// tsc-hash: 57a5aa62b412607a3d4c1fc9811e8e9ec66f85ef4aa82dab2cc6afe36885e6c9
    /// tsc-span: _tsc.js:88624-88691
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_create_late_bound_index_signatures(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        container: NodeId,
        enclosing_declaration: NodeId,
        flags: tsc_emitter::EmitNodeBuilderFlags,
        internal_flags: tsc_emitter::EmitInternalNodeBuilderFlags,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<Option<Vec<tsc_emitter::TransformNode>>, tsc_emitter::EmitResolverError> {
        crate::node_builder::late_bound_index_signatures(
            self,
            arena,
            target,
            container,
            enclosing_declaration,
            flags,
            internal_flags,
            tracker,
        )
    }

    /// tsc-port: getDeclarationStatementsForSourceFile @6.0.3
    /// tsc-hash: 517de08538d0b91488cd2e54201e7dc44b404b08fe126ba36a1b63ce84ec70dc
    /// tsc-span: _tsc.js:88612-88621
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_get_declaration_statements_for_source_file(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        node: NodeId,
        flags: tsc_emitter::EmitNodeBuilderFlags,
        internal_flags: tsc_emitter::EmitInternalNodeBuilderFlags,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<Option<Vec<tsc_emitter::TransformNode>>, tsc_emitter::EmitResolverError> {
        let method = tsc_emitter::EmitResolverMethod::GetDeclarationStatementsForSourceFile;
        if self.kind_of(node) != SyntaxKind::SourceFile {
            return Err(tsc_emitter::EmitResolverError::CheckerAborted {
                method,
                node: EmitResolverNode::from_raw_source(
                    u32::try_from(self.binder.file_index_of_node(node)).unwrap_or(0),
                    node,
                ),
                reason: "non-sourcefile node passed into getDeclarationsForSourceFile",
            });
        }
        let symbol = self.get_symbol_of_declaration_opt(node);
        let table = match symbol {
            None => match self.binder.locals_of(node) {
                None => return Ok(Some(Vec::new())),
                Some(locals) => locals.clone(),
            },
            Some(symbol) => {
                let resolved = self
                    .resolve_external_module_symbol(Some(symbol), false)
                    .map_err(|abort| node_builder_abort_error(self, method, node, abort))?;
                let _ = resolved;
                let exports = self
                    .get_exports_of_module(symbol)
                    .map_err(|abort| node_builder_abort_error(self, method, node, abort))?;
                if exports.is_empty() {
                    return Ok(Some(Vec::new()));
                }
                exports
            }
        };
        crate::node_builder::with_context(
            self,
            arena,
            target,
            Some(node),
            Some(flags),
            Some(internal_flags),
            Some(tracker),
            None,
            None,
            |checker, arena, target, context| {
                crate::node_builder::symbol_table_to_declaration_statements(
                    checker, arena, target, &table, context,
                )
            },
            None,
        )
    }
}

impl CheckerState<'_> {
    /// Project a sink produced-class into the trace coordinate space. At
    /// frame level (`opaque_frames`) synthesized results and containers are
    /// indistinguishable in the trace and both project as "opaque"
    /// (h2-7a-m-3 §6.3 frame-aware rule); at root level the classes stay
    /// distinct and the container carries its length.
    fn declaration_replay_actual_produced_json(
        &self,
        produced: &crate::node_builder::replay_sink::ProducedClass,
        file_map: &DeclarationReplayFileMap,
        opaque_frames: bool,
    ) -> Result<serde_json::Value, String> {
        use crate::node_builder::replay_sink::ProducedClass;
        Ok(match produced {
            ProducedClass::Absent => serde_json::json!({"class": "absent"}),
            ProducedClass::ParseOwn { node, .. }
            | ProducedClass::OriginalProjected { node, .. } => {
                let coordinate = self
                    .declaration_replay_project_node(NodeId(*node), file_map)
                    .map_err(|error| format!("produced-node projection failed: {error}"))?;
                let class = if matches!(produced, ProducedClass::ParseOwn { .. }) {
                    "parse"
                } else {
                    "original"
                };
                serde_json::json!({"class": class, "coordinate": coordinate.json()})
            }
            ProducedClass::SyntheticWithoutOriginal => {
                if opaque_frames {
                    serde_json::json!({"class": "opaque"})
                } else {
                    serde_json::json!({"class": "synthetic"})
                }
            }
            ProducedClass::Container { length } => {
                if opaque_frames {
                    serde_json::json!({"class": "opaque"})
                } else {
                    serde_json::json!({"class": "container", "length": length})
                }
            }
        })
    }

    fn declaration_replay_actual_raw_ref_json(
        &self,
        value: &serde_json::Value,
        file_map: &DeclarationReplayFileMap,
    ) -> Result<serde_json::Value, String> {
        if value.is_null() {
            return Ok(serde_json::Value::Null);
        }
        if value.as_str() == Some("opaque") {
            return Ok(serde_json::json!("opaque"));
        }
        let pair = value
            .as_array()
            .filter(|pair| pair.len() == 2)
            .ok_or_else(|| "raw tracker reference is not a pair".to_owned())?;
        let node = pair[1]
            .as_u64()
            .ok_or_else(|| "raw tracker reference node is not an integer".to_owned())?;
        let coordinate = self
            .declaration_replay_project_node(
                NodeId(u32::try_from(node).map_err(|_| "node id overflow".to_owned())?),
                file_map,
            )
            .map_err(|error| format!("tracker reference projection failed: {error}"))?;
        Ok(coordinate.json())
    }

    fn declaration_replay_actual_event_json(
        &self,
        event: &crate::node_builder::replay_sink::DecisionEvent,
        file_map: &DeclarationReplayFileMap,
    ) -> Result<serde_json::Value, String> {
        use crate::node_builder::replay_sink::DecisionEvent;
        Ok(match event {
            DecisionEvent::WithContextResult {
                status,
                flags,
                internal_flags,
                approximate_length,
                type_stack_len,
                truncating,
                out_truncated,
                encountered_error,
                produced,
            } => serde_json::json!({
                "site": "nodebuilder.withContext.result",
                "status": status,
                "flags": flags,
                "internal_flags": internal_flags,
                "approximate_length": approximate_length,
                "type_stack_len": type_stack_len,
                "truncating": truncating,
                "out_truncated": out_truncated,
                "encountered_error": encountered_error,
                "produced": self.declaration_replay_actual_produced_json(
                    produced, file_map, true,
                )?,
            }),
            DecisionEvent::SyntacticFrame {
                site,
                fallback,
                produced,
            } => serde_json::json!({
                "site": site,
                "frame": true,
                "fallback": fallback,
                "produced": self.declaration_replay_actual_produced_json(
                    produced, file_map, true,
                )?,
            }),
            DecisionEvent::SyntacticFallback {
                site,
                report_fallback,
            } => serde_json::json!({
                "site": format!("{site}.checkerFallback"),
                "report_fallback": report_fallback,
            }),
            DecisionEvent::Tracker { site, payload } => {
                let payload = match *site {
                    "tracker.trackSymbol" => serde_json::json!({
                        "name": payload["name"],
                        "node": self
                            .declaration_replay_actual_raw_ref_json(&payload["node"], file_map)?,
                        "meaning": payload["meaning"],
                    }),
                    "tracker.reportInferenceFallback" => {
                        self.declaration_replay_actual_raw_ref_json(payload, file_map)?
                    }
                    _ => payload.clone(),
                };
                serde_json::json!({"site": site, "payload": payload})
            }
        })
    }
}

/// Classify a traced eight-tuple node reference into the shared produced
/// JSON (parse/original coordinates; sentinel -> synthetic at root level or
/// opaque at frame level per the §6.3 frame-aware rule).
fn declaration_replay_trace_ref_json(
    reference: &serde_json::Value,
    file_map: &DeclarationReplayFileMap,
    opaque_frames: bool,
) -> Result<serde_json::Value, String> {
    let values = reference
        .as_array()
        .filter(|values| values.len() == 8)
        .ok_or_else(|| "trace node reference is not an eight-tuple".to_owned())?;
    let numbers = values
        .iter()
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| "trace node reference holds a non-integer".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (class, quad) = if numbers[0] >= 0 {
        ("parse", &numbers[0..4])
    } else if numbers[4] >= 0 {
        ("original", &numbers[4..8])
    } else {
        return Ok(if opaque_frames {
            serde_json::json!({"class": "opaque"})
        } else {
            serde_json::json!({"class": "synthetic"})
        });
    };
    let _ = file_map;
    Ok(serde_json::json!({
        "class": class,
        "coordinate": {
            "file_tag": quad[0],
            "kind": quad[1],
            "pos": quad[2],
            "end": quad[3],
        },
    }))
}

/// Tracker payload references: null stays null; sentinel tuples project as
/// "opaque"; coordinate tuples keep their quadruple.
fn declaration_replay_trace_ref_coordinate_json(
    reference: &serde_json::Value,
    file_map: &DeclarationReplayFileMap,
) -> Result<serde_json::Value, String> {
    if reference.is_null() {
        return Ok(serde_json::Value::Null);
    }
    let projected = declaration_replay_trace_ref_json(reference, file_map, true)?;
    if projected["class"] == "opaque" {
        return Ok(serde_json::json!("opaque"));
    }
    Ok(projected["coordinate"].clone())
}

#[cfg(test)]
mod node_builder_member_tests {
    use super::*;
    use crate::state::test_support::with_program_state;
    use tsc_emitter::{
        EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, EmitSymbolTracker, SourceFileId,
        TransformArena,
    };
    use tsc_types::CompilerOptions;

    struct NoopTracker;
    impl EmitSymbolTracker for NoopTracker {}

    fn with_member_state(
        source: &str,
        run: impl FnOnce(&mut CheckerState<'_>, &mut TransformArena, tsc_emitter::TransformSourceId),
    ) {
        with_program_state(
            &[("/main.ts", source)],
            &CompilerOptions::default(),
            |checker| {
                let mut arena = TransformArena::new();
                let target =
                    arena.add_source(checker.binder.source(0), Some(SourceFileId::from_raw(0)));
                run(checker, &mut arena, target);
            },
        );
    }

    fn node_kind(
        arena: &TransformArena,
        target: tsc_emitter::TransformSourceId,
        node: tsc_emitter::TransformNode,
    ) -> SyntaxKind {
        let _ = target;
        arena
            .source(node.source())
            .expect("transform source")
            .syntax()
            .arena
            .node(node.node())
            .kind
    }

    fn statement_at(checker: &CheckerState<'_>, index: usize) -> NodeId {
        let source = checker.binder.source(0);
        let NodeData::SourceFile(data) = &source.arena.node(source.root).data else {
            panic!("source file expected")
        };
        let statements = source
            .arena
            .node_array(data.statements.expect("statements"));
        statements.nodes[index]
    }

    fn first_variable_declaration(checker: &CheckerState<'_>, statement: usize) -> NodeId {
        let statement = statement_at(checker, statement);
        let source = checker.binder.source(0);
        let NodeData::VariableStatement(data) = &source.arena.node(statement).data else {
            panic!("variable statement expected")
        };
        let NodeData::VariableDeclarationList(list) = &source
            .arena
            .node(data.declaration_list.expect("declaration list"))
            .data
        else {
            panic!("declaration list expected")
        };
        source
            .arena
            .node_array(list.declarations.expect("declarations"))
            .nodes[0]
    }

    #[test]
    fn dm_serialize_type_of_declaration() {
        with_member_state(
            "export const named: string = \"a\";",
            |checker, arena, target| {
                let declaration = first_variable_declaration(checker, 0);
                let root = checker.binder.source(0).root;
                let mut tracker = NoopTracker;
                let node = checker
                    .emit_create_type_of_declaration(
                        arena,
                        target,
                        declaration,
                        root,
                        EmitNodeBuilderFlags::DECLARATION_EMIT,
                        EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                        &mut tracker,
                    )
                    .expect("serialization succeeds")
                    .expect("annotated declaration serializes");
                assert_eq!(node_kind(arena, target, node), SyntaxKind::StringKeyword);
            },
        );
        // Non-inferred-type kinds take the AnyKeyword token fallback.
        with_member_state("export function f(): void {}", |checker, arena, target| {
            let function = statement_at(checker, 0);
            let root = checker.binder.source(0).root;
            let mut tracker = NoopTracker;
            let node = checker
                .emit_create_type_of_declaration(
                    arena,
                    target,
                    function,
                    root,
                    EmitNodeBuilderFlags::DECLARATION_EMIT,
                    EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                    &mut tracker,
                )
                .expect("fallback succeeds")
                .expect("fallback token");
            assert_eq!(node_kind(arena, target, node), SyntaxKind::AnyKeyword);
        });
    }

    #[test]
    fn dm_serialize_return_type() {
        with_member_state(
            "export function f(): number { return 1; }",
            |checker, arena, target| {
                let function = statement_at(checker, 0);
                let root = checker.binder.source(0).root;
                let mut tracker = NoopTracker;
                let node = checker
                    .emit_create_return_type_of_signature_declaration(
                        arena,
                        target,
                        function,
                        root,
                        EmitNodeBuilderFlags::DECLARATION_EMIT,
                        EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                        &mut tracker,
                    )
                    .expect("serialization succeeds")
                    .expect("annotated return serializes");
                assert_eq!(node_kind(arena, target, node), SyntaxKind::NumberKeyword);
            },
        );
    }

    #[test]
    fn dm_literal_const_value() {
        let source = "export const s = \"hi\";\n\
                      export const n = 3;\n\
                      export const m = -4;\n\
                      export const b = true;\n\
                      export const g = 5n;";
        with_member_state(source, |checker, arena, target| {
            let mut tracker = NoopTracker;
            let expectations = [
                (0usize, SyntaxKind::StringLiteral),
                (1, SyntaxKind::NumericLiteral),
                (2, SyntaxKind::PrefixUnaryExpression),
                (3, SyntaxKind::TrueKeyword),
                (4, SyntaxKind::BigIntLiteral),
            ];
            for (statement, expected) in expectations {
                let declaration = first_variable_declaration(checker, statement);
                let node = checker
                    .emit_create_literal_const_value(arena, target, declaration, &mut tracker)
                    .expect("literal serializes");
                assert_eq!(
                    node_kind(arena, target, node),
                    expected,
                    "statement {statement}"
                );
            }
        });
    }

    #[test]
    fn dm_late_bound_index_signatures() {
        // A source-declared index signature is skipped (info.declaration
        // set) after the present-empty result materializes.
        with_member_state(
            "export class Boxes { [key: string]: number; }",
            |checker, arena, target| {
                let class = statement_at(checker, 0);
                let root = checker.binder.source(0).root;
                let mut tracker = NoopTracker;
                let result = checker
                    .emit_create_late_bound_index_signatures(
                        arena,
                        target,
                        class,
                        root,
                        EmitNodeBuilderFlags::DECLARATION_EMIT,
                        EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                        &mut tracker,
                    )
                    .expect("member succeeds");
                // Upstream returns a PRESENT-EMPTY array here: the instance
                // info list is non-empty (result ||= [] fires) and its one
                // declared info is then skipped (:88632-88633).
                assert_eq!(result, Some(Vec::new()));
            },
        );
    }

    #[test]
    fn dm_declaration_statements_for_source_file() {
        // Pre-lane-F this asserted the typed pending error; the statements
        // cluster now serializes the module's export table.
        with_member_state("export const x = 1;", |checker, arena, target| {
            let root = checker.binder.source(0).root;
            let mut tracker = NoopTracker;
            let statements = checker
                .emit_get_declaration_statements_for_source_file(
                    arena,
                    target,
                    root,
                    EmitNodeBuilderFlags::DECLARATION_EMIT,
                    EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                    &mut tracker,
                )
                .expect("statements serialize")
                .expect("module exports produce statements");
            assert!(!statements.is_empty());
            assert_eq!(
                node_kind(arena, target, statements[0]),
                SyntaxKind::VariableStatement
            );
        });
    }
}
