use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn chain(code: u32, text: &str) -> MessageChain {
    MessageChain {
        code,
        category: DiagnosticCategory::Error,
        text: text.into(),
        key: None,
        args: Vec::new(),
        next_present: false,
        next: Vec::new(),
        related: Vec::new(),
        repopulate: None,
        without_location: false,
    }
}

fn diagnostic(file_name: Option<&str>, start: Option<u32>, code: u32, text: &str) -> Diagnostic {
    Diagnostic::new(
        file_name.map(str::to_owned),
        start,
        Some(1),
        chain(code, text),
    )
}

#[test]
fn looks_up_generated_message_by_code() {
    let message = by_code(1005).expect("diagnostic 1005 exists");
    assert_eq!(message.text, "'{0}' expected.");
    assert_eq!(message.category, DiagnosticCategory::Error);
}

#[test]
fn formats_placeholder_arguments() {
    assert_eq!(
        format_message("'{0}' expected.", &args(&[";"])),
        "';' expected."
    );
    assert_eq!(
        format_message("{1} before {0}", &args(&["b", "a"])),
        "a before b"
    );
}

#[test]
fn diagnostic_arguments_preserve_units_and_join_surrogates() {
    let lead = JsString::from_code_units(&[0xD800]);
    let trail = JsString::from_code_units(&[0xDC00]);
    assert_eq!(
        format_message_value("{1}{0}", &[trail.clone(), lead.clone()]).as_bytes(),
        "\u{10000}".as_bytes()
    );
    let message = MessageChain::new_js(&gen::_0_expected, &[lead]);
    assert_eq!(
        message.text.to_utf16(),
        [vec![0x27, 0xD800], "' expected.".encode_utf16().collect()].concat()
    );
    assert_eq!(
        format_message_value("{0}/{1}", &[trail, JsString::from("\u{FFFD}")]).to_utf16(),
        [0xDC00, 0x2F, 0xFFFD]
    );
}

#[test]
fn surrogate_messages_remain_distinct_through_sort_and_dedupe() {
    let mut diagnostics = [0xFFFD, 0xD801, 0xDC00, 0xD800, 0xD800]
        .into_iter()
        .map(|unit| {
            let mut diagnostic = diagnostic(None, None, 1000, "");
            diagnostic.message.text = JsString::from_code_units(&[unit]);
            diagnostic
        })
        .collect::<Vec<_>>();
    sort_and_dedupe_diagnostics(&mut diagnostics);
    assert_eq!(
        diagnostics
            .iter()
            .map(|d| d.message.text.to_utf16())
            .collect::<Vec<_>>(),
        [vec![0xD800], vec![0xD801], vec![0xDC00], vec![0xFFFD]]
    );
}

#[test]
fn diagnostic_path_and_related_names_keep_units_and_use_byte_order() {
    let names = [
        vec![0xfffd],
        vec![0xd801],
        vec![0xdc00],
        vec![0xd800],
        vec![0xd800, 0xdc00],
        vec![0xe000],
    ];
    let mut diagnostics = names
        .iter()
        .map(|name| {
            Diagnostic::new_js(
                Some(JsString::from_code_units(name)),
                Some(0),
                Some(1),
                chain(1000, "same"),
            )
        })
        .collect::<Vec<_>>();
    diagnostics.push(diagnostics[3].clone());
    sort_and_dedupe_diagnostics(&mut diagnostics);
    assert_eq!(
        diagnostics
            .iter()
            .map(|d| d.file_name.as_ref().unwrap().to_utf16())
            .collect::<Vec<_>>(),
        [
            vec![0xd800],
            vec![0xd801],
            vec![0xdc00],
            vec![0xe000],
            vec![0xfffd],
            vec![0xd800, 0xdc00]
        ]
    );

    let mut left = diagnostic(Some("display.ts"), Some(0), 1000, "same");
    let mut right = left.clone();
    left.file_name = Some(JsString::from_code_units(&[0xd800, 0xdc00]));
    right.file_name = Some(JsString::from_code_units(&[0xe000]));
    assert_eq!(compare_diagnostics(&left, &right), Ordering::Greater);
    assert_eq!(left.file_name.cmp(&right.file_name), Ordering::Greater);
    let related = |unit| RelatedInfo {
        file_name: Some(JsString::from_code_units(&[unit])),
        start: Some(0),
        length: Some(1),
        message: chain(1000, "same"),
    };
    assert_eq!(
        compare_related_info(&related(0xd800), &related(0xd801)),
        Ordering::Less
    );
    assert_ne!(related(0xd800), related(0xfffd));
}

