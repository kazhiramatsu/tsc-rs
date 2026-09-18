use super::*;

#[derive(Default)]
pub(super) struct ContextRecoverySupport {
    pub(super) missing_slots: BTreeSet<NodeId>,
    pub(super) assertion_reports: BTreeSet<usize>,
    pub(super) assertion_missing: BTreeSet<usize>,
}

impl ParseRecovery {
    /// Await-context recovery is admitted only after independently accounting
    /// for every context run, consumed token and newly supported expression slot.
    pub(super) fn context_recovery_support(
        &self,
        source: &SourceFile,
    ) -> Option<ContextRecoverySupport> {
        let parents = Self::reachable_parents(source)?;
        if !self.supports_reparse_runs(source) {
            return None;
        }
        let mut support = ContextRecoverySupport::default();
        let mut spans = BTreeSet::new();
        let mut lists = Vec::new();
        for action in &self.actions {
            let ParseRecoveryAction::TokenSkipped {
                token,
                start,
                length,
                site,
                ..
            } = *action
            else {
                continue;
            };
            let end = start.checked_add(length).filter(|end| *end > start)?;
            source.positions().utf16_to_byte(start)?;
            source.positions().utf16_to_byte(end)?;
            if spans.iter().any(|(a, b)| start < *b && *a < end) {
                return None;
            }
            spans.insert((start, end));
            match site {
                ParseTokenSkipSite::DecoratorAwait if token == SyntaxKind::AwaitKeyword => {
                    let event = self.unique_skip_report(source, start, length)?;
                    let missing = event.missing_node?;
                    if missing.kind != SyntaxKind::Identifier
                        || missing.position != event.full_start
                    {
                        return None;
                    }
                    let position = source.positions().utf16_to_byte(missing.position)?;
                    let start_byte = source.positions().utf16_to_byte(start)?;
                    let end_byte = source.positions().utf16_to_byte(end)?;
                    let mut heads = Vec::new();
                    for id in parents.keys() {
                        let node = source.arena.node(*id);
                        let NodeData::Decorator(data) = &node.data else {
                            continue;
                        };
                        if node.pos > start_byte || node.end < end_byte {
                            continue;
                        }
                        let mut head = data.expression?;
                        loop {
                            let child = match &source.arena.node(head).data {
                                NodeData::CallExpression(data) => data.expression,
                                NodeData::PropertyAccessExpression(data) => data.expression,
                                NodeData::ElementAccessExpression(data) => data.expression,
                                NodeData::NonNullExpression(data) => data.expression,
                                NodeData::TaggedTemplateExpression(data) => data.tag,
                                _ => None,
                            };
                            let Some(child) = child else {
                                break;
                            };
                            head = child;
                        }
                        let node = source.arena.node(head);
                        if node.kind == SyntaxKind::Identifier
                            && node.pos == position
                            && node.end == position
                        {
                            heads.push(head);
                        }
                    }
                    if heads.len() != 1 || !support.missing_slots.insert(heads[0]) {
                        return None;
                    }
                }
                ParseTokenSkipSite::ListAbort => lists.push(*action),
                _ => return None,
            }
        }
        let heritage = self.heritage_gap_actions(source, &parents, &lists)?;
        let remaining: Vec<_> = lists
            .into_iter()
            .enumerate()
            .filter_map(|(index, action)| (!heritage.contains(&index)).then_some(action))
            .collect();
        if !remaining.is_empty() && !self.supports_array_gaps(source, true, &remaining) {
            return None;
        }
        for (&id, &parent) in &parents {
            let node = source.arena.node(id);
            if node.kind != SyntaxKind::Identifier || node.pos != node.end {
                continue;
            }
            let Some(parent) = parent else {
                continue;
            };
            let binary = source.arena.node(parent);
            let NodeData::BinaryExpression(data) = &binary.data else {
                continue;
            };
            if data.right != Some(id)
                || binary.end != node.end
                || !data
                    .operator_token
                    .is_some_and(|token| source.arena.node(token).kind == SyntaxKind::CommaToken)
            {
                continue;
            }
            let Some(container) = parents[&parent] else {
                continue;
            };
            let container = source.arena.node(container);
            let NodeData::ParenthesizedExpression(data) = &container.data else {
                continue;
            };
            if data.expression != Some(parent) || container.end <= binary.end {
                continue;
            }
            let position = source.positions().byte_to_utf16(node.pos)?;
            let reports = self
                .events
                .iter()
                .filter(|event| {
                    event.diagnostic_index.is_some()
                        && event.missing_node
                            == Some(MissingNodeRecovery {
                                kind: SyntaxKind::Identifier,
                                position,
                            })
                        && event.full_start == position
                        && Self::is_current_token_report(source, event)
                })
                .count();
            if reports == 1 {
                support.missing_slots.insert(id);
            }
        }
        // A failed assertion closer and its suppressed missing operand are
        // one boundary proof. No other suppressed missing event is admitted.
        for (index, event) in self.events.iter().enumerate() {
            if event.kind != ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                || event.diagnostic_index.is_none()
                || event.missing_node.is_some()
                || !Self::is_current_token_report(source, event)
            {
                continue;
            }
            let boundary = source.positions().utf16_to_byte(event.full_start)?;
            let owners = parents
                .keys()
                .filter(|id| {
                    let node = source.arena.node(**id);
                    let NodeData::TypeAssertionExpression(data) = &node.data else {
                        return false;
                    };
                    node.end == boundary
                        && data
                            .r#type
                            .is_some_and(|id| source.arena.node(id).end == boundary)
                        && data.expression.is_some_and(|id| {
                            let operand = source.arena.node(id);
                            operand.kind == SyntaxKind::Identifier
                                && operand.pos == boundary
                                && operand.end == boundary
                        })
                })
                .count();
            if owners != 1 {
                continue;
            }
            let suppressed: Vec<_> = self
                .events
                .iter()
                .enumerate()
                .filter_map(|(other_index, other)| {
                    (other.kind == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                        && other.diagnostic_index.is_none()
                        && other.start == event.start
                        && other.length == event.length
                        && other.full_start == event.full_start
                        && other.missing_node
                            == Some(MissingNodeRecovery {
                                kind: SyntaxKind::Identifier,
                                position: event.full_start,
                            }))
                    .then_some(other_index)
                })
                .collect();
            if suppressed.len() == 1 {
                support.assertion_reports.insert(index);
                if !support.assertion_missing.insert(suppressed[0]) {
                    return None;
                }
            }
        }
        Some(support)
    }

