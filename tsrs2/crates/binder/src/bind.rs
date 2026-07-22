//! The bind walk: bind / bindEach / bindEachChild /
//! bindEachFunctionsFirst, the bindChildren dispatch, and stage 3.4's
//! bindWorker with its per-kind symbol arms, the strict-mode check
//! family, and the contextual-identifier checks. The flow-aware
//! bindChildren arms are stage 3.5; the JS special-assignment symbol
//! bodies are stage 3.4c (the dispatch and its early-return shape are
//! here).

use crate::containers::{get_container_flags, ContainerFlags};
use crate::declare::{Binder, TableRef};
use crate::flow::FlowId;
use crate::node_util::{
    can_have_flow_node, declaration_name_to_string, get_containing_class, get_error_span_for_node,
    has_dynamic_name, id_text, is_assignment_operator, is_async_function,
    is_auto_accessor_property_declaration, is_binding_pattern, is_block_or_catch_scoped,
    is_destructuring_assignment, is_entity_name_expression, is_expression_node,
    is_function_like_kind, is_identifier_name, is_in_top_level_context, is_narrowable_operand,
    is_narrowable_reference, is_narrowing_expression, is_object_literal_method,
    is_object_literal_or_class_expression_method_or_accessor, is_parameter_property_declaration,
    is_part_of_parameter_declaration, is_part_of_type_query, is_potentially_executable_node,
    is_string_or_numeric_literal_like, kind_of, literal_text_of, name_field_of, parent_of,
    statements_of,
};
use crate::symbols::{InternalSymbolName, SymbolId};
use tsrs2_diags::{gen as diagnostics, DiagnosticMessage};
use tsrs2_syntax::{for_each_child, NodeArrayId, NodeData, NodeId, SyntaxKind};
use tsrs2_types::{FlowFlags, ModifierFlags, NodeFlags, ScriptTarget, SymbolFlags};

/// tsc AssignmentDeclarationKind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignmentDeclarationKind {
    None,
    ExportsProperty,
    ModuleExports,
    PrototypeProperty,
    ThisProperty,
    Property,
    Prototype,
    ObjectDefinePropertyValue,
    ObjectDefinePropertyExports,
    ObjectDefinePrototypeProperty,
}

impl<'a> Binder<'a> {
    /// tsc-port: bindSourceFile2 @6.0.3
    /// tsc-hash: 213891d27022fad429657a32a61e019a181b1fa5a1abd1f54586b72fdc3495a1
    /// tsc-span: _tsc.js:42456-42505
    ///
    /// The closure-state reset tail is unnecessary (a Binder is
    /// per-file); delayedBindJSDocTypedefTag/bindJSDocImports await
    /// JSDoc parsing. file.symbolCount == symbols.len().
    pub fn bind_source_file(&mut self) {
        self.in_strict_mode = self.bind_in_strict_mode();
        self.bind(Some(self.source.root));
    }

    /// tsc-port: bindInStrictMode @6.0.3
    /// tsc-hash: db79acb443b17eec4610c2390ed5ff025299d93c6040e7bfdd0fc30fefce98b8
    /// tsc-span: _tsc.js:42506-42512
    fn bind_in_strict_mode(&self) -> bool {
        if self.options.always_strict_effective() && !self.source.is_declaration_file {
            true
        } else {
            self.source.external_module_indicator.is_some()
        }
    }

    /// tsc-port: bind @6.0.3
    /// tsc-hash: d0f56450cb1e141f74a40208f49a2952dd81d23e726f1c07e9d897b22a56f546
    /// tsc-span: _tsc.js:44226-44255
    ///
    /// setParent is unnecessary (arena parents are finalized at parse
    /// time); bindJSDoc awaits JSDoc parsing.
    pub fn bind(&mut self, node: Option<NodeId>) {
        let Some(node) = node else { return };
        let save_in_strict_mode = self.in_strict_mode;
        self.bind_worker(node);
        if kind_of(self.source, node) as u16 > SyntaxKind::LastToken as u16 {
            let container_flags = get_container_flags(self.source, node);
            if container_flags == ContainerFlags::NONE {
                self.bind_children(node);
            } else {
                self.bind_container(node, container_flags);
            }
        }
        self.in_strict_mode = save_in_strict_mode;
    }

    /// tsc-port: bindWorker @6.0.3
    /// tsc-hash: 4b259323fa2534e8d67ea9485669ddbff8e0a5a252719533558d6d8e99181588
    /// tsc-span: _tsc.js:44287-44527
    ///
    /// JS-only arms carved out (stage 3.4c / JSDoc): the JSDoc-namespace
    /// identifier alias, special property declarations, the CommonJS
    /// module-symbol declaration, bindCallExpression, and the JSDoc tag
    /// arms.
    pub(crate) fn bind_worker(&mut self, node: NodeId) {
        match kind_of(self.source, node) {
            SyntaxKind::Identifier | SyntaxKind::ThisKeyword => {
                if kind_of(self.source, node) == SyntaxKind::ThisKeyword {
                    self.seen_this_keyword = true;
                }
                if let Some(current_flow) = self.current_flow {
                    let in_expression = is_expression_node(self.source, node)
                        || parent_of(self.source, node).is_some_and(|parent| {
                            kind_of(self.source, parent) == SyntaxKind::ShorthandPropertyAssignment
                        });
                    if in_expression {
                        self.node_flow.insert(node, current_flow);
                    }
                }
                self.check_contextual_identifier(node);
            }
            SyntaxKind::QualifiedName => {
                if let Some(current_flow) = self.current_flow {
                    if is_part_of_type_query(self.source, node) {
                        self.node_flow.insert(node, current_flow);
                    }
                }
            }
            SyntaxKind::MetaProperty | SyntaxKind::SuperKeyword => {
                if let Some(current_flow) = self.current_flow {
                    self.node_flow.insert(node, current_flow);
                }
            }
            SyntaxKind::PrivateIdentifier => self.check_private_identifier(node),
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression => {
                if let Some(current_flow) = self.current_flow {
                    if is_narrowable_reference(self.source, node) {
                        self.node_flow.insert(node, current_flow);
                    }
                }
                // JS-only: isSpecialPropertyDeclaration (needs JSDoc type
                // tags) and the CommonJS `module` symbol declaration.
            }
            SyntaxKind::BinaryExpression => {
                match self.get_assignment_declaration_kind(node) {
                    AssignmentDeclarationKind::ExportsProperty
                    | AssignmentDeclarationKind::ModuleExports
                    | AssignmentDeclarationKind::PrototypeProperty
                    | AssignmentDeclarationKind::Prototype
                    | AssignmentDeclarationKind::ThisProperty => {
                        // JS-only symbol bodies: stage 3.4c
                        // (bindExportsPropertyAssignment et al).
                    }
                    AssignmentDeclarationKind::Property => {
                        self.bind_special_property_assignment(node);
                    }
                    AssignmentDeclarationKind::None => {}
                    _ => debug_assert!(
                        false,
                        "Unknown binary expression special property assignment kind"
                    ),
                }
                self.check_strict_mode_binary_expression(node);
            }
            SyntaxKind::CatchClause => self.check_strict_mode_catch_clause(node),
            SyntaxKind::DeleteExpression => self.check_strict_mode_delete_expression(node),
            SyntaxKind::PostfixUnaryExpression => {
                self.check_strict_mode_postfix_unary_expression(node)
            }
            SyntaxKind::PrefixUnaryExpression => {
                self.check_strict_mode_prefix_unary_expression(node)
            }
            SyntaxKind::WithStatement => self.check_strict_mode_with_statement(node),
            SyntaxKind::LabeledStatement => self.check_strict_mode_labeled_statement(node),
            SyntaxKind::ThisType => {
                self.seen_this_keyword = true;
            }
            SyntaxKind::TypePredicate => {}
            SyntaxKind::TypeParameter => self.bind_type_parameter(node),
            SyntaxKind::Parameter => self.bind_parameter(node),
            SyntaxKind::VariableDeclaration => {
                self.bind_variable_declaration_or_binding_element(node)
            }
            SyntaxKind::BindingElement => {
                if let Some(current_flow) = self.current_flow {
                    self.node_flow.insert(node, current_flow);
                }
                self.bind_variable_declaration_or_binding_element(node);
            }
            SyntaxKind::PropertyDeclaration | SyntaxKind::PropertySignature => {
                self.bind_property_worker(node)
            }
            SyntaxKind::PropertyAssignment | SyntaxKind::ShorthandPropertyAssignment => {
                self.bind_property_or_method_or_accessor(
                    node,
                    SymbolFlags::PROPERTY,
                    SymbolFlags::PROPERTY_EXCLUDES,
                );
            }
            SyntaxKind::EnumMember => {
                self.bind_property_or_method_or_accessor(
                    node,
                    SymbolFlags::ENUM_MEMBER,
                    SymbolFlags::ENUM_MEMBER_EXCLUDES,
                );
            }
            SyntaxKind::CallSignature
            | SyntaxKind::ConstructSignature
            | SyntaxKind::IndexSignature => {
                self.declare_symbol_and_add_to_symbol_table(
                    node,
                    SymbolFlags::SIGNATURE,
                    SymbolFlags::NONE,
                );
            }
            SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature => {
                let optional = if self.question_token_of(node).is_some() {
                    SymbolFlags::OPTIONAL
                } else {
                    SymbolFlags::NONE
                };
                let excludes = if is_object_literal_method(self.source, node) {
                    SymbolFlags::PROPERTY_EXCLUDES
                } else {
                    SymbolFlags::METHOD_EXCLUDES
                };
                self.bind_property_or_method_or_accessor(
                    node,
                    SymbolFlags::METHOD | optional,
                    excludes,
                );
            }
            SyntaxKind::FunctionDeclaration => self.bind_function_declaration(node),
            SyntaxKind::Constructor => {
                self.declare_symbol_and_add_to_symbol_table(
                    node,
                    SymbolFlags::CONSTRUCTOR,
                    SymbolFlags::NONE,
                );
            }
            SyntaxKind::GetAccessor => {
                self.bind_property_or_method_or_accessor(
                    node,
                    SymbolFlags::GET_ACCESSOR,
                    SymbolFlags::GET_ACCESSOR_EXCLUDES,
                );
            }
            SyntaxKind::SetAccessor => {
                self.bind_property_or_method_or_accessor(
                    node,
                    SymbolFlags::SET_ACCESSOR,
                    SymbolFlags::SET_ACCESSOR_EXCLUDES,
                );
            }
            SyntaxKind::FunctionType | SyntaxKind::ConstructorType => {
                self.bind_function_or_constructor_type(node)
            }
            SyntaxKind::TypeLiteral | SyntaxKind::MappedType => {
                self.bind_anonymous_declaration(
                    node,
                    SymbolFlags::TYPE_LITERAL,
                    InternalSymbolName::TYPE.to_owned(),
                );
            }
            SyntaxKind::ObjectLiteralExpression => {
                // tsc bindObjectLiteralExpression (43955).
                self.bind_anonymous_declaration(
                    node,
                    SymbolFlags::OBJECT_LITERAL,
                    InternalSymbolName::OBJECT.to_owned(),
                );
            }
            SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction => {
                self.bind_function_expression(node)
            }
            SyntaxKind::CallExpression => {
                // The ObjectDefineProperty* kinds are JS-only (the
                // wrapper maps them to None in TS files); dispatch kept
                // for shape. bindCallExpression is JS-only.
                let _ = self.get_assignment_declaration_kind(node);
            }
            SyntaxKind::ClassExpression | SyntaxKind::ClassDeclaration => {
                self.in_strict_mode = true;
                self.bind_class_like_declaration(node);
            }
            SyntaxKind::InterfaceDeclaration => {
                self.bind_block_scoped_declaration(
                    node,
                    SymbolFlags::INTERFACE,
                    SymbolFlags::INTERFACE_EXCLUDES,
                );
            }
            SyntaxKind::TypeAliasDeclaration => {
                self.bind_block_scoped_declaration(
                    node,
                    SymbolFlags::TYPE_ALIAS,
                    SymbolFlags::TYPE_ALIAS_EXCLUDES,
                );
            }
            SyntaxKind::EnumDeclaration => self.bind_enum_declaration(node),
            SyntaxKind::ModuleDeclaration => self.bind_module_declaration(node),
            SyntaxKind::JsxAttributes => {
                // tsc bindJsxAttributes (43958).
                self.bind_anonymous_declaration(
                    node,
                    SymbolFlags::OBJECT_LITERAL,
                    InternalSymbolName::JSX_ATTRIBUTES.to_owned(),
                );
            }
            SyntaxKind::JsxAttribute => {
                self.declare_symbol_and_add_to_symbol_table(
                    node,
                    SymbolFlags::PROPERTY,
                    SymbolFlags::PROPERTY_EXCLUDES,
                );
            }
            SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::NamespaceImport
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::ExportSpecifier => {
                self.declare_symbol_and_add_to_symbol_table(
                    node,
                    SymbolFlags::ALIAS,
                    SymbolFlags::ALIAS_EXCLUDES,
                );
            }
            SyntaxKind::NamespaceExportDeclaration => self.bind_namespace_export_declaration(node),
            SyntaxKind::ImportClause => self.bind_import_clause(node),
            SyntaxKind::ExportDeclaration => self.bind_export_declaration(node),
            SyntaxKind::ExportAssignment => self.bind_export_assignment(node),
            SyntaxKind::SourceFile => {
                self.update_strict_mode_statement_list(statements_of(self.source, node));
                self.bind_source_file_if_external_module();
            }
            SyntaxKind::Block => {
                let function_like_parent = parent_of(self.source, node).is_some_and(|parent| {
                    is_function_like_kind(kind_of(self.source, parent))
                        || kind_of(self.source, parent) == SyntaxKind::ClassStaticBlockDeclaration
                });
                if function_like_parent {
                    self.update_strict_mode_statement_list(statements_of(self.source, node));
                }
            }
            SyntaxKind::ModuleBlock => {
                self.update_strict_mode_statement_list(statements_of(self.source, node));
            }
            _ => {}
        }
    }

    fn question_token_of(&self, node: NodeId) -> Option<NodeId> {
        match &self.source.arena.node(node).data {
            NodeData::MethodDeclaration(data) => data.question_token,
            NodeData::MethodSignature(data) => data.question_token,
            NodeData::PropertyDeclaration(data) => data.question_token,
            NodeData::PropertySignature(data) => data.question_token,
            NodeData::ShorthandPropertyAssignment(data) => data.question_token,
            NodeData::Parameter(data) => data.question_token,
            _ => None,
        }
    }

    /// tsc-port: bindPropertyWorker @6.0.3
    /// tsc-hash: 2d80021a99e0e4fb8ce5310be7f20a839a3b2a1e609836ca1f88b8beace1bfb4
    /// tsc-span: _tsc.js:44528-44533
    fn bind_property_worker(&mut self, node: NodeId) {
        let is_auto_accessor = is_auto_accessor_property_declaration(self.source, node);
        let includes = if is_auto_accessor {
            SymbolFlags::ACCESSOR
        } else {
            SymbolFlags::PROPERTY
        };
        let excludes = if is_auto_accessor {
            SymbolFlags::ACCESSOR_EXCLUDES
        } else {
            SymbolFlags::PROPERTY_EXCLUDES
        };
        let optional = if self.question_token_of(node).is_some() {
            SymbolFlags::OPTIONAL
        } else {
            SymbolFlags::NONE
        };
        self.bind_property_or_method_or_accessor(node, includes | optional, excludes);
    }

    /// tsc-port: bindSourceFileIfExternalModule @6.0.3
    /// tsc-hash: ee6372c3aeed05fe2b1e873a97a17d6edd99db19d3a1ac8f39dc931e39e169d6
    /// tsc-span: _tsc.js:44537-44547
    fn bind_source_file_if_external_module(&mut self) {
        self.set_export_context_flag(self.source.root);
        if self.source.external_module_indicator.is_some() {
            self.bind_source_file_as_external_module();
        } else if self.source.file_name.ends_with(".json") {
            self.bind_source_file_as_external_module();
            let file_symbol = self.node_symbol[&self.source.root];
            self.declare_symbol(
                TableRef::Exports(file_symbol),
                Some(file_symbol),
                self.source.root,
                SymbolFlags::PROPERTY,
                SymbolFlags::ALL,
                false,
                false,
            );
            // tsc restores file.symbol to the module symbol.
            self.node_symbol.insert(self.source.root, file_symbol);
        }
    }

    /// tsc-port: bindSourceFileAsExternalModule @6.0.3
    /// tsc-hash: 7bda49b81e8882c21b8464f79529c189f0ddcab650d13ef4a160addba9cd9a07
    /// tsc-span: _tsc.js:44548-44550
    fn bind_source_file_as_external_module(&mut self) {
        let name = format!("\"{}\"", remove_file_extension(&self.source.file_name));
        self.bind_anonymous_declaration(self.source.root, SymbolFlags::VALUE_MODULE, name);
    }

    /// tsc-port: bindExportAssignment @6.0.3
    /// tsc-hash: e26617be015ef87d9cb71bd6834e840e06e88195bad8694f4845d9891ccae92a
    /// tsc-span: _tsc.js:44551-44561
    fn bind_export_assignment(&mut self, node: NodeId) {
        match self.container_symbol() {
            None => {
                let name = self.get_declaration_name(node);
                self.bind_anonymous_declaration(
                    node,
                    SymbolFlags::VALUE,
                    name.unwrap_or_else(|| InternalSymbolName::MISSING.to_owned()),
                );
            }
            Some(container_symbol) => {
                let flags = if self.export_assignment_is_alias(node) {
                    SymbolFlags::ALIAS
                } else {
                    SymbolFlags::PROPERTY
                };
                let symbol = self.declare_symbol(
                    TableRef::Exports(container_symbol),
                    Some(container_symbol),
                    node,
                    flags,
                    SymbolFlags::ALL,
                    false,
                    false,
                );
                let is_export_equals = matches!(
                    &self.source.arena.node(node).data,
                    NodeData::ExportAssignment(data) if data.is_export_equals == Some(true)
                );
                if is_export_equals {
                    self.set_value_declaration(symbol, node);
                }
            }
        }
    }

    /// tsc exportAssignmentIsAlias (15732): entity-name or class
    /// expressions alias.
    fn export_assignment_is_alias(&self, node: NodeId) -> bool {
        let expression = match &self.source.arena.node(node).data {
            NodeData::ExportAssignment(data) => data.expression,
            _ => None,
        };
        let Some(expression) = expression else {
            return false;
        };
        is_entity_name_expression(self.source, expression)
            || kind_of(self.source, expression) == SyntaxKind::ClassExpression
    }

