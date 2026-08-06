use std::collections::VecDeque;
use std::error::Error;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::Serialize;
use tsc_diagnostics::{DocumentVersion, TextSnapshot};
use tsc_types::{
    CompilerOptions, IdentityDomain, IdentityDomainStats, IdentityLease, IdentitySpace,
};

const DEFAULT_SEED: u64 = 0x1d3a_71a5_e5ca_5e02;
const DEFAULT_ITERATIONS: usize = 10_000;
const DEFAULT_MAX_ACTIVE: usize = 64;
const DEFAULT_MAX_RSS_BYTES: u64 = 512 * 1024 * 1024;
const TRACE_LIMIT: usize = 128;

#[derive(Clone, Copy, Debug)]
struct Settings {
    seed: u64,
    iterations: usize,
    max_active: usize,
    max_rss_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
struct RangeReport {
    space: &'static str,
    start: u32,
    end: u32,
}

#[derive(Clone, Debug, Serialize)]
struct OperationReport {
    ordinal: usize,
    document: usize,
    project: usize,
    version: usize,
    script_kind: &'static str,
    option_variant: usize,
    ranges: Vec<RangeReport>,
}

#[derive(Debug, Serialize)]
struct IdentityStressReport {
    schema: u32,
    kind: &'static str,
    status: &'static str,
    seed: String,
    requested_iterations: usize,
    completed_iterations: usize,
    max_active_documents: usize,
    projects: usize,
    script_kinds: Vec<&'static str>,
    option_variants: usize,
    maximum_bumps: Vec<RangeReport>,
    final_active_ranges: usize,
    elapsed_millis: u128,
    peak_rss_bytes: Option<u64>,
    rss_ceiling_bytes: u64,
    trace_limit: usize,
    recent_operations: Vec<OperationReport>,
    error: Option<String>,
}

#[derive(Debug)]
struct RetainedDocument {
    _leases: Vec<IdentityLease>,
}

#[derive(Debug)]
struct StressState {
    completed: usize,
    maximum_bumps: [u32; 4],
    recent: VecDeque<OperationReport>,
    final_stats: IdentityDomainStats,
}

pub(crate) fn run(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let (settings, report_path) = parse_arguments(args)?;
    let started = Instant::now();
    let execution = catch_unwind(AssertUnwindSafe(|| execute(settings)));
    let (state, error) = match execution {
        Ok(Ok(state)) => (Some(state), None),
        Ok(Err(error)) => (None, Some(error)),
        Err(payload) => {
            let detail = payload
                .downcast_ref::<&str>()
                .map(|value| (*value).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "identity stress panicked with a non-string payload".to_owned());
            (None, Some(detail))
        }
    };
    let peak_rss_bytes = peak_rss_bytes();
    let rss_error = peak_rss_bytes
        .filter(|bytes| *bytes > settings.max_rss_bytes)
        .map(|bytes| {
            format!(
                "peak RSS {bytes} exceeds reviewed ceiling {}",
                settings.max_rss_bytes
            )
        });
    let error = error.or(rss_error);
    let status = if error.is_none() && state.is_some() {
        "passed"
    } else {
        "failed"
    };
    let maximum_bumps = state
        .as_ref()
        .map(|state| state.maximum_bumps)
        .unwrap_or_default();
    let report = IdentityStressReport {
        schema: 1,
        kind: "l0-identity-stress",
        status,
        seed: format!("0x{:016x}", settings.seed),
        requested_iterations: settings.iterations,
        completed_iterations: state.as_ref().map_or(0, |state| state.completed),
        max_active_documents: settings.max_active,
        projects: 4,
        script_kinds: vec!["ts", "tsx", "js", "json"],
        option_variants: 8,
        maximum_bumps: IdentitySpace::ALL
            .into_iter()
            .enumerate()
            .map(|(index, space)| RangeReport {
                space: space_name(space),
                start: space_start(space),
                end: maximum_bumps[index],
            })
            .collect(),
        final_active_ranges: state.as_ref().map_or(0, |state| {
            state
                .final_stats
                .spaces
                .iter()
                .map(|stats| stats.active_ranges)
                .sum()
        }),
        elapsed_millis: started.elapsed().as_millis(),
        peak_rss_bytes,
        rss_ceiling_bytes: settings.max_rss_bytes,
        trace_limit: TRACE_LIMIT,
        recent_operations: state
            .map(|state| state.recent.into_iter().collect())
            .unwrap_or_default(),
        error,
    };
    write_report(&report_path, &report)?;
    println!(
        "L0 identity stress: status={} iterations={}/{} active-bound={} peak-rss={:?}/{} report={}",
        report.status,
        report.completed_iterations,
        report.requested_iterations,
        report.max_active_documents,
        report.peak_rss_bytes,
        report.rss_ceiling_bytes,
        report_path.display()
    );
    if report.status != "passed" {
        return Err(format!(
            "L0 identity stress failed: {}",
            report.error.as_deref().unwrap_or("unknown failure")
        )
        .into());
    }
    Ok(())
}

fn execute(settings: Settings) -> Result<StressState, String> {
    let domain = IdentityDomain::reclaiming();
    let mut retained = VecDeque::<RetainedDocument>::new();
    let mut recent = VecDeque::new();
    let mut maximum_bumps = [0; 4];
    let mut random = XorShift64(settings.seed);

    for ordinal in 0..settings.iterations {
        let document = random.index(settings.max_active.saturating_mul(2).max(1));
        let project = random.index(4);
        let version = ordinal / settings.max_active.max(1);
        let kind = random.index(4);
        let option_variant = random.index(8);
        let options = compiler_options(option_variant);
        let (file_name, text, script_kind) = source_input(kind, project, document, version);
        let snapshot = TextSnapshot::new(text, DocumentVersion::new(version.to_string()));
        let source = if script_kind == "json" {
            tsc_syntax::parse_json_text_from_snapshot_in_identity_domain(
                file_name, snapshot, &domain,
            )
        } else {
            tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
                file_name,
                snapshot,
                tsc_syntax::ParseOptions {
                    script_target: options.emit_script_target(),
                    language_variant: if matches!(script_kind, "tsx" | "js") {
                        tsc_syntax::LanguageVariant::Jsx
                    } else {
                        tsc_syntax::LanguageVariant::Standard
                    },
                    javascript_file: script_kind == "js",
                    ..tsc_syntax::ParseOptions::default()
                },
                None,
                &domain,
            )
        }
        .map_err(|error| format!("parse identity publication {ordinal} failed: {error}"))?;
        let binder = tsc_binder::Binder::bind_in_identity_domain(&source, &options, &domain)
            .map_err(|error| format!("bind identity publication {ordinal} failed: {error}"))?;
        if !source.identity_owned_by(&domain) || !binder.identity_owned_by(&domain) {
            return Err(format!(
                "publication {ordinal} escaped its requested identity domain"
            ));
        }

        let leases = vec![
            source
                .node_identity_lease()
                .expect("published source node lease")
                .clone(),
            source
                .array_identity_lease()
                .expect("published source array lease")
                .clone(),
            binder
                .symbol_identity_lease()
                .expect("published bind symbol lease")
                .clone(),
            binder
                .private_name_serial_lease()
                .expect("published bind serial lease")
                .clone(),
        ];
        let ranges = leases
            .iter()
            .map(|lease| RangeReport {
                space: space_name(lease.space()),
                start: lease.range().start(),
                end: lease.range().end(),
            })
            .collect();
        retained.push_back(RetainedDocument { _leases: leases });
        while retained.len() > settings.max_active {
            retained.pop_front();
        }

        verify_non_overlap(&domain, ordinal)?;
        let stats = domain
            .stats()
            .map_err(|error| format!("identity stats {ordinal} failed: {error}"))?;
        for (index, space) in IdentitySpace::ALL.into_iter().enumerate() {
            maximum_bumps[index] = maximum_bumps[index].max(stats.space(space).bump);
            if stats.space(space).bump > 1_048_576 {
                return Err(format!(
                    "{space:?} high-water mark exceeded the reviewed 1,048,576 bound at iteration {ordinal}"
                ));
            }
        }
        recent.push_back(OperationReport {
            ordinal,
            document,
            project,
            version,
            script_kind,
            option_variant,
            ranges,
        });
        if recent.len() > TRACE_LIMIT {
            recent.pop_front();
        }
    }

