//! tsgo `ls/completions.go`: the completion list at a position and its
//! items.

mod context;
mod contextual;
mod data;
mod entries;
mod items;
mod jsdoc;
pub(crate) mod keywords;
mod paths;
mod resolve;
mod searches;
mod strings;

use tsc_checker::state::CheckResult;
use tsc_lsproto::{
    CompletionItem, CompletionItemData, CompletionItemDefaults, CompletionItemsOrListOrNull,
    CompletionParams, ResolvedClientCapabilities,
};

use tsc_syntax::SyntaxKind;

use crate::language_service::{LanguageService, LsError};

use context::Completions;
pub(crate) use context::{is_type_element, is_type_node_kind};
use data::CompletionData;

/// tsgo `SortText`s.
pub(crate) mod sort_text {
    pub(crate) const LOCAL_DECLARATION_PRIORITY: &str = "10";
    pub(crate) const LOCATION_PRIORITY: &str = "11";
    pub(crate) const OPTIONAL_MEMBER: &str = "12";
    pub(crate) const MEMBER_DECLARED_BY_SPREAD_ASSIGNMENT: &str = "13";
    pub(crate) const SUGGESTED_CLASS_MEMBERS: &str = "14";
    pub(crate) const GLOBALS_OR_KEYWORDS: &str = "15";
    pub(crate) const AUTO_IMPORT_SUGGESTIONS: &str = "16";
    pub(crate) const JAVASCRIPT_IDENTIFIERS: &str = "18";

    /// tsgo `DeprecateSortText`.
    pub(crate) fn deprecated(original: &str) -> String {
        format!("z{original}")
    }

    /// tsgo `ObjectLiteralPropertySortText`.
    pub(crate) fn object_literal_property(preset: &str, display_name: &str) -> String {
        format!("{preset}\0{display_name}\0")
    }
}

/// tsgo `KeywordCompletionFilters`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeywordCompletionFilters {
    /// No keywords.
    None,
    /// Every possible keyword.
    All,
    /// Keywords inside a class body.
    ClassElementKeywords,
    /// Keywords inside an interface body.
    InterfaceElementKeywords,
    /// Keywords at a constructor parameter.
    ConstructorParameterKeywords,
    /// Keywords at a function-like body.
    FunctionLikeBodyKeywords,
    TypeAssertionKeywords,
    TypeKeywords,
    /// Literally just `type`.
    TypeKeyword,
}

/// tsgo `CompletionKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompletionKind {
    None,
    ObjectPropertyDeclaration,
    Global,
    PropertyAccess,
    MemberLike,
    String,
}

/// tsgo `isMemberCompletionKind`.
pub(crate) fn is_member_completion_kind(kind: CompletionKind) -> bool {
    matches!(
        kind,
        CompletionKind::ObjectPropertyDeclaration
            | CompletionKind::MemberLike
            | CompletionKind::PropertyAccess
    )
}

/// tsgo `allCommitCharacters`: valid where no new identifier is written.
pub(crate) const ALL_COMMIT_CHARACTERS: [&str; 3] = [".", ",", ";"];

/// tsgo `noCommaCommitCharacters`: valid in an expression that could be
/// inside a parameter list.
pub(crate) const NO_COMMA_COMMIT_CHARACTERS: [&str; 2] = [".", ";"];

/// tsgo `getDefaultCommitCharacters`.
pub(crate) fn default_commit_characters(is_new_identifier_location: bool) -> Vec<String> {
    if is_new_identifier_location {
        Vec::new()
    } else {
        ALL_COMMIT_CHARACTERS
            .iter()
            .map(|&c| c.to_owned())
            .collect()
    }
}

/// tsgo `CompletionList` (before `toLSP`).
#[derive(Debug, Default)]
pub(crate) struct CompletionList {
    pub(crate) is_incomplete: bool,
    pub(crate) item_defaults: Option<CompletionItemDefaults>,
    pub(crate) items: Vec<CompletionItem>,
}