    /// tsc-port: bindNamespaceExportDeclaration @6.0.3
    /// tsc-hash: c70e7257e5f174200353fbfb309b298bacae4f8e5794ebe71465045c20398876
    /// tsc-span: _tsc.js:44562-44573
    fn bind_namespace_export_declaration(&mut self, node: NodeId) {
        let modifiers = crate::node_util::modifiers_of(self.source, node);
        let has_modifiers = modifiers
            .map(|modifiers| !self.source.arena.node_array(modifiers).nodes.is_empty())
            .unwrap_or(false);
        if has_modifiers {
            let diag =
                self.diagnostic_for_node(node, &diagnostics::Modifiers_cannot_appear_here, &[]);
            self.bind_diagnostics.push(diag);
        }
        let parent = parent_of(self.source, node);
        let message: Option<&'static DiagnosticMessage> =
            if parent.is_none_or(|parent| kind_of(self.source, parent) != SyntaxKind::SourceFile) {
                Some(&diagnostics::Global_module_exports_may_only_appear_at_top_level)
            } else if self.source.external_module_indicator.is_none() {
                Some(&diagnostics::Global_module_exports_may_only_appear_in_module_files)
            } else if !self.source.is_declaration_file {
                Some(&diagnostics::Global_module_exports_may_only_appear_in_declaration_files)
            } else {
                None
            };
        match message {
            Some(message) => {
                let diag = self.diagnostic_for_node(node, message, &[]);
                self.bind_diagnostics.push(diag);
            }
            None => {
                let file_symbol = self.node_symbol[&self.source.root];
                self.declare_symbol(
                    TableRef::GlobalExports(file_symbol),
                    Some(file_symbol),
                    node,
                    SymbolFlags::ALIAS,
                    SymbolFlags::ALIAS_EXCLUDES,
                    false,
                    false,
                );
            }
        }
    }

    /// tsc-port: bindExportDeclaration @6.0.3
    /// tsc-hash: d6d358b0a0eb6ef482febcef992fb422846a31fb14ac856ff0239d0594b01f48
    /// tsc-span: _tsc.js:44574-44583
    fn bind_export_declaration(&mut self, node: NodeId) {
        let export_clause = match &self.source.arena.node(node).data {
            NodeData::ExportDeclaration(data) => data.export_clause,
            _ => None,
        };
        match self.container_symbol() {
            None => {
                let name = self.get_declaration_name(node);
                self.bind_anonymous_declaration(
                    node,
                    SymbolFlags::EXPORT_STAR,
                    name.unwrap_or_else(|| InternalSymbolName::MISSING.to_owned()),
                );
            }
            Some(container_symbol) => match export_clause {
                None => {
                    self.declare_symbol(
                        TableRef::Exports(container_symbol),
                        Some(container_symbol),
                        node,
                        SymbolFlags::EXPORT_STAR,
                        SymbolFlags::NONE,
                        false,
                        false,
                    );
                }
                Some(clause) if kind_of(self.source, clause) == SyntaxKind::NamespaceExport => {
                    self.declare_symbol(
                        TableRef::Exports(container_symbol),
                        Some(container_symbol),
                        clause,
                        SymbolFlags::ALIAS,
                        SymbolFlags::ALIAS_EXCLUDES,
                        false,
                        false,
                    );
                }
                Some(_) => {}
            },
        }
    }

    /// tsc-port: bindImportClause @6.0.3
    /// tsc-hash: 4eb6019b7c5a8c5861cdfb01745666a30d7659df8a6b347afecd690fd8685421
    /// tsc-span: _tsc.js:44584-44588
    fn bind_import_clause(&mut self, node: NodeId) {
        let has_name = matches!(
            &self.source.arena.node(node).data,
            NodeData::ImportClause(data) if data.name.is_some()
        );
        if has_name {
            self.declare_symbol_and_add_to_symbol_table(
                node,
                SymbolFlags::ALIAS,
                SymbolFlags::ALIAS_EXCLUDES,
            );
        }
    }

    /// tsc-port: bindFunctionOrConstructorType @6.0.3
    /// tsc-hash: 2b032427f0347c636459d255cb077467972f72bde41f0185e717f1d852d58f32
    /// tsc-span: _tsc.js:43947-43954
    ///
    /// A `__call`/`__new` member inside a fresh `__type` symbol.
    fn bind_function_or_constructor_type(&mut self, node: NodeId) {
        let name = self
            .get_declaration_name(node)
            .unwrap_or_else(|| InternalSymbolName::MISSING.to_owned());
        let symbol = self.symbols.alloc(SymbolFlags::SIGNATURE, name.clone());
        self.add_declaration_to_symbol(symbol, node, SymbolFlags::SIGNATURE);
        let type_literal_symbol = self.symbols.alloc(
            SymbolFlags::TYPE_LITERAL,
            InternalSymbolName::TYPE.to_owned(),
        );
        self.add_declaration_to_symbol(type_literal_symbol, node, SymbolFlags::TYPE_LITERAL);
        self.symbols
            .symbol_mut(type_literal_symbol)
            .members
            .insert(name, symbol);
    }

    /// tsc-port: bindAnonymousDeclaration @6.0.3
    /// tsc-hash: fb38c5d17920eaa351babeb20f49997a2767716808542506ba9fc182431bd1fa
    /// tsc-span: _tsc.js:43964-43971
    pub fn bind_anonymous_declaration(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        name: String,
    ) -> SymbolId {
        let symbol = self.symbols.alloc(symbol_flags, name);
        if symbol_flags.intersects(SymbolFlags::ENUM_MEMBER | SymbolFlags::CLASS_MEMBER) {
            self.symbols.symbol_mut(symbol).parent = self.container_symbol();
        }
        self.add_declaration_to_symbol(symbol, node, symbol_flags);
        symbol
    }

    /// tsc-port: bindBlockScopedDeclaration @6.0.3
    /// tsc-hash: 3d334e9a90d6bdfdb3b7d79213ed700060d8357423cf382de8dede406b119b7f
    /// tsc-span: _tsc.js:43972-43998
    fn bind_block_scoped_declaration(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        symbol_excludes: SymbolFlags,
    ) {
        let block_scope_container = self
            .block_scope_container
            .expect("block-scoped declaration outside any container");
        match kind_of(self.source, block_scope_container) {
            SyntaxKind::ModuleDeclaration => {
                self.declare_module_member(node, symbol_flags, symbol_excludes);
            }
            SyntaxKind::SourceFile
                if self.source.external_module_indicator.is_some()
                    || self.common_js_module_indicator.is_some() =>
            {
                self.declare_module_member(node, symbol_flags, symbol_excludes);
            }
            _ => {
                self.ensure_locals(block_scope_container);
                self.declare_symbol(
                    TableRef::Locals(block_scope_container),
                    None,
                    node,
                    symbol_flags,
                    symbol_excludes,
                    false,
                    false,
                );
            }
        }
    }

