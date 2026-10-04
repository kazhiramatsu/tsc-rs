use std::collections::BTreeMap;

use tsc_syntax::{FileReference, NodeId, TypeReferenceDirective};

use crate::{TransformError, TransformNode, TransformSourceId, TransformationContext};

use super::subtree::preserve_js_doc;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RawFileReferences {
    pub(crate) referenced: Vec<(TransformSourceId, FileReference)>,
    pub(crate) type_directives: Vec<TypeReferenceDirective>,
    pub(crate) lib_directives: Vec<FileReference>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum VisitResult {
    None,
    Node(TransformNode),
    Nodes(Vec<TransformNode>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TransformState {
    pub(crate) needs_declare: bool,
    pub(crate) is_bundled_emit: bool,
    pub(crate) result_has_external_module_indicator: bool,
    pub(crate) needs_scope_fix_marker: bool,
    pub(crate) result_has_scope_marker: bool,
    pub(crate) enclosing_declaration: Option<TransformNode>,
    pub(crate) late_statement_replacement: BTreeMap<NodeId, VisitResult>,
    pub(crate) current_source_file: TransformSourceId,
    pub(crate) references: RawFileReferences,
    pub(crate) expandos: super::expando::ExpandoState,
}

impl TransformState {
    /// tsrs-native: source-only transformDeclarations state reset
    /// (_tsc.js:114513-114530). The bundle visitor starts from the same
    /// source-local state and then sets its bundle/ambient-wrapper flags.
    pub(crate) fn for_source(source: TransformSourceId, root: TransformNode) -> Self {
        Self {
            needs_declare: true,
            is_bundled_emit: false,
            result_has_external_module_indicator: false,
            needs_scope_fix_marker: false,
            result_has_scope_marker: false,
            enclosing_declaration: Some(root),
            late_statement_replacement: BTreeMap::new(),
            current_source_file: source,
            references: RawFileReferences::default(),
            expandos: super::expando::ExpandoState::default(),
        }
    }
}

/// tsrs-native: shared visitor-result provenance adoption (h2-7a-m-4 §5.2).
pub(crate) fn adopt_result(
    cx: &mut TransformationContext,
    input: TransformNode,
    result: VisitResult,
) -> Result<VisitResult, TransformError> {
    match result {
        VisitResult::Node(output) if output != input => {
            let output = preserve_js_doc(cx, output, input)?;
            cx.arena_mut()?.set_original_node(output, Some(input))?;
            Ok(VisitResult::Node(output))
        }
        other => Ok(other),
    }
}
