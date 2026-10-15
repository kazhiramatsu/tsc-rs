//! tsgo's completion items from the completion data:
//! `completionInfoFromData`, `getCompletionEntriesFromSymbols`,
//! `createCompletionItem` and what they use.

use std::collections::{HashMap, HashSet};

use tsc_binder::node_util;
use tsc_checker::exports::ConstantValue;
use tsc_checker::state::CheckResult;
use tsc_lsproto::{CompletionItem, CompletionItemKind, Range};
use tsc_syntax::{identifier_to_keyword_kind, NodeData, NodeId, SyntaxKind};
use tsc_types::{LiteralValue, SymbolFlags, SymbolId, TypeFlags, TypeId};

use crate::lsutil::{get_symbol_kind, get_symbol_modifiers, QuotePreference, ScriptElementKind};
use crate::utilities::Touching;

use super::context::Completions;
use super::data::{origin_is, origin_kind, CompletionDataData, SymbolOriginInfo};
use super::items::{create_lsp_completion_item, set_item_defaults, ItemSpec};
use super::keywords::{
    is_contextual_keyword_in_auto_importable_expression_space, is_type_keyword, keyword_completions,
};
use super::{
    is_member_completion_kind, sort_text, ClientSupport, CompletionKind, CompletionList,
    KeywordCompletionFilters,
};

/// tsgo `completionSource`s.
const SOURCE_THIS_PROPERTY: &str = "ThisProperty/";
const SOURCE_TYPE_ONLY_ALIAS: &str = "TypeOnlyAlias/";
const SOURCE_OBJECT_LITERAL_MEMBER_WITH_COMMA: &str = "ObjectLiteralMemberWithComma/";

