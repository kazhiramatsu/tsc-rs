//! tsgo's hosted JSDoc reparse (TypeScript 7.1 parser/reparser.go
//! reparseHosted, 346-613) as the side table of a JavaScript file.
//!
//! tsgo's parser reparses the tags of a node's last JSDoc comment as the node
//! finishes (jsdoc.go withJSDoc → reparseTags, JavaScript files only), so a
//! parameter's own `/** @type */` is applied before its function's `@param`
//! and a tag sees what earlier tags of the same comment already applied. The
//! walk below visits JSDoc hosts in that order (children first, in source
//! order) and records each decision in `tsc_syntax::JsDocHosted`.

use tsc_syntax::{
    for_each_child, HostedCast, HostedTypeParameters, JsDocHosted, NodeData, NodeId, SourceFile,
    SyntaxKind,
};
use tsc_types::NodeFlags;

use crate::assignment::{
    get_assignment_declaration_kind, get_right_most_assigned_expression, AssignmentDeclarationKind,
};
use crate::node_util::{is_function_like_kind, kind_of, node_flags};

/// The hosted JSDoc reparse of `source`, computed on first use. A TypeScript
/// file has none: tsgo reparses JSDoc only in JavaScript files.
pub fn jsdoc_hosted(source: &SourceFile) -> &JsDocHosted {
    source.jsdoc_hosted.get_or_init(|| compute(source))
}

/// The function or class whose reparsed type parameters include a
/// `@template` tag's: tsgo has those type parameters only there (a typedef or
/// callback in the same comment owns them instead).
pub fn template_tag_host(source: &SourceFile, tag: NodeId) -> Option<NodeId> {
    let host = source
        .arena
        .node(tag)
        .parent
        .and_then(|document| source.arena.node(document).parent)?;
    let hosted = jsdoc_hosted(source);
    [function_like_host(source, host), Some(host)]
        .into_iter()
        .flatten()
        .find(|&candidate| {
            hosted
                .type_parameters_of(candidate)
                .is_some_and(|list| list.tags.contains(&tag))
        })
}

/// The `@overload` signature whose type parameters a `@template` tag's are
/// when the comment hosts them on nothing else: tsgo gives a reparsed overload
/// every `@template` of its comment (reparser.go:162-164).
pub fn overload_template_signature(source: &SourceFile, tag: NodeId) -> Option<NodeId> {
    let document = source.arena.node(tag).parent?;
    let NodeData::JSDoc(data) = &source.arena.node(document).data else {
        return None;
    };
    let tags = data
        .tags
        .map_or(&[][..], |tags| source.arena.node_array(tags).nodes);
    tags.iter()
        .find_map(|&candidate| match &source.arena.node(candidate).data {
            NodeData::JSDocOverloadTag(overload)
                if reparsed_overload_host(source, candidate).is_some() =>
            {
                overload.type_expression
            }
            _ => None,
        })
}

/// The function, method or constructor an `@overload` tag is reparsed into an
/// overload of (reparser.go:138-142): its JSDoc's host, outside object
/// literals.
pub fn reparsed_overload_host(source: &SourceFile, tag: NodeId) -> Option<NodeId> {
    let host = source
        .arena
        .node(tag)
        .parent
        .and_then(|document| source.arena.node(document).parent)?;
    if !matches!(
        kind_of(source, host),
        SyntaxKind::FunctionDeclaration | SyntaxKind::MethodDeclaration | SyntaxKind::Constructor
    ) {
        return None;
    }
    let mut ancestor = source.arena.node(host).parent;
    while let Some(node) = ancestor {
        if kind_of(source, node) == SyntaxKind::ObjectLiteralExpression {
            return None;
        }
        ancestor = source.arena.node(node).parent;
    }
    Some(host)
}

