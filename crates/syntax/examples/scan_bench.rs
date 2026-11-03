//! Scan-only benchmark. Build with `cargo build --release -j 1 -p
//! tsc-rs-syntax --example scan_bench`, then run under `/usr/bin/time -l`.
//! Usage: scan_bench [--rounds N] DIR [DIR ...]. The default is ten rounds;
//! divide process instructions by the printed total token count. Input loading
//! is outside the reported scan time, but included in process instructions.
//! Use the same round count and directories when comparing revisions.
//!
//! Include the production scanner so the example can configure its private
//! parser-facing API without exposing a new public API or parsing the input.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::Instant;
use tsc_syntax::{JSDocParsingMode, ParseOptions, ScriptKind, SyntaxKind};

use tsc_syntax as parser;
mod chars {
    pub use tsc_syntax::{is_identifier_part, is_identifier_start};
}
mod keywords {
    pub use tsc_syntax::keyword_kind;
}
#[allow(dead_code)]
#[path = "../src/scanner.rs"]
mod scanner;

// Only the unused TokenScanner wrapper needs this source-file interface.
// The benchmark constructs Scanner directly, so it never constructs an AST.
#[allow(dead_code)]
struct SourceFile {
    language_variant: scanner::LanguageVariant,
    text: String,
}

impl SourceFile {
    fn text(&self) -> &str {
        &self.text
    }
}

fn collect(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name();
            if name == "node_modules" || name.to_string_lossy().starts_with('.') {
                continue;
            }
            if !entry.file_type()?.is_symlink() {
                collect(&entry.path(), files)?;
            }
        }
    } else if path.extension().is_some_and(|extension| {
        matches!(
            extension.to_string_lossy().to_ascii_lowercase().as_str(),
            "ts" | "tsx" | "js" | "jsx" | "mts" | "cts"
        )
    }) {
        files.push(path.to_owned());
    }
    Ok(())
}

fn decode(raw: &[u8]) -> String {
    match raw {
        [0xff, 0xfe, rest @ ..] | [0xfe, 0xff, rest @ ..] => {
            let little = raw[0] == 0xff;
            let units = rest.chunks_exact(2).map(|pair| {
                if little {
                    u16::from_le_bytes([pair[0], pair[1]])
                } else {
                    u16::from_be_bytes([pair[0], pair[1]])
                }
            });
            char::decode_utf16(units)
                .map(|ch| ch.unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect()
        }
        [0xef, 0xbb, 0xbf, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(raw).into_owned(),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rounds = 10u64;
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--rounds" {
            rounds = args.next().ok_or("--rounds requires a count")?.parse()?;
        } else {
            collect(Path::new(&arg), &mut files)?;
        }
    }
    if rounds == 0 || files.is_empty() {
        return Err("usage: scan_bench [--rounds N>0] DIR [DIR ...]".into());
    }
    files.sort();
    files.dedup();
    let sources: Vec<_> = files
        .iter()
        .map(|path| {
            let kind = match path.extension().and_then(|ext| ext.to_str()) {
                Some("tsx") => ScriptKind::Tsx,
                Some("jsx") => ScriptKind::Jsx,
                Some("js") => ScriptKind::Js,
                _ => ScriptKind::Ts,
            };
            std::fs::read(path).map(|raw| (decode(&raw), kind))
        })
        .collect::<Result<_, _>>()?;
    let bytes: usize = sources.iter().map(|(text, _)| text.len()).sum();
    let mut tokens = 0u64;
    let started = Instant::now();
    for _ in 0..rounds {
        for (text, kind) in &sources {
            let options = ParseOptions {
                script_kind: *kind,
                ..ParseOptions::default()
            };
            let variant = match options.language_variant() {
                tsc_syntax::LanguageVariant::Standard => scanner::LanguageVariant::Standard,
                tsc_syntax::LanguageVariant::Jsx => scanner::LanguageVariant::Jsx,
            };
            let mut scanner = scanner::Scanner::new(std::hint::black_box(text), variant);
            scanner.set_jsdoc_parsing_mode(
                JSDocParsingMode::ParseForTypeErrors,
                !options.javascript_file(),
            );
            while std::hint::black_box(scanner.scan()) != SyntaxKind::EndOfFileToken {
                tokens += 1;
            }
            std::hint::black_box(scanner);
        }
    }
    let nanos = started.elapsed().as_nanos();
    println!(
        "files={} bytes={} rounds={} tokens={} nanoseconds={} ns/token={:.4}",
        sources.len(),
        bytes,
        rounds,
        tokens,
        nanos,
        nanos as f64 / tokens as f64
    );
    Ok(())
}
