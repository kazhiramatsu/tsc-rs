//! H2.4b standard-decorator lowering.

use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags, ScriptTarget};

use crate::{
    factory::EmitHelperName, CommentRange, EmitFlags, EmitHelper, InternalEmitFlags,
    SourceMapRange, SourceRange, TransformError, TransformFlags, TransformNode, TransformNodeArray,
    TransformRoot, TransformSourceId, TransformationContext, Transformer,
    UnsupportedTransformFeature,
};

use super::{
    constructor_prologue, flags_after_update, system::collect_identifier_texts,
    target_bindings::TargetBinding, ConstructorPrologue,
};

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

/// A child admitted through tsc's `fallbackVisitor` array boundary.
///
/// Decorators are modifier-list markers rather than JavaScript runtime
/// nodes. Invalid placements are still represented by the recovery AST so
/// the checker can report TS1206, but `transformESDecorators` removes the
/// marker while preserving and visiting its owning declaration. Keeping the
/// recovery variant separate means a decorator that somehow reaches the
/// primary visitor still fails closed below, matching tsc's `Debug.fail`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StandardDecoratorFallbackChild {
    Runtime(TransformNode),
    ErasedDecorator,
}

impl Transformer for StandardDecoratorTransformer {
    fn name(&self) -> &'static str {
        "transformESDecorators"
    }

    fn initialize(&mut self, _context: &mut TransformationContext) -> Result<(), TransformError> {
        if self.target < ScriptTarget::ES5
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

    /// tsc-port: transformSourceFile @6.0.3
    /// tsc-hash: 7d3feb5348f0eb42475696e86a21c67b94f94b863eb41a092e280f3bda7f157c
    /// tsc-span: _tsc.js:98960-98970
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
        if visitor.should_transform_private_static_elements_in_file {
            let root = TransformNode::new(source, transformed);
            let internal_flags = visitor
                .context
                .arena()
                .metadata(root)
                .map_or(InternalEmitFlags::NONE, |metadata| {
                    metadata.internal_flags()
                });
            visitor
                .context
                .arena_mut()?
                .metadata_mut(root)
                .set_internal_flags(InternalEmitFlags::from_bits(
                    internal_flags.bits()
                        | InternalEmitFlags::TRANSFORM_PRIVATE_STATIC_ELEMENTS.bits(),
                ));
        }
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
    original: TransformNode,
    decorators: Vec<TransformNode>,
    decorators_name: String,
    descriptor_name: String,
    extra_initializers_name: String,
    class_this_name: String,
    reference: DecoratedClassReferenceBinding,
    has_static_initializers: bool,
}

/// The identity behind the independently materialized names of a decorated
/// class declaration.
///
/// Parsed names clone the declaration identifier and retain its source
/// metadata. A name inserted by `transformTypeScript` instead belongs to the
/// anonymous source class. Its projections therefore share a generated
/// binding rather than pretending that the synthetic identifier was parsed.
#[derive(Clone)]
enum DecoratedClassDeclarationName {
    Parsed {
        text: String,
        declaration_identity: TransformNode,
    },
    TypeScriptGeneratedAnonymousDefault {
        binding: TargetBinding,
        declaration_owner: TransformNode,
    },
}

impl DecoratedClassDeclarationName {
    fn runtime_name(&self) -> DecoratedClassRuntimeName {
        match self {
            Self::Parsed { text, .. } => DecoratedClassRuntimeName::Declared(text.clone()),
            Self::TypeScriptGeneratedAnonymousDefault { .. } => {
                DecoratedClassRuntimeName::AnonymousDefaultDeclaration
            }
        }
    }

    fn class_reference(&self) -> DecoratedClassReferenceBinding {
        match self {
            Self::Parsed {
                text,
                declaration_identity,
            } => DecoratedClassReferenceBinding::Parsed {
                text: text.clone(),
                declaration_identity: *declaration_identity,
            },
            Self::TypeScriptGeneratedAnonymousDefault {
                binding,
                declaration_owner,
            } => DecoratedClassReferenceBinding::Generated {
                binding: binding.clone(),
                declaration_owner: *declaration_owner,
            },
        }
    }
}

/// The source-language name installed on a decorated class is independent of
/// the binding used to publish and reference it. In particular,
/// `transformTypeScript` names an anonymous default declaration `default_N`,
/// while downlevel named evaluation must still install the runtime name
/// `"default"`; an unassigned decorated class expression instead installs
/// the empty string required by `transformESDecorators`.
#[derive(Clone, Debug, Eq, PartialEq)]
enum DecoratedClassRuntimeName {
    Declared(String),
    Assigned(String),
    AnonymousDefaultDeclaration,
    UnassignedDecoratedExpression,
}

impl DecoratedClassRuntimeName {
    fn text(&self) -> &str {
        match self {
            Self::Declared(text) | Self::Assigned(text) => text,
            Self::AnonymousDefaultDeclaration => "default",
            Self::UnassignedDecoratedExpression => "",
        }
    }
}

/// The `getLocalName(..., ignoreAssignedName = true)` projection used inside a
/// class-decorator IIFE.
///
/// Parsed names remain checker-addressable local projections. Anonymous
/// declarations and expressions instead share a generated binding across the
/// IIFE declaration, decoration assignment, and return assignment.
#[derive(Clone)]
enum DecoratedClassReferenceBinding {
    Parsed {
        text: String,
        declaration_identity: TransformNode,
    },
    Generated {
        binding: TargetBinding,
        declaration_owner: TransformNode,
    },
}

impl DecoratedClassReferenceBinding {
    fn planned_text(&self) -> &str {
        match self {
            Self::Parsed { text, .. } => text,
            Self::Generated { binding, .. } => binding.provisional_name(),
        }
    }
}

/// The three `NodeFactory.getName` projections used by
/// `transformESDecorators.visitClassDeclaration`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DecoratedClassNameProjection {
    LocalMapped,
    LocalUnmapped,
    DeclarationUnmapped,
}

impl DecoratedClassNameProjection {
    fn emit_flags(self) -> EmitFlags {
        match self {
            Self::LocalMapped => EmitFlags::LOCAL_NAME | EmitFlags::NO_COMMENTS,
            Self::LocalUnmapped => {
                EmitFlags::LOCAL_NAME | EmitFlags::NO_COMMENTS | EmitFlags::NO_SOURCE_MAP
            }
            Self::DeclarationUnmapped => EmitFlags::NO_COMMENTS | EmitFlags::NO_SOURCE_MAP,
        }
    }
}

/// Entry ownership for `transformClassLike`.
///
/// The generated class expression is semantically linked to the source
/// class-like node in both routes, but the enclosing IIFE is linked only for a
/// source class expression. A class declaration donates its comments to an
/// outer statement selected by `visit_class_declaration` instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DecoratedClassRoute {
    Declaration(TransformNode),
    Expression(TransformNode),
}

impl DecoratedClassRoute {
    const fn original(self) -> TransformNode {
        match self {
            Self::Declaration(original) | Self::Expression(original) => original,
        }
    }

    const fn iife_original(self) -> Option<TransformNode> {
        match self {
            Self::Declaration(_) => None,
            Self::Expression(original) => Some(original),
        }
    }
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
    leading_static_initializers: PendingDecoratorInitializerBatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DecoratorInitializerPlacement {
    Instance,
    Static,
}

#[derive(Clone, Debug)]
enum DecoratorInitializerReceiver {
    LexicalThis,
    ClassBinding(String),
}

#[derive(Clone, Debug)]
enum PendingDecoratorInitializer {
    MethodExtra {
        initializers_name: String,
        class: TransformNode,
    },
    FieldExtra {
        initializers_name: String,
    },
}

impl PendingDecoratorInitializer {
    fn initializers_name(&self) -> &str {
        match self {
            Self::MethodExtra {
                initializers_name, ..
            }
            | Self::FieldExtra { initializers_name } => initializers_name,
        }
    }
}

struct PendingDecoratorInitializerBatch {
    receiver: DecoratorInitializerReceiver,
    initializers: Vec<PendingDecoratorInitializer>,
}

impl PendingDecoratorInitializerBatch {
    fn empty(receiver: DecoratorInitializerReceiver) -> Self {
        Self {
            receiver,
            initializers: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.initializers.is_empty()
    }
}

/// Statement indices from a constructor body through nested `try` blocks to
/// the statement that owns the first reachable `super()` call.
#[derive(Debug)]
struct DecoratorConstructorSuperPath {
    statement_indices: Vec<usize>,
}

/// Upstream gives the constructor's two terminal-initializer paths different
/// statement-list shapes. A reachable `super()` keeps the copied prologue
/// once and inserts on its typed path. The fallback first copies the prologue,
/// then appends the initializer and the complete original body, replaying the
/// prefix. Keeping that distinction in the plan prevents an insertion index
/// from erasing the observable replay.
enum DecoratorConstructorInitializerPlacement {
    AfterSuper(DecoratorConstructorSuperPath),
    ReplayPrologueThenBody(ConstructorPrologue),
}

/// Ordered per-class ownership of standard-decorator initializer effects.
///
/// Method extras seed the placement queue before source-member traversal.
/// Every property drains its queue before a decorated field appends its own
/// extras. A static block and the constructor/trailing static block are the
/// remaining placement-specific consumers.
///
/// tsc-port: createClassInfo @6.0.3
/// tsc-hash: 2457b6249522b8b9349b000eff2caf2d626295f98eedc6ce759c1291f8004185
/// tsc-span: _tsc.js:99241-99318
struct ClassPendingDecoratorInitializers {
    instance: Vec<PendingDecoratorInitializer>,
    static_: Vec<PendingDecoratorInitializer>,
    static_receiver: DecoratorInitializerReceiver,
}

impl ClassPendingDecoratorInitializers {
    fn new(static_receiver: DecoratorInitializerReceiver) -> Self {
        Self {
            instance: Vec::new(),
            static_: Vec::new(),
            static_receiver,
        }
    }

