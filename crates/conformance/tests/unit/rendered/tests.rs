use super::*;
use std::path::Path;
use tsc_oracle::{OracleDiag, OracleMessageChain, OracleRelated};

fn oracle_chain(code: u32, category: &str, text: &str) -> OracleMessageChain {
    OracleMessageChain {
        text: text.to_owned(),
        code,
        category: category.to_owned(),
        next: Vec::new(),
    }
}

fn oracle_diag(
    file: Option<&str>,
    start: Option<u32>,
    length: Option<u32>,
    code: u32,
    category: &str,
    text: &str,
) -> OracleDiag {
    OracleDiag {
        file: file.map(str::to_owned),
        start,
        length,
        code,
        pass: None,
        category: category.to_owned(),
        chain: oracle_chain(code, category, text),
        related: Vec::new(),
        related_information_present: false,
        reports_unnecessary: false,
        reports_deprecated: false,
        source: None,
    }
}

#[test]
fn schema3_hash_contract_is_sha256_of_exact_rendered_utf8() {
    assert_eq!(
        rendered_sha256("error TS1: x\n"),
        "70897b64d4f29f0963accdf7d4b618f72f1313eb86d9f68fa6208815ebd8eb1d"
    );
    assert!(valid_sha256(
        "70897b64d4f29f0963accdf7d4b618f72f1313eb86d9f68fa6208815ebd8eb1d"
    ));
    assert!(!valid_sha256("CBF29CE484222325"));
}

#[test]
fn schema2_legacy_hashes_never_become_t4_pins() {
    let observed = "a".repeat(64);
    assert_eq!(
        evaluate_golden_oracle_pin(2, "cbf29ce484222325", &observed),
        (None, None)
    );
    assert_eq!(
        evaluate_golden_oracle_pin(2, &observed, &observed),
        (None, None),
        "even a SHA-256-shaped schema-2 value is legacy evidence"
    );
    assert_eq!(
        evaluate_golden_oracle_pin(3, &observed, &observed),
        (Some(observed.clone()), Some(true))
    );
    assert_eq!(
        evaluate_golden_oracle_pin(3, &"b".repeat(64), &observed),
        (Some("b".repeat(64)), Some(false))
    );
    assert_eq!(
        evaluate_golden_oracle_pin(3, "not-a-sha256", &observed),
        (Some("not-a-sha256".to_owned()), Some(false))
    );
}

