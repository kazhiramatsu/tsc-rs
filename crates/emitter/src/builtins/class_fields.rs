use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeCheckFlags, NodeFlags, ScriptTarget};

use crate::{
    CommentRange, EmitFlags, EmitHint, EmitResolver, InternalEmitFlags, LexicalEnvironment,
    LexicalEnvironmentFlags, SourceMapRange, SourceRange, TransformError, TransformFlags,
    TransformNode, TransformNodeArray, TransformRoot, TransformSourceId, TransformationContext,
    Transformer,
};

use super::{
    generated_bindings::{AncestorBindingPolicy, GeneratedBindingOwner, GeneratedBindingScopes},
    initialize_transform_flags,
    system::collect_identifier_texts,
    target_bindings::{finalize_generated_binding_names, TargetBinding},
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
        legacy_decorators: options.experimental_decorators,
    })
}

struct ClassFieldsTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    target: ScriptTarget,
    use_define_for_class_fields: bool,
    class_aliases: BTreeMap<(u32, u32), downlevel::ClassBinding>,
    legacy_decorators: bool,
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
        let root = context.arena().root(source)?;
        let transform_private_static_elements =
            context.arena().metadata(root).is_some_and(|metadata| {
                metadata
                    .internal_flags()
                    .contains(InternalEmitFlags::TRANSFORM_PRIVATE_STATIC_ELEMENTS)
            });
        // tsc-port: transformSourceFile @6.0.3 (private-static handoff)
        // tsc-hash: 6b4e789c9f79058aedb753f6e48b04c3b3966d3c02761c111fc08dabdc16c473
        // tsc-span: _tsc.js:95875-95920
        // `shouldTransformClassElementToWeakMap` honours the member flag at
        // every target: a decorated class with static private or
        // auto-accessor members is lowered at ES2022 and ESNext alike.
        if self.target < ScriptTarget::ES2022 || transform_private_static_elements {
            downlevel::transform_source(
                context,
                source,
                self.resolver,
                self.target,
                self.use_define_for_class_fields,
                self.target >= ScriptTarget::ES2021,
                &mut self.class_aliases,
            )?;
            return Ok(TransformRoot::SourceFile(source));
        }
        let mut visitor = ClassFieldsVisitor::new(
            context,
            source,
            self.target,
            self.use_define_for_class_fields,
            self.legacy_decorators,
            self.resolver,
            &mut self.class_aliases,
        )?;
        let transformed =
            visitor
                .visit(root.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "root",
                })?;
        let transformed = visitor.node(transformed);
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

struct ClassFieldsVisitor<'context, 'resolver, 'aliases> {
    context: &'context mut TransformationContext,
    resolver: &'resolver dyn EmitResolver,
    class_aliases: &'aliases mut BTreeMap<(u32, u32), downlevel::ClassBinding>,
    source: TransformSourceId,
    generated_names: GeneratedBindingScopes,
    class_frames: Vec<RetainedClassFrame>,
    computed_name_bindings: BTreeMap<NodeId, TargetBinding>,
    class_internal_names: BTreeMap<NodeId, TargetBinding>,
    parsed_private_names: BTreeSet<String>,
    private_name_scopes: Vec<RetainedPrivateNameScope>,
    private_storage_names: BTreeMap<NodeId, TransformNode>,
    target: ScriptTarget,
    use_define_for_class_fields: bool,
    legacy_decorators: bool,
}

struct ParameterPropertyLocal {
    emitted_name: TransformNode,
    source_name: TransformNode,
}

#[derive(Debug)]
struct SuperStatementPath(Vec<usize>);

impl<'context, 'resolver, 'aliases> ClassFieldsVisitor<'context, 'resolver, 'aliases> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        target: ScriptTarget,
        use_define_for_class_fields: bool,
        legacy_decorators: bool,
        resolver: &'resolver dyn EmitResolver,
        class_aliases: &'aliases mut BTreeMap<(u32, u32), downlevel::ClassBinding>,
    ) -> Result<Self, TransformError> {
        let used_names = collect_identifier_texts(context.arena(), source);
        let parsed_private_names = context
            .arena()
            .source(source)?
            .syntax()
            .arena
            .nodes()
            .iter()
            .filter_map(|node| match &node.data {
                NodeData::PrivateIdentifier(data)
                    if node.pos != u32::MAX
                        && node.end != u32::MAX
                        && (node.flags & NodeFlags::SYNTHESIZED.bits() as i32) == 0 =>
                {
                    Some(data.text.clone())
                }
                _ => None,
            })
            .collect();
        Ok(Self {
            context,
            resolver,
            class_aliases,
            source,
            generated_names: GeneratedBindingScopes::new(
                used_names,
                AncestorBindingPolicy::AllowShadow,
            ),
            class_frames: Vec::new(),
            computed_name_bindings: BTreeMap::new(),
            class_internal_names: BTreeMap::new(),
            parsed_private_names,
            private_name_scopes: Vec::new(),
            private_storage_names: BTreeMap::new(),
            target,
            use_define_for_class_fields,
            legacy_decorators,
        })
    }
}

impl<'context, 'resolver, 'aliases> ClassFieldsVisitor<'context, 'resolver, 'aliases> {
    fn create_accessor_storage_access(
        &mut self,
        storage: NodeId,
        receiver: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let receiver = match receiver {
            Some(receiver) => receiver,
            None => self.context.factory()?.create_token(
                self.source,
                SyntaxKind::ThisKeyword,
                TransformFlags::CONTAINS_LEXICAL_THIS,
            )?,
        };
        self.create_class_field_node(
            NodeData::PropertyAccessExpression(tsc_syntax::nodes::PropertyAccessExpressionData {
                expression: Some(receiver.node()),
                question_dot_token: None,
                name: Some(storage),
            }),
            TransformFlags::NONE,
        )
    }

    fn fresh_accessor_modifiers(
        &mut self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let modifiers = modifiers.map(|modifiers| self.array(modifiers));
        let flags = self.context.factory()?.modifier_flags(modifiers)?;
        Ok(self
            .context
            .factory()?
            .create_modifiers_from_modifier_flags(self.source, flags)?
            .map(|modifiers| modifiers.array()))
    }

    fn raw_comment_range(&self, node: TransformNode) -> Result<CommentRange, TransformError> {
        let arena = self.context.arena();
        let record = arena.node(node)?;
        CommentRange::from_raw(
            node.source(),
            record.pos,
            record.end,
            arena.source(node.source())?.syntax().positions(),
        )
        .map_err(|error| TransformError::InvalidSourceRange { node, error })
    }

    fn set_accessor_metadata(
        &mut self,
        original: TransformNode,
        backing: TransformNode,
        getter: TransformNode,
        setter: TransformNode,
    ) -> Result<(), TransformError> {
        let arena = self.context.arena();
        let record = arena.node(original)?;
        let metadata = arena.metadata(original);
        let comment_range = metadata
            .and_then(crate::EmitMetadata::comment_range)
            .map(Ok)
            .unwrap_or_else(|| self.raw_comment_range(original))?;
        let source_map_range = match metadata.and_then(crate::EmitMetadata::source_map_range) {
            Some(range) => range,
            None => SourceMapRange::new(
                original.source(),
                SourceRange::from_raw(
                    record.pos,
                    record.end,
                    arena.source(original.source())?.syntax().positions(),
                )
                .map_err(|error| TransformError::InvalidSourceRange {
                    node: original,
                    error,
                })?,
            ),
        };
        let arena = self.context.arena_mut()?;
        arena.set_original_node(backing, Some(original))?;
        arena
            .metadata_mut(backing)
            .set_flags(EmitFlags::NO_COMMENTS);
        arena
            .metadata_mut(backing)
            .set_source_map_range(source_map_range);
        arena.set_original_node(getter, Some(original))?;
        arena.metadata_mut(getter).set_comment_range(comment_range);
        arena
            .metadata_mut(getter)
            .set_source_map_range(source_map_range);
        arena.set_original_node(setter, Some(original))?;
        arena.metadata_mut(setter).set_flags(EmitFlags::NO_COMMENTS);
        arena
            .metadata_mut(setter)
            .set_source_map_range(source_map_range);
        Ok(())
    }

    fn complete_created_node_flags(&mut self, node: TransformNode) -> Result<(), TransformError> {
        let arena = self.context.arena();
        let record = arena.node(node)?;
        let flags = arena.transform_flags(node)
            | super::local_transform_flags(record)
            | super::local_contextual_target_flags(arena, self.source, record)?
            | super::factory_child_transform_flags(arena, self.source, record)?;
        self.context.arena_mut()?.set_transform_flags(node, flags);
        Ok(())
    }

    fn create_class_field_node(
        &mut self,
        data: NodeData,
        flags: TransformFlags,
    ) -> Result<TransformNode, TransformError> {
        let node = self
            .context
            .factory()?
            .create_node(self.source, data, flags)?;
        self.complete_created_node_flags(node)?;
        Ok(node)
    }

