//! tsgo `ls/hover.go`: a hover's quick info (the symbol's kind, name and
//! type, as the checker prints them) and documentation (the JSDoc of its
//! declaration, its signature, its roots or its alias target).

use tsc_binder::node_util;
use tsc_checker::exports::{
    VerbosityContext, TYPE_FORMAT_IN_TYPE_ALIAS, TYPE_FORMAT_MULTILINE_OBJECT_LITERALS,
    TYPE_FORMAT_USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE, TYPE_FORMAT_USE_INSTANTIATION_EXPRESSIONS,
    TYPE_FORMAT_WRITE_ARROW_STYLE_SIGNATURE, TYPE_FORMAT_WRITE_CALL_STYLE_SIGNATURE,
    TYPE_FORMAT_WRITE_TYPE_ARGUMENTS_OF_SIGNATURE,
};
use tsc_checker::state::{CheckResult, CheckerState, SignatureId, SignatureKind};
use tsc_checker::{DisplayPart, SymbolFormatFlags};
use tsc_diagnostics::spanmap::FEATURE_HOVER;
use tsc_lsproto::{
    Hover, HoverParams, MarkupContent,
    MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings, MarkupKind,
    VSClassifiedTextRun,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags, SymbolFlags, SymbolId, TypeData, TypeFlags, TypeId};

use crate::astnav::Navigator;
use crate::hovericon::{build_vs_hover_raw_content, stacked_raw_contents, vs_hover_image_id};
use crate::jsdoc::LinkLocations;
use crate::language_service::{LanguageService, LsError};
use crate::lsutil::{
    get_symbol_kind, get_symbol_modifiers, ScriptElementKind, ScriptElementKindModifier,
};
use crate::meaning::SemanticMeaning;
use crate::request::Request;
use crate::utilities::{containing_object_literal_element, range_of_node, touching, Touching};

/// tsgo `HoverResponse`.
pub type HoverResponse = Option<Hover>;

/// tsgo's `symbolFormatFlags` of a hover.
const SYMBOL_FORMAT_FLAGS: SymbolFormatFlags = SymbolFormatFlags(
    SymbolFormatFlags::WRITE_TYPE_PARAMETERS_OR_ARGUMENTS.0
        | SymbolFormatFlags::USE_ONLY_EXTERNAL_ALIASING.0
        | SymbolFormatFlags::ALLOW_ANY_NODE_KIND.0
        | SymbolFormatFlags::USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE.0,
);

/// tsgo's `typeFormatFlags` of a hover.
const TYPE_FORMAT_FLAGS: u32 =
    TYPE_FORMAT_USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE | TYPE_FORMAT_USE_INSTANTIATION_EXPRESSIONS;

impl LanguageService<'_> {
    /// tsgo `ProvideHover`.
    pub fn provide_hover(&mut self, params: &HoverParams) -> Result<HoverResponse, LsError> {
        let content_format = self
            .capabilities
            .text_document
            .hover
            .content_format
            .first()
            .cloned()
            .unwrap_or(MarkupKind::PLAIN_TEXT);
        let markdown = content_format == MarkupKind::MARKDOWN;
        let verbosity_level = params.verbosity_level.unwrap_or(0);
        let mut max_truncation_length = self.user_preferences().maximum_hover_length;
        if max_truncation_length <= 0 {
            max_truncation_length = 500;
        }
        let can_increase_verbosity_capability =
            self.capabilities.experimental.hover_verbosity_level;
        let vs_capability = self.capabilities._vs_supports_visual_studio_extensions;
        let file = self.file(&params.text_document.uri);
        let hovers = self.with_program_request(false, |request| {
            let mut hovers = Vec::new();
            // Each single-segment projection of the position (a content
            // mapper's output's original position may have several).
            for projection in request.position_projections(file, &params.position, FEATURE_HOVER) {
                if !projection.fidelity.is_single_segment() {
                    continue;
                }
                let hover = request.hover_at(
                    projection.file,
                    projection.position,
                    &content_format,
                    verbosity_level,
                    max_truncation_length,
                    can_increase_verbosity_capability,
                    vs_capability,
                )?;
                hovers.extend(hover);
            }
            Ok(hovers)
        })?;
        Ok(combine_hovers(hovers.unwrap_or_default(), markdown))
    }
}

/// tsgo `ProvideHover`'s hovers of several projections as one: their
/// different contents in order with a separator, the range they share, and
/// the stacked Visual Studio contents.
fn combine_hovers(hovers: Vec<Hover>, markdown: bool) -> Option<Hover> {
    let mut hovers = hovers.into_iter();
    let mut combined = hovers.next()?;
    let rest = hovers.collect::<Vec<_>>();
    if rest.is_empty() {
        return Some(combined);
    }
    let value_of = |hover: &Hover| match &hover.contents {
        MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings::MarkupContent(content) => {
            content.value.trim_end_matches('\n').to_owned()
        }
        _ => String::new(),
    };
    let mut contents = Vec::new();
    let mut raw_contents = Vec::new();
    let mut common_range = combined.range.clone();
    let mut can_increase_verbosity = combined.can_increase_verbosity;
    for hover in std::iter::once(&combined).chain(rest.iter()) {
        let content = value_of(hover);
        if !contents.contains(&content) {
            contents.push(content);
            if let Some(raw) = &hover._vs_raw_content {
                raw_contents.push(raw.clone());
            }
        }
        can_increase_verbosity |= hover.can_increase_verbosity;
        if common_range.is_none() || hover.range.is_none() || common_range != hover.range {
            common_range = None;
        }
    }
    let separator = if markdown { "\n\n---\n\n" } else { "\n\n" };
    if let MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings::MarkupContent(content) =
        &mut combined.contents
    {
        content.value = contents.join(separator);
    }
    combined.range = common_range;
    combined.can_increase_verbosity = can_increase_verbosity;
    combined._vs_raw_content = match raw_contents.len() {
        0 => None,
        1 => raw_contents.pop(),
        _ => Some(stacked_raw_contents(raw_contents)),
    };
    Some(combined)
}

