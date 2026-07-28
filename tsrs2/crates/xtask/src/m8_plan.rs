use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsrs2_conformance::ExactIdentity;

use super::{
    collect_ledger_entries, display_relative, exact_ledger_matches, find_tsrs2_root,
    mechanical_family_rows, read_json, sha256_file, validate_d2_inventory, LedgerEntry,
    M8EmitterDisposition, M8EmitterDispositions, M8EmitterFunction, M8EmitterInventory,
};

const ENTRY_BASELINE_COMMIT: &str = "8873bb74a2911a38ca2da6b6c305f7353bd3b31d";
const DEFAULT_MAX_LIB_CACHE_BUCKETS: usize = 8;

#[derive(Debug, Deserialize)]
struct ResidualSeed {
    supported_false_negative_diagnostics: usize,
    supported_false_negative_identities: Vec<ExactIdentity>,
}

#[derive(Clone, Debug)]
struct ProgramSeed {
    fixture: String,
    matrix_key: String,
    path: PathBuf,
    relative_path: String,
    sha256: String,
}

#[derive(Clone, Debug)]
struct IdentityObservation {
    id: String,
    identity: ExactIdentity,
    family: String,
    family_owner: String,
    family_note: String,
    program_key: String,
    events: Vec<Value>,
    producers: BTreeSet<String>,
}

#[derive(Debug)]
struct DraftArgs {
    conformance_json: PathBuf,
    out: PathBuf,
    raw_trace: Option<PathBuf>,
    program_dir: Option<PathBuf>,
    max_lib_cache_buckets: usize,
}

