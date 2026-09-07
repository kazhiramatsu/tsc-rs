//! Explicit current refusal observations; historical mismatch vectors stay history.
//! This is separate from an exact promotion. No rows are registered in this draft.
use super::*;

struct Migration {
    case_id: &'static str,
    old_case_sha256: &'static str,
    old_input_sha256: &'static str,
    new_input_sha256: &'static str,
    retained_vector_sha256: &'static str,
    current_option: &'static str,
    current_observation_sha256: &'static str,
}

static CURRENT: &[Migration] = &[];

pub(super) enum CaseOutcome {
    Compared(H2VectorCaseOutcome),
    MigratedRefusal(ObservedRefusal),
}

// Keep construction in real named functions so the source-closure validator
// follows calls to this module without treating enum variants as functions.
pub(super) fn compared(value: H2VectorCaseOutcome) -> CaseOutcome {
    CaseOutcome::Compared(value)
}

pub(super) fn migrated(value: ObservedRefusal) -> CaseOutcome {
    CaseOutcome::MigratedRefusal(value)
}

pub(super) struct ObservedRefusal {
    pub case_id: String,
    pub current_option: String,
    observation: Value,
}

fn find(case_id: &str) -> Option<&'static Migration> {
    CURRENT.iter().find(|row| row.case_id == case_id)
}

pub(super) fn registered(case_id: &str) -> bool {
    find(case_id).is_some()
}

pub(super) fn count() -> usize {
    CURRENT.len()
}

