#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tsrs2_checker::{CompilerOptions, InputFile};
use tsrs2_diags::DiagnosticList;

mod relpin;
mod symbol_audit;

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next();

    match command.as_deref() {
        None | Some("scaffold-smoke") => scaffold_smoke(),
        Some("expand") => run_or_exit(expand_fixture(args)),
        Some("tokens") => run_or_exit(tokens(args)),
        Some("token-diff") => run_or_exit(token_diff(args)),
        Some("ast-dump") => run_or_exit(ast_dump(args)),
        Some("ast-diff") => run_or_exit(ast_diff(args)),
        Some("symbol-diff") => run_or_exit(symbol_diff(args)),
        Some("lib-gate") => run_or_exit(lib_gate(args)),
        Some("bind-corpus") => run_or_exit(bind_corpus(args)),
        Some("parse-diags") => run_or_exit(parse_diags(args)),
        Some("oracle-smoke") => run_or_exit(oracle_smoke(args)),
        Some("oracle-refresh") => run_or_exit(oracle_refresh(args)),
        Some("goldens-diff") => run_or_exit(goldens_diff(args)),
        Some("conformance") => run_or_exit(conformance(args)),
        Some("invariants") => run_or_exit(invariants(args)),
        Some("m8") => match args.next().as_deref() {
            Some("readiness") => run_or_exit(m8_readiness(args)),
            Some(other) => {
                eprintln!("unknown m8 command: {other}");
                std::process::exit(2);
            }
            None => {
                eprintln!("missing m8 command (readiness)");
                std::process::exit(2);
            }
        },
        Some("relpin") => match args.next().as_deref() {
            Some("gen") => run_or_exit(relpin::gen(args)),
            Some("run") => run_or_exit(relpin::run(args)),
            Some(other) => {
                eprintln!("unknown relpin command: {other}");
                std::process::exit(2);
            }
            None => {
                eprintln!("missing relpin command (gen|run)");
                std::process::exit(2);
            }
        },
        Some("ratchet") => match args.next().as_deref() {
            Some("check") => run_or_exit(ratchet_check(args)),
            Some("update") => run_or_exit(ratchet_update(args)),
            Some(other) => {
                eprintln!("unknown ratchet command: {other}");
                std::process::exit(2);
            }
            None => {
                eprintln!("missing ratchet command (check|update)");
                std::process::exit(2);
            }
        },
        Some("scope") => match args.next().as_deref() {
            Some("audit") => run_or_exit(scope_audit(args)),
            Some(other) => {
                eprintln!("unknown scope command: {other}");
                std::process::exit(2);
            }
            None => {
                eprintln!("missing scope command (audit)");
                std::process::exit(2);
            }
        },
        Some("families") => match args.next().as_deref() {
            Some("check") => run_or_exit(families_check(args)),
            Some("report") => run_or_exit(families_report(args)),
            Some(other) => {
                eprintln!("unknown families command: {other}");
                std::process::exit(2);
            }
            None => {
                eprintln!("missing families command (check|report)");
                std::process::exit(2);
            }
        },
        Some("ledger") => match args.next().as_deref() {
            Some("check") => run_or_exit(ledger_check()),
            Some("write-backlog") => run_or_exit(ledger_write_backlog()),
            Some("coverage") => run_or_exit(ledger_coverage()),
            Some(other) => {
                eprintln!("unknown ledger command: {other}");
                std::process::exit(2);
            }
            None => {
                eprintln!("missing ledger command");
                std::process::exit(2);
            }
        },
        Some("ci") => run_or_exit(ci(args)),
        Some("escapes") => run_or_exit(escapes(args)),
        Some("codegen") => match args.next().as_deref() {
            Some("diags") => run_or_exit(codegen_diags(false)),
            Some("diags-check") => run_or_exit(codegen_diags(true)),
            Some("nodes") => run_or_exit(codegen_nodes(false)),
            Some("nodes-check") => run_or_exit(codegen_nodes(true)),
            Some("enums") => run_or_exit(codegen_enums(false)),
            Some("enums-check") => run_or_exit(codegen_enums(true)),
            Some("scanner") => run_or_exit(codegen_scanner(false)),
            Some("scanner-check") => run_or_exit(codegen_scanner(true)),
            Some("band-inventory") => run_or_exit(codegen_band_inventory(args)),
            Some(other) => {
                eprintln!("unknown codegen target: {other}");
                std::process::exit(2);
            }
            None => {
                eprintln!("missing codegen target");
                std::process::exit(2);
            }
        },
        Some(other) => {
            eprintln!("unknown xtask command: {other}");
            std::process::exit(2);
        }
    }
}

fn codegen_band_inventory(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let mut band = "all".to_owned();
    let mut check = false;
    let mut by_function = false;
    let mut out = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--band" => {
                band = args.next().ok_or("missing value after --band")?;
                if !matches!(band.as_str(), "all" | "2xxx") {
                    return Err(format!("unknown inventory band: {band}").into());
                }
            }
            "--by-function" => by_function = true,
            "--check" => check = true,
            "--out" => {
                out = Some(PathBuf::from(
                    args.next().ok_or("missing value after --out")?,
                ))
            }
            _ => return Err(format!("unexpected band-inventory argument: {arg}").into()),
        }
    }
    if !by_function {
        return Err("band-inventory requires --by-function; code-only inventory is not an M8 completeness proof".into());
    }
    let output = Command::new("node")
        .arg(workspace.join("crates/oracle/emitter-inventory.mjs"))
        .arg(workspace.join("vendor/typescript-6.0.3/lib/_tsc.js"))
        .arg(&band)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "emitter inventory worker failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let _: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let target = out.unwrap_or_else(|| {
        if band == "all" {
            workspace.join("m8-emitter-inventory.json")
        } else {
            workspace.join("target/codegen/2xxx-emitter-inventory.json")
        }
    });
    if check {
        let recorded = fs::read(&target).map_err(|err| {
            format!(
                "missing generated emitter inventory {}: {err}; run without --check",
                target.display()
            )
        })?;
        if recorded != output.stdout {
            return Err(format!(
                "stale emitter inventory {}; regenerate and review the diff",
                target.display()
            )
            .into());
        }
        println!("emitter inventory fresh: band={band} {}", target.display());
    } else {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, &output.stdout)?;
        println!(
            "emitter inventory written: band={band} {}",
            target.display()
        );
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct M8EmitterInventory {
    schema: u32,
    source_sha256: String,
    band: String,
    summary: M8EmitterInventorySummary,
    functions: Vec<M8EmitterFunction>,
}

#[derive(Debug, Deserialize)]
struct M8EmitterInventorySummary {
    emitter_functions: usize,
    diagnostic_references: usize,
    closure_functions: usize,
}

#[derive(Debug, Deserialize)]
struct M8EmitterFunction {
    id: String,
    name: String,
    direct_emitter: bool,
}

#[derive(Debug, Deserialize)]
struct M8EmitterDispositions {
    schema: u32,
    status: String,
    inventory_sha256: String,
    #[serde(default)]
    entries: Vec<M8EmitterDisposition>,
}

#[derive(Debug, Deserialize)]
struct M8EmitterDisposition {
    function: String,
    disposition: String,
    evidence: String,
}

#[derive(Debug, Deserialize)]
struct M8Evidence {
    schema: u32,
    runtime_coverage: M8RuntimeCoverageEvidence,
    fuzzer: M8FuzzerEvidence,
    performance: M8PerformanceEvidence,
}

#[derive(Debug, Deserialize)]
struct M8RuntimeCoverageEvidence {
    status: String,
    inventory_sha256: String,
    #[serde(default)]
    executed_emitters: Vec<String>,
    #[serde(default)]
    zero_hit_emitters: Vec<M8ZeroHitEmitter>,
    artifact: Option<String>,
}

#[derive(Debug, Deserialize)]
struct M8ZeroHitEmitter {
    function: String,
    evidence: String,
}

#[derive(Debug, Deserialize)]
struct M8FuzzerEvidence {
    status: String,
    ci_command: Option<String>,
    generated_cases: usize,
    oracle_comparisons: usize,
    reducer_smoke: bool,
    signature_dedupe: bool,
    artifact: Option<String>,
}

#[derive(Debug, Deserialize)]
struct M8PerformanceEvidence {
    status: String,
    wall_seconds: Option<f64>,
    max_rss_bytes: Option<u64>,
    ceiling_wall_seconds: Option<f64>,
    ceiling_rss_bytes: Option<u64>,
    artifact: Option<String>,
}

#[derive(Debug, Serialize)]
struct M8ReadinessGate {
    name: String,
    ready: bool,
    detail: String,
}

#[derive(Debug, Serialize)]
struct M8ReadinessReport {
    schema: u32,
    ready: bool,
    gates: Vec<M8ReadinessGate>,
}

fn m8_readiness(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut require_ready = false;
    for arg in args {
        match arg.as_str() {
            "--require-ready" => require_ready = true,
            _ => return Err(format!("unexpected m8 readiness argument: {arg}").into()),
        }
    }

    let workspace = find_tsrs2_root()?;
    let out_dir = workspace.join("target/m8");
    fs::create_dir_all(&out_dir)?;
    ledger_check()?;
    codegen_band_inventory(
        ["--by-function", "--band", "all", "--check"]
            .into_iter()
            .map(str::to_owned),
    )?;
    let conformance = tsrs2_conformance::run_conformance(&tsrs2_conformance::ConformanceOptions {
        workspace: workspace.clone(),
        limit: None,
        files: Vec::new(),
        out_json: out_dir.join("conformance.json"),
        band: tsrs2_conformance::DiagnosticBand::All,
    })?;

    let inventory_path = workspace.join("m8-emitter-inventory.json");
    let inventory: M8EmitterInventory = read_json(&inventory_path)?;
    if inventory.schema != 1 || inventory.band != "all" {
        return Err("m8-emitter-inventory.json must be schema 1, band all".into());
    }
    let bundle_hash = sha256_file(&workspace.join("vendor/typescript-6.0.3/lib/_tsc.js"))?;
    let inventory_fresh = inventory.source_sha256 == bundle_hash;
    let inventory_hash = sha256_file(&inventory_path)?;

    let dispositions: M8EmitterDispositions =
        read_json(&workspace.join("m8-emitter-dispositions.json"))?;
    if dispositions.schema != 1 {
        return Err("m8-emitter-dispositions.json must be schema 1".into());
    }
    let mut explicit = BTreeMap::new();
    for entry in &dispositions.entries {
        if !matches!(entry.disposition.as_str(), "deferred" | "not-applicable") {
            return Err(format!(
                "invalid M8 emitter disposition for {}: {}",
                entry.function, entry.disposition
            )
            .into());
        }
        if entry.evidence.trim().is_empty() {
            return Err(format!(
                "M8 emitter disposition for {} has no evidence",
                entry.function
            )
            .into());
        }
        if explicit.insert(entry.function.as_str(), entry).is_some() {
            return Err(format!("duplicate M8 emitter disposition for {}", entry.function).into());
        }
    }
    let inventory_ids = inventory
        .functions
        .iter()
        .map(|function| function.id.as_str())
        .collect::<BTreeSet<_>>();
    let extra_dispositions = explicit
        .keys()
        .filter(|function| !inventory_ids.contains(**function))
        .count();
    let ledger_entries = collect_ledger_entries(&workspace)?;
    let ported_names = ledger_entries
        .iter()
        .map(|entry| entry.port_name.as_str())
        .collect::<BTreeSet<_>>();
    let unaccounted_closure = inventory
        .functions
        .iter()
        .filter(|function| {
            !ported_names.contains(function.name.as_str())
                && !explicit.contains_key(function.id.as_str())
        })
        .count();
    let emitter_closure_ready = dispositions.status == "frozen"
        && dispositions.inventory_sha256 == inventory_hash
        && inventory_fresh
        && unaccounted_closure == 0
        && extra_dispositions == 0;

    let evidence: M8Evidence = read_json(&workspace.join("m8-evidence.json"))?;
    if evidence.schema != 1 {
        return Err("m8-evidence.json must be schema 1".into());
    }
    let runtime = &evidence.runtime_coverage;
    let direct_emitter_ids = inventory
        .functions
        .iter()
        .filter(|function| function.direct_emitter)
        .map(|function| function.id.as_str())
        .collect::<BTreeSet<_>>();
    let executed_emitters = runtime
        .executed_emitters
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut zero_hit_emitters = BTreeSet::new();
    let mut zero_hit_invalid_evidence = 0usize;
    for emitter in &runtime.zero_hit_emitters {
        if emitter.evidence.trim().is_empty()
            || !zero_hit_emitters.insert(emitter.function.as_str())
        {
            zero_hit_invalid_evidence += 1;
        }
    }
    let runtime_duplicates = runtime.executed_emitters.len() - executed_emitters.len()
        + runtime.zero_hit_emitters.len()
        - zero_hit_emitters.len();
    let runtime_overlap = executed_emitters.intersection(&zero_hit_emitters).count();
    let runtime_accounted = executed_emitters
        .union(&zero_hit_emitters)
        .copied()
        .collect::<BTreeSet<_>>();
    let runtime_missing = direct_emitter_ids.difference(&runtime_accounted).count();
    let runtime_extra = runtime_accounted.difference(&direct_emitter_ids).count();
    let runtime_ready = runtime.status == "ready"
        && runtime.inventory_sha256 == inventory_hash
        && direct_emitter_ids.len() == inventory.summary.emitter_functions
        && !executed_emitters.is_empty()
        && runtime_missing == 0
        && runtime_extra == 0
        && runtime_duplicates == 0
        && runtime_overlap == 0
        && zero_hit_invalid_evidence == 0
        && artifact_exists(&workspace, runtime.artifact.as_deref());
    let fuzzer = &evidence.fuzzer;
    let fuzzer_ready = fuzzer.status == "ready"
        && fuzzer
            .ci_command
            .as_deref()
            .is_some_and(|command| !command.trim().is_empty())
        && fuzzer.generated_cases > 0
        && fuzzer.oracle_comparisons == fuzzer.generated_cases
        && fuzzer.reducer_smoke
        && fuzzer.signature_dedupe
        && artifact_exists(&workspace, fuzzer.artifact.as_deref());
    let performance = &evidence.performance;
    let performance_ready = match (
        performance.wall_seconds,
        performance.max_rss_bytes,
        performance.ceiling_wall_seconds,
        performance.ceiling_rss_bytes,
    ) {
        (Some(wall), Some(rss), Some(wall_ceiling), Some(rss_ceiling)) => {
            performance.status == "ready"
                && wall >= 0.0
                && wall <= wall_ceiling
                && wall_ceiling > 0.0
                && wall_ceiling <= 60.0
                && rss > 0
                && rss <= rss_ceiling
                && rss_ceiling > 0
                && artifact_exists(&workspace, performance.artifact.as_deref())
        }
        _ => false,
    };

    let t1_active = ratchet_section_has_exact_counts(&workspace.join("ratchet.toml"), "t1")?;
    let undispositioned = collect_undispositioned_checker_fns(&workspace)?.len();
    let mut gates = Vec::new();
    add_m8_gate(
        &mut gates,
        "m7-gate",
        conformance.t0_rate >= 0.63 && conformance.false_positive_diagnostics == 0 && t1_active,
        format!(
            "T0={:.4}% FP={} T1-ratchet-active={t1_active}",
            conformance.t0_rate * 100.0,
            conformance.false_positive_diagnostics
        ),
    );
    add_m8_gate(
        &mut gates,
        "shadow-tiers",
        conformance.oracle_diagnostics > 0
            && conformance.shadow_t1_matched > 0
            && conformance.shadow_t2_matched > 0
            && conformance.shadow_t3_matched > 0,
        format!(
            "T1={:.4}% T2={:.4}% T3={:.4}%",
            conformance.shadow_t1_rate * 100.0,
            conformance.shadow_t2_rate * 100.0,
            conformance.shadow_t3_rate * 100.0
        ),
    );
    add_m8_gate(
        &mut gates,
        "scope-frozen",
        conformance.scope_status == "frozen" && conformance.scope_resolved_t0_diagnostics == 0,
        format!(
            "status={} entries={} excluded={} resolved-t0={}",
            conformance.scope_status,
            conformance.scope_manifest_entries,
            conformance.scope_excluded_diagnostics,
            conformance.scope_resolved_t0_diagnostics
        ),
    );
    add_m8_gate(
        &mut gates,
        "rust-function-dispositions",
        undispositioned == 0,
        format!("undispositioned={undispositioned}"),
    );
    add_m8_gate(
        &mut gates,
        "emitter-inventory",
        inventory_fresh,
        format!(
            "fresh={inventory_fresh} emitters={} diagnostic-refs={} closure={}",
            inventory.summary.emitter_functions,
            inventory.summary.diagnostic_references,
            inventory.summary.closure_functions
        ),
    );
    add_m8_gate(
        &mut gates,
        "emitter-dependency-closure",
        emitter_closure_ready,
        format!(
            "status={} unaccounted={} extra={} inventory-match={}",
            dispositions.status,
            unaccounted_closure,
            extra_dispositions,
            dispositions.inventory_sha256 == inventory_hash
        ),
    );
    add_m8_gate(
        &mut gates,
        "runtime-coverage",
        runtime_ready,
        format!(
            "status={} accounted={}/{} executed={} zero-hit={} missing={} extra={} duplicate={} overlap={} invalid-evidence={}",
            runtime.status,
            runtime_accounted.intersection(&direct_emitter_ids).count(),
            inventory.summary.emitter_functions,
            executed_emitters.len(),
            zero_hit_emitters.len(),
            runtime_missing,
            runtime_extra,
            runtime_duplicates,
            runtime_overlap,
            zero_hit_invalid_evidence
        ),
    );
    add_m8_gate(
        &mut gates,
        "differential-fuzzer",
        fuzzer_ready,
        format!(
            "status={} generated={} compared={} reducer-smoke={} signature-dedupe={}",
            fuzzer.status,
            fuzzer.generated_cases,
            fuzzer.oracle_comparisons,
            fuzzer.reducer_smoke,
            fuzzer.signature_dedupe
        ),
    );
    add_m8_gate(
        &mut gates,
        "performance-baseline",
        performance_ready,
        format!(
            "status={} wall={:?}/{:?}s rss={:?}/{:?}",
            performance.status,
            performance.wall_seconds,
            performance.ceiling_wall_seconds,
            performance.max_rss_bytes,
            performance.ceiling_rss_bytes
        ),
    );

    let ready = gates.iter().all(|gate| gate.ready);
    let report = M8ReadinessReport {
        schema: 1,
        ready,
        gates,
    };
    fs::write(
        out_dir.join("readiness.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    for gate in &report.gates {
        println!(
            "{} {}: {}",
            if gate.ready { "[ok]" } else { "[ ]" },
            gate.name,
            gate.detail
        );
    }
    println!(
        "M8 readiness: {}/{} gates ready; report={}",
        report.gates.iter().filter(|gate| gate.ready).count(),
        report.gates.len(),
        out_dir.join("readiness.json").display()
    );
    if require_ready && !ready {
        return Err("M8 readiness gate is not complete".into());
    }
    Ok(())
}

fn add_m8_gate(gates: &mut Vec<M8ReadinessGate>, name: &str, ready: bool, detail: String) {
    gates.push(M8ReadinessGate {
        name: name.to_owned(),
        ready,
        detail,
    });
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn Error>> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    hasher.update(fs::read(path)?);
    Ok(format!("{:x}", hasher.finalize()))
}

fn artifact_exists(workspace: &Path, artifact: Option<&str>) -> bool {
    artifact.is_some_and(|artifact| {
        let path = PathBuf::from(artifact);
        if path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            return false;
        }
        let Ok(workspace) = workspace.canonicalize() else {
            return false;
        };
        let Ok(path) = workspace.join(path).canonicalize() else {
            return false;
        };
        path.starts_with(&workspace) && path.is_file()
    })
}

fn ratchet_section_has_exact_counts(path: &Path, section: &str) -> Result<bool, Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    let mut in_section = false;
    let mut matched = None;
    let mut total = None;
    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_section = &line[1..line.len() - 1] == section;
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "matched" => matched = Some(value.trim().parse::<u64>()?),
            "total" => total = Some(value.trim().parse::<u64>()?),
            _ => {}
        }
    }
    Ok(matches!((matched, total), (Some(matched), Some(total)) if matched > 0 && total > 0))
}

fn expand_fixture(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut fixture = None;
    let mut out_dir = None;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => {
                let value = args.next().ok_or("missing value after --out-dir")?;
                out_dir = Some(PathBuf::from(value));
            }
            _ if fixture.is_none() => fixture = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected expand argument: {arg}").into()),
        }
    }

    let fixture = fixture.ok_or("missing fixture path for expand")?;
    let out_dir = out_dir.ok_or("missing --out-dir for expand")?;
    let workspace = find_tsrs2_root()?;
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let programs = tsrs2_harness::expand_fixture_file(&fixture, &vendor_lib_dir)?;
    let paths = tsrs2_harness::write_program_jsons(&programs, &out_dir)?;

    for path in paths {
        println!("{}", path.display());
    }

    Ok(())
}

fn tokens(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let path = parse_single_path_arg("tokens", args)?;
    print!("{}", rust_token_dump(&path)?);
    Ok(())
}

struct TokenDiffArgs {
    corpus: bool,
    files: Vec<PathBuf>,
    limit: Option<usize>,
}

fn token_diff(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let args = parse_token_diff_args(args)?;
    let workspace = find_tsrs2_root()?;
    let mut files = if args.corpus {
        collect_fixture_paths(&workspace.join("ts-tests/tests/cases/conformance"))?
    } else {
        args.files
    };
    if files.is_empty() {
        return Err("token-diff requires --corpus, --files, or a file path".into());
    }
    files.sort();
    if let Some(limit) = args.limit {
        files.truncate(limit);
    }

    let mut oracle = TokenDumpOracle::spawn(&workspace)?;
    let mut differing = 0usize;
    for file in &files {
        let text = fs::read_to_string(file)?;
        let variant = language_variant_for_path(file);
        let rust = rust_token_dump_text(&text, variant);
        let oracle_dump = oracle.token_dump(file, &text, language_variant_arg(file))?;
        if rust != oracle_dump {
            differing += 1;
            if differing <= 10 {
                let (line, left, right) = first_diff(&rust, &oracle_dump);
                println!(
                    "diff {} line {}:\n  tsrs:   {}\n  oracle: {}",
                    file.display(),
                    line,
                    left.unwrap_or("<missing>"),
                    right.unwrap_or("<missing>")
                );
            }
        }
    }

    if differing > 0 {
        return Err(format!(
            "token diff failed: {differing}/{} files differ",
            files.len()
        )
        .into());
    }
    println!("token diff ok: files={}", files.len());
    Ok(())
}

fn parse_single_path_arg(
    command: &str,
    args: impl Iterator<Item = String>,
) -> Result<PathBuf, Box<dyn Error>> {
    let mut path = None;
    for arg in args {
        if path.is_none() {
            path = Some(PathBuf::from(arg));
        } else {
            return Err(format!("unexpected {command} argument: {arg}").into());
        }
    }
    path.ok_or_else(|| format!("missing file path for {command}").into())
}

fn parse_token_diff_args(
    args: impl Iterator<Item = String>,
) -> Result<TokenDiffArgs, Box<dyn Error>> {
    let mut corpus = false;
    let mut files = Vec::new();
    let mut limit = None;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--corpus" => corpus = true,
            "--files" => {
                let value = args.next().ok_or("missing value after --files")?;
                files.extend(
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(PathBuf::from),
                );
            }
            "--limit" => {
                let value = args.next().ok_or("missing value after --limit")?;
                limit = Some(value.parse()?);
            }
            _ => files.push(PathBuf::from(arg)),
        }
    }

    Ok(TokenDiffArgs {
        corpus,
        files,
        limit,
    })
}

fn rust_token_dump(path: &Path) -> Result<String, Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    let variant = language_variant_for_path(path);
    Ok(rust_token_dump_text(&text, variant))
}

fn rust_token_dump_text(text: &str, variant: tsrs2_syntax::LanguageVariant) -> String {
    let mut out = String::new();
    for token in tsrs2_syntax::scan_tokens(text, variant) {
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}",
            token.kind as u16,
            token.start,
            token.end,
            u8::from(token.preceding_line_break)
        );
    }
    out
}

#[derive(Debug, Serialize)]
struct TokenDumpRequest<'text> {
    id: u64,
    payload: TokenDumpPayload<'text>,
}

#[derive(Debug, Serialize)]
struct TokenDumpPayload<'text> {
    #[serde(rename = "textBase64")]
    text_base64: &'text str,
    variant: &'static str,
}

#[derive(Debug, Deserialize)]
struct TokenDumpResponse {
    id: Option<u64>,
    ok: bool,
    result: Option<String>,
    error: Option<String>,
}

fn parse_diags(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let path = parse_single_path_arg("parse-diags", args)?;
    let text = fs::read_to_string(&path)?;
    let file_name = path.to_string_lossy();
    let source = if file_name.ends_with(".json") {
        tsrs2_syntax::parse_json_text(file_name.to_string(), text)
    } else {
        tsrs2_syntax::parse_source_file(
            file_name.to_string(),
            text,
            tsrs2_syntax::ParseOptions {
                language_variant: language_variant_for_path(&path),
                // ast-dump.mjs uses ScriptKind TS/TSX, never JS.
                javascript_file: false,
                ..tsrs2_syntax::ParseOptions::default()
            },
            None,
        )
    };
    for diagnostic in &source.parse_diagnostics {
        println!(
            "{} start={} len={} :: {}",
            diagnostic.code(),
            diagnostic.start.unwrap_or(0),
            diagnostic.length.unwrap_or(0),
            diagnostic.message.text
        );
    }
    Ok(())
}

