use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags};

use crate::{
    factory::EmitHelperName, EmitExportContainerMode, EmitHint, EmitResolver, EmitResolverNode,
    TransformArena, TransformError, TransformFlags, TransformNode, TransformNodeArray,
    TransformRoot, TransformSourceId, TransformationContext, Transformer, UnsupportedEmitFeature,
};

use super::{
    first_runtime_declaration_original, flags_after_update, generated_module_name, has_modifier,
    identifier_or_literal_text, is_identifier_export_name, is_prologue_statement, node_array_nodes,
    parsed_source_file_statement_array, source_contains_dynamic_import,
    source_file_statement_nodes, string_literal_text, variable_declarations, CommonJsModuleInfo,
    ImportBinding,
};

/// tsc-port: transformSystemModule @6.0.3
/// tsc-hash: a548f0ab50ad264de2d5783dfe3ef9247dac0d001492465e794fdacda03e48af
/// tsc-span: _tsc.js:112050-113366
pub(super) fn transform_system_module<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    Box::new(SystemModuleTransformer {
        resolver,
        always_strict: options.always_strict_effective(),
    })
}

struct SystemModuleTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    always_strict: bool,
}

impl Transformer for SystemModuleTransformer<'_> {
    fn name(&self) -> &'static str {
        "transformSystemModule"
    }

    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        for kind in [
            SyntaxKind::Identifier,
            SyntaxKind::ShorthandPropertyAssignment,
            SyntaxKind::BinaryExpression,
            SyntaxKind::MetaProperty,
        ] {
            context.enable_substitution(kind)?;
        }
        context.enable_emit_notification(SyntaxKind::SourceFile)?;
        Ok(())
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Err(TransformError::Unsupported(
                UnsupportedEmitFeature::BundleRoot,
            ));
        };
        if context.arena().source(source)?.syntax().is_declaration_file {
            return Ok(TransformRoot::SourceFile(source));
        }
        let root = context.arena().root(source)?;
        let is_external = context
            .arena()
            .source(source)?
            .syntax()
            .external_module_indicator
            .is_some();
        let has_dynamic_import = source_contains_dynamic_import(context.arena(), root)?;
        let has_import_meta = source_contains_import_meta(context.arena(), root)?;
        if !is_external && !has_dynamic_import && !has_import_meta {
            return Ok(TransformRoot::SourceFile(source));
        }

        let common = CommonJsModuleInfo::collect(
            context.arena(),
            source,
            root,
            self.resolver,
            super::MODULE_SYSTEM,
        )?;
        let info = SystemModuleInfo::collect(context.arena(), source, root, common)?;
        let mut visitor =
            SystemVisitor::new(context, source, self.resolver, info, self.always_strict);
        let updated = visitor.transform_source_file(root)?;
        visitor.context.arena_mut()?.replace_root(source, updated)?;
        Ok(TransformRoot::SourceFile(source))
    }

    fn substitute_node(
        &mut self,
        _context: &mut TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        // The Rust transform performs the upstream substitutions while its
        // arena is mutable. The hook is still installed so hook composition
        // and activity remain observable.
        Ok(node)
    }
}

fn source_contains_import_meta(
    arena: &TransformArena,
    root: TransformNode,
) -> Result<bool, TransformError> {
    let mut stack = vec![root.node()];
    while let Some(id) = stack.pop() {
        let node = arena
            .node_ref(root.source(), id)
            .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(root.source(), id)))?;
        let record = arena.node(node)?;
        if let NodeData::MetaProperty(data) = &record.data {
            if data.keyword_token == SyntaxKind::ImportKeyword
                && data
                    .name
                    .and_then(|name| arena.node_ref(root.source(), name))
                    .and_then(|name| identifier_or_literal_text(arena, name).ok())
                    .as_deref()
                    == Some("meta")
            {
                return Ok(true);
            }
        }
        tsc_syntax::for_each_child(
            &arena.source(root.source())?.syntax().arena,
            record,
            |child| {
                stack.push(child);
                false
            },
        );
    }
    Ok(false)
}

#[derive(Clone, Debug)]
struct SystemDependencyGroup {
    module_specifier: Box<str>,
    entries: Vec<NodeId>,
    fallback_generated_name: Box<str>,
}

#[derive(Debug)]
struct SystemModuleInfo {
    common: CommonJsModuleInfo,
    dependency_groups: Vec<SystemDependencyGroup>,
    non_function_exported_names: Vec<Box<str>>,
}

