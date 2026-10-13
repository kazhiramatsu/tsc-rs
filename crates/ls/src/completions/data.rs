//! tsgo `getCompletionData`: what the completion at a position offers (the
//! symbols, keywords and literals) and the facts its items need.

use std::collections::{HashMap, HashSet};

use tsc_binder::node_util;
use tsc_checker::state::CheckResult;
use tsc_lsproto::{CompletionItem, CompletionItemKind, Range};
use tsc_syntax::{identifier_to_keyword_kind, NodeData, NodeId, SyntaxKind};
use tsc_types::{LiteralValue, NodeFlags, SymbolFlags, SymbolId, TypeData, TypeFlags, TypeId};

use crate::utilities::Touching;

use super::context::{is_class_like, Completions};
use super::keywords::{is_non_contextual_keyword, keyword_completions};
use super::{
    default_commit_characters, sort_text, CompletionKind, KeywordCompletionFilters,
    ALL_COMMIT_CHARACTERS,
};

/// tsgo `symbolOriginInfoKind`.
pub(crate) mod origin_kind {
    pub(crate) const THIS_TYPE: u32 = 1 << 0;
    pub(crate) const SYMBOL_MEMBER: u32 = 1 << 1;
    pub(crate) const PROMISE: u32 = 1 << 2;
    pub(crate) const NULLABLE: u32 = 1 << 3;
    pub(crate) const TYPE_ONLY_ALIAS: u32 = 1 << 4;
    pub(crate) const OBJECT_LITERAL_METHOD: u32 = 1 << 5;
    pub(crate) const IGNORE: u32 = 1 << 6;
    pub(crate) const COMPUTED_PROPERTY_NAME: u32 = 1 << 7;
}

/// tsgo `symbolOriginInfo`.
#[derive(Clone, Debug, Default)]
pub(crate) struct SymbolOriginInfo {
    pub(crate) kind: u32,
    /// A computed property name's symbol name (tsgo
    /// `symbolOriginInfoComputedPropertyName`).
    pub(crate) symbol_name: Option<String>,
}

impl SymbolOriginInfo {
    fn of_kind(kind: u32) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }
}

pub(crate) fn origin_is(origin: Option<&SymbolOriginInfo>, kind: u32) -> bool {
    origin.is_some_and(|origin| origin.kind & kind != 0)
}

/// tsgo `getNullableSymbolOriginInfoKind`.
fn nullable_origin_kind(kind: u32, insert_question_dot: bool) -> u32 {
    if insert_question_dot {
        kind | origin_kind::NULLABLE
    } else {
        kind
    }
}

/// tsgo `importStatementCompletionInfo`.
#[derive(Clone, Debug, Default)]
pub(crate) struct ImportStatementCompletionInfo {
    pub(crate) is_keyword_only_completion: bool,
    pub(crate) keyword_completion: Option<SyntaxKind>,
    pub(crate) is_new_identifier_location: bool,
    pub(crate) is_top_level_type_only: bool,
    pub(crate) could_be_type_only_import_specifier: bool,
    pub(crate) replacement_span: Option<Range>,
}

/// tsgo `jsxInitializer`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct JsxInitializer {
    pub(crate) is_initializer: bool,
    pub(crate) initializer: Option<NodeId>,
}

/// tsgo `completionDataData`.
pub(crate) struct CompletionDataData {
    pub(crate) symbols: Vec<SymbolId>,
    pub(crate) completion_kind: CompletionKind,
    pub(crate) property_access_to_convert: Option<NodeId>,
    pub(crate) is_new_identifier_location: bool,
    pub(crate) location: Touching,
    pub(crate) keyword_filters: KeywordCompletionFilters,
    pub(crate) literals: Vec<LiteralValue>,
    pub(crate) symbol_to_origin_info_map: HashMap<usize, SymbolOriginInfo>,
    pub(crate) symbol_to_sort_text_map: HashMap<SymbolId, String>,
    pub(crate) recommended_completion: Option<SymbolId>,
    pub(crate) context_token: Option<Touching>,
    pub(crate) jsx_initializer: JsxInitializer,
    pub(crate) inside_js_doc_tag_type_expression: bool,
    pub(crate) is_type_only_location: bool,
    pub(crate) is_jsx_identifier_expected: bool,
    pub(crate) is_right_of_open_tag: bool,
    pub(crate) has_unresolved_auto_imports: bool,
    pub(crate) default_commit_characters: Vec<String>,
}

/// tsgo `completionData`.
pub(crate) enum CompletionData {
    Data(Box<CompletionDataData>),
    Keyword {
        keyword_completions: Vec<CompletionItem>,
        is_new_identifier_location: bool,
    },
    JSDocTagName,
    JSDocTag,
    /// The `@param` tag whose name the position is in.
    JSDocParameterName(NodeId),
}

/// tsgo `globalsSearch`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GlobalsSearch {
    Continue,
    Success,
    Fail,
}

