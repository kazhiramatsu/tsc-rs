use super::*;

#[derive(Default)]
pub(super) struct ContextRecoverySupport {
    pub(super) missing_slots: BTreeSet<NodeId>,
    pub(super) assertion_reports: BTreeSet<usize>,
    pub(super) assertion_missing: BTreeSet<usize>,
    pub(super) leading_binding_actions: BTreeSet<usize>,
    pub(super) leading_binding_reports: BTreeSet<usize>,
    pub(super) binding_delimiter_reports: BTreeSet<usize>,
    pub(super) binding_name_reports: BTreeSet<usize>,
    pub(super) binding_name_missing: BTreeSet<usize>,
}

impl ParseRecovery {
    // tsrs-native: commit only complete, disjoint composite recovery units.
    // Unmatched actions continue through the existing context gap checks.
    fn leading_binding_recovery_support(
        &self,
        source: &SourceFile,
        parents: &BTreeMap<NodeId, Option<NodeId>>,
    ) -> Option<ContextRecoverySupport> {
        let mut support = ContextRecoverySupport::default();
        let mut claimed_events = BTreeSet::new();
        let mut claimed_statements = BTreeSet::new();
        for (action_index, action) in self.actions.iter().copied().enumerate() {
            let Some((statement, name, events)) =
                self.leading_binding_recovery_unit(source, parents, action_index, action)
            else {
                continue;
            };
            if !claimed_statements.insert(statement) || !support.missing_slots.insert(name) {
                return None;
            }
            for index in events {
                if !claimed_events.insert(index) {
                    return None;
                }
            }
            let [scanner, suppressed_skip, delimiter, colon, missing] = events;
            support.leading_binding_actions.insert(action_index);
            support
                .leading_binding_reports
                .extend([scanner, suppressed_skip]);
            support.binding_delimiter_reports.insert(delimiter);
            support.binding_name_reports.insert(colon);
            support.binding_name_missing.insert(missing);
        }
        Some(support)
    }

