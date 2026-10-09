//! Comparison of tsc-rs's encodings with tsgo's (tsgo_fixtures.rs,
//! tsgo_corpus.rs).

#![allow(dead_code)]

use std::fmt::Write as _;

use tsc_api::encoder::{
    encode_source_file, kind_name, ScriptKind, SourceFileFacts, HEADER_OFFSET_EXTENDED_DATA,
    HEADER_OFFSET_NODES, HEADER_OFFSET_STRING_DATA, HEADER_OFFSET_STRING_OFFSETS,
    HEADER_OFFSET_STRUCTURED_DATA, HEADER_SIZE, NODE_SIZE,
};
use tsc_api::parse_source_file;
use tsc_api::references::collect_external_module_references;

/// tsc-rs's encoding of `text` as tsgo's API session encodes a file named
/// `name` (parsed by its extension, with the module references its parser
/// records).
pub fn encode(name: &str, text: &str) -> Vec<u8> {
    let file = parse_source_file(name, text, ScriptKind::from_file_name(name));
    let references = collect_external_module_references(&file);
    let facts = SourceFileFacts {
        path: Some(name),
        script_kind: ScriptKind::from_file_name(name),
        imports: &references.imports,
        module_augmentations: &references.module_augmentations,
        ambient_module_names: &references.ambient_module_names,
        ..SourceFileFacts::default()
    };
    encode_source_file(&file, &facts).0
}

pub fn read_u32(buffer: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap())
}

pub struct Sections<'a> {
    pub header: &'a [u8],
    pub string_offsets: &'a [u8],
    pub string_data: &'a [u8],
    pub extended: &'a [u8],
    pub structured: &'a [u8],
    pub nodes: &'a [u8],
}

pub fn sections(buffer: &[u8]) -> Sections<'_> {
    let at = |offset| read_u32(buffer, offset) as usize;
    Sections {
        header: &buffer[..HEADER_SIZE],
        string_offsets: &buffer[at(HEADER_OFFSET_STRING_OFFSETS)..at(HEADER_OFFSET_STRING_DATA)],
        string_data: &buffer[at(HEADER_OFFSET_STRING_DATA)..at(HEADER_OFFSET_EXTENDED_DATA)],
        extended: &buffer[at(HEADER_OFFSET_EXTENDED_DATA)..at(HEADER_OFFSET_STRUCTURED_DATA)],
        structured: &buffer[at(HEADER_OFFSET_STRUCTURED_DATA)..at(HEADER_OFFSET_NODES)],
        nodes: &buffer[at(HEADER_OFFSET_NODES)..],
    }
}

pub const FIELDS: [&str; 7] = ["kind", "pos", "end", "next", "parent", "data", "flags"];

pub fn record(nodes: &[u8], index: usize) -> Option<[u32; 7]> {
    let start = index * NODE_SIZE;
    (start + NODE_SIZE <= nodes.len())
        .then(|| std::array::from_fn(|field| read_u32(nodes, start + field * 4)))
}

pub fn describe(record: Option<[u32; 7]>) -> String {
    match record {
        None => "(none)".to_owned(),
        Some(r) => format!(
            "{} [{}, {}) next={} parent={} data={:#010x} flags={:#x}",
            kind_name(r[0]),
            r[1],
            r[2],
            r[3],
            r[4],
            r[5],
            r[6]
        ),
    }
}

/// The ancestors' kinds of record `index`, outermost first.
pub fn path(nodes: &[u8], index: usize) -> String {
    let mut kinds = Vec::new();
    let mut parent = record(nodes, index).map_or(0, |r| r[4]) as usize;
    while parent != 0 {
        let Some(r) = record(nodes, parent) else {
            break;
        };
        kinds.push(kind_name(r[0]).trim_start_matches("Kind"));
        parent = r[4] as usize;
    }
    kinds.reverse();
    kinds.join(" > ")
}

/// (category, detail) of the first difference: in the node records'
/// fields but the next sibling, in the other sections, in the siblings,
/// in the header.
pub fn difference(ours: &[u8], theirs: &[u8]) -> (String, String) {
    let (a, b) = (sections(ours), sections(theirs));
    let count = a.nodes.len().max(b.nodes.len()) / NODE_SIZE;
    let differing = |fields: &[usize]| {
        (1..count).find_map(|index| {
            let (x, y) = (record(a.nodes, index), record(b.nodes, index));
            let field = match (x, y) {
                (Some(x), Some(y)) => *fields.iter().find(|&&f| x[f] != y[f])?,
                (None, None) => return None,
                _ => 7,
            };
            Some((index, field, x, y))
        })
    };
    let report = |(index, field, x, y): (usize, usize, Option<[u32; 7]>, Option<[u32; 7]>)| {
        let kind = y.or(x).map_or("?", |r| kind_name(r[0]));
        let category = match field {
            0 => format!(
                "nodes.kind {} vs tsgo {}",
                x.map_or("(none)", |r| kind_name(r[0])),
                y.map_or("(none)", |r| kind_name(r[0]))
            ),
            7 => "nodes.count".to_owned(),
            _ => format!("nodes.{} {kind}", FIELDS[field]),
        };
        let detail = format!(
            "record {index} under {}\n    tsc-rs: {}\n    tsgo:   {}",
            path(b.nodes, index),
            describe(x),
            describe(y)
        );
        (category, detail)
    };
    if let Some(found) = differing(&[0, 1, 2, 4, 5, 6]) {
        return report(found);
    }
    for (name, x, y) in [
        ("string offsets", a.string_offsets, b.string_offsets),
        ("string data", a.string_data, b.string_data),
        ("extended data", a.extended, b.extended),
        ("structured data", a.structured, b.structured),
    ] {
        if x != y {
            let at = x
                .iter()
                .zip(y)
                .position(|(p, q)| p != q)
                .unwrap_or(x.len().min(y.len()));
            return (
                name.to_owned(),
                format!("byte {at} of {} / {}", x.len(), y.len()),
            );
        }
    }
    if let Some(found) = differing(&[3]) {
        return report(found);
    }
    if a.header != b.header {
        let at = a
            .header
            .iter()
            .zip(b.header)
            .position(|(p, q)| p != q)
            .unwrap_or(0);
        return ("header".to_owned(), format!("byte {at}"));
    }
    ("length".to_owned(), String::new())
}

/// Every record of an encoding, one per line, with its data and flags.
pub fn dump(encoded: &[u8]) -> String {
    let nodes = sections(encoded).nodes;
    let mut out = String::new();
    for index in 1..nodes.len() / NODE_SIZE {
        let r = record(nodes, index).unwrap();
        let depth = {
            let mut depth = 0;
            let mut parent = r[4] as usize;
            while parent != 0 {
                depth += 1;
                parent = record(nodes, parent).map_or(0, |p| p[4]) as usize;
            }
            depth
        };
        writeln!(out, "{}{index}: {}", "  ".repeat(depth), describe(Some(r))).unwrap();
    }
    out
}