#[test]
fn sorts_and_deduplicates_adjacent_diagnostics() {
    let duplicate = diagnostic(Some("b.ts"), Some(1), 1005, "';' expected.");
    let mut diagnostics = vec![
        diagnostic(Some("a.ts"), Some(4), 1003, "Identifier expected."),
        duplicate.clone(),
        diagnostic(None, None, 1002, "Unterminated string literal."),
        duplicate,
    ];

    sort_and_dedupe_diagnostics(&mut diagnostics);

    assert_eq!(diagnostics.len(), 3);
    assert_eq!(diagnostics[0].file_name, None);
    assert_eq!(
        diagnostics[1].file_name.as_ref().and_then(JsString::as_str),
        Some("a.ts")
    );
    assert_eq!(
        diagnostics[2].file_name.as_ref().and_then(JsString::as_str),
        Some("b.ts")
    );
}

#[test]
fn diagnostics_that_differ_only_in_their_chains_are_both_kept() {
    // tsgo's EqualDiagnosticsNoRelatedInfo compares the message chain (each
    // entry's code and arguments): checkInheritedPropertiesAreIdentical
    // reports one TS2320 per property that is not identical, at one position
    // with one head message.
    let reported = |property: &str| {
        Diagnostic::new(
            Some("a.ts".to_owned()),
            Some(3),
            Some(1),
            MessageChain::new(
                &gen::Interface_0_cannot_simultaneously_extend_types_1_and_2,
                &args(&["A", "B", "C"]),
            )
            .with_next(vec![MessageChain::new(
                &gen::Named_property_0_of_types_1_and_2_are_not_identical,
                &args(&[property, "B", "C"]),
            )]),
        )
    };
    let first = reported("x");
    let second = reported("y");
    let same_as_first = first.clone();

    let mut diagnostics = vec![second.clone(), first.clone(), same_as_first];
    sort_and_dedupe_diagnostics(&mut diagnostics);

    assert_eq!(diagnostics, vec![first, second]);
}

#[test]
fn a_did_you_mean_diagnostic_sorts_by_its_own_code_and_text() {
    // tsgo has no canonical diagnostic: a TS2552 at the same span as a TS2304
    // is a different diagnostic, ordered by code, and both are kept. tsc 6.0
    // sorted and deduplicated the 2552 through its plain 2304 form.
    let plain = diagnostic(Some("a.ts"), Some(7), 2304, "Cannot find name 'foo'.");
    let suggestion = diagnostic(
        Some("a.ts"),
        Some(7),
        2552,
        "Cannot find name 'foo'. Did you mean 'for'?",
    );

    let mut diagnostics = vec![suggestion.clone(), plain.clone()];
    sort_and_dedupe_diagnostics(&mut diagnostics);

    assert_eq!(diagnostics, vec![plain, suggestion]);
}

#[test]
fn present_empty_and_absent_related_information_compare_equal() {
    // tsgo has no absent list: no related information is none.
    let absent = diagnostic(Some("a.ts"), Some(0), 1005, "';' expected.");
    let mut present_empty = absent.clone();
    present_empty.related_information_present = true;

    assert_eq!(
        compare_diagnostics(&present_empty, &absent),
        Ordering::Equal
    );
    let mut diagnostics = vec![absent, present_empty];
    sort_and_dedupe_diagnostics(&mut diagnostics);
    assert_eq!(diagnostics.len(), 1);
    assert!(!diagnostics[0].related_information_present);
}

