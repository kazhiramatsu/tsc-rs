use std::collections::VecDeque;
use std::error::Error;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tsc_binder::BinderWorker;
use tsc_checker::{
    BoundDocument, DocumentAddress, DocumentRegistry, DocumentScriptKind,
    IncrementalDocumentOptions, ParsedDocument, ProgramSnapshot,
};
use tsc_diagnostics::{ByteTextSpan, DocumentVersion, TextSnapshot, VersionedTextStore};
use tsc_syntax::{
    create_language_service_source_file, parse_source_file_from_snapshot_in_identity_domain,
    source_files_structurally_equal, IncrementalParseOptions, ParseOptions,
};
use tsc_types::{CompilerOptions, IdentityDomain, IdentitySpace};

const DEFAULT_SEED: u64 = 0x1ac1_5eed_fae5_0001;
const DEFAULT_EDITS: usize = 512;
const DEFAULT_MAX_RSS_BYTES: u64 = 512 * 1024 * 1024;
const TRACE_LIMIT: usize = 128;
const DIAGNOSTIC_LIMIT: usize = 16;

#[derive(Clone, Copy)]
struct Settings {
    seed: u64,
    edits: usize,
    max_rss_bytes: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct DiagnosticPin {
    code: u32,
    start: Option<u32>,
    length: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct EditTrace {
    ordinal: usize,
    version: String,
    start_byte: u32,
    delete_bytes: u32,
    inserted: String,
    incremental: bool,
    reused_list_elements: usize,
    reused_nodes: usize,
    reused_node_arrays: usize,
    freshly_parsed_nodes: usize,
    node_range: [u32; 2],
    array_range: [u32; 2],
    parse_diagnostics: Vec<DiagnosticPin>,
    jsdoc_diagnostics: Vec<DiagnosticPin>,
}

#[derive(Default)]
struct StressState {
    completed: usize,
    recent: VecDeque<EditTrace>,
    minimum_reused_nodes: usize,
    maximum_freshly_parsed_nodes: usize,
    final_registry_entries: usize,
    final_active_ranges: usize,
}

#[derive(Debug, Deserialize, Serialize)]
struct StressReport {
    schema: u32,
    kind: String,
    status: String,
    seed: String,
    fixture: String,
    initial_text_sha256: String,
    requested_edits: usize,
    completed_edits: usize,
    minimum_reused_nodes: usize,
    maximum_freshly_parsed_nodes: usize,
    final_registry_entries: usize,
    final_active_ranges: usize,
    elapsed_millis: u128,
    peak_rss_bytes: Option<u64>,
    rss_measurement: String,
    rss_ceiling_bytes: u64,
    trace_limit: usize,
    recent_edits: Vec<EditTrace>,
    error: Option<String>,
}

struct Arguments {
    settings: Settings,
    fixture: PathBuf,
    report_path: PathBuf,
    internal_stress_child: bool,
}

struct ChildWait {
    status: ExitStatus,
    peak_rss_bytes: Option<u64>,
    mechanism: &'static str,
}

pub(crate) fn run(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let arguments = parse_arguments(args)?;
    if arguments.internal_stress_child {
        let report = execute_report(arguments.settings, &arguments.fixture)?;
        write_report(&arguments.report_path, &report)?;
        return Ok(());
    }
    run_parent(arguments)
}

fn run_parent(arguments: Arguments) -> Result<(), Box<dyn Error>> {
    let internal_report_path = internal_report_path(&arguments.report_path)?;
    let temporary_report = TemporaryReport(internal_report_path.clone());
    let executable = std::env::current_exe()?;
    let mut command = Command::new(executable);
    command
        .args(["l1", "incremental-stress", "--internal-stress-child"])
        .arg("--fixture")
        .arg(&arguments.fixture)
        .arg("--seed")
        .arg(format!("0x{:016x}", arguments.settings.seed))
        .arg("--edits")
        .arg(arguments.settings.edits.to_string())
        .arg("--max-rss-bytes")
        .arg(arguments.settings.max_rss_bytes.to_string())
        .arg("--report")
        .arg(&internal_report_path);
    let child = command.spawn()?;
    let child_wait = wait_for_stress_child(child)?;
    if !child_wait.status.success() {
        return Err(format!(
            "L1 incremental stress child failed with {}",
            child_wait.status
        )
        .into());
    }
    let mut report: StressReport = serde_json::from_slice(&fs::read(&internal_report_path)?)?;
    report.peak_rss_bytes = child_wait.peak_rss_bytes;
    report.rss_measurement = child_wait.mechanism.to_owned();
    if report.error.is_none() {
        if let Some(peak) = report
            .peak_rss_bytes
            .filter(|peak| *peak > arguments.settings.max_rss_bytes)
        {
            report.error = Some(format!(
                "peak RSS {peak} exceeds reviewed ceiling {}",
                arguments.settings.max_rss_bytes
            ));
        }
    }
    report.status = if report.error.is_none() {
        "passed".to_owned()
    } else {
        "failed".to_owned()
    };
    write_report(&arguments.report_path, &report)?;
    drop(temporary_report);
    println!(
        "L1 incremental stress: status={} edits={}/{} min-reuse={} max-fresh={} child-peak-rss={:?}/{} rss-mechanism={} report={}",
        report.status,
        report.completed_edits,
        report.requested_edits,
        report.minimum_reused_nodes,
        report.maximum_freshly_parsed_nodes,
        report.peak_rss_bytes,
        report.rss_ceiling_bytes,
        report.rss_measurement,
        arguments.report_path.display()
    );
    if report.status != "passed" {
        return Err(format!(
            "L1 incremental stress failed: {}",
            report.error.as_deref().unwrap_or("unknown failure")
        )
        .into());
    }
    Ok(())
}

fn execute_report(settings: Settings, fixture: &Path) -> Result<StressReport, Box<dyn Error>> {
    let initial = fs::read_to_string(fixture)?;
    let initial_hash = format!("{:x}", Sha256::digest(initial.as_bytes()));
    let started = Instant::now();
    let mut state = StressState {
        minimum_reused_nodes: usize::MAX,
        ..StressState::default()
    };
    let execution = catch_unwind(AssertUnwindSafe(|| execute(settings, &initial, &mut state)));
    let error = match execution {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(payload) => Some(
            payload
                .downcast_ref::<&str>()
                .map(|value| (*value).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "incremental stress panicked with a non-string payload".into()),
        ),
    };
    let peak_rss_bytes = peak_rss_bytes();
    let status = if error.is_none() { "passed" } else { "failed" };
    Ok(StressReport {
        schema: 1,
        kind: "l1-incremental-parser-stress".to_owned(),
        status: status.to_owned(),
        seed: format!("0x{:016x}", settings.seed),
        fixture: fixture.display().to_string(),
        initial_text_sha256: initial_hash,
        requested_edits: settings.edits,
        completed_edits: state.completed,
        minimum_reused_nodes: if state.minimum_reused_nodes == usize::MAX {
            0
        } else {
            state.minimum_reused_nodes
        },
        maximum_freshly_parsed_nodes: state.maximum_freshly_parsed_nodes,
        final_registry_entries: state.final_registry_entries,
        final_active_ranges: state.final_active_ranges,
        elapsed_millis: started.elapsed().as_millis(),
        peak_rss_bytes,
        rss_measurement: self_peak_rss_mechanism().to_owned(),
        rss_ceiling_bytes: settings.max_rss_bytes,
        trace_limit: TRACE_LIMIT,
        recent_edits: state.recent.into_iter().collect(),
        error,
    })
}

fn execute(settings: Settings, initial: &str, state: &mut StressState) -> Result<(), String> {
    if initial.len() < 1_000_000 {
        return Err("L1 stress fixture must be at least 1,000,000 bytes".into());
    }
    let domain = IdentityDomain::reclaiming();
    let compiler_options = CompilerOptions::default();
    let parse_options = ParseOptions::default();
    let changed_address = DocumentAddress::new(
        "l1-stress",
        "/large-edit.ts",
        DocumentScriptKind::TypeScript,
        compiler_options.clone(),
    );
    let stable_address = DocumentAddress::new(
        "l1-stress",
        "/stable.ts",
        DocumentScriptKind::TypeScript,
        compiler_options.clone(),
    );
    let mut store = VersionedTextStore::new(initial, DocumentVersion::new("0"));
    let stable_snapshot = TextSnapshot::new(
        "export interface Stable { value: string }",
        DocumentVersion::new("stable"),
    );
    let mut registry = DocumentRegistry::new("l1-stress");
    let first_snapshot = store.current_snapshot();
    let mut changed = registry
        .acquire(changed_address.clone(), Arc::clone(&first_snapshot), || {
            bound_document(
                "/large-edit.ts",
                Arc::clone(&first_snapshot),
                &compiler_options,
                &parse_options,
                &domain,
            )
            .expect("initial stress document must bind")
        })
        .map_err(|error| error.to_string())?;
    let stable = registry
        .acquire(stable_address.clone(), Arc::clone(&stable_snapshot), || {
            bound_document(
                "/stable.ts",
                Arc::clone(&stable_snapshot),
                &compiler_options,
                &parse_options,
                &domain,
            )
            .expect("stable stress document must bind")
        })
        .map_err(|error| error.to_string())?;
    let stable_document = Arc::clone(stable.document());
    let insertions = ["", "x", "😀", "編集", "\n", "/*c*/", "=>", "?", ";"];
    let mut random = settings.seed;

    for ordinal in 0..settings.edits {
        let current = changed.document().source().text();
        let mut start = (next_random(&mut random) as usize) % (current.len() + 1);
        while start > 0 && !current.is_char_boundary(start) {
            start -= 1;
        }
        let delete_scalars = (next_random(&mut random) % 5) as usize;
        let mut end = start;
        for scalar in current[start..].chars().take(delete_scalars) {
            end += scalar.len_utf8();
        }
        let mut inserted = insertions[next_random(&mut random) as usize % insertions.len()];
        if start == end && inserted.is_empty() {
            inserted = "x";
        }
        let version = (ordinal + 1).to_string();
        let edit = store
            .edit_bytes(
                ByteTextSpan::new(start as u32, (end - start) as u32),
                inserted,
                DocumentVersion::new(version.clone()),
            )
            .map_err(|error| format!("edit {ordinal} failed: {error}"))?;
        let snapshot = store.snapshot();
        let update = registry
            .update_incrementally(
                &changed,
                changed_address.clone(),
                Arc::clone(&snapshot),
                edit.byte_change(),
                IncrementalDocumentOptions {
                    parse: parse_options.clone(),
                    incremental: IncrementalParseOptions::default(),
                },
                &domain,
            )
            .map_err(|error| format!("incremental update {ordinal} failed: {error}"))?;
        let fresh =
            create_language_service_source_file("/large-edit.ts", snapshot, parse_options.clone());
        if !source_files_structurally_equal(update.lease.document().source(), &fresh) {
            return Err(format!(
                "fresh/incremental mismatch at edit {ordinal}: start={start} end={end} inserted={inserted:?}"
            ));
        }
        if !update.parse_stats.incremental || update.parse_stats.full_parse_fallback {
            return Err(format!("edit {ordinal} unexpectedly used a full parse"));
        }

        let program = ProgramSnapshot::new(
            vec![
                Arc::clone(update.lease.document()),
                Arc::clone(stable.document()),
            ],
            2,
        )
        .map_err(|error| format!("Program snapshot {ordinal} failed: {error}"))?;
        if !Arc::ptr_eq(program.document(1), &stable_document) {
            return Err(format!(
                "stable document identity changed at edit {ordinal}"
            ));
        }
        drop(program);

        if ordinal % 17 == 0 {
            let stable_again = registry
                .acquire(stable_address.clone(), Arc::clone(&stable_snapshot), || {
                    panic!("stable registry entry must not parse or bind again")
                })
                .map_err(|error| error.to_string())?;
            if !Arc::ptr_eq(stable_again.document(), &stable_document) {
                return Err(format!(
                    "stable registry acquire changed identity at {ordinal}"
                ));
            }
            registry
                .release(stable_again)
                .map_err(|error| error.to_string())?;
        }

        let node_lease = update
            .lease
            .document()
            .source()
            .node_identity_lease()
            .expect("registry source has a node lease")
            .range();
        let array_lease = update
            .lease
            .document()
            .source()
            .array_identity_lease()
            .expect("registry source has an array lease")
            .range();
        let trace = EditTrace {
            ordinal,
            version,
            start_byte: start as u32,
            delete_bytes: (end - start) as u32,
            inserted: inserted.to_owned(),
            incremental: update.parse_stats.incremental,
            reused_list_elements: update.parse_stats.reused_list_elements,
            reused_nodes: update.parse_stats.reused_nodes,
            reused_node_arrays: update.parse_stats.reused_node_arrays,
            freshly_parsed_nodes: update.parse_stats.freshly_parsed_nodes,
            node_range: [node_lease.start(), node_lease.end()],
            array_range: [array_lease.start(), array_lease.end()],
            parse_diagnostics: diagnostic_pins(&fresh.parse_diagnostics),
            jsdoc_diagnostics: diagnostic_pins(&fresh.js_doc_diagnostics),
        };
        state.minimum_reused_nodes = state
            .minimum_reused_nodes
            .min(update.parse_stats.reused_nodes);
        state.maximum_freshly_parsed_nodes = state
            .maximum_freshly_parsed_nodes
            .max(update.parse_stats.freshly_parsed_nodes);
        state.recent.push_back(trace);
        if state.recent.len() > TRACE_LIMIT {
            state.recent.pop_front();
        }

        let old = std::mem::replace(&mut changed, update.lease);
        registry.release(old).map_err(|error| error.to_string())?;
        if registry.active_entry_count() != 2 || registry.active_reference_count() != 2 {
            return Err(format!("registry bound drifted at edit {ordinal}"));
        }
        verify_identity_bound(&domain, ordinal)?;
        state.completed = ordinal + 1;
    }

    registry
        .release(changed)
        .map_err(|error| error.to_string())?;
    registry
        .release(stable)
        .map_err(|error| error.to_string())?;
    drop(stable_document);
    state.final_registry_entries = registry.active_entry_count();
    let final_stats = domain.stats().map_err(|error| error.to_string())?;
    state.final_active_ranges = IdentitySpace::ALL
        .into_iter()
        .map(|space| final_stats.space(space).active_ranges)
        .sum();
    if state.final_registry_entries != 0 || state.final_active_ranges != 0 {
        return Err(format!(
            "final reclamation failed: registry={} identity-ranges={}",
            state.final_registry_entries, state.final_active_ranges
        ));
    }
    Ok(())
}

fn bound_document(
    path: &str,
    snapshot: Arc<TextSnapshot>,
    compiler_options: &CompilerOptions,
    parse_options: &ParseOptions,
    domain: &IdentityDomain,
) -> Result<Arc<BoundDocument>, Box<dyn Error>> {
    let source = Arc::new(parse_source_file_from_snapshot_in_identity_domain(
        path.to_owned(),
        snapshot,
        parse_options.clone(),
        None,
        domain,
    )?);
    let worker = BinderWorker::bind_in_identity_domain(&source, compiler_options, domain)?;
    let data = worker.into_bind_data();
    Ok(Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(source)),
        data,
    )))
}

