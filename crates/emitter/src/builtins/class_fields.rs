use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    for_each_child, try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId,
    SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeCheckFlags, NodeFlags, ScriptTarget};

use crate::{
    EmitFlags, EmitHint, EmitResolver, InternalEmitFlags, SourceMapRange, SourceRange,
    TransformError, TransformFlags, TransformNode, TransformNodeArray, TransformRoot,
    TransformSourceId, TransformationContext, Transformer,
};

use super::{
    constructor_prologue, initialize_transform_flags,
    is_prologue_statement as is_prologue_statement_node, system::collect_identifier_texts,
    target_bindings::finalize_generated_binding_names,
};

mod downlevel;

/// tsc-port: transformClassFields @6.0.3
/// tsc-hash: 65cacc85f81402ff4468cf65c7636dbd5a0ce9eb6c3248f060aa5193c3af8304
/// tsc-span: _tsc.js:95852-98038
pub(super) fn transform_class_fields<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    Box::new(ClassFieldsTransformer {
        resolver,
        target: options.emit_script_target(),
        use_define_for_class_fields: options.use_define_for_class_fields_effective(),
        class_aliases: BTreeMap::new(),
    })
}

struct ClassFieldsTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    target: ScriptTarget,
    use_define_for_class_fields: bool,
    class_aliases: BTreeMap<(u32, u32), downlevel::ClassBinding>,
}

impl Transformer for ClassFieldsTransformer<'_> {
    fn name(&self) -> &'static str {
        "transformClassFields"
    }

    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        if self.target < ScriptTarget::ES5 || self.target > ScriptTarget::ES_NEXT {
            return Err(TransformError::UnsupportedCompilerOption {
                option: "class-field transform",
                detail: "the closed target band admits ES5 through ESNext class-field reachability",
            });
        }
        context.enable_substitution(SyntaxKind::Identifier)?;
        Ok(())
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        if self.target == ScriptTarget::ES_NEXT && self.use_define_for_class_fields {
            if let TransformRoot::SourceFile(source) = root {
                let transformed = context.arena().root(source)?;
                finalize_generated_binding_names(context, source, transformed)?;
            }
            return Ok(root);
        }
        let TransformRoot::SourceFile(source) = root else {
            return Err(TransformError::Unsupported(
                crate::UnsupportedEmitFeature::BundleRoot,
            ));
        };
        // Earlier transforms synthesize class elements whose local flags are
        // distributed across their completed child tree. Reclassify that tree
        // at this pass boundary so static lexical `this`/`super` ownership is
        // derived from the current arena, just as tsc's factory-propagated
        // transform flags are when transformClassFields begins.
        initialize_transform_flags(context.arena_mut()?, source)?;
        if self.target < ScriptTarget::ES2022 {
            downlevel::transform_source(
                context,
                source,
                self.resolver,
                self.target,
                self.use_define_for_class_fields,
                self.target == ScriptTarget::ES2021,
                &mut self.class_aliases,
            )?;
            return Ok(TransformRoot::SourceFile(source));
        }
        let root = context.arena().root(source)?;
        let mut visitor = ClassFieldsVisitor::new(
            context,
            source,
            self.target,
            self.use_define_for_class_fields,
        );
        let transformed =
            visitor
                .visit(root.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "root",
                })?;
        let transformed = visitor.prepend_hoisted_declarations(visitor.node(transformed))?;
        finalize_generated_binding_names(visitor.context, source, transformed)?;
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
    }

    fn substitute_node(
        &mut self,
        context: &mut TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if self.class_aliases.is_empty()
            || hint != EmitHint::Expression
            || !matches!(context.arena().node(node)?.data, NodeData::Identifier(_))
        {
            return Ok(node);
        }
        let generated_owner = context
            .arena()
            .metadata(node)
            .and_then(|metadata| metadata.class_constructor_reference);
        let alias_key = if let Some(owner) = generated_owner {
            let owner = context.arena().require_parse_tree_resolver_node(owner)?;
            (owner.source().raw(), owner.node().0)
        } else {
            let Some(resolver_node) = context.arena().parse_tree_resolver_node(node)? else {
                return Ok(node);
            };
            if !self.resolver.has_node_check_flag(
                resolver_node,
                NodeCheckFlags::CONSTRUCTOR_REFERENCE.bits() as u32,
            )? {
                return Ok(node);
            }
            let Some(declaration) = self
                .resolver
                .get_referenced_value_declaration(resolver_node)?
            else {
                return Ok(node);
            };
            (declaration.source().raw(), declaration.node().0)
        };
        let Some(alias) = self.class_aliases.get(&alias_key).cloned() else {
            return Ok(node);
        };
        let alias_text = alias.printable_text(context).to_owned();
        let replacement = {
            let mut factory = context.substitution_factory()?;
            let replacement = factory.create_node(
                node.source(),
                NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                    escaped_text: tsc_syntax::escape_leading_underscores(&alias_text),
                    text: alias_text,
                }),
                TransformFlags::NONE,
            )?;
            factory.set_text_range(replacement, node)?;
            replacement
        };
        context
            .arena_mut()?
            .set_original_node(replacement, Some(node))?;
        alias.write_generated_metadata(context.arena_mut()?, replacement);
        context
            .arena_mut()?
            .metadata_mut(replacement)
            .add_flags(EmitFlags::NO_SUBSTITUTION);
        Ok(replacement)
    }

    fn dispose(&mut self) {
        self.class_aliases.clear();
    }
}

struct ClassFieldsVisitor<'context> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
    used_names: BTreeSet<String>,
    hoisted_names: Vec<String>,
    next_temp_name: usize,
    private_name_scopes: Vec<BTreeSet<String>>,
    target: ScriptTarget,
    use_define_for_class_fields: bool,
}

enum MovedInstanceInitializerPlan {
    Statement(TransformNode),
    Field(MovedFieldInitializerPlan),
}

struct MovedFieldInitializerPlan {
    original: TransformNode,
    name: Option<NodeId>,
    value: MovedFieldValuePlan,
}

enum MovedFieldValuePlan {
    Declared(NodeId),
    ParameterProperty {
        prefix: Option<NodeId>,
        local: ParameterPropertyLocal,
    },
}

