use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{display_relative, find_tsrs2_root, sha256_file};

const DEFAULT_MAX_LIB_CACHE_BUCKETS: usize = 8;

#[derive(Debug, Eq, PartialEq)]
struct TraceArgs {
    programs: Vec<PathBuf>,
    codes: BTreeSet<u32>,
    out: PathBuf,
    max_lib_cache_buckets: usize,
}

pub(crate) fn run(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let args = parse_args(args)?;
    let workspace = find_tsrs2_root()?;
    let inventory = workspace.join("m8-emitter-inventory.json");
    let bundle = workspace.join("vendor/typescript-6.0.3/lib/_tsc.js");
    let instrumenter = workspace.join("crates/oracle/trace-instrument.mjs");
    let driver = workspace.join("crates/oracle/trace-driver.mjs");
    let oracle_driver = workspace.join("crates/oracle/driver.mjs");
    let program_host = workspace.join("crates/oracle/program-host.mjs");
    let node_pin = workspace.join(".node-version");
    let node_version = current_node_version()?;
    let pinned_node_version = fs::read_to_string(&node_pin)?
        .trim()
        .trim_start_matches('v')
        .to_owned();
    if node_version != pinned_node_version {
        return Err(format!(
            "M8 diagnostic trace refused: launched Node v{node_version} but .node-version pins v{pinned_node_version}"
        )
        .into());
    }

    let programs = args
        .programs
        .iter()
        .map(|path| {
            let resolved = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir()?.join(path)
            };
            Ok(resolved.canonicalize()?)
        })
        .collect::<Result<Vec<PathBuf>, Box<dyn Error>>>()?;
    let codes = args.codes.iter().copied().collect::<Vec<_>>();
    let codes_bytes = serde_json::to_vec(&codes)?;
    let fingerprint = trace_fingerprint(
        &[
            &bundle,
            &inventory,
            &instrumenter,
            &driver,
            &oracle_driver,
            &program_host,
            &node_pin,
        ],
        &codes_bytes,
        &node_version,
    )?;
    let cache_dir = workspace.join("target/m8/trace/cache").join(&fingerprint);
    fs::create_dir_all(&cache_dir)?;
    let codes_path = cache_dir.join("codes.json");
    let instrumented = cache_dir.join("instrumented-tsc.cjs");
    let instrumentation_path = cache_dir.join("instrumentation.json");
    if !codes_path.exists() {
        fs::write(&codes_path, &codes_bytes)?;
    } else if fs::read(&codes_path)? != codes_bytes {
        return Err(format!(
            "content-addressed trace cache {} carries different codes",
            cache_dir.display()
        )
        .into());
    }

    let instrumentation = if instrumented.exists() && instrumentation_path.exists() {
        let report: Value = serde_json::from_slice(&fs::read(&instrumentation_path)?)?;
        validate_instrumentation(&report, &bundle, &instrumented, &codes)?;
        println!(
            "M8 diagnostic trace instrumentation: reused {}",
            display_relative(&workspace, &instrumented)
        );
        report
    } else {
        let output = Command::new("node")
            .arg(&instrumenter)
            .arg(&bundle)
            .arg(&inventory)
            .arg(&codes_path)
            .arg(&instrumented)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "diagnostic trace instrumenter failed with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let report: Value = serde_json::from_slice(&output.stdout)?;
        validate_instrumentation(&report, &bundle, &instrumented, &codes)?;
        fs::write(&instrumentation_path, serde_json::to_vec_pretty(&report)?)?;
        println!(
            "M8 diagnostic trace instrumentation: sites={} strategy={} cache={}",
            report["selected_sites"].as_u64().unwrap_or_default(),
            report["strategy"].as_str().unwrap_or("unknown"),
            display_relative(&workspace, &cache_dir)
        );
        report
    };

    let requests = programs
        .iter()
        .enumerate()
        .map(|(index, path)| {
            json!({
                "id": index,
                "programJsonPath": path.display().to_string(),
            })
        })
        .collect::<Vec<_>>();
    let mut oracle = Command::new("node");
    oracle.arg("--single-threaded").arg(&oracle_driver);
    let expected = run_node_jsonl(&mut oracle, &requests)?;

    let offset_map = serde_json::to_string(
        instrumentation["offset_map"]
            .as_array()
            .ok_or("diagnostic trace instrumentation lacks offset_map")?,
    )?;
    let instrumented = instrumented.canonicalize()?;
    let inventory = inventory.canonicalize()?;
    let mut observed = Vec::with_capacity(requests.len());
    for request in &requests {
        // V8 precise coverage is sensitive to which functions an
        // earlier probe caused to compile. Use one process per probe
        // so emitting/sibling comparisons are order-independent.
        let mut trace = Command::new("node");
        trace
            .arg("--single-threaded")
            .arg(&driver)
            .arg(&instrumented)
            .arg(&inventory)
            .arg(args.max_lib_cache_buckets.to_string())
            .env("TSRS_M8_TRACE_OFFSET_MAP", &offset_map);
        let mut response = run_node_jsonl(&mut trace, std::slice::from_ref(request))?;
        if response.len() != 1 {
            return Err("diagnostic trace probe process returned other than one response".into());
        }
        observed.push(response.pop().expect("one response checked above"));
    }
    if expected.len() != programs.len() || observed.len() != programs.len() {
        return Err(format!(
            "diagnostic trace response count mismatch: oracle={} trace={} programs={}",
            expected.len(),
            observed.len(),
            programs.len()
        )
        .into());
    }

    let mut probes = Vec::with_capacity(programs.len());
    let mut trace_events = 0usize;
    for (index, ((program, expected), observed)) in
        programs.iter().zip(&expected).zip(&observed).enumerate()
    {
        let expected_projection = json!({
            "id": index,
            "diagnostics": observed["diagnostics"],
        });
        if serde_json::to_vec(expected)? != serde_json::to_vec(&expected_projection)? {
            return Err(format!(
                "instrumented diagnostic trace changed oracle output for {}",
                program.display()
            )
            .into());
        }
        validate_probe(observed, &args.codes)?;
        trace_events += observed["trace"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default();
        probes.push(json!({
            "program_json": display_relative(&workspace, program),
            "program_sha256": sha256_file(program)?,
            "diagnostics": observed["diagnostics"],
            "trace": observed["trace"],
            "coverage": observed["coverage"],
            "oracle_equivalent": true,
        }));
    }

    let report = json!({
        "schema": 1,
        "status": "draft/report-only",
        "strategy": "targeted exact D2 diagnostic references plus per-probe V8 precise coverage",
        "probe_process_isolation": "one single-threaded Node process per probe",
        "fingerprint": fingerprint,
        "inputs": {
            "source": display_relative(&workspace, &bundle),
            "source_sha256": sha256_file(&bundle)?,
            "inventory": display_relative(&workspace, &inventory),
            "inventory_sha256": sha256_file(&inventory)?,
            "instrumenter_sha256": sha256_file(&instrumenter)?,
            "driver_sha256": sha256_file(&driver)?,
            "oracle_driver_sha256": sha256_file(&oracle_driver)?,
            "program_host_sha256": sha256_file(&program_host)?,
            "node_pin_sha256": sha256_file(&node_pin)?,
            "node_version": node_version,
            "codes": codes,
        },
        "instrumentation": instrumentation,
        "probes": probes,
    });
    if let Some(parent) = args.out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.out, serde_json::to_vec_pretty(&report)?)?;
    println!(
        "M8 diagnostic trace: probes={} events={} oracle-equivalent=true report={}",
        programs.len(),
        trace_events,
        args.out.display()
    );
    Ok(())
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<TraceArgs, Box<dyn Error>> {
    let mut programs = Vec::new();
    let mut codes = BTreeSet::new();
    let mut out = None;
    let mut max_lib_cache_buckets = DEFAULT_MAX_LIB_CACHE_BUCKETS;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--program-json" => programs.push(PathBuf::from(
                args.next().ok_or("missing value after --program-json")?,
            )),
            "--code" => {
                let raw = args.next().ok_or("missing value after --code")?;
                let code = raw
                    .parse::<u32>()
                    .map_err(|_| format!("invalid diagnostic code {raw}"))?;
                if !codes.insert(code) {
                    return Err(format!("duplicate diagnostic code {code}").into());
                }
            }
            "--out" => {
                if out.is_some() {
                    return Err("duplicate --out".into());
                }
                out = Some(PathBuf::from(
                    args.next().ok_or("missing value after --out")?,
                ));
            }
            "--max-lib-cache-buckets" => {
                let raw = args
                    .next()
                    .ok_or("missing value after --max-lib-cache-buckets")?;
                max_lib_cache_buckets = raw
                    .parse::<usize>()
                    .map_err(|_| format!("invalid max lib cache bucket count {raw}"))?;
                if max_lib_cache_buckets == 0 {
                    return Err("--max-lib-cache-buckets must be at least 1".into());
                }
            }
            _ => return Err(format!("unexpected m8 trace argument: {arg}").into()),
        }
    }
    if programs.is_empty() {
        return Err("m8 trace requires at least one --program-json".into());
    }
    if codes.is_empty() {
        return Err("m8 trace requires at least one --code".into());
    }
    let out = out.ok_or("m8 trace requires --out")?;
    Ok(TraceArgs {
        programs,
        codes,
        out,
        max_lib_cache_buckets,
    })
}