impl SystemModuleInfo {
    fn collect(
        arena: &TransformArena,
        source: TransformSourceId,
        root: TransformNode,
        common: CommonJsModuleInfo,
    ) -> Result<Self, TransformError> {
        let statements = source_file_statement_nodes(arena, source, root)?;
        let mut group_indices = BTreeMap::<String, usize>::new();
        let mut dependency_groups = Vec::<SystemDependencyGroup>::new();
        let mut generated_names = BTreeMap::<String, usize>::new();
        let mut non_function_exported_names = Vec::<Box<str>>::new();

        for statement in &statements {
            let record = arena.node(*statement)?;
            let module_specifier = match &record.data {
                NodeData::ImportDeclaration(data) => data.module_specifier,
                NodeData::ImportEqualsDeclaration(data) => data
                    .module_reference
                    .and_then(|id| arena.node_ref(source, id))
                    .and_then(|reference| match &arena.node(reference).ok()?.data {
                        NodeData::ExternalModuleReference(data) => data.expression,
                        _ => None,
                    }),
                NodeData::ExportDeclaration(data) => data.module_specifier,
                _ => None,
            }
            .and_then(|id| arena.node_ref(source, id));
            if let Some(module_specifier) = module_specifier {
                let text = string_literal_text(arena, module_specifier)?.to_owned();
                let index = if let Some(index) = group_indices.get(&text).copied() {
                    index
                } else {
                    let base = generated_module_name(&text);
                    let ordinal = generated_names.entry(base.clone()).or_insert(0);
                    *ordinal += 1;
                    let index = dependency_groups.len();
                    group_indices.insert(text.clone(), index);
                    dependency_groups.push(SystemDependencyGroup {
                        module_specifier: text.clone().into_boxed_str(),
                        entries: Vec::new(),
                        fallback_generated_name: format!("{base}_{}", *ordinal).into_boxed_str(),
                    });
                    index
                };
                dependency_groups[index].entries.push(statement.node());
            }

            match &record.data {
                NodeData::VariableStatement(data)
                    if has_modifier(arena, source, data.modifiers, SyntaxKind::ExportKeyword)? =>
                {
                    for declaration in variable_declarations(arena, source, data.declaration_list)?
                    {
                        if let NodeData::VariableDeclaration(data) = &arena.node(declaration)?.data
                        {
                            collect_binding_names(
                                arena,
                                source,
                                data.name,
                                &mut non_function_exported_names,
                            )?;
                        }
                    }
                }
                NodeData::ClassDeclaration(data)
                    if has_modifier(arena, source, data.modifiers, SyntaxKind::ExportKeyword)?
                        && !has_modifier(
                            arena,
                            source,
                            data.modifiers,
                            SyntaxKind::DefaultKeyword,
                        )? =>
                {
                    if let Some(name) = data
                        .name
                        .and_then(|id| arena.node_ref(source, id))
                        .and_then(|name| identifier_or_literal_text(arena, name).ok())
                    {
                        push_unique(&mut non_function_exported_names, &name);
                    }
                }
                NodeData::ExportDeclaration(data) => {
                    if let Some(clause) =
                        data.export_clause.and_then(|id| arena.node_ref(source, id))
                    {
                        if let NodeData::NamedExports(data) = &arena.node(clause)?.data {
                            for specifier in node_array_nodes(arena, source, data.elements)? {
                                if let NodeData::ExportSpecifier(data) =
                                    &arena.node(specifier)?.data
                                {
                                    if let Some(name) = data
                                        .name
                                        .and_then(|id| arena.node_ref(source, id))
                                        .and_then(|name| {
                                            identifier_or_literal_text(arena, name).ok()
                                        })
                                    {
                                        push_unique(&mut non_function_exported_names, &name);
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(Self {
            common,
            dependency_groups,
            non_function_exported_names,
        })
    }
}

fn push_unique(values: &mut Vec<Box<str>>, value: &str) {
    if value != "default" && !values.iter().any(|current| current.as_ref() == value) {
        values.push(value.into());
    }
}

fn collect_binding_names(
    arena: &TransformArena,
    source: TransformSourceId,
    name: Option<NodeId>,
    output: &mut Vec<Box<str>>,
) -> Result<(), TransformError> {
    let Some(name) = name.and_then(|id| arena.node_ref(source, id)) else {
        return Ok(());
    };
    match &arena.node(name)?.data {
        NodeData::Identifier(data) => push_unique(output, &data.text),
        NodeData::ObjectBindingPattern(data) => {
            for element in node_array_nodes(arena, source, data.elements)? {
                if let NodeData::BindingElement(data) = &arena.node(element)?.data {
                    collect_binding_names(arena, source, data.name, output)?;
                }
            }
        }
        NodeData::ArrayBindingPattern(data) => {
            for element in node_array_nodes(arena, source, data.elements)? {
                if let NodeData::BindingElement(data) = &arena.node(element)?.data {
                    collect_binding_names(arena, source, data.name, output)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

struct SystemVisitor<'context, 'resolver> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    resolver: &'resolver dyn EmitResolver,
    info: SystemModuleInfo,
    always_strict: bool,
    exports_name: String,
    context_name: String,
    used_names: BTreeSet<String>,
    hoisted_names: Vec<String>,
    hoisted_declarations: Vec<TransformNode>,
    destructuring_temps: BTreeMap<NodeId, String>,
    temp_ordinal: usize,
    arrays: BTreeMap<NodeArrayId, NodeArrayId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SystemBlockScopeContainerKind {
    SourceFile,
    Block,
    CaseBlock,
    CatchClause,
    ForStatement,
    ForInStatement,
    ForOfStatement,
}

/// The active `enclosingBlockScopedContainer` used by the System transform.
/// Statements such as `if`, `while`, and labels do not create a new owner;
/// their embedded statements inherit the current one. Only the container
/// kinds represented here replace it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SystemBlockScopeOwner {
    container: TransformNode,
    kind: SystemBlockScopeContainerKind,
}

/// Required embedded-statement recovery after the System transform removes
/// every executable statement. Labels use tsc's synthetic empty-identifier
/// expression; other owners retain the ordinary `liftToBlock` behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SystemEmptyEmbeddedStatement {
    LiftToBlock,
    EmptyIdentifierExpression,
}

/// Ordered, typed result of flattening one SystemJS binding initializer.
/// Evaluation-only steps (for stable temporaries) cannot accidentally be
/// paired with an export name, while binding steps retain the local identity
/// needed by the System live-binding callback.
#[derive(Clone, Debug)]
enum SystemBindingStep {
    Evaluate(TransformNode),
    Bind {
        local: Box<str>,
        expression: TransformNode,
    },
}

#[derive(Clone, Debug, Default)]
struct SystemBindingPlan {
    steps: Vec<SystemBindingStep>,
}

impl SystemBindingPlan {
    fn push_evaluation(&mut self, expression: TransformNode) {
        self.steps.push(SystemBindingStep::Evaluate(expression));
    }

    fn push_binding(&mut self, local: impl Into<Box<str>>, expression: TransformNode) {
        self.steps.push(SystemBindingStep::Bind {
            local: local.into(),
            expression,
        });
    }

    fn into_expressions(self) -> Vec<TransformNode> {
        self.steps
            .into_iter()
            .map(|step| match step {
                SystemBindingStep::Evaluate(expression)
                | SystemBindingStep::Bind { expression, .. } => expression,
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug)]
struct SystemBindingElement {
    original: TransformNode,
    target: TransformNode,
    property_name: Option<TransformNode>,
    initializer: Option<TransformNode>,
    rest: bool,
}

#[derive(Clone, Debug)]
enum SystemExcludedProperty {
    Named(Box<str>),
    Computed(TransformNode),
}

impl SystemBlockScopeOwner {
    const fn source_file(container: TransformNode) -> Self {
        Self {
            container,
            kind: SystemBlockScopeContainerKind::SourceFile,
        }
    }

    fn entering(self, container: TransformNode, kind: SyntaxKind) -> Self {
        debug_assert_eq!(self.container.source(), container.source());
        let kind = match kind {
            SyntaxKind::Block => SystemBlockScopeContainerKind::Block,
            SyntaxKind::CaseBlock => SystemBlockScopeContainerKind::CaseBlock,
            SyntaxKind::CatchClause => SystemBlockScopeContainerKind::CatchClause,
            SyntaxKind::ForStatement => SystemBlockScopeContainerKind::ForStatement,
            SyntaxKind::ForInStatement => SystemBlockScopeContainerKind::ForInStatement,
            SyntaxKind::ForOfStatement => SystemBlockScopeContainerKind::ForOfStatement,
            _ => return self,
        };
        Self { container, kind }
    }

    const fn is_source_file(self) -> bool {
        matches!(self.kind, SystemBlockScopeContainerKind::SourceFile)
    }
}

impl<'context, 'resolver> SystemVisitor<'context, 'resolver> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        resolver: &'resolver dyn EmitResolver,
        info: SystemModuleInfo,
        always_strict: bool,
    ) -> Self {
        let mut used_names = collect_identifier_texts(context.arena(), source);
        let exports_name = unique_generated_name(&mut used_names, "exports");
        let context_name = unique_generated_name(&mut used_names, "context");
        Self {
            context,
            source,
            resolver,
            info,
            always_strict,
            exports_name,
            context_name,
            used_names,
            hoisted_names: Vec::new(),
            hoisted_declarations: Vec::new(),
            destructuring_temps: BTreeMap::new(),
            temp_ordinal: 0,
            arrays: BTreeMap::new(),
        }
    }

    fn transform_source_file(
        &mut self,
        root: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (mut source_data, original_array) = match self.context.arena().node(root)?.data.clone()
        {
            NodeData::SourceFile(data) => {
                let statements = data.statements;
                (data, statements)
            }
            _ => {
                return Err(TransformError::RootKindExpected {
                    actual: self.context.arena().node(root)?.kind,
                })
            }
        };
        let input = node_array_nodes(self.context.arena(), self.source, original_array)?;
        let parsed_statement_array =
            parsed_source_file_statement_array(self.context.arena(), root)?;
        let source_owner = SystemBlockScopeOwner::source_file(root);
        self.collect_hoisted_names(&input, source_owner)?;

        let mut outer = Vec::new();
        let mut offset = 0usize;
        while offset < input.len() && is_prologue_statement(self.context.arena(), input[offset])? {
            outer.push(self.visit(input[offset].node())?);
            offset += 1;
        }
        if !outer.iter().any(|statement| {
            system_is_use_strict(self.context.arena(), *statement).unwrap_or(false)
        }) && (self.always_strict
            || self
                .context
                .arena()
                .source(self.source)
                .is_ok_and(|source| source.syntax().external_module_indicator.is_some())
            || source_contains_dynamic_import(self.context.arena(), root).unwrap_or(false)
            || source_contains_import_meta(self.context.arena(), root).unwrap_or(false))
        {
            outer.push(self.create_use_strict()?);
        }

        let mut execute = Vec::new();
        for statement in input.into_iter().skip(offset) {
            if self.context.arena().node(statement)?.kind == SyntaxKind::FunctionDeclaration {
                let transformed = self.transform_hoisted_function(statement, true)?;
                self.hoisted_declarations.extend(transformed);
            } else {
                execute.extend(self.transform_execute_statement(statement, source_owner)?);
            }
        }
        // Earlier transforms can replace the current array or prepend a
        // synthetic directive. In the latter case `statementOffset` is
        // non-zero even though the first execute statement still starts at
        // the parse-tree SourceFile's detached-comment boundary. Retain that
        // parsed array independently of the current prologue offset: the
        // outer SourceFile owns its detached prefix, while the relocated
        // execute body uses it only as a resume seed. The printer consumes
        // the seed only when a retained statement has the same source owner.
        let relocated_execute_comments = parsed_statement_array
            .map(crate::metadata::RelocatedStatementListComments::owned_by_source_file);

        // Expression transforms can request temporaries, so materialize the
        // lexical hoist only after execute and hoisted functions are complete.
        if !self.hoisted_names.is_empty() {
            let mut declarations = Vec::with_capacity(self.hoisted_names.len());
            for name in self.hoisted_names.clone() {
                declarations.push(self.create_variable_declaration(&name, None)?);
            }
            outer.push(self.create_variable_statement(declarations, NodeFlags::NONE)?);
        }
        outer.push(self.create_module_name_statement()?);
        outer.append(&mut self.hoisted_declarations);

        if self.has_export_star() {
            outer.extend(self.create_export_star_prelude()?);
        }

        let setters = self.create_setters_array()?;
        let execute_body = self.create_block(execute, true)?;
        if let Some(relocated_execute_comments) = relocated_execute_comments {
            self.context
                .arena_mut()?
                .metadata_mut(execute_body)
                .set_relocated_statement_list_comments(relocated_execute_comments);
        }
        let execute_function = self.create_function_expression(Vec::new(), execute_body, None)?;
        let setters_property = self.create_property_assignment_identifier("setters", setters)?;
        let execute_property =
            self.create_property_assignment_identifier("execute", execute_function)?;
        let module_object =
            self.create_object_literal(vec![setters_property, execute_property], true)?;
        outer.push(self.create_return_statement(module_object)?);

        let body = self.create_block(outer, true)?;
        let parameters = vec![
            self.create_parameter(&self.exports_name.clone())?,
            self.create_parameter(&self.context_name.clone())?,
        ];
        let body_function = self.create_function_expression(parameters, body, None)?;
        let dependency_names = self
            .info
            .dependency_groups
            .iter()
            .map(|group| group.module_specifier.to_string())
            .collect::<Vec<_>>();
        let mut dependency_literals = Vec::with_capacity(dependency_names.len());
        for name in dependency_names {
            dependency_literals.push(self.create_string_literal(&name)?);
        }
        let dependencies = self.create_array_literal(dependency_literals, false)?;
        let system = self.create_identifier("System")?;
        let register = self.create_property_access(system, "register")?;
        let mut arguments = Vec::new();
        if let Some(module_name) = self
            .context
            .arena()
            .source(self.source)?
            .syntax()
            .module_name
            .clone()
        {
            arguments.push(self.create_string_literal(&module_name)?);
        }
        arguments.push(dependencies);
        arguments.push(body_function);
        let call = self.create_call(register, arguments)?;
        let wrapper = self.create_expression_statement(call)?;
        if let Some(first) =
            first_runtime_declaration_original(self.context.arena(), self.source, original_array)?
        {
            self.set_original_and_range(wrapper, first)?;
            self.context.arena_mut()?.metadata_mut(wrapper).add_flags(
                crate::EmitFlags::NO_LEADING_COMMENTS | crate::EmitFlags::NO_TRAILING_COMMENTS,
            );
        }

        let statements = if let Some(original) =
            original_array.and_then(|id| self.context.arena().node_array_ref(self.source, id))
        {
            self.context
                .factory()?
                .update_node_array(original, vec![wrapper])?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, vec![wrapper])?
        };
        source_data.statements = Some(statements.array());
        let flags = self.context.arena().transform_flags(root);
        let updated =
            self.context
                .factory()?
                .update_node(root, NodeData::SourceFile(source_data), flags)?;
        self.context
            .arena_mut()?
            .metadata_mut(updated)
            .add_flags(crate::EmitFlags::NO_TRAILING_COMMENTS);
        Ok(updated)
    }

    fn collect_hoisted_names(
        &mut self,
        statements: &[TransformNode],
        owner: SystemBlockScopeOwner,
    ) -> Result<(), TransformError> {
        // Dependency groups define the setter topology, but lexical hoists are
        // requested while tsc visits source statements. Walk each source
        // statement once so an import's generated namespace identity retains
        // its position relative to ordinary declarations and namespace/import-
        // equals lowering, even though setter entries are grouped separately.
        for statement in statements {
            let statement = *statement;
            match &self.context.arena().node(statement)?.data {
                NodeData::ImportDeclaration(data) if data.import_clause.is_some() => {
                    let key = self.context.arena().get_original_node(statement).node();
                    if let Some(name) = self
                        .info
                        .common
                        .imports
                        .get(&key)
                        .and_then(|plan| plan.runtime_name.as_deref().map(str::to_owned))
                    {
                        self.push_hoisted_name(&name);
                    }
                    if let Some(alias) = self
                        .info
                        .common
                        .imports
                        .get(&key)
                        .and_then(|plan| plan.namespace_alias.as_deref().map(str::to_owned))
                    {
                        self.push_hoisted_name(&alias);
                    }
                }
                NodeData::ImportEqualsDeclaration(data) => {
                    if let Some(name) = data
                        .name
                        .and_then(|id| self.context.arena().node_ref(self.source, id))
                        .and_then(|name| {
                            identifier_or_literal_text(self.context.arena(), name).ok()
                        })
                    {
                        self.push_hoisted_name(&name);
                    }
                }
                _ => {}
            }
            self.collect_statement_hoists(statement, owner)?;
        }
        Ok(())
    }

    fn collect_statement_hoists(
        &mut self,
        statement: TransformNode,
        owner: SystemBlockScopeOwner,
    ) -> Result<(), TransformError> {
        let record = self.context.arena().node(statement)?.clone();
        let owner = owner.entering(statement, record.kind);
        match record.data {
            NodeData::VariableStatement(data) => {
                let Some(list) = data
                    .declaration_list
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                else {
                    return Ok(());
                };
                if self.should_hoist_declaration_list(list, owner)? {
                    self.collect_declaration_list_hoists(list)?;
                }
            }
            NodeData::VariableDeclarationList(_) => {
                if self.should_hoist_declaration_list(statement, owner)? {
                    self.collect_declaration_list_hoists(statement)?;
                }
            }
            NodeData::ClassDeclaration(data) => {
                let key = self.context.arena().get_original_node(statement).node();
                let name = data
                    .name
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                    .and_then(|name| identifier_or_literal_text(self.context.arena(), name).ok())
                    .or_else(|| {
                        self.info
                            .common
                            .generated_declaration_names
                            .get(&key)
                            .map(ToString::to_string)
                    });
                if let Some(name) = name {
                    self.push_hoisted_name(&name);
                }
            }
            NodeData::FunctionDeclaration(_)
            | NodeData::FunctionExpression(_)
            | NodeData::ArrowFunction(_)
            | NodeData::ClassExpression(_) => {}
            _ => {
                let mut children = Vec::new();
                {
                    let syntax = &self.context.arena().source(self.source)?.syntax().arena;
                    tsc_syntax::for_each_child(syntax, &record, |child| {
                        children.push(child);
                        false
                    });
                }
                for child in children {
                    if let Some(child) = self.context.arena().node_ref(self.source, child) {
                        self.collect_statement_hoists(child, owner)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn should_hoist_declaration_list(
        &self,
        list: TransformNode,
        owner: SystemBlockScopeOwner,
    ) -> Result<bool, TransformError> {
        let no_hoisting = self
            .context
            .arena()
            .metadata(list)
            .is_some_and(|metadata| metadata.flags().contains(crate::EmitFlags::NO_HOISTING));
        let flags = NodeFlags::from_bits(self.context.arena().node(list)?.flags);
        Ok(!no_hoisting && (owner.is_source_file() || !flags.intersects(NodeFlags::BLOCK_SCOPED)))
    }

    fn collect_declaration_list_hoists(
        &mut self,
        list: TransformNode,
    ) -> Result<(), TransformError> {
        if !matches!(
            self.context.arena().node(list)?.data,
            NodeData::VariableDeclarationList(_)
        ) {
            return Ok(());
        }
        for declaration in
            variable_declarations(self.context.arena(), self.source, Some(list.node()))?
        {
            if let NodeData::VariableDeclaration(data) =
                self.context.arena().node(declaration)?.data.clone()
            {
                if data
                    .name
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                    .zip(
                        data.initializer
                            .and_then(|id| self.context.arena().node_ref(self.source, id)),
                    )
                    .map(|(name, initializer)| {
                        self.binding_pattern_requires_root_temp(name, initializer)
                    })
                    .transpose()?
                    .unwrap_or(false)
                {
                    let temp = self.next_temp_name();
                    self.destructuring_temps.insert(
                        self.context.arena().get_original_node(declaration).node(),
                        temp.clone(),
                    );
                    self.push_hoisted_name(&temp);
                }
                let mut names = Vec::new();
                collect_binding_names(self.context.arena(), self.source, data.name, &mut names)?;
                for name in names {
                    self.push_hoisted_name(&name);
                }
            }
        }
        Ok(())
    }

    fn push_hoisted_name(&mut self, name: &str) {
        if !self.hoisted_names.iter().any(|current| current == name) {
            self.hoisted_names.push(name.to_owned());
            self.used_names.insert(name.to_owned());
        }
    }

    fn next_temp_name(&mut self) -> String {
        loop {
            let ordinal = self.temp_ordinal;
            self.temp_ordinal += 1;
            let candidate = if ordinal < 26 {
                format!("_{}", (b'a' + ordinal as u8) as char)
            } else {
                format!("_{}", ordinal - 26)
            };
            if self.used_names.insert(candidate.clone()) {
                return candidate;
            }
        }
    }

    fn transform_hoisted_function(
        &mut self,
        original: TransformNode,
        publish_exports: bool,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let NodeData::FunctionDeclaration(mut data) =
            self.context.arena().node(original)?.data.clone()
        else {
            return Ok(Vec::new());
        };
        if data.name.is_none() {
            let key = self.context.arena().get_original_node(original).node();
            if let Some(name) = self
                .info
                .common
                .generated_declaration_names
                .get(&key)
                .map(ToString::to_string)
            {
                data.name = Some(self.create_identifier(&name)?.node());
            }
        }
        let local = data
            .name
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .and_then(|name| identifier_or_literal_text(self.context.arena(), name).ok());
        let exports = if publish_exports {
            local
                .as_deref()
                .map(|local| {
                    self.info.common.hoisted_declaration_exports(
                        self.context.arena(),
                        self.source,
                        data.modifiers,
                        local,
                    )
                })
                .transpose()?
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        data.modifiers = self.remove_export_modifiers(data.modifiers)?;
        let function = self.update_generic(original, NodeData::FunctionDeclaration(data))?;
        let mut output = vec![function];
        if let Some(local) = local {
            for export in exports {
                let value = self.create_identifier(&local)?;
                let call = self.create_export_call(&export, value)?;
                output.push(self.create_expression_statement(call)?);
            }
        }
        Ok(output)
    }

    fn transform_execute_statement(
        &mut self,
        statement: TransformNode,
        owner: SystemBlockScopeOwner,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let record = self.context.arena().node(statement)?.clone();
        let owner = owner.entering(statement, record.kind);
        match record.data {
            NodeData::ImportDeclaration(data) => self.transform_import_declaration(data),
            NodeData::ImportEqualsDeclaration(_) | NodeData::ExportDeclaration(_) => Ok(Vec::new()),
            NodeData::ExportAssignment(data) => {
                if data.is_export_equals == Some(true) {
                    return Ok(Vec::new());
                }
                let expression = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ExportAssignment,
                        field: "expression",
                    })?;
                let expression = self.visit(expression)?;
                // tsc's createExportExpression prevents the source value's
                // comments from becoming argument-list comments inside the
                // synthesized exports call. The export statement owns its
                // own comment range independently.
                self.context
                    .arena_mut()?
                    .metadata_mut(expression)
                    .add_flags(crate::EmitFlags::NO_COMMENTS);
                let call = self.create_export_call("default", expression)?;
                let emitted = self.create_expression_statement(call)?;
                self.set_original_and_range(emitted, statement)?;
                Ok(vec![emitted])
            }
            NodeData::VariableStatement(data) => {
                let list = data
                    .declaration_list
                    .and_then(|id| self.context.arena().node_ref(self.source, id));
                if let Some(list) = list {
                    if self.should_hoist_declaration_list(list, owner)? {
                        return self.transform_hoisted_variable_statement(statement, data);
                    }
                }
                Ok(vec![self.visit(statement.node())?])
            }
            NodeData::ClassDeclaration(data) => {
                self.transform_hoisted_class(statement, data, owner.is_source_file())
            }
            NodeData::FunctionDeclaration(_) => {
                let declarations =
                    self.transform_hoisted_function(statement, owner.is_source_file())?;
                self.hoisted_declarations.extend(declarations);
                Ok(Vec::new())
            }
            NodeData::Block(data) => {
                Ok(vec![self.transform_execute_block(statement, data, owner)?])
            }
            NodeData::IfStatement(mut data) => {
                data.expression = data
                    .expression
                    .map(|id| self.visit(id).map(TransformNode::node))
                    .transpose()?;
                data.then_statement = Some(
                    self.transform_execute_embedded(
                        data.then_statement,
                        SyntaxKind::IfStatement,
                        owner,
                    )?
                    .node(),
                );
                data.else_statement = data
                    .else_statement
                    .map(|id| {
                        self.transform_execute_embedded(Some(id), SyntaxKind::IfStatement, owner)
                            .map(TransformNode::node)
                    })
                    .transpose()?;
                let flags = self.context.arena().transform_flags(statement);
                Ok(vec![self.context.factory()?.update_node(
                    statement,
                    NodeData::IfStatement(data),
                    flags,
                )?])
            }
            NodeData::ForStatement(mut data) => {
                data.initializer = self
                    .transform_for_initializer(data.initializer, owner)?
                    .map(TransformNode::node);
                data.condition = data
                    .condition
                    .map(|id| self.visit(id).map(TransformNode::node))
                    .transpose()?;
                data.incrementor = data
                    .incrementor
                    .map(|id| self.visit_expression(id, true).map(TransformNode::node))
                    .transpose()?;
                data.statement = Some(
                    self.transform_execute_embedded(
                        data.statement,
                        SyntaxKind::ForStatement,
                        owner,
                    )?
                    .node(),
                );
                Ok(vec![self.update_generic_without_visit(
                    statement,
                    NodeData::ForStatement(data),
                )?])
            }
            NodeData::ForInStatement(mut data) => {
                data.initializer = self
                    .transform_for_initializer(data.initializer, owner)?
                    .map(TransformNode::node);
                data.expression = data
                    .expression
                    .map(|id| self.visit(id).map(TransformNode::node))
                    .transpose()?;
                data.statement = Some(
                    self.transform_execute_embedded(
                        data.statement,
                        SyntaxKind::ForInStatement,
                        owner,
                    )?
                    .node(),
                );
                Ok(vec![self.update_generic_without_visit(
                    statement,
                    NodeData::ForInStatement(data),
                )?])
            }
            NodeData::ForOfStatement(mut data) => {
                data.initializer = self
                    .transform_for_initializer(data.initializer, owner)?
                    .map(TransformNode::node);
                data.expression = data
                    .expression
                    .map(|id| self.visit(id).map(TransformNode::node))
                    .transpose()?;
                data.statement = Some(
                    self.transform_execute_embedded(
                        data.statement,
                        SyntaxKind::ForOfStatement,
                        owner,
                    )?
                    .node(),
                );
                Ok(vec![self.update_generic_without_visit(
                    statement,
                    NodeData::ForOfStatement(data),
                )?])
            }
            NodeData::DoStatement(mut data) => {
                data.statement = Some(
                    self.transform_execute_embedded(
                        data.statement,
                        SyntaxKind::DoStatement,
                        owner,
                    )?
                    .node(),
                );
                data.expression = data
                    .expression
                    .map(|id| self.visit(id).map(TransformNode::node))
                    .transpose()?;
                Ok(vec![self.update_generic_without_visit(
                    statement,
                    NodeData::DoStatement(data),
                )?])
            }
            NodeData::WhileStatement(mut data) => {
                data.expression = data
                    .expression
                    .map(|id| self.visit(id).map(TransformNode::node))
                    .transpose()?;
                data.statement = Some(
                    self.transform_execute_embedded(
                        data.statement,
                        SyntaxKind::WhileStatement,
                        owner,
                    )?
                    .node(),
                );
                Ok(vec![self.update_generic_without_visit(
                    statement,
                    NodeData::WhileStatement(data),
                )?])
            }
            NodeData::LabeledStatement(mut data) => {
                data.statement = Some(
                    self.transform_execute_embedded_with_empty_result(
                        data.statement,
                        SyntaxKind::LabeledStatement,
                        owner,
                        SystemEmptyEmbeddedStatement::EmptyIdentifierExpression,
                    )?
                    .node(),
                );
                Ok(vec![self.update_generic_without_visit(
                    statement,
                    NodeData::LabeledStatement(data),
                )?])
            }
            NodeData::WithStatement(mut data) => {
                data.expression = data
                    .expression
                    .map(|id| self.visit(id).map(TransformNode::node))
                    .transpose()?;
                data.statement = Some(
                    self.transform_execute_embedded(
                        data.statement,
                        SyntaxKind::WithStatement,
                        owner,
                    )?
                    .node(),
                );
                Ok(vec![self.update_generic_without_visit(
                    statement,
                    NodeData::WithStatement(data),
                )?])
            }
            NodeData::SwitchStatement(mut data) => {
                data.expression = data
                    .expression
                    .map(|id| self.visit(id).map(TransformNode::node))
                    .transpose()?;
                data.case_block = data
                    .case_block
                    .map(|id| {
                        self.transform_execute_case_block(id, owner)
                            .map(TransformNode::node)
                    })
                    .transpose()?;
                Ok(vec![self.update_generic_without_visit(
                    statement,
                    NodeData::SwitchStatement(data),
                )?])
            }
            NodeData::CaseClause(mut data) => {
                data.expression = data
                    .expression
                    .map(|id| self.visit(id).map(TransformNode::node))
                    .transpose()?;
                data.statements = self.transform_execute_statement_array(data.statements, owner)?;
                Ok(vec![self.update_generic_without_visit(
                    statement,
                    NodeData::CaseClause(data),
                )?])
            }
            NodeData::DefaultClause(mut data) => {
                data.statements = self.transform_execute_statement_array(data.statements, owner)?;
                Ok(vec![self.update_generic_without_visit(
                    statement,
                    NodeData::DefaultClause(data),
                )?])
            }
            NodeData::TryStatement(mut data) => {
                data.try_block = data
                    .try_block
                    .map(|id| {
                        self.transform_execute_required_block(id, owner)
                            .map(TransformNode::node)
                    })
                    .transpose()?;
                data.catch_clause = data
                    .catch_clause
                    .map(|id| {
                        self.transform_execute_catch_clause(id, owner)
                            .map(TransformNode::node)
                    })
                    .transpose()?;
                data.finally_block = data
                    .finally_block
                    .map(|id| {
                        self.transform_execute_required_block(id, owner)
                            .map(TransformNode::node)
                    })
                    .transpose()?;
                Ok(vec![self.update_generic_without_visit(
                    statement,
                    NodeData::TryStatement(data),
                )?])
            }
            NodeData::ExpressionStatement(mut data) => {
                data.expression = data
                    .expression
                    .map(|id| self.visit_expression(id, true).map(TransformNode::node))
                    .transpose()?;
                let flags = self.context.arena().transform_flags(statement);
                let emitted = self.context.factory()?.update_node(
                    statement,
                    NodeData::ExpressionStatement(data),
                    flags,
                )?;
                self.restore_runtime_declaration_leading_comments(emitted)?;
                Ok(vec![emitted])
            }
            _ => Ok(vec![self.visit(statement.node())?]),
        }
    }

    /// TypeScript's namespace/enum lowering places leading comments on the
    /// declaration statement and suppresses them on its following runtime
    /// IIFE. System hoists that declaration into a synthetic aggregate, so the
    /// executable statement must retake ownership inside `execute`.
    fn restore_runtime_declaration_leading_comments(
        &mut self,
        statement: TransformNode,
    ) -> Result<(), TransformError> {
        let original = self.context.arena().get_original_node(statement);
        if !matches!(
            self.context.arena().node(original)?.kind,
            SyntaxKind::ModuleDeclaration | SyntaxKind::EnumDeclaration
        ) {
            return Ok(());
        }
        let Some(flags) = self
            .context
            .arena()
            .metadata(statement)
            .map(crate::EmitMetadata::flags)
        else {
            return Ok(());
        };
        if flags.intersects(crate::EmitFlags::NO_LEADING_COMMENTS) {
            self.context.arena_mut()?.metadata_mut(statement).set_flags(
                crate::EmitFlags::from_bits(
                    flags.bits() & !crate::EmitFlags::NO_LEADING_COMMENTS.bits(),
                ),
            );
        }
        Ok(())
    }

    /// Imports receive their runtime value in a dependency setter, while
    /// aliases exported through a later `export { local }` are published at
    /// the import declaration's execute position. This mirrors tsc's
    /// appendExportsOfImportDeclaration ownership; the export declaration
    /// itself remains erased.
    fn transform_import_declaration(
        &mut self,
        data: tsc_syntax::nodes::ImportDeclarationData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let Some(clause) = data
            .import_clause
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        else {
            return Ok(Vec::new());
        };
        let NodeData::ImportClause(clause_data) = self.context.arena().node(clause)?.data.clone()
        else {
            return Ok(Vec::new());
        };
        let mut statements = Vec::new();
        if let Some(name) = clause_data
            .name
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        {
            self.append_exports_of_import_binding(&mut statements, clause, name)?;
        }
        if let Some(bindings) = clause_data
            .named_bindings
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        {
            match self.context.arena().node(bindings)?.data.clone() {
                NodeData::NamespaceImport(namespace) => {
                    if let Some(name) = namespace
                        .name
                        .and_then(|id| self.context.arena().node_ref(self.source, id))
                    {
                        self.append_exports_of_import_binding(&mut statements, bindings, name)?;
                    }
                }
                NodeData::NamedImports(named) => {
                    for specifier in
                        node_array_nodes(self.context.arena(), self.source, named.elements)?
                    {
                        let NodeData::ImportSpecifier(specifier_data) =
                            self.context.arena().node(specifier)?.data.clone()
                        else {
                            continue;
                        };
                        if specifier_data.is_type_only {
                            continue;
                        }
                        if let Some(name) = specifier_data
                            .name
                            .and_then(|id| self.context.arena().node_ref(self.source, id))
                        {
                            self.append_exports_of_import_binding(
                                &mut statements,
                                specifier,
                                name,
                            )?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(statements)
    }

    fn append_exports_of_import_binding(
        &mut self,
        statements: &mut Vec<TransformNode>,
        declaration: TransformNode,
        local_name: TransformNode,
    ) -> Result<(), TransformError> {
        let local = identifier_or_literal_text(self.context.arena(), local_name)?;
        let exports = self
            .info
            .common
            .export_specifiers_by_local
            .get(local.as_str())
            .cloned()
            .unwrap_or_default();
        if exports.is_empty() {
            return Ok(());
        }
        let key = self.context.arena().get_original_node(declaration).node();
        let Some(binding) = self.info.common.import_bindings.get(&key).cloned() else {
            return Ok(());
        };
        for export in exports {
            let target = self.create_identifier(&binding.generated_name)?;
            let value = if let Some(property) = binding.property.as_deref() {
                if is_identifier_export_name(property) {
                    self.create_property_access(target, property)?
                } else {
                    let property = self.create_string_literal(property)?;
                    self.create_element_access(target, property)?
                }
            } else {
                target
            };
            let call = self.create_export_call(&export, value)?;
            let statement = self.create_expression_statement(call)?;
            statements.push(statement);
        }
        Ok(())
    }

    fn transform_execute_block(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::BlockData,
        owner: SystemBlockScopeOwner,
    ) -> Result<TransformNode, TransformError> {
        let owner = owner.entering(original, SyntaxKind::Block);
        let input = node_array_nodes(self.context.arena(), self.source, data.statements)?;
        let mut output = Vec::new();
        for statement in input {
            output.extend(self.transform_execute_statement(statement, owner)?);
        }
        data.statements = Some(
            if let Some(array) = data
                .statements
                .and_then(|id| self.context.arena().node_array_ref(self.source, id))
            {
                self.context
                    .factory()?
                    .update_node_array(array, output)?
                    .array()
            } else {
                self.context
                    .factory()?
                    .create_node_array(self.source, output)?
                    .array()
            },
        );
        let flags = self.context.arena().transform_flags(original);
        self.context
            .factory()?
            .update_node(original, NodeData::Block(data), flags)
    }

    fn transform_execute_embedded(
        &mut self,
        statement: Option<NodeId>,
        parent: SyntaxKind,
        owner: SystemBlockScopeOwner,
    ) -> Result<TransformNode, TransformError> {
        self.transform_execute_embedded_with_empty_result(
            statement,
            parent,
            owner,
            SystemEmptyEmbeddedStatement::LiftToBlock,
        )
    }

    fn transform_execute_embedded_with_empty_result(
        &mut self,
        statement: Option<NodeId>,
        parent: SyntaxKind,
        owner: SystemBlockScopeOwner,
        empty_result: SystemEmptyEmbeddedStatement,
    ) -> Result<TransformNode, TransformError> {
        let statement = statement
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .ok_or(TransformError::RequiredChildRemoved {
                parent,
                field: "statement",
            })?;
        let mut statements = self.transform_execute_statement(statement, owner)?;
        if statements.len() == 1 {
            return Ok(statements.remove(0));
        }
        if statements.is_empty()
            && matches!(
                empty_result,
                SystemEmptyEmbeddedStatement::EmptyIdentifierExpression
            )
        {
            let empty = self.create_identifier("")?;
            return self.create_expression_statement(empty);
        }
        // `factory.liftToBlock` leaves the multi-line role unset. The printer
        // makes non-empty regular blocks multi-line independently, but keeps
        // a block synthesized for an erased statement on one line.
        self.create_block(statements, false)
    }

    fn transform_execute_statement_array(
        &mut self,
        statements: Option<NodeArrayId>,
        owner: SystemBlockScopeOwner,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(statements) = statements else {
            return Ok(None);
        };
        let original = self.array(statements);
        let input = node_array_nodes(self.context.arena(), self.source, Some(statements))?;
        let mut output = Vec::new();
        for statement in input {
            output.extend(self.transform_execute_statement(statement, owner)?);
        }
        Ok(Some(
            self.context
                .factory()?
                .update_node_array(original, output)?
                .array(),
        ))
    }

    fn transform_execute_case_block(
        &mut self,
        id: NodeId,
        owner: SystemBlockScopeOwner,
    ) -> Result<TransformNode, TransformError> {
        let original = self.node(id);
        let NodeData::CaseBlock(mut data) = self.context.arena().node(original)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SwitchStatement,
                field: "case_block",
            });
        };
        let owner = owner.entering(original, SyntaxKind::CaseBlock);
        let clauses = node_array_nodes(self.context.arena(), self.source, data.clauses)?;
        let mut output = Vec::new();
        for clause in clauses {
            output.extend(self.transform_execute_statement(clause, owner)?);
        }
        data.clauses = match data.clauses {
            Some(clauses) => {
                let original_clauses = self.array(clauses);
                Some(
                    self.context
                        .factory()?
                        .update_node_array(original_clauses, output)?
                        .array(),
                )
            }
            None => Some(
                self.context
                    .factory()?
                    .create_node_array(self.source, output)?
                    .array(),
            ),
        };
        self.update_generic_without_visit(original, NodeData::CaseBlock(data))
    }

    fn transform_execute_required_block(
        &mut self,
        id: NodeId,
        owner: SystemBlockScopeOwner,
    ) -> Result<TransformNode, TransformError> {
        let original = self.node(id);
        let NodeData::Block(data) = self.context.arena().node(original)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TryStatement,
                field: "block",
            });
        };
        self.transform_execute_block(original, data, owner)
    }

    fn transform_execute_catch_clause(
        &mut self,
        id: NodeId,
        owner: SystemBlockScopeOwner,
    ) -> Result<TransformNode, TransformError> {
        let original = self.node(id);
        let NodeData::CatchClause(mut data) = self.context.arena().node(original)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TryStatement,
                field: "catch_clause",
            });
        };
        let owner = owner.entering(original, SyntaxKind::CatchClause);
        data.block = data
            .block
            .map(|id| {
                self.transform_execute_required_block(id, owner)
                    .map(TransformNode::node)
            })
            .transpose()?;
        self.update_generic_without_visit(original, NodeData::CatchClause(data))
    }

    fn transform_for_initializer(
        &mut self,
        initializer: Option<NodeId>,
        owner: SystemBlockScopeOwner,
    ) -> Result<Option<TransformNode>, TransformError> {
        let Some(initializer) = initializer else {
            return Ok(None);
        };
        let original = self.node(initializer);
        if !matches!(
            self.context.arena().node(original)?.data,
            NodeData::VariableDeclarationList(_)
        ) || !self.should_hoist_declaration_list(original, owner)?
        {
            return self.visit(initializer).map(Some);
        }

        let declarations =
            variable_declarations(self.context.arena(), self.source, Some(initializer))?;
        let mut expressions = Vec::new();
        for declaration in declarations {
            let NodeData::VariableDeclaration(variable) =
                self.context.arena().node(declaration)?.data.clone()
            else {
                continue;
            };
            if let Some(initializer) = variable.initializer {
                let initializer = self.visit(initializer)?;
                let flattened =
                    self.flatten_binding_initialization(declaration, variable.name, initializer)?;
                expressions.extend(flattened.into_expressions());
            } else if let Some(name) = variable.name {
                expressions.push(self.visit(name)?);
            }
        }
        let Some(expression) = self.inline_expressions(expressions)? else {
            return Ok(None);
        };
        self.set_original_and_range(expression, original)?;
        Ok(Some(expression))
    }

    fn transform_hoisted_variable_statement(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::VariableStatementData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let direct_export = has_modifier(
            self.context.arena(),
            self.source,
            data.modifiers,
            SyntaxKind::ExportKeyword,
        )?;
        let declarations =
            variable_declarations(self.context.arena(), self.source, data.declaration_list)?;
        let mut initialization_expressions = Vec::new();
        let mut trailing_exports = Vec::new();
        for declaration in declarations {
            let NodeData::VariableDeclaration(variable) =
                self.context.arena().node(declaration)?.data.clone()
            else {
                continue;
            };
            let Some(initializer) = variable.initializer else {
                continue;
            };
            let initializer = self.visit(initializer)?;
            let mut plan =
                self.flatten_binding_initialization(declaration, variable.name, initializer)?;
            if direct_export {
                for step in &mut plan.steps {
                    let SystemBindingStep::Bind { local, expression } = step else {
                        continue;
                    };
                    if let Some(exports) = self
                        .info
                        .common
                        .exports_by_local
                        .get(local.as_ref())
                        .cloned()
                    {
                        let mut wrapped = *expression;
                        for export in exports {
                            wrapped = self.create_export_call(&export, wrapped)?;
                        }
                        *expression = wrapped;
                    }
                }
            } else {
                for local in plan.steps.iter().filter_map(|step| match step {
                    SystemBindingStep::Bind { local, .. } => Some(local.to_string()),
                    SystemBindingStep::Evaluate(_) => None,
                }) {
                    for export in self
                        .info
                        .common
                        .exports_by_local
                        .get(local.as_str())
                        .cloned()
                        .unwrap_or_default()
                    {
                        let value = self.create_identifier(&local)?;
                        let call = self.create_export_call(&export, value)?;
                        trailing_exports.push(self.create_expression_statement(call)?);
                    }
                }
            }
            initialization_expressions.extend(plan.into_expressions());
        }
        let mut output = Vec::new();
        if let Some(expression) = self.inline_expressions(initialization_expressions)? {
            let statement = self.create_expression_statement(expression)?;
            self.set_original_and_range(statement, original)?;
            output.push(statement);
        }
        output.extend(trailing_exports);
        Ok(output)
    }

    fn flatten_binding_initialization(
        &mut self,
        declaration: TransformNode,
        name: Option<NodeId>,
        initializer: TransformNode,
    ) -> Result<SystemBindingPlan, TransformError> {
        let Some(name) = name.and_then(|id| self.context.arena().node_ref(self.source, id)) else {
            return Ok(SystemBindingPlan::default());
        };
        let mut plan = SystemBindingPlan::default();
        let key = self.context.arena().get_original_node(declaration).node();
        let value = if let Some(temp) = self.destructuring_temps.get(&key).cloned() {
            let target = self.create_identifier(&temp)?;
            plan.push_evaluation(self.create_assignment(target, initializer)?);
            self.create_identifier(&temp)?
        } else {
            initializer
        };
        self.flatten_system_binding_target(&mut plan, name, value)?;
        Ok(plan)
    }

    fn flatten_system_binding_target(
        &mut self,
        plan: &mut SystemBindingPlan,
        target: TransformNode,
        value: TransformNode,
    ) -> Result<(), TransformError> {
        match self.context.arena().node(target)?.data.clone() {
            NodeData::Identifier(data) => {
                let local = data.text;
                let target = self.create_identifier(&local)?;
                let assignment = self.create_assignment(target, value)?;
                plan.push_binding(local, assignment);
                Ok(())
            }
            NodeData::ObjectBindingPattern(data) => {
                self.flatten_system_object_binding(plan, data.elements, value)
            }
            NodeData::ArrayBindingPattern(data) => {
                self.flatten_system_array_binding(plan, data.elements, value)
            }
            _ => Ok(()),
        }
    }

    fn flatten_system_object_binding(
        &mut self,
        plan: &mut SystemBindingPlan,
        elements: Option<NodeArrayId>,
        mut value: TransformNode,
    ) -> Result<(), TransformError> {
        let elements = node_array_nodes(self.context.arena(), self.source, elements)?;
        if elements.len() != 1 {
            value = self.ensure_system_binding_identifier(plan, value, true)?;
        }
        let has_rest = elements.iter().any(|element| {
            matches!(
                self.context.arena().node(*element).map(|node| &node.data),
                Ok(NodeData::BindingElement(data)) if data.dot_dot_dot_token.is_some()
            )
        });
        let mut excluded = Vec::new();
        for (index, element) in elements.iter().copied().enumerate() {
            let element = self.system_binding_element(element)?;
            if element.rest {
                if index + 1 == elements.len() {
                    let rest =
                        self.create_system_object_rest(value, &excluded, element.original)?;
                    self.flatten_system_binding_element(plan, element, rest)?;
                }
                continue;
            }
            let (property_value, excluded_property) =
                self.create_system_binding_property_access(plan, value, element, has_rest)?;
            excluded.push(excluded_property);
            self.flatten_system_binding_element(plan, element, property_value)?;
        }
        Ok(())
    }

    fn flatten_system_array_binding(
        &mut self,
        plan: &mut SystemBindingPlan,
        elements: Option<NodeArrayId>,
        mut value: TransformNode,
    ) -> Result<(), TransformError> {
        let elements = node_array_nodes(self.context.arena(), self.source, elements)?;
        let all_omitted = !elements.is_empty()
            && elements.iter().all(|element| {
                self.context
                    .arena()
                    .node(*element)
                    .is_ok_and(|node| node.kind == SyntaxKind::OmittedExpression)
            });
        if elements.len() != 1 || all_omitted {
            value = self.ensure_system_binding_identifier(plan, value, !all_omitted)?;
        }
        for (index, element) in elements.into_iter().enumerate() {
            if self.context.arena().node(element)?.kind == SyntaxKind::OmittedExpression {
                continue;
            }
            let element = self.system_binding_element(element)?;
            let base = self.context.factory()?.clone_node(value)?;
            let element_value = if element.rest {
                let slice = self.create_property_access(base, "slice")?;
                let index = self.create_numeric_literal(&index.to_string())?;
                self.create_call(slice, vec![index])?
            } else {
                let index = self.create_numeric_literal(&index.to_string())?;
                self.create_element_access(base, index)?
            };
            self.flatten_system_binding_element(plan, element, element_value)?;
        }
        Ok(())
    }

    fn flatten_system_binding_element(
        &mut self,
        plan: &mut SystemBindingPlan,
        element: SystemBindingElement,
        mut value: TransformNode,
    ) -> Result<(), TransformError> {
        if let Some(initializer) = element.initializer {
            let initializer = self.visit(initializer.node())?;
            value = self.ensure_system_binding_identifier(plan, value, true)?;
            let condition_value = self.context.factory()?.clone_node(value)?;
            let undefined = self.create_void_zero()?;
            let condition = self.create_binary(
                condition_value,
                SyntaxKind::EqualsEqualsEqualsToken,
                undefined,
            )?;
            let fallback = self.context.factory()?.clone_node(value)?;
            value = self.create_conditional(condition, initializer, fallback)?;
        }
        self.flatten_system_binding_target(plan, element.target, value)
    }

    fn ensure_system_binding_identifier(
        &mut self,
        plan: &mut SystemBindingPlan,
        value: TransformNode,
        reuse_identifier: bool,
    ) -> Result<TransformNode, TransformError> {
        if reuse_identifier && self.context.arena().node(value)?.kind == SyntaxKind::Identifier {
            return Ok(value);
        }
        let temp = self.next_temp_name();
        self.push_hoisted_name(&temp);
        let target = self.create_identifier(&temp)?;
        plan.push_evaluation(self.create_assignment(target, value)?);
        self.create_identifier(&temp)
    }

    fn system_binding_element(
        &self,
        element: TransformNode,
    ) -> Result<SystemBindingElement, TransformError> {
        let NodeData::BindingElement(data) = &self.context.arena().node(element)?.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(element)?.kind,
                field: "binding element",
            });
        };
        let target = data
            .name
            .and_then(|name| self.context.arena().node_ref(self.source, name))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BindingElement,
                field: "name",
            })?;
        Ok(SystemBindingElement {
            original: element,
            target,
            property_name: data
                .property_name
                .and_then(|name| self.context.arena().node_ref(self.source, name))
                .or_else(|| data.dot_dot_dot_token.is_none().then_some(target)),
            initializer: data
                .initializer
                .and_then(|initializer| self.context.arena().node_ref(self.source, initializer)),
            rest: data.dot_dot_dot_token.is_some(),
        })
    }

    fn create_system_binding_property_access(
        &mut self,
        plan: &mut SystemBindingPlan,
        value: TransformNode,
        element: SystemBindingElement,
        property_is_reused: bool,
    ) -> Result<(TransformNode, SystemExcludedProperty), TransformError> {
        let property_name = element
            .property_name
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BindingElement,
                field: "property name",
            })?;
        let base = self.context.factory()?.clone_node(value)?;
        if let NodeData::ComputedPropertyName(data) =
            self.context.arena().node(property_name)?.data.clone()
        {
            let argument = data
                .expression
                .map(|expression| self.visit(expression))
                .transpose()?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ComputedPropertyName,
                    field: "expression",
                })?;
            let argument = if property_is_reused {
                self.ensure_system_binding_identifier(plan, argument, true)?
            } else {
                argument
            };
            let access_argument = self.context.factory()?.clone_node(argument)?;
            return Ok((
                self.create_element_access(base, access_argument)?,
                SystemExcludedProperty::Computed(argument),
            ));
        }
        let kind = self.context.arena().node(property_name)?.kind;
        let text = identifier_or_literal_text(self.context.arena(), property_name)?;
        let access = if matches!(
            kind,
            SyntaxKind::StringLiteral | SyntaxKind::NumericLiteral | SyntaxKind::BigIntLiteral
        ) {
            let argument = self.context.factory()?.clone_node(property_name)?;
            self.create_element_access(base, argument)?
        } else if is_identifier_export_name(&text) {
            self.create_property_access(base, &text)?
        } else {
            let argument = self.create_string_literal(&text)?;
            self.create_element_access(base, argument)?
        };
        Ok((access, SystemExcludedProperty::Named(text.into())))
    }

    fn create_system_object_rest(
        &mut self,
        value: TransformNode,
        excluded: &[SystemExcludedProperty],
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context
            .request_emit_helper(super::helpers::object_rest())?;
        let mut properties = Vec::with_capacity(excluded.len());
        for property in excluded {
            let property = match property {
                SystemExcludedProperty::Named(name) => self.create_string_literal(name)?,
                SystemExcludedProperty::Computed(temp) => {
                    let type_value = self.context.factory()?.clone_node(*temp)?;
                    let type_of = self.create_typeof(type_value)?;
                    let symbol = self.create_string_literal("symbol")?;
                    let condition =
                        self.create_binary(type_of, SyntaxKind::EqualsEqualsEqualsToken, symbol)?;
                    let symbol_value = self.context.factory()?.clone_node(*temp)?;
                    let string_value = self.context.factory()?.clone_node(*temp)?;
                    let empty = self.create_string_literal("")?;
                    let as_string =
                        self.create_binary(string_value, SyntaxKind::PlusToken, empty)?;
                    self.create_conditional(condition, symbol_value, as_string)?
                }
            };
            properties.push(property);
        }
        let excluded = self.create_array_literal(properties, false)?;
        self.context.factory()?.set_text_range(excluded, original)?;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::Rest)?;
        let value = self.context.factory()?.clone_node(value)?;
        self.create_call(helper, vec![value, excluded])
    }

