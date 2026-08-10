use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    for_each_child, skip_trivia, try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor,
    NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags};

use crate::{
    EmitResolver, EmitResolverNode, JavaScriptString, TransformError, TransformFlags,
    TransformNode, TransformNodeArray, TransformRoot, TransformSourceId, TransformationContext,
    Transformer,
};

/// tsc-port: transformJsx @6.0.3
/// tsc-hash: 0c30b9970bbf613a28df95fd5c016cf34879d50284ab1470a4cbd9885dfa4f19
/// tsc-span: _tsc.js:103845-104388
///
/// H2.3b owns the classic `React.createElement` path. H2.3c owns the automatic
/// `jsx`/`jsxs` and development `jsxDEV` paths plus their implicit imports.
pub(super) fn transform_jsx<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    Box::new(JsxTransformer {
        resolver,
        jsx_factory: options.jsx_factory.clone(),
        jsx_fragment_factory: options.jsx_fragment_factory.clone(),
        jsx_import_source: options.jsx_import_source.clone(),
        jsx_mode: options.jsx.unwrap_or(2),
        react_namespace: options.react_namespace.clone(),
    })
}

struct JsxTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    jsx_factory: Option<String>,
    jsx_fragment_factory: Option<String>,
    jsx_import_source: Option<String>,
    jsx_mode: i32,
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
        let syntax = context.arena().source(source)?.syntax();
        let text = syntax.text().to_owned();
        let file_name = syntax.file_name.clone();
        let source_runtime = syntax.jsx_runtime_pragma.clone();
        let source_import = syntax.jsx_import_source_pragma.clone();
        let is_external_module = syntax.external_module_indicator.is_some();
        let pragmas = leading_jsx_pragmas(&text);
        let root = context.arena().root(source)?;
        let import_base = jsx_implicit_import_base(
            self.jsx_mode,
            self.jsx_import_source.as_deref(),
            source_import.as_deref(),
            source_runtime.as_deref(),
        );
        let is_external_or_common_js_module = if import_base.is_none() || is_external_module {
            is_external_module
        } else {
            let program_source = context
                .arena()
                .source(source)?
                .program_source()
                .ok_or(TransformError::MissingProgramSource(root))?;
            self.resolver
                .is_external_or_common_js_module(EmitResolverNode::new(
                    program_source,
                    root.node(),
                ))?
        };
        let mut visitor = JsxVisitor::new(
            context,
            source,
            self.resolver,
            JsxVisitorSettings {
                pragmas,
                jsx_mode: self.jsx_mode,
                import_base,
                file_name,
                is_external_module,
                is_external_or_common_js_module,
                jsx_factory: self.jsx_factory.as_deref(),
                jsx_fragment_factory: self.jsx_fragment_factory.as_deref(),
                react_namespace: self.react_namespace.as_deref(),
            },
        );
        let transformed =
            visitor
                .visit(root.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "root",
                })?;
        let transformed = visitor.finish_source_file(visitor.node(transformed))?;
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

/// tsc-port: getJSXImplicitImportBase/getJSXRuntimeImport @6.0.3
/// tsc-hash: eb7474face65e4978dcd8aca37dba525c4a2b0027decbad5f4e87e2a5d5cc9e5
/// tsc-span: _tsc.js:18305-18316
fn jsx_implicit_import_base(
    jsx_mode: i32,
    option_import_source: Option<&str>,
    pragma_import_source: Option<&str>,
    pragma_runtime: Option<&str>,
) -> Option<String> {
    if pragma_runtime == Some("classic") {
        return None;
    }
    if !matches!(jsx_mode, 4 | 5)
        && option_import_source.is_none()
        && pragma_import_source.is_none()
        && pragma_runtime != Some("automatic")
    {
        return None;
    }
    Some(
        pragma_import_source
            .or(option_import_source)
            .unwrap_or("react")
            .to_owned(),
    )
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
    jsx_mode: i32,
    import_base: Option<String>,
    file_name: String,
    is_external_module: bool,
    is_external_or_common_js_module: bool,
    used_names: BTreeSet<String>,
    implicit_imports: Vec<ImplicitImportGroup>,
    filename_declaration: Option<TransformNode>,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
}