    fn unique_skip_report(
        &self,
        source: &SourceFile,
        start: u32,
        length: u32,
    ) -> Option<&ParseRecoveryEvent> {
        let mut events = self.events.iter().filter(|event| {
            event.kind == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                && event.diagnostic_index.is_some()
                && event.start == start
                && event.length == length
                && Self::is_current_token_report(source, event)
        });
        let event = events.next()?;
        events.next().is_none().then_some(event)
    }

    fn supports_reparse_runs(&self, source: &SourceFile) -> bool {
        let mut ranges = BTreeSet::new();
        let mut covered = BTreeSet::new();
        let Some(statements) = Self::statement_array(&source.arena.node(source.root).data) else {
            return false;
        };
        let statements = &source.arena.node_array(statements).nodes;
        // An await reparse can overshoot and retain overlapping old tail
        // statements. Duplicated output needs its own emit proof.
        if statements
            .windows(2)
            .any(|pair| source.arena.node(pair[0]).end > source.arena.node(pair[1]).pos)
        {
            return false;
        }
        for action in &self.actions {
            let ParseRecoveryAction::Reparsed { start, end } = *action else {
                continue;
            };
            if source.is_declaration_file
                || source.external_module_indicator.is_none()
                || start >= end
                || ranges.iter().any(|(a, b)| start < *b && *a < end)
            {
                return false;
            }
            ranges.insert((start, end));
            let (Some(start), Some(end)) = (
                source.positions().utf16_to_byte(start),
                source.positions().utf16_to_byte(end),
            ) else {
                return false;
            };
            let starts: Vec<_> = statements
                .iter()
                .enumerate()
                .filter_map(|(i, id)| (source.arena.node(*id).pos == start).then_some(i))
                .collect();
            let ends: Vec<_> = statements
                .iter()
                .enumerate()
                .filter_map(|(i, id)| (source.arena.node(*id).end == end).then_some(i))
                .collect();
            if starts.len() != 1 || ends.len() != 1 || starts[0] > ends[0] {
                return false;
            }
            let (first, last) = (starts[0], ends[0]);
            covered.extend(first..=last);
            let in_await = |id: &NodeId| {
                NodeFlags::from_bits(source.arena.node(*id).flags)
                    .contains(NodeFlags::AWAIT_CONTEXT)
            };
            if !statements[first..=last].iter().all(in_await)
                || first
                    .checked_sub(1)
                    .is_some_and(|i| in_await(&statements[i]))
                || statements.get(last + 1).is_some_and(in_await)
            {
                return false;
            }
        }
        statements.iter().enumerate().all(|(index, id)| {
            NodeFlags::from_bits(source.arena.node(*id).flags).contains(NodeFlags::AWAIT_CONTEXT)
                == covered.contains(&index)
        })
    }

