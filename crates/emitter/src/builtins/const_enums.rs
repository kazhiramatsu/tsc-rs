//! Const enum inlining after the module transform, tsgo's
//! `ConstEnumInliningTransformer` (transformers/inliners/constenum.go), which
//! replaces tsc's print-time `substituteConstantValue`. The replacement is
//! part of the tree before printing, so the printer lays it out as
//! synthesized: a line break the source had before the access is not kept.

use tsc_syntax::{
    apply_child_slots, child_slots, map_child_slots, ChildSlots, NodeArrayId, NodeData,
    NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags};

use super::{
    safe_multi_line_comment, skip_trivia, update_node_array_lazily, ArrayElementVisit,
    LazyChildVisitor,
};
use crate::{
    EmitConstantValue, EmitResolver, EmitResolverNode, SyntheticComment, SyntheticCommentKind,
    TransformError, TransformFlags, TransformNode, TransformNodeArray, TransformRoot,
    TransformSourceId, TransformationContext, Transformer,
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
        let parsed = match ParsedCandidates::scan(context, source, self.resolver)? {
            Scan::NoCandidate => return Ok(root),
            Scan::Candidates(parsed) => Some(parsed),
            Scan::Unscanned => None,
        };
        let current = context.arena().root(source)?;
        let mut visitor = ConstEnumInliningVisitor {
            context,
            source,
            resolver: self.resolver,
            remove_comments: self.remove_comments,
            parsed,
        };
        let updated = visitor.visit(current.node())?;
        if updated != current {
            visitor.context.arena_mut()?.replace_root(source, updated)?;
        }
        Ok(root)
    }
}

/// What the scan of a file's parsed accesses found.
enum Scan {
    /// No parsed access has a constant value, so no visit changes the file.
    NoCandidate,
    Candidates(ParsedCandidates),
    /// The file has no program source to ask the resolver about, or its
    /// checker has not checked it: the visit asks each access it reaches.
    Unscanned,
}

/// The parsed accesses that may have a constant value, and their ancestors.
/// The transforms never change a parsed node's children in place, so a
/// parsed node in the transformed tree still roots its parsed subtree: the
/// visit skips one that holds no candidate, where tsgo's visits every node
/// and changes none of them.
struct ParsedCandidates {
    base: u32,
    /// Per parsed node: [`Self::CONTAINS`] when its subtree holds a
    /// candidate, and [`Self::CANDIDATE`] when it is one.
    marks: Vec<u8>,
}

impl ParsedCandidates {
    const CONTAINS: u8 = 1;
    const CANDIDATE: u8 = 2;

    /// Asks the resolver about every parsed property or element access in
    /// the tree (JSDoc excluded: the transformed tree never holds it). An
    /// access the resolver fails on is a candidate, so the visit reaches it
    /// and reports the failure as before.
    fn scan(
        context: &TransformationContext,
        source: TransformSourceId,
        resolver: &dyn EmitResolver,
    ) -> Result<Scan, TransformError> {
        let transform_source = context.arena().source(source)?;
        let Some(program_source) = transform_source.program_source() else {
            return Ok(Scan::Unscanned);
        };
        // The resolver checks an access of an unchecked source to answer
        // (`GetConstantValue`, checker/services.go:869-888). Asking every
        // parsed access would check the ones the earlier transforms erased
        // (`implements a.B`, a type query), which tsgo's inliner never sees:
        // the visit asks the accesses of the transformed tree, in its order.
        if !resolver.is_source_checked(program_source)? {
            return Ok(Scan::Unscanned);
        }
        let syntax = transform_source.syntax();
        let base = transform_source.parsed_node_base();
        let count = u32::try_from(transform_source.parsed_node_count())
            .expect("parsed node count fits node ids");
        let mut candidates = Vec::new();
        for index in base..base + count {
            let id = NodeId::new(index);
            let record = syntax.arena.node(id);
            if !matches!(
                record.kind,
                SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
            ) || record.parent.is_none()
                || NodeFlags::from_bits(record.flags).contains(NodeFlags::JS_DOC)
            {
                continue;
            }
            if !matches!(
                resolver.get_constant_value(EmitResolverNode::new(program_source, id)),
                Ok(None)
            ) {
                candidates.push(id);
            }
        }
        if candidates.is_empty() {
            return Ok(Scan::NoCandidate);
        }
        let mut marks = vec![0u8; count as usize];
        for candidate in candidates {
            marks[(candidate.index() - base) as usize] |= Self::CANDIDATE;
            let mut current = Some(candidate);
            while let Some(id) = current {
                let mark = &mut marks[(id.index() - base) as usize];
                if *mark & Self::CONTAINS != 0 {
                    break;
                }
                *mark |= Self::CONTAINS;
                current = syntax.arena.node(id).parent;
            }
        }
        Ok(Scan::Candidates(Self { base, marks }))
    }

    /// The marks of `id` when it is a parsed node.
    fn marks(&self, id: NodeId) -> Option<u8> {
        let index = id.index().wrapping_sub(self.base) as usize;
        self.marks.get(index).copied()
    }
}

struct ConstEnumInliningVisitor<'context, 'resolver> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    resolver: &'resolver dyn EmitResolver,
    remove_comments: bool,
    parsed: Option<ParsedCandidates>,
}

impl ConstEnumInliningVisitor<'_, '_> {
    /// `VisitEachChild` keeps a node whose children are unchanged, and an
    /// update keeps the node's transform flags: nothing reads them after the
    /// last transform, so neither takes the generic update's reconciliation.
    fn visit(&mut self, id: NodeId) -> Result<TransformNode, TransformError> {
        let node = TransformNode::new(self.source, id);
        let marks = self.parsed.as_ref().and_then(|parsed| parsed.marks(id));
        if marks.is_some_and(|marks| marks & ParsedCandidates::CONTAINS == 0) {
            return Ok(node);
        }
        let (kind, mut slots) = {
            let record = self.context.arena().node(node)?;
            let slots = if matches!(record.data, NodeData::MissingDeclaration(_)) {
                ChildSlots::new()
            } else {
                child_slots(&record.data, &*self)
            };
            (record.kind, slots)
        };
        if matches!(
            kind,
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        ) && marks.is_none_or(|marks| marks & ParsedCandidates::CANDIDATE != 0)
        {
            if let Some(substitute) =
                constant_substitute(self.context, self.resolver, self.remove_comments, node)?
            {
                return Ok(substitute);
            }
        }
        if !map_child_slots(&mut slots, self)? {
            return Ok(node);
        }
        let (mut data, flags) = {
            let arena = self.context.arena();
            (arena.node(node)?.data.clone(), arena.transform_flags(node))
        };
        apply_child_slots(&mut data, &slots, self)?;
        self.context.factory()?.update_node(node, data, flags)
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
