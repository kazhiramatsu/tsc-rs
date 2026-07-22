//! m2-binder-steps.md stage 3.2: declareSymbol — the merge engine —
//! with its supporting pieces (addDeclarationToSymbol,
//! getDeclarationName, the duplicate-declaration report family).

use std::collections::HashMap;

use crate::node_util::{
    declaration_name_to_string, get_containing_class, get_error_span_for_node,
    get_escaped_text_of_identifier_or_literal, get_escaped_text_of_jsx_namespaced_name,
    get_name_of_declaration, get_text_of_identifier_or_literal, has_dynamic_name,
    has_syntactic_modifier, is_ambient_module, is_global_scope_augmentation,
    is_property_name_literal, is_signed_numeric_literal, is_string_or_numeric_literal_like,
    kind_of, literal_text_of, module_export_name_is_default, node_is_missing,
};
use crate::symbols::{
    escape_leading_underscores, unescape_leading_underscores, InternalSymbolName, SymbolArena,
    SymbolId, SymbolTable,
};
use indexmap::IndexSet;
use tsrs2_diags::{
    gen as diagnostics, Diagnostic, DiagnosticList, DiagnosticMessage, MessageChain, RelatedInfo,
};
use tsrs2_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsrs2_types::{ModifierFlags, SymbolFlags};

/// Which symbol table a declaration lands in. tsc passes the table
/// object; the arena design passes its owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableRef {
    /// `container.locals` of a scope-owning node.
    Locals(NodeId),
    Members(SymbolId),
    Exports(SymbolId),
    /// tsc file.symbol.globalExports (bindNamespaceExportDeclaration).
    GlobalExports(SymbolId),
}

/// The binder for one source file. Grows container/flow state in
/// stages 3.3–3.5; stage 3.2 carries the symbol side only.
pub struct Binder<'a> {
    pub source: &'a SourceFile,
    pub options: &'a tsrs2_types::CompilerOptions,
    /// tsc languageVersion = getEmitScriptTarget(options).
    pub language_version: i32,
    /// tsc file.commonJsModuleIndicator (JS-only; set by
    /// setCommonJsModuleIndicator in stage 3.4c).
    pub common_js_module_indicator: Option<NodeId>,
    pub symbols: SymbolArena,
    /// tsc node.symbol (set by addDeclarationToSymbol).
    pub node_symbol: HashMap<NodeId, SymbolId>,
    /// tsc node.localSymbol (set by declareModuleMember).
    pub node_local_symbol: HashMap<NodeId, SymbolId>,
    /// tsc container.locals, keyed by the scope-owning node.
    pub locals: HashMap<NodeId, SymbolTable>,
    pub bind_diagnostics: DiagnosticList,
    /// TS-file expando parents: function symbols with at least one
    /// bindSpecialPropertyAssignment-shaped member assignment
    /// (44821's function-parent arm). The symbol-producing bodies are
    /// stage 3.4c — until they land, the checker treats member
    /// resolution on these symbols as unreliable (their own members
    /// are unbound) and contains instead of fabricating 2339s.
    pub expando_assignment_targets: std::collections::HashSet<SymbolId>,
    /// tsc file.classifiableNames (insertion-ordered Set).
    pub classifiable_names: IndexSet<String>,
    /// tsc getSymbolId's lazily-assigned global symbol ids; the counter
    /// is program-wide in tsc, so it is seedable for multi-file binds.
    assigned_symbol_ids: HashMap<SymbolId, u32>,
    next_symbol_id: u32,

    // ---- container state (stage 3.3, bindContainer 42734) ----
    pub container: Option<NodeId>,
    pub this_parent_container: Option<NodeId>,
    pub block_scope_container: Option<NodeId>,
    pub last_container: Option<NodeId>,
    /// tsc container.nextContainer chain (addToContainerChain).
    pub next_container: HashMap<NodeId, NodeId>,
    /// tsc mutates node.flags during binding (HasImplicitReturn,
    /// ContainsThis, ExportContext, Unreachable, emit flags); this is
    /// the binder's mutable view, seeded from the parse-time flags.
    /// Parse-time-only readers (node_util) keep reading the arena.
    pub node_flags_mut: Vec<i32>,
    /// tsc file.patternAmbientModules (bindModuleDeclaration).
    pub pattern_ambient_modules: Vec<(String, String, SymbolId)>,

    // ---- flow state (stage 3.3 scaffolding, stage 3.5 fills) ----
    pub flow: crate::flow::FlowArena,
    pub unreachable_flow: crate::flow::FlowId,
    pub current_flow: Option<crate::flow::FlowId>,
    pub current_break_target: Option<crate::flow::FlowId>,
    pub current_continue_target: Option<crate::flow::FlowId>,
    pub current_return_target: Option<crate::flow::FlowId>,
    pub current_true_target: Option<crate::flow::FlowId>,
    pub current_false_target: Option<crate::flow::FlowId>,
    pub current_exception_target: Option<crate::flow::FlowId>,
    pub pre_switch_case_flow: Option<crate::flow::FlowId>,
    /// tsc node.flowNode / endFlowNode / returnFlowNode side tables.
    pub node_flow: HashMap<NodeId, crate::flow::FlowId>,
    pub node_end_flow: HashMap<NodeId, crate::flow::FlowId>,
    pub node_return_flow: HashMap<NodeId, crate::flow::FlowId>,
    /// tsc ConditionalExpression flowNodeWhenTrue/WhenFalse (stamped in
    /// return position, consumed by the checker M5).
    pub node_flow_when_true: HashMap<NodeId, crate::flow::FlowId>,
    pub node_flow_when_false: HashMap<NodeId, crate::flow::FlowId>,
    /// tsc SwitchStatement.possiblyExhaustive.
    pub possibly_exhaustive: HashMap<NodeId, bool>,
    /// tsc clause.fallthroughFlowNode (noFallthroughCasesInSwitch).
    pub node_fallthrough_flow: HashMap<NodeId, crate::flow::FlowId>,
    /// tsc activeLabelList (a stack; tsc uses a linked list).
    pub active_label_list: Vec<crate::flow::ActiveLabel>,

    // ---- walk state ----
    pub in_strict_mode: bool,
    pub seen_this_keyword: bool,
    pub in_assignment_pattern: bool,
    pub has_explicit_return: bool,
    pub in_return_position: bool,
    pub has_flow_effects: bool,
    /// tsc emitFlags (NodeFlags bits accumulated onto the SourceFile).
    pub emit_flags: i32,
}

