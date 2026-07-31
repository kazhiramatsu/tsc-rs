use tsrs2_fuzz::classify::{classify_case, CanonicalClass, ClassFailure, ClassPass, OutcomeSide};
use tsrs2_fuzz::compare::{
    compare_case, Comparison, ComparisonTier, DiagnosticDivergence, DifferenceSide, Divergence,
    OneSidedDiagnostic, RendererDifference,
};
use tsrs2_fuzz::evaluate::evaluate_case;
use tsrs2_fuzz::model::{
    AssembledDiagnostic, CanonicalHead, CaseExecution, CompletedOutcome, DiagnosticCategory,
    DiagnosticFile, DiagnosticPass, DiagnosticRecord, EngineResult, MessageChain, OptionalBool,
    OptionalString, OptionalU32, ProducerFailure, ProducerFailureKind, ProducerFailureSource,
    RelatedDiagnostic, RenderSegment, RendererObservation, TerminalBoundaryId, TerminalKind,
    TerminalOutcome, TerminalPhase, MAX_MESSAGE_CHAIN_DEPTH, MAX_MESSAGE_CHAIN_NODES,
};
use tsrs2_fuzz::normalize::NormalizationSpec;
use tsrs2_fuzz::schema::{
    sha256_hex, CanonicalU64, CaseProvenance, CaseSpec, ChildProcessPolicy, DecisionValue,
    DomainMembership, EncodedFile, NodeProcessPolicy, OrderedArgument, ProcessPolicy,
    RustProcessPolicy, StableDecision, CASE_SPEC_SCHEMA,
};

use std::process::Command;

