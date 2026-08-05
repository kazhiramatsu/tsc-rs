use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn chain(code: u32, text: &str) -> MessageChain {
    MessageChain {
        code,
        category: DiagnosticCategory::Error,
        text: text.to_owned(),
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
    assert_eq!(diagnostics[1].file_name.as_deref(), Some("a.ts"));
    assert_eq!(diagnostics[2].file_name.as_deref(), Some("b.ts"));
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
        text: astral.to_owned(),
    });
    let mut private_use_head = diagnostic(None, None, 2000, "same raw head");
    private_use_head.canonical_head = Some(CanonicalHead {
        code: 1000,
        text: private_use.to_owned(),
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
        file_name: Some(astral.to_owned()),
        start: Some(0),
        length: Some(1),
        message: chain(1000, "same"),
    });
    let mut private_use_related_file = diagnostic(None, None, 1000, "same");
    private_use_related_file.related.push(RelatedInfo {
        file_name: Some(private_use.to_owned()),
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
