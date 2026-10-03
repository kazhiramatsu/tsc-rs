//! tsgo parser checkJSSyntax (TypeScript 7.1 parser/parser.go:6711-6856): the
//! diagnostics for TypeScript-only syntax in a JavaScript file.
//!
//! tsgo runs the check as the parser finishes the nodes listed in
//! [`is_checked`]; every diagnostic spans a node, name, type or list range
//! with its leading trivia skipped (jsErrorAtRange). The order does not
//! matter: the program sorts a file's syntactic diagnostics.

use tsc_diagnostics::{gen, Diagnostic, DiagnosticMessage, MessageChain, RelatedInfo};
use tsc_syntax::{for_each_child, NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::NodeFlags;

/// The JavaScript-only syntax diagnostics of `source`.
pub(crate) fn get_js_syntactic_diagnostics(source: &SourceFile) -> Vec<Diagnostic> {
    let mut checker = JsSyntaxChecker {
        source,
        diagnostics: Vec::new(),
    };
    // An explicit work stack: JavaScript stress fixtures nest thousands of
    // binary expressions.
    let mut stack = vec![source.root];
    let mut children = Vec::new();
    while let Some(node) = stack.pop() {
        if checker.is_checked(node) {
            checker.check_js_syntax(node);
        }
        children.clear();
        for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        stack.extend(children.iter().rev());
    }
    checker.diagnostics
}

/// tsgo getAdditionalJSSyntacticDiagnostics (compiler/program.go:762-784):
/// without `experimentalDecorators`, a parameter decorator in a JavaScript
/// file the checker does not check is TS1206 at the decorator's range.
pub(crate) fn get_additional_js_syntactic_diagnostics(source: &SourceFile) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut stack = vec![source.root];
    let mut children = Vec::new();
    while let Some(node) = stack.pop() {
        if let NodeData::Parameter(data) = &source.arena.node(node).data {
            let decorator = data.modifiers.and_then(|modifiers| {
                source
                    .arena
                    .node_array(modifiers)
                    .nodes
                    .iter()
                    .copied()
                    .find(|&modifier| source.arena.node(modifier).kind == SyntaxKind::Decorator)
            });
            if let Some(decorator) = decorator {
                let decorator = source.arena.node(decorator);
                let utf16 = |byte: u32| source.positions().byte_to_utf16(byte).unwrap_or(byte);
                let (start, end) = (utf16(decorator.pos), utf16(decorator.end));
                diagnostics.push(Diagnostic::new_js(
                    Some(source.file_name.clone()),
                    Some(start),
                    Some(end.saturating_sub(start)),
                    MessageChain::new(&gen::Decorators_are_not_valid_here, &[]),
                ));
            }
        }
        children.clear();
        for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        stack.extend(children.iter().rev());
    }
    diagnostics
}

struct JsSyntaxChecker<'a> {
    source: &'a SourceFile,
    diagnostics: Vec<Diagnostic>,
}

