//! tsgo's completion snippets (ls/completions.go): a class member's
//! declaration (`getEntryForMemberCompletion`), an object literal's method
//! (`getEntryForObjectLiteralMethodCompletion`) and a switch's missing cases
//! (`getExhaustiveCaseSnippets`), printed by tsgo's snippet printer.

use tsc_binder::node_util;
use tsc_binder::SymbolId;
use tsc_checker::class::MemberOverrideStatus;
use tsc_checker::state::CheckResult;
use tsc_emitter::{SnippetElement, TransformError, TransformNode};
use tsc_lsproto::{Range, TextEdit};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, SymbolFlags};

use super::context::Completions;
use super::data::ObjectLiteralMethodOrigin;
use super::ClientSupport;
use crate::autoimport::ImportAdder;
use crate::change::{get_format_code_settings_for_writing, Tracker};
use crate::code_actions::{prepared_view, MissingMemberFixer, PRESERVE_OPTIONAL_PROPERTY};
use crate::language_service::LsError;
use crate::lsutil::new_line_kind;
use crate::utilities::Touching;

/// tsgo `memberCompletionEntry`.
pub(crate) struct MemberCompletionEntry {
    pub(crate) insert_text: String,
    pub(crate) filter_text: String,
    pub(crate) is_snippet: bool,
    pub(crate) additional_text_edits: Vec<TextEdit>,
}

/// tsgo `presentMemberModifiers`: the modifiers (and decorators) written
/// before the member being completed, and the range they take, which the
/// completion erases.
#[derive(Default)]
struct PresentMemberModifiers {
    modifiers: ModifierFlags,
    decorators: Vec<NodeId>,
    erase_range: Option<Range>,
}

fn transform_error(error: TransformError) -> LsError {
    LsError(error.to_string())
}

