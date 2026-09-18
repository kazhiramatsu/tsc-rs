use std::collections::{BTreeMap, BTreeSet};

use crate::{for_each_child, NodeData, SourceFile, SyntaxKind};
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
        self.supports_missing_nodes(source, false, false)
    }

    pub(crate) fn is_missing_node_emit_recovery(&self, source: &SourceFile) -> bool {
        self.supports_missing_nodes(source, true, false)
    }

    pub(crate) fn is_supported_for_emit(&self, source: &SourceFile) -> bool {
        self.supports_missing_nodes(source, true, true)
    }

    fn supports_missing_nodes(
        &self,
        source: &SourceFile,
        allow_missing_declarations: bool,
        allow_parameter_gaps: bool,
    ) -> bool {
        if self.is_literal_only(source.parse_diagnostics.len()) {
            return true;
        }
        if self.diagnostic_origins.len() != source.parse_diagnostics.len()
            || (!self.actions.is_empty()
                && !(allow_parameter_gaps && self.supports_parameter_gaps(source)))
        {
            return false;
        }
        let mut missing_positions = BTreeMap::new();
        for event in &self.events {
            if matches!(event.kind, ParseRecoveryKind::Diagnostic(origin) if origin.is_literal())
                && event.missing_node.is_none()
            {
                continue;
            }
            let Some(missing) = event.missing_node else {
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
                || !event
                    .diagnostic_index
                    .is_some_and(|index| index < source.parse_diagnostics.len())
                || missing_positions
                    .insert(missing.position, missing.kind)
                    .is_some()
            {
                return false;
            }
        }
        if missing_positions.is_empty() {
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
                if parent != Some(SyntaxKind::AwaitExpression)
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

    // A parameter gap is emittable only when its preceding parameter ends in
    // a retained missing await operand, whose reporting token was skipped.
    // The reachable tree and array ownership are checked independently of the
    // diagnostic code, token spelling, and fixture identity.
    fn supports_parameter_gaps(&self, source: &SourceFile) -> bool {
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
                return false;
            }
            for_each_child(&source.arena, node, |child| {
                pending.push((child, Some(id)));
                false
            });
        }
        let mut skip_spans = BTreeSet::new();
        for action in &self.actions {
            let ParseRecoveryAction::TokenSkipped {
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
                        && event
                            .missing_node
                            .is_some_and(|missing| missing.kind == SyntaxKind::Identifier)
                })
                .collect::<Vec<_>>();
            if retained.len() != 1 {
                return false;
            }
            let missing = retained[0].missing_node.unwrap();
            let Some(missing_byte) = source.positions().utf16_to_byte(missing.position) else {
                return false;
            };
            let operands = parents
                .iter()
                .filter_map(|(&id, &parent)| {
                    let node = source.arena.node(id);
                    (node.kind == SyntaxKind::Identifier
                        && node.pos == missing_byte
                        && node.end == missing_byte
                        && parent.is_some_and(|parent| {
                            source.arena.node(parent).kind == SyntaxKind::AwaitExpression
                        }))
                    .then_some(id)
                })
                .collect::<Vec<_>>();
            if operands.len() != 1 {
                return false;
            }
            let mut operand_ancestors = BTreeSet::new();
            let mut current = Some(operands[0]);
            while let Some(ancestor) = current {
                operand_ancestors.insert(ancestor);
                current = parents[&ancestor];
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
                let Some(parameters) = parameters else {
                    continue;
                };
                let array = source.arena.node_array(parameters);
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
                        && operand_ancestors.contains(child)
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
            if owners.len() != 1 {
                return false;
            }
        }
        !skip_spans.is_empty()
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
