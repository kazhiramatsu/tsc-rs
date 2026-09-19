//! UTF-16 adjacent repair B: parser-owned literal-only recovery admission
//! census over the acceptance corpus.
//!
//! Before the repair, `preflight_source` refused every source with a retained
//! parse diagnostic. It now admits a source exactly when the committed parser
//! recovery record is literal-only (`SourceFile::has_only_literal_recovery`).
//! This census evaluates that predicate with the acceptance's own loaders and
//! parse projection over every corpus row whose inputs are recorded, so the
//! before/after admission delta is enumerated instead of inferred from
//! diagnostic codes. It performs no emit, no TypeScript run and no CI.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use base64::Engine as _;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tsc_harness::upstream_suites::execution::{
    load_compiler_emit, load_compiler_no_emit, load_project_emit, load_project_no_emit,
    load_qualified_compiler_emit_with_symlinks, load_recorded_execution_plans,
    observable_input::{self, limits},
    EmitOptionFloor, UpstreamExecutionInput,
};
use tsc_program::PreparedProgram;
use tsc_syntax::{ParseDiagnosticOrigin, ParseRecoveryAction, ParseRecoveryKind, SyntaxKind};

use crate::codegen_common::find_workspace_root;
use crate::h2_2c_acceptance::{
    is_declaration_file_path, parse_prepared_source, prepared_parse_options,
};
use crate::recovery_parse_snapshot as parse_snapshot;

const CENSUS_KIND: &str = "utf16-literal-recovery-admission-census";

/// Qualification artifacts whose rows embed the exact qualified VFS input the
/// acceptance runner reconstructs (`input.files` as base64 bytes).
pub(super) const QUALIFIED_INPUT_ARTIFACTS: &[&str] = &[
    "ratchets/h2-1a-qualification.v1.json",
    "ratchets/h2-1b-qualification.v1.json",
    "ratchets/h2-1c-qualification.v1.json",
    "ratchets/h2-1d-qualification.v1.json",
    "ratchets/h2-1e-qualification.v1.json",
    "ratchets/h2-2a-qualification.v1.json",
    "ratchets/h2-2b-qualification.v1.json",
    "ratchets/h2-2c-qualification.v1.json",
    "ratchets/h2-2d-qualification.v1.json",
    "ratchets/h2-3a-qualification.v1.json",
    "ratchets/h2-3b-qualification.v1.json",
    "ratchets/h2-3c-qualification.v1.json",
    "ratchets/h2-4a-qualification.v1.json",
    "ratchets/h2-4b-qualification.v1.json",
    "ratchets/h2-5a-qualification.v1.json",
    "ratchets/h2-5b-qualification.v1.json",
    "ratchets/h2-5c-qualification.v1.json",
    "ratchets/h2-5d-qualification.v1.json",
    "ratchets/h2-5e-qualification.v1.json",
    "ratchets/h2-5f-qualification.v1.json",
    "ratchets/h2-5g-qualification.v1.json",
    "ratchets/h2-5h-qualification.v1.json",
    "ratchets/h2-6a-qualification.v1.json",
    "ratchets/h2-6b-qualification.v1.json",
    "ratchets/h2-6c-qualification.v1.json",
    "ratchets/h2-7b-qualification.v1.json",
];

/// Candidate-input artifacts whose rows embed `input.files` as text plus the
/// merged harness settings (`settings` pairs) and `input.config`.
pub(super) const CANDIDATE_INPUT_ARTIFACTS: &[&str] = &[
    "ratchets/h2-7de-candidate-inputs.v1.json",
    "ratchets/h2-8a-candidate-inputs.v1.json",
];

/// Artifacts carrying TypeScript-side `source_facts.parse_diagnostic_units`
/// for a parity cross-check of diagnostic presence and codes.
pub(super) const TYPESCRIPT_FACT_ARTIFACTS: &[&str] = &[
    "ratchets/h2-7b-qualification.v1.json",
    "ratchets/h2-7de-candidates.v1.json",
    "ratchets/h2-8a-candidates.v1.json",
];

#[derive(Clone, Debug)]
struct UnitFacts {
    path: String,
    diagnostics: usize,
    codes: Vec<u32>,
    literal_only: bool,
    events: usize,
    literal_diagnostic_events: usize,
    structural_diagnostic_events: usize,
    silent_missing_events: usize,
    diagnostic_origins: usize,
    missing_await_supported: Option<bool>,
    emit_supported: Option<bool>,
    parameter_gaps_supported: Option<bool>,
    statement_gaps_supported: Option<bool>,
    context_supported: Option<bool>,
    recovery_facts: Option<Value>,
}

impl UnitFacts {
    fn json(&self) -> Value {
        let mut value = json!({
            "path": self.path,
            "diagnostics": self.diagnostics,
            "codes": self.codes,
            "literal_only": self.literal_only,
            "events": self.events,
            "literal_diagnostic_events": self.literal_diagnostic_events,
            "structural_diagnostic_events": self.structural_diagnostic_events,
            "silent_missing_events": self.silent_missing_events,
            "diagnostic_origins": self.diagnostic_origins,
        });
        if let Some(supported) = self.missing_await_supported {
            value["literal_or_missing_await"] = json!(supported);
            value["recovery"] = self.recovery_facts.clone().unwrap();
        }
        if let Some(supported) = self.emit_supported {
            value["supported_emit_recovery"] = json!(supported);
        }
        if let Some(supported) = self.parameter_gaps_supported {
            value["parameter_gap_emit_recovery"] = json!(supported);
        }
        if let Some(supported) = self.statement_gaps_supported {
            value["statement_gap_emit_recovery"] = json!(supported);
        }
        if let Some(supported) = self.context_supported {
            value["context_emit_recovery"] = json!(supported);
        }
        value
    }
}

