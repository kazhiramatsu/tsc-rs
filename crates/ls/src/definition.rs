//! tsgo `ls/definition.go`: go to definition and go to type definition.

use std::collections::HashSet;
use std::ops::Range;

use tsc_binder::node_util;
use tsc_checker::state::SignatureKind;
use tsc_checker::state::{CheckResult, CheckerState};
use tsc_lsproto::{
    DocumentUri, Location, LocationLink, LocationOrLocationsOrDefinitionLinksOrNull, Position,
};
use tsc_program::uri::file_name_to_document_uri;
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{ContextFlags, ModifierFlags, SymbolFlags, SymbolId, TypeFlags, TypeId};

use crate::astnav::Navigator;
use crate::language_service::{LanguageService, LsError};
use crate::request::Request;
use crate::utilities::{
    can_have_symbol, containing_object_literal_element, has_declaration_data, is_class_element,
    is_function_like_kind, is_object_literal_element, is_property_name, range_of_node,
    reference_at_position, script_of, token_start, touching, ReferenceInfo, Touching,
};

/// tsgo `DefinitionResponse` and `TypeDefinitionResponse`.
pub type DefinitionResponse = LocationOrLocationsOrDefinitionLinksOrNull;

impl LanguageService<'_> {
    /// tsgo `ProvideDefinition` (the source definition a user can prefer
    /// comes with the other navigation features).
    pub fn provide_definition(
        &mut self,
        uri: &DocumentUri,
        position: &Position,
    ) -> Result<DefinitionResponse, LsError> {
        let links = self.capabilities.text_document.definition.link_support;
        let file = self.file(uri);
        let result = self.with_definitions(file, position, links, |definitions, position| {
            definitions.definition_at_position(file, position)
        })?;
        Ok(combine_definition_responses(result, links))
    }

    /// tsgo `ProvideTypeDefinition`.
    pub fn provide_type_definition(
        &mut self,
        uri: &DocumentUri,
        position: &Position,
    ) -> Result<DefinitionResponse, LsError> {
        let links = self.capabilities.text_document.type_definition.link_support;
        let file = self.file(uri);
        let result = self.with_definitions(file, position, links, |definitions, position| {
            definitions.type_definition_at_position(file, position)
        })?;
        Ok(combine_definition_responses(result, links))
    }

    /// Run `find` over the program's query checker at the position of a
    /// document without a content mapper (one position).
    fn with_definitions(
        &mut self,
        file: usize,
        position: &Position,
        links: bool,
        find: impl FnOnce(&mut Request<'_, '_>, u32) -> CheckResult<DefinitionResponse>,
    ) -> Result<Vec<DefinitionResponse>, LsError> {
        Ok(self
            .with_request(file, position, links, find)?
            .into_iter()
            .collect())
    }
}

/// tsgo `combineDefinitionResponses`: the links (or locations) of the
/// positions' answers, each target once.
fn combine_definition_responses(
    results: Vec<DefinitionResponse>,
    links: bool,
) -> DefinitionResponse {
    let mut locations: Vec<Location> = Vec::new();
    let mut definition_links: Vec<LocationLink> = Vec::new();
    let mut seen: Vec<Location> = Vec::new();
    let mut add = |location: Location, link: LocationLink| {
        if !seen.contains(&location) {
            seen.push(location.clone());
            locations.push(location);
            definition_links.push(link);
        }
    };
    for result in results {
        match result {
            DefinitionResponse::DefinitionLinks(result_links) => {
                for link in result_links {
                    let location = Location {
                        uri: link.target_uri.clone(),
                        range: link.target_selection_range.clone(),
                    };
                    add(location, link);
                }
            }
            DefinitionResponse::Location(location) => {
                let link = link_of_location(&location);
                add(location, link);
            }
            DefinitionResponse::Locations(result_locations) => {
                for location in result_locations {
                    let link = link_of_location(&location);
                    add(location, link);
                }
            }
            DefinitionResponse::Null => {}
        }
    }
    if links {
        DefinitionResponse::DefinitionLinks(definition_links)
    } else {
        DefinitionResponse::Locations(locations)
    }
}

fn link_of_location(location: &Location) -> LocationLink {
    LocationLink {
        origin_selection_range: None,
        target_uri: location.uri.clone(),
        target_range: location.range.clone(),
        target_selection_range: location.range.clone(),
    }
}

/// tsgo `createLocationsFromLinks`.
fn locations_from_links(links: Vec<LocationLink>) -> DefinitionResponse {
    DefinitionResponse::Locations(
        links
            .into_iter()
            .map(|link| Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect(),
    )
}

/// tsgo `lspRangeContains`.
fn lsp_range_contains(outer: &tsc_lsproto::Range, inner: &tsc_lsproto::Range) -> bool {
    let before = |a: &Position, b: &Position| (a.line, a.character) <= (b.line, b.character);
    before(&outer.start, &inner.start) && before(&inner.end, &outer.end)
}

/// The call-like node a callee belongs to (tsgo `isCalleeWorker`'s
/// predicate and callee selector).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Callee {
    Call,
    New,
    CallOrNew,
    TaggedTemplate,
    Decorator,
    JsxOpeningLikeElement,
}

