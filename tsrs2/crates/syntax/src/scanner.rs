use crate::SyntaxKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageVariant {
    Standard,
    Jsx,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenRecord {
    pub kind: SyntaxKind,
    pub start: u32,
    pub end: u32,
    pub preceding_line_break: bool,
}

pub fn scan_tokens(_text: &str, _variant: LanguageVariant) -> Vec<TokenRecord> {
    Vec::new()
}