/// Reporting mirror of the parser's private literal-origin predicate; the
/// verdict itself always comes from `SourceFile::has_only_literal_recovery`.
fn is_literal_origin(origin: ParseDiagnosticOrigin) -> bool {
    matches!(
        origin,
        ParseDiagnosticOrigin::ScannerToken(
            SyntaxKind::StringLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::TemplateHead
                | SyntaxKind::TemplateMiddle
                | SyntaxKind::TemplateTail
        )
    )
}

/// The acceptance's source iteration (`h2_2c_acceptance`): emit-eligible,
/// non-declaration, non-JSON sources parsed with the loader's options.
fn program_facts(
    program: &PreparedProgram,
    missing_await: bool,
    emit_recovery: bool,
    parameter_gaps: bool,
    statement_gaps: bool,
    context_recovery: bool,
    mut capture: Option<&mut ParseCapture>,
) -> (Vec<UnitFacts>, Vec<Value>) {
    let options = program.compiler_options();
    let mut facts = Vec::new();
    let mut parse_units = Vec::new();
    for source in program.source_files() {
        let display = source.path().display().to_string_lossy();
        let path = display.as_ref();
        let lower_path = path.to_ascii_lowercase();
        if lower_path.ends_with(".json") {
            continue;
        }
        if let Some(capture) = capture.as_deref_mut() {
            // Capture the planner-owned options for every source, including
            // libraries and declaration files, using its case-preserving path.
            let module_options = tsc_program::source_request_parse_options(source, options);
            let input = parse_snapshot::input(path, source.snapshot().text(), &module_options);
            let id = input["input_id"].as_str().unwrap().to_owned();
            if !capture.inputs.contains_key(&id) {
                let syntax = parse_snapshot::replay(&input);
                assert_eq!(
                    capture.record(path, source.snapshot().text(), &module_options, &syntax),
                    id
                );
            }
            parse_units.push(json!({"path":path,"input_id":id,"role":"module-request-parse", "emit_eligible":false}));
        }
        if !source.may_be_emitted() || is_declaration_file_path(&lower_path) {
            continue;
        }
        let syntax = parse_prepared_source(options, source, path, &lower_path);
        if let Some(capture) = capture.as_deref_mut() {
            let parse_options = prepared_parse_options(options, source, &lower_path);
            let input_id = capture.record(path, source.snapshot().text(), &parse_options, &syntax);
            parse_units.push(json!({"path":path,"input_id":input_id,"role":"emit-preflight", "emit_eligible":true}));
        }
        let recovery = syntax.parse_recovery();
        let diagnostics = syntax.parse_diagnostics.len();
        let events = recovery.events();
        if diagnostics == 0 && events.is_empty() && recovery.diagnostic_origins().is_empty() {
            continue;
        }
        let mut unit = UnitFacts {
            path: path.to_owned(),
            diagnostics,
            codes: syntax
                .parse_diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect(),
            literal_only: syntax.has_only_literal_recovery(),
            events: events.len(),
            literal_diagnostic_events: 0,
            structural_diagnostic_events: 0,
            silent_missing_events: 0,
            diagnostic_origins: recovery.diagnostic_origins().len(),
            missing_await_supported: (missing_await || emit_recovery || parameter_gaps || statement_gaps || context_recovery).then(|| syntax.has_only_literal_or_missing_await_recovery()),
            emit_supported: (emit_recovery || parameter_gaps || statement_gaps || context_recovery).then(|| syntax.has_only_missing_node_emit_recovery()),
            parameter_gaps_supported: (parameter_gaps || statement_gaps || context_recovery).then(|| syntax.has_only_parameter_gap_emit_recovery()),
            statement_gaps_supported: (statement_gaps || context_recovery).then(|| syntax.has_only_statement_gap_emit_recovery()),
            context_supported: context_recovery.then(|| syntax.has_supported_emit_recovery()),
            recovery_facts: (missing_await || emit_recovery || parameter_gaps || statement_gaps || context_recovery).then(|| {
                let events: Vec<_> = recovery.events().iter().map(|event| json!({
                    "kind": format!("{:?}", event.kind), "start": event.start, "length": event.length,
                    "diagnostic_index": event.diagnostic_index, "full_start": event.full_start,
                    "missing_node": event.missing_node.map(|node| json!({"kind": node.kind as u16, "position": node.position})),
                })).collect();
                let actions: Vec<_> = recovery.actions().iter().map(|action| match *action {
                    ParseRecoveryAction::TokenSkipped { token, start, length, statement_start, site } =>
                        json!({"kind": "token-skipped", "token": token as u16, "start": start,
                            "length": length, "statement_start": statement_start, "site": format!("{site:?}")}),
                    ParseRecoveryAction::Reparsed { start, end } => json!({"kind": "reparsed", "start": start, "end": end}),
                }).collect();
                json!({"events": events, "actions": actions})
            }),
        };
        for event in events {
            match event.kind {
                ParseRecoveryKind::Diagnostic(origin) if is_literal_origin(origin) => {
                    unit.literal_diagnostic_events += 1;
                }
                ParseRecoveryKind::Diagnostic(_) => unit.structural_diagnostic_events += 1,
                ParseRecoveryKind::SilentMissingNode(_) => unit.silent_missing_events += 1,
            }
        }
        facts.push(unit);
    }
    (facts, parse_units)
}

