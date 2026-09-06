//! M4 5.7a/b: call resolution — the core call/new band plus the
//! tagged-template/import/instanceof tail (m4-57-call-extraction.md;
//! the JSX band is 5.7c).
//!
//! Error rendering follows tsc's diagnostic chains, including
//! elaboration for object/array literals and function bodies.

use tsc_binder::{node_util, SymbolId, SymbolTable};
use tsc_diagnostics::{
    gen as diagnostics, Diagnostic, DiagnosticMessage, MessageChain, RelatedInfo,
};
use tsc_syntax::nodes::{JsxOpeningElementData, JsxSelfClosingElementData};
use tsc_syntax::{NodeArrayId, NodeData, NodeId, SyntaxKind};
use tsc_types::{
    CheckMode, ContextFlags, ElementFlags, InferenceFlags, InferencePriority, IntersectionFlags,
    ModifierFlags, NodeFlags, ObjectFlags, SignatureFlags, SymbolFlags, TypeData, TypeFlags,
    TypeId, UnionReduction,
};

use crate::elaboration::ElaborationDiagnosticSink;
use crate::inference::InferenceContextId;
use crate::relate::RelationKind;

use crate::links::LinkSlot;
use crate::operators::OuterExpressionKinds;
use crate::speculate::SpeculationOutcome;
use crate::state::{CheckResult, CheckerState, Signature, SignatureId};
use crate::structural::SignatureKind;

/// The Rust stand-in for tsc's fabricated SyntheticExpression parse
/// nodes (createSyntheticExpression 76289): getEffectiveCallArguments
/// carries these instead of appending arena nodes. `pos`/`end` are the
/// byte range of the originating node (setTextRange semantics);
/// consumers are checkSyntheticExpression (73946), isSpreadArgument,
/// arity, applicability, spans, and the contextual indexOf.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum EffectiveArg {
    Node(NodeId),
    Synthetic {
        pos: u32,
        end: u32,
        ty: TypeId,
        is_spread: bool,
        tuple_name_source: Option<NodeId>,
    },
}

/// A resolved diagnostic location (file + UTF-16 start/length) carried
/// beside the applicability diagnostic while overload selection
/// chooses and combines its rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiagSpan {
    pub(crate) file_name: String,
    pub(crate) start: u32,
    pub(crate) length: u32,
}

/// One applicability failure: the span of the diagnostic tsc would
/// create, its related rows, and (in Report mode) the fully built
/// head diagnostic.
struct ApplicabilityError {
    span: DiagSpan,
    related: Vec<RelatedInfo>,
    diagnostic: Option<Diagnostic>,
}

/// getSignatureApplicabilityError run modes: Silent is the selection
/// pass (verdicts only, errorOutputContainer.skipLogging semantics);
/// Report builds the head diagnostics (display escapes allowed).
#[derive(Clone, Copy, Eq, PartialEq)]
enum ApplicabilityMode {
    Silent,
    Report,
}

/// resolveCall's closure state: the three error-candidate slots plus
/// the shared argCheckMode (mutated across BOTH chooseOverload passes,
/// 76590/76612/76841).
struct ResolveCallCtx {
    node: NodeId,
    args: Vec<EffectiveArg>,
    type_arguments_array: Option<NodeArrayId>,
    type_argument_nodes: Vec<NodeId>,
    arg_check_mode: CheckMode,
    candidates: Vec<SignatureId>,
    candidates_for_argument_error: Option<Vec<SignatureId>>,
    candidate_for_argument_arity_error: Option<SignatureId>,
    candidate_for_type_argument_error: Option<SignatureId>,
}

/// tsrs-native: the value carried out of one rollback-capable
/// chooseOverload candidate transaction. Inference arenas are
/// deliberately E-class and survive rollback.
enum OverloadCandidateDisposition {
    TypeArgumentError(SignatureId),
    ArgumentArityError(SignatureId),
    ArgumentError(SignatureId),
    Success(SignatureId),
}

struct OverloadCandidateTrial {
    disposition: OverloadCandidateDisposition,
    arg_check_mode: CheckMode,
}

