use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags};

use crate::{
    EmitHint, EmitResolver, EmitResolverNode, TransformArena, TransformError, TransformFlags,
    TransformNode, TransformNodeArray, TransformRoot, TransformSourceId, TransformationContext,
    Transformer, UnsupportedEmitFeature,
};

use super::{
    flags_after_update, generated_module_name, has_modifier, identifier_or_literal_text,
    is_identifier_export_name, is_prologue_statement, node_array_nodes,
    source_contains_dynamic_import, source_file_statement_nodes, string_literal_text,
    variable_declarations, CommonJsModuleInfo, ImportBinding, MODULE_SYSTEM,
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

        let common = CommonJsModuleInfo::collect(context.arena(), source, root)?;
        let info = SystemModuleInfo::collect(context.arena(), source, root, common)?;
        let mut visitor =
            SystemVisitor::new(context, source, self.resolver, info, self.always_strict);
        let updated = visitor.transform_source_file(root)?;
        visitor.context.arena_mut()?.replace_root(source, updated)?;
        Ok(TransformRoot::SourceFile(source))
    }

    fn substitute_node(
        &mut self,
        _context: &TransformationContext,
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
    destructuring_temps: BTreeMap<NodeId, String>,
    temp_ordinal: usize,
    arrays: BTreeMap<NodeArrayId, NodeArrayId>,
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
        self.collect_hoisted_names(&input)?;

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
        let mut hoisted_functions = Vec::new();
        for statement in input.into_iter().skip(offset) {
            if self.context.arena().node(statement)?.kind == SyntaxKind::FunctionDeclaration {
                hoisted_functions.extend(self.transform_hoisted_function(statement)?);
            } else {
                execute.extend(self.transform_execute_statement(statement)?);
            }
        }

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
        outer.extend(hoisted_functions);

        if self.has_export_star() {
            outer.extend(self.create_export_star_prelude()?);
        }

        let setters = self.create_setters_array()?;
        let execute_body = self.create_block(execute, true)?;
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
    ) -> Result<(), TransformError> {
        for group in self.info.dependency_groups.clone() {
            for entry in group.entries {
                let entry = self.node(entry);
                if let NodeData::ImportDeclaration(data) = &self.context.arena().node(entry)?.data {
                    if data.import_clause.is_some() {
                        let key = self.context.arena().get_original_node(entry).node();
                        if let Some(name) = self
                            .info
                            .common
                            .imports
                            .get(&key)
                            .map(|plan| plan.generated_name.to_string())
                        {
                            self.push_hoisted_name(&name);
                        }
                    }
                }
            }
        }
        for statement in statements {
            self.collect_statement_hoists(*statement, true)?;
        }
        Ok(())
    }

    fn collect_statement_hoists(
        &mut self,
        statement: TransformNode,
        direct: bool,
    ) -> Result<(), TransformError> {
        let record = self.context.arena().node(statement)?.clone();
        match record.data {
            NodeData::VariableStatement(data) => {
                let Some(list) = data
                    .declaration_list
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                else {
                    return Ok(());
                };
                let block_scoped = NodeFlags::from_bits(self.context.arena().node(list)?.flags)
                    .contains(NodeFlags::BLOCK_SCOPED);
                if direct || !block_scoped {
                    for declaration in variable_declarations(
                        self.context.arena(),
                        self.source,
                        data.declaration_list,
                    )? {
                        if let NodeData::VariableDeclaration(data) =
                            self.context.arena().node(declaration)?.data.clone()
                        {
                            if data
                                .name
                                .and_then(|id| self.context.arena().node_ref(self.source, id))
                                .is_some_and(|name| {
                                    self.context.arena().node(name).is_ok_and(|node| {
                                        matches!(
                                            node.kind,
                                            SyntaxKind::ObjectBindingPattern
                                                | SyntaxKind::ArrayBindingPattern
                                        )
                                    })
                                })
                                && data.initializer.is_some()
                            {
                                let temp = self.next_temp_name();
                                self.destructuring_temps.insert(
                                    self.context.arena().get_original_node(declaration).node(),
                                    temp.clone(),
                                );
                                self.push_hoisted_name(&temp);
                            }
                            let mut names = Vec::new();
                            collect_binding_names(
                                self.context.arena(),
                                self.source,
                                data.name,
                                &mut names,
                            )?;
                            for name in names {
                                self.push_hoisted_name(&name);
                            }
                        }
                    }
                }
            }
            NodeData::ClassDeclaration(data) if direct => {
                if let Some(name) = data
                    .name
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                    .and_then(|name| identifier_or_literal_text(self.context.arena(), name).ok())
                {
                    self.push_hoisted_name(&name);
                }
            }
            NodeData::FunctionDeclaration(_)
            | NodeData::FunctionExpression(_)
            | NodeData::ArrowFunction(_)
            | NodeData::ClassDeclaration(_)
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
                        self.collect_statement_hoists(child, false)?;
                    }
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
    ) -> Result<Vec<TransformNode>, TransformError> {
        let NodeData::FunctionDeclaration(mut data) =
            self.context.arena().node(original)?.data.clone()
        else {
            return Ok(Vec::new());
        };
        let local = data
            .name
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .and_then(|name| identifier_or_literal_text(self.context.arena(), name).ok());
        data.modifiers = self.remove_export_modifiers(data.modifiers)?;
        let function = self.update_generic(original, NodeData::FunctionDeclaration(data))?;
        let mut output = vec![function];
        if let Some(local) = local {
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
                output.push(self.create_expression_statement(call)?);
            }
        }
        Ok(output)
    }

    fn transform_execute_statement(
        &mut self,
        statement: TransformNode,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let record = self.context.arena().node(statement)?.clone();
        match record.data {
            NodeData::ImportDeclaration(_) | NodeData::ExportDeclaration(_) => Ok(Vec::new()),
            NodeData::ExportAssignment(data) => {
                if data.is_export_equals == Some(true) {
                    return Err(TransformError::DeferredModuleFormat {
                        format: MODULE_SYSTEM,
                        owner_slice: "H2.2d",
                    });
                }
                let expression = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ExportAssignment,
                        field: "expression",
                    })?;
                let expression = self.visit(expression)?;
                let call = self.create_export_call("default", expression)?;
                let emitted = self.create_expression_statement(call)?;
                self.set_original_and_range(emitted, statement)?;
                Ok(vec![emitted])
            }
            NodeData::VariableStatement(data) => {
                self.transform_top_level_variable_statement(statement, data)
            }
            NodeData::ClassDeclaration(data) => self.transform_top_level_class(statement, data),
            NodeData::Block(data) => Ok(vec![self.transform_execute_block(statement, data)?]),
            NodeData::IfStatement(mut data) => {
                data.expression = data
                    .expression
                    .map(|id| self.visit(id).map(TransformNode::node))
                    .transpose()?;
                data.then_statement = Some(
                    self.transform_execute_embedded(data.then_statement, SyntaxKind::IfStatement)?
                        .node(),
                );
                data.else_statement = data
                    .else_statement
                    .map(|id| {
                        self.transform_execute_embedded(Some(id), SyntaxKind::IfStatement)
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
            NodeData::ExpressionStatement(mut data) => {
                data.expression = data
                    .expression
                    .map(|id| self.visit_expression(id, true).map(TransformNode::node))
                    .transpose()?;
                let flags = self.context.arena().transform_flags(statement);
                Ok(vec![self.context.factory()?.update_node(
                    statement,
                    NodeData::ExpressionStatement(data),
                    flags,
                )?])
            }
            NodeData::FunctionDeclaration(_) => Ok(vec![self.visit(statement.node())?]),
            _ => Ok(vec![self.visit(statement.node())?]),
        }
    }

    fn transform_execute_block(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::BlockData,
    ) -> Result<TransformNode, TransformError> {
        let input = node_array_nodes(self.context.arena(), self.source, data.statements)?;
        let mut output = Vec::new();
        for statement in input {
            output.extend(self.transform_execute_statement(statement)?);
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
    ) -> Result<TransformNode, TransformError> {
        let statement = statement
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .ok_or(TransformError::RequiredChildRemoved {
                parent,
                field: "statement",
            })?;
        if let NodeData::Block(data) = self.context.arena().node(statement)?.data.clone() {
            return self.transform_execute_block(statement, data);
        }
        let mut statements = self.transform_execute_statement(statement)?;
        if statements.len() == 1 {
            return Ok(statements.remove(0));
        }
        self.create_block(statements, true)
    }

    fn transform_top_level_variable_statement(
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
            let (mut expressions, locals) =
                self.flatten_binding_initialization(declaration, variable.name, initializer)?;
            if direct_export {
                for (index, local) in locals.iter().enumerate() {
                    if let Some(exports) = self
                        .info
                        .common
                        .exports_by_local
                        .get(local.as_str())
                        .cloned()
                    {
                        if let Some(expression) = expressions.get_mut(index) {
                            let mut wrapped = *expression;
                            for export in exports {
                                wrapped = self.create_export_call(&export, wrapped)?;
                            }
                            *expression = wrapped;
                        }
                    }
                }
            } else {
                for local in locals {
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
            initialization_expressions.extend(expressions);
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
    ) -> Result<(Vec<TransformNode>, Vec<String>), TransformError> {
        let Some(name) = name.and_then(|id| self.context.arena().node_ref(self.source, id)) else {
            return Ok((Vec::new(), Vec::new()));
        };
        match self.context.arena().node(name)?.data.clone() {
            NodeData::Identifier(data) => {
                let target = self.create_identifier(&data.text)?;
                let assignment = self.create_assignment(target, initializer)?;
                Ok((vec![assignment], vec![data.text]))
            }
            NodeData::ObjectBindingPattern(data) => {
                let key = self.context.arena().get_original_node(declaration).node();
                let temp = self
                    .destructuring_temps
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| self.next_temp_name());
                self.push_hoisted_name(&temp);
                let target = self.create_identifier(&temp)?;
                let mut expressions = vec![self.create_assignment(target, initializer)?];
                let mut locals = vec![temp.clone()];
                for element in node_array_nodes(self.context.arena(), self.source, data.elements)? {
                    let NodeData::BindingElement(element_data) =
                        self.context.arena().node(element)?.data.clone()
                    else {
                        continue;
                    };
                    if element_data.dot_dot_dot_token.is_some()
                        || element_data.initializer.is_some()
                    {
                        return Err(TransformError::UnsupportedCompilerOption {
                            option: "System destructuring",
                            detail: "default and rest binding elements require the later lowering closure",
                        });
                    }
                    let Some(binding_name) = element_data
                        .name
                        .and_then(|id| self.context.arena().node_ref(self.source, id))
                    else {
                        continue;
                    };
                    let local = identifier_or_literal_text(self.context.arena(), binding_name)?;
                    let property = element_data
                        .property_name
                        .and_then(|id| self.context.arena().node_ref(self.source, id))
                        .and_then(|node| {
                            identifier_or_literal_text(self.context.arena(), node).ok()
                        })
                        .unwrap_or_else(|| local.clone());
                    let base = self.create_identifier(&temp)?;
                    let value = if is_identifier_export_name(&property) {
                        self.create_property_access(base, &property)?
                    } else {
                        let property = self.create_string_literal(&property)?;
                        self.create_element_access(base, property)?
                    };
                    let target = self.create_identifier(&local)?;
                    expressions.push(self.create_assignment(target, value)?);
                    locals.push(local);
                }
                Ok((expressions, locals))
            }
            NodeData::ArrayBindingPattern(_) => Err(TransformError::UnsupportedCompilerOption {
                option: "System destructuring",
                detail: "array binding flattening requires the later lowering closure",
            }),
            _ => Ok((Vec::new(), Vec::new())),
        }
    }

    fn transform_top_level_class(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassDeclarationData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let local = data
            .name
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .and_then(|name| identifier_or_literal_text(self.context.arena(), name).ok());
        let exports = local
            .as_deref()
            .and_then(|name| self.info.common.exports_by_local.get(name).cloned())
            .unwrap_or_default();
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
            let call = self.create_export_call("default", class_expression)?;
            output.push(self.create_expression_statement(call)?);
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

        let imported_callee = data
            .expression
            .and_then(|id| self.context.arena().node_ref(self.source, id))
            .map(|callee| self.import_binding_for_reference(callee))
            .transpose()?
            .flatten()
            .is_some();
        let mut node_data = NodeData::CallExpression(data);
        try_visit_each_child(&mut node_data, self)?;
        let NodeData::CallExpression(mut data) = node_data else {
            unreachable!("call expression visitor preserves kind")
        };
        if imported_callee {
            let callee = data
                .expression
                .and_then(|id| self.context.arena().node_ref(self.source, id))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::CallExpression,
                    field: "expression",
                })?;
            let zero = self.create_numeric_literal("0")?;
            let indirect = self.create_binary(zero, SyntaxKind::CommaToken, callee)?;
            let parenthesized = self.create_parenthesized(indirect)?;
            data.expression = Some(parenthesized.node());
        }
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
        let original = self.context.arena().get_original_node(node);
        if original == node
            && NodeFlags::from_bits(self.context.arena().node(node)?.flags)
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
        let resolver_node = self.resolver_node(node)?;
        if self
            .resolver
            .get_referenced_export_container(resolver_node)?
            .is_none()
        {
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
                                local_name = Some(plan.generated_name.to_string());
                                break;
                            }
                        }
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
                            let target = self.create_identifier(&plan.generated_name)?;
                            let value = self.create_identifier(&parameter_name)?;
                            let assignment = self.create_assignment(target, value)?;
                            statements.push(self.create_expression_statement(assignment)?);
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

    fn create_numeric_literal(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
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
        let original = self.context.arena().get_original_node(node);
        let source = self
            .context
            .arena()
            .source(original.source())?
            .program_source()
            .ok_or(TransformError::MissingProgramSource(original))?;
        Ok(EmitResolverNode::new(source, original.node()))
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
