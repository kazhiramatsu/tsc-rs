//! tsgo `ls/completions.go`: the completion list at a position and its
//! items.

mod context;
mod contextual;
mod data;
mod entries;
mod items;
mod keywords;

use tsc_checker::state::CheckResult;
use tsc_lsproto::{
    CompletionItem, CompletionItemData, CompletionItemDefaults, CompletionItemsOrListOrNull,
    CompletionParams, ResolvedClientCapabilities,
};

use crate::language_service::{LanguageService, LsError};

use context::Completions;
use data::CompletionData;

/// tsgo `SortText`s.
pub(crate) mod sort_text {
    pub(crate) const LOCAL_DECLARATION_PRIORITY: &str = "10";
    pub(crate) const LOCATION_PRIORITY: &str = "11";
    pub(crate) const SUGGESTED_CLASS_MEMBERS: &str = "14";
    pub(crate) const GLOBALS_OR_KEYWORDS: &str = "15";
    pub(crate) const JAVASCRIPT_IDENTIFIERS: &str = "18";

    /// tsgo `DeprecateSortText`.
    pub(crate) fn deprecated(original: &str) -> String {
        format!("z{original}")
    }
}

/// tsgo `KeywordCompletionFilters`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeywordCompletionFilters {
    /// No keywords.
    None,
    /// Every possible keyword.
    All,
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
}

/// tsgo `isMemberCompletionKind`.
pub(crate) fn is_member_completion_kind(kind: CompletionKind) -> bool {
    matches!(
        kind,
        CompletionKind::ObjectPropertyDeclaration | CompletionKind::PropertyAccess
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
            Ok(list.map(|list| completions.ensure_item_data(list)))
        })?;
        Ok(match list.flatten() {
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
        // JSDoc snippets (L4d), string completions (L4c) and label
        // completions (L4b) come first in tsgo.
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
            // The JSDoc completions (L4d).
            CompletionData::JSDocTagName
            | CompletionData::JSDocTag
            | CompletionData::JSDocParameterName => Ok(None),
        }
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
