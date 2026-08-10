//! H2.5a ESNext-to-standard lowering.
//!
//! The pinned TypeScript transformer is the semantic oracle, but its nested
//! closure graph is deliberately not mirrored here.  Explicit resource
//! management is represented as typed disposal modes and scope plans, while
//! generated syntax remains owned by the transform arena.

use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags, ScriptTarget};

use crate::{
    EmitFlags, EmitHelper, TransformArena, TransformError, TransformFlags, TransformNode,
    TransformNodeArray, TransformRoot, TransformSourceId, TransformationContext, Transformer,
};

use super::{flags_after_update, system::collect_identifier_texts};

const ADD_DISPOSABLE_RESOURCE_HELPER_TEXT: &str = r#"var __addDisposableResource = (this && this.__addDisposableResource) || function (env, value, async) {
    if (value !== null && value !== void 0) {
        if (typeof value !== "object" && typeof value !== "function") throw new TypeError("Object expected.");
        var dispose, inner;
        if (async) {
            if (!Symbol.asyncDispose) throw new TypeError("Symbol.asyncDispose is not defined.");
            dispose = value[Symbol.asyncDispose];
        }
        if (dispose === void 0) {
            if (!Symbol.dispose) throw new TypeError("Symbol.dispose is not defined.");
            dispose = value[Symbol.dispose];
            if (async) inner = dispose;
        }
        if (typeof dispose !== "function") throw new TypeError("Object not disposable.");
        if (inner) dispose = function() { try { inner.call(this); } catch (e) { return Promise.reject(e); } };
        env.stack.push({ value: value, dispose: dispose, async: async });
    }
    else if (async) {
        env.stack.push({ async: true });
    }
    return value;
};"#;

const DISPOSE_RESOURCES_HELPER_TEXT: &str = r#"var __disposeResources = (this && this.__disposeResources) || (function (SuppressedError) {
    return function (env) {
        function fail(e) {
            env.error = env.hasError ? new SuppressedError(e, env.error, "An error was suppressed during disposal.") : e;
            env.hasError = true;
        }
        var r, s = 0;
        function next() {
            while (r = env.stack.pop()) {
                try {
                    if (!r.async && s === 1) return s = 0, env.stack.push(r), Promise.resolve().then(next);
                    if (r.dispose) {
                        var result = r.dispose.call(r.value);
                        if (r.async) return s |= 2, Promise.resolve(result).then(next, function(e) { fail(e); return next(); });
                    }
                    else s |= 1;
                }
                catch (e) {
                    fail(e);
                }
            }
            if (s === 1) return env.hasError ? Promise.reject(env.error) : Promise.resolve();
            if (env.hasError) throw env.error;
        }
        return next();
    };
})(typeof SuppressedError === "function" ? SuppressedError : function (error, suppressed, message) {
    var e = new Error(message);
    return e.name = "SuppressedError", e.error = error, e.suppressed = suppressed, e;
});"#;

/// tsc-port: transformESNext @6.0.3
/// tsc-hash: 18658b254a7b4f2a9bc6759024d948076b6e63d78aed2a90f1c51cd012e4cead
/// tsc-span: _tsc.js:103278-103813
pub(super) fn transform_es_next(options: &CompilerOptions) -> Box<dyn Transformer> {
    Box::new(EsNextTransformer {
        target: options.emit_script_target(),
    })
}

struct EsNextTransformer {
    target: ScriptTarget,
}

impl Transformer for EsNextTransformer {
    fn name(&self) -> &'static str {
        "transformESNext"
    }

    fn initialize(&mut self, _context: &mut TransformationContext) -> Result<(), TransformError> {
        if self.target < ScriptTarget::ES2018 || self.target >= ScriptTarget::ES_NEXT {
            return Err(TransformError::UnsupportedCompilerOption {
                option: "ESNext transform",
                detail: "transformESNext is admitted only for the closed target band below ESNext",
            });
        }
        Ok(())
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
            return Ok(root);
        }
        let current_root = context.arena().root(source)?;
        let mut visitor = EsNextVisitor::new(context, source);
        visitor.plan_disposal_scopes(current_root)?;
        let transformed =
            visitor
                .visit(current_root.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "root",
                })?;
        let transformed = TransformNode::new(source, transformed);
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DisposalMode {
    Sync,
    Async,
}

impl DisposalMode {
    const fn is_async(self) -> bool {
        matches!(self, Self::Async)
    }

    const fn merge(self, other: Self) -> Self {
        if self.is_async() || other.is_async() {
            Self::Async
        } else {
            Self::Sync
        }
    }
}

#[derive(Clone)]
struct DisposalScope {
    environment_name: String,
    catch_name: String,
    result_name: Option<String>,
    mode: DisposalMode,
}

#[derive(Default)]
struct TopLevelPlan {
    outside: Vec<TransformNode>,
    hoisted_bindings: Vec<TransformNode>,
    body: Vec<TransformNode>,
    export_specifiers: Vec<TransformNode>,
    exported_variables: Vec<TransformNode>,
    export_equals: Option<TransformNode>,
    default_export_name: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NamedEvaluationOutcome {
    expression: TransformNode,
    applied: bool,
}

impl NamedEvaluationOutcome {
    const fn unchanged(expression: TransformNode) -> Self {
        Self {
            expression,
            applied: false,
        }
    }

    const fn applied(expression: TransformNode) -> Self {
        Self {
            expression,
            applied: true,
        }
    }
}

struct EsNextVisitor<'context> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
    used_names: BTreeSet<String>,
    generated_ordinals: BTreeMap<String, usize>,
    disposal_scopes: BTreeMap<NodeId, DisposalScope>,
    function_body_blocks: BTreeSet<NodeId>,
}

struct DirectChildCollector<'arena> {
    arena: &'arena TransformArena,
    source: TransformSourceId,
    children: Vec<NodeId>,
}

impl<'context> EsNextVisitor<'context> {
    fn new(context: &'context mut TransformationContext, source: TransformSourceId) -> Self {
        Self {
            used_names: collect_identifier_texts(context.arena(), source),
            context,
            source,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
            generated_ordinals: BTreeMap::new(),
            disposal_scopes: BTreeMap::new(),
            function_body_blocks: BTreeSet::new(),
        }
    }

    /// TypeScript assigns generated names when printing a name-generation
    /// scope, before printing nested scopes. Plan disposal bindings in that
    /// order so eager Rust-owned identifiers retain the same observable names.
    fn plan_disposal_scopes(&mut self, root: TransformNode) -> Result<(), TransformError> {
        self.plan_name_generation_scope(root)
    }