#[test]
fn diagnostic_sort_uses_go_byte_order() {
    // tsgo compares names and texts with Go's strings.Compare (UTF-8 bytes):
    // an astral character sorts after a private-use one.
    let astral = "\u{1f600}";
    let private_use = "\u{e000}";

    let astral_file = diagnostic(Some(astral), Some(0), 1000, "same");
    let private_use_file = diagnostic(Some(private_use), Some(0), 1000, "same");
    assert_eq!(
        compare_diagnostics(&astral_file, &private_use_file),
        Ordering::Greater
    );

    // A chain entry orders by its arguments, not its text.
    let mut astral_child = diagnostic(None, None, 1000, "same");
    astral_child.message.next_present = true;
    astral_child.message.next.push(chain(1000, astral));
    let mut private_use_child = diagnostic(None, None, 1000, "same");
    private_use_child.message.next_present = true;
    private_use_child
        .message
        .next
        .push(chain(1000, private_use));
    assert_eq!(
        compare_diagnostics(&astral_child, &private_use_child),
        Ordering::Equal
    );

    let mut astral_related_file = diagnostic(None, None, 1000, "same");
    astral_related_file.related.push(RelatedInfo {
        file_name: Some(astral.into()),
        start: Some(0),
        length: Some(1),
        message: chain(1000, "same"),
    });
    let mut private_use_related_file = diagnostic(None, None, 1000, "same");
    private_use_related_file.related.push(RelatedInfo {
        file_name: Some(private_use.into()),
        start: Some(0),
        length: Some(1),
        message: chain(1000, "same"),
    });
    assert_eq!(
        compare_diagnostics(&astral_related_file, &private_use_related_file),
        Ordering::Greater
    );

    // A message without a catalog key is identified by its text.
    let mut astral_related_text = diagnostic(None, None, 1000, "same");
    astral_related_text.related.push(RelatedInfo {
        file_name: None,
        start: None,
        length: None,
        message: chain(1000, astral),
    });
    let mut private_use_related_text = diagnostic(None, None, 1000, "same");
    private_use_related_text.related.push(RelatedInfo {
        file_name: None,
        start: None,
        length: None,
        message: chain(1000, private_use),
    });
    assert_eq!(
        compare_diagnostics(&astral_related_text, &private_use_related_text),
        Ordering::Greater
    );
}

#[test]
fn present_empty_and_absent_message_chains_compare_equal() {
    let absent = diagnostic(None, None, 1000, "same");
    let mut present_empty = absent.clone();
    present_empty.message.next_present = true;

    assert_eq!(
        compare_diagnostics(&present_empty, &absent),
        Ordering::Equal
    );
    let mut diagnostics = vec![absent, present_empty];
    sort_and_dedupe_diagnostics(&mut diagnostics);
    assert_eq!(diagnostics.len(), 1);
    assert!(!diagnostics[0].message.next_present);
}

#[test]
fn diagnostics_at_one_location_order_by_key_then_arguments() {
    // tsgo CompareDiagnostics: the message key and then the arguments, not
    // the text. TS2883 shows its third argument before its second, so its
    // text and its arguments order these two the other way round.
    let reference = |module: &str, name: &str| {
        let mut diagnostic = Diagnostic::new(
            Some("main.ts".to_owned()),
            Some(0),
            Some(5),
            MessageChain::new(
                &gen::The_inferred_type_of_0_cannot_be_named_without_a_reference_to_2_from_1_This_is_likely_not_portable_A_type_annotation_is_necessary,
                &args(&["value", module, name]),
            ),
        );
        diagnostic.related_information_present = false;
        diagnostic
    };
    let zebra = reference("a/node_modules/m1", "Zebra");
    let apple = reference("c/node_modules/m2", "Apple");
    assert_eq!(compare_diagnostics(&zebra, &apple), Ordering::Less);
    let mut diagnostics = vec![apple.clone(), zebra.clone()];
    sort_and_dedupe_diagnostics(&mut diagnostics);
    assert_eq!(diagnostics, vec![zebra, apple]);
}

