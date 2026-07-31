//! A2 exact scope identity — the versioned canonical occurrence
//! encoder (measurement-integrity.md §3).
//!
//! Schema 2 identifies one oracle diagnostic occurrence as
//! `fixture + matrix_key + pass + file + start + length + code +
//! category + message-chain hash + related-information hash +
//! occurrence`. Rust and Node share this encoder version:
//! `crates/oracle/identity.mjs` mirrors every byte rule below and the
//! scope audit compares the two outputs over the committed vector file
//! and the corpus duplicate-bucket canaries. Changing any byte rule is
//! A2's one reviewed `input-schema-extension`, never a silent edit —
//! bump [`ENCODER_VERSION`] there and only there.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{ConformanceResult, GoldenDiag, GoldenMessageChain, GoldenRelated};
use crate::ratchet::sha256_hex;

/// Version of the canonical byte encoding shared with
/// `crates/oracle/identity.mjs`. The scope manifest pins it.
pub const ENCODER_VERSION: u32 = 1;

/// One exact oracle diagnostic occurrence (measurement-integrity.md
/// §3). Line and column are deliberately absent: they are redundant
/// review fields on the manifest entry, verified against `start`,
/// never identity.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ExactIdentity {
    pub fixture: String,
    pub matrix_key: String,
    pub pass: String,
    pub file: Option<String>,
    pub start: Option<u32>,
    pub length: Option<u32>,
    pub code: u32,
    pub category: String,
    pub chain_sha256: String,
    pub related_sha256: String,
    pub occurrence: u32,
}

impl ExactIdentity {
    /// Human-readable identity label for error messages.
    pub fn label(&self) -> String {
        format!(
            "{} [{}] {}/{:?}/{}#{}",
            self.fixture, self.matrix_key, self.pass, self.file, self.code, self.occurrence
        )
    }

    /// Canonical identity bytes (alphabetical field order, v1).
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(b'{');
        write_key(&mut out, "category", false);
        write_string(&mut out, &self.category);
        write_key(&mut out, "chain_sha256", true);
        write_string(&mut out, &self.chain_sha256);
        write_key(&mut out, "code", true);
        write_u32(&mut out, self.code);
        write_key(&mut out, "file", true);
        write_opt_string(&mut out, self.file.as_deref());
        write_key(&mut out, "fixture", true);
        write_string(&mut out, &self.fixture);
        write_key(&mut out, "length", true);
        write_opt_u32(&mut out, self.length);
        write_key(&mut out, "matrix_key", true);
        write_string(&mut out, &self.matrix_key);
        write_key(&mut out, "occurrence", true);
        write_u32(&mut out, self.occurrence);
        write_key(&mut out, "pass", true);
        write_string(&mut out, &self.pass);
        write_key(&mut out, "related_sha256", true);
        write_string(&mut out, &self.related_sha256);
        write_key(&mut out, "start", true);
        write_opt_u32(&mut out, self.start);
        out.push(b'}');
        out
    }

    pub fn sha256(&self) -> String {
        sha256_hex(&self.canonical_bytes())
    }
}

// ---------------------------------------------------------------------------
// Canonical byte writer (encoder v1)
// ---------------------------------------------------------------------------
// UTF-8, fixed (alphabetical) object field order, decimal integers,
// JSON string escaping, no insignificant whitespace, observable array
// order, `null` for absent optionals. Missing and empty stay distinct
// (`null` vs `""`/`[]`). The escape table matches the JSON standard's
// minimal set exactly — `"`, `\`, and control characters below 0x20
// (shorthand for \b \t \n \f \r, lowercase `\u00xx` otherwise);
// everything else is raw UTF-8. `identity.mjs` implements the same
// table explicitly rather than trusting `JSON.stringify`.

fn write_key(out: &mut Vec<u8>, key: &str, comma: bool) {
    if comma {
        out.push(b',');
    }
    write_string(out, key);
    out.push(b':');
}