fn ast_dump(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let path = parse_single_path_arg("ast-dump", args)?;
    let text = fs::read_to_string(&path)?;
    let (dump, parse_errors) = rust_ast_dump_text(&path.to_string_lossy(), &text);
    print!("{dump}");
    if parse_errors > 0 {
        eprintln!("parse errors: {parse_errors}");
    }
    Ok(())
}

/// impl-nodes.md §5: the (kind, pos-utf16, end-utf16) indented pre-order tree
/// via the generated for_each_child, plus the parse-error count that gates
/// tree comparison.
fn rust_ast_dump_text(file_name: &str, text: &str) -> (String, usize) {
    let variant = language_variant_for_path(Path::new(file_name));
    let source = tsrs2_syntax::parse_source_file(
        file_name,
        text,
        tsrs2_syntax::ParseOptions {
            language_variant: variant,
            // ast-dump.mjs uses ScriptKind TS/TSX, never JS.
            javascript_file: false,
            ..tsrs2_syntax::ParseOptions::default()
        },
        None,
    );
    let map = tsrs2_diags::compute_line_map(text);
    let to_utf16 =
        |pos: u32| -> u32 { map.byte_to_utf16.get(pos as usize).copied().unwrap_or(pos) };

    let mut out = String::new();
    let mut stack = vec![(source.root, 0usize)];
    while let Some((id, depth)) = stack.pop() {
        let node = source.arena.node(id);
        let _ = writeln!(
            out,
            "{}{} {} {}",
            "  ".repeat(depth),
            node.kind as u16,
            to_utf16(node.pos),
            to_utf16(node.end)
        );
        let mut children = Vec::new();
        tsrs2_syntax::for_each_child(&source.arena, node, |child| {
            children.push(child);
            false
        });
        for child in children.into_iter().rev() {
            stack.push((child, depth + 1));
        }
    }
    (out, source.parse_diagnostics.len())
}

fn ast_diff(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let args = parse_token_diff_args(args)?;
    let workspace = find_tsrs2_root()?;
    let mut files = if args.corpus {
        collect_fixture_paths(&workspace.join("ts-tests/tests/cases/conformance"))?
    } else {
        args.files
    };
    if files.is_empty() {
        return Err("ast-diff requires --corpus, --files, or a file path".into());
    }
    files.sort();
    if let Some(limit) = args.limit {
        files.truncate(limit);
    }

    let mut oracle = AstDumpOracle::spawn(&workspace)?;
    let mut compared = 0usize;
    let mut excluded = 0usize;
    let mut differing = 0usize;
    let mut failures = String::new();
    for file in &files {
        let text = fs::read_to_string(file)?;
        let file_name = file.to_string_lossy();
        let (rust_dump, rust_parse_errors) = rust_ast_dump_text(&file_name, &text);
        let oracle_result = oracle.ast_dump(file, &text, &file_name)?;
        // Error-recovery trees may legitimately differ in Missing-node
        // placement; error fixtures are covered by the diagnostic gate.
        if rust_parse_errors > 0 || oracle_result.parse_errors > 0 {
            excluded += 1;
            continue;
        }
        compared += 1;
        if rust_dump != oracle_result.dump {
            differing += 1;
            let (line, left, right) = first_diff(&rust_dump, &oracle_result.dump);
            let entry = format!(
                "diff {} line {}:\n  tsrs:   {}\n  oracle: {}",
                file.display(),
                line,
                left.unwrap_or("<missing>"),
                right.unwrap_or("<missing>")
            );
            if differing <= 10 {
                println!("{entry}");
            }
            failures.push_str(&entry);
            failures.push('\n');
        }
    }

    let failures_path = workspace.join("target/ast-diff-failures.txt");
    if let Some(parent) = failures_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&failures_path, &failures)?;

    println!(
        "ast diff: files={} compared={} excluded={} differing={}",
        files.len(),
        compared,
        excluded,
        differing
    );
    println!("failures: {}", failures_path.display());
    if differing > 0 {
        return Err(
            format!("ast diff failed: {differing}/{compared} compared files differ").into(),
        );
    }
    Ok(())
}

struct AstDumpOracle {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl AstDumpOracle {
    fn spawn(workspace: &Path) -> Result<Self, Box<dyn Error>> {
        let mut child = Command::new("node")
            .arg(workspace.join("crates/oracle/ast-dump.mjs"))
            .arg("--server-jsonl")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or("ast dump oracle stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("ast dump oracle stdout unavailable")?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    fn ast_dump(
        &mut self,
        path: &Path,
        text: &str,
        file_name: &str,
    ) -> Result<AstDumpResult, Box<dyn Error>> {
        let id = self.next_id;
        self.next_id += 1;
        let text_base64 = BASE64.encode(text);
        let request = serde_json::to_string(&AstDumpRequest {
            id,
            payload: AstDumpPayload {
                text_base64: &text_base64,
                file_name,
            },
        })?;
        writeln!(self.stdin, "{request}")?;
        self.stdin.flush()?;

        let mut line = String::new();
        let read = self.stdout.read_line(&mut line)?;
        if read == 0 {
            return Err(format!(
                "oracle ast dump worker exited without a response for {}",
                path.display()
            )
            .into());
        }

        let response: AstDumpResponse = serde_json::from_str(&line)?;
        if response.id != Some(id) {
            return Err(format!(
                "oracle ast dump response id mismatch for {}: expected {id}, got {}{}",
                path.display(),
                response
                    .id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "null".to_owned()),
                response
                    .error
                    .as_deref()
                    .map(|error| format!(": {error}"))
                    .unwrap_or_default()
            )
            .into());
        }
        if !response.ok {
            return Err(format!(
                "oracle ast dump failed for {}: {}",
                path.display(),
                response.error.unwrap_or_else(|| "unknown error".to_owned())
            )
            .into());
        }
        response.result.ok_or_else(|| {
            format!(
                "oracle ast dump response missing result for {}",
                path.display()
            )
            .into()
        })
    }
}

impl Drop for AstDumpOracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Serialize)]
struct AstDumpRequest<'text> {
    id: u64,
    payload: AstDumpPayload<'text>,
}

#[derive(Debug, Serialize)]
struct AstDumpPayload<'text> {
    #[serde(rename = "textBase64")]
    text_base64: &'text str,
    #[serde(rename = "fileName")]
    file_name: &'text str,
}

#[derive(Debug, Deserialize)]
struct AstDumpResponse {
    id: Option<u64>,
    ok: bool,
    result: Option<AstDumpResult>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AstDumpResult {
    dump: String,
    #[serde(rename = "parseErrors")]
    parse_errors: usize,
}

/// m2-binder-steps.md stage 3.0: compare the Rust symbol audit against
/// oracle symbol-dump.mjs, program.json by program.json. The audit is a
/// TS-only SPOT check: .js/.jsx/.json program files are skipped (the JS
/// special-assignment symbol bodies land in stage 3.4), and files with
/// parse errors on either side are excluded like ast-diff.
fn symbol_diff(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut fixtures: Vec<PathBuf> = Vec::new();
    let mut sample: Option<usize> = None;
    let mut limit: Option<usize> = None;
    let mut positions_only = false;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sample" => {
                let value = args.next().ok_or("missing value after --sample")?;
                sample = Some(value.parse()?);
            }
            "--limit" => {
                let value = args.next().ok_or("missing value after --limit")?;
                limit = Some(value.parse()?);
            }
            // Walk-parity mode: compare only the pos/end columns, so the
            // audit WALK mirror is verifiable before the binder exists.
            "--positions-only" => positions_only = true,
            _ => fixtures.push(PathBuf::from(arg)),
        }
    }

    let workspace = find_tsrs2_root()?;
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    if let Some(sample) = sample {
        if !fixtures.is_empty() {
            return Err("--sample and explicit fixture paths are mutually exclusive".into());
        }
        let mut corpus =
            collect_fixture_paths(&workspace.join("ts-tests/tests/cases/conformance"))?;
        corpus.sort();
        // Deterministic stride sample across the sorted corpus.
        let count = sample.min(corpus.len());
        for index in 0..count {
            fixtures.push(corpus[index * corpus.len() / count].clone());
        }
    }
    if fixtures.is_empty() {
        return Err("symbol-diff requires fixture paths or --sample N".into());
    }
    fixtures.sort();
    if let Some(limit) = limit {
        fixtures.truncate(limit);
    }

    let temp_root = std::env::temp_dir().join(format!("tsrs2-symbol-diff-{}", std::process::id()));
    if temp_root.exists() {
        fs::remove_dir_all(&temp_root)?;
    }
    fs::create_dir_all(&temp_root)?;

    let mut oracle = SymbolDumpOracle::spawn(&workspace)?;
    let mut programs = 0usize;
    let mut compared = 0usize;
    let mut excluded = 0usize;
    let mut skipped_non_ts = 0usize;
    let mut differing = 0usize;
    let mut failures = String::new();

    for (fixture_index, fixture) in fixtures.iter().enumerate() {
        let expanded = tsrs2_harness::expand_fixture_file(fixture, &vendor_lib_dir)?;
        let out_dir = temp_root.join(fixture_index.to_string());
        let paths = tsrs2_harness::write_program_jsons(&expanded, &out_dir)?;
        for (program, path) in expanded.iter().zip(&paths) {
            programs += 1;
            let oracle_files = oracle.symbol_dump(path)?;
            let rust_files = rust_symbol_dump(program)?;
            if oracle_files.len() != rust_files.len() {
                return Err(format!(
                    "symbol dump file-count mismatch for {}: oracle {} vs tsrs {}",
                    path.display(),
                    oracle_files.len(),
                    rust_files.len()
                )
                .into());
            }
            for (oracle_file, rust_file) in oracle_files.iter().zip(&rust_files) {
                let Some(rust_file) = rust_file else {
                    skipped_non_ts += 1;
                    continue;
                };
                if oracle_file.parse_errors > 0 || rust_file.parse_errors > 0 {
                    excluded += 1;
                    continue;
                }
                compared += 1;
                let project = |lines: &[String]| -> String {
                    if positions_only {
                        lines
                            .iter()
                            .map(|line| line.splitn(3, '\t').take(2).collect::<Vec<_>>().join("\t"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        lines.join("\n")
                    }
                };
                // Documented audit normalizations (per-file binder vs a
                // whole-program checker):
                //  - lines whose ORACLE symbol carries the Transient bit
                //    (33554432) are checker-MERGED symbols (lib/global
                //    interface merging, M4 territory) — dropped in pairs;
                //  - `__#N@` private-name ids embed tsc's program-global
                //    getSymbolId counter (libs advance it) — the counter
                //    digits are wildcarded, keeping the structure check.
                let (oracle_lines, rust_lines) =
                    if oracle_file.lines.len() == rust_file.lines.len() && !positions_only {
                        normalized_symbol_audit_lines(&oracle_file.lines, &rust_file.lines)
                    } else {
                        (oracle_file.lines.clone(), rust_file.lines.clone())
                    };
                let oracle_dump = project(&oracle_lines);
                let rust_dump = project(&rust_lines);
                if !oracle_file.in_program || oracle_dump != rust_dump {
                    differing += 1;
                    let (line, left, right) = first_diff(&rust_dump, &oracle_dump);
                    let entry = format!(
                        "diff {} [{}] {} line {}:\n  tsrs:   {}\n  oracle: {}",
                        fixture.display(),
                        program.matrix_key,
                        rust_file.name,
                        line,
                        left.unwrap_or("<missing>"),
                        if oracle_file.in_program {
                            right.unwrap_or("<missing>")
                        } else {
                            "<file not in oracle program>"
                        }
                    );
                    if differing <= 10 {
                        println!("{entry}");
                    }
                    failures.push_str(&entry);
                    failures.push('\n');
                }
            }
        }
    }

    fs::remove_dir_all(&temp_root)?;
    let failures_path = workspace.join("target/symbol-diff-failures.txt");
    if let Some(parent) = failures_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&failures_path, &failures)?;

    println!(
        "symbol diff: fixtures={} programs={} compared={} excluded={} skipped-non-ts={} differing={}",
        fixtures.len(),
        programs,
        compared,
        excluded,
        skipped_non_ts,
        differing
    );
    println!("failures: {}", failures_path.display());
    if differing > 0 {
        return Err(
            format!("symbol diff failed: {differing}/{compared} compared files differ").into(),
        );
    }
    Ok(())
}

/// m2-binder-steps.md final gate: bind every corpus fixture; expect
/// zero panics. JS files bind too (crash-free is the gate; their
/// symbol bodies are stage 3.4c).
fn bind_corpus(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut limit: Option<usize> = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--limit" => {
                let value = args.next().ok_or("missing value after --limit")?;
                limit = Some(value.parse()?);
            }
            other => return Err(format!("unexpected bind-corpus argument: {other}").into()),
        }
    }
    let workspace = find_tsrs2_root()?;
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let mut fixtures = collect_fixture_paths(&workspace.join("ts-tests/tests/cases/conformance"))?;
    fixtures.sort();
    if let Some(limit) = limit {
        fixtures.truncate(limit);
    }

    let mut programs = 0usize;
    let mut files_bound = 0usize;
    let mut flow_nodes = 0usize;
    let mut symbols = 0usize;
    for fixture in &fixtures {
        let expanded = tsrs2_harness::expand_fixture_file(fixture, &vendor_lib_dir)?;
        for program in &expanded {
            programs += 1;
            let options = tsrs2_conformance::compiler_options_from_program(program);
            let mut last_text_b64: BTreeMap<&str, &str> = BTreeMap::new();
            for file in &program.files {
                last_text_b64.insert(file.name.as_str(), file.text_b64.as_str());
            }
            for file in &program.files {
                if file.name.ends_with(".json") {
                    continue;
                }
                let is_js = [".js", ".jsx", ".mjs", ".cjs"]
                    .iter()
                    .any(|extension| file.name.ends_with(extension));
                if is_js && !options.allow_js {
                    continue;
                }
                let bytes = BASE64.decode(last_text_b64[file.name.as_str()])?;
                let Ok(text) = String::from_utf8(bytes) else {
                    continue;
                };
                let language_variant = if file.name.ends_with(".tsx") || is_js {
                    tsrs2_syntax::LanguageVariant::Jsx
                } else {
                    tsrs2_syntax::LanguageVariant::Standard
                };
                let source = tsrs2_syntax::parse_source_file(
                    file.name.clone(),
                    text,
                    tsrs2_syntax::ParseOptions {
                        language_variant,
                        javascript_file: is_js,
                        ..tsrs2_syntax::ParseOptions::default()
                    },
                    None,
                );
                let binder = tsrs2_binder::bind_source_file(&source, &options);
                files_bound += 1;
                flow_nodes += binder.flow.len();
                symbols += binder.symbols.len();
            }
        }
    }
    println!(
        "bind corpus: fixtures={} programs={} files_bound={} symbols={} flow_nodes={} panics=0",
        fixtures.len(),
        programs,
        files_bound,
        symbols,
        flow_nodes
    );
    Ok(())
}

fn rust_symbol_dump(
    program: &tsrs2_harness::ProgramJson,
) -> Result<Vec<Option<symbol_audit::FileAudit>>, Box<dyn Error>> {
    // tsc host semantics: files are a name-keyed map, so a later file with
    // the same name shadows an earlier one entirely.
    let mut last_text_b64: BTreeMap<&str, &str> = BTreeMap::new();
    for file in &program.files {
        last_text_b64.insert(file.name.as_str(), file.text_b64.as_str());
    }

    let options = tsrs2_conformance::compiler_options_from_program(program);
    let mut out = Vec::with_capacity(program.files.len());
    for file in &program.files {
        if !is_ts_like_file_name(&file.name) {
            out.push(None);
            continue;
        }
        let bytes = BASE64.decode(last_text_b64[file.name.as_str()])?;
        let text = String::from_utf8(bytes)?;
        let language_variant = if file.name.ends_with(".tsx") {
            tsrs2_syntax::LanguageVariant::Jsx
        } else {
            tsrs2_syntax::LanguageVariant::Standard
        };
        let source = tsrs2_syntax::parse_source_file(
            file.name.clone(),
            text,
            tsrs2_syntax::ParseOptions {
                language_variant,
                javascript_file: false,
                ..tsrs2_syntax::ParseOptions::default()
            },
            None,
        );
        let binder = tsrs2_binder::bind_source_file(&source, &options);
        let lines = symbol_audit::audit_source_file(&source, &binder);
        out.push(Some(symbol_audit::FileAudit {
            name: file.name.clone(),
            parse_errors: source.parse_diagnostics.len(),
            lines,
        }));
    }
    Ok(out)
}

/// tsc getSymbolNameForPrivateIdentifier embeds the program-global
/// getSymbolId counter: `__#57@#name`. Wildcard the digits.
fn wildcard_private_name_ids(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(index) = rest.find("__#") {
        out.push_str(&rest[..index + 3]);
        rest = &rest[index + 3..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && rest[digits..].starts_with('@') {
            out.push('*');
            rest = &rest[digits..];
        }
    }
    out.push_str(rest);
    out
}

/// TS-only audit carve-out (m2-binder-steps.md stage 3.4): .js and .json
/// program files stay out of the audit until the JS special-assignment
/// symbol bodies land.
fn is_ts_like_file_name(name: &str) -> bool {
    [".ts", ".tsx", ".mts", ".cts"]
        .iter()
        .any(|extension| name.ends_with(extension))
}

struct SymbolDumpOracle {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl SymbolDumpOracle {
    fn spawn(workspace: &Path) -> Result<Self, Box<dyn Error>> {
        let mut child = Command::new("node")
            .arg(workspace.join("crates/oracle/symbol-dump.mjs"))
            .arg("--server-jsonl")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or("symbol dump oracle stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("symbol dump oracle stdout unavailable")?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    fn symbol_dump(&mut self, program_json: &Path) -> Result<Vec<OracleFileAudit>, Box<dyn Error>> {
        let id = self.next_id;
        self.next_id += 1;
        let request = serde_json::to_string(&SymbolDumpRequest {
            id,
            program_json_path: &program_json.display().to_string(),
        })?;
        writeln!(self.stdin, "{request}")?;
        self.stdin.flush()?;

        let mut line = String::new();
        let read = self.stdout.read_line(&mut line)?;
        if read == 0 {
            return Err(format!(
                "oracle symbol dump worker exited without a response for {}",
                program_json.display()
            )
            .into());
        }

        let response: SymbolDumpResponse = serde_json::from_str(&line)?;
        if response.id != Some(id) {
            return Err(format!(
                "oracle symbol dump response id mismatch for {}: expected {id}, got {}{}",
                program_json.display(),
                response
                    .id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "null".to_owned()),
                response
                    .error
                    .as_deref()
                    .map(|error| format!(": {error}"))
                    .unwrap_or_default()
            )
            .into());
        }
        if !response.ok {
            return Err(format!(
                "oracle symbol dump failed for {}: {}",
                program_json.display(),
                response.error.unwrap_or_else(|| "unknown error".to_owned())
            )
            .into());
        }
        let result = response.result.ok_or_else(|| {
            format!(
                "oracle symbol dump response missing result for {}",
                program_json.display()
            )
        })?;
        Ok(result.files)
    }
}

impl Drop for SymbolDumpOracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Serialize)]
struct SymbolDumpRequest<'path> {
    id: u64,
    #[serde(rename = "programJsonPath")]
    program_json_path: &'path str,
}

#[derive(Debug, Deserialize)]
struct SymbolDumpResponse {
    id: Option<u64>,
    ok: bool,
    result: Option<SymbolDumpResult>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SymbolDumpResult {
    files: Vec<OracleFileAudit>,
}

#[derive(Debug, Deserialize)]
struct OracleFileAudit {
    #[allow(dead_code)]
    name: String,
    #[serde(rename = "inProgram")]
    in_program: bool,
    #[serde(rename = "parseErrors")]
    parse_errors: usize,
    lines: Vec<String>,
}

struct TokenDumpOracle {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl TokenDumpOracle {
    fn spawn(workspace: &Path) -> Result<Self, Box<dyn Error>> {
        let mut child = Command::new("node")
            .arg(workspace.join("crates/oracle/token-dump.mjs"))
            .arg("--server-jsonl")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or("token dump oracle stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("token dump oracle stdout unavailable")?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    fn token_dump(
        &mut self,
        path: &Path,
        text: &str,
        variant: &'static str,
    ) -> Result<String, Box<dyn Error>> {
        let id = self.next_id;
        self.next_id += 1;
        let text_base64 = BASE64.encode(text);
        let request = serde_json::to_string(&TokenDumpRequest {
            id,
            payload: TokenDumpPayload {
                text_base64: &text_base64,
                variant,
            },
        })?;
        writeln!(self.stdin, "{request}")?;
        self.stdin.flush()?;

        let mut line = String::new();
        let read = self.stdout.read_line(&mut line)?;
        if read == 0 {
            return Err(format!(
                "oracle token dump worker exited without a response for {}",
                path.display()
            )
            .into());
        }

        let response: TokenDumpResponse = serde_json::from_str(&line)?;
        if response.id != Some(id) {
            return Err(format!(
                "oracle token dump response id mismatch for {}: expected {id}, got {}{}",
                path.display(),
                response
                    .id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "null".to_owned()),
                response
                    .error
                    .as_deref()
                    .map(|error| format!(": {error}"))
                    .unwrap_or_default()
            )
            .into());
        }
        if !response.ok {
            return Err(format!(
                "oracle token dump failed for {}: {}",
                path.display(),
                response.error.unwrap_or_else(|| "unknown error".to_owned())
            )
            .into());
        }
        response.result.ok_or_else(|| {
            format!(
                "oracle token dump response missing result for {}",
                path.display()
            )
            .into()
        })
    }
}

impl Drop for TokenDumpOracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn language_variant_for_path(path: &Path) -> tsrs2_syntax::LanguageVariant {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("tsx" | "jsx") => tsrs2_syntax::LanguageVariant::Jsx,
        _ => tsrs2_syntax::LanguageVariant::Standard,
    }
}

fn language_variant_arg(path: &Path) -> &'static str {
    match language_variant_for_path(path) {
        tsrs2_syntax::LanguageVariant::Standard => "standard",
        tsrs2_syntax::LanguageVariant::Jsx => "jsx",
    }
}

fn first_diff<'a>(left: &'a str, right: &'a str) -> (usize, Option<&'a str>, Option<&'a str>) {
    let mut left_lines = left.lines();
    let mut right_lines = right.lines();
    for line_number in 1.. {
        let left = left_lines.next();
        let right = right_lines.next();
        if left != right {
            return (line_number, left, right);
        }
        if left.is_none() && right.is_none() {
            return (line_number, None, None);
        }
    }
    unreachable!("unbounded line iterator returns from inside the loop")
}

fn oracle_refresh(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let parsed = parse_conformance_args(args)?;
    let workspace = find_tsrs2_root()?;
    let summary = tsrs2_conformance::refresh_oracle_goldens(&tsrs2_conformance::RefreshOptions {
        workspace,
        limit: parsed.limit,
        files: parsed.files,
    })?;
    println!(
        "oracle refresh wrote {} fixtures / {} cases / {} oracle diagnostics under {}",
        summary.fixtures, summary.cases, summary.oracle_diagnostics, summary.goldens_root
    );
    Ok(())
}

/// `cargo xtask goldens-diff [--baseline <ref>] [--out <path>]`: the
/// oracle-correction review surface — old (committed at the ref) vs
/// new (working tree) golden oracle records at occurrence
/// granularity, per-(code, pass) deltas, per-view bucket totals, and
/// the accepted identities guaranteed to lapse.
fn goldens_diff(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut baseline = "HEAD".to_owned();
    let mut out: Option<PathBuf> = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--baseline" => {
                baseline = args.next().ok_or("missing value after --baseline")?;
            }
            "--out" => {
                out = Some(PathBuf::from(
                    args.next().ok_or("missing value after --out")?,
                ));
            }
            _ => return Err(format!("unexpected goldens-diff argument: {arg}").into()),
        }
    }
    let workspace = find_tsrs2_root()?;
    let out_json = out.unwrap_or_else(|| workspace.join("target/goldens-diff.json"));
    let report = tsrs2_conformance::goldens_diff::goldens_diff(
        &tsrs2_conformance::goldens_diff::GoldensDiffOptions {
            workspace,
            baseline,
            out_json: out_json.clone(),
        },
    )?;
    println!(
        "goldens diff vs {}: {} of {} fixtures changed ({} cases); occurrences +{} / -{}",
        report.baseline,
        report.fixtures_changed,
        report.fixtures_total,
        report.cases_changed,
        report.added.len(),
        report.removed.len(),
    );
    for (view, totals) in &report.view_totals {
        println!(
            "  {view}: oracle T0 buckets {} -> {} ({:+})",
            totals.old_buckets,
            totals.new_buckets,
            totals.new_buckets as i64 - totals.old_buckets as i64,
        );
    }
    let mut deltas: Vec<_> = report.code_pass_deltas.iter().collect();
    deltas.sort_by_key(|(_, delta)| std::cmp::Reverse(delta.added + delta.removed));
    for (key, delta) in deltas.iter().take(15) {
        println!("  code/pass {key}: +{} / -{}", delta.added, delta.removed);
    }
    if deltas.len() > 15 {
        println!("  ... and {} more code/pass rows", deltas.len() - 15);
    }
    for (view, lapses) in &report.guaranteed_lapses {
        println!(
            "  guaranteed accepted-match lapses ({view}): {}",
            lapses.len()
        );
    }
    println!("full report: {}", out_json.display());
    Ok(())
}

