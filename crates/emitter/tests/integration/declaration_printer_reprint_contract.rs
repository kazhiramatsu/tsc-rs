use std::cell::RefCell;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use serde_json::Value;
use tsc_emitter::{
    create_printer, transform_nodes, JavaScriptString, NewLineKind, PrintRequest, PrinterOptions,
    SourceFileTextMode, StandaloneWriter, TextWriter, TransformArena, TransformError,
    TransformFlags, TransformNode, TransformRoot, TransformationContext, TransformationResult,
    Transformer,
};
use tsc_syntax::{
    for_each_child,
    nodes::{
        IdentifierData, JSDocFunctionTypeData, NoSubstitutionTemplateLiteralData, ParameterData,
        TemplateHeadData, TemplateMiddleData, TemplateTailData,
    },
    parse_source_file, LanguageVariant, NodeData, ParseOptions, SyntaxKind,
};
use tsc_types::ScriptTarget;

const REPRINT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-7a-printer-reprint.v1.json"
));

fn required_str<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} must be a string"))
}

fn required_u64(value: &Value, key: &str) -> u64 {
    value[key]
        .as_u64()
        .unwrap_or_else(|| panic!("{key} must be an unsigned integer"))
}

fn first_different_line(expected: &str, actual: &str) -> (usize, String, String) {
    let mut expected_lines = expected.split_inclusive('\n');
    let mut actual_lines = actual.split_inclusive('\n');
    let mut line = 1;
    loop {
        let expected_line = expected_lines.next();
        let actual_line = actual_lines.next();
        if expected_line != actual_line {
            return (
                line,
                expected_line
                    .unwrap_or("<end of output>")
                    .escape_debug()
                    .to_string(),
                actual_line
                    .unwrap_or("<end of output>")
                    .escape_debug()
                    .to_string(),
            );
        }
        if expected_line.is_none() {
            unreachable!("different strings must have a differing line");
        }
        line += 1;
    }
}

fn new_line(value: &str) -> NewLineKind {
    match value {
        "lf" => NewLineKind::LineFeed,
        "crlf" => NewLineKind::CarriageReturnLineFeed,
        other => panic!("unknown reprint newline {other}"),
    }
}

