//! tsc program.ts getJSSyntacticDiagnosticsForFile: the walker that flags
//! TypeScript-only syntax in JavaScript files. Runs over the parsed tree;
//! "skip" outcomes stop descent exactly like tsc's forEachChildRecursively.
//!
//! Known gaps (fields the generated node schema does not carry yet):
//! isTypeOnly (import/export type — part of 8006) and isExportEquals (8003).

use tsrs2_diags::{compute_line_map, gen, Diagnostic, DiagnosticMessage, LineMap, MessageChain};
use tsrs2_syntax::{
    for_each_child, LanguageVariant, NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind,
};
use tsrs2_types::NodeFlags;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Visit {
    Descend,
    Skip,
}

pub(crate) fn get_js_syntactic_diagnostics(
    source: &SourceFile,
    experimental_decorators: bool,
) -> Vec<Diagnostic> {
    let mut walker = JsGrammarWalker {
        source,
        experimental_decorators,
        line_map: compute_line_map(&source.text),
        diagnostics: Vec::new(),
    };
    walker.recurse(source.root);
    walker.diagnostics
}

struct JsGrammarWalker<'a> {
    source: &'a SourceFile,
    experimental_decorators: bool,
    line_map: LineMap,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Default)]
struct Roles {
    question_token: Option<NodeId>,
    r#type: Option<NodeId>,
    type_parameters: Option<NodeArrayId>,
    modifiers: Option<NodeArrayId>,
    type_arguments: Option<NodeArrayId>,
}

impl<'a> JsGrammarWalker<'a> {
    fn kind(&self, id: NodeId) -> SyntaxKind {
        self.source.arena.node(id).kind
    }

    fn to_utf16(&self, byte: usize) -> u32 {
        self.line_map
            .byte_to_utf16
            .get(byte)
            .copied()
            .unwrap_or(byte as u32)
    }

    fn push_span(
        &mut self,
        start: usize,
        end: usize,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let start_utf16 = self.to_utf16(start);
        let end_utf16 = self.to_utf16(end);
        self.diagnostics.push(Diagnostic::new(
            Some(self.source.file_name.clone()),
            Some(start_utf16),
            Some(end_utf16.saturating_sub(start_utf16)),
            MessageChain::new(message, &args),
        ));
    }

    /// tsc createDiagnosticForNodeInSourceFile → getErrorSpanForNode.
    fn push_for_node(&mut self, id: NodeId, message: &'static DiagnosticMessage, args: &[&str]) {
        let (start, end) = self.error_span_for_node(id);
        self.push_span(start, end, message, args);
    }

    /// tsc createDiagnosticForNodeArray: raw array pos, no trivia skip.
    fn push_for_array(&mut self, id: NodeArrayId, message: &'static DiagnosticMessage) {
        let array = self.source.arena.node_array(id);
        self.push_span(array.pos as usize, array.end as usize, message, &[]);
    }

    /// tsc getErrorSpanForNode: named declarations use the name span; other
    /// nodes their trivia-skipped span; a missing name falls back to the span
    /// of the token at the node position.
    fn error_span_for_node(&self, id: NodeId) -> (usize, usize) {
        let node = self.source.arena.node(id);
        let error_node = match node.kind {
            SyntaxKind::VariableDeclaration
            | SyntaxKind::BindingElement
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassExpression
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::EnumMember
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::NamespaceImport => self.name_of(id),
            _ => Some(id),
        };
        match error_node {
            None => self.token_span_at(node.pos as usize),
            Some(error_node) => {
                let node = self.source.arena.node(error_node);
                let pos = if node.pos == node.end {
                    node.pos as usize
                } else {
                    tsrs2_syntax::skip_trivia(&self.source.text, node.pos as usize)
                };
                (pos, node.end as usize)
            }
        }
    }

