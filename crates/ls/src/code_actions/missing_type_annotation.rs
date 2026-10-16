//! tsgo `ls/codeactions_fixmissingtypeannotation.go`: the
//! `--isolatedDeclarations` fixes — a type annotation, an inline
//! `satisfies T as T` assertion or an extracted variable for the
//! declarations and expressions whose types declaration emit cannot infer —
//! and their fix-all, "Add all missing type annotations".

use std::collections::BTreeSet;

use tsc_binder::node_util;
use tsc_checker::state::{CheckResult, CheckerState};
use tsc_diagnostics::{gen, DiagnosticMessage, MessageCatalog};
use tsc_emitter::{
    EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, GeneratedIdentifierFlags, TransformFlags,
    TransformNode, TransformNodeArray, TransformSourceId,
};
use tsc_lsproto::TextEdit;
use tsc_syntax::nodes::{
    ArrayLiteralExpressionData, AsExpressionData, BinaryExpressionData, ConditionalExpressionData,
    ObjectLiteralExpressionData, ParenthesizedExpressionData, SatisfiesExpressionData,
    SpreadAssignmentData, SpreadElementData,
};
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags, ObjectFlags, SymbolId, TypeData, TypeFlags, TypeId};

use super::{CodeAction, CodeFixContext, CodeFixProvider, CombinedCodeActions};
use crate::astnav::{Found, Navigator};
use crate::autoimport::{file_edits, try_get_auto_importable_reference_from_type_node};
use crate::change::{is_statement, LeadingTriviaOption, Tracker};
use crate::language_service::{LanguageService, LsError};
use crate::lsutil::new_line_kind;
use crate::request::Request;
use crate::tree::View;

const FIX_ID: &str = "fixMissingTypeAnnotationOnExports";

/// tsgo `isolatedDeclarationsFixErrorCodes`.
fn error_codes() -> Vec<u32> {
    [
        &gen::Function_must_have_an_explicit_return_type_annotation_with_isolatedDeclarations,
        &gen::Method_must_have_an_explicit_return_type_annotation_with_isolatedDeclarations,
        &gen::At_least_one_accessor_must_have_an_explicit_type_annotation_with_isolatedDeclarations,
        &gen::Variable_must_have_an_explicit_type_annotation_with_isolatedDeclarations,
        &gen::Parameter_must_have_an_explicit_type_annotation_with_isolatedDeclarations,
        &gen::Property_must_have_an_explicit_type_annotation_with_isolatedDeclarations,
        &gen::Expression_type_can_t_be_inferred_with_isolatedDeclarations,
        &gen::Binding_elements_with_initializers_can_t_be_exported_directly_with_isolatedDeclarations,
        &gen::Computed_property_names_on_class_or_object_literals_cannot_be_inferred_with_isolatedDeclarations,
        &gen::Computed_properties_must_be_number_or_string_literals_variables_or_dotted_expressions_with_isolatedDeclarations,
        &gen::Enum_member_initializers_must_be_computable_without_references_to_external_symbols_with_isolatedDeclarations,
        &gen::Extends_clause_can_t_contain_an_expression_with_isolatedDeclarations,
        &gen::Objects_that_contain_shorthand_properties_can_t_be_inferred_with_isolatedDeclarations,
        &gen::Objects_that_contain_spread_assignments_can_t_be_inferred_with_isolatedDeclarations,
        &gen::Arrays_with_spread_elements_can_t_inferred_with_isolatedDeclarations,
        &gen::Default_exports_can_t_be_inferred_with_isolatedDeclarations,
        &gen::Only_const_arrays_can_be_inferred_with_isolatedDeclarations,
        &gen::Assigning_properties_to_functions_without_declaring_them_is_not_supported_with_isolatedDeclarations_Add_an_explicit_declaration_for_the_properties_assigned_to_this_function,
        &gen::Declaration_emit_for_this_parameter_requires_implicitly_adding_undefined_to_its_type_This_is_not_supported_with_isolatedDeclarations,
        &gen::Type_containing_private_name_0_can_t_be_used_with_isolatedDeclarations,
        &gen::Add_satisfies_and_a_type_assertion_to_this_expression_satisfies_T_as_T_to_make_the_type_explicit,
    ]
    .iter()
    .map(|message| message.code)
    .collect()
}

/// tsgo `IsolatedDeclarationsFixProvider`.
pub(super) const PROVIDER: CodeFixProvider = CodeFixProvider {
    error_codes,
    get_code_actions,
    get_all_code_actions: Some(get_all_code_actions),
};

/// tsgo `typePrintMode`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TypePrintMode {
    Full,
    /// `typeof X`
    Relative,
    /// The widened literal type.
    Widened,
}

/// The fixes tried for one diagnostic.
#[derive(Clone, Copy)]
enum Fix {
    Annotation,
    InlineAssertion,
    ExtractAsVariable,
}

/// A failure of the checker, the node builder or the factory.
type FixResult<T> = Result<T, String>;

/// A fix's description, none when it does not apply (tsgo's `""`).
type Description = FixResult<Option<String>>;

/// tsgo `getIsolatedDeclarationsCodeActions`.
fn get_code_actions(
    service: &mut LanguageService<'_>,
    context: &CodeFixContext<'_>,
) -> Result<Vec<CodeAction>, LsError> {
    let format_settings = service.format_options().clone();
    let span = context.span;
    let actions = service.with_program_request(false, |request| {
        // Match TS ordering: Full annotation, Relative annotation, Widened annotation,
        // Full inline, Relative inline, Widened inline, Full extract
        let modes = [
            TypePrintMode::Full,
            TypePrintMode::Relative,
            TypePrintMode::Widened,
        ];
        let tries = modes
            .iter()
            .map(|&mode| (Fix::Annotation, mode))
            .chain(modes.iter().map(|&mode| (Fix::InlineAssertion, mode)))
            // extractAsVariable only in Full mode
            .chain(std::iter::once((
                Fix::ExtractAsVariable,
                TypePrintMode::Full,
            )));
        let mut actions = Vec::new();
        for (fix, mode) in tries {
            match try_code_action(request, &format_settings, context.file, mode, span, fix)? {
                Ok(Some(action)) => actions.push(action),
                Ok(None) => {}
                Err(error) => return Ok(Err(error)),
            }
        }
        Ok(Ok(actions))
    })?;
    actions.unwrap_or(Ok(Vec::new()))
}

/// tsgo `getAllIsolatedDeclarationsCodeActions`: a type annotation for each
/// diagnostic of the file.
fn get_all_code_actions(
    service: &mut LanguageService<'_>,
    context: &CodeFixContext<'_>,
) -> Result<Option<CombinedCodeActions>, LsError> {
    let codes = error_codes();
    let spans: Vec<(usize, usize)> = service
        .all_diagnostics(context.file)?
        .into_iter()
        .filter(|diagnostic| super::is_fixable_diagnostic(diagnostic, &codes))
        .map(|diagnostic| {
            let start = diagnostic.start.unwrap_or_default() as usize;
            (
                start,
                start + diagnostic.length.unwrap_or_default() as usize,
            )
        })
        .collect();
    let format_settings = service.format_options().clone();
    let description = service.localize(&gen::Add_all_missing_type_annotations, &[]);
    let changes = service.with_program_request(false, |request| {
        let source = request.state.binder.source(context.file);
        let mut tracker = Tracker::new(
            new_line_kind(request.state.options),
            format_settings.clone(),
        );
        let target =
            tracker.source_in_program(source, request.state.program_source_id(context.file));
        let mut fixer = Fixer::new(
            &mut tracker,
            source,
            target,
            request.catalog,
            TypePrintMode::Full,
        );
        for &span in &spans {
            if let Err(error) = fixer.add_type_annotation(request.state, span) {
                return Ok(Err(LsError(error)));
            }
        }
        if let Err(error) = fixer.add_symbols_to_existing_imports(request.state) {
            return Ok(Err(LsError(error)));
        }
        Ok(
            file_edits(tracker, source, request.converters, &request.state.binder)
                .map(|(edits, _)| edits)
                .map_err(LsError),
        )
    })?;
    match changes {
        Some(Ok(changes)) if !changes.is_empty() => Ok(Some(CombinedCodeActions {
            description,
            changes,
        })),
        Some(Err(error)) => Err(error),
        _ => Ok(None),
    }
}

/// tsgo `tryCodeAction`: one fix in a tracker of its own, with the symbols
/// its types need added to the file's imports.
fn try_code_action(
    request: &mut Request<'_, '_>,
    format_settings: &crate::lsutil::FormatCodeSettings,
    file: usize,
    mode: TypePrintMode,
    span: (usize, usize),
    fix: Fix,
) -> CheckResult<Result<Option<CodeAction>, LsError>> {
    let source = request.state.binder.source(file);
    let mut tracker = Tracker::new(
        new_line_kind(request.state.options),
        format_settings.clone(),
    );
    let target = tracker.source_in_program(source, request.state.program_source_id(file));
    let mut fixer = Fixer::new(&mut tracker, source, target, request.catalog, mode);
    let description = match fix {
        Fix::Annotation => fixer.add_type_annotation(request.state, span),
        Fix::InlineAssertion => fixer.add_inline_assertion(request.state, span),
        Fix::ExtractAsVariable => fixer.extract_as_variable(request.state, span),
    };
    let description = match description {
        Ok(Some(description)) => description,
        Ok(None) => return Ok(Ok(None)),
        Err(error) => return Ok(Err(LsError(error))),
    };
    // Add any symbols that need to be imported to existing import declarations
    if let Err(error) = fixer.add_symbols_to_existing_imports(request.state) {
        return Ok(Err(LsError(error)));
    }
    let changes: Vec<TextEdit> =
        match file_edits(tracker, source, request.converters, &request.state.binder) {
            Ok((changes, _)) => changes,
            Err(error) => return Ok(Err(LsError(error))),
        };
    if changes.is_empty() {
        return Ok(Ok(None));
    }
    Ok(Ok(Some(CodeAction {
        description,
        changes,
        fix_id: FIX_ID.to_owned(),
    })))
}

/// tsgo `isolatedDeclarationsFixer`.
struct Fixer<'f, 'a> {
    tracker: &'f mut Tracker<'a>,
    file: &'a SourceFile,
    /// The file's copy in the tracker's arena.
    target: TransformSourceId,
    catalog: Option<&'f dyn MessageCatalog>,
    fixed_nodes: BTreeSet<NodeId>,
    type_print_mode: TypePrintMode,
    symbols_to_import: Vec<SymbolId>,
    /// set by inferType/relativeType when the target was mutated (e.g., spread decomposition)
    mutated_target: bool,
}