impl Completions<'_, '_, '_> {
    /// tsgo `isClassLikeMemberCompletion`: a class member's symbol at a class
    /// body's start, at the name that ends a class element, or in a class's
    /// member list (not in JavaScript).
    pub(crate) fn is_class_like_member_completion(
        &self,
        symbol: SymbolId,
        location: Touching,
    ) -> bool {
        let location_node = match location {
            Touching::Node(node) => node,
            Touching::Token { parent, .. } => parent,
        };
        if self.is_in_js_file(location_node) {
            return false;
        }
        let member_flags = SymbolFlags::CLASS_MEMBER & SymbolFlags::ENUM_MEMBER_EXCLUDES;
        if !self
            .request
            .state
            .symbol_flags(symbol)
            .intersects(member_flags)
        {
            return false;
        }
        let Touching::Node(location) = location else {
            return false;
        };
        if is_class_like_kind(self.node_kind(location)) {
            return true;
        }
        let Some(parent) = self.parent_of(location) else {
            return false;
        };
        self.parent_of(parent).is_some_and(|grandparent| {
            crate::utilities::is_class_element(self.node_kind(parent))
                && self.name_of(parent) == Some(location)
                && self.last_token(parent) == Some(Touching::Node(location))
                && is_class_like_kind(self.node_kind(grandparent))
        })
    }

    /// tsgo `getEntryForMemberCompletion`: the declaration of the member a
    /// class would take for `symbol` (the missing member fixer's, with an
    /// empty or tab stop body), its modifiers and the ones written before
    /// it combined (none when one written cannot go with it), printed by the
    /// snippet printer; the import edits it needs and the erasure of the
    /// written modifiers as additional edits.
    pub(crate) fn member_completion_entry(
        &mut self,
        symbol: SymbolId,
        name: &str,
        location: Touching,
        context_token: Option<Touching>,
    ) -> CheckResult<Result<Option<MemberCompletionEntry>, LsError>> {
        let start = match location {
            Touching::Node(node) => Some(node),
            Touching::Token { parent, .. } => Some(parent),
        };
        let Some(class_like) =
            self.find_ancestor(start, |this, node| is_class_like_kind(this.node_kind(node)))
        else {
            return Ok(Ok(None));
        };
        let source = self.source;
        let mut view = match prepared_view(self.request, self.file) {
            Ok(view) => view,
            Err(error) => return Ok(Err(error)),
        };
        let format_settings = self.request.preferences.format_code_settings.clone();
        let mut adder = view.as_ref().map(|_| {
            ImportAdder::new(
                source,
                self.request.state.options,
                format_settings.clone(),
                self.request.converters,
                self.request.preferences,
            )
        });
        let mut tracker = Tracker::new(
            new_line_kind(self.request.state.options),
            format_settings.clone(),
        );
        let target =
            tracker.source_in_program(source, self.request.state.program_source_id(self.file));
        let present = self.present_member_modifiers(context_token);
        let class_modifiers = node_util::get_syntactic_modifier_flags(source, class_like);
        let is_abstract = present.modifiers.intersects(ModifierFlags::ABSTRACT)
            && class_modifiers.intersects(ModifierFlags::ABSTRACT);
        let is_snippet = ClientSupport(self.request.capabilities).item_snippet();
        let body = match create_body(&mut tracker, target, is_snippet) {
            Ok(body) => body,
            Err(error) => return Ok(Err(transform_error(error))),
        };
        let method_not_implemented = tsc_diagnostics::gen::Method_not_implemented
            .template_in(self.request.catalog)
            .to_owned();
        let nodes = {
            let mut fixer = MissingMemberFixer {
                tracker: &mut tracker,
                file: source,
                target,
                options: self.request.state.options,
                preferences: self.request.preferences,
                import_adder: match (&mut adder, &mut view) {
                    (Some(adder), Some(view)) => Some((adder, view)),
                    _ => None,
                },
                method_not_implemented,
            };
            fixer.create_member_from_symbol(
                self.request.state,
                symbol,
                class_like,
                Some(body),
                PRESERVE_OPTIONAL_PROPERTY,
                is_abstract,
            )
        };
        let nodes = match nodes {
            Ok(nodes) => nodes,
            Err(error) => return Ok(Err(LsError(error))),
        };
        let mut additional_text_edits = Vec::new();
        if let Some(adder) = adder.filter(ImportAdder::has_fixes) {
            match adder.edits(&self.request.state.binder) {
                Ok(edits) => additional_text_edits.extend(edits),
                Err(error) => return Ok(Err(LsError(error.to_string()))),
            }
        }
        if let Some(range) = present.erase_range.clone() {
            additional_text_edits.push(TextEdit {
                range,
                new_text: String::new(),
            });
        }
        let mut modifiers = ModifierFlags::NONE;
        let mut completion_nodes = Vec::new();
        for node in nodes {
            if completion_nodes.is_empty() {
                let flags = transform_modifier_flags(&mut tracker, node);
                modifiers = flags;
                if is_abstract {
                    modifiers |= ModifierFlags::ABSTRACT;
                }
                let kind = match tracker.arena().node(node) {
                    Ok(record) => record.kind,
                    Err(error) => return Ok(Err(transform_error(error))),
                };
                if crate::utilities::is_class_element(kind)
                    && self.request.state.get_member_override_modifier_status(
                        class_like,
                        symbol,
                        flags.intersects(ModifierFlags::OVERRIDE),
                        flags.intersects(ModifierFlags::ABSTRACT),
                        flags.intersects(ModifierFlags::STATIC),
                    )? == MemberOverrideStatus::NeedsOverride
                {
                    modifiers |= ModifierFlags::OVERRIDE;
                }
            }
            completion_nodes.push(node);
        }
        if completion_nodes.is_empty() {
            return Ok(Ok(Some(MemberCompletionEntry {
                insert_text: name.to_owned(),
                filter_text: name.to_owned(),
                is_snippet,
                additional_text_edits,
            })));
        }
        let mut allowed_modifiers = modifiers | ModifierFlags::OVERRIDE | ModifierFlags::PUBLIC;
        if self
            .request
            .state
            .symbol_flags(symbol)
            .intersects(SymbolFlags::METHOD)
        {
            allowed_modifiers |= ModifierFlags::ASYNC;
        } else {
            allowed_modifiers |= ModifierFlags::AMBIENT | ModifierFlags::READONLY;
        }
        let allowed_and_present = present.modifiers & allowed_modifiers;
        if present.modifiers.bits() & !allowed_modifiers.bits() != 0 {
            return Ok(Ok(None));
        }
        if modifiers.intersects(ModifierFlags::PROTECTED)
            && allowed_and_present.intersects(ModifierFlags::PUBLIC)
        {
            modifiers =
                ModifierFlags::from_bits(modifiers.bits() & !ModifierFlags::PROTECTED.bits());
        }
        if allowed_and_present != ModifierFlags::NONE
            && !allowed_and_present.intersects(ModifierFlags::PUBLIC)
        {
            modifiers = ModifierFlags::from_bits(modifiers.bits() & !ModifierFlags::PUBLIC.bits());
        }
        modifiers |= allowed_and_present;
        let new_line = format_settings.editor_settings.new_line_character.clone();
        let decorated_node = (!present.decorators.is_empty())
            .then(|| *completion_nodes.last().expect("a completion node"))
            .filter(|&node| {
                tracker
                    .arena()
                    .node(node)
                    .is_ok_and(|record| node_util::can_have_decorators(record.kind))
            });
        let target_version = self.request.state.options.emit_script_target();
        let settings = get_format_code_settings_for_writing(&format_settings, source);
        let mut texts = Vec::new();
        for node in completion_nodes {
            let decorators: &[NodeId] = if Some(node) == decorated_node {
                &present.decorators
            } else {
                &[]
            };
            let replaced =
                match replace_modifiers(&mut tracker, target, node, modifiers, decorators) {
                    Ok(replaced) => replaced,
                    Err(error) => return Ok(Err(transform_error(error))),
                };
            match tracker.print_and_format_snippet(
                replaced,
                source,
                Some(target_version),
                &settings,
            ) {
                Ok(text) => texts.push(text),
                Err(error) => return Ok(Err(LsError(error.to_string()))),
            }
        }
        let insert_text = texts.join(&new_line);
        if insert_text.is_empty() {
            return Ok(Ok(None));
        }
        Ok(Ok(Some(MemberCompletionEntry {
            insert_text,
            filter_text: name.to_owned(),
            is_snippet,
            additional_text_edits,
        })))
    }

    /// tsgo `getPresentMemberModifiers`: for a modifier keyword (or a name
    /// that is one) of a property declaration on the position's line, the
    /// property's modifiers and decorators, from the first to the name.
    fn present_member_modifiers(&self, context_token: Option<Touching>) -> PresentMemberModifiers {
        let Some(context) = context_token else {
            return PresentMemberModifiers::default();
        };
        if self.line_of(self.position) > self.line_of(self.end(context)) {
            return PresentMemberModifiers::default();
        }
        let mut modifiers = ModifierFlags::NONE;
        let mut decorators = Vec::new();
        let mut range_pos = self.position as usize;
        let mut range_end = self.position as usize;
        let parent = self.parent(context);
        if let Some(parent) =
            parent.filter(|&parent| self.node_kind(parent) == SyntaxKind::PropertyDeclaration)
        {
            let Some(context_modifier_kind) = self.modifier_like_kind(context) else {
                return PresentMemberModifiers::default();
            };
            let modifier_nodes = node_util::modifiers_of(self.source, parent)
                .map_or_else(Vec::new, |list| {
                    self.source.arena.node_array(list).nodes.to_vec()
                });
            if !modifier_nodes.is_empty() {
                modifiers |= node_util::modifiers_to_flags(
                    self.source,
                    node_util::modifiers_of(self.source, parent),
                ) & ModifierFlags::MODIFIER;
                for modifier in modifier_nodes {
                    if self.node_kind(modifier) == SyntaxKind::Decorator {
                        decorators.push(modifier);
                    }
                    range_pos = range_pos.min(self.node_start(modifier) as usize);
                }
            }
            let context_modifier_flag = node_util::modifier_to_flag(context_modifier_kind);
            if !modifiers.intersects(context_modifier_flag) {
                modifiers |= context_modifier_flag;
                range_pos = range_pos.min(self.start(context) as usize);
            }
            if let Some(name) = self.name_of(parent) {
                if Touching::Node(name) != context {
                    range_end = self.node_start(name) as usize;
                }
            }
        }
        let erase_range = (range_pos < range_end)
            .then(|| self.request.lsp_range(self.source, range_pos..range_end));
        PresentMemberModifiers {
            modifiers,
            decorators,
            erase_range,
        }
    }

    /// tsgo `modifierLikeKind`: a modifier's kind, or the modifier keyword
    /// an identifier spells.
    fn modifier_like_kind(&self, node: Touching) -> Option<SyntaxKind> {
        let kind = self.kind(node);
        if node_util::is_modifier_kind(kind) {
            return Some(kind);
        }
        if let Touching::Node(id) = node {
            if let NodeData::Identifier(identifier) = self.data(id) {
                return tsc_syntax::identifier_to_keyword_kind(
                    &identifier.escaped_text.unescape().to_string_lossy(),
                )
                .filter(|&keyword| node_util::is_modifier_kind(keyword));
            }
        }
        None
    }
}

