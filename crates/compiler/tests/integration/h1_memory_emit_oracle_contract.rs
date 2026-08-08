use std::collections::BTreeMap;
use std::path::Path;

use base64::Engine;
use serde_json::{json, Value};
use tsc_compiler::{
    EmitArtifactKind, EmitOutcome, EmitWriteMetadata, MemoryOutputSink, ProgramSession,
};
use tsc_diagnostics::{
    compute_line_starts, get_line_and_character_of_position, Diagnostic, MessageChain,
};
use tsc_program::{
    CompilerOptions, PathContext, PreparedProgram, PreparedSourceFile, ProgramOptions, ProgramPath,
};

const ORACLE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h1-emit-oracle.v1.json"
));

fn path(value: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(value, value).expect("trusted oracle path")
}

fn option_bool(options: &Value, name: &str) -> Option<bool> {
    options.get(name).and_then(Value::as_bool)
}

fn option_i32(options: &Value, name: &str) -> Option<i32> {
    options
        .get(name)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn prepared(case: &Value, library: &Value) -> PreparedProgram {
    let options = &case["input"]["compiler_options"];
    let mut builder = PreparedProgram::emitting_builder(
        PathContext::new(path("/project"), true),
        CompilerOptions {
            target: option_i32(options, "target"),
            module: option_i32(options, "module"),
            use_define_for_class_fields: option_bool(options, "useDefineForClassFields"),
            list_emitted_files: option_bool(options, "listEmittedFiles"),
            emit_bom: option_bool(options, "emitBOM"),
            no_emit_on_error: option_bool(options, "noEmitOnError"),
            new_line: option_i32(options, "newLine"),
            ..CompilerOptions::default()
        },
    );
    if let Some(no_lib) = option_bool(options, "noLib") {
        builder.set_program_options(ProgramOptions::default().with_no_lib(no_lib));
    }
    for file in case["input"]["root_files"]
        .as_array()
        .expect("oracle root files")
    {
        let file_name = file["path"].as_str().expect("oracle root path");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(file["utf8_base64"].as_str().expect("oracle source bytes"))
            .expect("valid oracle base64");
        let text = String::from_utf8(bytes).expect("oracle source is UTF-8");
        let source = builder
            .add_source_file(PreparedSourceFile::new(path(file_name), text))
            .expect("add oracle source");
        builder.add_root_file(source).expect("add oracle root");
    }
    let library_path = library["path"].as_str().expect("oracle library path");
    let library_bytes = base64::engine::general_purpose::STANDARD
        .decode(
            library["utf8_base64"]
                .as_str()
                .expect("oracle library bytes"),
        )
        .expect("valid oracle library base64");
    let library_source = builder
        .add_source_file(PreparedSourceFile::new(
            path(library_path),
            String::from_utf8(library_bytes).expect("oracle library UTF-8"),
        ))
        .expect("add oracle library");
    builder
        .add_root_file(library_source)
        .expect("add oracle library root");
    builder.build().expect("build emitting oracle program")
}

fn source_texts(case: &Value) -> BTreeMap<String, String> {
    case["input"]["root_files"]
        .as_array()
        .expect("oracle root files")
        .iter()
        .map(|file| {
            let path = file["path"].as_str().expect("oracle path").to_owned();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(file["utf8_base64"].as_str().expect("oracle source bytes"))
                .expect("valid oracle base64");
            (path, String::from_utf8(bytes).expect("oracle source UTF-8"))
        })
        .collect()
}

fn normalize_chain(chain: &MessageChain) -> Value {
    json!({
        "text": chain.text,
        "code": chain.code,
        "category": chain.category.name(),
        "next_present": chain.next_present,
        "next": chain.next.iter().map(normalize_chain).collect::<Vec<_>>(),
    })
}

fn optional_string(value: Option<&str>) -> Value {
    json!({"present": value.is_some(), "value": value})
}

fn optional_u32(value: Option<u32>) -> Value {
    json!({"present": value.is_some(), "value": value})
}

fn optional_bool(value: Option<bool>) -> Value {
    json!({"present": value.is_some(), "value": value})
}

fn normalize_diagnostic(diagnostic: &Diagnostic, sources: &BTreeMap<String, String>) -> Value {
    let location = diagnostic
        .file_name
        .as_ref()
        .zip(diagnostic.start)
        .and_then(|(file_name, start)| {
            sources
                .get(file_name)
                .map(|text| get_line_and_character_of_position(&compute_line_starts(text), start))
        });
    json!({
        "file": optional_string(diagnostic.file_name.as_deref()),
        "start": optional_u32(diagnostic.start),
        "length": optional_u32(diagnostic.length),
        "line": optional_u32(location.map(|location| location.line)),
        "column": optional_u32(location.map(|location| location.character)),
        "code": diagnostic.code(),
        "category": diagnostic.category().name(),
        "chain": normalize_chain(&diagnostic.message),
        "related_information_present": diagnostic.related_information_present
            || !diagnostic.related.is_empty(),
        "related": [],
        "reports_unnecessary": optional_bool(diagnostic.reports_unnecessary),
        "reports_deprecated": optional_bool(diagnostic.reports_deprecated),
        "source": optional_string(diagnostic.source.as_deref()),
    })
}

fn assert_outcome(
    case_id: &str,
    expected: &Value,
    outcome: &EmitOutcome,
    sources: &BTreeMap<String, String>,
) {
    assert_eq!(
        outcome.emit_skipped(),
        expected["emit_skipped"].as_bool().expect("emitSkipped"),
        "{case_id} emitSkipped"
    );
    let expected_diagnostics = expected["emit_diagnostics"]
        .as_array()
        .expect("emit diagnostics");
    let actual_diagnostics = outcome
        .diagnostics()
        .iter()
        .map(|diagnostic| normalize_diagnostic(diagnostic, sources))
        .collect::<Vec<_>>();
    assert_eq!(
        actual_diagnostics, *expected_diagnostics,
        "{case_id} diagnostics"
    );

    let expected_emitted_present = expected["emitted_files_present"]
        .as_bool()
        .expect("emitted files presence");
    assert_eq!(
        outcome.emitted_files().is_some(),
        expected_emitted_present,
        "{case_id} emittedFiles presence"
    );
    let actual_emitted = outcome
        .emitted_files()
        .unwrap_or_default()
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let expected_emitted = expected["emitted_files"]
        .as_array()
        .expect("emitted files")
        .iter()
        .map(|path| path.as_str().expect("emitted path").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(actual_emitted, expected_emitted, "{case_id} emittedFiles");
    assert_eq!(
        outcome.source_maps().is_some(),
        expected["source_maps_present"]
            .as_bool()
            .expect("source maps presence"),
        "{case_id} sourceMaps presence"
    );
}

fn assert_writes(case_id: &str, expected: &[Value], sink: &MemoryOutputSink) {
    assert_eq!(sink.writes().len(), expected.len(), "{case_id} write count");
    for (index, (actual, expected)) in sink.writes().iter().zip(expected).enumerate() {
        let label = format!("{case_id} write {index}");
        assert_eq!(
            actual.path(),
            Path::new(expected["path"].as_str().expect("write path")),
            "{label} path"
        );
        assert_eq!(actual.kind(), EmitArtifactKind::JavaScript, "{label} kind");
        assert_eq!(expected["kind"], "javascript", "{label} oracle kind");
        assert_eq!(
            actual.callback_text(),
            expected["callback_text"].as_str().expect("callback text"),
            "{label} callback text"
        );
        assert_eq!(
            actual.write_byte_order_mark(),
            expected["write_byte_order_mark"]
                .as_bool()
                .expect("BOM flag"),
            "{label} BOM"
        );
        let expected_materialized = base64::engine::general_purpose::STANDARD
            .decode(
                expected["materialized_utf8_base64"]
                    .as_str()
                    .expect("materialized bytes"),
            )
            .expect("materialized base64");
        assert_eq!(
            actual.materialized_bytes().as_ref(),
            expected_materialized,
            "{label} materialized bytes"
        );
        assert_eq!(
            actual.source_files().is_some(),
            expected["source_files_present"]
                .as_bool()
                .expect("sourceFiles presence"),
            "{label} sourceFiles presence"
        );
        let actual_sources = actual
            .source_files()
            .unwrap_or_default()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let expected_sources = expected["source_files"]
            .as_array()
            .expect("sourceFiles")
            .iter()
            .map(|path| path.as_str().expect("source path").to_owned())
            .collect::<Vec<_>>();
        assert_eq!(actual_sources, expected_sources, "{label} sourceFiles");

        let metadata = match actual.metadata().expect("text metadata") {
            EmitWriteMetadata::Text(metadata) => metadata,
            EmitWriteMetadata::BuildInfo(_) => panic!("{label} build-info metadata"),
        };
        let expected_metadata = &expected["metadata"]["value"];
        assert_eq!(
            expected_metadata["own_keys"],
            json!(["diagnostics", "sourceMapUrlPos"]),
            "{label} metadata keys"
        );
        assert!(
            metadata.diagnostics().is_empty(),
            "{label} transform diagnostics"
        );
        assert_eq!(
            metadata
                .source_map_url_position()
                .map(|value| value.value()),
            expected_metadata["source_map_url_position_utf16"]["value"]
                .as_u64()
                .map(|value| value as u32),
            "{label} sourceMapUrlPos"
        );
        assert_eq!(
            expected["sink_disposition"], "written",
            "{label} disposition"
        );
    }
}

#[test]
fn every_admitted_h1_case_matches_the_vendored_callback_oracle_twice() {
    let oracle: Value = serde_json::from_slice(ORACLE_BYTES).expect("H1 emit oracle JSON");
    let cases = oracle["cases"].as_array().expect("oracle cases");
    let admitted = cases
        .iter()
        .filter(|case| case["input"]["classification"] == "admitted")
        .collect::<Vec<_>>();
    assert_eq!(admitted.len(), 5, "frozen admitted-case count");

    for case in admitted {
        let case_id = case["input"]["id"].as_str().expect("case id");
        let sources = source_texts(case);
        let library = &oracle["oracle_environment"]["library"];
        let mut first_sink = MemoryOutputSink::new();
        let first = ProgramSession::new(prepared(case, library))
            .emit(&mut first_sink)
            .unwrap_or_else(|error| panic!("{case_id} first emit failed: {error}"));
        let mut second_sink = MemoryOutputSink::new();
        let second = ProgramSession::new(prepared(case, library))
            .emit(&mut second_sink)
            .unwrap_or_else(|error| panic!("{case_id} second emit failed: {error}"));

        assert_eq!(first, second, "{case_id} repeated outcome");
        assert_eq!(first_sink, second_sink, "{case_id} repeated callbacks");
        assert_outcome(
            case_id,
            &case["observation"]["emit_result"],
            &first,
            &sources,
        );
        assert_writes(
            case_id,
            case["observation"]["writes"]
                .as_array()
                .expect("oracle writes"),
            &first_sink,
        );
    }
}