/// The client capabilities completion items depend on (tsgo's
/// `clientSupports…`).
#[derive(Clone, Copy)]
pub(crate) struct ClientSupport<'c>(pub(crate) &'c ResolvedClientCapabilities);

impl ClientSupport<'_> {
    pub(crate) fn item_snippet(self) -> bool {
        self.0
            .text_document
            .completion
            .completion_item
            .snippet_support
    }

    pub(crate) fn item_commit_characters(self) -> bool {
        self.0
            .text_document
            .completion
            .completion_item
            .commit_characters_support
    }

    pub(crate) fn item_insert_replace(self) -> bool {
        self.0
            .text_document
            .completion
            .completion_item
            .insert_replace_support
    }

    pub(crate) fn default_commit_characters(self) -> bool {
        self.0
            .text_document
            .completion
            .completion_list
            .item_defaults
            .iter()
            .any(|default| default == "commitCharacters")
    }

    pub(crate) fn default_edit_range(self) -> bool {
        self.0
            .text_document
            .completion
            .completion_list
            .item_defaults
            .iter()
            .any(|default| default == "editRange")
    }
}

impl LanguageService<'_> {
    /// tsgo `ProvideCompletion`.
    pub fn provide_completion(
        &mut self,
        params: &CompletionParams,
    ) -> Result<CompletionItemsOrListOrNull, LsError> {
        let file = self.file(&params.text_document.uri);
        let trigger_character = params
            .context
            .as_ref()
            .and_then(|context| context.trigger_character.clone());
        let list = self.with_request(file, &params.position, false, |request, position| {
            let mut completions = Completions::new(request, file, position);
            let list = completions.get_completions_at_position(trigger_character.as_deref())?;
            if completions.needs_auto_imports {
                return Ok(Err(LsError::needs_auto_imports()));
            }
            Ok(Ok(list.map(|list| completions.ensure_item_data(list))))
        })?;
        Ok(match list.transpose()?.flatten() {
            Some(list) => CompletionItemsOrListOrNull::List(list),
            None => CompletionItemsOrListOrNull::Null,
        })
    }
}

