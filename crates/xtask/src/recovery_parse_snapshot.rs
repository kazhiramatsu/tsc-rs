//! Parse input and observable digest shared with the standalone historical
//! parser probe. Keep this module independent of current recovery predicates:
//! the same source is compiled against the merge-base syntax crate.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tsc_syntax::{
    for_each_observable_field, JSDocParsingMode, LanguageVariant, NodeArrayId, NodeData, NodeId,
    ObservableField, ParseOptions, SourceFile,
};
use tsc_types::{NodeFlags, ScriptTarget};

pub fn sha256(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

pub fn options_json(options: &ParseOptions) -> Value {
    // Destructure exhaustively so an added parse-steering field cannot be
    // silently omitted from either the snapshot or its replay.
    let ParseOptions {
        script_target,
        language_variant,
        javascript_file,
        js_doc_parsing_mode,
        force_external_module,
        detect_external_module_from_jsx,
        node_id_base,
        node_array_id_base,
    } = options;
    json!({"script_target": script_target.bits(),
        "language_variant": match language_variant { LanguageVariant::Standard => 0, LanguageVariant::Jsx => 1 },
        "javascript_file": javascript_file, "js_doc_parsing_mode": *js_doc_parsing_mode as u8,
        "force_external_module": force_external_module,
        "detect_external_module_from_jsx": detect_external_module_from_jsx,
        "node_id_base": node_id_base, "node_array_id_base": node_array_id_base})
}

pub fn input(file_name: &str, text: &str, options: &ParseOptions) -> Value {
    let options = options_json(options);
    let text_sha256 = sha256(text.as_bytes());
    let input_id = sha256(serde_json::to_vec(&json!([file_name, text_sha256, options])).unwrap());
    json!({"input_id":input_id, "file_name":file_name, "text_utf8_base64":STANDARD.encode(text),
        "text_sha256":text_sha256, "text_utf8_bytes":text.len(),
        "utf16_len":text.encode_utf16().count(), "options":options})
}

pub fn replay(input: &Value) -> SourceFile {
    let bytes = STANDARD
        .decode(input["text_utf8_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(sha256(&bytes), input["text_sha256"]);
    assert_eq!(
        bytes.len() as u64,
        input["text_utf8_bytes"].as_u64().unwrap()
    );
    let text = String::from_utf8(bytes).unwrap();
    assert_eq!(
        text.encode_utf16().count() as u64,
        input["utf16_len"].as_u64().unwrap()
    );
    let o = &input["options"];
    let options = ParseOptions {
        script_target: ScriptTarget::from_bits(
            i32::try_from(o["script_target"].as_i64().unwrap()).unwrap(),
        ),
        language_variant: match o["language_variant"].as_u64().unwrap() {
            0 => LanguageVariant::Standard,
            1 => LanguageVariant::Jsx,
            _ => panic!("unknown language variant"),
        },
        javascript_file: o["javascript_file"].as_bool().unwrap(),
        js_doc_parsing_mode: match o["js_doc_parsing_mode"].as_u64().unwrap() {
            0 => JSDocParsingMode::ParseAll,
            1 => JSDocParsingMode::ParseNone,
            2 => JSDocParsingMode::ParseForTypeErrors,
            3 => JSDocParsingMode::ParseForTypeInfo,
            _ => panic!("unknown JSDoc parsing mode"),
        },
        force_external_module: o["force_external_module"].as_bool().unwrap(),
        detect_external_module_from_jsx: o["detect_external_module_from_jsx"].as_bool().unwrap(),
        node_id_base: u32::try_from(o["node_id_base"].as_u64().unwrap()).unwrap(),
        node_array_id_base: u32::try_from(o["node_array_id_base"].as_u64().unwrap()).unwrap(),
    };
    assert_eq!(&options_json(&options), o);
    let file_name = input["file_name"].as_str().unwrap();
    assert_eq!(self::input(file_name, &text, &options), *input);
    tsc_syntax::parse_source_file(file_name, text, options, None)
}

fn diagnostic_digest(values: &[tsc_diagnostics::Diagnostic]) -> Vec<Value> {
    values
        .iter()
        .map(|diagnostic| {
            json!({"code":diagnostic.code(),
        "start":diagnostic.start,"length":diagnostic.length,
        // Debug is lossless for JsString and the entire MessageChain, and
        // contains no node identities or locale-dependent rendering.
        "message_sha256":sha256(format!("{:?}", diagnostic.message)),
        "diagnostic_sha256":sha256(format!("{diagnostic:?}"))})
        })
        .collect()
}

pub fn digest(source: &SourceFile) -> Value {
    let position = |byte| {
        if byte == u32::MAX {
            byte
        } else {
            source.positions().byte_to_utf16(byte).unwrap()
        }
    };
    validate_schema();
    let mut shape = Sha256::new();
    let mut contexts = Sha256::new();
    let mut module_requests = Vec::new();
    let mut graph = CanonicalGraph::default();
    graph.node(source.root);
    // This SourceFile reference is outside NodeData. Preserve both the target
    // graph and the reference identity even if a future parser detaches it.
    let module_indicator = source.external_module_indicator.map(|id| graph.node(id));
    let mut cursor = 0;
    while cursor < graph.entries.len() {
        let entry = graph.entries[cursor];
        cursor += 1;
        let descriptor = match entry {
            GraphEntry::Node(id) => {
                let node = source.arena.node(id);
                // Exhaustive public records make added structural fields a
                // compile failure; private SourceFile fields have a schema guard.
                let tsc_syntax::nodes::Node {
                    kind,
                    // The literal and tri-state bits are read through the accessors below.
                    literal_flags: _,
                    flags,
                    pos,
                    end,
                    parent,
                    js_doc,
                    data: _,
                } = node;
                let mut fields = Vec::new();
                for_each_observable_field(node, |name, value| {
                    fields.push((name, graph.observable(value)));
                });
                extra_fields(node, |name, value| fields.push((name, value)));
                contexts.update((flags & NodeFlags::CONTEXT_FLAGS.bits()).to_le_bytes());
                json!({"node":*kind as u16,"pos":position(*pos),"end":position(*end),
                    "flags":flags & !NodeFlags::CONTEXT_FLAGS.bits(),
                    "numeric_literal_flags":node.numeric_literal_flags(),
                    "template_flags":node.template_flags(),
                    "multi_line":node.multi_line(),"parent":parent.map(|id|graph.node(id)),
                    "js_doc":js_doc.map(|id|graph.array(id)),"fields":fields})
            }
            GraphEntry::Array(id) => {
                let tsc_syntax::nodes::NodeArray {
                    nodes,
                    pos,
                    end,
                    has_trailing_comma,
                    is_missing_list,
                } = source.arena.node_array(id);
                json!({"array":nodes.iter().map(|id|graph.node(*id)).collect::<Vec<_>>(),
                    "pos":position(pos),"end":position(end),
                    "has_trailing_comma":has_trailing_comma,"is_missing_list":is_missing_list})
            }
        };
        let bytes = serde_json::to_vec(&descriptor).unwrap();
        shape.update((bytes.len() as u64).to_le_bytes());
        shape.update(bytes);
    }
    for reference in &source.referenced_files {
        module_requests.push(json!([
            "reference-path",
            reference.file_name.to_utf16(),
            reference.preserve
        ]));
    }
    for reference in &source.type_reference_directives {
        module_requests.push(json!([
            "reference-types",
            reference.file_name.to_utf16(),
            format!("{:?}", reference.resolution_mode),
            reference.preserve
        ]));
    }
    for reference in &source.lib_reference_directives {
        module_requests.push(json!([
            "reference-lib",
            reference.file_name.to_utf16(),
            reference.preserve
        ]));
    }
    for dependency in &source.amd_dependencies {
        module_requests.push(json!(["amd", dependency.path.to_utf16(), dependency.name]));
    }
    module_requests.push(json!([
        "jsx-import-source",
        source.jsx_import_source_pragma
    ]));
    module_requests.push(json!(["jsx-runtime", source.jsx_runtime_pragma]));
    let NodeData::SourceFile(data) = &source.arena.node(source.root).data else {
        unreachable!()
    };
    let statements: Vec<_> = source
        .arena
        .node_array(data.statements.unwrap())
        .nodes
        .iter()
        .map(|id| {
            let node = source.arena.node(*id);
            json!({"kind":node.kind as u16,"pos":position(node.pos),"end":position(node.end),
            "await_context":NodeFlags::from_bits(node.flags).contains(NodeFlags::AWAIT_CONTEXT)})
        })
        .collect();
    json!({"diagnostics":diagnostic_digest(&source.parse_diagnostics),
    "jsdoc_diagnostics":diagnostic_digest(&source.js_doc_diagnostics),
    "external_module_indicator_graph_index":module_indicator,
    "statements":statements,"ast_shape_sha256":format!("{:x}",shape.finalize()),
    "ast_context_sha256":format!("{:x}",contexts.finalize()),
    "module_requests":module_requests,
    "module_requests_sha256":sha256(serde_json::to_vec(&module_requests).unwrap()),
    "source_facts": {
        "file_name":source.file_name.to_utf16(),"text_sha256":sha256(source.text()),
        "language_version":source.language_version.bits(),
        "language_variant":format!("{:?}",source.language_variant),
        "is_declaration_file":source.is_declaration_file,
        "js_doc_parsing_mode":source.js_doc_parsing_mode as u8,
        "module_name":source.module_name.as_ref().map(|s|s.to_utf16()),
        "renamed_dependencies":source.renamed_dependencies.iter().map(|(a,b)|json!([a.to_utf16(),b.to_utf16()])).collect::<Vec<_>>(),
        "has_jsx_import_source_pragma":source.has_jsx_import_source_pragma,
        "has_jsx_runtime_pragma":source.has_jsx_runtime_pragma,
        "comment_directives":source.comment_directives.iter().map(|d|json!([position(d.pos),position(d.end),format!("{:?}",d.kind)])).collect::<Vec<_>>(),
        "path_references":source.referenced_files.iter().map(reference_value).collect::<Vec<_>>(),
        "lib_references":source.lib_reference_directives.iter().map(reference_value).collect::<Vec<_>>(),
        "type_references":source.type_reference_directives.iter().map(|r|json!([r.file_name.to_utf16(),r.pos,r.end,format!("{:?}",r.resolution_mode),r.preserve])).collect::<Vec<_>>()
    },
    "external_module_indicator":source.external_module_indicator.map(|id| {
        let n=source.arena.node(id);json!([n.kind as u16,position(n.pos),position(n.end)])
    })})
}

fn reference_value(reference: &tsc_syntax::FileReference) -> Value {
    let tsc_syntax::FileReference {
        file_name,
        pos,
        end,
        preserve,
    } = reference;
    json!([file_name.to_utf16(), pos, end, preserve])
}

#[derive(Clone, Copy)]
enum GraphEntry {
    Node(NodeId),
    Array(NodeArrayId),
}
#[derive(Default)]
struct CanonicalGraph {
    nodes: BTreeMap<NodeId, usize>,
    arrays: BTreeMap<NodeArrayId, usize>,
    entries: Vec<GraphEntry>,
}
impl CanonicalGraph {
    fn node(&mut self, id: NodeId) -> usize {
        if let Some(index) = self.nodes.get(&id) {
            return *index;
        }
        let index = self.entries.len();
        self.entries.push(GraphEntry::Node(id));
        self.nodes.insert(id, index);
        index
    }
    fn array(&mut self, id: NodeArrayId) -> usize {
        if let Some(index) = self.arrays.get(&id) {
            return *index;
        }
        let index = self.entries.len();
        self.entries.push(GraphEntry::Array(id));
        self.arrays.insert(id, index);
        index
    }
    fn observable(&mut self, value: ObservableField<'_>) -> Value {
        match value {
            ObservableField::Node(id) => json!(["node", self.node(id)]),
            ObservableField::NodeArray(id) => json!(["array", self.array(id)]),
            ObservableField::Bool(value) => json!(["bool", value]),
            ObservableField::String(value) => json!(["string", value]),
            ObservableField::JsString(value) => json!(["js-string", value.to_utf16()]),
        }
    }
}

// The generated observable visitor deliberately omits some AST fields. Keep
// their names and exact optional presence, including kind-valued payloads.
fn extra_fields(node: &tsc_syntax::nodes::Node, mut cb: impl FnMut(&'static str, Value)) {
    // Kind values are tagged independently from graph indices.
    macro_rules! kind {
        ($field:expr,$name:literal) => {
            cb($name, json!(["kind", $field as u16]))
        };
    }
    match &node.data {
        NodeData::HeritageClause(d) => kind!(d.token, "token"),
        NodeData::Identifier(d) => cb("text", json!(["string", d.text()])),
        NodeData::ImportAttributes(d) => kind!(d.token, "token"),
        NodeData::ImportClause(d) => {
            if let Some(kind) = d.phase_modifier {
                kind!(kind, "phaseModifier");
            }
        }
        NodeData::MetaProperty(d) => kind!(d.keyword_token, "keywordToken"),
        NodeData::PostfixUnaryExpression(d) => kind!(d.operator, "operator"),
        NodeData::PrefixUnaryExpression(d) => kind!(d.operator, "operator"),
        NodeData::PrivateIdentifier(d) => cb("text", json!(["string", d.text()])),
        NodeData::TypeOperator(d) => kind!(d.operator, "operator"),
        _ => {}
    }
}

/// Compile-time source snapshots close the private-field and generated-visitor
/// gaps without changing the syntax crate's public API for a measurement tool.
/// Called before both census and historical replay, not merely by optional tests.
/// `parse_recovery` is deliberately excluded from the AST core: current and
/// projection replays compare its six admission predicates separately, and the
/// census asserts exact recovery-fact equality when reconstructing each input.
pub fn validate_schema() {
    static CHECKED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    CHECKED.get_or_init(|| {
        let nodes=include_str!("../../syntax/src/nodes.rs");
        let observed=include_str!("../../syntax/src/observable_fields.rs");
        let lib=include_str!("../../syntax/src/lib.rs");
        let extras=include_str!("recovery_parse_snapshot.rs").split("fn extra_fields").nth(1).unwrap().split("/// Compile-time").next().unwrap();
        for part in nodes.split("pub struct ").skip(1) {
            let Some((name,body))=part.split_once(" {") else {continue;};
            if name.contains('\n') {continue;}
            let Some(variant)=name.strip_suffix("Data") else {continue;};
            let body=body.split('}').next().unwrap();
            let fields=rust_fields(body);
            let ordinary=variant_body(observed,variant);
            let extra=variant_body(extras,variant);
            for field in fields {
                assert!(has_field(&ordinary,&format!("data.{field}")) || has_field(&extra,&format!("d.{field}")),
                    "uncovered AST field: {variant}.{field}");
            }
        }
        let body=lib.split("pub struct SourceFile {").nth(1).unwrap().split("\n}").next().unwrap();
        let expected="file_name snapshot language_version language_variant is_declaration_file js_doc_parsing_mode arena root external_module_indicator parse_diagnostics parse_recovery js_doc_diagnostics referenced_files type_reference_directives lib_reference_directives amd_dependencies module_name renamed_dependencies has_jsx_import_source_pragma jsx_import_source_pragma has_jsx_runtime_pragma jsx_runtime_pragma comment_directives";
        assert_eq!(rust_fields(body),expected.split_whitespace().map(str::to_owned).collect::<Vec<_>>(),"SourceFile digest schema changed");
    });
}
fn rust_fields(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with("//") {
                return None;
            }
            let line = line.strip_prefix("pub ").unwrap_or(line);
            line.split_once(": ").map(|(name, _)| name.to_owned())
        })
        .collect()
}
fn variant_body(source: &str, variant: &str) -> String {
    let marker = format!("NodeData::{variant}(");
    source
        .split_once(&marker)
        .map(|(_, body)| body.split("NodeData::").next().unwrap().to_owned())
        .unwrap_or_default()
}
fn has_field(source: &str, field: &str) -> bool {
    source.match_indices(field).any(|(start, _)| {
        source
            .as_bytes()
            .get(start + field.len())
            .is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'_')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsc_syntax::SyntaxKind;

    fn parse(text: &str, node_base: u32, array_base: u32) -> SourceFile {
        tsc_syntax::parse_source_file(
            "main.ts",
            text,
            ParseOptions {
                node_id_base: node_base,
                node_array_id_base: array_base,
                ..ParseOptions::default()
            },
            None,
        )
    }

    #[test]
    fn canonical_digest_ignores_allocator_ids_and_preserves_attachments() {
        let text = "/** attached {@link X} */ export class X { p = [1,2]; }";
        assert_eq!(digest(&parse(text, 0, 0)), digest(&parse(text, 100, 200)));
    }

    #[test]
    fn scalar_payloads_and_kind_fields_are_observable_without_changing_text() {
        let original = parse("import type { x } from 'm'; class C extends B {}", 0, 0);
        let before = digest(&original);
        for kind in [
            SyntaxKind::ImportClause,
            SyntaxKind::HeritageClause,
            SyntaxKind::Identifier,
        ] {
            let mut changed = original.clone();
            let index = changed
                .arena
                .nodes()
                .iter()
                .position(|node| {
                    node.kind == kind
                        && match &node.data {
                            // The initial `import type` lookahead leaves an
                            // unattached speculative identifier in the arena.
                            // Mutate the reachable class name, not that orphan.
                            NodeData::Identifier(data) => data.text() == "C",
                            _ => true,
                        }
                })
                .unwrap();
            match &mut changed.arena.node_mut(NodeId::new(index as u32)).data {
                NodeData::ImportClause(d) => d.is_type_only = !d.is_type_only,
                NodeData::HeritageClause(d) => d.token = SyntaxKind::ImplementsKeyword,
                NodeData::Identifier(d) => {
                    d.escaped_text = tsc_types::EscapedName::from_identifier_escaped_text(&format!(
                        "{}changed",
                        d.escaped_text.identifier_text()
                    ))
                }
                _ => unreachable!(),
            }
            assert_ne!(
                before,
                digest(&changed),
                "{kind:?} scalar payload was omitted"
            );
        }
    }

    #[test]
    fn array_and_source_file_facts_are_observable_without_changing_text() {
        let original = parse("export const x = 1;", 0, 0);
        let before = digest(&original);
        let mut changed = original.clone();
        let NodeData::SourceFile(data) = &changed.arena.node(changed.root).data else {
            unreachable!()
        };
        let array = data.statements.unwrap();
        changed.arena.node_array_mut(array).has_trailing_comma = true;
        assert_ne!(before, digest(&changed));
        let mut changed = original.clone();
        changed.module_name = Some("explicit-name".into());
        assert_ne!(before, digest(&changed));
        let mut changed = original.clone();
        changed
            .comment_directives
            .push(tsc_syntax::CommentDirective {
                pos: 0,
                end: 1,
                kind: tsc_syntax::CommentDirectiveKind::Ignore,
            });
        assert_ne!(before, digest(&changed));
    }
}
