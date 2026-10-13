//! tsgo's keyword completions (`getKeywordCompletions` and the keyword
//! predicates of ls/completions.go and ls/utilities.go).

use tsc_lsproto::{CompletionItem, CompletionItemKind};
use tsc_syntax::SyntaxKind;

use super::{sort_text, KeywordCompletionFilters};

/// tsgo `typeKeywords` (ls/utilities.go).
pub(crate) fn is_type_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::AnyKeyword
            | SyntaxKind::AssertsKeyword
            | SyntaxKind::BigIntKeyword
            | SyntaxKind::BooleanKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::InferKeyword
            | SyntaxKind::KeyOfKeyword
            | SyntaxKind::NeverKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::NumberKeyword
            | SyntaxKind::ObjectKeyword
            | SyntaxKind::ReadonlyKeyword
            | SyntaxKind::StringKeyword
            | SyntaxKind::SymbolKeyword
            | SyntaxKind::TypeOfKeyword
            | SyntaxKind::TrueKeyword
            | SyntaxKind::VoidKeyword
            | SyntaxKind::UndefinedKeyword
            | SyntaxKind::UniqueKeyword
            | SyntaxKind::UnknownKeyword
    )
}

/// tsgo `ast.IsContextualKeyword`.
pub(crate) fn is_contextual_keyword(kind: SyntaxKind) -> bool {
    (SyntaxKind::FirstContextualKeyword.value()..=SyntaxKind::LastContextualKeyword.value())
        .contains(&kind.value())
}

/// tsgo `ast.IsKeyword`.
pub(crate) fn is_keyword(kind: SyntaxKind) -> bool {
    (SyntaxKind::FirstKeyword.value()..=SyntaxKind::LastKeyword.value()).contains(&kind.value())
}

/// tsgo `ast.IsNonContextualKeyword`.
pub(crate) fn is_non_contextual_keyword(kind: SyntaxKind) -> bool {
    is_keyword(kind) && !is_contextual_keyword(kind)
}

/// tsgo `ast.IsParameterPropertyModifier`: an accessibility modifier,
/// `readonly` or `override` (`ModifierFlagsParameterPropertyModifier`).
pub(crate) fn is_parameter_property_modifier(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PublicKeyword
            | SyntaxKind::PrivateKeyword
            | SyntaxKind::ProtectedKeyword
            | SyntaxKind::ReadonlyKeyword
            | SyntaxKind::OverrideKeyword
    )
}

/// tsgo `ast.IsClassMemberModifier`.
fn is_class_member_modifier(kind: SyntaxKind) -> bool {
    is_parameter_property_modifier(kind)
        || matches!(
            kind,
            SyntaxKind::StaticKeyword | SyntaxKind::OverrideKeyword | SyntaxKind::AccessorKeyword
        )
}

/// tsgo `isTypeScriptOnlyKeyword`.
fn is_type_script_only_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::AbstractKeyword
            | SyntaxKind::AnyKeyword
            | SyntaxKind::BigIntKeyword
            | SyntaxKind::BooleanKeyword
            | SyntaxKind::DeclareKeyword
            | SyntaxKind::EnumKeyword
            | SyntaxKind::GlobalKeyword
            | SyntaxKind::ImplementsKeyword
            | SyntaxKind::InferKeyword
            | SyntaxKind::InterfaceKeyword
            | SyntaxKind::IsKeyword
            | SyntaxKind::KeyOfKeyword
            | SyntaxKind::ModuleKeyword
            | SyntaxKind::NamespaceKeyword
            | SyntaxKind::NeverKeyword
            | SyntaxKind::NumberKeyword
            | SyntaxKind::ObjectKeyword
            | SyntaxKind::OverrideKeyword
            | SyntaxKind::PrivateKeyword
            | SyntaxKind::ProtectedKeyword
            | SyntaxKind::PublicKeyword
            | SyntaxKind::ReadonlyKeyword
            | SyntaxKind::StringKeyword
            | SyntaxKind::SymbolKeyword
            | SyntaxKind::TypeKeyword
            | SyntaxKind::UniqueKeyword
            | SyntaxKind::UnknownKeyword
    )
}

