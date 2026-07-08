#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use tsrs2_diags::DiagnosticList;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleRequest {
    pub program_json_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleError {
    pub message: String,
}

pub fn oracle_diags(_program_json: &Path) -> Result<DiagnosticList, OracleError> {
    Ok(Vec::new())
}
