//! Parse-only benchmark: `nice -n 20 cargo build --release -j 1 -p
//! tsc-rs-syntax --example parse_bench`, then `/usr/bin/time -l
//! target/release/examples/parse_bench DIR [DIR ...]`.
//!
//! Inputs are sorted and loaded before timing. Reported nanoseconds cover
//! parse_source_file (including snapshot construction), excluding file IO and
//! tree destruction. Process instructions include loading and destruction.
//! Script kinds come from file names; all other parse options are defaults.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tsc_syntax::{parse_source_file, ParseOptions, ScriptKind};

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

// Match the loader's BOM handling and reject unpaired UTF-16 surrogates.
fn decode(raw: &[u8]) -> Result<String, std::string::FromUtf16Error> {
    Ok(match raw {
        [0xff, 0xfe, rest @ ..] | [0xfe, 0xff, rest @ ..] => {
            let little = raw[0] == 0xff;
            let units: Vec<_> = rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&pair| {
                    if little {
                        u16::from_le_bytes(pair)
                    } else {
                        u16::from_be_bytes(pair)
                    }
                })
                .collect();
            String::from_utf16(&units)?
        }
        [0xef, 0xbb, 0xbf, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(raw).into_owned(),
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    for directory in std::env::args().skip(1) {
        collect(Path::new(&directory), &mut files)?;
    }
    if files.is_empty() {
        return Err("usage: parse_bench DIR [DIR ...] (no source files found)".into());
    }
    files.sort();
    files.dedup();
    let sources: Vec<_> = files
        .iter()
        .map(|path| {
            let name = path.to_string_lossy().into_owned();
            Ok((name, decode(&std::fs::read(path)?)?))
        })
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;
    let mut nodes = 0;
    let mut arrays = 0;
    let mut elapsed = Duration::ZERO;
    for (name, text) in sources {
        let options = ParseOptions {
            script_kind: ScriptKind::ensure_from_file_name(&name),
            ..ParseOptions::default()
        };
        let started = Instant::now();
        let source = parse_source_file(name, text, options, None);
        elapsed += started.elapsed();
        nodes += source.node_count();
        arrays += source.arena.node_arrays().len();
        std::hint::black_box(&source);
    }
    println!(
        "files={} nodes={} arrays={} nanoseconds={}",
        files.len(),
        nodes,
        arrays,
        elapsed.as_nanos()
    );
    Ok(())
}