impl Completions<'_, '_, '_> {
    /// tsgo `completionInfoFromData`.
    pub(crate) fn completion_info_from_data(
        &mut self,
        data: &mut CompletionDataData,
        optional_replacement_span: Option<Range>,
    ) -> CheckResult<Option<CompletionList>> {
        // In a JSX file, a closing tag's name.
        if self.source.language_variant == tsc_syntax::LanguageVariant::Jsx {
            if let Some(list) = self.jsx_closing_tag_completion(data.location) {
                return Ok(Some(list));
            }
        }

        let keyword_filters = data.keyword_filters;
        let is_new_identifier_location = data.is_new_identifier_location;
        let context_token = data.context_token;
        let mut literals = std::mem::take(&mut data.literals);

        // At `case |`, no literal or enum member whose value an existing
        // case clause has.
        if let Some(context) = context_token {
            let start = match context {
                Touching::Node(node) => Some(node),
                Touching::Token { parent, .. } => Some(parent),
            };
            if let Some(case_clause) = self.find_ancestor(start, |this, node| {
                this.node_kind(node) == SyntaxKind::CaseClause
            }) {
                let in_expression = self.kind(context) == SyntaxKind::CaseKeyword
                    || self.expression_of(case_clause).is_some_and(|expression| {
                        let record = self.record(expression);
                        record.pos <= self.pos(context)
                            && self.end(context) <= record.end
                            && self.is_descendant_of(context, expression)
                    });
                if in_expression {
                    let block = self.parent_of(case_clause).expect("a case clause's block");
                    let tracker = self.case_clause_tracker(block)?;
                    literals.retain(|literal| !tracker.has_value(literal));
                    let mut kept = Vec::with_capacity(data.symbols.len());
                    for &symbol in &data.symbols {
                        let value_declaration =
                            self.request.state.binder.symbol(symbol).value_declaration;
                        if let Some(declaration) = value_declaration.filter(|&declaration| {
                            self.request.state.binder.node_record(declaration).kind
                                == SyntaxKind::EnumMember
                        }) {
                            if let Some(value) =
                                self.request.state.get_constant_value(declaration)?
                            {
                                if tracker.has_constant(&value) {
                                    continue;
                                }
                            }
                        }
                        kept.push(symbol);
                    }
                    // The origins are by index: keep them with their symbols.
                    if kept.len() != data.symbols.len() {
                        let mut origins = HashMap::new();
                        let mut index = 0;
                        for (old_index, &symbol) in data.symbols.iter().enumerate() {
                            if kept.get(index) == Some(&symbol) {
                                if let Some(origin) =
                                    data.symbol_to_origin_info_map.remove(&old_index)
                                {
                                    origins.insert(index, origin);
                                }
                                index += 1;
                            }
                        }
                        data.symbol_to_origin_info_map = origins;
                        data.symbols = kept;
                    }
                }
            }
        }

        let is_checked = self.is_checked_file_public();
        if is_checked
            && !is_new_identifier_location
            && data.symbols.is_empty()
            && keyword_filters == KeywordCompletionFilters::None
        {
            return Ok(None);
        }

        let (mut unique_names, mut sorted_entries) =
            self.completion_entries_from_symbols(data, None)?;

        if data.keyword_filters != KeywordCompletionFilters::None {
            let filter_out_ts_only =
                !data.inside_js_doc_tag_type_expression && self.is_source_file_js();
            for keyword_entry in keyword_completions(data.keyword_filters, filter_out_ts_only) {
                let label = keyword_entry.label.clone();
                let keyword_kind = identifier_to_keyword_kind(&label);
                if data.is_type_only_location && keyword_kind.is_some_and(is_type_keyword)
                    || !data.is_type_only_location
                        && is_contextual_keyword_in_auto_importable_expression_space(&label)
                    || !unique_names.contains(&label)
                {
                    unique_names.insert(label);
                    sorted_entries.push(keyword_entry);
                }
            }
        }

        for keyword_entry in self.contextual_keywords(context_token) {
            if !unique_names.contains(&keyword_entry.label) {
                unique_names.insert(keyword_entry.label.clone());
                sorted_entries.push(keyword_entry);
            }
        }

        for literal in &literals {
            let entry = self.completion_item_for_literal(literal);
            unique_names.insert(entry.label.clone());
            sorted_entries.push(entry);
        }

        if !is_checked {
            self.js_completion_entries(&mut unique_names, &mut sorted_entries);
        }

        let support = ClientSupport(self.request.capabilities);
        let position = self
            .request
            .lsp_range(self.source, self.position as usize..self.position as usize)
            .start;
        let item_defaults = set_item_defaults(
            support,
            position,
            &mut sorted_entries,
            Some(data.default_commit_characters.clone()),
            optional_replacement_span,
        );
        Ok(Some(CompletionList {
            is_incomplete: data.has_unresolved_auto_imports,
            item_defaults,
            items: sorted_entries,
        }))
    }

    /// tsgo `getJsxClosingTagCompletion`: at a closing tag, its opening
    /// tag's name (with `>` when the closing tag has none).
    fn jsx_closing_tag_completion(&self, location: Touching) -> Option<CompletionList> {
        // Up the tree to a JSX closing element.
        let mut current = Some(location);
        let mut closing = None;
        while let Some(node) = current {
            match self.kind(node) {
                SyntaxKind::JsxClosingElement => {
                    if let Touching::Node(node) = node {
                        closing = Some(node);
                    }
                    break;
                }
                SyntaxKind::LessThanSlashToken
                | SyntaxKind::GreaterThanToken
                | SyntaxKind::Identifier
                | SyntaxKind::PropertyAccessExpression => {
                    current = self.parent(node).map(Touching::Node);
                }
                _ => break,
            }
        }
        let closing = closing?;
        // At `var x = <div> </ /*1*/` the name is `div>`; with the closing
        // `>`, `div`. A property access tag name is the whole closing tag
        // name.
        let has_closing_angle_bracket = self
            .child_of_kind(closing, SyntaxKind::GreaterThanToken)
            .is_some();
        let element = self.parent_of(closing)?;
        let NodeData::JsxElement(element) = self.data(element) else {
            return None;
        };
        let opening_tag_name = match self.data(element.opening_element?) {
            NodeData::JsxOpeningElement(data) => data.tag_name,
            _ => None,
        }?;
        let closing_tag = &self.text()[self.node_start(opening_tag_name) as usize
            ..self.record(opening_tag_name).end as usize];
        let full_closing_tag = if has_closing_angle_bracket {
            closing_tag.to_owned()
        } else {
            format!("{closing_tag}>")
        };
        let closing_tag_name = match self.data(closing) {
            NodeData::JsxClosingElement(data) => data.tag_name,
            _ => None,
        }?;
        let optional_replacement_span = self.request.lsp_range(
            self.source,
            crate::utilities::range_of_node(self.request.state, closing_tag_name),
        );
        let file_name = self.file_name();
        let mut items = vec![create_lsp_completion_item(
            self.text(),
            &file_name,
            self.position,
            ItemSpec {
                name: full_closing_tag,
                sort_text: sort_text::LOCATION_PRIORITY.to_owned(),
                element_kind: Some(ScriptElementKind::ClassElement),
                is_member_completion: true,
                ..ItemSpec::default()
            },
        )];
        let position = self
            .request
            .lsp_range(self.source, self.position as usize..self.position as usize)
            .start;
        let item_defaults = set_item_defaults(
            ClientSupport(self.request.capabilities),
            position,
            &mut items,
            Some(super::default_commit_characters(false)),
            Some(optional_replacement_span),
        );
        Some(CompletionList {
            is_incomplete: false,
            item_defaults,
            items,
        })
    }

    /// Whether `node` is `ancestor` or below it.
    fn is_descendant_of(&self, node: Touching, ancestor: NodeId) -> bool {
        let start = match node {
            Touching::Node(node) => Some(node),
            Touching::Token { parent, .. } => Some(parent),
        };
        self.find_ancestor(start, |_, current| current == ancestor)
            .is_some()
    }

    pub(crate) fn is_checked_file_public(&self) -> bool {
        !self.is_source_file_js()
            || match tsc_checker::check_directive(self.text()) {
                Some(tsc_checker::CheckDirective::Check) => true,
                Some(tsc_checker::CheckDirective::NoCheck) => false,
                None => self.request.state.options.check_js == Some(true),
            }
    }

    /// tsgo `getCompletionEntriesFromSymbols`: the items, and the names
    /// taken.
    pub(crate) fn completion_entries_from_symbols(
        &mut self,
        data: &CompletionDataData,
        replacement_token: Option<Touching>,
    ) -> CheckResult<(HashSet<String>, Vec<CompletionItem>)> {
        let closest_symbol_declaration =
            self.closest_symbol_declaration(data.context_token, data.location);
        let is_member_completion = is_member_completion_kind(data.completion_kind);
        let mut sorted_entries = Vec::with_capacity(data.symbols.len());
        // Whether a name shadows later symbols: true for locals, false for
        // globals and module exports, of which there may be several. Locals
        // come first, then globals, then module exports.
        let mut uniques: HashMap<String, bool> = HashMap::new();
        for (index, &symbol) in data.symbols.iter().enumerate() {
            let origin = data.symbol_to_origin_info_map.get(&index);
            let (name, needs_convert_property_access) = self
                .completion_entry_display_name_for_symbol(
                    symbol,
                    origin,
                    data.completion_kind,
                    data.is_jsx_identifier_expected,
                );
            if name.is_empty()
                || uniques.get(&name).copied().unwrap_or(false)
                    && !origin_is(origin, origin_kind::OBJECT_LITERAL_METHOD)
                || data.completion_kind == CompletionKind::Global
                    && !self.should_include_symbol(symbol, data, closest_symbol_declaration)?
            {
                continue;
            }
            // At a value location of a JavaScript file, no symbol that
            // surely is a type only.
            if !data.is_type_only_location
                && self.is_source_file_js()
                && self.symbol_appears_to_be_type_only(symbol)?
            {
                continue;
            }
            let original_sort_text = data
                .symbol_to_sort_text_map
                .get(&symbol)
                .cloned()
                .unwrap_or_else(|| sort_text::LOCATION_PRIORITY.to_owned());
            let sort_text = if self.is_deprecated(symbol)? {
                sort_text::deprecated(&original_sort_text)
            } else {
                original_sort_text
            };
            let Some(entry) = self.create_completion_item(
                symbol,
                sort_text,
                replacement_token,
                data,
                name.clone(),
                needs_convert_property_access,
                origin,
                is_member_completion,
            )?
            else {
                continue;
            };
            // Locals shadow; globals, other files' module exports and
            // `this.` members do not.
            let declared_here = self.declarations_of(symbol).iter().any(|&declaration| {
                self.request.state.binder.file_index_of_node(declaration) == self.file
            });
            let should_shadow_later_symbols = (origin.is_none()
                || origin_is(origin, origin_kind::TYPE_ONLY_ALIAS))
                && (self.request.state.binder.symbol(symbol).parent.is_some() || declared_here);
            uniques.insert(name, should_shadow_later_symbols);
            sorted_entries.push(entry);
        }

        let use_semicolons = !data.auto_imports.is_empty()
            && data.import_statement_completion.is_some()
            && crate::lsutil::probably_uses_semicolons(self.source);
        let file_name = self.file_name();
        for auto_import in &data.auto_imports {
            // !!! check for type-only in JS
            // !!! deprecation
            let fix = &auto_import.fix.auto_import;
            let export = &auto_import.export;
            let mut replacement_span = None;
            let mut insert_text = String::new();
            let mut filter_text = String::new();
            let mut is_snippet = false;
            let mut sort_text = sort_text::AUTO_IMPORT_SUGGESTIONS.to_owned();
            if let Some(info) = &data.import_statement_completion {
                is_snippet = ClientSupport(self.request.capabilities).item_snippet();
                let import_kind =
                    crate::autoimport::get_import_kind(self.request.state, self.file, export, true);
                insert_text = self.insert_text_for_import_completion(
                    fix,
                    import_kind,
                    info,
                    use_semicolons,
                    is_snippet,
                );
                replacement_span = info.replacement_span.clone();
                // The edit range covers the whole import statement typed so far, and clients match that text against the
                // filter text, so it has to be the statement being inserted (as in Strada), not just the bare name.
                filter_text = insert_text.clone();
                sort_text = sort_text::LOCATION_PRIORITY.to_owned();
            }
            // Non-contextual keywords (e.g., `function`, `class`, `const`) cannot be used as identifiers,
            // so auto-imports with these names should not shadow keyword completions.
            if tsc_syntax::keyword_kind(&fix.name)
                .is_some_and(super::keywords::is_non_contextual_keyword)
            {
                continue;
            }
            if !export.is_unresolved_alias() {
                if data.is_type_only_location {
                    if !export
                        .flags
                        .intersects(SymbolFlags::TYPE | SymbolFlags::MODULE)
                    {
                        continue;
                    }
                } else if data.import_statement_completion.is_none()
                    && !export.flags.intersects(SymbolFlags::VALUE)
                {
                    continue;
                }
            }
            let mut entry = create_lsp_completion_item(
                self.text(),
                &file_name,
                self.position,
                ItemSpec {
                    name: fix.name.clone(),
                    insert_text,
                    filter_text,
                    sort_text,
                    element_kind: Some(export.script_element_kind),
                    kind_modifiers: export.script_element_kind_modifiers,
                    replacement_span,
                    commit_characters: None,
                    label_details: Some(tsc_lsproto::CompletionItemLabelDetails {
                        description: Some(fix.module_specifier.clone()),
                        ..tsc_lsproto::CompletionItemLabelDetails::default()
                    }),
                    is_member_completion: false,
                    is_snippet,
                    preselect: false,
                    source: fix.module_specifier.clone(),
                    auto_import_fix: Some(fix.clone()),
                    additional_text_edits: None,
                    detail: None,
                },
            );
            if let Some(item_data) = &mut entry.data {
                item_data.is_import_statement_completion =
                    data.import_statement_completion.is_some();
            }
            if !uniques.get(&fix.name).copied().unwrap_or(false) {
                uniques.insert(fix.name.clone(), false);
                sorted_entries.push(entry);
            }
        }
        Ok((uniques.into_keys().collect(), sorted_entries))
    }

    /// tsgo `getInsertTextAndReplacementSpanForImportCompletion`'s text.
    fn insert_text_for_import_completion(
        &self,
        fix: &tsc_lsproto::AutoImportFix,
        import_kind: tsc_lsproto::ImportKind,
        info: &super::data::ImportStatementCompletionInfo,
        use_semicolons: bool,
        is_snippet: bool,
    ) -> String {
        use tsc_lsproto::ImportKind;
        let quoted_module_specifier = escape_snippet_text(&self.quote(&fix.module_specifier));
        let tab_stop = if is_snippet { "$1" } else { "" };
        let suffix = if use_semicolons { ";" } else { "" };
        let top_level_type_only_text = if info.is_top_level_type_only {
            " type "
        } else {
            " "
        };
        let name = escape_snippet_text(&fix.name);
        match import_kind {
            ImportKind::COMMON_JS => format!(
                "import{top_level_type_only_text}{name}{tab_stop} = require({quoted_module_specifier}){suffix}"
            ),
            ImportKind::DEFAULT => format!(
                "import{top_level_type_only_text}{name}{tab_stop} from {quoted_module_specifier}{suffix}"
            ),
            ImportKind::NAMESPACE => format!(
                "import{top_level_type_only_text}* as {name} from {quoted_module_specifier}{suffix}"
            ),
            ImportKind::NAMED => format!(
                "import{top_level_type_only_text}{{ {}{name}{tab_stop} }} from {quoted_module_specifier}{suffix}",
                if info.could_be_type_only_import_specifier { "type " } else { "" }
            ),
            kind => panic!("unhandled import kind: {}", kind.0),
        }
    }

    /// tsgo `createCompletionItem`.
    #[allow(clippy::too_many_arguments)]
    fn create_completion_item(
        &mut self,
        symbol: SymbolId,
        sort_text: String,
        replacement_token: Option<Touching>,
        data: &CompletionDataData,
        mut name: String,
        needs_convert_property_access: bool,
        origin: Option<&SymbolOriginInfo>,
        is_member_completion: bool,
    ) -> CheckResult<Option<CompletionItem>> {
        let support = ClientSupport(self.request.capabilities);
        let context_token = data.context_token;
        let mut insert_text = String::new();
        let filter_text = String::new();
        let mut is_snippet = false;
        let mut replacement_span = self.replacement_range_for_context_token(replacement_token);
        let mut source = source_from_origin(origin).to_owned();
        let insert_question_dot = origin_is(origin, origin_kind::NULLABLE);
        let use_braces =
            origin_is(origin, origin_kind::SYMBOL_MEMBER) || needs_convert_property_access;
        if origin_is(origin, origin_kind::THIS_TYPE) {
            insert_text = if needs_convert_property_access {
                format!(
                    "this{}[{}]",
                    if insert_question_dot { "?." } else { "" },
                    self.quote_property_name(&name)
                )
            } else {
                format!("this{}{name}", if insert_question_dot { "?." } else { "." })
            };
        } else if let Some(property_access) = data
            .property_access_to_convert
            .filter(|_| use_braces || insert_question_dot)
        {
            // Only a property access to convert needs braces (though see
            // microsoft/TypeScript#21790: a global with a name that is no
            // identifier).
            insert_text = if use_braces {
                if needs_convert_property_access {
                    format!("[{}]", self.quote_property_name(&name))
                } else {
                    format!("[{name}]")
                }
            } else {
                name.clone()
            };
            let NodeData::PropertyAccessExpression(access) = self.data(property_access) else {
                unreachable!("a property access to convert");
            };
            if insert_question_dot || access.question_dot_token.is_some() {
                insert_text = format!("?.{insert_text}");
            }
            let dot = self
                .child_of_kind(property_access, SyntaxKind::DotToken)
                .or_else(|| self.child_of_kind(property_access, SyntaxKind::QuestionDotToken));
            let Some(dot) = dot else {
                return Ok(None);
            };
            // Over the text after the `.` when the name starts with it;
            // else new text.
            let access_name = access
                .name
                .and_then(|name| node_util::id_text(self.source, name))
                .unwrap_or_default();
            let end = if name.starts_with(access_name) {
                self.record(property_access).end
            } else {
                self.end(dot)
            };
            replacement_span = Some(
                self.request
                    .lsp_range(self.source, self.start(dot) as usize..end as usize),
            );
        }

        if data.jsx_initializer.is_initializer {
            if insert_text.is_empty() {
                insert_text = name.clone();
            }
            insert_text = format!("{{{insert_text}}}");
            if let Some(initializer) = data.jsx_initializer.initializer {
                replacement_span = Some(self.request.lsp_range(
                    self.source,
                    crate::utilities::range_of_node(self.request.state, initializer),
                ));
            }
        }

        if origin_is(origin, origin_kind::PROMISE) {
            if let Some(property_access) = data.property_access_to_convert {
                if insert_text.is_empty() {
                    insert_text = name.clone();
                }
                let preceding = self.preceding_token(self.record(property_access).pos);
                let mut await_text = String::new();
                if let Some(preceding) = preceding {
                    if self.position_is_asi_candidate(self.end(preceding), preceding) {
                        await_text.push(';');
                    }
                }
                let NodeData::PropertyAccessExpression(access) = self.data(property_access) else {
                    unreachable!("a property access to convert");
                };
                let expression = access.expression.expect("a property access's expression");
                let expression_text = &self.text()
                    [self.node_start(expression) as usize..self.record(expression).end as usize];
                await_text.push_str(&format!("(await {expression_text})"));
                insert_text = if needs_convert_property_access {
                    format!("{await_text}{insert_text}")
                } else {
                    format!(
                        "{await_text}{}{insert_text}",
                        if insert_question_dot { "?." } else { "." }
                    )
                };
                let in_await_expression = self
                    .parent_of(property_access)
                    .is_some_and(|parent| self.node_kind(parent) == SyntaxKind::AwaitExpression);
                let wrap_node = if in_await_expression {
                    self.parent_of(property_access)
                        .expect("an await expression")
                } else {
                    expression
                };
                replacement_span = Some(self.request.lsp_range(
                    self.source,
                    self.node_start(wrap_node) as usize..self.record(property_access).end as usize,
                ));
            }
        }

        // Members of an object literal missing a comma: the comma is
        // inserted (`const cc: I = { a: "red" | }` completes `b`).
        if data.completion_kind == CompletionKind::ObjectPropertyDeclaration {
            if let Some(context) = context_token {
                let preceding = self.navigator.preceding_token_from(
                    self.pos(context),
                    Some(crate::tree::View::Node(match context {
                        Touching::Node(node) => node,
                        Touching::Token { parent, .. } => parent,
                    })),
                    false,
                );
                let after_comma = preceding
                    .and_then(crate::utilities::touching)
                    .is_some_and(|preceding| self.kind(preceding) == SyntaxKind::CommaToken);
                if !after_comma {
                    let parent = self.parent(context).expect("a context token's parent");
                    let grandparent_kind = self.parent_of(parent).map(|node| self.node_kind(node));
                    let property_assignment = self.find_ancestor(Some(parent), |this, node| {
                        this.node_kind(node) == SyntaxKind::PropertyAssignment
                    });
                    if matches!(
                        grandparent_kind,
                        Some(
                            SyntaxKind::MethodDeclaration
                                | SyntaxKind::GetAccessor
                                | SyntaxKind::SetAccessor
                        )
                    ) || self.node_kind(parent) == SyntaxKind::SpreadAssignment
                        || property_assignment.and_then(|node| self.last_token(node))
                            == Some(context)
                        || self.node_kind(parent) == SyntaxKind::ShorthandPropertyAssignment
                            && self.line_of(self.end(context)) != self.line_of(self.position)
                    {
                        source = SOURCE_OBJECT_LITERAL_MEMBER_WITH_COMMA.to_owned();
                    }
                }
            }
        }

        // A JSX attribute's snippet.
        let location_parent = self.parent(data.location);
        if data.is_jsx_identifier_expected
            && !data.is_right_of_open_tag
            && support.item_snippet()
            && self
                .request
                .preferences
                .completion_preferences
                .jsx_attribute_completion_style
                != crate::lsutil::JsxAttributeCompletionStyle::NONE
            && !location_parent.is_some_and(|parent| {
                self.node_kind(parent) == SyntaxKind::JsxAttribute
                    && self.initializer_of(parent).is_some()
            })
        {
            let use_braces_preference = self
                .request
                .preferences
                .completion_preferences
                .jsx_attribute_completion_style
                == crate::lsutil::JsxAttributeCompletionStyle::BRACES;
            let mut use_braces = use_braces_preference;
            let location_node = match data.location {
                Touching::Node(node) => node,
                Touching::Token { parent, .. } => parent,
            };
            let ty = self
                .request
                .state
                .get_type_of_symbol_at_location(symbol, Some(location_node))?;
            // Boolean-like or undefined: just the name.
            if self
                .request
                .preferences
                .completion_preferences
                .jsx_attribute_completion_style
                == crate::lsutil::JsxAttributeCompletionStyle::AUTO
                && !self.is_boolean_like(ty)
                && !(self.is_union_type(ty)
                    && self
                        .union_types(ty)
                        .into_iter()
                        .any(|t| self.is_boolean_like(t)))
            {
                let string_like = self
                    .request
                    .state
                    .tables
                    .flags_of(ty)
                    .intersects(TypeFlags::STRING_LIKE)
                    || self.is_union_type(ty) && {
                        let mut all = true;
                        for t in self.union_types(ty) {
                            if !(self
                                .request
                                .state
                                .tables
                                .flags_of(t)
                                .intersects(TypeFlags::STRING_LIKE | TypeFlags::UNDEFINED)
                                || self.is_string_and_empty_anonymous_object_intersection(t)?)
                            {
                                all = false;
                                break;
                            }
                        }
                        all
                    };
                if string_like {
                    // String-like or undefined: quotes.
                    insert_text = format!("{}={}", escape_snippet_text(&name), self.quote("$1"));
                    is_snippet = true;
                } else {
                    // Braces for anything else.
                    use_braces = true;
                }
            }
            if use_braces {
                insert_text = format!("{}={{$1}}", escape_snippet_text(&name));
                is_snippet = true;
            }
        }

        // In named imports or exports, a name that is no identifier is a
        // string.
        let named_imports_or_exports = self.find_ancestor(
            match data.location {
                Touching::Node(node) => Some(node),
                Touching::Token { parent, .. } => Some(parent),
            },
            |this, node| {
                matches!(
                    this.node_kind(node),
                    SyntaxKind::NamedImports | SyntaxKind::NamedExports
                )
            },
        );
        if let Some(named) = named_imports_or_exports {
            if !is_identifier_text_of_variant(&name, false) {
                insert_text = self.quote_property_name(&name);
                if self.node_kind(named) == SyntaxKind::NamedImports {
                    // `import { ^here as name }` or `{ ^here, as, name }`.
                    let mut scanner =
                        tsc_syntax::TokenScanner::new(self.source, self.position as usize);
                    let first = scanner.token();
                    let second = scanner.scan();
                    if !(first == SyntaxKind::AsKeyword && second == SyntaxKind::Identifier) {
                        insert_text = format!(
                            "{insert_text} as {}",
                            generate_identifier_for_arbitrary_string(&name)
                        );
                    }
                }
            } else if self.node_kind(named) == SyntaxKind::NamedImports {
                if let Some(token) = identifier_to_keyword_kind(&name) {
                    if token == SyntaxKind::AwaitKeyword
                        || super::keywords::is_non_contextual_keyword(token)
                    {
                        insert_text = format!("{name} as {name}_");
                    }
                }
            }
        }

        // Commit characters.
        let location_node = match data.location {
            Touching::Node(node) => node,
            Touching::Token { parent, .. } => parent,
        };
        let element_kind = get_symbol_kind(self.request.state, symbol, location_node, true)?;
        let mut commit_characters = None;
        if support.item_commit_characters() {
            if element_kind == ScriptElementKind::Warning
                || element_kind == ScriptElementKind::String
            {
                commit_characters = Some(Vec::new());
            } else if !support.default_commit_characters() {
                commit_characters = Some(data.default_commit_characters.clone());
            }
            // Otherwise the list's default.
        }

        let preselect = self.is_recommended_completion_match(symbol, data.recommended_completion);
        let kind_modifiers = get_symbol_modifiers(self.request.state, symbol, true)?;

        let text = self.text();
        let file_name = self.file_name();
        let item = create_lsp_completion_item(
            text,
            &file_name,
            self.position,
            ItemSpec {
                name: std::mem::take(&mut name),
                insert_text,
                filter_text,
                sort_text,
                element_kind: Some(element_kind),
                kind_modifiers,
                replacement_span,
                commit_characters,
                label_details: None,
                is_member_completion,
                is_snippet,
                preselect,
                source,
                auto_import_fix: None,
                additional_text_edits: None,
                detail: None,
            },
        );
        Ok(Some(item))
    }

    /// The file's name, as the item data carries it.
    pub(crate) fn file_name(&self) -> String {
        self.source.file_name.to_string_lossy().into_owned()
    }

    /// tsgo `Type.IsBooleanLike`.
    fn is_boolean_like(&self, ty: TypeId) -> bool {
        self.request
            .state
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::BOOLEAN_LIKE)
    }

    fn is_union_type(&self, ty: TypeId) -> bool {
        self.request
            .state
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::UNION)
    }

    /// tsgo `isStringAndEmptyAnonymousObjectIntersection`: `string & {}`.
    fn is_string_and_empty_anonymous_object_intersection(
        &mut self,
        ty: TypeId,
    ) -> CheckResult<bool> {
        if !self
            .request
            .state
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::INTERSECTION)
        {
            return Ok(false);
        }
        let types = self.union_types(ty);
        if types.len() != 2 {
            return Ok(false);
        }
        Ok(
            self.are_intersected_types_avoiding_string_reduction(types[0], types[1])?
                || self.are_intersected_types_avoiding_string_reduction(types[1], types[0])?,
        )
    }

    /// tsgo `areIntersectedTypesAvoidingStringReduction`.
    fn are_intersected_types_avoiding_string_reduction(
        &mut self,
        first: TypeId,
        second: TypeId,
    ) -> CheckResult<bool> {
        Ok(self
            .request
            .state
            .tables
            .flags_of(first)
            .intersects(TypeFlags::STRING)
            && self.request.state.is_empty_anonymous_object_type(second)?)
    }

    /// tsgo `isRecommendedCompletionMatch`.
    fn is_recommended_completion_match(
        &self,
        symbol: SymbolId,
        recommended: Option<SymbolId>,
    ) -> bool {
        let Some(recommended) = recommended else {
            return false;
        };
        symbol == recommended
            || self
                .request
                .state
                .binder
                .symbol(symbol)
                .flags
                .intersects(SymbolFlags::EXPORT_VALUE)
                && self.request.state.get_export_symbol_of_symbol(symbol) == recommended
    }

    /// tsgo `getCompletionEntryDisplayNameForSymbol`: the name, and whether
    /// a property access needs converting to an element access.
    pub(super) fn completion_entry_display_name_for_symbol(
        &self,
        symbol: SymbolId,
        origin: Option<&SymbolOriginInfo>,
        completion_kind: CompletionKind,
        is_jsx_identifier_expected: bool,
    ) -> (String, bool) {
        if origin_is(origin, origin_kind::IGNORE) {
            return (String::new(), false);
        }
        let name = if origin_is(origin, origin_kind::COMPUTED_PROPERTY_NAME) {
            origin
                .and_then(|origin| origin.symbol_name.clone())
                .expect("a computed property name's symbol name")
        } else {
            self.symbol_name(symbol)
        };
        let data = self.request.state.binder.symbol(symbol);
        if name.is_empty()
            // No external module (`declare module "http"` names `"http"`).
            || data.flags.intersects(SymbolFlags::MODULE) && starts_with_quote(&name)
            // A unique symbol's internal name is no entry (tsgo
            // `checker.IsKnownSymbol`).
            || data
                .escaped_name
                .as_js()
                .starts_with(tsc_types::InternalSymbolName::LATE_BOUND_PREFIX)
        {
            return (String::new(), false);
        }
        let is_identifier = is_identifier_text_of_variant(&name, is_jsx_identifier_expected);
        let private_element = data.value_declaration.is_some_and(|declaration| {
            let source = self.request.state.binder.source_of_node(declaration);
            node_util::get_name_of_declaration(source, declaration)
                .is_some_and(|name| source.arena.node(name).kind == SyntaxKind::PrivateIdentifier)
                && matches!(
                    source.arena.node(declaration).kind,
                    SyntaxKind::PropertyDeclaration
                        | SyntaxKind::MethodDeclaration
                        | SyntaxKind::GetAccessor
                        | SyntaxKind::SetAccessor
                )
        });
        if is_identifier || private_element {
            return (name, false);
        }
        if data.flags.intersects(SymbolFlags::ALIAS) {
            // An import or export alias may be no identifier: it can be
            // inserted as a string.
            return (name, true);
        }
        match completion_kind {
            CompletionKind::MemberLike => {
                if origin_is(origin, origin_kind::COMPUTED_PROPERTY_NAME) {
                    return (
                        origin
                            .and_then(|origin| origin.symbol_name.clone())
                            .unwrap_or_default(),
                        false,
                    );
                }
                (String::new(), false)
            }
            CompletionKind::ObjectPropertyDeclaration => (self.quote(&name), false),
            CompletionKind::PropertyAccess | CompletionKind::Global => {
                // A `this.` completion is global, but may have a name that is
                // no identifier; none for a name starting with a space
                // (microsoft/TypeScript#20547).
                if name.starts_with(' ') {
                    return (String::new(), false);
                }
                (name, true)
            }
            CompletionKind::None | CompletionKind::String => (name, false),
        }
    }

    /// tsgo `shouldIncludeSymbol`.
    fn should_include_symbol(
        &mut self,
        symbol: SymbolId,
        data: &CompletionDataData,
        closest_symbol_declaration: Option<NodeId>,
    ) -> CheckResult<bool> {
        let mut all_flags = self.request.state.symbol_flags(symbol);
        let location = data.location;
        // `export = /**/` takes every meaning.
        if self
            .parent(location)
            .is_some_and(|parent| self.node_kind(parent) == SyntaxKind::ExportAssignment)
        {
            return Ok(true);
        }
        let symbol_data = self.request.state.binder.symbol(symbol);
        let value_declaration = symbol_data.value_declaration;
        let first_declaration = symbol_data.declarations.first().copied();
        let symbol_parent = symbol_data.parent;
        // Not a variable in its own initializer: `const a = /* no 'a' */`.
        if let Some(closest) = closest_symbol_declaration {
            if self.node_kind(closest) == SyntaxKind::VariableDeclaration
                && value_declaration == Some(closest)
            {
                return Ok(false);
            }
        }
        // Not the current or a later parameter in a default:
        // `function f(a = /* no 'a' and 'b' */, b) { }`,
        // `function f<T = /* no 'T' and 'T2' */>(a: T, b: T2) { }`.
        let symbol_declaration = value_declaration.or(first_declaration);
        if let (Some(closest), Some(declaration)) = (closest_symbol_declaration, symbol_declaration)
        {
            let declaration_record = self.request.state.binder.node_record(declaration);
            let closest_kind = self.node_kind(closest);
            if closest_kind == SyntaxKind::Parameter
                && declaration_record.kind == SyntaxKind::Parameter
            {
                let parameters_end = self
                    .parent_of(closest)
                    .and_then(|function| self.parameter_list_of(function))
                    .map(|list| self.list_range(list).1);
                if let Some(end) = parameters_end {
                    if declaration_record.pos >= self.record(closest).pos
                        && declaration_record.pos < end
                    {
                        return Ok(false);
                    }
                }
            } else if closest_kind == SyntaxKind::TypeParameter
                && declaration_record.kind == SyntaxKind::TypeParameter
            {
                if closest == declaration
                    && data
                        .context_token
                        .is_some_and(|context| self.kind(context) == SyntaxKind::ExtendsKeyword)
                {
                    // `type A<K extends /* no 'K' */> = K`
                    return Ok(false);
                }
                let closest_parent = self.parent_of(closest);
                if self.is_in_type_parameter_default(data.context_token)
                    && closest_parent
                        .is_none_or(|parent| self.node_kind(parent) != SyntaxKind::InferType)
                {
                    let type_parameters_end = closest_parent
                        .and_then(|parent| self.type_parameter_list_of(parent))
                        .map(|list| self.list_range(list).1);
                    if let Some(end) = type_parameters_end {
                        if declaration_record.pos >= self.record(closest).pos
                            && declaration_record.pos < end
                        {
                            return Ok(false);
                        }
                    }
                }
            }
        }

        // A module's global export declarations are global keywords in
        // every scope; a module with an explicit export filters them out
        // so the auto-imports win.
        let symbol_origin = self.request.state.skip_alias(symbol)?;
        if self.source.external_module_indicator.is_some()
            && self.request.state.options.allow_umd_global_access != Some(true)
            && symbol != symbol_origin
            && data
                .symbol_to_sort_text_map
                .get(&symbol)
                .map(String::as_str)
                == Some(sort_text::GLOBALS_OR_KEYWORDS)
            && symbol_parent.is_some_and(|parent| self.is_external_module_symbol(parent))
        {
            return Ok(false);
        }

        all_flags |= self.combined_local_and_export_symbol_flags(symbol_origin);
        if self
            .request
            .state
            .symbol_flags(symbol)
            .intersects(SymbolFlags::ALIAS)
        {
            all_flags |= self.request.state.get_symbol_flags_of(symbol)?;
        }

        // `import m = /**/` reaches only a namespace.
        if let Touching::Node(location_node) = location {
            if self.is_in_right_side_of_internal_import_equals_declaration(location_node) {
                return Ok(all_flags.intersects(SymbolFlags::NAMESPACE));
            }
        }
        if data.is_type_only_location {
            // A type, or a namespace with one.
            return self.symbol_can_be_referenced_at_type_location(symbol, &mut HashSet::new());
        }
        // An expression is in value space (value namespaces included).
        Ok(all_flags.intersects(SymbolFlags::VALUE))
    }

    /// tsgo `Symbol.CombinedLocalAndExportSymbolFlags`.
    fn combined_local_and_export_symbol_flags(&self, symbol: SymbolId) -> SymbolFlags {
        let data = self.request.state.binder.symbol(symbol);
        match data.export_symbol {
            Some(export) => data.flags | self.request.state.binder.symbol(export).flags,
            None => data.flags,
        }
    }

    /// tsgo `symbolAppearsToBeTypeOnly`.
    fn symbol_appears_to_be_type_only(&mut self, symbol: SymbolId) -> CheckResult<bool> {
        let target = self.request.state.skip_alias(symbol)?;
        let flags = self.combined_local_and_export_symbol_flags(target);
        let declarations = self.declarations_of(symbol);
        Ok(!flags.intersects(SymbolFlags::VALUE)
            && (declarations.is_empty()
                || !self.is_in_js_file(declarations[0])
                || flags.intersects(SymbolFlags::TYPE)))
    }

    /// tsgo `ast.IsInJSFile`.
    fn is_in_js_file(&self, node: NodeId) -> bool {
        let source = self.request.state.binder.source_of_node(node);
        source.arena.node(source.root).flags & tsc_types::NodeFlags::JAVA_SCRIPT_FILE.bits() != 0
    }

    /// tsgo `isDeprecated`: every declaration of the alias's target is.
    fn is_deprecated(&mut self, symbol: SymbolId) -> CheckResult<bool> {
        let target = self.request.state.skip_alias(symbol)?;
        let declarations = self.declarations_of(target);
        Ok(!declarations.is_empty()
            && declarations
                .iter()
                .all(|&declaration| self.request.state.is_deprecated_declaration(declaration)))
    }

    /// tsgo `getClosestSymbolDeclaration`: the parameter or type parameter
    /// the context token is in, else the variable declaration the location
    /// is in.
    fn closest_symbol_declaration(
        &self,
        context: Option<Touching>,
        location: Touching,
    ) -> Option<NodeId> {
        let context = context?;
        let quits = |this: &Self, node: NodeId| {
            this.is_function_block(node)
                || this.is_arrow_function_body(Touching::Node(node))
                || node_util::is_binding_pattern(this.source, node)
        };
        // tsgo's FindAncestorOrQuit from the token itself.
        let mut current = match context {
            Touching::Node(node) => Some(node),
            Touching::Token { parent, .. } => {
                if self.is_arrow_function_body(context) {
                    None
                } else {
                    Some(parent)
                }
            }
        };
        let mut closest = None;
        while let Some(node) = current {
            if quits(self, node) {
                break;
            }
            let kind = self.node_kind(node);
            if matches!(kind, SyntaxKind::Parameter | SyntaxKind::TypeParameter)
                && self
                    .parent_of(node)
                    .is_none_or(|parent| self.node_kind(parent) != SyntaxKind::IndexSignature)
            {
                closest = Some(node);
                break;
            }
            current = self.parent_of(node);
        }
        if closest.is_none() {
            let mut current = match location {
                Touching::Node(node) => Some(node),
                Touching::Token { parent, .. } => {
                    if self.is_arrow_function_body(location) {
                        None
                    } else {
                        Some(parent)
                    }
                }
            };
            while let Some(node) = current {
                if quits(self, node) {
                    break;
                }
                if self.node_kind(node) == SyntaxKind::VariableDeclaration {
                    closest = Some(node);
                    break;
                }
                current = self.parent_of(node);
            }
        }
        closest
    }

    /// tsgo `ast.IsFunctionBlock`.
    fn is_function_block(&self, node: NodeId) -> bool {
        self.node_kind(node) == SyntaxKind::Block
            && self.parent_of(node).is_some_and(|parent| {
                crate::utilities::is_function_like_kind(self.node_kind(parent))
            })
    }

    /// tsgo `isArrowFunctionBody`: an arrow function's body, or its `=>`
    /// (`const a = () => /**/;`).
    fn is_arrow_function_body(&self, node: Touching) -> bool {
        let Some(parent) = self.parent(node) else {
            return false;
        };
        self.node_kind(parent) == SyntaxKind::ArrowFunction
            && (self.body_of(parent).map(Touching::Node) == Some(node)
                || self.kind(node) == SyntaxKind::EqualsGreaterThanToken)
    }

    /// tsgo `isInTypeParameterDefault`.
    fn is_in_type_parameter_default(&self, context: Option<Touching>) -> bool {
        let Some(context) = context else {
            return false;
        };
        let mut node = context;
        let mut parent = self.parent(context);
        while let Some(current) = parent {
            if let NodeData::TypeParameter(data) = self.data(current) {
                return data.default.map(Touching::Node) == Some(node)
                    || self.kind(node) == SyntaxKind::EqualsToken;
            }
            node = Touching::Node(current);
            parent = self.parent_of(current);
        }
        false
    }

    /// tsgo `getContextualKeywords`: `assert` after an import's or a
    /// re-export's module specifier on its line.
    fn contextual_keywords(&self, context: Option<Touching>) -> Vec<CompletionItem> {
        let Some(context) = context else {
            return Vec::new();
        };
        let Some(parent) = self.parent(context) else {
            return Vec::new();
        };
        let token_line = self.line_of(self.end(context));
        let current_line = self.line_of(self.position);
        let specifier = match self.data(parent) {
            NodeData::ImportDeclaration(data) => data.module_specifier,
            NodeData::ExportDeclaration(data) => data.module_specifier,
            _ => None,
        };
        if specifier.is_some_and(|specifier| Touching::Node(specifier) == context)
            && token_line == current_line
        {
            return vec![CompletionItem {
                label: "assert".to_owned(),
                kind: Some(CompletionItemKind::KEYWORD),
                sort_text: Some(sort_text::GLOBALS_OR_KEYWORDS.to_owned()),
                ..CompletionItem::default()
            }];
        }
        Vec::new()
    }

    /// tsgo `createCompletionItemForLiteral`.
    fn completion_item_for_literal(&self, literal: &LiteralValue) -> CompletionItem {
        CompletionItem {
            label: self.completion_name_for_literal(literal),
            kind: Some(CompletionItemKind::CONSTANT),
            sort_text: Some(sort_text::LOCATION_PRIORITY.to_owned()),
            commit_characters: Some(Vec::new()),
            ..CompletionItem::default()
        }
    }

    /// tsgo `completionNameForLiteral`.
    pub(super) fn completion_name_for_literal(&self, literal: &LiteralValue) -> String {
        match literal {
            LiteralValue::String(text) => self.quote_js(&text.to_js_string()),
            // Go's JSON encoding of a float: ES's number formatting, but
            // `-0` keeps its sign.
            LiteralValue::Number(value) => {
                if *value == 0.0 && value.is_sign_negative() {
                    "-0".to_owned()
                } else {
                    tsc_types::js_number_to_string(*value)
                }
            }
            LiteralValue::BigInt(value) => format!("{}n", value.to_base10_string()),
        }
    }

    /// tsgo `getJSCompletionEntries`: every identifier of the file, as
    /// text.
    fn js_completion_entries(
        &self,
        unique_names: &mut HashSet<String>,
        entries: &mut Vec<CompletionItem>,
    ) {
        for (name, pos) in self.request.name_table(self.source) {
            // Not an identifier written only at the position.
            if pos == Some(self.position) {
                continue;
            }
            if !unique_names.contains(&name) && is_identifier_text_of_variant(&name, false) {
                unique_names.insert(name.clone());
                entries.push(CompletionItem {
                    label: name,
                    kind: Some(CompletionItemKind::TEXT),
                    sort_text: Some(sort_text::JAVASCRIPT_IDENTIFIERS.to_owned()),
                    commit_characters: Some(Vec::new()),
                    ..CompletionItem::default()
                });
            }
        }
    }

    /// tsgo `lsutil.PositionIsASICandidate`.
    fn position_is_asi_candidate(&self, pos: u32, context: Touching) -> bool {
        crate::lsutil::position_is_asi_candidate(
            &self.navigator,
            self.line_starts(),
            pos as usize,
            self.found(context),
        )
    }

    /// tsgo `quote`: a string literal in the file's quote style.
    pub(crate) fn quote(&self, text: &str) -> String {
        self.quote_quoted(go_json_quote(text.chars().map(Ok)))
    }

    fn quote_js(&self, text: &tsc_types::JsString) -> String {
        self.quote_quoted(go_json_quote(
            char::decode_utf16(text.code_units()).map(|unit| unit.map_err(|_| ())),
        ))
    }

    fn quote_quoted(&self, quoted: String) -> String {
        if self.quote_preference() == QuotePreference::SINGLE {
            let inner = &quoted[1..quoted.len() - 1];
            return format!("'{}'", inner.replace('\'', "\\'").replace("\\\"", "\""));
        }
        quoted
    }

    /// tsgo `quotePropertyName`: a name starting with a digit as it is,
    /// else quoted.
    fn quote_property_name(&self, name: &str) -> String {
        if name.chars().next().is_some_and(is_unicode_digit) {
            return name.to_owned();
        }
        self.quote(name)
    }

    /// tsgo `lsutil.GetQuotePreference`.
    pub(super) fn quote_preference(&self) -> QuotePreference {
        crate::lsutil::get_quote_preference(self.source, self.request.preferences)
    }

    /// tsgo `getOptionalReplacementSpan`: an identifier's range.
    /// tsgo `getReplacementRangeForContextToken`: a string literal's
    /// content, else the token.
    pub(crate) fn replacement_range_for_context_token(
        &self,
        context_token: Option<Touching>,
    ) -> Option<Range> {
        let context_token = context_token?;
        match (self.kind(context_token), context_token) {
            (
                SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral,
                Touching::Node(node),
            ) => self.range_from_string_literal_like_content(node),
            _ => Some(self.request.lsp_range(
                self.source,
                self.start(context_token) as usize..self.end(context_token) as usize,
            )),
        }
    }

    /// tsgo `createRangeFromStringLiteralLikeContent`: inside the quotes,
    /// up to the position in an unterminated literal (none in an empty
    /// one).
    pub(crate) fn range_from_string_literal_like_content(&self, node: NodeId) -> Option<Range> {
        let mut replacement_end = self.record(node).end - 1;
        let node_start = self.node_start(node);
        if self.is_unterminated_literal(Touching::Node(node)) {
            if node_start == replacement_end {
                return None;
            }
            replacement_end = self.position.min(self.record(node).end);
        }
        Some(self.request.lsp_range(
            self.source,
            (node_start + 1) as usize..replacement_end as usize,
        ))
    }

    pub(crate) fn optional_replacement_span(&self, location: Option<Touching>) -> Option<Range> {
        let location = location?;
        if !matches!(
            self.kind(location),
            SyntaxKind::Identifier | SyntaxKind::PrivateIdentifier
        ) {
            return None;
        }
        Some(self.request.lsp_range(
            self.source,
            self.start(location) as usize..self.end(location) as usize,
        ))
    }

    /// tsgo `newCaseClauseTracker`: the values of the case clauses.
    pub(crate) fn case_clause_tracker(
        &mut self,
        case_block: NodeId,
    ) -> CheckResult<CaseClauseTracker> {
        let mut tracker = CaseClauseTracker::default();
        let NodeData::CaseBlock(block) = self.data(case_block) else {
            return Ok(tracker);
        };
        let clauses = self.node_list(block.clauses).to_vec();
        for clause in clauses {
            if self.node_kind(clause) == SyntaxKind::DefaultClause {
                continue;
            }
            let Some(expression) = self.expression_of(clause) else {
                continue;
            };
            let skipped = node_util::skip_parentheses_pub(self.source, expression);
            match self.data(skipped) {
                NodeData::StringLiteral(data) => {
                    tracker
                        .strings
                        .insert(data.text.to_string_lossy().into_owned());
                }
                NodeData::NoSubstitutionTemplateLiteral(data) => {
                    tracker
                        .strings
                        .insert(data.text.to_string_lossy().into_owned());
                }
                NodeData::NumericLiteral(data) => {
                    tracker
                        .numbers
                        .push(data.text.parse::<f64>().unwrap_or(f64::NAN));
                }
                NodeData::BigIntLiteral(data) => {
                    tracker
                        .big_ints
                        .insert(data.text.trim_end_matches('n').to_owned());
                }
                _ => {
                    if super::context::is_literal_kind(self.node_kind(skipped)) {
                        continue;
                    }
                    let Some(symbol) = self.request.state.get_symbol_at_location(expression)?
                    else {
                        continue;
                    };
                    let Some(declaration) =
                        self.request.state.binder.symbol(symbol).value_declaration
                    else {
                        continue;
                    };
                    if self.request.state.binder.node_record(declaration).kind
                        != SyntaxKind::EnumMember
                    {
                        continue;
                    }
                    match self.request.state.get_constant_value(declaration)? {
                        Some(ConstantValue::String(text)) => {
                            tracker.strings.insert(text.to_string_lossy().into_owned());
                        }
                        Some(ConstantValue::Number(value)) => tracker.numbers.push(value),
                        None => {}
                    }
                }
            }
        }
        Ok(tracker)
    }
}

