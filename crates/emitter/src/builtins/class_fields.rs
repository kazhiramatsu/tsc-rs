use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    for_each_child, try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId,
    SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags, ScriptTarget};

use crate::{
    EmitFlags, InternalEmitFlags, TransformError, TransformFlags, TransformNode,
    TransformNodeArray, TransformRoot, TransformSourceId, TransformationContext, Transformer,
};

use super::{
    is_prologue_statement as is_prologue_statement_node, system::collect_identifier_texts,
};

/// tsc-port: transformClassFields @6.0.3
/// tsc-hash: 65cacc85f81402ff4468cf65c7636dbd5a0ce9eb6c3248f060aa5193c3af8304
/// tsc-span: _tsc.js:95852-98038
pub(super) fn transform_class_fields(options: &CompilerOptions) -> Box<dyn Transformer> {
    Box::new(ClassFieldsTransformer {
        target: options.emit_script_target(),
        use_define_for_class_fields: options.use_define_for_class_fields_effective(),
    })
}

struct ClassFieldsTransformer {
    target: ScriptTarget,
    use_define_for_class_fields: bool,
}

impl Transformer for ClassFieldsTransformer {
    fn name(&self) -> &'static str {
        "transformClassFields"
    }

    fn initialize(&mut self, _context: &mut TransformationContext) -> Result<(), TransformError> {
        if self.target != ScriptTarget::ES_NEXT {
            return Err(TransformError::UnsupportedCompilerOption {
                option: "class-field transform",
                detail: "target lowering remains owned by H2.5",
            });
        }
        Ok(())
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        if self.use_define_for_class_fields {
            return Ok(root);
        }
        let TransformRoot::SourceFile(source) = root else {
            return Err(TransformError::Unsupported(
                crate::UnsupportedEmitFeature::BundleRoot,
            ));
        };
        let root = context.arena().root(source)?;
        let mut visitor = ClassFieldsVisitor::new(context, source);
        let transformed =
            visitor
                .visit(root.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "root",
                })?;
        let transformed = visitor.prepend_hoisted_declarations(visitor.node(transformed))?;
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
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
}

