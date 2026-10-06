//! Checker-owned resolver projection used only while an emitting checker
//! session remains alive.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use tsc_emitter::{
    EmitConstantValue, EmitEnumMemberValue, EmitExportContainerMode, EmitFunctionProperty,
    EmitResolver, EmitResolverError, EmitResolverMethod, EmitResolverNode, EmitResolverSymbol,
    EmitSymbolAccessibilityResult, EmitSymbolMeaning, EmitTypeReferenceSerializationKind,
    JavaScriptNumber, JavaScriptString,
};

use crate::state::{CheckResult, CheckerState};
use crate::{evaluate::EvalValue, AuthoritativeSourceToken, ProgramSnapshot};
use tsc_binder::SymbolId;
use tsc_diagnostics::DiagnosticList;
use tsc_types::CompilerOptions;

static NEXT_EMIT_RESOLVER_SESSION_TOKEN: AtomicU64 = AtomicU64::new(1);

/// One fresh checker whose semantic links and transient arenas remain alive
/// while transform and print borrow its narrow [`EmitResolver`] projection.
/// The session never owns or mutates the immutable [`ProgramSnapshot`].
/// A checked state behind a mutex: the emit resolver of one checker can be
/// shared by the emit workers of every file that checker owns (tsgo's shape:
/// emit runs per file on all cores, each file's resolver queries locking its
/// checker), and the whole session moves between threads.
pub struct CheckerSession<'program> {
    state: Mutex<CheckerState<'program>>,
    session_token: u64,
    program_diagnostics: Mutex<Option<ProgramDiagnosticContext>>,
}

struct ProgramDiagnosticContext {
    preparation: DiagnosticList,
    semantic: Option<DiagnosticList>,
}

impl<'program> CheckerSession<'program> {
    /// Construct a fresh session over already parsed and bound documents.
    /// Host facts and source checking are installed by the checker driver
    /// before it exposes this session to the emitter.
    /// tsrs-native: checker-session owner over the immutable Program snapshot.
    pub fn from_snapshot(
        snapshot: &'program ProgramSnapshot,
        options: &'program CompilerOptions,
    ) -> Self {
        Self::from_checked_state(CheckerState::from_snapshot(snapshot, options))
    }

    /// Transfer a fully initialized checker into the scoped resolver owner.
    /// tsrs-native: ownership adapter for the H1 checker callback boundary.
    pub fn from_checked_state(mut state: CheckerState<'program>) -> Self {
        // The line profile of every path that hands a checked state to a
        // session (the emit gate, the no-emit declaration getter).
        state.line_profile.flush();
        Self {
            state: Mutex::new(state),
            session_token: NEXT_EMIT_RESOLVER_SESSION_TOKEN.fetch_add(1, Ordering::Relaxed),
            program_diagnostics: Mutex::new(None),
        }
    }

    /// tsrs-native: builder setter storing program preparation/semantic diagnostic context
    pub(crate) fn with_program_diagnostics(
        self,
        preparation: DiagnosticList,
        semantic: Option<DiagnosticList>,
    ) -> Self {
        *self
            .program_diagnostics
            .lock()
            .expect("checker session diagnostics") = Some(ProgramDiagnosticContext {
            preparation,
            semantic,
        });
        self
    }

    /// Observe the initialized checker's current file-less diagnostic bucket.
    /// This does not schedule source checking or declaration transforms.
    /// tsrs-native: observes the current file-less bucket with sort/dedupe WITHOUT tsc
    /// checker.getGlobalDiagnostics' ensurePendingDiagnosticWorkComplete (87133-87136);
    /// cite that difference
    pub fn get_global_diagnostics(&self) -> DiagnosticList {
        let mut diagnostics = self
            .state
            .lock()
            .expect("checker session state")
            .visible_global_diagnostics
            .clone();
        tsc_diagnostics::sort_and_dedupe_diagnostics(&mut diagnostics);
        diagnostics
    }

    /// Complete the whole-Program semantic getter on this same checker.
    /// Program diagnostics are cached independently of the declaration getter;
    /// a declaration resolver preparation does not populate this cache.
    /// The second tuple member preserves typed partial-check evidence.
    /// tsrs-native: scoped getDiagnosticsHelper/getDiagnosticsWorker adapter
    /// (_tsc.js:123631-123641,87111-87135), reusing the Program row producer.
    pub fn get_program_semantic_diagnostics(
        &self,
    ) -> Result<(DiagnosticList, Vec<crate::PartialCheck>), crate::AuthoritativeModuleFailure> {
        let mut state = self.state.lock().expect("checker session state");
        let mut context = self
            .program_diagnostics
            .lock()
            .expect("checker session diagnostics");
        let context =
            context
                .as_mut()
                .ok_or_else(|| crate::AuthoritativeModuleFailure::InvalidMetadata {
                    detail: "whole-Program diagnostics require an authoritative Program context"
                        .to_owned(),
                })?;
        if let Some(diagnostics) = &context.semantic {
            return Ok((diagnostics.clone(), state.partial_check_records.clone()));
        }
        let files = state.binder.file_ids().collect::<Vec<_>>();
        let mut new_globals = vec![Vec::new(); files.len()];
        for file in &files {
            if state.skip_type_checking_file(*file) {
                continue;
            }
            // getDiagnosticsWorker returns only globals newly published by
            // this semantic request. Earlier declaration preparations did
            // not populate the Program bind/check diagnostic cache.
            let start = state.visible_global_diagnostics.len();
            state.check_source_file(file.index());
            new_globals[file.index()].extend_from_slice(&state.visible_global_diagnostics[start..]);
        }
        if let Some(failure) = state.take_authoritative_module_failure() {
            return Err(failure);
        }
        let mut diagnostics = Vec::new();
        for file in files {
            if state.skip_type_checking_file(file) {
                continue;
            }
            diagnostics.extend(crate::semantic_diagnostics_for_program_file(
                &state,
                file.index(),
                &new_globals[file.index()],
                &context.preparation,
                state.options,
            ));
        }
        tsc_diagnostics::sort_and_dedupe_diagnostics(&mut diagnostics);
        context.semantic = Some(diagnostics.clone());
        Ok((diagnostics, state.partial_check_records.clone()))
    }