    fn name_of(&self, id: NodeId) -> Option<NodeId> {
        match &self.source.arena.node(id).data {
            NodeData::VariableDeclaration(data) => data.name,
            NodeData::BindingElement(data) => data.name,
            NodeData::ClassDeclaration(data) => data.name,
            NodeData::ClassExpression(data) => data.name,
            NodeData::InterfaceDeclaration(data) => data.name,
            NodeData::ModuleDeclaration(data) => data.name,
            NodeData::EnumDeclaration(data) => data.name,
            NodeData::EnumMember(data) => data.name,
            NodeData::FunctionDeclaration(data) => data.name,
            NodeData::FunctionExpression(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            NodeData::TypeAliasDeclaration(data) => data.name,
            NodeData::PropertyDeclaration(data) => data.name,
            NodeData::PropertySignature(data) => data.name,
            NodeData::NamespaceImport(data) => data.name,
            _ => None,
        }
    }

    /// tsc getSpanOfTokenAtPosition: one token scanned fresh at `pos`.
    fn token_span_at(&self, pos: usize) -> (usize, usize) {
        let tokens = tsrs2_syntax::scan_tokens(&self.source.text[pos..], LanguageVariant::Standard);
        match tokens.first() {
            Some(token) => (pos + token.start as usize, pos + token.end as usize),
            None => (pos, pos),
        }
    }

    fn token_kind_at(&self, pos: usize) -> Option<SyntaxKind> {
        tsrs2_syntax::scan_tokens(&self.source.text[pos..], LanguageVariant::Standard)
            .first()
            .map(|token| token.kind)
    }

    fn children_of(&self, id: NodeId) -> Vec<NodeId> {
        let mut children = Vec::new();
        for_each_child(&self.source.arena, self.source.arena.node(id), |child| {
            children.push(child);
            false
        });
        children
    }

    fn array_elements(&self, id: NodeArrayId) -> Vec<NodeId> {
        self.source.arena.node_array(id).nodes.clone()
    }

    fn recurse(&mut self, id: NodeId) {
        let kind = self.kind(id);
        let roles = self.roles_for(id, kind);
        let mut skipped: Vec<NodeId> = Vec::new();

        if let Some(modifiers) = roles.modifiers {
            if self.walk_modifiers_array(kind, modifiers) == Visit::Skip {
                skipped.extend(self.array_elements(modifiers));
            }
        }
        if let Some(type_parameters) = roles.type_parameters {
            self.push_for_array(
                type_parameters,
                &gen::Type_parameter_declarations_can_only_be_used_in_TypeScript_files,
            );
            skipped.extend(self.array_elements(type_parameters));
        }
        if let Some(type_arguments) = roles.type_arguments {
            self.push_for_array(
                type_arguments,
                &gen::Type_arguments_can_only_be_used_in_TypeScript_files,
            );
            skipped.extend(self.array_elements(type_arguments));
        }

        for child in self.children_of(id) {
            if skipped.contains(&child) {
                continue;
            }
            if roles.question_token == Some(child) {
                self.push_for_node(
                    child,
                    &gen::The_0_modifier_can_only_be_used_in_TypeScript_files,
                    &["?"],
                );
                continue;
            }
            if roles.r#type == Some(child) {
                self.push_for_node(
                    child,
                    &gen::Type_annotations_can_only_be_used_in_TypeScript_files,
                    &[],
                );
                continue;
            }
            if self.check_node(child, id) == Visit::Descend {
                self.recurse(child);
            }
        }
    }

