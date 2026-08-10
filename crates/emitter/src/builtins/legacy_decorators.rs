//! H2.4a legacy-decorator lowering.

use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    for_each_child, try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId,
    SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeCheckFlags, NodeFlags};

use crate::{
    EmitFlags, EmitHelper, EmitResolver, EmitResolverError, EmitResolverNode,
    EmitTypeReferenceSerializationKind, TransformError, TransformFlags, TransformNode,
    TransformNodeArray, TransformRoot, TransformSourceId, TransformationContext, Transformer,
    UnsupportedEmitFeature,
};

use super::{flags_after_update, is_prologue_statement, system::collect_identifier_texts};

const DECORATE_HELPER_TEXT: &str = r#"var __decorate = (this && this.__decorate) || function (decorators, target, key, desc) {
    var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;
    if (typeof Reflect === "object" && typeof Reflect.decorate === "function") r = Reflect.decorate(decorators, target, key, desc);
    else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
    return c > 3 && r && Object.defineProperty(target, key, r), r;
};"#;

const METADATA_HELPER_TEXT: &str = r#"var __metadata = (this && this.__metadata) || function (k, v) {
    if (typeof Reflect === "object" && typeof Reflect.metadata === "function") return Reflect.metadata(k, v);
};"#;

const PARAM_HELPER_TEXT: &str = r#"var __param = (this && this.__param) || function (paramIndex, decorator) {
    return function (target, key) { decorator(target, key, paramIndex); }
};"#;

/// tsc-port: transformLegacyDecorators @6.0.3
/// tsc-hash: 53edcf53fff8187874600e2823d9a99c018f4ecaf10b688648985794f199c722
/// tsc-span: _tsc.js:98430-98943
pub(super) fn transform_legacy_decorators<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    Box::new(LegacyDecoratorTransformer {
        resolver,
        emit_decorator_metadata: options.emit_decorator_metadata == Some(true),
        strict_null_checks: options.strict_option_value(options.strict_null_checks),
    })
}

struct LegacyDecoratorTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    emit_decorator_metadata: bool,
    strict_null_checks: bool,
}

impl Transformer for LegacyDecoratorTransformer<'_> {
    fn name(&self) -> &'static str {
        "transformLegacyDecorators"
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
            return Ok(root);
        }
        let current_root = context.arena().root(source)?;
        let mut visitor = LegacyDecoratorVisitor::new(
            context,
            source,
            self.resolver,
            self.emit_decorator_metadata,
            self.strict_null_checks,
        );
        let transformed =
            visitor
                .visit(current_root.node())?
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

struct LegacyDecoratorVisitor<'context, 'resolver> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    resolver: &'resolver dyn EmitResolver,
    emit_decorator_metadata: bool,
    strict_null_checks: bool,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
    expanded_classes: BTreeMap<NodeId, Vec<NodeId>>,
    used_names: BTreeSet<String>,
    hoisted_names: Vec<String>,
    class_aliases: BTreeMap<NodeId, String>,
    computed_names: BTreeMap<NodeId, String>,
    next_temp_name: usize,
}

