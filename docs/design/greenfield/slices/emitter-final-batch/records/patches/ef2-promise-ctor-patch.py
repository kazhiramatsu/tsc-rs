#!/usr/bin/env python3
"""EF2-PROMISE-CTOR: ES5 async functions pass the return-type entity name as the __awaiter promise constructor."""
p = "/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/builtins/es2017.rs"
s = open(p).read()
def rep(old, new, label):
    global s
    assert s.count(old) == 1, label
    s = s.replace(old, new)
rep("""        let awaiter = self.create_awaiter_call(
            self.has_lexical_this,
            arguments_expression,
            parameter_plan.inner,
            inner_body,
        )?;
""", """        // transformAsyncFunctionBody (_tsc.js:101315-101320): below ES2015 the
        // `__awaiter` call names the promise constructor from the original
        // function's return type annotation.
        let promise_constructor = if self.target < ScriptTarget::ES2015 {
            self.get_promise_constructor(original)?
        } else {
            None
        };
        let awaiter = self.create_awaiter_call(
            self.has_lexical_this,
            arguments_expression,
            parameter_plan.inner,
            inner_body,
            promise_constructor,
        )?;
""", "call site")
rep("""    fn create_awaiter_call(
        &mut self,
        has_lexical_this: bool,
        arguments_expression: Option<TransformNode>,
        inner_parameters: Option<NodeArrayId>,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
""", """    /// tsc-port: getPromiseConstructor @6.0.3
    /// tsc-span: _tsc.js:101432-101441
    ///
    /// The entity name of the original return type annotation, when the
    /// resolver classifies it as a value with a construct signature or cannot
    /// classify it at all; every other classification leaves `void 0`.
    fn get_promise_constructor(
        &mut self,
        function: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let original = self.context.arena().get_original_node(function);
        let type_node = match &self.context.arena().node(original)?.data {
            NodeData::FunctionDeclaration(data) => data.r#type,
            NodeData::FunctionExpression(data) => data.r#type,
            NodeData::ArrowFunction(data) => data.r#type,
            NodeData::MethodDeclaration(data) => data.r#type,
            _ => None,
        };
        let Some(type_node) = type_node.map(|id| TransformNode::new(original.source(), id)) else {
            return Ok(None);
        };
        // getEntityNameFromTypeNode (_tsc.js:14623-14635).
        let entity = match &self.context.arena().node(type_node)?.data {
            NodeData::TypeReference(data) => data.type_name,
            NodeData::ExpressionWithTypeArguments(data) => data
                .expression
                .filter(|id| self.is_entity_name_expression(TransformNode::new(original.source(), *id))),
            NodeData::Identifier(_) | NodeData::QualifiedName(_) => Some(type_node.node()),
            _ => None,
        };
        let Some(entity) = entity.map(|id| TransformNode::new(original.source(), id)) else {
            return Ok(None);
        };
        if !matches!(
            self.context.arena().node(entity)?.data,
            NodeData::Identifier(_) | NodeData::QualifiedName(_)
        ) {
            return Ok(None);
        }
        let kind = match self.context.arena().parse_tree_resolver_node(entity)? {
            Some(resolver_node) => self
                .resolver
                .get_type_reference_serialization_kind(resolver_node, resolver_node)?,
            None => EmitTypeReferenceSerializationKind::Unknown,
        };
        Ok(matches!(
            kind,
            EmitTypeReferenceSerializationKind::TypeWithConstructSignatureAndValue
                | EmitTypeReferenceSerializationKind::Unknown
        )
        .then_some(entity))
    }

    fn is_entity_name_expression(&self, node: TransformNode) -> bool {
        match self.context.arena().node(node).map(|record| record.data.clone()) {
            Ok(NodeData::Identifier(_)) => true,
            Ok(NodeData::PropertyAccessExpression(data)) => {
                data.expression.is_some_and(|expression| {
                    self.is_entity_name_expression(TransformNode::new(node.source(), expression))
                }) && data.name.is_some_and(|name| {
                    matches!(
                        self.context.arena().node(TransformNode::new(node.source(), name)).map(|record| &record.data),
                        Ok(NodeData::Identifier(_))
                    )
                })
            }
            _ => false,
        }
    }

    /// tsc-port: createExpressionFromEntityName @6.0.3
    /// tsc-span: _tsc.js:27330-27338
    fn create_expression_from_entity_name(
        &mut self,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        match self.context.arena().node(name)?.data.clone() {
            NodeData::QualifiedName(data) => {
                let left = data
                    .left
                    .map(|id| TransformNode::new(name.source(), id))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::QualifiedName,
                        field: "left",
                    })?;
                let right = data
                    .right
                    .map(|id| TransformNode::new(name.source(), id))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::QualifiedName,
                        field: "right",
                    })?;
                let left = self.create_expression_from_entity_name(left)?;
                let right_clone = self.context.factory()?.clone_node(right)?;
                self.context.factory()?.set_text_range(right_clone, right)?;
                let access = self.create_property_access(left, right_clone)?;
                self.context.factory()?.set_text_range(access, name)
            }
            _ => {
                let clone = self.context.factory()?.clone_node(name)?;
                self.context.factory()?.set_text_range(clone, name)
            }
        }
    }

    fn create_awaiter_call(
        &mut self,
        has_lexical_this: bool,
        arguments_expression: Option<TransformNode>,
        inner_parameters: Option<NodeArrayId>,
        body: TransformNode,
        promise_constructor: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
""", "signature")
rep("""        let promise = self.create_void_zero()?;
        self.create_call(helper, vec![this_arg, arguments, promise, generator])
""", """        let promise = match promise_constructor {
            Some(name) => self.create_expression_from_entity_name(name)?,
            None => self.create_void_zero()?,
        };
        self.create_call(helper, vec![this_arg, arguments, promise, generator])
""", "promise arg")
rep("""use crate::{
    factory::EmitHelperName, EmitFlags, EmitResolver, EmitResolverNode, LexicalEnvironment,
    TransformError, TransformFlags, TransformNode, TransformNodeArray, TransformRoot,
    TransformSourceId, TransformationContext, Transformer,
};
""", """use crate::{
    factory::EmitHelperName, EmitFlags, EmitResolver, EmitResolverNode,
    EmitTypeReferenceSerializationKind, LexicalEnvironment, TransformError, TransformFlags,
    TransformNode, TransformNodeArray, TransformRoot, TransformSourceId, TransformationContext,
    Transformer,
};
""", "imports")
open(p, "w").write(s)
print("EF2-PROMISE-CTOR patch applied (check imports: ScriptTarget, EmitTypeReferenceSerializationKind, create_property_access signature)")
