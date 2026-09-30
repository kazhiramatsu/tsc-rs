//! Checker-owned declaration-emit visibility and accessibility workers.
//!
//! These workers intentionally do not participate in the bounded display
//! slices in `check.rs`. The display path keeps its existing read-only
//! decisions, while this module owns declaration-emit memoization, alias
//! painting, and result-shaped accessibility.

use rustc_hash::FxHashSet as HashSet;

use tsc_binder::{node_util, SymbolId, SymbolTable};
use tsc_emitter::{
    create_printer, EmitFunctionProperty, EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags,
    EmitResolverError, EmitResolverNode, EmitResolverSymbol, EmitSymbolAccessibility,
    EmitSymbolAccessibilityResult, EmitSymbolMeaning, NewLineKind, PrintRequest, PrinterOptions,
    StandaloneWriter,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{CheckFlags, CompilerOptions, ModifierFlags, NodeFlags, SymbolFlags};

use crate::state::{CheckAbort, CheckResult, CheckerState, OracleCrashKind};

/// SymbolFormatFlags word consumed by checker `symbolToString`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SymbolFormatFlags(u32);

impl SymbolFormatFlags {
    const WRITE_TYPE_PARAMETERS_OR_ARGUMENTS: Self = Self(1);
    const USE_ONLY_EXTERNAL_ALIASING: Self = Self(2);
    const ALLOW_ANY_NODE_KIND: Self = Self(4);
    const USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE: Self = Self(8);
    const WRITE_COMPUTED_PROPS: Self = Self(16);
    const DO_NOT_INCLUDE_SYMBOL_CHAIN: Self = Self(32);

    const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

fn declaration_emit_symbol_builder_abort(error: EmitResolverError) -> CheckAbort {
    match error {
        EmitResolverError::CheckerAborted { reason, .. }
            if reason == OracleCrashKind::OuterJsdocTemplateReferenceDisplay.description() =>
        {
            CheckAbort::OracleCrash(OracleCrashKind::OuterJsdocTemplateReferenceDisplay)
        }
        error => panic!("symbolToString NodeBuilder failed unexpectedly: {error}"),
    }
}

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
        // getParseTreeNode returns an existing parse node before applying
        // its kind predicate (_tsc.js:11426-11436). The isolated tracker
        // therefore also queries VariableDeclaration for arrow expandos.
        if !self.emit_is_parse_tree_node(node) {
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
            symbol_index: container_symbol.index(),
        };
        let mut result = Vec::with_capacity(properties.len());
        for property in properties {
            let property_data = self.binder.symbol(property);
            let value_declaration = property_data
                .value_declaration
                .filter(|&declaration| self.emit_is_parse_tree_node(declaration))
                .map(|declaration| self.declaration_emit_resolver_node(declaration));
            result.push(EmitFunctionProperty {
                name: property_data.escaped_name,
                symbol: EmitResolverSymbol {
                    session_token,
                    symbol_index: property.index(),
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
        if let Some(visible) = *self.links.node_cold().is_visible.get(declaration) {
            return Ok(visible);
        }
        let visible = self.emit_determine_declaration_is_visible(declaration)?;
        self.links
            .set_node_is_visible(self.speculation_depth, declaration, visible);
        Ok(self
            .links
            .node_cold()
            .is_visible
            .get(declaration)
            .unwrap_or(visible))
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
        if !aliases_to_make_visible.contains(&aliasing_statement) {
            aliases_to_make_visible.push(aliasing_statement);
        }
    }

    /// tsc-port: symbolToString @6.0.3
    /// tsc-hash: db59b39300442558c3a8f0e1f1d1681dbfaf0fdb3951350b225677ed4851157e
    /// tsc-span: _tsc.js:50649-50681
    fn symbol_to_string_via_node_builder(
        &mut self,
        symbol: SymbolId,
        enclosing: Option<NodeId>,
        meaning: EmitSymbolMeaning,
        format_flags: SymbolFormatFlags,
    ) -> CheckResult<tsc_types::JsString> {
        let mut flags = 70_221_824_u32;
        let mut internal_flags = 0_u32;
        if format_flags.contains(SymbolFormatFlags::USE_ONLY_EXTERNAL_ALIASING) {
            flags |= 128;
        }
        if format_flags.contains(SymbolFormatFlags::WRITE_TYPE_PARAMETERS_OR_ARGUMENTS) {
            flags |= 512;
        }
        if format_flags.contains(SymbolFormatFlags::USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE) {
            flags |= 16_384;
        }
        if format_flags.contains(SymbolFormatFlags::DO_NOT_INCLUDE_SYMBOL_CHAIN) {
            internal_flags |= 4;
        }
        if format_flags.contains(SymbolFormatFlags::WRITE_COMPUTED_PROPS) {
            internal_flags |= 1;
        }

        let node = self.emit_build_symbol_display_node(
            symbol,
            enclosing,
            meaning,
            EmitNodeBuilderFlags(flags),
            EmitInternalNodeBuilderFlags(internal_flags),
            format_flags.contains(SymbolFormatFlags::ALLOW_ANY_NODE_KIND),
        );
        let node = node.map_err(declaration_emit_symbol_builder_abort)?;

        let mut options = PrinterOptions::new(NewLineKind::LineFeed)
            .with_remove_comments(true)
            .with_declaration_syntax(true);
        if enclosing.is_some_and(|node| self.kind_of(node) == SyntaxKind::SourceFile) {
            options = options.with_never_ascii_escape(true);
        }
        let printed = self
            .with_emit_display(|display| {
                create_printer(options).print(
                    display,
                    PrintRequest::StandaloneNode {
                        node,
                        writer: StandaloneWriter::SingleLine,
                    },
                    None,
                )
            })
            .expect("symbolToString standalone printing must succeed");
        Ok(tsc_types::JsString::from_code_units(
            printed.text_utf16().as_ref(),
        ))
    }

    /// The default `symbolToString(propertySymbol)` face used by declaration
    /// diagnostics: AllowAnyNodeKind, no requested meaning, and no computed-
    /// property rewrite flag.
    /// tsrs-native: typed adapter for declaration diagnostics.
    pub(crate) fn emit_symbol_to_string_default(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult<tsc_types::JsString> {
        self.symbol_to_string_via_node_builder(
            symbol,
            None,
            EmitSymbolMeaning(0),
            SymbolFormatFlags::ALLOW_ANY_NODE_KIND,
        )
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
                    let name = self.binder.symbol(symbol).escaped_name;
                    self.resolve_name(
                        Some(enclosing_declaration),
                        name,
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
            let error_symbol_name = self.symbol_to_string_via_node_builder(
                initial_symbol,
                Some(enclosing_declaration),
                EmitSymbolMeaning(meaning.bits() as u32),
                SymbolFormatFlags::ALLOW_ANY_NODE_KIND,
            )?;
            let error_module_name = if had_accessible_chain != initial_symbol {
                Some(self.symbol_to_string_via_node_builder(
                    had_accessible_chain,
                    Some(enclosing_declaration),
                    EmitSymbolMeaning::NAMESPACE,
                    SymbolFormatFlags::ALLOW_ANY_NODE_KIND,
                )?)
            } else {
                None
            };
            return Ok(Some(self.accessibility_result(
                EmitSymbolAccessibility::NotAccessible,
                None,
                Some(error_symbol_name),
                error_module_name,
                None,
            )));
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
    /// Both result names come from the NodeBuilder + standalone-printer
    /// symbolToString path.
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
                let error_symbol_name = self.symbol_to_string_via_node_builder(
                    symbol,
                    Some(enclosing_declaration),
                    EmitSymbolMeaning(meaning.bits() as u32),
                    SymbolFormatFlags::ALLOW_ANY_NODE_KIND,
                )?;
                let error_module_name = self.symbol_to_string_via_node_builder(
                    symbol_external_module,
                    None,
                    EmitSymbolMeaning::NAMESPACE,
                    SymbolFormatFlags::ALLOW_ANY_NODE_KIND,
                )?;
                return Ok(self.accessibility_result(
                    EmitSymbolAccessibility::CannotBeNamed,
                    None,
                    Some(error_symbol_name),
                    Some(error_module_name),
                    self.is_in_js_file(enclosing_declaration)
                        .then(|| self.declaration_emit_resolver_node(enclosing_declaration)),
                ));
            }
        }

        let error_symbol_name = self.symbol_to_string_via_node_builder(
            symbol,
            Some(enclosing_declaration),
            EmitSymbolMeaning(meaning.bits() as u32),
            SymbolFormatFlags::ALLOW_ANY_NODE_KIND,
        )?;
        Ok(self.accessibility_result(
            EmitSymbolAccessibility::NotAccessible,
            None,
            Some(error_symbol_name),
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
        self.emit_is_entity_name_visible_worker(
            entity_name,
            enclosing_declaration,
            should_compute_aliases_to_make_visible,
        )
    }

    fn emit_is_entity_name_visible_worker(
        &mut self,
        entity_name: NodeId,
        enclosing_declaration: NodeId,
        should_compute_aliases_to_make_visible: bool,
    ) -> CheckResult<EmitSymbolAccessibilityResult> {
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
                Some(error_name.into()),
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
            Some(error_name.into()),
            None,
            Some(self.declaration_emit_resolver_node(first_identifier)),
        ))
    }

    /// tsc-port: hasGlobalName @6.0.3
    /// tsc-hash: 53c9de85b0c10c5de2b868bc13acdbf715186648a451e034c1d6395b5096c7d9
    /// tsc-span: _tsc.js:88396-88398
    pub(crate) fn emit_has_global_name(&self, name: &str) -> bool {
        self.globals
            .contains_key(tsc_binder::escape_leading_underscores(name))
    }

    /// tsc-port: collectLinkedAliases @6.0.3
    /// tsc-hash: 8fe011e257a2763196e5bd485d330cf0df070bbdf96d1d78fd9edf54c0f391c5
    /// tsc-span: _tsc.js:55675-55727
    ///
    /// `set_visibility=true` is used by the two check-phase callers; the
    /// emit resolver's collectLinkedAliases also takes the ordered
    /// collection branch.
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
        let mut visited = HashSet::default();
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

    pub(crate) fn accessible_result(&self) -> EmitSymbolAccessibilityResult {
        self.accessibility_result(EmitSymbolAccessibility::Accessible, None, None, None, None)
    }

    fn accessibility_result(
        &self,
        accessibility: EmitSymbolAccessibility,
        aliases_to_make_visible: Option<Vec<EmitResolverNode>>,
        error_symbol_name: Option<tsc_types::JsString>,
        error_module_name: Option<tsc_types::JsString>,
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
        synthetic_module_scope: Option<crate::node_builder::SyntheticModuleScope<'_>>,
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
            synthetic_module_scope,
        )
    }

    /// tsc-port: createTypeOfDeclarationInExpandoScope @6.0.3
    /// tsc-hash: 37a21cd710c255c1fe8fc4e0e704b11c8062854069c854051651e47a8e392a90
    /// tsc-span: _tsc.js:115400-115425
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_create_type_of_declaration_in_expando_scope(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        declaration: NodeId,
        function: NodeId,
        enclosing_declaration: NodeId,
        flags: tsc_emitter::EmitNodeBuilderFlags,
        internal_flags: tsc_emitter::EmitInternalNodeBuilderFlags,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<Option<tsc_emitter::TransformNode>, tsc_emitter::EmitResolverError> {
        let method = tsc_emitter::EmitResolverMethod::CreateTypeOfDeclarationInExpandoScope;
        let properties = self
            .emit_get_properties_of_container_function(function, 0)
            .map_err(|abort| node_builder_abort_error(self, method, function, abort))?;
        let mut locals = SymbolTable::default();
        for property in properties {
            locals.insert(property.name, SymbolId::new(property.symbol.symbol_index));
        }
        self.emit_create_type_of_declaration(
            arena,
            target,
            declaration,
            enclosing_declaration,
            flags,
            internal_flags,
            Some(crate::node_builder::SyntheticModuleScope {
                enclosing_declaration: Some(enclosing_declaration),
                locals: &locals,
            }),
            tracker,
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
        // `type.flags & EnumLike` (88492): EnumLike = Enum | EnumLiteral, so
        // EITHER bit selects the symbol-expression arm — `contains` demanded
        // both and left the arm dead (h2-7b-m-2 fence amendment #4d: a
        // computed enum member's fresh Enum type aborted as "not a literal",
        // and constant members printed their values instead of `E.A`).
        if flags.intersects(TypeFlags::ENUM_LIKE) {
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
                .create_big_int_literal(target, format!("{}n", pseudo.to_base10_string()))
                .map_err(map_factory),
            LiteralValue::String(text) => factory
                .create_string_literal_from_code_units(target, text.units(), false)
                .map_err(map_factory),
            LiteralValue::Number(number) if number < 0.0 => {
                let operand = factory
                    .create_numeric_literal(target, tsc_types::js_number_to_string(-number))
                    .map_err(map_factory)?;
                factory
                    .create_prefix_unary_expression(target, SyntaxKind::MinusToken, operand)
                    .map_err(map_factory)
            }
            LiteralValue::Number(number) => factory
                .create_numeric_literal(target, tsc_types::js_number_to_string(number))
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
                self.resolve_external_module_symbol(Some(symbol), false)
                    .map_err(|abort| node_builder_abort_error(self, method, node, abort))?;
                (**self.binder.symbol(symbol).exports()).clone()
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

#[cfg(test)]
#[path = "../tests/unit/declaration_emit_members/tests.rs"]
mod node_builder_member_tests;