#[derive(Default)]
struct ParseCapture {
    inputs: BTreeMap<String, Value>,
    digests: BTreeMap<String, Value>,
    documents: BTreeMap<String, String>,
}

impl ParseCapture {
    fn document(&mut self, text: &[u8]) -> String {
        let id = parse_snapshot::sha256(text);
        self.documents
            .entry(id.clone())
            .or_insert_with(|| base64::engine::general_purpose::STANDARD.encode(text));
        id
    }

    fn plan_input(&mut self, input: &UpstreamExecutionInput, program: &PreparedProgram) -> Value {
        observable_input::plan_input(input, program, &mut |bytes| self.document(bytes))
    }

    fn record(
        &mut self,
        path: &str,
        text: &str,
        options: &tsc_syntax::ParseOptions,
        syntax: &tsc_syntax::SourceFile,
    ) -> String {
        let input = parse_snapshot::input(path, text, options);
        let id = input["input_id"].as_str().unwrap().to_owned();
        let core = parse_snapshot::digest(syntax);
        let recovery = syntax.parse_recovery();
        let digest = json!({"core":core,"recovery_counts":{
            "events":recovery.events().len(),"actions":recovery.actions().len(),
            "diagnostic_origins":recovery.diagnostic_origins().len()},
            "profiles":{
                "literal":syntax.has_only_literal_recovery(),
                "missing_await":syntax.has_only_literal_or_missing_await_recovery(),
                "missing_declaration":syntax.has_only_missing_node_emit_recovery(),
                "parameter_gaps":syntax.has_only_parameter_gap_emit_recovery(),
                "statement_gaps":syntax.has_only_statement_gap_emit_recovery(),
                "context_recovery":syntax.has_supported_emit_recovery()}});
        if let Some(previous) = self.inputs.get(&id) {
            assert_eq!(previous, &input, "parse snapshot identity collision");
            assert_eq!(
                self.digests[&id], digest,
                "same parse input produced a different digest"
            );
        } else {
            let replayed = parse_snapshot::replay(&input);
            assert_eq!(
                parse_snapshot::digest(&replayed),
                core,
                "export does not reproduce the actual census parse: {path}"
            );
            assert_eq!(
                replayed.parse_recovery(),
                syntax.parse_recovery(),
                "export changed committed recovery facts: {path}"
            );
            self.inputs.insert(id.clone(), input);
            self.digests.insert(id.clone(), digest);
        }
        id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Verdict {
    /// No retained diagnostic and no recovery event in any unit.
    NoRecovery,
    /// Refused before the repair (a unit retained diagnostics); admitted now.
    NewlyAdmitted,
    /// Admitted before the repair (no retained diagnostic); refused now.
    NewlyRefused,
    /// Refused before and after (a structural recovery remains).
    StillRefused,
    /// Only zero-diagnostic literal events: admitted before and after.
    UnchangedAdmitted,
}

impl Verdict {
    fn label(self) -> &'static str {
        match self {
            Self::NoRecovery => "no-recovery",
            Self::NewlyAdmitted => "newly-admitted",
            Self::NewlyRefused => "newly-refused",
            Self::StillRefused => "still-refused",
            Self::UnchangedAdmitted => "unchanged-admitted",
        }
    }

    fn of(units: &[UnitFacts]) -> Self {
        if units.is_empty() {
            return Self::NoRecovery;
        }
        let refused_before = units.iter().any(|unit| {
            if unit.context_supported.is_some() {
                !unit
                    .statement_gaps_supported
                    .expect("context comparison has the statement-gap baseline")
            } else if unit.statement_gaps_supported.is_some() {
                !unit
                    .parameter_gaps_supported
                    .expect("statement-gap comparison has the parameter-gap baseline")
            } else if unit.parameter_gaps_supported.is_some() {
                !unit
                    .emit_supported
                    .expect("parameter-gap comparison has the missing-node baseline")
            } else if unit.emit_supported.is_some() {
                !unit
                    .missing_await_supported
                    .expect("emit comparison has the missing-await baseline")
            } else if unit.missing_await_supported.is_some() {
                !unit.literal_only
            } else {
                unit.diagnostics > 0
            }
        });
        let refused_after = units.iter().any(|unit| {
            !unit
                .context_supported
                .or(unit.statement_gaps_supported)
                .or(unit.parameter_gaps_supported)
                .or(unit.emit_supported)
                .or(unit.missing_await_supported)
                .unwrap_or(unit.literal_only)
        });
        match (refused_before, refused_after) {
            (true, false) => Self::NewlyAdmitted,
            (false, true) => Self::NewlyRefused,
            (true, true) => Self::StillRefused,
            (false, false) => Self::UnchangedAdmitted,
        }
    }
}

#[derive(Clone)]
struct Row {
    case_id: String,
    suite: String,
    universe: String,
    loader: String,
    emit_load_error: Option<String>,
    units: Vec<UnitFacts>,
    parse_units: Vec<Value>,
    command_input: Option<Value>,
    verdict: Verdict,
}

impl Row {
    fn emit_disposition(&self) -> &'static str {
        if self.emit_load_error.is_some() {
            "parse-admission-only; emit-not-qualified"
        } else {
            "pending-complete-command-comparison"
        }
    }

    fn json(&self) -> Value {
        json!({
            "case_id": self.case_id,
            "suite": self.suite,
            "universe": self.universe,
            "loader": self.loader,
            "emit_load_error": self.emit_load_error,
            "emit_disposition": self.emit_disposition(),
            "verdict": self.verdict.label(),
            "units": self.units.iter().map(UnitFacts::json).collect::<Vec<_>>(),
        })
    }
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, Box<dyn Error>> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("{field} is not a string: {}", value[field]).into())
}

