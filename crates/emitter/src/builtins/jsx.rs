use std::collections::BTreeMap;

use tsc_syntax::{
    for_each_child, skip_trivia, try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor,
    NodeId, SyntaxKind,
};
use tsc_types::CompilerOptions;

use crate::{
    EmitResolver, EmitResolverNode, JavaScriptString, TransformError, TransformFlags,
    TransformNode, TransformNodeArray, TransformRoot, TransformSourceId, TransformationContext,
    Transformer,
};

/// tsc-port: transformJsx @6.0.3
/// tsc-hash: 0c30b9970bbf613a28df95fd5c016cf34879d50284ab1470a4cbd9885dfa4f19
/// tsc-span: _tsc.js:103845-104388
///
/// H2.3b owns the classic `React.createElement` path. The automatic-runtime
/// import state in the same upstream transformer remains fail-closed for
/// H2.3c at request validation.
pub(super) fn transform_jsx<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    Box::new(JsxTransformer {
        resolver,
        jsx_factory: options.jsx_factory.clone(),
        jsx_fragment_factory: options.jsx_fragment_factory.clone(),
        react_namespace: options.react_namespace.clone(),
    })
}

struct JsxTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    jsx_factory: Option<String>,
    jsx_fragment_factory: Option<String>,
    react_namespace: Option<String>,
}

impl Transformer for JsxTransformer<'_> {
    fn name(&self) -> &'static str {
        "transformJsx"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Err(TransformError::Unsupported(
                crate::UnsupportedEmitFeature::BundleRoot,
            ));
        };
        if context.arena().source(source)?.syntax().is_declaration_file {
            return Ok(TransformRoot::SourceFile(source));
        }
        let text = context.arena().source(source)?.syntax().text().to_owned();
        let pragmas = leading_jsx_pragmas(&text);
        let root = context.arena().root(source)?;
        let mut visitor = JsxVisitor::new(
            context,
            source,
            self.resolver,
            pragmas,
            self.jsx_factory.as_deref(),
            self.jsx_fragment_factory.as_deref(),
            self.react_namespace.as_deref(),
        );
        let transformed =
            visitor
                .visit(root.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "root",
                })?;
        let transformed = visitor.node(transformed);
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

#[derive(Clone, Debug, Default)]
struct JsxPragmaSettings {
    factory: Option<String>,
    fragment_factory: Option<String>,
}

/// TypeScript's JSX pragmas are collected only from leading multiline
/// comments. `@jsx` and `@jsxFrag` use the first recognized value.
fn leading_jsx_pragmas(text: &str) -> JsxPragmaSettings {
    fn collect(comment: &str, settings: &mut JsxPragmaSettings) {
        for line in comment.split(['\n', '\r', '\u{2028}', '\u{2029}']) {
            let Some(at) = line.find('@') else {
                continue;
            };
            let tail = &line[at + 1..];
            let name_end = tail.find(char::is_whitespace).unwrap_or(tail.len());
            let name = tail[..name_end].to_ascii_lowercase();
            let value = tail[name_end..].trim();
            if value.is_empty() {
                continue;
            }
            match name.as_str() {
                "jsx" if settings.factory.is_none() => {
                    settings.factory = Some(value.to_owned());
                }
                "jsxfrag" if settings.fragment_factory.is_none() => {
                    settings.fragment_factory = Some(value.to_owned());
                }
                _ => {}
            }
        }
    }

    let mut settings = JsxPragmaSettings::default();
    let mut offset = if text.starts_with("#!") {
        text.find(['\n', '\r', '\u{2028}', '\u{2029}'])
            .unwrap_or(text.len())
    } else {
        0
    };
    loop {
        while let Some(character) = text[offset..].chars().next() {
            if character.is_whitespace() || character == '\u{feff}' {
                offset += character.len_utf8();
            } else {
                break;
            }
        }
        let rest = &text[offset..];
        if let Some(comment) = rest.strip_prefix("//") {
            let end = comment
                .find(['\n', '\r', '\u{2028}', '\u{2029}'])
                .unwrap_or(comment.len());
            offset += 2 + end;
            continue;
        }
        if let Some(comment) = rest.strip_prefix("/*") {
            let end = comment.find("*/").unwrap_or(comment.len());
            collect(&comment[..end], &mut settings);
            offset += 2 + end + usize::from(end < comment.len()) * 2;
            continue;
        }
        break;
    }
    settings
}