    /// Only a nonempty HeritageClause.types array may own these leading and
    /// trailing gaps. Retained children and recorded skips must tile it;
    /// unrecorded punctuation never advances the cursor.
    fn heritage_gap_actions(
        &self,
        source: &SourceFile,
        parents: &BTreeMap<NodeId, Option<NodeId>>,
        lists: &[ParseRecoveryAction],
    ) -> Option<BTreeSet<usize>> {
        let mut owners: BTreeMap<usize, Vec<NodeId>> = BTreeMap::new();
        for &id in parents.keys() {
            let NodeData::HeritageClause(data) = &source.arena.node(id).data else {
                continue;
            };
            let Some(array) = data.types else {
                continue;
            };
            let array = source.arena.node_array(array);
            // This proof owns one retained heritage expression and its
            // surrounding recovery gap. Comma-separated heritage needs a
            // separate separator/element proof; a tiled span alone is not it.
            if array.nodes.len() != 1 {
                continue;
            }
            let mut ancestors = BTreeSet::new();
            let mut current = Some(id);
            while let Some(id) = current {
                ancestors.insert(id);
                current = parents[&id];
            }
            let mut segments: Vec<_> = array
                .nodes
                .iter()
                .map(|child| {
                    let node = source.arena.node(*child);
                    (node.pos, node.end, None)
                })
                .collect();
            let mut indices = Vec::new();
            for (index, action) in lists.iter().enumerate() {
                let ParseRecoveryAction::TokenSkipped { start, length, .. } = *action else {
                    return None;
                };
                let end = start.checked_add(length)?;
                let (start_byte, end_byte) = (
                    source.positions().utf16_to_byte(start)?,
                    source.positions().utf16_to_byte(end)?,
                );
                if start_byte < array.pos
                    || end_byte > array.end
                    || array.nodes.iter().any(|child| {
                        let n = source.arena.node(*child);
                        n.pos < end_byte && start_byte < n.end
                    })
                {
                    continue;
                }
                if !parents.keys().all(|other| {
                    let n = source.arena.node(*other);
                    !(n.pos <= start_byte && end_byte <= n.end) || ancestors.contains(other)
                }) {
                    continue;
                }
                let event = self.unique_skip_report(source, start, length)?;
                if event.missing_node.is_some() {
                    continue;
                }
                let full_start = source.positions().utf16_to_byte(event.full_start)?;
                segments.push((full_start, end_byte, Some(index)));
                indices.push(index);
            }
            if indices.is_empty() {
                continue;
            }
            segments.sort_unstable();
            let mut cursor = array.pos;
            let mut complete = true;
            for (start, end, _) in segments {
                if start != cursor || end <= start {
                    complete = false;
                    break;
                }
                cursor = end;
            }
            if complete && cursor == array.end {
                for index in indices {
                    owners.entry(index).or_default().push(id);
                }
            }
        }
        if owners.values().any(|owners| owners.len() != 1) {
            return None;
        }
        Some(owners.into_keys().collect())
    }
}
