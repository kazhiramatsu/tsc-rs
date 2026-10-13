//! tsgo's completion item construction: `createLSPCompletionItem`, the
//! filter text ported from the VS Code TypeScript extension, and
//! `setItemDefaults`.

use tsc_lsproto::{
    AutoImportFix, CompletionItem, CompletionItemData, CompletionItemDefaults, CompletionItemKind,
    CompletionItemLabelDetails, CompletionItemTag, EditRangeWithInsertReplace, InsertReplaceEdit,
    InsertTextFormat, Position, Range, RangeOrEditRangeWithInsertReplace, TextEdit,
    TextEditOrInsertReplaceEdit,
};

use crate::lsutil::{ScriptElementKind, ScriptElementKindModifier};

use super::ClientSupport;

/// What tsgo's `createLSPCompletionItem` takes besides the file and
/// position.
#[derive(Default)]
pub(crate) struct ItemSpec {
    pub(crate) name: String,
    pub(crate) insert_text: String,
    pub(crate) filter_text: String,
    pub(crate) sort_text: String,
    pub(crate) element_kind: Option<ScriptElementKind>,
    pub(crate) kind_modifiers: ScriptElementKindModifier,
    pub(crate) replacement_span: Option<Range>,
    pub(crate) commit_characters: Option<Vec<String>>,
    pub(crate) label_details: Option<CompletionItemLabelDetails>,
    pub(crate) is_member_completion: bool,
    pub(crate) is_snippet: bool,
    pub(crate) preselect: bool,
    pub(crate) source: String,
    pub(crate) auto_import_fix: Option<AutoImportFix>,
    pub(crate) additional_text_edits: Option<Vec<TextEdit>>,
    pub(crate) detail: Option<String>,
}

/// tsgo `createLSPCompletionItem`.
pub(crate) fn create_lsp_completion_item(
    text: &str,
    file_name: &str,
    position: u32,
    spec: ItemSpec,
) -> CompletionItem {
    let ItemSpec {
        mut name,
        mut insert_text,
        mut filter_text,
        sort_text,
        element_kind,
        kind_modifiers,
        replacement_span,
        commit_characters,
        label_details,
        is_member_completion,
        is_snippet,
        preselect,
        source,
        auto_import_fix,
        additional_text_edits,
        detail,
    } = spec;
    let kind = completions_symbol_kind(element_kind.unwrap_or(ScriptElementKind::Unknown));
    let data = CompletionItemData {
        file_name: file_name.to_owned(),
        position: position as i32,
        supplemental_file_index: None,
        source,
        name: name.clone(),
        auto_import: auto_import_fix,
        ..CompletionItemData::default()
    };
    let text_edit = replacement_span.map(|range| {
        TextEditOrInsertReplaceEdit::TextEdit(TextEdit {
            new_text: if insert_text.is_empty() {
                name.clone()
            } else {
                insert_text.clone()
            },
            range,
        })
    });

    // The filter text, ported from the VS Code TypeScript extension.
    let (word_size, word_start) = word_length_and_start(text, position as usize);
    let dot_accessor = dot_accessor(text, position as usize - word_size);
    if filter_text.is_empty() {
        filter_text = filter_text_of(&insert_text, &name, word_start, dot_accessor);
    }

    // The VS Code extension's `MyCompletionItem` constructor: an optional
    // member is labeled with `?`.
    if is_member_completion && kind_modifiers.contains(ScriptElementKindModifier::OPTIONAL) {
        if insert_text.is_empty() {
            insert_text = name.clone();
        }
        if filter_text.is_empty() || is_snippet {
            filter_text = name.clone();
        }
        name.push('?');
    }
    let tags = kind_modifiers
        .contains(ScriptElementKindModifier::DEPRECATED)
        .then(|| vec![CompletionItemTag::DEPRECATED]);

    CompletionItem {
        label: name,
        label_details,
        kind: Some(kind),
        tags,
        detail,
        preselect: preselect.then_some(true),
        sort_text: Some(sort_text),
        filter_text: (!filter_text.is_empty()).then_some(filter_text),
        insert_text: (!insert_text.is_empty()).then_some(insert_text),
        insert_text_format: is_snippet.then_some(InsertTextFormat::SNIPPET),
        text_edit,
        commit_characters,
        additional_text_edits,
        data: Some(data),
        ..CompletionItem::default()
    }
}

