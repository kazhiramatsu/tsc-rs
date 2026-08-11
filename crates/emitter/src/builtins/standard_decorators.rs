//! H2.4b standard-decorator lowering.

use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags, ScriptTarget};

use crate::{
    EmitFlags, EmitHelper, InternalEmitFlags, TransformError, TransformFlags, TransformNode,
    TransformNodeArray, TransformRoot, TransformSourceId, TransformationContext, Transformer,
    UnsupportedTransformFeature,
};

use super::{flags_after_update, system::collect_identifier_texts};

const ES_DECORATE_HELPER_TEXT: &str = r#"var __esDecorate = (this && this.__esDecorate) || function (ctor, descriptorIn, decorators, contextIn, initializers, extraInitializers) {
    function accept(f) { if (f !== void 0 && typeof f !== "function") throw new TypeError("Function expected"); return f; }
    var kind = contextIn.kind, key = kind === "getter" ? "get" : kind === "setter" ? "set" : "value";
    var target = !descriptorIn && ctor ? contextIn["static"] ? ctor : ctor.prototype : null;
    var descriptor = descriptorIn || (target ? Object.getOwnPropertyDescriptor(target, contextIn.name) : {});
    var _, done = false;
    for (var i = decorators.length - 1; i >= 0; i--) {
        var context = {};
        for (var p in contextIn) context[p] = p === "access" ? {} : contextIn[p];
        for (var p in contextIn.access) context.access[p] = contextIn.access[p];
        context.addInitializer = function (f) { if (done) throw new TypeError("Cannot add initializers after decoration has completed"); extraInitializers.push(accept(f || null)); };
        var result = (0, decorators[i])(kind === "accessor" ? { get: descriptor.get, set: descriptor.set } : descriptor[key], context);
        if (kind === "accessor") {
            if (result === void 0) continue;
            if (result === null || typeof result !== "object") throw new TypeError("Object expected");
            if (_ = accept(result.get)) descriptor.get = _;
            if (_ = accept(result.set)) descriptor.set = _;
            if (_ = accept(result.init)) initializers.unshift(_);
        }
        else if (_ = accept(result)) {
            if (kind === "field") initializers.unshift(_);
            else descriptor[key] = _;
        }
    }
    if (target) Object.defineProperty(target, contextIn.name, descriptor);
    done = true;
};"#;

const RUN_INITIALIZERS_HELPER_TEXT: &str = r#"var __runInitializers = (this && this.__runInitializers) || function (thisArg, initializers, value) {
    var useValue = arguments.length > 2;
    for (var i = 0; i < initializers.length; i++) {
        value = useValue ? initializers[i].call(thisArg, value) : initializers[i].call(thisArg);
    }
    return useValue ? value : void 0;
};"#;

const PROP_KEY_HELPER_TEXT: &str = r#"var __propKey = (this && this.__propKey) || function (x) {
    return typeof x === "symbol" ? x : "".concat(x);
};"#;

/// tsc-port: transformESDecorators @6.0.3
/// tsc-hash: 620f5815a8ddc5aa6c3143eb97180f9ca852fa847501dc4e326c97bec7724358
/// tsc-span: _tsc.js:98946-100807
pub(super) fn transform_standard_decorators(options: &CompilerOptions) -> Box<dyn Transformer> {
    Box::new(StandardDecoratorTransformer {
        target: options.emit_script_target(),
        use_define_for_class_fields: options.use_define_for_class_fields_effective(),
    })
}

struct StandardDecoratorTransformer {
    target: ScriptTarget,
    use_define_for_class_fields: bool,
}

impl Transformer for StandardDecoratorTransformer {
    fn name(&self) -> &'static str {
        "transformESDecorators"
    }

    fn initialize(&mut self, _context: &mut TransformationContext) -> Result<(), TransformError> {
        if self.target < ScriptTarget::ES2016
            || self.target > ScriptTarget::ES_NEXT
            || (self.target == ScriptTarget::ES_NEXT && self.use_define_for_class_fields)
        {
            return Err(TransformError::UnsupportedCompilerOption {
                option: "standard-decorator transform",
                detail: "the transform is reached below ESNext or by ESNext assignment-mode class fields",
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
        let mut visitor = StandardDecoratorVisitor::new(context, source, self.target);
        let transformed =
            visitor
                .visit(current_root.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "root",
                })?;
        visitor
            .context
            .arena_mut()?
            .replace_root(source, TransformNode::new(source, transformed))?;
        Ok(TransformRoot::SourceFile(source))
    }
}

#[derive(Clone)]
struct PropertyPlan {
    original: TransformNode,
    data: tsc_syntax::nodes::PropertyDeclarationData,
    name: String,
    is_static: bool,
    is_private: bool,
    is_accessor: bool,
    decorators: Vec<TransformNode>,
    decorators_name: String,
    initializers_name: String,
    extra_initializers_name: String,
    descriptor_name: Option<String>,
    backing_name: Option<String>,
    computed_temp_name: Option<String>,
    computed_expression: Option<NodeId>,
}

#[derive(Clone)]
struct ClassDecorationPlan {
    decorators: Vec<TransformNode>,
    decorators_name: String,
    descriptor_name: String,
    extra_initializers_name: String,
    class_this_name: String,
    reference_name: String,
    has_static_initializers: bool,
}

#[derive(Clone)]
enum StaticAccessorReceiver {
    GeneratedBinding(String),
    ClassReference {
        text: String,
        original_name: TransformNode,
        class_owner: TransformNode,
    },
}

impl PropertyPlan {
    const fn decoration_category(&self) -> u8 {
        match (self.is_static, self.is_accessor) {
            (true, true) => 0,
            (false, true) => 1,
            (true, false) => 2,
            (false, false) => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MethodKind {
    Method,
    Getter,
    Setter,
}

impl MethodKind {
    const fn context_name(self) -> &'static str {
        match self {
            Self::Method => "method",
            Self::Getter => "getter",
            Self::Setter => "setter",
        }
    }

    const fn helper_prefix(self) -> &'static str {
        match self {
            Self::Method => "",
            Self::Getter => "get_",
            Self::Setter => "set_",
        }
    }
}

#[derive(Clone)]
struct MethodPlan {
    original: TransformNode,
    name: String,
    is_static: bool,
    is_private: bool,
    kind: MethodKind,
    decorators: Vec<TransformNode>,
    decorators_name: String,
    descriptor_name: Option<String>,
    computed_temp_name: Option<String>,
    computed_expression: Option<NodeId>,
    emitted_name: Option<NodeId>,
}

struct DecorationBlockInputs<'a> {
    plans: &'a [PropertyPlan],
    method_plans: &'a [MethodPlan],
    static_method_extra: Option<&'a str>,
    instance_method_extra: Option<&'a str>,
    class_plan: Option<&'a ClassDecorationPlan>,
    class_super_name: Option<&'a str>,
    metadata_name: &'a str,
    has_static_initializers: bool,
}

/// Bindings whose lifetime is the class-definition wrapper rather than the
/// class body. Keeping the declaration plan separate from expression rewriting
/// makes it impossible to create a cached receiver without also declaring it.
#[derive(Default)]
struct DecoratorDefinitionBindings {
    temporary_names: Vec<String>,
    outer_this_name: Option<String>,
}

impl DecoratorDefinitionBindings {
    fn record_temporary(&mut self, name: String) {
        self.temporary_names.push(name);
    }
}

/// Rewrites only lexical `this` references evaluated while defining a class.
/// Ordinary functions and nested classes establish their own `this` boundary;
/// arrows intentionally do not.
struct DecoratorLexicalThisRewriter<'visitor, 'context> {
    visitor: &'visitor mut StandardDecoratorVisitor<'context>,
    bindings: &'visitor mut DecoratorDefinitionBindings,
    nodes: BTreeMap<NodeId, NodeId>,
    arrays: BTreeMap<NodeArrayId, NodeArrayId>,
}

struct StandardDecoratorVisitor<'context> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    target: ScriptTarget,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
    inferred_class_names: BTreeMap<NodeId, String>,
    expanded_classes: BTreeMap<NodeId, Vec<NodeId>>,
    used_names: BTreeSet<String>,
    generated_reference_names: BTreeSet<String>,
    computed_temp_ordinal: usize,
}