impl<'f, 'a> Fixer<'f, 'a> {
    fn new(
        tracker: &'f mut Tracker<'a>,
        file: &'a SourceFile,
        target: TransformSourceId,
        catalog: Option<&'f dyn MessageCatalog>,
        type_print_mode: TypePrintMode,
    ) -> Self {
        Self {
            tracker,
            file,
            target,
            catalog,
            fixed_nodes: BTreeSet::new(),
            type_print_mode,
            symbols_to_import: Vec::new(),
            mutated_target: false,
        }
    }

    /// tsgo `addTypeAnnotation`.
    fn add_type_annotation(
        &mut self,
        checker: &mut CheckerState<'_>,
        span: (usize, usize),
    ) -> Description {
        let node_with_diag = token_node_at(self.file, span.0);
        if let Some(expando_function) = find_expando_function(checker, self.file, node_with_diag)? {
            if self.kind(expando_function) == SyntaxKind::FunctionDeclaration {
                return self.create_namespace_for_expando_properties(checker, expando_function);
            }
            return self.fix_isolated_declaration_error(checker, expando_function);
        }
        match find_ancestor_with_missing_type(self.file, node_with_diag) {
            Some(node_missing_type) => {
                self.fix_isolated_declaration_error(checker, node_missing_type)
            }
            None => Ok(None),
        }
    }

    /// tsgo `createNamespaceForExpandoProperties`: `declare namespace f`
    /// with the type of each property assigned to the function.
    fn create_namespace_for_expando_properties(
        &mut self,
        checker: &mut CheckerState<'_>,
        expando_function: NodeId,
    ) -> Description {
        let Some(function_name) = name_of(self.file, expando_function) else {
            return Ok(None);
        };
        let function_name = identifier_text(self.file, function_name);
        let ty = checker
            .get_type_at_location(expando_function)
            .map_err(check_error)?;
        let elements = checker.get_properties_of_type(ty).map_err(check_error)?;
        if elements.is_empty() {
            return Ok(None);
        }
        let mut new_properties = Vec::new();
        for symbol in elements {
            let name = checker
                .binder
                .symbol(symbol)
                .escaped_name
                .unescape()
                .to_string_lossy()
                .into_owned();
            if !tsc_syntax::is_identifier_text(&name) {
                continue;
            }
            // skip symbols that already have a variable declaration
            if checker
                .binder
                .symbol(symbol)
                .value_declaration
                .is_some_and(|declaration| {
                    checker
                        .binder
                        .source_of_node(declaration)
                        .arena
                        .node(declaration)
                        .kind
                        == SyntaxKind::VariableDeclaration
                })
            {
                continue;
            }
            let symbol_type = checker.get_type_of_symbol(symbol).map_err(check_error)?;
            let Some(type_node) = self.type_to_minimized_reference_type(
                checker,
                symbol_type,
                expando_function,
                EmitNodeBuilderFlags::DECLARATION_EMIT.0,
            )?
            else {
                continue;
            };
            let name = self.identifier(&name)?;
            let declaration = self.factory_variable_declaration(name, Some(type_node), None)?;
            let declaration_list = self.variable_declaration_list(declaration, NodeFlags::NONE)?;
            let export_token = self.token(SyntaxKind::ExportKeyword)?;
            let modifiers = self.node_array(vec![export_token])?;
            new_properties.push(
                self.tracker
                    .arena()
                    .factory()
                    .create_variable_statement(self.target, Some(modifiers), declaration_list)
                    .map_err(transform_error)?,
            );
        }
        if new_properties.is_empty() {
            return Ok(None);
        }
        let mut modifiers = Vec::new();
        if node_util::has_syntactic_modifier(self.file, expando_function, ModifierFlags::EXPORT) {
            modifiers.push(self.token(SyntaxKind::ExportKeyword)?);
        }
        modifiers.push(self.token(SyntaxKind::DeclareKeyword)?);
        let modifiers = self.node_array(modifiers)?;
        let name = self.identifier(&function_name)?;
        let statements = self.node_array(new_properties)?;
        let body = self
            .tracker
            .arena()
            .factory()
            .create_module_block(self.target, statements)
            .map_err(transform_error)?;
        // Set the flags for namespace
        let namespace = self
            .tracker
            .arena()
            .factory()
            .create_module_declaration(
                self.target,
                Some(modifiers),
                name,
                None,
                Some(body),
                NodeFlags::NAMESPACE
                    | NodeFlags::AMBIENT
                    | NodeFlags::EXPORT_CONTEXT
                    | NodeFlags::CONTEXT_FLAGS,
            )
            .map_err(transform_error)?;
        self.tracker
            .insert_node_after(self.file, expando_function, namespace)
            .map_err(transform_error)?;
        Ok(Some(self.localize(
            &gen::Annotate_types_of_properties_expando_function_in_a_namespace,
            &[],
        )))
    }

    /// tsgo `addInlineAssertion`: `expr satisfies T as T` (a shorthand
    /// property: `name: name as T`).
    fn add_inline_assertion(
        &mut self,
        checker: &mut CheckerState<'_>,
        span: (usize, usize),
    ) -> Description {
        let node_with_diag = token_node_at(self.file, span.0);
        // No inline assertions for expando members
        if find_expando_function(checker, self.file, node_with_diag)?.is_some() {
            return Ok(None);
        }
        let Some(target_node) = find_best_fitting_node(self.file, node_with_diag, span) else {
            return Ok(None);
        };
        if is_value_signature_declaration(self.kind(target_node))
            || self
                .parent(target_node)
                .is_some_and(|parent| is_value_signature_declaration(self.kind(parent)))
        {
            return Ok(None);
        }
        let is_expression_target = crate::format::is_expression_kind(self.kind(target_node));
        let is_shorthand_property_assignment_target =
            self.kind(target_node) == SyntaxKind::ShorthandPropertyAssignment;
        // Go's IsDeclaration is broader than TS's isDeclaration (e.g. CallExpression has DeclarationData
        // in Go but is not a declaration kind in TS). Use isNamedDeclarationKind to match TS behavior.
        if !is_shorthand_property_assignment_target
            && is_named_declaration_kind(self.kind(target_node))
        {
            return Ok(None);
        }
        // No inline assertions on binding patterns
        if self
            .find_ancestor(target_node, |file, node| {
                node_util::is_binding_pattern(file, node)
            })
            .is_some()
        {
            return Ok(None);
        }
        // No inline assertions on enum members
        if self
            .find_ancestor_kind(target_node, SyntaxKind::EnumMember)
            .is_some()
        {
            return Ok(None);
        }
        // No support for typeof in extends clauses
        if is_expression_target
            && (self
                .find_ancestor_kind(target_node, SyntaxKind::HeritageClause)
                .is_some()
                || self
                    .find_ancestor(target_node, |file, node| {
                        crate::completions::is_type_node_kind(file.arena.node(node).kind)
                    })
                    .is_some())
        {
            return Ok(None);
        }
        // Can't inline type spread elements
        if self.kind(target_node) == SyntaxKind::SpreadElement {
            return Ok(None);
        }
        let variable_type =
            match self.find_ancestor_kind(target_node, SyntaxKind::VariableDeclaration) {
                Some(variable_declaration) => Some(
                    checker
                        .get_type_at_location(variable_declaration)
                        .map_err(check_error)?,
                ),
                None => None,
            };
        // Can't use typeof on unique symbols
        if variable_type.is_some_and(|ty| {
            checker
                .tables
                .flags_of(ty)
                .intersects(TypeFlags::UNIQUE_ES_SYMBOL)
        }) {
            return Ok(None);
        }
        if !is_expression_target && !is_shorthand_property_assignment_target {
            return Ok(None);
        }
        let Some(type_node) = self.infer_type(checker, target_node, variable_type)? else {
            return Ok(None);
        };
        if self.mutated_target {
            return Ok(None);
        }
        if is_shorthand_property_assignment_target {
            // Insert `: expr as Type` after the shorthand property name
            let name = name_of(self.file, target_node).ok_or("a shorthand property's name")?;
            let cloned_name = self.deep_clone(name)?;
            let as_expression = self.create_as_expression(cloned_name, type_node)?;
            self.tracker.insert_node_at(
                self.file,
                self.file.arena.node(target_node).end as usize,
                as_expression,
                crate::change::NodeOptions {
                    prefix: ": ".to_owned(),
                    ..crate::change::NodeOptions::default()
                },
            );
        } else {
            // Replace expression with `(expression) satisfies Type as Type` or `expression satisfies Type as Type`
            let mut cloned_target = self.deep_clone(target_node)?;
            if needs_parenthesized_expression_for_assertion(self.file, target_node) {
                cloned_target = self.create(NodeData::ParenthesizedExpression(
                    ParenthesizedExpressionData {
                        expression: Some(cloned_target.node()),
                    },
                ))?;
            }
            let cloned_type = self
                .tracker
                .arena()
                .factory()
                .deep_clone_node(type_node)
                .map_err(transform_error)?;
            let satisfies =
                self.create(NodeData::SatisfiesExpression(SatisfiesExpressionData {
                    expression: Some(cloned_target.node()),
                    r#type: Some(cloned_type.node()),
                }))?;
            let satisfies_as = self.create(NodeData::AsExpression(AsExpressionData {
                expression: Some(satisfies.node()),
                r#type: Some(type_node.node()),
            }))?;
            self.tracker
                .replace_node(self.file, target_node, satisfies_as, None);
        }
        let type_text = self.type_to_string_for_diag(type_node)?;
        Ok(Some(self.localize(
            &gen::Add_satisfies_and_an_inline_type_assertion_with_0,
            &[&type_text],
        )))
    }

