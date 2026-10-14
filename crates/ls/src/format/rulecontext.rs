//! tsgo `format/rulecontext.go`: the contexts the rules apply in.

use tsc_syntax::SyntaxKind as K;

use super::api::FormatRequestKind;
use super::context::FormattingContext;
use super::file::{is_trivia, Node};
use super::rule::{predicate, ContextPredicate};
use super::scanner::TextRangeWithKind;
use crate::lsutil::{FormatCodeSettings, SemicolonPreference};
use crate::utilities::is_function_like_kind;

/// A tristate option of the settings.
type OptionSelector = fn(&FormatCodeSettings) -> Option<bool>;

/// tsgo `optionEquals(semicolonOption, value)`.
pub(super) fn option_semicolons_equal(value: SemicolonPreference) -> ContextPredicate {
    predicate(move |context| context.options.semicolons == value)
}

/// tsgo `isOptionEnabled`.
pub(super) fn is_option_enabled(option: OptionSelector) -> ContextPredicate {
    predicate(move |context| option(context.options) == Some(true))
}

/// tsgo `isOptionDisabled`.
pub(super) fn is_option_disabled(option: OptionSelector) -> ContextPredicate {
    predicate(move |context| option(context.options) == Some(false))
}

/// tsgo `isOptionDisabledOrUndefined`.
pub(super) fn is_option_disabled_or_undefined(option: OptionSelector) -> ContextPredicate {
    predicate(move |context| option(context.options) != Some(true))
}

/// tsgo `isOptionDisabledOrUndefinedOrTokensOnSameLine`.
pub(super) fn is_option_disabled_or_undefined_or_tokens_on_same_line(
    option: OptionSelector,
) -> ContextPredicate {
    predicate(move |context| {
        option(context.options) != Some(true) || context.tokens_are_on_same_line()
    })
}

/// tsgo `isOptionEnabledOrUndefined`.
pub(super) fn is_option_enabled_or_undefined(option: OptionSelector) -> ContextPredicate {
    predicate(move |context| option(context.options) != Some(false))
}

fn context_kind(context: &FormattingContext<'_>) -> K {
    context.file.kind(context.context_node)
}

pub(super) fn is_for_context(context: &FormattingContext<'_>) -> bool {
    context_kind(context) == K::ForStatement
}

pub(super) fn is_not_for_context(context: &FormattingContext<'_>) -> bool {
    !is_for_context(context)
}

pub(super) fn is_binary_op_context(context: &FormattingContext<'_>) -> bool {
    let current = context.current_token_span.kind;
    let next = context.next_token_span.kind;
    match context_kind(context) {
        K::BinaryExpression => {
            context.file.operator_kind(context.context_node) != Some(K::CommaToken)
        }
        K::ConditionalExpression
        | K::ConditionalType
        | K::AsExpression
        | K::ExportSpecifier
        | K::ImportSpecifier
        | K::TypePredicate
        | K::UnionType
        | K::IntersectionType
        | K::SatisfiesExpression => true,

        // equals in binding elements func foo([[x, y] = [1, 2]])
        // equals in type X = ...
        // equal in import a = module('a');
        // equal in export = 1
        // equal in let a = 0
        // equal in p = 0
        K::BindingElement
        | K::TypeAliasDeclaration
        | K::ImportEqualsDeclaration
        | K::ExportAssignment
        | K::VariableDeclaration
        | K::Parameter
        | K::EnumMember
        | K::PropertyDeclaration
        | K::PropertySignature => current == K::EqualsToken || next == K::EqualsToken,
        // "in" keyword in for (let x in []) { }
        // "in" keyword in [P in keyof T] T[P]
        K::ForInStatement | K::TypeParameter => {
            current == K::InKeyword
                || next == K::InKeyword
                || current == K::EqualsToken
                || next == K::EqualsToken
        }
        // Technically, "of" is not a binary operator, but format it the same way as "in"
        K::ForOfStatement => current == K::OfKeyword || next == K::OfKeyword,
        _ => false,
    }
}