/// tsgo `getCompletionsSymbolKind`.
pub(crate) fn completions_symbol_kind(kind: ScriptElementKind) -> CompletionItemKind {
    use ScriptElementKind as Kind;
    match kind {
        Kind::PrimitiveType | Kind::Keyword => CompletionItemKind::KEYWORD,
        Kind::ConstElement
        | Kind::LetElement
        | Kind::VariableElement
        | Kind::LocalVariableElement
        | Kind::Alias
        | Kind::ParameterElement => CompletionItemKind::VARIABLE,
        Kind::MemberVariableElement
        | Kind::MemberGetAccessorElement
        | Kind::MemberSetAccessorElement => CompletionItemKind::FIELD,
        Kind::FunctionElement | Kind::LocalFunctionElement => CompletionItemKind::FUNCTION,
        Kind::MemberFunctionElement
        | Kind::ConstructSignatureElement
        | Kind::CallSignatureElement
        | Kind::IndexSignatureElement => CompletionItemKind::METHOD,
        Kind::EnumElement => CompletionItemKind::ENUM,
        Kind::EnumMemberElement => CompletionItemKind::ENUM_MEMBER,
        Kind::ModuleElement | Kind::ExternalModuleName => CompletionItemKind::MODULE,
        Kind::ClassElement | Kind::TypeElement => CompletionItemKind::CLASS,
        Kind::InterfaceElement => CompletionItemKind::INTERFACE,
        Kind::Warning => CompletionItemKind::TEXT,
        Kind::ScriptElement => CompletionItemKind::FILE,
        Kind::Directory => CompletionItemKind::FOLDER,
        Kind::String => CompletionItemKind::CONSTANT,
        _ => CompletionItemKind::PROPERTY,
    }
}

/// VS Code's word separators.
fn is_word_separator(c: char) -> bool {
    matches!(
        c,
        '`' | '~'
            | '!'
            | '@'
            | '%'
            | '^'
            | '&'
            | '*'
            | '('
            | ')'
            | '-'
            | '='
            | '+'
            | '['
            | '{'
            | ']'
            | '}'
            | '\\'
            | '|'
            | ';'
            | ':'
            | '\''
            | '"'
            | ','
            | '.'
            | '<'
            | '>'
            | '/'
            | '?'
    )
}

/// tsgo `getWordLengthAndStart`: the byte length and first character of
/// the word that ends at `position` (an `@` before it left out).
fn word_length_and_start(text: &str, position: usize) -> (usize, Option<char>) {
    let before = &text[..position];
    let mut total = 0;
    let mut first = None;
    for c in before.chars().rev() {
        if is_word_separator(c) || c.is_whitespace() {
            break;
        }
        total += c.len_utf8();
        first = Some(c);
    }
    if first == Some('@') {
        total -= 1;
        first = before[before.len() - total..].chars().next();
    }
    (total, first)
}

/// tsgo `getDotAccessor`: the `.` or `?.` before `position`.
fn dot_accessor(text: &str, position: usize) -> &str {
    let before = &text[..position];
    if before.ends_with("?.") {
        &before[position - 2..]
    } else if before.ends_with('.') {
        &before[position - 1..]
    } else {
        ""
    }
}

/// tsgo `trimElementAccess`: `["ab c"]` is `ab c`.
fn trim_element_access(text: &str) -> &str {
    let mut text = text.strip_prefix('[').unwrap_or(text);
    text = text.strip_suffix(']').unwrap_or(text);
    if text.starts_with('\'') && text.ends_with('\'') {
        text = text.strip_suffix('\'').unwrap_or(text);
        text = text.strip_prefix('\'').unwrap_or(text);
    }
    if text.starts_with('"') && text.ends_with('"') {
        text = text.strip_suffix('"').unwrap_or(text);
        text = text.strip_prefix('"').unwrap_or(text);
    }
    text
}