impl<'context> StandardDecoratorVisitor<'context> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        target: ScriptTarget,
    ) -> Self {
        let used_names = collect_identifier_texts(context.arena(), source);
        Self {
            context,
            source,
            target,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
            inferred_class_names: BTreeMap::new(),
            expanded_classes: BTreeMap::new(),
            used_names,
            generated_reference_names: BTreeSet::new(),
            computed_temp_ordinal: 0,
        }
    }

    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if let Some(mapped) = self.nodes.get(&id) {
            return Ok(*mapped);
        }
        let original = self.node(id);
        let record = self.context.arena().node(original)?.clone();
        let transformed = match record.data {
            NodeData::VariableDeclaration(data) => {
                self.record_variable_class_name(&data)?;
                Some(self.update_generic(original, NodeData::VariableDeclaration(data))?)
            }
            NodeData::ClassExpression(data)
                if self.class_is_decorated_like(data.modifiers, data.members)? =>
            {
                Some(self.transform_class_expression(original, data)?.node())
            }
            NodeData::MethodDeclaration(mut data) => {
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_generic(original, NodeData::MethodDeclaration(data))?)
            }
            NodeData::GetAccessor(mut data) => {
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_generic(original, NodeData::GetAccessor(data))?)
            }
            NodeData::SetAccessor(mut data) => {
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_generic(original, NodeData::SetAccessor(data))?)
            }
            NodeData::Decorator(_) => {
                return Err(TransformError::UnsupportedSyntax {
                    feature: UnsupportedTransformFeature::Decorators,
                    node: original,
                });
            }
            NodeData::Token => Some(id),
            data => Some(self.update_generic(original, data)?),
        };
        self.nodes.insert(id, transformed);
        Ok(transformed)
    }

    fn record_variable_class_name(
        &mut self,
        data: &tsc_syntax::nodes::VariableDeclarationData,
    ) -> Result<(), TransformError> {
        let Some(initializer) = data.initializer else {
            return Ok(());
        };
        let initializer = self.node(initializer);
        if !matches!(
            self.context.arena().node(initializer)?.data,
            NodeData::ClassExpression(_)
        ) {
            return Ok(());
        }
        let Some(name) = data.name else {
            return Ok(());
        };
        if let Some(text) = self.identifier_text(self.node(name))? {
            self.inferred_class_names
                .insert(initializer.node(), text.to_owned());
        }
        Ok(())
    }

    fn visit_class_declaration(
        &mut self,
        id: NodeId,
    ) -> Result<Vec<TransformNode>, TransformError> {
        if let Some(expanded) = self.expanded_classes.get(&id) {
            return Ok(expanded.iter().copied().map(|id| self.node(id)).collect());
        }
        let original = self.node(id);
        let record = self.context.arena().node(original)?.clone();
        let NodeData::ClassDeclaration(mut data) = record.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: record.kind,
                field: "class declaration",
            });
        };
        if !self.class_is_decorated_like(data.modifiers, data.members)? {
            let updated = self.update_generic(original, NodeData::ClassDeclaration(data))?;
            self.nodes.insert(id, Some(updated));
            self.expanded_classes.insert(id, vec![updated]);
            return Ok(vec![self.node(updated)]);
        }

        let is_export = self.has_modifier(data.modifiers, SyntaxKind::ExportKeyword)?;
        let is_default = self.has_modifier(data.modifiers, SyntaxKind::DefaultKeyword)?;
        let explicit_name = data
            .name
            .and_then(|name| self.identifier_text(self.node(name)).ok().flatten())
            .map(str::to_owned);
        if explicit_name.is_none() && !(is_export && is_default) {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassDeclaration,
                field: "name",
            });
        }
        data.modifiers = self.filter_modifiers(data.modifiers, |kind| {
            !matches!(kind, SyntaxKind::ExportKeyword | SyntaxKind::DefaultKeyword)
        })?;
        let transform_flags = self.context.arena().transform_flags(original);
        let expression = self.context.factory()?.create_node(
            self.source,
            NodeData::ClassExpression(tsc_syntax::nodes::ClassExpressionData {
                name: data.name,
                type_parameters: None,
                heritage_clauses: data.heritage_clauses,
                members: data.members,
                modifiers: data.modifiers,
            }),
            transform_flags,
        )?;
        self.set_original_and_range(expression, original)?;
        if explicit_name.is_none() {
            self.inferred_class_names
                .insert(expression.node(), "default".to_owned());
        }
        let NodeData::ClassExpression(expression_data) =
            self.context.arena().node(expression)?.data.clone()
        else {
            unreachable!("created class expression")
        };
        let transformed = self.transform_class_expression(expression, expression_data)?;
        let mut statements = Vec::new();
        if let Some(name) = explicit_name.as_deref() {
            let declaration = self.create_variable_declaration(name, Some(transformed))?;
            let statement = self
                .create_variable_statement_from_declarations(vec![declaration], NodeFlags::LET)?;
            self.set_original_and_range(statement, original)?;
            statements.push(statement);
            if is_export {
                statements.push(if is_default {
                    self.create_export_default(name)?
                } else {
                    self.create_named_export(name)?
                });
            }
        } else {
            statements.push(self.create_export_default_expression(transformed)?);
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

    fn class_is_decorated_like(
        &self,
        modifiers: Option<NodeArrayId>,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        if !self.decorator_expressions(modifiers)?.is_empty() {
            return Ok(true);
        }
        self.class_has_decorated_element(members)
    }

    fn class_has_decorated_element(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for member in self.array_nodes(members)? {
            let modifiers = match &self.context.arena().node(member)?.data {
                NodeData::PropertyDeclaration(data) => data.modifiers,
                NodeData::MethodDeclaration(data) => data.modifiers,
                NodeData::GetAccessor(data) => data.modifiers,
                NodeData::SetAccessor(data) => data.modifiers,
                _ => None,
            };
            if !self.decorator_expressions(modifiers)?.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn class_has_static_initializers(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for member in self.array_nodes(members)? {
            match &self.context.arena().node(member)?.data {
                NodeData::ClassStaticBlockDeclaration(_)
                    if self
                        .context
                        .arena()
                        .metadata(member)
                        .is_none_or(|metadata| {
                            metadata.assigned_name.is_none() && metadata.class_this.is_none()
                        }) =>
                {
                    return Ok(true);
                }
                NodeData::PropertyDeclaration(data)
                    if self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)?
                        && (data.initializer.is_some()
                            || !self.decorator_expressions(data.modifiers)?.is_empty()) =>
                {
                    return Ok(true);
                }
                _ => {}
            }
        }
        Ok(false)
    }

    fn member_starts_static_initialization(
        &self,
        member: TransformNode,
    ) -> Result<bool, TransformError> {
        Ok(match &self.context.arena().node(member)?.data {
            NodeData::ClassStaticBlockDeclaration(_) => true,
            NodeData::PropertyDeclaration(data) => {
                self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)?
                    && data.initializer.is_some()
            }
            _ => false,
        })
    }

    fn collect_method_plan(
        &mut self,
        original: TransformNode,
        name: Option<NodeId>,
        modifiers: Option<NodeArrayId>,
        kind: MethodKind,
        plans: &mut Vec<MethodPlan>,
    ) -> Result<(), TransformError> {
        let decorators = self.decorator_expressions(modifiers)?;
        if decorators.is_empty() {
            return Ok(());
        }
        let is_private = self.name_is_private(name)?;
        let (name, computed_temp_name, computed_expression) = self.decorator_property_name(name)?;
        let is_static = self.has_modifier(modifiers, SyntaxKind::StaticKeyword)?;
        let static_prefix = if is_static { "static_" } else { "" };
        let private_prefix = if is_private { "private_" } else { "" };
        let kind_prefix = kind.helper_prefix();
        let helper_name = name.trim_start_matches('#');
        let decorators_name = self.allocate_name(&format!(
            "_{static_prefix}{private_prefix}{kind_prefix}{helper_name}_decorators"
        ));
        let descriptor_name = is_private.then(|| {
            self.allocate_name(&format!(
                "_{static_prefix}{private_prefix}{kind_prefix}{helper_name}_descriptor"
            ))
        });
        plans.push(MethodPlan {
            original,
            name,
            is_static,
            is_private,
            kind,
            decorators,
            decorators_name,
            descriptor_name,
            computed_temp_name,
            computed_expression,
            emitted_name: None,
        });
        Ok(())
    }

    fn transform_class_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let class_scope_names = self.used_names.clone();
        let enclosing_temp_ordinal = self.computed_temp_ordinal;
        self.computed_temp_ordinal = 0;
        let class_decorators = self.decorator_expressions(data.modifiers)?;
        let explicit_class_name_node = data.name.map(|name| self.node(name));
        let explicit_class_name = if let Some(name) = data.name {
            self.identifier_text(self.node(name))?.map(str::to_owned)
        } else {
            None
        };
        let explicitly_assigned_name = self.explicitly_assigned_class_name(original)?;
        let assigned_class_name = explicitly_assigned_name
            .clone()
            .or_else(|| self.inferred_class_names.get(&original.node()).cloned());
        let class_name = explicit_class_name
            .clone()
            .or_else(|| assigned_class_name.clone());
        // A decorated named class still receives its name from the direct
        // variable initializer while native static blocks survive. Below
        // ES2022, class-field lowering extracts the `_classThis = this`
        // block and makes the anonymous class the right side of another
        // assignment, so that named-evaluation position no longer survives.
        let emitted_binding_infers_name = explicit_class_name.is_some()
            && (class_decorators.is_empty() || self.target >= ScriptTarget::ES2022);
        let needs_set_function_name = !emitted_binding_infers_name && class_name.is_some();
        let mut class_decoration = if class_decorators.is_empty() {
            None
        } else {
            let assigned_name = needs_set_function_name.then(|| {
                assigned_class_name
                    .clone()
                    .unwrap_or_else(|| "default".to_owned())
            });
            let reference_name = explicit_class_name.clone().unwrap_or_else(|| {
                let base = if assigned_name.as_deref() == Some("default") {
                    "default"
                } else {
                    "class"
                };
                self.allocate_generated_reference_name(base)
            });
            Some(ClassDecorationPlan {
                decorators: class_decorators,
                decorators_name: self.allocate_name("_classDecorators"),
                descriptor_name: self.allocate_name("_classDescriptor"),
                extra_initializers_name: self.allocate_name("_classExtraInitializers"),
                class_this_name: self.allocate_name("_classThis"),
                reference_name,
                has_static_initializers: self.class_has_static_initializers(data.members)?,
            })
        };
        let original_members = self.array_nodes(data.members)?;
        let mut plans = Vec::new();
        let mut method_plans = Vec::new();
        let mut used_private = self.collect_private_names(data.members)?;
        for member in &original_members {
            match self.context.arena().node(*member)?.data.clone() {
                NodeData::PropertyDeclaration(member_data) => {
                    let decorators = self.decorator_expressions(member_data.modifiers)?;
                    if decorators.is_empty() {
                        continue;
                    }
                    let is_private = self.name_is_private(member_data.name)?;
                    let (name, computed_temp_name, computed_expression) =
                        self.decorator_property_name(member_data.name)?;
                    let is_static =
                        self.has_modifier(member_data.modifiers, SyntaxKind::StaticKeyword)?;
                    let is_accessor =
                        self.has_modifier(member_data.modifiers, SyntaxKind::AccessorKeyword)?;
                    let static_prefix = if is_static { "static_" } else { "" };
                    let private_prefix = if is_private { "private_" } else { "" };
                    let helper_name = name.trim_start_matches('#');
                    let decorators_name = self.allocate_name(&format!(
                        "_{static_prefix}{private_prefix}{helper_name}_decorators"
                    ));
                    let initializers_name = self.allocate_name(&format!(
                        "_{static_prefix}{private_prefix}{helper_name}_initializers"
                    ));
                    let extra_initializers_name = self.allocate_name(&format!(
                        "_{static_prefix}{private_prefix}{helper_name}_extraInitializers"
                    ));
                    let descriptor_name = (is_private && is_accessor).then(|| {
                        self.allocate_name(&format!(
                            "_{static_prefix}{private_prefix}{helper_name}_descriptor"
                        ))
                    });
                    let backing_name = is_accessor.then(|| {
                        if computed_expression.is_some() {
                            self.allocate_computed_private_storage(&mut used_private)
                        } else {
                            self.allocate_private_storage(&name, &mut used_private)
                        }
                    });
                    plans.push(PropertyPlan {
                        original: *member,
                        data: member_data,
                        name,
                        is_static,
                        is_private,
                        is_accessor,
                        decorators,
                        decorators_name,
                        initializers_name,
                        extra_initializers_name,
                        descriptor_name,
                        backing_name,
                        computed_temp_name,
                        computed_expression,
                    });
                }
                NodeData::MethodDeclaration(member_data) => self.collect_method_plan(
                    *member,
                    member_data.name,
                    member_data.modifiers,
                    MethodKind::Method,
                    &mut method_plans,
                )?,
                NodeData::GetAccessor(member_data) => self.collect_method_plan(
                    *member,
                    member_data.name,
                    member_data.modifiers,
                    MethodKind::Getter,
                    &mut method_plans,
                )?,
                NodeData::SetAccessor(member_data) => self.collect_method_plan(
                    *member,
                    member_data.name,
                    member_data.modifiers,
                    MethodKind::Setter,
                    &mut method_plans,
                )?,
                _ => {
                    if self.node_has_decorators(*member)? {
                        return Err(TransformError::UnsupportedSyntax {
                            feature: UnsupportedTransformFeature::Decorators,
                            node: *member,
                        });
                    }
                }
            }
        }
        let class_super = self.prepare_class_super(&mut data.heritage_clauses)?;
        let (class_definition_bindings, computed_name_block) = self
            .prepare_decorators_and_computed_names(
                class_decoration.as_mut(),
                &mut plans,
                &mut method_plans,
            )?;
        let static_method_extra = method_plans
            .iter()
            .any(|plan| plan.is_static)
            .then(|| self.allocate_name("_staticExtraInitializers"));
        let instance_method_extra = method_plans
            .iter()
            .any(|plan| !plan.is_static)
            .then(|| self.allocate_name("_instanceExtraInitializers"));
        let needs_descriptor_names = method_plans
            .iter()
            .any(|plan| plan.descriptor_name.is_some())
            || plans.iter().any(|plan| plan.descriptor_name.is_some());
        self.request_helpers(
            needs_set_function_name || needs_descriptor_names,
            !method_plans.is_empty(),
        )?;
        if plans.iter().any(|plan| plan.computed_temp_name.is_some())
            || method_plans
                .iter()
                .any(|plan| plan.computed_temp_name.is_some())
        {
            self.request_prop_key_helper()?;
        }

        let metadata_name = self.allocate_name("_metadata");
        let mut definitions = Vec::new();
        if let Some(name) = class_definition_bindings.outer_this_name.as_deref() {
            let initializer = self.create_this()?;
            definitions.push(self.create_let(name, Some(initializer))?);
        }
        if !class_definition_bindings.temporary_names.is_empty() {
            let mut declarations =
                Vec::with_capacity(class_definition_bindings.temporary_names.len());
            for name in &class_definition_bindings.temporary_names {
                declarations.push(self.create_variable_declaration(name, None)?);
            }
            definitions.push(
                self.create_variable_statement_from_declarations(declarations, NodeFlags::NONE)?,
            );
        }
        if let Some(class_plan) = class_decoration.as_ref() {
            let mut decorators = Vec::with_capacity(class_plan.decorators.len());
            for decorator in &class_plan.decorators {
                let visited =
                    self.visit(decorator.node())?
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::Decorator,
                            field: "expression",
                        })?;
                decorators.push(self.node(visited));
            }
            let decorators = self.create_array_literal(decorators, false)?;
            definitions.push(self.create_let(&class_plan.decorators_name, Some(decorators))?);
            definitions.push(self.create_let(&class_plan.descriptor_name, None)?);
            let empty = self.create_array_literal(Vec::new(), false)?;
            definitions.push(self.create_let(&class_plan.extra_initializers_name, Some(empty))?);
            definitions.push(self.create_let(&class_plan.class_this_name, None)?);
        }
        if let Some((name, initializer)) = class_super.as_ref() {
            definitions.push(self.create_let(name, Some(*initializer))?);
        }
        if let Some(name) = static_method_extra.as_deref() {
            let empty = self.create_array_literal(Vec::new(), false)?;
            definitions.push(self.create_let(name, Some(empty))?);
        }
        if let Some(name) = instance_method_extra.as_deref() {
            let empty = self.create_array_literal(Vec::new(), false)?;
            definitions.push(self.create_let(name, Some(empty))?);
        }
        let mut declaration_order = Vec::with_capacity(plans.len() + method_plans.len());
        for (index, plan) in plans.iter().enumerate() {
            declaration_order.push((
                !plan.is_static,
                self.context.arena().node(plan.original)?.pos,
                false,
                index,
            ));
        }
        for (index, plan) in method_plans.iter().enumerate() {
            declaration_order.push((
                !plan.is_static,
                self.context.arena().node(plan.original)?.pos,
                true,
                index,
            ));
        }
        declaration_order.sort_by_key(|entry| *entry);
        for (_, _, is_method, index) in declaration_order {
            if is_method {
                let plan = &method_plans[index];
                definitions.push(self.create_let(&plan.decorators_name, None)?);
                if let Some(descriptor_name) = plan.descriptor_name.as_deref() {
                    definitions.push(self.create_let(descriptor_name, None)?);
                }
            } else {
                let plan = &plans[index];
                definitions.push(self.create_let(&plan.decorators_name, None)?);
                let empty = self.create_array_literal(Vec::new(), false)?;
                definitions.push(self.create_let(&plan.initializers_name, Some(empty))?);
                let empty = self.create_array_literal(Vec::new(), false)?;
                definitions.push(self.create_let(&plan.extra_initializers_name, Some(empty))?);
                if let Some(descriptor_name) = plan.descriptor_name.as_deref() {
                    definitions.push(self.create_let(descriptor_name, None)?);
                }
            }
        }

        let named_evaluation_member = original_members.iter().copied().find(|member| {
            self.context
                .arena()
                .metadata(*member)
                .and_then(|metadata| metadata.assigned_name)
                .is_some()
        });
        let mut transformed_members = Vec::new();
        if let Some(class_plan) = class_decoration.as_ref() {
            transformed_members
                .push(self.create_class_this_assignment_block(&class_plan.class_this_name)?);
        }
        if let Some(class_name) = class_name
            .as_deref()
            .filter(|_| needs_set_function_name && explicitly_assigned_name.is_none())
        {
            let target = class_decoration
                .as_ref()
                .map(|plan| plan.class_this_name.as_str());
            transformed_members.push(self.create_set_function_name_block(class_name, target)?);
        }
        if let Some(member) = named_evaluation_member {
            let visited =
                self.visit(member.node())?
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ClassExpression,
                        field: "named-evaluation helper block",
                    })?;
            transformed_members.push(self.node(visited));
        }
        if let Some(block) = computed_name_block {
            transformed_members.push(block);
        }
        let has_static_initializers = self.class_has_static_initializers(data.members)?;
        transformed_members.push(self.create_decoration_block(DecorationBlockInputs {
            plans: &plans,
            method_plans: &method_plans,
            static_method_extra: static_method_extra.as_deref(),
            instance_method_extra: instance_method_extra.as_deref(),
            class_plan: class_decoration.as_ref(),
            class_super_name: class_super.as_ref().map(|(name, _)| name.as_str()),
            metadata_name: &metadata_name,
            has_static_initializers,
        })?);

        let plans_by_node = plans
            .iter()
            .enumerate()
            .map(|(index, plan)| (plan.original.node(), index))
            .collect::<BTreeMap<_, _>>();
        let method_plans_by_node = method_plans
            .iter()
            .enumerate()
            .map(|(index, plan)| (plan.original.node(), index))
            .collect::<BTreeMap<_, _>>();
        let mut pending_instance = instance_method_extra;
        let mut pending_static = static_method_extra.filter(|_| has_static_initializers);
        let mut constructor_index = None;
        for member in original_members {
            if Some(member) == named_evaluation_member {
                continue;
            }
            let planned_static_property = plans_by_node
                .get(&member.node())
                .is_some_and(|index| plans[*index].is_static);
            if !planned_static_property
                && self.member_starts_static_initialization(member)?
                && pending_static.is_some()
            {
                let extra = pending_static
                    .take()
                    .expect("pending static initializer checked");
                let statement = if let Some(class_plan) = class_decoration.as_ref() {
                    self.create_run_initializers_statement_with_target(
                        &class_plan.class_this_name,
                        &extra,
                    )?
                } else {
                    self.create_run_initializers_statement(&extra)?
                };
                transformed_members.push(self.create_static_block(vec![statement], true)?);
            }
            if let Some(index) = method_plans_by_node.get(&member.node()).copied() {
                let plan = method_plans[index].clone();
                let transformed = if plan.is_private {
                    self.create_private_method_forwarder(&plan)?
                } else {
                    self.update_public_method(&plan)?
                };
                transformed_members.push(transformed);
                continue;
            }
            let Some(index) = plans_by_node.get(&member.node()).copied() else {
                let visited =
                    self.visit(member.node())?
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ClassExpression,
                            field: "member",
                        })?;
                if self.context.arena().node(self.node(visited))?.kind == SyntaxKind::Constructor {
                    constructor_index = Some(transformed_members.len());
                }
                transformed_members.push(self.node(visited));
                continue;
            };
            let plan = plans[index].clone();
            let pending = if plan.is_static {
                pending_static.take()
            } else {
                pending_instance.take()
            };
            let static_target = if plan.is_static {
                class_decoration
                    .as_ref()
                    .map(|class_plan| class_plan.class_this_name.as_str())
            } else {
                None
            };
            let static_accessor_receiver = if plan.is_static && self.target < ScriptTarget::ES_NEXT
            {
                if let Some(class_plan) = class_decoration.as_ref() {
                    Some(StaticAccessorReceiver::GeneratedBinding(
                        class_plan.class_this_name.clone(),
                    ))
                } else if let (Some(text), Some(original_name)) =
                    (explicit_class_name.as_ref(), explicit_class_name_node)
                {
                    Some(StaticAccessorReceiver::ClassReference {
                        text: text.clone(),
                        original_name,
                        class_owner: original,
                    })
                } else {
                    class_name
                        .as_ref()
                        .map(|name| StaticAccessorReceiver::GeneratedBinding(name.clone()))
                }
            } else {
                None
            };
            let initializer =
                self.create_decorated_initializer(&plan, pending.as_deref(), static_target)?;
            if plan.is_accessor {
                let backing_name =
                    plan.backing_name
                        .as_deref()
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::PropertyDeclaration,
                            field: "auto-accessor backing name",
                        })?;
                transformed_members.extend(self.create_auto_accessor_members(
                    &plan,
                    backing_name,
                    initializer,
                    static_accessor_receiver.as_ref(),
                )?);
            } else {
                transformed_members.push(self.update_decorated_property(&plan, initializer)?);
            }
            if plan.is_static {
                pending_static = Some(plan.extra_initializers_name.clone());
            } else {
                pending_instance = Some(plan.extra_initializers_name.clone());
            }
        }

        if let Some(extra) = pending_instance {
            let statement = self.create_run_initializers_statement(&extra)?;
            if let Some(index) = constructor_index {
                transformed_members[index] =
                    self.inject_constructor_statement(transformed_members[index], statement)?;
            } else {
                transformed_members
                    .push(self.create_constructor(vec![statement], class_super.is_some())?);
            }
        }
        if let Some(extra) = pending_static {
            let statement = if let Some(class_plan) = class_decoration.as_ref() {
                self.create_run_initializers_statement_with_target(
                    &class_plan.class_this_name,
                    &extra,
                )?
            } else {
                self.create_run_initializers_statement(&extra)?
            };
            transformed_members.push(self.create_static_block(vec![statement], true)?);
        }
        if let Some(class_plan) = class_decoration
            .as_ref()
            .filter(|plan| plan.has_static_initializers)
        {
            let statement = self.create_run_initializers_statement_with_target(
                &class_plan.class_this_name,
                &class_plan.extra_initializers_name,
            )?;
            transformed_members.push(self.create_static_block(vec![statement], true)?);
        }

        data.name = if class_decoration.is_some() {
            None
        } else {
            self.visit_optional_node(data.name)?
        };
        data.type_parameters = None;
        data.heritage_clauses = self.visit_optional_nodes(data.heritage_clauses)?;
        data.modifiers = self.strip_decorators(data.modifiers)?;
        let class_this_metadata = transformed_members.iter().find_map(|member| {
            self.context
                .arena()
                .metadata(*member)
                .and_then(|metadata| metadata.class_this)
        });
        let assigned_name_metadata = transformed_members.iter().find_map(|member| {
            self.context
                .arena()
                .metadata(*member)
                .and_then(|metadata| metadata.assigned_name)
        });
        let members = self
            .context
            .factory()?
            .create_node_array(self.source, transformed_members)?;
        data.members = Some(members.array());
        let flags = flags_after_update(
            self.context.arena(),
            original,
            &NodeData::ClassExpression(data.clone()),
        )?;
        let class = self.context.factory()?.update_node(
            original,
            NodeData::ClassExpression(data),
            flags,
        )?;
        if let Some(class_this) = class_this_metadata {
            self.context.arena_mut()?.metadata_mut(class).class_this = Some(class_this);
        }
        if let Some(assigned_name) = assigned_name_metadata {
            self.context.arena_mut()?.metadata_mut(class).assigned_name = Some(assigned_name);
        }
        if let Some(class_plan) = class_decoration.as_ref() {
            let declaration =
                self.create_variable_declaration(&class_plan.reference_name, Some(class))?;
            definitions.push(
                self.create_variable_statement_from_declarations(
                    vec![declaration],
                    NodeFlags::NONE,
                )?,
            );
            let reference = self.create_identifier(&class_plan.reference_name)?;
            let class_this = self.create_identifier(&class_plan.class_this_name)?;
            let assignment = self.create_assignment(reference, class_this)?;
            definitions.push(self.create_return_statement(assignment)?);
        } else {
            definitions.push(self.create_return_statement(class)?);
        }
        let body = self.create_block(definitions, true)?;
        let arrow = self.create_arrow(Vec::new(), body)?;
        let arrow = self.create_parenthesized(arrow)?;
        let call = self.create_call(arrow, Vec::new())?;
        let result = self.set_original_and_range(call, original)?;
        self.used_names = class_scope_names;
        self.computed_temp_ordinal = enclosing_temp_ordinal;
        Ok(result)
    }

    fn create_decoration_block(
        &mut self,
        inputs: DecorationBlockInputs<'_>,
    ) -> Result<TransformNode, TransformError> {
        let DecorationBlockInputs {
            plans,
            method_plans,
            static_method_extra,
            instance_method_extra,
            class_plan,
            class_super_name,
            metadata_name,
            has_static_initializers,
        } = inputs;
        let mut statements = Vec::new();
        let metadata = self.create_metadata_initializer(class_super_name)?;
        statements.push(self.create_variable_statement(
            metadata_name,
            Some(metadata),
            NodeFlags::CONST,
        )?);
        let mut assignments = Vec::with_capacity(plans.len() + method_plans.len());
        for (index, plan) in plans.iter().enumerate() {
            assignments.push((self.context.arena().node(plan.original)?.pos, false, index));
        }
        for (index, plan) in method_plans.iter().enumerate() {
            assignments.push((self.context.arena().node(plan.original)?.pos, true, index));
        }
        assignments.sort_by_key(|(position, is_method, index)| (*position, *is_method, *index));
        for (_, is_method, index) in assignments {
            if if is_method {
                method_plans[index].computed_temp_name.is_some()
            } else {
                plans[index].computed_temp_name.is_some()
            } {
                continue;
            }
            let (decorator_nodes, decorators_name) = if is_method {
                (
                    method_plans[index].decorators.clone(),
                    method_plans[index].decorators_name.clone(),
                )
            } else {
                (
                    plans[index].decorators.clone(),
                    plans[index].decorators_name.clone(),
                )
            };
            let array = self.create_decorator_array(&decorator_nodes)?;
            let target = self.create_identifier(&decorators_name)?;
            let assignment = self.create_assignment(target, array)?;
            statements.push(self.create_expression_statement(assignment)?);
        }
        let mut decoration_order = Vec::with_capacity(plans.len() + method_plans.len());
        for (index, plan) in plans.iter().enumerate() {
            decoration_order.push((
                plan.decoration_category(),
                self.context.arena().node(plan.original)?.pos,
                false,
                index,
            ));
        }
        for (index, plan) in method_plans.iter().enumerate() {
            decoration_order.push((
                if plan.is_static { 0 } else { 1 },
                self.context.arena().node(plan.original)?.pos,
                true,
                index,
            ));
        }
        decoration_order.sort_by_key(|(category, position, is_method, index)| {
            (*category, *position, *is_method, *index)
        });
        for (_, _, is_method, index) in decoration_order {
            if is_method {
                let extra = if method_plans[index].is_static {
                    static_method_extra
                } else {
                    instance_method_extra
                }
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ClassExpression,
                    field: "method extra initializers",
                })?;
                statements.push(self.create_method_es_decorate_statement(
                    &method_plans[index],
                    metadata_name,
                    extra,
                )?);
            } else {
                statements.push(self.create_es_decorate_statement(&plans[index], metadata_name)?);
            }
        }
        if let Some(class_plan) = class_plan {
            statements.push(self.create_class_decorate_statement(class_plan, metadata_name)?);
            statements.push(self.create_class_replacement_statement(class_plan)?);
        }
        statements.push(self.create_metadata_definition(
            metadata_name,
            class_plan.map(|plan| plan.class_this_name.as_str()),
        )?);
        if let Some(static_method_extra) = static_method_extra.filter(|_| !has_static_initializers)
        {
            statements.push(if let Some(class_plan) = class_plan {
                self.create_run_initializers_statement_with_target(
                    &class_plan.class_this_name,
                    static_method_extra,
                )?
            } else {
                self.create_run_initializers_statement(static_method_extra)?
            });
        }
        if let Some(class_plan) = class_plan.filter(|plan| !plan.has_static_initializers) {
            statements.push(self.create_run_initializers_statement_with_target(
                &class_plan.class_this_name,
                &class_plan.extra_initializers_name,
            )?);
        }
        self.create_static_block(statements, true)
    }

    fn prepare_class_super(
        &mut self,
        heritage_clauses: &mut Option<NodeArrayId>,
    ) -> Result<Option<(String, TransformNode)>, TransformError> {
        let Some(clauses_id) = *heritage_clauses else {
            return Ok(None);
        };
        let original_clauses = self.array(clauses_id);
        let mut clauses = self.array_nodes(Some(clauses_id))?;
        for clause in &mut clauses {
            let clause_node = *clause;
            let NodeData::HeritageClause(mut clause_data) =
                self.context.arena().node(clause_node)?.data.clone()
            else {
                continue;
            };
            if clause_data.token != SyntaxKind::ExtendsKeyword {
                continue;
            }
            let Some(extends_type) = self.array_nodes(clause_data.types)?.first().copied() else {
                continue;
            };
            let NodeData::ExpressionWithTypeArguments(mut extends_data) =
                self.context.arena().node(extends_type)?.data.clone()
            else {
                continue;
            };
            let expression =
                extends_data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ExpressionWithTypeArguments,
                        field: "expression",
                    })?;
            let initializer = self
                .visit(expression)?
                .map(|expression| self.node(expression))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ExpressionWithTypeArguments,
                    field: "expression",
                })?;
            let name = self.allocate_name("_classSuper");
            let reference = self.create_identifier(&name)?;
            extends_data.expression = Some(reference.node());
            extends_data.type_arguments = None;
            let flags = flags_after_update(
                self.context.arena(),
                extends_type,
                &NodeData::ExpressionWithTypeArguments(extends_data.clone()),
            )?;
            let extends_type = self.context.factory()?.update_node(
                extends_type,
                NodeData::ExpressionWithTypeArguments(extends_data),
                flags,
            )?;
            let types = self
                .context
                .factory()?
                .create_node_array(self.source, vec![extends_type])?;
            clause_data.types = Some(types.array());
            let flags = flags_after_update(
                self.context.arena(),
                clause_node,
                &NodeData::HeritageClause(clause_data.clone()),
            )?;
            *clause = self.context.factory()?.update_node(
                clause_node,
                NodeData::HeritageClause(clause_data),
                flags,
            )?;
            let clauses = self
                .context
                .factory()?
                .update_node_array(original_clauses, clauses)?;
            *heritage_clauses = Some(clauses.array());
            return Ok(Some((name, initializer)));
        }
        Ok(None)
    }

    fn prepare_decorators_and_computed_names(
        &mut self,
        class_plan: Option<&mut ClassDecorationPlan>,
        plans: &mut [PropertyPlan],
        method_plans: &mut [MethodPlan],
    ) -> Result<(DecoratorDefinitionBindings, Option<TransformNode>), TransformError> {
        let mut bindings = DecoratorDefinitionBindings::default();
        if let Some(class_plan) = class_plan {
            class_plan.decorators =
                self.transform_decorator_expressions(&class_plan.decorators, &mut bindings)?;
        }

        let mut order = Vec::with_capacity(plans.len() + method_plans.len());
        for (index, plan) in plans.iter().enumerate() {
            order.push((self.context.arena().node(plan.original)?.pos, false, index));
        }
        for (index, plan) in method_plans.iter().enumerate() {
            order.push((self.context.arena().node(plan.original)?.pos, true, index));
        }
        order.sort_by_key(|(position, is_method, index)| (*position, *is_method, *index));

        let mut pending = Vec::new();
        for (_, is_method, index) in order {
            if is_method {
                let decorators = method_plans[index].decorators.clone();
                method_plans[index].decorators =
                    self.transform_decorator_expressions(&decorators, &mut bindings)?;
            } else {
                let decorators = plans[index].decorators.clone();
                plans[index].decorators =
                    self.transform_decorator_expressions(&decorators, &mut bindings)?;
            }

            let (expression, decorators, decorators_name, survives) = if is_method {
                let plan = &method_plans[index];
                (
                    plan.computed_expression,
                    plan.decorators.clone(),
                    plan.decorators_name.clone(),
                    true,
                )
            } else {
                let plan = &plans[index];
                (
                    plan.computed_expression,
                    plan.decorators.clone(),
                    plan.decorators_name.clone(),
                    plan.is_accessor,
                )
            };
            let Some(expression) = expression else {
                continue;
            };
            let temporary_name = self.allocate_computed_temp_name();
            bindings.record_temporary(temporary_name.clone());
            if is_method {
                method_plans[index].computed_temp_name = Some(temporary_name.clone());
            } else {
                plans[index].computed_temp_name = Some(temporary_name.clone());
            }

            let decorators = self.create_decorator_array(&decorators)?;
            let decorators_target = self.create_identifier(&decorators_name)?;
            pending.push(self.create_assignment(decorators_target, decorators)?);

            let expression = self
                .visit(expression)?
                .map(|expression| self.node(expression))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ComputedPropertyName,
                    field: "expression",
                })?;
            let helper = self.create_identifier("__propKey")?;
            let key = self.create_call(helper, vec![expression])?;
            let temporary = self.create_identifier(&temporary_name)?;
            pending.push(self.create_assignment(temporary, key)?);

            let cached = self.create_identifier(&temporary_name)?;
            let cached_name = self.create_computed_property_name(cached)?;
            if is_method {
                method_plans[index].emitted_name = Some(cached_name.node());
            } else {
                plans[index].data.name = Some(cached_name.node());
            }

            if survives {
                let expressions = std::mem::take(&mut pending);
                let expression = self.inline_expressions(expressions)?;
                let expression = self.create_parenthesized(expression)?;
                let emitted_name = self.create_computed_property_name(expression)?;
                if is_method {
                    method_plans[index].emitted_name = Some(emitted_name.node());
                } else {
                    plans[index].data.name = Some(emitted_name.node());
                }
            }
        }

        let block = if pending.is_empty() {
            None
        } else {
            let expression = self.inline_expressions(pending)?;
            let statement = self.create_expression_statement(expression)?;
            Some(self.create_static_block(vec![statement], false)?)
        };
        Ok((bindings, block))
    }

    fn transform_decorator_expressions(
        &mut self,
        decorators: &[TransformNode],
        bindings: &mut DecoratorDefinitionBindings,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut output = Vec::with_capacity(decorators.len());
        for decorator in decorators {
            let visited = self
                .visit(decorator.node())?
                .map(|decorator| self.node(decorator))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Decorator,
                    field: "expression",
                })?;
            let bound = self.bind_decorator_expression(visited, bindings)?;
            output.push(self.rewrite_decorator_lexical_this(bound, bindings)?);
        }
        Ok(output)
    }

    fn bind_decorator_expression(
        &mut self,
        expression: TransformNode,
        bindings: &mut DecoratorDefinitionBindings,
    ) -> Result<TransformNode, TransformError> {
        let data = self.context.arena().node(expression)?.data.clone();
        let (target, receiver) = match data {
            NodeData::ParenthesizedExpression(mut data) => {
                let inner = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ParenthesizedExpression,
                        field: "expression",
                    })?;
                let bound = self.bind_decorator_expression(self.node(inner), bindings)?;
                data.expression = Some(bound.node());
                return self.update_decorator_outer_expression(
                    expression,
                    NodeData::ParenthesizedExpression(data),
                );
            }
            NodeData::PartiallyEmittedExpression(mut data) => {
                let inner = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PartiallyEmittedExpression,
                        field: "expression",
                    })?;
                let bound = self.bind_decorator_expression(self.node(inner), bindings)?;
                data.expression = Some(bound.node());
                return self.update_decorator_outer_expression(
                    expression,
                    NodeData::PartiallyEmittedExpression(data),
                );
            }
            NodeData::PropertyAccessExpression(mut data) => {
                let receiver = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyAccessExpression,
                        field: "expression",
                    })?;
                let receiver = self.node(receiver);
                let (bound_receiver, this_arg) =
                    self.bind_decorator_receiver(receiver, bindings)?;
                data.expression = Some(bound_receiver.node());
                let flags = flags_after_update(
                    self.context.arena(),
                    expression,
                    &NodeData::PropertyAccessExpression(data.clone()),
                )?;
                let target = self.context.factory()?.update_node(
                    expression,
                    NodeData::PropertyAccessExpression(data),
                    flags,
                )?;
                (target, this_arg)
            }
            NodeData::ElementAccessExpression(mut data) => {
                let receiver = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ElementAccessExpression,
                        field: "expression",
                    })?;
                let receiver = self.node(receiver);
                let (bound_receiver, this_arg) =
                    self.bind_decorator_receiver(receiver, bindings)?;
                data.expression = Some(bound_receiver.node());
                let flags = flags_after_update(
                    self.context.arena(),
                    expression,
                    &NodeData::ElementAccessExpression(data.clone()),
                )?;
                let target = self.context.factory()?.update_node(
                    expression,
                    NodeData::ElementAccessExpression(data),
                    flags,
                )?;
                (target, this_arg)
            }
            _ => return Ok(expression),
        };
        let bind = self.create_property_access(target, "bind")?;
        self.create_call(bind, vec![receiver])
    }

    fn update_decorator_outer_expression(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<TransformNode, TransformError> {
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        self.context.factory()?.update_node(original, data, flags)
    }

    fn bind_decorator_receiver(
        &mut self,
        receiver: TransformNode,
        bindings: &mut DecoratorDefinitionBindings,
    ) -> Result<(TransformNode, TransformNode), TransformError> {
        if self.decorator_receiver_is_super(receiver)? {
            return Ok((receiver, self.create_this()?));
        }
        if !self.decorator_receiver_needs_cache(receiver)? {
            return Ok((receiver, receiver));
        }

        let receiver_name = self.allocate_computed_temp_name();
        bindings.record_temporary(receiver_name.clone());
        let temporary = self.create_identifier(&receiver_name)?;
        let assignment = self.create_assignment(temporary, receiver)?;
        let assignment = self.create_parenthesized(assignment)?;
        let this_arg = self.create_identifier(&receiver_name)?;
        Ok((assignment, this_arg))
    }

    fn decorator_receiver_is_super(&self, receiver: TransformNode) -> Result<bool, TransformError> {
        let receiver = self.skip_parenthesized_expression(receiver)?;
        Ok(self.context.arena().node(receiver)?.kind == SyntaxKind::SuperKeyword)
    }

    fn decorator_receiver_needs_cache(
        &self,
        receiver: TransformNode,
    ) -> Result<bool, TransformError> {
        let receiver = self.skip_parenthesized_expression(receiver)?;
        let record = self.context.arena().node(receiver)?;
        Ok(match &record.data {
            // Decorator references deliberately cache identifiers. This is the
            // observable distinction from ordinary call binding in tsc.
            NodeData::Identifier(_) => true,
            NodeData::Token
                if matches!(
                    record.kind,
                    SyntaxKind::ThisKeyword
                        | SyntaxKind::NumericLiteral
                        | SyntaxKind::BigIntLiteral
                        | SyntaxKind::StringLiteral
                ) =>
            {
                false
            }
            NodeData::NumericLiteral(_)
            | NodeData::BigIntLiteral(_)
            | NodeData::StringLiteral(_) => false,
            NodeData::ArrayLiteralExpression(data) => !self.node_array_is_empty(data.elements)?,
            NodeData::ObjectLiteralExpression(data) => {
                !self.node_array_is_empty(data.properties)?
            }
            _ => true,
        })
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

    fn node_array_is_empty(&self, array: Option<NodeArrayId>) -> Result<bool, TransformError> {
        let Some(array) = array else {
            return Ok(true);
        };
        Ok(self
            .context
            .arena()
            .node_array(self.array(array))?
            .nodes
            .is_empty())
    }

    fn rewrite_decorator_lexical_this(
        &mut self,
        expression: TransformNode,
        bindings: &mut DecoratorDefinitionBindings,
    ) -> Result<TransformNode, TransformError> {
        DecoratorLexicalThisRewriter::new(self, bindings).rewrite(expression)
    }

    fn create_decorator_array(
        &mut self,
        decorators: &[TransformNode],
    ) -> Result<TransformNode, TransformError> {
        self.create_array_literal(decorators.to_vec(), false)
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

    fn create_metadata_initializer(
        &mut self,
        class_super_name: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let symbol = self.create_identifier("Symbol")?;
        let type_of_symbol = self.create_typeof(symbol)?;
        let function = self.create_string_literal("function")?;
        let is_function = self.create_binary(
            type_of_symbol,
            SyntaxKind::EqualsEqualsEqualsToken,
            function,
        )?;
        let symbol = self.create_identifier("Symbol")?;
        let metadata = self.create_property_access(symbol, "metadata")?;
        let condition =
            self.create_binary(is_function, SyntaxKind::AmpersandAmpersandToken, metadata)?;
        let object = self.create_identifier("Object")?;
        let create = self.create_property_access(object, "create")?;
        let prototype = if let Some(class_super_name) = class_super_name {
            let class_super = self.create_identifier(class_super_name)?;
            let symbol = self.create_identifier("Symbol")?;
            let metadata = self.create_property_access(symbol, "metadata")?;
            let inherited = self.create_element_access(class_super, metadata)?;
            let null = self.create_null()?;
            self.create_binary(inherited, SyntaxKind::QuestionQuestionToken, null)?
        } else {
            self.create_null()?
        };
        let when_true = self.create_call(create, vec![prototype])?;
        let when_false = self.create_void_zero()?;
        self.create_conditional(condition, when_true, when_false)
    }

    fn create_metadata_definition(
        &mut self,
        metadata_name: &str,
        target_name: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let object = self.create_identifier("Object")?;
        let define = self.create_property_access(object, "defineProperty")?;
        let target = if let Some(target_name) = target_name {
            self.create_identifier(target_name)?
        } else {
            self.create_this()?
        };
        let symbol = self.create_identifier("Symbol")?;
        let symbol_metadata = self.create_property_access(symbol, "metadata")?;
        let enumerable = self.create_true()?;
        let configurable = self.create_true()?;
        let writable = self.create_true()?;
        let value = self.create_identifier(metadata_name)?;
        let properties = vec![
            self.create_property("enumerable", enumerable)?,
            self.create_property("configurable", configurable)?,
            self.create_property("writable", writable)?,
            self.create_property("value", value)?,
        ];
        let descriptor = self.create_object_literal(properties, false)?;
        let call = self.create_call(define, vec![target, symbol_metadata, descriptor])?;
        let statement = self.create_expression_statement(call)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::SINGLE_LINE);
        let condition = self.create_identifier(metadata_name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::IfStatement(tsc_syntax::nodes::IfStatementData {
                expression: Some(condition.node()),
                then_statement: Some(statement.node()),
                else_statement: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_es_decorate_statement(
        &mut self,
        plan: &PropertyPlan,
        metadata_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let helper = self.create_identifier("__esDecorate")?;
        let ctor = if plan.is_accessor {
            self.create_this()?
        } else {
            self.create_null()?
        };
        let descriptor = if let Some(descriptor_name) = plan.descriptor_name.as_deref() {
            let descriptor = self.create_private_accessor_descriptor(plan)?;
            let descriptor_name = self.create_identifier(descriptor_name)?;
            self.create_assignment(descriptor_name, descriptor)?
        } else {
            self.create_null()?
        };
        let decorators = self.create_identifier(&plan.decorators_name)?;
        let context = self.create_decorator_context(plan, metadata_name)?;
        let initializers = self.create_identifier(&plan.initializers_name)?;
        let extra = self.create_identifier(&plan.extra_initializers_name)?;
        let call = self.create_call(
            helper,
            vec![ctor, descriptor, decorators, context, initializers, extra],
        )?;
        self.create_expression_statement(call)
    }

    fn create_method_es_decorate_statement(
        &mut self,
        plan: &MethodPlan,
        metadata_name: &str,
        extra_initializers_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let helper = self.create_identifier("__esDecorate")?;
        let ctor = self.create_this()?;
        let descriptor = if let Some(descriptor_name) = plan.descriptor_name.as_deref() {
            let descriptor = self.create_private_method_descriptor(plan)?;
            let descriptor_name = self.create_identifier(descriptor_name)?;
            self.create_assignment(descriptor_name, descriptor)?
        } else {
            self.create_null()?
        };
        let decorators = self.create_identifier(&plan.decorators_name)?;
        let context = self.create_method_decorator_context(plan, metadata_name)?;
        let initializers = self.create_null()?;
        let extra = self.create_identifier(extra_initializers_name)?;
        let call = self.create_call(
            helper,
            vec![ctor, descriptor, decorators, context, initializers, extra],
        )?;
        self.create_expression_statement(call)
    }

    fn create_private_method_descriptor(
        &mut self,
        plan: &MethodPlan,
    ) -> Result<TransformNode, TransformError> {
        let data = self.context.arena().node(plan.original)?.data.clone();
        let (parameters, body, asterisk_token, modifiers) = match data {
            NodeData::MethodDeclaration(data) => (
                self.visit_optional_nodes(data.parameters)?,
                self.visit_optional_node(data.body)?,
                self.visit_optional_node(data.asterisk_token)?,
                self.filter_modifiers(data.modifiers, |kind| kind == SyntaxKind::AsyncKeyword)?,
            ),
            NodeData::GetAccessor(data) => (
                Some(
                    self.context
                        .factory()?
                        .create_node_array(self.source, Vec::new())?
                        .array(),
                ),
                self.visit_optional_node(data.body)?,
                None,
                None,
            ),
            NodeData::SetAccessor(data) => (
                self.visit_optional_nodes(data.parameters)?,
                self.visit_optional_node(data.body)?,
                None,
                None,
            ),
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ClassExpression,
                    field: "private decorated method",
                });
            }
        };
        let body = if let Some(body) = body {
            self.node(body)
        } else {
            self.create_block(Vec::new(), false)?
        };
        let function =
            self.create_function_expression(parameters, body, asterisk_token, modifiers)?;
        let prefix = match plan.kind {
            MethodKind::Method => None,
            MethodKind::Getter => Some("get"),
            MethodKind::Setter => Some("set"),
        };
        let named = self.create_set_function_name(function, &plan.name, prefix)?;
        let property_name = match plan.kind {
            MethodKind::Method => "value",
            MethodKind::Getter => "get",
            MethodKind::Setter => "set",
        };
        let property = self.create_property(property_name, named)?;
        self.create_object_literal(vec![property], false)
    }

    fn create_private_accessor_descriptor(
        &mut self,
        plan: &PropertyPlan,
    ) -> Result<TransformNode, TransformError> {
        let backing_name =
            plan.backing_name
                .as_deref()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyDeclaration,
                    field: "private accessor backing name",
                })?;
        let empty_parameters = self
            .context
            .factory()?
            .create_node_array(self.source, Vec::new())?;
        let this = self.create_this()?;
        let backing = self.create_private_identifier(backing_name)?;
        let access = self.create_property_access_node(this, backing)?;
        let statement = self.create_return_statement(access)?;
        let body = self.create_block(vec![statement], false)?;
        let getter =
            self.create_function_expression(Some(empty_parameters.array()), body, None, None)?;
        let getter = self.create_set_function_name(getter, &plan.name, Some("get"))?;

        let value = self.create_parameter("value")?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, vec![value])?;
        let this = self.create_this()?;
        let backing = self.create_private_identifier(backing_name)?;
        let target = self.create_property_access_node(this, backing)?;
        let value = self.create_identifier("value")?;
        let assignment = self.create_assignment(target, value)?;
        let statement = self.create_expression_statement(assignment)?;
        let body = self.create_block(vec![statement], false)?;
        let setter = self.create_function_expression(Some(parameters.array()), body, None, None)?;
        let setter = self.create_set_function_name(setter, &plan.name, Some("set"))?;

        let getter = self.create_property("get", getter)?;
        let setter = self.create_property("set", setter)?;
        self.create_object_literal(vec![getter, setter], false)
    }

    fn create_set_function_name(
        &mut self,
        function: TransformNode,
        name: &str,
        prefix: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let helper = self.create_identifier("__setFunctionName")?;
        let name = self.create_string_literal(name)?;
        let mut arguments = vec![function, name];
        if let Some(prefix) = prefix {
            arguments.push(self.create_string_literal(prefix)?);
        }
        self.create_call(helper, arguments)
    }

    fn create_private_method_forwarder(
        &mut self,
        plan: &MethodPlan,
    ) -> Result<TransformNode, TransformError> {
        let data = self.context.arena().node(plan.original)?.data.clone();
        let (name, modifiers) = match &data {
            NodeData::MethodDeclaration(data) => (data.name, data.modifiers),
            NodeData::GetAccessor(data) => (data.name, data.modifiers),
            NodeData::SetAccessor(data) => (data.name, data.modifiers),
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ClassExpression,
                    field: "private method forwarder",
                });
            }
        };
        let name = name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ClassExpression,
            field: "private method name",
        })?;
        let modifiers =
            self.filter_modifiers(modifiers, |kind| kind == SyntaxKind::StaticKeyword)?;
        let descriptor_name =
            plan.descriptor_name
                .as_deref()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ClassExpression,
                    field: "private method descriptor",
                })?;
        let descriptor = self.create_identifier(descriptor_name)?;
        let property_name = match plan.kind {
            MethodKind::Method => "value",
            MethodKind::Getter => "get",
            MethodKind::Setter => "set",
        };
        let descriptor_property = self.create_property_access(descriptor, property_name)?;
        let expression = if plan.kind == MethodKind::Method {
            descriptor_property
        } else {
            let call_method = self.create_property_access(descriptor_property, "call")?;
            let this = self.create_this()?;
            let mut arguments = vec![this];
            if plan.kind == MethodKind::Setter {
                arguments.push(self.create_identifier("value")?);
            }
            self.create_call(call_method, arguments)?
        };
        let statement = self.create_return_statement(expression)?;
        let body = self.create_block(vec![statement], false)?;
        let result = if plan.kind == MethodKind::Setter {
            let value = self.create_parameter("value")?;
            let parameters = self
                .context
                .factory()?
                .create_node_array(self.source, vec![value])?;
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
            )?
        } else {
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
            )?
        };
        self.set_original_and_range(result, plan.original)
    }

    fn update_public_method(&mut self, plan: &MethodPlan) -> Result<TransformNode, TransformError> {
        let data = self.context.arena().node(plan.original)?.data.clone();
        let data = match data {
            NodeData::MethodDeclaration(mut data) => {
                if let Some(name) = plan.emitted_name {
                    data.name = Some(name);
                }
                data.modifiers = self.strip_decorators(data.modifiers)?;
                NodeData::MethodDeclaration(data)
            }
            NodeData::GetAccessor(mut data) => {
                if let Some(name) = plan.emitted_name {
                    data.name = Some(name);
                }
                data.modifiers = self.strip_decorators(data.modifiers)?;
                NodeData::GetAccessor(data)
            }
            NodeData::SetAccessor(mut data) => {
                if let Some(name) = plan.emitted_name {
                    data.name = Some(name);
                }
                data.modifiers = self.strip_decorators(data.modifiers)?;
                NodeData::SetAccessor(data)
            }
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ClassExpression,
                    field: "public decorated method",
                });
            }
        };
        let updated = self.update_generic(plan.original, data)?;
        Ok(self.node(updated))
    }

    fn create_class_decorate_statement(
        &mut self,
        plan: &ClassDecorationPlan,
        metadata_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let helper = self.create_identifier("__esDecorate")?;
        let ctor = self.create_null()?;
        let class_this = self.create_identifier(&plan.class_this_name)?;
        let value = self.create_property("value", class_this)?;
        let descriptor = self.create_object_literal(vec![value], false)?;
        let descriptor_name = self.create_identifier(&plan.descriptor_name)?;
        let descriptor = self.create_assignment(descriptor_name, descriptor)?;
        let decorators = self.create_identifier(&plan.decorators_name)?;
        let kind = self.create_string_literal("class")?;
        let class_this = self.create_identifier(&plan.class_this_name)?;
        let name = self.create_property_access(class_this, "name")?;
        let metadata = self.create_identifier(metadata_name)?;
        let kind_property = self.create_property("kind", kind)?;
        let name_property = self.create_property("name", name)?;
        let metadata_property = self.create_property("metadata", metadata)?;
        let context = self
            .create_object_literal(vec![kind_property, name_property, metadata_property], false)?;
        let initializers = self.create_null()?;
        let extra = self.create_identifier(&plan.extra_initializers_name)?;
        let call = self.create_call(
            helper,
            vec![ctor, descriptor, decorators, context, initializers, extra],
        )?;
        self.create_expression_statement(call)
    }

    fn create_class_replacement_statement(
        &mut self,
        plan: &ClassDecorationPlan,
    ) -> Result<TransformNode, TransformError> {
        let descriptor = self.create_identifier(&plan.descriptor_name)?;
        let value = self.create_property_access(descriptor, "value")?;
        let class_this = self.create_identifier(&plan.class_this_name)?;
        let class_this_assignment = self.create_assignment(class_this, value)?;
        let reference = self.create_identifier(&plan.reference_name)?;
        let assignment = self.create_assignment(reference, class_this_assignment)?;
        self.create_expression_statement(assignment)
    }

    fn create_decorator_context(
        &mut self,
        plan: &PropertyPlan,
        metadata_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let kind = if plan.is_accessor {
            "accessor"
        } else {
            "field"
        };
        let kind = self.create_string_literal(kind)?;
        let name = if let Some(temporary) = plan.computed_temp_name.as_deref() {
            self.create_identifier(temporary)?
        } else {
            self.create_string_literal(&plan.name)?
        };
        let static_ = if plan.is_static {
            self.create_true()?
        } else {
            self.create_false()?
        };
        let private = if plan.is_private {
            self.create_true()?
        } else {
            self.create_false()?
        };
        let access = self.create_access_object(
            &plan.name,
            true,
            true,
            plan.is_private,
            plan.computed_temp_name.as_deref(),
        )?;
        let metadata = self.create_identifier(metadata_name)?;
        let properties = vec![
            self.create_property("kind", kind)?,
            self.create_property("name", name)?,
            self.create_property("static", static_)?,
            self.create_property("private", private)?,
            self.create_property("access", access)?,
            self.create_property("metadata", metadata)?,
        ];
        self.create_object_literal(properties, false)
    }

    fn create_method_decorator_context(
        &mut self,
        plan: &MethodPlan,
        metadata_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let kind = self.create_string_literal(plan.kind.context_name())?;
        let name = if let Some(temporary) = plan.computed_temp_name.as_deref() {
            self.create_identifier(temporary)?
        } else {
            self.create_string_literal(&plan.name)?
        };
        let static_ = if plan.is_static {
            self.create_true()?
        } else {
            self.create_false()?
        };
        let private = if plan.is_private {
            self.create_true()?
        } else {
            self.create_false()?
        };
        let access = self.create_access_object(
            &plan.name,
            plan.kind != MethodKind::Setter,
            plan.kind == MethodKind::Setter,
            plan.is_private,
            plan.computed_temp_name.as_deref(),
        )?;
        let metadata = self.create_identifier(metadata_name)?;
        let properties = vec![
            self.create_property("kind", kind)?,
            self.create_property("name", name)?,
            self.create_property("static", static_)?,
            self.create_property("private", private)?,
            self.create_property("access", access)?,
            self.create_property("metadata", metadata)?,
        ];
        self.create_object_literal(properties, false)
    }

    fn create_access_object(
        &mut self,
        name: &str,
        include_get: bool,
        include_set: bool,
        is_private: bool,
        computed_temp_name: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let obj = self.create_parameter("obj")?;
        let property = if let Some(temporary) = computed_temp_name {
            self.create_identifier(temporary)?
        } else if is_private {
            self.create_private_identifier(name)?
        } else {
            self.create_string_literal(name)?
        };
        let obj_for_has = self.create_identifier("obj")?;
        let has_body = self.create_binary(property, SyntaxKind::InKeyword, obj_for_has)?;
        let has = self.create_arrow(vec![obj], has_body)?;
        let mut properties = vec![self.create_property("has", has)?];
        if include_get {
            let obj = self.create_parameter("obj")?;
            let obj_expression = self.create_identifier("obj")?;
            let get_body = if let Some(temporary) = computed_temp_name {
                let name = self.create_identifier(temporary)?;
                self.create_element_access(obj_expression, name)?
            } else if is_private {
                let name = self.create_private_identifier(name)?;
                self.create_property_access_node(obj_expression, name)?
            } else {
                self.create_property_access(obj_expression, name)?
            };
            let get = self.create_arrow(vec![obj], get_body)?;
            properties.push(self.create_property("get", get)?);
        }
        if include_set {
            let obj = self.create_parameter("obj")?;
            let value = self.create_parameter("value")?;
            let obj_expression = self.create_identifier("obj")?;
            let target = if let Some(temporary) = computed_temp_name {
                let name = self.create_identifier(temporary)?;
                self.create_element_access(obj_expression, name)?
            } else if is_private {
                let name = self.create_private_identifier(name)?;
                self.create_property_access_node(obj_expression, name)?
            } else {
                self.create_property_access(obj_expression, name)?
            };
            let value_expression = self.create_identifier("value")?;
            let assignment = self.create_assignment(target, value_expression)?;
            let statement = self.create_expression_statement(assignment)?;
            let body = self.create_block(vec![statement], false)?;
            let set = self.create_arrow(vec![obj, value], body)?;
            properties.push(self.create_property("set", set)?);
        }
        self.create_object_literal(properties, false)
    }

    fn create_decorated_initializer(
        &mut self,
        plan: &PropertyPlan,
        pending_extra: Option<&str>,
        target_name: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let initializer = if let Some(initializer) = plan.data.initializer {
            let visited = self
                .visit(initializer)?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyDeclaration,
                    field: "initializer",
                })?;
            self.node(visited)
        } else {
            self.create_void_zero()?
        };
        let run = self.create_run_initializers_with_target(
            &plan.initializers_name,
            Some(initializer),
            target_name,
        )?;
        let Some(pending_extra) = pending_extra else {
            return Ok(run);
        };
        let previous =
            self.create_run_initializers_with_target(pending_extra, None, target_name)?;
        let comma = self.create_binary(previous, SyntaxKind::CommaToken, run)?;
        self.create_parenthesized(comma)
    }

    fn update_decorated_property(
        &mut self,
        plan: &PropertyPlan,
        initializer: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let mut data = plan.data.clone();
        data.name = self.visit_optional_node(data.name)?;
        data.modifiers = self.strip_decorators(data.modifiers)?;
        data.initializer = Some(initializer.node());
        let flags = flags_after_update(
            self.context.arena(),
            plan.original,
            &NodeData::PropertyDeclaration(data.clone()),
        )?;
        self.context.factory()?.update_node(
            plan.original,
            NodeData::PropertyDeclaration(data),
            flags,
        )
    }

    fn create_auto_accessor_members(
        &mut self,
        plan: &PropertyPlan,
        backing_name: &str,
        initializer: TransformNode,
        static_receiver: Option<&StaticAccessorReceiver>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let backing = self.create_private_identifier(backing_name)?;
        let modifiers = self.filter_modifiers(plan.data.modifiers, |kind| {
            !matches!(kind, SyntaxKind::Decorator | SyntaxKind::AccessorKeyword)
        })?;
        let field = self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyDeclaration(tsc_syntax::nodes::PropertyDeclarationData {
                name: Some(backing.node()),
                modifiers,
                question_token: None,
                exclamation_token: None,
                r#type: None,
                initializer: Some(initializer.node()),
            }),
            TransformFlags::CONTAINS_CLASS_FIELDS,
        )?;
        self.set_original_and_range(field, plan.original)?;
        self.context
            .arena_mut()?
            .metadata_mut(field)
            .add_flags(EmitFlags::NO_COMMENTS);
        // tsc-port: decorated auto-accessor expansion @6.0.3
        // tsc-hash: a36d5d1d9f385cf80a5379c53a98ff9936ace13b998d268a79af1aaa7b791850
        // tsc-span: _tsc.js:100115-100150
        if plan.is_static && self.target < ScriptTarget::ES2022 {
            self.context
                .arena_mut()?
                .metadata_mut(field)
                .class_field_initializer_comment_source = Some(plan.original);
        }
        let name = plan.data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::PropertyDeclaration,
            field: "name",
        })?;
        let setter_name = if let Some(temporary) = plan.computed_temp_name.as_deref() {
            let temporary = self.create_identifier(temporary)?;
            self.create_computed_property_name(temporary)?.node()
        } else {
            name
        };
        let getter = self.create_get_accessor(
            name,
            backing.node(),
            modifiers,
            plan.descriptor_name.as_deref(),
            static_receiver,
        )?;
        let setter = self.create_set_accessor(
            setter_name,
            backing.node(),
            modifiers,
            plan.descriptor_name.as_deref(),
            static_receiver,
        )?;
        self.set_original_and_range(getter, plan.original)?;
        self.context
            .arena_mut()?
            .metadata_mut(setter)
            .add_flags(EmitFlags::NO_COMMENTS);
        Ok(vec![field, getter, setter])
    }

    fn create_get_accessor(
        &mut self,
        name: NodeId,
        backing: NodeId,
        modifiers: Option<NodeArrayId>,
        descriptor_name: Option<&str>,
        static_receiver: Option<&StaticAccessorReceiver>,
    ) -> Result<TransformNode, TransformError> {
        let access = if let Some(descriptor_name) = descriptor_name {
            let descriptor = self.create_identifier(descriptor_name)?;
            let getter = self.create_property_access(descriptor, "get")?;
            let call = self.create_property_access(getter, "call")?;
            let this = self.create_this()?;
            self.create_call(call, vec![this])?
        } else {
            let receiver = if let Some(receiver) = static_receiver {
                self.create_static_accessor_receiver(receiver)?
            } else {
                self.create_this()?
            };
            self.create_property_access_node(receiver, self.node(backing))?
        };
        let statement = self.create_return_statement(access)?;
        let body = self.create_block(vec![statement], false)?;
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
        backing: NodeId,
        modifiers: Option<NodeArrayId>,
        descriptor_name: Option<&str>,
        static_receiver: Option<&StaticAccessorReceiver>,
    ) -> Result<TransformNode, TransformError> {
        let parameter = self.create_parameter("value")?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, vec![parameter])?;
        let value = self.create_identifier("value")?;
        let statement = if let Some(descriptor_name) = descriptor_name {
            let descriptor = self.create_identifier(descriptor_name)?;
            let setter = self.create_property_access(descriptor, "set")?;
            let call = self.create_property_access(setter, "call")?;
            let this = self.create_this()?;
            let call = self.create_call(call, vec![this, value])?;
            self.create_return_statement(call)?
        } else {
            let receiver = if let Some(receiver) = static_receiver {
                self.create_static_accessor_receiver(receiver)?
            } else {
                self.create_this()?
            };
            let target = self.create_property_access_node(receiver, self.node(backing))?;
            let assignment = self.create_assignment(target, value)?;
            self.create_expression_statement(assignment)?
        };
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

    fn create_static_accessor_receiver(
        &mut self,
        receiver: &StaticAccessorReceiver,
    ) -> Result<TransformNode, TransformError> {
        match receiver {
            StaticAccessorReceiver::GeneratedBinding(text) => self.create_identifier(text),
            StaticAccessorReceiver::ClassReference {
                text,
                original_name,
                class_owner,
            } => {
                let identifier = self.create_identifier(text)?;
                let identifier = self.set_original_and_range(identifier, *original_name)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(identifier)
                    .class_constructor_reference = Some(*class_owner);
                Ok(identifier)
            }
        }
    }

    fn create_class_this_assignment_block(
        &mut self,
        class_this_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let class_this = self.create_identifier(class_this_name)?;
        let this = self.create_this()?;
        let assignment = self.create_assignment(class_this, this)?;
        let statement = self.create_expression_statement(assignment)?;
        let block = self.create_static_block(vec![statement], false)?;
        self.context.arena_mut()?.metadata_mut(block).class_this = Some(class_this);
        Ok(block)
    }

    fn create_set_function_name_block(
        &mut self,
        class_name: &str,
        target_name: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let helper = self.create_identifier("__setFunctionName")?;
        let target = if let Some(target_name) = target_name {
            self.create_identifier(target_name)?
        } else {
            self.create_this()?
        };
        let name = self.create_string_literal(class_name)?;
        let call = self.create_call(helper, vec![target, name])?;
        let statement = self.create_expression_statement(call)?;
        let block = self.create_static_block(vec![statement], false)?;
        self.context.arena_mut()?.metadata_mut(block).assigned_name = Some(name);
        Ok(block)
    }

    fn create_run_initializers(
        &mut self,
        initializers_name: &str,
        value: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.create_run_initializers_with_target(initializers_name, value, None)
    }

    fn create_run_initializers_with_target(
        &mut self,
        initializers_name: &str,
        value: Option<TransformNode>,
        target_name: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let helper = self.create_identifier("__runInitializers")?;
        let this = if let Some(target_name) = target_name {
            self.create_identifier(target_name)?
        } else {
            self.create_this()?
        };
        let initializers = self.create_identifier(initializers_name)?;
        let mut arguments = vec![this, initializers];
        if let Some(value) = value {
            arguments.push(value);
        }
        self.create_call(helper, arguments)
    }

    fn create_run_initializers_statement(
        &mut self,
        name: &str,
    ) -> Result<TransformNode, TransformError> {
        let run = self.create_run_initializers(name, None)?;
        self.create_expression_statement(run)
    }

    fn create_run_initializers_statement_with_target(
        &mut self,
        target_name: &str,
        initializers_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let helper = self.create_identifier("__runInitializers")?;
        let target = self.create_identifier(target_name)?;
        let initializers = self.create_identifier(initializers_name)?;
        let run = self.create_call(helper, vec![target, initializers])?;
        self.create_expression_statement(run)
    }

    fn create_static_block(
        &mut self,
        statements: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let body = self.create_block(statements, multi_line)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ClassStaticBlockDeclaration(
                tsc_syntax::nodes::ClassStaticBlockDeclarationData {
                    body: Some(body.node()),
                    modifiers: None,
                },
            ),
            TransformFlags::NONE,
        )
    }

    fn create_constructor(
        &mut self,
        mut statements: Vec<TransformNode>,
        derived: bool,
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
            let super_token = self.context.factory()?.create_token(
                self.source,
                SyntaxKind::SuperKeyword,
                TransformFlags::CONTAINS_LEXICAL_SUPER,
            )?;
            let call = self.create_call(super_token, vec![spread])?;
            statements.insert(0, self.create_expression_statement(call)?);
        }
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, Vec::new())?;
        let body = self.create_block(statements, true)?;
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

    fn inject_constructor_statement(
        &mut self,
        constructor: TransformNode,
        statement: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::Constructor(mut data) = self.context.arena().node(constructor)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassExpression,
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
        statements.insert(insertion, statement);
        let statements = self
            .context
            .factory()?
            .create_node_array(self.source, statements)?;
        block.statements = Some(statements.array());
        let flags =
            flags_after_update(self.context.arena(), body, &NodeData::Block(block.clone()))?;
        let body = self
            .context
            .factory()?
            .update_node(body, NodeData::Block(block), flags)?;
        data.body = Some(body.node());
        let flags = flags_after_update(
            self.context.arena(),
            constructor,
            &NodeData::Constructor(data.clone()),
        )?;
        self.context
            .factory()?
            .update_node(constructor, NodeData::Constructor(data), flags)
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

    fn request_helpers(
        &mut self,
        set_function_name: bool,
        run_initializers_first: bool,
    ) -> Result<(), TransformError> {
        if run_initializers_first {
            self.request_run_initializers_helper()?;
        }
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:esDecorate",
            false,
            ES_DECORATE_HELPER_TEXT,
            Some(2),
            Vec::new(),
        ))?;
        if !run_initializers_first {
            self.request_run_initializers_helper()?;
        }
        if set_function_name {
            self.context
                .request_emit_helper(super::helpers::set_function_name())?;
        }
        Ok(())
    }

    fn request_run_initializers_helper(&mut self) -> Result<(), TransformError> {
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:runInitializers",
            false,
            RUN_INITIALIZERS_HELPER_TEXT,
            Some(2),
            Vec::new(),
        ))
    }

    fn request_prop_key_helper(&mut self) -> Result<(), TransformError> {
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:propKey",
            false,
            PROP_KEY_HELPER_TEXT,
            None,
            Vec::new(),
        ))
    }

    fn create_let(
        &mut self,
        name: &str,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.create_variable_statement(name, initializer, NodeFlags::LET)
    }

    fn create_variable_statement(
        &mut self,
        name: &str,
        initializer: Option<TransformNode>,
        flags: NodeFlags,
    ) -> Result<TransformNode, TransformError> {
        let declaration = self.create_variable_declaration(name, initializer)?;
        self.create_variable_statement_from_declarations(vec![declaration], flags)
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

    fn create_variable_statement_from_declarations(
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

    fn create_true(&mut self) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_token(
            self.source,
            SyntaxKind::TrueKeyword,
            TransformFlags::NONE,
        )
    }

    fn create_false(&mut self) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_token(
            self.source,
            SyntaxKind::FalseKeyword,
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
        self.create_property_access_node(expression, name)
    }

    fn create_property_access_node(
        &mut self,
        expression: TransformNode,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
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

    fn create_computed_property_name(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = self.context.factory()?.create_node(
            self.source,
            NodeData::ComputedPropertyName(tsc_syntax::nodes::ComputedPropertyNameData {
                expression: Some(expression.node()),
            }),
            TransformFlags::CONTAINS_COMPUTED_PROPERTY_NAME,
        )?;
        self.context
            .arena_mut()?
            .metadata_mut(name)
            .set_internal_flags(InternalEmitFlags::GENERATED_COMPUTED_PROPERTY_NAME);
        Ok(name)
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

    fn create_property(
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

    fn create_arrow(
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
            TransformFlags::NONE,
        )
    }

    fn create_function_expression(
        &mut self,
        parameters: Option<NodeArrayId>,
        body: TransformNode,
        asterisk_token: Option<NodeId>,
        modifiers: Option<NodeArrayId>,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::FunctionExpression(tsc_syntax::nodes::FunctionExpressionData {
                name: None,
                type_parameters: None,
                parameters,
                r#type: None,
                asterisk_token,
                body: Some(body.node()),
                modifiers,
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
        let expression = self.create_identifier(name)?;
        self.create_export_default_expression(expression)
    }

    fn create_export_default_expression(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::ExportAssignment(tsc_syntax::nodes::ExportAssignmentData {
                modifiers: None,
                is_export_equals: Some(false),
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

    fn node_has_decorators(&self, node: TransformNode) -> Result<bool, TransformError> {
        let modifiers = match &self.context.arena().node(node)?.data {
            NodeData::MethodDeclaration(data) => data.modifiers,
            NodeData::GetAccessor(data) => data.modifiers,
            NodeData::SetAccessor(data) => data.modifiers,
            NodeData::Constructor(data) => data.modifiers,
            _ => None,
        };
        Ok(!self.decorator_expressions(modifiers)?.is_empty())
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

    fn strip_decorators(
        &mut self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        self.filter_modifiers(modifiers, |kind| kind != SyntaxKind::Decorator)
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

    fn name_is_private(&self, name: Option<NodeId>) -> Result<bool, TransformError> {
        Ok(name.is_some_and(|name| {
            self.context
                .arena()
                .node(self.node(name))
                .is_ok_and(|name| name.kind == SyntaxKind::PrivateIdentifier)
        }))
    }

    fn decorator_property_name(
        &mut self,
        name: Option<NodeId>,
    ) -> Result<(String, Option<String>, Option<NodeId>), TransformError> {
        let name = name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::PropertyDeclaration,
            field: "name",
        })?;
        match &self.context.arena().node(self.node(name))?.data {
            NodeData::Identifier(data) => Ok((data.text.clone(), None, None)),
            NodeData::PrivateIdentifier(data) => Ok((data.text.clone(), None, None)),
            NodeData::StringLiteral(data) => Ok((data.text.clone(), None, None)),
            NodeData::NumericLiteral(data) => Ok((data.text.clone(), None, None)),
            NodeData::ComputedPropertyName(data) => {
                let expression = data
                    .expression
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ComputedPropertyName,
                        field: "expression",
                    })?;
                let expression = self.node(expression).node();
                Ok(("member".to_owned(), None, Some(expression)))
            }
            _ => Err(TransformError::UnsupportedSyntax {
                feature: UnsupportedTransformFeature::Decorators,
                node: self.node(name),
            }),
        }
    }

    fn identifier_text(&self, node: TransformNode) -> Result<Option<&str>, TransformError> {
        Ok(match &self.context.arena().node(node)?.data {
            NodeData::Identifier(data) => Some(data.text.as_str()),
            _ => None,
        })
    }

    fn explicitly_assigned_class_name(
        &self,
        class: TransformNode,
    ) -> Result<Option<String>, TransformError> {
        let Some(assigned_name) = self
            .context
            .arena()
            .metadata(class)
            .and_then(|metadata| metadata.assigned_name)
        else {
            return Ok(None);
        };
        Ok(match &self.context.arena().node(assigned_name)?.data {
            NodeData::Identifier(data) => Some(data.text.clone()),
            NodeData::StringLiteral(data) => Some(data.text.clone()),
            NodeData::NumericLiteral(data) => Some(data.text.clone()),
            _ => None,
        })
    }

    fn collect_private_names(
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

    fn allocate_name(&mut self, base: &str) -> String {
        if self.used_names.insert(base.to_owned()) {
            return base.to_owned();
        }
        let mut ordinal = 1usize;
        loop {
            let candidate = format!("{base}_{ordinal}");
            if self.used_names.insert(candidate.clone()) {
                return candidate;
            }
            ordinal += 1;
        }
    }

    fn allocate_computed_temp_name(&mut self) -> String {
        loop {
            let ordinal = self.computed_temp_ordinal;
            self.computed_temp_ordinal += 1;
            let candidate = if ordinal < 26 {
                format!("_{}", char::from(b'a' + ordinal as u8))
            } else {
                format!("_{}", ordinal - 26)
            };
            if self.used_names.insert(candidate.clone()) {
                return candidate;
            }
        }
    }

    fn allocate_generated_reference_name(&mut self, base: &str) -> String {
        let stem = if base == "class" {
            "class".to_owned()
        } else {
            base.to_owned()
        };
        let mut ordinal = 1usize;
        loop {
            let candidate = format!("{stem}_{ordinal}");
            if !self.used_names.contains(&candidate)
                && self.generated_reference_names.insert(candidate.clone())
            {
                return candidate;
            }
            ordinal += 1;
        }
    }

    fn allocate_private_storage(&self, name: &str, used: &mut BTreeSet<String>) -> String {
        let name = name.trim_start_matches('#');
        let base = format!("#{name}_accessor_storage");
        if used.insert(base.clone()) {
            return base;
        }
        let mut ordinal = 1usize;
        loop {
            let candidate = format!("#{name}_{ordinal}_accessor_storage");
            if used.insert(candidate.clone()) {
                return candidate;
            }
            ordinal += 1;
        }
    }

    fn allocate_computed_private_storage(&self, used: &mut BTreeSet<String>) -> String {
        let mut ordinal = 0usize;
        loop {
            let stem = if ordinal < 26 {
                format!("_{}", char::from(b'a' + ordinal as u8))
            } else {
                format!("_{}", ordinal - 26)
            };
            let candidate = format!("#{stem}_accessor_storage");
            if used.insert(candidate.clone()) {
                return candidate;
            }
            ordinal += 1;
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

impl<'visitor, 'context> DecoratorLexicalThisRewriter<'visitor, 'context> {
    fn new(
        visitor: &'visitor mut StandardDecoratorVisitor<'context>,
        bindings: &'visitor mut DecoratorDefinitionBindings,
    ) -> Self {
        Self {
            visitor,
            bindings,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
        }
    }

    fn rewrite(mut self, expression: TransformNode) -> Result<TransformNode, TransformError> {
        let rewritten =
            self.rewrite_node(expression.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Decorator,
                    field: "lexical this expression",
                })?;
        Ok(self.visitor.node(rewritten))
    }

    fn rewrite_node(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if let Some(rewritten) = self.nodes.get(&id) {
            return Ok(Some(*rewritten));
        }
        let original = self.visitor.node(id);
        let record = self.visitor.context.arena().node(original)?.clone();
        let rewritten = if record.kind == SyntaxKind::ThisKeyword {
            let name = match self.bindings.outer_this_name.as_ref() {
                Some(name) => name.clone(),
                None => {
                    let name = self.visitor.allocate_name("_outerThis");
                    self.bindings.outer_this_name = Some(name.clone());
                    name
                }
            };
            self.visitor.create_identifier(&name)?.node()
        } else if matches!(&record.data, NodeData::Token)
            || Self::establishes_this_boundary(record.kind)
        {
            original.node()
        } else {
            let mut data = record.data;
            try_visit_each_child(&mut data, self)?;
            let flags = flags_after_update(self.visitor.context.arena(), original, &data)?;
            self.visitor
                .context
                .factory()?
                .update_node(original, data, flags)?
                .node()
        };
        self.nodes.insert(id, rewritten);
        Ok(Some(rewritten))
    }

    const fn establishes_this_boundary(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::ClassDeclaration
                | SyntaxKind::ClassExpression
                | SyntaxKind::Constructor
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::GetAccessor
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::SetAccessor
        )
    }

    fn rewrite_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, TransformError> {
        if let Some(rewritten) = self.arrays.get(&id) {
            return Ok(Some(*rewritten));
        }
        let original = self.visitor.array(id);
        let nodes = self
            .visitor
            .context
            .arena()
            .node_array(original)?
            .nodes
            .clone();
        let mut rewritten_nodes = Vec::with_capacity(nodes.len());
        for node in nodes {
            if let Some(rewritten) = self.rewrite_node(node)? {
                rewritten_nodes.push(self.visitor.node(rewritten));
            }
        }
        let rewritten = self
            .visitor
            .context
            .factory()?
            .update_node_array(original, rewritten_nodes)?
            .array();
        self.arrays.insert(id, rewritten);
        Ok(Some(rewritten))
    }
}

impl NodeDataChildVisitor for DecoratorLexicalThisRewriter<'_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.visitor
            .context
            .arena()
            .node(self.visitor.node(id))
            .expect("decorator expression child belongs to the current transform source")
            .kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.rewrite_node(id)
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        self.rewrite_nodes(id)
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}

impl NodeDataChildVisitor for StandardDecoratorVisitor<'_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("standard-decorator child belongs to the current transform source")
            .kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.visit(id)
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        self.visit_nodes(id)
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}

impl StandardDecoratorVisitor<'_> {
    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, TransformError> {
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
}
