//! Hosted H2.2c acceptance projection over the source-dispositioned
//! compiler/conformance rows in the pinned `ts-tests` tree.

use std::collections::{BTreeSet, HashMap};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::{EmitWriteMetadata, H2RuntimeSlice, MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, DiagnosticCategory, MessageChain};
use tsc_harness::upstream_suites::execution::{
    load_compiler_emit, load_compiler_emit_with_option_floor, load_project_emit,
    load_qualified_compiler_emit_with_option_floor, load_recorded_execution_plans,
    CompilerExecutionPlan, EmitOptionFloor, ProjectExecutionPlan, UpstreamExecutionInput,
};
use tsc_program::{PreparedProgram, PreparedSourceFile, ProgramLoadLimits, ResolutionMode};
use tsc_syntax::{
    for_each_child, parse_source_file_from_snapshot, LanguageVariant, NodeData, ParseOptions,
    SourceFile, SyntaxKind,
};
use tsc_types::{CompilerOptions, ModuleKind};

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-2c-qualification.v1.json";
const H2_4A_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-4a-qualification.v1.json";
const H2_4B_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-4b-qualification.v1.json";
const H2_5A_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-5a-qualification.v1.json";
const H2_5B_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-5b-qualification.v1.json";
const H2_5C_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-5c-qualification.v1.json";
const H2_5D_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-5d-qualification.v1.json";
const H2_5E_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-5e-qualification.v1.json";
const H2_5F_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-5f-qualification.v1.json";
const H2_5G_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-5g-qualification.v1.json";

#[derive(Clone, Copy)]
enum AcceptanceSlice {
    H2_2c,
    H2_4a,
    H2_4b,
    H2_5a,
    H2_5b,
    H2_5c,
    H2_5d,
    H2_5e,
    H2_5f,
    H2_5g,
}

impl AcceptanceSlice {
    const fn label(self) -> &'static str {
        match self {
            Self::H2_2c => "H2.2c",
            Self::H2_4a => "H2.4a",
            Self::H2_4b => "H2.4b",
            Self::H2_5a => "H2.5a",
            Self::H2_5b => "H2.5b",
            Self::H2_5c => "H2.5c",
            Self::H2_5d => "H2.5d",
            Self::H2_5e => "H2.5e",
            Self::H2_5f => "H2.5f",
            Self::H2_5g => "H2.5g",
        }
    }
}

struct RecordedCompilerCase {
    expansion_case: u32,
    plan: CompilerExecutionPlan,
}

