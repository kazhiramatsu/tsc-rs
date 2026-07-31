//! Module/import/export statement closure for annotation-reuse display.
//!
//! These declarations can occur in a parsed function-body initializer even
//! when placement checking later reports an error. The existing-TypeNode
//! visitor does not recover merely because of that placement diagnostic, so
//! its standard-printer face remains part of the clone-display closure.
//! Comments, source maps, and source text are deliberately not emitted.

use tsrs2_syntax::{NodeArrayId, NodeData, NodeId, SyntaxKind};
use tsrs2_types::NodeFlags;

use crate::state::{CheckResult, CheckerState};

struct DisplayCloneModulePrinter<'state, 'program> {
    state: &'state mut CheckerState<'program>,
}

impl<'program> CheckerState<'program> {
    /// tsrs-native: module-statement compartment adapter into the exact
    /// standard-printer closure ledgered by
    /// display_clone_expression_text_at_line_start.
    ///
    /// The caller owns the pending line-start indentation. Indentation and
    /// line-start state are restored even when a malformed child requests the
    /// enclosing TypeNode recovery boundary.
    pub(crate) fn display_clone_module_statement_text(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<String>> {
        let saved_indent = self.slice_display_clone_indent;
        let saved_line_start = self.slice_display_clone_at_line_start;
        let result = DisplayCloneModulePrinter { state: self }.node(node);
        self.slice_display_clone_indent = saved_indent;
        self.slice_display_clone_at_line_start = saved_line_start;
        result
    }
}

impl DisplayCloneModulePrinter<'_, '_> {
    fn node(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        match self.state.kind_of(node) {
            SyntaxKind::ModuleDeclaration => self.module_declaration(node),
            SyntaxKind::ModuleBlock => self.module_block(node),
            SyntaxKind::NamespaceExportDeclaration => self.namespace_export_declaration(node),
            SyntaxKind::ImportEqualsDeclaration => self.import_equals_declaration(node),
            SyntaxKind::ImportDeclaration => self.import_declaration(node),
            SyntaxKind::ImportClause => self.import_clause(node),
            SyntaxKind::NamespaceImport => self.namespace_import(node),
            SyntaxKind::NamedImports => self.named_imports(node),
            SyntaxKind::ImportSpecifier => self.import_specifier(node),
            SyntaxKind::ExportAssignment => self.export_assignment(node),
            SyntaxKind::ExportDeclaration => self.export_declaration(node),
            SyntaxKind::NamespaceExport => self.namespace_export(node),
            SyntaxKind::NamedExports => self.named_exports(node),
            SyntaxKind::ExportSpecifier => self.export_specifier(node),
            SyntaxKind::ImportAttributes => self.import_attributes(node),
            SyntaxKind::ImportAttribute => self.import_attribute(node),
            SyntaxKind::ExternalModuleReference => self.external_module_reference(node),
            // The standard printer emits no text for this parser-recovery
            // declaration. Keeping it empty is safer than manufacturing a
            // declaration face if it reaches an otherwise reusable tree.
            SyntaxKind::MissingDeclaration => Ok(Some(String::new())),
            _ => Ok(None),
        }
    }