fn sha256_hex(input: &[u8]) -> String {
    const INITIAL: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    const ROUND: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];

    let bit_len = (input.len() as u64) * 8;
    let mut padded = input.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    let mut state = INITIAL;
    for block in padded.chunks_exact(64) {
        let mut words = [0u32; 64];
        for (index, bytes) in block.chunks_exact(4).enumerate() {
            words[index] = u32::from_be_bytes(bytes.try_into().unwrap());
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(ROUND[index])
                .wrapping_add(words[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut result = String::with_capacity(64);
    for value in state {
        write!(result, "{value:08x}").unwrap();
    }
    result
}

#[test]
fn declaration_printer_reprint_contract() {
    let artifact: Value = serde_json::from_slice(REPRINT).expect("reprint artifact is valid JSON");
    let rows = artifact["rows"].as_array().expect("rows must be an array");
    let gating_rows = required_u64(&artifact["summary"], "gating_rows") as usize;
    assert_eq!(rows.len(), gating_rows, "all gating rows are loaded");

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut mismatches = Vec::new();
    for row in rows {
        let row_id = required_str(row, "id");
        let input = if let Some(input) = row["input_utf8"].as_str() {
            input.to_owned()
        } else {
            let path = workspace.join(required_str(&row["source"], "path"));
            match fs::read_to_string(&path) {
                Ok(input) => input,
                Err(error) => {
                    mismatches.push(format!("{row_id}: cannot read {}: {error}", path.display()));
                    continue;
                }
            }
        };
        let input_hash = sha256_hex(input.as_bytes());
        if input_hash != required_str(row, "input_sha256") {
            mismatches.push(format!(
                "{row_id}: input sha256 mismatch: expected {}, actual {input_hash}",
                required_str(row, "input_sha256")
            ));
            continue;
        }

        let target = row["options"]["target"]
            .as_i64()
            .map(|bits| ScriptTarget::from_bits(bits as i32));
        let parsed = parse_source_file(
            required_str(row, "file_name"),
            &input,
            ParseOptions {
                script_target: target.unwrap_or(ScriptTarget::LATEST),
                language_variant: LanguageVariant::Standard,
                ..ParseOptions::default()
            },
            None,
        );
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, None);
        let mut result = match transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            Vec::new(),
            true,
        ) {
            Ok(result) => result,
            Err(error) => {
                mismatches.push(format!("{row_id}: identity transformation failed: {error}"));
                continue;
            }
        };
        let mut options = PrinterOptions::new(new_line(required_str(&row["options"], "newLine")))
            .with_no_emit_helpers(true)
            .with_declaration_syntax(true)
            .with_only_print_js_doc_style(true)
            .with_omit_brace_source_map_positions(true)
            .with_remove_comments(
                row["options"]["removeComments"]
                    .as_bool()
                    .expect("removeComments must be boolean"),
            )
            .with_source_file_text_mode(SourceFileTextMode::Canonical);
        if let Some(target) = target {
            options = options.with_target(target);
        }
        let actual = match create_printer(options).print(
            &mut result,
            PrintRequest::SourceFile(source),
            None,
        ) {
            Ok(output) => output.text().to_owned(),
            Err(error) => {
                mismatches.push(format!("{row_id}: print failed: {error}"));
                continue;
            }
        };

        if let Some(expected) = row["expected_utf8"].as_str() {
            if actual != expected {
                let (line, expected_line, actual_line) = first_different_line(expected, &actual);
                mismatches.push(format!(
                    "{row_id}: first difference at line {line}\n  expected: {expected_line}\n    actual: {actual_line}"
                ));
            }
        } else {
            let actual_hash = sha256_hex(actual.as_bytes());
            let expected_hash = required_str(row, "expected_sha256");
            let expected_bytes = required_u64(row, "expected_bytes") as usize;
            if actual_hash != expected_hash || actual.len() != expected_bytes {
                mismatches.push(format!(
                    "{row_id}: expected sha256/len {expected_hash}/{expected_bytes}, actual {actual_hash}/{}",
                    actual.len()
                ));
            }
        }
    }

    eprintln!("declaration printer gating rows: {gating_rows}");
    assert!(
        mismatches.is_empty(),
        "{} declaration printer mismatch(es):\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn declaration_requests_and_ungated_type_roots_fail_closed() {
    let parsed = parse_source_file(
        "control.d.ts",
        "type Alias = string;\n",
        ParseOptions::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        Vec::new(),
        true,
    )
    .expect("identity transformation");
    let options = PrinterOptions::new(NewLineKind::LineFeed)
        .with_source_file_text_mode(SourceFileTextMode::Canonical);
    let mut printer = create_printer(options);
    assert!(
        printer
            .print(&mut result, PrintRequest::Declaration(source), None)
            .is_err(),
        "Declaration stays unsupported"
    );
    assert!(
        printer
            .print(&mut result, PrintRequest::SourceFile(source), None)
            .is_err(),
        "declaration syntax stays dormant by default"
    );
}

#[test]
fn single_line_writer_write_line_is_an_unconditional_space() {
    let mut writer = TextWriter::single_line();

    writer.write_line(false);
    assert_eq!(writer.text(), " ", "an empty buffer still receives a space");

    writer.clear();
    writer.write("tail ");
    writer.write_line(false);
    assert_eq!(
        writer.text(),
        "tail  ",
        "existing trailing whitespace is not coalesced"
    );

    writer.clear();
    writer.write_line(false);
    writer.write_line(true);
    assert_eq!(
        writer.text(),
        "  ",
        "consecutive writeLine calls each append one space"
    );
}

#[derive(Default)]
struct SyntheticPrinterNodes {
    templates: Vec<(SyntaxKind, TransformNode)>,
    js_doc_function_type: Option<TransformNode>,
}

struct SyntheticPrinterNodeTransformer {
    nodes: Rc<RefCell<SyntheticPrinterNodes>>,
}

impl Transformer for SyntheticPrinterNodeTransformer {
    fn name(&self) -> &'static str {
        "synthetic-printer-nodes"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Ok(root);
        };
        let template_data = [
            (
                SyntaxKind::NoSubstitutionTemplateLiteral,
                NodeData::NoSubstitutionTemplateLiteral(NoSubstitutionTemplateLiteralData {
                    text: String::new(),
                    raw_text: None,
                }),
            ),
            (
                SyntaxKind::TemplateHead,
                NodeData::TemplateHead(TemplateHeadData {
                    text: String::new(),
                    raw_text: None,
                }),
            ),
            (
                SyntaxKind::TemplateMiddle,
                NodeData::TemplateMiddle(TemplateMiddleData {
                    text: String::new(),
                    raw_text: None,
                }),
            ),
            (
                SyntaxKind::TemplateTail,
                NodeData::TemplateTail(TemplateTailData {
                    text: String::new(),
                    raw_text: None,
                }),
            ),
        ];
        let mut templates = Vec::with_capacity(template_data.len());
        for (kind, data) in template_data {
            let node = context
                .factory()?
                .create_node(source, data, TransformFlags::NONE)?;
            context
                .arena_mut()?
                .metadata_mut(node)
                .set_javascript_string_value(JavaScriptString::from_code_units(vec![
                    0xd83d, 0xde00, 0xd800,
                ]));
            templates.push((kind, node));
        }

        let name = context.factory()?.create_node(
            source,
            NodeData::Identifier(IdentifierData {
                escaped_text: "x".to_owned(),
                text: "x".to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        let number_type = context.factory()?.create_token(
            source,
            SyntaxKind::NumberKeyword,
            TransformFlags::NONE,
        )?;
        let parameter = context.factory()?.create_node(
            source,
            NodeData::Parameter(ParameterData {
                name: Some(name.node()),
                modifiers: None,
                dot_dot_dot_token: None,
                question_token: None,
                r#type: Some(number_type.node()),
                initializer: None,
            }),
            TransformFlags::NONE,
        )?;
        let parameters = context
            .factory()?
            .create_node_array(source, vec![parameter])?;
        let return_type = context.factory()?.create_token(
            source,
            SyntaxKind::StringKeyword,
            TransformFlags::NONE,
        )?;
        let js_doc_function_type = context.factory()?.create_node(
            source,
            NodeData::JSDocFunctionType(JSDocFunctionTypeData {
                name: None,
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: Some(return_type.node()),
            }),
            TransformFlags::NONE,
        )?;

        let mut nodes = self.nodes.borrow_mut();
        nodes.templates = templates;
        nodes.js_doc_function_type = Some(js_doc_function_type);
        Ok(root)
    }
}

fn synthetic_printer_nodes() -> (
    TransformationResult<'static>,
    Rc<RefCell<SyntheticPrinterNodes>>,
) {
    let parsed = parse_source_file("synthetic.ts", "", ParseOptions::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let nodes = Rc::new(RefCell::new(SyntheticPrinterNodes::default()));
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(SyntheticPrinterNodeTransformer {
            nodes: Rc::clone(&nodes),
        })],
        true,
    )
    .expect("create synthetic printer nodes");
    (result, nodes)
}

fn print_standalone(
    result: &mut TransformationResult<'_>,
    node: TransformNode,
    never_ascii_escape: bool,
) -> String {
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_declaration_syntax(true)
            .with_never_ascii_escape(never_ascii_escape),
    )
    .print(
        result,
        PrintRequest::StandaloneNode {
            node,
            writer: StandaloneWriter::MultiLine,
        },
        None,
    )
    .expect("standalone node prints")
    .text()
    .to_owned()
}

fn print_parsed_template_fragment(kind: SyntaxKind, source_text: &str) -> String {
    let parsed = parse_source_file(
        "parsed-template.ts",
        source_text,
        ParseOptions::default(),
        None,
    );
    let mut pending = vec![parsed.root];
    let mut fragment = None;
    while let Some(node) = pending.pop() {
        let record = parsed.arena.node(node);
        if record.kind == kind {
            fragment = Some(node);
            break;
        }
        for_each_child(&parsed.arena, record, |child| {
            pending.push(child);
            false
        });
    }
    let fragment = fragment.unwrap_or_else(|| panic!("parsed fixture contains {kind:?}"));
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let fragment = arena
        .node_ref(source, fragment)
        .expect("template fragment belongs to mounted source");
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        Vec::new(),
        true,
    )
    .expect("identity transform parsed template");
    print_standalone(&mut result, fragment, false)
}

