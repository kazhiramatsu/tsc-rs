use std::borrow::Cow;
use std::path::PathBuf;

use tsc_emitter::{
    EmitArtifact, EmitArtifactKind, EmitBuildInfoMetadata, EmitIoError, EmitIoOperation,
    EmitTextMetadata, EmitWriteDisposition, EmitWriteMetadata, GeneratedUtf16Position,
    MemoryOutputSink, OutputSink,
};

#[test]
fn artifact_retains_callback_bytes_bom_provenance_and_metadata_independently() {
    let metadata = EmitTextMetadata::new(Vec::new(), Some(GeneratedUtf16Position::new(4)));
    let artifact = EmitArtifact::javascript(
        "/project/out.js",
        "a😀b\n",
        true,
        Some(vec![PathBuf::from("/project/input.ts")]),
        metadata,
    );

    assert_eq!(artifact.path().to_str(), Some("/project/out.js"));
    assert_eq!(artifact.kind(), EmitArtifactKind::JavaScript);
    assert_eq!(artifact.callback_text(), "a😀b\n");
    assert_eq!(artifact.callback_bytes(), "a😀b\n".as_bytes());
    assert!(artifact.write_byte_order_mark());
    assert_eq!(
        artifact.materialized_bytes(),
        Cow::<[u8]>::Owned([&[0xEF, 0xBB, 0xBF], "a😀b\n".as_bytes()].concat())
    );
    assert_eq!(
        artifact.source_files(),
        Some([PathBuf::from("/project/input.ts")].as_slice())
    );
    let Some(EmitWriteMetadata::Text(metadata)) = artifact.metadata() else {
        panic!("JavaScript callback must carry text metadata");
    };
    assert!(metadata.diagnostics().is_empty());
    assert_eq!(
        metadata.source_map_url_position(),
        Some(GeneratedUtf16Position::new(4))
    );
}

#[test]
fn optional_callback_arguments_preserve_absent_versus_present_empty() {
    let absent = EmitArtifact::javascript(
        "/project/absent.js",
        "",
        false,
        None,
        EmitTextMetadata::default(),
    );
    let present_empty = EmitArtifact::javascript(
        "/project/empty.js",
        "",
        false,
        Some(Vec::new()),
        EmitTextMetadata::default(),
    );
    let map = EmitArtifact::javascript_map("/project/out.js.map", "{}", Some(Vec::new()));
    let build_info = EmitArtifact::build_info(
        "/project/tsconfig.tsbuildinfo",
        "{}",
        EmitBuildInfoMetadata::new(1, "{\"version\":\"6.0.3\"}"),
    );

    assert_eq!(absent.source_files(), None);
    assert_eq!(present_empty.source_files(), Some([].as_slice()));
    assert_eq!(map.metadata(), None);
    assert_eq!(map.kind(), EmitArtifactKind::JavaScriptMap);
    assert_eq!(build_info.source_files(), None);
    let Some(EmitWriteMetadata::BuildInfo(metadata)) = build_info.metadata() else {
        panic!("build info callback must carry typed metadata");
    };
    assert_eq!(metadata.schema_version(), 1);
    assert_eq!(metadata.canonical_json(), "{\"version\":\"6.0.3\"}");
}

#[test]
fn memory_sink_is_ordered_and_reports_only_written() {
    let first = EmitArtifact::javascript_map("/project/out.js.map", "{}", Some(Vec::new()));
    let second = EmitArtifact::javascript(
        "/project/out.js",
        "export {};\n",
        false,
        Some(Vec::new()),
        EmitTextMetadata::default(),
    );
    let mut sink = MemoryOutputSink::new();

    assert_eq!(
        sink.write(first).expect("memory write"),
        EmitWriteDisposition::Written
    );
    assert_eq!(
        sink.write(second).expect("memory write"),
        EmitWriteDisposition::Written
    );
    assert_eq!(
        sink.writes()
            .iter()
            .map(|artifact| artifact.kind())
            .collect::<Vec<_>>(),
        [
            EmitArtifactKind::JavaScriptMap,
            EmitArtifactKind::JavaScript
        ]
    );
    assert_eq!(
        EmitWriteDisposition::SkippedUnchanged,
        EmitWriteDisposition::SkippedUnchanged
    );
}

#[test]
fn sink_io_failures_retain_operation_path_and_stable_message() {
    let error = EmitIoError::new(
        EmitIoOperation::WriteFile,
        "/project/out.js",
        "permission denied",
    );
    assert_eq!(error.operation(), EmitIoOperation::WriteFile);
    assert_eq!(error.path().to_str(), Some("/project/out.js"));
    assert_eq!(error.message(), "permission denied");
    assert_eq!(
        error.to_string(),
        "write output file /project/out.js: permission denied"
    );
}