    fn module_declaration(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ModuleDeclaration(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(mut text) = self.modifiers(data.modifiers)? else {
            return Ok(None);
        };
        let flags = self.state.node_flags(node);
        if flags & NodeFlags::GLOBAL_AUGMENTATION.bits() == 0 {
            text.push_str(if flags & NodeFlags::NAMESPACE.bits() != 0 {
                "namespace "
            } else {
                "module "
            });
        }
        let Some(name) = data.name else {
            return Ok(None);
        };
        let Some(name) = self.module_name(name)? else {
            return Ok(None);
        };
        text.push_str(&name);

        let Some(mut body) = data.body else {
            text.push(';');
            return Ok(Some(text));
        };
        while self.state.kind_of(body) == SyntaxKind::ModuleDeclaration {
            let NodeData::ModuleDeclaration(nested) = self.state.data_of(body).clone() else {
                return Ok(None);
            };
            let Some(name) = nested.name else {
                return Ok(None);
            };
            let Some(name) = self.module_name(name)? else {
                return Ok(None);
            };
            text.push('.');
            text.push_str(&name);
            let Some(nested_body) = nested.body else {
                // emitModuleDeclaration's dotted-name loop has already
                // consumed this declaration; it writes one space and emits
                // the now-undefined body rather than a semicolon.
                text.push(' ');
                return Ok(Some(text));
            };
            body = nested_body;
        }

        text.push(' ');
        let Some(body) = self.node(body)? else {
            return Ok(None);
        };
        text.push_str(&body);
        Ok(Some(text))
    }

    fn module_block(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ModuleBlock(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let statements = self.nodes(data.statements);
        if statements.is_empty() {
            let single_line = match (
                self.state.display_clone_start_line(node),
                self.state.display_clone_end_line(node),
            ) {
                (Some(start), Some(end)) => start == end,
                // With no comparable current source range, isEmptyBlock
                // selects the forceSingleLine branch.
                _ => true,
            };
            return Ok(Some(if single_line {
                "{ }".to_owned()
            } else {
                format!("{{{}}}", self.indent_text())
            }));
        }

        let rendered = self.with_increased_indent(|printer| {
            let mut rendered = String::new();
            for statement in statements {
                let Some(statement) = printer
                    .state
                    .display_clone_body_node_text_at_line_start(statement, true)?
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

    fn import_equals_declaration(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ImportEqualsDeclaration(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(mut text) = self.modifiers(data.modifiers)? else {
            return Ok(None);
        };
        text.push_str("import ");
        if data.is_type_only {
            text.push_str("type ");
        }
        let Some(name) = data.name.and_then(|name| self.identifier(name)) else {
            return Ok(None);
        };
        text.push_str(&name);
        text.push_str(" = ");
        let Some(module_reference) = data.module_reference else {
            return Ok(None);
        };
        let Some(module_reference) = self.module_reference(module_reference)? else {
            return Ok(None);
        };
        text.push_str(&module_reference);
        text.push(';');
        Ok(Some(text))
    }

    fn import_declaration(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ImportDeclaration(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(mut text) = self.modifiers(data.modifiers)? else {
            return Ok(None);
        };
        text.push_str("import ");
        if let Some(clause) = data.import_clause {
            let Some(clause) = self.import_clause(clause)? else {
                return Ok(None);
            };
            text.push_str(&clause);
            text.push_str(" from ");
        }
        let Some(module_specifier) = data.module_specifier else {
            return Ok(None);
        };
        let Some(module_specifier) = self.expression(module_specifier)? else {
            return Ok(None);
        };
        text.push_str(&module_specifier);
        if let Some(attributes) = data.attributes {
            let Some(attributes) = self.import_attributes(attributes)? else {
                return Ok(None);
            };
            text.push(' ');
            text.push_str(&attributes);
        }
        text.push(';');
        Ok(Some(text))
    }

    fn import_clause(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ImportClause(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let mut text = String::new();
        if let Some(phase) = data.phase_modifier {
            let Some(phase) = tsrs2_syntax::tokens::token_to_string(phase) else {
                return Ok(None);
            };
            text.push_str(phase);
            text.push(' ');
        }
        if let Some(name) = data.name {
            let Some(name) = self.identifier(name) else {
                return Ok(None);
            };
            text.push_str(&name);
        }
        if data.name.is_some() && data.named_bindings.is_some() {
            text.push_str(", ");
        }
        if let Some(bindings) = data.named_bindings {
            let Some(bindings) = self.node(bindings)? else {
                return Ok(None);
            };
            text.push_str(&bindings);
        }
        Ok(Some(text))
    }

    fn namespace_import(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::NamespaceImport(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(name) = data.name.and_then(|name| self.identifier(name)) else {
            return Ok(None);
        };
        Ok(Some(format!("* as {name}")))
    }

    fn named_imports(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::NamedImports(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        self.named_imports_or_exports(data.elements, SyntaxKind::ImportSpecifier)
    }

    fn import_specifier(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ImportSpecifier(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        self.import_or_export_specifier(data.is_type_only, data.property_name, data.name)
    }

    fn export_assignment(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ExportAssignment(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let Some(mut expression_text) = self.expression(expression)? else {
            return Ok(None);
        };
        let export_equals = data.is_export_equals == Some(true);
        let needs_parentheses = if export_equals {
            self.is_comma_sequence(expression)
        } else {
            self.export_default_needs_parentheses(expression)
        };
        if needs_parentheses {
            expression_text = format!("({expression_text})");
        }
        Ok(Some(format!(
            "export {} {expression_text};",
            if export_equals { "=" } else { "default" }
        )))
    }

    fn export_declaration(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ExportDeclaration(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(mut text) = self.modifiers(data.modifiers)? else {
            return Ok(None);
        };
        text.push_str("export ");
        if data.is_type_only {
            text.push_str("type ");
        }
        if let Some(clause) = data.export_clause {
            let Some(clause) = self.node(clause)? else {
                return Ok(None);
            };
            text.push_str(&clause);
        } else {
            text.push('*');
        }
        if let Some(module_specifier) = data.module_specifier {
            let Some(module_specifier) = self.expression(module_specifier)? else {
                return Ok(None);
            };
            text.push_str(" from ");
            text.push_str(&module_specifier);
        }
        if let Some(attributes) = data.attributes {
            let Some(attributes) = self.import_attributes(attributes)? else {
                return Ok(None);
            };
            text.push(' ');
            text.push_str(&attributes);
        }
        text.push(';');
        Ok(Some(text))
    }

    fn namespace_export(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::NamespaceExport(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(name) = data.name else {
            return Ok(None);
        };
        let Some(name) = self.module_export_name(name)? else {
            return Ok(None);
        };
        Ok(Some(format!("* as {name}")))
    }

    fn named_exports(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::NamedExports(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        self.named_imports_or_exports(data.elements, SyntaxKind::ExportSpecifier)
    }

    fn export_specifier(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ExportSpecifier(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        self.import_or_export_specifier(data.is_type_only, data.property_name, data.name)
    }

    fn namespace_export_declaration(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::NamespaceExportDeclaration(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(name) = data.name.and_then(|name| self.identifier(name)) else {
            return Ok(None);
        };
        Ok(Some(format!("export as namespace {name};")))
    }

    fn named_imports_or_exports(
        &mut self,
        elements: Option<NodeArrayId>,
        expected_kind: SyntaxKind,
    ) -> CheckResult<Option<String>> {
        let (elements, has_trailing_comma) = self.node_array(elements);
        if elements.is_empty() {
            return Ok(Some("{}".to_owned()));
        }
        let mut rendered = Vec::with_capacity(elements.len());
        for element in elements {
            if self.state.kind_of(element) != expected_kind {
                return Ok(None);
            }
            let Some(element) = self.node(element)? else {
                return Ok(None);
            };
            rendered.push(element);
        }
        let mut text = format!("{{ {}", rendered.join(", "));
        if has_trailing_comma {
            text.push(',');
        }
        text.push_str(" }");
        Ok(Some(text))
    }

    fn import_or_export_specifier(
        &mut self,
        is_type_only: bool,
        property_name: Option<NodeId>,
        name: Option<NodeId>,
    ) -> CheckResult<Option<String>> {
        let mut text = String::new();
        if is_type_only {
            text.push_str("type ");
        }
        if let Some(property_name) = property_name {
            let Some(property_name) = self.module_export_name(property_name)? else {
                return Ok(None);
            };
            text.push_str(&property_name);
            text.push_str(" as ");
        }
        let Some(name) = name else {
            return Ok(None);
        };
        let Some(name) = self.module_export_name(name)? else {
            return Ok(None);
        };
        text.push_str(&name);
        Ok(Some(text))
    }

    fn import_attributes(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ImportAttributes(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(keyword) = tsrs2_syntax::tokens::token_to_string(data.token) else {
            return Ok(None);
        };
        let elements = self.nodes(data.elements);
        if elements.is_empty() {
            return Ok(Some(format!("{keyword} {{}}")));
        }
        let leading_new_line = self.list_has_leading_line(node, elements[0]);
        let closing_new_line =
            self.list_has_closing_line(node, *elements.last().expect("nonempty import attributes"));
        let contents = self.with_increased_indent(|printer| {
            let mut contents = String::new();
            if leading_new_line {
                contents.push_str(&printer.indent_text());
            } else {
                contents.push(' ');
            }
            let mut previous = None;
            for element in elements {
                let at_line_start;
                if let Some(previous) = previous {
                    contents.push(',');
                    if printer.list_has_separating_line(previous, element) {
                        contents.push_str(&printer.indent_text());
                        at_line_start = true;
                    } else {
                        contents.push(' ');
                        at_line_start = false;
                    }
                } else {
                    at_line_start = leading_new_line;
                }
                let Some(element_text) = printer
                    .with_line_start(at_line_start, |printer| printer.import_attribute(element))?
                else {
                    return Ok(None);
                };
                contents.push_str(&element_text);
                previous = Some(element);
            }
            Ok(Some(contents))
        })?;
        let Some(mut contents) = contents else {
            return Ok(None);
        };
        if closing_new_line {
            contents.push_str(&self.indent_text());
        } else {
            contents.push(' ');
        }
        Ok(Some(format!("{keyword} {{{contents}}}")))
    }

    fn import_attribute(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ImportAttribute(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(name) = data.name else {
            return Ok(None);
        };
        let Some(name) = self.module_export_name(name)? else {
            return Ok(None);
        };
        let Some(value) = data.value else {
            return Ok(None);
        };
        let Some(value) = self.expression(value)? else {
            return Ok(None);
        };
        Ok(Some(format!("{name}: {value}")))
    }

    fn external_module_reference(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::ExternalModuleReference(data) = self.state.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let Some(expression) = self.expression(expression)? else {
            return Ok(None);
        };
        Ok(Some(format!("require({expression})")))
    }

    fn module_reference(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        match self.state.kind_of(node) {
            SyntaxKind::Identifier | SyntaxKind::QualifiedName => Ok(self.entity_name(node)),
            SyntaxKind::ExternalModuleReference => self.external_module_reference(node),
            _ => Ok(None),
        }
    }

    fn entity_name(&self, node: NodeId) -> Option<String> {
        match self.state.data_of(node) {
            NodeData::Identifier(_) => self.identifier(node),
            NodeData::QualifiedName(data) => {
                let left = self.entity_name(data.left?)?;
                let right = self.identifier(data.right?)?;
                Some(format!("{left}.{right}"))
            }
            _ => None,
        }
    }

    fn module_name(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        match self.state.kind_of(node) {
            SyntaxKind::Identifier => Ok(self.identifier(node)),
            SyntaxKind::StringLiteral => self.expression(node),
            _ => Ok(None),
        }
    }

    fn module_export_name(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        match self.state.kind_of(node) {
            SyntaxKind::Identifier => Ok(self.identifier(node)),
            SyntaxKind::StringLiteral => self.expression(node),
            _ => Ok(None),
        }
    }

    fn modifiers(&self, modifiers: Option<NodeArrayId>) -> CheckResult<Option<String>> {
        let mut rendered = Vec::new();
        for modifier in self.nodes(modifiers) {
            if matches!(self.state.data_of(modifier), NodeData::Decorator(_)) {
                continue;
            }
            let Some(token) = tsrs2_syntax::tokens::token_to_string(self.state.kind_of(modifier))
            else {
                return Ok(None);
            };
            rendered.push(token);
        }
        Ok(Some(if rendered.is_empty() {
            String::new()
        } else {
            format!("{} ", rendered.join(" "))
        }))
    }

    fn expression(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        self.state
            .display_clone_expression_text_at_line_start(node, false)
    }

    fn export_default_needs_parentheses(&self, node: NodeId) -> bool {
        let node = self.skip_partially_emitted(node);
        self.is_comma_sequence(node)
            || matches!(
                self.state.kind_of(self.leftmost_expression(node)),
                SyntaxKind::ClassExpression | SyntaxKind::FunctionExpression
            )
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

    fn identifier(&self, node: NodeId) -> Option<String> {
        match self.state.data_of(node) {
            NodeData::Identifier(data) => {
                Some(tsrs2_binder::unescape_leading_underscores(&data.escaped_text).to_owned())
            }
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

    fn nodes(&self, nodes: Option<NodeArrayId>) -> Vec<NodeId> {
        self.state.nodes_of(nodes)
    }

    fn with_increased_indent<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> CheckResult<T>,
    ) -> CheckResult<T> {
        let saved = self.state.slice_display_clone_indent;
        self.state.slice_display_clone_indent += 1;
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

    fn indent_text(&self) -> String {
        self.state.display_clone_line_indent()
    }

    fn list_has_leading_line(&self, parent: NodeId, first: NodeId) -> bool {
        let parent_source = self.state.binder.source_of_node(parent);
        let first_source = self.state.binder.source_of_node(first);
        if !std::ptr::eq(parent_source, first_source) {
            return false;
        }
        matches!(
            (
                self.state.display_clone_start_line(parent),
                self.state.display_clone_start_line(first),
            ),
            (Some(parent), Some(first)) if parent != first
        )
    }

    fn list_has_separating_line(&self, previous: NodeId, next: NodeId) -> bool {
        let previous_source = self.state.binder.source_of_node(previous);
        let next_source = self.state.binder.source_of_node(next);
        if !std::ptr::eq(previous_source, next_source) {
            return false;
        }
        let previous_record = previous_source.arena.node(previous);
        let next_record = next_source.arena.node(next);
        if previous_record.parent.is_none()
            || previous_record.parent != next_record.parent
            || previous_record.end == u32::MAX
            || next_record.pos == u32::MAX
        {
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

    fn list_has_closing_line(&self, parent: NodeId, last: NodeId) -> bool {
        let parent_source = self.state.binder.source_of_node(parent);
        let last_source = self.state.binder.source_of_node(last);
        if !std::ptr::eq(parent_source, last_source) {
            return false;
        }
        matches!(
            (
                self.state.display_clone_end_line(parent),
                self.state.display_clone_end_line(last),
            ),
            (Some(parent), Some(last)) if parent != last
        )
    }
}