impl Completions<'_, '_, '_> {
    /// tsgo `getEntryForObjectLiteralMethodCompletion` and
    /// `createObjectLiteralMethod`: for a property or method of the
    /// contextual type whose (widened, subtype-reduced) type has one call
    /// signature, the method an object literal would take — the signature's
    /// parameters without types, an empty or tab stop body — printed by the
    /// snippet printer with a comma, and its parameter list as the label's
    /// detail.
    pub(crate) fn object_literal_method_entry(
        &mut self,
        symbol: SymbolId,
        enclosing_declaration: NodeId,
    ) -> CheckResult<Option<ObjectLiteralMethodOrigin>> {
        let Some(declaration) = self
            .request
            .state
            .binder
            .symbol(symbol)
            .declarations
            .first()
            .copied()
        else {
            return Ok(None);
        };
        if !matches!(
            self.node_kind(declaration),
            SyntaxKind::PropertySignature
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::MethodSignature
                | SyntaxKind::MethodDeclaration
        ) {
            return Ok(None);
        }
        let state = &mut *self.request.state;
        let ty = state.get_type_of_symbol_at_location(symbol, Some(enclosing_declaration))?;
        let mut effective = state.get_widened_type(ty)?;
        if state
            .tables
            .flags_of(effective)
            .intersects(tsc_types::TypeFlags::UNION)
        {
            let types = state.get_types_of_type(effective);
            if types.len() < 10 {
                effective =
                    state.get_union_type_ex(&types, tsc_types::tables::UnionReduction::Subtype)?;
            }
        }
        if state
            .tables
            .flags_of(effective)
            .intersects(tsc_types::TypeFlags::UNION)
        {
            let mut function_type = None;
            for member in state.get_types_of_type(effective) {
                if state
                    .get_signatures_of_type(member, tsc_checker::state::SignatureKind::Call)?
                    .is_empty()
                {
                    continue;
                }
                if function_type.is_some() {
                    return Ok(None);
                }
                function_type = Some(member);
            }
            let Some(function_type) = function_type else {
                return Ok(None);
            };
            effective = function_type;
        }
        if state
            .get_signatures_of_type(effective, tsc_checker::state::SignatureKind::Call)?
            .len()
            != 1
        {
            return Ok(None);
        }
        // tsgo `nodebuilder.FlagsOmitThisParameter`, with
        // `FlagsUseSingleQuotesForStringLiteralType` by the quote preference.
        let mut flags = 1 << 25;
        if crate::lsutil::get_quote_preference(self.source, self.request.preferences)
            == crate::lsutil::QuotePreference::SINGLE
        {
            flags |= 1 << 28;
        }
        let source = self.source;
        let format_settings = self.request.preferences.format_code_settings.clone();
        let mut tracker = Tracker::new(
            new_line_kind(self.request.state.options),
            format_settings.clone(),
        );
        let target =
            tracker.source_in_program(source, self.request.state.program_source_id(self.file));
        let built = self
            .request
            .state
            .type_to_type_node_in_arena(
                tracker.arena(),
                target,
                effective,
                Some(enclosing_declaration),
                flags,
                0,
            )
            .unwrap_or_else(|error| panic!("object literal method type node: {error}"));
        let Some((type_node, _)) = built else {
            return Ok(None);
        };
        let is_snippet = ClientSupport(self.request.capabilities).item_snippet();
        let declaration_source = self.source_of(declaration);
        let declaration_target = tracker.source_in_program(
            declaration_source,
            self.request
                .state
                .program_source_id(self.request.state.binder.file_index_of_node(declaration)),
        );
        let name =
            node_util::name_field_of(declaration_source, declaration).expect("a member's name");
        let method = create_object_literal_method(
            &mut tracker,
            target,
            type_node,
            TransformNode::new(declaration_target, name),
            is_snippet,
        )
        .unwrap_or_else(|error| panic!("object literal method: {error}"));
        let Some((method, parameters)) = method else {
            return Ok(None);
        };
        let target_version = self.request.state.options.emit_script_target();
        let settings = get_format_code_settings_for_writing(&format_settings, source);
        let insert_text = tracker
            .print_and_format_snippet(method, source, Some(target_version), &settings)
            .unwrap_or_else(|error| panic!("object literal method snippet: {error}"));
        // tsgo `printObjectLiteralMethodLabelDetail`: the method's signature
        // with an empty name.
        let new_line = if format_settings.editor_settings.new_line_character == "\r\n" {
            tsc_emitter::NewLineKind::CarriageReturnLineFeed
        } else {
            tsc_emitter::NewLineKind::LineFeed
        };
        let detail = (|| -> Result<String, String> {
            let name = tracker
                .arena()
                .factory()
                .create_identifier(target, "")
                .map_err(|error| error.to_string())?;
            let signature = tracker
                .arena()
                .factory()
                .create_method_signature(target, None, name, None, None, parameters, None)
                .map_err(|error| error.to_string())?;
            tracker
                .print_signature_label(signature, target_version, new_line)
                .map_err(|error| error.to_string())
        })()
        .unwrap_or_else(|error| panic!("object literal method label: {error}"));
        Ok(Some(ObjectLiteralMethodOrigin {
            insert_text: format!("{insert_text},"),
            detail,
            is_snippet,
        }))
    }
}