#[test]
fn longer_chains_and_more_related_information_sort_first() {
    let plain = diagnostic(Some("a.ts"), Some(0), 2322, "same");
    let mut chained = plain.clone();
    chained.message.next_present = true;
    chained.message.next.push(chain(2322, "inner"));
    assert_eq!(compare_diagnostics(&chained, &plain), Ordering::Less);

    let mut related = plain.clone();
    related.related.push(RelatedInfo {
        file_name: Some("b.ts".into()),
        start: Some(0),
        length: Some(1),
        message: chain(2728, "declared here"),
    });
    assert_eq!(compare_diagnostics(&related, &plain), Ordering::Less);
}

#[test]
fn diagnostic_new_propagates_generated_flags_and_sidecars() {
    let unnecessary = Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(
            &gen::Left_side_of_comma_operator_is_unused_and_has_no_side_effects,
            &[],
        ),
    );
    assert_eq!(unnecessary.reports_unnecessary, Some(true));
    assert_eq!(unnecessary.reports_deprecated, None);
    assert_eq!(unnecessary.source, None);

    let deprecated = Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(&gen::_0_is_deprecated, &args(&["old"])),
    )
    .with_source("typescript")
    .with_reports_unnecessary(Some(false));
    assert_eq!(deprecated.reports_unnecessary, Some(false));
    assert_eq!(deprecated.reports_deprecated, Some(true));
    assert_eq!(deprecated.source.as_deref(), Some("typescript"));
}

#[test]
fn config_diagnostics_order_by_the_config_file_name() {
    // tsgo's CompareDiagnostics compares File().FileName()
    // (ast/diagnostic.go:390-395), so a config file's diagnostics sort among
    // the sources by its name; tsc 6.0 compared the empty SourceFile.path of
    // a parsed config and put them first.
    let config = diagnostic(Some("/project/tsconfig.json"), Some(29), 5107, "deprecated");
    let source = diagnostic(Some("/project/src/main.ts"), Some(0), 1148, "module none");
    let global = diagnostic(None, None, 5000, "global");
    let mut diagnostics = vec![config.clone(), source, global, config];
    sort_and_dedupe_diagnostics(&mut diagnostics);
    assert_eq!(
        diagnostics.iter().map(Diagnostic::code).collect::<Vec<_>>(),
        [5000, 1148, 5107]
    );
}

#[test]
fn diagnostics_differing_only_by_related_information_merge_like_tsgo() {
    // tsgo compactAndMergeRelatedInfos (compiler/program.go:1657-1688): one
    // diagnostic keeps the sorted, deduplicated related rows of the run.
    let related = |start: u32| RelatedInfo {
        file_name: Some("b.ts".into()),
        start: Some(start),
        length: Some(1),
        message: chain(6203, "'x' was also declared here."),
    };
    let with_related = |starts: &[u32]| {
        let mut diagnostic = diagnostic(Some("a.ts"), Some(0), 2451, "Cannot redeclare.");
        diagnostic.related_information_present = true;
        diagnostic.related = starts.iter().map(|&start| related(start)).collect();
        diagnostic
    };
    let mut diagnostics = vec![
        with_related(&[8]),
        with_related(&[4]),
        with_related(&[8]),
        diagnostic(Some("a.ts"), Some(0), 2451, "Another message."),
    ];
    sort_and_dedupe_diagnostics(&mut diagnostics);
    let starts = diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.message.text.to_string_lossy().into_owned(),
                diagnostic
                    .related
                    .iter()
                    .map(|related| related.start)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        starts,
        [
            ("Another message.".to_owned(), vec![]),
            ("Cannot redeclare.".to_owned(), vec![Some(4), Some(8)]),
        ]
    );
}