const BASIC_SOURCE: &str = "abcdefghij\nklmnopqrst\nuvwxyz\n";

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        output.push(TABLE[usize::from(a >> 2)] as char);
        output.push(TABLE[usize::from(((a & 0x03) << 4) | (b >> 4))] as char);
        if chunk.len() > 1 {
            output.push(TABLE[usize::from(((b & 0x0f) << 2) | (c >> 6))] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[usize::from(c & 0x3f)] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn encoded_file(ordinal: u32, name: &str, text: &str) -> EncodedFile {
    EncodedFile {
        ordinal,
        name: name.to_owned(),
        text_base64: base64(text.as_bytes()),
    }
}

fn process_policy() -> ProcessPolicy {
    ProcessPolicy {
        schema: 1,
        oracle_node: NodeProcessPolicy {
            executable_id: "node-pinned".to_owned(),
            arguments: vec![OrderedArgument {
                ordinal: 0,
                value: "--single-threaded".to_owned(),
            }],
            single_threaded: true,
            deadline_ms: CanonicalU64::new(30_000),
            rollover_cases: CanonicalU64::new(500),
        },
        tsrs: RustProcessPolicy {
            worker_cap: 2,
            deadline_ms: CanonicalU64::new(30_000),
            rollover_cases: CanonicalU64::new(500),
        },
        child: ChildProcessPolicy {
            policy_id: "bounded-serial-v1".to_owned(),
            cases_per_child: CanonicalU64::new(500),
        },
    }
}

fn case_with(
    cwd: &str,
    identifier: &str,
    root_seed: u64,
    case_seed: u64,
    files: &[(&str, &str)],
) -> CaseSpec {
    CaseSpec {
        schema: CASE_SPEC_SCHEMA,
        case_id: "case-0001".to_owned(),
        generator_id: "foundation-contract".to_owned(),
        provenance: CaseProvenance {
            root_seed: CanonicalU64::new(root_seed),
            case_index: CanonicalU64::new(1),
            case_seed: CanonicalU64::new(case_seed),
        },
        decisions: vec![StableDecision {
            ordinal: 0,
            id: "generated-identifier".to_owned(),
            value: DecisionValue::Identifier {
                value: identifier.to_owned(),
            },
        }],
        domain_membership: vec![DomainMembership {
            ordinal: 0,
            id: "supported-batch".to_owned(),
        }],
        cwd: cwd.to_owned(),
        options: Vec::new(),
        libs: Vec::new(),
        files: files
            .iter()
            .enumerate()
            .map(|(index, (name, text))| encoded_file(u32::try_from(index).unwrap(), name, text))
            .collect(),
        matrix_key: String::new(),
        matrix: Vec::new(),
        normalization_schema: 1,
        process_policy: process_policy(),
    }
}

fn basic_case() -> CaseSpec {
    case_with(
        "/work",
        "generatedName",
        u64::MAX,
        9_876_543_210,
        &[("main.ts", BASIC_SOURCE), ("other.ts", BASIC_SOURCE)],
    )
}

fn present(value: u32) -> OptionalU32 {
    OptionalU32::Present { value }
}

fn chain(text: &str) -> MessageChain {
    MessageChain {
        text: text.to_owned(),
        code: 1,
        category: DiagnosticCategory::Error,
        next_present: false,
        next: Vec::new(),
    }
}

fn diagnostic(pass: DiagnosticPass, code: u32, head: &str) -> DiagnosticRecord {
    DiagnosticRecord {
        pass,
        file: DiagnosticFile::File {
            path: "main.ts".to_owned(),
        },
        code,
        line: present(0),
        column: present(0),
        category: DiagnosticCategory::Error,
        start: present(0),
        length: present(1),
        chain: chain(head),
        related_information_present: false,
        related: Vec::new(),
        reports_unnecessary: OptionalBool::absent(),
        reports_deprecated: OptionalBool::absent(),
        source: OptionalString::absent(),
    }
}

fn assembled(diagnostic: &DiagnosticRecord) -> AssembledDiagnostic {
    AssembledDiagnostic {
        diagnostic: diagnostic.clone(),
        canonical_head: CanonicalHead::absent(),
    }
}

fn observation(
    assembled_rows: &[DiagnosticRecord],
    deduped_rows: &[DiagnosticRecord],
    segments: &[(&str, &str)],
) -> RendererObservation {
    assert_eq!(deduped_rows.len(), segments.len());
    let segments = deduped_rows
        .iter()
        .zip(segments)
        .map(|(diagnostic, (_rendered_path, raw_text))| RenderSegment {
            diagnostic: assembled(diagnostic),
            raw_text: (*raw_text).to_owned(),
        })
        .collect::<Vec<_>>();
    let aggregate_text = segments
        .iter()
        .map(|segment| segment.raw_text.as_str())
        .collect::<String>();
    RendererObservation {
        assembled: assembled_rows.iter().map(assembled).collect(),
        deduped: deduped_rows.iter().map(assembled).collect(),
        segments,
        aggregate_text,
    }
}

fn default_observation(diagnostics: &[DiagnosticRecord]) -> RendererObservation {
    let owned = diagnostics
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| {
            (
                match &diagnostic.file {
                    DiagnosticFile::Global => "",
                    DiagnosticFile::File { path } => path.as_str(),
                },
                format!("{index}:TS{}:{}\n", diagnostic.code, diagnostic.top_text()),
            )
        })
        .collect::<Vec<_>>();
    let borrowed = owned
        .iter()
        .map(|(path, text)| (*path, text.as_str()))
        .collect::<Vec<_>>();
    observation(diagnostics, diagnostics, &borrowed)
}

fn completed(diagnostics: Vec<DiagnosticRecord>) -> CompletedOutcome {
    CompletedOutcome {
        renderer: default_observation(&diagnostics),
        diagnostics,
    }
}

fn completed_with_renderer(
    diagnostics: Vec<DiagnosticRecord>,
    renderer: RendererObservation,
) -> CompletedOutcome {
    CompletedOutcome {
        diagnostics,
        renderer,
    }
}

fn execution(oracle: CompletedOutcome, tsrs: CompletedOutcome) -> CaseExecution {
    CaseExecution::Compared {
        oracle: EngineResult::Completed { outcome: oracle },
        tsrs: EngineResult::Completed { outcome: tsrs },
    }
}

fn diagnostic_divergence(comparison: Comparison) -> DiagnosticDivergence {
    match comparison {
        Comparison::Divergence(Divergence::Diagnostic(divergence)) => divergence,
        other => panic!("expected diagnostic divergence, got {other:?}"),
    }
}

fn renderer_class(comparison: Comparison) -> (RendererDifference, DiagnosticRecord) {
    match comparison {
        Comparison::Divergence(Divergence::Renderer(divergence)) => {
            (divergence.class, divergence.affected)
        }
        other => panic!("expected renderer divergence, got {other:?}"),
    }
}

#[test]
fn case_schema_pins_canonical_u64_bytes_and_sha256() {
    let case = basic_case();
    case.validate().unwrap();
    let bytes = case.canonical_bytes().unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();

    assert!(text
        .starts_with(r#"{"schema":1,"case_id":"case-0001","generator_id":"foundation-contract""#));
    assert!(text.contains(r#""root_seed":"18446744073709551615""#));
    assert!(text.contains(r#""case_seed":"9876543210""#));
    assert!(!text.contains('\n'));
    assert_eq!(case.canonical_sha256().unwrap(), sha256_hex(&bytes));
    assert_eq!(case, CaseSpec::from_canonical_slice(&bytes).unwrap());
}

#[test]
fn case_schema_rejects_unknown_noncanonical_and_unsafe_json_forms() {
    let case = basic_case();
    let canonical = String::from_utf8(case.canonical_bytes().unwrap()).unwrap();

    let unknown = canonical.replacen(r#""schema":1"#, r#""unknown":0,"schema":1"#, 1);
    assert!(CaseSpec::from_json_slice(unknown.as_bytes()).is_err());

    let padded = format!("{canonical}\n");
    assert!(CaseSpec::from_json_slice(padded.as_bytes()).is_ok());
    assert!(CaseSpec::from_canonical_slice(padded.as_bytes()).is_err());

    let numeric_seed = canonical.replace(
        r#""root_seed":"18446744073709551615""#,
        r#""root_seed":18446744073709551615"#,
    );
    assert!(CaseSpec::from_json_slice(numeric_seed.as_bytes()).is_err());

    let leading_zero = canonical.replace(
        r#""case_seed":"9876543210""#,
        r#""case_seed":"09876543210""#,
    );
    assert!(CaseSpec::from_json_slice(leading_zero.as_bytes()).is_err());

    let unknown_enum = canonical.replace(r#""kind":"identifier""#, r#""kind":"not-a-decision""#);
    assert!(CaseSpec::from_json_slice(unknown_enum.as_bytes()).is_err());

    let lone_surrogate = canonical.replace("foundation-contract", r"\ud800");
    assert!(
        CaseSpec::from_json_slice(lone_surrogate.as_bytes()).is_err(),
        "a JavaScript lone surrogate has no valid Rust UTF-8 representation"
    );
}

#[test]
fn case_schema_rejects_noncanonical_base64_invalid_utf8_and_bad_paths() {
    let mut case = basic_case();
    case.files[0].text_base64 = "YR==".to_owned();
    assert!(case.validate().is_err(), "non-zero base64 padding bits");

    let mut case = basic_case();
    case.files[0].text_base64 = "/w==".to_owned();
    assert!(case.validate().is_err(), "source must be UTF-8");

    for bad in [
        "../main.ts",
        "dir/../main.ts",
        "dir\\main.ts",
        "",
        "main.ts/",
    ] {
        let mut case = basic_case();
        case.files[0].name = bad.to_owned();
        assert!(case.validate().is_err(), "{bad:?} must be rejected");
    }

    let mut case = basic_case();
    case.cwd = "/work\nchild".to_owned();
    assert!(
        case.validate().is_err(),
        "virtual cwd must reject control characters"
    );
}

#[test]
fn case_schema_preserves_relative_names_and_rejects_resolved_aliases() {
    let case = basic_case();
    assert_eq!(case.resolved_file_name("main.ts").unwrap(), "/work/main.ts");
    assert_eq!(case.source_text("main.ts").unwrap(), BASIC_SOURCE);
    assert!(case.source_text("/work/main.ts").is_err());

    let alias = case_with(
        "/work",
        "generatedName",
        10_000_001,
        10_000_002,
        &[("main.ts", BASIC_SOURCE), ("/work/main.ts", BASIC_SOURCE)],
    );
    assert!(
        alias.validate().is_err(),
        "two public names resolving to one SourceFile.path are ambiguous"
    );
}

#[test]
fn schema_uses_utf8_byte_order_for_semantic_sets() {
    let mut case = basic_case();
    case.domain_membership = vec![
        DomainMembership {
            ordinal: 0,
            id: "\u{e000}".to_owned(),
        },
        DomainMembership {
            ordinal: 1,
            id: "\u{10000}".to_owned(),
        },
    ];
    case.validate().unwrap();

    case.domain_membership.swap(0, 1);
    case.domain_membership[0].ordinal = 0;
    case.domain_membership[1].ordinal = 1;
    assert!(
        case.validate().is_err(),
        "UTF-16 default order must not replace UTF-8 byte order"
    );
}

#[test]
fn normalizer_handles_lf_paths_identifiers_boundaries_and_unicode_without_folding() {
    let case = basic_case();
    let normalization = NormalizationSpec::for_case(&case).unwrap();
    let input = concat!(
        "main.ts /work/main.ts \\work\\main.ts\r\n",
        "generatedName generatedName2 αgeneratedNameβ αmain.tsβ\r",
        "/unknown/user.ts e\u{301} é"
    );
    let normalized = normalization.normalize(input).unwrap();

    assert_eq!(
        normalized,
        concat!(
            "<@2:0@> <@2:0@> <@2:0@>\n",
            "<#0#> generatedName2 αgeneratedNameβ αmain.tsβ\n",
            "/unknown/user.ts e\u{301} é"
        )
    );
    assert_eq!(
        normalization.normalize_exact_path("main.ts").unwrap(),
        "<@2:0@>"
    );
    assert_eq!(
        normalization.normalize_exact_path("/work/main.ts").unwrap(),
        "<@2:0@>"
    );
    assert_ne!(
        normalization.normalize("e\u{301}").unwrap(),
        normalization.normalize("é").unwrap(),
        "normalization must not perform NFC/NFD folding"
    );
}

#[test]
fn normalizer_is_total_for_every_schema_valid_identifier() {
    for identifier in [
        "generatedName",
        "identifier",
        "file",
        "cwd",
        "root",
        "address",
        "_$ok9",
    ] {
        let case = case_with(
            "/work",
            identifier,
            12_345_678,
            87_654_321,
            &[("main.ts", BASIC_SOURCE)],
        );
        case.validate().unwrap();
        let normalization = NormalizationSpec::for_case(&case)
            .unwrap_or_else(|error| panic!("valid identifier {identifier:?}: {error}"));
        let once = normalization
            .normalize(identifier)
            .unwrap_or_else(|error| panic!("normalize {identifier:?}: {error}"));
        assert_eq!(once, "<#0#>", "{identifier:?}");
    }
}

#[test]
fn case_schema_rejects_cross_role_normalization_ownership_collisions() {
    let cwd_owned_as_file = case_with(
        "/work",
        "generatedName",
        12_345_678,
        87_654_321,
        &[("/work", BASIC_SOURCE)],
    );
    assert!(
        cwd_owned_as_file.validate().is_err(),
        "cwd and a source file cannot own the same raw path"
    );

    let identifier_owned_as_file = case_with(
        "/work",
        "main",
        12_345_678,
        87_654_321,
        &[("main", BASIC_SOURCE)],
    );
    assert!(
        identifier_owned_as_file.validate().is_err(),
        "one raw token cannot be both an owned path and generator identifier"
    );

    let adjacent = case_with(
        "/work",
        "main",
        12_345_678,
        87_654_321,
        &[("main.ts", BASIC_SOURCE)],
    );
    let normalization = NormalizationSpec::for_case(&adjacent).unwrap();
    assert_eq!(
        normalization.normalize("main main.ts").unwrap(),
        "<#0#> <@2:0@>",
        "prefix overlap remains valid when the complete owned sources differ"
    );
}

#[test]
fn normalizer_encodes_literal_markers_separately_from_typed_placeholders() {
    let case = case_with(
        "/work",
        "generatedName",
        12_345_678,
        87_654_321,
        &[("@0", BASIC_SOURCE), ("%0", BASIC_SOURCE)],
    );
    let normalization = NormalizationSpec::for_case(&case).unwrap();
    let once = normalization
        .normalize("/work @0 %0 generatedName")
        .unwrap();
    assert_eq!(once, "<@0@> <@2:0@> <@2:1@> <#0#>");

    let reserved = case_with(
        "/work",
        "generatedName",
        12_345_678,
        87_654_321,
        &[("<@0@>", BASIC_SOURCE)],
    );
    reserved
        .validate()
        .expect("raw paths may contain text that resembles a placeholder");
    let marker_normalization = NormalizationSpec::for_case(&basic_case()).unwrap();
    let literal = marker_normalization
        .normalize("literal <@2:0@> <#0#> <%0%>; path main.ts")
        .unwrap();
    assert_eq!(literal, "literal <<@2:0@> <<#0#> <<%0%>; path <@2:0@>");
    assert_ne!(
        marker_normalization.normalize("<@2:0@>").unwrap(),
        marker_normalization.normalize("main.ts").unwrap(),
        "raw marker text and an injected path token must not share a class"
    );

    let crossing = case_with(
        "/work",
        "generatedName",
        12_345_678,
        87_654_321,
        &[("(<@0", BASIC_SOURCE)],
    );
    let crossing_normalization = NormalizationSpec::for_case(&crossing).unwrap();
    let once = crossing_normalization.normalize("(/work").unwrap();
    assert_eq!(once, "(<@0@>");
}

#[test]
fn terminal_normalization_removes_only_reviewed_volatile_tokens() {
    let case = case_with(
        "/work",
        "generatedName",
        12_345_678,
        87_654_321,
        &[("main.ts", BASIC_SOURCE)],
    );
    let normalization = NormalizationSpec::for_case(&case).unwrap();
    let normalized = normalization
        .normalize_terminal(
            "panic /work/main.ts generatedName seed 12345678 case 87654321 at 0xabcdef",
        )
        .unwrap();
    assert_eq!(
        normalized,
        "panic <@2:0@> <#0#> seed <%0%> case <%1%> at <%2%>"
    );
    assert_ne!(
        normalization.normalize_terminal("semantic-A").unwrap(),
        normalization.normalize_terminal("semantic-B").unwrap()
    );
}

#[test]
fn model_accepts_fileless_and_relative_diagnostics_but_rejects_impossible_locations() {
    let global = DiagnosticRecord {
        pass: DiagnosticPass::Semantic,
        file: DiagnosticFile::Global,
        code: 18000,
        line: OptionalU32::Absent,
        column: OptionalU32::Absent,
        category: DiagnosticCategory::Error,
        start: OptionalU32::Absent,
        length: OptionalU32::Absent,
        chain: chain("global"),
        related_information_present: false,
        related: Vec::new(),
        reports_unnecessary: OptionalBool::absent(),
        reports_deprecated: OptionalBool::absent(),
        source: OptionalString::absent(),
    };
    global.validate("global").unwrap();

    let mut invalid_global = global.clone();
    invalid_global.start = present(0);
    invalid_global.length = present(0);
    invalid_global.line = present(0);
    invalid_global.column = present(0);
    assert!(invalid_global.validate("global").is_err());

    let case = basic_case();
    let relative = diagnostic(DiagnosticPass::Semantic, 2322, "relative");
    EngineResult::Completed {
        outcome: completed(vec![relative]),
    }
    .validate_for_case(&case, "relative")
    .unwrap();

    let mut impossible_related = diagnostic(DiagnosticPass::Semantic, 2322, "related");
    impossible_related.related_information_present = true;
    impossible_related.related.push(RelatedDiagnostic {
        file_present: false,
        file: None,
        start_present: true,
        start: Some(0),
        length_present: true,
        length: Some(1),
        code: 6200,
        category: DiagnosticCategory::Message,
        chain: chain("related"),
    });
    assert!(
        EngineResult::Completed {
            outcome: completed(vec![impossible_related]),
        }
        .validate_for_case(&case, "related")
        .is_err(),
        "a related span without a file is not a real diagnostic shape"
    );
}

#[test]
fn model_validates_utf16_positions_crlf_and_unicode_line_separators() {
    let text = "a\u{2028}b\u{2029}c\r\nd😀";
    let case = case_with(
        "/work",
        "generatedName",
        12_345_678,
        87_654_321,
        &[("main.ts", text)],
    );
    let mut ls = diagnostic(DiagnosticPass::Semantic, 1001, "ls");
    ls.start = present(2);
    ls.length = present(1);
    ls.line = present(1);
    ls.column = present(0);
    let mut ps = diagnostic(DiagnosticPass::Semantic, 1002, "ps");
    ps.start = present(4);
    ps.length = present(1);
    ps.line = present(2);
    ps.column = present(0);
    let mut astral = diagnostic(DiagnosticPass::Semantic, 1003, "astral");
    astral.start = present(8);
    astral.length = present(2);
    astral.line = present(3);
    astral.column = present(1);

    EngineResult::Completed {
        outcome: completed(vec![ls, ps.clone(), astral]),
    }
    .validate_for_case(&case, "unicode")
    .unwrap();

    ps.line = present(1);
    assert!(EngineResult::Completed {
        outcome: completed(vec![ps]),
    }
    .validate_for_case(&case, "bad-line")
    .is_err());

    let mut split = diagnostic(DiagnosticPass::Semantic, 1004, "split");
    split.start = present(9);
    split.length = present(1);
    split.line = present(3);
    split.column = present(2);
    assert!(EngineResult::Completed {
        outcome: completed(vec![split]),
    }
    .validate_for_case(&case, "split-surrogate")
    .is_err());
}

#[test]
fn message_chain_schema_bounds_depth_and_total_nodes_before_recursive_projection() {
    let nested = |depth: usize| {
        let mut current = chain("leaf");
        for index in 1..depth {
            current = MessageChain {
                text: format!("node-{index}"),
                code: 1,
                category: DiagnosticCategory::Error,
                next_present: true,
                next: vec![current],
            };
        }
        current
    };

    let max_depth = nested(MAX_MESSAGE_CHAIN_DEPTH);
    max_depth
        .validate("max-depth")
        .expect("the exact schema depth ceiling is valid");
    let case = basic_case();
    let mut max_depth_diagnostic =
        diagnostic(DiagnosticPass::Semantic, 1, "maximum message-chain depth");
    max_depth_diagnostic.chain = max_depth;
    let max_depth_engine = EngineResult::Completed {
        outcome: completed(vec![max_depth_diagnostic]),
    };
    let max_depth_engine_bytes = max_depth_engine
        .canonical_bytes(&case)
        .expect("the exact depth ceiling must serialize as an engine outcome");
    assert_eq!(
        EngineResult::from_canonical_slice(&case, &max_depth_engine_bytes)
            .expect("the exact depth ceiling must deserialize as an engine outcome"),
        max_depth_engine,
        "every schema-valid message chain must round-trip through its engine envelope"
    );
    let max_depth_execution = CaseExecution::Compared {
        oracle: max_depth_engine.clone(),
        tsrs: max_depth_engine,
    };
    let max_depth_bytes = max_depth_execution
        .canonical_bytes(&case)
        .expect("the exact depth ceiling must serialize");
    assert_eq!(
        CaseExecution::from_canonical_slice(&case, &max_depth_bytes)
            .expect("the exact depth ceiling must deserialize"),
        max_depth_execution,
        "every schema-valid message chain must round-trip through its evidence envelope"
    );
    assert!(
        nested(MAX_MESSAGE_CHAIN_DEPTH + 1)
            .validate("too-deep")
            .is_err(),
        "a chain deeper than the fixed ceiling must fail before comparison"
    );

    let too_wide = MessageChain {
        text: "root".to_owned(),
        code: 1,
        category: DiagnosticCategory::Error,
        next_present: true,
        next: (0..=MAX_MESSAGE_CHAIN_NODES)
            .map(|index| chain(&format!("child-{index}")))
            .collect(),
    };
    assert!(
        too_wide.validate("too-wide").is_err(),
        "a wide tree cannot bypass the total node ceiling"
    );
}

#[test]
fn model_uses_tsc_crlf_line_starts_for_midpoint_and_end() {
    let case = case_with(
        "/work",
        "generatedName",
        12_345_678,
        87_654_321,
        &[("main.ts", "\r\nx")],
    );

    let mut midpoint = diagnostic(DiagnosticPass::Semantic, 1001, "midpoint");
    midpoint.start = present(1);
    midpoint.length = present(1);
    midpoint.line = present(0);
    midpoint.column = present(1);
    midpoint.related_information_present = true;
    midpoint.related.push(RelatedDiagnostic {
        file_present: true,
        file: Some("main.ts".to_owned()),
        start_present: true,
        start: Some(1),
        length_present: true,
        length: Some(1),
        code: 6200,
        category: DiagnosticCategory::Message,
        chain: chain("related midpoint"),
    });

    let mut after_crlf = diagnostic(DiagnosticPass::Semantic, 1002, "after-crlf");
    after_crlf.start = present(2);
    after_crlf.length = present(1);
    after_crlf.line = present(1);
    after_crlf.column = present(0);

    EngineResult::Completed {
        outcome: completed(vec![midpoint, after_crlf]),
    }
    .validate_for_case(&case, "crlf")
    .unwrap();
}

#[test]
fn renderer_model_preserves_stage_defects_and_enforces_segment_boundaries() {
    let a = diagnostic(DiagnosticPass::Semantic, 1001, "a");
    let b = diagnostic(DiagnosticPass::Semantic, 1002, "b");
    let valid = completed_with_renderer(
        vec![a.clone(), b.clone()],
        observation(
            &[b.clone(), a.clone()],
            &[a.clone(), b.clone()],
            &[("main.ts", "a\n"), ("main.ts", "b\n")],
        ),
    );
    valid.validate("valid").unwrap();

    let dropped_assembled = completed_with_renderer(
        vec![a.clone(), b.clone()],
        observation(
            std::slice::from_ref(&a),
            std::slice::from_ref(&a),
            &[("main.ts", "a\n")],
        ),
    );
    dropped_assembled.validate("dropped").unwrap();

    let missing_identity = completed_with_renderer(
        vec![a.clone(), b.clone()],
        observation(
            &[a.clone(), b.clone()],
            std::slice::from_ref(&a),
            &[("main.ts", "a\n")],
        ),
    );
    missing_identity.validate("dedupe-drop").unwrap();

    let inflated = completed_with_renderer(
        vec![a.clone(), b.clone()],
        observation(
            &[a.clone(), b.clone()],
            &[a.clone(), a.clone(), b.clone()],
            &[("main.ts", "a\n"), ("main.ts", "a\n"), ("main.ts", "b\n")],
        ),
    );
    inflated.validate("inflated").unwrap();

    let empty_segment = completed_with_renderer(
        vec![a.clone()],
        observation(
            std::slice::from_ref(&a),
            std::slice::from_ref(&a),
            &[("main.ts", "")],
        ),
    );
    empty_segment.validate("empty-segment").unwrap();

    let mut bad_aggregate = valid.clone();
    bad_aggregate.renderer.aggregate_text.push('x');
    assert!(bad_aggregate.validate("aggregate").is_err());

    let mut bad_segment = valid;
    bad_segment.renderer.segments[0].diagnostic = assembled(&b);
    assert!(bad_segment.validate("segment").is_err());
}

#[test]
fn comparator_t0_is_a_set_but_class_input_retains_exclusive_multiplicity() {
    let case = basic_case();
    let row = diagnostic(DiagnosticPass::Semantic, 1001, "duplicate");

    let t1 = diagnostic_divergence(
        compare_case(
            &case,
            &execution(
                completed(vec![row.clone(), row.clone()]),
                completed(vec![row.clone()]),
            ),
        )
        .unwrap(),
    );
    assert_eq!(t1.tier, ComparisonTier::T1);
    assert_eq!(t1.one_sided.len(), 1);

    let t0 = diagnostic_divergence(
        compare_case(
            &case,
            &execution(completed(vec![row.clone(), row]), completed(Vec::new())),
        )
        .unwrap(),
    );
    assert_eq!(t0.tier, ComparisonTier::T0);
    assert_eq!(
        t0.one_sided.len(),
        2,
        "T0 detection is set-like, but the canonical one-sided input is a multiset"
    );
}

#[test]
fn comparator_t1_cancels_common_heads_and_pairs_residuals_deterministically() {
    let case = basic_case();
    let a = diagnostic(DiagnosticPass::Semantic, 1001, "A");
    let b = diagnostic(DiagnosticPass::Semantic, 1001, "B");
    let c = diagnostic(DiagnosticPass::Semantic, 1001, "C");

    let common = diagnostic_divergence(
        compare_case(
            &case,
            &execution(
                completed(vec![a.clone(), b.clone()]),
                completed(vec![a.clone()]),
            ),
        )
        .unwrap(),
    );
    assert_eq!(common.tier, ComparisonTier::T1);
    assert_eq!(common.one_sided.len(), 1);
    assert_eq!(common.one_sided[0].diagnostic.top_text(), "B");

    let residual = diagnostic_divergence(
        compare_case(
            &case,
            &execution(
                completed(vec![b.clone(), a.clone()]),
                completed(vec![c.clone()]),
            ),
        )
        .unwrap(),
    );
    assert_eq!(residual.tier, ComparisonTier::T1);
    assert_eq!(
        residual
            .one_sided
            .iter()
            .map(|row| (row.side, row.diagnostic.top_text()))
            .collect::<Vec<_>>(),
        vec![(DifferenceSide::Oracle, "B")],
        "the T1 count delta stays one occurrence before the retained row maps to its T2 head"
    );
    let reversed = diagnostic_divergence(
        compare_case(
            &case,
            &execution(completed(vec![c]), completed(vec![a.clone(), b.clone()])),
        )
        .unwrap(),
    );
    assert_eq!(
        reversed
            .one_sided
            .iter()
            .map(|row| (row.side, row.diagnostic.top_text()))
            .collect::<Vec<_>>(),
        vec![(DifferenceSide::Tsrs, "B")]
    );

    let many = diagnostic_divergence(
        compare_case(
            &case,
            &execution(completed(vec![a.clone(); 5]), completed(vec![a])),
        )
        .unwrap(),
    );
    assert_eq!(many.tier, ComparisonTier::T1);
    assert_eq!(many.one_sided.len(), 4);
}

#[test]
fn comparator_tiers_are_independent_complete_multisets() {
    let case = basic_case();
    let error_a = diagnostic(DiagnosticPass::Semantic, 1001, "A");
    let mut warning_b = diagnostic(DiagnosticPass::Semantic, 1001, "B");
    warning_b.category = DiagnosticCategory::Warning;
    let mut warning_a = warning_b.clone();
    warning_a.chain.text = "A".to_owned();
    let mut error_b = error_a.clone();
    error_b.chain.text = "B".to_owned();

    let swapped = diagnostic_divergence(
        compare_case(
            &case,
            &execution(
                completed(vec![error_a.clone(), warning_b]),
                completed(vec![warning_a, error_b]),
            ),
        )
        .unwrap(),
    );
    assert_eq!(
        swapped.tier,
        ComparisonTier::T2,
        "equal T1 category multisets must advance independently to T2"
    );

    let mut nested = error_a.clone();
    nested.chain.next_present = true;
    nested.chain.next.push(chain("child"));
    let t3 = diagnostic_divergence(
        compare_case(
            &case,
            &execution(completed(vec![error_a.clone()]), completed(vec![nested])),
        )
        .unwrap(),
    );
    assert_eq!(t3.tier, ComparisonTier::T3);

    let mut presence_only = error_a.clone();
    presence_only.chain.next_present = true;
    assert_eq!(
        compare_case(
            &case,
            &execution(
                completed(vec![error_a.clone()]),
                completed(vec![presence_only]),
            ),
        )
        .unwrap(),
        Comparison::Exact,
        "empty-next presence is not part of the frozen T3 tree projection"
    );
}

#[test]
fn comparator_related_presence_is_t4_only_and_related_order_is_t3() {
    let case = basic_case();
    let absent = diagnostic(DiagnosticPass::Semantic, 1001, "related");
    let mut present_empty = absent.clone();
    present_empty.related_information_present = true;

    let oracle_renderer = observation(
        std::slice::from_ref(&present_empty),
        std::slice::from_ref(&present_empty),
        &[("main.ts", "present-empty-related\n")],
    );
    let tsrs_renderer = observation(
        std::slice::from_ref(&absent),
        std::slice::from_ref(&absent),
        &[("main.ts", "absent-related\n")],
    );
    let (class, _) = renderer_class(
        compare_case(
            &case,
            &execution(
                completed_with_renderer(vec![present_empty], oracle_renderer),
                completed_with_renderer(vec![absent.clone()], tsrs_renderer),
            ),
        )
        .unwrap(),
    );
    assert_eq!(class, RendererDifference::Text);

    let related_a = RelatedDiagnostic {
        file_present: true,
        file: Some("main.ts".to_owned()),
        start_present: true,
        start: Some(0),
        length_present: true,
        length: Some(1),
        code: 6201,
        category: DiagnosticCategory::Message,
        chain: chain("A"),
    };
    let mut related_b = related_a.clone();
    related_b.code = 6202;
    related_b.chain.text = "B".to_owned();
    let mut oracle = absent.clone();
    oracle.related_information_present = true;
    oracle.related = vec![related_a.clone(), related_b.clone()];
    let mut tsrs = oracle.clone();
    tsrs.related = vec![related_b, related_a];
    let divergence = diagnostic_divergence(
        compare_case(
            &case,
            &execution(completed(vec![oracle]), completed(vec![tsrs])),
        )
        .unwrap(),
    );
    assert_eq!(divergence.tier, ComparisonTier::T3);
}

#[test]
fn comparator_keeps_formatter_metadata_out_of_t3() {
    let case = basic_case();
    let oracle = diagnostic(DiagnosticPass::Semantic, 1001, "same");
    let mut tsrs = oracle.clone();
    tsrs.reports_unnecessary = OptionalBool::present(true);
    tsrs.reports_deprecated = OptionalBool::present(false);
    tsrs.source = OptionalString::present("plugin-source");

    assert_eq!(
        compare_case(
            &case,
            &execution(completed(vec![oracle]), completed(vec![tsrs])),
        )
        .unwrap(),
        Comparison::Exact,
        "reports/source metadata is retained raw but not part of frozen T3"
    );
}

#[test]
fn comparator_selects_tier_before_pass_and_keeps_passes_separate() {
    let case = basic_case();
    let syntactic_a = diagnostic(DiagnosticPass::Syntactic, 1001, "A");
    let mut syntactic_b = syntactic_a.clone();
    syntactic_b.chain.text = "B".to_owned();
    let semantic_error = diagnostic(DiagnosticPass::Semantic, 1002, "same");
    let mut semantic_warning = semantic_error.clone();
    semantic_warning.category = DiagnosticCategory::Warning;

    let divergence = diagnostic_divergence(
        compare_case(
            &case,
            &execution(
                completed(vec![syntactic_a, semantic_error]),
                completed(vec![syntactic_b, semantic_warning]),
            ),
        )
        .unwrap(),
    );
    assert_eq!(divergence.tier, ComparisonTier::T1);
    assert_eq!(divergence.pass, DiagnosticPass::Semantic);

    let syntactic = diagnostic(DiagnosticPass::Syntactic, 1003, "pass");
    let semantic = DiagnosticRecord {
        pass: DiagnosticPass::Semantic,
        ..syntactic.clone()
    };
    let divergence = diagnostic_divergence(
        compare_case(
            &case,
            &execution(completed(vec![syntactic]), completed(vec![semantic])),
        )
        .unwrap(),
    );
    assert_eq!(divergence.tier, ComparisonTier::T0);
    assert_eq!(divergence.pass, DiagnosticPass::Syntactic);
}

#[test]
fn diagnostic_input_permutation_does_not_change_comparison_or_class_bytes() {
    let case = basic_case();
    let a = diagnostic(DiagnosticPass::Semantic, 9, "\u{10000}");
    let b = diagnostic(DiagnosticPass::Semantic, 9, "\u{e000}");
    let c = diagnostic(DiagnosticPass::Semantic, 10, "ten");
    let canonical_renderer = default_observation(&[a.clone(), b.clone(), c.clone()]);

    let oracle_one = completed_with_renderer(
        vec![c.clone(), a.clone(), b.clone()],
        canonical_renderer.clone(),
    );
    let oracle_two =
        completed_with_renderer(vec![b.clone(), c.clone(), a.clone()], canonical_renderer);
    let tsrs = completed(Vec::new());
    let first = compare_case(&case, &execution(oracle_one, tsrs.clone())).unwrap();
    let second = compare_case(&case, &execution(oracle_two, tsrs)).unwrap();
    let first_class = classify_case(&case, &first).unwrap().unwrap();
    let second_class = classify_case(&case, &second).unwrap().unwrap();
    assert_eq!(
        first_class.canonical_bytes().unwrap(),
        second_class.canonical_bytes().unwrap()
    );

    assert_eq!(
        first_class
            .rows
            .iter()
            .map(|row| (row.code, row.normalized_message_head.as_str()))
            .collect::<Vec<_>>(),
        vec![(9, "\u{e000}"), (9, "\u{10000}"), (10, "ten")],
        "numeric code order and UTF-8 byte order must be explicit"
    );
}

#[test]
fn canonical_class_bytes_pin_sign_multiplicity_field_order_and_digest() {
    let case = basic_case();
    let row = diagnostic(DiagnosticPass::Semantic, 9, "head");
    let oracle = Comparison::Divergence(Divergence::Diagnostic(DiagnosticDivergence {
        tier: ComparisonTier::T0,
        pass: DiagnosticPass::Semantic,
        one_sided: vec![OneSidedDiagnostic {
            side: DifferenceSide::Oracle,
            diagnostic: row.clone(),
        }],
    }));
    let tsrs = Comparison::Divergence(Divergence::Diagnostic(DiagnosticDivergence {
        tier: ComparisonTier::T0,
        pass: DiagnosticPass::Semantic,
        one_sided: vec![OneSidedDiagnostic {
            side: DifferenceSide::Tsrs,
            diagnostic: row.clone(),
        }],
    }));
    let duplicated = Comparison::Divergence(Divergence::Diagnostic(DiagnosticDivergence {
        tier: ComparisonTier::T0,
        pass: DiagnosticPass::Semantic,
        one_sided: vec![
            OneSidedDiagnostic {
                side: DifferenceSide::Oracle,
                diagnostic: row.clone(),
            },
            OneSidedDiagnostic {
                side: DifferenceSide::Oracle,
                diagnostic: row,
            },
        ],
    }));

    let oracle = classify_case(&case, &oracle).unwrap().unwrap();
    let tsrs = classify_case(&case, &tsrs).unwrap().unwrap();
    let duplicated = classify_case(&case, &duplicated).unwrap().unwrap();
    let bytes = oracle.canonical_bytes().unwrap();
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        r#"{"schema":1,"failure":{"kind":"tier","tier":"t0"},"pass":"semantic","outcome":{"side":"oracle","kind":"diagnostic"},"rows":[{"side":"oracle","code":9,"normalized_message_head":"head"}],"renderer":null}"#
    );
    assert_eq!(oracle.canonical_sha256().unwrap(), sha256_hex(&bytes));
    assert_eq!(
        oracle.canonical_sha256().unwrap(),
        "40e763745f5b3539907018cc866d876cdd589f4a8be9cc4258975e0929049dc0"
    );
    assert_ne!(
        oracle.canonical_bytes().unwrap(),
        tsrs.canonical_bytes().unwrap()
    );
    assert_ne!(
        oracle.canonical_bytes().unwrap(),
        duplicated.canonical_bytes().unwrap()
    );
    assert_eq!(duplicated.rows.len(), 2);
}

#[test]
fn canonical_class_parser_rejects_unknown_noncanonical_and_incoherent_shapes() {
    let case = basic_case();
    let row = diagnostic(DiagnosticPass::Semantic, 9, "head");
    let comparison = Comparison::Divergence(Divergence::Diagnostic(DiagnosticDivergence {
        tier: ComparisonTier::T0,
        pass: DiagnosticPass::Semantic,
        one_sided: vec![OneSidedDiagnostic {
            side: DifferenceSide::Oracle,
            diagnostic: row,
        }],
    }));
    let class = classify_case(&case, &comparison).unwrap().unwrap();
    let canonical = class.canonical_bytes().unwrap();
    assert_eq!(
        CanonicalClass::from_canonical_slice(&canonical).unwrap(),
        class
    );

    let text = String::from_utf8(canonical).unwrap();
    let unknown = text.replacen(r#""schema":1"#, r#""schema":1,"unknown":0"#, 1);
    assert!(CanonicalClass::from_json_slice(unknown.as_bytes()).is_err());
    assert!(CanonicalClass::from_json_slice(format!("{text}\n").as_bytes()).is_ok());
    assert!(CanonicalClass::from_canonical_slice(format!("{text}\n").as_bytes()).is_err());

    let mut wrong_kind = class.clone();
    wrong_kind.outcome.kind = "anything".to_owned();
    assert!(wrong_kind.validate().is_err());
    let mut wrong_side = class.clone();
    wrong_side.outcome.side = OutcomeSide::Tsrs;
    assert!(wrong_side.validate().is_err());
    let mut empty_head = class;
    empty_head.rows[0].normalized_message_head.clear();
    assert!(empty_head.validate().is_err());

    let mut unescaped_literal = serde_json::from_value::<CanonicalClass>(
        serde_json::from_str::<serde_json::Value>(&text).unwrap(),
    )
    .unwrap();
    unescaped_literal.rows[0].normalized_message_head = "<raw".to_owned();
    assert!(unescaped_literal.validate().is_err());
    unescaped_literal.rows[0].normalized_message_head = "<<raw".to_owned();
    unescaped_literal
        .validate()
        .expect("doubled '<' is the canonical literal encoding");
    unescaped_literal.rows[0].normalized_message_head = "<#4294967296#>".to_owned();
    assert!(
        unescaped_literal.validate().is_err(),
        "placeholder ordinals are bounded canonical u32 values"
    );
    unescaped_literal.rows[0].normalized_message_head = "<%0%>".to_owned();
    assert!(
        unescaped_literal.validate().is_err(),
        "terminal volatility placeholders cannot enter diagnostic classes"
    );
}

#[test]
fn committed_canonical_class_vectors_match_rust_and_node_once() {
    let crate_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = crate_root.join("vectors/canonical-class.schema1.json");
    let verifier_path = crate_root.join("vectors/verify-canonical-class.mjs");
    let fixture_bytes = std::fs::read(&fixture_path).unwrap();
    let fixture: serde_json::Value = serde_json::from_slice(&fixture_bytes).unwrap();
    assert_eq!(fixture["schema"], 1);
    let vectors = fixture["vectors"].as_array().unwrap();
    assert!(!vectors.is_empty());

    for vector in vectors {
        let id = vector["id"].as_str().unwrap();
        let expected = vector["canonical_utf8"].as_str().unwrap();
        let expected_sha256 = vector["sha256"].as_str().unwrap();
        let mut class: CanonicalClass = serde_json::from_value(vector["class"].clone()).unwrap();
        class.rows.sort();

        assert_eq!(
            class.canonical_bytes().unwrap(),
            expected.as_bytes(),
            "{id}: Rust canonical bytes"
        );
        assert_eq!(
            class.canonical_sha256().unwrap(),
            expected_sha256,
            "{id}: Rust SHA-256"
        );
        let decoded = CanonicalClass::from_canonical_slice(expected.as_bytes()).unwrap();
        assert_eq!(
            decoded.canonical_bytes().unwrap(),
            expected.as_bytes(),
            "{id}: canonical decode/roundtrip"
        );
    }

    let output = Command::new("node")
        .arg(&verifier_path)
        .arg(&fixture_path)
        .output()
        .unwrap_or_else(|error| panic!("Node is required for canonical vector parity: {error}"));
    assert!(
        output.status.success(),
        "Node canonical verifier failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(&format!(
        "verified {} canonical class schema-1 vectors",
        vectors.len()
    )));
}

#[test]
fn t4_order_and_dedupe_precede_rendered_text_noise() {
    let case = basic_case();
    let a = diagnostic(DiagnosticPass::Semantic, 1001, "a");
    let b = diagnostic(DiagnosticPass::Semantic, 1002, "b");
    let raw = vec![a.clone(), b.clone()];

    let order_oracle = observation(
        &raw,
        &[a.clone(), b.clone()],
        &[("main.ts", "oracle-a\n"), ("main.ts", "oracle-b\n")],
    );
    let order_tsrs = observation(
        &raw,
        &[b.clone(), a.clone()],
        &[("main.ts", "tsrs-b\n"), ("main.ts", "tsrs-a\n")],
    );
    let (class, affected) = renderer_class(
        compare_case(
            &case,
            &execution(
                completed_with_renderer(raw.clone(), order_oracle),
                completed_with_renderer(raw.clone(), order_tsrs),
            ),
        )
        .unwrap(),
    );
    assert_eq!(class, RendererDifference::Order);
    assert_eq!(affected.code, 1001);

    let duplicate_raw = vec![a.clone(), a.clone()];
    let dedupe_oracle = observation(
        &duplicate_raw,
        std::slice::from_ref(&a),
        &[("main.ts", "oracle\n")],
    );
    let dedupe_tsrs = observation(
        &duplicate_raw,
        &[a.clone(), a.clone()],
        &[("main.ts", "tsrs-1\n"), ("main.ts", "tsrs-2\n")],
    );
    let (class, _) = renderer_class(
        compare_case(
            &case,
            &execution(
                completed_with_renderer(duplicate_raw.clone(), dedupe_oracle),
                completed_with_renderer(duplicate_raw, dedupe_tsrs),
            ),
        )
        .unwrap(),
    );
    assert_eq!(class, RendererDifference::Dedupe);
}

#[test]
fn t4_effective_identity_handles_cross_pass_empty_and_canonical_head_cases() {
    let case = basic_case();
    let syntactic = diagnostic(DiagnosticPass::Syntactic, 1001, "same");
    let semantic = diagnostic(DiagnosticPass::Semantic, 1001, "same");
    let cross_pass_raw = vec![syntactic.clone(), semantic.clone()];
    let oracle = observation(
        &cross_pass_raw,
        std::slice::from_ref(&syntactic),
        &[("main.ts", "same\n")],
    );
    let tsrs = observation(
        &cross_pass_raw,
        &[syntactic.clone(), semantic.clone()],
        &[("main.ts", "same\n"), ("main.ts", "same\n")],
    );
    let (class, affected) = renderer_class(
        compare_case(
            &case,
            &execution(
                completed_with_renderer(cross_pass_raw.clone(), oracle),
                completed_with_renderer(cross_pass_raw, tsrs),
            ),
        )
        .unwrap(),
    );
    assert_eq!(class, RendererDifference::Dedupe);
    assert_eq!(affected.code, 1001);

    let row = diagnostic(DiagnosticPass::Semantic, 2001, "empty");
    let raw = vec![row.clone()];
    let oracle = observation(&raw, std::slice::from_ref(&row), &[("main.ts", "")]);
    let tsrs = observation(&raw, &[], &[]);
    let (class, affected) = renderer_class(
        compare_case(
            &case,
            &execution(
                completed_with_renderer(raw.clone(), oracle),
                completed_with_renderer(raw, tsrs),
            ),
        )
        .unwrap(),
    );
    assert_eq!(
        class,
        RendererDifference::Dedupe,
        "structured final-sequence loss must remain visible with equal empty aggregates"
    );
    assert_eq!(affected.code, 2001);

    let left = diagnostic(DiagnosticPass::Semantic, 3001, "raw-left");
    let right = diagnostic(DiagnosticPass::Semantic, 3002, "raw-right");
    let raw = vec![left.clone(), right.clone()];
    let mut oracle = observation(
        &raw,
        std::slice::from_ref(&left),
        &[("main.ts", "oracle representative\n")],
    );
    let mut tsrs = observation(
        &raw,
        std::slice::from_ref(&right),
        &[("main.ts", "tsrs representative\n")],
    );
    for captured in oracle.deduped.iter_mut().chain(
        oracle
            .segments
            .iter_mut()
            .map(|segment| &mut segment.diagnostic),
    ) {
        captured.canonical_head = CanonicalHead::present(9999, "canonical");
    }
    for captured in tsrs.deduped.iter_mut().chain(
        tsrs.segments
            .iter_mut()
            .map(|segment| &mut segment.diagnostic),
    ) {
        captured.canonical_head = CanonicalHead::present(9999, "canonical");
    }
    let (class, affected) = renderer_class(
        compare_case(
            &case,
            &execution(
                completed_with_renderer(raw.clone(), oracle),
                completed_with_renderer(raw, tsrs),
            ),
        )
        .unwrap(),
    );
    assert_eq!(
        class,
        RendererDifference::Text,
        "different representatives of one effective tsc key fall through to rendered bytes"
    );
    assert_eq!(affected.code, 3001);
}

#[test]
fn t4_uses_global_path_newline_text_precedence_and_exact_segment_key() {
    let case = basic_case();
    let first = diagnostic(DiagnosticPass::Semantic, 1001, "first");
    let mut second = diagnostic(DiagnosticPass::Semantic, 1002, "second");
    second.file = DiagnosticFile::File {
        path: "other.ts".to_owned(),
    };
    let raw = vec![first.clone(), second.clone()];

    let oracle = observation(
        &raw,
        &raw,
        &[
            ("main.ts", "unrelated-oracle-text\n"),
            ("other.ts", "other.ts:2:1 - error\n"),
        ],
    );
    let tsrs = observation(
        &raw,
        &raw,
        &[
            ("main.ts", "unrelated-tsrs-text\n"),
            ("/work/other.ts", "/work/other.ts:2:1 - error\n"),
        ],
    );
    let (class, affected) = renderer_class(
        compare_case(
            &case,
            &execution(
                completed_with_renderer(raw.clone(), oracle),
                completed_with_renderer(raw.clone(), tsrs),
            ),
        )
        .unwrap(),
    );
    assert_eq!(
        class,
        RendererDifference::Text,
        "aggregate path normalization does not hide an independent text delta"
    );
    assert_eq!(affected.code, 1001);

    let oracle = observation(
        &raw,
        &raw,
        &[
            ("main.ts", "same\n"),
            ("other.ts", "other.ts:2:1 - error\n"),
        ],
    );
    let tsrs = observation(
        &raw,
        &raw,
        &[
            ("main.ts", "same\n"),
            ("/work/other.ts", "/work/other.ts:2:1 - error\n"),
        ],
    );
    let (class, affected) = renderer_class(
        compare_case(
            &case,
            &execution(
                completed_with_renderer(raw.clone(), oracle),
                completed_with_renderer(raw.clone(), tsrs),
            ),
        )
        .unwrap(),
    );
    assert_eq!(
        class,
        RendererDifference::Path,
        "the whole aggregate becomes equal after path normalization"
    );
    assert_eq!(affected.code, 1002);

    let oracle = observation(
        &raw,
        &raw,
        &[("main.ts", "same\r\n"), ("other.ts", "later-oracle-text\n")],
    );
    let tsrs = observation(
        &raw,
        &raw,
        &[("main.ts", "same\n"), ("other.ts", "later-tsrs-text\n")],
    );
    let (class, affected) = renderer_class(
        compare_case(
            &case,
            &execution(
                completed_with_renderer(raw.clone(), oracle),
                completed_with_renderer(raw.clone(), tsrs),
            ),
        )
        .unwrap(),
    );
    assert_eq!(class, RendererDifference::Text);
    assert_eq!(affected.code, 1001);

    let oracle = observation(
        &raw,
        &raw,
        &[("main.ts", "same\r\n"), ("other.ts", "same\n")],
    );
    let tsrs = observation(&raw, &raw, &[("main.ts", "same\n"), ("other.ts", "same\n")]);
    let (class, affected) = renderer_class(
        compare_case(
            &case,
            &execution(
                completed_with_renderer(raw.clone(), oracle),
                completed_with_renderer(raw.clone(), tsrs),
            ),
        )
        .unwrap(),
    );
    assert_eq!(
        class,
        RendererDifference::Newline,
        "the whole aggregate becomes equal after LF normalization"
    );
    assert_eq!(affected.code, 1001);

    let oracle = observation(
        &raw,
        &raw,
        &[("main.ts", "same\n"), ("other.ts", "oracle-text\n")],
    );
    let tsrs = observation(
        &raw,
        &raw,
        &[("main.ts", "same\n"), ("other.ts", "tsrs-text\n")],
    );
    let (class, affected) = renderer_class(
        compare_case(
            &case,
            &execution(
                completed_with_renderer(raw.clone(), oracle),
                completed_with_renderer(raw, tsrs),
            ),
        )
        .unwrap(),
    );
    assert_eq!(class, RendererDifference::Text);
    assert_eq!(affected.code, 1002);
}

#[test]
fn t4_global_path_and_newline_classes_are_total_across_segment_boundaries() {
    let case = basic_case();
    let first = diagnostic(DiagnosticPass::Semantic, 1001, "first");
    let second = diagnostic(DiagnosticPass::Semantic, 1002, "second");
    let raw = vec![first, second];

    let path_oracle = observation(&raw, &raw, &[("main.ts", "main"), ("main.ts", ".ts")]);
    let path_tsrs = observation(&raw, &raw, &[("main.ts", "/work/main"), ("main.ts", ".ts")]);
    let (class, affected) = renderer_class(
        compare_case(
            &case,
            &execution(
                completed_with_renderer(raw.clone(), path_oracle),
                completed_with_renderer(raw.clone(), path_tsrs),
            ),
        )
        .unwrap(),
    );
    assert_eq!(class, RendererDifference::Path);
    assert_eq!(
        affected.code, 1001,
        "the first raw-differing segment owns an aggregate-spanning path delta"
    );

    let newline_oracle = observation(&raw, &raw, &[("main.ts", "\r"), ("main.ts", "\n")]);
    let newline_tsrs = observation(&raw, &raw, &[("main.ts", ""), ("main.ts", "\n")]);
    let (class, affected) = renderer_class(
        compare_case(
            &case,
            &execution(
                completed_with_renderer(raw.clone(), newline_oracle),
                completed_with_renderer(raw, newline_tsrs),
            ),
        )
        .unwrap(),
    );
    assert_eq!(class, RendererDifference::Newline);
    assert_eq!(
        affected.code, 1001,
        "the first raw-differing segment owns an aggregate-spanning CRLF delta"
    );
}

#[test]
fn pure_t4_class_uses_aggregate_pass_and_position_free_affected_key() {
    let case = basic_case();
    let row = diagnostic(
        DiagnosticPass::Suggestion,
        80001,
        "generatedName in main.ts",
    );
    let oracle = observation(
        std::slice::from_ref(&row),
        std::slice::from_ref(&row),
        &[("main.ts", "oracle\n")],
    );
    let tsrs = observation(
        std::slice::from_ref(&row),
        std::slice::from_ref(&row),
        &[("main.ts", "tsrs\n")],
    );
    let comparison = compare_case(
        &case,
        &execution(
            completed_with_renderer(vec![row.clone()], oracle),
            completed_with_renderer(vec![row], tsrs),
        ),
    )
    .unwrap();
    let class = classify_case(&case, &comparison).unwrap().unwrap();
    assert_eq!(
        class.failure,
        ClassFailure::Tier {
            tier: ComparisonTier::T4
        }
    );
    assert_eq!(class.pass, ClassPass::AggregateRender);
    assert_eq!(class.outcome.side, OutcomeSide::Both);
    assert!(class.rows.is_empty());
    let renderer = class.renderer.unwrap();
    assert_eq!(renderer.class, RendererDifference::Text);
    assert_eq!(renderer.affected_key.code, 80001);
    assert_eq!(
        renderer.affected_key.normalized_message_head,
        "<#0#> in <@2:0@>"
    );
}

#[test]
fn terminal_outcomes_are_typed_normalized_and_invalid_observations_have_no_class() {
    let case = basic_case();
    let oracle = completed(Vec::new());
    for phase in [
        TerminalPhase::Parse,
        TerminalPhase::Bind,
        TerminalPhase::Check,
        TerminalPhase::Format,
    ] {
        for kind in [
            TerminalKind::Panic,
            TerminalKind::Crash,
            TerminalKind::Timeout,
            TerminalKind::Oom,
            TerminalKind::Unsupported,
        ] {
            let execution = CaseExecution::Compared {
                oracle: EngineResult::Completed {
                    outcome: oracle.clone(),
                },
                tsrs: EngineResult::Terminal {
                    outcome: TerminalOutcome {
                        phase,
                        kind,
                        boundary_id: match kind {
                            TerminalKind::Panic => TerminalBoundaryId::PhaseInvariant,
                            TerminalKind::Crash => TerminalBoundaryId::ProcessSignal,
                            TerminalKind::Timeout => TerminalBoundaryId::Deadline,
                            TerminalKind::Oom => TerminalBoundaryId::AllocationLimit,
                            TerminalKind::Unsupported => TerminalBoundaryId::FeatureGate,
                        },
                        detail: "raw process detail".to_owned(),
                    },
                },
            };
            let comparison = compare_case(&case, &execution).unwrap();
            let class = classify_case(&case, &comparison).unwrap().unwrap();
            assert_eq!(class.failure, ClassFailure::Terminal { phase });
            assert_eq!(class.pass, ClassPass::Terminal);
            assert_eq!(class.outcome.side, OutcomeSide::Tsrs);
            assert!(class.rows.is_empty());
        }
    }

    let oracle_terminal = CaseExecution::Compared {
        oracle: EngineResult::Terminal {
            outcome: TerminalOutcome {
                phase: TerminalPhase::Parse,
                kind: TerminalKind::Crash,
                boundary_id: TerminalBoundaryId::ProcessSignal,
                detail: "oracle crash".to_owned(),
            },
        },
        tsrs: EngineResult::Completed {
            outcome: oracle.clone(),
        },
    };
    assert!(compare_case(&case, &oracle_terminal).is_err());

    for (source, kind) in [
        (
            ProducerFailureSource::Generator,
            ProducerFailureKind::Generator,
        ),
        (
            ProducerFailureSource::DomainValidator,
            ProducerFailureKind::Domain,
        ),
        (ProducerFailureSource::Harness, ProducerFailureKind::Harness),
        (
            ProducerFailureSource::OracleAdapter,
            ProducerFailureKind::MalformedResponse,
        ),
        (
            ProducerFailureSource::TsrsAdapter,
            ProducerFailureKind::MalformedResponse,
        ),
        (
            ProducerFailureSource::Controller,
            ProducerFailureKind::Controller,
        ),
        (
            ProducerFailureSource::Worker,
            ProducerFailureKind::WorkerInterruption,
        ),
    ] {
        let producer_failure = CaseExecution::ProducerFailure {
            failure: ProducerFailure {
                source,
                kind,
                detail: "producer failure".to_owned(),
            },
        };
        assert!(compare_case(&case, &producer_failure).is_err());
    }

    let incoherent = ProducerFailure {
        source: ProducerFailureSource::Generator,
        kind: ProducerFailureKind::WorkerInterruption,
        detail: "wrong pair".to_owned(),
    };
    assert!(incoherent.validate().is_err());
    assert!(classify_case(&case, &Comparison::Exact).unwrap().is_none());
    let mut invalid_case = case;
    invalid_case.schema = 99;
    assert!(
        classify_case(&invalid_case, &Comparison::Exact).is_err(),
        "even an exact standalone classification validates its CaseSpec"
    );
}

#[test]
fn terminal_class_uses_only_the_closed_boundary_and_not_raw_volatility() {
    let left_case = case_with(
        "/left",
        "leftName",
        12_345_678,
        87_654_321,
        &[("left.ts", BASIC_SOURCE)],
    );
    let right_case = case_with(
        "/right",
        "rightName",
        23_456_789,
        98_765_432,
        &[("right.ts", BASIC_SOURCE)],
    );
    let terminal = |boundary_id: TerminalBoundaryId, detail: &str| {
        Comparison::Divergence(Divergence::TsrsTerminal(TerminalOutcome {
            phase: TerminalPhase::Format,
            kind: TerminalKind::Panic,
            boundary_id,
            detail: detail.to_owned(),
        }))
    };
    let left = classify_case(
        &left_case,
        &terminal(
            TerminalBoundaryId::RendererInvariant,
            "panic /left/left.ts leftName seed 1 case 2 at 0xabcdef timestamp 123",
        ),
    )
    .unwrap()
    .unwrap();
    let right = classify_case(
        &right_case,
        &terminal(
            TerminalBoundaryId::RendererInvariant,
            "panic /right/right.ts rightName seed 999 case 42 at 0x123456 timestamp 456",
        ),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        left.canonical_bytes().unwrap(),
        right.canonical_bytes().unwrap()
    );

    let semantic_change = classify_case(
        &right_case,
        &terminal(
            TerminalBoundaryId::RendererState,
            "same volatile process detail",
        ),
    )
    .unwrap()
    .unwrap();
    assert_ne!(
        right.canonical_bytes().unwrap(),
        semantic_change.canonical_bytes().unwrap()
    );

    let incoherent = TerminalOutcome {
        phase: TerminalPhase::Format,
        kind: TerminalKind::Panic,
        boundary_id: TerminalBoundaryId::Deadline,
        detail: "raw".to_owned(),
    };
    assert!(
        EngineResult::Terminal {
            outcome: incoherent
        }
        .validate("terminal")
        .is_err(),
        "closed boundaries are valid only for their owning terminal kind"
    );
    let volatile = serde_json::json!({
        "status": "terminal",
        "outcome": {
            "phase": "format",
            "kind": "panic",
            "boundary_id": "seed42",
            "detail": "raw"
        }
    });
    assert!(
        serde_json::from_value::<EngineResult>(volatile).is_err(),
        "arbitrary seed/timestamp/hash text cannot inhabit the boundary enum"
    );
}

#[test]
fn engine_outcome_bytes_bind_schema_case_hash_and_raw_multiplicity() {
    let case = basic_case();
    let row = diagnostic(DiagnosticPass::Semantic, 1001, "row");
    let one = EngineResult::Completed {
        outcome: completed(vec![row.clone()]),
    };
    let two = EngineResult::Completed {
        outcome: completed(vec![row.clone(), row]),
    };
    let one_bytes = one.canonical_bytes(&case).unwrap();
    let text = String::from_utf8(one_bytes.clone()).unwrap();
    assert!(text.starts_with(r#"{"schema":1,"case_sha256":""#));
    assert!(text.contains(&case.canonical_sha256().unwrap()));
    assert_eq!(one.canonical_sha256(&case).unwrap(), sha256_hex(&one_bytes));
    assert_ne!(
        one.canonical_bytes(&case).unwrap(),
        two.canonical_bytes(&case).unwrap(),
        "raw outcome multiplicity is observable"
    );
}

#[test]
fn case_execution_envelope_binds_both_sides_and_producer_failures_to_the_case() {
    let case = basic_case();
    let row = diagnostic(DiagnosticPass::Semantic, 1001, "row");
    let compared = execution(
        completed(vec![row.clone()]),
        completed(vec![row.clone(), row]),
    );
    let compared_bytes = compared.canonical_bytes(&case).unwrap();
    assert_eq!(
        CaseExecution::from_canonical_slice(&case, &compared_bytes).unwrap(),
        compared
    );
    assert_eq!(
        compared.canonical_sha256(&case).unwrap(),
        sha256_hex(&compared_bytes)
    );
    assert!(CaseExecution::from_canonical_slice(
        &case,
        format!("{}\n", String::from_utf8(compared_bytes.clone()).unwrap()).as_bytes(),
    )
    .is_err());

    let other_case = case_with(
        "/other",
        "otherName",
        1,
        2,
        &[("main.ts", BASIC_SOURCE), ("other.ts", BASIC_SOURCE)],
    );
    assert!(CaseExecution::from_canonical_slice(&other_case, &compared_bytes).is_err());

    let producer_failure = CaseExecution::ProducerFailure {
        failure: ProducerFailure {
            source: ProducerFailureSource::Generator,
            kind: ProducerFailureKind::Generator,
            detail: "decision stream exhausted".to_owned(),
        },
    };
    let failure_bytes = producer_failure.canonical_bytes(&case).unwrap();
    assert_eq!(
        CaseExecution::from_canonical_slice(&case, &failure_bytes).unwrap(),
        producer_failure,
        "invalid comparisons still retain case-bound raw evidence"
    );
}

#[test]
fn atomic_evaluation_derives_evidence_comparison_and_class_from_one_execution() {
    let case = basic_case();
    let row = diagnostic(DiagnosticPass::Semantic, 2322, "generatedName in main.ts");
    let execution = execution(completed(vec![row]), completed(Vec::new()));
    let evaluated = evaluate_case(&case, &execution).unwrap();
    let standalone_comparison = compare_case(&case, &execution).unwrap();
    let standalone_class = classify_case(&case, &standalone_comparison)
        .unwrap()
        .unwrap();

    assert_eq!(
        evaluated.execution_canonical_bytes(),
        execution.canonical_bytes(&case).unwrap()
    );
    assert_eq!(
        evaluated.execution_sha256(),
        sha256_hex(evaluated.execution_canonical_bytes())
    );
    assert_eq!(evaluated.comparison(), &standalone_comparison);
    assert_eq!(evaluated.canonical_class(), Some(&standalone_class));
}