pub(super) fn is_not_binary_op_context(context: &FormattingContext<'_>) -> bool {
    !is_binary_op_context(context)
}

pub(super) fn is_not_type_annotation_context(context: &FormattingContext<'_>) -> bool {
    !is_type_annotation_context(context)
}

pub(super) fn is_type_annotation_context(context: &FormattingContext<'_>) -> bool {
    let context_kind = context_kind(context);
    context_kind == K::PropertyDeclaration
        || context_kind == K::PropertySignature
        || context_kind == K::Parameter
        || context_kind == K::VariableDeclaration
        || is_function_like_kind(context_kind)
}

fn is_optional_property_context(context: &FormattingContext<'_>) -> bool {
    context_kind(context) == K::PropertyDeclaration
        && context
            .file
            .property_declaration_has_question_token(context.context_node)
}

pub(super) fn is_non_optional_property_context(context: &FormattingContext<'_>) -> bool {
    !is_optional_property_context(context)
}

pub(super) fn is_conditional_operator_context(context: &FormattingContext<'_>) -> bool {
    matches!(
        context_kind(context),
        K::ConditionalExpression | K::ConditionalType
    )
}

pub(super) fn is_same_line_token_or_before_block_context(context: &FormattingContext<'_>) -> bool {
    context.tokens_are_on_same_line() || is_before_block_context(context)
}

pub(super) fn is_brace_wrapped_context(context: &FormattingContext<'_>) -> bool {
    matches!(
        context_kind(context),
        K::ObjectBindingPattern | K::MappedType
    ) || is_single_line_block_context(context)
}

/// This check is done before an open brace in a control construct, a
/// function, or a typescript block declaration.
pub(super) fn is_before_multiline_block_context(context: &FormattingContext<'_>) -> bool {
    is_before_block_context(context)
        && !(context.next_node_all_on_same_line() || context.next_node_block_is_on_one_line())
}

pub(super) fn is_multiline_block_context(context: &FormattingContext<'_>) -> bool {
    is_block_context(context)
        && !(context.context_node_all_on_same_line() || context.context_node_block_is_on_one_line())
}

fn is_single_line_block_context(context: &FormattingContext<'_>) -> bool {
    is_block_context(context)
        && (context.context_node_all_on_same_line() || context.context_node_block_is_on_one_line())
}

fn is_block_context(context: &FormattingContext<'_>) -> bool {
    node_is_block_context(context, context.context_node)
}

pub(super) fn is_before_block_context(context: &FormattingContext<'_>) -> bool {
    node_is_block_context(context, context.next_token_parent)
}

/// IMPORTANT!!! This method must return true ONLY for nodes with open and
/// close braces as immediate children.
fn node_is_block_context(context: &FormattingContext<'_>, node: Node) -> bool {
    let kind = context.file.kind(node);
    // This means we are in a context that looks like a block to the user, but in the grammar is actually not a node (it's a class, module, enum, object type literal, etc).
    node_is_type_script_decl_with_block_context(kind)
        || matches!(
            kind,
            K::Block | K::CaseBlock | K::ObjectLiteralExpression | K::ModuleBlock
        )
}

pub(super) fn is_function_decl_context(context: &FormattingContext<'_>) -> bool {
    matches!(
        context_kind(context),
        K::FunctionDeclaration
            | K::MethodDeclaration
            | K::MethodSignature
            | K::GetAccessor
            | K::SetAccessor
            | K::CallSignature
            | K::FunctionExpression
            | K::Constructor
            | K::ArrowFunction
            // This one is not truly a function, but for formatting purposes, it acts just like one
            | K::InterfaceDeclaration
    )
}

pub(super) fn is_not_function_decl_context(context: &FormattingContext<'_>) -> bool {
    !is_function_decl_context(context)
}

pub(super) fn is_function_declaration_or_function_expression_context(
    context: &FormattingContext<'_>,
) -> bool {
    matches!(
        context_kind(context),
        K::FunctionDeclaration | K::FunctionExpression
    )
}

