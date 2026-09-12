//! Source-owned import/require rewrite selection shared by module transforms.

use std::collections::VecDeque;

use super::*;

/// The ordered identity queue belongs to one module-transform instance. An
/// unvisited nested call remains at the head, including across source roots.
#[derive(Default)]
pub(super) struct ImportCallRewrites {
    pending: VecDeque<TransformNode>,
}

impl ImportCallRewrites {
    /// tsc-port: forEachDynamicImportOrRequireCall @6.0.3
    /// tsc-hash: 18fad0df970874a5281d71546ad89276245efc168a40dc588e26ae3fee67a42d
    /// tsc-span: _tsc.js:20016-20039
    pub(super) fn append(
        &mut self,
        arena: &TransformArena,
        root: TransformNode,
    ) -> Result<(), TransformError> {
        let source = arena.source(root.source())?.syntax();
        let javascript = arena.node(root)?.flags & NodeFlags::JAVA_SCRIPT_FILE.bits() != 0;
        // These are the source scanner's candidate positions, not call
        // recognition: every hit must resolve to a qualifying AST node.
        let mut positions = source
            .text()
            .match_indices("import")
            .chain(source.text().match_indices("require"))
            .map(|(start, spelling)| start + spelling.len())
            .collect::<Vec<_>>();
        positions.sort_unstable();
        for position in positions {
            let node = node_at_position(arena, root, position)?;
            let NodeData::CallExpression(call) = &arena.node(node)?.data else {
                continue;
            };
            let Some(callee) = call
                .expression
                .and_then(|id| arena.node_ref(root.source(), id))
            else {
                continue;
            };
            let arguments = node_array_nodes(arena, root.source(), call.arguments)?;
            let Some(first) = arguments.first() else {
                continue;
            };
            let callee = arena.node(callee)?;
            // isRequireCall (_tsc.js:14901-14914) is syntactic even when a
            // local declaration shadows require. TypeScript files are excluded.
            let require = javascript
                && arguments.len() == 1
                && matches!(&callee.data, NodeData::Identifier(name) if name.escaped_text == "require");
            if !require && callee.kind != SyntaxKind::ImportKeyword {
                continue;
            }
            let literal = match &arena.node(*first)?.data {
                NodeData::StringLiteral(data) => Some(data.text.as_str()),
                NodeData::NoSubstitutionTemplateLiteral(data) => Some(data.text.as_str()),
                _ => None,
            };
            if literal.is_none_or(|text| rewrite_relative_module_specifier(text).is_some()) {
                self.pending.push_back(node);
            }
        }
        Ok(())
    }

    pub(super) fn take(&mut self, node: TransformNode) -> bool {
        if self.pending.front() != Some(&node) {
            return false;
        }
        self.pending.pop_front();
        true
    }
}

/// tsc-port: getNodeAtPosition @6.0.3
/// tsc-hash: fc5235720fccffb0238b56e00eae48c726391a7ce760c4560b6ccdf11c4f393c
/// tsc-span: _tsc.js:20040-20055
fn node_at_position(
    arena: &TransformArena,
    mut current: TransformNode,
    position: usize,
) -> Result<TransformNode, TransformError> {
    let syntax = arena.source(current.source())?.syntax();
    loop {
        let mut containing = None;
        for_each_child(&syntax.arena, arena.node(current)?, |child| {
            let record = syntax.arena.node(child);
            // Raw synthetic positions represent tsc's -1, including nodes
            // with only one synthesized endpoint.
            let pos = if record.pos == u32::MAX {
                -1
            } else {
                i64::from(record.pos)
            };
            let end = if record.end == u32::MAX {
                -1
            } else {
                i64::from(record.end)
            };
            let position = position as i64;
            if pos <= position
                && (position < end || position == end && record.kind == SyntaxKind::EndOfFileToken)
            {
                containing = Some(child);
                true
            } else {
                false
            }
        });
        let Some(child) = containing else {
            return Ok(current);
        };
        let child = arena
            .node_ref(current.source(), child)
            .ok_or(TransformError::UnknownNode(TransformNode::new(
                current.source(),
                child,
            )))?;
        if arena.node(child)?.kind == SyntaxKind::MetaProperty {
            return Ok(current);
        }
        current = child;
    }
}

/// tsc-port: rewriteModuleSpecifier @6.0.3
/// tsc-hash: f922e640861acb3c4f3e223a052ecf480ebdc989e9c1d4b545efca742e40aace
/// tsc-span: _tsc.js:93242-93248
pub(super) fn rewrite_literal(
    context: &mut TransformationContext,
    node: TransformNode,
    preserve_jsx: bool,
) -> Result<TransformNode, TransformError> {
    let NodeData::StringLiteral(literal) = context.arena().node(node)?.data.clone() else {
        return Ok(node);
    };
    let Some(mut text) = rewrite_relative_module_specifier(&literal.text) else {
        return Ok(node);
    };
    if preserve_jsx && literal.text.ends_with(".tsx") {
        text.push('x');
    }
    let flags = context.arena().transform_flags(node);
    context.factory()?.update_node(
        node,
        NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
            text,
            has_extended_unicode_escape: literal.has_extended_unicode_escape,
        }),
        flags,
    )
}

/// tsc-port: createRewriteRelativeImportExtensionsHelper @6.0.3
/// tsc-hash: 9f8e5ae8ebf093baa14531dc1f3004059db1d12d91bc0e509c9ef8a880761e0a
/// tsc-span: _tsc.js:26013-26021
pub(super) fn rewrite_argument(
    context: &mut TransformationContext,
    argument: TransformNode,
    preserve_jsx: bool,
) -> Result<TransformNode, TransformError> {
    if matches!(
        context.arena().node(argument)?.kind,
        SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
    ) {
        return rewrite_literal(context, argument, preserve_jsx);
    }
    context.request_emit_helper(crate::EmitHelper::with_text(
        "typescript:rewriteRelativeImportExtensions",
        false,
        REWRITE_RELATIVE_IMPORT_EXTENSIONS_HELPER_TEXT,
        None,
        Vec::new(),
    ))?;
    let source = argument.source();
    let helper = context.factory()?.create_unscoped_helper_identifier(
        source,
        EmitHelperName::RewriteRelativeImportExtension,
    )?;
    let mut arguments = vec![argument];
    if preserve_jsx {
        arguments.push(context.factory()?.create_token(
            source,
            SyntaxKind::TrueKeyword,
            TransformFlags::NONE,
        )?);
    }
    let arguments = context.factory()?.create_node_array(source, arguments)?;
    context.factory()?.create_node(
        source,
        NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
            expression: Some(helper.node()),
            question_dot_token: None,
            type_arguments: None,
            arguments: Some(arguments.array()),
        }),
        TransformFlags::NONE,
    )
}
