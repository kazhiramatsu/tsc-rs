//! Shared TypeScript tagged-template lowering for ES2015 and ES2018.
//! Template flags and cooked values belong to the parser/factory nodes;
//! lowering does not rescan raw text to recover either fact.

use tsc_syntax::{nodes::NodeData, NodeArrayId, NodeId, SyntaxKind};
use tsc_types::{JsString, TokenFlags};

use super::{helpers, target_bindings::TargetBinding};
use crate::{
    factory::EmitHelperName, TransformError, TransformNode, TransformNodeArray, TransformSourceId,
    TransformationContext,
};

/// The transformer owns visitation, binding allocation and declaration lifetime.
/// The shared algorithm owns template values and the MakeTemplateObject helper.
/// `visit_each_child_required` must perform the second tag visit at LiftRestriction;
/// the ES2018 host bypasses its memo during this bounded algorithm.
pub(super) trait TaggedTemplateHost {
    fn context(&self) -> &TransformationContext;
    fn context_mut(&mut self) -> &mut TransformationContext;
    fn source(&self) -> TransformSourceId;
    fn visit_required_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError>;
    fn visit_each_child_required(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError>;
    fn allocate_numbered_binding(&mut self, text: &str) -> Result<TargetBinding, TransformError>;
    fn record_tagged_template_string(&mut self, name: TransformNode) -> Result<(), TransformError>;
    fn create_generated_identifier(
        &mut self,
        binding: &TargetBinding,
    ) -> Result<TransformNode, TransformError>;
    fn create_array_literal(
        &mut self,
        elements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError>;
    fn create_void_zero(&mut self) -> Result<TransformNode, TransformError>;
    fn create_call(
        &mut self,
        tag: TransformNode,
        arguments: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError>;
    fn create_assignment(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError>;
    fn create_logical_or(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError>;

    fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source(), id)
    }

    fn arena_node(&self, node: TransformNode) -> Result<&tsc_syntax::Node, TransformError> {
        self.context().arena().node(node)
    }

    fn array_nodes(
        &self,
        array: Option<NodeArrayId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        match array {
            Some(array) => Ok(self
                .context()
                .arena()
                .node_array(TransformNodeArray::new(self.source(), array))?
                .nodes
                .iter()
                .map(|id| self.node(*id))
                .collect()),
            None => Ok(Vec::new()),
        }
    }

    fn is_external_module_source(&self) -> Result<bool, TransformError> {
        Ok(self
            .context()
            .arena()
            .source(self.source())?
            .syntax()
            .external_module_indicator
            .is_some())
    }

    fn create_string_literal(&mut self, text: &JsString) -> Result<TransformNode, TransformError> {
        let source = self.source();
        self.context_mut()
            .factory()?
            .create_string_literal(source, text.as_js(), false)
    }

    fn create_template_object_helper_call(
        &mut self,
        cooked: TransformNode,
        raw: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context_mut()
            .request_emit_helper(helpers::make_template_object())?;
        let source = self.source();
        let helper = self
            .context_mut()
            .factory()?
            .create_unscoped_helper_identifier(source, EmitHelperName::MakeTemplateObject)?;
        self.create_call(helper, vec![cooked, raw])
    }
}

/// tsc `ProcessLevel` (taggedTemplate.ts): `LiftRestriction` lowers only
/// templates whose cooked text is invalid (the ES2018 lane); `All` lowers
/// every tagged template (the ES2015 lane).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProcessLevel {
    LiftRestriction,
    All,
}

/// tsc-port: processTaggedTemplateExpression @6.0.3
/// tsc-hash: d318d2539195d77c458bac08f12a8adfd7b03a2c933876e9f27df4bc4782446d
/// tsc-span: _tsc.js:93972-94018
pub(super) fn process_tagged_template_expression(
    host: &mut impl TaggedTemplateHost,
    node: TransformNode,
    level: ProcessLevel,
) -> Result<TransformNode, TransformError> {
    let (tag, template) = {
        let NodeData::TaggedTemplateExpression(data) = &host.arena_node(node)?.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TaggedTemplateExpression,
                field: "tagged template",
            });
        };
        let tag = data.tag.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::TaggedTemplateExpression,
            field: "tag",
        })?;
        let template = data.template.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::TaggedTemplateExpression,
            field: "template",
        })?;
        (host.node(tag), host.node(template))
    };
    let tag = host.visit_required_expression(tag)?;
    if level == ProcessLevel::LiftRestriction && !has_invalid_escape(host, template)? {
        return host.visit_each_child_required(node);
    }
    // `templateArguments[0]` is reserved for the template-object argument;
    // span expressions are visited IN the cooked/raw loop (nested tagged
    // templates therefore allocate and record their `templateObject`
    // temps before this one, matching the upstream visit order).
    let mut span_arguments: Vec<TransformNode> = Vec::new();
    let mut cooked_strings: Vec<TransformNode> = Vec::new();
    let mut raw_strings: Vec<TransformNode> = Vec::new();
    match host.arena_node(template)?.kind {
        SyntaxKind::NoSubstitutionTemplateLiteral => {
            cooked_strings.push(create_template_cooked(host, template)?);
            raw_strings.push(get_raw_literal(host, template)?);
        }
        SyntaxKind::TemplateExpression => {
            let (head, spans) = {
                let NodeData::TemplateExpression(data) = &host.arena_node(template)?.data else {
                    unreachable!("kind-checked template expression");
                };
                let head = data.head.ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::TemplateExpression,
                    field: "head",
                })?;
                (host.node(head), host.array_nodes(data.template_spans)?)
            };
            cooked_strings.push(create_template_cooked(host, head)?);
            raw_strings.push(get_raw_literal(host, head)?);
            for span in spans {
                let (expression, literal) = {
                    let NodeData::TemplateSpan(data) = &host.arena_node(span)?.data else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::TemplateSpan,
                            field: "template span",
                        });
                    };
                    let expression =
                        data.expression
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::TemplateSpan,
                                field: "expression",
                            })?;
                    let literal = data.literal.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::TemplateSpan,
                        field: "literal",
                    })?;
                    (host.node(expression), host.node(literal))
                };
                cooked_strings.push(create_template_cooked(host, literal)?);
                raw_strings.push(get_raw_literal(host, literal)?);
                span_arguments.push(host.visit_required_expression(expression)?);
            }
        }
        _ => {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TaggedTemplateExpression,
                field: "template literal",
            });
        }
    }
    let cooked = host.create_array_literal(cooked_strings)?;
    let raw = host.create_array_literal(raw_strings)?;
    let helper_call = host.create_template_object_helper_call(cooked, raw)?;
    let template_object = if host.is_external_module_source()? {
        // `factory2.createUniqueName("templateObject")` — the eager
        // numbered arm; three identifier instances of ONE binding stand
        // for upstream's three references to one node.
        let binding = host.allocate_numbered_binding("templateObject")?;
        let declaration_name = host.create_generated_identifier(&binding)?;
        host.record_tagged_template_string(declaration_name)?;
        let or_left = host.create_generated_identifier(&binding)?;
        let assignment_left = host.create_generated_identifier(&binding)?;
        let assignment = host.create_assignment(assignment_left, helper_call)?;
        host.create_logical_or(or_left, assignment)?
    } else {
        helper_call
    };
    let mut arguments = Vec::with_capacity(1 + span_arguments.len());
    arguments.push(template_object);
    arguments.extend(span_arguments);
    host.create_call(tag, arguments)
}

