//! The bind walk: bind / bindEach / bindEachChild /
//! bindEachFunctionsFirst, the bindChildren dispatch, and stage 3.4's
//! bindWorker with its per-kind symbol arms, the strict-mode check
//! family, and the contextual-identifier checks. The flow-aware
//! bindChildren arms are stage 3.5; the JS special-assignment symbol
//! bodies are stage 3.4c (the dispatch and its early-return shape are
//! here).

use crate::containers::{get_container_flags, ContainerFlags};
use crate::declare::{Binder, TableRef};
use crate::node_util::{
    declaration_name_to_string, get_containing_class,
    get_error_span_for_node, has_dynamic_name, id_text,
    is_assignment_operator, is_async_function, is_auto_accessor_property_declaration,
    is_binding_pattern, is_block_or_catch_scoped, is_entity_name_expression, is_expression_node,
    is_function_like_kind, is_in_top_level_context, is_identifier_name, is_narrowable_reference,
    is_object_literal_method, is_object_literal_or_class_expression_method_or_accessor,
    is_parameter_property_declaration, is_part_of_parameter_declaration, is_part_of_type_query,
    is_string_or_numeric_literal_like, kind_of, literal_text_of, name_field_of, parent_of,
    statements_of,
};
use crate::symbols::{InternalSymbolName, SymbolId};
use tsrs2_diags::{gen as diagnostics, DiagnosticMessage};
use tsrs2_syntax::{for_each_child, NodeArrayId, NodeData, NodeId, SyntaxKind};
use tsrs2_types::{ModifierFlags, NodeFlags, ScriptTarget, SymbolFlags};

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
                            kind_of(self.source, parent)
                                == SyntaxKind::ShorthandPropertyAssignment
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
            SyntaxKind::NamespaceExportDeclaration => {
                self.bind_namespace_export_declaration(node)
            }
            SyntaxKind::ImportClause => self.bind_import_clause(node),
            SyntaxKind::ExportDeclaration => self.bind_export_declaration(node),
            SyntaxKind::ExportAssignment => self.bind_export_assignment(node),
            SyntaxKind::SourceFile => {
                self.update_strict_mode_statement_list(statements_of(self.source, node));
                self.bind_source_file_if_external_module();
            }
            SyntaxKind::Block => {
                let function_like_parent =
                    parent_of(self.source, node).is_some_and(|parent| {
                        is_function_like_kind(kind_of(self.source, parent))
                            || kind_of(self.source, parent)
                                == SyntaxKind::ClassStaticBlockDeclaration
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
        let message: Option<&'static DiagnosticMessage> = if parent
            .is_none_or(|parent| kind_of(self.source, parent) != SyntaxKind::SourceFile)
        {
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
                Some(clause)
                    if kind_of(self.source, clause) == SyntaxKind::NamespaceExport =>
                {
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
        let type_literal_symbol = self
            .symbols
            .alloc(SymbolFlags::TYPE_LITERAL, InternalSymbolName::TYPE.to_owned());
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
        if self.current_flow.is_some()
            && is_object_literal_or_class_expression_method_or_accessor(self.source, node)
        {
            let current_flow = self.current_flow.unwrap();
            self.node_flow.insert(node, current_flow);
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
        if left_expression.is_some_and(|expression| {
            is_bindable_static_name_expression(source, expression, true)
        }) && get_element_or_property_access_name(source, left).as_deref() == Some("prototype")
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
        let NodeData::PropertyAccessExpression(access) = &source.arena.node(expression).data
        else {
            return false;
        };
        let object_ok = access.expression.is_some_and(|object| {
            id_text(source, object) == Some("Object")
        });
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

    /// bindChildren (42843): stage 3.3/3.4 carries the structural
    /// dispatch (functions-first statement lists, inAssignmentPattern
    /// save/restore); the flow-aware statement/expression arms and the
    /// unreachable stamping are stage 3.5.
    pub(crate) fn bind_children(&mut self, node: NodeId) {
        let save_in_assignment_pattern = self.in_assignment_pattern;
        self.in_assignment_pattern = false;
        match kind_of(self.source, node) {
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
        ".d.ts", ".d.mts", ".d.cts", ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs",
        ".cjs", ".json",
    ] {
        if let Some(stripped) = path.strip_suffix(extension) {
            return stripped;
        }
    }
    path
}

/// The `.expression` of an access expression.
fn access_expression_of(
    source: &tsrs2_syntax::SourceFile,
    node: NodeId,
) -> Option<NodeId> {
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
fn is_module_exports_access_expression(
    source: &tsrs2_syntax::SourceFile,
    node: NodeId,
) -> bool {
    let is_access = matches!(
        kind_of(source, node),
        SyntaxKind::PropertyAccessExpression
    ) || is_literal_like_element_access(source, node);
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
                literal_text_of(source, argument)
                    .map(crate::symbols::escape_leading_underscores)
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
            Some(right) if matches!(
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
        let mut state = |id: NodeId| {
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
        assert!(binder.symbols.symbol(class_symbol).members.contains_key("m"));
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
        assert!(binder
            .symbols
            .symbol(file_symbol)
            .exports
            .contains_key("f"));
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
