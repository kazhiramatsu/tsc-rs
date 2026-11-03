//! Structural fingerprints of the parser's and the binder's output: the
//! oracle a rewrite of the front end keeps stable.
//!
//! usage: syntax_fingerprint [--profile <name>] --conformance
//!        syntax_fingerprint --files <list file or -> [--root <directory>]
//!
//! Build it alone: `cargo build --release -p tsc-rs-binder --example
//! syntax_fingerprint` (it needs only the syntax, binder, diagnostics and
//! types crates).
//!
//! Every input is parsed twice, with the command line's JSDoc mode
//! (`ParseForTypeErrors`, mode `types`) and with the language service's
//! (`ParseAll`, mode `all`), and bound; one line per input and mode:
//!
//!   path TAB mode TAB parse_hash TAB bind_hash TAB node_count TAB diagnostics_hash
//!
//! The hashes (64-bit FNV-1a, hexadecimal) depend on structure, never on
//! `NodeId`, `NodeArrayId`, `SymbolId` or `FlowId` values: nodes are named by
//! their index in a pre-order walk (each node's JSDoc, then its
//! `for_each_child` children), symbols and flow nodes by their order of first
//! appearance from that walk. `parse_hash` covers each node's kind, range,
//! flags, literal flags, data variant, parent, JSDoc attachments and fields
//! (children, arrays with their range, trailing comma and missing-list bits,
//! booleans, `SyntaxKind`s, texts, JSDoc comment texts and ranges) and the
//! file's own facts (module indicator, pragmas, comment directives).
//! `bind_hash` covers each node's binder flags, symbol, local symbol, locals
//! table, next container and flow nodes; each symbol's flags, name,
//! declarations, value declaration, parent, export symbol and tables; the flow
//! graph (flags, payload, antecedents); the bind diagnostics and the file's
//! binder facts. `node_count` is the number of nodes the walk reaches;
//! `diagnostics_hash` covers the parse and JSDoc diagnostics (code, category,
//! start, length, message text and arguments, chains, related information).
//! A panic while fingerprinting an input prints `panic` in every column.
//!
//! `--conformance` walks the vendored native profile
//! (`vendor/typescript-native/<profile>/upstream`): its bundled libraries
//! (`tsc/internal/bundled/libs`), its test library (`tsc/testdata/tests/lib`)
//! and its compiler and conformance case files
//! (`tsc/testdata/tests/cases/{compiler,conformance}`), each source file read
//! whole and printed relative to the profile's root. A case file is not split
//! into its `@filename` units: the oracle compares two parsers on identical
//! inputs, so any deterministic input set serves, and a whole case file is
//! still a source text the parser must handle. `--files` reads one path per
//! line (`-` for standard input) and prints each relative to `--root`.
//! scripts/syntax_fingerprint.py runs both and compares two runs.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use tsc_binder::flow::{FlowId, FlowPayload};
use tsc_binder::{bind_source_file, Binder, Symbol, SymbolId, SymbolTable};
use tsc_diagnostics::{Diagnostic, MessageChain, RelatedInfo};
use tsc_syntax::{
    for_each_child, for_each_observable_field, JSDocComment, JSDocParsingMode, NodeArrayId,
    NodeData, NodeId, ObservableField, ParseOptions, ScriptKind, SourceFile, SyntaxKind,
};
use tsc_types::JsStr;