/// One of `tryGetGlobalSymbols`' searches.
type GlobalsSearchFn<'r, 's, 'a> =
    fn(&mut Completions<'r, 's, 'a>, &Location, &mut Collector) -> CheckResult<GlobalsSearch>;

/// tsgo `keywordCompletionData`.
fn keyword_completion_data(
    keyword_filters: KeywordCompletionFilters,
    filter_out_ts_only_keywords: bool,
    is_new_identifier_location: bool,
) -> CompletionData {
    CompletionData::Keyword {
        keyword_completions: keyword_completions(keyword_filters, filter_out_ts_only_keywords),
        is_new_identifier_location,
    }
}

/// tsgo `keywordFiltersFromSyntaxKind`.
fn keyword_filters_from_syntax_kind(keyword: SyntaxKind) -> KeywordCompletionFilters {
    match keyword {
        SyntaxKind::TypeKeyword => KeywordCompletionFilters::TypeKeyword,
        _ => panic!("Unknown mapping from ast.Kind `{keyword:?}` to KeywordCompletionFilters"),
    }
}

/// The state `getCompletionData`'s closures share.
pub(super) struct Collector {
    pub(super) symbols: Vec<SymbolId>,
    pub(super) symbol_to_origin_info_map: HashMap<usize, SymbolOriginInfo>,
    pub(super) symbol_to_sort_text_map: HashMap<SymbolId, String>,
    pub(super) seen_property_symbols: HashSet<SymbolId>,
    pub(super) completion_kind: CompletionKind,
    pub(super) is_new_identifier_location: bool,
    pub(super) keyword_filters: KeywordCompletionFilters,
    pub(super) default_commit_characters: Option<Vec<String>>,
}

/// The facts about the location `getCompletionData` computes before it
/// collects symbols.
pub(super) struct Location {
    pub(super) context_token: Option<Touching>,
    pub(super) previous_token: Option<Touching>,
    pub(super) node: Option<Touching>,
    pub(super) location: Touching,
    pub(super) is_right_of_dot: bool,
    pub(super) is_right_of_question_dot: bool,
    pub(super) import_statement_completion: Option<ImportStatementCompletionInfo>,
    pub(super) is_type_only_location: bool,
    pub(super) inside_js_doc_tag_type_expression: bool,
    pub(super) in_checked_file: bool,
}

impl Completions<'_, '_, '_> {
    /// tsgo `getCompletionData`.
    pub(crate) fn get_completion_data(&mut self) -> CheckResult<Option<CompletionData>> {
        let position = self.position;
        let in_checked_file = self.is_checked_file();
        let mut current_token = self.token_at(position);
        let preceding = self.preceding_token(position);
        let inside_comment = self.enclosing_comment(position, preceding, current_token);

        let mut inside_js_doc_tag_type_expression = false;
        let mut inside_js_doc_import_tag = false;
        if inside_comment.is_some() {
            if self.has_doc_comment(position) {
                if position > 0 && self.text().as_bytes()[position as usize - 1] == b'@' {
                    // Next to `@` with no tag name yet: every tag name.
                    return Ok(Some(CompletionData::JSDocTagName));
                }
                // Without `@`, only `*`, space and a few punctuators may
                // come before the position on its line.
                let line_start = self.line_start_of(position);
                let no_comment_prefix = self.text()[line_start as usize..position as usize]
                    .chars()
                    .all(|c| {
                        is_white_space_single_line(c) || matches!(c, '*' | '/' | '(' | ')' | '|')
                    });
                if no_comment_prefix {
                    return Ok(Some(CompletionData::JSDocTag));
                }
            }
            // Completions work inside some JSDoc tags, as in the braces of
            // `/** @type {number | string} */`.
            if let Some(tag) = self.js_doc_tag_at_position(current_token, position) {
                let tag_name = self.js_doc_tag_name(tag);
                if tag_name.is_some_and(|name| {
                    let record = self.record(name);
                    record.pos <= position && position <= record.end
                }) {
                    return Ok(Some(CompletionData::JSDocTagName));
                }
                if self.node_kind(tag) == SyntaxKind::JSDocImportTag {
                    inside_js_doc_import_tag = true;
                } else {
                    if let Some(type_expression) = self.type_expression_from_tag(tag) {
                        current_token = self.token_at(position);
                        let in_name = match current_token {
                            Touching::Node(node) => {
                                self.is_declaration_name(current_token)
                                    || self.parent_of(node).is_some_and(|parent| {
                                        self.node_kind(parent) == SyntaxKind::JSDocPropertyTag
                                            && self.name_of(parent) == Some(node)
                                    })
                            }
                            Touching::Token { .. } => false,
                        };
                        if !in_name {
                            // A type location inside the tag's type.
                            inside_js_doc_tag_type_expression = self.is_currently_editing_node(
                                Touching::Node(type_expression),
                                position,
                            );
                        }
                    }
                    if !inside_js_doc_tag_type_expression
                        && self.node_kind(tag) == SyntaxKind::JSDocParameterTag
                    {
                        let name = self.name_of(tag);
                        if self.node_is_missing(name)
                            || name.is_some_and(|name| {
                                let record = self.record(name);
                                record.pos <= position && position <= record.end
                            })
                        {
                            return Ok(Some(CompletionData::JSDocParameterName(tag)));
                        }
                    }
                }
            }
            if !inside_js_doc_tag_type_expression && !inside_js_doc_import_tag {
                // A plain comment, or the text of a JSDoc comment.
                return Ok(None);
            }
        }

        // The context token decides; it and the previous token are none at
        // the start of the file.
        let is_js_only_location = !inside_js_doc_tag_type_expression
            && !inside_js_doc_import_tag
            && self.is_source_file_js();
        let (mut context_token, previous_token) = self.relevant_tokens(position);

        let mut node = Some(current_token);
        let mut property_access_to_convert = None;
        let mut is_right_of_dot = false;
        let mut is_right_of_question_dot = false;
        let mut is_right_of_open_tag = false;
        let mut is_starting_close_tag = false;
        let mut jsx_initializer = JsxInitializer::default();
        let mut is_jsx_identifier_expected = false;
        let mut import_statement_completion = None;
        let mut location = self.touching_property_name(position);
        let mut keyword_filters = KeywordCompletionFilters::None;
        let mut is_new_identifier_location = false;
        let initial_commit_characters: Option<Vec<String>> = None;

        if let Some(context) = context_token {
            let info = self.import_statement_completion_info(context);
            if let Some(keyword) = info.keyword_completion {
                if info.is_keyword_only_completion {
                    return Ok(Some(CompletionData::Keyword {
                        keyword_completions: vec![CompletionItem {
                            label: tsc_syntax::tokens::token_to_string(keyword)
                                .expect("a keyword")
                                .to_owned(),
                            kind: Some(CompletionItemKind::KEYWORD),
                            sort_text: Some(sort_text::GLOBALS_OR_KEYWORDS.to_owned()),
                            ..CompletionItem::default()
                        }],
                        is_new_identifier_location: info.is_new_identifier_location,
                    }));
                }
                keyword_filters = keyword_filters_from_syntax_kind(keyword);
            }
            if info.replacement_span.is_some()
                && self
                    .request
                    .preferences
                    .completion_preferences
                    .include_completions_for_import_statements
                    == Some(true)
            {
                is_new_identifier_location = info.is_new_identifier_location;
                import_statement_completion = Some(info.clone());
            }
            // A known place for no completions.
            if info.replacement_span.is_none()
                && self.is_completion_list_blocker(context, previous_token, location, position)?
            {
                if keyword_filters != KeywordCompletionFilters::None {
                    let (is_new_identifier_location, _) = self
                        .compute_commit_characters_and_is_new_identifier(Some(context), position);
                    return Ok(Some(keyword_completion_data(
                        keyword_filters,
                        is_js_only_location,
                        is_new_identifier_location,
                    )));
                }
                return Ok(None);
            }

            let mut parent = self.parent(context);
            let context_kind = self.kind(context);
            if matches!(
                context_kind,
                SyntaxKind::DotToken | SyntaxKind::QuestionDotToken
            ) {
                is_right_of_dot = context_kind == SyntaxKind::DotToken;
                is_right_of_question_dot = context_kind == SyntaxKind::QuestionDotToken;
                let parent = parent.expect("a dot's parent");
                match self.data(parent) {
                    NodeData::PropertyAccessExpression(data) => {
                        property_access_to_convert = Some(parent);
                        let expression = data.expression.expect("a property access's expression");
                        node = Some(Touching::Node(expression));
                        let left_most = self.leftmost_access_expression(parent);
                        let expression_kind = self.node_kind(expression);
                        if self.node_is_missing(Some(left_most))
                            || ((expression_kind == SyntaxKind::CallExpression
                                || crate::utilities::is_function_like_kind(expression_kind))
                                && self.record(expression).end == self.pos(context)
                                && self.last_child(expression).is_none_or(|last| {
                                    self.kind(last) != SyntaxKind::CloseParenToken
                                }))
                        {
                            // A dot of an expression read wrongly, as the
                            // start of a spread: `Math.min(./**/)`,
                            // `const x = function (./**/) {}`, `({./**/})`.
                            return Ok(None);
                        }
                    }
                    NodeData::QualifiedName(data) => {
                        node = data.left.map(Touching::Node);
                    }
                    NodeData::ModuleDeclaration(data) => {
                        node = data.name.map(Touching::Node);
                    }
                    NodeData::ImportType(_) => {
                        node = Some(Touching::Node(parent));
                    }
                    NodeData::MetaProperty(_) => {
                        let first = self.first_token(parent).expect("a meta property's keyword");
                        let kind = self.kind(first);
                        if kind != SyntaxKind::ImportKeyword && kind != SyntaxKind::NewKeyword {
                            panic!("Unexpected token kind: {kind:?}");
                        }
                        node = Some(first);
                    }
                    // Nothing before the dot: a stray character, or the
                    // start of `...`.
                    _ => return Ok(None),
                }
            } else if import_statement_completion.is_none() {
                // In `<UI.Test /* completion position */ />`, the JSX
                // container is above the whole property access.
                if let Some(access) = parent.filter(|&parent| {
                    self.node_kind(parent) == SyntaxKind::PropertyAccessExpression
                }) {
                    context_token = Some(Touching::Node(access));
                    parent = self.parent_of(access);
                }
                let context = context_token.expect("a context token");
                // The location fixed.
                if parent.map(Touching::Node) == Some(location) {
                    let parent_kind = parent.map(|parent| self.node_kind(parent));
                    match self.kind(current_token) {
                        SyntaxKind::GreaterThanToken => {
                            if matches!(
                                parent_kind,
                                Some(SyntaxKind::JsxElement | SyntaxKind::JsxOpeningElement)
                            ) {
                                location = current_token;
                            }
                        }
                        SyntaxKind::LessThanSlashToken => {
                            if parent_kind == Some(SyntaxKind::JsxSelfClosingElement) {
                                location = current_token;
                            }
                        }
                        _ => {}
                    }
                }
                let parent = parent.expect("a context token's parent");
                match self.node_kind(parent) {
                    SyntaxKind::JsxClosingElement => {
                        if self.kind(context) == SyntaxKind::LessThanSlashToken {
                            is_starting_close_tag = true;
                            location = context;
                        }
                    }
                    SyntaxKind::BinaryExpression
                        if !self.binary_expression_may_be_open_tag(parent) => {}
                    SyntaxKind::BinaryExpression
                    | SyntaxKind::JsxSelfClosingElement
                    | SyntaxKind::JsxElement
                    | SyntaxKind::JsxOpeningElement => {
                        is_jsx_identifier_expected = true;
                        if self.kind(context) == SyntaxKind::LessThanToken {
                            is_right_of_open_tag = true;
                            location = context;
                        }
                    }
                    SyntaxKind::JsxExpression | SyntaxKind::JsxSpreadAttribute => {
                        // `<div foo={true} [||] />`, where the previous
                        // token is `}`, or `<div foo={true} t[||] ></div>`
                        // (not `<div foo={undefine[||]}></div>`).
                        let previous = previous_token.expect("a previous token");
                        if self.kind(previous) == SyntaxKind::CloseBraceToken
                            || self.kind(previous) == SyntaxKind::Identifier
                                && self.parent(previous).is_some_and(|node| {
                                    self.node_kind(node) == SyntaxKind::JsxAttribute
                                })
                        {
                            is_jsx_identifier_expected = true;
                        }
                    }
                    SyntaxKind::JsxAttribute => {
                        let previous = previous_token.expect("a previous token");
                        let initializer = self.initializer_of(parent);
                        // `<div className="x" [||] ></div>`
                        if initializer.map(Touching::Node) == Some(previous)
                            && self.end(previous) < position
                        {
                            is_jsx_identifier_expected = true;
                        } else {
                            match self.kind(previous) {
                                SyntaxKind::EqualsToken => jsx_initializer.is_initializer = true,
                                SyntaxKind::Identifier => {
                                    is_jsx_identifier_expected = true;
                                    // In `<div x=[|f/**/|]`, `f` is its own
                                    // attribute; `<div someBool f>` is an
                                    // attribute name.
                                    if Some(parent) != self.parent(previous)
                                        && initializer.is_none()
                                        && self
                                            .child_of_kind(parent, SyntaxKind::EqualsToken)
                                            .is_some()
                                    {
                                        if let Touching::Node(previous) = previous {
                                            jsx_initializer.initializer = Some(previous);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let is_type_only_location = inside_js_doc_tag_type_expression
            || inside_js_doc_import_tag
            || import_statement_completion.is_some()
                && self
                    .parent(location)
                    .is_some_and(|parent| self.is_type_only_import_or_export_declaration(parent))
            || !self.is_context_token_value_location(context_token)
                && (self.is_possibly_type_argument_position(context_token)?
                    || self.is_part_of_type_node(location)
                    || self.is_context_token_type_location(context_token));

        let mut collector = Collector {
            symbols: Vec::new(),
            symbol_to_origin_info_map: HashMap::new(),
            symbol_to_sort_text_map: HashMap::new(),
            seen_property_symbols: HashSet::new(),
            completion_kind: CompletionKind::None,
            is_new_identifier_location,
            keyword_filters,
            default_commit_characters: initial_commit_characters,
        };
        let facts = Location {
            context_token,
            previous_token,
            node,
            location,
            is_right_of_dot,
            is_right_of_question_dot,
            import_statement_completion,
            is_type_only_location,
            inside_js_doc_tag_type_expression,
            in_checked_file,
        };

        if is_right_of_dot || is_right_of_question_dot {
            self.get_type_script_member_symbols(&facts, &mut collector)?;
        } else if is_right_of_open_tag {
            let tag_location = match location {
                Touching::Node(node) => node,
                Touching::Token { parent, .. } => parent,
            };
            collector.symbols = self
                .request
                .state
                .get_jsx_intrinsic_tag_names_at(tag_location)?;
            self.try_get_global_symbols(&facts, &mut collector)?;
            collector.completion_kind = CompletionKind::Global;
            collector.keyword_filters = KeywordCompletionFilters::None;
        } else if is_starting_close_tag {
            let context = facts.context_token.expect("a context token");
            let element = self
                .parent(context)
                .and_then(|parent| self.parent_of(parent))
                .expect("a closing element's element");
            let NodeData::JsxElement(data) = self.data(element) else {
                panic!("a JSX element");
            };
            let opening = data.opening_element.expect("an opening element");
            let tag_name = match self.data(opening) {
                NodeData::JsxOpeningElement(data) => data.tag_name,
                _ => None,
            }
            .expect("an opening element's tag name");
            if let Some(tag_symbol) = self.request.state.get_symbol_at_location(tag_name)? {
                collector.symbols = vec![tag_symbol];
            }
            collector.completion_kind = CompletionKind::Global;
            collector.keyword_filters = KeywordCompletionFilters::None;
        } else if !self.try_get_global_symbols(&facts, &mut collector)? {
            // Not after a dot: the global symbols in scope, valid in
            // JavaScript and TypeScript alike.
            if collector.keyword_filters != KeywordCompletionFilters::None {
                return Ok(Some(keyword_completion_data(
                    collector.keyword_filters,
                    is_js_only_location,
                    collector.is_new_identifier_location,
                )));
            }
            return Ok(None);
        }

        let contextual_type_or_constraint = match previous_token {
            Some(previous) => match self.contextual_type_at(previous, position)? {
                Some(ty) => Some(ty),
                None => self.constraint_of_type_argument_property(previous)?,
            },
            None => None,
        };

        // No literal after `<input type="text" [||] />`
        // (microsoft/TypeScript#51667) or after a closing quote
        // (microsoft/TypeScript#52675); string completions handle strings.
        let is_literal_expected = !previous_token
            .is_some_and(|previous| super::context::is_string_literal_like(self.kind(previous)))
            && !is_jsx_identifier_expected;
        let mut literals = Vec::new();
        if is_literal_expected {
            if let Some(ty) = contextual_type_or_constraint {
                let types = if self
                    .request
                    .state
                    .tables
                    .flags_of(ty)
                    .intersects(TypeFlags::UNION)
                {
                    self.union_types(ty)
                } else {
                    vec![ty]
                };
                for ty in types {
                    let flags = self.request.state.tables.flags_of(ty);
                    if flags.intersects(
                        TypeFlags::STRING_LITERAL
                            | TypeFlags::NUMBER_LITERAL
                            | TypeFlags::BIG_INT_LITERAL,
                    ) && !flags.intersects(TypeFlags::ENUM_LITERAL)
                    {
                        if let Some(value) = self.literal_value(ty) {
                            literals.push(value);
                        }
                    }
                }
            }
        }

        let recommended_completion = match (previous_token, contextual_type_or_constraint) {
            (Some(previous), Some(ty)) => self.recommended_completion(previous, ty)?,
            _ => None,
        };

        let default_commit_characters = collector
            .default_commit_characters
            .unwrap_or_else(|| default_commit_characters(collector.is_new_identifier_location));

        Ok(Some(CompletionData::Data(Box::new(CompletionDataData {
            symbols: collector.symbols,
            completion_kind: collector.completion_kind,
            property_access_to_convert,
            is_new_identifier_location: collector.is_new_identifier_location,
            location: facts.location,
            keyword_filters: collector.keyword_filters,
            literals,
            symbol_to_origin_info_map: collector.symbol_to_origin_info_map,
            symbol_to_sort_text_map: collector.symbol_to_sort_text_map,
            recommended_completion,
            context_token: facts.context_token,
            jsx_initializer,
            inside_js_doc_tag_type_expression,
            is_type_only_location,
            is_jsx_identifier_expected,
            is_right_of_open_tag,
            has_unresolved_auto_imports: false,
            default_commit_characters,
        }))))
    }

    /// tsgo `getTypeScriptMemberSymbols`: the members right of a dot.
    fn get_type_script_member_symbols(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
    ) -> CheckResult<()> {
        collector.completion_kind = CompletionKind::PropertyAccess;
        let Some(node) = facts.node else {
            return Ok(());
        };
        let Touching::Node(node_id) = node else {
            return Ok(());
        };
        let node_kind = self.node_kind(node_id);
        let node_parent = self.parent_of(node_id);
        // A qualified name is a type location.
        let is_import_type = self.is_literal_import_type_node(node_id);
        let is_type_location = (is_import_type
            && !matches!(self.data(node_id), NodeData::ImportType(data) if data.is_type_of))
            || node_parent.is_some_and(|parent| self.request.state.is_part_of_type_node(parent))
            || self.is_possibly_type_argument_position(facts.context_token)?;
        let is_rhs_of_import_declaration =
            self.is_in_right_side_of_internal_import_equals_declaration(node_id);
        if matches!(
            node_kind,
            SyntaxKind::Identifier
                | SyntaxKind::QualifiedName
                | SyntaxKind::PropertyAccessExpression
        ) || is_import_type
        {
            let is_namespace_name = node_parent
                .is_some_and(|parent| self.node_kind(parent) == SyntaxKind::ModuleDeclaration);
            if is_namespace_name {
                collector.is_new_identifier_location = true;
                collector.default_commit_characters = Some(Vec::new());
            }
            if let Some(symbol) = self.request.state.get_symbol_at_location(node_id)? {
                let symbol = self.request.state.skip_alias(symbol)?;
                if self
                    .request
                    .state
                    .symbol_flags(symbol)
                    .intersects(SymbolFlags::MODULE | SymbolFlags::ENUM)
                {
                    let value_access_node = if is_import_type {
                        node_id
                    } else {
                        node_parent.expect("a member access's parent")
                    };
                    // The module's or enum's members.
                    let exported_symbols =
                        self.request.state.get_exports_of_module_symbols(symbol)?;
                    for exported_symbol in exported_symbols {
                        let is_valid_access = if is_namespace_name {
                            // At `namespace N.M/**/`, `M` is not offered
                            // when this is its only declaration.
                            self.request
                                .state
                                .symbol_flags(exported_symbol)
                                .intersects(SymbolFlags::NAMESPACE)
                                && !self
                                    .declarations_of(exported_symbol)
                                    .iter()
                                    .all(|&declaration| self.parent_of(declaration) == node_parent)
                        } else if is_rhs_of_import_declaration {
                            // Any kind after a namespace in an internal
                            // import equals declaration.
                            self.symbol_can_be_referenced_at_type_location(
                                exported_symbol,
                                &mut HashSet::new(),
                            )? || self.is_valid_value_access(value_access_node, exported_symbol)?
                        } else if is_type_location || facts.inside_js_doc_tag_type_expression {
                            self.symbol_can_be_referenced_at_type_location(
                                exported_symbol,
                                &mut HashSet::new(),
                            )?
                        } else {
                            self.is_valid_value_access(value_access_node, exported_symbol)?
                        };
                        if is_valid_access {
                            collector.symbols.push(exported_symbol);
                        }
                    }
                    // A module merged with a value: the class's properties
                    // too (inherited static methods).
                    if !is_type_location
                        && !facts.inside_js_doc_tag_type_expression
                        && self.declarations_of(symbol).iter().any(|&declaration| {
                            !matches!(
                                self.request.state.binder.node_record(declaration).kind,
                                SyntaxKind::SourceFile
                                    | SyntaxKind::ModuleDeclaration
                                    | SyntaxKind::EnumDeclaration
                            )
                        })
                    {
                        let ty = self
                            .request
                            .state
                            .get_type_of_symbol_at_location(symbol, Some(node_id))?;
                        let mut ty = self.request.state.get_non_optional_type(ty);
                        let mut insert_question_dot = false;
                        if self.request.state.is_nullable_type(ty)? {
                            let can_correct_to_question_dot = facts.is_right_of_dot
                                && !facts.is_right_of_question_dot
                                && self
                                    .request
                                    .preferences
                                    .completion_preferences
                                    .include_automatic_optional_chain_completions
                                    != Some(false);
                            if can_correct_to_question_dot || facts.is_right_of_question_dot {
                                ty = self.request.state.get_non_nullable_type(ty)?;
                                if can_correct_to_question_dot {
                                    insert_question_dot = true;
                                }
                            }
                        }
                        let insert_await =
                            self.record(node_id).flags & NodeFlags::AWAIT_CONTEXT.bits() != 0;
                        self.add_type_properties(
                            facts,
                            collector,
                            node_id,
                            ty,
                            insert_await,
                            insert_question_dot,
                        )?;
                    }
                    return Ok(());
                }
            }
        }

        if !is_type_location || self.request.state.is_in_type_query(node_id) {
            // microsoft/TypeScript#39946: the type of `this` first, as the
            // compiler checks it, so a contextual `this` parameter makes no
            // circularity.
            self.request
                .state
                .try_get_this_type_at_ex(node_id, false, None)?;
            let ty = self.request.state.get_type_at_location(node_id)?;
            let mut ty = self.request.state.get_non_optional_type(ty);
            if !is_type_location {
                let mut insert_question_dot = false;
                if self.request.state.is_nullable_type(ty)? {
                    let can_correct_to_question_dot = facts.is_right_of_dot
                        && !facts.is_right_of_question_dot
                        && self
                            .request
                            .preferences
                            .completion_preferences
                            .include_automatic_optional_chain_completions
                            != Some(false);
                    if can_correct_to_question_dot || facts.is_right_of_question_dot {
                        ty = self.request.state.get_non_nullable_type(ty)?;
                        if can_correct_to_question_dot {
                            insert_question_dot = true;
                        }
                    }
                }
                let insert_await =
                    self.record(node_id).flags & NodeFlags::AWAIT_CONTEXT.bits() != 0;
                self.add_type_properties(
                    facts,
                    collector,
                    node_id,
                    ty,
                    insert_await,
                    insert_question_dot,
                )?;
            } else {
                let ty = self.request.state.get_non_nullable_type(ty)?;
                self.add_type_properties(facts, collector, node_id, ty, false, false)?;
            }
        }
        Ok(())
    }

    /// The `isValidValueAccess` closure: `IsValidPropertyAccess` by the
    /// symbol's name.
    fn is_valid_value_access(&mut self, node: NodeId, symbol: SymbolId) -> CheckResult<bool> {
        let name = self.request.state.binder.symbol(symbol).escaped_name;
        self.request.state.is_valid_property_access(node, name)
    }

    /// tsgo `addTypeProperties`.
    fn add_type_properties(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
        node: NodeId,
        ty: TypeId,
        insert_await: bool,
        insert_question_dot: bool,
    ) -> CheckResult<()> {
        if self.request.state.get_string_index_type(ty)?.is_some() {
            collector.is_new_identifier_location = true;
            collector.default_commit_characters = Some(Vec::new());
        }
        if facts.is_right_of_question_dot
            && !self
                .request
                .state
                .get_signatures_of_type(ty, tsc_checker::state::SignatureKind::Call)?
                .is_empty()
        {
            collector.is_new_identifier_location = true;
            if collector.default_commit_characters.is_none() {
                // Only `(` would be wrong here.
                collector.default_commit_characters = Some(
                    ALL_COMMIT_CHARACTERS
                        .iter()
                        .map(|&c| c.to_owned())
                        .collect(),
                );
            }
        }
        let property_access = if self.node_kind(node) == SyntaxKind::ImportType {
            node
        } else {
            self.parent_of(node).expect("a member access's parent")
        };
        if facts.in_checked_file {
            for symbol in self.request.state.get_apparent_properties(ty)? {
                if self
                    .request
                    .state
                    .is_valid_property_access_for_completions(property_access, ty, symbol)?
                {
                    self.add_property_symbol(facts, collector, symbol, false, insert_question_dot)?;
                }
            }
        } else {
            // In JavaScript, a union's members include what each type has:
            // every identifier is added anyway, and these known members
            // rank higher.
            for symbol in self.properties_for_completion(ty)? {
                if self
                    .request
                    .state
                    .is_valid_property_access_for_completions(property_access, ty, symbol)?
                {
                    collector.symbols.push(symbol);
                }
            }
        }
        if insert_await {
            if let Some(promise_type) = self.request.state.get_promised_type_of_promise(ty)? {
                for symbol in self.request.state.get_apparent_properties(promise_type)? {
                    if self
                        .request
                        .state
                        .is_valid_property_access_for_completions(
                            property_access,
                            promise_type,
                            symbol,
                        )?
                    {
                        self.add_property_symbol(
                            facts,
                            collector,
                            symbol,
                            true,
                            insert_question_dot,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    /// tsgo `addPropertySymbol`: a property, or for a computed name with an
    /// accessible name like `[Symbol.iterator]`, that name's first symbol.
    fn add_property_symbol(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
        symbol: SymbolId,
        insert_await: bool,
        insert_question_dot: bool,
    ) -> CheckResult<()> {
        let computed_property_name =
            self.declarations_of(symbol)
                .into_iter()
                .find_map(|declaration| {
                    let source = self.request.state.binder.source_of_node(declaration);
                    node_util::get_name_of_declaration(source, declaration).filter(|&name| {
                        source.arena.node(name).kind == SyntaxKind::ComputedPropertyName
                    })
                });
        let Some(computed_property_name) = computed_property_name else {
            collector.symbols.push(symbol);
            self.add_symbol_origin_info(collector, symbol, insert_question_dot, insert_await);
            self.add_symbol_sort_info(collector, symbol);
            return Ok(());
        };
        // The completion is for `Symbol`, not `iterator`.
        let source = self
            .request
            .state
            .binder
            .source_of_node(computed_property_name);
        let expression = self.expression_of(computed_property_name);
        let left_most_name = expression.and_then(|expression| left_most_name(source, expression));
        let name_symbol = match left_most_name {
            Some(name) => self.request.state.get_symbol_at_location(name)?,
            None => None,
        };
        // Nested, as in `namespace N { export const sym = Symbol(); }`,
        // the completion is for `N`.
        let first_accessible_symbol = match name_symbol {
            Some(name_symbol) => self.first_symbol_in_chain(name_symbol, facts.context_token)?,
            None => None,
        };
        if let Some(first) = first_accessible_symbol {
            if collector.seen_property_symbols.insert(first) {
                collector.symbols.push(first);
                collector
                    .symbol_to_sort_text_map
                    .insert(first, sort_text::GLOBALS_OR_KEYWORDS.to_owned());
                let module_symbol = self.request.state.binder.symbol(first).parent;
                let is_module_export = match module_symbol {
                    Some(module_symbol) if self.is_external_module_symbol(module_symbol) => {
                        let name = self.symbol_name(first);
                        self.request
                            .state
                            .try_get_member_in_module_exports_and_properties(&name, module_symbol)?
                            == Some(first)
                    }
                    _ => false,
                };
                if !is_module_export {
                    let index = collector.symbols.len() - 1;
                    collector.symbol_to_origin_info_map.insert(
                        index,
                        SymbolOriginInfo::of_kind(nullable_origin_kind(
                            origin_kind::SYMBOL_MEMBER,
                            insert_question_dot,
                        )),
                    );
                }
                return Ok(());
            }
        }
        if first_accessible_symbol
            .is_none_or(|first| !collector.seen_property_symbols.contains(&first))
        {
            collector.symbols.push(symbol);
            self.add_symbol_origin_info(collector, symbol, insert_question_dot, insert_await);
            self.add_symbol_sort_info(collector, symbol);
        }
        Ok(())
    }

    /// tsgo `addSymbolOriginInfo`.
    fn add_symbol_origin_info(
        &self,
        collector: &mut Collector,
        symbol: SymbolId,
        insert_question_dot: bool,
        insert_await: bool,
    ) {
        let index = collector.symbols.len() - 1;
        if insert_await && collector.seen_property_symbols.insert(symbol) {
            collector.symbol_to_origin_info_map.insert(
                index,
                SymbolOriginInfo::of_kind(nullable_origin_kind(
                    origin_kind::PROMISE,
                    insert_question_dot,
                )),
            );
        } else if insert_question_dot {
            collector
                .symbol_to_origin_info_map
                .insert(index, SymbolOriginInfo::of_kind(origin_kind::NULLABLE));
        }
    }

    /// tsgo `addSymbolSortInfo`.
    fn add_symbol_sort_info(&self, collector: &mut Collector, symbol: SymbolId) {
        if self.is_static_property(symbol) {
            collector
                .symbol_to_sort_text_map
                .insert(symbol, sort_text::LOCAL_DECLARATION_PRIORITY.to_owned());
        }
    }

    /// tsgo `tryGetGlobalSymbols`: the searches in order, until one
    /// answers.
    fn try_get_global_symbols(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
    ) -> CheckResult<bool> {
        let searches: [GlobalsSearchFn<'_, '_, '_>; 10] = [
            Self::try_get_object_type_literal_in_type_argument_completion_symbols,
            Self::try_get_object_like_completion_symbols,
            Self::try_get_import_completion_symbols,
            Self::try_get_import_or_export_clause_completion_symbols,
            Self::try_get_import_attributes_completion_symbols,
            Self::try_get_local_named_export_completion_symbols,
            Self::try_get_constructor_completion,
            Self::try_get_class_like_completion_symbols,
            Self::try_get_jsx_completion_symbols,
            Self::get_global_completions,
        ];
        let mut result = GlobalsSearch::Continue;
        for search in searches {
            result = search(self, facts, collector)?;
            if result != GlobalsSearch::Continue {
                break;
            }
        }
        Ok(result == GlobalsSearch::Success)
    }

    /// tsgo `tryGetImportCompletionSymbols`: the auto-imports of an import
    /// statement being written (L5).
    fn try_get_import_completion_symbols(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
    ) -> CheckResult<GlobalsSearch> {
        if facts.import_statement_completion.is_none() {
            return Ok(GlobalsSearch::Continue);
        }
        collector.is_new_identifier_location = true;
        Ok(GlobalsSearch::Success)
    }

    /// tsgo `tryGetConstructorCompletion`: only keywords at a constructor
    /// parameter.
    fn try_get_constructor_completion(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
    ) -> CheckResult<GlobalsSearch> {
        if self
            .try_get_constructor_like_completion_container(facts.context_token)
            .is_none()
        {
            return Ok(GlobalsSearch::Continue);
        }
        // No members, only keywords, at a new property, method or accessor.
        collector.completion_kind = CompletionKind::None;
        collector.is_new_identifier_location = true;
        collector.keyword_filters = KeywordCompletionFilters::ConstructorParameterKeywords;
        Ok(GlobalsSearch::Success)
    }

    /// tsgo `getGlobalCompletions`: every entity in the scope.
    fn get_global_completions(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
    ) -> CheckResult<GlobalsSearch> {
        collector.keyword_filters = if self
            .try_get_function_like_body_completion_container(facts.context_token)
            .is_some()
        {
            KeywordCompletionFilters::FunctionLikeBodyKeywords
        } else {
            KeywordCompletionFilters::All
        };
        collector.completion_kind = CompletionKind::Global;
        let (is_new_identifier_location, default_commit_characters) = self
            .compute_commit_characters_and_is_new_identifier(facts.context_token, self.position);
        collector.is_new_identifier_location = is_new_identifier_location;
        collector.default_commit_characters = Some(default_commit_characters);

        // At the end of an identifier, the scope is the identifier's: a
        // completion there is one at its start (`xyz => x$` is
        // `xyz => $x`). The context token is then before the previous
        // token.
        let adjusted_position = if facts.previous_token != facts.context_token {
            self.start(facts.previous_token.expect(
                "Expected 'contextToken' to be defined when different from 'previousToken'.",
            ))
        } else {
            self.position
        };
        let scope_node = self
            .scope_node(facts.context_token, adjusted_position)
            .unwrap_or(self.source.root);

        let symbol_meanings = if facts.is_type_only_location {
            SymbolFlags::NONE
        } else {
            SymbolFlags::VALUE
        } | SymbolFlags::TYPE
            | SymbolFlags::NAMESPACE
            | SymbolFlags::ALIAS;
        let type_only_alias_needs_promotion = facts
            .previous_token
            .is_some_and(|previous| !self.is_valid_type_only_alias_use_site(previous));

        let in_scope = self
            .request
            .state
            .get_symbols_in_scope(scope_node, symbol_meanings)?;
        collector.symbols.extend(in_scope);
        for index in 0..collector.symbols.len() {
            let symbol = collector.symbols[index];
            let declarations = self.declarations_of(symbol);
            if !self.request.state.is_arguments_symbol(symbol)
                && !declarations.iter().any(|&declaration| {
                    self.request.state.binder.file_index_of_node(declaration) == self.file
                })
            {
                collector
                    .symbol_to_sort_text_map
                    .insert(symbol, sort_text::GLOBALS_OR_KEYWORDS.to_owned());
            }
            if type_only_alias_needs_promotion
                && !self
                    .request
                    .state
                    .symbol_flags(symbol)
                    .intersects(SymbolFlags::VALUE)
                && declarations
                    .iter()
                    .any(|&declaration| self.is_type_only_import_declaration(declaration))
            {
                collector.symbol_to_origin_info_map.insert(
                    index,
                    SymbolOriginInfo::of_kind(origin_kind::TYPE_ONLY_ALIAS),
                );
            }
        }

        // `this.` goes before the properties of the `this` type.
        if self.node_kind(scope_node) != SyntaxKind::SourceFile {
            let container = self
                .parent_of(scope_node)
                .filter(|&parent| is_class_like(self.node_kind(parent)))
                .map(|_| scope_node);
            let this_type = self
                .request
                .state
                .try_get_this_type_at_ex(scope_node, false, container)?;
            if let Some(this_type) = this_type {
                if !self.is_probably_global_type(this_type)? {
                    for symbol in self.properties_for_completion(this_type)? {
                        collector.symbols.push(symbol);
                        let index = collector.symbols.len() - 1;
                        collector
                            .symbol_to_origin_info_map
                            .insert(index, SymbolOriginInfo::of_kind(origin_kind::THIS_TYPE));
                        collector
                            .symbol_to_sort_text_map
                            .insert(symbol, sort_text::SUGGESTED_CLASS_MEMBERS.to_owned());
                    }
                }
            }
        }

        if facts.is_type_only_location {
            collector.keyword_filters = if facts.context_token.is_some_and(|context| {
                self.parent(context).is_some_and(|parent| {
                    matches!(
                        self.node_kind(parent),
                        SyntaxKind::TypeAssertionExpression | SyntaxKind::AsExpression
                    )
                })
            }) {
                KeywordCompletionFilters::TypeAssertionKeywords
            } else {
                KeywordCompletionFilters::TypeKeywords
            };
        }
        Ok(GlobalsSearch::Success)
    }
}

/// Small helpers of `getCompletionData`.
impl Completions<'_, '_, '_> {
    /// tsgo `GetTokenAtPosition`.
    pub(super) fn token_at(&self, position: u32) -> Touching {
        crate::utilities::touching(self.navigator.token_at(position))
            .unwrap_or(Touching::Node(self.source.root))
    }

    /// tsgo `GetTouchingPropertyName`.
    fn touching_property_name(&self, position: u32) -> Touching {
        crate::utilities::touching(self.navigator.touching_property_name(position))
            .unwrap_or(Touching::Node(self.source.root))
    }

    /// tsgo `isCheckedFile`.
    fn is_checked_file(&self) -> bool {
        // tsgo `ast.IsCheckJSEnabledForFile`: the file's `@ts-check` or
        // `@ts-nocheck`, else `checkJs`.
        !self.is_source_file_js()
            || match tsc_checker::check_directive(self.text()) {
                Some(tsc_checker::CheckDirective::Check) => true,
                Some(tsc_checker::CheckDirective::NoCheck) => false,
                None => self.request.state.options.check_js == Some(true),
            }
    }

    /// tsgo `ast.IsSourceFileJS`.
    pub(crate) fn is_source_file_js(&self) -> bool {
        self.record(self.source.root).flags & NodeFlags::JAVA_SCRIPT_FILE.bits() != 0
    }

    /// tsgo `hasDocComment`: the token at the position is in a JSDoc
    /// comment.
    fn has_doc_comment(&self, position: u32) -> bool {
        let token = self.token_at(position);
        let start = match token {
            Touching::Node(node) => Some(node),
            Touching::Token { parent, .. } => Some(parent),
        };
        self.find_ancestor(start, |this, node| {
            this.node_kind(node) == SyntaxKind::JSDoc
        })
        .is_some()
    }

    /// tsgo `getJSDocTagAtPosition`.
    fn js_doc_tag_at_position(&self, token: Touching, position: u32) -> Option<NodeId> {
        let mut current = match token {
            Touching::Node(node) => Some(node),
            Touching::Token { parent, .. } => Some(parent),
        };
        while let Some(node) = current {
            let record = self.record(node);
            if is_js_doc_tag_kind(record.kind) && record.pos <= position && position <= record.end {
                return Some(node);
            }
            if record.kind == SyntaxKind::JSDoc {
                return None;
            }
            current = record.parent;
        }
        None
    }

    /// A JSDoc tag's name (`@param`'s `param`).
    fn js_doc_tag_name(&self, tag: NodeId) -> Option<NodeId> {
        crate::jsdoc::tag_name(self.request.state, tag)
    }

    /// tsgo `tryGetTypeExpressionFromTag`.
    fn type_expression_from_tag(&self, tag: NodeId) -> Option<NodeId> {
        let type_expression = match self.data(tag) {
            NodeData::JSDocParameterTag(data) => data.type_expression,
            NodeData::JSDocPropertyTag(data) => data.type_expression,
            NodeData::JSDocReturnTag(data) => data.type_expression,
            NodeData::JSDocTypeTag(data) => data.type_expression,
            NodeData::JSDocTypedefTag(data) => data.type_expression,
            NodeData::JSDocThrowsTag(data) => data.type_expression,
            NodeData::JSDocSatisfiesTag(data) => data.type_expression,
            NodeData::JSDocTemplateTag(data) => data.constraint,
            NodeData::JSDocAugmentsTag(data) => return data.class,
            NodeData::JSDocImplementsTag(data) => return data.class,
            _ => None,
        };
        type_expression
            .filter(|&expression| self.node_kind(expression) == SyntaxKind::JSDocTypeExpression)
    }

    /// tsgo `ast.GetLeftmostAccessExpression`.
    fn leftmost_access_expression(&self, mut expression: NodeId) -> NodeId {
        loop {
            let next = match self.data(expression) {
                NodeData::PropertyAccessExpression(data) => data.expression,
                NodeData::ElementAccessExpression(data) => data.expression,
                NodeData::CallExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                NodeData::TaggedTemplateExpression(data) => data.tag,
                _ => None,
            };
            match next {
                Some(next) => expression = next,
                None => return expression,
            }
        }
    }

    /// tsgo `ast.IsTypeOnlyImportOrExportDeclaration`.
    fn is_type_only_import_or_export_declaration(&self, node: NodeId) -> bool {
        self.is_type_only_import_declaration(node)
            || match self.data(node) {
                NodeData::ExportSpecifier(data) => {
                    data.is_type_only
                        || self
                            .parent_of(node)
                            .and_then(|exports| self.parent_of(exports))
                            .is_some_and(|declaration| {
                                matches!(self.data(declaration), NodeData::ExportDeclaration(data) if data.is_type_only)
                            })
                }
                NodeData::ExportDeclaration(data) => {
                    data.is_type_only && data.module_specifier.is_some() && data.export_clause.is_none()
                }
                _ => false,
            }
    }

    /// tsgo `ast.IsTypeOnlyImportDeclaration`.
    fn is_type_only_import_declaration(&self, node: NodeId) -> bool {
        match self.data(node) {
            NodeData::ImportSpecifier(data) => {
                data.is_type_only
                    || self
                        .parent_of(node)
                        .and_then(|imports| self.parent_of(imports))
                        .is_some_and(|clause| {
                            matches!(self.data(clause), NodeData::ImportClause(data) if data.is_type_only)
                        })
            }
            NodeData::NamespaceImport(_) => self
                .parent_of(node)
                .is_some_and(|clause| {
                    matches!(self.data(clause), NodeData::ImportClause(data) if data.is_type_only)
                }),
            NodeData::ImportClause(data) => data.is_type_only,
            NodeData::ImportEqualsDeclaration(data) => data.is_type_only,
            _ => false,
        }
    }

    /// tsgo `ast.IsPartOfTypeNode` of a location.
    fn is_part_of_type_node(&self, location: Touching) -> bool {
        match location {
            Touching::Node(node) => self.request.state.is_part_of_type_node(node),
            Touching::Token { kind, .. } => is_type_node_kind_token(kind),
        }
    }

    /// tsgo `ast.IsValidTypeOnlyAliasUseSite` of the previous token; a
    /// token tsc-rs's tree does not keep is no expression, so a valid site.
    fn is_valid_type_only_alias_use_site(&self, token: Touching) -> bool {
        match token {
            Touching::Node(node) => self.request.state.is_valid_type_only_alias_use_site(node),
            Touching::Token { .. } => true,
        }
    }

    /// tsgo `ast.IsLiteralImportTypeNode`.
    fn is_literal_import_type_node(&self, node: NodeId) -> bool {
        let NodeData::ImportType(data) = self.data(node) else {
            return false;
        };
        data.argument.is_some_and(|argument| {
            matches!(self.data(argument), NodeData::LiteralType(literal)
                if literal.literal.is_some_and(|literal| self.node_kind(literal) == SyntaxKind::StringLiteral))
        })
    }

    /// tsgo `isInRightSideOfInternalImportEqualsDeclaration`.
    pub(super) fn is_in_right_side_of_internal_import_equals_declaration(
        &self,
        mut node: NodeId,
    ) -> bool {
        while let Some(parent) = self.parent_of(node) {
            if self.node_kind(parent) != SyntaxKind::QualifiedName {
                break;
            }
            node = parent;
        }
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        matches!(self.data(parent), NodeData::ImportEqualsDeclaration(data)
            if data.module_reference == Some(node))
    }

    pub(crate) fn declarations_of(&self, symbol: SymbolId) -> Vec<NodeId> {
        self.request
            .state
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .collect()
    }

    /// tsgo `ast.SymbolName`: a private name's text, else the name.
    pub(crate) fn symbol_name(&self, symbol: SymbolId) -> String {
        let data = self.request.state.binder.symbol(symbol);
        if let Some(value_declaration) = data.value_declaration {
            let source = self.request.state.binder.source_of_node(value_declaration);
            if let Some(name) = node_util::get_name_of_declaration(source, value_declaration) {
                if source.arena.node(name).kind == SyntaxKind::PrivateIdentifier {
                    return node_util::id_text(source, name)
                        .unwrap_or_default()
                        .to_owned();
                }
            }
        }
        tsc_binder::unescape_leading_underscores(data.escaped_name)
            .to_string_lossy()
            .into_owned()
    }

    /// tsgo `checker.IsExternalModuleSymbol`.
    pub(super) fn is_external_module_symbol(&self, symbol: SymbolId) -> bool {
        let data = self.request.state.binder.symbol(symbol);
        data.flags.intersects(SymbolFlags::VALUE_MODULE)
            && tsc_binder::unescape_leading_underscores(data.escaped_name).starts_with("\"")
    }

    /// tsgo `isStaticProperty`.
    fn is_static_property(&self, symbol: SymbolId) -> bool {
        let Some(declaration) = self.request.state.binder.symbol(symbol).value_declaration else {
            return false;
        };
        let source = self.request.state.binder.source_of_node(declaration);
        node_util::get_combined_modifier_flags(source, declaration)
            .intersects(tsc_types::ModifierFlags::STATIC)
            && source
                .arena
                .node(declaration)
                .parent
                .is_some_and(|parent| is_class_like(source.arena.node(parent).kind))
    }

    /// tsgo `getFirstSymbolInChain`.
    pub(super) fn first_symbol_in_chain(
        &mut self,
        symbol: SymbolId,
        enclosing: Option<Touching>,
    ) -> CheckResult<Option<SymbolId>> {
        let enclosing_node = enclosing.map(|enclosing| match enclosing {
            Touching::Node(node) => node,
            Touching::Token { parent, .. } => parent,
        });
        let chain = self.request.state.get_accessible_symbol_chain(
            symbol,
            enclosing_node,
            SymbolFlags::ALL,
            false,
        )?;
        if let Some(&first) = chain.as_ref().and_then(|chain| chain.first()) {
            return Ok(Some(first));
        }
        match self.request.state.binder.symbol(symbol).parent {
            Some(parent) => {
                if self.is_module_symbol(parent) {
                    return Ok(Some(symbol));
                }
                self.first_symbol_in_chain(parent, enclosing)
            }
            None => Ok(None),
        }
    }

    /// tsgo `isModuleSymbol`.
    fn is_module_symbol(&self, symbol: SymbolId) -> bool {
        self.declarations_of(symbol).iter().any(|&declaration| {
            self.request.state.binder.node_record(declaration).kind == SyntaxKind::SourceFile
        })
    }

    /// tsgo `getPropertiesForCompletion`: a union's every possible
    /// property, else the apparent properties.
    pub(crate) fn properties_for_completion(&mut self, ty: TypeId) -> CheckResult<Vec<SymbolId>> {
        if self
            .request
            .state
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::UNION)
        {
            let types = self.union_types(ty);
            return self
                .request
                .state
                .get_all_possible_properties_of_types(&types);
        }
        self.request.state.get_apparent_properties(ty)
    }

    /// A union's or intersection's types.
    pub(crate) fn union_types(&self, ty: TypeId) -> Vec<TypeId> {
        match &self.request.state.tables.type_of(ty).data {
            TypeData::Union { types, .. } => types.to_vec(),
            TypeData::Intersection { types } => types.to_vec(),
            _ => Vec::new(),
        }
    }

    /// tsgo `symbolCanBeReferencedAtTypeLocation`.
    pub(super) fn symbol_can_be_referenced_at_type_location(
        &mut self,
        symbol: SymbolId,
        seen_modules: &mut HashSet<SymbolId>,
    ) -> CheckResult<bool> {
        // An alias may merge with a local declaration: the alias and its
        // target both count.
        if self.non_alias_can_be_referenced_at_type_location(symbol, seen_modules)? {
            return Ok(true);
        }
        let export_symbol = self
            .request
            .state
            .binder
            .symbol(symbol)
            .export_symbol
            .unwrap_or(symbol);
        let target = self.request.state.skip_alias(export_symbol)?;
        self.non_alias_can_be_referenced_at_type_location(target, seen_modules)
    }

    /// tsgo `nonAliasCanBeReferencedAtTypeLocation`.
    fn non_alias_can_be_referenced_at_type_location(
        &mut self,
        symbol: SymbolId,
        seen_modules: &mut HashSet<SymbolId>,
    ) -> CheckResult<bool> {
        let flags = self.request.state.symbol_flags(symbol);
        if flags.intersects(SymbolFlags::TYPE) || self.request.state.is_unknown_symbol(symbol) {
            return Ok(true);
        }
        if flags.intersects(SymbolFlags::MODULE) && seen_modules.insert(symbol) {
            for export in self.request.state.get_exports_of_module_symbols(symbol)? {
                if self.symbol_can_be_referenced_at_type_location(export, seen_modules)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// tsgo `isProbablyGlobalType`: the type of `self`, `global` or
    /// `globalThis`.
    fn is_probably_global_type(&mut self, ty: TypeId) -> CheckResult<bool> {
        // `self` and `window` share lib.dom.d.ts's type; lib.webworker.d.ts
        // has no `window`, so `self` stands for both.
        for name in ["self", "global", "globalThis"] {
            if let Some(symbol) =
                self.request
                    .state
                    .get_global_symbol(name, SymbolFlags::VALUE, None)?
            {
                let root = self.source.root;
                if self
                    .request
                    .state
                    .get_type_of_symbol_at_location(symbol, Some(root))?
                    == ty
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// A literal type's value.
    fn literal_value(&self, ty: TypeId) -> Option<LiteralValue> {
        match &self.request.state.tables.type_of(ty).data {
            TypeData::Literal { value } => Some(value.clone()),
            _ => None,
        }
    }

    /// tsgo `getImportStatementCompletionInfo`.
    fn import_statement_completion_info(&self, context: Touching) -> ImportStatementCompletionInfo {
        let mut result = ImportStatementCompletionInfo::default();
        let mut candidate: Option<Touching> = None;
        let Some(parent) = self.parent(context) else {
            return result;
        };
        let context_kind = self.kind(context);
        match self.data(parent) {
            NodeData::ImportEqualsDeclaration(data) => {
                // import Foo |
                // import Foo f|
                let last_token = self.last_token(parent);
                if context_kind == SyntaxKind::Identifier && last_token != Some(context) {
                    result.keyword_completion = Some(SyntaxKind::FromKeyword);
                    result.is_keyword_only_completion = true;
                } else {
                    if context_kind != SyntaxKind::TypeKeyword {
                        result.keyword_completion = Some(SyntaxKind::TypeKeyword);
                    }
                    if self.is_module_specifier_missing_or_empty(data.module_reference) {
                        candidate = Some(Touching::Node(parent));
                    }
                }
            }
            _ if self.could_be_type_only_import_specifier(parent, context)
                && self
                    .parent_of(parent)
                    .is_some_and(|bindings| self.can_complete_from_named_bindings(bindings)) =>
            {
                candidate = Some(Touching::Node(parent));
            }
            NodeData::NamedImports(_) | NodeData::NamespaceImport(_) => {
                let clause = self.parent_of(parent);
                let type_only = clause.is_some_and(|clause| {
                    matches!(self.data(clause), NodeData::ImportClause(data) if data.is_type_only)
                });
                if !type_only
                    && matches!(
                        context_kind,
                        SyntaxKind::OpenBraceToken
                            | SyntaxKind::ImportKeyword
                            | SyntaxKind::CommaToken
                    )
                {
                    result.keyword_completion = Some(SyntaxKind::TypeKeyword);
                }
                if self.can_complete_from_named_bindings(parent) {
                    // At `import { ... } |` or `import * as Foo |`, only
                    // `from`.
                    if matches!(
                        context_kind,
                        SyntaxKind::CloseBraceToken | SyntaxKind::Identifier
                    ) {
                        result.is_keyword_only_completion = true;
                        result.keyword_completion = Some(SyntaxKind::FromKeyword);
                    } else {
                        candidate = clause
                            .and_then(|clause| self.parent_of(clause))
                            .map(Touching::Node);
                    }
                }
            }
            NodeData::ExportDeclaration(_) if context_kind == SyntaxKind::AsteriskToken => {
                result.is_keyword_only_completion = true;
                result.keyword_completion = Some(SyntaxKind::FromKeyword);
            }
            NodeData::NamedExports(_) if context_kind == SyntaxKind::CloseBraceToken => {
                result.is_keyword_only_completion = true;
                result.keyword_completion = Some(SyntaxKind::FromKeyword);
            }
            _ if context_kind == SyntaxKind::ImportKeyword => match self.node_kind(parent) {
                SyntaxKind::SourceFile => {
                    // A lone `import` parses as no statement at all.
                    result.keyword_completion = Some(SyntaxKind::TypeKeyword);
                    candidate = Some(context);
                }
                SyntaxKind::ImportDeclaration => {
                    // `import s| from`
                    result.keyword_completion = Some(SyntaxKind::TypeKeyword);
                    if self.is_module_specifier_missing_or_empty(self.module_specifier_of(parent)) {
                        candidate = Some(Touching::Node(parent));
                    }
                }
                _ => {}
            },
            _ => {}
        }
        if let Some(candidate) = candidate {
            result.is_new_identifier_location = true;
            result.replacement_span =
                self.single_line_replacement_span_for_import_completion_node(candidate);
            result.could_be_type_only_import_specifier = match candidate {
                Touching::Node(node) => self.could_be_type_only_import_specifier(node, context),
                Touching::Token { .. } => false,
            };
            if let Touching::Node(node) = candidate {
                match self.data(node) {
                    NodeData::ImportDeclaration(data) => {
                        if let Some(clause) = data.import_clause {
                            result.is_top_level_type_only = matches!(self.data(clause), NodeData::ImportClause(data) if data.is_type_only);
                        }
                    }
                    NodeData::ImportEqualsDeclaration(data) => {
                        result.is_top_level_type_only = data.is_type_only;
                    }
                    _ => {}
                }
            }
        } else {
            result.is_new_identifier_location =
                result.keyword_completion == Some(SyntaxKind::TypeKeyword);
        }
        result
    }

    /// tsgo `couldBeTypeOnlyImportSpecifier`.
    fn could_be_type_only_import_specifier(&self, specifier: NodeId, context: Touching) -> bool {
        let NodeData::ImportSpecifier(data) = self.data(specifier) else {
            return false;
        };
        data.is_type_only
            || data.name.map(Touching::Node) == Some(context)
                && self.is_type_keyword_token_or_identifier(context)
    }

    /// tsgo `isTypeKeywordTokenOrIdentifier`.
    pub(crate) fn is_type_keyword_token_or_identifier(&self, token: Touching) -> bool {
        self.kind(token) == SyntaxKind::TypeKeyword
            || self.kind(token) == SyntaxKind::Identifier
                && self
                    .identifier_text(token)
                    .and_then(identifier_to_keyword_kind)
                    == Some(SyntaxKind::TypeKeyword)
    }

    /// tsgo `canCompleteFromNamedBindings`.
    fn can_complete_from_named_bindings(&self, named_bindings: NodeId) -> bool {
        let Some(clause) = self.parent_of(named_bindings) else {
            return false;
        };
        let Some(declaration) = self.parent_of(clause) else {
            return false;
        };
        if !self.is_module_specifier_missing_or_empty(self.module_specifier_of(declaration))
            || self.name_of(clause).is_some()
        {
            return false;
        }
        if let NodeData::NamedImports(data) = self.data(named_bindings) {
            // Only with no other named imports, though parser recovery may
            // put later statements in the list: count the likely-valid
            // ones.
            let elements = self.node_list(data.elements);
            let valid_imports = match self.potentially_invalid_import_specifier(named_bindings) {
                Some(invalid) => elements.iter().position(|&element| element == invalid),
                None => Some(elements.len()),
            };
            return valid_imports.is_some_and(|count| count < 2);
        }
        true
    }

    /// tsgo `getPotentiallyInvalidImportSpecifier`: the first named import
    /// that is a later statement parser recovery put in the list
    /// (`import { Foo|` then `interface Bar {}`).
    fn potentially_invalid_import_specifier(&self, named_bindings: NodeId) -> Option<NodeId> {
        let NodeData::NamedImports(data) = self.data(named_bindings) else {
            return None;
        };
        self.node_list(data.elements)
            .iter()
            .copied()
            .find(|&element| {
                let NodeData::ImportSpecifier(specifier) = self.data(element) else {
                    return false;
                };
                let Some(name) = specifier.name else {
                    return false;
                };
                specifier.property_name.is_none()
                    && self
                        .identifier_text(Touching::Node(name))
                        .and_then(identifier_to_keyword_kind)
                        .is_some_and(is_non_contextual_keyword)
                    && self
                        .preceding_token(self.record(name).pos)
                        .is_some_and(|preceding| self.kind(preceding) != SyntaxKind::CommaToken)
            })
    }

    /// tsgo `isModuleSpecifierMissingOrEmpty`.
    fn is_module_specifier_missing_or_empty(&self, specifier: Option<NodeId>) -> bool {
        if self.node_is_missing(specifier) {
            return true;
        }
        let mut node = specifier.expect("a present module specifier");
        if let NodeData::ExternalModuleReference(data) = self.data(node) {
            match data.expression {
                Some(expression) => node = expression,
                None => return true,
            }
        }
        match self.data(node) {
            NodeData::StringLiteral(data) => data.text.is_empty(),
            NodeData::NoSubstitutionTemplateLiteral(data) => data.text.is_empty(),
            _ => true,
        }
    }

    /// tsgo `getSingleLineReplacementSpanForImportCompletionNode`.
    fn single_line_replacement_span_for_import_completion_node(
        &self,
        node: Touching,
    ) -> Option<Range> {
        let mut node = node;
        if let Some(ancestor) = self.find_ancestor(
            self.parent(node).or(match node {
                Touching::Node(node) => Some(node),
                Touching::Token { .. } => None,
            }),
            |this, node| {
                matches!(
                    this.node_kind(node),
                    SyntaxKind::ImportDeclaration
                        | SyntaxKind::ImportEqualsDeclaration
                        | SyntaxKind::JSDocImportTag
                )
            },
        ) {
            node = Touching::Node(ancestor);
        }
        // The token's start, not the JSDoc or trivia before it.
        let token_pos = self.start(node);
        if self.line_of(token_pos) == self.line_of(self.end(node)) {
            return Some(
                self.request
                    .lsp_range(self.source, token_pos as usize..self.end(node) as usize),
            );
        }
        let Touching::Node(node_id) = node else {
            panic!("ImportKeyword was necessarily on one line");
        };
        // Which point of the import may be a later statement parser
        // recovery made part of it: in the named imports, or the module
        // specifier.
        let potential_split_point = match self.data(node_id) {
            NodeData::ImportDeclaration(data) => data
                .import_clause
                .and_then(|clause| match self.data(clause) {
                    NodeData::ImportClause(clause) => clause.named_bindings,
                    _ => None,
                })
                .and_then(|bindings| self.potentially_invalid_import_specifier(bindings))
                .or(data.module_specifier),
            NodeData::JSDocImportTag(data) => data
                .import_clause
                .and_then(|clause| match self.data(clause) {
                    NodeData::ImportClause(clause) => clause.named_bindings,
                    _ => None,
                })
                .and_then(|bindings| self.potentially_invalid_import_specifier(bindings))
                .or(data.module_specifier),
            NodeData::ImportEqualsDeclaration(data) => data.module_reference,
            _ => None,
        }?;
        let first_token = self.first_token(node_id)?;
        let start = self.start(first_token);
        let end = self.record(potential_split_point).pos;
        // A multi-line import cannot be replaced, but with everything but
        // the "module specifier" on one line, that specifier is another
        // statement (`import Foo|` then `interface Blah {}`).
        if self.line_of(start) == self.line_of(end) {
            return Some(
                self.request
                    .lsp_range(self.source, start as usize..end as usize),
            );
        }
        None
    }
}

/// tsgo `getLeftMostName`: `a` of `a.b.c`.
fn left_most_name(source: &tsc_syntax::SourceFile, expression: NodeId) -> Option<NodeId> {
    match &source.arena.node(expression).data {
        NodeData::Identifier(_) => Some(expression),
        NodeData::PropertyAccessExpression(data) => left_most_name(source, data.expression?),
        _ => None,
    }
}

/// tsgo `stringutil.IsWhiteSpaceSingleLine`.
pub(super) fn is_white_space_single_line(c: char) -> bool {
    tsc_syntax::is_whitespace_like(c) && !tsc_syntax::is_line_break(c)
}

/// tsgo `ast.IsJSDocTag`.
fn is_js_doc_tag_kind(kind: SyntaxKind) -> bool {
    SyntaxKind::FirstJSDocTagNode.value() <= kind.value()
        && kind.value() <= SyntaxKind::LastJSDocTagNode.value()
}

/// A type node kind of a token tsc-rs's tree does not keep.
fn is_type_node_kind_token(kind: SyntaxKind) -> bool {
    super::context::is_type_node_kind(kind)
}