    /// The parent-side field/array roles the tsc walker keys on.
    fn roles_for(&self, id: NodeId, kind: SyntaxKind) -> Roles {
        let mut roles = Roles::default();
        match &self.source.arena.node(id).data {
            NodeData::Parameter(data) => {
                roles.question_token = data.question_token;
                roles.r#type = data.r#type;
                roles.modifiers = data.modifiers;
            }
            NodeData::PropertyDeclaration(data) => {
                roles.question_token = data.question_token;
                roles.r#type = data.r#type;
                roles.modifiers = data.modifiers;
            }
            NodeData::MethodDeclaration(data) => {
                roles.question_token = data.question_token;
                roles.r#type = data.r#type;
                roles.type_parameters = data.type_parameters;
                roles.modifiers = data.modifiers;
            }
            NodeData::MethodSignature(data) => {
                roles.r#type = data.r#type;
            }
            NodeData::Constructor(data) => {
                roles.r#type = data.r#type;
                roles.type_parameters = data.type_parameters;
                roles.modifiers = data.modifiers;
            }
            NodeData::GetAccessor(data) => {
                roles.r#type = data.r#type;
                roles.type_parameters = data.type_parameters;
                roles.modifiers = data.modifiers;
            }
            NodeData::SetAccessor(data) => {
                roles.r#type = data.r#type;
                roles.type_parameters = data.type_parameters;
                roles.modifiers = data.modifiers;
            }
            NodeData::FunctionExpression(data) => {
                roles.r#type = data.r#type;
                roles.type_parameters = data.type_parameters;
                roles.modifiers = data.modifiers;
            }
            NodeData::FunctionDeclaration(data) => {
                roles.r#type = data.r#type;
                roles.type_parameters = data.type_parameters;
                roles.modifiers = data.modifiers;
            }
            NodeData::ArrowFunction(data) => {
                roles.r#type = data.r#type;
                roles.type_parameters = data.type_parameters;
                roles.modifiers = data.modifiers;
            }
            NodeData::VariableDeclaration(data) => {
                roles.r#type = data.r#type;
            }
            NodeData::ClassDeclaration(data) => {
                roles.type_parameters = data.type_parameters;
                roles.modifiers = data.modifiers;
            }
            NodeData::ClassExpression(data) => {
                roles.type_parameters = data.type_parameters;
                roles.modifiers = data.modifiers;
            }
            NodeData::VariableStatement(data) => {
                roles.modifiers = data.modifiers;
            }
            NodeData::ImportDeclaration(data) => {
                roles.modifiers = data.modifiers;
            }
            NodeData::ExportDeclaration(data) => {
                roles.modifiers = data.modifiers;
            }
            NodeData::ExportAssignment(data) => {
                roles.modifiers = data.modifiers;
            }
            NodeData::IndexSignature(data) => {
                roles.modifiers = data.modifiers;
            }
            NodeData::CallExpression(data) => {
                roles.type_arguments = data.type_arguments;
            }
            NodeData::NewExpression(data) => {
                roles.type_arguments = data.type_arguments;
            }
            NodeData::ExpressionWithTypeArguments(data) => {
                roles.type_arguments = data.type_arguments;
            }
            NodeData::JsxSelfClosingElement(data) => {
                roles.type_arguments = data.type_arguments;
            }
            NodeData::JsxOpeningElement(data) => {
                roles.type_arguments = data.type_arguments;
            }
            NodeData::TaggedTemplateExpression(data) => {
                roles.type_arguments = data.type_arguments;
            }
            _ => {}
        }
        // 8004 only fires for the tsc-listed kinds; MethodSignature and
        // interface-ish members never reach here (their parents skip).
        if roles.type_parameters.is_some()
            && !matches!(
                kind,
                SyntaxKind::ClassDeclaration
                    | SyntaxKind::ClassExpression
                    | SyntaxKind::MethodDeclaration
                    | SyntaxKind::Constructor
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor
                    | SyntaxKind::FunctionExpression
                    | SyntaxKind::FunctionDeclaration
                    | SyntaxKind::ArrowFunction
            )
        {
            roles.type_parameters = None;
        }
        roles
    }

