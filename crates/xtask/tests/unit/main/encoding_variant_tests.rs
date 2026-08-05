use std::collections::BTreeSet;

use super::distinct_encoding_variants;

fn texts(original: &str) -> Vec<String> {
    distinct_encoding_variants(original)
        .into_iter()
        .map(|(_, text)| text)
        .collect()
}

#[test]
fn skips_lf_and_no_bom_transforms_that_equal_the_baseline() {
    let original = "let value = 1;\n";
    assert_eq!(
        distinct_encoding_variants(original),
        vec![
            ("with-bom", "\u{feff}let value = 1;\n".to_owned()),
            ("crlf", "let value = 1;\r\n".to_owned()),
        ]
    );
}

#[test]
fn skips_bom_and_crlf_transforms_that_equal_the_baseline() {
    let original = "\u{feff}let value = 1;\r\n";
    assert_eq!(
        distinct_encoding_variants(original),
        vec![
            ("without-bom", "let value = 1;\r\n".to_owned()),
            ("lf", "\u{feff}let value = 1;\n".to_owned()),
        ]
    );
}

#[test]
fn deduplicates_all_equivalent_no_newline_transforms() {
    let original = "value";
    assert_eq!(
        distinct_encoding_variants(original),
        vec![("with-bom", "\u{feff}value".to_owned())]
    );
}

#[test]
fn preserves_every_distinct_mixed_eol_transform() {
    let original = "a\r\nb\n";
    assert_eq!(
        texts(original),
        vec![
            "\u{feff}a\r\nb\n".to_owned(),
            "a\nb\n".to_owned(),
            "a\r\nb\r\n".to_owned(),
        ]
    );
}

#[test]
fn keeps_exactly_the_distinct_transformed_text_set() {
    for original in [
        "",
        "value",
        "a\nb\n",
        "a\r\nb\r\n",
        "a\r\nb\n",
        "\u{feff}a\r\nb\r\n",
        "\u{feff}\u{feff}value",
    ] {
        let lf = original.replace("\r\n", "\n");
        let mut expected = [
            original.trim_start_matches('\u{feff}').to_owned(),
            format!("\u{feff}{}", original.trim_start_matches('\u{feff}')),
            lf.clone(),
            lf.replace('\n', "\r\n"),
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        expected.remove(original);

        let observed = texts(original);
        let observed_set = observed.iter().cloned().collect::<BTreeSet<_>>();
        assert_eq!(
            observed.len(),
            observed_set.len(),
            "duplicate output for {original:?}"
        );
        assert_eq!(observed_set, expected, "lost transform for {original:?}");
    }
}