pub(crate) fn draft(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let args = parse_draft_args(args)?;
    let workspace = find_tsrs2_root()?;
    let conformance_path = workspace_path(&workspace, &args.conformance_json);
    let out = workspace_path(&workspace, &args.out);
    let raw_trace = args
        .raw_trace
        .map(|path| workspace_path(&workspace, &path))
        .unwrap_or_else(|| workspace.join("target/m8/owner-plan/raw-trace.json"));
    let program_dir = args
        .program_dir
        .map(|path| workspace_path(&workspace, &path))
        .unwrap_or_else(|| workspace.join("target/m8/owner-plan/programs"));

    let seed: ResidualSeed = read_json(&conformance_path)?;
    validate_residual_seed(&seed)?;
    let programs = materialize_programs(&workspace, &program_dir, &seed)?;
    let codes = seed
        .supported_false_negative_identities
        .iter()
        .map(|identity| identity.code)
        .collect::<BTreeSet<_>>();
    ensure_raw_trace(
        &workspace,
        &raw_trace,
        &programs,
        &codes,
        args.max_lib_cache_buckets,
    )?;
    let raw: Value = read_json(&raw_trace)?;
    validate_raw_trace(&raw, &programs, &codes)?;

    let inventory_path = workspace.join("m8-emitter-inventory.json");
    let dispositions_path = workspace.join("m8-emitter-dispositions.json");
    let inventory: M8EmitterInventory = read_json(&inventory_path)?;
    validate_d2_inventory(&inventory)?;
    let dispositions: M8EmitterDispositions = read_json(&dispositions_path)?;
    if dispositions.schema != 2 || dispositions.status != "frozen" {
        return Err("M8 owner plan requires frozen schema-2 emitter dispositions".into());
    }
    let ledger = collect_ledger_entries(&workspace)?;

    let probes = raw["probes"]
        .as_array()
        .ok_or("raw M8 trace lacks probes")?;
    let probe_by_path = probes
        .iter()
        .map(|probe| {
            let path = probe["program_json"]
                .as_str()
                .ok_or("raw M8 trace probe lacks program_json")?;
            Ok((path.to_owned(), probe))
        })
        .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?;
    let programs_by_key = programs
        .iter()
        .map(|program| (program_key(&program.fixture, &program.matrix_key), program))
        .collect::<BTreeMap<_, _>>();

    let mut observations = Vec::with_capacity(seed.supported_false_negative_identities.len());
    for identity in seed.supported_false_negative_identities {
        let key = program_key(&identity.fixture, &identity.matrix_key);
        let program = programs_by_key.get(&key).ok_or_else(|| {
            format!(
                "residual identity {} has no expanded program",
                identity.label()
            )
        })?;
        let probe = probe_by_path.get(&program.relative_path).ok_or_else(|| {
            format!(
                "residual identity {} has no raw trace probe {}",
                identity.label(),
                program.relative_path
            )
        })?;
        let events = probe["trace"]
            .as_array()
            .ok_or("raw trace probe lacks trace events")?
            .iter()
            // The trace pass is the execution phase in which tsc referenced the
            // diagnostic descriptor. That is intentionally distinct from the
            // oracle output bucket: parser-created JSDoc diagnostics surface in
            // semantic output, and some semantic 7016 diagnostics surface again
            // through suggestion output. The exact program plus diagnostic code
            // is therefore the conservative producer join; retaining the
            // execution pass below makes the phase distinction reviewable.
            .filter(|event| event["site"]["code"].as_u64() == Some(u64::from(identity.code)))
            .cloned()
            .collect::<Vec<_>>();
        let producers = events
            .iter()
            .filter_map(|event| event["site"]["declaration"].as_str().map(str::to_owned))
            .collect::<BTreeSet<_>>();
        let family = family_for(&workspace, identity.code, &identity.pass)?;
        observations.push(IdentityObservation {
            id: identity.sha256(),
            identity,
            family: family["family"].as_str().unwrap_or_default().to_owned(),
            family_owner: family["owner"].as_str().unwrap_or_default().to_owned(),
            family_note: family["note"].as_str().unwrap_or_default().to_owned(),
            program_key: key,
            events,
            producers,
        });
    }

    let mut grouped = BTreeMap::<String, Vec<&IdentityObservation>>::new();
    for observation in &observations {
        let basis = if observation.producers.is_empty() {
            format!(
                "{}\0unresolved\0{}\0{}",
                observation.family, observation.identity.pass, observation.identity.code
            )
        } else {
            format!(
                "{}\0{}",
                observation.family,
                observation
                    .producers
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\0")
            )
        };
        let cluster = format!(
            "m8:{}:{}",
            observation.family,
            &sha256_bytes(basis.as_bytes())[..16]
        );
        grouped.entry(cluster).or_default().push(observation);
    }

    let disposition_by_id = dispositions
        .entries
        .iter()
        .map(|entry| (entry.declaration.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let function_by_id = inventory
        .functions
        .iter()
        .map(|function| (function.id.as_str(), function))
        .collect::<BTreeMap<_, _>>();
    let probe_families = probe_family_sets(&observations);
    let mut cluster_values = Vec::with_capacity(grouped.len());
    let mut identity_cluster = BTreeMap::new();
    let mut missing_trace_clusters = 0usize;
    let mut missing_sibling_clusters = 0usize;
    let mut missing_rust_boundary_clusters = 0usize;
    for (cluster_id, members) in grouped {
        for member in &members {
            identity_cluster.insert(member.id.clone(), cluster_id.clone());
        }
        let family = members[0].family.clone();
        let family_owner = members[0].family_owner.clone();
        let family_note = members[0].family_note.clone();
        let producers = members
            .iter()
            .flat_map(|member| member.producers.iter().cloned())
            .collect::<BTreeSet<_>>();
        if producers.is_empty() {
            missing_trace_clusters += 1;
        }
        let static_closure = build_static_closure(
            &workspace,
            &producers,
            &inventory,
            &function_by_id,
            &disposition_by_id,
            &ledger,
        )?;
        if static_closure["ported_boundaries"]
            .as_array()
            .is_none_or(Vec::is_empty)
        {
            missing_rust_boundary_clusters += 1;
        }
        let emitting_programs = members
            .iter()
            .map(|member| member.program_key.clone())
            .collect::<BTreeSet<_>>();
        let code_passes = members
            .iter()
            .map(|member| {
                (
                    member.identity.code,
                    member.identity.pass.as_str().to_owned(),
                )
            })
            .collect::<BTreeSet<_>>();
        let closure_ids = static_closure["declarations"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry["id"].as_str().map(str::to_owned))
            .chain(
                static_closure["ported_boundaries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry["declaration"].as_str().map(str::to_owned)),
            )
            .chain(
                static_closure["reviewed_disposition_boundaries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry["declaration"].as_str().map(str::to_owned)),
            )
            .collect::<BTreeSet<_>>();
        let siblings = sibling_candidates(
            &family,
            &producers,
            &closure_ids,
            &emitting_programs,
            &code_passes,
            &programs,
            &probe_by_path,
            &probe_families,
        )?;
        if siblings["candidates"].as_array().is_none_or(Vec::is_empty) {
            missing_sibling_clusters += 1;
        }
        let mut trace_stacks = BTreeMap::new();
        for event in members.iter().flat_map(|member| &member.events) {
            let site = event["site"]["id"].as_str().unwrap_or_default();
            let declarations = event["frames"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|frame| frame["d2_declaration"].as_str())
                .fold(Vec::<String>::new(), |mut stack, declaration| {
                    if stack.last().is_none_or(|last| last != declaration) {
                        stack.push(declaration.to_owned());
                    }
                    stack
                });
            trace_stacks.insert(
                format!("{}\0{}", site, declarations.join("\0")),
                json!({
                    "diagnostic_site": site,
                    "producer": event["site"]["declaration"],
                    "execution_pass": event["pass"],
                    "exact_d2_stack": declarations,
                }),
            );
        }
        cluster_values.push(json!({
            "id": cluster_id,
            "family": family,
            "family_owner": family_owner,
            "family_note": family_note,
            "identity_ids": members.iter().map(|member| member.id.clone()).collect::<Vec<_>>(),
            "codes_and_passes": code_passes.iter().map(|(code, pass)| json!({
                "code": code,
                "pass": pass,
            })).collect::<Vec<_>>(),
            "emitting_programs": emitting_programs,
            "producer_candidates": producers,
            "diagnostic_traces": trace_stacks.into_values().collect::<Vec<_>>(),
            "static_closure": static_closure,
            "non_emitting_sibling": siblings,
            "tier_blockers": ["T0 trigger", "T1 category", "T2 exact span and top message", "T3 chain and related information"],
            "review": {
                "status": "unreviewed",
                "selected_sibling": null,
                "reviewed_boundary_overrides": [],
                "owner_slice": null,
                "rationale": null,
            },
        }));
    }

    let identity_values = observations
        .iter()
        .map(|observation| {
            json!({
                "id": observation.id,
                "identity": observation.identity,
                "family": observation.family,
                "program": observation.program_key,
                "cluster": identity_cluster[&observation.id],
                "trace_status": if observation.events.is_empty() { "missing" } else { "observed" },
            })
        })
        .collect::<Vec<_>>();
    let checked_inputs = checked_inputs(
        &workspace,
        &conformance_path,
        &raw_trace,
        &inventory_path,
        &dispositions_path,
    )?;
    let plan = json!({
        "schema": 1,
        "status": "draft",
        "entry_baseline_commit": ENTRY_BASELINE_COMMIT,
        "generated_from_commit": git_head(&workspace)?,
        "summary": {
            "identities": identity_values.len(),
            "programs": programs.len(),
            "codes": codes.len(),
            "clusters": cluster_values.len(),
            "missing_trace_clusters": missing_trace_clusters,
            "missing_sibling_clusters": missing_sibling_clusters,
            "missing_rust_boundary_clusters": missing_rust_boundary_clusters,
            "reviewed_clusters": 0,
            "assigned_slices": 0,
        },
        "checked_inputs": checked_inputs,
        "rust_ledger_sha256": ledger_sha256(&workspace, &ledger),
        "programs": programs.iter().map(|program| json!({
            "key": program_key(&program.fixture, &program.matrix_key),
            "fixture": program.fixture,
            "matrix_key": program.matrix_key,
            "program_json": program.relative_path,
            "program_sha256": program.sha256,
        })).collect::<Vec<_>>(),
        "identities": identity_values,
        "clusters": cluster_values,
    });
    audit_plan(&workspace, &plan, true)?;
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out, serde_json::to_vec_pretty(&plan)?)?;
    println!(
        "M8 owner plan draft: identities={} programs={} clusters={} missing-trace={} missing-sibling={} missing-rust-boundary={} report={}",
        plan["summary"]["identities"].as_u64().unwrap_or_default(),
        plan["summary"]["programs"].as_u64().unwrap_or_default(),
        plan["summary"]["clusters"].as_u64().unwrap_or_default(),
        missing_trace_clusters,
        missing_sibling_clusters,
        missing_rust_boundary_clusters,
        out.display()
    );
    Ok(())
}

pub(crate) fn check(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut plan = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--plan" => {
                if plan.is_some() {
                    return Err("duplicate --plan".into());
                }
                plan = Some(PathBuf::from(
                    args.next().ok_or("missing value after --plan")?,
                ));
            }
            _ => return Err(format!("unexpected m8 plan check argument: {arg}").into()),
        }
    }
    let workspace = find_tsrs2_root()?;
    let path = workspace_path(&workspace, &plan.ok_or("m8 plan check requires --plan")?);
    let value: Value = read_json(&path)?;
    audit_plan(&workspace, &value, true)?;
    println!(
        "M8 owner plan check: status={} identities={} clusters={} plan={}",
        value["status"].as_str().unwrap_or("unknown"),
        value["summary"]["identities"].as_u64().unwrap_or_default(),
        value["summary"]["clusters"].as_u64().unwrap_or_default(),
        path.display()
    );
    Ok(())
}