/// tsgo `createObjectLiteralMethod`'s nodes: a method named as the member,
/// with the parameters of the function type node (their `...`, names and
/// initializers, without types) and an empty or tab stop body; with its
/// parameter list.
fn create_object_literal_method(
    tracker: &mut Tracker<'_>,
    target: tsc_emitter::TransformSourceId,
    type_node: TransformNode,
    name: TransformNode,
    is_snippet: bool,
) -> Result<Option<(TransformNode, tsc_emitter::TransformNodeArray)>, TransformError> {
    let NodeData::FunctionType(function) = tracker.arena().node(type_node)?.data.clone() else {
        return Ok(None);
    };
    let type_parameters = function
        .parameters
        .map(|list| tsc_emitter::TransformNodeArray::new(type_node.source(), list));
    let mut parameters = Vec::new();
    if let Some(list) = type_parameters {
        let ids = tracker.arena().node_array(list)?.nodes.to_vec();
        for id in ids {
            let parameter = TransformNode::new(type_node.source(), id);
            let NodeData::Parameter(data) = tracker.arena().node(parameter)?.data.clone() else {
                continue;
            };
            let child =
                |id: Option<NodeId>| id.map(|id| TransformNode::new(type_node.source(), id));
            let parameter_name = child(data.name).expect("a parameter's name");
            let name = tracker.arena().factory().clone_node(parameter_name)?;
            parameters.push(tracker.arena().factory().create_parameter_declaration(
                target,
                None,
                child(data.dot_dot_dot_token),
                name,
                None,
                None,
                child(data.initializer),
            )?);
        }
    }
    let parameters = tracker
        .arena()
        .factory()
        .create_node_array(target, parameters)?;
    let body = create_body(tracker, target, is_snippet)?;
    let name = tracker
        .arena()
        .factory()
        .clone_node_to_source(name, target)?;
    let method = tracker.arena().factory().create_method_declaration(
        target,
        None,
        None,
        name,
        None,
        None,
        parameters,
        None,
        Some(body),
    )?;
    Ok(Some((method, parameters)))
}