    /// Borrow only the consumer-owned resolver protocol for transform/print.
    /// tsrs-native: scoped Rust borrowing form of tsc's checker-owned resolver.
    pub fn with_emit_resolver<T>(&self, operation: impl FnOnce(&dyn EmitResolver) -> T) -> T {
        operation(self)
    }

    /// tsc-port: getEmitResolver @6.0.3
    /// tsc-hash: 117340a1fd0f33afef752cc545f66125543fc3a9fd75b7c152e546461b65516a
    /// tsc-span: _tsc.js:47561-47564
    /// Complete the selected source before lending this same session's resolver.
    /// The compiler owns the declaration getter cache, so cached requests do
    /// not reach this preparation step again.
    pub fn prepare_declaration_source(
        &self,
        source: AuthoritativeSourceToken,
    ) -> Result<Vec<crate::PartialCheck>, crate::AuthoritativeModuleFailure> {
        let mut state = self.state.lock().expect("checker session state");
        let index = state
            .authoritative_source_index_by_token
            .get(&source)
            .copied()
            .ok_or_else(|| crate::AuthoritativeModuleFailure::InvalidMetadata {
                detail: format!(
                    "declaration getter source token {} is not in this Program",
                    source.0
                ),
            })?;
        state.check_source_file(index);
        if let Some(failure) = state.take_authoritative_module_failure() {
            return Err(failure);
        }
        Ok(state.partial_check_records.clone())
    }

    /// Complete the whole-Program getEmitResolver request without filling
    /// Program's semantic diagnostic cache. Reporting may have skipped its
    /// semantic getter because an earlier diagnostic bucket was nonempty.
    /// tsrs-native: whole-source branch of getEmitResolver (_tsc.js:47561-47564).
    pub fn prepare_program_emit(
        &self,
    ) -> Result<Vec<crate::PartialCheck>, crate::AuthoritativeModuleFailure> {
        let mut state = self.state.lock().expect("checker session state");
        let files = state.binder.file_ids().collect::<Vec<_>>();
        for file in files {
            state.check_source_file(file.index());
        }
        if let Some(failure) = state.take_authoritative_module_failure() {
            return Err(failure);
        }
        Ok(state.partial_check_records.clone())
    }

    /// tsrs-native: H2.8c evidence — how many source files ran the
    /// checkSourceFileWorker body in this session so far.
    pub fn checked_source_files(&self) -> u32 {
        self.state
            .lock()
            .expect("checker session state")
            .checked_source_files
    }

    /// Reclaim checker state after the emitter has released its resolver
    /// borrow so the driver can assemble diagnostics and observations.
    /// tsrs-native: ownership adapter after the H1 checker callback boundary.
    pub fn into_state(self) -> CheckerState<'program> {
        self.state.into_inner().expect("checker session state")
    }

    fn with_resolver_node<T>(
        &self,
        method: EmitResolverMethod,
        node: EmitResolverNode,
        operation: impl FnOnce(&mut CheckerState<'program>, tsc_syntax::NodeId) -> CheckResult<T>,
    ) -> Result<T, EmitResolverError> {
        let mut state = self.state.lock().expect("checker session state");
        validate_resolver_node(&state, method, node)?;
        operation(&mut state, node.node()).map_err(|abort| EmitResolverError::CheckerAborted {
            method,
            node,
            reason: abort.description(),
        })
    }

    fn with_resolver_node_and_location<T>(
        &self,
        method: EmitResolverMethod,
        node: EmitResolverNode,
        location: EmitResolverNode,
        operation: impl FnOnce(
            &mut CheckerState<'program>,
            tsc_syntax::NodeId,
            tsc_syntax::NodeId,
        ) -> CheckResult<T>,
    ) -> Result<T, EmitResolverError> {
        let mut state = self.state.lock().expect("checker session state");
        validate_resolver_node(&state, method, node)?;
        validate_resolver_node(&state, method, location)?;
        operation(&mut state, node.node(), location.node()).map_err(|abort| {
            EmitResolverError::CheckerAborted {
                method,
                node,
                reason: abort.description(),
            }
        })
    }

    fn with_resolver_node_and_symbol<T>(
        &self,
        method: EmitResolverMethod,
        node: EmitResolverNode,
        symbol: EmitResolverSymbol,
        operation: impl FnOnce(
            &mut CheckerState<'program>,
            tsc_syntax::NodeId,
            SymbolId,
        ) -> CheckResult<T>,
    ) -> Result<T, EmitResolverError> {
        let mut state = self.state.lock().expect("checker session state");
        validate_resolver_node(&state, method, node)?;
        let symbol = validate_resolver_symbol(&state, self.session_token, method, symbol)?;
        operation(&mut state, node.node(), symbol).map_err(|abort| {
            EmitResolverError::CheckerAborted {
                method,
                node,
                reason: abort.description(),
            }
        })
    }
}

