use std::borrow::Cow;
use std::collections::{BTreeSet, VecDeque};
use std::path::Path;
use std::path::PathBuf;

use tsc_emitter::{
    EmitArtifact, EmitArtifactKind, EmitBuildInfoMetadata, EmitFileSystem, EmitIoError,
    EmitIoOperation, EmitTextMetadata, EmitWriteDisposition, EmitWriteMetadata, FsOutputSink,
    GeneratedUtf16Position, MemoryOutputSink, OutputSink,
};

#[derive(Debug, Eq, PartialEq)]
enum FileSystemCall {
    Write(PathBuf, Vec<u8>),
    Exists(PathBuf),
    Create(PathBuf),
}

#[derive(Default)]
struct ObservedFileSystem {
    calls: Vec<FileSystemCall>,
    directories: BTreeSet<PathBuf>,
    write_results: VecDeque<Result<(), String>>,
    create_failure: Option<(PathBuf, String)>,
}

impl EmitFileSystem for ObservedFileSystem {
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.calls
            .push(FileSystemCall::Write(path.to_path_buf(), bytes.to_vec()));
        self.write_results.pop_front().unwrap_or(Ok(()))
    }

    fn create_directory(&mut self, path: &Path) -> Result<(), String> {
        self.calls.push(FileSystemCall::Create(path.to_path_buf()));
        if let Some((failed_path, message)) = &self.create_failure {
            if failed_path == path {
                return Err(message.clone());
            }
        }
        self.directories.insert(path.to_path_buf());
        Ok(())
    }

    fn directory_exists(&mut self, path: &Path) -> bool {
        self.calls.push(FileSystemCall::Exists(path.to_path_buf()));
        self.directories.contains(path)
    }
}

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

#[test]
fn filesystem_sink_writes_once_without_parent_observations_on_success() {
    let artifact = EmitArtifact::javascript(
        "/project/out.js",
        "export {};\n",
        false,
        Some(Vec::new()),
        EmitTextMetadata::default(),
    );
    let mut filesystem = ObservedFileSystem::default();
    let mut sink = FsOutputSink::new(&mut filesystem);

    assert_eq!(
        sink.write(artifact).expect("first write succeeds"),
        EmitWriteDisposition::Written
    );
    assert_eq!(
        filesystem.calls,
        [FileSystemCall::Write(
            PathBuf::from("/project/out.js"),
            b"export {};\n".to_vec(),
        )]
    );
}

#[test]
fn filesystem_sink_creates_missing_parents_outward_in_and_retries_with_bom() {
    let artifact = EmitArtifact::javascript(
        "/project/generated/nested/out.js",
        "value;\n",
        true,
        Some(Vec::new()),
        EmitTextMetadata::default(),
    );
    let mut filesystem = ObservedFileSystem {
        directories: [PathBuf::from("/project")].into_iter().collect(),
        write_results: [Err("first write".to_owned()), Ok(())]
            .into_iter()
            .collect(),
        ..ObservedFileSystem::default()
    };
    let mut sink = FsOutputSink::new(&mut filesystem);

    assert_eq!(
        sink.write(artifact).expect("retry succeeds"),
        EmitWriteDisposition::Written
    );
    let materialized = [&[0xEF, 0xBB, 0xBF][..], b"value;\n"].concat();
    assert_eq!(
        filesystem.calls,
        [
            FileSystemCall::Write(
                PathBuf::from("/project/generated/nested/out.js"),
                materialized.clone(),
            ),
            FileSystemCall::Exists(PathBuf::from("/project/generated/nested")),
            FileSystemCall::Exists(PathBuf::from("/project/generated")),
            FileSystemCall::Exists(PathBuf::from("/project")),
            FileSystemCall::Create(PathBuf::from("/project/generated")),
            FileSystemCall::Create(PathBuf::from("/project/generated/nested")),
            FileSystemCall::Write(
                PathBuf::from("/project/generated/nested/out.js"),
                materialized,
            ),
        ]
    );
}

#[test]
fn filesystem_sink_reports_create_failure_without_retrying_the_file() {
    let artifact = EmitArtifact::javascript(
        "/project/generated/nested/out.js",
        "",
        false,
        Some(Vec::new()),
        EmitTextMetadata::default(),
    );
    let mut filesystem = ObservedFileSystem {
        directories: [PathBuf::from("/project")].into_iter().collect(),
        write_results: [Err("first write is intentionally hidden".to_owned())]
            .into_iter()
            .collect(),
        create_failure: Some((
            PathBuf::from("/project/generated"),
            "stable create failure".to_owned(),
        )),
        ..ObservedFileSystem::default()
    };
    let mut sink = FsOutputSink::new(&mut filesystem);

    let error = sink.write(artifact).expect_err("parent creation fails");
    assert_eq!(error.operation(), EmitIoOperation::CreateParentDirectory);
    assert_eq!(error.path(), Path::new("/project/generated"));
    assert_eq!(error.message(), "stable create failure");
    assert_eq!(
        filesystem
            .calls
            .iter()
            .filter(|call| matches!(call, FileSystemCall::Write(_, _)))
            .count(),
        1
    );
}

#[test]
fn filesystem_sink_reports_only_the_final_retry_failure() {
    let artifact = EmitArtifact::javascript(
        "/project/out.js",
        "",
        false,
        Some(Vec::new()),
        EmitTextMetadata::default(),
    );
    let mut filesystem = ObservedFileSystem {
        directories: [PathBuf::from("/project")].into_iter().collect(),
        write_results: [
            Err("discarded first failure".to_owned()),
            Err("stable retry failure".to_owned()),
        ]
        .into_iter()
        .collect(),
        ..ObservedFileSystem::default()
    };
    let mut sink = FsOutputSink::new(&mut filesystem);

    let error = sink.write(artifact).expect_err("retry fails");
    assert_eq!(error.operation(), EmitIoOperation::WriteFile);
    assert_eq!(error.path(), Path::new("/project/out.js"));
    assert_eq!(error.message(), "stable retry failure");
}
