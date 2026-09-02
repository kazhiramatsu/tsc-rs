use tsc_binder::SymbolId;
use tsc_emitter::{
    EmitModuleSpecifierHost, EmitResolverError, EmitSymbolAccessibility, EmitSymbolMeaning,
    EmitSymbolTracker, EmitTrackerAccess, EmitTrackerNode, EmitTrackerNodeDescription,
    EmitTrackerSymbol,
};
use tsc_syntax::NodeId;
use tsc_types::SymbolFlags;

use super::context::{RecoveryTrackedSymbol, TrackedSymbol};

/// Safe Rust ownership makes the upstream constructor's unwrap loop
/// structural: this wrapper intentionally does not implement
/// `EmitSymbolTracker`, so `inner` can never itself be a
/// `NodeBuilderTracker`.
/// tsc-port: SymbolTrackerImpl @6.0.3
/// tsc-hash: e5de2244865bcd5f14f7d0e16b3f308b63a98cfa2c624b917288ec7d5aaf8b86
/// tsc-span: _tsc.js:90969-91068
pub(crate) struct NodeBuilderTracker<'tracker> {
    pub(crate) inner: Option<&'tracker mut dyn EmitSymbolTracker>,
    pub(crate) disable_track_symbol: bool,
    pub(crate) can_track_symbol: bool,
    /// `true` is the nullish-coalescing fallback to
    /// createBasicNodeBuilderModuleSpecifierResolutionHost. The concrete
    /// checker-backed host is borrowed on demand by the specifier worker, so
    /// it never aliases the mutable inner tracker.
    pub(crate) uses_basic_module_resolver_host: bool,
    /// `symbolTableToDeclarationStatements` replaces the caller's tracker
    /// with a wrapper that consumes accessible symbols as private declaration
    /// dependencies and forwards only inaccessible symbols. The serializer
    /// drains this queue while its private-context stack is live.
    statement_symbols: Option<Vec<(SymbolId, EmitSymbolMeaning)>>,
}

impl<'tracker> NodeBuilderTracker<'tracker> {
    /// tsc-port: SymbolTrackerImpl.constructor @6.0.3
    /// tsc-hash: 86a621c38feaa2ac30f30b3b2ac4e60669d06ecd8b94f923976af07f5b44e53d
    /// tsc-span: _tsc.js:90970-90982
    pub(crate) fn new(inner: Option<&'tracker mut dyn EmitSymbolTracker>) -> Self {
        let can_track_symbol = inner
            .as_deref()
            .is_some_and(EmitSymbolTracker::can_track_symbol);
        let uses_basic_module_resolver_host = inner
            .as_deref()
            .and_then(EmitSymbolTracker::module_specifier_host)
            .is_none();
        Self {
            inner,
            disable_track_symbol: false,
            can_track_symbol,
            uses_basic_module_resolver_host,
            statement_symbols: None,
        }
    }

    /// tsrs-native: statement-tracker window entry (upstream fake-scope tracking).
    pub(crate) fn begin_statement_tracking(
        &mut self,
    ) -> (bool, Option<Vec<(SymbolId, EmitSymbolMeaning)>>) {
        let old_can_track_symbol = self.can_track_symbol;
        self.can_track_symbol = true;
        (
            old_can_track_symbol,
            self.statement_symbols.replace(Vec::new()),
        )
    }