/// skipTrivia(text, pos, stopAfterLineBreak=true) followed by tsc's
/// `isLineBreak(text.charCodeAt(result - 1))` (77025-77031): true when
/// a line break separates the callee from its single argument.
fn line_break_precedes_next_token(text: &str, start: usize) -> bool {
    let mut pos = start;
    loop {
        let Some(ch) = text[pos..].chars().next() else {
            return false;
        };
        match ch {
            '\u{000A}' | '\u{000D}' | '\u{2028}' | '\u{2029}' => return true,
            c if c.is_whitespace() => pos += c.len_utf8(),
            '/' => {
                let rest = &text[pos..];
                if rest.starts_with("//") {
                    let mut cursor = pos + 2;
                    while let Some(c) = text[cursor..].chars().next() {
                        if matches!(c, '\u{000A}' | '\u{000D}' | '\u{2028}' | '\u{2029}') {
                            break;
                        }
                        cursor += c.len_utf8();
                    }
                    pos = cursor;
                } else if let Some(block) = rest.strip_prefix("/*") {
                    match block.find("*/") {
                        Some(offset) => pos += 2 + offset + 2,
                        None => return false,
                    }
                } else {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

impl<'a> CheckerState<'a> {
    // ---- §10 decorators band (m4-58; DUAL MODE:
    // legacy_decorators == options.experimental_decorators) ----

    /// tsc-port: checkDecorators @6.0.3
    /// tsc-hash: c9d5b1fd8dd418487134d131d2a23dc1575b2a5e9105dc7bf2d1278dd45ece84
    /// tsc-span: _tsc.js:82744-82783
    ///
    /// markLinkedReferences is declaration-emit bookkeeping. The
    /// importHelpers probe is semantic and verifies the resolved
    /// helper module before decorator checking.
    pub(crate) fn check_decorators(&mut self, node: NodeId) -> CheckResult<()> {
        if !crate::js_grammar::can_have_decorators(self.kind_of(node)) {
            return Ok(());
        }
        let source = self.binder.source_of_node(node);
        let Some(modifiers) = node_util::modifiers_of(source, node) else {
            return Ok(());
        };
        let modifiers = self.nodes_of(Some(modifiers));
        let first_decorator = modifiers
            .iter()
            .copied()
            .find(|&modifier| self.kind_of(modifier) == SyntaxKind::Decorator);
        let Some(first_decorator) = first_decorator else {
            return Ok(());
        };
        let parent = self.parent_of(node);
        let grandparent = parent.and_then(|parent| self.parent_of(parent));
        if !self.node_can_be_decorated(
            self.options.experimental_decorators,
            node,
            parent,
            grandparent,
        ) {
            return Ok(());
        }
        if self.options.experimental_decorators
            || self.options.emit_script_target() < tsc_types::ScriptTarget::ES_NEXT
        {
            self.check_external_emit_helpers(
                first_decorator,
                crate::modules::EMIT_HELPER_DECORATE,
            )?;
        }
        if !self.options.experimental_decorators
            && self.options.emit_script_target() < tsc_types::ScriptTarget::ES_NEXT
        {
            if self.kind_of(node) == SyntaxKind::ClassDeclaration {
                let (name, members) = match self.data_of(node) {
                    NodeData::ClassDeclaration(data) => (data.name, data.members),
                    _ => (None, None),
                };
                let needs_set_function_name = name.is_none()
                    || self.nodes_of(members).into_iter().any(|member| {
                        self.is_static_element(member)
                            && (self.kind_of(member) == SyntaxKind::ClassStaticBlockDeclaration
                                || self.name_of_node(member).is_some_and(|name| {
                                    self.kind_of(name) == SyntaxKind::PrivateIdentifier
                                })
                                || node_util::modifiers_of(
                                    self.binder.source_of_node(member),
                                    member,
                                )
                                .is_some_and(|modifiers| {
                                    self.nodes_of(Some(modifiers)).into_iter().any(|modifier| {
                                        self.kind_of(modifier) == SyntaxKind::Decorator
                                    })
                                }))
                    });
                if needs_set_function_name {
                    self.check_external_emit_helpers(
                        first_decorator,
                        crate::modules::EMIT_HELPER_SET_FUNCTION_NAME,
                    )?;
                }
            } else if self.kind_of(node) != SyntaxKind::ClassExpression {
                let name = self.name_of_node(node);
                let private_named_callable = name
                    .is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier)
                    && (matches!(
                        self.kind_of(node),
                        SyntaxKind::MethodDeclaration
                            | SyntaxKind::GetAccessor
                            | SyntaxKind::SetAccessor
                    ) || node_util::is_auto_accessor_property_declaration(
                        self.binder.source_of_node(node),
                        node,
                    ));
                if private_named_callable {
                    self.check_external_emit_helpers(
                        first_decorator,
                        crate::modules::EMIT_HELPER_SET_FUNCTION_NAME,
                    )?;
                }
                if name.is_some_and(|name| self.kind_of(name) == SyntaxKind::ComputedPropertyName) {
                    self.check_external_emit_helpers(
                        first_decorator,
                        crate::modules::EMIT_HELPER_PROP_KEY,
                    )?;
                }
            }
        }
        if self.options.emit_decorator_metadata == Some(true) {
            self.mark_decorator_metadata_aliases(node)?;
        }
        for modifier in modifiers {
            if self.kind_of(modifier) == SyntaxKind::Decorator {
                self.check_decorator(modifier)?;
            }
        }
        Ok(())
    }

    /// tsc-port: markDecoratorAliasReferenced @6.0.3
    /// tsc-hash: 3a68f792b0da68a120622558271469120ce40552a0547c6c3a40af09ea035a51
    /// tsc-span: _tsc.js:71867-71908
    ///
    /// Decorator metadata is emitted after TypeScript syntax erasure. The
    /// checker therefore records when a type-syntax use is also a runtime
    /// alias use, and import elision consumes that durable fact later.
    fn mark_decorator_metadata_aliases(&mut self, node: NodeId) -> CheckResult<()> {
        match self.data_of(node).clone() {
            NodeData::ClassDeclaration(data) => {
                let constructor = self.nodes_of(data.members).into_iter().find(|&member| {
                    matches!(
                        self.data_of(member),
                        NodeData::Constructor(data) if data.body.is_some()
                    )
                });
                if let Some(constructor) = constructor {
                    for parameter in self.function_like_parameters(constructor) {
                        let r#type = self.parameter_type_node_for_decorator_metadata(parameter);
                        self.mark_decorator_metadata_type_node_as_referenced(r#type)?;
                    }
                }
            }
            NodeData::GetAccessor(_) | NodeData::SetAccessor(_) => {
                let symbol = self.get_symbol_of_declaration(node)?;
                let other_kind = if self.kind_of(node) == SyntaxKind::GetAccessor {
                    SyntaxKind::SetAccessor
                } else {
                    SyntaxKind::GetAccessor
                };
                let other = self.get_declaration_of_kind(symbol, other_kind);
                let r#type = self
                    .annotated_accessor_type_node(Some(node))
                    .or_else(|| self.annotated_accessor_type_node(other));
                self.mark_decorator_metadata_type_node_as_referenced(r#type)?;
            }
            NodeData::MethodDeclaration(data) => {
                for parameter in self.nodes_of(data.parameters) {
                    let r#type = self.parameter_type_node_for_decorator_metadata(parameter);
                    self.mark_decorator_metadata_type_node_as_referenced(r#type)?;
                }
                let return_type = self.effective_return_type_node(node);
                self.mark_decorator_metadata_type_node_as_referenced(return_type)?;
            }
            NodeData::PropertyDeclaration(_) => {
                let r#type = self.effective_type_annotation_node(node);
                self.mark_decorator_metadata_type_node_as_referenced(r#type)?;
            }
            NodeData::Parameter(_) => {
                let r#type = self.parameter_type_node_for_decorator_metadata(node);
                self.mark_decorator_metadata_type_node_as_referenced(r#type)?;
                if let Some(signature) = self.parent_of(node) {
                    for parameter in self.function_like_parameters(signature) {
                        let r#type = self.parameter_type_node_for_decorator_metadata(parameter);
                        self.mark_decorator_metadata_type_node_as_referenced(r#type)?;
                    }
                    let return_type = self.effective_return_type_node(signature);
                    self.mark_decorator_metadata_type_node_as_referenced(return_type)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn function_like_parameters(&self, node: NodeId) -> Vec<NodeId> {
        let parameters = match self.data_of(node) {
            NodeData::Constructor(data) => data.parameters,
            NodeData::FunctionDeclaration(data) => data.parameters,
            NodeData::FunctionExpression(data) => data.parameters,
            NodeData::ArrowFunction(data) => data.parameters,
            NodeData::MethodDeclaration(data) => data.parameters,
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            _ => None,
        };
        self.nodes_of(parameters)
    }

    /// tsc-port: getParameterTypeNodeForDecoratorCheck @6.0.3
    /// tsc-hash: 1d9427a617814c2a84781a8d159888be1e22e7a8676bc1a173485f4a954763b1
    /// tsc-span: _tsc.js:82740-82743
    fn parameter_type_node_for_decorator_metadata(&self, parameter: NodeId) -> Option<NodeId> {
        let r#type = self.effective_type_annotation_node(parameter)?;
        if !self.is_rest_parameter_declaration(parameter) {
            return Some(r#type);
        }
        match self.data_of(r#type) {
            NodeData::ArrayType(data) => data.element_type,
            NodeData::TypeReference(data) => {
                let arguments = self.nodes_of(data.type_arguments);
                (arguments.len() == 1).then_some(arguments[0])
            }
            _ => None,
        }
    }

    /// tsc-port: markDecoratorMedataDataTypeNodeAsReferenced @6.0.3
    /// tsc-hash: 7424345a75a04f0186f51f2b0629f81e5711a22c586884d2e2c6d12fbe32ec11
    /// tsc-span: _tsc.js:71991-72000
    fn mark_decorator_metadata_type_node_as_referenced(
        &mut self,
        r#type: Option<NodeId>,
    ) -> CheckResult<()> {
        let Some(entity_name) = self.decorator_metadata_entity_name(r#type) else {
            return Ok(());
        };
        if !matches!(
            self.kind_of(entity_name),
            SyntaxKind::Identifier | SyntaxKind::QualifiedName
        ) {
            return Ok(());
        }
        self.mark_entity_name_or_entity_expression_as_reference(entity_name, true)
    }

    /// tsc-port: markEntityNameOrEntityExpressionAsReference @6.0.3
    /// tsc-hash: 2d7fc56d57e3ddbdf31eecd35c1fb485d6f0d2f83ec1af49e7638c5eb895b980
    /// tsc-span: _tsc.js:71959-71983
    ///
    /// Resolve the root in its type/namespace meaning first. Calling the
    /// ordinary expression resolver here would incorrectly diagnose local
    /// type aliases as value uses before metadata serialization has selected
    /// its runtime fallback.
    fn mark_entity_name_or_entity_expression_as_reference(
        &mut self,
        type_name: NodeId,
        for_decorator_metadata: bool,
    ) -> CheckResult<()> {
        let root_name = self.first_identifier(type_name);
        let meaning = if self.kind_of(type_name) == SyntaxKind::Identifier {
            SymbolFlags::TYPE
        } else {
            SymbolFlags::NAMESPACE
        } | SymbolFlags::ALIAS;
        let name = self
            .identifier_text(root_name)
            .unwrap_or_default()
            .to_owned();
        let Some(root_symbol) = self.resolve_name(
            Some(root_name),
            &name,
            meaning,
            /*name_not_found_message*/ None,
            /*is_use*/ true,
            /*exclude_globals*/ false,
        )?
        else {
            return Ok(());
        };
        if !self
            .symbol_flags(root_symbol)
            .intersects(SymbolFlags::ALIAS)
        {
            return Ok(());
        }

        let can_collect_alias_accessibility = self.options.verbatim_module_syntax != Some(true);
        let symbol_is_value =
            self.symbol_is_value(root_symbol, /*include_type_only_members*/ false)?;
        let target = self.resolve_alias(root_symbol)?;
        if can_collect_alias_accessibility
            && symbol_is_value
            && !self.is_const_enum_or_const_enum_only_module_symbol(target)
            && self.get_type_only_alias_declaration(root_symbol)?.is_none()
        {
            return self.mark_alias_symbol_as_referenced(root_symbol);
        }

        let isolated_modules = self.options.isolated_modules == Some(true)
            || self.options.verbatim_module_syntax == Some(true);
        let has_explicit_type_only_declaration = self
            .binder
            .symbol(root_symbol)
            .declarations
            .iter()
            .copied()
            .any(|declaration| self.is_type_only_import_or_export_declaration(declaration));
        if for_decorator_metadata
            && isolated_modules
            && self.options.emit_module_kind() >= 5
            && !symbol_is_value
            && !has_explicit_type_only_declaration
        {
            let related = self
                .binder
                .symbol(root_symbol)
                .declarations
                .iter()
                .copied()
                .find(|declaration| self.is_alias_symbol_declaration(*declaration))
                .map(|declaration| {
                    self.related_info_for_node(
                        declaration,
                        &diagnostics::_0_was_imported_here,
                        &[&name],
                    )
                })
                .into_iter()
                .collect();
            self.error_at_with_related(
                Some(type_name),
                &diagnostics::A_type_referenced_in_a_decorated_signature_must_be_imported_with_import_type_or_a_namespace_import_when_isolatedModules_and_emitDecoratorMetadata_are_enabled,
                &[],
                related,
            );
        }
        Ok(())
    }

    /// tsc-port: getEntityNameForDecoratorMetadata @6.0.3
    /// tsc-hash: 5688615fbf1c6bc55dbd481def6eda81d0d36556451169f5ccdb1731d927d968
    /// tsc-span: _tsc.js:82698-82713
    fn decorator_metadata_entity_name(&self, r#type: Option<NodeId>) -> Option<NodeId> {
        let r#type = r#type?;
        match self.data_of(r#type) {
            NodeData::IntersectionType(data) => {
                self.decorator_metadata_entity_name_from_types(data.types)
            }
            NodeData::UnionType(data) => self.decorator_metadata_entity_name_from_types(data.types),
            NodeData::ConditionalType(data) => self.decorator_metadata_common_entity_name(
                [data.true_type, data.false_type].into_iter().flatten(),
            ),
            NodeData::ParenthesizedType(data) => self.decorator_metadata_entity_name(data.r#type),
            NodeData::NamedTupleMember(data) => self.decorator_metadata_entity_name(data.r#type),
            NodeData::TypeReference(data) => data.type_name,
            _ => None,
        }
    }

    fn decorator_metadata_entity_name_from_types(
        &self,
        types: Option<NodeArrayId>,
    ) -> Option<NodeId> {
        self.decorator_metadata_common_entity_name(self.nodes_of(types))
    }

    /// tsc-port: getEntityNameForDecoratorMetadataFromTypeList @6.0.3
    /// tsc-hash: 00ebd0c69eec1bee6e917796ec115321557cd85b0cfcc5aa0ca45ae52936d662
    /// tsc-span: _tsc.js:82714-82739
    fn decorator_metadata_common_entity_name(
        &self,
        types: impl IntoIterator<Item = NodeId>,
    ) -> Option<NodeId> {
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        let mut common = None;
        for mut r#type in types {
            loop {
                r#type = match self.data_of(r#type) {
                    NodeData::ParenthesizedType(data) => data.r#type?,
                    NodeData::NamedTupleMember(data) => data.r#type?,
                    _ => break,
                };
            }
            if self.kind_of(r#type) == SyntaxKind::NeverKeyword {
                continue;
            }
            if !strict_null_checks
                && (self.kind_of(r#type) == SyntaxKind::UndefinedKeyword
                    || matches!(
                        self.data_of(r#type),
                        NodeData::LiteralType(data)
                            if data.literal.is_some_and(|literal| self.kind_of(literal) == SyntaxKind::NullKeyword)
                    ))
            {
                continue;
            }
            let individual = self.decorator_metadata_entity_name(Some(r#type))?;
            if let Some(previous) = common {
                let same_identifier = self.kind_of(previous) == SyntaxKind::Identifier
                    && self.kind_of(individual) == SyntaxKind::Identifier
                    && self.identifier_text(previous) == self.identifier_text(individual);
                if !same_identifier {
                    return None;
                }
            } else {
                common = Some(individual);
            }
        }
        common
    }

    /// tsc-port: nodeCanBeDecorated @6.0.3
    /// tsc-hash: 8b586f7c989010d7714f73dc58185bf97470cbc92765727657cc7754e3f72ccd
    /// tsc-span: _tsc.js:14651-14671
    ///
    /// The legacy flavor admits PARAMETER positions the ES flavor
    /// rejects and rejects private-named/class-expression targets the
    /// ES flavor admits — never hardcode either mode (risk #14).
    pub(crate) fn node_can_be_decorated(
        &self,
        use_legacy_decorators: bool,
        node: NodeId,
        parent: Option<NodeId>,
        grandparent: Option<NodeId>,
    ) -> bool {
        let source = self.binder.source_of_node(node);
        if use_legacy_decorators
            && self
                .name_of_node(node)
                .is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier)
        {
            return false;
        }
        match self.kind_of(node) {
            SyntaxKind::ClassDeclaration => true,
            SyntaxKind::ClassExpression => !use_legacy_decorators,
            SyntaxKind::PropertyDeclaration => parent.is_some_and(|parent| {
                if use_legacy_decorators {
                    self.kind_of(parent) == SyntaxKind::ClassDeclaration
                } else {
                    matches!(
                        self.kind_of(parent),
                        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                    ) && !node_util::has_syntactic_modifier(source, node, ModifierFlags::ABSTRACT)
                        && !node_util::has_syntactic_modifier(source, node, ModifierFlags::AMBIENT)
                }
            }),
            SyntaxKind::GetAccessor | SyntaxKind::SetAccessor | SyntaxKind::MethodDeclaration => {
                node_util::body_of(source, node).is_some()
                    && parent.is_some_and(|parent| {
                        if use_legacy_decorators {
                            self.kind_of(parent) == SyntaxKind::ClassDeclaration
                        } else {
                            matches!(
                                self.kind_of(parent),
                                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                            )
                        }
                    })
            }
            SyntaxKind::Parameter => {
                if !use_legacy_decorators {
                    return false;
                }
                let Some(parent) = parent else { return false };
                node_util::body_of(self.binder.source_of_node(parent), parent).is_some()
                    && matches!(
                        self.kind_of(parent),
                        SyntaxKind::Constructor
                            | SyntaxKind::MethodDeclaration
                            | SyntaxKind::SetAccessor
                    )
                    && self.get_this_parameter_of_function(parent) != Some(node)
                    && grandparent
                        .is_some_and(|gp| self.kind_of(gp) == SyntaxKind::ClassDeclaration)
            }
            _ => false,
        }
    }

    /// tsc getThisParameter (14688) reduced to the declaration side:
    /// the first parameter when its name is the `this` identifier.
    fn get_this_parameter_of_function(&self, function: NodeId) -> Option<NodeId> {
        let parameters = match self.data_of(function) {
            NodeData::Constructor(data) => data.parameters,
            NodeData::MethodDeclaration(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::FunctionDeclaration(data) => data.parameters,
            NodeData::FunctionExpression(data) => data.parameters,
            _ => None,
        };
        let first = self.nodes_of(parameters).first().copied()?;
        let name = match self.data_of(first) {
            NodeData::Parameter(data) => data.name?,
            _ => return None,
        };
        self.is_this_identifier(name).then_some(first)
    }

    /// tsc-port: checkGrammarDecorator @6.0.3
    /// tsc-hash: 87bdfc153db1d1abfd58e5df7a2ed16372dd3cf063cfa1d6d903c13308101ce6
    /// tsc-span: _tsc.js:82580-82627
    fn check_grammar_decorator(&mut self, decorator: NodeId) -> bool {
        if self.has_parse_diagnostics(decorator) {
            return false;
        }
        let NodeData::Decorator(data) = self.data_of(decorator) else {
            return false;
        };
        let Some(expression) = data.expression else {
            return false;
        };
        let mut node = expression;
        if self.kind_of(node) == SyntaxKind::ParenthesizedExpression {
            return false;
        }
        let mut can_have_call_expression = true;
        let mut error_node: Option<NodeId> = None;
        loop {
            match self.data_of(node) {
                NodeData::ExpressionWithTypeArguments(data) => {
                    let Some(next) = data.expression else { break };
                    node = next;
                }
                NodeData::NonNullExpression(data) => {
                    let Some(next) = data.expression else { break };
                    node = next;
                }
                NodeData::CallExpression(data) => {
                    if !can_have_call_expression {
                        error_node = Some(node);
                    }
                    if let Some(question_dot) = data.question_dot_token {
                        error_node = Some(question_dot);
                    }
                    let Some(next) = data.expression else { break };
                    node = next;
                    can_have_call_expression = false;
                }
                NodeData::PropertyAccessExpression(data) => {
                    if let Some(question_dot) = data.question_dot_token {
                        error_node = Some(question_dot);
                    }
                    let Some(next) = data.expression else { break };
                    node = next;
                    can_have_call_expression = false;
                }
                _ => {
                    if self.kind_of(node) != SyntaxKind::Identifier {
                        error_node = Some(node);
                    }
                    break;
                }
            }
        }
        if let Some(error_node) = error_node {
            let index = self.error_at(
                Some(expression),
                &diagnostics::Expression_must_be_enclosed_in_parentheses_to_be_used_as_a_decorator,
                &[],
            );
            let related = self.related_info_for_node(
                error_node,
                &diagnostics::Invalid_syntax_in_decorator,
                &[],
            );
            self.diagnostics[index].related.push(related);
            return true;
        }
        false
    }

    /// tsc-port: checkDecorator @6.0.3
    /// tsc-hash: 8be0ce0cdee3c8c15174b3fe4697c773f73b6373f7f5f145778a0079d717b494
    /// tsc-span: _tsc.js:82628-82663
    ///
    /// The headMessage switch: the legacy PropertyDeclaration face FALLS
    /// THROUGH to the Parameter void-or-any head; Parameter itself is
    /// reachable only under experimental_decorators=true.
    fn check_decorator(&mut self, node: NodeId) -> CheckResult<()> {
        self.check_grammar_decorator(node);
        let signature = self.get_resolved_signature(node, CheckMode::NORMAL)?;
        self.check_deprecated_signature(signature, node)?;
        let return_type = self.get_return_type_of_signature(signature)?;
        if self.tables.flags_of(return_type).intersects(TypeFlags::ANY) {
            return Ok(());
        }
        let Some(decorator_signature) = self.get_decorator_call_signature(node)? else {
            return Ok(());
        };
        let LinkSlot::Resolved(expected_return_type) =
            self.signature_of(decorator_signature).resolved_return_type
        else {
            return Ok(());
        };
        let parent = self
            .parent_of(node)
            .expect("decorators hang off their decorated node");
        let head_message: &'static DiagnosticMessage = match self.kind_of(parent) {
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => {
                &diagnostics::Decorator_function_return_type_0_is_not_assignable_to_type_1
            }
            SyntaxKind::PropertyDeclaration if !self.options.experimental_decorators => {
                &diagnostics::Decorator_function_return_type_0_is_not_assignable_to_type_1
            }
            SyntaxKind::PropertyDeclaration | SyntaxKind::Parameter => {
                &diagnostics::Decorator_function_return_type_is_0_but_is_expected_to_be_void_or_any
            }
            SyntaxKind::MethodDeclaration | SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
                &diagnostics::Decorator_function_return_type_0_is_not_assignable_to_type_1
            }
            _ => unreachable!("nodeCanBeDecorated gates the parent kinds"),
        };
        let expression = match self.data_of(node) {
            NodeData::Decorator(data) => data.expression,
            _ => None,
        };
        self.check_type_assignable_to(return_type, expected_return_type, expression, head_message)?;
        Ok(())
    }

    /// tsc-port: getDiagnosticHeadMessageForDecoratorResolution @6.0.3
    /// tsc-hash: a19523d9a6a7886fd56b87ae40ca6878b1d5bc4505b36a4704f18b9225da86f6
    /// tsc-span: _tsc.js:77281-77297
    fn diagnostic_head_message_for_decorator_resolution(
        &self,
        node: NodeId,
    ) -> &'static DiagnosticMessage {
        let parent = self
            .parent_of(node)
            .expect("decorators hang off their decorated node");
        match self.kind_of(parent) {
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => {
                &diagnostics::Unable_to_resolve_signature_of_class_decorator_when_called_as_an_expression
            }
            SyntaxKind::Parameter => {
                &diagnostics::Unable_to_resolve_signature_of_parameter_decorator_when_called_as_an_expression
            }
            SyntaxKind::PropertyDeclaration => {
                &diagnostics::Unable_to_resolve_signature_of_property_decorator_when_called_as_an_expression
            }
            SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor => {
                &diagnostics::Unable_to_resolve_signature_of_method_decorator_when_called_as_an_expression
            }
            _ => unreachable!("nodeCanBeDecorated gates the parent kinds"),
        }
    }

    /// tsc-port: resolveDecorator @6.0.3
    /// tsc-hash: 05a1c22981f35b25af51ce8b9aa5a5e84b25919cf2047938ded5ed36208ede3a
    /// tsc-span: _tsc.js:77298-77331
    ///
    /// The no-call-signatures face chains the full
    /// invocationErrorDetails result UNDER the 1238-family decorator
    /// head, including union/detail rows and the missing-await hint.
    fn resolve_decorator(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<SignatureId> {
        let NodeData::Decorator(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let expression = data.expression.expect(
            "parser invariant: try_parse_decorator always stores an expression \
             (parse recovery stores a missing expression node)",
        );
        let func_type = self.check_expression(expression, CheckMode::NORMAL)?;
        let apparent_type = self.get_apparent_type(func_type)?;
        if apparent_type == self.tables.intrinsics.error {
            return self.resolve_error_call(node);
        }
        let call_signatures = self.get_signatures_of_type(apparent_type, SignatureKind::Call)?;
        let num_construct_signatures = self
            .get_signatures_of_type(apparent_type, SignatureKind::Construct)?
            .len();
        if self.is_untyped_function_call(
            func_type,
            apparent_type,
            call_signatures.len(),
            num_construct_signatures,
        )? {
            return self.resolve_untyped_call(node);
        }
        if self.is_potentially_uncalled_decorator(node, &call_signatures)?
            && self.kind_of(expression) != SyntaxKind::ParenthesizedExpression
        {
            let node_str = self.text_of_node(expression)?;
            self.error_at(
                Some(node),
                &diagnostics::_0_accepts_too_few_arguments_to_be_used_as_a_decorator_here_Did_you_mean_to_call_it_first_and_write_0,
                &[&node_str],
            );
            return self.resolve_error_call(node);
        }
        let head_message = self.diagnostic_head_message_for_decorator_resolution(node);
        if call_signatures.is_empty() {
            let (invocation_chain, related_message) =
                self.invocation_error_details(expression, apparent_type, SignatureKind::Call)?;
            let span = self.diag_span_of_node(expression);
            let chain = MessageChain::new(head_message, &[]).with_next(vec![invocation_chain]);
            let mut diagnostic = self.diagnostic_at_span(&span, chain);
            if let Some(related_message) = related_message {
                diagnostic.related.push(self.related_info_for_node(
                    expression,
                    related_message,
                    &[],
                ));
            }
            self.push_error_diagnostic(diagnostic);
            return self.resolve_error_call(node);
        }
        self.resolve_call(
            node,
            &call_signatures,
            check_mode,
            SignatureFlags::NONE,
            Some(head_message),
        )
    }

    /// tsc-port: isPotentiallyUncalledDecorator @6.0.3
    /// tsc-hash: e3373547cf258d411ab7f2e7db1ed2347382c118fc0791edb5c75960f98b35a6
    /// tsc-span: _tsc.js:77469-77471
    fn is_potentially_uncalled_decorator(
        &mut self,
        decorator: NodeId,
        signatures: &[SignatureId],
    ) -> CheckResult<bool> {
        if signatures.is_empty() {
            return Ok(false);
        }
        for &signature in signatures {
            let data = self.signature_of(signature);
            if data.min_argument_count != 0
                || data.flags.intersects(SignatureFlags::HAS_REST_PARAMETER)
            {
                return Ok(false);
            }
            let parameter_count = data.parameters.len();
            if parameter_count >= self.get_decorator_argument_count(decorator, signature)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: getEffectiveDecoratorArguments @6.0.3
    /// tsc-hash: 5d455de97d258fb7c7000bcbbb726b122b7716afd59ccc7d2115f53901146a12
    /// tsc-span: _tsc.js:76340-76352
    ///
    /// The effective-arg COUNT comes from the DECORATOR SIGNATURE
    /// alone (ES = 2; legacy = 1/2/3 with the descriptor parameter
    /// ALWAYS present for method/get/set) — do not conflate with
    /// getDecoratorArgumentCount's arity ALLOWANCE.
    fn get_effective_decorator_arguments(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Vec<EffectiveArg>> {
        let NodeData::Decorator(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let expression = data.expression.expect(
            "parser invariant: try_parse_decorator always stores an expression \
             (parse recovery stores a missing expression node)",
        );
        let Some(signature) = self.get_decorator_call_signature(node)? else {
            // tsc Debug.fail(): ordinary checking reaches this helper
            // only after resolveDecorator has established the
            // decorated-node signature. A structurally detached
            // recovery decorator has no effective arguments to
            // synthesize; keep that non-oracle tree diagnostic-free.
            return Ok(Vec::new());
        };
        let (pos, end) = {
            let source = self.binder.source_of_node(expression);
            let raw = source.arena.node(expression);
            (raw.pos, raw.end)
        };
        let parameters = self.signature_of(signature).parameters.clone();
        let mut args = Vec::with_capacity(parameters.len());
        for param in parameters {
            let ty = self.get_type_of_symbol(param)?;
            args.push(EffectiveArg::Synthetic {
                pos,
                end,
                ty,
                is_spread: false,
                tuple_name_source: None,
            });
        }
        Ok(args)
    }

    /// tsc-port: getDecoratorArgumentCount @6.0.3
    /// tsc-hash: 2a65f755b4b632527b1587fdfa02177823fb6675c875b9a2383815618111370e
    /// tsc-span: _tsc.js:76353-76358
    fn get_decorator_argument_count(
        &mut self,
        node: NodeId,
        signature: SignatureId,
    ) -> CheckResult<usize> {
        if self.options.experimental_decorators {
            self.get_legacy_decorator_argument_count(node, signature)
        } else {
            Ok(self.get_parameter_count(signature)?.clamp(1, 2))
        }
    }

    /// tsc-port: getLegacyDecoratorArgumentCount @6.0.3
    /// tsc-hash: 15845dcaecff2638c744b4edd1a400e210ac13aaa242e037f0a32d585841f5c2
    /// tsc-span: _tsc.js:76359-76375
    ///
    /// The arity ALLOWANCE for a CANDIDATE decorator function —
    /// method/get/set vary by the candidate's own parameter count.
    fn get_legacy_decorator_argument_count(
        &mut self,
        node: NodeId,
        signature: SignatureId,
    ) -> CheckResult<usize> {
        let parent = self
            .parent_of(node)
            .expect("decorators hang off their decorated node");
        Ok(match self.kind_of(parent) {
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => 1,
            SyntaxKind::PropertyDeclaration => {
                if node_util::has_syntactic_modifier(
                    self.binder.source_of_node(parent),
                    parent,
                    ModifierFlags::ACCESSOR,
                ) {
                    3
                } else {
                    2
                }
            }
            SyntaxKind::MethodDeclaration | SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
                if self.signature_of(signature).parameters.len() <= 2 {
                    2
                } else {
                    3
                }
            }
            SyntaxKind::Parameter => 3,
            _ => unreachable!("nodeCanBeDecorated gates the parent kinds"),
        })
    }

    /// tsc-port: getDecoratorCallSignature @6.0.3
    /// tsc-hash: 32c67a2876b89d298ba7712c49d1d769b5145ba462c91c4300a9ae5f8d0a8d59
    /// tsc-span: _tsc.js:78699-78701
    ///
    /// The ONE mode dispatch (risk #14): every mode read routes
    /// through options.experimental_decorators.
    pub(crate) fn get_decorator_call_signature(
        &mut self,
        decorator: NodeId,
    ) -> CheckResult<Option<SignatureId>> {
        if self.options.experimental_decorators {
            self.get_legacy_decorator_call_signature(decorator)
        } else {
            self.get_es_decorator_call_signature(decorator)
        }
    }

    /// tsc-port: getESDecoratorCallSignature @6.0.3
    /// tsc-hash: 0578ee79cc80a950ccf2a1d50d605bf87c711ef2e37e75a8b76f99fb60ad02ac
    /// tsc-span: _tsc.js:78571-78612
    ///
    /// getTypeOfNode reduces to getTypeOfSymbol(getSymbolOfDeclaration)
    /// for class-element declarations (87730). tsc builds the getter/
    /// setter target and return function types as SEPARATE (equal)
    /// types — one shared TypeId here is relation-identical. On
    /// On checker-abort unwind the sentinel reverts so a later query
    /// recomputes (tsc cannot fail here).
    fn get_es_decorator_call_signature(
        &mut self,
        decorator: NodeId,
    ) -> CheckResult<Option<SignatureId>> {
        let parent = self
            .parent_of(decorator)
            .expect("decorators hang off their decorated node");
        if let Some(existing) = self.links.node(parent).decorator_signature {
            return Ok((existing != self.any_signature).then_some(existing));
        }
        let sentinel = self.any_signature;
        self.links
            .set_node_decorator_signature(self.speculation_depth, parent, Some(sentinel));
        let computed = self.compute_es_decorator_call_signature(parent);
        match computed {
            Ok(result) => {
                self.links.set_node_decorator_signature(
                    self.speculation_depth,
                    parent,
                    Some(result.unwrap_or(sentinel)),
                );
                Ok(result)
            }
            Err(err) => {
                self.links
                    .set_node_decorator_signature(self.speculation_depth, parent, None);
                Err(err)
            }
        }
    }

    /// The per-kind body of getESDecoratorCallSignature (span carried
    /// by the caller).
    fn compute_es_decorator_call_signature(
        &mut self,
        parent: NodeId,
    ) -> CheckResult<Option<SignatureId>> {
        match self.kind_of(parent) {
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => {
                let symbol = self.get_symbol_of_declaration(parent)?;
                let target_type = self.get_type_of_symbol(symbol)?;
                let context_type = self.create_class_decorator_context_type(target_type)?;
                Ok(Some(self.create_es_decorator_call_signature(
                    target_type,
                    context_type,
                    target_type,
                )?))
            }
            SyntaxKind::MethodDeclaration | SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
                let Some(class_node) = self.parent_of(parent).filter(|&class_node| {
                    matches!(
                        self.kind_of(class_node),
                        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                    )
                }) else {
                    return Ok(None);
                };
                let value_type = if self.kind_of(parent) == SyntaxKind::MethodDeclaration {
                    let signature = self.get_signature_from_declaration(parent)?;
                    self.get_or_create_type_from_signature(signature)?
                } else {
                    let symbol = self.get_symbol_of_declaration(parent)?;
                    self.get_type_of_symbol(symbol)?
                };
                let this_type = self.decorator_this_type_of_member(parent, class_node)?;
                let target_type = match self.kind_of(parent) {
                    SyntaxKind::GetAccessor => self.create_getter_function_type(value_type),
                    SyntaxKind::SetAccessor => self.create_setter_function_type(value_type),
                    _ => value_type,
                };
                let context_type = self.create_class_member_decorator_context_type_for_node(
                    parent, this_type, value_type,
                )?;
                Ok(Some(self.create_es_decorator_call_signature(
                    target_type,
                    context_type,
                    target_type,
                )?))
            }
            SyntaxKind::PropertyDeclaration => {
                let Some(class_node) = self.parent_of(parent).filter(|&class_node| {
                    matches!(
                        self.kind_of(class_node),
                        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                    )
                }) else {
                    return Ok(None);
                };
                let symbol = self.get_symbol_of_declaration(parent)?;
                let value_type = self.get_type_of_symbol(symbol)?;
                let this_type = self.decorator_this_type_of_member(parent, class_node)?;
                let has_accessor_modifier = node_util::has_syntactic_modifier(
                    self.binder.source_of_node(parent),
                    parent,
                    ModifierFlags::ACCESSOR,
                );
                let target_type = if has_accessor_modifier {
                    self.create_class_accessor_decorator_target_type(this_type, value_type)?
                } else {
                    self.tables.intrinsics.undefined
                };
                let context_type = self.create_class_member_decorator_context_type_for_node(
                    parent, this_type, value_type,
                )?;
                let return_type = if has_accessor_modifier {
                    self.create_class_accessor_decorator_result_type(this_type, value_type)?
                } else {
                    self.create_class_field_decorator_initializer_mutator_type(
                        this_type, value_type,
                    )?
                };
                Ok(Some(self.create_es_decorator_call_signature(
                    target_type,
                    context_type,
                    return_type,
                )?))
            }
            _ => Ok(None),
        }
    }

    /// tsc-port: getLegacyDecoratorCallSignature @6.0.3
    /// tsc-hash: f217d821b7868ed9af411efdd1cdc0f74762a3ba48c4589f17ca6fe2ad466049
    /// tsc-span: _tsc.js:78613-78698
    ///
    /// LIVE under experimental_decorators=true only. The memo protocol
    /// mirrors the ES flavor (sentinel + revert-on-unwind).
    fn get_legacy_decorator_call_signature(
        &mut self,
        decorator: NodeId,
    ) -> CheckResult<Option<SignatureId>> {
        let parent = self
            .parent_of(decorator)
            .expect("decorators hang off their decorated node");
        if let Some(existing) = self.links.node(parent).decorator_signature {
            return Ok((existing != self.any_signature).then_some(existing));
        }
        let sentinel = self.any_signature;
        self.links
            .set_node_decorator_signature(self.speculation_depth, parent, Some(sentinel));
        let computed = self.compute_legacy_decorator_call_signature(parent);
        match computed {
            Ok(result) => {
                self.links.set_node_decorator_signature(
                    self.speculation_depth,
                    parent,
                    Some(result.unwrap_or(sentinel)),
                );
                Ok(result)
            }
            Err(err) => {
                self.links
                    .set_node_decorator_signature(self.speculation_depth, parent, None);
                Err(err)
            }
        }
    }

    /// The per-kind body of getLegacyDecoratorCallSignature (span
    /// carried by the caller).
    fn compute_legacy_decorator_call_signature(
        &mut self,
        parent: NodeId,
    ) -> CheckResult<Option<SignatureId>> {
        let void_type = self.tables.intrinsics.void;
        match self.kind_of(parent) {
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => {
                let symbol = self.get_symbol_of_declaration(parent)?;
                let target_type = self.get_type_of_symbol(symbol)?;
                let target_param = self.create_synthetic_parameter("target", target_type);
                let return_type =
                    self.get_union_type_ex(&[target_type, void_type], UnionReduction::Literal)?;
                Ok(Some(self.create_synthetic_call_signature(
                    vec![target_param],
                    None,
                    return_type,
                )))
            }
            SyntaxKind::Parameter => {
                let Some(function) = self.parent_of(parent) else {
                    return Ok(None);
                };
                let function_kind = self.kind_of(function);
                let is_constructor = function_kind == SyntaxKind::Constructor;
                let class_parented = self.parent_of(function).is_some_and(|class_node| {
                    matches!(
                        self.kind_of(class_node),
                        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                    )
                });
                let method_or_setter_in_class = matches!(
                    function_kind,
                    SyntaxKind::MethodDeclaration | SyntaxKind::SetAccessor
                ) && class_parented;
                if !is_constructor && !method_or_setter_in_class {
                    return Ok(None);
                }
                let this_parameter = self.get_this_parameter_of_function(function);
                if this_parameter == Some(parent) {
                    return Ok(None);
                }
                let parameters = match self.data_of(function) {
                    NodeData::Constructor(data) => self.nodes_of(data.parameters),
                    NodeData::MethodDeclaration(data) => self.nodes_of(data.parameters),
                    NodeData::SetAccessor(data) => self.nodes_of(data.parameters),
                    _ => Vec::new(),
                };
                let raw_index = parameters
                    .iter()
                    .position(|&param| param == parent)
                    .expect("the decorated parameter sits in its function's list");
                let index = if this_parameter.is_some() {
                    raw_index
                        .checked_sub(1)
                        .expect("this-parameter precedes decorated parameters")
                } else {
                    raw_index
                };
                let target_type = if is_constructor {
                    let class_node = self
                        .parent_of(function)
                        .expect("constructors hang off their class");
                    let symbol = self.get_symbol_of_declaration(class_node)?;
                    self.get_type_of_symbol(symbol)?
                } else {
                    self.get_parent_type_of_class_element(function)?
                };
                let key_type = if is_constructor {
                    self.tables.intrinsics.undefined
                } else {
                    self.get_class_element_property_key_type(function)?
                };
                let index_type = self.tables.get_number_literal_type(index as f64);
                let target_param = self.create_synthetic_parameter("target", target_type);
                let key_param = self.create_synthetic_parameter("propertyKey", key_type);
                let index_param = self.create_synthetic_parameter("parameterIndex", index_type);
                Ok(Some(self.create_synthetic_call_signature(
                    vec![target_param, key_param, index_param],
                    None,
                    void_type,
                )))
            }
            SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::PropertyDeclaration => {
                let class_parented = self.parent_of(parent).is_some_and(|class_node| {
                    matches!(
                        self.kind_of(class_node),
                        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                    )
                });
                if !class_parented {
                    return Ok(None);
                }
                let is_property = self.kind_of(parent) == SyntaxKind::PropertyDeclaration;
                let target_type = self.get_parent_type_of_class_element(parent)?;
                let key_type = self.get_class_element_property_key_type(parent)?;
                let symbol = self.get_symbol_of_declaration(parent)?;
                let node_type = self.get_type_of_symbol(symbol)?;
                let return_type = if is_property {
                    void_type
                } else {
                    self.create_typed_property_descriptor_type(node_type)?
                };
                let has_prop_desc = !is_property
                    || node_util::has_syntactic_modifier(
                        self.binder.source_of_node(parent),
                        parent,
                        ModifierFlags::ACCESSOR,
                    );
                let target_param = self.create_synthetic_parameter("target", target_type);
                let key_param = self.create_synthetic_parameter("propertyKey", key_type);
                let full_return_type =
                    self.get_union_type_ex(&[return_type, void_type], UnionReduction::Literal)?;
                if has_prop_desc {
                    let descriptor_type = self.create_typed_property_descriptor_type(node_type)?;
                    let descriptor_param =
                        self.create_synthetic_parameter("descriptor", descriptor_type);
                    Ok(Some(self.create_synthetic_call_signature(
                        vec![target_param, key_param, descriptor_param],
                        None,
                        full_return_type,
                    )))
                } else {
                    Ok(Some(self.create_synthetic_call_signature(
                        vec![target_param, key_param],
                        None,
                        full_return_type,
                    )))
                }
            }
            _ => Ok(None),
        }
    }

    /// thisType selection shared by the ES member arms (78591/78602):
    /// static members take the class's static type, instance members
    /// the declared instance type.
    fn decorator_this_type_of_member(
        &mut self,
        member: NodeId,
        class_node: NodeId,
    ) -> CheckResult<TypeId> {
        let class_symbol = self.get_symbol_of_declaration(class_node)?;
        if self.has_static_modifier(member) {
            self.get_type_of_symbol(class_symbol)
        } else {
            self.get_declared_type_of_symbol_slice(class_symbol)
        }
    }

    /// tsc-port: getParentTypeOfClassElement @6.0.3
    /// tsc-hash: 3c26ce179366efa1cea432822c3cbfb5856061741314073968991eee7610b81c
    /// tsc-span: _tsc.js:87798-87801
    fn get_parent_type_of_class_element(&mut self, node: NodeId) -> CheckResult<TypeId> {
        let class_node = self
            .parent_of(node)
            .expect("class elements hang off their class");
        let class_symbol = self.get_symbol_of_declaration(class_node)?;
        if self.is_static_element(node) {
            self.get_type_of_symbol(class_symbol)
        } else {
            self.get_declared_type_of_symbol_slice(class_symbol)
        }
    }

    /// tsc-port: getClassElementPropertyKeyType @6.0.3
    /// tsc-hash: aac60b64694cdf8eae6dcb0edbef3d765eb1c39c713cd0f3d6fea6c7eeea4b2a
    /// tsc-span: _tsc.js:87802-87816
    fn get_class_element_property_key_type(&mut self, element: NodeId) -> CheckResult<TypeId> {
        let name = self.name_of_node(element).expect(
            "parser invariant: class method/accessor/property parsers always store a name \
             (parse recovery stores a missing Identifier)",
        );
        match self.data_of(name) {
            NodeData::Identifier(data) => {
                let text = tsc_binder::unescape_leading_underscores(&data.escaped_text).to_owned();
                Ok(self.tables.get_string_literal_type(&text))
            }
            NodeData::NumericLiteral(data) => {
                let text = data.text.clone();
                Ok(self.tables.get_string_literal_type(&text))
            }
            NodeData::StringLiteral(data) => {
                let text = data.text.clone();
                Ok(self.tables.get_string_literal_type(&text))
            }
            NodeData::ComputedPropertyName(_) => {
                let name_type = self.check_computed_property_name(name)?;
                if self.is_type_assignable_to_kind(
                    name_type,
                    TypeFlags::ES_SYMBOL_LIKE,
                    /*strict*/ false,
                )? {
                    Ok(name_type)
                } else {
                    Ok(self.tables.intrinsics.string)
                }
            }
            // tsc Debug.fail("Unsupported property name."). This is
            // reachable only on an already-invalid decorated member
            // (for example a bigint key); propagate the checker error
            // type instead of inventing a string/symbol key.
            _ => Ok(self.tables.intrinsics.error),
        }
    }

    /// tsc-port: createTypedPropertyDescriptorType @6.0.3
    /// tsc-hash: 78f79ac1082ac6506db4c68bec7fa63b6e85cea495de72ee075605bd902fc1af
    /// tsc-span: _tsc.js:61029-61031
    fn create_typed_property_descriptor_type(
        &mut self,
        property_type: TypeId,
    ) -> CheckResult<TypeId> {
        let target = self.get_global_typed_property_descriptor_type()?;
        Ok(self.create_type_from_generic_global_type(target, &[property_type]))
    }

    /// tsc-port: createClassDecoratorContextType @6.0.3
    /// tsc-hash: 4348c9337457af0838ccc26895dcc8a55ddc408fd694b54829235b2bd8fc478e
    /// tsc-span: _tsc.js:78468-78473
    fn create_class_decorator_context_type(&mut self, class_type: TypeId) -> CheckResult<TypeId> {
        let target = self.get_global_class_decorator_context_type()?;
        Ok(self.try_create_type_reference(target, &[class_type]))
    }

    /// tsc-port: createClassMemberDecoratorContextTypeForNode @6.0.3
    /// tsc-hash: 225565b05e1d5ce029d3c4e525791d8cbf9a8cc7450d96e60ad0bd71db12c1ed
    /// tsc-span: _tsc.js:78524-78531
    ///
    /// The five per-kind context builders (78474-78503) fold into the
    /// selector — each is one global lookup + tryCreateTypeReference.
    fn create_class_member_decorator_context_type_for_node(
        &mut self,
        node: NodeId,
        this_type: TypeId,
        value_type: TypeId,
    ) -> CheckResult<TypeId> {
        let is_static = self.has_static_modifier(node);
        let name = self.name_of_node(node).expect(
            "parser invariant: class method/accessor/property parsers always store a name \
             (parse recovery stores a missing Identifier)",
        );
        let is_private = self.kind_of(name) == SyntaxKind::PrivateIdentifier;
        let name_type = if is_private {
            let text = match self.data_of(name) {
                NodeData::PrivateIdentifier(data) => {
                    tsc_binder::unescape_leading_underscores(&data.escaped_text).to_owned()
                }
                _ => unreachable!("kind/data agree"),
            };
            self.tables.get_string_literal_type(&text)
        } else {
            self.get_literal_type_from_property_name(name)?
        };
        let target = match self.kind_of(node) {
            SyntaxKind::MethodDeclaration => {
                self.get_global_class_method_decorator_context_type()?
            }
            SyntaxKind::GetAccessor => self.get_global_class_getter_decorator_context_type()?,
            SyntaxKind::SetAccessor => self.get_global_class_setter_decorator_context_type()?,
            SyntaxKind::PropertyDeclaration => {
                if node_util::has_syntactic_modifier(
                    self.binder.source_of_node(node),
                    node,
                    ModifierFlags::ACCESSOR,
                ) {
                    self.get_global_class_accessor_decorator_context_type()?
                } else {
                    self.get_global_class_field_decorator_context_type()?
                }
            }
            _ => unreachable!("class-element kinds route here"),
        };
        let context_type = self.try_create_type_reference(target, &[this_type, value_type]);
        let override_type =
            self.get_class_member_decorator_context_override_type(name_type, is_private, is_static);
        self.get_intersection_type(
            &[context_type, override_type],
            tsc_types::IntersectionFlags::NONE,
        )
    }

    /// tsc-port: getClassMemberDecoratorContextOverrideType @6.0.3
    /// tsc-hash: fc32154b76ae590afc366fbe925ed5faf7f9ffe1afd0f0d15a07ae3676a1ef6d
    /// tsc-span: _tsc.js:78504-78523
    fn get_class_member_decorator_context_override_type(
        &mut self,
        name_type: TypeId,
        is_private: bool,
        is_static: bool,
    ) -> TypeId {
        let key = format!(
            "{}{}{}",
            if is_private { "p" } else { "P" },
            if is_static { "s" } else { "S" },
            name_type.0
        );
        if let Some(&cached) = self.decorator_context_override_type_cache.get(&key) {
            return cached;
        }
        let boolean_literal = |state: &Self, value: bool| {
            if value {
                state.tables.intrinsics.true_fresh
            } else {
                state.tables.intrinsics.false_fresh
            }
        };
        let name_prop = self.create_synthetic_property("name", name_type);
        let private_value = boolean_literal(self, is_private);
        let private_prop = self.create_synthetic_property("private", private_value);
        let static_value = boolean_literal(self, is_static);
        let static_prop = self.create_synthetic_property("static", static_value);
        let mut members = tsc_binder::SymbolTable::default();
        members.insert("name".to_owned(), name_prop);
        members.insert("private".to_owned(), private_prop);
        members.insert("static".to_owned(), static_prop);
        let override_type = self.create_resolved_empty_anonymous_type(None);
        let members_id = self
            .links
            .ty(override_type)
            .resolved_members
            .resolved()
            .expect("freshly created anonymous types carry resolved members");
        let resolved = self.members_mut(members_id);
        resolved.members = members;
        resolved.properties = vec![name_prop, private_prop, static_prop];
        self.decorator_context_override_type_cache
            .insert(key, override_type);
        override_type
    }

    /// tsc-port: createClassAccessorDecoratorTargetType @6.0.3
    /// tsc-hash: 182eb6887993302fe3628cf66bb41ea323863e03962b3c34a3728b093b144cda
    /// tsc-span: _tsc.js:78532-78537
    fn create_class_accessor_decorator_target_type(
        &mut self,
        this_type: TypeId,
        value_type: TypeId,
    ) -> CheckResult<TypeId> {
        let target = self.get_global_class_accessor_decorator_target_type()?;
        Ok(self.try_create_type_reference(target, &[this_type, value_type]))
    }

    /// tsc-port: createClassAccessorDecoratorResultType @6.0.3
    /// tsc-hash: 76a4bc9e9489206589373d1ac7f74509b3784f6a1a5593d0aa04c3b9cf18e385
    /// tsc-span: _tsc.js:78538-78543
    fn create_class_accessor_decorator_result_type(
        &mut self,
        this_type: TypeId,
        value_type: TypeId,
    ) -> CheckResult<TypeId> {
        let target = self.get_global_class_accessor_decorator_result_type()?;
        Ok(self.try_create_type_reference(target, &[this_type, value_type]))
    }

    /// tsc-port: createClassFieldDecoratorInitializerMutatorType @6.0.3
    /// tsc-hash: a8d34b1493af25459a49c375a0d61d6a9d2bc80f8c5df3b3656750262ab01b0d
    /// tsc-span: _tsc.js:78544-78557
    fn create_class_field_decorator_initializer_mutator_type(
        &mut self,
        this_type: TypeId,
        value_type: TypeId,
    ) -> CheckResult<TypeId> {
        let this_param = self.create_synthetic_parameter("this", this_type);
        let value_param = self.create_synthetic_parameter("value", value_type);
        let signature =
            self.create_synthetic_call_signature(vec![value_param], Some(this_param), value_type);
        Ok(self.create_single_signature_anonymous_type(None, signature))
    }

    /// tsc-port: createESDecoratorCallSignature @6.0.3
    /// tsc-hash: de37baad30ee9d12ac824e3d65005186ced51f9d34308ca656b6a27893d3b515
    /// tsc-span: _tsc.js:78558-78570
    fn create_es_decorator_call_signature(
        &mut self,
        target_type: TypeId,
        context_type: TypeId,
        non_optional_return_type: TypeId,
    ) -> CheckResult<SignatureId> {
        let target_param = self.create_synthetic_parameter("target", target_type);
        let context_param = self.create_synthetic_parameter("context", context_type);
        let return_type = self.get_union_type_ex(
            &[non_optional_return_type, self.tables.intrinsics.void],
            UnionReduction::Literal,
        )?;
        Ok(self.create_synthetic_call_signature(
            vec![target_param, context_param],
            None,
            return_type,
        ))
    }

    /// tsc-port: createGetterFunctionType @6.0.3
    /// tsc-hash: 784e4c2a63cd0b21f704d8dbcab0ad09b3da059931ff15dcee37b1a9058a5085
    /// tsc-span: _tsc.js:82677-82686
    fn create_getter_function_type(&mut self, ty: TypeId) -> TypeId {
        let signature = self.create_synthetic_call_signature(Vec::new(), None, ty);
        self.create_single_signature_anonymous_type(None, signature)
    }

    /// tsc-port: createSetterFunctionType @6.0.3
    /// tsc-hash: 895e12d1a6d34af32fa8b57f708a4b66f74a2bb76ec7d830ded3e040a2346cdb
    /// tsc-span: _tsc.js:82687-82697
    fn create_setter_function_type(&mut self, ty: TypeId) -> TypeId {
        let value_param = self.create_synthetic_parameter("value", ty);
        let signature = self.create_synthetic_call_signature(
            vec![value_param],
            None,
            self.tables.intrinsics.void,
        );
        self.create_single_signature_anonymous_type(None, signature)
    }

    /// tsc createParameter (47659): a transient function-scoped
    /// variable symbol with links.type.
    fn create_synthetic_parameter(&mut self, name: &str, ty: TypeId) -> SymbolId {
        let symbol = self
            .binder
            .create_symbol(SymbolFlags::FUNCTION_SCOPED_VARIABLE, name.to_owned());
        self.links
            .set_fresh_symbol_type(symbol, LinkSlot::Resolved(ty));
        symbol
    }

    /// tsc createProperty (47664): the transient Property twin.
    fn create_synthetic_property(&mut self, name: &str, ty: TypeId) -> SymbolId {
        let symbol = self
            .binder
            .create_symbol(SymbolFlags::PROPERTY, name.to_owned());
        self.links
            .set_fresh_symbol_type(symbol, LinkSlot::Resolved(ty));
        symbol
    }

    /// tsc createCallSignature (82664): declaration-less synthetic
    /// signature, minArgumentCount = parameters.length (the fabricated
    /// FunctionTypeNode declaration is display-only — elided; the
    /// explicit isolated-signature kind preserves its CALL flavor).
    fn create_synthetic_call_signature(
        &mut self,
        parameters: Vec<SymbolId>,
        this_parameter: Option<SymbolId>,
        return_type: TypeId,
    ) -> SignatureId {
        let min_argument_count = parameters.len() as u32;
        self.alloc_signature(crate::state::Signature {
            declaration: None,
            flags: SignatureFlags::NONE,
            type_parameters: None,
            parameters,
            this_parameter,
            min_argument_count,
            resolved_return_type: LinkSlot::Resolved(return_type),
            from_method: false,
            target: None,
            mapper: None,
            instantiations: std::collections::HashMap::new(),
            erased_signature_cache: None,
            canonical_signature_cache: None,
            base_signature_cache: None,
            composite_kind: None,
            composite_signatures: None,
            optional_call_signature_cache: (None, None),
            isolated_signature_kind: Some(SignatureKind::Call),
            isolated_signature_type: None,
        })
    }

    /// tsc-port: tryCreateTypeReference @6.0.3
    /// tsc-hash: ae88596c2a78b501835a0a4d259d84315f7a262cec928a6402a7137cec511f05
    /// tsc-span: _tsc.js:60163-60168
    fn try_create_type_reference(&mut self, target: TypeId, type_arguments: &[TypeId]) -> TypeId {
        if !type_arguments.is_empty() && target == self.empty_generic_type {
            return self.tables.intrinsics.unknown;
        }
        self.tables.create_type_reference(target, type_arguments)
    }

    // ---- spans ----

    /// createDiagnosticForNode's location for `node` (error span +
    /// UTF-16 mapping — the diagnostic_for_node twin that returns the
    /// location instead of building the diagnostic).
    /// tsrs-native: DiagSpan adapter over the ledgered
    /// createDiagnosticForNode path; tsc carries the location as
    /// ordinary object fields.
    pub(crate) fn diag_span_of_node(&self, node: NodeId) -> DiagSpan {
        let source = self.binder.source_of_node(node);
        let (start, end) = node_util::get_error_span_for_node(source, node);
        let to_utf16 = |byte: usize| -> u32 {
            source
                .positions()
                .byte_to_utf16((byte) as u32)
                .unwrap_or(byte as u32)
        };
        let (start, end) = (to_utf16(start), to_utf16(end));
        DiagSpan {
            file_name: source.file_name.clone(),
            start,
            length: end.saturating_sub(start),
        }
    }

    /// createDiagnosticForNodeArray / createSyntheticExpression range
    /// semantics: start = skipTrivia(text, pos), end taken verbatim.
    fn diag_span_of_byte_range(&self, node_in_file: NodeId, pos: u32, end: u32) -> DiagSpan {
        let source = self.binder.source_of_node(node_in_file);
        let start_byte = tsc_syntax::skip_trivia(source.text(), pos as usize);
        let to_utf16 = |byte: usize| -> u32 {
            source
                .positions()
                .byte_to_utf16((byte) as u32)
                .unwrap_or(byte as u32)
        };
        let (start, end) = (to_utf16(start_byte), to_utf16(end as usize));
        DiagSpan {
            file_name: source.file_name.clone(),
            start,
            length: end.saturating_sub(start),
        }
    }

    /// tsrs-native: Rust Diagnostic constructor adapter for a
    /// precomputed DiagSpan; tsc has no standalone counterpart.
    pub(crate) fn diagnostic_at_span(&self, span: &DiagSpan, chain: MessageChain) -> Diagnostic {
        Diagnostic::new(
            Some(span.file_name.clone()),
            Some(span.start),
            Some(span.length),
            chain,
        )
    }

    /// tsc-port: getDiagnosticSpanForCallNode @6.0.3
    /// tsc-hash: 82d39cfd61d399c95d6b1cf79bd2ca8680b17feaeb0e9bfe30a2330125180c07
    /// tsc-span: _tsc.js:76376-76380
    ///
    /// tsc-port: getDiagnosticForCallNode @6.0.3
    /// tsc-hash: d0eb85649689dc58b366f87ed7622bb43fb2c6a170f50c03cd57e07d64c84601
    /// tsc-span: _tsc.js:76381-76394
    ///
    /// CallExpression → the callee NAME span (property-access callee →
    /// `.name`); every other call-like → the node's own error span.
    fn diag_span_for_call_node(&self, node: NodeId) -> DiagSpan {
        if self.kind_of(node) == SyntaxKind::CallExpression {
            let NodeData::CallExpression(data) = self.data_of(node) else {
                unreachable!("kind/data agree");
            };
            if let Some(expression) = data.expression {
                let target = match self.data_of(expression) {
                    NodeData::PropertyAccessExpression(access) => access.name.unwrap_or(expression),
                    _ => expression,
                };
                return self.diag_span_of_node(target);
            }
        }
        self.diag_span_of_node(node)
    }

    /// tsc-port: getErrorNodeForCallNode @6.0.3
    /// tsc-hash: 296f56daeae9679cbab125c26d8d8e36bd611c9158598787c53d530e3b40b169
    /// tsc-span: _tsc.js:76395-76406
    fn get_error_node_for_call_node(&self, node: NodeId) -> NodeId {
        let (expression, is_tag) = match self.data_of(node) {
            NodeData::CallExpression(data) => (data.expression, false),
            NodeData::NewExpression(data) => (data.expression, false),
            NodeData::TaggedTemplateExpression(data) => (data.tag, true),
            _ => (None, false),
        };
        let _ = is_tag;
        let Some(expression) = expression else {
            // JSX opening-likes answer tagName; everything else the
            // node itself.
            let tag_name = match self.data_of(node) {
                NodeData::JsxOpeningElement(data) => data.tag_name,
                NodeData::JsxSelfClosingElement(data) => data.tag_name,
                _ => None,
            };
            return tag_name.unwrap_or(node);
        };
        match self.data_of(expression) {
            NodeData::PropertyAccessExpression(access) => access.name.unwrap_or(expression),
            _ => expression,
        }
    }

    // ---- untyped/error calls ----

    /// tsc-port: resolveUntypedCall @6.0.3
    /// tsc-hash: 379ef51c9ae1f6439afc5576f00e8dc816ee64ddb144f6aa41939d34a64eec13
    /// tsc-span: _tsc.js:75747-75763
    ///
    /// The deferred overload-failure pass re-enters here and walks the
    /// RAW node arguments — their contextual reads see the stashed
    /// failure candidate (§2 ordering). callLikeExpressionMayHaveType-
    /// Arguments = call/new/tagged/jsx-opening-like; the tagged/binary/
    /// jsx operand arms are 5.7b/c callers.
    pub(crate) fn resolve_untyped_call(&mut self, node: NodeId) -> CheckResult<SignatureId> {
        match self.data_of(node) {
            NodeData::CallExpression(data) => {
                let type_arguments = data.type_arguments;
                let arguments = data.arguments;
                for argument in self.nodes_of(type_arguments) {
                    self.check_source_element(Some(argument));
                }
                for argument in self.nodes_of(arguments) {
                    self.check_expression(argument, CheckMode::NORMAL)?;
                }
            }
            NodeData::NewExpression(data) => {
                let type_arguments = data.type_arguments;
                let arguments = data.arguments;
                for argument in self.nodes_of(type_arguments) {
                    self.check_source_element(Some(argument));
                }
                for argument in self.nodes_of(arguments) {
                    self.check_expression(argument, CheckMode::NORMAL)?;
                }
            }
            NodeData::TaggedTemplateExpression(data) => {
                let type_arguments = data.type_arguments;
                let template = data.template;
                for argument in self.nodes_of(type_arguments) {
                    self.check_source_element(Some(argument));
                }
                if let Some(template) = template {
                    self.check_expression(template, CheckMode::NORMAL)?;
                }
            }
            NodeData::BinaryExpression(data) => {
                if let Some(left) = data.left {
                    self.check_expression(left, CheckMode::NORMAL)?;
                }
            }
            NodeData::JsxOpeningElement(data) => {
                let type_arguments = data.type_arguments;
                let attributes = data.attributes;
                for argument in self.nodes_of(type_arguments) {
                    self.check_source_element(Some(argument));
                }
                if let Some(attributes) = attributes {
                    self.check_expression(attributes, CheckMode::NORMAL)?;
                }
            }
            NodeData::JsxSelfClosingElement(data) => {
                let type_arguments = data.type_arguments;
                let attributes = data.attributes;
                for argument in self.nodes_of(type_arguments) {
                    self.check_source_element(Some(argument));
                }
                if let Some(attributes) = attributes {
                    self.check_expression(attributes, CheckMode::NORMAL)?;
                }
            }
            _ if self.kind_of(node) == SyntaxKind::JsxOpeningFragment => {
                // Fragments carry no type arguments (not a
                // callLikeExpressionMayHaveTypeArguments kind) and no
                // operand to walk.
            }
            _ if self.kind_of(node) == SyntaxKind::Decorator => {
                // 75748-75761: decorators fall through every operand
                // branch (no type arguments, no operand walk).
            }
            _ => unreachable!("call-like kinds route here"),
        }
        Ok(self.any_signature)
    }

    /// tsc-port: resolveErrorCall @6.0.3
    /// tsc-hash: 6c240d4f52cedae55b64d4baf7391c105c57e9791fc744641e19d598212b953f
    /// tsc-span: _tsc.js:75764-75767
    fn resolve_error_call(&mut self, node: NodeId) -> CheckResult<SignatureId> {
        self.resolve_untyped_call(node)?;
        Ok(self.unknown_signature)
    }

    // ---- candidate ordering ----

    /// tsc-port: getOptionalCallSignature @6.0.3
    /// tsc-hash: 72c1153c4b5f22b531edb3d2a89992c1b967d04005f6061b1db6000fa3dadb8c
    /// tsc-span: _tsc.js:57895-57910
    ///
    /// createOptionalCallSignature folded in: the per-signature 2-slot
    /// (inner, outer) cache holds the chain-flagged clones consumed by
    /// getReturnTypeOfSignature's 59816-59820 arms.
    fn get_optional_call_signature(
        &mut self,
        signature: SignatureId,
        call_chain_flags: SignatureFlags,
    ) -> SignatureId {
        let existing_flags = self.signature_of(signature).flags;
        if SignatureFlags::from_bits(
            existing_flags.bits() & SignatureFlags::CALL_CHAIN_FLAGS.bits(),
        ) == call_chain_flags
        {
            return signature;
        }
        let inner = call_chain_flags == SignatureFlags::IS_INNER_CALL_CHAIN;
        debug_assert!(
            inner || call_chain_flags == SignatureFlags::IS_OUTER_CALL_CHAIN,
            "An optional call signature can either be for an inner call chain or an outer call chain, but not both."
        );
        let cache = self.signature_of(signature).optional_call_signature_cache;
        let cached = if inner { cache.0 } else { cache.1 };
        if let Some(cached) = cached {
            return cached;
        }
        let result = self.clone_signature(signature);
        let data = self.signature_mut(result);
        data.flags = SignatureFlags::from_bits(data.flags.bits() | call_chain_flags.bits());
        if self.speculation_depth == 0 {
            let cache = &mut self.signature_mut(signature).optional_call_signature_cache;
            if inner {
                cache.0 = Some(result);
            } else {
                cache.1 = Some(result);
            }
        }
        result
    }

    /// tsc-port: reorderCandidates @6.0.3
    /// tsc-hash: 57e0a955200c0d177aea8a27b170dcec198e247503a38539e0b1f93aec0ae896
    /// tsc-span: _tsc.js:75768-75800
    ///
    /// getSymbolOfDeclaration = getMergedSymbol (the L2 bug class);
    /// specialized (literal-typed) signatures splice ahead of the
    /// cutoff, same-symbol runs keep declaration order.
    fn reorder_candidates(
        &mut self,
        signatures: &[SignatureId],
        call_chain_flags: SignatureFlags,
    ) -> CheckResult<Vec<SignatureId>> {
        let mut result: Vec<SignatureId> = Vec::with_capacity(signatures.len());
        let mut last_parent: Option<NodeId> = None;
        let mut last_symbol: Option<SymbolId> = None;
        let mut cutoff_index = 0usize;
        let mut index = 0usize;
        let mut specialized_index: isize = -1;
        for &signature in signatures {
            let declaration = self.signature_of(signature).declaration;
            let symbol = match declaration {
                Some(declaration) => Some(self.get_symbol_of_declaration(declaration)?),
                None => None,
            };
            let parent = declaration.and_then(|declaration| self.parent_of(declaration));
            if last_symbol.is_none() || symbol == last_symbol {
                if last_parent.is_some() && parent == last_parent {
                    index += 1;
                } else {
                    last_parent = parent;
                    index = cutoff_index;
                }
            } else {
                index = result.len();
                cutoff_index = result.len();
                last_parent = parent;
            }
            last_symbol = symbol;
            let splice_index = if self
                .signature_of(signature)
                .flags
                .intersects(SignatureFlags::HAS_LITERAL_TYPES)
            {
                specialized_index += 1;
                cutoff_index += 1;
                specialized_index as usize
            } else {
                index
            };
            let inserted = if call_chain_flags != SignatureFlags::NONE {
                self.get_optional_call_signature(signature, call_chain_flags)
            } else {
                signature
            };
            result.insert(splice_index.min(result.len()), inserted);
        }
        Ok(result)
    }

    // ---- effective arguments ----

    /// tsc-port: isSpreadArgument @6.0.3
    /// tsc-hash: e8f25340d855029dda888e2c0040df70c87f8bf9ce2f2119994cea002e0cebe6
    /// tsc-span: _tsc.js:75801-75806
    ///
    /// (getSpreadArgumentIndex folded into the callers' findIndex.)
    fn is_spread_argument(&self, arg: &EffectiveArg) -> bool {
        match arg {
            EffectiveArg::Node(node) => self.kind_of(*node) == SyntaxKind::SpreadElement,
            EffectiveArg::Synthetic { is_spread, .. } => *is_spread,
        }
    }

    fn get_spread_argument_index(&self, args: &[EffectiveArg]) -> Option<usize> {
        args.iter().position(|arg| self.is_spread_argument(arg))
    }

    /// tsc-port: getEffectiveCallArguments @6.0.3
    /// tsc-hash: 67e81d21913803f705cb90293b8f58b8841797fe610040cf8b961cc8a5b6a981
    /// tsc-span: _tsc.js:76295-76339
    ///
    /// Call/new/tagged/instanceof bands live — the decorator/JSX arms
    /// own their slices (5.8/5.7c). Spread expansion: the operand
    /// checks through checkExpressionCached EXCEPT mid-fixpoint —
    /// 76324 branches on the RAW flowLoopCount (one of the 6.3
    /// fixpoint's call-site invariants; not the checkExpressionCached
    /// window, so the uncached arm applies inside a shield too);
    /// tuple spreads expand per element into Synthetics (Rest
    /// elements wrap in arrays, Variable bits mark spread-ness,
    /// labels ride tuple_name_source).
    pub(crate) fn get_effective_call_arguments(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Vec<EffectiveArg>> {
        let arguments = match self.data_of(node) {
            NodeData::CallExpression(data) => data.arguments,
            NodeData::NewExpression(data) => data.arguments,
            NodeData::TaggedTemplateExpression(data) => {
                // 76299-76308: [Synthetic(TemplateStringsArray)] at the
                // template's span + the span expressions.
                let template = data.template.expect(
                    "parser invariant: parse_tagged_template_rest always stores a template \
                     (parse recovery stores a missing TemplateTail)",
                );
                let strings_array = self.get_global_template_strings_array_type()?;
                let (pos, end) = {
                    let source = self.binder.source_of_node(template);
                    let raw = source.arena.node(template);
                    (raw.pos, raw.end)
                };
                let mut args = vec![EffectiveArg::Synthetic {
                    pos,
                    end,
                    ty: strings_array,
                    is_spread: false,
                    tuple_name_source: None,
                }];
                if let NodeData::TemplateExpression(template_data) = self.data_of(template) {
                    let spans = template_data.template_spans;
                    for span in self.nodes_of(spans) {
                        if let NodeData::TemplateSpan(span_data) = self.data_of(span) {
                            if let Some(expression) = span_data.expression {
                                args.push(EffectiveArg::Node(expression));
                            }
                        }
                    }
                }
                return Ok(args);
            }
            NodeData::BinaryExpression(data) => {
                let left = data.left.expect(
                    "parser invariant: make_binary_expression always stores its left operand \
                     (parse recovery stores a missing expression node)",
                );
                return Ok(vec![EffectiveArg::Node(left)]);
            }
            NodeData::JsxOpeningElement(JsxOpeningElementData { attributes, .. })
            | NodeData::JsxSelfClosingElement(JsxSelfClosingElementData { attributes, .. }) => {
                // 76315-76317: the attributes node is THE argument when
                // properties exist or an opening element has children.
                let attributes = attributes.expect(
                    "parser invariant: JSX opening/self-closing parsers always store an \
                     attributes node (empty or recovery)",
                );
                let has_properties = match self.data_of(attributes) {
                    NodeData::JsxAttributes(attributes_data) => {
                        !self.nodes_of(attributes_data.properties).is_empty()
                    }
                    _ => false,
                };
                let has_children = self.kind_of(node) == SyntaxKind::JsxOpeningElement
                    && self
                        .parent_of(node)
                        .is_some_and(|parent| match self.data_of(parent) {
                            NodeData::JsxElement(element) => {
                                !self.nodes_of(element.children).is_empty()
                            }
                            _ => false,
                        });
                return Ok(if has_properties || has_children {
                    vec![EffectiveArg::Node(attributes)]
                } else {
                    Vec::new()
                });
            }
            _ if self.kind_of(node) == SyntaxKind::JsxOpeningFragment => {
                // 76296-76298: one synthetic emptyFreshJsxObjectType
                // argument at the fragment's span.
                let (pos, end) = {
                    let source = self.binder.source_of_node(node);
                    let raw = source.arena.node(node);
                    (raw.pos, raw.end)
                };
                return Ok(vec![EffectiveArg::Synthetic {
                    pos,
                    end,
                    ty: self.empty_fresh_jsx_object_type,
                    is_spread: false,
                    tuple_name_source: None,
                }]);
            }
            _ if self.kind_of(node) == SyntaxKind::Decorator => {
                return self.get_effective_decorator_arguments(node);
            }
            _ => unreachable!("call-like kinds route here"),
        };
        let args: Vec<NodeId> = self.nodes_of(arguments);
        let spread_index = args
            .iter()
            .position(|&arg| self.kind_of(arg) == SyntaxKind::SpreadElement);
        let Some(spread_index) = spread_index else {
            return Ok(args.into_iter().map(EffectiveArg::Node).collect());
        };
        let mut effective_args: Vec<EffectiveArg> = args[..spread_index]
            .iter()
            .copied()
            .map(EffectiveArg::Node)
            .collect();
        for &arg in &args[spread_index..] {
            let spread_type = if self.kind_of(arg) == SyntaxKind::SpreadElement {
                let NodeData::SpreadElement(data) = self.data_of(arg) else {
                    unreachable!("kind/data agree");
                };
                match data.expression {
                    // 76324: mid-fixpoint the operand check must not
                    // memoize links.resolvedType — the memo outlives
                    // the loop, and the post-loop re-resolution that
                    // 77505 forces (the signature is never cached
                    // mid-loop) would consume a mid-loop-era operand
                    // type. tsc branches on the RAW flowLoopCount.
                    Some(expression) => Some(if self.flow_loop_stack.is_empty() {
                        self.check_expression_cached(expression, CheckMode::NORMAL)?
                    } else {
                        self.check_expression(expression, CheckMode::NORMAL)?
                    }),
                    None => None,
                }
            } else {
                None
            };
            match spread_type {
                Some(spread_type) if self.tables.is_tuple_type(spread_type) => {
                    let element_types = self.get_type_arguments(spread_type)?;
                    let target = self.tables.reference_target(spread_type);
                    let TypeData::TupleTarget(target_data) =
                        self.tables.type_of(target).data.clone()
                    else {
                        unreachable!("tuple type targets a tuple target");
                    };
                    let raw = {
                        let source = self.binder.source_of_node(arg);
                        let raw = source.arena.node(arg);
                        (raw.pos, raw.end)
                    };
                    for (i, &element) in element_types.iter().enumerate() {
                        let flags = target_data.element_flags[i];
                        let ty = if flags.intersects(ElementFlags::REST) {
                            self.create_array_type(element, /*readonly*/ false)?
                        } else {
                            element
                        };
                        let name = target_data
                            .labeled_element_declarations
                            .as_ref()
                            .and_then(|names| names.get(i).copied())
                            .flatten()
                            .map(NodeId);
                        effective_args.push(EffectiveArg::Synthetic {
                            pos: raw.0,
                            end: raw.1,
                            ty,
                            is_spread: flags.intersects(ElementFlags::VARIABLE),
                            tuple_name_source: name,
                        });
                    }
                }
                _ => effective_args.push(EffectiveArg::Node(arg)),
            }
        }
        Ok(effective_args)
    }

    /// checkSyntheticExpression (73946): spread synthetics answer the
    /// number-indexed access of their type, plain synthetics the type
    /// itself. Node args route through the real checkers.
    /// tsc-port: checkSyntheticExpression @6.0.3
    /// tsc-hash: 042f469bf501d3ed51235f98ed3d93ed5513dbe1b9f583c7a669d472c2043d23
    /// tsc-span: _tsc.js:73946-73948
    pub(crate) fn check_effective_arg(
        &mut self,
        arg: &EffectiveArg,
        check_mode: CheckMode,
    ) -> CheckResult<TypeId> {
        match *arg {
            EffectiveArg::Node(node) => self.check_expression(node, check_mode),
            EffectiveArg::Synthetic { ty, is_spread, .. } => {
                if is_spread {
                    self.get_indexed_access_type(
                        ty,
                        self.tables.intrinsics.number,
                        tsc_types::AccessFlags::NONE,
                        None,
                        None,
                        None,
                    )
                } else {
                    Ok(ty)
                }
            }
        }
    }

    fn check_effective_arg_with_contextual_type(
        &mut self,
        arg: &EffectiveArg,
        contextual_type: TypeId,
        inference_context: Option<InferenceContextId>,
        check_mode: CheckMode,
    ) -> CheckResult<TypeId> {
        match *arg {
            EffectiveArg::Node(node) => self.check_expression_with_contextual_type(
                node,
                contextual_type,
                inference_context,
                check_mode,
            ),
            EffectiveArg::Synthetic { .. } => self.check_effective_arg(arg, check_mode),
        }
    }

    fn effective_arg_kind(&self, arg: &EffectiveArg) -> Option<SyntaxKind> {
        match arg {
            EffectiveArg::Node(node) => Some(self.kind_of(*node)),
            EffectiveArg::Synthetic { .. } => None,
        }
    }

    /// EffectiveArg span (setTextRange semantics for synthetics).
    fn diag_span_of_effective_arg(&self, node_in_file: NodeId, arg: &EffectiveArg) -> DiagSpan {
        match *arg {
            EffectiveArg::Node(node) => self.diag_span_of_node(node),
            EffectiveArg::Synthetic { pos, end, .. } => {
                self.diag_span_of_byte_range(node_in_file, pos, end)
            }
        }
    }

    // ---- arity ----

    /// tsc isUnterminated (a scanner token flag the parser does not
    /// persist): reconstructed from the source text — the scanner only
    /// leaves a template literal unterminated at EOF, so a literal
    /// ending before EOF is always terminated; at EOF the closing
    /// backtick must be present and unescaped (an odd run of preceding
    /// backslashes escapes it).
    fn template_literal_is_unterminated(&self, literal: NodeId) -> bool {
        let source = self.binder.source_of_node(literal);
        let raw = source.arena.node(literal);
        let end = raw.end as usize;
        if end < source.text().len() {
            return false;
        }
        let start = tsc_syntax::skip_trivia(source.text(), raw.pos as usize);
        let text = &source.text()[start..end.min(source.text().len())];
        let Some(rest) = text.strip_suffix('`') else {
            return true;
        };
        if rest.is_empty() {
            // The lone opening backtick of a NoSubstitution literal
            // (or a bare tail) — nothing was closed.
            return true;
        }
        let backslashes = rest.len() - rest.trim_end_matches('\\').len();
        backslashes % 2 == 1
    }

    /// tsc-port: hasCorrectArity @6.0.3
    /// tsc-hash: f974d5e1c80a39323009b4a83dbeec3fa7eb8b99275f7b5b7f20b96184e65c1f
    /// tsc-span: _tsc.js:75813-75865
    ///
    /// acceptsVoid (75807-75809) folded into the under-min filter; the
    /// JS+nonstrict acceptsVoidUndefinedUnknownOrAny variant is
    /// JS-file-gated (constant false in TS programs). The JSX arm
    /// lands with 5.7c; the decorator arm with 5.8.
    fn has_correct_arity(
        &mut self,
        node: NodeId,
        args: &[EffectiveArg],
        signature: SignatureId,
        signature_help_trailing_comma: bool,
    ) -> CheckResult<bool> {
        if self.kind_of(node) == SyntaxKind::JsxOpeningFragment {
            return Ok(true);
        }
        let arg_count: usize;
        let mut call_is_incomplete = false;
        let mut effective_parameter_count = self.get_parameter_count(signature)?;
        let mut effective_minimum_arguments = self.get_min_argument_count(signature)?;
        match self.kind_of(node) {
            SyntaxKind::TaggedTemplateExpression => {
                arg_count = args.len();
                let NodeData::TaggedTemplateExpression(data) = self.data_of(node) else {
                    unreachable!("kind/data agree");
                };
                let template = data.template.expect(
                    "parser invariant: parse_tagged_template_rest always stores a template \
                     (parse recovery stores a missing TemplateTail)",
                );
                if let NodeData::TemplateExpression(template_data) = self.data_of(template) {
                    let spans = self.nodes_of(template_data.template_spans);
                    let last_span = spans.last().copied().expect(
                        "parser invariant: parse_template_expression always stores at least \
                         one TemplateSpan (ending in a real or missing TemplateTail)",
                    );
                    let literal = match self.data_of(last_span) {
                        NodeData::TemplateSpan(span_data) => span_data.literal,
                        _ => None,
                    };
                    let source = self.binder.source_of_node(node);
                    call_is_incomplete = node_util::node_is_missing(source, literal)
                        || literal.is_some_and(|l| self.template_literal_is_unterminated(l));
                } else {
                    debug_assert_eq!(
                        self.kind_of(template),
                        SyntaxKind::NoSubstitutionTemplateLiteral
                    );
                    call_is_incomplete = self.template_literal_is_unterminated(template);
                }
            }
            SyntaxKind::Decorator => {
                arg_count = self.get_decorator_argument_count(node, signature)?;
            }
            SyntaxKind::BinaryExpression => {
                arg_count = 1;
            }
            SyntaxKind::JsxOpeningElement | SyntaxKind::JsxSelfClosingElement => {
                // 75833-75840: an unterminated opening element
                // (attributes.end == node.end) is always arity-correct;
                // otherwise the counts clamp to the one-argument shape.
                let attributes = match self.data_of(node) {
                    NodeData::JsxOpeningElement(data) => data.attributes,
                    NodeData::JsxSelfClosingElement(data) => data.attributes,
                    _ => None,
                }
                .expect(
                    "parser invariant: JSX opening/self-closing parsers always store an \
                     attributes node (empty or recovery)",
                );
                let source = self.binder.source_of_node(node);
                call_is_incomplete =
                    source.arena.node(attributes).end == source.arena.node(node).end;
                if call_is_incomplete {
                    return Ok(true);
                }
                arg_count = if effective_minimum_arguments == 0 {
                    args.len()
                } else {
                    1
                };
                if !args.is_empty() {
                    effective_parameter_count = 1;
                }
                effective_minimum_arguments = effective_minimum_arguments.min(1);
            }
            _ => {
                let arguments = match self.data_of(node) {
                    NodeData::CallExpression(data) => data.arguments,
                    NodeData::NewExpression(data) => data.arguments,
                    _ => None,
                };
                let Some(arguments) = arguments else {
                    // Argument-less `new C`.
                    debug_assert_eq!(self.kind_of(node), SyntaxKind::NewExpression);
                    return Ok(self.get_min_argument_count(signature)? == 0);
                };
                arg_count = if signature_help_trailing_comma {
                    args.len() + 1
                } else {
                    args.len()
                };
                // callIsIncomplete: the argument list's close paren is
                // missing (arguments.end == node.end).
                let source = self.binder.source_of_node(node);
                let arguments_end = source.arena.node_array(arguments).end;
                call_is_incomplete = arguments_end == source.arena.node(node).end;
                if let Some(spread_arg_index) = self.get_spread_argument_index(args) {
                    return Ok(spread_arg_index >= self.get_min_argument_count(signature)?
                        && (self.has_effective_rest_parameter(signature)?
                            || spread_arg_index < self.get_parameter_count(signature)?));
                }
            }
        }
        if !self.has_effective_rest_parameter(signature)? && arg_count > effective_parameter_count {
            return Ok(false);
        }
        if call_is_incomplete || arg_count >= effective_minimum_arguments {
            return Ok(true);
        }
        let accepted_missing_types = if self.is_in_js_file(node)
            && !self
                .options
                .strict_option_value(self.options.strict_null_checks)
        {
            TypeFlags::VOID | TypeFlags::UNDEFINED | TypeFlags::UNKNOWN | TypeFlags::ANY
        } else {
            TypeFlags::VOID
        };
        for i in arg_count..effective_minimum_arguments {
            let ty = self.get_type_at_position(signature, i)?;
            let filtered = self.tables.filter_type(ty, |tables, t| {
                tables.flags_of(t).intersects(accepted_missing_types)
            });
            if self.tables.flags_of(filtered).intersects(TypeFlags::NEVER) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: hasCorrectTypeArgumentArity @6.0.3
    /// tsc-hash: cb80010a22cf0b9bd684af501fd92e442b330c0bbe9ea3005074570ee292c316
    /// tsc-span: _tsc.js:75866-75870
    pub(crate) fn has_correct_type_argument_arity(
        &self,
        signature: SignatureId,
        type_arguments: &[NodeId],
    ) -> bool {
        let type_parameters = self.signature_of(signature).type_parameters.clone();
        let num_type_parameters = type_parameters.as_deref().map_or(0, <[TypeId]>::len);
        let min_type_argument_count = self.get_min_type_argument_count(type_parameters.as_deref());
        type_arguments.is_empty()
            || (type_arguments.len() >= min_type_argument_count
                && type_arguments.len() <= num_type_parameters)
    }

    // ---- spread argument types ----

    /// tsc-port: getMutableArrayOrTupleType @6.0.3
    /// tsc-hash: 076893146c5750d0d2244745750d89fd6cd633d1aef8084b08066c1ea12ebd0a
    /// tsc-span: _tsc.js:75993-76001
    fn get_mutable_array_or_tuple_type(&mut self, ty: TypeId) -> CheckResult<TypeId> {
        if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            let mapped = self.map_type(
                ty,
                &mut |state, t| state.get_mutable_array_or_tuple_type(t).map(Some),
                /*no_reductions*/ false,
            )?;
            return Ok(mapped.expect("mapper always answers"));
        }
        if self.tables.flags_of(ty).intersects(TypeFlags::ANY) {
            return Ok(ty);
        }
        let constraint_or_self = self.get_base_constraint_of_type(ty)?.unwrap_or(ty);
        if self.is_mutable_array_or_tuple(constraint_or_self)? {
            return Ok(ty);
        }
        if self.tables.is_tuple_type(ty) {
            let element_types = self.get_type_arguments(ty)?;
            let target = self.tables.reference_target(ty);
            let TypeData::TupleTarget(data) = self.tables.type_of(target).data.clone() else {
                unreachable!("tuple type targets a tuple target");
            };
            let names = data.labeled_element_declarations.clone();
            return self.create_tuple_type_forced(
                &element_types,
                Some(&data.element_flags),
                /*readonly*/ false,
                names.as_deref(),
            );
        }
        self.create_tuple_type_forced(
            &[ty],
            Some(&[ElementFlags::VARIADIC]),
            /*readonly*/ false,
            None,
        )
    }

    /// The non-array-like spread element walk shared by both
    /// getSpreadArgumentType arms (76012/76026): Node spreads report
    /// at their expression; SYNTHETIC spreads report at the
    /// createSyntheticExpression setTextRange carried by EffectiveArg.
    fn iterated_spread_element_type(
        &mut self,
        node_in_file: NodeId,
        arg: &EffectiveArg,
        spread_type: TypeId,
        error_node: Option<NodeId>,
    ) -> CheckResult<TypeId> {
        let undefined_type = self.tables.intrinsics.undefined;
        if error_node.is_some() {
            return self.check_iterated_type_or_element_type(
                tsc_types::IterationUse::SPREAD,
                spread_type,
                undefined_type,
                error_node,
            );
        }
        let span = self.diag_span_of_effective_arg(node_in_file, arg);
        self.check_iterated_type_or_element_type_at_span(
            tsc_types::IterationUse::SPREAD,
            spread_type,
            undefined_type,
            &span,
        )
    }

    /// tsc-port: getSpreadArgumentType @6.0.3
    /// tsc-hash: dfdbbb36374c6ab5201a5e1f9856e353347e1d3cace2f7a7f8d6246a95d6fbce
    /// tsc-span: _tsc.js:76002-76042
    ///
    /// Non-array-like SYNTHETIC spreads use their EffectiveArg range
    /// as tsc's fabricated errorNode location.
    #[allow(clippy::too_many_arguments)] // tsc's spread inputs stay explicit at all four callers.
    pub(crate) fn get_spread_argument_type(
        &mut self,
        node_in_file: NodeId,
        args: &[EffectiveArg],
        index: usize,
        arg_count: usize,
        rest_type: TypeId,
        inference_context: Option<InferenceContextId>,
        check_mode: CheckMode,
    ) -> CheckResult<TypeId> {
        let in_const_context = self.is_const_type_variable(Some(rest_type), 0)?;
        if arg_count > 0 && index >= arg_count - 1 {
            let arg = &args[arg_count - 1];
            if self.is_spread_argument(arg) {
                let (spread_type, error_node) = match *arg {
                    EffectiveArg::Synthetic { ty, .. } => (ty, None),
                    EffectiveArg::Node(node) => {
                        let NodeData::SpreadElement(data) = self.data_of(node) else {
                            unreachable!("spread arguments are spread elements");
                        };
                        let expression = data.expression.expect(
                            "parser invariant: parse_spread_element always stores an operand \
                             (parse recovery stores a missing expression node)",
                        );
                        let ty = self.check_expression_with_contextual_type(
                            expression,
                            rest_type,
                            inference_context,
                            check_mode,
                        )?;
                        (ty, Some(expression))
                    }
                };
                if self.is_array_like_type(spread_type)? {
                    return self.get_mutable_array_or_tuple_type(spread_type);
                }
                let element =
                    self.iterated_spread_element_type(node_in_file, arg, spread_type, error_node)?;
                return self.create_array_type(element, in_const_context);
            }
        }
        let mut types: Vec<TypeId> = Vec::new();
        let mut flags: Vec<ElementFlags> = Vec::new();
        let mut names: Vec<Option<u32>> = Vec::new();
        for i in index..arg_count {
            let arg = args[i];
            if self.is_spread_argument(&arg) {
                let (spread_type, error_node) = match arg {
                    EffectiveArg::Synthetic { ty, .. } => (ty, None),
                    EffectiveArg::Node(node) => {
                        let NodeData::SpreadElement(data) = self.data_of(node) else {
                            unreachable!("spread arguments are spread elements");
                        };
                        let expression = data.expression.expect(
                            "parser invariant: parse_spread_element always stores an operand \
                             (parse recovery stores a missing expression node)",
                        );
                        let ty = self.check_expression(expression, CheckMode::NORMAL)?;
                        (ty, Some(expression))
                    }
                };
                if self.is_array_like_type(spread_type)? {
                    types.push(spread_type);
                    flags.push(ElementFlags::VARIADIC);
                } else {
                    let element = self.iterated_spread_element_type(
                        node_in_file,
                        &arg,
                        spread_type,
                        error_node,
                    )?;
                    types.push(element);
                    flags.push(ElementFlags::REST);
                }
            } else {
                let contextual_type = if self.tables.is_tuple_type(rest_type) {
                    self.get_contextual_type_for_element_expression_lengthed(
                        rest_type,
                        i - index,
                        arg_count - index,
                    )?
                    .unwrap_or(self.tables.intrinsics.unknown)
                } else {
                    let literal = self.tables.get_number_literal_type((i - index) as f64);
                    self.get_indexed_access_type(
                        rest_type,
                        literal,
                        tsc_types::AccessFlags::CONTEXTUAL,
                        None,
                        None,
                        None,
                    )?
                };
                let arg_type = self.check_effective_arg_with_contextual_type(
                    &arg,
                    contextual_type,
                    inference_context,
                    check_mode,
                )?;
                let has_primitive_contextual_type = in_const_context
                    || self.maybe_type_of_kind(
                        contextual_type,
                        TypeFlags::from_bits(
                            TypeFlags::PRIMITIVE.bits()
                                | TypeFlags::INDEX.bits()
                                | TypeFlags::TEMPLATE_LITERAL.bits()
                                | TypeFlags::STRING_MAPPING.bits(),
                        ),
                    );
                types.push(if has_primitive_contextual_type {
                    self.tables.get_regular_type_of_literal_type(arg_type)
                } else {
                    self.get_widened_literal_type(arg_type)?
                });
                flags.push(ElementFlags::REQUIRED);
            }
            if let EffectiveArg::Synthetic {
                tuple_name_source: Some(name),
                ..
            } = arg
            {
                names.push(Some(name.0));
            } else {
                names.push(None);
            }
        }
        let readonly = in_const_context
            && !self.some_type_result(rest_type, |state, t| state.is_mutable_array_like_type(t))?;
        let named = names.iter().any(Option::is_some);
        self.create_tuple_type_forced(
            &types,
            Some(&flags),
            readonly,
            named.then_some(names.as_slice()),
        )
    }

    // ---- explicit type arguments ----

    /// tsc-port: checkTypeArguments @6.0.3
    /// tsc-hash: f903e04f64b4cdb3a2c094232953e29f212a4f803d955fa7b4c8c902869a0cd2
    /// tsc-span: _tsc.js:76043-76074
    ///
    /// Silent during selection (reportErrors=false), real on the
    /// failure ladder. The headMessage flavor is decorator-only —
    /// instanceof (the 5.7b head producer) SKIPS type arguments at
    /// resolveCall, so candidateForTypeArgumentError never carries a
    /// head until decorators land. Plain calls report the bare 2344
    /// head; reportRelationError's source shaping (literal
    /// generalization) applies like every relation head.
    pub(crate) fn check_type_arguments(
        &mut self,
        signature: SignatureId,
        type_argument_nodes: &[NodeId],
        report_errors: bool,
        head_message: Option<&'static DiagnosticMessage>,
    ) -> CheckResult<Option<Vec<TypeId>>> {
        if head_message.is_some() {
            // The chained-head flavor (2344-coded outer chain over the
            // decorator head) arrives with decorator resolution (5.8).
            unreachable!("head producers (decorators) skip type-argument collection");
        }
        let type_parameters = self
            .signature_of(signature)
            .type_parameters
            .clone()
            .expect("checkTypeArguments callers guarantee a generic signature");
        let mut mapped: Vec<TypeId> = Vec::with_capacity(type_argument_nodes.len());
        for &node in type_argument_nodes {
            mapped.push(self.get_type_from_type_node(node)?);
        }
        let min_type_argument_count = self.get_min_type_argument_count(Some(&type_parameters));
        let type_argument_types = self
            .fill_missing_type_arguments(
                Some(&mapped),
                Some(&type_parameters),
                min_type_argument_count,
                /*is_javascript*/ false,
            )?
            .expect("Some input yields Some");
        let mut mapper = None;
        for (i, &type_argument_node) in type_argument_nodes.iter().enumerate() {
            debug_assert!(
                type_parameters.get(i).is_some(),
                "Should not call checkTypeArguments with too many type arguments"
            );
            let Some(constraint) = self.get_constraint_of_type_parameter(type_parameters[i])?
            else {
                continue;
            };
            if mapper.is_none() {
                mapper = Some(self.create_type_mapper(
                    type_parameters.clone(),
                    Some(type_argument_types.clone()),
                ));
            }
            let type_argument = type_argument_types[i];
            let instantiated = self.instantiate_type(constraint, mapper)?;
            let target = self.get_type_with_this_argument(
                instantiated,
                Some(type_argument),
                /*need_apparent_type*/ false,
            )?;
            if !self.is_type_assignable_to(type_argument, target)? {
                if report_errors {
                    let span = self.diag_span_of_node(type_argument_node);
                    let diagnostic = self.build_relation_error_with_head(
                        type_argument,
                        target,
                        &span,
                        &diagnostics::Type_0_does_not_satisfy_the_constraint_1,
                    )?;
                    self.push_error_diagnostic(diagnostic);
                }
                return Ok(None);
            }
        }
        Ok(Some(type_argument_types))
    }

    /// The tuple-rest contextual read getSpreadArgumentType makes
    /// (76029): getContextualTypeForElementExpression(restType, index,
    /// length) with no spread bookkeeping.
    fn get_contextual_type_for_element_expression_lengthed(
        &mut self,
        ty: TypeId,
        index: usize,
        length: usize,
    ) -> CheckResult<Option<TypeId>> {
        self.get_contextual_type_for_element_expression_at(ty, index, Some(length))
    }

    // ---- relation heads ----

    /// reportRelationError (65064-65115) under a PRESENT head message:
    /// the code is ALWAYS the head's (the unmatched-property override
    /// and the identically-named-types message swap are both gated on
    /// `!headMessage` — reportErrorResults 65286), the source display
    /// generalizes literals, and the reporting-mode relation walk
    /// supplies the nested errorInfo chain. Display failures abort
    /// the whole report rather than emitting a partial chain.
    /// tsrs-native: DiagSpan adapter over tsc's reportRelationError
    /// present-head path; SyntheticExpression locations have no arena
    /// NodeId in the Rust representation.
    pub(crate) fn build_relation_error_with_head(
        &mut self,
        source: TypeId,
        target: TypeId,
        span: &DiagSpan,
        head: &'static DiagnosticMessage,
    ) -> CheckResult<Diagnostic> {
        let original_source = source;
        let original_target = target;
        // Applicability's direct-head fallback bypasses isRelatedTo's
        // reporting closure. Reconstruct the same read/write
        // normalized pair used by that closure before rendering.
        let (source, target) = self.normalized_relation_report_types(source, target)?;
        // 65111: the 2345→2379 head swap under
        // exactOptionalPropertyTypes.
        let head = if head.code
            == diagnostics::Argument_of_type_0_is_not_assignable_to_parameter_of_type_1.code
            && self.options.exact_optional_property_types.unwrap_or(false)
            && self.has_exact_optional_unassignable_properties(source, target)?
        {
            &diagnostics::Argument_of_type_0_is_not_assignable_to_parameter_of_type_1_with_exactOptionalPropertyTypes_true_Consider_adding_undefined_to_the_types_of_the_target_s_properties
        } else {
            head
        };
        // The verdict is already known. Until every report-only
        // descendant is implemented, a checker abort while refining
        // the chain must not erase the accepted parent diagnostic.
        if let Ok(Some(output)) = self.relation_error_output(
            original_source,
            original_target,
            RelationKind::Assignable,
            head,
        ) {
            let mut diagnostic = self.diagnostic_at_span(span, output.message);
            diagnostic.related = output.related;
            return Ok(diagnostic);
        }
        let source_text = self.type_to_string_slice(source)?;
        let target_text = self.type_to_string_slice(target)?;
        // 65069-65072: literal sources generalize to their base
        // primitive unless the target could accept singletons.
        let source_text = if !self.tables.flags_of(target).intersects(TypeFlags::NEVER)
            && self.is_literal_type(source)
            && !self.type_could_have_top_level_singleton_types(target)?
        {
            let generalized = self.get_base_type_of_literal_type(source)?;
            // 65072: the generalized source renders through
            // getTypeNameForErrorDisplay (UseFullyQualifiedType) —
            // observable for unique symbols, which
            // getBaseTypeOfLiteralType passes through UNCHANGED and
            // whose typeof face qualifies only on the FQ chain
            // (`typeof Symbol.toPrimitive`, oracle-probed).
            self.get_type_name_for_error_display(generalized)?
        } else {
            source_text
        };
        Ok(self.diagnostic_at_span(span, MessageChain::new(head, &[source_text, target_text])))
    }

    fn build_relation_error_with_head_and_containing_chain(
        &mut self,
        source: TypeId,
        target: TypeId,
        span: &DiagSpan,
        head: &'static DiagnosticMessage,
        mut containing_message_chain: Option<MessageChain>,
    ) -> CheckResult<Diagnostic> {
        let head = if head.code
            == diagnostics::Argument_of_type_0_is_not_assignable_to_parameter_of_type_1.code
            && self.options.exact_optional_property_types.unwrap_or(false)
            && self.has_exact_optional_unassignable_properties(source, target)?
        {
            &diagnostics::Argument_of_type_0_is_not_assignable_to_parameter_of_type_1_with_exactOptionalPropertyTypes_true_Consider_adding_undefined_to_the_types_of_the_target_s_properties
        } else {
            head
        };
        if let Some(containing_message_chain) = containing_message_chain.as_mut() {
            if let Ok((_, Some(output))) = self.check_relation_with_shared_message_chain_at(
                source,
                target,
                RelationKind::Assignable,
                Some(head),
                containing_message_chain,
                None,
            ) {
                let mut diagnostic = self.diagnostic_at_span(span, output.message);
                diagnostic.related = output.related;
                return Ok(diagnostic);
            }
        }
        self.build_relation_error_with_head(source, target, span, head)
    }

    /// reportRelationError's no-head face for callers whose diagnostic
    /// target is a SyntheticExpression span rather than an arena node.
    /// This retains the relation walk's own 2322-family head selection
    /// and nested chain.
    /// tsrs-native: DiagSpan adapter over tsc's reportRelationError
    /// no-head path; SyntheticExpression locations have no arena
    /// NodeId in the Rust representation.
    pub(crate) fn build_relation_error_without_head(
        &mut self,
        source: TypeId,
        target: TypeId,
        span: &DiagSpan,
    ) -> CheckResult<Diagnostic> {
        let original_source = source;
        let original_target = target;
        let (source, target) = self.normalized_relation_report_types(source, target)?;
        if let Ok(Some(output)) = self.relation_error_output_with_context(
            original_source,
            original_target,
            RelationKind::Assignable,
            None,
            None,
        ) {
            let mut diagnostic = self.diagnostic_at_span(span, output.message);
            diagnostic.related = output.related;
            return Ok(diagnostic);
        }
        let mut source_text = self.type_to_string_slice_with_error_enclosing(source)?;
        let mut target_text = self.type_to_string_slice_with_error_enclosing(target)?;
        if source_text == target_text {
            source_text = self.get_type_name_for_error_display(source)?;
            target_text = self.get_type_name_for_error_display(target)?;
        }
        let head = if source_text == target_text {
            &diagnostics::Type_0_is_not_assignable_to_type_1_Two_different_types_with_this_name_exist_but_they_are_unrelated
        } else {
            &diagnostics::Type_0_is_not_assignable_to_type_1
        };
        let source_text = if !self.tables.flags_of(target).intersects(TypeFlags::NEVER)
            && self.is_literal_type(source)
            && !self.type_could_have_top_level_singleton_types(target)?
        {
            let generalized = self.get_base_type_of_literal_type(source)?;
            self.get_type_name_for_error_display(generalized)?
        } else {
            source_text
        };
        Ok(self.diagnostic_at_span(span, MessageChain::new(head, &[source_text, target_text])))
    }

    /// tsc-port: getExactOptionalUnassignableProperties @6.0.3
    /// tsc-hash: b8fb5d73a798dd33fc44c99fe19d5c91b0a4888656acf988ab30103a2735a1a9
    /// tsc-span: _tsc.js:67246-67249
    ///
    /// Consumers read only `.length` — the boolean face.
    pub(crate) fn has_exact_optional_unassignable_properties(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult<bool> {
        if self.tables.is_tuple_type(source) && self.tables.is_tuple_type(target) {
            return Ok(false);
        }
        for target_prop in self.get_properties_of_type(target)? {
            let name = self.binder.symbol(target_prop).escaped_name.clone();
            let source_type = self.get_type_of_property_of_type(source, &name)?;
            let target_type = self.get_type_of_symbol(target_prop)?;
            if self.is_exact_optional_property_mismatch(source_type, Some(target_type))? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // ---- this arguments ----

    /// tsc-port: getThisArgumentOfCall @6.0.3
    /// tsc-hash: 13dfb639a189d005e4cb66f7c3362059101d200d03a5001d3bb534e094422b13
    /// tsc-span: _tsc.js:76277-76288
    fn get_this_argument_of_call(&self, node: NodeId) -> Option<NodeId> {
        let expression = match self.data_of(node) {
            NodeData::BinaryExpression(data) => return data.right,
            NodeData::CallExpression(data) => data.expression,
            NodeData::TaggedTemplateExpression(data) => data.tag,
            // ES decorators take a this-argument from an access-
            // expression callee; LEGACY decorators take NONE (76281).
            NodeData::Decorator(data) if !self.options.experimental_decorators => data.expression,
            _ => None,
        }?;
        let callee = self.skip_outer_expressions(expression, OuterExpressionKinds::ALL);
        match self.data_of(callee) {
            NodeData::PropertyAccessExpression(data) => data.expression,
            NodeData::ElementAccessExpression(data) => data.expression,
            _ => None,
        }
    }

    /// tsc-port: getThisArgumentType @6.0.3
    /// tsc-hash: b30e99a4a3ccb0345c83b20f6696d1511bfd72a263972aa44b56318304205fbe
    /// tsc-span: _tsc.js:75931-75937
    fn get_this_argument_type(
        &mut self,
        this_argument_node: Option<NodeId>,
    ) -> CheckResult<TypeId> {
        let Some(node) = this_argument_node else {
            return Ok(self.tables.intrinsics.void);
        };
        let this_argument_type = self.check_expression(node, CheckMode::NORMAL)?;
        let is_instanceof_right = self.parent_of(node).is_some_and(|parent| {
            matches!(self.data_of(parent), NodeData::BinaryExpression(data)
                if data.right == Some(node)
                    && data.operator_token
                        .is_some_and(|token| self.kind_of(token) == SyntaxKind::InstanceOfKeyword))
        });
        if is_instanceof_right {
            return Ok(this_argument_type);
        }
        let source = self.binder.source_of_node(node);
        let parent = self.parent_of(node);
        if parent.is_some_and(|parent| node_util::is_optional_chain_root(source, parent)) {
            return self.get_non_nullable_type(this_argument_type);
        }
        if parent.is_some_and(|parent| node_util::is_optional_chain(source, parent)) {
            return Ok(self.remove_optional_type_marker(this_argument_type));
        }
        Ok(this_argument_type)
    }

    // ---- inferTypeArguments (the M6 7.4 stub swap) ----

    /// tsc-port: inferJsxTypeArguments @6.0.3
    /// tsc-hash: 97c59c8937f1a5f0e73a911e20391da968897e7614b1d625dcc12c7acdc5fca9
    /// tsc-span: _tsc.js:75925-75930
    fn infer_jsx_type_arguments(
        &mut self,
        node: NodeId,
        signature: SignatureId,
        check_mode: CheckMode,
        context: InferenceContextId,
    ) -> CheckResult<Vec<TypeId>> {
        let param_type = self.get_effective_first_argument_for_jsx_signature(signature, node)?;
        let attributes = match self.data_of(node) {
            NodeData::JsxOpeningElement(data) => data.attributes,
            NodeData::JsxSelfClosingElement(data) => data.attributes,
            _ => None,
        }
        .expect(
            "parser invariant: JSX opening/self-closing parsers always store an attributes \
             node (empty or recovery)",
        );
        let check_attr_type = self.check_expression_with_contextual_type(
            attributes,
            param_type,
            Some(context),
            check_mode,
        )?;
        let inferences = self.inference_context(context).inferences.clone();
        self.infer_types(
            &inferences,
            check_attr_type,
            param_type,
            InferencePriority::NONE,
            false,
        )?;
        self.get_inferred_types(context)
    }

    /// tsc-port: inferTypeArguments @6.0.3
    /// tsc-hash: 160f9b15ec563655daccf96be8c616d0522f74365e494eb6b759e4c35fc714f6
    /// tsc-span: _tsc.js:75938-75992
    ///
    /// The contextual-return pre-inference is TWO passes (75944-75961,
    /// checker-key §2.3 as corrected by the m6 doc): (a1) ReturnType-
    /// priority inference from the contextual type instantiated
    /// through a NoDefault CLONE of the outer context's fixing mapper
    /// — skipped for binding-pattern-derived contextual types; a
    /// generic contextual signature is re-keyed onto its own type
    /// parameters via getSignatureInstantiationWithoutFillingIn-
    /// TypeArguments so fresh inferences don't leak through the
    /// contextual signature's parameters. (a2) a FRESH priority-None
    /// returnContext inferring from the contextual type under
    /// createOuterReturnMapper(outerContext); `context.returnMapper`
    /// derives from cloneInferredPartOfContext of THAT context — not
    /// from the a1 pass. Then the impliedArity record (75969), this-
    /// type inference, and phase (b): per-argument inference under
    /// this SAME context (the single production Some-context producer
    /// for checkExpressionWithContextualType's push site, 80565).
    pub(crate) fn infer_type_arguments(
        &mut self,
        node: NodeId,
        signature: SignatureId,
        args: &[EffectiveArg],
        check_mode: CheckMode,
        context: InferenceContextId,
    ) -> CheckResult<Vec<TypeId>> {
        let node_kind = self.kind_of(node);
        if matches!(
            node_kind,
            SyntaxKind::JsxOpeningElement | SyntaxKind::JsxSelfClosingElement
        ) {
            return self.infer_jsx_type_arguments(node, signature, check_mode, context);
        }
        if node_kind != SyntaxKind::Decorator && node_kind != SyntaxKind::BinaryExpression {
            // 75943: skipBindingPatterns = every type parameter carries
            // a default (helper-every semantics: vacuous true).
            let type_parameters = self
                .signature_of(signature)
                .type_parameters
                .clone()
                .unwrap_or_default();
            let mut skip_binding_patterns = true;
            for &tp in &type_parameters {
                if self.get_default_from_type_parameter(tp)?.is_none() {
                    skip_binding_patterns = false;
                    break;
                }
            }
            let contextual_type = self.get_contextual_type(
                node,
                if skip_binding_patterns {
                    ContextFlags::SKIP_BINDING_PATTERNS
                } else {
                    ContextFlags::NONE
                },
            )?;
            if let Some(contextual_type) = contextual_type {
                let inference_target_type = self.get_return_type_of_signature(signature)?;
                if self.could_contain_type_variables(inference_target_type) {
                    let outer_context = self.get_inference_context(node);
                    let is_from_binding_pattern = !skip_binding_patterns
                        && self.get_contextual_type(node, ContextFlags::SKIP_BINDING_PATTERNS)?
                            != Some(contextual_type);
                    if !is_from_binding_pattern {
                        let outer_clone =
                            self.clone_inference_context(outer_context, InferenceFlags::NO_DEFAULT);
                        let outer_mapper = self.get_mapper_from_context(outer_clone);
                        let instantiated_type =
                            self.instantiate_type(contextual_type, outer_mapper)?;
                        let contextual_signature =
                            self.get_single_call_signature(instantiated_type)?;
                        let generic_contextual_signature = contextual_signature
                            .filter(|&sig| self.signature_of(sig).type_parameters.is_some());
                        let inference_source_type = match generic_contextual_signature {
                            Some(contextual_signature) => {
                                let contextual_type_parameters = self
                                    .signature_of(contextual_signature)
                                    .type_parameters
                                    .clone()
                                    .expect("filtered on Some above");
                                let instantiation = self
                                    .get_signature_instantiation_without_filling_in_type_arguments(
                                        contextual_signature,
                                        Some(&contextual_type_parameters),
                                    )?;
                                self.get_or_create_type_from_signature(instantiation)?
                            }
                            None => instantiated_type,
                        };
                        let inferences = self.inference_context(context).inferences.clone();
                        self.infer_types(
                            &inferences,
                            inference_source_type,
                            inference_target_type,
                            InferencePriority::RETURN_TYPE,
                            false,
                        )?;
                    }
                    // 75957-75960: the a2 pass — priority-None
                    // inference in a FRESH context; returnMapper comes
                    // from ITS inferred part.
                    let context_flags = self.inference_context(context).flags;
                    let return_context = self.create_inference_context(
                        &type_parameters,
                        Some(signature),
                        context_flags,
                        None,
                    );
                    let outer_return_mapper = outer_context
                        .map(|outer_context| self.create_outer_return_mapper(outer_context));
                    let return_source_type =
                        self.instantiate_type(contextual_type, outer_return_mapper)?;
                    let return_inferences =
                        self.inference_context(return_context).inferences.clone();
                    self.infer_types(
                        &return_inferences,
                        return_source_type,
                        inference_target_type,
                        InferencePriority::NONE,
                        false,
                    )?;
                    let has_candidates = self
                        .inference_context(return_context)
                        .inferences
                        .iter()
                        .any(|&slot| {
                            crate::inference::has_inference_candidates(self.inference_info(slot))
                        });
                    let return_mapper = if has_candidates {
                        let inferred_part = self.clone_inferred_part_of_context(return_context);
                        self.get_mapper_from_context(inferred_part)
                    } else {
                        None
                    };
                    self.inference_context_mut(context).return_mapper = return_mapper;
                }
            }
        }
        let rest_type = self.get_non_array_rest_type(signature)?;
        let arg_count = if rest_type.is_some() {
            std::cmp::min(self.get_parameter_count(signature)? - 1, args.len())
        } else {
            args.len()
        };
        if let Some(rest_type) = rest_type {
            if self
                .tables
                .flags_of(rest_type)
                .intersects(TypeFlags::TYPE_PARAMETER)
            {
                let info_slot = self
                    .inference_context(context)
                    .inferences
                    .iter()
                    .copied()
                    .find(|&slot| self.inference_info(slot).type_parameter == rest_type);
                if let Some(info_slot) = info_slot {
                    // 75969: findIndex(args, isSpreadArgument, argCount)
                    // — a spread at/after the rest position voids the
                    // implied arity.
                    let has_spread_from_arg_count =
                        (arg_count..args.len()).any(|index| self.is_spread_argument(&args[index]));
                    self.inference_info_mut(info_slot).implied_arity = if has_spread_from_arg_count
                    {
                        None
                    } else {
                        Some(args.len() - arg_count)
                    };
                }
            }
        }
        let this_type = self.get_this_type_of_signature(signature)?;
        if let Some(this_type) = this_type {
            if self.could_contain_type_variables(this_type) {
                let this_argument_node = self.get_this_argument_of_call(node);
                let this_argument_type = self.get_this_argument_type(this_argument_node)?;
                let inferences = self.inference_context(context).inferences.clone();
                self.infer_types(
                    &inferences,
                    this_argument_type,
                    this_type,
                    InferencePriority::NONE,
                    false,
                )?;
            }
        }
        for (index, arg) in args.iter().enumerate().take(arg_count) {
            let arg = *arg;
            if self.effective_arg_kind(&arg) == Some(SyntaxKind::OmittedExpression) {
                continue;
            }
            let param_type = self.get_type_at_position(signature, index)?;
            if self.could_contain_type_variables(param_type) {
                let arg_type = self.check_effective_arg_with_contextual_type(
                    &arg,
                    param_type,
                    Some(context),
                    check_mode,
                )?;
                let inferences = self.inference_context(context).inferences.clone();
                self.infer_types(
                    &inferences,
                    arg_type,
                    param_type,
                    InferencePriority::NONE,
                    false,
                )?;
            }
        }
        if let Some(rest_type) = rest_type {
            if self.could_contain_type_variables(rest_type) {
                let spread_type = self.get_spread_argument_type(
                    node,
                    args,
                    arg_count,
                    args.len(),
                    rest_type,
                    Some(context),
                    check_mode,
                )?;
                let inferences = self.inference_context(context).inferences.clone();
                self.infer_types(
                    &inferences,
                    spread_type,
                    rest_type,
                    InferencePriority::NONE,
                    false,
                )?;
            }
        }
        self.get_inferred_types(context)
    }

    // ---- applicability ----

    /// tsc-port: checkApplicableSignatureForJsxCallLikeElement @6.0.3
    /// tsc-hash: ee3bdb8977701e194a71bc78bdf77e9f22b52d20800425ade1f126d340f6bd65
    /// tsc-span: _tsc.js:76088-76189
    ///
    /// Report/Probe capture the shared elaborateError diagnostics as
    /// applicability data; a declined walk re-enters the source-level
    /// relation reporter at the tag name. Silent mode remains a verdict
    /// only, and the 6229 factory-arity probe is fully live.
    fn check_applicable_signature_for_jsx_call_like_element(
        &mut self,
        node: NodeId,
        signature: SignatureId,
        relation: RelationKind,
        check_mode: CheckMode,
        mode: ApplicabilityMode,
        mut containing_message_chain: Option<MessageChain>,
    ) -> CheckResult<Option<Vec<ApplicabilityError>>> {
        let param_type = self.get_effective_first_argument_for_jsx_signature(signature, node)?;
        let is_jsx_open_fragment = self.kind_of(node) == SyntaxKind::JsxOpeningFragment;
        let mut attributes_node = None;
        let attributes_type = if is_jsx_open_fragment {
            self.create_jsx_attributes_type_from_attributes_property(node, CheckMode::NORMAL)?
        } else {
            let attributes = match self.data_of(node) {
                NodeData::JsxOpeningElement(data) => data.attributes,
                NodeData::JsxSelfClosingElement(data) => data.attributes,
                _ => None,
            }
            .expect(
                "parser invariant: JSX opening/self-closing parsers always store an \
                 attributes node (empty or recovery)",
            );
            attributes_node = Some(attributes);
            self.check_expression_with_contextual_type(
                attributes, param_type, /*inference_context*/ None, check_mode,
            )?
        };
        let check_attributes_type = if check_mode.intersects(CheckMode::SKIP_CONTEXT_SENSITIVE) {
            self.get_regular_type_of_object_literal(attributes_type)?
        } else {
            attributes_type
        };
        if let Some(factory_arity_error) =
            self.check_tag_name_expects_too_many_arguments(node, mode)?
        {
            return Ok(Some(vec![factory_arity_error]));
        }
        let initially_related =
            self.is_type_related_to(check_attributes_type, param_type, relation)?;
        if initially_related {
            return Ok(None);
        }
        if mode == ApplicabilityMode::Silent {
            return Ok(Some(Vec::new()));
        }
        let relation_error_node = match self.data_of(node) {
            NodeData::JsxOpeningElement(data) => data.tag_name.unwrap_or(node),
            NodeData::JsxSelfClosingElement(data) => data.tag_name.unwrap_or(node),
            _ => node,
        };
        if let Some(attributes) = attributes_node {
            let (elaborated, diagnostics) = self.capture_literal_assignment_elaboration(
                attributes,
                param_type,
                Some(&diagnostics::Type_0_is_not_assignable_to_type_1),
                containing_message_chain.clone(),
            )?;
            if elaborated.reported() {
                return Ok(Some(
                    self.applicability_errors_from_diagnostics(diagnostics, mode),
                ));
            }
        }
        let (_, diagnostic, _) = self.capture_type_assignable_to_diagnostic_with_containing_chain(
            check_attributes_type,
            param_type,
            relation_error_node,
            &diagnostics::Type_0_is_not_assignable_to_type_1,
            &mut containing_message_chain,
        )?;
        Ok(Some(self.applicability_errors_from_diagnostics(
            diagnostic.into_iter().collect(),
            mode,
        )))
    }

    /// checkTagNameDoesNotExpectTooManyArguments (76109-76188), verdict
    /// INVERTED for the applicability protocol: None = the tag/factory
    /// pair passes; Some = the 6229 ApplicabilityError. The
    /// getJsxFactoryEntity face is the post-entity-guard survivor
    /// (jsxFactory/pragma shapes escaped upstream inside
    /// getEffectiveFirstArgumentForJsxSignature's namespace walk):
    /// `reactNamespace‖React` + ".createElement", resolved like
    /// resolveEntityName(QualifiedName, Value, ignoreErrors).
    fn check_tag_name_expects_too_many_arguments(
        &mut self,
        node: NodeId,
        mode: ApplicabilityMode,
    ) -> CheckResult<Option<ApplicabilityError>> {
        // getJsxNamespaceContainerForImplicitImport: None (guarded).
        let tag_name = match self.data_of(node) {
            NodeData::JsxOpeningElement(data) => data.tag_name,
            NodeData::JsxSelfClosingElement(data) => data.tag_name,
            _ => None,
        };
        let Some(tag_name) = tag_name else {
            return Ok(None);
        };
        if self.is_jsx_intrinsic_tag_name(tag_name)
            || self.kind_of(tag_name) == SyntaxKind::JsxNamespacedName
        {
            return Ok(None);
        }
        let tag_type = self.check_expression(tag_name, CheckMode::NORMAL)?;
        let tag_call_signatures = self.get_signatures_of_type(tag_type, SignatureKind::Call)?;
        if tag_call_signatures.is_empty() {
            return Ok(None);
        }
        // resolveEntityName(React.createElement, Value, ignoreErrors,
        // dontResolveAlias=false, node) over the SYNTHESIZED factory
        // entity, transcribed arm by arm (no arena node to hand the
        // ported resolveEntityName). Synthesized ⇒ not a JS-file name
        // (namespaceMeaning = 1920 exactly), no JS-prototype secondary
        // lookup, no type-only alias marking. The CJS-require namespace
        // re-resolution (JS valueDeclaration) is the same ledgered
        // JS-band slice as resolve_entity_name_ex's.
        let factory_namespace = self.get_jsx_namespace_name(node);
        let namespace_symbol = self.resolve_name(
            Some(node),
            &factory_namespace,
            SymbolFlags::NAMESPACE,
            /*name_not_found_message*/ None,
            /*is_use*/ true,
            /*exclude_globals*/ false,
        )?;
        let Some(namespace_symbol) = namespace_symbol else {
            return Ok(None);
        };
        let namespace_symbol = self.get_merged_symbol(namespace_symbol);
        // The left leg's tail hop: an alias without Namespace meaning
        // resolves before the exports probe.
        let namespace_symbol = if self
            .symbol_flags(namespace_symbol)
            .intersects(SymbolFlags::NAMESPACE)
        {
            namespace_symbol
        } else {
            self.resolve_alias(namespace_symbol)?
        };
        if namespace_symbol == self.unknown_symbol {
            // tsc returns unknownSymbol through: getTypeOfSymbol answers
            // errorType, which has no call signatures — the check passes.
            return Ok(None);
        }
        let exports = self.get_exports_of_symbol(namespace_symbol)?;
        let mut factory_symbol =
            self.get_symbol_in_table(&exports, "createElement", SymbolFlags::VALUE)?;
        if factory_symbol.is_none()
            && self
                .symbol_flags(namespace_symbol)
                .intersects(SymbolFlags::ALIAS)
        {
            let resolved = self.resolve_alias(namespace_symbol)?;
            let exports = self.get_exports_of_symbol(resolved)?;
            factory_symbol =
                self.get_symbol_in_table(&exports, "createElement", SymbolFlags::VALUE)?;
        }
        let Some(factory_symbol) = factory_symbol else {
            return Ok(None);
        };
        // resolveEntityName's tail hop (meaning = Value).
        let factory_symbol = if self
            .symbol_flags(factory_symbol)
            .intersects(SymbolFlags::VALUE)
        {
            factory_symbol
        } else {
            self.resolve_alias(factory_symbol)?
        };
        let factory_type = self.get_type_of_symbol(factory_symbol)?;
        let call_signatures = self.get_signatures_of_type(factory_type, SignatureKind::Call)?;
        if call_signatures.is_empty() {
            return Ok(None);
        }
        let mut has_first_param_signatures = false;
        let mut max_param_count = 0usize;
        for signature in call_signatures {
            let first_param = self.get_type_at_position(signature, 0)?;
            let signatures_of_param =
                self.get_signatures_of_type(first_param, SignatureKind::Call)?;
            if signatures_of_param.is_empty() {
                continue;
            }
            for param_signature in signatures_of_param {
                has_first_param_signatures = true;
                if self.has_effective_rest_parameter(param_signature)? {
                    return Ok(None);
                }
                let param_count = self.get_parameter_count(param_signature)?;
                max_param_count = max_param_count.max(param_count);
            }
        }
        if !has_first_param_signatures {
            return Ok(None);
        }
        let mut absolute_min_arg_count = usize::MAX;
        for tag_signature in tag_call_signatures {
            let tag_required_arg_count = self.get_min_argument_count(tag_signature)?;
            absolute_min_arg_count = absolute_min_arg_count.min(tag_required_arg_count);
        }
        if absolute_min_arg_count <= max_param_count {
            return Ok(None);
        }
        let span = self.diag_span_of_node(tag_name);
        let tag_text = self.text_of_node(tag_name)?;
        let factory_text = format!("{factory_namespace}.createElement");
        let mut related: Vec<RelatedInfo> = Vec::new();
        if let Some(tag_symbol) = self.links.node(tag_name).resolved_symbol.resolved() {
            if let Some(declaration) = self.binder.symbol(tag_symbol).value_declaration {
                related.push(self.related_info_for_node(
                    declaration,
                    &diagnostics::_0_is_declared_here,
                    &[&tag_text],
                ));
            }
        }
        let diagnostic = match mode {
            ApplicabilityMode::Report => {
                let mut diagnostic = self.diagnostic_at_span(
                    &span,
                    MessageChain::new(
                        &diagnostics::Tag_0_expects_at_least_1_arguments_but_the_JSX_factory_2_provides_at_most_3,
                        &[
                            tag_text.clone(),
                            absolute_min_arg_count.to_string(),
                            factory_text,
                            max_param_count.to_string(),
                        ],
                    ),
                );
                diagnostic.related = related.clone();
                Some(diagnostic)
            }
            _ => None,
        };
        Ok(Some(ApplicabilityError {
            span,
            related,
            diagnostic,
        }))
    }

    /// tsrs-native: run the common elaborateError reporter under
    /// errorOutputContainer.skipLogging semantics.
    ///
    /// The elaboration sink returns only rows explicitly owned by this
    /// applicability frame. Report mode publishes them after overload
    /// selection; lazy file-less diagnostics remain in the program
    /// sink and cannot accidentally suppress the outer relation head.
    fn capture_argument_elaboration(
        &mut self,
        node: NodeId,
        target: TypeId,
        head: &'static DiagnosticMessage,
        mode: ApplicabilityMode,
        containing_message_chain: Option<MessageChain>,
    ) -> CheckResult<Option<Vec<ApplicabilityError>>> {
        let (outcome, diagnostics) = self.capture_literal_assignment_elaboration(
            node,
            target,
            Some(head),
            containing_message_chain,
        )?;
        if !outcome.reported() {
            debug_assert!(diagnostics.is_empty());
            return Ok(None);
        }
        Ok(Some(
            self.applicability_errors_from_diagnostics(diagnostics, mode),
        ))
    }

    /// tsrs-native: capture the ordinary relation reporter through the
    /// same skipLogging-shaped channel as elaboration. This keeps
    /// source-level head selection (excess properties, weak targets,
    /// array readonly faces) live instead of rebuilding a head from
    /// only a preselected span.
    fn capture_argument_relation_error(
        &mut self,
        node: NodeId,
        source: TypeId,
        target: TypeId,
        head: &'static DiagnosticMessage,
        mode: ApplicabilityMode,
        mut containing_message_chain: Option<MessageChain>,
    ) -> CheckResult<Vec<ApplicabilityError>> {
        let head = if head.code
            == diagnostics::Argument_of_type_0_is_not_assignable_to_parameter_of_type_1.code
            && self.options.exact_optional_property_types.unwrap_or(false)
            && self.has_exact_optional_unassignable_properties(source, target)?
        {
            &diagnostics::Argument_of_type_0_is_not_assignable_to_parameter_of_type_1_with_exactOptionalPropertyTypes_true_Consider_adding_undefined_to_the_types_of_the_target_s_properties
        } else {
            head
        };
        let diagnostic = if let Some(containing_message_chain) = containing_message_chain.as_mut() {
            let (_, output) = self.check_relation_with_shared_message_chain_at(
                source,
                target,
                RelationKind::Assignable,
                Some(head),
                containing_message_chain,
                Some(node),
            )?;
            output.map(|output| {
                let mut diagnostic = self.create_error(output.error_node.or(Some(node)), head, &[]);
                diagnostic.message = output.message;
                diagnostic.related = output.related;
                diagnostic
            })
        } else {
            None
        };
        let diagnostic = if diagnostic.is_some() {
            diagnostic
        } else {
            self.capture_type_assignable_to_diagnostic(source, target, node, head)?
                .1
        };
        Ok(self.applicability_errors_from_diagnostics(diagnostic.into_iter().collect(), mode))
    }

    /// tsc-port: getUndefinedStrippedTargetIfNeeded @6.0.3
    /// tsc-hash: 45e8325a55be5a972853ecc64b0da3218d6a633e6d2087f63b706a5499a2d645
    /// tsc-span: _tsc.js:65586-65591
    ///
    /// Tuple spreads become synthetic arguments before applicability.
    /// By the time a synthetic element fails, the source is already
    /// the failing non-undefined constituent; carry tsc's corresponding
    /// undefined-stripped target across the split reporting boundary.
    fn spread_argument_relation_report_target(
        &mut self,
        call_like: NodeId,
        arg: &EffectiveArg,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult<TypeId> {
        let EffectiveArg::Synthetic { pos, end, .. } = *arg else {
            return Ok(target);
        };
        let arguments = match self.data_of(call_like) {
            NodeData::CallExpression(data) => data.arguments,
            NodeData::NewExpression(data) => data.arguments,
            _ => None,
        };
        let from_tuple_spread = arguments.is_some_and(|arguments| {
            self.nodes_of(Some(arguments)).iter().any(|&argument| {
                if self.kind_of(argument) != SyntaxKind::SpreadElement {
                    return false;
                }
                let source = self.binder.source_of_node(argument);
                let raw = source.arena.node(argument);
                raw.pos == pos && raw.end == end
            })
        });
        if !from_tuple_spread
            || self.tables.flags_of(source).intersects(TypeFlags::UNION)
            || !self.tables.flags_of(target).intersects(TypeFlags::UNION)
        {
            return Ok(target);
        }
        let target_types = match &self.tables.type_of(target).data {
            TypeData::Union { types, .. } => types.to_vec(),
            _ => return Ok(target),
        };
        if self
            .tables
            .flags_of(source)
            .intersects(TypeFlags::UNDEFINED)
            || !target_types.first().is_some_and(|&member| {
                self.tables
                    .flags_of(member)
                    .intersects(TypeFlags::UNDEFINED)
            })
        {
            return Ok(target);
        }
        let stripped = self.tables.filter_type(target, |tables, member| {
            !tables.flags_of(member).intersects(TypeFlags::UNDEFINED)
        });
        Ok(
            if self.tables.flags_of(stripped).intersects(TypeFlags::NEVER) {
                target
            } else {
                stripped
            },
        )
    }

    fn applicability_errors_from_diagnostics(
        &self,
        diagnostics: Vec<Diagnostic>,
        mode: ApplicabilityMode,
    ) -> Vec<ApplicabilityError> {
        diagnostics
            .into_iter()
            .map(|diagnostic| {
                let span = DiagSpan {
                    file_name: diagnostic
                        .file_name
                        .clone()
                        .expect("applicability-owned rows have a source file"),
                    start: diagnostic
                        .start
                        .expect("applicability-owned rows have a source start"),
                    length: diagnostic
                        .length
                        .expect("applicability-owned rows have a source length"),
                };
                let related = diagnostic.related.clone();
                ApplicabilityError {
                    span,
                    related,
                    diagnostic: (mode == ApplicabilityMode::Report).then_some(diagnostic),
                }
            })
            .collect()
    }

    /// tsc-port: getSignatureApplicabilityError @6.0.3
    /// tsc-hash: bd05784a6cdf0b44aae49b1b7135d05b6105da3b41e39e7b01d3950a29709f1b
    /// tsc-span: _tsc.js:76194-76276
    ///
    /// None = applicable; Some = the errorOutputContainer contents.
    /// Silent mode (selection) collects verdicts only; Report builds
    /// the head diagnostics that resolveCall either publishes directly
    /// or nests under its overload chain. maybeAddMissingAwaitInfo
    /// (76265-76275) rides as related rows.
    #[allow(clippy::too_many_arguments)] // Upstream arguments plus the shared diagnostic chain.
    fn get_signature_applicability_error(
        &mut self,
        node: NodeId,
        args: &[EffectiveArg],
        signature: SignatureId,
        relation: RelationKind,
        check_mode: CheckMode,
        mode: ApplicabilityMode,
        containing_message_chain: Option<MessageChain>,
    ) -> CheckResult<Option<Vec<ApplicabilityError>>> {
        if matches!(
            self.kind_of(node),
            SyntaxKind::JsxOpeningElement
                | SyntaxKind::JsxSelfClosingElement
                | SyntaxKind::JsxOpeningFragment
        ) {
            return self.check_applicable_signature_for_jsx_call_like_element(
                node,
                signature,
                relation,
                check_mode,
                mode,
                containing_message_chain,
            );
        }
        let this_type = self.get_this_type_of_signature(signature)?;
        if let Some(this_type) = this_type {
            let is_new = self.kind_of(node) == SyntaxKind::NewExpression;
            let is_super_property_call = matches!(self.data_of(node), NodeData::CallExpression(data)
                if data.expression.is_some_and(|expression| self.is_super_property(expression)));
            if this_type != self.tables.intrinsics.void && !is_new && !is_super_property_call {
                let this_argument_node = self.get_this_argument_of_call(node);
                let this_argument_type = self.get_this_argument_type(this_argument_node)?;
                if !self.is_type_related_to(this_argument_type, this_type, relation)? {
                    if mode == ApplicabilityMode::Silent {
                        return Ok(Some(Vec::new()));
                    }
                    let span = self.diag_span_of_node(this_argument_node.unwrap_or(node));
                    let diagnostic = match mode {
                        ApplicabilityMode::Report => Some(
                            self.build_relation_error_with_head_and_containing_chain(
                                this_argument_type,
                                this_type,
                                &span,
                                &diagnostics::The_this_context_of_type_0_is_not_assignable_to_method_s_this_of_type_1,
                                containing_message_chain,
                            )?,
                        ),
                        _ => None,
                    };
                    let related = diagnostic
                        .as_ref()
                        .map(|diagnostic| diagnostic.related.clone())
                        .unwrap_or_default();
                    return Ok(Some(vec![ApplicabilityError {
                        span,
                        related,
                        diagnostic,
                    }]));
                }
            }
        }
        let head = &diagnostics::Argument_of_type_0_is_not_assignable_to_parameter_of_type_1;
        let rest_type = self.get_non_array_rest_type(signature)?;
        let arg_count = if rest_type.is_some() {
            std::cmp::min(self.get_parameter_count(signature)? - 1, args.len())
        } else {
            args.len()
        };
        for (i, arg) in args.iter().enumerate().take(arg_count) {
            let arg = *arg;
            if self.effective_arg_kind(&arg) == Some(SyntaxKind::OmittedExpression) {
                continue;
            }
            let param_type = self.get_type_at_position(signature, i)?;
            let arg_type = self.check_effective_arg_with_contextual_type(
                &arg, param_type, /*inference_context*/ None, check_mode,
            )?;
            let check_arg_type = if check_mode.intersects(CheckMode::SKIP_CONTEXT_SENSITIVE) {
                self.get_regular_type_of_object_literal(arg_type)?
            } else {
                arg_type
            };
            if !self.is_type_related_to(check_arg_type, param_type, relation)? {
                if mode == ApplicabilityMode::Silent {
                    return Ok(Some(Vec::new()));
                }
                let effective = match arg {
                    EffectiveArg::Node(arg_node) => Some(self.get_effective_check_node(arg_node)),
                    EffectiveArg::Synthetic { .. } => None,
                };
                if let Some(effective) = effective {
                    if let Some(mut errors) = self.capture_argument_elaboration(
                        effective,
                        param_type,
                        head,
                        mode,
                        containing_message_chain.clone(),
                    )? {
                        if let Some(await_related) = self.missing_await_related(
                            node,
                            &arg,
                            check_arg_type,
                            param_type,
                            relation,
                        )? {
                            if let Some(first) = errors.first_mut() {
                                first.related.push(await_related.clone());
                                if let Some(diagnostic) = first.diagnostic.as_mut() {
                                    diagnostic.related.push(await_related);
                                }
                            }
                        }
                        return Ok(Some(errors));
                    }
                    let mut errors = self.capture_argument_relation_error(
                        effective,
                        check_arg_type,
                        param_type,
                        head,
                        mode,
                        containing_message_chain,
                    )?;
                    if let Some(await_related) = self.missing_await_related(
                        node,
                        &arg,
                        check_arg_type,
                        param_type,
                        relation,
                    )? {
                        if let Some(first) = errors.first_mut() {
                            first.related.push(await_related.clone());
                            if let Some(diagnostic) = first.diagnostic.as_mut() {
                                diagnostic.related.push(await_related);
                            }
                        }
                    }
                    return Ok(Some(errors));
                }
                // The elaboration gate: elementwise elaborations move
                // the code/span (Err); the did-you-mean flavor keeps
                // the head but reports at the walked node.
                //
                // 76229: errorNode = effectiveCheckArgumentNode — the
                // span skips parentheses/satisfies exactly like the
                // rest branch below.
                let span = match effective {
                    Some(effective) => self.diag_span_of_node(effective),
                    None => self.diag_span_of_effective_arg(node, &arg),
                };
                let mut related: Vec<RelatedInfo> = Vec::new();
                if let Some(await_related) =
                    self.missing_await_related(node, &arg, check_arg_type, param_type, relation)?
                {
                    related.push(await_related);
                }
                let diagnostic = match mode {
                    ApplicabilityMode::Report => {
                        let report_target = self.spread_argument_relation_report_target(
                            node,
                            &arg,
                            check_arg_type,
                            param_type,
                        )?;
                        let mut diagnostic = self
                            .build_relation_error_with_head_and_containing_chain(
                                check_arg_type,
                                report_target,
                                &span,
                                head,
                                containing_message_chain,
                            )?;
                        diagnostic.related.extend(related);
                        related = diagnostic.related.clone();
                        Some(diagnostic)
                    }
                    _ => None,
                };
                return Ok(Some(vec![ApplicabilityError {
                    span,
                    related,
                    diagnostic,
                }]));
            }
        }
        if let Some(rest_type) = rest_type {
            let spread_type = self.get_spread_argument_type(
                node,
                args,
                arg_count,
                args.len(),
                rest_type,
                /*inference_context*/ None,
                check_mode,
            )?;
            if !self.is_type_related_to(spread_type, rest_type, relation)? {
                if mode == ApplicabilityMode::Silent {
                    return Ok(Some(Vec::new()));
                }
                let rest_arg_count = args.len() - arg_count;
                let span = if rest_arg_count == 0 {
                    self.diag_span_of_node(node)
                } else if rest_arg_count == 1 {
                    match args[arg_count] {
                        EffectiveArg::Node(arg_node) => {
                            self.diag_span_of_node(self.get_effective_check_node(arg_node))
                        }
                        arg @ EffectiveArg::Synthetic { .. } => {
                            self.diag_span_of_effective_arg(node, &arg)
                        }
                    }
                } else {
                    let pos = self.effective_arg_pos(&args[arg_count]);
                    let end = self.effective_arg_end(&args[args.len() - 1]);
                    self.diag_span_of_byte_range(node, pos, end)
                };
                let mut related: Vec<RelatedInfo> = Vec::new();
                if let Some(await_related) =
                    self.missing_await_related_at(Some(&span), spread_type, rest_type, relation)?
                {
                    related.push(await_related);
                }
                let diagnostic = match mode {
                    ApplicabilityMode::Report => {
                        let mut diagnostic = self.build_relation_error_with_head(
                            spread_type,
                            rest_type,
                            &span,
                            head,
                        )?;
                        diagnostic.related.extend(related);
                        related = diagnostic.related.clone();
                        Some(diagnostic)
                    }
                    _ => None,
                };
                return Ok(Some(vec![ApplicabilityError {
                    span,
                    related,
                    diagnostic,
                }]));
            }
        }
        Ok(None)
    }

    fn effective_arg_pos(&self, arg: &EffectiveArg) -> u32 {
        match *arg {
            EffectiveArg::Node(node) => {
                let source = self.binder.source_of_node(node);
                source.arena.node(node).pos
            }
            EffectiveArg::Synthetic { pos, .. } => pos,
        }
    }

    fn effective_arg_end(&self, arg: &EffectiveArg) -> u32 {
        match *arg {
            EffectiveArg::Node(node) => {
                let source = self.binder.source_of_node(node);
                source.arena.node(node).end
            }
            EffectiveArg::Synthetic { end, .. } => end,
        }
    }

    /// maybeAddMissingAwaitInfo (76265-76275): related 2773 when the
    /// awaited source relates to the target and the target itself is
    /// not promise-like.
    fn missing_await_related(
        &mut self,
        node_in_file: NodeId,
        arg: &EffectiveArg,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> CheckResult<Option<RelatedInfo>> {
        let span = match *arg {
            EffectiveArg::Node(arg_node) => {
                self.diag_span_of_node(self.get_effective_check_node(arg_node))
            }
            EffectiveArg::Synthetic { .. } => self.diag_span_of_effective_arg(node_in_file, arg),
        };
        self.missing_await_related_at(Some(&span), source, target, relation)
    }

    fn missing_await_related_at(
        &mut self,
        span: Option<&DiagSpan>,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> CheckResult<Option<RelatedInfo>> {
        let Some(span) = span else { return Ok(None) };
        if self.get_awaited_type_of_promise(target)?.is_some() {
            return Ok(None);
        }
        let Some(awaited_source) = self.get_awaited_type_of_promise(source)? else {
            return Ok(None);
        };
        if !self.is_type_related_to(awaited_source, target, relation)? {
            return Ok(None);
        }
        Ok(Some(RelatedInfo {
            file_name: Some(span.file_name.clone()),
            start: Some(span.start),
            length: Some(span.length),
            message: MessageChain::new(&diagnostics::Did_you_forget_to_use_await, &[]),
        }))
    }

    /// isSuperProperty (16007): property/element access whose
    /// expression is `super`.
    fn is_super_property(&self, node: NodeId) -> bool {
        let expression = match self.data_of(node) {
            NodeData::PropertyAccessExpression(data) => data.expression,
            NodeData::ElementAccessExpression(data) => data.expression,
            _ => None,
        };
        expression.is_some_and(|expression| self.kind_of(expression) == SyntaxKind::SuperKeyword)
    }

    // ---- resolveCall ----

    /// tsc-port: resolveCall @6.0.3
    /// tsc-hash: 953dbc1e549a14a2152d422085bb1026d78c85964e4e14c962d7d0711c0875cb
    /// tsc-span: _tsc.js:76579-76870
    ///
    /// candidatesOutArray / IsForSignatureHelp are LSP-only (always
    /// None/false — signatureHelpTrailingComma stays false);
    /// isInferencePartiallyBlocked is M6 state (reportErrors stays
    /// true). The chooseOverload/addImplementationSuccessElaboration
    /// closures live on ResolveCallCtx.
    pub(crate) fn resolve_call(
        &mut self,
        node: NodeId,
        signatures: &[SignatureId],
        check_mode: CheckMode,
        call_chain_flags: SignatureFlags,
        mut head_message: Option<&'static DiagnosticMessage>,
    ) -> CheckResult<SignatureId> {
        let node_kind = self.kind_of(node);
        let is_decorator = node_kind == SyntaxKind::Decorator;
        let is_instanceof = node_kind == SyntaxKind::BinaryExpression;
        let is_jsx_open_fragment = node_kind == SyntaxKind::JsxOpeningFragment;
        debug_assert!(!self.is_inference_partially_blocked, "M6 state leaked");
        let is_super_call = matches!(self.data_of(node), NodeData::CallExpression(data)
            if data.expression.is_some_and(|e| self.kind_of(e) == SyntaxKind::SuperKeyword));

        // 76593-76598: type arguments — skipped entirely for
        // decorator/instanceof/super-call/jsx-fragment; each checks
        // EXCEPT on super-expression calls.
        let mut type_argument_nodes: Vec<NodeId> = Vec::new();
        let mut type_arguments_array: Option<NodeArrayId> = None;
        if !is_decorator && !is_instanceof && !is_super_call && !is_jsx_open_fragment {
            type_arguments_array = match self.data_of(node) {
                NodeData::CallExpression(data) => data.type_arguments,
                NodeData::NewExpression(data) => data.type_arguments,
                NodeData::TaggedTemplateExpression(data) => data.type_arguments,
                NodeData::JsxOpeningElement(data) => data.type_arguments,
                NodeData::JsxSelfClosingElement(data) => data.type_arguments,
                _ => None,
            };
            type_argument_nodes = self.nodes_of(type_arguments_array);
            for &argument in &type_argument_nodes {
                self.check_source_element(Some(argument));
            }
        }

        let candidates = self.reorder_candidates(signatures, call_chain_flags)?;
        if !is_jsx_open_fragment && candidates.is_empty() {
            let span = self.diag_span_for_call_node(node);
            let diagnostic = self.diagnostic_at_span(
                &span,
                MessageChain::new(
                    &diagnostics::Call_target_does_not_contain_any_signatures,
                    &[],
                ),
            );
            self.push_error_diagnostic(diagnostic);
            return self.resolve_error_call(node);
        }

        let args = self.get_effective_call_arguments(node)?;
        let has_computed_object_argument = args.iter().any(|arg| {
            let EffectiveArg::Node(argument) = *arg else {
                return false;
            };
            let argument = self.get_effective_check_node(argument);
            let NodeData::ObjectLiteralExpression(data) = self.data_of(argument) else {
                return false;
            };
            self.nodes_of(data.properties).into_iter().any(|property| {
                self.name_of_node(property)
                    .is_some_and(|name| self.kind_of(name) == SyntaxKind::ComputedPropertyName)
            })
        });
        let is_single_non_generic_candidate =
            candidates.len() == 1 && self.signature_of(candidates[0]).type_parameters.is_none();
        let mut arg_check_mode = CheckMode::NORMAL;
        if !is_decorator && !is_single_non_generic_candidate {
            let any_context_sensitive = args.iter().any(|arg| match arg {
                EffectiveArg::Node(node) => self.is_context_sensitive(*node),
                EffectiveArg::Synthetic { .. } => false,
            });
            if any_context_sensitive {
                arg_check_mode = CheckMode::SKIP_CONTEXT_SENSITIVE;
            }
        }

        let mut ctx = ResolveCallCtx {
            node,
            args,
            type_arguments_array,
            type_argument_nodes,
            arg_check_mode,
            candidates,
            candidates_for_argument_error: None,
            candidate_for_argument_arity_error: None,
            candidate_for_type_argument_error: None,
        };

        let mut result: Option<SignatureId> = None;
        if ctx.candidates.len() > 1 {
            result = self.choose_overload(
                &mut ctx,
                RelationKind::Subtype,
                is_single_non_generic_candidate,
                true,
            )?;
        }
        if result.is_none() {
            result = self.choose_overload(
                &mut ctx,
                RelationKind::Assignable,
                is_single_non_generic_candidate,
                true,
            )?;
        }

        // 76621-76625: a re-entrant resolution (context-sensitive arg →
        // contextual read → getResolvedSignature of the SAME node) may
        // have concretely resolved the links mid-flight.
        if let LinkSlot::Resolved(resolved) = self.links.node(node).resolved_signature {
            return Ok(resolved);
        }
        if let Some(result) = result {
            return Ok(result);
        }

        // Failure: stash the candidate BEFORE error reporting so the
        // deferred re-checks and contextual reads see its parameters
        // (76629-76630, load-bearing ordering).
        let result = self.get_candidate_for_overload_failure(node, &mut ctx, check_mode)?;

        // tsrs-native: candidate trials are rollback-capable because this
        // port has a typed CheckAbort exit that tsc does not. That boundary
        // also suppresses cold permanent-cache publication. For a lone
        // generic call, tsc's failure-candidate inference runs outside the
        // trial and may materialize the exact instantiation that its shared
        // candidate state would already have selected. Retry that one
        // materialized signature inside the same rollback boundary before
        // publishing the failure face. This preserves abort containment and
        // restores tsc's computed-symbol/contextual-object success path.
        if ctx.candidates.len() == 1
            && !is_single_non_generic_candidate
            && ctx.type_argument_nodes.is_empty()
            && has_computed_object_argument
            && ctx.candidates_for_argument_error.is_some()
            && ctx.candidate_for_argument_arity_error.is_none()
            && ctx.candidate_for_type_argument_error.is_none()
        {
            let retry = self.choose_overload(
                &mut ctx,
                RelationKind::Assignable,
                /*is_single_non_generic_candidate*/ false,
                true,
            )?;
            if let LinkSlot::Resolved(resolved) = self.links.node(node).resolved_signature {
                return Ok(resolved);
            }
            if let Some(retry) = retry {
                return Ok(retry);
            }
        }
        self.links.set_node_resolved_signature_call_protocol(
            self.speculation_depth,
            node,
            LinkSlot::Resolved(result),
        );

        if head_message.is_none() && is_instanceof {
            head_message = Some(&diagnostics::The_left_hand_side_of_an_instanceof_expression_must_be_assignable_to_the_first_argument_of_the_right_hand_side_s_Symbol_hasInstance_method);
        }
        self.report_call_resolution_failure(node, &mut ctx, signatures, head_message)?;
        Ok(result)
    }

    /// tsc-port: resolveCall @6.0.3
    /// tsc-hash: 68ea03ae08eca6dbec8884e7e22f605ee9cba8fef7ac659b0d6f2022c41b9781
    /// tsc-span: _tsc.js:76635-76663
    ///
    /// The reportErrors tail of resolveCall (76631-76742): the four-
    /// rung failure ladder. A present head message (instanceof 2860 at
    /// 5.7b; decorators 5.8) chains OUTERMOST and retains the selected
    /// applicability diagnostics beneath it.
    fn report_call_resolution_failure(
        &mut self,
        node: NodeId,
        ctx: &mut ResolveCallCtx,
        signatures: &[SignatureId],
        head_message: Option<&'static DiagnosticMessage>,
    ) -> CheckResult<()> {
        fn append_to_linear_tail(mut prefix: MessageChain, detail: MessageChain) -> MessageChain {
            let mut tail = &mut prefix;
            while tail.next.len() == 1 {
                tail = &mut tail.next[0];
            }
            debug_assert!(tail.next.is_empty(), "overload prefix is linear");
            tail.next_present = true;
            tail.next.push(detail);
            prefix
        }

        if let Some(candidates_for_argument_error) = ctx.candidates_for_argument_error.take() {
            ctx.candidates_for_argument_error = Some(candidates_for_argument_error.clone());
            if candidates_for_argument_error.len() == 1 || candidates_for_argument_error.len() > 3 {
                let last = *candidates_for_argument_error
                    .last()
                    .expect("non-empty by construction");
                let over_three = candidates_for_argument_error.len() > 3;
                let args = ctx.args.clone();
                let mut prefix = over_three.then(|| {
                    MessageChain::new(&diagnostics::No_overload_matches_this_call, &[]).with_next(
                        vec![MessageChain::new(
                            &diagnostics::The_last_overload_gave_the_following_error,
                            &[],
                        )],
                    )
                });
                if let Some(head) = head_message {
                    prefix = Some(
                        MessageChain::new(head, &[])
                            .with_next(prefix.into_iter().collect::<Vec<_>>()),
                    );
                }
                let errors = match self.get_signature_applicability_error(
                    node,
                    &args,
                    last,
                    RelationKind::Assignable,
                    CheckMode::NORMAL,
                    ApplicabilityMode::Report,
                    prefix,
                ) {
                    Ok(errors) => errors.unwrap_or_else(|| {
                        panic!(
                            "No error for last overload signature @{}",
                            self.binder.source_of_node(node).file_name
                        )
                    }),
                    Err(err) => {
                        // tsc still runs the post-report
                        // implementation probe before an unrenderable
                        // diagnostic unwinds; preserve its contextual
                        // burn/pin side effects.
                        let _ = self.implementation_success_elaboration(ctx, last);
                        return Err(err);
                    }
                };
                for error in errors {
                    let mut diagnostic = error.diagnostic.expect("Report mode builds diagnostics");
                    if over_three {
                        if let Some(declaration) = self.signature_of(last).declaration {
                            diagnostic.related.push(self.related_info_for_node(
                                declaration,
                                &diagnostics::The_last_overload_is_declared_here,
                                &[],
                            ));
                        }
                    }
                    if let Some(related) = self.implementation_success_elaboration(ctx, last)? {
                        diagnostic.related.push(related);
                    }
                    self.push_error_diagnostic(diagnostic);
                }
            } else {
                // 76667-76722: 2-3 failed candidates — each re-runs
                // under an `Overload N of M` chain. When any candidate
                // produced MORE than one error, only the min-error
                // candidate's diags feed the 2769 (last min wins, tsc
                // `diags.length <= min`); otherwise all candidates'
                // diagnostics flatten. One 2769 lands at the chosen
                // diagnostics' shared span, else at the callee error
                // node.
                let args = ctx.args.clone();
                let mut all_diagnostics: Vec<Vec<ApplicabilityError>> = Vec::new();
                let mut max = 0usize;
                let mut min = usize::MAX;
                let mut min_index = 0usize;
                for (i, &candidate) in candidates_for_argument_error.iter().enumerate() {
                    let mut errors = match self.get_signature_applicability_error(
                        node,
                        &args,
                        candidate,
                        RelationKind::Assignable,
                        CheckMode::NORMAL,
                        ApplicabilityMode::Report,
                        None,
                    ) {
                        Ok(errors) => errors.unwrap_or_else(|| {
                            panic!(
                                "No error for 3 or fewer overload signatures @{}",
                                self.binder.source_of_node(node).file_name
                            )
                        }),
                        Err(err) => {
                            // T2 containment side-effect parity — see
                            // the over_three arm (probe target is
                            // candidatesForArgumentError[0], 76724).
                            let _ = self.implementation_success_elaboration(
                                ctx,
                                candidates_for_argument_error[0],
                            );
                            return Err(err);
                        }
                    };
                    let signature_text =
                        self.signature_to_string_slice_for_overload_error(candidate)?;
                    for error in &mut errors {
                        let diagnostic = error
                            .diagnostic
                            .as_mut()
                            .expect("Report mode builds diagnostics");
                        let overload = MessageChain::new(
                            &diagnostics::Overload_0_of_1_2_gave_the_following_error,
                            &[
                                (i + 1).to_string(),
                                ctx.candidates.len().to_string(),
                                signature_text.clone(),
                            ],
                        );
                        diagnostic.message =
                            append_to_linear_tail(overload, diagnostic.message.clone());
                    }
                    if errors.len() <= min {
                        min = errors.len();
                        min_index = i;
                    }
                    max = std::cmp::max(max, errors.len());
                    all_diagnostics.push(errors);
                }
                let chosen: Vec<ApplicabilityError> = if max > 1 {
                    all_diagnostics.swap_remove(min_index)
                } else {
                    all_diagnostics.into_iter().flatten().collect()
                };
                debug_assert!(
                    !chosen.is_empty(),
                    "No errors reported for 3 or fewer overload signatures"
                );
                let details = chosen
                    .iter()
                    .map(|error| {
                        error
                            .diagnostic
                            .as_ref()
                            .expect("Report mode builds diagnostics")
                            .message
                            .clone()
                    })
                    .collect();
                let mut chain = MessageChain::new(&diagnostics::No_overload_matches_this_call, &[])
                    .with_next(details);
                if let Some(head) = head_message {
                    chain = MessageChain::new(head, &[]).with_next(vec![chain]);
                }
                let shared_span = chosen.iter().all(|error| error.span == chosen[0].span);
                let mut diagnostic = if shared_span {
                    self.diagnostic_at_span(&chosen[0].span, chain)
                } else {
                    let error_node = self.get_error_node_for_call_node(node);
                    let span = self.diag_span_of_node(error_node);
                    self.diagnostic_at_span(&span, chain)
                };
                diagnostic.related_information_present = true;
                diagnostic.related = chosen.into_iter().flat_map(|error| error.related).collect();
                if let Some(related) =
                    self.implementation_success_elaboration(ctx, candidates_for_argument_error[0])?
                {
                    diagnostic.related.push(related);
                }
                self.push_error_diagnostic(diagnostic);
            }
        } else if let Some(candidate) = ctx.candidate_for_argument_arity_error {
            let args = ctx.args.clone();
            let diagnostic =
                self.get_argument_arity_error(node, &[candidate], &args, head_message)?;
            self.push_error_diagnostic(diagnostic);
        } else if let Some(candidate) = ctx.candidate_for_type_argument_error {
            let type_argument_nodes = ctx.type_argument_nodes.clone();
            self.check_type_arguments(
                candidate,
                &type_argument_nodes,
                /*report_errors*/ true,
                head_message,
            )?;
        } else {
            let type_argument_nodes = ctx.type_argument_nodes.clone();
            let with_correct_type_argument_arity: Vec<SignatureId> = signatures
                .iter()
                .copied()
                .filter(|&s| self.has_correct_type_argument_arity(s, &type_argument_nodes))
                .collect();
            if with_correct_type_argument_arity.is_empty() {
                // Unreachable under a head: the only head producers
                // (instanceof; decorators at 5.8) skip type arguments,
                // and hasCorrectTypeArgumentArity is vacuously true
                // with none supplied.
                debug_assert!(head_message.is_none());
                let diagnostic = self.get_type_argument_arity_error(
                    node,
                    signatures,
                    ctx.type_arguments_array,
                    &type_argument_nodes,
                )?;
                self.push_error_diagnostic(diagnostic);
            } else {
                let args = ctx.args.clone();
                let diagnostic = self.get_argument_arity_error(
                    node,
                    &with_correct_type_argument_arity,
                    &args,
                    head_message,
                )?;
                self.push_error_diagnostic(diagnostic);
            }
        }
        Ok(())
    }

    /// addImplementationSuccessElaboration (76744-76762): when the
    /// failed signature's symbol has a body-bearing implementation
    /// declaration whose signature WOULD accept the call, add the 2793
    /// related row. The probe re-runs real argument checks (dedupe
    /// absorbs the duplicates); a containment inside the probe drops
    /// only the related row (attach-only, FN-safe).
    fn implementation_success_elaboration(
        &mut self,
        ctx: &mut ResolveCallCtx,
        failed: SignatureId,
    ) -> CheckResult<Option<RelatedInfo>> {
        let save_candidates = ctx.candidates_for_argument_error.take();
        let save_arity = ctx.candidate_for_argument_arity_error.take();
        let save_type_argument = ctx.candidate_for_type_argument_error.take();
        let mut probe_arg_check_mode: Option<CheckMode> = None;
        let result = (|state: &mut Self| -> CheckResult<Option<RelatedInfo>> {
            let Some(declaration) = state.signature_of(failed).declaration else {
                return Ok(None);
            };
            let Some(symbol) = state.node_symbol(declaration) else {
                return Ok(None);
            };
            let declarations = state.binder.symbol(symbol).declarations.clone();
            if declarations.len() <= 1 {
                return Ok(None);
            }
            let source = state.binder.source_of_node(declaration);
            let _ = source;
            let impl_decl = declarations.iter().copied().find(|&d| {
                node_util::is_function_like_declaration_kind(state.kind_of(d))
                    && node_util::body_of(state.binder.source_of_node(d), d).is_some()
            });
            let Some(impl_decl) = impl_decl else {
                return Ok(None);
            };
            let candidate = state.get_signature_from_declaration(impl_decl)?;
            let is_single_non_generic = state.signature_of(candidate).type_parameters.is_none();
            let mut probe_ctx = ResolveCallCtx {
                node: ctx.node,
                args: ctx.args.clone(),
                type_arguments_array: ctx.type_arguments_array,
                type_argument_nodes: ctx.type_argument_nodes.clone(),
                // 76755's chooseOverload probe reads resolveCall's LIVE
                // argCheckMode closure state (still SkipContextSensitive
                // when no pass-1 candidate survived to the re-run reset)
                // — tsc restores only the three error-candidate vars
                // around the probe, never argCheckMode, and nothing
                // reads it after reporting (7.4 review fix; the old
                // NORMAL seed skipped the probe's skip-then-recheck
                // two-step).
                arg_check_mode: ctx.arg_check_mode,
                candidates: vec![candidate],
                candidates_for_argument_error: None,
                candidate_for_argument_arity_error: None,
                candidate_for_type_argument_error: None,
            };
            let chosen = state.choose_overload(
                &mut probe_ctx,
                RelationKind::Assignable,
                is_single_non_generic,
                false,
            );
            probe_arg_check_mode = Some(probe_ctx.arg_check_mode);
            let chosen = chosen?;
            if chosen.is_some() {
                return Ok(Some(state.related_info_for_node(
                    impl_decl,
                    &diagnostics::The_call_would_have_succeeded_against_this_implementation_but_implementation_signatures_of_overloads_are_not_externally_visible,
                    &[],
                )));
            }
            Ok(None)
        })(self);
        ctx.candidates_for_argument_error = save_candidates;
        ctx.candidate_for_argument_arity_error = save_arity;
        ctx.candidate_for_type_argument_error = save_type_argument;
        // tsc restores ONLY the three error-candidate vars around the
        // probe (76746-76761) — the probe chooseOverload's argCheckMode
        // mutations write through to resolveCall's closure state, and a
        // later probe in the same report loop sees them.
        if let Some(mode) = probe_arg_check_mode {
            ctx.arg_check_mode = mode;
        }
        match result {
            Ok(related) => Ok(related),
            // Attach-only probe: containment drops the related row.
            Err(_) => Ok(None),
        }
    }

    /// tsc-port: chooseOverload @6.0.3
    /// tsc-hash: f8e61f36d383d1a4c7f036ac29776b3a5e9b119fffd53de9e28d0da96168c5f2
    /// tsc-span: _tsc.js:76763-76869
    ///
    /// The 7.4b live inference path: a generic candidate without
    /// explicit type arguments builds a per-candidate InferenceContext
    /// (76809-76814, AnyDefault in JS files), runs inferTypeArguments
    /// under `argCheckMode | SkipGenericFunctions`, and feeds the
    /// context's SkippedGenericFunction verdict back into argCheckMode
    /// (76816). Both type-argument sources share ONE instantiation
    /// tail (76821: isInJSFile(candidate.declaration) — the stub era
    /// passed false — plus the context's inferredTypeParameters). The
    /// re-run (76840-76864) re-infers on the SAME context in NORMAL
    /// mode, re-instantiates, and repeats the rest-tuple re-arity
    /// check before re-checking applicability. 9.6d wraps each real
    /// candidate body in the tsrs-native speculation transaction:
    /// success commits, ordinary rejection rolls back, inference
    /// contexts remain E-class, and permanent cache writes stay
    /// guarded while a trial is rollback-capable. The reporting-only
    /// implementation-success probe opts out: tsc deliberately lets
    /// its contextual argument burn/pin effects escape the probe, and
    /// the deferred pass observes them.
    fn choose_overload(
        &mut self,
        ctx: &mut ResolveCallCtx,
        relation: RelationKind,
        is_single_non_generic_candidate: bool,
        rollback_rejected_candidates: bool,
    ) -> CheckResult<Option<SignatureId>> {
        ctx.candidates_for_argument_error = None;
        ctx.candidate_for_argument_arity_error = None;
        ctx.candidate_for_type_argument_error = None;
        let node = ctx.node;
        if is_single_non_generic_candidate {
            let candidate = ctx.candidates[0];
            let args = ctx.args.clone();
            if !ctx.type_argument_nodes.is_empty()
                || !self.has_correct_arity(node, &args, candidate, false)?
            {
                return Ok(None);
            }
            if self
                .get_signature_applicability_error(
                    node,
                    &args,
                    candidate,
                    relation,
                    CheckMode::NORMAL,
                    ApplicabilityMode::Silent,
                    None,
                )?
                .is_some()
            {
                ctx.candidates_for_argument_error = Some(vec![candidate]);
                return Ok(None);
            }
            return Ok(Some(candidate));
        }
        for candidate_index in 0..ctx.candidates.len() {
            let candidate = ctx.candidates[candidate_index];
            let args = ctx.args.clone();
            if !self.has_correct_type_argument_arity(candidate, &ctx.type_argument_nodes)
                || !self.has_correct_arity(node, &args, candidate, false)?
            {
                continue;
            }
            let type_argument_nodes = ctx.type_argument_nodes.clone();
            let run_trial = |state: &mut Self| {
                state.try_overload_candidate(
                    node,
                    &args,
                    &type_argument_nodes,
                    candidate,
                    relation,
                    ctx.arg_check_mode,
                )
            };
            let trial = if rollback_rejected_candidates {
                self.speculate(|state| {
                    let trial = run_trial(state)?;
                    Ok(
                        if matches!(trial.disposition, OverloadCandidateDisposition::Success(_)) {
                            SpeculationOutcome::Commit(trial)
                        } else {
                            SpeculationOutcome::Reject(trial)
                        },
                    )
                })?
            } else {
                run_trial(self)?
            };
            ctx.arg_check_mode = trial.arg_check_mode;
            match trial.disposition {
                OverloadCandidateDisposition::TypeArgumentError(candidate) => {
                    ctx.candidate_for_type_argument_error = Some(candidate);
                }
                OverloadCandidateDisposition::ArgumentArityError(candidate) => {
                    ctx.candidate_for_argument_arity_error = Some(candidate);
                }
                OverloadCandidateDisposition::ArgumentError(candidate) => {
                    ctx.candidates_for_argument_error
                        .get_or_insert_with(Vec::new)
                        .push(candidate);
                }
                OverloadCandidateDisposition::Success(check_candidate) => {
                    ctx.candidates[candidate_index] = check_candidate;
                    return Ok(Some(check_candidate));
                }
            }
        }
        Ok(None)
    }

    /// One chooseOverload candidate body (76791-76868). The caller
    /// owns the transaction and applies bookkeeping after it resolves.
    fn try_overload_candidate(
        &mut self,
        node: NodeId,
        args: &[EffectiveArg],
        type_argument_nodes: &[NodeId],
        candidate: SignatureId,
        relation: RelationKind,
        mut arg_check_mode: CheckMode,
    ) -> CheckResult<OverloadCandidateTrial> {
        let mut check_candidate;
        let mut inference_context: Option<InferenceContextId> = None;
        if self.signature_of(candidate).type_parameters.is_some() {
            let type_argument_types;
            if !type_argument_nodes.is_empty() {
                let Some(checked) = self.check_type_arguments(
                    candidate,
                    type_argument_nodes,
                    /*report_errors*/ false,
                    /*head_message*/ None,
                )?
                else {
                    return Ok(OverloadCandidateTrial {
                        disposition: OverloadCandidateDisposition::TypeArgumentError(candidate),
                        arg_check_mode,
                    });
                };
                type_argument_types = checked;
            } else {
                let type_parameters = self
                    .signature_of(candidate)
                    .type_parameters
                    .clone()
                    .expect("checked Some above");
                let flags = if self.is_in_js_file(node) {
                    InferenceFlags::ANY_DEFAULT
                } else {
                    InferenceFlags::NONE
                };
                let context =
                    self.create_inference_context(&type_parameters, Some(candidate), flags, None);
                inference_context = Some(context);
                type_argument_types = self.infer_type_arguments(
                    node,
                    candidate,
                    args,
                    arg_check_mode | CheckMode::SKIP_GENERIC_FUNCTIONS,
                    context,
                )?;
                if self
                    .inference_context(context)
                    .flags
                    .intersects(InferenceFlags::SKIPPED_GENERIC_FUNCTION)
                {
                    arg_check_mode |= CheckMode::SKIP_GENERIC_FUNCTIONS;
                }
            }
            let is_javascript = self
                .signature_of(candidate)
                .declaration
                .is_some_and(|declaration| self.is_in_js_file(declaration));
            let inferred_type_parameters = inference_context.and_then(|context| {
                self.inference_context(context)
                    .inferred_type_parameters
                    .clone()
            });
            check_candidate = self.get_signature_instantiation(
                candidate,
                Some(&type_argument_types),
                is_javascript,
                inferred_type_parameters.as_deref(),
            )?;
            if self.get_non_array_rest_type(candidate)?.is_some()
                && !self.has_correct_arity(node, args, check_candidate, false)?
            {
                return Ok(OverloadCandidateTrial {
                    disposition: OverloadCandidateDisposition::ArgumentArityError(check_candidate),
                    arg_check_mode,
                });
            }
        } else {
            check_candidate = candidate;
        }
        if self
            .get_signature_applicability_error(
                node,
                args,
                check_candidate,
                relation,
                arg_check_mode,
                ApplicabilityMode::Silent,
                None,
            )?
            .is_some()
        {
            return Ok(OverloadCandidateTrial {
                disposition: OverloadCandidateDisposition::ArgumentError(check_candidate),
                arg_check_mode,
            });
        }
        if !arg_check_mode.is_empty() {
            arg_check_mode = CheckMode::NORMAL;
            if let Some(context) = inference_context {
                let type_argument_types =
                    self.infer_type_arguments(node, candidate, args, arg_check_mode, context)?;
                let is_javascript = self
                    .signature_of(candidate)
                    .declaration
                    .is_some_and(|declaration| self.is_in_js_file(declaration));
                let inferred_type_parameters = self
                    .inference_context(context)
                    .inferred_type_parameters
                    .clone();
                check_candidate = self.get_signature_instantiation(
                    candidate,
                    Some(&type_argument_types),
                    is_javascript,
                    inferred_type_parameters.as_deref(),
                )?;
                if self.get_non_array_rest_type(candidate)?.is_some()
                    && !self.has_correct_arity(node, args, check_candidate, false)?
                {
                    return Ok(OverloadCandidateTrial {
                        disposition: OverloadCandidateDisposition::ArgumentArityError(
                            check_candidate,
                        ),
                        arg_check_mode,
                    });
                }
            }
            if self
                .get_signature_applicability_error(
                    node,
                    args,
                    check_candidate,
                    relation,
                    arg_check_mode,
                    ApplicabilityMode::Silent,
                    None,
                )?
                .is_some()
            {
                return Ok(OverloadCandidateTrial {
                    disposition: OverloadCandidateDisposition::ArgumentError(check_candidate),
                    arg_check_mode,
                });
            }
        }
        Ok(OverloadCandidateTrial {
            disposition: OverloadCandidateDisposition::Success(check_candidate),
            arg_check_mode,
        })
    }

    // ---- failure candidates ----

    /// tsc-port: getCandidateForOverloadFailure @6.0.3
    /// tsc-hash: adb5aafbe61488c803eae179a53fac1b841d113aaf7957ea811c62d1c654f234
    /// tsc-span: _tsc.js:76871-76875
    ///
    /// checkNodeDeferred ALWAYS — the deferred pass re-checks the raw
    /// arguments with the stashed candidate feeding contextual reads.
    fn get_candidate_for_overload_failure(
        &mut self,
        node: NodeId,
        ctx: &mut ResolveCallCtx,
        check_mode: CheckMode,
    ) -> CheckResult<SignatureId> {
        debug_assert!(!ctx.candidates.is_empty());
        self.check_node_deferred(node);
        let any_generic = ctx
            .candidates
            .iter()
            .any(|&c| self.signature_of(c).type_parameters.is_some());
        if ctx.candidates.len() == 1 || any_generic {
            self.pick_longest_candidate_signature(node, ctx, check_mode)
        } else {
            self.create_union_of_signatures_for_overload_failure(&ctx.candidates)
        }
    }

    /// tsc-port: pickLongestCandidateSignature @6.0.3
    /// tsc-hash: 4fc7d0044870d548ebaedcda33e0f43d9cb442e80bc9903676738e800d523164
    /// tsc-span: _tsc.js:76924-76935
    ///
    /// (inferSignatureInstantiationForOverloadFailure 76946-76955
    /// folded in as the no-explicit-typeargs arm — live at 7.4b; the
    /// M6-stub fill and its overload_failure_stub private-clone
    /// containment machinery retired with it. The M5-review C5
    /// carry-in resolved the same commit: checkMode threads from
    /// getCandidateForOverloadFailure into the failure inference,
    /// ORed with SkipContextSensitive | SkipGenericFunctions.)
    fn pick_longest_candidate_signature(
        &mut self,
        node: NodeId,
        ctx: &mut ResolveCallCtx,
        check_mode: CheckMode,
    ) -> CheckResult<SignatureId> {
        let args_count = self.apparent_argument_count.unwrap_or(ctx.args.len());
        let best_index = self.get_longest_candidate_index(&ctx.candidates, args_count)?;
        let candidate = ctx.candidates[best_index];
        let Some(type_parameters) = self.signature_of(candidate).type_parameters.clone() else {
            return Ok(candidate);
        };
        let instantiated = if !ctx.type_argument_nodes.is_empty() {
            let type_argument_nodes = ctx.type_argument_nodes.clone();
            let is_javascript = self.is_in_js_file(node);
            let type_arguments = self.get_type_arguments_from_nodes(
                &type_argument_nodes,
                &type_parameters,
                is_javascript,
            )?;
            self.create_signature_instantiation(candidate, Some(&type_arguments))?
        } else {
            // 76946-76955: inferSignatureInstantiationForOverloadFailure.
            let args = ctx.args.clone();
            let flags = if self.is_in_js_file(node) {
                InferenceFlags::ANY_DEFAULT
            } else {
                InferenceFlags::NONE
            };
            let context =
                self.create_inference_context(&type_parameters, Some(candidate), flags, None);
            let type_argument_types = self.infer_type_arguments(
                node,
                candidate,
                &args,
                check_mode | CheckMode::SKIP_CONTEXT_SENSITIVE | CheckMode::SKIP_GENERIC_FUNCTIONS,
                context,
            )?;
            self.create_signature_instantiation(candidate, Some(&type_argument_types))?
        };
        ctx.candidates[best_index] = instantiated;
        Ok(instantiated)
    }

    /// tsc-port: getTypeArgumentsFromNodes @6.0.3
    /// tsc-hash: e42b94a48cb077bb4c85ccc9efd4acbf62ac12a8db1efbdf320901a5d0437865
    /// tsc-span: _tsc.js:76936-76945
    ///
    /// The default → constraint → getDefaultTypeArgumentType(isJs)
    /// fill here is tsc's REAL code for explicit-typearg failure
    /// candidates (not the M6 stub); isJs = isInJSFile(node) at the
    /// 76931 caller (any in JS files, unknown otherwise — dormant
    /// until the checkJs band, M8; threaded by the 7.4 review like
    /// the 76821 twin).
    fn get_type_arguments_from_nodes(
        &mut self,
        type_argument_nodes: &[NodeId],
        type_parameters: &[TypeId],
        is_javascript: bool,
    ) -> CheckResult<Vec<TypeId>> {
        let mut type_arguments: Vec<TypeId> = Vec::with_capacity(type_argument_nodes.len());
        for &node in type_argument_nodes {
            type_arguments.push(self.get_type_from_type_node(node)?);
        }
        while type_arguments.len() > type_parameters.len() {
            type_arguments.pop();
        }
        while type_arguments.len() < type_parameters.len() {
            let type_parameter = type_parameters[type_arguments.len()];
            let default_type_argument = if is_javascript {
                self.tables.intrinsics.any
            } else {
                self.tables.intrinsics.unknown
            };
            let ty = match self.get_default_from_type_parameter(type_parameter)? {
                Some(default) => default,
                None => self
                    .get_constraint_of_type_parameter(type_parameter)?
                    .unwrap_or(default_type_argument),
            };
            type_arguments.push(ty);
        }
        Ok(type_arguments)
    }

    /// tsc-port: getLongestCandidateIndex @6.0.3
    /// tsc-hash: 6cc04912575b8b07783bb427f4dc10ead8dbe05e659a452d2dfda194d22c6efb
    /// tsc-span: _tsc.js:76956-76971
    fn get_longest_candidate_index(
        &mut self,
        candidates: &[SignatureId],
        args_count: usize,
    ) -> CheckResult<usize> {
        let mut max_params_index: usize = 0;
        let mut max_params: isize = -1;
        for (i, &candidate) in candidates.iter().enumerate() {
            let param_count = self.get_parameter_count(candidate)?;
            if self.has_effective_rest_parameter(candidate)? || param_count >= args_count {
                return Ok(i);
            }
            if param_count as isize > max_params {
                max_params = param_count as isize;
                max_params_index = i;
            }
        }
        Ok(max_params_index)
    }

    /// tsc-port: createUnionOfSignaturesForOverloadFailure @6.0.3
    /// tsc-hash: 1fe0405cb7d3f33b2f768c339467f0b7db03798810ca3329b03ecb9a780d1c20
    /// tsc-span: _tsc.js:76876-76913
    ///
    /// (getNumNonRestParameters 76914-76917 and the combined-symbol
    /// helpers 76918-76923 folded in.)
    fn create_union_of_signatures_for_overload_failure(
        &mut self,
        candidates: &[SignatureId],
    ) -> CheckResult<SignatureId> {
        let this_parameters: Vec<SymbolId> = candidates
            .iter()
            .filter_map(|&c| self.signature_of(c).this_parameter)
            .collect();
        let mut this_parameter: Option<SymbolId> = None;
        if !this_parameters.is_empty() {
            let mut types = Vec::with_capacity(this_parameters.len());
            for &parameter in &this_parameters {
                types.push(self.get_type_of_parameter(parameter)?);
            }
            let unioned = self.get_union_type_ex(&types, UnionReduction::Subtype)?;
            this_parameter = Some(self.create_symbol_with_type(this_parameters[0], Some(unioned)));
        }
        let num_non_rest: Vec<usize> = candidates
            .iter()
            .map(|&c| {
                let data = self.signature_of(c);
                data.parameters.len()
                    - usize::from(data.flags.intersects(SignatureFlags::HAS_REST_PARAMETER))
            })
            .collect();
        let min_argument_count = candidates
            .iter()
            .map(|&c| self.signature_of(c).min_argument_count)
            .min()
            .expect("non-empty candidates");
        let max_non_rest_param = num_non_rest.iter().copied().max().expect("non-empty");
        let mut parameters: Vec<SymbolId> = Vec::with_capacity(max_non_rest_param);
        for i in 0..max_non_rest_param {
            let symbols: Vec<SymbolId> = candidates
                .iter()
                .filter_map(|&s| {
                    let data = self.signature_of(s);
                    if data.flags.intersects(SignatureFlags::HAS_REST_PARAMETER) {
                        if i < data.parameters.len() - 1 {
                            Some(data.parameters[i])
                        } else {
                            data.parameters.last().copied()
                        }
                    } else if i < data.parameters.len() {
                        Some(data.parameters[i])
                    } else {
                        None
                    }
                })
                .collect();
            debug_assert!(!symbols.is_empty());
            let mut types: Vec<TypeId> = Vec::new();
            for &candidate in candidates {
                if let Some(ty) = self.try_get_type_at_position(candidate, i)? {
                    types.push(ty);
                }
            }
            let unioned = self.get_union_type_ex(&types, UnionReduction::Subtype)?;
            parameters.push(self.create_symbol_with_type(symbols[0], Some(unioned)));
        }
        let rest_parameter_symbols: Vec<SymbolId> = candidates
            .iter()
            .filter_map(|&c| {
                let data = self.signature_of(c);
                if data.flags.intersects(SignatureFlags::HAS_REST_PARAMETER) {
                    data.parameters.last().copied()
                } else {
                    None
                }
            })
            .collect();
        let mut flags = SignatureFlags::IS_SIGNATURE_CANDIDATE_FOR_OVERLOAD_FAILURE;
        if !rest_parameter_symbols.is_empty() {
            let mut rest_types: Vec<TypeId> = Vec::new();
            for &candidate in candidates {
                if let Some(rest) = self.try_get_rest_type_of_signature(candidate)? {
                    rest_types.push(rest);
                }
            }
            let unioned = self.get_union_type_ex(&rest_types, UnionReduction::Subtype)?;
            let array = self.create_array_type(unioned, /*readonly*/ false)?;
            parameters.push(self.create_symbol_with_type(rest_parameter_symbols[0], Some(array)));
            flags =
                SignatureFlags::from_bits(flags.bits() | SignatureFlags::HAS_REST_PARAMETER.bits());
        }
        if candidates.iter().any(|&c| {
            self.signature_of(c)
                .flags
                .intersects(SignatureFlags::HAS_LITERAL_TYPES)
        }) {
            flags =
                SignatureFlags::from_bits(flags.bits() | SignatureFlags::HAS_LITERAL_TYPES.bits());
        }
        let mut return_types: Vec<TypeId> = Vec::with_capacity(candidates.len());
        for &candidate in candidates {
            return_types.push(self.get_return_type_of_signature(candidate)?);
        }
        let return_type =
            self.get_intersection_type(&return_types, tsc_types::IntersectionFlags::NONE)?;
        let first = self.signature_of(candidates[0]).clone();
        Ok(self.alloc_signature(Signature {
            declaration: first.declaration,
            flags,
            type_parameters: None,
            parameters,
            this_parameter,
            min_argument_count,
            resolved_return_type: LinkSlot::Resolved(return_type),
            from_method: first.from_method,
            target: None,
            mapper: None,
            instantiations: std::collections::HashMap::new(),
            erased_signature_cache: None,
            canonical_signature_cache: None,
            base_signature_cache: None,
            composite_kind: None,
            composite_signatures: None,
            optional_call_signature_cache: (None, None),
            isolated_signature_kind: first.isolated_signature_kind,
            isolated_signature_type: None,
        }))
    }

    /// tsc-port: tryGetRestTypeOfSignature @6.0.3
    /// tsc-hash: 0be56e511e900fd0aa622d918e53b2c5e132254bf3b61e8ad25be72950ff7728
    /// tsc-span: _tsc.js:59878-59885
    fn try_get_rest_type_of_signature(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult<Option<TypeId>> {
        let data = self.signature_of(signature);
        if !data.flags.intersects(SignatureFlags::HAS_REST_PARAMETER) {
            return Ok(None);
        }
        let rest_parameter = *data
            .parameters
            .last()
            .expect("rest-parameter signatures have parameters");
        let sig_rest_type = self.get_type_of_symbol(rest_parameter)?;
        let rest_type = if self.tables.is_tuple_type(sig_rest_type) {
            match self.get_rest_type_of_tuple_type(sig_rest_type)? {
                Some(rest) => rest,
                None => return Ok(None),
            }
        } else {
            sig_rest_type
        };
        self.get_index_type_of_type(rest_type, self.tables.intrinsics.number)
    }

    // ---- arity errors ----

    /// tsc-port: isPromiseResolveArityError @6.0.3
    /// tsc-hash: 6bba0fd86e72d239399c30338e816833a9ed67ed3cff895e44386acae0c8d48e
    /// tsc-span: _tsc.js:76407-76433
    ///
    /// The callee resolves to a parameter of a function-expression
    /// directly under `new <globalPromiseSymbol>`; getSymbolAtLocation
    /// on the constructor identifier reduces to the same resolveName
    /// probe for the identifier-callee shape this predicate demands.
    fn is_promise_resolve_arity_error(&mut self, node: NodeId) -> CheckResult<bool> {
        let NodeData::CallExpression(data) = self.data_of(node) else {
            return Ok(false);
        };
        let Some(callee) = data.expression else {
            return Ok(false);
        };
        if self.kind_of(callee) != SyntaxKind::Identifier {
            return Ok(false);
        }
        let callee_text = match self.identifier_text_of(callee) {
            Some(text) => text.to_owned(),
            None => return Ok(false),
        };
        let symbol = self.resolve_name(
            Some(callee),
            &callee_text,
            SymbolFlags::VALUE,
            /*name_not_found_message*/ None,
            /*is_use*/ false,
            /*exclude_globals*/ false,
        )?;
        let Some(symbol) = symbol else {
            return Ok(false);
        };
        let Some(decl) = self.binder.symbol(symbol).value_declaration else {
            return Ok(false);
        };
        if self.kind_of(decl) != SyntaxKind::Parameter {
            return Ok(false);
        }
        let Some(func) = self.parent_of(decl) else {
            return Ok(false);
        };
        if !matches!(
            self.kind_of(func),
            SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
        ) {
            return Ok(false);
        }
        let Some(new_expr) = self.parent_of(func) else {
            return Ok(false);
        };
        let NodeData::NewExpression(new_data) = self.data_of(new_expr) else {
            return Ok(false);
        };
        let Some(ctor) = new_data.expression else {
            return Ok(false);
        };
        if self.kind_of(ctor) != SyntaxKind::Identifier {
            return Ok(false);
        }
        let Some(global_promise) = self.get_global_promise_constructor_symbol(false)? else {
            return Ok(false);
        };
        let ctor_text = match self.identifier_text_of(ctor) {
            Some(text) => text.to_owned(),
            None => return Ok(false),
        };
        let ctor_symbol = self.resolve_name(
            Some(ctor),
            &ctor_text,
            SymbolFlags::VALUE,
            None,
            false,
            false,
        )?;
        Ok(ctor_symbol == Some(global_promise))
    }

    /// tsc-port: getArgumentArityError @6.0.3
    /// tsc-hash: 7584a2739d127f4143461c80dcc282d90a4308c3b7effba7add87f93001a2007
    /// tsc-span: _tsc.js:76434-76520
    ///
    /// Count-only payloads — the whole band is display-free. Decorator
    /// flavors are 5.8. A head message (instanceof 2860) chains
    /// outermost at all three report shapes (76464/76479/76509). The
    /// JS Promise-hint flavor is JS-file-gated.
    fn get_argument_arity_error(
        &mut self,
        node: NodeId,
        signatures: &[SignatureId],
        args: &[EffectiveArg],
        head_message: Option<&'static DiagnosticMessage>,
    ) -> CheckResult<Diagnostic> {
        let wrap_in_head = |chain: MessageChain| match head_message {
            Some(head) => MessageChain::new(head, &[]).with_next(vec![chain]),
            None => chain,
        };
        if let Some(spread_index) = self.get_spread_argument_index(args) {
            let span = self.diag_span_of_effective_arg(node, &args[spread_index]);
            return Ok(self.diagnostic_at_span(
                &span,
                MessageChain::new(
                    &diagnostics::A_spread_argument_must_either_have_a_tuple_type_or_be_passed_to_a_rest_parameter,
                    &[],
                ),
            ));
        }
        let mut min = usize::MAX;
        let mut max: usize = 0;
        let mut max_below: Option<usize> = None;
        let mut min_above: Option<usize> = None;
        let mut closest_signature: Option<SignatureId> = None;
        for &signature in signatures {
            let min_parameter = self.get_min_argument_count(signature)?;
            let max_parameter = self.get_parameter_count(signature)?;
            if min_parameter < min {
                min = min_parameter;
                closest_signature = Some(signature);
            }
            max = std::cmp::max(max, max_parameter);
            if min_parameter < args.len() && max_below.is_none_or(|below| min_parameter > below) {
                max_below = Some(min_parameter);
            }
            if args.len() < max_parameter && min_above.is_none_or(|above| max_parameter < above) {
                min_above = Some(max_parameter);
            }
        }
        let mut has_rest_parameter = false;
        for &signature in signatures {
            if self.has_effective_rest_parameter(signature)? {
                has_rest_parameter = true;
                break;
            }
        }
        let parameter_range = if has_rest_parameter {
            min.to_string()
        } else if min < max {
            format!("{min}-{max}")
        } else {
            min.to_string()
        };
        let is_void_promise_error = !has_rest_parameter
            && parameter_range == "1"
            && args.is_empty()
            && self.is_promise_resolve_arity_error(node)?;
        let error_message: &'static DiagnosticMessage = if self.kind_of(node)
            == SyntaxKind::Decorator
        {
            if has_rest_parameter {
                &diagnostics::The_runtime_will_invoke_the_decorator_with_1_arguments_but_the_decorator_expects_at_least_0
            } else {
                &diagnostics::The_runtime_will_invoke_the_decorator_with_1_arguments_but_the_decorator_expects_0
            }
        } else if has_rest_parameter {
            &diagnostics::Expected_at_least_0_arguments_but_got_1
        } else if is_void_promise_error {
            &diagnostics::Expected_0_arguments_but_got_1_Did_you_forget_to_include_void_in_your_type_argument_to_Promise
        } else {
            &diagnostics::Expected_0_arguments_but_got_1
        };
        let arg_count_text = args.len().to_string();
        if min < args.len() && args.len() < max {
            // 76463-76476: between the overload boundaries.
            let span = self.diag_span_for_call_node(node);
            let max_below = max_below.expect("between-range implies a below bound");
            let min_above = min_above.expect("between-range implies an above bound");
            let chain = wrap_in_head(MessageChain::new(
                &diagnostics::No_overload_expects_0_arguments_but_overloads_do_exist_that_expect_either_1_or_2_arguments,
                &[
                    arg_count_text,
                    max_below.to_string(),
                    min_above.to_string(),
                ],
            ));
            return Ok(self.diagnostic_at_span(&span, chain));
        }
        if args.len() < min {
            let span = self.diag_span_for_call_node(node);
            let chain = wrap_in_head(MessageChain::new(
                error_message,
                &[parameter_range, arg_count_text],
            ));
            let mut diagnostic = self.diagnostic_at_span(&span, chain);
            // 76492-76497: the "argument not provided" related row on
            // the closest signature's missing parameter.
            if let Some(declaration) =
                closest_signature.and_then(|s| self.signature_of(s).declaration)
            {
                let has_this = closest_signature
                    .is_some_and(|s| self.signature_of(s).this_parameter.is_some());
                let parameter_index = if has_this { args.len() + 1 } else { args.len() };
                let parameters = match self.data_of(declaration) {
                    NodeData::FunctionDeclaration(data) => data.parameters,
                    NodeData::FunctionExpression(data) => data.parameters,
                    NodeData::ArrowFunction(data) => data.parameters,
                    NodeData::MethodDeclaration(data) => data.parameters,
                    NodeData::MethodSignature(data) => data.parameters,
                    NodeData::CallSignature(data) => data.parameters,
                    NodeData::ConstructSignature(data) => data.parameters,
                    NodeData::FunctionType(data) => data.parameters,
                    NodeData::JSDocFunctionType(data) => data.parameters,
                    NodeData::JSDocSignature(data) => data.parameters,
                    NodeData::ConstructorType(data) => data.parameters,
                    NodeData::Constructor(data) => data.parameters,
                    _ => None,
                };
                let parameter = self.nodes_of(parameters).get(parameter_index).copied();
                if let Some(parameter) = parameter {
                    let related = self.argument_not_provided_related(parameter, args.len())?;
                    diagnostic.related.push(related);
                }
            }
            return Ok(diagnostic);
        }
        // 76499-76519: over max — the excess-args range (end==pos bump).
        let pos = self.effective_arg_pos(&args[max]);
        let mut end = self.effective_arg_end(&args[args.len() - 1]);
        if end == pos {
            end += 1;
        }
        let span = self.diag_span_of_byte_range(node, pos, end);
        let chain = wrap_in_head(MessageChain::new(
            error_message,
            &[parameter_range, arg_count_text],
        ));
        Ok(self.diagnostic_at_span(&span, chain))
    }

    /// tsc-port: getArgumentArityError @6.0.3
    /// tsc-hash: b93c4ab581d5b6e865ac4ae75cb83cc787012677392b4428ad1095375cf63751
    /// tsc-span: _tsc.js:76492-76497
    ///
    /// Binding patterns and rest parameters select their own rows.
    /// An unnamed parameter (the JSDoc `function(string)` form) uses
    /// the missing argument's zero-based index as its display name.
    fn argument_not_provided_related(
        &mut self,
        parameter: NodeId,
        argument_index: usize,
    ) -> CheckResult<RelatedInfo> {
        let (name, is_rest) = match self.data_of(parameter) {
            NodeData::Parameter(data) => (data.name, data.dot_dot_dot_token.is_some()),
            NodeData::JSDocParameterTag(data) => {
                (data.name, self.is_rest_parameter_declaration(parameter))
            }
            _ => unreachable!("signature declarations carry parameter-like nodes"),
        };
        let name_kind = name.map(|name| self.kind_of(name));
        if matches!(
            name_kind,
            Some(SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern)
        ) {
            return Ok(self.related_info_for_node(
                parameter,
                &diagnostics::An_argument_matching_this_binding_pattern_was_not_provided,
                &[],
            ));
        }
        let name_text = name
            .and_then(|name| self.identifier_text_of(name))
            .map(str::to_owned);
        if is_rest {
            let text = name_text.unwrap_or_default();
            return Ok(self.related_info_for_node(
                parameter,
                &diagnostics::Arguments_for_the_rest_parameter_0_were_not_provided,
                &[&text],
            ));
        }
        let text = name_text.unwrap_or_else(|| argument_index.to_string());
        Ok(self.related_info_for_node(
            parameter,
            &diagnostics::An_argument_for_0_was_not_provided,
            &[&text],
        ))
    }

    /// tsc-port: getTypeArgumentArityError @6.0.3
    /// tsc-hash: 6ed32b61094692b28f2f33ddd7c2c03c8a86d35230e4008c87ac294ac74100a8
    /// tsc-span: _tsc.js:76521-76578
    ///
    /// headMessage chains are decorator-only (5.8; instanceof skips
    /// type arguments — see the ladder's debug_assert). The span is
    /// the typeArguments node-array range in every arm.
    fn get_type_argument_arity_error(
        &mut self,
        node: NodeId,
        signatures: &[SignatureId],
        type_arguments_array: Option<NodeArrayId>,
        type_argument_nodes: &[NodeId],
    ) -> CheckResult<Diagnostic> {
        let arg_count = type_argument_nodes.len();
        let span = match type_arguments_array {
            Some(array) => {
                let source = self.binder.source_of_node(node);
                let array = source.arena.node_array(array);
                self.diag_span_of_byte_range(node, array.pos, array.end)
            }
            None => self.diag_span_of_node(node),
        };
        if signatures.len() == 1 {
            let signature = signatures[0];
            let type_parameters = self.signature_of(signature).type_parameters.clone();
            let min = self.get_min_type_argument_count(type_parameters.as_deref());
            let max = type_parameters.as_deref().map_or(0, <[TypeId]>::len);
            let range = if min < max {
                format!("{min}-{max}")
            } else {
                min.to_string()
            };
            return Ok(self.diagnostic_at_span(
                &span,
                MessageChain::new(
                    &diagnostics::Expected_0_type_arguments_but_got_1,
                    &[range, arg_count.to_string()],
                ),
            ));
        }
        let mut below_arg_count: Option<usize> = None;
        let mut above_arg_count: Option<usize> = None;
        for &signature in signatures {
            let type_parameters = self.signature_of(signature).type_parameters.clone();
            let min = self.get_min_type_argument_count(type_parameters.as_deref());
            let max = type_parameters.as_deref().map_or(0, <[TypeId]>::len);
            if min > arg_count {
                above_arg_count = Some(above_arg_count.map_or(min, |above| above.min(min)));
            } else if max < arg_count {
                below_arg_count = Some(below_arg_count.map_or(max, |below| below.max(max)));
            }
        }
        if let (Some(below), Some(above)) = (below_arg_count, above_arg_count) {
            return Ok(self.diagnostic_at_span(
                &span,
                MessageChain::new(
                    &diagnostics::No_overload_expects_0_type_arguments_but_overloads_do_exist_that_expect_either_1_or_2_type_arguments,
                    &[arg_count.to_string(), below.to_string(), above.to_string()],
                ),
            ));
        }
        let boundary = below_arg_count.or(above_arg_count).unwrap_or(0);
        Ok(self.diagnostic_at_span(
            &span,
            MessageChain::new(
                &diagnostics::Expected_0_type_arguments_but_got_1,
                &[boundary.to_string(), arg_count.to_string()],
            ),
        ))
    }

    // ---- per-kind resolvers ----

    /// tsc-port: isUntypedFunctionCall @6.0.3
    /// tsc-hash: 2353c2c317bde5a830b031cc38da3caee3d391b53a8e95bae70d601f17a12321
    /// tsc-span: _tsc.js:77052-77054
    fn is_untyped_function_call(
        &mut self,
        func_type: TypeId,
        apparent_func_type: TypeId,
        num_call_signatures: usize,
        num_construct_signatures: usize,
    ) -> CheckResult<bool> {
        if self.tables.flags_of(func_type).intersects(TypeFlags::ANY) {
            return Ok(true);
        }
        if self
            .tables
            .flags_of(apparent_func_type)
            .intersects(TypeFlags::ANY)
            && self
                .tables
                .flags_of(func_type)
                .intersects(TypeFlags::TYPE_PARAMETER)
        {
            return Ok(true);
        }
        if num_call_signatures != 0 || num_construct_signatures != 0 {
            return Ok(false);
        }
        if self
            .tables
            .flags_of(apparent_func_type)
            .intersects(TypeFlags::UNION)
        {
            return Ok(false);
        }
        let reduced = self.get_reduced_type(apparent_func_type)?;
        if self.tables.flags_of(reduced).intersects(TypeFlags::NEVER) {
            return Ok(false);
        }
        let global_function = self.global_function_type()?;
        self.is_type_assignable_to(func_type, global_function)
    }

    /// tsrs-native: Rust return-shape adapter for tsc's
    /// invocationErrorDetails, whose ledger block is folded into
    /// invocation_error below.
    fn invocation_error_details(
        &mut self,
        error_target: NodeId,
        apparent_type: TypeId,
        kind: SignatureKind,
    ) -> CheckResult<(MessageChain, Option<&'static DiagnosticMessage>)> {
        fn prepend(
            details: Option<MessageChain>,
            message: &'static DiagnosticMessage,
            args: &[String],
        ) -> MessageChain {
            MessageChain::new(message, args).with_next(details.into_iter().collect())
        }

        let is_call = kind == SignatureKind::Call;
        let awaited = self.get_awaited_type_probe(apparent_type)?;
        let maybe_missing_await = match awaited {
            Some(awaited) => !self.get_signatures_of_type(awaited, kind)?.is_empty(),
            None => false,
        };

        let union_types = match &self.tables.type_of(apparent_type).data {
            TypeData::Union { types, .. } => Some(types.to_vec()),
            _ => None,
        };
        let error_info = if let Some(types) = union_types {
            let apparent_text = self.type_to_string_slice(apparent_type)?;
            let mut error_info = None;
            let mut has_signatures = false;
            for constituent in types {
                let signatures = self.get_signatures_of_type(constituent, kind)?;
                if !signatures.is_empty() {
                    has_signatures = true;
                    if error_info.is_some() {
                        break;
                    }
                } else {
                    if error_info.is_none() {
                        let constituent_text = self.type_to_string_slice(constituent)?;
                        error_info = Some(prepend(
                            error_info,
                            if is_call {
                                &diagnostics::Type_0_has_no_call_signatures
                            } else {
                                &diagnostics::Type_0_has_no_construct_signatures
                            },
                            &[constituent_text],
                        ));
                        error_info = Some(prepend(
                            error_info,
                            if is_call {
                                &diagnostics::Not_all_constituents_of_type_0_are_callable
                            } else {
                                &diagnostics::Not_all_constituents_of_type_0_are_constructable
                            },
                            std::slice::from_ref(&apparent_text),
                        ));
                    }
                    if has_signatures {
                        break;
                    }
                }
            }
            if !has_signatures {
                Some(prepend(
                    None,
                    if is_call {
                        &diagnostics::No_constituent_of_type_0_is_callable
                    } else {
                        &diagnostics::No_constituent_of_type_0_is_constructable
                    },
                    &[apparent_text],
                ))
            } else if error_info.is_none() {
                Some(prepend(
                    None,
                    if is_call {
                        &diagnostics::Each_member_of_the_union_type_0_has_signatures_but_none_of_those_signatures_are_compatible_with_each_other
                    } else {
                        &diagnostics::Each_member_of_the_union_type_0_has_construct_signatures_but_none_of_those_signatures_are_compatible_with_each_other
                    },
                    &[apparent_text],
                ))
            } else {
                error_info
            }
        } else {
            let apparent_text = self.type_to_string_slice(apparent_type)?;
            Some(prepend(
                None,
                if is_call {
                    &diagnostics::Type_0_has_no_call_signatures
                } else {
                    &diagnostics::Type_0_has_no_construct_signatures
                },
                &[apparent_text],
            ))
        };

        let mut head: &'static DiagnosticMessage = if is_call {
            &diagnostics::This_expression_is_not_callable
        } else {
            &diagnostics::This_expression_is_not_constructable
        };
        let parent = self.parent_of(error_target);
        let parent_call_args = parent.and_then(|parent| match self.data_of(parent) {
            NodeData::CallExpression(data) => Some(self.nodes_of(data.arguments).len()),
            _ => None,
        });
        if parent_call_args == Some(0) {
            if let LinkSlot::Resolved(resolved_symbol) =
                self.links.node(error_target).resolved_symbol
            {
                if self
                    .binder
                    .symbol(resolved_symbol)
                    .flags
                    .intersects(SymbolFlags::GET_ACCESSOR)
                {
                    head = &diagnostics::This_expression_is_not_callable_because_it_is_a_get_accessor_Did_you_mean_to_use_it_without;
                }
            }
        }
        Ok((
            prepend(error_info, head, &[]),
            maybe_missing_await.then_some(&diagnostics::Did_you_forget_to_use_await),
        ))
    }

    /// tsc-port: invocationError @6.0.3 (invocationErrorDetails folded in)
    /// tsc-hash: f2d2133394f805817e33a6c4b1534917ab876d99027c097b8c1f6d172778d90b
    /// tsc-span: _tsc.js:77167-77247
    ///
    /// Related rows: the await hint (2773); invocationErrorRecovery
    /// 7038 rides the unmodeled originatingImport link (absent =
    /// attach-only, safe).
    fn invocation_error(
        &mut self,
        error_target: NodeId,
        apparent_type: TypeId,
        kind: SignatureKind,
        related_information: Option<RelatedInfo>,
    ) -> CheckResult<()> {
        let (chain, related_message) =
            self.invocation_error_details(error_target, apparent_type, kind)?;
        let parent = self.parent_of(error_target);
        // 77240-77244: the span override inside call parents.
        let span =
            if parent.is_some_and(|parent| self.kind_of(parent) == SyntaxKind::CallExpression) {
                self.diag_span_for_call_node(parent.expect("checked above"))
            } else {
                self.diag_span_of_node(error_target)
            };
        let mut diagnostic = self.diagnostic_at_span(&span, chain);
        if let Some(related_message) = related_message {
            diagnostic
                .related
                .push(self.related_info_for_node(error_target, related_message, &[]));
        }
        if let Some(related) = related_information {
            diagnostic.related.push(related);
        }
        self.push_error_diagnostic(diagnostic);
        Ok(())
    }

    /// tsc-port: resolveCallExpression @6.0.3
    /// tsc-hash: 80e582aa9064a2e37878a85900269bfeb17fa3b61b12cb8e5697a910d13c0b73
    /// tsc-span: _tsc.js:76972-77048
    fn resolve_call_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<SignatureId> {
        let NodeData::CallExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let expression = data.expression.expect(
            "parser invariant: parse_call_expression_rest always stores its callee \
             (parse recovery stores a missing expression node)",
        );
        let type_arguments = data.type_arguments;
        if self.kind_of(expression) == SyntaxKind::SuperKeyword {
            // 76973-76989: the super() arm.
            let super_type = self.check_super_expression(expression)?;
            if self.tables.flags_of(super_type).intersects(TypeFlags::ANY) {
                let arguments = match self.data_of(node) {
                    NodeData::CallExpression(data) => data.arguments,
                    _ => None,
                };
                for argument in self.nodes_of(arguments) {
                    self.check_expression(argument, CheckMode::NORMAL)?;
                }
                return Ok(self.any_signature);
            }
            if super_type != self.tables.intrinsics.error {
                let base_type_node = self
                    .get_containing_class_of(node)
                    .and_then(|class| self.get_effective_base_type_node(class));
                if let Some(base_type_node) = base_type_node {
                    let base_constructors = self.get_instantiated_constructors_for_type_arguments(
                        super_type,
                        base_type_node,
                    )?;
                    return self.resolve_call(
                        node,
                        &base_constructors,
                        check_mode,
                        SignatureFlags::NONE,
                        None,
                    );
                }
            }
            return self.resolve_untyped_call(node);
        }
        let mut func_type = self.check_expression(expression, CheckMode::NORMAL)?;
        // 76990-76998: call-chain flags.
        let source = self.binder.source_of_node(node);
        let call_chain_flags = if node_util::is_optional_chain(source, node) {
            let non_optional_type = self.get_optional_expression_type(func_type, expression)?;
            if non_optional_type == func_type {
                SignatureFlags::NONE
            } else {
                let flags = if node_util::is_outermost_optional_chain(
                    self.binder.source_of_node(node),
                    node,
                ) {
                    SignatureFlags::IS_OUTER_CALL_CHAIN
                } else {
                    SignatureFlags::IS_INNER_CALL_CHAIN
                };
                func_type = non_optional_type;
                flags
            }
        } else {
            SignatureFlags::NONE
        };
        let func_type = self.check_non_null_type_with_reporter(
            func_type,
            expression,
            Self::report_cannot_invoke_possibly_null_or_undefined_error,
        )?;
        if func_type == self.tables.intrinsics.silent_never {
            return Ok(self.silent_never_signature);
        }
        let apparent_type = self.get_apparent_type(func_type)?;
        if apparent_type == self.tables.intrinsics.error {
            return self.resolve_error_call(node);
        }
        let call_signatures = self.get_signatures_of_type(apparent_type, SignatureKind::Call)?;
        let num_construct_signatures = self
            .get_signatures_of_type(apparent_type, SignatureKind::Construct)?
            .len();
        if self.is_untyped_function_call(
            func_type,
            apparent_type,
            call_signatures.len(),
            num_construct_signatures,
        )? {
            // (The [FLOW M5] auto-callee gate retired at 6.6f: the
            // flipped initialType ladders flow `var a;` callees to
            // real undefined, so checkNonNullExpression's 2722 face
            // forms exactly like tsc's.)
            // 77014-77016: 2347 on non-error targets with typeArguments.
            if func_type != self.tables.intrinsics.error && type_arguments.is_some() {
                self.error_at(
                    Some(node),
                    &diagnostics::Untyped_function_calls_may_not_accept_type_arguments,
                    &[],
                );
            }
            return self.resolve_untyped_call(node);
        }
        if call_signatures.is_empty() {
            if num_construct_signatures != 0 {
                let display = self.type_to_string_slice(func_type)?;
                self.error_at(
                    Some(node),
                    &diagnostics::Value_of_type_0_is_not_callable_Did_you_mean_to_include_new,
                    &[&display],
                );
            } else {
                // 77023-77034: the missing-semicolon hint on a
                // single-argument call whose argument opens on a new
                // line.
                let mut related_information: Option<RelatedInfo> = None;
                let arguments = self.nodes_of(match self.data_of(node) {
                    NodeData::CallExpression(data) => data.arguments,
                    _ => None,
                });
                if arguments.len() == 1 {
                    let source = self.binder.source_of_node(node);
                    let callee_end = source.arena.node(expression).end as usize;
                    if line_break_precedes_next_token(source.text(), callee_end) {
                        related_information = Some(self.related_info_for_node(
                            expression,
                            &diagnostics::Are_you_missing_a_semicolon,
                            &[],
                        ));
                    }
                }
                self.invocation_error(
                    expression,
                    apparent_type,
                    SignatureKind::Call,
                    related_information,
                )?;
            }
            return self.resolve_error_call(node);
        }
        // 77039-77042: the SkipGenericFunctions defer — a generic
        // signature returning a function type is skipped during the
        // first inference pass (the sentinel result is load-bearing:
        // 72918's contextual read and the 77616/79572 silentNever
        // consumers key on the links slot staying Resolving).
        if check_mode.intersects(CheckMode::SKIP_GENERIC_FUNCTIONS) && type_arguments.is_none() {
            let mut any_generic_returning_function = false;
            for &signature in &call_signatures {
                if self.is_generic_function_returning_function(signature)? {
                    any_generic_returning_function = true;
                    break;
                }
            }
            if any_generic_returning_function {
                self.skipped_generic_function(node, check_mode);
                return Ok(self.resolving_signature);
            }
        }
        // 77043-77046: a callable declaration carrying JSDoc
        // `@class`/`@constructor` must be invoked with `new`.
        let mut has_jsdoc_class_signature = false;
        for &signature in &call_signatures {
            let Some(declaration) = self.signature_of(signature).declaration else {
                continue;
            };
            if self.is_in_js_file(declaration)
                && self
                    .first_jsdoc_tag(declaration, SyntaxKind::JSDocClassTag)
                    .is_some()
            {
                has_jsdoc_class_signature = true;
                break;
            }
        }
        if has_jsdoc_class_signature {
            let display = self.type_to_string_slice(func_type)?;
            self.error_at(
                Some(node),
                &diagnostics::Value_of_type_0_is_not_callable_Did_you_mean_to_include_new,
                &[&display],
            );
            return self.resolve_error_call(node);
        }
        self.resolve_call(node, &call_signatures, check_mode, call_chain_flags, None)
    }

    /// tsc-port: isGenericFunctionReturningFunction @6.0.3
    /// tsc-hash: 07821b6d14f8a88cba21ff55612b2992b7b6b8bcfe7d56bf1b200800469a1dc2
    /// tsc-span: _tsc.js:77049-77051
    fn is_generic_function_returning_function(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult<bool> {
        if self.signature_of(signature).type_parameters.is_none() {
            return Ok(false);
        }
        let return_type = self.get_return_type_of_signature(signature)?;
        self.is_function_type(return_type)
    }

    /// tsc-port: resolveNewExpression @6.0.3
    /// tsc-hash: 1d3882b681eb1a6defdf1901381e33d6091c5bfc756487595475c65db0511b41
    /// tsc-span: _tsc.js:77055-77101
    ///
    /// The 2350/2679 tail is dead under the strict default but live
    /// under noImplicitAny:false directives.
    fn resolve_new_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<SignatureId> {
        let NodeData::NewExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let expression = data.expression.expect(
            "parser invariant: parse_new_expression_stub always stores its callee \
             (parse recovery stores a missing Identifier)",
        );
        let type_arguments = data.type_arguments;
        let expression_type = self.check_non_null_expression(expression)?;
        if expression_type == self.tables.intrinsics.silent_never {
            return Ok(self.silent_never_signature);
        }
        let expression_type = self.get_apparent_type(expression_type)?;
        if expression_type == self.tables.intrinsics.error {
            return self.resolve_error_call(node);
        }
        if self
            .tables
            .flags_of(expression_type)
            .intersects(TypeFlags::ANY)
        {
            if type_arguments.is_some() {
                self.error_at(
                    Some(node),
                    &diagnostics::Untyped_function_calls_may_not_accept_type_arguments,
                    &[],
                );
            }
            return self.resolve_untyped_call(node);
        }
        let construct_signatures =
            self.get_signatures_of_type(expression_type, SignatureKind::Construct)?;
        if !construct_signatures.is_empty() {
            if !self.is_constructor_accessible(node, construct_signatures[0])? {
                return self.resolve_error_call(node);
            }
            // 77075-77083: abstract construct signatures and abstract
            // class modifiers.
            let mut some_abstract = false;
            for &signature in &construct_signatures {
                if self.some_signature_has_flags(signature, SignatureFlags::ABSTRACT) {
                    some_abstract = true;
                    break;
                }
            }
            if some_abstract {
                self.error_at(
                    Some(node),
                    &diagnostics::Cannot_create_an_instance_of_an_abstract_class,
                    &[],
                );
                return self.resolve_error_call(node);
            }
            let value_decl = self
                .tables
                .type_of(expression_type)
                .symbol
                .and_then(|symbol| self.get_class_like_declaration_of_symbol(symbol));
            if let Some(value_decl) = value_decl {
                let source = self.binder.source_of_node(value_decl);
                if node_util::has_syntactic_modifier(source, value_decl, ModifierFlags::ABSTRACT) {
                    self.error_at(
                        Some(node),
                        &diagnostics::Cannot_create_an_instance_of_an_abstract_class,
                        &[],
                    );
                    return self.resolve_error_call(node);
                }
            }
            return self.resolve_call(
                node,
                &construct_signatures,
                check_mode,
                SignatureFlags::NONE,
                None,
            );
        }
        let call_signatures = self.get_signatures_of_type(expression_type, SignatureKind::Call)?;
        if !call_signatures.is_empty() {
            let signature = self.resolve_call(
                node,
                &call_signatures,
                check_mode,
                SignatureFlags::NONE,
                None,
            )?;
            if !self
                .options
                .strict_option_value(self.options.no_implicit_any)
            {
                let declaration = self.signature_of(signature).declaration;
                if let Some(declaration) = declaration {
                    if !self.is_js_constructor(declaration) {
                        let return_type = self.get_return_type_of_signature(signature)?;
                        if return_type != self.tables.intrinsics.void {
                            self.error_at(
                                Some(node),
                                &diagnostics::Only_a_void_function_can_be_called_with_the_new_keyword,
                                &[],
                            );
                        }
                    }
                }
                if self.get_this_type_of_signature(signature)? == Some(self.tables.intrinsics.void)
                {
                    self.error_at(
                        Some(node),
                        &diagnostics::A_function_that_is_called_with_the_new_keyword_cannot_have_a_this_type_that_is_void,
                        &[],
                    );
                }
            }
            return Ok(signature);
        }
        self.invocation_error(expression, expression_type, SignatureKind::Construct, None)?;
        self.resolve_error_call(node)
    }

    /// tsc-port: someSignature @6.0.3
    /// tsc-hash: a879e6e70ac9beeb2da83e2e0dbc48b5ec7df38c5f6acaab206b517a080294c7
    /// tsc-span: _tsc.js:77102-77107
    fn some_signature_has_flags(&self, signature: SignatureId, flags: SignatureFlags) -> bool {
        let data = self.signature_of(signature);
        if data.composite_kind == Some(TypeFlags::UNION) {
            if let Some(composite) = data.composite_signatures.clone() {
                return composite
                    .iter()
                    .any(|&member| self.some_signature_has_flags(member, flags));
            }
        }
        data.flags.intersects(flags)
    }

    /// tsc-port: typeHasProtectedAccessibleBase @6.0.3
    /// tsc-hash: 16b2b29a9deaee99d4aada788ecf55b7eecdd3c3d5a814fd4268b565c6291703
    /// tsc-span: _tsc.js:77108-77137
    fn type_has_protected_accessible_base(
        &mut self,
        target: SymbolId,
        ty: TypeId,
    ) -> CheckResult<bool> {
        let base_types = self.get_base_types(ty)?;
        if base_types.is_empty() {
            return Ok(false);
        }
        let first_base = base_types[0];
        if self
            .tables
            .flags_of(first_base)
            .intersects(TypeFlags::INTERSECTION)
        {
            let types = match &self.tables.type_of(first_base).data {
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("intersection flag implies payload"),
            };
            let mixin_flags = self.find_mixins(&types)?;
            for (i, &member) in types.iter().enumerate() {
                if mixin_flags[i] {
                    continue;
                }
                if self
                    .tables
                    .object_flags_of(member)
                    .intersects(tsc_types::ObjectFlags::CLASS | tsc_types::ObjectFlags::INTERFACE)
                {
                    if self.tables.type_of(member).symbol == Some(target) {
                        return Ok(true);
                    }
                    if self.type_has_protected_accessible_base(target, member)? {
                        return Ok(true);
                    }
                }
            }
            return Ok(false);
        }
        if self.tables.type_of(first_base).symbol == Some(target) {
            return Ok(true);
        }
        self.type_has_protected_accessible_base(target, first_base)
    }

    /// tsc-port: isConstructorAccessible @6.0.3
    /// tsc-hash: e7b60027f1bf535adc73a98a8f9e83b7cab35f1ca3a39b39b602b55c6db52baf
    /// tsc-span: _tsc.js:77138-77166
    fn is_constructor_accessible(
        &mut self,
        node: NodeId,
        signature: SignatureId,
    ) -> CheckResult<bool> {
        let Some(declaration) = self.signature_of(signature).declaration else {
            return Ok(true);
        };
        let source = self.binder.source_of_node(declaration);
        let modifiers = ModifierFlags::from_bits(
            node_util::get_combined_modifier_flags(source, declaration).bits()
                & ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER.bits(),
        );
        if modifiers == ModifierFlags::NONE || self.kind_of(declaration) != SyntaxKind::Constructor
        {
            return Ok(true);
        }
        let class_declaration = self.parent_of(declaration).expect(
            "tree invariant: parsed constructors are class elements and finalize_tree assigns \
             their class parent",
        );
        let class_symbol = self.get_symbol_of_declaration(class_declaration)?;
        let declaring_class_declaration = self.get_class_like_declaration_of_symbol(class_symbol);
        let declaring_class = self.get_declared_type_of_class_or_interface(class_symbol)?;
        if !self.is_node_within_class(node, declaring_class_declaration) {
            let containing_class = self.get_containing_class_of(node);
            if let Some(containing_class) = containing_class {
                if modifiers.intersects(ModifierFlags::PROTECTED) {
                    let containing_symbol = self.get_symbol_of_declaration(containing_class)?;
                    let containing_type =
                        self.get_declared_type_of_class_or_interface(containing_symbol)?;
                    if self.type_has_protected_accessible_base(class_symbol, containing_type)? {
                        return Ok(true);
                    }
                }
            }
            if modifiers.intersects(ModifierFlags::PRIVATE) {
                let display = self.type_to_string_slice(declaring_class)?;
                self.error_at(
                    Some(node),
                    &diagnostics::Constructor_of_class_0_is_private_and_only_accessible_within_the_class_declaration,
                    &[&display],
                );
            }
            if modifiers.intersects(ModifierFlags::PROTECTED) {
                let display = self.type_to_string_slice(declaring_class)?;
                self.error_at(
                    Some(node),
                    &diagnostics::Constructor_of_class_0_is_protected_and_only_accessible_within_the_class_declaration,
                    &[&display],
                );
            }
            return Ok(false);
        }
        Ok(true)
    }

    // ---- JSX opening-like elements ----

    /// tsrs-native: generateJsxAttributes +
    /// elaborateJsxComponents' named-attribute slice. Attribute
    /// mismatches are reported at the JSX attribute name (not at the
    /// enclosing tag), preserving the elementwise 2322 span.
    pub(crate) fn elaborate_jsx_named_attributes(
        &mut self,
        attributes: NodeId,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
        sink: &mut ElaborationDiagnosticSink,
    ) -> CheckResult<bool> {
        let properties = match self.data_of(attributes) {
            NodeData::JsxAttributes(data) => data.properties,
            _ => return Ok(false),
        };
        let mut reported = false;
        for attribute in self.nodes_of(properties) {
            let NodeData::JsxAttribute(data) = self.data_of(attribute).clone() else {
                continue;
            };
            let Some(name_node) = data.name else {
                continue;
            };
            let initializer = data.initializer;
            let name = self.jsx_attribute_name_text(name_node);
            if name.contains('-') {
                continue;
            }
            let Some(source_property) = self.get_property_of_type_full(source, &name)? else {
                continue;
            };
            let name_type = self.tables.get_string_literal_type(&name);
            let Some(target_type) =
                self.member_elaboration_target_type(source, target, name_type)?
            else {
                continue;
            };
            let source_type = self.get_type_of_symbol(source_property)?;
            if self.check_type_related_to(source_type, target_type, relation)? {
                continue;
            }
            if let Some(initializer) = initializer {
                if self
                    .elaborate_literal_assignment_into_sink(
                        initializer,
                        target_type,
                        Some(&diagnostics::Type_0_is_not_assignable_to_type_1),
                        sink,
                    )?
                    .reported()
                {
                    reported = true;
                    continue;
                }
            }
            let (source_type, target_type) = self.remove_missing_for_member_report(
                source,
                target,
                &name,
                source_type,
                target_type,
            )?;
            let (_, mut diagnostic, used_containing_message_chain) = self
                .capture_type_assignable_to_diagnostic_for_sink(
                    source_type,
                    target_type,
                    name_node,
                    &diagnostics::Type_0_is_not_assignable_to_type_1,
                    sink,
                )?;
            if let Some(diagnostic) = &mut diagnostic {
                let name_type = self.tables.get_string_literal_type(&name);
                if let Some(related) = self.elementwise_elaboration_related(target, name_type)? {
                    diagnostic.related.push(related);
                }
            }
            if let Some(diagnostic) = diagnostic {
                sink.publish_relation(self, diagnostic, used_containing_message_chain);
                reported = true;
            }
        }
        Ok(reported)
    }

    /// tsc-port: resolveJsxOpeningLikeElement @6.0.3
    /// tsc-hash: de958e239f9938f6db012bfdfb5c38e1a8708ed8c5bf2a1bf4fd79c49a878fa0
    /// tsc-span: _tsc.js:77397-77444
    ///
    /// The intrinsic path returns the fake signature WITHOUT entering
    /// resolveCall (risk-register #6); its attributes-vs-intrinsic
    /// relation failure contains at the elaboration gate
    /// (elaborateJsxComponents = elementwise elaboration, T2 — a plain
    /// head at the tag would be a wrong-span FP).
    fn resolve_jsx_opening_like_element(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<SignatureId> {
        let is_jsx_open_fragment = self.kind_of(node) == SyntaxKind::JsxOpeningFragment;
        let mut value_tag_name: Option<NodeId> = None;
        let expr_types;
        if !is_jsx_open_fragment {
            let (tag_name, type_arguments, attributes) = match self.data_of(node) {
                NodeData::JsxOpeningElement(data) => {
                    (data.tag_name, data.type_arguments, data.attributes)
                }
                NodeData::JsxSelfClosingElement(data) => {
                    (data.tag_name, data.type_arguments, data.attributes)
                }
                _ => (None, None, None),
            };
            let tag_name = tag_name.expect(
                "parser invariant: JSX opening/self-closing parsers always store a tag name \
                 (parse recovery stores a missing Identifier)",
            );
            if self.is_jsx_intrinsic_tag_name(tag_name) {
                let result =
                    self.get_intrinsic_attributes_type_from_jsx_opening_like_element(node)?;
                let fake_signature = self.create_signature_for_jsx_intrinsic(node, result)?;
                let param_type =
                    self.get_effective_first_argument_for_jsx_signature(fake_signature, node)?;
                let attributes = attributes.expect(
                    "parser invariant: JSX opening/self-closing parsers always store an \
                     attributes node (empty or recovery)",
                );
                let attr_type = self.check_expression_with_contextual_type(
                    attributes,
                    param_type,
                    /*inference_context*/ None,
                    CheckMode::NORMAL,
                )?;
                // checkTypeAssignableToAndOptionallyElaborate(attrType,
                // result, errorNode=tagName, expr=attributes).
                let initially_related = self.is_type_assignable_to(attr_type, result)?;
                if !initially_related {
                    let elaborated = self.elaborate_literal_assignment(
                        attributes,
                        result,
                        Some(&diagnostics::Type_0_is_not_assignable_to_type_1),
                    )?;
                    if !elaborated.reported() {
                        self.check_type_assignable_to(
                            attr_type,
                            result,
                            Some(tag_name),
                            &diagnostics::Type_0_is_not_assignable_to_type_1,
                        )?;
                    }
                }
                let type_argument_nodes = self.nodes_of(type_arguments);
                if !type_argument_nodes.is_empty() {
                    for &type_argument in &type_argument_nodes {
                        self.check_source_element(Some(type_argument));
                    }
                    // createDiagnosticForNodeArray(2558, 0, n) on the
                    // typeArguments range.
                    let array = type_arguments.expect("non-empty nodes imply an array");
                    let (pos, end) = {
                        let source = self.binder.source_of_node(node);
                        let array = source.arena.node_array(array);
                        (array.pos, array.end)
                    };
                    let span = self.diag_span_of_byte_range(node, pos, end);
                    let diagnostic = self.diagnostic_at_span(
                        &span,
                        MessageChain::new(
                            &diagnostics::Expected_0_type_arguments_but_got_1,
                            &["0".to_owned(), type_argument_nodes.len().to_string()],
                        ),
                    );
                    self.push_error_diagnostic(diagnostic);
                }
                return Ok(fake_signature);
            }
            value_tag_name = Some(tag_name);
            expr_types = self.check_expression(tag_name, CheckMode::NORMAL)?;
        } else {
            expr_types = self.get_jsx_fragment_type(node)?;
        }
        let apparent_type = self.get_apparent_type(expr_types)?;
        if self.tables.is_error_type(apparent_type) {
            return self.resolve_error_call(node);
        }
        let signatures = self.get_uninstantiated_jsx_signatures_of_type(expr_types, node)?;
        if self.is_untyped_function_call(
            expr_types,
            apparent_type,
            signatures.len(),
            /*construct_signatures*/ 0,
        )? {
            return self.resolve_untyped_call(node);
        }
        if signatures.is_empty() {
            let error_target = match value_tag_name {
                Some(tag_name) => tag_name,
                None => node,
            };
            let text = self.text_of_node(error_target)?;
            self.error_at(
                Some(error_target),
                &diagnostics::JSX_element_type_0_does_not_have_any_construct_or_call_signatures,
                &[&text],
            );
            return self.resolve_error_call(node);
        }
        self.resolve_call(node, &signatures, check_mode, SignatureFlags::NONE, None)
    }

    // ---- tagged templates ----

    /// tsc-port: resolveTaggedTemplateExpression @6.0.3
    /// tsc-hash: af09a7ac9f2e0a442b7c66f5d1054fbee169367451da40bd5cd8b87dc329ea06
    /// tsc-span: _tsc.js:77259-77280
    fn resolve_tagged_template_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<SignatureId> {
        let NodeData::TaggedTemplateExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let tag = data.tag.expect(
            "parser invariant: parse_tagged_template_rest always stores its tag \
             (parse recovery stores a missing expression node)",
        );
        let tag_type = self.check_expression(tag, CheckMode::NORMAL)?;
        let apparent_type = self.get_apparent_type(tag_type)?;
        if apparent_type == self.tables.intrinsics.error {
            return self.resolve_error_call(node);
        }
        let call_signatures = self.get_signatures_of_type(apparent_type, SignatureKind::Call)?;
        let num_construct_signatures = self
            .get_signatures_of_type(apparent_type, SignatureKind::Construct)?
            .len();
        if self.is_untyped_function_call(
            tag_type,
            apparent_type,
            call_signatures.len(),
            num_construct_signatures,
        )? {
            return self.resolve_untyped_call(node);
        }
        if call_signatures.is_empty() {
            let parent_is_array_literal = self
                .parent_of(node)
                .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ArrayLiteralExpression);
            if parent_is_array_literal {
                // 77271-77275: the missing-comma hint (2796) AT the tag.
                self.error_at(
                    Some(tag),
                    &diagnostics::It_is_likely_that_you_are_missing_a_comma_to_separate_these_two_template_expressions_They_form_a_tagged_template_expression_which_cannot_be_invoked,
                    &[],
                );
                return self.resolve_error_call(node);
            }
            self.invocation_error(tag, apparent_type, SignatureKind::Call, None)?;
            return self.resolve_error_call(node);
        }
        self.resolve_call(
            node,
            &call_signatures,
            check_mode,
            SignatureFlags::NONE,
            None,
        )
    }

    /// tsc-port: checkTaggedTemplateExpression @6.0.3
    /// tsc-hash: bf84590375623f25ebdfe8448c801b669b4a508fc18eb3541399e48c96cb7230
    /// tsc-span: _tsc.js:77854-77862
    ///
    /// The MakeTemplateObject emit-helper check is dead at ES2025
    /// (languageVersion >= TaggedTemplates).
    pub(crate) fn check_tagged_template_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<TypeId> {
        let NodeData::TaggedTemplateExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let type_arguments = data.type_arguments;
        if !self.check_grammar_tagged_template_chain(node) {
            self.check_grammar_type_arguments(node, type_arguments);
        }
        let signature = self.get_resolved_signature(node, check_mode)?;
        self.check_deprecated_signature(signature, node)?;
        self.get_return_type_of_signature(signature)
    }

    /// tsc-port: checkGrammarTaggedTemplateChain @6.0.3
    /// tsc-hash: c082b1bcdc184cc37412b14ed705d10bc415a8b6c7d80ac4b82d0e8b7185fc32
    /// tsc-span: _tsc.js:89540-89545
    fn check_grammar_tagged_template_chain(&mut self, node: NodeId) -> bool {
        let NodeData::TaggedTemplateExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let question_dot = data.question_dot_token.is_some();
        let error_node = data.template.unwrap_or(node);
        if question_dot
            || NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::OPTIONAL_CHAIN)
        {
            return self.grammar_error_on_node(
                error_node,
                &diagnostics::Tagged_template_expressions_are_not_permitted_in_an_optional_chain,
                &[],
            );
        }
        false
    }

    // ---- instanceof ----

    /// tsc-port: resolveInstanceofExpression @6.0.3
    /// tsc-hash: 1b57ade58e5e9d430720f20a20c3fd5f9d7d8cd533fc485e82e01ae945166e9a
    /// tsc-span: _tsc.js:77445-77468
    ///
    /// The 2860 head message is injected at resolveCall's failure
    /// ladder (76632-76634), not here.
    fn resolve_instanceof_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<SignatureId> {
        let NodeData::BinaryExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let right = data.right.expect(
            "parser invariant: make_binary_expression always stores its right operand \
             (parse recovery stores a missing expression node)",
        );
        let right_type = self.check_expression(right, CheckMode::NORMAL)?;
        if !self.tables.flags_of(right_type).intersects(TypeFlags::ANY) {
            let has_instance_method_type =
                self.get_symbol_has_instance_method_of_object_type(right_type)?;
            if let Some(has_instance_method_type) = has_instance_method_type {
                let apparent_type = self.get_apparent_type(has_instance_method_type)?;
                if apparent_type == self.tables.intrinsics.error {
                    return self.resolve_error_call(node);
                }
                let call_signatures =
                    self.get_signatures_of_type(apparent_type, SignatureKind::Call)?;
                let num_construct_signatures = self
                    .get_signatures_of_type(apparent_type, SignatureKind::Construct)?
                    .len();
                if self.is_untyped_function_call(
                    has_instance_method_type,
                    apparent_type,
                    call_signatures.len(),
                    num_construct_signatures,
                )? {
                    return self.resolve_untyped_call(node);
                }
                if !call_signatures.is_empty() {
                    return self.resolve_call(
                        node,
                        &call_signatures,
                        check_mode,
                        SignatureFlags::NONE,
                        None,
                    );
                }
            } else {
                let function_like = self.type_has_call_or_construct_signatures(right_type)? || {
                    let global_function = self.global_function_type()?;
                    self.is_type_subtype_of(right_type, global_function)?
                };
                if !function_like {
                    self.error_at(
                        Some(right),
                        &diagnostics::The_right_hand_side_of_an_instanceof_expression_must_be_either_of_type_any_a_class_function_or_other_type_assignable_to_the_Function_interface_type_or_an_object_type_with_a_Symbol_hasInstance_method,
                        &[],
                    );
                    return self.resolve_error_call(node);
                }
            }
        }
        Ok(self.any_signature)
    }

    // ---- import calls ----

    /// tsc-port: checkImportCallExpression @6.0.3
    /// tsc-hash: 5f33130227377ed901de44406b3248f24a3c9d08cdd001c90cd5f8cab946d127
    /// tsc-span: _tsc.js:77718-77767
    ///
    /// Serves `import(...)` AND `import.defer(...)` (the meta-property
    /// callee flavor). Module resolution is LIVE: the
    /// resolveExternalModuleName read below is the real M4 5.8d
    /// worker (the "SILENT None stub" era ended there; the stale
    /// header outlived it — m4-review F8), and resolved modules
    /// produce real Promise-wrapped module types.
    pub(crate) fn check_import_call_expression(&mut self, node: NodeId) -> CheckResult<TypeId> {
        self.check_grammar_import_call_expression(node);
        let NodeData::CallExpression(data) = self.data_of(node) else {
            unreachable!("import calls are call expressions");
        };
        let arguments = data.arguments;
        let args = self.nodes_of(arguments);
        if args.is_empty() {
            let any = self.tables.intrinsics.any;
            return self.create_promise_return_type(node, any);
        }
        let specifier = args[0];
        let specifier_type = self.check_expression_cached(specifier, CheckMode::NORMAL)?;
        let options_type = if args.len() > 1 {
            Some(self.check_expression_cached(args[1], CheckMode::NORMAL)?)
        } else {
            None
        };
        for &argument in args.iter().skip(2) {
            self.check_expression_cached(argument, CheckMode::NORMAL)?;
        }
        let specifier_flags = self.tables.flags_of(specifier_type);
        if specifier_flags.intersects(TypeFlags::UNDEFINED)
            || specifier_flags.intersects(TypeFlags::NULL)
            || !self.is_type_assignable_to(specifier_type, self.tables.intrinsics.string)?
        {
            let display = self.type_to_string_slice(specifier_type)?;
            self.error_at(
                Some(specifier),
                &diagnostics::Dynamic_import_s_specifier_must_be_of_type_string_but_here_has_type_0,
                &[&display],
            );
        }
        if let Some(options_type) = options_type {
            let import_call_options_type =
                self.get_global_import_call_options_type(/*report_errors*/ true)?;
            if import_call_options_type != self.empty_object_type {
                let nullable =
                    self.get_nullable_type(import_call_options_type, TypeFlags::UNDEFINED.bits());
                self.check_type_assignable_to(
                    options_type,
                    nullable,
                    Some(args[1]),
                    &diagnostics::Type_0_is_not_assignable_to_type_1,
                )?;
            }
            // TypeScript 6.0 silences this deprecated `assert` property only
            // for the exact `ignoreDeprecations: "6.0"` value.
            if self.options.ignore_deprecations.as_deref() != Some("6.0") {
                if let NodeData::ObjectLiteralExpression(literal) = self.data_of(args[1]) {
                    let properties = literal.properties;
                    for property in self.nodes_of(properties) {
                        let NodeData::PropertyAssignment(assignment) = self.data_of(property)
                        else {
                            continue;
                        };
                        let Some(name) = assignment.name else {
                            continue;
                        };
                        if self.kind_of(name) == SyntaxKind::Identifier
                            && self.identifier_text_of(name) == Some("assert")
                        {
                            self.grammar_error_on_node(
                                name,
                                &diagnostics::Import_assertions_have_been_replaced_by_import_attributes_Use_with_instead_of_assert,
                                &[],
                            );
                            break;
                        }
                    }
                }
            }
        }
        // 77749-77766: the module-band worker (M4 5.8d un-silences
        // the 5.7b stub). dontResolveAlias=TRUE skips the interop-
        // cloning arm inside resolveESModuleSymbol; the wrap happens
        // here instead. getTypeWithSyntheticDefaultOnly is mode
        // machinery (None at the modeled defaults).
        let module_symbol = self.resolve_external_module_name(node, specifier, false)?;
        if let Some(module_symbol) = module_symbol {
            let es_module_symbol = self.resolve_es_module_symbol(
                Some(module_symbol),
                specifier,
                /*dont_resolve_alias*/ true,
                /*suppress_interop_error*/ false,
            )?;
            if let Some(es_module_symbol) = es_module_symbol {
                let module_type = self.get_type_of_symbol(es_module_symbol)?;
                let synthetic = match self.get_type_with_synthetic_default_only(
                    module_type,
                    es_module_symbol,
                    module_symbol,
                    specifier,
                )? {
                    Some(default_only) => default_only,
                    None => self.get_type_with_synthetic_default_import_type(
                        module_type,
                        es_module_symbol,
                        module_symbol,
                        specifier,
                    )?,
                };
                return self.create_promise_return_type(node, synthetic);
            }
        }
        let any = self.tables.intrinsics.any;
        self.create_promise_return_type(node, any)
    }

    /// tsc-port: checkGrammarImportCallExpression @6.0.3
    /// tsc-hash: 0e938b6a66eadcf72e4bb7f476d2e86b30ea2619d168352eff47d39deed635d5
    /// tsc-span: _tsc.js:90428-90458
    ///
    /// The verbatimModuleSyntax/CommonJS row is live and returns before
    /// every ordinary dynamic-import grammar check. Every row gates on
    /// the modeled module kind (CompilerOptions::emit_module_kind —
    /// the `module` directive maps through the conformance runner).
    fn check_grammar_import_call_expression(&mut self, node: NodeId) -> bool {
        let module_kind = self.options.emit_module_kind();
        if self.options.verbatim_module_syntax == Some(true) && module_kind == 1 {
            let message = self.get_verbatim_module_syntax_error_message(node);
            return self.grammar_error_on_node(node, message, &[]);
        }
        let NodeData::CallExpression(data) = self.data_of(node) else {
            unreachable!("import calls are call expressions");
        };
        let expression = data.expression;
        let type_arguments = data.type_arguments;
        let arguments = data.arguments;
        let is_defer = expression.is_some_and(|e| self.kind_of(e) == SyntaxKind::MetaProperty);
        if is_defer {
            // ModuleKind.ESNext = 99, ModuleKind.Preserve = 200.
            if module_kind != 99 && module_kind != 200 {
                return self.grammar_error_on_node(
                    node,
                    &diagnostics::Deferred_imports_are_only_supported_when_the_module_flag_is_set_to_esnext_or_preserve,
                    &[],
                );
            }
        } else if module_kind == 5 {
            // ModuleKind.ES2015.
            return self.grammar_error_on_node(
                node,
                &diagnostics::Dynamic_imports_are_only_supported_when_the_module_flag_is_set_to_es2020_es2022_esnext_commonjs_amd_system_umd_node16_node18_node20_or_nodenext,
                &[],
            );
        }
        if type_arguments.is_some() {
            return self.grammar_error_on_node(
                node,
                &diagnostics::This_use_of_import_is_invalid_import_calls_can_be_written_but_they_must_have_parentheses_and_cannot_have_type_arguments,
                &[],
            );
        }
        let args = self.nodes_of(arguments);
        // Node16 = 100 .. NodeNext = 199 (the whole Node band).
        if !(100..=199).contains(&module_kind) && module_kind != 99 && module_kind != 200 {
            self.check_grammar_for_disallowed_trailing_comma(
                arguments,
                &diagnostics::Trailing_comma_not_allowed,
            );
            if args.len() > 1 {
                let import_attributes_argument = args[1];
                return self.grammar_error_on_node(
                    import_attributes_argument,
                    &diagnostics::Dynamic_imports_only_support_a_second_argument_when_the_module_option_is_set_to_esnext_node16_node18_node20_nodenext_or_preserve,
                    &[],
                );
            }
        }
        if args.is_empty() || args.len() > 2 {
            return self.grammar_error_on_node(
                node,
                &diagnostics::Dynamic_imports_can_only_accept_a_module_specifier_and_an_optional_set_of_attributes_as_arguments,
                &[],
            );
        }
        let spread = args
            .iter()
            .copied()
            .find(|&arg| self.kind_of(arg) == SyntaxKind::SpreadElement);
        if let Some(spread) = spread {
            return self.grammar_error_on_node(
                spread,
                &diagnostics::Argument_of_dynamic_import_cannot_be_spread_element,
                &[],
            );
        }
        false
    }

    // ---- dispatch + links protocol ----

    /// tsc-port: resolveSignature @6.0.3
    /// tsc-hash: 76619800b60dc3d6783ffd65d95f10e7eb835be6e3f4ea709139adedbf508a9a
    /// tsc-span: _tsc.js:77472-77490
    fn resolve_signature_dispatch(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<SignatureId> {
        match self.kind_of(node) {
            SyntaxKind::CallExpression => self.resolve_call_expression(node, check_mode),
            SyntaxKind::NewExpression => self.resolve_new_expression(node, check_mode),
            SyntaxKind::TaggedTemplateExpression => {
                self.resolve_tagged_template_expression(node, check_mode)
            }
            SyntaxKind::Decorator => self.resolve_decorator(node, check_mode),
            SyntaxKind::JsxOpeningFragment
            | SyntaxKind::JsxOpeningElement
            | SyntaxKind::JsxSelfClosingElement => {
                self.resolve_jsx_opening_like_element(node, check_mode)
            }
            SyntaxKind::BinaryExpression => self.resolve_instanceof_expression(node, check_mode),
            _ => unreachable!("Branch in 'resolveSignature' should be unreachable."),
        }
    }

    /// tsc-port: getResolvedSignature @6.0.3
    /// tsc-hash: 6a0c3093b217f129ec9c4778d89b3a819996877ac666147b4eb6521ad514fd66
    /// tsc-span: _tsc.js:77491-77508
    ///
    /// candidatesOutArray is LSP-only (always None): the cached
    /// early-return needs no re-run arm. tsc's exit write (77504-77506)
    /// is UNCONDITIONAL per completed frame:
    /// `links.resolvedSignature = flowLoopStart === flowLoopCount ?
    /// result : cached` — where `cached` is the FRAME-ENTRY value. The
    /// port's typed protocol spells the three arms out (M4-review F7,
    /// re-derived at 7.4b when the re-run landed):
    /// - loop-clean completion memoizes `result` (any frame);
    /// - mid-fixpoint completion restores the ENTRY value: Vacant for
    ///   a fresh frame (clear — resolveCall's 76629 failure stash
    ///   included, tsc clobbers it identically), the outer sentinel
    ///   for a RE-ENTRANT frame (restore twin — an inner stash must
    ///   not survive over the outer frame's Resolving);
    /// - the Err channel (no tsc counterpart) restores entry state the
    ///   same way, with ONE deliberate deviation: a loop-clean fresh
    ///   frame keeps a COMPLETED failure stash (Resolving-gated
    ///   revert) — tsc memoizes the failure-face signature and the
    ///   gate's containment only suppressed the report.
    pub(crate) fn get_resolved_signature(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<SignatureId> {
        let cached = self.links.node(node).resolved_signature.clone();
        if let LinkSlot::Resolved(cached) = cached {
            return Ok(cached);
        }
        let save_resolution_start = self.resolution_start;
        let wrote_sentinel = matches!(cached, LinkSlot::Vacant);
        if wrote_sentinel {
            self.resolution_start = self.resolution_targets.len();
        }
        self.links.set_node_resolved_signature_call_protocol(
            self.speculation_depth,
            node,
            LinkSlot::Resolving,
        );
        let result = self.resolve_signature_dispatch(node, check_mode);
        self.resolution_start = save_resolution_start;
        match result {
            Ok(result) => {
                // 77504 `result !== resolvingSignature`: the
                // SkipGenericFunctions defer (77041) skips the exit
                // write entirely — the Resolving sentinel stays in
                // links as the load-bearing skip marker (72918/77616).
                if result == self.resolving_signature {
                    return Ok(result);
                }
                if self.flow_loop_start as usize == self.flow_loop_stack.len() {
                    self.links.set_node_resolved_signature_call_protocol(
                        self.speculation_depth,
                        node,
                        LinkSlot::Resolved(result),
                    );
                } else if wrote_sentinel {
                    self.links.clear_node_resolved_signature_call(node);
                } else {
                    self.links
                        .restore_node_resolved_signature_call_resolving(node);
                }
                Ok(result)
            }
            Err(err) => {
                if wrote_sentinel {
                    if self.flow_loop_start as usize == self.flow_loop_stack.len() {
                        self.links.revert_node_resolved_signature_call(node);
                    } else {
                        self.links.clear_node_resolved_signature_call(node);
                    }
                    // tsrs-native: a Vacant left by THIS unwind is a
                    // containment-reverted resolution — record it so
                    // check_deferred_node's skip can tell it from the
                    // benign mid-fixpoint Ok clear above (a COMPLETED
                    // failure stash survives the Resolving-gated
                    // revert instead and feeds contextual reads, so it
                    // never reaches here as Vacant).
                    if matches!(self.links.node(node).resolved_signature, LinkSlot::Vacant) {
                        self.contained_call_resolutions.insert(node);
                    }
                } else {
                    self.links
                        .restore_node_resolved_signature_call_resolving(node);
                }
                Err(err)
            }
        }
    }

    // ---- the checkCallExpression worker ----

    /// tsc-port: checkCallExpression @6.0.3
    /// tsc-hash: 3459b258ce93da62aaf8212b10d3765e2f130715cb86f663d60d438cecfb09a1
    /// tsc-span: _tsc.js:77607-77660
    ///
    /// Serves Call AND New (tsc dispatches both here).
    /// The void-return type-predicate assertion band (2775/2776)
    /// landed with the 6.6 review (its M4 "provably dead" residual
    /// lapsed when 6.6a/c made getEffectsSignature and
    /// getTypePredicateOfSignature real).
    /// JS arms (require, expando) are JS-file-gated.
    pub(crate) fn check_call_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<TypeId> {
        let (type_arguments, expression) = match self.data_of(node) {
            NodeData::CallExpression(data) => (data.type_arguments, data.expression),
            NodeData::NewExpression(data) => (data.type_arguments, data.expression),
            _ => unreachable!("checkCallExpression serves call/new"),
        };
        self.check_grammar_type_arguments(node, type_arguments);
        let signature = self.get_resolved_signature(node, check_mode)?;
        if signature == self.resolving_signature {
            // 77616-77618: the SkipGenericFunctions defer (77041, live
            // at 7.4b) — silentNever until the NORMAL-mode re-run
            // resolves for real.
            return Ok(self.tables.intrinsics.silent_never);
        }
        self.check_deprecated_signature(signature, node)?;
        if expression.is_some_and(|e| self.kind_of(e) == SyntaxKind::SuperKeyword) {
            return Ok(self.tables.intrinsics.void);
        }
        if self.kind_of(node) == SyntaxKind::NewExpression {
            // 77623-77631: a `new` that resolved through call
            // signatures — 7009 under noImplicitAny, anyType result.
            let declaration = self.signature_of(signature).declaration;
            if let Some(declaration) = declaration {
                // 77625: a materialized JSDocSignature inherits construct
                // semantics directly from its JSDoc root's constructor host.
                let is_constructor_like = matches!(
                    self.kind_of(declaration),
                    SyntaxKind::Constructor
                        | SyntaxKind::ConstructSignature
                        | SyntaxKind::ConstructorType
                ) || (self.kind_of(declaration)
                    == SyntaxKind::JSDocSignature
                    && self
                        .get_jsdoc_root(declaration)
                        .and_then(|root| self.parent_of(root))
                        .is_some_and(|host| self.kind_of(host) == SyntaxKind::Constructor))
                    || node_util::is_jsdoc_construct_signature(
                        self.binder.source_of_node(declaration),
                        declaration,
                    )
                    || self.is_js_constructor(declaration);
                if !is_constructor_like {
                    if self
                        .options
                        .strict_option_value(self.options.no_implicit_any)
                    {
                        self.error_at(
                                Some(node),
                                &diagnostics::new_expression_whose_target_lacks_a_construct_signature_implicitly_has_an_any_type,
                                &[],
                            );
                    }
                    return Ok(self.tables.intrinsics.any);
                }
            }
        }
        // 77632-77634: checked-JS CommonJS require calls return the
        // resolved module's type. This is deliberately stricter than
        // the syntactic isRequireCall probe: a local non-ambient
        // function named `require` remains an ordinary call.
        if self.is_in_js_file(node) && self.is_common_js_require(node)? {
            let argument = match self.data_of(node) {
                NodeData::CallExpression(data) => self.nodes_of(data.arguments).first().copied(),
                _ => None,
            };
            if let Some(argument) = argument {
                return self.resolve_external_module_type_by_literal(argument);
            }
        }
        let return_type = self.get_return_type_of_signature(signature)?;
        if self
            .tables
            .flags_of(return_type)
            .intersects(TypeFlags::ES_SYMBOL_LIKE)
            && self.is_symbol_or_symbol_for_call(node)?
        {
            // 77636-77638: `Symbol()`/`Symbol.for()` results take the
            // owning declaration's unique-symbol type when the
            // position is a valid `unique symbol` declaration.
            let parent = self.parent_of(node).expect(
                "tree invariant: parsed call/new expressions are non-root nodes and \
                 finalize_tree assigns their parent",
            );
            let target = self.walk_up_parenthesized_expressions(parent);
            return self.get_es_symbol_like_type_for_node(target);
        }
        // 77639-77646: the assertion-position checks. A plain
        // (non-optional-chain) call STATEMENT whose signature is
        // void-returning and carries a type predicate must sit on a
        // dotted name (2776), and every name in that target must
        // carry an explicit type annotation — i.e. the
        // effects-signature resolution must succeed (2775).
        // Body-inference uncertainty cannot reach the predicate read:
        // inferred predicates are boolean-valued `x is T`, never
        // void/asserts, so the VOID filter keeps it
        // annotation-driven. The live 2775 diagnostic is passed back
        // through getTypeOfDottedName so getExplicitTypeOfSymbol can
        // attach 2782 at the first declaration without an annotation.
        if self.kind_of(node) == SyntaxKind::CallExpression {
            let question_dot_token = match self.data_of(node) {
                NodeData::CallExpression(data) => data.question_dot_token,
                _ => None,
            };
            if question_dot_token.is_none()
                && self
                    .parent_of(node)
                    .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ExpressionStatement)
                && self
                    .tables
                    .flags_of(return_type)
                    .intersects(TypeFlags::VOID)
                && self.get_type_predicate_of_signature(signature)?.is_some()
            {
                if let Some(expression) = expression {
                    if !self.is_dotted_name(expression) {
                        self.error_at(
                            Some(expression),
                            &diagnostics::Assertions_require_the_call_target_to_be_an_identifier_or_qualified_name,
                            &[],
                        );
                    } else if self.get_effects_signature(node)?.is_none() {
                        let mut diagnostic = self.diagnostic_for_node(
                            expression,
                            &diagnostics::Assertions_require_every_name_in_the_call_target_to_be_declared_with_an_explicit_type_annotation,
                            &[],
                        );
                        self.get_type_of_dotted_name_with_diagnostic(
                            expression,
                            Some(&mut diagnostic),
                        )?;
                        self.push_error_diagnostic(diagnostic);
                    }
                }
            }
        }
        if self.is_in_js_file(node) {
            if let Some(js_symbol) = self.get_symbol_of_expando(node) {
                let exports: SymbolTable = self.binder.symbol(js_symbol).exports.clone();
                if !exports.is_empty() {
                    let properties = exports.values().copied().collect();
                    let js_assignment_type = self.make_resolved_anonymous_type(
                        Some(js_symbol),
                        exports,
                        properties,
                        Vec::new(),
                        ObjectFlags::JS_LITERAL,
                    );
                    return self.get_intersection_type(
                        &[return_type, js_assignment_type],
                        IntersectionFlags::NONE,
                    );
                }
            }
        }
        Ok(return_type)
    }

    /// tsc-port: isCommonJsRequire @6.0.3
    /// tsc-hash: cbe4149bc7b8d5ed1d14b9e30a1eaa24c415995f183e60d0a57cc69a4e530abd
    /// tsc-span: _tsc.js:77823-77853
    ///
    /// The upstream `requireSymbol` is represented by an unresolved
    /// checked-JS `require` identifier in this checker. Explicit
    /// ambient function/variable declarations are accepted too;
    /// aliases and local declarations are not CommonJS require calls.
    fn is_common_js_require(&mut self, node: NodeId) -> CheckResult<bool> {
        if !self.is_require_call(node, /*require_string_literal_like_argument*/ true) {
            return Ok(false);
        }
        let callee = match self.data_of(node) {
            NodeData::CallExpression(data) => data.expression,
            _ => None,
        };
        let Some(callee) = callee else {
            return Ok(false);
        };
        let resolved = self.resolve_name(
            Some(callee),
            "require",
            SymbolFlags::VALUE,
            None,
            /*is_use*/ true,
            /*exclude_globals*/ false,
        )?;
        let Some(resolved) = resolved else {
            return Ok(true);
        };
        let flags = self.symbol_flags(resolved);
        if flags.intersects(SymbolFlags::ALIAS) {
            return Ok(false);
        }
        let declaration_kind = if flags.intersects(SymbolFlags::FUNCTION) {
            Some(SyntaxKind::FunctionDeclaration)
        } else if flags.intersects(SymbolFlags::VARIABLE) {
            Some(SyntaxKind::VariableDeclaration)
        } else {
            None
        };
        let declaration =
            declaration_kind.and_then(|kind| self.get_declaration_of_kind(resolved, kind));
        Ok(declaration.is_some_and(|declaration| {
            self.binder
                .flags_of(declaration)
                .intersects(NodeFlags::AMBIENT)
        }))
    }

    /// tsc-port: isDottedName @6.0.3
    /// tsc-hash: ec6ff8964b04776720f7c9510ace6f55a714e4d3555762f938fdc934060e35c2
    /// tsc-span: _tsc.js:17147-17149
    ///
    /// Identifier / this / super / meta-property roots through
    /// property-access and parenthesized links.
    fn is_dotted_name(&self, node: NodeId) -> bool {
        match self.kind_of(node) {
            SyntaxKind::Identifier
            | SyntaxKind::ThisKeyword
            | SyntaxKind::SuperKeyword
            | SyntaxKind::MetaProperty => true,
            SyntaxKind::PropertyAccessExpression => match self.data_of(node) {
                NodeData::PropertyAccessExpression(data) => data
                    .expression
                    .is_some_and(|expression| self.is_dotted_name(expression)),
                _ => false,
            },
            SyntaxKind::ParenthesizedExpression => match self.data_of(node) {
                NodeData::ParenthesizedExpression(data) => data
                    .expression
                    .is_some_and(|expression| self.is_dotted_name(expression)),
                _ => false,
            },
            _ => false,
        }
    }

    /// tsc-port: isSymbolOrSymbolForCall @6.0.3
    /// tsc-hash: 7f795d82739f8c0d3e0537b4833ca9e15fe55c71dd23938052fff87798ea1dfc
    /// tsc-span: _tsc.js:77692-77717
    pub(crate) fn is_symbol_or_symbol_for_call(&mut self, node: NodeId) -> CheckResult<bool> {
        let NodeData::CallExpression(data) = self.data_of(node) else {
            return Ok(false);
        };
        let Some(mut left) = data.expression else {
            return Ok(false);
        };
        if let NodeData::PropertyAccessExpression(access) = self.data_of(left) {
            let is_for = access
                .name
                .and_then(|name| self.identifier_text_of(name))
                .is_some_and(|text| text == "for");
            if is_for {
                if let Some(inner) = access.expression {
                    left = inner;
                }
            }
        }
        if self.kind_of(left) != SyntaxKind::Identifier
            || self.identifier_text_of(left) != Some("Symbol")
        {
            return Ok(false);
        }
        // getGlobalESSymbolConstructorSymbol(reportErrors=false)
        // (77701): the silent global-value probe; the deferredGlobal*
        // memo elides (deterministic, no suggestion-budget burn).
        let Some(global_es_symbol) = self.get_global_symbol("Symbol", SymbolFlags::VALUE, None)?
        else {
            return Ok(false);
        };
        let resolved =
            self.resolve_name(Some(left), "Symbol", SymbolFlags::VALUE, None, false, false)?;
        Ok(resolved == Some(global_es_symbol))
    }
}

#[cfg(test)]
#[path = "../tests/unit/calls/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/calls/eopt_pins.rs"]
mod eopt_pins;
