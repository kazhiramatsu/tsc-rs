//! Current-runtime exact promotions of unchanged H2.6c inputs.
//! No rows are admitted by this draft. Populate only after both the historical
//! comparator and the separate current original-input full-tuple runner pass.
use super::*;

pub(super) struct Promotion {
    pub case_id: &'static str,
    old_case_sha256: &'static str,
    old_input_sha256: &'static str,
    new_input_sha256: &'static str,
    old_refused_option: &'static str,
    pub declaration_members: u64,
    c_requests: u64,
    d_requests: u64,
    e_requests: u64,
}

// Empty intentionally: D/E production comparisons are not complete.
static CURRENT: &[Promotion] = &[];

pub(super) fn find(case_id: &str) -> Option<&'static Promotion> {
    CURRENT.iter().find(|row| row.case_id == case_id)
}

fn frozen(workspace: &Path, path: &str, expected: &str) -> Result<Value, Box<dyn Error>> {
    let bytes = fs::read(workspace.join(path))?;
    if sha256(&bytes) != expected {
        return Err(failure(format!("D/E promotion input changed: {path}")));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) fn validate(
    workspace: &Path,
    old_cases: &[Value],
    old_members: &HashMap<String, u64>,
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
        let old = old_cases.iter().find(|case| case["case_id"] == row.case_id);
        let new = array(&census, "cases")?
            .iter()
            .find(|case| case["case_id"] == row.case_id);
        let input = array(&inputs, "cases")?
            .iter()
            .find(|case| case["case_id"] == row.case_id);
        let observation = array(&oracle, "cases")?
            .iter()
            .find(|case| case["case_id"] == row.case_id);
        let (Some(old), Some(new), Some(input), Some(observation)) = (old, new, input, observation)
        else {
            return Err(failure(format!(
                "{}: incomplete old/new promotion join",
                row.case_id
            )));
        };
        let d = row.d_requests == 1;
        let e = row.e_requests == 1;
        let owners = match (d, e) {
            (true, false) => json!(["H2.7d"]),
            (false, true) => json!(["H2.7e"]),
            (true, true) => json!(["H2.7d", "H2.7e"]),
            _ => return Err(failure("promotion must request D or E")),
        };
        if !ids.insert(row.case_id)
            || row.d_requests > 1
            || row.e_requests > 1
            || row.c_requests > 1
            || old["disposition"] != "admitted-for-execution"
            || old["case_fingerprint_sha256"] != row.old_case_sha256
            || old["observation_input_sha256"] != row.old_input_sha256
            || new["input_sha256"] != row.new_input_sha256
            || observation["input_sha256"] != row.new_input_sha256
            || new["required_slices"] != owners
            || input["input"]["route"] != "whole-program"
            || old["source"] != new["source"]
            || old_members.get(row.case_id).copied().unwrap_or(0) != 0
            || row.old_refused_option != if e { "declarationMap" } else { "outFile" }
        {
            return Err(failure(format!(
                "{}: pinned promotion identity/owner changed",
                row.case_id
            )));
        }
    }
    Ok(())
}

pub(super) fn pinned_request(case_id: &str, slice: H2RuntimeSlice) -> bool {
    find(case_id).is_some()
        && matches!(
            slice,
            H2RuntimeSlice::H2_7c | H2RuntimeSlice::H2_7d | H2RuntimeSlice::H2_7e
        )
}

pub(super) fn validate_activity(
    case_id: &str,
    outcome: &EmitOutcome,
) -> Result<(), Box<dyn Error>> {
    if let Some(row) = find(case_id) {
        for (slice, expected) in [
            (H2RuntimeSlice::H2_7c, row.c_requests),
            (H2RuntimeSlice::H2_7d, row.d_requests),
            (H2RuntimeSlice::H2_7e, row.e_requests),
        ] {
            let actual = outcome.h2_activity().runtime_slice(slice);
            if actual != expected {
                return Err(failure(format!(
                    "{case_id}: pinned {} request count expected {expected}, actual {actual}",
                    slice.name()
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn declaration_members_total() -> u64 {
    293 + CURRENT
        .iter()
        .map(|row| row.declaration_members)
        .sum::<u64>()
}

pub(super) fn promoted_count() -> usize {
    CURRENT.len()
}

pub(super) fn adjusted_refusals(
    mut projected: BTreeMap<String, u64>,
) -> Result<BTreeMap<String, u64>, Box<dyn Error>> {
    for row in CURRENT {
        let count = projected
            .get_mut(row.old_refused_option)
            .ok_or_else(|| failure("missing historical refusal bucket"))?;
        *count = count
            .checked_sub(1)
            .ok_or_else(|| failure("promotion refusal underflow"))?;
    }
    projected.retain(|_, count| *count != 0);
    Ok(projected)
}

/// Keep the old comparator's exact facets authoritative, even in manifest-write
/// mode. A removed option refusal is never replaced by newly exposed mismatches.
pub(super) fn validate_results(
    results: &[Result<H2VectorCaseOutcome, String>],
    listed: &HashMap<String, H2VectorDivergence>,
) -> Result<(), Box<dyn Error>> {
    if CURRENT.is_empty() {
        return Ok(());
    }
    let mut seen = BTreeSet::new();
    for result in results {
        let outcome = result.as_ref().map_err(|error| failure(error.clone()))?;
        if let Some(row) = find(&outcome.case_id) {
            if !seen.insert(row.case_id)
                || outcome.deferred
                || !outcome.divergence.is_exact()
                || !outcome.divergence.mismatch_vector.is_empty()
            {
                return Err(failure(format!(
                    "{}: promotion is not exact under unchanged H2.6c comparator",
                    row.case_id
                )));
            }
            if let Some(previous) = listed.get(row.case_id) {
                if *previous != vectorize_refusal(H2MismatchProfile::H2_6c, row.old_refused_option)
                {
                    return Err(failure(format!(
                        "{}: promotion did not start from its pinned typed refusal",
                        row.case_id
                    )));
                }
            }
        } else if !outcome.deferred
            && !outcome.divergence.is_exact()
            && listed.get(&outcome.case_id) != Some(&outcome.divergence)
        {
            return Err(failure(format!(
                "{}: non-promoted residual changed (also forbidden in write mode)",
                outcome.case_id
            )));
        }
    }
    if seen.len() != CURRENT.len() {
        return Err(failure("promotion row was not executed"));
    }
    Ok(())
}
