//! The bounded expression printer used by nodeBuilder annotation reuse.
//!
//! This is deliberately not an emitter. It prints the synthesized face of
//! expression nodes reachable from an existing annotation. It does not copy
//! source text, comments, source maps, substitutions, or declaration emit
//! state; like tsc's standard printer, however, it probes parsed source ranges
//! to preserve the line layout that controls indentation. Its writer has an
//! empty newline string, so a line event contributes only the indentation
//! prefixed to the next token. A `None` result asks the enclosing reuse
//! boundary to serialize the owning type semantically.

use tsrs2_syntax::{NodeArrayId, NodeData, NodeId, SyntaxKind};
use tsrs2_types::NodeFlags;

use crate::state::{CheckResult, CheckerState};

const PRECEDENCE_INVALID: i8 = -1;
const PRECEDENCE_COMMA: i8 = 0;
const PRECEDENCE_SPREAD: i8 = 1;
const PRECEDENCE_YIELD: i8 = 2;
const PRECEDENCE_ASSIGNMENT: i8 = 3;
const PRECEDENCE_CONDITIONAL: i8 = 4;
const PRECEDENCE_RELATIONAL: i8 = 11;
const PRECEDENCE_UNARY: i8 = 16;
const PRECEDENCE_UPDATE: i8 = 17;
const PRECEDENCE_LEFT_HAND_SIDE: i8 = 18;
const PRECEDENCE_MEMBER: i8 = 19;
const PRECEDENCE_PRIMARY: i8 = 20;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Associativity {
    Left,
    Right,
}

struct DisplayClonePrinter<'state, 'program> {
    state: &'state mut CheckerState<'program>,
}

impl<'program> CheckerState<'program> {
    /// tsc-port: emitExpression-family @6.0.3
    /// tsc-hash: 4e2066b2aa5c01833faa01905c0115436468778dc369c25b6719db8a5f531af9
    /// tsc-span: _tsc.js:117158-119469
    ///
    /// Prints the standard-printer face of a cloned AssignmentExpression,
    /// consulting source ranges only for tsc's layout/indentation probes.
    /// Callers provide the writer's pending line-start state explicitly.
    /// Body-bearing expressions cross one narrow hook into the companion body
    /// printer; either half can return `None` to replace the enclosing TypeNode
    /// at the caller-owned recovery boundary.
    ///
    /// Enters a nested expression with the standard writer's line-start
    /// state scoped to that one recursive emission. Callers have already
    /// materialized the pending indentation in their composed `String` when
    /// `at_line_start` is true.
    pub(crate) fn display_clone_expression_text_at_line_start(
        &mut self,
        node: NodeId,
        at_line_start: bool,
    ) -> CheckResult<Option<String>> {
        let saved = self.slice_display_clone_at_line_start;
        self.slice_display_clone_at_line_start = at_line_start;
        let result = DisplayClonePrinter { state: self }.expression(node);
        self.slice_display_clone_at_line_start = saved;
        result
    }

    /// The standard printer's range-line probes operate on a range's
    /// trivia-skipped start. Parser positions are bytes in this port, while
    /// `LineMap` (like tsc) is indexed in UTF-16.
    /// tsrs-native: byte-to-UTF-16 line-map adapter for printer range probes.
    pub(crate) fn display_clone_start_line(&self, node: NodeId) -> Option<usize> {
        let source = self.binder.source_of_node(node);
        let record = source.arena.node(node);
        if record.pos == u32::MAX || record.pos as usize > source.text.len() {
            return None;
        }
        let byte = tsrs2_syntax::skip_trivia(&source.text, record.pos as usize);
        display_clone_line_of_byte(source, byte)
    }

    /// The standard printer compares range ends without skipping trivia.
    /// tsrs-native: byte-to-UTF-16 line-map adapter for printer range probes.
    pub(crate) fn display_clone_end_line(&self, node: NodeId) -> Option<usize> {
        let source = self.binder.source_of_node(node);
        let record = source.arena.node(node);
        if record.end == u32::MAX || record.end as usize > source.text.len() {
            return None;
        }
        display_clone_line_of_byte(source, record.end as usize)
    }

    /// `createTextWriter("")` drops the newline bytes but still emits four
    /// spaces at the next write for every active indentation level.
    /// tsrs-native: string projection of the empty-newline writer's indent.
    pub(crate) fn display_clone_line_indent(&self) -> String {
        "    ".repeat(self.slice_display_clone_indent)
    }
}