/// tsc-port: createTemplateCooked @6.0.3
/// tsc-hash: 1f8f38eeb9dc74ce5fa36ea4158a351d9274f829b792c388a5a955c1c8253090
/// tsc-span: _tsc.js:94019-94021
fn create_template_cooked(
    host: &mut impl TaggedTemplateHost,
    template: TransformNode,
) -> Result<TransformNode, TransformError> {
    if host.arena_node(template)?.template_flags & TokenFlags::IS_INVALID.bits() != 0 {
        host.create_void_zero()
    } else {
        let (text, _) = template_fragment_texts(host, template)?;
        host.create_string_literal(&text)
    }
}

/// tsc-port: getRawLiteral @6.0.3
/// tsc-hash: ed2b608e1bc5d71e6dbd771ebae0d3b917f9fe54d4c114a2520a15de62c6a854
/// tsc-span: _tsc.js:94022-94033
///
/// Parsed fragments always carry `raw_text` (the parser stores exactly
/// upstream's delimiter-stripped source slice, parser.rs:7258-7270), so
/// the upstream source-file substring fallback collapses to the stored
/// bytes; a synthesized fragment without `raw_text` is a typed error
/// (upstream asserts and slices garbage positions there — "Possibly bad
/// transform").
fn get_raw_literal(
    host: &mut impl TaggedTemplateHost,
    node: TransformNode,
) -> Result<TransformNode, TransformError> {
    let (_, raw) = template_fragment_texts(host, node)?;
    // getRawLiteral's /\r\n?/g replacement on a JavaScript value.
    let mut units = raw.code_units().peekable();
    let mut text = JsString::new();
    while let Some(unit) = units.next() {
        if unit == 13 {
            if units.peek() == Some(&10) {
                units.next();
            }
            text.push_code_unit(10);
        } else {
            text.push_code_unit(unit);
        }
    }
    let literal = host.create_string_literal(&text)?;
    host.context_mut()
        .factory()?
        .set_text_range(literal, node)?;
    Ok(literal)
}

