//! M8 paired batch (observation only, never merged): parse-only benchmark, the tsc-rs twin of tsrs's
//! `tsrs_parser/examples/parse_batch.rs`. The files' text snapshots are made once, outside the rounds (the loader's
//! work in the CLI); each parse uses the CLI's JSDoc mode (ParseForTypeErrors) and drops its tree afterwards.
//! Usage: parse_batch [--rounds N] DIR [DIR ...] under `/usr/bin/time -l`; difference two round counts:
//! (instructions(R2) - instructions(R1)) / ((R2 - R1) * nodes per round).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tsc_diagnostics::{DocumentVersion, TextSnapshot};
use tsc_syntax::{parse_source_file_from_snapshot, JSDocParsingMode, ParseOptions, ScriptKind};

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
        return Err("usage: parse_batch [--rounds N>0] DIR [DIR ...]".into());
    }
    files.sort();
    files.dedup();
    let sources: Vec<(String, Arc<TextSnapshot>, ScriptKind)> = files
        .iter()
        .map(|path| {
            let name = path.to_string_lossy().into_owned();
            let kind = ScriptKind::ensure_from_file_name(&name);
            std::fs::read(path).map(|raw| {
                (name, TextSnapshot::new(decode(&raw), DocumentVersion::default()), kind)
            })
        })
        .collect::<Result<_, _>>()?;
    let bytes: usize = sources.iter().map(|(_, text, _)| text.text().len()).sum();
    let mut nodes = 0u64;
    let started = std::time::Instant::now();
    for _ in 0..rounds {
        for (name, snapshot, kind) in &sources {
            let options = ParseOptions {
                script_kind: *kind,
                js_doc_parsing_mode: JSDocParsingMode::ParseForTypeErrors,
                ..ParseOptions::default()
            };
            let source = parse_source_file_from_snapshot(
                name.as_str(),
                Arc::clone(std::hint::black_box(snapshot)),
                options,
                None,
            );
            nodes += source.node_count() as u64;
            std::hint::black_box(&source);
        }
    }
    let nanos = started.elapsed().as_nanos();
    println!(
        "files={} bytes={} rounds={} nodes={} nanoseconds={} ns/node={:.4}",
        sources.len(),
        bytes,
        rounds,
        nodes,
        nanos,
        nanos as f64 / nodes as f64
    );
    Ok(())
}