fn conformance(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let parsed = parse_conformance_args(args)?;
    let workspace = find_tsrs2_root()?;
    let out_json = parsed
        .out_json
        .unwrap_or_else(|| workspace.join("target/conformance/mismatches.json"));
    let summary = tsrs2_conformance::run_conformance(&tsrs2_conformance::ConformanceOptions {
        workspace,
        limit: parsed.limit,
        files: parsed.files,
        out_json: out_json.clone(),
        band: parsed.band,
    })?;
    println!(
        "conformance band={} fixtures={} cases={} T0={:.4}% matched={}/{} FP={} FN={} mismatches={}",
        summary.band,
        summary.fixtures_total,
        summary.cases_total,
        summary.t0_rate * 100.0,
        summary.matched_t0_diagnostics,
        summary.oracle_diagnostics,
        summary.false_positive_diagnostics,
        summary.false_negative_diagnostics,
        summary.mismatch_cases
    );
    println!(
        "FN partial-boundary audit: reached={} no-evidence={}",
        summary.fn_with_partial_boundary_evidence, summary.fn_without_partial_boundary_evidence
    );
    println!(
        "shadow tiers T1={:.4}% ({}, ratcheted when configured) T2={:.4}% ({}, non-gating) T3={:.4}% ({}, non-gating)",
        summary.shadow_t1_rate * 100.0,
        summary.shadow_t1_matched,
        summary.shadow_t2_rate * 100.0,
        summary.shadow_t2_matched,
        summary.shadow_t3_rate * 100.0,
        summary.shadow_t3_matched
    );
    println!(
        "M8 scope={} entries={} excluded={} unresolved={} resolved-t0={} supported T0={:.4}% ({}/{}) T1={:.4}% T2={:.4}% T3={:.4}% FN={}",
        summary.scope_status,
        summary.scope_manifest_entries,
        summary.scope_excluded_diagnostics,
        summary.scope_unresolved_diagnostics,
        summary.scope_resolved_t0_diagnostics,
        summary.supported_t0_rate * 100.0,
        summary.supported_matched_t0_diagnostics,
        summary.supported_oracle_diagnostics,
        summary.supported_t1_rate * 100.0,
        summary.supported_t2_rate * 100.0,
        summary.supported_t3_rate * 100.0,
        summary.supported_false_negative_diagnostics,
    );
    println!("mismatch json: {}", out_json.display());
    Ok(())
}

/// A1 set-monotone conformance state (measurement-integrity.md §2):
/// `check` verifies both `ratchets/` artifacts against the tree and
/// their append-only lineage (plus the trusted PR-base compare with
/// `--baseline`); `update` measures the full corpus and adds
/// identities only.
fn ratchet_check(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut baseline = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--baseline" => {
                baseline = Some(args.next().ok_or("missing value after --baseline")?);
            }
            _ => return Err(format!("unexpected ratchet check argument: {arg}").into()),
        }
    }
    tsrs2_conformance::ratchet::check(&find_tsrs2_root()?, baseline.as_deref())
}

/// `cargo xtask scope audit [--baseline <trusted-ref>]`: the A2 exact
/// scope audit (measurement-integrity.md §3) — manifest structure,
/// occurrence resolution against pinned goldens, duplicate-bucket
/// canaries, the Node/Rust canonical-encoder cross-check, band-pin and
/// global-freeze anchors, standing tombstone proofs, and the
/// trusted-base compare.
fn scope_audit(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut baseline = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--baseline" => {
                baseline = Some(args.next().ok_or("missing value after --baseline")?);
            }
            _ => return Err(format!("unexpected scope audit argument: {arg}").into()),
        }
    }
    tsrs2_conformance::scope_audit(&find_tsrs2_root()?, baseline.as_deref())
}

/// `cargo xtask families check [--baseline <trusted-ref>]`: the A5
/// family-map audit (measurement-integrity.md §5) — map structure and
/// the exactly-once domain over every corpus-exercised non-2XXX
/// (code, pass) row, canary existence, the freeze/extension reviewed
/// snapshot anchors, and the trusted-base compare.
fn families_check(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut baseline = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--baseline" => {
                baseline = Some(args.next().ok_or("missing value after --baseline")?);
            }
            _ => return Err(format!("unexpected families check argument: {arg}").into()),
        }
    }
    tsrs2_conformance::families_check(&find_tsrs2_root()?, baseline.as_deref())
}

/// `cargo xtask families report [--out-json <path>] [--verify]`: the
/// A5 supported rollup from one current full band=all gating run
/// (never from A1 summaries). `--verify` re-checks an existing
/// report's input fingerprints against the tree instead of running.
fn families_report(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let root = find_tsrs2_root()?;
    let mut out_json: Option<PathBuf> = None;
    let mut verify = false;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-json" => {
                out_json = Some(PathBuf::from(
                    args.next().ok_or("missing value after --out-json")?,
                ));
            }
            "--verify" => verify = true,
            _ => return Err(format!("unexpected families report argument: {arg}").into()),
        }
    }
    let out_json = out_json.unwrap_or_else(|| root.join("target/families/report.json"));
    if verify {
        tsrs2_conformance::families_verify_report(&root, &out_json)
    } else {
        tsrs2_conformance::families_report(&root, &out_json)
    }
}

fn ratchet_update(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut transition = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--transition" => {
                transition = Some(args.next().ok_or("missing value after --transition")?);
            }
            _ => return Err(format!("unexpected ratchet update argument: {arg}").into()),
        }
    }
    tsrs2_conformance::ratchet::update(&find_tsrs2_root()?, transition.as_deref())
}

struct ConformanceArgs {
    limit: Option<usize>,
    files: Vec<PathBuf>,
    out_json: Option<PathBuf>,
    band: tsrs2_conformance::DiagnosticBand,
}

fn parse_conformance_args(
    args: impl Iterator<Item = String>,
) -> Result<ConformanceArgs, Box<dyn Error>> {
    let mut limit = None;
    let mut files = Vec::new();
    let mut out_json = None;
    let mut band = tsrs2_conformance::DiagnosticBand::All;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--limit" => {
                let value = args.next().ok_or("missing value after --limit")?;
                limit = Some(value.parse()?);
            }
            "--files" => {
                let value = args.next().ok_or("missing value after --files")?;
                files.extend(
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(PathBuf::from),
                );
            }
            "--out-json" => {
                let value = args.next().ok_or("missing value after --out-json")?;
                out_json = Some(PathBuf::from(value));
            }
            "--band" => {
                let value = args.next().ok_or("missing value after --band")?;
                band = match value.as_str() {
                    "all" => tsrs2_conformance::DiagnosticBand::All,
                    "2xxx" => tsrs2_conformance::DiagnosticBand::TwoXxx,
                    "syntactic" => tsrs2_conformance::DiagnosticBand::Syntactic,
                    _ => return Err(format!("unknown conformance band: {value}").into()),
                };
            }
            "--syntactic-only" => band = tsrs2_conformance::DiagnosticBand::Syntactic,
            _ => return Err(format!("unexpected conformance argument: {arg}").into()),
        }
    }

    Ok(ConformanceArgs {
        limit,
        files,
        out_json,
        band,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InvariantSuite {
    All,
    PrefixDeterminism,
    PrefixConformance,
    Idempotence,
    JobsIndependence,
    Encodings,
    MatrixIndependence,
    UnsupportedUnwind,
}

impl InvariantSuite {
    fn parse(value: &str) -> Result<Self, Box<dyn Error>> {
        match value {
            "all" => Ok(Self::All),
            "prefix-determinism" => Ok(Self::PrefixDeterminism),
            "prefix-conformance" => Ok(Self::PrefixConformance),
            "idempotence" => Ok(Self::Idempotence),
            "jobs-independence" => Ok(Self::JobsIndependence),
            "encodings" => Ok(Self::Encodings),
            "matrix-independence" => Ok(Self::MatrixIndependence),
            "unsupported-unwind" => Ok(Self::UnsupportedUnwind),
            _ => Err(format!("unknown invariant suite: {value}").into()),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::PrefixDeterminism => "prefix-determinism",
            Self::PrefixConformance => "prefix-conformance",
            Self::Idempotence => "idempotence",
            Self::JobsIndependence => "jobs-independence",
            Self::Encodings => "encodings",
            Self::MatrixIndependence => "matrix-independence",
            Self::UnsupportedUnwind => "unsupported-unwind",
        }
    }

    fn includes(self, suite: Self) -> bool {
        // prefix-conformance needs the node oracle; it never rides `all`.
        if suite == Self::PrefixConformance {
            return self == Self::PrefixConformance;
        }
        self == Self::All || self == suite
    }
}

struct InvariantArgs {
    suite: InvariantSuite,
    limit: usize,
}

#[derive(Clone, Debug)]
struct SampleProgram {
    fixture: String,
    matrix_key: String,
    files: Vec<InputFile>,
}

fn invariants(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let args = parse_invariant_args(args)?;
    let workspace = find_tsrs2_root()?;
    let programs = load_sample_programs(&workspace, args.limit)?;
    let fixture_count = programs
        .iter()
        .map(|program| program.fixture.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    if args.suite.includes(InvariantSuite::PrefixDeterminism) {
        run_prefix_determinism(&programs)?;
        println!(
            "invariant prefix-determinism ok: programs={}",
            programs.len()
        );
    }
    if args.suite.includes(InvariantSuite::PrefixConformance) {
        let summary = tsrs2_conformance::run_prefix_conformance(
            &tsrs2_conformance::PrefixConformanceOptions {
                workspace: workspace.clone(),
                limit: Some(args.limit),
                files: Vec::new(),
            },
        )?;
        for mismatch in summary.mismatches.iter().take(10) {
            println!(
                "prefix-conformance mismatch: {} [{}] file {} cut {} FP={:?} FN={:?}",
                mismatch.fixture,
                mismatch.matrix_key,
                mismatch.file,
                mismatch.cut,
                mismatch.false_positive,
                mismatch.false_negative
            );
        }
        if summary.mismatched_cases > 0 {
            return Err(format!(
                "prefix-conformance failed: {}/{} truncated cases diverge from the oracle",
                summary.mismatched_cases, summary.cases
            )
            .into());
        }
        println!(
            "invariant prefix-conformance ok: fixtures={} cases={}",
            summary.fixtures, summary.cases
        );
    }
    if args.suite.includes(InvariantSuite::Idempotence) {
        run_idempotence(&programs)?;
        println!("invariant idempotence ok: programs={}", programs.len());
    }
    if args.suite.includes(InvariantSuite::JobsIndependence) {
        run_jobs_independence(&programs)?;
        println!(
            "invariant jobs-independence ok: programs={}",
            programs.len()
        );
    }
    if args.suite.includes(InvariantSuite::Encodings) {
        run_encodings(&programs)?;
        println!("invariant encodings ok: programs={}", programs.len());
    }
    if args.suite.includes(InvariantSuite::MatrixIndependence) {
        run_matrix_independence(&programs)?;
        println!(
            "invariant matrix-independence ok: programs={}",
            programs.len()
        );
    }
    if args.suite.includes(InvariantSuite::UnsupportedUnwind) {
        run_unsupported_unwind(&programs)?;
        println!(
            "invariant unsupported-unwind ok: programs={}",
            programs.len()
        );
    }

    println!(
        "invariants suite={} fixtures={} programs={} ok",
        args.suite.name(),
        fixture_count,
        programs.len()
    );
    Ok(())
}

fn parse_invariant_args(
    args: impl Iterator<Item = String>,
) -> Result<InvariantArgs, Box<dyn Error>> {
    let mut suite = InvariantSuite::All;
    let mut limit = 200usize;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--suite" => {
                let value = args.next().ok_or("missing value after --suite")?;
                suite = InvariantSuite::parse(&value)?;
            }
            "--limit" => {
                let value = args.next().ok_or("missing value after --limit")?;
                limit = value.parse()?;
            }
            _ => return Err(format!("unexpected invariants argument: {arg}").into()),
        }
    }

    Ok(InvariantArgs { suite, limit })
}

fn load_sample_programs(
    workspace: &Path,
    limit: usize,
) -> Result<Vec<SampleProgram>, Box<dyn Error>> {
    let fixtures_root = workspace.join("ts-tests/tests/cases/conformance");
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let mut fixtures = collect_fixture_paths(&fixtures_root)?;
    fixtures.sort();
    fixtures.truncate(limit);

    let mut programs = Vec::new();
    for fixture in fixtures {
        let fixture_key = fixture
            .strip_prefix(&fixtures_root)?
            .to_string_lossy()
            .replace('\\', "/");
        for program in tsrs2_harness::expand_fixture_file(&fixture, &vendor_lib_dir)? {
            let files = program
                .files
                .into_iter()
                .map(|file| {
                    Ok(InputFile {
                        name: file.name,
                        text: base64_decode_to_string(&file.text_b64)?,
                    })
                })
                .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
            programs.push(SampleProgram {
                fixture: fixture_key.clone(),
                matrix_key: program.matrix_key,
                files,
            });
        }
    }

    Ok(programs)
}

fn midpoint_char_boundary(text: &str) -> usize {
    let midpoint = text.len() / 2;
    text.char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= midpoint)
        .last()
        .unwrap_or(0)
}

/// greenfield §7.6 prefix-determinism, reformulated at the TOKEN level:
/// scanning a truncated file yields the same tokens strictly before the cut
/// as scanning the full file. The original diagnostic-level formulation is
/// unsatisfiable for a tsc-faithful parser — recovery legitimately attributes
/// errors before the cut depending on later text, and tsc itself does
/// (counterexample in docs/NOTES-m1.md). Diagnostic-level fidelity on
/// truncated inputs is covered by the oracle-backed `prefix-conformance`
/// suite instead.
fn run_prefix_determinism(programs: &[SampleProgram]) -> Result<(), Box<dyn Error>> {
    for program in programs {
        for file in &program.files {
            let cut = midpoint_char_boundary(&file.text);
            let variant = language_variant_for_path(Path::new(&file.name));
            let full = tsrs2_syntax::scan_tokens(&file.text, variant);
            let prefix = tsrs2_syntax::scan_tokens(&file.text[..cut], variant);
            // Tokens touching the cut are inherently ambiguous (they may be
            // truncated or merge with later text); everything strictly
            // before it must be byte-identical.
            let full_before = full.iter().filter(|token| (token.end as usize) < cut);
            let prefix_before = prefix.iter().filter(|token| (token.end as usize) < cut);
            if !full_before.eq(prefix_before) {
                return Err(format!(
                    "prefix-determinism failed for {} [{}] file {} (cut {})",
                    program.fixture, program.matrix_key, file.name, cut
                )
                .into());
            }
        }
    }
    Ok(())
}

fn run_idempotence(programs: &[SampleProgram]) -> Result<(), Box<dyn Error>> {
    for program in programs {
        let first = check_bytes(&program.files);
        let second = check_bytes(&program.files);
        if first != second {
            return Err(format!(
                "idempotence failed for {} [{}]",
                program.fixture, program.matrix_key
            )
            .into());
        }
    }
    Ok(())
}

/// The unsupported-unwind sweep: run every sample program once with
/// the checker's debug unwind guards active (check.rs UnwindSnapshot
/// and the links.rs Resolving census) — a violated guard panics with
/// the offending element. The guards are plain debug_assertions, so
/// the lib-loaded conformance gate exercises them corpus-wide too;
/// this suite is the labeled, fast-attribution entry point.
fn run_unsupported_unwind(programs: &[SampleProgram]) -> Result<(), Box<dyn Error>> {
    if !cfg!(debug_assertions) {
        return Err(
            "unsupported-unwind needs debug_assertions (run via the dev-profile xtask)".into(),
        );
    }
    for program in programs {
        let _ = check_bytes(&program.files);
    }
    Ok(())
}

fn run_jobs_independence(programs: &[SampleProgram]) -> Result<(), Box<dyn Error>> {
    let baseline = run_programs_in_job_order(programs, 1);
    for jobs in 2..=16 {
        let candidate = run_programs_in_job_order(programs, jobs);
        if baseline != candidate {
            return Err(format!("jobs-independence failed for jobs={jobs}").into());
        }
    }
    Ok(())
}

fn run_encodings(programs: &[SampleProgram]) -> Result<(), Box<dyn Error>> {
    for program in programs {
        let baseline = diagnostic_semantic_bytes(&check_diagnostics(&program.files));
        for file_index in 0..program.files.len() {
            let original = &program.files[file_index].text;
            let variants = [
                original.trim_start_matches('\u{feff}').to_owned(),
                format!("\u{feff}{}", original.trim_start_matches('\u{feff}')),
                original.replace("\r\n", "\n"),
                original.replace('\n', "\r\n"),
            ];
            for variant in variants {
                let mut files = program.files.clone();
                files[file_index].text = variant;
                let candidate = diagnostic_semantic_bytes(&check_diagnostics(&files));
                if baseline != candidate {
                    return Err(format!(
                        "encodings failed for {} [{}] file {}",
                        program.fixture, program.matrix_key, files[file_index].name
                    )
                    .into());
                }
            }
        }
    }
    Ok(())
}

fn run_matrix_independence(programs: &[SampleProgram]) -> Result<(), Box<dyn Error>> {
    let mut by_fixture = BTreeMap::<&str, Vec<&SampleProgram>>::new();
    for program in programs {
        by_fixture
            .entry(&program.fixture)
            .or_default()
            .push(program);
    }

    for (fixture, fixture_programs) in by_fixture {
        if fixture_programs.len() < 2 {
            continue;
        }
        let forward = fixture_programs
            .iter()
            .map(|program| (program_key(program), check_bytes(&program.files)))
            .collect::<BTreeMap<_, _>>();
        let reverse = fixture_programs
            .iter()
            .rev()
            .map(|program| (program_key(program), check_bytes(&program.files)))
            .collect::<BTreeMap<_, _>>();
        if forward != reverse {
            return Err(format!("matrix-independence failed for {fixture}").into());
        }
    }
    Ok(())
}

fn run_programs_in_job_order(programs: &[SampleProgram], jobs: usize) -> BTreeMap<String, String> {
    let mut output = BTreeMap::new();
    for job in 0..jobs {
        for (index, program) in programs.iter().enumerate() {
            if index % jobs == job {
                output.insert(program_key(program), check_bytes(&program.files));
            }
        }
    }
    output
}

fn program_key(program: &SampleProgram) -> String {
    if program.matrix_key.is_empty() {
        program.fixture.clone()
    } else {
        format!("{}#{}", program.fixture, program.matrix_key)
    }
}

fn check_diagnostics(files: &[InputFile]) -> DiagnosticList {
    tsrs2_checker::check_program(files, &CompilerOptions::default()).diagnostics
}

fn check_bytes(files: &[InputFile]) -> String {
    diagnostic_bytes(&check_diagnostics(files))
}

fn diagnostic_bytes(diagnostics: &DiagnosticList) -> String {
    format!("{diagnostics:#?}")
}

fn diagnostic_semantic_bytes(diagnostics: &DiagnosticList) -> String {
    let mut out = String::new();
    for diagnostic in diagnostics {
        let _ = writeln!(
            out,
            "{}|{}|{}",
            diagnostic.file_name.as_deref().unwrap_or(""),
            diagnostic.code(),
            diagnostic.message_text()
        );
    }
    out
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LedgerEntry {
    rust_path: PathBuf,
    rust_line: usize,
    rust_fn: String,
    port_name: String,
    version: String,
    span_file: String,
    span_start: usize,
    span_end: usize,
    hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct PublicFunction {
    path: PathBuf,
    line: usize,
    name: String,
}

fn ledger_check() -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let entries = collect_ledger_entries(&workspace)?;
    let stale = verify_ledger_entries(&workspace, &entries)?;
    let public_functions = collect_hot_public_functions(&workspace)?;
    let unported = unported_public_functions(&entries, &public_functions);
    let todo_sites = collect_todo_port_sites(&workspace)?;
    let undispositioned = collect_undispositioned_checker_fns(&workspace)?;

    for entry in &stale {
        eprintln!("{entry}");
    }
    for site in &todo_sites {
        eprintln!("todo_port site: {site}");
    }

    println!(
        "ledger check: entries={} stale={} hot_pub_fns={} unported_pub_fns={} todo_port={} undispositioned={}",
        entries.len(),
        stale.len(),
        public_functions.len(),
        unported.len(),
        todo_sites.len(),
        undispositioned.len()
    );
    if !unported.is_empty() {
        println!("unported pub fns:");
        for function in &unported {
            println!(
                "  {}:{} {}",
                display_relative(&workspace, &function.path),
                function.line,
                function.name
            );
        }
    }

    // The disposition BACKLOG gate (review round 2): equality against
    // fn-dispositions.toml — a NEW undispositioned identity is
    // rejected outright (a same-commit annotate+add swap cannot slip
    // through a count ceiling), and burn-down must land as a
    // shrinking, reviewable diff.
    let backlog_path = workspace.join("fn-dispositions.toml");
    if !backlog_path.exists() {
        return Err(
            "fn-dispositions.toml is missing — run `cargo xtask ledger write-backlog`, \
             review, and commit it"
                .into(),
        );
    }
    let recorded_backlog = parse_fn_backlog(&fs::read_to_string(&backlog_path)?)?;
    let scanned_backlog = backlog_map(&undispositioned, &workspace);
    let mut backlog_divergences = 0usize;
    for (key, count) in &scanned_backlog {
        match recorded_backlog.get(key) {
            Some(recorded) if recorded == count => {}
            Some(recorded) if count < recorded => {
                backlog_divergences += 1;
                println!(
                    "BACKLOG-STALE-COUNT {}::{} — {count} undispositioned left of {recorded}: \
                     run `cargo xtask ledger write-backlog` (burn-down lands as a diff)",
                    key.0, key.1
                );
            }
            _ => {
                backlog_divergences += 1;
                println!(
                    "BACKLOG-NEW {}::{} — NEW undispositioned checker fn: give it a \
                     disposition header ({}) instead of listing it",
                    key.0,
                    key.1,
                    fn_disposition_markers().join(" / ")
                );
            }
        }
    }
    for key in recorded_backlog.keys() {
        if !scanned_backlog.contains_key(key) {
            backlog_divergences += 1;
            println!(
                "BACKLOG-STALE {}::{} — dispositioned or removed: run \
                 `cargo xtask ledger write-backlog` (the shrinking diff is the record)",
                key.0, key.1
            );
        }
    }
    if backlog_divergences > 0 {
        return Err(format!(
            "fn-disposition backlog out of date: {backlog_divergences} divergence(s)"
        )
        .into());
    }
    if !stale.is_empty() || !todo_sites.is_empty() {
        return Err("ledger check failed".into());
    }
    Ok(())
}

fn ledger_write_backlog() -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let undispositioned = collect_undispositioned_checker_fns(&workspace)?;
    let map = backlog_map(&undispositioned, &workspace);
    fs::write(
        workspace.join("fn-dispositions.toml"),
        render_fn_backlog(&map),
    )?;
    println!(
        "fn-dispositions.toml written: {} identities ({} fns) — review the diff",
        map.len(),
        undispositioned.len()
    );
    Ok(())
}

fn ledger_coverage() -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let entries = collect_ledger_entries(&workspace)?;
    let public_functions = collect_hot_public_functions(&workspace)?;
    let unported = unported_public_functions(&entries, &public_functions);

    println!(
        "ledger coverage: ported_entries={} hot_pub_fns={} unported_pub_fns={}",
        entries.len(),
        public_functions.len(),
        unported.len()
    );
    println!("ledger coverage: runtime hit data is not instrumented in M0");
    Ok(())
}

/// A parsed containment escape site: an `Unsupported::new(...)` or
/// `M4Dependency(...)` call with the stage owner parsed out of its
/// reason string.
struct EscapeSite {
    path: PathBuf,
    line: usize,
    /// The enclosing function's name — part of the manifest identity
    /// (review finding: (file, reason) alone let a same-count MOVE
    /// between functions land without a manifest diff).
    containing_fn: String,
    reason: String,
    owner: Option<StageKey>,
    /// Owner-less milestone-stable guards for malformed/parse-recovery
    /// trees — auditable as a class through M7, so they do not count
    /// against the untagged ratchet. Done still removes Unsupported
    /// from these paths. Classification is strict:
    /// only reasons carrying an explicit recovery marker qualify.
    recovery: bool,
}

/// The strict recovery-marker test: `(parse recovery)`,
/// `parse-recovery`, or `recovery node` in the reason text.
fn is_recovery_reason(reason: &str) -> bool {
    reason.contains("parse recovery")
        || reason.contains("parse-recovery")
        || reason.contains("recovery node")
}

/// Orderable stage key: M4 sub-stages sort inside milestone 4
/// ((4, minor, letter)), later milestones as (5..8, 0, 0). T2 counts
/// as M8 (display/precision work).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct StageKey(u32, u32, u8);