impl<'a> Request<'_, 'a> {
    // -- tsgo ProvideDefinition ---------------------------------------------

    /// tsgo `provideDefinitionAtPosition`.
    fn definition_at_position(
        &mut self,
        file: usize,
        position: u32,
    ) -> CheckResult<DefinitionResponse> {
        let source = self.state.binder.source(file);
        let navigator = Navigator::new(source);
        let found = navigator.touching_property_name(position);
        let reference = reference_at_position(self.state, self.prepared, file, position);
        if navigator.kind(found) == SyntaxKind::SourceFile {
            return Ok(DefinitionResponse::Null);
        }
        let origin = self.lsp_range(
            source,
            navigator.start_of_node(found, false) as usize..navigator.end(found) as usize,
        );
        if let Some(reference) = reference
            .as_ref()
            .filter(|reference| reference.file.is_some())
        {
            return Ok(self.definition_locations(origin, &[], Some(reference)));
        }
        let Some(node) = touching(found) else {
            return Ok(self.definition_locations(origin, &[], reference.as_ref()));
        };

        let kind = self.kind(node);
        if kind == SyntaxKind::OverrideKeyword {
            if let Some(symbol) = self.symbol_for_overridden_member(node)? {
                let declarations = self.declarations_of(symbol);
                return Ok(self.definition_locations(origin, &declarations, None));
            }
        }
        if let Touching::Node(id) = node {
            if self.is_jump_statement_target(id) {
                let parent = self.parent(node).expect("a jump statement");
                if let Some(label) = self.target_label(parent, id) {
                    return Ok(self.definition_locations(origin, &[label], None));
                }
            }
        }
        let parent = self.parent(node);
        if kind == SyntaxKind::CaseKeyword
            || kind == SyntaxKind::DefaultKeyword
                && parent.is_some_and(|parent| self.node_kind(parent) == SyntaxKind::DefaultClause)
        {
            if let Some(statement) = self.find_ancestor(parent, |definitions, node| {
                definitions.node_kind(node) == SyntaxKind::SwitchStatement
            }) {
                let source = self.state.binder.source_of_node(statement);
                let pos = self.state.binder.node_record(statement).pos as usize;
                let (start, end) = node_util::get_span_of_token_at_position(source, pos);
                let location = self.source_maps.location(source, start..end);
                return Ok(DefinitionResponse::Location(location));
            }
        }
        if matches!(
            kind,
            SyntaxKind::ReturnKeyword | SyntaxKind::YieldKeyword | SyntaxKind::AwaitKeyword
        ) {
            let start = match node {
                Touching::Node(id) => Some(id),
                Touching::Token { parent, .. } => Some(parent),
            };
            if let Some(function) = self.find_ancestor(start, |definitions, node| {
                node_util::is_function_like_declaration_kind(definitions.node_kind(node))
            }) {
                return Ok(self.definition_locations(origin, &[function], None));
            }
        }

        let mut declarations = self.declarations_from_location(node)?;
        if let Some(called) = self.signature_declaration(node)? {
            let jsx_constructor = parent.is_some_and(|parent| {
                matches!(
                    self.node_kind(parent),
                    SyntaxKind::JsxOpeningElement | SyntaxKind::JsxSelfClosingElement
                )
            }) && matches!(
                self.node_kind(called),
                SyntaxKind::Constructor
                    | SyntaxKind::ConstructorType
                    | SyntaxKind::CallSignature
                    | SyntaxKind::ConstructSignature
            );
            if !jsx_constructor {
                let name = self.declaration_name_for_keyword(node);
                let symbol = self.symbol_at(name)?;
                let mut matches = false;
                if let Some(symbol) = symbol {
                    for root in self.state.get_root_symbols(symbol)? {
                        if self.symbol_matches_signature(root, called) {
                            matches = true;
                            break;
                        }
                    }
                }
                if matches {
                    if self.node_kind(called) != SyntaxKind::Constructor {
                        declarations.clear();
                    } else {
                        declarations.retain(|&declaration| {
                            declaration != called
                                && matches!(
                                    self.node_kind(declaration),
                                    SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                                )
                        });
                    }
                } else {
                    declarations.retain(|&declaration| declaration != called);
                }
                declarations.push(called);
            }
        }
        Ok(self.definition_locations(origin, &declarations, reference.as_ref()))
    }

    // -- tsgo ProvideTypeDefinition -----------------------------------------

    /// tsgo `provideTypeDefinitionAtPosition`.
    fn type_definition_at_position(
        &mut self,
        file: usize,
        position: u32,
    ) -> CheckResult<DefinitionResponse> {
        let source = self.state.binder.source(file);
        let navigator = Navigator::new(source);
        let found = navigator.touching_property_name(position);
        if navigator.kind(found) == SyntaxKind::SourceFile {
            return Ok(DefinitionResponse::Null);
        }
        let origin = self.lsp_range(
            source,
            navigator.start_of_node(found, false) as usize..navigator.end(found) as usize,
        );
        let Some(node) = touching(found) else {
            return Ok(DefinitionResponse::Null);
        };
        let node = self.declaration_name_for_keyword(node);
        if let Some(symbol) = self.symbol_at(node)? {
            let location = match node {
                Touching::Node(id) => Some(id),
                Touching::Token { .. } => None,
            };
            let symbol_type = self.type_of_symbol_at_location(symbol, location)?;
            let mut declarations = self.declarations_from_type(symbol_type);
            if let Some(type_argument) = self
                .state
                .get_first_type_argument_from_known_type(symbol_type)?
            {
                let mut first = self.declarations_from_type(type_argument);
                first.extend(declarations);
                declarations = first;
            }
            if !declarations.is_empty() {
                return Ok(self.definition_locations(origin, &declarations, None));
            }
            let flags = self.state.binder.symbol(symbol).flags;
            if !flags.intersects(SymbolFlags::VALUE) && flags.intersects(SymbolFlags::TYPE) {
                let declarations = self.declarations_of(symbol);
                return Ok(self.definition_locations(origin, &declarations, None));
            }
        }
        Ok(DefinitionResponse::Null)
    }

    // -- locations ----------------------------------------------------------

    /// tsgo `createDefinitionLocations`: a link from the origin to each
    /// declaration's name (once per file and name range) within its
    /// context, after the referenced file's start.
    fn definition_locations(
        &self,
        origin: tsc_lsproto::Range,
        declarations: &[NodeId],
        reference: Option<&ReferenceInfo>,
    ) -> DefinitionResponse {
        let mut locations = Vec::new();
        let mut location_ranges: HashSet<(usize, usize, usize)> = HashSet::new();
        if let Some(reference) = reference {
            let target = tsc_lsproto::Range::default();
            locations.push(LocationLink {
                origin_selection_range: Some(origin.clone()),
                target_uri: DocumentUri(file_name_to_document_uri(&reference.file_name)),
                target_range: target.clone(),
                target_selection_range: target,
            });
        }
        for &declaration in declarations {
            let file = self.state.binder.file_index_of_node(declaration);
            let source = self.state.binder.source(file);
            let name =
                node_util::get_name_of_declaration(source, declaration).unwrap_or(declaration);
            let name_range = if self.node_kind(name) == SyntaxKind::EmptyStatement {
                let pos = self.state.binder.node_record(name).pos as usize;
                pos..pos
            } else {
                self.range_from_node(name)
            };
            if !location_ranges.insert((file, name_range.start, name_range.end)) {
                continue;
            }
            let context = context_node(self.state, declaration).unwrap_or(declaration);
            let mut context_range = to_context_range(self.state, &name_range, context)
                .unwrap_or_else(|| name_range.clone());
            if !(context_range.start <= name_range.start && name_range.end <= context_range.end) {
                context_range = context_range.start.min(name_range.start)
                    ..context_range.end.max(name_range.end);
            }
            let target_selection = self.source_maps.location(source, name_range);
            let mut target = self.source_maps.location(source, context_range);
            if target.uri != target_selection.uri
                || !lsp_range_contains(&target.range, &target_selection.range)
            {
                target = target_selection.clone();
            }
            locations.push(LocationLink {
                origin_selection_range: Some(origin.clone()),
                target_selection_range: target_selection.range,
                target_uri: target.uri,
                target_range: target.range,
            });
        }
        if self.links {
            DefinitionResponse::DefinitionLinks(locations)
        } else {
            locations_from_links(locations)
        }
    }

    /// tsgo `ToLSPRange` of a range of a file.
    pub(crate) fn lsp_range(&self, source: &SourceFile, range: Range<usize>) -> tsc_lsproto::Range {
        self.converters.to_lsp_range(&script_of(source), range)
    }

    /// tsgo `createRangeFromNode`: from the node's first token to its end.
    pub(crate) fn range_from_node(&self, node: NodeId) -> Range<usize> {
        let source = self.state.binder.source_of_node(node);
        token_start(source, node)..self.state.binder.node_record(node).end as usize
    }

    // -- declarations -------------------------------------------------------

    /// tsgo `getDeclarationsFromLocation`.
    pub(crate) fn declarations_from_location(
        &mut self,
        node: Touching,
    ) -> CheckResult<Vec<NodeId>> {
        if let Touching::Node(id) = node {
            let parent = self.parent(node);
            if self.node_kind(id) == SyntaxKind::Identifier
                && parent.is_some_and(|parent| {
                    self.node_kind(parent) == SyntaxKind::ShorthandPropertyAssignment
                })
            {
                // The name of a shorthand property is the property and the
                // value: the value's declarations, then the contextual
                // type's property's.
                let mut declarations = match self.state.get_resolved_symbol(id)? {
                    Some(symbol) => self.declarations_of(symbol),
                    None => Vec::new(),
                };
                declarations.extend(self.declarations_from_object_literal_element(id)?);
                return Ok(declarations);
            }
            if is_property_name(self.node_kind(id)) {
                if let Some(element) = parent {
                    if let NodeData::BindingElement(binding) =
                        &self.state.binder.node_record(element).data
                    {
                        let pattern = self.state.binder.node_record(element).parent;
                        if pattern.is_some_and(|pattern| {
                            self.node_kind(pattern) == SyntaxKind::ObjectBindingPattern
                        }) && binding.dot_dot_dot_token.is_none()
                            && Some(id) == binding.property_name.or(binding.name)
                        {
                            if let Some(name) = self.state.try_get_text_of_property_name(id) {
                                // A property of the pattern's type: its
                                // declarations in each union member.
                                let pattern = pattern.expect("a binding pattern");
                                let ty = self.state.get_type_at_location(pattern)?;
                                let types = if self
                                    .state
                                    .tables
                                    .flags_of(ty)
                                    .intersects(TypeFlags::UNION)
                                {
                                    self.state.get_types_of_type(ty)
                                } else {
                                    vec![ty]
                                };
                                let mut result = Vec::new();
                                for member in types {
                                    if let Some(property) =
                                        self.state.get_property_of_type_full(member, name)?
                                    {
                                        result.extend(self.declarations_of(property));
                                    }
                                }
                                return Ok(result);
                            }
                        }
                    }
                }
            }
        }

        let node = self.declaration_name_for_keyword(node);
        if let Some(mut symbol) = self.symbol_at(node)? {
            let flags = self.state.binder.symbol(symbol).flags;
            if flags.intersects(SymbolFlags::CLASS)
                && !flags.intersects(SymbolFlags::FUNCTION | SymbolFlags::VARIABLE)
                && self.kind(node) == SyntaxKind::ConstructorKeyword
            {
                let constructor = self
                    .state
                    .binder
                    .symbol(symbol)
                    .members()
                    .get(tsc_binder::InternalSymbolName::CONSTRUCTOR)
                    .copied();
                if let Some(constructor) = constructor {
                    symbol = constructor;
                }
            }
            if self
                .state
                .binder
                .symbol(symbol)
                .flags
                .intersects(SymbolFlags::ALIAS)
            {
                let resolved = self.state.resolve_alias(symbol)?;
                if resolved != self.state.unknown_symbol {
                    symbol = resolved;
                }
            }
            if let Touching::Node(id) = node {
                let declarations = self.declarations_from_object_literal_element(id)?;
                if !declarations.is_empty() {
                    return Ok(declarations);
                }
            }
            let declarations = self.declarations_of(symbol);
            if !declarations.is_empty() {
                return Ok(declarations);
            }
        }
        if let Touching::Node(id) = node {
            let signatures = self.state.get_index_signatures_at_location(id)?;
            if !signatures.is_empty() {
                return Ok(signatures);
            }
        }
        Ok(Vec::new())
    }

    /// tsgo `getDeclarationsFromObjectLiteralElement`: the declarations of
    /// the contextual type's property an object literal element names.
    fn declarations_from_object_literal_element(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Vec<NodeId>> {
        let Some(element) = containing_object_literal_element(self.state, node) else {
            return Ok(Vec::new());
        };
        let literal = self
            .state
            .binder
            .node_record(element)
            .parent
            .expect("an object literal");
        let Some(contextual_type) = self
            .state
            .language_service_contextual_type(literal, ContextFlags::NONE)?
        else {
            return Ok(Vec::new());
        };
        let mut properties = self.state.get_property_symbols_from_contextual_type(
            element,
            contextual_type,
            false,
        )?;
        let own_property = properties.iter().any(|&property| {
            self.state
                .binder
                .symbol(property)
                .value_declaration
                .is_some_and(|declaration| {
                    let parent = self.state.binder.node_record(declaration).parent;
                    parent.is_some_and(|parent| {
                        self.node_kind(parent) == SyntaxKind::ObjectLiteralExpression
                    }) && is_object_literal_element(self.node_kind(declaration))
                        && self.name_of(declaration) == Some(node)
                })
        });
        if own_property {
            if let Some(without_node_inferences) = self
                .state
                .language_service_contextual_type(literal, ContextFlags::IGNORE_NODE_INFERENCES)?
            {
                let without = self.state.get_property_symbols_from_contextual_type(
                    element,
                    without_node_inferences,
                    false,
                )?;
                if !without.is_empty() {
                    properties = without;
                }
            }
        }
        let mut result = Vec::new();
        for property in properties {
            result.extend(self.declarations_of(property));
        }
        Ok(result)
    }

    /// tsgo `tryGetSignatureDeclaration`: the function-like declaration of
    /// the signature a call of the node resolves to (not a function type).
    fn signature_declaration(&mut self, node: Touching) -> CheckResult<Option<NodeId>> {
        let Some(call) = self.ancestor_call_like_expression(node) else {
            return Ok(None);
        };
        let signature =
            self.state
                .get_resolved_signature(call, None, tsc_types::CheckMode::NORMAL)?;
        let Some(declaration) = self.state.signature_of(signature).declaration else {
            return Ok(None);
        };
        let kind = self.node_kind(declaration);
        Ok(
            (is_function_like_kind(kind) && kind != SyntaxKind::FunctionType)
                .then_some(declaration),
        )
    }

    /// tsgo `getAncestorCallLikeExpression`: the call-like expression that
    /// invokes the node (through property accesses whose name it is).
    fn ancestor_call_like_expression(&self, node: Touching) -> Option<NodeId> {
        let target = match node {
            Touching::Token { parent, .. } => {
                // A token is not the name of a property access.
                return self.invoking_call(Some(parent), None);
            }
            Touching::Node(id) => {
                let mut target = id;
                while self.is_right_side_of_property_access(target) {
                    target = self
                        .state
                        .binder
                        .node_record(target)
                        .parent
                        .expect("an access");
                }
                target
            }
        };
        let call = self.state.binder.node_record(target).parent;
        self.invoking_call(call, Some(target))
    }

    /// The call-like `call` when `target` is what it invokes.
    fn invoking_call(&self, call: Option<NodeId>, target: Option<NodeId>) -> Option<NodeId> {
        let call = call?;
        if self.is_call_like_expression(call) && self.invoked_expression(call) == target {
            return Some(call);
        }
        None
    }

    /// tsgo `symbolMatchesSignature`.
    fn symbol_matches_signature(&self, symbol: SymbolId, called: NodeId) -> bool {
        let called_symbol = self.state.binder.node_symbol(called);
        if Some(symbol) == called_symbol
            || called_symbol
                .is_some_and(|called| self.state.binder.symbol(called).parent == Some(symbol))
        {
            return true;
        }
        let Some(parent) = self.state.binder.node_record(called).parent else {
            return false;
        };
        self.is_assignment_expression(parent)
            || !self.is_call_like_expression(parent)
                && can_have_symbol(self.node_kind(parent))
                && self.state.binder.node_symbol(parent) == Some(symbol)
    }

    /// tsgo `getSymbolForOverriddenMember`: the base class's member an
    /// `override` modifier overrides.
    fn symbol_for_overridden_member(&mut self, node: Touching) -> CheckResult<Option<SymbolId>> {
        let start = match node {
            Touching::Node(id) => Some(id),
            Touching::Token { parent, .. } => Some(parent),
        };
        let Some(element) = self.find_ancestor(start, |definitions, node| {
            is_class_element(definitions.node_kind(node))
        }) else {
            return Ok(None);
        };
        let Some(element_name) = self.name_of(element) else {
            return Ok(None);
        };
        let Some(class) = self.find_ancestor(Some(element), |definitions, node| {
            matches!(
                definitions.node_kind(node),
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
            )
        }) else {
            return Ok(None);
        };
        let Some(base_type) = node_util::get_class_extends_heritage_element(
            self.state.binder.source_of_node(class),
            class,
        ) else {
            return Ok(None);
        };
        let NodeData::ExpressionWithTypeArguments(data) =
            &self.state.binder.node_record(base_type).data
        else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let expression = node_util::skip_parentheses_pub(
            self.state.binder.source_of_node(expression),
            expression,
        );
        let base = if self.node_kind(expression) == SyntaxKind::ClassExpression {
            self.state.binder.node_symbol(expression)
        } else {
            self.state.get_symbol_at_location(expression)?
        };
        let Some(base) = base else {
            return Ok(None);
        };
        let Some(name) = self.state.try_get_text_of_property_name(element_name) else {
            return Ok(None);
        };
        let source = self.state.binder.source_of_node(element);
        let ty = if node_util::has_syntactic_modifier(source, element, ModifierFlags::STATIC) {
            self.state.get_type_of_symbol(base)?
        } else {
            self.state.get_declared_type_of_symbol(base)?
        };
        self.state.get_property_of_type_full(ty, name)
    }

    /// tsgo `getTypeOfSymbolAtLocation`: a function's inferred type goes to
    /// its return type (go to definition goes to the function).
    fn type_of_symbol_at_location(
        &mut self,
        symbol: SymbolId,
        location: Option<NodeId>,
    ) -> CheckResult<TypeId> {
        let ty = self
            .state
            .get_type_of_symbol_at_location(symbol, location)?;
        let type_symbol = self.state.tables.type_of(ty).symbol;
        let inferred_function = type_symbol == Some(symbol)
            || type_symbol.is_some_and(|type_symbol| {
                let value_declaration = self.state.binder.symbol(symbol).value_declaration;
                value_declaration.is_some_and(|declaration| {
                    matches!(
                        &self.state.binder.node_record(declaration).data,
                        NodeData::VariableDeclaration(variable)
                            if variable.initializer.is_some()
                                && variable.initializer
                                    == self.state.binder.symbol(type_symbol).value_declaration
                    )
                })
            });
        if inferred_function {
            let signatures = self.state.get_signatures_of_type(ty, SignatureKind::Call)?;
            if let [signature] = signatures[..] {
                return self.state.get_return_type_of_signature(signature);
            }
        }
        Ok(ty)
    }

    /// tsgo `getDeclarationsFromType`: the declarations of the symbols of
    /// the type's union members, each once.
    fn declarations_from_type(&self, ty: TypeId) -> Vec<NodeId> {
        let flags = self.state.tables.flags_of(ty);
        let members = if flags.intersects(TypeFlags::UNION) {
            self.state.get_types_of_type(ty)
        } else if flags.intersects(TypeFlags::NEVER) {
            Vec::new()
        } else {
            vec![ty]
        };
        let mut result = Vec::new();
        for member in members {
            if let Some(symbol) = self.state.tables.type_of(member).symbol {
                for declaration in self.declarations_of(symbol) {
                    if !result.contains(&declaration) {
                        result.push(declaration);
                    }
                }
            }
        }
        result
    }

    // -- tree helpers -------------------------------------------------------

    /// tsgo `getDeclarationNameForKeyword`: a keyword before a
    /// declaration's name stands for the name.
    pub(crate) fn declaration_name_for_keyword(&self, node: Touching) -> Touching {
        let kind = self.kind(node);
        if !crate::astnav::is_keyword_kind(kind) {
            return node;
        }
        let Some(parent) = self.parent(node) else {
            return node;
        };
        if let NodeData::VariableDeclarationList(list) = &self.state.binder.node_record(parent).data
        {
            let first = list
                .declarations
                .and_then(|list| self.state.binder.node_array(list).nodes.first().copied());
            if let Some(name) = first.and_then(|declaration| self.name_of(declaration)) {
                return Touching::Node(name);
            }
        } else if has_declaration_data(self.node_kind(parent)) {
            if let Some(name) = self.name_of(parent) {
                if self.pos(node) < self.state.binder.node_record(name).pos {
                    return Touching::Node(name);
                }
            }
        }
        node
    }

    /// tsgo `GetSymbolAtLocation` of a node or token.
    pub(crate) fn symbol_at(&mut self, node: Touching) -> CheckResult<Option<SymbolId>> {
        match node {
            Touching::Node(id) => self.state.get_symbol_at_location(id),
            Touching::Token { kind, parent, .. } => self.state.get_symbol_at_token(kind, parent),
        }
    }

    /// tsgo `SymbolToString`: a symbol's name as the checker writes it
    /// (every meaning, any node kind).
    pub(crate) fn symbol_to_string(&mut self, symbol: SymbolId) -> CheckResult<String> {
        self.state.symbol_to_string_with_flags(
            symbol,
            None,
            tsc_types::SymbolFlags::ALL,
            tsc_checker::SymbolFormatFlags::ALLOW_ANY_NODE_KIND,
        )
    }

    /// tsgo `ast.IsJumpStatementTarget`.
    pub(crate) fn is_jump_statement_target(&self, node: NodeId) -> bool {
        if self.node_kind(node) != SyntaxKind::Identifier {
            return false;
        }
        let Some(parent) = self.state.binder.node_record(node).parent else {
            return false;
        };
        match &self.state.binder.node_record(parent).data {
            NodeData::BreakStatement(data) => data.label == Some(node),
            NodeData::ContinueStatement(data) => data.label == Some(node),
            _ => false,
        }
    }

    /// tsgo `getTargetLabel`: the label of the labeled statement around
    /// the jump with the reference's name.
    pub(crate) fn target_label(&self, mut reference: NodeId, name: NodeId) -> Option<NodeId> {
        let source = self.state.binder.source_of_node(name);
        let text = node_util::id_text(source, name)?;
        loop {
            if let NodeData::LabeledStatement(data) = &self.state.binder.node_record(reference).data
            {
                if let Some(label) = data.label {
                    if node_util::id_text(source, label) == Some(text) {
                        return Some(label);
                    }
                }
            }
            reference = self.state.binder.node_record(reference).parent?;
        }
    }

    /// tsgo `ast.IsRightSideOfPropertyAccess`.
    pub(crate) fn is_right_side_of_property_access(&self, node: NodeId) -> bool {
        let Some(parent) = self.state.binder.node_record(node).parent else {
            return false;
        };
        matches!(
            &self.state.binder.node_record(parent).data,
            NodeData::PropertyAccessExpression(access) if access.name == Some(node)
        )
    }

    /// tsgo `ast.IsArgumentExpressionOfElementAccess`.
    pub(crate) fn is_argument_expression_of_element_access(&self, node: NodeId) -> bool {
        let Some(parent) = self.state.binder.node_record(node).parent else {
            return false;
        };
        matches!(
            &self.state.binder.node_record(parent).data,
            NodeData::ElementAccessExpression(access) if access.argument_expression == Some(node)
        )
    }

    /// tsgo `isCalleeWorker` (`IsCallExpressionTarget`,
    /// `IsNewExpressionTarget`, `IsCallOrNewExpressionTarget`,
    /// `IsTaggedTemplateTag`, `IsDecoratorTarget`,
    /// `IsJsxOpeningLikeElementTagName`): the node — past a property access
    /// it names (or an element access it is the argument of), and with
    /// `skip_past_outer_expressions` the outer expressions it is — is what
    /// a call-like node of `callee` calls.
    pub(crate) fn is_callee_worker(
        &self,
        node: NodeId,
        callee: Callee,
        include_element_access: bool,
        skip_past_outer_expressions: bool,
    ) -> bool {
        let parent = |node: NodeId| self.state.binder.node_record(node).parent;
        let mut target = if self.is_right_side_of_property_access(node)
            || include_element_access && self.is_argument_expression_of_element_access(node)
        {
            parent(node).expect("an access")
        } else {
            node
        };
        // Only skip outer expressions if the target is actually an expression node
        if skip_past_outer_expressions && self.is_expression(target) {
            target = self.skip_outer_expressions(target);
        }
        let Some(call) = parent(target) else {
            return false;
        };
        let selected = match (&self.state.binder.node_record(call).data, callee) {
            (NodeData::CallExpression(data), Callee::Call | Callee::CallOrNew) => data.expression,
            (NodeData::NewExpression(data), Callee::New | Callee::CallOrNew) => data.expression,
            (NodeData::TaggedTemplateExpression(data), Callee::TaggedTemplate) => data.tag,
            (NodeData::Decorator(data), Callee::Decorator) => data.expression,
            (NodeData::JsxOpeningElement(data), Callee::JsxOpeningLikeElement) => data.tag_name,
            (NodeData::JsxSelfClosingElement(data), Callee::JsxOpeningLikeElement) => data.tag_name,
            _ => None,
        };
        selected == Some(target)
    }

    /// [`Self::is_callee_worker`] of a node or token (a token is no callee)
    /// without element accesses or outer expressions.
    pub(crate) fn is_callee(&self, node: Touching, callee: Callee) -> bool {
        match node {
            Touching::Node(id) => self.is_callee_worker(id, callee, false, false),
            Touching::Token { .. } => false,
        }
    }

    /// tsgo `ast.IsExpression`: an expression kind, past partially emitted
    /// expressions.
    pub(crate) fn is_expression(&self, node: NodeId) -> bool {
        let mut node = node;
        while let NodeData::PartiallyEmittedExpression(data) =
            &self.state.binder.node_record(node).data
        {
            match data.expression {
                Some(expression) => node = expression,
                None => break,
            }
        }
        crate::format::is_expression_kind(self.node_kind(node))
    }

    /// tsgo `ast.IsCallLikeExpression`.
    pub(crate) fn is_call_like_expression(&self, node: NodeId) -> bool {
        match &self.state.binder.node_record(node).data {
            NodeData::BinaryExpression(binary) => binary
                .operator_token
                .is_some_and(|operator| self.node_kind(operator) == SyntaxKind::InstanceOfKeyword),
            _ => matches!(
                self.node_kind(node),
                SyntaxKind::JsxOpeningElement
                    | SyntaxKind::JsxSelfClosingElement
                    | SyntaxKind::JsxOpeningFragment
                    | SyntaxKind::CallExpression
                    | SyntaxKind::NewExpression
                    | SyntaxKind::TaggedTemplateExpression
                    | SyntaxKind::Decorator
            ),
        }
    }

    /// tsgo `ast.GetInvokedExpression`.
    pub(crate) fn invoked_expression(&self, call: NodeId) -> Option<NodeId> {
        if self.node_kind(call) == SyntaxKind::JsxOpeningFragment {
            return Some(call);
        }
        match &self.state.binder.node_record(call).data {
            NodeData::TaggedTemplateExpression(data) => data.tag,
            NodeData::JsxOpeningElement(data) => data.tag_name,
            NodeData::JsxSelfClosingElement(data) => data.tag_name,
            NodeData::BinaryExpression(data) => data.right,
            NodeData::CallExpression(data) => data.expression,
            NodeData::NewExpression(data) => data.expression,
            NodeData::Decorator(data) => data.expression,
            _ => None,
        }
    }

    /// tsgo `ast.IsAssignmentExpression(node, false)`.
    fn is_assignment_expression(&self, node: NodeId) -> bool {
        let NodeData::BinaryExpression(binary) = &self.state.binder.node_record(node).data else {
            return false;
        };
        let (Some(operator), Some(left)) = (binary.operator_token, binary.left) else {
            return false;
        };
        let source = self.state.binder.source_of_node(node);
        node_util::is_assignment_operator(self.node_kind(operator))
            && node_util::is_left_hand_side_expression(source, left)
    }

    /// tsgo `ast.FindAncestor` from `node`.
    pub(crate) fn find_ancestor(
        &self,
        mut node: Option<NodeId>,
        test: impl Fn(&Self, NodeId) -> bool,
    ) -> Option<NodeId> {
        while let Some(current) = node {
            if test(self, current) {
                return Some(current);
            }
            node = self.state.binder.node_record(current).parent;
        }
        None
    }

    pub(crate) fn declarations_of(&self, symbol: SymbolId) -> Vec<NodeId> {
        self.state
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .collect()
    }

    pub(crate) fn name_of(&self, node: NodeId) -> Option<NodeId> {
        node_util::name_field_of(self.state.binder.source_of_node(node), node)
    }

    /// tsgo `Node.Parent`.
    pub(crate) fn parent_of(&self, node: NodeId) -> Option<NodeId> {
        self.state.binder.node_record(node).parent
    }

    /// tsgo `Node.Initializer()`.
    pub(crate) fn initializer_of(&self, node: NodeId) -> Option<NodeId> {
        node_util::initializer_of(self.state.binder.source_of_node(node), node)
    }

    /// tsgo `Node.Members()` of a class, interface or type literal.
    pub(crate) fn members_of(&self, node: NodeId) -> Vec<NodeId> {
        self.node_list(node_util::members_of(
            self.state.binder.source_of_node(node),
            node,
        ))
    }

    /// tsgo `Node.Modifiers()` (decorators among them).
    pub(crate) fn modifiers_of(&self, node: NodeId) -> Vec<NodeId> {
        self.node_list(node_util::modifiers_of(
            self.state.binder.source_of_node(node),
            node,
        ))
    }

    /// The nodes of a list (none without one).
    pub(crate) fn node_list(&self, list: Option<tsc_syntax::NodeArrayId>) -> Vec<NodeId> {
        list.map(|list| self.state.binder.node_array(list).nodes.to_vec())
            .unwrap_or_default()
    }

    /// tsgo `Node.ForEachChild`'s children, in order.
    pub(crate) fn children(&self, node: NodeId) -> Vec<NodeId> {
        let source = self.state.binder.source_of_node(node);
        let mut children = Vec::new();
        tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        children
    }

    pub(crate) fn node_kind(&self, node: NodeId) -> SyntaxKind {
        self.state.binder.node_record(node).kind
    }

    pub(crate) fn kind(&self, node: Touching) -> SyntaxKind {
        match node {
            Touching::Node(id) => self.node_kind(id),
            Touching::Token { kind, .. } => kind,
        }
    }

    pub(crate) fn parent(&self, node: Touching) -> Option<NodeId> {
        match node {
            Touching::Node(id) => self.state.binder.node_record(id).parent,
            Touching::Token { parent, .. } => Some(parent),
        }
    }

    pub(crate) fn pos(&self, node: Touching) -> u32 {
        match node {
            Touching::Node(id) => self.state.binder.node_record(id).pos,
            Touching::Token { pos, .. } => pos,
        }
    }
}