    fn create_get_accessor(
        &mut self,
        name: NodeId,
        storage: NodeId,
        modifiers: Option<NodeArrayId>,
        receiver: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let access = self.create_accessor_storage_access(storage, receiver)?;
        let return_statement = self.create_class_field_node(
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
        self.create_class_field_node(
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
        receiver: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let value = self.create_identifier("value")?;
        let parameter = self.create_class_field_node(
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
        let access = self.create_accessor_storage_access(storage, receiver)?;
        let assignment = self.create_assignment(access, value)?;
        let statement = self.create_expression_statement(assignment)?;
        let body = self.create_block(vec![statement], false)?;
        self.create_class_field_node(
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

    fn parameter_property_local(
        &self,
        property: TransformNode,
        emitted_name: Option<NodeId>,
    ) -> Result<Option<ParameterPropertyLocal>, TransformError> {
        let original = self.context.arena().get_original_node(property);
        let NodeData::Parameter(data) = &self.context.arena().node(original)?.data else {
            return Ok(None);
        };
        let parameter_modifiers = self.array_nodes(data.modifiers)?;
        let has_parameter_modifier = parameter_modifiers.iter().any(|modifier| {
            self.context.arena().node(*modifier).is_ok_and(|record| {
                matches!(
                    record.kind,
                    SyntaxKind::PublicKeyword
                        | SyntaxKind::PrivateKeyword
                        | SyntaxKind::ProtectedKeyword
                        | SyntaxKind::ReadonlyKeyword
                        | SyntaxKind::OverrideKeyword
                )
            })
        });
        let parent_is_constructor =
            self.context
                .arena()
                .node(original)?
                .parent
                .is_some_and(|parent| {
                    self.context
                        .arena()
                        .node(self.node(parent))
                        .is_ok_and(|record| record.kind == SyntaxKind::Constructor)
                });
        if !has_parameter_modifier || !parent_is_constructor {
            return Ok(None);
        }
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
        self.create_class_field_node(
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
        self.create_class_field_node(
            NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                type_arguments: None,
                arguments: Some(arguments.array()),
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
        self.create_class_field_node(
            NodeData::ArrowFunction(tsc_syntax::nodes::ArrowFunctionData {
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers: None,
                equals_greater_than_token: Some(arrow.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_expression_statement(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_class_field_node(
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
        let block = self.create_class_field_node(
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

    const fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    const fn array(&self, id: NodeArrayId) -> TransformNodeArray {
        TransformNodeArray::new(self.source, id)
    }
}
// A40 staged design only; not compiled or included by the production module.
// Class entry owns/restores RetainedClassFrame. Constructor and member-result
// callers must be completed before this is an implementation-ready candidate.

#[derive(Clone, Copy, Default)]
struct RetainedClassFacts {
    class_was_decorated: bool,
    needs_constructor_reference: bool,
    will_hoist_initializers: bool,
}

impl RetainedClassFacts {
    fn any(self) -> bool {
        self.class_was_decorated || self.needs_constructor_reference || self.will_hoist_initializers
    }
}

struct RetainedClassFrame {
    container: TransformNode,
    members: Option<NodeArrayId>,
    facts: RetainedClassFacts,
    receiver: Option<TransformNode>,
    pending_expressions: Vec<TransformNode>,
}

impl<'context, 'resolver, 'aliases> ClassFieldsVisitor<'context, 'resolver, 'aliases> {
    fn retained_resolver_flag(
        &self,
        node: TransformNode,
        flag: NodeCheckFlags,
    ) -> Result<bool, TransformError> {
        let Some(node) = self.context.arena().parse_tree_resolver_node(node)? else {
            return Ok(false);
        };
        Ok(self
            .resolver
            .has_node_check_flag(node, flag.bits() as u32)?)
    }

    fn retained_class_facts(
        &self,
        container: TransformNode,
        members: Option<NodeArrayId>,
    ) -> Result<RetainedClassFacts, TransformError> {
        // Called only after the existing transform_root retained-target guard.
        // Private/static downlevel, static-this and static-super predicates are
        // false here. ClassWasDecorated does not alter either retained fact.
        let class_name = self.class_declared_name(container)?;
        let has_class_this = self
            .context
            .arena()
            .metadata(container)
            .and_then(|metadata| metadata.class_this)
            .is_some();
        let mut facts = RetainedClassFacts {
            class_was_decorated: self.retained_class_was_decorated(container)?,
            ..RetainedClassFacts::default()
        };
        for member in self.array_nodes(members)? {
            let record = self.context.arena().node(member)?;
            let (name, modifiers, initializer, property) = match &record.data {
                NodeData::PropertyDeclaration(data) => {
                    (data.name, data.modifiers, data.initializer, true)
                }
                NodeData::MethodDeclaration(data) => (data.name, data.modifiers, None, false),
                NodeData::GetAccessor(data) => (data.name, data.modifiers, None, false),
                NodeData::SetAccessor(data) => (data.name, data.modifiers, None, false),
                _ => continue,
            };
            let accessor = property && self.has_modifier(modifiers, SyntaxKind::AccessorKeyword)?;
            if self.has_modifier(modifiers, SyntaxKind::StaticKeyword)? {
                if accessor
                    && self.target < ScriptTarget::ES_NEXT
                    && class_name.is_none()
                    && !has_class_this
                {
                    facts.needs_constructor_reference = true;
                }
                continue;
            }
            let original = self.context.arena().get_original_node(member);
            let original_modifiers = match &self.context.arena().node(original)?.data {
                NodeData::PropertyDeclaration(data) => data.modifiers,
                NodeData::MethodDeclaration(data) => data.modifiers,
                NodeData::GetAccessor(data) => data.modifiers,
                NodeData::SetAccessor(data) => data.modifiers,
                _ => None,
            };
            if self.has_modifier(original_modifiers, SyntaxKind::AbstractKeyword)? {
                continue;
            }
            if accessor {
                // This branch precedes the private constructor-reference test
                // in getClassFacts, even for a private auto-accessor.
                continue;
            }
            if self.name_is_private(name)? {
                facts.needs_constructor_reference |= self.retained_resolver_flag(
                    member,
                    NodeCheckFlags::CONTAINS_CONSTRUCTOR_REFERENCE,
                )?;
            } else if property && !self.use_define_for_class_fields && initializer.is_some() {
                facts.will_hoist_initializers = true;
            }
        }
        Ok(facts)
    }

    fn class_declared_name(&self, class: TransformNode) -> Result<Option<NodeId>, TransformError> {
        match &self.context.arena().node(class)?.data {
            NodeData::ClassDeclaration(data) => Ok(data.name),
            NodeData::ClassExpression(data) => Ok(data.name),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(class)?.kind,
                field: "class container",
            }),
        }
    }

    fn should_transform_retained_auto_accessors(&self) -> bool {
        self.target < ScriptTarget::ES_NEXT
            || !self.use_define_for_class_fields
                && self
                    .class_frames
                    .last()
                    .is_some_and(|frame| frame.facts.will_hoist_initializers)
    }

    fn visit_class_member_modifiers(
        &mut self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(modifiers) = modifiers else {
            return Ok(None);
        };
        let original = self.array(modifiers);
        let remove_accessor = self.should_transform_retained_auto_accessors();
        let mut retained = Vec::new();
        for modifier in self.array_nodes(Some(modifiers))? {
            let kind = self.context.arena().node(modifier)?.kind;
            if kind == SyntaxKind::Decorator
                || remove_accessor && kind == SyntaxKind::AccessorKeyword
            {
                continue;
            }
            retained.push(modifier);
        }
        Ok(Some(
            self.context
                .factory()?
                .update_node_array(original, retained)?
                .array(),
        ))
    }

    fn visit_class_member_name(
        &mut self,
        name: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        let Some(name) = name else {
            return Ok(None);
        };
        let original = self.node(name);
        let NodeData::ComputedPropertyName(mut data) =
            self.context.arena().node(original)?.data.clone()
        else {
            return self.visit(name);
        };
        let expression =
            self.visit_required_expression(data.expression, SyntaxKind::ComputedPropertyName)?;
        data.expression = Some(self.inject_pending_expressions(expression)?.node());
        Ok(Some(
            self.update_contextual_node(original, NodeData::ComputedPropertyName(data))?
                .node(),
        ))
    }

    fn visit_required_expression(
        &mut self,
        expression: Option<NodeId>,
        parent: SyntaxKind,
    ) -> Result<TransformNode, TransformError> {
        self.visit_optional_node(expression)?
            .map(|node| self.node(node))
            .ok_or(TransformError::RequiredChildRemoved {
                parent,
                field: "expression",
            })
    }

    fn inject_pending_expressions(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let Some(frame) = self.class_frames.last_mut() else {
            return Ok(expression);
        };
        if frame.pending_expressions.is_empty() {
            return Ok(expression);
        }
        let mut pending = std::mem::take(&mut frame.pending_expressions);
        if let NodeData::ParenthesizedExpression(mut data) =
            self.context.arena().node(expression)?.data.clone()
        {
            let inner = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ParenthesizedExpression,
                    field: "expression",
                })?;
            pending.push(self.node(inner));
            data.expression = Some(self.inline_expressions(pending)?.node());
            self.update_contextual_node(expression, NodeData::ParenthesizedExpression(data))
        } else {
            pending.push(expression);
            self.inline_expressions(pending)
        }
    }

    fn skip_retained_outer_expressions(
        &self,
        mut expression: TransformNode,
        partially_emitted_only: bool,
    ) -> Result<TransformNode, TransformError> {
        loop {
            let record = self.context.arena().node(expression)?;
            let inner = match &record.data {
                NodeData::PartiallyEmittedExpression(data) => data.expression,
                _ if partially_emitted_only => return Ok(expression),
                NodeData::ParenthesizedExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::TypeAssertionExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                NodeData::ExpressionWithTypeArguments(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                _ => return Ok(expression),
            };
            expression =
                inner
                    .map(|node| self.node(node))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: record.kind,
                        field: "expression",
                    })?;
        }
    }

    fn generated_assignment_left(
        &self,
        expression: TransformNode,
        exclude_compound: bool,
    ) -> Result<Option<TransformNode>, TransformError> {
        let NodeData::BinaryExpression(data) = &self.context.arena().node(expression)?.data else {
            return Ok(None);
        };
        let Some(operator) = data.operator_token else {
            return Ok(None);
        };
        let operator = self.context.arena().node(self.node(operator))?.kind;
        let assignment = if exclude_compound {
            operator == SyntaxKind::EqualsToken
        } else {
            operator.value() >= SyntaxKind::FirstAssignment.value()
                && operator.value() <= SyntaxKind::LastAssignment.value()
        };
        let Some(left) = data.left.map(|node| self.node(node)) else {
            return Ok(None);
        };
        let generated = self.context.arena().node(left)?.kind == SyntaxKind::Identifier
            && self
                .context
                .arena()
                .metadata(left)
                .and_then(crate::EmitMetadata::generated_binding_id)
                .is_some();
        // A generated Identifier is a left-hand-side expression, satisfying
        // the final predicate of upstream isAssignmentExpression as well.
        Ok((assignment && generated).then_some(left))
    }

    fn find_computed_name_cache(
        &self,
        mut expression: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        loop {
            expression = self.skip_retained_outer_expressions(expression, false)?;
            match &self.context.arena().node(expression)?.data {
                NodeData::CommaListExpression(data) => {
                    let elements = self.array_nodes(data.elements)?;
                    expression = *elements
                        .last()
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::CommaListExpression,
                            field: "last element",
                        })?;
                }
                NodeData::BinaryExpression(data)
                    if data
                        .operator_token
                        .map(|operator| self.context.arena().node(self.node(operator)))
                        .transpose()?
                        .is_some_and(|operator| operator.kind == SyntaxKind::CommaToken) =>
                {
                    expression = data.right.map(|node| self.node(node)).ok_or(
                        TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::BinaryExpression,
                            field: "right",
                        },
                    )?;
                }
                _ => return self.generated_assignment_left(expression, true),
            }
        }
    }

    fn retained_simple_inlineable(
        &self,
        expression: TransformNode,
    ) -> Result<bool, TransformError> {
        let kind = self.context.arena().node(expression)?.kind;
        // isSimpleCopiableExpression + !isIdentifier: BigIntLiteral is absent.
        Ok(matches!(
            kind,
            SyntaxKind::StringLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::NumericLiteral
        ) || kind.value() >= SyntaxKind::FirstKeyword.value()
            && kind.value() <= SyntaxKind::LastKeyword.value())
    }

    /// The generated binding an identifier already carries (a cache
    /// assignment target hoisted by an earlier transform).
    fn binding_of_generated_identifier(&self, name: TransformNode) -> Option<TargetBinding> {
        let metadata = self.context.arena().metadata(name)?;
        let id = metadata.generated_binding_id()?;
        let NodeData::Identifier(identifier) = &self.context.arena().node(name).ok()?.data else {
            return None;
        };
        Some(TargetBinding::from_existing(
            id,
            identifier.text.clone(),
            metadata.generated_binding_base().map(str::to_owned),
            metadata
                .generated_binding_preferred_base()
                .map(str::to_owned),
            metadata.generated_binding_role_suffix().map(str::to_owned),
            metadata.generated_binding_is_file_level_optimistic(),
            metadata.generated_binding_planned_name_is_authoritative(),
            metadata.generated_binding_reserved_in_nested_scopes(),
        ))
    }

    fn computed_name_binding(
        &mut self,
        name: TransformNode,
    ) -> Result<TargetBinding, TransformError> {
        let key = self.context.arena().get_original_node(name).node();
        if let Some(binding) = self.computed_name_bindings.get(&key) {
            return Ok(binding.clone());
        }
        // getGeneratedNameForNode can be called by constructor initialization
        // before the name's evaluation owner hoists its declaration.
        let provisional = self.generated_names.allocate_temp();
        let binding = TargetBinding::allocate_reserved_in_nested_scopes(self.context, provisional)?;
        self.computed_name_bindings.insert(key, binding.clone());
        Ok(binding)
    }

    fn property_name_expression_if_needed(
        &mut self,
        name: TransformNode,
        should_hoist: bool,
    ) -> Result<Option<TransformNode>, TransformError> {
        let NodeData::ComputedPropertyName(data) = self.context.arena().node(name)?.data.clone()
        else {
            return Ok(None);
        };
        let original_expression = data.expression.map(|node| self.node(node)).ok_or(
            TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ComputedPropertyName,
                field: "expression",
            },
        )?;
        let cache = self.find_computed_name_cache(original_expression)?;
        let expression =
            self.visit_required_expression(data.expression, SyntaxKind::ComputedPropertyName)?;
        let inner = self.skip_retained_outer_expressions(expression, true)?;
        let inlineable = self.retained_simple_inlineable(inner)?;
        let already_transformed =
            cache.is_some() || self.generated_assignment_left(inner, false)?.is_some();
        if !already_transformed && !inlineable && should_hoist {
            let binding = self.computed_name_binding(name)?;
            let generated = self.create_binding_identifier(&binding)?;
            if self.retained_resolver_flag(name, NodeCheckFlags::BLOCK_SCOPED_BINDING_IN_LOOP)? {
                self.context.add_block_scoped_variable(generated)?;
            } else {
                self.context.hoist_variable_declaration(generated)?;
            }
            return self.create_assignment(generated, expression).map(Some);
        }
        Ok(
            (!inlineable && self.context.arena().node(inner)?.kind != SyntaxKind::Identifier)
                .then_some(expression),
        )
    }

    fn raw_map_range(&self, node: TransformNode) -> Result<SourceMapRange, TransformError> {
        let record = self.context.arena().node(node)?;
        SourceRange::from_raw(
            record.pos,
            record.end,
            self.context
                .arena()
                .source(node.source())?
                .syntax()
                .positions(),
        )
        .map(|range| SourceMapRange::new(node.source(), range))
        .map_err(|error| TransformError::InvalidSourceRange { node, error })
    }

    fn retained_auto_accessor_names(
        &mut self,
        name: TransformNode,
    ) -> Result<(NodeId, NodeId), TransformError> {
        let NodeData::ComputedPropertyName(data) = self.context.arena().node(name)?.data.clone()
        else {
            return Ok((name.node(), name.node()));
        };
        let original_expression = data.expression.map(|node| self.node(node)).ok_or(
            TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ComputedPropertyName,
                field: "expression",
            },
        )?;
        if self.retained_simple_inlineable(original_expression)? {
            return Ok((name.node(), name.node()));
        }
        let (getter_expression, setter_expression) = if let Some(left) =
            self.find_computed_name_cache(original_expression)?
        {
            (
                self.visit_required_expression(data.expression, SyntaxKind::ComputedPropertyName)?,
                left,
            )
        } else {
            let binding = self.allocate_binding(RetainedBindingPlacement::Hoisted, false)?;
            let temp = self.create_binding_identifier(&binding)?;
            let range = self.raw_map_range(original_expression)?;
            self.context
                .arena_mut()?
                .metadata_mut(temp)
                .set_source_map_range(range);
            let expression =
                self.visit_required_expression(data.expression, SyntaxKind::ComputedPropertyName)?;
            let assignment = self.create_assignment(temp, expression)?;
            self.context
                .arena_mut()?
                .metadata_mut(assignment)
                .set_source_map_range(range);
            (assignment, temp)
        };
        let getter = self.update_contextual_node(
            name,
            NodeData::ComputedPropertyName(tsc_syntax::nodes::ComputedPropertyNameData {
                expression: Some(getter_expression.node()),
            }),
        )?;
        let setter = self.update_contextual_node(
            name,
            NodeData::ComputedPropertyName(tsc_syntax::nodes::ComputedPropertyNameData {
                expression: Some(setter_expression.node()),
            }),
        )?;
        Ok((getter.node(), setter.node()))
    }
}

// A40 staged retained property/private-name design. Not a production module.

#[derive(Default)]
struct RetainedPrivateNameScope {
    allocated: BTreeSet<String>,
    next_anonymous: usize,
}

impl<'context, 'resolver, 'aliases> ClassFieldsVisitor<'context, 'resolver, 'aliases> {
    fn prepare_retained_private_storage(
        &mut self,
        members: Option<NodeArrayId>,
    ) -> Result<(), TransformError> {
        if !self.should_transform_retained_auto_accessors() {
            return Ok(());
        }
        // Printer generateMemberNames precedes emitting any constructor/body.
        // Heritage visitation must finish before entering this private scope.
        for member in self.array_nodes(members)? {
            let NodeData::PropertyDeclaration(data) = &self.context.arena().node(member)?.data
            else {
                continue;
            };
            if self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)? {
                let name = data.name.ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyDeclaration,
                    field: "name",
                })?;
                self.retained_private_storage(self.node(name))?;
            }
        }
        Ok(())
    }

    fn retained_member_name_text(&self, name: TransformNode) -> Result<String, TransformError> {
        let record = self.context.arena().node(name)?;
        let text = match &record.data {
            NodeData::Identifier(data) => &data.text,
            NodeData::PrivateIdentifier(data) => &data.text,
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: record.kind,
                    field: "member name text",
                })
            }
        };
        if let Some(generated) = self
            .context
            .arena()
            .metadata(name)
            .and_then(crate::EmitMetadata::generated_binding_id)
            .and_then(|binding| self.context.generated_binding_name(binding))
        {
            return Ok(generated.to_owned());
        }
        if record.parent.is_none() || record.pos == u32::MAX || record.end == u32::MAX {
            return Ok(text.clone());
        }
        let syntax = self.context.arena().source(name.source())?.syntax();
        let start = tsc_syntax::skip_trivia(syntax.text(), record.pos as usize);
        syntax
            .text()
            .get(start..record.end as usize)
            .map(str::to_owned)
            .ok_or(TransformError::RequiredChildRemoved {
                parent: record.kind,
                field: "source member spelling",
            })
    }

    fn retained_private_storage(
        &mut self,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let original = self.context.arena().get_original_node(name);
        let key = original.node();
        if let Some(storage) = self.private_storage_names.get(&key) {
            return Ok(*storage);
        }
        let base = if matches!(
            self.context.arena().node(original)?.kind,
            SyntaxKind::Identifier | SyntaxKind::PrivateIdentifier
        ) {
            Some(self.retained_member_name_text(original)?)
        } else {
            None
        };
        let mut ordinal = 0usize;
        loop {
            let candidate = if let Some(base) = &base {
                let base = base.strip_prefix('#').unwrap_or(base);
                if ordinal == 0 {
                    format!("#{base}_accessor_storage")
                } else if base.ends_with('_') {
                    format!("#{base}{ordinal}_accessor_storage")
                } else {
                    format!("#{base}_{ordinal}_accessor_storage")
                }
            } else {
                let scope = self.private_name_scopes.last_mut().ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyDeclaration,
                        field: "private-name scope",
                    },
                )?;
                let count = scope.next_anonymous;
                scope.next_anonymous += 1;
                if count == 8 || count == 13 {
                    continue;
                }
                let stem = if count < 26 {
                    format!("_{}", char::from(b'a' + count as u8))
                } else {
                    format!("_{}", count - 26)
                };
                format!("#{stem}_accessor_storage")
            };
            ordinal += 1;
            if self.parsed_private_names.contains(&candidate)
                || self
                    .private_name_scopes
                    .iter()
                    .any(|scope| scope.allocated.contains(&candidate))
            {
                continue;
            }
            self.private_name_scopes
                .last_mut()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyDeclaration,
                    field: "private-name scope",
                })?
                .allocated
                .insert(candidate.clone());
            let storage = self.create_private_identifier(&candidate)?;
            self.context
                .arena_mut()?
                .set_original_node(storage, Some(name))?;
            self.private_storage_names.insert(key, storage);
            return Ok(storage);
        }
    }

    fn transform_retained_auto_accessor(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::PropertyDeclarationData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let name =
            data.name
                .map(|node| self.node(node))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyDeclaration,
                    field: "name",
                })?;
        let (getter_name, setter_name) = self.retained_auto_accessor_names(name)?;
        let modifiers = self.visit_class_member_modifiers(data.modifiers)?;
        let storage = self.retained_private_storage(name)?;
        let backing = self.update_property(
            original,
            tsc_syntax::nodes::PropertyDeclarationData {
                name: Some(storage.node()),
                modifiers,
                question_token: None,
                exclamation_token: None,
                r#type: None,
                initializer: data.initializer,
            },
        )?;
        self.complete_created_node_flags(backing)?;
        let receiver = if self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)? {
            self.class_frames.last().and_then(|frame| frame.receiver)
        } else {
            None
        };
        let getter = self.create_get_accessor(getter_name, storage.node(), modifiers, receiver)?;
        let setter_modifiers = self.fresh_accessor_modifiers(modifiers)?;
        let setter =
            self.create_set_accessor(setter_name, storage.node(), setter_modifiers, receiver)?;
        self.set_accessor_metadata(original, backing, getter, setter)?;
        let mut output = Vec::with_capacity(3);
        if let Some(backing) = self.transform_retained_field(backing)? {
            output.push(backing);
        }
        for accessor in [getter, setter] {
            let data = self.context.arena().node(accessor)?.data.clone();
            let accessor = self.visit_class_member_function(accessor, data)?;
            output.push(self.node(accessor));
        }
        Ok(output)
    }

    fn transform_retained_field(
        &mut self,
        original: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let NodeData::PropertyDeclaration(mut data) =
            self.context.arena().node(original)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyDeclaration,
                field: "field data",
            });
        };
        let static_ = self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)?;
        let private = self.name_is_private(data.name)?;
        let will_hoist = self
            .class_frames
            .last()
            .is_some_and(|frame| frame.facts.will_hoist_initializers);
        if private && !self.use_define_for_class_fields && !static_ && will_hoist {
            data.modifiers = self.visit_optional_nodes(data.modifiers)?;
            data.question_token = None;
            data.exclamation_token = None;
            data.r#type = None;
            data.initializer = None;
            return self.update_property(original, data).map(Some);
        }
        if !private
            && !self.use_define_for_class_fields
            && !self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?
        {
            if let Some(name) = data.name {
                if let Some(expression) = self.property_name_expression_if_needed(
                    self.node(name),
                    data.initializer.is_some(),
                )? {
                    let mut pending = Vec::new();
                    self.flatten_retained_comma_list(expression, &mut pending)?;
                    self.class_frames
                        .last_mut()
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::PropertyDeclaration,
                            field: "class environment",
                        })?
                        .pending_expressions
                        .extend(pending);
                }
            }
            if static_ {
                if let Some(statement) = self.retained_property_initializer(original)? {
                    let body = self.create_block(vec![statement], false)?;
                    let block = self.create_class_field_node(
                        NodeData::ClassStaticBlockDeclaration(
                            tsc_syntax::nodes::ClassStaticBlockDeclarationData {
                                body: Some(body.node()),
                                modifiers: None,
                            },
                        ),
                        TransformFlags::NONE,
                    )?;
                    let comments = self.raw_comment_range(original)?;
                    let arena = self.context.arena_mut()?;
                    arena.set_original_node(block, Some(original))?;
                    arena.metadata_mut(block).set_comment_range(comments);
                    let metadata = arena.metadata_mut(statement);
                    metadata.set_comment_range(CommentRange::new(
                        self.source,
                        SourceRange::Synthesized,
                    ));
                    metadata.leading_comments.clear();
                    metadata.trailing_comments.clear();
                    return Ok(Some(block));
                }
            }
            return Ok(None);
        }
        data.modifiers = self.visit_class_member_modifiers(data.modifiers)?;
        data.name = self.visit_class_member_name(data.name)?;
        data.question_token = None;
        data.exclamation_token = None;
        data.r#type = None;
        data.initializer = self.visit_optional_node(data.initializer)?;
        self.update_property(original, data).map(Some)
    }

    fn retained_property_initializer(
        &mut self,
        property: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let NodeData::PropertyDeclaration(data) = self.context.arena().node(property)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyDeclaration,
                field: "initializer field",
            });
        };
        let source_name =
            data.name
                .map(|node| self.node(node))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyDeclaration,
                    field: "name",
                })?;
        let name = if self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)? {
            self.retained_private_storage(source_name)?
        } else if let NodeData::ComputedPropertyName(mut computed) =
            self.context.arena().node(source_name)?.data.clone()
        {
            let expression = computed.expression.map(|node| self.node(node)).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ComputedPropertyName,
                    field: "expression",
                },
            )?;
            if self.retained_simple_inlineable(expression)? {
                source_name
            } else {
                // getGeneratedNameForNode(property.name): when an earlier
                // transform (standard decorators) already cached the key as
                // `[…, temp = __propKey(expr)]`, that temp is the name's
                // generated binding (findComputedPropertyNameCacheAssignment).
                let cached = self
                    .find_computed_name_cache(expression)?
                    .and_then(|left| self.binding_of_generated_identifier(left));
                let binding = match cached {
                    Some(binding) => {
                        let key = self.context.arena().get_original_node(source_name).node();
                        self.computed_name_bindings
                            .entry(key)
                            .or_insert_with(|| binding.clone());
                        binding
                    }
                    None => self.computed_name_binding(source_name)?,
                };
                computed.expression = Some(self.create_binding_identifier(&binding)?.node());
                self.update_contextual_node(source_name, NodeData::ComputedPropertyName(computed))?
            }
        } else {
            source_name
        };
        let static_ = self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)?;
        let private = self.context.arena().node(name)?.kind == SyntaxKind::PrivateIdentifier;
        if (private || static_) && data.initializer.is_none() {
            return Ok(None);
        }
        let original = self.context.arena().get_original_node(property);
        let original_modifiers = match &self.context.arena().node(original)?.data {
            NodeData::PropertyDeclaration(data) => data.modifiers,
            NodeData::Parameter(data) => data.modifiers,
            _ => None,
        };
        if self.has_modifier(original_modifiers, SyntaxKind::AbstractKeyword)? {
            return Ok(None);
        }
        let initializer = self
            .visit_optional_node(data.initializer)?
            .map(|node| self.node(node));
        let initializer =
            if let Some(local) = self.parameter_property_local(property, Some(name.node()))? {
                let local_name = self.context.factory()?.clone_node(local.emitted_name)?;
                let initializer = if let Some(mut prefix) = initializer {
                    if let Some(run_initializers) = self.retained_run_initializers_prefix(prefix)? {
                        prefix = run_initializers;
                    }
                    self.inline_expressions(vec![prefix, local_name])?
                } else {
                    local_name
                };
                let range = self.raw_map_range(local.source_name)?;
                let arena = self.context.arena_mut()?;
                arena
                    .metadata_mut(name)
                    .set_flags(EmitFlags::NO_COMMENTS | EmitFlags::NO_SOURCE_MAP);
                arena.metadata_mut(local_name).set_source_map_range(range);
                arena
                    .metadata_mut(local_name)
                    .set_flags(EmitFlags::NO_COMMENTS);
                initializer
            } else if let Some(initializer) = initializer {
                initializer
            } else {
                self.create_void_zero()?
            };
        // Retained public fields are emitted natively under define mode;
        // constructor/static initializer requests here are assignment/private.
        if self.use_define_for_class_fields && !private {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyDeclaration,
                field: "retained assignment/private initializer",
            });
        }
        let target = self.create_this_access(Some(name.node()))?;
        self.complete_created_node_flags(target)?;
        self.context
            .arena_mut()?
            .metadata_mut(target)
            .add_flags(EmitFlags::NO_LEADING_COMMENTS);
        let expression = self.create_assignment(target, initializer)?;
        if static_
            && self
                .class_frames
                .last()
                .is_some_and(|frame| frame.facts.any())
        {
            let range = self
                .context
                .arena()
                .metadata(source_name)
                .and_then(crate::EmitMetadata::source_map_range)
                .map(Ok)
                .unwrap_or_else(|| self.raw_map_range(source_name))?;
            let arena = self.context.arena_mut()?;
            arena.set_original_node(expression, Some(property))?;
            arena
                .metadata_mut(expression)
                .add_flags(EmitFlags::ADVISE_ON_EMIT_NODE);
            arena.metadata_mut(expression).set_source_map_range(range);
            // Static-this/super substitution is unreachable on this retained
            // branch, so the upstream lexical-map entry has no active consumer.
        }
        let statement = self.create_expression_statement(expression)?;
        let comments = self.raw_comment_range(property)?;
        let inherited_flags = self
            .context
            .arena()
            .metadata(property)
            .map(|metadata| {
                EmitFlags::from_bits(metadata.flags().bits() & EmitFlags::NO_COMMENTS.bits())
            })
            .unwrap_or(EmitFlags::NONE);
        let parameter = self.context.arena().node(original)?.kind == SyntaxKind::Parameter;
        let range = if parameter {
            self.raw_map_range(original)?
        } else {
            self.retained_initializer_map_range(property)?
        };
        let accessor = self.has_modifier(original_modifiers, SyntaxKind::AccessorKeyword)?;
        let arena = self.context.arena_mut()?;
        arena.set_original_node(statement, Some(property))?;
        arena.metadata_mut(statement).add_flags(inherited_flags);
        arena.metadata_mut(statement).set_comment_range(comments);
        arena.metadata_mut(statement).set_source_map_range(range);
        if parameter {
            arena.remove_all_comments(statement);
        }
        arena.metadata_mut(expression).leading_comments.clear();
        arena.metadata_mut(expression).trailing_comments.clear();
        if accessor {
            arena
                .metadata_mut(statement)
                .add_flags(EmitFlags::NO_COMMENTS);
        }
        Ok(Some(statement))
    }

    fn retained_initializer_map_range(
        &self,
        property: TransformNode,
    ) -> Result<SourceMapRange, TransformError> {
        let record = self.context.arena().node(property)?;
        let NodeData::PropertyDeclaration(data) = &record.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: record.kind,
                field: "property initializer range",
            });
        };
        let name = data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::PropertyDeclaration,
            field: "name",
        })?;
        // moveRangePastModifiers has a property/method-specific arm: use
        // name.pos, even when it differs from the last modifier's end.
        let start = self.context.arena().node(self.node(name))?.pos;
        SourceRange::from_raw(
            start,
            record.end,
            self.context
                .arena()
                .source(property.source())?
                .syntax()
                .positions(),
        )
        .map(|range| SourceMapRange::new(property.source(), range))
        .map_err(|error| TransformError::InvalidSourceRange {
            node: property,
            error,
        })
    }

    fn retained_run_initializers_prefix(
        &self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let NodeData::ParenthesizedExpression(data) = &self.context.arena().node(node)?.data else {
            return Ok(None);
        };
        let Some(inner) = data.expression.map(|node| self.node(node)) else {
            return Ok(None);
        };
        let NodeData::BinaryExpression(data) = &self.context.arena().node(inner)?.data else {
            return Ok(None);
        };
        let Some(operator) = data.operator_token else {
            return Ok(None);
        };
        if self.context.arena().node(self.node(operator))?.kind != SyntaxKind::CommaToken {
            return Ok(None);
        }
        let (Some(left), Some(right)) = (data.left, data.right) else {
            return Ok(None);
        };
        let left = self.node(left);
        let NodeData::VoidExpression(data) = &self.context.arena().node(self.node(right))?.data
        else {
            return Ok(None);
        };
        let Some(number) = data.expression else {
            return Ok(None);
        };
        if self.context.arena().node(self.node(number))?.kind != SyntaxKind::NumericLiteral {
            return Ok(None);
        }
        Ok(self
            .context
            .arena()
            .is_call_to_emit_helper(left, crate::factory::EmitHelperName::RunInitializers)?
            .then_some(left))
    }

    fn flatten_retained_comma_list(
        &self,
        node: TransformNode,
        output: &mut Vec<TransformNode>,
    ) -> Result<(), TransformError> {
        let record = self.context.arena().node(node)?;
        match &record.data {
            NodeData::ParenthesizedExpression(data)
                if (record.pos == u32::MAX || record.end == u32::MAX)
                    && self.context.arena().metadata(node).is_none() =>
            {
                let inner = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ParenthesizedExpression,
                        field: "expression",
                    })?;
                self.flatten_retained_comma_list(self.node(inner), output)?;
            }
            NodeData::BinaryExpression(data)
                if data
                    .operator_token
                    .map(|node| self.context.arena().node(self.node(node)))
                    .transpose()?
                    .is_some_and(|operator| operator.kind == SyntaxKind::CommaToken) =>
            {
                for child in [data.left, data.right] {
                    let child = child.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::BinaryExpression,
                        field: "comma operand",
                    })?;
                    self.flatten_retained_comma_list(self.node(child), output)?;
                }
            }
            NodeData::CommaListExpression(data) => {
                for child in self.array_nodes(data.elements)? {
                    self.flatten_retained_comma_list(child, output)?;
                }
            }
            _ => output.push(node),
        }
        Ok(())
    }

    fn flatten_retained_comma_elements(
        &self,
        expression: TransformNode,
        output: &mut Vec<TransformNode>,
    ) -> Result<(), TransformError> {
        let record = self.context.arena().node(expression)?;
        // flattenCommaElements, _tsc.js24371-24381. This factory operation
        // expands one level; erased-field pending collection is recursive.
        // Original-node provenance shares our EmitMetadata table, so absence
        // of that entry implies both !original and !emitNode in this domain.
        // The retained built-in pipeline never requests a source lazy node.id
        // for an unanchored comma expression. Its generated-name/cache keys
        // are declarations, binding/property names, or parsed resolver nodes;
        // module/printer ID consumers run after this pass. An arena NodeId
        // must not be mistaken for that separate source identity predicate.
        let unanchored = (record.pos == u32::MAX || record.end == u32::MAX)
            && (record.flags & NodeFlags::SYNTHESIZED.bits() as i32) != 0
            && self.context.arena().metadata(expression).is_none();
        if unanchored {
            match &record.data {
                NodeData::CommaListExpression(data) => {
                    output.extend(self.array_nodes(data.elements)?);
                    return Ok(());
                }
                NodeData::BinaryExpression(data)
                    if data
                        .operator_token
                        .map(|node| self.context.arena().node(self.node(node)))
                        .transpose()?
                        .is_some_and(|operator| operator.kind == SyntaxKind::CommaToken) =>
                {
                    for child in [data.left, data.right] {
                        let child = child.ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::BinaryExpression,
                            field: "comma factory operand",
                        })?;
                        output.push(self.node(child));
                    }
                    return Ok(());
                }
                _ => {}
            }
        }
        output.push(expression);
        Ok(())
    }

    fn inline_expressions(
        &mut self,
        expressions: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if expressions.len() > 10 {
            let mut flattened = Vec::with_capacity(expressions.len());
            for expression in expressions {
                self.flatten_retained_comma_elements(expression, &mut flattened)?;
            }
            let elements = self
                .context
                .factory()?
                .create_node_array(self.source, flattened)?;
            return self.create_class_field_node(
                NodeData::CommaListExpression(tsc_syntax::nodes::CommaListExpressionData {
                    elements: Some(elements.array()),
                }),
                TransformFlags::NONE,
            );
        }
        let mut expressions = expressions.into_iter();
        let mut expression = expressions
            .next()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassExpression,
                field: "inline expressions",
            })?;
        for next in expressions {
            expression = self.create_binary(expression, SyntaxKind::CommaToken, next)?;
        }
        Ok(expression)
    }
}