/// tsgo `getFilterText` (from the VS Code TypeScript extension).
fn filter_text_of(
    insert_text: &str,
    label: &str,
    word_start: Option<char>,
    dot_accessor: &str,
) -> String {
    // A private field, labeled `#bar`.
    if let Some(after) = label.strip_prefix('#') {
        if !insert_text.is_empty() {
            if let Some(after) = insert_text.strip_prefix("this.#") {
                return if word_start == Some('#') {
                    String::new()
                } else {
                    after.to_owned()
                };
            }
        } else {
            return if word_start == Some('#') {
                String::new()
            } else {
                after.to_owned()
            };
        }
    }
    // `this.` completions keep no filter text, so they are not
    // deprioritized (microsoft/vscode#74164).
    if insert_text.starts_with("this.") {
        return String::new();
    }
    // `xyz.ab|` inserting `['ab c']` filters on `.ab c`.
    if insert_text.starts_with('[') {
        return format!("{dot_accessor}{}", trim_element_access(insert_text));
    }
    if let Some(rest) = insert_text.strip_prefix("?.") {
        if rest.starts_with('[') {
            return format!("{dot_accessor}{}", trim_element_access(rest));
        }
        return format!("{dot_accessor}{rest}");
    }
    insert_text.to_owned()
}

/// tsgo `setItemDefaults`: the list's defaults where the client takes them,
/// or the items' own commit characters and edits where it does not.
pub(crate) fn set_item_defaults(
    support: ClientSupport<'_>,
    position: Position,
    items: &mut [CompletionItem],
    default_commit_characters: Option<Vec<String>>,
    optional_replacement_span: Option<Range>,
) -> Option<CompletionItemDefaults> {
    let mut item_defaults: Option<CompletionItemDefaults> = None;
    if let Some(default_commit_characters) = default_commit_characters {
        let supports_item_commit_characters = support.item_commit_characters();
        if support.default_commit_characters() && supports_item_commit_characters {
            item_defaults = Some(CompletionItemDefaults {
                commit_characters: Some(default_commit_characters),
                ..CompletionItemDefaults::default()
            });
        } else if supports_item_commit_characters {
            for item in items.iter_mut() {
                if item.commit_characters.is_none() {
                    item.commit_characters = Some(default_commit_characters.clone());
                }
            }
        }
    }
    if let Some(replacement) = optional_replacement_span {
        // Ported from the VS Code TypeScript extension.
        let insert = Range {
            start: replacement.start.clone(),
            end: position,
        };
        if support.default_edit_range() {
            item_defaults
                .get_or_insert_with(CompletionItemDefaults::default)
                .edit_range = Some(
                RangeOrEditRangeWithInsertReplace::EditRangeWithInsertReplace(
                    EditRangeWithInsertReplace {
                        insert: insert.clone(),
                        replace: replacement.clone(),
                    },
                ),
            );
            for item in items.iter_mut() {
                // With an edit range, the client ignores `insertText`.
                if item.text_edit.is_none() {
                    if let Some(insert_text) = item.insert_text.take() {
                        item.text_edit = Some(TextEditOrInsertReplaceEdit::InsertReplaceEdit(
                            InsertReplaceEdit {
                                new_text: insert_text,
                                insert: insert.clone(),
                                replace: replacement.clone(),
                            },
                        ));
                    }
                }
            }
        } else if support.item_insert_replace() {
            for item in items.iter_mut() {
                if item.text_edit.is_none() {
                    item.text_edit = Some(TextEditOrInsertReplaceEdit::InsertReplaceEdit(
                        InsertReplaceEdit {
                            new_text: item
                                .insert_text
                                .clone()
                                .unwrap_or_else(|| item.label.clone()),
                            insert: insert.clone(),
                            replace: replacement.clone(),
                        },
                    ));
                }
            }
        }
    }
    item_defaults
}
