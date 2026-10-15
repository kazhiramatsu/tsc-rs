//! tsgo `ResolveCompletionItem` (completions.go): the details and the
//! documentation of a completion item, found again from its data.

use tsc_checker::exports::VerbosityContext;
use tsc_checker::state::CheckResult;
use tsc_diagnostics::JsStr;
use tsc_lsproto::{
    CompletionItem, CompletionItemData, MarkupContent, MarkupKind, StringOrMarkupContent,
};
use tsc_types::{LiteralValue, SymbolFlags, SymbolId, TypeData};

use crate::language_service::{LanguageService, LsError};
use crate::utilities::Touching;

use super::context::{is_string_literal_like, Completions};
use super::data::CompletionData;
use super::strings::StringLiteralCompletions;

/// tsgo's completion sources the item data names.
use super::{
    SOURCE_CLASS_MEMBER_SNIPPET, SOURCE_OBJECT_LITERAL_MEMBER_WITH_COMMA,
    SOURCE_OBJECT_LITERAL_METHOD_SNIPPET, SOURCE_SWITCH_CASES,
};

/// tsgo `detailsData`: what the item's name finds in the completions
/// computed again.
enum DetailsData {
    /// A completion of no symbols (JSDoc or keywords).
    Request(CompletionData),
    Symbol {
        symbol: SymbolId,
        location: Touching,
    },
    Literal(LiteralValue),
    Cases,
    None,
}

impl LanguageService<'_> {
    /// tsgo `ResolveCompletionItem`.
    pub fn resolve_completion_item(
        &mut self,
        item: CompletionItem,
    ) -> Result<CompletionItem, LsError> {
        let Some(data) = item.data.clone() else {
            return Err(LsError("completion item data is nil".to_owned()));
        };
        let Some(file) = self.program.file_index(JsStr::from_str(&data.file_name)) else {
            return Err(LsError(format!("file not found: {}", data.file_name)));
        };
        let fallback = item.clone();
        let resolved = self.with_program_request(false, |request| {
            let Some(file) =
                request.source_for_supplemental_file_index(file, data.supplemental_file_index)
            else {
                return Ok(Err(LsError(format!(
                    "supplemental source file index not found: {}",
                    data.supplemental_file_index.unwrap_or_default()
                ))));
            };
            let mut completions = Completions::new(request, file, data.position as u32);
            completions.for_item_resolve = true;
            completions.completion_item_details(item, &data).map(Ok)
        })?;
        Ok(resolved.transpose()?.unwrap_or(fallback))
    }
}

