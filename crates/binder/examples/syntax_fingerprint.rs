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
//!
//! Storage assumptions are confined to the current `FingerprintSource`
//! implementation. The hash no longer synthesizes `SymbolId::new(base + i)`
//! from a dense symbol arena: `symbols_in_creation_order` supplies identities.
//! It no longer synthesizes `FlowId::new(i)` from a zero-based dense flow
//! arena: `flows_in_creation_order` supplies identities, and `flow` supplies
//! payloads and antecedent lists. Identities are opaque keys to the hash;
//! creation order is observable only after walk-reachable objects are numbered.
//! Node records, arrays, payloads and JSDoc are read through `parse_node`,
//! `array`, `children` and `observable_fields`; binder side tables and flag
//! flags through `bind_node`. File facts and diagnostics also cross the
//! trait. An arena implementation must preserve these orders and facts, not
//! the current storage layout.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use tsc_binder::flow::{FlowData, FlowId};
use tsc_binder::{Binder, SymbolId, SymbolTable};
use tsc_diagnostics::{Diagnostic, MessageChain, RelatedInfo};
use tsc_syntax::{
    for_each_child, for_each_observable_field, JSDocComment, JSDocParsingMode, NodeArrayId,
    NodeData, NodeId, ObservableField, ParseOptions, ScriptKind, SyntaxKind,
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

/// The complete input to the oracle. Methods expose logical, ordered facts,
/// never concrete node/symbol records, arenas or binder maps. IDs are opaque
/// identities used only for equality and lookup. Table entries, declarations,
/// children, observable fields and antecedents retain their original order.
trait FingerprintSource {
    fn root(&self) -> NodeId;
    fn parse_file(&self) -> ParseFileFacts<'_>;
    fn parse_node(&self, id: NodeId) -> ParseNodeFacts<'_>;
    fn array(&self, id: NodeArrayId) -> ArrayFacts<'_>;
    /// Direct children in `for_each_child` order; attached JSDoc is separate.
    fn children(&self, id: NodeId, visit: impl FnMut(NodeId));
    fn observable_fields<'a>(
        &'a self,
        id: NodeId,
        visit: impl FnMut(&'static str, ObservableField<'a>),
    );
    fn symbols_in_creation_order(&self) -> impl Iterator<Item = SymbolId>;
    fn symbol(&self, id: SymbolId) -> Option<SymbolFacts<'_>>;
    fn flows_in_creation_order(&self) -> impl Iterator<Item = FlowId>;
    fn flow(&self, id: FlowId) -> FlowFacts<'_>;
    fn bind_node(&self, id: NodeId) -> BindNodeFacts<'_>;
    fn bind_file(&self) -> BindFileFacts<'_>;
}

struct ParseFileFacts<'a> {
    /// Preserve the original Debug spelling of the file metadata.
    language: String,
    is_declaration_file: bool,
    external_module_indicator: Option<NodeId>,
    references_and_directives: String,
    jsx_pragmas: String,
    diagnostics: [&'a [Diagnostic]; 2],
}

struct ParseNodeFacts<'a> {
    kind: SyntaxKind,
    pos: u32,
    end: u32,
    flags: i32,
    literal_flags: u16,
    data_kind: Option<SyntaxKind>,
    parent: Option<NodeId>,
    js_doc: Option<NodeArrayId>,
    syntax_kind_field: Option<SyntaxKind>,
    jsdoc_comment: Option<(&'a str, u32, u32, u32)>,
}

struct ArrayFacts<'a> {
    nodes: &'a [NodeId],
    pos: u32,
    end: u32,
    has_trailing_comma: bool,
    is_missing_list: bool,
}

type TableFacts<'a> = Vec<(JsStr<'a>, SymbolId)>;

struct SymbolFacts<'a> {
    flags: i32,
    escaped_name: JsStr<'a>,
    declarations: &'a [NodeId],
    value_declaration: Option<NodeId>,
    parent: Option<SymbolId>,
    export_symbol: Option<SymbolId>,
    /// Members, exports, global exports, in that order.
    tables: [TableFacts<'a>; 3],
    const_enum_only_module: Option<bool>,
    is_replaceable_by_method: bool,
    assignment_declaration_members: Vec<(NodeId, NodeId)>,
}

