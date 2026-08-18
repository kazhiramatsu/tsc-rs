use crate::{CommentRange, SourceBytePosition, SourceRange, TransformSourceId};

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

/// The printer's three independently scoped comment-container values,
/// threaded as one immutable value.
///
/// tsc keeps `containerPos`, `containerEnd`, and
/// `declarationListContainerEnd` as printer-closure variables with `-1`
/// as the "no container" sentinel, saved and restored around every
/// commented node emission. The Rust model threads the scope by value:
/// a child's claim produces a new scope for the child's subtree while
/// the parent's value is untouched, which expresses the restore-before-
/// trailing ordering structurally — a node's trailing phase always
/// consults the parent's scope value.
///
/// The claimed container is stored as the paired unit the qualified
/// H2.5g projection established: both sides claimed together, and a
/// synthesized or zero-width inherited range accepted as a
/// present-but-inert claim that matches no guard. tsc's claim gate
/// rejects those ranges, and the per-side independent claim conditions
/// split this unit into per-side values when the expression-route
/// migration lands under the frozen witnesses; the two `-1` sentinels
/// are the `None` view results until then. There is deliberately no
/// `Default`: the zero scope exists only at the printer's root and its
/// named transitional entries.
///
/// tsc-port: containerPos/containerEnd/declarationListContainerEnd @6.0.3
/// tsc-span: _tsc.js:116957-116959 (state); _tsc.js:121007-121052
/// (claim/restore); _tsc.js:121219-121238 (guarded readers)
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CommentEmissionScope {
    container: Option<CommentRange>,
    declaration_list_container_end: Option<CommentCursor>,
}

impl CommentEmissionScope {
    /// The `-1/-1/-1` initial printer state. Constructed only by the
    /// printer root and its named transitional detached entries; nested
    /// routes receive the threaded value.
    pub(crate) const fn empty() -> Self {
        Self {
            container: None,
            declaration_list_container_end: None,
        }
    }

    /// A container claim in the qualified paired form: the claimed unit
    /// is replaced while the declaration-list end keeps the inherited
    /// value, exactly the shape `emitLeadingCommentsOfNode` writes for a
    /// non-list node.
    pub(crate) const fn claim_container_unit(self, container: CommentRange) -> Self {
        Self {
            container: Some(container),
            declaration_list_container_end: self.declaration_list_container_end,
        }
    }

    /// The claimed unit in its stored range form, for the deferred
    /// comment stores and the leading owned-prefix helpers. Presence
    /// alone is meaningful: an inert claim still suppresses the
    /// parent-end fallback at the trailing escape checks.
    pub(crate) const fn container_unit(self) -> Option<CommentRange> {
        self.container
    }

    /// The `containerPos` view of one container range, read by the leading
    /// guard (`pos !== containerPos`). Inert containers have no position:
    /// tsc would never have claimed them.
    pub(crate) fn container_pos_of(container: CommentRange) -> Option<CommentCursor> {
        match container.range() {
            SourceRange::Original(range) if range.start() != range.end() => {
                Some(CommentCursor::new(container.source(), range.start()))
            }
            _ => None,
        }
    }

    /// The `containerEnd` view of one container range, read by the trailing
    /// guard. Shared with the per-node deferred-container check so the
    /// ambient guard and the per-node guard cannot drift apart.
    pub(crate) fn container_end_of(container: CommentRange) -> Option<CommentCursor> {
        match container.range() {
            SourceRange::Original(range) if range.start() != range.end() => {
                Some(CommentCursor::new(container.source(), range.end()))
            }
            _ => None,
        }
    }

    /// The `containerEnd` view of the claimed unit.
    pub(crate) fn container_end(self) -> Option<CommentCursor> {
        self.container.and_then(Self::container_end_of)
    }

    /// tsc's trailing guard: trivia at `end` stays with the enclosing
    /// owner when the claimed container or the active declaration list
    /// already ends there (`end !== containerEnd && end !==
    /// declarationListContainerEnd`, inverted).
    pub(crate) fn retains_end(self, end: CommentCursor) -> bool {
        self.container_end() == Some(end) || self.declaration_list_container_end == Some(end)
    }

    /// Arbitrary scope state for unit contracts only. Production code has
    /// exactly one zero-scope constructor and, until the declaration-list
    /// route migration lands, no `declaration_list_container_end` writer.
    #[cfg(test)]
    pub(crate) const fn contract_scope(
        container: Option<CommentRange>,
        declaration_list_container_end: Option<CommentCursor>,
    ) -> Self {
        Self {
            container,
            declaration_list_container_end,
        }
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
