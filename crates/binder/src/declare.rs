//! m2-binder-steps.md stage 3.2: declareSymbol — the merge engine —
//! with its supporting pieces (addDeclarationToSymbol,
//! getDeclarationName, the duplicate-declaration report family).

use std::collections::HashMap;

use crate::node_util::{
    declaration_name_to_string, get_containing_class, get_error_span_for_node,
    get_escaped_text_of_identifier_or_literal, get_escaped_text_of_jsx_namespaced_name,
    get_name_of_declaration, get_text_of_identifier_or_literal, has_dynamic_name,
    has_syntactic_modifier, is_ambient_module, is_global_scope_augmentation,
    is_jsdoc_construct_signature, is_property_name_literal, is_signed_numeric_literal,
    is_string_or_numeric_literal_like, kind_of, literal_text_of, module_export_name_is_default,
    node_is_missing, parent_of,
};
use crate::symbols::{
    escape_leading_underscores, relocate_symbol_table_values, unescape_leading_underscores,
    InternalSymbolName, SymbolArena, SymbolId, SymbolIdentityRelocation, SymbolTable,
};
use indexmap::IndexSet;
use tsc_diagnostics::{
    gen as diagnostics, Diagnostic, DiagnosticList, DiagnosticMessage, MessageChain, RelatedInfo,
};
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{
    IdentityAllocationPolicy, IdentityDomain, IdentityError, IdentityLease, IdentityRange,
    IdentitySpace, ModifierFlags, SymbolFlags, TRANSIENT_SYMBOL_BIT,
};

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
pub struct BinderWorker<'a> {
    pub source: &'a SourceFile,
    pub options: &'a tsc_types::CompilerOptions,
    /// tsc languageVersion = getEmitScriptTarget(options).
    pub language_version: i32,
    /// tsc file.commonJsModuleIndicator.
    pub common_js_module_indicator: Option<NodeId>,
    pub symbols: SymbolArena,
    /// tsc node.symbol (set by addDeclarationToSymbol).
    pub node_symbol: HashMap<NodeId, SymbolId>,
    /// tsc node.localSymbol (set by declareModuleMember).
    pub node_local_symbol: HashMap<NodeId, SymbolId>,
    /// tsc container.locals, keyed by the scope-owning node.
    pub locals: HashMap<NodeId, SymbolTable>,
    /// tsc SourceFile.jsGlobalAugmentations: namespaces introduced by
    /// top-level JavaScript property assignments before checker merge.
    pub js_global_augmentations: SymbolTable,
    pub bind_diagnostics: DiagnosticList,
    /// tsc file.classifiableNames (insertion-ordered Set).
    pub classifiable_names: IndexSet<String>,
    /// tsc getSymbolId's lazily-assigned global symbol ids; the counter
    /// is program-wide in tsc, so it is seedable for multi-file binds.
    assigned_symbol_ids: HashMap<SymbolId, u32>,
    private_name_serial_base: u32,
    next_symbol_id: u32,
    private_name_serial_lease: Option<IdentityLease>,

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
    /// tsc delayedTypeAliases: JSDoc typedef/callback/enum tags bind
    /// their type expression and scope declaration after the ordinary
    /// source walk has established every host container.
    pub delayed_type_aliases: Vec<NodeId>,
    /// tsc jsDocImports: import clauses bind after the ordinary JSDoc
    /// walk, in the enclosing scope of the last attached JSDoc host.
    pub js_doc_imports: Vec<NodeId>,
}