fn verify_identity_bound(domain: &IdentityDomain, ordinal: usize) -> Result<(), String> {
    for space in IdentitySpace::ALL {
        let ranges = domain
            .active_ranges(space)
            .map_err(|error| error.to_string())?;
        if ranges.len() > 2 {
            return Err(format!(
                "{space:?} retained {} ranges after edit {ordinal}",
                ranges.len()
            ));
        }
        if ranges.windows(2).any(|pair| pair[0].overlaps(pair[1])) {
            return Err(format!("{space:?} ranges overlap after edit {ordinal}"));
        }
    }
    Ok(())
}

fn diagnostic_pins(diagnostics: &[tsc_diagnostics::Diagnostic]) -> Vec<DiagnosticPin> {
    diagnostics
        .iter()
        .take(DIAGNOSTIC_LIMIT)
        .map(|diagnostic| DiagnosticPin {
            code: diagnostic.code(),
            start: diagnostic.start,
            length: diagnostic.length,
        })
        .collect()
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn parse_arguments(mut args: impl Iterator<Item = String>) -> Result<Arguments, Box<dyn Error>> {
    let mut settings = Settings {
        seed: DEFAULT_SEED,
        edits: DEFAULT_EDITS,
        max_rss_bytes: DEFAULT_MAX_RSS_BYTES,
    };
    let mut fixture = None;
    let mut report = PathBuf::from("target/l1/incremental-stress.json");
    let mut internal_stress_child = false;
    while let Some(argument) = args.next() {
        let value = |name: &str, args: &mut dyn Iterator<Item = String>| {
            args.next()
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match argument.as_str() {
            "--internal-stress-child" if !internal_stress_child => {
                internal_stress_child = true;
            }
            "--internal-stress-child" => {
                return Err("--internal-stress-child may only be specified once".into())
            }
            "--fixture" => fixture = Some(PathBuf::from(value("--fixture", &mut args)?)),
            "--seed" => {
                let raw = value("--seed", &mut args)?;
                settings.seed = raw
                    .strip_prefix("0x")
                    .map(|hex| u64::from_str_radix(hex, 16))
                    .unwrap_or_else(|| raw.parse())?;
            }
            "--edits" => settings.edits = value("--edits", &mut args)?.parse()?,
            "--max-rss-bytes" => {
                settings.max_rss_bytes = value("--max-rss-bytes", &mut args)?.parse()?;
            }
            "--report" => report = PathBuf::from(value("--report", &mut args)?),
            other => return Err(format!("unknown l1 incremental-stress argument {other:?}").into()),
        }
    }
    if settings.edits == 0 || settings.edits > 100_000 {
        return Err("--edits must be between 1 and 100,000".into());
    }
    if settings.max_rss_bytes < 64 * 1024 * 1024 {
        return Err("--max-rss-bytes must be at least 64 MiB".into());
    }
    Ok(Arguments {
        settings,
        fixture: fixture.ok_or("l1 incremental-stress requires --fixture <large-edit.ts>")?,
        report_path: report,
        internal_stress_child,
    })
}

struct TemporaryReport(PathBuf);

impl Drop for TemporaryReport {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn internal_report_path(report_path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let parent = report_path.parent().unwrap_or_else(|| Path::new("."));
    let name = report_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("L1 stress report file name is not UTF-8")?;
    Ok(parent.join(format!(
        ".{name}.internal-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    )))
}

fn write_report(path: &Path, report: &StressReport) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec_pretty(report)?;
    bytes.push(b'\n');
    fs::write(path, bytes)?;
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(unsafe_code)]
fn wait_for_stress_child(child: Child) -> Result<ChildWait, Box<dyn Error>> {
    use std::mem::MaybeUninit;
    use std::os::unix::process::ExitStatusExt;

    let pid = libc::pid_t::try_from(child.id())?;
    let mut raw_status = 0;
    let mut usage = MaybeUninit::<libc::rusage>::uninit();
    loop {
        // SAFETY: `raw_status` and `usage` point to writable storage for the
        // duration of wait4, and `pid` names the live child owned here. The
        // rusage value is read only after wait4 reports successful reaping.
        let waited = unsafe { libc::wait4(pid, &mut raw_status, 0, usage.as_mut_ptr()) };
        if waited == pid {
            break;
        }
        if waited == -1 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        return Err(format!("wait4 reaped unexpected process {waited} instead of {pid}").into());
    }
    // SAFETY: the successful wait4 call initialized the complete rusage.
    let usage = unsafe { usage.assume_init() };
    drop(child);
    Ok(ChildWait {
        status: ExitStatus::from_raw(raw_status),
        peak_rss_bytes: ru_maxrss_bytes(usage.ru_maxrss),
        mechanism: child_peak_rss_mechanism(),
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn wait_for_stress_child(mut child: Child) -> Result<ChildWait, Box<dyn Error>> {
    Ok(ChildWait {
        status: child.wait()?,
        peak_rss_bytes: None,
        mechanism: "child wait without rusage",
    })
}

#[cfg(target_os = "macos")]
fn ru_maxrss_bytes(max_rss: libc::c_long) -> Option<u64> {
    u64::try_from(max_rss).ok()
}

#[cfg(target_os = "linux")]
fn ru_maxrss_bytes(max_rss: libc::c_long) -> Option<u64> {
    u64::try_from(max_rss).ok()?.checked_mul(1024)
}

#[cfg(target_os = "macos")]
fn child_peak_rss_mechanism() -> &'static str {
    "wait4 ru_maxrss (darwin bytes)"
}

#[cfg(target_os = "linux")]
fn child_peak_rss_mechanism() -> &'static str {
    "wait4 ru_maxrss (linux KiB converted to bytes)"
}

fn self_peak_rss_mechanism() -> &'static str {
    if cfg!(target_os = "linux") {
        "procfs VmHWM (bytes)"
    } else if cfg!(target_os = "macos") {
        "getrusage RUSAGE_SELF ru_maxrss (darwin bytes)"
    } else {
        "unavailable"
    }
}

fn peak_rss_bytes() -> Option<u64> {
    if cfg!(target_os = "linux") {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        let kilobytes = status
            .lines()
            .find_map(|line| line.strip_prefix("VmHWM:"))?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?;
        return kilobytes.checked_mul(1024);
    }

    #[cfg(target_os = "macos")]
    {
        darwin_peak_rss_bytes()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn darwin_peak_rss_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `usage` is valid writable storage and is read only when
    // getrusage reports success.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: successful getrusage initialized the complete value.
    let usage = unsafe { usage.assume_init() };
    u64::try_from(usage.ru_maxrss).ok()
}

#[cfg(test)]
#[path = "../tests/unit/l1_incremental_stress/tests.rs"]
mod tests;