struct H2_5gExecutionInputs {
    compiler_cases: HashMap<String, RecordedCompilerCase>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct H2_5gCaseTotals {
    admitted: usize,
    h2_8a_deferred: usize,
    h2_9_deferred: usize,
    writes: usize,
    diagnostics: usize,
}

impl H2_5gCaseTotals {
    fn add_assign(&mut self, other: Self) {
        self.admitted += other.admitted;
        self.h2_8a_deferred += other.h2_8a_deferred;
        self.h2_9_deferred += other.h2_9_deferred;
        self.writes += other.writes;
        self.diagnostics += other.diagnostics;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum H2_5gDeferredOwner {
    H2_8a,
    H2_9,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum H2_5gCaseDisposition {
    Admitted,
    Deferred(H2_5gDeferredOwner),
}

const MAX_H2_5G_WORKERS: usize = 2;
const H2_5G_WORKERS_ENV: &str = "TSRS_H2_5G_WORKERS";

fn select_h2_5g_workers(configured: Option<&str>, available: usize) -> Result<usize, String> {
    if available == 0 {
        return Err("available H2.5g parallelism must be positive".to_owned());
    }
    let ceiling = available.min(MAX_H2_5G_WORKERS);
    let Some(configured) = configured else {
        // Keep local acceptance resource-safe. Hosted CI opts into two
        // workers explicitly; the pipeline itself remains ordered below.
        return Ok(1);
    };
    let workers = configured.parse::<usize>().map_err(|_| {
        format!("{H2_5G_WORKERS_ENV} must be an integer from 1 to {MAX_H2_5G_WORKERS}")
    })?;
    if workers == 0 || workers > MAX_H2_5G_WORKERS {
        return Err(format!(
            "{H2_5G_WORKERS_ENV} must be an integer from 1 to {MAX_H2_5G_WORKERS}"
        ));
    }
    Ok(workers.min(ceiling))
}

fn h2_5g_worker_count() -> Result<usize, Box<dyn Error>> {
    let available = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    select_h2_5g_workers(std::env::var(H2_5G_WORKERS_ENV).ok().as_deref(), available)
        .map_err(Into::into)
}

fn validate_h2_5g_qualification(artifact: &Value) -> Result<&[Value], Box<dyn Error>> {
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.5g-es2016-target"
        || artifact["selection_contract"]["global_h2_5g_rows"] != 11_910
        || artifact["selection_contract"]["global_candidate_denominator"] != 9_027
        || artifact["selection_contract"]["candidate_denominator"] != 9_027
        || artifact["selection_contract"]["future_deferred_rows"] != 2_883
        || artifact["summary"]["candidates"] != 9_027
        || artifact["summary"]["compiler_candidates"] != 4_712
        || artifact["summary"]["conformance_candidates"] != 4_315
        || artifact["summary"]["recorded_compiler_plan_cases"] != 4_712
        || artifact["summary"]["qualified_vfs_cases"] != 4_315
        || artifact["summary"]["virtual_config_cases"] != 56
        || artifact["summary"]["vfs_symlink_cases"] != 3
        || artifact["summary"]["vfs_symlink_paths"] != 4
        || artifact["summary"]["admitted_cases"] != 8_511
        || artifact["summary"]["deferred_cases"] != 516
        || artifact["summary"]["source_deferred_cases"] != 516
        || artifact["summary"]["no_emit_control_cases"] != 59
        || artifact["summary"]["typescript_runs"] != 18_054
        || artifact["summary"]["deterministic_typescript_cases"] != 9_027
        || artifact["summary"]["admitted_typescript_writes"] != 9_466
        || artifact["summary"]["admitted_typescript_diagnostics"] != 26_815
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
        || artifact["owner_closure"]
            .as_array()
            .is_none_or(|owners| owners.len() != 1 || owners[0]["key"] != "transform-es2016")
    {
        return Err(failure("H2.5g qualification header is not closed"));
    }
    let cases = array(artifact, "cases")?;
    if cases.len() != 9_027 {
        return Err(failure("H2.5g qualification case denominator changed"));
    }
    Ok(cases)
}

fn classify_h2_5g_case(case: &Value) -> Result<H2_5gCaseDisposition, Box<dyn Error>> {
    let case_id = string(case, "case_id")?;
    match string(case, "disposition")? {
        "admitted-for-execution" => Ok(H2_5gCaseDisposition::Admitted),
        "deferred-to-slices"
            if case["diagnostic_disposition"]["state"] == "not-observed-source-deferred"
                && case["rust_expectation"] == "typed-failure-before-first-sink-write" =>
        {
            match array(case, "required_slices")?
                .first()
                .and_then(Value::as_str)
            {
                Some("H2.8a") => Ok(H2_5gCaseDisposition::Deferred(H2_5gDeferredOwner::H2_8a)),
                Some("H2.9") => Ok(H2_5gCaseDisposition::Deferred(H2_5gDeferredOwner::H2_9)),
                first => Err(failure(format!(
                    "{case_id}: unknown first H2.5g deferred owner {first:?}"
                ))),
            }
        }
        disposition => Err(failure(format!(
            "unknown H2.5g disposition {disposition} for {case_id}"
        ))),
    }
}

impl H2_5gExecutionInputs {
    fn load(workspace: &Path) -> Result<Self, Box<dyn Error>> {
        let corpus = load_recorded_execution_plans(workspace)?;
        let compiler_cases = corpus
            .plans
            .iter()
            .filter_map(|recorded| match &recorded.input {
                UpstreamExecutionInput::Compiler(plan) => Some((
                    recorded.provenance.case_id.to_string(),
                    RecordedCompilerCase {
                        expansion_case: recorded.provenance.case_index,
                        plan: plan.clone(),
                    },
                )),
                UpstreamExecutionInput::Project(_) => None,
            })
            .collect::<HashMap<_, _>>();
        if compiler_cases.len() != 7_276 {
            return Err(failure(format!(
                "H2.5g recorded compiler-plan denominator changed: {}",
                compiler_cases.len(),
            )));
        }
        Ok(Self { compiler_cases })
    }

    fn prepare(
        &self,
        workspace: &Path,
        case: &Value,
    ) -> Result<tsc_program::PreparedProgram, Box<dyn Error>> {
        match string(case, "execution_route")? {
            "qualified-vfs" => case_input(workspace, case),
            "recorded-compiler-plan" => {
                let case_id = string(case, "case_id")?;
                let recorded = self.compiler_cases.get(case_id).ok_or_else(|| {
                    failure(format!("{case_id}: recorded compiler plan is absent"))
                })?;
                if case["suite"] != "compiler"
                    || case["expansion_case"].as_u64() != Some(u64::from(recorded.expansion_case))
                {
                    return Err(failure(format!(
                        "{case_id}: recorded compiler-plan provenance differs"
                    )));
                }
                Ok(load_compiler_emit(workspace, &recorded.plan, limits())?)
            }
            route => Err(failure(format!(
                "{}: unknown H2.5g execution route {route}",
                string(case, "case_id")?,
            ))),
        }
    }
}

fn failure(message: impl Into<String>) -> Box<dyn Error> {
    std::io::Error::other(message.into()).into()
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024)
}

fn sha256(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, Box<dyn Error>> {
    value[field]
        .as_str()
        .ok_or_else(|| failure(format!("H2.2c field {field} is not a string")))
}

fn array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], Box<dyn Error>> {
    value[field]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| failure(format!("H2.2c field {field} is not an array")))
}

fn case_input(
    workspace: &Path,
    case: &Value,
) -> Result<tsc_program::PreparedProgram, Box<dyn Error>> {
    case_input_with_floor(workspace, case, EmitOptionFloor::Established)
}

fn case_input_with_floor(
    workspace: &Path,
    case: &Value,
    floor: EmitOptionFloor,
) -> Result<tsc_program::PreparedProgram, Box<dyn Error>> {
    let input = &case["input"];
    let current_directory = string(input, "current_directory")?;
    let mut files = array(input, "files")?
        .iter()
        .map(|file| {
            let path = PathBuf::from(string(file, "path")?);
            let bytes =
                base64::engine::general_purpose::STANDARD.decode(string(file, "utf8_base64")?)?;
            if bytes.len() as u64 != file["utf8_bytes"].as_u64().unwrap_or(u64::MAX)
                || sha256(&bytes) != string(file, "utf8_sha256")?
            {
                return Err(failure(format!(
                    "{}: virtual input identity differs",
                    path.display()
                )));
            }
            Ok((path, bytes))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    if !input["virtual_config"].is_null() {
        let config = &input["virtual_config"];
        let path = PathBuf::from(string(config, "path")?);
        let bytes =
            base64::engine::general_purpose::STANDARD.decode(string(config, "utf8_base64")?)?;
        if bytes.len() as u64 != config["utf8_bytes"].as_u64().unwrap_or(u64::MAX)
            || sha256(&bytes) != string(config, "utf8_sha256")?
        {
            return Err(failure(format!(
                "{}: virtual config identity differs",
                path.display()
            )));
        }
        files.push((path, bytes));
    }
    let roots = array(input, "roots")?
        .iter()
        .map(|root| {
            root.as_str()
                .map(PathBuf::from)
                .ok_or_else(|| failure("H2.2c root is not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let settings = array(input, "settings")?
        .iter()
        .map(|setting| {
            Ok((
                string(setting, "name")?.to_owned(),
                string(setting, "value")?.to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    Ok(load_qualified_compiler_emit_with_option_floor(
        workspace,
        current_directory,
        &files,
        &roots,
        &settings,
        limits(),
        floor,
    )?)
}

fn assert_exact_writes(
    case_id: &str,
    expected: &[Value],
    actual: &MemoryOutputSink,
) -> Result<(), Box<dyn Error>> {
    if actual.writes().len() != expected.len() {
        return Err(failure(format!(
            "{case_id}: expected {} writes, observed {}",
            expected.len(),
            actual.writes().len()
        )));
    }
    for (index, (expected, actual)) in expected.iter().zip(actual.writes()).enumerate() {
        let expected_path = Path::new(string(expected, "path")?);
        let expected_bytes = base64::engine::general_purpose::STANDARD
            .decode(string(expected, "callback_utf8_base64")?)?;
        if actual.path() != expected_path
            || actual.callback_text().as_bytes() != expected_bytes
            || actual.callback_text().len() as u64
                != expected["callback_utf8_bytes"].as_u64().unwrap_or(u64::MAX)
            || sha256(actual.callback_text().as_bytes())
                != string(expected, "callback_utf8_sha256")?
            || actual.write_byte_order_mark()
                != expected["write_byte_order_mark"].as_bool().unwrap_or(true)
            || actual.materialized_bytes().len() as u64
                != expected["materialized_utf8_bytes"]
                    .as_u64()
                    .unwrap_or(u64::MAX)
            || sha256(actual.materialized_bytes()) != string(expected, "materialized_utf8_sha256")?
        {
            let difference =
                exact_write_difference(&expected_bytes, actual.callback_text().as_bytes());
            return Err(failure(format!(
                "{case_id}: write {index} path or exact bytes differ: expected_path={} actual_path={} expected_callback_sha256={} actual_callback_sha256={} expected_materialized_sha256={} actual_materialized_sha256={} expected_bom={} actual_bom={} {difference}",
                expected_path.display(),
                actual.path().display(),
                string(expected, "callback_utf8_sha256")?,
                sha256(actual.callback_text().as_bytes()),
                string(expected, "materialized_utf8_sha256")?,
                sha256(actual.materialized_bytes()),
                expected["write_byte_order_mark"].as_bool().unwrap_or(true),
                actual.write_byte_order_mark(),
            )));
        }
        let expected_sources = array(expected, "source_files")?
            .iter()
            .map(|source| source.as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        let actual_sources = actual
            .source_files()
            .unwrap_or_default()
            .iter()
            .map(|source| source.to_string_lossy())
            .collect::<Vec<_>>();
        if actual_sources
            .iter()
            .map(|source| source.as_ref())
            .collect::<Vec<_>>()
            != expected_sources
        {
            return Err(failure(format!(
                "{case_id}: write {index} source provenance differs"
            )));
        }
    }
    Ok(())
}

/// Recover the source files that reached JavaScript artifact construction from
/// the oracle's exact write provenance. A source can occur in more than one
/// output (for example JavaScript plus a source map), so activity accounting
/// is based on the unique source union rather than the write count.
fn transform_source_paths(expected: &Value) -> Result<BTreeSet<&str>, Box<dyn Error>> {
    let mut paths = BTreeSet::new();
    for write in array(expected, "writes")? {
        for source in array(write, "source_files")? {
            let source = source
                .as_str()
                .ok_or_else(|| failure("write source provenance is not a string"))?;
            paths.insert(source);
        }
    }
    Ok(paths)
}

fn is_transform_source(file: &Value, paths: &BTreeSet<&str>) -> bool {
    file["emit_eligible"] == true
        && file["path"]
            .as_str()
            .is_some_and(|path| paths.contains(path))
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ExpectedTypedActivity {
    routed_sources: u64,
    transformed_sources: u64,
    preserve_sources: u64,
    node_format_sources: u64,
    javascript_sources: u64,
    jsx_sources: u64,
    automatic_jsx_sources: u64,
    json_sources: u64,
    decorator_sources: u64,
    h2_4a_sources: u64,
    h2_4b_sources: u64,
    h2_1a_sources: u64,
    h2_1b_sources: u64,
    h2_1c_sources: u64,
    h2_1d_sources: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TypedSourceFacts {
    has_decorator: bool,
    has_import_attributes: bool,
}

/// Project activity from the same durable program facts consumed by the
/// emitter. This deliberately does not use the qualification feature
/// inventory: recovery nodes and per-file pragmas are typed syntax facts, and
/// the global module transformer is selected before any per-file format is
/// consulted.
fn expected_typed_activity(
    program: &PreparedProgram,
    transform_source_paths: &BTreeSet<&str>,
) -> ExpectedTypedActivity {
    let options = program.compiler_options();
    let module_kind = ModuleKind::from_bits(options.emit_module_kind());
    let all_sources_own_node_format = matches!(
        module_kind,
        ModuleKind::NODE16 | ModuleKind::NODE18 | ModuleKind::NODE20 | ModuleKind::NODE_NEXT
    ) || options.rewrite_relative_import_extensions == Some(true);
    let mut activity = ExpectedTypedActivity::default();

    for source in program
        .source_files()
        .iter()
        .filter(|source| source.may_be_emitted())
    {
        let display_path = source.path().display().to_string_lossy();
        let path = display_path.as_ref();
        let lower_path = path.to_ascii_lowercase();
        if is_declaration_file_path(&lower_path) {
            continue;
        }
        let is_javascript = [".js", ".mjs", ".cjs", ".jsx"]
            .iter()
            .any(|extension| lower_path.ends_with(extension));
        let is_jsx = lower_path.ends_with(".tsx") || lower_path.ends_with(".jsx");
        let is_json = lower_path.ends_with(".json");
        if is_json && options.out_dir.is_none() && options.out_file.is_none() {
            continue;
        }
        activity.routed_sources += 1;
        activity.javascript_sources += u64::from(is_javascript);
        activity.jsx_sources += u64::from(is_jsx);
        activity.json_sources += u64::from(is_json);
        let is_transform_source = transform_source_paths.contains(path);
        let syntax = (is_jsx || (is_transform_source && !is_json))
            .then(|| parse_prepared_source(options, source, path, &lower_path));

        if is_jsx
            && syntax.as_ref().is_some_and(|syntax| {
                syntax.jsx_runtime_pragma.as_deref() != Some("classic")
                    && (matches!(options.jsx, Some(4 | 5))
                        || options.jsx_import_source.is_some()
                        || syntax.has_jsx_import_source_pragma
                        || syntax.jsx_runtime_pragma.as_deref() == Some("automatic"))
            })
        {
            activity.automatic_jsx_sources += 1;
        }

        if !is_transform_source {
            continue;
        }
        activity.transformed_sources += 1;
        let facts = syntax.as_ref().map(typed_source_facts).unwrap_or_default();
        activity.decorator_sources += u64::from(facts.has_decorator);
        if options.experimental_decorators && facts.has_decorator {
            activity.h2_4a_sources += 1;
        }
        if !options.use_define_for_class_fields_effective()
            || (!options.experimental_decorators && facts.has_decorator)
        {
            activity.h2_4b_sources += 1;
        }
        if all_sources_own_node_format
            || lower_path.ends_with(".mts")
            || lower_path.ends_with(".cts")
            || facts.has_import_attributes
        {
            activity.node_format_sources += 1;
        }

        if module_kind == ModuleKind::PRESERVE {
            activity.preserve_sources += 1;
        } else if module_kind == ModuleKind::SYSTEM {
            activity.h2_1d_sources += 1;
        } else {
            activity.h2_1a_sources += 1;
            let emit_format = match source.implied_node_format_for_emit() {
                Some(ResolutionMode::CommonJs) => ModuleKind::COMMON_JS,
                Some(ResolutionMode::EsNext) => ModuleKind::ES_NEXT,
                Some(ResolutionMode::Unspecified) | None => module_kind,
            };
            if emit_format.bits() < ModuleKind::ES2015.bits() {
                activity.h2_1b_sources += 1;
            }
            if matches!(emit_format, ModuleKind::AMD | ModuleKind::UMD) {
                activity.h2_1c_sources += 1;
            }
        }
    }

    activity
}

fn parse_prepared_source(
    options: &CompilerOptions,
    source: &PreparedSourceFile,
    path: &str,
    lower_path: &str,
) -> SourceFile {
    let javascript_file = [".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|extension| lower_path.ends_with(extension));
    let language_variant = if lower_path.ends_with(".tsx") || javascript_file {
        LanguageVariant::Jsx
    } else {
        LanguageVariant::Standard
    };
    let is_declaration_file = is_declaration_file_path(lower_path);
    let module_detection = options.emit_module_detection_kind();
    let force_external_module = !is_declaration_file
        && match module_detection {
            3 => true,
            2 => {
                [".cjs", ".cts", ".mjs", ".mts"]
                    .iter()
                    .any(|extension| lower_path.ends_with(extension))
                    || source.implied_node_format() == Some(ResolutionMode::EsNext)
            }
            _ => false,
        };
    let detect_external_module_from_jsx =
        !is_declaration_file && module_detection == 2 && matches!(options.jsx, Some(4 | 5));
    parse_source_file_from_snapshot(
        path.to_owned(),
        Arc::clone(source.snapshot()),
        ParseOptions {
            script_target: options.emit_script_target(),
            language_variant,
            javascript_file,
            force_external_module,
            detect_external_module_from_jsx,
            ..ParseOptions::default()
        },
        None,
    )
}

fn is_declaration_file_path(lower_path: &str) -> bool {
    lower_path.ends_with(".d.ts")
        || lower_path.ends_with(".d.cts")
        || lower_path.ends_with(".d.mts")
        || lower_path
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|name| name.ends_with(".ts") && name.contains(".d."))
}

fn typed_source_facts(source: &SourceFile) -> TypedSourceFacts {
    let mut facts = TypedSourceFacts::default();
    let mut stack = vec![source.root];
    while let Some(id) = stack.pop() {
        let record = source.arena.node(id);
        facts.has_decorator |= record.kind == SyntaxKind::Decorator;
        facts.has_import_attributes |= matches!(
            &record.data,
            NodeData::ImportDeclaration(data) if data.attributes.is_some()
        ) || matches!(
            &record.data,
            NodeData::ExportDeclaration(data) if data.attributes.is_some()
        ) || matches!(
            &record.data,
            NodeData::CallExpression(data)
                if data.expression.is_some_and(|expression| {
                    source.arena.node(expression).kind == SyntaxKind::ImportKeyword
                }) && data.arguments.is_some_and(|arguments| {
                    source.arena.node_array(arguments).nodes.len() > 1
                })
        );
        if facts.has_decorator && facts.has_import_attributes {
            break;
        }
        for_each_child(&source.arena, record, |child| {
            stack.push(child);
            false
        });
    }
    facts
}

fn exact_write_difference(expected: &[u8], actual: &[u8]) -> String {
    let index = expected
        .iter()
        .zip(actual)
        .position(|(expected, actual)| expected != actual)
        .unwrap_or_else(|| expected.len().min(actual.len()));
    let start = index.saturating_sub(96);
    let expected_end = expected.len().min(index.saturating_add(192));
    let actual_end = actual.len().min(index.saturating_add(192));
    format!(
        "first_difference={index} expected_len={} actual_len={} expected_context={:?} actual_context={:?}",
        expected.len(),
        actual.len(),
        String::from_utf8_lossy(&expected[start.min(expected.len())..expected_end]),
        String::from_utf8_lossy(&actual[start.min(actual.len())..actual_end]),
    )
}

fn flatten_message_chain(chain: &MessageChain, indent: usize, output: &mut String) {
    if indent != 0 {
        output.push('\n');
        for _ in 0..indent {
            output.push_str("  ");
        }
    }
    output.push_str(&chain.text);
    for child in &chain.next {
        flatten_message_chain(child, indent + 1, output);
    }
}

fn diagnostic_category(category: DiagnosticCategory) -> &'static str {
    match category {
        DiagnosticCategory::Warning => "Warning",
        DiagnosticCategory::Error => "Error",
        DiagnosticCategory::Suggestion => "Suggestion",
        DiagnosticCategory::Message => "Message",
    }
}

fn normalize_diagnostic(diagnostic: &Diagnostic) -> Value {
    let mut message = String::new();
    flatten_message_chain(&diagnostic.message, 0, &mut message);
    json!({
        "code": diagnostic.code(),
        "category": diagnostic_category(diagnostic.category()),
        "file": diagnostic.file_name,
        "start": diagnostic.start,
        "length": diagnostic.length,
        "message": message,
    })
}

fn normalize_diagnostics(actual: &[Diagnostic]) -> Vec<Value> {
    actual.iter().map(normalize_diagnostic).collect()
}

fn canonicalize_diagnostic_paths(rows: &[Value]) -> Vec<Value> {
    rows.iter()
        .map(|row| {
            let Some(path) = row["file"].as_str() else {
                return row.clone();
            };
            let Some(index) = path.find("/vendor/typescript-6.0.3/") else {
                return row.clone();
            };
            let mut canonical = row.clone();
            canonical["file"] = Value::String(format!("<workspace>{}", &path[index..]));
            canonical
        })
        .collect()
}

fn assert_reported_diagnostics(
    case_id: &str,
    expected: &[Value],
    actual: &[Diagnostic],
) -> Result<(), Box<dyn Error>> {
    // Vendored library diagnostics contain the host's absolute workspace
    // prefix. Keep the exact library-relative identity while making the
    // acceptance artifact portable across local and hosted runners.
    let expected = canonicalize_diagnostic_paths(expected);
    let actual = canonicalize_diagnostic_paths(&normalize_diagnostics(actual));
    if actual != expected {
        let expected_sha256 = sha256(serde_json::to_vec(&expected)?);
        let actual_sha256 = sha256(serde_json::to_vec(&actual)?);
        let detail = if expected.len().max(actual.len()) <= 20 {
            format!(
                "\nexpected={}\nactual={}",
                serde_json::to_string_pretty(&expected)?,
                serde_json::to_string_pretty(&actual)?,
            )
        } else {
            let index = expected
                .iter()
                .zip(actual.iter())
                .position(|(expected, actual)| expected != actual)
                .unwrap_or_else(|| expected.len().min(actual.len()));
            format!(
                " first_difference={index} expected_row={} actual_row={}",
                expected
                    .get(index)
                    .map(serde_json::to_string)
                    .transpose()?
                    .as_deref()
                    .unwrap_or("<missing>"),
                actual
                    .get(index)
                    .map(serde_json::to_string)
                    .transpose()?
                    .as_deref()
                    .unwrap_or("<missing>"),
            )
        };
        return Err(failure(format!(
            "{case_id}: reported diagnostics differ: expected_count={} actual_count={} expected_sha256={expected_sha256} actual_sha256={actual_sha256}{detail}",
            expected.len(),
            actual.len(),
        )));
    }
    Ok(())
}

fn compact_typescript_observation(case: &Value) -> Result<&Value, Box<dyn Error>> {
    let case_id = string(case, "case_id")?;
    let fingerprints = array(case, "typescript_run_fingerprints")?;
    let observation = &case["typescript_observation"];
    let fingerprint = string(observation, "run_fingerprint_sha256")?;
    if fingerprints.len() != 2
        || fingerprints
            .iter()
            .any(|entry| entry.as_str() != Some(fingerprint))
    {
        return Err(failure(format!(
            "{case_id}: compact TypeScript repetition proof differs"
        )));
    }
    Ok(observation)
}

fn execute_slice_observed(
    workspace: &Path,
    case: &Value,
    accepted_slice: AcceptanceSlice,
) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed_with_inputs(workspace, case, accepted_slice, None)
}

fn execute_slice_observed_with_inputs(
    workspace: &Path,
    case: &Value,
    accepted_slice: AcceptanceSlice,
    h2_5g_inputs: Option<&H2_5gExecutionInputs>,
) -> Result<(usize, usize), Box<dyn Error>> {
    let case_id = string(case, "case_id")?;
    let prepare = || match h2_5g_inputs {
        Some(inputs) => inputs.prepare(workspace, case),
        None => case_input(workspace, case),
    };
    let expected = if matches!(accepted_slice, AcceptanceSlice::H2_5g) {
        compact_typescript_observation(case)?
    } else {
        &array(case, "typescript_runs")?[0]
    };
    let transform_source_paths = transform_source_paths(expected)?;
    // `PreparedProgram` is an immutable, fully-owned semantic input. Build it
    // once, then clone that value for the second consuming session. This keeps
    // the two `ProgramSession`s isolated while avoiding a second filesystem/
    // parse/resolve traversal whose only purpose was to recreate identical
    // bytes. The repetition still consumes two distinct owned programs and
    // compares every observable below.
    let first_program = prepare()?;
    let second_program = first_program.clone();
    let typed_activity = if matches!(accepted_slice, AcceptanceSlice::H2_5g) {
        Some(expected_typed_activity(
            &first_program,
            &transform_source_paths,
        ))
    } else {
        None
    };
    let first_session = ProgramSession::new(first_program);
    let harness_lib_bundle = if matches!(accepted_slice, AcceptanceSlice::H2_5g) {
        first_session.prepare_harness_lib_bundle()?
    } else {
        None
    };
    let mut first_sink = MemoryOutputSink::new();
    let (first, first_reported) = first_session
        .emit_with_reported_diagnostics_for_harness_with_lib_bundle(
            &mut first_sink,
            harness_lib_bundle.as_ref(),
        )
        .map_err(|error| failure(format!("{case_id}: first Rust emit failed: {error}")))?;
    let mut second_sink = MemoryOutputSink::new();
    let (second, second_reported) = ProgramSession::new(second_program)
        .emit_with_reported_diagnostics_for_harness_with_lib_bundle(
            &mut second_sink,
            harness_lib_bundle.as_ref(),
        )
        .map_err(|error| failure(format!("{case_id}: second Rust emit failed: {error}")))?;
    if first != second || first_sink != second_sink || first_reported != second_reported {
        return Err(failure(format!(
            "{case_id}: repeated Rust emit is not deterministic"
        )));
    }
    let expected_reported = array(expected, "reported_diagnostics")?;
    if case["diagnostic_disposition"]["state"] != "exact-required" {
        return Err(failure(format!(
            "{case_id}: exact case lacks its diagnostic disposition"
        )));
    }
    assert_reported_diagnostics(case_id, expected_reported, &first_reported)?;
    let actual_exit_code = if first.emit_skipped() && !first_reported.is_empty() {
        1
    } else if !first_reported.is_empty() {
        2
    } else {
        0
    };
    if first.emit_skipped()
        != expected["emit_result"]["emit_skipped"]
            .as_bool()
            .unwrap_or(true)
        || !first.diagnostics().is_empty()
        || !expected["emit_result"]["diagnostics"]
            .as_array()
            .is_some_and(Vec::is_empty)
        || first.emitted_files().is_some() == expected["emit_result"]["emitted_files"].is_null()
        || first.source_maps().is_some() == expected["emit_result"]["source_maps"].is_null()
        || !array(expected, "status_writes")?.is_empty()
        || expected["exit_code"].as_i64() != Some(actual_exit_code)
    {
        return Err(failure(format!(
            "{case_id}: exact Program.emit result differs"
        )));
    }
    assert_exact_writes(case_id, array(expected, "writes")?, &first_sink)?;
    let activity = first.h2_activity();
    let files = array(case, "files")?;
    let inventory_routed_sources = files
        .iter()
        .filter(|file| file["emit_eligible"] == true)
        .count() as u64;
    let routed_sources = typed_activity
        .map(|activity| activity.routed_sources)
        .unwrap_or(inventory_routed_sources);
    for path in &transform_source_paths {
        if !files
            .iter()
            .any(|file| file["emit_eligible"] == true && file["path"].as_str() == Some(path))
        {
            return Err(failure(format!(
                "{case_id}: write provenance names non-emittable source {path}"
            )));
        }
    }
    let transformed_sources = files
        .iter()
        .filter(|file| is_transform_source(file, &transform_source_paths))
        .count() as u64;
    let enum_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    is_transform_source(file, &transform_source_paths)
                        && file["feature_roots"].as_array().is_some_and(|roots| {
                            roots.iter().any(|root| root["feature"] == "runtime-enums")
                        })
                })
                .count() as u64
        })
        .unwrap_or(0);
    let namespace_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    is_transform_source(file, &transform_source_paths)
                        && file["feature_roots"].as_array().is_some_and(|roots| {
                            roots
                                .iter()
                                .any(|root| root["feature"] == "runtime-namespaces")
                        })
                })
                .count() as u64
        })
        .unwrap_or(0);
    let parameter_property_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    is_transform_source(file, &transform_source_paths)
                        && file["feature_roots"].as_array().is_some_and(|roots| {
                            roots
                                .iter()
                                .any(|root| root["feature"] == "parameter-properties")
                        })
                })
                .count() as u64
        })
        .unwrap_or(0);
    let import_export_equals_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    is_transform_source(file, &transform_source_paths)
                        && file["feature_roots"].as_array().is_some_and(|roots| {
                            roots.iter().any(|root| {
                                matches!(
                                    root["feature"].as_str(),
                                    Some("import-equals" | "export-equals")
                                )
                            })
                        })
                })
                .count() as u64
        })
        .unwrap_or(0);
    let inventory_decorator_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    is_transform_source(file, &transform_source_paths)
                        && file["feature_roots"].as_array().is_some_and(|roots| {
                            roots.iter().any(|root| root["feature"] == "decorators")
                        })
                })
                .count() as u64
        })
        .unwrap_or(0);
    let decorator_sources = typed_activity
        .map(|activity| activity.decorator_sources)
        .unwrap_or(inventory_decorator_sources);
    let legacy_decorator_sources = if array(&case["input"], "settings")?.iter().any(|setting| {
        setting["name"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case("experimentalDecorators"))
            && setting["value"]
                .as_str()
                .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    }) {
        decorator_sources
    } else {
        0
    };
    let standard_decorator_sources = decorator_sources - legacy_decorator_sources;
    let assignment_field_mode = array(&case["input"], "settings")?
        .iter()
        .find(|setting| {
            setting["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("useDefineForClassFields"))
        })
        .and_then(|setting| setting["value"].as_str())
        .map(|value| value.eq_ignore_ascii_case("false"))
        .unwrap_or(matches!(
            case["target_state"].as_str(),
            Some(
                "ES2015(2)"
                    | "ES2016(3)"
                    | "ES2017(4)"
                    | "ES2018(5)"
                    | "ES2019(6)"
                    | "ES2020(7)"
                    | "ES2021(8)"
            )
        ));
    let inventory_javascript_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    file["emit_eligible"] == true
                        && matches!(file["script_kind"].as_str(), Some("JS" | "JSX"))
                })
                .count() as u64
        })
        .unwrap_or(0);
    let javascript_sources = typed_activity
        .map(|activity| activity.javascript_sources)
        .unwrap_or(inventory_javascript_sources);
    let inventory_jsx_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    file["emit_eligible"] == true
                        && matches!(file["script_kind"].as_str(), Some("TSX" | "JSX"))
                })
                .count() as u64
        })
        .unwrap_or(0);
    let jsx_sources = typed_activity
        .map(|activity| activity.jsx_sources)
        .unwrap_or(inventory_jsx_sources);
    let configured_automatic_jsx_sources =
        if array(&case["input"], "settings")?.iter().any(|setting| {
            setting["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("jsx"))
                && setting["value"].as_str().is_some_and(|value| {
                    matches!(
                        value.to_ascii_lowercase().as_str(),
                        "react-jsx" | "react-jsxdev"
                    )
                })
        }) {
            jsx_sources
        } else {
            0
        };
    let automatic_jsx_sources = typed_activity
        .map(|activity| activity.automatic_jsx_sources)
        .unwrap_or(configured_automatic_jsx_sources);
    let inventory_json_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| file["emit_eligible"] == true && file["script_kind"] == "JSON")
                .count() as u64
        })
        .unwrap_or(0);
    let json_sources = typed_activity
        .map(|activity| activity.json_sources)
        .unwrap_or(inventory_json_sources);
    let transform_module_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    is_transform_source(file, &transform_source_paths)
                        && matches!(file["emit_module_format"].as_i64(), Some(0..=3))
                })
                .count() as u64
        })
        .unwrap_or(0);
    let system_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    is_transform_source(file, &transform_source_paths)
                        && file["emit_module_format"] == 4
                })
                .count() as u64
        })
        .unwrap_or(0);
    let preserve_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    is_transform_source(file, &transform_source_paths)
                        && file["emit_module_format"] == 200
                })
                .count() as u64
        })
        .unwrap_or(0);
    let amd_umd_sources = case["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    is_transform_source(file, &transform_source_paths)
                        && matches!(file["emit_module_format"].as_i64(), Some(2 | 3))
                })
                .count() as u64
        })
        .unwrap_or(0);
    let configured_node_format_sources =
        expected_node_format_sources(case, &transform_source_paths)?;
    let node_format_sources = typed_activity
        .map(|activity| activity.node_format_sources)
        .unwrap_or(configured_node_format_sources);
    let (
        expected_h2_1a_sources,
        expected_h2_1b_sources,
        expected_h2_1c_sources,
        expected_h2_1d_sources,
        preserve_sources,
    ) = typed_activity.map_or_else(
        || {
            (
                transformed_sources - system_sources - preserve_sources,
                transform_module_sources,
                amd_umd_sources,
                system_sources,
                preserve_sources,
            )
        },
        |activity| {
            (
                activity.h2_1a_sources,
                activity.h2_1b_sources,
                activity.h2_1c_sources,
                activity.h2_1d_sources,
                activity.preserve_sources,
            )
        },
    );
    if let Some(activity) = typed_activity {
        if activity.transformed_sources != transformed_sources {
            return Err(failure(format!(
                "{case_id}: prepared-program transform source count differs: expected={transformed_sources} actual={}",
                activity.transformed_sources,
            )));
        }
    }
    let expected_h2_4a_sources = if matches!(
        accepted_slice,
        AcceptanceSlice::H2_4a
            | AcceptanceSlice::H2_4b
            | AcceptanceSlice::H2_5a
            | AcceptanceSlice::H2_5b
            | AcceptanceSlice::H2_5c
            | AcceptanceSlice::H2_5d
            | AcceptanceSlice::H2_5e
            | AcceptanceSlice::H2_5f
            | AcceptanceSlice::H2_5g
    ) {
        typed_activity
            .map(|activity| activity.h2_4a_sources)
            .unwrap_or(legacy_decorator_sources)
    } else {
        0
    };
    let expected_h2_4b_sources = match accepted_slice {
        AcceptanceSlice::H2_4b => transformed_sources,
        AcceptanceSlice::H2_5a if assignment_field_mode => transformed_sources,
        AcceptanceSlice::H2_5a => standard_decorator_sources,
        AcceptanceSlice::H2_5b if assignment_field_mode => transformed_sources,
        AcceptanceSlice::H2_5b => standard_decorator_sources,
        AcceptanceSlice::H2_5c if assignment_field_mode => transformed_sources,
        AcceptanceSlice::H2_5c => standard_decorator_sources,
        AcceptanceSlice::H2_5d if assignment_field_mode => transformed_sources,
        AcceptanceSlice::H2_5d => standard_decorator_sources,
        AcceptanceSlice::H2_5e if assignment_field_mode => transformed_sources,
        AcceptanceSlice::H2_5e => standard_decorator_sources,
        AcceptanceSlice::H2_5f if assignment_field_mode => transformed_sources,
        AcceptanceSlice::H2_5f => standard_decorator_sources,
        AcceptanceSlice::H2_5g => typed_activity
            .map(|activity| activity.h2_4b_sources)
            .unwrap_or_else(|| {
                if assignment_field_mode {
                    transformed_sources
                } else {
                    standard_decorator_sources
                }
            }),
        AcceptanceSlice::H2_2c | AcceptanceSlice::H2_4a => 0,
    };
    if activity.runtime_slice(H2RuntimeSlice::H2_1a) != expected_h2_1a_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_1b) != expected_h2_1b_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_1c) != expected_h2_1c_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_1d) != expected_h2_1d_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_1e) != node_format_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_2a) != enum_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_2b) != namespace_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_2c) != parameter_property_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_2d) != import_export_equals_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_3a) != javascript_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_3b) != jsx_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_3c) != automatic_jsx_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_3d) != json_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_4a) != expected_h2_4a_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_4b) != expected_h2_4b_sources
        || activity.runtime_slice(H2RuntimeSlice::H2_5a)
            != if matches!(
                accepted_slice,
                AcceptanceSlice::H2_5a
                    | AcceptanceSlice::H2_5b
                    | AcceptanceSlice::H2_5c
                    | AcceptanceSlice::H2_5d
                    | AcceptanceSlice::H2_5e
                    | AcceptanceSlice::H2_5f
                    | AcceptanceSlice::H2_5g
            ) {
                transformed_sources
            } else {
                0
            }
        || activity.runtime_slice(H2RuntimeSlice::H2_5b)
            != if matches!(
                accepted_slice,
                AcceptanceSlice::H2_5b
                    | AcceptanceSlice::H2_5c
                    | AcceptanceSlice::H2_5d
                    | AcceptanceSlice::H2_5e
                    | AcceptanceSlice::H2_5f
                    | AcceptanceSlice::H2_5g
            ) {
                transformed_sources
            } else {
                0
            }
        || activity.runtime_slice(H2RuntimeSlice::H2_5c)
            != if matches!(
                accepted_slice,
                AcceptanceSlice::H2_5c
                    | AcceptanceSlice::H2_5d
                    | AcceptanceSlice::H2_5e
                    | AcceptanceSlice::H2_5f
                    | AcceptanceSlice::H2_5g
            ) {
                transformed_sources
            } else {
                0
            }
        || activity.runtime_slice(H2RuntimeSlice::H2_5d)
            != if matches!(
                accepted_slice,
                AcceptanceSlice::H2_5d
                    | AcceptanceSlice::H2_5e
                    | AcceptanceSlice::H2_5f
                    | AcceptanceSlice::H2_5g
            ) {
                transformed_sources
            } else {
                0
            }
        || activity.runtime_slice(H2RuntimeSlice::H2_5e)
            != if matches!(
                accepted_slice,
                AcceptanceSlice::H2_5e | AcceptanceSlice::H2_5f | AcceptanceSlice::H2_5g
            ) {
                transformed_sources
            } else {
                0
            }
        || activity.runtime_slice(H2RuntimeSlice::H2_5f)
            != if matches!(
                accepted_slice,
                AcceptanceSlice::H2_5f | AcceptanceSlice::H2_5g
            ) {
                transformed_sources
            } else {
                0
            }
        || activity.runtime_slice(H2RuntimeSlice::H2_5g)
            != if matches!(accepted_slice, AcceptanceSlice::H2_5g) {
                transformed_sources
            } else {
                0
            }
    {
        return Err(failure(format!(
            "{case_id}: {} activity does not match {routed_sources} routed, {transformed_sources} transformed, {preserve_sources} preserve, {node_format_sources} node-format, {enum_sources} enum, {namespace_sources} namespace, {parameter_property_sources} parameter-property, {import_export_equals_sources} import/export-equals, {javascript_sources} JavaScript, {jsx_sources} JSX, {automatic_jsx_sources} automatic-JSX, {json_sources} JSON, and {decorator_sources} decorator sources: actual H2.1a={} H2.1b={} H2.1c={} H2.1d={} H2.1e={} H2.2a={} H2.2b={} H2.2c={} H2.2d={} H2.3a={} H2.3b={} H2.3c={} H2.3d={} H2.4a={} H2.4b={} H2.5a={} H2.5b={} H2.5c={} H2.5d={} H2.5e={} H2.5f={} H2.5g={}",
            accepted_slice.label(),
            activity.runtime_slice(H2RuntimeSlice::H2_1a),
            activity.runtime_slice(H2RuntimeSlice::H2_1b),
            activity.runtime_slice(H2RuntimeSlice::H2_1c),
            activity.runtime_slice(H2RuntimeSlice::H2_1d),
            activity.runtime_slice(H2RuntimeSlice::H2_1e),
            activity.runtime_slice(H2RuntimeSlice::H2_2a),
            activity.runtime_slice(H2RuntimeSlice::H2_2b),
            activity.runtime_slice(H2RuntimeSlice::H2_2c),
            activity.runtime_slice(H2RuntimeSlice::H2_2d),
            activity.runtime_slice(H2RuntimeSlice::H2_3a),
            activity.runtime_slice(H2RuntimeSlice::H2_3b),
            activity.runtime_slice(H2RuntimeSlice::H2_3c),
            activity.runtime_slice(H2RuntimeSlice::H2_3d),
            activity.runtime_slice(H2RuntimeSlice::H2_4a),
            activity.runtime_slice(H2RuntimeSlice::H2_4b),
            activity.runtime_slice(H2RuntimeSlice::H2_5a),
            activity.runtime_slice(H2RuntimeSlice::H2_5b),
            activity.runtime_slice(H2RuntimeSlice::H2_5c),
            activity.runtime_slice(H2RuntimeSlice::H2_5d),
            activity.runtime_slice(H2RuntimeSlice::H2_5e),
            activity.runtime_slice(H2RuntimeSlice::H2_5f),
            activity.runtime_slice(H2RuntimeSlice::H2_5g),
        )));
    }
    for slice in H2RuntimeSlice::ALL {
        if !matches!(
            slice,
            H2RuntimeSlice::H2_1a
                | H2RuntimeSlice::H2_1b
                | H2RuntimeSlice::H2_1c
                | H2RuntimeSlice::H2_1d
                | H2RuntimeSlice::H2_1e
                | H2RuntimeSlice::H2_2a
                | H2RuntimeSlice::H2_2b
                | H2RuntimeSlice::H2_2c
                | H2RuntimeSlice::H2_2d
                | H2RuntimeSlice::H2_3a
                | H2RuntimeSlice::H2_3b
                | H2RuntimeSlice::H2_3c
                | H2RuntimeSlice::H2_3d
                | H2RuntimeSlice::H2_4a
                | H2RuntimeSlice::H2_4b
                | H2RuntimeSlice::H2_5a
                | H2RuntimeSlice::H2_5b
                | H2RuntimeSlice::H2_5c
                | H2RuntimeSlice::H2_5d
                | H2RuntimeSlice::H2_5e
                | H2RuntimeSlice::H2_5f
                | H2RuntimeSlice::H2_5g
        ) && activity.runtime_slice(slice) != 0
        {
            return Err(failure(format!(
                "{case_id}: unadmitted {} activity",
                slice.name()
            )));
        }
    }
    Ok((first_sink.writes().len(), first_reported.len()))
}