impl JsSyntaxChecker<'_> {
    fn kind(&self, id: NodeId) -> SyntaxKind {
        self.source.arena.node(id).kind
    }

    fn data(&self, id: NodeId) -> &NodeData {
        &self.source.arena.node(id).data
    }

    fn parent_kind(&self, id: NodeId) -> Option<SyntaxKind> {
        self.source
            .arena
            .node(id)
            .parent
            .map(|parent| self.kind(parent))
    }

    fn nodes(&self, array: Option<NodeArrayId>) -> &[NodeId] {
        array.map_or(&[], |array| self.source.arena.node_array(array).nodes)
    }

    /// Whether tsgo's parser runs checkJSSyntax on `node` (the calls in
    /// parser.go): not on the parameters or accessors of a type signature
    /// (ParseFlagsType), not on an index signature outside a class, and on an
    /// expression with type arguments only in a class `extends` clause.
    fn is_checked(&self, node: NodeId) -> bool {
        match self.kind(node) {
            SyntaxKind::VariableStatement
            | SyntaxKind::VariableDeclaration
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassExpression
            | SyntaxKind::HeritageClause
            | SyntaxKind::Constructor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ImportDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::ExportAssignment
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::ExportSpecifier
            | SyntaxKind::AsExpression
            | SyntaxKind::SatisfiesExpression
            | SyntaxKind::NonNullExpression
            | SyntaxKind::CallExpression
            | SyntaxKind::NewExpression
            | SyntaxKind::TaggedTemplateExpression => true,
            SyntaxKind::Parameter | SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => !matches!(
                self.parent_kind(node),
                Some(
                    SyntaxKind::FunctionType
                        | SyntaxKind::ConstructorType
                        | SyntaxKind::CallSignature
                        | SyntaxKind::ConstructSignature
                        | SyntaxKind::MethodSignature
                        | SyntaxKind::IndexSignature
                        | SyntaxKind::TypeLiteral
                        | SyntaxKind::InterfaceDeclaration
                        | SyntaxKind::JSDocFunctionType
                        | SyntaxKind::JSDocSignature
                )
            ),
            SyntaxKind::IndexSignature => matches!(
                self.parent_kind(node),
                Some(SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression)
            ),
            SyntaxKind::ExpressionWithTypeArguments => {
                let Some(clause) = self.source.arena.node(node).parent else {
                    return false;
                };
                matches!(
                    self.data(clause),
                    NodeData::HeritageClause(data) if data.token == SyntaxKind::ExtendsKeyword
                ) && matches!(
                    self.parent_kind(clause),
                    Some(SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression)
                )
            }
            _ => false,
        }
    }

    /// jsErrorAtRange: `pos..end` with the leading trivia skipped.
    fn diagnostic_at(
        &self,
        pos: u32,
        end: u32,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> Diagnostic {
        let start = tsc_syntax::skip_trivia(self.source.text(), pos as usize).min(end as usize);
        let utf16 = |byte: usize| {
            self.source
                .positions()
                .byte_to_utf16(byte as u32)
                .unwrap_or(byte as u32)
        };
        let (start, end) = (utf16(start), utf16(end as usize));
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        Diagnostic::new_js(
            Some(self.source.file_name.clone()),
            Some(start),
            Some(end.saturating_sub(start)),
            MessageChain::new(message, &args),
        )
    }

    fn error_at_node(&mut self, id: NodeId, message: &'static DiagnosticMessage, args: &[&str]) {
        let node = self.source.arena.node(id);
        let diagnostic = self.diagnostic_at(node.pos, node.end, message, args);
        self.diagnostics.push(diagnostic);
    }

    fn error_at_list(&mut self, list: NodeArrayId, message: &'static DiagnosticMessage) {
        let array = self.source.arena.node_array(list);
        let diagnostic = self.diagnostic_at(array.pos, array.end, message, &[]);
        self.diagnostics.push(diagnostic);
    }

    fn question_token(&self, id: NodeId) -> Option<NodeId> {
        match self.data(id) {
            NodeData::Parameter(data) => data.question_token,
            NodeData::PropertyDeclaration(data) => data.question_token,
            NodeData::MethodDeclaration(data) => data.question_token,
            _ => None,
        }
    }

    fn type_of(&self, id: NodeId) -> Option<NodeId> {
        match self.data(id) {
            NodeData::Parameter(data) => data.r#type,
            NodeData::PropertyDeclaration(data) => data.r#type,
            NodeData::MethodDeclaration(data) => data.r#type,
            NodeData::Constructor(data) => data.r#type,
            NodeData::GetAccessor(data) => data.r#type,
            NodeData::SetAccessor(data) => data.r#type,
            NodeData::FunctionExpression(data) => data.r#type,
            NodeData::FunctionDeclaration(data) => data.r#type,
            NodeData::ArrowFunction(data) => data.r#type,
            NodeData::VariableDeclaration(data) => data.r#type,
            NodeData::IndexSignature(data) => data.r#type,
            _ => None,
        }
    }

    /// `Some(body)` for a function-like node (`None` inside when it has no
    /// body); `None` for any other node.
    fn function_body(&self, id: NodeId) -> Option<Option<NodeId>> {
        match self.data(id) {
            NodeData::MethodDeclaration(data) => Some(data.body),
            NodeData::Constructor(data) => Some(data.body),
            NodeData::GetAccessor(data) => Some(data.body),
            NodeData::SetAccessor(data) => Some(data.body),
            NodeData::FunctionExpression(data) => Some(data.body),
            NodeData::FunctionDeclaration(data) => Some(data.body),
            NodeData::ArrowFunction(data) => Some(data.body),
            NodeData::IndexSignature(_) => Some(None),
            _ => None,
        }
    }

    fn name_of(&self, id: NodeId) -> Option<NodeId> {
        match self.data(id) {
            NodeData::InterfaceDeclaration(data) => data.name,
            NodeData::ModuleDeclaration(data) => data.name,
            NodeData::TypeAliasDeclaration(data) => data.name,
            NodeData::EnumDeclaration(data) => data.name,
            _ => None,
        }
    }

    fn modifiers_of(&self, id: NodeId) -> Option<NodeArrayId> {
        match self.data(id) {
            NodeData::Parameter(data) => data.modifiers,
            NodeData::PropertyDeclaration(data) => data.modifiers,
            NodeData::MethodDeclaration(data) => data.modifiers,
            NodeData::Constructor(data) => data.modifiers,
            NodeData::GetAccessor(data) => data.modifiers,
            NodeData::SetAccessor(data) => data.modifiers,
            NodeData::FunctionExpression(data) => data.modifiers,
            NodeData::FunctionDeclaration(data) => data.modifiers,
            NodeData::ArrowFunction(data) => data.modifiers,
            NodeData::ClassDeclaration(data) => data.modifiers,
            NodeData::ClassExpression(data) => data.modifiers,
            NodeData::VariableStatement(data) => data.modifiers,
            NodeData::IndexSignature(data) => data.modifiers,
            NodeData::InterfaceDeclaration(data) => data.modifiers,
            NodeData::TypeAliasDeclaration(data) => data.modifiers,
            NodeData::EnumDeclaration(data) => data.modifiers,
            NodeData::ModuleDeclaration(data) => data.modifiers,
            NodeData::ImportEqualsDeclaration(data) => data.modifiers,
            NodeData::ImportDeclaration(data) => data.modifiers,
            NodeData::ExportDeclaration(data) => data.modifiers,
            NodeData::ExportAssignment(data) => data.modifiers,
            _ => None,
        }
    }

    fn type_parameters_of(&self, id: NodeId) -> Option<NodeArrayId> {
        match self.data(id) {
            NodeData::ClassDeclaration(data) => data.type_parameters,
            NodeData::ClassExpression(data) => data.type_parameters,
            NodeData::MethodDeclaration(data) => data.type_parameters,
            NodeData::Constructor(data) => data.type_parameters,
            NodeData::GetAccessor(data) => data.type_parameters,
            NodeData::SetAccessor(data) => data.type_parameters,
            NodeData::FunctionExpression(data) => data.type_parameters,
            NodeData::FunctionDeclaration(data) => data.type_parameters,
            NodeData::ArrowFunction(data) => data.type_parameters,
            _ => None,
        }
    }

    fn type_arguments_of(&self, id: NodeId) -> Option<NodeArrayId> {
        match self.data(id) {
            NodeData::CallExpression(data) => data.type_arguments,
            NodeData::NewExpression(data) => data.type_arguments,
            NodeData::ExpressionWithTypeArguments(data) => data.type_arguments,
            NodeData::TaggedTemplateExpression(data) => data.type_arguments,
            _ => None,
        }
    }

    fn check_js_syntax(&mut self, node: NodeId) {
        let kind = self.kind(node);
        match kind {
            SyntaxKind::Parameter
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::FunctionExpression
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::ArrowFunction
            | SyntaxKind::VariableDeclaration
            | SyntaxKind::IndexSignature => {
                if let Some(token) = self.question_token(node) {
                    self.error_at_node(
                        token,
                        &gen::The_0_modifier_can_only_be_used_in_TypeScript_files,
                        &["?"],
                    );
                }
                if self.function_body(node) == Some(None) {
                    self.error_at_node(
                        node,
                        &gen::Signature_declarations_can_only_be_used_in_TypeScript_files,
                        &[],
                    );
                } else if let Some(r#type) = self.type_of(node) {
                    self.error_at_node(
                        r#type,
                        &gen::Type_annotations_can_only_be_used_in_TypeScript_files,
                        &[],
                    );
                }
            }
            SyntaxKind::ImportDeclaration => {
                let type_only = matches!(self.data(node), NodeData::ImportDeclaration(data)
                if data.import_clause.is_some_and(|clause| matches!(
                    self.data(clause),
                    NodeData::ImportClause(clause) if clause.is_type_only
                )));
                if type_only {
                    self.error_at_node(
                        node,
                        &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                        &["import type"],
                    );
                }
            }
            SyntaxKind::ExportDeclaration => {
                if matches!(self.data(node), NodeData::ExportDeclaration(data) if data.is_type_only)
                {
                    self.error_at_node(
                        node,
                        &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                        &["export type"],
                    );
                }
            }
            SyntaxKind::ImportSpecifier => {
                if matches!(self.data(node), NodeData::ImportSpecifier(data) if data.is_type_only) {
                    self.error_at_node(
                        node,
                        &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                        &["import...type"],
                    );
                }
            }
            SyntaxKind::ExportSpecifier => {
                if matches!(self.data(node), NodeData::ExportSpecifier(data) if data.is_type_only) {
                    self.error_at_node(
                        node,
                        &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                        &["export...type"],
                    );
                }
            }
            SyntaxKind::ImportEqualsDeclaration => {
                self.error_at_node(node, &gen::import_can_only_be_used_in_TypeScript_files, &[]);
            }
            SyntaxKind::ExportAssignment => {
                if matches!(
                    self.data(node),
                    NodeData::ExportAssignment(data) if data.is_export_equals == Some(true)
                ) {
                    self.error_at_node(
                        node,
                        &gen::export_can_only_be_used_in_TypeScript_files,
                        &[],
                    );
                }
            }
            SyntaxKind::HeritageClause => {
                if matches!(
                    self.data(node),
                    NodeData::HeritageClause(data) if data.token == SyntaxKind::ImplementsKeyword
                ) {
                    self.error_at_node(
                        node,
                        &gen::implements_clauses_can_only_be_used_in_TypeScript_files,
                        &[],
                    );
                }
            }
            SyntaxKind::InterfaceDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration => {
                if let Some(name) = self.name_of(node) {
                    let flags = NodeFlags::from_bits(self.source.arena.node(node).flags);
                    let (message, keyword): (&'static DiagnosticMessage, &str) = match kind {
                        SyntaxKind::InterfaceDeclaration => (
                            &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                            "interface",
                        ),
                        SyntaxKind::TypeAliasDeclaration => {
                            (&gen::Type_aliases_can_only_be_used_in_TypeScript_files, "")
                        }
                        SyntaxKind::EnumDeclaration => (
                            &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                            "enum",
                        ),
                        _ if flags.contains(NodeFlags::GLOBAL_AUGMENTATION) => (
                            &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                            "global",
                        ),
                        _ if flags.contains(NodeFlags::NAMESPACE) => (
                            &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                            "namespace",
                        ),
                        _ => (
                            &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                            "module",
                        ),
                    };
                    let args: &[&str] = if keyword.is_empty() { &[] } else { &[keyword] };
                    self.error_at_node(name, message, args);
                }
            }
            SyntaxKind::NonNullExpression => {
                self.error_at_node(
                    node,
                    &gen::Non_null_assertions_can_only_be_used_in_TypeScript_files,
                    &[],
                );
            }
            SyntaxKind::AsExpression => {
                if let NodeData::AsExpression(data) = self.data(node) {
                    if let Some(r#type) = data.r#type {
                        self.error_at_node(
                            r#type,
                            &gen::Type_assertion_expressions_can_only_be_used_in_TypeScript_files,
                            &[],
                        );
                    }
                }
            }
            SyntaxKind::SatisfiesExpression => {
                if let NodeData::SatisfiesExpression(data) = self.data(node) {
                    if let Some(r#type) = data.r#type {
                        self.error_at_node(
                            r#type,
                            &gen::Type_satisfaction_expressions_can_only_be_used_in_TypeScript_files,
                            &[],
                        );
                    }
                }
            }
            _ => {}
        }
        self.check_js_decorator_syntax(node);
        match kind {
            SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassExpression
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::FunctionExpression
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::ArrowFunction
            | SyntaxKind::VariableStatement
            | SyntaxKind::PropertyDeclaration => {
                if let Some(list) = self
                    .type_parameters_of(node)
                    .filter(|&list| !self.nodes(Some(list)).is_empty())
                {
                    self.error_at_list(
                        list,
                        &gen::Type_parameter_declarations_can_only_be_used_in_TypeScript_files,
                    );
                }
                let modifiers = self.nodes(self.modifiers_of(node)).to_vec();
                for modifier in modifiers {
                    let modifier_kind = self.kind(modifier);
                    if is_modifier_kind(modifier_kind) && !is_javascript_modifier(modifier_kind) {
                        self.error_at_node(
                            modifier,
                            &gen::The_0_modifier_can_only_be_used_in_TypeScript_files,
                            &[modifier_text(modifier_kind)],
                        );
                    }
                }
            }
            SyntaxKind::Parameter => {
                let modifiers = self.modifiers_of(node);
                if self
                    .nodes(modifiers)
                    .iter()
                    .any(|&modifier| is_modifier_kind(self.kind(modifier)))
                {
                    if let Some(list) = modifiers {
                        self.error_at_list(
                            list,
                            &gen::Parameter_modifiers_can_only_be_used_in_TypeScript_files,
                        );
                    }
                }
            }
            SyntaxKind::CallExpression
            | SyntaxKind::NewExpression
            | SyntaxKind::ExpressionWithTypeArguments
            | SyntaxKind::TaggedTemplateExpression => {
                if let Some(list) = self
                    .type_arguments_of(node)
                    .filter(|&list| !self.nodes(Some(list)).is_empty())
                {
                    self.error_at_list(
                        list,
                        &gen::Type_arguments_can_only_be_used_in_TypeScript_files,
                    );
                }
            }
            _ => {}
        }
    }

    /// tsgo checkJSDecoratorSyntax (parser.go:6715-6767).
    fn check_js_decorator_syntax(&mut self, node: NodeId) {
        let modifiers = self.nodes(self.modifiers_of(node)).to_vec();
        if modifiers.is_empty() {
            return;
        }
        let kind = self.kind(node);
        let is_decorator = |checker: &Self, id: NodeId| checker.kind(id) == SyntaxKind::Decorator;
        if can_have_illegal_decorators(kind) {
            if let Some(&decorator) = modifiers.iter().find(|&&id| is_decorator(self, id)) {
                self.error_at_node(decorator, &gen::Decorators_are_not_valid_here, &[]);
            }
            return;
        }
        if !can_have_decorators(kind) || kind != SyntaxKind::ClassDeclaration {
            return;
        }
        let Some(decorator_index) = modifiers.iter().position(|&id| is_decorator(self, id)) else {
            return;
        };
        let Some(export_index) = modifiers
            .iter()
            .position(|&id| self.kind(id) == SyntaxKind::ExportKeyword)
        else {
            return;
        };
        let default_index = modifiers
            .iter()
            .position(|&id| self.kind(id) == SyntaxKind::DefaultKeyword);
        if decorator_index > export_index
            && default_index.is_some_and(|default_index| decorator_index < default_index)
        {
            // A decorator between `export` and `default`.
            self.error_at_node(
                modifiers[decorator_index],
                &gen::Decorators_are_not_valid_here,
                &[],
            );
        } else if decorator_index < export_index {
            let trailing = modifiers
                .iter()
                .skip(export_index)
                .position(|&id| is_decorator(self, id))
                .map(|offset| export_index + offset);
            if let Some(trailing) = trailing {
                let trailing = self.source.arena.node(modifiers[trailing]);
                let leading = self.source.arena.node(modifiers[decorator_index]);
                let mut diagnostic = self.diagnostic_at(
                    trailing.pos,
                    trailing.end,
                    &gen::Decorators_may_not_appear_after_export_or_export_default_if_they_also_appear_before_export,
                    &[],
                );
                let related = self.diagnostic_at(
                    leading.pos,
                    leading.end,
                    &gen::Decorator_used_before_export_here,
                    &[],
                );
                diagnostic.related.push(RelatedInfo {
                    file_name: related.file_name,
                    start: related.start,
                    length: related.length,
                    message: related.message,
                });
                self.diagnostics.push(diagnostic);
            }
        }
    }
}