fn trace_fingerprint(
    paths: &[&Path],
    codes: &[u8],
    node_version: &str,
) -> Result<String, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    hasher.update(b"tsrs2-m8-diagnostic-trace-v1\0");
    for path in paths {
        hasher.update(sha256_file(path)?.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(codes);
    hasher.update(b"\0");
    hasher.update(node_version.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn current_node_version() -> Result<String, Box<dyn Error>> {
    let output = Command::new("node").arg("--version").output()?;
    if !output.status.success() {
        return Err(format!(
            "failed to query Node version with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let version = String::from_utf8(output.stdout)?
        .trim()
        .trim_start_matches('v')
        .to_owned();
    if version.is_empty() {
        return Err("Node returned an empty version".into());
    }
    Ok(version)
}

fn validate_instrumentation(
    report: &Value,
    bundle: &Path,
    instrumented: &Path,
    codes: &[u32],
) -> Result<(), Box<dyn Error>> {
    if report["schema"].as_u64() != Some(1)
        || report["strategy"].as_str() != Some("exact-d2-site-offsets/no-ast-visit")
        || report["source_declarations_visited"].as_u64() != Some(0)
        || report["selected_sites"]
            .as_u64()
            .is_none_or(|count| count == 0)
        || report["codes"] != json!(codes)
        || report["source_sha256"].as_str() != Some(&sha256_file(bundle)?)
        || report["output_sha256"].as_str() != Some(&sha256_file(instrumented)?)
        || !report["offset_map"].is_array()
    {
        return Err("invalid or stale M8 diagnostic trace instrumentation report".into());
    }
    Ok(())
}

fn validate_probe(probe: &Value, codes: &BTreeSet<u32>) -> Result<(), Box<dyn Error>> {
    let events = probe["trace"]
        .as_array()
        .ok_or("diagnostic trace probe lacks trace array")?;
    let coverage = probe["coverage"]["exact_d2_declarations"]
        .as_array()
        .ok_or("diagnostic trace probe lacks exact D2 coverage")?;
    let ordered = coverage
        .iter()
        .map(|id| {
            id.as_str()
                .ok_or_else(|| "diagnostic trace coverage contains a non-string id".into())
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    if ordered.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("diagnostic trace coverage identities are not sorted and unique".into());
    }
    let covered = ordered.into_iter().collect::<BTreeSet<_>>();
    for event in events {
        let code = event["site"]["code"]
            .as_u64()
            .and_then(|code| u32::try_from(code).ok())
            .ok_or("diagnostic trace event lacks a valid code")?;
        let declaration = event["site"]["declaration"]
            .as_str()
            .ok_or("diagnostic trace event lacks an exact D2 declaration")?;
        let frames = event["frames"]
            .as_array()
            .ok_or("diagnostic trace event lacks frames")?;
        if !codes.contains(&code)
            || !declaration.starts_with("d2:")
            || declaration.len() != 67
            || !covered.contains(declaration)
            || !matches!(
                event["pass"].as_str(),
                Some("syntactic" | "semantic" | "suggestion")
            )
            || frames.is_empty()
            || frames[0]["d2_declaration"].as_str() != Some(declaration)
        {
            return Err(format!(
                "invalid diagnostic trace event for code {code} at declaration {declaration}"
            )
            .into());
        }
    }
    Ok(())
}

fn run_node_jsonl(command: &mut Command, requests: &[Value]) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    {
        let stdin = child.stdin.as_mut().ok_or("Node JSONL stdin unavailable")?;
        for request in requests {
            serde_json::to_writer(&mut *stdin, request)?;
            writeln!(stdin)?;
        }
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!(
            "Node JSONL process failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    String::from_utf8(output.stdout)?
        .lines()
        .map(|line| Ok(serde_json::from_str(line)?))
        .collect()
}

#[cfg(test)]
mod tests {
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
}
