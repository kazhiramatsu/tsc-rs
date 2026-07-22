//! m2-binder-steps.md stage 3.3: container classification and the
//! scope tree — getContainerFlags, bindContainer, the
//! declareSymbolAndAddToSymbolTable routing family, and
//! bindModuleDeclaration with its instance-state machinery.

use crate::declare::{Binder, TableRef};
use crate::flow::FlowPayload;
use crate::node_util::{
    asterisk_token_of, body_of, get_combined_modifier_flags,
    get_immediately_invoked_function_expression, get_syntactic_modifier_flags,
    has_syntactic_modifier, is_ambient_module, is_function_like_kind,
    is_module_augmentation_external, is_object_literal_or_class_expression_method_or_accessor,
    kind_of, node_is_missing, parent_of, statements_of, try_parse_pattern, ParsedPattern,
};
use crate::symbols::{SymbolId, SymbolTable};
use std::collections::HashMap;
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{for_each_child, NodeData, NodeId, SourceFile, SyntaxKind};
use tsrs2_types::{FlowFlags, ModifierFlags, NodeFlags, SymbolFlags};

/// tsc ContainerFlags (binder-internal, not in the generated enums).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContainerFlags(pub i32);

impl ContainerFlags {
    pub const NONE: Self = Self(0);
    pub const IS_CONTAINER: Self = Self(1);
    pub const IS_BLOCK_SCOPED_CONTAINER: Self = Self(2);
    pub const IS_CONTROL_FLOW_CONTAINER: Self = Self(4);
    pub const IS_FUNCTION_LIKE: Self = Self(8);
    pub const IS_FUNCTION_EXPRESSION: Self = Self(16);
    pub const HAS_LOCALS: Self = Self(32);
    pub const IS_INTERFACE: Self = Self(64);
    pub const IS_OBJECT_LITERAL_OR_CLASS_EXPRESSION_METHOD_OR_ACCESSOR: Self = Self(128);
    pub const PROPAGATES_THIS_KEYWORD: Self = Self(256);

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for ContainerFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// tsc ModuleInstanceState. Ordering is observable: the alias-target
/// walk keeps the MAXIMUM state.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ModuleInstanceState {
    NonInstantiated = 0,
    Instantiated = 1,
    ConstEnumOnly = 2,
}

impl ModuleInstanceState {
    /// tsc compares raw enum values (ConstEnumOnly=2 > Instantiated=1)
    /// and early-returns on Instantiated — mirror the raw comparison.
    fn raw(self) -> u8 {
        self as u8
    }
}