fn execute_observed(workspace: &Path, case: &Value) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed(workspace, case, AcceptanceSlice::H2_2c)
}

fn execute_h2_4a_observed(
    workspace: &Path,
    case: &Value,
) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed(workspace, case, AcceptanceSlice::H2_4a)
}

fn execute_h2_4b_observed(
    workspace: &Path,
    case: &Value,
) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed(workspace, case, AcceptanceSlice::H2_4b)
}

fn execute_h2_5a_observed(
    workspace: &Path,
    case: &Value,
) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed(workspace, case, AcceptanceSlice::H2_5a)
}

fn execute_h2_5b_observed(
    workspace: &Path,
    case: &Value,
) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed(workspace, case, AcceptanceSlice::H2_5b)
}

fn execute_h2_5c_observed(
    workspace: &Path,
    case: &Value,
) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed(workspace, case, AcceptanceSlice::H2_5c)
}

fn execute_h2_5d_observed(
    workspace: &Path,
    case: &Value,
) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed(workspace, case, AcceptanceSlice::H2_5d)
}

fn execute_h2_5e_observed(
    workspace: &Path,
    case: &Value,
) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed(workspace, case, AcceptanceSlice::H2_5e)
}

fn execute_h2_5f_observed(
    workspace: &Path,
    case: &Value,
) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed(workspace, case, AcceptanceSlice::H2_5f)
}