fn parse_draft_args(args: impl Iterator<Item = String>) -> Result<DraftArgs, Box<dyn Error>> {
    let mut conformance_json = None;
    let mut out = None;
    let mut raw_trace = None;
    let mut program_dir = None;
    let mut max_lib_cache_buckets = DEFAULT_MAX_LIB_CACHE_BUCKETS;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--conformance-json" => {
                conformance_json = Some(PathBuf::from(
                    args.next()
                        .ok_or("missing value after --conformance-json")?,
                ))
            }
            "--out" => {
                out = Some(PathBuf::from(
                    args.next().ok_or("missing value after --out")?,
                ))
            }
            "--raw-trace" => {
                raw_trace = Some(PathBuf::from(
                    args.next().ok_or("missing value after --raw-trace")?,
                ))
            }
            "--program-dir" => {
                program_dir = Some(PathBuf::from(
                    args.next().ok_or("missing value after --program-dir")?,
                ))
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
            _ => return Err(format!("unexpected m8 plan draft argument: {arg}").into()),
        }
    }
    Ok(DraftArgs {
        conformance_json: conformance_json.ok_or("m8 plan draft requires --conformance-json")?,
        out: out.ok_or("m8 plan draft requires --out")?,
        raw_trace,
        program_dir,
        max_lib_cache_buckets,
    })
}

fn validate_residual_seed(seed: &ResidualSeed) -> Result<(), Box<dyn Error>> {
    if seed.supported_false_negative_diagnostics != seed.supported_false_negative_identities.len() {
        return Err(format!(
            "supported residual count {} differs from exact identity count {}",
            seed.supported_false_negative_diagnostics,
            seed.supported_false_negative_identities.len()
        )
        .into());
    }
    let unique = seed
        .supported_false_negative_identities
        .iter()
        .map(ExactIdentity::sha256)
        .collect::<BTreeSet<_>>();
    if unique.len() != seed.supported_false_negative_identities.len() {
        return Err("supported residual identities are not unique".into());
    }
    Ok(())
}