/// tsc-port: getContainerFlags @6.0.3
/// tsc-hash: 762f60f62494f6fa1a80f5b3464b7c4bc552f5cda69ebddff47c8847eaed9a81
/// tsc-span: _tsc.js:45143-45201
///
/// JSDoc kinds (JSDocTypeLiteral/JSDocSignature/JSDocFunctionType/
/// JSDocImportTag) await JSDoc parsing.
pub fn get_container_flags(source: &SourceFile, node: NodeId) -> ContainerFlags {
    match kind_of(source, node) {
        SyntaxKind::ClassExpression
        | SyntaxKind::ClassDeclaration
        | SyntaxKind::EnumDeclaration
        | SyntaxKind::ObjectLiteralExpression
        | SyntaxKind::TypeLiteral
        | SyntaxKind::JsxAttributes => ContainerFlags::IS_CONTAINER,
        SyntaxKind::InterfaceDeclaration => {
            ContainerFlags::IS_CONTAINER | ContainerFlags::IS_INTERFACE
        }
        SyntaxKind::ModuleDeclaration
        | SyntaxKind::TypeAliasDeclaration
        | SyntaxKind::MappedType
        | SyntaxKind::IndexSignature => ContainerFlags::IS_CONTAINER | ContainerFlags::HAS_LOCALS,
        SyntaxKind::SourceFile => {
            ContainerFlags::IS_CONTAINER
                | ContainerFlags::IS_CONTROL_FLOW_CONTAINER
                | ContainerFlags::HAS_LOCALS
        }
        SyntaxKind::GetAccessor | SyntaxKind::SetAccessor | SyntaxKind::MethodDeclaration
            if is_object_literal_or_class_expression_method_or_accessor(source, node) =>
        {
            ContainerFlags::IS_CONTAINER
                | ContainerFlags::IS_CONTROL_FLOW_CONTAINER
                | ContainerFlags::HAS_LOCALS
                | ContainerFlags::IS_FUNCTION_LIKE
                | ContainerFlags::IS_OBJECT_LITERAL_OR_CLASS_EXPRESSION_METHOD_OR_ACCESSOR
        }
        SyntaxKind::GetAccessor
        | SyntaxKind::SetAccessor
        | SyntaxKind::MethodDeclaration
        | SyntaxKind::Constructor
        | SyntaxKind::FunctionDeclaration
        | SyntaxKind::ClassStaticBlockDeclaration => {
            ContainerFlags::IS_CONTAINER
                | ContainerFlags::IS_CONTROL_FLOW_CONTAINER
                | ContainerFlags::HAS_LOCALS
                | ContainerFlags::IS_FUNCTION_LIKE
        }
        SyntaxKind::MethodSignature
        | SyntaxKind::CallSignature
        | SyntaxKind::FunctionType
        | SyntaxKind::ConstructSignature
        | SyntaxKind::ConstructorType => {
            ContainerFlags::IS_CONTAINER
                | ContainerFlags::IS_CONTROL_FLOW_CONTAINER
                | ContainerFlags::HAS_LOCALS
                | ContainerFlags::IS_FUNCTION_LIKE
                | ContainerFlags::PROPAGATES_THIS_KEYWORD
        }
        SyntaxKind::FunctionExpression => {
            ContainerFlags::IS_CONTAINER
                | ContainerFlags::IS_CONTROL_FLOW_CONTAINER
                | ContainerFlags::HAS_LOCALS
                | ContainerFlags::IS_FUNCTION_LIKE
                | ContainerFlags::IS_FUNCTION_EXPRESSION
        }
        SyntaxKind::ArrowFunction => {
            ContainerFlags::IS_CONTAINER
                | ContainerFlags::IS_CONTROL_FLOW_CONTAINER
                | ContainerFlags::HAS_LOCALS
                | ContainerFlags::IS_FUNCTION_LIKE
                | ContainerFlags::IS_FUNCTION_EXPRESSION
                | ContainerFlags::PROPAGATES_THIS_KEYWORD
        }
        SyntaxKind::ModuleBlock => ContainerFlags::IS_CONTROL_FLOW_CONTAINER,
        SyntaxKind::PropertyDeclaration => {
            let has_initializer = matches!(
                &source.arena.node(node).data,
                NodeData::PropertyDeclaration(data) if data.initializer.is_some()
            );
            if has_initializer {
                ContainerFlags::IS_CONTROL_FLOW_CONTAINER
            } else {
                ContainerFlags::NONE
            }
        }
        SyntaxKind::CatchClause
        | SyntaxKind::ForStatement
        | SyntaxKind::ForInStatement
        | SyntaxKind::ForOfStatement
        | SyntaxKind::CaseBlock => {
            ContainerFlags::IS_BLOCK_SCOPED_CONTAINER | ContainerFlags::HAS_LOCALS
        }
        SyntaxKind::Block => {
            let function_like_parent = parent_of(source, node).is_some_and(|parent| {
                is_function_like_kind(kind_of(source, parent))
                    || kind_of(source, parent) == SyntaxKind::ClassStaticBlockDeclaration
            });
            if function_like_parent {
                ContainerFlags::NONE
            } else {
                ContainerFlags::IS_BLOCK_SCOPED_CONTAINER | ContainerFlags::HAS_LOCALS
            }
        }
        _ => ContainerFlags::NONE,
    }
}

impl<'a> Binder<'a> {
    /// tsc container.symbol — the symbol of the current container node.
    pub fn container_symbol(&self) -> Option<SymbolId> {
        self.container
            .and_then(|container| self.node_symbol.get(&container).copied())
    }

    /// tsc isInJSFile: the JavaScriptFile node flag. Every node in a
    /// JS file carries it (parser context flags); the root carries
    /// sourceFlags, so the file-level question reads the root.
    pub(crate) fn is_in_js_file(&self) -> bool {
        self.flags_of(self.source.root)
            .intersects(NodeFlags::JAVA_SCRIPT_FILE)
    }

    /// tsc-port: addToContainerChain @6.0.3
    /// tsc-hash: e24ee70c25ec2d285ceae0b89e0b616a61e0e4dda58077013ac7e891793f3bd3
    /// tsc-span: _tsc.js:43829-43834
    pub fn add_to_container_chain(&mut self, next: NodeId) {
        if let Some(last) = self.last_container {
            self.next_container.insert(last, next);
        }
        self.last_container = Some(next);
    }

