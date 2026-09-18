use std::collections::BTreeSet;

use crate::{for_each_child, SourceFile, SyntaxKind};
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
        if self.is_literal_only(source.parse_diagnostics.len()) {
            return true;
        }
        if self.diagnostic_origins.len() != source.parse_diagnostics.len()
            || !self.actions.is_empty()
        {
            return false;
        }
        let mut missing_positions = BTreeSet::new();
        for event in &self.events {
            if matches!(event.kind, ParseRecoveryKind::Diagnostic(origin) if origin.is_literal())
                && event.missing_node.is_none()
            {
                continue;
            }
            let Some(missing) = event.missing_node else {
                return false;
            };
            if event.kind != ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
                || missing.kind != SyntaxKind::Identifier
                || !event
                    .diagnostic_index
                    .is_some_and(|index| index < source.parse_diagnostics.len())
                || !missing_positions.insert(missing.position)
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
                    || !missing_positions.remove(&position)
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