impl Completions<'_, '_, '_> {
    /// tsgo `getCompletionsAtPosition`.
    fn get_completions_at_position(
        &mut self,
        trigger_character: Option<&str>,
    ) -> CheckResult<Option<CompletionList>> {
        let position = self.position;
        let (_, previous_token) = self.relevant_tokens(position);
        if let Some(trigger) = trigger_character {
            if !self.is_in_string(position, previous_token)
                && !self.is_valid_trigger(trigger, previous_token, position)
            {
                return Ok(None);
            }
            if trigger == " " {
                // `isValidTrigger` has made sure of `import |`.
                if self
                    .request
                    .preferences
                    .completion_preferences
                    .include_completions_for_import_statements
                    == Some(true)
                {
                    return Ok(Some(CompletionList {
                        is_incomplete: true,
                        ..CompletionList::default()
                    }));
                }
                return Ok(None);
            }
        }
        if let Some(list) = self.jsdoc_snippet_completion() {
            return Ok(Some(list));
        }
        if let Some(list) = self.string_literal_completions(previous_token)? {
            return Ok(Some(list));
        }
        if let Some(previous) = previous_token {
            let parent = self.parent(previous);
            if matches!(
                self.kind(previous),
                SyntaxKind::BreakKeyword | SyntaxKind::ContinueKeyword | SyntaxKind::Identifier
            ) && parent.is_some_and(|parent| {
                matches!(
                    self.node_kind(parent),
                    SyntaxKind::BreakStatement | SyntaxKind::ContinueStatement
                )
            }) {
                let optional_replacement_span = self.optional_replacement_span(Some(previous));
                return Ok(self.label_completions_at_position(
                    parent.expect("a break or continue statement"),
                    optional_replacement_span,
                ));
            }
        }
        let Some(data) = self.get_completion_data()? else {
            return Ok(None);
        };
        match data {
            CompletionData::Data(mut data) => {
                let optional_replacement_span = self.optional_replacement_span(Some(data.location));
                self.completion_info_from_data(&mut data, optional_replacement_span)
            }
            CompletionData::Keyword {
                keyword_completions,
                is_new_identifier_location,
            } => {
                let optional_replacement_span = self.optional_replacement_span(previous_token);
                Ok(Some(self.specific_keyword_completion_info(
                    keyword_completions,
                    is_new_identifier_location,
                    optional_replacement_span,
                )))
            }
            // At a JSDoc tag name, only tag names (and parameter tags).
            CompletionData::JSDocTagName => Ok(Some(self.jsdoc_tag_completions(true)?)),
            CompletionData::JSDocTag => Ok(Some(self.jsdoc_tag_completions(false)?)),
            CompletionData::JSDocParameterName(tag) => {
                Ok(Some(self.jsdoc_parameter_name_completions(tag)))
            }
        }
    }

    /// tsgo `getLabelCompletionsAtPosition`: the labels of the statements
    /// around a `break` or `continue`, in its function.
    fn label_completions_at_position(
        &self,
        statement: tsc_syntax::NodeId,
        optional_replacement_span: Option<tsc_lsproto::Range>,
    ) -> Option<CompletionList> {
        let mut names = std::collections::HashSet::new();
        let mut items = Vec::new();
        let file_name = self.file_name();
        let mut current = Some(statement);
        while let Some(node) = current {
            if crate::utilities::is_function_like_kind(self.node_kind(node)) {
                break;
            }
            if let tsc_syntax::NodeData::LabeledStatement(data) = self.data(node) {
                let name = data
                    .label
                    .and_then(|label| self.identifier_text(crate::utilities::Touching::Node(label)))
                    .unwrap_or_default()
                    .to_owned();
                if names.insert(name.clone()) {
                    items.push(items::create_lsp_completion_item(
                        self.text(),
                        &file_name,
                        self.position,
                        items::ItemSpec {
                            name,
                            sort_text: sort_text::LOCATION_PRIORITY.to_owned(),
                            element_kind: Some(crate::lsutil::ScriptElementKind::Label),
                            ..items::ItemSpec::default()
                        },
                    ));
                }
            }
            current = self.parent_of(node);
        }
        if items.is_empty() {
            return None;
        }
        let position = self
            .request
            .lsp_range(self.source, self.position as usize..self.position as usize)
            .start;
        let item_defaults = items::set_item_defaults(
            ClientSupport(self.request.capabilities),
            position,
            &mut items,
            Some(default_commit_characters(false)),
            optional_replacement_span,
        );
        Some(CompletionList {
            is_incomplete: false,
            item_defaults,
            items,
        })
    }

    /// tsgo `specificKeywordCompletionInfo`.
    fn specific_keyword_completion_info(
        &self,
        mut items: Vec<CompletionItem>,
        is_new_identifier_location: bool,
        optional_replacement_span: Option<tsc_lsproto::Range>,
    ) -> CompletionList {
        let position = self
            .request
            .lsp_range(self.source, self.position as usize..self.position as usize)
            .start;
        let item_defaults = items::set_item_defaults(
            ClientSupport(self.request.capabilities),
            position,
            &mut items,
            Some(default_commit_characters(is_new_identifier_location)),
            optional_replacement_span,
        );
        CompletionList {
            is_incomplete: false,
            item_defaults,
            items,
        }
    }

    /// tsgo `ensureItemData` of the list `toLSP` makes.
    fn ensure_item_data(&self, list: CompletionList) -> tsc_lsproto::CompletionList {
        let file_name = self.file_name();
        let mut items = list.items;
        for item in &mut items {
            if item.data.is_none() {
                item.data = Some(CompletionItemData {
                    file_name: file_name.clone(),
                    position: self.position as i32,
                    supplemental_file_index: None,
                    name: item.label.clone(),
                    ..CompletionItemData::default()
                });
            }
        }
        tsc_lsproto::CompletionList {
            is_incomplete: list.is_incomplete,
            item_defaults: list.item_defaults,
            apply_kind: None,
            items,
        }
    }
}
