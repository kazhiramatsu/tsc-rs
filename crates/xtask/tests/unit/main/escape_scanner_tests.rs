use super::*;

fn scan(text: &str) -> Vec<EscapeSite> {
    scan_escape_text(Path::new("test.rs"), text).expect("escape scan")
}

#[test]
fn plain_reason_parses_its_owner() {
    let sites = scan(r#"return Err(Unsupported::new("mapped types (M4-end sweep 5.8)"));"#);
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].owner, Some(StageKey(4, 8, u8::MAX)));
}

#[test]
fn wrapper_call_sites_are_scanned() {
    let sites = scan(
        r#"self.expression_stub("checkFoo ([ITER])", "5.8 iteration protocol")
           self.source_element_stub("checkBar", "M5")"#,
    );
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[0].owner, Some(StageKey(4, 8, u8::MAX)));
    assert_eq!(sites[1].owner, Some(StageKey(5, 0, 0)));
}

#[test]
fn wrapper_definitions_are_excluded() {
    let sites = scan(
        r#"fn expression_stub(&self, worker: &str, owner: &str) -> CheckResult<TypeId> {
               Err(Unsupported::new(format!(
                   "{worker} (expression band, lands at {owner})"
               )))
           }"#,
    );
    assert!(sites.is_empty(), "{:?}", sites[0].reason);
}

#[test]
fn format_reasons_are_scanned_not_dropped() {
    // A real escape whose reason is built with format! — the
    // static text carries the owner; the blanket `{` skip that
    // hid these was a false negative.
    let sites = scan(
        r#"Err(Unsupported::new(format!(
               "anonymous members for symbol flags {flags:?} (M4 5.3e/5.8)"
           )))"#,
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].owner, Some(StageKey(4, 8, u8::MAX)));
}

#[test]
fn dormant_annotation_reclassifies_an_escape_and_roundtrips_canary() {
    let sites = scan(
        r#"fn mapped() {
            // tsc-dormant: canary=mapped_type_model_constructibility; owner=9.5a
            return Err(Unsupported::new("mapped types (unported family, M8-stub)"));
        }"#,
    );
    assert_eq!(sites.len(), 1);
    let metadata = sites[0].dormant.as_ref().expect("dormant metadata");
    assert_eq!(metadata.canary, "mapped_type_model_constructibility");
    assert_eq!(metadata.review_owner.as_deref(), Some("9.5a"));
    let entries = escape_manifest_from_sites(Path::new(""), &sites).unwrap();
    assert_eq!(entries[0].class, "dormant-assumption");
    assert_eq!(
        entries[0].canary.as_deref(),
        Some("mapped_type_model_constructibility")
    );
    assert_eq!(
        parse_escape_manifest(&render_escape_manifest(&entries)).unwrap(),
        entries
    );
}

#[test]
fn standalone_dormant_annotation_requires_an_exact_reason() {
    let sites = scan(
        r#"fn fold() {
            // tsc-dormant: canary=utf16_fold_constructibility; owner=9.5a; reason=lossy UTF-16 fold
            let value = 1;
        }"#,
    );
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].reason, "lossy UTF-16 fold");
    assert!(sites[0].dormant.is_some());
}

#[test]
fn manifest_rejects_mixed_metadata_for_one_identity() {
    let sites = scan(
        r#"fn mapped() {
            // tsc-dormant: canary=mapped_type_model_constructibility; owner=9.5a
            if first {
                return Err(Unsupported::new("mapped types (M8)"));
            }
            return Err(Unsupported::new("mapped types (M8)"));
        }"#,
    );
    assert_eq!(sites.len(), 2);
    let error = escape_manifest_from_sites(Path::new(""), &sites).unwrap_err();
    assert!(
        error.to_string().contains("mixes metadata"),
        "unexpected error: {error}"
    );
}

#[test]
fn manifest_roundtrips_and_keys_on_file_reason() {
    let sites = scan(
        r#"Err(Unsupported::new("alias value types (getTypeOfAlias, 5.8)"));
           Err(Unsupported::new("alias value types (getTypeOfAlias, 5.8)"));
           Err(Unsupported::new("entityNameToString on recovery node"));
           Err(Unsupported::new("a reason with \"quotes\" and back\\slash"));"#,
    );
    let entries = escape_manifest_from_sites(Path::new(""), &sites).unwrap();
    // Duplicate reasons fold into one entry with count 2; classes
    // derive from the reason text.
    assert_eq!(entries.len(), 3);
    let dup = entries
        .iter()
        .find(|entry| entry.reason.starts_with("alias value types"))
        .expect("folded entry");
    assert_eq!(
        (dup.count, dup.class.as_str(), dup.owner.as_deref()),
        (2, "stage", Some("5.8"))
    );
    let recovery = entries
        .iter()
        .find(|entry| entry.reason.contains("recovery node"))
        .expect("recovery entry");
    assert_eq!(
        (recovery.class.as_str(), recovery.owner.as_deref()),
        ("recovery", None)
    );
    let parsed = parse_escape_manifest(&render_escape_manifest(&entries)).expect("roundtrip");
    assert_eq!(parsed, entries);
}

