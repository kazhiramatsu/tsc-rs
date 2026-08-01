//! Body-bearing expression and statement closure for annotation reuse.
//!
//! This is the body half of `display_clone`. It deliberately prints a
//! clone face without reading source text or emitting comments/source maps.
//! Parsed source ranges and line-layout bits are still consulted for the
//! standard printer's line/indent probes; the display writer drops newline
//! bytes but retains four-space indentation. Declaration emit transforms are
//! not consulted.
//! Returning `None` asks the owning reused-TypeNode boundary to rebuild the
//! enclosing type semantically.

use tsc_syntax::nodes::{
    DoStatementData, ForInStatementData, ForOfStatementData, ForStatementData, IfStatementData,
    MethodDeclarationData, SwitchStatementData, TryStatementData, WhileStatementData,
    WithStatementData,
};
use tsc_syntax::{NodeArrayId, NodeData, NodeId, SyntaxKind};
use tsc_types::NodeFlags;

use crate::state::{CheckResult, CheckerState};

struct DisplayCloneBodyPrinter<'state, 'program> {
    state: &'state mut CheckerState<'program>,
}

impl<'program> CheckerState<'program> {
    /// tsrs-native: compartment adapter into the exact standard-printer
    /// closure ledgered by display_clone_expression_text_at_line_start.
    ///
    /// The expression half calls this for FunctionExpression, ArrowFunction,
    /// ClassExpression, and object-literal method/accessor members. The same
    /// dispatcher is used recursively for blocks, statements, declarations,
    /// and class members.
    pub(crate) fn display_clone_body_expression_text(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<String>> {
        DisplayCloneBodyPrinter { state: self }.node(node)
    }

    /// tsrs-native: line-start state adapter into the body-printer compartment.
    pub(crate) fn display_clone_body_node_text_at_line_start(
        &mut self,
        node: NodeId,
        at_line_start: bool,
    ) -> CheckResult<Option<String>> {
        let saved_indent = self.slice_display_clone_indent;
        let saved = self.slice_display_clone_at_line_start;
        self.slice_display_clone_at_line_start = at_line_start;
        let result = DisplayCloneBodyPrinter { state: self }.node(node);
        self.slice_display_clone_indent = saved_indent;
        self.slice_display_clone_at_line_start = saved;
        result
    }

    /// tsrs-native: computed-name adapter into the body-printer compartment.
    pub(crate) fn display_clone_computed_property_expression_text(
        &mut self,
        expression: NodeId,
    ) -> CheckResult<Option<String>> {
        let saved = self.slice_display_clone_at_line_start;
        self.slice_display_clone_at_line_start = false;
        let result =
            DisplayCloneBodyPrinter { state: self }.computed_property_expression(expression);
        self.slice_display_clone_at_line_start = saved;
        result
    }

    /// tsrs-native: Rust node-list adapter into the body-printer compartment.
    pub(crate) fn display_clone_parameter_nodes_text(
        &mut self,
        nodes: Vec<NodeId>,
    ) -> CheckResult<Option<String>> {
        let saved = self.slice_display_clone_at_line_start;
        self.slice_display_clone_at_line_start = false;
        let result = DisplayCloneBodyPrinter { state: self }.parameter_nodes_text(nodes);
        self.slice_display_clone_at_line_start = saved;
        result
    }

    /// tsrs-native: function-body adapter into the body-printer compartment.
    pub(crate) fn display_clone_function_body_text(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<String>> {
        let saved_indent = self.slice_display_clone_indent;
        let saved_line_start = self.slice_display_clone_at_line_start;
        self.slice_display_clone_at_line_start = false;
        let result = DisplayCloneBodyPrinter { state: self }.function_body_block(node);
        self.slice_display_clone_indent = saved_indent;
        self.slice_display_clone_at_line_start = saved_line_start;
        result
    }
}

impl DisplayCloneBodyPrinter<'_, '_> {
    fn node(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        if self.node_enters_reuse_scope(node) {
            return self.with_reuse_scope(node, |printer| printer.node_worker(node));
        }
        self.node_worker(node)
    }

    fn node_worker(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        match self.state.kind_of(node) {
            SyntaxKind::FunctionExpression => self.function_expression(node),
            SyntaxKind::ArrowFunction => self.arrow_function(node),
            SyntaxKind::ClassExpression => self.class_expression(node),

            SyntaxKind::Block => self.block(node),
            SyntaxKind::EmptyStatement
            | SyntaxKind::VariableStatement
            | SyntaxKind::ExpressionStatement
            | SyntaxKind::IfStatement
            | SyntaxKind::DoStatement
            | SyntaxKind::WhileStatement
            | SyntaxKind::ForStatement
            | SyntaxKind::ForInStatement
            | SyntaxKind::ForOfStatement
            | SyntaxKind::ContinueStatement
            | SyntaxKind::BreakStatement
            | SyntaxKind::ReturnStatement
            | SyntaxKind::WithStatement
            | SyntaxKind::SwitchStatement
            | SyntaxKind::LabeledStatement
            | SyntaxKind::ThrowStatement
            | SyntaxKind::TryStatement
            | SyntaxKind::DebuggerStatement
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportDeclaration
            | SyntaxKind::ExportAssignment
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::NamespaceExportDeclaration
            | SyntaxKind::MissingDeclaration => self.statement(node),

            SyntaxKind::ModuleBlock => self.state.display_clone_module_statement_text(node),

            SyntaxKind::VariableDeclarationList => self.variable_declaration_list(node),
            SyntaxKind::VariableDeclaration => self.variable_declaration(node),
            SyntaxKind::EnumMember => self.enum_member(node),
            SyntaxKind::CaseBlock => self.case_block(node),
            SyntaxKind::CaseClause | SyntaxKind::DefaultClause => self.case_or_default_clause(node),
            SyntaxKind::CatchClause => self.catch_clause(node),
            SyntaxKind::HeritageClause => self.heritage_clause(node),

            SyntaxKind::PropertyDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::Constructor
            | SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::IndexSignature
            | SyntaxKind::SemicolonClassElement => self.class_or_object_member(node),

            _ => Ok(None),
        }
    }

    fn expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        self.expression_at_line_start(node, false)
    }

    fn expression_at_line_start(
        &mut self,
        node: NodeId,
        at_line_start: bool,
    ) -> CheckResult<Option<String>> {
        self.state
            .display_clone_expression_text_at_line_start(node, at_line_start)
    }

    fn function_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::FunctionExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        self.function_like(
            data.modifiers,
            data.name,
            data.asterisk_token.is_some(),
            data.type_parameters,
            data.parameters,
            data.r#type,
            data.body,
        )
    }