    retained.clear();
    let final_stats = domain
        .stats()
        .map_err(|error| format!("final identity stats failed: {error}"))?;
    for space in IdentitySpace::ALL {
        let stats = final_stats.space(space);
        if stats.active_ranges != 0 || stats.active_values != 0 || stats.provisional {
            return Err(format!(
                "{space:?} retained identity ownership after final close: {stats:?}"
            ));
        }
        if stats.bump != space_start(space) {
            return Err(format!(
                "{space:?} did not reclaim its tail after final close: {}",
                stats.bump
            ));
        }
    }
    Ok(StressState {
        completed: settings.iterations,
        maximum_bumps,
        recent,
        final_stats,
    })
}

fn verify_non_overlap(domain: &IdentityDomain, ordinal: usize) -> Result<(), String> {
    for space in IdentitySpace::ALL {
        let ranges = domain
            .active_ranges(space)
            .map_err(|error| format!("active-range query {ordinal} failed: {error}"))?;
        for pair in ranges.windows(2) {
            if pair[0].overlaps(pair[1]) {
                return Err(format!(
                    "{space:?} ranges overlap at iteration {ordinal}: {:?} and {:?}",
                    pair[0], pair[1]
                ));
            }
        }
    }
    Ok(())
}

fn compiler_options(variant: usize) -> CompilerOptions {
    CompilerOptions {
        target: Some(match variant % 4 {
            0 => 1,
            1 => 2,
            2 => 7,
            _ => 99,
        }),
        always_strict: Some(variant & 1 != 0),
        no_fallthrough_cases_in_switch: Some(variant & 2 != 0),
        allow_js: variant & 4 != 0,
        ..CompilerOptions::default()
    }
}