fn materialize_programs(
    workspace: &Path,
    out_dir: &Path,
    seed: &ResidualSeed,
) -> Result<Vec<ProgramSeed>, Box<dyn Error>> {
    let requested = seed
        .supported_false_negative_identities
        .iter()
        .map(|identity| (identity.fixture.clone(), identity.matrix_key.clone()))
        .collect::<BTreeSet<_>>();
    let mut by_fixture = BTreeMap::<String, BTreeSet<String>>::new();
    for (fixture, matrix) in requested {
        by_fixture.entry(fixture).or_default().insert(matrix);
    }
    let vendor = workspace.join("vendor/typescript-6.0.3/lib");
    let mut result = Vec::new();
    for (fixture, matrices) in by_fixture {
        let fixture_path = workspace.join("ts-tests/tests/cases").join(&fixture);
        let expanded = tsrs2_harness::expand_fixture_file(&fixture_path, &vendor)?;
        let by_matrix = expanded
            .iter()
            .map(|program| (program.matrix_key.as_str(), program))
            .collect::<BTreeMap<_, _>>();
        for matrix_key in matrices {
            let program = by_matrix.get(matrix_key.as_str()).ok_or_else(|| {
                format!("fixture {fixture} has no expanded matrix {matrix_key:?}")
            })?;
            let key = program_key(&fixture, &matrix_key);
            let dir = out_dir.join(&sha256_bytes(key.as_bytes())[..16]);
            let paths = tsrs2_harness::write_program_jsons(std::slice::from_ref(*program), &dir)?;
            let path = paths
                .into_iter()
                .next()
                .expect("one program writes one path");
            result.push(ProgramSeed {
                fixture: fixture.clone(),
                matrix_key,
                relative_path: display_relative(workspace, &path),
                sha256: sha256_file(&path)?,
                path,
            });
        }
    }
    result.sort_by(|left, right| {
        left.fixture
            .cmp(&right.fixture)
            .then_with(|| left.matrix_key.cmp(&right.matrix_key))
    });
    Ok(result)
}

fn ensure_raw_trace(
    workspace: &Path,
    path: &Path,
    programs: &[ProgramSeed],
    codes: &BTreeSet<u32>,
    max_lib_cache_buckets: usize,
) -> Result<(), Box<dyn Error>> {
    if path.exists() {
        let existing: Value = read_json(path)?;
        if validate_raw_trace(&existing, programs, codes).is_ok() {
            println!(
                "M8 owner plan raw trace: reused {}",
                display_relative(workspace, path)
            );
            return Ok(());
        }
    }
    let mut args = Vec::new();
    for program in programs {
        args.push("--program-json".to_owned());
        args.push(program.path.display().to_string());
    }
    for code in codes {
        args.push("--code".to_owned());
        args.push(code.to_string());
    }
    args.push("--out".to_owned());
    args.push(path.display().to_string());
    args.push("--max-lib-cache-buckets".to_owned());
    args.push(max_lib_cache_buckets.to_string());
    crate::m8_trace::run(args.into_iter())
}

fn validate_raw_trace(
    raw: &Value,
    programs: &[ProgramSeed],
    codes: &BTreeSet<u32>,
) -> Result<(), Box<dyn Error>> {
    if raw["schema"].as_u64() != Some(1)
        || raw["status"].as_str() != Some("draft/report-only")
        || raw["inputs"]["codes"] != json!(codes.iter().copied().collect::<Vec<_>>())
    {
        return Err("raw M8 owner-plan trace has incompatible schema or codes".into());
    }
    let observed = raw["probes"]
        .as_array()
        .ok_or("raw M8 owner-plan trace lacks probes")?
        .iter()
        .map(|probe| {
            Ok((
                probe["program_json"]
                    .as_str()
                    .ok_or("raw trace probe lacks program_json")?
                    .to_owned(),
                probe["program_sha256"]
                    .as_str()
                    .ok_or("raw trace probe lacks program_sha256")?
                    .to_owned(),
            ))
        })
        .collect::<Result<BTreeSet<_>, Box<dyn Error>>>()?;
    let expected = programs
        .iter()
        .map(|program| (program.relative_path.clone(), program.sha256.clone()))
        .collect::<BTreeSet<_>>();
    if observed != expected {
        return Err("raw M8 owner-plan trace does not cover the exact program set".into());
    }
    Ok(())
}

fn family_for(workspace: &Path, code: u32, pass: &str) -> Result<Value, Box<dyn Error>> {
    let row = mechanical_family_rows(workspace, code, Some(pass))?;
    let matches = row["matches"]
        .as_array()
        .ok_or_else(|| format!("diagnostic {code}/{pass} has no A5 family matches array"))?;
    if matches.len() != 1 {
        return Err(format!(
            "diagnostic {code}/{pass} must map to exactly one owner family, found {}",
            matches.len()
        )
        .into());
    }
    Ok(matches[0].clone())
}