    fn plan_name_generation_scope(&mut self, root: TransformNode) -> Result<(), TransformError> {
        let mut nested_scopes = Vec::new();
        self.plan_node_in_name_scope(root, true, &mut nested_scopes)?;
        for nested_scope in nested_scopes {
            self.plan_name_generation_scope(nested_scope)?;
        }
        Ok(())
    }

    fn plan_node_in_name_scope(
        &mut self,
        node: TransformNode,
        is_scope_root: bool,
        nested_scopes: &mut Vec<TransformNode>,
    ) -> Result<(), TransformError> {
        let record = self.context.arena().node(node)?.clone();
        if !is_scope_root && Self::establishes_name_generation_scope(record.kind) {
            nested_scopes.push(node);
            return Ok(());
        }

        if let Some(body) = Self::function_body(&record.data) {
            self.function_body_blocks.insert(body);
        }

        let mode = match &record.data {
            NodeData::SourceFile(data) => {
                let statements = self.array_nodes(data.statements)?;
                self.statements_mode(&statements)?
            }
            NodeData::Block(data) => {
                let statements = self.array_nodes(data.statements)?;
                self.statements_mode(&statements)?
            }
            _ => None,
        };

        let environment_name = mode.map(|_| self.allocate_generated_name("env"));
        for child in self.direct_children(record.data)? {
            self.plan_node_in_name_scope(child, false, nested_scopes)?;
        }
        if let (Some(mode), Some(environment_name)) = (mode, environment_name) {
            let catch_name = self.allocate_generated_name("e");
            let result_name = mode
                .is_async()
                .then(|| self.allocate_generated_name("result"));
            self.disposal_scopes.insert(
                node.node(),
                DisposalScope {
                    environment_name,
                    catch_name,
                    result_name,
                    mode,
                },
            );
        }
        Ok(())
    }