impl<'a> Binder<'a> {
    pub fn new(source: &'a SourceFile, options: &'a tsrs2_types::CompilerOptions) -> Self {
        Self::with_symbol_id_seed(source, options, 1)
    }

    pub fn with_symbol_id_seed(
        source: &'a SourceFile,
        options: &'a tsrs2_types::CompilerOptions,
        next_symbol_id: u32,
    ) -> Self {
        Self::with_bases(source, options, next_symbol_id, 0)
    }

    /// Program bind (M4 5.0): file N's symbols allocate from
    /// `symbol_base` so SymbolIds are program-unique, mirroring the
    /// parse-side NodeId bases (ParseOptions::node_id_base).
    pub fn with_bases(
        source: &'a SourceFile,
        options: &'a tsrs2_types::CompilerOptions,
        next_symbol_id: u32,
        symbol_base: u32,
    ) -> Self {
        let mut flow = crate::flow::FlowArena::default();
        // tsc createBinder: unreachableFlow is allocated once up front.
        let unreachable_flow = flow.create_flow_node(
            tsrs2_types::FlowFlags::UNREACHABLE,
            crate::flow::FlowPayload::None,
            None,
        );
        Self {
            source,
            options,
            language_version: options.emit_script_target().bits(),
            common_js_module_indicator: None,
            symbols: SymbolArena::with_base(symbol_base),
            node_symbol: HashMap::new(),
            node_local_symbol: HashMap::new(),
            locals: HashMap::new(),
            bind_diagnostics: Vec::new(),
            expando_assignment_targets: std::collections::HashSet::new(),
            classifiable_names: IndexSet::new(),
            assigned_symbol_ids: HashMap::new(),
            next_symbol_id,
            container: None,
            this_parent_container: None,
            block_scope_container: None,
            last_container: None,
            next_container: HashMap::new(),
            node_flags_mut: source.arena.nodes().iter().map(|node| node.flags).collect(),
            pattern_ambient_modules: Vec::new(),
            flow,
            unreachable_flow,
            current_flow: None,
            current_break_target: None,
            current_continue_target: None,
            current_return_target: None,
            current_true_target: None,
            current_false_target: None,
            current_exception_target: None,
            pre_switch_case_flow: None,
            node_flow: HashMap::new(),
            node_end_flow: HashMap::new(),
            node_return_flow: HashMap::new(),
            node_flow_when_true: HashMap::new(),
            node_flow_when_false: HashMap::new(),
            possibly_exhaustive: HashMap::new(),
            node_fallthrough_flow: HashMap::new(),
            active_label_list: Vec::new(),
            in_strict_mode: false,
            seen_this_keyword: false,
            in_assignment_pattern: false,
            has_explicit_return: false,
            in_return_position: false,
            has_flow_effects: false,
            emit_flags: 0,
        }
    }

    /// The binder's mutable view of tsc node.flags. `node_flags_mut` is
    /// indexed by the file-local node index (program binds parse each
    /// file with a NodeId base — see ParseOptions::node_id_base).
    pub fn flags_of(&self, node: NodeId) -> tsrs2_types::NodeFlags {
        let index = (node.0 - self.source.arena.node_base()) as usize;
        tsrs2_types::NodeFlags::from_bits(self.node_flags_mut[index])
    }

    pub fn set_flags_of(&mut self, node: NodeId, flags: tsrs2_types::NodeFlags) {
        let index = (node.0 - self.source.arena.node_base()) as usize;
        self.node_flags_mut[index] = flags.bits();
    }

    pub fn next_symbol_id(&self) -> u32 {
        self.next_symbol_id
    }