fn parse_stage_key(text: &str) -> Option<StageKey> {
    let bytes = text.as_bytes();
    let mut best: Option<StageKey> = None;
    let mut push = |key: StageKey| {
        // The LATEST stage named in a reason is its owner (a re-marked
        // deferral names the future stage after the historical one).
        if best.is_none_or(|current| key > current) {
            best = Some(key);
        }
    };
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'5' if i + 2 < bytes.len() && bytes[i + 1] == b'.' => {
                if let Some(minor) = (bytes[i + 2] as char).to_digit(10) {
                    // A letterless tag (`5.7`) owns the WHOLE stage:
                    // it only expires once a later stage is current.
                    let letter = bytes
                        .get(i + 3)
                        .filter(|byte| byte.is_ascii_lowercase())
                        .copied()
                        .unwrap_or(u8::MAX);
                    push(StageKey(4, minor, letter));
                    i += 3;
                    continue;
                }
            }
            b'M' => {
                if let Some(digit) = bytes.get(i + 1).and_then(|&b| (b as char).to_digit(10)) {
                    if (5..=8).contains(&digit) {
                        push(StageKey(digit, 0, 0));
                        i += 2;
                        continue;
                    }
                }
            }
            b'T' if bytes.get(i + 1) == Some(&b'2') => {
                push(StageKey(8, 0, 0));
                i += 2;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    best
}

/// Extract the first string literal following `offset` in `text`,
/// concatenating adjacent literals (rustfmt splits long reasons).
/// Byte-walking is UTF-8-safe here because every delimiter tested is
/// ASCII; the literal CONTENT is collected as raw bytes and decoded
/// once (pushing bytes as chars mojibake'd every multi-byte reason —
/// review finding).
fn escape_reason_after(text: &str, offset: usize) -> String {
    let mut content = Vec::new();
    let bytes = text.as_bytes();
    let mut i = offset;
    let mut depth = 0i32;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth <= 0 {
                    break;
                }
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    content.push(bytes[i]);
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let reason = String::from_utf8_lossy(&content);
    reason.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Scan one file's text for escape sites. Wrapper constructors count
/// too: expression_stub / source_element_stub carry (worker, owner)
/// string pairs — source_element_stub is a SILENT Ok(()) stub,
/// invisible to any Err-based accounting. Reasons built with format!
/// ARE scanned (their static text carries the owner tag); only the
/// wrappers' own `{worker}…{owner}` templates are excluded.
/// Owner-less reasons carrying an explicit recovery marker classify
/// as milestone-stable RECOVERY guards (auditable through M7 and
/// exempt from the untagged ratchet); everything else owner-less is
/// untagged debt. The final gate still removes Unsupported here.
fn scan_escape_text(path: &Path, text: &str) -> Vec<EscapeSite> {
    // Line-indexed fn-definition table for containing-fn lookup: the
    // last `fn name(` at or before an escape's line encloses it
    // (closures don't match `fn `; nested named fns resolve to the
    // innermost preceding definition, which is the enclosing one for
    // straight-line code).
    let mut fn_lines: Vec<(usize, String)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut trimmed = line.trim_start();
        loop {
            let stripped = trimmed
                .strip_prefix("pub(crate) ")
                .or_else(|| trimmed.strip_prefix("pub "))
                .or_else(|| trimmed.strip_prefix("async "))
                .or_else(|| trimmed.strip_prefix("const "))
                .or_else(|| trimmed.strip_prefix("unsafe "));
            match stripped {
                Some(rest) => trimmed = rest,
                None => break,
            }
        }
        if trimmed.starts_with("fn ") {
            if let Some(name) = function_name(trimmed) {
                fn_lines.push((index + 1, name));
            }
        }
    }
    let containing_fn = |line: usize| -> String {
        match fn_lines.iter().rev().find(|(fn_line, _)| *fn_line <= line) {
            Some((_, name)) => name.clone(),
            None => "<module>".to_owned(),
        }
    };
    let mut sites = Vec::new();
    for marker in [
        "Unsupported::new(",
        "M4Dependency(",
        "expression_stub(",
        "source_element_stub(",
    ] {
        let mut search = 0usize;
        while let Some(found) = text[search..].find(marker) {
            let offset = search + found;
            search = offset + marker.len();
            let line = text[..offset].bytes().filter(|&b| b == b'\n').count() + 1;
            let reason = escape_reason_after(text, offset + marker.len() - 1);
            // Empty reasons are definitions/imports; the wrapper
            // definitions interpolate their `worker` parameter.
            if reason.is_empty() || reason.contains("{worker}") {
                continue;
            }
            let owner = parse_stage_key(&reason);
            let recovery = owner.is_none() && is_recovery_reason(&reason);
            sites.push(EscapeSite {
                path: path.to_owned(),
                line,
                containing_fn: containing_fn(line),
                reason,
                owner,
                recovery,
            });
        }
    }
    sites
}

fn collect_escape_sites(workspace: &Path) -> Result<Vec<EscapeSite>, Box<dyn Error>> {
    let mut sites = Vec::new();
    for path in collect_rs_paths(&workspace.join("crates"))? {
        // xtask itself holds the marker strings (this scanner) — no
        // checker escapes live here.
        if path.components().any(|part| part.as_os_str() == "xtask") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        sites.extend(scan_escape_text(&path, &text));
    }
    sites.sort_by(|left, right| {
        left.owner
            .cmp(&right.owner)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.line.cmp(&right.line))
    });
    Ok(sites)
}

/// The expiry audit (stage-closing loop): list containment escapes
/// whose parsed owner stage is at or before `--stale <stage>` — those
/// justifications have expired and must be implemented or re-marked
/// with their real future owner. Untagged reasons are reported
/// separately (they cannot be audited).
fn escapes(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut stale_before: Option<StageKey> = None;
    let mut write_manifest = false;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--stale" => {
                let value = args.next().ok_or("missing value after --stale")?;
                stale_before = Some(
                    parse_stage_key(&value).ok_or_else(|| format!("unparseable stage: {value}"))?,
                );
            }
            "--write-manifest" => write_manifest = true,
            other => return Err(format!("unexpected escapes argument: {other}").into()),
        }
    }
    let workspace = find_tsrs2_root()?;
    let sites = collect_escape_sites(&workspace)?;
    if write_manifest {
        let entries = escape_manifest_from_sites(&workspace, &sites);
        fs::write(
            workspace.join("escapes.toml"),
            render_escape_manifest(&entries),
        )?;
        println!(
            "escapes.toml written: {} entries ({} sites) — review the diff",
            entries.len(),
            sites.len()
        );
    } else {
        check_escape_manifest(&workspace, &sites)?;
    }
    let mut stale = 0usize;
    let mut untagged = 0usize;
    let mut recovery = 0usize;
    for site in &sites {
        let relative = display_relative(&workspace, &site.path);
        match (site.owner, stale_before) {
            (Some(owner), Some(threshold)) if owner <= threshold => {
                stale += 1;
                println!("STALE {:?} {relative}:{} {}", owner, site.line, site.reason);
            }
            (None, _) if site.recovery => {
                recovery += 1;
                if stale_before.is_none() {
                    println!("RECOVERY {relative}:{} {}", site.line, site.reason);
                }
            }
            (None, _) => {
                untagged += 1;
                if stale_before.is_none() {
                    println!("UNTAGGED {relative}:{} {}", site.line, site.reason);
                }
            }
            (Some(owner), None) => {
                println!("{:?} {relative}:{} {}", owner, site.line, site.reason);
            }
            _ => {}
        }
    }
    println!(
        "escapes: sites={} stale={} untagged={} recovery={}",
        sites.len(),
        stale,
        untagged,
        recovery
    );
    if stale_before.is_some() && stale > 0 {
        return Err(format!("{stale} escape(s) have an expired owner stage").into());
    }
    // The untagged/recovery-count ratchets (gate mode only): both
    // monotone non-increasing — new escapes must carry a parseable
    // owner, new recovery guards may not accumulate unnoticed, and
    // re-tagging/retiring legacy reasons lowers the recorded ceilings
    // like any ratchet bump.
    if stale_before.is_some() {
        if let Some(ceiling) = read_ratchet_ceiling(&workspace, "escapes", "max_untagged")? {
            if untagged > ceiling {
                return Err(format!(
                    "untagged escape ratchet regression: {untagged} > recorded ceiling {ceiling} \
                     (tag the new reasons or bump [escapes].max_untagged in ratchet.toml)"
                )
                .into());
            }
        }
        if let Some(ceiling) = read_ratchet_ceiling(&workspace, "escapes", "max_recovery")? {
            if recovery > ceiling {
                return Err(format!(
                    "recovery escape ratchet regression: {recovery} > recorded ceiling {ceiling} \
                     (a new `(parse recovery)`-marked guard needs review — real containment \
                     escapes must carry an owner stage instead; bump [escapes].max_recovery \
                     in ratchet.toml only for genuine malformed-tree guards)"
                )
                .into());
            }
        }
    }
    Ok(())
}

/// An integer ceiling from a ratchet.toml section — an absent
/// section/key means that ratchet is not armed.
fn read_ratchet_ceiling(
    workspace: &Path,
    section: &str,
    ceiling_key: &str,
) -> Result<Option<usize>, Box<dyn Error>> {
    let text = fs::read_to_string(workspace.join("ratchet.toml"))?;
    let mut in_section = false;
    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_section = &line[1..line.len() - 1] == section;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim() == ceiling_key {
                return Ok(Some(value.trim().parse::<usize>()?));
            }
        }
    }
    Ok(None)
}

/// One escape MANIFEST entry: the reviewable identity of an escape
/// class is (file, containing fn, reason) — line numbers are
/// deliberately absent so unrelated edits don't churn the manifest,
/// while the containing fn pins moves/replacements between functions
/// (review finding). `count` catches duplicate-site growth under an
/// existing identity; class/owner are derived from the reason text
/// and recorded so retags surface as manifest diffs. Accepted
/// residual (review round 3): a same-reason site swap WITHIN one
/// function at unchanged count does not diff — this is
/// function-level debt tracking, not per-site audit.
#[derive(Clone, Debug, Eq, PartialEq)]
struct EscapeManifestEntry {
    file: String,
    containing_fn: String,
    reason: String,
    /// "stage" (owner-tagged deferral) | "recovery" (milestone-stable
    /// malformed-tree guard through M7) | "untagged" (debt — 0 by M4
    /// close).
    class: String,
    /// Display owner for class == "stage" ("5.8", "5.7b", "M5"…).
    owner: Option<String>,
    count: usize,
}

fn stage_key_display(key: StageKey) -> String {
    match key {
        StageKey(4, minor, u8::MAX) => format!("5.{minor}"),
        StageKey(4, minor, letter) => format!("5.{}{}", minor, letter as char),
        StageKey(milestone, _, _) => format!("M{milestone}"),
    }
}

fn escape_manifest_from_sites(workspace: &Path, sites: &[EscapeSite]) -> Vec<EscapeManifestEntry> {
    let mut map: BTreeMap<(String, String, String), EscapeManifestEntry> = BTreeMap::new();
    for site in sites {
        let file = display_relative(workspace, &site.path);
        let (class, owner) = match (site.owner, site.recovery) {
            (Some(key), _) => ("stage", Some(stage_key_display(key))),
            (None, true) => ("recovery", None),
            (None, false) => ("untagged", None),
        };
        map.entry((
            file.clone(),
            site.containing_fn.clone(),
            site.reason.clone(),
        ))
        .or_insert_with(|| EscapeManifestEntry {
            file,
            containing_fn: site.containing_fn.clone(),
            reason: site.reason.clone(),
            class: class.to_owned(),
            owner,
            count: 0,
        })
        .count += 1;
    }
    map.into_values().collect()
}

fn toml_escape_string(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn render_escape_manifest(entries: &[EscapeManifestEntry]) -> String {
    let mut out = String::from(
        "# Escape site manifest — REVIEW EVERY DIFF TO THIS FILE.\n\
         # Generated by `cargo xtask escapes --write-manifest`; verified by\n\
         # `cargo xtask escapes` (plain and --stale/ci gate runs): a scan/manifest\n\
         # mismatch fails the build. GRANULARITY: identity is (file, containing\n\
         # fn, reason) with a count — adds, removals, retags, cross-function\n\
         # moves, and count changes all land as diffs; the ACCEPTED residual is\n\
         # a same-reason site swap WITHIN one function at unchanged count\n\
         # (function-level debt tracking by design; per-site IDs were judged\n\
         # not worth the annotation churn). Line numbers deliberately omitted.\n\
         # class: stage (owner-tagged deferral) | recovery (milestone-stable\n\
         # malformed-tree guard through M7; leaves Unsupported before Done) |\n\
         # untagged (debt; 0 by M4 close —\n\
         # ratchet.toml [escapes] ceilings still apply on top).\n",
    );
    for entry in entries {
        out.push_str("\n[[site]]\n");
        out.push_str(&format!("file = \"{}\"\n", toml_escape_string(&entry.file)));
        out.push_str(&format!(
            "in = \"{}\"\n",
            toml_escape_string(&entry.containing_fn)
        ));
        out.push_str(&format!(
            "reason = \"{}\"\n",
            toml_escape_string(&entry.reason)
        ));
        out.push_str(&format!("class = \"{}\"\n", entry.class));
        if let Some(owner) = &entry.owner {
            out.push_str(&format!("owner = \"{}\"\n", toml_escape_string(owner)));
        }
        if entry.count != 1 {
            out.push_str(&format!("count = {}\n", entry.count));
        }
    }
    out
}

/// Line-based reader for the manifest's own fixed shape (the xtask
/// convention: no toml crate — see read_escapes_ceiling).
fn parse_escape_manifest(text: &str) -> Result<Vec<EscapeManifestEntry>, Box<dyn Error>> {
    fn parse_string(value: &str, line_no: usize) -> Result<String, Box<dyn Error>> {
        let value = value.trim();
        let inner = value
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .ok_or_else(|| format!("escapes.toml:{line_no}: expected a quoted string"))?;
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    other => {
                        return Err(
                            format!("escapes.toml:{line_no}: unsupported escape {other:?}").into(),
                        )
                    }
                }
            } else {
                out.push(ch);
            }
        }
        Ok(out)
    }

    let mut entries: Vec<EscapeManifestEntry> = Vec::new();
    let mut current: Option<EscapeManifestEntry> = None;
    let finish = |current: &mut Option<EscapeManifestEntry>,
                  entries: &mut Vec<EscapeManifestEntry>|
     -> Result<(), Box<dyn Error>> {
        if let Some(entry) = current.take() {
            if entry.file.is_empty()
                || entry.containing_fn.is_empty()
                || entry.reason.is_empty()
                || entry.class.is_empty()
            {
                return Err(format!(
                    "escapes.toml: incomplete [[site]] entry (file/reason/class required): \
                     {entry:?}"
                )
                .into());
            }
            entries.push(entry);
        }
        Ok(())
    };
    for (index, raw_line) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[site]]" {
            finish(&mut current, &mut entries)?;
            current = Some(EscapeManifestEntry {
                file: String::new(),
                containing_fn: String::new(),
                reason: String::new(),
                class: String::new(),
                owner: None,
                count: 1,
            });
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("escapes.toml:{line_no}: unrecognized line: {line}").into());
        };
        let entry = current
            .as_mut()
            .ok_or_else(|| format!("escapes.toml:{line_no}: key outside a [[site]] entry"))?;
        match key.trim() {
            "file" => entry.file = parse_string(value, line_no)?,
            "in" => entry.containing_fn = parse_string(value, line_no)?,
            "reason" => entry.reason = parse_string(value, line_no)?,
            "class" => entry.class = parse_string(value, line_no)?,
            "owner" => entry.owner = Some(parse_string(value, line_no)?),
            "count" => entry.count = value.trim().parse::<usize>()?,
            other => {
                return Err(format!("escapes.toml:{line_no}: unknown key {other}").into());
            }
        }
    }
    finish(&mut current, &mut entries)?;
    Ok(entries)
}

/// The manifest gate: the scan and escapes.toml must agree EXACTLY.
/// Every divergence is printed with its remedy; any divergence fails.
fn check_escape_manifest(workspace: &Path, sites: &[EscapeSite]) -> Result<(), Box<dyn Error>> {
    let manifest_path = workspace.join("escapes.toml");
    if !manifest_path.exists() {
        return Err(
            "escapes.toml is missing — run `cargo xtask escapes --write-manifest`, \
                    review the generated file, and commit it"
                .into(),
        );
    }
    let recorded = parse_escape_manifest(&fs::read_to_string(&manifest_path)?)?;
    let expected = escape_manifest_from_sites(workspace, sites);
    let key = |entry: &EscapeManifestEntry| {
        (
            entry.file.clone(),
            entry.containing_fn.clone(),
            entry.reason.clone(),
        )
    };
    let recorded_map: BTreeMap<_, _> = recorded.iter().map(|e| (key(e), e.clone())).collect();
    let expected_map: BTreeMap<_, _> = expected.iter().map(|e| (key(e), e.clone())).collect();
    let mut divergences = 0usize;
    for (k, entry) in &expected_map {
        match recorded_map.get(k) {
            None => {
                divergences += 1;
                println!(
                    "MANIFEST-NEW {} ({}): \"{}\" [{}{}] — new escape site: run \
                     `cargo xtask escapes --write-manifest` and get the diff reviewed",
                    entry.file,
                    entry.containing_fn,
                    entry.reason,
                    entry.class,
                    entry
                        .owner
                        .as_deref()
                        .map(|owner| format!(" {owner}"))
                        .unwrap_or_default(),
                );
            }
            Some(prior) if prior != entry => {
                divergences += 1;
                println!(
                    "MANIFEST-CHANGED {} ({}): \"{}\" — recorded {}/{:?}/count {}, scanned \
                     {}/{:?}/count {} — regenerate + review",
                    entry.file,
                    entry.containing_fn,
                    entry.reason,
                    prior.class,
                    prior.owner,
                    prior.count,
                    entry.class,
                    entry.owner,
                    entry.count,
                );
            }
            Some(_) => {}
        }
    }
    for (k, prior) in &recorded_map {
        if !expected_map.contains_key(k) {
            divergences += 1;
            println!(
                "MANIFEST-STALE {} ({}): \"{}\" — site no longer in the code: regenerate \
                 (retiring an escape is progress; the diff records it)",
                prior.file, prior.containing_fn, prior.reason,
            );
        }
    }
    if divergences > 0 {
        return Err(format!(
            "escape manifest out of date: {divergences} divergence(s) — \
             `cargo xtask escapes --write-manifest` + review"
        )
        .into());
    }
    Ok(())
}

fn collect_ledger_entries(workspace: &Path) -> Result<Vec<LedgerEntry>, Box<dyn Error>> {
    let mut entries = Vec::new();
    for path in collect_rs_paths(&workspace.join("crates"))? {
        let text = fs::read_to_string(&path)?;
        entries.extend(parse_ledger_entries_in_file(&path, &text)?);
    }
    entries.sort_by(|left, right| {
        left.rust_path
            .cmp(&right.rust_path)
            .then_with(|| left.rust_line.cmp(&right.rust_line))
    });
    Ok(entries)
}

fn parse_ledger_entries_in_file(
    path: &Path,
    text: &str,
) -> Result<Vec<LedgerEntry>, Box<dyn Error>> {
    let mut entries = Vec::new();
    let mut docs = Vec::<String>::new();
    let mut doc_start = 0usize;

    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim_start();
        if let Some(doc) = trimmed.strip_prefix("///") {
            if docs.is_empty() {
                doc_start = line_number;
            }
            docs.push(doc.trim().to_owned());
            continue;
        }

        if trimmed.is_empty() || trimmed.starts_with("#[") {
            continue;
        }

        if let Some(fn_name) = function_name(trimmed) {
            if let Some(entry) = parse_ledger_doc(path, doc_start, &fn_name, &docs)? {
                entries.push(entry);
            }
        }
        docs.clear();
    }

    Ok(entries)
}

fn parse_ledger_doc(
    path: &Path,
    rust_line: usize,
    rust_fn: &str,
    docs: &[String],
) -> Result<Option<LedgerEntry>, Box<dyn Error>> {
    let Some(port_line) = docs
        .iter()
        .find_map(|doc| doc.strip_prefix("tsc-port:").map(str::trim))
    else {
        return Ok(None);
    };
    let hash = docs
        .iter()
        .find_map(|doc| doc.strip_prefix("tsc-hash:").map(str::trim))
        .and_then(|value| value.split_whitespace().next())
        .ok_or_else(|| format!("{}:{rust_line} missing tsc-hash", path.display()))?;
    let span = docs
        .iter()
        .find_map(|doc| doc.strip_prefix("tsc-span:").map(str::trim))
        .ok_or_else(|| format!("{}:{rust_line} missing tsc-span", path.display()))?;
    let (port_name, version) = parse_tsc_port(port_line)
        .ok_or_else(|| format!("{}:{rust_line} malformed tsc-port", path.display()))?;
    let (span_file, span_start, span_end) = parse_tsc_span(span)
        .ok_or_else(|| format!("{}:{rust_line} malformed tsc-span", path.display()))?;

    Ok(Some(LedgerEntry {
        rust_path: path.to_owned(),
        rust_line,
        rust_fn: rust_fn.to_owned(),
        port_name,
        version,
        span_file,
        span_start,
        span_end,
        hash: hash.to_owned(),
    }))
}

fn parse_tsc_port(value: &str) -> Option<(String, String)> {
    let mut parts = value.split_whitespace();
    let name = parts.next()?.to_owned();
    let version = parts.next()?.strip_prefix('@')?.to_owned();
    Some((name, version))
}

fn parse_tsc_span(value: &str) -> Option<(String, usize, usize)> {
    let (file, range) = value.rsplit_once(':')?;
    let (start, end) = range.split_once('-')?;
    Some((file.to_owned(), start.parse().ok()?, end.parse().ok()?))
}

fn verify_ledger_entries(
    workspace: &Path,
    entries: &[LedgerEntry],
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut stale = Vec::new();
    for entry in entries {
        let actual = source_slice_hash(
            workspace,
            &entry.span_file,
            entry.span_start,
            entry.span_end,
        )?;
        if actual != entry.hash {
            stale.push(format!(
                "{}:{} {} stale: expected {} actual {}",
                display_relative(workspace, &entry.rust_path),
                entry.rust_line,
                entry.rust_fn,
                entry.hash,
                actual
            ));
        }
    }
    Ok(stale)
}

fn source_slice_hash(
    workspace: &Path,
    span_file: &str,
    start: usize,
    end: usize,
) -> Result<String, Box<dyn Error>> {
    if start == 0 || end < start {
        return Err(format!("invalid tsc span range {start}-{end}").into());
    }
    let path = ledger_source_path(workspace, span_file)?;
    let text = fs::read_to_string(&path)?;
    let lines = text.split_inclusive('\n').collect::<Vec<_>>();
    if end > lines.len() {
        return Err(format!(
            "{} has {} lines, cannot read {start}-{end}",
            path.display(),
            lines.len()
        )
        .into());
    }
    let slice = lines[start - 1..end].concat();
    Ok(sha256_hex(slice.as_bytes()))
}

fn ledger_source_path(workspace: &Path, span_file: &str) -> Result<PathBuf, Box<dyn Error>> {
    let span_path = Path::new(span_file);
    if span_path.is_absolute() && span_path.is_file() {
        return Ok(span_path.to_owned());
    }

    let mut candidates = vec![
        workspace
            .join("vendor/typescript-6.0.3/src/compiler")
            .join(span_file),
        workspace
            .join("vendor/typescript-6.0.3/lib")
            .join(span_file),
    ];
    if let Some(parent) = workspace.parent() {
        candidates.push(parent.join("ts-tests/src/compiler").join(span_file));
        candidates.push(parent.join(span_file));
    }
    candidates.push(workspace.join(span_file));

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| format!("missing ledger source file: {span_file}").into())
}

/// Function-level DISPOSITION census (external review item #5): every
/// checker pub/pub(crate) fn must say where it came from — the
/// existing `tsc-port` ledger header family (incl. tsc-hash/tsc-span
/// partials), `tsrs-native` (Rust-side glue with no tsc counterpart:
/// arenas, links accessors, harness plumbing), `tsc-deferred`
/// M5|M6|M7|M8 (the WHOLE fn is a later stage's port; finer-grained
/// deferral stays with escapes), or `tsc-not-applicable` with a
/// reason (LSP-only/emit-only surfaces). Rust-side accountability
/// only: the "missing tsc function" direction is the M8
/// emitter-inventory + dependency closure. The backlog is the
/// fn-dispositions.toml EQUALITY allowlist (deletions only; empties
/// before M8 starts per definition-of-done.md).
fn fn_disposition_markers() -> [&'static str; 4] {
    // concat! keeps the contiguous marker tokens out of THIS file's
    // source text (the ledger-entry scanner walks xtask too and would
    // otherwise read the literals as headerless port entries).
    // tsc-hash:/tsc-span: are deliberately NOT accepted alone (review
    // round 3): the ledger parser keys entries on the port header, so
    // a bare hash/span line would satisfy the census while evading
    // ledger validation entirely — only the port header counts, and
    // ledger check owns the hash/span completeness of its block.
    [
        concat!("tsc-", "port:"),
        concat!("tsrs-", "native:"),
        concat!("tsc-", "deferred:"),
        concat!("tsc-", "not-applicable:"),
    ]
}