/// tsc-port: createResolver @6.0.3
/// tsc-hash: 56a0d47f897fcf258d6e316a00f9dc5e7d18a3ed1936033ab7c9350f623b3df2
/// tsc-span: _tsc.js:88545-88718
///
/// Resolver producers are exposed only as their consuming transform slices
/// become live. H2.2a adds constant and enum-member values; P2/P3 add the
/// declaration-visibility and predicate workers while later consumer-owned
/// methods retain the trait's typed unavailable default.
impl EmitResolver for CheckerSession<'_> {
    fn has_global_name(&self, name: &str) -> Result<bool, EmitResolverError> {
        Ok(self
            .state
            .lock()
            .expect("checker session state")
            .emit_has_global_name(name))
    }

    fn collect_linked_aliases(
        &self,
        node: EmitResolverNode,
        set_visibility: bool,
    ) -> Result<Option<Vec<EmitResolverNode>>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::CollectLinkedAliases,
            node,
            |state, node| {
                state
                    .collect_linked_aliases(node, set_visibility)
                    .map(|nodes| {
                        nodes.map(|nodes| {
                            nodes
                                .into_iter()
                                .map(|node| project_resolver_node(state, node))
                                .collect()
                        })
                    })
            },
        )
    }

    fn is_source_checked(
        &self,
        source: tsc_program::SourceFileId,
    ) -> Result<bool, EmitResolverError> {
        let state = self.state.lock().expect("checker session state");
        let index = resolver_source_index(&state, EmitResolverMethod::IsSourceChecked, source)?;
        let root = state.binder.source(index).root;
        Ok(state
            .links
            .read_node(root, |links| links.check_flags)
            .intersects(tsc_types::NodeCheckFlags::TYPE_CHECKED))
    }

    fn can_include_bind_and_check_diagnostics(
        &self,
        source: tsc_program::SourceFileId,
    ) -> Result<bool, EmitResolverError> {
        let state = self.state.lock().expect("checker session state");
        let source_index = resolver_source_index(
            &state,
            EmitResolverMethod::CanIncludeBindAndCheckDiagnostics,
            source,
        )?;
        let source = state.binder.source(source_index);
        Ok(crate::can_include_bind_and_check_diagnostics(
            crate::is_js_file_name(&source.file_name),
            crate::check_directive(source.text()),
            state.options,
        ))
    }

    fn get_constant_value(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetConstantValue,
            node,
            CheckerState::emit_get_constant_value,
        )
    }

    fn get_enum_member_value(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitEnumMemberValue>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetEnumMemberValue,
            node,
            CheckerState::emit_get_enum_member_value,
        )
    }

    fn get_referenced_export_container(
        &self,
        node: EmitResolverNode,
        mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        // A container or declaration may live in another file (a UMD
        // `export as namespace` alias resolved from a JSX factory, an alias
        // imported through a different source): project it with its own
        // source so the emitter's same-source guards see the truth.
        self.with_resolver_node(
            EmitResolverMethod::GetReferencedExportContainer,
            node,
            |state, reference| {
                let container = state.emit_get_referenced_export_container(reference, mode)?;
                Ok(container.map(|container| project_resolver_node(state, container)))
            },
        )
    }

    fn get_external_module_file_from_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetExternalModuleFileFromDeclaration,
            node,
            |state, declaration| {
                state
                    .get_external_module_file_from_declaration(declaration)
                    .map(|file| file.map(|file| project_resolver_node(state, file)))
            },
        )
    }

    fn get_referenced_import_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetReferencedImportDeclaration,
            node,
            |state, reference| {
                let declaration = state.emit_get_referenced_import_declaration(reference)?;
                Ok(declaration.map(|declaration| project_resolver_node(state, declaration)))
            },
        )
    }

    fn get_referenced_import_declaration_at_location(
        &self,
        node: EmitResolverNode,
        location: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node_and_location(
            EmitResolverMethod::GetReferencedImportDeclarationAtLocation,
            node,
            location,
            |state, reference, location| {
                let declaration = state
                    .emit_get_referenced_import_declaration_at_location(reference, location)?;
                Ok(declaration.map(|declaration| project_resolver_node(state, declaration)))
            },
        )
    }

    fn get_jsx_factory_import_declaration(
        &self,
        node: EmitResolverNode,
        name: &str,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetJsxFactoryImportDeclaration,
            node,
            |state, location| {
                let declaration = state.emit_get_jsx_factory_import_declaration(location, name)?;
                Ok(declaration.map(|declaration| project_resolver_node(state, declaration)))
            },
        )
    }

    fn get_jsx_factory_export_container(
        &self,
        node: EmitResolverNode,
        name: &str,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetJsxFactoryExportContainer,
            node,
            |state, location| {
                let container = state.emit_get_jsx_factory_export_container(location, name)?;
                Ok(container.map(|container| project_resolver_node(state, container)))
            },
        )
    }

    fn get_referenced_value_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetReferencedValueDeclaration,
            node,
            |state, reference| {
                let declaration = state.emit_get_referenced_value_declaration(reference)?;
                Ok(declaration.map(|declaration| project_resolver_node(state, declaration)))
            },
        )
    }

    fn get_referenced_member_value_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetReferencedMemberValueDeclaration,
            node,
            |state, reference| {
                let declaration = state.emit_get_referenced_member_value_declaration(reference)?;
                Ok(declaration.map(|declaration| project_resolver_node(state, declaration)))
            },
        )
    }

    fn get_referenced_value_declaration_of_name(
        &self,
        location: EmitResolverNode,
        name: &str,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetReferencedValueDeclarationOfName,
            location,
            |state, location| {
                let declaration =
                    state.emit_get_referenced_value_declaration_of_name(location, name)?;
                Ok(declaration.map(|declaration| project_resolver_node(state, declaration)))
            },
        )
    }

    fn is_common_js_alias_export(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsCommonJsAliasExport,
            node,
            |state, node| Ok(state.emit_is_common_js_alias_export(node)),
        )
    }

    fn get_tracker_symbol_of_node(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<tsc_emitter::EmitTrackerSymbol>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetTrackerSymbolOfNode,
            node,
            |state, node| {
                Ok(state
                    .node_symbol(node)
                    .map(crate::node_builder::tracker_symbol))
            },
        )
    }

    fn precalculate_declaration_emit_visibility(
        &self,
        node: EmitResolverNode,
    ) -> Result<(), EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::PrecalculateDeclarationEmitVisibility,
            node,
            |state, file| state.emit_precalculate_declaration_emit_visibility(file),
        )
    }

    fn get_export_equals_declarations(
        &self,
        node: EmitResolverNode,
    ) -> Result<Vec<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetExportEqualsDeclarations,
            node,
            |state, file| {
                let declarations = state.emit_get_export_equals_declarations(file);
                Ok(declarations
                    .into_iter()
                    .map(|declaration| project_resolver_node(state, declaration))
                    .collect())
            },
        )
    }

    fn is_assignment_declaration(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsAssignmentDeclaration,
            node,
            |state, declaration| Ok(state.emit_is_assignment_declaration(declaration)),
        )
    }

    fn get_element_access_expression_name(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<String>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetElementAccessExpressionName,
            node,
            |state, expression| state.emit_get_element_access_expression_name(expression),
        )
    }

    fn is_name_resolvable(
        &self,
        location: EmitResolverNode,
        name: &str,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsNameResolvable,
            location,
            |state, location| state.emit_is_name_resolvable(location, name),
        )
    }

    fn create_type_of_expando_member(
        &self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        declaration: EmitResolverNode,
        local_name: &str,
        enclosing_declaration: EmitResolverNode,
        flags: tsc_emitter::EmitNodeBuilderFlags,
        internal_flags: tsc_emitter::EmitInternalNodeBuilderFlags,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<Option<tsc_emitter::TransformNode>, EmitResolverError> {
        let method = EmitResolverMethod::CreateTypeOfExpandoMember;
        let mut state = self.state.lock().expect("checker session state");
        validate_resolver_node(&state, method, declaration)?;
        validate_resolver_node(&state, method, enclosing_declaration)?;
        state.emit_create_type_of_expando_member(
            arena,
            target,
            declaration.node(),
            local_name,
            enclosing_declaration.node(),
            flags,
            internal_flags,
            tracker,
        )
    }

    fn is_this_property_assignment_declaration_redundant(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsThisPropertyAssignmentDeclarationRedundant,
            node,
            |state, declaration| {
                state.emit_is_this_property_assignment_declaration_redundant(declaration)
            },
        )
    }

    fn get_referenced_value_declarations(
        &self,
        node: EmitResolverNode,
    ) -> Result<Vec<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetReferencedValueDeclarations,
            node,
            |state, reference| {
                let declarations = state.emit_get_referenced_value_declarations(reference)?;
                Ok(declarations
                    .into_iter()
                    .map(|declaration| project_resolver_node(state, declaration))
                    .collect())
            },
        )
    }

    fn get_referenced_declaration_with_colliding_name(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetReferencedDeclarationWithCollidingName,
            node,
            |state, reference| {
                let declaration =
                    state.emit_get_referenced_declaration_with_colliding_name(reference)?;
                Ok(declaration.map(|declaration| project_resolver_node(state, declaration)))
            },
        )
    }

    fn is_declaration_with_colliding_name(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsDeclarationWithCollidingName,
            node,
            |state, declaration| state.emit_is_declaration_with_colliding_name(declaration),
        )
    }

    fn is_binding_captured_by_node(
        &self,
        node: EmitResolverNode,
        declaration: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node_and_location(
            EmitResolverMethod::IsBindingCapturedByNode,
            node,
            declaration,
            CheckerState::emit_is_binding_captured_by_node,
        )
    }

    fn get_type_reference_serialization_kind(
        &self,
        node: EmitResolverNode,
        location: EmitResolverNode,
    ) -> Result<EmitTypeReferenceSerializationKind, EmitResolverError> {
        self.with_resolver_node_and_location(
            EmitResolverMethod::GetTypeReferenceSerializationKind,
            node,
            location,
            CheckerState::emit_get_type_reference_serialization_kind,
        )
    }

    /// tsc-port: hasNodeCheckFlag @6.0.3
    /// tsc-hash: 9625b75cf4c5d68d752872a88e9af36590a50b9c8b9c4f2e5fb23f49446afe43
    /// tsc-span: _tsc.js:88127-88130
    /// The unchecked-source branch (calculateNodeCheckFlagWorker) runs
    /// before the links read; checked sources return immediately from it.
    fn has_node_check_flag(
        &self,
        node: EmitResolverNode,
        flag: u32,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(EmitResolverMethod::HasNodeCheckFlag, node, |state, node| {
            let flag = tsc_types::NodeCheckFlags::from_bits(flag as i32);
            state.calculate_node_check_flag_worker(node, flag)?;
            Ok(state
                .links
                .read_node(node, |links| links.check_flags)
                .intersects(flag))
        })
    }

    /// tsgo-port: MarkLinkedReferencesRecursively @7.1
    /// (checker/emitresolver.go:808-830).
    ///
    /// tsgo's import elision calls this for every source it transforms
    /// (transformers/tstransforms/importelision.go:29). A source this checker
    /// has checked already carries every mark the walk would add (the check
    /// reaches the same markers from checkIdentifier, checkPropertyAccess and
    /// the rest), so the walk is left to the sources it has not checked: emit
    /// before the check, `noCheck`, and a file excluded by
    /// canIncludeBindAndCheckDiagnostics.
    fn mark_linked_references(
        &self,
        source: tsc_program::SourceFileId,
    ) -> Result<(), EmitResolverError> {
        let mut state = self.state.lock().expect("checker session state");
        let index =
            resolver_source_index(&state, EmitResolverMethod::MarkLinkedReferences, source)?;
        let file = state.binder.source(index);
        if crate::is_js_file_name(&file.file_name)
            || state.options.verbatim_module_syntax == Some(true)
            || state
                .links
                .read_node(file.root, |links| links.check_flags)
                .intersects(tsc_types::NodeCheckFlags::TYPE_CHECKED)
        {
            return Ok(());
        }
        // forEachChildRecursively(file, cb): pre-order over the children of
        // the source file, pruning import declarations and non-exported
        // import-equals declarations.
        let mut stack = Vec::new();
        tsc_syntax::for_each_child(&file.arena, file.arena.node(file.root), |child| {
            stack.push(child);
            false
        });
        stack.reverse();
        let root = file.root;
        while let Some(node) = stack.pop() {
            let source_file = state.binder.source_of_node(node);
            let skip = match &source_file.arena.node(node).data {
                tsc_syntax::NodeData::ImportDeclaration(_) => true,
                tsc_syntax::NodeData::ImportEqualsDeclaration(_) => {
                    !tsc_binder::node_util::has_syntactic_modifier(
                        source_file,
                        node,
                        tsc_types::ModifierFlags::EXPORT,
                    )
                }
                _ => false,
            };
            if skip {
                continue;
            }
            state
                .mark_linked_references_unspecified(node)
                .map_err(|abort| EmitResolverError::CheckerAborted {
                    method: EmitResolverMethod::MarkLinkedReferences,
                    node: EmitResolverNode::new(source, root),
                    reason: abort.description(),
                })?;
            let source_file = state.binder.source_of_node(node);
            let first = stack.len();
            tsc_syntax::for_each_child(&source_file.arena, source_file.arena.node(node), |child| {
                stack.push(child);
                false
            });
            stack[first..].reverse();
        }
        Ok(())
    }

    fn is_arguments_local_binding(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsArgumentsLocalBinding,
            node,
            CheckerState::emit_is_arguments_local_binding,
        )
    }

    /// tsc-port: isExternalOrCommonJsModule @6.0.3
    /// tsc-hash: e395fd4c4d5df1373eb3cc17bc653dfcd8f2e41b9e32d949b3063633dc02c07d
    /// tsc-span: _tsc.js:14119-14121
    fn is_external_or_common_js_module(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsExternalOrCommonJsModule,
            node,
            |state, node| Ok(state.binder.is_external_or_common_js_module_of_node(node)),
        )
    }

    /// tsrs-native: validated borrowing projection of the binder's CommonJS
    /// indicator for the module transform's esModule marker decision.
    fn is_common_js_module(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(EmitResolverMethod::IsCommonJsModule, node, |state, node| {
            Ok(state.binder.is_common_js_module_of_node(node))
        })
    }

    fn is_instantiated_module(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsInstantiatedModule,
            node,
            |state, node| Ok(state.is_instantiated_module(node)),
        )
    }

    fn is_unique_local_name(
        &self,
        node: EmitResolverNode,
        name: &str,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsUniqueLocalName,
            node,
            |state, node| state.emit_is_unique_local_name(node, name),
        )
    }

    fn is_referenced_alias_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsReferencedAliasDeclaration,
            node,
            CheckerState::emit_is_referenced_alias_declaration,
        )
    }

    fn is_top_level_value_import_equals_with_entity_name(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsTopLevelValueImportEqualsWithEntityName,
            node,
            CheckerState::emit_is_top_level_value_import_equals_with_entity_name,
        )
    }

    fn is_value_alias_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsValueAliasDeclaration,
            node,
            CheckerState::emit_is_value_alias_declaration,
        )
    }

    fn is_definitely_reference_to_global_symbol_object(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        let method = EmitResolverMethod::IsDefinitelyReferenceToGlobalSymbolObject;
        self.with_resolver_node(
            method,
            node,
            CheckerState::emit_is_definitely_reference_to_global_symbol_object,
        )
    }

    fn is_symbol_accessible(
        &self,
        symbol: EmitResolverSymbol,
        enclosing_declaration: EmitResolverNode,
        meaning: EmitSymbolMeaning,
        should_compute_aliases: bool,
    ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError> {
        let method = EmitResolverMethod::IsSymbolAccessible;
        self.with_resolver_node_and_symbol(
            method,
            enclosing_declaration,
            symbol,
            |state, enclosing_declaration, symbol| {
                state.emit_is_symbol_accessible(
                    symbol,
                    enclosing_declaration,
                    meaning,
                    should_compute_aliases,
                )
            },
        )
    }

    fn is_entity_name_visible(
        &self,
        entity_name: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
    ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError> {
        let method = EmitResolverMethod::IsEntityNameVisible;
        self.with_resolver_node_and_location(
            method,
            entity_name,
            enclosing_declaration,
            |state, entity_name, enclosing_declaration| {
                state.emit_is_entity_name_visible(
                    entity_name,
                    enclosing_declaration,
                    /*should_compute_aliases_to_make_visible*/ true,
                )
            },
        )
    }

    fn is_declaration_visible(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsDeclarationVisible,
            node,
            CheckerState::emit_is_declaration_visible,
        )
    }

    fn is_optional_parameter(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsOptionalParameter,
            node,
            CheckerState::emit_is_optional_parameter,
        )
    }

    fn is_implementation_of_overload(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsImplementationOfOverload,
            node,
            CheckerState::emit_is_implementation_of_overload,
        )
    }

    fn requires_adding_implicit_undefined(
        &self,
        parameter: EmitResolverNode,
        enclosing_declaration: Option<EmitResolverNode>,
    ) -> Result<bool, EmitResolverError> {
        let method = EmitResolverMethod::RequiresAddingImplicitUndefined;
        if let Some(enclosing_declaration) = enclosing_declaration {
            self.with_resolver_node_and_location(
                method,
                parameter,
                enclosing_declaration,
                |state, parameter, enclosing_declaration| {
                    state.emit_requires_adding_implicit_undefined(
                        parameter,
                        Some(enclosing_declaration),
                    )
                },
            )
        } else {
            self.with_resolver_node(method, parameter, |state, parameter| {
                state.emit_requires_adding_implicit_undefined(parameter, None)
            })
        }
    }

    fn is_expando_function_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsExpandoFunctionDeclaration,
            node,
            CheckerState::emit_is_expando_function_declaration,
        )
    }

    fn get_properties_of_container_function(
        &self,
        node: EmitResolverNode,
    ) -> Result<Vec<EmitFunctionProperty>, EmitResolverError> {
        let method = EmitResolverMethod::GetPropertiesOfContainerFunction;
        let session_token = self.session_token;
        self.with_resolver_node(method, node, move |state, node| {
            state.emit_get_properties_of_container_function(node, session_token)
        })
    }

    fn is_literal_const_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsLiteralConstDeclaration,
            node,
            CheckerState::emit_is_literal_const_declaration,
        )
    }

    fn is_late_bound(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsLateBound,
            node,
            CheckerState::emit_is_late_bound,
        )
    }

    fn is_import_required_by_augmentation(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsImportRequiredByAugmentation,
            node,
            CheckerState::emit_is_import_required_by_augmentation,
        )
    }

    /// tsgo-port: shouldEmitFunctionProperties @7.1
    /// (transformers/declarations/util.go:155-162): the function has a body,
    /// or another function declaration of its symbol has one.
    fn should_emit_function_properties(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::ShouldEmitFunctionProperties,
            node,
            |state, node| {
                let has_body = |declaration| {
                    matches!(
                        state.data_of(declaration),
                        tsc_syntax::NodeData::FunctionDeclaration(data) if data.body.is_some()
                    )
                };
                if has_body(node) {
                    return Ok(true);
                }
                let Some(symbol) = state.node_symbol(node) else {
                    return Ok(false);
                };
                Ok(state
                    .binder
                    .symbol(symbol)
                    .declarations
                    .iter()
                    .copied()
                    .any(has_body))
            },
        )
    }

    /// tsc-port: visitDeclarationSubtree @6.0.3 (first-declaration filter)
    /// tsc-hash: 6bef4aa822019d44c58d0738e8c20b2f979f1b30a94bbb089b596794d54d19a3
    /// tsc-span: _tsc.js:114986-114988
    fn is_first_declaration_of_symbol(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsFirstDeclarationOfSymbol,
            node,
            |state, node| {
                let Some(symbol) = state.node_symbol(node) else {
                    return Ok(true);
                };
                Ok(state
                    .binder
                    .symbol(symbol)
                    .declarations
                    .first()
                    .is_none_or(|&first| first == node))
            },
        )
    }

    fn create_type_of_declaration(
        &self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        declaration: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: tsc_emitter::EmitNodeBuilderFlags,
        internal_flags: tsc_emitter::EmitInternalNodeBuilderFlags,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<Option<tsc_emitter::TransformNode>, EmitResolverError> {
        let method = EmitResolverMethod::CreateTypeOfDeclaration;
        let mut state = self.state.lock().expect("checker session state");
        validate_resolver_node(&state, method, declaration)?;
        validate_resolver_node(&state, method, enclosing_declaration)?;
        state.emit_create_type_of_declaration(
            arena,
            target,
            declaration.node(),
            enclosing_declaration.node(),
            flags,
            internal_flags,
            None,
            tracker,
        )
    }

    fn try_js_type_node_to_type_node(
        &self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        type_node: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: tsc_emitter::EmitNodeBuilderFlags,
        internal_flags: tsc_emitter::EmitInternalNodeBuilderFlags,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<Option<tsc_emitter::TransformNode>, EmitResolverError> {
        let method = EmitResolverMethod::TryJsTypeNodeToTypeNode;
        let mut state = self.state.lock().expect("checker session state");
        validate_resolver_node(&state, method, type_node)?;
        validate_resolver_node(&state, method, enclosing_declaration)?;
        crate::node_builder::try_js_type_node_to_type_node(
            &mut state,
            arena,
            target,
            type_node.node(),
            enclosing_declaration.node(),
            flags,
            internal_flags,
            tracker,
        )
    }

    fn create_return_type_of_signature_declaration(
        &self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        signature_declaration: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: tsc_emitter::EmitNodeBuilderFlags,
        internal_flags: tsc_emitter::EmitInternalNodeBuilderFlags,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<Option<tsc_emitter::TransformNode>, EmitResolverError> {
        let method = EmitResolverMethod::CreateReturnTypeOfSignatureDeclaration;
        let mut state = self.state.lock().expect("checker session state");
        validate_resolver_node(&state, method, signature_declaration)?;
        validate_resolver_node(&state, method, enclosing_declaration)?;
        state.emit_create_return_type_of_signature_declaration(
            arena,
            target,
            signature_declaration.node(),
            enclosing_declaration.node(),
            flags,
            internal_flags,
            tracker,
        )
    }

    fn create_type_of_expression(
        &self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        expression: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: tsc_emitter::EmitNodeBuilderFlags,
        internal_flags: tsc_emitter::EmitInternalNodeBuilderFlags,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<Option<tsc_emitter::TransformNode>, EmitResolverError> {
        let method = EmitResolverMethod::CreateTypeOfExpression;
        let mut state = self.state.lock().expect("checker session state");
        validate_resolver_node(&state, method, expression)?;
        validate_resolver_node(&state, method, enclosing_declaration)?;
        state.emit_create_type_of_expression(
            arena,
            target,
            expression.node(),
            enclosing_declaration.node(),
            flags,
            internal_flags,
            tracker,
        )
    }

    fn report_inference_fallback(
        &self,
        node: EmitResolverNode,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<(), EmitResolverError> {
        let method = EmitResolverMethod::ReportInferenceFallback;
        let mut state = self.state.lock().expect("checker session state");
        validate_resolver_node(&state, method, node)?;
        let mut access = crate::node_builder::StandaloneTrackerAccess {
            checker: &mut state,
            method,
        };
        tracker.report_inference_fallback(
            &mut access,
            tsc_emitter::EmitTrackerNode(u64::from(node.node().index())),
        )
    }

    fn create_literal_const_value(
        &self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        node: EmitResolverNode,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<Option<tsc_emitter::TransformNode>, EmitResolverError> {
        let method = EmitResolverMethod::CreateLiteralConstValue;
        let mut state = self.state.lock().expect("checker session state");
        validate_resolver_node(&state, method, node)?;
        state.emit_create_literal_const_value(arena, target, node.node(), tracker)
    }

    fn create_late_bound_index_signatures(
        &self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        container: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: tsc_emitter::EmitNodeBuilderFlags,
        internal_flags: tsc_emitter::EmitInternalNodeBuilderFlags,
        tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
    ) -> Result<Option<Vec<tsc_emitter::TransformNode>>, EmitResolverError> {
        let method = EmitResolverMethod::CreateLateBoundIndexSignatures;
        let mut state = self.state.lock().expect("checker session state");
        validate_resolver_node(&state, method, container)?;
        validate_resolver_node(&state, method, enclosing_declaration)?;
        state.emit_create_late_bound_index_signatures(
            arena,
            target,
            container.node(),
            enclosing_declaration.node(),
            flags,
            internal_flags,
            tracker,
        )
    }
}

impl CheckerState<'_> {
    /// tsc-port: isUniqueLocalName @6.0.3
    /// tsc-hash: 05e97318dc4eb5faf6820efe117cc4e7f19c54a7506d8d2a13db46d3fe3e8959
    /// tsc-span: _tsc.js:120668-120678
    fn emit_is_unique_local_name(
        &mut self,
        container: tsc_syntax::NodeId,
        name: &str,
    ) -> CheckResult<bool> {
        let mut current = Some(container);
        let mut seen = std::collections::BTreeSet::new();
        while let Some(node) = current {
            // A malformed container chain is a binder invariant violation.
            // Fail closed instead of accepting a colliding generated name.
            if !seen.insert(node) {
                return Ok(false);
            }
            if !self.is_node_descendant_of(node, container) {
                return Ok(true);
            }
            if let Some(symbol) = self
                .binder
                .locals_of(node)
                .and_then(|locals| locals.get(name))
                .copied()
            {
                if self.symbol_flags(symbol).intersects(
                    tsc_types::SymbolFlags::VALUE
                        | tsc_types::SymbolFlags::EXPORT_VALUE
                        | tsc_types::SymbolFlags::ALIAS,
                ) {
                    return Ok(false);
                }
            }
            current = match self.binder.next_container_of(node) {
                Ok(next) => next,
                Err(()) => return Ok(false),
            };
        }
        Ok(true)
    }

    fn emit_get_constant_value(
        &mut self,
        node: tsc_syntax::NodeId,
    ) -> CheckResult<Option<EmitConstantValue>> {
        self.get_constant_value_for_emit(node)
            .map(|value| value.map(project_constant_value))
    }

    fn emit_get_enum_member_value(
        &mut self,
        node: tsc_syntax::NodeId,
    ) -> CheckResult<Option<EmitEnumMemberValue>> {
        self.get_enum_member_value(node).map(|result| {
            Some(EmitEnumMemberValue::new(
                result.value.map(project_constant_value),
                result.is_syntactically_string,
                result.has_external_references,
            ))
        })
    }
}

fn project_constant_value(value: EvalValue) -> EmitConstantValue {
    match value {
        EvalValue::Str(value) => {
            EmitConstantValue::String(JavaScriptString::from_code_units(value.to_utf16()))
        }
        EvalValue::Num(value) => EmitConstantValue::Number(JavaScriptNumber::from_f64(value)),
    }
}

fn project_resolver_node(state: &CheckerState<'_>, node: tsc_syntax::NodeId) -> EmitResolverNode {
    let file_index = state.binder.file_index_of_node(node);
    let source = if state.authoritative_source_tokens.is_empty() {
        u32::try_from(file_index).expect("checker file index exceeds SourceFileId")
    } else {
        state
            .authoritative_source_tokens
            .get(file_index)
            .expect("authoritative metadata covers every checker file")
            .0
    };
    EmitResolverNode::from_raw_source(source, node)
}

fn validate_resolver_node(
    state: &CheckerState<'_>,
    method: EmitResolverMethod,
    node: EmitResolverNode,
) -> Result<(), EmitResolverError> {
    let source_token = AuthoritativeSourceToken(node.source().raw());
    let expected_program_index = if state.authoritative_source_index_by_token.is_empty() {
        let index = node.source().index();
        if index >= state.binder.file_count() {
            return Err(EmitResolverError::UnknownSource { method, node });
        }
        index
    } else {
        state
            .authoritative_source_index_by_token
            .get(&source_token)
            .copied()
            .ok_or(EmitResolverError::UnknownSource { method, node })?
    };
    let actual_program_index = state
        .binder
        .try_file_index_of_node(node.node())
        .ok_or(EmitResolverError::UnknownNode { method, node })?;
    if actual_program_index != expected_program_index {
        return Err(EmitResolverError::SourceNodeMismatch {
            method,
            node,
            actual_program_index,
        });
    }
    Ok(())
}

fn resolver_source_index(
    state: &CheckerState<'_>,
    method: EmitResolverMethod,
    source: tsc_program::SourceFileId,
) -> Result<usize, EmitResolverError> {
    if state.authoritative_source_index_by_token.is_empty() {
        let index = source.index();
        return (index < state.binder.file_count())
            .then_some(index)
            .ok_or(EmitResolverError::UnavailableForSource { method, source });
    }
    state
        .authoritative_source_index_by_token
        .get(&AuthoritativeSourceToken(source.raw()))
        .copied()
        .ok_or(EmitResolverError::UnavailableForSource { method, source })
}

fn validate_resolver_symbol(
    state: &CheckerState<'_>,
    session_token: u64,
    method: EmitResolverMethod,
    symbol: EmitResolverSymbol,
) -> Result<SymbolId, EmitResolverError> {
    if symbol.session_token != session_token {
        return Err(EmitResolverError::ForeignSymbol { method, symbol });
    }
    SymbolId::checked_new(symbol.symbol_index)
        .filter(|&symbol_id| state.binder.try_symbol(symbol_id).is_some())
        .ok_or(EmitResolverError::UnknownSymbol { method, symbol })
}

#[cfg(test)]
#[path = "../tests/unit/emit/tests.rs"]
mod tests;