    fn receiver(&self, placement: DecoratorInitializerPlacement) -> DecoratorInitializerReceiver {
        match placement {
            DecoratorInitializerPlacement::Instance => DecoratorInitializerReceiver::LexicalThis,
            DecoratorInitializerPlacement::Static => self.static_receiver.clone(),
        }
    }

    fn enqueue(
        &mut self,
        placement: DecoratorInitializerPlacement,
        initializer: PendingDecoratorInitializer,
    ) {
        self.queue_mut(placement).push(initializer);
    }

    fn drain(
        &mut self,
        placement: DecoratorInitializerPlacement,
    ) -> PendingDecoratorInitializerBatch {
        let receiver = self.receiver(placement);
        let initializers = std::mem::take(self.queue_mut(placement));
        PendingDecoratorInitializerBatch {
            receiver,
            initializers,
        }
    }

    fn queue_mut(
        &mut self,
        placement: DecoratorInitializerPlacement,
    ) -> &mut Vec<PendingDecoratorInitializer> {
        match placement {
            DecoratorInitializerPlacement::Instance => &mut self.instance,
            DecoratorInitializerPlacement::Static => &mut self.static_,
        }
    }
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

/// Substitutes the semantic class-definition identity into an existing named
/// evaluation helper block. Earlier transforms spell that helper against
/// lexical `this`; once a class decorator allocates `_classThis`, tsc's class
/// element visitor projects every lexical `this` in the helper through that
/// generated identity instead.
struct DecoratorClassThisRewriter<'visitor, 'context> {
    visitor: &'visitor mut StandardDecoratorVisitor<'context>,
    class_this: TransformNode,
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
    should_transform_private_static_elements_in_file: bool,
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
            should_transform_private_static_elements_in_file: false,
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
                Some(
                    self.transform_class_like(
                        DecoratedClassRoute::Expression(original),
                        None,
                        data,
                    )?
                    .node(),
                )
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
        let declaration_name = self.decorated_class_declaration_name(original, data.name)?;
        if declaration_name.is_none() && !(is_export && is_default) {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassDeclaration,
                field: "name",
            });
        }
        let original_modifiers = data.modifiers;
        data.modifiers = self.filter_modifiers(data.modifiers, |kind| {
            !matches!(kind, SyntaxKind::ExportKeyword | SyntaxKind::DefaultKeyword)
        })?;
        let transformed = self.transform_class_like(
            DecoratedClassRoute::Declaration(original),
            declaration_name.as_ref(),
            tsc_syntax::nodes::ClassExpressionData {
                name: data.name,
                type_parameters: None,
                heritage_clauses: data.heritage_clauses,
                members: data.members,
                modifiers: data.modifiers,
            },
        )?;
        let mut statements = Vec::new();
        if let Some(name) = declaration_name.as_ref() {
            let local_projection = if is_export && is_default {
                DecoratedClassNameProjection::LocalUnmapped
            } else {
                DecoratedClassNameProjection::LocalMapped
            };
            let local_name = self.materialize_decorated_declaration_name(name, local_projection)?;
            let declaration =
                self.create_variable_declaration_with_name(local_name, Some(transformed))?;
            self.set_original_only(declaration, original)?;
            let statement = self
                .create_variable_statement_from_declarations(vec![declaration], NodeFlags::LET)?;
            if is_export && is_default {
                statements.push(statement);
                let declaration_name = self.materialize_decorated_declaration_name(
                    name,
                    DecoratedClassNameProjection::DeclarationUnmapped,
                )?;
                let export = self.create_export_default_expression(declaration_name)?;
                self.set_declaration_comment_owner(export, original)?;
                self.set_source_map_range_past_decorators(export, original, original_modifiers)?;
                statements.push(export);
            } else {
                self.set_declaration_comment_owner(statement, original)?;
                statements.push(statement);
                if is_export {
                    let export = self.create_named_export(name)?;
                    self.set_original_only(export, original)?;
                    statements.push(export);
                }
            }
        } else {
            let export = self.create_export_default_expression(transformed)?;
            self.set_declaration_comment_owner(export, original)?;
            self.set_source_map_range_past_decorators(export, original, original_modifiers)?;
            statements.push(export);
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

    /// tsc-port: transformClassLike @6.0.3
    /// tsc-hash: 81ca1253994891573290dc0bff9915660cf030bc5f7a1b69ae5f71288ebe1d43
    /// tsc-span: _tsc.js:99335-99375
    fn transform_class_like(
        &mut self,
        route: DecoratedClassRoute,
        declaration_name: Option<&DecoratedClassDeclarationName>,
        mut data: tsc_syntax::nodes::ClassExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let original = route.original();
        debug_assert!(
            matches!(route, DecoratedClassRoute::Declaration(_)) || declaration_name.is_none()
        );
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
        let class_evaluation_binding_name = explicit_class_name
            .clone()
            .or_else(|| assigned_class_name.clone());
        let runtime_class_name = match route {
            DecoratedClassRoute::Declaration(_) => Some(declaration_name.map_or(
                DecoratedClassRuntimeName::AnonymousDefaultDeclaration,
                DecoratedClassDeclarationName::runtime_name,
            )),
            DecoratedClassRoute::Expression(_) => self.class_expression_runtime_name(
                original,
                explicit_class_name_node,
                explicit_class_name.as_deref(),
                assigned_class_name.as_deref(),
                !class_decorators.is_empty(),
            )?,
        };
        let has_static_private_class_elements = self.array_nodes(data.members)?.iter().try_fold(
            false,
            |found, member| -> Result<bool, TransformError> {
                Ok(found || self.is_private_static_class_element(*member)?)
            },
        )?;
        // A decorated named class still receives its name from the direct
        // variable initializer while native static blocks survive. Below
        // ES2022, class-field lowering extracts the `_classThis = this`
        // block and makes the anonymous class the right side of another
        // assignment, so that named-evaluation position no longer survives.
        let emitted_binding_infers_name = explicit_class_name.is_some()
            && (class_decorators.is_empty()
                || self.target > ScriptTarget::ES2022
                || self.target == ScriptTarget::ES2022 && !has_static_private_class_elements);
        let needs_set_function_name = !emitted_binding_infers_name && runtime_class_name.is_some();
        let mut class_decoration = if class_decorators.is_empty() {
            None
        } else {
            let reference = if let Some(declaration_name) = declaration_name {
                declaration_name.class_reference()
            } else if let Some(declaration_identity) = explicit_class_name_node {
                if self.is_generated_binding_name(declaration_identity)? {
                    let declaration_owner = self.generated_class_reference_owner(original)?;
                    let family = self.generated_class_reference_family(declaration_owner)?;
                    DecoratedClassReferenceBinding::Generated {
                        binding: self.ensure_generated_class_reference_binding(
                            declaration_identity,
                            declaration_owner,
                            family,
                        )?,
                        declaration_owner,
                    }
                } else {
                    DecoratedClassReferenceBinding::Parsed {
                        text: explicit_class_name.clone().ok_or(
                            TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::ClassExpression,
                                field: "class reference identifier text",
                            },
                        )?,
                        declaration_identity,
                    }
                }
            } else {
                let declaration_owner = self.generated_class_reference_owner(original)?;
                let base = self.generated_class_reference_family(declaration_owner)?;
                let provisional_name = self.allocate_generated_reference_name(base);
                DecoratedClassReferenceBinding::Generated {
                    binding: TargetBinding::allocate_numbered(
                        self.context,
                        base.to_owned(),
                        provisional_name,
                    )?,
                    declaration_owner,
                }
            };
            Some(ClassDecorationPlan {
                original,
                decorators: class_decorators,
                decorators_name: self.allocate_name("_classDecorators"),
                descriptor_name: self.allocate_name("_classDescriptor"),
                extra_initializers_name: self.allocate_name("_classExtraInitializers"),
                class_this_name: self.allocate_name("_classThis"),
                reference,
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
                    let backing_name = (is_accessor
                        && (is_private || self.target > ScriptTarget::ES2022))
                        .then(|| {
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
        let class_this_identity = if let Some(class_plan) = class_decoration.as_ref() {
            let assignment =
                self.create_class_this_assignment_block(&class_plan.class_this_name)?;
            let class_this = self
                .context
                .arena()
                .metadata(assignment)
                .and_then(|metadata| metadata.class_this)
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ClassStaticBlockDeclaration,
                    field: "decorated class-this identity",
                })?;
            transformed_members.push(assignment);
            Some(class_this)
        } else {
            None
        };
        if let Some(runtime_class_name) = runtime_class_name
            .as_ref()
            .filter(|_| needs_set_function_name && explicitly_assigned_name.is_none())
        {
            let target = (!(self.target == ScriptTarget::ES2022
                && has_static_private_class_elements))
                .then(|| {
                    class_decoration
                        .as_ref()
                        .map(|plan| plan.class_this_name.as_str())
                })
                .flatten();
            transformed_members
                .push(self.create_set_function_name_block(runtime_class_name.text(), target)?);
        }
        if let Some(member) = named_evaluation_member {
            let visited = if let Some(class_this) = class_this_identity {
                self.retarget_named_evaluation_class_this(member, class_this)?
            } else {
                self.visit(member.node())?
                    .map(|visited| self.node(visited))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ClassExpression,
                        field: "named-evaluation helper block",
                    })?
            };
            transformed_members.push(visited);
        }
        if let Some(block) = computed_name_block {
            transformed_members.push(block);
        }
        let has_static_initializers = self.class_has_static_initializers(data.members)?;
        let decoration_block_index = transformed_members.len();

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
        let static_initializer_receiver = class_decoration
            .as_ref()
            .map_or(DecoratorInitializerReceiver::LexicalThis, |plan| {
                DecoratorInitializerReceiver::ClassBinding(plan.class_this_name.clone())
            });
        let mut pending_initializers =
            ClassPendingDecoratorInitializers::new(static_initializer_receiver);
        if let Some(initializers_name) = static_method_extra.as_ref() {
            pending_initializers.enqueue(
                DecoratorInitializerPlacement::Static,
                PendingDecoratorInitializer::MethodExtra {
                    initializers_name: initializers_name.clone(),
                    class: original,
                },
            );
        }
        if let Some(initializers_name) = instance_method_extra.as_ref() {
            pending_initializers.enqueue(
                DecoratorInitializerPlacement::Instance,
                PendingDecoratorInitializer::MethodExtra {
                    initializers_name: initializers_name.clone(),
                    class: original,
                },
            );
        }

        // tsc-port: transformClassLike @6.0.3
        // tsc-hash: 7199607733dc27e3d53faa0e8e37a065b7ec4ae8f2fdf154d925291fa23f61df
        // tsc-span: _tsc.js:99319-99616
        let mut constructor_index = None;
        for member in original_members {
            if Some(member) == named_evaluation_member {
                continue;
            }

            let member_data = self.context.arena().node(member)?.data.clone();
            if matches!(&member_data, NodeData::ClassStaticBlockDeclaration(_)) {
                let is_runtime_static_block =
                    self.context
                        .arena()
                        .metadata(member)
                        .is_none_or(|metadata| {
                            metadata.assigned_name.is_none() && metadata.class_this.is_none()
                        });
                if is_runtime_static_block {
                    let pending = pending_initializers.drain(DecoratorInitializerPlacement::Static);
                    let statements = self.materialize_pending_initializer_statements(pending)?;
                    if !statements.is_empty() {
                        transformed_members.push(self.create_static_block(statements, true)?);
                    }
                }
                let visited =
                    self.visit(member.node())?
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ClassExpression,
                            field: "static block",
                        })?;
                transformed_members.push(self.node(visited));
                continue;
            }

            if let NodeData::PropertyDeclaration(member_data) = member_data {
                let placement =
                    if self.has_modifier(member_data.modifiers, SyntaxKind::StaticKeyword)? {
                        DecoratorInitializerPlacement::Static
                    } else {
                        DecoratorInitializerPlacement::Instance
                    };
                let pending = pending_initializers.drain(placement);
                if let Some(index) = plans_by_node.get(&member.node()).copied() {
                    let plan = plans[index].clone();
                    let own_initializer =
                        self.create_decorated_initializer(&plan, &pending.receiver)?;
                    let initializer = self
                        .inject_pending_initializer_expression(pending, Some(own_initializer))?
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::PropertyDeclaration,
                            field: "decorated initializer",
                        })?;
                    let static_accessor_receiver =
                        if plan.is_static && self.target < ScriptTarget::ES2022 {
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
                                class_evaluation_binding_name.as_ref().map(|name| {
                                    StaticAccessorReceiver::GeneratedBinding(name.clone())
                                })
                            }
                        } else {
                            None
                        };
                    if plan.descriptor_name.is_some()
                        || plan.is_accessor && self.target > ScriptTarget::ES2022
                    {
                        let backing_name = plan.backing_name.as_deref().ok_or(
                            TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::PropertyDeclaration,
                                field: "auto-accessor backing name",
                            },
                        )?;
                        transformed_members.extend(self.create_auto_accessor_members(
                            &plan,
                            backing_name,
                            initializer,
                            static_accessor_receiver.as_ref(),
                        )?);
                    } else {
                        transformed_members
                            .push(self.update_decorated_property(&plan, initializer)?);
                    }
                    pending_initializers.enqueue(
                        placement,
                        PendingDecoratorInitializer::FieldExtra {
                            initializers_name: plan.extra_initializers_name.clone(),
                        },
                    );
                } else {
                    let visited =
                        self.visit(member.node())?
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::ClassExpression,
                                field: "property",
                            })?;
                    let property = self
                        .inject_pending_initializers_into_property(self.node(visited), pending)?;
                    transformed_members.push(property);
                }
                continue;
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
        }

        let pending_instance = pending_initializers.drain(DecoratorInitializerPlacement::Instance);
        if let Some(statement) = self.materialize_pending_initializer_statement(pending_instance)? {
            if let Some(index) = constructor_index {
                transformed_members[index] =
                    self.inject_constructor_statement(transformed_members[index], statement)?;
            } else {
                transformed_members
                    .push(self.create_constructor(vec![statement], class_super.is_some())?);
            }
        }

        let pending_static = pending_initializers.drain(DecoratorInitializerPlacement::Static);
        let static_receiver = pending_static.receiver.clone();
        let (leading_static_initializers, trailing_static_initializers) = if has_static_initializers
        {
            (
                PendingDecoratorInitializerBatch::empty(static_receiver),
                pending_static,
            )
        } else {
            (
                pending_static,
                PendingDecoratorInitializerBatch::empty(static_receiver),
            )
        };
        let decoration_block = self.create_decoration_block(DecorationBlockInputs {
            plans: &plans,
            method_plans: &method_plans,
            static_method_extra: static_method_extra.as_deref(),
            instance_method_extra: instance_method_extra.as_deref(),
            class_plan: class_decoration.as_ref(),
            class_super_name: class_super.as_ref().map(|(name, _)| name.as_str()),
            metadata_name: &metadata_name,
            leading_static_initializers,
        })?;
        transformed_members.insert(decoration_block_index, decoration_block);

        let mut trailing_static_statements =
            self.materialize_pending_initializer_statements(trailing_static_initializers)?;
        if let Some(class_plan) = class_decoration
            .as_ref()
            .filter(|plan| plan.has_static_initializers)
        {
            let statement = self.create_run_initializers_statement_with_target(
                &class_plan.class_this_name,
                &class_plan.extra_initializers_name,
            )?;
            let statement = self.set_class_finalizer_source_map_range(statement, class_plan)?;
            trailing_static_statements.push(statement);
        }
        if !trailing_static_statements.is_empty() {
            transformed_members.push(self.create_static_block(trailing_static_statements, true)?);
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
        let should_transform_private_static_elements = class_decoration.is_some()
            && transformed_members.iter().try_fold(
                false,
                |found, member| -> Result<bool, TransformError> {
                    Ok(found || self.is_private_static_class_element(*member)?)
                },
            )?;
        if should_transform_private_static_elements {
            // tsc-port: transformClassLike @6.0.3 (private-static class-fields handoff)
            // tsc-hash: e9f9d45ff72748b5c675933e394408bf146fdffe93091079edd1a1e57cd684e3
            // tsc-span: _tsc.js:99588-99615
            for member in &transformed_members {
                if self.is_private_static_class_element(*member)? {
                    self.add_internal_emit_flag(
                        *member,
                        InternalEmitFlags::TRANSFORM_PRIVATE_STATIC_ELEMENTS,
                    )?;
                }
            }
        }
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
        let class = self.context.factory()?.create_node(
            self.source,
            NodeData::ClassExpression(data),
            flags,
        )?;
        self.set_original_only(class, original)?;
        if let Some(class_this) = class_this_metadata {
            self.context.arena_mut()?.metadata_mut(class).class_this = Some(class_this);
        }
        if let Some(assigned_name) = assigned_name_metadata {
            self.context.arena_mut()?.metadata_mut(class).assigned_name = Some(assigned_name);
        }
        if should_transform_private_static_elements {
            self.add_internal_emit_flag(
                class,
                InternalEmitFlags::TRANSFORM_PRIVATE_STATIC_ELEMENTS,
            )?;
            self.should_transform_private_static_elements_in_file = true;
        }
        if let Some(class_plan) = class_decoration.as_ref() {
            let reference = self.materialize_decorated_class_reference(&class_plan.reference)?;
            let declaration = self.create_variable_declaration_with_name(reference, Some(class))?;
            definitions.push(
                self.create_variable_statement_from_declarations(
                    vec![declaration],
                    NodeFlags::NONE,
                )?,
            );
            let reference = self.materialize_decorated_class_reference(&class_plan.reference)?;
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
        if let Some(original) = route.iife_original() {
            self.set_original_only(call, original)?;
        }
        self.used_names = class_scope_names;
        self.computed_temp_ordinal = enclosing_temp_ordinal;
        Ok(call)
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
            leading_static_initializers,
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
                let static_receiver = (plans[index].is_static
                    && self.target < ScriptTarget::ES2022)
                    .then(|| class_plan.map(|plan| plan.class_this_name.as_str()))
                    .flatten();
                statements.push(self.create_es_decorate_statement(
                    &plans[index],
                    metadata_name,
                    static_receiver,
                )?);
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
        statements
            .extend(self.materialize_pending_initializer_statements(leading_static_initializers)?);
        if let Some(class_plan) = class_plan.filter(|plan| !plan.has_static_initializers) {
            let statement = self.create_run_initializers_statement_with_target(
                &class_plan.class_this_name,
                &class_plan.extra_initializers_name,
            )?;
            statements.push(self.set_class_finalizer_source_map_range(statement, class_plan)?);
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
            let helper = self
                .context
                .factory()?
                .create_unscoped_helper_identifier(self.source, EmitHelperName::PropKey)?;
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
        let condition = self.create_identifier(metadata_name)?;
        let if_statement = self.context.factory()?.create_node(
            self.source,
            NodeData::IfStatement(tsc_syntax::nodes::IfStatementData {
                expression: Some(condition.node()),
                then_statement: Some(statement.node()),
                else_statement: None,
            }),
            TransformFlags::NONE,
        )?;
        self.context
            .arena_mut()?
            .metadata_mut(if_statement)
            .add_flags(EmitFlags::SINGLE_LINE);
        Ok(if_statement)
    }

    fn create_es_decorate_statement(
        &mut self,
        plan: &PropertyPlan,
        metadata_name: &str,
        static_receiver: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::EsDecorate)?;
        let ctor = if plan.is_accessor {
            self.create_this()?
        } else {
            self.create_null()?
        };
        let descriptor = if let Some(descriptor_name) = plan.descriptor_name.as_deref() {
            let descriptor = self.create_private_accessor_descriptor(plan, static_receiver)?;
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
        let statement = self.create_expression_statement(call)?;
        self.set_source_map_range_past_decorators(statement, plan.original, plan.data.modifiers)
    }

    fn create_method_es_decorate_statement(
        &mut self,
        plan: &MethodPlan,
        metadata_name: &str,
        extra_initializers_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::EsDecorate)?;
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
        let statement = self.create_expression_statement(call)?;
        let modifiers = self.declaration_modifiers(plan.original)?;
        self.set_source_map_range_past_decorators(statement, plan.original, modifiers)
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
        self.set_original_only(function, plan.original)?;
        let original_modifiers = self.declaration_modifiers(plan.original)?;
        self.set_source_map_range_past_decorators(function, plan.original, original_modifiers)?;
        self.context
            .arena_mut()?
            .metadata_mut(function)
            .add_flags(EmitFlags::NO_COMMENTS);
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
        self.set_original_only(property, plan.original)?;
        self.set_source_map_range_past_decorators(property, plan.original, original_modifiers)?;
        self.context
            .arena_mut()?
            .metadata_mut(property)
            .add_flags(EmitFlags::NO_COMMENTS);
        self.create_object_literal(vec![property], false)
    }

    fn create_private_accessor_descriptor(
        &mut self,
        plan: &PropertyPlan,
        static_receiver: Option<&str>,
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
        let this = if let Some(receiver) = static_receiver {
            self.create_identifier(receiver)?
        } else {
            self.create_this()?
        };
        let backing = self.create_private_identifier(backing_name)?;
        let access = self.create_property_access_node(this, backing)?;
        let statement = self.create_return_statement(access)?;
        let body = self.create_block(vec![statement], false)?;
        let getter =
            self.create_function_expression(Some(empty_parameters.array()), body, None, None)?;
        self.set_original_only(getter, plan.original)?;
        self.set_source_map_range_past_decorators(getter, plan.original, plan.data.modifiers)?;
        self.context
            .arena_mut()?
            .metadata_mut(getter)
            .add_flags(EmitFlags::NO_COMMENTS);
        let getter = self.create_set_function_name(getter, &plan.name, Some("get"))?;

        let value = self.create_parameter("value")?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, vec![value])?;
        let this = if let Some(receiver) = static_receiver {
            self.create_identifier(receiver)?
        } else {
            self.create_this()?
        };
        let backing = self.create_private_identifier(backing_name)?;
        let target = self.create_property_access_node(this, backing)?;
        let value = self.create_identifier("value")?;
        let assignment = self.create_assignment(target, value)?;
        let statement = self.create_expression_statement(assignment)?;
        let body = self.create_block(vec![statement], false)?;
        let setter = self.create_function_expression(Some(parameters.array()), body, None, None)?;
        self.set_original_only(setter, plan.original)?;
        self.set_source_map_range_past_decorators(setter, plan.original, plan.data.modifiers)?;
        self.context
            .arena_mut()?
            .metadata_mut(setter)
            .add_flags(EmitFlags::NO_COMMENTS);
        let setter = self.create_set_function_name(setter, &plan.name, Some("set"))?;

        let getter = self.create_property("get", getter)?;
        self.set_original_only(getter, plan.original)?;
        self.set_source_map_range_past_decorators(getter, plan.original, plan.data.modifiers)?;
        self.context
            .arena_mut()?
            .metadata_mut(getter)
            .add_flags(EmitFlags::NO_COMMENTS);
        let setter = self.create_property("set", setter)?;
        self.set_original_only(setter, plan.original)?;
        self.set_source_map_range_past_decorators(setter, plan.original, plan.data.modifiers)?;
        self.context
            .arena_mut()?
            .metadata_mut(setter)
            .add_flags(EmitFlags::NO_COMMENTS);
        self.create_object_literal(vec![getter, setter], false)
    }

    fn create_set_function_name(
        &mut self,
        function: TransformNode,
        name: &str,
        prefix: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::SetFunctionName)?;
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
        let (name, original_modifiers) = match &data {
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
            self.filter_modifiers(original_modifiers, |kind| kind == SyntaxKind::StaticKeyword)?;
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
        let result = self.set_original_and_range(result, plan.original)?;
        self.set_source_map_range_past_decorators(result, plan.original, original_modifiers)
    }

    fn update_public_method(&mut self, plan: &MethodPlan) -> Result<TransformNode, TransformError> {
        let data = self.context.arena().node(plan.original)?.data.clone();
        let modifiers = self.declaration_modifiers(plan.original)?;
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
        let updated = self.node(updated);
        self.set_source_map_range_past_decorators(updated, plan.original, modifiers)
    }

    fn create_class_decorate_statement(
        &mut self,
        plan: &ClassDecorationPlan,
        metadata_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::EsDecorate)?;
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
        let statement = self.create_expression_statement(call)?;
        let modifiers = self.declaration_modifiers(plan.original)?;
        self.set_source_map_range_past_decorators(statement, plan.original, modifiers)
    }

    fn create_class_replacement_statement(
        &mut self,
        plan: &ClassDecorationPlan,
    ) -> Result<TransformNode, TransformError> {
        let descriptor = self.create_identifier(&plan.descriptor_name)?;
        let value = self.create_property_access(descriptor, "value")?;
        let class_this = self.create_identifier(&plan.class_this_name)?;
        let class_this_assignment = self.create_assignment(class_this, value)?;
        let reference = self.materialize_decorated_class_reference(&plan.reference)?;
        let assignment = self.create_assignment(reference, class_this_assignment)?;
        self.create_expression_statement(assignment)
    }

    fn create_decorator_context(
        &mut self,
        plan: &PropertyPlan,
        metadata_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let source_name = plan
            .data
            .name
            .and_then(|name| self.context.arena().node_ref(self.source, name));
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
            source_name,
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
        let source_name = self.declaration_property_name(plan.original)?;
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
            source_name,
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
        source_name: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let obj = self.create_parameter("obj")?;
        let property = if let Some(temporary) = computed_temp_name {
            self.create_identifier(temporary)?
        } else if is_private {
            source_name.unwrap_or(self.create_private_identifier(name)?)
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
            } else {
                let name = match source_name {
                    Some(source_name) => source_name,
                    None if is_private => self.create_private_identifier(name)?,
                    None => self.create_identifier(name)?,
                };
                self.create_property_access_node(obj_expression, name)?
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
            } else {
                let name = match source_name {
                    Some(source_name) => source_name,
                    None if is_private => self.create_private_identifier(name)?,
                    None => self.create_identifier(name)?,
                };
                self.create_property_access_node(obj_expression, name)?
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

    /// tsc-port: partialTransformClassElement @6.0.3
    /// tsc-hash: 46d06f1175b4bfd01fe5b4df1893f5f1164b80e7a9c94f3340604e4a6b06b912
    /// tsc-span: _tsc.js:99831-99944
    fn create_decorated_initializer(
        &mut self,
        plan: &PropertyPlan,
        receiver: &DecoratorInitializerReceiver,
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
        self.create_run_initializers_for_receiver(
            &plan.initializers_name,
            Some(initializer),
            receiver,
        )
    }

    fn create_run_initializers_for_receiver(
        &mut self,
        initializers_name: &str,
        value: Option<TransformNode>,
        receiver: &DecoratorInitializerReceiver,
    ) -> Result<TransformNode, TransformError> {
        match receiver {
            DecoratorInitializerReceiver::LexicalThis => {
                self.create_run_initializers(initializers_name, value)
            }
            DecoratorInitializerReceiver::ClassBinding(target_name) => self
                .create_run_initializers_with_target(initializers_name, value, Some(target_name)),
        }
    }

    fn materialize_pending_initializer_expressions(
        &mut self,
        pending: PendingDecoratorInitializerBatch,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let PendingDecoratorInitializerBatch {
            receiver,
            initializers,
        } = pending;
        let mut expressions = Vec::with_capacity(initializers.len());
        for initializer in initializers {
            let expression = self.create_run_initializers_for_receiver(
                initializer.initializers_name(),
                None,
                &receiver,
            )?;
            let expression = match initializer {
                PendingDecoratorInitializer::MethodExtra { class, .. } => {
                    self.set_class_source_map_range(expression, class)?
                }
                PendingDecoratorInitializer::FieldExtra { .. } => expression,
            };
            expressions.push(expression);
        }
        Ok(expressions)
    }

    /// Injects placement-owned effects ahead of the property's own value while
    /// retaining an existing parenthesized expression as the range owner.
    ///
    /// tsc-port: visitPropertyDeclaration @6.0.3
    /// tsc-hash: 32896629db3e477cdb54934eeefadb12c80ac10d8909d811eb7a90e2de4b164d
    /// tsc-span: _tsc.js:100041-100150
    /// tsc-port: injectPendingExpressionsCommon @6.0.3
    /// tsc-hash: 0409cc30806f5998022df21eceb6369af27f21778e795597936eddc5350f379b
    /// tsc-span: _tsc.js:100511-100526
    /// tsc-port: injectPendingInitializers @6.0.3
    /// tsc-hash: dee2ea62ca9186b228bf257a5bf9a171193f1b637d16426df8d8b7713e7cd8d5
    /// tsc-span: _tsc.js:100535-100545
    fn inject_pending_initializer_expression(
        &mut self,
        pending: PendingDecoratorInitializerBatch,
        expression: Option<TransformNode>,
    ) -> Result<Option<TransformNode>, TransformError> {
        if pending.is_empty() {
            return Ok(expression);
        }
        let mut expressions = self.materialize_pending_initializer_expressions(pending)?;
        if let Some(expression) = expression {
            if let NodeData::ParenthesizedExpression(mut data) =
                self.context.arena().node(expression)?.data.clone()
            {
                let inner = data
                    .expression
                    .and_then(|inner| self.context.arena().node_ref(self.source, inner))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ParenthesizedExpression,
                        field: "expression",
                    })?;
                expressions.push(inner);
                let inline = self.inline_expressions(expressions)?;
                data.expression = Some(inline.node());
                let flags = flags_after_update(
                    self.context.arena(),
                    expression,
                    &NodeData::ParenthesizedExpression(data.clone()),
                )?;
                let updated = self.context.factory()?.update_node(
                    expression,
                    NodeData::ParenthesizedExpression(data),
                    flags,
                )?;
                return Ok(Some(updated));
            }
            expressions.push(expression);
        }
        Ok(Some(self.inline_expressions(expressions)?))
    }

    fn inject_pending_initializers_into_property(
        &mut self,
        property: TransformNode,
        pending: PendingDecoratorInitializerBatch,
    ) -> Result<TransformNode, TransformError> {
        if pending.is_empty() {
            return Ok(property);
        }
        let NodeData::PropertyDeclaration(mut data) =
            self.context.arena().node(property)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassExpression,
                field: "property",
            });
        };
        let initializer = data
            .initializer
            .and_then(|initializer| self.context.arena().node_ref(self.source, initializer));
        data.initializer = self
            .inject_pending_initializer_expression(pending, initializer)?
            .map(TransformNode::node);
        let flags = flags_after_update(
            self.context.arena(),
            property,
            &NodeData::PropertyDeclaration(data.clone()),
        )?;
        // Use the visited property as the immediate original. In particular,
        // this keeps a synthetic parameter-property's Property -> Parameter
        // ownership chain available to the class-fields transform.
        self.context
            .factory()?
            .update_node(property, NodeData::PropertyDeclaration(data), flags)
    }

    /// tsc-port: visitClassStaticBlockDeclaration @6.0.3
    /// tsc-hash: 5ba6f2d5e5b218a418e3ca67a6714022b5a77e460c16e042d950b765f0a6504a
    /// tsc-span: _tsc.js:100005-100040
    fn materialize_pending_initializer_statements(
        &mut self,
        pending: PendingDecoratorInitializerBatch,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let expressions = self.materialize_pending_initializer_expressions(pending)?;
        let mut statements = Vec::with_capacity(expressions.len());
        for expression in expressions {
            statements.push(self.create_expression_statement(expression)?);
        }
        Ok(statements)
    }

    /// tsc-port: prepareConstructor @6.0.3
    /// tsc-hash: 2a79ab99613abecdfd7e854650bbaac5f5b831bde37c6c0a45fd71d923d79954
    /// tsc-span: _tsc.js:99747-99758
    fn materialize_pending_initializer_statement(
        &mut self,
        pending: PendingDecoratorInitializerBatch,
    ) -> Result<Option<TransformNode>, TransformError> {
        let expressions = self.materialize_pending_initializer_expressions(pending)?;
        if expressions.is_empty() {
            return Ok(None);
        }
        let inline = self.inline_expressions(expressions)?;
        Ok(Some(self.create_expression_statement(inline)?))
    }

    /// tsc-port: finishClassElement @6.0.3
    /// tsc-hash: 277a54cd03b69044d9781c73b5c6c417dcfc495d3fda96ca0e49a3466b4e4d01
    /// tsc-span: _tsc.js:99824-99830
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
        let updated = self.context.factory()?.update_node(
            plan.original,
            NodeData::PropertyDeclaration(data),
            flags,
        )?;
        self.set_source_map_range_past_decorators(updated, plan.original, plan.data.modifiers)
    }

    fn create_auto_accessor_members(
        &mut self,
        plan: &PropertyPlan,
        backing_name: &str,
        initializer: TransformNode,
        static_receiver: Option<&StaticAccessorReceiver>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let name = plan.data.name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::PropertyDeclaration,
            field: "name",
        })?;
        let backing = self.create_private_identifier(backing_name)?;
        // The backing name is synthetic, but its resolver identity is the
        // parsed auto-accessor name. Class-field lowering queries resolver
        // facts on this child, so preserve the original-name chain used by
        // `create_generated_private_identifier`.
        let original_name = self.node(name);
        self.context
            .arena_mut()?
            .set_original_node(backing, Some(original_name))?;
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
        self.set_source_map_range_past_decorators(field, plan.original, plan.data.modifiers)?;
        self.context
            .arena_mut()?
            .metadata_mut(field)
            .add_flags(EmitFlags::NO_COMMENTS);
        // tsc-port: decorated @6.0.3 (auto-accessor expansion)
        // tsc-hash: a36d5d1d9f385cf80a5379c53a98ff9936ace13b998d268a79af1aaa7b791850
        // tsc-span: _tsc.js:100115-100150
        if plan.is_static && self.target < ScriptTarget::ES2022 {
            self.context
                .arena_mut()?
                .metadata_mut(field)
                .class_field_initializer_comment_source = Some(plan.original);
        }
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
        self.set_source_map_range_past_decorators(getter, plan.original, plan.data.modifiers)?;
        self.set_original_and_range(setter, plan.original)?;
        self.set_source_map_range_past_decorators(setter, plan.original, plan.data.modifiers)?;
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

    /// Retarget a named-evaluation helper injected by an earlier transform to
    /// the semantic constructor identity allocated for class decoration.
    ///
    /// The helper's assigned-name expression remains owned by the existing
    /// block. Only its lexical `this` projection changes, matching
    /// `transformESDecorators.visitThisExpression` while a class element is
    /// visited. The replacement is cloned from the identity transported by
    /// the class-this assignment block, rather than recovered from printable
    /// identifier text.
    fn retarget_named_evaluation_class_this(
        &mut self,
        block: TransformNode,
        class_this: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let (assigned_name, flags, internal_flags) = self
            .context
            .arena()
            .metadata(block)
            .map(|metadata| {
                (
                    metadata.assigned_name,
                    metadata.flags(),
                    metadata.internal_flags(),
                )
            })
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassStaticBlockDeclaration,
                field: "named-evaluation metadata",
            })?;
        let assigned_name = assigned_name.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ClassStaticBlockDeclaration,
            field: "named-evaluation assigned name",
        })?;
        let rewritten = DecoratorClassThisRewriter::new(self, class_this).rewrite(block)?;
        let metadata = self.context.arena_mut()?.metadata_mut(rewritten);
        metadata.assigned_name = Some(assigned_name);
        metadata.set_flags(flags);
        metadata.set_internal_flags(internal_flags);
        Ok(rewritten)
    }

    fn create_set_function_name_block(
        &mut self,
        class_name: &str,
        target_name: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::SetFunctionName)?;
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

    /// tsc-port: createRunInitializersHelper @6.0.3
    /// tsc-hash: ac7241f25e6f4d82e533ae048fbe9de24149093224ff8713b1483e39c8798e68
    /// tsc-span: _tsc.js:25715-25723
    fn create_run_initializers_with_target(
        &mut self,
        initializers_name: &str,
        value: Option<TransformNode>,
        target_name: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::RunInitializers)?;
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

    /// tsc-port: createRunInitializersHelper @6.0.3
    /// tsc-hash: ac7241f25e6f4d82e533ae048fbe9de24149093224ff8713b1483e39c8798e68
    /// tsc-span: _tsc.js:25715-25723
    fn create_run_initializers_statement_with_target(
        &mut self,
        target_name: &str,
        initializers_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::RunInitializers)?;
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

    /// The constructor subtree has already passed through this transform's
    /// visitor before the residual initializer queue is known. This is the
    /// Rust ownership split for tsc's single-pass constructor visitor: a
    /// reachable `super()` updates only its typed block path, while the
    /// no-super fallback replays the already-visited prologue node identities
    /// before the initializer and complete visited body. The visitor itself is
    /// never run twice.
    ///
    /// tsc-port: visitConstructorDeclaration @6.0.3
    /// tsc-hash: c5fbc638b5cdc3d6b829a1354f0c0eaafd8821813e4634bc9ca8c45426b49c61
    /// tsc-span: _tsc.js:99788-99823
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
        let prologue = constructor_prologue(self.context.arena(), &statements)?;
        let placement = self
            .find_constructor_super_path(&statements, prologue.body_start())?
            .map_or(
                DecoratorConstructorInitializerPlacement::ReplayPrologueThenBody(prologue),
                DecoratorConstructorInitializerPlacement::AfterSuper,
            );
        match placement {
            DecoratorConstructorInitializerPlacement::AfterSuper(path) => {
                self.inject_constructor_statement_at_super_path(
                    &mut statements,
                    &path.statement_indices,
                    statement,
                )?;
            }
            DecoratorConstructorInitializerPlacement::ReplayPrologueThenBody(prologue) => {
                let mut replayed = Vec::with_capacity(
                    prologue
                        .body_start()
                        .saturating_add(statements.len())
                        .saturating_add(1),
                );
                replayed.extend_from_slice(&statements[..prologue.standard_end()]);
                replayed
                    .extend_from_slice(&statements[prologue.standard_end()..prologue.custom_end()]);
                replayed.push(statement);
                replayed.append(&mut statements);
                statements = replayed;
            }
        }
        let statements = if let Some(original) = block.statements.map(|array| self.array(array)) {
            self.context
                .factory()?
                .update_node_array(original, statements)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
        };
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

    fn find_constructor_super_path(
        &self,
        statements: &[TransformNode],
        start: usize,
    ) -> Result<Option<DecoratorConstructorSuperPath>, TransformError> {
        for (index, statement) in statements.iter().enumerate().skip(start) {
            if self.statement_is_super_call(*statement)? {
                return Ok(Some(DecoratorConstructorSuperPath {
                    statement_indices: vec![index],
                }));
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
            let nested_statements = self.array_nodes(block.statements)?;
            if let Some(mut nested_path) =
                self.find_constructor_super_path(&nested_statements, 0)?
            {
                nested_path.statement_indices.insert(0, index);
                return Ok(Some(nested_path));
            }
        }
        Ok(None)
    }

    /// Updates the already-visited nodes along the typed `try`/`super()` path
    /// and places the residual instance initializer immediately after the
    /// `super()` statement.
    ///
    /// tsc-port: transformConstructorBodyWorker @6.0.3
    /// tsc-hash: aaf0c5324b33bbc52730bda4f4a77db2c952a35f0f18f78dafe9750923fd9c12
    /// tsc-span: _tsc.js:99759-99787
    fn inject_constructor_statement_at_super_path(
        &mut self,
        statements: &mut Vec<TransformNode>,
        path: &[usize],
        initializer: TransformNode,
    ) -> Result<(), TransformError> {
        let (&index, remaining) =
            path.split_first()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Constructor,
                    field: "super statement path",
                })?;
        if remaining.is_empty() {
            statements.insert(index + 1, initializer);
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
        let mut nested_statements = self.array_nodes(block.statements)?;
        self.inject_constructor_statement_at_super_path(
            &mut nested_statements,
            remaining,
            initializer,
        )?;
        let nested_statements =
            if let Some(original) = block.statements.map(|array| self.array(array)) {
                self.context
                    .factory()?
                    .update_node_array(original, nested_statements)?
            } else {
                self.context
                    .factory()?
                    .create_node_array(self.source, nested_statements)?
            };
        block.statements = Some(nested_statements.array());
        let flags = flags_after_update(
            self.context.arena(),
            try_block,
            &NodeData::Block(block.clone()),
        )?;
        let try_block =
            self.context
                .factory()?
                .update_node(try_block, NodeData::Block(block), flags)?;
        try_statement.try_block = Some(try_block.node());
        let flags = flags_after_update(
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
        self.context.request_emit_helper(super::helpers::prop_key())
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
        self.create_variable_declaration_with_name(name, initializer)
    }

    fn create_variable_declaration_with_name(
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

    fn create_named_export(
        &mut self,
        declaration_name: &DecoratedClassDeclarationName,
    ) -> Result<TransformNode, TransformError> {
        let name = self.materialize_decorated_declaration_name(
            declaration_name,
            DecoratedClassNameProjection::LocalMapped,
        )?;
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

    fn decorated_class_declaration_name(
        &mut self,
        declaration: TransformNode,
        name: Option<NodeId>,
    ) -> Result<Option<DecoratedClassDeclarationName>, TransformError> {
        let Some(name) = name.map(|name| self.node(name)) else {
            return Ok(None);
        };
        let Some(text) = self.identifier_text(name)?.map(str::to_owned) else {
            return Ok(None);
        };

        let source_declaration = self.context.arena().get_original_node(declaration);
        let source_has_name = match &self.context.arena().node(source_declaration)?.data {
            NodeData::ClassDeclaration(data) => data.name.is_some(),
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ClassDeclaration,
                    field: "original class declaration",
                });
            }
        };
        if source_has_name {
            return Ok(Some(DecoratedClassDeclarationName::Parsed {
                text,
                declaration_identity: name,
            }));
        }

        let binding = if let Some(binding) = self.generated_binding_for_identifier(name) {
            binding
        } else {
            TargetBinding::allocate_numbered(self.context, "default".to_owned(), text)?
        };
        binding.write_generated_metadata(self.context.arena_mut()?, name);
        self.context
            .arena_mut()?
            .set_original_node(name, Some(source_declaration))?;
        Ok(Some(
            DecoratedClassDeclarationName::TypeScriptGeneratedAnonymousDefault {
                binding,
                declaration_owner: declaration,
            },
        ))
    }

    fn generated_binding_for_identifier(&self, name: TransformNode) -> Option<TargetBinding> {
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

    /// Mirrors `getNodeForGeneratedName` for class references. An ESNext
    /// using-hoist changes the surface syntax from a declaration to an
    /// expression, but the generated-name family continues to belong to the
    /// ultimate source class-like node.
    fn generated_class_reference_owner(
        &self,
        class_like: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let owner = self.context.arena().get_original_node(class_like);
        match self.context.arena().node(owner)?.kind {
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => Ok(owner),
            kind => Err(TransformError::RequiredChildRemoved {
                parent: kind,
                field: "generated class-reference owner",
            }),
        }
    }

    fn generated_class_reference_family(
        &self,
        owner: TransformNode,
    ) -> Result<&'static str, TransformError> {
        match self.context.arena().node(owner)?.kind {
            SyntaxKind::ClassDeclaration => Ok("default"),
            SyntaxKind::ClassExpression => Ok("class"),
            kind => Err(TransformError::RequiredChildRemoved {
                parent: kind,
                field: "generated class-reference family",
            }),
        }
    }

    /// Class-expression surface syntax is not sufficient to recover named
    /// evaluation semantics after an earlier transform has converted an
    /// anonymous default declaration into an expression. The ultimate source
    /// owner decides the declaration case; only a genuine source class
    /// expression may derive its runtime name from a parsed explicit name or
    /// from its assignment context.
    fn class_expression_runtime_name(
        &self,
        class_like: TransformNode,
        explicit_name_node: Option<TransformNode>,
        explicit_name: Option<&str>,
        assigned_name: Option<&str>,
        has_class_decorators: bool,
    ) -> Result<Option<DecoratedClassRuntimeName>, TransformError> {
        let owner = self.generated_class_reference_owner(class_like)?;
        match self.context.arena().node(owner)?.kind {
            SyntaxKind::ClassDeclaration => {
                Ok(Some(DecoratedClassRuntimeName::AnonymousDefaultDeclaration))
            }
            SyntaxKind::ClassExpression => {
                if let Some(explicit_name_node) = explicit_name_node {
                    if !self.is_generated_binding_name(explicit_name_node)? {
                        return Ok(explicit_name
                            .map(|name| DecoratedClassRuntimeName::Declared(name.to_owned())));
                    }
                }
                Ok(assigned_name
                    .map(|name| DecoratedClassRuntimeName::Assigned(name.to_owned()))
                    // tsc injects a named-evaluation helper with the empty
                    // string only when the otherwise anonymous expression
                    // has a class decorator. A member-only decorated
                    // expression remains anonymous.
                    .or_else(|| {
                        has_class_decorators
                            .then_some(DecoratedClassRuntimeName::UnassignedDecoratedExpression)
                    }))
            }
            kind => Err(TransformError::RequiredChildRemoved {
                parent: kind,
                field: "decorated class runtime-name owner",
            }),
        }
    }

    fn is_generated_binding_name(&self, name: TransformNode) -> Result<bool, TransformError> {
        if self
            .context
            .arena()
            .metadata(name)
            .and_then(|metadata| metadata.generated_binding_id())
            .is_some()
        {
            return Ok(true);
        }
        if let Some(parsed) = self.context.arena().parse_tree_node(name)? {
            return Ok(!matches!(
                self.context.arena().node(parsed)?.data,
                NodeData::Identifier(_)
            ));
        }
        Ok(NodeFlags::from_bits(self.context.arena().node(name)?.flags)
            .contains(NodeFlags::SYNTHESIZED))
    }

    fn ensure_generated_class_reference_binding(
        &mut self,
        name: TransformNode,
        declaration_owner: TransformNode,
        family: &str,
    ) -> Result<TargetBinding, TransformError> {
        let binding = if let Some(binding) = self.generated_binding_for_identifier(name) {
            binding
        } else {
            let text = self
                .identifier_text(name)?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ClassExpression,
                    field: "generated class reference identifier",
                })?
                .to_owned();
            TargetBinding::allocate_numbered(self.context, family.to_owned(), text)?
        };
        binding.write_generated_metadata(self.context.arena_mut()?, name);
        self.context
            .arena_mut()?
            .set_original_node(name, Some(declaration_owner))?;
        Ok(binding)
    }

    /// Materialize one of the three declaration-name projections used by the
    /// standard-decorator transform.
    ///
    /// Parsed projections clone the declaration-name provenance and emit
    /// flags. `getName` bypasses those projection flags for a generated name:
    /// its separately owned AST nodes have no parsed text range and instead
    /// share one target binding with the name inserted by
    /// `transformTypeScript`.
    fn materialize_decorated_declaration_name(
        &mut self,
        declaration_name: &DecoratedClassDeclarationName,
        projection: DecoratedClassNameProjection,
    ) -> Result<TransformNode, TransformError> {
        match declaration_name {
            DecoratedClassDeclarationName::Parsed {
                text,
                declaration_identity,
            } => {
                let source_flags = self
                    .context
                    .arena()
                    .metadata(*declaration_identity)
                    .map_or(EmitFlags::NONE, |metadata| metadata.flags());
                let identifier = self.create_identifier(text)?;
                self.set_original_and_range(identifier, *declaration_identity)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(identifier)
                    .add_flags(source_flags | projection.emit_flags());
                Ok(identifier)
            }
            DecoratedClassDeclarationName::TypeScriptGeneratedAnonymousDefault {
                binding,
                declaration_owner,
            } => {
                let identifier = self.create_identifier(binding.provisional_name())?;
                binding.write_generated_metadata(self.context.arena_mut()?, identifier);
                self.set_original_only(identifier, *declaration_owner)?;
                Ok(identifier)
            }
        }
    }

    fn materialize_decorated_class_reference(
        &mut self,
        reference: &DecoratedClassReferenceBinding,
    ) -> Result<TransformNode, TransformError> {
        let identifier = self.create_identifier(reference.planned_text())?;
        match reference {
            DecoratedClassReferenceBinding::Parsed {
                declaration_identity,
                ..
            } => {
                let source_flags = self
                    .context
                    .arena()
                    .metadata(*declaration_identity)
                    .map_or(EmitFlags::NONE, |metadata| metadata.flags());
                self.set_original_and_range(identifier, *declaration_identity)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(identifier)
                    .add_flags(
                        source_flags
                            | EmitFlags::LOCAL_NAME
                            | EmitFlags::NO_COMMENTS
                            | EmitFlags::NO_SOURCE_MAP,
                    );
            }
            DecoratedClassReferenceBinding::Generated {
                binding,
                declaration_owner,
            } => {
                binding.write_generated_metadata(self.context.arena_mut()?, identifier);
                self.set_original_only(identifier, *declaration_owner)?;
            }
        }
        Ok(identifier)
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

    fn is_private_static_class_element(
        &self,
        member: TransformNode,
    ) -> Result<bool, TransformError> {
        let (name, modifiers) = match &self.context.arena().node(member)?.data {
            NodeData::PropertyDeclaration(data) => (data.name, data.modifiers),
            NodeData::MethodDeclaration(data) => (data.name, data.modifiers),
            NodeData::GetAccessor(data) => (data.name, data.modifiers),
            NodeData::SetAccessor(data) => (data.name, data.modifiers),
            _ => return Ok(false),
        };
        Ok(self.name_is_private(name)?
            && self.has_modifier(modifiers, SyntaxKind::StaticKeyword)?)
    }

    fn add_internal_emit_flag(
        &mut self,
        node: TransformNode,
        flag: InternalEmitFlags,
    ) -> Result<(), TransformError> {
        let flags = self
            .context
            .arena()
            .metadata(node)
            .map_or(InternalEmitFlags::NONE, |metadata| {
                metadata.internal_flags()
            });
        self.context
            .arena_mut()?
            .metadata_mut(node)
            .set_internal_flags(InternalEmitFlags::from_bits(flags.bits() | flag.bits()));
        Ok(())
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

    fn set_original_only(
        &mut self,
        node: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context
            .arena_mut()?
            .set_original_node(node, Some(original))?;
        Ok(node)
    }

    fn effective_comment_range(&self, node: TransformNode) -> Result<CommentRange, TransformError> {
        if let Some(range) = self
            .context
            .arena()
            .metadata(node)
            .and_then(crate::EmitMetadata::comment_range)
        {
            return Ok(range);
        }
        let source = self.context.arena().source(node.source())?.syntax();
        let record = self.context.arena().node(node)?;
        let range = SourceRange::from_raw(record.pos, record.end, source.positions())
            .map_err(|error| TransformError::InvalidSourceRange { node, error })?;
        Ok(CommentRange::new(node.source(), range))
    }

    fn set_declaration_comment_owner(
        &mut self,
        node: TransformNode,
        declaration: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let comment_range = self.effective_comment_range(declaration)?;
        self.set_original_only(node, declaration)?;
        self.context
            .arena_mut()?
            .metadata_mut(node)
            .set_comment_range(comment_range);
        Ok(node)
    }

    fn declaration_modifiers(
        &self,
        declaration: TransformNode,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        Ok(match &self.context.arena().node(declaration)?.data {
            NodeData::ClassDeclaration(data) => data.modifiers,
            NodeData::ClassExpression(data) => data.modifiers,
            NodeData::PropertyDeclaration(data) => data.modifiers,
            NodeData::MethodDeclaration(data) => data.modifiers,
            NodeData::GetAccessor(data) => data.modifiers,
            NodeData::SetAccessor(data) => data.modifiers,
            NodeData::Constructor(data) => data.modifiers,
            _ => None,
        })
    }

    fn declaration_name(
        &self,
        declaration: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let name = match &self.context.arena().node(declaration)?.data {
            NodeData::ClassDeclaration(data) => data.name,
            NodeData::ClassExpression(data) => data.name,
            _ => None,
        };
        Ok(name.and_then(|name| self.context.arena().node_ref(self.source, name)))
    }

    fn declaration_property_name(
        &self,
        declaration: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let name = match &self.context.arena().node(declaration)?.data {
            NodeData::PropertyDeclaration(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            _ => None,
        };
        Ok(name.and_then(|name| self.context.arena().node_ref(self.source, name)))
    }

    fn set_source_map_range_from(
        &mut self,
        node: TransformNode,
        range_source: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source_map_range = self
            .context
            .arena()
            .metadata(range_source)
            .and_then(crate::EmitMetadata::source_map_range);
        let source_map_range = match source_map_range {
            Some(range) => range,
            None => {
                let record = self.context.arena().node(range_source)?;
                let source = self.context.arena().source(range_source.source())?.syntax();
                let range = SourceRange::from_raw(record.pos, record.end, source.positions())
                    .map_err(|error| TransformError::InvalidSourceRange {
                        node: range_source,
                        error,
                    })?;
                SourceMapRange::new(range_source.source(), range)
            }
        };
        self.context
            .arena_mut()?
            .metadata_mut(node)
            .set_source_map_range(source_map_range);
        Ok(node)
    }

    fn set_class_finalizer_source_map_range(
        &mut self,
        statement: TransformNode,
        plan: &ClassDecorationPlan,
    ) -> Result<TransformNode, TransformError> {
        self.set_class_source_map_range(statement, plan.original)
    }

    fn set_class_source_map_range(
        &mut self,
        node: TransformNode,
        class: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if let Some(name) = self.declaration_name(class)? {
            self.set_source_map_range_from(node, name)
        } else {
            let modifiers = self.declaration_modifiers(class)?;
            self.set_source_map_range_past_decorators(node, class, modifiers)
        }
    }

    /// tsc-port: moveRangePastDecorators @6.0.3
    /// tsc-hash: 27d3b9fba1576ed2d7269a9fe1b694ac1e16e977da92c9935f13359611222a93
    /// tsc-span: _tsc.js:17307-17310
    fn set_source_map_range_past_decorators(
        &mut self,
        node: TransformNode,
        declaration: TransformNode,
        modifiers: Option<NodeArrayId>,
    ) -> Result<TransformNode, TransformError> {
        let declaration_record = self.context.arena().node(declaration)?.clone();
        let last_decorator = self
            .array_nodes(modifiers)?
            .into_iter()
            .rev()
            .find(|modifier| {
                self.context
                    .arena()
                    .node(*modifier)
                    .is_ok_and(|record| record.kind == SyntaxKind::Decorator)
            });
        let start = last_decorator
            .and_then(|decorator| self.context.arena().node(decorator).ok())
            .map_or(declaration_record.pos, |decorator| {
                if decorator.end == u32::MAX {
                    declaration_record.pos
                } else {
                    decorator.end
                }
            });
        let source = self.context.arena().source(declaration.source())?.syntax();
        let range = SourceRange::from_raw(start, declaration_record.end, source.positions())
            .map_err(|error| TransformError::InvalidSourceRange {
                node: declaration,
                error,
            })?;
        self.context
            .arena_mut()?
            .metadata_mut(node)
            .set_source_map_range(SourceMapRange::new(declaration.source(), range));
        Ok(node)
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

impl<'visitor, 'context> DecoratorClassThisRewriter<'visitor, 'context> {
    fn new(
        visitor: &'visitor mut StandardDecoratorVisitor<'context>,
        class_this: TransformNode,
    ) -> Self {
        Self {
            visitor,
            class_this,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
        }
    }

    fn rewrite(mut self, block: TransformNode) -> Result<TransformNode, TransformError> {
        let rewritten =
            self.rewrite_node(block.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ClassStaticBlockDeclaration,
                    field: "named-evaluation class-this substitution",
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
            let flags = self
                .visitor
                .context
                .arena()
                .metadata(self.class_this)
                .map_or(EmitFlags::NONE, |metadata| metadata.flags());
            let internal_flags = self
                .visitor
                .context
                .arena()
                .metadata(self.class_this)
                .map_or(InternalEmitFlags::NONE, |metadata| {
                    metadata.internal_flags()
                });
            let replacement = self
                .visitor
                .context
                .factory()?
                .clone_node(self.class_this)?;
            let metadata = self.visitor.context.arena_mut()?.metadata_mut(replacement);
            metadata.set_flags(flags);
            metadata.set_internal_flags(internal_flags);
            replacement.node()
        } else if matches!(&record.data, NodeData::Token)
            || matches!(
                record.kind,
                SyntaxKind::ClassDeclaration
                    | SyntaxKind::ClassExpression
                    | SyntaxKind::Constructor
                    | SyntaxKind::FunctionDeclaration
                    | SyntaxKind::FunctionExpression
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::MethodDeclaration
                    | SyntaxKind::SetAccessor
            )
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

impl NodeDataChildVisitor for DecoratorClassThisRewriter<'_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.visitor
            .context
            .arena()
            .node(self.visitor.node(id))
            .expect("named-evaluation child belongs to the current transform source")
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
    fn classify_fallback_child(
        &self,
        id: NodeId,
    ) -> Result<StandardDecoratorFallbackChild, TransformError> {
        let node = self.node(id);
        if self.context.arena().node(node)?.kind == SyntaxKind::Decorator {
            Ok(StandardDecoratorFallbackChild::ErasedDecorator)
        } else {
            Ok(StandardDecoratorFallbackChild::Runtime(node))
        }
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, TransformError> {
        if let Some(mapped) = self.arrays.get(&id) {
            return Ok(*mapped);
        }
        let original = self.array(id);
        let nodes = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(nodes.len());
        for node in nodes {
            match self.classify_fallback_child(node)? {
                StandardDecoratorFallbackChild::ErasedDecorator => {}
                StandardDecoratorFallbackChild::Runtime(node)
                    if self.context.arena().node(node)?.kind == SyntaxKind::ClassDeclaration =>
                {
                    visited.extend(self.visit_class_declaration(node.node())?);
                }
                StandardDecoratorFallbackChild::Runtime(node) => {
                    if let Some(node) = self.visit(node.node())? {
                        visited.push(self.node(node));
                    }
                }
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
