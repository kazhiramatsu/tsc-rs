use tsc_syntax::SyntaxKind;

use crate::comment_cursor::CommentResume;
use crate::{SourceBytePosition, TransformSourceId};

#[cfg(test)]
use std::cell::Cell;

/// Test-only accounting for the amount of local source work performed by the
/// position cursor. The complete type and storage disappear from production
/// builds; in particular, the emitter hot path retains no branch or atomic.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CursorWork {
    pub(crate) emissions: u64,
    pub(crate) source_bytes: u64,
}

#[cfg(test)]
std::thread_local! {
    static CURSOR_WORK: Cell<CursorWork> = Cell::new(CursorWork {
        emissions: 0,
        source_bytes: 0,
    });
}

#[cfg(test)]
pub(crate) fn reset_cursor_work() {
    CURSOR_WORK.with(|work| work.set(CursorWork::default()));
}

#[cfg(test)]
pub(crate) fn cursor_work() -> CursorWork {
    CURSOR_WORK.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn record_cursor_work(source_bytes: usize) {
    CURSOR_WORK.with(|work| {
        let current = work.get();
        work.set(CursorWork {
            emissions: current.emissions + 1,
            source_bytes: current.source_bytes + source_bytes as u64,
        });
    });
}

#[cfg(test)]
pub(crate) fn record_cursor_source_work(source_bytes: usize) {
    CURSOR_WORK.with(|work| {
        let current = work.get();
        work.set(CursorWork {
            emissions: current.emissions,
            source_bytes: current.source_bytes + source_bytes as u64,
        });
    });
}

/// A local source position threaded between fixed-token writes.
///
/// This is the typed counterpart of the `pos` returned by tsc's
/// `emitTokenWithComment`. It is intentionally not a lexer cursor: emission
/// may advance over a differently-spelled source token when a transform
/// inserts syntax (notably ES2019's optional-catch binding).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TokenCursor {
    Source {
        source: TransformSourceId,
        position: SourceBytePosition,
    },
    Synthetic,
}

impl TokenCursor {
    pub(crate) const fn source(source: TransformSourceId, position: SourceBytePosition) -> Self {
        Self::Source { source, position }
    }

    pub(crate) const fn source_position(self) -> Option<(TransformSourceId, SourceBytePosition)> {
        match self {
            Self::Source { source, position } => Some((source, position)),
            Self::Synthetic => None,
        }
    }
}

/// Selects the writer channel used for a fixed token. The channels currently
/// share storage behavior, but retaining the distinction keeps token emission
/// compatible with tsc's writer callbacks and future writer instrumentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TokenWriteKind {
    Keyword,
    Operator,
    Punctuation,
}

/// Selects the source boundary that may donate trailing comments to a token.
///
/// Most tokens stop at their containing node's end: trivia at that exact
/// boundary belongs to the parent container. A case-clause colon is also the
/// separator before the next clause, so it must retain its trailing comments
/// even when the current (fall-through) clause ends at the colon.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TokenCommentBoundary {
    OwnerEnd,
    AdjacentListItem,
}

/// Layout required immediately before the fixed spelling, after any comments
/// donated by its source anchor have been written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TokenLeadingSpace {
    None,
    Required,
}

/// Result of one token emission.
///
/// `comment_resume` is transient ownership information for the immediately
/// following token or child. Its owner boundary prevents reuse at a different
/// source position; it never indexes or caches source tokens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TokenEmission {
    cursor: TokenCursor,
    comment_resume: Option<CommentResume>,
}

/// A token anchor together with comments already consumed at that exact
/// position. Chaining the prior emission mirrors tsc's emitted-comment map
/// without retaining a source-wide token or comment cache.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TokenAnchor {
    cursor: TokenCursor,
    comment_resume: Option<CommentResume>,
}

impl TokenAnchor {
    pub(crate) const fn new(cursor: TokenCursor, comment_resume: Option<CommentResume>) -> Self {
        Self {
            cursor,
            comment_resume,
        }
    }

    pub(crate) const fn cursor(self) -> TokenCursor {
        self.cursor
    }

    pub(crate) const fn comment_resume(self) -> Option<CommentResume> {
        self.comment_resume
    }
}

impl From<TokenCursor> for TokenAnchor {
    fn from(cursor: TokenCursor) -> Self {
        Self {
            cursor,
            comment_resume: None,
        }
    }
}

impl From<TokenEmission> for TokenAnchor {
    fn from(emission: TokenEmission) -> Self {
        Self {
            cursor: emission.cursor,
            comment_resume: emission.comment_resume,
        }
    }
}

impl TokenEmission {
    pub(crate) const fn new(cursor: TokenCursor, comment_resume: Option<CommentResume>) -> Self {
        Self {
            cursor,
            comment_resume,
        }
    }

    pub(crate) const fn cursor(self) -> TokenCursor {
        self.cursor
    }

    pub(crate) const fn comment_resume(self) -> Option<CommentResume> {
        self.comment_resume
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FixedToken {
    pub(crate) kind: SyntaxKind,
    pub(crate) write_as: TokenWriteKind,
}

impl FixedToken {
    pub(crate) const fn keyword(kind: SyntaxKind) -> Self {
        Self {
            kind,
            write_as: TokenWriteKind::Keyword,
        }
    }

    pub(crate) const fn operator(kind: SyntaxKind) -> Self {
        Self {
            kind,
            write_as: TokenWriteKind::Operator,
        }
    }

    pub(crate) const fn punctuation(kind: SyntaxKind) -> Self {
        Self {
            kind,
            write_as: TokenWriteKind::Punctuation,
        }
    }
}
