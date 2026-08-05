use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_conformance::ExactIdentity;

use super::{
    collect_ledger_entries, display_relative, exact_ledger_matches, find_workspace_root,
    git_repository_root, is_full_lower_hex_commit, m8_git_is_ancestor, m8_resolve_git_commit,
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
    entry_residual: bool,
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
    sibling_fixtures: Vec<String>,
    max_lib_cache_buckets: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewOverlay {
    schema: u64,
    status: String,
    clusters: Vec<ClusterReview>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClusterReview {
    id: String,
    selected_sibling: String,
    owner_slice: String,
    rationale: String,
    scc_decisions: Vec<SccDecision>,
    rust_boundary_overrides: Vec<RustBoundaryOverride>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct SccDecision {
    id: String,
    decision: String,
    rationale: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RustBoundaryOverride {
    kind: String,
    file: String,
    function: String,
    rationale: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FreezeRecord {
    adjudication_commit: String,
}

pub(crate) fn draft(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let args = parse_draft_args(args)?;
    let workspace = find_workspace_root()?;
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
    let programs = materialize_programs(&workspace, &program_dir, &seed, &args.sibling_fixtures)?;
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
    validate_raw_trace(&workspace, &raw, &programs, &codes)?;

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
        let trace_ids = members
            .iter()
            .flat_map(|member| &member.events)
            .flat_map(|event| event["frames"].as_array().into_iter().flatten())
            .filter_map(|frame| frame["d2_declaration"].as_str().map(str::to_owned))
            .collect::<BTreeSet<_>>();
        let comparison_ids = closure_ids
            .union(&trace_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        let siblings = sibling_candidates(
            &family,
            &producers,
            &comparison_ids,
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
            "role": if program.entry_residual { "entry-residual" } else { "sibling-probe" },
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

pub(crate) fn apply_review(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut plan_path = None;
    let mut review_path = None;
    let mut out = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let slot = match arg.as_str() {
            "--plan" => &mut plan_path,
            "--review" => &mut review_path,
            "--out" => &mut out,
            _ => return Err(format!("unexpected m8 plan apply-review argument: {arg}").into()),
        };
        if slot.is_some() {
            return Err(format!("duplicate {arg}").into());
        }
        *slot = Some(PathBuf::from(
            args.next()
                .ok_or_else(|| format!("missing value after {arg}"))?,
        ));
    }
    let workspace = find_workspace_root()?;
    let plan_path = workspace_path(
        &workspace,
        &plan_path.ok_or("m8 plan apply-review requires --plan")?,
    );
    let review_path = workspace_path(
        &workspace,
        &review_path.ok_or("m8 plan apply-review requires --review")?,
    );
    let out = workspace_path(
        &workspace,
        &out.ok_or("m8 plan apply-review requires --out")?,
    );
    let mut plan: Value = read_json(&plan_path)?;
    audit_plan(&workspace, &plan, true)?;
    let review: ReviewOverlay = read_json(&review_path)?;
    if review.schema != 1 || review.status != "reviewed" {
        return Err("M8 owner-plan review must be schema 1 reviewed".into());
    }
    let by_cluster = review
        .clusters
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    if by_cluster.len() != review.clusters.len() {
        return Err("M8 owner-plan review contains duplicate cluster ids".into());
    }
    let cluster_count;
    {
        let clusters = plan["clusters"]
            .as_array_mut()
            .ok_or("M8 owner plan lacks clusters")?;
        cluster_count = clusters.len();
        let expected_ids = clusters
            .iter()
            .filter_map(|cluster| cluster["id"].as_str().map(str::to_owned))
            .collect::<BTreeSet<_>>();
        if by_cluster
            .keys()
            .map(|id| (*id).to_owned())
            .collect::<BTreeSet<_>>()
            != expected_ids
        {
            return Err("M8 owner-plan review must enumerate the exact cluster set".into());
        }
        for cluster in clusters.iter_mut() {
            let cluster_id = cluster["id"]
                .as_str()
                .ok_or("M8 owner plan cluster lacks id")?;
            let entry = by_cluster[cluster_id];
            if entry.owner_slice.trim().is_empty() || entry.rationale.trim().is_empty() {
                return Err(format!(
                    "M8 owner-plan review {cluster_id} requires owner_slice and rationale"
                )
                .into());
            }
            let candidates = cluster["non_emitting_sibling"]["candidates"]
                .as_array()
                .ok_or("M8 owner plan cluster lacks sibling candidates")?;
            let selection = candidates
                .iter()
                .find(|candidate| {
                    candidate["program"].as_str() == Some(entry.selected_sibling.as_str())
                })
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "M8 owner-plan review {cluster_id} selects unknown sibling {}",
                        entry.selected_sibling
                    )
                })?;
            let expected_sccs = cluster["static_closure"]["producer_sccs"]
                .as_array()
                .ok_or("M8 owner plan cluster lacks producer_sccs")?;
            validate_scc_decisions(cluster_id, expected_sccs, &entry.scc_decisions)?;
            let ported = cluster["static_closure"]["ported_boundaries"]
                .as_array()
                .ok_or("M8 owner plan cluster lacks ported_boundaries")?;
            if ported.is_empty() == entry.rust_boundary_overrides.is_empty() {
                return Err(format!(
                    "M8 owner-plan review {cluster_id} requires overrides exactly when no exact Rust boundary exists"
                )
                .into());
            }
            let overrides =
                resolve_rust_boundary_overrides(&workspace, &entry.rust_boundary_overrides)?;
            cluster["non_emitting_sibling"]["selection"] = selection;
            cluster["static_closure"]["reviewed_boundary_overrides"] = json!(overrides);
            cluster["review"] = json!({
                "status": "reviewed",
                "selected_sibling": entry.selected_sibling,
                "scc_decisions": entry.scc_decisions,
                "reviewed_boundary_overrides": overrides,
                "owner_slice": entry.owner_slice,
                "rationale": entry.rationale,
            });
        }
    }
    plan["summary"]["missing_rust_boundary_clusters"] = json!(0);
    plan["summary"]["reviewed_clusters"] = json!(cluster_count);
    plan["summary"]["assigned_slices"] = json!(review
        .clusters
        .iter()
        .map(|entry| entry.owner_slice.as_str())
        .collect::<BTreeSet<_>>()
        .len());
    plan["review_input"] = json!({
        "path": display_relative(&workspace, &review_path),
        "sha256": sha256_file(&review_path)?,
    });
    audit_plan(&workspace, &plan, true)?;
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out, serde_json::to_vec_pretty(&plan)?)?;
    println!(
        "M8 owner plan review applied: clusters={} slices={} report={}",
        plan["summary"]["reviewed_clusters"]
            .as_u64()
            .unwrap_or_default(),
        plan["summary"]["assigned_slices"]
            .as_u64()
            .unwrap_or_default(),
        out.display()
    );
    Ok(())
}

pub(crate) fn freeze(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
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
            _ => return Err(format!("unexpected m8 plan freeze argument: {arg}").into()),
        }
    }
    let workspace = find_workspace_root()?;
    let path = workspace_path(&workspace, &plan.ok_or("m8 plan freeze requires --plan")?);
    let mut value: Value = read_json(&path)?;
    audit_plan(&workspace, &value, false)?;
    if value["status"].as_str() != Some("draft") {
        return Err("M8 owner plan freeze requires a reviewed draft".into());
    }
    require_complete_review(&value)?;
    verify_review_input(&workspace, &value)?;

    let adjudication_commit = m8_resolve_git_commit(&workspace, "HEAD")?;
    let adjudicated = plan_at(&workspace, &adjudication_commit, &path)?;
    if adjudicated != value {
        return Err(
            "M8 owner plan freeze requires the identical reviewed draft at HEAD; land it first"
                .into(),
        );
    }
    value["status"] = json!("frozen");
    value["freeze"] = json!({
        "adjudication_commit": adjudication_commit,
    });
    audit_plan(&workspace, &value, false)?;
    verify_frozen_anchor(&workspace, &path, &value)?;
    fs::write(&path, serde_json::to_vec_pretty(&value)?)?;
    println!(
        "M8 owner plan frozen: identities={} clusters={} anchor={} plan={}",
        value["summary"]["identities"].as_u64().unwrap_or_default(),
        value["summary"]["clusters"].as_u64().unwrap_or_default(),
        value["freeze"]["adjudication_commit"]
            .as_str()
            .unwrap_or("unknown"),
        path.display()
    );
    Ok(())
}

pub(crate) fn check(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut plan = None;
    let mut baseline = None;
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
            "--baseline" => {
                if baseline.is_some() {
                    return Err("duplicate --baseline".into());
                }
                baseline = Some(args.next().ok_or("missing value after --baseline")?);
            }
            _ => return Err(format!("unexpected m8 plan check argument: {arg}").into()),
        }
    }
    let workspace = find_workspace_root()?;
    let path = workspace_path(&workspace, &plan.ok_or("m8 plan check requires --plan")?);
    let value: Value = read_json(&path)?;
    let frozen = value["status"].as_str() == Some("frozen");
    audit_plan(&workspace, &value, !frozen)?;
    if frozen {
        verify_review_input(&workspace, &value)?;
        verify_frozen_anchor(&workspace, &path, &value)?;
    }
    if let Some(baseline) = baseline {
        verify_plan_baseline(&workspace, &path, &baseline, &value)?;
    }
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
    let mut sibling_fixtures = Vec::new();
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
            "--sibling-fixture" => {
                sibling_fixtures.push(args.next().ok_or("missing value after --sibling-fixture")?)
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
        sibling_fixtures,
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
    sibling_fixtures: &[String],
) -> Result<Vec<ProgramSeed>, Box<dyn Error>> {
    let mut requested = seed
        .supported_false_negative_identities
        .iter()
        .map(|identity| (identity.fixture.clone(), identity.matrix_key.clone()))
        .map(|pair| (pair, true))
        .collect::<BTreeMap<_, _>>();
    for selector in sibling_fixtures {
        let (fixture, matrix) = selector
            .split_once('#')
            .map_or((selector.as_str(), None), |(fixture, matrix)| {
                (fixture, Some(matrix))
            });
        if fixture.is_empty() || matrix.is_some_and(str::is_empty) {
            return Err(format!("invalid --sibling-fixture selector {selector:?}").into());
        }
        requested
            .entry((fixture.to_owned(), matrix.unwrap_or("").to_owned()))
            .or_insert(false);
    }
    let mut by_fixture = BTreeMap::<String, BTreeMap<String, bool>>::new();
    for ((fixture, matrix), entry_residual) in requested {
        by_fixture
            .entry(fixture)
            .or_default()
            .insert(matrix, entry_residual);
    }
    let vendor = workspace.join("vendor/typescript-6.0.3/lib");
    let mut result = Vec::new();
    for (fixture, matrices) in by_fixture {
        let fixture_path = workspace.join("ts-tests/tests/cases").join(&fixture);
        let expanded = tsc_harness::expand_fixture_file(&fixture_path, &vendor)?;
        let by_matrix = expanded
            .iter()
            .map(|program| (program.matrix_key.as_str(), program))
            .collect::<BTreeMap<_, _>>();
        let matrices = if matrices.len() == 1 && matrices.contains_key("") && expanded.len() > 1 {
            return Err(format!(
                "sibling fixture {fixture} expands to {} matrices; select one with #<matrix-key>",
                expanded.len()
            )
            .into());
        } else {
            matrices
        };
        for (matrix_key, entry_residual) in matrices {
            let program = by_matrix.get(matrix_key.as_str()).ok_or_else(|| {
                format!("fixture {fixture} has no expanded matrix {matrix_key:?}")
            })?;
            let key = program_key(&fixture, &matrix_key);
            let dir = out_dir.join(&sha256_bytes(key.as_bytes())[..16]);
            let paths = tsc_harness::write_program_jsons(std::slice::from_ref(*program), &dir)?;
            let path = paths
                .into_iter()
                .next()
                .expect("one program writes one path");
            result.push(ProgramSeed {
                fixture: fixture.clone(),
                matrix_key,
                entry_residual,
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
    let expected = programs
        .iter()
        .map(|program| (program.relative_path.clone(), program.sha256.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut reusable = BTreeMap::new();
    let mut template = None;
    if path.exists() {
        let existing: Value = read_json(path)?;
        if validate_raw_trace_header(workspace, &existing, codes).is_ok() {
            for probe in existing["probes"]
                .as_array()
                .ok_or("raw M8 owner-plan trace lacks probes")?
            {
                let Some(program_path) = probe["program_json"].as_str() else {
                    continue;
                };
                let Some(program_sha256) = probe["program_sha256"].as_str() else {
                    continue;
                };
                if expected.get(program_path).map(String::as_str) == Some(program_sha256) {
                    reusable.insert(program_path.to_owned(), probe.clone());
                }
            }
            template = Some(existing);
        }
        if reusable.len() == programs.len() {
            validate_raw_trace(
                workspace,
                template
                    .as_ref()
                    .expect("a reusable raw trace always keeps its template"),
                programs,
                codes,
            )?;
            println!(
                "M8 owner plan raw trace: reused {}",
                display_relative(workspace, path)
            );
            return Ok(());
        }
    }
    let missing = programs
        .iter()
        .filter(|program| !reusable.contains_key(&program.relative_path))
        .cloned()
        .collect::<Vec<_>>();
    let delta_path = path.with_file_name("raw-trace.incremental.json");
    let mut args = Vec::new();
    for program in &missing {
        args.push("--program-json".to_owned());
        args.push(program.path.display().to_string());
    }
    for code in codes {
        args.push("--code".to_owned());
        args.push(code.to_string());
    }
    args.push("--out".to_owned());
    args.push(delta_path.display().to_string());
    args.push("--max-lib-cache-buckets".to_owned());
    args.push(max_lib_cache_buckets.to_string());
    crate::m8_trace::run(args.into_iter())?;
    let delta: Value = read_json(&delta_path)?;
    validate_raw_trace_header(workspace, &delta, codes)?;
    for probe in delta["probes"]
        .as_array()
        .ok_or("incremental M8 owner-plan trace lacks probes")?
    {
        let program_path = probe["program_json"]
            .as_str()
            .ok_or("incremental M8 owner-plan trace probe lacks program_json")?;
        reusable.insert(program_path.to_owned(), probe.clone());
    }
    let mut combined = template.unwrap_or_else(|| delta.clone());
    combined["probes"] = json!(programs
        .iter()
        .map(|program| {
            reusable
                .get(&program.relative_path)
                .cloned()
                .ok_or_else(|| format!("incremental M8 trace omitted {}", program.relative_path))
        })
        .collect::<Result<Vec<_>, String>>()?);
    validate_raw_trace(workspace, &combined, programs, codes)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&combined)?)?;
    println!(
        "M8 owner plan raw trace: reused={} generated={} report={}",
        programs.len() - missing.len(),
        missing.len(),
        display_relative(workspace, path)
    );
    Ok(())
}

fn validate_raw_trace(
    workspace: &Path,
    raw: &Value,
    programs: &[ProgramSeed],
    codes: &BTreeSet<u32>,
) -> Result<(), Box<dyn Error>> {
    validate_raw_trace_header(workspace, raw, codes)?;
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
    if raw["probes"].as_array().map(Vec::len) != Some(observed.len()) {
        return Err("raw M8 owner-plan trace contains duplicate programs".into());
    }
    let expected = programs
        .iter()
        .map(|program| (program.relative_path.clone(), program.sha256.clone()))
        .collect::<BTreeSet<_>>();
    if observed != expected {
        return Err("raw M8 owner-plan trace does not cover the exact program set".into());
    }
    Ok(())
}

fn validate_raw_trace_header(
    workspace: &Path,
    raw: &Value,
    codes: &BTreeSet<u32>,
) -> Result<(), Box<dyn Error>> {
    if raw["schema"].as_u64() != Some(1)
        || raw["status"].as_str() != Some("draft/report-only")
        || raw["inputs"]["codes"] != json!(codes.iter().copied().collect::<Vec<_>>())
    {
        return Err("raw M8 owner-plan trace has incompatible schema or codes".into());
    }
    for (field, relative) in [
        ("source_sha256", "vendor/typescript-6.0.3/lib/_tsc.js"),
        ("inventory_sha256", "m8-emitter-inventory.json"),
        ("instrumenter_sha256", "crates/oracle/trace-instrument.mjs"),
        ("driver_sha256", "crates/oracle/trace-driver.mjs"),
        ("oracle_driver_sha256", "crates/oracle/driver.mjs"),
        ("program_host_sha256", "crates/oracle/program-host.mjs"),
        ("node_pin_sha256", ".node-version"),
    ] {
        if raw["inputs"][field].as_str() != Some(&sha256_file(&workspace.join(relative))?) {
            return Err(format!("raw M8 owner-plan trace has stale input {relative}").into());
        }
    }
    let pinned_node = fs::read_to_string(workspace.join(".node-version"))?
        .trim()
        .trim_start_matches('v')
        .to_owned();
    if raw["inputs"]["node_version"].as_str() != Some(pinned_node.as_str()) {
        return Err("raw M8 owner-plan trace has stale Node version".into());
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
    comparison_ids: &BTreeSet<String>,
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
                .intersection(comparison_ids)
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
        let relevant = coverage
            .intersection(comparison_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        if relevant.is_empty() {
            continue;
        }
        let symmetric_difference = emitting_coverage.symmetric_difference(&relevant).count();
        let same_fixture = emitting_fixtures.contains(program.fixture.as_str());
        let same_family = probe_families
            .get(&key)
            .is_some_and(|families| families.contains(family));
        let missing_producers = producers.len() - covered_producers.len();
        let missing_comparison = emitting_coverage.difference(&relevant).count();
        candidates.push((
            (
                missing_producers,
                missing_producers > 0 && program.entry_residual,
                !same_fixture,
                !same_family,
                missing_comparison,
                symmetric_difference,
                key.clone(),
            ),
            json!({
                "program": key,
                "role": if program.entry_residual { "entry-residual" } else { "targeted-sibling-probe" },
                "program_sha256": program.sha256,
                "covered_producers": covered_producers,
                "missing_producers": missing_producers,
                "missing_comparison_declarations": missing_comparison,
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

fn validate_scc_decisions(
    cluster_id: &str,
    expected: &[Value],
    decisions: &[SccDecision],
) -> Result<(), Box<dyn Error>> {
    let expected_by_id = expected
        .iter()
        .map(|scc| {
            Ok((
                scc["id"].as_str().ok_or("M8 owner plan SCC lacks id")?,
                scc["member_count"]
                    .as_u64()
                    .ok_or("M8 owner plan SCC lacks member_count")?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?;
    let by_id = decisions
        .iter()
        .map(|decision| (decision.id.as_str(), decision))
        .collect::<BTreeMap<_, _>>();
    if by_id.len() != decisions.len()
        || by_id.keys().copied().collect::<BTreeSet<_>>()
            != expected_by_id.keys().copied().collect::<BTreeSet<_>>()
    {
        return Err(format!(
            "M8 owner-plan review {cluster_id} must decide the exact producer SCC set"
        )
        .into());
    }
    for (id, decision) in by_id {
        if decision.rationale.trim().is_empty()
            || !matches!(decision.decision.as_str(), "singleton" | "keep-separate")
            || (decision.decision == "singleton" && expected_by_id[id] != 1)
            || (decision.decision == "keep-separate" && expected_by_id[id] == 1)
        {
            return Err(format!(
                "M8 owner-plan review {cluster_id} has invalid SCC decision for {id}"
            )
            .into());
        }
    }
    Ok(())
}

fn resolve_rust_boundary_overrides(
    workspace: &Path,
    overrides: &[RustBoundaryOverride],
) -> Result<Vec<Value>, Box<dyn Error>> {
    overrides
        .iter()
        .map(|boundary| {
            if boundary.kind != "native-adjacent"
                || boundary.file.trim().is_empty()
                || boundary.function.trim().is_empty()
                || boundary.rationale.trim().is_empty()
            {
                return Err("invalid reviewed native-adjacent Rust boundary override".into());
            }
            let relative = Path::new(&boundary.file);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir))
            {
                return Err(format!(
                    "reviewed Rust boundary path must stay inside the workspace: {}",
                    boundary.file
                )
                .into());
            }
            let path = workspace.join(relative);
            let source = fs::read_to_string(&path)?;
            let needle = format!("fn {}", boundary.function);
            let line = source
                .lines()
                .position(|line| line.contains(&needle))
                .map(|index| index + 1)
                .ok_or_else(|| {
                    format!(
                        "reviewed Rust boundary {} does not define {}",
                        boundary.file, boundary.function
                    )
                })?;
            Ok(json!({
                "kind": boundary.kind,
                "file": boundary.file,
                "function": boundary.function,
                "line": line,
                "file_sha256": sha256_file(&path)?,
                "rationale": boundary.rationale,
            }))
        })
        .collect()
}

fn audit_reviewed_overrides(workspace: &Path, overrides: &[Value]) -> Result<(), Box<dyn Error>> {
    for boundary in overrides {
        if boundary["kind"].as_str() != Some("native-adjacent") {
            return Err("reviewed Rust boundary has invalid kind".into());
        }
        let file = boundary["file"]
            .as_str()
            .ok_or("reviewed Rust boundary lacks file")?;
        let function = boundary["function"]
            .as_str()
            .ok_or("reviewed Rust boundary lacks function")?;
        let relative = Path::new(file);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(format!(
                "reviewed Rust boundary path must stay inside the workspace: {file}"
            )
            .into());
        }
        let path = workspace.join(relative);
        if boundary["file_sha256"].as_str() != Some(&sha256_file(&path)?) {
            return Err(format!("reviewed Rust boundary {file} is stale").into());
        }
        let needle = format!("fn {function}");
        let line = fs::read_to_string(&path)?
            .lines()
            .position(|line| line.contains(&needle))
            .map(|index| index + 1)
            .ok_or_else(|| format!("reviewed Rust boundary {file} lost function {function}"))?;
        if boundary["line"].as_u64() != Some(line as u64)
            || boundary["rationale"].as_str().is_none_or(str::is_empty)
        {
            return Err(format!("reviewed Rust boundary {file} is malformed").into());
        }
    }
    Ok(())
}

fn audit_plan(workspace: &Path, plan: &Value, require_live: bool) -> Result<(), Box<dyn Error>> {
    if plan["schema"].as_u64() != Some(1) {
        return Err("M8 owner plan must be schema 1".into());
    }
    match plan["status"].as_str() {
        Some("draft") if plan["freeze"].is_null() => {}
        Some("frozen") => {
            let freeze: FreezeRecord = serde_json::from_value(plan["freeze"].clone())
                .map_err(|error| format!("M8 owner plan freeze record is malformed: {error}"))?;
            if !is_full_lower_hex_commit(&freeze.adjudication_commit) {
                return Err(
                    "M8 owner plan freeze anchor must be a full lowercase commit hash".into(),
                );
            }
        }
        Some("draft") => return Err("draft M8 owner plan cannot carry a freeze record".into()),
        _ => return Err("M8 owner plan status must be draft or frozen".into()),
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
        if cluster["review"]["status"].as_str() == Some("reviewed") {
            let selected = cluster["review"]["selected_sibling"]
                .as_str()
                .ok_or("reviewed M8 owner-plan cluster lacks selected_sibling")?;
            if cluster["non_emitting_sibling"]["selection"]["program"].as_str() != Some(selected) {
                return Err(
                    "reviewed M8 owner-plan cluster sibling selection is inconsistent".into(),
                );
            }
            let decisions: Vec<SccDecision> =
                serde_json::from_value(cluster["review"]["scc_decisions"].clone()).map_err(
                    |error| format!("reviewed M8 owner-plan SCC decisions are malformed: {error}"),
                )?;
            validate_scc_decisions(
                cluster["id"].as_str().expect("cluster id validated above"),
                cluster["static_closure"]["producer_sccs"]
                    .as_array()
                    .ok_or("reviewed M8 owner-plan cluster lacks producer_sccs")?,
                &decisions,
            )?;
            if cluster["review"]["owner_slice"]
                .as_str()
                .is_none_or(str::is_empty)
                || cluster["review"]["rationale"]
                    .as_str()
                    .is_none_or(str::is_empty)
            {
                return Err("reviewed M8 owner-plan cluster lacks owner slice or rationale".into());
            }
            let ported = cluster["static_closure"]["ported_boundaries"]
                .as_array()
                .ok_or("reviewed M8 owner-plan cluster lacks ported boundaries")?;
            let overrides = cluster["static_closure"]["reviewed_boundary_overrides"]
                .as_array()
                .ok_or("reviewed M8 owner-plan cluster lacks boundary overrides")?;
            if ported.is_empty() == overrides.is_empty()
                || cluster["review"]["reviewed_boundary_overrides"] != json!(overrides)
            {
                return Err(
                    "reviewed M8 owner-plan cluster has inconsistent Rust boundary review".into(),
                );
            }
            if require_live {
                audit_reviewed_overrides(workspace, overrides)?;
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
                && cluster["static_closure"]["reviewed_boundary_overrides"]
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
    let mut slice_families = BTreeMap::new();
    for cluster in clusters {
        let Some(slice) = cluster["review"]["owner_slice"].as_str() else {
            continue;
        };
        let family = cluster["family"]
            .as_str()
            .expect("cluster family validated above");
        if slice_families
            .insert(slice, family)
            .is_some_and(|previous| previous != family)
        {
            return Err("M8 owner-plan slice crosses owner families".into());
        }
    }
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
    if plan["status"].as_str() == Some("frozen") {
        require_complete_review(plan)?;
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
            let input_path = safe_workspace_path(workspace, path, "M8 owner-plan checked input")?;
            if sha256_file(&input_path)? != expected {
                return Err(format!("M8 owner plan input {path} is stale").into());
            }
        }
        if reviewed > 0 {
            verify_review_input(workspace, plan)?;
        }
    }
    Ok(())
}

fn require_complete_review(plan: &Value) -> Result<(), Box<dyn Error>> {
    let clusters = plan["clusters"]
        .as_array()
        .ok_or("M8 owner plan lacks clusters")?;
    if clusters.is_empty()
        || plan["summary"]["missing_trace_clusters"].as_u64() != Some(0)
        || plan["summary"]["missing_sibling_clusters"].as_u64() != Some(0)
        || plan["summary"]["missing_rust_boundary_clusters"].as_u64() != Some(0)
        || plan["summary"]["reviewed_clusters"].as_u64() != Some(clusters.len() as u64)
        || plan["summary"]["assigned_slices"]
            .as_u64()
            .is_none_or(|count| count == 0)
    {
        return Err(
            "M8 owner plan cannot freeze until every cluster has complete reviewed evidence".into(),
        );
    }
    Ok(())
}

fn verify_review_input(workspace: &Path, plan: &Value) -> Result<(), Box<dyn Error>> {
    let relative = plan["review_input"]["path"]
        .as_str()
        .ok_or("reviewed M8 owner plan lacks review_input path")?;
    let expected = plan["review_input"]["sha256"]
        .as_str()
        .ok_or("reviewed M8 owner plan lacks review_input hash")?;
    let path = safe_workspace_path(workspace, relative, "M8 owner-plan review input")?;
    if sha256_file(&path)? != expected {
        return Err("M8 owner-plan review input is stale".into());
    }
    Ok(())
}

fn verify_frozen_anchor(workspace: &Path, path: &Path, plan: &Value) -> Result<(), Box<dyn Error>> {
    let freeze: FreezeRecord = serde_json::from_value(plan["freeze"].clone())
        .map_err(|error| format!("M8 owner plan freeze record is malformed: {error}"))?;
    let anchor = m8_resolve_git_commit(workspace, &freeze.adjudication_commit)?;
    if anchor != freeze.adjudication_commit {
        return Err("M8 owner plan freeze anchor must name its commit directly".into());
    }
    let head = m8_resolve_git_commit(workspace, "HEAD")?;
    if !m8_git_is_ancestor(workspace, &anchor, &head)? {
        return Err(
            format!("M8 owner plan freeze anchor {anchor} is not an ancestor of HEAD").into(),
        );
    }
    let adjudicated = plan_at(workspace, &anchor, path)?;
    audit_plan(workspace, &adjudicated, false)?;
    if adjudicated["status"].as_str() != Some("draft") || !adjudicated["freeze"].is_null() {
        return Err(format!(
            "M8 owner plan freeze anchor {anchor} does not hold the reviewed draft"
        )
        .into());
    }
    require_complete_review(&adjudicated)?;
    if normalized_frozen_plan(plan)? != adjudicated {
        return Err(format!(
            "M8 owner plan differs from its reviewed draft at freeze anchor {anchor}"
        )
        .into());
    }

    let review_path = adjudicated["review_input"]["path"]
        .as_str()
        .ok_or("anchored M8 owner plan lacks review_input path")?;
    let expected_review = adjudicated["review_input"]["sha256"]
        .as_str()
        .ok_or("anchored M8 owner plan lacks review_input hash")?;
    let review_path = safe_workspace_path(
        workspace,
        review_path,
        "anchored M8 owner-plan review input",
    )?;
    if sha256_bytes(&git_blob_at(workspace, &anchor, &review_path)?) != expected_review {
        return Err(format!(
            "M8 owner plan review input at freeze anchor {anchor} does not match its recorded hash"
        )
        .into());
    }
    Ok(())
}

fn verify_plan_baseline(
    workspace: &Path,
    path: &Path,
    baseline: &str,
    current: &Value,
) -> Result<(), Box<dyn Error>> {
    let baseline_commit = m8_resolve_git_commit(workspace, baseline)?;
    let trusted = plan_at(workspace, &baseline_commit, path)?;
    audit_plan(workspace, &trusted, false)?;
    validate_plan_transition(&trusted, current, &baseline_commit)
}

fn validate_plan_transition(
    trusted: &Value,
    current: &Value,
    baseline_commit: &str,
) -> Result<(), Box<dyn Error>> {
    match (trusted["status"].as_str(), current["status"].as_str()) {
        (Some("draft"), Some("draft")) if trusted == current => Ok(()),
        (Some("draft"), Some("draft")) => {
            Err("M8 owner-plan draft changed against the trusted baseline".into())
        }
        (Some("draft"), Some("frozen")) => {
            if current["freeze"]["adjudication_commit"].as_str() != Some(baseline_commit) {
                return Err(
                    "M8 owner plan first freeze must anchor the trusted baseline commit".into(),
                );
            }
            if &normalized_frozen_plan(current)? != trusted {
                return Err(
                    "M8 owner plan can freeze only the identical trusted reviewed draft".into(),
                );
            }
            Ok(())
        }
        (Some("frozen"), Some("frozen")) if trusted == current => Ok(()),
        (Some("frozen"), Some("frozen")) => {
            Err("frozen M8 owner plan changed against the trusted baseline".into())
        }
        (Some("frozen"), Some("draft")) => {
            Err("M8 owner plan cannot downgrade from frozen to draft".into())
        }
        _ => Err("unsupported M8 owner-plan baseline transition".into()),
    }
}

fn normalized_frozen_plan(plan: &Value) -> Result<Value, Box<dyn Error>> {
    let mut normalized = plan.clone();
    let object = normalized
        .as_object_mut()
        .ok_or("M8 owner plan root must be an object")?;
    object.insert("status".to_owned(), json!("draft"));
    object.remove("freeze");
    Ok(normalized)
}

fn safe_workspace_path(
    workspace: &Path,
    relative: &str,
    what: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!("{what} path must stay inside the workspace").into());
    }
    Ok(workspace.join(relative))
}

fn plan_at(workspace: &Path, commit: &str, path: &Path) -> Result<Value, Box<dyn Error>> {
    let bytes = git_blob_at(workspace, commit, path)?;
    Ok(serde_json::from_slice(&bytes)
        .map_err(|error| format!("cannot parse M8 owner plan at commit {commit}: {error}"))?)
}

fn git_blob_at(workspace: &Path, commit: &str, path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    let relative = repository_relative_path(workspace, path)?;
    let root = git_repository_root(workspace)?;
    tsc_conformance::ratchet::git_blob_optional(&root, commit, &relative)?.ok_or_else(|| {
        format!("cannot read M8 owner-plan input {relative} at commit {commit}").into()
    })
}

fn repository_relative_path(workspace: &Path, path: &Path) -> Result<String, Box<dyn Error>> {
    let root = git_repository_root(workspace)?;
    let canonical_workspace = fs::canonicalize(workspace)?;
    let normalized_path = path
        .strip_prefix(workspace)
        .map(|relative| canonical_workspace.join(relative))
        .unwrap_or_else(|_| path.to_owned());
    let relative = normalized_path.strip_prefix(&root).map_err(|_| {
        format!(
            "M8 owner-plan path {} is outside git root {}",
            path.display(),
            root.display()
        )
    })?;
    if relative
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("M8 owner-plan path cannot contain parent traversal".into());
    }
    Ok(relative.to_string_lossy().replace('\\', "/"))
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
#[path = "../tests/unit/m8_plan/tests.rs"]
mod tests;