/// tsgo's member body: an empty block, or the snippet's tab stop
/// (`createSnippetTabStopBody`).
fn create_body(
    tracker: &mut Tracker<'_>,
    target: tsc_emitter::TransformSourceId,
    is_snippet: bool,
) -> Result<TransformNode, TransformError> {
    let mut factory = tracker.arena().factory();
    let statements = if is_snippet {
        let empty = factory.create_empty_statement(target)?;
        tracker
            .arena()
            .metadata_mut(empty)
            .set_snippet_element(SnippetElement::TabStop(0));
        vec![empty]
    } else {
        Vec::new()
    };
    let mut factory = tracker.arena().factory();
    let statements = factory.create_node_array(target, statements)?;
    factory.create_block(target, statements, true)
}

/// tsgo `node.ModifierFlags()` of a built node: its modifiers' flags.
fn transform_modifier_flags(tracker: &mut Tracker<'_>, node: TransformNode) -> ModifierFlags {
    let arena = tracker.arena();
    let Ok(record) = arena.node(node) else {
        return ModifierFlags::NONE;
    };
    let Some(list) = node_util::modifiers_of_data(&record.data) else {
        return ModifierFlags::NONE;
    };
    let Ok(array) = arena.node_array(tsc_emitter::TransformNodeArray::new(node.source(), list))
    else {
        return ModifierFlags::NONE;
    };
    let mut flags = ModifierFlags::NONE;
    for &modifier in array.nodes {
        if let Some(modifier) = arena.node_ref(node.source(), modifier) {
            if let Ok(record) = arena.node(modifier) {
                flags |= node_util::modifier_to_flag(record.kind);
            }
        }
    }
    flags
}