fn execute_h2_5g_observed(
    workspace: &Path,
    case: &Value,
    inputs: &H2_5gExecutionInputs,
) -> Result<(usize, usize), Box<dyn Error>> {
    execute_slice_observed_with_inputs(workspace, case, AcceptanceSlice::H2_5g, Some(inputs))
}

fn execute_h2_5g_case(
    workspace: &Path,
    case: &Value,
    inputs: &H2_5gExecutionInputs,
) -> Result<H2_5gCaseTotals, String> {
    let result = (|| -> Result<H2_5gCaseTotals, Box<dyn Error>> {
        compact_typescript_observation(case)?;
        let totals = match classify_h2_5g_case(case)? {
            H2_5gCaseDisposition::Admitted => {
                let (writes, diagnostics) = execute_h2_5g_observed(workspace, case, inputs)?;
                H2_5gCaseTotals {
                    admitted: 1,
                    writes,
                    diagnostics,
                    ..H2_5gCaseTotals::default()
                }
            }
            H2_5gCaseDisposition::Deferred(H2_5gDeferredOwner::H2_8a) => H2_5gCaseTotals {
                h2_8a_deferred: 1,
                ..H2_5gCaseTotals::default()
            },
            H2_5gCaseDisposition::Deferred(H2_5gDeferredOwner::H2_9) => H2_5gCaseTotals {
                h2_9_deferred: 1,
                ..H2_5gCaseTotals::default()
            },
        };
        Ok(totals)
    })();
    result.map_err(|error| error.to_string())
}

fn expected_node_format_sources(
    case: &Value,
    transform_source_paths: &BTreeSet<&str>,
) -> Result<u64, Box<dyn Error>> {
    let settings = array(&case["input"], "settings")?;
    let all_sources = settings.iter().any(|setting| {
        let name = setting["name"].as_str().unwrap_or_default();
        let value = &setting["value"];
        name == "rewriteRelativeImportExtensions"
            && (value == true
                || value
                    .as_str()
                    .is_some_and(|value| value.eq_ignore_ascii_case("true")))
            || name == "module"
                && value.as_str().is_some_and(|value| {
                    matches!(
                        value.to_ascii_lowercase().as_str(),
                        "node16" | "node18" | "node20" | "nodenext"
                    )
                })
    });
    let mut count = 0_u64;
    for file in array(case, "files")?
        .iter()
        .filter(|file| is_transform_source(file, transform_source_paths))
    {
        let path = string(file, "path")?;
        let owns_format = all_sources
            || path.to_ascii_lowercase().ends_with(".mts")
            || path.to_ascii_lowercase().ends_with(".cts")
            || file["import_attributes"] == true;
        if owns_format {
            count += 1;
        }
    }
    Ok(count)
}

