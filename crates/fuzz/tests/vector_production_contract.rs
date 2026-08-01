use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use tsc_fuzz::classify::{CanonicalClass, ClassFailure};
use tsc_fuzz::compare::ComparisonTier;
use tsc_fuzz::evaluate::evaluate_case;
use tsc_fuzz::model::{
    AssembledDiagnostic, CanonicalHead, CaseExecution, CompletedOutcome, DiagnosticCategory,
    DiagnosticFile, DiagnosticPass, DiagnosticRecord, EngineResult, MessageChain, OptionalBool,
    OptionalString, OptionalU32, RelatedDiagnostic, RenderSegment, RendererObservation,
    TerminalBoundaryId, TerminalKind, TerminalOutcome, TerminalPhase,
};
use tsc_fuzz::normalize::NormalizationSpec;
use tsc_fuzz::schema::{
    CanonicalU64, CaseProvenance, CaseSpec, ChildProcessPolicy, DecisionValue, DomainMembership,
    EncodedFile, NodeProcessPolicy, OrderedArgument, ProcessPolicy, RustProcessPolicy,
    StableDecision, CASE_SPEC_SCHEMA,
};

const SOURCE_LINE_WIDTH: u32 = 128;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    schema: u32,
    rejection_canaries: RejectionCanaries,
    vectors: Vec<Vector>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RejectionCanaries {
    terminal_boundary_ids: Vec<String>,
    normalization_cross_role_source: String,
    renderer_foreign_deduped: RendererForeignDedupedCanary,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RendererForeignDedupedCanary {
    assembled_id: String,
    foreign_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vector {
    id: String,
    raw: RawVector,
    class: CanonicalClass,
    canonical_utf8: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum RawVector {
    Diagnostic {
        normalization: RawNormalization,
        diagnostics: Vec<RawDiagnostic>,
    },
    Renderer {
        normalization: RawNormalization,
        diagnostics: Vec<RawRendererDiagnostic>,
        oracle: RawRendererObservation,
        tsrs: RawRendererObservation,
    },
    Terminal {
        oracle: RawTerminalSide,
        tsrs: RawTerminalSide,
    },
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNormalization {
    paths: Vec<RawMapping>,
    generated_identifiers: Vec<RawMapping>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMapping {
    from: String,
    to: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawEngine {
    Oracle,
    Tsrs,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDiagnostic {
    engine: RawEngine,
    pass: DiagnosticPass,
    file: String,
    code: u32,
    line: u32,
    col: u32,
    category: DiagnosticCategory,
    start: u32,
    length: u32,
    head: String,
    chain: String,
    related: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRendererDiagnostic {
    id: String,
    file: String,
    resolved_file: String,
    start: u32,
    length: u32,
    code: u32,
    top_text: String,
    canonical_head: Option<RawCanonicalHead>,
    pass: DiagnosticPass,
    category: DiagnosticCategory,
    tail: String,
    related: Vec<String>,
    flags: Vec<RawDiagnosticFlag>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCanonicalHead {
    code: u32,
    message_text: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawDiagnosticFlag {
    ReportsUnnecessary,
    ReportsDeprecated,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRendererObservation {
    assembled: Vec<String>,
    deduped: Vec<String>,
    aggregate_text: String,
    segments: Vec<RawRenderSegment>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRenderSegment {
    diagnostic: String,
    raw_text: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum RawTerminalSide {
    Completed,
    Terminal {
        phase: TerminalPhase,
        terminal_kind: TerminalKind,
        boundary_id: TerminalBoundaryId,
    },
}

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

fn source_text() -> String {
    let line = "x".repeat(usize::try_from(SOURCE_LINE_WIDTH).unwrap());
    format!("{line}\n{line}\n")
}

fn decisions(normalization: Option<&RawNormalization>) -> Vec<StableDecision> {
    let Some(normalization) = normalization else {
        return vec![StableDecision {
            ordinal: 0,
            id: "fixture-control".to_owned(),
            value: DecisionValue::Boolean { value: true },
        }];
    };
    if normalization.generated_identifiers.is_empty() {
        return vec![StableDecision {
            ordinal: 0,
            id: "fixture-control".to_owned(),
            value: DecisionValue::Boolean { value: true },
        }];
    }
    normalization
        .generated_identifiers
        .iter()
        .enumerate()
        .map(|(index, mapping)| {
            assert_eq!(
                mapping.to,
                format!("<#{index}#>"),
                "fixture identifier placeholders must follow CaseSpec decision ordinals"
            );
            StableDecision {
                ordinal: u32::try_from(index).unwrap(),
                id: format!("fixture-identifier-{index}"),
                value: DecisionValue::Identifier {
                    value: mapping.from.clone(),
                },
            }
        })
        .collect()
}

fn observed_files(raw: &RawVector) -> BTreeSet<&str> {
    match raw {
        RawVector::Diagnostic { diagnostics, .. } => {
            diagnostics.iter().map(|row| row.file.as_str()).collect()
        }
        RawVector::Renderer { diagnostics, .. } => {
            diagnostics.iter().map(|row| row.file.as_str()).collect()
        }
        RawVector::Terminal { .. } => BTreeSet::from(["main.ts"]),
    }
}

fn path_file_ordinal(placeholder: &str) -> Option<usize> {
    placeholder
        .strip_prefix("<@2:")?
        .strip_suffix("@>")?
        .parse()
        .ok()
}

fn owned_files(raw: &RawVector) -> Vec<String> {
    let mut mapped = BTreeMap::<usize, String>::new();
    if let Some(normalization) = normalization(raw) {
        for mapping in &normalization.paths {
            let Some(ordinal) = path_file_ordinal(&mapping.to) else {
                continue;
            };
            if mapping.from.starts_with('/') {
                continue;
            }
            let previous = mapped.insert(ordinal, mapping.from.clone());
            assert!(
                previous.is_none(),
                "fixture has multiple public names for file placeholder {}",
                mapping.to
            );
        }
    }
    if !mapped.is_empty() {
        for (expected, actual) in mapped.keys().copied().enumerate() {
            assert_eq!(
                actual, expected,
                "fixture file placeholders must be contiguous from zero"
            );
        }
        let files = mapped.into_values().collect::<Vec<_>>();
        for observed in observed_files(raw) {
            assert!(
                files.iter().any(|file| file == observed),
                "observed diagnostic file {observed:?} is absent from normalization-owned files"
            );
        }
        return files;
    }
    observed_files(raw).into_iter().map(str::to_owned).collect()
}

fn normalization(raw: &RawVector) -> Option<&RawNormalization> {
    match raw {
        RawVector::Diagnostic { normalization, .. } | RawVector::Renderer { normalization, .. } => {
            Some(normalization)
        }
        RawVector::Terminal { .. } => None,
    }
}

fn fixture_cwd(raw: &RawVector, files: &[String]) -> String {
    let resolved_and_file = match raw {
        RawVector::Renderer { diagnostics, .. } => diagnostics
            .first()
            .map(|diagnostic| (diagnostic.resolved_file.as_str(), diagnostic.file.as_str())),
        RawVector::Diagnostic { normalization, .. } => {
            normalization.paths.iter().find_map(|absolute| {
                if !absolute.from.starts_with('/') {
                    return None;
                }
                let ordinal = path_file_ordinal(&absolute.to)?;
                files
                    .get(ordinal)
                    .map(|file| (absolute.from.as_str(), file.as_str()))
            })
        }
        RawVector::Terminal { .. } => None,
    };
    let Some((resolved, file)) = resolved_and_file else {
        return "/work".to_owned();
    };
    let suffix = format!("/{file}");
    resolved
        .strip_suffix(&suffix)
        .unwrap_or_else(|| panic!("resolved fixture path {resolved:?} must end in {suffix:?}"))
        .to_owned()
}

fn case_for_vector(vector: &Vector) -> CaseSpec {
    let file_names = owned_files(&vector.raw);
    let cwd = fixture_cwd(&vector.raw, &file_names);
    let source = source_text();
    CaseSpec {
        schema: CASE_SPEC_SCHEMA,
        case_id: vector.id.clone(),
        generator_id: "class-vector-production-adapter".to_owned(),
        provenance: CaseProvenance {
            root_seed: CanonicalU64::new(10_000_001),
            case_index: CanonicalU64::new(0),
            case_seed: CanonicalU64::new(10_000_002),
        },
        decisions: decisions(normalization(&vector.raw)),
        domain_membership: vec![DomainMembership {
            ordinal: 0,
            id: "schema-one-class-vector".to_owned(),
        }],
        cwd,
        options: Vec::new(),
        libs: Vec::new(),
        files: file_names
            .into_iter()
            .enumerate()
            .map(|(index, name)| EncodedFile {
                ordinal: u32::try_from(index).unwrap(),
                name: name.to_owned(),
                text_base64: base64(source.as_bytes()),
            })
            .collect(),
        matrix_key: String::new(),
        matrix: Vec::new(),
        normalization_schema: 1,
        process_policy: process_policy(),
    }
}

fn child_chain(text: &str, code: u32, category: DiagnosticCategory) -> MessageChain {
    MessageChain {
        text: text.to_owned(),
        code,
        category,
        next_present: false,
        next: Vec::new(),
    }
}

fn chain(head: &str, tail: &str, code: u32, category: DiagnosticCategory) -> MessageChain {
    let next = (!tail.is_empty())
        .then(|| child_chain(tail, code, category))
        .into_iter()
        .collect::<Vec<_>>();
    MessageChain {
        text: head.to_owned(),
        code,
        category,
        next_present: !next.is_empty(),
        next,
    }
}

fn related(texts: &[String], code: u32, category: DiagnosticCategory) -> Vec<RelatedDiagnostic> {
    texts
        .iter()
        .map(|text| RelatedDiagnostic {
            file_present: false,
            file: None,
            start_present: false,
            start: None,
            length_present: false,
            length: None,
            code,
            category,
            chain: child_chain(text, code, category),
        })
        .collect()
}

fn present(value: u32) -> OptionalU32 {
    OptionalU32::Present { value }
}

fn diagnostic_record(raw: &RawDiagnostic, abstract_span_is_irrelevant: bool) -> DiagnosticRecord {
    assert!(
        raw.start.checked_add(raw.length).is_some(),
        "abstract fixture span must not overflow"
    );
    // The portable vector schema deliberately groups diagnostics by an
    // abstract (line,col), while CaseExecution requires one physically
    // coherent UTF-16 span. Map each abstract coordinate to one real span;
    // this preserves every comparison equivalence used by the vector.
    let start = raw
        .line
        .checked_mul(SOURCE_LINE_WIDTH + 1)
        .and_then(|line_start| line_start.checked_add(raw.col))
        .unwrap();
    assert!(
        raw.start == start || abstract_span_is_irrelevant,
        "an abstract start may differ from its physical span only below the vector's failing tier"
    );
    DiagnosticRecord {
        pass: raw.pass,
        file: DiagnosticFile::File {
            path: raw.file.clone(),
        },
        code: raw.code,
        line: present(raw.line),
        column: present(raw.col),
        category: raw.category,
        start: present(start),
        length: present(raw.length),
        chain: chain(&raw.head, &raw.chain, raw.code, raw.category),
        related_information_present: !raw.related.is_empty(),
        related: related(&raw.related, raw.code, raw.category),
        reports_unnecessary: OptionalBool::absent(),
        reports_deprecated: OptionalBool::absent(),
        source: OptionalString::absent(),
    }
}

fn renderer_diagnostic(raw: &RawRendererDiagnostic) -> AssembledDiagnostic {
    let mut reports_unnecessary = false;
    let mut reports_deprecated = false;
    for flag in &raw.flags {
        match flag {
            RawDiagnosticFlag::ReportsUnnecessary => {
                assert!(
                    !reports_unnecessary,
                    "raw renderer flags must not contain duplicate reports-unnecessary"
                );
                reports_unnecessary = true;
            }
            RawDiagnosticFlag::ReportsDeprecated => {
                assert!(
                    !reports_deprecated,
                    "raw renderer flags must not contain duplicate reports-deprecated"
                );
                reports_deprecated = true;
            }
        }
    }
    let diagnostic = DiagnosticRecord {
        pass: raw.pass,
        file: DiagnosticFile::File {
            path: raw.file.clone(),
        },
        code: raw.code,
        line: present(0),
        column: present(raw.start),
        category: raw.category,
        start: present(raw.start),
        length: present(raw.length),
        chain: chain(&raw.top_text, &raw.tail, raw.code, raw.category),
        related_information_present: !raw.related.is_empty(),
        related: related(&raw.related, raw.code, raw.category),
        reports_unnecessary: if reports_unnecessary {
            OptionalBool::present(true)
        } else {
            OptionalBool::absent()
        },
        reports_deprecated: if reports_deprecated {
            OptionalBool::present(true)
        } else {
            OptionalBool::absent()
        },
        source: OptionalString::absent(),
    };
    let canonical_head = raw
        .canonical_head
        .as_ref()
        .map_or_else(CanonicalHead::absent, |head| {
            CanonicalHead::present(head.code, &head.message_text)
        });
    AssembledDiagnostic {
        diagnostic,
        canonical_head,
    }
}

fn empty_observation() -> RendererObservation {
    RendererObservation {
        assembled: Vec::new(),
        deduped: Vec::new(),
        segments: Vec::new(),
        aggregate_text: String::new(),
    }
}

fn completed(diagnostics: Vec<DiagnosticRecord>, renderer: RendererObservation) -> EngineResult {
    EngineResult::Completed {
        outcome: CompletedOutcome {
            diagnostics,
            renderer,
        },
    }
}

fn completed_structured(diagnostics: Vec<DiagnosticRecord>) -> EngineResult {
    let assembled = diagnostics
        .iter()
        .cloned()
        .map(|diagnostic| AssembledDiagnostic {
            diagnostic,
            canonical_head: CanonicalHead::absent(),
        })
        .collect::<Vec<_>>();
    let segments = assembled
        .iter()
        .cloned()
        .map(|diagnostic| RenderSegment {
            diagnostic,
            raw_text: String::new(),
        })
        .collect();
    completed(
        diagnostics,
        RendererObservation {
            assembled: assembled.clone(),
            deduped: assembled,
            segments,
            aggregate_text: String::new(),
        },
    )
}

fn renderer_observation(
    raw: &RawRendererObservation,
    diagnostics: &BTreeMap<&str, AssembledDiagnostic>,
) -> RendererObservation {
    let lookup = |id: &str| {
        diagnostics
            .get(id)
            .unwrap_or_else(|| panic!("renderer references unknown diagnostic {id:?}"))
            .clone()
    };
    RendererObservation {
        assembled: raw.iter_ids(&raw.assembled, &lookup),
        deduped: raw.iter_ids(&raw.deduped, &lookup),
        segments: raw
            .segments
            .iter()
            .map(|segment| RenderSegment {
                diagnostic: lookup(&segment.diagnostic),
                raw_text: segment.raw_text.clone(),
            })
            .collect(),
        aggregate_text: raw.aggregate_text.clone(),
    }
}

fn renderer_completed(
    raw: &RawRendererObservation,
    diagnostics: &BTreeMap<&str, AssembledDiagnostic>,
) -> EngineResult {
    let renderer = renderer_observation(raw, diagnostics);
    let structured = renderer
        .assembled
        .iter()
        .map(|assembled| assembled.diagnostic.clone())
        .collect();
    completed(structured, renderer)
}

impl RawRendererObservation {
    fn iter_ids(
        &self,
        ids: &[String],
        lookup: &impl Fn(&str) -> AssembledDiagnostic,
    ) -> Vec<AssembledDiagnostic> {
        ids.iter().map(|id| lookup(id)).collect()
    }
}

fn terminal_side(raw: &RawTerminalSide) -> EngineResult {
    match raw {
        RawTerminalSide::Completed => completed(Vec::new(), empty_observation()),
        RawTerminalSide::Terminal {
            phase,
            terminal_kind,
            boundary_id,
        } => EngineResult::Terminal {
            outcome: TerminalOutcome {
                phase: *phase,
                kind: *terminal_kind,
                boundary_id: *boundary_id,
                detail: "raw fixture terminal witness".to_owned(),
            },
        },
    }
}

fn execution_for_vector(vector: &Vector, case: &CaseSpec) -> CaseExecution {
    match &vector.raw {
        RawVector::Diagnostic { diagnostics, .. } => {
            let mut oracle = Vec::new();
            let mut tsrs = Vec::new();
            let abstract_span_is_irrelevant = matches!(
                &vector.class.failure,
                ClassFailure::Tier {
                    tier: ComparisonTier::T0 | ComparisonTier::T1
                }
            );
            for raw in diagnostics {
                match raw.engine {
                    RawEngine::Oracle => {
                        oracle.push(diagnostic_record(raw, abstract_span_is_irrelevant))
                    }
                    RawEngine::Tsrs => {
                        tsrs.push(diagnostic_record(raw, abstract_span_is_irrelevant))
                    }
                }
            }
            CaseExecution::Compared {
                oracle: completed_structured(oracle),
                tsrs: completed_structured(tsrs),
            }
        }
        RawVector::Renderer {
            diagnostics,
            oracle,
            tsrs,
            ..
        } => {
            let mut by_id = BTreeMap::new();
            for raw in diagnostics {
                assert_eq!(
                    case.resolved_file_name(&raw.file).unwrap(),
                    raw.resolved_file,
                    "{}: renderer resolved path",
                    vector.id
                );
                assert!(
                    by_id
                        .insert(raw.id.as_str(), renderer_diagnostic(raw))
                        .is_none(),
                    "{}: duplicate renderer diagnostic id {:?}",
                    vector.id,
                    raw.id
                );
            }
            CaseExecution::Compared {
                oracle: renderer_completed(oracle, &by_id),
                tsrs: renderer_completed(tsrs, &by_id),
            }
        }
        RawVector::Terminal { oracle, tsrs } => CaseExecution::Compared {
            oracle: terminal_side(oracle),
            tsrs: terminal_side(tsrs),
        },
    }
}

fn assert_normalization_contract(vector: &Vector, case: &CaseSpec) {
    let Some(raw) = normalization(&vector.raw) else {
        return;
    };
    let derived = NormalizationSpec::for_case(case).unwrap();
    for mapping in &raw.paths {
        assert_eq!(
            derived.normalize_exact_path(&mapping.from).unwrap(),
            mapping.to,
            "{}: path normalization for {:?}",
            vector.id,
            mapping.from
        );
    }
    for mapping in &raw.generated_identifiers {
        assert_eq!(
            derived.normalize(&mapping.from).unwrap(),
            mapping.to,
            "{}: generated identifier normalization for {:?}",
            vector.id,
            mapping.from
        );
    }
}

fn assert_rejection_canaries(canaries: &RejectionCanaries) {
    assert!(!canaries.terminal_boundary_ids.is_empty());
    for boundary in &canaries.terminal_boundary_ids {
        assert!(
            serde_json::from_value::<TerminalBoundaryId>(serde_json::Value::String(
                boundary.clone()
            ))
            .is_err(),
            "terminal boundary {boundary:?} must be outside the closed Rust enum"
        );
    }

    let source = source_text();
    let collision = CaseSpec {
        schema: CASE_SPEC_SCHEMA,
        case_id: "normalization-role-collision".to_owned(),
        generator_id: "class-vector-production-adapter".to_owned(),
        provenance: CaseProvenance {
            root_seed: CanonicalU64::new(10_000_001),
            case_index: CanonicalU64::new(0),
            case_seed: CanonicalU64::new(10_000_002),
        },
        decisions: vec![StableDecision {
            ordinal: 0,
            id: "fixture-identifier-0".to_owned(),
            value: DecisionValue::Identifier {
                value: canaries.normalization_cross_role_source.clone(),
            },
        }],
        domain_membership: vec![DomainMembership {
            ordinal: 0,
            id: "schema-one-class-vector".to_owned(),
        }],
        cwd: "/work".to_owned(),
        options: Vec::new(),
        libs: Vec::new(),
        files: vec![EncodedFile {
            ordinal: 0,
            name: canaries.normalization_cross_role_source.clone(),
            text_base64: base64(source.as_bytes()),
        }],
        matrix_key: String::new(),
        matrix: Vec::new(),
        normalization_schema: 1,
        process_policy: process_policy(),
    };
    assert!(
        collision.validate().is_err(),
        "one raw source cannot be both a path and generated identifier"
    );

    assert_ne!(
        canaries.renderer_foreign_deduped.assembled_id,
        canaries.renderer_foreign_deduped.foreign_id
    );
    let diagnostic = |code, text: &str| DiagnosticRecord {
        pass: DiagnosticPass::Semantic,
        file: DiagnosticFile::File {
            path: "main.ts".to_owned(),
        },
        code,
        line: present(0),
        column: present(0),
        category: DiagnosticCategory::Error,
        start: present(0),
        length: present(1),
        chain: child_chain(text, code, DiagnosticCategory::Error),
        related_information_present: false,
        related: Vec::new(),
        reports_unnecessary: OptionalBool::absent(),
        reports_deprecated: OptionalBool::absent(),
        source: OptionalString::absent(),
    };
    let assembled = AssembledDiagnostic {
        diagnostic: diagnostic(1, &canaries.renderer_foreign_deduped.assembled_id),
        canonical_head: CanonicalHead::absent(),
    };
    let foreign = AssembledDiagnostic {
        diagnostic: diagnostic(2, &canaries.renderer_foreign_deduped.foreign_id),
        canonical_head: CanonicalHead::absent(),
    };
    let foreign_deduped = CompletedOutcome {
        diagnostics: vec![assembled.diagnostic.clone()],
        renderer: RendererObservation {
            assembled: vec![assembled],
            deduped: vec![foreign.clone()],
            segments: vec![RenderSegment {
                diagnostic: foreign,
                raw_text: String::new(),
            }],
            aggregate_text: String::new(),
        },
    };
    assert!(
        foreign_deduped.validate("foreign-deduped-canary").is_err(),
        "a final renderer row must select an assembled diagnostic"
    );
}

#[test]
fn committed_raw_vectors_rederive_through_rust_production_path() {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("vectors/canonical-class.schema1.json");
    let fixture: Fixture = serde_json::from_slice(&std::fs::read(&fixture_path).unwrap()).unwrap();
    assert_eq!(fixture.schema, 1);
    assert_rejection_canaries(&fixture.rejection_canaries);
    assert!(!fixture.vectors.is_empty());

    for vector in fixture.vectors {
        let case = case_for_vector(&vector);
        case.validate()
            .unwrap_or_else(|error| panic!("{}: invalid adapted CaseSpec: {error}", vector.id));
        assert_normalization_contract(&vector, &case);

        let execution = execution_for_vector(&vector, &case);
        execution.validate_for_case(&case).unwrap_or_else(|error| {
            panic!("{}: invalid adapted CaseExecution: {error}", vector.id)
        });
        let evaluated = evaluate_case(&case, &execution)
            .unwrap_or_else(|error| panic!("{}: production evaluation failed: {error}", vector.id));
        let actual = evaluated
            .canonical_class()
            .cloned()
            .unwrap_or_else(|| panic!("{}: raw vector unexpectedly compared exact", vector.id));

        let mut expected = vector.class;
        expected.rows.sort();
        assert_eq!(actual, expected, "{}: derived canonical class", vector.id);
        assert_eq!(
            actual.canonical_bytes().unwrap(),
            vector.canonical_utf8.as_bytes(),
            "{}: derived canonical bytes",
            vector.id
        );
        assert_eq!(
            actual.canonical_sha256().unwrap(),
            vector.sha256,
            "{}: derived canonical SHA-256",
            vector.id
        );
    }
}
