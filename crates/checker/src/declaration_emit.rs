//! Checker-owned declaration-emit visibility and accessibility workers.
//!
//! These workers intentionally do not participate in the bounded display
//! slices in `check.rs`. The display path keeps its existing read-only
//! decisions, while this module owns declaration-emit memoization, alias
//! painting, and result-shaped accessibility.

use std::collections::HashSet;

use tsc_binder::{node_util, SymbolId};
use tsc_emitter::{
    EmitResolverNode, EmitSymbolAccessibility, EmitSymbolAccessibilityResult, EmitSymbolMeaning,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{CompilerOptions, ModifierFlags, SymbolFlags};

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

/// tsc-port: getEmitDeclarations computeValue @6.0.3
/// tsc-hash: b592b5f4c632a60784ba25e59679201356befc86d16c5a441be6439b53a5150c
/// tsc-span: _tsc.js:18151-18155
pub(crate) fn emit_declarations(options: &CompilerOptions) -> bool {
    options.declaration == Some(true) || options.composite == Some(true)
}

impl CheckerState<'_> {
    /// tsc-port: isDeclarationVisible @6.0.3
    /// tsc-hash: b569e8243cf2db9de0dbec7462f29fa1e70f4b94405adb5a134b6571d4c8fbeb
    /// tsc-span: _tsc.js:55589-55674
    ///
    /// The kind walk is shared with the existing display slice. Only this
    /// emit worker reads and writes NodeLinks.isVisible, preserving the
    /// display-path invariant while giving declaration emit its upstream
    /// memo-and-paint behavior.
    pub(crate) fn emit_is_declaration_visible(&mut self, declaration: NodeId) -> CheckResult<bool> {
        if let Some(visible) = self.links.node(declaration).is_visible {
            return Ok(visible);
        }
        let visible = self.reused_declaration_is_visible_slice(declaration);
        self.links
            .set_node_is_visible(self.speculation_depth, declaration, visible);
        Ok(self.links.node(declaration).is_visible.unwrap_or(visible))
    }

    /// tsc-port: hasVisibleDeclarations (+ addVisibleAlias) @6.0.3
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
        Ok(Some(self.accessibility_result(
            EmitSymbolAccessibility::Accessible,
            (!aliases_to_make_visible.is_empty()).then(|| {
                aliases_to_make_visible
                    .into_iter()
                    .map(|node| self.declaration_emit_resolver_node(node))
                    .collect()
            }),
            None,
            None,
            None,
        )))
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
            let accessible_symbol_chain = self.declaration_emit_accessible_symbol_chain(
                symbol,
                meaning,
                Some(enclosing_declaration),
            )?;
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