fn build_static_closure(
    workspace: &Path,
    producers: &BTreeSet<String>,
    inventory: &M8EmitterInventory,
    function_by_id: &BTreeMap<&str, &M8EmitterFunction>,
    disposition_by_id: &BTreeMap<&str, &M8EmitterDisposition>,
    ledger: &[LedgerEntry],
) -> Result<Value, Box<dyn Error>> {
    let mut queue = producers
        .iter()
        .map(|producer| (producer.as_str(), 0usize))
        .collect::<VecDeque<_>>();
    let mut seen = BTreeSet::new();
    let mut declarations = BTreeMap::new();
    let mut boundaries = BTreeMap::new();
    let mut disposition_boundaries = BTreeMap::new();
    let mut producer_sccs = BTreeMap::new();
    let mut property_candidates = BTreeMap::new();
    let mut unresolved = BTreeMap::new();
    let mut deferred_leaves = BTreeSet::new();
    while let Some((id, distance)) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        let function = function_by_id
            .get(id)
            .ok_or_else(|| format!("static closure references unknown D2 declaration {id}"))?;
        let disposition = disposition_by_id
            .get(id)
            .ok_or_else(|| format!("D2 declaration {id} lacks a frozen disposition"))?;
        if distance > 0 && disposition.disposition != "ported" {
            disposition_boundaries.insert(
                id,
                json!({
                    "declaration": id,
                    "distance": distance,
                    "source_slice_sha256": function.source_slice_sha256,
                    "disposition": disposition.disposition,
                    "owner": disposition.owner,
                    "evidence": disposition.evidence,
                }),
            );
            continue;
        }
        if disposition.disposition == "ported" {
            let joins = exact_ledger_matches(function, ledger);
            if joins.is_empty() {
                return Err(
                    format!("ported D2 boundary {id} lacks an exact Rust ledger join").into(),
                );
            }
            boundaries.insert(
                id,
                json!({
                    "declaration": id,
                    "distance": distance,
                    "source_slice_sha256": function.source_slice_sha256,
                    "rust": joins.iter().map(|entry| json!({
                        "file": display_relative(workspace, &entry.rust_path),
                        "line": entry.rust_line,
                        "function": entry.rust_fn,
                        "tsc_port": entry.port_name,
                        "typescript_version": entry.version,
                        "span": format!("{}:{}-{}", entry.span_file, entry.span_start, entry.span_end),
                        "source_slice_sha256": entry.hash,
                    })).collect::<Vec<_>>(),
                }),
            );
            continue;
        }
        if disposition.disposition == "not-applicable" {
            return Err(format!(
                "runtime-observed M8 producer {id} has a not-applicable frozen disposition"
            )
            .into());
        }
        declarations.insert(
            id,
            json!({
                "id": id,
                "distance": distance,
                "name": function.name,
                "lexical_path": function.lexical_path,
                "source_range": {
                    "start": function.source_range.start.offset,
                    "end": function.source_range.end.offset,
                },
                "source_slice_sha256": function.source_slice_sha256,
                "scc": function.scc,
                "disposition": disposition.disposition,
            }),
        );
        let scc = inventory
            .graph
            .sccs
            .iter()
            .find(|scc| scc.id == function.scc)
            .ok_or_else(|| format!("D2 producer {id} references unknown SCC {}", function.scc))?;
        producer_sccs.insert(
            scc.id.as_str(),
            json!({
                "id": scc.id,
                "member_count": scc.members.len(),
                "members": scc.members,
                "merge_status": "review-required",
            }),
        );
        let mut traversable = 0usize;
        for edge in inventory
            .graph
            .edges
            .iter()
            .filter(|edge| edge.caller == id)
        {
            if edge.kind == "property-candidate" {
                property_candidates.insert(
                    format!("{}\0{}", edge.caller, edge.callee),
                    json!({
                        "caller": edge.caller,
                        "candidate": edge.callee,
                        "sites": edge.sites.iter().map(|site| json!({
                            "line": site.line,
                            "character": site.character,
                        })).collect::<Vec<_>>(),
                    }),
                );
            } else {
                traversable += 1;
                queue.push_back((edge.callee.as_str(), distance + 1));
            }
        }
        for call in inventory
            .graph
            .unresolved_calls
            .iter()
            .filter(|call| call.caller == id)
        {
            unresolved.insert(
                format!(
                    "{}\0{}\0{}\0{}",
                    call.caller, call.line, call.character, call.expression
                ),
                json!({
                    "caller": call.caller,
                    "expression": call.expression,
                    "kind": call.kind,
                    "line": call.line,
                    "character": call.character,
                }),
            );
        }
        if traversable == 0 {
            deferred_leaves.insert(id.to_owned());
        }
    }
    Ok(json!({
        "declarations": declarations.into_values().collect::<Vec<_>>(),
        "ported_boundaries": boundaries.into_values().collect::<Vec<_>>(),
        "reviewed_disposition_boundaries": disposition_boundaries.into_values().collect::<Vec<_>>(),
        "deferred_leaves": deferred_leaves,
        "producer_sccs": producer_sccs.into_values().collect::<Vec<_>>(),
        "property_call_candidates": property_candidates.into_values().collect::<Vec<_>>(),
        "unresolved_static_calls": unresolved.into_values().collect::<Vec<_>>(),
        "reviewed_boundary_overrides": [],
    }))
}

