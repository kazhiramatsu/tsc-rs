//! The created-token transform-flag table, asserted facet by facet.

use std::collections::BTreeSet;

use super::*;
use tsc_syntax::SyntaxKind;

const FACETS: &[(&str, TransformFlags)] = &[
    ("ES2015", TransformFlags::CONTAINS_ES_2015),
    ("Generator", TransformFlags::CONTAINS_GENERATOR),
    ("Yield", TransformFlags::CONTAINS_YIELD),
    (
        "HoistedDeclarationOrCompletion",
        TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION,
    ),
    ("LexicalThis", TransformFlags::CONTAINS_LEXICAL_THIS),
    ("LexicalSuper", TransformFlags::CONTAINS_LEXICAL_SUPER),
    ("BindingPattern", TransformFlags::CONTAINS_BINDING_PATTERN),
    ("RestOrSpread", TransformFlags::CONTAINS_REST_OR_SPREAD),
];

fn facet_row(flags: TransformFlags) -> BTreeSet<&'static str> {
    FACETS
        .iter()
        .filter(|(_, facet)| flags.contains(*facet))
        .map(|(name, _)| *name)
        .collect()
}

fn row(names: &[&'static str]) -> BTreeSet<&'static str> {
    names.iter().copied().collect()
}

#[test]
fn token_facets_match_the_creation_table() {
    assert_eq!(
        facet_row(classify_created_token_flags(SyntaxKind::SuperKeyword)),
        row(&["ES2015", "LexicalSuper"]),
    );
    assert_eq!(
        facet_row(classify_created_token_flags(SyntaxKind::ThisKeyword)),
        row(&["LexicalThis"]),
    );
    assert_eq!(
        facet_row(classify_created_token_flags(SyntaxKind::StaticKeyword)),
        row(&["ES2015"]),
    );
    assert_eq!(
        facet_row(classify_created_token_flags(SyntaxKind::AsteriskToken)),
        row(&[]),
    );
}
