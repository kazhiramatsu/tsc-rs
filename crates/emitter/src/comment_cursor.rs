use crate::{SourceBytePosition, TransformSourceId};

/// A validated source position whose meaning is comment ownership progress.
///
/// This is deliberately distinct from [`crate::token_cursor::TokenCursor`].
/// A token cursor advances across fixed token spellings; a comment cursor
/// advances only after a comment boundary has been emitted or claimed. Keeping
/// the domains separate prevents a detached-comment separator or a synthetic
/// token from being mistaken for source trivia that has already been owned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CommentCursor {
    source: TransformSourceId,
    position: SourceBytePosition,
}

/// Comment progress scoped to one leading-trivia owner.
///
/// A bare source position is insufficient here: two adjacent transformed
/// nodes can share a source while having different leading-comment owners.
/// Carrying the owner's start makes such resumptions impossible to merge by
/// accident and mirrors the key used by tsc's emitted-comment bookkeeping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CommentResume {
    owner_start: CommentCursor,
    next: CommentCursor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommentResumeError {
    SourceMismatch {
        owner_start: CommentCursor,
        next: CommentCursor,
    },
    BeforeOwner {
        owner_start: CommentCursor,
        next: CommentCursor,
    },
    OwnerMismatch {
        left: CommentCursor,
        right: CommentCursor,
    },
}

impl CommentCursor {
    pub(crate) const fn new(source: TransformSourceId, position: SourceBytePosition) -> Self {
        Self { source, position }
    }

    pub(crate) const fn source(self) -> TransformSourceId {
        self.source
    }

    pub(crate) const fn position(self) -> SourceBytePosition {
        self.position
    }
}

impl CommentResume {
    pub(crate) fn new(
        owner_start: CommentCursor,
        next: CommentCursor,
    ) -> Result<Self, CommentResumeError> {
        if owner_start.source() != next.source() {
            return Err(CommentResumeError::SourceMismatch { owner_start, next });
        }
        if next.position() < owner_start.position() {
            return Err(CommentResumeError::BeforeOwner { owner_start, next });
        }
        Ok(Self { owner_start, next })
    }

    pub(crate) const fn owner_start(self) -> CommentCursor {
        self.owner_start
    }

    pub(crate) const fn next(self) -> CommentCursor {
        self.next
    }

    pub(crate) fn furthest(self, other: Self) -> Result<Self, CommentResumeError> {
        if self.owner_start != other.owner_start {
            return Err(CommentResumeError::OwnerMismatch {
                left: self.owner_start,
                right: other.owner_start,
            });
        }
        Ok(if self.next.position() >= other.next.position() {
            self
        } else {
            other
        })
    }
}