/// The completed, checker-consumed half of one bind operation.
///
/// `BinderWorker` is the temporary worker: it borrows the parsed source and
/// compiler options while it walks containers and builds this record. Once
/// publication succeeds, callers retain only `BindData`; no walk cursor,
/// borrowed input, or in-flight target is carried into a Program snapshot.
#[derive(Clone, Debug)]
pub struct BindData {
    pub language_version: i32,
    pub common_js_module_indicator: Option<NodeId>,
    pub symbols: SymbolArena,
    pub node_symbol: HashMap<NodeId, SymbolId>,
    pub node_local_symbol: HashMap<NodeId, SymbolId>,
    pub locals: HashMap<NodeId, SymbolTable>,
    pub js_global_augmentations: SymbolTable,
    pub bind_diagnostics: DiagnosticList,
    pub classifiable_names: IndexSet<String>,
    pub assigned_symbol_ids: HashMap<SymbolId, u32>,
    pub private_name_serial_base: u32,
    pub next_symbol_id: u32,
    pub private_name_serial_lease: Option<IdentityLease>,
    pub next_container: HashMap<NodeId, NodeId>,
    pub node_flags_mut: Vec<i32>,
    pub pattern_ambient_modules: Vec<(String, String, SymbolId)>,
    pub flow: crate::flow::FlowArena,
    pub unreachable_flow: crate::flow::FlowId,
    pub node_flow: HashMap<NodeId, crate::flow::FlowId>,
    pub node_end_flow: HashMap<NodeId, crate::flow::FlowId>,
    pub node_return_flow: HashMap<NodeId, crate::flow::FlowId>,
    pub node_flow_when_true: HashMap<NodeId, crate::flow::FlowId>,
    pub node_flow_when_false: HashMap<NodeId, crate::flow::FlowId>,
    pub possibly_exhaustive: HashMap<NodeId, bool>,
    pub node_fallthrough_flow: HashMap<NodeId, crate::flow::FlowId>,
    pub emit_flags: i32,
}

/// Compatibility name retained for existing binder/checker callers. New
/// publication code should name the worker explicitly as `BinderWorker` and
/// retain only `BindData` in an owned document.
pub type Binder<'a> = BinderWorker<'a>;

impl BindData {
    /// Clone only the completed result for a compatibility adapter. The
    /// production snapshot path uses `Binder::into_bind_data` to move these
    /// fields without retaining the worker or borrowing its source.
    pub fn from_binder(binder: &BinderWorker<'_>) -> Self {
        Self {
            language_version: binder.language_version,
            common_js_module_indicator: binder.common_js_module_indicator,
            symbols: binder.symbols.clone(),
            node_symbol: binder.node_symbol.clone(),
            node_local_symbol: binder.node_local_symbol.clone(),
            locals: binder.locals.clone(),
            js_global_augmentations: binder.js_global_augmentations.clone(),
            bind_diagnostics: binder.bind_diagnostics.clone(),
            classifiable_names: binder.classifiable_names.clone(),
            assigned_symbol_ids: binder.assigned_symbol_ids.clone(),
            private_name_serial_base: binder.private_name_serial_base,
            next_symbol_id: binder.next_symbol_id,
            private_name_serial_lease: binder.private_name_serial_lease.clone(),
            next_container: binder.next_container.clone(),
            node_flags_mut: binder.node_flags_mut.clone(),
            pattern_ambient_modules: binder.pattern_ambient_modules.clone(),
            flow: binder.flow.clone(),
            unreachable_flow: binder.unreachable_flow,
            node_flow: binder.node_flow.clone(),
            node_end_flow: binder.node_end_flow.clone(),
            node_return_flow: binder.node_return_flow.clone(),
            node_flow_when_true: binder.node_flow_when_true.clone(),
            node_flow_when_false: binder.node_flow_when_false.clone(),
            possibly_exhaustive: binder.possibly_exhaustive.clone(),
            node_fallthrough_flow: binder.node_fallthrough_flow.clone(),
            emit_flags: binder.emit_flags,
        }
    }

    pub fn symbol_identity_lease(&self) -> Option<&IdentityLease> {
        self.symbols.identity_lease()
    }

    pub fn private_name_serial_lease(&self) -> Option<&IdentityLease> {
        self.private_name_serial_lease.as_ref()
    }

    /// tsrs-native: verifies that every persistent identity carried by the published bind
    /// belongs to one document domain. Publication paths use this before a
    /// `BindData` enters an owned Program store; a partial or cross-domain
    /// record must fail closed rather than becoming a cacheable variant.
    pub fn identity_owned_by(&self, domain: &IdentityDomain) -> bool {
        self.symbol_identity_lease()
            .is_some_and(|lease| lease.belongs_to(domain))
            && self
                .private_name_serial_lease()
                .is_some_and(|lease| lease.belongs_to(domain))
    }

    pub fn next_symbol_id(&self) -> u32 {
        self.next_symbol_id
    }

    pub fn flags_of(&self, node: NodeId, node_base: u32) -> tsc_types::NodeFlags {
        let index = (node.0 - node_base) as usize;
        tsc_types::NodeFlags::from_bits(self.node_flags_mut[index])
    }
}

