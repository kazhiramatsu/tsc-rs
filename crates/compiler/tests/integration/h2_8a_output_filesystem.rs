//! Complete command observations plus the production FsOutputSink protocol.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Value};
use tsc_compiler::{EmitFileSystem, FsOutputSink, MemoryOutputSink, ProgramSession};
use tsc_emitter::{EmitArtifact, EmitIoError, EmitWriteDisposition, OutputSink};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    canonical_emit_path, load_emitting_program, CompilerOptions, LibraryCatalog, PreparedProgram,
    ProgramLoadLimits, ProgramOptions,
};

struct FileSystem<'a> {
    case: &'a Value,
    directories: Vec<String>,
    operations: Vec<Value>,
    materialized_files: Vec<Value>,
    attempts: BTreeMap<PathBuf, usize>,
}

impl<'a> FileSystem<'a> {
    fn new(case: &'a Value) -> Self {
        Self {
            case,
            directories: case["initial_directories"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect(),
            operations: Vec::new(),
            materialized_files: Vec::new(),
            attempts: BTreeMap::new(),
        }
    }

    fn absolute(&self, path: &Path) -> String {
        canonical_emit_path(
            path,
            Path::new(self.case["current_directory"].as_str().unwrap()),
            true,
        )
        .to_string_lossy()
        .into_owned()
    }

    fn observation(&self) -> Value {
        json!({"operations": self.operations, "materialized_files": self.materialized_files,
            "directories": self.directories})
    }
}

impl EmitFileSystem for FileSystem<'_> {
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        let attempt = self.attempts.entry(path.to_path_buf()).or_default();
        *attempt += 1;
        let attempt = *attempt;
        let absolute = self.absolute(path);
        let parent = Path::new(&absolute).parent().unwrap().to_string_lossy();
        let error = if self.case["fault"] == "all-writes" {
            Some("H2.8 controlled write failure")
        } else if self.case["fault"] == "first-write" && attempt == 1 {
            Some("H2.8 discarded first write failure")
        } else if !self
            .directories
            .iter()
            .any(|directory| directory == parent.as_ref())
        {
            Some("H2.8 parent directory is missing")
        } else {
            None
        };
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        self.operations
            .push(json!({"operation":"write-file", "path":path,
            "utf8_base64":encoded, "error":error}));
        if let Some(error) = error {
            return Err(error.to_owned());
        }
        let materialized = json!({"path":absolute,"utf8_base64":encoded});
        if let Some(file) = self
            .materialized_files
            .iter_mut()
            .find(|file| file["path"] == absolute)
        {
            *file = materialized;
        } else {
            self.materialized_files.push(materialized);
        }
        Ok(())
    }

    fn create_directory(&mut self, path: &Path) -> Result<(), String> {
        let error =
            (self.case["fault"] == "create-directory").then_some("H2.8 controlled create failure");
        self.operations
            .push(json!({"operation":"create-directory", "path":path, "error":error}));
        if let Some(error) = error {
            return Err(error.to_owned());
        }
        let absolute = self.absolute(path);
        if !self.directories.contains(&absolute) {
            self.directories.push(absolute);
        }
        Ok(())
    }

    fn directory_exists(&mut self, path: &Path) -> bool {
        let exists = self.directories.contains(&self.absolute(path));
        self.operations
            .push(json!({"operation":"directory-exists", "path":path, "exists":exists}));
        exists
    }
}

struct RecordingSink<'a> {
    inner: FsOutputSink<'a>,
    writes: Vec<EmitArtifact>,
}

impl OutputSink for RecordingSink<'_> {
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        self.writes.push(artifact.clone());
        self.inner.write(artifact)
    }
}

fn prepare(case: &Value, libraries: &[(String, Vec<u8>)]) -> PreparedProgram {
    let mut builder = MemoryCompilerHost::builder(case["current_directory"].as_str().unwrap());
    let mut roots = Vec::new();
    for file in case["files"].as_array().unwrap() {
        let name = file["path"].as_str().unwrap();
        builder = builder.file(name, file["text"].as_str().unwrap().as_bytes());
        roots.push(PathBuf::from(name));
    }
    for (name, bytes) in libraries {
        builder = builder.file(name, bytes.clone());
    }
    let mut options = CompilerOptions::default();
    for (name, value) in case["options"].as_object().unwrap() {
        match name.as_str() {
            "target" => options.target = Some(value.as_i64().unwrap() as i32),
            "module" => options.module = Some(value.as_i64().unwrap() as i32),
            "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
            "outDir" => options.out_dir = value.as_str().map(str::to_owned),
            "strict" => options.strict = value.as_bool(),
            "emitBOM" => options.emit_bom = value.as_bool(),
            "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
            "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
            "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
            other => panic!("unprojected filesystem option {other}"),
        }
    }
    load_emitting_program(
        &builder.build().unwrap(),
        &roots,
        options,
        ProgramOptions::default(),
        &LibraryCatalog::typescript_6_0_3("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .unwrap()
}

#[test]
fn output_filesystem_matches_complete_typescript_observations() {
    let fixture: Value =
        serde_json::from_slice(include_bytes!("../fixtures/output-filesystem.json")).unwrap();
    assert_eq!(fixture["typescript"], "6.0.3");
    assert_eq!(fixture["repetitions"], 2);
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 24);
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/typescript-6.0.3/lib");
    let libraries = std::fs::read_dir(directory)
        .unwrap()
        .map(Result::unwrap)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (name.starts_with("lib.") && name.ends_with(".d.ts"))
                .then(|| (format!("/lib/{name}"), std::fs::read(entry.path()).unwrap()))
        })
        .collect::<Vec<_>>();
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let compared = std::panic::catch_unwind(|| {
            for _ in 0..2 {
                let mut filesystem = FileSystem::new(case);
                let mut sink = RecordingSink {
                    inner: FsOutputSink::new(&mut filesystem),
                    writes: Vec::new(),
                };
                let command = ProgramSession::new(prepare(case, &libraries))
                    .emit_command_for_harness(&mut sink)
                    .unwrap();
                let writes = std::mem::take(&mut sink.writes);
                drop(sink);
                super::h2_7b_w4a_controls::assert_completed_observation(
                    id,
                    command.emit(),
                    command.diagnostics(),
                    Some((command.status_writes().to_vec(), command.exit_code())),
                    &writes,
                    &case["typescript_observation"],
                    true,
                );
                assert_eq!(
                    filesystem.observation(),
                    case["typescript_observation"]["filesystem"],
                    "{id}: exact filesystem protocol"
                );
                if case["fault"] == "none" {
                    let mut memory = MemoryOutputSink::new();
                    let memory_command = ProgramSession::new(prepare(case, &libraries))
                        .emit_command_for_harness(&mut memory)
                        .unwrap();
                    assert_eq!(
                        memory.writes(),
                        writes,
                        "{id}: Memory/Fs callback equivalence"
                    );
                    super::h2_7b_w4a_controls::assert_completed_observation(
                        id,
                        memory_command.emit(),
                        memory_command.diagnostics(),
                        Some((
                            memory_command.status_writes().to_vec(),
                            memory_command.exit_code(),
                        )),
                        memory.writes(),
                        &case["typescript_observation"],
                        true,
                    );
                }
            }
        });
        if let Err(error) = compared {
            let detail = error
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| error.downcast_ref::<&str>().copied())
                .unwrap_or("non-string panic");
            failures.push(format!("{id}: {detail}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} divergent filesystem observations:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