// A40 staged class/frame/member composition design, not production code.

impl<'context, 'resolver, 'aliases> ClassFieldsVisitor<'context, 'resolver, 'aliases> {
    fn retained_class_was_decorated(&self, class: TransformNode) -> Result<bool, TransformError> {
        let original = self.context.arena().get_original_node(class);
        let (modifiers, members, declaration) = match &self.context.arena().node(original)?.data {
            NodeData::ClassDeclaration(data) => (data.modifiers, data.members, true),
            NodeData::ClassExpression(data) => (data.modifiers, data.members, false),
            _ => return Ok(false),
        };
        if (declaration || !self.legacy_decorators)
            && self.has_modifier(modifiers, SyntaxKind::Decorator)?
        {
            return Ok(true);
        }
        if !declaration || !self.legacy_decorators {
            return Ok(false);
        }
        for member in self.array_nodes(members)? {
            let NodeData::Constructor(data) = &self.context.arena().node(member)?.data else {
                continue;
            };
            if data.body.is_none() {
                continue;
            }
            for parameter in self.array_nodes(data.parameters)? {
                let NodeData::Parameter(data) = &self.context.arena().node(parameter)?.data else {
                    continue;
                };
                if let Some(name) = data.name {
                    if matches!(&self.context.arena().node(self.node(name))?.data,
                        NodeData::Identifier(name) if name.text == "this")
                    {
                        continue;
                    }
                }
                if self.has_modifier(data.modifiers, SyntaxKind::Decorator)? {
                    return Ok(true);
                }
            }
            // Only the first constructor with a body is queried upstream.
            break;
        }
        Ok(false)
    }