fn source_input(
    kind: usize,
    project: usize,
    document: usize,
    version: usize,
) -> (String, String, &'static str) {
    match kind {
        0 => (
            format!("/project-{project}/source-{document}.ts"),
            format!(
                "export class C{document} {{ #value = {version}; read(v: number) {{ return this.#value + v; }} }}"
            ),
            "ts",
        ),
        1 => (
            format!("/project-{project}/view-{document}.tsx"),
            format!(
                "export const View{document} = (p: {{ value: number }}) => <div>{{p.value + {version}}}</div>;"
            ),
            "tsx",
        ),
        2 => (
            format!("/project-{project}/script-{document}.js"),
            format!(
                "// @ts-check\nexport class J{document} {{ #value = {version}; read(v) {{ return this.#value + v; }} }}"
            ),
            "js",
        ),
        _ => (
            format!("/project-{project}/data-{document}.json"),
            format!("{{\"document\":{document},\"version\":{version}}}"),
            "json",
        ),
    }
}

fn parse_arguments(
    mut args: impl Iterator<Item = String>,
) -> Result<(Settings, PathBuf), Box<dyn Error>> {
    let mut settings = Settings {
        seed: DEFAULT_SEED,
        iterations: DEFAULT_ITERATIONS,
        max_active: DEFAULT_MAX_ACTIVE,
        max_rss_bytes: DEFAULT_MAX_RSS_BYTES,
    };
    let mut report = PathBuf::from("target/l0/identity-stress.json");
    while let Some(argument) = args.next() {
        let value = |name: &str, args: &mut dyn Iterator<Item = String>| {
            args.next()
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match argument.as_str() {
            "--seed" => {
                let raw = value("--seed", &mut args)?;
                settings.seed = raw
                    .strip_prefix("0x")
                    .map(|hex| u64::from_str_radix(hex, 16))
                    .unwrap_or_else(|| raw.parse())?;
            }
            "--iterations" => settings.iterations = value("--iterations", &mut args)?.parse()?,
            "--max-active" => settings.max_active = value("--max-active", &mut args)?.parse()?,
            "--max-rss-bytes" => {
                settings.max_rss_bytes = value("--max-rss-bytes", &mut args)?.parse()?
            }
            "--report" => report = PathBuf::from(value("--report", &mut args)?),
            other => return Err(format!("unknown l0 identity-stress argument {other:?}").into()),
        }
    }
    if settings.iterations == 0 || settings.iterations > 1_000_000 {
        return Err("--iterations must be between 1 and 1,000,000".into());
    }
    if settings.max_active == 0 || settings.max_active > 4096 {
        return Err("--max-active must be between 1 and 4096".into());
    }
    if settings.max_rss_bytes < 64 * 1024 * 1024 {
        return Err("--max-rss-bytes must be at least 64 MiB".into());
    }
    Ok((settings, report))
}

fn write_report(path: &Path, report: &IdentityStressReport) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec_pretty(report)?;
    bytes.push(b'\n');
    fs::write(path, bytes)?;
    Ok(())
}

fn space_name(space: IdentitySpace) -> &'static str {
    match space {
        IdentitySpace::Node => "node",
        IdentitySpace::NodeArray => "node-array",
        IdentitySpace::Symbol => "symbol",
        IdentitySpace::PrivateNameSerial => "private-name-serial",
    }
}

fn space_start(space: IdentitySpace) -> u32 {
    match space {
        IdentitySpace::PrivateNameSerial => 1,
        _ => 0,
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
    let process_selector = ['-', 'p'].into_iter().collect::<String>();
    let output = Command::new("ps")
        .args(["-o", "rss="])
        .arg(process_selector)
        .arg(std::process::id().to_string())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
}

struct XorShift64(u64);

impl XorShift64 {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn index(&mut self, upper: usize) -> usize {
        debug_assert!(upper > 0);
        self.next() as usize % upper
    }
}
