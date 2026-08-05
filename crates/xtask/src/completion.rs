use std::error::Error;

use serde::{Deserialize, Serialize};

pub(crate) const COMPLETION_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompletionArgs {
    pub(crate) require_done: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct CompletionProbe {
    pub(crate) ready: bool,
    pub(crate) detail: String,
}

impl CompletionProbe {
    pub(crate) fn new(ready: bool, detail: impl Into<String>) -> Self {
        Self {
            ready,
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompletionInputs {
    pub(crate) all_corpus_fp_zero: CompletionProbe,
    pub(crate) exact_scope: CompletionProbe,
    pub(crate) supported_t0_t3: CompletionProbe,
    pub(crate) supported_t4: CompletionProbe,
    pub(crate) syntactic_in_scope: CompletionProbe,
    pub(crate) zero_escapes: CompletionProbe,
    pub(crate) rust_ledger: CompletionProbe,
    pub(crate) declaration_converse: CompletionProbe,
    pub(crate) b1_b4_evidence: CompletionProbe,
    pub(crate) full_corpus_invariants: CompletionProbe,
    pub(crate) m9_steady_state: CompletionProbe,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct CompletionRow {
    pub(crate) number: u8,
    pub(crate) name: String,
    pub(crate) ready: bool,
    pub(crate) detail: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct CompletionReport {
    pub(crate) schema: u32,
    pub(crate) complete: bool,
    pub(crate) ready_rows: usize,
    pub(crate) total_rows: usize,
    pub(crate) rows: Vec<CompletionRow>,
}

pub(crate) fn parse_args(
    args: impl Iterator<Item = String>,
) -> Result<CompletionArgs, Box<dyn Error>> {
    let mut require_done = false;
    for arg in args {
        match arg.as_str() {
            "--require-done" => require_done = true,
            _ => return Err(format!("unexpected completion argument: {arg}").into()),
        }
    }
    Ok(CompletionArgs { require_done })
}

pub(crate) fn build_report(inputs: CompletionInputs) -> CompletionReport {
    let rows = [
        ("all-corpus-fp-zero", inputs.all_corpus_fp_zero),
        ("exact-scope", inputs.exact_scope),
        ("supported-t0-t3", inputs.supported_t0_t3),
        ("supported-t4", inputs.supported_t4),
        ("syntactic-in-scope", inputs.syntactic_in_scope),
        ("zero-escapes", inputs.zero_escapes),
        ("rust-function-ledger", inputs.rust_ledger),
        ("declaration-converse", inputs.declaration_converse),
        ("b1-b4-evidence", inputs.b1_b4_evidence),
        ("full-corpus-invariants", inputs.full_corpus_invariants),
        ("m9-steady-state", inputs.m9_steady_state),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (name, probe))| CompletionRow {
        number: (index + 1) as u8,
        name: name.to_owned(),
        ready: probe.ready,
        detail: probe.detail,
    })
    .collect::<Vec<_>>();
    let ready_rows = rows.iter().filter(|row| row.ready).count();
    CompletionReport {
        schema: COMPLETION_SCHEMA,
        complete: ready_rows == rows.len(),
        ready_rows,
        total_rows: rows.len(),
        rows,
    }
}

pub(crate) fn enforce(report: &CompletionReport, require_done: bool) -> Result<(), Box<dyn Error>> {
    if !require_done || report.complete {
        return Ok(());
    }
    let pending = report
        .rows
        .iter()
        .filter(|row| !row.ready)
        .map(|row| format!("{}:{}", row.number, row.name))
        .collect::<Vec<_>>();
    Err(format!(
        "completion gate is not done; pending rows: {}",
        pending.join(", ")
    )
    .into())
}

#[cfg(test)]
#[path = "../tests/unit/completion/tests.rs"]
mod tests;
