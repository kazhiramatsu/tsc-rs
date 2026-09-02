use std::collections::BTreeMap;
use std::mem;

use tsc_syntax::{FileReference, NodeId};

use crate::{TransformError, TransformNode, TransformSourceId, TransformationContext};

use super::subtree::preserve_js_doc;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RawFileReferences {
    pub(crate) referenced: Vec<(TransformSourceId, FileReference)>,
    pub(crate) type_directives: Vec<FileReference>,
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
}

impl TransformState {
    /// tsrs-native: source-only transformDeclarations state reset
    /// (_tsc.js:114513-114530; bundle reset is deliberately absent).
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
        }
    }

    /// tsrs-native: owned enclosingDeclaration frame; restoration precedes
    /// Result propagation and never relies on Drop.
    pub(crate) fn with_enclosing_declaration<R>(
        &mut self,
        value: Option<TransformNode>,
        body: impl FnOnce(&mut Self) -> Result<R, TransformError>,
    ) -> Result<R, TransformError> {
        let saved = mem::replace(&mut self.enclosing_declaration, value);
        let result = body(self);
        self.enclosing_declaration = saved;
        result
    }

    /// tsrs-native: owned needsDeclare frame; restoration precedes Result
    /// propagation and never relies on Drop.
    pub(crate) fn with_needs_declare<R>(
        &mut self,
        value: bool,
        body: impl FnOnce(&mut Self) -> Result<R, TransformError>,
    ) -> Result<R, TransformError> {
        let saved = mem::replace(&mut self.needs_declare, value);
        let result = body(self);
        self.needs_declare = saved;
        result
    }

    /// tsrs-native: owned ModuleBlock scope-marker frame.
    pub(crate) fn with_scope_markers<R>(
        &mut self,
        needs_scope_fix_marker: bool,
        result_has_scope_marker: bool,
        body: impl FnOnce(&mut Self) -> Result<R, TransformError>,
    ) -> Result<R, TransformError> {
        let saved_needs = mem::replace(&mut self.needs_scope_fix_marker, needs_scope_fix_marker);
        let saved_result = mem::replace(&mut self.result_has_scope_marker, result_has_scope_marker);
        let result = body(self);
        self.needs_scope_fix_marker = saved_needs;
        self.result_has_scope_marker = saved_result;
        result
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