/// tsgo `getContextNode` (findallreferences.go): the node whose range is
/// a declaration's context.
pub(crate) fn context_node(state: &CheckerState<'_>, node: NodeId) -> Option<NodeId> {
    let record = state.binder.node_record(node);
    let parent = record.parent;
    let parent_kind =
        |state: &CheckerState<'_>| parent.map(|parent| state.binder.node_record(parent).kind);
    match record.kind {
        SyntaxKind::VariableDeclaration => {
            let parent = parent?;
            let NodeData::VariableDeclarationList(list) = &state.binder.node_record(parent).data
            else {
                return Some(node);
            };
            let count = list
                .declarations
                .map_or(0, |list| state.binder.node_array(list).nodes.len());
            if count != 1 {
                return Some(node);
            }
            let grandparent = state.binder.node_record(parent).parent;
            match grandparent.map(|grandparent| state.binder.node_record(grandparent).kind) {
                Some(SyntaxKind::VariableStatement) => grandparent,
                Some(SyntaxKind::ForInStatement | SyntaxKind::ForOfStatement) => {
                    context_node(state, grandparent?)
                }
                _ => Some(parent),
            }
        }
        SyntaxKind::BindingElement => {
            let pattern = parent?;
            context_node(state, state.binder.node_record(pattern).parent?)
        }
        SyntaxKind::ImportSpecifier => {
            let named = parent?;
            let clause = state.binder.node_record(named).parent?;
            state.binder.node_record(clause).parent
        }
        SyntaxKind::ExportSpecifier | SyntaxKind::NamespaceImport => {
            state.binder.node_record(parent?).parent
        }
        SyntaxKind::ImportClause | SyntaxKind::NamespaceExport => parent,
        SyntaxKind::BinaryExpression => {
            if parent_kind(state) == Some(SyntaxKind::ExpressionStatement) {
                parent
            } else {
                Some(node)
            }
        }
        SyntaxKind::ForOfStatement | SyntaxKind::ForInStatement | SyntaxKind::SwitchStatement => {
            None
        }
        SyntaxKind::PropertyAssignment | SyntaxKind::ShorthandPropertyAssignment => {
            if parent.is_some_and(|parent| {
                state.is_array_literal_or_object_literal_destructuring_pattern(parent)
            }) {
                let mut ancestor = parent;
                while let Some(current) = ancestor {
                    let kind = state.binder.node_record(current).kind;
                    if matches!(
                        kind,
                        SyntaxKind::BinaryExpression
                            | SyntaxKind::ForInStatement
                            | SyntaxKind::ForOfStatement
                    ) {
                        break;
                    }
                    ancestor = state.binder.node_record(current).parent;
                }
                return context_node(state, ancestor?);
            }
            Some(node)
        }
        _ => Some(node),
    }
}

/// tsgo `toContextRange`: the context's range when it is not the name's.
fn to_context_range(
    state: &CheckerState<'_>,
    name_range: &Range<usize>,
    context: NodeId,
) -> Option<Range<usize>> {
    let range = range_of_node(state, context);
    (range != *name_range).then_some(range)
}