    /// Mirrors flattenObject/ArrayBindingOrAssignmentPattern's root-value
    /// ownership rule. Identifiers can be safely reused across leaves; other
    /// expressions need a stable temporary whenever the root is read more
    /// than once.
    fn binding_pattern_requires_root_temp(
        &self,
        pattern: TransformNode,
        initializer: TransformNode,
    ) -> Result<bool, TransformError> {
        if self.context.arena().node(initializer)?.kind == SyntaxKind::Identifier {
            return Ok(false);
        }
        match &self.context.arena().node(pattern)?.data {
            NodeData::ObjectBindingPattern(data) => {
                Ok(node_array_nodes(self.context.arena(), self.source, data.elements)?.len() != 1)
            }
            NodeData::ArrayBindingPattern(data) => {
                let elements = node_array_nodes(self.context.arena(), self.source, data.elements)?;
                let all_omitted = !elements.is_empty()
                    && elements.iter().all(|element| {
                        self.context
                            .arena()
                            .node(*element)
                            .is_ok_and(|node| node.kind == SyntaxKind::OmittedExpression)
                    });
                Ok(elements.len() != 1 || all_omitted)
            }
            _ => Ok(false),
        }
    }

    fn transform_hoisted_class(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassDeclarationData,
        publish_exports: bool,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let key = self.context.arena().get_original_node(original).node();
        let local = data
            .name
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .and_then(|name| identifier_or_literal_text(self.context.arena(), name).ok())
            .or_else(|| {
                self.info
                    .common
                    .generated_declaration_names
                    .get(&key)
                    .map(ToString::to_string)
            });
        let exports = if publish_exports {
            local
                .as_deref()
                .map(|local| {
                    self.info.common.hoisted_declaration_exports(
                        self.context.arena(),
                        self.source,
                        data.modifiers,
                        local,
                    )
                })
                .transpose()?
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        data.modifiers = self.remove_export_modifiers(data.modifiers)?;
        let mut expression_data =
            NodeData::ClassExpression(tsc_syntax::nodes::ClassExpressionData {
                name: data.name,
                type_parameters: data.type_parameters,
                heritage_clauses: data.heritage_clauses,
                members: data.members,
                modifiers: data.modifiers,
            });
        try_visit_each_child(&mut expression_data, self)?;
        let flags = self.context.arena().transform_flags(original);
        let class_expression =
            self.context
                .factory()?
                .create_node(self.source, expression_data, flags)?;
        self.set_original_and_range(class_expression, original)?;
        let mut output = Vec::new();
        if let Some(local) = local {
            let target = self.create_identifier(&local)?;
            let assignment = self.create_assignment(target, class_expression)?;
            let statement = self.create_expression_statement(assignment)?;
            self.set_original_and_range(statement, original)?;
            output.push(statement);
            for export in exports {
                let value = self.create_identifier(&local)?;
                let call = self.create_export_call(&export, value)?;
                output.push(self.create_expression_statement(call)?);
            }
        } else {
            output.push(self.create_expression_statement(class_expression)?);
        }
        Ok(output)
    }

    fn visit(&mut self, id: NodeId) -> Result<TransformNode, TransformError> {
        self.visit_expression(id, false)
    }

    fn visit_expression(
        &mut self,
        id: NodeId,
        value_is_discarded: bool,
    ) -> Result<TransformNode, TransformError> {
        let original = self
            .context
            .arena()
            .node_ref(self.source, id)
            .ok_or_else(|| TransformError::UnknownNode(self.node(id)))?;
        let record = self.context.arena().node(original)?.clone();
        match record.data {
            NodeData::Token => Ok(original),
            NodeData::Identifier(_) => self.substitute_import_identifier(original),
            NodeData::CallExpression(data) => self.visit_call_expression(original, data),
            NodeData::MetaProperty(data) => self.visit_meta_property(original, data),
            NodeData::BinaryExpression(data) => self.visit_binary_expression(original, data),
            NodeData::PrefixUnaryExpression(data) => self.visit_prefix_expression(original, data),
            NodeData::PostfixUnaryExpression(data) => {
                self.visit_postfix_expression(original, data, value_is_discarded)
            }
            NodeData::ShorthandPropertyAssignment(data) => {
                self.visit_shorthand_property(original, data)
            }
            data => self.update_generic(original, data),
        }
    }

    fn visit_call_expression(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::CallExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let is_dynamic_import = data
            .expression
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .is_some_and(|expression| {
                self.context
                    .arena()
                    .node(expression)
                    .is_ok_and(|node| node.kind == SyntaxKind::ImportKeyword)
            });
        if is_dynamic_import {
            let arguments = node_array_nodes(self.context.arena(), self.source, data.arguments)?;
            let arguments = arguments
                .into_iter()
                .map(|argument| self.visit(argument.node()))
                .collect::<Result<Vec<_>, _>>()?;
            let context = self.create_identifier(&self.context_name.clone())?;
            let import = self.create_property_access(context, "import")?;
            let transformed = self.create_call(import, arguments)?;
            self.set_original_and_range(transformed, original)?;
            return Ok(transformed);
        }

        let mut node_data = NodeData::CallExpression(data);
        try_visit_each_child(&mut node_data, self)?;
        let NodeData::CallExpression(data) = node_data else {
            unreachable!("call expression visitor preserves kind")
        };
        // SystemJS import substitution intentionally keeps a substituted
        // namespace property as the direct callee. Unlike CommonJS, tsc's
        // System transform does not synthesize `(0, imported)(...)` here.
        self.update_generic_without_visit(original, NodeData::CallExpression(data))
    }

    fn visit_meta_property(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::MetaPropertyData,
    ) -> Result<TransformNode, TransformError> {
        let is_import_meta = data.keyword_token == SyntaxKind::ImportKeyword
            && data
                .name
                .and_then(|id| self.context.arena().node_ref(self.source, id))
                .and_then(|name| identifier_or_literal_text(self.context.arena(), name).ok())
                .as_deref()
                == Some("meta");
        if !is_import_meta {
            return self.update_generic(original, NodeData::MetaProperty(data));
        }
        let context = self.create_identifier(&self.context_name.clone())?;
        let transformed = self.create_property_access(context, "meta")?;
        self.set_original_and_range(transformed, original)?;
        Ok(transformed)
    }

    fn visit_binary_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::BinaryExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let exports = if data
            .operator_token
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .and_then(|operator| self.context.arena().node(operator).ok())
            .is_some_and(|operator| {
                operator.kind.value() >= SyntaxKind::FirstAssignment.value()
                    && operator.kind.value() <= SyntaxKind::LastAssignment.value()
            }) {
            data.left
                .and_then(|id| self.context.arena().node_ref(self.source, id))
                .map(|left| self.exports_for_identifier(left))
                .transpose()?
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        data.left = data
            .left
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        data.right = data
            .right
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        let mut expression =
            self.update_generic_without_visit(original, NodeData::BinaryExpression(data))?;
        for export in exports {
            expression = self.create_export_call(&export, expression)?;
        }
        Ok(expression)
    }

    fn visit_prefix_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::PrefixUnaryExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let exports = if matches!(
            data.operator,
            SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
        ) {
            data.operand
                .and_then(|id| self.context.arena().node_ref(self.source, id))
                .map(|operand| self.exports_for_identifier(operand))
                .transpose()?
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        data.operand = data
            .operand
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        let mut expression =
            self.update_generic_without_visit(original, NodeData::PrefixUnaryExpression(data))?;
        for export in exports {
            expression = self.create_export_call(&export, expression)?;
        }
        Ok(expression)
    }

    fn visit_postfix_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::PostfixUnaryExpressionData,
        value_is_discarded: bool,
    ) -> Result<TransformNode, TransformError> {
        let original_operand = data
            .operand
            .and_then(|id| self.context.arena().node_ref(self.source, id));
        let exports = if matches!(
            data.operator,
            SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
        ) {
            original_operand
                .map(|operand| self.exports_for_identifier(operand))
                .transpose()?
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        data.operand = data
            .operand
            .map(|id| self.visit(id).map(TransformNode::node))
            .transpose()?;
        let update =
            self.update_generic_without_visit(original, NodeData::PostfixUnaryExpression(data))?;
        if exports.is_empty() {
            return Ok(update);
        }
        let operand_text = original_operand
            .and_then(|operand| identifier_or_literal_text(self.context.arena(), operand).ok())
            .unwrap_or_default();
        if value_is_discarded {
            let current = self.create_identifier(&operand_text)?;
            let comma = self.create_binary(update, SyntaxKind::CommaToken, current)?;
            let mut expression = self.create_parenthesized(comma)?;
            for export in exports {
                expression = self.create_export_call(&export, expression)?;
            }
            return Ok(expression);
        }
        let temp = self.next_temp_name();
        self.push_hoisted_name(&temp);
        let temp_target = self.create_identifier(&temp)?;
        let save = self.create_assignment(temp_target, update)?;
        let current = self.create_identifier(&operand_text)?;
        let mut publish = current;
        for export in exports {
            publish = self.create_export_call(&export, publish)?;
        }
        let first = self.create_binary(save, SyntaxKind::CommaToken, publish)?;
        let temp_value = self.create_identifier(&temp)?;
        let result = self.create_binary(first, SyntaxKind::CommaToken, temp_value)?;
        self.create_parenthesized(result)
    }

    fn visit_shorthand_property(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ShorthandPropertyAssignmentData,
    ) -> Result<TransformNode, TransformError> {
        let Some(name) = data
            .name
            .and_then(|id| self.context.arena().node_ref(self.source, id))
        else {
            return self.update_generic(original, NodeData::ShorthandPropertyAssignment(data));
        };
        let substituted = self.substitute_import_identifier(name)?;
        if substituted == name {
            return self.update_generic(original, NodeData::ShorthandPropertyAssignment(data));
        }
        let property = self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                name: Some(name.node()),
                initializer: Some(substituted.node()),
                modifiers: None,
                question_token: None,
                exclamation_token: None,
            }),
            TransformFlags::NONE,
        )?;
        self.set_original_and_range(property, original)?;
        Ok(property)
    }

    fn substitute_import_identifier(
        &mut self,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let Some(binding) = self.import_binding_for_reference(original)? else {
            return Ok(original);
        };
        let target = self.create_identifier(&binding.generated_name)?;
        let transformed = if let Some(property) = binding.property {
            if is_identifier_export_name(&property) {
                self.create_property_access(target, &property)?
            } else {
                let property = self.create_string_literal(&property)?;
                self.create_element_access(target, property)?
            }
        } else {
            target
        };
        self.set_original_and_range(transformed, original)?;
        Ok(transformed)
    }

    fn import_binding_for_reference(
        &self,
        node: TransformNode,
    ) -> Result<Option<ImportBinding>, TransformError> {
        if let Some(metadata) = self.context.arena().metadata(node) {
            if metadata.is_generated_import_reference() {
                return Ok(None);
            }
            let Some(declaration) = metadata.referenced_import_declaration() else {
                return self.import_binding_for_parsed_reference(node);
            };
            return Ok(self
                .info
                .common
                .import_bindings
                .get(&declaration.node())
                .cloned());
        }
        self.import_binding_for_parsed_reference(node)
    }

    fn import_binding_for_parsed_reference(
        &self,
        node: TransformNode,
    ) -> Result<Option<ImportBinding>, TransformError> {
        let original = self.context.arena().get_original_node(node);
        if NodeFlags::from_bits(self.context.arena().node(original)?.flags)
            .contains(NodeFlags::SYNTHESIZED)
        {
            return Ok(None);
        }
        let resolver_node = self.resolver_node(node)?;
        let declaration = self
            .resolver
            .get_referenced_import_declaration(resolver_node)?;
        Ok(declaration.and_then(|declaration| {
            self.info
                .common
                .import_bindings
                .get(&declaration.node())
                .cloned()
        }))
    }

    fn exports_for_identifier(&self, node: TransformNode) -> Result<Vec<Box<str>>, TransformError> {
        if self.context.arena().node(node)?.kind != SyntaxKind::Identifier {
            return Ok(Vec::new());
        }
        // `getLocalName` projections are references to the local binding
        // inside a generated lowering (for example the standard-decorator
        // class IIFE). They retain the parsed declaration as resolver
        // provenance, but must not turn an internal assignment into a live
        // SystemJS publication. CommonJS applies the same ownership rule in
        // `is_local_name`; keep SystemJS aligned with that semantic boundary.
        if self
            .context
            .arena()
            .metadata(node)
            .is_some_and(|metadata| metadata.flags().contains(crate::EmitFlags::LOCAL_NAME))
        {
            return Ok(Vec::new());
        }
        let original = self.context.arena().get_original_node(node);
        if self.context.arena().node(original)?.pos == u32::MAX
            || NodeFlags::from_bits(self.context.arena().node(original)?.flags)
                .contains(NodeFlags::SYNTHESIZED)
        {
            return Ok(Vec::new());
        }
        let resolver_node = self.resolver_node(node)?;
        let exported_from_source = self
            .resolver
            .get_referenced_export_container(resolver_node, EmitExportContainerMode::Reference)?
            .is_some();
        let value_declaration = self
            .resolver
            .get_referenced_value_declaration(resolver_node)?;
        if let Some(exports) = value_declaration
            .and_then(|declaration| self.info.common.exported_bindings.get(&declaration.node()))
        {
            return Ok(exports.clone());
        }
        for declaration in self
            .resolver
            .get_referenced_value_declarations(resolver_node)?
        {
            if Some(declaration) == value_declaration {
                continue;
            }
            if let Some(exports) = self.info.common.exported_bindings.get(&declaration.node()) {
                return Ok(exports.clone());
            }
        }
        if !exported_from_source {
            return Ok(Vec::new());
        }
        let name = identifier_or_literal_text(self.context.arena(), node)?;
        Ok(self
            .info
            .common
            .exports_by_local
            .get(name.as_str())
            .cloned()
            .unwrap_or_default())
    }

    fn has_export_star(&self) -> bool {
        self.info.dependency_groups.iter().any(|group| {
            group.entries.iter().any(|id| {
                self.context
                    .arena()
                    .node(self.node(*id))
                    .is_ok_and(|node| {
                        matches!(&node.data, NodeData::ExportDeclaration(data) if data.export_clause.is_none())
                    })
            })
        })
    }

    fn create_export_star_prelude(&mut self) -> Result<Vec<TransformNode>, TransformError> {
        let mut output = Vec::new();
        let mut exported_names = self.info.non_function_exported_names.clone();
        for (export, _) in self.info.common.hoisted_function_exports.clone() {
            push_unique(&mut exported_names, &export);
        }
        let local_names = if exported_names.is_empty() {
            None
        } else {
            let name = unique_generated_name(&mut self.used_names, "exportedNames");
            let properties = exported_names
                .into_iter()
                .map(|export| {
                    let name = self.create_string_literal(&export)?;
                    let value = self.context.factory()?.create_token(
                        self.source,
                        SyntaxKind::TrueKeyword,
                        TransformFlags::NONE,
                    )?;
                    self.create_property_assignment(name, value)
                })
                .collect::<Result<Vec<_>, TransformError>>()?;
            let object = self.create_object_literal(properties, true)?;
            let declaration = self.create_variable_declaration(&name, Some(object))?;
            output.push(self.create_variable_statement(vec![declaration], NodeFlags::NONE)?);
            Some(name)
        };
        let function_name = unique_generated_name(&mut self.used_names, "exportStar");
        let function = self.create_export_star_function(&function_name, local_names.as_deref())?;
        output.push(function);
        Ok(output)
    }

    fn create_export_star_function(
        &mut self,
        function_name: &str,
        local_names: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let module_parameter = self.create_parameter("m")?;
        let exports_object = self.create_object_literal(Vec::new(), false)?;
        let exports_declaration =
            self.create_variable_declaration("exports", Some(exports_object))?;
        let exports_statement =
            self.create_variable_statement(vec![exports_declaration], NodeFlags::NONE)?;

        let n = self.create_identifier("n")?;
        let default = self.create_string_literal("default")?;
        let mut condition =
            self.create_binary(n, SyntaxKind::ExclamationEqualsEqualsToken, default)?;
        if let Some(local_names) = local_names {
            let local = self.create_identifier(local_names)?;
            let has_own = self.create_property_access(local, "hasOwnProperty")?;
            let n = self.create_identifier("n")?;
            let call = self.create_call(has_own, vec![n])?;
            let not = self.context.factory()?.create_node(
                self.source,
                NodeData::PrefixUnaryExpression(tsc_syntax::nodes::PrefixUnaryExpressionData {
                    operator: SyntaxKind::ExclamationToken,
                    operand: Some(call.node()),
                }),
                TransformFlags::NONE,
            )?;
            condition = self.create_binary(condition, SyntaxKind::AmpersandAmpersandToken, not)?;
        }

        let exports = self.create_identifier("exports")?;
        let n = self.create_identifier("n")?;
        let left = self.create_element_access(exports, n)?;
        let module = self.create_identifier("m")?;
        let n = self.create_identifier("n")?;
        let right = self.create_element_access(module, n)?;
        let assignment = self.create_assignment(left, right)?;
        let assignment = self.create_expression_statement(assignment)?;
        self.context
            .arena_mut()?
            .metadata_mut(assignment)
            .add_flags(crate::EmitFlags::SINGLE_LINE);
        let if_statement = self.context.factory()?.create_node(
            self.source,
            NodeData::IfStatement(tsc_syntax::nodes::IfStatementData {
                expression: Some(condition.node()),
                then_statement: Some(assignment.node()),
                else_statement: None,
            }),
            TransformFlags::NONE,
        )?;
        self.context
            .arena_mut()?
            .metadata_mut(if_statement)
            .add_flags(crate::EmitFlags::SINGLE_LINE);
        let for_body = self.create_block(vec![if_statement], true)?;
        let n_declaration = self.create_variable_declaration("n", None)?;
        let n_list = self.create_variable_declaration_list(vec![n_declaration], NodeFlags::NONE)?;
        let module = self.create_identifier("m")?;
        let for_in = self.context.factory()?.create_node(
            self.source,
            NodeData::ForInStatement(tsc_syntax::nodes::ForInStatementData {
                statement: Some(for_body.node()),
                initializer: Some(n_list.node()),
                expression: Some(module.node()),
            }),
            TransformFlags::NONE,
        )?;
        let exports = self.create_identifier("exports")?;
        let exports_function = self.create_identifier(&self.exports_name.clone())?;
        let publish = self.create_call(exports_function, vec![exports])?;
        let publish = self.create_expression_statement(publish)?;
        let body = self.create_block(vec![exports_statement, for_in, publish], true)?;
        let name = self.create_identifier(function_name)?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, vec![module_parameter])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::FunctionDeclaration(tsc_syntax::nodes::FunctionDeclarationData {
                name: Some(name.node()),
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                asterisk_token: None,
                body: Some(body.node()),
                modifiers: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_setters_array(&mut self) -> Result<TransformNode, TransformError> {
        let groups = self.info.dependency_groups.clone();
        let mut setters = Vec::new();
        for group in groups {
            let mut local_name = None;
            let mut side_effect_only = true;
            for entry in &group.entries {
                let entry = self.node(*entry);
                match &self.context.arena().node(entry)?.data {
                    NodeData::ImportDeclaration(data) => {
                        if data.import_clause.is_some() {
                            side_effect_only = false;
                            let key = self.context.arena().get_original_node(entry).node();
                            if let Some(plan) = self.info.common.imports.get(&key) {
                                local_name = plan.runtime_name.as_deref().map(str::to_owned);
                                break;
                            }
                        }
                    }
                    NodeData::ImportEqualsDeclaration(data) => {
                        side_effect_only = false;
                        local_name = data
                            .name
                            .and_then(|id| self.context.arena().node_ref(self.source, id))
                            .and_then(|name| {
                                identifier_or_literal_text(self.context.arena(), name).ok()
                            });
                        break;
                    }
                    _ => side_effect_only = false,
                }
            }
            let parameter_base = local_name.clone().unwrap_or_else(|| {
                if side_effect_only {
                    String::new()
                } else {
                    group.fallback_generated_name.to_string()
                }
            });
            let parameter_name = unique_generated_name(&mut self.used_names, &parameter_base);
            let mut statements = Vec::new();
            for entry in group.entries {
                let entry = self.node(entry);
                match self.context.arena().node(entry)?.data.clone() {
                    NodeData::ImportDeclaration(data) if data.import_clause.is_some() => {
                        let key = self.context.arena().get_original_node(entry).node();
                        if let Some(plan) = self.info.common.imports.get(&key).cloned() {
                            if let Some(runtime_name) = plan.runtime_name.as_deref() {
                                let target = self.create_identifier(runtime_name)?;
                                let value = self.create_identifier(&parameter_name)?;
                                let assignment = self.create_assignment(target, value)?;
                                statements.push(self.create_expression_statement(assignment)?);
                                if let Some(namespace_alias) = plan.namespace_alias.as_deref() {
                                    let target = self.create_identifier(namespace_alias)?;
                                    let value = self.create_identifier(runtime_name)?;
                                    let assignment = self.create_assignment(target, value)?;
                                    statements.push(self.create_expression_statement(assignment)?);
                                }
                            }
                        }
                    }
                    NodeData::ImportEqualsDeclaration(data) => {
                        let name = data
                            .name
                            .and_then(|id| self.context.arena().node_ref(self.source, id))
                            .and_then(|name| {
                                identifier_or_literal_text(self.context.arena(), name).ok()
                            })
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::ImportEqualsDeclaration,
                                field: "name",
                            })?;
                        let target = self.create_identifier(&name)?;
                        let value = self.create_identifier(&parameter_name)?;
                        let assignment = self.create_assignment(target, value)?;
                        statements.push(self.create_expression_statement(assignment)?);
                        if has_modifier(
                            self.context.arena(),
                            self.source,
                            data.modifiers,
                            SyntaxKind::ExportKeyword,
                        )? {
                            let value = self.create_identifier(&parameter_name)?;
                            let call = self.create_export_call(&name, value)?;
                            statements.push(self.create_expression_statement(call)?);
                        }
                    }
                    NodeData::ExportDeclaration(data) => {
                        if let Some(clause) = data
                            .export_clause
                            .and_then(|id| self.context.arena().node_ref(self.source, id))
                        {
                            if let NodeData::NamedExports(named) =
                                self.context.arena().node(clause)?.data.clone()
                            {
                                let mut properties = Vec::new();
                                for specifier in node_array_nodes(
                                    self.context.arena(),
                                    self.source,
                                    named.elements,
                                )? {
                                    let NodeData::ExportSpecifier(specifier) =
                                        self.context.arena().node(specifier)?.data.clone()
                                    else {
                                        continue;
                                    };
                                    let export = specifier
                                        .name
                                        .and_then(|id| {
                                            self.context.arena().node_ref(self.source, id)
                                        })
                                        .and_then(|name| {
                                            identifier_or_literal_text(self.context.arena(), name)
                                                .ok()
                                        })
                                        .unwrap_or_default();
                                    let property = specifier
                                        .property_name
                                        .or(specifier.name)
                                        .and_then(|id| {
                                            self.context.arena().node_ref(self.source, id)
                                        })
                                        .and_then(|name| {
                                            identifier_or_literal_text(self.context.arena(), name)
                                                .ok()
                                        })
                                        .unwrap_or_default();
                                    let name = self.create_string_literal(&export)?;
                                    let parameter = self.create_identifier(&parameter_name)?;
                                    let property = self.create_string_literal(&property)?;
                                    let value = self.create_element_access(parameter, property)?;
                                    properties.push(self.create_property_assignment(name, value)?);
                                }
                                let object = self.create_object_literal(properties, true)?;
                                let exports_function =
                                    self.create_identifier(&self.exports_name.clone())?;
                                let call = self.create_call(exports_function, vec![object])?;
                                statements.push(self.create_expression_statement(call)?);
                            }
                        } else {
                            let helper = self.export_star_function_name();
                            let parameter = self.create_identifier(&parameter_name)?;
                            let helper = self.create_identifier(&helper)?;
                            let call = self.create_call(helper, vec![parameter])?;
                            statements.push(self.create_expression_statement(call)?);
                        }
                    }
                    _ => {}
                }
            }
            let body = self.create_block(statements, true)?;
            let parameter = self.create_parameter(&parameter_name)?;
            setters.push(self.create_function_expression(vec![parameter], body, None)?);
        }
        self.create_array_literal(setters, true)
    }

    fn export_star_function_name(&self) -> String {
        self.used_names
            .iter()
            .find(|name| name.starts_with("exportStar_"))
            .cloned()
            .unwrap_or_else(|| "exportStar_1".to_owned())
    }

    fn create_use_strict(&mut self) -> Result<TransformNode, TransformError> {
        let literal = self.create_string_literal("use strict")?;
        let statement = self.create_expression_statement(literal)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(crate::EmitFlags::CUSTOM_PROLOGUE);
        Ok(statement)
    }

    fn create_module_name_statement(&mut self) -> Result<TransformNode, TransformError> {
        let context = self.create_identifier(&self.context_name.clone())?;
        let context_again = self.create_identifier(&self.context_name.clone())?;
        let id = self.create_property_access(context_again, "id")?;
        let value = self.create_binary(context, SyntaxKind::AmpersandAmpersandToken, id)?;
        let declaration = self.create_variable_declaration("__moduleName", Some(value))?;
        self.create_variable_statement(vec![declaration], NodeFlags::NONE)
    }

    fn create_export_call(
        &mut self,
        name: &str,
        value: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let exports = self.create_identifier(&self.exports_name.clone())?;
        let name = self.create_string_literal(name)?;
        self.create_call(exports, vec![name, value])
    }

    fn inline_expressions(
        &mut self,
        expressions: Vec<TransformNode>,
    ) -> Result<Option<TransformNode>, TransformError> {
        let mut expressions = expressions.into_iter();
        let Some(mut current) = expressions.next() else {
            return Ok(None);
        };
        for expression in expressions {
            current = self.create_binary(current, SyntaxKind::CommaToken, expression)?;
        }
        Ok(Some(current))
    }

    fn remove_export_modifiers(
        &mut self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(array) =
            modifiers.and_then(|id| self.context.arena().node_array_ref(self.source, id))
        else {
            return Ok(None);
        };
        let retained = self
            .context
            .arena()
            .node_array(array)?
            .nodes
            .iter()
            .filter_map(|id| self.context.arena().node_ref(self.source, *id))
            .filter(|modifier| {
                self.context.arena().node(*modifier).is_ok_and(|node| {
                    !matches!(
                        node.kind,
                        SyntaxKind::ExportKeyword | SyntaxKind::DefaultKeyword
                    )
                })
            })
            .collect::<Vec<_>>();
        if retained.is_empty() {
            Ok(None)
        } else {
            Ok(Some(
                self.context
                    .factory()?
                    .update_node_array(array, retained)?
                    .array(),
            ))
        }
    }

    fn update_generic(
        &mut self,
        original: TransformNode,
        mut data: NodeData,
    ) -> Result<TransformNode, TransformError> {
        try_visit_each_child(&mut data, self)?;
        self.update_generic_without_visit(original, data)
    }

    fn update_generic_without_visit(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<TransformNode, TransformError> {
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        self.context.factory()?.update_node(original, data, flags)
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

    fn create_numeric_literal(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_typeof(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.context.arena().propagate_child_flags(expression)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::TypeOfExpression(tsc_syntax::nodes::TypeOfExpressionData {
                expression: Some(expression.node()),
            }),
            flags,
        )
    }

    fn create_void_zero(&mut self) -> Result<TransformNode, TransformError> {
        let zero = self.create_numeric_literal("0")?;
        let flags = self.context.arena().propagate_child_flags(zero)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VoidExpression(tsc_syntax::nodes::VoidExpressionData {
                expression: Some(zero.node()),
            }),
            flags,
        )
    }

    fn create_conditional(
        &mut self,
        condition: TransformNode,
        when_true: TransformNode,
        when_false: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let question = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::QuestionToken,
            TransformFlags::NONE,
        )?;
        let colon = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ColonToken,
            TransformFlags::NONE,
        )?;
        let flags = self.context.arena().transform_flags(condition)
            | self.context.arena().transform_flags(when_true)
            | self.context.arena().transform_flags(when_false);
        self.context.factory()?.create_node(
            self.source,
            NodeData::ConditionalExpression(tsc_syntax::nodes::ConditionalExpressionData {
                condition: Some(condition.node()),
                question_token: Some(question.node()),
                when_true: Some(when_true.node()),
                colon_token: Some(colon.node()),
                when_false: Some(when_false.node()),
            }),
            flags,
        )
    }

    fn create_parameter(&mut self, name: &str) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::Parameter(tsc_syntax::nodes::ParameterData {
                name: Some(name.node()),
                modifiers: None,
                dot_dot_dot_token: None,
                question_token: None,
                r#type: None,
                initializer: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_array_literal(
        &mut self,
        elements: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let elements = self
            .context
            .factory()?
            .create_node_array(self.source, elements)?;
        let array = self.context.factory()?.create_node(
            self.source,
            NodeData::ArrayLiteralExpression(tsc_syntax::nodes::ArrayLiteralExpressionData {
                elements: Some(elements.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.set_multi_line(array, multi_line)
    }

    fn create_object_literal(
        &mut self,
        properties: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let properties = self
            .context
            .factory()?
            .create_node_array(self.source, properties)?;
        let object = self.context.factory()?.create_node(
            self.source,
            NodeData::ObjectLiteralExpression(tsc_syntax::nodes::ObjectLiteralExpressionData {
                properties: Some(properties.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.set_multi_line(object, multi_line)
    }

    fn create_property_assignment_identifier(
        &mut self,
        name: &str,
        value: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        self.create_property_assignment(name, value)
    }

    fn create_property_assignment(
        &mut self,
        name: TransformNode,
        value: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                name: Some(name.node()),
                initializer: Some(value.node()),
                modifiers: None,
                question_token: None,
                exclamation_token: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_function_expression(
        &mut self,
        parameters: Vec<TransformNode>,
        body: TransformNode,
        modifiers: Option<NodeArrayId>,
    ) -> Result<TransformNode, TransformError> {
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, parameters)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::FunctionExpression(tsc_syntax::nodes::FunctionExpressionData {
                name: None,
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                asterisk_token: None,
                body: Some(body.node()),
                modifiers,
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
                name: Some(name.node()),
                expression: Some(expression.node()),
                question_dot_token: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_element_access(
        &mut self,
        expression: TransformNode,
        argument: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::ElementAccessExpression(tsc_syntax::nodes::ElementAccessExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                argument_expression: Some(argument.node()),
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

    fn create_parenthesized(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                expression: Some(expression.node()),
            }),
            TransformFlags::NONE,
        )
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
        name: &str,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
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

    fn create_variable_statement(
        &mut self,
        declarations: Vec<TransformNode>,
        flags: NodeFlags,
    ) -> Result<TransformNode, TransformError> {
        let list = self.create_variable_declaration_list(declarations, flags)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers: None,
                declaration_list: Some(list.node()),
            }),
            TransformFlags::NONE,
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

    fn create_return_statement(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                expression: Some(expression.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn set_original_and_range(
        &mut self,
        node: TransformNode,
        original: TransformNode,
    ) -> Result<(), TransformError> {
        self.context.factory()?.set_text_range(node, original)?;
        self.context
            .arena_mut()?
            .set_original_node(node, Some(original))
    }

    fn resolver_node(&self, node: TransformNode) -> Result<EmitResolverNode, TransformError> {
        self.context.arena().require_parse_tree_resolver_node(node)
    }

    const fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    const fn array(&self, id: NodeArrayId) -> TransformNodeArray {
        TransformNodeArray::new(self.source, id)
    }
}

impl NodeDataChildVisitor for SystemVisitor<'_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("System child belongs to its transform source")
            .kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.visit(id).map(|node| Some(node.node()))
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        if let Some(mapped) = self.arrays.get(&id) {
            return Ok(Some(*mapped));
        }
        let original = self.array(id);
        let nodes = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(nodes.len());
        for node in nodes {
            visited.push(self.visit(node)?);
        }
        let updated = self
            .context
            .factory()?
            .update_node_array(original, visited)?;
        self.arrays.insert(id, updated.array());
        Ok(Some(updated.array()))
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}

pub(super) fn collect_identifier_texts(
    arena: &TransformArena,
    source: TransformSourceId,
) -> BTreeSet<String> {
    let syntax = match arena.source(source) {
        Ok(source) => source.syntax(),
        Err(_) => return BTreeSet::new(),
    };
    syntax
        .arena
        .nodes()
        .iter()
        .filter_map(|node| match &node.data {
            NodeData::Identifier(data) => Some(data.text.clone()),
            _ => None,
        })
        .collect()
}

fn unique_generated_name(used: &mut BTreeSet<String>, base: &str) -> String {
    let base = base.trim_end_matches('_');
    let mut ordinal = 1usize;
    loop {
        let candidate = if base.is_empty() {
            format!("_{ordinal}")
        } else {
            format!("{base}_{ordinal}")
        };
        if used.insert(candidate.clone()) {
            return candidate;
        }
        ordinal += 1;
    }
}

fn system_is_use_strict(
    arena: &TransformArena,
    statement: TransformNode,
) -> Result<bool, TransformError> {
    let NodeData::ExpressionStatement(data) = &arena.node(statement)?.data else {
        return Ok(false);
    };
    let Some(expression) = data
        .expression
        .and_then(|id| arena.node_ref(statement.source(), id))
    else {
        return Ok(false);
    };
    Ok(
        matches!(&arena.node(expression)?.data, NodeData::StringLiteral(data) if data.text == "use strict"),
    )
}