struct FlowFacts<'a> {
    flags: i32,
    payload: FlowData<'a>,
    antecedents: &'a [FlowId],
}

struct BindNodeFacts<'a> {
    flags: i32,
    symbol: Option<SymbolId>,
    local_symbol: Option<SymbolId>,
    locals: Option<TableFacts<'a>>,
    next_container: Option<NodeId>,
    /// Flow, end, return, when-true, when-false, fallthrough, in that order.
    flows: [Option<FlowId>; 6],
    possibly_exhaustive: Option<bool>,
}

struct BindFileFacts<'a> {
    unreachable_flow: FlowId,
    common_js_module_indicator: Option<NodeId>,
    emit_flags: i32,
    classifiable_names: Vec<JsStr<'a>>,
    pattern_ambient_modules: Vec<(JsStr<'a>, JsStr<'a>, SymbolId)>,
    diagnostics: &'a [Diagnostic],
}

/// Today's storage adapter. This is the only code that knows how the
/// parser and binder store the facts above. `Binder` exclusively borrows
/// its source and owns the tables subsequently published as `BindData`.
impl FingerprintSource for Binder<'_> {
    fn root(&self) -> NodeId {
        self.source.root
    }

    fn parse_file(&self) -> ParseFileFacts<'_> {
        let source = &*self.source;
        ParseFileFacts {
            language: format!("{:?} {:?}", source.language_variant, source.script_kind),
            is_declaration_file: source.is_declaration_file,
            external_module_indicator: source.external_module_indicator,
            references_and_directives: format!(
                "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
                source.referenced_files,
                source.type_reference_directives,
                source.lib_reference_directives,
                source.amd_dependencies,
                source.module_name,
                source.comment_directives,
            ),
            jsx_pragmas: format!(
                "{:?}|{:?}|{:?}|{:?}",
                source.has_jsx_import_source_pragma,
                source.jsx_import_source_pragma,
                source.has_jsx_runtime_pragma,
                source.jsx_runtime_pragma,
            ),
            diagnostics: [&source.parse_diagnostics, &source.js_doc_diagnostics],
        }
    }

    fn parse_node(&self, id: NodeId) -> ParseNodeFacts<'_> {
        let node = self.source.arena.node(id);
        let data = node.data();
        ParseNodeFacts {
            kind: node.kind,
            pos: node.pos,
            end: node.end,
            flags: node.flags,
            literal_flags: node.literal_flags(),
            data_kind: data.kind(),
            parent: node.parent,
            js_doc: node.js_doc(),
            syntax_kind_field: syntax_kind_field(&data),
            jsdoc_comment: match jsdoc_comment(&data) {
                Some(JSDocComment::Text {
                    text,
                    pos,
                    end,
                    text_end,
                }) => Some((self.source.arena.text_utf8(*text), *pos, *end, *text_end)),
                _ => None,
            },
        }
    }

    fn array(&self, id: NodeArrayId) -> ArrayFacts<'_> {
        let array = self.source.arena.node_array(id);
        ArrayFacts {
            nodes: array.nodes,
            pos: array.pos,
            end: array.end,
            has_trailing_comma: array.has_trailing_comma,
            is_missing_list: array.is_missing_list,
        }
    }

    fn children(&self, id: NodeId, mut visit: impl FnMut(NodeId)) {
        for_each_child(&self.source.arena, self.source.arena.node(id), |child| {
            visit(child);
            false
        });
    }

    fn observable_fields<'a>(
        &'a self,
        id: NodeId,
        visit: impl FnMut(&'static str, ObservableField<'a>),
    ) {
        for_each_observable_field(&self.source.arena, self.source.arena.node(id), visit);
    }

    fn symbols_in_creation_order(&self) -> impl Iterator<Item = SymbolId> {
        let base = self.symbols.base();
        (0..self.symbols.len() as u32).map(move |index| SymbolId::new(base + index))
    }

    fn symbol(&self, id: SymbolId) -> Option<SymbolFacts<'_>> {
        let symbol = self.symbols.get(id)?;
        let extras = symbol.extras();
        Some(SymbolFacts {
            flags: symbol.flags.bits(),
            escaped_name: symbol.escaped_name.as_js(),
            declarations: symbol.declarations(),
            value_declaration: symbol.value_declaration,
            parent: symbol.parent,
            export_symbol: symbol.export_symbol(),
            tables: [symbol.members(), symbol.exports(), &extras.global_exports]
                .map(|table| table_facts(table)),
            const_enum_only_module: extras.const_enum_only_module,
            is_replaceable_by_method: extras.is_replaceable_by_method,
            assignment_declaration_members: extras
                .assignment_declaration_members
                .iter()
                .map(|(key, value)| (*key, *value))
                .collect(),
        })
    }

    fn flows_in_creation_order(&self) -> impl Iterator<Item = FlowId> {
        (0..self.flow.len() as u32).map(FlowId::new)
    }

    fn flow(&self, id: FlowId) -> FlowFacts<'_> {
        FlowFacts {
            flags: self.flow.flow(id).flags.bits(),
            payload: self.flow.payload(id),
            antecedents: self.flow.antecedents(id),
        }
    }

    fn bind_node(&self, id: NodeId) -> BindNodeFacts<'_> {
        BindNodeFacts {
            flags: self.flags_of(id).bits(),
            symbol: self.node_symbol.get(&id).copied(),
            local_symbol: self.node_local_symbol.get(&id).copied(),
            locals: self.locals.get(&id).map(table_facts),
            next_container: self.next_container.get(&id).copied(),
            flows: [
                self.node_flow.get(&id),
                self.node_end_flow.get(&id).copied(),
                self.node_return_flow.get(&id).copied(),
                self.node_flow_when_true.get(&id).copied(),
                self.node_flow_when_false.get(&id).copied(),
                self.node_fallthrough_flow.get(&id).copied(),
            ],
            possibly_exhaustive: self.possibly_exhaustive.get(&id).copied(),
        }
    }

    fn bind_file(&self) -> BindFileFacts<'_> {
        BindFileFacts {
            unreachable_flow: self.unreachable_flow,
            common_js_module_indicator: self.common_js_module_indicator,
            emit_flags: self.emit_flags,
            classifiable_names: self
                .classifiable_names
                .iter()
                .map(|name| name.as_js())
                .collect(),
            pattern_ambient_modules: self
                .pattern_ambient_modules
                .iter()
                .map(|(pattern, name, symbol)| (pattern.as_js(), name.as_js(), *symbol))
                .collect(),
            diagnostics: &self.bind_diagnostics,
        }
    }
}