    fn visit_class_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassDeclarationData,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let facts = self.retained_class_facts(original, data.members)?;
        let constructor_binding = if facts.needs_constructor_reference {
            Some(self.allocate_binding(RetainedBindingPlacement::Hoisted, true)?)
        } else {
            None
        };
        let pending_reference = if let Some(binding) = &constructor_binding {
            let reference = self.create_binding_identifier(binding)?;
            let internal_name = self.retained_class_name(original, data.name, false, true)?;
            Some(self.create_assignment(reference, internal_name)?)
        } else {
            None
        };
        let constructor_reference = constructor_binding
            .as_ref()
            .map(|binding| self.create_binding_identifier(binding))
            .transpose()?;
        let receiver = self
            .context
            .arena()
            .metadata(original)
            .and_then(|metadata| metadata.class_this)
            .or(constructor_reference)
            .or_else(|| data.name.map(|name| self.node(name)));
        let has_constructor_reference =
            self.retained_resolver_flag(original, NodeCheckFlags::CONTAINS_CONSTRUCTOR_REFERENCE)?;
        let default_export = self.has_modifier(data.modifiers, SyntaxKind::ExportKeyword)?
            && self.has_modifier(data.modifiers, SyntaxKind::DefaultKeyword)?;
        self.class_frames.push(RetainedClassFrame {
            container: original,
            members: data.members,
            facts,
            receiver,
            pending_expressions: Vec::new(),
        });
        let result: Result<_, TransformError> = (|| {
            data.modifiers = self.visit_class_member_modifiers(data.modifiers)?;
            data.heritage_clauses = self.visit_optional_nodes(data.heritage_clauses)?;
            let (members, prologue) = self.transform_retained_members(data.members)?;
            data.members = members;
            data.type_parameters = None;
            let mut pending = std::mem::take(
                &mut self
                    .class_frames
                    .last_mut()
                    .expect("class frame")
                    .pending_expressions,
            );
            if let Some(reference) = pending_reference {
                pending.insert(0, reference);
            }
            let mut postfix = Vec::new();
            if !pending.is_empty() {
                let expression = self.inline_expressions(pending)?;
                postfix.push(self.create_expression_statement(expression)?);
            }
            // addPropertyOrClassStaticBlockStatements skips all static elements
            // when private/static downlevel lowering is false; their retained
            // initializer blocks were produced by the member visitor already.
            if !postfix.is_empty() && default_export {
                data.modifiers = self.filter_modifier(data.modifiers, SyntaxKind::ExportKeyword)?;
                data.modifiers =
                    self.filter_modifier(data.modifiers, SyntaxKind::DefaultKeyword)?;
                let local_name = self.retained_class_name(original, data.name, true, false)?;
                postfix.push(self.create_class_field_node(
                    NodeData::ExportAssignment(tsc_syntax::nodes::ExportAssignmentData {
                        modifiers: None,
                        is_export_equals: Some(false),
                        expression: Some(local_name.node()),
                    }),
                    TransformFlags::NONE,
                )?);
            }
            if has_constructor_reference {
                if let Some(binding) = constructor_binding {
                    let resolver_node = self
                        .context
                        .arena()
                        .require_parse_tree_resolver_node(original)?;
                    self.class_aliases.insert(
                        (resolver_node.source().raw(), resolver_node.node().0),
                        downlevel::ClassBinding::Generated(binding),
                    );
                }
            }
            let declaration =
                self.update_contextual_node(original, NodeData::ClassDeclaration(data))?;
            let mut statements = Vec::with_capacity(postfix.len() + 2);
            if let Some(prologue) = prologue {
                statements.push(self.create_expression_statement(prologue)?);
            }
            statements.push(declaration);
            statements.extend(postfix);
            Ok(statements)
        })();
        self.class_frames
            .pop()
            .expect("class frame remains balanced");
        result
    }

    fn visit_class_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassExpressionData,
    ) -> Result<NodeId, TransformError> {
        let facts = self.retained_class_facts(original, data.members)?;
        let constructor_reference = if facts.needs_constructor_reference {
            let placement = if self
                .retained_resolver_flag(original, NodeCheckFlags::BLOCK_SCOPED_BINDING_IN_LOOP)?
            {
                RetainedBindingPlacement::Iteration
            } else {
                RetainedBindingPlacement::Hoisted
            };
            let binding = self.allocate_binding(placement, true)?;
            Some(self.create_binding_identifier(&binding)?)
        } else {
            None
        };
        let receiver = self
            .context
            .arena()
            .metadata(original)
            .and_then(|metadata| metadata.class_this)
            .or(constructor_reference)
            .or_else(|| data.name.map(|name| self.node(name)));
        self.class_frames.push(RetainedClassFrame {
            container: original,
            members: data.members,
            facts,
            receiver,
            pending_expressions: Vec::new(),
        });
        let result: Result<_, TransformError> = (|| {
            data.modifiers = self.visit_class_member_modifiers(data.modifiers)?;
            data.heritage_clauses = self.visit_optional_nodes(data.heritage_clauses)?;
            let (members, prologue) = self.transform_retained_members(data.members)?;
            data.members = members;
            data.type_parameters = None;
            let class = self.update_contextual_node(original, NodeData::ClassExpression(data))?;
            // transformClassMembers consumes retained pending expressions into
            // its synthetic static block. With the A41 private-static guard
            // false, the source expression assignment/alias branch is unreachable.
            // A required constructor temp can consequently remain unassigned.
            let Some(prologue) = prologue else {
                return Ok(class.node());
            };
            let arena = self.context.arena_mut()?;
            arena.metadata_mut(class).add_flags(EmitFlags::INDENTED);
            arena.metadata_mut(class).set_starts_on_new_line(true);
            arena.metadata_mut(prologue).set_starts_on_new_line(true);
            self.inline_expressions(vec![prologue, class])
                .map(TransformNode::node)
        })();
        self.class_frames
            .pop()
            .expect("class frame remains balanced");
        result
    }

    fn retained_class_name(
        &mut self,
        class: TransformNode,
        name: Option<NodeId>,
        allow_source_maps: bool,
        internal: bool,
    ) -> Result<TransformNode, TransformError> {
        if let Some(name) = name.map(|name| self.node(name)) {
            let generated = self
                .context
                .arena()
                .metadata(name)
                .and_then(crate::EmitMetadata::generated_binding_id)
                .is_some();
            if self.context.arena().node(name)?.kind == SyntaxKind::Identifier && !generated {
                let copy = self.context.factory()?.clone_node(name)?;
                self.context.factory()?.set_text_range(copy, name)?;
                let mut flags = EmitFlags::LOCAL_NAME | EmitFlags::NO_COMMENTS;
                if internal {
                    flags |= EmitFlags::INTERNAL_NAME;
                }
                if !allow_source_maps {
                    flags |= EmitFlags::NO_SOURCE_MAP;
                }
                // The original link supplies parse provenance to the native
                // consumer; cloned syntax does not install a mutable parent.
                self.context
                    .arena_mut()?
                    .metadata_mut(copy)
                    .add_flags(flags);
                return Ok(copy);
            }
        }
        let key = self.context.arena().get_original_node(class).node();
        if let Some(binding) = self.class_internal_names.get(&key).cloned() {
            return self.create_binding_identifier(&binding);
        }
        let provisional = self.generated_names.allocate_numbered("default");
        let binding =
            TargetBinding::allocate_numbered(self.context, "default".to_owned(), provisional)?;
        self.class_internal_names.insert(key, binding.clone());
        self.create_binding_identifier(&binding)
    }

    fn transform_retained_members(
        &mut self,
        members: Option<NodeArrayId>,
    ) -> Result<(Option<NodeArrayId>, Option<TransformNode>), TransformError> {
        self.private_name_scopes
            .push(RetainedPrivateNameScope::default());
        let result: Result<_, TransformError> = (|| {
            self.prepare_retained_private_storage(members)?;
            let mut output = Vec::new();
            for member in self.array_nodes(members)? {
                match self.context.arena().node(member)?.data.clone() {
                    NodeData::PropertyDeclaration(data) => {
                        if self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?
                            && self.should_transform_retained_auto_accessors()
                        {
                            output.extend(self.transform_retained_auto_accessor(member, data)?);
                        } else if let Some(member) = self.transform_retained_field(member)? {
                            output.push(member);
                        }
                    }
                    NodeData::Constructor(_) => {
                        if let Some(constructor) =
                            self.transform_retained_constructor(Some(member))?
                        {
                            output.push(constructor);
                        }
                    }
                    data @ (NodeData::MethodDeclaration(_)
                    | NodeData::GetAccessor(_)
                    | NodeData::SetAccessor(_)) => {
                        let visited = self.visit_class_member_function(member, data)?;
                        output.push(self.node(visited));
                    }
                    NodeData::ClassStaticBlockDeclaration(data) => {
                        let visited = self.visit_static_block(member, data)?;
                        output.push(self.node(visited));
                    }
                    NodeData::Token => output.push(member),
                    data => {
                        let visited = self.update_generic(member, data)?;
                        output.push(self.node(visited));
                    }
                }
            }
            let has_constructor = output.iter().any(|node| {
                self.context
                    .arena()
                    .node(*node)
                    .is_ok_and(|node| node.kind == SyntaxKind::Constructor)
            });
            let constructor = if has_constructor {
                None
            } else {
                self.transform_retained_constructor(None)?
            };
            let pending = std::mem::take(
                &mut self
                    .class_frames
                    .last_mut()
                    .expect("class frame")
                    .pending_expressions,
            );
            let mut prologue = None;
            let static_block = if pending.is_empty() {
                None
            } else {
                let expression = self.inline_expressions(pending)?;
                let mut statement = self.create_expression_statement(expression)?;
                if self.context.arena().transform_flags(statement).bits()
                    & TransformFlags::CONTAINS_LEXICAL_THIS_OR_SUPER.bits()
                    != 0
                {
                    let binding =
                        self.allocate_binding(RetainedBindingPlacement::Hoisted, false)?;
                    let body = self.create_block(vec![statement], false)?;
                    let arrow = self.create_arrow_function(Vec::new(), body)?;
                    let temp = self.create_binding_identifier(&binding)?;
                    prologue = Some(self.create_assignment(temp, arrow)?);
                    let temp = self.create_binding_identifier(&binding)?;
                    let call = self.create_call(temp, Vec::new())?;
                    statement = self.create_expression_statement(call)?;
                }
                let body = self.create_block(vec![statement], false)?;
                Some(self.create_class_field_node(
                    NodeData::ClassStaticBlockDeclaration(
                        tsc_syntax::nodes::ClassStaticBlockDeclarationData {
                            body: Some(body.node()),
                            modifiers: None,
                        },
                    ),
                    TransformFlags::NONE,
                )?)
            };
            let add_leading = constructor.is_some() || static_block.is_some();
            if add_leading {
                let mut class_this = None;
                let mut assigned_name = None;
                for (index, member) in output.iter().copied().enumerate() {
                    if class_this.is_none() && self.retained_class_this_block(member)? {
                        class_this = Some(index);
                    }
                    if assigned_name.is_none() && self.retained_assigned_name_block(member)? {
                        assigned_name = Some(index);
                    }
                }
                let mut ordered = Vec::with_capacity(output.len() + 2);
                if let Some(index) = class_this {
                    ordered.push(output[index]);
                }
                if let Some(index) = assigned_name {
                    ordered.push(output[index]);
                }
                ordered.extend(constructor);
                ordered.extend(static_block);
                for (index, member) in output.into_iter().enumerate() {
                    if Some(index) != class_this && Some(index) != assigned_name {
                        ordered.push(member);
                    }
                }
                output = ordered;
            }
            let array = if let Some(members) = members {
                let members = self.array(members);
                if add_leading {
                    let source_range = self.context.arena().node_array(members)?;
                    let (pos, end) = (source_range.pos, source_range.end);
                    let array = self
                        .context
                        .factory()?
                        .create_node_array(self.source, output)?;
                    self.context
                        .factory()?
                        .set_node_array_text_range(array, pos, end)?;
                    array
                } else {
                    self.context.factory()?.update_node_array(members, output)?
                }
            } else {
                self.context
                    .factory()?
                    .create_node_array(self.source, output)?
            };
            Ok((Some(array.array()), prologue))
        })();
        self.private_name_scopes
            .pop()
            .expect("private name scope remains balanced");
        result
    }

    fn retained_static_block_expression(
        &self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let NodeData::ClassStaticBlockDeclaration(data) = &self.context.arena().node(node)?.data
        else {
            return Ok(None);
        };
        let Some(body) = data.body else {
            return Ok(None);
        };
        let NodeData::Block(data) = &self.context.arena().node(self.node(body))?.data else {
            return Ok(None);
        };
        let statements = self.array_nodes(data.statements)?;
        if statements.len() != 1 {
            return Ok(None);
        }
        let NodeData::ExpressionStatement(data) = &self.context.arena().node(statements[0])?.data
        else {
            return Ok(None);
        };
        Ok(data.expression.map(|node| self.node(node)))
    }

    fn retained_class_this_block(&self, node: TransformNode) -> Result<bool, TransformError> {
        let Some(expression) = self.retained_static_block_expression(node)? else {
            return Ok(false);
        };
        let NodeData::BinaryExpression(data) = &self.context.arena().node(expression)?.data else {
            return Ok(false);
        };
        let (Some(operator), Some(left), Some(right)) =
            (data.operator_token, data.left, data.right)
        else {
            return Ok(false);
        };
        let left = self.node(left);
        Ok(
            self.context.arena().node(self.node(operator))?.kind == SyntaxKind::EqualsToken
                && self.context.arena().node(left)?.kind == SyntaxKind::Identifier
                && self.context.arena().node(self.node(right))?.kind == SyntaxKind::ThisKeyword
                && self
                    .context
                    .arena()
                    .metadata(node)
                    .and_then(|metadata| metadata.class_this)
                    == Some(left),
        )
    }

    fn retained_assigned_name_block(&self, node: TransformNode) -> Result<bool, TransformError> {
        let Some(expression) = self.retained_static_block_expression(node)? else {
            return Ok(false);
        };
        if !self
            .context
            .arena()
            .is_call_to_emit_helper(expression, crate::factory::EmitHelperName::SetFunctionName)?
        {
            return Ok(false);
        }
        let NodeData::CallExpression(data) = &self.context.arena().node(expression)?.data else {
            return Ok(false);
        };
        let arguments = self.array_nodes(data.arguments)?;
        Ok(arguments.get(1).copied().is_some_and(|assigned| {
            self.context
                .arena()
                .metadata(node)
                .and_then(|metadata| metadata.assigned_name)
                == Some(assigned)
        }))
    }
}

