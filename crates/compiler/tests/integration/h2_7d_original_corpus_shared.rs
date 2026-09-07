//! Original D and D/E candidates through ordinary production Program/command emission.
//! Frozen census/oracle membership is not an admission or a promised success count.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_emitter::{EmitArtifact, EmitArtifactKind, EmitWriteMetadata};
use tsc_host::{CompilerHost, MemoryCompilerHost};
use tsc_program::{
    load_emitting_program, parse_config_root_plan, CompilerConfigHost, CompilerOptions,
    ConfigRootPlanRequest, LibraryCatalog, PreparedProgram, ProgramLoadError, ProgramLoadLimits,
    ProgramLoadOperation, ProgramOptions, ProgramPath,
};

const SOURCE_COMMIT: &str = "050880ce59e30b356b686bd3144efe24f875ebc8";
const CENSUS: (&str, &str) = (
    "ratchets/h2-7de-candidates.v1.json",
    "1af6d75acf8212135a0850c5ff09487a5589de4d0f825ff1f0e9bc8e3f0f141d",
);
const INPUTS: (&str, &str) = (
    "ratchets/h2-7de-candidate-inputs.v1.json",
    "f2e078a6b6d10cd3c6df833584924c18e8f78fe98c1e41621c70e10e215a073a",
);
const ORACLE: (&str, &str) = (
    "ratchets/h2-7de-observations.v1.json",
    "1a1681b2375d27d9012b06e29808aca72aa3e39d1dbc1536b80ba2aadf9e8ce2",
);

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn digest(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn frozen((path, hash): (&str, &str)) -> Value {
    let bytes = std::fs::read(workspace().join(path)).unwrap();
    assert_eq!(digest(&bytes), hash, "frozen {path}");
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["typescript"], "6.0.3");
    assert_eq!(value["source_commit"], SOURCE_COMMIT);
    value
}

fn indexed(artifact: &Value) -> BTreeMap<&str, &Value> {
    let rows = artifact["cases"].as_array().unwrap();
    let index = rows
        .iter()
        .map(|row| (row["case_id"].as_str().unwrap(), row))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(index.len(), rows.len(), "duplicate original case IDs");
    index
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

fn owners(row: &Value) -> Vec<String> {
    strings(&row["required_slices"])
}

fn historical_overlap(row: &Value) -> bool {
    row["parent_membership"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["parent"] == "H2.6c")
}

/// Preserve the pinned 107-file named-library digest, then include the default
/// lib.d.ts wrapper used by the observer's ES5 programs. Both sets use original
/// bytes; there is no installed TypeScript or global host fallback.
fn libraries() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(workspace().join("vendor/typescript-6.0.3/lib")).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        // Match /^lib\..*\.d\.ts$/ without allowing the prefix and suffix
        // to overlap, as they do in "lib.d.ts".
        if name
            .strip_prefix("lib.")
            .is_some_and(|rest| rest.ends_with(".d.ts"))
        {
            files.insert(name, std::fs::read(entry.path()).unwrap());
        }
    }
    assert_eq!(files.len(), 107);
    let mut hash = Sha256::new();
    for (name, bytes) in &files {
        hash.update(name.as_bytes());
        hash.update([0]);
        hash.update(bytes);
        hash.update([0]);
    }
    assert_eq!(
        format!("{:x}", hash.finalize()),
        "9bb5d7e6912946bf4ed42015a9549d5c61f536bdebb0e1f72e3200235c569b22"
    );
    // The frozen observer permits this wrapper under its library-root fallback;
    // 23 original observations (21 D-only) actually load it. Excluding it from
    // the digest must not remove it from the Program's available library inputs.
    let default_wrapper =
        std::fs::read(workspace().join("vendor/typescript-6.0.3/lib/lib.d.ts")).unwrap();
    assert_eq!(
        digest(&default_wrapper),
        "0e6477e5049e579cc5de741a70ac91235252edc0468b2e569f22de9db3357786"
    );
    assert!(files
        .insert("lib.d.ts".to_owned(), default_wrapper)
        .is_none());
    assert_eq!(files.len(), 108);
    files
}