struct JsxVisitor<'context> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    resolver: &'context dyn EmitResolver,
    factory_entity: Vec<String>,
    fragment_entity: Vec<String>,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
}

impl<'context> JsxVisitor<'context> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        resolver: &'context dyn EmitResolver,
        pragmas: JsxPragmaSettings,
        jsx_factory: Option<&str>,
        jsx_fragment_factory: Option<&str>,
        react_namespace: Option<&str>,
    ) -> Self {
        let namespace = parse_entity_name(react_namespace.unwrap_or("React"))
            .unwrap_or_else(|| vec!["React".to_owned()]);
        let mut default_factory = namespace.clone();
        default_factory.push("createElement".to_owned());
        let factory_entity = pragmas
            .factory
            .as_deref()
            .and_then(parse_entity_name)
            .or_else(|| jsx_factory.and_then(parse_entity_name))
            .unwrap_or(default_factory);

        let mut default_fragment = namespace;
        default_fragment.push("Fragment".to_owned());
        let fragment_entity = match pragmas.fragment_factory.as_deref() {
            Some(local) => parse_entity_name(local).unwrap_or(default_fragment),
            None => jsx_fragment_factory
                .and_then(parse_entity_name)
                .unwrap_or(default_fragment),
        };

        Self {
            context,
            source,
            resolver,
            factory_entity,
            fragment_entity,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
        }
    }

    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if let Some(mapped) = self.nodes.get(&id) {
            return Ok(*mapped);
        }
        let original = self
            .context
            .arena()
            .node_ref(self.source, id)
            .ok_or_else(|| TransformError::UnknownNode(self.node(id)))?;
        let record = self.context.arena().node(original)?.clone();
        let transformed = match record.data {
            NodeData::JsxElement(data) => {
                Some(self.visit_jsx_element(original, data, false)?.node())
            }
            NodeData::JsxSelfClosingElement(data) => Some(
                self.visit_jsx_self_closing_element(original, data, false)?
                    .node(),
            ),
            NodeData::JsxFragment(data) => {
                Some(self.visit_jsx_fragment(original, data, false)?.node())
            }
            NodeData::JsxExpression(data) => self.visit_jsx_expression(original, data)?,
            NodeData::JsxText(data) => self.visit_jsx_text(&data.text)?.map(TransformNode::node),
            NodeData::Token => Some(id),
            mut data => {
                try_visit_each_child(&mut data, self)?;
                Some(self.update_generic(original, data)?.node())
            }
        };
        self.nodes.insert(id, transformed);
        Ok(transformed)
    }

    fn visit_jsx_element(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::JsxElementData,
        is_child: bool,
    ) -> Result<TransformNode, TransformError> {
        let opening = data
            .opening_element
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::JsxElement,
                field: "opening_element",
            })?;
        let NodeData::JsxOpeningElement(opening) = self.context.arena().node(opening)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::JsxElement,
                field: "opening_element",
            });
        };
        self.visit_jsx_opening_like(
            original,
            opening.tag_name,
            opening.attributes,
            data.children,
            is_child,
        )
    }

    fn visit_jsx_self_closing_element(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::JsxSelfClosingElementData,
        is_child: bool,
    ) -> Result<TransformNode, TransformError> {
        self.visit_jsx_opening_like(original, data.tag_name, data.attributes, None, is_child)
    }

    fn visit_jsx_fragment(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::JsxFragmentData,
        is_child: bool,
    ) -> Result<TransformNode, TransformError> {
        let tag = self.create_entity_expression(self.fragment_entity.clone(), original)?;
        let props = self.create_token(SyntaxKind::NullKeyword)?;
        let (children, multi_line) = self.transform_jsx_children(data.children)?;
        self.create_element_call(original, tag, props, children, multi_line, is_child)
    }

    fn visit_jsx_opening_like(
        &mut self,
        original: TransformNode,
        tag_name: Option<NodeId>,
        attributes: Option<NodeId>,
        children: Option<NodeArrayId>,
        is_child: bool,
    ) -> Result<TransformNode, TransformError> {
        let tag_name = tag_name.ok_or(TransformError::RequiredChildRemoved {
            parent: self.context.arena().node(original)?.kind,
            field: "tag_name",
        })?;
        let tag = self.transform_tag_name(tag_name)?;
        let props = self.transform_attributes(attributes)?;
        let (children, multi_line) = self.transform_jsx_children(children)?;
        self.create_element_call(original, tag, props, children, multi_line, is_child)
    }

    fn transform_tag_name(&mut self, id: NodeId) -> Result<TransformNode, TransformError> {
        let node = self.node(id);
        match self.context.arena().node(node)?.data.clone() {
            NodeData::Identifier(data) if is_intrinsic_jsx_name(&data.text) => {
                self.create_string_literal(data.text.encode_utf16().collect(), false)
            }
            NodeData::JsxNamespacedName(data) => {
                let namespace =
                    self.identifier_text(data.namespace, SyntaxKind::JsxNamespacedName)?;
                let name = self.identifier_text(data.name, SyntaxKind::JsxNamespacedName)?;
                self.create_string_literal(
                    format!("{namespace}:{name}").encode_utf16().collect(),
                    false,
                )
            }
            _ => self.required_visit(id, SyntaxKind::JsxOpeningElement, "tag_name"),
        }
    }

    fn transform_attributes(
        &mut self,
        attributes: Option<NodeId>,
    ) -> Result<TransformNode, TransformError> {
        let Some(attributes) = attributes else {
            return self.create_token(SyntaxKind::NullKeyword);
        };
        let attributes_node = self.node(attributes);
        let NodeData::JsxAttributes(data) =
            self.context.arena().node(attributes_node)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::JsxOpeningElement,
                field: "attributes",
            });
        };
        let ids = self.array_nodes(data.properties)?;
        if ids.is_empty() {
            return self.create_token(SyntaxKind::NullKeyword);
        }
        let mut properties = Vec::new();
        for id in ids {
            let attribute = self.node(id);
            match self.context.arena().node(attribute)?.data.clone() {
                NodeData::JsxAttribute(data) => {
                    properties.push(self.transform_attribute(attribute, data)?);
                }
                NodeData::JsxSpreadAttribute(data) => {
                    properties.extend(self.transform_spread_attribute(attribute, data)?);
                }
                _ => {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::JsxAttributes,
                        field: "properties",
                    });
                }
            }
        }
        self.create_object_literal(properties)
    }

    fn transform_attribute(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::JsxAttributeData,
    ) -> Result<TransformNode, TransformError> {
        let name_id = data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::JsxAttribute,
            field: "name",
        })?;
        let name_node = self.node(name_id);
        let name = match self.context.arena().node(name_node)?.data.clone() {
            NodeData::Identifier(data) if is_identifier_attribute_name(&data.text) => {
                self.create_identifier(&data.text)?
            }
            NodeData::Identifier(data) => {
                self.create_string_literal(data.text.encode_utf16().collect(), false)?
            }
            NodeData::JsxNamespacedName(data) => {
                let namespace =
                    self.identifier_text(data.namespace, SyntaxKind::JsxNamespacedName)?;
                let name = self.identifier_text(data.name, SyntaxKind::JsxNamespacedName)?;
                self.create_string_literal(
                    format!("{namespace}:{name}").encode_utf16().collect(),
                    false,
                )?
            }
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::JsxAttribute,
                    field: "name",
                });
            }
        };
        let initializer = self.transform_attribute_initializer(data.initializer)?;
        let property = self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                name: Some(name.node()),
                initializer: Some(initializer.node()),
                modifiers: None,
                question_token: None,
                exclamation_token: None,
            }),
            TransformFlags::NONE,
        )?;
        self.set_original_and_range(property, original)
    }

    fn transform_attribute_initializer(
        &mut self,
        initializer: Option<NodeId>,
    ) -> Result<TransformNode, TransformError> {
        let Some(initializer) = initializer else {
            return self.create_token(SyntaxKind::TrueKeyword);
        };
        let original = self.node(initializer);
        match self.context.arena().node(original)?.data.clone() {
            NodeData::StringLiteral(data) => {
                let units = decode_entities(&data.text);
                let single_quote = self.original_string_is_single_quoted(original)?;
                let literal = self.create_string_literal(units, single_quote)?;
                self.set_original_and_range(literal, original)
            }
            NodeData::JsxExpression(data) => match self.visit_jsx_expression(original, data)? {
                Some(id) => Ok(self.node(id)),
                None => self.create_token(SyntaxKind::TrueKeyword),
            },
            NodeData::JsxElement(data) => self.visit_jsx_element(original, data, false),
            NodeData::JsxSelfClosingElement(data) => {
                self.visit_jsx_self_closing_element(original, data, false)
            }
            NodeData::JsxFragment(data) => self.visit_jsx_fragment(original, data, false),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::JsxAttribute,
                field: "initializer",
            }),
        }
    }

    fn transform_spread_attribute(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::JsxSpreadAttributeData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let expression = data
            .expression
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::JsxSpreadAttribute,
                field: "expression",
            })?;
        let expression_node = self.node(expression);
        if let NodeData::ObjectLiteralExpression(object) =
            self.context.arena().node(expression_node)?.data.clone()
        {
            if !self.object_literal_has_proto(&object)? {
                return self
                    .array_nodes(object.properties)?
                    .into_iter()
                    .map(|id| {
                        self.required_visit(id, SyntaxKind::ObjectLiteralExpression, "properties")
                    })
                    .collect();
            }
        }
        let expression =
            self.required_visit(expression, SyntaxKind::JsxSpreadAttribute, "expression")?;
        let spread = self.context.factory()?.create_node(
            self.source,
            NodeData::SpreadAssignment(tsc_syntax::nodes::SpreadAssignmentData {
                expression: Some(expression.node()),
            }),
            TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD,
        )?;
        Ok(vec![self.set_original_and_range(spread, original)?])
    }

    fn object_literal_has_proto(
        &self,
        data: &tsc_syntax::nodes::ObjectLiteralExpressionData,
    ) -> Result<bool, TransformError> {
        for id in self.array_nodes(data.properties)? {
            let NodeData::PropertyAssignment(property) =
                self.context.arena().node(self.node(id))?.data.clone()
            else {
                continue;
            };
            let Some(name) = property.name else {
                continue;
            };
            match &self.context.arena().node(self.node(name))?.data {
                NodeData::Identifier(data) if data.text == "__proto__" => return Ok(true),
                NodeData::StringLiteral(data) if data.text == "__proto__" => return Ok(true),
                _ => {}
            }
        }
        Ok(false)
    }

    fn transform_jsx_children(
        &mut self,
        children: Option<NodeArrayId>,
    ) -> Result<(Vec<TransformNode>, bool), TransformError> {
        let ids = self.array_nodes(children)?;
        let mut transformed = Vec::new();
        let mut contains_direct_jsx = false;
        for id in ids {
            let child = self.node(id);
            let record = self.context.arena().node(child)?.clone();
            let result = match record.data {
                NodeData::JsxText(data) => self.visit_jsx_text(&data.text)?,
                NodeData::JsxExpression(data) => self
                    .visit_jsx_expression(child, data)?
                    .map(|id| self.node(id)),
                NodeData::JsxElement(data) => {
                    contains_direct_jsx = true;
                    Some(self.visit_jsx_element(child, data, true)?)
                }
                NodeData::JsxSelfClosingElement(data) => {
                    contains_direct_jsx = true;
                    Some(self.visit_jsx_self_closing_element(child, data, true)?)
                }
                NodeData::JsxFragment(data) => {
                    contains_direct_jsx = true;
                    Some(self.visit_jsx_fragment(child, data, true)?)
                }
                _ => {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::JsxElement,
                        field: "children",
                    });
                }
            };
            self.nodes.insert(id, result.map(TransformNode::node));
            if let Some(result) = result {
                transformed.push(result);
            }
        }
        let multi_line = transformed.len() > 1 || contains_direct_jsx;
        Ok((transformed, multi_line))
    }

    fn visit_jsx_expression(
        &mut self,
        _original: TransformNode,
        data: tsc_syntax::nodes::JsxExpressionData,
    ) -> Result<Option<NodeId>, TransformError> {
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let expression =
            self.required_visit(expression, SyntaxKind::JsxExpression, "expression")?;
        if data.dot_dot_dot_token.is_none() {
            return Ok(Some(expression.node()));
        }
        let spread = self.context.factory()?.create_node(
            self.source,
            NodeData::SpreadElement(tsc_syntax::nodes::SpreadElementData {
                expression: Some(expression.node()),
            }),
            TransformFlags::CONTAINS_REST_OR_SPREAD,
        )?;
        Ok(Some(spread.node()))
    }

    fn visit_jsx_text(&mut self, text: &str) -> Result<Option<TransformNode>, TransformError> {
        let Some(units) = fixup_whitespace_and_decode_entities(text) else {
            return Ok(None);
        };
        self.create_string_literal(units, false).map(Some)
    }

    fn create_element_call(
        &mut self,
        original: TransformNode,
        tag: TransformNode,
        props: TransformNode,
        children: Vec<TransformNode>,
        multi_line: bool,
        is_child: bool,
    ) -> Result<TransformNode, TransformError> {
        let callee = self.create_entity_expression(self.factory_entity.clone(), original)?;
        let mut arguments = Vec::with_capacity(children.len() + 2);
        arguments.push(tag);
        arguments.push(props);
        arguments.extend(children);
        let arguments = self
            .context
            .factory()?
            .create_node_array(self.source, arguments)?;
        let call = self.context.factory()?.create_node(
            self.source,
            NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
                expression: Some(callee.node()),
                question_dot_token: None,
                type_arguments: None,
                arguments: Some(arguments.array()),
            }),
            TransformFlags::NONE,
        )?;
        if multi_line {
            self.context.factory()?.set_multi_line(call, true)?;
        }
        self.set_original_and_range(call, original)?;
        if is_child {
            self.context
                .arena_mut()?
                .metadata_mut(call)
                .set_starts_on_new_line(true);
        }
        Ok(call)
    }

    fn create_entity_expression(
        &mut self,
        parts: Vec<String>,
        reference: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let mut parts = parts.into_iter();
        let first = parts.next().unwrap_or_else(|| "React".to_owned());
        let mut expression = self.create_identifier(&first)?;
        let resolver_node = self.resolver_node(reference)?;
        if let Some(declaration) = self
            .resolver
            .get_jsx_factory_import_declaration(resolver_node, &first)?
        {
            let current_source = self.context.arena().source(self.source)?.program_source();
            if current_source == Some(declaration.source()) {
                let declaration = self
                    .context
                    .arena()
                    .node_ref(self.source, declaration.node())
                    .ok_or_else(|| TransformError::UnknownNode(self.node(declaration.node())))?;
                self.context
                    .arena_mut()?
                    .metadata_mut(expression)
                    .set_referenced_import_declaration(declaration);
            }
        }
        for part in parts {
            let name = self.create_identifier(&part)?;
            expression = self.context.factory()?.create_node(
                self.source,
                NodeData::PropertyAccessExpression(
                    tsc_syntax::nodes::PropertyAccessExpressionData {
                        name: Some(name.node()),
                        expression: Some(expression.node()),
                        question_dot_token: None,
                    },
                ),
                TransformFlags::NONE,
            )?;
        }
        Ok(expression)
    }

    fn resolver_node(&self, node: TransformNode) -> Result<EmitResolverNode, TransformError> {
        let original = self.context.arena().get_original_node(node);
        let source = self
            .context
            .arena()
            .source(original.source())?
            .program_source()
            .ok_or(TransformError::MissingProgramSource(original))?;
        Ok(EmitResolverNode::new(source, original.node()))
    }

    fn create_identifier(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: text.to_owned(),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_string_literal(
        &mut self,
        units: Vec<u16>,
        single_quote: bool,
    ) -> Result<TransformNode, TransformError> {
        let text = String::from_utf16_lossy(&units);
        let literal = self.context.factory()?.create_node(
            self.source,
            NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
                text,
                has_extended_unicode_escape: None,
            }),
            TransformFlags::NONE,
        )?;
        let metadata = self.context.arena_mut()?.metadata_mut(literal);
        metadata.set_javascript_string_value(JavaScriptString::from_code_units(units));
        metadata.set_string_literal_single_quote(single_quote);
        Ok(literal)
    }

    fn create_token(&mut self, kind: SyntaxKind) -> Result<TransformNode, TransformError> {
        self.context
            .factory()?
            .create_token(self.source, kind, TransformFlags::NONE)
    }

    fn create_object_literal(
        &mut self,
        properties: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let properties = self
            .context
            .factory()?
            .create_node_array(self.source, properties)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ObjectLiteralExpression(tsc_syntax::nodes::ObjectLiteralExpressionData {
                properties: Some(properties.array()),
            }),
            TransformFlags::NONE,
        )
    }

    fn required_visit(
        &mut self,
        id: NodeId,
        parent: SyntaxKind,
        field: &'static str,
    ) -> Result<TransformNode, TransformError> {
        self.visit(id)?
            .map(|id| self.node(id))
            .ok_or(TransformError::RequiredChildRemoved { parent, field })
    }

    fn update_generic(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<TransformNode, TransformError> {
        let mut flags =
            self.context.arena().transform_flags(original) & !TransformFlags::CONTAINS_JSX;
        let mut probe = self.context.arena().node(original)?.clone();
        probe.data = data.clone();
        let mut children = Vec::new();
        let syntax = self.context.arena().source(self.source)?.syntax();
        for_each_child(&syntax.arena, &probe, |child| {
            children.push(child);
            false
        });
        for child in children {
            if let Some(child) = self.context.arena().node_ref(self.source, child) {
                flags |= self.context.arena().propagate_child_flags(child)?;
            }
        }
        self.context.factory()?.update_node(original, data, flags)
    }

    fn set_original_and_range(
        &mut self,
        node: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.set_text_range(node, original)?;
        self.context
            .arena_mut()?
            .set_original_node(node, Some(original))?;
        Ok(node)
    }

    fn array_nodes(&self, array: Option<NodeArrayId>) -> Result<Vec<NodeId>, TransformError> {
        let Some(array) = array.and_then(|id| self.context.arena().node_array_ref(self.source, id))
        else {
            return Ok(Vec::new());
        };
        Ok(self.context.arena().node_array(array)?.nodes.clone())
    }

    fn identifier_text(
        &self,
        id: Option<NodeId>,
        parent: SyntaxKind,
    ) -> Result<String, TransformError> {
        let id = id.ok_or(TransformError::RequiredChildRemoved {
            parent,
            field: "name",
        })?;
        let NodeData::Identifier(data) = &self.context.arena().node(self.node(id))?.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent,
                field: "name",
            });
        };
        Ok(data.text.clone())
    }

    fn original_string_is_single_quoted(
        &self,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let original = self.context.arena().get_original_node(node);
        let source = self.context.arena().source(original.source())?.syntax();
        let record = self.context.arena().node(original)?;
        if record.pos == u32::MAX {
            return Ok(false);
        }
        let start = skip_trivia(source.text(), record.pos as usize);
        Ok(source.text().as_bytes().get(start) == Some(&b'\''))
    }

    const fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    const fn array(&self, id: NodeArrayId) -> TransformNodeArray {
        TransformNodeArray::new(self.source, id)
    }
}