pub(super) fn is_type_script_decl_with_block_context(context: &FormattingContext<'_>) -> bool {
    node_is_type_script_decl_with_block_context(context_kind(context))
}

fn node_is_type_script_decl_with_block_context(kind: K) -> bool {
    matches!(
        kind,
        K::ClassDeclaration
            | K::ClassExpression
            | K::InterfaceDeclaration
            | K::EnumDeclaration
            | K::TypeLiteral
            | K::ModuleDeclaration
            | K::ExportDeclaration
            | K::NamedExports
            | K::ImportDeclaration
            | K::NamedImports
    )
}

pub(super) fn is_after_code_block_context(context: &FormattingContext<'_>) -> bool {
    let file = context.file;
    match file.kind(context.current_token_parent) {
        K::ClassDeclaration
        | K::ModuleDeclaration
        | K::EnumDeclaration
        | K::CatchClause
        | K::ModuleBlock
        | K::SwitchStatement => true,
        K::Block => {
            // In a codefix scenario, we can't rely on parents being set. So just always return true.
            file.parent(context.current_token_parent)
                .is_none_or(|block_parent| {
                    !matches!(
                        file.kind(block_parent),
                        K::ArrowFunction | K::FunctionExpression
                    )
                })
        }
        _ => false,
    }
}

pub(super) fn is_control_decl_context(context: &FormattingContext<'_>) -> bool {
    matches!(
        context_kind(context),
        K::IfStatement
            | K::SwitchStatement
            | K::ForStatement
            | K::ForInStatement
            | K::ForOfStatement
            | K::WhileStatement
            | K::TryStatement
            | K::DoStatement
            | K::WithStatement
            | K::CatchClause
    )
}

pub(super) fn is_object_context(context: &FormattingContext<'_>) -> bool {
    context_kind(context) == K::ObjectLiteralExpression
}

pub(super) fn is_function_call_or_new_context(context: &FormattingContext<'_>) -> bool {
    matches!(context_kind(context), K::CallExpression | K::NewExpression)
}

pub(super) fn is_previous_token_not_comma(context: &FormattingContext<'_>) -> bool {
    context.current_token_span.kind != K::CommaToken
}

pub(super) fn is_next_token_not_close_bracket(context: &FormattingContext<'_>) -> bool {
    context.next_token_span.kind != K::CloseBracketToken
}

pub(super) fn is_next_token_not_close_paren(context: &FormattingContext<'_>) -> bool {
    context.next_token_span.kind != K::CloseParenToken
}

pub(super) fn is_arrow_function_context(context: &FormattingContext<'_>) -> bool {
    context_kind(context) == K::ArrowFunction
}

pub(super) fn is_import_type_context(context: &FormattingContext<'_>) -> bool {
    context_kind(context) == K::ImportType
}

pub(super) fn is_non_jsx_same_line_token_context(context: &FormattingContext<'_>) -> bool {
    context.tokens_are_on_same_line() && context_kind(context) != K::JsxText
}

pub(super) fn is_non_jsx_text_context(context: &FormattingContext<'_>) -> bool {
    context_kind(context) != K::JsxText
}

pub(super) fn is_non_jsx_element_or_fragment_context(context: &FormattingContext<'_>) -> bool {
    !matches!(context_kind(context), K::JsxElement | K::JsxFragment)
}

pub(super) fn is_jsx_expression_context(context: &FormattingContext<'_>) -> bool {
    matches!(
        context_kind(context),
        K::JsxExpression | K::JsxSpreadAttribute
    )
}

pub(super) fn is_next_token_parent_jsx_attribute(context: &FormattingContext<'_>) -> bool {
    let file = context.file;
    match file.kind(context.next_token_parent) {
        K::JsxAttribute => true,
        K::JsxNamespacedName => file
            .parent(context.next_token_parent)
            .is_some_and(|parent| file.kind(parent) == K::JsxAttribute),
        _ => false,
    }
}

pub(super) fn is_jsx_attribute_context(context: &FormattingContext<'_>) -> bool {
    context_kind(context) == K::JsxAttribute
}