impl<'a> BinderWorker<'a> {
    pub fn new(source: &'a SourceFile, options: &'a tsc_types::CompilerOptions) -> Self {
        Self::with_symbol_id_seed(source, options, 1)
    }

    pub fn with_symbol_id_seed(
        source: &'a SourceFile,
        options: &'a tsc_types::CompilerOptions,
        next_symbol_id: u32,
    ) -> Self {
        Self::with_bases(source, options, next_symbol_id, 0)
    }

    /// Program bind (M4 5.0): file N's symbols allocate from
    /// `symbol_base` so SymbolIds are program-unique, mirroring the
    /// parse-side NodeId bases (ParseOptions::node_id_base).
    pub fn with_bases(
        source: &'a SourceFile,
        options: &'a tsc_types::CompilerOptions,
        next_symbol_id: u32,
        symbol_base: u32,
    ) -> Self {
        let mut flow = crate::flow::FlowArena::default();
        // tsc createBinder: unreachableFlow is allocated once up front.
        let unreachable_flow = flow.create_flow_node(
            tsc_types::FlowFlags::UNREACHABLE,
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
            js_global_augmentations: SymbolTable::default(),
            bind_diagnostics: Vec::new(),
            classifiable_names: IndexSet::new(),
            assigned_symbol_ids: HashMap::new(),
            private_name_serial_base: next_symbol_id,
            next_symbol_id,
            private_name_serial_lease: None,
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
            delayed_type_aliases: Vec::new(),
            js_doc_imports: Vec::new(),
        }
    }

    /// The binder's mutable view of tsc node.flags. `node_flags_mut` is
    /// indexed by the file-local node index (program binds parse each
    /// file with a NodeId base — see ParseOptions::node_id_base).
    pub fn flags_of(&self, node: NodeId) -> tsc_types::NodeFlags {
        let index = (node.0 - self.source.arena.node_base()) as usize;
        tsc_types::NodeFlags::from_bits(self.node_flags_mut[index])
    }

    pub fn set_flags_of(&mut self, node: NodeId, flags: tsc_types::NodeFlags) {
        let index = (node.0 - self.source.arena.node_base()) as usize;
        self.node_flags_mut[index] = flags.bits();
    }

    pub fn next_symbol_id(&self) -> u32 {
        self.next_symbol_id
    }

    pub fn symbol_identity_lease(&self) -> Option<&IdentityLease> {
        self.symbols.identity_lease()
    }

    pub fn private_name_serial_lease(&self) -> Option<&IdentityLease> {
        self.private_name_serial_lease.as_ref()
    }

    pub fn identity_owned_by(&self, domain: &IdentityDomain) -> bool {
        self.source.identity_owned_by(domain)
            && self
                .symbol_identity_lease()
                .is_some_and(|lease| lease.belongs_to(domain))
            && self
                .private_name_serial_lease()
                .is_some_and(|lease| lease.belongs_to(domain))
    }