    /// tsc walkArray for a modifiers array: decorator legality first, then
    /// the per-parent-kind modifier rules.
    fn walk_modifiers_array(&mut self, parent_kind: SyntaxKind, modifiers: NodeArrayId) -> Visit {
        let elements = self.array_elements(modifiers);
        let is_decorator = |walker: &Self, id: NodeId| walker.kind(id) == SyntaxKind::Decorator;

        if can_have_illegal_decorators(parent_kind) {
            if let Some(decorator) = elements.iter().copied().find(|id| is_decorator(self, *id)) {
                self.push_for_node(decorator, &gen::Decorators_are_not_valid_here, &[]);
            }
        } else if can_have_decorators(parent_kind) {
            let decorator_index = elements.iter().position(|id| is_decorator(self, *id));
            if let Some(decorator_index) = decorator_index {
                if parent_kind == SyntaxKind::Parameter && !self.experimental_decorators {
                    self.push_for_node(
                        elements[decorator_index],
                        &gen::Decorators_are_not_valid_here,
                        &[],
                    );
                } else if parent_kind == SyntaxKind::ClassDeclaration {
                    let export_index = elements
                        .iter()
                        .position(|id| self.kind(*id) == SyntaxKind::ExportKeyword);
                    if let Some(export_index) = export_index {
                        let default_index = elements
                            .iter()
                            .position(|id| self.kind(*id) == SyntaxKind::DefaultKeyword);
                        if decorator_index > export_index
                            && default_index
                                .is_some_and(|default_index| decorator_index < default_index)
                        {
                            self.push_for_node(
                                elements[decorator_index],
                                &gen::Decorators_are_not_valid_here,
                                &[],
                            );
                        } else if decorator_index < export_index {
                            let trailing = elements
                                .iter()
                                .skip(export_index)
                                .position(|id| is_decorator(self, *id))
                                .map(|offset| export_index + offset);
                            if let Some(trailing) = trailing {
                                self.push_for_node(
                                    elements[trailing],
                                    &gen::Decorators_may_not_appear_after_export_or_export_default_if_they_also_appear_before_export,
                                    &[],
                                );
                            }
                        }
                    }
                }
            }
        }

        match parent_kind {
            SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassExpression
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::FunctionExpression
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::ArrowFunction => {
                self.check_modifiers(&elements, false);
                Visit::Skip
            }
            SyntaxKind::VariableStatement => {
                self.check_modifiers(&elements, true);
                Visit::Skip
            }
            SyntaxKind::PropertyDeclaration => {
                for modifier in elements {
                    let kind = self.kind(modifier);
                    if is_modifier_token(kind)
                        && kind != SyntaxKind::StaticKeyword
                        && kind != SyntaxKind::AccessorKeyword
                    {
                        self.push_for_node(
                            modifier,
                            &gen::The_0_modifier_can_only_be_used_in_TypeScript_files,
                            &[modifier_text(kind)],
                        );
                    }
                }
                Visit::Skip
            }
            SyntaxKind::Parameter => {
                if elements.iter().any(|id| is_modifier_token(self.kind(*id))) {
                    self.push_for_array(
                        modifiers,
                        &gen::Parameter_modifiers_can_only_be_used_in_TypeScript_files,
                    );
                    Visit::Skip
                } else {
                    Visit::Descend
                }
            }
            _ => Visit::Descend,
        }
    }

    /// tsc checkModifiers.
    fn check_modifiers(&mut self, modifiers: &[NodeId], is_const_valid: bool) {
        for modifier in modifiers {
            let kind = self.kind(*modifier);
            match kind {
                SyntaxKind::ConstKeyword if is_const_valid => {}
                SyntaxKind::ConstKeyword
                | SyntaxKind::PublicKeyword
                | SyntaxKind::PrivateKeyword
                | SyntaxKind::ProtectedKeyword
                | SyntaxKind::ReadonlyKeyword
                | SyntaxKind::DeclareKeyword
                | SyntaxKind::AbstractKeyword
                | SyntaxKind::OverrideKeyword
                | SyntaxKind::InKeyword
                | SyntaxKind::OutKeyword => {
                    self.push_for_node(
                        *modifier,
                        &gen::The_0_modifier_can_only_be_used_in_TypeScript_files,
                        &[modifier_text(kind)],
                    );
                }
                _ => {}
            }
        }
    }