fn modifier_text(kind: SyntaxKind) -> &'static str {
    match kind {
        SyntaxKind::AbstractKeyword => "abstract",
        SyntaxKind::AccessorKeyword => "accessor",
        SyntaxKind::AsyncKeyword => "async",
        SyntaxKind::ConstKeyword => "const",
        SyntaxKind::DeclareKeyword => "declare",
        SyntaxKind::DefaultKeyword => "default",
        SyntaxKind::ExportKeyword => "export",
        SyntaxKind::InKeyword => "in",
        SyntaxKind::OutKeyword => "out",
        SyntaxKind::OverrideKeyword => "override",
        SyntaxKind::PrivateKeyword => "private",
        SyntaxKind::ProtectedKeyword => "protected",
        SyntaxKind::PublicKeyword => "public",
        SyntaxKind::ReadonlyKeyword => "readonly",
        SyntaxKind::StaticKeyword => "static",
        _ => "?",
    }
}

/// tsgo IsModifier: a modifier keyword (not a decorator).
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
            | SyntaxKind::OutKeyword
            | SyntaxKind::OverrideKeyword
            | SyntaxKind::PrivateKeyword
            | SyntaxKind::ProtectedKeyword
            | SyntaxKind::PublicKeyword
            | SyntaxKind::ReadonlyKeyword
            | SyntaxKind::StaticKeyword
    )
}

/// tsgo ModifierFlagsJavaScript (ast/modifierflags.go:52): export, static,
/// accessor, async and default.
fn is_javascript_modifier(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ExportKeyword
            | SyntaxKind::StaticKeyword
            | SyntaxKind::AccessorKeyword
            | SyntaxKind::AsyncKeyword
            | SyntaxKind::DefaultKeyword
    )
}

/// tsgo CanHaveIllegalDecorators (ast/utilities.go:1072-1087).
fn can_have_illegal_decorators(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PropertyAssignment
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::IndexSignature
            | SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::MissingDeclaration
            | SyntaxKind::VariableStatement
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportDeclaration
            | SyntaxKind::NamespaceExportDeclaration
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::ExportAssignment
    )
}

/// tsc-port: canHaveDecorators @6.0.3
/// tsc-hash: 55d6b35e1b66572fa2e24ca5a5d956c35dd1bcbef83c1de2d02e4fe8129b7290
/// tsc-span: _tsc.js:28263-28266
pub(crate) fn can_have_decorators(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Parameter
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::ClassExpression
            | SyntaxKind::ClassDeclaration
    )
}

#[cfg(test)]
#[path = "../tests/unit/js_grammar/tests.rs"]
mod tests;
