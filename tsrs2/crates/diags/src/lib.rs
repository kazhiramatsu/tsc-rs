#![forbid(unsafe_code)]

pub mod line_map;

pub use line_map::{
    compute_line_map, compute_line_starts, get_line_and_character_of_position, LineAndCharacter,
    LineMap,
};

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