/// The fragment's `(cooked text, raw text)` pair, typed-failing on
/// non-fragment kinds and on a missing raw channel.
fn template_fragment_texts(
    host: &impl TaggedTemplateHost,
    node: TransformNode,
) -> Result<(JsString, JsString), TransformError> {
    let record = host.arena_node(node)?;
    let (text, raw_text) = match &record.data {
        NodeData::NoSubstitutionTemplateLiteral(data) => (&data.text, &data.raw_text),
        NodeData::TemplateHead(data) => (&data.text, &data.raw_text),
        NodeData::TemplateMiddle(data) => (&data.text, &data.raw_text),
        NodeData::TemplateTail(data) => (&data.text, &data.raw_text),
        _ => {
            return Err(TransformError::RequiredChildRemoved {
                parent: record.kind,
                field: "template literal fragment",
            });
        }
    };
    let raw =
        match host
            .context()
            .arena()
            .literal_properties(node)
            .and_then(|properties| properties.raw_template_text())
        {
            Some(raw) => JsString::from_code_units(raw.code_units()),
            None => raw_text.as_deref().map(JsString::from).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: record.kind,
                    field: "template literal raw text",
                },
            )?,
        };
    Ok((text.clone(), raw))
}

/// tsc `hasInvalidEscape` over the template's fragments: any fragment
/// whose parser-owned flags contain an invalid escape. Reached only from the
/// `LiftRestriction` arm.
fn has_invalid_escape(
    host: &impl TaggedTemplateHost,
    template: TransformNode,
) -> Result<bool, TransformError> {
    let record = host.arena_node(template)?;
    match &record.data {
        NodeData::NoSubstitutionTemplateLiteral(_) => {
            Ok(record.template_flags & TokenFlags::CONTAINS_INVALID_ESCAPE.bits() != 0)
        }
        NodeData::TemplateExpression(data) => {
            let head = data.head.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TemplateExpression,
                field: "head",
            })?;
            if host.arena_node(host.node(head))?.template_flags
                & TokenFlags::CONTAINS_INVALID_ESCAPE.bits()
                != 0
            {
                return Ok(true);
            }
            for span in host.array_nodes(data.template_spans)? {
                let NodeData::TemplateSpan(span_data) = &host.arena_node(span)?.data else {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::TemplateSpan,
                        field: "template span",
                    });
                };
                let literal = span_data
                    .literal
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::TemplateSpan,
                        field: "literal",
                    })?;
                if host.arena_node(host.node(literal))?.template_flags
                    & TokenFlags::CONTAINS_INVALID_ESCAPE.bits()
                    != 0
                {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Err(TransformError::RequiredChildRemoved {
            parent: record.kind,
            field: "template literal",
        }),
    }
}