impl<'context, 'resolver> LegacyDecoratorVisitor<'context, 'resolver> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        resolver: &'resolver dyn EmitResolver,
        emit_decorator_metadata: bool,
        strict_null_checks: bool,
    ) -> Self {
        let used_names = collect_identifier_texts(context.arena(), source);
        Self {
            context,
            source,
            resolver,
            emit_decorator_metadata,
            strict_null_checks,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
            expanded_classes: BTreeMap::new(),
            used_names,
            hoisted_names: Vec::new(),
            class_aliases: BTreeMap::new(),
            computed_names: BTreeMap::new(),
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
            NodeData::Decorator(_) => None,
            NodeData::Token => Some(original.node()),
            NodeData::Identifier(data) => {
                if let Some(alias) = self.class_alias_for_reference(original)? {
                    let replacement = self.create_identifier(&alias)?;
                    self.set_original_and_range(replacement, original)?;
                    Some(replacement.node())
                } else {
                    Some(self.update_generic(original, NodeData::Identifier(data))?)
                }
            }
            NodeData::ClassExpression(mut data) => {
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_generic(original, NodeData::ClassExpression(data))?)
            }
            NodeData::Constructor(mut data) => {
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_generic(original, NodeData::Constructor(data))?)
            }
            NodeData::MethodDeclaration(mut data) => {
                data.name = self.rewrite_computed_member_name(original, data.name)?;
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_generic(original, NodeData::MethodDeclaration(data))?)
            }
            NodeData::GetAccessor(mut data) => {
                data.name = self.rewrite_computed_member_name(original, data.name)?;
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_generic(original, NodeData::GetAccessor(data))?)
            }
            NodeData::SetAccessor(mut data) => {
                data.name = self.rewrite_computed_member_name(original, data.name)?;
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_generic(original, NodeData::SetAccessor(data))?)
            }
            NodeData::PropertyDeclaration(mut data) => {
                data.name = self.rewrite_computed_member_name(original, data.name)?;
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_generic(original, NodeData::PropertyDeclaration(data))?)
            }
            NodeData::Parameter(mut data) => {
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_generic(original, NodeData::Parameter(data))?)
            }
            data => Some(self.update_generic(original, data)?),
        };
        self.nodes.insert(id, transformed);
        Ok(transformed)
    }

    fn visit_class_declaration(
        &mut self,
        id: NodeId,
    ) -> Result<Vec<TransformNode>, TransformError> {
        if let Some(expanded) = self.expanded_classes.get(&id) {
            return Ok(expanded.iter().copied().map(|id| self.node(id)).collect());
        }
        let current = self.node(id);
        let record = self.context.arena().node(current)?.clone();
        let NodeData::ClassDeclaration(mut data) = record.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: record.kind,
                field: "class declaration",
            });
        };
        let is_export = self.has_modifier(data.modifiers, SyntaxKind::ExportKeyword)?;
        let is_default = self.has_modifier(data.modifiers, SyntaxKind::DefaultKeyword)?;
        let class_decorators = self.decorator_expressions(data.modifiers)?;
        let constructor = self.first_constructor(data.members)?;
        let mut constructor_decorators = if let Some(constructor) = constructor {
            self.parameter_decorator_expressions(constructor)?
        } else {
            Vec::new()
        };
        let has_constructor_decoration =
            !class_decorators.is_empty() || !constructor_decorators.is_empty();
        let original_class = self.context.arena().get_original_node(current);
        let original_members = match &self.context.arena().node(original_class)?.data {
            NodeData::ClassDeclaration(data) => data.members,
            _ => data.members,
        };
        let has_member_decoration = self.class_has_member_decoration(original_members)?;
        if !has_constructor_decoration && !has_member_decoration {
            let updated = self.update_generic(current, NodeData::ClassDeclaration(data))?;
            self.nodes.insert(id, Some(updated));
            self.expanded_classes.insert(id, vec![updated]);
            return Ok(vec![self.node(updated)]);
        }
        let explicit_name = data.name;
        let name = if let Some(name) = explicit_name {
            name
        } else if is_export && is_default {
            let generated = self.allocate_generated_class_name("default");
            self.create_identifier(&generated)?.node()
        } else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassDeclaration,
                field: "name",
            });
        };
        let name_text = self.identifier_text(name)?.to_owned();
        if self.emit_decorator_metadata && has_constructor_decoration {
            let parameter_types = if let Some(constructor) = constructor {
                self.serialize_parameter_types(constructor)?
            } else {
                Vec::new()
            };
            let value = self.create_array_literal(parameter_types, false)?;
            constructor_decorators.push(self.create_metadata("design:paramtypes", value)?);
        }
        let class_alias = if has_constructor_decoration
            && self.resolver.has_node_check_flag(
                self.resolver_node(original_class)?,
                NodeCheckFlags::CONTAINS_CONSTRUCTOR_REFERENCE.bits() as u32,
            )? {
            let alias = self.allocate_class_alias(&name_text);
            self.class_aliases
                .insert(original_class.node(), alias.clone());
            Some(alias)
        } else {
            None
        };

        let (mut members, mut decoration_statements, private_decoration) =
            self.transform_class_members(current, data.members, &name_text)?;
        if private_decoration && !decoration_statements.is_empty() {
            let block = self.create_block(decoration_statements, true)?;
            let static_block = self.context.factory()?.create_node(
                self.source,
                NodeData::ClassStaticBlockDeclaration(
                    tsc_syntax::nodes::ClassStaticBlockDeclarationData {
                        body: Some(block.node()),
                        modifiers: None,
                    },
                ),
                TransformFlags::NONE,
            )?;
            let mut member_nodes = self.array_nodes(members)?;
            member_nodes.push(static_block);
            members = Some(
                self.context
                    .factory()?
                    .create_node_array(self.source, member_nodes)?
                    .array(),
            );
            decoration_statements = Vec::new();
        }
        data.members = members;

        let mut statements = if has_constructor_decoration {
            self.transform_decorated_class_declaration(
                current,
                data,
                name,
                explicit_name,
                class_alias.as_deref(),
            )?
        } else {
            data.name = Some(name);
            data.modifiers = self.strip_decorators(data.modifiers)?;
            let transform_flags = self.context.arena().transform_flags(current);
            let updated = self.context.factory()?.update_node(
                current,
                NodeData::ClassDeclaration(data),
                transform_flags,
            )?;
            vec![updated]
        };
        statements.append(&mut decoration_statements);

        if has_constructor_decoration {
            let mut decorators = class_decorators;
            decorators.extend(constructor_decorators);
            let class_name = self.create_identifier(&name_text)?;
            let mut decorate = self.create_decorate_call(decorators, class_name, None, None)?;
            if let Some(alias) = class_alias.as_deref() {
                let alias = self.create_identifier(alias)?;
                decorate = self.create_assignment(alias, decorate)?;
            }
            let class_name = self.create_identifier(&name_text)?;
            if let Some(explicit_name) = explicit_name {
                self.set_original_and_range(class_name, self.node(explicit_name))?;
            }
            self.context
                .arena_mut()?
                .metadata_mut(class_name)
                .add_flags(EmitFlags::LOCAL_NAME);
            let assignment = self.create_assignment(class_name, decorate)?;
            let statement = self.create_expression_statement(assignment)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::NO_COMMENTS);
            statements.push(statement);
        }
        if has_constructor_decoration && is_export {
            statements.push(if is_default {
                self.create_export_default(&name_text)?
            } else {
                self.create_named_export(&name_text)?
            });
        }

        self.nodes.insert(id, None);
        self.expanded_classes.insert(
            id,
            statements
                .iter()
                .map(|statement| statement.node())
                .collect(),
        );
        Ok(statements)
    }

    fn transform_decorated_class_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassDeclarationData,
        name: NodeId,
        expression_name: Option<NodeId>,
        class_alias: Option<&str>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let assign_class_alias_in_static_block =
            class_alias.is_some() && self.class_has_static_property_or_block(data.members)?;
        if assign_class_alias_in_static_block {
            let alias = self.create_identifier(class_alias.expect("class alias is present"))?;
            let this = self.context.factory()?.create_token(
                self.source,
                SyntaxKind::ThisKeyword,
                TransformFlags::NONE,
            )?;
            let assignment = self.create_assignment(alias, this)?;
            let statement = self.create_expression_statement(assignment)?;
            let block = self.create_block(vec![statement], false)?;
            let static_block = self.context.factory()?.create_node(
                self.source,
                NodeData::ClassStaticBlockDeclaration(
                    tsc_syntax::nodes::ClassStaticBlockDeclarationData {
                        body: Some(block.node()),
                        modifiers: None,
                    },
                ),
                TransformFlags::NONE,
            )?;
            let mut members = self.array_nodes(data.members)?;
            members.insert(0, static_block);
            data.members = Some(
                self.context
                    .factory()?
                    .create_node_array(self.source, members)?
                    .array(),
            );
        }
        let modifiers = self.strip_class_declaration_modifiers(data.modifiers)?;
        let transform_flags = self.context.arena().transform_flags(original);
        let class_expression = self.context.factory()?.create_node(
            self.source,
            NodeData::ClassExpression(tsc_syntax::nodes::ClassExpressionData {
                name: expression_name,
                type_parameters: None,
                heritage_clauses: data.heritage_clauses.take(),
                members: data.members.take(),
                modifiers,
            }),
            transform_flags,
        )?;
        self.set_original_and_range(class_expression, original)?;
        let initializer =
            if let Some(alias) = class_alias.filter(|_| !assign_class_alias_in_static_block) {
                let alias = self.create_identifier(alias)?;
                self.create_assignment(alias, class_expression)?
            } else {
                class_expression
            };
        let declaration = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name),
                exclamation_token: None,
                r#type: None,
                initializer: Some(initializer.node()),
            }),
            TransformFlags::NONE,
        )?;
        self.context
            .arena_mut()?
            .set_original_node(declaration, Some(original))?;
        let statement = self.create_variable_statement(vec![declaration], NodeFlags::LET)?;
        self.set_original_and_range(statement, original)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::NO_TRAILING_COMMENTS);
        Ok(vec![statement])
    }

    fn transform_class_members(
        &mut self,
        class: TransformNode,
        members: Option<NodeArrayId>,
        class_name: &str,
    ) -> Result<(Option<NodeArrayId>, Vec<TransformNode>, bool), TransformError> {
        let current_members = self.array_nodes(members)?;
        let original_class = self.context.arena().get_original_node(class);
        let original_members = match &self.context.arena().node(original_class)?.data {
            NodeData::ClassDeclaration(data) => self.array_nodes(data.members)?,
            NodeData::ClassExpression(data) => self.array_nodes(data.members)?,
            _ => Vec::new(),
        };
        let mut by_original = BTreeMap::new();
        for member in &current_members {
            let original = self.context.arena().get_original_node(*member);
            by_original.insert(original.node(), *member);
        }

        let mut instance = Vec::new();
        let mut static_ = Vec::new();
        let mut private_decoration = false;
        for original_member in original_members {
            let source_member = by_original
                .get(&original_member.node())
                .copied()
                .unwrap_or(original_member);
            if self.member_name_is_private(source_member)? {
                continue;
            }
            let Some(statement) =
                self.create_member_decoration_statement(class_name, source_member)?
            else {
                continue;
            };
            private_decoration |= self.member_decorators_contain_private(source_member)?;
            if self.has_static_modifier(source_member)? {
                static_.push(statement);
            } else {
                instance.push(statement);
            }
        }
        instance.extend(static_);

        let visited = if let Some(members) = members {
            self.visit_nodes(members)?
        } else {
            None
        };
        Ok((visited, instance, private_decoration))
    }

    fn create_member_decoration_statement(
        &mut self,
        class_name: &str,
        mut member: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let original_member = self.context.arena().get_original_node(member);
        if matches!(
            self.context.arena().node(original_member)?.kind,
            SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
        ) {
            let Some(owner) = self.accessor_decoration_owner(original_member)? else {
                return Ok(None);
            };
            if owner != original_member {
                return Ok(None);
            }
            member = owner;
        }
        let record = self.context.arena().node(member)?.clone();
        let (name, modifiers, parameters, is_property) = match &record.data {
            NodeData::PropertyDeclaration(data) => (data.name, data.modifiers, None, true),
            NodeData::MethodDeclaration(data) => {
                (data.name, data.modifiers, data.parameters, false)
            }
            NodeData::GetAccessor(data) => {
                let parameters = self.paired_set_accessor(member)?.and_then(|setter| {
                    match &self.context.arena().node(setter).ok()?.data {
                        NodeData::SetAccessor(data) => data.parameters,
                        _ => None,
                    }
                });
                (data.name, data.modifiers, parameters, false)
            }
            NodeData::SetAccessor(data) => (data.name, data.modifiers, data.parameters, false),
            _ => return Ok(None),
        };
        let mut decorators = self.decorator_expressions(modifiers)?;
        if let Some(parameters) = parameters {
            decorators.extend(self.parameter_decorators(parameters)?);
        }
        if decorators.is_empty() {
            return Ok(None);
        }
        if self.emit_decorator_metadata {
            decorators.extend(self.member_metadata(member)?);
        }
        let target = if self.has_modifier(modifiers, SyntaxKind::StaticKeyword)? {
            self.create_identifier(class_name)?
        } else {
            let class = self.create_identifier(class_name)?;
            self.create_property_access(class, "prototype")?
        };
        let name = name.ok_or(TransformError::RequiredChildRemoved {
            parent: record.kind,
            field: "name",
        })?;
        let member_name = self.property_name_expression(member, name)?;
        let descriptor =
            if is_property && !self.has_modifier(modifiers, SyntaxKind::AccessorKeyword)? {
                self.create_void_zero()?
            } else {
                self.create_null()?
            };
        let call =
            self.create_decorate_call(decorators, target, Some(member_name), Some(descriptor))?;
        let statement = self.create_expression_statement(call)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::NO_COMMENTS);
        Ok(Some(statement))
    }

    fn member_metadata(
        &mut self,
        member: TransformNode,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let original = self.context.arena().get_original_node(member);
        let record = self.context.arena().node(original)?.clone();
        let mut metadata = Vec::new();
        match record.data {
            NodeData::PropertyDeclaration(data) => {
                let value = self.serialize_type(data.r#type, MetadataFallback::Object)?;
                metadata.push(self.create_metadata("design:type", value)?);
            }
            NodeData::MethodDeclaration(data) => {
                let value = self.create_identifier("Function")?;
                metadata.push(self.create_metadata("design:type", value)?);
                let parameters = self.serialize_parameter_types_from_array(data.parameters)?;
                let value = self.create_array_literal(parameters, false)?;
                metadata.push(self.create_metadata("design:paramtypes", value)?);
                let value = if data.r#type.is_none()
                    && self.has_modifier(data.modifiers, SyntaxKind::AsyncKeyword)?
                {
                    self.create_identifier("Promise")?
                } else {
                    self.serialize_type(data.r#type, MetadataFallback::VoidZero)?
                };
                metadata.push(self.create_metadata("design:returntype", value)?);
            }
            NodeData::GetAccessor(data) => {
                let paired_setter = self.paired_set_accessor(original)?;
                let (accessor_type, parameters) = if let Some(setter) = paired_setter {
                    let NodeData::SetAccessor(setter) =
                        self.context.arena().node(setter)?.data.clone()
                    else {
                        unreachable!("paired accessor is a setter")
                    };
                    (
                        self.first_parameter_type(setter.parameters)?
                            .or(data.r#type),
                        setter.parameters,
                    )
                } else {
                    (data.r#type, data.parameters)
                };
                let value = self.serialize_type(accessor_type, MetadataFallback::Object)?;
                metadata.push(self.create_metadata("design:type", value)?);
                let parameters = self.serialize_parameter_types_from_array(parameters)?;
                let value = self.create_array_literal(parameters, false)?;
                metadata.push(self.create_metadata("design:paramtypes", value)?);
            }
            NodeData::SetAccessor(data) => {
                let value = data
                    .parameters
                    .and_then(|array| self.context.arena().node_array_ref(self.source, array))
                    .and_then(|array| self.context.arena().node_array(array).ok())
                    .and_then(|array| array.nodes.first().copied())
                    .and_then(|parameter| self.context.arena().node_ref(self.source, parameter))
                    .map(|parameter| self.serialize_parameter_type(parameter))
                    .transpose()?
                    .unwrap_or(self.create_identifier("Object")?);
                metadata.push(self.create_metadata("design:type", value)?);
                let parameters = self.serialize_parameter_types_from_array(data.parameters)?;
                let value = self.create_array_literal(parameters, false)?;
                metadata.push(self.create_metadata("design:paramtypes", value)?);
            }
            _ => {}
        }
        Ok(metadata)
    }

    fn paired_set_accessor(
        &self,
        getter: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let record = self.context.arena().node(getter)?;
        let NodeData::GetAccessor(getter_data) = &record.data else {
            return Ok(None);
        };
        let Some(parent) = record.parent else {
            return Ok(None);
        };
        let parent = self.node(parent);
        let members = match &self.context.arena().node(parent)?.data {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            _ => None,
        };
        let getter_is_static =
            self.has_modifier(getter_data.modifiers, SyntaxKind::StaticKeyword)?;
        for member in self.array_nodes(members)? {
            let NodeData::SetAccessor(setter_data) = &self.context.arena().node(member)?.data
            else {
                continue;
            };
            if self.has_modifier(setter_data.modifiers, SyntaxKind::StaticKeyword)?
                == getter_is_static
                && self.property_names_equal(getter_data.name, setter_data.name)?
            {
                return Ok(Some(member));
            }
        }
        Ok(None)
    }

    fn accessor_decoration_owner(
        &self,
        accessor: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let record = self.context.arena().node(accessor)?;
        let (accessor_name, accessor_modifiers) = match &record.data {
            NodeData::GetAccessor(data) => (data.name, data.modifiers),
            NodeData::SetAccessor(data) => (data.name, data.modifiers),
            _ => return Ok(None),
        };
        let accessor_is_static =
            self.has_modifier(accessor_modifiers, SyntaxKind::StaticKeyword)?;
        let Some(parent) = record.parent else {
            return Ok(None);
        };
        let members = match &self.context.arena().node(self.node(parent))?.data {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            _ => None,
        };
        for candidate in self.array_nodes(members)? {
            let modifiers = match &self.context.arena().node(candidate)?.data {
                NodeData::GetAccessor(data) => data.modifiers,
                NodeData::SetAccessor(data) => data.modifiers,
                _ => continue,
            };
            let name = match &self.context.arena().node(candidate)?.data {
                NodeData::GetAccessor(data) => data.name,
                NodeData::SetAccessor(data) => data.name,
                _ => None,
            };
            if self.has_modifier(modifiers, SyntaxKind::StaticKeyword)? == accessor_is_static
                && self.property_names_equal(accessor_name, name)?
                && !self.decorator_expressions(modifiers)?.is_empty()
            {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    fn first_parameter_type(
        &self,
        parameters: Option<NodeArrayId>,
    ) -> Result<Option<NodeId>, TransformError> {
        let Some(parameter) = self.array_nodes(parameters)?.first().copied() else {
            return Ok(None);
        };
        Ok(match &self.context.arena().node(parameter)?.data {
            NodeData::Parameter(data) => data.r#type,
            _ => None,
        })
    }

    fn property_names_equal(
        &self,
        left: Option<NodeId>,
        right: Option<NodeId>,
    ) -> Result<bool, TransformError> {
        let (Some(left), Some(right)) = (left, right) else {
            return Ok(false);
        };
        let left = self.property_name_key(self.node(left))?;
        let right = self.property_name_key(self.node(right))?;
        Ok(left.is_some() && left == right)
    }

    fn property_name_key(&self, name: TransformNode) -> Result<Option<String>, TransformError> {
        Ok(match &self.context.arena().node(name)?.data {
            NodeData::Identifier(data) => Some(format!("i:{}", data.text)),
            NodeData::PrivateIdentifier(data) => Some(format!("p:{}", data.text)),
            NodeData::StringLiteral(data) => Some(format!("s:{}", data.text)),
            NodeData::NumericLiteral(data) => Some(format!("n:{}", data.text)),
            NodeData::NoSubstitutionTemplateLiteral(data) => Some(format!("s:{}", data.text)),
            NodeData::ComputedPropertyName(data) => data
                .expression
                .and_then(|expression| self.context.arena().node_ref(self.source, expression))
                .map(|expression| self.property_name_key(expression))
                .transpose()?
                .flatten(),
            _ => None,
        })
    }

    fn serialize_parameter_types(
        &mut self,
        constructor: TransformNode,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let original = self.context.arena().get_original_node(constructor);
        let parameters = match &self.context.arena().node(original)?.data {
            NodeData::Constructor(data) => data.parameters,
            _ => None,
        };
        self.serialize_parameter_types_from_array(parameters)
    }

    fn serialize_parameter_types_from_array(
        &mut self,
        parameters: Option<NodeArrayId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let parameters = self.array_nodes(parameters)?;
        let mut serialized = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            if !self.is_this_parameter(parameter) {
                serialized.push(self.serialize_parameter_type(parameter)?);
            }
        }
        Ok(serialized)
    }

    fn serialize_parameter_type(
        &mut self,
        parameter: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let original = self.context.arena().get_original_node(parameter);
        let r#type = match &self.context.arena().node(original)?.data {
            NodeData::Parameter(data) if data.dot_dot_dot_token.is_some() => {
                self.rest_parameter_element_type(data.r#type)?
            }
            NodeData::Parameter(data) => data.r#type,
            _ => None,
        };
        self.serialize_type(r#type, MetadataFallback::Object)
    }

    fn is_this_parameter(&self, parameter: TransformNode) -> bool {
        let original = self.context.arena().get_original_node(parameter);
        let Ok(record) = self.context.arena().node(original) else {
            return false;
        };
        let NodeData::Parameter(data) = &record.data else {
            return false;
        };
        data.name.is_some_and(|name| {
            self.context.arena().node(self.node(name)).is_ok_and(
                |name| matches!(&name.data, NodeData::Identifier(data) if data.text == "this"),
            )
        })
    }

    fn rest_parameter_element_type(
        &self,
        r#type: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        let Some(r#type) = r#type else {
            return Ok(None);
        };
        Ok(match &self.context.arena().node(self.node(r#type))?.data {
            NodeData::ArrayType(data) => data.element_type,
            NodeData::TypeReference(data) => data.type_arguments.and_then(|arguments| {
                self.context
                    .arena()
                    .node_array_ref(self.source, arguments)
                    .and_then(|arguments| self.context.arena().node_array(arguments).ok())
                    .and_then(|arguments| {
                        (arguments.nodes.len() == 1).then_some(arguments.nodes[0])
                    })
            }),
            _ => None,
        })
    }

    /// tsc-port: serializeTypeNode @6.0.3
    /// tsc-hash: dcacf9f0f369cd137334d19de5369beae8d497b91141b060ced299bda8c51810
    /// tsc-span: _tsc.js:98161-98330
    fn serialize_type(
        &mut self,
        r#type: Option<NodeId>,
        fallback: MetadataFallback,
    ) -> Result<TransformNode, TransformError> {
        let Some(r#type) = r#type else {
            return self.metadata_fallback(fallback);
        };
        let node = self.node(r#type);
        let record = self.context.arena().node(node)?.clone();
        match record.kind {
            SyntaxKind::StringKeyword => self.create_identifier("String"),
            SyntaxKind::NumberKeyword => self.create_identifier("Number"),
            SyntaxKind::BooleanKeyword => self.create_identifier("Boolean"),
            SyntaxKind::BigIntKeyword => self.create_identifier("BigInt"),
            SyntaxKind::SymbolKeyword => self.create_identifier("Symbol"),
            SyntaxKind::ObjectKeyword | SyntaxKind::AnyKeyword | SyntaxKind::UnknownKeyword => {
                self.create_identifier("Object")
            }
            SyntaxKind::VoidKeyword | SyntaxKind::UndefinedKeyword | SyntaxKind::NeverKeyword => {
                self.create_void_zero()
            }
            SyntaxKind::FunctionType | SyntaxKind::ConstructorType => {
                self.create_identifier("Function")
            }
            SyntaxKind::ArrayType | SyntaxKind::TupleType => self.create_identifier("Array"),
            SyntaxKind::TypePredicate => match record.data {
                NodeData::TypePredicate(data) if data.asserts_modifier.is_some() => {
                    self.create_void_zero()
                }
                NodeData::TypePredicate(_) => self.create_identifier("Boolean"),
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::TemplateLiteralType => self.create_identifier("String"),
            SyntaxKind::LiteralType => match record.data {
                NodeData::LiteralType(data) => self.serialize_literal_type(data.literal),
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::ParenthesizedType => match record.data {
                NodeData::ParenthesizedType(data) => self.serialize_type(data.r#type, fallback),
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::TypeReference => match record.data {
                NodeData::TypeReference(data) => {
                    self.serialize_entity_name_type(data.type_name, fallback)
                }
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::IntersectionType => match record.data {
                NodeData::IntersectionType(data) => {
                    self.serialize_union_or_intersection(data.types, true)
                }
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::UnionType => match record.data {
                NodeData::UnionType(data) => {
                    self.serialize_union_or_intersection(data.types, false)
                }
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::ConditionalType => match record.data {
                NodeData::ConditionalType(data) => self.serialize_type_constituents(
                    [data.true_type, data.false_type].into_iter().flatten(),
                    false,
                ),
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::TypeOperator => match record.data {
                NodeData::TypeOperator(data) if data.operator == SyntaxKind::ReadonlyKeyword => {
                    self.serialize_type(data.r#type, fallback)
                }
                _ => self.create_identifier("Object"),
            },
            _ => self.metadata_fallback(fallback),
        }
    }

    fn serialize_literal_type(
        &mut self,
        literal: Option<NodeId>,
    ) -> Result<TransformNode, TransformError> {
        let Some(literal) = literal else {
            return self.create_identifier("Object");
        };
        let literal = self.node(literal);
        match self.context.arena().node(literal)?.data.clone() {
            NodeData::StringLiteral(_) | NodeData::NoSubstitutionTemplateLiteral(_) => {
                self.create_identifier("String")
            }
            NodeData::NumericLiteral(_) => self.create_identifier("Number"),
            NodeData::BigIntLiteral(_) => self.create_identifier("BigInt"),
            NodeData::Token => match self.context.arena().node(literal)?.kind {
                SyntaxKind::TrueKeyword | SyntaxKind::FalseKeyword => {
                    self.create_identifier("Boolean")
                }
                SyntaxKind::NullKeyword => self.create_void_zero(),
                _ => self.create_identifier("Object"),
            },
            NodeData::PrefixUnaryExpression(data) => {
                let Some(operand) = data.operand else {
                    return self.create_identifier("Object");
                };
                match self.context.arena().node(self.node(operand))?.kind {
                    SyntaxKind::NumericLiteral => self.create_identifier("Number"),
                    SyntaxKind::BigIntLiteral => self.create_identifier("BigInt"),
                    _ => self.create_identifier("Object"),
                }
            }
            _ => self.create_identifier("Object"),
        }
    }

    fn serialize_union_or_intersection(
        &mut self,
        types: Option<NodeArrayId>,
        is_intersection: bool,
    ) -> Result<TransformNode, TransformError> {
        let types = self
            .array_nodes(types)?
            .into_iter()
            .map(TransformNode::node);
        self.serialize_type_constituents(types, is_intersection)
    }

    fn serialize_type_constituents(
        &mut self,
        types: impl IntoIterator<Item = NodeId>,
        is_intersection: bool,
    ) -> Result<TransformNode, TransformError> {
        let mut serialized: Option<(TransformNode, String)> = None;
        for r#type in types {
            let r#type = self.skip_type_parentheses(r#type)?;
            let kind = self.context.arena().node(self.node(r#type))?.kind;
            if kind == SyntaxKind::NeverKeyword {
                if is_intersection {
                    return self.create_void_zero();
                }
                continue;
            }
            if kind == SyntaxKind::UnknownKeyword {
                if !is_intersection {
                    return self.create_identifier("Object");
                }
                continue;
            }
            if kind == SyntaxKind::AnyKeyword {
                return self.create_identifier("Object");
            }
            if !self.strict_null_checks && self.is_null_or_undefined_type(r#type)? {
                continue;
            }
            let node = self.serialize_type(Some(r#type), MetadataFallback::Object)?;
            let key = self
                .serialized_type_key(node)?
                .unwrap_or_else(|| "other".to_owned());
            if key == "id:Object" {
                return Ok(node);
            }
            if let Some((_, previous)) = &serialized {
                if *previous != key {
                    return self.create_identifier("Object");
                }
            } else {
                serialized = Some((node, key));
            }
        }
        if let Some((node, _)) = serialized {
            Ok(node)
        } else {
            self.create_void_zero()
        }
    }

    fn skip_type_parentheses(&self, mut r#type: NodeId) -> Result<NodeId, TransformError> {
        loop {
            let NodeData::ParenthesizedType(data) =
                &self.context.arena().node(self.node(r#type))?.data
            else {
                return Ok(r#type);
            };
            let Some(inner) = data.r#type else {
                return Ok(r#type);
            };
            r#type = inner;
        }
    }

    fn is_null_or_undefined_type(&self, r#type: NodeId) -> Result<bool, TransformError> {
        let record = self.context.arena().node(self.node(r#type))?;
        if record.kind == SyntaxKind::UndefinedKeyword {
            return Ok(true);
        }
        let NodeData::LiteralType(data) = &record.data else {
            return Ok(false);
        };
        Ok(data.literal.is_some_and(|literal| {
            self.context
                .arena()
                .node(self.node(literal))
                .is_ok_and(|literal| literal.kind == SyntaxKind::NullKeyword)
        }))
    }

    fn serialized_type_key(&self, node: TransformNode) -> Result<Option<String>, TransformError> {
        Ok(match &self.context.arena().node(node)?.data {
            NodeData::Identifier(data) => Some(format!("id:{}", data.text)),
            NodeData::VoidExpression(data) => data
                .expression
                .and_then(|expression| self.context.arena().node_ref(self.source, expression))
                .and_then(|expression| self.context.arena().node(expression).ok())
                .and_then(|expression| {
                    matches!(&expression.data, NodeData::NumericLiteral(data) if data.text == "0")
                        .then_some("void:0".to_owned())
                }),
            NodeData::PropertyAccessExpression(data) => {
                let expression = data
                    .expression
                    .and_then(|expression| self.context.arena().node_ref(self.source, expression));
                let name = data
                    .name
                    .and_then(|name| self.context.arena().node_ref(self.source, name));
                match (expression, name) {
                    (Some(expression), Some(name)) => {
                        let left = self.serialized_type_key(expression)?;
                        let right = self.serialized_type_key(name)?;
                        left.zip(right)
                            .map(|(left, right)| format!("{left}.{right}"))
                    }
                    _ => None,
                }
            }
            _ => None,
        })
    }

    /// tsc-port: serializeTypeReferenceNode @6.0.3
    /// tsc-hash: 8c542ad686faae965916b6b92788bca6ec6fd36d4d190e6111d3777832bc9ad3
    /// tsc-span: _tsc.js:98331-98412
    fn serialize_entity_name_type(
        &mut self,
        name: Option<NodeId>,
        fallback: MetadataFallback,
    ) -> Result<TransformNode, TransformError> {
        let Some(name) = name else {
            return self.metadata_fallback(fallback);
        };
        let node = self.node(name);
        let kind = self
            .resolver
            .get_type_reference_serialization_kind(self.resolver_node(node)?)?;
        match kind {
            EmitTypeReferenceSerializationKind::Unknown => {
                self.serialize_unknown_entity_name_type(node)
            }
            EmitTypeReferenceSerializationKind::TypeWithConstructSignatureAndValue => {
                self.entity_name_expression(node)
            }
            EmitTypeReferenceSerializationKind::VoidNullableOrNeverType => self.create_void_zero(),
            EmitTypeReferenceSerializationKind::NumberLikeType => self.create_identifier("Number"),
            EmitTypeReferenceSerializationKind::BigIntLikeType => self.create_identifier("BigInt"),
            EmitTypeReferenceSerializationKind::StringLikeType => self.create_identifier("String"),
            EmitTypeReferenceSerializationKind::BooleanType => self.create_identifier("Boolean"),
            EmitTypeReferenceSerializationKind::ArrayLikeType => self.create_identifier("Array"),
            EmitTypeReferenceSerializationKind::ESSymbolType => self.create_identifier("Symbol"),
            EmitTypeReferenceSerializationKind::Promise => self.create_identifier("Promise"),
            EmitTypeReferenceSerializationKind::TypeWithCallSignature => {
                self.create_identifier("Function")
            }
            EmitTypeReferenceSerializationKind::ObjectType => self.create_identifier("Object"),
        }
    }

    fn serialize_unknown_entity_name_type(
        &mut self,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (guard, value) = self.checked_entity_name_parts(name)?;
        let checked = self.create_binary(guard, SyntaxKind::AmpersandAmpersandToken, value)?;
        let temp_name = self.allocate_temp_name();
        let temp = self.create_identifier(&temp_name)?;
        let assignment = self.create_assignment(temp, checked)?;
        let assignment = self.create_parenthesized(assignment)?;
        let type_of = self.create_typeof(assignment)?;
        let function = self.create_string_literal("function")?;
        let condition =
            self.create_binary(type_of, SyntaxKind::EqualsEqualsEqualsToken, function)?;
        let when_true = self.create_identifier(&temp_name)?;
        let when_false = self.create_identifier("Object")?;
        self.create_conditional(condition, when_true, when_false)
    }

    fn checked_entity_name_parts(
        &mut self,
        name: TransformNode,
    ) -> Result<(TransformNode, TransformNode), TransformError> {
        match self.context.arena().node(name)?.data.clone() {
            NodeData::Identifier(data) => {
                let left = self.create_identifier(&data.text)?;
                let type_of = self.create_typeof(left)?;
                let undefined = self.create_string_literal("undefined")?;
                let guard = self.create_binary(
                    type_of,
                    SyntaxKind::ExclamationEqualsEqualsToken,
                    undefined,
                )?;
                let value = self.create_identifier(&data.text)?;
                Ok((guard, value))
            }
            NodeData::QualifiedName(data) => {
                let left = data
                    .left
                    .and_then(|left| self.context.arena().node_ref(self.source, left))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::QualifiedName,
                        field: "left",
                    })?;
                let right = data
                    .right
                    .and_then(|right| self.context.arena().node_ref(self.source, right))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::QualifiedName,
                        field: "right",
                    })?;
                let right = self.identifier_text(right.node())?.to_owned();
                if matches!(
                    self.context.arena().node(left)?.data,
                    NodeData::Identifier(_)
                ) {
                    let (guard, left_value) = self.checked_entity_name_parts(left)?;
                    let value = self.create_property_access(left_value, &right)?;
                    return Ok((guard, value));
                }
                let (left_guard, left_value) = self.checked_entity_name_parts(left)?;
                let temp_name = self.allocate_temp_name();
                let temp = self.create_identifier(&temp_name)?;
                let assignment = self.create_assignment(temp, left_value)?;
                let void_zero = self.create_void_zero()?;
                let defined = self.create_binary(
                    assignment,
                    SyntaxKind::ExclamationEqualsEqualsToken,
                    void_zero,
                )?;
                let guard =
                    self.create_binary(left_guard, SyntaxKind::AmpersandAmpersandToken, defined)?;
                let temp = self.create_identifier(&temp_name)?;
                let value = self.create_property_access(temp, &right)?;
                Ok((guard, value))
            }
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TypeReference,
                field: "type name",
            }),
        }
    }

    fn entity_name_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        match self.context.arena().node(node)?.data.clone() {
            NodeData::Identifier(data) => self.create_identifier(&data.text),
            NodeData::QualifiedName(data) => {
                let left = data
                    .left
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::QualifiedName,
                        field: "left",
                    })?;
                let right = data
                    .right
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::QualifiedName,
                        field: "right",
                    })?;
                let right = self.identifier_text(right.node())?.to_owned();
                let left = self.entity_name_expression(left)?;
                self.create_property_access(left, &right)
            }
            _ => self.create_identifier("Object"),
        }
    }

    fn metadata_fallback(
        &mut self,
        fallback: MetadataFallback,
    ) -> Result<TransformNode, TransformError> {
        match fallback {
            MetadataFallback::Object => self.create_identifier("Object"),
            MetadataFallback::VoidZero => self.create_void_zero(),
        }
    }

    fn first_constructor(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<Option<TransformNode>, TransformError> {
        for member in self.array_nodes(members)? {
            if matches!(
                &self.context.arena().node(member)?.data,
                NodeData::Constructor(data) if data.body.is_some()
            ) {
                return Ok(Some(member));
            }
        }
        Ok(None)
    }

    fn class_has_member_decoration(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for member in self.array_nodes(members)? {
            let (modifiers, parameters) = match &self.context.arena().node(member)?.data {
                NodeData::Constructor(data) => (data.modifiers, data.parameters),
                NodeData::PropertyDeclaration(data) => (data.modifiers, None),
                NodeData::MethodDeclaration(data) => (data.modifiers, data.parameters),
                NodeData::GetAccessor(data) => (data.modifiers, data.parameters),
                NodeData::SetAccessor(data) => (data.modifiers, data.parameters),
                _ => continue,
            };
            if !self.decorator_expressions(modifiers)?.is_empty() {
                return Ok(true);
            }
            for parameter in self.array_nodes(parameters)? {
                let modifiers = match &self.context.arena().node(parameter)?.data {
                    NodeData::Parameter(data) => data.modifiers,
                    _ => None,
                };
                if !self.decorator_expressions(modifiers)?.is_empty() {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn parameter_decorator_expressions(
        &mut self,
        declaration: TransformNode,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let parameters = match &self.context.arena().node(declaration)?.data {
            NodeData::Constructor(data) => data.parameters,
            NodeData::MethodDeclaration(data) => data.parameters,
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            _ => None,
        };
        match parameters {
            Some(parameters) => self.parameter_decorators(parameters),
            None => Ok(Vec::new()),
        }
    }

    fn parameter_decorators(
        &mut self,
        parameters: NodeArrayId,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let parameter_nodes = self.array_nodes(Some(parameters))?;
        let mut decorators = Vec::new();
        for (index, parameter) in parameter_nodes.into_iter().enumerate() {
            let modifiers = match &self.context.arena().node(parameter)?.data {
                NodeData::Parameter(data) => data.modifiers,
                _ => None,
            };
            for decorator in self.decorator_expressions(modifiers)? {
                decorators.push(self.create_param(index, decorator)?);
            }
        }
        Ok(decorators)
    }

    fn decorator_expressions(
        &self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut decorators = Vec::new();
        for modifier in self.array_nodes(modifiers)? {
            let NodeData::Decorator(data) = &self.context.arena().node(modifier)?.data else {
                continue;
            };
            let expression = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Decorator,
                    field: "expression",
                })?;
            decorators.push(self.node(expression));
        }
        Ok(decorators)
    }

    fn member_decorators_contain_private(
        &self,
        member: TransformNode,
    ) -> Result<bool, TransformError> {
        let record = self.context.arena().node(member)?;
        let (modifiers, parameters) = match &record.data {
            NodeData::PropertyDeclaration(data) => (data.modifiers, None),
            NodeData::MethodDeclaration(data) => (data.modifiers, data.parameters),
            NodeData::GetAccessor(data) => {
                let parameters = self.paired_set_accessor(member)?.and_then(|setter| {
                    match &self.context.arena().node(setter).ok()?.data {
                        NodeData::SetAccessor(data) => data.parameters,
                        _ => None,
                    }
                });
                (data.modifiers, parameters)
            }
            NodeData::SetAccessor(data) => (data.modifiers, data.parameters),
            _ => return Ok(false),
        };
        let mut expressions = self.decorator_expressions(modifiers)?;
        for parameter in self.array_nodes(parameters)? {
            let modifiers = match &self.context.arena().node(parameter)?.data {
                NodeData::Parameter(data) => data.modifiers,
                _ => None,
            };
            expressions.extend(self.decorator_expressions(modifiers)?);
        }
        for expression in expressions {
            if self.subtree_contains_private(expression)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn subtree_contains_private(&self, root: TransformNode) -> Result<bool, TransformError> {
        let mut stack = vec![root.node()];
        while let Some(id) = stack.pop() {
            let node = self.node(id);
            let record = self.context.arena().node(node)?;
            if record.kind == SyntaxKind::PrivateIdentifier {
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

    fn member_name_is_private(&self, member: TransformNode) -> Result<bool, TransformError> {
        let name = match &self.context.arena().node(member)?.data {
            NodeData::PropertyDeclaration(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            _ => None,
        };
        Ok(name.is_some_and(|name| {
            self.context
                .arena()
                .node(self.node(name))
                .is_ok_and(|name| name.kind == SyntaxKind::PrivateIdentifier)
        }))
    }

    fn has_static_modifier(&self, member: TransformNode) -> Result<bool, TransformError> {
        let modifiers = match &self.context.arena().node(member)?.data {
            NodeData::PropertyDeclaration(data) => data.modifiers,
            NodeData::MethodDeclaration(data) => data.modifiers,
            NodeData::GetAccessor(data) => data.modifiers,
            NodeData::SetAccessor(data) => data.modifiers,
            _ => None,
        };
        self.has_modifier(modifiers, SyntaxKind::StaticKeyword)
    }

    fn strip_decorators(
        &mut self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        self.filter_modifiers(modifiers, |kind| kind != SyntaxKind::Decorator)
    }

    fn strip_class_declaration_modifiers(
        &mut self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        self.filter_modifiers(modifiers, |kind| {
            !matches!(
                kind,
                SyntaxKind::Decorator | SyntaxKind::ExportKeyword | SyntaxKind::DefaultKeyword
            )
        })
    }

    fn filter_modifiers(
        &mut self,
        modifiers: Option<NodeArrayId>,
        keep: impl Fn(SyntaxKind) -> bool,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(modifiers) =
            modifiers.and_then(|id| self.context.arena().node_array_ref(self.source, id))
        else {
            return Ok(None);
        };
        let retained = self
            .context
            .arena()
            .node_array(modifiers)?
            .nodes
            .iter()
            .filter_map(|id| self.context.arena().node_ref(self.source, *id))
            .filter(|modifier| {
                self.context
                    .arena()
                    .node(*modifier)
                    .is_ok_and(|modifier| keep(modifier.kind))
            })
            .collect::<Vec<_>>();
        if retained.is_empty() {
            Ok(None)
        } else {
            Ok(Some(
                self.context
                    .factory()?
                    .update_node_array(modifiers, retained)?
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
                .is_ok_and(|modifier| modifier.kind == expected)
        }))
    }

    fn property_name_expression(
        &mut self,
        member: TransformNode,
        name: NodeId,
    ) -> Result<TransformNode, TransformError> {
        let original_member = self.context.arena().get_original_node(member).node();
        if let Some(generated) = self.computed_names.get(&original_member).cloned() {
            return self.create_identifier(&generated);
        }
        let name = self.node(name);
        match self.context.arena().node(name)?.data.clone() {
            NodeData::Identifier(data) => self.create_string_literal(&data.text),
            NodeData::StringLiteral(_) | NodeData::NumericLiteral(_) => {
                self.context.factory()?.clone_node(name)
            }
            NodeData::ComputedPropertyName(data) => {
                let expression = data
                    .expression
                    .and_then(|expression| self.context.arena().node_ref(self.source, expression))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ComputedPropertyName,
                        field: "expression",
                    })?;
                if self.is_simple_inlineable_expression(expression)? {
                    Ok(expression)
                } else {
                    let generated = self.allocate_temp_name();
                    self.computed_names
                        .insert(original_member, generated.clone());
                    self.create_identifier(&generated)
                }
            }
            NodeData::PrivateIdentifier(_) => self.create_identifier(""),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyDeclaration,
                field: "property name",
            }),
        }
    }

    fn rewrite_computed_member_name(
        &mut self,
        member: TransformNode,
        name: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        let original_member = self.context.arena().get_original_node(member).node();
        let Some(generated) = self.computed_names.get(&original_member).cloned() else {
            return Ok(name);
        };
        let Some(name) = name.and_then(|name| self.context.arena().node_ref(self.source, name))
        else {
            return Ok(name);
        };
        let NodeData::ComputedPropertyName(data) = self.context.arena().node(name)?.data.clone()
        else {
            return Ok(Some(name.node()));
        };
        let expression = data
            .expression
            .and_then(|expression| self.context.arena().node_ref(self.source, expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ComputedPropertyName,
                field: "expression",
            })?;
        let target = self.create_identifier(&generated)?;
        let assignment = self.create_assignment(target, expression)?;
        let computed = self.context.factory()?.create_node(
            self.source,
            NodeData::ComputedPropertyName(tsc_syntax::nodes::ComputedPropertyNameData {
                expression: Some(assignment.node()),
            }),
            TransformFlags::NONE,
        )?;
        self.set_original_and_range(computed, name)?;
        Ok(Some(computed.node()))
    }

    fn class_alias_for_reference(
        &self,
        node: TransformNode,
    ) -> Result<Option<String>, TransformError> {
        let original = self.context.arena().get_original_node(node);
        let resolver_node = self.resolver_node(original)?;
        let has_flag = match self.resolver.has_node_check_flag(
            resolver_node,
            NodeCheckFlags::CONSTRUCTOR_REFERENCE.bits() as u32,
        ) {
            Ok(has_flag) => has_flag,
            Err(EmitResolverError::UnknownNode { .. }) => false,
            Err(error) => return Err(error.into()),
        };
        if !has_flag {
            return Ok(None);
        }
        let declaration = match self
            .resolver
            .get_referenced_value_declaration(resolver_node)
        {
            Ok(declaration) => declaration,
            Err(EmitResolverError::UnknownNode { .. }) => None,
            Err(error) => return Err(error.into()),
        };
        Ok(
            declaration
                .and_then(|declaration| self.class_aliases.get(&declaration.node()).cloned()),
        )
    }

    fn class_has_static_property_or_block(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for member in self.array_nodes(members)? {
            match &self.context.arena().node(member)?.data {
                NodeData::ClassStaticBlockDeclaration(_) => return Ok(true),
                NodeData::PropertyDeclaration(data)
                    if self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)? =>
                {
                    return Ok(true);
                }
                _ => {}
            }
        }
        Ok(false)
    }

    fn is_simple_inlineable_expression(
        &self,
        expression: TransformNode,
    ) -> Result<bool, TransformError> {
        Ok(matches!(
            self.context.arena().node(expression)?.kind,
            SyntaxKind::Identifier
                | SyntaxKind::StringLiteral
                | SyntaxKind::NumericLiteral
                | SyntaxKind::BigIntLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::TrueKeyword
                | SyntaxKind::FalseKeyword
                | SyntaxKind::NullKeyword
                | SyntaxKind::ThisKeyword
        ))
    }

    fn allocate_class_alias(&mut self, class_name: &str) -> String {
        let base = class_name.trim_end_matches('_');
        let mut ordinal = 1usize;
        loop {
            let candidate = format!("{base}_{ordinal}");
            if self.used_names.insert(candidate.clone()) {
                self.hoisted_names.push(candidate.clone());
                return candidate;
            }
            ordinal += 1;
        }
    }

    fn allocate_generated_class_name(&mut self, base: &str) -> String {
        let mut ordinal = 1usize;
        loop {
            let candidate = format!("{base}_{ordinal}");
            if self.used_names.insert(candidate.clone()) {
                return candidate;
            }
            ordinal += 1;
        }
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
        let hoisted = self.create_variable_statement(declarations, NodeFlags::NONE)?;
        let mut statements = self.array_nodes(data.statements)?;
        let mut position = 0;
        while position < statements.len()
            && is_prologue_statement(self.context.arena(), statements[position])?
        {
            position += 1;
        }
        statements.insert(position, hoisted);
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

    fn create_named_export(&mut self, name: &str) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        let specifier = self.context.factory()?.create_node(
            self.source,
            NodeData::ExportSpecifier(tsc_syntax::nodes::ExportSpecifierData {
                name: Some(name.node()),
                property_name: None,
                is_type_only: false,
            }),
            TransformFlags::NONE,
        )?;
        let elements = self
            .context
            .factory()?
            .create_node_array(self.source, vec![specifier])?;
        let clause = self.context.factory()?.create_node(
            self.source,
            NodeData::NamedExports(tsc_syntax::nodes::NamedExportsData {
                elements: Some(elements.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ExportDeclaration(tsc_syntax::nodes::ExportDeclarationData {
                modifiers: None,
                is_type_only: false,
                export_clause: Some(clause.node()),
                module_specifier: None,
                attributes: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_export_default(&mut self, name: &str) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ExportAssignment(tsc_syntax::nodes::ExportAssignmentData {
                modifiers: None,
                is_export_equals: Some(false),
                expression: Some(name.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_decorate_call(
        &mut self,
        decorators: Vec<TransformNode>,
        target: TransformNode,
        member_name: Option<TransformNode>,
        descriptor: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:decorate",
            false,
            DECORATE_HELPER_TEXT,
            2,
            Vec::new(),
        ))?;
        let decorators = self.create_array_literal(decorators, true)?;
        let mut arguments = vec![decorators, target];
        if let Some(member_name) = member_name {
            arguments.push(member_name);
            if let Some(descriptor) = descriptor {
                arguments.push(descriptor);
            }
        }
        let helper = self.create_identifier("__decorate")?;
        self.create_call(helper, arguments)
    }

    fn create_metadata(
        &mut self,
        key: &str,
        value: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:metadata",
            false,
            METADATA_HELPER_TEXT,
            3,
            Vec::new(),
        ))?;
        let helper = self.create_identifier("__metadata")?;
        let key = self.create_string_literal(key)?;
        self.create_call(helper, vec![key, value])
    }

    fn create_param(
        &mut self,
        index: usize,
        decorator: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:param",
            false,
            PARAM_HELPER_TEXT,
            4,
            Vec::new(),
        ))?;
        let helper = self.create_identifier("__param")?;
        let index = self.create_numeric_literal(&index.to_string())?;
        self.create_call(helper, vec![index, decorator])
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

    fn create_null(&mut self) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_token(
            self.source,
            SyntaxKind::NullKeyword,
            TransformFlags::NONE,
        )
    }

    fn create_void_zero(&mut self) -> Result<TransformNode, TransformError> {
        let zero = self.create_numeric_literal("0")?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VoidExpression(tsc_syntax::nodes::VoidExpressionData {
                expression: Some(zero.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_typeof(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::TypeOfExpression(tsc_syntax::nodes::TypeOfExpressionData {
                expression: Some(expression.node()),
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

    fn create_assignment(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let equals = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::EqualsToken,
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                left: Some(left.node()),
                operator_token: Some(equals.node()),
                right: Some(right.node()),
            }),
            TransformFlags::NONE,
        )
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
        self.context.factory()?.create_node(
            self.source,
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

    fn create_variable_statement(
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
        self.context.factory()?.set_node_flags(list, flags)?;
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

    fn identifier_text(&self, id: NodeId) -> Result<&str, TransformError> {
        match &self.context.arena().node(self.node(id))?.data {
            NodeData::Identifier(data) => Ok(&data.text),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassDeclaration,
                field: "identifier name",
            }),
        }
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

    const fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    const fn array(&self, id: NodeArrayId) -> TransformNodeArray {
        TransformNodeArray::new(self.source, id)
    }
}

impl NodeDataChildVisitor for LegacyDecoratorVisitor<'_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("legacy-decorator child belongs to its transform source")
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
            if self.context.arena().node(self.node(node))?.kind == SyntaxKind::ClassDeclaration {
                visited.extend(self.visit_class_declaration(node)?);
            } else if let Some(node) = self.visit(node)? {
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

#[derive(Clone, Copy)]
enum MetadataFallback {
    Object,
    VoidZero,
}