    /// tsgo `extractAsVariable`: `const newLocal = expr` and `newLocal as
    /// typeof newLocal` in its place (an array literal: `as const`).
    fn extract_as_variable(
        &mut self,
        _checker: &mut CheckerState<'_>,
        span: (usize, usize),
    ) -> Description {
        let node_with_diag = token_node_at(self.file, span.0);
        let Some(target_node) = find_best_fitting_node(self.file, node_with_diag, span) else {
            return Ok(None);
        };
        if is_value_signature_declaration(self.kind(target_node))
            || self
                .parent(target_node)
                .is_some_and(|parent| is_value_signature_declaration(self.kind(parent)))
        {
            return Ok(None);
        }
        if !crate::format::is_expression_kind(self.kind(target_node)) {
            return Ok(None);
        }
        // Array literals should be marked as const
        if self.kind(target_node) == SyntaxKind::ArrayLiteralExpression {
            let const_reference = self.const_type_reference()?;
            let cloned = self.deep_clone(target_node)?;
            let as_const = self.create_as_expression(cloned, const_reference)?;
            self.tracker
                .replace_node(self.file, target_node, as_const, None);
            return Ok(Some(self.localize(&gen::Mark_array_literal_as_const, &[])));
        }
        let Some(parent_property_assignment) =
            self.find_ancestor_kind(target_node, SyntaxKind::PropertyAssignment)
        else {
            return Ok(None);
        };
        // Identifiers or entity names can already be typeof-ed
        if Some(parent_property_assignment) == self.parent(target_node)
            && node_util::is_entity_name_expression(self.file, target_node)
        {
            return Ok(None);
        }
        let temp_name =
            self.unique_name(&get_identifier_name_for_node(self.file, target_node), true)?;
        let mut replacement_target = target_node;
        let mut initialization_node = None;
        // Handle spread elements: walk up to the spread's parent and handle const assertions
        if self.kind(replacement_target) == SyntaxKind::SpreadElement {
            let parent = self
                .parent(replacement_target)
                .ok_or("a spread element's parent")?;
            replacement_target = walk_up_parenthesized_expressions(self.file, parent);
            if self
                .parent(replacement_target)
                .is_some_and(|parent| is_const_assertion(self.file, parent))
            {
                replacement_target = self.parent(replacement_target).expect("a const assertion");
            } else {
                let const_reference = self.const_type_reference()?;
                let cloned = self.deep_clone(replacement_target)?;
                initialization_node = Some(self.create_as_expression(cloned, const_reference)?);
            }
        }
        if node_util::is_entity_name_expression(self.file, replacement_target) {
            return Ok(None);
        }
        let initializer = match initialization_node {
            Some(node) => self
                .tracker
                .arena()
                .factory()
                .deep_clone_node(node)
                .map_err(transform_error)?,
            None => self.deep_clone(if self.kind(target_node) == SyntaxKind::SpreadElement {
                replacement_target
            } else {
                target_node
            })?,
        };
        let declaration = self.factory_variable_declaration(temp_name, None, Some(initializer))?;
        let declaration_list = self.variable_declaration_list(declaration, NodeFlags::CONST)?;
        let statement_node = self
            .tracker
            .arena()
            .factory()
            .create_variable_statement(self.target, None, declaration_list)
            .map_err(transform_error)?;
        let Some(statement) = self.find_ancestor(target_node, is_statement) else {
            return Ok(None);
        };
        self.tracker.insert_node_before(
            self.file,
            statement,
            statement_node,
            false,
            LeadingTriviaOption::None,
        );
        let type_query = self
            .tracker
            .arena()
            .factory()
            .create_type_query_node(self.target, temp_name, None)
            .map_err(transform_error)?;
        let as_expression = self.create(NodeData::AsExpression(AsExpressionData {
            expression: Some(temp_name.node()),
            r#type: Some(type_query.node()),
        }))?;
        self.tracker
            .replace_node(self.file, replacement_target, as_expression, None);
        let id_text = self.type_to_string_for_diag(temp_name)?;
        Ok(Some(self.localize(
            &gen::Extract_to_variable_and_replace_with_0_as_typeof_0,
            &[&id_text],
        )))
    }

    /// tsgo `fixIsolatedDeclarationError`.
    fn fix_isolated_declaration_error(
        &mut self,
        checker: &mut CheckerState<'_>,
        node: NodeId,
    ) -> Description {
        // Avoid creating duplicate fixes for the same node
        if !self.fixed_nodes.insert(node) {
            return Ok(None);
        }
        match self.kind(node) {
            SyntaxKind::Parameter
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::VariableDeclaration => self.add_type_to_variable_like(checker, node),
            SyntaxKind::ArrowFunction
            | SyntaxKind::FunctionExpression
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor => self.add_type_to_signature_declaration(checker, node),
            SyntaxKind::ExportAssignment => self.transform_export_assignment(checker, node),
            SyntaxKind::ClassDeclaration => {
                self.transform_extends_clause_with_expression(checker, node)
            }
            SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern => {
                self.transform_destructuring_patterns(checker, node)
            }
            _ => Ok(None),
        }
    }

    /// tsgo `addTypeToSignatureDeclaration`.
    fn add_type_to_signature_declaration(
        &mut self,
        checker: &mut CheckerState<'_>,
        function: NodeId,
    ) -> Description {
        if node_util::type_annotation_of(self.file, function).is_some() {
            return Ok(None);
        }
        let Some(type_node) = self.infer_type(checker, function, None)? else {
            return Ok(None);
        };
        self.tracker
            .try_insert_type_annotation(self.file, function, type_node);
        let type_text = self.type_to_string_for_diag(type_node)?;
        Ok(Some(self.localize(&gen::Add_return_type_0, &[&type_text])))
    }

    /// tsgo `transformExportAssignment`: `const _default: T = expr;
    /// export default _default;`.
    fn transform_export_assignment(
        &mut self,
        checker: &mut CheckerState<'_>,
        default_export: NodeId,
    ) -> Description {
        let NodeData::ExportAssignment(data) = self.data(default_export).clone() else {
            return Ok(None);
        };
        if data.is_export_equals == Some(true) {
            return Ok(None);
        }
        let expression = data.expression.ok_or("an export assignment's expression")?;
        let Some(type_node) = self.infer_type(checker, expression, None)? else {
            return Ok(None);
        };
        let default_identifier = self.unique_name("_default", false)?;
        // Deep clone the expression so synthesized nodes don't reference original source positions
        let cloned_expression = self.deep_clone(expression)?;
        let declaration = self.factory_variable_declaration(
            default_identifier,
            Some(type_node),
            Some(cloned_expression),
        )?;
        let declaration_list = self.variable_declaration_list(declaration, NodeFlags::CONST)?;
        let statement = self
            .tracker
            .arena()
            .factory()
            .create_variable_statement(self.target, None, declaration_list)
            .map_err(transform_error)?;
        let original = self.existing(default_export)?;
        let modifiers = data
            .modifiers
            .map(|modifiers| self.existing_array(modifiers))
            .transpose()?;
        let new_export = self
            .tracker
            .arena()
            .factory()
            .update_export_assignment(original, modifiers, default_identifier)
            .map_err(transform_error)?;
        self.tracker.replace_node_with_nodes(
            self.file,
            default_export,
            vec![statement, new_export],
            None,
        );
        Ok(Some(
            self.localize(&gen::Extract_default_export_to_variable, &[]),
        ))
    }

    /// tsgo `transformExtendsClauseWithExpression`: `const CBase: T = expr;`
    /// before the class and `extends CBase`.
    fn transform_extends_clause_with_expression(
        &mut self,
        checker: &mut CheckerState<'_>,
        class_declaration: NodeId,
    ) -> Description {
        let Some(heritage_expression) =
            node_util::get_class_extends_heritage_element(self.file, class_declaration)
        else {
            return Ok(None);
        };
        let NodeData::ExpressionWithTypeArguments(heritage) =
            self.data(heritage_expression).clone()
        else {
            return Ok(None);
        };
        let expression = heritage
            .expression
            .ok_or("a heritage element's expression")?;
        let Some(heritage_type_node) = self.infer_type(checker, expression, None)? else {
            return Ok(None);
        };
        let base_name = match name_of(self.file, class_declaration) {
            Some(name) => format!("{}Base", identifier_text(self.file, name)),
            None => "Anonymous".to_owned(),
        };
        let base_class_name = self.unique_name(&base_name, true)?;
        // Create: const <BaseName>: <type> = <expression>;
        let cloned_expression = self.deep_clone(expression)?;
        let declaration = self.factory_variable_declaration(
            base_class_name,
            Some(heritage_type_node),
            Some(cloned_expression),
        )?;
        let declaration_list = self.variable_declaration_list(declaration, NodeFlags::CONST)?;
        let statement = self
            .tracker
            .arena()
            .factory()
            .create_variable_statement(self.target, None, declaration_list)
            .map_err(transform_error)?;
        self.tracker.insert_node_before(
            self.file,
            class_declaration,
            statement,
            false,
            LeadingTriviaOption::None,
        );
        // Replace the heritage expression with the base class name
        let replacement = self
            .tracker
            .arena()
            .factory()
            .create_expression_with_type_arguments(self.target, base_class_name, None)
            .map_err(transform_error)?;
        self.tracker
            .replace_node(self.file, heritage_expression, replacement, None);
        Ok(Some(
            self.localize(&gen::Extract_base_class_to_variable, &[]),
        ))
    }