    /// Bind one source and publish completed symbol/private-name identities.
    /// Ephemeral domains construct directly at a sealed tail; reclaiming
    /// domains bind locally and relocate only after exact counts are known.
    pub fn bind_in_identity_domain(
        source: &'a SourceFile,
        options: &'a tsc_types::CompilerOptions,
        domain: &IdentityDomain,
    ) -> Result<Self, IdentityError> {
        if !source.identity_owned_by(domain) {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Node,
                detail: "bound source does not belong to the requested identity domain",
            });
        }
        match domain.policy() {
            IdentityAllocationPolicy::EphemeralBump => {
                let reservation = domain.reserve_provisional(&[
                    IdentitySpace::Symbol,
                    IdentitySpace::PrivateNameSerial,
                ])?;
                let mut binder = Self::with_bases(
                    source,
                    options,
                    reservation.base(IdentitySpace::PrivateNameSerial)?,
                    reservation.base(IdentitySpace::Symbol)?,
                );
                binder.bind_source_file();
                let (symbol_count, serial_count) = binder.identity_counts()?;
                let leases = reservation.seal(&[
                    (IdentitySpace::Symbol, symbol_count),
                    (IdentitySpace::PrivateNameSerial, serial_count),
                ])?;
                binder.attach_identity_leases(domain, leases)?;
                Ok(binder)
            }
            IdentityAllocationPolicy::Reclaiming => {
                let mut binder = Self::with_bases(source, options, 1, 0);
                binder.bind_source_file();
                binder.relocate_into_identity_domain(domain)?;
                Ok(binder)
            }
        }
    }

    pub fn relocate_into_identity_domain(
        &mut self,
        domain: &IdentityDomain,
    ) -> Result<(), IdentityError> {
        if !self.source.identity_owned_by(domain) {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Node,
                detail: "bound source and bind identities would use different domains",
            });
        }
        let (symbol_count, serial_count) = self.identity_counts()?;
        let leases = domain.lease_batch(&[
            (IdentitySpace::Symbol, symbol_count),
            (IdentitySpace::PrivateNameSerial, serial_count),
        ])?;
        let (symbol_lease, serial_lease) = bind_leases(leases)?;
        self.apply_identity_relocation(domain, symbol_lease, serial_lease)
    }

    fn identity_counts(&self) -> Result<(u32, u32), IdentityError> {
        let symbol_count =
            u32::try_from(self.symbols.len()).map_err(|_| IdentityError::Exhausted {
                space: IdentitySpace::Symbol,
                requested: u32::MAX,
                limit: TRANSIENT_SYMBOL_BIT,
            })?;
        let serial_count = self
            .next_symbol_id
            .checked_sub(self.private_name_serial_base)
            .ok_or(IdentityError::InvalidLease {
                space: IdentitySpace::PrivateNameSerial,
                detail: "private-name serial counter precedes its base",
            })?;
        Ok((symbol_count, serial_count))
    }

    fn attach_identity_leases(
        &mut self,
        domain: &IdentityDomain,
        leases: Vec<IdentityLease>,
    ) -> Result<(), IdentityError> {
        let (symbol_lease, serial_lease) = bind_leases(leases)?;
        if !domain.owns(&symbol_lease)
            || !domain.owns(&serial_lease)
            || !symbol_lease.same_domain(&serial_lease)
        {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Symbol,
                detail: "bind leases do not share the requested domain",
            });
        }
        self.symbols.attach_identity_lease(symbol_lease)?;
        validate_serial_lease(
            &serial_lease,
            self.private_name_serial_base,
            self.next_symbol_id,
            true,
        )?;
        self.private_name_serial_lease = Some(serial_lease);
        Ok(())
    }

    fn apply_identity_relocation(
        &mut self,
        domain: &IdentityDomain,
        symbol_lease: IdentityLease,
        serial_lease: IdentityLease,
    ) -> Result<(), IdentityError> {
        if !domain.owns(&symbol_lease)
            || !domain.owns(&serial_lease)
            || !symbol_lease.same_domain(&serial_lease)
        {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Symbol,
                detail: "bind relocation leases do not share the requested domain",
            });
        }
        let symbol_relocation = self.symbols.identity_relocation(&symbol_lease)?;
        validate_serial_lease(
            &serial_lease,
            self.private_name_serial_base,
            self.next_symbol_id,
            false,
        )?;
        let serial_relocation = PrivateNameSerialRelocation {
            old: IdentityRange::new(self.private_name_serial_base, self.next_symbol_id),
            new: serial_lease.range(),
        };
        self.apply_declared_identity_relocation(
            symbol_relocation,
            serial_relocation,
            symbol_lease,
            serial_lease,
        )
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
        self.next_symbol_id = self
            .next_symbol_id
            .checked_add(1)
            .expect("private-name serial identity space exhausted");
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
                    .intersects(tsc_types::NodeFlags::AMBIENT);
                let value_ambient = crate::node_util::node_flags(self.source, value_declaration)
                    .intersects(tsc_types::NodeFlags::AMBIENT);
                let in_js = self.is_in_js_file();
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

    /// tsc-port: getDeclarationName @6.0.3
    /// tsc-hash: d2af29f322058fe2e4f4a1064734eea28f25a726f6cbbbd5d8e19bf6d8dbd4bd
    /// tsc-span: _tsc.js:42534-42598
    ///
    /// JS-only: the BinaryExpression module.exports arm.
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
        if let Some(name) = crate::assignment::get_assignment_declaration_name(self.source, node) {
            return get_escaped_text_of_identifier_or_literal(self.source, name);
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
            SyntaxKind::FunctionType | SyntaxKind::CallSignature | SyntaxKind::JSDocSignature => {
                Some(InternalSymbolName::CALL.to_owned())
            }
            SyntaxKind::JSDocFunctionType => Some(
                if is_jsdoc_construct_signature(self.source, node) {
                    InternalSymbolName::NEW
                } else {
                    InternalSymbolName::CALL
                }
                .to_owned(),
            ),
            SyntaxKind::ConstructorType | SyntaxKind::ConstructSignature => {
                Some(InternalSymbolName::NEW.to_owned())
            }
            SyntaxKind::Parameter => {
                let parent = parent_of(self.source, node)?;
                let NodeData::JSDocFunctionType(data) = &self.source.arena.node(parent).data else {
                    return None;
                };
                let index = data.parameters.and_then(|parameters| {
                    self.source
                        .arena
                        .node_array(parameters)
                        .nodes
                        .iter()
                        .position(|&parameter| parameter == node)
                })?;
                Some(format!("arg{index}"))
            }
            SyntaxKind::IndexSignature => Some(InternalSymbolName::INDEX.to_owned()),
            SyntaxKind::ExportDeclaration => Some(InternalSymbolName::EXPORT_STAR.to_owned()),
            SyntaxKind::SourceFile => Some(InternalSymbolName::EXPORT_EQUALS.to_owned()),
            SyntaxKind::BinaryExpression
                if crate::assignment::get_assignment_declaration_kind(self.source, node)
                    == crate::assignment::AssignmentDeclarationKind::ModuleExports =>
            {
                Some(InternalSymbolName::EXPORT_EQUALS.to_owned())
            }
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
            .positions()
            .byte_to_utf16(byte as u32)
            .expect("binder diagnostic offsets are UTF-8 scalar boundaries")
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

#[derive(Clone, Copy, Debug)]
struct PrivateNameSerialRelocation {
    old: IdentityRange,
    new: IdentityRange,
}

impl PrivateNameSerialRelocation {
    fn serial(&self, value: &mut u32) -> Result<(), IdentityError> {
        if self.old.len() != self.new.len() {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::PrivateNameSerial,
                detail: "private-name relocation ranges have different lengths",
            });
        }
        let offset = value
            .checked_sub(self.old.start())
            .filter(|offset| *offset < self.old.len())
            .ok_or(IdentityError::InvalidLease {
                space: IdentitySpace::PrivateNameSerial,
                detail: "private-name serial is outside its source lease",
            })?;
        *value = self
            .new
            .start()
            .checked_add(offset)
            .ok_or(IdentityError::InvalidLease {
                space: IdentitySpace::PrivateNameSerial,
                detail: "private-name serial relocation overflowed",
            })?;
        Ok(())
    }

    fn name(&self, value: &mut String) -> Result<bool, IdentityError> {
        let Some(rest) = value.strip_prefix("__#") else {
            return Ok(false);
        };
        let Some(at) = rest.find('@') else {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::PrivateNameSerial,
                detail: "mangled private name has no @ delimiter",
            });
        };
        let digits = &rest[..at];
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::PrivateNameSerial,
                detail: "mangled private name has an invalid serial",
            });
        }
        let mut serial = digits
            .parse::<u32>()
            .map_err(|_| IdentityError::InvalidLease {
                space: IdentitySpace::PrivateNameSerial,
                detail: "mangled private-name serial exceeds u32",
            })?;
        self.serial(&mut serial)?;
        value.replace_range(3..3 + digits.len(), &serial.to_string());
        Ok(true)
    }
}

