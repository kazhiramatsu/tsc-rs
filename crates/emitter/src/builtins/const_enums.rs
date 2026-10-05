//! Const enum inlining after the module transform, tsgo's
//! `ConstEnumInliningTransformer` (transformers/inliners/constenum.go), which
//! replaces tsc's print-time `substituteConstantValue`. The replacement is
//! part of the tree before printing, so the printer lays it out as
//! synthesized: a line break the source had before the access is not kept.

use tsc_syntax::{NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind};
use tsc_types::CompilerOptions;

use super::{
    safe_multi_line_comment, skip_trivia, update_children_lazily, update_node_array_lazily,
    ArrayElementVisit, LazyChildVisitor,
};
use crate::{
    EmitConstantValue, EmitResolver, SyntheticComment, SyntheticCommentKind, TransformError,
    TransformFlags, TransformNode, TransformNodeArray, TransformRoot, TransformSourceId,
    TransformationContext, Transformer,
};

pub(super) fn transform_const_enum_inlining<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    Box::new(ConstEnumInliner {
        resolver,
        remove_comments: options.remove_comments == Some(true),
    })
}

struct ConstEnumInliner<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    remove_comments: bool,
}

impl Transformer for ConstEnumInliner<'_> {
    fn name(&self) -> &'static str {
        "inlineConstEnums"
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
        let current = context.arena().root(source)?;
        let mut visitor = ConstEnumInliningVisitor {
            context,
            source,
            resolver: self.resolver,
            remove_comments: self.remove_comments,
        };
        let updated = update_children_lazily(&mut visitor, current)?;
        if updated != current {
            visitor.context.arena_mut()?.replace_root(source, updated)?;
        }
        Ok(root)
    }
}

struct ConstEnumInliningVisitor<'context, 'resolver> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    resolver: &'resolver dyn EmitResolver,
    remove_comments: bool,
}

impl ConstEnumInliningVisitor<'_, '_> {
    fn visit(&mut self, id: NodeId) -> Result<TransformNode, TransformError> {
        let node = TransformNode::new(self.source, id);
        if matches!(
            self.context.arena().node(node)?.kind,
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        ) {
            if let Some(substitute) =
                constant_substitute(self.context, self.resolver, self.remove_comments, node)?
            {
                return Ok(substitute);
            }
        }
        update_children_lazily(self, node)
    }
}

impl NodeDataChildVisitor for ConstEnumInliningVisitor<'_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(TransformNode::new(self.source, id))
            .expect("const enum inlining child belongs to its transform source")
            .kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.visit(id).map(|node| Some(node.node()))
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        let original = TransformNodeArray::new(self.source, id);
        let updated = update_node_array_lazily(self, original, |visitor, element| {
            Ok(ArrayElementVisit::One(visitor.visit(element)?))
        })?;
        Ok(Some(updated.array()))
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}

impl LazyChildVisitor for ConstEnumInliningVisitor<'_, '_> {
    fn transformation_context(&self) -> &TransformationContext {
        self.context
    }

    fn transformation_context_mut(&mut self) -> &mut TransformationContext {
        self.context
    }
}

/// The replacement of one property or element access whose value is a
/// constant: a literal with the access's text as a trailing comment, with no
/// range and no original (`ConstEnumInliningTransformer.visit`,
/// transformers/inliners/constenum.go:32-89).
fn constant_substitute(
    context: &mut TransformationContext,
    resolver: &dyn EmitResolver,
    remove_comments: bool,
    node: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let original = context.arena().get_original_node(node);
    if !matches!(
        context.arena().node(original)?.kind,
        SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
    ) {
        return Ok(None);
    }
    let Some(resolver_node) = context.arena().parse_tree_resolver_node(node)? else {
        return Ok(None);
    };
    let Some(value) = resolver.get_constant_value(resolver_node)? else {
        return Ok(None);
    };

    let trailing_comment = if remove_comments {
        None
    } else {
        let source = context.arena().source(original.source())?.syntax();
        let record = context.arena().node(original)?;
        let text = if record.end == u32::MAX || record.pos > record.end {
            String::new()
        } else {
            let start = skip_trivia(source.text(), record.pos as usize);
            safe_multi_line_comment(
                source
                    .text()
                    .get(start..record.end as usize)
                    .unwrap_or_default(),
            )
        };
        Some(text)
    };

    let source = node.source();
    let substitute = {
        let mut factory = context.factory()?;
        let substitute = match &value {
            EmitConstantValue::String(value) => factory.create_node(
                source,
                NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
                    text: tsc_types::JsString::from_code_units(value.code_units()),
                }),
                TransformFlags::NONE,
            )?,
            EmitConstantValue::Number(value) => {
                let value = value.as_f64();
                if value.is_nan() {
                    factory.create_node(
                        source,
                        NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                            escaped_text: tsc_types::EscapedName::from_identifier_escaped_text(
                                "NaN",
                            ),
                        }),
                        TransformFlags::NONE,
                    )?
                } else if value.is_infinite() {
                    let infinity = factory.create_node(
                        source,
                        NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                            escaped_text: tsc_types::EscapedName::from_identifier_escaped_text(
                                "Infinity",
                            ),
                        }),
                        TransformFlags::NONE,
                    )?;
                    if value.is_sign_negative() {
                        factory.create_node(
                            source,
                            NodeData::PrefixUnaryExpression(
                                tsc_syntax::nodes::PrefixUnaryExpressionData {
                                    operator: SyntaxKind::MinusToken,
                                    operand: Some(infinity.node()),
                                },
                            ),
                            TransformFlags::NONE,
                        )?
                    } else {
                        infinity
                    }
                } else {
                    let magnitude = if value < 0.0 { -value } else { value };
                    let literal = factory.create_node(
                        source,
                        NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                            text: tsc_types::js_number_to_string(magnitude),
                        }),
                        TransformFlags::NONE,
                    )?;
                    if value < 0.0 {
                        factory.create_node(
                            source,
                            NodeData::PrefixUnaryExpression(
                                tsc_syntax::nodes::PrefixUnaryExpressionData {
                                    operator: SyntaxKind::MinusToken,
                                    operand: Some(literal.node()),
                                },
                            ),
                            TransformFlags::NONE,
                        )?
                    } else {
                        literal
                    }
                }
            }
            EmitConstantValue::Boolean(value) => factory.create_token(
                source,
                if *value {
                    SyntaxKind::TrueKeyword
                } else {
                    SyntaxKind::FalseKeyword
                },
                TransformFlags::NONE,
            )?,
        };
        substitute
    };
    // TypeScript returns the synthetic constant directly. Giving it the
    // access expression's range/original adds node and token map spans
    // which the substitution pipeline does not emit.
    if let Some(text) = trailing_comment {
        context
            .arena_mut()?
            .metadata_mut(substitute)
            .add_trailing_comment(SyntheticComment::new(
                SyntheticCommentKind::MultiLine,
                format!(" {text} "),
                false,
                false,
            ));
    }
    Ok(Some(substitute))
}