    fn table(&mut self, table: TableRef) -> &SymbolTable {
        match table {
            TableRef::Locals(node) => self.locals.entry(node).or_default(),
            TableRef::Members(symbol) => &self.symbols.symbol(symbol).members,
            TableRef::Exports(symbol) => &self.symbols.symbol(symbol).exports,
            TableRef::GlobalExports(symbol) => &self.symbols.symbol(symbol).global_exports,
        }
    }

    fn table_mut(&mut self, table: TableRef) -> &mut SymbolTable {
        match table {
            TableRef::Locals(node) => self.locals.entry(node).or_default(),
            TableRef::Members(symbol) => &mut self.symbols.symbol_mut(symbol).members,
            TableRef::Exports(symbol) => &mut self.symbols.symbol_mut(symbol).exports,
            TableRef::GlobalExports(symbol) => &mut self.symbols.symbol_mut(symbol).global_exports,
        }
    }

    /// tsc createSymbol (42513): allocation + the symbolCount bump
    /// (arena length doubles as file.symbolCount).
    fn create_symbol(&mut self, flags: SymbolFlags, name: String) -> SymbolId {
        self.symbols.alloc(flags, name)
    }

    /// tsc getSymbolId: ids are assigned lazily from a global counter.
    pub fn get_symbol_id(&mut self, symbol: SymbolId) -> u32 {
        if let Some(&id) = self.assigned_symbol_ids.get(&symbol) {
            return id;
        }
        let id = self.next_symbol_id;
        self.next_symbol_id += 1;
        self.assigned_symbol_ids.insert(symbol, id);
        id
    }

    /// tsc-port: declareSymbol @6.0.3
    /// tsc-hash: cb8ed21f44a66ba3e0ee2c2bbdcc066276c64ca5f4a0cd18d8c8f87883cec24e
    /// tsc-span: _tsc.js:42602-42674
    #[allow(clippy::too_many_arguments)]
    pub fn declare_symbol(
        &mut self,
        table: TableRef,
        parent: Option<SymbolId>,
        node: NodeId,
        includes: SymbolFlags,
        excludes: SymbolFlags,
        is_replaceable_by_method: bool,
        is_computed_name: bool,
    ) -> SymbolId {
        debug_assert!(is_computed_name || !has_dynamic_name(self.source, node));
        let is_default_export = has_syntactic_modifier(self.source, node, ModifierFlags::DEFAULT)
            || kind_of(self.source, node) == SyntaxKind::ExportSpecifier
                && self.export_specifier_name_is_default(node);

        let name: Option<String> = if is_computed_name {
            Some(InternalSymbolName::COMPUTED.to_owned())
        } else if is_default_export && parent.is_some() {
            Some(InternalSymbolName::DEFAULT.to_owned())
        } else {
            self.get_declaration_name(node)
        };

        let symbol = match name {
            None => self.create_symbol(SymbolFlags::NONE, InternalSymbolName::MISSING.to_owned()),
            Some(name) => {
                if includes.intersects(SymbolFlags::CLASSIFIABLE) {
                    self.classifiable_names.insert(name.clone());
                }
                let existing = self.table(table).get(&name).copied();
                match existing {
                    None => {
                        let symbol = self.create_symbol(SymbolFlags::NONE, name.clone());
                        self.table_mut(table).insert(name, symbol);
                        if is_replaceable_by_method {
                            self.symbols.symbol_mut(symbol).is_replaceable_by_method = true;
                        }
                        symbol
                    }
                    Some(existing)
                        if is_replaceable_by_method
                            && !self.symbols.symbol(existing).is_replaceable_by_method =>
                    {
                        // A replaceable-by-method binding cannot replace
                        // an ordinary symbol: keep the existing one and
                        // do NOT add this declaration.
                        return existing;
                    }
                    Some(existing) if self.symbols.symbol(existing).flags.intersects(excludes) => {
                        if self.symbols.symbol(existing).is_replaceable_by_method {
                            let symbol = self.create_symbol(SymbolFlags::NONE, name.clone());
                            self.table_mut(table).insert(name, symbol);
                            symbol
                        } else if !(includes.intersects(SymbolFlags::VARIABLE)
                            && self
                                .symbols
                                .symbol(existing)
                                .flags
                                .intersects(SymbolFlags::ASSIGNMENT))
                        {
                            self.report_duplicate(existing, node, includes, is_default_export);
                            // The FRESH symbol is detached — the table
                            // keeps the original, so later duplicates
                            // keep conflicting against it.
                            self.create_symbol(SymbolFlags::NONE, name)
                        } else {
                            // JS var/assignment-declaration merge.
                            existing
                        }
                    }
                    Some(existing) => existing, // clean merge
                }
            }
        };

        self.add_declaration_to_symbol(symbol, node, includes);
        let symbol_parent = self.symbols.symbol(symbol).parent;
        match symbol_parent {
            Some(existing_parent) => {
                debug_assert!(
                    Some(existing_parent) == parent,
                    "Existing symbol parent should match new one"
                );
            }
            None => self.symbols.symbol_mut(symbol).parent = parent,
        }
        symbol
    }