impl BinderWorker<'_> {
    /// Declared completeness boundary for published bind identity. The
    /// exhaustive destructure makes every new BinderWorker field a compile-time
    /// relocation review item.
    fn apply_declared_identity_relocation(
        &mut self,
        symbol_relocation: SymbolIdentityRelocation,
        serial_relocation: PrivateNameSerialRelocation,
        symbol_lease: IdentityLease,
        serial_lease: IdentityLease,
    ) -> Result<(), IdentityError> {
        let Self {
            source: _,
            options: _,
            language_version: _,
            common_js_module_indicator: _,
            symbols,
            node_symbol,
            node_local_symbol,
            locals,
            js_global_augmentations,
            bind_diagnostics: _,
            classifiable_names,
            assigned_symbol_ids,
            private_name_serial_base,
            next_symbol_id,
            private_name_serial_lease,
            container: _,
            this_parent_container: _,
            block_scope_container: _,
            last_container: _,
            next_container: _,
            node_flags_mut: _,
            pattern_ambient_modules,
            flow: _,
            unreachable_flow: _,
            current_flow: _,
            current_break_target: _,
            current_continue_target: _,
            current_return_target: _,
            current_true_target: _,
            current_false_target: _,
            current_exception_target: _,
            pre_switch_case_flow: _,
            node_flow: _,
            node_end_flow: _,
            node_return_flow: _,
            node_flow_when_true: _,
            node_flow_when_false: _,
            possibly_exhaustive: _,
            node_fallthrough_flow: _,
            active_label_list: _,
            in_strict_mode: _,
            seen_this_keyword: _,
            in_assignment_pattern: _,
            has_explicit_return: _,
            in_return_position: _,
            has_flow_effects: _,
            emit_flags: _,
            delayed_type_aliases: _,
            js_doc_imports: _,
        } = self;

        if private_name_serial_lease.is_some() {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::PrivateNameSerial,
                detail: "binder is already identity-owned",
            });
        }

        symbols.apply_identity_relocation(symbol_relocation, symbol_lease)?;
        for symbol in symbols.symbols_mut() {
            serial_relocation.name(&mut symbol.escaped_name)?;
            relocate_private_table_keys(&mut symbol.members, &serial_relocation)?;
            relocate_private_table_keys(&mut symbol.exports, &serial_relocation)?;
            relocate_private_table_keys(&mut symbol.global_exports, &serial_relocation)?;
        }
        let mut node_symbol_keys = node_symbol.keys().copied().collect::<Vec<_>>();
        node_symbol_keys.sort_unstable();
        for node in node_symbol_keys {
            symbol_relocation.symbol(
                node_symbol
                    .get_mut(&node)
                    .expect("collected node-symbol key must remain present"),
            )?;
        }
        let mut node_local_symbol_keys = node_local_symbol.keys().copied().collect::<Vec<_>>();
        node_local_symbol_keys.sort_unstable();
        for node in node_local_symbol_keys {
            symbol_relocation.symbol(
                node_local_symbol
                    .get_mut(&node)
                    .expect("collected local-symbol key must remain present"),
            )?;
        }
        let mut local_keys = locals.keys().copied().collect::<Vec<_>>();
        local_keys.sort_unstable();
        for node in local_keys {
            relocate_symbol_table(
                locals
                    .get_mut(&node)
                    .expect("collected locals key must remain present"),
                &symbol_relocation,
                &serial_relocation,
            )?;
        }
        relocate_symbol_table(
            js_global_augmentations,
            &symbol_relocation,
            &serial_relocation,
        )?;

        let mut old_assigned = std::mem::take(assigned_symbol_ids)
            .into_iter()
            .collect::<Vec<_>>();
        old_assigned.sort_unstable_by_key(|(symbol, _)| *symbol);
        assigned_symbol_ids.reserve(old_assigned.len());
        for (mut symbol, mut serial) in old_assigned {
            symbol_relocation.symbol(&mut symbol)?;
            serial_relocation.serial(&mut serial)?;
            if assigned_symbol_ids.insert(symbol, serial).is_some() {
                return Err(IdentityError::InvalidLease {
                    space: IdentitySpace::Symbol,
                    detail: "symbol relocation duplicated an assigned-serial key",
                });
            }
        }

        if classifiable_names
            .iter()
            .any(|name| name.starts_with("__#"))
        {
            let old_names = std::mem::take(classifiable_names);
            classifiable_names.reserve(old_names.len());
            for mut name in old_names {
                serial_relocation.name(&mut name)?;
                if !classifiable_names.insert(name) {
                    return Err(IdentityError::InvalidLease {
                        space: IdentitySpace::PrivateNameSerial,
                        detail: "private-name relocation duplicated a classifiable name",
                    });
                }
            }
        }
        for (_, _, symbol) in pattern_ambient_modules {
            symbol_relocation.symbol(symbol)?;
        }

        *private_name_serial_base = serial_relocation.new.start();
        *next_symbol_id = serial_relocation.new.end();
        *private_name_serial_lease = Some(serial_lease);
        Ok(())
    }
}