impl DisplayClonePrinter<'_, '_> {
    fn expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        match self.state.kind_of(node) {
            // Token/name/literal leaves.
            SyntaxKind::ThisKeyword
            | SyntaxKind::SuperKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::ImportKeyword => self.token_expression(node),
            SyntaxKind::Identifier => self.identifier(node),
            SyntaxKind::PrivateIdentifier => self.private_identifier(node),
            SyntaxKind::NumericLiteral => self.numeric_literal(node),
            SyntaxKind::BigIntLiteral => self.bigint_literal(node),
            SyntaxKind::StringLiteral => self.string_literal(node),
            SyntaxKind::RegularExpressionLiteral => self.regular_expression_literal(node),
            SyntaxKind::NoSubstitutionTemplateLiteral => {
                self.no_substitution_template_literal(node)
            }

            // Primary/member/call expressions.
            SyntaxKind::ArrayLiteralExpression => self.array_literal(node),
            SyntaxKind::ObjectLiteralExpression => self.object_literal(node),
            SyntaxKind::PropertyAccessExpression => self.property_access(node),
            SyntaxKind::ElementAccessExpression => self.element_access(node),
            SyntaxKind::CallExpression => self.call_expression(node),
            SyntaxKind::NewExpression => self.new_expression(node),
            SyntaxKind::TaggedTemplateExpression => self.tagged_template(node),
            SyntaxKind::ParenthesizedExpression => self.parenthesized_expression(node),

            // Type-bearing expressions reuse the annotation visitor for every
            // embedded TypeNode before printing the surrounding expression.
            SyntaxKind::TypeAssertionExpression => self.type_assertion(node),
            SyntaxKind::ExpressionWithTypeArguments => self.expression_with_type_arguments(node),
            SyntaxKind::AsExpression => self.as_expression(node),
            SyntaxKind::SatisfiesExpression => self.satisfies_expression(node),
            SyntaxKind::NonNullExpression => self.non_null_expression(node),

            // Unary/binary/assignment grammar.
            SyntaxKind::DeleteExpression => self.keyword_unary(node, "delete"),
            SyntaxKind::TypeOfExpression => self.keyword_unary(node, "typeof"),
            SyntaxKind::VoidExpression => self.keyword_unary(node, "void"),
            SyntaxKind::AwaitExpression => self.keyword_unary(node, "await"),
            SyntaxKind::PrefixUnaryExpression => self.prefix_unary(node),
            SyntaxKind::PostfixUnaryExpression => self.postfix_unary(node),
            SyntaxKind::BinaryExpression => self.binary_expression(node),
            SyntaxKind::ConditionalExpression => self.conditional_expression(node),
            SyntaxKind::YieldExpression => self.yield_expression(node),
            SyntaxKind::SpreadElement => self.spread_element(node),

            // Template/meta expressions.
            SyntaxKind::TemplateExpression => self.template_expression(node),
            SyntaxKind::MetaProperty => self.meta_property(node),

            // JSX is an AssignmentExpression leaf whose children can contain
            // arbitrary nested AssignmentExpressions.
            SyntaxKind::JsxElement
            | SyntaxKind::JsxSelfClosingElement
            | SyntaxKind::JsxFragment => self.jsx(node),

            // Transformation-only nodes are handled defensively even though
            // existing source annotations do not construct them.
            SyntaxKind::PartiallyEmittedExpression => self.partially_emitted_expression(node),
            SyntaxKind::CommaListExpression => self.comma_list_expression(node),
            SyntaxKind::OmittedExpression => Ok(Some(String::new())),
            SyntaxKind::MissingDeclaration => Ok(Some(String::new())),

            // Bodies open the statement/declaration grammar closure and cross
            // the single companion-module hook.
            SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::ClassExpression => self.state.display_clone_body_expression_text(node),

            // SyntheticExpression is a printer assertion in tsc. All other
            // syntax kinds are outside AssignmentExpression grammar.
            SyntaxKind::SyntheticExpression => Ok(None),
            _ => Ok(None),
        }
    }

    fn token_expression(&self, node: NodeId) -> CheckResult<Option<String>> {
        Ok(tsrs2_syntax::tokens::token_to_string(self.state.kind_of(node)).map(str::to_owned))
    }

    fn identifier(&self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::Identifier(data) = self.state.data_of(node) else {
            return Ok(None);
        };
        Ok(Some(
            tsrs2_binder::unescape_leading_underscores(&data.escaped_text).to_owned(),
        ))
    }

    fn private_identifier(&self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::PrivateIdentifier(data) = self.state.data_of(node) else {
            return Ok(None);
        };
        Ok(Some(data.text.clone()))
    }

    fn numeric_literal(&self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::NumericLiteral(data) = self.state.data_of(node) else {
            return Ok(None);
        };
        Ok(Some(data.text.clone()))
    }

    fn bigint_literal(&self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::BigIntLiteral(data) = self.state.data_of(node) else {
            return Ok(None);
        };
        Ok(Some(data.text.clone()))
    }

    fn string_literal(&self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::StringLiteral(data) = self.state.data_of(node) else {
            return Ok(None);
        };
        Ok(Some(quoted_string(&data.text)))
    }

    fn regular_expression_literal(&self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::RegularExpressionLiteral(data) = self.state.data_of(node) else {
            return Ok(None);
        };
        Ok(Some(data.text.clone()))
    }

    fn no_substitution_template_literal(&self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::NoSubstitutionTemplateLiteral(data) = self.state.data_of(node) else {
            return Ok(None);
        };
        let raw = data
            .raw_text
            .clone()
            .unwrap_or_else(|| template_text_raw(&data.text));
        Ok(Some(format!("`{raw}`")))
    }

    fn array_literal(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ArrayLiteralExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let (elements, has_trailing_comma) = self.node_array(data.elements);
        if elements.is_empty() {
            return Ok(Some("[]".to_owned()));
        }
        let prefer_new_line = self.node_is_multi_line(node);
        let leading_new_line = self.list_has_leading_line(node, elements[0], prefer_new_line);
        let closing_new_line = self.list_has_closing_line(
            node,
            *elements.last().expect("nonempty array literal"),
            prefer_new_line,
        );
        let contents = self.with_increased_indent(|printer| {
            let mut contents = String::new();
            if leading_new_line {
                contents.push_str(&printer.state.display_clone_line_indent());
            }
            let mut previous = None;
            for element in elements {
                let at_line_start;
                if let Some(previous) = previous {
                    if printer
                        .state
                        .binder
                        .source_of_node(previous)
                        .arena
                        .node(previous)
                        .end
                        != printer
                            .state
                            .binder
                            .source_of_node(node)
                            .arena
                            .node(node)
                            .end
                    {
                        contents.push(',');
                    }
                    if printer.list_has_separating_line(previous, element, prefer_new_line) {
                        contents.push_str(&printer.state.display_clone_line_indent());
                        at_line_start = true;
                    } else {
                        contents.push(' ');
                        at_line_start = false;
                    }
                } else {
                    at_line_start = leading_new_line;
                }
                if printer.state.kind_of(element) != SyntaxKind::OmittedExpression {
                    let Some(text) = printer.with_line_start(at_line_start, |printer| {
                        printer.expression_for_disallowed_comma(element)
                    })?
                    else {
                        return Ok(None);
                    };
                    contents.push_str(&text);
                }
                previous = Some(element);
            }
            if has_trailing_comma {
                contents.push(',');
            }
            Ok(Some(contents))
        })?;
        let Some(mut contents) = contents else {
            return Ok(None);
        };
        if closing_new_line {
            contents.push_str(&self.state.display_clone_line_indent());
        }
        Ok(Some(format!("[{contents}]")))
    }

    fn object_literal(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ObjectLiteralExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let (properties, has_trailing_comma) = self.node_array(data.properties);
        if properties.is_empty() {
            return Ok(Some("{}".to_owned()));
        }
        let mut emitted = Vec::with_capacity(properties.len());
        for property in properties {
            if self.object_member_is_removed(property)? {
                continue;
            }
            emitted.push(property);
        }
        if emitted.is_empty() {
            return Ok(Some("{}".to_owned()));
        }
        let prefer_new_line = self.node_is_multi_line(node);
        let leading_new_line = self.list_has_leading_line(node, emitted[0], prefer_new_line);
        let closing_new_line = self.list_has_closing_line(
            node,
            *emitted.last().expect("nonempty object literal"),
            prefer_new_line,
        );
        let contents = self.with_increased_indent(|printer| {
            let mut contents = String::new();
            if leading_new_line {
                contents.push_str(&printer.state.display_clone_line_indent());
            } else {
                contents.push(' ');
            }
            let mut previous = None;
            for property in emitted {
                let at_line_start;
                if let Some(previous) = previous {
                    if printer
                        .state
                        .binder
                        .source_of_node(previous)
                        .arena
                        .node(previous)
                        .end
                        != printer
                            .state
                            .binder
                            .source_of_node(node)
                            .arena
                            .node(node)
                            .end
                    {
                        contents.push(',');
                    }
                    if printer.list_has_separating_line(previous, property, prefer_new_line) {
                        contents.push_str(&printer.state.display_clone_line_indent());
                        at_line_start = true;
                    } else {
                        contents.push(' ');
                        at_line_start = false;
                    }
                } else {
                    at_line_start = leading_new_line;
                }
                let Some(text) = printer
                    .with_line_start(at_line_start, |printer| printer.object_member(property))?
                else {
                    return Ok(None);
                };
                contents.push_str(&text);
                previous = Some(property);
            }
            if has_trailing_comma {
                contents.push(',');
            }
            Ok(Some(contents))
        })?;
        let Some(mut contents) = contents else {
            return Ok(None);
        };
        if closing_new_line {
            contents.push_str(&self.state.display_clone_line_indent());
        } else {
            contents.push(' ');
        }
        Ok(Some(format!("{{{contents}}}")))
    }

    fn object_member(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        match self.state.data_of(node).clone() {
            NodeData::PropertyAssignment(data) => {
                let Some(name) = data.name else {
                    return Ok(None);
                };
                let name = self.with_line_start(false, |printer| {
                    printer.state.member_name_node_text_slice(name)
                })?;
                let Some(initializer) = data.initializer else {
                    return Ok(None);
                };
                let Some(initializer) = self.with_line_start(false, |printer| {
                    printer.expression_for_disallowed_comma(initializer)
                })?
                else {
                    return Ok(None);
                };
                Ok(Some(format!("{name}: {initializer}")))
            }
            NodeData::ShorthandPropertyAssignment(data) => {
                let Some(name) = data.name else {
                    return Ok(None);
                };
                let Some(mut text) = self.identifier_name(name) else {
                    return Ok(None);
                };
                if let Some(initializer) = data.object_assignment_initializer {
                    let Some(initializer) = self.with_line_start(false, |printer| {
                        printer.expression_for_disallowed_comma(initializer)
                    })?
                    else {
                        return Ok(None);
                    };
                    text.push_str(" = ");
                    text.push_str(&initializer);
                }
                Ok(Some(text))
            }
            NodeData::SpreadAssignment(data) => {
                let Some(expression) = data.expression else {
                    return Ok(None);
                };
                let Some(expression) = self.with_line_start(false, |printer| {
                    printer.expression_for_disallowed_comma(expression)
                })?
                else {
                    return Ok(None);
                };
                Ok(Some(format!("...{expression}")))
            }
            NodeData::MethodDeclaration(_)
            | NodeData::GetAccessor(_)
            | NodeData::SetAccessor(_) => self.state.display_clone_body_expression_text(node),
            _ => Ok(None),
        }
    }

    fn object_member_is_removed(&mut self, node: NodeId) -> CheckResult<bool> {
        let source = self.state.binder.source_of_node(node);
        let computed_name = tsrs2_binder::node_util::get_name_of_declaration(source, node)
            .is_some_and(|name| self.state.kind_of(name) == SyntaxKind::ComputedPropertyName);
        if !computed_name || !tsrs2_binder::node_util::has_dynamic_name(source, node) {
            return Ok(false);
        }
        Ok(!self.state.has_bindable_name(node)?)
    }

    fn property_access(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::PropertyAccessExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let Some(name_node) = data.name else {
            return Ok(None);
        };
        let Some(name) = self.identifier_name(name_node) else {
            return Ok(None);
        };
        let (line_before_dot, line_after_dot) = match data.question_dot_token {
            Some(token) => (
                self.nodes_have_line_between(expression, token),
                self.nodes_have_line_between(token, name_node),
            ),
            None => self.synthetic_property_dot_lines(expression, name_node),
        };
        self.with_restored_indent(|printer| {
            let Some(mut text) =
                printer.left_side_of_access(expression, printer.is_optional_chain(node))?
            else {
                return Ok(None);
            };
            if line_before_dot {
                printer.state.slice_display_clone_indent += 1;
                text.push_str(&printer.state.display_clone_line_indent());
            }
            if data.question_dot_token.is_some() {
                text.push_str("?.");
            } else {
                if printer.numeric_access_needs_second_dot(expression) {
                    text.push('.');
                }
                text.push('.');
            }
            if line_after_dot {
                printer.state.slice_display_clone_indent += 1;
                text.push_str(&printer.state.display_clone_line_indent());
            }
            text.push_str(&name);
            Ok(Some(text))
        })
    }

    fn element_access(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ElementAccessExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let Some(expression) =
            self.left_side_of_access(expression, self.is_optional_chain(node))?
        else {
            return Ok(None);
        };
        let Some(argument) = data.argument_expression else {
            return Ok(None);
        };
        let Some(argument) = self.with_line_start(false, |printer| printer.expression(argument))?
        else {
            return Ok(None);
        };
        let optional = if data.question_dot_token.is_some() {
            "?."
        } else {
            ""
        };
        Ok(Some(format!("{expression}{optional}[{argument}]")))
    }

    fn call_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::CallExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let Some(mut text) = self.left_side_of_access(expression, self.is_optional_chain(node))?
        else {
            return Ok(None);
        };
        if data.question_dot_token.is_some() {
            text.push_str("?.");
        }
        text.push_str(&self.type_arguments(data.type_arguments)?);
        let (arguments, _) = self.node_array(data.arguments);
        let Some(arguments) =
            self.with_line_start(false, |printer| printer.expression_list(arguments, true))?
        else {
            return Ok(None);
        };
        text.push('(');
        text.push_str(&arguments);
        text.push(')');
        Ok(Some(text))
    }

    fn new_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::NewExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let Some(expression) =
            self.with_line_start(false, |printer| printer.expression_of_new(expression))?
        else {
            return Ok(None);
        };
        let mut text = format!(
            "new {expression}{}",
            self.type_arguments(data.type_arguments)?
        );
        if let Some(arguments) = data.arguments {
            let (arguments, _) = self.node_array(Some(arguments));
            let Some(arguments) =
                self.with_line_start(false, |printer| printer.expression_list(arguments, true))?
            else {
                return Ok(None);
            };
            text.push('(');
            text.push_str(&arguments);
            text.push(')');
        }
        Ok(Some(text))
    }

    fn tagged_template(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::TaggedTemplateExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(tag) = data.tag else {
            return Ok(None);
        };
        let Some(tag) = self.left_side_of_access(tag, self.is_optional_chain(node))? else {
            return Ok(None);
        };
        let type_arguments = self.type_arguments(data.type_arguments)?;
        let Some(template) = data.template else {
            return Ok(None);
        };
        let Some(template) = self.with_line_start(false, |printer| printer.expression(template))?
        else {
            return Ok(None);
        };
        Ok(Some(format!("{tag}{type_arguments} {template}")))
    }

    fn parenthesized_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ParenthesizedExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        Ok(self
            .with_line_start(false, |printer| printer.expression(expression))?
            .map(|expression| format!("({expression})")))
    }

    fn type_assertion(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::TypeAssertionExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(ty) = data.r#type else {
            return Ok(None);
        };
        let ty = self.with_line_start(false, |printer| {
            printer.state.type_annotation_text_slice(ty)
        })?;
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let Some(expression) =
            self.with_line_start(false, |printer| printer.operand_of_prefix_unary(expression))?
        else {
            return Ok(None);
        };
        Ok(Some(format!("<{ty}>{expression}")))
    }

    fn expression_with_type_arguments(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ExpressionWithTypeArguments(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let Some(expression) =
            self.left_side_of_access(expression, self.is_optional_chain(node))?
        else {
            return Ok(None);
        };
        let type_arguments = self.type_arguments(data.type_arguments)?;
        Ok(Some(format!("{expression}{type_arguments}")))
    }

    fn as_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::AsExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let Some(expression) = self.expression(expression)? else {
            return Ok(None);
        };
        let Some(ty) = data.r#type else {
            return Ok(None);
        };
        let ty = self.with_line_start(false, |printer| {
            printer.state.type_annotation_text_slice(ty)
        })?;
        Ok(Some(format!("{expression} as {ty}")))
    }

    fn satisfies_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::SatisfiesExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let Some(expression) = self.expression(expression)? else {
            return Ok(None);
        };
        let Some(ty) = data.r#type else {
            return Ok(None);
        };
        let ty = self.with_line_start(false, |printer| {
            printer.state.type_annotation_text_slice(ty)
        })?;
        Ok(Some(format!("{expression} satisfies {ty}")))
    }

    fn non_null_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::NonNullExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        Ok(self
            .left_side_of_access(expression, self.is_optional_chain(node))?
            .map(|expression| format!("{expression}!")))
    }

    fn keyword_unary(
        &mut self,
        node: NodeId,
        keyword: &'static str,
    ) -> CheckResult<Option<String>> {
        let expression = match self.state.data_of(node).clone() {
            NodeData::DeleteExpression(data) => data.expression,
            NodeData::TypeOfExpression(data) => data.expression,
            NodeData::VoidExpression(data) => data.expression,
            NodeData::AwaitExpression(data) => data.expression,
            _ => return Ok(None),
        };
        let Some(expression) = expression else {
            return Ok(None);
        };
        Ok(self
            .with_line_start(false, |printer| printer.operand_of_prefix_unary(expression))?
            .map(|expression| format!("{keyword} {expression}")))
    }

    fn prefix_unary(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::PrefixUnaryExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(operator) = tsrs2_syntax::tokens::token_to_string(data.operator) else {
            return Ok(None);
        };
        let Some(operand) = data.operand else {
            return Ok(None);
        };
        let needs_space = match self.state.data_of(operand) {
            NodeData::PrefixUnaryExpression(inner) => {
                (data.operator == SyntaxKind::PlusToken
                    && matches!(
                        inner.operator,
                        SyntaxKind::PlusToken | SyntaxKind::PlusPlusToken
                    ))
                    || (data.operator == SyntaxKind::MinusToken
                        && matches!(
                            inner.operator,
                            SyntaxKind::MinusToken | SyntaxKind::MinusMinusToken
                        ))
            }
            _ => false,
        };
        let Some(operand) =
            self.with_line_start(false, |printer| printer.operand_of_prefix_unary(operand))?
        else {
            return Ok(None);
        };
        Ok(Some(format!(
            "{operator}{}{operand}",
            if needs_space { " " } else { "" }
        )))
    }

    fn postfix_unary(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::PostfixUnaryExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(operator) = tsrs2_syntax::tokens::token_to_string(data.operator) else {
            return Ok(None);
        };
        let Some(operand) = data.operand else {
            return Ok(None);
        };
        let Some(operand) = self.operand_of_postfix_unary(operand)? else {
            return Ok(None);
        };
        Ok(Some(format!("{operand}{operator}")))
    }

    fn binary_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::BinaryExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let (Some(left), Some(operator_token), Some(right)) =
            (data.left, data.operator_token, data.right)
        else {
            return Ok(None);
        };
        let operator_kind = self.state.kind_of(operator_token);
        let Some(operator) = tsrs2_syntax::tokens::token_to_string(operator_kind) else {
            return Ok(None);
        };
        let line_before_operator = self.nodes_have_line_between(left, operator_token);
        let line_after_operator = self.nodes_have_line_between(operator_token, right);
        self.with_restored_indent(|printer| {
            let Some(mut text) = printer.binary_operand(operator_kind, left, true, None)? else {
                return Ok(None);
            };
            if line_before_operator {
                printer.state.slice_display_clone_indent += 1;
                text.push_str(&printer.state.display_clone_line_indent());
            } else if operator_kind != SyntaxKind::CommaToken {
                text.push(' ');
            }
            text.push_str(operator);
            if line_after_operator {
                printer.state.slice_display_clone_indent += 1;
                text.push_str(&printer.state.display_clone_line_indent());
            } else {
                text.push(' ');
            }
            let Some(right) = printer.with_line_start(line_after_operator, |printer| {
                printer.binary_operand(operator_kind, right, false, Some(left))
            })?
            else {
                return Ok(None);
            };
            text.push_str(&right);
            Ok(Some(text))
        })
    }

    fn conditional_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ConditionalExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let (Some(condition), Some(when_true), Some(when_false)) =
            (data.condition, data.when_true, data.when_false)
        else {
            return Ok(None);
        };
        let (Some(question), Some(colon)) = (data.question_token, data.colon_token) else {
            return Ok(None);
        };
        let line_before_question = self.nodes_have_line_between(condition, question);
        let line_after_question = self.nodes_have_line_between(question, when_true);
        let line_before_colon = self.nodes_have_line_between(when_true, colon);
        let line_after_colon = self.nodes_have_line_between(colon, when_false);
        self.with_restored_indent(|printer| {
            let condition_needs_parentheses =
                printer.expression_precedence(condition) <= PRECEDENCE_CONDITIONAL;
            let condition_entry = if condition_needs_parentheses {
                false
            } else {
                printer.state.slice_display_clone_at_line_start
            };
            let Some(mut text) = printer
                .with_line_start(condition_entry, |printer| printer.expression(condition))?
            else {
                return Ok(None);
            };
            if condition_needs_parentheses {
                text = format!("({text})");
            }
            if line_before_question {
                printer.state.slice_display_clone_indent += 1;
                text.push_str(&printer.state.display_clone_line_indent());
            } else {
                text.push(' ');
            }
            text.push('?');
            if line_after_question {
                printer.state.slice_display_clone_indent += 1;
                text.push_str(&printer.state.display_clone_line_indent());
            } else {
                text.push(' ');
            }
            let Some(when_true_text) = printer.with_line_start(line_after_question, |printer| {
                printer.conditional_branch(when_true)
            })?
            else {
                return Ok(None);
            };
            text.push_str(&when_true_text);
            if line_after_question {
                printer.state.slice_display_clone_indent -= 1;
            }
            if line_before_question {
                printer.state.slice_display_clone_indent -= 1;
            }
            if line_before_colon {
                printer.state.slice_display_clone_indent += 1;
                text.push_str(&printer.state.display_clone_line_indent());
            } else {
                text.push(' ');
            }
            text.push(':');
            if line_after_colon {
                printer.state.slice_display_clone_indent += 1;
                text.push_str(&printer.state.display_clone_line_indent());
            } else {
                text.push(' ');
            }
            let Some(when_false_text) = printer.with_line_start(line_after_colon, |printer| {
                printer.conditional_branch(when_false)
            })?
            else {
                return Ok(None);
            };
            text.push_str(&when_false_text);
            Ok(Some(text))
        })
    }

    fn yield_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::YieldExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let mut text = "yield".to_owned();
        if data.asterisk_token.is_some() {
            text.push('*');
        }
        if let Some(expression) = data.expression {
            let Some(expression) = self.with_line_start(false, |printer| {
                printer.expression_for_disallowed_comma(expression)
            })?
            else {
                return Ok(None);
            };
            text.push(' ');
            text.push_str(&expression);
        }
        Ok(Some(text))
    }

    fn spread_element(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::SpreadElement(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        Ok(self
            .with_line_start(false, |printer| {
                printer.expression_for_disallowed_comma(expression)
            })?
            .map(|expression| format!("...{expression}")))
    }

    fn template_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::TemplateExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(head) = data.head else {
            return Ok(None);
        };
        let NodeData::TemplateHead(head) = self.state.data_of(head).clone() else {
            return Ok(None);
        };
        let raw = head
            .raw_text
            .unwrap_or_else(|| template_text_raw(&head.text));
        let mut text = format!("`{raw}");
        let (spans, _) = self.node_array(data.template_spans);
        for span in spans {
            let NodeData::TemplateSpan(span) = self.state.data_of(span).clone() else {
                return Ok(None);
            };
            let Some(expression) = span.expression else {
                return Ok(None);
            };
            let Some(expression) =
                self.with_line_start(false, |printer| printer.expression(expression))?
            else {
                return Ok(None);
            };
            let Some(literal) = span.literal else {
                return Ok(None);
            };
            let raw = match self.state.data_of(literal).clone() {
                NodeData::TemplateMiddle(data) => data
                    .raw_text
                    .unwrap_or_else(|| template_text_raw(&data.text)),
                NodeData::TemplateTail(data) => data
                    .raw_text
                    .unwrap_or_else(|| template_text_raw(&data.text)),
                _ => return Ok(None),
            };
            text.push_str("${");
            text.push_str(&expression);
            text.push('}');
            text.push_str(&raw);
        }
        text.push('`');
        Ok(Some(text))
    }

    fn meta_property(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::MetaProperty(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(keyword) = tsrs2_syntax::tokens::token_to_string(data.keyword_token) else {
            return Ok(None);
        };
        let Some(name) = data.name.and_then(|name| self.identifier_name(name)) else {
            return Ok(None);
        };
        Ok(Some(format!("{keyword}.{name}")))
    }

    fn partially_emitted_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::PartiallyEmittedExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        match data.expression {
            Some(expression) => self.expression(expression),
            None => Ok(None),
        }
    }

    fn comma_list_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::CommaListExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let (elements, _) = self.node_array(data.elements);
        self.expression_list(elements, false)
    }

    fn jsx(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        match self.state.data_of(node).clone() {
            NodeData::JsxElement(data) => {
                let (Some(opening), Some(closing)) = (data.opening_element, data.closing_element)
                else {
                    return Ok(None);
                };
                let Some(mut text) = self.with_line_start(false, |printer| printer.jsx(opening))?
                else {
                    return Ok(None);
                };
                let (children, _) = self.node_array(data.children);
                for child in children {
                    let child_at_line_start = text_ends_at_line_start(&text);
                    let entry_indent = usize::from(
                        self.state.kind_of(child) == SyntaxKind::JsxExpression
                            && self.node_raw_range_is_multi_line(child),
                    );
                    let Some(child) = self
                        .with_line_start(child_at_line_start, |printer| printer.jsx_child(child))?
                    else {
                        return Ok(None);
                    };
                    self.append_jsx_write(&mut text, &child, entry_indent);
                }
                let closing_at_line_start = text_ends_at_line_start(&text);
                let Some(closing) =
                    self.with_line_start(closing_at_line_start, |printer| printer.jsx(closing))?
                else {
                    return Ok(None);
                };
                self.append_jsx_write(&mut text, &closing, 0);
                Ok(Some(text))
            }
            NodeData::JsxSelfClosingElement(data) => {
                let Some(tag_name) = data.tag_name else {
                    return Ok(None);
                };
                let Some(tag_name) =
                    self.with_line_start(false, |printer| printer.jsx_tag_name(tag_name))?
                else {
                    return Ok(None);
                };
                let type_arguments = self.type_arguments(data.type_arguments)?;
                let Some(attributes) = data.attributes else {
                    return Ok(None);
                };
                let Some(attributes) =
                    self.with_line_start(false, |printer| printer.jsx(attributes))?
                else {
                    return Ok(None);
                };
                Ok(Some(format!("<{tag_name}{type_arguments} {attributes}/>")))
            }
            NodeData::JsxOpeningElement(data) => {
                let Some(tag_name) = data.tag_name else {
                    return Ok(None);
                };
                let Some(tag_name) =
                    self.with_line_start(false, |printer| printer.jsx_tag_name(tag_name))?
                else {
                    return Ok(None);
                };
                let type_arguments = self.type_arguments(data.type_arguments)?;
                let Some(attributes) = data.attributes else {
                    return Ok(None);
                };
                let Some(attributes) =
                    self.with_line_start(false, |printer| printer.jsx(attributes))?
                else {
                    return Ok(None);
                };
                let separator = if attributes.is_empty() { "" } else { " " };
                Ok(Some(format!(
                    "<{tag_name}{type_arguments}{separator}{attributes}>"
                )))
            }
            NodeData::JsxClosingElement(data) => {
                let Some(tag_name) = data.tag_name else {
                    return Ok(None);
                };
                Ok(self
                    .with_line_start(false, |printer| printer.jsx_tag_name(tag_name))?
                    .map(|tag_name| format!("</{tag_name}>")))
            }
            NodeData::JsxFragment(data) => {
                let mut text = "<>".to_owned();
                let (children, _) = self.node_array(data.children);
                for child in children {
                    let child_at_line_start = text_ends_at_line_start(&text);
                    let entry_indent = usize::from(
                        self.state.kind_of(child) == SyntaxKind::JsxExpression
                            && self.node_raw_range_is_multi_line(child),
                    );
                    let Some(child) = self
                        .with_line_start(child_at_line_start, |printer| printer.jsx_child(child))?
                    else {
                        return Ok(None);
                    };
                    self.append_jsx_write(&mut text, &child, entry_indent);
                }
                self.append_jsx_write(&mut text, "</>", 0);
                Ok(Some(text))
            }
            NodeData::JsxAttributes(data) => {
                let (properties, _) = self.node_array(data.properties);
                let mut rendered = Vec::with_capacity(properties.len());
                for property in properties {
                    let Some(property) =
                        self.with_line_start(false, |printer| printer.jsx(property))?
                    else {
                        return Ok(None);
                    };
                    rendered.push(property);
                }
                Ok(Some(rendered.join(" ")))
            }
            NodeData::JsxAttribute(data) => {
                let Some(name) = data.name else {
                    return Ok(None);
                };
                let Some(mut text) = self.jsx_attribute_name(name) else {
                    return Ok(None);
                };
                if let Some(initializer) = data.initializer {
                    let Some(initializer) = self.with_line_start(false, |printer| {
                        printer.jsx_attribute_value(initializer)
                    })?
                    else {
                        return Ok(None);
                    };
                    text.push('=');
                    text.push_str(&initializer);
                }
                Ok(Some(text))
            }
            NodeData::JsxSpreadAttribute(data) => {
                let Some(expression) = data.expression else {
                    return Ok(None);
                };
                Ok(self
                    .with_line_start(false, |printer| printer.expression(expression))?
                    .map(|expression| format!("{{...{expression}}}")))
            }
            NodeData::JsxExpression(data) => {
                let Some(expression) = data.expression else {
                    // With removeComments enabled, an empty JSX expression
                    // (normally a comment container) emits nothing.
                    return Ok(Some(String::new()));
                };
                let multi_line = self.node_raw_range_is_multi_line(node);
                self.with_restored_indent(|printer| {
                    if multi_line {
                        printer.state.slice_display_clone_indent += 1;
                    }
                    let Some(expression) =
                        printer.with_line_start(false, |printer| printer.expression(expression))?
                    else {
                        return Ok(None);
                    };
                    let dots = if data.dot_dot_dot_token.is_some() {
                        "..."
                    } else {
                        ""
                    };
                    Ok(Some(format!("{{{dots}{expression}}}")))
                })
            }
            NodeData::JsxNamespacedName(_) => Ok(self.jsx_attribute_name(node)),
            NodeData::JsxText(data) => Ok(Some(data.text)),
            NodeData::Token if self.state.kind_of(node) == SyntaxKind::JsxOpeningFragment => {
                Ok(Some("<>".to_owned()))
            }
            NodeData::Token if self.state.kind_of(node) == SyntaxKind::JsxClosingFragment => {
                Ok(Some("</>".to_owned()))
            }
            _ => Ok(None),
        }
    }

    fn jsx_child(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        match self.state.kind_of(node) {
            SyntaxKind::JsxText
            | SyntaxKind::JsxTextAllWhiteSpaces
            | SyntaxKind::JsxExpression
            | SyntaxKind::JsxElement
            | SyntaxKind::JsxSelfClosingElement
            | SyntaxKind::JsxFragment => self.jsx(node),
            _ => Ok(None),
        }
    }

    fn jsx_attribute_value(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        if let NodeData::StringLiteral(data) = self.state.data_of(node) {
            return Ok(Some(quoted_jsx_attribute(&data.text)));
        }
        match self.state.kind_of(node) {
            SyntaxKind::JsxExpression
            | SyntaxKind::JsxElement
            | SyntaxKind::JsxSelfClosingElement
            | SyntaxKind::JsxFragment => self.jsx(node),
            _ => Ok(None),
        }
    }

    fn jsx_tag_name(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        match self.state.kind_of(node) {
            SyntaxKind::Identifier
            | SyntaxKind::ThisKeyword
            | SyntaxKind::PropertyAccessExpression => self.expression(node),
            SyntaxKind::JsxNamespacedName => Ok(self.jsx_attribute_name(node)),
            _ => Ok(None),
        }
    }

    fn jsx_attribute_name(&self, node: NodeId) -> Option<String> {
        match self.state.data_of(node) {
            NodeData::Identifier(data) => {
                Some(tsrs2_binder::unescape_leading_underscores(&data.escaped_text).to_owned())
            }
            NodeData::JsxNamespacedName(data) => {
                let namespace = data.namespace.and_then(|node| self.identifier_name(node))?;
                let name = data.name.and_then(|node| self.identifier_name(node))?;
                Some(format!("{namespace}:{name}"))
            }
            _ => None,
        }
    }

    fn type_arguments(&mut self, nodes: Option<NodeArrayId>) -> CheckResult<String> {
        let Some(nodes) = nodes else {
            return Ok(String::new());
        };
        let nodes = self.state.binder.node_array(nodes).nodes.clone();
        let rendered = self.with_line_start(false, |printer| {
            printer.state.type_argument_nodes_text_slice(nodes)
        })?;
        Ok(format!("<{}>", rendered.join(", ")))
    }

    fn expression_list(
        &mut self,
        nodes: Vec<NodeId>,
        disallow_comma: bool,
    ) -> CheckResult<Option<String>> {
        let mut rendered = Vec::with_capacity(nodes.len());
        let first_at_line_start = self.state.slice_display_clone_at_line_start;
        for (index, node) in nodes.into_iter().enumerate() {
            let at_line_start = index == 0 && first_at_line_start;
            let text = self.with_line_start(at_line_start, |printer| {
                if disallow_comma {
                    printer.expression_for_disallowed_comma(node)
                } else {
                    printer.expression(node)
                }
            })?;
            let Some(text) = text else {
                return Ok(None);
            };
            rendered.push(text);
        }
        Ok(Some(rendered.join(", ")))
    }

    fn conditional_branch(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let needs_parentheses = self.is_comma_sequence(node);
        let child_at_line_start = if needs_parentheses {
            false
        } else {
            self.state.slice_display_clone_at_line_start
        };
        let Some(mut text) =
            self.with_line_start(child_at_line_start, |printer| printer.expression(node))?
        else {
            return Ok(None);
        };
        if needs_parentheses {
            text = format!("({text})");
        }
        Ok(Some(text))
    }

    fn expression_for_disallowed_comma(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let needs_parentheses = self.expression_precedence(node) <= PRECEDENCE_COMMA;
        let child_at_line_start = if needs_parentheses {
            false
        } else {
            self.state.slice_display_clone_at_line_start
        };
        let Some(mut text) =
            self.with_line_start(child_at_line_start, |printer| printer.expression(node))?
        else {
            return Ok(None);
        };
        if needs_parentheses {
            text = format!("({text})");
        }
        Ok(Some(text))
    }

    fn operand_of_prefix_unary(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let needs_parentheses = !self.is_unary_expression(node);
        let child_at_line_start = if needs_parentheses {
            false
        } else {
            self.state.slice_display_clone_at_line_start
        };
        let Some(mut text) =
            self.with_line_start(child_at_line_start, |printer| printer.expression(node))?
        else {
            return Ok(None);
        };
        if needs_parentheses {
            text = format!("({text})");
        }
        Ok(Some(text))
    }

    fn operand_of_postfix_unary(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let needs_parentheses = !self.is_left_hand_side_expression(node);
        let child_at_line_start = if needs_parentheses {
            false
        } else {
            self.state.slice_display_clone_at_line_start
        };
        let Some(mut text) =
            self.with_line_start(child_at_line_start, |printer| printer.expression(node))?
        else {
            return Ok(None);
        };
        if needs_parentheses {
            text = format!("({text})");
        }
        Ok(Some(text))
    }

    fn left_side_of_access(
        &mut self,
        node: NodeId,
        optional_chain: bool,
    ) -> CheckResult<Option<String>> {
        let needs_parentheses = !self.is_left_hand_side_expression(node)
            || (self.state.kind_of(self.skip_partially_emitted(node)) == SyntaxKind::NewExpression
                && !self.new_expression_has_arguments(node))
            || (!optional_chain && self.is_optional_chain(node));
        let child_at_line_start = if needs_parentheses {
            false
        } else {
            self.state.slice_display_clone_at_line_start
        };
        let Some(mut text) =
            self.with_line_start(child_at_line_start, |printer| printer.expression(node))?
        else {
            return Ok(None);
        };
        if needs_parentheses {
            text = format!("({text})");
        }
        Ok(Some(text))
    }

    fn expression_of_new(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let leftmost = self.leftmost_expression(node, true);
        let leftmost_kind = self.state.kind_of(leftmost);
        let needs_parentheses = leftmost_kind == SyntaxKind::CallExpression
            || (leftmost_kind == SyntaxKind::NewExpression
                && !self.new_expression_has_arguments(leftmost))
            || !self.is_left_hand_side_expression(node)
            || self.is_optional_chain(node);
        let child_at_line_start = if needs_parentheses {
            false
        } else {
            self.state.slice_display_clone_at_line_start
        };
        let Some(mut text) =
            self.with_line_start(child_at_line_start, |printer| printer.expression(node))?
        else {
            return Ok(None);
        };
        if needs_parentheses {
            text = format!("({text})");
        }
        Ok(Some(text))
    }

    /// tsc-port: binaryOperandNeedsParentheses @6.0.3
    /// tsc-hash: 7fdf522c085caae040c757af4fb1b4cd79b9b119347a48009c3c8177a316cce5
    /// tsc-span: _tsc.js:20329-20419
    fn binary_operand(
        &mut self,
        operator: SyntaxKind,
        operand: NodeId,
        is_left: bool,
        left_operand: Option<NodeId>,
    ) -> CheckResult<Option<String>> {
        let needs_parentheses =
            self.binary_operand_needs_parentheses(operator, operand, is_left, left_operand);
        let child_at_line_start = if needs_parentheses {
            false
        } else {
            self.state.slice_display_clone_at_line_start
        };
        let Some(mut text) =
            self.with_line_start(child_at_line_start, |printer| printer.expression(operand))?
        else {
            return Ok(None);
        };
        if needs_parentheses {
            text = format!("({text})");
        }
        Ok(Some(text))
    }

    fn binary_operand_needs_parentheses(
        &self,
        operator: SyntaxKind,
        operand: NodeId,
        is_left: bool,
        left_operand: Option<NodeId>,
    ) -> bool {
        let operand = self.skip_partially_emitted(operand);
        if let Some(operand_operator) = self.binary_operator(operand) {
            if mixing_binary_operators_requires_parentheses(operator, operand_operator) {
                return true;
            }
        }
        let operator_precedence = binary_operator_precedence(operator);
        let operator_associativity = binary_operator_associativity(operator);
        if !is_left
            && self.state.kind_of(operand) == SyntaxKind::ArrowFunction
            && operator_precedence > PRECEDENCE_ASSIGNMENT
        {
            return true;
        }
        let operand_precedence = self.expression_precedence(operand);
        if operand_precedence < operator_precedence {
            return !(operator_associativity == Associativity::Right
                && !is_left
                && self.state.kind_of(operand) == SyntaxKind::YieldExpression);
        }
        if operand_precedence > operator_precedence {
            return false;
        }
        if is_left {
            return operator_associativity == Associativity::Right;
        }
        if self.binary_operator(operand) == Some(operator) {
            if operator_has_associative_property(operator) {
                return false;
            }
            if operator == SyntaxKind::PlusToken {
                let left_kind = left_operand.and_then(|node| self.binary_plus_literal_kind(node));
                if left_kind.is_some() && left_kind == self.binary_plus_literal_kind(operand) {
                    return false;
                }
            }
        }
        self.expression_associativity(operand) == Associativity::Left
    }

    /// tsc-port: getExpressionPrecedence/getOperatorPrecedence @6.0.3
    /// tsc-hash: cdc9dac5af5a55f744b5dd4616441f37bbae41c1d6f3c58d4d0dcc9c63c044e0
    /// tsc-span: _tsc.js:16044-16142
    fn expression_precedence(&self, node: NodeId) -> i8 {
        let node = self.skip_partially_emitted(node);
        match self.state.kind_of(node) {
            SyntaxKind::CommaListExpression => PRECEDENCE_COMMA,
            SyntaxKind::SpreadElement => PRECEDENCE_SPREAD,
            SyntaxKind::YieldExpression => PRECEDENCE_YIELD,
            SyntaxKind::ConditionalExpression => PRECEDENCE_CONDITIONAL,
            SyntaxKind::BinaryExpression => self
                .binary_operator(node)
                .map(binary_operator_precedence)
                .unwrap_or(PRECEDENCE_INVALID),
            SyntaxKind::TypeAssertionExpression
            | SyntaxKind::NonNullExpression
            | SyntaxKind::PrefixUnaryExpression
            | SyntaxKind::TypeOfExpression
            | SyntaxKind::VoidExpression
            | SyntaxKind::DeleteExpression
            | SyntaxKind::AwaitExpression => PRECEDENCE_UNARY,
            SyntaxKind::PostfixUnaryExpression => PRECEDENCE_UPDATE,
            SyntaxKind::CallExpression => PRECEDENCE_LEFT_HAND_SIDE,
            SyntaxKind::NewExpression => {
                if self.new_expression_has_arguments(node) {
                    PRECEDENCE_MEMBER
                } else {
                    PRECEDENCE_LEFT_HAND_SIDE
                }
            }
            SyntaxKind::TaggedTemplateExpression
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::MetaProperty => PRECEDENCE_MEMBER,
            SyntaxKind::AsExpression | SyntaxKind::SatisfiesExpression => PRECEDENCE_RELATIONAL,
            SyntaxKind::ThisKeyword
            | SyntaxKind::SuperKeyword
            | SyntaxKind::Identifier
            | SyntaxKind::PrivateIdentifier
            | SyntaxKind::NullKeyword
            | SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::StringLiteral
            | SyntaxKind::ArrayLiteralExpression
            | SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::ClassExpression
            | SyntaxKind::RegularExpressionLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TemplateExpression
            | SyntaxKind::ParenthesizedExpression
            | SyntaxKind::OmittedExpression
            | SyntaxKind::JsxElement
            | SyntaxKind::JsxSelfClosingElement
            | SyntaxKind::JsxFragment => PRECEDENCE_PRIMARY,
            _ => PRECEDENCE_INVALID,
        }
    }

    fn expression_associativity(&self, node: NodeId) -> Associativity {
        let node = self.skip_partially_emitted(node);
        match self.state.kind_of(node) {
            SyntaxKind::NewExpression => {
                if self.new_expression_has_arguments(node) {
                    Associativity::Left
                } else {
                    Associativity::Right
                }
            }
            SyntaxKind::PrefixUnaryExpression
            | SyntaxKind::TypeOfExpression
            | SyntaxKind::VoidExpression
            | SyntaxKind::DeleteExpression
            | SyntaxKind::AwaitExpression
            | SyntaxKind::ConditionalExpression
            | SyntaxKind::YieldExpression => Associativity::Right,
            SyntaxKind::BinaryExpression => self
                .binary_operator(node)
                .map(binary_operator_associativity)
                .unwrap_or(Associativity::Left),
            _ => Associativity::Left,
        }
    }

    fn binary_operator(&self, node: NodeId) -> Option<SyntaxKind> {
        let NodeData::BinaryExpression(data) = self.state.data_of(node) else {
            return None;
        };
        data.operator_token.map(|token| self.state.kind_of(token))
    }

    fn binary_plus_literal_kind(&self, node: NodeId) -> Option<SyntaxKind> {
        let node = self.skip_partially_emitted(node);
        let kind = self.state.kind_of(node);
        if matches!(
            kind,
            SyntaxKind::NumericLiteral
                | SyntaxKind::BigIntLiteral
                | SyntaxKind::StringLiteral
                | SyntaxKind::RegularExpressionLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
        ) {
            return Some(kind);
        }
        if self.binary_operator(node) != Some(SyntaxKind::PlusToken) {
            return None;
        }
        let NodeData::BinaryExpression(data) = self.state.data_of(node) else {
            return None;
        };
        let left = self.binary_plus_literal_kind(data.left?);
        if left.is_some() && left == self.binary_plus_literal_kind(data.right?) {
            left
        } else {
            None
        }
    }

    fn is_comma_sequence(&self, node: NodeId) -> bool {
        let node = self.skip_partially_emitted(node);
        self.state.kind_of(node) == SyntaxKind::CommaListExpression
            || self.binary_operator(node) == Some(SyntaxKind::CommaToken)
    }

    fn is_left_hand_side_expression(&self, node: NodeId) -> bool {
        matches!(
            self.state.kind_of(self.skip_partially_emitted(node)),
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

    fn is_unary_expression(&self, node: NodeId) -> bool {
        matches!(
            self.state.kind_of(self.skip_partially_emitted(node)),
            SyntaxKind::PrefixUnaryExpression
                | SyntaxKind::PostfixUnaryExpression
                | SyntaxKind::DeleteExpression
                | SyntaxKind::TypeOfExpression
                | SyntaxKind::VoidExpression
                | SyntaxKind::AwaitExpression
                | SyntaxKind::TypeAssertionExpression
        ) || self.is_left_hand_side_expression(node)
    }

    fn new_expression_has_arguments(&self, node: NodeId) -> bool {
        let node = self.skip_partially_emitted(node);
        matches!(
            self.state.data_of(node),
            NodeData::NewExpression(data) if data.arguments.is_some()
        )
    }

    fn numeric_access_needs_second_dot(&self, node: NodeId) -> bool {
        let node = self.skip_partially_emitted(node);
        let NodeData::NumericLiteral(data) = self.state.data_of(node) else {
            return false;
        };
        let node = self.state.binder.source_of_node(node).arena.node(node);
        node.numeric_literal_flags & 448 == 0 && !data.text.contains(['.', 'e', 'E'])
    }

    fn is_optional_chain(&self, node: NodeId) -> bool {
        let node = self.skip_partially_emitted(node);
        let node = self.state.binder.source_of_node(node).arena.node(node);
        NodeFlags::from_bits(node.flags).intersects(NodeFlags::OPTIONAL_CHAIN)
    }

    fn leftmost_expression(&self, mut node: NodeId, stop_at_calls: bool) -> NodeId {
        loop {
            node = self.skip_partially_emitted(node);
            let next = match self.state.data_of(node) {
                NodeData::PostfixUnaryExpression(data) => data.operand,
                NodeData::BinaryExpression(data) => data.left,
                NodeData::ConditionalExpression(data) => data.condition,
                NodeData::TaggedTemplateExpression(data) => data.tag,
                NodeData::CallExpression(_) if stop_at_calls => None,
                NodeData::CallExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                NodeData::ElementAccessExpression(data) => data.expression,
                NodeData::PropertyAccessExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                _ => None,
            };
            let Some(next) = next else {
                return node;
            };
            node = next;
        }
    }

    fn skip_partially_emitted(&self, mut node: NodeId) -> NodeId {
        while let NodeData::PartiallyEmittedExpression(data) = self.state.data_of(node) {
            let Some(expression) = data.expression else {
                break;
            };
            node = expression;
        }
        node
    }

    fn identifier_name(&self, node: NodeId) -> Option<String> {
        match self.state.data_of(node) {
            NodeData::Identifier(data) => {
                Some(tsrs2_binder::unescape_leading_underscores(&data.escaped_text).to_owned())
            }
            NodeData::PrivateIdentifier(data) => Some(data.text.clone()),
            _ => None,
        }
    }

    fn node_array(&self, nodes: Option<NodeArrayId>) -> (Vec<NodeId>, bool) {
        let Some(nodes) = nodes else {
            return (Vec::new(), false);
        };
        let nodes = self.state.binder.node_array(nodes);
        (nodes.nodes.clone(), nodes.has_trailing_comma)
    }

    fn with_increased_indent<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> CheckResult<T>,
    ) -> CheckResult<T> {
        self.with_restored_indent(|printer| {
            printer.state.slice_display_clone_indent += 1;
            operation(printer)
        })
    }

    fn with_restored_indent<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> CheckResult<T>,
    ) -> CheckResult<T> {
        let saved = self.state.slice_display_clone_indent;
        let result = operation(self);
        self.state.slice_display_clone_indent = saved;
        result
    }

    fn with_line_start<T>(
        &mut self,
        at_line_start: bool,
        operation: impl FnOnce(&mut Self) -> CheckResult<T>,
    ) -> CheckResult<T> {
        let saved = self.state.slice_display_clone_at_line_start;
        self.state.slice_display_clone_at_line_start = at_line_start;
        let result = operation(self);
        self.state.slice_display_clone_at_line_start = saved;
        result
    }

    fn node_is_multi_line(&self, node: NodeId) -> bool {
        self.state
            .binder
            .source_of_node(node)
            .arena
            .node(node)
            .multi_line
            == Some(true)
    }

    /// tsc-port: getLeadingLineTerminatorCount @6.0.3 for parsed
    /// PreserveLines expression lists.
    /// tsc-hash: 893244ddd50971f9938c07f3bb0b10b520dfbf70880816b69aa4b00cc1384819
    /// tsc-span: _tsc.js:120268-120300
    fn list_has_leading_line(&self, parent: NodeId, first: NodeId, prefer_new_line: bool) -> bool {
        if prefer_new_line {
            return true;
        }
        let parent_source = self.state.binder.source_of_node(parent);
        let first_source = self.state.binder.source_of_node(first);
        if !std::ptr::eq(parent_source, first_source) {
            return false;
        }
        match (
            self.state.display_clone_start_line(parent),
            self.state.display_clone_start_line(first),
        ) {
            (Some(parent), Some(first)) => parent != first,
            _ => false,
        }
    }

    /// tsc-port: getSeparatingLineTerminatorCount @6.0.3 for parsed
    /// PreserveLines expression lists.
    /// tsc-hash: 78d63da04f114ae40f8ad9f012131e94a83000cf268d393c5608372aab734539
    /// tsc-span: _tsc.js:120301-120329
    fn list_has_separating_line(
        &self,
        previous: NodeId,
        next: NodeId,
        prefer_new_line: bool,
    ) -> bool {
        let previous_source = self.state.binder.source_of_node(previous);
        let next_source = self.state.binder.source_of_node(next);
        if std::ptr::eq(previous_source, next_source) {
            let previous_record = previous_source.arena.node(previous);
            let next_record = next_source.arena.node(next);
            if previous_record.parent.is_some()
                && previous_record.parent == next_record.parent
                && previous_record.end != u32::MAX
                && next_record.pos != u32::MAX
            {
                if let (Some(previous), Some(next)) = (
                    self.state.display_clone_end_line(previous),
                    self.state.display_clone_start_line(next),
                ) {
                    return previous != next;
                }
            }
        }
        prefer_new_line
    }

    /// tsc-port: getClosingLineTerminatorCount @6.0.3 for parsed
    /// PreserveLines expression lists.
    /// tsc-hash: 30b0be7e1586d76d08caeaa3f4605323147137e9c73a0bd825dd3c92881635dc
    /// tsc-span: _tsc.js:120330-120360
    fn list_has_closing_line(&self, parent: NodeId, last: NodeId, prefer_new_line: bool) -> bool {
        if prefer_new_line {
            return true;
        }
        let parent_source = self.state.binder.source_of_node(parent);
        let last_source = self.state.binder.source_of_node(last);
        if !std::ptr::eq(parent_source, last_source) {
            return false;
        }
        match (
            self.state.display_clone_end_line(parent),
            self.state.display_clone_end_line(last),
        ) {
            (Some(parent), Some(last)) => parent != last,
            _ => false,
        }
    }

    /// tsc-port: getLinesBetweenNodes @6.0.3 with the typeToString
    /// printer's default `preserveSourceNewlines = false`.
    /// tsc-hash: c5d47a80179fc3b56a4cf499a9133abaec3676a2710aeda024119b26a477524b
    /// tsc-span: _tsc.js:120408-120432
    fn nodes_have_line_between(&self, previous: NodeId, next: NodeId) -> bool {
        let previous_source = self.state.binder.source_of_node(previous);
        let next_source = self.state.binder.source_of_node(next);
        if !std::ptr::eq(previous_source, next_source) {
            return false;
        }
        matches!(
            (
                self.state.display_clone_end_line(previous),
                self.state.display_clone_start_line(next),
            ),
            (Some(previous), Some(next)) if previous != next
        )
    }

    /// An ordinary property-access dot is a factory token with
    /// `pos = expression.end` and `end = name.pos`. Recreate the two
    /// getLinesBetweenNodes probes without manufacturing a Rust AST node.
    fn synthetic_property_dot_lines(&self, expression: NodeId, name: NodeId) -> (bool, bool) {
        let expression_source = self.state.binder.source_of_node(expression);
        let name_source = self.state.binder.source_of_node(name);
        if !std::ptr::eq(expression_source, name_source) {
            return (false, false);
        }
        let expression_record = expression_source.arena.node(expression);
        let name_record = name_source.arena.node(name);
        if expression_record.end == u32::MAX
            || name_record.pos == u32::MAX
            || expression_record.end as usize > expression_source.text.len()
            || name_record.pos as usize > name_source.text.len()
        {
            return (false, false);
        }
        let dot_start =
            tsrs2_syntax::skip_trivia(&expression_source.text, expression_record.end as usize);
        let line_before_dot = match (
            self.state.display_clone_end_line(expression),
            display_clone_line_of_byte(expression_source, dot_start),
        ) {
            (Some(expression), Some(dot)) => expression != dot,
            _ => false,
        };
        let line_after_dot = match (
            display_clone_line_of_byte(name_source, name_record.pos as usize),
            self.state.display_clone_start_line(name),
        ) {
            (Some(dot), Some(name)) => dot != name,
            _ => false,
        };
        (line_before_dot, line_after_dot)
    }

    fn node_raw_range_is_multi_line(&self, node: NodeId) -> bool {
        let source = self.state.binder.source_of_node(node);
        let record = source.arena.node(node);
        if record.pos == u32::MAX
            || record.end == u32::MAX
            || record.pos as usize > source.text.len()
            || record.end as usize > source.text.len()
        {
            return false;
        }
        match (
            display_clone_line_of_byte(source, record.pos as usize),
            display_clone_line_of_byte(source, record.end as usize),
        ) {
            (Some(start), Some(end)) => start != end,
            _ => false,
        }
    }

    fn append_jsx_write(&self, text: &mut String, fragment: &str, extra_indent: usize) {
        if !fragment.is_empty() && text_ends_at_line_start(text) {
            text.push_str(&"    ".repeat(self.state.slice_display_clone_indent + extra_indent));
        }
        text.push_str(fragment);
    }
}