impl Completions<'_, '_, '_> {
    /// tsgo `getCompletionItemDetails`.
    fn completion_item_details(
        &mut self,
        item: CompletionItem,
        data: &CompletionItemData,
    ) -> CheckResult<CompletionItem> {
        let markdown = self.documentation_format() == MarkupKind::MARKDOWN;
        let (context_token, previous_token) = self.relevant_tokens(self.position);
        if self.is_in_string(self.position, previous_token) {
            return self.string_literal_completion_details(
                item,
                &data.name,
                context_token,
                markdown,
            );
        }
        if let Some(fix) = &data.auto_import {
            if data.is_import_statement_completion {
                return Ok(item);
            }
            let request = &*self.request;
            let (edits, description, _) = crate::autoimport::fix_edits(
                &crate::autoimport::Fix::of(fix.clone()),
                self.source,
                request.state.options,
                request.preferences.format_code_settings.clone(),
                request.converters,
                &request.state.binder,
                request.preferences,
                request.catalog,
            )
            .unwrap_or_else(|error| panic!("{error}"));
            let mut item = item;
            item.additional_text_edits = Some(edits);
            item.detail = Some(description);
            return Ok(item);
        }
        Ok(match self.symbol_completion_from_item_data(data)? {
            DetailsData::Request(
                CompletionData::JSDocTagName
                | CompletionData::JSDocTag
                | CompletionData::JSDocParameterName(_),
            ) => create_completion_details(item, &data.name, "", markdown),
            DetailsData::Request(CompletionData::Keyword {
                keyword_completions,
                ..
            }) => {
                if keyword_completions
                    .iter()
                    .any(|keyword| keyword.label == data.name)
                {
                    create_completion_details(item, &data.name, "", markdown)
                } else {
                    item
                }
            }
            DetailsData::Request(CompletionData::Data(_)) => {
                unreachable!("symbol completions find a symbol")
            }
            DetailsData::Symbol { symbol, location } => {
                self.completion_details_for_symbol(item, symbol, location, markdown)?
            }
            DetailsData::Literal(literal) => {
                let name = self.completion_name_for_literal(&literal);
                create_completion_details(item, &name, "", markdown)
            }
            DetailsData::Cases => item,
            DetailsData::None => {
                // No symbol of the name: maybe a keyword.
                if tsc_syntax::identifier_to_keyword_kind(&data.name).is_some() {
                    create_completion_details(item, &data.name, "", markdown)
                } else {
                    item
                }
            }
        })
    }

    /// tsgo `getCompletionDocumentationFormat`: the client's first format.
    fn documentation_format(&self) -> MarkupKind {
        self.request
            .capabilities
            .text_document
            .completion
            .completion_item
            .documentation_format
            .first()
            .cloned()
            .unwrap_or(MarkupKind::PLAIN_TEXT)
    }

    /// tsgo `getSymbolCompletionFromItemData`: the completions computed
    /// again, and the symbol the item names among them.
    fn symbol_completion_from_item_data(
        &mut self,
        item_data: &CompletionItemData,
    ) -> CheckResult<DetailsData> {
        if item_data.source == SOURCE_SWITCH_CASES {
            return Ok(DetailsData::Cases);
        }
        let Some(completion_data) = self.get_completion_data()? else {
            return Ok(DetailsData::None);
        };
        let CompletionData::Data(data) = completion_data else {
            return Ok(DetailsData::Request(completion_data));
        };
        if let Some(literal) = data
            .literals
            .iter()
            .find(|&literal| self.completion_name_for_literal(literal) == item_data.name)
        {
            return Ok(DetailsData::Literal(literal.clone()));
        }
        // The names of the entries are good: no character checks.
        for (index, &symbol) in data.symbols.iter().enumerate() {
            let origin = data.symbol_to_origin_info_map.get(&index);
            let (display_name, _) = self.completion_entry_display_name_for_symbol(
                symbol,
                origin,
                data.completion_kind,
                data.is_jsx_identifier_expected,
            );
            if display_name != item_data.name {
                continue;
            }
            let flags = self.request.state.symbol_flags(symbol);
            let source = item_data.source.as_str();
            if source == SOURCE_CLASS_MEMBER_SNIPPET && flags.intersects(SymbolFlags::CLASS_MEMBER)
                || source == SOURCE_OBJECT_LITERAL_METHOD_SNIPPET
                    && flags.intersects(SymbolFlags::PROPERTY | SymbolFlags::METHOD)
                || super::entries::source_from_origin(origin) == source
                || source == SOURCE_OBJECT_LITERAL_MEMBER_WITH_COMMA
            {
                return Ok(DetailsData::Symbol {
                    symbol,
                    location: data.location,
                });
            }
        }
        Ok(DetailsData::None)
    }

    /// tsgo `createCompletionDetailsForSymbol`: the symbol's quick info and
    /// documentation.
    fn completion_details_for_symbol(
        &mut self,
        item: CompletionItem,
        symbol: SymbolId,
        location: Touching,
        markdown: bool,
    ) -> CheckResult<CompletionItem> {
        let info = self.request.quick_info_and_documentation(
            Some(symbol),
            location,
            markdown,
            &mut VerbosityContext::default(),
            false,
        )?;
        Ok(create_completion_details(
            item,
            &info.quick_info,
            &info.documentation,
            markdown,
        ))
    }

    /// tsgo `getStringLiteralCompletionDetails`.
    fn string_literal_completion_details(
        &mut self,
        item: CompletionItem,
        name: &str,
        context_token: Option<Touching>,
        markdown: bool,
    ) -> CheckResult<CompletionItem> {
        let Some(Touching::Node(token)) =
            context_token.filter(|&token| is_string_literal_like(self.kind(token)))
        else {
            return Ok(item);
        };
        let Some(completions) = self.string_literal_completion_entries(token)? else {
            return Ok(item);
        };
        match completions {
            // Path completions resolve their details eagerly.
            StringLiteralCompletions::Paths(_) => Ok(item),
            StringLiteralCompletions::Properties(properties) => {
                for symbol in properties.symbols {
                    if self.raw_symbol_name(symbol) == name {
                        return self.completion_details_for_symbol(
                            item,
                            symbol,
                            Touching::Node(token),
                            markdown,
                        );
                    }
                }
                Ok(item)
            }
            StringLiteralCompletions::Types(types) => {
                for ty in types.types {
                    if let TypeData::Literal {
                        value: LiteralValue::String(text),
                    } = &self.request.state.tables.type_of(ty).data
                    {
                        if text.to_js_string().to_string_lossy() == name {
                            return Ok(create_completion_details(item, name, "", markdown));
                        }
                    }
                }
                Ok(item)
            }
        }
    }
}

/// tsgo `createCompletionDetails`: the detail when the item has none, and
/// the documentation.
fn create_completion_details(
    mut item: CompletionItem,
    detail: &str,
    documentation: &str,
    markdown: bool,
) -> CompletionItem {
    if item.detail.is_none() && !detail.is_empty() {
        item.detail = Some(detail.to_owned());
    }
    if !documentation.is_empty() {
        item.documentation = Some(StringOrMarkupContent::MarkupContent(MarkupContent {
            kind: if markdown {
                MarkupKind::MARKDOWN
            } else {
                MarkupKind::PLAIN_TEXT
            },
            value: documentation.to_owned(),
        }));
    }
    item
}