impl NodeDataChildVisitor for JsxVisitor<'_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("JSX child belongs to its transform source")
            .kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.visit(id)
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        if let Some(mapped) = self.arrays.get(&id) {
            return Ok(*mapped);
        }
        let original = self.array(id);
        let ids = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(id) = self.visit(id)? {
                visited.push(self.node(id));
            }
        }
        let updated = self
            .context
            .factory()?
            .update_node_array(original, visited)?;
        let mapped = Some(updated.array());
        self.arrays.insert(id, mapped);
        Ok(mapped)
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}

fn parse_entity_name(value: &str) -> Option<Vec<String>> {
    fn valid_identifier(part: &str) -> bool {
        !part.is_empty()
            && part.chars().next().is_some_and(|character| {
                character == '_' || character == '$' || character.is_alphabetic()
            })
            && part.chars().all(|character| {
                character == '_' || character == '$' || character.is_alphanumeric()
            })
    }

    let parts = value
        .split('.')
        .map(str::trim)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    (!parts.is_empty() && parts.iter().all(|part| valid_identifier(part))).then_some(parts)
}

fn is_intrinsic_jsx_name(name: &str) -> bool {
    name.as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_lowercase())
        || name.contains('-')
}

fn is_identifier_attribute_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn fixup_whitespace_and_decode_entities(text: &str) -> Option<Vec<u16>> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut first_line = true;
    for (index, character) in text.char_indices() {
        if !matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            continue;
        }
        let line = &text[start..index];
        if let Some(line) = trim_jsx_line(line, first_line, true) {
            lines.push(line);
        }
        first_line = false;
        start = index + character.len_utf8();
    }
    let tail = &text[start..];
    if let Some(line) = trim_jsx_line(tail, first_line, false) {
        lines.push(line);
    }
    if lines.is_empty() {
        return None;
    }
    let mut result = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        if index != 0 {
            result.push(b' ' as u16);
        }
        result.extend(decode_entities(line));
    }
    Some(result)
}