/// tsgo `caseClauseTracker`.
#[derive(Default)]
pub(crate) struct CaseClauseTracker {
    strings: HashSet<String>,
    numbers: Vec<f64>,
    big_ints: HashSet<String>,
}

impl CaseClauseTracker {
    pub(crate) fn has_value(&self, literal: &LiteralValue) -> bool {
        match literal {
            LiteralValue::String(text) => self
                .strings
                .contains(&text.to_js_string().to_string_lossy().into_owned()),
            LiteralValue::Number(value) => self.numbers.iter().any(|existing| existing == value),
            LiteralValue::BigInt(value) => self.big_ints.contains(&value.to_base10_string()),
        }
    }

    fn has_constant(&self, value: &ConstantValue) -> bool {
        match value {
            ConstantValue::String(text) => {
                self.strings.contains(&text.to_string_lossy().into_owned())
            }
            ConstantValue::Number(value) => self.numbers.iter().any(|existing| existing == value),
        }
    }
}

/// tsgo `getSourceFromOrigin`.
pub(super) fn source_from_origin(origin: Option<&SymbolOriginInfo>) -> &'static str {
    if origin_is(origin, origin_kind::THIS_TYPE) {
        return SOURCE_THIS_PROPERTY;
    }
    if origin_is(origin, origin_kind::TYPE_ONLY_ALIAS) {
        return SOURCE_TYPE_ONLY_ALIAS;
    }
    ""
}

