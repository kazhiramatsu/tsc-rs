use crate::SyntaxKind;

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
pub struct ParseRecoveryEvent {
    pub kind: ParseRecoveryKind,
    /// Source positions in UTF-16 units, as in syntactic diagnostics.
    pub start: u32,
    pub length: u32,
    pub diagnostic_index: Option<usize>,
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
}

impl ParseRecovery {
    pub fn diagnostic_origins(&self) -> &[ParseDiagnosticOrigin] {
        &self.diagnostic_origins
    }

    pub fn events(&self) -> &[ParseRecoveryEvent] {
        &self.events
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

    pub(crate) fn checkpoint(&self) -> RecoveryCheckpoint {
        RecoveryCheckpoint {
            diagnostic_count: self.diagnostic_origins.len(),
            event_count: self.events.len(),
        }
    }

    pub(crate) fn restore(&mut self, checkpoint: RecoveryCheckpoint) {
        self.diagnostic_origins
            .truncate(checkpoint.diagnostic_count);
        self.events.truncate(checkpoint.event_count);
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RecoveryCheckpoint {
    diagnostic_count: usize,
    event_count: usize,
}