#[allow(clippy::too_many_arguments)]
fn sibling_candidates(
    family: &str,
    producers: &BTreeSet<String>,
    closure_ids: &BTreeSet<String>,
    emitting_programs: &BTreeSet<String>,
    code_passes: &BTreeSet<(u32, String)>,
    programs: &[ProgramSeed],
    probe_by_path: &BTreeMap<String, &Value>,
    probe_families: &BTreeMap<String, BTreeSet<String>>,
) -> Result<Value, Box<dyn Error>> {
    let program_by_key = programs
        .iter()
        .map(|program| (program_key(&program.fixture, &program.matrix_key), program))
        .collect::<BTreeMap<_, _>>();
    let emitting_fixtures = emitting_programs
        .iter()
        .filter_map(|key| {
            program_by_key
                .get(key)
                .map(|program| program.fixture.as_str())
        })
        .collect::<BTreeSet<_>>();
    let mut emitting_coverage = BTreeSet::new();
    for key in emitting_programs {
        let program = program_by_key[key];
        emitting_coverage.extend(
            coverage_set(probe_by_path[&program.relative_path])?
                .intersection(closure_ids)
                .cloned(),
        );
    }
    let mut candidates = Vec::new();
    for program in programs {
        let key = program_key(&program.fixture, &program.matrix_key);
        if emitting_programs.contains(&key) {
            continue;
        }
        let probe = probe_by_path[&program.relative_path];
        let diagnostics = probe["diagnostics"]
            .as_array()
            .ok_or("raw trace probe lacks diagnostics")?;
        if diagnostics.iter().any(|diagnostic| {
            diagnostic["code"]
                .as_u64()
                .and_then(|code| u32::try_from(code).ok())
                .zip(diagnostic["pass"].as_str())
                .is_some_and(|pair| code_passes.contains(&(pair.0, pair.1.to_owned())))
        }) {
            continue;
        }
        let coverage = coverage_set(probe)?;
        let covered_producers = producers
            .intersection(&coverage)
            .cloned()
            .collect::<BTreeSet<_>>();
        if covered_producers.is_empty() {
            continue;
        }
        let relevant = coverage
            .intersection(closure_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        let symmetric_difference = emitting_coverage.symmetric_difference(&relevant).count();
        let same_fixture = emitting_fixtures.contains(program.fixture.as_str());
        let same_family = probe_families
            .get(&key)
            .is_some_and(|families| families.contains(family));
        let missing_producers = producers.len() - covered_producers.len();
        candidates.push((
            (
                missing_producers,
                !same_fixture,
                !same_family,
                symmetric_difference,
                key.clone(),
            ),
            json!({
                "program": key,
                "program_sha256": program.sha256,
                "covered_producers": covered_producers,
                "missing_producers": missing_producers,
                "same_fixture": same_fixture,
                "same_family": same_family,
                "closure_symmetric_difference": symmetric_difference,
                "emitting_only_closure": emitting_coverage.difference(&relevant).cloned().collect::<Vec<_>>(),
                "sibling_only_closure": relevant.difference(&emitting_coverage).cloned().collect::<Vec<_>>(),
                "shared_closure": relevant.intersection(&emitting_coverage).cloned().collect::<Vec<_>>(),
                "coverage_sha256": sha256_json(&probe["coverage"]["exact_d2_declarations"]),
            }),
        ));
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    candidates.truncate(5);
    let status = match candidates.first() {
        Some((score, _)) if score.0 == 0 => "candidate",
        Some(_) => "weak-candidate",
        None => "missing",
    };
    Ok(json!({
        "status": status,
        "selection": null,
        "emitting_coverage_sha256": sha256_json(&json!(emitting_coverage)),
        "candidates": candidates.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
    }))
}

fn coverage_set(probe: &Value) -> Result<BTreeSet<String>, Box<dyn Error>> {
    probe["coverage"]["exact_d2_declarations"]
        .as_array()
        .ok_or("raw trace probe lacks exact D2 coverage")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "raw trace coverage contains a non-string D2 identity".into())
        })
        .collect()
}

fn probe_family_sets(observations: &[IdentityObservation]) -> BTreeMap<String, BTreeSet<String>> {
    let mut result = BTreeMap::<String, BTreeSet<String>>::new();
    for observation in observations {
        result
            .entry(observation.program_key.clone())
            .or_default()
            .insert(observation.family.clone());
    }
    result
}

fn checked_inputs(
    workspace: &Path,
    conformance: &Path,
    raw_trace: &Path,
    inventory: &Path,
    dispositions: &Path,
) -> Result<Vec<Value>, Box<dyn Error>> {
    let paths = vec![
        conformance.to_owned(),
        raw_trace.to_owned(),
        inventory.to_owned(),
        dispositions.to_owned(),
        workspace.join("diag-families.json"),
        workspace.join("m8-scope.json"),
        workspace.join("ratchet.toml"),
        workspace.join(".node-version"),
        workspace.join("vendor/typescript-6.0.3/lib/_tsc.js"),
    ];
    paths
        .into_iter()
        .map(|path| {
            Ok(json!({
                "path": display_relative(workspace, &path),
                "sha256": sha256_file(&path)?,
            }))
        })
        .collect()
}

fn ledger_sha256(workspace: &Path, entries: &[LedgerEntry]) -> String {
    let value = entries
        .iter()
        .map(|entry| {
            json!({
                "rust_path": display_relative(workspace, &entry.rust_path),
                "rust_line": entry.rust_line,
                "rust_fn": entry.rust_fn,
                "port_name": entry.port_name,
                "version": entry.version,
                "span_file": entry.span_file,
                "span_start": entry.span_start,
                "span_end": entry.span_end,
                "hash": entry.hash,
            })
        })
        .collect::<Vec<_>>();
    sha256_json(&json!(value))
}

