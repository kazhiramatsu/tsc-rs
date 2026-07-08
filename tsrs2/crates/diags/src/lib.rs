#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticCategory {
    Warning,
    Error,
    Suggestion,
    Message,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub file_name: Option<String>,
    pub start: Option<u32>,
    pub length: Option<u32>,
    pub code: u32,
    pub category: DiagnosticCategory,
    pub message: String,
}

pub type DiagnosticList = Vec<Diagnostic>;