struct JsxVisitorSettings<'settings> {
    pragmas: JsxPragmaSettings,
    jsx_mode: i32,
    import_base: Option<String>,
    file_name: String,
    is_external_module: bool,
    is_external_or_common_js_module: bool,
    jsx_factory: Option<&'settings str>,
    jsx_fragment_factory: Option<&'settings str>,
    react_namespace: Option<&'settings str>,
}

#[derive(Clone, Copy)]
struct ElementCallFormatting {
    multi_line: bool,
    is_child: bool,
}

#[derive(Clone, Debug)]
struct ImplicitImport {
    exported_name: String,
    local_name: String,
    specifier: TransformNode,
}

#[derive(Clone, Debug)]
struct ImplicitImportGroup {
    module_specifier: String,
    imports: Vec<ImplicitImport>,
}

impl<'context> JsxVisitor<'context> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        resolver: &'context dyn EmitResolver,
        settings: JsxVisitorSettings<'_>,
    ) -> Self {
        let JsxVisitorSettings {
            pragmas,
            jsx_mode,
            import_base,
            file_name,
            is_external_module,
            is_external_or_common_js_module,
            jsx_factory,
            jsx_fragment_factory,
            react_namespace,
        } = settings;
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

        let used_names = super::system::collect_identifier_texts(context.arena(), source);
        Self {
            context,
            source,
            resolver,
            factory_entity,
            fragment_entity,
            jsx_mode,
            import_base,
            file_name,
            is_external_module,
            is_external_or_common_js_module,
            used_names,
            implicit_imports: Vec::new(),
            filename_declaration: None,
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
        if self.import_base.is_some() {
            let (children, _) = self.transform_jsx_children(data.children)?;
            let static_children = self.children_are_static(&children)?;
            let props = self.create_automatic_props(Vec::new(), children.clone())?;
            let tag = self.get_implicit_import_for_name("Fragment")?;
            return self.create_automatic_call(
                original,
                tag,
                props,
                None,
                static_children,
                is_child,
            );
        }
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
        if self.import_base.is_none() || self.has_key_after_props_spread(attributes)? {
            let props = self.transform_attributes(attributes)?;
            let (children, multi_line) = self.transform_jsx_children(children)?;
            let callee = if self.import_base.is_some() {
                self.get_implicit_import_for_name("createElement")?
            } else {
                self.create_entity_expression(self.factory_entity.clone(), original)?
            };
            return self.create_element_call_with_callee(
                original,
                callee,
                tag,
                props,
                children,
                ElementCallFormatting {
                    multi_line,
                    is_child,
                },
            );
        }

        let (children, _) = self.transform_jsx_children(children)?;
        let static_children = self.children_are_static(&children)?;
        let key = self.find_key_attribute(attributes)?;
        let properties = self.transform_attribute_properties(attributes, key)?;
        let props = self.create_automatic_props(properties, children)?;
        let key = key
            .map(|key| self.transform_key_attribute(key))
            .transpose()?;
        self.create_automatic_call(original, tag, props, key, static_children, is_child)
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
        let properties = self.transform_attribute_properties(attributes, None)?;
        if properties.is_empty() {
            return self.create_token(SyntaxKind::NullKeyword);
        }
        self.create_object_literal(properties)
    }

    fn transform_attribute_properties(
        &mut self,
        attributes: Option<NodeId>,
        excluded: Option<NodeId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let ids = self.attribute_nodes(attributes)?;
        let mut properties = Vec::new();
        for id in ids {
            if excluded == Some(id) {
                continue;
            }
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
        Ok(properties)
    }

    fn attribute_nodes(&self, attributes: Option<NodeId>) -> Result<Vec<NodeId>, TransformError> {
        let Some(attributes) = attributes else {
            return Ok(Vec::new());
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
        self.array_nodes(data.properties)
    }

    fn find_key_attribute(
        &self,
        attributes: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        for id in self.attribute_nodes(attributes)? {
            let NodeData::JsxAttribute(attribute) = &self.context.arena().node(self.node(id))?.data
            else {
                continue;
            };
            let Some(name) = attribute.name else {
                continue;
            };
            if matches!(
                &self.context.arena().node(self.node(name))?.data,
                NodeData::Identifier(data) if data.text == "key"
            ) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// tsc-port: hasKeyAfterPropsSpread @6.0.3
    /// tsc-hash: c12878f424d9b345c85312351cbdfbe5e6a5d986ff140daaad024cb83e86d3c1
    /// tsc-span: _tsc.js:104052-104062
    fn has_key_after_props_spread(
        &self,
        attributes: Option<NodeId>,
    ) -> Result<bool, TransformError> {
        let mut saw_unflattened_spread = false;
        for id in self.attribute_nodes(attributes)? {
            match &self.context.arena().node(self.node(id))?.data {
                NodeData::JsxSpreadAttribute(spread) => {
                    let flattenable = spread
                        .expression
                        .and_then(|id| self.context.arena().node_ref(self.source, id))
                        .and_then(|expression| {
                            let NodeData::ObjectLiteralExpression(object) =
                                &self.context.arena().node(expression).ok()?.data
                            else {
                                return None;
                            };
                            Some(object)
                        })
                        .is_some_and(|object| {
                            self.array_nodes(object.properties).is_ok_and(|properties| {
                                properties.into_iter().all(|property| {
                                    !matches!(
                                        self.context.arena().node(self.node(property)),
                                        Ok(record) if matches!(record.data, NodeData::SpreadAssignment(_))
                                    )
                                })
                            })
                        });
                    saw_unflattened_spread |= !flattenable;
                }
                NodeData::JsxAttribute(attribute) if saw_unflattened_spread => {
                    let is_key = attribute.name.is_some_and(|name| {
                        matches!(
                            self.context.arena().node(self.node(name)),
                            Ok(record) if matches!(&record.data, NodeData::Identifier(data) if data.text == "key")
                        )
                    });
                    if is_key {
                        return Ok(true);
                    }
                }
                _ => {}
            }
        }
        Ok(false)
    }

    fn transform_key_attribute(&mut self, id: NodeId) -> Result<TransformNode, TransformError> {
        let attribute = self.node(id);
        let NodeData::JsxAttribute(data) = self.context.arena().node(attribute)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::JsxOpeningElement,
                field: "key",
            });
        };
        self.transform_attribute_initializer(data.initializer)
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

    fn children_are_static(&self, children: &[TransformNode]) -> Result<bool, TransformError> {
        Ok(children.len() > 1
            || children.first().is_some_and(|child| {
                self.context
                    .arena()
                    .node(*child)
                    .is_ok_and(|record| record.kind == SyntaxKind::SpreadElement)
            }))
    }

    fn create_automatic_props(
        &mut self,
        mut properties: Vec<TransformNode>,
        mut children: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if !children.is_empty() {
            let initializer = if children.len() == 1
                && self.context.arena().node(children[0])?.kind != SyntaxKind::SpreadElement
            {
                children.remove(0)
            } else {
                let elements = self
                    .context
                    .factory()?
                    .create_node_array(self.source, children)?;
                self.context.factory()?.create_node(
                    self.source,
                    NodeData::ArrayLiteralExpression(
                        tsc_syntax::nodes::ArrayLiteralExpressionData {
                            elements: Some(elements.array()),
                        },
                    ),
                    TransformFlags::NONE,
                )?
            };
            properties.push(self.create_property_assignment("children", initializer)?);
        }
        self.create_object_literal(properties)
    }

    fn create_property_assignment(
        &mut self,
        name: &str,
        initializer: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                name: Some(name.node()),
                initializer: Some(initializer.node()),
                modifiers: None,
                question_token: None,
                exclamation_token: None,
            }),
            TransformFlags::NONE,
        )
    }

    /// tsc-port: visitJsxOpeningLikeElementOrFragmentJSX @6.0.3
    /// tsc-hash: cc719ed9da86526b78a2fbfecb9e1151a2aa179d94ff0afe1df43b7e6326b42b
    /// tsc-span: _tsc.js:104125-104162
    fn create_automatic_call(
        &mut self,
        original: TransformNode,
        tag: TransformNode,
        props: TransformNode,
        key: Option<TransformNode>,
        static_children: bool,
        is_child: bool,
    ) -> Result<TransformNode, TransformError> {
        let mut arguments = vec![tag, props];
        if let Some(key) = key {
            arguments.push(key);
        }
        if self.jsx_mode == 5 {
            if arguments.len() == 2 {
                arguments.push(self.create_void_zero()?);
            }
            arguments.push(self.create_token(if static_children {
                SyntaxKind::TrueKeyword
            } else {
                SyntaxKind::FalseKeyword
            })?);
            arguments.push(self.create_jsx_source_object(original)?);
            arguments.push(self.create_token(SyntaxKind::ThisKeyword)?);
        }
        let helper = if self.jsx_mode == 5 {
            "jsxDEV"
        } else if static_children {
            "jsxs"
        } else {
            "jsx"
        };
        let callee = self.get_implicit_import_for_name(helper)?;
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
        self.set_original_and_range(call, original)?;
        if is_child {
            self.context
                .arena_mut()?
                .metadata_mut(call)
                .set_starts_on_new_line(true);
        }
        Ok(call)
    }

    fn create_jsx_source_object(
        &mut self,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let record = self.context.arena().node(original)?.clone();
        let syntax = self.context.arena().source(self.source)?.syntax();
        let start = if record.pos == u32::MAX {
            0
        } else {
            skip_trivia(syntax.text(), record.pos as usize)
        };
        let utf16 = syntax
            .positions()
            .byte_to_utf16(u32::try_from(start).unwrap_or(u32::MAX))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: record.kind,
                field: "source position",
            })?;
        let location = syntax.positions().line_and_character_utf16(utf16).ok_or(
            TransformError::RequiredChildRemoved {
                parent: record.kind,
                field: "source location",
            },
        )?;
        let file_name = self.current_file_name_expression()?;
        let line = self.create_numeric_literal(location.line + 1)?;
        let column = self.create_numeric_literal(location.character + 1)?;
        let properties = vec![
            self.create_property_assignment("fileName", file_name)?,
            self.create_property_assignment("lineNumber", line)?,
            self.create_property_assignment("columnNumber", column)?,
        ];
        self.create_object_literal(properties)
    }

    fn current_file_name_expression(&mut self) -> Result<TransformNode, TransformError> {
        if let Some(declaration) = self.filename_declaration {
            let NodeData::VariableDeclaration(data) =
                self.context.arena().node(declaration)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableDeclaration,
                    field: "name",
                });
            };
            let name = self.identifier_text(data.name, SyntaxKind::VariableDeclaration)?;
            return self.create_identifier(&name);
        }
        let name = self.fresh_name("_jsxFileName");
        let declaration_name = self.create_identifier(&name)?;
        let file_name = self.file_name.clone();
        let initializer = self.create_string_literal(file_name.encode_utf16().collect(), false)?;
        let declaration = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(declaration_name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: Some(initializer.node()),
            }),
            TransformFlags::NONE,
        )?;
        self.filename_declaration = Some(declaration);
        self.create_identifier(&name)
    }

    fn create_numeric_literal(&mut self, value: u32) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                text: value.to_string(),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_void_zero(&mut self) -> Result<TransformNode, TransformError> {
        let zero = self.create_numeric_literal(0)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VoidExpression(tsc_syntax::nodes::VoidExpressionData {
                expression: Some(zero.node()),
            }),
            TransformFlags::NONE,
        )
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
        self.create_element_call_with_callee(
            original,
            callee,
            tag,
            props,
            children,
            ElementCallFormatting {
                multi_line,
                is_child,
            },
        )
    }

    fn create_element_call_with_callee(
        &mut self,
        original: TransformNode,
        callee: TransformNode,
        tag: TransformNode,
        props: TransformNode,
        children: Vec<TransformNode>,
        formatting: ElementCallFormatting,
    ) -> Result<TransformNode, TransformError> {
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
        if formatting.multi_line {
            self.context.factory()?.set_multi_line(call, true)?;
        }
        self.set_original_and_range(call, original)?;
        if formatting.is_child {
            self.context
                .arena_mut()?
                .metadata_mut(call)
                .set_starts_on_new_line(true);
        }
        Ok(call)
    }

    /// tsc-port: getImplicitImportForName/transformSourceFile @6.0.3
    /// tsc-hash: 7412f5e98f419ccb7f04ab8501f14c08136428c68a94c0be6391f0dd23672350
    /// tsc-span: _tsc.js:103879-103984
    fn get_implicit_import_for_name(
        &mut self,
        exported_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let base =
            self.import_base
                .as_deref()
                .ok_or(TransformError::UnsupportedCompilerOption {
                    option: "jsx",
                    detail: "automatic JSX helper requested without an implicit import base",
                })?;
        let module_specifier = if exported_name == "createElement" {
            base.to_owned()
        } else {
            format!(
                "{base}/{}",
                if self.jsx_mode == 5 {
                    "jsx-dev-runtime"
                } else {
                    "jsx-runtime"
                }
            )
        };

        if let Some(existing) = self
            .implicit_imports
            .iter()
            .find(|group| group.module_specifier == module_specifier)
            .and_then(|group| {
                group
                    .imports
                    .iter()
                    .find(|import| import.exported_name == exported_name)
            })
            .cloned()
        {
            return self.create_implicit_import_reference(&existing);
        }

        let local_name = self.fresh_name(&format!("_{exported_name}"));
        let property = self.create_identifier(exported_name)?;
        let local = self.create_identifier(&local_name)?;
        let specifier = self.context.factory()?.create_node(
            self.source,
            NodeData::ImportSpecifier(tsc_syntax::nodes::ImportSpecifierData {
                name: Some(local.node()),
                property_name: Some(property.node()),
                is_type_only: false,
            }),
            TransformFlags::NONE,
        )?;
        let import = ImplicitImport {
            exported_name: exported_name.to_owned(),
            local_name,
            specifier,
        };
        if let Some(group) = self
            .implicit_imports
            .iter_mut()
            .find(|group| group.module_specifier == module_specifier)
        {
            group.imports.push(import.clone());
        } else {
            self.implicit_imports.push(ImplicitImportGroup {
                module_specifier,
                imports: vec![import.clone()],
            });
        }
        self.create_implicit_import_reference(&import)
    }

    fn create_implicit_import_reference(
        &mut self,
        import: &ImplicitImport,
    ) -> Result<TransformNode, TransformError> {
        let reference = self.create_identifier(&import.local_name)?;
        self.context
            .arena_mut()?
            .metadata_mut(reference)
            .set_referenced_import_declaration(import.specifier);
        Ok(reference)
    }

    fn fresh_name(&mut self, base: &str) -> String {
        if self.used_names.insert(base.to_owned()) {
            return base.to_owned();
        }
        let mut ordinal = 1usize;
        loop {
            let candidate = format!("{base}_{ordinal}");
            if self.used_names.insert(candidate.clone()) {
                return candidate;
            }
            ordinal += 1;
        }
    }

    fn finish_source_file(&mut self, root: TransformNode) -> Result<TransformNode, TransformError> {
        if self.filename_declaration.is_none() && self.implicit_imports.is_empty() {
            return Ok(root);
        }
        let (mut data, original_statements) = match self.context.arena().node(root)?.data.clone() {
            NodeData::SourceFile(data) => {
                let statements = data.statements;
                (data, statements)
            }
            _ => {
                return Err(TransformError::RootKindExpected {
                    actual: self.context.arena().node(root)?.kind,
                });
            }
        };
        let mut statements = self.array_nodes(original_statements)?;
        let prologue_count = statements
            .iter()
            .take_while(|statement| {
                super::is_prologue_statement(self.context.arena(), self.node(**statement))
                    .unwrap_or(false)
            })
            .count();
        let mut insertions = Vec::new();
        for group in self.implicit_imports.clone().into_iter().rev() {
            if self.is_external_module {
                insertions.push(self.create_implicit_import_statement(&group)?);
            } else if self.is_external_or_common_js_module {
                insertions.push(self.create_implicit_require_statement(&group)?);
            }
        }
        if let Some(declaration) = self.filename_declaration {
            insertions.push(self.create_filename_statement(declaration)?);
        }
        statements.splice(
            prologue_count..prologue_count,
            insertions.into_iter().map(TransformNode::node),
        );
        let statements = statements
            .into_iter()
            .map(|id| self.node(id))
            .collect::<Vec<_>>();
        // `insertStatementAfterCustomPrologue(statements.slice(), ...)` hands
        // tsc's updated SourceFile a fresh, synthesized statement array. Keep
        // that ownership boundary: source-leading detached trivia must then
        // follow the synthesized JSX runtime import instead of moving ahead
        // of it as source-file-owned trivia.
        let statements = self
            .context
            .factory()?
            .create_node_array(self.source, statements)?;
        data.statements = Some(statements.array());
        let flags = self.context.arena().transform_flags(root);
        self.context
            .factory()?
            .update_node(root, NodeData::SourceFile(data), flags)
    }

    fn create_implicit_import_statement(
        &mut self,
        group: &ImplicitImportGroup,
    ) -> Result<TransformNode, TransformError> {
        let elements = self.context.factory()?.create_node_array(
            self.source,
            group
                .imports
                .iter()
                .map(|import| import.specifier)
                .collect(),
        )?;
        let named = self.context.factory()?.create_node(
            self.source,
            NodeData::NamedImports(tsc_syntax::nodes::NamedImportsData {
                elements: Some(elements.array()),
            }),
            TransformFlags::NONE,
        )?;
        let clause = self.context.factory()?.create_node(
            self.source,
            NodeData::ImportClause(tsc_syntax::nodes::ImportClauseData {
                name: None,
                is_type_only: false,
                phase_modifier: None,
                named_bindings: Some(named.node()),
            }),
            TransformFlags::NONE,
        )?;
        let module =
            self.create_string_literal(group.module_specifier.encode_utf16().collect(), false)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ImportDeclaration(tsc_syntax::nodes::ImportDeclarationData {
                modifiers: None,
                import_clause: Some(clause.node()),
                module_specifier: Some(module.node()),
                attributes: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_implicit_require_statement(
        &mut self,
        group: &ImplicitImportGroup,
    ) -> Result<TransformNode, TransformError> {
        let mut bindings = Vec::with_capacity(group.imports.len());
        for import in &group.imports {
            let property = self.create_identifier(&import.exported_name)?;
            let name = self.create_identifier(&import.local_name)?;
            bindings.push(self.context.factory()?.create_node(
                self.source,
                NodeData::BindingElement(tsc_syntax::nodes::BindingElementData {
                    name: Some(name.node()),
                    property_name: Some(property.node()),
                    dot_dot_dot_token: None,
                    initializer: None,
                }),
                TransformFlags::NONE,
            )?);
        }
        let bindings = self
            .context
            .factory()?
            .create_node_array(self.source, bindings)?;
        let pattern = self.context.factory()?.create_node(
            self.source,
            NodeData::ObjectBindingPattern(tsc_syntax::nodes::ObjectBindingPatternData {
                elements: Some(bindings.array()),
            }),
            TransformFlags::CONTAINS_BINDING_PATTERN,
        )?;
        let require = self.create_identifier("require")?;
        let module =
            self.create_string_literal(group.module_specifier.encode_utf16().collect(), false)?;
        let arguments = self
            .context
            .factory()?
            .create_node_array(self.source, vec![module])?;
        let call = self.context.factory()?.create_node(
            self.source,
            NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
                expression: Some(require.node()),
                question_dot_token: None,
                type_arguments: None,
                arguments: Some(arguments.array()),
            }),
            TransformFlags::NONE,
        )?;
        let declaration = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(pattern.node()),
                exclamation_token: None,
                r#type: None,
                initializer: Some(call.node()),
            }),
            TransformFlags::NONE,
        )?;
        self.create_variable_statement(vec![declaration], NodeFlags::CONST)
    }

    fn create_filename_statement(
        &mut self,
        declaration: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_variable_statement(vec![declaration], NodeFlags::CONST)
    }

    fn create_variable_statement(
        &mut self,
        declarations: Vec<TransformNode>,
        flags: NodeFlags,
    ) -> Result<TransformNode, TransformError> {
        let declarations = self
            .context
            .factory()?
            .create_node_array(self.source, declarations)?;
        let list = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclarationList(tsc_syntax::nodes::VariableDeclarationListData {
                declarations: Some(declarations.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.set_node_flags(list, flags)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers: None,
                declaration_list: Some(list.node()),
            }),
            TransformFlags::NONE,
        )
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