/// tsgo `ast.ReplaceModifiers(node, createModifierList(flags, decorators))`:
/// the decorators (cloned) then the modifiers of the flags.
fn replace_modifiers(
    tracker: &mut Tracker<'_>,
    target: tsc_emitter::TransformSourceId,
    node: TransformNode,
    flags: ModifierFlags,
    decorators: &[NodeId],
) -> Result<TransformNode, TransformError> {
    let mut nodes = Vec::new();
    for &decorator in decorators {
        let decorator = TransformNode::new(target, decorator);
        nodes.push(tracker.arena().factory().clone_node(decorator)?);
    }
    let mut factory = tracker.arena().factory();
    if let Some(modifiers) = factory.create_modifiers_from_modifier_flags(target, flags)? {
        let arena = tracker.arena();
        let ids = arena.node_array(modifiers)?.nodes.to_vec();
        let modifiers: Vec<TransformNode> = ids
            .into_iter()
            .filter_map(|modifier| arena.node_ref(target, modifier))
            .collect();
        nodes.extend(modifiers);
    }
    let mut factory = tracker.arena().factory();
    let list = if nodes.is_empty() {
        None
    } else {
        Some(factory.create_node_array(target, nodes)?)
    };
    tracker.arena().factory().replace_modifiers(node, list)
}

/// tsgo `ast.IsClassLike`.
fn is_class_like_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
    )
}