// A40 staged design only. Requires the complete class/property owner draft.
// Ordinary constructor visitation and field initialization are separate
// lexical phases, including the second visitation of existing body statements.

impl<'context, 'resolver, 'aliases> ClassFieldsVisitor<'context, 'resolver, 'aliases> {
    fn transform_retained_constructor(
        &mut self,
        constructor: Option<TransformNode>,
    ) -> Result<Option<TransformNode>, TransformError> {
        let constructor = self
            .visit_optional_node(constructor.map(TransformNode::node))?
            .map(|node| self.node(node));
        let frame = self
            .class_frames
            .last()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Constructor,
                field: "class environment",
            })?;
        if !frame.facts.will_hoist_initializers {
            return Ok(constructor);
        }
        let container = frame.container;
        let source_members = frame.members;
        let derived = self.retained_class_is_derived(container)?;
        let existing_data = constructor
            .map(|node| {
                let NodeData::Constructor(data) = self.context.arena().node(node)?.data.clone()
                else {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::Constructor,
                        field: "constructor data",
                    });
                };
                Ok(data)
            })
            .transpose()?;
        self.context.start_lexical_environment()?;
        let (previous, scope) = self
            .generated_names
            .enter(GeneratedBindingOwner::FunctionBody);
        let result: Result<_, TransformError> = (|| {
            let parameters = self.visit_retained_parameters(
                existing_data.as_ref().and_then(|data| data.parameters),
            )?;
            self.context.resume_lexical_environment()?;
            let initializers = self.retained_instance_initializers(source_members, constructor)?;
            let old_body = existing_data
                .as_ref()
                .and_then(|data| data.body)
                .map(|node| self.node(node));
            let (old_statements, mut statements) = if let Some(body) = old_body {
                let NodeData::Block(data) = &self.context.arena().node(body)?.data else {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::Constructor,
                        field: "body block",
                    });
                };
                (data.statements, self.array_nodes(data.statements)?)
            } else {
                (None, Vec::new())
            };
            if old_body.is_some() {
                let input = std::mem::take(&mut statements);
                let mut offset = 0;
                while offset < input.len() && self.prologue_text(input[offset])?.is_some() {
                    statements.push(input[offset]);
                    offset += 1;
                }
                while offset < input.len() && self.is_custom_prologue(input[offset]) {
                    if let Some(statement) = self.visit(input[offset].node())? {
                        statements.push(self.node(statement));
                    }
                    offset += 1;
                }
                if let Some(path) = self.find_super_statement_path(&input, offset)? {
                    self.visit_constructor_super_path(
                        &mut statements,
                        &input,
                        offset,
                        &path.0,
                        &initializers,
                        constructor,
                    )?;
                } else {
                    while offset < input.len()
                        && self.is_retained_parameter_property(input[offset], constructor)?
                    {
                        offset += 1;
                    }
                    statements.extend(initializers);
                    self.visit_statement_slice(&mut statements, &input[offset..])?;
                }
            } else {
                if constructor.is_none() && derived {
                    let arguments = self.create_identifier("arguments")?;
                    let spread = self.create_class_field_node(
                        NodeData::SpreadElement(tsc_syntax::nodes::SpreadElementData {
                            expression: Some(arguments.node()),
                        }),
                        TransformFlags::NONE,
                    )?;
                    let super_token = self.context.factory()?.create_token(
                        self.source,
                        SyntaxKind::SuperKeyword,
                        TransformFlags::CONTAINS_LEXICAL_SUPER,
                    )?;
                    let call = self.create_call(super_token, vec![spread])?;
                    statements.push(self.create_expression_statement(call)?);
                }
                statements.extend(initializers);
            }
            Ok((parameters, old_body, old_statements, statements))
        })();
        let environment = self.context.end_lexical_environment();
        self.generated_names.exit(previous, scope);
        let (parameters, old_body, old_statements, mut statements) = result?;
        self.merge_lexical_environment(&mut statements, environment?)?;
        if statements.is_empty() && constructor.is_none() {
            return Ok(None);
        }
        let multiline = if let Some(body) = old_body {
            let old_count = self.array_nodes(old_statements)?.len();
            if old_count >= statements.len() {
                self.context
                    .arena()
                    .node(body)?
                    .multi_line
                    .unwrap_or(!statements.is_empty())
            } else {
                !statements.is_empty()
            }
        } else {
            !statements.is_empty()
        };
        let body = self.create_block(statements, multiline)?;
        let statement_array = match &self.context.arena().node(body)?.data {
            NodeData::Block(data) => data.statements,
            _ => None,
        }
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::Block,
            field: "statements",
        })?;
        if let Some(range) = old_statements.or(source_members) {
            let range = self.context.arena().node_array(self.array(range))?;
            let (pos, end) = (range.pos, range.end);
            let statement_array = self.array(statement_array);
            self.context
                .factory()?
                .set_node_array_text_range(statement_array, pos, end)?;
        }
        if let Some(old_body) = old_body {
            self.context.factory()?.set_text_range(body, old_body)?;
        }
        if let (Some(constructor), Some(mut data)) = (constructor, existing_data) {
            data.modifiers = None;
            data.parameters = parameters;
            data.body = Some(body.node());
            return self
                .update_contextual_node(constructor, NodeData::Constructor(data))
                .map(Some);
        }
        let parameters = match parameters {
            Some(parameters) => self.array(parameters),
            None => self
                .context
                .factory()?
                .create_node_array(self.source, Vec::new())?,
        };
        let node = self.create_class_field_node(
            NodeData::Constructor(tsc_syntax::nodes::ConstructorData {
                name: None,
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers: None,
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.set_text_range(node, container)?;
        let metadata = self.context.arena_mut()?.metadata_mut(node);
        metadata.set_starts_on_new_line(true);
        // The container range positions the generated constructor without
        // transferring the class boundary's comments to that member.
        metadata.set_comment_range(CommentRange::new(self.source, SourceRange::Synthesized));
        Ok(Some(node))
    }

    fn retained_instance_initializers(
        &mut self,
        members: Option<NodeArrayId>,
        constructor: Option<TransformNode>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut parameter_properties = Vec::new();
        let mut properties = Vec::new();
        for member in self.array_nodes(members)? {
            let NodeData::PropertyDeclaration(data) = &self.context.arena().node(member)?.data
            else {
                continue;
            };
            if self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)? {
                continue;
            }
            // Parameter properties are selected from unfiltered instance
            // fields, separately from ordinary initialized/private/accessor fields.
            if constructor.is_some() && self.is_retained_parameter_property(member, constructor)? {
                parameter_properties.push(member);
            } else if self.use_define_for_class_fields
                || data.initializer.is_some()
                || self.name_is_private(data.name)?
                || self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?
            {
                properties.push(member);
            }
        }
        parameter_properties.extend(properties);
        let mut statements = Vec::with_capacity(parameter_properties.len());
        for property in parameter_properties {
            if let Some(statement) = self.retained_property_initializer(property)? {
                statements.push(statement);
            }
        }
        Ok(statements)
    }

    fn is_retained_parameter_property(
        &self,
        node: TransformNode,
        constructor: Option<TransformNode>,
    ) -> Result<bool, TransformError> {
        if constructor.is_none() {
            return Ok(false);
        }
        let original = self.context.arena().get_original_node(node);
        let NodeData::Parameter(data) = &self.context.arena().node(original)?.data else {
            return Ok(false);
        };
        for modifier in self.array_nodes(data.modifiers)? {
            if matches!(
                self.context.arena().node(modifier)?.kind,
                SyntaxKind::PublicKeyword
                    | SyntaxKind::PrivateKeyword
                    | SyntaxKind::ProtectedKeyword
                    | SyntaxKind::ReadonlyKeyword
                    | SyntaxKind::OverrideKeyword
            ) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn visit_statement_slice(
        &mut self,
        output: &mut Vec<TransformNode>,
        input: &[TransformNode],
    ) -> Result<(), TransformError> {
        for statement in input {
            match self.visit_outcome(*statement)? {
                RetainedVisitOutcome::One(statement) => output.push(statement),
                RetainedVisitOutcome::Many(statements) => output.extend(statements),
            }
        }
        Ok(())
    }

    fn visit_constructor_super_path(
        &mut self,
        output: &mut Vec<TransformNode>,
        input: &[TransformNode],
        offset: usize,
        path: &[usize],
        initializers: &[TransformNode],
        constructor: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let (&index, remaining) =
            path.split_first()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Constructor,
                    field: "super path",
                })?;
        let statement = *input
            .get(index)
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Constructor,
                field: "super statement",
            })?;
        self.visit_statement_slice(output, &input[offset..index])?;
        let mut offset = index + 1;
        if let NodeData::TryStatement(mut data) = self.context.arena().node(statement)?.data.clone()
        {
            let block = data.try_block.map(|node| self.node(node)).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::TryStatement,
                    field: "try_block",
                },
            )?;
            let NodeData::Block(mut block_data) = self.context.arena().node(block)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::TryStatement,
                    field: "block data",
                });
            };
            let input = self.array_nodes(block_data.statements)?;
            let mut nested = Vec::new();
            self.visit_constructor_super_path(
                &mut nested,
                &input,
                0,
                remaining,
                initializers,
                constructor,
            )?;
            // Upstream makes a ranged unused NodeArray, then passes the plain
            // array to updateBlock. The actual updated block gets a new array.
            let nested = self
                .context
                .factory()?
                .create_node_array(self.source, nested)?;
            block_data.statements = Some(nested.array());
            data.try_block = Some(
                self.update_contextual_node(block, NodeData::Block(block_data))?
                    .node(),
            );
            data.catch_clause = self.visit_optional_node(data.catch_clause)?;
            data.finally_block = self.visit_optional_node(data.finally_block)?;
            output.push(self.update_contextual_node(statement, NodeData::TryStatement(data))?);
        } else {
            self.visit_statement_slice(output, &input[index..index + 1])?;
            while offset < input.len()
                && self.is_retained_parameter_property(input[offset], constructor)?
            {
                offset += 1;
            }
            output.extend_from_slice(initializers);
        }
        self.visit_statement_slice(output, &input[offset..])
    }

    fn retained_class_is_derived(&self, class: TransformNode) -> Result<bool, TransformError> {
        let clauses = match &self.context.arena().node(class)?.data {
            NodeData::ClassDeclaration(data) => data.heritage_clauses,
            NodeData::ClassExpression(data) => data.heritage_clauses,
            _ => return Ok(false),
        };
        for clause in self.array_nodes(clauses)? {
            let NodeData::HeritageClause(data) = &self.context.arena().node(clause)?.data else {
                continue;
            };
            if data.token != SyntaxKind::ExtendsKeyword {
                continue;
            }
            let Some(base) = self.array_nodes(data.types)?.first().copied() else {
                return Ok(false);
            };
            let NodeData::ExpressionWithTypeArguments(data) =
                &self.context.arena().node(base)?.data
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::HeritageClause,
                    field: "base expression",
                });
            };
            let expression = data.expression.map(|node| self.node(node)).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ExpressionWithTypeArguments,
                    field: "expression",
                },
            )?;
            let expression = self.skip_retained_outer_expressions(expression, false)?;
            return Ok(self.context.arena().node(expression)?.kind != SyntaxKind::NullKeyword);
        }
        Ok(false)
    }
}

