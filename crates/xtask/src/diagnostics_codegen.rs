//! `cargo xtask codegen diagnostics`: the diagnostic message catalog
//! `crates/diagnostics/src/gen.rs`, generated from TypeScript 7.1's
//! `diagnosticMessages.json` at the vendored native profile.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;

use crate::codegen_common::{find_workspace_root, rustfmt_text, write_generated};

const DIAGNOSTIC_MESSAGES_PATH: &str =
    "vendor/typescript-native/7.1.0-dev-19dadef8/upstream/tsc/internal/diagnostics/diagnosticMessages.json";
const GENERATED_PATH: &str = "crates/diagnostics/src/gen.rs";

#[derive(Clone, Debug)]
struct DiagnosticEntry {
    name: String,
    /// tsgo's `Message.Key()` (diagnostics/generate.go convertPropertyName):
    /// the message text as an identifier, cut to 100 bytes, plus `_code`.
    key: String,
    code: u32,
    category: String,
    text: String,
    reports_unnecessary: bool,
    reports_deprecated: bool,
    elided_in_compatibility_pyramid: bool,
}

#[derive(Clone, Debug)]
struct DiagnosticEntryFields {
    code: u32,
    category: String,
    reports_unnecessary: bool,
    reports_deprecated: bool,
    elided_in_compatibility_pyramid: bool,
}

pub(crate) fn run(check: bool) -> Result<(), Box<dyn Error>> {
    let workspace = find_workspace_root()?;
    let raw = fs::read_to_string(workspace.join(DIAGNOSTIC_MESSAGES_PATH))?;
    let mut entries = parse_diagnostic_catalog(&raw)?;
    entries.sort_by_key(|entry| entry.code);

    let gen_rs = rustfmt_text(&render_diagnostics_gen(&entries)?)?;
    write_generated(&workspace.join(GENERATED_PATH), &gen_rs, check)?;

    if check {
        println!("generated diagnostic messages are up to date");
    } else {
        println!("generated diagnostic messages");
    }
    Ok(())
}

fn parse_diagnostic_catalog(src: &str) -> Result<Vec<DiagnosticEntry>, Box<dyn Error>> {
    let mut json = JsonReader::new(src);
    json.ws();
    json.expect('{')?;
    let mut entries = Vec::new();

    loop {
        json.ws();
        if json.peek() == Some('}') {
            json.bump();
            break;
        }

        let text = json.string()?;
        json.ws();
        json.expect(':')?;
        json.ws();
        let fields = parse_diagnostic_entry(&mut json)?;
        entries.push(DiagnosticEntry {
            name: diagnostic_static_name(&text),
            key: tsgo_message_key(&text, fields.code),
            code: fields.code,
            category: fields.category,
            text,
            reports_unnecessary: fields.reports_unnecessary,
            reports_deprecated: fields.reports_deprecated,
            elided_in_compatibility_pyramid: fields.elided_in_compatibility_pyramid,
        });

        json.ws();
        match json.bump() {
            Some(',') => continue,
            Some('}') => break,
            other => {
                return Err(
                    format!("expected ',' or '}}' after diagnostic entry, got {other:?}").into(),
                )
            }
        }
    }

    let mut names = BTreeMap::<String, u32>::new();
    for entry in &entries {
        if let Some(existing) = names.insert(entry.name.clone(), entry.code) {
            return Err(format!(
                "diagnostic static name collision: {} for codes {} and {}",
                entry.name, existing, entry.code
            )
            .into());
        }
    }

    Ok(entries)
}

fn parse_diagnostic_entry(
    json: &mut JsonReader<'_>,
) -> Result<DiagnosticEntryFields, Box<dyn Error>> {
    json.expect('{')?;
    let mut code = None;
    let mut category = None;
    let mut reports_unnecessary = false;
    let mut reports_deprecated = false;
    let mut elided = false;

    loop {
        json.ws();
        if json.peek() == Some('}') {
            json.bump();
            break;
        }

        let key = json.string()?;
        json.ws();
        json.expect(':')?;
        json.ws();
        match key.as_str() {
            "code" => code = Some(json.number()? as u32),
            "category" => category = Some(json.string()?),
            "reportsUnnecessary" => reports_unnecessary = json.boolean()?,
            "reportsDeprecated" => reports_deprecated = json.boolean()?,
            "elidedInCompatabilityPyramid" => elided = json.boolean()?,
            _ => json.skip_value()?,
        }
        json.ws();
        match json.bump() {
            Some(',') => continue,
            Some('}') => break,
            other => {
                return Err(
                    format!("expected ',' or '}}' in diagnostic entry, got {other:?}").into(),
                )
            }
        }
    }

    Ok(DiagnosticEntryFields {
        code: code.ok_or("diagnostic entry missing code")?,
        category: category.ok_or("diagnostic entry missing category")?,
        reports_unnecessary,
        reports_deprecated,
        elided_in_compatibility_pyramid: elided,
    })
}