    /// The tsc walk() node switch.
    fn check_node(&mut self, id: NodeId, parent: NodeId) -> Visit {
        match self.kind(id) {
            SyntaxKind::ImportClause => {
                if let NodeData::ImportClause(data) = &self.source.arena.node(id).data {
                    if data.is_type_only {
                        // tsc reports at the parent ImportDeclaration.
                        self.push_for_node(
                            parent,
                            &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                            &["import type"],
                        );
                        return Visit::Skip;
                    }
                }
                Visit::Descend
            }
            SyntaxKind::ExportDeclaration => {
                if let NodeData::ExportDeclaration(data) = &self.source.arena.node(id).data {
                    if data.is_type_only {
                        self.push_for_node(
                            id,
                            &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                            &["export type"],
                        );
                        return Visit::Skip;
                    }
                }
                Visit::Descend
            }
            SyntaxKind::ImportSpecifier | SyntaxKind::ExportSpecifier => {
                let (is_type_only, is_import) = match &self.source.arena.node(id).data {
                    NodeData::ImportSpecifier(data) => (data.is_type_only, true),
                    NodeData::ExportSpecifier(data) => (data.is_type_only, false),
                    _ => (false, false),
                };
                if is_type_only {
                    self.push_for_node(
                        id,
                        &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                        &[if is_import {
                            "import...type"
                        } else {
                            "export...type"
                        }],
                    );
                    return Visit::Skip;
                }
                Visit::Descend
            }
            SyntaxKind::ExportAssignment => {
                if let NodeData::ExportAssignment(data) = &self.source.arena.node(id).data {
                    if data.is_export_equals == Some(true) {
                        self.push_for_node(
                            id,
                            &gen::export_can_only_be_used_in_TypeScript_files,
                            &[],
                        );
                        return Visit::Skip;
                    }
                }
                Visit::Descend
            }
            SyntaxKind::ImportEqualsDeclaration => {
                self.push_for_node(id, &gen::import_can_only_be_used_in_TypeScript_files, &[]);
                Visit::Skip
            }
            SyntaxKind::HeritageClause => {
                let pos = tsrs2_syntax::skip_trivia(
                    &self.source.text,
                    self.source.arena.node(id).pos as usize,
                );
                if self.token_kind_at(pos) == Some(SyntaxKind::ImplementsKeyword) {
                    self.push_for_node(
                        id,
                        &gen::implements_clauses_can_only_be_used_in_TypeScript_files,
                        &[],
                    );
                    Visit::Skip
                } else {
                    Visit::Descend
                }
            }
            SyntaxKind::InterfaceDeclaration => {
                self.push_for_node(
                    id,
                    &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                    &["interface"],
                );
                Visit::Skip
            }
            SyntaxKind::ModuleDeclaration => {
                let keyword = if NodeFlags::from_bits(self.source.arena.node(id).flags)
                    .contains(NodeFlags::NAMESPACE)
                {
                    "namespace"
                } else {
                    "module"
                };
                self.push_for_node(
                    id,
                    &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                    &[keyword],
                );
                Visit::Skip
            }
            SyntaxKind::TypeAliasDeclaration => {
                self.push_for_node(
                    id,
                    &gen::Type_aliases_can_only_be_used_in_TypeScript_files,
                    &[],
                );
                Visit::Skip
            }
            SyntaxKind::Constructor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::FunctionDeclaration => {
                let body = match &self.source.arena.node(id).data {
                    NodeData::Constructor(data) => data.body,
                    NodeData::MethodDeclaration(data) => data.body,
                    NodeData::FunctionDeclaration(data) => data.body,
                    _ => None,
                };
                if body.is_none() {
                    self.push_for_node(
                        id,
                        &gen::Signature_declarations_can_only_be_used_in_TypeScript_files,
                        &[],
                    );
                    Visit::Skip
                } else {
                    Visit::Descend
                }
            }
            SyntaxKind::EnumDeclaration => {
                self.push_for_node(
                    id,
                    &gen::_0_declarations_can_only_be_used_in_TypeScript_files,
                    &["enum"],
                );
                Visit::Skip
            }
            SyntaxKind::NonNullExpression => {
                self.push_for_node(
                    id,
                    &gen::Non_null_assertions_can_only_be_used_in_TypeScript_files,
                    &[],
                );
                Visit::Skip
            }
            SyntaxKind::AsExpression => {
                let r#type = match &self.source.arena.node(id).data {
                    NodeData::AsExpression(data) => data.r#type,
                    _ => None,
                };
                if let Some(r#type) = r#type {
                    self.push_for_node(
                        r#type,
                        &gen::Type_assertion_expressions_can_only_be_used_in_TypeScript_files,
                        &[],
                    );
                }
                Visit::Skip
            }
            SyntaxKind::SatisfiesExpression => {
                let r#type = match &self.source.arena.node(id).data {
                    NodeData::SatisfiesExpression(data) => data.r#type,
                    _ => None,
                };
                if let Some(r#type) = r#type {
                    self.push_for_node(
                        r#type,
                        &gen::Type_satisfaction_expressions_can_only_be_used_in_TypeScript_files,
                        &[],
                    );
                }
                Visit::Skip
            }
            _ => Visit::Descend,
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

fn is_modifier_token(kind: SyntaxKind) -> bool {
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

/// tsc canHaveIllegalDecorators.
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
