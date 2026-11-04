use super::*;

#[test]
fn source_and_diagnostic_file_names_preserve_js_units() {
    for unit in [0xd800, 0xd801, 0xdc00, 0xfffd] {
        let mut name = JsString::from("/virtual/file.d.");
        name.push_code_unit(unit);
        name.push_str(".ts");
        let source = parse_source_file(&name, "'\\unicode';", ParseOptions::default(), None);
        assert_eq!(source.file_name.to_utf16(), name.to_utf16());
        assert!(source.is_declaration_file);
        assert!(!source.parse_diagnostics.is_empty());
        assert!(source.has_only_literal_recovery());
        assert!(source
            .parse_diagnostics
            .iter()
            .all(|d| d.file_name.as_ref() == Some(&name)));

        let json = parse_json_text(&name, "{\"a\": }");
        assert_eq!(json.file_name.to_utf16(), name.to_utf16());
        assert!(!json.parse_diagnostics.is_empty());
        assert!(json
            .parse_diagnostics
            .iter()
            .all(|d| d.file_name.as_ref() == Some(&name)));
    }
}

#[test]
fn parse_source_file_creates_root_and_eof_nodes() {
    let source = parse_source_file("a.ts", "", ParseOptions::default(), None);

    assert_eq!(source.node_count(), 2);
    assert_eq!(source.identifier_count(), 0);
    assert_eq!(source.positions().line_count(), 1);
    assert_eq!(source.positions().line_start_utf16(0), Some(0));
    assert_eq!(source.arena.node(source.root).kind, SyntaxKind::SourceFile);

    let data = source
        .arena
        .node(source.root)
        .data()
        .as_source_file()
        .expect("root is a source file");
    let eof = data.end_of_file_token.expect("source file has EOF token");
    assert_eq!(source.arena.node(eof).kind, SyntaxKind::EndOfFileToken);
    assert_eq!(source.arena.node(eof).parent, Some(source.root));
}

/// tsgo `IdentifierCount` (tsc 7.1.0-dev `aa814927`, `--extendedDiagnostics`
/// on each text alone): an arrow function whose body is an `async function`
/// expression is parsed once (nextIsUnParenthesizedAsyncArrowFunction reads
/// tokens only), and every element access with an argument counts the missing
/// identifier tsgo creates before parsing the argument.
#[test]
fn identifier_count_matches_tsgo() {
    for (text, count) in [
        (
            "const f = (options: X): Y => async function g(c, next) { a1; a2; a3; a4 }\n",
            12,
        ),
        (
            "const f = (options: X) => async function g(c, next) { a1; a2; a3; a4 }\n",
            10,
        ),
        ("const f = async x => x\n", 3),
        ("x[y]\n", 3),
        ("a[0] as T\n", 3),
        ("const m = this.x[method] as Matcher<T>\n", 6),
        ("x[]\n", 2),
    ] {
        let source = parse_source_file("a.ts", text, ParseOptions::default(), None);
        assert_eq!(source.identifier_count(), count, "{text:?}");
    }
}

/// tsgo's ScannerState carries the comment directives: a speculation that is
/// rewound (here a failed parenthesized-arrow parse, and the async-arrow
/// lookahead of 6.0.3 that read a whole function body) leaves each directive
/// once.
#[test]
fn rewound_speculation_leaves_each_comment_directive_once() {
    for text in [
        "let v = (a,\n// @ts-expect-error\nb);\n",
        "const f = (o: X) => async function () {\n// @ts-ignore\na\n}\n",
    ] {
        let source = parse_source_file("a.ts", text, ParseOptions::default(), None);
        assert_eq!(source.comment_directives.len(), 1, "{text:?}");
    }
}

#[test]
fn snapshot_entry_preserves_text_and_position_owner_identity() {
    let snapshot = TextSnapshot::new("const value = '😀';\n", DocumentVersion::new("host-v1"));
    let source = parse_source_file_from_snapshot(
        "a.ts",
        Arc::clone(&snapshot),
        ParseOptions::default(),
        None,
    );

    assert!(Arc::ptr_eq(&snapshot, source.snapshot()));
    assert!(Arc::ptr_eq(
        &snapshot.shared_text(),
        &source.snapshot().shared_text()
    ));
    assert!(Arc::ptr_eq(
        &snapshot.shared_positions(),
        &source.snapshot().shared_positions()
    ));
    assert_eq!(
        source.positions().kind(),
        tsc_diagnostics::PositionIndexKind::StaticDense
    );
}

