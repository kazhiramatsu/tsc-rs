//! tsgo `format/rules.go`: the formatting rules, in priority order.

use tsc_syntax::SyntaxKind as K;

use super::rule::{
    predicate as p, rule, rule_with_flags, RuleAction as A, RuleFlags, RuleSpec, TokenRange,
};
use super::rulecontext::*;
use crate::lsutil::SemicolonPreference;

/// tsgo `getAllRules`: the high-priority common rules, then the
/// user-configurable ones, then the low-priority common ones.
#[rustfmt::skip]
pub(super) fn get_all_rules() -> Vec<RuleSpec> {
    let all_tokens: Vec<K> = (K::FirstToken.value()..=K::LastToken.value())
        .filter_map(K::from_u16)
        .filter(|&token| token != K::EndOfFileToken)
        .collect();

    let any_token_except = |tokens: &[K]| TokenRange {
        tokens: all_tokens
            .iter()
            .copied()
            .filter(|token| !tokens.contains(token))
            .collect(),
        is_specific: false,
    };

    let any_token = TokenRange {
        tokens: all_tokens.clone(),
        is_specific: false,
    };

    let any_token_including_multiline_comments =
        token_range_from_ex(&all_tokens, &[K::MultiLineCommentTrivia]);
    let any_token_including_eof = token_range_from_ex(&all_tokens, &[K::EndOfFileToken]);
    let keywords = token_range_from_range(K::FirstKeyword, K::LastKeyword);
    let binary_operators = token_range_from_range(K::FirstBinaryOperator, K::LastBinaryOperator);
    let binary_keyword_operators = [
        K::InKeyword,
        K::InstanceOfKeyword,
        K::OfKeyword,
        K::AsKeyword,
        K::IsKeyword,
        K::SatisfiesKeyword,
    ];
    let unary_prefix_operators = [
        K::PlusPlusToken,
        K::MinusToken,
        K::TildeToken,
        K::ExclamationToken,
    ];
    let unary_prefix_expressions = [
        K::NumericLiteral,
        K::BigIntLiteral,
        K::Identifier,
        K::OpenParenToken,
        K::OpenBracketToken,
        K::OpenBraceToken,
        K::ThisKeyword,
        K::NewKeyword,
    ];
    let unary_preincrement_expressions = [
        K::Identifier,
        K::OpenParenToken,
        K::ThisKeyword,
        K::NewKeyword,
    ];
    let unary_postincrement_expressions = [
        K::Identifier,
        K::CloseParenToken,
        K::CloseBracketToken,
        K::NewKeyword,
    ];
    let unary_predecrement_expressions = [
        K::Identifier,
        K::OpenParenToken,
        K::ThisKeyword,
        K::NewKeyword,
    ];
    let unary_postdecrement_expressions = [
        K::Identifier,
        K::CloseParenToken,
        K::CloseBracketToken,
        K::NewKeyword,
    ];
    let comments = [K::SingleLineCommentTrivia, K::MultiLineCommentTrivia];
    let type_keywords = [
        K::AnyKeyword,
        K::AssertsKeyword,
        K::BigIntKeyword,
        K::BooleanKeyword,
        K::FalseKeyword,
        K::InferKeyword,
        K::KeyOfKeyword,
        K::NeverKeyword,
        K::NullKeyword,
        K::NumberKeyword,
        K::ObjectKeyword,
        K::ReadonlyKeyword,
        K::StringKeyword,
        K::SymbolKeyword,
        K::TypeOfKeyword,
        K::TrueKeyword,
        K::VoidKeyword,
        K::UndefinedKeyword,
        K::UniqueKeyword,
        K::UnknownKeyword,
    ];
    let type_names: Vec<K> = std::iter::once(K::Identifier).chain(type_keywords).collect();
    let type_names = TokenRange::from(type_names.as_slice());

    // Place a space before open brace in a function declaration
    // TypeScript: Function can have return types, which can be made of tons of different token kinds
    let function_open_brace_left_token_range = any_token_including_multiline_comments.clone();

    // Place a space before open brace in a TypeScript declaration that has braces as children (class, module, enum, etc)
    let type_script_open_brace_left_token_range = TokenRange::from([
        K::Identifier,
        K::GreaterThanToken,
        K::MultiLineCommentTrivia,
        K::ClassKeyword,
        K::ExportKeyword,
        K::ImportKeyword,
    ]);

    // Place a space before open brace in a control flow construct
    let control_open_brace_left_token_range = TokenRange::from([
        K::CloseParenToken,
        K::MultiLineCommentTrivia,
        K::DoKeyword,
        K::TryKeyword,
        K::FinallyKeyword,
        K::ElseKeyword,
        K::CatchKeyword,
    ]);

    // These rules are higher in priority than user-configurable
    let high_priority_common_rules = vec![
        // Leave comments alone
        rule("IgnoreBeforeComment", &any_token, comments, vec![], A::STOP_PROCESSING_SPACE_ACTIONS),
        rule("IgnoreAfterLineComment", K::SingleLineCommentTrivia, &any_token, vec![], A::STOP_PROCESSING_SPACE_ACTIONS),

        rule("NotSpaceBeforeColon", &any_token, K::ColonToken, vec![p(is_non_jsx_same_line_token_context), p(is_not_binary_op_context), p(is_not_type_annotation_context)], A::DELETE_SPACE),
        rule("SpaceAfterColon", K::ColonToken, &any_token, vec![p(is_non_jsx_same_line_token_context), p(is_not_binary_op_context), p(is_next_token_parent_not_jsx_namespaced_name)], A::INSERT_SPACE),
        rule("NoSpaceBeforeQuestionMark", &any_token, K::QuestionToken, vec![p(is_non_jsx_same_line_token_context), p(is_not_binary_op_context), p(is_not_type_annotation_context)], A::DELETE_SPACE),
        // insert space after '?' only when it is used in conditional operator
        rule("SpaceAfterQuestionMarkInConditionalOperator", K::QuestionToken, &any_token, vec![p(is_non_jsx_same_line_token_context), p(is_conditional_operator_context)], A::INSERT_SPACE),

        // in other cases there should be no space between '?' and next token
        rule("NoSpaceAfterQuestionMark", K::QuestionToken, &any_token, vec![p(is_non_jsx_same_line_token_context), p(is_non_optional_property_context)], A::DELETE_SPACE),

        rule("NoSpaceBeforeDot", &any_token, [K::DotToken, K::QuestionDotToken], vec![p(is_non_jsx_same_line_token_context), p(is_not_property_access_on_integer_literal)], A::DELETE_SPACE),
        rule("NoSpaceAfterDot", [K::DotToken, K::QuestionDotToken], &any_token, vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),

        rule("NoSpaceBetweenImportParenInImportType", K::ImportKeyword, K::OpenParenToken, vec![p(is_non_jsx_same_line_token_context), p(is_import_type_context)], A::DELETE_SPACE),

        // Special handling of unary operators.
        // Prefix operators generally shouldn't have a space between
        // them and their target unary expression.
        rule("NoSpaceAfterUnaryPrefixOperator", unary_prefix_operators, unary_prefix_expressions, vec![p(is_non_jsx_same_line_token_context), p(is_not_binary_op_context)], A::DELETE_SPACE),
        rule("NoSpaceAfterUnaryPreincrementOperator", K::PlusPlusToken, unary_preincrement_expressions, vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        rule("NoSpaceAfterUnaryPredecrementOperator", K::MinusMinusToken, unary_predecrement_expressions, vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        rule("NoSpaceBeforeUnaryPostincrementOperator", unary_postincrement_expressions, K::PlusPlusToken, vec![p(is_non_jsx_same_line_token_context), p(is_not_statement_condition_context)], A::DELETE_SPACE),
        rule("NoSpaceBeforeUnaryPostdecrementOperator", unary_postdecrement_expressions, K::MinusMinusToken, vec![p(is_non_jsx_same_line_token_context), p(is_not_statement_condition_context)], A::DELETE_SPACE),

        // More unary operator special-casing.
        // DevDiv 181814: Be careful when removing leading whitespace
        // around unary operators.  Examples:
        //      1 - -2  --X--> 1--2
        //      a + ++b --X--> a+++b
        rule("SpaceAfterPostincrementWhenFollowedByAdd", K::PlusPlusToken, K::PlusToken, vec![p(is_non_jsx_same_line_token_context), p(is_binary_op_context)], A::INSERT_SPACE),
        rule("SpaceAfterAddWhenFollowedByUnaryPlus", K::PlusToken, K::PlusToken, vec![p(is_non_jsx_same_line_token_context), p(is_binary_op_context)], A::INSERT_SPACE),
        rule("SpaceAfterAddWhenFollowedByPreincrement", K::PlusToken, K::PlusPlusToken, vec![p(is_non_jsx_same_line_token_context), p(is_binary_op_context)], A::INSERT_SPACE),
        rule("SpaceAfterPostdecrementWhenFollowedBySubtract", K::MinusMinusToken, K::MinusToken, vec![p(is_non_jsx_same_line_token_context), p(is_binary_op_context)], A::INSERT_SPACE),
        rule("SpaceAfterSubtractWhenFollowedByUnaryMinus", K::MinusToken, K::MinusToken, vec![p(is_non_jsx_same_line_token_context), p(is_binary_op_context)], A::INSERT_SPACE),
        rule("SpaceAfterSubtractWhenFollowedByPredecrement", K::MinusToken, K::MinusMinusToken, vec![p(is_non_jsx_same_line_token_context), p(is_binary_op_context)], A::INSERT_SPACE),

        rule("NoSpaceAfterCloseBrace", K::CloseBraceToken, [K::CommaToken, K::SemicolonToken], vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        // For functions and control block place } on a new line [multi-line rule]
        rule("NewLineBeforeCloseBraceInBlockContext", &any_token_including_multiline_comments, K::CloseBraceToken, vec![p(is_multiline_block_context)], A::INSERT_NEW_LINE),

        // Space/new line after }.
        rule("SpaceAfterCloseBrace", K::CloseBraceToken, any_token_except(&[K::CloseParenToken]), vec![p(is_non_jsx_same_line_token_context), p(is_after_code_block_context)], A::INSERT_SPACE),
        // Special case for (}, else) and (}, while) since else & while tokens are not part of the tree which makes SpaceAfterCloseBrace rule not applied
        // Also should not apply to })
        rule("SpaceBetweenCloseBraceAndElse", K::CloseBraceToken, K::ElseKeyword, vec![p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("SpaceBetweenCloseBraceAndWhile", K::CloseBraceToken, K::WhileKeyword, vec![p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("NoSpaceBetweenEmptyBraceBrackets", K::OpenBraceToken, K::CloseBraceToken, vec![p(is_non_jsx_same_line_token_context), p(is_object_context)], A::DELETE_SPACE),

        // Add a space after control dec context if the next character is an open bracket ex: 'if (false)[a, b] = [1, 2];' -> 'if (false) [a, b] = [1, 2];'
        rule("SpaceAfterConditionalClosingParen", K::CloseParenToken, K::OpenBracketToken, vec![p(is_control_decl_context)], A::INSERT_SPACE),

        rule("NoSpaceBetweenFunctionKeywordAndStar", K::FunctionKeyword, K::AsteriskToken, vec![p(is_function_declaration_or_function_expression_context)], A::DELETE_SPACE),
        rule("SpaceAfterStarInGeneratorDeclaration", K::AsteriskToken, K::Identifier, vec![p(is_function_declaration_or_function_expression_context)], A::INSERT_SPACE),

        rule("SpaceAfterFunctionInFuncDecl", K::FunctionKeyword, &any_token, vec![p(is_function_decl_context)], A::INSERT_SPACE),
        // Insert new line after { and before } in multi-line contexts.
        rule("NewLineAfterOpenBraceInBlockContext", K::OpenBraceToken, &any_token, vec![p(is_multiline_block_context)], A::INSERT_NEW_LINE),

        // For get/set members, we check for (identifier,identifier) since get/set don't have tokens and they are represented as just an identifier token.
        // Though, we do extra check on the context to make sure we are dealing with get/set node. Example:
        //      get x() {}
        //      set x(val) {}
        rule("SpaceAfterGetSetInMember", [K::GetKeyword, K::SetKeyword], K::Identifier, vec![p(is_function_decl_context)], A::INSERT_SPACE),

        rule("NoSpaceBetweenYieldKeywordAndStar", K::YieldKeyword, K::AsteriskToken, vec![p(is_non_jsx_same_line_token_context), p(is_yield_or_yield_star_with_operand)], A::DELETE_SPACE),
        rule("SpaceBetweenYieldOrYieldStarAndOperand", [K::YieldKeyword, K::AsteriskToken], &any_token, vec![p(is_non_jsx_same_line_token_context), p(is_yield_or_yield_star_with_operand)], A::INSERT_SPACE),

        rule("NoSpaceBetweenReturnAndSemicolon", K::ReturnKeyword, K::SemicolonToken, vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        rule("SpaceAfterCertainKeywords", [K::VarKeyword, K::ThrowKeyword, K::NewKeyword, K::DeleteKeyword, K::ReturnKeyword, K::TypeOfKeyword, K::AwaitKeyword], &any_token, vec![p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("SpaceAfterLetConstInVariableDeclaration", [K::LetKeyword, K::ConstKeyword], &any_token, vec![p(is_non_jsx_same_line_token_context), p(is_start_of_variable_declaration_list)], A::INSERT_SPACE),
        rule("NoSpaceBeforeOpenParenInFuncCall", &any_token, K::OpenParenToken, vec![p(is_non_jsx_same_line_token_context), p(is_function_call_or_new_context), p(is_previous_token_not_comma)], A::DELETE_SPACE),

        // Special case for binary operators (that are keywords). For these we have to add a space and shouldn't follow any user options.
        rule("SpaceBeforeBinaryKeywordOperator", &any_token, binary_keyword_operators, vec![p(is_non_jsx_same_line_token_context), p(is_binary_op_context)], A::INSERT_SPACE),
        rule("SpaceAfterBinaryKeywordOperator", binary_keyword_operators, &any_token, vec![p(is_non_jsx_same_line_token_context), p(is_binary_op_context)], A::INSERT_SPACE),

        rule("SpaceAfterVoidOperator", K::VoidKeyword, &any_token, vec![p(is_non_jsx_same_line_token_context), p(is_void_op_context)], A::INSERT_SPACE),

        // Async-await
        rule("SpaceBetweenAsyncAndOpenParen", K::AsyncKeyword, K::OpenParenToken, vec![p(is_arrow_function_context), p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("SpaceBetweenAsyncAndFunctionKeyword", K::AsyncKeyword, [K::FunctionKeyword, K::Identifier], vec![p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),

        // Template string
        rule("NoSpaceBetweenTagAndTemplateString", [K::Identifier, K::CloseParenToken], [K::NoSubstitutionTemplateLiteral, K::TemplateHead], vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),

        // JSX opening elements
        rule("SpaceBeforeJsxAttribute", &any_token, K::Identifier, vec![p(is_next_token_parent_jsx_attribute), p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("SpaceBeforeSlashInJsxOpeningElement", &any_token, K::SlashToken, vec![p(is_jsx_self_closing_element_context), p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("NoSpaceBeforeGreaterThanTokenInJsxOpeningElement", K::SlashToken, K::GreaterThanToken, vec![p(is_jsx_self_closing_element_context), p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        rule("NoSpaceBeforeEqualInJsxAttribute", &any_token, K::EqualsToken, vec![p(is_jsx_attribute_context), p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        rule("NoSpaceAfterEqualInJsxAttribute", K::EqualsToken, &any_token, vec![p(is_jsx_attribute_context), p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        rule("NoSpaceBeforeJsxNamespaceColon", K::Identifier, K::ColonToken, vec![p(is_next_token_parent_jsx_namespaced_name)], A::DELETE_SPACE),
        rule("NoSpaceAfterJsxNamespaceColon", K::ColonToken, K::Identifier, vec![p(is_next_token_parent_jsx_namespaced_name)], A::DELETE_SPACE),

        // TypeScript-specific rules
        // Use of module as a function call. e.g.: import m2 = module("m2");
        rule("NoSpaceAfterModuleImport", [K::ModuleKeyword, K::RequireKeyword], K::OpenParenToken, vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        // Add a space around certain TypeScript keywords
        rule(
            "SpaceAfterCertainTypeScriptKeywords",
            [
                K::AbstractKeyword,
                K::AccessorKeyword,
                K::ClassKeyword,
                K::DeclareKeyword,
                K::DefaultKeyword,
                K::EnumKeyword,
                K::ExportKeyword,
                K::ExtendsKeyword,
                K::GetKeyword,
                K::ImplementsKeyword,
                K::ImportKeyword,
                K::InterfaceKeyword,
                K::ModuleKeyword,
                K::NamespaceKeyword,
                K::OverrideKeyword,
                K::PrivateKeyword,
                K::PublicKeyword,
                K::ProtectedKeyword,
                K::ReadonlyKeyword,
                K::SetKeyword,
                K::StaticKeyword,
                K::TypeKeyword,
                K::FromKeyword,
                K::KeyOfKeyword,
                K::InferKeyword,
            ],
            &any_token,
            vec![p(is_non_jsx_same_line_token_context)],
            A::INSERT_SPACE,
        ),
        rule(
            "SpaceBeforeCertainTypeScriptKeywords",
            &any_token,
            [K::ExtendsKeyword, K::ImplementsKeyword, K::FromKeyword],
            vec![p(is_non_jsx_same_line_token_context)],
            A::INSERT_SPACE,
        ),
        // Treat string literals in module names as identifiers, and add a space between the literal and the opening Brace braces, e.g.: module "m2" {
        rule("SpaceAfterModuleName", K::StringLiteral, K::OpenBraceToken, vec![p(is_module_decl_context)], A::INSERT_SPACE),

        // Lambda expressions
        rule("SpaceBeforeArrow", &any_token, K::EqualsGreaterThanToken, vec![p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("SpaceAfterArrow", K::EqualsGreaterThanToken, &any_token, vec![p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),

        // Optional parameters and let args
        rule("NoSpaceAfterEllipsis", K::DotDotDotToken, K::Identifier, vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        rule("NoSpaceAfterOptionalParameters", K::QuestionToken, [K::CloseParenToken, K::CommaToken], vec![p(is_non_jsx_same_line_token_context), p(is_not_binary_op_context)], A::DELETE_SPACE),

        // Remove spaces in empty interface literals. e.g.: x: {}
        rule("NoSpaceBetweenEmptyInterfaceBraceBrackets", K::OpenBraceToken, K::CloseBraceToken, vec![p(is_non_jsx_same_line_token_context), p(is_object_type_context)], A::DELETE_SPACE),

        // generics and type assertions
        rule("NoSpaceBeforeOpenAngularBracket", &type_names, K::LessThanToken, vec![p(is_non_jsx_same_line_token_context), p(is_type_argument_or_parameter_or_assertion_context)], A::DELETE_SPACE),
        rule("NoSpaceBetweenCloseParenAndAngularBracket", K::CloseParenToken, K::LessThanToken, vec![p(is_non_jsx_same_line_token_context), p(is_type_argument_or_parameter_or_assertion_context)], A::DELETE_SPACE),
        rule("NoSpaceAfterOpenAngularBracket", K::LessThanToken, &any_token, vec![p(is_non_jsx_same_line_token_context), p(is_type_argument_or_parameter_or_assertion_context)], A::DELETE_SPACE),
        rule("NoSpaceBeforeCloseAngularBracket", &any_token, K::GreaterThanToken, vec![p(is_non_jsx_same_line_token_context), p(is_type_argument_or_parameter_or_assertion_context)], A::DELETE_SPACE),
        rule("NoSpaceAfterCloseAngularBracket", K::GreaterThanToken, [K::OpenParenToken, K::OpenBracketToken, K::GreaterThanToken, K::CommaToken], vec![
            p(is_non_jsx_same_line_token_context),
            p(is_type_argument_or_parameter_or_assertion_context),
            p(is_not_function_decl_context), /*To prevent an interference with the SpaceBeforeOpenParenInFuncDecl rule*/
            p(is_non_type_assertion_context),
        ], A::DELETE_SPACE),

        // decorators
        rule("SpaceBeforeAt", [K::CloseParenToken, K::Identifier], K::AtToken, vec![p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("NoSpaceAfterAt", K::AtToken, &any_token, vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        // Insert space after @ in decorator
        rule(
            "SpaceAfterDecorator",
            &any_token,
            [
                K::AbstractKeyword,
                K::Identifier,
                K::ExportKeyword,
                K::DefaultKeyword,
                K::ClassKeyword,
                K::StaticKeyword,
                K::PublicKeyword,
                K::PrivateKeyword,
                K::ProtectedKeyword,
                K::GetKeyword,
                K::SetKeyword,
                K::OpenBracketToken,
                K::AsteriskToken,
            ],
            vec![p(is_end_of_decorator_context_on_same_line)],
            A::INSERT_SPACE,
        ),

        rule("NoSpaceBeforeNonNullAssertionOperator", &any_token, K::ExclamationToken, vec![p(is_non_jsx_same_line_token_context), p(is_non_null_assertion_context)], A::DELETE_SPACE),
        rule("NoSpaceAfterNewKeywordOnConstructorSignature", K::NewKeyword, K::OpenParenToken, vec![p(is_non_jsx_same_line_token_context), p(is_constructor_signature_context)], A::DELETE_SPACE),
        rule("SpaceLessThanAndNonJSXTypeAnnotation", K::LessThanToken, K::LessThanToken, vec![p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
    ];

    // These rules are applied after high priority
    let user_configurable_rules = vec![
        // Treat constructor as an identifier in a function declaration, and remove spaces between constructor and following left parentheses
        rule("SpaceAfterConstructor", K::ConstructorKeyword, K::OpenParenToken, vec![is_option_enabled(|o| o.insert_space_after_constructor), p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("NoSpaceAfterConstructor", K::ConstructorKeyword, K::OpenParenToken, vec![is_option_disabled_or_undefined(|o| o.insert_space_after_constructor), p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),

        rule("SpaceAfterComma", K::CommaToken, &any_token, vec![is_option_enabled(|o| o.insert_space_after_comma_delimiter), p(is_non_jsx_same_line_token_context), p(is_non_jsx_element_or_fragment_context), p(is_next_token_not_close_bracket), p(is_next_token_not_close_paren)], A::INSERT_SPACE),
        rule("NoSpaceAfterComma", K::CommaToken, &any_token, vec![is_option_disabled_or_undefined(|o| o.insert_space_after_comma_delimiter), p(is_non_jsx_same_line_token_context), p(is_non_jsx_element_or_fragment_context)], A::DELETE_SPACE),

        // Insert space after function keyword for anonymous functions
        rule("SpaceAfterAnonymousFunctionKeyword", [K::FunctionKeyword, K::AsteriskToken], K::OpenParenToken, vec![is_option_enabled(|o| o.insert_space_after_function_keyword_for_anonymous_functions), p(is_function_decl_context)], A::INSERT_SPACE),
        rule("NoSpaceAfterAnonymousFunctionKeyword", [K::FunctionKeyword, K::AsteriskToken], K::OpenParenToken, vec![is_option_disabled_or_undefined(|o| o.insert_space_after_function_keyword_for_anonymous_functions), p(is_function_decl_context)], A::DELETE_SPACE),

        // Insert space after keywords in control flow statements
        rule("SpaceAfterKeywordInControl", &keywords, K::OpenParenToken, vec![is_option_enabled(|o| o.insert_space_after_keywords_in_control_flow_statements), p(is_control_decl_context)], A::INSERT_SPACE),
        rule("NoSpaceAfterKeywordInControl", &keywords, K::OpenParenToken, vec![is_option_disabled_or_undefined(|o| o.insert_space_after_keywords_in_control_flow_statements), p(is_control_decl_context)], A::DELETE_SPACE),

        // Insert space after opening and before closing nonempty parenthesis
        rule("SpaceAfterOpenParen", K::OpenParenToken, &any_token, vec![is_option_enabled(|o| o.insert_space_after_opening_and_before_closing_nonempty_parenthesis), p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("SpaceBeforeCloseParen", &any_token, K::CloseParenToken, vec![is_option_enabled(|o| o.insert_space_after_opening_and_before_closing_nonempty_parenthesis), p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("SpaceBetweenOpenParens", K::OpenParenToken, K::OpenParenToken, vec![is_option_enabled(|o| o.insert_space_after_opening_and_before_closing_nonempty_parenthesis), p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("NoSpaceBetweenParens", K::OpenParenToken, K::CloseParenToken, vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        rule("NoSpaceAfterOpenParen", K::OpenParenToken, &any_token, vec![is_option_disabled_or_undefined(|o| o.insert_space_after_opening_and_before_closing_nonempty_parenthesis), p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        rule("NoSpaceBeforeCloseParen", &any_token, K::CloseParenToken, vec![is_option_disabled_or_undefined(|o| o.insert_space_after_opening_and_before_closing_nonempty_parenthesis), p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),

        // Insert space after opening and before closing nonempty brackets
        rule("SpaceAfterOpenBracket", K::OpenBracketToken, &any_token, vec![is_option_enabled(|o| o.insert_space_after_opening_and_before_closing_nonempty_brackets), p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("SpaceBeforeCloseBracket", &any_token, K::CloseBracketToken, vec![is_option_enabled(|o| o.insert_space_after_opening_and_before_closing_nonempty_brackets), p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule("NoSpaceBetweenBrackets", K::OpenBracketToken, K::CloseBracketToken, vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        rule("NoSpaceAfterOpenBracket", K::OpenBracketToken, &any_token, vec![is_option_disabled_or_undefined(|o| o.insert_space_after_opening_and_before_closing_nonempty_brackets), p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        rule("NoSpaceBeforeCloseBracket", &any_token, K::CloseBracketToken, vec![is_option_disabled_or_undefined(|o| o.insert_space_after_opening_and_before_closing_nonempty_brackets), p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),

        // Insert a space after { and before } in single-line contexts, but remove space from empty object literals {}.
        rule("SpaceAfterOpenBrace", K::OpenBraceToken, &any_token, vec![is_option_enabled_or_undefined(|o| o.insert_space_after_opening_and_before_closing_nonempty_braces), p(is_brace_wrapped_context)], A::INSERT_SPACE),
        rule("SpaceBeforeCloseBrace", &any_token, K::CloseBraceToken, vec![is_option_enabled_or_undefined(|o| o.insert_space_after_opening_and_before_closing_nonempty_braces), p(is_brace_wrapped_context)], A::INSERT_SPACE),
        rule("NoSpaceBetweenEmptyBraceBrackets", K::OpenBraceToken, K::CloseBraceToken, vec![p(is_non_jsx_same_line_token_context), p(is_object_context)], A::DELETE_SPACE),
        rule("NoSpaceAfterOpenBrace", K::OpenBraceToken, &any_token, vec![is_option_disabled(|o| o.insert_space_after_opening_and_before_closing_nonempty_braces), p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        rule("NoSpaceBeforeCloseBrace", &any_token, K::CloseBraceToken, vec![is_option_disabled(|o| o.insert_space_after_opening_and_before_closing_nonempty_braces), p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),

        // Insert a space after opening and before closing empty brace brackets
        rule("SpaceBetweenEmptyBraceBrackets", K::OpenBraceToken, K::CloseBraceToken, vec![is_option_enabled(|o| o.insert_space_after_opening_and_before_closing_empty_braces)], A::INSERT_SPACE),
        rule("NoSpaceBetweenEmptyBraceBrackets", K::OpenBraceToken, K::CloseBraceToken, vec![is_option_disabled(|o| o.insert_space_after_opening_and_before_closing_empty_braces), p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),

        // Insert space after opening and before closing template string braces
        rule_with_flags("SpaceAfterTemplateHeadAndMiddle", [K::TemplateHead, K::TemplateMiddle], &any_token, vec![is_option_enabled(|o| o.insert_space_after_opening_and_before_closing_template_string_braces), p(is_non_jsx_text_context)], A::INSERT_SPACE, RuleFlags::CanDeleteNewLines),
        rule("SpaceBeforeTemplateMiddleAndTail", &any_token, [K::TemplateMiddle, K::TemplateTail], vec![is_option_enabled(|o| o.insert_space_after_opening_and_before_closing_template_string_braces), p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
        rule_with_flags("NoSpaceAfterTemplateHeadAndMiddle", [K::TemplateHead, K::TemplateMiddle], &any_token, vec![is_option_disabled_or_undefined(|o| o.insert_space_after_opening_and_before_closing_template_string_braces), p(is_non_jsx_text_context)], A::DELETE_SPACE, RuleFlags::CanDeleteNewLines),
        rule("NoSpaceBeforeTemplateMiddleAndTail", &any_token, [K::TemplateMiddle, K::TemplateTail], vec![is_option_disabled_or_undefined(|o| o.insert_space_after_opening_and_before_closing_template_string_braces), p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),

        // No space after { and before } in JSX expression
        rule("SpaceAfterOpenBraceInJsxExpression", K::OpenBraceToken, &any_token, vec![is_option_enabled(|o| o.insert_space_after_opening_and_before_closing_jsx_expression_braces), p(is_non_jsx_same_line_token_context), p(is_jsx_expression_context)], A::INSERT_SPACE),
        rule("SpaceBeforeCloseBraceInJsxExpression", &any_token, K::CloseBraceToken, vec![is_option_enabled(|o| o.insert_space_after_opening_and_before_closing_jsx_expression_braces), p(is_non_jsx_same_line_token_context), p(is_jsx_expression_context)], A::INSERT_SPACE),
        rule("NoSpaceAfterOpenBraceInJsxExpression", K::OpenBraceToken, &any_token, vec![is_option_disabled_or_undefined(|o| o.insert_space_after_opening_and_before_closing_jsx_expression_braces), p(is_non_jsx_same_line_token_context), p(is_jsx_expression_context)], A::DELETE_SPACE),
        rule("NoSpaceBeforeCloseBraceInJsxExpression", &any_token, K::CloseBraceToken, vec![is_option_disabled_or_undefined(|o| o.insert_space_after_opening_and_before_closing_jsx_expression_braces), p(is_non_jsx_same_line_token_context), p(is_jsx_expression_context)], A::DELETE_SPACE),

        // Insert space after semicolon in for statement
        rule("SpaceAfterSemicolonInFor", K::SemicolonToken, &any_token, vec![is_option_enabled(|o| o.insert_space_after_semicolon_in_for_statements), p(is_non_jsx_same_line_token_context), p(is_for_context)], A::INSERT_SPACE),
        rule("NoSpaceAfterSemicolonInFor", K::SemicolonToken, &any_token, vec![is_option_disabled_or_undefined(|o| o.insert_space_after_semicolon_in_for_statements), p(is_non_jsx_same_line_token_context), p(is_for_context)], A::DELETE_SPACE),

        // Insert space before and after binary operators
        rule("SpaceBeforeBinaryOperator", &any_token, &binary_operators, vec![is_option_enabled(|o| o.insert_space_before_and_after_binary_operators), p(is_non_jsx_same_line_token_context), p(is_binary_op_context)], A::INSERT_SPACE),
        rule("SpaceAfterBinaryOperator", &binary_operators, &any_token, vec![is_option_enabled(|o| o.insert_space_before_and_after_binary_operators), p(is_non_jsx_same_line_token_context), p(is_binary_op_context)], A::INSERT_SPACE),
        rule("NoSpaceBeforeBinaryOperator", &any_token, &binary_operators, vec![is_option_disabled_or_undefined(|o| o.insert_space_before_and_after_binary_operators), p(is_non_jsx_same_line_token_context), p(is_binary_op_context)], A::DELETE_SPACE),
        rule("NoSpaceAfterBinaryOperator", &binary_operators, &any_token, vec![is_option_disabled_or_undefined(|o| o.insert_space_before_and_after_binary_operators), p(is_non_jsx_same_line_token_context), p(is_binary_op_context)], A::DELETE_SPACE),

        rule("SpaceBeforeOpenParenInFuncDecl", &any_token, K::OpenParenToken, vec![is_option_enabled(|o| o.insert_space_before_function_parenthesis), p(is_non_jsx_same_line_token_context), p(is_function_decl_context)], A::INSERT_SPACE),
        rule("NoSpaceBeforeOpenParenInFuncDecl", &any_token, K::OpenParenToken, vec![is_option_disabled_or_undefined(|o| o.insert_space_before_function_parenthesis), p(is_non_jsx_same_line_token_context), p(is_function_decl_context)], A::DELETE_SPACE),

        // Open Brace braces after control block
        rule_with_flags("NewLineBeforeOpenBraceInControl", &control_open_brace_left_token_range, K::OpenBraceToken, vec![is_option_enabled(|o| o.place_open_brace_on_new_line_for_control_blocks), p(is_control_decl_context), p(is_before_multiline_block_context)], A::INSERT_NEW_LINE, RuleFlags::CanDeleteNewLines),

        // Open Brace braces after function
        // TypeScript: Function can have return types, which can be made of tons of different token kinds
        rule_with_flags("NewLineBeforeOpenBraceInFunction", &function_open_brace_left_token_range, K::OpenBraceToken, vec![is_option_enabled(|o| o.place_open_brace_on_new_line_for_functions), p(is_function_decl_context), p(is_before_multiline_block_context)], A::INSERT_NEW_LINE, RuleFlags::CanDeleteNewLines),
        // Open Brace braces after TypeScript module/class/interface
        rule_with_flags("NewLineBeforeOpenBraceInTypeScriptDeclWithBlock", &type_script_open_brace_left_token_range, K::OpenBraceToken, vec![is_option_enabled(|o| o.place_open_brace_on_new_line_for_functions), p(is_type_script_decl_with_block_context), p(is_before_multiline_block_context)], A::INSERT_NEW_LINE, RuleFlags::CanDeleteNewLines),

        rule("SpaceAfterTypeAssertion", K::GreaterThanToken, &any_token, vec![is_option_enabled(|o| o.insert_space_after_type_assertion), p(is_non_jsx_same_line_token_context), p(is_type_assertion_context)], A::INSERT_SPACE),
        rule("NoSpaceAfterTypeAssertion", K::GreaterThanToken, &any_token, vec![is_option_disabled_or_undefined(|o| o.insert_space_after_type_assertion), p(is_non_jsx_same_line_token_context), p(is_type_assertion_context)], A::DELETE_SPACE),

        rule("SpaceBeforeTypeAnnotation", &any_token, [K::QuestionToken, K::ColonToken], vec![is_option_enabled(|o| o.insert_space_before_type_annotation), p(is_non_jsx_same_line_token_context), p(is_type_annotation_context)], A::INSERT_SPACE),
        rule("NoSpaceBeforeTypeAnnotation", &any_token, [K::QuestionToken, K::ColonToken], vec![is_option_disabled_or_undefined(|o| o.insert_space_before_type_annotation), p(is_non_jsx_same_line_token_context), p(is_type_annotation_context)], A::DELETE_SPACE),

        rule("NoOptionalSemicolon", K::SemicolonToken, &any_token_including_eof, vec![option_semicolons_equal(SemicolonPreference::REMOVE), p(is_semicolon_deletion_context)], A::DELETE_TOKEN),
        rule("OptionalSemicolon", &any_token, &any_token_including_eof, vec![option_semicolons_equal(SemicolonPreference::INSERT), p(is_semicolon_insertion_context)], A::INSERT_TRAILING_SEMICOLON),
    ];

    // These rules are lower in priority than user-configurable. Rules earlier in this list have priority over rules later in the list.
    let low_priority_common_rules = vec![
        // Space after keyword but not before ; or : or ?
        rule("NoSpaceBeforeSemicolon", &any_token, K::SemicolonToken, vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),

        rule_with_flags("SpaceBeforeOpenBraceInControl", &control_open_brace_left_token_range, K::OpenBraceToken, vec![is_option_disabled_or_undefined_or_tokens_on_same_line(|o| o.place_open_brace_on_new_line_for_control_blocks), p(is_control_decl_context), p(is_not_format_on_enter), p(is_same_line_token_or_before_block_context)], A::INSERT_SPACE, RuleFlags::CanDeleteNewLines),
        rule_with_flags("SpaceBeforeOpenBraceInFunction", &function_open_brace_left_token_range, K::OpenBraceToken, vec![is_option_disabled_or_undefined_or_tokens_on_same_line(|o| o.place_open_brace_on_new_line_for_functions), p(is_function_decl_context), p(is_before_block_context), p(is_not_format_on_enter), p(is_same_line_token_or_before_block_context)], A::INSERT_SPACE, RuleFlags::CanDeleteNewLines),
        rule_with_flags("SpaceBeforeOpenBraceInTypeScriptDeclWithBlock", &type_script_open_brace_left_token_range, K::OpenBraceToken, vec![is_option_disabled_or_undefined_or_tokens_on_same_line(|o| o.place_open_brace_on_new_line_for_functions), p(is_type_script_decl_with_block_context), p(is_not_format_on_enter), p(is_same_line_token_or_before_block_context)], A::INSERT_SPACE, RuleFlags::CanDeleteNewLines),

        rule("NoSpaceBeforeComma", &any_token, K::CommaToken, vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),

        // No space before and after indexer `x[]`
        rule("NoSpaceBeforeOpenBracket", any_token_except(&[K::AsyncKeyword, K::CaseKeyword]), K::OpenBracketToken, vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),
        rule("NoSpaceAfterCloseBracket", K::CloseBracketToken, &any_token, vec![p(is_non_jsx_same_line_token_context), p(is_not_before_block_in_function_declaration_context)], A::DELETE_SPACE),
        rule("SpaceAfterSemicolon", K::SemicolonToken, &any_token, vec![p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),

        // Remove extra space between for and await
        rule("SpaceBetweenForAndAwaitKeyword", K::ForKeyword, K::AwaitKeyword, vec![p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),

        // Remove extra spaces between ... and type name in tuple spread
        rule("SpaceBetweenDotDotDotAndTypeName", K::DotDotDotToken, &type_names, vec![p(is_non_jsx_same_line_token_context)], A::DELETE_SPACE),

        // Add a space between statements. All keywords except (do,else,case) has open/close parens after them.
        // So, we have a rule to add a space for [),Any], [do,Any], [else,Any], and [case,Any]
        rule(
            "SpaceBetweenStatements",
            [K::CloseParenToken, K::DoKeyword, K::ElseKeyword, K::CaseKeyword],
            &any_token,
            vec![p(is_non_jsx_same_line_token_context), p(is_non_jsx_element_or_fragment_context), p(is_not_for_context)],
            A::INSERT_SPACE,
        ),
        // This low-pri rule takes care of "try {", "catch {" and "finally {" in case the rule SpaceBeforeOpenBraceInControl didn't execute on FormatOnEnter.
        rule("SpaceAfterTryCatchFinally", [K::TryKeyword, K::CatchKeyword, K::FinallyKeyword], K::OpenBraceToken, vec![p(is_non_jsx_same_line_token_context)], A::INSERT_SPACE),
    ];

    let mut result = high_priority_common_rules;
    result.extend(user_configurable_rules);
    result.extend(low_priority_common_rules);
    result
}

/// tsgo `tokenRangeFromEx`.
fn token_range_from_ex(prefix: &[K], tokens: &[K]) -> TokenRange {
    TokenRange {
        tokens: prefix.iter().chain(tokens).copied().collect(),
        is_specific: true,
    }
}

/// tsgo `tokenRangeFromRange`.
fn token_range_from_range(start: K, end: K) -> TokenRange {
    TokenRange {
        tokens: (start.value()..=end.value())
            .filter_map(K::from_u16)
            .collect(),
        is_specific: true,
    }
}