    /// tsc moduleExportNameIsDefault(node.name) for an ExportSpecifier.
    fn export_specifier_name_is_default(&self, node: NodeId) -> bool {
        match &self.source.arena.node(node).data {
            NodeData::ExportSpecifier(data) => data
                .name
                .is_some_and(|name| module_export_name_is_default(self.source, name)),
            _ => false,
        }
    }

    /// tsc-port: addDeclarationToSymbol @6.0.3
    /// tsc-hash: b4e0085a801d7f096cc88364fcea1a1e90f84f85df7658320276ff034e9368ad
    /// tsc-span: _tsc.js:42517-42533
    ///
    /// The members/exports table-creation arms are existence-only in
    /// tsc (tables here always exist, see symbols.rs).
    pub fn add_declaration_to_symbol(
        &mut self,
        symbol: SymbolId,
        node: NodeId,
        symbol_flags: SymbolFlags,
    ) {
        let sym = self.symbols.symbol_mut(symbol);
        sym.flags |= symbol_flags;
        self.node_symbol.insert(node, symbol);
        // appendIfUnique
        if !self.symbols.symbol(symbol).declarations.contains(&node) {
            self.symbols.symbol_mut(symbol).declarations.push(node);
        }
        let sym = self.symbols.symbol_mut(symbol);
        if sym.const_enum_only_module == Some(true)
            && sym
                .flags
                .intersects(SymbolFlags::FUNCTION | SymbolFlags::CLASS | SymbolFlags::REGULAR_ENUM)
        {
            sym.const_enum_only_module = Some(false);
        }
        if symbol_flags.intersects(SymbolFlags::VALUE) {
            self.set_value_declaration(symbol, node);
        }
    }

    /// tsc-port: setValueDeclaration @6.0.3
    /// tsc-hash: a59d9538fb29e56c3a8225e23c78e2a2c0e3570f1bbc442be1dcc2ed93436dac
    /// tsc-span: _tsc.js:15190-15195
    pub(crate) fn set_value_declaration(&mut self, symbol: SymbolId, node: NodeId) {
        let value_declaration = self.symbols.symbol(symbol).value_declaration;
        let replace = match value_declaration {
            None => true,
            Some(value_declaration) => {
                let node_ambient = crate::node_util::node_flags(self.source, node)
                    .intersects(tsrs2_types::NodeFlags::AMBIENT);
                let value_ambient = crate::node_util::node_flags(self.source, value_declaration)
                    .intersects(tsrs2_types::NodeFlags::AMBIENT);
                let in_js = self.in_js_file();
                (!(node_ambient && !in_js && !value_ambient)
                    && (is_assignment_declaration(self.source, value_declaration)
                        && !is_assignment_declaration(self.source, node)))
                    || (kind_of(self.source, value_declaration) != kind_of(self.source, node)
                        && is_effective_module_declaration(self.source, value_declaration))
            }
        };
        if replace {
            self.symbols.symbol_mut(symbol).value_declaration = Some(node);
        }
    }

    fn in_js_file(&self) -> bool {
        [".js", ".jsx", ".mjs", ".cjs"]
            .iter()
            .any(|extension| self.source.file_name.ends_with(extension))
    }