// A40 staged Rust design only. Not included by the production module and not
// implementation-ready until the complete class/member design and gate exist.

#[derive(Clone, Copy)]
enum RetainedBindingPlacement {
    Hoisted,
    Iteration,
    Parameter,
}

#[derive(Clone, Copy)]
enum RetainedFunctionNameVisitor {
    Ordinary,
    ClassElement,
}

impl<'context, 'resolver, 'aliases> ClassFieldsVisitor<'context, 'resolver, 'aliases> {
    fn create_binding_identifier(
        &mut self,
        binding: &TargetBinding,
    ) -> Result<TransformNode, TransformError> {
        let text = binding.printable_text(self.context).to_owned();
        let identifier = self.create_identifier(&text)?;
        binding.write_generated_metadata(self.context.arena_mut()?, identifier);
        Ok(identifier)
    }

    fn allocate_binding(
        &mut self,
        placement: RetainedBindingPlacement,
        reserve_in_nested_scopes: bool,
    ) -> Result<TargetBinding, TransformError> {
        let provisional = match placement {
            RetainedBindingPlacement::Parameter => self.generated_names.allocate_local_temp(),
            _ => self.generated_names.allocate_temp(),
        };
        let binding = if reserve_in_nested_scopes {
            TargetBinding::allocate_reserved_in_nested_scopes(self.context, provisional)?
        } else {
            TargetBinding::allocate(self.context, provisional)?
        };
        match placement {
            RetainedBindingPlacement::Hoisted => {
                let name = self.create_binding_identifier(&binding)?;
                self.context.hoist_variable_declaration(name)?;
            }
            RetainedBindingPlacement::Iteration => {
                let name = self.create_binding_identifier(&binding)?;
                self.context.add_block_scoped_variable(name)?;
            }
            RetainedBindingPlacement::Parameter => {}
        }
        Ok(binding)
    }

