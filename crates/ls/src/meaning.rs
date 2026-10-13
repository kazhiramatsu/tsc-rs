//! tsgo's semantic meaning of a location (`getMeaningFromLocation`,
//! `getMeaningFromDeclaration` and `getAdjustedLocation` of
//! ls/utilities.go): whether a name stands for a value, a type, a
//! namespace or all of them.

use tsc_binder::containers::{get_module_instance_state, ModuleInstanceState};
use tsc_binder::node_util;
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags};

use crate::request::Request;
use crate::utilities::Touching;

/// tsgo `ast.SemanticMeaning`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SemanticMeaning(pub(crate) u8);

impl SemanticMeaning {
    pub(crate) const VALUE: Self = Self(1);
    pub(crate) const TYPE: Self = Self(2);
    pub(crate) const NAMESPACE: Self = Self(4);
    pub(crate) const ALL: Self = Self(7);

    pub(crate) fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl Request<'_, '_> {
    /// tsgo `getMeaningFromLocation`.
    pub(crate) fn meaning_from_location(&self, node: Touching) -> SemanticMeaning {
        let node = self.adjusted_location(node, false);
        let kind = self.kind(node);
        let Some(parent) = self.parent(node) else {
            return SemanticMeaning::VALUE;
        };
        let parent_kind = self.node_kind(parent);
        if kind == SyntaxKind::SourceFile {
            return SemanticMeaning::VALUE;
        }
        let node_id = match node {
            Touching::Node(id) => Some(id),
            Touching::Token { .. } => None,
        };
        if matches!(
            parent_kind,
            SyntaxKind::ExportAssignment
                | SyntaxKind::ExportSpecifier
                | SyntaxKind::ExternalModuleReference
                | SyntaxKind::ImportSpecifier
                | SyntaxKind::ImportClause
        ) || parent_kind == SyntaxKind::ImportEqualsDeclaration
            && node_id.is_some()
            && self.name_of(parent) == node_id
        {
            return SemanticMeaning::ALL;
        }
        let Some(id) = node_id else {
            if parent_kind == SyntaxKind::TypeParameter {
                return SemanticMeaning::TYPE;
            }
            if parent_kind == SyntaxKind::LiteralType {
                return SemanticMeaning::TYPE.union(SemanticMeaning::VALUE);
            }
            return SemanticMeaning::VALUE;
        };
        if self.is_in_right_side_of_internal_import_equals_declaration(id) {
            // import a = |b|; // Namespace
            // import a = |b.c|; // Value, type, namespace
            // import a = |b.c|.d; // Namespace
            let name = if kind == SyntaxKind::QualifiedName {
                Some(id)
            } else {
                match &self.state.binder.node_record(parent).data {
                    NodeData::QualifiedName(qualified) if qualified.right == Some(id) => {
                        Some(parent)
                    }
                    _ => None,
                }
            };
            if name.is_some_and(|name| {
                self.state
                    .binder
                    .node_record(name)
                    .parent
                    .is_some_and(|parent| {
                        self.node_kind(parent) == SyntaxKind::ImportEqualsDeclaration
                    })
            }) {
                return SemanticMeaning::ALL;
            }
            return SemanticMeaning::NAMESPACE;
        }
        if self.is_declaration_name(id) {
            return self.meaning_from_declaration(parent);
        }
        if matches!(kind, SyntaxKind::Identifier | SyntaxKind::QualifiedName)
            && self.is_js_doc_name_reference_context(id)
        {
            return SemanticMeaning::ALL;
        }
        if self.is_type_reference(id) {
            return SemanticMeaning::TYPE;
        }
        if self.is_namespace_reference(id) {
            return SemanticMeaning::NAMESPACE;
        }
        if parent_kind == SyntaxKind::TypeParameter {
            return SemanticMeaning::TYPE;
        }
        if parent_kind == SyntaxKind::LiteralType {
            // This might be T["name"], which is actually referencing a
            // property and not a type: both meanings.
            return SemanticMeaning::TYPE.union(SemanticMeaning::VALUE);
        }
        SemanticMeaning::VALUE
    }

    /// tsgo `getMeaningFromDeclaration`.
    pub(crate) fn meaning_from_declaration(&self, node: NodeId) -> SemanticMeaning {
        match self.node_kind(node) {
            SyntaxKind::VariableDeclaration
            | SyntaxKind::Parameter
            | SyntaxKind::BindingElement
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::Constructor
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::CatchClause
            | SyntaxKind::JsxAttribute => SemanticMeaning::VALUE,
            SyntaxKind::TypeParameter
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::JSDocTypedefTag
            | SyntaxKind::JSDocCallbackTag
            | SyntaxKind::TypeLiteral => SemanticMeaning::TYPE,
            SyntaxKind::EnumMember | SyntaxKind::ClassDeclaration => {
                SemanticMeaning::VALUE.union(SemanticMeaning::TYPE)
            }
            SyntaxKind::ModuleDeclaration => {
                let source = self.state.binder.source_of_node(node);
                if node_util::is_ambient_module(source, node)
                    || get_module_instance_state(source, node, &mut Default::default())
                        == ModuleInstanceState::Instantiated
                {
                    SemanticMeaning::NAMESPACE.union(SemanticMeaning::VALUE)
                } else {
                    SemanticMeaning::NAMESPACE
                }
            }
            SyntaxKind::EnumDeclaration
            | SyntaxKind::NamedImports
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportDeclaration
            | SyntaxKind::JSDocImportTag
            | SyntaxKind::ExportAssignment
            | SyntaxKind::ExportDeclaration => SemanticMeaning::ALL,
            // An external module can be a value.
            SyntaxKind::SourceFile => SemanticMeaning::NAMESPACE.union(SemanticMeaning::VALUE),
            _ => SemanticMeaning::ALL,
        }
    }

    /// tsgo `getAdjustedLocation`: a modifier or keyword of a declaration,
    /// import, export or type stands for the name or specifier it
    /// introduces.
    pub(crate) fn adjusted_location(&self, node: Touching, for_rename: bool) -> Touching {
        let Some(parent) = self.parent(node) else {
            return node;
        };
        let kind = self.kind(node);
        let parent_kind = self.node_kind(parent);
        let parent_data = &self.state.binder.node_record(parent).data;
        // /**/<modifier> [|name|] ...
        // /**/<modifier> <class|interface|type|enum|module|namespace|function|get|set> [|name|] ...
        // /**/import [|name|] = ...
        if self.is_adjustable_modifier(node, parent, for_rename) {
            if let Some(location) = self.adjusted_location_for_declaration(parent, for_rename) {
                return location;
            }
        }
        // /**/<var|let|const> [|name|] ...
        if matches!(
            kind,
            SyntaxKind::VarKeyword | SyntaxKind::ConstKeyword | SyntaxKind::LetKeyword
        ) {
            if let NodeData::VariableDeclarationList(list) = parent_data {
                let declarations = list
                    .declarations
                    .map(|list| self.state.binder.node_array(list).nodes)
                    .unwrap_or_default();
                if let [declaration] = declarations {
                    if let Some(name) = self.name_of(*declaration) {
                        if self.node_kind(name) == SyntaxKind::Identifier {
                            return Touching::Node(name);
                        }
                    }
                }
            }
        }
        if kind == SyntaxKind::TypeKeyword {
            // import /**/type [|name|] from ...;
            if let NodeData::ImportClause(clause) = parent_data {
                if clause.is_type_only {
                    let import = self
                        .state
                        .binder
                        .node_record(parent)
                        .parent
                        .expect("an import");
                    if let Some(location) =
                        self.adjusted_location_for_import_declaration(import, for_rename)
                    {
                        return location;
                    }
                }
            }
            // export /**/type { [|name|] } from ...;
            if let NodeData::ExportDeclaration(export) = parent_data {
                if export.is_type_only {
                    if let Some(location) =
                        self.adjusted_location_for_export_declaration(parent, for_rename)
                    {
                        return location;
                    }
                }
            }
        }
        // import { propertyName /**/as [|name|] } ...
        // import * /**/as [|name|] ...
        // export { propertyName /**/as [|name|] } ...
        // export * /**/as [|name|] ...
        if kind == SyntaxKind::AsKeyword {
            match parent_data {
                NodeData::ImportSpecifier(specifier) if specifier.property_name.is_some() => {
                    if let Some(name) = specifier.name {
                        return Touching::Node(name);
                    }
                }
                NodeData::ExportSpecifier(specifier) if specifier.property_name.is_some() => {
                    if let Some(name) = specifier.name {
                        return Touching::Node(name);
                    }
                }
                NodeData::NamespaceImport(namespace) => {
                    if let Some(name) = namespace.name {
                        return Touching::Node(name);
                    }
                }
                NodeData::NamespaceExport(namespace) => {
                    if let Some(name) = namespace.name {
                        return Touching::Node(name);
                    }
                }
                NodeData::ExportDeclaration(export) => {
                    if let Some(clause) = export.export_clause {
                        if let NodeData::NamespaceExport(namespace) =
                            &self.state.binder.node_record(clause).data
                        {
                            if let Some(name) = namespace.name {
                                return Touching::Node(name);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // /**/import [|name|] from ...;
        // /**/import "[|module|]";
        if kind == SyntaxKind::ImportKeyword && parent_kind == SyntaxKind::ImportDeclaration {
            if let Some(location) =
                self.adjusted_location_for_import_declaration(parent, for_rename)
            {
                return location;
            }
        }
        if kind == SyntaxKind::ExportKeyword {
            // /**/export { [|name|] } ...;
            // /**/export * from "[|module|]";
            if parent_kind == SyntaxKind::ExportDeclaration {
                if let Some(location) =
                    self.adjusted_location_for_export_declaration(parent, for_rename)
                {
                    return location;
                }
            }
            // /**/export default [|name|];
            // /**/export = [|name|];
            if let NodeData::ExportAssignment(assignment) = parent_data {
                if let Some(expression) = assignment.expression {
                    return Touching::Node(self.skip_outer_expressions(expression));
                }
            }
        }
        // import name = /**/require("[|module|]");
        if kind == SyntaxKind::RequireKeyword {
            if let NodeData::ExternalModuleReference(reference) = parent_data {
                if let Some(expression) = reference.expression {
                    return Touching::Node(expression);
                }
            }
        }
        // import ... /**/from "[|module|]";
        // export ... /**/from "[|module|]";
        if kind == SyntaxKind::FromKeyword {
            let specifier = match parent_data {
                NodeData::ImportDeclaration(import) => import.module_specifier,
                NodeData::ExportDeclaration(export) => export.module_specifier,
                _ => None,
            };
            if let Some(specifier) = specifier {
                return Touching::Node(specifier);
            }
        }
        // class ... /**/extends [|name|] ...
        // class ... /**/implements [|name|] ...
        // interface ... /**/extends [|name|] ...
        if matches!(
            kind,
            SyntaxKind::ExtendsKeyword | SyntaxKind::ImplementsKeyword
        ) {
            if let NodeData::HeritageClause(clause) = parent_data {
                if clause.token == kind {
                    let types = clause
                        .types
                        .map(|types| self.state.binder.node_array(types).nodes)
                        .unwrap_or_default();
                    if let [element] = types {
                        if let Some(name) = self.heritage_clause_element_name(*element) {
                            return Touching::Node(name);
                        }
                    }
                }
            }
        }
        if kind == SyntaxKind::ExtendsKeyword {
            // ... <T /**/extends [|U|]> ...
            if let NodeData::TypeParameter(parameter) = parent_data {
                if let Some(name) = parameter
                    .constraint
                    .and_then(|constraint| self.type_reference_name(constraint))
                {
                    return Touching::Node(name);
                }
            }
            // ... T /**/extends [|U|] ? ...
            if let NodeData::ConditionalType(conditional) = parent_data {
                if let Some(name) = conditional
                    .extends_type
                    .and_then(|extends| self.type_reference_name(extends))
                {
                    return Touching::Node(name);
                }
            }
        }
        // ... T extends /**/infer [|U|] ? ...
        if kind == SyntaxKind::InferKeyword {
            if let NodeData::InferType(infer) = parent_data {
                if let Some(name) = infer
                    .type_parameter
                    .and_then(|parameter| self.name_of(parameter))
                {
                    return Touching::Node(name);
                }
            }
        }
        // { [ [|K|] /**/in keyof T]: ... }
        if kind == SyntaxKind::InKeyword
            && parent_kind == SyntaxKind::TypeParameter
            && self
                .state
                .binder
                .node_record(parent)
                .parent
                .is_some_and(|mapped| self.node_kind(mapped) == SyntaxKind::MappedType)
        {
            if let Some(name) = self.name_of(parent) {
                return Touching::Node(name);
            }
        }
        // /**/keyof [|T|]
        if kind == SyntaxKind::KeyOfKeyword {
            if let NodeData::TypeOperator(operator) = parent_data {
                if operator.operator == SyntaxKind::KeyOfKeyword {
                    if let Some(name) = operator.r#type.and_then(|ty| self.type_reference_name(ty))
                    {
                        return Touching::Node(name);
                    }
                }
            }
        }
        // /**/readonly [|name|][]
        if kind == SyntaxKind::ReadonlyKeyword {
            if let NodeData::TypeOperator(operator) = parent_data {
                if operator.operator == SyntaxKind::ReadonlyKeyword {
                    if let Some(array) = operator.r#type {
                        if let NodeData::ArrayType(array) =
                            &self.state.binder.node_record(array).data
                        {
                            if let Some(name) = array
                                .element_type
                                .and_then(|element| self.type_reference_name(element))
                            {
                                return Touching::Node(name);
                            }
                        }
                    }
                }
            }
        }
        if !for_rename {
            // /**/new [|name|], /**/void [|name|], /**/typeof [|name|],
            // /**/await [|name|], /**/yield [|name|], /**/delete obj.[|name|]
            let operand = match (kind, parent_data) {
                (SyntaxKind::NewKeyword, NodeData::NewExpression(data)) => data.expression,
                (SyntaxKind::VoidKeyword, NodeData::VoidExpression(data)) => data.expression,
                (SyntaxKind::TypeOfKeyword, NodeData::TypeOfExpression(data)) => data.expression,
                (SyntaxKind::AwaitKeyword, NodeData::AwaitExpression(data)) => data.expression,
                (SyntaxKind::YieldKeyword, NodeData::YieldExpression(data)) => data.expression,
                (SyntaxKind::DeleteKeyword, NodeData::DeleteExpression(data)) => data.expression,
                _ => None,
            };
            if let Some(operand) = operand {
                return Touching::Node(self.skip_outer_expressions(operand));
            }
            // left /**/in [|name|]
            // left /**/instanceof [|name|]
            if matches!(kind, SyntaxKind::InKeyword | SyntaxKind::InstanceOfKeyword) {
                if let NodeData::BinaryExpression(binary) = parent_data {
                    let is_operator = match node {
                        Touching::Node(id) => binary.operator_token == Some(id),
                        Touching::Token { .. } => binary
                            .operator_token
                            .is_none_or(|operator| self.node_kind(operator) == kind),
                    };
                    if is_operator {
                        if let Some(right) = binary.right {
                            return Touching::Node(self.skip_outer_expressions(right));
                        }
                    }
                }
            }
            // left /**/as [|name|]
            if kind == SyntaxKind::AsKeyword {
                if let NodeData::AsExpression(expression) = parent_data {
                    if let Some(name) = expression
                        .r#type
                        .and_then(|ty| self.type_reference_name(ty))
                    {
                        return Touching::Node(name);
                    }
                }
            }
            // for (... /**/in [|name|])
            // for (... /**/of [|name|])
            let iterated = match (kind, parent_data) {
                (SyntaxKind::InKeyword, NodeData::ForInStatement(statement)) => {
                    statement.expression
                }
                (SyntaxKind::OfKeyword, NodeData::ForOfStatement(statement)) => {
                    statement.expression
                }
                _ => None,
            };
            if let Some(iterated) = iterated {
                return Touching::Node(self.skip_outer_expressions(iterated));
            }
        }
        node
    }

    /// The `isModifier` of tsgo `getAdjustedLocation`: a modifier of the
    /// parent (but `default`, which `getSymbolAtLocation` handles), or the
    /// keyword of a declaration.
    fn is_adjustable_modifier(&self, node: Touching, parent: NodeId, for_rename: bool) -> bool {
        let kind = self.kind(node);
        if let Touching::Node(id) = node {
            if is_modifier_kind(kind) && (for_rename || kind != SyntaxKind::DefaultKeyword) {
                let source = self.state.binder.source_of_node(parent);
                return node_util::modifiers_of(source, parent).is_some_and(|modifiers| {
                    self.state.binder.node_array(modifiers).nodes.contains(&id)
                });
            }
        }
        let parent_kind = self.node_kind(parent);
        match kind {
            SyntaxKind::ClassKeyword => parent_kind == SyntaxKind::ClassDeclaration,
            SyntaxKind::FunctionKeyword => parent_kind == SyntaxKind::FunctionDeclaration,
            SyntaxKind::InterfaceKeyword => parent_kind == SyntaxKind::InterfaceDeclaration,
            SyntaxKind::EnumKeyword => parent_kind == SyntaxKind::EnumDeclaration,
            SyntaxKind::TypeKeyword => parent_kind == SyntaxKind::TypeAliasDeclaration,
            SyntaxKind::NamespaceKeyword | SyntaxKind::ModuleKeyword => {
                parent_kind == SyntaxKind::ModuleDeclaration
            }
            SyntaxKind::ImportKeyword => parent_kind == SyntaxKind::ImportEqualsDeclaration,
            SyntaxKind::GetKeyword => parent_kind == SyntaxKind::GetAccessor,
            SyntaxKind::SetKeyword => parent_kind == SyntaxKind::SetAccessor,
            _ => false,
        }
    }

    /// tsgo `getAdjustedLocationForDeclaration`.
    fn adjusted_location_for_declaration(
        &self,
        node: NodeId,
        for_rename: bool,
    ) -> Option<Touching> {
        if let Some(name) = self.name_of(node) {
            return Some(Touching::Node(name));
        }
        if for_rename {
            return None;
        }
        let source = self.state.binder.source_of_node(node);
        match self.node_kind(node) {
            SyntaxKind::ClassDeclaration | SyntaxKind::FunctionDeclaration => {
                // An unnamed declaration's `default` modifier (tsgo finds the
                // modifier when the declaration itself is one, which it never
                // is: no location).
                let _ = node_util::has_syntactic_modifier(source, node, ModifierFlags::DEFAULT);
                None
            }
            SyntaxKind::ClassExpression => self.child_token_of_kind(node, SyntaxKind::ClassKeyword),
            SyntaxKind::FunctionExpression => {
                self.child_token_of_kind(node, SyntaxKind::FunctionKeyword)
            }
            SyntaxKind::Constructor => Some(Touching::Node(node)),
            _ => None,
        }
    }

    /// tsgo `astnav.FindChildOfKind` for a keyword the tree does not keep:
    /// the node's first token with the keyword's text.
    fn child_token_of_kind(&self, node: NodeId, kind: SyntaxKind) -> Option<Touching> {
        let source = self.state.binder.source_of_node(node);
        let record = self.state.binder.node_record(node);
        let keyword = match kind {
            SyntaxKind::ClassKeyword => "class",
            SyntaxKind::FunctionKeyword => "function",
            _ => return None,
        };
        let mut position = record.pos as usize;
        while position < record.end as usize {
            let (start, end) = node_util::get_span_of_token_at_position(source, position);
            if end <= position || start >= record.end as usize {
                return None;
            }
            if &source.text()[start..end] == keyword {
                return Some(Touching::Token {
                    kind,
                    pos: position as u32,
                    parent: node,
                });
            }
            position = end;
        }
        None
    }

    /// tsgo `getAdjustedLocationForImportDeclaration`.
    fn adjusted_location_for_import_declaration(
        &self,
        import: NodeId,
        for_rename: bool,
    ) -> Option<Touching> {
        let NodeData::ImportDeclaration(data) = &self.state.binder.node_record(import).data else {
            return None;
        };
        if let Some(clause) = data.import_clause {
            if let NodeData::ImportClause(clause) = &self.state.binder.node_record(clause).data {
                if let Some(name) = clause.name {
                    if clause.named_bindings.is_some() {
                        // Not with both a name and named bindings.
                        return None;
                    }
                    return Some(Touching::Node(name));
                }
                if let Some(bindings) = clause.named_bindings {
                    match &self.state.binder.node_record(bindings).data {
                        NodeData::NamedImports(named) => {
                            let elements = named
                                .elements
                                .map(|elements| self.state.binder.node_array(elements).nodes)
                                .unwrap_or_default();
                            let [element] = elements else {
                                return None;
                            };
                            return self.name_of(*element).map(Touching::Node);
                        }
                        NodeData::NamespaceImport(namespace) => {
                            return namespace.name.map(Touching::Node);
                        }
                        _ => {}
                    }
                }
            }
        }
        if !for_rename {
            return data.module_specifier.map(Touching::Node);
        }
        None
    }

    /// tsgo `getAdjustedLocationForExportDeclaration`.
    fn adjusted_location_for_export_declaration(
        &self,
        export: NodeId,
        for_rename: bool,
    ) -> Option<Touching> {
        let NodeData::ExportDeclaration(data) = &self.state.binder.node_record(export).data else {
            return None;
        };
        if let Some(clause) = data.export_clause {
            match &self.state.binder.node_record(clause).data {
                NodeData::NamedExports(named) => {
                    let elements = named
                        .elements
                        .map(|elements| self.state.binder.node_array(elements).nodes)
                        .unwrap_or_default();
                    let [element] = elements else {
                        return None;
                    };
                    return self.name_of(*element).map(Touching::Node);
                }
                NodeData::NamespaceExport(namespace) => return namespace.name.map(Touching::Node),
                _ => {}
            }
        }
        if !for_rename {
            return data.module_specifier.map(Touching::Node);
        }
        None
    }

    /// tsgo `ast.GetHeritageClauseElementName`: the name an implemented or
    /// extended type names.
    fn heritage_clause_element_name(&self, element: NodeId) -> Option<NodeId> {
        let NodeData::ExpressionWithTypeArguments(data) =
            &self.state.binder.node_record(element).data
        else {
            return None;
        };
        let expression = data.expression?;
        match &self.state.binder.node_record(expression).data {
            NodeData::Identifier(_) => Some(expression),
            NodeData::PropertyAccessExpression(access) => access.name,
            _ => None,
        }
    }

    /// The name of a type reference.
    fn type_reference_name(&self, node: NodeId) -> Option<NodeId> {
        match &self.state.binder.node_record(node).data {
            NodeData::TypeReference(reference) => reference.type_name,
            _ => None,
        }
    }

    /// tsgo `ast.SkipOuterExpressions(node, OEKAll)`.
    fn skip_outer_expressions(&self, mut node: NodeId) -> NodeId {
        loop {
            let next = match &self.state.binder.node_record(node).data {
                NodeData::ParenthesizedExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                NodeData::TypeAssertionExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                NodeData::ExpressionWithTypeArguments(data) => data.expression,
                NodeData::PartiallyEmittedExpression(data) => data.expression,
                _ => None,
            };
            match next {
                Some(next) => node = next,
                None => return node,
            }
        }
    }

    /// tsgo `isInRightSideOfInternalImportEqualsDeclaration`.
    fn is_in_right_side_of_internal_import_equals_declaration(&self, mut node: NodeId) -> bool {
        loop {
            let Some(parent) = self.state.binder.node_record(node).parent else {
                return false;
            };
            if self.node_kind(parent) == SyntaxKind::QualifiedName {
                node = parent;
                continue;
            }
            return match &self.state.binder.node_record(parent).data {
                NodeData::ImportEqualsDeclaration(import) => {
                    import.module_reference == Some(node)
                        && self.node_kind(node) != SyntaxKind::ExternalModuleReference
                }
                _ => false,
            };
        }
    }

    /// tsgo `ast.IsDeclarationName`. tsgo's JSDoc tags are no declarations
    /// (their reparsed clones are), so a tag's name is no declaration name.
    pub(crate) fn is_declaration_name(&self, node: NodeId) -> bool {
        let Some(parent) = self.state.binder.node_record(node).parent else {
            return false;
        };
        let source = self.state.binder.source_of_node(node);
        self.node_kind(node) != SyntaxKind::SourceFile
            && !node_util::is_binding_pattern(source, node)
            && node_util::is_declaration(source, parent)
            && !matches!(
                self.node_kind(parent),
                SyntaxKind::JSDocTypedefTag
                    | SyntaxKind::JSDocCallbackTag
                    | SyntaxKind::JSDocPropertyTag
            )
            && self.name_of(parent) == Some(node)
    }

    /// tsgo `ast.IsJSDocNameReferenceContext`.
    fn is_js_doc_name_reference_context(&self, node: NodeId) -> bool {
        if !NodeFlags::from_bits(self.state.binder.node_record(node).flags)
            .contains(NodeFlags::JS_DOC)
        {
            return false;
        }
        let mut current = Some(node);
        while let Some(id) = current {
            if matches!(
                self.node_kind(id),
                SyntaxKind::JSDocNameReference
                    | SyntaxKind::JSDocLink
                    | SyntaxKind::JSDocLinkCode
                    | SyntaxKind::JSDocLinkPlain
            ) {
                return true;
            }
            current = self.state.binder.node_record(id).parent;
        }
        false
    }

    /// tsgo `isTypeReference`.
    fn is_type_reference(&self, mut node: NodeId) -> bool {
        if self.is_right_side_of_qualified_name_or_property_access(node) {
            node = self
                .state
                .binder
                .node_record(node)
                .parent
                .expect("a parent");
        }
        match self.node_kind(node) {
            SyntaxKind::ThisKeyword => return !self.state.is_expression_node_like_tsgo(node),
            SyntaxKind::ThisType => return true,
            _ => {}
        }
        let Some(parent) = self.state.binder.node_record(node).parent else {
            return false;
        };
        match &self.state.binder.node_record(parent).data {
            NodeData::TypeReference(_) => true,
            NodeData::ImportType(import) => !import.is_type_of,
            NodeData::ExpressionWithTypeArguments(_) => self.state.is_part_of_type_node(parent),
            _ => false,
        }
    }

    /// tsgo `ast.IsRightSideOfQualifiedNameOrPropertyAccess`.
    fn is_right_side_of_qualified_name_or_property_access(&self, node: NodeId) -> bool {
        let Some(parent) = self.state.binder.node_record(node).parent else {
            return false;
        };
        match &self.state.binder.node_record(parent).data {
            NodeData::QualifiedName(name) => name.right == Some(node),
            NodeData::PropertyAccessExpression(access) => access.name == Some(node),
            NodeData::MetaProperty(meta) => meta.name == Some(node),
            _ => false,
        }
    }

    /// tsgo `isNamespaceReference`.
    fn is_namespace_reference(&self, node: NodeId) -> bool {
        self.is_qualified_name_namespace_reference(node)
            || self.is_property_access_namespace_reference(node)
    }

    fn is_qualified_name_namespace_reference(&self, node: NodeId) -> bool {
        let mut root = node;
        let mut is_last_clause = true;
        let parent_of = |id: NodeId| self.state.binder.node_record(id).parent;
        if parent_of(root).is_some_and(|parent| self.node_kind(parent) == SyntaxKind::QualifiedName)
        {
            while let Some(parent) = parent_of(root)
                .filter(|&parent| self.node_kind(parent) == SyntaxKind::QualifiedName)
            {
                root = parent;
            }
            is_last_clause = matches!(
                &self.state.binder.node_record(root).data,
                NodeData::QualifiedName(name) if name.right == Some(node)
            );
        }
        parent_of(root).is_some_and(|parent| self.node_kind(parent) == SyntaxKind::TypeReference)
            && !is_last_clause
    }

    fn is_property_access_namespace_reference(&self, node: NodeId) -> bool {
        let mut root = node;
        let mut is_last_clause = true;
        let parent_of = |id: NodeId| self.state.binder.node_record(id).parent;
        if parent_of(root)
            .is_some_and(|parent| self.node_kind(parent) == SyntaxKind::PropertyAccessExpression)
        {
            while let Some(parent) = parent_of(root)
                .filter(|&parent| self.node_kind(parent) == SyntaxKind::PropertyAccessExpression)
            {
                root = parent;
            }
            is_last_clause = self.name_of(root) == Some(node);
        }
        if is_last_clause {
            return false;
        }
        let Some(element) = parent_of(root)
            .filter(|&parent| self.node_kind(parent) == SyntaxKind::ExpressionWithTypeArguments)
        else {
            return false;
        };
        let Some(clause) = parent_of(element) else {
            return false;
        };
        let NodeData::HeritageClause(heritage) = &self.state.binder.node_record(clause).data else {
            return false;
        };
        let Some(declaration) = parent_of(clause) else {
            return false;
        };
        self.node_kind(declaration) == SyntaxKind::ClassDeclaration
            && heritage.token == SyntaxKind::ImplementsKeyword
            || self.node_kind(declaration) == SyntaxKind::InterfaceDeclaration
                && heritage.token == SyntaxKind::ExtendsKeyword
    }
}

/// tsgo `ast.IsModifier`.
fn is_modifier_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::AbstractKeyword
            | SyntaxKind::AccessorKeyword
            | SyntaxKind::AsyncKeyword
            | SyntaxKind::ConstKeyword
            | SyntaxKind::DeclareKeyword
            | SyntaxKind::DefaultKeyword
            | SyntaxKind::ExportKeyword
            | SyntaxKind::InKeyword
            | SyntaxKind::PublicKeyword
            | SyntaxKind::PrivateKeyword
            | SyntaxKind::ProtectedKeyword
            | SyntaxKind::ReadonlyKeyword
            | SyntaxKind::StaticKeyword
            | SyntaxKind::OutKeyword
            | SyntaxKind::OverrideKeyword
    )
}