    /// tsc-port: getDeclarationName @6.0.3
    /// tsc-hash: d2af29f322058fe2e4f4a1064734eea28f25a726f6cbbbd5d8e19bf6d8dbd4bd
    /// tsc-span: _tsc.js:42534-42598
    ///
    /// JS-only: the BinaryExpression module.exports arm and JSDoc
    /// function-type/parameter arms land with stage 3.4 / JSDoc.
    pub fn get_declaration_name(&mut self, node: NodeId) -> Option<String> {
        if kind_of(self.source, node) == SyntaxKind::ExportAssignment {
            let is_export_equals = match &self.source.arena.node(node).data {
                NodeData::ExportAssignment(data) => data.is_export_equals.unwrap_or(false),
                _ => false,
            };
            return Some(
                if is_export_equals {
                    InternalSymbolName::EXPORT_EQUALS
                } else {
                    InternalSymbolName::DEFAULT
                }
                .to_owned(),
            );
        }
        if let Some(name) = get_name_of_declaration(self.source, node) {
            if is_ambient_module(self.source, node) {
                let module_name =
                    get_text_of_identifier_or_literal(self.source, name).unwrap_or_default();
                return Some(if is_global_scope_augmentation(self.source, node) {
                    InternalSymbolName::GLOBAL.to_owned()
                } else {
                    format!("\"{module_name}\"")
                });
            }
            if kind_of(self.source, name) == SyntaxKind::ComputedPropertyName {
                let name_expression = match &self.source.arena.node(name).data {
                    NodeData::ComputedPropertyName(data) => data.expression?,
                    _ => return None,
                };
                if is_string_or_numeric_literal_like(self.source, name_expression) {
                    return literal_text_of(self.source, name_expression)
                        .map(escape_leading_underscores);
                }
                if is_signed_numeric_literal(self.source, name_expression) {
                    let NodeData::PrefixUnaryExpression(data) =
                        &self.source.arena.node(name_expression).data
                    else {
                        return None;
                    };
                    let token = match data.operator {
                        SyntaxKind::PlusToken => "+",
                        SyntaxKind::MinusToken => "-",
                        _ => return None,
                    };
                    let operand_text = data
                        .operand
                        .and_then(|operand| literal_text_of(self.source, operand))?;
                    return Some(format!("{token}{operand_text}"));
                }
                debug_assert!(
                    false,
                    "Only computed properties with literal names have declaration names"
                );
                return None;
            }
            if kind_of(self.source, name) == SyntaxKind::PrivateIdentifier {
                let containing_class = get_containing_class(self.source, node)?;
                let class_symbol = self.node_symbol.get(&containing_class).copied()?;
                let escaped_text = match &self.source.arena.node(name).data {
                    NodeData::PrivateIdentifier(data) => data.escaped_text.clone(),
                    _ => return None,
                };
                // tsc getSymbolNameForPrivateIdentifier (_tsc.js 15905).
                let id = self.get_symbol_id(class_symbol);
                return Some(format!("__#{id}@{escaped_text}"));
            }
            if kind_of(self.source, name) == SyntaxKind::JsxNamespacedName {
                return get_escaped_text_of_jsx_namespaced_name(self.source, name);
            }
            return if is_property_name_literal(self.source, name) {
                get_escaped_text_of_identifier_or_literal(self.source, name)
            } else {
                None
            };
        }
        match kind_of(self.source, node) {
            SyntaxKind::Constructor => Some(InternalSymbolName::CONSTRUCTOR.to_owned()),
            SyntaxKind::FunctionType | SyntaxKind::CallSignature => {
                Some(InternalSymbolName::CALL.to_owned())
            }
            SyntaxKind::ConstructorType | SyntaxKind::ConstructSignature => {
                Some(InternalSymbolName::NEW.to_owned())
            }
            SyntaxKind::IndexSignature => Some(InternalSymbolName::INDEX.to_owned()),
            SyntaxKind::ExportDeclaration => Some(InternalSymbolName::EXPORT_STAR.to_owned()),
            SyntaxKind::SourceFile => Some(InternalSymbolName::EXPORT_EQUALS.to_owned()),
            // JS-only: BinaryExpression `module.exports =` resolves via
            // getAssignmentDeclarationKind (stage 3.4).
            _ => None,
        }
    }

    /// tsc getDisplayName (42599).
    fn get_display_name(&mut self, node: NodeId) -> String {
        if let Some(name) = crate::node_util::name_field_of(self.source, node) {
            return declaration_name_to_string(self.source, Some(name));
        }
        match self.get_declaration_name(node) {
            Some(name) => unescape_leading_underscores(&name).to_owned(),
            None => declaration_name_to_string(self.source, None),
        }
    }

    /// The conflict block inside declareSymbol (42621-42663): message
    /// selection between 2300/2451/2567/2528 and the relatedInformation
    /// wiring.
    fn report_duplicate(
        &mut self,
        existing: SymbolId,
        node: NodeId,
        includes: SymbolFlags,
        is_default_export: bool,
    ) {
        let existing_flags = self.symbols.symbol(existing).flags;
        let mut message: &'static DiagnosticMessage =
            if existing_flags.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE) {
                &diagnostics::Cannot_redeclare_block_scoped_variable_0
            } else {
                &diagnostics::Duplicate_identifier_0
            };
        let mut message_needs_name = true;
        if existing_flags.intersects(SymbolFlags::ENUM) || includes.intersects(SymbolFlags::ENUM) {
            message = &diagnostics::Enum_declarations_can_only_merge_with_namespace_or_other_enum_declarations;
            message_needs_name = false;
        }

        let mut multiple_default_exports = false;
        if !self.symbols.symbol(existing).declarations.is_empty() {
            let is_unnamed_default = kind_of(self.source, node) == SyntaxKind::ExportAssignment
                && !matches!(
                    &self.source.arena.node(node).data,
                    NodeData::ExportAssignment(data) if data.is_export_equals == Some(true)
                );
            if is_default_export || is_unnamed_default {
                message = &diagnostics::A_module_cannot_have_multiple_default_exports;
                message_needs_name = false;
                multiple_default_exports = true;
            }
        }