impl Request<'_, '_> {
    /// tsgo `ProvideHover` at one projection of the position.
    #[allow(clippy::too_many_arguments)]
    fn hover_at(
        &mut self,
        file: usize,
        position: u32,
        content_format: &MarkupKind,
        verbosity_level: i32,
        max_truncation_length: i64,
        can_increase_verbosity_capability: bool,
        vs_capability: bool,
    ) -> CheckResult<Option<Hover>> {
        let markdown = *content_format == MarkupKind::MARKDOWN;
        let request = self;
        {
            let source = request.state.binder.source(file);
            let navigator = Navigator::new(source);
            let found = navigator.touching_property_name(position);
            let kind = navigator.kind(found);
            if kind == SyntaxKind::SourceFile {
                return Ok(None);
            }
            let Some(node) = touching(found) else {
                return Ok(None);
            };
            if matches!(
                kind,
                SyntaxKind::PropertyAccessExpression | SyntaxKind::QualifiedName
            ) && !crate::utilities::is_in_comment(&navigator, position, found)
            {
                // No quick info inside the comment of `a/**/.b`.
                return Ok(None);
            }
            let range_node = request.node_for_quick_info(node);
            let symbol = request.symbol_at_location_for_quick_info(range_node)?;
            let mut verbosity = VerbosityContext {
                level: verbosity_level,
                max_truncation_length: max_truncation_length as u32,
                ..VerbosityContext::default()
            };
            let QuickInfoAndDocumentation {
                quick_info,
                documentation,
                vs_documentation,
                runs,
            } = request.quick_info_and_documentation(
                symbol,
                range_node,
                markdown,
                &mut verbosity,
                vs_capability,
            )?;
            if quick_info.is_empty() {
                return Ok(None);
            }
            // tsgo `ToLSPRangeForFeature`: a range only where it maps to one
            // segment that takes part in hovers.
            let (range, range_fidelity) = match range_node {
                Touching::Node(id) => {
                    let source = request.state.binder.source_of_node(id);
                    request.lsp_range_for_feature(
                        source,
                        range_of_node(request.state, id),
                        FEATURE_HOVER,
                    )
                }
                Touching::Token { .. } => request.lsp_range_for_feature(
                    source,
                    navigator.start_of_node(found, false) as usize..navigator.end(found) as usize,
                    FEATURE_HOVER,
                ),
            };
            let value = if markdown {
                format!("{}{documentation}", format_quick_info(&quick_info))
            } else {
                format!("{quick_info}{documentation}")
            };
            // A client with Visual Studio extensions shows `_vs_rawContent`
            // instead: the symbol's icon beside the colorized quick info.
            let vs_raw_content = if vs_capability && !runs.is_empty() {
                let mut kind = ScriptElementKind::Keyword;
                let mut modifiers = ScriptElementKindModifier::NONE;
                if let Some(symbol) = symbol {
                    // An alias shows its target's icon.
                    let mut icon_symbol = symbol;
                    if request
                        .state
                        .symbol_flags(symbol)
                        .intersects(SymbolFlags::ALIAS)
                    {
                        let resolved = request.state.get_aliased_symbol(symbol)?;
                        if resolved != symbol {
                            icon_symbol = resolved;
                        }
                    }
                    let location = match range_node {
                        Touching::Node(id) => id,
                        Touching::Token { parent, .. } => parent,
                    };
                    kind = get_symbol_kind(request.state, icon_symbol, location, true)?;
                    modifiers = get_symbol_modifiers(request.state, symbol, true)?;
                }
                let documentation_runs = match vs_documentation.trim_start_matches('\n') {
                    "" => Vec::new(),
                    text => vec![VSClassifiedTextRun {
                        classification_type_name: "text".to_owned(),
                        text: text.to_owned(),
                        marker_tag_type: None,
                        style: 0,
                        _vs_type: Default::default(),
                    }],
                };
                build_vs_hover_raw_content(
                    vs_hover_image_id(kind, modifiers),
                    runs,
                    documentation_runs,
                )
            } else {
                None
            };
            Ok(Some(Hover {
                contents:
                    MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings::MarkupContent(
                        MarkupContent {
                            kind: content_format.clone(),
                            value,
                        },
                    ),
                range: range_fidelity.is_single_segment().then_some(range),
                can_increase_verbosity: can_increase_verbosity_capability
                    && verbosity.can_increase_verbosity
                    && !verbosity.truncated,
                _vs_raw_content: vs_raw_content,
            }))
        }
    }
}

/// tsgo `getQuickInfoAndDocumentationForSymbol`'s results.
#[derive(Default)]
pub(crate) struct QuickInfoAndDocumentation {
    pub(crate) quick_info: String,
    pub(crate) documentation: String,
    vs_documentation: String,
    runs: Vec<VSClassifiedTextRun>,
}

/// tsgo `classificationForSymbol`: a symbol's name's Visual Studio
/// classification, as Strada's `displayPartKind` and `GetClassificationName`
/// chain it.
pub(crate) fn classification_for_symbol(
    state: &CheckerState<'_>,
    symbol: SymbolId,
) -> &'static str {
    let data = state.binder.symbol(symbol);
    let flags = data.flags;
    if flags.intersects(SymbolFlags::VARIABLE) {
        let parameter = data.declarations.first().is_some_and(|&declaration| {
            state.binder.node_record(declaration).kind == SyntaxKind::Parameter
        });
        return if parameter {
            "parameter name"
        } else {
            "local name"
        };
    }
    if flags
        .intersects(SymbolFlags::PROPERTY | SymbolFlags::GET_ACCESSOR | SymbolFlags::SET_ACCESSOR)
    {
        "property name"
    } else if flags.intersects(SymbolFlags::ENUM_MEMBER) {
        "field name"
    } else if flags.intersects(SymbolFlags::FUNCTION) {
        "method name"
    } else if flags.intersects(SymbolFlags::CLASS) {
        "class name"
    } else if flags.intersects(SymbolFlags::INTERFACE) {
        "interface name"
    } else if flags.intersects(SymbolFlags::ENUM) {
        "enum name"
    } else if flags.intersects(SymbolFlags::MODULE) {
        "module name"
    } else if flags.intersects(SymbolFlags::METHOD) {
        "method name"
    } else if flags.intersects(SymbolFlags::TYPE_PARAMETER) {
        "type parameter name"
    } else if flags.intersects(SymbolFlags::TYPE_ALIAS | SymbolFlags::ALIAS) {
        "identifier"
    } else {
        "text"
    }
}

/// tsgo `formatQuickInfo`: the quick info as a TypeScript code block.
fn format_quick_info(quick_info: &str) -> String {
    let mut text = String::new();
    write_code(&mut text, "typescript", quick_info);
    text
}

/// tsgo `writeCode`: a fenced code block, with more backticks than the code
/// has in a row.
pub(crate) fn write_code(text: &mut String, language: &str, code: &str) {
    if code.is_empty() {
        return;
    }
    let mut ticks = 3;
    while code.contains(&"`".repeat(ticks)) {
        ticks += 1;
    }
    text.push_str(&"`".repeat(ticks));
    text.push_str(language);
    text.push('\n');
    text.push_str(code);
    text.push('\n');
    text.push_str(&"`".repeat(ticks));
    text.push('\n');
}

/// The quick info being written: tsgo's `displayPartsWriter`, whose
/// classified runs a client with Visual Studio extensions gets.
pub(crate) struct QuickInfo {
    pub(crate) text: String,
    /// The runs, for a client with Visual Studio extensions.
    pub(crate) runs: Option<Vec<VSClassifiedTextRun>>,
    alias_level: u32,
    declaration: Option<NodeId>,
}

impl QuickInfo {
    fn new(vs_capability: bool) -> Self {
        Self {
            text: String::new(),
            runs: vs_capability.then(Vec::new),
            alias_level: 0,
            declaration: None,
        }
    }

    /// tsgo `addRun`.
    fn add_run(&mut self, classification: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(runs) = &mut self.runs {
            runs.push(VSClassifiedTextRun {
                classification_type_name: classification.to_owned(),
                text: text.to_owned(),
                marker_tag_type: None,
                style: 0,
                _vs_type: Default::default(),
            });
        }
        self.text.push_str(text);
    }

    fn write(&mut self, text: &str) {
        self.add_run("text", text);
    }

    fn write_keyword(&mut self, text: &str) {
        self.add_run("keyword", text);
    }

    fn write_punctuation(&mut self, text: &str) {
        self.add_run("punctuation", text);
    }

