//! M8 bind batch (observation only, never merged): the binder (`bind_source_file`, the CLI's
//! `BinderWorker::bind_source_file`) over parsed files of DIR, each file bound once per round and the binder
//! dropped. The files are parsed once before the rounds (ParseForTypeErrors, as the CLI). `--mode noop` runs the
//! same loop without binding. Per round = (I(R2) - I(R1)) / (R2 - R1), or the difference of two modes' totals over
//! the rounds. `--stride K` keeps every K-th file.
//! Usage: bind_batch [--rounds N] [--mode bind|noop] [--stride K] DIR ...

use std::hint::black_box;
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
    let mut mode = String::from("bind");
    let mut stride = 2usize;
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--rounds" {
            rounds = args.next().ok_or("--rounds requires a count")?.parse()?;
        } else if arg == "--mode" {
            mode = args.next().ok_or("--mode requires bind|noop")?;
        } else if arg == "--stride" {
            stride = args.next().ok_or("--stride requires a count")?.parse()?;
        } else {
            collect(Path::new(&arg), &mut files)?;
        }
    }
    if rounds == 0 || stride == 0 || files.is_empty() {
        return Err(
            "usage: bind_batch [--rounds N>0] [--mode bind|noop] [--stride K>0] DIR ...".into(),
        );
    }
    files.sort();
    files.dedup();
    let mut sources = Vec::new();
    for path in files.iter().step_by(stride) {
        let name = path.to_string_lossy().into_owned();
        let kind = ScriptKind::ensure_from_file_name(&name);
        let snapshot = TextSnapshot::new(decode(&std::fs::read(path)?), DocumentVersion::default());
        let options = ParseOptions {
            script_kind: kind,
            js_doc_parsing_mode: JSDocParsingMode::ParseForTypeErrors,
            ..ParseOptions::default()
        };
        sources.push(parse_source_file_from_snapshot(
            name.as_str(),
            Arc::clone(&snapshot),
            options,
            None,
        ));
    }
    let nodes: u64 = sources
        .iter()
        .map(|source| source.node_count() as u64)
        .sum();
    let mut symbols = 0u64;
    let started = std::time::Instant::now();
    match mode.as_str() {
        "bind" => {
            for _ in 0..rounds {
                for source in &sources {
                    let binder = tsc_binder::bind_source_file(black_box(source));
                    symbols += binder.symbols.len() as u64;
                    black_box(&binder);
                }
            }
        }
        "noop" => {
            for _ in 0..rounds {
                for source in &sources {
                    black_box(black_box(source).node_count());
                }
            }
        }
        other => return Err(format!("unknown mode {other}").into()),
    }
    let nanos = started.elapsed().as_nanos();
    println!(
        "mode={mode} files={} rounds={rounds} nodes/round={nodes} symbols={symbols} nanoseconds={nanos} ns/node={:.4}",
        sources.len(),
        nanos as f64 / (nodes as f64 * rounds as f64)
    );
    Ok(())
}