fn render_diagnostic_static(
    out: &mut String,
    entry: &DiagnosticEntry,
) -> Result<(), Box<dyn Error>> {
    writeln!(
        out,
        "pub static {}: DiagnosticMessage = DiagnosticMessage {{",
        entry.name
    )?;
    writeln!(out, "    code: {},", entry.code)?;
    writeln!(out, "    key: {:?},", entry.key)?;
    writeln!(out, "    category: DiagnosticCategory::{},", entry.category)?;
    writeln!(out, "    text: {:?},", entry.text)?;
    writeln!(
        out,
        "    reports_unnecessary: {},",
        entry.reports_unnecessary
    )?;
    writeln!(out, "    reports_deprecated: {},", entry.reports_deprecated)?;
    writeln!(
        out,
        "    elided_in_compatibility_pyramid: {},",
        entry.elided_in_compatibility_pyramid
    )?;
    writeln!(out, "}};")?;
    Ok(())
}

fn render_diagnostics_gen(entries: &[DiagnosticEntry]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen diagnostics`. Do not edit by hand."
    )?;
    writeln!(out, "// Source: {DIAGNOSTIC_MESSAGES_PATH}")?;
    writeln!(out)?;
    writeln!(out, "use super::{{DiagnosticCategory, DiagnosticMessage}};")?;
    writeln!(out)?;

    for entry in entries {
        render_diagnostic_static(&mut out, entry)?;
    }

    writeln!(out)?;
    writeln!(
        out,
        "pub static ALL_BY_CODE: &[(u32, &DiagnosticMessage)] = &["
    )?;
    for entry in entries {
        writeln!(out, "    ({}, &{}),", entry.code, entry.name)?;
    }
    writeln!(out, "];")?;
    writeln!(out)?;
    writeln!(out, "#[cfg(test)]")?;
    writeln!(out, "#[path = \"../tests/unit/gen/tests.rs\"]")?;
    writeln!(out, "mod tests;")?;

    Ok(out)
}

/// tsgo diagnostics/generate.go convertPropertyName: the message key that
/// `ast.Diagnostic.MessageKey()` reports and the build info records. `*`,
/// `/` and `:` become `_Asterisk`, `_Slash` and `_Colon`; every other
/// non-letter, non-digit becomes `_`; underscore runs collapse; a leading
/// underscore goes unless a digit follows; a trailing underscore goes; the
/// key is the first 100 bytes of that name plus `_` and the code.
fn tsgo_message_key(message: &str, code: u32) -> String {
    let mut name = String::with_capacity(message.len() + 8);
    for ch in message.chars() {
        match ch {
            '*' => name.push_str("_Asterisk"),
            '/' => name.push_str("_Slash"),
            ':' => name.push_str("_Colon"),
            _ if ch.is_alphabetic() || ch.is_numeric() => name.push(ch),
            _ => name.push('_'),
        }
    }
    let mut collapsed = String::with_capacity(name.len());
    let mut previous_was_underscore = false;
    for ch in name.chars() {
        if ch == '_' {
            if !previous_was_underscore {
                collapsed.push('_');
            }
            previous_was_underscore = true;
        } else {
            collapsed.push(ch);
            previous_was_underscore = false;
        }
    }
    let mut var_name = collapsed;
    if var_name.starts_with('_')
        && var_name[1..]
            .chars()
            .next()
            .is_some_and(|next| !next.is_ascii_digit())
    {
        var_name.remove(0);
    }
    if var_name.ends_with('_') {
        var_name.pop();
    }
    let mut key = var_name;
    if key.len() > 100 {
        let mut cut = 100;
        while !key.is_char_boundary(cut) {
            cut -= 1;
        }
        key.truncate(cut);
    }
    key.push('_');
    key.push_str(&code.to_string());
    key
}

fn diagnostic_static_name(message: &str) -> String {
    let mut out = String::new();
    let mut previous_was_separator = false;

    for ch in message.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            previous_was_separator = false;
        } else if !previous_was_separator {
            out.push('_');
            previous_was_separator = true;
        }
    }

    let mut out = out.trim_matches('_').to_owned();
    if out.is_empty() {
        out = "Diagnostic".to_owned();
    }
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if is_rust_keyword(&out) {
        out.insert_str(0, "r#");
    }
    out
}

fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
    )
}

struct JsonReader<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> JsonReader<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            bytes: src.as_bytes(),
            index: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.bytes.get(self.index).copied().map(char::from)
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek();
        if ch.is_some() {
            self.index += 1;
        }
        ch
    }

    fn expect(&mut self, expected: char) -> Result<(), Box<dyn Error>> {
        match self.bump() {
            Some(actual) if actual == expected => Ok(()),
            actual => Err(format!(
                "expected {expected:?}, got {actual:?} at byte {}",
                self.index
            )
            .into()),
        }
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.index += 1;
        }
    }

    fn string(&mut self) -> Result<String, Box<dyn Error>> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            let ch = self.bump().ok_or("unterminated JSON string")?;
            match ch {
                '"' => return Ok(out),
                '\\' => {
                    let escaped = self.bump().ok_or("unterminated JSON escape")?;
                    match escaped {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000C}'),
                        'u' => {
                            let mut hex = String::new();
                            for _ in 0..4 {
                                hex.push(self.bump().ok_or("short JSON unicode escape")?);
                            }
                            let code = u32::from_str_radix(&hex, 16)?;
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        other => return Err(format!("unknown JSON escape \\{other}").into()),
                    }
                }
                _ if ch.is_ascii() => out.push(ch),
                _ => {
                    self.index -= 1;
                    let rest = std::str::from_utf8(&self.bytes[self.index..])?;
                    let decoded = rest.chars().next().ok_or("invalid UTF-8 in JSON string")?;
                    self.index += decoded.len_utf8();
                    out.push(decoded);
                }
            }
        }
    }

    fn number(&mut self) -> Result<i64, Box<dyn Error>> {
        let start = self.index;
        while matches!(self.peek(), Some('-' | '+' | '0'..='9')) {
            self.index += 1;
        }
        Ok(std::str::from_utf8(&self.bytes[start..self.index])?.parse()?)
    }

    fn boolean(&mut self) -> Result<bool, Box<dyn Error>> {
        if self.bytes[self.index..].starts_with(b"true") {
            self.index += 4;
            Ok(true)
        } else if self.bytes[self.index..].starts_with(b"false") {
            self.index += 5;
            Ok(false)
        } else {
            Err(format!("expected JSON boolean at byte {}", self.index).into())
        }
    }

    fn skip_value(&mut self) -> Result<(), Box<dyn Error>> {
        self.ws();
        match self.peek() {
            Some('"') => {
                self.string()?;
            }
            Some('{') => self.skip_balanced('{', '}')?,
            Some('[') => self.skip_balanced('[', ']')?,
            Some('t') | Some('f') => {
                self.boolean()?;
            }
            Some('n') if self.bytes[self.index..].starts_with(b"null") => {
                self.index += 4;
            }
            Some('-' | '+' | '0'..='9') => {
                self.number()?;
            }
            other => {
                return Err(
                    format!("unexpected JSON value {other:?} at byte {}", self.index).into(),
                )
            }
        }
        Ok(())
    }

    fn skip_balanced(&mut self, open: char, close: char) -> Result<(), Box<dyn Error>> {
        self.expect(open)?;
        let mut depth = 1usize;
        while depth > 0 {
            match self.bump() {
                Some('"') => {
                    self.index -= 1;
                    self.string()?;
                }
                Some(ch) if ch == open => depth += 1,
                Some(ch) if ch == close => depth -= 1,
                Some(_) => {}
                None => return Err("unterminated JSON container".into()),
            }
        }
        Ok(())
    }
}