    fn write_operator(&mut self, text: &str) {
        self.add_run("operator", text);
    }

    fn write_literal(&mut self, text: &str) {
        self.add_run("string", text);
    }

    /// tsgo `WriteSymbol`: a name with its symbol's classification.
    fn write_symbol(&mut self, text: &str, classification: &str) {
        self.add_run(classification, text);
    }

    /// tsgo `WriteFrom`: another writer's runs.
    fn write_parts(&mut self, parts: Vec<DisplayPart>) {
        for part in parts {
            self.add_run(part.classification, &part.text);
        }
    }

    /// tsgo's `(` text `) ` prefix.
    fn write_parenthesized(&mut self, text: &str) {
        self.write_punctuation("(");
        self.write(text);
        self.write_punctuation(") ");
    }

    /// tsgo `writeNewLine`: a line between meanings, and an alias's prefix.
    fn write_new_line(&mut self) {
        if !self.text.is_empty() {
            self.write("\n");
        }
        if self.alias_level != 0 {
            self.write_parenthesized("alias");
        }
    }

    /// tsgo `setDeclaration`: the first declaration the quick info names.
    fn set_declaration(&mut self, declaration: Option<NodeId>) {
        if self.declaration.is_none() {
            self.declaration = declaration;
        }
    }
}