fn read_json(workspace: &Path, relative: &str) -> Result<(Value, String), Box<dyn Error>> {
    let bytes = fs::read(workspace.join(relative))
        .map_err(|source| format!("failed to read {relative}: {source}"))?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    Ok((serde_json::from_slice(&bytes)?, digest))
}

struct Census {
    workspace: PathBuf,
    missing_await: bool,
    emit_recovery: bool,
    parameter_gaps: bool,
    statement_gaps: bool,
    context_recovery: bool,
    rows: Vec<Row>,
    seen: BTreeSet<String>,
    load_failures: Vec<Value>,
    inputs: Map<String, Value>,
    parse_capture: Option<ParseCapture>,
}

impl Census {
    fn record(&mut self, row: Row) {
        self.rows.push(row);
    }

    fn claim(&mut self, case_id: &str) -> bool {
        self.seen.insert(case_id.to_owned())
    }

    fn recorded_plans(&mut self) -> Result<(), Box<dyn Error>> {
        let started = Instant::now();
        let corpus = load_recorded_execution_plans(&self.workspace)?;
        self.inputs.insert(
            "recorded_execution_plans".to_owned(),
            json!({
                "manifest": tsc_harness::upstream_suites::MANIFEST_RELATIVE_PATH,
                "plans": corpus.plans.len(),
                "verified_source_paths": corpus.cache_stats.verified_source_paths,
            }),
        );
        let mut processed = 0usize;
        for plan in corpus.plans.iter() {
            let case_id = plan.provenance.case_id.as_ref();
            if !self.claim(case_id) {
                continue;
            }
            let suite = plan.provenance.suite.as_str().to_owned();
            let mut emit_load_error = None;
            let (loader, loaded) = match &plan.input {
                UpstreamExecutionInput::Compiler(compiler) => {
                    match load_compiler_emit(&self.workspace, compiler, limits()) {
                        Ok(program) => ("load_compiler_emit", Ok(program)),
                        Err(emit_error) => {
                            emit_load_error = Some(emit_error.to_string());
                            match load_compiler_no_emit(&self.workspace, compiler, limits()) {
                                Ok(program) => ("load_compiler_no_emit", Ok(program)),
                                Err(no_emit_error) => (
                                    "load_compiler_emit|load_compiler_no_emit",
                                    Err(format!("{emit_error} | {no_emit_error}")),
                                ),
                            }
                        }
                    }
                }
                UpstreamExecutionInput::Project(project) => {
                    match load_project_emit(&self.workspace, project, limits()) {
                        Ok(program) => ("load_project_emit", Ok(program.prepared_program)),
                        Err(emit_error) => {
                            emit_load_error = Some(emit_error.to_string());
                            match load_project_no_emit(&self.workspace, project, limits()) {
                                Ok(program) => {
                                    ("load_project_no_emit", Ok(program.prepared_program))
                                }
                                Err(no_emit_error) => (
                                    "load_project_emit|load_project_no_emit",
                                    Err(format!("{emit_error} | {no_emit_error}")),
                                ),
                            }
                        }
                    }
                }
            };
            match loaded {
                Ok(program) => {
                    let (units, parse_units) = program_facts(
                        &program,
                        self.missing_await,
                        self.emit_recovery,
                        self.parameter_gaps,
                        self.statement_gaps,
                        self.context_recovery,
                        self.parse_capture.as_mut(),
                    );
                    let verdict = Verdict::of(&units);
                    let row = Row {
                        case_id: case_id.to_owned(),
                        suite,
                        universe: "recorded-execution-plans".to_owned(),
                        loader: loader.to_owned(),
                        emit_load_error,
                        units,
                        parse_units,
                        command_input: self
                            .parse_capture
                            .as_mut()
                            .map(|capture| capture.plan_input(&plan.input, &program)),
                        verdict,
                    };
                    self.record(row);
                }
                Err(error) => self.load_failures.push(json!({
                    "case_id": case_id,
                    "suite": suite,
                    "universe": "recorded-execution-plans",
                    "loader": loader,
                    "error": error,
                    "emit_load_error": emit_load_error,
                    "emit_disposition": "not-loaded; emit-not-qualified",
                })),
            }
            processed += 1;
            if processed.is_multiple_of(500) {
                eprintln!(
                    "census: recorded plans {processed}/{} ({:.0}s)",
                    corpus.plans.len(),
                    started.elapsed().as_secs_f64()
                );
            }
        }
        Ok(())
    }

