//! M8 micro-batch (observation only, never merged): the parser's identifier
//! interning, `EscapedName::escape` (current_token_name), replayed over the
//! identifier tokens of DIR in source order. The tokens are scanned once
//! before the rounds; difference two round counts:
//! (instructions(R2) - instructions(R1)) / ((R2 - R1) * names per round).
//! Usage: intern_batch [--rounds N] DIR [DIR ...] under `/usr/bin/time -l`.
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
            let units = rest.as_chunks::<2>().0.iter().map(|&pair| {
                if little {
                    u16::from_le_bytes(pair)
                } else {
                    u16::from_be_bytes(pair)
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
    let mut mode = String::from("escape");
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--rounds" {
            rounds = args.next().ok_or("--rounds requires a count")?.parse()?;
        } else if arg == "--mode" {
            mode = args.next().ok_or("--mode requires escape|twice|hash|loop|sim")?;
        } else {
            collect(Path::new(&arg), &mut files)?;
        }
    }
    if rounds == 0 || files.is_empty() {
        return Err("usage: intern_batch [--rounds N>0] DIR [DIR ...]".into());
    }
    files.sort();
    files.dedup();
    let mut names: Vec<String> = Vec::new();
    for path in &files {
        let text = decode(&std::fs::read(path)?);
        let kind = match path.extension().and_then(|ext| ext.to_str()) {
            Some("tsx") => ScriptKind::Tsx,
            Some("jsx") => ScriptKind::Jsx,
            Some("js") => ScriptKind::Js,
            _ => ScriptKind::Ts,
        };
        let options = ParseOptions {
            script_kind: kind,
            ..ParseOptions::default()
        };
        let variant = match options.language_variant() {
            tsc_syntax::LanguageVariant::Standard => scanner::LanguageVariant::Standard,
            tsc_syntax::LanguageVariant::Jsx => scanner::LanguageVariant::Jsx,
        };
        let mut scanner = scanner::Scanner::new(&text, variant);
        scanner.set_jsdoc_parsing_mode(JSDocParsingMode::ParseForTypeErrors, !options.javascript_file());
        loop {
            let token = scanner.scan();
            if token == SyntaxKind::EndOfFileToken {
                break;
            }
            if token == SyntaxKind::Identifier {
                names.push(scanner.token_value_str().to_owned());
            }
        }
    }
    use std::hint::black_box;
    use tsc_types::EscapedName;
    let started = std::time::Instant::now();
    match mode.as_str() {
        // The parser's call (current_token_name).
        "escape" => {
            for _ in 0..rounds {
                for name in &names {
                    black_box(EscapedName::escape(black_box(name.as_str()).into()));
                }
            }
        }
        // Each name twice: the second call is a hit of the thread's recent
        // cache (the first left the name in its slot), and the first calls
        // see exactly the cache states of "escape", so twice - escape = the
        // hit path.
        "twice" => {
            for _ in 0..rounds {
                for name in &names {
                    black_box(EscapedName::escape(black_box(name.as_str()).into()));
                    black_box(EscapedName::escape(black_box(name.as_str()).into()));
                }
            }
        }
        // Prototype P1: the same direct-mapped recent cache, owned by the
        // caller (taken once, passed by &mut: no thread-local access, no
        // RefCell, no slot copy), the same Fx hash and slice compare; a miss
        // goes through the current EscapedName::escape.
        "p1" | "p1big" => {
            let mut cache = Cache1::new(if mode == "p1" { 13 } else { 16 });
            for _ in 0..rounds {
                for name in &names {
                    black_box(cache.escape(black_box(name.as_str())));
                }
            }
            println!("p1 misses {} of {}", cache.misses, names.len() as u64 * rounds);
        }
        // Prototype P2: P1 plus, for names of at most 16 bytes, a key of two
        // overlapping little-endian words and the length (exact), a
        // multiplicative hash of the key and a compare of three words.
        "p2" | "p2big" => {
            let mut cache = Cache2::new(if mode == "p2" { 13 } else { 16 });
            for _ in 0..rounds {
                for name in &names {
                    black_box(cache.escape(black_box(name.as_str())));
                }
            }
            println!(
                "p2 misses {} (long-name misses {}) of {}",
                cache.misses + cache.long.misses,
                cache.long.misses,
                names.len() as u64 * rounds
            );
        }
        // Both prototypes return escape's name for every name, every round.
        "verify" => {
            let (mut one, mut two) = (Cache1::new(13), Cache2::new(13));
            let (mut one_big, mut two_big) = (Cache1::new(16), Cache2::new(16));
            for _ in 0..rounds {
                for name in &names {
                    let expected = EscapedName::escape(name.as_str().into());
                    assert!(one.escape(name) == expected, "p1 differs on {name:?}");
                    assert!(two.escape(name) == expected, "p2 differs on {name:?}");
                    assert!(one_big.escape(name) == expected, "p1big differs on {name:?}");
                    assert!(two_big.escape(name) == expected, "p2big differs on {name:?}");
                }
            }
            println!(
                "verify ok: misses p1 {} p2 {} p1big {} p2big {} of {}",
                one.misses,
                two.misses + two.long.misses,
                one_big.misses,
                two_big.misses + two_big.long.misses,
                names.len() as u64 * rounds
            );
        }
        // names::hash_bytes alone (FxHasher over the bytes, length first).
        "hash" => {
            for _ in 0..rounds {
                for name in &names {
                    black_box(fx(black_box(name.as_str()).as_bytes()));
                }
            }
        }
        // The loop and its black boxes alone.
        "loop" => {
            for _ in 0..rounds {
                for name in &names {
                    black_box(black_box(name.as_str()).len());
                }
            }
        }
        // names::intern's recent cache simulated (same hash, same slots):
        // its hit rate per round, and the distinct names (first sights).
        "sim" => {
            const RECENT: usize = 1 << 13;
            let mut slots: Vec<Option<(u64, &str)>> = vec![None; RECENT];
            let mut distinct = std::collections::HashSet::new();
            for round in 0..rounds {
                let (mut hits, mut long) = (0u64, 0u64);
                for name in &names {
                    let hash = fx(name.as_bytes());
                    let slot = hash as usize % RECENT;
                    if slots[slot] == Some((hash, name.as_str())) {
                        hits += 1;
                    } else {
                        slots[slot] = Some((hash, name.as_str()));
                    }
                    if name.len() > 19 {
                        long += 1;
                    }
                    if round == 0 {
                        distinct.insert(name.as_str());
                    }
                }
                println!(
                    "sim round {} hits {} of {} ({:.2}%), names longer than the inline 19 bytes {}, distinct {}",
                    round + 1,
                    hits,
                    names.len(),
                    100.0 * hits as f64 / names.len() as f64,
                    long,
                    distinct.len()
                );
            }
        }
        other => return Err(format!("unknown mode {other}").into()),
    }
    let nanos = started.elapsed().as_nanos();
    println!(
        "mode={} files={} rounds={} names={} nanoseconds={} ns/name={:.4}",
        mode,
        files.len(),
        rounds,
        names.len(),
        nanos,
        nanos as f64 / (names.len() as f64 * rounds as f64)
    );
    Ok(())
}