    fn function_declaration(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::FunctionDeclaration(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        self.function_like(
            data.modifiers,
            data.name,
            data.asterisk_token.is_some(),
            data.type_parameters,
            data.parameters,
            data.r#type,
            data.body,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn function_like(
        &mut self,
        modifiers: Option<NodeArrayId>,
        name: Option<NodeId>,
        generator: bool,
        type_parameters: Option<NodeArrayId>,
        parameters: Option<NodeArrayId>,
        return_type: Option<NodeId>,
        body: Option<NodeId>,
    ) -> CheckResult<Option<String>> {
        let Some(modifiers) = self.modifiers(modifiers)? else {
            return Ok(None);
        };
        let mut text = modifiers;
        text.push_str("function");
        if generator {
            text.push('*');
        }
        text.push(' ');
        if let Some(name) = name {
            let Some(name) = self.identifier_name(name) else {
                return Ok(None);
            };
            text.push_str(&name);
        }
        let Some(type_parameters) = self.type_parameter_nodes_text(type_parameters, false)? else {
            return Ok(None);
        };
        text.push_str(&type_parameters);
        let Some(parameters) = self.parameter_nodes_text(self.nodes(parameters))? else {
            return Ok(None);
        };
        text.push('(');
        text.push_str(&parameters);
        text.push(')');
        text.push_str(&self.return_type_annotation(return_type)?);
        match body {
            Some(body) => {
                let Some(body) =
                    self.with_line_start(false, |printer| printer.function_body_block(body))?
                else {
                    return Ok(None);
                };
                text.push(' ');
                text.push_str(&body);
            }
            None => text.push(';'),
        }
        Ok(Some(text))
    }

    fn arrow_function(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ArrowFunction(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(mut text) = self.modifiers(data.modifiers)? else {
            return Ok(None);
        };
        let Some(type_parameters) = self.type_parameter_nodes_text(data.type_parameters, true)?
        else {
            return Ok(None);
        };
        text.push_str(&type_parameters);
        let Some(parameters) = self.parameter_nodes_text(self.nodes(data.parameters))? else {
            return Ok(None);
        };
        text.push('(');
        text.push_str(&parameters);
        text.push(')');
        text.push_str(&self.return_type_annotation(data.r#type)?);
        text.push_str(" => ");
        let Some(body) = data.body else {
            return Ok(None);
        };
        if self.state.kind_of(body) == SyntaxKind::Block {
            let Some(body) =
                self.with_line_start(false, |printer| printer.function_body_block(body))?
            else {
                return Ok(None);
            };
            text.push_str(&body);
        } else {
            let Some(mut body_text) = self.expression(body)? else {
                return Ok(None);
            };
            if self.arrow_body_needs_parentheses(body) {
                body_text = format!("({body_text})");
            }
            text.push_str(&body_text);
        }
        Ok(Some(text))
    }

    fn class_expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ClassExpression(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        self.class_like(
            data.modifiers,
            data.name,
            data.type_parameters,
            data.heritage_clauses,
            data.members,
        )
    }

    fn class_declaration(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ClassDeclaration(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        self.class_like(
            data.modifiers,
            data.name,
            data.type_parameters,
            data.heritage_clauses,
            data.members,
        )
    }

    fn class_like(
        &mut self,
        modifiers: Option<NodeArrayId>,
        name: Option<NodeId>,
        type_parameters: Option<NodeArrayId>,
        heritage_clauses: Option<NodeArrayId>,
        members: Option<NodeArrayId>,
    ) -> CheckResult<Option<String>> {
        let Some(mut text) = self.decorated_modifiers(modifiers)? else {
            return Ok(None);
        };
        text.push_str("class");
        if let Some(name) = name {
            let Some(name) = self.identifier_name(name) else {
                return Ok(None);
            };
            text.push(' ');
            text.push_str(&name);
        }
        let Some(type_parameters) = self.type_parameter_nodes_text(type_parameters, false)? else {
            return Ok(None);
        };
        text.push_str(&type_parameters);
        for heritage in self.nodes(heritage_clauses) {
            let Some(heritage) = self.heritage_clause(heritage)? else {
                return Ok(None);
            };
            text.push_str(&heritage);
        }
        let members = self.nodes(members);
        if members.is_empty() {
            text.push(' ');
            text.push_str(&self.empty_multiline_braces());
            return Ok(Some(text));
        }
        let mut retained = Vec::with_capacity(members.len());
        for member in members {
            if self.named_declaration_is_removed(member)? {
                continue;
            }
            retained.push(member);
        }
        if retained.is_empty() {
            text.push(' ');
            text.push_str(&self.empty_multiline_braces());
            return Ok(Some(text));
        }
        let rendered = self.with_increased_indent(|printer| {
            let mut rendered = String::new();
            for member in retained {
                let Some(member) = printer.with_line_start(true, |printer| printer.node(member))?
                else {
                    return Ok(None);
                };
                if member.is_empty() {
                    continue;
                }
                rendered.push_str(&printer.indent_text());
                rendered.push_str(&member);
            }
            Ok(Some(rendered))
        })?;
        let Some(rendered) = rendered else {
            return Ok(None);
        };
        text.push_str(" {");
        text.push_str(&rendered);
        text.push_str(&self.indent_text());
        text.push('}');
        Ok(Some(text))
    }

    fn block(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::Block(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let statements = self.nodes(data.statements);
        if statements.is_empty() {
            let same_line = match (
                self.state.display_clone_start_line(node),
                self.state.display_clone_end_line(node),
            ) {
                (Some(start), Some(end)) => start == end,
                _ => true,
            };
            return Ok(Some(if !self.node_is_multi_line(node) && same_line {
                "{ }".to_owned()
            } else {
                format!("{{{}}}", self.indent_text())
            }));
        }
        let rendered = self.with_increased_indent(|printer| {
            let mut rendered = String::new();
            for statement in statements {
                let Some(statement) =
                    printer.with_line_start(true, |printer| printer.node(statement))?
                else {
                    return Ok(None);
                };
                if statement.is_empty() {
                    continue;
                }
                rendered.push_str(&printer.indent_text());
                rendered.push_str(&statement);
            }
            Ok(Some(rendered))
        })?;
        let Some(rendered) = rendered else {
            return Ok(None);
        };
        Ok(Some(format!("{{{rendered}{}}}", self.indent_text())))
    }

    fn function_body_block(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::Block(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let statements = self.nodes(data.statements);
        if self.function_body_is_single_line(node) {
            if statements.is_empty() {
                return Ok(Some("{ }".to_owned()));
            }
            let mut rendered = Vec::with_capacity(statements.len());
            for statement in statements {
                let Some(statement) =
                    self.with_line_start(false, |printer| printer.node(statement))?
                else {
                    return Ok(None);
                };
                rendered.push(statement);
            }
            return Ok(Some(format!("{{ {} }}", rendered.join(" "))));
        }

        if statements.is_empty() {
            return Ok(Some(format!("{{{}}}", self.indent_text())));
        }
        let rendered = self.with_increased_indent(|printer| {
            let mut rendered = String::new();
            for statement in statements {
                let Some(statement) =
                    printer.with_line_start(true, |printer| printer.node(statement))?
                else {
                    return Ok(None);
                };
                if statement.is_empty() {
                    continue;
                }
                rendered.push_str(&printer.indent_text());
                rendered.push_str(&statement);
            }
            Ok(Some(rendered))
        })?;
        let Some(rendered) = rendered else {
            return Ok(None);
        };
        Ok(Some(format!("{{{rendered}{}}}", self.indent_text())))
    }

    fn statement(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        if matches!(
            self.state.kind_of(node),
            SyntaxKind::ModuleDeclaration
                | SyntaxKind::ImportEqualsDeclaration
                | SyntaxKind::ImportDeclaration
                | SyntaxKind::ExportAssignment
                | SyntaxKind::ExportDeclaration
                | SyntaxKind::NamespaceExportDeclaration
                | SyntaxKind::MissingDeclaration
        ) {
            return self.state.display_clone_module_statement_text(node);
        }
        match self.state.data_of(node).clone() {
            NodeData::Block(_) => self.block(node),
            NodeData::EmptyStatement(_) => Ok(Some(";".to_owned())),
            NodeData::VariableStatement(data) => {
                let Some(mut text) = self.modifiers(data.modifiers)? else {
                    return Ok(None);
                };
                let Some(list) = data.declaration_list else {
                    return Ok(None);
                };
                let Some(list) = self.variable_declaration_list(list)? else {
                    return Ok(None);
                };
                text.push_str(&list);
                text.push(';');
                Ok(Some(text))
            }
            NodeData::ExpressionStatement(data) => {
                let Some(expression) = data.expression else {
                    return Ok(None);
                };
                if self.expression_statement_needs_callee_recovery(expression) {
                    return Ok(None);
                }
                let needs_parentheses = self.expression_statement_needs_parentheses(expression);
                let child_at_line_start = if needs_parentheses {
                    false
                } else {
                    self.state.slice_display_clone_at_line_start
                };
                let Some(mut text) =
                    self.expression_at_line_start(expression, child_at_line_start)?
                else {
                    return Ok(None);
                };
                if needs_parentheses {
                    text = format!("({text})");
                }
                text.push(';');
                Ok(Some(text))
            }
            NodeData::IfStatement(data) => self.if_statement(data),
            NodeData::DoStatement(data) => self.do_statement(data),
            NodeData::WhileStatement(data) => self.while_statement(data),
            NodeData::ForStatement(data) => self.for_statement(data),
            NodeData::ForInStatement(data) => self.for_in_statement(data),
            NodeData::ForOfStatement(data) => self.for_of_statement(data),
            NodeData::ContinueStatement(data) => self.jump_statement("continue", data.label),
            NodeData::BreakStatement(data) => self.jump_statement("break", data.label),
            NodeData::ReturnStatement(data) => {
                self.expression_statement_with_keyword("return", data.expression)
            }
            NodeData::WithStatement(data) => self.with_statement(data),
            NodeData::SwitchStatement(data) => self.switch_statement(data),
            NodeData::LabeledStatement(data) => {
                let Some(label) = data.label.and_then(|label| self.identifier_name(label)) else {
                    return Ok(None);
                };
                let Some(statement) = data.statement else {
                    return Ok(None);
                };
                let Some(statement) =
                    self.with_line_start(false, |printer| printer.node(statement))?
                else {
                    return Ok(None);
                };
                Ok(Some(format!("{label}: {statement}")))
            }
            NodeData::ThrowStatement(data) => match data.expression {
                Some(expression) => {
                    self.expression_statement_with_keyword("throw", Some(expression))
                }
                None => Ok(None),
            },
            NodeData::TryStatement(data) => self.try_statement(data),
            NodeData::DebuggerStatement(_) => Ok(Some("debugger;".to_owned())),
            NodeData::FunctionDeclaration(_) => self.function_declaration(node),
            NodeData::ClassDeclaration(_) => self.class_declaration(node),
            NodeData::InterfaceDeclaration(_) => self.interface_declaration(node),
            NodeData::TypeAliasDeclaration(_) => self.type_alias_declaration(node),
            NodeData::EnumDeclaration(_) => self.enum_declaration(node),
            _ => Ok(None),
        }
    }

    fn interface_declaration(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::InterfaceDeclaration(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(mut text) = self.modifiers(data.modifiers)? else {
            return Ok(None);
        };
        let Some(name) = data.name.and_then(|name| self.identifier_name(name)) else {
            return Ok(None);
        };
        text.push_str("interface ");
        text.push_str(&name);
        let Some(type_parameters) = self.type_parameter_nodes_text(data.type_parameters, false)?
        else {
            return Ok(None);
        };
        text.push_str(&type_parameters);
        for heritage in self.nodes(data.heritage_clauses) {
            let Some(heritage) = self.heritage_clause(heritage)? else {
                return Ok(None);
            };
            text.push_str(&heritage);
        }

        let members = self.nodes(data.members);
        if members.is_empty() {
            text.push(' ');
            text.push_str(&self.empty_multiline_braces());
            return Ok(Some(text));
        }
        let rendered = self.with_increased_indent(|printer| {
            let mut rendered = String::new();
            for member_node in members {
                if printer.named_declaration_is_removed(member_node)? {
                    continue;
                }
                let Some(member) = printer
                    .with_line_start(true, |printer| printer.type_member_entry(member_node))?
                else {
                    return Ok(None);
                };
                if member.is_empty() {
                    continue;
                }
                rendered.push_str(&printer.indent_text());
                rendered.push_str(&member);
                if !printer.type_member_has_body(member_node) {
                    rendered.push(';');
                }
            }
            Ok(Some(rendered))
        })?;
        let Some(rendered) = rendered else {
            return Ok(None);
        };
        if rendered.is_empty() {
            text.push(' ');
            text.push_str(&self.empty_multiline_braces());
        } else {
            text.push_str(" {");
            text.push_str(&rendered);
            text.push_str(&self.indent_text());
            text.push('}');
        }
        Ok(Some(text))
    }

    fn type_member(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        match self.state.data_of(node).clone() {
            NodeData::PropertySignature(data) => {
                let Some(mut text) = self.modifiers(data.modifiers)? else {
                    return Ok(None);
                };
                let Some(name) = data.name else {
                    return Ok(None);
                };
                text.push_str(&self.member_name_text(name)?);
                if data.question_token.is_some() {
                    text.push('?');
                }
                if let Some(ty) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text(ty)?);
                } else if data.initializer.is_none() {
                    text.push_str(": any");
                }
                Ok(Some(text))
            }
            NodeData::MethodSignature(data) => {
                let Some(mut text) = self.modifiers(data.modifiers)? else {
                    return Ok(None);
                };
                let Some(name) = data.name else {
                    return Ok(None);
                };
                text.push_str(&self.member_name_text(name)?);
                if data.question_token.is_some() {
                    text.push('?');
                }
                let Some(type_parameters) =
                    self.type_parameter_nodes_text(data.type_parameters, false)?
                else {
                    return Ok(None);
                };
                text.push_str(&type_parameters);
                let Some(parameters) = self.parameter_nodes_text(self.nodes(data.parameters))?
                else {
                    return Ok(None);
                };
                text.push('(');
                text.push_str(&parameters);
                text.push(')');
                text.push_str(&self.return_type_annotation(data.r#type)?);
                Ok(Some(text))
            }
            NodeData::CallSignature(data) => {
                let Some(mut text) = self.type_parameter_nodes_text(data.type_parameters, false)?
                else {
                    return Ok(None);
                };
                let Some(parameters) = self.parameter_nodes_text(self.nodes(data.parameters))?
                else {
                    return Ok(None);
                };
                text.push('(');
                text.push_str(&parameters);
                text.push(')');
                text.push_str(&self.return_type_annotation(data.r#type)?);
                Ok(Some(text))
            }
            NodeData::ConstructSignature(data) => {
                let Some(type_parameters) =
                    self.type_parameter_nodes_text(data.type_parameters, false)?
                else {
                    return Ok(None);
                };
                let Some(parameters) = self.parameter_nodes_text(self.nodes(data.parameters))?
                else {
                    return Ok(None);
                };
                let mut text = format!("new {type_parameters}({parameters})");
                text.push_str(&self.return_type_annotation(data.r#type)?);
                Ok(Some(text))
            }
            NodeData::IndexSignature(data) => {
                let Some(mut text) = self.modifiers(data.modifiers)? else {
                    return Ok(None);
                };
                let Some(parameters) = self.parameter_nodes_text(self.nodes(data.parameters))?
                else {
                    return Ok(None);
                };
                text.push('[');
                text.push_str(&parameters);
                text.push(']');
                text.push_str(&self.return_type_annotation(data.r#type)?);
                Ok(Some(text))
            }
            NodeData::GetAccessor(data) => self.type_accessor_member(
                "get",
                data.modifiers,
                data.name,
                data.type_parameters,
                data.parameters,
                data.r#type,
                data.body,
            ),
            NodeData::SetAccessor(data) => self.type_accessor_member(
                "set",
                data.modifiers,
                data.name,
                data.type_parameters,
                data.parameters,
                data.r#type,
                data.body,
            ),
            _ => Ok(None),
        }
    }

    fn type_member_entry(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        if self.node_enters_reuse_scope(node) {
            self.with_reuse_scope(node, |printer| printer.type_member(node))
        } else {
            self.type_member(node)
        }
    }

    #[allow(clippy::too_many_arguments)] // Mirrors the accessor node's emitted fields.
    fn type_accessor_member(
        &mut self,
        keyword: &'static str,
        modifiers: Option<NodeArrayId>,
        name: Option<NodeId>,
        type_parameters: Option<NodeArrayId>,
        parameters: Option<NodeArrayId>,
        return_type: Option<NodeId>,
        body: Option<NodeId>,
    ) -> CheckResult<Option<String>> {
        let Some(mut text) = self.decorated_modifiers(modifiers)? else {
            return Ok(None);
        };
        let Some(name) = name else {
            return Ok(None);
        };
        text.push_str(keyword);
        text.push(' ');
        text.push_str(&self.member_name_text(name)?);
        let Some(type_parameters) = self.type_parameter_nodes_text(type_parameters, false)? else {
            return Ok(None);
        };
        text.push_str(&type_parameters);
        let Some(parameters) = self.parameter_nodes_text(self.nodes(parameters))? else {
            return Ok(None);
        };
        text.push('(');
        text.push_str(&parameters);
        text.push(')');
        text.push_str(&self.return_type_annotation(return_type)?);
        if let Some(body) = body {
            let Some(body) =
                self.with_line_start(false, |printer| printer.function_body_block(body))?
            else {
                return Ok(None);
            };
            text.push(' ');
            text.push_str(&body);
        }
        Ok(Some(text))
    }

    fn type_member_has_body(&self, node: NodeId) -> bool {
        match self.state.data_of(node) {
            NodeData::GetAccessor(data) => data.body.is_some(),
            NodeData::SetAccessor(data) => data.body.is_some(),
            _ => false,
        }
    }

    fn type_alias_declaration(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::TypeAliasDeclaration(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(mut text) = self.modifiers(data.modifiers)? else {
            return Ok(None);
        };
        let Some(name) = data.name.and_then(|name| self.identifier_name(name)) else {
            return Ok(None);
        };
        let Some(ty) = data.r#type else {
            return Ok(None);
        };
        text.push_str("type ");
        text.push_str(&name);
        let Some(type_parameters) = self.type_parameter_nodes_text(data.type_parameters, false)?
        else {
            return Ok(None);
        };
        text.push_str(&type_parameters);
        text.push_str(" = ");
        text.push_str(&self.type_annotation_text(ty)?);
        text.push(';');
        Ok(Some(text))
    }

    fn enum_declaration(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::EnumDeclaration(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(mut text) = self.modifiers(data.modifiers)? else {
            return Ok(None);
        };
        let Some(name) = data.name.and_then(|name| self.identifier_name(name)) else {
            return Ok(None);
        };
        text.push_str("enum ");
        text.push_str(&name);
        let members = self.nodes(data.members);
        if members.is_empty() {
            text.push(' ');
            text.push_str(&self.empty_multiline_braces());
            return Ok(Some(text));
        }
        let mut retained = Vec::with_capacity(members.len());
        for member in members {
            if self.named_declaration_is_removed(member)? {
                continue;
            }
            retained.push(member);
        }
        if retained.is_empty() {
            text.push(' ');
            text.push_str(&self.empty_multiline_braces());
            return Ok(Some(text));
        }
        let rendered = self.with_increased_indent(|printer| {
            let mut rendered = String::new();
            for (index, member) in retained.into_iter().enumerate() {
                if index != 0 {
                    rendered.push(',');
                }
                rendered.push_str(&printer.indent_text());
                let Some(member) = printer.with_line_start(true, |printer| printer.node(member))?
                else {
                    return Ok(None);
                };
                rendered.push_str(&member);
            }
            Ok(Some(rendered))
        })?;
        let Some(rendered) = rendered else {
            return Ok(None);
        };
        text.push_str(" {");
        text.push_str(&rendered);
        text.push_str(&self.indent_text());
        text.push('}');
        Ok(Some(text))
    }

    fn enum_member(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::EnumMember(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(name) = data.name else {
            return Ok(None);
        };
        let mut text = self.member_name_text(name)?;
        if let Some(initializer) = data.initializer {
            let Some(mut initializer_text) = self.expression(initializer)? else {
                return Ok(None);
            };
            if self.is_comma_sequence(initializer) {
                initializer_text = format!("({initializer_text})");
            }
            text.push_str(" = ");
            text.push_str(&initializer_text);
        }
        Ok(Some(text))
    }

    fn if_statement(&mut self, data: IfStatementData) -> CheckResult<Option<String>> {
        let (Some(condition), Some(then_statement)) = (data.expression, data.then_statement) else {
            return Ok(None);
        };
        let Some(condition) = self.expression(condition)? else {
            return Ok(None);
        };
        let Some(then_statement_text) = self.embedded_statement(then_statement)? else {
            return Ok(None);
        };
        let mut text = format!("if ({condition}){then_statement_text}");
        if let Some(else_statement) = data.else_statement {
            text.push_str(&self.indent_text());
            text.push_str("else");
            if self.state.kind_of(else_statement) == SyntaxKind::IfStatement {
                let Some(else_statement) =
                    self.with_line_start(false, |printer| printer.node(else_statement))?
                else {
                    return Ok(None);
                };
                text.push(' ');
                text.push_str(&else_statement);
            } else {
                let Some(else_statement) = self.embedded_statement(else_statement)? else {
                    return Ok(None);
                };
                text.push_str(&else_statement);
            }
        }
        Ok(Some(text))
    }

    fn do_statement(&mut self, data: DoStatementData) -> CheckResult<Option<String>> {
        let (Some(statement), Some(expression)) = (data.statement, data.expression) else {
            return Ok(None);
        };
        let Some(statement_text) = self.embedded_statement(statement)? else {
            return Ok(None);
        };
        let Some(expression) = self.expression(expression)? else {
            return Ok(None);
        };
        let before_while = if self.state.kind_of(statement) == SyntaxKind::Block {
            " ".to_owned()
        } else {
            self.indent_text()
        };
        Ok(Some(format!(
            "do{statement_text}{before_while}while ({expression});"
        )))
    }

    fn while_statement(&mut self, data: WhileStatementData) -> CheckResult<Option<String>> {
        let (Some(expression), Some(statement)) = (data.expression, data.statement) else {
            return Ok(None);
        };
        let Some(expression) = self.expression(expression)? else {
            return Ok(None);
        };
        let Some(statement) = self.embedded_statement(statement)? else {
            return Ok(None);
        };
        Ok(Some(format!("while ({expression}){statement}")))
    }

    fn for_statement(&mut self, data: ForStatementData) -> CheckResult<Option<String>> {
        let initializer = match data.initializer {
            Some(initializer) => {
                let Some(initializer) = self.for_initializer(initializer)? else {
                    return Ok(None);
                };
                initializer
            }
            None => String::new(),
        };
        let condition = match data.condition {
            Some(condition) => {
                let Some(condition) = self.expression(condition)? else {
                    return Ok(None);
                };
                condition
            }
            None => String::new(),
        };
        let incrementor = match data.incrementor {
            Some(incrementor) => {
                let Some(incrementor) = self.expression(incrementor)? else {
                    return Ok(None);
                };
                incrementor
            }
            None => String::new(),
        };
        let Some(statement) = data.statement else {
            return Ok(None);
        };
        let Some(statement) = self.embedded_statement(statement)? else {
            return Ok(None);
        };
        let condition_space = if condition.is_empty() { "" } else { " " };
        let incrementor_space = if incrementor.is_empty() { "" } else { " " };
        Ok(Some(format!(
            "for ({initializer};{condition_space}{condition};{incrementor_space}{incrementor}){statement}"
        )))
    }

    fn for_in_statement(&mut self, data: ForInStatementData) -> CheckResult<Option<String>> {
        let (Some(initializer), Some(expression), Some(statement)) =
            (data.initializer, data.expression, data.statement)
        else {
            return Ok(None);
        };
        let Some(initializer) = self.for_initializer(initializer)? else {
            return Ok(None);
        };
        let Some(expression) = self.expression(expression)? else {
            return Ok(None);
        };
        let Some(statement) = self.embedded_statement(statement)? else {
            return Ok(None);
        };
        Ok(Some(format!(
            "for ({initializer} in {expression}){statement}"
        )))
    }

    fn for_of_statement(&mut self, data: ForOfStatementData) -> CheckResult<Option<String>> {
        let (Some(initializer), Some(expression), Some(statement)) =
            (data.initializer, data.expression, data.statement)
        else {
            return Ok(None);
        };
        let Some(initializer) = self.for_initializer(initializer)? else {
            return Ok(None);
        };
        let Some(expression) = self.expression(expression)? else {
            return Ok(None);
        };
        let Some(statement) = self.embedded_statement(statement)? else {
            return Ok(None);
        };
        let await_text = if data.await_modifier.is_some() {
            "await "
        } else {
            ""
        };
        Ok(Some(format!(
            "for {await_text}({initializer} of {expression}){statement}"
        )))
    }

    fn with_statement(&mut self, data: WithStatementData) -> CheckResult<Option<String>> {
        let (Some(expression), Some(statement)) = (data.expression, data.statement) else {
            return Ok(None);
        };
        let Some(expression) = self.expression(expression)? else {
            return Ok(None);
        };
        let Some(statement) = self.embedded_statement(statement)? else {
            return Ok(None);
        };
        Ok(Some(format!("with ({expression}){statement}")))
    }

    fn switch_statement(&mut self, data: SwitchStatementData) -> CheckResult<Option<String>> {
        let (Some(expression), Some(case_block)) = (data.expression, data.case_block) else {
            return Ok(None);
        };
        let Some(expression) = self.expression(expression)? else {
            return Ok(None);
        };
        let Some(case_block) = self.case_block(case_block)? else {
            return Ok(None);
        };
        Ok(Some(format!("switch ({expression}) {case_block}")))
    }

    fn try_statement(&mut self, data: TryStatementData) -> CheckResult<Option<String>> {
        let Some(try_block) = data.try_block else {
            return Ok(None);
        };
        let Some(try_block) = self.with_line_start(false, |printer| printer.block(try_block))?
        else {
            return Ok(None);
        };
        let mut text = format!("try {try_block}");
        if let Some(catch_clause) = data.catch_clause {
            let Some(catch_clause) =
                self.with_line_start(true, |printer| printer.catch_clause(catch_clause))?
            else {
                return Ok(None);
            };
            text.push_str(&self.indent_text());
            text.push_str(&catch_clause);
        }
        if let Some(finally_block) = data.finally_block {
            let Some(finally_block) =
                self.with_line_start(false, |printer| printer.block(finally_block))?
            else {
                return Ok(None);
            };
            text.push_str(&self.indent_text());
            text.push_str("finally ");
            text.push_str(&finally_block);
        }
        Ok(Some(text))
    }

    fn case_block(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::CaseBlock(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let clauses = self.nodes(data.clauses);
        if clauses.is_empty() {
            return Ok(Some(self.empty_multiline_braces()));
        }
        let rendered = self.with_increased_indent(|printer| {
            let mut rendered = String::new();
            for clause in clauses {
                let Some(clause) = printer.with_line_start(true, |printer| printer.node(clause))?
                else {
                    return Ok(None);
                };
                if clause.is_empty() {
                    continue;
                }
                rendered.push_str(&printer.indent_text());
                rendered.push_str(&clause);
            }
            Ok(Some(rendered))
        })?;
        let Some(rendered) = rendered else {
            return Ok(None);
        };
        Ok(Some(format!("{{{rendered}{}}}", self.indent_text())))
    }

    fn case_or_default_clause(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let (head, statements) = match self.state.data_of(node).clone() {
            NodeData::CaseClause(data) => {
                let Some(expression) = data.expression else {
                    return Ok(None);
                };
                let Some(mut expression_text) = self.expression(expression)? else {
                    return Ok(None);
                };
                if self.is_comma_sequence(expression) {
                    expression_text = format!("({expression_text})");
                }
                (format!("case {expression_text}:"), data.statements)
            }
            NodeData::DefaultClause(data) => ("default:".to_owned(), data.statements),
            _ => return Ok(None),
        };
        let statements = self.nodes(statements);
        if statements.is_empty() {
            return Ok(Some(head));
        }
        if statements.len() == 1 && self.case_statement_is_single_line(node, statements[0]) {
            let Some(statement) =
                self.with_line_start(false, |printer| printer.node(statements[0]))?
            else {
                return Ok(None);
            };
            return Ok(Some(format!("{head} {statement}")));
        }
        let rendered = self.with_increased_indent(|printer| {
            let mut rendered = String::new();
            for statement in statements {
                let Some(statement) =
                    printer.with_line_start(true, |printer| printer.node(statement))?
                else {
                    return Ok(None);
                };
                if statement.is_empty() {
                    continue;
                }
                rendered.push_str(&printer.indent_text());
                rendered.push_str(&statement);
            }
            Ok(Some(rendered))
        })?;
        Ok(rendered.map(|rendered| format!("{head}{rendered}")))
    }

    fn catch_clause(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::CatchClause(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let mut text = "catch".to_owned();
        if let Some(variable) = data.variable_declaration {
            let Some(variable) = self.variable_declaration(variable)? else {
                return Ok(None);
            };
            text.push_str(" (");
            text.push_str(&variable);
            text.push(')');
        }
        let Some(block) = data.block else {
            return Ok(None);
        };
        let Some(block) = self.with_line_start(false, |printer| printer.block(block))? else {
            return Ok(None);
        };
        text.push(' ');
        text.push_str(&block);
        Ok(Some(text))
    }

    fn variable_declaration_list(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::VariableDeclarationList(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let flags = self.state.node_flags(node)
            & (NodeFlags::LET.bits() | NodeFlags::CONST.bits() | NodeFlags::USING.bits());
        let keyword = if flags == NodeFlags::AWAIT_USING.bits() {
            "await using"
        } else if flags == NodeFlags::USING.bits() {
            "using"
        } else if flags == NodeFlags::CONST.bits() {
            "const"
        } else if flags == NodeFlags::LET.bits() {
            "let"
        } else {
            "var"
        };
        let declarations = self.nodes(data.declarations);
        if declarations.is_empty() {
            return Ok(None);
        }
        let mut rendered = Vec::with_capacity(declarations.len());
        for declaration in declarations {
            let Some(declaration) = self.variable_declaration(declaration)? else {
                return Ok(None);
            };
            rendered.push(declaration);
        }
        Ok(Some(format!("{keyword} {}", rendered.join(", "))))
    }

    fn variable_declaration(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::VariableDeclaration(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(name) = data.name else {
            return Ok(None);
        };
        let Some(mut text) = self.binding_name(name)? else {
            return Ok(None);
        };
        if data.exclamation_token.is_some() {
            text.push('!');
        }
        if let Some(ty) = data.r#type {
            text.push_str(": ");
            text.push_str(&self.type_annotation_text(ty)?);
        }
        if let Some(initializer) = data.initializer {
            let Some(mut initializer_text) = self.expression(initializer)? else {
                return Ok(None);
            };
            if self.is_comma_sequence(initializer) {
                initializer_text = format!("({initializer_text})");
            }
            text.push_str(" = ");
            text.push_str(&initializer_text);
        }
        Ok(Some(text))
    }

    fn heritage_clause(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::HeritageClause(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(keyword) = tsc_syntax::tokens::token_to_string(data.token) else {
            return Ok(None);
        };
        let types = self.nodes(data.types);
        if types.is_empty() {
            return Ok(None);
        }
        let mut rendered = Vec::with_capacity(types.len());
        for ty in types {
            let Some(ty) = self.expression(ty)? else {
                return Ok(None);
            };
            rendered.push(ty);
        }
        Ok(Some(format!(" {keyword} {}", rendered.join(", "))))
    }

    fn class_or_object_member(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        match self.state.data_of(node).clone() {
            NodeData::PropertyDeclaration(data) => {
                let Some(mut text) = self.decorated_modifiers(data.modifiers)? else {
                    return Ok(None);
                };
                let Some(name) = data.name else {
                    return Ok(None);
                };
                text.push_str(&self.member_name_text(name)?);
                if data.question_token.is_some() {
                    text.push('?');
                }
                if data.exclamation_token.is_some() {
                    text.push('!');
                }
                if let Some(ty) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text(ty)?);
                } else if data.initializer.is_none() {
                    text.push_str(": any");
                }
                if let Some(initializer) = data.initializer {
                    let Some(mut initializer_text) = self.expression(initializer)? else {
                        return Ok(None);
                    };
                    if self.is_comma_sequence(initializer) {
                        initializer_text = format!("({initializer_text})");
                    }
                    text.push_str(" = ");
                    text.push_str(&initializer_text);
                }
                text.push(';');
                Ok(Some(text))
            }
            NodeData::MethodDeclaration(data) => self.method_declaration(data),
            NodeData::GetAccessor(data) => self.accessor_declaration(
                "get",
                data.modifiers,
                data.name,
                data.type_parameters,
                data.parameters,
                data.r#type,
                data.body,
            ),
            NodeData::SetAccessor(data) => self.accessor_declaration(
                "set",
                data.modifiers,
                data.name,
                data.type_parameters,
                data.parameters,
                data.r#type,
                data.body,
            ),
            NodeData::Constructor(data) => {
                let Some(mut text) = self.modifiers(data.modifiers)? else {
                    return Ok(None);
                };
                text.push_str("constructor");
                let Some(type_parameters) =
                    self.type_parameter_nodes_text(data.type_parameters, false)?
                else {
                    return Ok(None);
                };
                text.push_str(&type_parameters);
                let Some(parameters) = self.parameter_nodes_text(self.nodes(data.parameters))?
                else {
                    return Ok(None);
                };
                text.push('(');
                text.push_str(&parameters);
                text.push(')');
                text.push_str(&self.return_type_annotation(data.r#type)?);
                self.finish_member_body(text, data.body)
            }
            NodeData::ClassStaticBlockDeclaration(data) => {
                let Some(body) = data.body else {
                    return Ok(None);
                };
                let Some(body) =
                    self.with_line_start(false, |printer| printer.function_body_block(body))?
                else {
                    return Ok(None);
                };
                Ok(Some(format!("static {body}")))
            }
            NodeData::IndexSignature(data) => {
                let Some(mut text) = self.modifiers(data.modifiers)? else {
                    return Ok(None);
                };
                let Some(parameters) = self.parameter_nodes_text(self.nodes(data.parameters))?
                else {
                    return Ok(None);
                };
                text.push('[');
                text.push_str(&parameters);
                text.push(']');
                text.push_str(&self.return_type_annotation(data.r#type)?);
                text.push(';');
                Ok(Some(text))
            }
            NodeData::Token if self.state.kind_of(node) == SyntaxKind::SemicolonClassElement => {
                Ok(Some(";".to_owned()))
            }
            _ => Ok(None),
        }
    }

    fn method_declaration(&mut self, data: MethodDeclarationData) -> CheckResult<Option<String>> {
        let Some(mut text) = self.decorated_modifiers(data.modifiers)? else {
            return Ok(None);
        };
        if data.asterisk_token.is_some() {
            text.push('*');
        }
        let Some(name) = data.name else {
            return Ok(None);
        };
        text.push_str(&self.member_name_text(name)?);
        if data.question_token.is_some() {
            text.push('?');
        }
        let Some(type_parameters) = self.type_parameter_nodes_text(data.type_parameters, false)?
        else {
            return Ok(None);
        };
        text.push_str(&type_parameters);
        let Some(parameters) = self.parameter_nodes_text(self.nodes(data.parameters))? else {
            return Ok(None);
        };
        text.push('(');
        text.push_str(&parameters);
        text.push(')');
        text.push_str(&self.return_type_annotation(data.r#type)?);
        self.finish_member_body(text, data.body)
    }

    #[allow(clippy::too_many_arguments)]
    fn accessor_declaration(
        &mut self,
        keyword: &'static str,
        modifiers: Option<NodeArrayId>,
        name: Option<NodeId>,
        type_parameters: Option<NodeArrayId>,
        parameters: Option<NodeArrayId>,
        return_type: Option<NodeId>,
        body: Option<NodeId>,
    ) -> CheckResult<Option<String>> {
        let Some(mut text) = self.decorated_modifiers(modifiers)? else {
            return Ok(None);
        };
        let Some(name) = name else {
            return Ok(None);
        };
        text.push_str(keyword);
        text.push(' ');
        text.push_str(&self.member_name_text(name)?);
        let Some(type_parameters) = self.type_parameter_nodes_text(type_parameters, false)? else {
            return Ok(None);
        };
        text.push_str(&type_parameters);
        let Some(parameters) = self.parameter_nodes_text(self.nodes(parameters))? else {
            return Ok(None);
        };
        text.push('(');
        text.push_str(&parameters);
        text.push(')');
        text.push_str(&self.return_type_annotation(return_type)?);
        self.finish_member_body(text, body)
    }

    fn finish_member_body(
        &mut self,
        mut text: String,
        body: Option<NodeId>,
    ) -> CheckResult<Option<String>> {
        match body {
            Some(body) => {
                let Some(body) =
                    self.with_line_start(false, |printer| printer.function_body_block(body))?
                else {
                    return Ok(None);
                };
                text.push(' ');
                text.push_str(&body);
            }
            None => text.push(';'),
        }
        Ok(Some(text))
    }

    fn modifiers(&mut self, modifiers: Option<NodeArrayId>) -> CheckResult<Option<String>> {
        self.modifiers_worker(modifiers, false)
    }

    fn decorated_modifiers(
        &mut self,
        modifiers: Option<NodeArrayId>,
    ) -> CheckResult<Option<String>> {
        self.modifiers_worker(modifiers, true)
    }

    fn modifiers_worker(
        &mut self,
        modifiers: Option<NodeArrayId>,
        allow_decorators: bool,
    ) -> CheckResult<Option<String>> {
        let modifiers = self.nodes(modifiers);
        if modifiers.is_empty() {
            return Ok(Some(String::new()));
        }
        let mut text = String::new();
        let mut index = 0;
        let mut at_line_start = self.state.slice_display_clone_at_line_start;
        while index < modifiers.len() {
            if matches!(self.state.data_of(modifiers[index]), NodeData::Decorator(_)) {
                if !allow_decorators {
                    while index < modifiers.len()
                        && matches!(self.state.data_of(modifiers[index]), NodeData::Decorator(_))
                    {
                        index += 1;
                    }
                    continue;
                }
                // A MultiLine decorator list starts with writeLine. It emits
                // indentation only when the enclosing writer was mid-line;
                // a list caller may already have materialized indentation
                // while deliberately retaining the virtual line-start bit.
                if !at_line_start {
                    text.push_str(&self.indent_text());
                }
                let mut first = true;
                while index < modifiers.len() {
                    let NodeData::Decorator(data) = self.state.data_of(modifiers[index]).clone()
                    else {
                        break;
                    };
                    if !first {
                        text.push_str(&self.indent_text());
                    }
                    let Some(expression) = data.expression else {
                        return Ok(None);
                    };
                    let Some(expression) = self.expression_at_line_start(expression, false)? else {
                        return Ok(None);
                    };
                    text.push('@');
                    text.push_str(&expression);
                    first = false;
                    index += 1;
                }
                // Decorators use a MultiLine list. With the display writer's
                // empty newline text, its trailing writeLine contributes only
                // the current indentation before the declaration/modifier.
                text.push_str(&self.indent_text());
                at_line_start = true;
                continue;
            }

            let mut first = true;
            while index < modifiers.len()
                && !matches!(self.state.data_of(modifiers[index]), NodeData::Decorator(_))
            {
                if !first {
                    text.push(' ');
                }
                let Some(token) =
                    tsc_syntax::tokens::token_to_string(self.state.kind_of(modifiers[index]))
                else {
                    return Ok(None);
                };
                text.push_str(token);
                first = false;
                index += 1;
                at_line_start = false;
            }
            // Modifier lists are single-line, space-delimited, and carry
            // SpaceAfterList.
            text.push(' ');
        }
        Ok(Some(text))
    }

    fn named_declaration_is_removed(&mut self, node: NodeId) -> CheckResult<bool> {
        let source = self.state.binder.source_of_node(node);
        let computed_name = tsc_binder::node_util::get_name_of_declaration(source, node)
            .is_some_and(|name| self.state.kind_of(name) == SyntaxKind::ComputedPropertyName);
        if !computed_name || !tsc_binder::node_util::has_dynamic_name(source, node) {
            return Ok(false);
        }
        Ok(!self.state.has_bindable_name(node)?)
    }

    fn return_type_annotation(&mut self, ty: Option<NodeId>) -> CheckResult<String> {
        Ok(match ty {
            Some(ty) => format!(": {}", self.type_annotation_text(ty)?),
            None => ": any".to_owned(),
        })
    }

    fn type_annotation_text(&mut self, node: NodeId) -> CheckResult<String> {
        self.with_line_start(false, |printer| {
            printer.state.type_annotation_text_slice(node)
        })
    }

    fn member_name_text(&mut self, node: NodeId) -> CheckResult<String> {
        self.with_line_start(false, |printer| {
            printer.state.member_name_node_text_slice(node)
        })
    }

    fn type_parameter_nodes_text(
        &mut self,
        nodes: Option<NodeArrayId>,
        allow_trailing_comma: bool,
    ) -> CheckResult<Option<String>> {
        let Some(nodes) = nodes else {
            return Ok(Some(String::new()));
        };
        let node_array = self.state.binder.node_array(nodes);
        let parameters = node_array.nodes.clone();
        let has_trailing_comma = node_array.has_trailing_comma;
        if parameters.is_empty() {
            return Ok(Some(String::new()));
        }

        let mut rendered = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            let NodeData::TypeParameter(data) = self.state.data_of(parameter).clone() else {
                return Ok(None);
            };
            let mut text = String::new();
            for modifier in self.nodes(data.modifiers) {
                if matches!(self.state.data_of(modifier), NodeData::Decorator(_)) {
                    continue;
                }
                let Some(token) = tsc_syntax::tokens::token_to_string(self.state.kind_of(modifier))
                else {
                    return Ok(None);
                };
                text.push_str(token);
                text.push(' ');
            }
            let Some(name) = data.name.and_then(|name| self.identifier_name(name)) else {
                return Ok(None);
            };
            text.push_str(&name);
            if let Some(constraint) = data.constraint {
                let constraint = self.type_annotation_text(constraint)?;
                text.push_str(" extends ");
                text.push_str(&constraint);
            }
            if let Some(default) = data.r#default {
                let default = self.type_annotation_text(default)?;
                text.push_str(" = ");
                text.push_str(&default);
            }
            rendered.push(text);
        }

        let mut text = format!("<{}", rendered.join(", "));
        if allow_trailing_comma && has_trailing_comma {
            text.push(',');
        }
        text.push('>');
        Ok(Some(text))
    }

    fn parameter_nodes_text(&mut self, nodes: Vec<NodeId>) -> CheckResult<Option<String>> {
        let mut rendered = Vec::with_capacity(nodes.len());
        for node in nodes {
            let NodeData::Parameter(data) = self.state.data_of(node).clone() else {
                return Ok(None);
            };
            let retain_modifiers = data.r#type.is_some() || data.initializer.is_some();
            let mut text = if retain_modifiers {
                let Some(modifiers) = self.with_line_start(false, |printer| {
                    printer.decorated_modifiers(data.modifiers)
                })?
                else {
                    return Ok(None);
                };
                modifiers
            } else {
                // visitExistingNodeTreeSymbols strips parameter modifiers in
                // the same branch that synthesizes a missing `any` type.
                String::new()
            };
            if data.dot_dot_dot_token.is_some() {
                text.push_str("...");
            }
            let Some(name) = data.name else {
                return Ok(None);
            };
            let Some(name) = self.binding_name(name)? else {
                return Ok(None);
            };
            text.push_str(&name);
            if data.question_token.is_some() {
                text.push('?');
            }
            if let Some(ty) = data.r#type {
                text.push_str(": ");
                text.push_str(&self.type_annotation_text(ty)?);
            } else if data.initializer.is_none() {
                text.push_str(": any");
            }
            if let Some(initializer) = data.initializer {
                let Some(mut initializer_text) = self.expression(initializer)? else {
                    return Ok(None);
                };
                if self.is_comma_sequence(initializer) {
                    initializer_text = format!("({initializer_text})");
                }
                text.push_str(" = ");
                text.push_str(&initializer_text);
            }
            rendered.push(text);
        }
        Ok(Some(rendered.join(", ")))
    }

    fn binding_name(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        match self.state.data_of(node) {
            NodeData::Identifier(_) => Ok(self.identifier_name(node)),
            NodeData::ObjectBindingPattern(_) | NodeData::ArrayBindingPattern(_) => {
                self.binding_pattern(node)
            }
            _ => Ok(None),
        }
    }

    fn binding_pattern(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let (elements, has_trailing_comma, open, close, padded) =
            match self.state.data_of(node).clone() {
                NodeData::ObjectBindingPattern(data) => {
                    let (elements, trailing) = self.node_array(data.elements);
                    (elements, trailing, '{', '}', true)
                }
                NodeData::ArrayBindingPattern(data) => {
                    let (elements, trailing) = self.node_array(data.elements);
                    (elements, trailing, '[', ']', false)
                }
                _ => return Ok(None),
            };

        if elements.is_empty() {
            return Ok(Some(format!("{open}{close}")));
        }
        let mut rendered = Vec::with_capacity(elements.len());
        for element in elements {
            let Some(element) =
                self.with_line_start(false, |printer| printer.binding_element(element))?
            else {
                return Ok(None);
            };
            rendered.push(element);
        }
        let mut contents = rendered.join(", ");
        if has_trailing_comma {
            contents.push(',');
        }
        Ok(Some(if padded {
            format!("{open} {contents} {close}")
        } else {
            format!("{open}{contents}{close}")
        }))
    }

    fn binding_element(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let data = match self.state.data_of(node).clone() {
            NodeData::OmittedExpression(_) => return Ok(Some(String::new())),
            NodeData::BindingElement(data) => data,
            _ => return Ok(None),
        };
        let mut text = String::new();
        if data.dot_dot_dot_token.is_some() {
            text.push_str("...");
        }
        if let Some(property_name) = data.property_name {
            let Some(property_name) = self.binding_property_name(property_name)? else {
                return Ok(None);
            };
            text.push_str(&property_name);
            text.push_str(": ");
        }
        let Some(name) = data.name else {
            return Ok(None);
        };
        let Some(name) = self.binding_name(name)? else {
            return Ok(None);
        };
        text.push_str(&name);
        if let Some(initializer) = data.initializer {
            let Some(mut initializer_text) = self.expression(initializer)? else {
                return Ok(None);
            };
            if self.is_comma_sequence(initializer) {
                initializer_text = format!("({initializer_text})");
            }
            text.push_str(" = ");
            text.push_str(&initializer_text);
        }
        Ok(Some(text))
    }

    fn binding_property_name(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ComputedPropertyName(data) = self.state.data_of(node).clone() else {
            return Ok(Some(self.member_name_text(node)?));
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        if self.state.is_entity_name_expression(expression) {
            return Ok(Some(self.member_name_text(node)?));
        }
        self.computed_property_expression(expression)
    }

    fn computed_property_expression(&mut self, expression: NodeId) -> CheckResult<Option<String>> {
        let Some(mut expression_text) = self.expression(expression)? else {
            return Ok(None);
        };
        if self.is_comma_sequence(expression) {
            expression_text = format!("({expression_text})");
        }
        Ok(Some(format!("[{expression_text}]")))
    }

    fn for_initializer(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        if self.state.kind_of(node) == SyntaxKind::VariableDeclarationList {
            self.variable_declaration_list(node)
        } else {
            self.expression(node)
        }
    }

    fn embedded_statement(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        if self.state.kind_of(node) == SyntaxKind::Block {
            return Ok(self
                .with_line_start(false, |printer| printer.node(node))?
                .map(|statement| format!(" {statement}")));
        }
        self.with_increased_indent(|printer| {
            let Some(statement) = printer.with_line_start(true, |printer| printer.node(node))?
            else {
                return Ok(None);
            };
            if statement.is_empty() {
                return Ok(Some(String::new()));
            }
            let mut text = printer.indent_text();
            text.push_str(&statement);
            Ok(Some(text))
        })
    }

    fn jump_statement(
        &self,
        keyword: &'static str,
        label: Option<NodeId>,
    ) -> CheckResult<Option<String>> {
        match label {
            Some(label) => Ok(self
                .identifier_name(label)
                .map(|label| format!("{keyword} {label};"))),
            None => Ok(Some(format!("{keyword};"))),
        }
    }

    fn expression_statement_with_keyword(
        &mut self,
        keyword: &'static str,
        expression: Option<NodeId>,
    ) -> CheckResult<Option<String>> {
        let mut text = keyword.to_owned();
        if let Some(expression) = expression {
            let Some(expression) = self.expression(expression)? else {
                return Ok(None);
            };
            text.push(' ');
            text.push_str(&expression);
        }
        text.push(';');
        Ok(Some(text))
    }

    fn arrow_body_needs_parentheses(&self, node: NodeId) -> bool {
        let node = self.skip_partially_emitted(node);
        self.is_comma_sequence(node)
            || self.state.kind_of(self.leftmost_expression(node))
                == SyntaxKind::ObjectLiteralExpression
    }

    fn expression_statement_needs_parentheses(&self, node: NodeId) -> bool {
        matches!(
            self.state.kind_of(self.leftmost_expression(node)),
            SyntaxKind::ObjectLiteralExpression | SyntaxKind::FunctionExpression
        )
    }

    fn expression_statement_needs_callee_recovery(&self, node: NodeId) -> bool {
        let node = self.skip_partially_emitted(node);
        let NodeData::CallExpression(data) = self.state.data_of(node) else {
            return false;
        };
        data.expression.is_some_and(|callee| {
            matches!(
                self.state.kind_of(self.skip_partially_emitted(callee)),
                SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
            )
        })
    }

    fn is_comma_sequence(&self, node: NodeId) -> bool {
        let node = self.skip_partially_emitted(node);
        if self.state.kind_of(node) == SyntaxKind::CommaListExpression {
            return true;
        }
        matches!(
            self.state.data_of(node),
            NodeData::BinaryExpression(data)
                if data.operator_token.is_some_and(|operator| {
                    self.state.kind_of(operator) == SyntaxKind::CommaToken
                })
        )
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

    fn leftmost_expression(&self, mut node: NodeId) -> NodeId {
        loop {
            node = self.skip_partially_emitted(node);
            let next = match self.state.data_of(node) {
                NodeData::PostfixUnaryExpression(data) => data.operand,
                NodeData::BinaryExpression(data) => data.left,
                NodeData::ConditionalExpression(data) => data.condition,
                NodeData::TaggedTemplateExpression(data) => data.tag,
                NodeData::CallExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::ElementAccessExpression(data) => data.expression,
                NodeData::PropertyAccessExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                _ => None,
            };
            let Some(next) = next else {
                return node;
            };
            node = next;
        }
    }

    fn identifier_name(&self, node: NodeId) -> Option<String> {
        match self.state.data_of(node) {
            NodeData::Identifier(data) => {
                Some(tsc_binder::unescape_leading_underscores(&data.escaped_text).to_owned())
            }
            NodeData::PrivateIdentifier(data) => Some(data.text.clone()),
            _ => None,
        }
    }

    fn nodes(&self, nodes: Option<NodeArrayId>) -> Vec<NodeId> {
        self.state.nodes_of(nodes)
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
        let saved = self.state.slice_display_clone_indent;
        self.state.slice_display_clone_indent = saved + 1;
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

    fn with_reuse_scope<T>(
        &mut self,
        node: NodeId,
        operation: impl FnOnce(&mut Self) -> CheckResult<T>,
    ) -> CheckResult<T> {
        let saved = self.state.slice_display_enclosing.replace(node);
        let result = operation(self);
        self.state.slice_display_enclosing = saved;
        result
    }

    fn node_enters_reuse_scope(&self, node: NodeId) -> bool {
        matches!(
            self.state.kind_of(node),
            SyntaxKind::FunctionDeclaration
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::Constructor
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
                | SyntaxKind::MethodSignature
                | SyntaxKind::CallSignature
                | SyntaxKind::ConstructSignature
                | SyntaxKind::IndexSignature
        )
    }

    fn indent_text(&self) -> String {
        self.state.display_clone_line_indent()
    }

    fn empty_multiline_braces(&self) -> String {
        format!("{{{}}}", self.indent_text())
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

    fn function_body_is_single_line(&self, node: NodeId) -> bool {
        let starts_with_prologue = match self.state.data_of(node) {
            NodeData::Block(data) => self
                .nodes(data.statements)
                .first()
                .is_some_and(|statement| self.is_prologue_directive(*statement)),
            _ => false,
        };
        if starts_with_prologue {
            // emitPrologueDirectives writes a line before each leading
            // directive. Its changed writer position disables the otherwise
            // selected SingleLineFunctionBodyStatements branch.
            return false;
        }
        if self.node_is_multi_line(node) {
            return false;
        }
        match (
            self.state.display_clone_start_line(node),
            self.state.display_clone_end_line(node),
        ) {
            (Some(start), Some(end)) => start == end,
            // Synthesized bodies have no comparable source range and tsc
            // treats them as single-line unless an explicit MultiLine bit is
            // present.
            _ => true,
        }
    }

    fn is_prologue_directive(&self, node: NodeId) -> bool {
        matches!(
            self.state.data_of(node),
            NodeData::ExpressionStatement(data)
                if data.expression.is_some_and(|expression| {
                    self.state.kind_of(expression) == SyntaxKind::StringLiteral
                })
        )
    }

    fn case_statement_is_single_line(&self, parent: NodeId, statement: NodeId) -> bool {
        if self.state.node_flags(parent) & NodeFlags::SYNTHESIZED.bits() != 0
            || self.state.node_flags(statement) & NodeFlags::SYNTHESIZED.bits() != 0
        {
            return true;
        }
        let parent_source = self.state.binder.source_of_node(parent);
        let statement_source = self.state.binder.source_of_node(statement);
        if !std::ptr::eq(parent_source, statement_source) {
            return false;
        }
        matches!(
            (
                self.state.display_clone_start_line(parent),
                self.state.display_clone_start_line(statement),
            ),
            (Some(parent), Some(statement)) if parent == statement
        )
    }
}