fn table_facts(table: &SymbolTable) -> TableFacts<'_> {
    table
        .iter()
        .map(|(name, symbol)| (name.as_js(), *symbol))
        .collect()
}

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
    let mut source = tsc_syntax::parse_source_file(file_name, text, options, None);
    let mut binder = Binder::new(&mut source);
    let tree = Tree::walk(&binder);
    let parse = tree.parse_hash(&binder);
    binder.bind_source_file();
    fingerprint_bound_source(&tree, parse, &binder)
}

#[cfg(test)]
fn fingerprint_source(source: &impl FingerprintSource) -> (u64, u64, usize, u64) {
    let tree = Tree::walk(source);
    let parse = tree.parse_hash(source);
    fingerprint_bound_source(&tree, parse, source)
}

fn fingerprint_bound_source(
    tree: &Tree,
    parse: u64,
    source: &impl FingerprintSource,
) -> (u64, u64, usize, u64) {
    let bind = tree.bind_hash(source);
    let mut diagnostics = Fnv::new();
    for list in source.parse_file().diagnostics {
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
struct Tree {
    order: Vec<NodeId>,
    index: HashMap<NodeId, u32>,
    /// The walk's parent of each node in `order` (`NONE` for the root).
    walk_parent: Vec<u32>,
}

impl Tree {
    /// Pre-order from the root: a node, its JSDoc, then its children. A node
    /// reached twice is walked once.
    fn walk(source: &impl FingerprintSource) -> Self {
        let mut tree = Self {
            order: Vec::new(),
            index: HashMap::new(),
            walk_parent: Vec::new(),
        };
        let mut stack = vec![(source.root(), NONE)];
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
            let node = source.parse_node(id);
            children.clear();
            if let Some(docs) = node.js_doc {
                children.extend_from_slice(source.array(docs).nodes);
            }
            source.children(id, |child| children.push(child));
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

    fn array(&self, source: &impl FingerprintSource, hash: &mut Fnv, id: NodeArrayId) {
        let array = source.array(id);
        hash.u32(array.pos);
        hash.u32(array.end);
        hash.bool(array.has_trailing_comma);
        hash.bool(array.is_missing_list);
        hash.u32(array.nodes.len() as u32);
        for node in array.nodes {
            hash.u32(self.node_ref(*node));
        }
    }

    fn parse_hash(&self, source: &impl FingerprintSource) -> u64 {
        let file = source.parse_file();
        let mut hash = Fnv::new();
        hash.str(&file.language);
        hash.bool(file.is_declaration_file);
        hash.u32(self.opt_node_ref(file.external_module_indicator));
        hash.str(&file.references_and_directives);
        hash.str(&file.jsx_pragmas);
        hash.u32(self.order.len() as u32);
        for (walk_index, id) in self.order.iter().enumerate() {
            let node = source.parse_node(*id);
            hash.u8(b'N');
            hash.u16(node.kind as u16);
            hash.u32(node.pos);
            hash.u32(node.end);
            hash.i32(node.flags);
            hash.u16(node.literal_flags);
            hash.u16(node.data_kind.map_or(u16::MAX, |kind| kind as u16));
            // The recorded parent, and whether it is the walk's parent.
            let parent = self.opt_node_ref(node.parent);
            hash.u32(parent);
            hash.bool(parent == self.walk_parent[walk_index]);
            match node.js_doc {
                Some(docs) => {
                    hash.u8(1);
                    self.array(source, &mut hash, docs);
                }
                None => hash.u8(0),
            }
            source.observable_fields(*id, |name, field| {
                hash.str(name);
                match field {
                    ObservableField::Node(child) => {
                        hash.u8(0);
                        hash.u32(self.node_ref(child));
                    }
                    ObservableField::NodeArray(array) => {
                        hash.u8(1);
                        self.array(source, &mut hash, array);
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
            if let Some(kind) = node.syntax_kind_field {
                hash.u8(b'K');
                hash.u16(kind as u16);
            }
            if let Some((text, pos, end, text_end)) = node.jsdoc_comment {
                hash.u8(b'C');
                hash.str(text);
                hash.u32(pos);
                hash.u32(end);
                hash.u32(text_end);
            }
        }
        hash.finish()
    }

    fn bind_hash(&self, source: &impl FingerprintSource) -> u64 {
        let file = source.bind_file();
        let mut symbols = Numbering::<SymbolId>::default();
        let mut flows = Numbering::<FlowId>::default();
        flows.number(file.unreachable_flow);
        // Symbols and flow nodes in order of first appearance from the walk.
        for id in &self.order {
            let node = source.bind_node(*id);
            if let Some(symbol) = node.symbol {
                symbols.number(symbol);
            }
            if let Some(symbol) = node.local_symbol {
                symbols.number(symbol);
            }
            if let Some(table) = &node.locals {
                for (_, symbol) in table {
                    symbols.number(*symbol);
                }
            }
            for flow in node.flows.into_iter().flatten() {
                number_flow_graph(source, flow, &mut flows);
            }
        }
        // Then what those symbols reach, breadth first, then the rest of the
        // arena in creation order.
        let mut next = 0;
        for pass in 0..2 {
            while next < symbols.order.len() {
                if let Some(symbol) = source.symbol(symbols.order[next]) {
                    number_symbol_links(&symbol, &mut symbols);
                }
                next += 1;
            }
            if pass == 0 {
                for symbol in source.symbols_in_creation_order() {
                    symbols.number(symbol);
                }
            }
        }
        for flow in source.flows_in_creation_order() {
            number_flow_graph(source, flow, &mut flows);
        }

        let mut hash = Fnv::new();
        hash.u32(self.order.len() as u32);
        for id in &self.order {
            let node = source.bind_node(*id);
            hash.u8(b'N');
            hash.i32(node.flags);
            hash.opt_u32(node.symbol.map(|symbol| symbols.get(symbol)));
            hash.opt_u32(node.local_symbol.map(|symbol| symbols.get(symbol)));
            match &node.locals {
                Some(locals) => {
                    hash.u8(1);
                    hash_table(&mut hash, locals, &symbols);
                }
                None => hash.u8(0),
            }
            hash.u32(self.opt_node_ref(node.next_container));
            for flow in node.flows {
                hash.opt_u32(flow.map(|flow| flows.get(flow)));
            }
            hash.u8(match node.possibly_exhaustive {
                None => 0,
                Some(false) => 1,
                Some(true) => 2,
            });
        }
        hash.u32(symbols.order.len() as u32);
        for id in &symbols.order {
            hash.u8(b'S');
            // A symbol of no arena of this bind (none is expected).
            let Some(symbol) = source.symbol(*id) else {
                hash.u8(0);
                continue;
            };
            hash.u8(1);
            hash.i32(symbol.flags);
            hash.js(symbol.escaped_name);
            hash.u32(symbol.declarations.len() as u32);
            for declaration in symbol.declarations {
                hash.u32(self.node_ref(*declaration));
            }
            hash.u32(self.opt_node_ref(symbol.value_declaration));
            hash.opt_u32(symbol.parent.map(|parent| symbols.get(parent)));
            hash.opt_u32(symbol.export_symbol.map(|export| symbols.get(export)));
            for table in &symbol.tables {
                hash_table(&mut hash, table, &symbols);
            }
            hash.u8(match symbol.const_enum_only_module {
                None => 0,
                Some(false) => 1,
                Some(true) => 2,
            });
            hash.bool(symbol.is_replaceable_by_method);
            hash.u32(symbol.assignment_declaration_members.len() as u32);
            for (key, value) in &symbol.assignment_declaration_members {
                hash.u32(self.node_ref(*key));
                hash.u32(self.node_ref(*value));
            }
        }
        hash.u32(flows.order.len() as u32);
        for id in &flows.order {
            let flow = source.flow(*id);
            hash.u8(b'F');
            hash.i32(flow.flags);
            match flow.payload {
                FlowData::None => hash.u8(0),
                FlowData::Node(node) => {
                    hash.u8(1);
                    hash.u32(self.node_ref(node));
                }
                FlowData::SwitchClause {
                    switch_statement,
                    clause_start,
                    clause_end,
                } => {
                    hash.u8(2);
                    hash.u32(self.node_ref(switch_statement));
                    hash.u32(clause_start);
                    hash.u32(clause_end);
                }
                FlowData::ReduceLabel {
                    target,
                    antecedents,
                } => {
                    hash.u8(3);
                    hash.u32(flows.get(target));
                    hash.u32(antecedents.len() as u32);
                    for antecedent in antecedents {
                        hash.u32(flows.get(*antecedent));
                    }
                }
                FlowData::Label { .. } => hash.u8(4),
            }
            let antecedents = flow.antecedents;
            hash.u32(antecedents.len() as u32);
            for antecedent in antecedents {
                hash.u32(flows.get(*antecedent));
            }
        }
        hash.u32(self.opt_node_ref(file.common_js_module_indicator));
        hash.i32(file.emit_flags);
        hash.u32(file.classifiable_names.len() as u32);
        for name in file.classifiable_names {
            hash.js(name);
        }
        hash.u32(file.pattern_ambient_modules.len() as u32);
        for (pattern, name, symbol) in &file.pattern_ambient_modules {
            hash.js(*pattern);
            hash.js(*name);
            hash.u32(symbols.get(*symbol));
        }
        hash.u32(file.diagnostics.len() as u32);
        for diagnostic in file.diagnostics {
            hash_diagnostic(&mut hash, diagnostic);
        }
        hash.finish()
    }
}

/// A symbol table: its names in insertion order with their symbols' numbers.
fn hash_table(hash: &mut Fnv, table: &[(JsStr<'_>, SymbolId)], symbols: &Numbering<SymbolId>) {
    hash.u32(table.len() as u32);
    for (name, symbol) in table.iter() {
        hash.js(*name);
        hash.u32(symbols.get(*symbol));
    }
}

/// Number the symbols `symbol` links to: its parent, export symbol and table
/// entries.
fn number_symbol_links(symbol: &SymbolFacts<'_>, symbols: &mut Numbering<SymbolId>) {
    for linked in [symbol.parent, symbol.export_symbol].into_iter().flatten() {
        symbols.number(linked);
    }
    for table in &symbol.tables {
        for (_, member) in table.iter() {
            symbols.number(*member);
        }
    }
}

/// Number `start` and every flow node it reaches through its antecedents and
/// reduce-label targets, depth first, in first-visit order.
fn number_flow_graph(
    source: &impl FingerprintSource,
    start: FlowId,
    flows: &mut Numbering<FlowId>,
) {
    let mut stack = vec![start];
    while let Some(id) = stack.pop() {
        if !flows.number(id) {
            continue;
        }
        let flow = source.flow(id);
        let mut reached: Vec<FlowId> = flow.antecedents.to_vec();
        if let FlowData::ReduceLabel {
            target,
            antecedents,
        } = flow.payload
        {
            reached.push(target);
            reached.extend_from_slice(antecedents);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// No parser or binder storage: even unreached symbols and flows must be
    /// obtained from the trait, in creation order, regardless of their IDs.
    struct Fixture {
        nodes: [NodeId; 2],
        array: NodeArrayId,
        symbols: [SymbolId; 2],
        flows: [FlowId; 3],
        symbol_order: [usize; 2],
        flow_order: [usize; 3],
    }

    impl Fixture {
        fn new(nodes: [u32; 2], array: u32, symbols: [u32; 2], flows: [u32; 3]) -> Self {
            Self {
                nodes: nodes.map(NodeId::new),
                array: NodeArrayId::new(array),
                symbols: symbols.map(SymbolId::new),
                flows: flows.map(FlowId::new),
                symbol_order: [0, 1],
                flow_order: [0, 1, 2],
            }
        }
    }

    impl FingerprintSource for Fixture {
        fn root(&self) -> NodeId {
            self.nodes[0]
        }

        fn parse_file(&self) -> ParseFileFacts<'_> {
            ParseFileFacts {
                language: "fixture".into(),
                is_declaration_file: false,
                external_module_indicator: Some(self.root()),
                references_and_directives: String::new(),
                jsx_pragmas: String::new(),
                diagnostics: [&[], &[]],
            }
        }

        fn parse_node(&self, id: NodeId) -> ParseNodeFacts<'_> {
            assert!(self.nodes.contains(&id));
            ParseNodeFacts {
                kind: SyntaxKind::Identifier,
                pos: 0,
                end: 10,
                flags: 0,
                literal_flags: 0,
                data_kind: None,
                parent: (id != self.root()).then_some(self.root()),
                js_doc: (id == self.root()).then_some(self.array),
                syntax_kind_field: Some(SyntaxKind::PlusToken),
                jsdoc_comment: Some(("comment", 0, 10, 8)),
            }
        }

        fn array(&self, id: NodeArrayId) -> ArrayFacts<'_> {
            assert_eq!(id, self.array);
            ArrayFacts {
                nodes: &self.nodes[1..],
                pos: 0,
                end: 10,
                has_trailing_comma: true,
                is_missing_list: false,
            }
        }

        fn children(&self, id: NodeId, mut visit: impl FnMut(NodeId)) {
            if id == self.root() {
                visit(self.nodes[1]); // Also reached through JSDoc: walk once.
            }
        }

        fn observable_fields<'a>(
            &'a self,
            id: NodeId,
            mut visit: impl FnMut(&'static str, ObservableField<'a>),
        ) {
            if id == self.root() {
                visit("child", ObservableField::Node(self.nodes[1]));
                visit("docs", ObservableField::NodeArray(self.array));
            }
            visit("flag", ObservableField::Bool(true));
            visit("text", ObservableField::String("text"));
            visit("js", ObservableField::JsString(JsStr::from_str("js")));
        }

        fn symbols_in_creation_order(&self) -> impl Iterator<Item = SymbolId> {
            self.symbol_order
                .map(|index| self.symbols[index])
                .into_iter()
        }

        fn symbol(&self, id: SymbolId) -> Option<SymbolFacts<'_>> {
            let index = self.symbols.iter().position(|symbol| *symbol == id)?;
            Some(SymbolFacts {
                flags: 0,
                escaped_name: JsStr::from_str(["first", "second"][index]),
                declarations: &self.nodes,
                value_declaration: Some(self.root()),
                parent: None,
                export_symbol: None,
                tables: [Vec::new(), Vec::new(), Vec::new()],
                const_enum_only_module: Some(false),
                is_replaceable_by_method: false,
                assignment_declaration_members: vec![(self.nodes[0], self.nodes[1])],
            })
        }

        fn flows_in_creation_order(&self) -> impl Iterator<Item = FlowId> {
            self.flow_order.map(|index| self.flows[index]).into_iter()
        }

        fn flow(&self, id: FlowId) -> FlowFacts<'_> {
            let index = self.flows.iter().position(|flow| *flow == id).unwrap();
            FlowFacts {
                flags: 0,
                payload: match index {
                    0 => FlowData::None,
                    1 => FlowData::Node(self.root()),
                    _ => FlowData::ReduceLabel {
                        target: self.flows[1],
                        antecedents: &self.flows[..1],
                    },
                },
                antecedents: &self.flows[..usize::from(index != 0)],
            }
        }

        fn bind_node(&self, _id: NodeId) -> BindNodeFacts<'_> {
            BindNodeFacts {
                flags: 0,
                symbol: None,
                local_symbol: None,
                locals: None,
                next_container: Some(self.root()),
                flows: [None; 6],
                possibly_exhaustive: Some(false),
            }
        }

        fn bind_file(&self) -> BindFileFacts<'_> {
            BindFileFacts {
                unreachable_flow: self.flows[0],
                common_js_module_indicator: None,
                emit_flags: 0,
                classifiable_names: Vec::new(),
                pattern_ambient_modules: Vec::new(),
                diagnostics: &[],
            }
        }
    }

    #[test]
    fn sparse_out_of_order_identities_keep_the_fingerprint() {
        let dense = Fixture::new([0, 1], 0, [0, 1], [0, 1, 2]);
        let sparse = Fixture::new([501, 73], 904, [4001, 39], [8001, 197, 24]);
        let expected = fingerprint_source(&dense);
        assert_eq!(expected.2, 2);
        assert_eq!(fingerprint_source(&sparse), expected);
    }

    #[test]
    fn creation_order_of_unreached_symbols_and_flows_is_observable() {
        let mut source = Fixture::new([501, 73], 904, [4001, 39], [8001, 197, 24]);
        let expected = fingerprint_source(&source);
        source.symbol_order = [1, 0];
        assert_ne!(fingerprint_source(&source).1, expected.1);
        source.symbol_order = [0, 1];
        source.flow_order = [0, 2, 1];
        assert_ne!(fingerprint_source(&source).1, expected.1);
    }
}