/// tsgo `startsWithQuote`.
fn starts_with_quote(text: &str) -> bool {
    text.starts_with('"') || text.starts_with('\'')
}

/// tsgo `scanner.IsIdentifierText`, in JSX with `-` after the start.
pub(crate) fn is_identifier_text_of_variant(text: &str, jsx: bool) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    tsc_syntax::is_identifier_start(first)
        && chars.all(|c| tsc_syntax::is_identifier_part(c) || jsx && c == '-')
}

/// Go's `unicode.IsDigit`: a decimal digit of any script.
fn is_unicode_digit(c: char) -> bool {
    c.is_ascii_digit() || !c.is_ascii() && c.is_numeric()
}

/// tsgo `generateIdentifierForArbitraryString`: `(example, text)` is
/// `_example_text_`.
fn generate_identifier_for_arbitrary_string(text: &str) -> String {
    let mut needs_underscore = false;
    let mut identifier = String::new();
    for (index, c) in text.char_indices() {
        let valid_char = if index == 0 {
            tsc_syntax::is_identifier_start(c)
        } else {
            tsc_syntax::is_identifier_part(c)
        };
        if valid_char {
            if needs_underscore {
                identifier.push('_');
            }
            identifier.push(c);
            needs_underscore = false;
        } else {
            needs_underscore = true;
        }
    }
    if needs_underscore {
        identifier.push('_');
    }
    // `_` for empty text.
    if identifier.is_empty() {
        "_".to_owned()
    } else {
        identifier
    }
}