const SOURCE_EXTENSIONS: [&str; 8] = [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"];
const MODES: [(&str, JSDocParsingMode); 2] = [
    ("types", JSDocParsingMode::ParseForTypeErrors),
    ("all", JSDocParsingMode::ParseAll),
];
/// The stack of a fingerprint thread: the compiler's worker stack
/// (`tsc_program::WORKER_STACK_BYTES`), which deep test inputs need.
const STACK_BYTES: usize = 1 << 30;
/// The profile's directories `--conformance` walks, under its root.
const PROFILE_DIRECTORIES: [&str; 4] = [
    "tsc/internal/bundled/libs",
    "tsc/testdata/tests/lib",
    "tsc/testdata/tests/cases/compiler",
    "tsc/testdata/tests/cases/conformance",
];
/// A reference to a node, symbol or flow node the walk does not reach.
const UNREACHED: u32 = u32::MAX - 1;
const NONE: u32 = u32::MAX;

fn main() {
    let mut profile = "7.1.0-dev-aa814927".to_owned();
    let mut conformance = false;
    let mut files: Option<String> = None;
    let mut root: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{arg} takes a value"));
        match arg.as_str() {
            "--profile" => profile = value(),
            "--conformance" => conformance = true,
            "--files" => files = Some(value()),
            "--root" => root = Some(PathBuf::from(value())),
            other => panic!("unknown argument {other}"),
        }
    }
    let worker = std::thread::Builder::new()
        .stack_size(STACK_BYTES)
        .spawn(move || {
            let stdout = std::io::stdout();
            let mut out = std::io::BufWriter::new(stdout.lock());
            if conformance {
                run_conformance(&profile, &mut out);
            }
            if let Some(list) = files {
                run_files(&list, root.as_deref(), &mut out);
            }
            out.flush().expect("write standard output");
        })
        .expect("spawn the fingerprint thread");
    if worker.join().is_err() {
        std::process::exit(1);
    }
}

fn run_conformance(profile: &str, out: &mut impl Write) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/typescript-native")
        .join(profile)
        .join("upstream");
    for directory in PROFILE_DIRECTORIES {
        for path in source_files_under(&root.join(directory)) {
            let raw =
                std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let shown = path
                .strip_prefix(&root)
                .expect("walked under the profile root")
                .to_string_lossy()
                .replace('\\', "/");
            fingerprint_input(&shown, &decode(&raw), out);
        }
    }
}

fn run_files(list: &str, root: Option<&Path>, out: &mut impl Write) {
    let lines: Vec<String> = if list == "-" {
        std::io::stdin()
            .lock()
            .lines()
            .map(|line| line.expect("read standard input"))
            .collect()
    } else {
        std::fs::read_to_string(list)
            .unwrap_or_else(|error| panic!("{list}: {error}"))
            .lines()
            .map(str::to_owned)
            .collect()
    };
    for line in lines.iter().filter(|line| !line.is_empty()) {
        let path = Path::new(line);
        let shown = root
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let raw = match std::fs::read(path) {
            Ok(raw) => raw,
            Err(error) => {
                eprintln!("{line}: {error}");
                continue;
            }
        };
        fingerprint_input(&shown, &decode(&raw), out);
    }
}

