//! One contract per row of the per-side claim predicate table
//! (`emitLeadingCommentsOfNode`, `_tsc.js:121007-121032`): a side of an
//! `Original` nonempty range goes unclaimed only for `JsxText` without
//! that side's suppression flag, a suppression flag claims while
//! suppressing, and unclaimable ranges leave the enclosing scope
//! active.

use tsc_syntax::{parse_source_file, SyntaxKind};

use super::{
    CommentCursor, CommentEmissionScope, CommentRange, EmitFlags, ExpressionCommentPhaseOwner,
    Printer, SourceRange,
};
use crate::{SourceBytePosition, SourceByteRange, TransformArena, TransformSourceId};

struct PredicateFixture {
    parsed: tsc_syntax::SourceFile,
    source: TransformSourceId,
}

fn fixture() -> PredicateFixture {
    let parsed = parse_source_file(
        "predicate.ts",
        "/* a */ value; other;\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    PredicateFixture { parsed, source }
}

impl PredicateFixture {
    fn cursor(&self, value: u32) -> CommentCursor {
        CommentCursor::new(
            self.source,
            SourceBytePosition::new(value, self.parsed.positions()).expect("source position"),
        )
    }

    fn owner(
        &self,
        start: u32,
        end: u32,
        flags: EmitFlags,
        kind: SyntaxKind,
    ) -> ExpressionCommentPhaseOwner {
        ExpressionCommentPhaseOwner {
            range: CommentRange::new(
                self.source,
                SourceRange::Original(
                    SourceByteRange::new(start, end, self.parsed.positions())
                        .expect("source range"),
                ),
            ),
            flags,
            kind,
            relocated_trailing: false,
        }
    }

    fn synthesized_owner(&self, flags: EmitFlags, kind: SyntaxKind) -> ExpressionCommentPhaseOwner {
        ExpressionCommentPhaseOwner {
            range: CommentRange::new(self.source, SourceRange::Synthesized),
            flags,
            kind,
            relocated_trailing: false,
        }
    }
}

#[test]
fn ordinary_original_range_claims_both_sides() {
    let fixture = fixture();
    let owner = fixture.owner(8, 14, EmitFlags::NONE, SyntaxKind::Identifier);
    assert_eq!(
        Printer::established_container_sides(owner),
        (Some(fixture.cursor(8)), Some(fixture.cursor(14))),
    );
}

#[test]
fn at_zero_start_with_positive_end_claims_both_sides() {
    let fixture = fixture();
    let owner = fixture.owner(0, 14, EmitFlags::NONE, SyntaxKind::Identifier);
    assert_eq!(
        Printer::established_container_sides(owner),
        (Some(fixture.cursor(0)), Some(fixture.cursor(14))),
    );
}

#[test]
fn suppression_flags_claim_while_suppressing_emission() {
    let fixture = fixture();
    for flags in [
        EmitFlags::NO_LEADING_COMMENTS,
        EmitFlags::NO_TRAILING_COMMENTS,
        EmitFlags::NO_LEADING_COMMENTS | EmitFlags::NO_TRAILING_COMMENTS,
    ] {
        let owner = fixture.owner(8, 14, flags, SyntaxKind::Identifier);
        assert_eq!(
            Printer::established_container_sides(owner),
            (Some(fixture.cursor(8)), Some(fixture.cursor(14))),
            "a suppression flag suppresses emission, never the claim",
        );
    }
}

#[test]
fn jsx_text_without_flags_claims_neither_side() {
    let fixture = fixture();
    let owner = fixture.owner(8, 14, EmitFlags::NONE, SyntaxKind::JsxText);
    assert_eq!(Printer::established_container_sides(owner), (None, None));
}

#[test]
fn jsx_text_claims_exactly_the_flagged_side() {
    let fixture = fixture();
    let leading = fixture.owner(8, 14, EmitFlags::NO_LEADING_COMMENTS, SyntaxKind::JsxText);
    assert_eq!(
        Printer::established_container_sides(leading),
        (Some(fixture.cursor(8)), None),
    );
    let trailing = fixture.owner(8, 14, EmitFlags::NO_TRAILING_COMMENTS, SyntaxKind::JsxText);
    assert_eq!(
        Printer::established_container_sides(trailing),
        (None, Some(fixture.cursor(14))),
    );
}

#[test]
fn synthesized_and_zero_width_ranges_claim_nothing_so_the_scope_inherits() {
    let fixture = fixture();
    let synthesized = fixture.synthesized_owner(EmitFlags::NONE, SyntaxKind::Identifier);
    assert_eq!(
        Printer::established_container_sides(synthesized),
        (None, None)
    );
    let zero_width = fixture.owner(14, 14, EmitFlags::NONE, SyntaxKind::Identifier);
    assert_eq!(
        Printer::established_container_sides(zero_width),
        (None, None)
    );

    let outer = CommentEmissionScope::empty()
        .claim_sides(Some(fixture.cursor(0)), Some(fixture.cursor(21)));
    let (pos, end) = Printer::established_container_sides(synthesized);
    assert_eq!(
        outer.claim_sides(pos, end),
        outer,
        "the enclosing scope stays active"
    );
}