/// The TS observer's directory overlay canonicalizes directory spellings.
/// MemoryCompilerHost deliberately retains raw keys and uses native parent paths:
/// on POSIX, A:/file.ts creates parent A:, and /a/src/file.ts creates /a/src.
/// Adapt only directory existence; preserve bytes, file identity, enumeration,
/// realpath, original roots and every compiler option.
struct OriginalCorpusHost(MemoryCompilerHost);

impl CompilerHost for OriginalCorpusHost {
    fn current_directory(&self) -> Result<PathBuf, tsc_host::HostError> {
        self.0.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.0.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, tsc_host::HostError> {
        self.0.read_file(path)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, tsc_host::HostError> {
        self.0.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, tsc_host::HostError> {
        if self.0.directory_exists(path)? {
            return Ok(true);
        }
        let lookup = path.to_str().map(|name| name.trim_end_matches('/'));
        match lookup {
            Some(name) if !name.is_empty() => self.0.directory_exists(Path::new(name)),
            _ => self.0.directory_exists(path),
        }
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, tsc_host::HostError> {
        self.0.read_directory(path)
    }

    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, tsc_host::HostError> {
        self.0.get_directories(path)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, tsc_host::HostError> {
        self.0.realpath(path)
    }
}

/// The observer overlays shared mounts, per-case writes, missing config, then aliases.
/// Retaining that order matters independently of root order and discovery order.
fn memory_host(
    case: &Value,
    input_artifact: &Value,
    libraries: &BTreeMap<String, Vec<u8>>,
) -> OriginalCorpusHost {
    let input = &case["input"];
    assert_eq!(input["route"], "whole-program");
    let mut files = BTreeMap::<String, Vec<u8>>::new();
    let mut add = |file: &Value| {
        files.insert(
            file["path"].as_str().unwrap().to_owned(),
            file["text"].as_str().unwrap().as_bytes().to_vec(),
        );
    };
    if let Some(mount) = input["shared_mount"].as_str() {
        assert_eq!(mount, "projects");
        for file in input_artifact["shared_mounts"][mount].as_array().unwrap() {
            add(file);
        }
    }
    for file in input["files"].as_array().unwrap() {
        add(file);
    }
    if !input["config"].is_null() {
        let config = &input["config"];
        files
            .entry(config["path"].as_str().unwrap().to_owned())
            .or_insert_with(|| config["text"].as_str().unwrap().as_bytes().to_vec());
    }
    for link in input["vfs_symlinks"].as_array().unwrap() {
        let bytes = files[link["target_path"].as_str().unwrap()].clone();
        files.insert(link["link_path"].as_str().unwrap().to_owned(), bytes);
    }
    let mut builder = MemoryCompilerHost::builder(input["current_directory"].as_str().unwrap())
        .case_sensitive(input["use_case_sensitive_file_names"].as_bool().unwrap());
    for (path, bytes) in files {
        // Two originals use drive-rooted TS paths even on this POSIX host.
        // Pass their original spellings to the existing Program path normalizer.
        assert!(
            path.starts_with('/') || path.as_bytes().get(1..3).is_some_and(|p| p == b":/"),
            "original VFS path {path}"
        );
        builder = builder.file(path, bytes);
    }
    for (name, bytes) in libraries {
        builder = builder.file(format!("/lib/{name}"), bytes.clone());
    }
    for link in input["vfs_symlinks"].as_array().unwrap() {
        builder = builder.realpath(
            link["link_path"].as_str().unwrap(),
            link["target_path"].as_str().unwrap(),
        );
    }
    OriginalCorpusHost(builder.build().unwrap())
}

fn projected_options(case: &Value, host: &dyn CompilerHost) -> (CompilerOptions, ProgramOptions) {
    let input = &case["input"];
    // The fixed observer uses createProgram(original roots, parsed options overlaid
    // with effective_options). It does not run tsc's config-program entry or replace
    // those roots with a second config glob. That object spread retains the
    // enumerable configFilePath, but drops the non-enumerable configFile AST.
    let mut program = if input["config"].is_null() {
        ProgramOptions::default()
    } else {
        let config = &input["config"];
        let name = config["path"].as_str().unwrap();
        let plan = parse_config_root_plan(
            &CompilerConfigHost::new(host),
            ConfigRootPlanRequest {
                file_name: name.to_owned(),
                text: config["text"].as_str().unwrap().to_owned(),
                base_path: Path::new(name)
                    .parent()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            },
        )
        .unwrap();
        assert!(plan.root_parse_diagnostics().is_empty());
        assert!(
            plan.errors().is_empty(),
            "original parseConfig errors: {:?}",
            plan.errors()
        );
        plan.program_options()
            .clone()
            .with_config_file_path(
                plan.program_options()
                    .config_file_path()
                    .expect("the original TS config parse retains configFilePath")
                    .clone(),
            )
            .with_program_owned_config_option_diagnostics()
    };
    let mut options = CompilerOptions::default();
    for (key, value) in case["effective_options"].as_object().unwrap() {
        match key.as_str() {
            "target" => options.target = Some(value.as_i64().unwrap() as i32),
            "module" => options.module = Some(value.as_i64().unwrap() as i32),
            "moduleResolution" => options.module_resolution = Some(value.as_i64().unwrap() as i32),
            "moduleDetection" => options.module_detection = Some(value.as_i64().unwrap() as i32),
            "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
            "allowJs" => options.allow_js = value.as_bool().unwrap(),
            "alwaysStrict" => options.always_strict = Some(value.as_bool().unwrap()),
            "checkJs" => options.check_js = Some(value.as_bool().unwrap()),
            "composite" => options.composite = Some(value.as_bool().unwrap()),
            "declaration" => options.declaration = Some(value.as_bool().unwrap()),
            "declarationMap" => options.declaration_map = Some(value.as_bool().unwrap()),
            "emitDeclarationOnly" => options.emit_declaration_only = Some(value.as_bool().unwrap()),
            "esModuleInterop" => options.es_module_interop = Some(value.as_bool().unwrap()),
            "importHelpers" => options.import_helpers = Some(value.as_bool().unwrap()),
            "incremental" => options.incremental = Some(value.as_bool().unwrap()),
            "inlineSourceMap" => options.inline_source_map = Some(value.as_bool().unwrap()),
            "inlineSources" => options.inline_sources = Some(value.as_bool().unwrap()),
            "isolatedDeclarations" => {
                options.isolated_declarations = Some(value.as_bool().unwrap())
            }
            "noEmit" => options.no_emit = Some(value.as_bool().unwrap()),
            "noErrorTruncation" => options.no_error_truncation = Some(value.as_bool().unwrap()),
            "noImplicitAny" => options.no_implicit_any = Some(value.as_bool().unwrap()),
            "noResolve" => options.no_resolve = Some(value.as_bool().unwrap()),
            "resolveJsonModule" => options.resolve_json_module = Some(value.as_bool().unwrap()),
            "skipDefaultLibCheck" => {
                options.skip_default_lib_check = Some(value.as_bool().unwrap())
            }
            "sourceMap" => options.source_map = Some(value.as_bool().unwrap()),
            "strict" => options.strict = Some(value.as_bool().unwrap()),
            "strictNullChecks" => options.strict_null_checks = Some(value.as_bool().unwrap()),
            "useDefineForClassFields" => {
                options.use_define_for_class_fields = Some(value.as_bool().unwrap())
            }
            "baseUrl" => options.base_url = Some(value.as_str().unwrap().to_owned()),
            "declarationDir" => options.declaration_dir = Some(value.as_str().unwrap().to_owned()),
            "ignoreDeprecations" => {
                options.ignore_deprecations = Some(value.as_str().unwrap().to_owned())
            }
            "mapRoot" => options.map_root = Some(value.as_str().unwrap().to_owned()),
            "outDir" => options.out_dir = Some(value.as_str().unwrap().to_owned()),
            "outFile" => options.out_file = Some(value.as_str().unwrap().to_owned()),
            "rootDir" => options.root_dir = Some(value.as_str().unwrap().to_owned()),
            "sourceRoot" => options.source_root = Some(value.as_str().unwrap().to_owned()),
            "noLib" => program = program.with_no_lib(value.as_bool().unwrap()),
            "types" => program = program.with_types(strings(value)),
            "typeRoots" => {
                assert!(host.use_case_sensitive_file_names());
                program = program.with_type_roots(
                    strings(value)
                        .into_iter()
                        .map(|path| {
                            // Both fixed typeRoots inputs are already absolute /types.
                            assert_eq!(path, "/types");
                            ProgramPath::from_trusted_parts(&path, &path).unwrap()
                        })
                        .collect(),
                );
            }
            "lib" => {
                // TS's serialized program option uses file names; Rust's existing
                // public option contract uses corresponding catalog keys.
                let catalog = LibraryCatalog::typescript_6_0_3("/lib");
                options.lib = Some(
                    strings(value)
                        .into_iter()
                        .map(|name| {
                            let key = name
                                .strip_prefix("lib.")
                                .and_then(|s| s.strip_suffix(".d.ts"))
                                .unwrap()
                                .to_owned();
                            assert_eq!(catalog.option_file_name(&key), Some(name.as_str()));
                            key
                        })
                        .collect(),
                );
            }
            "traceResolution" => {
                // The frozen observer deliberately supplies host.trace() {}.
                // This host-only reporting flag has no CompilerOptions field and
                // contributes neither command status nor diagnostic observations.
                assert_eq!(value, &json!(true));
            }
            other => panic!("unprojected original option {other}"),
        }
    }
    if case["effective_options"].get("allowJs").is_none() {
        options.allow_js = options.check_js == Some(true);
    }
    if let Some(name) = input["default_library_file_name"].as_str() {
        program = program.with_default_library_file_name(name);
    }
    (options, program)
}

fn prepared(case: &Value, host: &dyn CompilerHost) -> Result<PreparedProgram, ProgramLoadError> {
    let (options, program) = projected_options(case, host);
    let roots = strings(&case["input"]["roots"])
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    load_emitting_program(
        host,
        &roots,
        options,
        program,
        &LibraryCatalog::typescript_6_0_3("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
}

fn message(chain: &MessageChain, indent: usize, text: &mut String) {
    if indent != 0 {
        text.push('\n');
        text.push_str(&"  ".repeat(indent));
    }
    text.push_str(&chain.text);
    for next in &chain.next {
        message(next, indent + 1, text);
    }
}

fn diagnostics(diagnostics: &[Diagnostic]) -> Value {
    json!(diagnostics.iter().map(|d| {
        let mut text = String::new(); message(&d.message, 0, &mut text);
        let related = (d.related_information_present || !d.related.is_empty()).then(|| d.related.iter().map(|r| {
            let mut text = String::new(); message(&r.message, 0, &mut text);
            json!({"code":r.message.code,"category":format!("{:?}",r.message.category),
                "file":r.file_name,"start":r.start,"length":r.length,"message":text,"related_information":null})
        }).collect::<Vec<_>>());
        json!({"code":d.code(),"category":format!("{:?}",d.category()),"file":d.file_name,
            "start":d.start,"length":d.length,"message":text,"related_information":related})
    }).collect::<Vec<_>>())
}

fn write(index: usize, artifact: &EmitArtifact) -> Value {
    let path = artifact.path().to_string_lossy();
    let kind = match artifact.kind() {
        EmitArtifactKind::JavaScript => "javascript",
        EmitArtifactKind::Declaration => "declaration",
        EmitArtifactKind::JavaScriptMap | EmitArtifactKind::DeclarationMap => "source-map",
        EmitArtifactKind::BuildInfo => panic!("BLD1 output escaped D-only comparison"),
    };
    let (keys, position, data_diagnostics) = match artifact.metadata() {
        Some(EmitWriteMetadata::Text(data)) => (
            json!(["sourceMapUrlPos", "diagnostics"]),
            json!(data.source_map_url_position().map(|p| p.value())),
            diagnostics(data.diagnostics()),
        ),
        None => (Value::Null, Value::Null, Value::Null),
        _ => panic!("unexpected original callback metadata"),
    };
    json!({"index":index,"path":path,"kind":kind,
        "callback_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.callback_bytes()),
        "callback_utf8_bytes":artifact.callback_bytes().len(),"write_byte_order_mark":artifact.write_byte_order_mark(),
        "materialized_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.materialized_bytes()),
        "materialized_utf8_bytes":artifact.materialized_bytes().len(),
        // OutputSink::write's Result is the typed equivalent of onError.
        "on_error_callback_present":true,"source_files":artifact.source_files(),
        "data_present":artifact.metadata().is_some(),"data_keys":keys,
        "data_source_map_url_pos":position,"data_diagnostics":data_diagnostics,"data_build_info":null})
}

fn observe(case: &Value, host: &dyn CompilerHost, libraries: &BTreeMap<String, Vec<u8>>) -> Value {
    let program =
        prepared(case, host).unwrap_or_else(|error| panic!("load_emitting_program: {error:?}"));
    let mut sources = Vec::new();
    let mut standard_libraries = Vec::new();
    for source in program.source_files() {
        let path = source.path().display();
        let name = path.to_str().unwrap();
        // Check actual loaded source bytes independently from its observed order.
        let bytes = host.read_file(path).unwrap().unwrap();
        let text = std::str::from_utf8(&bytes)
            .unwrap()
            .strip_prefix('\u{feff}')
            .unwrap_or(std::str::from_utf8(&bytes).unwrap());
        assert_eq!(
            digest(source.text()),
            digest(text),
            "loaded source bytes {name}"
        );
        if let Some(basename) = name
            .strip_prefix("/lib/")
            .filter(|n| libraries.contains_key(*n))
        {
            standard_libraries.push(basename.to_owned());
        } else {
            sources.push(name.to_owned());
        }
    }
    let mut sink = MemoryOutputSink::new();
    let command = ProgramSession::new(program)
        .emit_command_for_harness(&mut sink)
        .unwrap_or_else(|error| {
            let writes = sink
                .writes()
                .iter()
                .map(|a| (a.path(), digest(a.callback_bytes())))
                .collect::<Vec<_>>();
            panic!(
                "ordinary Program/command: {error:?}; partial callback paths/SHA256: {writes:?}"
            );
        });
    let outcome = command.emit();
    let activity = outcome.h2_activity();
    for (slice, requested) in [
        (
            tsc_emitter::H2RuntimeSlice::H2_7d,
            case["effective_options"]["outFile"]
                .as_str()
                .is_some_and(|path| !path.is_empty()),
        ),
        (
            tsc_emitter::H2RuntimeSlice::H2_7e,
            case["effective_options"]["declarationMap"] == true,
        ),
    ] {
        assert_eq!(
            activity.runtime_slice(slice),
            u64::from(requested),
            "{}: {} request",
            case["case_id"],
            slice.name()
        );
    }
    for slice in tsc_emitter::H2RuntimeSlice::ALL {
        if slice == tsc_emitter::H2RuntimeSlice::H2_7a || slice > tsc_emitter::H2RuntimeSlice::H2_7e
        {
            assert_eq!(
                activity.runtime_slice(slice),
                0,
                "{}: inactive {}",
                case["case_id"],
                slice.name()
            );
        }
    }
    let maps = outcome.source_maps().map(|maps| {
        maps.iter().map(|map| json!({
        "input_source_file_names":map.input_source_files(),"source_map_json":map.canonical_json()
    })).collect::<Vec<_>>()
    });
    json!({"program_source_order":sources,"standard_libraries":standard_libraries,
        "writes":sink.writes().iter().enumerate().map(|(i,a)| write(i,a)).collect::<Vec<_>>(),
        "reported_diagnostics":diagnostics(command.diagnostics()),"emit_refused":outcome.emit_skipped(),
        "emit_result":{"emit_skipped":outcome.emit_skipped(),"diagnostics":diagnostics(outcome.diagnostics()),
            "emitted_files":outcome.emitted_files(),"source_maps":maps},
        // Real cli::emit_command_status producer; no test-side status/exit inference.
        "status_writes":command.status_writes(),"exit_code":command.exit_code()})
}

fn short(value: &Value) -> String {
    let text = value.to_string();
    let mut result = text.chars().take(160).collect::<String>();
    if text.chars().count() > 160 {
        result.push_str("...");
    }
    result
}

fn difference(actual: &Value, expected: &Value, path: &str) -> Option<String> {
    if actual == expected {
        return None;
    }
    match (actual, expected) {
        (Value::Object(a), Value::Object(e)) => {
            if a.keys().ne(e.keys()) {
                return Some(format!(
                    "{path}: object keys {:?} != {:?}",
                    a.keys(),
                    e.keys()
                ));
            }
            for (key, value) in e {
                if let Some(diff) = difference(&a[key], value, &format!("{path}.{key}")) {
                    return Some(diff);
                }
            }
        }
        (Value::Array(a), Value::Array(e)) => {
            if a.len() != e.len() {
                return Some(format!("{path}: length {} != {}", a.len(), e.len()));
            }
            for (index, (a, e)) in a.iter().zip(e).enumerate() {
                if let Some(diff) = difference(a, e, &format!("{path}[{index}]")) {
                    return Some(diff);
                }
            }
        }
        (Value::String(a), Value::String(e)) if path.ends_with("_base64") => {
            let decode = |s: &str| base64::engine::general_purpose::STANDARD.decode(s).unwrap();
            let (a, e) = (decode(a), decode(e));
            let offset = a
                .iter()
                .zip(&e)
                .position(|(a, e)| a != e)
                .unwrap_or(a.len().min(e.len()));
            return Some(format!(
                "{path}: UTF-8 byte {offset}, actual length {}, expected length {}",
                a.len(),
                e.len()
            ));
        }
        _ => {}
    }
    Some(format!(
        "{path}: actual {} != expected {}",
        short(actual),
        short(expected)
    ))
}

fn assert_complete(actual: &Value, expected: &Value) {
    assert_eq!(
        actual.as_object().unwrap().keys().collect::<Vec<_>>(),
        expected.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    let differences = expected
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(field, expected)| difference(&actual[field], expected, field))
        .collect::<Vec<_>>();
    assert!(differences.is_empty(), "{}", differences.join("; "));
}

fn panic_text(error: &(dyn std::any::Any + Send)) -> String {
    error
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| error.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "non-string panic".to_owned())
}

fn assert_original_source(row: &Value) {
    let source = &row["source"];
    let path = workspace()
        .join("ts-tests/tests/cases")
        .join(row["suite"].as_str().unwrap())
        .join(source["path"].as_str().unwrap());
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(json!(bytes.len()), source["bytes"], "{path:?}");
    assert_eq!(json!(digest(&bytes)), source["sha256"], "{path:?}");
}

/// Preserve the 32 later-owner intersections without treating facet comparisons or
/// a present typed error as a complete TS tuple. Only noEmit's loader gate is
/// independently established here; the other entrances are recorded for review.
fn assert_reference(
    row: &Value,
    case: &Value,
    artifact: &Value,
    libraries: &BTreeMap<String, Vec<u8>>,
) {
    let options = &case["effective_options"];
    let owner = owners(row);
    let gate = match owner.as_slice() {
        [d, a] if d == "H2.7d" && a == "H2.8a" => {
            assert!(options.get("rootDir").is_some() || options.get("outDir").is_some());
            "ordinary emitter validate_emit_request: rootDir/outDir"
        }
        [d, b] if d == "H2.7d" && b == "H2.8b" => {
            if options["incremental"] == true || options["composite"] == true {
                "ordinary emitter validate_emit_options: incremental/composite; build runtime BLD1"
            } else if options["importHelpers"] == true {
                "bundle importHelpers: post-outFile production refusal remains to be established"
            } else {
                assert_eq!(case["input"]["use_case_sensitive_file_names"], false);
                "case-insensitive bundle host: post-outFile production refusal remains to be established"
            }
        }
        [d, n] if d == "H2.7d" && n == "H2.9" => {
            if options["noEmit"] == true {
                let host = memory_host(case, artifact, libraries);
                for _ in 0..2 {
                    let error = prepared(case, &host).unwrap_err();
                    assert!(
                        matches!(error, ProgramLoadError::InvalidInput {
                        operation: ProgramLoadOperation::ValidateOptions, path: None, ref detail,
                    } if detail == "emitting program rejects effective compilerOptions.noEmit=true"),
                        "{error:?}"
                    );
                }
                "load_emitting_program ValidateOptions: noEmit=true, verified twice"
            } else {
                assert_eq!(
                    row["case_id"],
                    "typescript-6.0.3/compiler/jsFileCompilationTypeAssertions.ts#default"
                );
                assert_eq!(
                    row["source_facts"]["parse_diagnostic_units"],
                    json!([{"path":"/src/a.js","codes":[17008,1005]}])
                );
                "ordinary transform ParseDiagnosticsDeferred/H2.9; not a successful emit"
            }
        }
        _ => panic!("unexpected D reference owners {owner:?}"),
    };
    eprintln!("H2.7d original REFERENCE {}: {gate}", row["case_id"]);
}

pub(super) fn assert_original_corpus(workspace_root: &Path) -> BTreeSet<String> {
    assert_eq!(
        workspace_root.canonicalize().unwrap(),
        workspace().canonicalize().unwrap(),
        "H2.7d caller must select the compiled workspace",
    );
    let census_artifact = frozen(CENSUS);
    let input_artifact = frozen(INPUTS);
    let observation_artifact = frozen(ORACLE);
    assert_eq!(observation_artifact["repetitions"], 2);
    // Check the observer and all its input dependencies, without re-running TS
    // or requiring historical qualification files to be rewritten for this test.
    for pin in observation_artifact["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .chain(std::iter::once(&observation_artifact["generator"]))
    {
        let path = pin["path"].as_str().unwrap();
        assert_eq!(
            json!(digest(std::fs::read(workspace().join(path)).unwrap())),
            pin["sha256"],
            "{path}"
        );
    }
    let census = indexed(&census_artifact);
    let inputs = indexed(&input_artifact);
    let observations = indexed(&observation_artifact);
    assert_eq!(census.len(), 325);
    assert_eq!(inputs.len(), 325);
    assert_eq!(observations.len(), 323);
    assert!(census.keys().eq(inputs.keys()));
    assert_eq!(
        input_artifact["shared_mounts"]["projects"]
            .as_array()
            .unwrap()
            .len(),
        233
    );
    let libraries = libraries();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut overlaps = BTreeMap::<String, usize>::new();
    for (id, row) in &census {
        let input = inputs[id];
        assert_eq!(row["source"], input["source"]);
        assert_eq!(row["suite"], input["suite"]);
        assert_eq!(row["runtime_admitted"], false);
        assert_eq!(row["disposition"], "candidate-only");
        let group = owners(row).join(",");
        *counts.entry(group.clone()).or_default() += 1;
        *overlaps.entry(group).or_default() += usize::from(historical_overlap(row));
        if input["input"]["route"] == "transpile-api" {
            assert_eq!(owners(row), ["H2.7e", "H2.8c"]);
            assert!(!observations.contains_key(id));
            assert_eq!(input["input"]["units"].as_array().unwrap().len(), 5);
            continue;
        }
        let reference = observations[id];
        // The SHA-pinned input file fixes every row byte, including original JSON
        // member order used by JS's JSON.stringify hash. Join that row identity;
        // reserializing serde_json's sorted object map would hash different bytes.
        assert_eq!(row["input_sha256"], reference["input_sha256"]);
        assert_eq!(row["required_slices"], reference["required_slices"]);
        assert_eq!(reference["repetitions"], 2);
        assert_eq!(reference["disposition"], "typescript-reference-only");
        if owners(row).iter().any(|owner| owner == "H2.7d") {
            assert_original_source(row);
        }
    }
    assert_eq!(
        json!(counts),
        json!({"H2.7d":280,"H2.7d,H2.7e":3,"H2.7d,H2.8a":23,
        "H2.7d,H2.8b":5,"H2.7d,H2.9":4,"H2.7e":8,"H2.7e,H2.8c":2})
    );
    assert_eq!(overlaps["H2.7d"], 154);
    assert_eq!(overlaps.values().sum::<usize>(), 177);
    let mut failures = Vec::new();
    let mut compared_ids = BTreeSet::new();
    let mut compared = 0;
    let mut exact = 0;
    let mut exact_overlap = 0;
    let mut references = 0;
    for (id, row) in &census {
        if !owners(row).iter().any(|owner| owner == "H2.7d") {
            continue;
        }
        let case = inputs[id];
        if !matches!(owners(row).as_slice(), [d] if d == "H2.7d")
            && owners(row) != ["H2.7d", "H2.7e"]
        {
            references += 1;
            assert_reference(row, case, &input_artifact, &libraries);
            continue;
        }
        compared += 1;
        let expected = &observations[id]["typescript_observation"];
        let before = failures.len();
        // Catch each repetition independently: one failure cannot skip its second
        // fresh Program or hide later cases. No checker/library-prefix cache reuse.
        for repetition in 0..2 {
            let result = std::panic::catch_unwind(|| {
                let host = memory_host(case, &input_artifact, &libraries);
                let actual = observe(case, &host, &libraries);
                // Optional failure artifacts preserve the complete observations;
                // selecting an artifact directory never filters the corpus.
                if actual != *expected {
                    if let Some(directory) = std::env::var_os("TSC_RS_H2_7D_FAILURE_DIR") {
                        let directory = PathBuf::from(directory);
                        std::fs::create_dir_all(&directory).unwrap();
                        let record = json!({
                            "case_id": id,
                            "repetition": repetition + 1,
                            "actual": actual,
                            "expected": expected,
                        });
                        std::fs::write(
                            directory.join(format!("{}-{}.json", digest(id), repetition + 1)),
                            serde_json::to_vec_pretty(&record).unwrap(),
                        )
                        .unwrap();
                    }
                }
                assert_complete(&actual, expected);
            });
            if let Err(error) = result {
                failures.push(format!(
                    "{id} repetition {}: {}",
                    repetition + 1,
                    panic_text(error.as_ref())
                ));
            }
        }
        if failures.len() == before {
            assert!(compared_ids.insert((*id).to_owned()));
            exact += 1;
            exact_overlap += usize::from(historical_overlap(row));
            eprintln!("H2.7d original EXACT x2 {id}");
        }
    }
    assert_eq!(compared, 283);
    assert_eq!(references, 32);
    // Distinct original IDs, never band totals added together. These are local
    // comparison results only; runtime activity/admission remains separately owned.
    eprintln!("H2.7d original: compared={compared}, exact={exact}, references={references}, H2.6c exact overlap={exact_overlap}, exact IDs outside H2.6c={}; union=325, input-preserving repetitions=2", exact - exact_overlap);
    assert!(
        failures.is_empty(),
        "{} failed repetitions:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(exact, 283);
    assert_eq!(exact_overlap, 155);
    assert_eq!(exact - exact_overlap, 128);
    assert_eq!(compared_ids.len(), 283);
    compared_ids
}