/// Go's `encoding/json` encoding of a string: `"`, `\\`, the short control
/// escapes, `\u00XX` for other controls and for `<`, `>` and `&`, U+2028
/// and U+2029 escaped, and U+FFFD for an unpaired surrogate.
fn go_json_quote(units: impl Iterator<Item = Result<char, ()>>) -> String {
    use std::fmt::Write as _;
    let mut quoted = String::from("\"");
    for unit in units {
        match unit {
            Ok('"') => quoted.push_str("\\\""),
            Ok('\\') => quoted.push_str("\\\\"),
            Ok('\u{8}') => quoted.push_str("\\b"),
            Ok('\u{c}') => quoted.push_str("\\f"),
            Ok('\n') => quoted.push_str("\\n"),
            Ok('\r') => quoted.push_str("\\r"),
            Ok('\t') => quoted.push_str("\\t"),
            Ok(ch @ ('<' | '>' | '&' | '\u{2028}' | '\u{2029}' | '\0'..='\u{1f}')) => {
                let _ = write!(quoted, "\\u{:04x}", u32::from(ch));
            }
            Ok(ch) => quoted.push(ch),
            Err(()) => quoted.push_str("\\ufffd"),
        }
    }
    quoted.push('"');
    quoted
}

/// tsgo `escapeSnippetText`.
pub(crate) fn escape_snippet_text(text: &str) -> String {
    text.replace('$', "\\$")
}