impl<'a> Request<'_, 'a> {
    /// tsgo `getQuickInfoAndDocumentationForSymbol`: the quick info, its
    /// documentation and, for a client with Visual Studio extensions, the
    /// plain JSDoc summary (Visual Studio renders no Markdown or tags) and
    /// the quick info's classified runs.
    pub(crate) fn quick_info_and_documentation(
        &mut self,
        symbol: Option<SymbolId>,
        node: Touching,
        markdown: bool,
        verbosity: &mut VerbosityContext,
        vs_capability: bool,
    ) -> CheckResult<QuickInfoAndDocumentation> {
        let meaning = self.meaning_from_location(node);
        let info = self.quick_info_and_declaration_at_location(
            symbol,
            node,
            verbosity,
            meaning,
            vs_capability,
        )?;
        if info.text.is_empty() {
            return Ok(QuickInfoAndDocumentation::default());
        }
        let documentation = self.documentation_for_symbol(
            symbol,
            node,
            info.declaration,
            markdown,
            false,
            LinkLocations::Feature(FEATURE_HOVER),
        )?;
        let vs_documentation = if vs_capability {
            self.documentation_for_symbol(
                symbol,
                node,
                info.declaration,
                false,
                true,
                LinkLocations::Feature(FEATURE_HOVER),
            )?
        } else {
            String::new()
        };
        Ok(QuickInfoAndDocumentation {
            quick_info: info.text,
            documentation,
            vs_documentation,
            runs: info.runs.unwrap_or_default(),
        })
    }

    /// tsgo `getQuickInfoAndDeclarationAtLocation`.
    pub(crate) fn quick_info_and_declaration_at_location(
        &mut self,
        symbol: Option<SymbolId>,
        node: Touching,
        verbosity: &mut VerbosityContext,
        meaning: SemanticMeaning,
        vs_capability: bool,
    ) -> CheckResult<QuickInfo> {
        let container = self.container_node(node);
        let mut info = QuickInfo::new(vs_capability);
        let kind = self.kind(node);
        if kind == SyntaxKind::ThisKeyword && self.is_in_expression_context(node)
            || self.is_this_in_type_query(node)
        {
            info.write_keyword("this");
            info.write_punctuation(": ");
            let ty = self.type_at(node)?;
            self.write_type(&mut info, ty, container, TYPE_FORMAT_FLAGS, verbosity);
            return Ok(info);
        }
        let Some(symbol) = symbol else {
            if self.should_get_type(node) {
                let ty = self.type_at(node)?;
                self.write_type(&mut info, ty, container, TYPE_FORMAT_FLAGS, verbosity);
            }
            return Ok(info);
        };
        let mut visited_aliases = Vec::new();
        let mut symbol_was_expanded = false;
        self.write_symbol_quick_info(
            &mut info,
            symbol,
            node,
            container,
            meaning,
            verbosity,
            &mut visited_aliases,
            &mut symbol_was_expanded,
        )?;
        Ok(info)
    }

    /// tsgo's `writeSymbol` of `getQuickInfoAndDeclarationAtLocation`: each
    /// meaning of the symbol (an alias's target's first).
    #[allow(clippy::too_many_arguments)]
    fn write_symbol_quick_info(
        &mut self,
        info: &mut QuickInfo,
        symbol: SymbolId,
        node: Touching,
        container: Option<NodeId>,
        meaning: SemanticMeaning,
        verbosity: &mut VerbosityContext,
        visited_aliases: &mut Vec<SymbolId>,
        symbol_was_expanded: &mut bool,
    ) -> CheckResult<()> {
        let symbol_flags = self.state.binder.symbol(symbol).flags;
        if symbol_flags.intersects(SymbolFlags::ALIAS) && !visited_aliases.contains(&symbol) {
            visited_aliases.push(symbol);
            let aliased = self.state.get_aliased_symbol(symbol)?;
            if aliased != self.state.unknown_symbol {
                info.alias_level += 1;
                self.write_symbol_quick_info(
                    info,
                    aliased,
                    node,
                    container,
                    meaning,
                    verbosity,
                    visited_aliases,
                    symbol_was_expanded,
                )?;
                info.alias_level -= 1;
            }
        }
        let all = SymbolFlags::VALUE
            | SymbolFlags::SIGNATURE
            | SymbolFlags::TYPE
            | SymbolFlags::NAMESPACE;
        let mut flags = if meaning == SemanticMeaning::VALUE {
            symbol_flags & (SymbolFlags::VALUE | SymbolFlags::SIGNATURE)
        } else if meaning == SemanticMeaning::TYPE {
            symbol_flags & SymbolFlags::TYPE
        } else if meaning == SemanticMeaning::NAMESPACE {
            symbol_flags & SymbolFlags::NAMESPACE
        } else {
            symbol_flags & all
        };
        if flags.is_empty() {
            if info.alias_level != 0 || !info.text.is_empty() {
                return Ok(());
            }
            flags = symbol_flags & all;
            if flags.is_empty() {
                return Ok(());
            }
        }
        let value_declaration = self.state.binder.symbol(symbol).value_declaration;
        if flags.intersects(SymbolFlags::PROPERTY)
            && value_declaration.is_some_and(|declaration| {
                self.node_kind(declaration) == SyntaxKind::MethodDeclaration
            })
        {
            flags = SymbolFlags::METHOD;
        }
        if flags.intersects(SymbolFlags::VARIABLE | SymbolFlags::PROPERTY | SymbolFlags::ACCESSOR) {
            info.write_new_line();
            if !self.state.is_index_symbol(symbol) {
                if flags.intersects(SymbolFlags::PROPERTY) {
                    info.write_parenthesized("property");
                } else if flags.intersects(SymbolFlags::ACCESSOR) {
                    info.write_parenthesized("accessor");
                } else if let Some(declaration) = value_declaration {
                    let source = self.state.binder.source_of_node(declaration);
                    let root = node_util::get_root_declaration(source, declaration);
                    if self.node_kind(root) == SyntaxKind::Parameter {
                        info.write_parenthesized("parameter");
                    } else {
                        let block_scoped = node_util::get_combined_node_flags(source, root)
                            & NodeFlags::BLOCK_SCOPED;
                        if block_scoped == NodeFlags::LET {
                            info.write_keyword("let ");
                        } else if block_scoped == NodeFlags::CONST {
                            info.write_keyword("const ");
                        } else if block_scoped == NodeFlags::USING {
                            info.write_keyword("using ");
                        } else if block_scoped == NodeFlags::AWAIT_USING {
                            info.write_keyword("await ");
                            info.write_keyword("using ");
                        } else {
                            info.write_keyword("var ");
                        }
                    }
                }
                let parent = self.state.binder.symbol(symbol).parent;
                if self.state.binder.symbol(symbol).escaped_name.as_str() == Some("export=")
                    && parent.is_some_and(|parent| {
                        self.state
                            .binder
                            .symbol(parent)
                            .flags
                            .intersects(SymbolFlags::MODULE)
                    })
                {
                    info.write("exports");
                } else {
                    self.write_symbol_name(
                        info,
                        symbol,
                        container,
                        SymbolFlags::NONE,
                        SYMBOL_FORMAT_FLAGS,
                    )?;
                }
                if symbol_flags.intersects(SymbolFlags::OPTIONAL) {
                    info.write_punctuation("?");
                }
                info.write_punctuation(": ");
            }
            if let Some(call) = self.call_or_new_expression(node) {
                let mut flags = TYPE_FORMAT_FLAGS
                    | TYPE_FORMAT_WRITE_TYPE_ARGUMENTS_OF_SIGNATURE
                    | TYPE_FORMAT_WRITE_ARROW_STYLE_SIGNATURE;
                if self.node_kind(call) == SyntaxKind::CallExpression {
                    flags |= TYPE_FORMAT_WRITE_CALL_STYLE_SIGNATURE;
                }
                let signature =
                    self.state
                        .get_resolved_signature(call, None, tsc_types::CheckMode::NORMAL)?;
                self.write_signature(info, signature, container, flags, verbosity);
            } else {
                let location = match node {
                    Touching::Node(id) => Some(id),
                    Touching::Token { .. } => None,
                };
                let ty = self
                    .state
                    .get_type_of_symbol_at_location(symbol, location)?;
                let constrained_type_parameter =
                    self.state.tables.type_of(ty).symbol.is_some_and(|symbol| {
                        self.state
                            .binder
                            .symbol(symbol)
                            .flags
                            .intersects(SymbolFlags::TYPE_PARAMETER)
                    }) && self.state.get_constraint_of_type_parameter(ty)?.is_some();
                if constrained_type_parameter {
                    // A constrained type parameter shows its constraint at a
                    // higher verbosity level.
                    if verbosity.level > 0 {
                        let mut expand = VerbosityContext {
                            level: verbosity.level - 1,
                            max_truncation_length: verbosity.max_truncation_length,
                            ..VerbosityContext::default()
                        };
                        let text = self
                            .state
                            .type_parameter_to_string_with_flags(ty, container, Some(&mut expand))
                            .unwrap_or_default();
                        info.write(&text);
                        verbosity.can_increase_verbosity |= expand.can_increase_verbosity;
                        verbosity.truncated |= expand.truncated;
                    } else {
                        self.write_type(info, ty, container, TYPE_FORMAT_FLAGS, verbosity);
                        verbosity.can_increase_verbosity = true;
                    }
                } else {
                    self.write_type(info, ty, container, TYPE_FORMAT_FLAGS, verbosity);
                }
            }
            let first = self.declarations_of(symbol).first().copied();
            info.set_declaration(value_declaration.or(first));
        }
        if flags.intersects(SymbolFlags::ENUM_MEMBER) {
            info.write_new_line();
            info.write_parenthesized("enum member");
            let ty = self.state.get_type_of_symbol(symbol)?;
            self.write_type(info, ty, container, TYPE_FORMAT_FLAGS, verbosity);
            if self
                .state
                .tables
                .flags_of(ty)
                .intersects(TypeFlags::LITERAL)
            {
                info.write_operator(" = ");
                info.write_literal(&self.literal_text(ty));
            }
            info.set_declaration(value_declaration);
        }
        if flags.intersects(SymbolFlags::FUNCTION | SymbolFlags::METHOD) {
            let is_method = flags.intersects(SymbolFlags::METHOD);
            let prefix = if is_method { "method" } else { "function " };
            let named_declaration = match node {
                Touching::Node(id) if self.node_kind(id) == SyntaxKind::Identifier => {
                    self.parent(node).filter(|&parent| {
                        let kind = self.node_kind(parent);
                        (node_util::is_function_like_declaration_kind(kind)
                            || kind == SyntaxKind::MethodSignature)
                            && self.name_of(parent) == Some(id)
                            && self.declarations_of(symbol).contains(&parent)
                    })
                }
                _ => None,
            };
            if let Some(declaration) = named_declaration {
                info.set_declaration(Some(declaration));
                let signature = self
                    .state
                    .get_signature_from_declaration_checked(declaration)?;
                self.write_signatures(
                    info,
                    &[signature],
                    prefix,
                    is_method,
                    symbol,
                    container,
                    verbosity,
                )?;
            } else {
                let signatures = self.signatures_at_location(symbol, SignatureKind::Call, node)?;
                if let [signature] = signatures[..] {
                    if let Some(declaration) = self.state.signature_of(signature).declaration {
                        if !self.has_js_doc_flag(declaration) {
                            info.set_declaration(Some(declaration));
                        }
                    }
                }
                self.write_signatures(
                    info,
                    &signatures,
                    prefix,
                    is_method,
                    symbol,
                    container,
                    verbosity,
                )?;
            }
            info.set_declaration(value_declaration);
        }
        if flags.intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE) {
            let parent = self.parent(node);
            if kind_is(self.kind(node), SyntaxKind::ThisKeyword) || self.is_this_in_type_query(node)
            {
                info.write_new_line();
                info.write_keyword("this");
            } else if self.kind(node) == SyntaxKind::ConstructorKeyword
                && parent.is_some_and(|parent| {
                    matches!(
                        self.node_kind(parent),
                        SyntaxKind::Constructor | SyntaxKind::ConstructSignature
                    )
                })
            {
                let parent = parent.expect("a constructor");
                info.set_declaration(Some(parent));
                let signature = self.state.get_signature_from_declaration_checked(parent)?;
                self.write_signatures(
                    info,
                    &[signature],
                    "constructor ",
                    false,
                    symbol,
                    container,
                    verbosity,
                )?;
            } else {
                let mut signatures = Vec::new();
                if flags.intersects(SymbolFlags::CLASS)
                    && self.call_or_new_expression(node).is_some()
                {
                    signatures =
                        self.signatures_at_location(symbol, SignatureKind::Construct, node)?;
                }
                if let [signature] = signatures[..] {
                    if let Some(declaration) = self.state.signature_of(signature).declaration {
                        if !self.has_js_doc_flag(declaration) {
                            info.set_declaration(Some(declaration));
                        }
                    }
                    self.write_signatures(
                        info,
                        &signatures,
                        "constructor ",
                        false,
                        symbol,
                        container,
                        verbosity,
                    )?;
                } else {
                    info.write_new_line();
                    if flags.intersects(SymbolFlags::CLASS) {
                        let class_expression =
                            self.declaration_of_kind(symbol, SyntaxKind::ClassExpression);
                        if class_expression.is_some() {
                            info.write_parenthesized("local class");
                        }
                        if !self.try_expand_symbol(
                            info,
                            symbol,
                            flags,
                            node,
                            verbosity,
                            symbol_was_expanded,
                        )? {
                            if class_expression.is_none() {
                                let is_abstract =
                                    self.declarations_of(symbol).into_iter().any(|declaration| {
                                        self.node_kind(declaration) == SyntaxKind::ClassDeclaration
                                            && node_util::has_syntactic_modifier(
                                                self.state.binder.source_of_node(declaration),
                                                declaration,
                                                ModifierFlags::ABSTRACT,
                                            )
                                    });
                                if is_abstract {
                                    info.write_keyword("abstract ");
                                }
                                info.write_keyword("class ");
                            }
                            self.write_symbol_name(
                                info,
                                symbol,
                                container,
                                SymbolFlags::NONE,
                                SYMBOL_FORMAT_FLAGS,
                            )?;
                            let parameters = self.local_type_parameters(symbol)?;
                            self.write_type_params(info, &parameters, verbosity)?;
                        }
                    } else if !self.try_expand_symbol(
                        info,
                        symbol,
                        flags,
                        node,
                        verbosity,
                        symbol_was_expanded,
                    )? {
                        info.write_keyword("interface ");
                        self.write_symbol_name(
                            info,
                            symbol,
                            container,
                            SymbolFlags::NONE,
                            SYMBOL_FORMAT_FLAGS,
                        )?;
                        let parameters = self.local_type_parameters(symbol)?;
                        self.write_type_params(info, &parameters, verbosity)?;
                    }
                }
            }
            if flags.intersects(SymbolFlags::CLASS) {
                info.set_declaration(value_declaration);
            } else {
                let interface = self
                    .declarations_of(symbol)
                    .into_iter()
                    .find(|&declaration| {
                        self.node_kind(declaration) == SyntaxKind::InterfaceDeclaration
                    });
                info.set_declaration(interface);
            }
        }
        if flags.intersects(SymbolFlags::ENUM) {
            info.write_new_line();
            if !self.try_expand_symbol(info, symbol, flags, node, verbosity, symbol_was_expanded)? {
                let is_const = self.declarations_of(symbol).into_iter().any(|declaration| {
                    self.node_kind(declaration) == SyntaxKind::EnumDeclaration
                        && node_util::get_combined_modifier_flags(
                            self.state.binder.source_of_node(declaration),
                            declaration,
                        )
                        .intersects(ModifierFlags::CONST)
                });
                if is_const {
                    info.write_keyword("const ");
                }
                info.write_keyword("enum ");
                self.write_symbol_name(
                    info,
                    symbol,
                    container,
                    SymbolFlags::NONE,
                    SYMBOL_FORMAT_FLAGS,
                )?;
            }
            let declaration = self
                .declarations_of(symbol)
                .into_iter()
                .find(|&declaration| self.node_kind(declaration) == SyntaxKind::EnumDeclaration);
            info.set_declaration(declaration);
        }
        if flags.intersects(SymbolFlags::MODULE) {
            info.write_new_line();
            if !self.try_expand_symbol(info, symbol, flags, node, verbosity, symbol_was_expanded)? {
                let is_module = value_declaration.is_some_and(|declaration| {
                    self.node_kind(declaration) == SyntaxKind::SourceFile
                        || node_util::is_ambient_module(
                            self.state.binder.source_of_node(declaration),
                            declaration,
                        )
                });
                info.write_keyword(if is_module { "module " } else { "namespace " });
                self.write_symbol_name(
                    info,
                    symbol,
                    container,
                    SymbolFlags::NONE,
                    SYMBOL_FORMAT_FLAGS,
                )?;
                self.write_module_import_attributes(info, symbol);
            }
            let declaration = self
                .declarations_of(symbol)
                .into_iter()
                .find(|&declaration| self.node_kind(declaration) == SyntaxKind::ModuleDeclaration);
            info.set_declaration(declaration);
        }
        if flags.intersects(SymbolFlags::TYPE_PARAMETER) {
            info.write_new_line();
            info.write_parenthesized("type parameter");
            if let Touching::Node(id) = node {
                let parent = self.parent(node);
                if self.node_kind(id) == SyntaxKind::Identifier
                    && parent
                        .is_some_and(|parent| self.node_kind(parent) == SyntaxKind::TypeReference)
                {
                    let ty = self
                        .state
                        .get_type_at_location(parent.expect("a type reference"))?;
                    if matches!(
                        self.state.tables.type_of(ty).data,
                        TypeData::TypeParameter {
                            is_distributed: true,
                            ..
                        }
                    ) {
                        info.write_parenthesized("distributed");
                    }
                }
            }
            let ty = self.state.get_declared_type_of_symbol(symbol)?;
            self.write_symbol_name(
                info,
                symbol,
                container,
                SymbolFlags::NONE,
                SYMBOL_FORMAT_FLAGS,
            )?;
            if let Some(constraint) = self.state.get_constraint_of_type_parameter(ty)? {
                info.write_keyword(" extends ");
                self.write_type(info, constraint, container, TYPE_FORMAT_FLAGS, verbosity);
            }
            if let Some(parent) = self.state.binder.symbol(symbol).parent {
                // A class or interface's type parameter.
                info.write_keyword(" in ");
                self.write_symbol_name(
                    info,
                    parent,
                    container,
                    SymbolFlags::NONE,
                    SYMBOL_FORMAT_FLAGS,
                )?;
                let parent_type = self.state.get_declared_type_of_symbol(parent)?;
                if self.is_interface_type(parent_type) {
                    let parameters = self.local_type_parameters_of_type(parent_type);
                    self.write_type_params(info, &parameters, verbosity)?;
                }
            } else if let Some(declaration) =
                self.declaration_of_kind(symbol, SyntaxKind::TypeParameter)
            {
                // A function's or a type alias's type parameter.
                if let Some(owner) = self.state.binder.node_record(declaration).parent {
                    let owner_kind = self.node_kind(owner);
                    if crate::utilities::is_function_like_kind(owner_kind) {
                        info.write_keyword(" in ");
                        if owner_kind == SyntaxKind::ConstructSignature {
                            info.write_keyword("new ");
                        } else if owner_kind != SyntaxKind::CallSignature
                            && self.name_of(owner).is_some()
                        {
                            if let Some(owner_symbol) = self.state.binder.node_symbol(owner) {
                                self.write_symbol_name(
                                    info,
                                    owner_symbol,
                                    container,
                                    SymbolFlags::NONE,
                                    SYMBOL_FORMAT_FLAGS,
                                )?;
                            }
                        }
                        let signature = self.state.get_signature_from_declaration_checked(owner)?;
                        self.write_signature(
                            info,
                            signature,
                            container,
                            TYPE_FORMAT_FLAGS | TYPE_FORMAT_WRITE_TYPE_ARGUMENTS_OF_SIGNATURE,
                            verbosity,
                        );
                    } else if owner_kind == SyntaxKind::TypeAliasDeclaration {
                        info.write_keyword(" in ");
                        info.write_keyword("type ");
                        if let Some(owner_symbol) = self.state.binder.node_symbol(owner) {
                            self.write_symbol_name(
                                info,
                                owner_symbol,
                                container,
                                SymbolFlags::NONE,
                                SYMBOL_FORMAT_FLAGS,
                            )?;
                            let parameters = self.type_alias_type_parameters(owner_symbol)?;
                            self.write_type_params(info, &parameters, verbosity)?;
                        }
                    }
                }
            }
            info.set_declaration(self.declaration_of_kind(symbol, SyntaxKind::TypeParameter));
        }
        if flags.intersects(SymbolFlags::TYPE_ALIAS) {
            info.write_new_line();
            info.write_keyword("type ");
            self.write_symbol_name(
                info,
                symbol,
                container,
                SymbolFlags::NONE,
                SYMBOL_FORMAT_FLAGS,
            )?;
            let parameters = self.type_alias_type_parameters(symbol)?;
            self.write_type_params(info, &parameters, verbosity)?;
            info.write_operator(" = ");
            let parent = self.parent(node);
            let ty = match parent.filter(|&parent| self.is_const_type_reference(parent)) {
                Some(parent) => self.state.get_type_at_location(parent)?,
                None => self.state.get_declared_type_of_symbol(symbol)?,
            };
            self.write_type(
                info,
                ty,
                container,
                TYPE_FORMAT_FLAGS | TYPE_FORMAT_IN_TYPE_ALIAS,
                verbosity,
            );
            let declaration = self
                .declarations_of(symbol)
                .into_iter()
                .find(|&declaration| {
                    matches!(
                        self.node_kind(declaration),
                        SyntaxKind::TypeAliasDeclaration
                            | SyntaxKind::JSDocTypedefTag
                            | SyntaxKind::JSDocCallbackTag
                    )
                });
            info.set_declaration(declaration);
        }
        if flags.intersects(SymbolFlags::SIGNATURE) {
            info.write_new_line();
            let ty = self.state.get_type_of_symbol(symbol)?;
            self.write_type(info, ty, container, TYPE_FORMAT_FLAGS, verbosity);
        }
        Ok(())
    }

    /// tsgo's `writeSignatures`: each signature after its prefix and the
    /// symbol's name (three of five or more, and a count).
    #[allow(clippy::too_many_arguments)]
    fn write_signatures(
        &mut self,
        info: &mut QuickInfo,
        signatures: &[SignatureId],
        prefix: &str,
        parenthesized: bool,
        symbol: SymbolId,
        container: Option<NodeId>,
        verbosity: &mut VerbosityContext,
    ) -> CheckResult<()> {
        for (index, &signature) in signatures.iter().enumerate() {
            info.write_new_line();
            if index == 3 && signatures.len() >= 5 {
                // tsgo `WriteComment`, a text run.
                info.write(&format!("// +{} more overloads", signatures.len() - 3));
                break;
            }
            if parenthesized {
                info.write_parenthesized(prefix);
            } else {
                info.write_keyword(prefix);
            }
            self.write_symbol_name(
                info,
                symbol,
                container,
                SymbolFlags::NONE,
                SYMBOL_FORMAT_FLAGS,
            )?;
            if self
                .state
                .binder
                .symbol(symbol)
                .flags
                .intersects(SymbolFlags::OPTIONAL)
            {
                info.write_punctuation("?");
            }
            self.write_signature(
                info,
                signature,
                container,
                TYPE_FORMAT_FLAGS
                    | TYPE_FORMAT_WRITE_CALL_STYLE_SIGNATURE
                    | TYPE_FORMAT_WRITE_TYPE_ARGUMENTS_OF_SIGNATURE,
                verbosity,
            );
        }
        Ok(())
    }

    /// tsgo's `writeTypeParams`.
    fn write_type_params(
        &mut self,
        info: &mut QuickInfo,
        parameters: &[TypeId],
        verbosity: &mut VerbosityContext,
    ) -> CheckResult<()> {
        if parameters.is_empty() {
            return Ok(());
        }
        info.write_punctuation("<");
        for (index, &parameter) in parameters.iter().enumerate() {
            if index != 0 {
                info.write_punctuation(", ");
            }
            if let Some(symbol) = self.state.tables.type_of(parameter).symbol {
                self.write_symbol_name(info, symbol, None, SymbolFlags::NONE, SYMBOL_FORMAT_FLAGS)?;
            }
            if let Some(constraint) = self.state.get_constraint_of_type_parameter(parameter)? {
                info.write_keyword(" extends ");
                self.write_type(info, constraint, None, TYPE_FORMAT_FLAGS, verbosity);
            }
            if let Some(default) = self.state.get_default_from_type_parameter(parameter)? {
                info.write_operator(" = ");
                self.write_type(info, default, None, TYPE_FORMAT_FLAGS, verbosity);
            }
        }
        info.write_punctuation(">");
        Ok(())
    }

    /// tsgo's `tryExpandSymbol`: a class, interface, enum or namespace at a
    /// higher verbosity level (at level 0 only the signal that one would
    /// show more).
    fn try_expand_symbol(
        &mut self,
        info: &mut QuickInfo,
        symbol: SymbolId,
        meaning: SymbolFlags,
        node: Touching,
        verbosity: &mut VerbosityContext,
        symbol_was_expanded: &mut bool,
    ) -> CheckResult<bool> {
        if *symbol_was_expanded {
            return Ok(true);
        }
        if !self.can_expand_symbol(symbol, node, verbosity)? {
            return Ok(false);
        }
        // The declarations, one level down.
        let mut expand = VerbosityContext {
            level: verbosity.level - 1,
            max_truncation_length: verbosity.max_truncation_length,
            ..VerbosityContext::default()
        };
        let expanded = self
            .state
            .expand_symbol_for_hover(symbol, meaning, &mut expand)
            .unwrap_or_default();
        if expanded.is_empty() {
            return Ok(false);
        }
        verbosity.can_increase_verbosity |= expand.can_increase_verbosity;
        verbosity.truncated |= expand.truncated;
        info.write(&expanded);
        *symbol_was_expanded = true;
        Ok(true)
    }

    /// tsgo's `canExpandSymbol`.
    fn can_expand_symbol(
        &mut self,
        symbol: SymbolId,
        node: Touching,
        verbosity: &mut VerbosityContext,
    ) -> CheckResult<bool> {
        let flags = self.state.binder.symbol(symbol).flags;
        if !flags.intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE | SymbolFlags::NAMESPACE) {
            return Ok(false);
        }
        let ty = if flags.intersects(SymbolFlags::CLASS | SymbolFlags::INTERFACE) {
            self.state.get_declared_type_of_symbol(symbol)?
        } else {
            let location = match node {
                Touching::Node(id) => Some(id),
                Touching::Token { .. } => None,
            };
            self.state
                .get_type_of_symbol_at_location(symbol, location)?
        };
        if self.state.is_lib_type_for_hover_verbosity(ty) {
            return Ok(false);
        }
        if verbosity.level > 0 {
            return Ok(true);
        }
        verbosity.can_increase_verbosity = true;
        Ok(false)
    }

    /// tsgo `writeModuleImportAttributes`: an ambient module's `with { … }`.
    fn write_module_import_attributes(&mut self, info: &mut QuickInfo, symbol: SymbolId) {
        let attributes = self
            .state
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .find_map(
                |&declaration| match &self.state.binder.node_record(declaration).data {
                    NodeData::ModuleDeclaration(data) => data.attributes,
                    _ => None,
                },
            );
        let Some(attributes) = attributes else {
            return;
        };
        let display_parts = info.runs.is_some();
        if let Ok((text, parts)) = self
            .state
            .print_source_node_single_line(attributes, display_parts)
        {
            info.write_keyword(" with ");
            match parts {
                Some(parts) => info.write_parts(parts),
                None => info.write(&text),
            }
        }
    }

    /// tsgo's `writeTypeClassified`.
    fn write_type(
        &mut self,
        info: &mut QuickInfo,
        ty: TypeId,
        enclosing: Option<NodeId>,
        flags: u32,
        verbosity: &mut VerbosityContext,
    ) {
        let flags = flags | TYPE_FORMAT_MULTILINE_OBJECT_LITERALS;
        if info.runs.is_some() {
            if let Ok(Some(parts)) =
                self.state
                    .type_to_display_parts(ty, enclosing, flags, &mut |state, symbol| {
                        classification_for_symbol(state, symbol)
                    })
            {
                info.write_parts(parts);
                return;
            }
        }
        let text = self
            .state
            .type_to_string_with_flags(ty, enclosing, flags, Some(verbosity))
            .unwrap_or_default();
        info.write(&text);
    }

    /// tsgo's `writeSignatureClassified`.
    fn write_signature(
        &mut self,
        info: &mut QuickInfo,
        signature: SignatureId,
        enclosing: Option<NodeId>,
        flags: u32,
        verbosity: &mut VerbosityContext,
    ) {
        let flags = flags | TYPE_FORMAT_MULTILINE_OBJECT_LITERALS;
        if info.runs.is_some() {
            if let Ok(Some(parts)) = self.state.signature_to_display_parts(
                signature,
                enclosing,
                flags,
                &mut |state, symbol| classification_for_symbol(state, symbol),
            ) {
                info.write_parts(parts);
                return;
            }
        }
        let text = self
            .state
            .signature_to_string_with_flags(signature, enclosing, flags, Some(verbosity))
            .unwrap_or_default();
        info.write(&text);
    }

    fn write_symbol_name(
        &mut self,
        info: &mut QuickInfo,
        symbol: SymbolId,
        enclosing: Option<NodeId>,
        meaning: SymbolFlags,
        flags: SymbolFormatFlags,
    ) -> CheckResult<()> {
        let text = self
            .state
            .symbol_to_string_with_flags(symbol, enclosing, meaning, flags)?;
        // tsgo's `writeSymbolClassified`.
        info.write_symbol(&text, classification_for_symbol(self.state, symbol));
        Ok(())
    }

    /// A literal type's value as tsgo's `LiteralType.String()` writes it.
    fn literal_text(&self, ty: TypeId) -> String {
        self.state.literal_type_value_text(ty)
    }

    /// tsgo `getSignaturesAtLocation`: the symbol's signatures of `kind`,
    /// the resolved one for a call of several or a generic one.
    fn signatures_at_location(
        &mut self,
        symbol: SymbolId,
        kind: SignatureKind,
        node: Touching,
    ) -> CheckResult<Vec<SignatureId>> {
        let ty = self.state.get_type_of_symbol(symbol)?;
        let ty = self.state.remove_missing_or_undefined_type_public(ty)?;
        let signatures = self.state.get_signatures_of_type(ty, kind)?;
        let generic = |state: &tsc_checker::state::CheckerState<'_>, signature: SignatureId| {
            state
                .signature_of(signature)
                .type_parameters
                .as_ref()
                .is_some_and(|parameters| !parameters.is_empty())
        };
        if signatures.len() > 1 || signatures.len() == 1 && generic(self.state, signatures[0]) {
            if let Some(call) = self.call_or_new_expression(node) {
                return Ok(vec![self.state.get_resolved_signature(
                    call,
                    None,
                    tsc_types::CheckMode::NORMAL,
                )?]);
            }
        }
        Ok(signatures)
    }

    /// tsgo `getCallOrNewExpression`: the call or `new` the node names.
    pub(crate) fn call_or_new_expression(&self, node: Touching) -> Option<NodeId> {
        let Touching::Node(mut node) = node else {
            return None;
        };
        if self.node_kind(node) == SyntaxKind::SourceFile {
            return None;
        }
        let parent = self.state.binder.node_record(node).parent?;
        if let NodeData::PropertyAccessExpression(access) =
            &self.state.binder.node_record(parent).data
        {
            if access.name == Some(node) {
                node = parent;
            }
        }
        let parent = self.state.binder.node_record(node).parent?;
        match &self.state.binder.node_record(parent).data {
            NodeData::CallExpression(call) if call.expression == Some(node) => Some(parent),
            NodeData::NewExpression(new) if new.expression == Some(node) => Some(parent),
            _ => None,
        }
    }

    /// tsgo `getNodeForQuickInfo`.
    fn node_for_quick_info(&self, node: Touching) -> Touching {
        let Some(parent) = self.parent(node) else {
            return node;
        };
        let parent_record = self.state.binder.node_record(parent);
        match &parent_record.data {
            NodeData::NewExpression(new) if self.pos(node) == parent_record.pos => {
                if let Some(expression) = new.expression {
                    return Touching::Node(expression);
                }
            }
            NodeData::NamedTupleMember(_) if self.pos(node) == parent_record.pos => {
                return Touching::Node(parent);
            }
            NodeData::MetaProperty(meta) => {
                if Touching::Node(meta.name.unwrap_or(parent)) == node
                    && self.is_import_meta(parent)
                {
                    return Touching::Node(parent);
                }
            }
            NodeData::JsxNamespacedName(_) => return Touching::Node(parent),
            _ => {}
        }
        node
    }

    /// tsgo `getSymbolAtLocationForQuickInfo`: an object literal element's
    /// one contextual property, else the symbol at the node.
    fn symbol_at_location_for_quick_info(
        &mut self,
        node: Touching,
    ) -> CheckResult<Option<SymbolId>> {
        if let Touching::Node(id) = node {
            if let Some(element) = containing_object_literal_element(self.state, id) {
                let literal = self
                    .state
                    .binder
                    .node_record(element)
                    .parent
                    .expect("a literal");
                if let Some(contextual_type) = self
                    .state
                    .language_service_contextual_type(literal, tsc_types::ContextFlags::NONE)?
                {
                    let properties = self.state.get_property_symbols_from_contextual_type(
                        element,
                        contextual_type,
                        false,
                    )?;
                    if let [property] = properties[..] {
                        return Ok(Some(property));
                    }
                }
            }
        }
        self.symbol_at(node)
    }

    /// tsgo `shouldGetType`.
    fn should_get_type(&self, node: Touching) -> bool {
        let Touching::Node(id) = node else {
            return false;
        };
        match self.node_kind(id) {
            SyntaxKind::Identifier => {
                let parent = self.parent(node);
                !(self.has_js_doc_flag(id) && self.is_declaration_name(id)
                    || self.is_label_name(id)
                    || self.is_tag_name(id)
                    || parent.is_some_and(|parent| self.is_const_type_reference(parent)))
            }
            SyntaxKind::ThisKeyword
            | SyntaxKind::ThisType
            | SyntaxKind::SuperKeyword
            | SyntaxKind::NamedTupleMember => true,
            SyntaxKind::MetaProperty => self.is_import_meta(id),
            _ => false,
        }
    }

    /// tsgo `getContainerNode`.
    pub(crate) fn container_node(&self, node: Touching) -> Option<NodeId> {
        match node {
            Touching::Node(id) => {
                node_util::get_container_node(self.state.binder.source_of_node(id), id)
            }
            Touching::Token { parent, .. } => node_util::get_container_node_at_or_above(
                self.state.binder.source_of_node(parent),
                Some(parent),
            ),
        }
    }

    /// tsgo `GetTypeAtLocation` of a node or token.
    fn type_at(&mut self, node: Touching) -> CheckResult<TypeId> {
        match node {
            Touching::Node(id) => self.state.get_type_at_location(id),
            Touching::Token { kind, parent, .. } => self.state.get_type_at_token(kind, parent),
        }
    }

    // -- the tree ------------------------------------------------------------

    /// tsgo `ast.IsInExpressionContext`.
    fn is_in_expression_context(&self, node: Touching) -> bool {
        match node {
            Touching::Node(id) => {
                node_util::is_in_expression_context(self.state.binder.source_of_node(id), id)
            }
            Touching::Token { .. } => false,
        }
    }

    /// tsgo `ast.IsThisInTypeQuery`.
    fn is_this_in_type_query(&self, node: Touching) -> bool {
        let Touching::Node(mut id) = node else {
            return false;
        };
        let is_this = match &self.state.binder.node_record(id).data {
            NodeData::Identifier(identifier) => identifier.escaped_text.as_str() == Some("this"),
            _ => false,
        };
        if !is_this {
            return false;
        }
        loop {
            let Some(parent) = self.state.binder.node_record(id).parent else {
                return false;
            };
            match &self.state.binder.node_record(parent).data {
                NodeData::QualifiedName(name) if name.left == Some(id) => id = parent,
                _ => return self.node_kind(parent) == SyntaxKind::TypeQuery,
            }
        }
    }

    /// tsgo `ast.IsImportMeta`.
    pub(crate) fn is_import_meta(&self, node: NodeId) -> bool {
        match &self.state.binder.node_record(node).data {
            NodeData::MetaProperty(meta) => {
                meta.keyword_token == SyntaxKind::ImportKeyword
                    && meta.name.is_some_and(|name| {
                        node_util::id_text(self.state.binder.source_of_node(name), name)
                            == Some("meta")
                    })
            }
            _ => false,
        }
    }

    /// tsgo `ast.IsConstTypeReference`.
    fn is_const_type_reference(&self, node: NodeId) -> bool {
        let NodeData::TypeReference(reference) = &self.state.binder.node_record(node).data else {
            return false;
        };
        let no_arguments = reference
            .type_arguments
            .is_none_or(|arguments| self.state.binder.node_array(arguments).nodes.is_empty());
        no_arguments
            && reference.type_name.is_some_and(|name| {
                self.node_kind(name) == SyntaxKind::Identifier
                    && node_util::id_text(self.state.binder.source_of_node(name), name)
                        == Some("const")
            })
    }

    /// tsgo `ast.IsLabelName`.
    pub(crate) fn is_label_name(&self, node: NodeId) -> bool {
        let Some(parent) = self.state.binder.node_record(node).parent else {
            return false;
        };
        match &self.state.binder.node_record(parent).data {
            NodeData::LabeledStatement(statement) => statement.label == Some(node),
            NodeData::BreakStatement(statement) => statement.label == Some(node),
            NodeData::ContinueStatement(statement) => statement.label == Some(node),
            _ => false,
        }
    }

    /// tsgo `ast.IsTagName`.
    pub(crate) fn is_tag_name(&self, node: NodeId) -> bool {
        let Some(parent) = self.state.binder.node_record(node).parent else {
            return false;
        };
        is_jsdoc_tag_kind(self.node_kind(parent))
            && crate::jsdoc::tag_name(self.state, parent) == Some(node)
    }

    /// `NodeFlagsJSDoc`.
    pub(crate) fn has_js_doc_flag(&self, node: NodeId) -> bool {
        NodeFlags::from_bits(self.state.binder.node_record(node).flags).contains(NodeFlags::JS_DOC)
    }

    /// tsgo `ast.GetDeclarationOfKind`.
    fn declaration_of_kind(&self, symbol: SymbolId, kind: SyntaxKind) -> Option<NodeId> {
        self.declarations_of(symbol)
            .into_iter()
            .find(|&declaration| self.node_kind(declaration) == kind)
    }

    /// The local type parameters of a class or interface's declared type
    /// (tsgo `AsInterfaceType().LocalTypeParameters()`).
    fn local_type_parameters(&mut self, symbol: SymbolId) -> CheckResult<Vec<TypeId>> {
        let ty = self.state.get_declared_type_of_symbol(symbol)?;
        Ok(self.local_type_parameters_of_type(ty))
    }

    fn local_type_parameters_of_type(&self, ty: TypeId) -> Vec<TypeId> {
        let parts = self.state.get_interface_type_parts(ty);
        parts.type_parameters[parts.outer_type_parameter_count..].to_vec()
    }

    /// Whether the type is tsgo's `InterfaceType` (`AsInterfaceType() != nil`).
    fn is_interface_type(&self, ty: TypeId) -> bool {
        self.state
            .tables
            .object_flags_of(ty)
            .intersects(tsc_types::ObjectFlags::CLASS_OR_INTERFACE)
    }

    /// tsgo `GetTypeAliasTypeParameters`.
    fn type_alias_type_parameters(&mut self, symbol: SymbolId) -> CheckResult<Vec<TypeId>> {
        self.state.get_type_alias_type_parameters(symbol)
    }
}