#[derive(Clone, Copy)]
struct Slot1 {
    hash: u64,
    id: u32,
    len: u32,
    text: [u8; 24],
}

struct Cache1 {
    slots: Box<[Slot1]>,
    mask: usize,
    misses: u64,
}

impl Cache1 {
    fn new(bits: u32) -> Self {
        let empty = Slot1 { hash: 0, id: u32::MAX, len: u32::MAX, text: [0; 24] };
        Self { slots: vec![empty; 1 << bits].into_boxed_slice(), mask: (1 << bits) - 1, misses: 0 }
    }

    #[inline]
    fn escape(&mut self, raw: &str) -> tsc_types::EscapedName {
        let bytes = raw.as_bytes();
        let hash = fx(bytes);
        let slot = &mut self.slots[hash as usize & self.mask];
        if slot.hash == hash
            && slot.len as usize == bytes.len()
            && if bytes.len() <= 24 {
                slot.text[..bytes.len()] == *bytes
            } else {
                tsc_store::names::text(slot.id) == bytes
            }
        {
            return tsc_types::EscapedName::from_index(slot.id);
        }
        self.misses += 1;
        let name = tsc_types::EscapedName::escape(raw.into());
        // A name the escape changed (a leading "__") is not cached: its
        // stored text is not the raw text.
        if !bytes.starts_with(b"__") {
            slot.hash = hash;
            slot.id = name.index();
            slot.len = bytes.len() as u32;
            if bytes.len() <= 24 {
                slot.text[..bytes.len()].copy_from_slice(bytes);
            }
        }
        name
    }
}

#[derive(Clone, Copy)]
struct Slot2 {
    a: u64,
    b: u64,
    len: u32,
    id: u32,
}

struct Cache2 {
    slots: Box<[Slot2]>,
    shift: u32,
    misses: u64,
    long: Cache1,
}

/// The exact key of a name of at most 16 bytes: two overlapping
/// little-endian words (or bytes) that together cover every byte.
#[inline]
fn key(bytes: &[u8]) -> (u64, u64) {
    let len = bytes.len();
    if len >= 8 {
        (
            u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            u64::from_le_bytes(bytes[len - 8..].try_into().unwrap()),
        )
    } else if len >= 4 {
        (
            u64::from(u32::from_le_bytes(bytes[..4].try_into().unwrap())),
            u64::from(u32::from_le_bytes(bytes[len - 4..].try_into().unwrap())),
        )
    } else if len > 0 {
        (
            u64::from(bytes[0]) | u64::from(bytes[len / 2]) << 8 | u64::from(bytes[len - 1]) << 16,
            0,
        )
    } else {
        (0, 0)
    }
}

impl Cache2 {
    fn new(bits: u32) -> Self {
        let empty = Slot2 { a: 0, b: 0, len: u32::MAX, id: u32::MAX };
        Self {
            slots: vec![empty; 1 << bits].into_boxed_slice(),
            shift: 64 - bits,
            misses: 0,
            long: Cache1::new(bits),
        }
    }

    #[inline]
    fn escape(&mut self, raw: &str) -> tsc_types::EscapedName {
        let bytes = raw.as_bytes();
        if bytes.len() > 16 {
            return self.long.escape(raw);
        }
        let (a, b) = key(bytes);
        let hash = (a ^ b.rotate_left(29) ^ (bytes.len() as u64) << 59)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let slot = &mut self.slots[(hash >> self.shift) as usize];
        if slot.len as usize == bytes.len() && slot.a == a && slot.b == b {
            return tsc_types::EscapedName::from_index(slot.id);
        }
        self.misses += 1;
        let name = tsc_types::EscapedName::escape(raw.into());
        if !bytes.starts_with(b"__") {
            *slot = Slot2 { a, b, len: bytes.len() as u32, id: name.index() };
        }
        name
    }
}

/// names::hash_bytes.
fn fx(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    bytes.hash(&mut hasher);
    hasher.finish()
}