fn display_clone_line_of_byte(source: &tsrs2_syntax::SourceFile, byte: usize) -> Option<usize> {
    let utf16 = *source.line_map.byte_to_utf16.get(byte)?;
    Some(match source.line_map.line_starts.binary_search(&utf16) {
        Ok(line) => line,
        Err(insertion) => insertion.saturating_sub(1),
    })
}

fn text_ends_at_line_start(text: &str) -> bool {
    text.chars()
        .next_back()
        .is_some_and(tsrs2_syntax::is_line_break)
}

fn binary_operator_precedence(operator: SyntaxKind) -> i8 {
    match operator {
        SyntaxKind::CommaToken => PRECEDENCE_COMMA,
        SyntaxKind::EqualsToken
        | SyntaxKind::PlusEqualsToken
        | SyntaxKind::MinusEqualsToken
        | SyntaxKind::AsteriskEqualsToken
        | SyntaxKind::AsteriskAsteriskEqualsToken
        | SyntaxKind::SlashEqualsToken
        | SyntaxKind::PercentEqualsToken
        | SyntaxKind::LessThanLessThanEqualsToken
        | SyntaxKind::GreaterThanGreaterThanEqualsToken
        | SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken
        | SyntaxKind::AmpersandEqualsToken
        | SyntaxKind::CaretEqualsToken
        | SyntaxKind::BarEqualsToken
        | SyntaxKind::BarBarEqualsToken
        | SyntaxKind::AmpersandAmpersandEqualsToken
        | SyntaxKind::QuestionQuestionEqualsToken => PRECEDENCE_ASSIGNMENT,
        SyntaxKind::QuestionQuestionToken | SyntaxKind::BarBarToken => 5,
        SyntaxKind::AmpersandAmpersandToken => 6,
        SyntaxKind::BarToken => 7,
        SyntaxKind::CaretToken => 8,
        SyntaxKind::AmpersandToken => 9,
        SyntaxKind::EqualsEqualsToken
        | SyntaxKind::ExclamationEqualsToken
        | SyntaxKind::EqualsEqualsEqualsToken
        | SyntaxKind::ExclamationEqualsEqualsToken => 10,
        SyntaxKind::LessThanToken
        | SyntaxKind::GreaterThanToken
        | SyntaxKind::LessThanEqualsToken
        | SyntaxKind::GreaterThanEqualsToken
        | SyntaxKind::InstanceOfKeyword
        | SyntaxKind::InKeyword
        | SyntaxKind::AsKeyword
        | SyntaxKind::SatisfiesKeyword => PRECEDENCE_RELATIONAL,
        SyntaxKind::LessThanLessThanToken
        | SyntaxKind::GreaterThanGreaterThanToken
        | SyntaxKind::GreaterThanGreaterThanGreaterThanToken => 12,
        SyntaxKind::PlusToken | SyntaxKind::MinusToken => 13,
        SyntaxKind::AsteriskToken | SyntaxKind::SlashToken | SyntaxKind::PercentToken => 14,
        SyntaxKind::AsteriskAsteriskToken => 15,
        _ => PRECEDENCE_INVALID,
    }
}