impl Completions<'_, '_, '_> {
    /// tsgo `getExhaustiveCaseSnippets`: for a switch over a union of
    /// literals, a case clause for each member no clause has (an enum
    /// member's by its auto-importable type node as an expression, a
    /// literal's as itself), printed by the snippet printer with a tab stop
    /// after each, as one snippet item named by the first; with the import
    /// edits the enum members need.
    pub(crate) fn exhaustive_case_item(
        &mut self,
        case_block: NodeId,
    ) -> CheckResult<Result<Option<tsc_lsproto::CompletionItem>, LsError>> {
        let Some(switch) = self.parent_of(case_block) else {
            return Ok(Ok(None));
        };
        let NodeData::SwitchStatement(statement) = self.data(switch) else {
            return Ok(Ok(None));
        };
        let Some(expression) = statement.expression else {
            return Ok(Ok(None));
        };
        let switch_type = self.request.state.get_type_at_location(expression)?;
        let tables = &self.request.state.tables;
        if !tables
            .flags_of(switch_type)
            .intersects(tsc_types::TypeFlags::UNION)
        {
            return Ok(Ok(None));
        }
        let members = self.request.state.get_types_of_type(switch_type);
        let literal_flags = tsc_types::TypeFlags::STRING_LITERAL
            | tsc_types::TypeFlags::NUMBER_LITERAL
            | tsc_types::TypeFlags::BIG_INT_LITERAL;
        if !members.iter().all(|&member| {
            self.request
                .state
                .tables
                .flags_of(member)
                .intersects(literal_flags)
        }) {
            return Ok(Ok(None));
        }
        // The values the clauses have.
        let mut tracker = self.case_clause_tracker(case_block)?;
        let source = self.source;
        let quote_preference =
            crate::lsutil::get_quote_preference(source, self.request.preferences);
        let single_quote = quote_preference == crate::lsutil::QuotePreference::SINGLE;
        let mut view =
            if crate::utilities::is_dynamic_file_name(&source.file_name.to_string_lossy()) {
                None
            } else {
                match prepared_view(self.request, self.file) {
                    Ok(view) => view,
                    Err(error) => return Ok(Err(error)),
                }
            };
        let format_settings = self.request.preferences.format_code_settings.clone();
        let mut adder: Option<ImportAdder<'_>> = view.as_ref().map(|_| {
            ImportAdder::new(
                source,
                self.request.state.options,
                format_settings.clone(),
                self.request.converters,
                self.request.preferences,
            )
        });
        let mut change_tracker = Tracker::new(
            new_line_kind(self.request.state.options),
            format_settings.clone(),
        );
        let target = change_tracker
            .source_in_program(source, self.request.state.program_source_id(self.file));
        let mut elements = Vec::new();
        for member in members {
            let flags = self.request.state.tables.flags_of(member);
            if flags.intersects(tsc_types::TypeFlags::ENUM_LITERAL) {
                // An enum member, unless a clause has its value.
                let value_declaration = self
                    .request
                    .state
                    .tables
                    .type_of(member)
                    .symbol
                    .and_then(|symbol| self.request.state.binder.symbol(symbol).value_declaration);
                if let Some(declaration) = value_declaration {
                    if let Some(value) = self.request.state.get_constant_value(declaration)? {
                        if tracker.has_constant(&value) {
                            continue;
                        }
                        tracker.add_constant(&value);
                    }
                }
                let type_node = crate::autoimport::type_to_auto_importable_type_node(
                    self.request.state,
                    change_tracker.arena(),
                    target,
                    match (&mut adder, &mut view) {
                        (Some(adder), Some(view)) => Some((adder, view)),
                        _ => None,
                    },
                    member,
                    case_block,
                )?;
                let Some(type_node) = type_node else {
                    return Ok(Ok(None));
                };
                let expression =
                    type_node_to_expression(&mut change_tracker, target, type_node, single_quote)
                        .unwrap_or_else(|error| panic!("case snippet expression: {error}"));
                let Some(expression) = expression else {
                    return Ok(Ok(None));
                };
                elements.push(expression);
            } else if let Some(value) = self.literal_value(member) {
                if tracker.has_value(&value) {
                    continue;
                }
                let expression =
                    literal_expression(&mut change_tracker, target, &value, single_quote)
                        .unwrap_or_else(|error| panic!("case snippet literal: {error}"));
                elements.push(expression);
            }
        }
        if elements.is_empty() {
            return Ok(Ok(None));
        }
        let mut clauses = Vec::new();
        for element in elements {
            let mut factory = change_tracker.arena().factory();
            let statements = factory
                .create_node_array(target, Vec::new())
                .unwrap_or_else(|error| panic!("case clause: {error}"));
            clauses.push(
                change_tracker
                    .arena()
                    .factory()
                    .create_case_clause(target, element, statements)
                    .unwrap_or_else(|error| panic!("case clause: {error}")),
            );
        }
        let is_snippet = ClientSupport(self.request.capabilities).item_snippet();
        let new_line = format_settings.editor_settings.new_line_character.clone();
        let mut texts = Vec::new();
        for (index, &clause) in clauses.iter().enumerate() {
            let text = if is_snippet {
                let printed = change_tracker
                    .print_and_format_snippet(clause, source, None, &format_settings)
                    .unwrap_or_else(|error| panic!("case snippet: {error}"));
                format!("{printed}${}", index + 1)
            } else {
                change_tracker
                    .print_unescaped_snippet(clause, source, None, &format_settings)
                    .unwrap_or_else(|error| panic!("case snippet: {error}"))
            };
            texts.push(text);
        }
        let insert_text = texts.join(&new_line);
        let first_clause = change_tracker
            .print_unescaped_snippet(clauses[0], source, None, &format_settings)
            .unwrap_or_else(|error| panic!("case snippet: {error}"));
        let name = format!("{first_clause} ...");
        let data_file = self.data_file();
        let mut additional_text_edits = None;
        if let Some(adder) = adder {
            match adder.edits(&self.request.state.binder) {
                Ok(edits) if !edits.is_empty() => additional_text_edits = Some(edits),
                Ok(_) => {}
                Err(error) => return Ok(Err(LsError(error))),
            }
        }
        Ok(Ok(Some(tsc_lsproto::CompletionItem {
            label: name.clone(),
            kind: Some(tsc_lsproto::CompletionItemKind::SNIPPET),
            sort_text: Some(super::sort_text::GLOBALS_OR_KEYWORDS.to_owned()),
            insert_text: Some(insert_text),
            additional_text_edits,
            insert_text_format: is_snippet.then_some(tsc_lsproto::InsertTextFormat::SNIPPET),
            data: Some(tsc_lsproto::CompletionItemData {
                file_name: data_file.file_name,
                position: self.position as i32,
                supplemental_file_index: data_file.supplemental_file_index,
                name,
                source: super::SOURCE_SWITCH_CASES.to_owned(),
                ..tsc_lsproto::CompletionItemData::default()
            }),
            ..tsc_lsproto::CompletionItem::default()
        })))
    }
}