#[test]
fn schema3_empty_related_metadata_does_not_change_structured_oracle_bytes() {
    let file_texts = BTreeMap::new();
    let absent = oracle_diag(
        None,
        None,
        None,
        2769,
        "error",
        "No overload matches this call.",
    );
    let mut present = absent.clone();
    present.related_information_present = true;
    let absent_structured = GoldenDiag::from_oracle(&absent, &file_texts);
    let oracle = vec![GoldenDiag::from_oracle(&present, &file_texts)];
    assert_eq!(oracle[0], absent_structured);
    let mut case = super::super::GoldenCase {
        matrix_key: String::new(),
        tsrs: Vec::new(),
        oracle,
        oracle_empty_related_information: Vec::new(),
        tsrs_cli_hash: String::new(),
        oracle_cli_hash: "a".repeat(64),
    };
    let structured_before = serde_json::to_vec(&case.oracle).unwrap();
    assert!(!serde_json::to_string(&case)
        .unwrap()
        .contains("oracle_empty_related_information"));

    case.oracle_empty_related_information = vec![0];
    assert_eq!(serde_json::to_vec(&case.oracle).unwrap(), structured_before);
    assert!(serde_json::to_string(&case)
        .unwrap()
        .contains(r#""oracle_empty_related_information":[0]"#));
}

#[test]
fn empty_related_metadata_rehydrates_the_formatter_presence_bit() {
    let file_texts = BTreeMap::new();
    let records = vec![GoldenDiag::from_oracle(
        &oracle_diag(
            None,
            None,
            None,
            2769,
            "error",
            "No overload matches this call.",
        ),
        &file_texts,
    )];
    let absent = diagnostics_from_golden(&records).unwrap();
    let present =
        diagnostics_from_golden_with_empty_related_information(&records, &BTreeSet::from([0]))
            .unwrap();
    assert!(!absent[0].related_information_present);
    assert!(present[0].related_information_present);

    let host = FormatDiagnosticsHost::new("/workspace", &file_texts);
    assert_eq!(
        format_sorted_diagnostics_with_context(&absent, &host).unwrap(),
        "error TS2769: No overload matches this call.\n"
    );
    assert_eq!(
        format_sorted_diagnostics_with_context(&present, &host).unwrap(),
        "error TS2769: No overload matches this call.\n\n"
    );
}

#[test]
fn empty_related_metadata_is_validated_and_projection_keeps_original_indices() {
    let file_texts = BTreeMap::new();
    let first = GoldenDiag::from_oracle(
        &oracle_diag(None, None, None, 1, "error", "first"),
        &file_texts,
    );
    let second = GoldenDiag::from_oracle(
        &oracle_diag(None, None, None, 2, "error", "second"),
        &file_texts,
    );
    let records = vec![first, second];
    let indices = validate_empty_related_information(&records, &[1], "test").unwrap();
    let projected =
        diagnostics_from_indexed_golden_refs(records.iter().enumerate().skip(1), &indices).unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].code(), 2);
    assert!(projected[0].related_information_present);

    let duplicate = validate_empty_related_information(&records, &[1, 1], "test")
        .unwrap_err()
        .to_string();
    assert!(duplicate.contains("strictly increasing"), "{duplicate}");
    let out_of_range = validate_empty_related_information(&records, &[2], "test")
        .unwrap_err()
        .to_string();
    assert!(out_of_range.contains("out of range"), "{out_of_range}");
    let mut non_empty = records.clone();
    non_empty[0].related.push(super::super::GoldenRelated {
        file: None,
        start: None,
        length: None,
        code: 1,
        category: "message".to_owned(),
        chain: GoldenMessageChain {
            text: "related".to_owned(),
            code: 1,
            category: "message".to_owned(),
            next: Vec::new(),
        },
    });
    let points_to_rows = validate_empty_related_information(&non_empty, &[0], "test")
        .unwrap_err()
        .to_string();
    assert!(
        points_to_rows.contains("serialized related rows"),
        "{points_to_rows}"
    );

    assert_eq!(
        effective_oracle_empty_related_information(2, &records, &[], &[1], "test").unwrap(),
        BTreeSet::from([1])
    );
    let schema2_metadata =
        effective_oracle_empty_related_information(2, &records, &[1], &[1], "test")
            .unwrap_err()
            .to_string();
    assert!(schema2_metadata.contains("schema-2"), "{schema2_metadata}");
    let stale = effective_oracle_empty_related_information(3, &records, &[], &[1], "test")
        .unwrap_err()
        .to_string();
    assert!(stale.contains("drifted"), "{stale}");
}

#[test]
fn first_difference_pins_newline_sensitive_report_bytes() {
    assert_eq!(
        serde_json::to_string(&first_rendered_difference(
            "a.ts:1:1 - error TS1: x\n",
            "a.ts:1:1 - error TS1: x\r\n"
        ))
        .unwrap(),
        r#"{"line":1,"oracle":"a.ts:1:1 - error TS1: x","tsrs":"a.ts:1:1 - error TS1: x\r"}"#
    );
}