    /// The lazy block-scope locals creation from
    /// bindBlockScopedDeclaration: `if (!blockScopeContainer.locals)
    /// { ...createSymbolTable(); addToContainerChain(...) }`.
    pub fn ensure_locals(&mut self, node: NodeId) {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.locals.entry(node) {
            entry.insert(SymbolTable::default());
            self.add_to_container_chain(node);
        }
    }

    /// tsc-port: bindContainer @6.0.3
    /// tsc-hash: fef772b3e91e50f62414b2e5b26c7432798feb04e6372869308938583e7923b7
    /// tsc-span: _tsc.js:42734-42829
    pub fn bind_container(&mut self, node: NodeId, container_flags: ContainerFlags) {
        let save_container = self.container;
        let save_this_parent_container = self.this_parent_container;
        let saved_block_scope_container = self.block_scope_container;
        let saved_in_return_position = self.in_return_position;
        if kind_of(self.source, node) == SyntaxKind::ArrowFunction
            && body_of(self.source, node)
                .is_some_and(|body| kind_of(self.source, body) != SyntaxKind::Block)
        {
            self.in_return_position = true;
        }

        if container_flags.intersects(ContainerFlags::IS_CONTAINER) {
            if kind_of(self.source, node) != SyntaxKind::ArrowFunction {
                self.this_parent_container = self.container;
            }
            self.container = Some(node);
            self.block_scope_container = Some(node);
            if container_flags.intersects(ContainerFlags::HAS_LOCALS) {
                self.locals.insert(node, SymbolTable::default());
                self.add_to_container_chain(node);
            }
        } else if container_flags.intersects(ContainerFlags::IS_BLOCK_SCOPED_CONTAINER) {
            self.block_scope_container = Some(node);
            if container_flags.intersects(ContainerFlags::HAS_LOCALS) {
                // Cleared here, created lazily on the first block-scoped
                // declaration (ensure_locals).
                self.locals.remove(&node);
            }
        }

        if container_flags.intersects(ContainerFlags::IS_CONTROL_FLOW_CONTAINER) {
            let save_current_flow = self.current_flow;
            let save_break_target = self.current_break_target;
            let save_continue_target = self.current_continue_target;
            let save_return_target = self.current_return_target;
            let save_exception_target = self.current_exception_target;
            // 42761 saveActiveLabelList + 42781 `activeLabelList =
            // undefined`: the take is both the save and the clear.
            let save_active_label_list = std::mem::take(&mut self.active_label_list);
            let save_has_explicit_return = self.has_explicit_return;
            let save_seen_this_keyword = self.seen_this_keyword;
            let is_immediately_invoked = (container_flags
                .intersects(ContainerFlags::IS_FUNCTION_EXPRESSION)
                && !has_syntactic_modifier(self.source, node, ModifierFlags::ASYNC)
                && asterisk_token_of(self.source, node).is_none()
                && get_immediately_invoked_function_expression(self.source, node).is_some())
                || kind_of(self.source, node) == SyntaxKind::ClassStaticBlockDeclaration;
            if !is_immediately_invoked {
                let payload = if container_flags.intersects(
                    ContainerFlags::IS_FUNCTION_EXPRESSION
                        | ContainerFlags::IS_OBJECT_LITERAL_OR_CLASS_EXPRESSION_METHOD_OR_ACCESSOR,
                ) {
                    FlowPayload::Node(node)
                } else {
                    FlowPayload::None
                };
                self.current_flow =
                    Some(self.flow.create_flow_node(FlowFlags::START, payload, None));
            }
            self.current_return_target = if is_immediately_invoked
                || kind_of(self.source, node) == SyntaxKind::Constructor
                || self.is_in_js_file()
                    && matches!(
                        kind_of(self.source, node),
                        SyntaxKind::FunctionDeclaration | SyntaxKind::FunctionExpression
                    ) {
                Some(self.flow.create_branch_label())
            } else {
                None
            };
            self.current_exception_target = None;
            self.current_break_target = None;
            self.current_continue_target = None;
            self.has_explicit_return = false;
            self.seen_this_keyword = false;
            self.bind_children(node);
            let mut flags = self.flags_of(node);
            flags = NodeFlags::from_bits(
                flags.bits()
                    & !(NodeFlags::REACHABILITY_AND_EMIT_FLAGS.bits()
                        | NodeFlags::CONTAINS_THIS.bits()),
            );
            let current_flow = self.current_flow.expect("control-flow container has flow");
            if !self
                .flow
                .flow(current_flow)
                .flags
                .intersects(FlowFlags::UNREACHABLE)
                && container_flags.intersects(ContainerFlags::IS_FUNCTION_LIKE)
                && !node_is_missing(self.source, body_of(self.source, node))
            {
                flags |= NodeFlags::HAS_IMPLICIT_RETURN;
                if self.has_explicit_return {
                    flags |= NodeFlags::HAS_EXPLICIT_RETURN;
                }
                self.node_end_flow.insert(node, current_flow);
            }
            if self.seen_this_keyword {
                flags |= NodeFlags::CONTAINS_THIS;
            }
            if kind_of(self.source, node) == SyntaxKind::SourceFile {
                flags = NodeFlags::from_bits(flags.bits() | self.emit_flags);
                self.node_end_flow.insert(node, current_flow);
            }
            self.set_flags_of(node, flags);
            if let Some(return_target) = self.current_return_target {
                self.flow.add_antecedent(return_target, current_flow);
                let finished = self
                    .flow
                    .finish_flow_label(return_target, self.unreachable_flow);
                self.current_flow = Some(finished);
                if kind_of(self.source, node) == SyntaxKind::Constructor
                    || kind_of(self.source, node) == SyntaxKind::ClassStaticBlockDeclaration
                    || self.is_in_js_file()
                        && matches!(
                            kind_of(self.source, node),
                            SyntaxKind::FunctionDeclaration | SyntaxKind::FunctionExpression
                        )
                {
                    self.node_return_flow.insert(node, finished);
                }
            }
            if !is_immediately_invoked {
                self.current_flow = save_current_flow;
            }
            self.current_break_target = save_break_target;
            self.current_continue_target = save_continue_target;
            self.current_return_target = save_return_target;
            self.current_exception_target = save_exception_target;
            self.active_label_list = save_active_label_list;
            self.has_explicit_return = save_has_explicit_return;
            self.seen_this_keyword =
                if container_flags.intersects(ContainerFlags::PROPAGATES_THIS_KEYWORD) {
                    save_seen_this_keyword || self.seen_this_keyword
                } else {
                    save_seen_this_keyword
                };
        } else if container_flags.intersects(ContainerFlags::IS_INTERFACE) {
            let save_seen_this_keyword = self.seen_this_keyword;
            self.seen_this_keyword = false;
            self.bind_children(node);
            let flags = self.flags_of(node);
            self.set_flags_of(
                node,
                if self.seen_this_keyword {
                    flags | NodeFlags::CONTAINS_THIS
                } else {
                    NodeFlags::from_bits(flags.bits() & !NodeFlags::CONTAINS_THIS.bits())
                },
            );
            self.seen_this_keyword = save_seen_this_keyword;
        } else {
            self.bind_children(node);
        }

        self.in_return_position = saved_in_return_position;
        self.container = save_container;
        self.this_parent_container = save_this_parent_container;
        self.block_scope_container = saved_block_scope_container;
    }