fn trim_jsx_line(line: &str, first_line: bool, ended_by_line_break: bool) -> Option<&str> {
    if !line
        .chars()
        .any(|character| !is_single_line_whitespace(character))
    {
        return None;
    }
    let line = if first_line {
        line
    } else {
        line.trim_start_matches(is_single_line_whitespace)
    };
    Some(if ended_by_line_break {
        line.trim_end_matches(is_single_line_whitespace)
    } else {
        line
    })
}

fn is_single_line_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}' | '\u{000b}' | '\u{000c}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

fn decode_entities(text: &str) -> Vec<u16> {
    let mut result = Vec::new();
    let mut cursor = 0usize;
    while cursor < text.len() {
        let Some(relative_amp) = text[cursor..].find('&') else {
            result.extend(text[cursor..].encode_utf16());
            break;
        };
        let amp = cursor + relative_amp;
        result.extend(text[cursor..amp].encode_utf16());
        let Some(relative_semicolon) = text[amp + 1..].find(';') else {
            result.extend(text[amp..].encode_utf16());
            break;
        };
        let semicolon = amp + 1 + relative_semicolon;
        let entity = &text[amp + 1..semicolon];
        if let Some(value) = decode_entity(entity) {
            push_code_point(&mut result, value);
            cursor = semicolon + 1;
        } else {
            result.push(b'&' as u16);
            cursor = amp + 1;
        }
    }
    result
}