    /// tsgo `transformDestructuringPatterns`: one annotated variable per
    /// binding element.
    fn transform_destructuring_patterns(
        &mut self,
        checker: &mut CheckerState<'_>,
        binding_pattern: NodeId,
    ) -> Description {
        let Some(enclosing_variable_declaration) = self.parent(binding_pattern) else {
            return Ok(None);
        };
        if self.kind(enclosing_variable_declaration) != SyntaxKind::VariableDeclaration {
            return Ok(None);
        }
        let Some(enclosing_variable_statement) = self
            .parent(enclosing_variable_declaration)
            .and_then(|list| self.parent(list))
            .filter(|&statement| self.kind(statement) == SyntaxKind::VariableStatement)
        else {
            return Ok(None);
        };
        let NodeData::VariableDeclaration(variable) =
            self.data(enclosing_variable_declaration).clone()
        else {
            return Ok(None);
        };
        let Some(initializer) = variable.initializer else {
            return Ok(None);
        };
        let mut new_nodes = Vec::new();
        let base_expression = if self.kind(initializer) != SyntaxKind::Identifier {
            // Create a temporary variable for complex expressions
            let temp_name = self.unique_name("dest", true)?;
            let cloned_initializer = self.deep_clone(initializer)?;
            let declaration =
                self.factory_variable_declaration(temp_name, None, Some(cloned_initializer))?;
            let declaration_list = self.variable_declaration_list(declaration, NodeFlags::CONST)?;
            new_nodes.push(
                self.tracker
                    .arena()
                    .factory()
                    .create_variable_statement(self.target, None, declaration_list)
                    .map_err(transform_error)?,
            );
            temp_name
        } else {
            // Use a new identifier to avoid referencing original source positions
            let text = identifier_text(self.file, initializer);
            self.identifier(&text)?
        };
        // Extract each binding element as a separate variable with type annotation
        self.extract_binding_elements(
            checker,
            binding_pattern,
            base_expression,
            &mut new_nodes,
            enclosing_variable_statement,
        )?;
        if new_nodes.is_empty() {
            return Ok(None);
        }
        // If the enclosing variable statement has multiple declarations, preserve the non-destructuring ones
        let NodeData::VariableStatement(statement) =
            self.data(enclosing_variable_statement).clone()
        else {
            return Ok(None);
        };
        let declaration_list = statement
            .declaration_list
            .ok_or("a variable statement's list")?;
        let NodeData::VariableDeclarationList(list) = self.data(declaration_list).clone() else {
            return Ok(None);
        };
        let declarations = list
            .declarations
            .map(|declarations| self.file.arena.node_array(declarations).nodes.to_vec())
            .unwrap_or_default();
        if declarations.len() > 1 {
            let remaining: Vec<NodeId> = declarations
                .into_iter()
                .filter(|&declaration| declaration != enclosing_variable_declaration)
                .collect();
            if !remaining.is_empty() {
                let remaining = remaining
                    .into_iter()
                    .map(|declaration| self.existing(declaration))
                    .collect::<FixResult<Vec<_>>>()?;
                let remaining = self.node_array(remaining)?;
                let original_list = self.existing(declaration_list)?;
                let updated_list = self
                    .tracker
                    .arena()
                    .factory()
                    .update_variable_declaration_list(original_list, remaining)
                    .map_err(transform_error)?;
                let original_statement = self.existing(enclosing_variable_statement)?;
                let modifiers = statement
                    .modifiers
                    .map(|modifiers| self.existing_array(modifiers))
                    .transpose()?;
                new_nodes.push(
                    self.tracker
                        .arena()
                        .factory()
                        .update_variable_statement(original_statement, modifiers, updated_list)
                        .map_err(transform_error)?,
                );
            }
        }
        self.tracker.replace_node_with_nodes(
            self.file,
            enclosing_variable_statement,
            new_nodes,
            None,
        );
        Ok(Some(self.localize(
            &gen::Extract_binding_expressions_to_variable,
            &[],
        )))
    }

    /// tsgo `extractBindingElements`.
    fn extract_binding_elements(
        &mut self,
        checker: &mut CheckerState<'_>,
        binding_pattern: NodeId,
        base_expression: TransformNode,
        new_nodes: &mut Vec<TransformNode>,
        enclosing_variable_statement: NodeId,
    ) -> FixResult<()> {
        let elements = match self.data(binding_pattern) {
            NodeData::ObjectBindingPattern(data) => data.elements,
            NodeData::ArrayBindingPattern(data) => data.elements,
            _ => None,
        };
        let elements = elements
            .map(|elements| self.file.arena.node_array(elements).nodes.to_vec())
            .unwrap_or_default();
        let is_object = self.kind(binding_pattern) == SyntaxKind::ObjectBindingPattern;
        for (index, element) in elements.into_iter().enumerate() {
            if self.kind(element) == SyntaxKind::OmittedExpression {
                continue;
            }
            let NodeData::BindingElement(binding) = self.data(element).clone() else {
                continue;
            };
            let Some(name) = binding.name else {
                continue;
            };
            let base = base_expression;
            let access_expression = if is_object {
                // Build property access expression
                match binding.property_name {
                    Some(property_name)
                        if self.kind(property_name) == SyntaxKind::ComputedPropertyName =>
                    {
                        // Handle computed property names: create a temp variable for the computed expression
                        let NodeData::ComputedPropertyName(computed) =
                            self.data(property_name).clone()
                        else {
                            continue;
                        };
                        let computed_expression =
                            computed.expression.ok_or("a computed name's expression")?;
                        let is_member_name = matches!(
                            self.kind(computed_expression),
                            SyntaxKind::Identifier | SyntaxKind::PrivateIdentifier
                        );
                        let computed_expression = self.existing(computed_expression)?;
                        let mut factory = self.tracker.arena().factory();
                        let identifier_for_computed_property = if is_member_name {
                            factory.get_generated_name_for_node(
                                computed_expression,
                                GeneratedIdentifierFlags::NONE,
                                None,
                                None,
                            )
                        } else {
                            factory.get_generated_name_for_non_member_node(computed_expression)
                        }
                        .map_err(transform_error)?;
                        let declaration = self.factory_variable_declaration(
                            identifier_for_computed_property,
                            None,
                            Some(computed_expression),
                        )?;
                        let declaration_list =
                            self.variable_declaration_list(declaration, NodeFlags::CONST)?;
                        new_nodes.push(
                            self.tracker
                                .arena()
                                .factory()
                                .create_variable_statement(self.target, None, declaration_list)
                                .map_err(transform_error)?,
                        );
                        self.tracker
                            .arena()
                            .factory()
                            .create_element_access_expression(
                                self.target,
                                base,
                                identifier_for_computed_property,
                            )
                            .map_err(transform_error)?
                    }
                    Some(property_name) => {
                        // Use property name text (handles identifiers, string literals, numeric literals)
                        let text = property_name_text(self.file, property_name);
                        let name = self.identifier(&text)?;
                        self.tracker
                            .arena()
                            .factory()
                            .create_property_access_expression(self.target, base, name)
                            .map_err(transform_error)?
                    }
                    None if self.kind(name) == SyntaxKind::Identifier => {
                        let text = identifier_text(self.file, name);
                        let name = self.identifier(&text)?;
                        self.tracker
                            .arena()
                            .factory()
                            .create_property_access_expression(self.target, base, name)
                            .map_err(transform_error)?
                    }
                    None => continue,
                }
            } else {
                let index = self
                    .tracker
                    .arena()
                    .factory()
                    .create_numeric_literal(self.target, index.to_string())
                    .map_err(transform_error)?;
                self.tracker
                    .arena()
                    .factory()
                    .create_element_access_expression(self.target, base, index)
                    .map_err(transform_error)?
            };
            if node_util::is_binding_pattern(self.file, name) {
                self.extract_binding_elements(
                    checker,
                    name,
                    access_expression,
                    new_nodes,
                    enclosing_variable_statement,
                )?;
            } else {
                self.emit_binding_element_variable(
                    checker,
                    name,
                    &binding,
                    access_expression,
                    new_nodes,
                    enclosing_variable_statement,
                )?;
            }
        }
        Ok(())
    }

    /// tsgo `emitBindingElementVariable`: a variable for one binding
    /// element, a default initializer through `temp === undefined ?
    /// default : temp`.
    fn emit_binding_element_variable(
        &mut self,
        checker: &mut CheckerState<'_>,
        name: NodeId,
        binding: &tsc_syntax::nodes::BindingElementData,
        access_expression: TransformNode,
        new_nodes: &mut Vec<TransformNode>,
        enclosing_variable_statement: NodeId,
    ) -> FixResult<()> {
        let type_node = self.infer_type(checker, name, None)?;
        let mut variable_initializer = access_expression;
        if let Some(initializer) = binding.initializer {
            // Create a temp variable to hold the accessed value, then use a conditional expression
            // to apply the default: temp === undefined ? defaultValue : temp
            let temp_base_name = match binding.property_name {
                Some(property_name) if self.kind(property_name) == SyntaxKind::Identifier => {
                    identifier_text(self.file, property_name)
                }
                _ => "temp".to_owned(),
            };
            let temp_name = self.unique_name(&temp_base_name, true)?;
            let declaration =
                self.factory_variable_declaration(temp_name, None, Some(variable_initializer))?;
            let declaration_list = self.variable_declaration_list(declaration, NodeFlags::CONST)?;
            new_nodes.push(
                self.tracker
                    .arena()
                    .factory()
                    .create_variable_statement(self.target, None, declaration_list)
                    .map_err(transform_error)?,
            );
            let equals = self.token(SyntaxKind::EqualsEqualsEqualsToken)?;
            let undefined = self.identifier("undefined")?;
            let condition = self.create(NodeData::BinaryExpression(BinaryExpressionData {
                left: Some(temp_name.node()),
                operator_token: Some(equals.node()),
                right: Some(undefined.node()),
            }))?;
            let question = self.token(SyntaxKind::QuestionToken)?;
            let colon = self.token(SyntaxKind::ColonToken)?;
            let default_value = self.existing(initializer)?;
            variable_initializer =
                self.create(NodeData::ConditionalExpression(ConditionalExpressionData {
                    condition: Some(condition.node()),
                    question_token: Some(question.node()),
                    when_true: Some(default_value.node()),
                    colon_token: Some(colon.node()),
                    when_false: Some(variable_initializer.node()),
                }))?;
        }
        let export_modifier = self.get_export_modifier(enclosing_variable_statement)?;
        let text = identifier_text(self.file, name);
        let name = self.identifier(&text)?;
        let declaration =
            self.factory_variable_declaration(name, type_node, Some(variable_initializer))?;
        let declaration_list = self.variable_declaration_list(declaration, NodeFlags::CONST)?;
        new_nodes.push(
            self.tracker
                .arena()
                .factory()
                .create_variable_statement(self.target, export_modifier, declaration_list)
                .map_err(transform_error)?,
        );
        Ok(())
    }

    /// tsgo `getExportModifier`.
    fn get_export_modifier(
        &mut self,
        enclosing_variable_statement: NodeId,
    ) -> FixResult<Option<TransformNodeArray>> {
        if node_util::has_syntactic_modifier(
            self.file,
            enclosing_variable_statement,
            ModifierFlags::EXPORT,
        ) {
            let export_token = self.token(SyntaxKind::ExportKeyword)?;
            return self.node_array(vec![export_token]).map(Some);
        }
        Ok(None)
    }

