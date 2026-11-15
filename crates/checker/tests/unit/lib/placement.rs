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

#[test]
fn a_mixed_library_prefix_reserves_before_restoring_image_binds() {
    const CHILD: &str = "TSC_TEST_MIXED_IMAGE_ORDER_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "direct_contract_tests::a_mixed_library_prefix_reserves_before_restoring_image_binds",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }

    // The child owns a fresh process identity allocator and a test catalog.
    // It does not replace the compiler catalog in other checker tests.
    let name = "lib.image-order-test.d.ts";
    let file_name = format!("bundled:///libs/{name}");
    let text = "interface ImageLater { value: string }";
    let catalog = Box::leak(
        [
            (name, text),
            (
                "lib.image-order-two.d.ts",
                "interface ImageOther { value: number }",
            ),
        ]
        .into_iter()
        .map(|(name, text)| {
            let file_name = format!("bundled:///libs/{name}");
            let image =
                tsc_binder::library_image::build_command_library_image(&file_name, text).unwrap();
            let source = Box::leak(Box::new(tsc_diagnostics::SharedText::from(text)));
            tsc_syntax::library_image::StaticLibraryImage {
                name,
                source,
                source_fingerprint: tsc_syntax::library_image::fingerprint(text.as_bytes()),
                syntax: Box::leak(image.syntax.into_boxed_slice()),
                binding: Box::leak(image.binding.into_boxed_slice()),
                edits: Box::leak(image.edits.into_boxed_slice()),
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice(),
    );
    let source = catalog[0].source;
    tsc_syntax::library_image::register(catalog);
    let ordinary = InputFile::new("/earlier.d.ts", "interface OrdinaryFirst { value: number }")
        .with_js_doc_parsing_mode(Some(JSDocParsingMode::ParseForTypeErrors));
    let baked = InputFile::from_snapshot(
        file_name,
        TextSnapshot::from_text(source.clone(), Default::default()),
    )
    .with_js_doc_parsing_mode(Some(JSDocParsingMode::ParseForTypeErrors));
    let domain = IdentityDomain::new();
    let (mut sources, _) =
        parse_lib_sources(&[&ordinary, &baked], &domain, WorkerBudget::serial(), None);
    let first_count = tsc_binder::BindReservations::counts(sources[0].source())[0];
    let (_, first, first_data) = sources[0].bind_input();
    let (_, second, second_data) = sources[1].bind_input();
    assert_eq!(first.unwrap().symbols.range().start(), 0);
    let second = second.unwrap();
    assert_eq!(second.symbols.range().start(), first_count);
    assert!(first_data.is_none());
    assert_eq!(
        second_data.unwrap().symbols.base(),
        second.symbols.range().start()
    );

    static STARTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    fn started() {
        STARTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    tsc_program::set_thread_start_hook(started);
    let another = InputFile::from_snapshot(
        "bundled:///libs/lib.image-order-two.d.ts",
        TextSnapshot::from_text(catalog[1].source.clone(), Default::default()),
    )
    .with_js_doc_parsing_mode(Some(JSDocParsingMode::ParseForTypeErrors));
    let workers = WorkerBudget::new(std::num::NonZeroUsize::new(4).unwrap());
    let (mut completed, _) = parse_lib_sources(&[&baked, &another], &domain, workers, None);
    let binds = bind_lib_sources(&mut completed, &domain, workers, None);
    assert_eq!(binds.len(), 2);
    assert_eq!(
        STARTS.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "completed library prefixes must not start idle parse or bind workers"
    );
}