        let mut related_information: Vec<RelatedInfo> = Vec::new();
        if kind_of(self.source, node) == SyntaxKind::TypeAliasDeclaration {
            let (type_node, alias_name) = match &self.source.arena.node(node).data {
                NodeData::TypeAliasDeclaration(data) => (data.r#type, data.name),
                _ => (None, None),
            };
            if node_is_missing(self.source, type_node)
                && has_syntactic_modifier(self.source, node, ModifierFlags::EXPORT)
                && existing_flags
                    .intersects(SymbolFlags::ALIAS | SymbolFlags::TYPE | SymbolFlags::NAMESPACE)
            {
                let escaped = alias_name
                    .and_then(|name| match &self.source.arena.node(name).data {
                        NodeData::Identifier(data) => Some(data.escaped_text.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let suggestion = format!(
                    "export type {{ {} }}",
                    unescape_leading_underscores(&escaped)
                );
                related_information.push(self.related_for_node(
                    node,
                    &diagnostics::Did_you_mean_0,
                    &[&suggestion],
                ));
            }
        }

        let declaration_name_node = get_name_of_declaration(self.source, node).unwrap_or(node);
        let prior_declarations = self.symbols.symbol(existing).declarations.clone();
        for (index, &declaration) in prior_declarations.iter().enumerate() {
            let decl = get_name_of_declaration(self.source, declaration).unwrap_or(declaration);
            let mut diag = if message_needs_name {
                let display = self.get_display_name(declaration);
                self.diagnostic_for_node(decl, message, &[&display])
            } else {
                self.diagnostic_for_node(decl, message, &[])
            };
            if multiple_default_exports {
                let related_message: &'static DiagnosticMessage = if index == 0 {
                    &diagnostics::Another_export_default_is_here
                } else {
                    &diagnostics::and_here
                };
                diag.related.push(self.related_for_node(
                    declaration_name_node,
                    related_message,
                    &[],
                ));
            }
            self.bind_diagnostics.push(diag);
            if multiple_default_exports {
                related_information.push(self.related_for_node(
                    decl,
                    &diagnostics::The_first_export_default_is_here,
                    &[],
                ));
            }
        }
        let mut diag = if message_needs_name {
            let display = self.get_display_name(node);
            self.diagnostic_for_node(declaration_name_node, message, &[&display])
        } else {
            self.diagnostic_for_node(declaration_name_node, message, &[])
        };
        diag.related.extend(related_information);
        self.bind_diagnostics.push(diag);
    }

    fn to_utf16(&self, byte: usize) -> u32 {
        self.source
            .line_map
            .byte_to_utf16
            .get(byte)
            .copied()
            .unwrap_or(byte as u32)
    }

    /// tsc createDiagnosticForNode(InSourceFile): span from
    /// getErrorSpanForNode, positions in UTF-16.
    pub fn diagnostic_for_node(
        &self,
        node: NodeId,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> Diagnostic {
        let (start, end) = get_error_span_for_node(self.source, node);
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let start_utf16 = self.to_utf16(start);
        let end_utf16 = self.to_utf16(end);
        Diagnostic::new(
            Some(self.source.file_name.clone()),
            Some(start_utf16),
            Some(end_utf16.saturating_sub(start_utf16)),
            MessageChain::new(message, &args),
        )
    }

    fn related_for_node(
        &self,
        node: NodeId,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> RelatedInfo {
        let diag = self.diagnostic_for_node(node, message, args);
        RelatedInfo {
            file_name: diag.file_name,
            start: diag.start,
            length: diag.length,
            message: diag.message,
        }
    }
}

/// tsc isAssignmentDeclaration (_tsc.js 14964).
pub fn is_assignment_declaration(source: &SourceFile, id: NodeId) -> bool {
    matches!(
        kind_of(source, id),
        SyntaxKind::BinaryExpression
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::Identifier
            | SyntaxKind::CallExpression
    )
}

/// tsc isEffectiveModuleDeclaration (_tsc.js 13722).
pub fn is_effective_module_declaration(source: &SourceFile, id: NodeId) -> bool {
    matches!(
        kind_of(source, id),
        SyntaxKind::ModuleDeclaration | SyntaxKind::Identifier
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsrs2_syntax::{parse_source_file, ParseOptions};

    fn parse(text: &str) -> SourceFile {
        parse_source_file("main.ts", text, ParseOptions::default(), None)
    }

    fn statements(source: &SourceFile) -> Vec<NodeId> {
        let data = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("root");
        let statements = data.statements.expect("statements");
        source.arena.node_array(statements).nodes.clone()
    }

    /// The declared node + includes/excludes a statement would get from
    /// bindWorker (test-only shim until stage 3.4).
    fn masks_for(source: &SourceFile, statement: NodeId) -> (NodeId, SymbolFlags, SymbolFlags) {
        match &source.arena.node(statement).data {
            NodeData::FunctionDeclaration(_) => (
                statement,
                SymbolFlags::FUNCTION,
                SymbolFlags::FUNCTION_EXCLUDES,
            ),
            NodeData::ClassDeclaration(_) => {
                (statement, SymbolFlags::CLASS, SymbolFlags::CLASS_EXCLUDES)
            }
            NodeData::InterfaceDeclaration(_) => (
                statement,
                SymbolFlags::INTERFACE,
                SymbolFlags::INTERFACE_EXCLUDES,
            ),
            NodeData::EnumDeclaration(_) => (
                statement,
                SymbolFlags::REGULAR_ENUM,
                SymbolFlags::REGULAR_ENUM_EXCLUDES,
            ),
            NodeData::ModuleDeclaration(_) => (
                statement,
                SymbolFlags::VALUE_MODULE,
                SymbolFlags::VALUE_MODULE_EXCLUDES,
            ),
            NodeData::TypeAliasDeclaration(_) => (
                statement,
                SymbolFlags::TYPE_ALIAS,
                SymbolFlags::TYPE_ALIAS_EXCLUDES,
            ),
            NodeData::ExportAssignment(_) => (statement, SymbolFlags::PROPERTY, SymbolFlags::ALL),
            NodeData::VariableStatement(data) => {
                let list = data.declaration_list.expect("list");
                let declarations = match &source.arena.node(list).data {
                    NodeData::VariableDeclarationList(data) => {
                        data.declarations.expect("declarations")
                    }
                    _ => panic!("not a declaration list"),
                };
                let declaration = source.arena.node_array(declarations).nodes[0];
                let list_flags = crate::node_util::node_flags(source, list);
                if list_flags.intersects(tsrs2_types::NodeFlags::BLOCK_SCOPED) {
                    (
                        declaration,
                        SymbolFlags::BLOCK_SCOPED_VARIABLE,
                        SymbolFlags::BLOCK_SCOPED_VARIABLE_EXCLUDES,
                    )
                } else {
                    (
                        declaration,
                        SymbolFlags::FUNCTION_SCOPED_VARIABLE,
                        SymbolFlags::FUNCTION_SCOPED_VARIABLE_EXCLUDES,
                    )
                }
            }
            other => panic!("unhandled test statement kind: {other:?}"),
        }
    }

    /// Declare every top-level statement into one locals table (a mini
    /// stand-in for bindWorker's routing), returning the symbols.
    fn declare_all(
        binder: &mut Binder<'_>,
        table: TableRef,
        parent: Option<SymbolId>,
    ) -> Vec<SymbolId> {
        let statements = statements(binder.source);
        statements
            .iter()
            .map(|&statement| {
                let (node, includes, excludes) = masks_for(binder.source, statement);
                binder.declare_symbol(table, parent, node, includes, excludes, false, false)
            })
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
    fn function_overloads_merge() {
        let source = parse("function f(a: string): void;\nfunction f(a: number): void {}\n");
        let options: &'static tsrs2_types::CompilerOptions =
            Box::leak(Box::new(tsrs2_types::CompilerOptions::default()));
        let mut binder = Binder::new(&source, options);
        let symbols = declare_all(&mut binder, TableRef::Locals(source.root), None);
        assert_eq!(symbols[0], symbols[1]);
        let symbol = binder.symbols.symbol(symbols[0]);
        assert!(symbol.flags.contains(SymbolFlags::FUNCTION));
        assert_eq!(symbol.declarations.len(), 2);
        // setValueDeclaration: the FIRST value declaration wins.
        assert_eq!(symbol.value_declaration, Some(symbol.declarations[0]));
        assert!(binder.bind_diagnostics.is_empty());
        // Function is NOT in SymbolFlags.Classifiable.
        assert!(!binder.classifiable_names.contains("f"));
    }

    #[test]
    fn namespace_merges_into_function() {
        let source = parse("function f() {}\nnamespace f { export const x = 1; }\n");
        let options: &'static tsrs2_types::CompilerOptions =
            Box::leak(Box::new(tsrs2_types::CompilerOptions::default()));
        let mut binder = Binder::new(&source, options);
        let symbols = declare_all(&mut binder, TableRef::Locals(source.root), None);
        assert_eq!(symbols[0], symbols[1]);
        assert!(binder.bind_diagnostics.is_empty());
    }

    #[test]
    fn interface_merges_into_class() {
        let source = parse("class D {}\ninterface D {}\n");
        let options: &'static tsrs2_types::CompilerOptions =
            Box::leak(Box::new(tsrs2_types::CompilerOptions::default()));
        let mut binder = Binder::new(&source, options);
        let symbols = declare_all(&mut binder, TableRef::Locals(source.root), None);
        assert_eq!(symbols[0], symbols[1]);
        let symbol = binder.symbols.symbol(symbols[0]);
        assert!(symbol
            .flags
            .contains(SymbolFlags::CLASS | SymbolFlags::INTERFACE));
        assert!(binder.bind_diagnostics.is_empty());
    }

    #[test]
    fn block_scoped_redeclaration_reports_2451_and_detaches_fresh_symbol() {
        // Pins from tsc sf.bindDiagnostics on "let x = 1;\nlet x = 2;".
        let source = parse("let x = 1;\nlet x = 2;");
        let options: &'static tsrs2_types::CompilerOptions =
            Box::leak(Box::new(tsrs2_types::CompilerOptions::default()));
        let mut binder = Binder::new(&source, options);
        let symbols = declare_all(&mut binder, TableRef::Locals(source.root), None);
        assert_eq!(diag_pins(&binder), [(2451, 4, 1), (2451, 15, 1)]);
        // The fresh conflict symbol is DETACHED: the table keeps the
        // original, whose declarations stay [decl1].
        assert_ne!(symbols[0], symbols[1]);
        let table = binder.locals.get(&source.root).expect("locals");
        assert_eq!(table.get("x"), Some(&symbols[0]));
        assert_eq!(binder.symbols.symbol(symbols[0]).declarations.len(), 1);
    }

    #[test]
    fn triple_let_conflicts_against_the_original_symbol() {
        // Pins from tsc: "let y = 1;\nlet y = 2;\nlet y = 3;".
        let source = parse("let y = 1;\nlet y = 2;\nlet y = 3;");
        let options: &'static tsrs2_types::CompilerOptions =
            Box::leak(Box::new(tsrs2_types::CompilerOptions::default()));
        let mut binder = Binder::new(&source, options);
        declare_all(&mut binder, TableRef::Locals(source.root), None);
        assert_eq!(
            diag_pins(&binder),
            [(2451, 4, 1), (2451, 15, 1), (2451, 4, 1), (2451, 26, 1)]
        );
    }

    #[test]
    fn var_then_function_reports_duplicate_identifier() {
        // tsc pins {(2300,21,1),(2300,4,1)} — tsc's order differs
        // because bindEachFunctionsFirst binds the function BEFORE the
        // var (stage 3.4); source-order declaration flips it.
        let source = parse("var f: any;\nfunction f() {}");
        let options: &'static tsrs2_types::CompilerOptions =
            Box::leak(Box::new(tsrs2_types::CompilerOptions::default()));
        let mut binder = Binder::new(&source, options);
        declare_all(&mut binder, TableRef::Locals(source.root), None);
        assert_eq!(diag_pins(&binder), [(2300, 4, 1), (2300, 21, 1)]);
    }

    #[test]
    fn enum_cannot_merge_with_class_reports_2567() {
        // Pins from tsc: "class C {}\nenum C {}".
        let source = parse("class C {}\nenum C {}");
        let options: &'static tsrs2_types::CompilerOptions =
            Box::leak(Box::new(tsrs2_types::CompilerOptions::default()));
        let mut binder = Binder::new(&source, options);
        declare_all(&mut binder, TableRef::Locals(source.root), None);
        assert_eq!(diag_pins(&binder), [(2567, 6, 1), (2567, 16, 1)]);
        // messageNeedsName = false: the 2567 text carries no name.
        assert!(!binder.bind_diagnostics[0].message_text().contains('C'));
    }

    #[test]
    fn multiple_default_export_classes_report_2528_with_relateds() {
        // Pins from tsc: "export default class C {}\nexport default class D {}"
        //   2528@(21,1) related 2753@(47,1); 2528@(47,1) related 2752@(21,1).
        let source = parse("export default class C {}\nexport default class D {}");
        let options: &'static tsrs2_types::CompilerOptions =
            Box::leak(Box::new(tsrs2_types::CompilerOptions::default()));
        let mut binder = Binder::new(&source, options);
        let container = binder
            .symbols
            .alloc(SymbolFlags::NONE, "container".to_owned());
        declare_all(&mut binder, TableRef::Exports(container), Some(container));
        assert_eq!(diag_pins(&binder), [(2528, 21, 1), (2528, 47, 1)]);
        let first = &binder.bind_diagnostics[0];
        assert_eq!(
            (first.related[0].message.code, first.related[0].start),
            (2753, Some(47))
        );
        let second = &binder.bind_diagnostics[1];
        assert_eq!(
            (second.related[0].message.code, second.related[0].start),
            (2752, Some(21))
        );
        // Both bound under the "default" export name.
        assert!(binder
            .symbols
            .symbol(container)
            .exports
            .contains_key("default"));
    }

    #[test]
    fn multiple_export_assignments_report_2528_full_statement_spans() {
        // Pins from tsc: "export default 1;\nexport default 2;"
        //   2528@(0,17) related 2753@(18,17); 2528@(18,17) related 2752@(0,17).
        let source = parse("export default 1;\nexport default 2;");
        let options: &'static tsrs2_types::CompilerOptions =
            Box::leak(Box::new(tsrs2_types::CompilerOptions::default()));
        let mut binder = Binder::new(&source, options);
        let container = binder
            .symbols
            .alloc(SymbolFlags::NONE, "container".to_owned());
        declare_all(&mut binder, TableRef::Exports(container), Some(container));
        assert_eq!(diag_pins(&binder), [(2528, 0, 17), (2528, 18, 17)]);
        let first = &binder.bind_diagnostics[0];
        assert_eq!(
            (
                first.related[0].message.code,
                first.related[0].start,
                first.related[0].length
            ),
            (2753, Some(18), Some(17))
        );
    }

    #[test]
    fn escaped_names_key_the_table() {
        let source = parse("let __proto__ = 1;");
        let options: &'static tsrs2_types::CompilerOptions =
            Box::leak(Box::new(tsrs2_types::CompilerOptions::default()));
        let mut binder = Binder::new(&source, options);
        declare_all(&mut binder, TableRef::Locals(source.root), None);
        let table = binder.locals.get(&source.root).expect("locals");
        assert!(table.contains_key("___proto__"));
        assert!(!table.contains_key("__proto__"));
    }
}
