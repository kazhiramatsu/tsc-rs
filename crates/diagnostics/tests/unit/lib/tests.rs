use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn chain(code: u32, text: &str) -> MessageChain {
    MessageChain {
        code,
        category: DiagnosticCategory::Error,
        text: text.into(),
        next_present: false,
        next: Vec::new(),
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
fn diagnostic_path_and_related_names_keep_units_and_use_utf16_order() {
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
            vec![0xd800, 0xdc00],
            vec![0xd801],
            vec![0xdc00],
            vec![0xe000],
            vec![0xfffd]
        ]
    );

    let mut left = diagnostic(Some("display.ts"), Some(0), 1000, "same");
    let mut right = left.clone();
    left.file_path = Some(JsString::from_code_units(&[0xd800, 0xdc00]));
    right.file_path = Some(JsString::from_code_units(&[0xe000]));
    assert_eq!(compare_diagnostics(&left, &right), Ordering::Less);
    assert_eq!(left.file_path.cmp(&right.file_path), Ordering::Greater);
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
fn present_empty_related_information_sorts_before_absent_and_wins_dedupe() {
    let absent = diagnostic(Some("a.ts"), Some(0), 1005, "';' expected.");
    let mut present_empty = absent.clone();
    present_empty.related_information_present = true;

    assert_eq!(compare_diagnostics(&present_empty, &absent), Ordering::Less);
    assert_eq!(
        compare_diagnostics(&absent, &present_empty),
        Ordering::Greater
    );

    let mut diagnostics = vec![absent, present_empty];
    sort_and_dedupe_diagnostics(&mut diagnostics);
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].related_information_present);
    assert!(diagnostics[0].related.is_empty());
}

#[test]
fn diagnostic_sort_uses_javascript_utf16_code_unit_order() {
    let astral = "\u{1f600}";
    let private_use = "\u{e000}";
    assert_eq!(
        compare_strings_case_sensitive(astral, private_use),
        Ordering::Less
    );

    let astral_file = diagnostic(Some(astral), Some(0), 1000, "same");
    let private_use_file = diagnostic(Some(private_use), Some(0), 1000, "same");
    assert_eq!(
        compare_diagnostics(&astral_file, &private_use_file),
        Ordering::Less
    );

    let mut astral_head = diagnostic(None, None, 2000, "same raw head");
    astral_head.canonical_head = Some(CanonicalHead {
        code: 1000,
        text: astral.into(),
    });
    let mut private_use_head = diagnostic(None, None, 2000, "same raw head");
    private_use_head.canonical_head = Some(CanonicalHead {
        code: 1000,
        text: private_use.into(),
    });
    assert_eq!(
        compare_diagnostics(&astral_head, &private_use_head),
        Ordering::Less
    );

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
        Ordering::Less
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
        Ordering::Less
    );

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
        Ordering::Less
    );
}

#[test]
fn present_empty_message_chain_sorts_before_absent_and_wins_dedupe() {
    let absent = diagnostic(None, None, 1000, "same");
    let mut present_empty = absent.clone();
    present_empty.message.next_present = true;

    assert_eq!(compare_diagnostics(&present_empty, &absent), Ordering::Less);
    assert_eq!(
        compare_diagnostics(&absent, &present_empty),
        Ordering::Greater
    );

    let mut diagnostics = vec![absent, present_empty];
    sort_and_dedupe_diagnostics(&mut diagnostics);
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.next_present);
    assert!(diagnostics[0].message.next.is_empty());
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
fn config_source_path_orders_before_program_sources_without_changing_display_name() {
    // TypeScript 6.0.3 config-none-deprecated-blocked command: the parsed
    // config SourceFile.path is "", while its fileName is the full path.
    let config =
        diagnostic(Some("/project/tsconfig.json"), Some(29), 5107, "deprecated").with_file_path("");
    let source = diagnostic(Some("/project/src/main.ts"), Some(0), 1148, "module none");
    let global = diagnostic(None, None, 5000, "global");
    let mut diagnostics = vec![source, config.clone(), global, config];
    sort_and_dedupe_diagnostics(&mut diagnostics);
    assert_eq!(diagnostics.len(), 3);
    assert_eq!(
        diagnostics.iter().map(Diagnostic::code).collect::<Vec<_>>(),
        [5000, 5107, 1148]
    );
    assert_eq!(
        diagnostics[1].file_name.as_ref().and_then(JsString::as_str),
        Some("/project/tsconfig.json")
    );
}