/// tsgo `isFunctionLikeBodyKeyword`.
fn is_function_like_body_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::AsyncKeyword
            | SyntaxKind::AwaitKeyword
            | SyntaxKind::UsingKeyword
            | SyntaxKind::AsKeyword
            | SyntaxKind::SatisfiesKeyword
            | SyntaxKind::TypeKeyword
    ) || !is_contextual_keyword(kind) && !is_class_member_completion_keyword(kind)
}

/// tsgo `isClassMemberCompletionKeyword`.
pub(crate) fn is_class_member_completion_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::AbstractKeyword
            | SyntaxKind::AccessorKeyword
            | SyntaxKind::ConstructorKeyword
            | SyntaxKind::GetKeyword
            | SyntaxKind::SetKeyword
            | SyntaxKind::AsyncKeyword
            | SyntaxKind::DeclareKeyword
            | SyntaxKind::OverrideKeyword
    ) || is_class_member_modifier(kind)
}

/// tsgo `isContextualKeywordInAutoImportableExpressionSpace`.
pub(crate) fn is_contextual_keyword_in_auto_importable_expression_space(keyword: &str) -> bool {
    matches!(
        keyword,
        "abstract"
            | "async"
            | "await"
            | "declare"
            | "module"
            | "namespace"
            | "type"
            | "satisfies"
            | "as"
    )
}

/// A keyword's completion item.
pub(crate) fn keyword_item(kind: SyntaxKind) -> CompletionItem {
    CompletionItem {
        label: tsc_syntax::tokens::token_to_string(kind)
            .expect("a keyword has its text")
            .to_owned(),
        kind: Some(CompletionItemKind::KEYWORD),
        sort_text: Some(sort_text::GLOBALS_OR_KEYWORDS.to_owned()),
        ..CompletionItem::default()
    }
}

/// tsgo `getKeywordCompletions`: the keywords the filter allows, without
/// TypeScript-only ones when asked.
pub(crate) fn keyword_completions(
    filter: KeywordCompletionFilters,
    filter_out_ts_only_keywords: bool,
) -> Vec<CompletionItem> {
    (SyntaxKind::FirstKeyword.value()..=SyntaxKind::LastKeyword.value())
        .filter_map(SyntaxKind::from_u16)
        .filter(|&kind| allows(filter, kind))
        .filter(|&kind| !filter_out_ts_only_keywords || !is_type_script_only_keyword(kind))
        .map(keyword_item)
        .collect()
}

/// tsgo `getTypescriptKeywordCompletions`'s filter.
fn allows(filter: KeywordCompletionFilters, kind: SyntaxKind) -> bool {
    match filter {
        KeywordCompletionFilters::None => false,
        KeywordCompletionFilters::All => {
            is_function_like_body_keyword(kind)
                || matches!(
                    kind,
                    SyntaxKind::DeclareKeyword
                        | SyntaxKind::ModuleKeyword
                        | SyntaxKind::TypeKeyword
                        | SyntaxKind::NamespaceKeyword
                        | SyntaxKind::AbstractKeyword
                )
                || is_type_keyword(kind) && kind != SyntaxKind::UndefinedKeyword
        }
        KeywordCompletionFilters::FunctionLikeBodyKeywords => is_function_like_body_keyword(kind),
        KeywordCompletionFilters::ConstructorParameterKeywords => {
            is_parameter_property_modifier(kind)
        }
        KeywordCompletionFilters::TypeAssertionKeywords => {
            is_type_keyword(kind) || kind == SyntaxKind::ConstKeyword
        }
        KeywordCompletionFilters::TypeKeywords => is_type_keyword(kind),
        KeywordCompletionFilters::TypeKeyword => kind == SyntaxKind::TypeKeyword,
    }
}
