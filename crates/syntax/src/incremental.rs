use crate::for_each_child::for_each_child;
use crate::nodes::{NodeArrayId, NodeId, SourceFileData};
use crate::parser;
use crate::relocate::{collect_node_data_ids, node_data_structurally_equal};
use crate::{CommentDirective, NodeData, ParseOptions, SourceFile, SyntaxKind};
use std::collections::{hash_map::Entry, HashMap};
use std::sync::Arc;
use tsc_diagnostics::{
    sort_and_dedupe_diagnostics, ByteTextChangeRange, ByteTextSpan, Diagnostic, TextSnapshot,
};
use tsc_types::{IdentityDomain, IdentityError, NodeFlags};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IncrementalParseOptions {
    /// Validate unchanged prefix/suffix text for every accepted change and
    /// retain per-list-element lineage for tests and qualification evidence.
    pub record_reuse_lineage: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReuseLineage {
    pub old_node: NodeId,
    pub new_node: NodeId,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IncrementalParseStats {
    pub incremental: bool,
    pub full_parse_fallback: bool,
    pub reused_list_elements: usize,
    pub reused_nodes: usize,
    pub reused_node_arrays: usize,
    pub freshly_parsed_nodes: usize,
    pub lineage: Vec<ReuseLineage>,
    pub(crate) reused_old_ranges: Vec<(u32, u32, i64)>,
    pub(crate) reused_range_roots: Vec<NodeId>,
    pub(crate) reused_roots: Vec<NodeId>,
    pub(crate) reused_new_nodes: Vec<NodeId>,
    pub(crate) reused_new_arrays: Vec<NodeArrayId>,
}

impl IncrementalParseStats {
    pub(crate) fn finish(&mut self, source: &SourceFile) {
        self.freshly_parsed_nodes = source.node_count().saturating_sub(self.reused_nodes);
        self.reused_range_roots.clear();
        self.reused_roots.clear();
        self.reused_new_nodes.clear();
        self.reused_new_arrays.clear();
    }

    fn retain_reachable_reuse(&mut self, source: &SourceFile) {
        let reachable = reachable_identity_graph(source);
        self.reused_roots
            .retain(|root| reachable.contains_node(*root));
        self.reused_list_elements = self.reused_roots.len();
        self.reused_new_nodes
            .retain(|node| reachable.contains_node(*node));
        self.reused_nodes = self.reused_new_nodes.len();
        self.reused_new_arrays
            .retain(|array| reachable.contains_array(*array));
        self.reused_node_arrays = self.reused_new_arrays.len();
        self.lineage
            .retain(|entry| reachable.contains_node(entry.new_node));

        let mut retained_ranges = Vec::with_capacity(self.reused_old_ranges.len());
        for (range, root) in self
            .reused_old_ranges
            .drain(..)
            .zip(self.reused_range_roots.drain(..))
        {
            if reachable.contains_node(root) {
                retained_ranges.push(range);
            }
        }
        self.reused_old_ranges = retained_ranges;
    }

    fn relocate_new_ids(&mut self, old_base: u32, new_base: u32) {
        for lineage in &mut self.lineage {
            let offset = lineage
                .new_node
                .0
                .checked_sub(old_base)
                .expect("incremental lineage belongs to the local result arena");
            lineage.new_node = NodeId(
                new_base
                    .checked_add(offset)
                    .expect("relocated incremental lineage overflows"),
            );
        }
    }
}

struct ReachableIdentityGraph {
    node_base: u32,
    array_base: u32,
    nodes: Vec<bool>,
    arrays: Vec<bool>,
}

impl ReachableIdentityGraph {
    fn contains_node(&self, id: NodeId) -> bool {
        id.0.checked_sub(self.node_base)
            .and_then(|index| self.nodes.get(index as usize))
            .copied()
            .unwrap_or(false)
    }

    fn contains_array(&self, id: NodeArrayId) -> bool {
        id.0.checked_sub(self.array_base)
            .and_then(|index| self.arrays.get(index as usize))
            .copied()
            .unwrap_or(false)
    }
}

fn reachable_identity_graph(source: &SourceFile) -> ReachableIdentityGraph {
    let node_base = source.arena.node_base();
    let array_base = source.arena.array_base();
    let mut nodes = vec![false; source.arena.nodes().len()];
    let mut arrays = vec![false; source.arena.node_arrays().len()];
    let mut pending_nodes = vec![source.root];
    let mut pending_arrays = Vec::new();
    while !pending_nodes.is_empty() || !pending_arrays.is_empty() {
        if let Some(id) = pending_nodes.pop() {
            let index = (id.0 - node_base) as usize;
            if nodes[index] {
                continue;
            }
            nodes[index] = true;
            let node = source.arena.node(id);
            collect_node_data_ids(&node.data, &mut pending_nodes, &mut pending_arrays);
            if let Some(js_doc) = node.js_doc {
                pending_arrays.push(js_doc);
            }
            continue;
        }
        let id = pending_arrays
            .pop()
            .expect("a non-empty reachability work list has an element");
        let index = (id.0 - array_base) as usize;
        if !arrays[index] {
            arrays[index] = true;
            pending_nodes.extend(source.arena.node_array(id).nodes.iter().copied());
        }
    }
    ReachableIdentityGraph {
        node_base,
        array_base,
        nodes,
        arrays,
    }
}

fn syntax_graphs_equal(left: &SourceFile, right: &SourceFile) -> bool {
    let mut left_nodes = vec![None; left.arena.nodes().len()];
    let mut right_nodes = vec![None; right.arena.nodes().len()];
    let mut left_arrays = vec![None; left.arena.node_arrays().len()];
    let mut right_arrays = vec![None; right.arena.node_arrays().len()];
    let mut pending_nodes = vec![(left.root, right.root)];
    let mut pending_arrays = Vec::new();

    match (
        left.external_module_indicator,
        right.external_module_indicator,
    ) {
        (Some(left), Some(right)) => pending_nodes.push((left, right)),
        (None, None) => {}
        _ => return false,
    }

    while !pending_nodes.is_empty() || !pending_arrays.is_empty() {
        if let Some((left_id, right_id)) = pending_nodes.pop() {
            let Some(left_index) = left_id
                .0
                .checked_sub(left.arena.node_base())
                .map(|index| index as usize)
                .filter(|index| *index < left_nodes.len())
            else {
                return false;
            };
            let Some(right_index) = right_id
                .0
                .checked_sub(right.arena.node_base())
                .map(|index| index as usize)
                .filter(|index| *index < right_nodes.len())
            else {
                return false;
            };
            match (left_nodes[left_index], right_nodes[right_index]) {
                (Some(mapped_right), Some(mapped_left)) => {
                    if mapped_right != right_id || mapped_left != left_id {
                        return false;
                    }
                    continue;
                }
                (None, None) => {
                    left_nodes[left_index] = Some(right_id);
                    right_nodes[right_index] = Some(left_id);
                }
                _ => return false,
            }

            let left_node = left.arena.node(left_id);
            let right_node = right.arena.node(right_id);
            if left_node.kind != right_node.kind
                || left_node.flags != right_node.flags
                || left_node.numeric_literal_flags != right_node.numeric_literal_flags
                || left_node.multi_line != right_node.multi_line
                || left_node.pos != right_node.pos
                || left_node.end != right_node.end
            {
                return false;
            }
            match (left_node.parent, right_node.parent) {
                (Some(left), Some(right)) => pending_nodes.push((left, right)),
                (None, None) => {}
                _ => return false,
            }
            match (left_node.js_doc, right_node.js_doc) {
                (Some(left), Some(right)) => pending_arrays.push((left, right)),
                (None, None) => {}
                _ => return false,
            }
            if !node_data_structurally_equal(
                &left_node.data,
                &right_node.data,
                |left, right| {
                    pending_nodes.push((left, right));
                    true
                },
                |left, right| {
                    pending_arrays.push((left, right));
                    true
                },
            ) {
                return false;
            }
            continue;
        }

        let (left_id, right_id) = pending_arrays
            .pop()
            .expect("a non-empty structural-comparison work list has an element");
        let Some(left_index) = left_id
            .0
            .checked_sub(left.arena.array_base())
            .map(|index| index as usize)
            .filter(|index| *index < left_arrays.len())
        else {
            return false;
        };
        let Some(right_index) = right_id
            .0
            .checked_sub(right.arena.array_base())
            .map(|index| index as usize)
            .filter(|index| *index < right_arrays.len())
        else {
            return false;
        };
        match (left_arrays[left_index], right_arrays[right_index]) {
            (Some(mapped_right), Some(mapped_left)) => {
                if mapped_right != right_id || mapped_left != left_id {
                    return false;
                }
                continue;
            }
            (None, None) => {
                left_arrays[left_index] = Some(right_id);
                right_arrays[right_index] = Some(left_id);
            }
            _ => return false,
        }

        let left_array = left.arena.node_array(left_id);
        let right_array = right.arena.node_array(right_id);
        if left_array.pos != right_array.pos
            || left_array.end != right_array.end
            || left_array.has_trailing_comma != right_array.has_trailing_comma
            || left_array.is_missing_list != right_array.is_missing_list
            || left_array.nodes.len() != right_array.nodes.len()
        {
            return false;
        }
        pending_nodes.extend(
            left_array
                .nodes
                .iter()
                .copied()
                .zip(right_array.nodes.iter().copied()),
        );
    }

    true
}

/// Qualification-facing exact comparison for a fresh and an incrementally
/// produced immutable source. Numeric identity domains and allocation order
/// are normalized; every reachable node/array field and every source-owned
/// diagnostic/directive/module fact remains exact.
#[doc(hidden)]
pub fn source_files_structurally_equal(left: &SourceFile, right: &SourceFile) -> bool {
    left.file_name == right.file_name
        && left.text() == right.text()
        && left.language_version == right.language_version
        && left.language_variant == right.language_variant
        && left.is_declaration_file == right.is_declaration_file
        && left.js_doc_parsing_mode == right.js_doc_parsing_mode
        && syntax_graphs_equal(left, right)
        && left.parse_diagnostics == right.parse_diagnostics
        && left.js_doc_diagnostics == right.js_doc_diagnostics
        && left.referenced_files == right.referenced_files
        && left.type_reference_directives == right.type_reference_directives
        && left.lib_reference_directives == right.lib_reference_directives
        && left.amd_dependencies == right.amd_dependencies
        && left.module_name == right.module_name
        && left.has_jsx_import_source_pragma == right.has_jsx_import_source_pragma
        && left.jsx_import_source_pragma == right.jsx_import_source_pragma
        && left.has_jsx_runtime_pragma == right.has_jsx_runtime_pragma
        && left.jsx_runtime_pragma == right.jsx_runtime_pragma
        && left.comment_directives == right.comment_directives
}

#[derive(Clone, Debug)]
pub struct IncrementalParseResult {
    pub source: Arc<SourceFile>,
    pub stats: IncrementalParseStats,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IncrementalParseError {
    ChangeRangeOverflow,
    OldRangeOutOfBounds { end: u32, old_length: u32 },
    NewRangeOutOfBounds { end: u32, new_length: u32 },
    InvalidOldScalarBoundary { position: u32 },
    InvalidNewScalarBoundary { position: u32 },
    LengthMismatch { expected: u32, actual: u32 },
    PrefixMismatch,
    SuffixMismatch,
    IdentityDomainMismatch,
    Identity(IdentityError),
}

impl std::fmt::Display for IncrementalParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChangeRangeOverflow => formatter.write_str("incremental change range overflows"),
            Self::OldRangeOutOfBounds { end, old_length } => write!(
                formatter,
                "incremental old range ends at {end}, beyond old byte length {old_length}"
            ),
            Self::NewRangeOutOfBounds { end, new_length } => write!(
                formatter,
                "incremental new range ends at {end}, beyond new byte length {new_length}"
            ),
            Self::InvalidOldScalarBoundary { position } => write!(
                formatter,
                "incremental old byte position {position} is not a Unicode scalar boundary"
            ),
            Self::InvalidNewScalarBoundary { position } => write!(
                formatter,
                "incremental new byte position {position} is not a Unicode scalar boundary"
            ),
            Self::LengthMismatch { expected, actual } => write!(
                formatter,
                "incremental change predicts {expected} bytes, but the new snapshot has {actual}"
            ),
            Self::PrefixMismatch => {
                formatter.write_str("incremental change leaves a mismatched prefix")
            }
            Self::SuffixMismatch => {
                formatter.write_str("incremental change leaves a mismatched suffix")
            }
            Self::IdentityDomainMismatch => formatter.write_str(
                "incremental source belongs to a different or unmanaged identity domain",
            ),
            Self::Identity(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for IncrementalParseError {}

impl From<IdentityError> for IncrementalParseError {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ReusableNode {
    pub(crate) old_node: NodeId,
    pub(crate) adjusted_pos: u32,
    pub(crate) adjusted_end: u32,
    pub(crate) position_delta: i64,
    pub(crate) intersects_change: bool,
}

#[derive(Debug)]
struct SyntaxCursorData {
    source: Arc<SourceFile>,
    nodes_by_position: HashMap<u32, (usize, ReusableNode)>,
}

/// Immutable Rust adaptation of tsc's stateful syntax cursor.
///
/// The index owns the old source through one document-granular `Arc`. Cloning
/// the public cursor only clones this index handle; it never clones the old
/// arena. Lookup returns the highest old list element at a requested adjusted
/// byte position.
#[derive(Clone, Debug)]
pub struct SyntaxCursor {
    data: Arc<SyntaxCursorData>,
}

impl SyntaxCursor {
    pub(crate) fn source(&self) -> &Arc<SourceFile> {
        &self.data.source
    }

    pub(crate) fn current_node(&self, position: u32) -> Option<ReusableNode> {
        self.data
            .nodes_by_position
            .get(&position)
            .map(|(_, candidate)| *candidate)
    }

    fn from_affected_range(
        source: Arc<SourceFile>,
        change: ByteTextChangeRange,
    ) -> Result<Self, IncrementalParseError> {
        let change_start = change.span.start;
        let old_end = change
            .span
            .end()
            .ok_or(IncrementalParseError::ChangeRangeOverflow)?;
        let new_end = change_start
            .checked_add(change.new_length)
            .ok_or(IncrementalParseError::ChangeRangeOverflow)?;
        let delta = i64::from(change.new_length) - i64::from(change.span.length);

        let mut indexed = HashMap::<u32, (usize, ReusableNode)>::with_capacity(
            source.arena.node_arrays().len().saturating_mul(2),
        );
        let mut stack = vec![(source.root, 0usize)];
        while let Some((node_id, depth)) = stack.pop() {
            let node = source.arena.node(node_id);
            let mut direct_nodes = Vec::new();
            let mut arrays = Vec::new();
            collect_node_data_ids(&node.data, &mut direct_nodes, &mut arrays);
            if let Some(js_doc) = node.js_doc {
                arrays.push(js_doc);
            }
            for array in arrays {
                index_list_elements(
                    &source,
                    array,
                    depth + 1,
                    change_start,
                    old_end,
                    new_end,
                    delta,
                    &mut indexed,
                    &mut stack,
                )?;
            }
            // Direct node fields are structural paths into a changed list
            // element. Array elements that do not intersect the edit stop
            // here: if the ordinary parser accepts one, it consumes the
            // complete subtree, so indexing all of its nested lists would be
            // pure large-file overhead. A conservative miss only reparses a
            // subtree; it cannot change the resulting syntax.
            for child in direct_nodes {
                stack.push((child, depth + 1));
            }
        }
        Ok(Self {
            data: Arc::new(SyntaxCursorData {
                source,
                nodes_by_position: indexed,
            }),
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn index_list_elements(
    source: &SourceFile,
    array_id: NodeArrayId,
    depth: usize,
    change_start: u32,
    old_end: u32,
    new_end: u32,
    delta: i64,
    indexed: &mut HashMap<u32, (usize, ReusableNode)>,
    pending: &mut Vec<(NodeId, usize)>,
) -> Result<(), IncrementalParseError> {
    for child in &source.arena.node_array(array_id).nodes {
        let node = source.arena.node(*child);
        let candidate = adjusted_candidate(
            *child,
            node.pos,
            node.end,
            change_start,
            old_end,
            new_end,
            delta,
        )?;
        match indexed.entry(candidate.adjusted_pos) {
            Entry::Vacant(entry) => {
                entry.insert((depth, candidate));
            }
            Entry::Occupied(mut entry) if depth < entry.get().0 => {
                entry.insert((depth, candidate));
            }
            Entry::Occupied(_) => {}
        }
        if candidate.intersects_change {
            pending.push((*child, depth));
        }
    }
    Ok(())
}

fn adjusted_candidate(
    old_node: NodeId,
    pos: u32,
    end: u32,
    change_start: u32,
    old_end: u32,
    new_end: u32,
    delta: i64,
) -> Result<ReusableNode, IncrementalParseError> {
    if pos > old_end {
        return Ok(ReusableNode {
            old_node,
            adjusted_pos: shift_position(pos, delta)?,
            adjusted_end: shift_position(end, delta)?,
            position_delta: delta,
            intersects_change: false,
        });
    }
    if end < change_start {
        return Ok(ReusableNode {
            old_node,
            adjusted_pos: pos,
            adjusted_end: end,
            position_delta: 0,
            intersects_change: false,
        });
    }

    let adjusted_pos = pos.min(new_end);
    let adjusted_end = if end >= old_end {
        shift_position(end, delta)?
    } else {
        end.min(new_end)
    };
    Ok(ReusableNode {
        old_node,
        adjusted_pos,
        adjusted_end,
        position_delta: 0,
        intersects_change: true,
    })
}

fn shift_position(position: u32, delta: i64) -> Result<u32, IncrementalParseError> {
    let shifted = i64::from(position)
        .checked_add(delta)
        .ok_or(IncrementalParseError::ChangeRangeOverflow)?;
    u32::try_from(shifted).map_err(|_| IncrementalParseError::ChangeRangeOverflow)
}

pub fn extend_to_affected_range(
    source: &SourceFile,
    change: ByteTextChangeRange,
) -> Result<ByteTextChangeRange, IncrementalParseError> {
    validate_old_change_boundaries(source, change)?;
    let old_end = change
        .span
        .end()
        .ok_or(IncrementalParseError::ChangeRangeOverflow)?;
    let mut start = change.span.start;
    // tsc's maxLookahead is one. The first iteration aligns with the edit;
    // the second backs up one additional token/node.
    for _ in 0..=1 {
        if start == 0 {
            break;
        }
        let nearest = nearest_node_start(source, start);
        start = previous_scalar_start(source.text(), nearest);
    }
    Ok(ByteTextChangeRange {
        span: ByteTextSpan::new(start, old_end - start),
        new_length: change
            .new_length
            .checked_add(change.span.start - start)
            .ok_or(IncrementalParseError::ChangeRangeOverflow)?,
    })
}

pub fn create_syntax_cursor(
    source: Arc<SourceFile>,
    change: ByteTextChangeRange,
) -> Result<SyntaxCursor, IncrementalParseError> {
    let affected = extend_to_affected_range(&source, change)?;
    SyntaxCursor::from_affected_range(source, affected)
}

fn nearest_node_start(source: &SourceFile, position: u32) -> u32 {
    let mut best = (0u32, 0usize);
    let mut stack = vec![(source.root, 0usize)];
    while let Some((id, depth)) = stack.pop() {
        let node = source.arena.node(id);
        if node.kind != SyntaxKind::EndOfFileToken && node.pos == node.end {
            continue;
        }
        if node.pos <= position && (node.pos > best.0 || node.pos == best.0 && depth > best.1) {
            best = (node.pos, depth);
        }
        for_each_child(&source.arena, node, |child| {
            stack.push((child, depth + 1));
            false
        });
    }
    best.0
}

fn previous_scalar_start(text: &str, position: u32) -> u32 {
    let position = position as usize;
    text[..position]
        .char_indices()
        .next_back()
        .map_or(0, |(start, _)| start as u32)
}

pub fn create_language_service_source_file(
    file_name: impl Into<String>,
    snapshot: Arc<TextSnapshot>,
    options: ParseOptions,
) -> Arc<SourceFile> {
    Arc::new(parser::parse_source_file_from_snapshot(
        file_name.into(),
        snapshot,
        options,
        None,
    ))
}

pub fn create_language_service_source_file_in_identity_domain(
    file_name: impl Into<String>,
    snapshot: Arc<TextSnapshot>,
    options: ParseOptions,
    domain: &IdentityDomain,
) -> Result<Arc<SourceFile>, IncrementalParseError> {
    let source = crate::parse_source_file_from_snapshot_in_identity_domain(
        file_name, snapshot, options, None, domain,
    )?;
    Ok(Arc::new(source))
}

pub fn update_language_service_source_file(
    source: Arc<SourceFile>,
    snapshot: Arc<TextSnapshot>,
    change: ByteTextChangeRange,
    options: ParseOptions,
    incremental_options: IncrementalParseOptions,
) -> Result<IncrementalParseResult, IncrementalParseError> {
    update_source_file_worker(source, snapshot, change, options, incremental_options, None)
}

pub fn update_language_service_source_file_in_identity_domain(
    source: Arc<SourceFile>,
    snapshot: Arc<TextSnapshot>,
    change: ByteTextChangeRange,
    options: ParseOptions,
    incremental_options: IncrementalParseOptions,
    domain: &IdentityDomain,
) -> Result<IncrementalParseResult, IncrementalParseError> {
    if !source.identity_owned_by(domain) {
        return Err(IncrementalParseError::IdentityDomainMismatch);
    }
    update_source_file_worker(
        source,
        snapshot,
        change,
        options,
        incremental_options,
        Some(domain),
    )
}

fn update_source_file_worker(
    source: Arc<SourceFile>,
    snapshot: Arc<TextSnapshot>,
    change: ByteTextChangeRange,
    options: ParseOptions,
    incremental_options: IncrementalParseOptions,
    domain: Option<&IdentityDomain>,
) -> Result<IncrementalParseResult, IncrementalParseError> {
    validate_change(&source, &snapshot, change)?;

    if change == ByteTextChangeRange::UNCHANGED && Arc::ptr_eq(source.snapshot(), &snapshot) {
        return Ok(IncrementalParseResult {
            source,
            stats: IncrementalParseStats::default(),
        });
    }

    let can_increment = has_statements(&source) && parse_options_match(&source, &options);
    let mut affected_change = None;
    let (mut parsed, mut stats) = if can_increment {
        let affected = extend_to_affected_range(&source, change)?;
        affected_change = Some(affected);
        let cursor = SyntaxCursor::from_affected_range(Arc::clone(&source), affected)?;
        let (parsed, mut stats) = parser::parse_source_file_from_snapshot_incrementally(
            source.file_name.clone(),
            snapshot,
            options,
            &cursor,
            incremental_options,
        );
        stats.incremental = true;
        (parsed, stats)
    } else {
        let parsed = parser::parse_source_file_from_snapshot(
            source.file_name.clone(),
            snapshot,
            options,
            None,
        );
        (
            parsed,
            IncrementalParseStats {
                full_parse_fallback: true,
                ..IncrementalParseStats::default()
            },
        )
    };

    if stats.incremental {
        // Speculative arrow/type parsing can consume an old list element and
        // then rewind. Only copied nodes that survived into the published
        // root graph count as reuse or contribute retained diagnostics.
        stats.retain_reachable_reuse(&parsed);
        parsed.comment_directives = merge_comment_directives(
            &source.comment_directives,
            &parsed.comment_directives,
            affected_change.expect("incremental parsing computed an affected range"),
        )?;
        let permanent = source.arena.node(source.root).flags
            & NodeFlags::PERMANENTLY_SET_INCREMENTAL_FLAGS.bits();
        parsed.arena.node_mut(parsed.root).flags |= permanent;
        parsed.js_doc_diagnostics =
            merge_jsdoc_diagnostics(&source, &parsed, &stats.reused_old_ranges)?;
    }

    let local_base = parsed.arena.node_base();
    if let Some(domain) = domain {
        parsed.relocate_into_identity_domain(domain)?;
        stats.relocate_new_ids(local_base, parsed.arena.node_base());
    }
    stats.finish(&parsed);
    Ok(IncrementalParseResult {
        source: Arc::new(parsed),
        stats,
    })
}

fn validate_old_change_boundaries(
    source: &SourceFile,
    change: ByteTextChangeRange,
) -> Result<(), IncrementalParseError> {
    let end = change
        .span
        .end()
        .ok_or(IncrementalParseError::ChangeRangeOverflow)?;
    let old_length = source.positions().byte_len();
    if end > old_length {
        return Err(IncrementalParseError::OldRangeOutOfBounds { end, old_length });
    }
    for position in [change.span.start, end] {
        if !source.text().is_char_boundary(position as usize) {
            return Err(IncrementalParseError::InvalidOldScalarBoundary { position });
        }
    }
    Ok(())
}

fn validate_change(
    source: &SourceFile,
    snapshot: &TextSnapshot,
    change: ByteTextChangeRange,
) -> Result<(), IncrementalParseError> {
    validate_old_change_boundaries(source, change)?;
    let old_end = change
        .span
        .end()
        .ok_or(IncrementalParseError::ChangeRangeOverflow)?;
    let new_end = change
        .span
        .start
        .checked_add(change.new_length)
        .ok_or(IncrementalParseError::ChangeRangeOverflow)?;
    let new_length = snapshot.positions().byte_len();
    if new_end > new_length {
        return Err(IncrementalParseError::NewRangeOutOfBounds {
            end: new_end,
            new_length,
        });
    }
    for position in [change.span.start, new_end] {
        if !snapshot.text().is_char_boundary(position as usize) {
            return Err(IncrementalParseError::InvalidNewScalarBoundary { position });
        }
    }
    let expected = source
        .positions()
        .byte_len()
        .checked_sub(change.span.length)
        .and_then(|length| length.checked_add(change.new_length))
        .ok_or(IncrementalParseError::ChangeRangeOverflow)?;
    if expected != new_length {
        return Err(IncrementalParseError::LengthMismatch {
            expected,
            actual: new_length,
        });
    }
    let start = change.span.start as usize;
    if source.text()[..start] != snapshot.text()[..start] {
        return Err(IncrementalParseError::PrefixMismatch);
    }
    if source.text()[old_end as usize..] != snapshot.text()[new_end as usize..] {
        return Err(IncrementalParseError::SuffixMismatch);
    }
    Ok(())
}

fn has_statements(source: &SourceFile) -> bool {
    let NodeData::SourceFile(SourceFileData {
        statements: Some(statements),
        ..
    }) = &source.arena.node(source.root).data
    else {
        return false;
    };
    !source.arena.node_array(*statements).nodes.is_empty()
}

fn parse_options_match(source: &SourceFile, options: &ParseOptions) -> bool {
    let flags = NodeFlags::from_bits(source.arena.node(source.root).flags);
    source.language_version == options.script_target
        && source.language_variant == options.language_variant
        && source.js_doc_parsing_mode == options.js_doc_parsing_mode
        && flags.contains(NodeFlags::JAVA_SCRIPT_FILE) == options.javascript_file
}

fn merge_comment_directives(
    old: &[CommentDirective],
    newly_scanned: &[CommentDirective],
    affected: ByteTextChangeRange,
) -> Result<Vec<CommentDirective>, IncrementalParseError> {
    let old_end = affected
        .span
        .end()
        .ok_or(IncrementalParseError::ChangeRangeOverflow)?;
    let delta = i64::from(affected.new_length) - i64::from(affected.span.length);
    let mut merged = Vec::with_capacity(old.len() + newly_scanned.len());
    let mut added_new = false;
    for directive in old {
        if directive.end < affected.span.start {
            merged.push(*directive);
        } else if directive.pos > old_end {
            if !added_new {
                merged.extend_from_slice(newly_scanned);
                added_new = true;
            }
            merged.push(CommentDirective {
                pos: shift_position(directive.pos, delta)?,
                end: shift_position(directive.end, delta)?,
                kind: directive.kind,
            });
        }
    }
    if !added_new {
        merged.extend_from_slice(newly_scanned);
    }
    Ok(merged)
}

fn merge_jsdoc_diagnostics(
    old_source: &SourceFile,
    new_source: &SourceFile,
    reused_ranges: &[(u32, u32, i64)],
) -> Result<Vec<Diagnostic>, IncrementalParseError> {
    let mut diagnostics = new_source.js_doc_diagnostics.clone();
    for diagnostic in &old_source.js_doc_diagnostics {
        let Some(start_utf16) = diagnostic.start else {
            continue;
        };
        let start_byte = old_source.positions().utf16_to_byte(start_utf16).ok_or(
            IncrementalParseError::InvalidOldScalarBoundary {
                position: start_utf16,
            },
        )?;
        let Some((_, _, delta)) = reused_ranges
            .iter()
            .find(|(start, end, _)| start_byte >= *start && start_byte <= *end)
        else {
            continue;
        };
        let mut reused = diagnostic.clone();
        relocate_diagnostic_location(
            &mut reused.start,
            &mut reused.length,
            old_source,
            new_source,
            *delta,
        )?;
        for related in &mut reused.related {
            if related.file_name.as_deref() == Some(old_source.file_name.as_str()) {
                relocate_diagnostic_location(
                    &mut related.start,
                    &mut related.length,
                    old_source,
                    new_source,
                    *delta,
                )?;
            }
        }
        diagnostics.push(reused);
    }
    sort_and_dedupe_diagnostics(&mut diagnostics);
    Ok(diagnostics)
}

fn relocate_diagnostic_location(
    start: &mut Option<u32>,
    length: &mut Option<u32>,
    old_source: &SourceFile,
    new_source: &SourceFile,
    delta: i64,
) -> Result<(), IncrementalParseError> {
    let Some(old_start_utf16) = *start else {
        return Ok(());
    };
    let old_end_utf16 = old_start_utf16
        .checked_add(length.unwrap_or(0))
        .ok_or(IncrementalParseError::ChangeRangeOverflow)?;
    let old_start = old_source
        .positions()
        .utf16_to_byte(old_start_utf16)
        .ok_or(IncrementalParseError::InvalidOldScalarBoundary {
            position: old_start_utf16,
        })?;
    let old_end = old_source.positions().utf16_to_byte(old_end_utf16).ok_or(
        IncrementalParseError::InvalidOldScalarBoundary {
            position: old_end_utf16,
        },
    )?;
    let new_start_byte = shift_position(old_start, delta)?;
    let new_end_byte = shift_position(old_end, delta)?;
    let new_start = new_source.positions().byte_to_utf16(new_start_byte).ok_or(
        IncrementalParseError::InvalidNewScalarBoundary {
            position: new_start_byte,
        },
    )?;
    let new_end = new_source.positions().byte_to_utf16(new_end_byte).ok_or(
        IncrementalParseError::InvalidNewScalarBoundary {
            position: new_end_byte,
        },
    )?;
    *start = Some(new_start);
    if length.is_some() {
        *length = Some(new_end - new_start);
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/incremental/tests.rs"]
mod tests;