/// Execute all six H2.2c candidates and compare every TypeScript observable
/// twice against two deterministic Rust executions.
pub fn run(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value =
        serde_json::from_slice(&fs::read(workspace.join(QUALIFICATION_RELATIVE_PATH))?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.2c-parameter-properties"
        || artifact["summary"]["candidates"] != 6
        || artifact["summary"]["admitted_cases"] != 6
        || artifact["summary"]["deferred_cases"] != 0
        || artifact["summary"]["diagnostic_deferred_output_control_cases"] != 0
        || artifact["summary"]["source_deferred_cases"] != 0
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
    {
        return Err(failure("H2.2c qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 6 {
        return Err(failure("H2.2c qualification case denominator changed"));
    }
    let mut admitted = 0;
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in cases {
        match string(case, "disposition")? {
            "admitted-for-execution" => {
                admitted += 1;
                let (case_writes, case_diagnostics) = execute_observed(workspace, case)?;
                writes += case_writes;
                diagnostics += case_diagnostics;
            }
            disposition => return Err(failure(format!("unknown H2.2c disposition {disposition}"))),
        }
    }
    if admitted != 6 || writes != 6 || diagnostics != 12 {
        return Err(failure(format!(
            "H2.2c execution totals differ: admitted={admitted} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.2c emit acceptance: candidates=6 exact={admitted} source_deferred=0 exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

/// Execute the nine admitted H2.4a legacy-decorator candidates and retain the
/// one parser-owned source deferral as an explicit H2.9 boundary.
pub fn run_h2_4a(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_4A_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.4a-legacy-decorators"
        || artifact["selection_contract"]["global_h2_4a_rows"] != 418
        || artifact["selection_contract"]["candidate_denominator"] != 10
        || artifact["selection_contract"]["future_deferred_rows"] != 408
        || artifact["summary"]["candidates"] != 10
        || artifact["summary"]["admitted_cases"] != 9
        || artifact["summary"]["deferred_cases"] != 1
        || artifact["summary"]["source_deferred_cases"] != 1
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
    {
        return Err(failure("H2.4a qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 10 {
        return Err(failure("H2.4a qualification case denominator changed"));
    }
    let mut admitted = 0;
    let mut source_deferred = 0;
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in cases {
        match string(case, "disposition")? {
            "admitted-for-execution" => {
                admitted += 1;
                let (case_writes, case_diagnostics) = execute_h2_4a_observed(workspace, case)?;
                writes += case_writes;
                diagnostics += case_diagnostics;
            }
            "deferred-to-slices"
                if case["required_slices"]
                    .as_array()
                    .is_some_and(|slices| slices.len() == 1 && slices[0] == "H2.9")
                    && case["diagnostic_disposition"]["state"]
                        == "not-observed-source-deferred" =>
            {
                source_deferred += 1;
            }
            disposition => {
                return Err(failure(format!(
                    "unknown H2.4a disposition {disposition} for {}",
                    string(case, "case_id")?,
                )))
            }
        }
    }
    if admitted != 9 || source_deferred != 1 || writes != 9 || diagnostics != 8 {
        return Err(failure(format!(
            "H2.4a execution totals differ: admitted={admitted} source_deferred={source_deferred} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.4a emit acceptance: candidates=10 exact={admitted} source_deferred={source_deferred} exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

/// Execute the 42 dependency-closed H2.4b cases, including three immutable
/// H2.1a standard-decorator preservation promotions, and retain the two
/// parser-owned source deferrals as explicit H2.9 boundaries.
pub fn run_h2_4b(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_4B_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.4b-standard-decorators-class-fields"
        || artifact["selection_contract"]["global_h2_4b_rows"] != 104
        || artifact["selection_contract"]["global_candidate_denominator"] != 41
        || artifact["selection_contract"]["historical_promotion_candidates"] != 3
        || artifact["selection_contract"]["candidate_denominator"] != 44
        || artifact["selection_contract"]["future_deferred_rows"] != 63
        || artifact["summary"]["candidates"] != 44
        || artifact["summary"]["admitted_cases"] != 42
        || artifact["summary"]["deferred_cases"] != 2
        || artifact["summary"]["source_deferred_cases"] != 2
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
    {
        return Err(failure("H2.4b qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 44 {
        return Err(failure("H2.4b qualification case denominator changed"));
    }
    let mut admitted = 0;
    let mut source_deferred = 0;
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in cases {
        match string(case, "disposition")? {
            "admitted-for-execution" => {
                admitted += 1;
                let (case_writes, case_diagnostics) = execute_h2_4b_observed(workspace, case)?;
                writes += case_writes;
                diagnostics += case_diagnostics;
            }
            "deferred-to-slices"
                if case["required_slices"]
                    .as_array()
                    .is_some_and(|slices| slices.len() == 1 && slices[0] == "H2.9")
                    && case["diagnostic_disposition"]["state"]
                        == "not-observed-source-deferred" =>
            {
                source_deferred += 1;
            }
            disposition => {
                return Err(failure(format!(
                    "unknown H2.4b disposition {disposition} for {}",
                    string(case, "case_id")?,
                )))
            }
        }
    }
    if admitted != 42 || source_deferred != 2 || writes != 56 || diagnostics != 150 {
        return Err(failure(format!(
            "H2.4b execution totals differ: admitted={admitted} source_deferred={source_deferred} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.4b emit acceptance: candidates=44 exact={admitted} source_deferred={source_deferred} exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

/// Execute every dependency-closed H2.5a target row twice and retain only the
/// explicitly owned parser/recovery and general comment-placement deferrals.
pub fn run_h2_5a(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_5A_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.5a-esnext-target"
        || artifact["selection_contract"]["global_h2_5a_rows"] != 634
        || artifact["selection_contract"]["global_candidate_denominator"] != 172
        || artifact["selection_contract"]["candidate_denominator"] != 172
        || artifact["selection_contract"]["future_deferred_rows"] != 462
        || artifact["summary"]["candidates"] != 172
        || artifact["summary"]["admitted_cases"] != 167
        || artifact["summary"]["deferred_cases"] != 5
        || artifact["summary"]["source_deferred_cases"] != 5
        || artifact["summary"]["admitted_typescript_writes"] != 287
        || artifact["summary"]["admitted_typescript_diagnostics"] != 335
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
        || artifact["owner_closure"]
            .as_array()
            .is_none_or(|owners| owners.len() != 1 || owners[0]["key"] != "transform-esnext")
    {
        return Err(failure("H2.5a qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 172 {
        return Err(failure("H2.5a qualification case denominator changed"));
    }
    let mut admitted = 0;
    let mut h2_8a_deferred = 0;
    let mut h2_9_deferred = 0;
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in cases {
        match string(case, "disposition")? {
            "admitted-for-execution" => {
                admitted += 1;
                let (case_writes, case_diagnostics) = execute_h2_5a_observed(workspace, case)?;
                writes += case_writes;
                diagnostics += case_diagnostics;
            }
            "deferred-to-slices"
                if case["required_slices"]
                    .as_array()
                    .is_some_and(|slices| slices.len() == 1)
                    && case["diagnostic_disposition"]["state"]
                        == "not-observed-source-deferred" =>
            {
                match case["required_slices"][0].as_str() {
                    Some("H2.8a") => h2_8a_deferred += 1,
                    Some("H2.9") => h2_9_deferred += 1,
                    _ => {
                        return Err(failure(format!(
                            "unknown H2.5a deferred owner for {}",
                            string(case, "case_id")?,
                        )))
                    }
                }
            }
            disposition => {
                return Err(failure(format!(
                    "unknown H2.5a disposition {disposition} for {}",
                    string(case, "case_id")?,
                )))
            }
        }
    }
    if admitted != 167
        || h2_8a_deferred != 1
        || h2_9_deferred != 4
        || writes != 287
        || diagnostics != 335
    {
        return Err(failure(format!(
            "H2.5a execution totals differ: admitted={admitted} h2_8a_deferred={h2_8a_deferred} h2_9_deferred={h2_9_deferred} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.5a emit acceptance: candidates=172 exact={admitted} h2_8a_deferred={h2_8a_deferred} h2_9_deferred={h2_9_deferred} exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

/// Execute every dependency-closed H2.5b ES2020 target row twice and retain
/// only the explicitly owned parser/recovery deferrals.
pub fn run_h2_5b(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_5B_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.5b-es2021-target"
        || artifact["selection_contract"]["global_h2_5b_rows"] != 84
        || artifact["selection_contract"]["global_candidate_denominator"] != 72
        || artifact["selection_contract"]["candidate_denominator"] != 72
        || artifact["selection_contract"]["future_deferred_rows"] != 12
        || artifact["summary"]["candidates"] != 72
        || artifact["summary"]["admitted_cases"] != 68
        || artifact["summary"]["deferred_cases"] != 4
        || artifact["summary"]["source_deferred_cases"] != 4
        || artifact["summary"]["admitted_typescript_writes"] != 93
        || artifact["summary"]["admitted_typescript_diagnostics"] != 48
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
        || artifact["owner_closure"]
            .as_array()
            .is_none_or(|owners| owners.len() != 1 || owners[0]["key"] != "transform-es2021")
    {
        return Err(failure("H2.5b qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 72 {
        return Err(failure("H2.5b qualification case denominator changed"));
    }
    let mut admitted = 0;
    let mut h2_9_deferred = 0;
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in cases {
        match string(case, "disposition")? {
            "admitted-for-execution" => {
                admitted += 1;
                let (case_writes, case_diagnostics) = execute_h2_5b_observed(workspace, case)?;
                writes += case_writes;
                diagnostics += case_diagnostics;
            }
            "deferred-to-slices"
                if case["required_slices"]
                    .as_array()
                    .is_some_and(|slices| slices.len() == 1 && slices[0] == "H2.9")
                    && case["diagnostic_disposition"]["state"]
                        == "not-observed-source-deferred" =>
            {
                h2_9_deferred += 1;
            }
            disposition => {
                return Err(failure(format!(
                    "unknown H2.5b disposition {disposition} for {}",
                    string(case, "case_id")?,
                )))
            }
        }
    }
    if admitted != 68 || h2_9_deferred != 4 || writes != 93 || diagnostics != 48 {
        return Err(failure(format!(
            "H2.5b execution totals differ: admitted={admitted} h2_9_deferred={h2_9_deferred} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.5b emit acceptance: candidates=72 exact={admitted} h2_9_deferred={h2_9_deferred} exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

/// Execute every dependency-closed H2.5c ES2019 target row twice and retain
/// only the explicitly owned parser/recovery deferral.
pub fn run_h2_5c(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_5C_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.5c-es2020-target"
        || artifact["selection_contract"]["global_h2_5c_rows"] != 16
        || artifact["selection_contract"]["global_candidate_denominator"] != 15
        || artifact["selection_contract"]["candidate_denominator"] != 15
        || artifact["selection_contract"]["future_deferred_rows"] != 1
        || artifact["summary"]["candidates"] != 15
        || artifact["summary"]["admitted_cases"] != 14
        || artifact["summary"]["deferred_cases"] != 1
        || artifact["summary"]["source_deferred_cases"] != 1
        || artifact["summary"]["admitted_typescript_writes"] != 14
        || artifact["summary"]["admitted_typescript_diagnostics"] != 19
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
        || artifact["owner_closure"]
            .as_array()
            .is_none_or(|owners| owners.len() != 1 || owners[0]["key"] != "transform-es2020")
    {
        return Err(failure("H2.5c qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 15 {
        return Err(failure("H2.5c qualification case denominator changed"));
    }
    let mut admitted = 0;
    let mut h2_9_deferred = 0;
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in cases {
        match string(case, "disposition")? {
            "admitted-for-execution" => {
                admitted += 1;
                let (case_writes, case_diagnostics) = execute_h2_5c_observed(workspace, case)?;
                writes += case_writes;
                diagnostics += case_diagnostics;
            }
            "deferred-to-slices"
                if case["required_slices"]
                    .as_array()
                    .is_some_and(|slices| slices.len() == 1 && slices[0] == "H2.9")
                    && case["diagnostic_disposition"]["state"]
                        == "not-observed-source-deferred" =>
            {
                h2_9_deferred += 1;
            }
            disposition => {
                return Err(failure(format!(
                    "unknown H2.5c disposition {disposition} for {}",
                    string(case, "case_id")?,
                )))
            }
        }
    }
    if admitted != 14 || h2_9_deferred != 1 || writes != 14 || diagnostics != 19 {
        return Err(failure(format!(
            "H2.5c execution totals differ: admitted={admitted} h2_9_deferred={h2_9_deferred} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.5c emit acceptance: candidates=15 exact={admitted} h2_9_deferred={h2_9_deferred} exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

/// Execute every dependency-closed H2.5d ES2018 target row twice and retain
/// only the explicitly owned parser/recovery deferral.
pub fn run_h2_5d(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_5D_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.5d-es2019-target"
        || artifact["selection_contract"]["global_h2_5d_rows"] != 45
        || artifact["selection_contract"]["global_candidate_denominator"] != 24
        || artifact["selection_contract"]["candidate_denominator"] != 24
        || artifact["selection_contract"]["future_deferred_rows"] != 21
        || artifact["summary"]["candidates"] != 24
        || artifact["summary"]["admitted_cases"] != 23
        || artifact["summary"]["deferred_cases"] != 1
        || artifact["summary"]["source_deferred_cases"] != 1
        || artifact["summary"]["admitted_typescript_writes"] != 57
        || artifact["summary"]["admitted_typescript_diagnostics"] != 47
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
        || artifact["owner_closure"]
            .as_array()
            .is_none_or(|owners| owners.len() != 1 || owners[0]["key"] != "transform-es2019")
    {
        return Err(failure("H2.5d qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 24 {
        return Err(failure("H2.5d qualification case denominator changed"));
    }
    let mut admitted = 0;
    let mut h2_9_deferred = 0;
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in cases {
        match string(case, "disposition")? {
            "admitted-for-execution" => {
                admitted += 1;
                let (case_writes, case_diagnostics) = execute_h2_5d_observed(workspace, case)?;
                writes += case_writes;
                diagnostics += case_diagnostics;
            }
            "deferred-to-slices"
                if case["required_slices"]
                    .as_array()
                    .is_some_and(|slices| slices.len() == 1 && slices[0] == "H2.9")
                    && case["diagnostic_disposition"]["state"]
                        == "not-observed-source-deferred" =>
            {
                h2_9_deferred += 1;
            }
            disposition => {
                return Err(failure(format!(
                    "unknown H2.5d disposition {disposition} for {}",
                    string(case, "case_id")?,
                )))
            }
        }
    }
    if admitted != 23 || h2_9_deferred != 1 || writes != 57 || diagnostics != 47 {
        return Err(failure(format!(
            "H2.5d execution totals differ: admitted={admitted} h2_9_deferred={h2_9_deferred} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.5d emit acceptance: candidates=24 exact={admitted} h2_9_deferred={h2_9_deferred} exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

/// Execute every dependency-closed H2.5e ES2017 target row twice and retain
/// only the explicitly owned parser/recovery deferral.
pub fn run_h2_5e(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_5E_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.5e-es2018-target"
        || artifact["selection_contract"]["global_h2_5e_rows"] != 163
        || artifact["selection_contract"]["global_candidate_denominator"] != 41
        || artifact["selection_contract"]["candidate_denominator"] != 41
        || artifact["selection_contract"]["future_deferred_rows"] != 122
        || artifact["summary"]["candidates"] != 41
        || artifact["summary"]["admitted_cases"] != 40
        || artifact["summary"]["deferred_cases"] != 1
        || artifact["summary"]["source_deferred_cases"] != 1
        || artifact["summary"]["admitted_typescript_writes"] != 46
        || artifact["summary"]["admitted_typescript_diagnostics"] != 88
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
        || artifact["owner_closure"]
            .as_array()
            .is_none_or(|owners| owners.len() != 1 || owners[0]["key"] != "transform-es2018")
    {
        return Err(failure("H2.5e qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 41 {
        return Err(failure("H2.5e qualification case denominator changed"));
    }
    let mut admitted = 0;
    let mut h2_9_deferred = 0;
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in cases {
        match string(case, "disposition")? {
            "admitted-for-execution" => {
                admitted += 1;
                let (case_writes, case_diagnostics) = execute_h2_5e_observed(workspace, case)?;
                writes += case_writes;
                diagnostics += case_diagnostics;
            }
            "deferred-to-slices"
                if case["required_slices"]
                    .as_array()
                    .is_some_and(|slices| slices.len() == 1 && slices[0] == "H2.9")
                    && case["diagnostic_disposition"]["state"]
                        == "not-observed-source-deferred" =>
            {
                h2_9_deferred += 1;
            }
            disposition => {
                return Err(failure(format!(
                    "unknown H2.5e disposition {disposition} for {}",
                    string(case, "case_id")?,
                )))
            }
        }
    }
    if admitted != 40 || h2_9_deferred != 1 || writes != 46 || diagnostics != 88 {
        return Err(failure(format!(
            "H2.5e execution totals differ: admitted={admitted} h2_9_deferred={h2_9_deferred} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.5e emit acceptance: candidates=41 exact={admitted} h2_9_deferred={h2_9_deferred} exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

/// Execute every dependency-closed H2.5f ES2016 target row twice.
pub fn run_h2_5f(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_5F_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.5f-es2017-target"
        || artifact["selection_contract"]["global_h2_5f_rows"] != 9
        || artifact["selection_contract"]["global_candidate_denominator"] != 8
        || artifact["selection_contract"]["candidate_denominator"] != 8
        || artifact["selection_contract"]["future_deferred_rows"] != 1
        || artifact["summary"]["candidates"] != 8
        || artifact["summary"]["admitted_cases"] != 8
        || artifact["summary"]["deferred_cases"] != 0
        || artifact["summary"]["source_deferred_cases"] != 0
        || artifact["summary"]["admitted_typescript_writes"] != 8
        || artifact["summary"]["admitted_typescript_diagnostics"] != 20
        || artifact["summary"]["unexecuted_candidates"] != 0
        || artifact["summary"]["undispositioned_candidates"] != 0
        || artifact["owner_closure"]
            .as_array()
            .is_none_or(|owners| owners.len() != 1 || owners[0]["key"] != "transform-es2017")
    {
        return Err(failure("H2.5f qualification header is not closed"));
    }
    let cases = array(&artifact, "cases")?;
    if cases.len() != 8 {
        return Err(failure("H2.5f qualification case denominator changed"));
    }
    let mut writes = 0;
    let mut diagnostics = 0;
    for case in cases {
        if string(case, "disposition")? != "admitted-for-execution" {
            return Err(failure(format!(
                "unknown H2.5f disposition {} for {}",
                string(case, "disposition")?,
                string(case, "case_id")?,
            )));
        }
        let (case_writes, case_diagnostics) = execute_h2_5f_observed(workspace, case)?;
        writes += case_writes;
        diagnostics += case_diagnostics;
    }
    if writes != 8 || diagnostics != 20 {
        return Err(failure(format!(
            "H2.5f execution totals differ: admitted={} writes={writes} diagnostics={diagnostics}",
            cases.len(),
        )));
    }
    println!(
        "H2.5f emit acceptance: candidates=8 exact={} exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2",
        cases.len(),
    );
    Ok(())
}

/// Execute every H2.5g ES2015 target row whose reached-source owners close
/// through transformES2016 twice. Later comment/recovery owners remain typed
/// pre-sink deferrals.
pub fn run_h2_5g_probe(workspace: &Path, indices: &[usize]) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_5G_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    let cases = validate_h2_5g_qualification(&artifact)?;
    let mut unique = BTreeSet::new();
    if indices.iter().any(|index| !unique.insert(*index)) {
        return Err(failure("H2.5g probe indices must be unique"));
    }
    let inputs = H2_5gExecutionInputs::load(workspace)?;
    for index in indices {
        let case = cases
            .get(*index)
            .ok_or_else(|| failure(format!("H2.5g probe index {index} is out of range")))?;
        let case_id = string(case, "case_id")?;
        compact_typescript_observation(case)?;
        if classify_h2_5g_case(case)? != H2_5gCaseDisposition::Admitted {
            return Err(failure(format!(
                "H2.5g probe index {index} ({case_id}) is not admitted for execution"
            )));
        }
        let (writes, diagnostics) = execute_h2_5g_observed(workspace, case, &inputs)?;
        println!(
            "H2.5g probe exact index={index} case={case_id} writes={writes} diagnostics={diagnostics} repetitions=2"
        );
    }
    Ok(())
}

/// Execute a half-open H2.5g qualification range and emit one JSON line for
/// each exact-oracle mismatch. Unlike acceptance this diagnostic traversal is
/// intentionally exhaustive: a failing row does not hide independent later
/// owners. The qualification and recorded execution plans are loaded once per
/// range so callers can split the corpus without paying per-case setup costs.
pub fn run_h2_5g_inventory(
    workspace: &Path,
    start: usize,
    end: usize,
) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_5G_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    let cases = validate_h2_5g_qualification(&artifact)?;
    if start >= end || end > cases.len() {
        return Err(failure(format!(
            "H2.5g inventory range must satisfy 0 <= start < end <= {} (got {start}..{end})",
            cases.len(),
        )));
    }
    let inputs = H2_5gExecutionInputs::load(workspace)?;
    let mut failing_cases = 0usize;
    let mut admitted = 0usize;
    let mut h2_8a_deferred = 0usize;
    let mut h2_9_deferred = 0usize;
    for (index, case) in cases.iter().enumerate().take(end).skip(start) {
        let case_id = string(case, "case_id")?.to_owned();
        let result = (|| -> Result<(), Box<dyn Error>> {
            compact_typescript_observation(case)?;
            match classify_h2_5g_case(case)? {
                H2_5gCaseDisposition::Admitted => {
                    admitted += 1;
                    execute_h2_5g_observed(workspace, case, &inputs)?;
                }
                H2_5gCaseDisposition::Deferred(H2_5gDeferredOwner::H2_8a) => {
                    h2_8a_deferred += 1;
                }
                H2_5gCaseDisposition::Deferred(H2_5gDeferredOwner::H2_9) => {
                    h2_9_deferred += 1;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            failing_cases += 1;
            println!(
                "{}",
                json!({
                    "index": index,
                    "case_id": case_id,
                    "error": error.to_string(),
                })
            );
        }
        let processed = index - start + 1;
        if processed.is_multiple_of(256) && index + 1 != end {
            eprintln!(
                "H2.5g inventory progress: range={start}..{end} processed={processed}/{} failing_cases={failing_cases}",
                end - start,
            );
        }
    }
    let classified_cases = admitted + h2_8a_deferred + h2_9_deferred;
    if classified_cases != end - start {
        return Err(failure(format!(
            "H2.5g inventory range classification differs: range={} classified={classified_cases} admitted={admitted} h2_8a_deferred={h2_8a_deferred} h2_9_deferred={h2_9_deferred} failing_cases={failing_cases}",
            end - start,
        )));
    }
    eprintln!(
        "H2.5g inventory complete: range={start}..{end} cases={} admitted={admitted} h2_8a_deferred={h2_8a_deferred} h2_9_deferred={h2_9_deferred} failing_cases={failing_cases}",
        end - start,
    );
    Ok(())
}

pub fn run_h2_5g(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_5G_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    let cases = validate_h2_5g_qualification(&artifact)?;
    let inputs = H2_5gExecutionInputs::load(workspace)?;
    let worker_count = h2_5g_worker_count()?.min(cases.len());
    println!(
        "H2.5g ordered acceptance pipeline: cases={} workers={worker_count}",
        cases.len()
    );
    let results = crate::bounded_pipeline::ordered_map(cases, worker_count, |index, case| {
        execute_h2_5g_case(workspace, case, &inputs)
            .map_err(|error| format!("H2.5g case index {index}: {error}"))
    })?;
    let mut totals = H2_5gCaseTotals::default();
    for result in results {
        totals.add_assign(result.map_err(failure)?);
    }
    let H2_5gCaseTotals {
        admitted,
        h2_8a_deferred,
        h2_9_deferred,
        writes,
        diagnostics,
    } = totals;
    if admitted != 8_511
        || h2_8a_deferred != 6
        || h2_9_deferred != 510
        || writes != 9_466
        || diagnostics != 26_815
    {
        return Err(failure(format!(
            "H2.5g execution totals differ: admitted={admitted} h2_8a_deferred={h2_8a_deferred} h2_9_deferred={h2_9_deferred} writes={writes} diagnostics={diagnostics}"
        )));
    }
    println!(
        "H2.5g emit acceptance: candidates=9027 exact={admitted} h2_8a_deferred={h2_8a_deferred} h2_9_deferred={h2_9_deferred} exact_diagnostics={diagnostics} exact_writes={writes} repetitions=2"
    );
    Ok(())
}

// ---- H2.5h corpus-adoption acceptance (CA-4) ----
//
// The band is honestly divergent: the CA-2a residual families (r1-r5) and
// any project-lane residuals live in a FROZEN divergence manifest with a
// facet-exact pass rule. A new divergence fails; a fixed-but-listed row
// fails (the manifest only shrinks); the shared 5g `assert_*` helpers stay
// untouched — this lane owns its own typed comparisons.

const H2_5H_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-5h-qualification.v1.json";
const H2_5H_KNOWN_DIVERGENCES_RELATIVE_PATH: &str = "ratchets/h2-5h-known-divergences.v1.json";
const H2_5H_WRITE_DIVERGENCES_ENV: &str = "TSRS_H2_5H_WRITE_DIVERGENCES";

fn validate_h2_5h_qualification(artifact: &Value) -> Result<&[Value], Box<dyn Error>> {
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.5h-es5-target"
        || artifact["selection_contract"]["global_h2_5h_rows"] != 2_012
        || artifact["selection_contract"]["global_candidate_denominator"] != 932
        || artifact["selection_contract"]["observed_candidate_denominator"] != 932
        || artifact["selection_contract"]["project_deferred_candidates"] != 0
        || artifact["summary"]["candidates"] != 932
        || artifact["summary"]["observed_candidates"] != 932
        || artifact["summary"]["admitted_cases"] != 888
        || artifact["summary"]["deferred_cases"] != 44
        || artifact["summary"]["project_candidates"] != 82
    {
        return Err(failure(
            "H2.5h qualification artifact contract differs from the CA-4 wiring",
        ));
    }
    let cases = artifact["cases"]
        .as_array()
        .ok_or_else(|| failure("H2.5h qualification cases are not an array"))?;
    if cases.len() != 932 {
        return Err(failure(format!(
            "H2.5h qualification case count changed: {}",
            cases.len()
        )));
    }
    Ok(cases)
}

struct H2_5hExecutionInputs {
    compiler: H2_5gExecutionInputs,
    project_plans: HashMap<String, ProjectExecutionPlan>,
}

impl H2_5hExecutionInputs {
    fn load(workspace: &Path) -> Result<Self, Box<dyn Error>> {
        let compiler = H2_5gExecutionInputs::load(workspace)?;
        let corpus = load_recorded_execution_plans(workspace)?;
        let project_plans = corpus
            .plans
            .iter()
            .filter_map(|recorded| match &recorded.input {
                UpstreamExecutionInput::Project(plan) => {
                    Some((recorded.provenance.case_id.to_string(), plan.clone()))
                }
                UpstreamExecutionInput::Compiler(_) => None,
            })
            .collect::<HashMap<_, _>>();
        if project_plans.len() < 82 {
            return Err(failure(format!(
                "H2.5h recorded project-plan denominator too small: {}",
                project_plans.len(),
            )));
        }
        Ok(Self {
            compiler,
            project_plans,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct H2_5hDivergence {
    writes_diverging: u64,
    diagnostics_diverging: bool,
    emit_result_diverging: bool,
    /// The production emitter's fails-closed option preflight refused the
    /// row (`unsupported emit compiler option` — the l0-source-options
    /// owner's surface): a TYPED refusal, not an infrastructure failure.
    emit_refused: bool,
}

impl H2_5hDivergence {
    fn is_exact(&self) -> bool {
        self.writes_diverging == 0
            && !self.diagnostics_diverging
            && !self.emit_result_diverging
            && !self.emit_refused
    }
}

struct H2_5hCaseOutcome {
    case_id: String,
    deferred: bool,
    divergence: H2_5hDivergence,
}

fn count_diverging_writes(expected: &[Value], actual: &MemoryOutputSink) -> u64 {
    let mut diverging = 0u64;
    let pairs = expected.len().min(actual.writes().len());
    for (expected, actual) in expected.iter().zip(actual.writes()).take(pairs) {
        let path_matches = string(expected, "path")
            .map(|path| actual.path() == Path::new(path))
            .unwrap_or(false);
        let bytes_match = string(expected, "callback_utf8_sha256")
            .map(|sha| sha256(actual.callback_text().as_bytes()) == sha)
            .unwrap_or(false);
        let bom_matches =
            expected["write_byte_order_mark"].as_bool() == Some(actual.write_byte_order_mark());
        if !path_matches || !bytes_match || !bom_matches {
            diverging += 1;
        }
    }
    diverging + expected.len().abs_diff(actual.writes().len()) as u64
}

fn reported_diagnostics_match(expected: &[Value], actual: &[Diagnostic]) -> bool {
    let expected = canonicalize_diagnostic_paths(expected);
    let actual = canonicalize_diagnostic_paths(&normalize_diagnostics(actual));
    actual == expected
}

fn execute_h2_5h_case(
    workspace: &Path,
    case: &Value,
    inputs: &H2_5hExecutionInputs,
) -> Result<H2_5hCaseOutcome, Box<dyn Error>> {
    let case_id = string(case, "case_id")?.to_owned();
    match string(case, "disposition")? {
        "admitted-for-execution" => {}
        "deferred-to-slices" => {
            return Ok(H2_5hCaseOutcome {
                case_id,
                deferred: true,
                divergence: H2_5hDivergence::default(),
            });
        }
        other => {
            return Err(failure(format!(
                "{case_id}: unexpected H2.5h disposition {other}"
            )));
        }
    }
    let expected = compact_typescript_observation(case)?;
    // Prepare once; clone for the deterministic repetition (the 5g model).
    let first_program = match string(case, "execution_route")? {
        "project-mount" => {
            let plan = inputs
                .project_plans
                .get(&case_id)
                .ok_or_else(|| failure(format!("{case_id}: recorded project plan is absent")))?;
            load_project_emit(workspace, plan, limits())
                .map_err(|error| failure(format!("{case_id}: project prepare failed: {error}")))?
                .prepared_program
        }
        _ => inputs.compiler.prepare(workspace, case)?,
    };
    let second_program = first_program.clone();
    let first_session = ProgramSession::new(first_program);
    let harness_lib_bundle = first_session.prepare_harness_lib_bundle()?;
    let mut first_sink = MemoryOutputSink::new();
    let (first, first_reported) = match first_session
        .emit_with_reported_diagnostics_for_harness_with_lib_bundle(
            &mut first_sink,
            harness_lib_bundle.as_ref(),
        ) {
        Ok(result) => result,
        Err(error) => {
            let message = error.to_string();
            if message.contains("unsupported emit compiler option") {
                return Ok(H2_5hCaseOutcome {
                    case_id,
                    deferred: false,
                    divergence: H2_5hDivergence {
                        emit_refused: true,
                        ..H2_5hDivergence::default()
                    },
                });
            }
            return Err(failure(format!(
                "{case_id}: first Rust emit failed: {message}"
            )));
        }
    };
    let mut second_sink = MemoryOutputSink::new();
    let (second, second_reported) = ProgramSession::new(second_program)
        .emit_with_reported_diagnostics_for_harness_with_lib_bundle(
            &mut second_sink,
            harness_lib_bundle.as_ref(),
        )
        .map_err(|error| failure(format!("{case_id}: second Rust emit failed: {error}")))?;
    if first != second || first_sink != second_sink || first_reported != second_reported {
        return Err(failure(format!(
            "{case_id}: repeated Rust emit is not deterministic"
        )));
    }
    // The unadmitted-runtime-slice guard with H2.5h ADMITTED for this lane
    // only; every other slice's acceptance keeps its own strict list.
    let activity = first.h2_activity();
    for slice in H2RuntimeSlice::ALL {
        if !matches!(
            slice,
            H2RuntimeSlice::H2_1a
                | H2RuntimeSlice::H2_1b
                | H2RuntimeSlice::H2_1c
                | H2RuntimeSlice::H2_1d
                | H2RuntimeSlice::H2_1e
                | H2RuntimeSlice::H2_2a
                | H2RuntimeSlice::H2_2b
                | H2RuntimeSlice::H2_2c
                | H2RuntimeSlice::H2_2d
                | H2RuntimeSlice::H2_3a
                | H2RuntimeSlice::H2_3b
                | H2RuntimeSlice::H2_3c
                | H2RuntimeSlice::H2_3d
                | H2RuntimeSlice::H2_4a
                | H2RuntimeSlice::H2_4b
                | H2RuntimeSlice::H2_5a
                | H2RuntimeSlice::H2_5b
                | H2RuntimeSlice::H2_5c
                | H2RuntimeSlice::H2_5d
                | H2RuntimeSlice::H2_5e
                | H2RuntimeSlice::H2_5f
                | H2RuntimeSlice::H2_5g
                | H2RuntimeSlice::H2_5h
        ) && activity.runtime_slice(slice) != 0
        {
            return Err(failure(format!(
                "{case_id}: unadmitted {} activity",
                slice.name()
            )));
        }
    }
    // Typed comparisons (never opaque errors): writes count ALL diverging
    // entries; the emit result uses the exact emit-result-diagnostics
    // compare (the CA-2b blocked-row contract) instead of the 5g lane's
    // both-empty requirement.
    let expected_writes = array(expected, "writes")?;
    let writes_diverging = count_diverging_writes(expected_writes, &first_sink);
    let expected_reported = array(expected, "reported_diagnostics")?;
    let diagnostics_diverging = !reported_diagnostics_match(expected_reported, &first_reported);
    let actual_exit_code = if first.emit_skipped() && !first_reported.is_empty() {
        1
    } else if !first_reported.is_empty() {
        2
    } else {
        0
    };
    let expected_emit_diagnostics = expected["emit_result"]["diagnostics"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let emit_result_diverging = first.emit_skipped()
        != expected["emit_result"]["emit_skipped"]
            .as_bool()
            .unwrap_or(true)
        || !reported_diagnostics_match(expected_emit_diagnostics, first.diagnostics())
        || first.emitted_files().is_some() == expected["emit_result"]["emitted_files"].is_null()
        || first.source_maps().is_some() == expected["emit_result"]["source_maps"].is_null()
        || !array(expected, "status_writes")?.is_empty()
        || expected["exit_code"].as_i64() != Some(actual_exit_code);
    Ok(H2_5hCaseOutcome {
        case_id,
        deferred: false,
        divergence: H2_5hDivergence {
            writes_diverging,
            diagnostics_diverging,
            emit_result_diverging,
            emit_refused: false,
        },
    })
}

fn load_h2_5h_divergence_manifest(
    workspace: &Path,
) -> Result<HashMap<String, H2_5hDivergence>, Box<dyn Error>> {
    let manifest: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_5H_KNOWN_DIVERGENCES_RELATIVE_PATH),
    )?)?;
    if manifest["schema"] != 1 {
        return Err(failure("H2.5h divergence manifest schema differs"));
    }
    let mut listed = HashMap::new();
    for entry in array(&manifest, "cases")? {
        let case_id = string(entry, "case_id")?.to_owned();
        let owner = string(entry, "owner")?;
        if !(owner.starts_with("h2-5h-ca-2a-r") || owner.starts_with("h2-5h-project-r")) {
            return Err(failure(format!(
                "H2.5h divergence manifest entry {case_id} carries an un-named owner {owner}"
            )));
        }
        let divergence = H2_5hDivergence {
            writes_diverging: entry["writes_diverging"].as_u64().unwrap_or(0),
            diagnostics_diverging: entry["diagnostics_diverging"].as_bool().unwrap_or(false),
            emit_result_diverging: entry["emit_result_diverging"].as_bool().unwrap_or(false),
            emit_refused: entry["emit_refused"].as_bool().unwrap_or(false),
        };
        if divergence.is_exact() {
            return Err(failure(format!(
                "H2.5h divergence manifest entry {case_id} lists no divergence facet"
            )));
        }
        if listed.insert(case_id.clone(), divergence).is_some() {
            return Err(failure(format!(
                "H2.5h divergence manifest duplicates {case_id}"
            )));
        }
    }
    Ok(listed)
}

fn write_h2_5h_divergence_manifest(
    workspace: &Path,
    diverging: &[(String, H2_5hDivergence)],
) -> Result<(), Box<dyn Error>> {
    let cases = diverging
        .iter()
        .map(|(case_id, divergence)| {
            serde_json::json!({
                "case_id": case_id,
                "owner": "UNASSIGNED-review-and-name",
                "writes_diverging": divergence.writes_diverging,
                "diagnostics_diverging": divergence.diagnostics_diverging,
                "emit_result_diverging": divergence.emit_result_diverging,
                "emit_refused": divergence.emit_refused,
            })
        })
        .collect::<Vec<_>>();
    let body = serde_json::json!({ "schema": 1, "cases": cases });
    let rendered = format!("{}\n", serde_json::to_string_pretty(&body)?);
    fs::write(
        workspace.join(H2_5H_KNOWN_DIVERGENCES_RELATIVE_PATH),
        rendered,
    )?;
    println!(
        "H2.5h divergence manifest written: {} entries (owners UNASSIGNED - review and name)",
        diverging.len()
    );
    Ok(())
}

/// The four-outcome divergence ratchet (CA-4 packet §4):
/// diverging+listed(facet-exact) = pass, diverging+unlisted = FAIL,
/// exact+listed = FAIL (stale entry — the manifest only shrinks),
/// exact+unlisted = pass. In write mode the join only accumulates.
#[allow(clippy::type_complexity)]
fn h2_5h_ratchet_join(
    results: Vec<Result<H2_5hCaseOutcome, String>>,
    listed: &HashMap<String, H2_5hDivergence>,
    write_manifest: bool,
) -> Result<(u64, u64, Vec<(String, H2_5hDivergence)>), Box<dyn Error>> {
    h2_slice_ratchet_join("H2.5h", results, listed, write_manifest)
}

/// The shared four-outcome join with the slice label threaded so the
/// H2.6a lane's failures name themselves (ca-2; otherwise byte-identical
/// to the CA-4 join).
#[allow(clippy::type_complexity)]
fn h2_slice_ratchet_join(
    slice: &str,
    results: Vec<Result<H2_5hCaseOutcome, String>>,
    listed: &HashMap<String, H2_5hDivergence>,
    write_manifest: bool,
) -> Result<(u64, u64, Vec<(String, H2_5hDivergence)>), Box<dyn Error>> {
    let mut deferred = 0u64;
    let mut exact = 0u64;
    let mut diverging: Vec<(String, H2_5hDivergence)> = Vec::new();
    for result in results {
        let outcome = result.map_err(failure)?;
        if outcome.deferred {
            deferred += 1;
        } else if outcome.divergence.is_exact() {
            exact += 1;
            if !write_manifest && listed.contains_key(&outcome.case_id) {
                return Err(failure(format!(
                    "{slice} stale divergence-manifest entry: {} is exact now (shrink the manifest)",
                    outcome.case_id
                )));
            }
        } else {
            if !write_manifest {
                match listed.get(&outcome.case_id) {
                    None => {
                        return Err(failure(format!(
                            "{slice} NEW divergence (not in the manifest): {} writes={} diagnostics={} emit_result={} refused={}",
                            outcome.case_id,
                            outcome.divergence.writes_diverging,
                            outcome.divergence.diagnostics_diverging,
                            outcome.divergence.emit_result_diverging,
                            outcome.divergence.emit_refused,
                        )));
                    }
                    Some(expected) if *expected != outcome.divergence => {
                        return Err(failure(format!(
                            "{slice} divergence facets differ from the manifest for {}: observed writes={} diagnostics={} emit_result={} refused={}",
                            outcome.case_id,
                            outcome.divergence.writes_diverging,
                            outcome.divergence.diagnostics_diverging,
                            outcome.divergence.emit_result_diverging,
                            outcome.divergence.emit_refused,
                        )));
                    }
                    Some(_) => {}
                }
            }
            diverging.push((outcome.case_id, outcome.divergence));
        }
    }
    Ok((exact, deferred, diverging))
}

pub fn run_h2_5h(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_5H_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    let cases = validate_h2_5h_qualification(&artifact)?;
    let write_manifest = std::env::var_os(H2_5H_WRITE_DIVERGENCES_ENV).is_some();
    let listed = if write_manifest {
        HashMap::new()
    } else {
        load_h2_5h_divergence_manifest(workspace)?
    };
    let inputs = H2_5hExecutionInputs::load(workspace)?;
    let worker_count = h2_5g_worker_count()?.min(cases.len());
    println!(
        "H2.5h ordered acceptance pipeline: cases={} workers={worker_count}",
        cases.len()
    );
    let results = crate::bounded_pipeline::ordered_map(cases, worker_count, |index, case| {
        execute_h2_5h_case(workspace, case, &inputs)
            .map_err(|error| format!("H2.5h case index {index}: {error}"))
    })?;
    let (exact, deferred, mut diverging) = h2_5h_ratchet_join(results, &listed, write_manifest)?;
    if write_manifest {
        diverging.sort_by(|left, right| left.0.cmp(&right.0));
        write_h2_5h_divergence_manifest(workspace, &diverging)?;
    } else if diverging.len() != listed.len() {
        return Err(failure(format!(
            "H2.5h divergence-manifest coverage differs: observed {} listed {}",
            diverging.len(),
            listed.len()
        )));
    }
    if exact + diverging.len() as u64 != 888 || deferred != 44 {
        return Err(failure(format!(
            "H2.5h execution totals differ: exact={exact} known_diverging={} deferred={deferred}",
            diverging.len()
        )));
    }
    println!(
        "H2.5h emit acceptance: candidates=932 exact={exact} known_diverging={} deferred={deferred} repetitions=2",
        diverging.len()
    );
    Ok(())
}

const H2_6A_QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-6a-qualification.v1.json";
const H2_6A_KNOWN_DIVERGENCES_RELATIVE_PATH: &str = "ratchets/h2-6a-known-divergences.v1.json";
const H2_6A_WRITE_DIVERGENCES_ENV: &str = "TSRS_H2_6A_WRITE_DIVERGENCES";

fn validate_h2_6a_qualification(artifact: &Value) -> Result<&[Value], Box<dyn Error>> {
    if artifact["schema"] != 1
        || artifact["status"] != "qualified-typescript-oracle"
        || artifact["phase"] != "H2.6a-source-map"
        || artifact["selection_contract"]["global_h2_6a_rows"] != 630
        || artifact["selection_contract"]["global_candidate_denominator"] != 177
        || artifact["selection_contract"]["observed_candidate_denominator"] != 177
        || artifact["selection_contract"]["project_deferred_candidates"] != 0
        || artifact["summary"]["candidates"] != 177
        || artifact["summary"]["observed_candidates"] != 177
        || artifact["summary"]["admitted_cases"] != 175
        || artifact["summary"]["deferred_cases"] != 2
        || artifact["summary"]["project_candidates"] != 0
    {
        return Err(failure(
            "H2.6a qualification artifact contract differs from the ca-2 wiring",
        ));
    }
    let cases = artifact["cases"]
        .as_array()
        .ok_or_else(|| failure("H2.6a qualification cases are not an array"))?;
    if cases.len() != 177 {
        return Err(failure(format!(
            "H2.6a qualification case count changed: {}",
            cases.len()
        )));
    }
    Ok(cases)
}

/// The H2.6a write compare extends the 5h facet with the callback `data`
/// argument: presence, `sourceMapUrlPos`, and the diagnostics count the
/// oracle recorded per write (ca-2 packet §2.4 — the only compare-side
/// delta).
fn count_diverging_writes_with_data(expected: &[Value], actual: &MemoryOutputSink) -> u64 {
    let mut diverging = count_diverging_writes(expected, actual);
    let pairs = expected.len().min(actual.writes().len());
    for (expected, actual) in expected.iter().zip(actual.writes()).take(pairs) {
        let metadata = actual.metadata();
        let data_present = metadata.is_some();
        let (url_position, diagnostics_count) = match metadata {
            Some(EmitWriteMetadata::Text(text)) => (
                text.source_map_url_position()
                    .map(|position| u64::from(position.value())),
                Some(text.diagnostics().len() as u64),
            ),
            Some(EmitWriteMetadata::BuildInfo(_)) => (None, None),
            None => (None, None),
        };
        let expected_present = expected["data_present"].as_bool().unwrap_or(false);
        let expected_url = expected["data_source_map_url_pos"].as_u64();
        let expected_diagnostics = expected["data_diagnostics_count"].as_u64();
        if data_present != expected_present
            || url_position != expected_url
            || diagnostics_count != expected_diagnostics
        {
            diverging += 1;
        }
    }
    diverging
}

fn emitted_files_match(expected: &Value, actual: Option<&[PathBuf]>) -> bool {
    match (expected.as_array(), actual) {
        (None, None) => expected.is_null(),
        (Some(expected), Some(actual)) => {
            expected.len() == actual.len()
                && expected.iter().zip(actual).all(|(expected, actual)| {
                    expected.as_str().map(Path::new) == Some(actual.as_path())
                })
        }
        _ => false,
    }
}

fn source_maps_match(
    expected: &Value,
    actual: Option<&[tsc_compiler::SourceMapObservation]>,
) -> bool {
    match (expected.as_array(), actual) {
        (None, None) => expected.is_null(),
        (Some(expected), Some(actual)) => {
            expected.len() == actual.len()
                && expected.iter().zip(actual).all(|(expected, actual)| {
                    let names_match = match expected["input_source_file_names"].as_array() {
                        Some(names) => {
                            names.len() == actual.input_source_files().len()
                                && names.iter().zip(actual.input_source_files()).all(
                                    |(name, actual)| {
                                        name.as_str().map(Path::new) == Some(actual.as_path())
                                    },
                                )
                        }
                        None => false,
                    };
                    names_match
                        && expected["source_map_json"].as_str() == Some(actual.canonical_json())
                })
        }
        _ => false,
    }
}

/// The 6a prepare: the shared 5g plan/VFS reconstruction with the
/// `sourceMap` floor projected (the ca-1 oracle observed WITH maps; the
/// established floor would leave every emit mapless — the ca-2 first
/// sweep proved exactly that as 175 uniform write/emit-result facets).
fn prepare_h2_6a_case(
    workspace: &Path,
    case: &Value,
    inputs: &H2_5gExecutionInputs,
) -> Result<tsc_program::PreparedProgram, Box<dyn Error>> {
    match string(case, "execution_route")? {
        "qualified-vfs" => case_input_with_floor(workspace, case, EmitOptionFloor::SourceMap),
        "recorded-compiler-plan" => {
            let case_id = string(case, "case_id")?;
            let recorded = inputs
                .compiler_cases
                .get(case_id)
                .ok_or_else(|| failure(format!("{case_id}: recorded compiler plan is absent")))?;
            if case["suite"] != "compiler"
                || case["expansion_case"].as_u64() != Some(u64::from(recorded.expansion_case))
            {
                return Err(failure(format!(
                    "{case_id}: recorded compiler-plan provenance differs"
                )));
            }
            Ok(load_compiler_emit_with_option_floor(
                workspace,
                &recorded.plan,
                limits(),
                EmitOptionFloor::SourceMap,
            )?)
        }
        route => Err(failure(format!(
            "{}: unexpected H2.6a execution route {route}",
            string(case, "case_id")?,
        ))),
    }
}

fn execute_h2_6a_case(
    workspace: &Path,
    case: &Value,
    inputs: &H2_5gExecutionInputs,
) -> Result<H2_5hCaseOutcome, Box<dyn Error>> {
    let case_id = string(case, "case_id")?.to_owned();
    match string(case, "disposition")? {
        "admitted-for-execution" => {}
        "deferred-to-slices" => {
            return Ok(H2_5hCaseOutcome {
                case_id,
                deferred: true,
                divergence: H2_5hDivergence::default(),
            });
        }
        other => {
            return Err(failure(format!(
                "{case_id}: unexpected H2.6a disposition {other}"
            )));
        }
    }
    let expected = compact_typescript_observation(case)?;
    match string(case, "execution_route")? {
        "recorded-compiler-plan" | "qualified-vfs" => {}
        "project-mount" => {
            return Err(failure(format!(
                "{case_id}: the H2.6a band carries no project rows (ca-1 census guard)"
            )));
        }
        other => {
            return Err(failure(format!(
                "{case_id}: unexpected H2.6a execution route {other}"
            )));
        }
    }
    let first_program = prepare_h2_6a_case(workspace, case, inputs)?;
    let second_program = first_program.clone();
    let first_session = ProgramSession::new(first_program);
    let harness_lib_bundle = first_session.prepare_harness_lib_bundle()?;
    let mut first_sink = MemoryOutputSink::new();
    let (first, first_reported) = match first_session
        .emit_with_reported_diagnostics_for_harness_with_lib_bundle(
            &mut first_sink,
            harness_lib_bundle.as_ref(),
        ) {
        Ok(result) => result,
        Err(error) => {
            let message = error.to_string();
            if message.contains("unsupported emit compiler option") {
                return Ok(H2_5hCaseOutcome {
                    case_id,
                    deferred: false,
                    divergence: H2_5hDivergence {
                        emit_refused: true,
                        ..H2_5hDivergence::default()
                    },
                });
            }
            return Err(failure(format!(
                "{case_id}: first Rust emit failed: {message}"
            )));
        }
    };
    let mut second_sink = MemoryOutputSink::new();
    let (second, second_reported) = ProgramSession::new(second_program)
        .emit_with_reported_diagnostics_for_harness_with_lib_bundle(
            &mut second_sink,
            harness_lib_bundle.as_ref(),
        )
        .map_err(|error| failure(format!("{case_id}: second Rust emit failed: {error}")))?;
    if first != second || first_sink != second_sink || first_reported != second_reported {
        return Err(failure(format!(
            "{case_id}: repeated Rust emit is not deterministic"
        )));
    }
    // The unadmitted-runtime-slice guard with the H2.1a..H2.5h ladder plus
    // H2.6a admitted for this lane.
    let activity = first.h2_activity();
    for slice in H2RuntimeSlice::ALL {
        if !matches!(
            slice,
            H2RuntimeSlice::H2_1a
                | H2RuntimeSlice::H2_1b
                | H2RuntimeSlice::H2_1c
                | H2RuntimeSlice::H2_1d
                | H2RuntimeSlice::H2_1e
                | H2RuntimeSlice::H2_2a
                | H2RuntimeSlice::H2_2b
                | H2RuntimeSlice::H2_2c
                | H2RuntimeSlice::H2_2d
                | H2RuntimeSlice::H2_3a
                | H2RuntimeSlice::H2_3b
                | H2RuntimeSlice::H2_3c
                | H2RuntimeSlice::H2_3d
                | H2RuntimeSlice::H2_4a
                | H2RuntimeSlice::H2_4b
                | H2RuntimeSlice::H2_5a
                | H2RuntimeSlice::H2_5b
                | H2RuntimeSlice::H2_5c
                | H2RuntimeSlice::H2_5d
                | H2RuntimeSlice::H2_5e
                | H2RuntimeSlice::H2_5f
                | H2RuntimeSlice::H2_5g
                | H2RuntimeSlice::H2_5h
                | H2RuntimeSlice::H2_6a
        ) && activity.runtime_slice(slice) != 0
        {
            return Err(failure(format!(
                "{case_id}: unadmitted {} activity",
                slice.name()
            )));
        }
    }
    let expected_writes = array(expected, "writes")?;
    let writes_diverging = count_diverging_writes_with_data(expected_writes, &first_sink);
    let expected_reported = array(expected, "reported_diagnostics")?;
    let diagnostics_diverging = !reported_diagnostics_match(expected_reported, &first_reported);
    let actual_exit_code = if first.emit_skipped() && !first_reported.is_empty() {
        1
    } else if !first_reported.is_empty() {
        2
    } else {
        0
    };
    let expected_emit_diagnostics = expected["emit_result"]["diagnostics"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let emit_result_diverging = first.emit_skipped()
        != expected["emit_result"]["emit_skipped"]
            .as_bool()
            .unwrap_or(true)
        || !reported_diagnostics_match(expected_emit_diagnostics, first.diagnostics())
        || !emitted_files_match(
            &expected["emit_result"]["emitted_files"],
            first.emitted_files(),
        )
        || !source_maps_match(&expected["emit_result"]["source_maps"], first.source_maps())
        || !array(expected, "status_writes")?.is_empty()
        || expected["exit_code"].as_i64() != Some(actual_exit_code);
    Ok(H2_5hCaseOutcome {
        case_id,
        deferred: false,
        divergence: H2_5hDivergence {
            writes_diverging,
            diagnostics_diverging,
            emit_result_diverging,
            emit_refused: false,
        },
    })
}

fn load_h2_6a_divergence_manifest(
    workspace: &Path,
) -> Result<HashMap<String, H2_5hDivergence>, Box<dyn Error>> {
    let path = workspace.join(H2_6A_KNOWN_DIVERGENCES_RELATIVE_PATH);
    // The expected steady state is an ABSENT manifest (the m-3 witness
    // floor was byte-exact through the production path); a file exists
    // only after a first sweep proved diverging rows.
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let manifest: Value = serde_json::from_slice(&fs::read(path)?)?;
    if manifest["schema"] != 1 {
        return Err(failure("H2.6a divergence manifest schema differs"));
    }
    let mut listed = HashMap::new();
    for entry in array(&manifest, "cases")? {
        let case_id = string(entry, "case_id")?.to_owned();
        let owner = string(entry, "owner")?;
        if !owner.starts_with("h2-6a-") {
            return Err(failure(format!(
                "H2.6a divergence manifest entry {case_id} carries an un-named owner {owner}"
            )));
        }
        let divergence = H2_5hDivergence {
            writes_diverging: entry["writes_diverging"].as_u64().unwrap_or(0),
            diagnostics_diverging: entry["diagnostics_diverging"].as_bool().unwrap_or(false),
            emit_result_diverging: entry["emit_result_diverging"].as_bool().unwrap_or(false),
            emit_refused: entry["emit_refused"].as_bool().unwrap_or(false),
        };
        if divergence.is_exact() {
            return Err(failure(format!(
                "H2.6a divergence manifest entry {case_id} lists no divergence facet"
            )));
        }
        if listed.insert(case_id.clone(), divergence).is_some() {
            return Err(failure(format!(
                "H2.6a divergence manifest duplicates {case_id}"
            )));
        }
    }
    Ok(listed)
}

fn write_h2_6a_divergence_manifest(
    workspace: &Path,
    diverging: &[(String, H2_5hDivergence)],
) -> Result<(), Box<dyn Error>> {
    let cases = diverging
        .iter()
        .map(|(case_id, divergence)| {
            serde_json::json!({
                "case_id": case_id,
                "owner": "UNASSIGNED-review-and-name",
                "writes_diverging": divergence.writes_diverging,
                "diagnostics_diverging": divergence.diagnostics_diverging,
                "emit_result_diverging": divergence.emit_result_diverging,
                "emit_refused": divergence.emit_refused,
            })
        })
        .collect::<Vec<_>>();
    let body = serde_json::json!({ "schema": 1, "cases": cases });
    let rendered = format!("{}\n", serde_json::to_string_pretty(&body)?);
    fs::write(
        workspace.join(H2_6A_KNOWN_DIVERGENCES_RELATIVE_PATH),
        rendered,
    )?;
    println!(
        "H2.6a divergence manifest written: {} entries (owners UNASSIGNED - review and name)",
        diverging.len()
    );
    Ok(())
}

pub fn run_h2_6a(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let artifact: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_6A_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    let cases = validate_h2_6a_qualification(&artifact)?;
    let write_manifest = std::env::var_os(H2_6A_WRITE_DIVERGENCES_ENV).is_some();
    let listed = if write_manifest {
        HashMap::new()
    } else {
        load_h2_6a_divergence_manifest(workspace)?
    };
    let inputs = H2_5gExecutionInputs::load(workspace)?;
    let worker_count = h2_5g_worker_count()?.min(cases.len());
    println!(
        "H2.6a ordered acceptance pipeline: cases={} workers={worker_count}",
        cases.len()
    );
    let results = crate::bounded_pipeline::ordered_map(cases, worker_count, |index, case| {
        execute_h2_6a_case(workspace, case, &inputs)
            .map_err(|error| format!("H2.6a case index {index}: {error}"))
    })?;
    let (exact, deferred, mut diverging) =
        h2_slice_ratchet_join("H2.6a", results, &listed, write_manifest)?;
    if write_manifest {
        if diverging.is_empty() {
            println!("H2.6a first sweep proved zero diverging rows: no manifest is created");
        } else {
            diverging.sort_by(|left, right| left.0.cmp(&right.0));
            write_h2_6a_divergence_manifest(workspace, &diverging)?;
        }
    } else if diverging.len() != listed.len() {
        return Err(failure(format!(
            "H2.6a divergence-manifest coverage differs: observed {} listed {}",
            diverging.len(),
            listed.len()
        )));
    }
    if exact + diverging.len() as u64 != 175 || deferred != 2 {
        return Err(failure(format!(
            "H2.6a execution totals differ: exact={exact} known_diverging={} deferred={deferred}",
            diverging.len()
        )));
    }
    println!(
        "H2.6a emit acceptance: candidates=177 exact={exact} known_diverging={} deferred={deferred} repetitions=2",
        diverging.len()
    );
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/h2_2c_acceptance/tests.rs"]
mod tests;