    /// tsgo `inferType`: the node's type (a signature's return type or type
    /// predicate) as a type node, per the print mode.
    fn infer_type(
        &mut self,
        checker: &mut CheckerState<'_>,
        node: NodeId,
        variable_type: Option<TypeId>,
    ) -> FixResult<Option<TransformNode>> {
        self.mutated_target = false;
        // Handle Relative mode first: return typeof X for identifiers
        if self.type_print_mode == TypePrintMode::Relative {
            return self.relative_type(node);
        }
        let mut ty = if is_value_signature_declaration(self.kind(node)) {
            let signature = checker
                .get_signature_from_declaration(node)
                .map_err(check_error)?;
            if let Some(predicate) = checker
                .get_type_predicate_of_signature(signature)
                .map_err(check_error)?
            {
                let Some(predicate_type) = predicate.ty else {
                    return Ok(None);
                };
                let enclosing_declaration = self.enclosing_declaration(node);
                let mut flags = EmitNodeBuilderFlags::DECLARATION_EMIT.0;
                if checker
                    .tables
                    .flags_of(predicate_type)
                    .intersects(TypeFlags::UNIQUE_ES_SYMBOL)
                {
                    flags |= EmitNodeBuilderFlags::ALLOW_UNIQUE_ES_SYMBOL_TYPE.0;
                }
                let Some((node, _)) = checker.type_predicate_to_type_predicate_node_in_arena(
                    self.tracker.arena(),
                    self.target,
                    &predicate,
                    Some(enclosing_declaration),
                    flags,
                )?
                else {
                    return Ok(None);
                };
                // Built in the checker's own emit context.
                self.tracker
                    .arena()
                    .forget_emit_context(node)
                    .map_err(transform_error)?;
                return Ok(Some(node));
            }
            checker
                .get_return_type_of_signature(signature)
                .map_err(check_error)?
        } else {
            checker.get_type_at_location(node).map_err(check_error)?
        };
        // Handle Widened mode: return widened literal type if different
        if self.type_print_mode == TypePrintMode::Widened {
            if let Some(variable_type) = variable_type {
                ty = variable_type;
            }
            let widened_type = checker.get_widened_literal_type(ty).map_err(check_error)?;
            if checker
                .is_type_assignable_to(widened_type, ty)
                .map_err(check_error)?
            {
                return Ok(None); // widened type is same, no fix needed
            }
            ty = widened_type;
        }
        let enclosing_declaration = self.enclosing_declaration(node);
        let flags =
            EmitNodeBuilderFlags::DECLARATION_EMIT.0 | self.get_extra_flags(checker, node, ty);
        // For parameters that require adding implicit undefined, add it to the type
        if self.kind(node) == SyntaxKind::Parameter
            && checker
                .requires_adding_implicit_undefined(node)
                .map_err(check_error)?
        {
            let undefined = checker.tables.intrinsics.undefined;
            ty = checker
                .get_union_type_ex(&[undefined, ty], tsc_types::UnionReduction::None)
                .map_err(check_error)?;
        }
        self.type_to_minimized_reference_type(checker, ty, enclosing_declaration, flags)
    }

    /// tsgo `getExtraFlags`: a unique symbol type is allowed for a variable
    /// and a static or readonly property.
    fn get_extra_flags(&self, checker: &CheckerState<'_>, node: NodeId, ty: TypeId) -> u32 {
        let kind = self.kind(node);
        if (kind == SyntaxKind::VariableDeclaration
            || kind == SyntaxKind::PropertyDeclaration
                && node_util::has_syntactic_modifier(
                    self.file,
                    node,
                    ModifierFlags::STATIC | ModifierFlags::READONLY,
                ))
            && checker
                .tables
                .flags_of(ty)
                .intersects(TypeFlags::UNIQUE_ES_SYMBOL)
        {
            return EmitNodeBuilderFlags::ALLOW_UNIQUE_ES_SYMBOL_TYPE.0;
        }
        0
    }

    /// tsgo `createTypeOfFromEntityNameExpression`: `typeof X`.
    fn create_type_of_from_entity_name_expression(
        &mut self,
        node: TransformNode,
    ) -> FixResult<TransformNode> {
        let name = self
            .tracker
            .arena()
            .factory()
            .deep_clone_node(node)
            .map_err(transform_error)?;
        self.tracker
            .arena()
            .factory()
            .create_type_query_node(self.target, name, None)
            .map_err(transform_error)
    }

    /// tsgo `typeFromArraySpreadElements`: an array literal in a const
    /// context split into variables, the tuple of their spreads.
    fn type_from_array_spread_elements(
        &mut self,
        node: NodeId,
        name: &str,
    ) -> FixResult<Option<TransformNode>> {
        let is_in_const_context = self.find_ancestor(node, is_const_assertion).is_some();
        if !is_in_const_context {
            return Ok(None);
        }
        let name = if name.is_empty() { "temp" } else { name };
        self.type_from_spreads(node, name, is_in_const_context, Spreads::Array)
    }

    /// tsgo `typeFromObjectSpreadAssignment`: an object literal split into
    /// variables, the intersection of their types.
    fn type_from_object_spread_assignment(
        &mut self,
        node: NodeId,
        name: &str,
    ) -> FixResult<Option<TransformNode>> {
        let is_in_const_context = self.find_ancestor(node, is_const_assertion).is_some();
        let name = if name.is_empty() { "temp" } else { name };
        self.type_from_spreads(node, name, is_in_const_context, Spreads::Object)
    }

    /// tsgo `typeFromSpreads`: the literal's spreads kept, the runs of other
    /// elements between them each made a const variable, and the composed
    /// type of `typeof` each part.
    fn type_from_spreads(
        &mut self,
        node: NodeId,
        name: &str,
        is_in_const_context: bool,
        spreads: Spreads,
    ) -> FixResult<Option<TransformNode>> {
        let mut intersection_types = Vec::new();
        let mut new_spreads = Vec::new();
        let mut current_variable_properties = Vec::new();
        let statement = self.find_ancestor(node, is_statement);
        let children = match (spreads, self.data(node)) {
            (Spreads::Array, NodeData::ArrayLiteralExpression(data)) => data.elements,
            (Spreads::Object, NodeData::ObjectLiteralExpression(data)) => data.properties,
            _ => None,
        };
        let children = children
            .map(|children| self.file.arena.node_array(children).nodes.to_vec())
            .unwrap_or_default();
        for property in children {
            let is_spread = matches!(
                (spreads, self.kind(property)),
                (Spreads::Array, SyntaxKind::SpreadElement)
                    | (Spreads::Object, SyntaxKind::SpreadAssignment)
            );
            if is_spread {
                self.finalizes_variable_part(
                    name,
                    is_in_const_context,
                    statement,
                    spreads,
                    &mut current_variable_properties,
                    &mut intersection_types,
                    &mut new_spreads,
                )?;
                let expression = match self.data(property) {
                    NodeData::SpreadElement(data) => data.expression,
                    NodeData::SpreadAssignment(data) => data.expression,
                    _ => None,
                }
                .ok_or("a spread's expression")?;
                if node_util::is_entity_name_expression(self.file, expression) {
                    let expression_node = self.existing(expression)?;
                    intersection_types
                        .push(self.create_type_of_from_entity_name_expression(expression_node)?);
                    new_spreads.push(self.existing(property)?);
                } else {
                    let expression = self.existing(expression)?;
                    self.make_spread_variable(
                        name,
                        is_in_const_context,
                        statement,
                        spreads,
                        expression,
                        &mut intersection_types,
                        &mut new_spreads,
                    )?;
                }
            } else {
                current_variable_properties.push(self.existing(property)?);
            }
        }
        if new_spreads.is_empty() {
            return Ok(None);
        }
        self.finalizes_variable_part(
            name,
            is_in_const_context,
            statement,
            spreads,
            &mut current_variable_properties,
            &mut intersection_types,
            &mut new_spreads,
        )?;
        let replacement = self.make_node_of_kind(spreads, new_spreads)?;
        self.tracker
            .replace_node(self.file, node, replacement, None);
        self.mutated_target = true;
        let types = match spreads {
            Spreads::Array => {
                let mut rest_types = Vec::with_capacity(intersection_types.len());
                for ty in intersection_types {
                    rest_types.push(
                        self.tracker
                            .arena()
                            .factory()
                            .create_rest_type_node(self.target, ty)
                            .map_err(transform_error)?,
                    );
                }
                let elements = self.node_array(rest_types)?;
                self.tracker
                    .arena()
                    .factory()
                    .create_tuple_type_node(self.target, elements)
                    .map_err(transform_error)?
            }
            Spreads::Object => {
                let types = self.node_array(intersection_types)?;
                self.tracker
                    .arena()
                    .factory()
                    .create_intersection_type_node(self.target, types)
                    .map_err(transform_error)?
            }
        };
        Ok(Some(types))
    }

    /// tsgo `makeSpreadVariable`: `const <name>_Part<n> = expr` (in a const
    /// context `as const`) before the statement, its spread and `typeof`.
    #[allow(clippy::too_many_arguments)]
    fn make_spread_variable(
        &mut self,
        name: &str,
        is_in_const_context: bool,
        statement: Option<NodeId>,
        spreads: Spreads,
        expression: TransformNode,
        intersection_types: &mut Vec<TransformNode>,
        new_spreads: &mut Vec<TransformNode>,
    ) -> FixResult<()> {
        let temp_name = self.unique_name(&format!("{name}_Part{}", new_spreads.len() + 1), true)?;
        let cloned = self
            .tracker
            .arena()
            .factory()
            .deep_clone_node(expression)
            .map_err(transform_error)?;
        let initializer = if is_in_const_context {
            let const_reference = self.const_type_reference()?;
            self.create(NodeData::AsExpression(AsExpressionData {
                expression: Some(cloned.node()),
                r#type: Some(const_reference.node()),
            }))?
        } else {
            cloned
        };
        let declaration = self.factory_variable_declaration(temp_name, None, Some(initializer))?;
        let declaration_list = self.variable_declaration_list(declaration, NodeFlags::CONST)?;
        let variable_statement = self
            .tracker
            .arena()
            .factory()
            .create_variable_statement(self.target, None, declaration_list)
            .map_err(transform_error)?;
        if let Some(statement) = statement {
            self.tracker.insert_node_before(
                self.file,
                statement,
                variable_statement,
                false,
                LeadingTriviaOption::None,
            );
        }
        intersection_types.push(self.create_type_of_from_entity_name_expression(temp_name)?);
        new_spreads.push(self.create_spread(spreads, temp_name)?);
        Ok(())
    }

    /// tsgo `finalizesVariablePart`: the run of other elements so far as a
    /// spread variable.
    #[allow(clippy::too_many_arguments)]
    fn finalizes_variable_part(
        &mut self,
        name: &str,
        is_in_const_context: bool,
        statement: Option<NodeId>,
        spreads: Spreads,
        current_variable_properties: &mut Vec<TransformNode>,
        intersection_types: &mut Vec<TransformNode>,
        new_spreads: &mut Vec<TransformNode>,
    ) -> FixResult<()> {
        if current_variable_properties.is_empty() {
            return Ok(());
        }
        let properties = std::mem::take(current_variable_properties);
        let node = self.make_node_of_kind(spreads, properties)?;
        self.make_spread_variable(
            name,
            is_in_const_context,
            statement,
            spreads,
            node,
            intersection_types,
            new_spreads,
        )
    }