    /// tsc-port: declareSymbolAndAddToSymbolTable @6.0.3
    /// tsc-hash: 1d49674ad7f6ff204b4de21213eba7d9990c9ce5574e6cef7e4b1e9dcc0edeb1
    /// tsc-span: _tsc.js:43835-43884
    ///
    /// JSDoc container kinds await JSDoc parsing.
    pub fn declare_symbol_and_add_to_symbol_table(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        symbol_excludes: SymbolFlags,
    ) -> Option<SymbolId> {
        let container = self.container.expect("declaration outside any container");
        match kind_of(self.source, container) {
            SyntaxKind::ModuleDeclaration => {
                Some(self.declare_module_member(node, symbol_flags, symbol_excludes))
            }
            SyntaxKind::SourceFile => {
                Some(self.declare_source_file_member(node, symbol_flags, symbol_excludes))
            }
            SyntaxKind::ClassExpression | SyntaxKind::ClassDeclaration => {
                Some(self.declare_class_member(node, symbol_flags, symbol_excludes))
            }
            SyntaxKind::EnumDeclaration => {
                let container_symbol = self.container_symbol().expect("enum symbol");
                Some(self.declare_symbol(
                    TableRef::Exports(container_symbol),
                    Some(container_symbol),
                    node,
                    symbol_flags,
                    symbol_excludes,
                    false,
                    false,
                ))
            }
            SyntaxKind::TypeLiteral
            | SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::JsxAttributes => {
                let container_symbol = self.container_symbol().expect("members symbol");
                Some(self.declare_symbol(
                    TableRef::Members(container_symbol),
                    Some(container_symbol),
                    node,
                    symbol_flags,
                    symbol_excludes,
                    false,
                    false,
                ))
            }
            SyntaxKind::FunctionType
            | SyntaxKind::ConstructorType
            | SyntaxKind::CallSignature
            | SyntaxKind::ConstructSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::Constructor
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::MappedType => Some(self.declare_symbol(
                TableRef::Locals(container),
                None,
                node,
                symbol_flags,
                symbol_excludes,
                false,
                false,
            )),
            _ => None,
        }
    }