    fn qualified_artifacts(&mut self) -> Result<(), Box<dyn Error>> {
        for relative in QUALIFIED_INPUT_ARTIFACTS {
            let started = Instant::now();
            let (artifact, digest) = read_json(&self.workspace, relative)?;
            let cases = artifact["cases"]
                .as_array()
                .ok_or_else(|| format!("{relative}: cases is not an array"))?;
            let mut claimed = 0usize;
            for case in cases {
                let case_id = string(case, "case_id")?;
                if case["input"]["files"].as_array().is_none_or(Vec::is_empty) {
                    continue;
                }
                if !self.claim(case_id) {
                    continue;
                }
                claimed += 1;
                let suite = case["suite"].as_str().unwrap_or("unknown").to_owned();
                match qualified_input(&self.workspace, &case["input"]) {
                    Ok(program) => {
                        let (units, parse_units) = program_facts(
                            &program,
                            self.missing_await,
                            self.emit_recovery,
                            self.parameter_gaps,
                            self.statement_gaps,
                            self.context_recovery,
                            self.parse_capture.as_mut(),
                        );
                        let verdict = Verdict::of(&units);
                        let row = Row {
                            case_id: case_id.to_owned(),
                            suite,
                            universe: (*relative).to_owned(),
                            loader: "load_qualified_compiler_emit_with_symlinks".to_owned(),
                            emit_load_error: None,
                            units,
                            parse_units,
                            command_input: self.parse_capture.as_mut().map(|_| {
                                observable_input::artifact_input(
                                    "qualified",
                                    &case["input"],
                                    &Value::Null,
                                    &program,
                                )
                            }),
                            verdict,
                        };
                        self.record(row);
                    }
                    Err(error) => self.load_failures.push(json!({
                        "case_id": case_id,
                        "suite": suite,
                        "universe": relative,
                        "loader": "load_qualified_compiler_emit_with_symlinks",
                        "error": error.to_string(),
                        "emit_disposition": "not-loaded; emit-not-qualified",
                    })),
                }
            }
            self.inputs.insert(
                (*relative).to_owned(),
                json!({ "sha256": digest, "cases": cases.len(), "claimed": claimed }),
            );
            eprintln!(
                "census: {relative} cases={} claimed={claimed} ({:.0}s)",
                cases.len(),
                started.elapsed().as_secs_f64()
            );
        }
        Ok(())
    }

    fn candidate_artifacts(&mut self) -> Result<(), Box<dyn Error>> {
        for relative in CANDIDATE_INPUT_ARTIFACTS {
            let started = Instant::now();
            let (artifact, digest) = read_json(&self.workspace, relative)?;
            let cases = artifact["cases"]
                .as_array()
                .ok_or_else(|| format!("{relative}: cases is not an array"))?;
            let mut claimed = 0usize;
            let mut skipped_mount = 0usize;
            for case in cases {
                let case_id = string(case, "case_id")?;
                let input = &case["input"];
                if input["route"] != "whole-program" {
                    continue;
                }
                if !input["shared_mount"].is_null() {
                    // Project-mount rows are covered by the recorded project plans.
                    skipped_mount += 1;
                    continue;
                }
                if input["files"].as_array().is_none_or(Vec::is_empty) {
                    continue;
                }
                if !self.claim(case_id) {
                    continue;
                }
                claimed += 1;
                let suite = case["suite"].as_str().unwrap_or("unknown").to_owned();
                match candidate_input(&self.workspace, case) {
                    Ok(program) => {
                        let (units, parse_units) = program_facts(
                            &program,
                            self.missing_await,
                            self.emit_recovery,
                            self.parameter_gaps,
                            self.statement_gaps,
                            self.context_recovery,
                            self.parse_capture.as_mut(),
                        );
                        let verdict = Verdict::of(&units);
                        let row = Row {
                            case_id: case_id.to_owned(),
                            suite,
                            universe: (*relative).to_owned(),
                            loader: "load_qualified_compiler_emit_with_symlinks".to_owned(),
                            emit_load_error: None,
                            units,
                            parse_units,
                            command_input: self.parse_capture.as_mut().map(|_| {
                                observable_input::artifact_input(
                                    "candidate",
                                    input,
                                    &case["settings"],
                                    &program,
                                )
                            }),
                            verdict,
                        };
                        self.record(row);
                    }
                    Err(error) => self.load_failures.push(json!({
                        "case_id": case_id,
                        "suite": suite,
                        "universe": relative,
                        "loader": "load_qualified_compiler_emit_with_symlinks",
                        "error": error.to_string(),
                        "emit_disposition": "not-loaded; emit-not-qualified",
                    })),
                }
            }
            self.inputs.insert(
                (*relative).to_owned(),
                json!({ "sha256": digest, "cases": cases.len(), "claimed": claimed, "skipped_shared_mount": skipped_mount }),
            );
            eprintln!(
                "census: {relative} cases={} claimed={claimed} ({:.0}s)",
                cases.len(),
                started.elapsed().as_secs_f64()
            );
        }
        Ok(())
    }

