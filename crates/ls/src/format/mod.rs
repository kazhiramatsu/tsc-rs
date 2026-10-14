//! tsgo `format/`: the formatter. Its rules apply between pairs of tokens;
//! the span worker walks a span's nodes with the formatting scanner,
//! applies them and indents the lines.

mod api;
mod context;
mod file;
mod indent;
mod rule;
mod rulecontext;
mod rules;
mod rulesmap;
mod scanner;
mod span;
mod util;

pub(crate) use api::{
    format_document, format_on_closing_curly, format_on_enter, format_on_opening_curly,
    format_on_semicolon, format_selection, TextChange,
};
pub(crate) use file::File;