    /// tsrs-native: Rust-structural helper for the h2-7a-m-3 foundation.
    pub(crate) fn take_statement_symbols(&mut self) -> Vec<(SymbolId, EmitSymbolMeaning)> {
        self.statement_symbols
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// tsrs-native: statement-tracker window probe.
    pub(crate) fn is_statement_tracking(&self) -> bool {
        self.statement_symbols.is_some()
    }

    /// tsrs-native: statement-tracker window exit.
    pub(crate) fn end_statement_tracking(
        &mut self,
        restore: (bool, Option<Vec<(SymbolId, EmitSymbolMeaning)>>),
    ) {
        self.can_track_symbol = restore.0;
        self.statement_symbols = restore.1;
    }

    /// A `None` result selects the checker-backed basic host recorded by
    /// `uses_basic_module_resolver_host`.
    /// tsc-port: withContext @6.0.3 (moduleResolverHost selection)
    /// tsc-hash: 48f43182478e60c913e8248cd1b555bc04b83c31cfbbcf7e790f6bb005ac13b6
    /// tsc-span: _tsc.js:51205-51206
    pub(crate) fn caller_module_resolver_host(&self) -> Option<&dyn EmitModuleSpecifierHost> {
        self.inner
            .as_deref()
            .and_then(EmitSymbolTracker::module_specifier_host)
    }

    /// tsc-port: SymbolTrackerImpl.trackSymbol @6.0.3
    /// tsc-hash: 63befc8640c710b6cfa04e423a074a71e87ea4e1c450c0312f1ca8ebd55ae808
    /// tsc-span: _tsc.js:90983-90993
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn track_symbol(
        &mut self,
        reported_diagnostic: &mut bool,
        tracked_symbols: &mut Option<Vec<TrackedSymbol>>,
        recovery_tracked_symbols: &mut Option<Vec<RecoveryTrackedSymbol>>,
        access: &mut dyn EmitTrackerAccess,
        symbol: SymbolId,
        symbol_flags: SymbolFlags,
        enclosing_declaration: Option<NodeId>,
        enclosing_declaration_is_synthetic: bool,
        meaning: EmitSymbolMeaning,
        symbol_is_remapped: bool,
    ) -> Result<bool, EmitResolverError> {
        if self.disable_track_symbol {
            return Ok(false);
        }
        if let Some(buffered) = recovery_tracked_symbols.as_mut() {
            buffered.push((
                symbol,
                symbol_flags,
                enclosing_declaration,
                enclosing_declaration_is_synthetic,
                meaning,
                symbol_is_remapped,
            ));
            return Ok(false);
        }
        if let Some(statement_symbols) = self.statement_symbols.as_mut() {
            if symbol_is_remapped {
                return Ok(false);
            }
            let accessibility = access.is_symbol_accessible(
                tracker_symbol(symbol),
                enclosing_declaration
                    .map(|node| tracker_enclosing_node(node, enclosing_declaration_is_synthetic)),
                meaning,
                false,
            )?;
            if accessibility.accessibility == EmitSymbolAccessibility::Accessible {
                if !symbol_flags.intersects(SymbolFlags::PROPERTY) {
                    statement_symbols.push((symbol, meaning));
                }
                if !symbol_flags.intersects(SymbolFlags::TYPE_PARAMETER) {
                    tracked_symbols.get_or_insert_with(Vec::new).push((
                        symbol,
                        enclosing_declaration,
                        meaning,
                    ));
                }
                return Ok(false);
            }
        }
        if !self.can_track_symbol {
            return Ok(false);
        }
        let Some(inner) = self.inner.as_deref_mut() else {
            return Ok(false);
        };
        let introduced_error = inner.track_symbol(
            access,
            tracker_symbol(symbol),
            symbol_flags,
            enclosing_declaration
                .map(|node| tracker_enclosing_node(node, enclosing_declaration_is_synthetic)),
            meaning,
        )?;
        if introduced_error {
            Self::on_diagnostic_reported(reported_diagnostic);
            return Ok(true);
        }
        if !symbol_flags.intersects(SymbolFlags::TYPE_PARAMETER) {
            tracked_symbols.get_or_insert_with(Vec::new).push((
                symbol,
                enclosing_declaration,
                meaning,
            ));
        }
        Ok(false)
    }

    /// tsc-port: SymbolTrackerImpl.reportInaccessibleThisError @6.0.3
    /// tsc-hash: 3342767095907d12b63b016d1f4d19d0437033c9882b108ee01c55afe184085c
    /// tsc-span: _tsc.js:90994-91000
    pub(crate) fn report_inaccessible_this_error(&mut self, reported_diagnostic: &mut bool) {
        if let Some(inner) = self.inner.as_deref_mut() {
            Self::on_diagnostic_reported(reported_diagnostic);
            inner.report_inaccessible_this_error();
        }
    }

    /// tsc-port: SymbolTrackerImpl.reportPrivateInBaseOfClassExpression @6.0.3
    /// tsc-hash: 7246a663106e14972dabfa9c357b0dd78e7b86948c6cb3774f680d377a1d8872
    /// tsc-span: _tsc.js:91001-91007
    pub(crate) fn report_private_in_base_of_class_expression(
        &mut self,
        reported_diagnostic: &mut bool,
        property_name: &str,
    ) {
        if let Some(inner) = self.inner.as_deref_mut() {
            Self::on_diagnostic_reported(reported_diagnostic);
            inner.report_private_in_base_of_class_expression(property_name);
        }
    }

    /// tsc-port: SymbolTrackerImpl.reportInaccessibleUniqueSymbolError @6.0.3
    /// tsc-hash: b320eba6c2f9b16133783215e2b814dc5d4737c696a4ec65c8f6a7f057140764
    /// tsc-span: _tsc.js:91008-91014
    pub(crate) fn report_inaccessible_unique_symbol_error(
        &mut self,
        reported_diagnostic: &mut bool,
    ) {
        if let Some(inner) = self.inner.as_deref_mut() {
            Self::on_diagnostic_reported(reported_diagnostic);
            inner.report_inaccessible_unique_symbol_error();
        }
    }

    /// tsc-port: SymbolTrackerImpl.reportCyclicStructureError @6.0.3
    /// tsc-hash: c10bbc9a18d082f3ee7a2a148869e59743b71ea4227ebd8ef9c711d56b85f4af
    /// tsc-span: _tsc.js:91015-91021
    pub(crate) fn report_cyclic_structure_error(&mut self, reported_diagnostic: &mut bool) {
        if let Some(inner) = self.inner.as_deref_mut() {
            Self::on_diagnostic_reported(reported_diagnostic);
            inner.report_cyclic_structure_error();
        }
    }

    /// tsc-port: SymbolTrackerImpl.reportLikelyUnsafeImportRequiredError @6.0.3
    /// tsc-hash: 3c9958ba706a65a30fe5710d708d1fef9689ee5678fb0424b6990ad4460b5292
    /// tsc-span: _tsc.js:91022-91028
    pub(crate) fn report_likely_unsafe_import_required_error(
        &mut self,
        reported_diagnostic: &mut bool,
        specifier: &str,
        symbol_name: Option<&str>,
    ) {
        if let Some(inner) = self.inner.as_deref_mut() {
            Self::on_diagnostic_reported(reported_diagnostic);
            inner.report_likely_unsafe_import_required_error(specifier, symbol_name);
        }
    }

    /// tsc-port: SymbolTrackerImpl.reportTruncationError @6.0.3
    /// tsc-hash: bcf968c46debc71700366d17b5b5a169bb10b1bd4c132fd6dbf72fa4857faeae
    /// tsc-span: _tsc.js:91029-91035
    pub(crate) fn report_truncation_error(&mut self, reported_diagnostic: &mut bool) {
        if let Some(inner) = self.inner.as_deref_mut() {
            Self::on_diagnostic_reported(reported_diagnostic);
            inner.report_truncation_error();
        }
    }

    /// tsc-port: SymbolTrackerImpl.reportNonlocalAugmentation @6.0.3
    /// tsc-hash: ff2f1f906bf5e0e8fde4d2a5bc2a77fd88ae85fb680311faec75c79d6c740d1a
    /// tsc-span: _tsc.js:91036-91042
    pub(crate) fn report_nonlocal_augmentation(
        &mut self,
        reported_diagnostic: &mut bool,
        primary_declaration: Option<EmitTrackerNodeDescription>,
        augmenting_declarations: Vec<EmitTrackerNodeDescription>,
    ) {
        if let Some(inner) = self.inner.as_deref_mut() {
            Self::on_diagnostic_reported(reported_diagnostic);
            inner.report_nonlocal_augmentation(primary_declaration, augmenting_declarations);
        }
    }

    /// tsc-port: SymbolTrackerImpl.reportNonSerializableProperty @6.0.3
    /// tsc-hash: 227a16e3134442aaeeefc90843ff7448eb455298647ae1e455df5cf9754eaab0
    /// tsc-span: _tsc.js:91043-91049
    pub(crate) fn report_non_serializable_property(
        &mut self,
        reported_diagnostic: &mut bool,
        property_name: &str,
    ) {
        if let Some(inner) = self.inner.as_deref_mut() {
            Self::on_diagnostic_reported(reported_diagnostic);
            inner.report_non_serializable_property(property_name);
        }
    }

    /// tsc-port: SymbolTrackerImpl.onDiagnosticReported @6.0.3
    /// tsc-hash: f8411fa0d36491bdd5b6d799a2ff2ad1c9361a13c8bca16fa759c1e37240477f
    /// tsc-span: _tsc.js:91050-91052
    fn on_diagnostic_reported(reported_diagnostic: &mut bool) {
        *reported_diagnostic = true;
    }

    /// tsc-port: SymbolTrackerImpl.reportInferenceFallback @6.0.3
    /// tsc-hash: 4b25aaa0362e46313978c2e2428ccb8251c56ef95e8a53858df6f79877d0b11a
    /// tsc-span: _tsc.js:91053-91059
    pub(crate) fn report_inference_fallback(
        &mut self,
        reported_diagnostic: &mut bool,
        suppress_report_inference_fallback: bool,
        access: &mut dyn EmitTrackerAccess,
        node: NodeId,
    ) -> Result<(), EmitResolverError> {
        if suppress_report_inference_fallback {
            return Ok(());
        }
        if let Some(inner) = self.inner.as_deref_mut() {
            Self::on_diagnostic_reported(reported_diagnostic);
            inner.report_inference_fallback(access, tracker_node(node))?;
        }
        Ok(())
    }

    /// tsc-port: SymbolTrackerImpl.pushErrorFallbackNode @6.0.3
    /// tsc-hash: db55706784167d0fabdb0d4f8e58a8f1790a83aa703005a9c7f6cdbcdc5d760a
    /// tsc-span: _tsc.js:91060-91063
    pub(crate) fn push_error_fallback_node(&mut self, node: Option<EmitTrackerNodeDescription>) {
        if let Some(inner) = self.inner.as_deref_mut() {
            inner.push_error_fallback_node(node);
        }
    }

    /// tsc-port: SymbolTrackerImpl.popErrorFallbackNode @6.0.3
    /// tsc-hash: 322f6a54fab329e4057a1a3d87d8854e558ae7c9d22922ddae1aec23579c1f54
    /// tsc-span: _tsc.js:91064-91067
    pub(crate) fn pop_error_fallback_node(&mut self) {
        if let Some(inner) = self.inner.as_deref_mut() {
            inner.pop_error_fallback_node();
        }
    }
}

fn tracker_symbol(symbol: SymbolId) -> EmitTrackerSymbol {
    EmitTrackerSymbol(u64::from(symbol.0))
}

fn tracker_node(node: NodeId) -> EmitTrackerNode {
    EmitTrackerNode(u64::from(node.0))
}

const SYNTHETIC_SCOPE_BIT: u64 = 1 << 63;

fn tracker_enclosing_node(node: NodeId, synthetic: bool) -> EmitTrackerNode {
    EmitTrackerNode(u64::from(node.0) | if synthetic { SYNTHETIC_SCOPE_BIT } else { 0 })
}

/// tsrs-native: Rust-structural helper for the h2-7a-m-3 foundation.
pub(crate) fn tracker_node_id(node: EmitTrackerNode) -> Option<NodeId> {
    u32::try_from(node.0 & !SYNTHETIC_SCOPE_BIT)
        .ok()
        .map(NodeId)
}

/// tsrs-native: Rust-structural helper for the h2-7a-m-3 foundation.
pub(crate) const fn tracker_node_is_synthetic(node: EmitTrackerNode) -> bool {
    node.0 & SYNTHETIC_SCOPE_BIT != 0
}