fn binary_operator_associativity(operator: SyntaxKind) -> Associativity {
    if matches!(
        operator,
        SyntaxKind::AsteriskAsteriskToken
            | SyntaxKind::EqualsToken
            | SyntaxKind::PlusEqualsToken
            | SyntaxKind::MinusEqualsToken
            | SyntaxKind::AsteriskEqualsToken
            | SyntaxKind::AsteriskAsteriskEqualsToken
            | SyntaxKind::SlashEqualsToken
            | SyntaxKind::PercentEqualsToken
            | SyntaxKind::LessThanLessThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken
            | SyntaxKind::AmpersandEqualsToken
            | SyntaxKind::CaretEqualsToken
            | SyntaxKind::BarEqualsToken
            | SyntaxKind::BarBarEqualsToken
            | SyntaxKind::AmpersandAmpersandEqualsToken
            | SyntaxKind::QuestionQuestionEqualsToken
    ) {
        Associativity::Right
    } else {
        Associativity::Left
    }
}

fn mixing_binary_operators_requires_parentheses(a: SyntaxKind, b: SyntaxKind) -> bool {
    (a == SyntaxKind::QuestionQuestionToken
        && matches!(
            b,
            SyntaxKind::AmpersandAmpersandToken | SyntaxKind::BarBarToken
        ))
        || (b == SyntaxKind::QuestionQuestionToken
            && matches!(
                a,
                SyntaxKind::AmpersandAmpersandToken | SyntaxKind::BarBarToken
            ))
}

