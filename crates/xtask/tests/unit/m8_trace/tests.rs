use super::*;

#[test]
fn parser_normalizes_codes_and_preserves_probe_order() {
    let parsed = parse_args(
        [
            "--program-json",
            "b.json",
            "--code",
            "8020",
            "--program-json",
            "a.json",
            "--code",
            "1453",
            "--out",
            "report.json",
            "--max-lib-cache-buckets",
            "2",
        ]
        .into_iter()
        .map(str::to_owned),
    )
    .unwrap();
    assert_eq!(
        parsed.programs,
        vec![PathBuf::from("b.json"), PathBuf::from("a.json")]
    );
    assert_eq!(parsed.codes, BTreeSet::from([1453, 8020]));
    assert_eq!(parsed.out, PathBuf::from("report.json"));
    assert_eq!(parsed.max_lib_cache_buckets, 2);
}

#[test]
fn parser_rejects_vacuous_or_ambiguous_requests() {
    for args in [
        vec!["--code", "8020", "--out", "report.json"],
        vec!["--program-json", "a.json", "--out", "report.json"],
        vec![
            "--program-json",
            "a.json",
            "--code",
            "8020",
            "--code",
            "8020",
            "--out",
            "report.json",
        ],
    ] {
        assert!(parse_args(args.into_iter().map(str::to_owned)).is_err());
    }
}

#[test]
fn trace_event_requires_covered_exact_declaration_and_valid_pass() {
    let declaration = format!("d2:{}", "a".repeat(64));
    let event = json!({
        "site": {
            "code": 8020,
            "declaration": declaration,
        },
        "pass": "semantic",
        "frames": [{
            "function_name": "producer",
            "d2_declaration": declaration,
        }],
    });
    let codes = BTreeSet::from([8020]);
    let valid = json!({
        "trace": [event.clone()],
        "coverage": {
            "exact_d2_declarations": [declaration],
        },
    });
    assert!(validate_probe(&valid, &codes).is_ok());

    let missing_coverage = json!({
        "trace": [event.clone()],
        "coverage": {
            "exact_d2_declarations": [],
        },
    });
    assert!(validate_probe(&missing_coverage, &codes).is_err());

    let wrong_pass = json!({
        "trace": [{
            "site": event["site"],
            "pass": "declaration",
            "frames": event["frames"],
        }],
        "coverage": {
            "exact_d2_declarations": [declaration],
        },
    });
    assert!(validate_probe(&wrong_pass, &codes).is_err());
}