/// The source files under `root`, sorted.
fn source_files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
        for entry in entries {
            let path = entry.expect("read a directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if is_source_name(&path.to_string_lossy()) {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn is_source_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SOURCE_EXTENSIONS
        .iter()
        .any(|extension| lower.ends_with(extension))
}

/// The text of a file: UTF-8 without its byte order mark, or UTF-16 by its
/// byte order mark; invalid UTF-8 is replaced.
fn decode(raw: &[u8]) -> String {
    let utf16 = |bytes: &[u8], little: bool| -> String {
        let units = bytes.as_chunks::<2>().0.iter().map(|&pair| {
            if little {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        });
        char::decode_utf16(units)
            .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect()
    };
    match raw {
        [0xef, 0xbb, 0xbf, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [0xff, 0xfe, rest @ ..] => utf16(rest, true),
        [0xfe, 0xff, rest @ ..] => utf16(rest, false),
        _ => String::from_utf8_lossy(raw).into_owned(),
    }
}

/// Fingerprint one input in both JSDoc modes. The parser sees the shown path
/// under `/` as the file name (the binder names a module's symbol after it),
/// so the rows do not depend on where the corpus lies.
fn fingerprint_input(path: &str, text: &str, out: &mut impl Write) {
    let file_name = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    };
    let file_name = file_name.as_str();
    for (mode, js_doc_parsing_mode) in MODES {
        let row = catch_unwind(AssertUnwindSafe(|| {
            fingerprint(file_name, text, js_doc_parsing_mode)
        }));
        let row = match row {
            Ok((parse, bind, nodes, diagnostics)) => {
                format!("{parse:016x}\t{bind:016x}\t{nodes}\t{diagnostics:016x}")
            }
            Err(_) => "panic\tpanic\tpanic\tpanic".to_owned(),
        };
        writeln!(out, "{path}\t{mode}\t{row}").expect("write standard output");
    }
}

fn fingerprint(
    file_name: &str,
    text: &str,
    js_doc_parsing_mode: JSDocParsingMode,
) -> (u64, u64, usize, u64) {
    let lower = file_name.to_ascii_lowercase();
    // tsgo IsDeclarationFileName: `.d.ts`, `.d.mts`, `.d.cts` or `.d.<ext>.ts`.
    let declaration = [".d.ts", ".d.mts", ".d.cts"]
        .iter()
        .any(|suffix| lower.ends_with(suffix))
        || (lower.contains(".d.") && lower.ends_with(".ts"));
    let module_extension = [".mts", ".cts", ".mjs", ".cjs"]
        .iter()
        .any(|suffix| lower.ends_with(suffix));
    let options = ParseOptions {
        script_kind: ScriptKind::from_file_name(file_name),
        js_doc_parsing_mode,
        force_external_module: module_extension && !declaration,
        ..ParseOptions::default()
    };
    let source = tsc_syntax::parse_source_file(file_name, text, options, None);
    let tree = Tree::walk(&source);
    let parse = tree.parse_hash();
    let binder = bind_source_file(&source);
    let bind = tree.bind_hash(&binder);
    let mut diagnostics = Fnv::new();
    for list in [&source.parse_diagnostics, &source.js_doc_diagnostics] {
        diagnostics.u32(list.len() as u32);
        for diagnostic in list.iter() {
            hash_diagnostic(&mut diagnostics, diagnostic);
        }
    }
    (parse, bind, tree.order.len(), diagnostics.finish())
}

/// 64-bit FNV-1a over a typed byte stream.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.bytes(&value.to_le_bytes());
    }

    fn opt_u32(&mut self, value: Option<u32>) {
        self.u32(value.unwrap_or(NONE));
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    /// A length-prefixed string.
    fn str(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.bytes(value.as_bytes());
    }

    /// A length-prefixed JavaScript string, by its UTF-16 code units.
    fn js(&mut self, value: JsStr<'_>) {
        let units: Vec<u16> = value.code_units().collect();
        self.u32(units.len() as u32);
        for unit in units {
            self.u16(unit);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// A parsed tree in walk order, with each node's walk index.
struct Tree<'a> {
    source: &'a SourceFile,
    order: Vec<NodeId>,
    index: HashMap<NodeId, u32>,
    /// The walk's parent of each node in `order` (`NONE` for the root).
    walk_parent: Vec<u32>,
}

impl<'a> Tree<'a> {
    /// Pre-order from the root: a node, its JSDoc, then its children. A node
    /// reached twice is walked once.
    fn walk(source: &'a SourceFile) -> Self {
        let mut tree = Self {
            source,
            order: Vec::new(),
            index: HashMap::new(),
            walk_parent: Vec::new(),
        };
        let mut stack = vec![(source.root, NONE)];
        let mut children = Vec::new();
        while let Some((id, parent)) = stack.pop() {
            match tree.index.entry(id) {
                Entry::Occupied(_) => continue,
                Entry::Vacant(slot) => {
                    slot.insert(tree.order.len() as u32);
                }
            }
            tree.order.push(id);
            tree.walk_parent.push(parent);
            let this = tree.order.len() as u32 - 1;
            let node = source.arena.node(id);
            children.clear();
            if let Some(docs) = node.js_doc {
                children.extend_from_slice(source.arena.node_array(docs).nodes);
            }
            for_each_child(&source.arena, node, |child| {
                children.push(child);
                false
            });
            stack.extend(children.iter().rev().map(|child| (*child, this)));
        }
        tree
    }

    fn node_ref(&self, id: NodeId) -> u32 {
        self.index.get(&id).copied().unwrap_or(UNREACHED)
    }

    fn opt_node_ref(&self, id: Option<NodeId>) -> u32 {
        id.map_or(NONE, |id| self.node_ref(id))
    }

    fn array(&self, hash: &mut Fnv, id: NodeArrayId) {
        let array = self.source.arena.node_array(id);
        hash.u32(array.pos);
        hash.u32(array.end);
        hash.bool(array.has_trailing_comma);
        hash.bool(array.is_missing_list);
        hash.u32(array.nodes.len() as u32);
        for node in array.nodes {
            hash.u32(self.node_ref(*node));
        }
    }

    fn parse_hash(&self) -> u64 {
        let source = self.source;
        let mut hash = Fnv::new();
        hash.str(&format!(
            "{:?} {:?}",
            source.language_variant, source.script_kind
        ));
        hash.bool(source.is_declaration_file);
        hash.u32(self.opt_node_ref(source.external_module_indicator));
        hash.str(&format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            source.referenced_files,
            source.type_reference_directives,
            source.lib_reference_directives,
            source.amd_dependencies,
            source.module_name,
            source.comment_directives,
        ));
        hash.str(&format!(
            "{:?}|{:?}|{:?}|{:?}",
            source.has_jsx_import_source_pragma,
            source.jsx_import_source_pragma,
            source.has_jsx_runtime_pragma,
            source.jsx_runtime_pragma,
        ));
        hash.u32(self.order.len() as u32);
        for (walk_index, id) in self.order.iter().enumerate() {
            let node = source.arena.node(*id);
            hash.u8(b'N');
            hash.u16(node.kind as u16);
            hash.u32(node.pos);
            hash.u32(node.end);
            hash.i32(node.flags);
            hash.u16(node.literal_flags);
            hash.u16(node.data().kind().map_or(u16::MAX, |kind| kind as u16));
            // The recorded parent, and whether it is the walk's parent.
            let parent = self.opt_node_ref(node.parent);
            hash.u32(parent);
            hash.bool(parent == self.walk_parent[walk_index]);
            match node.js_doc {
                Some(docs) => {
                    hash.u8(1);
                    self.array(&mut hash, docs);
                }
                None => hash.u8(0),
            }
            for_each_observable_field(&source.arena, node, |name, field| {
                hash.str(name);
                match field {
                    ObservableField::Node(child) => {
                        hash.u8(0);
                        hash.u32(self.node_ref(child));
                    }
                    ObservableField::NodeArray(array) => {
                        hash.u8(1);
                        self.array(&mut hash, array);
                    }
                    ObservableField::Bool(value) => {
                        hash.u8(2);
                        hash.bool(value);
                    }
                    ObservableField::String(value) => {
                        hash.u8(3);
                        hash.str(value);
                    }
                    ObservableField::JsString(value) => {
                        hash.u8(4);
                        hash.js(value);
                    }
                }
            });
            if let Some(kind) = syntax_kind_field(node.data()) {
                hash.u8(b'K');
                hash.u16(kind as u16);
            }
            if let Some(JSDocComment::Text {
                text,
                pos,
                end,
                text_end,
            }) = jsdoc_comment(node.data())
            {
                hash.u8(b'C');
                hash.str(source.arena.text_utf8(*text));
                hash.u32(*pos);
                hash.u32(*end);
                hash.u32(*text_end);
            }
        }
        hash.finish()
    }

    fn bind_hash(&self, binder: &Binder<'_>) -> u64 {
        let mut symbols = Numbering::<SymbolId>::default();
        let mut flows = Numbering::<FlowId>::default();
        flows.number(binder.unreachable_flow);
        // Symbols and flow nodes in order of first appearance from the walk.
        for id in &self.order {
            if let Some(symbol) = binder.node_symbol.get(id) {
                symbols.number(*symbol);
            }
            if let Some(symbol) = binder.node_local_symbol.get(id) {
                symbols.number(*symbol);
            }
            if let Some(table) = binder.locals.get(id) {
                for (_, symbol) in table.iter() {
                    symbols.number(*symbol);
                }
            }
            for flow in node_flows(binder, *id).into_iter().flatten() {
                number_flow_graph(binder, flow, &mut flows);
            }
        }
        // Then what those symbols reach, breadth first, then the rest of the
        // arena in creation order.
        let mut next = 0;
        for pass in 0..2 {
            while next < symbols.order.len() {
                if let Some(symbol) = binder.symbols.get(symbols.order[next]) {
                    number_symbol_links(symbol, &mut symbols);
                }
                next += 1;
            }
            if pass == 0 {
                let base = binder.symbols.base();
                for index in 0..binder.symbols.len() as u32 {
                    symbols.number(SymbolId::new(base + index));
                }
            }
        }
        for index in 0..binder.flow.len() as u32 {
            number_flow_graph(binder, FlowId::new(index), &mut flows);
        }

        let mut hash = Fnv::new();
        hash.u32(self.order.len() as u32);
        for id in &self.order {
            hash.u8(b'N');
            hash.i32(binder.flags_of(*id).bits());
            hash.opt_u32(
                binder
                    .node_symbol
                    .get(id)
                    .map(|symbol| symbols.get(*symbol)),
            );
            hash.opt_u32(
                binder
                    .node_local_symbol
                    .get(id)
                    .map(|symbol| symbols.get(*symbol)),
            );
            match binder.locals.get(id) {
                Some(locals) => {
                    hash.u8(1);
                    hash_table(&mut hash, locals, &symbols);
                }
                None => hash.u8(0),
            }
            hash.u32(self.opt_node_ref(binder.next_container.get(id).copied()));
            for flow in node_flows(binder, *id) {
                hash.opt_u32(flow.map(|flow| flows.get(flow)));
            }
            hash.u8(match binder.possibly_exhaustive.get(id) {
                None => 0,
                Some(false) => 1,
                Some(true) => 2,
            });
        }
        hash.u32(symbols.order.len() as u32);
        for id in &symbols.order {
            hash.u8(b'S');
            // A symbol of no arena of this bind (none is expected).
            let Some(symbol) = binder.symbols.get(*id) else {
                hash.u8(0);
                continue;
            };
            hash.u8(1);
            hash.i32(symbol.flags.bits());
            hash.js(symbol.escaped_name.as_js());
            hash.u32(symbol.declarations.len() as u32);
            for declaration in symbol.declarations.iter() {
                hash.u32(self.node_ref(*declaration));
            }
            hash.u32(self.opt_node_ref(symbol.value_declaration));
            hash.opt_u32(symbol.parent.map(|parent| symbols.get(parent)));
            hash.opt_u32(symbol.export_symbol.map(|export| symbols.get(export)));
            hash_table(&mut hash, symbol.members(), &symbols);
            hash_table(&mut hash, symbol.exports(), &symbols);
            let extras = symbol.extras();
            hash_table(&mut hash, &extras.global_exports, &symbols);
            hash.u8(match extras.const_enum_only_module {
                None => 0,
                Some(false) => 1,
                Some(true) => 2,
            });
            hash.bool(extras.is_replaceable_by_method);
            hash.u32(extras.assignment_declaration_members.len() as u32);
            for (key, value) in extras.assignment_declaration_members.iter() {
                hash.u32(self.node_ref(*key));
                hash.u32(self.node_ref(*value));
            }
        }
        hash.u32(flows.order.len() as u32);
        for id in &flows.order {
            let flow = binder.flow.flow(*id);
            hash.u8(b'F');
            hash.i32(flow.flags.bits());
            match &flow.payload {
                FlowPayload::None => hash.u8(0),
                FlowPayload::Node(node) => {
                    hash.u8(1);
                    hash.u32(self.node_ref(*node));
                }
                FlowPayload::SwitchClause {
                    switch_statement,
                    clause_start,
                    clause_end,
                } => {
                    hash.u8(2);
                    hash.u32(self.node_ref(*switch_statement));
                    hash.u32(*clause_start);
                    hash.u32(*clause_end);
                }
                FlowPayload::ReduceLabel(reduce) => {
                    hash.u8(3);
                    hash.u32(flows.get(reduce.target));
                    hash.u32(reduce.antecedents.len() as u32);
                    for antecedent in &reduce.antecedents {
                        hash.u32(flows.get(*antecedent));
                    }
                }
                FlowPayload::Label { .. } => hash.u8(4),
            }
            let antecedents = flow.antecedents();
            hash.u32(antecedents.len() as u32);
            for antecedent in antecedents {
                hash.u32(flows.get(*antecedent));
            }
        }
        hash.u32(self.opt_node_ref(binder.common_js_module_indicator));
        hash.i32(binder.emit_flags);
        hash.u32(binder.classifiable_names.len() as u32);
        for name in binder.classifiable_names.iter() {
            hash.js(name.as_js());
        }
        hash.u32(binder.pattern_ambient_modules.len() as u32);
        for (pattern, name, symbol) in &binder.pattern_ambient_modules {
            hash.js(pattern.as_js());
            hash.js(name.as_js());
            hash.u32(symbols.get(*symbol));
        }
        hash.u32(binder.bind_diagnostics.len() as u32);
        for diagnostic in binder.bind_diagnostics.iter() {
            hash_diagnostic(&mut hash, diagnostic);
        }
        hash.finish()
    }
}

/// A symbol table: its names in insertion order with their symbols' numbers.
fn hash_table(hash: &mut Fnv, table: &SymbolTable, symbols: &Numbering<SymbolId>) {
    hash.u32(table.len() as u32);
    for (name, symbol) in table.iter() {
        hash.js(name.as_js());
        hash.u32(symbols.get(*symbol));
    }
}

/// Number the symbols `symbol` links to: its parent, export symbol and table
/// entries.
fn number_symbol_links(symbol: &Symbol, symbols: &mut Numbering<SymbolId>) {
    for linked in [symbol.parent, symbol.export_symbol].into_iter().flatten() {
        symbols.number(linked);
    }
    for table in [
        symbol.members(),
        symbol.exports(),
        &symbol.extras().global_exports,
    ] {
        for (_, member) in table.iter() {
            symbols.number(*member);
        }
    }
}

/// The flow nodes the binder records for a node, in a fixed order: its flow
/// node, end, return, when-true, when-false and fallthrough flow.
fn node_flows(binder: &Binder<'_>, id: NodeId) -> [Option<FlowId>; 6] {
    [
        binder.node_flow.get(&id).copied(),
        binder.node_end_flow.get(&id).copied(),
        binder.node_return_flow.get(&id).copied(),
        binder.node_flow_when_true.get(&id).copied(),
        binder.node_flow_when_false.get(&id).copied(),
        binder.node_fallthrough_flow.get(&id).copied(),
    ]
}

/// Number `start` and every flow node it reaches through its antecedents and
/// reduce-label targets, depth first, in first-visit order.
fn number_flow_graph(binder: &Binder<'_>, start: FlowId, flows: &mut Numbering<FlowId>) {
    let mut stack = vec![start];
    while let Some(id) = stack.pop() {
        if !flows.number(id) {
            continue;
        }
        let flow = binder.flow.flow(id);
        let mut reached: Vec<FlowId> = flow.antecedents().to_vec();
        if let FlowPayload::ReduceLabel(reduce) = &flow.payload {
            reached.push(reduce.target);
            reached.extend_from_slice(&reduce.antecedents);
        }
        stack.extend(reached.into_iter().rev());
    }
}

/// Identities numbered in order of first appearance.
struct Numbering<T> {
    order: Vec<T>,
    number: HashMap<T, u32>,
}

impl<T> Default for Numbering<T> {
    fn default() -> Self {
        Self {
            order: Vec::new(),
            number: HashMap::new(),
        }
    }
}

impl<T: Copy + Eq + std::hash::Hash> Numbering<T> {
    /// Number `id` if it has no number yet; whether it was new.
    fn number(&mut self, id: T) -> bool {
        match self.number.entry(id) {
            Entry::Occupied(_) => false,
            Entry::Vacant(slot) => {
                slot.insert(self.order.len() as u32);
                self.order.push(id);
                true
            }
        }
    }

    fn get(&self, id: T) -> u32 {
        self.number.get(&id).copied().unwrap_or(UNREACHED)
    }
}

fn hash_diagnostic(hash: &mut Fnv, diagnostic: &Diagnostic) {
    hash.opt_u32(diagnostic.start);
    hash.opt_u32(diagnostic.length);
    hash_chain(hash, &diagnostic.message);
    hash.bool(diagnostic.related_information_present);
    hash.u32(diagnostic.related.len() as u32);
    for related in &diagnostic.related {
        hash_related(hash, related);
    }
    hash.str(&format!(
        "{:?}|{:?}|{:?}|{}",
        diagnostic.reports_unnecessary,
        diagnostic.reports_deprecated,
        diagnostic.source,
        diagnostic.skipped_on_no_emit,
    ));
}

fn hash_related(hash: &mut Fnv, related: &RelatedInfo) {
    hash.bool(related.file_name.is_some());
    hash.opt_u32(related.start);
    hash.opt_u32(related.length);
    hash_chain(hash, &related.message);
}

fn hash_chain(hash: &mut Fnv, chain: &MessageChain) {
    hash.u32(chain.code);
    hash.str(&format!("{:?}", chain.category));
    hash.js(chain.text.as_js());
    hash.u32(chain.args.len() as u32);
    for argument in &chain.args {
        hash.js(argument.as_js());
    }
    hash.bool(chain.next_present);
    hash.u32(chain.next.len() as u32);
    for next in &chain.next {
        hash_chain(hash, next);
    }
}

/// The `SyntaxKind` field of a payload (the observable field view leaves
/// these out).
fn syntax_kind_field(data: &NodeData) -> Option<SyntaxKind> {
    match data {
        NodeData::HeritageClause(data) => Some(data.token),
        NodeData::ImportAttributes(data) => Some(data.token),
        NodeData::ImportClause(data) => data.phase_modifier,
        NodeData::MetaProperty(data) => Some(data.keyword_token),
        NodeData::PostfixUnaryExpression(data) => Some(data.operator),
        NodeData::PrefixUnaryExpression(data) => Some(data.operator),
        NodeData::TypeOperator(data) => Some(data.operator),
        _ => None,
    }
}

macro_rules! jsdoc_comment_of {
    ($data:expr; $($kind:ident),* $(,)?) => {
        match $data {
            $(NodeData::$kind(data) => data.comment.as_ref(),)*
            _ => None,
        }
    };
}

/// The JSDoc comment of a payload (the observable field view keeps its text
/// but not its ranges).
fn jsdoc_comment(data: &NodeData) -> Option<&JSDocComment> {
    jsdoc_comment_of!(data;
        JSDoc, JSDocAugmentsTag, JSDocAuthorTag, JSDocCallbackTag, JSDocClassTag,
        JSDocDeprecatedTag, JSDocEnumTag, JSDocImplementsTag, JSDocImportTag, JSDocOverloadTag,
        JSDocOverrideTag, JSDocParameterTag, JSDocPrivateTag, JSDocPropertyTag,
        JSDocProtectedTag, JSDocPublicTag, JSDocReadonlyTag, JSDocReturnTag, JSDocSatisfiesTag,
        JSDocSeeTag, JSDocTag, JSDocTemplateTag, JSDocThisTag, JSDocThrowsTag, JSDocTypeTag,
        JSDocTypedefTag,
    )
}