/// A case snippet's literal: a string in the preferred quotes, a number or
/// bigint (a negative one under `-`).
fn literal_expression(
    tracker: &mut Tracker<'_>,
    target: tsc_emitter::TransformSourceId,
    value: &tsc_types::LiteralValue,
    single_quote: bool,
) -> Result<TransformNode, TransformError> {
    let mut factory = tracker.arena().factory();
    match value {
        tsc_types::LiteralValue::BigInt(value) => {
            let text = format!("{}n", value.base10_value);
            let literal = factory.create_big_int_literal(target, text)?;
            if value.negative {
                return tracker.arena().factory().create_prefix_unary_expression(
                    target,
                    SyntaxKind::MinusToken,
                    literal,
                );
            }
            Ok(literal)
        }
        tsc_types::LiteralValue::Number(value) => {
            if *value < 0.0 {
                let literal = factory
                    .create_numeric_literal(target, tsc_types::js_number_to_string(-value))?;
                return tracker.arena().factory().create_prefix_unary_expression(
                    target,
                    SyntaxKind::MinusToken,
                    literal,
                );
            }
            factory.create_numeric_literal(target, tsc_types::js_number_to_string(*value))
        }
        tsc_types::LiteralValue::String(text) => {
            factory.create_string_literal(target, text.to_js_string(), single_quote)
        }
    }
}

/// tsgo `typeNodeToExpression`: an enum member's type node as the
/// expression that names it.
fn type_node_to_expression(
    tracker: &mut Tracker<'_>,
    target: tsc_emitter::TransformSourceId,
    type_node: TransformNode,
    single_quote: bool,
) -> Result<Option<TransformNode>, TransformError> {
    let source = type_node.source();
    let child = |id: Option<NodeId>| id.map(|id| TransformNode::new(source, id));
    match tracker.arena().node(type_node)?.data.clone() {
        NodeData::TypeReference(reference) => match child(reference.type_name) {
            Some(name) => entity_name_to_expression(tracker, target, name).map(Some),
            None => Ok(None),
        },
        NodeData::IndexedAccessType(access) => {
            let (Some(object), Some(index)) = (child(access.object_type), child(access.index_type))
            else {
                return Ok(None);
            };
            let object = type_node_to_expression(tracker, target, object, single_quote)?;
            let index = type_node_to_expression(tracker, target, index, single_quote)?;
            match (object, index) {
                (Some(object), Some(index)) => tracker
                    .arena()
                    .factory()
                    .create_element_access_expression(target, object, index)
                    .map(Some),
                _ => Ok(None),
            }
        }
        NodeData::LiteralType(literal) => {
            let Some(literal) = child(literal.literal) else {
                return Ok(None);
            };
            match tracker.arena().node(literal)?.data.clone() {
                NodeData::StringLiteral(text) => tracker
                    .arena()
                    .factory()
                    .create_string_literal(target, text.text, single_quote)
                    .map(Some),
                NodeData::NumericLiteral(number) => tracker
                    .arena()
                    .factory()
                    .create_numeric_literal(target, number.text)
                    .map(Some),
                _ => Ok(None),
            }
        }
        NodeData::ParenthesizedType(parenthesized) => {
            let Some(inner) = child(parenthesized.r#type) else {
                return Ok(None);
            };
            let Some(expression) = type_node_to_expression(tracker, target, inner, single_quote)?
            else {
                return Ok(None);
            };
            if tracker.arena().node(expression)?.kind == SyntaxKind::Identifier {
                return Ok(Some(expression));
            }
            tracker
                .arena()
                .factory()
                .create_parenthesized_expression(target, expression)
                .map(Some)
        }
        NodeData::TypeQuery(query) => match child(query.expr_name) {
            Some(name) => entity_name_to_expression(tracker, target, name).map(Some),
            None => Ok(None),
        },
        _ => Ok(None),
    }
}

/// tsgo `entityNameToExpression`: an identifier as it is, a qualified name
/// as a property access.
fn entity_name_to_expression(
    tracker: &mut Tracker<'_>,
    target: tsc_emitter::TransformSourceId,
    name: TransformNode,
) -> Result<TransformNode, TransformError> {
    let NodeData::QualifiedName(qualified) = tracker.arena().node(name)?.data.clone() else {
        return Ok(name);
    };
    let source = name.source();
    let left = TransformNode::new(source, qualified.left.expect("a qualified name's left"));
    let right = TransformNode::new(source, qualified.right.expect("a qualified name's right"));
    let left = entity_name_to_expression(tracker, target, left)?;
    tracker
        .arena()
        .factory()
        .create_property_access_expression(target, left, right)
}