struct ParameterPropertyLocal {
    emitted_name: TransformNode,
    source_name: TransformNode,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ParameterPropertyAssignmentPolicy {
    Preserve,
    Replace,
}

struct MovedInstanceInitializers {
    statements: Vec<TransformNode>,
    parameter_assignments: ParameterPropertyAssignmentPolicy,
}

#[derive(Debug)]
struct SuperStatementPath(Vec<usize>);

impl<'context> ClassFieldsVisitor<'context> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        target: ScriptTarget,
        use_define_for_class_fields: bool,
    ) -> Self {
        let used_names = collect_identifier_texts(context.arena(), source);
        Self {
            context,
            source,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
            used_names,
            hoisted_names: Vec::new(),
            next_temp_name: 0,
            private_name_scopes: Vec::new(),
            target,
            use_define_for_class_fields,
        }
    }

    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if let Some(mapped) = self.nodes.get(&id) {
            return Ok(*mapped);
        }
        let original = self.node(id);
        let record = self.context.arena().node(original)?.clone();
        let transformed = match record.data {
            NodeData::ClassDeclaration(data) => Some(self.visit_class_declaration(original, data)?),
            NodeData::ClassExpression(data) => Some(self.visit_class_expression(original, data)?),
            NodeData::Token => Some(id),
            data => Some(self.update_generic(original, data)?),
        };
        self.nodes.insert(id, transformed);
        Ok(transformed)
    }

    fn visit_class_declaration(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ClassDeclarationData,
    ) -> Result<NodeId, TransformError> {
        let private_names = self.declared_private_names(data.members)?;
        self.private_name_scopes.push(private_names);
        let result = self.visit_class_declaration_in_scope(original, data);
        self.private_name_scopes
            .pop()
            .expect("class private-name scope remains balanced");
        result
    }

    fn visit_class_declaration_in_scope(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassDeclarationData,
    ) -> Result<NodeId, TransformError> {
        if !self.class_members_require_transform(data.members)? {
            return self.update_generic(original, NodeData::ClassDeclaration(data));
        }
        let (members, prologue) = self.rewrite_computed_names_with_lexical_this(data.members)?;
        if prologue.is_some() {
            return Err(TransformError::UnsupportedSyntax {
                feature: crate::UnsupportedTransformFeature::Decorators,
                node: original,
            });
        }
        data.members = members;
        let class_receiver = data
            .name
            .and_then(|name| self.identifier_text(self.node(name)).map(str::to_owned));
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.heritage_clauses = self.visit_optional_nodes(data.heritage_clauses)?;
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        let derived = self.has_extends_clause(data.heritage_clauses)?;
        data.members = self.transform_members(data.members, derived, class_receiver.as_deref())?;
        let flags = super::flags_after_update(
            self.context.arena(),
            original,
            &NodeData::ClassDeclaration(data.clone()),
        )?;
        Ok(self
            .context
            .factory()?
            .update_node(original, NodeData::ClassDeclaration(data), flags)?
            .node())
    }

    fn visit_class_expression(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ClassExpressionData,
    ) -> Result<NodeId, TransformError> {
        let private_names = self.declared_private_names(data.members)?;
        self.private_name_scopes.push(private_names);
        let result = self.visit_class_expression_in_scope(original, data);
        self.private_name_scopes
            .pop()
            .expect("class private-name scope remains balanced");
        result
    }

    fn visit_class_expression_in_scope(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassExpressionData,
    ) -> Result<NodeId, TransformError> {
        if !self.class_members_require_transform(data.members)? {
            return self.update_generic(original, NodeData::ClassExpression(data));
        }
        let (members, prologue) = self.rewrite_computed_names_with_lexical_this(data.members)?;
        data.members = members;
        let class_receiver = data
            .name
            .and_then(|name| self.identifier_text(self.node(name)).map(str::to_owned));
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.heritage_clauses = self.visit_optional_nodes(data.heritage_clauses)?;
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        let derived = self.has_extends_clause(data.heritage_clauses)?;
        data.members = self.transform_members(data.members, derived, class_receiver.as_deref())?;
        let flags = super::flags_after_update(
            self.context.arena(),
            original,
            &NodeData::ClassExpression(data.clone()),
        )?;
        let class = self
            .context
            .factory()?
            .update_node(original, NodeData::ClassExpression(data), flags)?
            .node();
        self.wrap_class_expression_prologue(self.node(class), prologue)
    }

    fn rewrite_computed_names_with_lexical_this(
        &mut self,
        members: Option<NodeArrayId>,
    ) -> Result<(Option<NodeArrayId>, Option<TransformNode>), TransformError> {
        let Some(members_id) = members else {
            return Ok((None, None));
        };
        let original_array = self.array(members_id);
        let original_members = self
            .context
            .arena()
            .node_array(original_array)?
            .nodes
            .clone();
        let mut output = Vec::with_capacity(original_members.len() + 1);
        let mut pending_expressions = Vec::new();
        let mut captures_this = false;
        for member in original_members {
            let member_node = self.node(member);
            let NodeData::PropertyDeclaration(mut data) =
                self.context.arena().node(member_node)?.data.clone()
            else {
                output.push(member_node);
                continue;
            };
            let Some(name) = data.name else {
                output.push(member_node);
                continue;
            };
            if self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)? {
                // Auto-accessors own a paired getter/setter name plan. A
                // non-inlineable computed key is evaluated by the getter's
                // name and reused by the setter, not moved to a class
                // prologue with ordinary computed fields.
                output.push(member_node);
                continue;
            }
            let name_node = self.node(name);
            let NodeData::ComputedPropertyName(computed) =
                self.context.arena().node(name_node)?.data.clone()
            else {
                output.push(member_node);
                continue;
            };
            if self
                .context
                .arena()
                .metadata(name_node)
                .is_some_and(|metadata| {
                    metadata
                        .internal_flags()
                        .contains(InternalEmitFlags::GENERATED_COMPUTED_PROPERTY_NAME)
                })
            {
                output.push(member_node);
                continue;
            }
            let Some(expression) = computed.expression else {
                output.push(member_node);
                continue;
            };
            let contains_this = self.subtree_contains_this(self.node(expression))?;
            let is_static = self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)?;
            let simple = self.is_simple_inlineable_expression(self.node(expression))?;
            if data.initializer.is_none() {
                if !simple && !self.is_identifier_expression(self.node(expression))? {
                    captures_this |= contains_this;
                    let expression = self
                        .visit(expression)?
                        .map(|expression| self.node(expression))
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ComputedPropertyName,
                            field: "expression",
                        })?;
                    pending_expressions.push(expression);
                }
                output.push(member_node);
                continue;
            }
            if is_static && !contains_this || simple {
                output.push(member_node);
                continue;
            }
            captures_this |= contains_this;
            let expression = self
                .visit(expression)?
                .map(|expression| self.node(expression))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ComputedPropertyName,
                    field: "expression",
                })?;
            let temporary_name = self.allocate_temp_name();
            let temporary = self.create_identifier(&temporary_name)?;
            pending_expressions.push(self.create_assignment(temporary, expression)?);
            let cached = self.create_identifier(&temporary_name)?;
            let computed = self.context.factory()?.create_node(
                self.source,
                NodeData::ComputedPropertyName(tsc_syntax::nodes::ComputedPropertyNameData {
                    expression: Some(cached.node()),
                }),
                TransformFlags::CONTAINS_COMPUTED_PROPERTY_NAME,
            )?;
            data.name = Some(computed.node());
            let flags = super::flags_after_update(
                self.context.arena(),
                member_node,
                &NodeData::PropertyDeclaration(data.clone()),
            )?;
            output.push(self.context.factory()?.update_node(
                member_node,
                NodeData::PropertyDeclaration(data),
                flags,
            )?);
        }
        if pending_expressions.is_empty() {
            return Ok((members, None));
        }

        let assignments = self.inline_expressions(pending_expressions)?;
        if !captures_this {
            let statement = self.create_expression_statement(assignments)?;
            let body = self.create_block(vec![statement], false)?;
            let static_block = self.context.factory()?.create_node(
                self.source,
                NodeData::ClassStaticBlockDeclaration(
                    tsc_syntax::nodes::ClassStaticBlockDeclarationData {
                        body: Some(body.node()),
                        modifiers: None,
                    },
                ),
                TransformFlags::NONE,
            )?;
            output.insert(0, static_block);
            let updated = self
                .context
                .factory()?
                .update_node_array(original_array, output)?;
            return Ok((Some(updated.array()), None));
        }

        let initializer_name = self.allocate_temp_name();
        let assignment_statement = self.create_expression_statement(assignments)?;
        let arrow_body = self.create_block(vec![assignment_statement], false)?;
        let arrow = self.create_arrow_function(Vec::new(), arrow_body)?;
        let initializer = self.create_identifier(&initializer_name)?;
        let prologue = self.create_assignment(initializer, arrow)?;

        let initializer = self.create_identifier(&initializer_name)?;
        let call = self.create_call(initializer, Vec::new())?;
        let statement = self.create_expression_statement(call)?;
        let body = self.create_block(vec![statement], false)?;
        let static_block = self.context.factory()?.create_node(
            self.source,
            NodeData::ClassStaticBlockDeclaration(
                tsc_syntax::nodes::ClassStaticBlockDeclarationData {
                    body: Some(body.node()),
                    modifiers: None,
                },
            ),
            TransformFlags::NONE,
        )?;
        output.insert(0, static_block);
        let updated = self
            .context
            .factory()?
            .update_node_array(original_array, output)?;
        Ok((Some(updated.array()), Some(prologue)))
    }

    fn wrap_class_expression_prologue(
        &mut self,
        class: TransformNode,
        prologue: Option<TransformNode>,
    ) -> Result<NodeId, TransformError> {
        let Some(prologue) = prologue else {
            return Ok(class.node());
        };
        self.context
            .arena_mut()?
            .metadata_mut(class)
            .add_flags(EmitFlags::INDENTED);
        self.context
            .arena_mut()?
            .metadata_mut(class)
            .set_starts_on_new_line(true);
        let comma = self.create_binary(prologue, SyntaxKind::CommaToken, class)?;
        Ok(self.create_parenthesized(comma)?.node())
    }

    fn inline_expressions(
        &mut self,
        expressions: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut expressions = expressions.into_iter();
        let mut expression = expressions
            .next()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassExpression,
                field: "computed-name expressions",
            })?;
        for next in expressions {
            expression = self.create_binary(expression, SyntaxKind::CommaToken, next)?;
        }
        Ok(expression)
    }

    fn subtree_contains_this(&self, root: TransformNode) -> Result<bool, TransformError> {
        let mut stack = vec![root.node()];
        while let Some(id) = stack.pop() {
            let node = self.node(id);
            let record = self.context.arena().node(node)?;
            if record.kind == SyntaxKind::ThisKeyword {
                return Ok(true);
            }
            for_each_child(
                &self.context.arena().source(self.source)?.syntax().arena,
                record,
                |child| {
                    stack.push(child);
                    false
                },
            );
        }
        Ok(false)
    }

    fn is_simple_inlineable_expression(
        &self,
        expression: TransformNode,
    ) -> Result<bool, TransformError> {
        let record = self.context.arena().node(expression)?;
        Ok(matches!(
            record.kind,
            SyntaxKind::StringLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::NumericLiteral
                | SyntaxKind::BigIntLiteral
                | SyntaxKind::TrueKeyword
                | SyntaxKind::FalseKeyword
                | SyntaxKind::NullKeyword
                | SyntaxKind::ThisKeyword
        ))
    }

    fn is_identifier_expression(&self, expression: TransformNode) -> Result<bool, TransformError> {
        Ok(self.context.arena().node(expression)?.kind == SyntaxKind::Identifier)
    }

    fn declared_private_names(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<BTreeSet<String>, TransformError> {
        let mut names = BTreeSet::new();
        for member in self.array_nodes(members)? {
            let name = match &self.context.arena().node(member)?.data {
                NodeData::PropertyDeclaration(data) => data.name,
                NodeData::MethodDeclaration(data) => data.name,
                NodeData::GetAccessor(data) => data.name,
                NodeData::SetAccessor(data) => data.name,
                _ => None,
            };
            let Some(name) = name else {
                continue;
            };
            if let NodeData::PrivateIdentifier(data) =
                &self.context.arena().node(self.node(name))?.data
            {
                names.insert(data.text.clone());
            }
        }
        Ok(names)
    }

    fn allocate_private_storage_name(&mut self, base: &str) -> String {
        let mut ordinal = 0usize;
        loop {
            let candidate = if ordinal == 0 {
                format!("#{base}_accessor_storage")
            } else {
                format!("#{base}_{ordinal}_accessor_storage")
            };
            let visible = self
                .private_name_scopes
                .iter()
                .any(|scope| scope.contains(&candidate));
            if !visible {
                self.private_name_scopes
                    .last_mut()
                    .expect("auto-accessor belongs to a class private-name scope")
                    .insert(candidate.clone());
                return candidate;
            }
            ordinal += 1;
        }
    }

    fn allocate_anonymous_private_storage_name(&mut self) -> String {
        let mut ordinal = 0usize;
        loop {
            let stem = if ordinal < 26 {
                format!("_{}", char::from(b'a' + ordinal as u8))
            } else {
                format!("_{}", ordinal - 26)
            };
            let candidate = format!("#{stem}_accessor_storage");
            let visible = self
                .private_name_scopes
                .iter()
                .any(|scope| scope.contains(&candidate));
            if !visible {
                self.private_name_scopes
                    .last_mut()
                    .expect("auto-accessor belongs to a class private-name scope")
                    .insert(candidate.clone());
                return candidate;
            }
            ordinal += 1;
        }
    }

    fn auto_accessor_names(&mut self, name: NodeId) -> Result<(NodeId, NodeId), TransformError> {
        let name_node = self.node(name);
        let NodeData::ComputedPropertyName(data) =
            self.context.arena().node(name_node)?.data.clone()
        else {
            return Ok((name, name));
        };
        let expression = data
            .expression
            .and_then(|expression| self.context.arena().node_ref(self.source, expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ComputedPropertyName,
                field: "auto-accessor name expression",
            })?;
        if self.is_simple_inlineable_expression(expression)? {
            return Ok((name, name));
        }

        let temporary_name = self.allocate_temp_name();
        let temporary = self.create_identifier(&temporary_name)?;
        let assignment = self.create_assignment(temporary, expression)?;
        let getter_name = self.context.factory()?.create_node(
            self.source,
            NodeData::ComputedPropertyName(tsc_syntax::nodes::ComputedPropertyNameData {
                expression: Some(assignment.node()),
            }),
            TransformFlags::CONTAINS_COMPUTED_PROPERTY_NAME,
        )?;
        self.set_original_and_range(getter_name, name_node)?;

        let temporary = self.create_identifier(&temporary_name)?;
        let setter_name = self.context.factory()?.create_node(
            self.source,
            NodeData::ComputedPropertyName(tsc_syntax::nodes::ComputedPropertyNameData {
                expression: Some(temporary.node()),
            }),
            TransformFlags::CONTAINS_COMPUTED_PROPERTY_NAME,
        )?;
        self.set_original_and_range(setter_name, name_node)?;
        Ok((getter_name.node(), setter_name.node()))
    }

    fn transform_members(
        &mut self,
        members: Option<NodeArrayId>,
        derived: bool,
        class_receiver: Option<&str>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(members_id) = members else {
            return Ok(None);
        };
        let original_array = self.array(members_id);
        let original_members = self
            .context
            .arena()
            .node_array(original_array)?
            .nodes
            .clone();
        let mut move_instance_initializers = if self.use_define_for_class_fields {
            false
        } else {
            original_members.iter().try_fold(
                false,
                |found, member| -> Result<bool, TransformError> {
                    if found {
                        return Ok(true);
                    }
                    let NodeData::PropertyDeclaration(data) =
                        &self.context.arena().node(self.node(*member))?.data
                    else {
                        return Ok(false);
                    };
                    Ok(data.initializer.is_some()
                        && !self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)?
                        && !self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?
                        && !self.name_is_private(data.name)?)
                },
            )?
        };
        if self.target < ScriptTarget::ES_NEXT && !self.use_define_for_class_fields {
            move_instance_initializers |= original_members.iter().try_fold(
                false,
                |found, member| -> Result<bool, TransformError> {
                    if found {
                        return Ok(true);
                    }
                    let NodeData::PropertyDeclaration(data) =
                        &self.context.arena().node(self.node(*member))?.data
                    else {
                        return Ok(false);
                    };
                    Ok(
                        self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?
                            && !self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)?,
                    )
                },
            )?;
        }

        let mut output = Vec::with_capacity(original_members.len() + 3);
        let mut instance_initializer_plans = Vec::new();
        let mut constructor_index = None;
        for member in original_members {
            let member_node = self.node(member);
            let record = self.context.arena().node(member_node)?.clone();
            match record.data {
                NodeData::PropertyDeclaration(data) => {
                    let mut node_data = NodeData::PropertyDeclaration(data);
                    try_visit_each_child(&mut node_data, self)?;
                    let NodeData::PropertyDeclaration(mut data) = node_data else {
                        unreachable!("property wrapper remains a property")
                    };
                    let static_ = self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)?;
                    let accessor =
                        self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?;
                    let private = self.name_is_private(data.name)?;
                    if accessor {
                        if self.target < ScriptTarget::ES_NEXT
                            && self.target >= ScriptTarget::ES2022
                        {
                            output.extend(self.transform_native_auto_accessor(
                                member_node,
                                data,
                                class_receiver,
                            )?);
                        } else if move_instance_initializers && !static_ {
                            let transformed = self.transform_auto_accessor(member_node, data)?;
                            instance_initializer_plans.push(
                                MovedInstanceInitializerPlan::Statement(transformed.initializer),
                            );
                            output.extend(transformed.members);
                        } else {
                            output.push(self.update_property(member_node, data)?);
                        }
                    } else if private {
                        if move_instance_initializers && !static_ && data.initializer.is_some() {
                            let initializer = data.initializer.take().expect("initializer checked");
                            instance_initializer_plans.push(
                                MovedInstanceInitializerPlan::Statement(
                                    self.create_property_initializer_statement(
                                        member_node,
                                        data.name,
                                        initializer,
                                    )?,
                                ),
                            );
                        }
                        output.push(self.update_property(member_node, data)?);
                    } else if self.use_define_for_class_fields
                        && self.target >= ScriptTarget::ES2022
                    {
                        output.push(self.update_property(member_node, data)?);
                    } else if static_ {
                        if let Some(initializer) = data.initializer {
                            output.push(self.create_static_initializer_block(
                                member_node,
                                data.name,
                                initializer,
                            )?);
                        }
                    } else if let Some(local) = move_instance_initializers
                        .then(|| self.parameter_property_local(member_node, data.name))
                        .transpose()?
                        .flatten()
                    {
                        instance_initializer_plans.push(MovedInstanceInitializerPlan::Field(
                            MovedFieldInitializerPlan {
                                original: member_node,
                                name: data.name,
                                value: MovedFieldValuePlan::ParameterProperty {
                                    prefix: data.initializer,
                                    local,
                                },
                            },
                        ));
                    } else if let Some(initializer) = data.initializer {
                        instance_initializer_plans.push(MovedInstanceInitializerPlan::Field(
                            MovedFieldInitializerPlan {
                                original: member_node,
                                name: data.name,
                                value: MovedFieldValuePlan::Declared(initializer),
                            },
                        ));
                    }
                }
                NodeData::Constructor(data) => {
                    let constructor =
                        self.update_generic(member_node, NodeData::Constructor(data))?;
                    constructor_index = Some(output.len());
                    output.push(self.node(constructor));
                }
                data => {
                    let updated = self.update_generic(member_node, data)?;
                    output.push(self.node(updated));
                }
            }
        }

        let instance_initializers =
            self.materialize_moved_instance_initializers(instance_initializer_plans)?;
        if !instance_initializers.statements.is_empty() {
            if let Some(index) = constructor_index {
                output[index] = self
                    .inject_initializers_into_constructor(output[index], &instance_initializers)?;
            } else {
                output.insert(
                    0,
                    self.create_synthetic_constructor(derived, instance_initializers.statements)?,
                );
            }
        }
        let updated = self
            .context
            .factory()?
            .update_node_array(original_array, output)?;
        Ok(Some(updated.array()))
    }

    /// tsc-port: transformPropertyWorker @6.0.3
    /// tsc-hash: fb5e7b8fdfc4fab54f8fdd4ea6f48902c80207af52647e23cb47491f0ce46edd
    /// tsc-span: _tsc.js:97501-97575
    fn materialize_moved_instance_initializers(
        &mut self,
        plans: Vec<MovedInstanceInitializerPlan>,
    ) -> Result<MovedInstanceInitializers, TransformError> {
        let mut statements = Vec::with_capacity(plans.len());
        let mut parameter_assignments = ParameterPropertyAssignmentPolicy::Preserve;
        for plan in plans {
            match plan {
                MovedInstanceInitializerPlan::Statement(statement) => statements.push(statement),
                MovedInstanceInitializerPlan::Field(plan) => {
                    let initializer = match plan.value {
                        MovedFieldValuePlan::Declared(initializer) => self.node(initializer),
                        MovedFieldValuePlan::ParameterProperty { prefix, local } => {
                            parameter_assignments = ParameterPropertyAssignmentPolicy::Replace;
                            let local_name =
                                self.context.factory()?.clone_node(local.emitted_name)?;
                            self.context
                                .factory()?
                                .set_text_range(local_name, local.source_name)?;
                            self.context
                                .arena_mut()?
                                .metadata_mut(local_name)
                                .add_flags(EmitFlags::NO_COMMENTS);
                            if let Some(prefix) = prefix {
                                self.create_binary(
                                    self.node(prefix),
                                    SyntaxKind::CommaToken,
                                    local_name,
                                )?
                            } else {
                                local_name
                            }
                        }
                    };
                    statements.push(self.create_property_initializer_statement(
                        plan.original,
                        plan.name,
                        initializer.node(),
                    )?);
                }
            }
        }
        Ok(MovedInstanceInitializers {
            statements,
            parameter_assignments,
        })
    }

    fn class_members_require_transform(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for member in self.array_nodes(members)? {
            let NodeData::PropertyDeclaration(data) = &self.context.arena().node(member)?.data
            else {
                if self.target < ScriptTarget::ES2022
                    && self.context.arena().node(member)?.kind
                        == SyntaxKind::ClassStaticBlockDeclaration
                {
                    return Ok(true);
                }
                continue;
            };
            if self.target < ScriptTarget::ES2022 {
                return Ok(true);
            }
            if self.target < ScriptTarget::ES_NEXT
                && self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?
            {
                return Ok(true);
            }
            if !self.use_define_for_class_fields
                && !self.name_is_private(data.name)?
                && !self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn transform_auto_accessor(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::PropertyDeclarationData,
    ) -> Result<TransformedAccessor, TransformError> {
        let name = data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::PropertyDeclaration,
            field: "name",
        })?;
        let storage_base = match &self.context.arena().node(self.node(name))?.data {
            NodeData::Identifier(data) => Some(data.text.trim_start_matches('#').to_owned()),
            NodeData::PrivateIdentifier(data) => Some(data.text.trim_start_matches('#').to_owned()),
            _ => None,
        };
        let storage = match storage_base {
            Some(base) => self.allocate_private_storage_name(&base),
            None => self.allocate_anonymous_private_storage_name(),
        };
        let storage_name = self.create_private_identifier(&storage)?;
        let backing = self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyDeclaration(tsc_syntax::nodes::PropertyDeclarationData {
                name: Some(storage_name.node()),
                modifiers: None,
                question_token: None,
                exclamation_token: None,
                r#type: None,
                initializer: None,
            }),
            TransformFlags::CONTAINS_CLASS_FIELDS,
        )?;
        self.set_original_and_range(backing, original)?;

        let modifiers = self.filter_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?;
        let (getter_name, setter_name) = self.auto_accessor_names(name)?;
        let getter = self.create_get_accessor(getter_name, storage_name.node(), modifiers, None)?;
        let setter = self.create_set_accessor(setter_name, storage_name.node(), modifiers, None)?;
        self.set_original_and_range(getter, original)?;
        let initializer = data
            .initializer
            .unwrap_or(self.create_identifier("undefined")?.node());
        let statement = self.create_property_initializer_statement(
            original,
            Some(storage_name.node()),
            initializer,
        )?;
        self.context
            .arena_mut()?
            .metadata_mut(backing)
            .add_flags(EmitFlags::NO_COMMENTS);
        self.context
            .arena_mut()?
            .metadata_mut(setter)
            .add_flags(EmitFlags::NO_COMMENTS);
        Ok(TransformedAccessor {
            members: vec![backing, getter, setter],
            initializer: statement,
        })
    }

    fn transform_native_auto_accessor(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::PropertyDeclarationData,
        class_receiver: Option<&str>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let name = data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::PropertyDeclaration,
            field: "name",
        })?;
        let storage_base = match &self.context.arena().node(self.node(name))?.data {
            NodeData::Identifier(data) => Some(data.text.trim_start_matches('#').to_owned()),
            NodeData::PrivateIdentifier(data) => Some(data.text.trim_start_matches('#').to_owned()),
            _ => None,
        };
        let storage = match storage_base {
            Some(base) => self.allocate_private_storage_name(&base),
            None => self.allocate_anonymous_private_storage_name(),
        };
        let storage_name = self.create_private_identifier(&storage)?;
        let modifiers = self.filter_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?;
        let backing = self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyDeclaration(tsc_syntax::nodes::PropertyDeclarationData {
                name: Some(storage_name.node()),
                modifiers,
                question_token: None,
                exclamation_token: None,
                r#type: None,
                initializer: data.initializer,
            }),
            TransformFlags::CONTAINS_CLASS_FIELDS,
        )?;
        self.set_original_and_range(backing, original)?;
        let static_ = self.has_modifier(modifiers, SyntaxKind::StaticKeyword)?;
        let receiver = static_.then_some(class_receiver).flatten();
        let (getter_name, setter_name) = self.auto_accessor_names(name)?;
        let getter =
            self.create_get_accessor(getter_name, storage_name.node(), modifiers, receiver)?;
        let setter =
            self.create_set_accessor(setter_name, storage_name.node(), modifiers, receiver)?;
        self.set_original_and_range(getter, original)?;
        self.context
            .arena_mut()?
            .metadata_mut(backing)
            .add_flags(EmitFlags::NO_COMMENTS);
        self.context
            .arena_mut()?
            .metadata_mut(setter)
            .add_flags(EmitFlags::NO_COMMENTS);
        Ok(vec![backing, getter, setter])
    }

    fn create_get_accessor(
        &mut self,
        name: NodeId,
        storage: NodeId,
        modifiers: Option<NodeArrayId>,
        receiver: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let access = self.create_receiver_access(Some(storage), receiver)?;
        let return_statement = self.context.factory()?.create_node(
            self.source,
            NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                expression: Some(access.node()),
            }),
            TransformFlags::NONE,
        )?;
        let body = self.create_block(vec![return_statement], false)?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, Vec::new())?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::GetAccessor(tsc_syntax::nodes::GetAccessorData {
                name: Some(name),
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_set_accessor(
        &mut self,
        name: NodeId,
        storage: NodeId,
        modifiers: Option<NodeArrayId>,
        receiver: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let value = self.create_identifier("value")?;
        let parameter = self.context.factory()?.create_node(
            self.source,
            NodeData::Parameter(tsc_syntax::nodes::ParameterData {
                name: Some(value.node()),
                modifiers: None,
                dot_dot_dot_token: None,
                question_token: None,
                r#type: None,
                initializer: None,
            }),
            TransformFlags::NONE,
        )?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, vec![parameter])?;
        let access = self.create_receiver_access(Some(storage), receiver)?;
        let assignment = self.create_assignment(access, value)?;
        let statement = self.create_expression_statement(assignment)?;
        let body = self.create_block(vec![statement], false)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::SetAccessor(tsc_syntax::nodes::SetAccessorData {
                name: Some(name),
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers,
            }),
            TransformFlags::NONE,
        )
    }

    /// tsc-port: transformPropertyOrClassStaticBlock @6.0.3
    /// tsc-hash: b86b07fb81b4ec313a647283e7ecf39e8071848b80454d149aad9c3237d123f2
    /// tsc-span: _tsc.js:97444-97465
    fn create_property_initializer_statement(
        &mut self,
        original: TransformNode,
        name: Option<NodeId>,
        initializer: NodeId,
    ) -> Result<TransformNode, TransformError> {
        let target = self.create_this_access(name)?;
        // The relocated statement owns the property's leading comments. The
        // parsed name remains a child of this synthetic access so its spelling
        // and source-map range survive, but it must not re-emit the same
        // comment between `this.` and the name. This is tsc's
        // NoLeadingComments boundary on createMemberAccessForPropertyName.
        self.context
            .arena_mut()?
            .metadata_mut(target)
            .add_flags(EmitFlags::NO_LEADING_COMMENTS);
        let assignment = self.create_assignment(target, self.node(initializer))?;
        let statement = self.create_expression_statement(assignment)?;
        self.set_original_and_range(statement, original)?;
        let property_original = self.context.arena().get_original_node(original);
        if self.context.arena().node(property_original)?.kind == SyntaxKind::Parameter {
            let source_map_range = {
                let arena = self.context.arena();
                let record = arena.node(property_original)?;
                let source = arena.source(property_original.source())?.syntax();
                SourceRange::from_raw(record.pos, record.end, source.positions())
                    .map(|range| SourceMapRange::new(property_original.source(), range))
                    .map_err(|error| TransformError::InvalidSourceRange {
                        node: property_original,
                        error,
                    })?
            };
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .set_source_map_range(source_map_range);
        } else {
            let source_map_range = {
                let arena = self.context.arena();
                let record = arena.node(original)?;
                let modifiers = match &record.data {
                    NodeData::PropertyDeclaration(data) => data.modifiers,
                    _ => None,
                };
                let modifier_end = modifiers
                    .and_then(|modifiers| arena.node_array_ref(self.source, modifiers))
                    .and_then(|modifiers| arena.node_array(modifiers).ok())
                    .and_then(|modifiers| modifiers.nodes.last())
                    .and_then(|modifier| arena.node_ref(self.source, *modifier))
                    .and_then(|modifier| arena.node(modifier).ok())
                    .map(|modifier| modifier.end)
                    .filter(|end| *end != u32::MAX);
                let source = arena.source(original.source())?.syntax();
                SourceRange::from_raw(
                    modifier_end.unwrap_or(record.pos),
                    record.end,
                    source.positions(),
                )
                .map(|range| SourceMapRange::new(original.source(), range))
                .map_err(|error| TransformError::InvalidSourceRange {
                    node: original,
                    error,
                })?
            };
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .set_source_map_range(source_map_range);
        }
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .set_starts_on_new_line(true);
        Ok(statement)
    }

    fn create_static_initializer_block(
        &mut self,
        original: TransformNode,
        name: Option<NodeId>,
        initializer: NodeId,
    ) -> Result<TransformNode, TransformError> {
        let statement = self.create_property_initializer_statement(original, name, initializer)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::NO_COMMENTS);
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
        self.set_original_and_range(block, original)
    }

    fn create_this_access(
        &mut self,
        name: Option<NodeId>,
    ) -> Result<TransformNode, TransformError> {
        self.create_receiver_access(name, None)
    }

    fn create_receiver_access(
        &mut self,
        name: Option<NodeId>,
        receiver_name: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let name = name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::PropertyDeclaration,
            field: "name",
        })?;
        let receiver = match receiver_name {
            Some(receiver) => self.create_identifier(receiver)?,
            None => self.context.factory()?.create_token(
                self.source,
                SyntaxKind::ThisKeyword,
                TransformFlags::CONTAINS_LEXICAL_THIS,
            )?,
        };
        let name_node = self.node(name);
        let (access, no_nested_source_maps) =
            match self.context.arena().node(name_node)?.data.clone() {
                NodeData::Identifier(_) | NodeData::PrivateIdentifier(_) => (
                    self.context.factory()?.create_node(
                        self.source,
                        NodeData::PropertyAccessExpression(
                            tsc_syntax::nodes::PropertyAccessExpressionData {
                                expression: Some(receiver.node()),
                                question_dot_token: None,
                                name: Some(name),
                            },
                        ),
                        TransformFlags::CONTAINS_LEXICAL_THIS,
                    )?,
                    true,
                ),
                NodeData::ComputedPropertyName(data) => (
                    self.context.factory()?.create_node(
                        self.source,
                        NodeData::ElementAccessExpression(
                            tsc_syntax::nodes::ElementAccessExpressionData {
                                expression: Some(receiver.node()),
                                question_dot_token: None,
                                argument_expression: data.expression,
                            },
                        ),
                        TransformFlags::CONTAINS_LEXICAL_THIS,
                    )?,
                    false,
                ),
                _ => (
                    self.context.factory()?.create_node(
                        self.source,
                        NodeData::ElementAccessExpression(
                            tsc_syntax::nodes::ElementAccessExpressionData {
                                expression: Some(receiver.node()),
                                question_dot_token: None,
                                argument_expression: Some(name),
                            },
                        ),
                        TransformFlags::CONTAINS_LEXICAL_THIS,
                    )?,
                    true,
                ),
            };
        // createMemberAccessForPropertyName positions the generated access at
        // the source member name. Besides preserving its outer source-map
        // span, that range is the comment-container boundary established by
        // NoLeadingComments when this access is relocated into a constructor.
        self.context.factory()?.set_text_range(access, name_node)?;
        if no_nested_source_maps {
            self.context
                .arena_mut()?
                .metadata_mut(access)
                .add_flags(EmitFlags::NO_NESTED_SOURCE_MAPS);
        }
        Ok(access)
    }

    /// tsc-port: transformConstructorBody @6.0.3
    /// tsc-hash: 6ab03601cab55c7af832a1cec8e17a822e21aa330f32a65b2b79637c4765c9f3
    /// tsc-span: _tsc.js:97329-97431
    fn inject_initializers_into_constructor(
        &mut self,
        constructor: TransformNode,
        initializers: &MovedInstanceInitializers,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::Constructor(mut data) = self.context.arena().node(constructor)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassDeclaration,
                field: "constructor",
            });
        };
        let body = data
            .body
            .and_then(|body| self.context.arena().node_ref(self.source, body))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Constructor,
                field: "body",
            })?;
        let NodeData::Block(mut block) = self.context.arena().node(body)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Constructor,
                field: "body block",
            });
        };
        let mut statements = self.array_nodes(block.statements)?;
        let insertion = constructor_prologue(self.context.arena(), &statements)?.body_start();
        if let Some(path) = self.find_super_statement_path(&statements, insertion)? {
            self.inject_initializers_at_super_path(&mut statements, &path.0, initializers)?;
        } else {
            self.insert_constructor_initializers(&mut statements, insertion, initializers);
        }
        let statement_array = if let Some(original) = block
            .statements
            .and_then(|array| self.context.arena().node_array_ref(self.source, array))
        {
            self.context
                .factory()?
                .update_node_array(original, statements)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
        };
        block.statements = Some(statement_array.array());
        let flags =
            super::flags_after_update(self.context.arena(), body, &NodeData::Block(block.clone()))?;
        let body = self
            .context
            .factory()?
            .update_node(body, NodeData::Block(block), flags)?;
        self.context.factory()?.set_multi_line(body, true)?;
        data.body = Some(body.node());
        let flags = super::flags_after_update(
            self.context.arena(),
            constructor,
            &NodeData::Constructor(data.clone()),
        )?;
        self.context
            .factory()?
            .update_node(constructor, NodeData::Constructor(data), flags)
    }

    fn find_super_statement_path(
        &self,
        statements: &[TransformNode],
        start: usize,
    ) -> Result<Option<SuperStatementPath>, TransformError> {
        for (index, statement) in statements.iter().enumerate().skip(start) {
            if self.statement_is_super_call(*statement)? {
                return Ok(Some(SuperStatementPath(vec![index])));
            }
            let NodeData::TryStatement(data) = &self.context.arena().node(*statement)?.data else {
                continue;
            };
            let Some(try_block) = data
                .try_block
                .and_then(|block| self.context.arena().node_ref(self.source, block))
            else {
                continue;
            };
            let NodeData::Block(block) = &self.context.arena().node(try_block)?.data else {
                continue;
            };
            let nested = self.array_nodes(block.statements)?;
            if let Some(SuperStatementPath(mut path)) =
                self.find_super_statement_path(&nested, 0)?
            {
                path.insert(0, index);
                return Ok(Some(SuperStatementPath(path)));
            }
        }
        Ok(None)
    }

    /// tsc-port: transformConstructorBodyWorker @6.0.3
    /// tsc-hash: 37e090fcc937a5c99a0fce3410f7d5a67fd9612316d31ef64b3dba2d7212ad4a
    /// tsc-span: _tsc.js:97290-97328
    fn inject_initializers_at_super_path(
        &mut self,
        statements: &mut Vec<TransformNode>,
        path: &[usize],
        initializers: &MovedInstanceInitializers,
    ) -> Result<(), TransformError> {
        let (&index, remaining) =
            path.split_first()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Constructor,
                    field: "super statement path",
                })?;
        if remaining.is_empty() {
            self.insert_constructor_initializers(statements, index + 1, initializers);
            return Ok(());
        }

        let statement = *statements
            .get(index)
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Constructor,
                field: "super statement path index",
            })?;
        let NodeData::TryStatement(mut try_statement) =
            self.context.arena().node(statement)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Constructor,
                field: "try statement on super path",
            });
        };
        let try_block = try_statement
            .try_block
            .and_then(|block| self.context.arena().node_ref(self.source, block))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TryStatement,
                field: "try_block on super path",
            })?;
        let NodeData::Block(mut block) = self.context.arena().node(try_block)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TryStatement,
                field: "try block on super path",
            });
        };
        let mut nested = self.array_nodes(block.statements)?;
        self.inject_initializers_at_super_path(&mut nested, remaining, initializers)?;
        let nested = if let Some(original) = block.statements.map(|array| self.array(array)) {
            self.context
                .factory()?
                .update_node_array(original, nested)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, nested)?
        };
        block.statements = Some(nested.array());
        let flags = super::flags_after_update(
            self.context.arena(),
            try_block,
            &NodeData::Block(block.clone()),
        )?;
        let try_block =
            self.context
                .factory()?
                .update_node(try_block, NodeData::Block(block), flags)?;
        try_statement.try_block = Some(try_block.node());
        let flags = super::flags_after_update(
            self.context.arena(),
            statement,
            &NodeData::TryStatement(try_statement.clone()),
        )?;
        statements[index] = self.context.factory()?.update_node(
            statement,
            NodeData::TryStatement(try_statement),
            flags,
        )?;
        Ok(())
    }

    fn insert_constructor_initializers(
        &self,
        statements: &mut Vec<TransformNode>,
        insertion: usize,
        initializers: &MovedInstanceInitializers,
    ) {
        let parameter_end = statements[insertion..]
            .iter()
            .take_while(|statement| self.original_kind(**statement) == Some(SyntaxKind::Parameter))
            .count()
            + insertion;
        if initializers.parameter_assignments == ParameterPropertyAssignmentPolicy::Replace {
            statements.drain(insertion..parameter_end);
            statements.splice(
                insertion..insertion,
                initializers.statements.iter().copied(),
            );
        } else {
            statements.splice(
                parameter_end..parameter_end,
                initializers.statements.iter().copied(),
            );
        }
    }

    fn create_synthetic_constructor(
        &mut self,
        derived: bool,
        mut initializers: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if derived {
            let arguments = self.create_identifier("arguments")?;
            let spread = self.context.factory()?.create_node(
                self.source,
                NodeData::SpreadElement(tsc_syntax::nodes::SpreadElementData {
                    expression: Some(arguments.node()),
                }),
                TransformFlags::CONTAINS_REST_OR_SPREAD,
            )?;
            let argument_array = self
                .context
                .factory()?
                .create_node_array(self.source, vec![spread])?;
            let super_token = self.context.factory()?.create_token(
                self.source,
                SyntaxKind::SuperKeyword,
                TransformFlags::CONTAINS_LEXICAL_SUPER,
            )?;
            let call = self.context.factory()?.create_node(
                self.source,
                NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
                    expression: Some(super_token.node()),
                    question_dot_token: None,
                    type_arguments: None,
                    arguments: Some(argument_array.array()),
                }),
                TransformFlags::CONTAINS_LEXICAL_SUPER | TransformFlags::CONTAINS_REST_OR_SPREAD,
            )?;
            initializers.insert(0, self.create_expression_statement(call)?);
        }
        let body = self.create_block(initializers, true)?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, Vec::new())?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::Constructor(tsc_syntax::nodes::ConstructorData {
                name: None,
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn statement_is_super_call(&self, statement: TransformNode) -> Result<bool, TransformError> {
        let NodeData::ExpressionStatement(data) = &self.context.arena().node(statement)?.data
        else {
            return Ok(false);
        };
        let Some(expression) = data.expression else {
            return Ok(false);
        };
        let expression = self.skip_parenthesized_expression(self.node(expression))?;
        let NodeData::CallExpression(data) = &self.context.arena().node(expression)?.data else {
            return Ok(false);
        };
        Ok(data.expression.is_some_and(|expression| {
            self.context
                .arena()
                .node(self.node(expression))
                .is_ok_and(|node| node.kind == SyntaxKind::SuperKeyword)
        }))
    }

    /// `getSuperCallFromStatement` in tsc applies `skipParentheses` before
    /// testing for a direct `super()` call. Only parentheses are transparent
    /// here: comma expressions and other wrappers remain evaluation boundaries.
    fn skip_parenthesized_expression(
        &self,
        mut expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        loop {
            let NodeData::ParenthesizedExpression(data) =
                &self.context.arena().node(expression)?.data
            else {
                return Ok(expression);
            };
            let inner = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ParenthesizedExpression,
                    field: "expression",
                })?;
            expression = self.node(inner);
        }
    }

    fn original_kind(&self, node: TransformNode) -> Option<SyntaxKind> {
        let original = self.context.arena().get_original_node(node);
        self.context
            .arena()
            .node(original)
            .ok()
            .map(|node| node.kind)
    }

    /// tsc-port: transformClassMembers.parameterPropertyProjection @6.0.3
    /// tsc-hash: 306e5388a9a5c510a3594d97b7fbe7bf945415e4f4601770e266d55ce28765f8
    /// tsc-span: _tsc.js:94564-94598
    fn parameter_property_local(
        &self,
        property: TransformNode,
        emitted_name: Option<NodeId>,
    ) -> Result<Option<ParameterPropertyLocal>, TransformError> {
        let original = self.context.arena().get_original_node(property);
        let NodeData::Parameter(data) = &self.context.arena().node(original)?.data else {
            return Ok(None);
        };
        let Some(source_name) = data.name else {
            return Ok(None);
        };
        let source_name = TransformNode::new(original.source(), source_name);
        let Some(emitted_name) = emitted_name.map(|name| self.node(name)) else {
            return Ok(None);
        };
        if self.context.arena().node(source_name)?.kind != SyntaxKind::Identifier
            || self.context.arena().node(emitted_name)?.kind != SyntaxKind::Identifier
        {
            return Ok(None);
        }
        Ok(Some(ParameterPropertyLocal {
            emitted_name,
            source_name,
        }))
    }

    fn has_extends_clause(
        &self,
        heritage_clauses: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        Ok(self.array_nodes(heritage_clauses)?.iter().any(|clause| {
            matches!(
                &self.context.arena().node(*clause).ok().map(|node| &node.data),
                Some(NodeData::HeritageClause(data)) if data.token == SyntaxKind::ExtendsKeyword
            )
        }))
    }

    fn update_property(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::PropertyDeclarationData,
    ) -> Result<TransformNode, TransformError> {
        let flags = super::flags_after_update(
            self.context.arena(),
            original,
            &NodeData::PropertyDeclaration(data.clone()),
        )?;
        self.context
            .factory()?
            .update_node(original, NodeData::PropertyDeclaration(data), flags)
    }

    fn update_generic(
        &mut self,
        original: TransformNode,
        mut data: NodeData,
    ) -> Result<NodeId, TransformError> {
        try_visit_each_child(&mut data, self)?;
        let flags = super::flags_after_update(self.context.arena(), original, &data)?;
        Ok(self
            .context
            .factory()?
            .update_node(original, data, flags)?
            .node())
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

    fn create_arrow_function(
        &mut self,
        parameters: Vec<TransformNode>,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, parameters)?;
        let arrow = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::EqualsGreaterThanToken,
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ArrowFunction(tsc_syntax::nodes::ArrowFunctionData {
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers: None,
                equals_greater_than_token: Some(arrow.node()),
            }),
            TransformFlags::CONTAINS_LEXICAL_THIS,
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

    fn create_private_identifier(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::PrivateIdentifier(tsc_syntax::nodes::PrivateIdentifierData {
                escaped_text: tsc_syntax::escape_leading_underscores(text),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    fn allocate_temp_name(&mut self) -> String {
        loop {
            let ordinal = self.next_temp_name;
            self.next_temp_name += 1;
            let candidate = if ordinal < 26 {
                format!("_{}", char::from(b'a' + ordinal as u8))
            } else {
                format!("_{}", ordinal - 26)
            };
            if self.used_names.insert(candidate.clone()) {
                self.hoisted_names.push(candidate.clone());
                return candidate;
            }
        }
    }

    fn prepend_hoisted_declarations(
        &mut self,
        root: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if self.hoisted_names.is_empty() {
            return Ok(root);
        }
        let NodeData::SourceFile(mut data) = self.context.arena().node(root)?.data.clone() else {
            return Err(TransformError::RootKindExpected {
                actual: self.context.arena().node(root)?.kind,
            });
        };
        let mut declarations = Vec::with_capacity(self.hoisted_names.len());
        for name in self.hoisted_names.clone() {
            let name = self.create_identifier(&name)?;
            declarations.push(self.context.factory()?.create_node(
                self.source,
                NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                    name: Some(name.node()),
                    exclamation_token: None,
                    r#type: None,
                    initializer: None,
                }),
                TransformFlags::NONE,
            )?);
        }
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
        self.context
            .factory()?
            .set_node_flags(list, NodeFlags::NONE)?;
        let statement = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers: None,
                declaration_list: Some(list.node()),
            }),
            TransformFlags::NONE,
        )?;
        let original_statements = data
            .statements
            .and_then(|array| self.context.arena().node_array_ref(self.source, array));
        let mut statements = self.array_nodes(data.statements)?;
        let mut position = 0;
        while position < statements.len()
            && is_prologue_statement_node(self.context.arena(), statements[position])?
        {
            position += 1;
        }
        statements.insert(position, statement);
        let statements = if let Some(original) = original_statements {
            self.context
                .factory()?
                .update_node_array(original, statements)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
        };
        data.statements = Some(statements.array());
        let flags = self.context.arena().transform_flags(root);
        self.context
            .factory()?
            .update_node(root, NodeData::SourceFile(data), flags)
    }

    fn filter_modifier(
        &mut self,
        modifiers: Option<NodeArrayId>,
        removed: SyntaxKind,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(modifiers) = modifiers else {
            return Ok(None);
        };
        let original = self.array(modifiers);
        let retained = self
            .context
            .arena()
            .node_array(original)?
            .nodes
            .iter()
            .filter_map(|modifier| {
                self.context
                    .arena()
                    .node_ref(self.source, *modifier)
                    .filter(|modifier| {
                        self.context
                            .arena()
                            .node(*modifier)
                            .is_ok_and(|node| node.kind != removed)
                    })
            })
            .collect::<Vec<_>>();
        if retained.is_empty() {
            Ok(None)
        } else {
            Ok(Some(
                self.context
                    .factory()?
                    .update_node_array(original, retained)?
                    .array(),
            ))
        }
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
                .is_ok_and(|node| node.kind == expected)
        }))
    }

    fn name_is_private(&self, name: Option<NodeId>) -> Result<bool, TransformError> {
        Ok(name.is_some_and(|name| {
            self.context
                .arena()
                .node(self.node(name))
                .is_ok_and(|node| node.kind == SyntaxKind::PrivateIdentifier)
        }))
    }

    fn identifier_text(&self, node: TransformNode) -> Option<&str> {
        match &self.context.arena().node(node).ok()?.data {
            NodeData::Identifier(data) => Some(&data.text),
            _ => None,
        }
    }

    fn visit_optional_node(
        &mut self,
        node: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        node.map(|node| self.visit(node))
            .transpose()
            .map(Option::flatten)
    }

    fn visit_optional_nodes(
        &mut self,
        nodes: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        nodes
            .map(|nodes| self.visit_nodes(nodes))
            .transpose()
            .map(Option::flatten)
    }

    fn array_nodes(
        &self,
        array: Option<NodeArrayId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let Some(array) =
            array.and_then(|array| self.context.arena().node_array_ref(self.source, array))
        else {
            return Ok(Vec::new());
        };
        self.context
            .arena()
            .node_array(array)?
            .nodes
            .iter()
            .map(|node| {
                self.context
                    .arena()
                    .node_ref(self.source, *node)
                    .ok_or_else(|| TransformError::UnknownNode(self.node(*node)))
            })
            .collect()
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

struct TransformedAccessor {
    members: Vec<TransformNode>,
    initializer: TransformNode,
}

impl NodeDataChildVisitor for ClassFieldsVisitor<'_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("class-fields child belongs to the current transform source")
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