    /// tsc-port: declareClassMember @6.0.3
    /// tsc-hash: bff03dfbf209711cf2cadbc68bd1dd2d94bea6266a59c41cdecf37fe2e506846
    /// tsc-span: _tsc.js:43885-43887
    pub fn declare_class_member(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        symbol_excludes: SymbolFlags,
    ) -> SymbolId {
        let container_symbol = self.container_symbol().expect("class symbol");
        // tsc isStatic: static modifier or a class static block.
        let is_static = get_syntactic_modifier_flags(self.source, node)
            .intersects(ModifierFlags::STATIC)
            || kind_of(self.source, node) == SyntaxKind::ClassStaticBlockDeclaration;
        let table = if is_static {
            TableRef::Exports(container_symbol)
        } else {
            TableRef::Members(container_symbol)
        };
        self.declare_symbol(
            table,
            Some(container_symbol),
            node,
            symbol_flags,
            symbol_excludes,
            false,
            false,
        )
    }

    /// tsc-port: declareSourceFileMember @6.0.3
    /// tsc-hash: 42e6973e730b6a2f5f3bcbb7c5870ba002db844f8e8658a35187916ac2901e18
    /// tsc-span: _tsc.js:43888-43897
    pub fn declare_source_file_member(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        symbol_excludes: SymbolFlags,
    ) -> SymbolId {
        if self.source.external_module_indicator.is_some() {
            self.declare_module_member(node, symbol_flags, symbol_excludes)
        } else {
            self.declare_symbol(
                TableRef::Locals(self.source.root),
                None,
                node,
                symbol_flags,
                symbol_excludes,
                false,
                false,
            )
        }
    }

    /// tsc-port: declareModuleMember @6.0.3
    /// tsc-hash: 321dca7a35fea34529abd83898fd2920715b40d72de2e003dad0b0ae21731ed9
    /// tsc-span: _tsc.js:42675-42733
    ///
    /// JS-only: jsdocTreatAsExported awaits JSDoc parsing (always false
    /// here).
    pub fn declare_module_member(
        &mut self,
        node: NodeId,
        symbol_flags: SymbolFlags,
        symbol_excludes: SymbolFlags,
    ) -> SymbolId {
        let container = self.container.expect("module member outside container");
        let has_export_modifier =
            get_combined_modifier_flags(self.source, node).intersects(ModifierFlags::EXPORT);
        if symbol_flags.intersects(SymbolFlags::ALIAS) {
            if kind_of(self.source, node) == SyntaxKind::ExportSpecifier
                || kind_of(self.source, node) == SyntaxKind::ImportEqualsDeclaration
                    && has_export_modifier
            {
                let container_symbol = self.container_symbol().expect("alias container symbol");
                return self.declare_symbol(
                    TableRef::Exports(container_symbol),
                    Some(container_symbol),
                    node,
                    symbol_flags,
                    symbol_excludes,
                    false,
                    false,
                );
            }
            return self.declare_symbol(
                TableRef::Locals(container),
                None,
                node,
                symbol_flags,
                symbol_excludes,
                false,
                false,
            );
        }
        let export_context = self
            .flags_of(container)
            .intersects(NodeFlags::EXPORT_CONTEXT);
        if !is_ambient_module(self.source, node) && (has_export_modifier || export_context) {
            // tsc: `!canHaveLocals(container) || !container.locals ||
            // (default modifier && !getDeclarationName(node))` — a
            // locals-less container or an unnamed default lands
            // directly in exports.
            let unnamed_default = has_syntactic_modifier(self.source, node, ModifierFlags::DEFAULT)
                && self.get_declaration_name(node).is_none();
            if !self.locals.contains_key(&container) || unnamed_default {
                let container_symbol = self.container_symbol().expect("exports symbol");
                return self.declare_symbol(
                    TableRef::Exports(container_symbol),
                    Some(container_symbol),
                    node,
                    symbol_flags,
                    symbol_excludes,
                    false,
                    false,
                );
            }
            let export_kind = if symbol_flags.intersects(SymbolFlags::VALUE) {
                SymbolFlags::EXPORT_VALUE
            } else {
                SymbolFlags::NONE
            };
            let local = self.declare_symbol(
                TableRef::Locals(container),
                None,
                node,
                export_kind,
                symbol_excludes,
                false,
                false,
            );
            let container_symbol = self.container_symbol().expect("exports symbol");
            let exported = self.declare_symbol(
                TableRef::Exports(container_symbol),
                Some(container_symbol),
                node,
                symbol_flags,
                symbol_excludes,
                false,
                false,
            );
            self.symbols.symbol_mut(local).export_symbol = Some(exported);
            self.node_local_symbol.insert(node, local);
            local
        } else {
            self.declare_symbol(
                TableRef::Locals(container),
                None,
                node,
                symbol_flags,
                symbol_excludes,
                false,
                false,
            )
        }
    }

