use std::collections::VecDeque;
use std::path::Path;

use tsc_diagnostics::{gen as d, DiagnosticMessage, RelatedInfo};
use tsc_program::SourceFileId;
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{CompilerOptions, SymbolFlags};

use crate::{
    EmitHost, EmitModuleSpecifierHost, EmitResolutionMode, EmitResolverError,
    EmitSymbolAccessibility, EmitSymbolAccessibilityResult, EmitSymbolMeaning, EmitSymbolTracker,
    EmitTrackerAccess, EmitTrackerNode, EmitTrackerNodeDescription, EmitTrackerSymbol,
    TransformError, TransformNode, TransformationContext, UnsupportedEmitFeature,
};

use super::diagnostics::{
    diagnostic_for_source_node, name_of_declaration, DiagnosticContext, DiagnosticContextPlan,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TrackerAnchor {
    Transform(TransformNode),
    Resolver(EmitTrackerNodeDescription),
}

impl TrackerAnchor {
    /// tsrs-native: callback-safe resolver-node diagnostic projection.
    pub(crate) const fn resolver(node: crate::EmitResolverNode) -> Self {
        Self::Resolver(EmitTrackerNodeDescription {
            parse: Some(node),
            original: None,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DiagnosticArgument {
    Text(String),
    NodeText(TrackerAnchor),
    DeclarationName {
        anchor: TrackerAnchor,
        anchor_is_name: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RelatedDiagnosticSpec {
    pub(crate) message: &'static DiagnosticMessage,
    pub(crate) args: Vec<DiagnosticArgument>,
    pub(crate) anchor: TrackerAnchor,
    pub(crate) only_when_anchor_parent_is_variable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiagnosticSpec {
    pub(crate) message: &'static DiagnosticMessage,
    pub(crate) args: Vec<DiagnosticArgument>,
    pub(crate) anchor: TrackerAnchor,
    pub(crate) related: Vec<RelatedDiagnosticSpec>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TrackerEffect {
    Diagnostic(DiagnosticSpec),
    Unsupported(UnsupportedEmitFeature),
    Contract(&'static str),
}

pub(crate) struct DeclarationSymbolTracker<'t> {
    options: &'t CompilerOptions,
    host: &'t dyn EmitHost,
    module_host: ModuleSpecifierHostAdapter<'t>,
    pub(crate) diagnostic_context: DiagnosticContext,
    diagnostic_plan: DiagnosticContextPlan,
    pub(crate) suppress_new_diagnostic_contexts: bool,
    pub(crate) late_marked_statements: Option<Vec<TransformNode>>,
    pub(crate) error_name_node: Option<TransformNode>,
    pub(crate) error_fallback_node: Option<TrackerAnchor>,
    pub(crate) error_fallback_stack: Vec<Option<TrackerAnchor>>,
    current_program_source: Option<SourceFileId>,
    current_transform_source: Option<crate::TransformSourceId>,
    current_source_is_js: bool,
    pending_effects: VecDeque<TrackerEffect>,
}

impl<'t> DeclarationSymbolTracker<'t> {
    /// tsrs-native: declaration symbol-tracker construction.
    pub(crate) fn new(options: &'t CompilerOptions, host: &'t dyn EmitHost) -> Self {
        Self {
            options,
            host,
            module_host: ModuleSpecifierHostAdapter { host },
            diagnostic_context: DiagnosticContext::None,
            diagnostic_plan: DiagnosticContextPlan::None,
            suppress_new_diagnostic_contexts: false,
            late_marked_statements: None,
            error_name_node: None,
            error_fallback_node: None,
            error_fallback_stack: Vec::new(),
            current_program_source: None,
            current_transform_source: None,
            current_source_is_js: false,
            pending_effects: VecDeque::new(),
        }
    }

    /// tsrs-native: tracker-owned per-file closure-state reset.
    pub(crate) fn reset_for_file(
        &mut self,
        program_source: Option<SourceFileId>,
        transform_source: crate::TransformSourceId,
        source_is_js: bool,
    ) {
        self.diagnostic_context = DiagnosticContext::None;
        self.diagnostic_plan = DiagnosticContextPlan::None;
        self.suppress_new_diagnostic_contexts = false;
        self.late_marked_statements = None;
        self.error_name_node = None;
        self.error_fallback_node = None;
        self.error_fallback_stack.clear();
        self.current_program_source = program_source;
        self.current_transform_source = Some(transform_source);
        self.current_source_is_js = source_is_js;
        self.pending_effects.clear();
    }

    /// tsrs-native: owned diagnostic-context frame replacement. Callers
    /// restore the returned value on every cleanup/error path.
    pub(crate) fn replace_diagnostic_context(
        &mut self,
        arena: &crate::TransformArena,
        context: DiagnosticContext,
    ) -> Result<(DiagnosticContext, DiagnosticContextPlan), TransformError> {
        let plan = context.plan(arena)?;
        let saved_context = std::mem::replace(&mut self.diagnostic_context, context);
        let saved_plan = std::mem::replace(&mut self.diagnostic_plan, plan);
        Ok((saved_context, saved_plan))
    }

    /// tsrs-native: owned diagnostic-context frame restoration.
    pub(crate) fn restore_diagnostic_context(
        &mut self,
        saved: (DiagnosticContext, DiagnosticContextPlan),
    ) {
        self.diagnostic_context = saved.0;
        self.diagnostic_plan = saved.1;
    }

    /// tsc-port: handleSymbolAccessibilityError @6.0.3
    /// tsc-hash: e3e03322d5eaa1dd8dcb833455758bc40478ff54d882c6b1a4a91d90cb9d308b
    /// tsc-span: _tsc.js:114336-114359
    pub(crate) fn handle_symbol_accessibility_error(
        &mut self,
        result: EmitSymbolAccessibilityResult,
    ) -> bool {
        if result.accessibility == EmitSymbolAccessibility::Accessible {
            if let Some(aliases) = result.aliases_to_make_visible {
                let Some(transform_source) = self.current_transform_source else {
                    self.pending_effects.push_back(TrackerEffect::Contract(
                        "late visibility aliases arrived before tracker source reset",
                    ));
                    return false;
                };
                let mut transformed = Vec::with_capacity(aliases.len());
                for alias in aliases {
                    if Some(alias.source()) != self.current_program_source {
                        self.pending_effects.push_back(TrackerEffect::Contract(
                            "late visibility alias belongs to another source",
                        ));
                        return false;
                    }
                    transformed.push(TransformNode::new(transform_source, alias.node()));
                }
                if self.late_marked_statements.is_none() {
                    // First assignment is verbatim: order and duplicates are
                    // observable and intentionally retained.
                    self.late_marked_statements = Some(transformed);
                } else if let Some(marked) = self.late_marked_statements.as_mut() {
                    for alias in transformed {
                        if !marked.contains(&alias) {
                            marked.push(alias);
                        }
                    }
                }
            }
            return false;
        }
        if result.accessibility == EmitSymbolAccessibility::NotResolved {
            return false;
        }
        match self.diagnostic_plan.resolve(self.host, &result) {
            Ok(Some(spec)) => {
                self.pending_effects
                    .push_back(TrackerEffect::Diagnostic(spec));
                true
            }
            Ok(None) => false,
            Err(_) => {
                self.pending_effects.push_back(TrackerEffect::Contract(
                    "diagnostic emitted without a declaration diagnostic context",
                ));
                false
            }
        }
    }

    /// tsc-port: reportExpandoFunctionErrors @6.0.3
    /// tsc-hash: ad70a87485461e78fb80c07131d4df898fcc9f49f901be20c3abbc2d9dc0603e
    /// tsc-span: _tsc.js:114316-114326
    pub(crate) fn report_expando_function_errors(&mut self) {
        self.pending_effects.push_back(TrackerEffect::Unsupported(
            UnsupportedEmitFeature::IsolatedDeclarations,
        ));
    }

    /// tsc-port: errorDeclarationNameWithFallback @6.0.3
    /// tsc-hash: 4bf6dc273bdda078539a812ee455228865fbffb7a0d51f6dd819951162293549
    /// tsc-span: _tsc.js:114381-114383
    fn error_declaration_name_with_fallback(&self) -> Option<DiagnosticArgument> {
        self.error_name_node
            .map(|node| DiagnosticArgument::DeclarationName {
                anchor: TrackerAnchor::Transform(node),
                anchor_is_name: true,
            })
            .or_else(|| {
                self.error_fallback_node
                    .clone()
                    .map(|anchor| DiagnosticArgument::DeclarationName {
                        anchor,
                        anchor_is_name: false,
                    })
            })
    }

    fn current_error_anchor(&self) -> Option<TrackerAnchor> {
        self.error_name_node
            .map(TrackerAnchor::Transform)
            .or_else(|| self.error_fallback_node.clone())
    }

    fn report_at_current_anchor(
        &mut self,
        message: &'static DiagnosticMessage,
        mut args: Vec<DiagnosticArgument>,
    ) {
        let Some(anchor) = self.current_error_anchor() else {
            return;
        };
        if let Some(name) = self.error_declaration_name_with_fallback() {
            args.insert(0, name);
        }
        self.pending_effects
            .push_back(TrackerEffect::Diagnostic(DiagnosticSpec {
                message,
                args,
                anchor,
                related: Vec::new(),
            }));
    }

    /// tsrs-native: FIFO transfer from callback-owned effects to the
    /// transformer materialization boundary.
    pub(crate) fn take_pending_effects(&mut self) -> VecDeque<TrackerEffect> {
        std::mem::take(&mut self.pending_effects)
    }
}

impl EmitSymbolTracker for DeclarationSymbolTracker<'_> {
    fn can_track_symbol(&self) -> bool {
        true
    }

    /// tsc-port: trackSymbol @6.0.3
    /// tsc-hash: d64605e9e90a69fc35680689c16e2c076a85f20902bf78ac699284e7189c7f85
    /// tsc-span: _tsc.js:114360-114370
    fn track_symbol(
        &mut self,
        access: &mut dyn EmitTrackerAccess,
        symbol: EmitTrackerSymbol,
        symbol_flags: SymbolFlags,
        enclosing_declaration: Option<EmitTrackerNode>,
        meaning: EmitSymbolMeaning,
    ) -> Result<bool, EmitResolverError> {
        if symbol_flags.contains(SymbolFlags::TYPE_PARAMETER) {
            return Ok(false);
        }
        let result = access.is_symbol_accessible(
            symbol,
            enclosing_declaration,
            meaning,
            /* should_compute_aliases */ true,
        )?;
        Ok(self.handle_symbol_accessibility_error(result))
    }

    /// tsc-port: reportInferenceFallback @6.0.3
    /// tsc-hash: f81bf9e236fba1289977baf167ed46ac103cbe6ade0aa59b90f044a5a4f6910a
    /// tsc-span: _tsc.js:114327-114335
    fn report_inference_fallback(
        &mut self,
        access: &mut dyn EmitTrackerAccess,
        node: EmitTrackerNode,
    ) -> Result<(), EmitResolverError> {
        if self.options.isolated_declarations != Some(true) || self.current_source_is_js {
            return Ok(());
        }
        let description = access.describe_node(node);
        let node_source = description
            .parse
            .or(description.original)
            .map(crate::EmitResolverNode::source);
        if node_source != self.current_program_source {
            return Ok(());
        }
        self.pending_effects.push_back(TrackerEffect::Unsupported(
            UnsupportedEmitFeature::IsolatedDeclarations,
        ));
        Ok(())
    }

    /// tsc-port: reportPrivateInBaseOfClassExpression @6.0.3
    /// tsc-hash: 507c8b075bab67d7e5288b77f7f6847880c83a6ae134508088ae4ab4ae156ec1
    /// tsc-span: _tsc.js:114371-114380
    fn report_private_in_base_of_class_expression(&mut self, property_name: &str) {
        let Some(anchor) = self.current_error_anchor() else {
            return;
        };
        let Some(name) = self.error_declaration_name_with_fallback() else {
            return;
        };
        self.pending_effects
            .push_back(TrackerEffect::Diagnostic(DiagnosticSpec {
                message:
                    &d::Property_0_of_exported_anonymous_class_type_may_not_be_private_or_protected,
                args: vec![DiagnosticArgument::Text(property_name.to_owned())],
                anchor: anchor.clone(),
                related: vec![RelatedDiagnosticSpec {
                    message: &d::Add_a_type_annotation_to_the_variable_0,
                    args: vec![name],
                    anchor,
                    only_when_anchor_parent_is_variable: true,
                }],
            }));
    }

    /// tsc-port: reportInaccessibleUniqueSymbolError @6.0.3
    /// tsc-hash: 636f484e2cf10c53f40e989f92273b01ffc440402aa2eb895b1a034dafd0e771
    /// tsc-span: _tsc.js:114384-114388
    fn report_inaccessible_unique_symbol_error(&mut self) {
        self.report_at_current_anchor(
            &d::The_inferred_type_of_0_references_an_inaccessible_1_type_A_type_annotation_is_necessary,
            vec![DiagnosticArgument::Text("unique symbol".to_owned())],
        );
    }

    /// tsc-port: reportCyclicStructureError @6.0.3
    /// tsc-hash: 14cc9702ad59b7ac3941bbb1aaa459e6518a900952e842c46ad4cd9c0481027b
    /// tsc-span: _tsc.js:114389-114393
    fn report_cyclic_structure_error(&mut self) {
        self.report_at_current_anchor(
            &d::The_inferred_type_of_0_references_a_type_with_a_cyclic_structure_which_cannot_be_trivially_serialized_A_type_annotation_is_necessary,
            Vec::new(),
        );
    }

    /// tsc-port: reportInaccessibleThisError @6.0.3
    /// tsc-hash: d2c22ea74a4bbb193626231cdb4968409526cdb5a03fbe00a9ed3717552143ec
    /// tsc-span: _tsc.js:114394-114398
    fn report_inaccessible_this_error(&mut self) {
        self.report_at_current_anchor(
            &d::The_inferred_type_of_0_references_an_inaccessible_1_type_A_type_annotation_is_necessary,
            vec![DiagnosticArgument::Text("this".to_owned())],
        );
    }

    /// tsc-port: reportLikelyUnsafeImportRequiredError @6.0.3
    /// tsc-hash: 6ef70ef33a0f79987bb9754d6c334c7185f4ac92159b2f30f056964a63656e7b
    /// tsc-span: _tsc.js:114399-114407
    fn report_likely_unsafe_import_required_error(
        &mut self,
        specifier: &str,
        symbol_name: Option<&str>,
    ) {
        if let Some(symbol_name) = symbol_name {
            self.report_at_current_anchor(
                &d::The_inferred_type_of_0_cannot_be_named_without_a_reference_to_2_from_1_This_is_likely_not_portable_A_type_annotation_is_necessary,
                vec![
                    DiagnosticArgument::Text(specifier.to_owned()),
                    DiagnosticArgument::Text(symbol_name.to_owned()),
                ],
            );
        } else {
            self.report_at_current_anchor(
                &d::The_inferred_type_of_0_cannot_be_named_without_a_reference_to_1_This_is_likely_not_portable_A_type_annotation_is_necessary,
                vec![DiagnosticArgument::Text(specifier.to_owned())],
            );
        }
    }

    /// tsc-port: reportTruncationError @6.0.3
    /// tsc-hash: da26edf672e1d46e1d793f23bdb9a4147a9c2305cf5a6ea6384a7bb9b044aa01
    /// tsc-span: _tsc.js:114408-114412
    fn report_truncation_error(&mut self) {
        let Some(anchor) = self.current_error_anchor() else {
            return;
        };
        self.pending_effects
            .push_back(TrackerEffect::Diagnostic(DiagnosticSpec {
                message: &d::The_inferred_type_of_this_node_exceeds_the_maximum_length_the_compiler_will_serialize_An_explicit_type_annotation_is_needed,
                args: Vec::new(),
                anchor,
                related: Vec::new(),
            }));
    }

    /// tsc-port: reportNonlocalAugmentation @6.0.3
    /// tsc-hash: 087a8e3b3f966c3356348f688be63ce7db15cc3179310de23a29fe56669d95fe
    /// tsc-span: _tsc.js:114413-114425
    fn report_nonlocal_augmentation(
        &mut self,
        primary_declaration: Option<EmitTrackerNodeDescription>,
        augmenting_declarations: Vec<EmitTrackerNodeDescription>,
    ) {
        let Some(primary) = primary_declaration else {
            return;
        };
        for augmentation in augmenting_declarations {
            self.pending_effects
                .push_back(TrackerEffect::Diagnostic(DiagnosticSpec {
                    message: &d::Declaration_augments_declaration_in_another_file_This_cannot_be_serialized,
                    args: Vec::new(),
                    anchor: TrackerAnchor::Resolver(augmentation),
                    related: vec![RelatedDiagnosticSpec {
                        message: &d::This_is_the_declaration_being_augmented_Consider_moving_the_augmenting_declaration_into_the_same_file,
                        args: Vec::new(),
                        anchor: TrackerAnchor::Resolver(primary),
                        only_when_anchor_parent_is_variable: false,
                    }],
                }));
        }
    }

    /// tsc-port: reportNonSerializableProperty @6.0.3
    /// tsc-hash: eec5bea72529fcb719dc83610b55061075d7969c74aad2e479de13ccf64e7474
    /// tsc-span: _tsc.js:114426-114430
    fn report_non_serializable_property(&mut self, property_name: &str) {
        let Some(anchor) = self.current_error_anchor() else {
            return;
        };
        self.pending_effects
            .push_back(TrackerEffect::Diagnostic(DiagnosticSpec {
                message: &d::The_type_of_this_node_cannot_be_serialized_because_its_property_0_cannot_be_serialized,
                args: vec![DiagnosticArgument::Text(property_name.to_owned())],
                anchor,
                related: Vec::new(),
            }));
    }

    /// tsc-port: pushErrorFallbackNode @6.0.3
    /// tsc-hash: 3c76d7c1df2f8bb11f48c5d1fc17a8aa1f4045e220624ba13bef2eca2702f409
    /// tsc-span: _tsc.js:114292-114300
    fn push_error_fallback_node(&mut self, node: Option<EmitTrackerNodeDescription>) {
        self.error_fallback_stack
            .push(self.error_fallback_node.take());
        self.error_fallback_node = node.map(TrackerAnchor::Resolver);
    }

    /// tsc-port: popErrorFallbackNode @6.0.3
    /// tsc-hash: 06f2e7a8c8107b794f5bf181df9a8ce09b1abe9d3daaaba92dab2593036d18c9
    /// tsc-span: _tsc.js:114301-114303
    fn pop_error_fallback_node(&mut self) {
        if let Some(saved) = self.error_fallback_stack.pop() {
            self.error_fallback_node = saved;
        }
    }

    fn module_specifier_host(&self) -> Option<&dyn EmitModuleSpecifierHost> {
        Some(&self.module_host)
    }
}

struct ModuleSpecifierHostAdapter<'t> {
    host: &'t dyn EmitHost,
}

impl EmitModuleSpecifierHost for ModuleSpecifierHostAdapter<'_> {
    fn get_current_directory(&self) -> String {
        self.host.current_directory().to_string_lossy().into_owned()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.host.use_case_sensitive_file_names()
    }

    fn file_exists(&self, file_name: &str) -> bool {
        self.find_source(file_name).is_some()
    }

    fn read_file(&self, file_name: &str) -> Option<String> {
        self.find_source(file_name)
            .and_then(crate::EmitSource::syntax)
            .map(|source| source.text().to_owned())
    }

    fn get_common_source_directory(&self) -> String {
        self.host
            .common_source_directory()
            .to_string_lossy()
            .into_owned()
    }

    fn get_default_resolution_mode_for_file(
        &self,
        file: crate::EmitResolverNode,
    ) -> EmitResolutionMode {
        match self.host.get_emit_module_format_of_file(file.source()) {
            Some(1) => EmitResolutionMode::CommonJs,
            Some(99) => EmitResolutionMode::EsNext,
            _ => EmitResolutionMode::None,
        }
    }

    fn get_mode_for_resolution_at_index(
        &self,
        _file: crate::EmitResolverNode,
        _index: u32,
    ) -> EmitResolutionMode {
        EmitResolutionMode::None
    }

    fn symlinked_directories(&self) -> Vec<(String, String)> {
        self.host.symlinked_directories()
    }

    fn symlinked_files(&self) -> Vec<(String, String)> {
        self.host.symlinked_files()
    }
}

impl ModuleSpecifierHostAdapter<'_> {
    fn find_source(&self, file_name: &str) -> Option<crate::EmitSource<'_>> {
        let wanted = self.host.canonical_output_path(Path::new(file_name));
        self.host.source_file_ids().iter().find_map(|&source| {
            let candidate = self.host.source_file(source)?;
            (self.host.canonical_output_path(candidate.path()) == wanted).then_some(candidate)
        })
    }
}

/// tsrs-native: callback-effect materialization at the declaration
/// transformer/resolver boundary (h2-7a-m-4 §5.3).
pub(crate) fn materialize_effects(
    cx: &mut TransformationContext,
    host: &dyn EmitHost,
    effects: VecDeque<TrackerEffect>,
) -> Result<(), TransformError> {
    for effect in effects {
        match effect {
            TrackerEffect::Unsupported(feature) => {
                return Err(TransformError::Unsupported(feature))
            }
            TrackerEffect::Contract(detail) => {
                return Err(TransformError::UnsupportedCompilerOption {
                    option: "declaration transformer contract",
                    detail,
                })
            }
            TrackerEffect::Diagnostic(spec) => {
                let args = materialize_arguments(cx, host, &spec.args)?;
                let mut diagnostic = with_anchor(cx, host, &spec.anchor, |source, node| {
                    diagnostic_for_source_node(source, node, spec.message, &args)
                })?;
                for related in spec.related {
                    if related.only_when_anchor_parent_is_variable
                        && !with_anchor(cx, host, &related.anchor, |source, node| {
                            source.arena.node(node).parent.is_some_and(|parent| {
                                source.arena.node(parent).kind == SyntaxKind::VariableDeclaration
                            })
                        })?
                    {
                        continue;
                    }
                    let related_args = materialize_arguments(cx, host, &related.args)?;
                    let related_diagnostic =
                        with_anchor(cx, host, &related.anchor, |source, node| {
                            diagnostic_for_source_node(source, node, related.message, &related_args)
                        })?;
                    diagnostic.related_information_present = true;
                    diagnostic.related.push(RelatedInfo {
                        file_name: related_diagnostic.file_name,
                        start: related_diagnostic.start,
                        length: related_diagnostic.length,
                        message: related_diagnostic.message,
                    });
                }
                cx.add_diagnostic(diagnostic)?;
            }
        }
    }
    Ok(())
}

fn materialize_arguments(
    cx: &TransformationContext,
    host: &dyn EmitHost,
    args: &[DiagnosticArgument],
) -> Result<Vec<String>, TransformError> {
    args.iter()
        .map(|arg| match arg {
            DiagnosticArgument::Text(text) => Ok(text.clone()),
            DiagnosticArgument::NodeText(anchor) => with_anchor(cx, host, anchor, text_of_node),
            DiagnosticArgument::DeclarationName {
                anchor,
                anchor_is_name,
            } => with_anchor(cx, host, anchor, |source, node| {
                declaration_name_to_string(source, node, *anchor_is_name)
            }),
        })
        .collect()
}

fn with_anchor<R>(
    cx: &TransformationContext,
    host: &dyn EmitHost,
    anchor: &TrackerAnchor,
    body: impl FnOnce(&SourceFile, NodeId) -> R,
) -> Result<R, TransformError> {
    match anchor {
        TrackerAnchor::Transform(node) => {
            let node = cx
                .arena()
                .parse_tree_node(*node)?
                .ok_or(TransformError::ResolverNodeNotInParseTree(*node))?;
            let source = cx.arena().source(node.source())?.syntax();
            Ok(body(source, node.node()))
        }
        TrackerAnchor::Resolver(description) => {
            let resolver_node = description.parse.or(description.original).ok_or(
                TransformError::UnsupportedCompilerOption {
                    option: "declaration diagnostic anchor",
                    detail: "resolver callback supplied no parse or original projection",
                },
            )?;
            let source = host.source_file(resolver_node.source()).ok_or(
                TransformError::UnsupportedCompilerOption {
                    option: "declaration diagnostic anchor",
                    detail: "resolver callback source is absent from the emit host",
                },
            )?;
            let syntax = source
                .syntax()
                .ok_or(TransformError::UnsupportedCompilerOption {
                    option: "declaration diagnostic anchor",
                    detail: "resolver callback source has no checked syntax",
                })?;
            if !syntax.arena.contains_node(resolver_node.node()) {
                return Err(TransformError::UnsupportedCompilerOption {
                    option: "declaration diagnostic anchor",
                    detail: "resolver callback node is absent from the checked syntax",
                });
            }
            Ok(body(syntax, resolver_node.node()))
        }
    }
}

fn text_of_node(source: &SourceFile, node: NodeId) -> String {
    let node = source.arena.node(node);
    if let NodeData::Identifier(identifier) = &node.data {
        return identifier.text.clone();
    }
    let start = tsc_syntax::skip_trivia(source.text(), node.pos as usize);
    source
        .text()
        .get(start..node.end as usize)
        .unwrap_or_default()
        .to_owned()
}

fn declaration_name_to_string(source: &SourceFile, node: NodeId, node_is_name: bool) -> String {
    let name = if node_is_name {
        Some(node)
    } else {
        name_of_declaration(source, node)
    };
    let Some(name) = name else {
        if let NodeData::ExportAssignment(export) = &source.arena.node(node).data {
            return if export.is_export_equals == Some(true) {
                "export=".to_owned()
            } else {
                "default".to_owned()
            };
        }
        return "(Missing)".to_owned();
    };
    let name_node = source.arena.node(name);
    if name_node.end == name_node.pos {
        return "(Missing)".to_owned();
    }
    text_of_node(source, name)
}
