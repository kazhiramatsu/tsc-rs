//! Shared JSDoc ownership, host, and effective-declaration utilities.
//!
//! These are AST-only ports of the TypeScript 6.0.3 utilities.  Checker
//! consumers must use this module instead of rescanning source comments.

use tsc_binder::{node_util, AssignmentDeclarationKind};
use tsc_syntax::{HostedCast, JsDocHosted, NodeData, NodeId, SyntaxKind};
use tsc_types::SymbolId;

use crate::state::CheckerState;

/// tsgo's hosted JSDoc reparse (TypeScript 7.1 parser/reparser.go
/// reparseHosted) as the checker reads it: the annotations tsgo writes into a
/// JavaScript declaration from its JSDoc tags. tsgo's checker sees only those
/// annotations, never the tags; `tsc_binder::jsdoc_hosted` records them.
impl<'a> CheckerState<'a> {
    pub(crate) fn hosted_jsdoc(&self, node: NodeId) -> &'a JsDocHosted {
        tsc_binder::jsdoc_hosted(self.binder.source_of_node(node))
    }

    /// The reparsed `Type()` of a JavaScript declaration (its `@type` or
    /// `@param` type) or the reparsed return type of a function-like
    /// declaration (`@return`, or `@type` on a get accessor).
    pub(crate) fn reparsed_type_node(&self, node: NodeId) -> Option<NodeId> {
        self.hosted_jsdoc(node).type_of(node)
    }

    /// tsgo `FullSignature`: the `@type` that types a whole JavaScript
    /// function.
    pub(crate) fn full_signature_node(&self, node: NodeId) -> Option<NodeId> {
        self.hosted_jsdoc(node).full_signature_of(node)
    }

    /// The reparsed type parameters of a JavaScript function or class: the
    /// type parameters of its hosted `@template` tags, in order.
    pub(crate) fn reparsed_type_parameters(&self, node: NodeId) -> Vec<NodeId> {
        let Some(list) = self.hosted_jsdoc(node).type_parameters_of(node) else {
            return Vec::new();
        };
        list.tags
            .iter()
            .flat_map(|&tag| match self.data_of(tag) {
                NodeData::JSDocTemplateTag(data) => self.nodes_of(data.type_parameters),
                _ => Vec::new(),
            })
            .collect()
    }

    /// The range of the reparsed type parameter list: the hosted `@template`
    /// tags, from the first one's start to the last one's end.
    pub(crate) fn reparsed_type_parameter_range(&self, node: NodeId) -> Option<(u32, u32)> {
        self.hosted_jsdoc(node)
            .type_parameters_of(node)
            .map(|list| (list.pos, list.end))
    }

    /// The `@param` tag whose bracketed name or `=` type gave a JavaScript
    /// parameter its reparsed question token.
    pub(crate) fn reparsed_question_tag(&self, parameter: NodeId) -> Option<NodeId> {
        self.hosted_jsdoc(parameter).question_token_of(parameter)
    }

    /// The `@this` tag that gave a JavaScript function its reparsed `this`
    /// parameter.
    pub(crate) fn reparsed_this_tag(&self, function: NodeId) -> Option<NodeId> {
        self.hosted_jsdoc(function).this_tag_of(function)
    }

    /// The modifier tags reparsed into modifiers of a JavaScript member,
    /// constructor or binary expression.
    pub(crate) fn reparsed_modifier_tags(&self, node: NodeId) -> &'a [NodeId] {
        self.hosted_jsdoc(node).modifier_tags_of(node)
    }

    /// The `as`/`satisfies` cast the reparser wraps around an expression.
    pub(crate) fn reparsed_cast(&self, expression: NodeId) -> Option<HostedCast> {
        self.hosted_jsdoc(expression).cast_of(expression)
    }

    /// tsgo `node.TypeArguments()` of a heritage `extends` element: a
    /// JavaScript class's single `extends` type without type arguments takes
    /// those of a same-named `@augments` tag of the class's last JSDoc
    /// comment (reparser.go:593-611).
    pub(crate) fn heritage_type_arguments(
        &self,
        element: NodeId,
    ) -> Option<tsc_syntax::NodeArrayId> {
        let NodeData::ExpressionWithTypeArguments(data) = self.data_of(element) else {
            return None;
        };
        data.type_arguments.or_else(|| {
            let tag = self.hosted_jsdoc(element).augments_tag_of(element)?;
            let NodeData::JSDocAugmentsTag(tag) = self.data_of(tag) else {
                return None;
            };
            match self.data_of(tag.class?) {
                NodeData::ExpressionWithTypeArguments(class) => class.type_arguments,
                _ => None,
            }
        })
    }

    /// The `@implements` tags reparsed into a JavaScript class's heritage.
    pub(crate) fn reparsed_implements_tags(&self, class: NodeId) -> &'a [NodeId] {
        self.hosted_jsdoc(class).implements_tags_of(class)
    }

    /// An `@overload` tsgo reparses into an overload declaration of its
    /// host: a function, method or constructor outside object literals
    /// (reparser.go:138-142).
    pub(crate) fn is_reparsed_overload_tag(&self, tag: NodeId) -> bool {
        self.kind_of(tag) == SyntaxKind::JSDocOverloadTag
            && tsc_binder::hosted::reparsed_overload_host(self.binder.source_of_node(tag), tag)
                .is_some()
    }

    /// The function whose reparsed return type is `type_node`, the type of
    /// a hosted `@return` (or a get accessor's `@type`): in tsgo that type's
    /// parent is the function.
    pub(crate) fn reparsed_return_type_owner(&self, type_node: NodeId) -> Option<NodeId> {
        let expression = self.parent_of(type_node)?;
        if self.kind_of(expression) != SyntaxKind::JSDocTypeExpression {
            return None;
        }
        let tag = self.parent_of(expression)?;
        if !matches!(
            self.kind_of(tag),
            SyntaxKind::JSDocReturnTag | SyntaxKind::JSDocTypeTag
        ) {
            return None;
        }
        let document = self.parent_of(tag)?;
        // A `@callback`/`@overload` signature's `@return` is its own.
        if self.kind_of(document) == SyntaxKind::JSDocSignature {
            return Some(document);
        }
        let host = self.parent_of(document)?;
        let function =
            tsc_binder::hosted::function_like_host(self.binder.source_of_node(host), host)?;
        (self.reparsed_type_node(function) == Some(type_node)).then_some(function)
    }

    /// tsgo-port: getAllJSDocTags @7.1 (checker/jsdoc.go:86-100): the tags of
    /// the last JSDoc comment of the first node on tsgo's
    /// GetNextJSDocCommentLocation walk whose last comment has tags.
    pub(crate) fn all_jsdoc_tags_tsgo(&self, node: NodeId) -> Vec<NodeId> {
        if tsc_types::NodeFlags::from_bits(self.binder.node_record(node).flags)
            .intersects(tsc_types::NodeFlags::JS_DOC)
        {
            return Vec::new();
        }
        let mut current = Some(node);
        while let Some(location) = current {
            if let Some(&last) = self.direct_jsdoc_documents(location).last() {
                if let NodeData::JSDoc(data) = self.data_of(last) {
                    if data.tags.is_some() {
                        return self.nodes_of(data.tags);
                    }
                }
            }
            current = self.next_jsdoc_comment_location_tsgo(location);
        }
        Vec::new()
    }

    /// tsgo-port: ast.GetNextJSDocCommentLocation @7.1 (ast/utilities.go:4100-4113).
    pub(crate) fn next_jsdoc_comment_location_tsgo(&self, node: NodeId) -> Option<NodeId> {
        let parent = self.parent_of(node)?;
        match self.data_of(parent) {
            NodeData::PropertyAssignment(_)
            | NodeData::ExportAssignment(_)
            | NodeData::PropertyDeclaration(_)
            | NodeData::VariableDeclaration(_)
            | NodeData::SatisfiesExpression(_)
            | NodeData::ReturnStatement(_)
            | NodeData::VariableStatement(_)
            | NodeData::ExpressionStatement(_) => Some(parent),
            NodeData::VariableDeclarationList(list) => {
                (self.nodes_of(list.declarations).first().copied() == Some(node)).then_some(parent)
            }
            _ => None,
        }
    }

    /// Project direct `node.jsDoc` property presence. getJSDocTagsWorker
    /// creates an empty array when caching tags, including an empty result;
    /// our immutable syntax arena keeps that state in jsdoc_tag_cache.
    /// tsrs-native: projects node.jsDoc presence from the immutable arena plus the
    /// getJSDocTagsWorker cache side effect
    pub(crate) fn has_jsdoc_property(&self, node: NodeId) -> bool {
        self.binder
            .source_of_node(node)
            .arena
            .node(node)
            .js_doc
            .is_some()
            || self.jsdoc_tag_cache.borrow().contains_key(&node)
    }

    /// tsc-port: hasJSDocNodes @6.0.3
    /// tsc-hash: 45123251ec15122f9da8fa85555784320976613ea79839fdaec4671cfb8e256d
    /// tsc-span: _tsc.js:12534-12538
    pub(crate) fn has_jsdoc_nodes(&self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        source
            .arena
            .node(node)
            .js_doc
            .is_some_and(|docs| !source.arena.node_array(docs).nodes.is_empty())
    }

    /// tsrs-native: owned arena projection of tsc's direct `node.jsDoc`
    /// array access.
    pub(crate) fn direct_jsdoc_documents(&self, node: NodeId) -> Vec<NodeId> {
        let source = self.binder.source_of_node(node);
        source
            .arena
            .node(node)
            .js_doc
            .map(|docs| source.arena.node_array(docs).nodes.to_vec())
            .unwrap_or_default()
    }

    fn is_jsdoc_type_or_satisfies_tag(&self, tag: NodeId) -> bool {
        matches!(
            self.kind_of(tag),
            SyntaxKind::JSDocTypeTag | SyntaxKind::JSDocSatisfiesTag
        )
    }

    /// tsc-port: ownsJSDocTag @6.0.3
    /// tsc-hash: b2fe170504879c65b19c45cf47f347e85cc00cfb334570284df207ea27b3a651
    /// tsc-span: _tsc.js:15462-15464
    fn owns_jsdoc_tag(&self, host: NodeId, tag: NodeId) -> bool {
        if !self.is_jsdoc_type_or_satisfies_tag(tag) {
            return true;
        }
        let Some(document) = self.parent_of(tag) else {
            return true;
        };
        if self.kind_of(document) != SyntaxKind::JSDoc {
            return true;
        }
        let Some(document_host) = self.parent_of(document) else {
            return true;
        };
        self.kind_of(document_host) != SyntaxKind::ParenthesizedExpression || document_host == host
    }

    /// tsc-port: filterOwnedJSDocTags @6.0.3
    /// tsc-hash: 4287e545ec38802a2766922acf834c9bd3408679eaeb047278c09528ed924c94
    /// tsc-span: _tsc.js:15451-15461
    fn owned_jsdoc_tags(&self, host: NodeId, documents: &[NodeId]) -> Vec<NodeId> {
        let Some(&last_document) = documents.last() else {
            return Vec::new();
        };
        let mut tags = Vec::new();
        for &document in documents {
            let NodeData::JSDoc(data) = self.data_of(document) else {
                continue;
            };
            for tag in self.nodes_of(data.tags) {
                if document == last_document {
                    if self.owns_jsdoc_tag(host, tag) {
                        tags.push(tag);
                    }
                } else if self.kind_of(tag) == SyntaxKind::JSDocOverloadTag {
                    tags.push(tag);
                }
            }
        }
        tags
    }

    /// tsc-port: getSingleVariableOfVariableStatement @6.0.3
    /// tsc-hash: 318a699709d8ba4125a2ebf93d36a24e35aaa5401c283d507581e58631d93052
    /// tsc-span: _tsc.js:15327-15329
    pub(crate) fn single_variable_of_variable_statement(&self, node: NodeId) -> Option<NodeId> {
        let NodeData::VariableStatement(statement) = self.data_of(node) else {
            return None;
        };
        let list = statement.declaration_list?;
        let NodeData::VariableDeclarationList(list) = self.data_of(list) else {
            return None;
        };
        self.nodes_of(list.declarations).first().copied()
    }

    fn single_initializer_of_variable_statement_or_property_declaration(
        &self,
        node: NodeId,
    ) -> Option<NodeId> {
        match self.data_of(node) {
            NodeData::VariableStatement(_) => self
                .single_variable_of_variable_statement(node)
                .and_then(|declaration| self.initializer_of(declaration)),
            NodeData::PropertyDeclaration(data) => data.initializer,
            NodeData::PropertyAssignment(data) => data.initializer,
            _ => None,
        }
    }

    fn nested_module_declaration(&self, node: NodeId) -> Option<NodeId> {
        let NodeData::ModuleDeclaration(data) = self.data_of(node) else {
            return None;
        };
        data.body
            .filter(|&body| self.kind_of(body) == SyntaxKind::ModuleDeclaration)
    }

    fn source_of_assignment(&self, node: NodeId) -> Option<NodeId> {
        let NodeData::ExpressionStatement(statement) = self.data_of(node) else {
            return None;
        };
        let expression = statement.expression?;
        if !node_util::is_assignment_expression_simple(
            self.binder.source_of_node(expression),
            expression,
        ) {
            return None;
        }
        Some(tsc_binder::assignment::get_right_most_assigned_expression(
            self.binder.source_of_node(expression),
            expression,
        ))
    }

    fn source_of_defaulted_assignment(&self, node: NodeId) -> Option<NodeId> {
        let NodeData::ExpressionStatement(statement) = self.data_of(node) else {
            return None;
        };
        let expression = statement.expression?;
        let source = self.binder.source_of_node(expression);
        if tsc_binder::get_assignment_declaration_kind(source, expression)
            == AssignmentDeclarationKind::None
        {
            return None;
        }
        let NodeData::BinaryExpression(assignment) = self.data_of(expression) else {
            return None;
        };
        let right = assignment.right?;
        let NodeData::BinaryExpression(defaulted) = self.data_of(right) else {
            return None;
        };
        let is_defaulting_operator = defaulted.operator_token.is_some_and(|operator| {
            matches!(
                self.kind_of(operator),
                SyntaxKind::BarBarToken | SyntaxKind::QuestionQuestionToken
            )
        });
        is_defaulting_operator.then_some(defaulted.right).flatten()
    }

    fn is_assignment_expression(&self, node: NodeId) -> bool {
        let NodeData::BinaryExpression(data) = self.data_of(node) else {
            return false;
        };
        data.operator_token
            .is_some_and(|operator| node_util::is_assignment_operator(self.kind_of(operator)))
    }

    /// tsc-port: getNextJSDocCommentLocation @6.0.3
    /// tsc-hash: 12679e1abba9e5aa883b2cae53161fead29e07485c4853f7e507a7fe5ff433c2
    /// tsc-span: _tsc.js:15465-15474
    fn next_jsdoc_comment_location(&self, node: NodeId) -> Option<NodeId> {
        let parent = self.parent_of(node)?;
        let parent_kind = self.kind_of(parent);
        if matches!(
            parent_kind,
            SyntaxKind::PropertyAssignment
                | SyntaxKind::ExportAssignment
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::ReturnStatement
        ) || (parent_kind == SyntaxKind::ExpressionStatement
            && self.kind_of(node) == SyntaxKind::PropertyAccessExpression)
            || self.nested_module_declaration(parent).is_some()
            || self.is_assignment_expression(node)
        {
            return Some(parent);
        }
        let grandparent = self.parent_of(parent);
        if grandparent.is_some_and(|grandparent| {
            self.single_variable_of_variable_statement(grandparent) == Some(node)
                || self.is_assignment_expression(parent)
        }) {
            return grandparent;
        }
        let great_grandparent = grandparent.and_then(|grandparent| self.parent_of(grandparent));
        if great_grandparent.is_some_and(|great_grandparent| {
            self.single_variable_of_variable_statement(great_grandparent)
                .is_some_and(|_| true)
                || self.single_initializer_of_variable_statement_or_property_declaration(
                    great_grandparent,
                ) == Some(node)
                || self
                    .source_of_defaulted_assignment(great_grandparent)
                    .is_some()
        }) {
            return great_grandparent;
        }
        None
    }

    /// tsc-port: getJSDocParameterTagsWorker @6.0.3
    /// tsc-hash: c4ae77082ed964a051e20d75c5bc3a2241efe281cb9beccbb1491403bb38c5c4
    /// tsc-span: _tsc.js:11591-11606
    pub(crate) fn get_jsdoc_parameter_tags(&self, parameter: NodeId) -> Vec<NodeId> {
        let Some(name) = self.name_of_node(parameter) else {
            return Vec::new();
        };
        let Some(parent) = self.parent_of(parameter) else {
            return Vec::new();
        };
        let tags: Vec<NodeId> = self
            .get_jsdoc_tags(parent)
            .into_iter()
            .filter(|&tag| self.kind_of(tag) == SyntaxKind::JSDocParameterTag)
            .collect();
        if self.kind_of(name) == SyntaxKind::Identifier {
            let name = self.identifier_text_of(name);
            return tags
                .into_iter()
                .filter(|&tag| {
                    let NodeData::JSDocParameterTag(data) = self.data_of(tag) else {
                        return false;
                    };
                    data.name
                        .filter(|&tag_name| self.kind_of(tag_name) == SyntaxKind::Identifier)
                        .and_then(|tag_name| self.identifier_text_of(tag_name))
                        == name
                })
                .collect();
        }
        let parameters = self.parameters_of_function(parent);
        let Some(index) = parameters
            .iter()
            .position(|&candidate| candidate == parameter)
        else {
            return Vec::new();
        };
        tags.get(index).copied().into_iter().collect()
    }

    /// tsc-port: getJSDocTypeParameterTagsWorker @6.0.3
    /// tsc-hash: a1428005e49ec1098b913f74e91e5deca686a52e3b4b22ee2106c22bee416691
    /// tsc-span: _tsc.js:11621-11624
    pub(crate) fn get_jsdoc_type_parameter_tags(&self, parameter: NodeId) -> Vec<NodeId> {
        let Some(name) = self
            .name_of_node(parameter)
            .and_then(|name| self.identifier_text_of(name))
        else {
            return Vec::new();
        };
        let Some(parent) = self.parent_of(parameter) else {
            return Vec::new();
        };
        self.get_jsdoc_tags(parent)
            .into_iter()
            .filter(|&tag| {
                let NodeData::JSDocTemplateTag(data) = self.data_of(tag) else {
                    return false;
                };
                self.nodes_of(data.type_parameters)
                    .into_iter()
                    .any(|parameter| {
                        self.name_of_node(parameter)
                            .and_then(|name| self.identifier_text_of(name))
                            == Some(name)
                    })
            })
            .collect()
    }

    /// tsc-port: getEffectiveConstraintOfTypeParameter @6.0.3
    /// tsc-hash: ad9e9b55930c500a6fef895bb77041e3fc7048fb4d538d975073c541c6b945a0
    /// tsc-span: _tsc.js:11814-11816
    pub(crate) fn effective_constraint_of_type_parameter_node(
        &self,
        node: NodeId,
    ) -> Option<NodeId> {
        let NodeData::TypeParameter(data) = self.data_of(node) else {
            return None;
        };
        data.constraint.or_else(|| {
            let parent = self.parent_of(node)?;
            let NodeData::JSDocTemplateTag(template) = self.data_of(parent) else {
                return None;
            };
            (self.nodes_of(template.type_parameters).first().copied() == Some(node))
                .then_some(template.constraint)
                .flatten()
        })
    }

    /// tsc-port: getJSDocCommentsAndTags @6.0.3
    /// tsc-hash: 06bd3326770ddb5efcb52a26bc02e694410ca4cfc4fdd1b0a5da80215773703d
    /// tsc-span: _tsc.js:15429-15450
    /// tsc-port: getJSDocTagsWorker @6.0.3
    /// tsc-hash: 57325aecd61d6df7de8277e221e63b1ecb7a7f6a3501d999255bc88fb82d81f2
    /// tsc-span: _tsc.js:11745-11759
    pub(crate) fn get_jsdoc_tags(&self, host: NodeId) -> Vec<NodeId> {
        if !self.can_have_jsdoc(host) {
            return Vec::new();
        }
        if let Some(cached) = self.jsdoc_tag_cache.borrow().get(&host).cloned() {
            return cached;
        }
        let mut result = Vec::new();
        let is_variable_like = matches!(
            self.kind_of(host),
            SyntaxKind::BindingElement
                | SyntaxKind::EnumMember
                | SyntaxKind::Parameter
                | SyntaxKind::PropertyAssignment
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::PropertySignature
                | SyntaxKind::ShorthandPropertyAssignment
                | SyntaxKind::VariableDeclaration
        );
        if is_variable_like
            && self
                .initializer_of(host)
                .is_some_and(|initializer| self.has_jsdoc_nodes(initializer))
        {
            let initializer = self.initializer_of(host).expect("tested Some");
            result.extend(self.owned_jsdoc_tags(host, &self.direct_jsdoc_documents(initializer)));
        }
        let mut current = Some(host);
        while let Some(node) = current {
            if self.has_jsdoc_nodes(node) {
                result.extend(self.owned_jsdoc_tags(host, &self.direct_jsdoc_documents(node)));
            }
            match self.kind_of(node) {
                SyntaxKind::Parameter => {
                    result.extend(self.get_jsdoc_parameter_tags(node));
                    break;
                }
                SyntaxKind::TypeParameter => {
                    result.extend(self.get_jsdoc_type_parameter_tags(node));
                    break;
                }
                _ => {}
            }
            current = self.next_jsdoc_comment_location(node);
        }
        self.jsdoc_tag_cache
            .borrow_mut()
            .insert(host, result.clone());
        result
    }

    /// tsc-port: canHaveJSDoc @6.0.3
    /// tsc-hash: c14283027bf72d74453c623f30b9e83c53a3644c737a887c245a00a14fa071ae
    /// tsc-span: _tsc.js:15356-15428
    fn can_have_jsdoc(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::ArrowFunction
                | SyntaxKind::BinaryExpression
                | SyntaxKind::Block
                | SyntaxKind::BreakStatement
                | SyntaxKind::CallSignature
                | SyntaxKind::CaseClause
                | SyntaxKind::ClassDeclaration
                | SyntaxKind::ClassExpression
                | SyntaxKind::ClassStaticBlockDeclaration
                | SyntaxKind::Constructor
                | SyntaxKind::ConstructorType
                | SyntaxKind::ConstructSignature
                | SyntaxKind::ContinueStatement
                | SyntaxKind::DebuggerStatement
                | SyntaxKind::DoStatement
                | SyntaxKind::ElementAccessExpression
                | SyntaxKind::EmptyStatement
                | SyntaxKind::EndOfFileToken
                | SyntaxKind::EnumDeclaration
                | SyntaxKind::EnumMember
                | SyntaxKind::ExportAssignment
                | SyntaxKind::ExportDeclaration
                | SyntaxKind::ExportSpecifier
                | SyntaxKind::ExpressionStatement
                | SyntaxKind::ForInStatement
                | SyntaxKind::ForOfStatement
                | SyntaxKind::ForStatement
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::FunctionType
                | SyntaxKind::GetAccessor
                | SyntaxKind::Identifier
                | SyntaxKind::IfStatement
                | SyntaxKind::ImportDeclaration
                | SyntaxKind::ImportEqualsDeclaration
                | SyntaxKind::IndexSignature
                | SyntaxKind::InterfaceDeclaration
                | SyntaxKind::JSDocFunctionType
                | SyntaxKind::JSDocSignature
                | SyntaxKind::LabeledStatement
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::MethodSignature
                | SyntaxKind::ModuleDeclaration
                | SyntaxKind::NamedTupleMember
                | SyntaxKind::NamespaceExportDeclaration
                | SyntaxKind::ObjectLiteralExpression
                | SyntaxKind::Parameter
                | SyntaxKind::ParenthesizedExpression
                | SyntaxKind::PropertyAccessExpression
                | SyntaxKind::PropertyAssignment
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::PropertySignature
                | SyntaxKind::ReturnStatement
                | SyntaxKind::SemicolonClassElement
                | SyntaxKind::SetAccessor
                | SyntaxKind::ShorthandPropertyAssignment
                | SyntaxKind::SpreadAssignment
                | SyntaxKind::SwitchStatement
                | SyntaxKind::ThrowStatement
                | SyntaxKind::TryStatement
                | SyntaxKind::TypeAliasDeclaration
                | SyntaxKind::TypeParameter
                | SyntaxKind::VariableDeclaration
                | SyntaxKind::VariableStatement
                | SyntaxKind::WhileStatement
                | SyntaxKind::WithStatement
        )
    }

    /// tsc-port: getFirstJSDocTag @6.0.3
    /// tsc-hash: 692bf3520107ebc11067c254d2e883b0e4767a5dccd1d9dbdd795d8e7dd5e679
    /// tsc-span: _tsc.js:11767-11769
    pub(crate) fn first_jsdoc_tag(&self, node: NodeId, kind: SyntaxKind) -> Option<NodeId> {
        self.get_jsdoc_tags(node)
            .into_iter()
            .find(|&tag| self.kind_of(tag) == kind)
    }

    /// tsc-port: getAllJSDocTags @6.0.3
    /// tsc-hash: 6c226e3f48d70df7f1b44a4c28faa15dc7cf9e6d1771b9613d28408109deb00a
    /// tsc-span: _tsc.js:11770-11772
    pub(crate) fn all_jsdoc_tags(&self, node: NodeId, kind: SyntaxKind) -> Vec<NodeId> {
        self.get_jsdoc_tags(node)
            .into_iter()
            .filter(|&tag| self.kind_of(tag) == kind)
            .collect()
    }

    /// tsrs-native: nullable typed-arena projection of tsc's direct
    /// `typeExpression && typeExpression.type` reads.
    pub(crate) fn jsdoc_type_expression_type(&self, expression: Option<NodeId>) -> Option<NodeId> {
        let expression = expression?;
        let NodeData::JSDocTypeExpression(data) = self.data_of(expression) else {
            return None;
        };
        data.r#type
    }

    /// The reparsed type of a JavaScript declaration that is not a function:
    /// tsgo `node.Type()` from a hosted `@type` or `@param` tag. (A
    /// function's `@type` is its FullSignature, and its reparsed `Type()` is
    /// its return type.)
    pub(crate) fn get_jsdoc_type(&self, node: NodeId) -> Option<NodeId> {
        if node_util::is_function_like_kind(self.kind_of(node)) {
            return None;
        }
        self.reparsed_type_node(node)
    }

    /// tsc-port: isJSDocTypeAlias @6.0.3
    /// tsc-hash: a1e836d7b6dc47b667df1038eb98365d3afa6c80f54e0697f49bc7004c374848
    /// tsc-span: _tsc.js:15304-15306
    pub(crate) fn is_jsdoc_type_alias(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::JSDocTypedefTag | SyntaxKind::JSDocCallbackTag
        )
    }

    /// tsc-port: getJSDocRoot @6.0.3
    /// tsc-hash: fde1a04a62f04dca02d69fdd109ba7580c4892c11a8a7dd56c2e40e7326bec00
    /// tsc-span: _tsc.js:15525-15527
    pub(crate) fn get_jsdoc_root(&self, node: NodeId) -> Option<NodeId> {
        let mut current = self.parent_of(node);
        while let Some(ancestor) = current {
            if self.kind_of(ancestor) == SyntaxKind::JSDoc {
                return Some(ancestor);
            }
            current = self.parent_of(ancestor);
        }
        None
    }

    /// tsc-port: getJSDocHost @6.0.3
    /// tsc-hash: b693b84ddd06dca629b90d76ea5060d059793320ad0b808c90a9bd575cbccbe1
    /// tsc-span: _tsc.js:15515-15524
    pub(crate) fn get_jsdoc_host(&self, node: NodeId) -> Option<NodeId> {
        let document = self.get_jsdoc_root(node)?;
        let host = self.parent_of(document)?;
        let documents = self.direct_jsdoc_documents(host);
        (documents.last().copied() == Some(document)).then_some(host)
    }

    /// tsc-port: getEffectiveJSDocHost @6.0.3
    /// tsc-hash: ca435509b2c5c1c6e598a84874ed1acb719e56d826668972fc0fd3a374dfaee3
    /// tsc-span: _tsc.js:15509-15514
    pub(crate) fn get_effective_jsdoc_host(&self, node: NodeId) -> Option<NodeId> {
        let host = self.get_jsdoc_host(node)?;
        self.source_of_defaulted_assignment(host)
            .or_else(|| self.source_of_assignment(host))
            .or_else(|| self.single_initializer_of_variable_statement_or_property_declaration(host))
            .or_else(|| self.single_variable_of_variable_statement(host))
            .or_else(|| self.nested_module_declaration(host))
            .or(Some(host))
    }

    /// tsc-port: getHostSignatureFromJSDoc @6.0.3
    /// tsc-hash: 9b5b1bbaafdcf0b45d4aa46856b689a0795ac0eda656dec2cf3159b905045bb4
    /// tsc-span: _tsc.js:15502-15508
    pub(crate) fn get_host_signature_from_jsdoc(&self, node: NodeId) -> Option<NodeId> {
        let host = self.get_effective_jsdoc_host(node)?;
        if let NodeData::PropertySignature(data) = self.data_of(host) {
            if let Some(ty) = data.r#type {
                if node_util::is_function_like_kind(self.kind_of(ty)) {
                    return Some(ty);
                }
            }
        }
        node_util::is_function_like_kind(self.kind_of(host)).then_some(host)
    }

    /// The symbol of the parameter a `@param` tag declares: a `@callback`
    /// signature's own parameter, or the function parameter tsgo's reparser
    /// matched the hosted tag to (by name, or by position for a tag without
    /// a name or a binding-pattern parameter).
    pub(crate) fn parameter_symbol_from_jsdoc(&self, tag: NodeId) -> Option<SymbolId> {
        if let Some(symbol) = self.node_symbol(tag) {
            return Some(symbol);
        }
        let parameter = self.hosted_jsdoc(tag).matched_parameter_of(tag)?;
        self.node_symbol(parameter)
    }

    /// tsc-port: isRestParameter/JSDocDeclarationTest @6.0.3
    /// tsc-hash: 24527d56f922af19f90134af44bfc873edd90fee7095f21d029e44f9ce0d3214
    /// tsc-span: _tsc.js:12593-12596
    pub(crate) fn is_rest_parameter_declaration(&self, node: NodeId) -> bool {
        match self.data_of(node) {
            NodeData::Parameter(data) => {
                data.dot_dot_dot_token.is_some()
                    || data
                        .r#type
                        .is_some_and(|ty| self.kind_of(ty) == SyntaxKind::JSDocVariadicType)
            }
            NodeData::JSDocParameterTag(data) => self
                .jsdoc_type_expression_type(data.type_expression)
                .is_some_and(|ty| self.kind_of(ty) == SyntaxKind::JSDocVariadicType),
            _ => false,
        }
    }

    /// tsgo (TypeScript 7.1): the declaration a `@template` tag's type
    /// parameters belong to — a `@typedef` or `@callback` of the same comment,
    /// else the function or class whose reparsed type parameters they are
    /// (reparser.go:297-344, 457-475).
    pub(crate) fn effective_container_for_jsdoc_template_tag(&self, tag: NodeId) -> Option<NodeId> {
        if let Some(document) = self.parent_of(tag) {
            if let NodeData::JSDoc(data) = self.data_of(document) {
                if let Some(alias) = self
                    .nodes_of(data.tags)
                    .into_iter()
                    .find(|&candidate| self.is_jsdoc_type_alias(candidate))
                {
                    return Some(alias);
                }
            }
        }
        let source = self.binder.source_of_node(tag);
        tsc_binder::hosted::template_tag_host(source, tag)
            .or_else(|| tsc_binder::hosted::overload_template_signature(source, tag))
    }
}