    const fn establishes_name_generation_scope(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::ArrowFunction
                | SyntaxKind::ClassDeclaration
                | SyntaxKind::ClassExpression
                | SyntaxKind::ClassStaticBlockDeclaration
                | SyntaxKind::Constructor
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::GetAccessor
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::ModuleBlock
                | SyntaxKind::ObjectLiteralExpression
                | SyntaxKind::SetAccessor
                | SyntaxKind::SourceFile
        )
    }

    const fn function_body(data: &NodeData) -> Option<NodeId> {
        match data {
            NodeData::ArrowFunction(data) => data.body,
            NodeData::Constructor(data) => data.body,
            NodeData::FunctionDeclaration(data) => data.body,
            NodeData::FunctionExpression(data) => data.body,
            NodeData::GetAccessor(data) => data.body,
            NodeData::MethodDeclaration(data) => data.body,
            NodeData::SetAccessor(data) => data.body,
            _ => None,
        }
    }

    fn direct_children(&self, mut data: NodeData) -> Result<Vec<TransformNode>, TransformError> {
        let mut collector = DirectChildCollector {
            arena: self.context.arena(),
            source: self.source,
            children: Vec::new(),
        };
        try_visit_each_child(&mut data, &mut collector)?;
        Ok(collector
            .children
            .into_iter()
            .map(|child| self.node(child))
            .collect())
    }

    fn disposal_scope_for(
        &mut self,
        node: TransformNode,
        mode: DisposalMode,
    ) -> Result<DisposalScope, TransformError> {
        if let Some(scope) = self.disposal_scopes.get(&node.node()) {
            if scope.mode != mode {
                return Err(TransformError::RequiredChildRemoved {
                    parent: self.context.arena().node(node)?.kind,
                    field: "planned disposal mode",
                });
            }
            return Ok(scope.clone());
        }

        // Synthetic blocks created while lowering `for (using ...)` have no
        // parsed identity to pre-plan. They still own a complete typed scope.
        Ok(DisposalScope {
            environment_name: self.allocate_generated_name("env"),
            catch_name: self.allocate_generated_name("e"),
            result_name: mode
                .is_async()
                .then(|| self.allocate_generated_name("result")),
            mode,
        })
    }

    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if let Some(mapped) = self.nodes.get(&id) {
            return Ok(*mapped);
        }
        let original = self.node(id);
        let record = self.context.arena().node(original)?.clone();
        // Earlier transforms may synthesize wrappers around still-live ESNext
        // syntax. Their transform flags are an optimization hint, not proof
        // that the subtree is semantically inert, so this pass walks the owned
        // tree and lets the typed node dispatch decide what changes.
        let transformed = match record.data {
            NodeData::SourceFile(data) => Some(self.visit_source_file(original, data)?),
            NodeData::Block(data) => Some(self.visit_block(original, data)?),
            NodeData::ForStatement(data) => Some(self.visit_for_statement(original, data)?),
            NodeData::ForOfStatement(data) => Some(self.visit_for_of_statement(original, data)?),
            NodeData::Token => Some(id),
            data => Some(self.update_generic(original, data)?),
        };
        self.nodes.insert(id, transformed);
        Ok(transformed)
    }

    fn visit_source_file(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::SourceFileData,
    ) -> Result<NodeId, TransformError> {
        let statements = self.array_nodes(data.statements)?;
        let Some(mode) = self.statements_mode(&statements)? else {
            return self.update_generic(original, NodeData::SourceFile(data));
        };
        let prologue_count = self.count_prologues(&statements)?;
        let first_using = statements[prologue_count..]
            .iter()
            .position(|statement| self.statement_mode(*statement).is_some())
            .map(|offset| offset + prologue_count)
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SourceFile,
                field: "using statement",
            })?;

        let scope = self.disposal_scope_for(original, mode)?;
        let mut plan = TopLevelPlan::default();
        for statement in &statements[..first_using] {
            if let Some(visited) = self.visit(statement.node())? {
                plan.outside.push(self.node(visited));
            }
        }
        for statement in &statements[first_using..] {
            let lowered = self.lower_statement(*statement, &scope)?;
            for lowered in lowered {
                self.hoist_top_level(lowered, &mut plan)?;
            }
        }

        if !plan.export_specifiers.is_empty() {
            let specifiers = std::mem::take(&mut plan.export_specifiers);
            plan.outside
                .push(self.create_export_declaration(specifiers)?);
        }
        if !plan.hoisted_bindings.is_empty() {
            let bindings = std::mem::take(&mut plan.hoisted_bindings);
            plan.outside
                .push(self.create_variable_statement_from_declarations(
                    bindings,
                    NodeFlags::NONE,
                    None,
                )?);
        }
        if !plan.exported_variables.is_empty() {
            let variables = std::mem::take(&mut plan.exported_variables);
            let export = self.create_modifier_array(&[SyntaxKind::ExportKeyword])?;
            plan.outside
                .push(self.create_variable_statement_from_declarations(
                    variables,
                    NodeFlags::LET,
                    export,
                )?);
        }
        let body = std::mem::take(&mut plan.body);
        plan.outside
            .extend(self.create_disposal_statements(body, &scope)?);
        if let Some(export_equals) = plan.export_equals {
            plan.outside.push(export_equals);
        }
        let statements = self
            .context
            .factory()?
            .create_node_array(self.source, plan.outside)?;
        data.statements = Some(statements.array());
        let flags = flags_after_update(
            self.context.arena(),
            original,
            &NodeData::SourceFile(data.clone()),
        )?;
        Ok(self
            .context
            .factory()?
            .update_node(original, NodeData::SourceFile(data), flags)?
            .node())
    }

    fn visit_block(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::BlockData,
    ) -> Result<NodeId, TransformError> {
        let statements = self.array_nodes(data.statements)?;
        let Some(mode) = self.statements_mode(&statements)? else {
            return self.update_generic(original, NodeData::Block(data));
        };
        let scope = self.disposal_scope_for(original, mode)?;
        let prologue_count = self.count_prologues(&statements)?;
        let mut output = Vec::with_capacity(statements.len() + 2);
        for statement in &statements[..prologue_count] {
            if let Some(visited) = self.visit(statement.node())? {
                output.push(self.node(visited));
            }
        }
        let mut body = Vec::new();
        for statement in &statements[prologue_count..] {
            body.extend(self.lower_statement(*statement, &scope)?);
        }
        output.extend(self.create_disposal_statements(body, &scope)?);
        let statement_array = self
            .context
            .factory()?
            .create_node_array(self.source, output)?;
        data.statements = Some(statement_array.array());
        let flags = flags_after_update(
            self.context.arena(),
            original,
            &NodeData::Block(data.clone()),
        )?;
        let updated =
            self.context
                .factory()?
                .update_node(original, NodeData::Block(data), flags)?;
        if self.function_body_blocks.contains(&original.node()) {
            // `visitFunctionBody` retains the parsed body's layout intent.
            // A one-line body therefore keeps its outer braces even when its
            // synthetic try/catch/finally contents span multiple lines.
            Ok(updated.node())
        } else {
            // Ordinary transformed blocks are promoted to multiline, matching
            // transformESNext's `transformUsingDeclarations` block boundary.
            Ok(self
                .context
                .factory()?
                .set_multi_line(updated, true)?
                .node())
        }
    }

    fn visit_for_statement(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ForStatementData,
    ) -> Result<NodeId, TransformError> {
        let Some(initializer) = data
            .initializer
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        else {
            return self.update_generic(original, NodeData::ForStatement(data));
        };
        if self.declaration_list_mode(initializer)?.is_none() {
            return self.update_generic(original, NodeData::ForStatement(data));
        }
        let using_statement = self.create_variable_statement_from_list(initializer)?;
        let condition = self.visit_optional_node(data.condition)?;
        let incrementor = self.visit_optional_node(data.incrementor)?;
        let statement = self.visit_optional_node(data.statement)?;
        if let Some(statement) = statement {
            let statement = self.node(statement);
            if self.context.arena().node(statement)?.kind == SyntaxKind::Block {
                // Removing a `using` initializer makes the updated loop a
                // synthetic child of the disposal block. TypeScript's printer
                // expands that loop body even when the parsed body was on one
                // line, independently of function-body layout preservation.
                self.context.factory()?.set_multi_line(statement, true)?;
            }
        }
        let for_statement = self.context.factory()?.create_node(
            self.source,
            NodeData::ForStatement(tsc_syntax::nodes::ForStatementData {
                initializer: None,
                condition,
                incrementor,
                statement,
            }),
            TransformFlags::NONE,
        )?;
        self.set_original_and_range(for_statement, original)?;
        let block = self.create_block(vec![using_statement, for_statement], true)?;
        self.visit_synthetic_disposal_block(block)
    }

    fn visit_for_of_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ForOfStatementData,
    ) -> Result<NodeId, TransformError> {
        let Some(initializer) = data
            .initializer
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        else {
            return self.update_generic(original, NodeData::ForOfStatement(data));
        };
        let Some(mode) = self.declaration_list_mode(initializer)? else {
            return self.update_generic(original, NodeData::ForOfStatement(data));
        };
        let declaration_ids = match &self.context.arena().node(initializer)?.data {
            NodeData::VariableDeclarationList(list) => self.array_node_ids(list.declarations)?,
            _ => Vec::new(),
        };
        let first = declaration_ids.first().copied();
        let (binding_name, original_declaration) = if let Some(declaration) = first {
            let declaration_node = self.node(declaration);
            let name = match &self.context.arena().node(declaration_node)?.data {
                NodeData::VariableDeclaration(data) => data.name,
                _ => None,
            };
            (name, Some(declaration_node))
        } else {
            (None, None)
        };
        let base = binding_name
            .and_then(|name| self.identifier_text(self.node(name)).map(str::to_owned))
            .unwrap_or_else(|| "value".to_owned());
        let temp_name = self.allocate_generated_name(&base);
        let temp = self.create_identifier(&temp_name)?;
        let loop_declaration = self.create_variable_declaration(temp, None)?;
        data.initializer = Some(
            self.create_variable_declaration_list(vec![loop_declaration], NodeFlags::CONST)?
                .node(),
        );
        data.await_modifier = self.visit_optional_node(data.await_modifier)?;
        data.expression = self.visit_optional_node(data.expression)?;

        let using_name = binding_name.unwrap_or(temp.node());
        let using_declaration = if let Some(original_declaration) = original_declaration {
            let NodeData::VariableDeclaration(mut declaration) = self
                .context
                .arena()
                .node(original_declaration)?
                .data
                .clone()
            else {
                unreachable!("for-of declaration remains a variable declaration")
            };
            declaration.name = Some(using_name);
            declaration.exclamation_token = None;
            declaration.r#type = None;
            declaration.initializer = Some(temp.node());
            let flags = flags_after_update(
                self.context.arena(),
                original_declaration,
                &NodeData::VariableDeclaration(declaration.clone()),
            )?;
            self.context.factory()?.update_node(
                original_declaration,
                NodeData::VariableDeclaration(declaration),
                flags,
            )?
        } else {
            self.create_variable_declaration(self.node(using_name), Some(temp))?
        };
        let using_list = self.create_variable_declaration_list(
            vec![using_declaration],
            match mode {
                DisposalMode::Sync => NodeFlags::USING,
                DisposalMode::Async => NodeFlags::AWAIT_USING,
            },
        )?;
        let using_statement = self.create_variable_statement_from_list(using_list)?;
        let body = if let Some(statement) = data.statement {
            let statement = self.node(statement);
            match self.context.arena().node(statement)?.data.clone() {
                NodeData::Block(mut block) => {
                    let mut statements = vec![using_statement];
                    statements.extend(self.array_nodes(block.statements)?);
                    let array = self
                        .context
                        .factory()?
                        .create_node_array(self.source, statements)?;
                    block.statements = Some(array.array());
                    let flags = flags_after_update(
                        self.context.arena(),
                        statement,
                        &NodeData::Block(block.clone()),
                    )?;
                    self.context
                        .factory()?
                        .update_node(statement, NodeData::Block(block), flags)?
                }
                _ => self.create_block(vec![using_statement, statement], true)?,
            }
        } else {
            self.create_block(vec![using_statement], true)?
        };
        data.statement = Some(self.visit_synthetic_disposal_block(body)?);
        let flags = flags_after_update(
            self.context.arena(),
            original,
            &NodeData::ForOfStatement(data.clone()),
        )?;
        Ok(self
            .context
            .factory()?
            .update_node(original, NodeData::ForOfStatement(data), flags)?
            .node())
    }

    fn visit_synthetic_disposal_block(
        &mut self,
        block: TransformNode,
    ) -> Result<NodeId, TransformError> {
        let NodeData::Block(data) = self.context.arena().node(block)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(block)?.kind,
                field: "synthetic disposal block",
            });
        };
        self.visit_block(block, data)
    }

    fn lower_statement(
        &mut self,
        statement: TransformNode,
        scope: &DisposalScope,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let Some(mode) = self.statement_mode(statement) else {
            return Ok(self
                .visit(statement.node())?
                .map(|id| vec![self.node(id)])
                .unwrap_or_default());
        };
        let NodeData::VariableStatement(statement_data) =
            self.context.arena().node(statement)?.data.clone()
        else {
            unreachable!("a using statement is a variable statement")
        };
        let list = statement_data
            .declaration_list
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::VariableStatement,
                field: "declaration_list",
            })?;
        let NodeData::VariableDeclarationList(list_data) =
            self.context.arena().node(list)?.data.clone()
        else {
            unreachable!("using statement list remains a declaration list")
        };
        let declarations = self.array_nodes(list_data.declarations)?;
        if declarations.iter().any(|declaration| {
            !matches!(
                self.context.arena().node(*declaration).map(|record| &record.data),
                Ok(NodeData::VariableDeclaration(data))
                    if data.name.is_some_and(|name| self.context.arena().node(self.node(name)).is_ok_and(|name| name.kind == SyntaxKind::Identifier))
            )
        }) {
            return Ok(self
                .visit_regular_using_statement(statement)?
                .map(|node| vec![node])
                .unwrap_or_default());
        }

        let environment = self.create_identifier(&scope.environment_name)?;
        let mut lowered = Vec::with_capacity(declarations.len());
        for declaration in declarations {
            let NodeData::VariableDeclaration(mut data) =
                self.context.arena().node(declaration)?.data.clone()
            else {
                unreachable!("checked variable declaration")
            };
            data.name = self.visit_optional_node(data.name)?;
            data.exclamation_token = None;
            data.r#type = None;
            let initializer = match self.visit_optional_node(data.initializer)? {
                Some(initializer) => self.node(initializer),
                None => self.create_void_zero()?,
            };
            let initializer = self.ensure_named_evaluation(data.name, initializer)?;
            data.initializer = Some(
                self.create_add_disposable_resource_call(
                    environment,
                    initializer,
                    mode.is_async(),
                )?
                .node(),
            );
            let flags = flags_after_update(
                self.context.arena(),
                declaration,
                &NodeData::VariableDeclaration(data.clone()),
            )?;
            lowered.push(self.context.factory()?.update_node(
                declaration,
                NodeData::VariableDeclaration(data),
                flags,
            )?);
        }
        let declaration_list = self.create_variable_declaration_list(lowered, NodeFlags::CONST)?;
        self.set_original_and_range(declaration_list, list)?;
        let updated_data = NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
            modifiers: None,
            declaration_list: Some(declaration_list.node()),
        });
        let flags = flags_after_update(self.context.arena(), statement, &updated_data)?;
        let updated = self
            .context
            .factory()?
            .update_node(statement, updated_data, flags)?;
        Ok(vec![updated])
    }

    fn visit_regular_using_statement(
        &mut self,
        original: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let mut data = self.context.arena().node(original)?.data.clone();
        try_visit_each_child(&mut data, self)?;
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        Ok(Some(
            self.context.factory()?.update_node(original, data, flags)?,
        ))
    }

    fn create_disposal_statements(
        &mut self,
        body: Vec<TransformNode>,
        scope: &DisposalScope,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let environment = self.create_identifier(&scope.environment_name)?;
        let empty_stack = self.create_array_literal(Vec::new())?;
        let stack = self.create_property_assignment("stack", empty_stack)?;
        let void_zero = self.create_void_zero()?;
        let error = self.create_property_assignment("error", void_zero)?;
        let false_value = self.create_boolean(false)?;
        let has_error = self.create_property_assignment("hasError", false_value)?;
        let environment_object = self.create_object_literal(vec![stack, error, has_error])?;
        let environment_declaration =
            self.create_variable_declaration(environment, Some(environment_object))?;
        let environment_statement = self.create_variable_statement_from_declarations(
            vec![environment_declaration],
            NodeFlags::CONST,
            None,
        )?;

        let try_block = self.create_block(body, true)?;
        let catch_identifier = self.create_identifier(&scope.catch_name)?;
        let catch_declaration = self.create_variable_declaration(catch_identifier, None)?;
        let environment_for_error = self.create_identifier(&scope.environment_name)?;
        let error_target = self.create_property_access(environment_for_error, "error")?;
        let error_value = self.create_identifier(&scope.catch_name)?;
        let error_assignment = self.create_assignment(error_target, error_value)?;
        let set_error = self.create_expression_statement(error_assignment)?;
        let environment_for_flag = self.create_identifier(&scope.environment_name)?;
        let flag_target = self.create_property_access(environment_for_flag, "hasError")?;
        let true_value = self.create_boolean(true)?;
        let flag_assignment = self.create_assignment(flag_target, true_value)?;
        let set_flag = self.create_expression_statement(flag_assignment)?;
        let catch_block = self.create_block(vec![set_error, set_flag], true)?;
        let catch_clause = self.context.factory()?.create_node(
            self.source,
            NodeData::CatchClause(tsc_syntax::nodes::CatchClauseData {
                variable_declaration: Some(catch_declaration.node()),
                block: Some(catch_block.node()),
            }),
            TransformFlags::NONE,
        )?;

        let dispose_environment = self.create_identifier(&scope.environment_name)?;
        let dispose = self.create_dispose_resources_call(dispose_environment)?;
        let finally_statements =
            match scope.mode {
                DisposalMode::Sync => vec![self.create_expression_statement(dispose)?],
                DisposalMode::Async => {
                    let result_name = scope.result_name.as_deref().ok_or(
                        TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::Block,
                            field: "async disposal result binding",
                        },
                    )?;
                    let result_identifier = self.create_identifier(result_name)?;
                    let declaration =
                        self.create_variable_declaration(result_identifier, Some(dispose))?;
                    let statement = self.create_variable_statement_from_declarations(
                        vec![declaration],
                        NodeFlags::CONST,
                        None,
                    )?;
                    let condition = self.create_identifier(result_name)?;
                    let awaited_identifier = self.create_identifier(result_name)?;
                    let awaited = self.context.factory()?.create_node(
                        self.source,
                        NodeData::AwaitExpression(tsc_syntax::nodes::AwaitExpressionData {
                            expression: Some(awaited_identifier.node()),
                        }),
                        TransformFlags::CONTAINS_AWAIT,
                    )?;
                    let awaited = self.create_expression_statement(awaited)?;
                    let if_statement = self.context.factory()?.create_node(
                        self.source,
                        NodeData::IfStatement(tsc_syntax::nodes::IfStatementData {
                            expression: Some(condition.node()),
                            then_statement: Some(awaited.node()),
                            else_statement: None,
                        }),
                        TransformFlags::CONTAINS_AWAIT,
                    )?;
                    vec![statement, if_statement]
                }
            };
        let finally_block = self.create_block(finally_statements, true)?;
        let try_statement = self.context.factory()?.create_node(
            self.source,
            NodeData::TryStatement(tsc_syntax::nodes::TryStatementData {
                try_block: Some(try_block.node()),
                catch_clause: Some(catch_clause.node()),
                finally_block: Some(finally_block.node()),
            }),
            TransformFlags::NONE,
        )?;
        Ok(vec![environment_statement, try_statement])
    }

    fn hoist_top_level(
        &mut self,
        statement: TransformNode,
        plan: &mut TopLevelPlan,
    ) -> Result<(), TransformError> {
        let record = self.context.arena().node(statement)?.clone();
        match record.data {
            NodeData::ImportDeclaration(_)
            | NodeData::ImportEqualsDeclaration(_)
            | NodeData::ExportDeclaration(_)
            | NodeData::FunctionDeclaration(_) => plan.outside.push(statement),
            NodeData::ExportAssignment(data) => {
                self.hoist_export_assignment(statement, data, plan)?;
            }
            NodeData::ClassDeclaration(data) => {
                if let Some(statement) = self.hoist_class(statement, data, plan)? {
                    plan.body.push(statement);
                }
            }
            NodeData::VariableStatement(data) => {
                if let Some(statement) = self.hoist_variable_statement(statement, data, plan)? {
                    plan.body.push(statement);
                }
            }
            _ => plan.body.push(statement),
        }
        Ok(())
    }

    fn hoist_variable_statement(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::VariableStatementData,
        plan: &mut TopLevelPlan,
    ) -> Result<Option<TransformNode>, TransformError> {
        let exported = self.has_modifier(data.modifiers, SyntaxKind::ExportKeyword)?;
        let declarations = data
            .declaration_list
            .and_then(|list| self.context.arena().node_ref(self.source, list))
            .and_then(|list| match &self.context.arena().node(list).ok()?.data {
                NodeData::VariableDeclarationList(data) => Some(data.declarations),
                _ => None,
            });
        let mut assignments = Vec::new();
        for declaration in self.array_nodes(declarations.flatten())? {
            let NodeData::VariableDeclaration(data) =
                self.context.arena().node(declaration)?.data.clone()
            else {
                continue;
            };
            if let Some(name) = data.name {
                self.hoist_binding_pattern(self.node(name), exported, plan)?;
                if let Some(initializer) = data.initializer {
                    let target = self.clone_binding_target(self.node(name))?;
                    let assignment = self.create_assignment(target, self.node(initializer))?;
                    self.set_original_and_range(assignment, declaration)?;
                    assignments.push(assignment);
                }
            }
        }
        if assignments.is_empty() {
            return Ok(None);
        }
        let expression = self.inline_expressions(assignments)?;
        let statement = self.create_expression_statement(expression)?;
        self.set_original_and_range(statement, original)?;
        Ok(Some(statement))
    }

    fn hoist_class(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ClassDeclarationData,
        plan: &mut TopLevelPlan,
    ) -> Result<Option<TransformNode>, TransformError> {
        let exported = self.has_modifier(data.modifiers, SyntaxKind::ExportKeyword)?;
        let default = self.has_modifier(data.modifiers, SyntaxKind::DefaultKeyword)?;
        let binding = if let Some(name) = data.name {
            self.identifier_text(self.node(name))
                .map(str::to_owned)
                .unwrap_or_else(|| self.allocate_generated_name("class"))
        } else if let Some(existing) = &plan.default_export_name {
            existing.clone()
        } else {
            let name = self.allocate_generated_name("default");
            plan.default_export_name = Some(name.clone());
            name
        };
        self.hoist_binding_name(&binding, exported, default.then_some("default"), plan)?;
        let class_name = data.name;
        let modifiers = self.filter_modifiers(
            data.modifiers,
            &[SyntaxKind::ExportKeyword, SyntaxKind::DefaultKeyword],
        )?;
        let transform_flags = self.context.arena().transform_flags(original);
        let class = self.context.factory()?.create_node(
            self.source,
            NodeData::ClassExpression(tsc_syntax::nodes::ClassExpressionData {
                name: class_name,
                type_parameters: data.type_parameters,
                heritage_clauses: data.heritage_clauses,
                members: data.members,
                modifiers,
            }),
            transform_flags,
        )?;
        self.set_original_and_range(class, original)?;
        let target = self.create_identifier(&binding)?;
        let assignment = self.create_assignment(target, class)?;
        self.set_original_and_range(assignment, original)?;
        let statement = self.create_expression_statement(assignment)?;
        self.set_original_and_range(statement, original)?;
        Ok(Some(statement))
    }

    fn hoist_export_assignment(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ExportAssignmentData,
        plan: &mut TopLevelPlan,
    ) -> Result<(), TransformError> {
        let expression = data
            .expression
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ExportAssignment,
                field: "expression",
            })?;
        let name = self.allocate_generated_name("default");
        if data.is_export_equals == Some(true) {
            let binding_identifier = self.create_identifier(&name)?;
            let binding = self.create_variable_declaration(binding_identifier, None)?;
            plan.hoisted_bindings.push(binding);
            let target = self.create_identifier(&name)?;
            let assignment_expression = self.create_assignment(target, self.node(expression))?;
            let assignment = self.create_expression_statement(assignment_expression)?;
            self.set_original_and_range(assignment, original)?;
            plan.body.push(assignment);
            let export_identifier = self.create_identifier(&name)?;
            let export = self.context.factory()?.create_node(
                self.source,
                NodeData::ExportAssignment(tsc_syntax::nodes::ExportAssignmentData {
                    modifiers: None,
                    is_export_equals: Some(true),
                    expression: Some(export_identifier.node()),
                }),
                TransformFlags::NONE,
            )?;
            plan.export_equals = Some(export);
        } else {
            self.hoist_binding_name(&name, true, Some("default"), plan)?;
            let target = self.create_identifier(&name)?;
            let assignment_expression = self.create_assignment(target, self.node(expression))?;
            let assignment = self.create_expression_statement(assignment_expression)?;
            self.set_original_and_range(assignment, original)?;
            plan.body.push(assignment);
            plan.default_export_name = Some(name);
        }
        Ok(())
    }

    fn hoist_binding_pattern(
        &mut self,
        name: TransformNode,
        exported: bool,
        plan: &mut TopLevelPlan,
    ) -> Result<(), TransformError> {
        match self.context.arena().node(name)?.data.clone() {
            NodeData::Identifier(data) => self.hoist_binding_name(&data.text, exported, None, plan),
            NodeData::ObjectBindingPattern(data) => {
                for element in self.array_nodes(data.elements)? {
                    if let NodeData::BindingElement(data) =
                        self.context.arena().node(element)?.data.clone()
                    {
                        if let Some(name) = data.name {
                            self.hoist_binding_pattern(self.node(name), exported, plan)?;
                        }
                    }
                }
                Ok(())
            }
            NodeData::ArrayBindingPattern(data) => {
                for element in self.array_nodes(data.elements)? {
                    if let NodeData::BindingElement(data) =
                        self.context.arena().node(element)?.data.clone()
                    {
                        if let Some(name) = data.name {
                            self.hoist_binding_pattern(self.node(name), exported, plan)?;
                        }
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn hoist_binding_name(
        &mut self,
        name: &str,
        exported: bool,
        export_alias: Option<&str>,
        plan: &mut TopLevelPlan,
    ) -> Result<(), TransformError> {
        let identifier = self.create_identifier(name)?;
        if exported {
            if let Some(alias) = export_alias {
                let local = self.create_identifier(name)?;
                let exported = self.create_identifier(alias)?;
                plan.export_specifiers
                    .push(self.create_export_specifier(Some(local), exported)?);
            } else {
                let declaration = self.create_variable_declaration(identifier, None)?;
                plan.exported_variables.push(declaration);
                return Ok(());
            }
        }
        plan.hoisted_bindings
            .push(self.create_variable_declaration(identifier, None)?);
        Ok(())
    }

    fn ensure_named_evaluation(
        &mut self,
        binding: Option<NodeId>,
        initializer: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let Some(binding) = binding else {
            return Ok(initializer);
        };
        let binding = self.node(binding);
        let NodeData::Identifier(binding_data) = &self.context.arena().node(binding)?.data else {
            return Ok(initializer);
        };
        let assigned_name_text = binding_data.text.clone();
        Ok(self
            .apply_named_evaluation(initializer, &assigned_name_text)?
            .expression)
    }

    fn apply_named_evaluation(
        &mut self,
        expression: TransformNode,
        assigned_name_text: &str,
    ) -> Result<NamedEvaluationOutcome, TransformError> {
        match self.context.arena().node(expression)?.data.clone() {
            NodeData::ParenthesizedExpression(mut data) => {
                let inner = data
                    .expression
                    .and_then(|inner| self.context.arena().node_ref(self.source, inner))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ParenthesizedExpression,
                        field: "expression",
                    })?;
                let outcome = self.apply_named_evaluation(inner, assigned_name_text)?;
                if !outcome.applied {
                    return Ok(NamedEvaluationOutcome::unchanged(expression));
                }
                data.expression = Some(outcome.expression.node());
                let updated = self.update_named_evaluation_outer(
                    expression,
                    NodeData::ParenthesizedExpression(data),
                )?;
                Ok(NamedEvaluationOutcome::applied(updated))
            }
            NodeData::PartiallyEmittedExpression(mut data) => {
                let inner = data
                    .expression
                    .and_then(|inner| self.context.arena().node_ref(self.source, inner))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PartiallyEmittedExpression,
                        field: "expression",
                    })?;
                let outcome = self.apply_named_evaluation(inner, assigned_name_text)?;
                if !outcome.applied {
                    return Ok(NamedEvaluationOutcome::unchanged(expression));
                }
                data.expression = Some(outcome.expression.node());
                let updated = self.update_named_evaluation_outer(
                    expression,
                    NodeData::PartiallyEmittedExpression(data),
                )?;
                Ok(NamedEvaluationOutcome::applied(updated))
            }
            NodeData::ClassExpression(mut data)
                if data.name.is_none()
                    && self
                        .context
                        .arena()
                        .metadata(expression)
                        .is_none_or(|metadata| metadata.assigned_name.is_none()) =>
            {
                let assigned_name = self.create_string_literal(assigned_name_text)?;
                let this = self.create_this()?;
                let call = self.create_set_function_name_call(this, assigned_name)?;
                let statement = self.create_expression_statement(call)?;
                let body = self.create_block(vec![statement], false)?;
                let block = self.context.factory()?.create_node(
                    self.source,
                    NodeData::ClassStaticBlockDeclaration(
                        tsc_syntax::nodes::ClassStaticBlockDeclarationData {
                            body: Some(body.node()),
                            modifiers: None,
                        },
                    ),
                    TransformFlags::NONE,
                )?;
                self.context.arena_mut()?.metadata_mut(block).assigned_name = Some(assigned_name);
                let mut members = self.array_nodes(data.members)?;
                members.insert(0, block);
                data.members = Some(
                    self.context
                        .factory()?
                        .create_node_array(self.source, members)?
                        .array(),
                );
                let flags = flags_after_update(
                    self.context.arena(),
                    expression,
                    &NodeData::ClassExpression(data.clone()),
                )?;
                let class = self.context.factory()?.update_node(
                    expression,
                    NodeData::ClassExpression(data),
                    flags,
                )?;
                self.context.arena_mut()?.metadata_mut(class).assigned_name = Some(assigned_name);
                Ok(NamedEvaluationOutcome::applied(class))
            }
            NodeData::FunctionExpression(data) if data.name.is_none() => {
                let assigned_name = self.create_string_literal(assigned_name_text)?;
                let call = self.create_set_function_name_call(expression, assigned_name)?;
                Ok(NamedEvaluationOutcome::applied(call))
            }
            NodeData::ArrowFunction(_) => {
                let assigned_name = self.create_string_literal(assigned_name_text)?;
                let call = self.create_set_function_name_call(expression, assigned_name)?;
                Ok(NamedEvaluationOutcome::applied(call))
            }
            _ => Ok(NamedEvaluationOutcome::unchanged(expression)),
        }
    }

    fn update_named_evaluation_outer(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<TransformNode, TransformError> {
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        self.context.factory()?.update_node(original, data, flags)
    }

    fn create_set_function_name_call(
        &mut self,
        value: TransformNode,
        assigned_name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context
            .request_emit_helper(super::helpers::set_function_name())?;
        let helper = self.create_identifier("__setFunctionName")?;
        self.create_call(helper, vec![value, assigned_name])
    }

    fn statements_mode(
        &self,
        statements: &[TransformNode],
    ) -> Result<Option<DisposalMode>, TransformError> {
        let mut result = None;
        for statement in statements {
            if let Some(mode) = self.statement_mode(*statement) {
                result = Some(result.map_or(mode, |current: DisposalMode| current.merge(mode)));
                if mode == DisposalMode::Async {
                    break;
                }
            }
        }
        Ok(result)
    }

    fn statement_mode(&self, statement: TransformNode) -> Option<DisposalMode> {
        let NodeData::VariableStatement(data) = &self.context.arena().node(statement).ok()?.data
        else {
            return None;
        };
        let list = data
            .declaration_list
            .and_then(|id| self.context.arena().node_ref(self.source, id))?;
        self.declaration_list_mode(list).ok().flatten()
    }

    fn declaration_list_mode(
        &self,
        list: TransformNode,
    ) -> Result<Option<DisposalMode>, TransformError> {
        if self.context.arena().node(list)?.kind != SyntaxKind::VariableDeclarationList {
            return Ok(None);
        }
        Ok(
            match NodeFlags::from_bits(self.context.arena().node(list)?.flags)
                & NodeFlags::BLOCK_SCOPED
            {
                flags if flags == NodeFlags::AWAIT_USING => Some(DisposalMode::Async),
                flags if flags == NodeFlags::USING => Some(DisposalMode::Sync),
                _ => None,
            },
        )
    }

    fn count_prologues(&self, statements: &[TransformNode]) -> Result<usize, TransformError> {
        let mut count = 0;
        for statement in statements {
            let string_prologue = matches!(
                &self.context.arena().node(*statement)?.data,
                NodeData::ExpressionStatement(data)
                    if data.expression.and_then(|expression| self.context.arena().node_ref(self.source, expression)).is_some_and(|expression| self.context.arena().node(expression).is_ok_and(|expression| matches!(expression.data, NodeData::StringLiteral(_))))
            );
            let custom_prologue = self
                .context
                .arena()
                .metadata(*statement)
                .is_some_and(|metadata| metadata.flags().intersects(EmitFlags::CUSTOM_PROLOGUE));
            if !string_prologue && !custom_prologue {
                break;
            }
            count += 1;
        }
        Ok(count)
    }

    fn allocate_generated_name(&mut self, base: &str) -> String {
        let ordinal = self.generated_ordinals.entry(base.to_owned()).or_insert(1);
        loop {
            let candidate = format!("{base}_{}", *ordinal);
            *ordinal += 1;
            if self.used_names.insert(candidate.clone()) {
                return candidate;
            }
        }
    }

    fn create_add_disposable_resource_call(
        &mut self,
        environment: TransformNode,
        value: TransformNode,
        asynchronous: bool,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:addDisposableResource",
            false,
            ADD_DISPOSABLE_RESOURCE_HELPER_TEXT,
            None,
            Vec::new(),
        ))?;
        let helper = self.create_identifier("__addDisposableResource")?;
        let asynchronous = self.create_boolean(asynchronous)?;
        self.create_call(helper, vec![environment, value, asynchronous])
    }

    fn create_dispose_resources_call(
        &mut self,
        environment: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:disposeResources",
            false,
            DISPOSE_RESOURCES_HELPER_TEXT,
            None,
            Vec::new(),
        ))?;
        let helper = self.create_identifier("__disposeResources")?;
        self.create_call(helper, vec![environment])
    }

    fn create_identifier(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: tsc_syntax::escape_leading_underscores(text),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_string_literal(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
                text: text.to_owned(),
                has_extended_unicode_escape: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_this(&mut self) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ThisKeyword,
            TransformFlags::CONTAINS_LEXICAL_THIS,
        )
    }

    fn create_boolean(&mut self, value: bool) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_token(
            self.source,
            if value {
                SyntaxKind::TrueKeyword
            } else {
                SyntaxKind::FalseKeyword
            },
            TransformFlags::NONE,
        )
    }

    fn create_void_zero(&mut self) -> Result<TransformNode, TransformError> {
        let zero = self.context.factory()?.create_node(
            self.source,
            NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                text: "0".to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VoidExpression(tsc_syntax::nodes::VoidExpressionData {
                expression: Some(zero.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_array_literal(
        &mut self,
        elements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let elements = self
            .context
            .factory()?
            .create_node_array(self.source, elements)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ArrayLiteralExpression(tsc_syntax::nodes::ArrayLiteralExpressionData {
                elements: Some(elements.array()),
            }),
            TransformFlags::NONE,
        )
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

    fn create_property_access(
        &mut self,
        expression: TransformNode,
        name: &str,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAccessExpression(tsc_syntax::nodes::PropertyAccessExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                name: Some(name.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_call(
        &mut self,
        expression: TransformNode,
        arguments: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let arguments = self
            .context
            .factory()?
            .create_node_array(self.source, arguments)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                type_arguments: None,
                arguments: Some(arguments.array()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_assignment(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::EqualsToken, right)
    }

    fn create_binary(
        &mut self,
        left: TransformNode,
        operator: SyntaxKind,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let operator =
            self.context
                .factory()?
                .create_token(self.source, operator, TransformFlags::NONE)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                left: Some(left.node()),
                operator_token: Some(operator.node()),
                right: Some(right.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn inline_expressions(
        &mut self,
        mut expressions: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let first = expressions.remove(0);
        expressions.into_iter().try_fold(first, |left, right| {
            self.create_binary(left, SyntaxKind::CommaToken, right)
        })
    }

    fn create_expression_statement(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::ExpressionStatement(tsc_syntax::nodes::ExpressionStatementData {
                expression: Some(expression.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_variable_declaration(
        &mut self,
        name: TransformNode,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: initializer.map(TransformNode::node),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_variable_declaration_list(
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
        self.context.factory()?.set_node_flags(list, flags)
    }

    fn create_variable_statement_from_declarations(
        &mut self,
        declarations: Vec<TransformNode>,
        flags: NodeFlags,
        modifiers: Option<NodeArrayId>,
    ) -> Result<TransformNode, TransformError> {
        let list = self.create_variable_declaration_list(declarations, flags)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers,
                declaration_list: Some(list.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_variable_statement_from_list(
        &mut self,
        list: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let transform_flags = self.context.arena().transform_flags(list);
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers: None,
                declaration_list: Some(list.node()),
            }),
            transform_flags,
        )
    }

    fn create_block(
        &mut self,
        statements: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let statements = self
            .context
            .factory()?
            .create_node_array(self.source, statements)?;
        let block = self.context.factory()?.create_node(
            self.source,
            NodeData::Block(tsc_syntax::nodes::BlockData {
                statements: Some(statements.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.set_multi_line(block, multi_line)
    }

    fn create_modifier_array(
        &mut self,
        kinds: &[SyntaxKind],
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let mut modifiers = Vec::with_capacity(kinds.len());
        for kind in kinds {
            modifiers.push(self.context.factory()?.create_token(
                self.source,
                *kind,
                TransformFlags::NONE,
            )?);
        }
        Ok(Some(
            self.context
                .factory()?
                .create_node_array(self.source, modifiers)?
                .array(),
        ))
    }

    fn create_export_specifier(
        &mut self,
        local: Option<TransformNode>,
        exported: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::ExportSpecifier(tsc_syntax::nodes::ExportSpecifierData {
                name: Some(exported.node()),
                is_type_only: false,
                property_name: local.map(TransformNode::node),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_export_declaration(
        &mut self,
        specifiers: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let specifiers = self
            .context
            .factory()?
            .create_node_array(self.source, specifiers)?;
        let named = self.context.factory()?.create_node(
            self.source,
            NodeData::NamedExports(tsc_syntax::nodes::NamedExportsData {
                elements: Some(specifiers.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ExportDeclaration(tsc_syntax::nodes::ExportDeclarationData {
                modifiers: None,
                is_type_only: false,
                export_clause: Some(named.node()),
                module_specifier: None,
                attributes: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn has_modifier(
        &self,
        modifiers: Option<NodeArrayId>,
        expected: SyntaxKind,
    ) -> Result<bool, TransformError> {
        Ok(self.array_nodes(modifiers)?.iter().any(|modifier| {
            self.context
                .arena()
                .node(*modifier)
                .is_ok_and(|modifier| modifier.kind == expected)
        }))
    }

    fn filter_modifiers(
        &mut self,
        modifiers: Option<NodeArrayId>,
        removed: &[SyntaxKind],
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(original) = modifiers else {
            return Ok(None);
        };
        let original_array = self.array(original);
        let kept = self
            .context
            .arena()
            .node_array(original_array)?
            .nodes
            .clone()
            .into_iter()
            .filter(|modifier| {
                self.context
                    .arena()
                    .node(self.node(*modifier))
                    .is_ok_and(|modifier| !removed.contains(&modifier.kind))
            })
            .map(|modifier| self.node(modifier))
            .collect();
        Ok(Some(
            self.context
                .factory()?
                .update_node_array(original_array, kept)?
                .array(),
        ))
    }

    fn clone_binding_target(
        &mut self,
        target: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.clone_node(target)
    }

    fn update_generic(
        &mut self,
        original: TransformNode,
        mut data: NodeData,
    ) -> Result<NodeId, TransformError> {
        try_visit_each_child(&mut data, self)?;
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        Ok(self
            .context
            .factory()?
            .update_node(original, data, flags)?
            .node())
    }

    fn visit_optional_node(
        &mut self,
        node: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        node.map(|node| self.visit(node))
            .transpose()
            .map(Option::flatten)
    }

    fn array_nodes(
        &self,
        array: Option<NodeArrayId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let Some(array) = array.and_then(|id| self.context.arena().node_array_ref(self.source, id))
        else {
            return Ok(Vec::new());
        };
        self.context
            .arena()
            .node_array(array)?
            .nodes
            .iter()
            .map(|id| {
                self.context
                    .arena()
                    .node_ref(self.source, *id)
                    .ok_or_else(|| TransformError::UnknownNode(self.node(*id)))
            })
            .collect()
    }

    fn array_node_ids(&self, array: Option<NodeArrayId>) -> Result<Vec<NodeId>, TransformError> {
        Ok(self
            .array_nodes(array)?
            .into_iter()
            .map(TransformNode::node)
            .collect())
    }

    fn identifier_text(&self, node: TransformNode) -> Option<&str> {
        match &self.context.arena().node(node).ok()?.data {
            NodeData::Identifier(data) => Some(&data.text),
            _ => None,
        }
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

    const fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    const fn array(&self, id: NodeArrayId) -> TransformNodeArray {
        TransformNodeArray::new(self.source, id)
    }
}

impl NodeDataChildVisitor for DirectChildCollector<'_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.arena
            .node(TransformNode::new(self.source, id))
            .expect("ESNext planning child belongs to the current transform source")
            .kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.children.push(id);
        Ok(Some(id))
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        let nodes = self
            .arena
            .node_array(TransformNodeArray::new(self.source, id))?
            .nodes
            .clone();
        self.children.extend(nodes);
        Ok(Some(id))
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}

impl NodeDataChildVisitor for EsNextVisitor<'_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("ESNext child belongs to the current transform source")
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
        let nodes = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(nodes.len());
        for node in nodes {
            if let Some(node) = self.visit(node)? {
                visited.push(self.node(node));
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