    fn create_variable_declaration(
        &mut self,
        name: TransformNode,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.create_class_field_node(
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: initializer.map(TransformNode::node),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_variable_statement(
        &mut self,
        declarations: Vec<TransformNode>,
        flags: NodeFlags,
    ) -> Result<TransformNode, TransformError> {
        let declarations = self
            .context
            .factory()?
            .create_node_array(self.source, declarations)?;
        let list = self.create_class_field_node(
            NodeData::VariableDeclarationList(tsc_syntax::nodes::VariableDeclarationListData {
                declarations: Some(declarations.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context
            .factory()?
            .set_node_flags(list, NodeFlags::SYNTHESIZED | flags)?;
        self.complete_created_node_flags(list)?;
        self.create_class_field_node(
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers: None,
                declaration_list: Some(list.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn materialize_lexical_environment(
        &mut self,
        environment: LexicalEnvironment,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut statements = environment.function_declarations().to_vec();
        if !environment.variable_declarations().is_empty() {
            let mut declarations = Vec::new();
            for name in environment.variable_declarations().iter().copied() {
                let declaration = self.create_variable_declaration(name, None)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(declaration)
                    .add_flags(EmitFlags::NO_NESTED_SOURCE_MAPS);
                declarations.push(declaration);
            }
            let statement = self.create_variable_statement(declarations, NodeFlags::NONE)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::CUSTOM_PROLOGUE);
            statements.push(statement);
        }
        statements.extend_from_slice(environment.initialization_statements());
        Ok(statements)
    }

    fn is_custom_prologue(&self, node: TransformNode) -> bool {
        self.context
            .arena()
            .metadata(node)
            .is_some_and(|metadata| metadata.flags().contains(EmitFlags::CUSTOM_PROLOGUE))
    }

    fn is_hoisted_function(&self, node: TransformNode) -> Result<bool, TransformError> {
        Ok(self.is_custom_prologue(node)
            && matches!(
                self.context.arena().node(node)?.data,
                NodeData::FunctionDeclaration(_)
            ))
    }

    fn is_hoisted_variable_statement(&self, node: TransformNode) -> Result<bool, TransformError> {
        if !self.is_custom_prologue(node) {
            return Ok(false);
        }
        let NodeData::VariableStatement(statement) = &self.context.arena().node(node)?.data else {
            return Ok(false);
        };
        let Some(list) = statement.declaration_list else {
            return Ok(false);
        };
        let NodeData::VariableDeclarationList(list) =
            &self.context.arena().node(self.node(list))?.data
        else {
            return Ok(false);
        };
        for declaration in self.array_nodes(list.declarations)? {
            let NodeData::VariableDeclaration(data) = &self.context.arena().node(declaration)?.data
            else {
                return Ok(false);
            };
            if data.initializer.is_some() {
                return Ok(false);
            }
            let Some(name) = data.name else {
                return Ok(false);
            };
            if !matches!(
                self.context.arena().node(self.node(name))?.data,
                NodeData::Identifier(_)
            ) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn prologue_text(&self, node: TransformNode) -> Result<Option<String>, TransformError> {
        let NodeData::ExpressionStatement(data) = &self.context.arena().node(node)?.data else {
            return Ok(None);
        };
        let Some(expression) = data.expression else {
            return Ok(None);
        };
        let record = self.context.arena().node(self.node(expression))?;
        if record.kind != SyntaxKind::StringLiteral {
            return Ok(None);
        }
        // The syntax payload of all literal-like expressions is shared.
        let NodeData::StringLiteral(data) = &record.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ExpressionStatement,
                field: "string literal prologue payload",
            });
        };
        Ok(Some(data.text.clone()))
    }

    fn find_prologue_span_end<F>(
        &self,
        statements: &[TransformNode],
        start: usize,
        test: F,
    ) -> Result<usize, TransformError>
    where
        F: Fn(&Self, TransformNode) -> Result<bool, TransformError>,
    {
        let mut end = start;
        while end < statements.len() && test(self, statements[end])? {
            end += 1;
        }
        Ok(end)
    }

    fn merge_lexical_environment(
        &mut self,
        statements: &mut Vec<TransformNode>,
        environment: LexicalEnvironment,
    ) -> Result<(), TransformError> {
        let declarations = self.materialize_lexical_environment(environment)?;
        if declarations.is_empty() {
            return Ok(());
        }
        let left_directives =
            self.find_prologue_span_end(statements, 0, |v, n| Ok(v.prologue_text(n)?.is_some()))?;
        let left_functions =
            self.find_prologue_span_end(statements, left_directives, Self::is_hoisted_function)?;
        let left_variables = self.find_prologue_span_end(
            statements,
            left_functions,
            Self::is_hoisted_variable_statement,
        )?;
        let right_directives = self
            .find_prologue_span_end(&declarations, 0, |v, n| Ok(v.prologue_text(n)?.is_some()))?;
        let right_functions = self.find_prologue_span_end(
            &declarations,
            right_directives,
            Self::is_hoisted_function,
        )?;
        let right_variables = self.find_prologue_span_end(
            &declarations,
            right_functions,
            Self::is_hoisted_variable_statement,
        )?;
        let right_custom =
            self.find_prologue_span_end(&declarations, right_variables, |v, n| {
                Ok(v.is_custom_prologue(n))
            })?;
        if right_custom != declarations.len() {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SourceFile,
                field: "lexical declarations must be standard or custom prologues",
            });
        }
        let existing_directives = statements[..left_directives]
            .iter()
            .map(|node| self.prologue_text(*node))
            .collect::<Result<BTreeSet<_>, _>>()?;
        statements.splice(
            left_variables..left_variables,
            declarations[right_variables..right_custom].iter().copied(),
        );
        statements.splice(
            left_functions..left_functions,
            declarations[right_functions..right_variables]
                .iter()
                .copied(),
        );
        statements.splice(
            left_directives..left_directives,
            declarations[right_directives..right_functions]
                .iter()
                .copied(),
        );
        if left_directives == 0 {
            statements.splice(0..0, declarations[..right_directives].iter().copied());
        } else {
            for declaration in declarations[..right_directives].iter().rev() {
                if !existing_directives.contains(&self.prologue_text(*declaration)?) {
                    statements.insert(0, *declaration);
                }
            }
        }
        Ok(())
    }

    fn lower_parameter_defaults(
        &mut self,
        parameters: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(parameters) = parameters else {
            return Ok(None);
        };
        let original = self.array(parameters);
        let nodes = self.context.arena().node_array(original)?.nodes.clone();
        let mut output = Vec::with_capacity(nodes.len());
        for parameter in nodes {
            output.push(self.lower_parameter_default(self.node(parameter))?);
        }
        Ok(Some(
            self.context
                .factory()?
                .update_node_array(original, output)?
                .array(),
        ))
    }

    fn lower_parameter_default(
        &mut self,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::Parameter(mut data) = self.context.arena().node(original)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Parameter,
                field: "parameter data",
            });
        };
        if data.dot_dot_dot_token.is_some() {
            return Ok(original);
        }
        let name =
            data.name
                .map(|id| self.node(id))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Parameter,
                    field: "name",
                })?;
        if matches!(
            self.context.arena().node(name)?.kind,
            SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
        ) {
            let binding = self.allocate_binding(RetainedBindingPlacement::Parameter, false)?;
            let condition_name = self.create_binding_identifier(&binding)?;
            let fallback_name = self.create_binding_identifier(&binding)?;
            let value = if let Some(initializer) = data.initializer {
                let undefined = self.create_void_zero()?;
                let condition = self.create_binary(
                    condition_name,
                    SyntaxKind::EqualsEqualsEqualsToken,
                    undefined,
                )?;
                self.create_conditional(condition, self.node(initializer), fallback_name)?
            } else {
                fallback_name
            };
            let declaration = self.create_variable_declaration(name, Some(value))?;
            let statement = self.create_variable_statement(vec![declaration], NodeFlags::NONE)?;
            self.context.add_initialization_statement(statement)?;
            data.name = Some(self.create_binding_identifier(&binding)?.node());
            data.initializer = None;
        } else if let Some(initializer) = data.initializer.map(|id| self.node(id)) {
            let condition_name = self.context.factory()?.clone_node(name)?;
            let undefined = self.create_void_zero()?;
            let condition = self.create_binary(
                condition_name,
                SyntaxKind::EqualsEqualsEqualsToken,
                undefined,
            )?;
            let assignment_name = self.context.factory()?.clone_node(name)?;
            self.context
                .arena_mut()?
                .metadata_mut(assignment_name)
                .set_flags(EmitFlags::NO_SOURCE_MAP);
            self.context
                .arena_mut()?
                .metadata_mut(initializer)
                .add_flags(EmitFlags::NO_SOURCE_MAP | EmitFlags::NO_COMMENTS);
            let assignment = self.create_assignment(assignment_name, initializer)?;
            self.context
                .factory()?
                .set_text_range(assignment, original)?;
            self.context
                .arena_mut()?
                .metadata_mut(assignment)
                .set_flags(EmitFlags::NO_COMMENTS);
            let statement = self.create_expression_statement(assignment)?;
            let block = self.create_block(vec![statement], false)?;
            self.context.factory()?.set_text_range(block, original)?;
            self.context.arena_mut()?.metadata_mut(block).set_flags(
                EmitFlags::SINGLE_LINE
                    | EmitFlags::NO_TRAILING_SOURCE_MAP
                    | EmitFlags::NO_TOKEN_SOURCE_MAPS
                    | EmitFlags::NO_COMMENTS,
            );
            let statement = self.create_class_field_node(
                NodeData::IfStatement(tsc_syntax::nodes::IfStatementData {
                    expression: Some(condition.node()),
                    then_statement: Some(block.node()),
                    else_statement: None,
                }),
                TransformFlags::NONE,
            )?;
            self.context.add_initialization_statement(statement)?;
            data.initializer = None;
        }
        self.update_contextual_node(original, NodeData::Parameter(data))
    }

    fn install_function_environment(
        &mut self,
        body: Option<NodeId>,
        environment: LexicalEnvironment,
    ) -> Result<Option<NodeId>, TransformError> {
        if environment.variable_declarations().is_empty()
            && environment.function_declarations().is_empty()
            && environment.initialization_statements().is_empty()
        {
            return Ok(body);
        }
        let Some(original) = body.map(|id| self.node(id)) else {
            let declarations = self.materialize_lexical_environment(environment)?;
            return Ok(Some(self.create_block(declarations, false)?.node()));
        };
        let block = if matches!(
            self.context.arena().node(original)?.data,
            NodeData::Block(_)
        ) {
            original
        } else {
            let statement = self.create_class_field_node(
                NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                    expression: Some(original.node()),
                }),
                TransformFlags::NONE,
            )?;
            self.context
                .factory()?
                .set_text_range(statement, original)?;
            let block = self.create_block(vec![statement], false)?;
            self.context.factory()?.set_text_range(block, original)?;
            block
        };
        let NodeData::Block(mut data) = self.context.arena().node(block)?.data.clone() else {
            unreachable!("function conversion creates a block")
        };
        let mut statements = self.array_nodes(data.statements)?;
        self.merge_lexical_environment(&mut statements, environment)?;
        let statements = if let Some(original) = data.statements {
            let original = self.array(original);
            self.context
                .factory()?
                .update_node_array(original, statements)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
        };
        data.statements = Some(statements.array());
        Ok(Some(
            self.update_contextual_node(block, NodeData::Block(data))?
                .node(),
        ))
    }
}