/// A line carries a disposition only when it is a `///` DOC comment
/// (plain `//` comments are rejected — review round 4: the
/// ledger-entry collector reads /// blocks alone, so a plain-comment
/// `// tsc-port: …` would satisfy the census while evading the
/// hash/span validation entirely), the marker STARTS the content
/// (prose mentions don't count), and its payload validates: tsc-port
/// needs no payload here (ledger check owns its block's hash/span
/// completeness); tsrs-native/tsc-not-applicable need a non-empty
/// reason; tsc-deferred must name its owner milestone as a WHOLE
/// WORD (M5-M8; "M50" does not pass).
fn line_is_valid_disposition(line: &str) -> bool {
    let trimmed = line.trim_start();
    // `////…` banner lines are not doc comments; reject them along
    // with plain `//`.
    let Some(after) = trimmed.strip_prefix("///") else {
        return false;
    };
    if after.starts_with('/') {
        return false;
    }
    let content = after.trim_start();
    let [port, native, deferred, not_applicable] = fn_disposition_markers();
    if content.starts_with(port) {
        return true;
    }
    for marker in [native, not_applicable] {
        if let Some(tail) = content.strip_prefix(marker) {
            return !tail.trim().is_empty();
        }
    }
    if let Some(tail) = content.strip_prefix(deferred) {
        let tail = tail.trim_start();
        return ["M5", "M6", "M7", "M8"].iter().any(|stage| {
            tail.strip_prefix(stage).is_some_and(|rest| {
                rest.chars()
                    .next()
                    .is_none_or(|ch| !ch.is_ascii_alphanumeric())
            })
        });
    }
    false
}

/// Mirrors parse_ledger_entries_in_file's doc-block rules EXACTLY,
/// so a disposition can never be visible to the census yet invisible
/// to the ledger parser (review round 5: a plain `//` line between
/// the doc block and the fn CLEARS the block on the ledger side —
/// walking over it here let a detached `/// tsc-port:` satisfy the
/// census while evading hash/span validation). Upward from the fn:
/// `///` doc lines accumulate, blank lines and `#[` attributes are
/// transparent, ANYTHING else — including plain `//` comments —
/// terminates the block. Keep the two in lockstep: a rule change in
/// either MUST land in both.
fn doc_block_has_disposition(lines: &[&str], fn_index: usize) -> bool {
    let mut index = fn_index;
    while index > 0 {
        let line = lines[index - 1].trim_start();
        if line.starts_with("///") {
            if line_is_valid_disposition(line) {
                return true;
            }
            index -= 1;
        } else if line.is_empty() || line.starts_with("#[") {
            index -= 1;
        } else {
            break;
        }
    }
    false
}

fn collect_undispositioned_checker_fns(
    workspace: &Path,
) -> Result<Vec<PublicFunction>, Box<dyn Error>> {
    let mut functions = Vec::new();
    for path in collect_rs_paths(&workspace.join("crates/checker/src"))? {
        let text = fs::read_to_string(&path)?;
        let lines = text.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            let is_pub_fn = ["pub fn ", "pub async fn ", "pub const fn "]
                .iter()
                .any(|prefix| trimmed.starts_with(prefix))
                || ["fn ", "async fn ", "const fn "].iter().any(|suffix| {
                    trimmed
                        .strip_prefix("pub(crate) ")
                        .is_some_and(|rest| rest.starts_with(suffix))
                });
            if !is_pub_fn {
                continue;
            }
            if doc_block_has_disposition(&lines, index) {
                continue;
            }
            if let Some(name) = function_name(trimmed) {
                functions.push(PublicFunction {
                    path: path.clone(),
                    line: index + 1,
                    name,
                });
            }
        }
    }
    functions.sort();
    Ok(functions)
}

/// The disposition BACKLOG allowlist (fn-dispositions.toml): the
/// pre-existing undispositioned fns, keyed (file, name) with a count
/// for same-named impl-block collisions. The gate is EQUALITY: a
/// scanned undispositioned fn absent from the file is a NEW
/// undispositioned identity (forbidden — annotate it instead); a
/// listed identity no longer undispositioned is STALE (progress —
/// regenerate so the burn-down lands as a reviewable diff). The file
/// only ever shrinks toward 0 before M8 (definition-of-done.md).
fn backlog_map(
    functions: &[PublicFunction],
    workspace: &Path,
) -> BTreeMap<(String, String), usize> {
    let mut map = BTreeMap::new();
    for function in functions {
        *map.entry((
            display_relative(workspace, &function.path),
            function.name.clone(),
        ))
        .or_insert(0) += 1;
    }
    map
}

fn render_fn_backlog(map: &BTreeMap<(String, String), usize>) -> String {
    let mut out = String::from(
        "# fn-disposition BACKLOG — pre-existing checker pub fns without a\n\
         # disposition header. DELETIONS ONLY: annotate a fn (tsc-port family /\n\
         # tsrs-native: <reason> / tsc-deferred: M5-M8 / tsc-not-applicable:\n\
         # <reason>), then `cargo xtask ledger write-backlog` — the shrinking\n\
         # diff is the review surface. New undispositioned fns are rejected by\n\
         # `cargo xtask ledger check`. Identity is (file, fn name) with a count\n\
         # for same-named impl-block fns — function-level tracking, same\n\
         # accepted residual as escapes.toml. Reaches 0 before M8 starts\n\
         # (definition-of-done.md clause 4).\n",
    );
    for ((file, name), count) in map {
        out.push_str("\n[[fn]]\n");
        out.push_str(&format!("file = \"{}\"\n", toml_escape_string(file)));
        out.push_str(&format!("name = \"{}\"\n", toml_escape_string(name)));
        if *count != 1 {
            out.push_str(&format!("count = {count}\n"));
        }
    }
    out
}

fn parse_fn_backlog(text: &str) -> Result<BTreeMap<(String, String), usize>, Box<dyn Error>> {
    let mut map = BTreeMap::new();
    let mut file = String::new();
    let mut name = String::new();
    let mut count = 1usize;
    let mut open = false;
    let flush = |file: &mut String,
                 name: &mut String,
                 count: &mut usize,
                 open: &mut bool,
                 map: &mut BTreeMap<(String, String), usize>|
     -> Result<(), Box<dyn Error>> {
        if *open {
            if file.is_empty() || name.is_empty() {
                return Err("fn-dispositions.toml: incomplete [[fn]] entry".into());
            }
            map.insert((std::mem::take(file), std::mem::take(name)), *count);
            *count = 1;
        }
        *open = true;
        Ok(())
    };
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[fn]]" {
            flush(&mut file, &mut name, &mut count, &mut open, &mut map)?;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("fn-dispositions.toml: unrecognized line: {line}").into());
        };
        let value = value
            .trim()
            .trim_matches('"')
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
        match key.trim() {
            "file" => file = value,
            "name" => name = value,
            "count" => count = value.parse()?,
            other => return Err(format!("fn-dispositions.toml: unknown key {other}").into()),
        }
    }
    if open {
        if file.is_empty() || name.is_empty() {
            return Err("fn-dispositions.toml: incomplete [[fn]] entry".into());
        }
        map.insert((file, name), count);
    }
    Ok(map)
}

fn collect_hot_public_functions(workspace: &Path) -> Result<Vec<PublicFunction>, Box<dyn Error>> {
    let hot_files = [
        workspace.join("crates/checker/src/lib.rs"),
        workspace.join("crates/binder/src/lib.rs"),
        workspace.join("crates/syntax/src/lib.rs"),
        workspace.join("crates/syntax/src/for_each_child.rs"),
        workspace.join("crates/syntax/src/scanner.rs"),
    ];
    let mut functions = Vec::new();
    for path in hot_files {
        if !path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if is_public_fn_line(trimmed) {
                if let Some(name) = function_name(trimmed) {
                    functions.push(PublicFunction {
                        path: path.clone(),
                        line: index + 1,
                        name,
                    });
                }
            }
        }
    }
    functions.sort();
    Ok(functions)
}

fn unported_public_functions(
    entries: &[LedgerEntry],
    public_functions: &[PublicFunction],
) -> Vec<PublicFunction> {
    let ported = entries
        .iter()
        .map(|entry| (entry.rust_path.clone(), entry.rust_fn.clone()))
        .collect::<BTreeSet<_>>();
    public_functions
        .iter()
        .filter(|function| !ported.contains(&(function.path.clone(), function.name.clone())))
        .cloned()
        .collect()
}

fn collect_todo_port_sites(workspace: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let mut sites = Vec::new();
    for path in collect_rs_paths(&workspace.join("crates"))? {
        if path
            .strip_prefix(workspace)
            .is_ok_and(|relative| relative.starts_with("crates/xtask"))
        {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        for (index, line) in text.lines().enumerate() {
            if line.contains("todo_port!(") {
                sites.push(format!(
                    "{}:{}",
                    display_relative(workspace, &path),
                    index + 1
                ));
            }
        }
    }
    sites.sort();
    Ok(sites)
}

fn collect_rs_paths(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut stack = vec![root.to_owned()];
    let mut paths = Vec::new();
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) != Some("target") {
                    stack.push(path);
                }
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn is_public_fn_line(line: &str) -> bool {
    line.starts_with("pub fn ") || line.starts_with("pub async fn ")
}

fn function_name(line: &str) -> Option<String> {
    let fn_start = line.find("fn ")? + "fn ".len();
    let rest = &line[fn_start..];
    let name = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn base64_decode_to_string(input: &str) -> Result<String, Box<dyn Error>> {
    let bytes = input.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err("invalid base64 length".into());
    }

    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks_exact(4) {
        let chunk = [chunk[0], chunk[1], chunk[2], chunk[3]];
        decode_base64_chunk(&chunk, &mut out)?;
    }
    Ok(String::from_utf8(out)?)
}

fn decode_base64_chunk(chunk: &[u8; 4], out: &mut Vec<u8>) -> Result<(), Box<dyn Error>> {
    let pad = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
    if pad > 2 {
        return Err("invalid base64 padding".into());
    }

    let first = decode_base64_value(chunk[0])?;
    let second = decode_base64_value(chunk[1])?;
    let third = if chunk[2] == b'=' {
        0
    } else {
        decode_base64_value(chunk[2])?
    };
    let fourth = if chunk[3] == b'=' {
        0
    } else {
        decode_base64_value(chunk[3])?
    };

    out.push((first << 2) | (second >> 4));
    if pad < 2 {
        out.push((second << 4) | (third >> 2));
    }
    if pad < 1 {
        out.push((third << 6) | fourth);
    }
    Ok(())
}

fn decode_base64_value(byte: u8) -> Result<u8, Box<dyn Error>> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(format!("invalid base64 byte: {byte}").into()),
    }
}

fn ci(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut baseline = "origin/main".to_owned();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--baseline" => {
                baseline = args.next().ok_or("missing value after --baseline")?;
            }
            _ => return Err(format!("unexpected ci argument: {arg}").into()),
        }
    }

    run_command(
        Command::new("cargo")
            .arg("fmt")
            .arg("--all")
            .arg("--")
            .arg("--check"),
    )?;
    run_command(
        Command::new("cargo")
            .arg("clippy")
            .arg("--workspace")
            .arg("--all-targets")
            .arg("--")
            .arg("-D")
            .arg("warnings"),
    )?;
    run_command(Command::new("cargo").arg("build").arg("--workspace"))?;
    run_command(Command::new("cargo").arg("test").arg("--workspace"))?;
    run_command(
        Command::new("cargo")
            .arg("xtask")
            .arg("codegen")
            .arg("band-inventory")
            .arg("--by-function")
            .arg("--band")
            .arg("all")
            .arg("--check"),
    )?;
    run_command(Command::new("cargo").arg("xtask").arg("relpin").arg("run"))?;
    // Parse+bind smoke over the full corpus (~1s): the cheap panic
    // net for the parser/binder invariants the 5.9a dead-guard
    // conversions lean on (m4-end-sweep-steps.md dead-guard policy).
    run_command(Command::new("cargo").arg("xtask").arg("bind-corpus"))?;
    // A1 accepted-state coherence: artifact/inputs/lineage verify
    // before the behavior runs that gate against them. Hosted PR CI
    // supplies GitHub's immutable base SHA; local runs default to the
    // origin/main convenience ref. The direct compare prevents a
    // rewritten branch from replacing the accepted set with a smaller
    // self-consistent chain.
    run_command(
        Command::new("cargo")
            .arg("xtask")
            .arg("ratchet")
            .arg("check")
            .arg("--baseline")
            .arg(&baseline),
    )?;
    // A2 exact scope coherence: manifest identities, encoder
    // cross-check, snapshot anchors, and tombstone proofs verify
    // against the same trusted base before the supported view that
    // depends on them gates anything.
    run_command(
        Command::new("cargo")
            .arg("xtask")
            .arg("scope")
            .arg("audit")
            .arg("--baseline")
            .arg(&baseline),
    )?;
    // A5 family-map coherence: the exactly-once (code, pass) domain,
    // freeze/extension anchors, and the trusted-base compare — before
    // the rollup below reads the map as a verified input.
    run_command(
        Command::new("cargo")
            .arg("xtask")
            .arg("families")
            .arg("check")
            .arg("--baseline")
            .arg(&baseline),
    )?;
    run_command(Command::new("cargo").arg("xtask").arg("conformance"))?;
    run_command(
        Command::new("cargo")
            .arg("xtask")
            .arg("conformance")
            .arg("--band")
            .arg("2xxx"),
    )?;
    // The permanent syntactic gate (convergence invariant 3): parser
    // fidelity is ratcheted independently on every merge so a
    // semantic gain can never hide a syntactic regression.
    run_command(
        Command::new("cargo")
            .arg("xtask")
            .arg("conformance")
            .arg("--syntactic-only"),
    )?;
    // A5 rollup: one more full band=all gating run that collects the
    // per-bucket observation and writes the per-family supported
    // grading (report-only; the set ratchet and FP=0 gate the run).
    run_command(
        Command::new("cargo")
            .arg("xtask")
            .arg("families")
            .arg("report"),
    )?;
    run_command(
        Command::new("cargo")
            .arg("xtask")
            .arg("invariants")
            .arg("--suite")
            .arg("all"),
    )?;
    run_command(
        Command::new("cargo")
            .arg("xtask")
            .arg("ledger")
            .arg("check"),
    )?;
    // The expiry audit: escapes whose owner stage (per the STAGE
    // marker file) has passed must be implemented or re-marked.
    let stage = fs::read_to_string(find_tsrs2_root()?.join("STAGE"))?;
    run_command(
        Command::new("cargo")
            .arg("xtask")
            .arg("escapes")
            .arg("--stale")
            .arg(stage.trim()),
    )?;
    Ok(())
}

fn run_command(command: &mut Command) -> Result<(), Box<dyn Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("command failed with status {status:?}").into());
    }
    Ok(())
}

fn oracle_smoke(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut limit = 100usize;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--limit" => {
                let value = args.next().ok_or("missing value after --limit")?;
                limit = value.parse()?;
            }
            _ => return Err(format!("unexpected oracle-smoke argument: {arg}").into()),
        }
    }

    let workspace = find_tsrs2_root()?;
    let fixtures_root = workspace.join("ts-tests/tests/cases/conformance");
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let temp_root = std::env::temp_dir().join(format!("tsrs2-oracle-smoke-{}", std::process::id()));
    if temp_root.exists() {
        fs::remove_dir_all(&temp_root)?;
    }
    fs::create_dir_all(&temp_root)?;

    let mut fixtures = collect_fixture_paths(&fixtures_root)?;
    fixtures.sort();
    fixtures.truncate(limit);

    let pool = tsrs2_oracle::OraclePool::new(tsrs2_oracle::OraclePool::default_size())?;
    let mut program_count = 0usize;
    for (index, fixture) in fixtures.iter().enumerate() {
        let programs = tsrs2_harness::expand_fixture_file(fixture, &vendor_lib_dir)?;
        let out_dir = temp_root.join(index.to_string());
        let paths = tsrs2_harness::write_program_jsons(&programs, &out_dir)?;
        for path in paths {
            let first = pool.diagnostics(&path)?;
            let second = pool.diagnostics(&path)?;
            if first != second {
                return Err(
                    format!("oracle output changed between runs for {}", path.display()).into(),
                );
            }
            program_count += 1;
        }
    }

    fs::remove_dir_all(&temp_root)?;
    println!(
        "oracle smoke passed: {} fixtures, {} program.json files",
        fixtures.len(),
        program_count
    );
    Ok(())
}

fn collect_fixture_paths(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut stack = vec![root.to_owned()];
    let mut fixtures = Vec::new();
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if is_fixture_path(&path) {
                fixtures.push(path);
            }
        }
    }
    Ok(fixtures)
}

fn is_fixture_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "js" | "jsx")
    )
}