fn decode_entity(entity: &str) -> Option<u32> {
    if let Some(decimal) = entity.strip_prefix('#') {
        if let Some(hex) = decimal.strip_prefix('x') {
            return u32::from_str_radix(hex, 16).ok();
        }
        return decimal.parse().ok();
    }
    named_entity(entity)
}

fn push_code_point(result: &mut Vec<u16>, value: u32) {
    if value <= u16::MAX as u32 {
        result.push(value as u16);
    } else if value <= 0x10ffff {
        let value = value - 0x10000;
        result.push(0xd800 | ((value >> 10) as u16));
        result.push(0xdc00 | ((value & 0x3ff) as u16));
    }
}

// The JSX transformer uses TypeScript's HTML4 entity table. The less common
// names are kept here rather than delegated to a locale or HTML library so
// output remains pinned and deterministic.
fn named_entity(name: &str) -> Option<u32> {
    Some(match name {
        "quot" => 34,
        "amp" => 38,
        "apos" => 39,
        "lt" => 60,
        "gt" => 62,
        "nbsp" => 160,
        "iexcl" => 161,
        "cent" => 162,
        "pound" => 163,
        "curren" => 164,
        "yen" => 165,
        "brvbar" => 166,
        "sect" => 167,
        "uml" => 168,
        "copy" => 169,
        "ordf" => 170,
        "laquo" => 171,
        "not" => 172,
        "shy" => 173,
        "reg" => 174,
        "macr" => 175,
        "deg" => 176,
        "plusmn" => 177,
        "sup2" => 178,
        "sup3" => 179,
        "acute" => 180,
        "micro" => 181,
        "para" => 182,
        "middot" => 183,
        "cedil" => 184,
        "sup1" => 185,
        "ordm" => 186,
        "raquo" => 187,
        "frac14" => 188,
        "frac12" => 189,
        "frac34" => 190,
        "iquest" => 191,
        "Agrave" => 192,
        "Aacute" => 193,
        "Acirc" => 194,
        "Atilde" => 195,
        "Auml" => 196,
        "Aring" => 197,
        "AElig" => 198,
        "Ccedil" => 199,
        "Egrave" => 200,
        "Eacute" => 201,
        "Ecirc" => 202,
        "Euml" => 203,
        "Igrave" => 204,
        "Iacute" => 205,
        "Icirc" => 206,
        "Iuml" => 207,
        "ETH" => 208,
        "Ntilde" => 209,
        "Ograve" => 210,
        "Oacute" => 211,
        "Ocirc" => 212,
        "Otilde" => 213,
        "Ouml" => 214,
        "times" => 215,
        "Oslash" => 216,
        "Ugrave" => 217,
        "Uacute" => 218,
        "Ucirc" => 219,
        "Uuml" => 220,
        "Yacute" => 221,
        "THORN" => 222,
        "szlig" => 223,
        "agrave" => 224,
        "aacute" => 225,
        "acirc" => 226,
        "atilde" => 227,
        "auml" => 228,
        "aring" => 229,
        "aelig" => 230,
        "ccedil" => 231,
        "egrave" => 232,
        "eacute" => 233,
        "ecirc" => 234,
        "euml" => 235,
        "igrave" => 236,
        "iacute" => 237,
        "icirc" => 238,
        "iuml" => 239,
        "eth" => 240,
        "ntilde" => 241,
        "ograve" => 242,
        "oacute" => 243,
        "ocirc" => 244,
        "otilde" => 245,
        "ouml" => 246,
        "divide" => 247,
        "oslash" => 248,
        "ugrave" => 249,
        "uacute" => 250,
        "ucirc" => 251,
        "uuml" => 252,
        "yacute" => 253,
        "thorn" => 254,
        "yuml" => 255,
        "OElig" => 338,
        "oelig" => 339,
        "Scaron" => 352,
        "scaron" => 353,
        "Yuml" => 376,
        "fnof" => 402,
        "circ" => 710,
        "tilde" => 732,
        "Alpha" => 913,
        "Beta" => 914,
        "Gamma" => 915,
        "Delta" => 916,
        "Epsilon" => 917,
        "Zeta" => 918,
        "Eta" => 919,
        "Theta" => 920,
        "Iota" => 921,
        "Kappa" => 922,
        "Lambda" => 923,
        "Mu" => 924,
        "Nu" => 925,
        "Xi" => 926,
        "Omicron" => 927,
        "Pi" => 928,
        "Rho" => 929,
        "Sigma" => 931,
        "Tau" => 932,
        "Upsilon" => 933,
        "Phi" => 934,
        "Chi" => 935,
        "Psi" => 936,
        "Omega" => 937,
        "alpha" => 945,
        "beta" => 946,
        "gamma" => 947,
        "delta" => 948,
        "epsilon" => 949,
        "zeta" => 950,
        "eta" => 951,
        "theta" => 952,
        "iota" => 953,
        "kappa" => 954,
        "lambda" => 955,
        "mu" => 956,
        "nu" => 957,
        "xi" => 958,
        "omicron" => 959,
        "pi" => 960,
        "rho" => 961,
        "sigmaf" => 962,
        "sigma" => 963,
        "tau" => 964,
        "upsilon" => 965,
        "phi" => 966,
        "chi" => 967,
        "psi" => 968,
        "omega" => 969,
        "thetasym" => 977,
        "upsih" => 978,
        "piv" => 982,
        "ensp" => 8194,
        "emsp" => 8195,
        "thinsp" => 8201,
        "zwnj" => 8204,
        "zwj" => 8205,
        "lrm" => 8206,
        "rlm" => 8207,
        "ndash" => 8211,
        "mdash" => 8212,
        "lsquo" => 8216,
        "rsquo" => 8217,
        "sbquo" => 8218,
        "ldquo" => 8220,
        "rdquo" => 8221,
        "bdquo" => 8222,
        "dagger" => 8224,
        "Dagger" => 8225,
        "bull" => 8226,
        "hellip" => 8230,
        "permil" => 8240,
        "prime" => 8242,
        "Prime" => 8243,
        "lsaquo" => 8249,
        "rsaquo" => 8250,
        "oline" => 8254,
        "frasl" => 8260,
        "euro" => 8364,
        "image" => 8465,
        "weierp" => 8472,
        "real" => 8476,
        "trade" => 8482,
        "alefsym" => 8501,
        "larr" => 8592,
        "uarr" => 8593,
        "rarr" => 8594,
        "darr" => 8595,
        "harr" => 8596,
        "crarr" => 8629,
        "lArr" => 8656,
        "uArr" => 8657,
        "rArr" => 8658,
        "dArr" => 8659,
        "hArr" => 8660,
        "forall" => 8704,
        "part" => 8706,
        "exist" => 8707,
        "empty" => 8709,
        "nabla" => 8711,
        "isin" => 8712,
        "notin" => 8713,
        "ni" => 8715,
        "prod" => 8719,
        "sum" => 8721,
        "minus" => 8722,
        "lowast" => 8727,
        "radic" => 8730,
        "prop" => 8733,
        "infin" => 8734,
        "ang" => 8736,
        "and" => 8743,
        "or" => 8744,
        "cap" => 8745,
        "cup" => 8746,
        "int" => 8747,
        "there4" => 8756,
        "sim" => 8764,
        "cong" => 8773,
        "asymp" => 8776,
        "ne" => 8800,
        "equiv" => 8801,
        "le" => 8804,
        "ge" => 8805,
        "sub" => 8834,
        "sup" => 8835,
        "nsub" => 8836,
        "sube" => 8838,
        "supe" => 8839,
        "oplus" => 8853,
        "otimes" => 8855,
        "perp" => 8869,
        "sdot" => 8901,
        "lceil" => 8968,
        "rceil" => 8969,
        "lfloor" => 8970,
        "rfloor" => 8971,
        "lang" => 9001,
        "rang" => 9002,
        "loz" => 9674,
        "spades" => 9824,
        "clubs" => 9827,
        "hearts" => 9829,
        "diams" => 9830,
        _ => return None,
    })
}