fn operator_has_associative_property(operator: SyntaxKind) -> bool {
    matches!(
        operator,
        SyntaxKind::AsteriskToken
            | SyntaxKind::BarToken
            | SyntaxKind::AmpersandToken
            | SyntaxKind::CaretToken
            | SyntaxKind::CommaToken
    )
}

fn quoted_string(text: &str) -> String {
    let units = text.encode_utf16().collect::<Vec<_>>();
    let mut escaped = String::with_capacity(text.len());
    for (index, &unit) in units.iter().enumerate() {
        match unit {
            0 if units
                .get(index + 1)
                .is_some_and(|next| (b'0' as u16..=b'9' as u16).contains(next)) =>
            {
                escaped.push_str("\\x00");
            }
            0 => escaped.push_str("\\0"),
            0x0008 => escaped.push_str("\\b"),
            0x0009 => escaped.push_str("\\t"),
            0x000B => escaped.push_str("\\v"),
            0x000C => escaped.push_str("\\f"),
            0x000D => escaped.push_str("\\r"),
            0x000A => escaped.push_str("\\n"),
            0x005C => escaped.push_str("\\\\"),
            0x0022 => escaped.push_str("\\\""),
            0x2028 => escaped.push_str("\\u2028"),
            0x2029 => escaped.push_str("\\u2029"),
            0x0085 => escaped.push_str("\\u0085"),
            0x0001..=0x001F | 0x0080..=0xFFFF => {
                escaped.push_str(&encode_utf16_escape_sequence(unit));
            }
            _ => escaped.push(
                char::from_u32(u32::from(unit)).expect("ASCII UTF-16 unit is a scalar value"),
            ),
        }
    }
    format!("\"{escaped}\"")
}

