use super::*;
use crate::scope::VectorFile;

/// The committed cross-language canaries; scope audit feeds the
/// same file through `crates/oracle/identity.mjs`.
fn vectors() -> VectorFile {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/identity-vectors-v1.json"
    )))
    .expect("identity vector file parses")
}

fn vector_case(name: &str) -> (String, String, Vec<GoldenDiag>) {
    let case = vectors()
        .cases
        .into_iter()
        .find(|case| case.name == name)
        .unwrap_or_else(|| panic!("vector case {name} exists"));
    (case.fixture, case.matrix_key, case.records)
}

fn identities_of(name: &str) -> Vec<ExactIdentity> {
    let (fixture, matrix_key, records) = vector_case(name);
    assign_case_identities(&fixture, &matrix_key, &records).unwrap()
}

fn simple_diag() -> GoldenDiag {
    GoldenDiag {
        file: Some("a.ts".to_owned()),
        start: Some(15),
        length: Some(1),
        line: Some(2),
        col: Some(4),
        code: 2304,
        pass: Some("semantic".to_owned()),
        category: "error".to_owned(),
        chain: GoldenMessageChain {
            text: "Cannot find name 'x'.".to_owned(),
            code: 2304,
            category: "error".to_owned(),
            next: Vec::new(),
        },
        related: Vec::new(),
        reports_unnecessary: false,
        reports_deprecated: false,
        source: None,
    }
}

/// Encoder v1 byte-stability pin: these literals are the contract.
/// If this test fails, the encoding changed — that is A2's one
/// reviewed schema extension, never a silent edit.
#[test]
fn encoder_v1_bytes_are_pinned() {
    let diag = simple_diag();
    assert_eq!(
        String::from_utf8(record_bytes(&diag)).unwrap(),
        r#"{"category":"error","chain":{"category":"error","code":2304,"next":[],"text":"Cannot find name 'x'."},"code":2304,"col":4,"file":"a.ts","length":1,"line":2,"pass":"semantic","related":[],"reports_deprecated":false,"reports_unnecessary":false,"source":null,"start":15}"#
    );
    let identities =
        assign_case_identities("conformance/a.ts", "", std::slice::from_ref(&diag)).unwrap();
    assert_eq!(
        identities[0].chain_sha256,
        "0afd9675048f1dc17cdf48a89d98593e911b4423e475626e8e0d95dcf453c952"
    );
    assert_eq!(
        identities[0].related_sha256,
        "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945"
    );
    assert_eq!(
        String::from_utf8(identities[0].canonical_bytes()).unwrap(),
        r#"{"category":"error","chain_sha256":"0afd9675048f1dc17cdf48a89d98593e911b4423e475626e8e0d95dcf453c952","code":2304,"file":"a.ts","fixture":"conformance/a.ts","length":1,"matrix_key":"","occurrence":0,"pass":"semantic","related_sha256":"4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945","start":15}"#
    );
    assert_eq!(
        identities[0].sha256(),
        "8140c8bc8da41b5c4f9c9fc76f36dcc977ea557dfbfe114db833ac7c5d1a7a55"
    );
}

#[test]
fn string_escaping_matches_the_declared_table() {
    let mut out = Vec::new();
    write_string(
        &mut out,
        "quote\" back\\slash tab\t nul\0 esc\u{1b} unit\u{1f} del\u{7f}",
    );
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "\"quote\\\" back\\\\slash tab\\t nul\\u0000 esc\\u001b unit\\u001f del\u{7f}\""
    );
}

/// A2 identity row: same T0 key but different span/message must
/// NOT conflate — each occurrence gets its own identity.
#[test]
fn same_t0_key_different_message_stays_distinct() {
    let identities = identities_of("same-t0-key-different-message");
    assert_ne!(identities[0], identities[1]);
    assert_ne!(identities[0].chain_sha256, identities[1].chain_sha256);
    // Distinct tuples, so both are occurrence 0 of their own
    // identity — not occurrence 0/1 of a conflated one.
    assert_eq!(identities[0].occurrence, 0);
    assert_eq!(identities[1].occurrence, 0);
}

/// Canary: an observable reorder must change the identity.
#[test]
fn reordered_related_information_changes_identity() {
    let identities = identities_of("reordered-related-information");
    assert_ne!(identities[0].related_sha256, identities[1].related_sha256);
    assert_ne!(identities[0], identities[1]);
}

#[test]
fn reordered_chain_children_change_identity() {
    let identities = identities_of("nested-chains-child-order");
    assert_ne!(identities[0].chain_sha256, identities[1].chain_sha256);
    assert_ne!(identities[0], identities[1]);
}

/// Byte-identical neighbors retain oracle input order: the three
/// duplicates number 0, 1, 2 in input order.
#[test]
fn byte_identical_duplicates_number_in_input_order() {
    let identities = identities_of("byte-identical-duplicates");
    assert_eq!(
        identities.iter().map(|i| i.occurrence).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    let tuple = |i: &ExactIdentity| {
        (
            i.chain_sha256.clone(),
            i.related_sha256.clone(),
            i.start,
            i.code,
        )
    };
    assert_eq!(tuple(&identities[0]), tuple(&identities[1]));
}

/// Same identity tuple, different non-identity field: occurrence
/// assignment follows canonical record BYTE order, not input
/// order. `"source":"ts"` sorts before `"source":null` (0x22 <
/// 0x6e), so the second input record gets occurrence 0.
#[test]
fn same_tuple_records_number_by_canonical_byte_order() {
    let (fixture, matrix_key, records) = vector_case("same-tuple-different-source");
    assert_eq!(records[0].source, None);
    assert_eq!(records[1].source.as_deref(), Some("ts"));
    let identities = assign_case_identities(&fixture, &matrix_key, &records).unwrap();
    assert_eq!(identities[0].occurrence, 1);
    assert_eq!(identities[1].occurrence, 0);
}

/// Missing and empty remain distinct at every level: a file-less
/// global diagnostic and an empty-string file differ.
#[test]
fn null_and_empty_stay_distinct() {
    let identities = identities_of("null-vs-empty-and-global-diagnostic");
    assert_ne!(identities[0], identities[1]);
    assert_eq!(identities[0].file, None);
    assert_eq!(identities[1].file.as_deref(), Some(""));
    let (_, _, records) = vector_case("null-vs-empty-and-global-diagnostic");
    let null_bytes = String::from_utf8(record_bytes(&records[0])).unwrap();
    assert!(null_bytes.contains(r#""file":null"#), "{null_bytes}");
    let empty_bytes = String::from_utf8(record_bytes(&records[1])).unwrap();
    assert!(empty_bytes.contains(r#""file":"""#), "{empty_bytes}");
}

#[test]
fn identity_requires_pass_provenance() {
    let mut diag = simple_diag();
    diag.pass = None;
    let error = assign_case_identities("conformance/a.ts", "", &[diag])
        .unwrap_err()
        .to_string();
    assert!(error.contains("pass provenance"), "{error}");
}

/// Every vector case round-trips through the report constructor
/// with unique identities — the same property the audit enforces
/// over the corpus duplicate-bucket canaries.
#[test]
fn vector_cases_assign_unique_identities() {
    for case in vectors().cases {
        let report = case_identity_report(&case.fixture, &case.matrix_key, &case.records)
            .unwrap_or_else(|err| panic!("vector {}: {err}", case.name));
        let unique = report
            .identities
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            unique.len(),
            report.identities.len(),
            "vector {} assigns duplicate identities",
            case.name
        );
    }
}