fn run_or_exit(result: Result<(), Box<dyn Error>>) {
    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn scaffold_smoke() {
    let harness_diags = tsrs2_harness::check_empty_program().diagnostics.len();
    let conformance_diags = tsrs2_conformance::run_empty_engine_smoke();

    if harness_diags != 0 || conformance_diags != 0 {
        eprintln!("empty-engine scaffold emitted diagnostics");
        std::process::exit(1);
    }

    println!("tsrs2 scaffold ready");
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EnumMember {
    name: String,
    value: EnumValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum EnumValue {
    Int(i32),
    Str(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EnumTable {
    name: String,
    members: Vec<EnumMember>,
}

#[derive(Clone, Copy)]
struct SourceEnum {
    name: &'static str,
    file: &'static str,
}

const RUNTIME_ENUMS: &[&str] = &[
    "SyntaxKind",
    "NodeFlags",
    "ModifierFlags",
    "RelationComparisonResult",
    "FlowFlags",
    "SymbolFlags",
    "TypeFlags",
    "ObjectFlags",
    "SignatureFlags",
    "DiagnosticCategory",
    "ModuleKind",
    "TypeFacts",
    "CheckMode",
];

const CONST_ENUMS: &[SourceEnum] = &[
    SourceEnum {
        name: "TokenFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "UnionReduction",
        file: "types.ts",
    },
    SourceEnum {
        name: "ContextFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "CheckFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "InternalSymbolName",
        file: "types.ts",
    },
    SourceEnum {
        name: "ElementFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "AccessFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "TypeMapKind",
        file: "types.ts",
    },
    SourceEnum {
        name: "InferencePriority",
        file: "types.ts",
    },
    SourceEnum {
        name: "InferenceFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "Ternary",
        file: "types.ts",
    },
    SourceEnum {
        name: "ScriptTarget",
        file: "types.ts",
    },
    SourceEnum {
        name: "CharacterCodes",
        file: "types.ts",
    },
    SourceEnum {
        name: "VarianceFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "IndexFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "SignatureKind",
        file: "types.ts",
    },
    SourceEnum {
        name: "MemberOverrideStatus",
        file: "types.ts",
    },
    SourceEnum {
        name: "NodeCheckFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "TypeSystemPropertyName",
        file: "checker.ts",
    },
    SourceEnum {
        name: "WideningKind",
        file: "checker.ts",
    },
    SourceEnum {
        name: "IterationUse",
        file: "checker.ts",
    },
    SourceEnum {
        name: "IterationTypeKind",
        file: "checker.ts",
    },
    SourceEnum {
        name: "IntersectionState",
        file: "checker.ts",
    },
    SourceEnum {
        name: "RecursionFlags",
        file: "checker.ts",
    },
    SourceEnum {
        name: "ExpandingFlags",
        file: "checker.ts",
    },
    SourceEnum {
        name: "ParsingContext",
        file: "parser.ts",
    },
];

fn codegen_enums(check: bool) -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let tsc_path = workspace.join("vendor/typescript-6.0.3/lib/_tsc.js");
    let tsc = fs::read_to_string(&tsc_path)?;

    let mut runtime_tables = BTreeMap::new();
    for name in RUNTIME_ENUMS {
        let table = parse_runtime_enum(&tsc, name)?;
        runtime_tables.insert((*name).to_owned(), table);
    }

    let mut source_tables = BTreeMap::new();
    for source in CONST_ENUMS {
        let path = compiler_source_path(&workspace, source.file)?;
        let text = fs::read_to_string(path)?;
        let table = parse_source_enum(&text, source.name)?;
        source_tables.insert(source.name.to_owned(), table);
    }

    let syntax = runtime_tables
        .remove("SyntaxKind")
        .ok_or("missing generated SyntaxKind")?;
    let kind_rs = rustfmt_text(&render_syntax_kind(&syntax)?)?;

    let mut flags_tables: Vec<EnumTable> = runtime_tables.into_values().collect();
    flags_tables.extend(source_tables.into_values());
    flags_tables.sort_by(|a, b| a.name.cmp(&b.name));
    let flags_rs = rustfmt_text(&render_flags(&flags_tables)?)?;

    let kind_path = workspace.join("crates/syntax/src/kind.rs");
    let flags_path = workspace.join("crates/types/src/flags.rs");
    write_generated(&kind_path, &kind_rs, check)?;
    write_generated(&flags_path, &flags_rs, check)?;

    if check {
        println!("generated enum files are up to date");
    } else {
        println!("generated enum files");
    }

    Ok(())
}

fn codegen_scanner(check: bool) -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let tsc_path = workspace.join("vendor/typescript-6.0.3/lib/_tsc.js");
    let tsc = fs::read_to_string(&tsc_path)?;

    let identifier_start = parse_unicode_range_pairs(&tsc, "unicodeESNextIdentifierStart")?;
    let identifier_part = parse_unicode_range_pairs(&tsc, "unicodeESNextIdentifierPart")?;
    let keywords = parse_text_to_keyword_obj(&tsc)?;

    let chars_rs = rustfmt_text(&render_scanner_chars_rs(
        &identifier_start,
        &identifier_part,
    )?)?;
    let keywords_rs = rustfmt_text(&render_scanner_keywords_rs(&keywords)?)?;
    let punctuation = parse_text_to_token_punctuation(&tsc)?;
    let tokens_rs = rustfmt_text(&render_scanner_tokens_rs(&keywords, &punctuation)?)?;

    write_generated(
        &workspace.join("crates/syntax/src/chars.rs"),
        &chars_rs,
        check,
    )?;
    write_generated(
        &workspace.join("crates/syntax/src/keywords.rs"),
        &keywords_rs,
        check,
    )?;
    write_generated(
        &workspace.join("crates/syntax/src/tokens.rs"),
        &tokens_rs,
        check,
    )?;

    if check {
        println!("generated scanner files are up to date");
    } else {
        println!("generated scanner files");
    }

    Ok(())
}

fn parse_unicode_range_pairs(tsc: &str, name: &str) -> Result<Vec<(u32, u32)>, Box<dyn Error>> {
    let values = parse_js_number_array(tsc, name)?;
    if values.len() % 2 != 0 {
        return Err(format!("{name} has an odd number of range endpoints").into());
    }

    let mut ranges = Vec::with_capacity(values.len() / 2);
    for pair in values.chunks_exact(2) {
        ranges.push((pair[0], pair[1]));
    }
    Ok(ranges)
}

fn parse_js_number_array(tsc: &str, name: &str) -> Result<Vec<u32>, Box<dyn Error>> {
    let marker = format!("var {name} = [");
    let start = tsc
        .find(&marker)
        .ok_or_else(|| format!("missing scanner array: {name}"))?
        + marker.len();
    let rest = &tsc[start..];
    let end = rest
        .find("];")
        .ok_or_else(|| format!("unterminated scanner array: {name}"))?;

    rest[..end]
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(parse_js_number)
        .collect()
}

fn parse_js_number(value: &str) -> Result<u32, Box<dyn Error>> {
    if value.contains('e') || value.contains('E') {
        let parsed = value.parse::<f64>()?;
        if parsed.fract() != 0.0 || parsed < 0.0 || parsed > u32::MAX as f64 {
            return Err(format!("non-integer JavaScript number literal: {value}").into());
        }
        Ok(parsed as u32)
    } else {
        Ok(value.parse()?)
    }
}

/// The punctuation half of `textToToken` (8117): the entries spread
/// after `...textToKeywordObj`. Keys are quoted strings (`"{": 19`).
fn parse_text_to_token_punctuation(tsc: &str) -> Result<Vec<(String, u16)>, Box<dyn Error>> {
    let marker = "var textToToken = new Map(Object.entries({";
    let start = tsc.find(marker).ok_or("missing textToToken")? + marker.len();
    let rest = &tsc[start..];
    let end = rest.find("\n}));").ok_or("unterminated textToToken")?;

    let mut entries = Vec::new();
    for line in rest[..end].lines() {
        let line = line.trim().trim_end_matches(',');
        if line.is_empty() || line.starts_with("...") {
            continue;
        }
        let quoted = line
            .strip_prefix('"')
            .ok_or_else(|| format!("malformed textToToken entry: {line}"))?;
        let close = quoted
            .find('"')
            .ok_or_else(|| format!("malformed textToToken entry: {line}"))?;
        let key = quoted[..close].to_owned();
        let value = quoted[close + 1..]
            .trim_start_matches(':')
            .split_whitespace()
            .next()
            .ok_or_else(|| format!("missing textToToken value: {line}"))?
            .parse()?;
        entries.push((key, value));
    }
    Ok(entries)
}

/// tokenStrings = makeReverseMap(textToToken) (8239): value→text,
/// insertion order, last write wins (no duplicate values exist).
fn render_scanner_tokens_rs(
    keywords: &[(String, u16)],
    punctuation: &[(String, u16)],
) -> Result<String, Box<dyn Error>> {
    let mut reverse: Vec<(u16, String)> = Vec::new();
    for (text, value) in keywords.iter().chain(punctuation) {
        if let Some(entry) = reverse.iter_mut().find(|(v, _)| v == value) {
            entry.1 = text.clone();
        } else {
            reverse.push((*value, text.clone()));
        }
    }
    reverse.sort_by_key(|(value, _)| *value);

    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen scanner`. Do not edit by hand.\n"
    )?;
    writeln!(out, "use crate::SyntaxKind;\n")?;
    writeln!(
        out,
        "/// tsc tokenToString (8240): tokenStrings reverse map of textToToken."
    )?;
    writeln!(
        out,
        "pub fn token_to_string(kind: SyntaxKind) -> Option<&'static str> {{"
    )?;
    writeln!(out, "    Some(match kind.value() {{")?;
    for (value, text) in &reverse {
        writeln!(out, "        {value} => \"{}\",", text.escape_default())?;
    }
    writeln!(out, "        _ => return None,")?;
    writeln!(out, "    }})")?;
    writeln!(out, "}}")?;
    Ok(out)
}

fn parse_text_to_keyword_obj(tsc: &str) -> Result<Vec<(String, u16)>, Box<dyn Error>> {
    let marker = "var textToKeywordObj = {";
    let start = tsc.find(marker).ok_or("missing textToKeywordObj")? + marker.len();
    let rest = &tsc[start..];
    let end = rest.find("\n};").ok_or("unterminated textToKeywordObj")?;

    let mut keywords = Vec::new();
    for line in rest[..end].lines() {
        let line = line.trim().trim_end_matches(',');
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| format!("malformed keyword entry: {line}"))?;
        let key = parse_keyword_key(key.trim())?;
        let value = value
            .split_whitespace()
            .next()
            .ok_or_else(|| format!("missing keyword value: {line}"))?
            .parse()?;
        keywords.push((key, value));
    }
    keywords.sort_by(|(left, _), (right, _)| left.cmp(right));
    Ok(keywords)
}

fn parse_keyword_key(key: &str) -> Result<String, Box<dyn Error>> {
    if let Some(quoted) = key
        .strip_prefix("[\"")
        .and_then(|key| key.strip_suffix("\"]"))
    {
        Ok(quoted.to_owned())
    } else if key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        Ok(key.to_owned())
    } else {
        Err(format!("malformed keyword key: {key}").into())
    }
}

fn render_scanner_chars_rs(
    identifier_start: &[(u32, u32)],
    identifier_part: &[(u32, u32)],
) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen scanner`. Do not edit by hand.\n"
    )?;
    writeln!(
        out,
        "pub(crate) fn is_identifier_start(ch: char) -> bool {{"
    )?;
    writeln!(out, "    ch.is_ascii_alphabetic()")?;
    writeln!(out, "        || ch == '$'")?;
    writeln!(out, "        || ch == '_'")?;
    writeln!(
        out,
        "        || (ch as u32 > 0x7f && lookup_in_unicode_map(ch as u32, UNICODE_ES_NEXT_IDENTIFIER_START))"
    )?;
    writeln!(out, "}}\n")?;

    writeln!(out, "pub(crate) fn is_identifier_part(ch: char) -> bool {{")?;
    writeln!(out, "    ch.is_ascii_alphanumeric()")?;
    writeln!(out, "        || ch == '$'")?;
    writeln!(out, "        || ch == '_'")?;
    writeln!(
        out,
        "        || (ch as u32 > 0x7f && lookup_in_unicode_map(ch as u32, UNICODE_ES_NEXT_IDENTIFIER_PART))"
    )?;
    writeln!(out, "}}\n")?;

    writeln!(
        out,
        "fn lookup_in_unicode_map(code: u32, map: &[(u32, u32)]) -> bool {{"
    )?;
    writeln!(out, "    map.binary_search_by(|&(start, end)| {{")?;
    writeln!(out, "        if code < start {{")?;
    writeln!(out, "            std::cmp::Ordering::Greater")?;
    writeln!(out, "        }} else if code > end {{")?;
    writeln!(out, "            std::cmp::Ordering::Less")?;
    writeln!(out, "        }} else {{")?;
    writeln!(out, "            std::cmp::Ordering::Equal")?;
    writeln!(out, "        }}")?;
    writeln!(out, "    }})")?;
    writeln!(out, "    .is_ok()")?;
    writeln!(out, "}}\n")?;

    render_range_table(
        &mut out,
        "UNICODE_ES_NEXT_IDENTIFIER_START",
        identifier_start,
    )?;
    writeln!(out)?;
    render_range_table(&mut out, "UNICODE_ES_NEXT_IDENTIFIER_PART", identifier_part)?;
    Ok(out)
}

fn render_range_table(
    out: &mut String,
    name: &str,
    ranges: &[(u32, u32)],
) -> Result<(), Box<dyn Error>> {
    writeln!(out, "const {name}: &[(u32, u32)] = &[")?;
    for chunk in ranges.chunks(4) {
        write!(out, "    ")?;
        for (index, (start, end)) in chunk.iter().enumerate() {
            if index > 0 {
                write!(out, ", ")?;
            }
            write!(out, "({start}, {end})")?;
        }
        writeln!(out, ",")?;
    }
    writeln!(out, "];")?;
    Ok(())
}

fn render_scanner_keywords_rs(keywords: &[(String, u16)]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen scanner`. Do not edit by hand.\n"
    )?;
    writeln!(out, "use crate::SyntaxKind;\n")?;
    writeln!(
        out,
        "pub(crate) fn keyword_kind(text: &str) -> Option<SyntaxKind> {{"
    )?;
    writeln!(out, "    let kind = match text {{")?;
    for (keyword, value) in keywords {
        writeln!(out, "        \"{keyword}\" => {value},")?;
    }
    writeln!(out, "        _ => return None,")?;
    writeln!(out, "    }};")?;
    writeln!(out, "    SyntaxKind::from_u16(kind)")?;
    writeln!(out, "}}")?;
    Ok(out)
}

fn find_tsrs2_root() -> Result<PathBuf, Box<dyn Error>> {
    let cwd = std::env::current_dir()?;
    for dir in cwd.ancestors() {
        if dir.join("vendor/typescript-6.0.3/lib/_tsc.js").is_file() {
            return Ok(dir.to_owned());
        }

        let nested = dir.join("tsrs2");
        if nested.join("vendor/typescript-6.0.3/lib/_tsc.js").is_file() {
            return Ok(nested);
        }
    }

    Err("could not find tsrs2 workspace root".into())
}

fn compiler_source_path(workspace: &Path, file: &str) -> Result<PathBuf, Box<dyn Error>> {
    let vendored = workspace
        .join("vendor/typescript-6.0.3/src/compiler")
        .join(file);
    if vendored.is_file() {
        return Ok(vendored);
    }

    let checkout = workspace
        .parent()
        .ok_or("tsrs2 workspace has no parent")?
        .join("ts-tests/src/compiler")
        .join(file);
    if checkout.is_file() {
        return Ok(checkout);
    }

    Err(format!("missing TypeScript compiler source file for const enum extraction: {file}").into())
}

fn write_generated(path: &Path, text: &str, check: bool) -> Result<(), Box<dyn Error>> {
    if check {
        let current = fs::read_to_string(path)?;
        if current != text {
            return Err(format!("{} is not up to date", path.display()).into());
        }
    } else {
        fs::write(path, text)?;
    }
    Ok(())
}

fn rustfmt_text(text: &str) -> Result<String, Box<dyn Error>> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2021", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    child
        .stdin
        .as_mut()
        .ok_or("failed to open rustfmt stdin")?
        .write_all(text.as_bytes())?;

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!(
            "rustfmt failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn parse_runtime_enum(tsc: &str, enum_name: &str) -> Result<EnumTable, Box<dyn Error>> {
    let start_marker = format!("var {enum_name} = /* @__PURE__ */ ((");
    let start = tsc
        .find(&start_marker)
        .ok_or_else(|| format!("runtime enum {enum_name} not found in _tsc.js"))?;
    let after_start = &tsc[start..];
    let end = after_start
        .find(&format!("return {enum_name}"))
        .ok_or_else(|| format!("runtime enum {enum_name} has no return sentinel"))?;
    let block = &after_start[..end];

    let mut members = Vec::new();
    for line in block.lines() {
        if let Some(member) = parse_runtime_member(line)? {
            members.push(member);
        }
    }

    if members.is_empty() {
        return Err(format!("runtime enum {enum_name} had no members").into());
    }

    Ok(EnumTable {
        name: enum_name.to_owned(),
        members,
    })
}

fn parse_runtime_member(line: &str) -> Result<Option<EnumMember>, Box<dyn Error>> {
    let Some(name_marker_start) = line.find("[\"") else {
        return Ok(None);
    };
    let name_start = name_marker_start + 2;
    let name_end = line[name_start..]
        .find("\"]")
        .map(|offset| name_start + offset)
        .ok_or_else(|| format!("malformed runtime enum line: {line}"))?;
    let name = &line[name_start..name_end];

    let after_name = &line[name_end + 2..];
    let equals = after_name
        .find('=')
        .ok_or_else(|| format!("runtime enum member has no value: {line}"))?;
    let value_text = after_name[equals + 1..].trim_start();
    // JS emits large round enum initializers in scientific notation
    // (TypeFacts.FunctionFacts = 16728e3) — include the exponent in
    // the value token.
    let value_end = value_text
        .char_indices()
        .find_map(|(idx, ch)| {
            if (idx == 0 && ch == '-') || ch.is_ascii_digit() || ch == 'e' || ch == 'E' {
                None
            } else {
                Some(idx)
            }
        })
        .unwrap_or(value_text.len());
    let raw = &value_text[..value_end];
    let value: i32 = if raw.contains(['e', 'E']) {
        let parsed = raw.parse::<f64>()?;
        if parsed.fract() != 0.0 || parsed < i32::MIN as f64 || parsed > i32::MAX as f64 {
            return Err(format!("non-integer runtime enum value: {line}").into());
        }
        parsed as i32
    } else {
        raw.parse()?
    };

    Ok(Some(EnumMember {
        name: name.to_owned(),
        value: EnumValue::Int(value),
    }))
}

fn parse_source_enum(source: &str, enum_name: &str) -> Result<EnumTable, Box<dyn Error>> {
    let block = source_enum_block(source, enum_name)?;
    let mut values = BTreeMap::<String, EnumValue>::new();
    let mut members = Vec::new();
    let mut next_auto_int = Some(0i32);
    let mut in_block_comment = false;

    for raw_line in block.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if in_block_comment {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].trim();
                in_block_comment = false;
            } else {
                continue;
            }
        }

        while line.starts_with("/*") {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].trim();
            } else {
                in_block_comment = true;
                line = "";
                break;
            }
        }

        if line.is_empty() || line.starts_with('*') || line.starts_with("//") {
            continue;
        }

        let without_comment = strip_line_comment(line);
        let mut entry = without_comment.trim().trim_end_matches(',').trim();
        if entry.is_empty() {
            continue;
        }

        if entry.starts_with("export ") {
            continue;
        }

        let (name, value) = if let Some(eq) = entry.find('=') {
            let name = entry[..eq].trim();
            let expr = entry[eq + 1..].trim();
            let value = if is_string_literal(expr) {
                EnumValue::Str(unquote_string(expr)?)
            } else {
                EnumValue::Int(eval_int_expr(expr, &values)?)
            };
            (name, value)
        } else {
            let value = next_auto_int.ok_or_else(|| {
                format!("cannot auto-increment after string enum member: {entry}")
            })?;
            (entry, EnumValue::Int(value))
        };

        if name.is_empty() {
            return Err(format!("empty member name in enum {enum_name}").into());
        }

        entry = name;
        values.insert(entry.to_owned(), value.clone());
        next_auto_int = match value {
            EnumValue::Int(value) => Some(value + 1),
            EnumValue::Str(_) => None,
        };
        members.push(EnumMember {
            name: entry.to_owned(),
            value,
        });
    }

    if members.is_empty() {
        return Err(format!("source enum {enum_name} had no members").into());
    }

    Ok(EnumTable {
        name: enum_name.to_owned(),
        members,
    })
}

fn source_enum_block<'a>(source: &'a str, enum_name: &str) -> Result<&'a str, Box<dyn Error>> {
    let needle = format!("enum {enum_name}");
    let enum_pos = source
        .find(&needle)
        .ok_or_else(|| format!("source enum {enum_name} not found"))?;
    let after_enum = &source[enum_pos..];
    let open_rel = after_enum
        .find('{')
        .ok_or_else(|| format!("source enum {enum_name} has no opening brace"))?;
    let open = enum_pos + open_rel;
    let mut depth = 0usize;
    let mut close = None;

    for (offset, ch) in source[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + offset);
                    break;
                }
            }
            _ => {}
        }
    }

    let close = close.ok_or_else(|| format!("source enum {enum_name} has no closing brace"))?;
    Ok(&source[open + 1..close])
}

fn strip_line_comment(line: &str) -> String {
    let mut quoted = false;
    let mut escaped = false;
    let mut prev = '\0';

    for (idx, ch) in line.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
        } else if ch == '"' {
            quoted = true;
        } else if prev == '/' && ch == '/' {
            return line[..idx - 1].to_owned();
        }
        prev = ch;
    }

    line.to_owned()
}

fn is_string_literal(expr: &str) -> bool {
    expr.starts_with('"') && expr.ends_with('"')
}

fn unquote_string(expr: &str) -> Result<String, Box<dyn Error>> {
    if !is_string_literal(expr) {
        return Err(format!("not a string literal: {expr}").into());
    }

    Ok(expr[1..expr.len() - 1]
        .replace("\\\"", "\"")
        .replace("\\\\", "\\"))
}

fn eval_int_expr(expr: &str, values: &BTreeMap<String, EnumValue>) -> Result<i32, Box<dyn Error>> {
    let expr = trim_wrapping_parens(expr.trim());
    let mut result = 0i32;

    for part in expr.split('|') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        result |= eval_shift_expr(part, values)?;
    }

    Ok(result)
}

fn eval_shift_expr(
    expr: &str,
    values: &BTreeMap<String, EnumValue>,
) -> Result<i32, Box<dyn Error>> {
    if let Some(shift) = expr.find("<<") {
        let left = eval_atom(&expr[..shift], values)?;
        let right = eval_atom(&expr[shift + 2..], values)?;
        return Ok(left << right);
    }

    eval_atom(expr, values)
}

fn eval_atom(expr: &str, values: &BTreeMap<String, EnumValue>) -> Result<i32, Box<dyn Error>> {
    let expr = trim_wrapping_parens(expr.trim());
    if let Some(rest) = expr.strip_prefix('-') {
        return Ok(-eval_atom(rest, values)?);
    }

    if let Some(hex) = expr.strip_prefix("0x").or_else(|| expr.strip_prefix("0X")) {
        return Ok(i32::from_str_radix(hex, 16)?);
    }

    if expr.chars().all(|ch| ch.is_ascii_digit()) {
        return Ok(expr.parse()?);
    }

    match values.get(expr) {
        Some(EnumValue::Int(value)) => Ok(*value),
        Some(EnumValue::Str(_)) => {
            Err(format!("string enum member used as integer: {expr}").into())
        }
        None => Err(format!("unknown enum value expression: {expr}").into()),
    }
}

fn trim_wrapping_parens(mut expr: &str) -> &str {
    loop {
        let trimmed = expr.trim();
        if trimmed.starts_with(')') || !trimmed.starts_with('(') || !trimmed.ends_with(')') {
            return trimmed;
        }

        let mut depth = 0i32;
        let mut wraps = true;
        for (idx, ch) in trimmed.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && idx != trimmed.len() - 1 {
                        wraps = false;
                        break;
                    }
                }
                _ => {}
            }
        }

        if wraps {
            expr = &trimmed[1..trimmed.len() - 1];
        } else {
            return trimmed;
        }
    }
}

fn render_syntax_kind(table: &EnumTable) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen enums`. Do not edit by hand."
    )?;
    writeln!(out)?;
    writeln!(out, "#![allow(non_upper_case_globals)]")?;
    writeln!(out)?;
    writeln!(out, "#[repr(u16)]")?;
    writeln!(
        out,
        "#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]"
    )?;
    writeln!(out, "pub enum SyntaxKind {{")?;

    let mut canonical = BTreeMap::<i32, String>::new();
    let mut aliases = Vec::<(&EnumMember, String)>::new();
    for member in &table.members {
        let value = member_int(member, &table.name)?;
        if let Some(existing) = canonical.get(&value) {
            aliases.push((member, existing.clone()));
            continue;
        }

        canonical.insert(value, member.name.clone());
        writeln!(out, "    /// tsc SyntaxKind.{}", member.name)?;
        writeln!(out, "    {} = {},", member.name, value)?;
    }
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl SyntaxKind {{")?;
    for (member, target) in aliases {
        writeln!(out, "    /// tsc SyntaxKind.{}", member.name)?;
        writeln!(
            out,
            "    pub const {}: Self = Self::{};",
            member.name, target
        )?;
    }
    writeln!(out)?;
    writeln!(out, "    pub const fn value(self) -> u16 {{")?;
    writeln!(out, "        self as u16")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(
        out,
        "    pub const fn from_u16(value: u16) -> Option<Self> {{"
    )?;
    writeln!(out, "        match value {{")?;
    for (value, name) in &canonical {
        writeln!(out, "            {} => Some(Self::{}),", value, name)?;
    }
    writeln!(out, "            _ => None,")?;
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[cfg(test)]")?;
    writeln!(out, "mod tests {{")?;
    writeln!(out, "    use super::SyntaxKind;")?;
    writeln!(out)?;
    writeln!(out, "    #[test]")?;
    writeln!(out, "    fn generated_values_match_tsc_pins() {{")?;
    writeln!(
        out,
        "        assert_eq!(SyntaxKind::Identifier as u16, 80);"
    )?;
    writeln!(
        out,
        "        assert_eq!(SyntaxKind::FirstAssignment.value(), SyntaxKind::EqualsToken as u16);"
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(out)
}

fn render_flags(tables: &[EnumTable]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen enums`. Do not edit by hand."
    )?;
    writeln!(out)?;

    for table in tables {
        if table
            .members
            .iter()
            .all(|member| matches!(member.value, EnumValue::Int(_)))
        {
            render_int_table(&mut out, table)?;
        } else {
            render_string_table(&mut out, table)?;
        }
        writeln!(out)?;
    }

    writeln!(out, "#[cfg(test)]")?;
    writeln!(out, "mod tests {{")?;
    writeln!(out, "    use super::*;")?;
    writeln!(out)?;
    writeln!(out, "    #[test]")?;
    writeln!(out, "    fn generated_values_match_tsc_pins() {{")?;
    writeln!(
        out,
        "        assert_eq!(TypeFlags::STRING_LITERAL.bits(), 1024);"
    )?;
    writeln!(
        out,
        "        assert_eq!(FlowFlags::TRUE_CONDITION.bits(), 32);"
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(out)
}

fn render_int_table(out: &mut String, table: &EnumTable) -> Result<(), Box<dyn Error>> {
    writeln!(
        out,
        "#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]"
    )?;
    writeln!(out, "pub struct {}(i32);", table.name)?;
    writeln!(out)?;
    writeln!(out, "impl {} {{", table.name)?;

    let mut used_names = BTreeMap::<String, usize>::new();
    for member in &table.members {
        let const_name = screaming_const_name(&member.name);
        let const_name = disambiguate_const_name(const_name, &mut used_names);
        let value = member_int(member, &table.name)?;
        writeln!(out, "    /// tsc {}.{}", table.name, member.name)?;
        writeln!(out, "    pub const {}: Self = Self({});", const_name, value)?;
    }

    writeln!(out)?;
    writeln!(out, "    pub const fn from_bits(bits: i32) -> Self {{")?;
    writeln!(out, "        Self(bits)")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(out, "    pub const fn bits(self) -> i32 {{")?;
    writeln!(out, "        self.0")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(out, "    pub const fn is_empty(self) -> bool {{")?;
    writeln!(out, "        self.0 == 0")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(
        out,
        "    pub const fn contains(self, other: Self) -> bool {{"
    )?;
    writeln!(out, "        (self.0 & other.0) == other.0")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(
        out,
        "    pub const fn intersects(self, other: Self) -> bool {{"
    )?;
    writeln!(out, "        (self.0 & other.0) != 0")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl std::ops::BitOr for {} {{", table.name)?;
    writeln!(out, "    type Output = Self;")?;
    writeln!(out)?;
    writeln!(out, "    fn bitor(self, rhs: Self) -> Self::Output {{")?;
    writeln!(out, "        Self(self.0 | rhs.0)")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl std::ops::BitAnd for {} {{", table.name)?;
    writeln!(out, "    type Output = Self;")?;
    writeln!(out)?;
    writeln!(out, "    fn bitand(self, rhs: Self) -> Self::Output {{")?;
    writeln!(out, "        Self(self.0 & rhs.0)")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl std::ops::BitOrAssign for {} {{", table.name)?;
    writeln!(out, "    fn bitor_assign(&mut self, rhs: Self) {{")?;
    writeln!(out, "        self.0 |= rhs.0;")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(())
}

fn render_string_table(out: &mut String, table: &EnumTable) -> Result<(), Box<dyn Error>> {
    writeln!(out, "pub struct {};", table.name)?;
    writeln!(out)?;
    writeln!(out, "impl {} {{", table.name)?;
    let mut used_names = BTreeMap::<String, usize>::new();
    for member in &table.members {
        let const_name = screaming_const_name(&member.name);
        let const_name = disambiguate_const_name(const_name, &mut used_names);
        let EnumValue::Str(value) = &member.value else {
            return Err(format!("mixed string/int enum is not supported: {}", table.name).into());
        };
        writeln!(out, "    /// tsc {}.{}", table.name, member.name)?;
        writeln!(
            out,
            "    pub const {}: &'static str = {:?};",
            const_name, value
        )?;
    }
    writeln!(out, "}}")?;
    Ok(())
}

fn member_int(member: &EnumMember, enum_name: &str) -> Result<i32, Box<dyn Error>> {
    match member.value {
        EnumValue::Int(value) => Ok(value),
        EnumValue::Str(_) => Err(format!("{enum_name}.{} is not an integer", member.name).into()),
    }
}

fn disambiguate_const_name(name: String, used: &mut BTreeMap<String, usize>) -> String {
    let count = used.entry(name.clone()).or_default();
    *count += 1;
    if *count == 1 {
        name
    } else {
        format!("{name}_{}", *count)
    }
}

fn screaming_const_name(ts_name: &str) -> String {
    if ts_name == "$" {
        return "DOLLAR".to_owned();
    }
    if ts_name == "_" {
        return "UNDERSCORE".to_owned();
    }

    let mut out = String::new();
    let chars: Vec<char> = ts_name.chars().collect();
    for (idx, ch) in chars.iter().copied().enumerate() {
        if !ch.is_ascii_alphanumeric() {
            if !out.ends_with('_') {
                out.push('_');
            }
            continue;
        }

        if idx > 0 && ch.is_ascii_uppercase() {
            let prev = chars[idx - 1];
            let next = chars.get(idx + 1).copied();
            let splits_word = (prev.is_ascii_lowercase() || prev.is_ascii_digit())
                || (prev.is_ascii_uppercase() && next.is_some_and(|c| c.is_ascii_lowercase()));
            if splits_word && !out.ends_with('_') {
                out.push('_');
            }
        }

        out.push(ch.to_ascii_uppercase());
    }

    let mut out = out.trim_matches('_').to_owned();
    if out.is_empty() {
        out = "VALUE".to_owned();
    }
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "union"
            | "unsafe"
            | "use"
            | "where"
            | "while"
    )
}

#[derive(Clone, Debug)]
struct DtsField {
    name: String,
    type_text: String,
    optional: bool,
}

#[derive(Clone, Debug, Default)]
struct InterfaceDecl {
    bases: Vec<String>,
    fields: Vec<DtsField>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChildKind {
    Node,
    Nodes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChildVisit {
    name: String,
    kind: ChildKind,
}

#[derive(Clone, Debug)]
struct NodeSchema {
    kind_name: String,
    data_name: String,
    fields: Vec<SchemaField>,
    children: Vec<ChildVisit>,
}

#[derive(Clone, Debug)]
struct SchemaField {
    ts_name: String,
    rust_name: String,
    ty: RustFieldType,
    optional: bool,
    child: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RustFieldType {
    Node,
    NodeArray,
    Bool,
    String,
    Number,
    SyntaxKind,
    Payload,
}

fn codegen_nodes(check: bool) -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let tsc = fs::read_to_string(workspace.join("vendor/typescript-6.0.3/lib/_tsc.js"))?;
    let dts = fs::read_to_string(workspace.join("vendor/typescript-6.0.3/lib/typescript.d.ts"))?;

    let child_table = parse_for_each_child_table(&tsc)?;
    let interfaces = parse_dts_interfaces(&dts)?;
    let mut dts_nodes = collect_dts_nodes(&interfaces)?;
    seed_token_payload_nodes(&mut dts_nodes);
    seed_fieldless_nodes(&mut dts_nodes);
    seed_grammar_flag_fields(&mut dts_nodes);
    let schemas = merge_node_schema(child_table, dts_nodes);

    let nodes_rs = rustfmt_text(&render_nodes_rs(&schemas)?)?;
    let for_each_child_rs = rustfmt_text(&render_for_each_child_rs(&schemas)?)?;
    let schema_json = render_nodes_schema_json(&schemas)?;

    write_generated(
        &workspace.join("crates/syntax/src/nodes.rs"),
        &nodes_rs,
        check,
    )?;
    write_generated(
        &workspace.join("crates/syntax/src/for_each_child.rs"),
        &for_each_child_rs,
        check,
    )?;
    write_generated(
        &workspace.join("crates/syntax/nodes.schema.json"),
        &schema_json,
        check,
    )?;

    if check {
        println!("generated node schema files are up to date");
    } else {
        println!("generated node schema files");
    }

    Ok(())
}

fn parse_for_each_child_table(
    tsc: &str,
) -> Result<BTreeMap<String, Vec<ChildVisit>>, Box<dyn Error>> {
    let table = extract_balanced_after(tsc, "var forEachChildTable = ", '{', '}')?;
    let mut helper_cache = BTreeMap::<String, Vec<ChildVisit>>::new();
    let mut result = BTreeMap::<String, Vec<ChildVisit>>::new();

    for entry in split_top_level_entries(table) {
        let Some(kind_start) = entry.find("/*") else {
            continue;
        };
        let kind_name_start = kind_start + 2;
        let kind_name_end = entry[kind_name_start..]
            .find("*/")
            .map(|offset| kind_name_start + offset)
            .ok_or_else(|| format!("malformed forEachChildTable entry: {entry}"))?;
        let kind_name = entry[kind_name_start..kind_name_end].trim().to_owned();
        let value = entry
            .split_once(':')
            .map(|(_, value)| value.trim())
            .ok_or_else(|| format!("forEachChildTable entry has no value: {entry}"))?;

        let visits = if value.starts_with("function ") {
            extract_visits(value)
        } else {
            let helper_name = value.trim_end_matches(',').trim();
            if let Some(visits) = helper_cache.get(helper_name) {
                visits.clone()
            } else {
                let helper = extract_function(tsc, helper_name)?;
                let visits = extract_visits(helper);
                helper_cache.insert(helper_name.to_owned(), visits.clone());
                visits
            }
        };
        result.insert(kind_name, visits);
    }

    if result.is_empty() {
        return Err("forEachChildTable extraction produced no entries".into());
    }

    Ok(result)
}

fn extract_balanced_after<'a>(
    text: &'a str,
    marker: &str,
    open_ch: char,
    close_ch: char,
) -> Result<&'a str, Box<dyn Error>> {
    let marker_pos = text
        .find(marker)
        .ok_or_else(|| format!("marker not found: {marker}"))?;
    let after_marker = marker_pos + marker.len();
    let open_rel = text[after_marker..]
        .find(open_ch)
        .ok_or_else(|| format!("opening delimiter not found after marker: {marker}"))?;
    let open = after_marker + open_rel;
    let mut depth = 0usize;
    let mut close = None;
    for (offset, ch) in text[open..].char_indices() {
        if ch == open_ch {
            depth += 1;
        } else if ch == close_ch {
            depth -= 1;
            if depth == 0 {
                close = Some(open + offset);
                break;
            }
        }
    }
    let close =
        close.ok_or_else(|| format!("closing delimiter not found after marker: {marker}"))?;
    Ok(&text[open + 1..close])
}

fn extract_function<'a>(text: &'a str, name: &str) -> Result<&'a str, Box<dyn Error>> {
    extract_balanced_after(text, &format!("function {name}("), '{', '}')
}

fn split_top_level_entries(block: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (idx, ch) in block.char_indices() {
        match ch {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                let entry = block[start..idx].trim();
                if !entry.is_empty() {
                    entries.push(entry.to_owned());
                }
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    let tail = block[start..].trim();
    if !tail.is_empty() {
        entries.push(tail.to_owned());
    }
    entries
}

fn extract_visits(text: &str) -> Vec<ChildVisit> {
    let mut visits = Vec::new();
    for (needle, kind) in [
        ("visitNode2(cbNode, node.", ChildKind::Node),
        ("visitNodes(cbNode, cbNodes, node.", ChildKind::Nodes),
    ] {
        let mut rest = text;
        while let Some(pos) = rest.find(needle) {
            let field_start = pos + needle.len();
            let after = &rest[field_start..];
            let field_len = after
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .map(char::len_utf8)
                .sum::<usize>();
            if field_len > 0 {
                visits.push(ChildVisit {
                    name: after[..field_len].to_owned(),
                    kind,
                });
            }
            rest = &after[field_len..];
        }
    }

    // Whole-identifier occurrence position: a bare `find` would match
    // `node.type` inside `node.typeParameters` and scramble the visit order.
    visits.sort_by_key(|visit| field_occurrence_position(text, &visit.name));
    visits.dedup();
    visits
}

fn field_occurrence_position(text: &str, name: &str) -> usize {
    let needle = format!("node.{name}");
    let mut from = 0usize;
    while let Some(pos) = text[from..].find(&needle) {
        let abs = from + pos;
        let after = text[abs + needle.len()..].chars().next();
        if !matches!(after, Some(ch) if ch.is_ascii_alphanumeric() || ch == '_') {
            return abs;
        }
        from = abs + needle.len();
    }
    usize::MAX
}

fn parse_dts_interfaces(dts: &str) -> Result<BTreeMap<String, InterfaceDecl>, Box<dyn Error>> {
    let mut interfaces = BTreeMap::<String, InterfaceDecl>::new();
    let lines: Vec<&str> = dts.lines().collect();
    let mut idx = 0usize;

    while idx < lines.len() {
        let line = lines[idx].trim();
        let Some(interface_pos) = line.find("interface ") else {
            idx += 1;
            continue;
        };
        if !line[..interface_pos].trim().is_empty() {
            idx += 1;
            continue;
        }

        let header = line;
        let name_start = interface_pos + "interface ".len();
        let name_end = header[name_start..]
            .find(['<', ' ', '{'])
            .map(|offset| name_start + offset)
            .unwrap_or(header.len());
        let name = header[name_start..name_end].to_owned();
        let bases = parse_interface_bases(header);

        let mut body = String::new();
        let mut depth = header.matches('{').count() as i32 - header.matches('}').count() as i32;
        if let Some(open) = header.find('{') {
            body.push_str(&header[open + 1..]);
            body.push('\n');
        }

        idx += 1;
        while idx < lines.len() && depth > 0 {
            let body_line = lines[idx];
            depth += body_line.matches('{').count() as i32;
            depth -= body_line.matches('}').count() as i32;
            if depth >= 0 {
                body.push_str(body_line);
                body.push('\n');
            }
            idx += 1;
        }

        let fields = parse_interface_fields(&body);
        let decl = interfaces.entry(name).or_default();
        for base in bases {
            if !decl.bases.contains(&base) {
                decl.bases.push(base);
            }
        }
        for field in fields {
            merge_dts_field(&mut decl.fields, field);
        }
    }

    Ok(interfaces)
}

fn parse_interface_bases(header: &str) -> Vec<String> {
    let Some(extends_pos) = header.find(" extends ") else {
        return Vec::new();
    };
    let bases_text = header[extends_pos + " extends ".len()..]
        .split('{')
        .next()
        .unwrap_or_default();
    bases_text
        .split(',')
        .filter_map(|base| {
            let base = base.trim();
            if base.is_empty() {
                return None;
            }
            Some(
                base.split(|ch: char| ch == '<' || ch.is_whitespace())
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .filter(|base| !base.is_empty())
        .collect()
}

fn parse_interface_fields(body: &str) -> Vec<DtsField> {
    let mut fields = Vec::new();
    let mut entry = String::new();
    let mut in_block_comment = false;

    for raw_line in body.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if in_block_comment {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].trim();
                in_block_comment = false;
            } else {
                continue;
            }
        }
        while line.starts_with("/*") {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].trim();
            } else {
                in_block_comment = true;
                line = "";
                break;
            }
        }
        if line.is_empty() || line.starts_with('*') || line.starts_with("//") {
            continue;
        }

        entry.push_str(line);
        entry.push(' ');
        if line.ends_with(';') {
            if let Some(field) = parse_dts_field(&entry) {
                fields.push(field);
            }
            entry.clear();
        }
    }

    fields
}

fn parse_dts_field(entry: &str) -> Option<DtsField> {
    let entry = strip_line_comment(entry)
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_owned();
    if entry.is_empty() || entry.contains('(') || entry.starts_with('[') {
        return None;
    }
    let entry = entry
        .strip_prefix("readonly ")
        .unwrap_or(&entry)
        .strip_prefix("/** @internal */ ")
        .unwrap_or(entry.as_str())
        .trim();
    let colon = entry.find(':')?;
    let mut name = entry[..colon].trim();
    let optional = name.ends_with('?') || entry[colon + 1..].contains("undefined");
    name = name.trim_end_matches('?').trim();
    if name.starts_with('_') || name == "parent" {
        return None;
    }
    Some(DtsField {
        name: name.trim_matches('"').to_owned(),
        type_text: entry[colon + 1..].trim().to_owned(),
        optional,
    })
}

fn merge_dts_field(fields: &mut Vec<DtsField>, field: DtsField) {
    if let Some(existing) = fields
        .iter_mut()
        .find(|existing| existing.name == field.name)
    {
        *existing = field;
    } else {
        fields.push(field);
    }
}

fn collect_dts_nodes(
    interfaces: &BTreeMap<String, InterfaceDecl>,
) -> Result<BTreeMap<String, Vec<DtsField>>, Box<dyn Error>> {
    let mut nodes = BTreeMap::<String, Vec<DtsField>>::new();
    for (interface_name, decl) in interfaces {
        let Some(kind_field) = decl.fields.iter().find(|field| field.name == "kind") else {
            continue;
        };
        let kinds = syntax_kinds_from_type(&kind_field.type_text);
        if kinds.is_empty() {
            continue;
        }
        let fields = collect_interface_fields(interface_name, interfaces, &mut Vec::new())?;
        let fields: Vec<DtsField> = fields
            .into_iter()
            .filter(|field| field.name != "kind")
            .collect();
        for kind in kinds {
            nodes.entry(kind).or_insert_with(|| fields.clone());
        }
    }
    Ok(nodes)
}

fn seed_token_payload_nodes(nodes: &mut BTreeMap<String, Vec<DtsField>>) {
    for kind in ["Identifier", "PrivateIdentifier"] {
        nodes.entry(kind.to_owned()).or_insert_with(|| {
            vec![
                DtsField {
                    name: "escapedText".to_owned(),
                    type_text: "__String".to_owned(),
                    optional: false,
                },
                DtsField {
                    name: "text".to_owned(),
                    type_text: "string".to_owned(),
                    optional: false,
                },
            ]
        });
    }

    for kind in [
        "StringLiteral",
        "NumericLiteral",
        "BigIntLiteral",
        "RegularExpressionLiteral",
        "NoSubstitutionTemplateLiteral",
        "JsxText",
    ] {
        nodes.entry(kind.to_owned()).or_insert_with(|| {
            vec![DtsField {
                name: "text".to_owned(),
                type_text: "string".to_owned(),
                optional: false,
            }]
        });
    }

    for kind in ["TemplateHead", "TemplateMiddle", "TemplateTail"] {
        nodes.entry(kind.to_owned()).or_insert_with(|| {
            vec![
                DtsField {
                    name: "text".to_owned(),
                    type_text: "string".to_owned(),
                    optional: false,
                },
                DtsField {
                    name: "rawText".to_owned(),
                    type_text: "string".to_owned(),
                    optional: true,
                },
            ]
        });
    }
}

fn seed_fieldless_nodes(nodes: &mut BTreeMap<String, Vec<DtsField>>) {
    for kind in ["DebuggerStatement", "EmptyStatement", "OmittedExpression"] {
        nodes.entry(kind.to_owned()).or_default();
    }
}

/// isTypeOnly/isExportEquals: grammar bits the JS-file walker (and later
/// import elision) reads; seeded explicitly because the dts payload
/// extraction does not surface them.
fn seed_grammar_flag_fields(nodes: &mut BTreeMap<String, Vec<DtsField>>) {
    for kind in [
        "ImportClause",
        "ExportDeclaration",
        "ImportSpecifier",
        "ExportSpecifier",
    ] {
        nodes.entry(kind.to_owned()).or_default().push(DtsField {
            name: "isTypeOnly".to_owned(),
            type_text: "boolean".to_owned(),
            optional: false,
        });
    }
    nodes
        .entry("ExportAssignment".to_owned())
        .or_default()
        .push(DtsField {
            name: "isExportEquals".to_owned(),
            type_text: "boolean".to_owned(),
            optional: true,
        });
    // tsc PrefixUnaryExpression.operator / PostfixUnaryExpression.operator:
    // a SyntaxKind payload, not a child node. The binder consumes it
    // (getDeclarationName signed-numeric computed names, strict-mode
    // ++/-- checks, createFlowMutation).
    for kind in ["PrefixUnaryExpression", "PostfixUnaryExpression"] {
        nodes.entry(kind.to_owned()).or_default().push(DtsField {
            name: "operator".to_owned(),
            type_text: "SyntaxKind".to_owned(),
            optional: false,
        });
    }
    // tsc TypeOperatorNode.operator: KeyOfKeyword | UniqueKeyword |
    // ReadonlyKeyword — a SyntaxKind payload, not a child node. The
    // checker consumes it (isReadonlyTypeOperator 61138 for readonly
    // array/tuple targets, getTypeFromTypeOperatorNode dispatch).
    nodes
        .entry("TypeOperator".to_owned())
        .or_default()
        .push(DtsField {
            name: "operator".to_owned(),
            type_text: "SyntaxKind".to_owned(),
            optional: false,
        });
    // tsc ImportAttributes.token: WithKeyword | AssertKeyword — a
    // SyntaxKind payload, not a child node. The checker consumes it
    // (checkImportType's assert-deprecation row 2880; the parser
    // consumes the keyword before the attributes braces, so source
    // reconstruction cannot recover it).
    nodes
        .entry("ImportAttributes".to_owned())
        .or_default()
        .push(DtsField {
            name: "token".to_owned(),
            type_text: "SyntaxKind".to_owned(),
            optional: false,
        });
}

fn collect_interface_fields(
    interface_name: &str,
    interfaces: &BTreeMap<String, InterfaceDecl>,
    stack: &mut Vec<String>,
) -> Result<Vec<DtsField>, Box<dyn Error>> {
    if stack.iter().any(|name| name == interface_name) {
        return Ok(Vec::new());
    }
    let Some(decl) = interfaces.get(interface_name) else {
        return Ok(Vec::new());
    };

    stack.push(interface_name.to_owned());
    let mut fields = Vec::new();
    for base in &decl.bases {
        for field in collect_interface_fields(base, interfaces, stack)? {
            merge_dts_field(&mut fields, field);
        }
    }
    for field in &decl.fields {
        merge_dts_field(&mut fields, field.clone());
    }
    stack.pop();
    Ok(fields)
}

fn syntax_kinds_from_type(type_text: &str) -> Vec<String> {
    let mut kinds = Vec::new();
    let mut rest = type_text;
    while let Some(pos) = rest.find("SyntaxKind.") {
        let start = pos + "SyntaxKind.".len();
        let after = &rest[start..];
        let len = after
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .map(char::len_utf8)
            .sum::<usize>();
        if len > 0 {
            kinds.push(after[..len].to_owned());
        }
        rest = &after[len..];
    }
    kinds
}

fn merge_node_schema(
    child_table: BTreeMap<String, Vec<ChildVisit>>,
    dts_nodes: BTreeMap<String, Vec<DtsField>>,
) -> Vec<NodeSchema> {
    let mut schemas = BTreeMap::<String, NodeSchema>::new();
    for (kind_name, dts_fields) in dts_nodes {
        let children = child_table.get(&kind_name).cloned().unwrap_or_default();
        schemas.insert(
            kind_name.clone(),
            build_node_schema(kind_name, dts_fields, children),
        );
    }
    for (kind_name, children) in child_table {
        schemas.entry(kind_name.clone()).or_insert_with(|| {
            let dts_fields = children
                .iter()
                .map(|child| DtsField {
                    name: child.name.clone(),
                    type_text: match child.kind {
                        ChildKind::Node => "Node".to_owned(),
                        ChildKind::Nodes => "NodeArray<Node>".to_owned(),
                    },
                    optional: true,
                })
                .collect();
            build_node_schema(kind_name, dts_fields, children)
        });
    }
    schemas.into_values().collect()
}

fn build_node_schema(
    kind_name: String,
    dts_fields: Vec<DtsField>,
    children: Vec<ChildVisit>,
) -> NodeSchema {
    let mut fields = Vec::new();
    for dts_field in dts_fields {
        let child = children.iter().find(|child| child.name == dts_field.name);
        let ty = if let Some(child) = child {
            match child.kind {
                ChildKind::Node => RustFieldType::Node,
                ChildKind::Nodes => RustFieldType::NodeArray,
            }
        } else {
            rust_field_type(&dts_field.type_text)
        };
        let optional = dts_field.optional;
        fields.push(SchemaField {
            rust_name: rust_field_name(&dts_field.name),
            ts_name: dts_field.name,
            ty,
            optional,
            child: child.is_some(),
        });
    }
    for child in &children {
        if fields.iter().all(|field| field.ts_name != child.name) {
            fields.push(SchemaField {
                ts_name: child.name.clone(),
                rust_name: rust_field_name(&child.name),
                ty: match child.kind {
                    ChildKind::Node => RustFieldType::Node,
                    ChildKind::Nodes => RustFieldType::NodeArray,
                },
                optional: true,
                child: true,
            });
        }
    }

    NodeSchema {
        data_name: format!("{}Data", kind_name),
        kind_name,
        fields,
        children,
    }
}

fn rust_field_type(type_text: &str) -> RustFieldType {
    if type_text.contains("NodeArray<") {
        RustFieldType::NodeArray
    } else if type_text.contains("boolean") {
        RustFieldType::Bool
    } else if type_text.contains("string") || type_text.contains("__String") {
        RustFieldType::String
    } else if type_text.contains("number") {
        RustFieldType::Number
    } else if type_text.contains("SyntaxKind") {
        RustFieldType::SyntaxKind
    } else if type_text.contains("Node")
        || type_text.contains("Expression")
        || type_text.contains("Declaration")
        || type_text.contains("Identifier")
        || type_text.contains("Token")
        || type_text.contains("Type")
        || type_text.contains("Statement")
        || type_text.contains("Clause")
        || type_text.contains("Element")
        || type_text.contains("Literal")
        || type_text.contains("Name")
    {
        RustFieldType::Node
    } else {
        RustFieldType::Payload
    }
}

fn rust_field_name(ts_name: &str) -> String {
    let snake = snake_case(ts_name);
    match snake.as_str() {
        "type" | "default" | "abstract" | "final" | "box" | "move" | "ref" | "use" => {
            format!("r#{snake}")
        }
        _ => snake,
    }
}

fn snake_case(name: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (idx, ch) in chars.iter().copied().enumerate() {
        if !ch.is_ascii_alphanumeric() {
            if !out.ends_with('_') {
                out.push('_');
            }
            continue;
        }
        if idx > 0 && ch.is_ascii_uppercase() {
            let prev = chars[idx - 1];
            let next = chars.get(idx + 1).copied();
            let splits_word = (prev.is_ascii_lowercase() || prev.is_ascii_digit())
                || (prev.is_ascii_uppercase() && next.is_some_and(|c| c.is_ascii_lowercase()));
            if splits_word && !out.ends_with('_') {
                out.push('_');
            }
        }
        out.push(ch.to_ascii_lowercase());
    }
    out.trim_matches('_').to_owned()
}

fn render_nodes_rs(schemas: &[NodeSchema]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen nodes`. Do not edit by hand."
    )?;
    writeln!(out)?;
    writeln!(out, "use crate::SyntaxKind;")?;
    writeln!(out)?;
    writeln!(
        out,
        "#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]"
    )?;
    writeln!(out, "pub struct NodeId(pub u32);")?;
    writeln!(out)?;
    writeln!(
        out,
        "#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]"
    )?;
    writeln!(out, "pub struct NodeArrayId(pub u32);")?;
    writeln!(out)?;
    writeln!(out, "#[derive(Clone, Debug, Eq, PartialEq)]")?;
    writeln!(out, "pub struct NodeArray {{")?;
    writeln!(out, "    pub nodes: Vec<NodeId>,")?;
    writeln!(out, "    pub pos: u32,")?;
    writeln!(out, "    pub end: u32,")?;
    writeln!(out, "    pub has_trailing_comma: bool,")?;
    writeln!(out, "    /// tsc createMissingList's isMissingList marker.")?;
    writeln!(out, "    pub is_missing_list: bool,")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[derive(Clone, Debug, PartialEq)]")?;
    writeln!(out, "pub enum NodePayload {{")?;
    writeln!(out, "    Bool(bool),")?;
    writeln!(out, "    String(String),")?;
    writeln!(out, "    Number(f64),")?;
    writeln!(out, "    Kind(SyntaxKind),")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[derive(Clone, Debug, PartialEq)]")?;
    writeln!(out, "pub struct Node {{")?;
    writeln!(out, "    pub kind: SyntaxKind,")?;
    writeln!(out, "    pub flags: i32,")?;
    writeln!(out, "    pub pos: u32,")?;
    writeln!(out, "    pub end: u32,")?;
    writeln!(out, "    pub parent: Option<NodeId>,")?;
    writeln!(out, "    pub data: NodeData,")?;
    writeln!(out, "}}")?;
    writeln!(out)?;

    for schema in schemas {
        writeln!(out, "#[derive(Clone, Debug, PartialEq)]")?;
        writeln!(out, "pub struct {} {{", schema.data_name)?;
        for field in &schema.fields {
            writeln!(
                out,
                "    pub {}: {},",
                field.rust_name,
                render_field_type(field)
            )?;
        }
        writeln!(out, "}}")?;
        writeln!(out)?;
    }

    writeln!(out, "#[derive(Clone, Debug, PartialEq)]")?;
    writeln!(out, "pub enum NodeData {{")?;
    writeln!(out, "    Token,")?;
    for schema in schemas {
        writeln!(out, "    {}({}),", schema.kind_name, schema.data_name)?;
    }
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl NodeData {{")?;
    writeln!(out, "    pub const fn kind(&self) -> Option<SyntaxKind> {{")?;
    writeln!(out, "        match self {{")?;
    writeln!(out, "            Self::Token => None,")?;
    for schema in schemas {
        writeln!(
            out,
            "            Self::{}(_) => Some(SyntaxKind::{}),",
            schema.kind_name, schema.kind_name
        )?;
    }
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(out, "    pub fn missing(kind: SyntaxKind) -> Self {{")?;
    writeln!(out, "        match kind {{")?;
    for schema in schemas {
        writeln!(
            out,
            "            SyntaxKind::{} => Self::{}({} {{",
            schema.kind_name, schema.kind_name, schema.data_name
        )?;
        for field in &schema.fields {
            writeln!(
                out,
                "                {}: {},",
                field.rust_name,
                render_missing_field_value(field, &schema.kind_name)
            )?;
        }
        writeln!(out, "            }}),")?;
    }
    writeln!(out, "            _ => Self::Token,")?;
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    for schema in schemas {
        let accessor = format!("as_{}", snake_case(&schema.kind_name));
        writeln!(out)?;
        writeln!(
            out,
            "    pub fn {}(&self) -> Option<&{}> {{",
            accessor, schema.data_name
        )?;
        writeln!(out, "        match self {{")?;
        writeln!(
            out,
            "            Self::{}(data) => Some(data),",
            schema.kind_name
        )?;
        writeln!(out, "            _ => None,")?;
        writeln!(out, "        }}")?;
        writeln!(out, "    }}")?;
    }
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[cfg(test)]")?;
    writeln!(out, "mod tests {{")?;
    writeln!(out, "    use super::*;")?;
    writeln!(out)?;
    writeln!(out, "    #[test]")?;
    writeln!(out, "    fn generated_node_schema_has_core_nodes() {{")?;
    writeln!(out, "        assert_eq!(NodeData::Token.kind(), None);")?;
    writeln!(
        out,
        "        let _ = IdentifierData {{ escaped_text: String::new(), text: String::new() }};"
    )?;
    writeln!(
        out,
        "        assert_eq!(NodeData::missing(SyntaxKind::Identifier).kind(), Some(SyntaxKind::Identifier));"
    )?;
    writeln!(
        out,
        "        assert_eq!(NodeData::missing(SyntaxKind::SemicolonToken).kind(), None);"
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(out)
}

fn render_missing_field_value(field: &SchemaField, kind_name: &str) -> String {
    if field.optional {
        return "None".to_owned();
    }

    match field.ty {
        RustFieldType::Node => "NodeId::default()".to_owned(),
        RustFieldType::NodeArray => "NodeArrayId::default()".to_owned(),
        RustFieldType::Bool => "false".to_owned(),
        RustFieldType::String => "String::new()".to_owned(),
        RustFieldType::Number => "0.0".to_owned(),
        RustFieldType::SyntaxKind => format!("SyntaxKind::{kind_name}"),
        RustFieldType::Payload => "NodePayload::String(String::new())".to_owned(),
    }
}

fn render_field_type(field: &SchemaField) -> String {
    let base = match field.ty {
        RustFieldType::Node => "NodeId",
        RustFieldType::NodeArray => "NodeArrayId",
        RustFieldType::Bool => "bool",
        RustFieldType::String => "String",
        RustFieldType::Number => "f64",
        RustFieldType::SyntaxKind => "SyntaxKind",
        RustFieldType::Payload => "NodePayload",
    };
    if field.optional {
        format!("Option<{base}>")
    } else {
        base.to_owned()
    }
}

fn render_for_each_child_rs(schemas: &[NodeSchema]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen nodes`. Do not edit by hand."
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "use crate::nodes::{{Node, NodeArray, NodeArrayId, NodeData, NodeId}};"
    )?;
    writeln!(out)?;
    writeln!(out, "pub trait NodeLookup {{")?;
    writeln!(
        out,
        "    fn node_array(&self, id: NodeArrayId) -> &NodeArray;"
    )?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "pub fn for_each_child<L, F>(lookup: &L, node: &Node, mut cb: F) -> Option<NodeId>"
    )?;
    writeln!(out, "where")?;
    writeln!(out, "    L: NodeLookup,")?;
    writeln!(out, "    F: FnMut(NodeId) -> bool,")?;
    writeln!(out, "{{")?;
    writeln!(out, "    match &node.data {{")?;
    writeln!(out, "        NodeData::Token => None,")?;
    for schema in schemas {
        if schema.children.is_empty() {
            writeln!(
                out,
                "        NodeData::{}(_data) => None,",
                schema.kind_name
            )?;
        } else {
            writeln!(out, "        NodeData::{}(data) => {{", schema.kind_name)?;
            for child in &schema.children {
                let field = schema
                    .fields
                    .iter()
                    .find(|field| field.ts_name == child.name)
                    .ok_or_else(|| format!("missing generated field for child {}", child.name))?;
                let helper = match (child.kind, field.optional) {
                    (ChildKind::Node, false) => "visit_node",
                    (ChildKind::Node, true) => "visit_optional_node",
                    (ChildKind::Nodes, false) => "visit_nodes",
                    (ChildKind::Nodes, true) => "visit_optional_nodes",
                };
                if child.kind == ChildKind::Node {
                    writeln!(
                        out,
                        "            if let Some(result) = {}(data.{}, &mut cb) {{ return Some(result); }}",
                        helper, field.rust_name
                    )?;
                } else {
                    writeln!(
                        out,
                        "            if let Some(result) = {}(lookup, data.{}, &mut cb) {{ return Some(result); }}",
                        helper, field.rust_name
                    )?;
                }
            }
            writeln!(out, "            None")?;
            writeln!(out, "        }}")?;
        }
    }
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "fn visit_node<F>(id: NodeId, cb: &mut F) -> Option<NodeId>"
    )?;
    writeln!(
        out,
        "where F: FnMut(NodeId) -> bool {{ if cb(id) {{ Some(id) }} else {{ None }} }}"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "fn visit_optional_node<F>(id: Option<NodeId>, cb: &mut F) -> Option<NodeId>"
    )?;
    writeln!(
        out,
        "where F: FnMut(NodeId) -> bool {{ id.and_then(|id| visit_node(id, cb)) }}"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "fn visit_nodes<L, F>(lookup: &L, id: NodeArrayId, cb: &mut F) -> Option<NodeId>"
    )?;
    writeln!(out, "where L: NodeLookup, F: FnMut(NodeId) -> bool {{")?;
    writeln!(out, "    for node in &lookup.node_array(id).nodes {{")?;
    writeln!(out, "        if cb(*node) {{ return Some(*node); }}")?;
    writeln!(out, "    }}")?;
    writeln!(out, "    None")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "fn visit_optional_nodes<L, F>(lookup: &L, id: Option<NodeArrayId>, cb: &mut F) -> Option<NodeId>")?;
    writeln!(out, "where L: NodeLookup, F: FnMut(NodeId) -> bool {{ id.and_then(|id| visit_nodes(lookup, id, cb)) }}")?;
    Ok(out)
}

fn render_nodes_schema_json(schemas: &[NodeSchema]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(out, "{{")?;
    writeln!(out, "  \"schema\": 1,")?;
    writeln!(out, "  \"nodes\": [")?;
    for (idx, schema) in schemas.iter().enumerate() {
        writeln!(out, "    {{")?;
        writeln!(out, "      \"kindName\": {:?},", schema.kind_name)?;
        writeln!(out, "      \"dataName\": {:?},", schema.data_name)?;
        writeln!(out, "      \"fields\": [")?;
        for (field_idx, field) in schema.fields.iter().enumerate() {
            writeln!(
                out,
                "        {{\"name\": {:?}, \"rustName\": {:?}, \"type\": {:?}, \"optional\": {}, \"child\": {}}}{}",
                field.ts_name,
                field.rust_name,
                format!("{:?}", field.ty),
                field.optional,
                field.child,
                if field_idx + 1 == schema.fields.len() { "" } else { "," }
            )?;
        }
        writeln!(out, "      ],")?;
        writeln!(out, "      \"children\": [")?;
        for (child_idx, child) in schema.children.iter().enumerate() {
            writeln!(
                out,
                "        {{\"name\": {:?}, \"array\": {}}}{}",
                child.name,
                child.kind == ChildKind::Nodes,
                if child_idx + 1 == schema.children.len() {
                    ""
                } else {
                    ","
                }
            )?;
        }
        writeln!(out, "      ]")?;
        writeln!(
            out,
            "    }}{}",
            if idx + 1 == schemas.len() { "" } else { "," }
        )?;
    }
    writeln!(out, "  ]")?;
    writeln!(out, "}}")?;
    Ok(out)
}

#[derive(Clone, Debug)]
struct DiagnosticEntry {
    name: String,
    code: u32,
    category: String,
    text: String,
    reports_unnecessary: bool,
    reports_deprecated: bool,
    elided_in_compatibility_pyramid: bool,
}

#[derive(Clone, Debug)]
struct DiagnosticEntryFields {
    code: u32,
    category: String,
    reports_unnecessary: bool,
    reports_deprecated: bool,
    elided_in_compatibility_pyramid: bool,
}

fn codegen_diags(check: bool) -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let path = workspace.join("vendor/typescript-6.0.3/lib/diagnosticMessages.json");
    let raw = fs::read_to_string(path)?;
    let mut entries = parse_diagnostic_catalog(&raw)?;

    entries.sort_by_key(|entry| entry.code);
    let gen_rs = rustfmt_text(&render_diags_gen(&entries)?)?;
    write_generated(&workspace.join("crates/diags/src/gen.rs"), &gen_rs, check)?;

    if check {
        println!("generated diagnostic messages are up to date");
    } else {
        println!("generated diagnostic messages");
    }

    Ok(())
}

fn parse_diagnostic_catalog(src: &str) -> Result<Vec<DiagnosticEntry>, Box<dyn Error>> {
    let mut json = JsonReader::new(src);
    json.ws();
    json.expect('{')?;
    let mut entries = Vec::new();

    loop {
        json.ws();
        if json.peek() == Some('}') {
            json.bump();
            break;
        }

        let text = json.string()?;
        json.ws();
        json.expect(':')?;
        json.ws();
        let fields = parse_diagnostic_entry(&mut json)?;
        entries.push(DiagnosticEntry {
            name: diagnostic_static_name(&text),
            code: fields.code,
            category: fields.category,
            text,
            reports_unnecessary: fields.reports_unnecessary,
            reports_deprecated: fields.reports_deprecated,
            elided_in_compatibility_pyramid: fields.elided_in_compatibility_pyramid,
        });

        json.ws();
        match json.bump() {
            Some(',') => continue,
            Some('}') => break,
            other => {
                return Err(
                    format!("expected ',' or '}}' after diagnostic entry, got {other:?}").into(),
                )
            }
        }
    }

    let mut names = BTreeMap::<String, u32>::new();
    for entry in &entries {
        if let Some(existing) = names.insert(entry.name.clone(), entry.code) {
            return Err(format!(
                "diagnostic static name collision: {} for codes {} and {}",
                entry.name, existing, entry.code
            )
            .into());
        }
    }

    Ok(entries)
}

fn parse_diagnostic_entry(
    json: &mut JsonReader<'_>,
) -> Result<DiagnosticEntryFields, Box<dyn Error>> {
    json.expect('{')?;
    let mut code = None;
    let mut category = None;
    let mut reports_unnecessary = false;
    let mut reports_deprecated = false;
    let mut elided = false;

    loop {
        json.ws();
        if json.peek() == Some('}') {
            json.bump();
            break;
        }

        let key = json.string()?;
        json.ws();
        json.expect(':')?;
        json.ws();
        match key.as_str() {
            "code" => code = Some(json.number()? as u32),
            "category" => category = Some(json.string()?),
            "reportsUnnecessary" => reports_unnecessary = json.boolean()?,
            "reportsDeprecated" => reports_deprecated = json.boolean()?,
            "elidedInCompatabilityPyramid" => elided = json.boolean()?,
            _ => json.skip_value()?,
        }
        json.ws();
        match json.bump() {
            Some(',') => continue,
            Some('}') => break,
            other => {
                return Err(
                    format!("expected ',' or '}}' in diagnostic entry, got {other:?}").into(),
                )
            }
        }
    }

    Ok(DiagnosticEntryFields {
        code: code.ok_or("diagnostic entry missing code")?,
        category: category.ok_or("diagnostic entry missing category")?,
        reports_unnecessary,
        reports_deprecated,
        elided_in_compatibility_pyramid: elided,
    })
}

fn render_diags_gen(entries: &[DiagnosticEntry]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen diags`. Do not edit by hand."
    )?;
    writeln!(out)?;
    writeln!(out, "use super::{{DiagnosticCategory, DiagnosticMessage}};")?;
    writeln!(out)?;

    for entry in entries {
        writeln!(
            out,
            "pub static {}: DiagnosticMessage = DiagnosticMessage {{",
            entry.name
        )?;
        writeln!(out, "    code: {},", entry.code)?;
        writeln!(out, "    category: DiagnosticCategory::{},", entry.category)?;
        writeln!(out, "    text: {:?},", entry.text)?;
        writeln!(
            out,
            "    reports_unnecessary: {},",
            entry.reports_unnecessary
        )?;
        writeln!(out, "    reports_deprecated: {},", entry.reports_deprecated)?;
        writeln!(
            out,
            "    elided_in_compatibility_pyramid: {},",
            entry.elided_in_compatibility_pyramid
        )?;
        writeln!(out, "}};")?;
    }

    writeln!(out)?;
    writeln!(
        out,
        "pub static ALL_BY_CODE: &[(u32, &DiagnosticMessage)] = &["
    )?;
    for entry in entries {
        writeln!(out, "    ({}, &{}),", entry.code, entry.name)?;
    }
    writeln!(out, "];")?;
    writeln!(out)?;
    writeln!(out, "#[cfg(test)]")?;
    writeln!(out, "mod tests {{")?;
    writeln!(out, "    use super::*;")?;
    writeln!(out)?;
    writeln!(out, "    #[test]")?;
    writeln!(out, "    fn generated_diagnostic_pins_match_tsc() {{")?;
    writeln!(
        out,
        "        assert_eq!(Unterminated_string_literal.code, 1002);"
    )?;
    writeln!(out, "        assert_eq!(_0_expected.code, 1005);")?;
    writeln!(
        out,
        "        assert_eq!(ALL_BY_CODE.len(), {});",
        entries.len()
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(out)
}

fn diagnostic_static_name(message: &str) -> String {
    let mut out = String::new();
    let mut previous_was_separator = false;

    for ch in message.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            previous_was_separator = false;
        } else if !previous_was_separator {
            out.push('_');
            previous_was_separator = true;
        }
    }

    let mut out = out.trim_matches('_').to_owned();
    if out.is_empty() {
        out = "Diagnostic".to_owned();
    }
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if is_rust_keyword(&out) {
        out.insert_str(0, "r#");
    }
    out
}

struct JsonReader<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> JsonReader<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            bytes: src.as_bytes(),
            index: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.bytes.get(self.index).copied().map(char::from)
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek();
        if ch.is_some() {
            self.index += 1;
        }
        ch
    }

    fn expect(&mut self, expected: char) -> Result<(), Box<dyn Error>> {
        match self.bump() {
            Some(actual) if actual == expected => Ok(()),
            actual => Err(format!(
                "expected {expected:?}, got {actual:?} at byte {}",
                self.index
            )
            .into()),
        }
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.index += 1;
        }
    }

    fn string(&mut self) -> Result<String, Box<dyn Error>> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            let ch = self.bump().ok_or("unterminated JSON string")?;
            match ch {
                '"' => return Ok(out),
                '\\' => {
                    let escaped = self.bump().ok_or("unterminated JSON escape")?;
                    match escaped {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000C}'),
                        'u' => {
                            let mut hex = String::new();
                            for _ in 0..4 {
                                hex.push(self.bump().ok_or("short JSON unicode escape")?);
                            }
                            let code = u32::from_str_radix(&hex, 16)?;
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        other => return Err(format!("unknown JSON escape \\{other}").into()),
                    }
                }
                _ if ch.is_ascii() => out.push(ch),
                _ => {
                    self.index -= 1;
                    let rest = std::str::from_utf8(&self.bytes[self.index..])?;
                    let decoded = rest.chars().next().ok_or("invalid UTF-8 in JSON string")?;
                    self.index += decoded.len_utf8();
                    out.push(decoded);
                }
            }
        }
    }

    fn number(&mut self) -> Result<i64, Box<dyn Error>> {
        let start = self.index;
        while matches!(self.peek(), Some('-' | '+' | '0'..='9')) {
            self.index += 1;
        }
        Ok(std::str::from_utf8(&self.bytes[start..self.index])?.parse()?)
    }

    fn boolean(&mut self) -> Result<bool, Box<dyn Error>> {
        if self.bytes[self.index..].starts_with(b"true") {
            self.index += 4;
            Ok(true)
        } else if self.bytes[self.index..].starts_with(b"false") {
            self.index += 5;
            Ok(false)
        } else {
            Err(format!("expected JSON boolean at byte {}", self.index).into())
        }
    }

    fn skip_value(&mut self) -> Result<(), Box<dyn Error>> {
        self.ws();
        match self.peek() {
            Some('"') => {
                self.string()?;
            }
            Some('{') => self.skip_balanced('{', '}')?,
            Some('[') => self.skip_balanced('[', ']')?,
            Some('t') | Some('f') => {
                self.boolean()?;
            }
            Some('n') if self.bytes[self.index..].starts_with(b"null") => {
                self.index += 4;
            }
            Some('-' | '+' | '0'..='9') => {
                self.number()?;
            }
            other => {
                return Err(
                    format!("unexpected JSON value {other:?} at byte {}", self.index).into(),
                )
            }
        }
        Ok(())
    }

    fn skip_balanced(&mut self, open: char, close: char) -> Result<(), Box<dyn Error>> {
        self.expect(open)?;
        let mut depth = 1usize;
        while depth > 0 {
            match self.bump() {
                Some('"') => {
                    self.index -= 1;
                    self.string()?;
                }
                Some(ch) if ch == open => depth += 1,
                Some(ch) if ch == close => depth -= 1,
                Some(_) => {}
                None => return Err("unterminated JSON container".into()),
            }
        }
        Ok(())
    }
}