fn quoted_jsx_attribute(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\0' => escaped.push_str("&#0;"),
            '"' => escaped.push_str("&quot;"),
            '\u{0001}'..='\u{001F}' | '\u{0085}' | '\u{2028}' | '\u{2029}' => {
                escaped.push_str(&format!("&#x{:X};", character as u32));
            }
            _ => escaped.push(character),
        }
    }
    format!("\"{escaped}\"")
}

fn template_text_raw(text: &str) -> String {
    let units = text.encode_utf16().collect::<Vec<_>>();
    let mut out = String::with_capacity(text.len());
    let mut index = 0usize;
    while index < units.len() {
        let unit = units[index];
        match unit {
            0x000D if units.get(index + 1) == Some(&0x000A) => {
                out.push_str("\\r\\n");
                index += 2;
                continue;
            }
            0x005C => out.push_str("\\\\"),
            0x0060 => out.push_str("\\`"),
            0 if units
                .get(index + 1)
                .is_some_and(|next| (b'0' as u16..=b'9' as u16).contains(next)) =>
            {
                out.push_str("\\x00");
            }
            0 => out.push_str("\\0"),
            0x0009 => out.push_str("\\t"),
            0x0008 => out.push_str("\\b"),
            0x000B => out.push_str("\\v"),
            0x000C => out.push_str("\\f"),
            0x000D => out.push_str("\\r"),
            0x2028 => out.push_str("\\u2028"),
            0x2029 => out.push_str("\\u2029"),
            0x0085 => out.push_str("\\u0085"),
            0x0000..=0x001F if unit != 0x000A => {
                out.push_str(&encode_utf16_escape_sequence(unit));
            }
            0x0080..=0xFFFF => out.push_str(&encode_utf16_escape_sequence(unit)),
            _ => out.push(
                char::from_u32(u32::from(unit)).expect("ASCII UTF-16 unit is a scalar value"),
            ),
        }
        index += 1;
    }
    out.replace("${", "\\${")
}

fn encode_utf16_escape_sequence(unit: u16) -> String {
    format!("\\u{unit:04X}")
}
