//! tsgo `internal/api/encoder/encoder_test.go`.

use std::path::Path;

use tsc_api::encoder::{
    build_node_index_table, encode_source_file, format_encoded_source_file, ScriptKind,
    SourceFileFacts, HEADER_OFFSET_EXTENDED_DATA, HEADER_OFFSET_METADATA, HEADER_OFFSET_NODES,
    NODE_DATA_STRING_INDEX_MASK, NODE_OFFSET_DATA, NODE_SIZE, NO_STRUCTURED_DATA, PROTOCOL_VERSION,
};
use tsc_api::parse_source_file;

const PROFILE: &str = "7.1.0-dev-19dadef8";

fn reference(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/typescript-native")
        .join(PROFILE)
        .join("upstream/tsc/testdata/baselines/reference/api")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn encode(text: &str) -> Vec<u8> {
    let file = parse_source_file("/test.ts", text, ScriptKind::Ts);
    let facts = SourceFileFacts {
        script_kind: ScriptKind::Ts,
        ..SourceFileFacts::default()
    };
    encode_source_file(&file, &facts).0
}

fn read_u32(buffer: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap())
}

#[test]
fn encode_source_file_baseline() {
    let encoded = encode(
        "import { bar } from \"bar\";\nexport function foo<T, U>(a: string, b: string): any {}\nfoo();",
    );
    assert_eq!(
        format_encoded_source_file(&encoded),
        reference("encodeSourceFile.txt")
    );
}

/// tsgo's source text, with `~` standing for each escape's backslash.
fn unicode_escapes_text() -> String {
    r#"let a = "😃"; let b = "~ud83d~ude03"; let c = "~udc00~ud83d~ude03"; let d = "~ud83d~ud83d~ude03""#
        .replace('~', "\\")
}

#[test]
fn encode_source_file_with_unicode_escapes_baseline() {
    let encoded = encode(&unicode_escapes_text());
    assert_eq!(
        format_encoded_source_file(&encoded),
        reference("encodeSourceFileWithUnicodeEscapes.txt")
    );
}

/// tsgo TestBuildNodeIndexTableMatchesEncode.
#[test]
fn build_node_index_table_matches_encode() {
    let file = parse_source_file(
        "/test.ts",
        "import { bar } from \"bar\";\nexport function foo<T, U>(a: string, b: string): any {}\nfoo();",
        ScriptKind::Ts,
    );
    let (_, encoded) = encode_source_file(&file, &SourceFileFacts::default());
    let built = build_node_index_table(&file);
    assert_eq!(built.nodes(), encoded.nodes());
    for (index, node) in encoded.nodes().iter().enumerate() {
        if let Some(node) = node {
            assert_eq!(encoded.get_index(*node), index as u32);
            assert_eq!(built.get_index(*node), index as u32);
        }
    }
}

/// The protocol version and the source file's content-mapper fields of
/// tsgo TestEncodeContentMapperSourceFileMetadata, for a file without a
/// content mapper (tsc-rs has none).
#[test]
fn source_file_metadata_without_content_mapper() {
    assert_eq!(PROTOCOL_VERSION, 9);
    let encoded = encode("let a = 1;");
    assert_eq!(encoded[HEADER_OFFSET_METADATA + 3], 9);
    let nodes = read_u32(&encoded, HEADER_OFFSET_NODES) as usize;
    let root_data = read_u32(&encoded, nodes + NODE_SIZE + NODE_OFFSET_DATA);
    let extended = read_u32(&encoded, HEADER_OFFSET_EXTENDED_DATA) as usize
        + (root_data & NODE_DATA_STRING_INDEX_MASK) as usize;
    for field in [52, 56, 60, 64, 68, 72] {
        assert_eq!(
            read_u32(&encoded, extended + field),
            NO_STRUCTURED_DATA,
            "field {field}"
        );
    }
    // originalText is the text.
    assert_eq!(
        read_u32(&encoded, extended + 48),
        read_u32(&encoded, extended)
    );
}

/// tsgo parser/references.go: imports in source order (a nested ambient
/// module's non-relative ones), then the import calls and types of the
/// text; augmentations of a module file; ambient module names of a script.
#[test]
fn module_references_follow_tsgo_parser() {
    use tsc_api::references::collect_external_module_references;
    use tsc_syntax::NodeData;

    let texts = |file: &tsc_syntax::SourceFile, ids: &[tsc_syntax::NodeId]| -> Vec<String> {
        ids.iter()
            .map(|&id| match &file.arena.node(id).data {
                NodeData::StringLiteral(literal) => literal.text.to_string_lossy().into_owned(),
                NodeData::Identifier(identifier) => identifier
                    .escaped_text
                    .unescape()
                    .to_string_lossy()
                    .into_owned(),
                _ => panic!("not a module name"),
            })
            .collect()
    };
    let module = parse_source_file(
        "/module.ts",
        "import a from \"a\";\nexport * from \"b\";\nimport c = require(\"c\");\nconst d = import(\"d\");\ntype E = import(\"e\").E;\ndeclare module \"f\" {}\ndeclare global {}\n",
        ScriptKind::Ts,
    );
    let references = collect_external_module_references(&module);
    assert_eq!(
        texts(&module, &references.imports),
        ["a", "b", "c", "d", "e"]
    );
    assert_eq!(
        texts(&module, &references.module_augmentations),
        ["f", "global"]
    );
    assert!(references.ambient_module_names.is_empty());

    let script = parse_source_file(
        "/script.ts",
        "declare module \"amb\" { import x = require(\"inner\"); import y = require(\"./relative\"); }\n",
        ScriptKind::Ts,
    );
    let references = collect_external_module_references(&script);
    assert_eq!(texts(&script, &references.imports), ["inner"]);
    assert!(references.module_augmentations.is_empty());
    assert_eq!(
        references
            .ambient_module_names
            .iter()
            .map(|name| name.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["amb"]
    );

    let javascript = parse_source_file(
        "/a.js",
        "const x = require(\"x\");\nrequire(\"y\", 1);\n",
        ScriptKind::Js,
    );
    let references = collect_external_module_references(&javascript);
    assert_eq!(texts(&javascript, &references.imports), ["x"]);
}