/// reparser.go getFunctionLikeHost (657-676): the function a hosted tag on
/// `host` annotates.
pub fn function_like_host(source: &SourceFile, host: NodeId) -> Option<NodeId> {
    let data = |node: NodeId| &source.arena.node(node).data;
    let expression = |node: NodeId| match data(node) {
        NodeData::ExpressionStatement(data) => data.expression,
        NodeData::ReturnStatement(data) => data.expression,
        NodeData::ExportAssignment(data) => data.expression,
        NodeData::SatisfiesExpression(data) => data.expression,
        _ => None,
    };
    let initializer = |node: NodeId| match data(node) {
        NodeData::VariableDeclaration(data) => data.initializer,
        NodeData::PropertyDeclaration(data) => data.initializer,
        NodeData::PropertyAssignment(data) => data.initializer,
        _ => None,
    };
    let mut fun = match kind_of(source, host) {
        SyntaxKind::VariableStatement => {
            let first = match data(host) {
                NodeData::VariableStatement(statement) => statement
                    .declaration_list
                    .and_then(|list| match data(list) {
                        NodeData::VariableDeclarationList(list) => list.declarations,
                        _ => None,
                    })
                    .and_then(|declarations| {
                        source.arena.node_array(declarations).nodes.first().copied()
                    }),
                _ => None,
            };
            match first {
                Some(declaration) => initializer(declaration),
                None => Some(host),
            }
        }
        SyntaxKind::PropertyAssignment | SyntaxKind::PropertyDeclaration => initializer(host),
        SyntaxKind::ExportAssignment | SyntaxKind::ReturnStatement => expression(host),
        SyntaxKind::ExpressionStatement => expression(host)
            .map(|expression| get_right_most_assigned_expression(source, expression)),
        _ => Some(host),
    }?;
    while kind_of(source, fun) == SyntaxKind::SatisfiesExpression {
        fun = expression(fun)?;
    }
    is_function_like_kind(kind_of(source, fun)).then_some(fun)
}

fn compute(source: &SourceFile) -> JsDocHosted {
    let mut hosted = JsDocHosted::default();
    if !node_flags(source, source.root).intersects(NodeFlags::JAVA_SCRIPT_FILE)
        || !source
            .arena
            .nodes()
            .iter()
            .any(|node| node.js_doc.is_some())
    {
        return hosted;
    }
    let mut reparser = Reparser {
        source,
        hosted: &mut hosted,
    };
    // Post-order: a node's JSDoc is reparsed when the node finishes, after its
    // children. `object_literals` counts the enclosing object literals, whose
    // member list sets tsgo's PCObjectLiteralMembers parsing context for
    // everything parsed inside it.
    let mut object_literals = 0u32;
    let mut stack = vec![(source.root, false)];
    let mut children = Vec::new();
    while let Some((node, finished)) = stack.pop() {
        let is_object_literal = kind_of(source, node) == SyntaxKind::ObjectLiteralExpression;
        if finished {
            if is_object_literal {
                object_literals -= 1;
            }
            if source.arena.node(node).js_doc.is_some() {
                reparser.reparse_tags(node, object_literals > 0);
            }
            continue;
        }
        if is_object_literal {
            object_literals += 1;
        }
        stack.push((node, true));
        children.clear();
        for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        stack.extend(children.iter().rev().map(|&child| (child, false)));
    }
    hosted
}

struct Reparser<'a, 'h> {
    source: &'a SourceFile,
    hosted: &'h mut JsDocHosted,
}

impl Reparser<'_, '_> {
    fn kind(&self, node: NodeId) -> SyntaxKind {
        kind_of(self.source, node)
    }

    fn data(&self, node: NodeId) -> &NodeData {
        &self.source.arena.node(node).data
    }

    fn nodes(&self, array: Option<tsc_syntax::NodeArrayId>) -> &[NodeId] {
        array.map_or(&[], |array| self.source.arena.node_array(array).nodes)
    }