impl<'context, 'resolver, 'aliases> ClassFieldsVisitor<'context, 'resolver, 'aliases> {
    fn update_contextual_node(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<TransformNode, TransformError> {
        let flags = super::flags_after_update(self.context.arena(), original, &data)?;
        self.context.factory()?.update_node(original, data, flags)
    }

    fn create_void_zero(&mut self) -> Result<TransformNode, TransformError> {
        let zero = self.create_class_field_node(
            NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                text: "0".to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        self.create_class_field_node(
            NodeData::VoidExpression(tsc_syntax::nodes::VoidExpressionData {
                expression: Some(zero.node()),
            }),
            TransformFlags::NONE,
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
        self.create_class_field_node(
            NodeData::ConditionalExpression(tsc_syntax::nodes::ConditionalExpressionData {
                condition: Some(condition.node()),
                question_token: Some(question.node()),
                when_true: Some(when_true.node()),
                colon_token: Some(colon.node()),
                when_false: Some(when_false.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn visit_function_parts(
        &mut self,
        parameters: Option<NodeArrayId>,
        return_type: Option<NodeId>,
        body: Option<NodeId>,
        owner: GeneratedBindingOwner,
    ) -> Result<(Option<NodeArrayId>, Option<NodeId>, Option<NodeId>), TransformError> {
        self.context.start_lexical_environment()?;
        let (previous, scope) = self.generated_names.enter(owner);
        let result: Result<_, TransformError> = (|| {
            let parameters = self.visit_retained_parameters(parameters)?;
            let return_type = self.visit_optional_node(return_type);
            let resumed = self.context.resume_lexical_environment();
            let return_type = return_type?;
            resumed?;
            let body = self.visit_optional_node(body)?;
            Ok((parameters, return_type, body))
        })();
        let environment = self.context.end_lexical_environment();
        self.generated_names.exit(previous, scope);
        let (parameters, return_type, body) = result?;
        let body = self.install_function_environment(body, environment?)?;
        Ok((parameters, return_type, body))
    }

    fn visit_retained_parameters(
        &mut self,
        parameters: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let mut parameters = parameters;
        if parameters.is_some() {
            self.context
                .set_lexical_environment_flags(LexicalEnvironmentFlags::IN_PARAMETERS, true)?;
            parameters = self.visit_optional_nodes(parameters)?;
            if self
                .context
                .lexical_environment_flags()
                .contains(LexicalEnvironmentFlags::VARIABLES_HOISTED_IN_PARAMETERS)
            {
                parameters = self.lower_parameter_defaults(parameters)?;
            }
            self.context
                .set_lexical_environment_flags(LexicalEnvironmentFlags::IN_PARAMETERS, false)?;
        }
        self.context.suspend_lexical_environment()?;
        Ok(parameters)
    }

    fn visit_function(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<NodeId, TransformError> {
        self.visit_function_with_name_visitor(original, data, RetainedFunctionNameVisitor::Ordinary)
    }

    fn visit_class_member_function(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<NodeId, TransformError> {
        self.visit_function_with_name_visitor(
            original,
            data,
            RetainedFunctionNameVisitor::ClassElement,
        )
    }

    fn visit_function_header(
        &mut self,
        modifiers: Option<NodeArrayId>,
        name: Option<NodeId>,
        visitor: RetainedFunctionNameVisitor,
    ) -> Result<(Option<NodeArrayId>, Option<NodeId>), TransformError> {
        match visitor {
            RetainedFunctionNameVisitor::Ordinary => {
                let modifiers = self.visit_optional_nodes(modifiers)?;
                let name = self.visit_optional_node(name)?;
                Ok((modifiers, name))
            }
            RetainedFunctionNameVisitor::ClassElement => {
                let modifiers = self.visit_class_member_modifiers(modifiers)?;
                let name = self.visit_class_member_name(name)?;
                Ok((modifiers, name))
            }
        }
    }

    fn visit_function_with_name_visitor(
        &mut self,
        original: TransformNode,
        data: NodeData,
        name_visitor: RetainedFunctionNameVisitor,
    ) -> Result<NodeId, TransformError> {
        let data = match data {
            NodeData::FunctionDeclaration(mut data) => {
                data.modifiers = self.visit_optional_nodes(data.modifiers)?;
                data.name = self.visit_optional_node(data.name)?;
                data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
                let (parameters, return_type, body) = self.visit_function_parts(
                    data.parameters,
                    data.r#type,
                    data.body,
                    GeneratedBindingOwner::FunctionBody,
                )?;
                data.parameters = parameters;
                data.body = body;
                data.r#type = return_type;
                NodeData::FunctionDeclaration(data)
            }
            NodeData::FunctionExpression(mut data) => {
                data.modifiers = self.visit_optional_nodes(data.modifiers)?;
                data.name = self.visit_optional_node(data.name)?;
                data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
                let (parameters, return_type, body) = self.visit_function_parts(
                    data.parameters,
                    data.r#type,
                    data.body,
                    GeneratedBindingOwner::FunctionBody,
                )?;
                data.parameters = parameters;
                data.body = body;
                data.r#type = return_type;
                NodeData::FunctionExpression(data)
            }
            NodeData::ArrowFunction(mut data) => {
                data.modifiers = self.visit_optional_nodes(data.modifiers)?;
                data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
                let (parameters, return_type, body) = self.visit_function_parts(
                    data.parameters,
                    data.r#type,
                    data.body,
                    GeneratedBindingOwner::FunctionBody,
                )?;
                data.parameters = parameters;
                data.body = body;
                data.r#type = return_type;
                NodeData::ArrowFunction(data)
            }
            NodeData::MethodDeclaration(mut data) => {
                (data.modifiers, data.name) =
                    self.visit_function_header(data.modifiers, data.name, name_visitor)?;
                data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
                let (parameters, return_type, body) = self.visit_function_parts(
                    data.parameters,
                    data.r#type,
                    data.body,
                    GeneratedBindingOwner::FunctionBody,
                )?;
                data.parameters = parameters;
                data.body = body;
                data.r#type = return_type;
                NodeData::MethodDeclaration(data)
            }
            NodeData::GetAccessor(mut data) => {
                (data.modifiers, data.name) =
                    self.visit_function_header(data.modifiers, data.name, name_visitor)?;
                let (parameters, return_type, body) = self.visit_function_parts(
                    data.parameters,
                    data.r#type,
                    data.body,
                    GeneratedBindingOwner::FunctionBody,
                )?;
                data.parameters = parameters;
                data.body = body;
                data.r#type = return_type;
                NodeData::GetAccessor(data)
            }
            NodeData::SetAccessor(mut data) => {
                (data.modifiers, data.name) =
                    self.visit_function_header(data.modifiers, data.name, name_visitor)?;
                let (parameters, return_type, body) = self.visit_function_parts(
                    data.parameters,
                    None,
                    data.body,
                    GeneratedBindingOwner::FunctionBody,
                )?;
                data.parameters = parameters;
                data.body = body;
                debug_assert!(return_type.is_none());
                NodeData::SetAccessor(data)
            }
            NodeData::Constructor(mut data) => {
                data.modifiers = self.visit_optional_nodes(data.modifiers)?;
                let (parameters, return_type, body) = self.visit_function_parts(
                    data.parameters,
                    None,
                    data.body,
                    GeneratedBindingOwner::FunctionBody,
                )?;
                data.parameters = parameters;
                data.body = body;
                debug_assert!(return_type.is_none());
                NodeData::Constructor(data)
            }
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: self.context.arena().node(original)?.kind,
                    field: "function data",
                })
            }
        };
        Ok(self.update_contextual_node(original, data)?.node())
    }

    fn visit_static_block(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassStaticBlockDeclarationData,
    ) -> Result<NodeId, TransformError> {
        let (parameters, return_type, body) = self.visit_function_parts(
            None,
            None,
            data.body,
            GeneratedBindingOwner::StaticEvaluation,
        )?;
        debug_assert!(parameters.is_none() && return_type.is_none());
        data.body = body;
        Ok(self
            .update_contextual_node(original, NodeData::ClassStaticBlockDeclaration(data))?
            .node())
    }

    fn visit_iteration_body(
        &mut self,
        body: Option<NodeId>,
        parent: SyntaxKind,
    ) -> Result<Option<NodeId>, TransformError> {
        let Some(body) = body else {
            return Ok(None);
        };
        self.context.start_block_scope()?;
        let result = self.visit_statement_lifted(self.node(body));
        let names = self.context.end_block_scope();
        let visited = result?.ok_or(TransformError::RequiredChildRemoved {
            parent,
            field: "statement",
        })?;
        let names = names?;
        if names.is_empty() {
            return Ok(Some(visited.node()));
        }
        let mut declarations = Vec::with_capacity(names.len());
        for name in names {
            declarations.push(self.create_variable_declaration(name, None)?);
        }
        let statement = self.create_variable_statement(declarations, NodeFlags::LET)?;
        let record = self.context.arena().node(visited)?.data.clone();
        let block = if let NodeData::Block(mut data) = record {
            let mut statements = vec![statement];
            statements.extend(self.array_nodes(data.statements)?);
            // Source visitIterationBody supplies a new array when adding lets.
            data.statements = Some(
                self.context
                    .factory()?
                    .create_node_array(self.source, statements)?
                    .array(),
            );
            self.update_contextual_node(visited, NodeData::Block(data))?
        } else {
            self.create_block(vec![statement, visited], false)?
        };
        Ok(Some(block.node()))
    }

    fn visit_iteration(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<NodeId, TransformError> {
        let data = match data {
            NodeData::ForStatement(mut data) => {
                data.initializer = self.visit_optional_node(data.initializer)?;
                data.condition = self.visit_optional_node(data.condition)?;
                data.incrementor = self.visit_optional_node(data.incrementor)?;
                data.statement =
                    self.visit_iteration_body(data.statement, SyntaxKind::ForStatement)?;
                NodeData::ForStatement(data)
            }
            NodeData::ForInStatement(mut data) => {
                data.initializer = self.visit_optional_node(data.initializer)?;
                data.expression = self.visit_optional_node(data.expression)?;
                data.statement =
                    self.visit_iteration_body(data.statement, SyntaxKind::ForInStatement)?;
                NodeData::ForInStatement(data)
            }
            NodeData::ForOfStatement(mut data) => {
                data.initializer = self.visit_optional_node(data.initializer)?;
                data.expression = self.visit_optional_node(data.expression)?;
                data.statement =
                    self.visit_iteration_body(data.statement, SyntaxKind::ForOfStatement)?;
                NodeData::ForOfStatement(data)
            }
            NodeData::WhileStatement(mut data) => {
                data.expression = self.visit_optional_node(data.expression)?;
                data.statement =
                    self.visit_iteration_body(data.statement, SyntaxKind::WhileStatement)?;
                NodeData::WhileStatement(data)
            }
            NodeData::DoStatement(mut data) => {
                data.statement =
                    self.visit_iteration_body(data.statement, SyntaxKind::DoStatement)?;
                data.expression = self.visit_optional_node(data.expression)?;
                NodeData::DoStatement(data)
            }
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: self.context.arena().node(original)?.kind,
                    field: "iteration data",
                })
            }
        };
        Ok(self.update_contextual_node(original, data)?.node())
    }
}

// A40 staged visitor design. The class/member producers are supplied by the
// separate staged class design; this is not an executable production patch.
enum RetainedVisitOutcome {
    One(TransformNode),
    Many(Vec<TransformNode>),
}

impl<'context, 'resolver, 'aliases> ClassFieldsVisitor<'context, 'resolver, 'aliases> {
    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        match self.visit_outcome(self.node(id))? {
            RetainedVisitOutcome::One(node) => Ok(Some(node.node())),
            RetainedVisitOutcome::Many(_) => Err(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(self.node(id))?.kind,
                field: "single-node position received a statement list",
            }),
        }
    }

    fn visit_statement_lifted(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        match self.visit_outcome(node)? {
            RetainedVisitOutcome::One(node) => Ok(Some(node)),
            RetainedVisitOutcome::Many(mut statements) => {
                if statements.len() == 1 {
                    Ok(statements.pop())
                } else {
                    Ok(Some(self.create_block(statements, false)?))
                }
            }
        }
    }

    fn visit_optional_statement(
        &mut self,
        node: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        match node {
            Some(node) => self
                .visit_statement_lifted(self.node(node))
                .map(|node| node.map(TransformNode::node)),
            None => Ok(None),
        }
    }

    fn visit_outcome(
        &mut self,
        original: TransformNode,
    ) -> Result<RetainedVisitOutcome, TransformError> {
        let record = self.context.arena().node(original)?.clone();
        if record.kind != SyntaxKind::SourceFile
            && self.context.arena().transform_flags(original).bits()
                & (TransformFlags::CONTAINS_CLASS_FIELDS
                    | TransformFlags::CONTAINS_LEXICAL_THIS_OR_SUPER)
                    .bits()
                == 0
        {
            return Ok(RetainedVisitOutcome::One(original));
        }
        let transformed = match record.data {
            NodeData::SourceFile(data) => self.visit_source(original, data)?,
            NodeData::ClassDeclaration(data) => {
                return self
                    .visit_class_declaration(original, data)
                    .map(RetainedVisitOutcome::Many);
            }
            NodeData::ClassExpression(data) => self.visit_class_expression(original, data)?,
            data @ (NodeData::FunctionDeclaration(_)
            | NodeData::FunctionExpression(_)
            | NodeData::ArrowFunction(_)
            | NodeData::MethodDeclaration(_)
            | NodeData::GetAccessor(_)
            | NodeData::SetAccessor(_)
            | NodeData::Constructor(_)) => self.visit_function(original, data)?,
            NodeData::ClassStaticBlockDeclaration(data) => {
                self.visit_static_block(original, data)?
            }
            data @ (NodeData::ForStatement(_)
            | NodeData::ForInStatement(_)
            | NodeData::ForOfStatement(_)
            | NodeData::WhileStatement(_)
            | NodeData::DoStatement(_)) => self.visit_iteration(original, data)?,
            NodeData::IfStatement(mut data) => {
                data.expression = self.visit_optional_node(data.expression)?;
                data.then_statement = self.visit_optional_statement(data.then_statement)?;
                data.else_statement = self.visit_optional_statement(data.else_statement)?;
                self.update_contextual_node(original, NodeData::IfStatement(data))?
                    .node()
            }
            NodeData::LabeledStatement(mut data) => {
                data.label = self.visit_optional_node(data.label)?;
                data.statement = self.visit_optional_statement(data.statement)?;
                self.update_contextual_node(original, NodeData::LabeledStatement(data))?
                    .node()
            }
            NodeData::WithStatement(mut data) => {
                data.expression = self.visit_optional_node(data.expression)?;
                data.statement = self.visit_optional_statement(data.statement)?;
                self.update_contextual_node(original, NodeData::WithStatement(data))?
                    .node()
            }
            NodeData::Token => original.node(),
            data => self.update_generic(original, data)?,
        };
        Ok(RetainedVisitOutcome::One(self.node(transformed)))
    }

    fn visit_source(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::SourceFileData,
    ) -> Result<NodeId, TransformError> {
        if self
            .context
            .arena()
            .source(self.source)?
            .syntax()
            .is_declaration_file
        {
            return Ok(original.node());
        }
        self.context.start_lexical_environment()?;
        let visited = self.visit_optional_nodes(data.statements);
        let environment = self.context.end_lexical_environment();
        let visited = visited?;
        let mut statements = self.array_nodes(visited)?;
        self.merge_lexical_environment(&mut statements, environment?)?;
        let statements = if let Some(previous) = visited {
            let previous = self.array(previous);
            self.context
                .factory()?
                .update_node_array(previous, statements)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
        };
        data.statements = Some(statements.array());
        Ok(self
            .update_contextual_node(original, NodeData::SourceFile(data))?
            .node())
    }
}

impl NodeDataChildVisitor for ClassFieldsVisitor<'_, '_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("retained child belongs to the transform source")
            .kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.visit(id)
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        let original = self.array(id);
        let nodes = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(nodes.len());
        for node in nodes {
            match self.visit_outcome(self.node(node))? {
                RetainedVisitOutcome::One(node) => visited.push(node),
                RetainedVisitOutcome::Many(nodes) => visited.extend(nodes),
            }
        }
        Ok(Some(
            self.context
                .factory()?
                .update_node_array(original, visited)?
                .array(),
        ))
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}