fn bind_leases(
    leases: Vec<IdentityLease>,
) -> Result<(IdentityLease, IdentityLease), IdentityError> {
    let mut symbol = None;
    let mut serial = None;
    for lease in leases {
        match lease.space() {
            IdentitySpace::Symbol => symbol = Some(lease),
            IdentitySpace::PrivateNameSerial => serial = Some(lease),
            space => {
                return Err(IdentityError::InvalidLease {
                    space,
                    detail: "bind publication received a non-bind lease",
                });
            }
        }
    }
    Ok((
        symbol.ok_or(IdentityError::ReservationMismatch)?,
        serial.ok_or(IdentityError::ReservationMismatch)?,
    ))
}

fn validate_serial_lease(
    lease: &IdentityLease,
    old_start: u32,
    old_end: u32,
    require_same_base: bool,
) -> Result<(), IdentityError> {
    if lease.space() != IdentitySpace::PrivateNameSerial {
        return Err(IdentityError::InvalidLease {
            space: IdentitySpace::PrivateNameSerial,
            detail: "binder received a non-private-name lease",
        });
    }
    if lease.range().len() != old_end - old_start {
        return Err(IdentityError::InvalidLease {
            space: IdentitySpace::PrivateNameSerial,
            detail: "private-name lease length differs from the assigned serial count",
        });
    }
    if require_same_base && lease.range().start() != old_start {
        return Err(IdentityError::InvalidLease {
            space: IdentitySpace::PrivateNameSerial,
            detail: "direct-construction serial lease base differs from the binder seed",
        });
    }
    Ok(())
}