    /// reparseTags: the hosted tags come from the last JSDoc comment only.
    fn reparse_tags(&mut self, host: NodeId, in_object_literal: bool) {
        let Some(js_doc) = self.source.arena.node(host).js_doc else {
            return;
        };
        let Some(&last) = self.source.arena.node_array(js_doc).nodes.last() else {
            return;
        };
        let NodeData::JSDoc(data) = self.data(last) else {
            return;
        };
        let tags = self.nodes(data.tags).to_vec();
        for tag in tags {
            self.reparse_hosted(tag, host, last, in_object_literal);
        }
    }

    /// The type inside a tag's JSDoc type expression (`tag.TypeExpression().Type()`).
    fn tag_type(&self, tag: NodeId) -> Option<NodeId> {
        let expression = match self.data(tag) {
            NodeData::JSDocTypeTag(data) => data.type_expression,
            NodeData::JSDocSatisfiesTag(data) => data.type_expression,
            NodeData::JSDocParameterTag(data) => data.type_expression,
            NodeData::JSDocReturnTag(data) => data.type_expression,
            NodeData::JSDocThisTag(data) => data.type_expression,
            _ => None,
        }?;
        match self.data(expression) {
            NodeData::JSDocTypeExpression(data) => data.r#type,
            _ => None,
        }
    }

    /// The written `Type()` of a declaration (a function-like's return type).
    fn written_type(&self, node: NodeId) -> Option<NodeId> {
        match self.data(node) {
            NodeData::VariableDeclaration(data) => data.r#type,
            NodeData::Parameter(data) => data.r#type,
            NodeData::PropertyDeclaration(data) => data.r#type,
            NodeData::FunctionDeclaration(data) => data.r#type,
            NodeData::MethodDeclaration(data) => data.r#type,
            NodeData::Constructor(data) => data.r#type,
            NodeData::GetAccessor(data) => data.r#type,
            NodeData::SetAccessor(data) => data.r#type,
            NodeData::FunctionExpression(data) => data.r#type,
            NodeData::ArrowFunction(data) => data.r#type,
            _ => None,
        }
    }

    /// `node.Type()` after the reparse so far.
    fn has_type(&self, node: NodeId) -> bool {
        self.written_type(node).is_some() || self.hosted.types.contains_key(&node)
    }

    fn written_type_parameters(&self, node: NodeId) -> Option<tsc_syntax::NodeArrayId> {
        match self.data(node) {
            NodeData::FunctionDeclaration(data) => data.type_parameters,
            NodeData::MethodDeclaration(data) => data.type_parameters,
            NodeData::Constructor(data) => data.type_parameters,
            NodeData::GetAccessor(data) => data.type_parameters,
            NodeData::SetAccessor(data) => data.type_parameters,
            NodeData::FunctionExpression(data) => data.type_parameters,
            NodeData::ArrowFunction(data) => data.type_parameters,
            NodeData::ClassDeclaration(data) => data.type_parameters,
            NodeData::ClassExpression(data) => data.type_parameters,
            _ => None,
        }
    }

    /// `TypeParameterList() != nil` after the reparse so far.
    fn has_type_parameters(&self, node: NodeId) -> bool {
        self.written_type_parameters(node).is_some()
            || self.hosted.type_parameters.contains_key(&node)
    }

    fn written_parameters(&self, node: NodeId) -> &[NodeId] {
        let parameters = match self.data(node) {
            NodeData::FunctionDeclaration(data) => data.parameters,
            NodeData::MethodDeclaration(data) => data.parameters,
            NodeData::Constructor(data) => data.parameters,
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            NodeData::FunctionExpression(data) => data.parameters,
            NodeData::ArrowFunction(data) => data.parameters,
            _ => None,
        };
        self.nodes(parameters)
    }

    fn parameter_name(&self, parameter: NodeId) -> Option<NodeId> {
        match self.data(parameter) {
            NodeData::Parameter(data) => data.name,
            _ => None,
        }
    }