    /// tsgo's `makeNodeOfKind`: a multi-line array or object literal.
    fn make_node_of_kind(
        &mut self,
        spreads: Spreads,
        elements: Vec<TransformNode>,
    ) -> FixResult<TransformNode> {
        let elements = self.node_array(elements)?;
        let node = match spreads {
            Spreads::Array => self.create(NodeData::ArrayLiteralExpression(
                ArrayLiteralExpressionData {
                    elements: Some(elements.array()),
                },
            ))?,
            Spreads::Object => self.create(NodeData::ObjectLiteralExpression(
                ObjectLiteralExpressionData {
                    properties: Some(elements.array()),
                },
            ))?,
        };
        self.tracker
            .arena()
            .factory()
            .set_multi_line(node, true)
            .map_err(transform_error)
    }

    /// tsgo's `createSpread`.
    fn create_spread(
        &mut self,
        spreads: Spreads,
        expression: TransformNode,
    ) -> FixResult<TransformNode> {
        match spreads {
            Spreads::Array => self.create(NodeData::SpreadElement(SpreadElementData {
                expression: Some(expression.node()),
            })),
            Spreads::Object => self.create(NodeData::SpreadAssignment(SpreadAssignmentData {
                expression: Some(expression.node()),
            })),
        }
    }

    /// tsgo `relativeType`: `typeof X` for an entity name, a literal with
    /// spreads split into variables, the union of a conditional's branches.
    fn relative_type(&mut self, node: NodeId) -> FixResult<Option<TransformNode>> {
        match self.data(node).clone() {
            NodeData::Parameter(_) => Ok(None),
            NodeData::ShorthandPropertyAssignment(data) => {
                let name = self.existing(data.name.ok_or("a shorthand property's name")?)?;
                self.create_type_of_from_entity_name_expression(name)
                    .map(Some)
            }
            _ if node_util::is_entity_name_expression(self.file, node) => {
                let node = self.existing(node)?;
                self.create_type_of_from_entity_name_expression(node)
                    .map(Some)
            }
            NodeData::AsExpression(data) if is_const_assertion(self.file, node) => {
                self.relative_type(data.expression.ok_or("an assertion's expression")?)
            }
            NodeData::TypeAssertionExpression(data) if is_const_assertion(self.file, node) => {
                self.relative_type(data.expression.ok_or("an assertion's expression")?)
            }
            NodeData::ArrayLiteralExpression(_) => {
                let part_name = self.variable_name_for_parts(node);
                self.type_from_array_spread_elements(node, &part_name)
            }
            NodeData::ObjectLiteralExpression(_) => {
                let part_name = self.variable_name_for_parts(node);
                self.type_from_object_spread_assignment(node, &part_name)
            }
            NodeData::VariableDeclaration(data) => match data.initializer {
                Some(initializer) => self.relative_type(initializer),
                None => Ok(None),
            },
            NodeData::ConditionalExpression(data) => {
                let Some(true_type) =
                    self.relative_type(data.when_true.ok_or("a conditional's branch")?)?
                else {
                    return Ok(None);
                };
                let true_mutated = self.mutated_target;
                let Some(false_type) =
                    self.relative_type(data.when_false.ok_or("a conditional's branch")?)?
                else {
                    return Ok(None);
                };
                self.mutated_target = true_mutated || self.mutated_target;
                let types = self.node_array(vec![true_type, false_type])?;
                self.tracker
                    .arena()
                    .factory()
                    .create_union_type_node(self.target, types)
                    .map(Some)
                    .map_err(transform_error)
            }
            _ => Ok(None),
        }
    }

    /// The name of the variable declaration around `node`, if an
    /// identifier (the parts' base name).
    fn variable_name_for_parts(&self, node: NodeId) -> String {
        self.find_ancestor_kind(node, SyntaxKind::VariableDeclaration)
            .and_then(|declaration| name_of(self.file, declaration))
            .filter(|&name| self.kind(name) == SyntaxKind::Identifier)
            .map(|name| identifier_text(self.file, name))
            .unwrap_or_default()
    }

    /// tsgo `typeToMinimizedReferenceType`: the type's node, its trailing
    /// type arguments equal to their defaults dropped and its import types
    /// made references to the symbols to import.
    fn type_to_minimized_reference_type(
        &mut self,
        checker: &mut CheckerState<'_>,
        ty: TypeId,
        enclosing_declaration: NodeId,
        flags: u32,
    ) -> FixResult<Option<TransformNode>> {
        // !!! When truncation tracking is supported, check if the type was truncated
        // and return factory.NewKeywordTypeNode(ast.KindAnyKeyword) instead of the truncated node.
        let Some((mut type_node, id_to_symbol)) = checker.type_to_type_node_in_arena(
            self.tracker.arena(),
            self.target,
            ty,
            Some(enclosing_declaration),
            flags,
            EmitInternalNodeBuilderFlags::WRITE_COMPUTED_PROPS.0,
        )?
        else {
            return Ok(None);
        };
        // Built in the checker's own emit context (tsgo `TypeToTypeNodeEx`).
        self.tracker
            .arena()
            .forget_emit_context(type_node)
            .map_err(transform_error)?;
        let type_reference = match &self
            .tracker
            .arena()
            .node(type_node)
            .map_err(transform_error)?
            .data
        {
            NodeData::TypeReference(data) => Some(data.clone()),
            _ => None,
        };
        if let Some(reference) = type_reference.filter(|_| {
            checker
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::REFERENCE)
        }) {
            let type_arguments = checker.get_type_arguments(ty).map_err(check_error)?;
            let node_type_arguments = match reference.type_arguments {
                Some(list) => {
                    let array = self
                        .tracker
                        .arena()
                        .node_array_ref(self.target, list)
                        .ok_or("a type argument list")?;
                    self.tracker
                        .arena()
                        .node_array(array)
                        .map_err(transform_error)?
                        .nodes
                        .to_vec()
                }
                None => Vec::new(),
            };
            if !type_arguments.is_empty() && !node_type_arguments.is_empty() {
                let cutoff = end_of_required_type_parameters(checker, ty)?;
                if cutoff < node_type_arguments.len() {
                    // Trim trailing default type arguments
                    let trimmed = node_type_arguments[..cutoff]
                        .iter()
                        .map(|&id| {
                            self.tracker
                                .arena()
                                .node_ref(self.target, id)
                                .ok_or("a type argument")
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let trimmed = self.node_array(trimmed)?;
                    let type_name = self
                        .tracker
                        .arena()
                        .node_ref(
                            self.target,
                            reference.type_name.ok_or("a type reference's name")?,
                        )
                        .ok_or("a type reference's name")?;
                    type_node = self
                        .tracker
                        .arena()
                        .factory()
                        .update_type_reference_node(type_node, type_name, Some(trimmed))
                        .map_err(transform_error)?;
                }
            }
        }
        // Convert import type references (e.g. import("./path").Name) to simple type references
        // and collect symbols that need to be imported
        let (reference_type_node, importable_symbols) =
            try_get_auto_importable_reference_from_type_node(
                checker,
                self.tracker.arena(),
                type_node,
                &id_to_symbol,
            )
            .map_err(transform_error)?;
        self.symbols_to_import.extend(importable_symbols);
        Ok(Some(reference_type_node))
    }

    /// tsgo `addTypeToVariableLike`.
    fn add_type_to_variable_like(
        &mut self,
        checker: &mut CheckerState<'_>,
        declaration: NodeId,
    ) -> Description {
        let Some(type_node) = self.infer_type(checker, declaration, None)? else {
            return Ok(None);
        };
        if let Some(existing_type) = node_util::type_annotation_of(self.file, declaration) {
            self.tracker
                .replace_node(self.file, existing_type, type_node, None);
        } else {
            self.tracker
                .try_insert_type_annotation(self.file, declaration, type_node);
            // Parenthesize paren-less arrow function parameters (`x => ...`) so the inserted `: T`
            // produces `(x: T) => ...` instead of the invalid `x: T => ...`. Queued after the type
            // annotation so that the `)` edit at param.End() sorts after the annotation insertion.
            if self.kind(declaration) == SyntaxKind::Parameter {
                if let Some(parent) = self
                    .parent(declaration)
                    .filter(|&parent| self.kind(parent) == SyntaxKind::ArrowFunction)
                {
                    self.tracker
                        .parenthesize_arrow_parameters(self.file, parent);
                }
            }
        }
        let type_text = self.type_to_string_for_diag(type_node)?;
        Ok(Some(
            self.localize(&gen::Add_annotation_of_type_0, &[&type_text]),
        ))
    }

    /// tsgo `addSymbolToExistingImport` for each symbol to import: the
    /// symbol added to the named imports of the file's import of its
    /// module.
    fn add_symbols_to_existing_imports(&mut self, checker: &mut CheckerState<'_>) -> FixResult<()> {
        for symbol in std::mem::take(&mut self.symbols_to_import) {
            self.add_symbol_to_existing_import(checker, symbol)?;
        }
        Ok(())
    }

    /// tsgo `addSymbolToExistingImport`.
    fn add_symbol_to_existing_import(
        &mut self,
        checker: &mut CheckerState<'_>,
        symbol: SymbolId,
    ) -> FixResult<()> {
        // Find the module specifier for this symbol
        let Some(module_symbol) = checker.binder.symbol(symbol).parent else {
            return Ok(());
        };
        let symbol_name = checker
            .binder
            .symbol(symbol)
            .escaped_name
            .unescape()
            .to_string_lossy()
            .into_owned();
        let statements = match &self.file.arena.node(self.file.root).data {
            NodeData::SourceFile(data) => data.statements,
            _ => None,
        };
        let statements = statements
            .map(|statements| self.file.arena.node_array(statements).nodes.to_vec())
            .unwrap_or_default();
        // Walk the source file's import declarations to find the one importing from the same module
        for statement in statements {
            let NodeData::ImportDeclaration(import) = self.data(statement).clone() else {
                continue;
            };
            let Some(import_clause) = import.import_clause else {
                continue;
            };
            let Some(module_specifier) = import.module_specifier else {
                continue;
            };
            // Check if this import is from the same module
            let import_module_symbol = checker
                .get_symbol_at_location(module_specifier)
                .map_err(check_error)?;
            if !import_module_symbol.is_some_and(|import_module_symbol| {
                checker.get_merged_symbol(import_module_symbol)
                    == checker.get_merged_symbol(module_symbol)
            }) {
                continue;
            }
            // Found the matching import - add the symbol to named imports
            let NodeData::ImportClause(clause) = self.data(import_clause).clone() else {
                return Ok(());
            };
            if let Some(named_bindings) = clause
                .named_bindings
                .filter(|&bindings| self.kind(bindings) == SyntaxKind::NamedImports)
            {
                // Add to existing named imports
                let NodeData::NamedImports(named_imports) = self.data(named_bindings).clone()
                else {
                    return Ok(());
                };
                let mut elements = named_imports
                    .elements
                    .map(|elements| self.file.arena.node_array(elements).nodes.to_vec())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|element| self.existing(element))
                    .collect::<FixResult<Vec<_>>>()?;
                let name = self.identifier(&symbol_name)?;
                elements.push(
                    self.tracker
                        .arena()
                        .factory()
                        .create_import_specifier(self.target, false, None, name)
                        .map_err(transform_error)?,
                );
                let elements = self.node_array(elements)?;
                let new_named_imports = self
                    .tracker
                    .arena()
                    .factory()
                    .create_named_imports(self.target, elements)
                    .map_err(transform_error)?;
                let original_clause = self.existing(import_clause)?;
                let clause_name = clause.name.map(|name| self.existing(name)).transpose()?;
                let new_import_clause = self
                    .tracker
                    .arena()
                    .factory()
                    .update_import_clause(
                        original_clause,
                        clause.phase_modifier,
                        clause_name,
                        Some(new_named_imports),
                    )
                    .map_err(transform_error)?;
                let original_import = self.existing(statement)?;
                let modifiers = import
                    .modifiers
                    .map(|modifiers| self.existing_array(modifiers))
                    .transpose()?;
                let module_specifier = self.existing(module_specifier)?;
                let attributes = import
                    .attributes
                    .map(|attributes| self.existing(attributes))
                    .transpose()?;
                let new_import = self
                    .tracker
                    .arena()
                    .factory()
                    .update_import_declaration(
                        original_import,
                        modifiers,
                        Some(new_import_clause),
                        module_specifier,
                        attributes,
                    )
                    .map_err(transform_error)?;
                self.tracker
                    .replace_node(self.file, statement, new_import, None);
            }
            return Ok(());
        }
        Ok(())
    }

