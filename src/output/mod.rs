//! Plain (non-pretty) diagnostic formatting — byte-identical to
//! `ts.formatDiagnostic`: `{relPath}({line},{col}): {category} TS{code}: {text}\n`
//! with message-chain children indented two spaces per depth on their own lines.

use crate::diagnostics::{Diagnostic, MessageChain};
use crate::text::SourceText;

pub struct OutputFile<'a> {
    /// The path exactly as it should print (root files keep their command-line
    /// spelling; see `relativize`).
    pub display_name: String,
    pub text: &'a SourceText,
}

fn flatten_into(out: &mut String, chain: &MessageChain, depth: usize) {
    for child in &chain.next {
        out.push('\n');
        for _ in 0..depth {
            out.push_str("  ");
        }
        out.push_str(&child.text);
        flatten_into(out, child, depth + 1);
    }
}

pub fn format_diagnostics(diags: &[Diagnostic], files: &[OutputFile]) -> String {
    let mut out = String::new();
    for d in diags {
        // Suggestions are editor-only (tsc getSuggestionDiagnostics); the
        // compile/--noEmit diagnostic list contains only errors and warnings.
        if d.category() == crate::diagnostics::Category::Suggestion {
            continue;
        }
        if let Some(fi) = d.file {
            let f = &files[fi];
            let (line, col) = f.text.line_col(d.start);
            out.push_str(&f.display_name);
            out.push('(');
            out.push_str(&line.to_string());
            out.push(',');
            out.push_str(&col.to_string());
            out.push_str("): ");
        }
        out.push_str(d.category().name());
        out.push_str(" TS");
        out.push_str(&d.code().to_string());
        out.push_str(": ");
        out.push_str(&d.message.text);
        flatten_into(&mut out, &d.message, 1);
        out.push('\n');
    }
    out
}

/// tsc convertToRelativePath: non-absolute names print verbatim; absolute names
/// are made relative to `cwd` (component-wise, case-insensitive on macOS).
pub fn relativize(name: &str, cwd: &str, case_insensitive: bool) -> String {
    if !name.starts_with('/') {
        return name.to_string();
    }
    let canon = |s: &str| {
        if case_insensitive {
            s.to_lowercase()
        } else {
            s.to_string()
        }
    };
    let name_parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty()).collect();
    let cwd_parts: Vec<&str> = cwd.split('/').filter(|p| !p.is_empty()).collect();
    let mut common = 0;
    while common < name_parts.len()
        && common < cwd_parts.len()
        && canon(name_parts[common]) == canon(cwd_parts[common])
    {
        common += 1;
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..cwd_parts.len() {
        parts.push("..".to_string());
    }
    for p in &name_parts[common..] {
        parts.push((*p).to_string());
    }
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

// ── Phase-2 structured JSON output ──────────────────────────────────────────
// Emits diagnostics in the same shape as difftest/diag_oracle.js so tsrs's
// checker output can be diffed against tsc's structured diagnostics
// (messageChain + relatedInformation).

fn json_escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn tsc_category(c: crate::diagnostics::Category) -> u32 {
    // tsc DiagnosticCategory: Warning=0, Error=1, Suggestion=2, Message=3.
    use crate::diagnostics::Category::*;
    match c {
        Warning => 0,
        Error => 1,
        Suggestion => 2,
        Message => 3,
    }
}

fn json_chain(chain: &MessageChain, is_top: bool, out: &mut String) {
    // Mirror the oracle: a top-level flat message serializes as `{ "text": … }`;
    // any chain node (or nested node) carries code/category/next as well.
    if is_top && chain.next.is_empty() {
        out.push_str("{\"text\":");
        json_escape(&chain.text, out);
        out.push('}');
        return;
    }
    out.push_str("{\"text\":");
    json_escape(&chain.text, out);
    out.push_str(",\"code\":");
    out.push_str(&chain.code.to_string());
    out.push_str(",\"category\":");
    out.push_str(&tsc_category(chain.category).to_string());
    out.push_str(",\"next\":[");
    for (i, ch) in chain.next.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_chain(ch, false, out);
    }
    out.push_str("]}");
}

fn json_loc(file: Option<usize>, start: u32, length: u32, files: &[OutputFile], out: &mut String) {
    match file {
        Some(fi) => {
            let f = &files[fi];
            let (sl, sc) = f.text.line_col(start);
            let (el, ec) = f.text.line_col(start + length);
            // tsc/LSP positions are UTF-16; emit those as `start`/`length` for
            // drop-in compatibility, and keep the native UTF-8 byte offsets
            // under `byteStart`/`byteLength` for tools indexing the raw buffer.
            let u16_start = f.text.utf16_offset(start);
            let u16_end = f.text.utf16_offset(start + length);
            let base = f.display_name.rsplit('/').next().unwrap_or(&f.display_name);
            out.push_str("\"file\":");
            json_escape(base, out);
            out.push_str(&format!(
                ",\"start\":{},\"length\":{},\"byteStart\":{},\"byteLength\":{},\"startLine\":{},\"startCol\":{},\"endLine\":{},\"endCol\":{}",
                u16_start, u16_end - u16_start, start, length, sl, sc, el, ec
            ));
        }
        None => out.push_str(
            "\"file\":null,\"start\":null,\"length\":null,\"byteStart\":null,\"byteLength\":null,\"startLine\":null,\"startCol\":null,\"endLine\":null,\"endCol\":null",
        ),
    }
}

pub fn format_diagnostics_json(diags: &[Diagnostic], files: &[OutputFile]) -> String {
    let mut out = String::new();
    // Envelope mirrors difftest/diag_oracle.js. tsrs does not emit yet, so the
    // emit fields are placeholders until noEmit:false support lands.
    out.push_str("{\"emittedFiles\":[],\"emitSkipped\":false,\"diagnostics\":[");
    for (i, d) in diags.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"code\":");
        out.push_str(&d.code().to_string());
        out.push_str(",\"category\":");
        out.push_str(&tsc_category(d.category()).to_string());
        // `reportsUnnecessary`/`reportsDeprecated` are static properties of the
        // diagnostic message (faded unused-code hints / deprecation strikes); read
        // them from the message catalog by code. `source` is not tracked yet.
        let (ru, rd) = crate::diagnostics::by_code(d.code())
            .map(|m| (m.reports_unnecessary, m.reports_deprecated))
            .unwrap_or((false, false));
        out.push_str(",\"source\":null,\"reportsUnnecessary\":");
        out.push_str(if ru { "true" } else { "false" });
        out.push_str(",\"reportsDeprecated\":");
        out.push_str(if rd { "true" } else { "false" });
        out.push(',');
        json_loc(d.file, d.start, d.length, files, &mut out);
        out.push_str(",\"message\":");
        json_chain(&d.message, true, &mut out);
        out.push_str(",\"related\":[");
        for (j, r) in d.related.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str("{\"code\":");
            out.push_str(&r.message.code.to_string());
            out.push_str(",\"category\":");
            out.push_str(&tsc_category(r.message.category).to_string());
            out.push(',');
            json_loc(r.file, r.start, r.length, files, &mut out);
            out.push_str(",\"message\":");
            json_chain(&r.message, true, &mut out);
            out.push('}');
        }
        out.push_str("]}");
    }
    out.push_str("]}\n");
    out
}