fn audit_plan(workspace: &Path, plan: &Value, require_live: bool) -> Result<(), Box<dyn Error>> {
    if plan["schema"].as_u64() != Some(1) || plan["status"].as_str() != Some("draft") {
        return Err("M8 owner plan must be schema 1 draft".into());
    }
    let identities = plan["identities"]
        .as_array()
        .ok_or("M8 owner plan lacks identities")?;
    let clusters = plan["clusters"]
        .as_array()
        .ok_or("M8 owner plan lacks clusters")?;
    let programs = plan["programs"]
        .as_array()
        .ok_or("M8 owner plan lacks programs")?;
    let codes = identities
        .iter()
        .map(|identity| {
            identity["identity"]["code"]
                .as_u64()
                .ok_or_else(|| "M8 owner plan identity lacks a numeric code".into())
        })
        .collect::<Result<BTreeSet<_>, Box<dyn Error>>>()?;
    if plan["summary"]["identities"].as_u64() != Some(identities.len() as u64)
        || plan["summary"]["clusters"].as_u64() != Some(clusters.len() as u64)
        || plan["summary"]["programs"].as_u64() != Some(programs.len() as u64)
        || plan["summary"]["codes"].as_u64() != Some(codes.len() as u64)
    {
        return Err("M8 owner plan summary counts are stale".into());
    }
    let program_keys = programs
        .iter()
        .map(|program| {
            program["key"]
                .as_str()
                .ok_or_else(|| "M8 owner plan program lacks key".into())
        })
        .collect::<Result<BTreeSet<_>, Box<dyn Error>>>()?;
    if program_keys.len() != programs.len() {
        return Err("M8 owner plan contains duplicate program keys".into());
    }
    let identity_ids = identities
        .iter()
        .map(|identity| {
            let id = identity["id"]
                .as_str()
                .ok_or("M8 owner plan identity lacks id")?;
            let exact: ExactIdentity = serde_json::from_value(identity["identity"].clone())
                .map_err(|error| format!("M8 owner plan identity {id} is malformed: {error}"))?;
            if exact.sha256() != id {
                return Err(format!("M8 owner plan identity {id} has a stale exact hash").into());
            }
            Ok(id)
        })
        .collect::<Result<BTreeSet<_>, Box<dyn Error>>>()?;
    if identity_ids.len() != identities.len() {
        return Err("M8 owner plan contains duplicate exact identities".into());
    }
    let cluster_ids = clusters
        .iter()
        .map(|cluster| {
            cluster["id"]
                .as_str()
                .ok_or_else(|| "M8 owner plan cluster lacks id".into())
        })
        .collect::<Result<BTreeSet<_>, Box<dyn Error>>>()?;
    if cluster_ids.len() != clusters.len() {
        return Err("M8 owner plan contains duplicate cluster ids".into());
    }
    let mut assigned = Vec::new();
    let mut assigned_cluster = BTreeMap::new();
    for cluster in clusters {
        let cluster_id = cluster["id"]
            .as_str()
            .ok_or("M8 owner plan cluster lacks id")?;
        let member_ids = cluster["identity_ids"]
            .as_array()
            .ok_or("M8 owner plan cluster lacks identity_ids")?;
        for member_id in member_ids {
            let member_id = member_id
                .as_str()
                .ok_or("M8 owner plan cluster contains a non-string identity id")?;
            assigned.push(member_id);
            assigned_cluster.insert(member_id, cluster_id);
        }
    }
    let assigned_set = assigned.iter().copied().collect::<BTreeSet<_>>();
    if assigned_set != identity_ids || assigned_set.len() != assigned.len() {
        return Err("M8 owner plan clusters do not partition the exact identities".into());
    }
    for identity in identities {
        let identity_id = identity["id"]
            .as_str()
            .ok_or("M8 owner plan identity lacks id")?;
        let cluster_id = identity["cluster"]
            .as_str()
            .ok_or("M8 owner plan identity lacks cluster")?;
        if !cluster_ids.contains(cluster_id) {
            return Err("M8 owner plan identity references an unknown cluster".into());
        }
        if assigned_cluster.get(identity_id).copied() != Some(cluster_id) {
            return Err(
                "M8 owner plan identity and cluster membership disagree on assignment".into(),
            );
        }
        if !program_keys.contains(
            identity["program"]
                .as_str()
                .ok_or("M8 owner plan identity lacks program")?,
        ) {
            return Err("M8 owner plan identity references an unknown program".into());
        }
    }
    let identity_by_id = identities
        .iter()
        .map(|identity| {
            (
                identity["id"]
                    .as_str()
                    .expect("identity ids validated above"),
                identity,
            )
        })
        .collect::<BTreeMap<_, _>>();
    for cluster in clusters {
        let family = cluster["family"]
            .as_str()
            .ok_or("M8 owner plan cluster lacks family")?;
        let expected_code_passes = cluster["identity_ids"]
            .as_array()
            .expect("cluster membership validated above")
            .iter()
            .map(|id| {
                let identity = identity_by_id[id.as_str().expect("member ids validated above")];
                Ok((
                    identity["identity"]["code"]
                        .as_u64()
                        .ok_or("M8 owner plan exact identity lacks code")?,
                    identity["identity"]["pass"]
                        .as_str()
                        .ok_or("M8 owner plan exact identity lacks pass")?
                        .to_owned(),
                ))
            })
            .collect::<Result<BTreeSet<_>, Box<dyn Error>>>()?;
        if cluster["codes_and_passes"]
            != json!(expected_code_passes
                .iter()
                .map(|(code, pass)| json!({"code": code, "pass": pass}))
                .collect::<Vec<_>>())
        {
            return Err("M8 owner plan cluster has stale code/pass membership".into());
        }
        for id in cluster["identity_ids"]
            .as_array()
            .expect("cluster membership validated above")
        {
            if identity_by_id[id.as_str().expect("member ids validated above")]["family"].as_str()
                != Some(family)
            {
                return Err("M8 owner plan cluster crosses owner families".into());
            }
        }
    }
    let missing_trace = clusters
        .iter()
        .filter(|cluster| {
            cluster["producer_candidates"]
                .as_array()
                .is_none_or(Vec::is_empty)
        })
        .count();
    let missing_sibling = clusters
        .iter()
        .filter(|cluster| {
            cluster["non_emitting_sibling"]["status"]
                .as_str()
                .is_none_or(|status| status == "missing")
        })
        .count();
    let missing_rust_boundary = clusters
        .iter()
        .filter(|cluster| {
            cluster["static_closure"]["ported_boundaries"]
                .as_array()
                .is_none_or(Vec::is_empty)
        })
        .count();
    let reviewed = clusters
        .iter()
        .filter(|cluster| cluster["review"]["status"].as_str() == Some("reviewed"))
        .count();
    let assigned_slices = clusters
        .iter()
        .filter_map(|cluster| cluster["review"]["owner_slice"].as_str())
        .collect::<BTreeSet<_>>()
        .len();
    for (field, actual) in [
        ("missing_trace_clusters", missing_trace),
        ("missing_sibling_clusters", missing_sibling),
        ("missing_rust_boundary_clusters", missing_rust_boundary),
        ("reviewed_clusters", reviewed),
        ("assigned_slices", assigned_slices),
    ] {
        if plan["summary"][field].as_u64() != Some(actual as u64) {
            return Err(format!("M8 owner plan summary field {field} is stale").into());
        }
    }
    if require_live {
        for input in plan["checked_inputs"]
            .as_array()
            .ok_or("M8 owner plan lacks checked_inputs")?
        {
            let path = input["path"]
                .as_str()
                .ok_or("M8 owner plan input lacks path")?;
            let expected = input["sha256"]
                .as_str()
                .ok_or("M8 owner plan input lacks sha256")?;
            if sha256_file(&workspace.join(path))? != expected {
                return Err(format!("M8 owner plan input {path} is stale").into());
            }
        }
    }
    Ok(())
}