    /// TypeScript-side parse facts recorded by the oracle for candidate rows:
    /// diagnostic presence per unit and the code multiset must agree with the
    /// Rust parser for every row this census loaded.
    fn typescript_parity(&mut self) -> Result<Vec<Value>, Box<dyn Error>> {
        let rust: BTreeMap<&str, &Row> = self
            .rows
            .iter()
            .map(|row| (row.case_id.as_str(), row))
            .collect();
        let mut mismatches = Vec::new();
        let mut compared = 0usize;
        for relative in TYPESCRIPT_FACT_ARTIFACTS {
            let (artifact, digest) = read_json(&self.workspace, relative)?;
            let cases = artifact["cases"]
                .as_array()
                .ok_or_else(|| format!("{relative}: cases is not an array"))?;
            for case in cases {
                let Some(units) = case["source_facts"]["parse_diagnostic_units"].as_array() else {
                    continue;
                };
                let case_id = string(case, "case_id")?;
                let Some(row) = rust.get(case_id) else {
                    continue;
                };
                compared += 1;
                let mut expected: BTreeMap<String, Vec<u32>> = BTreeMap::new();
                for unit in units {
                    // JSON sources take the JSON text route and are never
                    // preflighted; the oracle records their parse facts anyway.
                    if string(unit, "path")?
                        .to_ascii_lowercase()
                        .ends_with(".json")
                    {
                        continue;
                    }
                    let mut codes = unit["codes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_u64)
                        .map(|code| code as u32)
                        .collect::<Vec<_>>();
                    // The oracle records a unit's codes as a Set: compare
                    // presence, not multiplicity (the parser may report one
                    // code twice at distinct positions, as tsc does).
                    codes.sort_unstable();
                    codes.dedup();
                    expected.insert(string(unit, "path")?.to_owned(), codes);
                }
                let mut actual: BTreeMap<String, Vec<u32>> = BTreeMap::new();
                for unit in row.units.iter().filter(|unit| unit.diagnostics > 0) {
                    let mut codes = unit.codes.clone();
                    codes.sort_unstable();
                    codes.dedup();
                    actual.insert(unit.path.clone(), codes);
                }
                if expected != actual {
                    mismatches.push(json!({
                        "case_id": case_id,
                        "artifact": relative,
                        "typescript": expected,
                        "rust": actual,
                    }));
                }
            }
            self.inputs
                .entry((*relative).to_owned())
                .or_insert_with(|| json!({ "sha256": digest, "cases": cases.len() }));
        }
        self.inputs.insert(
            "typescript_parity".to_owned(),
            json!({ "compared_rows": compared, "mismatches": mismatches.len() }),
        );
        Ok(mismatches)
    }
}

fn decode_base64_file(file: &Value) -> Result<(PathBuf, Vec<u8>), Box<dyn Error>> {
    let path = PathBuf::from(string(file, "path")?);
    let bytes = base64::engine::general_purpose::STANDARD.decode(string(file, "utf8_base64")?)?;
    if bytes.len() as u64 != file["utf8_bytes"].as_u64().unwrap_or(u64::MAX)
        || format!("{:x}", Sha256::digest(&bytes)) != string(file, "utf8_sha256")?
    {
        return Err(format!("{}: virtual input identity differs", path.display()).into());
    }
    Ok((path, bytes))
}

fn symlinks(input: &Value) -> Result<Vec<(PathBuf, PathBuf)>, Box<dyn Error>> {
    input["vfs_symlinks"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|link| {
            Ok((
                PathBuf::from(string(link, "link_path")?),
                PathBuf::from(string(link, "target_path")?),
            ))
        })
        .collect()
}

fn roots(input: &Value) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    input["roots"]
        .as_array()
        .ok_or("roots is not an array")?
        .iter()
        .map(|root| {
            root.as_str()
                .map(PathBuf::from)
                .ok_or_else(|| "root is not a string".into())
        })
        .collect()
}