#[test]
fn detached_arena_preserves_ids_without_extending_published_leases() {
    let mut source = parse_source_file(
        "emit.ts",
        "const value = 1;\n",
        ParseOptions::default(),
        None,
    );
    let domain = IdentityDomain::new();
    source.relocate_into_identity_domain(&domain).unwrap();
    assert!(source.arena.has_identity_leases());

    let original_node_end = source.arena.node_end();
    let original_array_end = source.arena.array_end();
    let mut detached = source.arena.detached_clone();
    assert!(!detached.has_identity_leases());
    assert_eq!(detached.node_base(), source.arena.node_base());
    assert_eq!(detached.array_base(), source.arena.array_base());
    assert_eq!(detached.node_end(), original_node_end);
    assert_eq!(detached.array_end(), original_array_end);

    let synthetic = detached.alloc_token(
        SyntaxKind::ThisKeyword,
        usize::MAX,
        usize::MAX,
        tsc_types::NodeFlags::SYNTHESIZED,
    );
    assert_eq!(synthetic.index(), original_node_end);
    assert!(!source.arena.contains_node(synthetic));
    assert_eq!(source.arena.node_end(), original_node_end);
    assert!(source.arena.has_identity_leases());
}

#[test]
fn generated_relocation_matches_a_direct_forced_nonzero_parse() {
    let text = r#"
/** @typedef {{ value: string }} Alias */
export class Box { #value = 1; method<T>(arg: T) { return [arg, this.#value]; } }
"#;
    let mut local = parse_source_file("forced.ts", text, ParseOptions::default(), None);
    let domain = IdentityDomain::new();
    // Leases ahead of the tree keep its bases away from the start of the
    // spaces in a fresh process.
    let node_gap = domain.lease(IdentitySpace::Node, 11).unwrap();
    let array_gap = domain.lease(IdentitySpace::NodeArray, 7).unwrap();

    local.relocate_into_identity_domain(&domain).unwrap();
    assert!(local.identity_owned_by(&domain));
    let leases = local.arena.identity_leases().unwrap();
    assert_eq!(leases.nodes.range().start(), local.arena.node_base());
    assert_eq!(leases.arrays.range().start(), local.arena.array_base());
    let nodes = leases.nodes.watch();

    let direct = parse_source_file(
        "forced.ts",
        text,
        ParseOptions {
            node_id_base: local.arena.node_base(),
            node_array_id_base: local.arena.array_base(),
            ..ParseOptions::default()
        },
        None,
    );
    assert_eq!(local, direct);
    assert!(local
        .arena
        .node_ids()
        .all(|id| local.arena.contains_node(id)));

    drop(local);
    assert!(!nodes.is_held());
    let gap = node_gap.watch();
    drop(node_gap);
    drop(array_gap);
    assert!(!gap.is_held());
}

#[test]
fn source_clones_retain_leases_and_ephemeral_parse_seals_exact_counts() {
    let reclaiming = IdentityDomain::new();
    let mut source = parse_source_file("clone.ts", "export const x = 1;", Default::default(), None);
    source.relocate_into_identity_domain(&reclaiming).unwrap();
    let nodes = source.arena.identity_leases().unwrap().nodes.watch();
    let retained = source.clone();
    drop(source);
    assert!(nodes.is_held(), "a clone retains the lease");
    drop(retained);
    assert!(!nodes.is_held());

    let ephemeral = IdentityDomain::new();
    let snapshot = TextSnapshot::new("let y = 2;", DocumentVersion::new("v1"));
    let source = parse_source_file_from_snapshot_in_identity_domain(
        "ephemeral.ts",
        snapshot,
        ParseOptions::default(),
        None,
        &ephemeral,
    )
    .unwrap();
    assert!(source.identity_owned_by(&ephemeral));
    assert_eq!(
        source.arena.identity_leases().unwrap().nodes.range().len() as usize,
        source.arena.nodes().len()
    );
    assert_eq!(
        source.arena.identity_leases().unwrap().arrays.range().len() as usize,
        source.arena.node_arrays().len()
    );
}

#[test]
fn incremental_update_reuses_unchanged_list_elements_with_fresh_ids() {
    use tsc_diagnostics::{ByteTextChangeRange, ByteTextSpan};

    let before = "const first = 1;\nconst middle = 2;\nconst last = 3;\n";
    let old_snapshot = TextSnapshot::new(before, DocumentVersion::new("1"));
    let domain = IdentityDomain::new();
    let old = Arc::new(
        parse_source_file_from_snapshot_in_identity_domain(
            "incremental.ts",
            Arc::clone(&old_snapshot),
            ParseOptions::default(),
            None,
            &domain,
        )
        .unwrap(),
    );
    let edit_start = before.find("2;").unwrap() as u32;
    let after = before.replacen("2;", "20;", 1);
    let new_snapshot = TextSnapshot::new(after, DocumentVersion::new("2"));
    let result = update_language_service_source_file_in_identity_domain(
        Arc::clone(&old),
        Arc::clone(&new_snapshot),
        ByteTextChangeRange {
            span: ByteTextSpan::new(edit_start, 1),
            new_length: 2,
        },
        ParseOptions::default(),
        IncrementalParseOptions {
            record_reuse_lineage: true,
        },
        &domain,
    )
    .unwrap();

    assert!(result.stats.incremental);
    assert!(!result.stats.full_parse_fallback);
    assert!(
        result.stats.reused_list_elements >= 2,
        "{:#?}",
        result.stats
    );
    assert!(result.stats.reused_nodes > result.stats.reused_list_elements);
    assert_eq!(result.stats.lineage.len(), result.stats.reused_nodes);
    assert!(Arc::ptr_eq(result.source.snapshot(), &new_snapshot));
    assert_eq!(
        result.source.text(),
        "const first = 1;\nconst middle = 20;\nconst last = 3;\n"
    );
    assert!(result.source.parse_diagnostics.is_empty());
    assert!(result.stats.lineage.iter().all(|lineage| {
        old.arena.contains_node(lineage.old_node)
            && result.source.arena.contains_node(lineage.new_node)
            && lineage.old_node != lineage.new_node
    }));
}

#[test]
fn incremental_update_rejects_an_inexact_change_range() {
    use tsc_diagnostics::{ByteTextChangeRange, ByteTextSpan};

    let old_snapshot = TextSnapshot::new("const value = 1;", DocumentVersion::new("1"));
    let old = Arc::new(parse_source_file_from_snapshot(
        "invalid-change.ts",
        old_snapshot,
        ParseOptions::default(),
        None,
    ));
    let new_snapshot = TextSnapshot::new("const value = 2;", DocumentVersion::new("2"));
    let error = update_language_service_source_file(
        old,
        new_snapshot,
        ByteTextChangeRange {
            span: ByteTextSpan::new(0, 1),
            new_length: 1,
        },
        ParseOptions::default(),
        IncrementalParseOptions::default(),
    )
    .unwrap_err();
    assert_eq!(error, IncrementalParseError::SuffixMismatch);
}

/// tsgo textToKeyword: every keyword kind is found from its text, and only
/// a keyword's exact text finds one.
#[test]
fn keyword_kind_finds_every_keyword_and_nothing_else() {
    let first = SyntaxKind::FirstKeyword as u16;
    let last = SyntaxKind::LastKeyword as u16;
    for value in first..=last {
        let kind = SyntaxKind::from_u16(value).expect("a keyword kind");
        let text = tokens::token_to_string(kind).expect("a keyword's text");
        assert_eq!(keyword_kind(text), Some(kind), "{text}");
    }
    for text in [
        "",
        "a",
        "x",
        "_",
        "$",
        "Abstract",
        "constructors",
        "awaits",
        "infer_",
        "intrinsics",
        "yieldd",
        "zzzzzzzzzzzz",
        "ńew",
    ] {
        assert_eq!(keyword_kind(text), None, "{text:?}");
    }
}
