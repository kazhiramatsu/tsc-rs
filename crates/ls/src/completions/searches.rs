//! tsgo `tryGetGlobalSymbols`' searches for the places whose completions
//! have a set shape: object types in type arguments, object literals and
//! binding patterns, import and export clauses, import attributes, local
//! named exports, class and interface members, and JSX attributes.

use std::collections::HashSet;

use tsc_binder::node_util;
use tsc_checker::state::CheckResult;
use tsc_syntax::{identifier_to_keyword_kind, NodeData, NodeId, SyntaxKind};
use tsc_types::{
    ContextFlags, ModifierFlags, NodeFlags, ObjectFlags, SymbolFlags, SymbolId, TypeFlags, TypeId,
};

use crate::utilities::Touching;

use super::context::{is_class_like, is_object_type_declaration, Completions};
use super::data::{origin_kind, Collector, GlobalsSearch, Location, SymbolOriginInfo};
use super::keywords::is_class_member_completion_keyword;
use super::{sort_text, CompletionKind, KeywordCompletionFilters};

/// tsgo `ast.InternalSymbolNameMissing`, a name no member has.
const INTERNAL_SYMBOL_NAME_MISSING: &str = "\u{FFFD}missing";

impl Completions<'_, '_, '_> {
    /// tsgo `tryGetObjectTypeLiteralInTypeArgumentCompletionSymbols`: the
    /// members of the constraint of a type argument that is an object type.
    pub(super) fn try_get_object_type_literal_in_type_argument_completion_symbols(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
    ) -> CheckResult<GlobalsSearch> {
        let Some(type_literal) = self.try_get_type_literal_node(facts.context_token) else {
            return Ok(GlobalsSearch::Continue);
        };
        let intersection = self
            .parent_of(type_literal)
            .filter(|&parent| self.node_kind(parent) == SyntaxKind::IntersectionType);
        let container = intersection.unwrap_or(type_literal);
        let Some(expected) =
            self.constraint_of_type_argument_property(Touching::Node(container))?
        else {
            return Ok(GlobalsSearch::Continue);
        };
        let actual = self.request.state.get_type_from_type_node(container)?;
        let members = self.properties_for_completion(expected)?;
        let existing_members = self.properties_for_completion(actual)?;
        let existing_names = existing_members
            .iter()
            .map(|&member| self.raw_symbol_name(member))
            .collect::<HashSet<_>>();
        collector.symbols.extend(
            members
                .into_iter()
                .filter(|&member| !existing_names.contains(&self.raw_symbol_name(member))),
        );
        collector.completion_kind = CompletionKind::ObjectPropertyDeclaration;
        collector.is_new_identifier_location = true;
        Ok(GlobalsSearch::Success)
    }

    /// tsgo `tryGetTypeLiteralNode`.
    fn try_get_type_literal_node(&self, token: Option<Touching>) -> Option<NodeId> {
        let token = token?;
        let parent = self.parent(token)?;
        match self.kind(token) {
            SyntaxKind::OpenBraceToken => {
                (self.node_kind(parent) == SyntaxKind::TypeLiteral).then_some(parent)
            }
            SyntaxKind::SemicolonToken | SyntaxKind::CommaToken | SyntaxKind::Identifier => {
                if self.node_kind(parent) != SyntaxKind::PropertySignature {
                    return None;
                }
                self.parent_of(parent)
                    .filter(|&grandparent| self.node_kind(grandparent) == SyntaxKind::TypeLiteral)
            }
            _ => None,
        }
    }

    /// tsgo `tryGetObjectLikeCompletionSymbols`: the properties of an
    /// object literal's contextual type, or of the type a binding pattern
    /// destructures, less those written already.
    pub(super) fn try_get_object_like_completion_symbols(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
    ) -> CheckResult<GlobalsSearch> {
        if facts
            .context_token
            .is_some_and(|context| self.kind(context) == SyntaxKind::DotDotDotToken)
        {
            return Ok(GlobalsSearch::Continue);
        }
        let Some(container) = self.try_get_object_like_completion_container(facts.context_token)
        else {
            return Ok(GlobalsSearch::Continue);
        };
        // Property names from the contextual, inferred or declared type.
        collector.completion_kind = CompletionKind::ObjectPropertyDeclaration;
        let mut type_members = Vec::new();
        let mut existing_members = Vec::new();
        if self.node_kind(container) == SyntaxKind::ObjectLiteralExpression {
            let Some(instantiated_type) = self.try_get_object_literal_contextual_type(container)?
            else {
                // A shorthand property's value.
                if self.record(container).flags & NodeFlags::IN_WITH_STATEMENT.bits() != 0 {
                    return Ok(GlobalsSearch::Fail);
                }
                return Ok(GlobalsSearch::Continue);
            };
            let completions_type = self.request.state.language_service_contextual_type(
                container,
                ContextFlags::IGNORE_NODE_INFERENCES,
            )?;
            let ty = completions_type.unwrap_or(instantiated_type);
            let string_index_type = self.request.state.get_string_index_type(ty)?;
            let number_index_type = self.request.state.get_number_index_type(ty)?;
            collector.is_new_identifier_location =
                string_index_type.is_some() || number_index_type.is_some();
            type_members = self.properties_for_object_expression(
                instantiated_type,
                completions_type,
                container,
            )?;
            if let NodeData::ObjectLiteralExpression(data) = self.data(container) {
                existing_members = self.node_list(data.properties).to_vec();
            }
            // With a number index, still members.
            if type_members.is_empty() && number_index_type.is_none() {
                return Ok(GlobalsSearch::Continue);
            }
        } else {
            assert_eq!(
                self.node_kind(container),
                SyntaxKind::ObjectBindingPattern,
                "Expected 'objectLikeContainer' to be an object binding pattern."
            );
            // Only the properties of the type destructured.
            collector.is_new_identifier_location = false;
            let root =
                self.root_declaration(self.parent_of(container).expect("a pattern's parent"));
            // The type from a declaration or inference, not from the
            // pattern's shape: a parameter of a contextually typed function
            // expression or method takes it from the context.
            let mut can_get_type = self.initializer_of(root).is_some()
                || self.type_of(root).is_some()
                || self
                    .parent_of(root)
                    .and_then(|parent| self.parent_of(parent))
                    .is_some_and(|node| self.node_kind(node) == SyntaxKind::ForOfStatement);
            if !can_get_type && self.node_kind(root) == SyntaxKind::Parameter {
                let function = self.parent_of(root).expect("a parameter's function");
                if self.is_expression(function) {
                    can_get_type = self
                        .request
                        .state
                        .language_service_contextual_type(function, ContextFlags::NONE)?
                        .is_some();
                } else if matches!(
                    self.node_kind(function),
                    SyntaxKind::MethodDeclaration | SyntaxKind::SetAccessor
                ) {
                    let outer = self.parent_of(function).expect("a method's parent");
                    can_get_type = self.is_expression(outer)
                        && self
                            .request
                            .state
                            .language_service_contextual_type(outer, ContextFlags::NONE)?
                            .is_some();
                }
            }
            if can_get_type {
                let type_for_object = self.request.state.get_type_at_location(container)?;
                for property in self.request.state.get_properties_of_type(type_for_object)? {
                    if self.request.state.is_property_accessible(
                        container,
                        false,
                        false,
                        type_for_object,
                        property,
                    )? {
                        type_members.push(property);
                    }
                }
                if let NodeData::ObjectBindingPattern(data) = self.data(container) {
                    existing_members = self.node_list(data.elements).to_vec();
                }
            }
        }
        let is_object_literal = self.node_kind(container) == SyntaxKind::ObjectLiteralExpression;
        let method_snippets = self
            .request
            .preferences
            .completion_preferences
            .include_completions_with_object_literal_method_snippets
            == Some(true);
        if !type_members.is_empty() {
            let (filtered_members, spread_member_names) =
                self.filter_object_members_list(type_members, &existing_members)?;
            for &member in &filtered_members {
                collector.symbols.push(member);
                if spread_member_names.contains(&self.raw_symbol_name(member)) {
                    collector.symbol_to_sort_text_map.insert(
                        member,
                        sort_text::MEMBER_DECLARED_BY_SPREAD_ASSIGNMENT.to_owned(),
                    );
                }
                if self
                    .request
                    .state
                    .symbol_flags(member)
                    .intersects(SymbolFlags::OPTIONAL)
                {
                    collector
                        .symbol_to_sort_text_map
                        .entry(member)
                        .or_insert_with(|| sort_text::OPTIONAL_MEMBER.to_owned());
                }
                if is_object_literal && method_snippets {
                    let (display_name, _) = self.completion_entry_display_name_for_symbol(
                        member,
                        None,
                        CompletionKind::ObjectPropertyDeclaration,
                        false,
                    );
                    if !display_name.is_empty() {
                        let original = collector
                            .symbol_to_sort_text_map
                            .get(&member)
                            .cloned()
                            .unwrap_or_else(|| sort_text::LOCATION_PRIORITY.to_owned());
                        collector.symbol_to_sort_text_map.insert(
                            member,
                            sort_text::object_literal_property(&original, &display_name),
                        );
                    }
                }
            }
            // The method snippets of an object literal's members print and
            // format methods, which come with the formatter (L8).
        }
        Ok(GlobalsSearch::Success)
    }

    /// tsgo `tryGetObjectLikeCompletionContainer`: the object literal or
    /// binding pattern the context token is in.
    fn try_get_object_like_completion_container(
        &self,
        context: Option<Touching>,
    ) -> Option<NodeId> {
        let context = context?;
        let parent = self.parent(context)?;
        let grandparent = self.parent_of(parent);
        let is_object_literal = |node: Option<NodeId>| {
            node.is_some_and(|node| self.node_kind(node) == SyntaxKind::ObjectLiteralExpression)
        };
        match self.kind(context) {
            // const x = { |
            // const x = { a: 0, |
            SyntaxKind::OpenBraceToken | SyntaxKind::CommaToken => {
                if matches!(
                    self.node_kind(parent),
                    SyntaxKind::ObjectLiteralExpression | SyntaxKind::ObjectBindingPattern
                ) {
                    return Some(parent);
                }
            }
            SyntaxKind::AsteriskToken => {
                if self.node_kind(parent) == SyntaxKind::MethodDeclaration
                    && is_object_literal(grandparent)
                {
                    return grandparent;
                }
            }
            SyntaxKind::AsyncKeyword => {
                if is_object_literal(grandparent) {
                    return grandparent;
                }
            }
            SyntaxKind::Identifier => {
                if self.identifier_text(context) == Some("async")
                    && self.node_kind(parent) == SyntaxKind::ShorthandPropertyAssignment
                {
                    return grandparent;
                }
                if is_object_literal(grandparent)
                    && (self.node_kind(parent) == SyntaxKind::SpreadAssignment
                        || self.node_kind(parent) == SyntaxKind::ShorthandPropertyAssignment
                            && self.line_of(self.end(context)) != self.line_of(self.position))
                {
                    return grandparent;
                }
                let ancestor = self.find_ancestor(Some(parent), |this, node| {
                    this.node_kind(node) == SyntaxKind::PropertyAssignment
                });
                if let Some(ancestor) = ancestor {
                    let outer = self.parent_of(ancestor);
                    if self.last_token(ancestor) == Some(context) && is_object_literal(outer) {
                        return outer;
                    }
                }
            }
            kind => {
                let great_grandparent = grandparent.and_then(|node| self.parent_of(node));
                if grandparent.is_some_and(|node| {
                    matches!(
                        self.node_kind(node),
                        SyntaxKind::MethodDeclaration
                            | SyntaxKind::GetAccessor
                            | SyntaxKind::SetAccessor
                    )
                }) && is_object_literal(great_grandparent)
                {
                    return great_grandparent;
                }
                if self.node_kind(parent) == SyntaxKind::SpreadAssignment
                    && is_object_literal(grandparent)
                {
                    return grandparent;
                }
                let ancestor = self.find_ancestor(Some(parent), |this, node| {
                    this.node_kind(node) == SyntaxKind::PropertyAssignment
                });
                if let Some(ancestor) = ancestor {
                    let outer = self.parent_of(ancestor);
                    if kind != SyntaxKind::ColonToken
                        && self.last_token(ancestor) == Some(context)
                        && is_object_literal(outer)
                    {
                        return outer;
                    }
                }
            }
        }
        None
    }

    /// tsgo `tryGetObjectLiteralContextualType`.
    fn try_get_object_literal_contextual_type(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<TypeId>> {
        if let Some(ty) = self
            .request
            .state
            .language_service_contextual_type(node, ContextFlags::NONE)?
        {
            return Ok(Some(ty));
        }
        let mut parent = self.parent_of(node).expect("an object literal's parent");
        while self.node_kind(parent) == SyntaxKind::ParenthesizedExpression {
            parent = self
                .parent_of(parent)
                .expect("a parenthesized expression's parent");
        }
        if let NodeData::BinaryExpression(data) = self.data(parent) {
            // An assignment pattern: `({ | } = x)`.
            if data
                .operator_token
                .is_some_and(|operator| self.node_kind(operator) == SyntaxKind::EqualsToken)
                && data.left == Some(node)
            {
                return Ok(Some(self.request.state.get_type_at_location(parent)?));
            }
        }
        if self.is_expression(parent) {
            // f(() => (({ | })));
            return self
                .request
                .state
                .language_service_contextual_type(parent, ContextFlags::NONE);
        }
        Ok(None)
    }

    /// tsgo `getPropertiesForObjectExpression`.
    pub(crate) fn properties_for_object_expression(
        &mut self,
        contextual_type: TypeId,
        completions_type: Option<TypeId>,
        object: NodeId,
    ) -> CheckResult<Vec<SymbolId>> {
        let has_completions_type = completions_type.is_some_and(|ty| ty != contextual_type);
        let types = if self
            .request
            .state
            .tables
            .flags_of(contextual_type)
            .intersects(TypeFlags::UNION)
        {
            self.union_types(contextual_type)
        } else {
            vec![contextual_type]
        };
        let mut not_promises = Vec::new();
        for ty in types {
            if self
                .request
                .state
                .get_promised_type_of_promise(ty)?
                .is_none()
            {
                not_promises.push(ty);
            }
        }
        let promise_filtered = self
            .request
            .state
            .get_union_type_ex(&not_promises, tsc_types::UnionReduction::Literal)?;
        let ty = match completions_type {
            Some(completions_type)
                if has_completions_type
                    && !self
                        .request
                        .state
                        .tables
                        .flags_of(completions_type)
                        .intersects(TypeFlags::ANY_OR_UNKNOWN) =>
            {
                self.request.state.get_union_type_ex(
                    &[promise_filtered, completions_type],
                    tsc_types::UnionReduction::Literal,
                )?
            }
            _ => promise_filtered,
        };
        let properties = self.apparent_properties_of(ty, object)?;
        if self.is_class_type(ty) && self.contains_non_public_properties(&properties) {
            return Ok(Vec::new());
        }
        if has_completions_type {
            // Not a member whose only declaration is in the object literal
            // itself: `f({ abc/**/: "" })` with `f<T>(x: T)`.
            return Ok(properties
                .into_iter()
                .filter(|&member| {
                    let declarations = self.declarations_of(member);
                    declarations.is_empty()
                        || declarations
                            .iter()
                            .any(|&declaration| self.parent_of(declaration) != Some(object))
                })
                .collect());
        }
        Ok(properties)
    }

    /// tsgo `getApparentProperties` (completions): a union's possible
    /// properties, of the types an object literal can be.
    fn apparent_properties_of(&mut self, ty: TypeId, node: NodeId) -> CheckResult<Vec<SymbolId>> {
        if !self
            .request
            .state
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::UNION)
        {
            return self.request.state.get_apparent_properties(ty);
        }
        let mut member_types = Vec::new();
        for member_type in self.union_types(ty) {
            let excluded = self
                .request
                .state
                .tables
                .flags_of(member_type)
                .intersects(TypeFlags::PRIMITIVE)
                || self.request.state.is_array_like_type(member_type)?
                || self
                    .request
                    .state
                    .is_type_invalid_due_to_union_discriminant(member_type, node)?
                || self
                    .request
                    .state
                    .type_has_call_or_construct_signatures(member_type)?
                || self.is_class_type(member_type) && {
                    let properties = self.request.state.get_apparent_properties(member_type)?;
                    self.contains_non_public_properties(&properties)
                };
            if !excluded {
                member_types.push(member_type);
            }
        }
        self.request
            .state
            .get_all_possible_properties_of_types(&member_types)
    }

    /// tsgo `Type.IsClass`.
    fn is_class_type(&self, ty: TypeId) -> bool {
        self.request
            .state
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::CLASS)
    }

    /// tsgo `containsNonPublicProperties`.
    fn contains_non_public_properties(&self, properties: &[SymbolId]) -> bool {
        properties.iter().any(|&property| {
            self.request
                .state
                .get_declaration_modifier_flags_from_symbol(property)
                .intersects(ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER)
        })
    }

    /// tsgo `filterObjectMembersList`: the members not written already, and
    /// the names spread assignments declare.
    fn filter_object_members_list(
        &mut self,
        members: Vec<SymbolId>,
        existing_members: &[NodeId],
    ) -> CheckResult<(Vec<SymbolId>, HashSet<String>)> {
        let mut spread_member_names = HashSet::new();
        if existing_members.is_empty() {
            return Ok((members, spread_member_names));
        }
        let mut existing_names = HashSet::new();
        for &member in existing_members {
            // Not an omitted expression of a missing member.
            if !matches!(
                self.node_kind(member),
                SyntaxKind::PropertyAssignment
                    | SyntaxKind::ShorthandPropertyAssignment
                    | SyntaxKind::BindingElement
                    | SyntaxKind::MethodDeclaration
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor
                    | SyntaxKind::SpreadAssignment
            ) {
                continue;
            }
            // Not the member being written.
            if self.is_currently_editing_node(Touching::Node(member), self.position) {
                continue;
            }
            let mut existing_name = None;
            if self.node_kind(member) == SyntaxKind::SpreadAssignment {
                self.set_member_declared_by_spread_assignment(member, &mut spread_member_names)?;
            } else if let Some(property_name) = match self.data(member) {
                NodeData::BindingElement(data) => data.property_name,
                _ => None,
            } {
                // Only identifiers.
                if self.node_kind(property_name) == SyntaxKind::Identifier {
                    existing_name = self
                        .identifier_text(Touching::Node(property_name))
                        .map(str::to_owned);
                }
            } else if let Some(name) =
                node_util::get_name_of_declaration(self.source_of(member), member)
            {
                // A computed property name is not counted, and with only
                // identifiers `__proto__` would not be.
                existing_name = self.property_name_literal_text(name);
            }
            if let Some(name) = existing_name.filter(|name| !name.is_empty()) {
                existing_names.insert(name);
            }
        }
        let filtered = members
            .into_iter()
            .filter(|&member| !existing_names.contains(&self.raw_symbol_name(member)))
            .collect();
        Ok((filtered, spread_member_names))
    }

    /// The text of a name that is tsgo's `IsPropertyNameLiteral`.
    fn property_name_literal_text(&self, name: NodeId) -> Option<String> {
        let source = self.source_of(name);
        match self.node_kind(name) {
            SyntaxKind::Identifier => node_util::id_text(source, name).map(str::to_owned),
            SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::NumericLiteral => node_util::literal_text_of(source, name)
                .map(|text| text.to_string_lossy().into_owned()),
            _ => None,
        }
    }

    /// tsgo `setMemberDeclaredBySpreadAssignment`.
    fn set_member_declared_by_spread_assignment(
        &mut self,
        declaration: NodeId,
        members: &mut HashSet<String>,
    ) -> CheckResult<()> {
        let Some(expression) = self.expression_of(declaration) else {
            return Ok(());
        };
        let Some(symbol) = self.request.state.get_symbol_at_location(expression)? else {
            return Ok(());
        };
        let ty = self
            .request
            .state
            .get_type_of_symbol_at_location(symbol, Some(expression))?;
        if !self
            .request
            .state
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::STRUCTURED_TYPE)
        {
            return Ok(());
        }
        for property in self.request.state.get_properties_of_type(ty)? {
            members.insert(self.raw_symbol_name(property));
        }
        Ok(())
    }

    /// tsgo `tryGetImportOrExportClauseCompletionSymbols`: the exports of
    /// the module of `import { | } from "m"` or `export { | } from "m"`.
    pub(super) fn try_get_import_or_export_clause_completion_symbols(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
    ) -> CheckResult<GlobalsSearch> {
        let Some(context) = facts.context_token else {
            return Ok(GlobalsSearch::Continue);
        };
        let is_named_imports_or_exports = |this: &Self, node: Option<NodeId>| {
            node.filter(|&node| {
                matches!(
                    this.node_kind(node),
                    SyntaxKind::NamedImports | SyntaxKind::NamedExports
                )
            })
        };
        // `import { |`, `import { a as 0, | }` or `import { type | }`.
        let named = match self.kind(context) {
            SyntaxKind::OpenBraceToken | SyntaxKind::CommaToken => {
                is_named_imports_or_exports(self, self.parent(context))
            }
            _ if self.is_type_keyword_token_or_identifier(context) => is_named_imports_or_exports(
                self,
                self.parent(context)
                    .and_then(|parent| self.parent_of(parent)),
            ),
            _ => None,
        };
        let Some(named) = named else {
            return Ok(GlobalsSearch::Continue);
        };
        // At least `type` at `import { |`.
        if !self.is_type_keyword_token_or_identifier(context) {
            collector.keyword_filters = KeywordCompletionFilters::TypeKeyword;
        }
        // The exports of the module imported or re-exported.
        let declaration = if self.node_kind(named) == SyntaxKind::NamedImports {
            self.parent_of(named)
                .and_then(|clause| self.parent_of(clause))
        } else {
            self.parent_of(named)
        };
        let Some(module_specifier) =
            declaration.and_then(|declaration| self.module_specifier_of(declaration))
        else {
            collector.is_new_identifier_location = true;
            if self.node_kind(named) == SyntaxKind::NamedImports {
                return Ok(GlobalsSearch::Fail);
            }
            return Ok(GlobalsSearch::Continue);
        };
        let Some(module_symbol) = self
            .request
            .state
            .get_symbol_at_location(module_specifier)?
        else {
            collector.is_new_identifier_location = true;
            return Ok(GlobalsSearch::Fail);
        };
        collector.completion_kind = CompletionKind::MemberLike;
        collector.is_new_identifier_location = false;
        let exports = self
            .request
            .state
            .get_exports_and_properties_of_module(module_symbol)?;
        let elements = match self.data(named) {
            NodeData::NamedImports(data) => self.node_list(data.elements),
            NodeData::NamedExports(data) => self.node_list(data.elements),
            _ => &[],
        };
        let mut existing = HashSet::new();
        for &element in elements {
            if self.is_currently_editing_node(Touching::Node(element), self.position) {
                continue;
            }
            let (property_name, name) = match self.data(element) {
                NodeData::ImportSpecifier(data) => (data.property_name, data.name),
                NodeData::ExportSpecifier(data) => (data.property_name, data.name),
                _ => (None, None),
            };
            if let Some(name) = property_name.or(name) {
                existing.insert(self.module_export_name_text(name));
            }
        }
        let uniques = exports
            .into_iter()
            .filter(|&symbol| {
                let name = self.symbol_name(symbol);
                name != "default" && !existing.contains(&name)
            })
            .collect::<Vec<_>>();
        if uniques.is_empty() {
            // With nothing else to import, not `type` either.
            collector.keyword_filters = KeywordCompletionFilters::None;
        }
        collector.symbols.extend(uniques);
        Ok(GlobalsSearch::Success)
    }

    /// The text of an import or export specifier's name (an identifier or a
    /// string literal).
    pub(super) fn module_export_name_text(&self, name: NodeId) -> String {
        let source = self.source_of(name);
        node_util::id_text(source, name)
            .map(str::to_owned)
            .or_else(|| {
                node_util::literal_text_of(source, name)
                    .map(|text| text.to_string_lossy().into_owned())
            })
            .unwrap_or_default()
    }

    /// tsgo `tryGetImportAttributesCompletionSymbols`:
    /// `import { x } from "foo" with { | }`.
    pub(super) fn try_get_import_attributes_completion_symbols(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
    ) -> CheckResult<GlobalsSearch> {
        let Some(context) = facts.context_token else {
            return Ok(GlobalsSearch::Continue);
        };
        let attributes = match self.kind(context) {
            SyntaxKind::OpenBraceToken | SyntaxKind::CommaToken => self.parent(context),
            SyntaxKind::ColonToken => self
                .parent(context)
                .and_then(|parent| self.parent_of(parent)),
            _ => None,
        };
        let Some(attributes) =
            attributes.filter(|&node| self.node_kind(node) == SyntaxKind::ImportAttributes)
        else {
            return Ok(GlobalsSearch::Continue);
        };
        let elements = match self.data(attributes) {
            NodeData::ImportAttributes(data) => self.node_list(data.elements),
            _ => &[],
        };
        let existing = elements
            .iter()
            .filter_map(|&element| self.name_of(element))
            .map(|name| self.module_export_name_text(name))
            .collect::<HashSet<_>>();
        let ty = self.request.state.get_type_at_location(attributes)?;
        for symbol in self.request.state.get_apparent_properties(ty)? {
            if !existing.contains(&self.symbol_name(symbol)) {
                collector.symbols.push(symbol);
            }
        }
        Ok(GlobalsSearch::Success)
    }

    /// tsgo `tryGetLocalNamedExportCompletionSymbols`: the locals of the
    /// file or module at `export { | };` (a module specifier's clause the
    /// import and export clause search answers).
    pub(super) fn try_get_local_named_export_completion_symbols(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
    ) -> CheckResult<GlobalsSearch> {
        let Some(context) = facts.context_token else {
            return Ok(GlobalsSearch::Continue);
        };
        let named_exports = match self.kind(context) {
            SyntaxKind::OpenBraceToken | SyntaxKind::CommaToken => self
                .parent(context)
                .filter(|&parent| self.node_kind(parent) == SyntaxKind::NamedExports),
            _ => None,
        };
        let Some(named_exports) = named_exports else {
            return Ok(GlobalsSearch::Continue);
        };
        let locals_container = self
            .find_ancestor(Some(named_exports), |this, node| {
                matches!(
                    this.node_kind(node),
                    SyntaxKind::SourceFile | SyntaxKind::ModuleDeclaration
                )
            })
            .expect("a file or module around named exports");
        collector.completion_kind = CompletionKind::None;
        collector.is_new_identifier_location = false;
        let local_exports = self
            .request
            .state
            .node_symbol(locals_container)
            .map(|symbol| self.request.state.symbol_exports(symbol));
        let locals = self
            .request
            .state
            .binder
            .locals_of(locals_container)
            .map(|table| {
                table
                    .iter()
                    .map(|(&name, &symbol)| (name, symbol))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for (name, symbol) in locals {
            collector.symbols.push(symbol);
            if local_exports
                .as_ref()
                .is_some_and(|exports| exports.contains_key(name))
            {
                collector
                    .symbol_to_sort_text_map
                    .insert(symbol, sort_text::OPTIONAL_MEMBER.to_owned());
            }
        }
        Ok(GlobalsSearch::Success)
    }

    /// tsgo `tryGetClassLikeCompletionSymbols`: at a class member, the base
    /// types' members not implemented yet; at an interface member,
    /// keywords only.
    pub(super) fn try_get_class_like_completion_symbols(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
    ) -> CheckResult<GlobalsSearch> {
        let Some(declaration) = self.try_get_object_type_declaration_completion_container(
            facts.context_token,
            facts.location,
        ) else {
            return Ok(GlobalsSearch::Continue);
        };
        let context = facts.context_token.expect("a context token");
        // The parent type's property names, at a new member.
        collector.completion_kind = CompletionKind::MemberLike;
        collector.is_new_identifier_location = true;
        let is_class = is_class_like(self.node_kind(declaration));
        collector.keyword_filters = if self.kind(context) == SyntaxKind::AsteriskToken {
            KeywordCompletionFilters::None
        } else if is_class {
            KeywordCompletionFilters::ClassElementKeywords
        } else {
            KeywordCompletionFilters::InterfaceElementKeywords
        };
        // An interface repeats nothing from its base interfaces.
        if !is_class {
            return Ok(GlobalsSearch::Success);
        }
        let class_element = if self.kind(context) == SyntaxKind::SemicolonToken {
            self.parent(context)
                .and_then(|parent| self.parent_of(parent))
        } else {
            self.parent(context)
        };
        let mut modifier_flags = match class_element {
            Some(element) if crate::utilities::is_class_element(self.node_kind(element)) => {
                node_util::get_effective_modifier_flags(self.source_of(element), element)
            }
            _ => ModifierFlags::NONE,
        };
        // A context token not being written may become a modifier.
        if self.kind(context) == SyntaxKind::Identifier
            && !self.is_currently_editing_node(context, self.position)
        {
            match self.identifier_text(context) {
                Some("private") => modifier_flags |= ModifierFlags::PRIVATE,
                Some("static") => modifier_flags |= ModifierFlags::STATIC,
                Some("override") => modifier_flags |= ModifierFlags::OVERRIDE,
                _ => {}
            }
        }
        if class_element.is_some_and(|element| {
            self.node_kind(element) == SyntaxKind::ClassStaticBlockDeclaration
        }) {
            modifier_flags |= ModifierFlags::STATIC;
        }
        // No member list for a private member.
        if !modifier_flags.intersects(ModifierFlags::PRIVATE) {
            // The base types' members neither private nor implemented.
            let base_type_nodes = if modifier_flags.intersects(ModifierFlags::OVERRIDE) {
                node_util::get_class_extends_heritage_element(
                    self.request.state.binder.source_of_node(declaration),
                    declaration,
                )
                .into_iter()
                .collect()
            } else {
                node_util::get_all_super_type_nodes(
                    self.request.state.binder.source_of_node(declaration),
                    declaration,
                )
            };
            let mut base_symbols = Vec::new();
            for base_type_node in base_type_nodes {
                let ty = self.request.state.get_type_at_location(base_type_node)?;
                if modifier_flags.intersects(ModifierFlags::STATIC) {
                    if let Some(symbol) = self.request.state.tables.type_of(ty).symbol {
                        let static_type = self
                            .request
                            .state
                            .get_type_of_symbol_at_location(symbol, Some(declaration))?;
                        base_symbols
                            .extend(self.request.state.get_properties_of_type(static_type)?);
                    }
                } else {
                    base_symbols.extend(self.request.state.get_properties_of_type(ty)?);
                }
            }
            let members = match self.data(declaration) {
                NodeData::ClassDeclaration(data) => self.node_list(data.members).to_vec(),
                NodeData::ClassExpression(data) => self.node_list(data.members).to_vec(),
                _ => Vec::new(),
            };
            let filtered = self.filter_class_members_list(base_symbols, &members, modifier_flags);
            collector.symbols.extend(filtered);
            for index in 0..collector.symbols.len() {
                let symbol = collector.symbols[index];
                let Some(declaration) = self.request.state.binder.symbol(symbol).value_declaration
                else {
                    continue;
                };
                let kind = self.node_kind(declaration);
                let computed = crate::utilities::is_class_element(kind)
                    && self.name_of(declaration).is_some_and(|name| {
                        self.node_kind(name) == SyntaxKind::ComputedPropertyName
                    });
                if computed {
                    // tsgo `SymbolToString`.
                    let symbol_name = self.request.state.symbol_to_string_with_flags(
                        symbol,
                        None,
                        SymbolFlags::ALL,
                        tsc_checker::SymbolFormatFlags::ALLOW_ANY_NODE_KIND,
                    )?;
                    collector.symbol_to_origin_info_map.insert(
                        index,
                        SymbolOriginInfo {
                            kind: origin_kind::COMPUTED_PROPERTY_NAME,
                            symbol_name: Some(symbol_name),
                        },
                    );
                }
            }
        }
        Ok(GlobalsSearch::Success)
    }

    /// tsgo `tryGetObjectTypeDeclarationCompletionContainer`: the class or
    /// interface (or type literal) whose member the position starts.
    fn try_get_object_type_declaration_completion_container(
        &self,
        context: Option<Touching>,
        location: Touching,
    ) -> Option<NodeId> {
        // class c { method() { } | method2() { } }
        match location {
            Touching::Node(node) => match self.node_kind(node) {
                SyntaxKind::EndOfFileToken => {
                    let file = self.parent_of(node)?;
                    let statements = match self.data(file) {
                        NodeData::SourceFile(data) => self.node_list(data.statements),
                        _ => &[],
                    };
                    if let Some(&last) = statements.last() {
                        if is_object_type_declaration(self.node_kind(last))
                            && self
                                .child_of_kind(last, SyntaxKind::CloseBraceToken)
                                .is_none()
                        {
                            return Some(last);
                        }
                    }
                }
                SyntaxKind::PrivateIdentifier => {
                    if self.parent_of(node).is_some_and(|parent| {
                        self.node_kind(parent) == SyntaxKind::PropertyDeclaration
                    }) {
                        return self.find_ancestor(Some(node), |this, node| {
                            is_class_like(this.node_kind(node))
                        });
                    }
                }
                SyntaxKind::Identifier => {
                    if self
                        .identifier_text(location)
                        .and_then(identifier_to_keyword_kind)
                        .is_some()
                    {
                        return None;
                    }
                    let parent = self.parent_of(node);
                    // class c { public prop = c| }
                    if parent.is_some_and(|parent| {
                        self.node_kind(parent) == SyntaxKind::PropertyDeclaration
                            && self.initializer_of(parent) == Some(node)
                    }) {
                        return None;
                    }
                    // class c extends React.Component { a: () => 1\n compon| }
                    if self.is_from_object_type_declaration(location) {
                        return self.find_ancestor(Some(node), |this, node| {
                            is_object_type_declaration(this.node_kind(node))
                        });
                    }
                }
                _ => {}
            },
            Touching::Token { .. } => {}
        }
        let context = context?;
        // class C { blah; constructor/**/ } or class C { blah \n constructor/**/ }
        if self.kind(location) == SyntaxKind::ConstructorKeyword
            || self.kind(context) == SyntaxKind::Identifier
                && self
                    .parent(context)
                    .is_some_and(|parent| self.node_kind(parent) == SyntaxKind::PropertyDeclaration)
                && is_class_like(self.kind(location))
        {
            return self.find_ancestor(self.parent(context), |this, node| {
                is_class_like(this.node_kind(node))
            });
        }
        let location_node = match location {
            Touching::Node(node) => Some(node),
            Touching::Token { .. } => None,
        };
        match self.kind(context) {
            // class c { public prop = | /* global completions */ }
            SyntaxKind::EqualsToken => None,
            // class c {getValue(): number; | } or class c { method() { } | }
            SyntaxKind::SemicolonToken | SyntaxKind::CloseBraceToken => {
                // class c { method() { } b| }
                if self.is_from_object_type_declaration(location) {
                    let parent = self.parent(location)?;
                    if self.name_of(parent).map(Touching::Node) == Some(location) {
                        return self.parent_of(parent);
                    }
                }
                location_node.filter(|&node| is_object_type_declaration(self.node_kind(node)))
            }
            // class c { | or class c {getValue(): number, | }
            SyntaxKind::OpenBraceToken | SyntaxKind::CommaToken => self
                .parent(context)
                .filter(|&parent| is_object_type_declaration(self.node_kind(parent))),
            _ => {
                let node = location_node
                    .filter(|&node| is_object_type_declaration(self.node_kind(node)))?;
                // class C extends React.Component { a: () => 1\n| } or
                // class C { prop = ""\n | }
                if self.line_of(self.end(context)) != self.line_of(self.position) {
                    return Some(node);
                }
                let grandparent = self
                    .parent(context)
                    .and_then(|parent| self.parent_of(parent));
                let class_member =
                    grandparent.is_some_and(|node| is_class_like(self.node_kind(node)));
                let is_valid_keyword = |kind: SyntaxKind| {
                    if class_member {
                        is_class_member_completion_keyword(kind)
                    } else {
                        kind == SyntaxKind::ReadonlyKeyword
                    }
                };
                let kind = self.kind(context);
                if is_valid_keyword(kind)
                    || kind == SyntaxKind::AsteriskToken
                    || kind == SyntaxKind::Identifier
                        && self
                            .identifier_text(context)
                            .and_then(identifier_to_keyword_kind)
                            .is_some_and(is_valid_keyword)
                {
                    return grandparent;
                }
                None
            }
        }
    }

    /// tsgo `filterClassMembersList`: the base members not implemented,
    /// private or a private name.
    fn filter_class_members_list(
        &self,
        base_symbols: Vec<SymbolId>,
        existing_members: &[NodeId],
        modifier_flags: ModifierFlags,
    ) -> Vec<SymbolId> {
        let mut existing_names = HashSet::new();
        for &member in existing_members {
            // Not an omitted expression of a missing member.
            if !matches!(
                self.node_kind(member),
                SyntaxKind::PropertyDeclaration
                    | SyntaxKind::MethodDeclaration
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor
            ) {
                continue;
            }
            // Not the member being written.
            if self.is_currently_editing_node(Touching::Node(member), self.position) {
                continue;
            }
            let source = self.source_of(member);
            let flags = node_util::get_effective_modifier_flags(source, member);
            // A private member does not filter the name out.
            if flags.intersects(ModifierFlags::PRIVATE) {
                continue;
            }
            // Nor a member of the other static side.
            if flags.intersects(ModifierFlags::STATIC)
                != modifier_flags.intersects(ModifierFlags::STATIC)
            {
                continue;
            }
            if let Some(name) = self.name_of(member) {
                let existing = self.property_name_for_property_name_node(name);
                if !existing.is_empty() {
                    existing_names.insert(existing);
                }
            }
        }
        base_symbols
            .into_iter()
            .filter(|&property| {
                let data = self.request.state.binder.symbol(property);
                !existing_names.contains(&self.symbol_name(property))
                    && !data.declarations.is_empty()
                    && !self
                        .request
                        .state
                        .get_declaration_modifier_flags_from_symbol(property)
                        .intersects(ModifierFlags::PRIVATE)
                    && !data.value_declaration.is_some_and(|declaration| {
                        let source = self.source_of(declaration);
                        node_util::get_name_of_declaration(source, declaration).is_some_and(
                            |name| source.arena.node(name).kind == SyntaxKind::PrivateIdentifier,
                        )
                    })
            })
            .collect()
    }

    /// tsgo `ast.GetPropertyNameForPropertyNameNode`.
    fn property_name_for_property_name_node(&self, name: NodeId) -> String {
        let source = self.source_of(name);
        match self.node_kind(name) {
            SyntaxKind::Identifier | SyntaxKind::PrivateIdentifier => {
                node_util::id_text(source, name)
                    .unwrap_or_default()
                    .to_owned()
            }
            SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral => node_util::literal_text_of(source, name)
                .map(|text| text.to_string_lossy().into_owned())
                .or_else(|| match self.data(name) {
                    NodeData::BigIntLiteral(data) => Some(data.text.clone()),
                    _ => None,
                })
                .unwrap_or_default(),
            SyntaxKind::ComputedPropertyName => {
                let Some(expression) = self.expression_of(name) else {
                    return INTERNAL_SYMBOL_NAME_MISSING.to_owned();
                };
                if node_util::is_string_or_numeric_literal_like(source, expression) {
                    return node_util::literal_text_of(source, expression)
                        .map(|text| text.to_string_lossy().into_owned())
                        .unwrap_or_default();
                }
                if let NodeData::PrefixUnaryExpression(data) = self.data(expression) {
                    if node_util::is_signed_numeric_literal(source, expression) {
                        let text = data
                            .operand
                            .and_then(|operand| node_util::literal_text_of(source, operand))
                            .map(|text| text.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        if data.operator == SyntaxKind::MinusToken {
                            return format!("-{text}");
                        }
                        return text;
                    }
                }
                INTERNAL_SYMBOL_NAME_MISSING.to_owned()
            }
            _ => String::new(),
        }
    }

    /// tsgo `tryGetJsxCompletionSymbols`: the attributes of the JSX
    /// element the position is in, less those written.
    pub(super) fn try_get_jsx_completion_symbols(
        &mut self,
        facts: &Location,
        collector: &mut Collector,
    ) -> CheckResult<GlobalsSearch> {
        let Some(jsx_container) = self.try_get_containing_jsx_element(facts.context_token) else {
            return Ok(GlobalsSearch::Continue);
        };
        let attributes = match self.data(jsx_container) {
            NodeData::JsxOpeningElement(data) => data.attributes,
            NodeData::JsxSelfClosingElement(data) => data.attributes,
            _ => None,
        }
        .expect("a JSX element's attributes");
        // In a self-closing or opening element.
        let Some(attributes_type) = self
            .request
            .state
            .language_service_contextual_type(attributes, ContextFlags::NONE)?
        else {
            return Ok(GlobalsSearch::Continue);
        };
        let completions_type = self
            .request
            .state
            .language_service_contextual_type(attributes, ContextFlags::IGNORE_NODE_INFERENCES)?;
        let properties =
            self.properties_for_object_expression(attributes_type, completions_type, attributes)?;
        let written = match self.data(attributes) {
            NodeData::JsxAttributes(data) => self.node_list(data.properties).to_vec(),
            _ => Vec::new(),
        };
        let (filtered, spread_member_names) = self.filter_jsx_attributes(properties, &written)?;
        for &symbol in &filtered {
            collector.symbols.push(symbol);
            if spread_member_names.contains(&self.symbol_name(symbol)) {
                collector.symbol_to_sort_text_map.insert(
                    symbol,
                    sort_text::MEMBER_DECLARED_BY_SPREAD_ASSIGNMENT.to_owned(),
                );
            }
            if self
                .request
                .state
                .symbol_flags(symbol)
                .intersects(SymbolFlags::OPTIONAL)
            {
                collector
                    .symbol_to_sort_text_map
                    .entry(symbol)
                    .or_insert_with(|| sort_text::OPTIONAL_MEMBER.to_owned());
            }
        }
        collector.completion_kind = CompletionKind::MemberLike;
        collector.is_new_identifier_location = false;
        Ok(GlobalsSearch::Success)
    }

    /// tsgo `tryGetContainingJsxElement`.
    fn try_get_containing_jsx_element(&self, context: Option<Touching>) -> Option<NodeId> {
        let context = context?;
        let parent = self.parent(context);
        let is_opening_like = |node: Option<NodeId>| {
            node.is_some_and(|node| {
                matches!(
                    self.node_kind(node),
                    SyntaxKind::JsxSelfClosingElement | SyntaxKind::JsxOpeningElement
                )
            })
        };
        let parent_kind = parent.map(|parent| self.node_kind(parent));
        match self.kind(context) {
            SyntaxKind::GreaterThanToken
            | SyntaxKind::LessThanSlashToken
            | SyntaxKind::SlashToken
            | SyntaxKind::Identifier
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::JsxNamespacedName
            | SyntaxKind::JsxAttributes
            | SyntaxKind::JsxAttribute
            | SyntaxKind::JsxSpreadAttribute => {
                let parent = parent?;
                if is_opening_like(Some(parent)) {
                    if self.kind(context) == SyntaxKind::GreaterThanToken {
                        let preceding = self.preceding_token(self.pos(context));
                        let type_arguments = match self.data(parent) {
                            NodeData::JsxOpeningElement(data) => data.type_arguments,
                            NodeData::JsxSelfClosingElement(data) => data.type_arguments,
                            _ => None,
                        };
                        if self.node_list(type_arguments).is_empty()
                            || preceding.is_some_and(|preceding| {
                                self.kind(preceding) == SyntaxKind::SlashToken
                            })
                        {
                            return None;
                        }
                    }
                    return Some(parent);
                }
                if parent_kind == Some(SyntaxKind::JsxNamespacedName) {
                    let grandparent = self.parent_of(parent);
                    if is_opening_like(grandparent) {
                        return grandparent;
                    }
                } else if parent_kind == Some(SyntaxKind::JsxAttribute) {
                    // JsxOpeningLikeElement > JsxAttributes > JsxAttribute.
                    return self
                        .parent_of(parent)
                        .and_then(|attributes| self.parent_of(attributes));
                }
                None
            }
            // The closing `"` of an attribute: JsxAttribute's string.
            SyntaxKind::StringLiteral => {
                if matches!(
                    parent_kind,
                    Some(SyntaxKind::JsxAttribute | SyntaxKind::JsxSpreadAttribute)
                ) {
                    return parent
                        .and_then(|parent| self.parent_of(parent))
                        .and_then(|attributes| self.parent_of(attributes));
                }
                None
            }
            // The closing `}` of an attribute's expression or of a spread.
            SyntaxKind::CloseBraceToken => {
                let parent = parent?;
                if parent_kind == Some(SyntaxKind::JsxExpression)
                    && self
                        .parent_of(parent)
                        .is_some_and(|node| self.node_kind(node) == SyntaxKind::JsxAttribute)
                {
                    return self
                        .parent_of(parent)
                        .and_then(|attribute| self.parent_of(attribute))
                        .and_then(|attributes| self.parent_of(attributes));
                }
                if parent_kind == Some(SyntaxKind::JsxSpreadAttribute) {
                    return self
                        .parent_of(parent)
                        .and_then(|attributes| self.parent_of(attributes));
                }
                None
            }
            _ => None,
        }
    }

    /// tsgo `filterJsxAttributes`: the attributes not written, and the
    /// names spread attributes declare.
    fn filter_jsx_attributes(
        &mut self,
        symbols: Vec<SymbolId>,
        attributes: &[NodeId],
    ) -> CheckResult<(Vec<SymbolId>, HashSet<String>)> {
        let mut existing_names = HashSet::new();
        let mut spread_member_names = HashSet::new();
        for &attribute in attributes {
            // Not the attribute being written.
            if self.is_currently_editing_node(Touching::Node(attribute), self.position) {
                continue;
            }
            match self.node_kind(attribute) {
                SyntaxKind::JsxAttribute => {
                    if let Some(name) = self.name_of(attribute) {
                        existing_names.insert(self.jsx_attribute_name_text(name));
                    }
                }
                SyntaxKind::JsxSpreadAttribute => {
                    self.set_member_declared_by_spread_assignment(
                        attribute,
                        &mut spread_member_names,
                    )?;
                }
                _ => {}
            }
        }
        let filtered = symbols
            .into_iter()
            .filter(|&symbol| !existing_names.contains(&self.raw_symbol_name(symbol)))
            .collect();
        Ok((filtered, spread_member_names))
    }

    /// A JSX attribute's name text (`a` or `ns:a`).
    pub(super) fn jsx_attribute_name_text(&self, name: NodeId) -> String {
        let source = self.source_of(name);
        if let Some(text) = node_util::id_text(source, name) {
            return text.to_owned();
        }
        node_util::get_text_of_jsx_namespaced_name(source, name).unwrap_or_default()
    }

    /// A symbol's name as tsgo's `Symbol.Name` spells it.
    pub(super) fn raw_symbol_name(&self, symbol: SymbolId) -> String {
        tsc_binder::unescape_leading_underscores(
            self.request.state.binder.symbol(symbol).escaped_name,
        )
        .to_string_lossy()
        .into_owned()
    }

    /// tsgo `ast.GetRootDeclaration`.
    fn root_declaration(&self, mut node: NodeId) -> NodeId {
        while self.node_kind(node) == SyntaxKind::BindingElement {
            node = self
                .parent_of(node)
                .and_then(|pattern| self.parent_of(pattern))
                .expect("a binding element's declaration");
        }
        node
    }

    /// tsgo `ast.IsExpression`.
    pub(super) fn is_expression(&self, node: NodeId) -> bool {
        let mut node = node;
        while let NodeData::PartiallyEmittedExpression(data) = self.data(node) {
            match data.expression {
                Some(expression) => node = expression,
                None => break,
            }
        }
        is_expression_kind(self.node_kind(node))
    }
}

/// tsgo `ast.isExpressionKind`.
fn is_expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ConditionalExpression
            | SyntaxKind::YieldExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::BinaryExpression
            | SyntaxKind::SpreadElement
            | SyntaxKind::AsExpression
            | SyntaxKind::OmittedExpression
            | SyntaxKind::PartiallyEmittedExpression
            | SyntaxKind::SatisfiesExpression
    ) || is_unary_expression_kind(kind)
}