fn workspace_path(workspace: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        workspace.join(path)
    }
}

fn program_key(fixture: &str, matrix_key: &str) -> String {
    if matrix_key.is_empty() {
        fixture.to_owned()
    } else {
        format!("{fixture}#{matrix_key}")
    }
}

fn git_head(workspace: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "failed to resolve current commit: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn sha256_json(value: &Value) -> String {
    sha256_bytes(&serde_json::to_vec(value).expect("JSON value serialization cannot fail"))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_parser_requires_exact_inputs_and_positive_cache_bound() {
        let parsed = parse_draft_args(
            [
                "--conformance-json",
                "conformance.json",
                "--out",
                "plan.json",
                "--max-lib-cache-buckets",
                "2",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(parsed.conformance_json, PathBuf::from("conformance.json"));
        assert_eq!(parsed.out, PathBuf::from("plan.json"));
        assert_eq!(parsed.max_lib_cache_buckets, 2);
        assert!(parse_draft_args(
            [
                "--conformance-json",
                "c.json",
                "--out",
                "p.json",
                "--max-lib-cache-buckets",
                "0"
            ]
            .into_iter()
            .map(str::to_owned)
        )
        .is_err());
    }

    #[test]
    fn residual_seed_rejects_count_only_or_duplicate_claims() {
        let identity = ExactIdentity {
            fixture: "conformance/a.ts".to_owned(),
            matrix_key: String::new(),
            pass: "semantic".to_owned(),
            file: Some("a.ts".to_owned()),
            start: Some(0),
            length: Some(1),
            code: 7005,
            category: "error".to_owned(),
            chain_sha256: "a".repeat(64),
            related_sha256: "b".repeat(64),
            occurrence: 0,
        };
        assert!(validate_residual_seed(&ResidualSeed {
            supported_false_negative_diagnostics: 2,
            supported_false_negative_identities: vec![identity.clone()],
        })
        .is_err());
        assert!(validate_residual_seed(&ResidualSeed {
            supported_false_negative_diagnostics: 2,
            supported_false_negative_identities: vec![identity.clone(), identity],
        })
        .is_err());
    }

    #[test]
    fn plan_audit_rejects_disagreeing_identity_and_cluster_assignment() {
        let exact = ExactIdentity {
            fixture: "conformance/a.ts".to_owned(),
            matrix_key: String::new(),
            pass: "semantic".to_owned(),
            file: Some("a.ts".to_owned()),
            start: Some(0),
            length: Some(1),
            code: 7005,
            category: "error".to_owned(),
            chain_sha256: "a".repeat(64),
            related_sha256: "b".repeat(64),
            occurrence: 0,
        };
        let identity_id = exact.sha256();
        let plan = json!({
            "schema": 1,
            "status": "draft",
            "summary": {
                "identities": 1,
                "programs": 1,
                "codes": 1,
                "clusters": 2,
            },
            "programs": [{"key": "conformance/a.ts"}],
            "identities": [{
                "id": identity_id,
                "identity": exact,
                "family": "implicit-any",
                "program": "conformance/a.ts",
                "cluster": "cluster-b",
            }],
            "clusters": [
                {
                    "id": "cluster-a",
                    "family": "implicit-any",
                    "identity_ids": [identity_id],
                    "codes_and_passes": [{"code": 7005, "pass": "semantic"}],
                },
                {
                    "id": "cluster-b",
                    "family": "implicit-any",
                    "identity_ids": [],
                    "codes_and_passes": [],
                },
            ],
        });
        assert!(audit_plan(Path::new("."), &plan, false)
            .unwrap_err()
            .to_string()
            .contains("disagree"));
    }
}