pub(super) fn is_next_token_parent_not_jsx_namespaced_name(
    context: &FormattingContext<'_>,
) -> bool {
    context.file.kind(context.next_token_parent) != K::JsxNamespacedName
}

pub(super) fn is_next_token_parent_jsx_namespaced_name(context: &FormattingContext<'_>) -> bool {
    context.file.kind(context.next_token_parent) == K::JsxNamespacedName
}

pub(super) fn is_jsx_self_closing_element_context(context: &FormattingContext<'_>) -> bool {
    context_kind(context) == K::JsxSelfClosingElement
}

pub(super) fn is_not_before_block_in_function_declaration_context(
    context: &FormattingContext<'_>,
) -> bool {
    !is_function_decl_context(context) && !is_before_block_context(context)
}

pub(super) fn is_end_of_decorator_context_on_same_line(context: &FormattingContext<'_>) -> bool {
    context.tokens_are_on_same_line()
        && context.file.has_decorators(context.context_node)
        && node_is_in_decorator_context(context, context.current_token_parent)
        && !node_is_in_decorator_context(context, context.next_token_parent)
}

fn node_is_in_decorator_context(context: &FormattingContext<'_>, node: Node) -> bool {
    let file = context.file;
    let mut node = Some(node);
    while let Some(current) =
        node.filter(|&current| super::file::is_expression_kind(file.kind(current)))
    {
        node = file.parent(current);
    }
    node.is_some_and(|node| file.kind(node) == K::Decorator)
}

pub(super) fn is_start_of_variable_declaration_list(context: &FormattingContext<'_>) -> bool {
    context.file.kind(context.current_token_parent) == K::VariableDeclarationList
        && context.file.token_start(context.current_token_parent) == context.current_token_span.pos
}

pub(super) fn is_not_format_on_enter(context: &FormattingContext<'_>) -> bool {
    context.formatting_request_kind != FormatRequestKind::OnEnter
}

pub(super) fn is_module_decl_context(context: &FormattingContext<'_>) -> bool {
    context_kind(context) == K::ModuleDeclaration
}

pub(super) fn is_object_type_context(context: &FormattingContext<'_>) -> bool {
    context_kind(context) == K::TypeLiteral
}

pub(super) fn is_constructor_signature_context(context: &FormattingContext<'_>) -> bool {
    context_kind(context) == K::ConstructSignature
}

fn is_type_argument_or_parameter_or_assertion(
    context: &FormattingContext<'_>,
    token: TextRangeWithKind,
    parent: Node,
) -> bool {
    if token.kind != K::LessThanToken && token.kind != K::GreaterThanToken {
        return false;
    }
    matches!(
        context.file.kind(parent),
        K::TypeReference
            | K::TypeAssertionExpression
            | K::TypeAliasDeclaration
            | K::ClassDeclaration
            | K::ClassExpression
            | K::InterfaceDeclaration
            | K::FunctionDeclaration
            | K::FunctionExpression
            | K::ArrowFunction
            | K::MethodDeclaration
            | K::MethodSignature
            | K::CallSignature
            | K::ConstructSignature
            | K::CallExpression
            | K::NewExpression
            | K::ExpressionWithTypeArguments
    )
}

pub(super) fn is_type_argument_or_parameter_or_assertion_context(
    context: &FormattingContext<'_>,
) -> bool {
    is_type_argument_or_parameter_or_assertion(
        context,
        context.current_token_span,
        context.current_token_parent,
    ) || is_type_argument_or_parameter_or_assertion(
        context,
        context.next_token_span,
        context.next_token_parent,
    )
}

pub(super) fn is_type_assertion_context(context: &FormattingContext<'_>) -> bool {
    context_kind(context) == K::TypeAssertionExpression
}

pub(super) fn is_non_type_assertion_context(context: &FormattingContext<'_>) -> bool {
    !is_type_assertion_context(context)
}

pub(super) fn is_void_op_context(context: &FormattingContext<'_>) -> bool {
    context.current_token_span.kind == K::VoidKeyword
        && context.file.kind(context.current_token_parent) == K::VoidExpression
}