/// The acceptance runner's qualified input reconstruction
/// (`h2_2c_acceptance::case_input_with_floor`), at the established floor.
pub(super) fn qualified_input(
    workspace: &Path,
    input: &Value,
) -> Result<PreparedProgram, Box<dyn Error>> {
    let current_directory = string(input, "current_directory")?;
    let mut files = input["files"]
        .as_array()
        .ok_or("files is not an array")?
        .iter()
        .map(decode_base64_file)
        .collect::<Result<Vec<_>, _>>()?;
    if !input["virtual_config"].is_null() {
        files.push(decode_base64_file(&input["virtual_config"])?);
    }
    let settings = input["settings"]
        .as_array()
        .ok_or("settings is not an array")?
        .iter()
        .map(|setting| {
            Ok((
                string(setting, "name")?.to_owned(),
                string(setting, "value")?.to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    Ok(load_qualified_compiler_emit_with_symlinks(
        workspace,
        current_directory,
        &files,
        &symlinks(input)?,
        &roots(input)?,
        &settings,
        limits(),
        EmitOptionFloor::Established,
    )?)
}

/// Candidate-input rows carry the merged harness settings as pairs and the
/// VFS as text; the same qualified loader reconstructs the program.
pub(super) fn candidate_input(
    workspace: &Path,
    case: &Value,
) -> Result<PreparedProgram, Box<dyn Error>> {
    let input = &case["input"];
    let current_directory = string(input, "current_directory")?;
    let mut files = input["files"]
        .as_array()
        .ok_or("files is not an array")?
        .iter()
        .map(|file| {
            Ok((
                PathBuf::from(string(file, "path")?),
                string(file, "text")?.as_bytes().to_vec(),
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    if !input["config"].is_null() {
        let config = &input["config"];
        let path = PathBuf::from(string(config, "path")?);
        if !files.iter().any(|(existing, _)| *existing == path) {
            files.push((path, string(config, "text")?.as_bytes().to_vec()));
        }
    }
    let settings = case["settings"]
        .as_array()
        .ok_or("settings is not an array")?
        .iter()
        .map(|pair| {
            let pair = pair.as_array().ok_or("setting pair is not an array")?;
            Ok((
                pair.first()
                    .and_then(Value::as_str)
                    .ok_or("setting name is not a string")?
                    .to_owned(),
                pair.get(1)
                    .and_then(Value::as_str)
                    .ok_or("setting value is not a string")?
                    .to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    Ok(load_qualified_compiler_emit_with_symlinks(
        workspace,
        current_directory,
        &files,
        &symlinks(input)?,
        &roots(input)?,
        &settings,
        limits(),
        EmitOptionFloor::Established,
    )?)
}

pub fn run(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut out = None;
    let mut parse_snapshot_out = None;
    let mut missing_await = false;
    let mut emit_recovery = false;
    let mut parameter_gaps = false;
    let mut statement_gaps = false;
    let mut context_recovery = false;
    let mut all_profiles = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--missing-await" => missing_await = true,
            "--emit-recovery" => emit_recovery = true,
            "--parameter-gaps" => parameter_gaps = true,
            "--statement-gaps" => statement_gaps = true,
            "--context-recovery" => context_recovery = true,
            "--all-profiles" => all_profiles = true,
            "--parse-snapshot-out" => {
                parse_snapshot_out = Some(PathBuf::from(
                    args.next().ok_or("missing --parse-snapshot-out value")?,
                ));
            }
            "--out" => {
                out = Some(PathBuf::from(
                    args.next().ok_or("missing value after --out")?,
                ));
            }
            other => return Err(format!("unexpected census argument: {other}").into()),
        }
    }
    if [
        missing_await,
        emit_recovery,
        parameter_gaps,
        statement_gaps,
        context_recovery,
        all_profiles,
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count()
        > 1
    {
        return Err("choose one recovery comparison profile".into());
    }
    if parse_snapshot_out.is_some() && !all_profiles {
        return Err("parse snapshots require --all-profiles".into());
    }
    let workspace = find_workspace_root()?;
    if parse_snapshot_out.is_some() {
        let clean = std::process::Command::new("git")
            .args(["diff", "--quiet", "HEAD", "--"])
            .current_dir(&workspace)
            .status()?;
        if !clean.success() {
            return Err("parse snapshots require a clean committed tree".into());
        }
        let untracked = std::process::Command::new("git")
            .args([
                "ls-files",
                "--others",
                "--exclude-standard",
                "--",
                "crates",
                "scripts",
            ])
            .current_dir(&workspace)
            .output()?;
        if !untracked.status.success() || !untracked.stdout.is_empty() {
            return Err("parse snapshots require all code and fixtures to be tracked".into());
        }
        if fs::read(workspace.join("crates/xtask/src/recovery_parse_snapshot.rs"))?
            != include_bytes!("recovery_parse_snapshot.rs")
        {
            return Err("parse snapshot binary does not match its digest source".into());
        }
    }
    let out = out.unwrap_or_else(|| {
        workspace.join(if all_profiles {
            "target/emitter-recovery-census"
        } else if context_recovery {
            "target/emitter-context-recovery-census.json"
        } else if statement_gaps {
            "target/emitter-statement-gap-recovery-census.json"
        } else if parameter_gaps {
            "target/emitter-parameter-gap-recovery-census.json"
        } else if emit_recovery {
            "target/emitter-structural-recovery-census.json"
        } else if missing_await {
            "target/emitter-missing-await-recovery-census.json"
        } else {
            "target/utf16-literal-recovery-census.json"
        })
    });
    let started = Instant::now();
    let mut census = Census {
        workspace: workspace.clone(),
        missing_await: missing_await || all_profiles,
        emit_recovery: emit_recovery || all_profiles,
        parameter_gaps: parameter_gaps || all_profiles,
        statement_gaps: statement_gaps || all_profiles,
        context_recovery: context_recovery || all_profiles,
        rows: Vec::new(),
        seen: BTreeSet::new(),
        load_failures: Vec::new(),
        inputs: Map::new(),
        parse_capture: parse_snapshot_out.as_ref().map(|_| ParseCapture::default()),
    };
    census.recorded_plans()?;
    census.qualified_artifacts()?;
    census.candidate_artifacts()?;
    let mismatches = census.typescript_parity()?;

    if let Some(path) = parse_snapshot_out {
        let capture = census.parse_capture.as_ref().unwrap();
        let git = |spec: &str| -> Result<String, Box<dyn Error>> {
            let output = std::process::Command::new("git")
                .args(["rev-parse", spec])
                .current_dir(&workspace)
                .output()?;
            if !output.status.success() {
                return Err(format!("cannot pin git identity {spec}").into());
            }
            Ok(String::from_utf8(output.stdout)?.trim().to_owned())
        };
        let rows: Vec<_> = census
            .rows
            .iter()
            .map(|row| {
                json!({"case_id":row.case_id,
            "universe":row.universe,"loader":row.loader,"emit_load_error":row.emit_load_error,
            "emit_disposition":row.emit_disposition(),"units":row.parse_units,"command_input":row.command_input})
            })
            .collect();
        let artifact = json!({"schema":1,"kind":"emitter-recovery-parse-snapshot",
            "head":git("HEAD")?,"syntax_tree_hash":git("HEAD:crates/syntax")?,
            "vendor_tree_hash":git("HEAD:vendor/typescript-6.0.3")?,
            "digest_code_sha256":parse_snapshot::sha256(include_bytes!("recovery_parse_snapshot.rs")),
            "plan_manifest_sha256":parse_snapshot::sha256(fs::read(workspace.join(tsc_harness::upstream_suites::MANIFEST_RELATIVE_PATH))?),
            "input_manifest":census.inputs,"load_failures":census.load_failures,
            "inputs":capture.inputs,"digests":capture.digests,"documents":capture.documents,"rows":rows});
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec(&artifact)?)?;
    }
    let elapsed_seconds = started.elapsed().as_secs_f64();
    if all_profiles {
        for (filename, flags) in [
            ("missing-await.json", (true, false, false, false, false)),
            (
                "missing-declaration.json",
                (false, true, false, false, false),
            ),
            ("parameter-gaps.json", (false, false, true, false, false)),
            ("statement-gaps.json", (false, false, false, true, false)),
            ("context-recovery.json", (false, false, false, false, true)),
        ] {
            write_report(
                &census,
                &mismatches,
                elapsed_seconds,
                &out.join(filename),
                flags,
            )?;
        }
    } else {
        write_report(
            &census,
            &mismatches,
            elapsed_seconds,
            &out,
            (
                missing_await,
                emit_recovery,
                parameter_gaps,
                statement_gaps,
                context_recovery,
            ),
        )?;
    }
    Ok(())
}

fn write_report(
    census: &Census,
    mismatches: &[Value],
    elapsed_seconds: f64,
    out: &Path,
    (missing_await, emit_recovery, parameter_gaps, statement_gaps, context_recovery): (
        bool,
        bool,
        bool,
        bool,
        bool,
    ),
) -> Result<(), Box<dyn Error>> {
    // A shared loader pass evaluates every historical boundary. Projection
    // removes later predicates before classifying each report, so a later
    // admission cannot silently change an earlier stage's before/after set.
    let rows = census
        .rows
        .iter()
        .cloned()
        .map(|mut row| {
            for unit in &mut row.units {
                if !context_recovery {
                    unit.context_supported = None;
                }
                if !(statement_gaps || context_recovery) {
                    unit.statement_gaps_supported = None;
                }
                if !(parameter_gaps || statement_gaps || context_recovery) {
                    unit.parameter_gaps_supported = None;
                }
                if !(emit_recovery || parameter_gaps || statement_gaps || context_recovery) {
                    unit.emit_supported = None;
                }
                if !(missing_await
                    || emit_recovery
                    || parameter_gaps
                    || statement_gaps
                    || context_recovery)
                {
                    unit.missing_await_supported = None;
                    unit.recovery_facts = None;
                }
            }
            row.verdict = Verdict::of(&row.units);
            row
        })
        .collect::<Vec<_>>();
    let mut by_verdict: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_universe: BTreeMap<String, BTreeMap<&str, usize>> = BTreeMap::new();
    for row in &rows {
        *by_verdict.entry(row.verdict.label()).or_default() += 1;
        *by_universe
            .entry(row.universe.clone())
            .or_default()
            .entry(row.verdict.label())
            .or_default() += 1;
    }
    let select = |verdict: Verdict| {
        rows.iter()
            .filter(|row| row.verdict == verdict)
            .map(Row::json)
            .collect::<Vec<_>>()
    };
    let report = json!({
        "schema": 1,
        "kind": if context_recovery { "emitter-context-recovery-admission-census" } else if statement_gaps { "emitter-statement-gap-recovery-admission-census" } else if parameter_gaps { "emitter-parameter-gap-recovery-admission-census" } else if emit_recovery { "emitter-structural-recovery-admission-census" } else if missing_await { "emitter-missing-await-recovery-admission-census" } else { CENSUS_KIND },
        "scope": if context_recovery { "Await-context runs and structurally owned recovery evaluated with all acceptance loaders; no emit and no CI" } else if statement_gaps { "Statement/declaration gaps and report-only boundaries evaluated with all acceptance loaders; no emit and no CI" } else if parameter_gaps { "Parameter-gap recovery evaluated with the acceptance loaders over every recorded input; no emit and no CI" } else if emit_recovery { "Current structural emit-recovery predicate evaluated with the acceptance loaders over every corpus row with recorded inputs; no emit, no TypeScript execution, no CI" } else if missing_await { "Missing-await recovery predicate evaluated with the acceptance loaders and parse projection over every corpus row with recorded inputs; no emit, no TypeScript execution, no CI" } else { "Rust parser literal-only recovery admission evaluated with the acceptance loaders and parse projection over every corpus row with recorded inputs; no emit, no TypeScript execution, no CI" },
        "predicate": if context_recovery { "Emit-eligible sources; before = SourceFile::has_only_statement_gap_emit_recovery; after = SourceFile::has_supported_emit_recovery" } else if statement_gaps { "Emit-eligible sources; before = SourceFile::has_only_parameter_gap_emit_recovery; after = SourceFile::has_only_statement_gap_emit_recovery" } else if parameter_gaps { "Emit-eligible sources; before = SourceFile::has_only_missing_node_emit_recovery; after = SourceFile::has_only_parameter_gap_emit_recovery" } else if emit_recovery { "Emit-eligible non-declaration non-JSON sources; before = SourceFile::has_only_literal_or_missing_await_recovery; after = SourceFile::has_only_missing_node_emit_recovery" } else if missing_await { "Emit-eligible non-declaration non-JSON sources; before = SourceFile::has_only_literal_recovery; after = SourceFile::has_only_literal_or_missing_await_recovery" } else { "SourceFile::has_only_literal_recovery over emit-eligible non-declaration non-JSON sources; before = refuse when any unit retains a parse diagnostic; after = refuse when any unit is not literal-only" },
        "head": std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&census.workspace)
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|head| head.trim().to_owned()),
        "inputs": census.inputs,
        "summary": {
            "rows": census.rows.len(),
            "unique_case_ids": census.seen.len(),
            "load_failures": census.load_failures.len(),
            "by_verdict": by_verdict,
            "by_universe": by_universe,
            "typescript_parity_mismatches": mismatches.len(),
            "elapsed_seconds": elapsed_seconds,
        },
        "newly_admitted": select(Verdict::NewlyAdmitted),
        "newly_refused": select(Verdict::NewlyRefused),
        "still_refused": select(Verdict::StillRefused),
        "unchanged_admitted": select(Verdict::UnchangedAdmitted),
        "typescript_parity_mismatches": mismatches,
        "load_failures": census.load_failures,
    });
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &out,
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!(
        "{}",
        json!({
            "out": out,
            "rows": census.rows.len(),
            "by_verdict": report["summary"]["by_verdict"],
            "load_failures": census.load_failures.len(),
            "typescript_parity_mismatches": mismatches.len(),
        })
    );
    Ok(())
}