    /// tsgo `typeToStringForDiag`.
    fn type_to_string_for_diag(&mut self, type_node: TransformNode) -> FixResult<String> {
        self.tracker
            .print_single_line(type_node)
            .map_err(|error| error.to_string())
    }

    /// The nearest declaration from `node` (tsgo `ast.IsDeclaration`), else
    /// the file.
    fn enclosing_declaration(&self, node: NodeId) -> NodeId {
        self.find_ancestor(node, node_util::is_declaration_node)
            .unwrap_or(self.file.root)
    }

    fn find_ancestor(
        &self,
        node: NodeId,
        predicate: impl Fn(&SourceFile, NodeId) -> bool,
    ) -> Option<NodeId> {
        let mut current = Some(node);
        while let Some(node) = current {
            if predicate(self.file, node) {
                return Some(node);
            }
            current = self.parent(node);
        }
        None
    }

    fn find_ancestor_kind(&self, node: NodeId, kind: SyntaxKind) -> Option<NodeId> {
        self.find_ancestor(node, |file, node| file.arena.node(node).kind == kind)
    }

    fn kind(&self, node: NodeId) -> SyntaxKind {
        self.file.arena.node(node).kind
    }

    fn data(&self, node: NodeId) -> &'a NodeData {
        &self.file.arena.node(node).data
    }

    fn parent(&self, node: NodeId) -> Option<NodeId> {
        self.file.arena.node(node).parent
    }

    fn localize(&self, message: &'static DiagnosticMessage, args: &[&str]) -> String {
        crate::lsutil::localize(self.catalog, message, args)
    }

    /// The tracker's copy of a node of the file.
    fn existing(&mut self, node: NodeId) -> FixResult<TransformNode> {
        self.tracker
            .arena()
            .node_ref(self.target, node)
            .ok_or_else(|| "a node outside the file".to_owned())
    }

    fn existing_array(&mut self, array: tsc_syntax::NodeArrayId) -> FixResult<TransformNodeArray> {
        self.tracker
            .arena()
            .node_array_ref(self.target, array)
            .ok_or_else(|| "a node array outside the file".to_owned())
    }

    /// tsgo `DeepCloneNode` of a node of the file.
    fn deep_clone(&mut self, node: NodeId) -> FixResult<TransformNode> {
        let node = self.existing(node)?;
        self.tracker
            .arena()
            .factory()
            .deep_clone_node(node)
            .map_err(transform_error)
    }

    fn create(&mut self, data: NodeData) -> FixResult<TransformNode> {
        self.tracker
            .arena()
            .factory()
            .create_node(self.target, data, TransformFlags::NONE)
            .map_err(transform_error)
    }

    fn identifier(&mut self, text: &str) -> FixResult<TransformNode> {
        self.tracker
            .arena()
            .factory()
            .create_identifier(self.target, text)
            .map_err(transform_error)
    }

    fn token(&mut self, kind: SyntaxKind) -> FixResult<TransformNode> {
        self.tracker
            .arena()
            .factory()
            .create_token(self.target, kind, TransformFlags::NONE)
            .map_err(transform_error)
    }

    fn node_array(&mut self, nodes: Vec<TransformNode>) -> FixResult<TransformNodeArray> {
        self.tracker
            .arena()
            .factory()
            .create_node_array(self.target, nodes)
            .map_err(transform_error)
    }

    /// tsgo `NewUniqueName` (optimistic: `NewUniqueNameEx` with
    /// `GeneratedIdentifierFlagsOptimistic`).
    fn unique_name(&mut self, text: &str, optimistic: bool) -> FixResult<TransformNode> {
        let flags = if optimistic {
            GeneratedIdentifierFlags::OPTIMISTIC
        } else {
            GeneratedIdentifierFlags::NONE
        };
        self.tracker
            .arena()
            .factory()
            .create_unique_name(self.target, text, flags)
            .map_err(transform_error)
    }

    /// `const`, the type reference of a const assertion.
    fn const_type_reference(&mut self) -> FixResult<TransformNode> {
        let name = self.identifier("const")?;
        self.tracker
            .arena()
            .factory()
            .create_type_reference_node(self.target, name, None)
            .map_err(transform_error)
    }

    /// tsgo `createAsExpression`: `expr as T`, the expression in parentheses
    /// when it needs them.
    fn create_as_expression(
        &mut self,
        node: TransformNode,
        type_node: TransformNode,
    ) -> FixResult<TransformNode> {
        let needs_parentheses = {
            let arena = self.tracker.arena();
            let record = arena.node(node).map_err(transform_error)?;
            !matches!(
                record.kind,
                SyntaxKind::CallExpression
                    | SyntaxKind::ObjectLiteralExpression
                    | SyntaxKind::ArrayLiteralExpression
            ) && !is_entity_name_expression_in(arena, node)?
        };
        let expression = if needs_parentheses {
            self.create(NodeData::ParenthesizedExpression(
                ParenthesizedExpressionData {
                    expression: Some(node.node()),
                },
            ))?
        } else {
            node
        };
        self.create(NodeData::AsExpression(AsExpressionData {
            expression: Some(expression.node()),
            r#type: Some(type_node.node()),
        }))
    }

    fn factory_variable_declaration(
        &mut self,
        name: TransformNode,
        type_node: Option<TransformNode>,
        initializer: Option<TransformNode>,
    ) -> FixResult<TransformNode> {
        self.tracker
            .arena()
            .factory()
            .create_variable_declaration(self.target, name, None, type_node, initializer)
            .map_err(transform_error)
    }

    fn variable_declaration_list(
        &mut self,
        declaration: TransformNode,
        flags: NodeFlags,
    ) -> FixResult<TransformNode> {
        let declarations = self.node_array(vec![declaration])?;
        self.tracker
            .arena()
            .factory()
            .create_variable_declaration_list(self.target, declarations, flags)
            .map_err(transform_error)
    }
}

/// Which literal `typeFromSpreads` splits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Spreads {
    Array,
    Object,
}

/// tsgo `endOfRequiredTypeParameters`: the number of type arguments that
/// differ from what their defaults fill in.
fn end_of_required_type_parameters(checker: &mut CheckerState<'_>, ty: TypeId) -> FixResult<usize> {
    let type_arguments = checker.get_type_arguments(ty).map_err(check_error)?;
    if type_arguments.is_empty() {
        return Ok(0);
    }
    let target = match &checker.tables.type_of(ty).data {
        TypeData::Reference { target, .. } => Some(*target),
        _ => None,
    };
    let Some(target) = target.filter(|&target| {
        checker
            .tables
            .object_flags_of(target)
            .intersects(ObjectFlags::CLASS_OR_INTERFACE)
    }) else {
        return Ok(type_arguments.len());
    };
    let parts = checker.get_interface_type_parts(target);
    let type_parameters = parts.type_parameters;
    let local_type_parameters = &type_parameters[parts.outer_type_parameter_count..];
    let outer_count = type_parameters.len() - local_type_parameters.len();
    for cutoff in 0..type_arguments.len() {
        // Skip cutoff positions where the local type parameter has no default.
        // This matches TS's check for constraint === undefined on localTypeParameters,
        // which in practice skips type parameters without defaults (e.g. Set<T>
        // where T has no default should not have <unknown> elided).
        let Some(local_index) = cutoff.checked_sub(outer_count) else {
            continue;
        };
        if local_index >= local_type_parameters.len()
            || !type_parameter_has_default(checker, local_type_parameters[local_index])
        {
            continue;
        }
        let filled_in = checker
            .fill_missing_type_arguments(
                Some(&type_arguments[..cutoff]),
                Some(&type_parameters),
                cutoff,
                false,
            )
            .map_err(check_error)?
            .unwrap_or_default();
        if filled_in
            .iter()
            .zip(&type_arguments)
            .all(|(fill, argument)| fill == argument)
        {
            return Ok(cutoff);
        }
    }
    Ok(type_arguments.len())
}

/// tsgo `typeParamHasDefault`.
fn type_parameter_has_default(checker: &CheckerState<'_>, type_parameter: TypeId) -> bool {
    let Some(symbol) = checker.tables.type_of(type_parameter).symbol else {
        return false;
    };
    checker
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .any(|&declaration| {
            matches!(
                &checker.binder.source_of_node(declaration).arena.node(declaration).data,
                NodeData::TypeParameter(data) if data.default.is_some()
            )
        })
}