pub(super) fn is_yield_or_yield_star_with_operand(context: &FormattingContext<'_>) -> bool {
    context_kind(context) == K::YieldExpression
        && context.file.expression(context.context_node).is_some()
}

pub(super) fn is_non_null_assertion_context(context: &FormattingContext<'_>) -> bool {
    context_kind(context) == K::NonNullExpression
}

pub(super) fn is_not_statement_condition_context(context: &FormattingContext<'_>) -> bool {
    !is_statement_condition_context(context)
}

fn is_statement_condition_context(context: &FormattingContext<'_>) -> bool {
    matches!(
        context_kind(context),
        K::IfStatement
            | K::ForStatement
            | K::ForInStatement
            | K::ForOfStatement
            | K::DoStatement
            | K::WhileStatement
    )
}

pub(super) fn is_semicolon_deletion_context(context: &FormattingContext<'_>) -> bool {
    let file = context.file;
    let mut next_token_kind = context.next_token_span.kind;
    let mut next_token_start = context.next_token_span.pos;
    if is_trivia(next_token_kind) {
        let next_real_token = if context.next_token_parent == context.current_token_parent {
            // !!! TODO: very different from strada, but strada's logic here is wonky - find the first ancestor without a parent? that's just the source file.
            file.find_next_token(context.next_token_parent, file.root())
        } else {
            file.first_token(context.next_token_parent)
        };

        let Some(next_real_token) = next_real_token else {
            return true;
        };
        next_token_kind = file.kind(next_real_token);
        next_token_start = file.token_start(next_real_token);
    }

    let start_line = file.line_of(context.current_token_span.pos);
    let end_line = file.line_of(next_token_start);
    if start_line == end_line {
        return next_token_kind == K::CloseBraceToken || next_token_kind == K::EndOfFileToken;
    }

    if next_token_kind == K::SemicolonToken && context.current_token_span.kind == K::SemicolonToken
    {
        return true;
    }

    if next_token_kind == K::SemicolonClassElement || next_token_kind == K::SemicolonToken {
        return false;
    }

    let current_parent_kind = file.kind(context.current_token_parent);
    if matches!(
        context_kind(context),
        K::InterfaceDeclaration | K::TypeAliasDeclaration
    ) {
        // Can't remove semicolon after `foo`; it would parse as a method declaration:
        //
        // interface I {
        //   foo;
        //   () void
        // }
        return current_parent_kind != K::PropertySignature
            || file
                .property_signature_type(context.current_token_parent)
                .is_some()
            || next_token_kind != K::OpenParenToken;
    }

    if current_parent_kind == K::PropertyDeclaration {
        return file.initializer(context.current_token_parent).is_none();
    }

    current_parent_kind != K::ForStatement
        && current_parent_kind != K::EmptyStatement
        && current_parent_kind != K::SemicolonClassElement
        && next_token_kind != K::OpenBracketToken
        && next_token_kind != K::OpenParenToken
        && next_token_kind != K::PlusToken
        && next_token_kind != K::MinusToken
        && next_token_kind != K::SlashToken
        && next_token_kind != K::RegularExpressionLiteral
        && next_token_kind != K::CommaToken
        && next_token_kind != K::TemplateExpression
        && next_token_kind != K::TemplateHead
        && next_token_kind != K::NoSubstitutionTemplateLiteral
        && next_token_kind != K::DotToken
}

pub(super) fn is_semicolon_insertion_context(context: &FormattingContext<'_>) -> bool {
    crate::lsutil::position_is_asi_candidate(
        &context.file.navigator,
        context.file.line_starts(),
        context.current_token_span.end,
        context.current_token_parent,
    )
}

pub(super) fn is_not_property_access_on_integer_literal(context: &FormattingContext<'_>) -> bool {
    let file = context.file;
    if context_kind(context) != K::PropertyAccessExpression {
        return true;
    }
    let Some(expression) = file.expression(context.context_node) else {
        return true;
    };
    file.kind(expression) != K::NumericLiteral
        || file.numeric_literal_text(expression).contains('.')
}