    /// tsc-port: hasExportDeclarations @6.0.3
    /// tsc-hash: 6facdd1f5c29ff676f0c56a9595aa6c75c61d2c0d42526cddab1a682398ca2b7
    /// tsc-span: _tsc.js:43898-43901
    fn has_export_declarations(&self, node: NodeId) -> bool {
        let body = if kind_of(self.source, node) == SyntaxKind::SourceFile {
            Some(node)
        } else {
            body_of(self.source, node)
                .filter(|&body| kind_of(self.source, body) == SyntaxKind::ModuleBlock)
        };
        let Some(body) = body else { return false };
        let Some(statements) = statements_of(self.source, body) else {
            return false;
        };
        self.source
            .arena
            .node_array(statements)
            .nodes
            .iter()
            .any(|&statement| {
                matches!(
                    kind_of(self.source, statement),
                    SyntaxKind::ExportDeclaration | SyntaxKind::ExportAssignment
                )
            })
    }

    /// tsc-port: setExportContextFlag @6.0.3
    /// tsc-hash: e4c1c48fc0a5693ec28e7a56325e8cd54d87766fc1d1930745ccc0c5e1d74466
    /// tsc-span: _tsc.js:43902-43908
    pub fn set_export_context_flag(&mut self, node: NodeId) {
        let flags = self.flags_of(node);
        if flags.intersects(NodeFlags::AMBIENT) && !self.has_export_declarations(node) {
            self.set_flags_of(node, flags | NodeFlags::EXPORT_CONTEXT);
        } else {
            self.set_flags_of(
                node,
                NodeFlags::from_bits(flags.bits() & !NodeFlags::EXPORT_CONTEXT.bits()),
            );
        }
    }