    fn identifier_text(&self, node: NodeId) -> Option<&str> {
        match self.data(node) {
            NodeData::Identifier(data) => Some(data.text()),
            _ => None,
        }
    }

    /// ast.IsThisParameter's name test on a written first parameter.
    fn is_this_parameter(&self, parameter: NodeId) -> bool {
        self.parameter_name(parameter).is_some_and(|name| {
            self.kind(name) == SyntaxKind::ThisKeyword || self.identifier_text(name) == Some("this")
        })
    }

    fn expression(&self, node: NodeId) -> Option<NodeId> {
        match self.data(node) {
            NodeData::ExpressionStatement(data) => data.expression,
            NodeData::ReturnStatement(data) => data.expression,
            NodeData::ParenthesizedExpression(data) => data.expression,
            NodeData::ExportAssignment(data) => data.expression,
            NodeData::SatisfiesExpression(data) => data.expression,
            _ => None,
        }
    }

    fn initializer(&self, node: NodeId) -> Option<NodeId> {
        match self.data(node) {
            NodeData::VariableDeclaration(data) => data.initializer,
            NodeData::PropertyDeclaration(data) => data.initializer,
            NodeData::PropertyAssignment(data) => data.initializer,
            _ => None,
        }
    }

    fn declarations(&self, statement: NodeId) -> &[NodeId] {
        let NodeData::VariableStatement(data) = self.data(statement) else {
            return &[];
        };
        let Some(list) = data.declaration_list else {
            return &[];
        };
        match self.data(list) {
            NodeData::VariableDeclarationList(list) => self.nodes(list.declarations),
            _ => &[],
        }
    }

    /// The assignment-declaration binary expression of an expression
    /// statement (`GetAssignmentDeclarationKind(bin) != None`).
    fn assignment_declaration(&self, statement: NodeId) -> Option<NodeId> {
        let expression = self.expression(statement)?;
        (self.kind(expression) == SyntaxKind::BinaryExpression
            && get_assignment_declaration_kind(self.source, expression)
                != AssignmentDeclarationKind::None)
            .then_some(expression)
    }

    fn function_like_host(&self, host: NodeId) -> Option<NodeId> {
        function_like_host(self.source, host)
    }