fn write_string(out: &mut Vec<u8>, value: &str) {
    out.push(b'"');
    for ch in value.chars() {
        match ch {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{08}' => out.extend_from_slice(b"\\b"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\u{0c}' => out.extend_from_slice(b"\\f"),
            '\r' => out.extend_from_slice(b"\\r"),
            ch if (ch as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", ch as u32).as_bytes());
            }
            ch => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(value.to_string().as_bytes());
}

fn write_bool(out: &mut Vec<u8>, value: bool) {
    out.extend_from_slice(if value { b"true".as_slice() } else { b"false" });
}

fn write_null(out: &mut Vec<u8>) {
    out.extend_from_slice(b"null");
}

fn write_opt_string(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => write_string(out, value),
        None => write_null(out),
    }
}

fn write_opt_u32(out: &mut Vec<u8>, value: Option<u32>) {
    match value {
        Some(value) => write_u32(out, value),
        None => write_null(out),
    }
}

/// Message-chain bytes: recursively text, code, category, children
/// (measurement-integrity.md §3).
fn write_chain(out: &mut Vec<u8>, chain: &GoldenMessageChain) {
    out.push(b'{');
    write_key(out, "category", false);
    write_string(out, &chain.category);
    write_key(out, "code", true);
    write_u32(out, chain.code);
    write_key(out, "next", true);
    out.push(b'[');
    for (index, child) in chain.next.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        write_chain(out, child);
    }
    out.push(b']');
    write_key(out, "text", true);
    write_string(out, &chain.text);
    out.push(b'}');
}

pub(crate) fn chain_bytes(chain: &GoldenMessageChain) -> Vec<u8> {
    let mut out = Vec::new();
    write_chain(&mut out, chain);
    out
}

/// Related-information bytes: the same diagnostic fields plus the
/// normalized virtual file/span, in observable array order. A reorder
/// of this array changes the identity.
pub(crate) fn related_bytes(related: &[GoldenRelated]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(b'[');
    for (index, entry) in related.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        out.push(b'{');
        write_key(&mut out, "category", false);
        write_string(&mut out, &entry.category);
        write_key(&mut out, "chain", true);
        write_chain(&mut out, &entry.chain);
        write_key(&mut out, "code", true);
        write_u32(&mut out, entry.code);
        write_key(&mut out, "file", true);
        write_opt_string(&mut out, entry.file.as_deref());
        write_key(&mut out, "length", true);
        write_opt_u32(&mut out, entry.length);
        write_key(&mut out, "start", true);
        write_opt_u32(&mut out, entry.start);
        out.push(b'}');
    }
    out.push(b']');
    out
}

/// Complete canonical record bytes (excluding occurrence, which is
/// assigned FROM this ordering). Every stored golden field appears so
/// two records differing anywhere order deterministically.
pub(crate) fn record_bytes(diag: &GoldenDiag) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(b'{');
    write_key(&mut out, "category", false);
    write_string(&mut out, &diag.category);
    write_key(&mut out, "chain", true);
    write_chain(&mut out, &diag.chain);
    write_key(&mut out, "code", true);
    write_u32(&mut out, diag.code);
    write_key(&mut out, "col", true);
    write_opt_u32(&mut out, diag.col);
    write_key(&mut out, "file", true);
    write_opt_string(&mut out, diag.file.as_deref());
    write_key(&mut out, "length", true);
    write_opt_u32(&mut out, diag.length);
    write_key(&mut out, "line", true);
    write_opt_u32(&mut out, diag.line);
    write_key(&mut out, "pass", true);
    write_opt_string(&mut out, diag.pass.as_deref());
    write_key(&mut out, "related", true);
    out.extend_from_slice(&related_bytes(&diag.related));
    write_key(&mut out, "reports_deprecated", true);
    write_bool(&mut out, diag.reports_deprecated);
    write_key(&mut out, "reports_unnecessary", true);
    write_bool(&mut out, diag.reports_unnecessary);
    write_key(&mut out, "source", true);
    write_opt_string(&mut out, diag.source.as_deref());
    write_key(&mut out, "start", true);
    write_opt_u32(&mut out, diag.start);
    out.push(b'}');
    out
}