/// Documented audit normalizations shared by symbol-diff and lib-gate
/// (per-file binder vs a whole-program checker): lines whose ORACLE
/// symbol carries the Transient bit (33554432) are checker-MERGED
/// symbols (lib/global interface merging) — dropped in pairs; `__#N@`
/// private-name ids embed tsc's program-global getSymbolId counter —
/// the digits are wildcarded, keeping the structure check.
fn normalized_symbol_audit_lines(
    oracle_lines: &[String],
    rust_lines: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut normalized_oracle = Vec::new();
    let mut normalized_rust = Vec::new();
    for (oracle_line, rust_line) in oracle_lines.iter().zip(rust_lines) {
        let oracle_flags: i64 = oracle_line
            .split('\t')
            .nth(3)
            .and_then(|flags| flags.parse().ok())
            .unwrap_or(0);
        if oracle_flags & 33554432 != 0 {
            continue;
        }
        normalized_oracle.push(wildcard_private_name_ids(oracle_line));
        normalized_rust.push(wildcard_private_name_ids(rust_line));
    }
    (normalized_oracle, normalized_rust)
}

/// The lib-loading L1 gate (m4-lib-loading-steps.md §3): prove
/// parse+bind exactness over the vendored default-library files and
/// pin the program-order contract for every distinct lib set the
/// conformance corpus produces.
///
/// Phase 1 (parse): ast-diff over every vendor lib.*.d.ts — zero
/// parse errors on both sides, zero dump diffs.
/// Phase 2 (bind): per lib file, a single-file program whose FILES
/// list is the lib content (libs = [], so the oracle host does not
/// double-load it) — symbol dumps must match under the shared
/// normalizations.
/// Phase 3 (order): for each distinct ProgramJson.libs list across
/// the corpus, the oracle's getSourceFiles() order must equal
/// libs ++ files (the engine consumes the list as given).
fn lib_gate(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut skip_order = false;
    for arg in args {
        match arg.as_str() {
            "--skip-order" => skip_order = true,
            _ => return Err(format!("unexpected lib-gate argument: {arg}").into()),
        }
    }

    let workspace = find_tsrs2_root()?;
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let mut lib_files: Vec<PathBuf> = fs::read_dir(&vendor_lib_dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("lib.") && name.ends_with(".d.ts"))
        })
        .collect();
    lib_files.sort();

    // Phase 1: parse gate.
    let mut ast_oracle = AstDumpOracle::spawn(&workspace)?;
    let mut parse_failures = 0usize;
    for file in &lib_files {
        let text = fs::read_to_string(file)?;
        let file_name = file
            .file_name()
            .and_then(|name| name.to_str())
            .expect("lib file names are UTF-8")
            .to_owned();
        let (rust_dump, rust_parse_errors) = rust_ast_dump_text(&file_name, &text);
        let oracle_result = ast_oracle.ast_dump(file, &text, &file_name)?;
        if rust_parse_errors > 0 || oracle_result.parse_errors > 0 {
            parse_failures += 1;
            println!(
                "lib-gate parse errors in {file_name}: tsrs={rust_parse_errors} oracle={}",
                oracle_result.parse_errors
            );
            continue;
        }
        if rust_dump != oracle_result.dump {
            parse_failures += 1;
            let (line, left, right) = first_diff(&rust_dump, &oracle_result.dump);
            println!(
                "lib-gate ast diff {file_name} line {line}:\n  tsrs:   {}\n  oracle: {}",
                left.unwrap_or("<missing>"),
                right.unwrap_or("<missing>")
            );
        }
    }

    // Phase 2: bind gate.
    let temp_root = std::env::temp_dir().join(format!("tsrs2-lib-gate-{}", std::process::id()));
    if temp_root.exists() {
        fs::remove_dir_all(&temp_root)?;
    }
    fs::create_dir_all(&temp_root)?;
    let mut symbol_oracle = SymbolDumpOracle::spawn(&workspace)?;
    let mut bind_failures = 0usize;
    for (index, file) in lib_files.iter().enumerate() {
        let text = fs::read_to_string(file)?;
        let file_name = file
            .file_name()
            .and_then(|name| name.to_str())
            .expect("lib file names are UTF-8")
            .to_owned();
        let program = tsrs2_harness::ProgramJson {
            schema: 1,
            cwd: "/".to_owned(),
            options: BTreeMap::new(),
            libs: Vec::new(),
            files: vec![tsrs2_harness::ProgramFile {
                name: file_name.clone(),
                text_b64: BASE64.encode(text.as_bytes()),
            }],
            matrix_key: String::new(),
        };
        let out_dir = temp_root.join(format!("bind-{index}"));
        let paths = tsrs2_harness::write_program_jsons(std::slice::from_ref(&program), &out_dir)?;
        let oracle_files = symbol_oracle.symbol_dump(&paths[0])?;
        let rust_files = rust_symbol_dump(&program)?;
        let (Some(oracle_file), Some(Some(rust_file))) = (oracle_files.first(), rust_files.first())
        else {
            return Err(format!("lib-gate bind dump missing for {file_name}").into());
        };
        if oracle_file.parse_errors > 0 || rust_file.parse_errors > 0 {
            bind_failures += 1;
            println!("lib-gate bind parse errors in {file_name}");
            continue;
        }
        let (oracle_lines, rust_lines) = if oracle_file.lines.len() == rust_file.lines.len() {
            normalized_symbol_audit_lines(&oracle_file.lines, &rust_file.lines)
        } else {
            (oracle_file.lines.clone(), rust_file.lines.clone())
        };
        let oracle_dump = oracle_lines.join("\n");
        let rust_dump = rust_lines.join("\n");
        if !oracle_file.in_program || oracle_dump != rust_dump {
            bind_failures += 1;
            let (line, left, right) = first_diff(&rust_dump, &oracle_dump);
            println!(
                "lib-gate symbol diff {file_name} line {line}:\n  tsrs:   {}\n  oracle: {}",
                left.unwrap_or("<missing>"),
                right.unwrap_or("<missing>")
            );
        }
    }

    // Phase 3: order probe per distinct corpus lib set.
    let mut order_failures = 0usize;
    let mut lib_sets: std::collections::BTreeSet<Vec<String>> = std::collections::BTreeSet::new();
    if !skip_order {
        let fixtures = collect_fixture_paths(&workspace.join("ts-tests/tests/cases/conformance"))?;
        for fixture in &fixtures {
            let programs = match tsrs2_harness::expand_fixture_file(fixture, &vendor_lib_dir) {
                Ok(programs) => programs,
                // Fixtures the harness cannot expand are outside every
                // suite (conformance skips them the same way).
                Err(_) => continue,
            };
            for program in programs {
                lib_sets.insert(program.libs);
            }
        }
        let mut probe_paths = Vec::new();
        let mut expected: Vec<Vec<String>> = Vec::new();
        for (index, libs) in lib_sets.iter().enumerate() {
            let program = tsrs2_harness::ProgramJson {
                schema: 1,
                cwd: "/".to_owned(),
                options: BTreeMap::new(),
                libs: libs.clone(),
                files: vec![tsrs2_harness::ProgramFile {
                    name: "a.ts".to_owned(),
                    text_b64: BASE64.encode(b""),
                }],
                matrix_key: String::new(),
            };
            let out_dir = temp_root.join(format!("order-{index}"));
            let paths =
                tsrs2_harness::write_program_jsons(std::slice::from_ref(&program), &out_dir)?;
            probe_paths.push(paths[0].clone());
            let mut order = libs.clone();
            order.push("a.ts".to_owned());
            expected.push(order);
        }
        let output = std::process::Command::new("node")
            .arg(workspace.join("crates/oracle/files-dump.mjs"))
            .args(&probe_paths)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "files-dump probe failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let probes: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)?;
        for (index, probe) in probes.iter().enumerate() {
            let observed: Vec<String> = probe["files"]
                .as_array()
                .map(|files| {
                    files
                        .iter()
                        .filter_map(|value| value.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            if observed != expected[index] {
                order_failures += 1;
                println!(
                    "lib-gate order mismatch for set #{index}:\n  expected: {:?}\n  observed: {observed:?}",
                    expected[index]
                );
            }
        }
    }

    println!(
        "lib-gate: files={} parse_failures={parse_failures} bind_failures={bind_failures} lib_sets={} order_failures={order_failures}",
        lib_files.len(),
        lib_sets.len(),
    );
    if parse_failures + bind_failures + order_failures > 0 {
        return Err("lib-gate failed".into());
    }
    Ok(())
}

#[cfg(test)]
mod escape_scanner_tests {
    use super::*;

    fn scan(text: &str) -> Vec<EscapeSite> {
        scan_escape_text(Path::new("test.rs"), text)
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
            r#"fn expression_stub(&self, worker: &str, owner: &str) -> CheckResult2<TypeId> {
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
    fn manifest_roundtrips_and_keys_on_file_reason() {
        let sites = scan(
            r#"Err(Unsupported::new("alias value types (getTypeOfAlias, 5.8)"));
               Err(Unsupported::new("alias value types (getTypeOfAlias, 5.8)"));
               Err(Unsupported::new("entityNameToString on recovery node"));
               Err(Unsupported::new("a reason with \"quotes\" and back\\slash"));"#,
        );
        let entries = escape_manifest_from_sites(Path::new(""), &sites);
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
        let sites =
            scan(r#"Err(Unsupported::new("expired 5.5f dep; folded into the 5.7b close"))"#);
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
}

#[cfg(test)]
mod escapes_ceiling_tests {
    use super::*;

    #[test]
    fn parses_the_escapes_section() {
        let dir = std::env::temp_dir().join(format!("tsrs2-ceiling-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("ratchet.toml"),
            "[t0]\nrate = 0.1\n\n[escapes]\n# comment\nmax_untagged = 178\nmax_recovery = 71\n",
        )
        .unwrap();
        assert_eq!(
            read_ratchet_ceiling(&dir, "escapes", "max_untagged").unwrap(),
            Some(178)
        );
        assert_eq!(
            read_ratchet_ceiling(&dir, "escapes", "max_recovery").unwrap(),
            Some(71)
        );
        fs::write(dir.join("ratchet.toml"), "[t0]\nrate = 0.1\n").unwrap();
        assert_eq!(
            read_ratchet_ceiling(&dir, "escapes", "max_untagged").unwrap(),
            None
        );
        assert_eq!(
            read_ratchet_ceiling(&dir, "escapes", "max_recovery").unwrap(),
            None
        );
        fs::remove_dir_all(&dir).ok();
    }
}
