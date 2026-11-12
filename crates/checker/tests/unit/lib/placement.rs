use super::*;

#[test]
fn changed_adoption_options_drop_the_placed_tree_and_report_its_reserved_hole() {
    use tsc_syntax::placement::{self as direct, SyntaxStripe};
    let prepared = tsc_program::PreparedSourceFile::new(
        tsc_program::ProgramPath::from_trusted_parts("/p/a.ts", "/p/a.ts").unwrap(),
        "/** @type {number} */ export const x = 1;",
    )
    .with_implied_node_formats(
        Some(tsc_program::ResolutionMode::CommonJs),
        Some(tsc_program::ResolutionMode::CommonJs),
    );
    let options = CompilerOptions {
        no_emit: Some(true),
        module: Some(199),
        module_resolution: Some(3),
        ..Default::default()
    };
    let identity_domain = IdentityDomain::new();
    let domain = &identity_domain;
    let mut stripe = SyntaxStripe::new(
        domain
            .lease_rooms(&[direct::bounds(prepared.text().len(), 1).unwrap()])
            .unwrap()
            .pop()
            .unwrap(),
    );
    let (_, syntax) =
        tsc_program::plan_source_requests_in_stripe(&prepared, &options, &mut stripe).unwrap();
    let layout = stripe.placements().to_vec();
    assert_eq!(layout.len(), 1);
    stripe.seal().unwrap();
    let input = InputFile::from_snapshot(
        prepared.path().display().to_owned(),
        Arc::clone(prepared.snapshot()),
    )
    .with_preparsed_syntax(syntax);
    let changed = if tsc_program::default_js_doc_parsing_mode() == JSDocParsingMode::ParseNone {
        JSDocParsingMode::ParseAll
    } else {
        JSDocParsingMode::ParseNone
    };
    let parse_options = tsc_syntax::ParseOptions {
        script_kind: tsc_syntax::ScriptKind::Ts,
        js_doc_parsing_mode: changed,
        ..Default::default()
    };
    assert!(input.take_preparsed_source(&parse_options).is_none());
    let replacement = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        input.name.clone(),
        Arc::clone(input.snapshot()),
        parse_options,
        None,
        domain,
    )
    .unwrap();
    let statistics = direct::adoption_statistics(&layout, [&replacement]);
    assert_eq!(
        (
            statistics.planned_sources,
            statistics.adopted_sources,
            statistics.dropped_sources
        ),
        (1, 0, 1)
    );
    assert_eq!(
        statistics.dropped_ids,
        layout[0].ranges().map(|range| u64::from(range.len()))
    );
}

#[test]
fn renamed_input_rejects_a_placed_tree_even_when_snapshot_and_options_match() {
    use tsc_syntax::placement::{self as direct, SyntaxStripe};
    let prepared = tsc_program::PreparedSourceFile::new(
        tsc_program::ProgramPath::from_trusted_parts("/p/a.ts", "/p/a.ts").unwrap(),
        "export const a = 1;",
    )
    .with_implied_node_formats(
        Some(tsc_program::ResolutionMode::CommonJs),
        Some(tsc_program::ResolutionMode::CommonJs),
    );
    let options = CompilerOptions {
        no_emit: Some(true),
        module: Some(199),
        module_resolution: Some(3),
        ..Default::default()
    };
    let identity_domain = IdentityDomain::new();
    let domain = &identity_domain;
    let mut stripe = SyntaxStripe::new(
        domain
            .lease_rooms(&[direct::bounds(prepared.text().len(), 1).unwrap()])
            .unwrap()
            .pop()
            .unwrap(),
    );
    let (_, syntax) =
        tsc_program::plan_source_requests_in_stripe(&prepared, &options, &mut stripe).unwrap();
    let layout = stripe.placements().to_vec();
    stripe.seal().unwrap();
    let expected = tsc_program::source_request_parse_options(&prepared, &options);
    let input = InputFile::from_snapshot("/p/A.ts", Arc::clone(prepared.snapshot()))
        .with_preparsed_syntax(syntax);
    assert!(input.take_preparsed_source(&expected).is_none());
    let actual = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        input.name.clone(),
        Arc::clone(input.snapshot()),
        expected,
        None,
        domain,
    )
    .unwrap();
    let statistics = direct::adoption_statistics(&layout, [&actual]);
    assert_eq!(
        (statistics.adopted_sources, statistics.dropped_sources),
        (0, 1)
    );
}