/// Assign exact identities to one case's oracle records, parallel to
/// the input order. `occurrence` is zero-based over records sharing
/// the identity tuple, numbered after stable sorting by complete
/// canonical record bytes — byte-identical neighbors retain oracle
/// input order, and same-tuple records that differ in a non-identity
/// field order deterministically by those bytes.
pub(crate) fn assign_case_identities(
    fixture: &str,
    matrix_key: &str,
    oracle: &[GoldenDiag],
) -> ConformanceResult<Vec<ExactIdentity>> {
    let mut tuples = Vec::with_capacity(oracle.len());
    for diag in oracle {
        let Some(pass) = diag.pass.as_deref() else {
            return Err(format!(
                "golden {fixture} [{matrix_key}] lacks pass provenance for code {}; exact \
                 scope identity requires schema-2 goldens (run `cargo xtask oracle-refresh`)",
                diag.code
            )
            .into());
        };
        tuples.push(ExactIdentity {
            fixture: fixture.to_owned(),
            matrix_key: matrix_key.to_owned(),
            pass: pass.to_owned(),
            file: diag.file.clone(),
            start: diag.start,
            length: diag.length,
            code: diag.code,
            category: diag.category.clone(),
            chain_sha256: sha256_hex(&chain_bytes(&diag.chain)),
            related_sha256: sha256_hex(&related_bytes(&diag.related)),
            occurrence: 0,
        });
    }

    let record_bytes = oracle.iter().map(record_bytes).collect::<Vec<_>>();
    let mut order = (0..oracle.len()).collect::<Vec<_>>();
    order.sort_by(|&a, &b| record_bytes[a].cmp(&record_bytes[b]).then(a.cmp(&b)));

    let mut counts: BTreeMap<ExactIdentity, u32> = BTreeMap::new();
    for index in order {
        let occurrence = {
            let counter = counts.entry(tuples[index].clone()).or_insert(0);
            let occurrence = *counter;
            *counter += 1;
            occurrence
        };
        tuples[index].occurrence = occurrence;
    }
    Ok(tuples)
}

/// Everything the cross-language check compares, per record in input
/// order. `identity.mjs` emits the identical JSON shape; the scope
/// audit fails on the first differing byte, naming the case and index.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CaseIdentityReport {
    pub record_canonical: Vec<String>,
    pub identities: Vec<ExactIdentity>,
    pub identity_canonical: Vec<String>,
    pub identity_sha256: Vec<String>,
}

pub(crate) fn case_identity_report(
    fixture: &str,
    matrix_key: &str,
    oracle: &[GoldenDiag],
) -> ConformanceResult<CaseIdentityReport> {
    let identities = assign_case_identities(fixture, matrix_key, oracle)?;
    let record_canonical = oracle
        .iter()
        .map(|diag| {
            String::from_utf8(record_bytes(diag)).expect("canonical record bytes are UTF-8")
        })
        .collect();
    let identity_canonical = identities
        .iter()
        .map(|identity| {
            String::from_utf8(identity.canonical_bytes()).expect("canonical bytes are UTF-8")
        })
        .collect::<Vec<_>>();
    let identity_sha256 = identities.iter().map(ExactIdentity::sha256).collect();
    Ok(CaseIdentityReport {
        record_canonical,
        identities,
        identity_canonical,
        identity_sha256,
    })
}

// ---------------------------------------------------------------------------
// Required adversarial tests (measurement-integrity.md §7, A2 identity
// row) and the canonical-encoder canaries.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::VectorFile;

    /// The committed cross-language canaries; scope audit feeds the
    /// same file through `crates/oracle/identity.mjs`.
    fn vectors() -> VectorFile {
        serde_json::from_str(include_str!("../identity-vectors-v1.json"))
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
}