fn frozen(workspace: &Path, path: &str, expected: &str) -> Result<Value, Box<dyn Error>> {
    let bytes = fs::read(workspace.join(path))?;
    if sha256(&bytes) != expected {
        return Err(failure(format!(
            "refusal migration artifact changed: {path}"
        )));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) fn validate(
    workspace: &Path,
    cases: &[Value],
    listed: &H2LoadedVectorManifest,
) -> Result<(), Box<dyn Error>> {
    if CURRENT.is_empty() {
        return Ok(());
    }
    let census = frozen(
        workspace,
        "ratchets/h2-7de-candidates.v1.json",
        "1af6d75acf8212135a0850c5ff09487a5589de4d0f825ff1f0e9bc8e3f0f141d",
    )?;
    let inputs = frozen(
        workspace,
        "ratchets/h2-7de-candidate-inputs.v1.json",
        "f2e078a6b6d10cd3c6df833584924c18e8f78fe98c1e41621c70e10e215a073a",
    )?;
    let oracle = frozen(
        workspace,
        "ratchets/h2-7de-observations.v1.json",
        "1a1681b2375d27d9012b06e29808aca72aa3e39d1dbc1536b80ba2aadf9e8ce2",
    )?;
    let mut ids = BTreeSet::new();
    for row in CURRENT {
        let old = cases.iter().find(|case| case["case_id"] == row.case_id);
        let new = array(&census, "cases")?
            .iter()
            .find(|case| case["case_id"] == row.case_id);
        let input = array(&inputs, "cases")?
            .iter()
            .find(|case| case["case_id"] == row.case_id);
        let observation = array(&oracle, "cases")?
            .iter()
            .find(|case| case["case_id"] == row.case_id);
        let retained = listed.entries.get(row.case_id);
        let (Some(old), Some(new), Some(input), Some(observation), Some(retained)) =
            (old, new, input, observation, retained)
        else {
            return Err(failure(format!(
                "{}: missing migration identity or retained historical vector",
                row.case_id
            )));
        };
        let owners = match row.current_option {
            "outDir" => json!(["H2.7d", "H2.8a"]),
            "useCaseSensitiveFileNames" => json!(["H2.7d", "H2.8b"]),
            _ => {
                return Err(failure(
                    "unowned migration boundary; requires separate review",
                ))
            }
        };
        if !ids.insert(row.case_id)
            || !listed.vectors_populated
            || h2_6c_de_promotions::find(row.case_id).is_some()
            || old["disposition"] != "admitted-for-execution"
            || old["case_fingerprint_sha256"] != row.old_case_sha256
            || old["observation_input_sha256"] != row.old_input_sha256
            || new["input_sha256"] != row.new_input_sha256
            || observation["input_sha256"] != row.new_input_sha256
            || input["input"]["route"] != "whole-program"
            || old["source"] != new["source"]
            || new["required_slices"] != owners
            || *retained != vectorize_refusal(H2MismatchProfile::H2_6c, "outFile")
            || retained.facet_fingerprint_sha256.as_deref() != Some(row.retained_vector_sha256)
        {
            return Err(failure(format!(
                "{}: invalid pinned refusal migration",
                row.case_id
            )));
        }
    }
    Ok(())
}

/// Both public calls already returned the same UnsupportedCompilerOption variant.
/// Capture what that API actually exposes; unavailable activity/diagnostics/exit
/// are null, never manufactured zero or success observations.
pub(super) fn observe(
    case_id: &str,
    options: &CompilerOptions,
    case_sensitive: bool,
    current_option: &str,
    sink_writes: [usize; 2],
) -> Result<Option<ObservedRefusal>, Box<dyn Error>> {
    let Some(row) = find(case_id) else {
        return Ok(None);
    };
    let active = match current_option {
        "outDir" => options.out_dir.is_some(),
        "useCaseSensitiveFileNames" => !case_sensitive,
        _ => false,
    };
    if row.current_option != current_option
        || !active
        || sink_writes != [0, 0]
        || !options
            .out_file
            .as_deref()
            .is_some_and(|path| !path.is_empty())
    {
        return Err(failure(format!(
            "{case_id}: current typed refusal boundary differs"
        )));
    }
    let observation = json!({
        "case_id": case_id,
        "entry": "ProgramSession::emit_with_reported_diagnostics_for_harness_with_lib_bundle",
        "repetitions": 2,
        "result": {"kind": "DriverError::Emit(EmitFailure::UnsupportedCompilerOption)", "option": current_option},
        "sink_write_counts": sink_writes,
        "actual_input_facets": {"outFile": options.out_file, "outDir": options.out_dir, "case_sensitive": case_sensitive},
        "emit_result": null, "reported_diagnostics": null, "command_exit": null, "runtime_activity": null
    });
    let actual = sha256(serde_json::to_vec(&observation)?);
    if actual != row.current_observation_sha256 {
        return Err(failure(format!(
            "{case_id}: current refusal fingerprint differs: {actual}; observation={observation}"
        )));
    }
    Ok(Some(ObservedRefusal {
        case_id: case_id.to_owned(),
        current_option: current_option.to_owned(),
        observation,
    }))
}

pub(super) type Partition = (
    Vec<Result<H2VectorCaseOutcome, String>>,
    Vec<ObservedRefusal>,
);

pub(super) fn partition(
    results: Vec<Result<CaseOutcome, String>>,
) -> Result<Partition, Box<dyn Error>> {
    let mut compared = Vec::new();
    let mut migrations = Vec::new();
    let mut seen = BTreeSet::new();
    for result in results {
        match result.map_err(failure)? {
            CaseOutcome::Compared(result) => {
                if registered(&result.case_id) {
                    return Err(failure(
                        "registered refusal returned a compared/success result",
                    ));
                }
                compared.push(Ok(result));
            }
            CaseOutcome::MigratedRefusal(result) => {
                if !registered(&result.case_id) || !seen.insert(result.case_id.clone()) {
                    return Err(failure("duplicate or unknown observed refusal migration"));
                }
                println!("H2.6c current refusal migration: {}", result.observation);
                migrations.push(result);
            }
        }
    }
    if seen.len() != CURRENT.len() {
        return Err(failure("a registered refusal migration was not executed"));
    }
    Ok((compared, migrations))
}

pub(super) fn ordinary_manifest(
    listed: &H2LoadedVectorManifest,
) -> HashMap<String, H2VectorDivergence> {
    listed
        .entries
        .iter()
        .filter(|(id, _)| !registered(id))
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect()
}

pub(super) fn validate_ordinary_results(
    results: &[Result<H2VectorCaseOutcome, String>],
    listed: &HashMap<String, H2VectorDivergence>,
) -> Result<(), Box<dyn Error>> {
    if CURRENT.is_empty() {
        return Ok(());
    }
    for result in results {
        let observed = result.as_ref().map_err(|error| failure(error.clone()))?;
        if !observed.deferred
            && !observed.divergence.is_exact()
            && listed.get(&observed.case_id) != Some(&observed.divergence)
        {
            return Err(failure(format!("{}: ordinary residual changed; refusal migrations cannot authorize vector replacement", observed.case_id)));
        }
    }
    Ok(())
}

/// Build the next historical manifest: retain migrated rows verbatim as prior
/// vectors. These are not inserted into the current observed mismatch stream.
pub(super) fn retained_manifest(
    listed: &H2LoadedVectorManifest,
    ordinary: &[(String, H2VectorDivergence)],
) -> Result<Vec<(String, H2VectorDivergence)>, Box<dyn Error>> {
    let mut retained = ordinary.to_vec();
    for row in CURRENT {
        retained.push((
            row.case_id.to_owned(),
            listed
                .entries
                .get(row.case_id)
                .ok_or_else(|| failure("missing retained migration row"))?
                .clone(),
        ));
    }
    retained.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(retained)
}

pub(super) fn adjust_refusal_totals(
    mut projected: BTreeMap<String, u64>,
) -> Result<BTreeMap<String, u64>, Box<dyn Error>> {
    for row in CURRENT {
        let count = projected
            .get_mut("outFile")
            .ok_or_else(|| failure("missing historical outFile bucket"))?;
        *count = count
            .checked_sub(1)
            .ok_or_else(|| failure("migration refusal underflow"))?;
        *projected.entry(row.current_option.to_owned()).or_default() += 1;
    }
    projected.retain(|_, count| *count != 0);
    Ok(projected)
}