    // tsrs-native: match one leading skip, missing delimiter and binding name
    // without committing a partial claim or widening any predecessor profile.
    fn leading_binding_recovery_unit(
        &self,
        source: &SourceFile,
        parents: &BTreeMap<NodeId, Option<NodeId>>,
        action_index: usize,
        action: ParseRecoveryAction,
    ) -> Option<(NodeId, NodeId, [usize; 5])> {
        if source.is_declaration_file {
            return None;
        }
        let ParseRecoveryAction::TokenSkipped {
            token: SyntaxKind::Unknown,
            start,
            length: 1,
            statement_start,
            site: ParseTokenSkipSite::ListAbort,
        } = action
        else {
            return None;
        };
        let start_byte = source.positions().utf16_to_byte(start)?;
        let end_byte = source.positions().utf16_to_byte(start.checked_add(1)?)?;
        let scanner = self.unique_leading_binding_event(|event| {
            event.kind
                == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::ScannerToken(
                    SyntaxKind::Unknown,
                ))
                && event.start == start
                // tsgo's Invalid character spans the backslash.
                && event.length == 1
                && event.missing_node.is_none()
                && event
                    .diagnostic_index
                    .and_then(|index| source.parse_diagnostics.get(index))
                    .is_some_and(|diagnostic| diagnostic.code() == 1127)
        })?;
        let full_start = self.events[scanner].full_start;
        let suppressed_skip = self.unique_leading_binding_event(|event| {
            event.kind == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                && event.start == start
                && event.length == 1
                && event.full_start == full_start
                && event.diagnostic_index.is_none()
                && event.missing_node.is_none()
                && Self::is_current_token_report(source, event)
        })?;
        let full_start_byte = source.positions().utf16_to_byte(full_start)?;
        let candidates = parents
            .iter()
            .filter_map(|(&id, &parent)| {
                let list = source.arena.node(id);
                let NodeData::VariableDeclarationList(data) = &list.data else {
                    return None;
                };
                if NodeFlags::from_bits(list.flags).contains(NodeFlags::USING) {
                    return None;
                }
                let statement = parent?;
                let record = source.arena.node(statement);
                let NodeData::VariableStatement(statement_data) = &record.data else {
                    return None;
                };
                if NodeFlags::from_bits(record.flags).contains(NodeFlags::AMBIENT)
                    || statement_data.declaration_list != Some(id)
                    || source.positions().byte_to_utf16(record.pos) != Some(statement_start)
                    || statement_data.modifiers.is_some_and(|array| {
                        source.arena.node_array(array).nodes.iter().any(|modifier| {
                            source.arena.node(*modifier).kind == SyntaxKind::DeclareKeyword
                        })
                    })
                {
                    return None;
                }
                let array = source.arena.node_array(data.declarations?);
                (array.nodes.len() == 2
                    && array.pos == full_start_byte
                    && source.arena.node(array.nodes[0]).pos == end_byte
                    && start_byte >= array.pos
                    && end_byte <= array.end)
                    .then(|| (id, statement, array.nodes[0], array.nodes[1]))
            })
            .collect::<Vec<_>>();
        let [(list_id, statement_id, first_id, second_id)] = candidates.as_slice() else {
            return None;
        };
        let mut ancestors = BTreeSet::new();
        let mut current = Some(*list_id);
        while let Some(id) = current {
            ancestors.insert(id);
            current = parents[&id];
        }
        if parents.keys().any(|id| {
            let node = source.arena.node(*id);
            node.pos <= start_byte && end_byte <= node.end && !ancestors.contains(id)
        }) {
            return None;
        }
        let list = source.arena.node(*list_id);
        let list_start = source.positions().byte_to_utf16(list.pos)?;
        let list_end = source.positions().byte_to_utf16(list.end)?;
        if self.actions.iter().enumerate().any(|(index, action)| {
            index != action_index
                && matches!(action, ParseRecoveryAction::TokenSkipped { start, length, .. }
                if *start < list_end && start.saturating_add(*length) > list_start)
        }) {
            return None;
        }
        let first = source.arena.node(*first_id);
        let second = source.arena.node(*second_id);
        let (NodeData::VariableDeclaration(first_data), NodeData::VariableDeclaration(second_data)) =
            (&first.data, &second.data)
        else {
            return None;
        };
        let first_name = source.arena.node(first_data.name?);
        if first_name.kind != SyntaxKind::Identifier
            || first_name.pos != first.pos
            || first_name.pos >= first_name.end
            || first_name.end != first.end
            || first.end != second.pos
            || first_data.initializer.is_some()
            || first_data.r#type.is_some()
            || first_data.exclamation_token.is_some()
            || second_data.r#type.is_some()
            || second_data.exclamation_token.is_some()
            || second_data.initializer.is_none_or(|initializer| {
                source.arena.node(initializer).kind != SyntaxKind::NumericLiteral
            })
        {
            return None;
        }
        let pattern = source.arena.node(second_data.name?);
        let NodeData::ObjectBindingPattern(pattern_data) = &pattern.data else {
            return None;
        };
        if pattern.pos != second.pos {
            return None;
        }
        let elements = source.arena.node_array(pattern_data.elements?).nodes;
        let [element_id] = elements else {
            return None;
        };
        let element = source.arena.node(*element_id);
        let NodeData::BindingElement(binding) = &element.data else {
            return None;
        };
        if binding.dot_dot_dot_token.is_some() || binding.initializer.is_some() {
            return None;
        }
        let name_id = binding.name?;
        let name = source.arena.node(name_id);
        let property = source.arena.node(binding.property_name?);
        if name.kind != SyntaxKind::Identifier
            || name.pos != name.end
            || name.end != element.end
            || property.kind != SyntaxKind::NumericLiteral
            || property.pos >= property.end
            || property.end != name.pos
        {
            return None;
        }
        let delimiter = self.unique_leading_binding_event(|event| {
            event.kind == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                && event.missing_node.is_none()
                && Self::is_current_token_report(source, event)
                && source.positions().utf16_to_byte(event.full_start) == Some(first.end)
                && event
                    .diagnostic_index
                    .and_then(|index| source.parse_diagnostics.get(index))
                    .is_some_and(|diagnostic| {
                        diagnostic.code() == 1005
                            && diagnostic.message_text().as_str() == Some("',' expected.")
                    })
                && source
                    .positions()
                    .utf16_to_byte(event.start)
                    .is_some_and(|start| {
                        crate::scanner::skip_trivia(source.text(), pattern.pos as usize)
                            == start as usize
                            && source.text().as_bytes().get(start as usize) == Some(&b'{')
                    })
                && event.length == 1
        })?;
        let position = source.positions().byte_to_utf16(name.pos)?;
        let colon = self.unique_leading_binding_event(|event| {
            event.kind == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                && event.missing_node.is_none()
                && Self::is_current_token_report(source, event)
                && event.full_start == position
                && event.length == 1
                && event
                    .start
                    .checked_add(event.length)
                    .and_then(|end| source.positions().utf16_to_byte(end))
                    == Some(pattern.end)
                && source
                    .positions()
                    .utf16_to_byte(event.start)
                    .is_some_and(|start| {
                        source.text().as_bytes().get(start as usize) == Some(&b'}')
                    })
                && event
                    .diagnostic_index
                    .and_then(|index| source.parse_diagnostics.get(index))
                    .is_some_and(|diagnostic| {
                        diagnostic.code() == 1005
                            && diagnostic.message_text().as_str() == Some("':' expected.")
                    })
        })?;
        let colon_event = &self.events[colon];
        let missing = self.unique_leading_binding_event(|event| {
            event.kind == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                && event.diagnostic_index.is_none()
                && event.start == colon_event.start
                && event.length == colon_event.length
                && event.full_start == position
                && event.missing_node
                    == Some(MissingNodeRecovery {
                        kind: SyntaxKind::Identifier,
                        position,
                    })
        })?;
        Some((
            *statement_id,
            name_id,
            [scanner, suppressed_skip, delimiter, colon, missing],
        ))
    }

    // tsrs-native: each composite recovery fact has a unique reporting attempt.
    fn unique_leading_binding_event(
        &self,
        predicate: impl Fn(&ParseRecoveryEvent) -> bool,
    ) -> Option<usize> {
        let mut indices = self
            .events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| predicate(event).then_some(index));
        let index = indices.next()?;
        indices.next().is_none().then_some(index)
    }

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
        let mut support = self.leading_binding_recovery_support(source, &parents)?;
        let mut spans = BTreeSet::new();
        let mut lists = Vec::new();
        for (action_index, action) in self.actions.iter().enumerate() {
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
            if support.leading_binding_actions.contains(&action_index) {
                continue;
            }
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
        let class_bodies = self.class_member_body_gap_actions(source, &parents, &lists);
        let empty_variables = self.empty_variable_list_gap_actions(source, &parents, &lists);
        if !heritage.is_disjoint(&class_bodies)
            || !heritage.is_disjoint(&empty_variables)
            || !class_bodies.is_disjoint(&empty_variables)
        {
            return None;
        }
        let remaining: Vec<_> = lists
            .into_iter()
            .enumerate()
            .filter_map(|(index, action)| {
                (!heritage.contains(&index)
                    && !class_bodies.contains(&index)
                    && !empty_variables.contains(&index))
                .then_some(action)
            })
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
                || data
                    .operator_token
                    .is_none_or(|token| source.arena.node(token).kind != SyntaxKind::CommaToken)
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

    // tsrs-native: an empty top-level variable list may own its skipped '='
    // only when the following numeric statement survives independently.
    // Its reports remain subject to the existing retained-syntax checks.
    fn empty_variable_list_gap_actions(
        &self,
        source: &SourceFile,
        parents: &BTreeMap<NodeId, Option<NodeId>>,
        lists: &[ParseRecoveryAction],
    ) -> BTreeSet<usize> {
        let mut claimed = BTreeSet::new();
        if source.is_declaration_file {
            return claimed;
        }
        let Some(statements) = Self::statement_array(&source.arena.node(source.root).data) else {
            return claimed;
        };
        let statements = &source.arena.node_array(statements).nodes;
        for (index, action) in lists.iter().enumerate() {
            let ParseRecoveryAction::TokenSkipped {
                token: SyntaxKind::EqualsToken,
                start,
                length: 1,
                site: ParseTokenSkipSite::ListAbort,
                ..
            } = *action
            else {
                continue;
            };
            let (Some(start_byte), Some(end_byte)) = (
                source.positions().utf16_to_byte(start),
                start
                    .checked_add(1)
                    .and_then(|end| source.positions().utf16_to_byte(end)),
            ) else {
                continue;
            };
            if source.text().get(start_byte as usize..end_byte as usize) != Some("=") {
                continue;
            }
            let Some(event) = self.unique_skip_report(source, start, 1) else {
                continue;
            };
            let Some(boundary) = source.positions().utf16_to_byte(event.full_start) else {
                continue;
            };
            if event.missing_node.is_some() {
                continue;
            }
            let mut owners = Vec::new();
            for pair in statements.windows(2) {
                let statement_id = pair[0];
                let statement = source.arena.node(statement_id);
                let NodeData::VariableStatement(data) = &statement.data else {
                    continue;
                };
                if data.modifiers.is_some()
                    || NodeFlags::from_bits(statement.flags).contains(NodeFlags::AMBIENT)
                    || parents.get(&statement_id).copied().flatten() != Some(source.root)
                {
                    continue;
                }
                let Some(list_id) = data.declaration_list else {
                    continue;
                };
                let list = source.arena.node(list_id);
                let NodeData::VariableDeclarationList(data) = &list.data else {
                    continue;
                };
                let declaration_kind = list.flags & NodeFlags::BLOCK_SCOPED.bits();
                if !matches!(declaration_kind, 0..=2)
                    || NodeFlags::from_bits(list.flags).contains(NodeFlags::AMBIENT)
                    || parents.get(&list_id).copied().flatten() != Some(statement_id)
                {
                    continue;
                }
                let Some(array) = data.declarations else {
                    continue;
                };
                let array = source.arena.node_array(array);
                if !array.nodes.is_empty()
                    || array.pos != boundary
                    || start_byte < array.pos
                    || array.end != end_byte
                    || list.end != array.end
                    || statement.end != array.end
                {
                    continue;
                }
                let next = source.arena.node(pair[1]);
                let NodeData::ExpressionStatement(data) = &next.data else {
                    continue;
                };
                let Some(expression) = data.expression else {
                    continue;
                };
                let expression = source.arena.node(expression);
                let Some(semicolon) = next.end.checked_sub(1) else {
                    continue;
                };
                if next.pos != statement.end
                    || expression.pos != next.pos
                    || expression.kind != SyntaxKind::NumericLiteral
                    || crate::scanner::skip_trivia(source.text(), expression.end as usize)
                        != semicolon as usize
                    || source.text().as_bytes().get(semicolon as usize) != Some(&b';')
                {
                    continue;
                }
                if parents.keys().all(|other| {
                    let node = source.arena.node(*other);
                    !(node.pos <= start_byte && end_byte <= node.end)
                        || [source.root, statement_id, list_id].contains(other)
                }) {
                    owners.push(list_id);
                }
            }
            if owners.len() == 1 {
                claimed.insert(index);
            }
        }
        claimed
    }

    // A class member's missing parsed body can leave one skipped arrow in
    // the member-array gap. This is not general class-list recovery and is
    // deliberately separate from the predecessor statement-gap profile.
    fn class_member_body_gap_actions(
        &self,
        source: &SourceFile,
        parents: &BTreeMap<NodeId, Option<NodeId>>,
        lists: &[ParseRecoveryAction],
    ) -> BTreeSet<usize> {
        let mut claimed = BTreeSet::new();
        for (index, action) in lists.iter().enumerate() {
            let ParseRecoveryAction::TokenSkipped {
                token: SyntaxKind::EqualsGreaterThanToken,
                start,
                length,
                site: ParseTokenSkipSite::ListAbort,
                ..
            } = *action
            else {
                continue;
            };
            let Some(end) = start.checked_add(length) else {
                continue;
            };
            let (Some(start_byte), Some(end_byte)) = (
                source.positions().utf16_to_byte(start),
                source.positions().utf16_to_byte(end),
            ) else {
                continue;
            };
            if source.text().get(start_byte as usize..end_byte as usize) != Some("=>") {
                continue;
            }
            let Some(event) = self.unique_skip_report(source, start, length) else {
                continue;
            };
            if event.missing_node.is_some() {
                continue;
            }
            let Some(boundary) = source.positions().utf16_to_byte(event.full_start) else {
                continue;
            };
            let mut owners = Vec::new();
            for &id in parents.keys() {
                let members = match &source.arena.node(id).data {
                    NodeData::ClassDeclaration(data) => data.members,
                    NodeData::ClassExpression(data) => data.members,
                    _ => None,
                };
                let Some(members) = members else {
                    continue;
                };
                let members = source.arena.node_array(members);
                if members.pos > start_byte
                    || members.end < end_byte
                    || members.nodes.iter().any(|member| {
                        let member = source.arena.node(*member);
                        member.pos < end_byte && start_byte < member.end
                    })
                {
                    continue;
                }
                let preceding = members
                    .nodes
                    .iter()
                    .rev()
                    .map(|member| source.arena.node(*member))
                    .find(|member| member.end <= start_byte);
                let Some(preceding) = preceding else {
                    continue;
                };
                let body = match &preceding.data {
                    NodeData::MethodDeclaration(data) => data.body,
                    NodeData::Constructor(data) => data.body,
                    NodeData::GetAccessor(data) => data.body,
                    NodeData::SetAccessor(data) => data.body,
                    _ => None,
                };
                if preceding.end != boundary
                    || body.is_none_or(|body| {
                        let body = source.arena.node(body);
                        body.kind != SyntaxKind::Block
                            || body.pos != boundary
                            || body.end != boundary
                    })
                {
                    continue;
                }
                let following = members
                    .nodes
                    .iter()
                    .map(|member| source.arena.node(*member))
                    .find(|member| member.pos >= end_byte);
                if following.map_or(members.end, |member| member.pos) != end_byte {
                    continue;
                }
                let mut ancestors = BTreeSet::new();
                let mut current = Some(id);
                while let Some(ancestor) = current {
                    ancestors.insert(ancestor);
                    current = parents[&ancestor];
                }
                if parents.keys().all(|other| {
                    let node = source.arena.node(*other);
                    !(node.pos <= start_byte && end_byte <= node.end) || ancestors.contains(other)
                }) {
                    owners.push(id);
                }
            }
            if owners.len() == 1 {
                claimed.insert(index);
            }
        }
        claimed
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