#[test]
fn rust_and_vendored_node_pin_every_formatter_structure() {
    let temp = super::super::temp_root("tsc-rs-render-vector");
    fs::create_dir_all(&temp).unwrap();
    let program_json = temp.join("program.json");
    fs::write(
        &program_json,
        r#"{
  "schema": 1,
  "cwd": "/workspace/src",
  "options": {"noLib": true},
  "libs": [],
  "files": [
    {"name": "main.ts", "textB64": "Y29uc3QJZmFjZSA9ICLwn5iAIjsNCmINCmMNCmQNCmUNCmYNCg=="},
    {"name": "origin.ts", "textB64": "ZXhwb3J0IGNvbnN0IG9yaWdpbiA9IDE7Cg=="},
    {"name": "../z.ts", "textB64": "ego="}
  ],
  "matrixKey": ""
}
"#,
    )
    .unwrap();

    let mut error = oracle_diag(Some("main.ts"), Some(14), Some(2), 2322, "error", "Head");
    // The outer Diagnostic header owns these bytes; a chain root
    // may carry different metadata and must not replace it.
    error.chain.code = 9999;
    error.chain.category = "message".to_owned();
    error.chain.next = vec![oracle_chain(2322, "error", "Child")];
    error.related = vec![OracleRelated {
        file: Some("origin.ts".to_owned()),
        start: Some(13),
        length: Some(6),
        code: 2728,
        category: "message".to_owned(),
        chain: oracle_chain(9998, "warning", "Origin"),
    }];
    let mut suggestion = oracle_diag(
        Some("main.ts"),
        Some(20),
        Some(13),
        80001,
        "suggestion",
        "Hint",
    );
    suggestion.pass = Some("suggestion".to_owned());
    suggestion.reports_unnecessary = true;
    let z = oracle_diag(Some("../z.ts"), Some(0), Some(1), 1, "error", "Z");
    let fileless = oracle_diag(None, None, None, 999, "message", "Global");
    let records = vec![z, suggestion.clone(), error, fileless, suggestion];

    let pool = OraclePool::new_render_only();
    let node = pool.render_records(&program_json, &records).unwrap();
    let file_texts = BTreeMap::from([
        (
            "main.ts".to_owned(),
            "const\tface = \"😀\";\r\nb\r\nc\r\nd\r\ne\r\nf\r\n".to_owned(),
        ),
        (
            "origin.ts".to_owned(),
            "export const origin = 1;\n".to_owned(),
        ),
        ("../z.ts".to_owned(), "z\n".to_owned()),
    ]);
    let golden = records
        .iter()
        .map(|record| GoldenDiag::from_oracle(record, &file_texts))
        .collect::<Vec<_>>();
    let rust = tsc_diagnostics::format_diagnostics_with_context(
        &diagnostics_from_golden(&golden).unwrap(),
        &FormatDiagnosticsHost::new("/workspace/src", &file_texts),
    )
    .unwrap();

    assert_eq!(rust, node);
    let sorted_node = pool.render_sorted_records(&program_json, &records).unwrap();
    let sorted_rust = format_sorted_diagnostics_with_context(
        &diagnostics_from_golden(&golden).unwrap(),
        &FormatDiagnosticsHost::new("/workspace/src", &file_texts),
    )
    .unwrap();
    assert_eq!(sorted_rust, sorted_node);
    assert_ne!(
        sorted_node, node,
        "already-sorted entry point must preserve input order and duplicates"
    );
    assert_eq!(
        rendered_sha256(&node),
        "849163464e947f86eaf4b616e1280c4c918983968d355eda4162b2b137c37713"
    );
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn focused_schema3_t4_report_stays_report_only_and_checks_active_pins() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let report_temp = super::super::temp_root("tsc-rs-rendered-output-report");
    let report = run_t4_report(&T4ReportOptions {
        workspace,
        limit: None,
        files: vec![PathBuf::from(
            "ts-tests/tests/cases/conformance/decorators/missingDecoratorType.ts",
        )],
    })
    .unwrap();

    assert!(
        !report_temp.exists(),
        "focused report must remove its temporary program JSON tree"
    );
    assert_eq!(report.schema, 2);
    assert_eq!(report.status, "report-only");
    assert_eq!(report.fixtures, 1);
    assert_eq!(report.cases, 2);
    assert_eq!(report.schema_3_pinned_cases, 2);
    assert_eq!(report.matched_cases, 2);
    assert_eq!(report.mismatched_cases, 0);
    assert_eq!(report.oracle_pin_failures, 0);
    assert_eq!(report.rust_formatter_failures, 0);
    assert!(report.cases_detail.iter().all(|case| {
        case.golden_schema == 3
            && case.golden_oracle_cli_hash.as_deref() == Some(case.oracle_full_cli_hash.as_str())
            && case.oracle_pin_matches == Some(true)
            && case.rust_formatter_matches_oracle
            && case.oracle_full_cli_hash == case.rust_oracle_full_cli_hash
            && valid_sha256(&case.oracle_cli_hash)
            && valid_sha256(&case.tsrs_cli_hash)
    }));
}