#[test]
fn disposition_census_reads_the_doc_block() {
    let ported = ["/// tsc-port: checkFoo @6.0.3", "pub fn check_foo() {}"];
    let native = [
        "/// tsrs-native: arena accessor",
        "#[inline]",
        "pub(crate) fn arena_get() {}",
    ];
    let deferred = [
        "/// tsc-deferred: M6 inferTypeArguments",
        "pub fn infer() {}",
    ];
    let bare = ["/// plain prose only", "pub fn mystery() {}"];
    assert!(doc_block_has_disposition(&ported, 1));
    assert!(doc_block_has_disposition(&native, 2));
    assert!(doc_block_has_disposition(&deferred, 1));
    assert!(!doc_block_has_disposition(&bare, 1));
    // Review round 2: prose MENTIONS and invalid payloads do not
    // count — the marker must start the line and validate.
    let prose = ["/// this helper is tsrs-native: in spirit", "pub fn x() {}"];
    let empty_native = ["/// tsrs-native:", "pub fn y() {}"];
    let bad_stage = ["/// tsc-deferred: someday", "pub fn z() {}"];
    assert!(!doc_block_has_disposition(&prose, 1));
    assert!(!doc_block_has_disposition(&empty_native, 1));
    assert!(!doc_block_has_disposition(&bad_stage, 1));
    // Review round 3: bare hash/span lines are NOT dispositions
    // (the ledger parser keys on the port header — a bare hash
    // would evade both checks), and stage names are whole words.
    let hash_only = ["/// tsc-hash: abc123", "pub fn h() {}"];
    let span_only = ["/// tsc-span: _tsc.js:1-2", "pub fn s() {}"];
    let stage_prefix = ["/// tsc-deferred: M50 someday", "pub fn w() {}"];
    let stage_word = ["/// tsc-deferred: M5, with reason", "pub fn v() {}"];
    assert!(!doc_block_has_disposition(&hash_only, 1));
    assert!(!doc_block_has_disposition(&span_only, 1));
    assert!(!doc_block_has_disposition(&stage_prefix, 1));
    assert!(doc_block_has_disposition(&stage_word, 1));
    // Review round 4: PLAIN `//` comments (and //// banners) are
    // not dispositions — the ledger collector reads /// blocks
    // alone, so a plain-comment tsc-port would evade hash/span
    // validation.
    let plain_port = ["// tsc-port: fake @6.0.3", "pub(crate) fn sneaky() {}"];
    let plain_native = ["// tsrs-native: also fake", "pub fn sly() {}"];
    let banner = ["//// tsc-port: banner", "pub fn b() {}"];
    assert!(!doc_block_has_disposition(&plain_port, 1));
    assert!(!doc_block_has_disposition(&plain_native, 1));
    assert!(!doc_block_has_disposition(&banner, 1));
    // Review round 5: a plain `//` line TERMINATES the block
    // (the ledger parser clears its doc block there — a doc
    // comment detached by a separator must not count), while a
    // BLANK line is transparent on both sides.
    let separated = [
        "/// tsc-port: dummy @6.0.3",
        "// ordinary separator",
        "pub(crate) fn newly_added() {}",
    ];
    let blank_gap = ["/// tsc-port: real @6.0.3", "", "pub fn f() {}"];
    assert!(!doc_block_has_disposition(&separated, 2));
    assert!(doc_block_has_disposition(&blank_gap, 2));
    // The block scan stops at the first non-comment/attr line.
    let detached = [
        "/// tsrs-native: someone else's fn",
        "pub fn other() {}",
        "pub fn unrelated() {}",
    ];
    assert!(!doc_block_has_disposition(&detached, 2));
}

#[test]
fn fn_backlog_roundtrips() {
    let mut map = BTreeMap::new();
    map.insert(("crates/checker/src/a.rs".to_owned(), "foo".to_owned()), 1);
    map.insert(("crates/checker/src/b.rs".to_owned(), "bar".to_owned()), 2);
    let parsed = parse_fn_backlog(&render_fn_backlog(&map)).expect("roundtrip");
    assert_eq!(parsed, map);
}

#[test]
fn stage_key_displays_match_reason_conventions() {
    assert_eq!(stage_key_display(StageKey(4, 8, u8::MAX)), "5.8");
    assert_eq!(stage_key_display(StageKey(4, 7, b'b')), "5.7b");
    assert_eq!(stage_key_display(StageKey(5, 0, 0)), "M5");
    assert_eq!(stage_key_display(StageKey(8, 0, 0)), "M8");
}

#[test]
fn latest_stage_in_a_reason_wins() {
    let sites = scan(r#"Err(Unsupported::new("expired 5.5f dep; folded into the 5.7b close"))"#);
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].owner, Some(StageKey(4, 7, b'b')));
}

#[test]
fn letterless_stage_owns_the_whole_stage() {
    let sites = scan(r#"Err(Unsupported::new("resolveFoo (5.7)"))"#);
    assert_eq!(sites[0].owner, Some(StageKey(4, 7, u8::MAX)));
    // 5.7 letterless does NOT expire mid-stage (threshold 5.7a).
    assert!(sites[0].owner.unwrap() > parse_stage_key("5.7a").unwrap());
}

#[test]
fn recovery_markers_classify_owner_less_guards() {
    let sites = scan(
        r#"Err(Unsupported::new("tagged template without a tag (parse recovery)"))
           Err(Unsupported::new("conditional with missing branch (parse-recovery tree)"))
           Err(Unsupported::new("entityNameToString on recovery node"))
           Err(Unsupported::new("template span with missing literal"))"#,
    );
    assert_eq!(sites.len(), 4);
    assert!(sites[0].recovery && sites[1].recovery && sites[2].recovery);
    // No marker → stays a plain untagged debt.
    assert!(!sites[3].recovery);
}

#[test]
fn owned_reasons_never_classify_as_recovery() {
    // The owner tag wins even when recovery words appear.
    let sites = scan(r#"Err(Unsupported::new("checkFoo recovery node handling (5.8)"))"#);
    assert_eq!(sites.len(), 1);
    assert!(sites[0].owner.is_some());
    assert!(!sites[0].recovery);
}
