use tsc_syntax::{parse_source_file, SourceFile};

use super::{
    comment_cursor::{CommentCursor, CommentEmissionScope},
    CommentRange, SourceBytePosition, SourceByteRange, SourceRange, TransformArena,
    TransformSourceId,
};

struct ScopeFixture {
    parsed: SourceFile,
    source: TransformSourceId,
}

fn ranged_fixture(file_name: &str) -> ScopeFixture {
    let parsed = parse_source_file(
        file_name,
        "/* a */ value; other;\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    ScopeFixture { parsed, source }
}

impl ScopeFixture {
    fn cursor(&self, value: u32) -> CommentCursor {
        CommentCursor::new(
            self.source,
            SourceBytePosition::new(value, self.parsed.positions()).expect("source position"),
        )
    }

    fn range(&self, start: u32, end: u32) -> CommentRange {
        CommentRange::new(
            self.source,
            SourceRange::Original(
                SourceByteRange::new(start, end, self.parsed.positions()).expect("source range"),
            ),
        )
    }
}

#[test]
fn empty_scope_retains_no_end_and_exposes_no_views() {
    let fixture = ranged_fixture("scope.ts");
    let scope = CommentEmissionScope::empty();
    assert_eq!(scope.container_unit(), None);
    assert_eq!(scope.container_end(), None);
    assert!(!scope.retains_end(fixture.cursor(14)));
}

#[test]
fn claimed_scope_retains_exactly_its_end_cursor() {
    let fixture = ranged_fixture("scope.ts");
    let container = fixture.range(8, 14);
    let scope = CommentEmissionScope::empty().claim_container_unit(container);
    assert_eq!(scope.container_unit(), Some(container));
    assert_eq!(
        CommentEmissionScope::container_pos_of(container),
        Some(fixture.cursor(8)),
    );
    assert_eq!(scope.container_end(), Some(fixture.cursor(14)));
    assert!(scope.retains_end(fixture.cursor(14)));
    assert!(!scope.retains_end(fixture.cursor(8)));

    // The same byte offset in a different source is a different cursor:
    // the guard must not match across sources.
    let first = parse_source_file(
        "scope.ts",
        "/* a */ value; other;\n",
        Default::default(),
        None,
    );
    let second = parse_source_file(
        "other.ts",
        "/* a */ value; other;\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let first_source = arena.add_source(&first, None);
    let second_source = arena.add_source(&second, None);
    assert_ne!(first_source, second_source);
    let first_container = CommentRange::new(
        first_source,
        SourceRange::Original(
            SourceByteRange::new(8, 14, first.positions()).expect("source range"),
        ),
    );
    let cross_scope = CommentEmissionScope::empty().claim_container_unit(first_container);
    let foreign_end = CommentCursor::new(
        second_source,
        SourceBytePosition::new(14, second.positions()).expect("source position"),
    );
    assert!(!cross_scope.retains_end(foreign_end));
}

#[test]
fn synthesized_and_zero_width_claims_are_present_but_inert() {
    let fixture = ranged_fixture("scope.ts");
    let synthesized = CommentRange::new(fixture.source, SourceRange::Synthesized);
    let zero_width = fixture.range(14, 14);
    for container in [synthesized, zero_width] {
        let scope = CommentEmissionScope::empty().claim_container_unit(container);
        assert_eq!(scope.container_unit(), Some(container));
        assert_eq!(CommentEmissionScope::container_pos_of(container), None);
        assert_eq!(scope.container_end(), None);
        assert!(!scope.retains_end(fixture.cursor(14)));
        assert_eq!(CommentEmissionScope::container_end_of(container), None);
    }
}

#[test]
fn claiming_replaces_the_unit_and_preserves_the_declaration_list_end() {
    let fixture = ranged_fixture("scope.ts");
    let inherited =
        CommentEmissionScope::contract_scope(Some(fixture.range(0, 21)), Some(fixture.cursor(14)));
    let claimed = inherited.claim_container_unit(fixture.range(8, 13));
    assert_eq!(claimed.container_unit(), Some(fixture.range(8, 13)));
    assert_eq!(claimed.container_end(), Some(fixture.cursor(13)));
    assert!(claimed.retains_end(fixture.cursor(13)));
    assert!(!claimed.retains_end(fixture.cursor(21)));
    // The declaration-list end survives the claim, exactly the non-list
    // shape of tsc's emitLeadingCommentsOfNode.
    assert!(claimed.retains_end(fixture.cursor(14)));
}

#[test]
fn declaration_list_end_guards_without_a_claimed_unit() {
    let fixture = ranged_fixture("scope.ts");
    let scope = CommentEmissionScope::contract_scope(None, Some(fixture.cursor(14)));
    assert_eq!(scope.container_unit(), None);
    assert_eq!(scope.container_end(), None);
    assert!(scope.retains_end(fixture.cursor(14)));
    assert!(!scope.retains_end(fixture.cursor(13)));
}