/// tsgo `astnav.GetTokenAtPosition` as a node: a punctuation or keyword
/// token is no node here, its parent is.
fn token_node_at(file: &SourceFile, position: usize) -> NodeId {
    let navigator = Navigator::new(file);
    let token = navigator.token_at(position as u32);
    match token {
        Found::Node(View::Node(node)) => node,
        found => match navigator.parent(found) {
            Some(Found::Node(View::Node(parent))) => parent,
            _ => file.root,
        },
    }
}

/// tsgo `isExpandoPropertyDeclarationForFix`.
fn is_expando_property_declaration_for_fix(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::BinaryExpression
    )
}

/// tsgo `findExpandoFunction`: the function declaration (or the variable
/// of a function expression) whose expando property assignment `node` is
/// in.
fn find_expando_function(
    checker: &mut CheckerState<'_>,
    file: &SourceFile,
    node: NodeId,
) -> FixResult<Option<NodeId>> {
    let mut current = Some(node);
    let mut expando_declaration = None;
    while let Some(candidate) = current {
        if is_statement(file, candidate) {
            break;
        }
        if is_expando_property_declaration_for_fix(file.arena.node(candidate).kind) {
            expando_declaration = Some(candidate);
            break;
        }
        current = file.arena.node(candidate).parent;
    }
    let Some(expando_declaration) = expando_declaration else {
        return Ok(None);
    };
    let mut assignment_target = expando_declaration;
    // Some late bound expando members use the whole expression as the declaration.
    if let NodeData::BinaryExpression(binary) = &file.arena.node(assignment_target).data {
        let Some(left) = binary
            .left
            .filter(|&left| is_expando_property_declaration_for_fix(file.arena.node(left).kind))
        else {
            return Ok(None);
        };
        assignment_target = left;
    }
    let expression = match &file.arena.node(assignment_target).data {
        NodeData::PropertyAccessExpression(data) => data.expression,
        NodeData::ElementAccessExpression(data) => data.expression,
        _ => None,
    };
    let Some(expression) = expression else {
        return Ok(None);
    };
    let target_type = checker
        .get_type_at_location(expression)
        .map_err(check_error)?;
    let properties = checker
        .get_properties_of_type(target_type)
        .map_err(check_error)?;
    let parent = file.arena.node(expando_declaration).parent;
    let found = properties.iter().any(|&property| {
        let value_declaration = checker.binder.symbol(property).value_declaration;
        value_declaration == Some(expando_declaration)
            || value_declaration.is_some() && value_declaration == parent
    });
    if !found {
        return Ok(None);
    }
    let Some(symbol) = checker.tables.type_of(target_type).symbol else {
        return Ok(None);
    };
    let Some(function) = checker.binder.symbol(symbol).value_declaration else {
        return Ok(None);
    };
    let function_source = checker.binder.source_of_node(function);
    if !std::ptr::eq(function_source, file) {
        return Ok(None);
    }
    let record = file.arena.node(function);
    if matches!(
        record.kind,
        SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
    ) && record
        .parent
        .is_some_and(|parent| file.arena.node(parent).kind == SyntaxKind::VariableDeclaration)
    {
        return Ok(record.parent);
    }
    if record.kind == SyntaxKind::FunctionDeclaration {
        return Ok(Some(function));
    }
    Ok(None)
}

/// tsgo `canHaveTypeAnnotationKinds`.
fn can_have_type_annotation_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::GetAccessor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::VariableDeclaration
            | SyntaxKind::Parameter
            | SyntaxKind::ExportAssignment
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::ObjectBindingPattern
            | SyntaxKind::ArrayBindingPattern
    )
}

/// tsgo `findAncestorWithMissingType`: the nearest node that can have a
/// type annotation (a binding pattern only of a variable).
fn find_ancestor_with_missing_type(file: &SourceFile, node: NodeId) -> Option<NodeId> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        let record = file.arena.node(candidate);
        let is_binding_pattern = matches!(
            record.kind,
            SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
        );
        if can_have_type_annotation_kind(record.kind)
            && (!is_binding_pattern
                || record.parent.is_some_and(|parent| {
                    file.arena.node(parent).kind == SyntaxKind::VariableDeclaration
                }))
        {
            return Some(candidate);
        }
        current = record.parent;
    }
    None
}

/// tsgo `findBestFittingNode`: from the token up to the node that covers
/// the span, and up through parents of the same range; an identifier of a
/// declaration with an initializer gives the initializer, of a shorthand
/// property the property.
fn find_best_fitting_node(file: &SourceFile, node: NodeId, span: (usize, usize)) -> Option<NodeId> {
    let mut node = node;
    while (file.arena.node(node).end as usize) < span.1 {
        node = file.arena.node(node).parent?;
    }
    while let Some(parent) = file.arena.node(node).parent {
        let (parent_record, record) = (file.arena.node(parent), file.arena.node(node));
        if parent_record.pos != record.pos || parent_record.end != record.end {
            break;
        }
        node = parent;
    }
    if file.arena.node(node).kind == SyntaxKind::Identifier {
        if let Some(parent) = file.arena.node(node).parent {
            if let Some(initializer) = node_util::initializer_of(file, parent) {
                return Some(initializer);
            }
            if file.arena.node(parent).kind == SyntaxKind::ShorthandPropertyAssignment {
                return Some(parent);
            }
        }
    }
    Some(node)
}

/// tsgo `isNamedDeclarationKind`: tsc's `isDeclarationKind`.
fn is_named_declaration_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ArrowFunction
            | SyntaxKind::BindingElement
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassExpression
            | SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::EnumMember
            | SyntaxKind::ExportSpecifier
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::GetAccessor
            | SyntaxKind::ImportClause
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::JsxAttribute
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::NamespaceExportDeclaration
            | SyntaxKind::NamespaceImport
            | SyntaxKind::NamespaceExport
            | SyntaxKind::Parameter
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::SetAccessor
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::TypeParameter
            | SyntaxKind::VariableDeclaration
            | SyntaxKind::JSDocTypedefTag
            | SyntaxKind::JSDocCallbackTag
            | SyntaxKind::JSDocPropertyTag
            | SyntaxKind::NamedTupleMember
    )
}

/// tsgo `isValueSignatureDeclaration`.
fn is_value_signature_declaration(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::Constructor
    )
}

/// tsgo `needsParenthesizedExpressionForAssertion`.
fn needs_parenthesized_expression_for_assertion(file: &SourceFile, node: NodeId) -> bool {
    !node_util::is_entity_name_expression(file, node)
        && !matches!(
            file.arena.node(node).kind,
            SyntaxKind::CallExpression
                | SyntaxKind::ObjectLiteralExpression
                | SyntaxKind::ArrayLiteralExpression
        )
}

/// tsgo `ast.IsEntityNameExpression` of a node in the tracker's arena: an
/// identifier or a property access of one.
fn is_entity_name_expression_in(
    arena: &mut tsc_emitter::TransformArena,
    node: TransformNode,
) -> FixResult<bool> {
    let record = arena.node(node).map_err(transform_error)?;
    match &record.data {
        NodeData::Identifier(_) => Ok(true),
        NodeData::PropertyAccessExpression(data) => {
            let (Some(expression), Some(name)) = (data.expression, data.name) else {
                return Ok(false);
            };
            let name = arena
                .node_ref(node.source(), name)
                .ok_or("a property access's name")?;
            if arena.node(name).map_err(transform_error)?.kind != SyntaxKind::Identifier {
                return Ok(false);
            }
            let expression = arena
                .node_ref(node.source(), expression)
                .ok_or("a property access's expression")?;
            is_entity_name_expression_in(arena, expression)
        }
        _ => Ok(false),
    }
}

/// tsgo `isConstAssertion`: `x as const` or `<const>x`.
fn is_const_assertion(file: &SourceFile, node: NodeId) -> bool {
    let type_node = match &file.arena.node(node).data {
        NodeData::AsExpression(data) => data.r#type,
        NodeData::TypeAssertionExpression(data) => data.r#type,
        _ => return false,
    };
    type_node.is_some_and(|type_node| match &file.arena.node(type_node).data {
        NodeData::TypeReference(data) => {
            data.type_arguments.is_none()
                && data.type_name.is_some_and(|name| {
                    matches!(&file.arena.node(name).data, NodeData::Identifier(identifier) if identifier.text() == "const")
                })
        }
        _ => false,
    })
}

/// tsgo `ast.WalkUpParenthesizedExpressions`.
fn walk_up_parenthesized_expressions(file: &SourceFile, mut node: NodeId) -> NodeId {
    while file.arena.node(node).kind == SyntaxKind::ParenthesizedExpression {
        match file.arena.node(node).parent {
            Some(parent) => node = parent,
            None => break,
        }
    }
    node
}

/// tsgo `getIdentifierNameForNode`: a property access's name, else
/// `newLocal`.
fn get_identifier_name_for_node(file: &SourceFile, node: NodeId) -> String {
    if let NodeData::PropertyAccessExpression(data) = &file.arena.node(node).data {
        if let Some(NodeData::Identifier(name)) = data.name.map(|name| &file.arena.node(name).data)
        {
            if tsc_syntax::identifier_to_keyword_kind(name.text()).is_none() {
                return name.text().to_owned();
            }
        }
    }
    "newLocal".to_owned()
}

/// A declaration's name.
fn name_of(file: &SourceFile, node: NodeId) -> Option<NodeId> {
    node_util::get_name_of_declaration(file, node)
}

/// The text of an identifier.
fn identifier_text(file: &SourceFile, node: NodeId) -> String {
    match &file.arena.node(node).data {
        NodeData::Identifier(identifier) => identifier.text().to_owned(),
        _ => String::new(),
    }
}

/// tsgo `Node.Text()` of a binding element's property name: an
/// identifier's, a string or numeric literal's.
fn property_name_text(file: &SourceFile, node: NodeId) -> String {
    match &file.arena.node(node).data {
        NodeData::Identifier(identifier) => identifier.text().to_owned(),
        NodeData::StringLiteral(literal) => literal.text.to_string_lossy().into_owned(),
        NodeData::NumericLiteral(literal) => literal.text.clone(),
        _ => String::new(),
    }
}

fn transform_error(error: tsc_emitter::TransformError) -> String {
    error.to_string()
}

fn check_error(error: tsc_checker::state::CheckAbort) -> String {
    error.to_string()
}
