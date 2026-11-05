//! M8 replay micro-batch (observation only, never merged): node construction's
//! record writing, `NodeArena::alloc_node`/`alloc_token` (push_node: the id,
//! `Node::new`, `encode_payload` into the record's words and tail, the push),
//! replayed over the nodes a parse of DIR allocates, in allocation order.
//! Each file is parsed once before the rounds (ParseForTypeErrors, as the
//! CLI) and its nodes are captured as (kind, payload, pos, end, flags); every
//! round writes each file's nodes into a fresh arena, as a parse does.
//! `--mode copy` runs the same loop (fresh arenas included) without the
//! writes. Per node = ((I_alloc(R2) - I_alloc(R1)) - (I_copy(R2) - I_copy(R1)))
//! / ((R2 - R1) * nodes per round). `--stride K` keeps every K-th file.
//! Usage: node_batch [--rounds N] [--mode alloc|copy] [--stride K] DIR ...

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tsc_diagnostics::{DocumentVersion, TextSnapshot};
use tsc_syntax::{
    parse_source_file_from_snapshot, JSDocParsingMode, NodeArena, NodeData, ParseOptions,
    ScriptKind, SyntaxKind,
};
use tsc_types::NodeFlags;

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

#[derive(Clone, Copy)]
struct Captured {
    kind: SyntaxKind,
    data: NodeData,
    pos: u32,
    end: u32,
    flags: i32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rounds = 10u64;
    let mut mode = String::from("alloc");
    let mut stride = 1usize;
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--rounds" {
            rounds = args.next().ok_or("--rounds requires a count")?.parse()?;
        } else if arg == "--mode" {
            mode = args.next().ok_or("--mode requires alloc|copy")?;
        } else if arg == "--stride" {
            stride = args.next().ok_or("--stride requires a count")?.parse()?;
        } else {
            collect(Path::new(&arg), &mut files)?;
        }
    }
    if rounds == 0 || stride == 0 || files.is_empty() {
        return Err("usage: node_batch [--rounds N>0] [--mode alloc|copy] [--stride K>0] DIR ...".into());
    }
    files.sort();
    files.dedup();
    let mut streams: Vec<Vec<Captured>> = Vec::new();
    let (mut tokens, mut payloads) = (0u64, 0u64);
    for path in files.iter().step_by(stride) {
        let name = path.to_string_lossy().into_owned();
        let kind = ScriptKind::ensure_from_file_name(&name);
        let snapshot = TextSnapshot::new(decode(&std::fs::read(path)?), DocumentVersion::default());
        let options = ParseOptions {
            script_kind: kind,
            js_doc_parsing_mode: JSDocParsingMode::ParseForTypeErrors,
            ..ParseOptions::default()
        };
        let source = parse_source_file_from_snapshot(name.as_str(), Arc::clone(&snapshot), options, None);
        let stream: Vec<Captured> = source
            .arena
            .nodes()
            .iter()
            .map(|node| {
                let data = node.data();
                if data.kind().is_some() {
                    payloads += 1;
                } else {
                    tokens += 1;
                }
                Captured {
                    kind: node.kind,
                    data,
                    pos: node.pos,
                    end: node.end,
                    flags: node.flags,
                }
            })
            .collect();
        streams.push(stream);
    }
    let nodes: u64 = streams.iter().map(|stream| stream.len() as u64).sum();
    use std::hint::black_box;
    let started = std::time::Instant::now();
    match mode.as_str() {
        "alloc" => {
            for _ in 0..rounds {
                for stream in &streams {
                    let mut arena = NodeArena::new();
                    for entry in stream {
                        let entry = black_box(*entry);
                        let flags = NodeFlags::from_bits(entry.flags);
                        let id = if entry.data.kind().is_some() {
                            arena.alloc_node(entry.data, entry.pos as usize, entry.end as usize, flags)
                        } else {
                            arena.alloc_token(entry.kind, entry.pos as usize, entry.end as usize, flags)
                        };
                        black_box(id);
                    }
                    black_box(&arena);
                }
            }
        }
        // Knock-out: every record pushed with an empty payload (alloc_token:
        // the id, Node::new, the zero-slot store, the push), so alloc - token
        // is the payload's encoding (and alloc_node's kind() match).
        "token" => {
            for _ in 0..rounds {
                for stream in &streams {
                    let mut arena = NodeArena::new();
                    for entry in stream {
                        let entry = black_box(*entry);
                        black_box(entry.data);
                        let flags = NodeFlags::from_bits(entry.flags);
                        black_box(arena.alloc_token(entry.kind, entry.pos as usize, entry.end as usize, flags));
                    }
                    black_box(&arena);
                }
            }
        }
        "copy" => {
            for _ in 0..rounds {
                for stream in &streams {
                    let arena = NodeArena::new();
                    for entry in stream {
                        let entry = black_box(*entry);
                        black_box((entry.kind, entry.data, entry.pos, entry.end, NodeFlags::from_bits(entry.flags)));
                    }
                    black_box(&arena);
                }
            }
        }
        other => return Err(format!("unknown mode {other}").into()),
    }
    let nanos = started.elapsed().as_nanos();
    println!(
        "mode={} files={} rounds={} nodes={} (payload {} token {}) size_of NodeData={} Captured={} nanoseconds={} ns/node={:.4}",
        mode,
        streams.len(),
        rounds,
        nodes,
        payloads,
        tokens,
        std::mem::size_of::<NodeData>(),
        std::mem::size_of::<Captured>(),
        nanos,
        nanos as f64 / (nodes as f64 * rounds as f64)
    );
    Ok(())
}