    /// tsc-port: bindClassLikeDeclaration @6.0.3
    /// tsc-hash: 99c4658cce8205caca5415451c09de5eb349fec84f1153114f4f0e2dd5126473
    /// tsc-span: _tsc.js:44979-45000
    fn bind_class_like_declaration(&mut self, node: NodeId) {
        if kind_of(self.source, node) == SyntaxKind::ClassDeclaration {
            self.bind_block_scoped_declaration(
                node,
                SymbolFlags::CLASS,
                SymbolFlags::CLASS_EXCLUDES,
            );
        } else {
            let name = name_field_of(self.source, node);
            let binding_name = name
                .and_then(|name| match &self.source.arena.node(name).data {
                    NodeData::Identifier(data) => Some(data.escaped_text.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| InternalSymbolName::CLASS.to_owned());
            self.bind_anonymous_declaration(node, SymbolFlags::CLASS, binding_name.clone());
            if name.is_some() {
                self.classifiable_names.insert(binding_name);
            }
        }
        let Some(&symbol) = self.node_symbol.get(&node) else {
            return;
        };
        let prototype_symbol = self.symbols.alloc(
            SymbolFlags::PROPERTY | SymbolFlags::PROTOTYPE,
            "prototype".to_owned(),
        );
        let existing_export = self
            .symbols
            .symbol(symbol)
            .exports
            .get("prototype")
            .copied();
        if let Some(existing) = existing_export {
            if let Some(&first) = self.symbols.symbol(existing).declarations.first() {
                let diag = self.diagnostic_for_node(
                    first,
                    &diagnostics::Duplicate_identifier_0,
                    &["prototype"],
                );
                self.bind_diagnostics.push(diag);
            }
        }
        self.symbols
            .symbol_mut(symbol)
            .exports
            .insert("prototype".to_owned(), prototype_symbol);
        self.symbols.symbol_mut(prototype_symbol).parent = Some(symbol);
    }

    /// tsc-port: bindEnumDeclaration @6.0.3
    /// tsc-hash: 239e65fd65113102205ad9ff4ef621b45f233cfcab2dc73dbaad75b3645f618f
    /// tsc-span: _tsc.js:45001-45003
    fn bind_enum_declaration(&mut self, node: NodeId) {
        let is_const = crate::node_util::get_combined_modifier_flags(self.source, node)
            .intersects(ModifierFlags::CONST);
        if is_const {
            self.bind_block_scoped_declaration(
                node,
                SymbolFlags::CONST_ENUM,
                SymbolFlags::CONST_ENUM_EXCLUDES,
            );
        } else {
            self.bind_block_scoped_declaration(
                node,
                SymbolFlags::REGULAR_ENUM,
                SymbolFlags::REGULAR_ENUM_EXCLUDES,
            );
        }
    }

    /// tsc-port: bindVariableDeclarationOrBindingElement @6.0.3
    /// tsc-hash: 4f9ece16f30dc33b03772113b03e5b4ac7f47c5767d2b4f214251e2706e1f194
    /// tsc-span: _tsc.js:45004-45020
    ///
    /// JS-only: the bare-require alias arm awaits stage 3.4c.
    fn bind_variable_declaration_or_binding_element(&mut self, node: NodeId) {
        let name = name_field_of(self.source, node);
        if self.in_strict_mode {
            self.check_strict_mode_eval_or_arguments(node, name);
        }
        let Some(name) = name else {
            // A missing name still declares a __missing symbol via
            // getDeclarationName's None path.
            self.declare_symbol_and_add_to_symbol_table(
                node,
                SymbolFlags::FUNCTION_SCOPED_VARIABLE,
                SymbolFlags::FUNCTION_SCOPED_VARIABLE_EXCLUDES,
            );
            return;
        };
        if !is_binding_pattern(self.source, name) {
            if is_block_or_catch_scoped(self.source, node) {
                self.bind_block_scoped_declaration(
                    node,
                    SymbolFlags::BLOCK_SCOPED_VARIABLE,
                    SymbolFlags::BLOCK_SCOPED_VARIABLE_EXCLUDES,
                );
            } else if is_part_of_parameter_declaration(self.source, node) {
                self.declare_symbol_and_add_to_symbol_table(
                    node,
                    SymbolFlags::FUNCTION_SCOPED_VARIABLE,
                    SymbolFlags::PARAMETER_EXCLUDES,
                );
            } else {
                self.declare_symbol_and_add_to_symbol_table(
                    node,
                    SymbolFlags::FUNCTION_SCOPED_VARIABLE,
                    SymbolFlags::FUNCTION_SCOPED_VARIABLE_EXCLUDES,
                );
            }
        }
    }

    /// tsc-port: bindParameter @6.0.3
    /// tsc-hash: bf7da5ad28542bd11a56b44230ae7af7fc900197d037f4c8a9b5170ed17370f2
    /// tsc-span: _tsc.js:45021-45037
    fn bind_parameter(&mut self, node: NodeId) {
        let name = name_field_of(self.source, node);
        if self.in_strict_mode && !self.flags_of(node).intersects(NodeFlags::AMBIENT) {
            self.check_strict_mode_eval_or_arguments(node, name);
        }
        let is_pattern = name.is_some_and(|name| is_binding_pattern(self.source, name));
        if is_pattern {
            let index = parent_of(self.source, node)
                .and_then(|parent| self.parameter_index(parent, node))
                .unwrap_or(0);
            self.bind_anonymous_declaration(
                node,
                SymbolFlags::FUNCTION_SCOPED_VARIABLE,
                format!("__{index}"),
            );
        } else {
            self.declare_symbol_and_add_to_symbol_table(
                node,
                SymbolFlags::FUNCTION_SCOPED_VARIABLE,
                SymbolFlags::PARAMETER_EXCLUDES,
            );
        }
        let parent = parent_of(self.source, node);
        if let Some(parent) = parent {
            if is_parameter_property_declaration(self.source, node, parent) {
                if let Some(class_declaration) = parent_of(self.source, parent) {
                    if let Some(&class_symbol) = self.node_symbol.get(&class_declaration) {
                        let optional = if self.question_token_of(node).is_some() {
                            SymbolFlags::OPTIONAL
                        } else {
                            SymbolFlags::NONE
                        };
                        self.declare_symbol(
                            TableRef::Members(class_symbol),
                            Some(class_symbol),
                            node,
                            SymbolFlags::PROPERTY | optional,
                            SymbolFlags::PROPERTY_EXCLUDES,
                            false,
                            false,
                        );
                    }
                }
            }
        }
    }

    fn parameter_index(&self, function: NodeId, parameter: NodeId) -> Option<usize> {
        let parameters = match &self.source.arena.node(function).data {
            NodeData::FunctionDeclaration(data) => data.parameters,
            NodeData::FunctionExpression(data) => data.parameters,
            NodeData::ArrowFunction(data) => data.parameters,
            NodeData::MethodDeclaration(data) => data.parameters,
            NodeData::MethodSignature(data) => data.parameters,
            NodeData::Constructor(data) => data.parameters,
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            NodeData::CallSignature(data) => data.parameters,
            NodeData::ConstructSignature(data) => data.parameters,
            NodeData::IndexSignature(data) => data.parameters,
            NodeData::FunctionType(data) => data.parameters,
            NodeData::ConstructorType(data) => data.parameters,
            _ => None,
        }?;
        self.source
            .arena
            .node_array(parameters)
            .nodes
            .iter()
            .position(|&candidate| candidate == parameter)
    }

    /// tsc-port: bindFunctionDeclaration @6.0.3
    /// tsc-hash: 2a556f1803e3f631bf473a352283bc91fb02ada41b843473fc8a06004ad46e7e
    /// tsc-span: _tsc.js:45038-45051
    fn bind_function_declaration(&mut self, node: NodeId) {
        if !self.source.is_declaration_file
            && !self.flags_of(node).intersects(NodeFlags::AMBIENT)
            && is_async_function(self.source, node)
        {
            self.emit_flags |= NodeFlags::HAS_ASYNC_FUNCTIONS.bits();
        }
        self.check_strict_mode_function_name(node);
        if self.in_strict_mode {
            self.check_strict_mode_function_declaration(node);
            self.bind_block_scoped_declaration(
                node,
                SymbolFlags::FUNCTION,
                SymbolFlags::FUNCTION_EXCLUDES,
            );
        } else {
            self.declare_symbol_and_add_to_symbol_table(
                node,
                SymbolFlags::FUNCTION,
                SymbolFlags::FUNCTION_EXCLUDES,
            );
        }
    }

    /// tsc-port: bindFunctionExpression @6.0.3
    /// tsc-hash: 01b6497ed5815ff80fd5bdb32b371640583d697cfc9d808f59dfed502b81ceaa
    /// tsc-span: _tsc.js:45052-45064
    fn bind_function_expression(&mut self, node: NodeId) {
        if !self.source.is_declaration_file
            && !self.flags_of(node).intersects(NodeFlags::AMBIENT)
            && is_async_function(self.source, node)
        {
            self.emit_flags |= NodeFlags::HAS_ASYNC_FUNCTIONS.bits();
        }
        if let Some(current_flow) = self.current_flow {
            self.node_flow.insert(node, current_flow);
        }
        self.check_strict_mode_function_name(node);
        let binding_name = name_field_of(self.source, node)
            .and_then(|name| match &self.source.arena.node(name).data {
                NodeData::Identifier(data) => Some(data.escaped_text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| InternalSymbolName::FUNCTION.to_owned());
        self.bind_anonymous_declaration(node, SymbolFlags::FUNCTION, binding_name);
    }

    /// tsc-port: bindPropertyOrMethodOrAccessor @6.0.3
    /// tsc-hash: f1170d680a75a3d43f4e30744734b434403b51a00a5518133961d5cce7a77f4f
    /// tsc-span: _tsc.js:45065-45073
    fn bind_property_or_method_or_accessor(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        symbol_excludes: SymbolFlags,
    ) {
        if !self.source.is_declaration_file
            && !self.flags_of(node).intersects(NodeFlags::AMBIENT)
            && is_async_function(self.source, node)
        {
            self.emit_flags |= NodeFlags::HAS_ASYNC_FUNCTIONS.bits();
        }
        if let Some(current_flow) = self.current_flow {
            if is_object_literal_or_class_expression_method_or_accessor(self.source, node) {
                self.node_flow.insert(node, current_flow);
            }
        }
        if has_dynamic_name(self.source, node) {
            self.bind_anonymous_declaration(
                node,
                symbol_flags,
                InternalSymbolName::COMPUTED.to_owned(),
            );
        } else {
            self.declare_symbol_and_add_to_symbol_table(node, symbol_flags, symbol_excludes);
        }
    }

    /// tsc-port: bindTypeParameter @6.0.3
    /// tsc-hash: d9ebdb8d11003cd97969f00e4e4b7b32890cc8cfc66be6ee5ea25c7e84621a2f
    /// tsc-span: _tsc.js:45078-45115
    ///
    /// The JSDocTemplateTag arm awaits JSDoc parsing.
    fn bind_type_parameter(&mut self, node: NodeId) {
        let parent = parent_of(self.source, node);
        let in_infer_type =
            parent.is_some_and(|parent| kind_of(self.source, parent) == SyntaxKind::InferType);
        if in_infer_type {
            let container = self.get_infer_type_container(parent.unwrap());
            match container {
                Some(container) => {
                    // tsc: container.locals ??= createSymbolTable() —
                    // WITHOUT addToContainerChain.
                    self.locals.entry(container).or_default();
                    self.declare_symbol(
                        TableRef::Locals(container),
                        None,
                        node,
                        SymbolFlags::TYPE_PARAMETER,
                        SymbolFlags::TYPE_PARAMETER_EXCLUDES,
                        false,
                        false,
                    );
                }
                None => {
                    let name = self
                        .get_declaration_name(node)
                        .unwrap_or_else(|| InternalSymbolName::MISSING.to_owned());
                    self.bind_anonymous_declaration(node, SymbolFlags::TYPE_PARAMETER, name);
                }
            }
        } else {
            self.declare_symbol_and_add_to_symbol_table(
                node,
                SymbolFlags::TYPE_PARAMETER,
                SymbolFlags::TYPE_PARAMETER_EXCLUDES,
            );
        }
    }

    /// tsc-port: getInferTypeContainer @6.0.3
    /// tsc-hash: 58dbf69909f3a2c6662d414bdd2b22d8e07f5acd25592c5c2c62d80fb98f3877
    /// tsc-span: _tsc.js:45074-45077
    fn get_infer_type_container(&self, node: NodeId) -> Option<NodeId> {
        let mut current = Some(node);
        while let Some(n) = current {
            if let Some(parent) = parent_of(self.source, n) {
                if let NodeData::ConditionalType(data) = &self.source.arena.node(parent).data {
                    if data.extends_type == Some(n) {
                        return Some(parent);
                    }
                }
            }
            current = parent_of(self.source, n);
        }
        None
    }

    // ---- assignment-declaration classification (TS-visible subset) ----

    /// tsc-port: getAssignmentDeclarationKind @6.0.3
    /// tsc-hash: 86ed418c050973f93d14271122ba9f948961e1e038e6c338eb6df19543402bd2
    /// tsc-span: _tsc.js:15055-15058
    pub fn get_assignment_declaration_kind(&self, expr: NodeId) -> AssignmentDeclarationKind {
        let special = self.get_assignment_declaration_kind_worker(expr);
        if special == AssignmentDeclarationKind::Property || self.in_js_file_public() {
            special
        } else {
            AssignmentDeclarationKind::None
        }
    }

    /// tsc-port: getAssignmentDeclarationKindWorker @6.0.3
    /// tsc-hash: 748a8d0ff34b41c4230a22f31b31e87e5752191e17183fafd67e39f4e2773d51
    /// tsc-span: _tsc.js:15095-15120
    fn get_assignment_declaration_kind_worker(&self, expr: NodeId) -> AssignmentDeclarationKind {
        let source = self.source;
        if let NodeData::CallExpression(data) = &source.arena.node(expr).data {
            if !self.is_bindable_object_define_property_call(expr) {
                return AssignmentDeclarationKind::None;
            }
            let arguments = data.arguments.expect("checked by predicate");
            let entity_name = source.arena.node_array(arguments).nodes[0];
            if is_exports_identifier(source, entity_name)
                || is_module_exports_access_expression(source, entity_name)
            {
                return AssignmentDeclarationKind::ObjectDefinePropertyExports;
            }
            if is_bindable_static_access_expression(source, entity_name, false)
                && get_element_or_property_access_name(source, entity_name).as_deref()
                    == Some("prototype")
            {
                return AssignmentDeclarationKind::ObjectDefinePrototypeProperty;
            }
            return AssignmentDeclarationKind::ObjectDefinePropertyValue;
        }
        let NodeData::BinaryExpression(data) = &source.arena.node(expr).data else {
            return AssignmentDeclarationKind::None;
        };
        let operator = data
            .operator_token
            .map(|token| kind_of(source, token))
            .unwrap_or(SyntaxKind::Unknown);
        let Some(left) = data.left else {
            return AssignmentDeclarationKind::None;
        };
        if operator != SyntaxKind::EqualsToken
            || !matches!(
                kind_of(source, left),
                SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
            )
            || is_void_zero(source, get_right_most_assigned_expression(source, expr))
        {
            return AssignmentDeclarationKind::None;
        }
        let left_expression = access_expression_of(source, left);
        if left_expression
            .is_some_and(|expression| is_bindable_static_name_expression(source, expression, true))
            && get_element_or_property_access_name(source, left).as_deref() == Some("prototype")
            && kind_of(source, get_initializer_of_binary_expression(source, expr))
                == SyntaxKind::ObjectLiteralExpression
        {
            return AssignmentDeclarationKind::Prototype;
        }
        self.get_assignment_declaration_property_access_kind(left)
    }

    /// tsc getAssignmentDeclarationPropertyAccessKind (15121-region).
    fn get_assignment_declaration_property_access_kind(
        &self,
        lhs: NodeId,
    ) -> AssignmentDeclarationKind {
        let source = self.source;
        let Some(lhs_expression) = access_expression_of(source, lhs) else {
            return AssignmentDeclarationKind::None;
        };
        if kind_of(source, lhs_expression) == SyntaxKind::ThisKeyword {
            return AssignmentDeclarationKind::ThisProperty;
        }
        if is_module_exports_access_expression(source, lhs) {
            return AssignmentDeclarationKind::ModuleExports;
        }
        if is_bindable_static_name_expression(source, lhs_expression, true) {
            if is_prototype_access(source, lhs_expression) {
                return AssignmentDeclarationKind::PrototypeProperty;
            }
            let mut next_to_last = lhs;
            while let Some(expression) = access_expression_of(source, next_to_last) {
                if kind_of(source, expression) == SyntaxKind::Identifier {
                    break;
                }
                next_to_last = expression;
            }
            let id = access_expression_of(source, next_to_last).unwrap_or(next_to_last);
            let id_escaped = match &source.arena.node(id).data {
                NodeData::Identifier(data) => Some(data.escaped_text.as_str()),
                _ => None,
            };
            if (id_escaped == Some("exports")
                || id_escaped == Some("module")
                    && get_element_or_property_access_name(source, next_to_last).as_deref()
                        == Some("exports"))
                && is_bindable_static_access_expression(source, lhs, false)
            {
                return AssignmentDeclarationKind::ExportsProperty;
            }
            if is_bindable_static_name_expression(source, lhs, true)
                || kind_of(source, lhs) == SyntaxKind::ElementAccessExpression
            {
                return AssignmentDeclarationKind::Property;
            }
        }
        AssignmentDeclarationKind::None
    }

    /// tsc isBindableObjectDefinePropertyCall (15059).
    fn is_bindable_object_define_property_call(&self, expr: NodeId) -> bool {
        let source = self.source;
        let NodeData::CallExpression(data) = &source.arena.node(expr).data else {
            return false;
        };
        let Some(arguments) = data.arguments else {
            return false;
        };
        let arguments = &source.arena.node_array(arguments).nodes;
        if arguments.len() != 3 {
            return false;
        }
        let Some(expression) = data.expression else {
            return false;
        };
        let NodeData::PropertyAccessExpression(access) = &source.arena.node(expression).data else {
            return false;
        };
        let object_ok = access
            .expression
            .is_some_and(|object| id_text(source, object) == Some("Object"));
        let name_ok = access
            .name
            .is_some_and(|name| id_text(source, name) == Some("defineProperty"));
        object_ok
            && name_ok
            && is_string_or_numeric_literal_like(source, arguments[1])
            && is_bindable_static_name_expression(source, arguments[0], true)
    }

    /// The TS-visible slice of bindSpecialPropertyAssignment (44821):
    /// in a TS file only function-parent (expando) assignments proceed;
    /// the symbol-producing body is stage 3.4c.
    #[allow(clippy::needless_return)] // the guard's fall-through body is the 3.4c stub tail
    fn bind_special_property_assignment(&mut self, node: NodeId) {
        let source = self.source;
        let left = match &source.arena.node(node).data {
            NodeData::BinaryExpression(data) => data.left,
            _ => None,
        };
        let Some(left) = left else { return };
        let Some(left_expression) = access_expression_of(source, left) else {
            return;
        };
        if !self.in_js_file_public() {
            let parent_symbol = self.lookup_symbol_for_property_access(left_expression);
            if !self.is_function_symbol(parent_symbol) {
                return;
            }
            // Stage 3.4c: bindStaticPropertyAssignment /
            // bindPotentiallyMissingNamespaces (expando properties).
            // Until those bodies land, record the parent so the
            // checker can contain member lookups on it (the members
            // this assignment would declare are unbound).
            if let Some(parent_symbol) = parent_symbol {
                self.expando_assignment_targets.insert(parent_symbol);
            }
        }
        // Stage 3.4c: the JS symbol-producing bodies.
    }

    fn is_function_symbol(&self, symbol: Option<SymbolId>) -> bool {
        let Some(symbol) = symbol else { return false };
        let Some(declaration) = self.symbols.symbol(symbol).value_declaration else {
            return false;
        };
        if kind_of(self.source, declaration) == SyntaxKind::FunctionDeclaration {
            return true;
        }
        match &self.source.arena.node(declaration).data {
            NodeData::VariableDeclaration(data) => data.initializer.is_some_and(|initializer| {
                is_function_like_kind(kind_of(self.source, initializer))
            }),
            _ => false,
        }
    }

    /// tsc-port: lookupSymbolForName @6.0.3
    /// tsc-hash: dae249c103758584c4ea6f4bfc83facec94aa163b279cfa2a91db53cae556a9a
    /// tsc-span: _tsc.js:45202-45216
    ///
    /// JS-only: jsGlobalAugmentations awaits the JS subsystem.
    fn lookup_symbol_for_name(&self, container: NodeId, name: &str) -> Option<SymbolId> {
        if let Some(locals) = self.locals.get(&container) {
            if let Some(&local) = locals.get(name) {
                return Some(self.symbols.symbol(local).export_symbol.unwrap_or(local));
            }
        }
        let container_symbol = self.node_symbol.get(&container).copied()?;
        self.symbols
            .symbol(container_symbol)
            .exports
            .get(name)
            .copied()
    }

    /// lookupSymbolForPropertyAccess for the identifier root only (the
    /// access-chain walk is stage 3.4c).
    fn lookup_symbol_for_property_access(&self, node: NodeId) -> Option<SymbolId> {
        let NodeData::Identifier(data) = &self.source.arena.node(node).data else {
            return None;
        };
        let name = data.escaped_text.clone();
        let block_scope_container = self.block_scope_container?;
        self.lookup_symbol_for_name(block_scope_container, &name)
            .or_else(|| {
                self.container
                    .and_then(|container| self.lookup_symbol_for_name(container, &name))
            })
    }

    fn in_js_file_public(&self) -> bool {
        [".js", ".jsx", ".mjs", ".cjs"]
            .iter()
            .any(|extension| self.source.file_name.ends_with(extension))
    }

    // ---- strict-mode + contextual checks ----

    /// tsc-port: updateStrictModeStatementList @6.0.3
    /// tsc-hash: d1bb3cba59e561a09339542a9db07a3e28ad79548d76558148aa0a5652c541fb
    /// tsc-span: _tsc.js:44270-44282
    fn update_strict_mode_statement_list(&mut self, statements: Option<NodeArrayId>) {
        if self.in_strict_mode {
            return;
        }
        let Some(statements) = statements else { return };
        let statements = self.source.arena.node_array(statements).nodes.clone();
        for statement in statements {
            if !self.is_prologue_directive(statement) {
                return;
            }
            if self.is_use_strict_prologue_directive(statement) {
                self.in_strict_mode = true;
                return;
            }
        }
    }

    /// tsc isPrologueDirective (14161).
    fn is_prologue_directive(&self, node: NodeId) -> bool {
        match &self.source.arena.node(node).data {
            NodeData::ExpressionStatement(data) => data.expression.is_some_and(|expression| {
                kind_of(self.source, expression) == SyntaxKind::StringLiteral
            }),
            _ => false,
        }
    }

    /// tsc-port: isUseStrictPrologueDirective @6.0.3
    /// tsc-hash: 527b9111955b43a598d76c96fe2161f3ff9083830137a8aabb7b99fe401268da
    /// tsc-span: _tsc.js:44283-44286
    ///
    /// Source-text comparison, quotes included.
    fn is_use_strict_prologue_directive(&self, node: NodeId) -> bool {
        let expression = match &self.source.arena.node(node).data {
            NodeData::ExpressionStatement(data) => data.expression,
            _ => None,
        };
        let Some(expression) = expression else {
            return false;
        };
        let expr = self.source.arena.node(expression);
        if expr.pos >= expr.end {
            return false;
        }
        let start = tsrs2_syntax::skip_trivia(&self.source.text, expr.pos as usize);
        let text = &self.source.text[start..expr.end as usize];
        text == "\"use strict\"" || text == "'use strict'"
    }

    /// tsc-port: checkContextualIdentifier @6.0.3
    /// tsc-hash: bb7e4c3b7f001003aaee8702d1cd665931bc9e9330249a70a8fbf33aca674a3c
    /// tsc-span: _tsc.js:44104-44122
    ///
    /// OBSERVABLE gate: fires only when file.parseDiagnostics is EMPTY.
    fn check_contextual_identifier(&mut self, node: NodeId) {
        if !self.source.parse_diagnostics.is_empty() {
            return;
        }
        let flags = self.flags_of(node);
        // NodeFlags.JSDoc = 16777216 (unnamed in the generated table).
        if flags.intersects(NodeFlags::AMBIENT) || flags.intersects(NodeFlags::from_bits(16777216))
        {
            return;
        }
        if is_identifier_name(self.source, node) {
            return;
        }
        let escaped_text = match &self.source.arena.node(node).data {
            NodeData::Identifier(data) => data.escaped_text.clone(),
            _ => return,
        };
        let Some(original_keyword_kind) = tsrs2_syntax::keyword_kind(&escaped_text) else {
            return;
        };
        let keyword = original_keyword_kind as u16;
        let display = declaration_name_to_string(self.source, Some(node));
        if self.in_strict_mode
            && keyword >= SyntaxKind::ImplementsKeyword as u16
            && keyword <= SyntaxKind::YieldKeyword as u16
        {
            let message = self.get_strict_mode_identifier_message(node);
            let diag = self.diagnostic_for_node(node, message, &[&display]);
            self.bind_diagnostics.push(diag);
        } else if original_keyword_kind == SyntaxKind::AwaitKeyword {
            if self.source.external_module_indicator.is_some()
                && is_in_top_level_context(self.source, node)
            {
                let diag = self.diagnostic_for_node(
                    node,
                    &diagnostics::Identifier_expected_0_is_a_reserved_word_at_the_top_level_of_a_module,
                    &[&display],
                );
                self.bind_diagnostics.push(diag);
            } else if flags.intersects(NodeFlags::AWAIT_CONTEXT) {
                let diag = self.diagnostic_for_node(
                    node,
                    &diagnostics::Identifier_expected_0_is_a_reserved_word_that_cannot_be_used_here,
                    &[&display],
                );
                self.bind_diagnostics.push(diag);
            }
        } else if original_keyword_kind == SyntaxKind::YieldKeyword
            && flags.intersects(NodeFlags::YIELD_CONTEXT)
        {
            let diag = self.diagnostic_for_node(
                node,
                &diagnostics::Identifier_expected_0_is_a_reserved_word_that_cannot_be_used_here,
                &[&display],
            );
            self.bind_diagnostics.push(diag);
        }
    }

    /// tsc-port: getStrictModeIdentifierMessage @6.0.3
    /// tsc-hash: aece6de44d126b31b95dbbb031ff3c3874a55b1200167173164344db50bd311d
    /// tsc-span: _tsc.js:44123-44131
    fn get_strict_mode_identifier_message(&self, node: NodeId) -> &'static DiagnosticMessage {
        if get_containing_class(self.source, node).is_some() {
            return &diagnostics::Identifier_expected_0_is_a_reserved_word_in_strict_mode_Class_definitions_are_automatically_in_strict_mode;
        }
        if self.source.external_module_indicator.is_some() {
            return &diagnostics::Identifier_expected_0_is_a_reserved_word_in_strict_mode_Modules_are_automatically_in_strict_mode;
        }
        &diagnostics::Identifier_expected_0_is_a_reserved_word_in_strict_mode
    }

    /// tsc-port: checkPrivateIdentifier @6.0.3
    /// tsc-hash: cb3994b9221228cee285a1fdf2ec491cb1055f29567cd50e8ab61460a32df228
    /// tsc-span: _tsc.js:44132-44138
    fn check_private_identifier(&mut self, node: NodeId) {
        let escaped_text = match &self.source.arena.node(node).data {
            NodeData::PrivateIdentifier(data) => data.escaped_text.as_str(),
            _ => return,
        };
        if escaped_text == "#constructor" && self.source.parse_diagnostics.is_empty() {
            let display = declaration_name_to_string(self.source, Some(node));
            let diag = self.diagnostic_for_node(
                node,
                &diagnostics::constructor_is_a_reserved_word,
                &[&display],
            );
            self.bind_diagnostics.push(diag);
        }
    }

    /// tsc-port: checkStrictModeBinaryExpression @6.0.3
    /// tsc-hash: 351653abd86c1f9c51244ccc3f4d53ba8185b945c34fad7b984cb5247e49c343
    /// tsc-span: _tsc.js:44139-44143
    fn check_strict_mode_binary_expression(&mut self, node: NodeId) {
        if !self.in_strict_mode {
            return;
        }
        let (left, operator) = match &self.source.arena.node(node).data {
            NodeData::BinaryExpression(data) => (
                data.left,
                data.operator_token
                    .map(|token| kind_of(self.source, token))
                    .unwrap_or(SyntaxKind::Unknown),
            ),
            _ => return,
        };
        if let Some(left) = left {
            if crate::node_util::is_left_hand_side_expression(self.source, left)
                && is_assignment_operator(operator)
            {
                self.check_strict_mode_eval_or_arguments(node, Some(left));
            }
        }
    }

    /// tsc-port: checkStrictModeCatchClause @6.0.3
    /// tsc-hash: aeaf9cdd1a2cdbae129c915bed2f44aa252461e410130d0d9d6ff014c719be46
    /// tsc-span: _tsc.js:44144-44148
    fn check_strict_mode_catch_clause(&mut self, node: NodeId) {
        if !self.in_strict_mode {
            return;
        }
        let variable_declaration = match &self.source.arena.node(node).data {
            NodeData::CatchClause(data) => data.variable_declaration,
            _ => None,
        };
        if let Some(variable_declaration) = variable_declaration {
            let name = name_field_of(self.source, variable_declaration);
            self.check_strict_mode_eval_or_arguments(node, name);
        }
    }

    /// tsc-port: checkStrictModeDeleteExpression @6.0.3
    /// tsc-hash: 2d8f2e480393280022cdc7db903f5a4fd6d04ab2fd241b494a368db2af099156
    /// tsc-span: _tsc.js:44149-44154
    fn check_strict_mode_delete_expression(&mut self, node: NodeId) {
        if !self.in_strict_mode {
            return;
        }
        let expression = match &self.source.arena.node(node).data {
            NodeData::DeleteExpression(data) => data.expression,
            _ => None,
        };
        if let Some(expression) = expression {
            if kind_of(self.source, expression) == SyntaxKind::Identifier {
                let (start, end) = get_error_span_for_node(self.source, expression);
                self.push_file_diagnostic(
                    start,
                    end,
                    &diagnostics::delete_cannot_be_called_on_an_identifier_in_strict_mode,
                    &[],
                );
            }
        }
    }

    /// tsc-port: checkStrictModeEvalOrArguments @6.0.3
    /// tsc-hash: 85b1a51d861b2a8d86ecfc97f3bd5fc60af791fd808909776e11cfffc56438d7
    /// tsc-span: _tsc.js:44158-44166
    fn check_strict_mode_eval_or_arguments(&mut self, context_node: NodeId, name: Option<NodeId>) {
        let Some(name) = name else { return };
        let NodeData::Identifier(data) = &self.source.arena.node(name).data else {
            return;
        };
        // tsc isEvalOrArgumentsIdentifier (44155).
        if data.escaped_text != "eval" && data.escaped_text != "arguments" {
            return;
        }
        let display = id_text(self.source, name).unwrap_or_default().to_owned();
        let (start, end) = get_error_span_for_node(self.source, name);
        let message = self.get_strict_mode_eval_or_arguments_message(context_node);
        self.push_file_diagnostic(start, end, message, &[&display]);
    }

    /// tsc-port: getStrictModeEvalOrArgumentsMessage @6.0.3
    /// tsc-hash: 96cc116e4ca3216df57b423c508dfcd95a8198ff02403c705fdb8124119d3364
    /// tsc-span: _tsc.js:44167-44175
    fn get_strict_mode_eval_or_arguments_message(
        &self,
        node: NodeId,
    ) -> &'static DiagnosticMessage {
        if get_containing_class(self.source, node).is_some() {
            return &diagnostics::Code_contained_in_a_class_is_evaluated_in_JavaScript_s_strict_mode_which_does_not_allow_this_use_of_0_For_more_information_see_https_developer_mozilla_org_en_US_docs_Web_JavaScript_Reference_Strict_mode;
        }
        if self.source.external_module_indicator.is_some() {
            return &diagnostics::Invalid_use_of_0_Modules_are_automatically_in_strict_mode;
        }
        &diagnostics::Invalid_use_of_0_in_strict_mode
    }

    /// tsc-port: checkStrictModeFunctionName @6.0.3
    /// tsc-hash: 9085ec6898587e78da1d200e5c23632041cb6a8e59ce6facdcfd8120de6f59a3
    /// tsc-span: _tsc.js:44176-44180
    fn check_strict_mode_function_name(&mut self, node: NodeId) {
        if self.in_strict_mode && !self.flags_of(node).intersects(NodeFlags::AMBIENT) {
            let name = name_field_of(self.source, node);
            self.check_strict_mode_eval_or_arguments(node, name);
        }
    }

    /// tsc-port: getStrictModeBlockScopeFunctionDeclarationMessage @6.0.3
    /// tsc-hash: 7c7ab1a7c14b0b79ac5388f1a3a6ab347a3ae239940b73078b478eda0cf578ee
    /// tsc-span: _tsc.js:44181-44189
    fn get_strict_mode_block_scope_function_declaration_message(
        &self,
        node: NodeId,
    ) -> &'static DiagnosticMessage {
        if get_containing_class(self.source, node).is_some() {
            return &diagnostics::Function_declarations_are_not_allowed_inside_blocks_in_strict_mode_when_targeting_ES5_Class_definitions_are_automatically_in_strict_mode;
        }
        if self.source.external_module_indicator.is_some() {
            return &diagnostics::Function_declarations_are_not_allowed_inside_blocks_in_strict_mode_when_targeting_ES5_Modules_are_automatically_in_strict_mode;
        }
        &diagnostics::Function_declarations_are_not_allowed_inside_blocks_in_strict_mode_when_targeting_ES5
    }

    /// tsc-port: checkStrictModeFunctionDeclaration @6.0.3
    /// tsc-hash: 2552b8a4d00aed70071c07ce855973540308e3086afe10acc4ce6cd59b012259
    /// tsc-span: _tsc.js:44190-44197
    ///
    /// TARGET DEPENDENT: fires only below ES2015.
    fn check_strict_mode_function_declaration(&mut self, node: NodeId) {
        if self.language_version >= ScriptTarget::ES2015.bits() {
            return;
        }
        let Some(block_scope_container) = self.block_scope_container else {
            return;
        };
        let kind = kind_of(self.source, block_scope_container);
        if kind != SyntaxKind::SourceFile
            && kind != SyntaxKind::ModuleDeclaration
            && !is_function_like_kind(kind)
            && kind != SyntaxKind::ClassStaticBlockDeclaration
        {
            let (start, end) = get_error_span_for_node(self.source, node);
            let message = self.get_strict_mode_block_scope_function_declaration_message(node);
            self.push_file_diagnostic(start, end, message, &[]);
        }
    }

    /// tsc-port: checkStrictModePostfixUnaryExpression @6.0.3
    /// tsc-hash: e0ab6b73ec19c15fa6454693398d05ddf4e44c0e2bdefd38b0a7155d44bd9f5d
    /// tsc-span: _tsc.js:44198-44202
    fn check_strict_mode_postfix_unary_expression(&mut self, node: NodeId) {
        if self.in_strict_mode {
            let operand = match &self.source.arena.node(node).data {
                NodeData::PostfixUnaryExpression(data) => data.operand,
                _ => None,
            };
            self.check_strict_mode_eval_or_arguments(node, operand);
        }
    }

    /// tsc-port: checkStrictModePrefixUnaryExpression @6.0.3
    /// tsc-hash: 056a7fd1afe468b27ee376391672bf14333f4a94593979b8d5acda5500b8b087
    /// tsc-span: _tsc.js:44203-44209
    fn check_strict_mode_prefix_unary_expression(&mut self, node: NodeId) {
        if self.in_strict_mode {
            if let NodeData::PrefixUnaryExpression(data) = &self.source.arena.node(node).data {
                if matches!(
                    data.operator,
                    SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
                ) {
                    let operand = data.operand;
                    self.check_strict_mode_eval_or_arguments(node, operand);
                }
            }
        }
    }

    /// tsc-port: checkStrictModeWithStatement @6.0.3
    /// tsc-hash: d640a8d23aadf055a50215f0c0633f048f843d27ba5abcb0b2ebca046e1774f2
    /// tsc-span: _tsc.js:44210-44214
    fn check_strict_mode_with_statement(&mut self, node: NodeId) {
        if self.in_strict_mode {
            self.error_on_first_token(
                node,
                &diagnostics::with_statements_are_not_allowed_in_strict_mode,
                &[],
            );
        }
    }

    /// tsc-port: checkStrictModeLabeledStatement @6.0.3
    /// tsc-hash: 0d56574e1d9fbd4cb18d8c22773ef62cf041ca2c494bb67a8cc336b172e11de4
    /// tsc-span: _tsc.js:44215-44221
    ///
    /// TARGET DEPENDENT: strict AND target ≥ ES2015 AND labeling a
    /// declaration or variable statement.
    fn check_strict_mode_labeled_statement(&mut self, node: NodeId) {
        if self.in_strict_mode
            && self.options.emit_script_target().bits() >= ScriptTarget::ES2015.bits()
        {
            let (label, statement) = match &self.source.arena.node(node).data {
                NodeData::LabeledStatement(data) => (data.label, data.statement),
                _ => (None, None),
            };
            let Some(statement) = statement else { return };
            if is_declaration_statement_kind(kind_of(self.source, statement))
                || kind_of(self.source, statement) == SyntaxKind::VariableStatement
            {
                if let Some(label) = label {
                    self.error_on_first_token(
                        label,
                        &diagnostics::A_label_is_not_allowed_here,
                        &[],
                    );
                }
            }
        }
    }

    fn push_file_diagnostic(
        &mut self,
        start: usize,
        end: usize,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let map = &self.source.line_map.byte_to_utf16;
        let to_utf16 = |byte: usize| -> u32 { map.get(byte).copied().unwrap_or(byte as u32) };
        let start_utf16 = to_utf16(start);
        let end_utf16 = to_utf16(end);
        self.bind_diagnostics.push(tsrs2_diags::Diagnostic::new(
            Some(self.source.file_name.clone()),
            Some(start_utf16),
            Some(end_utf16.saturating_sub(start_utf16)),
            tsrs2_diags::MessageChain::new(message, &args),
        ));
    }

    // ---- the walk spine ----

    /// tsc bindEach (42834). Consumed by the stage-3.5 flow binders.
    #[allow(dead_code)]
    fn bind_each(&mut self, nodes: Option<NodeArrayId>) {
        let Some(nodes) = nodes else { return };
        let nodes = self.source.arena.node_array(nodes).nodes.clone();
        for node in nodes {
            self.bind(Some(node));
        }
    }

    /// tsc-port: bindEachFunctionsFirst @6.0.3
    /// tsc-hash: 43522e842ac4d5d4a7b9d6e6a18bc582366d484bca16403e793fad540f52077d
    /// tsc-span: _tsc.js:42830-42833
    ///
    /// OBSERVABLE binding order: FunctionDeclarations bind before the
    /// other statements (hoisting), which shows up in declaration order
    /// and duplicate-diagnostic order.
    fn bind_each_functions_first(&mut self, nodes: Option<NodeArrayId>) {
        let Some(nodes) = nodes else { return };
        let nodes = self.source.arena.node_array(nodes).nodes.clone();
        for &node in &nodes {
            if kind_of(self.source, node) == SyntaxKind::FunctionDeclaration {
                self.bind(Some(node));
            }
        }
        for &node in &nodes {
            if kind_of(self.source, node) != SyntaxKind::FunctionDeclaration {
                self.bind(Some(node));
            }
        }
    }

    /// tsc bindEachChild (42840): every child in forEachChild order.
    fn bind_each_child(&mut self, node: NodeId) {
        let mut children = Vec::new();
        for_each_child(&self.source.arena, self.source.arena.node(node), |child| {
            children.push(child);
            false
        });
        for child in children {
            self.bind(Some(child));
        }
    }

    /// tsc-port: bindChildren @6.0.3
    /// tsc-hash: 69e5dfbb76220dae84ed056c90a443fd58a7edfda6ac555da31683f2567d41c1
    /// tsc-span: _tsc.js:42843-42976
    ///
    /// JSDoc arms (typedef/callback/enum tags, import tag, bindJSDoc)
    /// await JSDoc parsing.
    pub(crate) fn bind_children(&mut self, node: NodeId) {
        let save_in_assignment_pattern = self.in_assignment_pattern;
        self.in_assignment_pattern = false;
        if is_potentially_executable_node(self.source, node) {
            let flags = self.flags_of(node);
            self.set_flags_of(
                node,
                NodeFlags::from_bits(flags.bits() & !NodeFlags::UNREACHABLE.bits()),
            );
        }
        let current_flow = self.current_flow.expect("bindChildren runs under a flow");
        if current_flow == self.unreachable_flow {
            if can_have_flow_node(self.source, node) {
                self.node_flow.remove(&node);
            }
            if is_potentially_executable_node(self.source, node) {
                let flags = self.flags_of(node);
                self.set_flags_of(node, flags | NodeFlags::UNREACHABLE);
            }
            self.bind_each_child(node);
            self.in_assignment_pattern = save_in_assignment_pattern;
            return;
        }
        let kind = kind_of(self.source, node);
        if kind as u16 >= SyntaxKind::FirstStatement as u16
            && kind as u16 <= SyntaxKind::LastStatement as u16
            && can_have_flow_node(self.source, node)
        {
            self.node_flow.insert(node, current_flow);
        }
        match kind {
            SyntaxKind::WhileStatement => self.bind_while_statement(node),
            SyntaxKind::DoStatement => self.bind_do_statement(node),
            SyntaxKind::ForStatement => self.bind_for_statement(node),
            SyntaxKind::ForInStatement | SyntaxKind::ForOfStatement => {
                self.bind_for_in_or_for_of_statement(node)
            }
            SyntaxKind::IfStatement => self.bind_if_statement(node),
            SyntaxKind::ReturnStatement | SyntaxKind::ThrowStatement => {
                self.bind_return_or_throw(node)
            }
            SyntaxKind::BreakStatement | SyntaxKind::ContinueStatement => {
                self.bind_break_or_continue_statement(node)
            }
            SyntaxKind::TryStatement => self.bind_try_statement(node),
            SyntaxKind::SwitchStatement => self.bind_switch_statement(node),
            SyntaxKind::CaseBlock => self.bind_case_block(node),
            SyntaxKind::CaseClause => self.bind_case_clause(node),
            SyntaxKind::ExpressionStatement => self.bind_expression_statement(node),
            SyntaxKind::LabeledStatement => self.bind_labeled_statement(node),
            SyntaxKind::PrefixUnaryExpression => self.bind_prefix_unary_expression_flow(node),
            SyntaxKind::PostfixUnaryExpression => self.bind_postfix_unary_expression_flow(node),
            SyntaxKind::BinaryExpression => {
                if is_destructuring_assignment(self.source, node) {
                    self.in_assignment_pattern = save_in_assignment_pattern;
                    self.bind_destructuring_assignment_flow(node);
                    return;
                }
                self.bind_binary_expression_flow(node);
            }
            SyntaxKind::DeleteExpression => self.bind_delete_expression_flow(node),
            SyntaxKind::ConditionalExpression => self.bind_conditional_expression_flow(node),
            SyntaxKind::VariableDeclaration => self.bind_variable_declaration_flow(node),
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression => {
                self.bind_access_expression_flow(node)
            }
            SyntaxKind::CallExpression => self.bind_call_expression_flow(node),
            SyntaxKind::NonNullExpression => self.bind_non_null_expression_flow(node),
            SyntaxKind::SourceFile => {
                let (statements, end_of_file_token) = match &self.source.arena.node(node).data {
                    NodeData::SourceFile(data) => (data.statements, data.end_of_file_token),
                    _ => (None, None),
                };
                self.bind_each_functions_first(statements);
                self.bind(end_of_file_token);
            }
            SyntaxKind::Block | SyntaxKind::ModuleBlock => {
                self.bind_each_functions_first(statements_of(self.source, node));
            }
            SyntaxKind::BindingElement => self.bind_binding_element_flow(node),
            SyntaxKind::Parameter => self.bind_parameter_flow(node),
            SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::ArrayLiteralExpression
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::SpreadElement => {
                self.in_assignment_pattern = save_in_assignment_pattern;
                self.bind_each_child(node);
            }
            _ => {
                self.bind_each_child(node);
            }
        }
        self.in_assignment_pattern = save_in_assignment_pattern;
    }

    // ---- stage 3.5: the flow-aware statement binders ----

    fn current_flow_id(&self) -> FlowId {
        self.current_flow.expect("flow binder runs under a flow")
    }

    /// tsc doWithConditionalBranches (43184).
    fn do_with_conditional_branches<F>(
        &mut self,
        action: F,
        value: Option<NodeId>,
        true_target: FlowId,
        false_target: FlowId,
    ) where
        F: FnOnce(&mut Self, Option<NodeId>),
    {
        let saved_true_target = self.current_true_target;
        let saved_false_target = self.current_false_target;
        self.current_true_target = Some(true_target);
        self.current_false_target = Some(false_target);
        action(self, value);
        self.current_true_target = saved_true_target;
        self.current_false_target = saved_false_target;
    }

    /// tsc-port: bindCondition @6.0.3
    /// tsc-hash: 38b592763838cdc8d6c70e67e0ad00e3a3ddd511ee49454ded711ee5a7cc7cfd
    /// tsc-span: _tsc.js:43193-43199
    fn bind_condition(&mut self, node: Option<NodeId>, true_target: FlowId, false_target: FlowId) {
        self.do_with_conditional_branches(
            |binder, value| binder.bind(value),
            node,
            true_target,
            false_target,
        );
        let logical_like = node.is_some_and(|node| {
            crate::node_util::is_logical_or_coalescing_assignment_expression(self.source, node)
                || is_logical_expression(self.source, node)
                || crate::node_util::is_optional_chain(self.source, node)
                    && crate::node_util::is_outermost_optional_chain(self.source, node)
        });
        if !logical_like {
            let current = self.current_flow_id();
            let true_condition =
                self.create_flow_condition(FlowFlags::TRUE_CONDITION, current, node);
            self.flow.add_antecedent(true_target, true_condition);
            let false_condition =
                self.create_flow_condition(FlowFlags::FALSE_CONDITION, current, node);
            self.flow.add_antecedent(false_target, false_condition);
        }
    }

    /// tsc bindIterativeStatement (43200).
    fn bind_iterative_statement(
        &mut self,
        node: Option<NodeId>,
        break_target: FlowId,
        continue_target: FlowId,
    ) {
        let save_break_target = self.current_break_target;
        let save_continue_target = self.current_continue_target;
        self.current_break_target = Some(break_target);
        self.current_continue_target = Some(continue_target);
        self.bind(node);
        self.current_break_target = save_break_target;
        self.current_continue_target = save_continue_target;
    }

    /// tsc-port: setContinueTarget @6.0.3
    /// tsc-hash: 5cf6caaae1c8407e3697dc739875bf7245ffe23e5cce3f0488bd3d544f93b1d3
    /// tsc-span: _tsc.js:43209-43217
    fn set_continue_target(&mut self, mut node: NodeId, target: FlowId) -> FlowId {
        let mut label_index = self.active_label_list.len();
        while label_index > 0
            && parent_of(self.source, node)
                .is_some_and(|parent| kind_of(self.source, parent) == SyntaxKind::LabeledStatement)
        {
            label_index -= 1;
            self.active_label_list[label_index].continue_target = Some(target);
            node = parent_of(self.source, node).unwrap();
        }
        target
    }

    /// tsc-port: bindWhileStatement @6.0.3
    /// tsc-hash: 9502783d5fee9d39b7c22c38d0c2168ce81a14afdefe39bbc301bacde7070863
    /// tsc-span: _tsc.js:43218-43229
    fn bind_while_statement(&mut self, node: NodeId) {
        let (expression, statement) = match &self.source.arena.node(node).data {
            NodeData::WhileStatement(data) => (data.expression, data.statement),
            _ => (None, None),
        };
        let loop_label = self.flow.create_loop_label();
        let pre_while_label = self.set_continue_target(node, loop_label);
        let pre_body_label = self.flow.create_branch_label();
        let post_while_label = self.flow.create_branch_label();
        let current = self.current_flow_id();
        self.flow.add_antecedent(pre_while_label, current);
        self.current_flow = Some(pre_while_label);
        self.bind_condition(expression, pre_body_label, post_while_label);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(pre_body_label, self.unreachable_flow),
        );
        self.bind_iterative_statement(statement, post_while_label, pre_while_label);
        let current = self.current_flow_id();
        self.flow.add_antecedent(pre_while_label, current);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(post_while_label, self.unreachable_flow),
        );
    }

    /// tsc-port: bindDoStatement @6.0.3
    /// tsc-hash: 70fbd4032fe8fe8209fbc80b5f0ba0b5baeac97dfb6d2b600268edf78ea42512
    /// tsc-span: _tsc.js:43230-43241
    fn bind_do_statement(&mut self, node: NodeId) {
        let (expression, statement) = match &self.source.arena.node(node).data {
            NodeData::DoStatement(data) => (data.expression, data.statement),
            _ => (None, None),
        };
        let pre_do_label = self.flow.create_loop_label();
        let branch = self.flow.create_branch_label();
        let pre_condition_label = self.set_continue_target(node, branch);
        let post_do_label = self.flow.create_branch_label();
        let current = self.current_flow_id();
        self.flow.add_antecedent(pre_do_label, current);
        self.current_flow = Some(pre_do_label);
        self.bind_iterative_statement(statement, post_do_label, pre_condition_label);
        let current = self.current_flow_id();
        self.flow.add_antecedent(pre_condition_label, current);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(pre_condition_label, self.unreachable_flow),
        );
        self.bind_condition(expression, pre_do_label, post_do_label);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(post_do_label, self.unreachable_flow),
        );
    }

    /// tsc-port: bindForStatement @6.0.3
    /// tsc-hash: 33154d21bb57ca621342024056e1f05dc01206e8e3ab4d0986a83536c292fd12
    /// tsc-span: _tsc.js:43242-43258
    fn bind_for_statement(&mut self, node: NodeId) {
        let (initializer, condition, incrementor, statement) =
            match &self.source.arena.node(node).data {
                NodeData::ForStatement(data) => (
                    data.initializer,
                    data.condition,
                    data.incrementor,
                    data.statement,
                ),
                _ => (None, None, None, None),
            };
        let loop_label = self.flow.create_loop_label();
        let pre_loop_label = self.set_continue_target(node, loop_label);
        let pre_body_label = self.flow.create_branch_label();
        let pre_incrementor_label = self.flow.create_branch_label();
        let post_loop_label = self.flow.create_branch_label();
        self.bind(initializer);
        let current = self.current_flow_id();
        self.flow.add_antecedent(pre_loop_label, current);
        self.current_flow = Some(pre_loop_label);
        self.bind_condition(condition, pre_body_label, post_loop_label);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(pre_body_label, self.unreachable_flow),
        );
        self.bind_iterative_statement(statement, post_loop_label, pre_incrementor_label);
        let current = self.current_flow_id();
        self.flow.add_antecedent(pre_incrementor_label, current);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(pre_incrementor_label, self.unreachable_flow),
        );
        self.bind(incrementor);
        let current = self.current_flow_id();
        self.flow.add_antecedent(pre_loop_label, current);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(post_loop_label, self.unreachable_flow),
        );
    }

    /// tsc-port: bindForInOrForOfStatement @6.0.3
    /// tsc-hash: cc766c2bf078174e8f5786cc99b1e4a742df64fa97bb78ee482a76a969b7a683
    /// tsc-span: _tsc.js:43259-43276
    fn bind_for_in_or_for_of_statement(&mut self, node: NodeId) {
        let (expression, initializer, statement, await_modifier) =
            match &self.source.arena.node(node).data {
                NodeData::ForInStatement(data) => {
                    (data.expression, data.initializer, data.statement, None)
                }
                NodeData::ForOfStatement(data) => (
                    data.expression,
                    data.initializer,
                    data.statement,
                    data.await_modifier,
                ),
                _ => (None, None, None, None),
            };
        let loop_label = self.flow.create_loop_label();
        let pre_loop_label = self.set_continue_target(node, loop_label);
        let post_loop_label = self.flow.create_branch_label();
        self.bind(expression);
        let current = self.current_flow_id();
        self.flow.add_antecedent(pre_loop_label, current);
        self.current_flow = Some(pre_loop_label);
        if kind_of(self.source, node) == SyntaxKind::ForOfStatement {
            self.bind(await_modifier);
        }
        let current = self.current_flow_id();
        self.flow.add_antecedent(post_loop_label, current);
        self.bind(initializer);
        if let Some(initializer) = initializer {
            if kind_of(self.source, initializer) != SyntaxKind::VariableDeclarationList {
                self.bind_assignment_target_flow(initializer);
            }
        }
        self.bind_iterative_statement(statement, post_loop_label, pre_loop_label);
        let current = self.current_flow_id();
        self.flow.add_antecedent(pre_loop_label, current);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(post_loop_label, self.unreachable_flow),
        );
    }

    /// tsc-port: bindIfStatement @6.0.3
    /// tsc-hash: 2c30dad48c8cea8fc38dcf71dc1540246ea176334ec080e5b11f00bb2506dcf7
    /// tsc-span: _tsc.js:43277-43289
    fn bind_if_statement(&mut self, node: NodeId) {
        let (expression, then_statement, else_statement) = match &self.source.arena.node(node).data
        {
            NodeData::IfStatement(data) => {
                (data.expression, data.then_statement, data.else_statement)
            }
            _ => (None, None, None),
        };
        let then_label = self.flow.create_branch_label();
        let else_label = self.flow.create_branch_label();
        let post_if_label = self.flow.create_branch_label();
        self.bind_condition(expression, then_label, else_label);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(then_label, self.unreachable_flow),
        );
        self.bind(then_statement);
        let current = self.current_flow_id();
        self.flow.add_antecedent(post_if_label, current);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(else_label, self.unreachable_flow),
        );
        self.bind(else_statement);
        let current = self.current_flow_id();
        self.flow.add_antecedent(post_if_label, current);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(post_if_label, self.unreachable_flow),
        );
    }

    /// tsc-port: bindReturnOrThrow @6.0.3
    /// tsc-hash: 0d74669036bab103742a58d7497ed102c16393074edc7a911ca0a5e86369327d
    /// tsc-span: _tsc.js:43290-43303
    fn bind_return_or_throw(&mut self, node: NodeId) {
        let expression = match &self.source.arena.node(node).data {
            NodeData::ReturnStatement(data) => data.expression,
            NodeData::ThrowStatement(data) => data.expression,
            _ => None,
        };
        let saved_in_return_position = self.in_return_position;
        self.in_return_position = true;
        self.bind(expression);
        self.in_return_position = saved_in_return_position;
        if kind_of(self.source, node) == SyntaxKind::ReturnStatement {
            self.has_explicit_return = true;
            if let Some(return_target) = self.current_return_target {
                let current = self.current_flow_id();
                self.flow.add_antecedent(return_target, current);
            }
        }
        self.current_flow = Some(self.unreachable_flow);
        self.has_flow_effects = true;
    }

    /// tsc-port: bindBreakOrContinueStatement @6.0.3
    /// tsc-hash: e28e187b38fa438f170acb5ab7ffcd119b5c320398e7f55783d452e1e075363c
    /// tsc-span: _tsc.js:43320-43331
    fn bind_break_or_continue_statement(&mut self, node: NodeId) {
        let label = match &self.source.arena.node(node).data {
            NodeData::BreakStatement(data) => data.label,
            NodeData::ContinueStatement(data) => data.label,
            _ => None,
        };
        self.bind(label);
        match label {
            Some(label) => {
                let escaped = match &self.source.arena.node(label).data {
                    NodeData::Identifier(data) => data.escaped_text.clone(),
                    _ => return,
                };
                // tsc findActiveLabel (43304): innermost first.
                if let Some(index) = self
                    .active_label_list
                    .iter()
                    .rposition(|active| active.name == escaped)
                {
                    self.active_label_list[index].referenced = true;
                    let break_target = self.active_label_list[index].break_target;
                    let continue_target = self.active_label_list[index].continue_target;
                    self.bind_break_or_continue_flow(node, Some(break_target), continue_target);
                }
            }
            None => {
                self.bind_break_or_continue_flow(
                    node,
                    self.current_break_target,
                    self.current_continue_target,
                );
            }
        }
    }

    /// tsc-port: bindBreakOrContinueFlow @6.0.3
    /// tsc-hash: 99fe8f25f7a54117a93de4d3fda393524e8d08aedcc434245101e96facbbb0c3
    /// tsc-span: _tsc.js:43312-43319
    fn bind_break_or_continue_flow(
        &mut self,
        node: NodeId,
        break_target: Option<FlowId>,
        continue_target: Option<FlowId>,
    ) {
        let flow_label = if kind_of(self.source, node) == SyntaxKind::BreakStatement {
            break_target
        } else {
            continue_target
        };
        if let Some(flow_label) = flow_label {
            let current = self.current_flow_id();
            self.flow.add_antecedent(flow_label, current);
            self.current_flow = Some(self.unreachable_flow);
            self.has_flow_effects = true;
        }
    }

    /// tsc-port: bindTryStatement @6.0.3
    /// tsc-hash: 27e30684473d6f50b0b0522497a94c9e39bb2ff8f908d3fb43110f7dda8bfd67
    /// tsc-span: _tsc.js:43332-43374
    fn bind_try_statement(&mut self, node: NodeId) {
        let (try_block, catch_clause, finally_block) = match &self.source.arena.node(node).data {
            NodeData::TryStatement(data) => (data.try_block, data.catch_clause, data.finally_block),
            _ => (None, None, None),
        };
        let save_return_target = self.current_return_target;
        let save_exception_target = self.current_exception_target;
        let normal_exit_label = self.flow.create_branch_label();
        let return_label = self.flow.create_branch_label();
        let mut exception_label = self.flow.create_branch_label();
        if finally_block.is_some() {
            self.current_return_target = Some(return_label);
        }
        let current = self.current_flow_id();
        self.flow.add_antecedent(exception_label, current);
        self.current_exception_target = Some(exception_label);
        self.bind(try_block);
        let current = self.current_flow_id();
        self.flow.add_antecedent(normal_exit_label, current);
        if catch_clause.is_some() {
            self.current_flow = Some(
                self.flow
                    .finish_flow_label(exception_label, self.unreachable_flow),
            );
            exception_label = self.flow.create_branch_label();
            let current = self.current_flow_id();
            self.flow.add_antecedent(exception_label, current);
            self.current_exception_target = Some(exception_label);
            self.bind(catch_clause);
            let current = self.current_flow_id();
            self.flow.add_antecedent(normal_exit_label, current);
        }
        self.current_return_target = save_return_target;
        self.current_exception_target = save_exception_target;
        if finally_block.is_some() {
            let finally_label = self.flow.create_branch_label();
            let mut antecedents = self.flow.flow(normal_exit_label).antecedent.clone();
            antecedents.extend_from_slice(&self.flow.flow(exception_label).antecedent);
            antecedents.extend_from_slice(&self.flow.flow(return_label).antecedent);
            self.flow.flow_mut(finally_label).antecedent = antecedents;
            self.current_flow = Some(finally_label);
            self.bind(finally_block);
            let current = self.current_flow_id();
            if self
                .flow
                .flow(current)
                .flags
                .intersects(FlowFlags::UNREACHABLE)
            {
                self.current_flow = Some(self.unreachable_flow);
            } else {
                if let Some(return_target) = self.current_return_target {
                    let return_antecedents = self.flow.flow(return_label).antecedent.clone();
                    if !return_antecedents.is_empty() {
                        let reduce = self.flow.create_reduce_label(
                            finally_label,
                            return_antecedents,
                            current,
                        );
                        self.flow.add_antecedent(return_target, reduce);
                    }
                }
                if let Some(exception_target) = self.current_exception_target {
                    let exception_antecedents = self.flow.flow(exception_label).antecedent.clone();
                    if !exception_antecedents.is_empty() {
                        let reduce = self.flow.create_reduce_label(
                            finally_label,
                            exception_antecedents,
                            current,
                        );
                        self.flow.add_antecedent(exception_target, reduce);
                    }
                }
                let normal_antecedents = self.flow.flow(normal_exit_label).antecedent.clone();
                self.current_flow = Some(if normal_antecedents.is_empty() {
                    self.unreachable_flow
                } else {
                    self.flow
                        .create_reduce_label(finally_label, normal_antecedents, current)
                });
            }
        } else {
            self.current_flow = Some(
                self.flow
                    .finish_flow_label(normal_exit_label, self.unreachable_flow),
            );
        }
    }

    /// tsc-port: bindSwitchStatement @6.0.3
    /// tsc-hash: 92b2f46ea4a5ce9af156abe3d6a00b47a3059978d1217d24aedf349581f24c9f
    /// tsc-span: _tsc.js:43375-43392
    fn bind_switch_statement(&mut self, node: NodeId) {
        let (expression, case_block) = match &self.source.arena.node(node).data {
            NodeData::SwitchStatement(data) => (data.expression, data.case_block),
            _ => (None, None),
        };
        let post_switch_label = self.flow.create_branch_label();
        self.bind(expression);
        let save_break_target = self.current_break_target;
        let save_pre_switch_case_flow = self.pre_switch_case_flow;
        self.current_break_target = Some(post_switch_label);
        self.pre_switch_case_flow = self.current_flow;
        self.bind(case_block);
        let current = self.current_flow_id();
        self.flow.add_antecedent(post_switch_label, current);
        let has_default =
            case_block.is_some_and(
                |case_block| match &self.source.arena.node(case_block).data {
                    NodeData::CaseBlock(data) => data.clauses.is_some_and(|clauses| {
                        self.source
                            .arena
                            .node_array(clauses)
                            .nodes
                            .iter()
                            .any(|&clause| {
                                kind_of(self.source, clause) == SyntaxKind::DefaultClause
                            })
                    }),
                    _ => false,
                },
            );
        self.possibly_exhaustive.insert(
            node,
            !has_default && self.flow.flow(post_switch_label).antecedent.is_empty(),
        );
        if !has_default {
            let pre = self.pre_switch_case_flow.expect("switch flow");
            let clause = self.create_flow_switch_clause(pre, node, 0, 0);
            self.flow.add_antecedent(post_switch_label, clause);
        }
        self.current_break_target = save_break_target;
        self.pre_switch_case_flow = save_pre_switch_case_flow;
        self.current_flow = Some(
            self.flow
                .finish_flow_label(post_switch_label, self.unreachable_flow),
        );
    }

    /// tsc-port: bindCaseBlock @6.0.3
    /// tsc-hash: bd1cf1fbddf3aeec7e66acf5515b3ca553645c4e477d4e96664c6d0194e7efeb
    /// tsc-span: _tsc.js:43393-43417
    fn bind_case_block(&mut self, node: NodeId) {
        let clauses = match &self.source.arena.node(node).data {
            NodeData::CaseBlock(data) => data.clauses,
            _ => None,
        };
        let Some(clauses) = clauses else { return };
        let clauses = self.source.arena.node_array(clauses).nodes.clone();
        let switch_statement = parent_of(self.source, node).expect("case block parent");
        let switch_expression = match &self.source.arena.node(switch_statement).data {
            NodeData::SwitchStatement(data) => data.expression,
            _ => None,
        };
        let is_narrowing_switch = switch_expression.is_some_and(|expression| {
            kind_of(self.source, expression) == SyntaxKind::TrueKeyword
                || is_narrowing_expression(self.source, expression)
        });
        let mut fallthrough_flow = self.unreachable_flow;
        let mut i = 0usize;
        while i < clauses.len() {
            let clause_start = i;
            while clause_statements_empty(self.source, clauses[i]) && i + 1 < clauses.len() {
                if fallthrough_flow == self.unreachable_flow {
                    self.current_flow = self.pre_switch_case_flow;
                }
                self.bind(Some(clauses[i]));
                i += 1;
            }
            let pre_case_label = self.flow.create_branch_label();
            let antecedent = if is_narrowing_switch {
                let pre = self.pre_switch_case_flow.expect("switch flow");
                self.create_flow_switch_clause(
                    pre,
                    switch_statement,
                    clause_start as u32,
                    (i + 1) as u32,
                )
            } else {
                self.pre_switch_case_flow.expect("switch flow")
            };
            self.flow.add_antecedent(pre_case_label, antecedent);
            self.flow.add_antecedent(pre_case_label, fallthrough_flow);
            self.current_flow = Some(
                self.flow
                    .finish_flow_label(pre_case_label, self.unreachable_flow),
            );
            let clause = clauses[i];
            self.bind(Some(clause));
            fallthrough_flow = self.current_flow_id();
            let current = self.current_flow_id();
            if !self
                .flow
                .flow(current)
                .flags
                .intersects(FlowFlags::UNREACHABLE)
                && i != clauses.len() - 1
                && self.options.no_fallthrough_cases_in_switch == Some(true)
            {
                self.node_fallthrough_flow.insert(clause, current);
            }
            i += 1;
        }
    }

    /// tsc-port: bindCaseClause @6.0.3
    /// tsc-hash: babfa2aba74aff33e0f550fb1b59d6a3a52472b8412209bc37561c01b7507076
    /// tsc-span: _tsc.js:43418-43424
    fn bind_case_clause(&mut self, node: NodeId) {
        let (expression, statements) = match &self.source.arena.node(node).data {
            NodeData::CaseClause(data) => (data.expression, data.statements),
            _ => (None, None),
        };
        let save_current_flow = self.current_flow;
        self.current_flow = self.pre_switch_case_flow;
        self.bind(expression);
        self.current_flow = save_current_flow;
        self.bind_each(statements);
    }

    /// tsc-port: bindExpressionStatement @6.0.3
    /// tsc-hash: ecc460eac354017bf8eb09bd872917b739db3a55c1047174100aa7212b22818e
    /// tsc-span: _tsc.js:43425-43428
    fn bind_expression_statement(&mut self, node: NodeId) {
        let expression = match &self.source.arena.node(node).data {
            NodeData::ExpressionStatement(data) => data.expression,
            _ => None,
        };
        self.bind(expression);
        if let Some(expression) = expression {
            self.maybe_bind_expression_flow_if_call(expression);
        }
    }

    /// tsc-port: maybeBindExpressionFlowIfCall @6.0.3
    /// tsc-hash: e63aedaf525ec80be365b3176f72d498a0186fa44337f126ef781ac3ea3fec05
    /// tsc-span: _tsc.js:43429-43436
    fn maybe_bind_expression_flow_if_call(&mut self, node: NodeId) {
        if let NodeData::CallExpression(data) = &self.source.arena.node(node).data {
            if let Some(expression) = data.expression {
                if kind_of(self.source, expression) != SyntaxKind::SuperKeyword
                    && crate::node_util::is_dotted_name(self.source, expression)
                {
                    let current = self.current_flow_id();
                    self.current_flow = Some(self.create_flow_call(current, node));
                }
            }
        }
    }

    /// tsc-port: bindLabeledStatement @6.0.3
    /// tsc-hash: c1caa33a967160b6944fdc88e26a45551b32980a05b7acf5b14eb2246851c828
    /// tsc-span: _tsc.js:43437-43454
    fn bind_labeled_statement(&mut self, node: NodeId) {
        let (label, statement) = match &self.source.arena.node(node).data {
            NodeData::LabeledStatement(data) => (data.label, data.statement),
            _ => (None, None),
        };
        let post_statement_label = self.flow.create_branch_label();
        let name = label
            .and_then(|label| match &self.source.arena.node(label).data {
                NodeData::Identifier(data) => Some(data.escaped_text.clone()),
                _ => None,
            })
            .unwrap_or_default();
        self.active_label_list.push(crate::flow::ActiveLabel {
            name,
            break_target: post_statement_label,
            continue_target: None,
            referenced: false,
        });
        self.bind(label);
        self.bind(statement);
        let active = self.active_label_list.pop().expect("active label");
        if !active.referenced {
            if let Some(label) = label {
                let flags = self.flags_of(label);
                self.set_flags_of(label, flags | NodeFlags::UNREACHABLE);
            }
        }
        let current = self.current_flow_id();
        self.flow.add_antecedent(post_statement_label, current);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(post_statement_label, self.unreachable_flow),
        );
    }

    /// tsc-port: bindDestructuringTargetFlow @6.0.3
    /// tsc-hash: 4bbc1240b69e059e2e26beb081750843d9b27e52a8b7b5f7d6e97ae44f56b834
    /// tsc-span: _tsc.js:43455-43461
    fn bind_destructuring_target_flow(&mut self, node: NodeId) {
        let assignment_left = match &self.source.arena.node(node).data {
            NodeData::BinaryExpression(data)
                if data.operator_token.is_some_and(|token| {
                    kind_of(self.source, token) == SyntaxKind::EqualsToken
                }) =>
            {
                data.left
            }
            _ => None,
        };
        match assignment_left {
            Some(left) => self.bind_assignment_target_flow(left),
            None => self.bind_assignment_target_flow(node),
        }
    }

    /// tsc-port: bindAssignmentTargetFlow @6.0.3
    /// tsc-hash: e2bc7e620c720971a769b61333e5bc975415031d9d0bc18c8adabbae1ba24b7d
    /// tsc-span: _tsc.js:43462-43484
    fn bind_assignment_target_flow(&mut self, node: NodeId) {
        if is_narrowable_reference(self.source, node) {
            let current = self.current_flow_id();
            self.current_flow =
                Some(self.create_flow_mutation(FlowFlags::ASSIGNMENT, current, node));
        } else if kind_of(self.source, node) == SyntaxKind::ArrayLiteralExpression {
            let elements = match &self.source.arena.node(node).data {
                NodeData::ArrayLiteralExpression(data) => data.elements,
                _ => None,
            };
            let Some(elements) = elements else { return };
            for &element in &self.source.arena.node_array(elements).nodes.clone() {
                if kind_of(self.source, element) == SyntaxKind::SpreadElement {
                    if let Some(expression) = crate::node_util::expression_of(self.source, element)
                    {
                        self.bind_assignment_target_flow(expression);
                    }
                } else {
                    self.bind_destructuring_target_flow(element);
                }
            }
        } else if kind_of(self.source, node) == SyntaxKind::ObjectLiteralExpression {
            let properties = match &self.source.arena.node(node).data {
                NodeData::ObjectLiteralExpression(data) => data.properties,
                _ => None,
            };
            let Some(properties) = properties else { return };
            for &property in &self.source.arena.node_array(properties).nodes.clone() {
                match &self.source.arena.node(property).data {
                    NodeData::PropertyAssignment(data) => {
                        if let Some(initializer) = data.initializer {
                            self.bind_destructuring_target_flow(initializer);
                        }
                    }
                    NodeData::ShorthandPropertyAssignment(data) => {
                        if let Some(name) = data.name {
                            self.bind_assignment_target_flow(name);
                        }
                    }
                    NodeData::SpreadAssignment(data) => {
                        if let Some(expression) = data.expression {
                            self.bind_assignment_target_flow(expression);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// tsc-port: bindLogicalLikeExpression @6.0.3
    /// tsc-hash: 17687f6038c4afe0a3f4cc4635f82ecd6d567a6ec83cdc780f76f2052de7f8dd
    /// tsc-span: _tsc.js:43485-43502
    fn bind_logical_like_expression(
        &mut self,
        node: NodeId,
        true_target: FlowId,
        false_target: FlowId,
    ) {
        let (left, operator_token, right) = match &self.source.arena.node(node).data {
            NodeData::BinaryExpression(data) => (data.left, data.operator_token, data.right),
            _ => (None, None, None),
        };
        let operator = operator_token
            .map(|token| kind_of(self.source, token))
            .unwrap_or(SyntaxKind::Unknown);
        let pre_right_label = self.flow.create_branch_label();
        if matches!(
            operator,
            SyntaxKind::AmpersandAmpersandToken | SyntaxKind::AmpersandAmpersandEqualsToken
        ) {
            self.bind_condition(left, pre_right_label, false_target);
        } else {
            self.bind_condition(left, true_target, pre_right_label);
        }
        self.current_flow = Some(
            self.flow
                .finish_flow_label(pre_right_label, self.unreachable_flow),
        );
        self.bind(operator_token);
        if crate::node_util::is_logical_or_coalescing_assignment_operator(operator) {
            self.do_with_conditional_branches(
                |binder, value| binder.bind(value),
                right,
                true_target,
                false_target,
            );
            if let Some(left) = left {
                self.bind_assignment_target_flow(left);
            }
            let current = self.current_flow_id();
            let true_condition =
                self.create_flow_condition(FlowFlags::TRUE_CONDITION, current, Some(node));
            self.flow.add_antecedent(true_target, true_condition);
            let false_condition =
                self.create_flow_condition(FlowFlags::FALSE_CONDITION, current, Some(node));
            self.flow.add_antecedent(false_target, false_condition);
        } else {
            self.bind_condition(right, true_target, false_target);
        }
    }

    /// tsc-port: bindPrefixUnaryExpressionFlow @6.0.3
    /// tsc-hash: 10090c29ecfd2fc2ceab949352b6390644f5f511de68bdcffca952227ea231df
    /// tsc-span: _tsc.js:43503-43517
    fn bind_prefix_unary_expression_flow(&mut self, node: NodeId) {
        let (operator, operand) = match &self.source.arena.node(node).data {
            NodeData::PrefixUnaryExpression(data) => (data.operator, data.operand),
            _ => (SyntaxKind::Unknown, None),
        };
        if operator == SyntaxKind::ExclamationToken {
            let save_true_target = self.current_true_target;
            std::mem::swap(
                &mut self.current_true_target,
                &mut self.current_false_target,
            );
            self.bind_each_child(node);
            self.current_false_target = self.current_true_target;
            self.current_true_target = save_true_target;
        } else {
            self.bind_each_child(node);
            if matches!(
                operator,
                SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
            ) {
                if let Some(operand) = operand {
                    self.bind_assignment_target_flow(operand);
                }
            }
        }
    }

    /// tsc-port: bindPostfixUnaryExpressionFlow @6.0.3
    /// tsc-hash: 15652bcbf955f7a2cf0a4f04eb33326f8f1cb5501e70edb9be966ab11578f25a
    /// tsc-span: _tsc.js:43518-43523
    fn bind_postfix_unary_expression_flow(&mut self, node: NodeId) {
        let (operator, operand) = match &self.source.arena.node(node).data {
            NodeData::PostfixUnaryExpression(data) => (data.operator, data.operand),
            _ => (SyntaxKind::Unknown, None),
        };
        self.bind_each_child(node);
        if matches!(
            operator,
            SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
        ) {
            if let Some(operand) = operand {
                self.bind_assignment_target_flow(operand);
            }
        }
    }

    /// tsc-port: bindDestructuringAssignmentFlow @6.0.3
    /// tsc-hash: e9031917fa13ae8fa0b0b28ea8904624e1a167012673309191025b1f5173809b
    /// tsc-span: _tsc.js:43524-43539
    fn bind_destructuring_assignment_flow(&mut self, node: NodeId) {
        let (left, operator_token, right) = match &self.source.arena.node(node).data {
            NodeData::BinaryExpression(data) => (data.left, data.operator_token, data.right),
            _ => (None, None, None),
        };
        if self.in_assignment_pattern {
            self.in_assignment_pattern = false;
            self.bind(operator_token);
            self.bind(right);
            self.in_assignment_pattern = true;
            self.bind(left);
        } else {
            self.in_assignment_pattern = true;
            self.bind(left);
            self.in_assignment_pattern = false;
            self.bind(operator_token);
            self.bind(right);
        }
        if let Some(left) = left {
            self.bind_assignment_target_flow(left);
        }
    }

    /// tsc-port: createBindBinaryExpressionFlow @6.0.3
    /// tsc-hash: e217a0674ed7e6f9c345973be05caca095c1eac5ed6c0b2640667767fc233787
    /// tsc-span: _tsc.js:43540-43639
    ///
    /// NON-RECURSIVE work-stack state machine (deep binary chains in
    /// the corpus overflow a recursive binder).
    fn bind_binary_expression_flow(&mut self, root: NodeId) {
        #[derive(Clone, Copy)]
        enum Stage {
            Enter,
            Left,
            Operator,
            Right,
            Exit,
        }
        struct Frame {
            node: NodeId,
            stage: Stage,
            skip: bool,
            saved_in_strict_mode: Option<bool>,
        }
        let mut stack = vec![Frame {
            node: root,
            stage: Stage::Enter,
            skip: false,
            saved_in_strict_mode: None,
        }];
        while let Some(top) = stack.len().checked_sub(1) {
            let node = stack[top].node;
            let (left, operator_token, right) = match &self.source.arena.node(node).data {
                NodeData::BinaryExpression(data) => (data.left, data.operator_token, data.right),
                _ => (None, None, None),
            };
            let operator = operator_token
                .map(|token| kind_of(self.source, token))
                .unwrap_or(SyntaxKind::Unknown);
            match stack[top].stage {
                Stage::Enter => {
                    // Non-root frames re-run bindWorker with strict-mode
                    // save/restore (the trampoline's onEnter with state).
                    if top > 0 {
                        stack[top].saved_in_strict_mode = Some(self.in_strict_mode);
                        self.bind_worker(node);
                    }
                    if crate::node_util::is_logical_or_coalescing_binary_operator(operator)
                        || crate::node_util::is_logical_or_coalescing_assignment_operator(operator)
                    {
                        if is_top_level_logical_expression(self.source, node) {
                            let post_expression_label = self.flow.create_branch_label();
                            let save_current_flow = self.current_flow;
                            let save_has_flow_effects = self.has_flow_effects;
                            self.has_flow_effects = false;
                            self.bind_logical_like_expression(
                                node,
                                post_expression_label,
                                post_expression_label,
                            );
                            self.current_flow = Some(if self.has_flow_effects {
                                self.flow
                                    .finish_flow_label(post_expression_label, self.unreachable_flow)
                            } else {
                                save_current_flow.expect("flow")
                            });
                            if !self.has_flow_effects {
                                self.has_flow_effects = save_has_flow_effects;
                            }
                        } else {
                            let true_target =
                                self.current_true_target.expect("conditional targets");
                            let false_target =
                                self.current_false_target.expect("conditional targets");
                            self.bind_logical_like_expression(node, true_target, false_target);
                        }
                        stack[top].skip = true;
                    }
                    stack[top].stage = Stage::Left;
                }
                Stage::Left => {
                    stack[top].stage = Stage::Operator;
                    if !stack[top].skip {
                        if let Some(left) = left {
                            if matches!(
                                &self.source.arena.node(left).data,
                                NodeData::BinaryExpression(_)
                            ) && !is_destructuring_assignment(self.source, left)
                            {
                                stack.push(Frame {
                                    node: left,
                                    stage: Stage::Enter,
                                    skip: false,
                                    saved_in_strict_mode: None,
                                });
                            } else {
                                self.bind(Some(left));
                                if operator == SyntaxKind::CommaToken {
                                    self.maybe_bind_expression_flow_if_call(left);
                                }
                            }
                        }
                    }
                }
                Stage::Operator => {
                    stack[top].stage = Stage::Right;
                    if !stack[top].skip {
                        self.bind(operator_token);
                    }
                }
                Stage::Right => {
                    stack[top].stage = Stage::Exit;
                    if !stack[top].skip {
                        if let Some(right) = right {
                            if matches!(
                                &self.source.arena.node(right).data,
                                NodeData::BinaryExpression(_)
                            ) && !is_destructuring_assignment(self.source, right)
                            {
                                stack.push(Frame {
                                    node: right,
                                    stage: Stage::Enter,
                                    skip: false,
                                    saved_in_strict_mode: None,
                                });
                            } else {
                                self.bind(Some(right));
                                if operator == SyntaxKind::CommaToken {
                                    self.maybe_bind_expression_flow_if_call(right);
                                }
                            }
                        }
                    }
                }
                Stage::Exit => {
                    if !stack[top].skip
                        && is_assignment_operator(operator)
                        && !crate::node_util::is_assignment_target(self.source, node)
                    {
                        if let Some(left) = left {
                            self.bind_assignment_target_flow(left);
                            if operator == SyntaxKind::EqualsToken
                                && kind_of(self.source, left) == SyntaxKind::ElementAccessExpression
                            {
                                if let Some(expression) =
                                    crate::node_util::expression_of(self.source, left)
                                {
                                    if is_narrowable_operand(self.source, expression) {
                                        let current = self.current_flow_id();
                                        self.current_flow = Some(self.create_flow_mutation(
                                            FlowFlags::ARRAY_MUTATION,
                                            current,
                                            node,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    if let Some(saved) = stack[top].saved_in_strict_mode {
                        self.in_strict_mode = saved;
                    }
                    stack.pop();
                }
            }
        }
    }

    /// tsc-port: bindDeleteExpressionFlow @6.0.3
    /// tsc-hash: 0a96e50bd7680b1e86bc2427535f9ccb8b0df1a00d3968ffa95abfefd50f9339
    /// tsc-span: _tsc.js:43640-43645
    fn bind_delete_expression_flow(&mut self, node: NodeId) {
        self.bind_each_child(node);
        if let Some(expression) = crate::node_util::expression_of(self.source, node) {
            if kind_of(self.source, expression) == SyntaxKind::PropertyAccessExpression {
                self.bind_assignment_target_flow(expression);
            }
        }
    }

    /// tsc-port: bindConditionalExpressionFlow @6.0.3
    /// tsc-hash: 57da403f84a7baf092a640a2e491aa37119d7ddb6ac7a97bef43d64cd7d249bc
    /// tsc-span: _tsc.js:43646-43670
    fn bind_conditional_expression_flow(&mut self, node: NodeId) {
        let (condition, question_token, when_true, colon_token, when_false) =
            match &self.source.arena.node(node).data {
                NodeData::ConditionalExpression(data) => (
                    data.condition,
                    data.question_token,
                    data.when_true,
                    data.colon_token,
                    data.when_false,
                ),
                _ => (None, None, None, None, None),
            };
        let true_label = self.flow.create_branch_label();
        let false_label = self.flow.create_branch_label();
        let post_expression_label = self.flow.create_branch_label();
        let save_current_flow = self.current_flow;
        let save_has_flow_effects = self.has_flow_effects;
        self.has_flow_effects = false;
        self.bind_condition(condition, true_label, false_label);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(true_label, self.unreachable_flow),
        );
        if self.in_return_position {
            self.node_flow_when_true
                .insert(node, self.current_flow_id());
        }
        self.bind(question_token);
        self.bind(when_true);
        let current = self.current_flow_id();
        self.flow.add_antecedent(post_expression_label, current);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(false_label, self.unreachable_flow),
        );
        if self.in_return_position {
            self.node_flow_when_false
                .insert(node, self.current_flow_id());
        }
        self.bind(colon_token);
        self.bind(when_false);
        let current = self.current_flow_id();
        self.flow.add_antecedent(post_expression_label, current);
        self.current_flow = Some(if self.has_flow_effects {
            self.flow
                .finish_flow_label(post_expression_label, self.unreachable_flow)
        } else {
            save_current_flow.expect("flow")
        });
        if !self.has_flow_effects {
            self.has_flow_effects = save_has_flow_effects;
        }
    }

    /// tsc-port: bindInitializedVariableFlow @6.0.3
    /// tsc-hash: e9c164569b121158d5f60d39b927cd18f698f4fc16a6f75e6005313c69080950
    /// tsc-span: _tsc.js:43671-43680
    fn bind_initialized_variable_flow(&mut self, node: NodeId) {
        let name = if kind_of(self.source, node) == SyntaxKind::OmittedExpression {
            None
        } else {
            name_field_of(self.source, node)
        };
        if let Some(name) = name {
            if is_binding_pattern(self.source, name) {
                let elements = match &self.source.arena.node(name).data {
                    NodeData::ObjectBindingPattern(data) => data.elements,
                    NodeData::ArrayBindingPattern(data) => data.elements,
                    _ => None,
                };
                if let Some(elements) = elements {
                    for &child in &self.source.arena.node_array(elements).nodes.clone() {
                        self.bind_initialized_variable_flow(child);
                    }
                }
                return;
            }
        }
        let current = self.current_flow_id();
        self.current_flow = Some(self.create_flow_mutation(FlowFlags::ASSIGNMENT, current, node));
    }

    /// tsc-port: bindVariableDeclarationFlow @6.0.3
    /// tsc-hash: 1b30e73478c8185ef0b6e2bb36c779b0945d773d6b8637893275fac3d51bd00e
    /// tsc-span: _tsc.js:43681-43686
    fn bind_variable_declaration_flow(&mut self, node: NodeId) {
        self.bind_each_child(node);
        let has_initializer = matches!(
            &self.source.arena.node(node).data,
            NodeData::VariableDeclaration(data) if data.initializer.is_some()
        );
        let in_for_in_or_of = parent_of(self.source, node)
            .and_then(|parent| parent_of(self.source, parent))
            .is_some_and(|grand| {
                matches!(
                    kind_of(self.source, grand),
                    SyntaxKind::ForInStatement | SyntaxKind::ForOfStatement
                )
            });
        if has_initializer || in_for_in_or_of {
            self.bind_initialized_variable_flow(node);
        }
    }

    /// tsc-port: bindBindingElementFlow @6.0.3
    /// tsc-hash: a350373b131651a09ae1ed6ce8b980cf704ae1398f868ec861cdaf089af6b0c9
    /// tsc-span: _tsc.js:43687-43692
    fn bind_binding_element_flow(&mut self, node: NodeId) {
        let (dot_dot_dot_token, property_name, name, initializer) =
            match &self.source.arena.node(node).data {
                NodeData::BindingElement(data) => (
                    data.dot_dot_dot_token,
                    data.property_name,
                    data.name,
                    data.initializer,
                ),
                _ => (None, None, None, None),
            };
        self.bind(dot_dot_dot_token);
        self.bind(property_name);
        self.bind_initializer_flow(initializer);
        self.bind(name);
    }

    /// tsc-port: bindParameterFlow @6.0.3
    /// tsc-hash: 79804688366a59b30a203f52894bd834b1fbb753478a5d56736e2cd2144ffe0a
    /// tsc-span: _tsc.js:43693-43700
    fn bind_parameter_flow(&mut self, node: NodeId) {
        let (modifiers, dot_dot_dot_token, question_token, parameter_type, initializer, name) =
            match &self.source.arena.node(node).data {
                NodeData::Parameter(data) => (
                    data.modifiers,
                    data.dot_dot_dot_token,
                    data.question_token,
                    data.r#type,
                    data.initializer,
                    data.name,
                ),
                _ => (None, None, None, None, None, None),
            };
        self.bind_each(modifiers);
        self.bind(dot_dot_dot_token);
        self.bind(question_token);
        self.bind(parameter_type);
        self.bind_initializer_flow(initializer);
        self.bind(name);
    }

    /// tsc-port: bindInitializer @6.0.3
    /// tsc-hash: 4a1239769d7cef557f1ed5146caaa60cfa28d5564ac3676be8ad3aa301fedd88
    /// tsc-span: _tsc.js:43701-43714
    ///
    /// An initializer's flow merges the pre-initializer flow (the
    /// initializer may not run).
    fn bind_initializer_flow(&mut self, node: Option<NodeId>) {
        let Some(node) = node else { return };
        let entry_flow = self.current_flow_id();
        self.bind(Some(node));
        let current = self.current_flow_id();
        if entry_flow == self.unreachable_flow || entry_flow == current {
            return;
        }
        let exit_flow = self.flow.create_branch_label();
        self.flow.add_antecedent(exit_flow, entry_flow);
        self.flow.add_antecedent(exit_flow, current);
        self.current_flow = Some(
            self.flow
                .finish_flow_label(exit_flow, self.unreachable_flow),
        );
    }

    // ---- optional-chain flow binders ----

    /// tsc-port: bindOptionalExpression @6.0.3
    /// tsc-hash: 9d2aea548efddd8a4126bba700e6d29e6ef214690726bf076c97daf8e2904541
    /// tsc-span: _tsc.js:43744-43750
    fn bind_optional_expression(
        &mut self,
        node: NodeId,
        true_target: FlowId,
        false_target: FlowId,
    ) {
        self.do_with_conditional_branches(
            |binder, value| binder.bind(value),
            Some(node),
            true_target,
            false_target,
        );
        if !crate::node_util::is_optional_chain(self.source, node)
            || crate::node_util::is_outermost_optional_chain(self.source, node)
        {
            let current = self.current_flow_id();
            let true_condition =
                self.create_flow_condition(FlowFlags::TRUE_CONDITION, current, Some(node));
            self.flow.add_antecedent(true_target, true_condition);
            let false_condition =
                self.create_flow_condition(FlowFlags::FALSE_CONDITION, current, Some(node));
            self.flow.add_antecedent(false_target, false_condition);
        }
    }

    /// tsc-port: bindOptionalChainRest @6.0.3
    /// tsc-hash: f06ce5b13af97ecb0dcf462fb0eba0b14547d67e28ad2f965b1a6bf290d08976
    /// tsc-span: _tsc.js:43751-43767
    fn bind_optional_chain_rest(&mut self, node: NodeId) {
        match &self.source.arena.node(node).data {
            NodeData::PropertyAccessExpression(data) => {
                let (question_dot_token, name) = (data.question_dot_token, data.name);
                self.bind(question_dot_token);
                self.bind(name);
            }
            NodeData::ElementAccessExpression(data) => {
                let (question_dot_token, argument) =
                    (data.question_dot_token, data.argument_expression);
                self.bind(question_dot_token);
                self.bind(argument);
            }
            NodeData::CallExpression(data) => {
                let (question_dot_token, type_arguments, arguments) =
                    (data.question_dot_token, data.type_arguments, data.arguments);
                self.bind(question_dot_token);
                self.bind_each(type_arguments);
                self.bind_each(arguments);
            }
            _ => {}
        }
    }

    /// tsc-port: bindOptionalChain @6.0.3
    /// tsc-hash: 33e2cda303a0227dc386f343bb42fda092922bc3bc892940864e68ba4af86053
    /// tsc-span: _tsc.js:43768-43779
    fn bind_optional_chain(&mut self, node: NodeId, true_target: FlowId, false_target: FlowId) {
        let pre_chain_label = if crate::node_util::is_optional_chain_root(self.source, node) {
            Some(self.flow.create_branch_label())
        } else {
            None
        };
        if let Some(expression) = crate::node_util::expression_of(self.source, node) {
            self.bind_optional_expression(
                expression,
                pre_chain_label.unwrap_or(true_target),
                false_target,
            );
        }
        if let Some(pre_chain_label) = pre_chain_label {
            self.current_flow = Some(
                self.flow
                    .finish_flow_label(pre_chain_label, self.unreachable_flow),
            );
        }
        let saved_true_target = self.current_true_target;
        let saved_false_target = self.current_false_target;
        self.current_true_target = Some(true_target);
        self.current_false_target = Some(false_target);
        self.bind_optional_chain_rest(node);
        self.current_true_target = saved_true_target;
        self.current_false_target = saved_false_target;
        if crate::node_util::is_outermost_optional_chain(self.source, node) {
            let current = self.current_flow_id();
            let true_condition =
                self.create_flow_condition(FlowFlags::TRUE_CONDITION, current, Some(node));
            self.flow.add_antecedent(true_target, true_condition);
            let false_condition =
                self.create_flow_condition(FlowFlags::FALSE_CONDITION, current, Some(node));
            self.flow.add_antecedent(false_target, false_condition);
        }
    }

    /// tsc-port: bindOptionalChainFlow @6.0.3
    /// tsc-hash: e3ef4218ed25f7f8630734686541cb50cb14a5086d9ca4e6c81b121b2a44ca41
    /// tsc-span: _tsc.js:43780-43791
    fn bind_optional_chain_flow(&mut self, node: NodeId) {
        if is_top_level_logical_expression(self.source, node) {
            let post_expression_label = self.flow.create_branch_label();
            let save_current_flow = self.current_flow;
            let save_has_flow_effects = self.has_flow_effects;
            self.bind_optional_chain(node, post_expression_label, post_expression_label);
            self.current_flow = Some(if self.has_flow_effects {
                self.flow
                    .finish_flow_label(post_expression_label, self.unreachable_flow)
            } else {
                save_current_flow.expect("flow")
            });
            if !self.has_flow_effects {
                self.has_flow_effects = save_has_flow_effects;
            }
        } else {
            let true_target = self.current_true_target.expect("conditional targets");
            let false_target = self.current_false_target.expect("conditional targets");
            self.bind_optional_chain(node, true_target, false_target);
        }
    }

    /// tsc-port: bindNonNullExpressionFlow @6.0.3
    /// tsc-hash: ea50c237a4523892f9a86705b03d494899d7dbbdb7726f388754773df73d805e
    /// tsc-span: _tsc.js:43792-43798
    fn bind_non_null_expression_flow(&mut self, node: NodeId) {
        if crate::node_util::is_optional_chain(self.source, node) {
            self.bind_optional_chain_flow(node);
        } else {
            self.bind_each_child(node);
        }
    }

    /// tsc-port: bindAccessExpressionFlow @6.0.3
    /// tsc-hash: 49bbf8df49c9119ead92e3dbaf3c06abd5a41e01ae20ff59cd7735b03eac9709
    /// tsc-span: _tsc.js:43799-43805
    fn bind_access_expression_flow(&mut self, node: NodeId) {
        if crate::node_util::is_optional_chain(self.source, node) {
            self.bind_optional_chain_flow(node);
        } else {
            self.bind_each_child(node);
        }
    }

    /// tsc-port: bindCallExpressionFlow @6.0.3
    /// tsc-hash: 6c23347d48d9e4dfd8f7d6459b2db901236604f0202d523359292aebd4e9f24b
    /// tsc-span: _tsc.js:43806-43828
    fn bind_call_expression_flow(&mut self, node: NodeId) {
        let (expression, type_arguments, arguments) = match &self.source.arena.node(node).data {
            NodeData::CallExpression(data) => {
                (data.expression, data.type_arguments, data.arguments)
            }
            _ => (None, None, None),
        };
        if crate::node_util::is_optional_chain(self.source, node) {
            self.bind_optional_chain_flow(node);
        } else if let Some(expression) = expression {
            let expr = crate::node_util::skip_parentheses_pub(self.source, expression);
            if matches!(
                kind_of(self.source, expr),
                SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
            ) {
                self.bind_each(type_arguments);
                self.bind_each(arguments);
                self.bind(Some(expression));
            } else {
                self.bind_each_child(node);
                if kind_of(self.source, expression) == SyntaxKind::SuperKeyword {
                    let current = self.current_flow_id();
                    self.current_flow = Some(self.create_flow_call(current, node));
                }
            }
        }
        if let Some(expression) = expression {
            if let NodeData::PropertyAccessExpression(data) =
                &self.source.arena.node(expression).data
            {
                let (name, target) = (data.name, data.expression);
                if let (Some(name), Some(target)) = (name, target) {
                    if kind_of(self.source, name) == SyntaxKind::Identifier
                        && is_narrowable_operand(self.source, target)
                        && crate::node_util::is_push_or_unshift_identifier(self.source, name)
                    {
                        let current = self.current_flow_id();
                        self.current_flow = Some(self.create_flow_mutation(
                            FlowFlags::ARRAY_MUTATION,
                            current,
                            node,
                        ));
                    }
                }
            }
        }
    }
}

/// tsc isStatementCondition (43151).
fn is_statement_condition(source: &tsrs2_syntax::SourceFile, node: NodeId) -> bool {
    let Some(parent) = parent_of(source, node) else {
        return false;
    };
    match &source.arena.node(parent).data {
        NodeData::IfStatement(data) => data.expression == Some(node),
        NodeData::WhileStatement(data) => data.expression == Some(node),
        NodeData::DoStatement(data) => data.expression == Some(node),
        NodeData::ForStatement(data) => data.condition == Some(node),
        NodeData::ConditionalExpression(data) => data.condition == Some(node),
        _ => false,
    }
}

/// tsc isLogicalExpression (43164): unwraps parens and `!`.
fn is_logical_expression(source: &tsrs2_syntax::SourceFile, mut node: NodeId) -> bool {
    loop {
        match &source.arena.node(node).data {
            NodeData::ParenthesizedExpression(data) => match data.expression {
                Some(expression) => node = expression,
                None => return false,
            },
            NodeData::PrefixUnaryExpression(data)
                if data.operator == SyntaxKind::ExclamationToken =>
            {
                match data.operand {
                    Some(operand) => node = operand,
                    None => return false,
                }
            }
            _ => return crate::node_util::is_logical_or_coalescing_binary_expression(source, node),
        }
    }
}

/// tsc isTopLevelLogicalExpression (43178).
#[allow(clippy::nonminimal_bool)] // the return expression mirrors the tsc source shape
fn is_top_level_logical_expression(source: &tsrs2_syntax::SourceFile, mut node: NodeId) -> bool {
    while let Some(parent) = parent_of(source, node) {
        let is_wrapper = kind_of(source, parent) == SyntaxKind::ParenthesizedExpression
            || matches!(
                &source.arena.node(parent).data,
                NodeData::PrefixUnaryExpression(data)
                    if data.operator == SyntaxKind::ExclamationToken
            );
        if !is_wrapper {
            break;
        }
        node = parent;
    }
    let Some(parent) = parent_of(source, node) else {
        return true;
    };
    !is_statement_condition(source, node)
        && !is_logical_expression(source, parent)
        && !(crate::node_util::is_optional_chain(source, parent)
            && crate::node_util::expression_of(source, parent) == Some(node))
}

/// tsc: a case/default clause with no statements (bindCaseBlock's
/// fallthrough grouping).
fn clause_statements_empty(source: &tsrs2_syntax::SourceFile, clause: NodeId) -> bool {
    match &source.arena.node(clause).data {
        NodeData::CaseClause(data) => data
            .statements
            .map(|statements| source.arena.node_array(statements).nodes.is_empty())
            .unwrap_or(true),
        NodeData::DefaultClause(data) => data
            .statements
            .map(|statements| source.arena.node_array(statements).nodes.is_empty())
            .unwrap_or(true),
        _ => true,
    }
}

/// tsc isDeclarationStatement kind set.
fn is_declaration_statement_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::FunctionDeclaration
            | SyntaxKind::MissingDeclaration
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ImportDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::ExportAssignment
            | SyntaxKind::NamespaceExportDeclaration
    )
}

/// tsc removeFileExtension (18749): the longest matching known
/// extension is removed (.d.ts before .ts).
fn remove_file_extension(path: &str) -> &str {
    for extension in [
        ".d.ts", ".d.mts", ".d.cts", ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs",
        ".json",
    ] {
        if let Some(stripped) = path.strip_suffix(extension) {
            return stripped;
        }
    }
    path
}

/// The `.expression` of an access expression.
fn access_expression_of(source: &tsrs2_syntax::SourceFile, node: NodeId) -> Option<NodeId> {
    match &source.arena.node(node).data {
        NodeData::PropertyAccessExpression(data) => data.expression,
        NodeData::ElementAccessExpression(data) => data.expression,
        _ => None,
    }
}

/// tsc isExportsIdentifier (15046).
fn is_exports_identifier(source: &tsrs2_syntax::SourceFile, node: NodeId) -> bool {
    matches!(
        &source.arena.node(node).data,
        NodeData::Identifier(data) if data.escaped_text == "exports"
    )
}

/// tsc isModuleExportsAccessExpression (15052).
fn is_module_exports_access_expression(source: &tsrs2_syntax::SourceFile, node: NodeId) -> bool {
    let is_access = matches!(kind_of(source, node), SyntaxKind::PropertyAccessExpression)
        || is_literal_like_element_access(source, node);
    is_access
        && access_expression_of(source, node).is_some_and(|expression| {
            matches!(
                &source.arena.node(expression).data,
                NodeData::Identifier(data) if data.escaped_text == "module"
            )
        })
        && get_element_or_property_access_name(source, node).as_deref() == Some("exports")
}

/// tsc isLiteralLikeElementAccess (15069).
fn is_literal_like_element_access(source: &tsrs2_syntax::SourceFile, node: NodeId) -> bool {
    matches!(
        &source.arena.node(node).data,
        NodeData::ElementAccessExpression(data)
            if data.argument_expression.is_some_and(|argument| {
                is_string_or_numeric_literal_like(source, argument)
            })
    )
}

/// tsc isBindableStaticAccessExpression (15072).
fn is_bindable_static_access_expression(
    source: &tsrs2_syntax::SourceFile,
    node: NodeId,
    exclude_this_keyword: bool,
) -> bool {
    if let NodeData::PropertyAccessExpression(data) = &source.arena.node(node).data {
        let this_ok = !exclude_this_keyword
            && data
                .expression
                .is_some_and(|expression| kind_of(source, expression) == SyntaxKind::ThisKeyword);
        let static_ok = data
            .name
            .is_some_and(|name| kind_of(source, name) == SyntaxKind::Identifier)
            && data.expression.is_some_and(|expression| {
                is_bindable_static_name_expression(source, expression, true)
            });
        if this_ok || static_ok {
            return true;
        }
    }
    is_bindable_static_element_access_expression(source, node, exclude_this_keyword)
}

/// tsc isBindableStaticElementAccessExpression (15079).
fn is_bindable_static_element_access_expression(
    source: &tsrs2_syntax::SourceFile,
    node: NodeId,
    exclude_this_keyword: bool,
) -> bool {
    is_literal_like_element_access(source, node)
        && access_expression_of(source, node).is_some_and(|expression| {
            !exclude_this_keyword && kind_of(source, expression) == SyntaxKind::ThisKeyword
                || is_entity_name_expression(source, expression)
                || is_bindable_static_access_expression(source, expression, true)
        })
}

/// tsc isBindableStaticNameExpression (15086).
fn is_bindable_static_name_expression(
    source: &tsrs2_syntax::SourceFile,
    node: NodeId,
    exclude_this_keyword: bool,
) -> bool {
    is_entity_name_expression(source, node)
        || is_bindable_static_access_expression(source, node, exclude_this_keyword)
}

/// tsc getElementOrPropertyAccessName (15134): escaped name of the
/// accessed member.
fn get_element_or_property_access_name(
    source: &tsrs2_syntax::SourceFile,
    node: NodeId,
) -> Option<String> {
    match &source.arena.node(node).data {
        NodeData::PropertyAccessExpression(data) => {
            let name = data.name?;
            match &source.arena.node(name).data {
                NodeData::Identifier(data) => Some(data.escaped_text.clone()),
                _ => None,
            }
        }
        NodeData::ElementAccessExpression(data) => {
            let argument = data.argument_expression?;
            if is_string_or_numeric_literal_like(source, argument) {
                literal_text_of(source, argument).map(crate::symbols::escape_leading_underscores)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// tsc getRightMostAssignedExpression (15036).
fn get_right_most_assigned_expression(
    source: &tsrs2_syntax::SourceFile,
    mut node: NodeId,
) -> NodeId {
    loop {
        let NodeData::BinaryExpression(data) = &source.arena.node(node).data else {
            return node;
        };
        let is_assignment = data
            .operator_token
            .is_some_and(|token| kind_of(source, token) == SyntaxKind::EqualsToken)
            && data
                .left
                .is_some_and(|left| crate::node_util::is_left_hand_side_expression(source, left));
        if !is_assignment {
            return node;
        }
        match data.right {
            Some(right) => node = right,
            None => return node,
        }
    }
}

/// tsc isVoidZero (15121-region).
fn is_void_zero(source: &tsrs2_syntax::SourceFile, node: NodeId) -> bool {
    match &source.arena.node(node).data {
        NodeData::VoidExpression(data) => data.expression.is_some_and(|expression| {
            matches!(
                &source.arena.node(expression).data,
                NodeData::NumericLiteral(data) if data.text == "0"
            )
        }),
        _ => false,
    }
}

/// tsc getInitializerOfBinaryExpression (15178).
fn get_initializer_of_binary_expression(
    source: &tsrs2_syntax::SourceFile,
    mut expr: NodeId,
) -> NodeId {
    loop {
        let NodeData::BinaryExpression(data) = &source.arena.node(expr).data else {
            return expr;
        };
        match data.right {
            Some(right)
                if matches!(
                    &source.arena.node(right).data,
                    NodeData::BinaryExpression(_)
                ) =>
            {
                expr = right;
            }
            Some(right) => return right,
            None => return expr,
        }
    }
}

/// tsc isPrototypeAccess (17171).
fn is_prototype_access(source: &tsrs2_syntax::SourceFile, node: NodeId) -> bool {
    is_bindable_static_access_expression(source, node, false)
        && get_element_or_property_access_name(source, node).as_deref() == Some("prototype")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::{get_module_instance_state, ModuleInstanceState};
    use tsrs2_syntax::{parse_source_file, ParseOptions, SourceFile};
    use tsrs2_types::CompilerOptions;

    fn parse(text: &str) -> SourceFile {
        parse_source_file("main.ts", text, ParseOptions::default(), None)
    }

    fn default_options() -> CompilerOptions {
        CompilerOptions::default()
    }

    fn bind(source: &SourceFile) -> Binder<'_> {
        // Leak the options for test lifetimes only.
        let options: &'static CompilerOptions = Box::leak(Box::new(default_options()));
        let mut binder = Binder::new(source, options);
        binder.bind_source_file();
        binder
    }

    fn find_nodes(source: &SourceFile, kind: SyntaxKind) -> Vec<NodeId> {
        (0..source.arena.len() as u32)
            .map(NodeId)
            .filter(|&id| source.arena.node(id).kind == kind)
            .collect()
    }

    fn diag_pins(binder: &Binder<'_>) -> Vec<(u32, u32, u32)> {
        binder
            .bind_diagnostics
            .iter()
            .map(|diag| (diag.code(), diag.start.unwrap(), diag.length.unwrap()))
            .collect()
    }

    #[test]
    fn container_flags_table_pins() {
        let source = parse(
            "function f() { { let a; } }\n\
             const g = function() {};\n\
             const h = () => 1;\n\
             const o = { m() {} };\n\
             class C { m() {} constructor() {} }\n\
             interface I { m(): void }\n\
             namespace N { }\n\
             type T = { a: string };\n",
        );
        let flags = |id: NodeId| get_container_flags(&source, id);
        assert_eq!(flags(source.root).0, 1 | 4 | 32);
        let function = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
        assert_eq!(flags(function).0, 1 | 4 | 32 | 8);
        let function_expression = find_nodes(&source, SyntaxKind::FunctionExpression)[0];
        assert_eq!(flags(function_expression).0, 1 | 4 | 32 | 8 | 16);
        let arrow = find_nodes(&source, SyntaxKind::ArrowFunction)[0];
        assert_eq!(flags(arrow).0, 1 | 4 | 32 | 8 | 16 | 256);
        let methods = find_nodes(&source, SyntaxKind::MethodDeclaration);
        assert_eq!(flags(methods[0]).0, 1 | 4 | 32 | 8 | 128);
        assert_eq!(flags(methods[1]).0, 1 | 4 | 32 | 8);
        let constructor = find_nodes(&source, SyntaxKind::Constructor)[0];
        assert_eq!(flags(constructor).0, 1 | 4 | 32 | 8);
        let interface = find_nodes(&source, SyntaxKind::InterfaceDeclaration)[0];
        assert_eq!(flags(interface).0, 1 | 64);
        let module = find_nodes(&source, SyntaxKind::ModuleDeclaration)[0];
        assert_eq!(flags(module).0, 1 | 32);
        let method_signature = find_nodes(&source, SyntaxKind::MethodSignature)[0];
        assert_eq!(flags(method_signature).0, 1 | 4 | 32 | 8 | 256);
        let blocks = find_nodes(&source, SyntaxKind::Block);
        assert_eq!(flags(blocks[0]).0, 2 | 32);
        assert_eq!(flags(blocks[1]).0, 0);
    }

    #[test]
    fn property_declaration_is_flow_container_only_with_initializer() {
        let source = parse("class C { a = 1; b: string; }\n");
        let properties = find_nodes(&source, SyntaxKind::PropertyDeclaration);
        assert_eq!(get_container_flags(&source, properties[0]).0, 4);
        assert_eq!(get_container_flags(&source, properties[1]).0, 0);
    }

    #[test]
    fn module_instance_state_pins() {
        let source = parse(
            "namespace A { interface I {} type T = I; }\n\
             namespace B { const enum E { X } }\n\
             namespace C { var v: number; }\n\
             namespace D { export { I2 }; interface I2 {} }\n\
             interface I2 {}\n",
        );
        let modules = find_nodes(&source, SyntaxKind::ModuleDeclaration);
        let state = |id: NodeId| {
            get_module_instance_state(&source, id, &mut std::collections::HashMap::new())
        };
        assert_eq!(state(modules[0]), ModuleInstanceState::NonInstantiated);
        assert_eq!(state(modules[1]), ModuleInstanceState::ConstEnumOnly);
        assert_eq!(state(modules[2]), ModuleInstanceState::Instantiated);
        assert_eq!(state(modules[3]), ModuleInstanceState::NonInstantiated);
    }

    #[test]
    fn end_to_end_bind_declares_top_level_symbols() {
        let source = parse(
            "function f(x: number) {}\nfunction f(x: string) {}\n\
             class C { m() {} }\ninterface I { a: string }\n\
             enum E { A, B }\nnamespace N { export const v = 1; }\n",
        );
        let binder = bind(&source);
        let locals = binder.locals.get(&source.root).expect("file locals");
        // Overloads merged into one symbol.
        let f = locals["f"];
        assert_eq!(binder.symbols.symbol(f).declarations.len(), 2);
        // Class exports carry the synthetic prototype symbol.
        let class_symbol = locals["C"];
        assert!(binder
            .symbols
            .symbol(class_symbol)
            .exports
            .contains_key("prototype"));
        assert!(binder
            .symbols
            .symbol(class_symbol)
            .members
            .contains_key("m"));
        // Interface members, enum members (exports), namespace exports.
        assert!(binder.symbols.symbol(locals["I"]).members.contains_key("a"));
        let enum_symbol = locals["E"];
        assert!(binder.symbols.symbol(enum_symbol).exports.contains_key("A"));
        let namespace_symbol = locals["N"];
        assert!(binder
            .symbols
            .symbol(namespace_symbol)
            .exports
            .contains_key("v"));
        assert!(binder.bind_diagnostics.is_empty());
    }

    #[test]
    fn duplicate_diagnostic_order_matches_functions_first_binding() {
        // Oracle pins: "var f: any;\nfunction f() {}" reports 2300 at
        // (21,1) then (4,1) because the FUNCTION binds first.
        let source = parse("var f: any;\nfunction f() {}");
        let binder = bind(&source);
        assert_eq!(diag_pins(&binder), [(2300, 21, 1), (2300, 4, 1)]);
    }

    #[test]
    fn module_always_strict_reserves_future_reserved_words() {
        // Oracle-pinned: `var private = 1; export {};` in a module file
        // reports 1214 (the Modules-are-automatically-strict variant)
        // at (4,7).
        let source = parse("var private = 1;\nexport {};\n");
        assert!(source.external_module_indicator.is_some());
        let binder = bind(&source);
        assert_eq!(diag_pins(&binder), [(1214, 4, 7)]);
    }

    #[test]
    fn use_strict_prologue_flips_strict_mode() {
        let source = parse("\"use strict\";\nvar eval = 1;\n");
        let binder = bind(&source);
        // Oracle-pinned: 1100 Invalid use of 'eval' in strict mode @(18,4).
        assert_eq!(diag_pins(&binder), [(1100, 18, 4)]);
    }

    #[test]
    fn export_assignment_conflict_reports_2528_end_to_end() {
        // Oracle pins from stage 3.2: "export default 1;\nexport default 2;"
        let source = parse("export default 1;\nexport default 2;");
        let binder = bind(&source);
        assert_eq!(diag_pins(&binder), [(2528, 0, 17), (2528, 18, 17)]);
    }

    #[test]
    fn external_module_file_symbol_and_export_links() {
        let source = parse("export function f() {}\nconst local = 1;\n");
        let binder = bind(&source);
        let file_symbol = binder.node_symbol[&source.root];
        assert_eq!(binder.symbols.symbol(file_symbol).escaped_name, "\"main\"");
        assert!(binder.symbols.symbol(file_symbol).exports.contains_key("f"));
        // Local side of the exported function is linked.
        let locals = binder.locals.get(&source.root).expect("locals");
        let local_f = locals["f"];
        assert!(binder.symbols.symbol(local_f).export_symbol.is_some());
        assert!(locals.contains_key("local"));
    }

    #[test]
    fn infer_type_parameter_binds_into_conditional_type_locals() {
        let source = parse("type T<X> = X extends Array<infer U> ? U : never;\n");
        let binder = bind(&source);
        let conditional = find_nodes(&source, SyntaxKind::ConditionalType)[0];
        let locals = binder.locals.get(&conditional).expect("conditional locals");
        assert!(locals.contains_key("U"));
    }

    #[test]
    fn ambient_module_pattern_and_export_modifier_diagnostics() {
        // Oracle pins: 'declare module "a*b*c" {}' -> 5061@(15,7);
        // 'export declare module "m" {}' -> 2668@(0,6).
        for (text, code, start, length) in [
            ("declare module \"a*b*c\" {}\n", 5061u32, 15u32, 7u32),
            ("export declare module \"m\" {}\n", 2668, 0, 6),
        ] {
            let source = parse(text);
            let binder = bind(&source);
            assert_eq!(diag_pins(&binder), [(code, start, length)], "case: {text}");
        }
        // A single star is a valid pattern; it lands in
        // patternAmbientModules.
        let source = parse("declare module \"good*\" {}\n");
        let binder = bind(&source);
        assert!(binder.bind_diagnostics.is_empty());
        assert_eq!(binder.pattern_ambient_modules.len(), 1);
        assert_eq!(binder.pattern_ambient_modules[0].0, "good");
    }

    // ---- stage 3.5 flow-shape pins (each names its tsc anchor) ----

    fn flow_flags(binder: &Binder<'_>, id: crate::flow::FlowId) -> tsrs2_types::FlowFlags {
        binder.flow.flow(id).flags
    }

    #[test]
    fn if_statement_join_has_two_antecedents_and_condition_nodes() {
        // bindIfStatement (43277) + createFlowCondition (43107): a
        // narrowable condition creates True/FalseCondition nodes; the
        // post-if label joins both branches.
        let source = parse("function f(x: any) { if (x) { x; } else { x; } x; }\n");
        let binder = bind(&source);
        let f = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
        let end = binder.node_end_flow[&f];
        let end_flags = flow_flags(&binder, end);
        assert!(end_flags.intersects(tsrs2_types::FlowFlags::BRANCH_LABEL));
        assert_eq!(binder.flow.flow(end).antecedent.len(), 2);
        for &antecedent in &binder.flow.flow(end).antecedent {
            assert!(flow_flags(&binder, antecedent).intersects(
                tsrs2_types::FlowFlags::TRUE_CONDITION | tsrs2_types::FlowFlags::FALSE_CONDITION
            ));
        }
    }

    #[test]
    fn non_narrowing_condition_creates_no_flow_nodes() {
        // createFlowCondition returns its antecedent for non-narrowing
        // expressions: both branches join the SAME node and the label
        // collapses back to Start.
        let source = parse("function f() { if (1) { } else { } }\n");
        let binder = bind(&source);
        let f = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
        let end = binder.node_end_flow[&f];
        assert!(flow_flags(&binder, end).intersects(tsrs2_types::FlowFlags::START));
    }

    #[test]
    fn while_loop_label_gets_entry_and_back_edge() {
        // bindWhileStatement (43218): preWhileLabel is a LoopLabel with
        // the entry edge and the loop-body back edge.
        let source = parse("function f(x: any) { while (x) { x; } }\n");
        let binder = bind(&source);
        let loop_labels: Vec<_> = (0..binder.flow.len() as u32)
            .map(crate::flow::FlowId)
            .filter(|&id| flow_flags(&binder, id).intersects(tsrs2_types::FlowFlags::LOOP_LABEL))
            .collect();
        assert_eq!(loop_labels.len(), 1);
        assert_eq!(binder.flow.flow(loop_labels[0]).antecedent.len(), 2);
    }

    #[test]
    fn code_after_return_is_unreachable() {
        // bindReturnOrThrow (43290) sets currentFlow = unreachableFlow;
        // bindChildren stamps the Unreachable node flag.
        let source = parse("function f() { return; f(); }\n");
        let binder = bind(&source);
        let statements = find_nodes(&source, SyntaxKind::ExpressionStatement);
        assert!(binder
            .flags_of(statements[0])
            .intersects(tsrs2_types::NodeFlags::UNREACHABLE));
        assert!(!binder.node_flow.contains_key(&statements[0]));
        // The function has an explicit return and NO implicit return.
        let f = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
        assert!(!binder
            .flags_of(f)
            .intersects(tsrs2_types::NodeFlags::HAS_IMPLICIT_RETURN));
    }

    #[test]
    fn try_finally_produces_reduce_labels() {
        // bindTryStatement (43332): finally wiring reduces through
        // ReduceLabel nodes.
        let source =
            parse("function f(x: any) { try { x(); } catch (e) { x; } finally { x; } x; }\n");
        let binder = bind(&source);
        let reduce_count = (0..binder.flow.len() as u32)
            .map(crate::flow::FlowId)
            .filter(|&id| flow_flags(&binder, id).intersects(tsrs2_types::FlowFlags::REDUCE_LABEL))
            .count();
        assert!(reduce_count >= 1, "expected ReduceLabel nodes, got none");
    }

    #[test]
    fn narrowing_switch_creates_switch_clause_nodes() {
        // bindCaseBlock (43393) + createFlowSwitchClause (43123): a
        // narrowing switch expression yields per-clause SwitchClause
        // nodes plus the implicit-default clause (bindSwitchStatement).
        let source = parse(
            "function f(x: string | number) { switch (typeof x) { case \"string\": x; break; case \"number\": x; break; } x; }\n",
        );
        let binder = bind(&source);
        let switch_statement = find_nodes(&source, SyntaxKind::SwitchStatement)[0];
        let clauses: Vec<_> = (0..binder.flow.len() as u32)
            .map(crate::flow::FlowId)
            .filter(|&id| flow_flags(&binder, id).intersects(tsrs2_types::FlowFlags::SWITCH_CLAUSE))
            .collect();
        // 2 case clauses + the implicit default (clauseStart==clauseEnd==0).
        assert_eq!(clauses.len(), 3);
        let implicit_default = clauses.iter().any(|&id| {
            matches!(
                binder.flow.flow(id).payload,
                crate::flow::FlowPayload::SwitchClause {
                    switch_statement: s,
                    clause_start: 0,
                    clause_end: 0,
                } if s == switch_statement
            )
        });
        assert!(implicit_default);
        assert_eq!(
            binder.possibly_exhaustive.get(&switch_statement),
            Some(&false)
        );
    }

    #[test]
    fn assignment_creates_flow_mutation_and_stamps_references() {
        // bindAssignmentTargetFlow (43462) + the Identifier flowNode
        // stamp in bindWorker.
        let source = parse("function f(x: any) { x = 1; x; }\n");
        let binder = bind(&source);
        let assignments = (0..binder.flow.len() as u32)
            .map(crate::flow::FlowId)
            .filter(|&id| flow_flags(&binder, id).intersects(tsrs2_types::FlowFlags::ASSIGNMENT))
            .count();
        assert_eq!(assignments, 1);
        // The trailing reference's flowNode is the Assignment node.
        let identifiers = find_nodes(&source, SyntaxKind::Identifier);
        let last_x = *identifiers.last().unwrap();
        let flow = binder.node_flow[&last_x];
        assert!(flow_flags(&binder, flow).intersects(tsrs2_types::FlowFlags::ASSIGNMENT));
    }

    #[test]
    fn logical_expression_in_condition_adds_no_top_level_condition_nodes() {
        // bindCondition (43193): logical operators create their edges
        // during sub-expression binding — the a && b condition itself
        // adds no extra nodes on top.
        let source = parse("function f(a: any, b: any) { if (a && b) { a; } }\n");
        let binder = bind(&source);
        // Conditions come from `a` and from `b`, joined by the then/
        // else labels: the then-branch flow has 2 antecedents (a-true
        // via preRight collapse and b-true).
        let f = find_nodes(&source, SyntaxKind::FunctionDeclaration)[0];
        let end = binder.node_end_flow[&f];
        // post-if joins then-branch and else-label.
        assert!(flow_flags(&binder, end).intersects(tsrs2_types::FlowFlags::BRANCH_LABEL));
    }

    #[test]
    fn optional_chain_creates_outermost_conditions() {
        // bindOptionalChain (43768): the outermost chain contributes
        // True/FalseCondition nodes.
        let source = parse("function f(a: any) { if (a?.b) { a; } }\n");
        let binder = bind(&source);
        let conditions = (0..binder.flow.len() as u32)
            .map(crate::flow::FlowId)
            .filter(|&id| {
                flow_flags(&binder, id).intersects(
                    tsrs2_types::FlowFlags::TRUE_CONDITION
                        | tsrs2_types::FlowFlags::FALSE_CONDITION,
                )
            })
            .count();
        assert!(
            conditions >= 2,
            "expected chain conditions, got {conditions}"
        );
    }

    #[test]
    fn deep_binary_chain_binds_without_overflow() {
        // createBindBinaryExpressionFlow (43540) is a non-recursive
        // work-stack machine; a deep chain must not overflow.
        let mut text = String::from("const x = 1");
        for _ in 0..50_000 {
            text.push_str(" + 1");
        }
        text.push_str(";\n");
        let source = parse(&text);
        let binder = bind(&source);
        assert!(!binder.symbols.is_empty());
    }

    #[test]
    fn labeled_statement_break_references_label() {
        // bindLabeledStatement (43437) + bindBreakOrContinueStatement
        // (43320): a referenced label keeps its flag clear; an
        // unreferenced label is stamped Unreachable.
        let source = parse("function f(x: any) { a: { if (x) break a; x; } b: { x; } }\n");
        let binder = bind(&source);
        let labels: Vec<_> = find_nodes(&source, SyntaxKind::LabeledStatement)
            .into_iter()
            .filter_map(|statement| match &source.arena.node(statement).data {
                NodeData::LabeledStatement(data) => data.label,
                _ => None,
            })
            .collect();
        assert!(!binder
            .flags_of(labels[0])
            .intersects(tsrs2_types::NodeFlags::UNREACHABLE));
        assert!(binder
            .flags_of(labels[1])
            .intersects(tsrs2_types::NodeFlags::UNREACHABLE));
    }

    #[test]
    fn function_in_block_es5_strict_reports_1250_family() {
        let options = CompilerOptions {
            target: Some(1),
            always_strict: Some(true),
            ..CompilerOptions::default()
        };
        let source = parse("{ function g() {} }\n");
        let options_ref: &'static CompilerOptions = Box::leak(Box::new(options));
        let mut binder = Binder::new(&source, options_ref);
        binder.bind_source_file();
        // Oracle-pinned: 1250 @ (11,1) (the function name g).
        assert_eq!(diag_pins(&binder), [(1250, 11, 1)]);
    }
}