fn relocate_symbol_table(
    table: &mut SymbolTable,
    symbol_relocation: &SymbolIdentityRelocation,
    serial_relocation: &PrivateNameSerialRelocation,
) -> Result<(), IdentityError> {
    relocate_symbol_table_values(table, symbol_relocation)?;
    relocate_private_table_keys(table, serial_relocation)
}

fn relocate_private_table_keys(
    table: &mut SymbolTable,
    serial_relocation: &PrivateNameSerialRelocation,
) -> Result<(), IdentityError> {
    if !table.keys().any(|name| name.starts_with("__#")) {
        return Ok(());
    }
    let old_table = std::mem::take(table);
    table.reserve(old_table.len());
    for (mut name, symbol) in old_table {
        serial_relocation.name(&mut name)?;
        if table.insert(name, symbol).is_some() {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::PrivateNameSerial,
                detail: "private-name relocation duplicated a symbol-table key",
            });
        }
    }
    Ok(())
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

impl BinderWorker<'_> {
    /// Publish the completed result and discard all temporary worker state.
    /// The exhaustive move is deliberate: adding a field to `BinderWorker` forces
    /// an explicit decision about whether it belongs in `BindData` or remains
    /// worker-local.
    pub fn into_bind_data(self) -> BindData {
        let Self {
            source: _,
            options: _,
            language_version,
            common_js_module_indicator,
            symbols,
            node_symbol,
            node_local_symbol,
            locals,
            js_global_augmentations,
            bind_diagnostics,
            classifiable_names,
            assigned_symbol_ids,
            private_name_serial_base,
            next_symbol_id,
            private_name_serial_lease,
            container: _,
            this_parent_container: _,
            block_scope_container: _,
            last_container: _,
            next_container,
            node_flags_mut,
            pattern_ambient_modules,
            flow,
            unreachable_flow,
            current_flow: _,
            current_break_target: _,
            current_continue_target: _,
            current_return_target: _,
            current_true_target: _,
            current_false_target: _,
            current_exception_target: _,
            pre_switch_case_flow: _,
            node_flow,
            node_end_flow,
            node_return_flow,
            node_flow_when_true,
            node_flow_when_false,
            possibly_exhaustive,
            node_fallthrough_flow,
            active_label_list: _,
            in_strict_mode: _,
            seen_this_keyword: _,
            in_assignment_pattern: _,
            has_explicit_return: _,
            in_return_position: _,
            has_flow_effects: _,
            emit_flags,
            delayed_type_aliases: _,
            js_doc_imports: _,
        } = self;
        BindData {
            language_version,
            common_js_module_indicator,
            symbols,
            node_symbol,
            node_local_symbol,
            locals,
            js_global_augmentations,
            bind_diagnostics,
            classifiable_names,
            assigned_symbol_ids,
            private_name_serial_base,
            next_symbol_id,
            private_name_serial_lease,
            next_container,
            node_flags_mut,
            pattern_ambient_modules,
            flow,
            unreachable_flow,
            node_flow,
            node_end_flow,
            node_return_flow,
            node_flow_when_true,
            node_flow_when_false,
            possibly_exhaustive,
            node_fallthrough_flow,
            emit_flags,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/declare/tests.rs"]
mod tests;