/// tsgo `ast.isUnaryExpressionKind`.
fn is_unary_expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PrefixUnaryExpression
            | SyntaxKind::PostfixUnaryExpression
            | SyntaxKind::DeleteExpression
            | SyntaxKind::TypeOfExpression
            | SyntaxKind::VoidExpression
            | SyntaxKind::AwaitExpression
            | SyntaxKind::TypeAssertionExpression
    ) || is_left_hand_side_expression_kind(kind)
}

/// tsgo `ast.isLeftHandSideExpressionKind`.
fn is_left_hand_side_expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::NewExpression
            | SyntaxKind::CallExpression
            | SyntaxKind::JsxElement
            | SyntaxKind::JsxSelfClosingElement
            | SyntaxKind::JsxFragment
            | SyntaxKind::TaggedTemplateExpression
            | SyntaxKind::ArrayLiteralExpression
            | SyntaxKind::ParenthesizedExpression
            | SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::ClassExpression
            | SyntaxKind::FunctionExpression
            | SyntaxKind::Identifier
            | SyntaxKind::PrivateIdentifier
            | SyntaxKind::RegularExpressionLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TemplateExpression
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::ThisKeyword
            | SyntaxKind::TrueKeyword
            | SyntaxKind::SuperKeyword
            | SyntaxKind::NonNullExpression
            | SyntaxKind::ExpressionWithTypeArguments
            | SyntaxKind::MetaProperty
            | SyntaxKind::ImportKeyword
            | SyntaxKind::MissingDeclaration
    )
}