#[test]
fn synthesized_template_fragments_escape_paired_and_lone_surrogates_like_parsed_tokens() {
    let (mut result, nodes) = synthetic_printer_nodes();
    let templates = nodes.borrow().templates.clone();
    let fixtures = [
        (
            SyntaxKind::NoSubstitutionTemplateLiteral,
            "const value = `\\uD83D\\uDE00\\uD800`;",
            "const value = `😀\\uD800`;",
        ),
        (
            SyntaxKind::TemplateHead,
            "const value = `\\uD83D\\uDE00\\uD800${expression}`;",
            "const value = `😀\\uD800${expression}`;",
        ),
        (
            SyntaxKind::TemplateMiddle,
            "const value = `${left}\\uD83D\\uDE00\\uD800${right}`;",
            "const value = `${left}😀\\uD800${right}`;",
        ),
        (
            SyntaxKind::TemplateTail,
            "const value = `${left}\\uD83D\\uDE00\\uD800`;",
            "const value = `${left}😀\\uD800`;",
        ),
    ];
    for (kind, default_source, never_ascii_source) in fixtures {
        let node = templates
            .iter()
            .find_map(|(candidate, node)| (*candidate == kind).then_some(*node))
            .unwrap_or_else(|| panic!("synthetic fixture contains {kind:?}"));
        assert_eq!(
            print_standalone(&mut result, node, false),
            print_parsed_template_fragment(kind, default_source),
            "default escaping for {kind:?}"
        );
        assert_eq!(
            print_standalone(&mut result, node, true),
            print_parsed_template_fragment(kind, never_ascii_source),
            "neverAsciiEscape for {kind:?}"
        );
    }
}

#[test]
fn jsdoc_function_type_arm_prints_upstream_token_shape() {
    let (mut result, nodes) = synthetic_printer_nodes();
    let node = nodes
        .borrow()
        .js_doc_function_type
        .expect("synthetic JSDoc function type");
    assert_eq!(
        print_standalone(&mut result, node, false),
        "function(x: number): string"
    );
}