    fn reparse_hosted(&mut self, tag: NodeId, parent: NodeId, js_doc: NodeId, in_object: bool) {
        match self.kind(tag) {
            SyntaxKind::JSDocTypeTag => self.reparse_type_tag(tag, parent),
            SyntaxKind::JSDocSatisfiesTag => self.reparse_satisfies_tag(tag, parent),
            SyntaxKind::JSDocTemplateTag => {
                if let Some(fun) = self.function_like_host(parent) {
                    if !self.has_type_parameters(fun)
                        && !self.hosted.full_signatures.contains_key(&fun)
                    {
                        if let Some(list) = self.gather_type_parameters(js_doc) {
                            self.hosted.type_parameters.insert(fun, list);
                        }
                    }
                } else if matches!(
                    self.kind(parent),
                    SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                ) && !self.has_type_parameters(parent)
                {
                    if let Some(list) = self.gather_type_parameters(js_doc) {
                        self.hosted.type_parameters.insert(parent, list);
                    }
                }
            }
            SyntaxKind::JSDocParameterTag => {
                let Some(fun) = self.function_like_host(parent) else {
                    return;
                };
                if self.hosted.full_signatures.contains_key(&fun) {
                    return;
                }
                let Some(parameter) = self.find_matching_parameter(fun, tag, js_doc) else {
                    return;
                };
                self.hosted.matched_parameters.insert(tag, parameter);
                if !self.has_type(parameter) {
                    if let Some(r#type) = self.tag_type(tag) {
                        self.hosted.types.insert(parameter, r#type);
                    }
                }
                let written_question = matches!(
                    self.data(parameter),
                    NodeData::Parameter(data) if data.question_token.is_some()
                );
                if !written_question
                    && !self.hosted.question_tokens.contains_key(&parameter)
                    && self.makes_question(tag)
                {
                    self.hosted.question_tokens.insert(parameter, tag);
                }
            }
            SyntaxKind::JSDocThisTag => {
                let Some(fun) = self.function_like_host(parent) else {
                    return;
                };
                let has_this = self.hosted.this_tags.contains_key(&fun)
                    || self
                        .written_parameters(fun)
                        .first()
                        .is_some_and(|&first| self.is_this_parameter(first));
                if !has_this {
                    self.hosted.this_tags.insert(fun, tag);
                }
            }
            SyntaxKind::JSDocReturnTag => {
                let Some(fun) = self.function_like_host(parent) else {
                    return;
                };
                if self.hosted.full_signatures.contains_key(&fun) || self.has_type(fun) {
                    return;
                }
                if let Some(r#type) = self.tag_type(tag) {
                    self.hosted.types.insert(fun, r#type);
                }
            }
            SyntaxKind::JSDocReadonlyTag
            | SyntaxKind::JSDocPrivateTag
            | SyntaxKind::JSDocPublicTag
            | SyntaxKind::JSDocProtectedTag
            | SyntaxKind::JSDocOverrideTag => {
                let target = if self.kind(parent) == SyntaxKind::ExpressionStatement {
                    match self.expression(parent) {
                        Some(expression) => expression,
                        None => return,
                    }
                } else {
                    parent
                };
                let applies = match self.kind(target) {
                    // In object literals these are not class-like members
                    // (reparser.go:530-535, #4437).
                    SyntaxKind::MethodDeclaration
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor => !in_object,
                    SyntaxKind::PropertyDeclaration
                    | SyntaxKind::Constructor
                    | SyntaxKind::BinaryExpression => true,
                    _ => false,
                };
                if applies {
                    self.hosted.modifiers.entry(target).or_default().push(tag);
                }
            }
            SyntaxKind::JSDocImplementsTag => {
                if matches!(
                    self.kind(parent),
                    SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                ) {
                    self.hosted.implements.entry(parent).or_default().push(tag);
                }
            }
            SyntaxKind::JSDocAugmentsTag => self.reparse_augments_tag(tag, parent),
            _ => {}
        }
    }

    /// reparseHosted's KindJSDocTypeTag arm (348-399).
    fn reparse_type_tag(&mut self, tag: NodeId, parent: NodeId) {
        let r#type = self.tag_type(tag);
        match self.kind(parent) {
            SyntaxKind::VariableStatement => {
                if let Some(r#type) = r#type {
                    let declarations = self.declarations(parent).to_vec();
                    for declaration in declarations {
                        if !self.has_type(declaration) {
                            self.hosted.types.insert(declaration, r#type);
                            return;
                        }
                    }
                }
            }
            SyntaxKind::VariableDeclaration
            | SyntaxKind::ExportAssignment
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::GetAccessor
            | SyntaxKind::Parameter => {
                if let (false, Some(r#type)) = (self.has_type(parent), r#type) {
                    self.hosted.types.insert(parent, r#type);
                    return;
                }
            }
            SyntaxKind::ExpressionStatement => {
                if let (Some(binary), Some(r#type)) = (self.assignment_declaration(parent), r#type)
                {
                    // SetType replaces: a later `@type` wins here.
                    self.hosted.types.insert(binary, r#type);
                    return;
                }
            }
            SyntaxKind::ReturnStatement | SyntaxKind::ParenthesizedExpression => {
                if let (Some(expression), Some(r#type)) = (self.expression(parent), r#type) {
                    self.hosted.casts.entry(expression).or_insert(HostedCast {
                        type_node: r#type,
                        tag,
                        is_assertion: true,
                    });
                    return;
                }
            }
            _ => {}
        }
        let Some(fun) = self.function_like_host(parent) else {
            return;
        };
        let untyped_parameters = self
            .written_parameters(fun)
            .iter()
            .all(|&parameter| !self.has_type(parameter));
        if !self.has_type_parameters(fun) && !self.has_type(fun) && untyped_parameters {
            if let Some(r#type) = r#type {
                self.hosted.full_signatures.insert(fun, r#type);
            }
        }
    }

    /// reparseHosted's KindJSDocSatisfiesTag arm (400-456).
    fn reparse_satisfies_tag(&mut self, tag: NodeId, parent: NodeId) {
        let Some(r#type) = self.tag_type(tag) else {
            return;
        };
        let cast = HostedCast {
            type_node: r#type,
            tag,
            is_assertion: false,
        };
        let target = match self.kind(parent) {
            SyntaxKind::VariableStatement => self
                .declarations(parent)
                .iter()
                .find_map(|&declaration| self.initializer(declaration)),
            SyntaxKind::VariableDeclaration
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertyAssignment => self.initializer(parent),
            SyntaxKind::ShorthandPropertyAssignment => match self.data(parent) {
                NodeData::ShorthandPropertyAssignment(data) => data.object_assignment_initializer,
                _ => None,
            },
            SyntaxKind::ReturnStatement
            | SyntaxKind::ParenthesizedExpression
            | SyntaxKind::ExportAssignment => self.expression(parent),
            SyntaxKind::ExpressionStatement => {
                self.assignment_declaration(parent)
                    .and_then(|binary| match self.data(binary) {
                        NodeData::BinaryExpression(data) => data.right,
                        _ => None,
                    })
            }
            _ => None,
        };
        if let Some(target) = target {
            self.hosted.casts.entry(target).or_insert(cast);
        }
    }

    /// reparseHosted's KindJSDocAugmentsTag arm (593-611): a class whose single
    /// `extends` type names the same entity takes the tag's type arguments
    /// when it has none.
    fn reparse_augments_tag(&mut self, tag: NodeId, parent: NodeId) {
        let heritage = match self.data(parent) {
            NodeData::ClassDeclaration(data) => data.heritage_clauses,
            NodeData::ClassExpression(data) => data.heritage_clauses,
            _ => return,
        };
        let Some(clause) = self.nodes(heritage).iter().copied().find(|&clause| {
            matches!(self.data(clause), NodeData::HeritageClause(data)
                if data.token == SyntaxKind::ExtendsKeyword)
        }) else {
            return;
        };
        let NodeData::HeritageClause(clause) = self.data(clause) else {
            return;
        };
        let [target] = self.nodes(clause.types) else {
            return;
        };
        let target = *target;
        let NodeData::JSDocAugmentsTag(augments) = self.data(tag) else {
            return;
        };
        let Some(source) = augments.class else {
            return;
        };
        let (
            NodeData::ExpressionWithTypeArguments(target_data),
            NodeData::ExpressionWithTypeArguments(source_data),
        ) = (self.data(target), self.data(source))
        else {
            return;
        };
        let same_name = match (target_data.expression, source_data.expression) {
            (Some(left), Some(right)) => self.has_same_property_access_name(left, right),
            _ => false,
        };
        if same_name
            && target_data.type_arguments.is_none()
            && source_data.type_arguments.is_some()
            && !self.hosted.augments.contains_key(&target)
        {
            self.hosted.augments.insert(target, tag);
        }
    }

    /// ast.HasSamePropertyAccessName: identifiers with the same text, or
    /// property accesses with the same name on the same expression.
    fn has_same_property_access_name(&self, left: NodeId, right: NodeId) -> bool {
        match (self.data(left), self.data(right)) {
            (NodeData::Identifier(left), NodeData::Identifier(right)) => {
                left.text() == right.text()
            }
            (
                NodeData::PropertyAccessExpression(left),
                NodeData::PropertyAccessExpression(right),
            ) => {
                let names = match (left.name, right.name) {
                    (Some(left), Some(right)) => {
                        self.identifier_text(left).is_some()
                            && self.identifier_text(left) == self.identifier_text(right)
                    }
                    _ => false,
                };
                names
                    && match (left.expression, right.expression) {
                        (Some(left), Some(right)) => {
                            self.has_same_property_access_name(left, right)
                        }
                        _ => false,
                    }
            }
            _ => false,
        }
    }

    /// reparser.go makeQuestionIfOptional (615-623): a bracketed name or an
    /// `=`-suffixed type makes the parameter optional.
    fn makes_question(&self, tag: NodeId) -> bool {
        let NodeData::JSDocParameterTag(data) = self.data(tag) else {
            return false;
        };
        data.is_bracketed
            || self
                .tag_type(tag)
                .is_some_and(|r#type| self.kind(r#type) == SyntaxKind::JSDocOptionalType)
    }

    /// reparser.go findMatchingParameter (625-648). The parameter list is the
    /// reparsed one: a `@this` applied earlier is its first parameter.
    fn find_matching_parameter(&self, fun: NodeId, tag: NodeId, js_doc: NodeId) -> Option<NodeId> {
        let NodeData::JSDoc(document) = self.data(js_doc) else {
            return None;
        };
        let tag_index = self
            .nodes(document.tags)
            .iter()
            .filter(|&&candidate| self.kind(candidate) == SyntaxKind::JSDocParameterTag)
            .position(|&candidate| candidate == tag)?;
        let NodeData::JSDocParameterTag(tag_data) = self.data(tag) else {
            return None;
        };
        let tag_name = tag_data.name?;
        let tag_text = self.identifier_text(tag_name);
        let reparsed_this = usize::from(self.hosted.this_tags.contains_key(&fun));
        if reparsed_this == 1 {
            // The reparsed `this` parameter is an identifier: it matches by
            // name only (a tag named `this`), or by position when the tag
            // has no name.
            if tag_text == Some("this") || (tag_index == 0 && tag_text == Some("")) {
                return None;
            }
        }
        for (index, &parameter) in self.written_parameters(fun).iter().enumerate() {
            let parameter_index = index + reparsed_this;
            let Some(name) = self.parameter_name(parameter) else {
                continue;
            };
            if self.kind(name) == SyntaxKind::Identifier {
                if let Some(tag_text) = tag_text {
                    if self.identifier_text(name) == Some(tag_text)
                        || (parameter_index == tag_index && tag_text.is_empty())
                    {
                        return Some(parameter);
                    }
                }
            } else if parameter_index == tag_index {
                return Some(parameter);
            }
        }
        None
    }

    /// reparser.go gatherTypeParameters (297-344) for a function or class:
    /// none when the comment has a `@typedef` or `@callback` (the template
    /// parameters belong to it), or no template type parameter.
    fn gather_type_parameters(&self, js_doc: NodeId) -> Option<HostedTypeParameters> {
        let NodeData::JSDoc(document) = self.data(js_doc) else {
            return None;
        };
        let mut tags = Vec::new();
        let mut pos = 0;
        let mut end = 0;
        let mut any = false;
        for &tag in self.nodes(document.tags) {
            match self.data(tag) {
                NodeData::JSDocTypedefTag(_) | NodeData::JSDocCallbackTag(_) => return None,
                NodeData::JSDocTemplateTag(data) => {
                    let node = self.source.arena.node(tag);
                    if tags.is_empty() {
                        pos = node.pos;
                    }
                    end = node.end;
                    tags.push(tag);
                    any |= !self.nodes(data.type_parameters).is_empty();
                }
                _ => {}
            }
        }
        any.then_some(HostedTypeParameters { pos, end, tags })
    }
}

#[cfg(test)]
#[path = "../tests/unit/hosted/tests.rs"]
mod tests;