impl<'context> ClassFieldsVisitor<'context> {
    fn new(context: &'context mut TransformationContext, source: TransformSourceId) -> Self {
        let used_names = collect_identifier_texts(context.arena(), source);
        Self {
            context,
            source,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
            used_names,
            hoisted_names: Vec::new(),
            next_temp_name: 0,
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
        mut data: tsc_syntax::nodes::ClassDeclarationData,
    ) -> Result<NodeId, TransformError> {
        let (members, prologue) = self.rewrite_computed_names_with_lexical_this(data.members)?;
        if prologue.is_some() {
            return Err(TransformError::UnsupportedSyntax {
                feature: crate::UnsupportedTransformFeature::Decorators,
                node: original,
            });
        }
        data.members = members;
        if !self.class_members_require_transform(data.members)? {
            return self.update_generic(original, NodeData::ClassDeclaration(data));
        }
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.heritage_clauses = self.visit_optional_nodes(data.heritage_clauses)?;
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        let derived = self.has_extends_clause(data.heritage_clauses)?;
        data.members = self.transform_members(data.members, derived)?;
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
        mut data: tsc_syntax::nodes::ClassExpressionData,
    ) -> Result<NodeId, TransformError> {
        let (members, prologue) = self.rewrite_computed_names_with_lexical_this(data.members)?;
        data.members = members;
        if !self.class_members_require_transform(data.members)? {
            let class = self.update_generic(original, NodeData::ClassExpression(data))?;
            return self.wrap_class_expression_prologue(self.node(class), prologue);
        }
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.heritage_clauses = self.visit_optional_nodes(data.heritage_clauses)?;
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        let derived = self.has_extends_clause(data.heritage_clauses)?;
        data.members = self.transform_members(data.members, derived)?;
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

    fn transform_members(
        &mut self,
        members: Option<NodeArrayId>,
        derived: bool,
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
        let move_instance_initializers = original_members.iter().try_fold(
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
        )?;

        let mut used_private_names = BTreeSet::new();
        for member in &original_members {
            let NodeData::PropertyDeclaration(data) =
                &self.context.arena().node(self.node(*member))?.data
            else {
                continue;
            };
            if let Some(name) = data.name {
                if let NodeData::PrivateIdentifier(name) =
                    &self.context.arena().node(self.node(name))?.data
                {
                    used_private_names.insert(name.text.clone());
                }
            }
        }

        let mut output = Vec::with_capacity(original_members.len() + 3);
        let mut instance_initializers = Vec::new();
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
                        if move_instance_initializers && !static_ {
                            let transformed = self.transform_auto_accessor(
                                member_node,
                                data,
                                &mut used_private_names,
                            )?;
                            instance_initializers.push(transformed.initializer);
                            output.extend(transformed.members);
                        } else {
                            output.push(self.update_property(member_node, data)?);
                        }
                    } else if private {
                        if move_instance_initializers && !static_ && data.initializer.is_some() {
                            let initializer = data.initializer.take().expect("initializer checked");
                            instance_initializers.push(
                                self.create_property_initializer_statement(
                                    member_node,
                                    data.name,
                                    initializer,
                                )?,
                            );
                        }
                        output.push(self.update_property(member_node, data)?);
                    } else if static_ {
                        if let Some(initializer) = data.initializer {
                            output.push(self.create_static_initializer_block(
                                member_node,
                                data.name,
                                initializer,
                            )?);
                        }
                    } else if let Some(initializer) = data.initializer {
                        instance_initializers.push(self.create_property_initializer_statement(
                            member_node,
                            data.name,
                            initializer,
                        )?);
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

        if !instance_initializers.is_empty() {
            if let Some(index) = constructor_index {
                output[index] = self
                    .inject_initializers_into_constructor(output[index], &instance_initializers)?;
            } else {
                output.insert(
                    0,
                    self.create_synthetic_constructor(derived, instance_initializers)?,
                );
            }
        }
        let updated = self
            .context
            .factory()?
            .update_node_array(original_array, output)?;
        Ok(Some(updated.array()))
    }

    fn class_members_require_transform(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for member in self.array_nodes(members)? {
            let NodeData::PropertyDeclaration(data) = &self.context.arena().node(member)?.data
            else {
                continue;
            };
            if !self.name_is_private(data.name)?
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
        used_private_names: &mut BTreeSet<String>,
    ) -> Result<TransformedAccessor, TransformError> {
        let name = data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::PropertyDeclaration,
            field: "name",
        })?;
        let base = match &self.context.arena().node(self.node(name))?.data {
            NodeData::Identifier(data) => data.text.trim_start_matches('#').to_owned(),
            NodeData::PrivateIdentifier(data) => data.text.trim_start_matches('#').to_owned(),
            _ => "accessor".to_owned(),
        };
        let mut storage = format!("#{base}_accessor_storage");
        let mut ordinal = 1usize;
        while !used_private_names.insert(storage.clone()) {
            storage = format!("#{base}_{ordinal}_accessor_storage");
            ordinal += 1;
        }
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
        let getter = self.create_get_accessor(name, storage_name.node(), modifiers)?;
        let setter = self.create_set_accessor(name, storage_name.node(), modifiers)?;
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
            .metadata_mut(getter)
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

    fn create_get_accessor(
        &mut self,
        name: NodeId,
        storage: NodeId,
        modifiers: Option<NodeArrayId>,
    ) -> Result<TransformNode, TransformError> {
        let access = self.create_this_access(Some(storage))?;
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
        let access = self.create_this_access(Some(storage))?;
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

    fn create_property_initializer_statement(
        &mut self,
        original: TransformNode,
        name: Option<NodeId>,
        initializer: NodeId,
    ) -> Result<TransformNode, TransformError> {
        let target = self.create_this_access(name)?;
        let assignment = self.create_assignment(target, self.node(initializer))?;
        let statement = self.create_expression_statement(assignment)?;
        self.set_original_and_range(statement, original)?;
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
        let name = name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::PropertyDeclaration,
            field: "name",
        })?;
        let this = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ThisKeyword,
            TransformFlags::CONTAINS_LEXICAL_THIS,
        )?;
        match self.context.arena().node(self.node(name))?.data.clone() {
            NodeData::Identifier(_) | NodeData::PrivateIdentifier(_) => {
                self.context.factory()?.create_node(
                    self.source,
                    NodeData::PropertyAccessExpression(
                        tsc_syntax::nodes::PropertyAccessExpressionData {
                            expression: Some(this.node()),
                            question_dot_token: None,
                            name: Some(name),
                        },
                    ),
                    TransformFlags::CONTAINS_LEXICAL_THIS,
                )
            }
            NodeData::ComputedPropertyName(data) => self.context.factory()?.create_node(
                self.source,
                NodeData::ElementAccessExpression(tsc_syntax::nodes::ElementAccessExpressionData {
                    expression: Some(this.node()),
                    question_dot_token: None,
                    argument_expression: data.expression,
                }),
                TransformFlags::CONTAINS_LEXICAL_THIS,
            ),
            _ => self.context.factory()?.create_node(
                self.source,
                NodeData::ElementAccessExpression(tsc_syntax::nodes::ElementAccessExpressionData {
                    expression: Some(this.node()),
                    question_dot_token: None,
                    argument_expression: Some(name),
                }),
                TransformFlags::CONTAINS_LEXICAL_THIS,
            ),
        }
    }

    fn inject_initializers_into_constructor(
        &mut self,
        constructor: TransformNode,
        initializers: &[TransformNode],
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
        let mut insertion = 0usize;
        while insertion < statements.len() && self.is_prologue_statement(statements[insertion])? {
            insertion += 1;
        }
        if let Some(super_index) = statements[insertion..]
            .iter()
            .position(|statement| self.statement_is_super_call(*statement).unwrap_or(false))
        {
            insertion += super_index + 1;
        }
        while insertion < statements.len()
            && self.original_kind(statements[insertion]) == Some(SyntaxKind::Parameter)
        {
            insertion += 1;
        }
        statements.splice(insertion..insertion, initializers.iter().copied());
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
        let NodeData::CallExpression(data) =
            &self.context.arena().node(self.node(expression))?.data
        else {
            return Ok(false);
        };
        Ok(data.expression.is_some_and(|expression| {
            self.context
                .arena()
                .node(self.node(expression))
                .is_ok_and(|node| node.kind == SyntaxKind::SuperKeyword)
        }))
    }

    fn is_prologue_statement(&self, statement: TransformNode) -> Result<bool, TransformError> {
        let NodeData::ExpressionStatement(data) = &self.context.arena().node(statement)?.data
        else {
            return Ok(false);
        };
        Ok(data.expression.is_some_and(|expression| {
            self.context
                .arena()
                .node(self.node(expression))
                .is_ok_and(|node| node.kind == SyntaxKind::StringLiteral)
        }))
    }

    fn original_kind(&self, node: TransformNode) -> Option<SyntaxKind> {
        let original = self.context.arena().get_original_node(node);
        self.context
            .arena()
            .node(original)
            .ok()
            .map(|node| node.kind)
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
        let mut statements = self.array_nodes(data.statements)?;
        let mut position = 0;
        while position < statements.len()
            && is_prologue_statement_node(self.context.arena(), statements[position])?
        {
            position += 1;
        }
        statements.insert(position, statement);
        data.statements = Some(
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
                .array(),
        );
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