    /// tsc-port: errorOnFirstToken @6.0.3
    /// tsc-hash: e30484cab3c619fee00083ecb958b86a8dafe84bc148d2ae1fc106539c278a7f
    /// tsc-span: _tsc.js:44222-44225
    pub fn error_on_first_token(
        &mut self,
        node: NodeId,
        message: &'static tsrs2_diags::DiagnosticMessage,
        args: &[&str],
    ) {
        let pos = self.source.arena.node(node).pos as usize;
        let (start, end) = crate::node_util::get_span_of_token_at_position(self.source, pos);
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

    /// tsc-port: bindModuleDeclaration @6.0.3
    /// tsc-hash: 2fcdcb0353bc7be04d1583702b07325bc47f7f9353fe62f56c6358d1e6021a9e
    /// tsc-span: _tsc.js:43909-43936
    pub fn bind_module_declaration(&mut self, node: NodeId) {
        self.set_export_context_flag(node);
        if is_ambient_module(self.source, node) {
            if has_syntactic_modifier(self.source, node, ModifierFlags::EXPORT) {
                self.error_on_first_token(
                    node,
                    &diagnostics::export_modifier_cannot_be_applied_to_ambient_modules_and_module_augmentations_since_they_are_always_visible,
                    &[],
                );
            }
            if is_module_augmentation_external(self.source, node) {
                self.declare_module_symbol(node);
            } else {
                let name = match &self.source.arena.node(node).data {
                    NodeData::ModuleDeclaration(data) => data.name,
                    _ => None,
                };
                let mut pattern: Option<ParsedPattern> = None;
                if let Some(name) = name {
                    if let NodeData::StringLiteral(data) = &self.source.arena.node(name).data {
                        let text = data.text.clone();
                        pattern = try_parse_pattern(&text);
                        if pattern.is_none() {
                            self.error_on_first_token(
                                name,
                                &diagnostics::Pattern_0_can_have_at_most_one_character,
                                &[&text],
                            );
                        }
                    }
                }
                let symbol = self.declare_symbol_and_add_to_symbol_table(
                    node,
                    SymbolFlags::VALUE_MODULE,
                    SymbolFlags::VALUE_MODULE_EXCLUDES,
                );
                if let (Some(ParsedPattern::Wildcard { prefix, suffix }), Some(symbol)) =
                    (pattern, symbol)
                {
                    self.pattern_ambient_modules.push((prefix, suffix, symbol));
                }
            }
        } else {
            let state = self.declare_module_symbol(node);
            if state != ModuleInstanceState::NonInstantiated {
                if let Some(&symbol) = self.node_symbol.get(&node) {
                    let flags = self.symbols.symbol(symbol).flags;
                    let const_enum_only = !flags.intersects(
                        SymbolFlags::FUNCTION | SymbolFlags::CLASS | SymbolFlags::REGULAR_ENUM,
                    ) && state == ModuleInstanceState::ConstEnumOnly
                        && self.symbols.symbol(symbol).const_enum_only_module != Some(false);
                    self.symbols.symbol_mut(symbol).const_enum_only_module = Some(const_enum_only);
                }
            }
        }
    }

    /// tsc-port: declareModuleSymbol @6.0.3
    /// tsc-hash: 21ca198e239d4aa3c6965ee0c8cb7c4c4cd7534a9e6c906a0fa20356e4413843
    /// tsc-span: _tsc.js:43937-43946
    pub fn declare_module_symbol(&mut self, node: NodeId) -> ModuleInstanceState {
        let state = get_module_instance_state(self.source, node, &mut HashMap::new());
        let instantiated = state != ModuleInstanceState::NonInstantiated;
        self.declare_symbol_and_add_to_symbol_table(
            node,
            if instantiated {
                SymbolFlags::VALUE_MODULE
            } else {
                SymbolFlags::NAMESPACE_MODULE
            },
            if instantiated {
                SymbolFlags::VALUE_MODULE_EXCLUDES
            } else {
                SymbolFlags::NAMESPACE_MODULE_EXCLUDES
            },
        );
        state
    }
}

/// tsc-port: getModuleInstanceState @6.0.3
/// tsc-hash: 1a9fc19120f49f71f37213191d8c6a8e8a4c8795b1bf0fdaa6857c7dbda2abed
/// tsc-span: _tsc.js:42278-42288
///
/// The parent-fixup branch is unnecessary here (arena parents are
/// finalized at parse time).
pub fn get_module_instance_state(
    source: &SourceFile,
    node: NodeId,
    visited: &mut HashMap<NodeId, Option<ModuleInstanceState>>,
) -> ModuleInstanceState {
    match body_of(source, node) {
        Some(body) => get_module_instance_state_cached(source, body, visited),
        None => ModuleInstanceState::Instantiated,
    }
}

/// tsc-port: getModuleInstanceStateCached @6.0.3
/// tsc-hash: 704a032fb7d70ab7301cb4e7f6267291fe96771e01865d506906560f864d254a
/// tsc-span: _tsc.js:42289-42298
fn get_module_instance_state_cached(
    source: &SourceFile,
    node: NodeId,
    visited: &mut HashMap<NodeId, Option<ModuleInstanceState>>,
) -> ModuleInstanceState {
    if let Some(cached) = visited.get(&node) {
        return cached.unwrap_or(ModuleInstanceState::NonInstantiated);
    }
    visited.insert(node, None);
    let result = get_module_instance_state_worker(source, node, visited);
    visited.insert(node, Some(result));
    result
}

/// tsc-port: getModuleInstanceStateWorker @6.0.3
/// tsc-hash: 70ea6673fe8ada4bcb3ddca8b67b9d93d3b1a4b5fb9b86944a59c40458f61ed5
/// tsc-span: _tsc.js:42299-42363
fn get_module_instance_state_worker(
    source: &SourceFile,
    node: NodeId,
    visited: &mut HashMap<NodeId, Option<ModuleInstanceState>>,
) -> ModuleInstanceState {
    match &source.arena.node(node).data {
        NodeData::InterfaceDeclaration(_) | NodeData::TypeAliasDeclaration(_) => {
            return ModuleInstanceState::NonInstantiated;
        }
        NodeData::EnumDeclaration(_) => {
            // tsc isEnumConst: combined CONST modifier.
            if get_combined_modifier_flags(source, node).intersects(ModifierFlags::CONST) {
                return ModuleInstanceState::ConstEnumOnly;
            }
        }
        NodeData::ImportDeclaration(_) | NodeData::ImportEqualsDeclaration(_) => {
            if !has_syntactic_modifier(source, node, ModifierFlags::EXPORT) {
                return ModuleInstanceState::NonInstantiated;
            }
        }
        NodeData::ExportDeclaration(data) => {
            if data.module_specifier.is_none() {
                if let Some(clause) = data.export_clause {
                    if let NodeData::NamedExports(named) = &source.arena.node(clause).data {
                        let mut state = ModuleInstanceState::NonInstantiated;
                        if let Some(elements) = named.elements {
                            for &specifier in &source.arena.node_array(elements).nodes {
                                let specifier_state = get_module_instance_state_for_alias_target(
                                    source, specifier, visited,
                                );
                                if specifier_state.raw() > state.raw() {
                                    state = specifier_state;
                                }
                                if state == ModuleInstanceState::Instantiated {
                                    return state;
                                }
                            }
                        }
                        return state;
                    }
                }
            }
        }
        NodeData::ModuleBlock(_) => {
            let mut state = ModuleInstanceState::NonInstantiated;
            let mut children = Vec::new();
            for_each_child(&source.arena, source.arena.node(node), |child| {
                children.push(child);
                false
            });
            for child in children {
                match get_module_instance_state_cached(source, child, visited) {
                    ModuleInstanceState::NonInstantiated => {}
                    ModuleInstanceState::ConstEnumOnly => {
                        state = ModuleInstanceState::ConstEnumOnly;
                    }
                    ModuleInstanceState::Instantiated => {
                        state = ModuleInstanceState::Instantiated;
                        break;
                    }
                }
            }
            return state;
        }
        NodeData::ModuleDeclaration(_) => {
            return get_module_instance_state(source, node, visited);
        }
        NodeData::Identifier(_) => {
            // JS-only: IdentifierIsInJSDocNamespace (bit 4096 on an
            // Identifier) — never set while JSDoc parsing is unported.
            if crate::node_util::node_flags(source, node).intersects(NodeFlags::from_bits(4096)) {
                return ModuleInstanceState::NonInstantiated;
            }
        }
        _ => {}
    }
    ModuleInstanceState::Instantiated
}

/// tsc-port: getModuleInstanceStateForAliasTarget @6.0.3
/// tsc-hash: 63d11b15e7cf7afaed8bb48bdccbe4e8803f3aece463deea0b71fc9a1537cf9d
/// tsc-span: _tsc.js:42364-42403
fn get_module_instance_state_for_alias_target(
    source: &SourceFile,
    specifier: NodeId,
    visited: &mut HashMap<NodeId, Option<ModuleInstanceState>>,
) -> ModuleInstanceState {
    let (property_name, spec_name) = match &source.arena.node(specifier).data {
        NodeData::ExportSpecifier(data) => (data.property_name, data.name),
        _ => (None, None),
    };
    let Some(name) = property_name.or(spec_name) else {
        return ModuleInstanceState::Instantiated;
    };
    if kind_of(source, name) != SyntaxKind::Identifier {
        return ModuleInstanceState::Instantiated;
    }
    let mut p = parent_of(source, specifier);
    while let Some(current) = p {
        let is_scope = matches!(
            kind_of(source, current),
            SyntaxKind::Block | SyntaxKind::ModuleBlock | SyntaxKind::SourceFile
        );
        if is_scope {
            if let Some(statements) = statements_of(source, current) {
                let mut found: Option<ModuleInstanceState> = None;
                for &statement in &source.arena.node_array(statements).nodes {
                    if crate::node_util::node_has_name(source, statement, name) {
                        let state = get_module_instance_state_cached(source, statement, visited);
                        if found.is_none() || state.raw() > found.unwrap().raw() {
                            found = Some(state);
                        }
                        if found == Some(ModuleInstanceState::Instantiated) {
                            return ModuleInstanceState::Instantiated;
                        }
                        if kind_of(source, statement) == SyntaxKind::ImportEqualsDeclaration {
                            found = Some(ModuleInstanceState::Instantiated);
                        }
                    }
                }
                if let Some(found) = found {
                    return found;
                }
            }
        }
        p = parent_of(source, current);
    }
    ModuleInstanceState::Instantiated
}