fn kind_is(kind: SyntaxKind, expected: SyntaxKind) -> bool {
    kind == expected
}

/// tsgo `ast.IsJSDocTag`.
pub(crate) fn is_jsdoc_tag_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::JSDocTag
            | SyntaxKind::JSDocAugmentsTag
            | SyntaxKind::JSDocImplementsTag
            | SyntaxKind::JSDocAuthorTag
            | SyntaxKind::JSDocDeprecatedTag
            | SyntaxKind::JSDocClassTag
            | SyntaxKind::JSDocPublicTag
            | SyntaxKind::JSDocPrivateTag
            | SyntaxKind::JSDocProtectedTag
            | SyntaxKind::JSDocReadonlyTag
            | SyntaxKind::JSDocOverrideTag
            | SyntaxKind::JSDocCallbackTag
            | SyntaxKind::JSDocOverloadTag
            | SyntaxKind::JSDocEnumTag
            | SyntaxKind::JSDocParameterTag
            | SyntaxKind::JSDocReturnTag
            | SyntaxKind::JSDocThisTag
            | SyntaxKind::JSDocTypeTag
            | SyntaxKind::JSDocTemplateTag
            | SyntaxKind::JSDocTypedefTag
            | SyntaxKind::JSDocSeeTag
            | SyntaxKind::JSDocPropertyTag
            | SyntaxKind::JSDocThrowsTag
            | SyntaxKind::JSDocSatisfiesTag
            | SyntaxKind::JSDocImportTag
    )
}
