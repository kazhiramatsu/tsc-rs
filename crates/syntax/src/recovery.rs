mod context;

use std::collections::{BTreeMap, BTreeSet};

use crate::{for_each_child, NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::NodeFlags;

/// The producer of a retained syntactic diagnostic. Scanner trivia is kept
/// separate from the completed token even when both drain in the same scan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseDiagnosticOrigin {
    ScannerToken(SyntaxKind),
    ScannerTrivia(SyntaxKind),
    Parser,
    ReferenceDirective,
}

impl ParseDiagnosticOrigin {
    fn is_literal(self) -> bool {
        matches!(
            self,
            Self::ScannerToken(
                SyntaxKind::StringLiteral
                    | SyntaxKind::NoSubstitutionTemplateLiteral
                    | SyntaxKind::TemplateHead
                    | SyntaxKind::TemplateMiddle
                    | SyntaxKind::TemplateTail
            )
        )
    }
}

/// A reporting attempt remains an event when same-start deduplication drops
/// its diagnostic. Missing nodes with a message use that reporting event;
/// missing nodes without a message have their own event instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseRecoveryKind {
    Diagnostic(ParseDiagnosticOrigin),
    SilentMissingNode(SyntaxKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MissingNodeRecovery {
    pub kind: SyntaxKind,
    /// Full start at creation, before trivia, in UTF-16 units. A recovering
    /// declaration can later acquire a range that includes its modifiers.
    /// A diagnostic on the same event may instead start at the current token.
    pub position: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseTokenSkipSite {
    ListAbort,
    ListNoProgress,
    DelimitedSemicolon,
    DelimitedNoProgress,
    BlockTrailingEquals,
    ParameterModifier,
    TypePredicateArrow,
    TypeAnnotationCall,
    TopLevelAwaitReparse,
    DecoratorAwait,
}

/// Recovery operations that need not produce a diagnostic event. Positions
/// use UTF-16 units and remain independent of arena allocation identities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseRecoveryAction {
    TokenSkipped {
        token: SyntaxKind,
        start: u32,
        length: u32,
        statement_start: u32,
        site: ParseTokenSkipSite,
    },
    Reparsed {
        start: u32,
        end: u32,
    },
}

impl ParseRecoveryAction {
    pub(crate) fn owner_start(self) -> u32 {
        match self {
            Self::TokenSkipped {
                statement_start, ..
            } => statement_start,
            Self::Reparsed { start, .. } => start,
        }
    }

    pub(crate) fn intersects(self, node_start: u32, node_end: u32) -> bool {
        let (start, end) = match self {
            Self::TokenSkipped {
                start,
                length,
                statement_start,
                ..
            } => (start.min(statement_start), start.saturating_add(length)),
            // The top-level reparse pass runs again after incremental parsing.
            // Its range is recreated even when valid nodes are reused within
            // that pass; it is not itself lost recovery inside a reused node.
            Self::Reparsed { .. } => return false,
        };
        start <= node_end && end >= node_start
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseRecoveryEvent {
    pub kind: ParseRecoveryKind,
    /// Source positions in UTF-16 units, as in syntactic diagnostics.
    pub start: u32,
    pub length: u32,
    /// Scanner full start at event creation, in UTF-16 units. This remains
    /// independent of the diagnostic span, which can skip comment trivia.
    /// Only current-token parser reports can use it as a retained-node end;
    /// explicit-range reports may refer to an earlier node instead.
    pub full_start: u32,
    pub diagnostic_index: Option<usize>,
    /// Message-bearing missing-node provenance on this fresh reporting event.
    /// Silent missing nodes are already represented by kind/start above.
    pub missing_node: Option<MissingNodeRecovery>,
    // Reparse ownership for events with no diagnostic index: scanner token
    // start, or the enclosing source element for structural recovery. The
    // error position itself may point at the next statement or end of file.
    pub(crate) reparse_start: u32,
}

/// Parser-owned committed recovery facts. JSDoc diagnostics have a separate
/// destination and do not contribute to this syntactic diagnostic record.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParseRecovery {
    pub(crate) diagnostic_origins: Vec<ParseDiagnosticOrigin>,
    pub(crate) events: Vec<ParseRecoveryEvent>,
    pub(crate) actions: Vec<ParseRecoveryAction>,
}

impl ParseRecovery {
    pub fn diagnostic_origins(&self) -> &[ParseDiagnosticOrigin] {
        &self.diagnostic_origins
    }

    pub fn events(&self) -> &[ParseRecoveryEvent] {
        &self.events
    }

    pub fn actions(&self) -> &[ParseRecoveryAction] {
        &self.actions
    }

    pub(crate) fn is_literal_only(&self, diagnostic_count: usize) -> bool {
        self.diagnostic_origins.len() == diagnostic_count
            && self
                .diagnostic_origins
                .iter()
                .all(|origin| origin.is_literal())
            && self.events.iter().all(|event| match event.kind {
                ParseRecoveryKind::Diagnostic(origin) => origin.is_literal(),
                ParseRecoveryKind::SilentMissingNode(_) => false,
            })
    }

    pub(crate) fn is_literal_or_missing_await(&self, source: &SourceFile) -> bool {
        self.supports_missing_nodes(source, false, false, false, false)
    }

    pub(crate) fn is_missing_node_emit_recovery(&self, source: &SourceFile) -> bool {
        self.supports_missing_nodes(source, true, false, false, false)
    }

    pub(crate) fn is_parameter_gap_emit_recovery(&self, source: &SourceFile) -> bool {
        self.supports_missing_nodes(source, true, true, false, false)
    }

    pub(crate) fn is_statement_gap_emit_recovery(&self, source: &SourceFile) -> bool {
        self.supports_missing_nodes(source, true, true, true, false)
    }

    pub(crate) fn is_supported_for_emit(&self, source: &SourceFile) -> bool {
        self.supports_missing_nodes(source, true, true, true, true)
    }

    fn supports_missing_nodes(
        &self,
        source: &SourceFile,
        allow_missing_declarations: bool,
        allow_parameter_gaps: bool,
        allow_statement_gaps: bool,
        allow_context_recovery: bool,
    ) -> bool {
        if self.is_literal_only(source.parse_diagnostics.len()) {
            return true;
        }
        let context_support = if allow_context_recovery {
            let Some(support) = self.context_recovery_support(source) else {
                return false;
            };
            Some(support)
        } else {
            None
        };
        if self.diagnostic_origins.len() != source.parse_diagnostics.len()
            || !(allow_context_recovery
                || self.actions.is_empty()
                || allow_parameter_gaps
                    && self.supports_array_gaps(source, allow_statement_gaps, &self.actions))
        {
            return false;
        }
        if allow_statement_gaps {
            let mut retained = BTreeSet::new();
            for event in &self.events {
                if let Some(index) = event.diagnostic_index {
                    let Some(diagnostic) = source.parse_diagnostics.get(index) else {
                        return false;
                    };
                    if !retained.insert(index)
                        || event.kind
                            != ParseRecoveryKind::Diagnostic(self.diagnostic_origins[index])
                        || diagnostic.start != Some(event.start)
                        || diagnostic.length != Some(event.length)
                    {
                        return false;
                    }
                }
            }
            if retained.len() != source.parse_diagnostics.len() {
                return false;
            }
        }
        let parents = if allow_statement_gaps {
            let Some(parents) = Self::reachable_parents(source) else {
                return false;
            };
            Some(parents)
        } else {
            None
        };
        let mut missing_positions = BTreeMap::new();
        let mut report_only_count = 0;
        for (event_index, event) in self.events.iter().enumerate() {
            if matches!(event.kind, ParseRecoveryKind::Diagnostic(origin) if origin.is_literal())
                && event.missing_node.is_none()
            {
                continue;
            }
            let Some(missing) = event.missing_node else {
                if allow_statement_gaps
                    && event.kind == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                {
                    let admitted = match event.diagnostic_index {
                        Some(index) if index < source.parse_diagnostics.len() => {
                            self.actions.iter().any(|action| matches!(action,
                                ParseRecoveryAction::TokenSkipped { start, .. } if *start == event.start))
                            || self.report_has_retained_syntax_owner(source, parents.as_ref().unwrap(), event)
                            || context_support.as_ref().is_some_and(|support| support.assertion_reports.contains(&event_index))
                        }
                        None => self.events.iter().any(|retained| {
                            retained.kind == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                                && retained.start == event.start
                                && retained.diagnostic_index.is_some_and(|index| index < source.parse_diagnostics.len())
                        }),
                        _ => false,
                    };
                    if admitted && event.length > 0 {
                        report_only_count += 1;
                        continue;
                    }
                    return false;
                }
                if allow_parameter_gaps
                    && event.kind == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                    && event.diagnostic_index.is_none()
                    && self.actions.iter().any(|action| {
                        matches!(action, ParseRecoveryAction::TokenSkipped { start, .. } if *start == event.start)
                    })
                    && self.events.iter().any(|retained| {
                        retained.start == event.start
                            && retained.diagnostic_index.is_some()
                            && retained.missing_node.is_some_and(|missing| missing.kind == SyntaxKind::Identifier)
                    })
                {
                    continue;
                }
                return false;
            };
            if event.kind != ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                || !(missing.kind == SyntaxKind::Identifier
                    || allow_missing_declarations && missing.kind == SyntaxKind::MissingDeclaration)
                || !(event
                    .diagnostic_index
                    .is_some_and(|index| index < source.parse_diagnostics.len())
                    || context_support
                        .as_ref()
                        .is_some_and(|support| support.assertion_missing.contains(&event_index)))
                || missing_positions
                    .insert(missing.position, missing.kind)
                    .is_some()
            {
                return false;
            }
        }
        if missing_positions.is_empty() && report_only_count == 0 {
            return false;
        }
        // Inspect reachable syntax only: speculative parsing can leave orphaned
        // nodes in the arena, and JSDoc has its own diagnostic destination.
        let mut pending = vec![(source.root, None)];
        while let Some((id, parent)) = pending.pop() {
            let node = source.arena.node(id);
            if NodeFlags::from_bits(node.flags).contains(NodeFlags::JS_DOC) {
                continue;
            }
            if node.kind == SyntaxKind::Identifier && node.pos == node.end {
                let position = source
                    .positions()
                    .byte_to_utf16(node.pos)
                    .expect("syntax node positions are scalar boundaries");
                if !(parent == Some(SyntaxKind::AwaitExpression)
                    || allow_statement_gaps && parent == Some(SyntaxKind::TypeAssertionExpression)
                    || context_support
                        .as_ref()
                        .is_some_and(|support| support.missing_slots.contains(&id)))
                    || missing_positions.remove(&position) != Some(SyntaxKind::Identifier)
                {
                    return false;
                }
            }
            if node.kind == SyntaxKind::MissingDeclaration {
                let end = source
                    .positions()
                    .byte_to_utf16(node.end)
                    .expect("syntax node positions are scalar boundaries");
                if !matches!(
                    parent,
                    Some(
                        SyntaxKind::SourceFile
                            | SyntaxKind::Block
                            | SyntaxKind::ModuleBlock
                            | SyntaxKind::CaseClause
                            | SyntaxKind::DefaultClause
                    )
                ) || missing_positions.remove(&end) != Some(SyntaxKind::MissingDeclaration)
                {
                    return false;
                }
            }
            for_each_child(&source.arena, node, |child| {
                pending.push((child, Some(node.kind)));
                false
            });
        }
        missing_positions.is_empty()
    }

    fn reachable_parents(source: &SourceFile) -> Option<BTreeMap<NodeId, Option<NodeId>>> {
        let mut parents = BTreeMap::new();
        let mut pending = vec![(source.root, None)];
        while let Some((id, parent)) = pending.pop() {
            let node = source.arena.node(id);
            if NodeFlags::from_bits(node.flags).contains(NodeFlags::JS_DOC) {
                continue;
            }
            // Defensive tree invariant: ownership must not depend on which
            // edge reaches a shared node first.
            if parents.insert(id, parent).is_some() {
                return None;
            }
            for_each_child(&source.arena, node, |child| {
                pending.push((child, Some(id)));
                false
            });
        }
        Some(parents)
    }

    fn statement_array(data: &NodeData) -> Option<NodeArrayId> {
        match data {
            NodeData::SourceFile(data) => data.statements,
            NodeData::Block(data) => data.statements,
            NodeData::ModuleBlock(data) => data.statements,
            NodeData::CaseClause(data) => data.statements,
            NodeData::DefaultClause(data) => data.statements,
            _ => None,
        }
    }

    // Every skipped span belongs to one innermost reachable array gap. A
    // parameter gap additionally requires the retained missing await operand
    // in its preceding parameter. Statement/declaration gaps use the same
    // creation full-start boundary; all reports are checked independently.
    fn supports_array_gaps(
        &self,
        source: &SourceFile,
        allow_statement_gaps: bool,
        actions: &[ParseRecoveryAction],
    ) -> bool {
        let Some(parents) = Self::reachable_parents(source) else {
            return false;
        };
        let mut skip_spans = BTreeSet::new();
        // Only validated close-paren runs may bridge a gap after the last
        // retained statement. Every token still needs its own report and
        // must have the same unique reachable array owner.
        let mut close_paren_runs = BTreeMap::new();
        for action in actions {
            let ParseRecoveryAction::TokenSkipped {
                token,
                start,
                length,
                site: ParseTokenSkipSite::ListAbort,
                ..
            } = *action
            else {
                return false;
            };
            let Some(end) = start.checked_add(length).filter(|end| *end > start) else {
                return false;
            };
            if skip_spans
                .iter()
                .any(|(old_start, old_end)| start < *old_end && *old_start < end)
            {
                return false;
            }
            skip_spans.insert((start, end));
            let (Some(start_byte), Some(end_byte)) = (
                source.positions().utf16_to_byte(start),
                source.positions().utf16_to_byte(end),
            ) else {
                return false;
            };
            let retained = self
                .events
                .iter()
                .filter(|event| {
                    event.kind == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                        && event.start == start
                        && event.diagnostic_index.is_some()
                        && (event
                            .missing_node
                            .is_some_and(|missing| missing.kind == SyntaxKind::Identifier)
                            || allow_statement_gaps && event.missing_node.is_none())
                })
                .collect::<Vec<_>>();
            if retained.len() != 1 {
                return false;
            }
            let event = retained[0];
            let can_bridge_close_paren = allow_statement_gaps
                && event.missing_node.is_none()
                && token == SyntaxKind::CloseParenToken
                && event.length == length
                && source.text().get(start_byte as usize..end_byte as usize) == Some(")");
            let position = if allow_statement_gaps {
                if !Self::is_current_token_report(source, event)
                    || event
                        .missing_node
                        .is_some_and(|missing| missing.position != event.full_start)
                {
                    return false;
                }
                event.full_start
            } else {
                event.missing_node.unwrap().position
            };
            let Some(mut missing_byte) = source.positions().utf16_to_byte(position) else {
                return false;
            };
            let prior_close_paren = if can_bridge_close_paren {
                close_paren_runs.get(&event.full_start).copied()
            } else {
                None
            };
            if let Some((_, boundary)) = prior_close_paren {
                missing_byte = boundary;
            }
            let mut operand_ancestors = BTreeSet::new();
            let mut await_operand = false;
            if event.missing_node.is_some() {
                let operands = parents
                    .iter()
                    .filter_map(|(&id, &parent)| {
                        let node = source.arena.node(id);
                        let allowed_parent = parent.is_some_and(|parent| {
                            let kind = source.arena.node(parent).kind;
                            kind == SyntaxKind::AwaitExpression
                                || allow_statement_gaps
                                    && kind == SyntaxKind::TypeAssertionExpression
                        });
                        (node.kind == SyntaxKind::Identifier
                            && node.pos == missing_byte
                            && node.end == missing_byte
                            && allowed_parent)
                            .then_some(id)
                    })
                    .collect::<Vec<_>>();
                if operands.len() != 1 {
                    return false;
                }
                await_operand = parents[&operands[0]].is_some_and(|parent| {
                    source.arena.node(parent).kind == SyntaxKind::AwaitExpression
                });
                let mut current = Some(operands[0]);
                while let Some(ancestor) = current {
                    operand_ancestors.insert(ancestor);
                    current = parents[&ancestor];
                }
            }
            let mut owners = Vec::new();
            for &id in parents.keys() {
                let node = source.arena.node(id);
                let parameters = match &node.data {
                    NodeData::FunctionDeclaration(data) => data.parameters,
                    NodeData::FunctionExpression(data) => data.parameters,
                    NodeData::ArrowFunction(data) => data.parameters,
                    NodeData::MethodDeclaration(data) => data.parameters,
                    NodeData::Constructor(data) => data.parameters,
                    NodeData::GetAccessor(data) => data.parameters,
                    NodeData::SetAccessor(data) => data.parameters,
                    _ => None,
                };
                let is_parameters = parameters.is_some();
                let array = parameters.or_else(|| {
                    if !allow_statement_gaps {
                        return None;
                    }
                    if let NodeData::VariableDeclarationList(data) = &node.data {
                        data.declarations
                    } else {
                        Self::statement_array(&node.data)
                    }
                });
                let Some(array) = array else {
                    continue;
                };
                if is_parameters && !await_operand {
                    continue;
                }
                let array = source.arena.node_array(array);
                if array.pos > start_byte
                    || end_byte > array.end
                    || array.nodes.iter().any(|child| {
                        let child = source.arena.node(*child);
                        child.pos < end_byte && start_byte < child.end
                    })
                {
                    continue;
                }
                let preceding = array
                    .nodes
                    .iter()
                    .rev()
                    .find(|child| source.arena.node(**child).end <= start_byte);
                if !preceding.is_some_and(|child| {
                    source.arena.node(*child).end == missing_byte
                        && (event.missing_node.is_none() || operand_ancestors.contains(child))
                }) {
                    continue;
                }
                let mut ancestors = BTreeSet::new();
                let mut current = Some(id);
                while let Some(ancestor) = current {
                    ancestors.insert(ancestor);
                    current = parents[&ancestor];
                }
                if parents.keys().all(|other| {
                    let other_node = source.arena.node(*other);
                    !(other_node.pos <= start_byte && end_byte <= other_node.end)
                        || ancestors.contains(other)
                }) {
                    owners.push(id);
                }
            }
            if owners.len() != 1 || prior_close_paren.is_some_and(|(owner, _)| owner != owners[0]) {
                return false;
            }
            if can_bridge_close_paren {
                close_paren_runs.insert(end, (owners[0], missing_byte));
            }
        }
        !skip_spans.is_empty()
    }

    fn is_current_token_report(source: &SourceFile, event: &ParseRecoveryEvent) -> bool {
        let (Some(full_start), Some(start), Some(_end)) = (
            source.positions().utf16_to_byte(event.full_start),
            source.positions().utf16_to_byte(event.start),
            event
                .start
                .checked_add(event.length)
                .and_then(|end| source.positions().utf16_to_byte(end)),
        ) else {
            return false;
        };
        event.length > 0
            && crate::scanner::skip_trivia(source.text(), full_start as usize) == start as usize
    }

    fn report_has_retained_syntax_owner(
        &self,
        source: &SourceFile,
        parents: &BTreeMap<NodeId, Option<NodeId>>,
        event: &ParseRecoveryEvent,
    ) -> bool {
        let (Some(start), Some(end), Some(full_start)) = (
            source.positions().utf16_to_byte(event.start),
            event
                .start
                .checked_add(event.length)
                .and_then(|end| source.positions().utf16_to_byte(end)),
            source.positions().utf16_to_byte(event.full_start),
        ) else {
            return false;
        };
        if event.length == 0 {
            return false;
        }
        if Self::is_current_token_report(source, event) {
            let closers = parents
                .keys()
                .filter(|id| {
                    let node = source.arena.node(**id);
                    let NodeData::ParenthesizedExpression(data) = &node.data else {
                        return false;
                    };
                    node.end == full_start
                        && data
                            .expression
                            .is_some_and(|expression| source.arena.node(expression).end == node.end)
                })
                .copied()
                .collect::<BTreeSet<_>>();
            if closers.len() == 1 {
                return true;
            }
            if !closers.is_empty() {
                let roots = closers
                    .iter()
                    .filter(|id| !parents[*id].is_some_and(|parent| closers.contains(&parent)))
                    .copied()
                    .collect::<Vec<_>>();
                if roots.len() != 1 {
                    return false;
                }
                let mut chain = BTreeSet::new();
                let mut current = Some(roots[0]);
                while let Some(id) = current.filter(|id| closers.contains(id)) {
                    if !chain.insert(id) {
                        return false;
                    }
                    let NodeData::ParenthesizedExpression(data) = &source.arena.node(id).data
                    else {
                        return false;
                    };
                    current = data.expression;
                }
                let reports = self.events.iter().filter(|candidate| {
                    candidate.kind == ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                        && candidate.start == event.start
                        && candidate.length == event.length
                        && candidate.full_start == event.full_start
                        && candidate.missing_node.is_none()
                });
                return chain == closers
                    && reports.clone().count() == closers.len()
                    && reports
                        .filter(|candidate| candidate.diagnostic_index.is_some())
                        .count()
                        == 1;
            }
        }
        let statements = parents
            .keys()
            .filter(|id| {
                let node = source.arena.node(**id);
                let NodeData::ExpressionStatement(data) = &node.data else {
                    return false;
                };
                node.end == end
                    && crate::scanner::skip_trivia(source.text(), node.pos as usize)
                        == start as usize
                    && data
                        .expression
                        .is_some_and(|expression| source.arena.node(expression).end == node.end)
            })
            .count();
        if statements > 0 {
            return statements == 1;
        }
        Self::report_has_declaration_list_boundary(source, parents, event)
    }

    // A declaration list can stop without consuming the token that begins
    // the next statement. Both list and statement must end flush with their
    // last child, and the next independently checked missing operand belongs
    // to a TypeAssertionExpression. A consumed comma/semicolon fails this tie.
    fn report_has_declaration_list_boundary(
        source: &SourceFile,
        parents: &BTreeMap<NodeId, Option<NodeId>>,
        event: &ParseRecoveryEvent,
    ) -> bool {
        if !Self::is_current_token_report(source, event) {
            return false;
        }
        let Some(boundary) = source.positions().utf16_to_byte(event.full_start) else {
            return false;
        };
        let mut candidates = 0;
        for (&id, &parent) in parents {
            let list = source.arena.node(id);
            let NodeData::VariableDeclarationList(data) = &list.data else {
                continue;
            };
            if list.end != boundary {
                continue;
            }
            let Some(declarations) = data.declarations else {
                continue;
            };
            if source
                .arena
                .node_array(declarations)
                .nodes
                .last()
                .is_none_or(|last| source.arena.node(*last).end != boundary)
            {
                continue;
            }
            let Some(statement) = parent else {
                continue;
            };
            let node = source.arena.node(statement);
            if node.kind != SyntaxKind::VariableStatement || node.end != boundary {
                continue;
            }
            let Some(owner) = parents[&statement] else {
                continue;
            };
            let Some(statements) = Self::statement_array(&source.arena.node(owner).data) else {
                continue;
            };
            let statements = &source.arena.node_array(statements).nodes;
            let Some(index) = statements.iter().position(|child| *child == statement) else {
                continue;
            };
            let Some(next) = statements.get(index + 1) else {
                continue;
            };
            let next = source.arena.node(*next);
            let NodeData::ExpressionStatement(data) = &next.data else {
                continue;
            };
            if next.pos != boundary {
                continue;
            }
            let Some(expression) = data.expression else {
                continue;
            };
            let expression = source.arena.node(expression);
            let NodeData::TypeAssertionExpression(data) = &expression.data else {
                continue;
            };
            if expression.pos != boundary || expression.end != next.end {
                continue;
            }
            let Some(operand) = data.expression else {
                continue;
            };
            let operand = source.arena.node(operand);
            if operand.kind == SyntaxKind::Identifier && operand.pos == operand.end {
                candidates += 1;
            }
        }
        candidates == 1
    }

    pub(crate) fn checkpoint(&self) -> RecoveryCheckpoint {
        RecoveryCheckpoint {
            diagnostic_count: self.diagnostic_origins.len(),
            event_count: self.events.len(),
            action_count: self.actions.len(),
        }
    }

    pub(crate) fn restore(&mut self, checkpoint: RecoveryCheckpoint) {
        self.diagnostic_origins
            .truncate(checkpoint.diagnostic_count);
        self.events.truncate(checkpoint.event_count);
        self.actions.truncate(checkpoint.action_count);
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RecoveryCheckpoint {
    diagnostic_count: usize,
    event_count: usize,
    action_count: usize,
}
