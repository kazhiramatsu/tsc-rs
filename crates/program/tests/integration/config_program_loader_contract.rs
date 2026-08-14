use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tsc_host::{CompilerHost, FsCompilerHost, MemoryCompilerHost};
use tsc_program::{
    decode_host_text, load_config_program, load_config_program_with_no_emit_override,
    load_emitting_config_program, load_emitting_config_program_with_no_emit_override,
    load_emitting_config_program_with_overrides, parse_config_root_plan, CompilerConfigHost,
    ConfigEmitOptionOverrides, ConfigHostError, ConfigHostOperation, ConfigParseHost,
    ConfigProgramLoadError, ConfigRootPlanRequest, LibraryCatalog, PreparedProgramMode,
    ProgramLoadLimits,
};

const LIMITS: ProgramLoadLimits = ProgramLoadLimits::new(128, 512, 32, 1 << 20, 1 << 22);

static NEXT_TEMP_TREE: AtomicU64 = AtomicU64::new(0);

#[test]
fn unsupported_h0_config_scope_fails_at_the_program_gate() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .build()
        .expect("build unsupported-scope host");
    let adapter = ConfigHostAdapter::new(&host);

    let references = parse_config_root_plan(
        &adapter,
        ConfigRootPlanRequest {
            file_name: "/work/tsconfig.json".to_owned(),
            text: r#"{"files":["main.ts"],"references":[{"path":"other"}]}"#.to_owned(),
            base_path: "/work".to_owned(),
        },
    )
    .expect("project-reference config remains observable as a partial plan");
    let references_error = load_config_program_with_no_emit_override(
        &host,
        &references,
        &LibraryCatalog::typescript_6_0_3(PathBuf::from("/work/lib")),
        LIMITS,
    )
    .expect_err("project references must not enter the single-project loader");
    let ConfigProgramLoadError::Program(references_error) = references_error else {
        panic!("project references should be a typed program-scope failure");
    };
    assert_eq!(
        references_error.kind(),
        tsc_program::ProgramLoadErrorKind::Unsupported
    );
    assert!(references_error.to_string().contains("project references"));

    let emit = parse_config_root_plan(
        &adapter,
        ConfigRootPlanRequest {
            file_name: "/work/tsconfig.json".to_owned(),
            text: r#"{"files":["main.ts"],"compilerOptions":{"declaration":true}}"#.to_owned(),
            base_path: "/work".to_owned(),
        },
    )
    .expect("declaration config remains observable as a partial plan");
    let emit_error = load_config_program_with_no_emit_override(
        &host,
        &emit,
        &LibraryCatalog::typescript_6_0_3(PathBuf::from("/work/lib")),
        LIMITS,
    )
    .expect_err("declaration output must not enter the no-emit loader");
    let ConfigProgramLoadError::Program(emit_error) = emit_error else {
        panic!("declaration output should be a typed program-scope failure");
    };
    assert_eq!(
        emit_error.kind(),
        tsc_program::ProgramLoadErrorKind::Unsupported
    );
    assert!(emit_error.to_string().contains("declaration"));
}

#[test]
fn recognized_but_unprojected_config_options_fail_closed() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"rootDir":"src"},"files":["main.ts"]}"#,
        ),
    )
    .expect("rootDir is a recognized partial-plan option");

    let error = load_config_program_with_no_emit_override(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect_err("rootDir must not be silently ignored by the no-emit loader");
    let ConfigProgramLoadError::Program(error) = error else {
        panic!("recognized out-of-scope options must fail at the program gate");
    };
    assert_eq!(error.kind(), tsc_program::ProgramLoadErrorKind::Unsupported);
    assert!(error.to_string().contains("rootDir"));
}

#[test]
fn unsupported_root_config_scopes_fail_closed_even_when_inherited() {
    let host = host();
    for (scope, value) in [
        ("watchOptions", r#"{"watchFile":"useFsEvents"}"#),
        ("typeAcquisition", r#"{"include":["jest"]}"#),
        ("compileOnSave", "true"),
    ] {
        let plan = parse_config_root_plan(
            &ConfigHostAdapter {
                host: &host,
                files: BTreeMap::from([(
                    "/project/base.json".to_owned(),
                    format!(r#"{{"{scope}":{value}}}"#),
                )]),
            },
            request(r#"{"extends":"./base.json","compilerOptions":{"noEmit":true,"noLib":true},"files":["main.ts"]}"#),
        )
        .expect("inherited root scope remains a partial plan");
        let error = load_config_program_with_no_emit_override(
            &host,
            &plan,
            &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
            LIMITS,
        )
        .expect_err("unported root scope must not be silently ignored");
        let ConfigProgramLoadError::Program(error) = error else {
            panic!("unported root scope should be a typed program-scope failure");
        };
        assert_eq!(error.kind(), tsc_program::ProgramLoadErrorKind::Unsupported);
        assert!(error.to_string().contains(scope));
    }
}

struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new() -> Self {
        loop {
            let sequence = NEXT_TEMP_TREE.fetch_add(1, Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock is after the Unix epoch")
                .as_nanos();
            let candidate = std::env::temp_dir().join(format!(
                "tsc-rs-config-program-{timestamp}-{sequence}-{}",
                std::process::id()
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => return Self { root: candidate },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create config-program temp tree: {error}"),
            }
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if !std::thread::panicking() {
                panic!(
                    "remove config-program temp tree {}: {error}",
                    self.root.display()
                );
            }
        }
    }
}

struct ConfigHostAdapter<'a> {
    host: &'a dyn CompilerHost,
    files: BTreeMap<String, String>,
}

impl<'a> ConfigHostAdapter<'a> {
    fn new(host: &'a dyn CompilerHost) -> Self {
        Self {
            host,
            files: BTreeMap::new(),
        }
    }

    fn host_error(operation: ConfigHostOperation, path: &str, detail: &str) -> ConfigHostError {
        ConfigHostError::new(operation, path, detail)
    }
}

impl ConfigParseHost for ConfigHostAdapter<'_> {
    fn use_case_sensitive_file_names(&self) -> bool {
        self.host.use_case_sensitive_file_names()
    }

    fn file_exists(&self, path: &str) -> Result<bool, ConfigHostError> {
        self.host.file_exists(Path::new(path)).map_err(|error| {
            Self::host_error(ConfigHostOperation::FileExists, path, &error.to_string())
        })
    }

    fn read_file(&self, path: &str) -> Result<Option<String>, ConfigHostError> {
        if let Some(text) = self.files.get(path) {
            return Ok(Some(text.clone()));
        }
        self.host
            .read_file(Path::new(path))
            .map_err(|error| {
                Self::host_error(ConfigHostOperation::ReadFile, path, &error.to_string())
            })?
            .map(|bytes| {
                decode_host_text(bytes).map_err(|error| {
                    Self::host_error(ConfigHostOperation::ReadFile, path, &error.to_string())
                })
            })
            .transpose()
    }

    fn read_directory(
        &self,
        directory: &str,
        _extensions: &[&str],
        _excludes: Option<&[String]>,
        _includes: Option<&[String]>,
        _depth: Option<usize>,
    ) -> Result<Vec<String>, ConfigHostError> {
        Err(Self::host_error(
            ConfigHostOperation::ReadDirectory,
            directory,
            "the files-only contract does not enumerate directories",
        ))
    }
}

fn request(text: &str) -> ConfigRootPlanRequest {
    request_at(Path::new("/project"), text)
}

fn request_at(base: &Path, text: &str) -> ConfigRootPlanRequest {
    let base = base.to_str().expect("test path is Unicode");
    ConfigRootPlanRequest {
        file_name: format!("{base}/tsconfig.json"),
        text: text.to_owned(),
        base_path: base.to_owned(),
    }
}

fn host() -> MemoryCompilerHost {
    MemoryCompilerHost::builder("/project")
        .file("/project/main.ts", b"const value: number = 1;\n")
        .build()
        .expect("memory compiler host")
}

#[test]
fn config_plan_loads_no_emit_program_without_reparsing_options() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(r#"{"compilerOptions":{"noEmit":true,"noLib":true},"files":["main.ts"]}"#),
    )
    .expect("parse config plan");

    assert!(plan.diagnostics().next().is_none());
    assert!(plan.option_diagnostics().is_empty());
    assert_eq!(plan.compiler_options().no_emit, Some(true));
    assert_eq!(plan.program_options().no_lib(), Some(true));

    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("load config program");
    assert_eq!(prepared.roots().len(), 1);
    assert_eq!(prepared.roots()[0].path().display(), "/project/main.ts");
    assert_eq!(prepared.compiler_options().no_emit, Some(true));
    assert_eq!(prepared.program_options().no_lib(), Some(true));
}

#[test]
fn conflicting_lib_and_no_lib_are_option_diagnostics_at_both_names() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"lib":["es5"]},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse conflicting-library plan");

    assert_eq!(
        plan.option_diagnostics()
            .iter()
            .map(|diagnostic| (diagnostic.code(), diagnostic.start, diagnostic.length))
            .collect::<Vec<_>>(),
        vec![(5053, Some(34), Some(7)), (5053, Some(47), Some(5))]
    );
    assert_eq!(
        plan.option_diagnostics()[0].message_text(),
        "Option 'lib' cannot be specified with option 'noLib'."
    );
}

#[test]
fn missing_configured_type_retains_ts1419_config_related_information() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let text = r#"{"note":"😀","compilerOptions":{"noEmit":true,"noLib":true,"types":["missing"]},"files":["main.ts"]}"#;
    let plan = parse_config_root_plan(&adapter, request(text)).expect("parse configured types");
    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("load missing configured type as a diagnostic");

    let (_, resolution) = prepared
        .resolutions()
        .type_references()
        .next()
        .expect("configured type owns an authoritative row");
    let [diagnostic] = resolution.diagnostics() else {
        panic!("missing configured type must publish one TS2688 diagnostic");
    };
    assert_eq!(diagnostic.code(), 2688);
    assert!(diagnostic.related_information_present);
    let [related] = diagnostic.related.as_slice() else {
        panic!("TS2688 must point back to compilerOptions.types");
    };
    assert_eq!(related.message.code, 1419);
    assert_eq!(related.file_name.as_deref(), Some("/project/tsconfig.json"));
    let literal_byte = text.find("\"missing\"").expect("types literal span");
    let literal_utf16 = text[..literal_byte].encode_utf16().count() as u32;
    assert_eq!(related.start, Some(literal_utf16));
    assert_eq!(related.length, Some("\"missing\"".len() as u32));
    let auxiliary = prepared.auxiliary_files().collect::<Vec<_>>();
    assert_eq!(auxiliary.len(), 1);
    assert_eq!(auxiliary[0].path().display(), "/project/tsconfig.json");
    assert_eq!(auxiliary[0].text(), text);
}

#[test]
fn missing_default_library_retains_ts1426_target_related_information() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let text = r#"{"note":"😀","compilerOptions":{"noEmit":true,"target":"es5","types":[]},"files":["main.ts"]}"#;
    let plan = parse_config_root_plan(&adapter, request(text)).expect("parse target config");
    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("load missing default library as a diagnostic");

    let diagnostic = prepared
        .diagnostics()
        .program()
        .iter()
        .find(|diagnostic| diagnostic.code() == 6053)
        .expect("missing default library publishes TS6053");
    assert_eq!(
        diagnostic.message.next[0].next[0].text,
        "Default library for target 'es5'"
    );
    assert!(diagnostic.related_information_present);
    let [related] = diagnostic.related.as_slice() else {
        panic!("missing default library must point back to compilerOptions.target");
    };
    assert_eq!(related.message.code, 1426);
    assert_eq!(related.file_name.as_deref(), Some("/project/tsconfig.json"));
    let literal_byte = text.find("\"es5\"").expect("target literal span");
    let literal_utf16 = text[..literal_byte].encode_utf16().count() as u32;
    assert_eq!(related.start, Some(literal_utf16));
    assert_eq!(related.length, Some("\"es5\"".len() as u32));
}

#[test]
fn missing_explicit_library_matches_typescript_without_ts1423_related_information() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"lib":["es5"],"types":[]},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse explicit library config");
    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("load missing explicit library as a diagnostic");

    let diagnostic = prepared
        .diagnostics()
        .program()
        .iter()
        .find(|diagnostic| diagnostic.code() == 6053)
        .expect("missing explicit library publishes TS6053");
    assert_eq!(
        diagnostic.message.next[0].next[0].text,
        "Library 'lib.es5.d.ts' specified in compilerOptions"
    );
    assert!(!diagnostic.related_information_present);
    assert!(diagnostic.related.is_empty());
}

#[test]
fn case_only_alias_retains_ts1410_files_list_related_information() {
    let host = MemoryCompilerHost::builder("/project")
        .case_sensitive(false)
        .file(
            "/project/main.ts",
            b"import { value } from './Value';\n".to_vec(),
        )
        .file("/project/value.ts", b"export const value = 1;\n".to_vec())
        .build()
        .expect("build case-insensitive config host");
    let text = r#"{"note":"😀","compilerOptions":{"noEmit":true,"noLib":true,"types":[]},"files":["main.ts","value.ts"]}"#;
    let plan = parse_config_root_plan(&ConfigHostAdapter::new(&host), request(text))
        .expect("parse case-only alias config");
    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("load case-only alias config");

    let diagnostic = prepared
        .diagnostics()
        .program()
        .iter()
        .find(|diagnostic| diagnostic.code() == 1261)
        .expect("case-only root alias publishes TS1261");
    assert!(diagnostic.related_information_present);
    let [related] = diagnostic.related.as_slice() else {
        panic!("TS1261 must point back to the matching files entry");
    };
    assert_eq!(related.message.code, 1410);
    assert_eq!(related.file_name.as_deref(), Some("/project/tsconfig.json"));
    let literal_byte = text.rfind("\"value.ts\"").expect("files literal span");
    let literal_utf16 = text[..literal_byte].encode_utf16().count() as u32;
    assert_eq!(related.start, Some(literal_utf16));
    assert_eq!(related.length, Some("\"value.ts\"".len() as u32));
}

#[test]
fn case_only_alias_retains_ts1408_include_pattern_related_information() {
    let host = MemoryCompilerHost::builder("/project")
        .case_sensitive(false)
        .file(
            "/project/main.ts",
            b"import { value } from './Value';\n".to_vec(),
        )
        .file("/project/value.ts", b"export const value = 1;\n".to_vec())
        .build()
        .expect("build case-insensitive include host");
    let text = r#"{"note":"😀","compilerOptions":{"noEmit":true,"noLib":true,"types":[]},"include":["**/*.ts"]}"#;
    let plan = parse_config_root_plan(&CompilerConfigHost::new(&host), request(text))
        .expect("parse include-pattern alias config");
    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("load include-pattern alias config");

    let diagnostic = prepared
        .diagnostics()
        .program()
        .iter()
        .find(|diagnostic| diagnostic.code() == 1261)
        .expect("case-only include alias publishes TS1261");
    let inclusion_reasons = &diagnostic.message.next[0].next;
    assert_eq!(inclusion_reasons[1].code, 1407);
    assert_eq!(
        inclusion_reasons[1].text,
        "Matched by include pattern '**/*.ts' in '/project/tsconfig.json'"
    );
    assert!(diagnostic.related_information_present);
    let [related] = diagnostic.related.as_slice() else {
        panic!("TS1261 must point back to the matching include entry");
    };
    assert_eq!(related.message.code, 1408);
    assert_eq!(related.file_name.as_deref(), Some("/project/tsconfig.json"));
    let literal_byte = text.rfind("\"**/*.ts\"").expect("include literal span");
    let literal_utf16 = text[..literal_byte].encode_utf16().count() as u32;
    assert_eq!(related.start, Some(literal_utf16));
    assert_eq!(related.length, Some("\"**/*.ts\"".len() as u32));
}

#[test]
fn case_only_alias_retains_default_include_reason_without_related_information() {
    let host = MemoryCompilerHost::builder("/project")
        .case_sensitive(false)
        .file(
            "/project/main.ts",
            b"import { value } from './Value';\n".to_vec(),
        )
        .file("/project/value.ts", b"export const value = 1;\n".to_vec())
        .build()
        .expect("build case-insensitive default-include host");
    let text = r#"{"compilerOptions":{"noEmit":true,"noLib":true,"types":[]}}"#;
    let plan = parse_config_root_plan(&CompilerConfigHost::new(&host), request(text))
        .expect("parse default-include alias config");
    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("load default-include alias config");

    let diagnostic = prepared
        .diagnostics()
        .program()
        .iter()
        .find(|diagnostic| diagnostic.code() == 1261)
        .expect("case-only default-include alias publishes TS1261");
    let inclusion_reasons = &diagnostic.message.next[0].next;
    assert_eq!(inclusion_reasons[1].code, 1457);
    assert_eq!(
        inclusion_reasons[1].text,
        "Matched by default include pattern '**/*'"
    );
    assert!(!diagnostic.related_information_present);
    assert!(diagnostic.related.is_empty());
}

#[test]
fn case_sensitive_distinct_files_retain_both_files_list_provenance_entries() {
    let host = MemoryCompilerHost::builder("/project")
        .case_sensitive(true)
        .file("/project/Value.ts", b"export {};\n".to_vec())
        .file("/project/value.ts", b"export {};\n".to_vec())
        .build()
        .expect("build case-sensitive config host");
    let text = r#"{"compilerOptions":{"noEmit":true,"noLib":true,"types":[],"forceConsistentCasingInFileNames":false},"files":["Value.ts","value.ts"]}"#;
    let plan = parse_config_root_plan(&ConfigHostAdapter::new(&host), request(text))
        .expect("parse case-sensitive casing config");
    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("load both case-sensitive files");

    assert_eq!(
        prepared
            .source_files()
            .iter()
            .map(|source| source.path().display())
            .collect::<Vec<_>>(),
        [
            Path::new("/project/Value.ts"),
            Path::new("/project/value.ts")
        ]
    );
    let [diagnostic] = prepared.diagnostics().program() else {
        panic!("case-sensitive file-name fold collision publishes TS1149");
    };
    assert_eq!(diagnostic.code(), 1149);
    assert_eq!(diagnostic.message.next[0].next.len(), 2);
    assert!(diagnostic.message.next[0]
        .next
        .iter()
        .all(|reason| reason.code == 1409));
    assert!(diagnostic.related_information_present);
    assert_eq!(diagnostic.related.len(), 2);
    for (related, spec) in diagnostic
        .related
        .iter()
        .zip(["\"Value.ts\"", "\"value.ts\""])
    {
        assert_eq!(related.message.code, 1410);
        assert_eq!(related.file_name.as_deref(), Some("/project/tsconfig.json"));
        let byte = text.find(spec).expect("files entry span");
        assert_eq!(related.start, Some(byte as u32));
        assert_eq!(related.length, Some(spec.len() as u32));
    }
}

#[test]
#[ignore = "local H0 program oracle audit; requires the pinned Node runtime"]
fn missing_library_config_related_information_matches_vendored_typescript() {
    const PROBE: &str = r#"
const ts = require(process.argv[1]);
const configs = JSON.parse(process.argv[2]);
function probe(configText) {
  const configFile = ts.parseJsonText('/project/tsconfig.json', configText);
  const parseHost = {
    useCaseSensitiveFileNames: true,
    readDirectory: () => [],
    fileExists: path => path === '/project/main.ts',
    readFile: path => path === '/project/main.ts' ? 'export {};\n' : undefined,
  };
  const parsed = ts.parseJsonSourceFileConfigFileContent(configFile, parseHost, '/project');
  const host = ts.createCompilerHost(parsed.options);
  host.getCurrentDirectory = () => '/project';
  host.getDefaultLibLocation = () => '/vendor/typescript/lib';
  host.getDefaultLibFileName = () => '/vendor/typescript/lib/lib.d.ts';
  host.fileExists = path => path === '/project/main.ts';
  host.readFile = path => path === '/project/main.ts' ? 'export {};\n' : undefined;
  host.getSourceFile = (path, target) => path === '/project/main.ts'
    ? ts.createSourceFile(path, 'export {};\n', target, true)
    : undefined;
  const program = ts.createProgram({
    rootNames: parsed.fileNames,
    options: parsed.options,
    host,
  });
  const diagnostic = ts.getPreEmitDiagnostics(program).find(row => row.code === 6053);
  if (!diagnostic) throw new Error('missing TS6053');
  return {
    relatedInformationPresent: diagnostic.relatedInformation !== undefined,
    related: (diagnostic.relatedInformation || []).map(related => ({
      code: related.code,
      file: related.file && related.file.fileName,
      start: related.start,
      length: related.length,
      message: ts.flattenDiagnosticMessageText(related.messageText, '\n'),
    })),
  };
}
process.stdout.write(JSON.stringify(configs.map(probe)));
"#;
    let configs = [
        r#"{"note":"😀","compilerOptions":{"noEmit":true,"target":"es5","types":[]},"files":["main.ts"]}"#,
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"],"types":[]},"files":["main.ts"]}"#,
        r#"{"compilerOptions":{"noEmit":true,"target":"ES5","types":[]},"files":["main.ts"]}"#,
        r#"{"compilerOptions":{"noEmit":true,"target":"es2015","target":"es5","types":[]},"files":["main.ts"]}"#,
        r#"{"compilerOptions":{"noEmit":true,"target":"es5","target":"es5","types":[]},"files":["main.ts"]}"#,
        r#"{"compilerOptions":{"noEmit":true,"types":[]},"files":["main.ts"]}"#,
    ];
    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("vendor/typescript-6.0.3/lib/typescript.js");
    let output = Command::new("node")
        .arg("-e")
        .arg(PROBE)
        .arg(bundle)
        .arg(json!(configs).to_string())
        .output()
        .expect("run vendored TypeScript program provenance probe");
    assert!(
        output.status.success(),
        "TypeScript probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let oracle: Value = serde_json::from_slice(&output.stdout).expect("probe output is JSON");

    let rust = configs
        .iter()
        .map(|text| {
            let host = host();
            let plan = parse_config_root_plan(&ConfigHostAdapter::new(&host), request(text))
                .expect("parse oracle config");
            let prepared = load_config_program(
                &host,
                &plan,
                &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
                LIMITS,
            )
            .expect("load oracle config");
            let diagnostic = prepared
                .diagnostics()
                .program()
                .iter()
                .find(|diagnostic| diagnostic.code() == 6053)
                .expect("Rust program publishes TS6053");
            json!({
                "relatedInformationPresent": diagnostic.related_information_present,
                "related": diagnostic.related.iter().map(|related| json!({
                    "code": related.message.code,
                    "file": related.file_name,
                    "start": related.start,
                    "length": related.length,
                    "message": related.message.text,
                })).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(json!(rust), oracle);
}

#[test]
fn config_plan_projects_checker_options_into_the_prepared_program() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"target":"es2015","module":"commonjs","strict":true,"noImplicitReturns":true,"jsx":"preserve","forceConsistentCasingInFileNames":false},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse option projection plan");
    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("load projected options");

    assert_eq!(prepared.compiler_options().target, Some(2));
    assert_eq!(prepared.compiler_options().module, Some(1));
    assert_eq!(prepared.compiler_options().strict, Some(true));
    assert_eq!(prepared.compiler_options().no_implicit_returns, Some(true));
    assert_eq!(prepared.compiler_options().jsx, Some(1));
    assert_eq!(
        prepared
            .compiler_options()
            .force_consistent_casing_in_file_names,
        Some(false)
    );
}

#[test]
fn config_plan_retains_h1_printer_options_without_broadening_h0_loader() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"newLine":"crlf","removeComments":true,"noImplicitUseStrict":false,"noEmitHelpers":true},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse H1 printer option projection plan");

    assert_eq!(plan.compiler_options().new_line, Some(0));
    assert_eq!(plan.compiler_options().remove_comments, Some(true));
    assert_eq!(plan.compiler_options().no_implicit_use_strict, Some(false));
    assert_eq!(plan.compiler_options().no_emit_helpers, Some(true));

    let error = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect_err("the H0 loader remains closed over its no-emit option profile");
    assert!(error.to_string().contains("unsupported-config-option"));
}

#[test]
fn config_plan_projects_no_resolve_and_keeps_dependencies_out_of_the_program() {
    let host = MemoryCompilerHost::builder("/project")
        .file(
            "/project/main.ts",
            concat!(
                "/// <reference path=\"./path.ts\" />\n",
                "/// <reference types=\"pkg\" />\n",
                "import './dependency';\n",
            )
            .as_bytes()
            .to_vec(),
        )
        .file("/project/path.ts", b"export {};".to_vec())
        .file("/project/dependency.ts", b"export {};".to_vec())
        .file(
            "/project/node_modules/@types/pkg/index.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build noResolve config host");
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"noResolve":true,"module":"commonjs","moduleResolution":"node"},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse noResolve config plan");

    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("load noResolve config program");
    assert_eq!(prepared.compiler_options().no_resolve, Some(true));
    assert_eq!(prepared.source_files().len(), 1);
    assert_eq!(prepared.resolutions().module_len(), 1);
    assert_eq!(prepared.resolutions().type_reference_len(), 0);
}

#[test]
fn filesystem_and_memory_config_programs_are_identical() {
    let tree = TempTree::new();
    let source = b"const value: number = 1;\n";
    fs::write(tree.path("main.ts"), source).expect("write filesystem source");

    let filesystem = FsCompilerHost::new(&tree.root, true).expect("filesystem compiler host");
    let memory = MemoryCompilerHost::builder(&tree.root)
        .case_sensitive(true)
        .file(tree.path("main.ts"), source.to_vec())
        .build()
        .expect("memory compiler host");
    let config_text = r#"{"compilerOptions":{"noEmit":true,"noLib":true},"files":["main.ts"]}"#;
    let filesystem_adapter = ConfigHostAdapter::new(&filesystem);
    let memory_adapter = ConfigHostAdapter::new(&memory);
    let filesystem_plan =
        parse_config_root_plan(&filesystem_adapter, request_at(&tree.root, config_text))
            .expect("filesystem config plan");
    let memory_plan = parse_config_root_plan(&memory_adapter, request_at(&tree.root, config_text))
        .expect("memory config plan");
    assert_eq!(filesystem_plan, memory_plan);

    let catalog = LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib");
    let filesystem_program = load_config_program(&filesystem, &filesystem_plan, &catalog, LIMITS)
        .expect("filesystem config program");
    let memory_program = load_config_program(&memory, &memory_plan, &catalog, LIMITS)
        .expect("memory config program");
    assert_eq!(filesystem_program, memory_program);
}

#[test]
fn shared_compiler_host_config_adapter_keeps_include_exclude_equivalent() {
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("src/generated")).expect("create config directories");
    fs::write(tree.path("src/main.ts"), "const main = 1;\n").expect("write main source");
    fs::write(
        tree.path("src/generated/ignored.ts"),
        "const ignored = 1;\n",
    )
    .expect("write generated source");
    fs::write(tree.path("src/readme.txt"), "ignored\n").expect("write text source");

    let filesystem = FsCompilerHost::new(&tree.root, true).expect("filesystem compiler host");
    let memory = MemoryCompilerHost::builder(&tree.root)
        .case_sensitive(true)
        .file(tree.path("src/main.ts"), b"const main = 1;\n".to_vec())
        .file(
            tree.path("src/generated/ignored.ts"),
            b"const ignored = 1;\n".to_vec(),
        )
        .file(tree.path("src/readme.txt"), b"ignored\n".to_vec())
        .build()
        .expect("memory compiler host");
    let config_text = r#"{"compilerOptions":{"noEmit":true,"noLib":true},"include":["src/**/*.ts"],"exclude":["src/generated"]}"#;

    let filesystem_plan = parse_config_root_plan(
        &CompilerConfigHost::new(&filesystem),
        request_at(&tree.root, config_text),
    )
    .expect("filesystem include/exclude plan");
    let memory_plan = parse_config_root_plan(
        &CompilerConfigHost::new(&memory),
        request_at(&tree.root, config_text),
    )
    .expect("memory include/exclude plan");
    assert_eq!(filesystem_plan, memory_plan);
    let main_name = tree.path("src/main.ts").to_string_lossy().into_owned();
    assert_eq!(
        filesystem_plan.file_names(),
        std::slice::from_ref(&main_name)
    );

    let catalog = LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib");
    let filesystem_program = load_config_program(&filesystem, &filesystem_plan, &catalog, LIMITS)
        .expect("filesystem include/exclude program");
    let memory_program = load_config_program(&memory, &memory_plan, &catalog, LIMITS)
        .expect("memory include/exclude program");
    assert_eq!(filesystem_program, memory_program);
}

#[test]
fn compiler_config_host_prunes_implicit_packages_but_honors_explicit_package_includes() {
    let host = MemoryCompilerHost::builder("/project")
        .file("/project/main.ts", b"const main = 1;\n".to_vec())
        .file(
            "/project/node_modules/pkg/index.ts",
            b"export const packageValue = 1;\n".to_vec(),
        )
        .build()
        .expect("package include memory host");

    let all_files = CompilerConfigHost::new(&host)
        .read_directory("/project", &[".ts"], None, None, None)
        .expect("unfiltered recursive directory listing");
    assert_eq!(
        all_files,
        vec![
            "/project/main.ts".to_owned(),
            "/project/node_modules/pkg/index.ts".to_owned()
        ]
    );

    let implicit = parse_config_root_plan(
        &CompilerConfigHost::new(&host),
        request(r#"{"compilerOptions":{"noEmit":true,"noLib":true},"include":["**/*.ts"]}"#),
    )
    .expect("implicit package exclusion plan");
    assert_eq!(implicit.file_names(), &["/project/main.ts".to_owned()]);

    let explicit = parse_config_root_plan(
        &CompilerConfigHost::new(&host),
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true},"include":["node_modules/**/*.ts"]}"#,
        ),
    )
    .expect("explicit package include plan");
    assert_eq!(
        explicit.file_names(),
        &["/project/node_modules/pkg/index.ts".to_owned()]
    );
}

#[test]
fn compiler_config_host_flattens_multiple_includes_in_written_order() {
    let host = MemoryCompilerHost::builder("/project")
        .file("/project/a/z.ts", b"export const a = 1;\n".to_vec())
        .file("/project/b/a.ts", b"export const b = 1;\n".to_vec())
        .build()
        .expect("multiple include memory host");
    let files = CompilerConfigHost::new(&host)
        .read_directory(
            "/project",
            &[".ts"],
            None,
            Some(&["b/**/*.ts".to_owned(), "a/**/*.ts".to_owned()]),
            None,
        )
        .expect("multiple include directory listing");
    assert_eq!(
        files,
        vec!["/project/b/a.ts".to_owned(), "/project/a/z.ts".to_owned()]
    );
}

#[test]
fn compiler_config_host_deduplicates_realpath_directory_cycles() {
    let host = MemoryCompilerHost::builder("/project")
        .file("/project/main.ts", b"const main = 1;\n".to_vec())
        .file("/project/link/nested.ts", b"const nested = 1;\n".to_vec())
        .realpath("/project/link", "/project")
        .build()
        .expect("realpath-cycle memory host");
    let files = CompilerConfigHost::new(&host)
        .read_directory(
            "/project",
            &[".ts"],
            None,
            Some(&["**/*.ts".to_owned()]),
            None,
        )
        .expect("realpath-cycle directory listing");
    assert_eq!(files, vec!["/project/main.ts".to_owned()]);
}

#[test]
fn config_loader_rejects_omitted_or_false_no_emit_before_host_loading() {
    for value in ["false", "null"] {
        let host = host();
        let adapter = ConfigHostAdapter::new(&host);
        let plan = parse_config_root_plan(
            &adapter,
            request(&format!(
                r#"{{"compilerOptions":{{"noEmit":{value},"noLib":true}},"files":["main.ts"]}}"#
            )),
        )
        .expect("parse noEmit plan");
        let error = load_config_program(
            &host,
            &plan,
            &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
            LIMITS,
        )
        .expect_err("non-true noEmit must fail closed");
        assert!(matches!(
            error,
            ConfigProgramLoadError::NoEmitRequired { .. }
        ));
    }
}

#[test]
fn command_line_no_emit_override_wins_over_a_false_config_value() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(r#"{"compilerOptions":{"noEmit":false,"noLib":true},"files":["main.ts"]}"#),
    )
    .expect("parse false noEmit plan");
    let prepared = load_config_program_with_no_emit_override(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("command-line noEmit override");
    assert_eq!(prepared.compiler_options().no_emit, Some(true));
}

#[test]
fn emitting_config_loader_is_distinct_and_rejects_effective_no_emit() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let catalog = LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib");
    let emit_plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"target":"esnext","module":"preserve","noLib":true},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse emitting plan");
    let prepared = load_emitting_config_program(&host, &emit_plan, &catalog, LIMITS)
        .expect("load emitting config");
    assert_eq!(prepared.mode(), PreparedProgramMode::Emit);
    assert_eq!(prepared.compiler_options().no_emit, None);

    let no_emit_plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"target":"esnext","module":"preserve","noLib":true},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse no-emit plan");
    assert!(matches!(
        load_emitting_config_program(&host, &no_emit_plan, &catalog, LIMITS),
        Err(ConfigProgramLoadError::EmitRequired { value: Some(true) })
    ));

    let overridden =
        load_emitting_config_program_with_no_emit_override(&host, &no_emit_plan, &catalog, LIMITS)
            .expect("explicit false override selects emitting mode");
    assert_eq!(overridden.mode(), PreparedProgramMode::Emit);
    assert_eq!(overridden.compiler_options().no_emit, Some(false));
}

#[test]
fn emitting_config_loader_applies_typed_command_line_precedence() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"target":"es2025","module":"esnext","lib":["es5"]},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse overridden emitting plan");
    let prepared = load_emitting_config_program_with_overrides(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
        ConfigEmitOptionOverrides {
            target: Some(99),
            module: Some(200),
            emit_bom: Some(true),
            new_line: Some(0),
            list_emitted_files: Some(true),
            ..ConfigEmitOptionOverrides::default()
        },
    )
    .expect("load config with command-line emit overrides");
    let options = prepared.compiler_options();
    assert_eq!(options.target, Some(99));
    assert_eq!(options.module, Some(200));
    assert_eq!(options.emit_bom, Some(true));
    assert_eq!(options.new_line, Some(0));
    assert_eq!(options.list_emitted_files, Some(true));
}

#[test]
fn config_diagnostics_are_a_gate_and_remain_separate_from_option_diagnostics() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"notAnOption":true},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse diagnostic plan");
    let error = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect_err("config diagnostic must stop program construction");
    let ConfigProgramLoadError::Diagnostics { config, options } = error else {
        panic!("expected separated config/option diagnostics");
    };
    assert_eq!(config.len(), plan.diagnostics().count());
    assert_eq!(options.len(), plan.option_diagnostics().len());
    assert!(options.is_empty());
    assert_eq!(config[0].code(), 5023);
}

#[test]
fn ts6_option_deprecations_are_reported_without_blocking_no_emit_loading() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"moduleResolution":"node"},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse deprecated-option plan");
    assert_eq!(
        plan.option_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5107]
    );
    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("a deprecation diagnostic must not prevent source loading");
    assert_eq!(prepared.compiler_options().ignore_deprecations, None);

    let silenced = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"moduleResolution":"node","ignoreDeprecations":"6.0"},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse silenced deprecated-option plan");
    assert!(silenced.option_diagnostics().is_empty());
    assert_eq!(
        silenced.compiler_options().ignore_deprecations.as_deref(),
        Some("6.0")
    );

    let invalid = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"ignoreDeprecations":"5.1"},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse invalid ignoreDeprecations plan");
    assert!(invalid
        .option_diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code() == 5103));

    let removed = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"target":"ES3","ignoreDeprecations":"5.0"},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse removed-target plan");
    assert_eq!(
        removed
            .option_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5108]
    );
    let error = load_config_program(
        &host,
        &removed,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect_err("removed compiler options remain a fatal getOptionsDiagnostics row");
    assert_eq!(error.options_diagnostics()[0].code(), 5108);

    let removed_with_current_suppression = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"target":"ES3","ignoreDeprecations":"6.0"},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse removed-target plan with current suppression");
    assert_eq!(
        removed_with_current_suppression
            .option_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5108]
    );
}

#[test]
fn root_config_retains_option_syntax_for_effective_program_diagnostics() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let text = r#"{
        "compilerOptions": {
            "module": "amd",
            "target": "ES3",
            "noImplicitUseStrict": true
        },
        "files": ["main.ts"]
    }"#;
    let plan = parse_config_root_plan(&adapter, request(text)).expect("parse config provenance");
    let options = plan.program_options();
    assert!(options.external_config_option_diagnostics());
    let config = options.config_file().expect("retained root config");

    let span = |needle: &str| {
        let start = u32::try_from(text.find(needle).expect("syntax needle")).expect("UTF-16 start");
        (start, u32::try_from(needle.len()).expect("UTF-16 length"))
    };
    let module_name = span("\"module\"");
    let module_value = span("\"amd\"");
    let removed_name = span("\"noImplicitUseStrict\"");
    assert_eq!(
        config.compiler_option_name_locations("module"),
        [tsc_program::ProgramConfigSpan::new(
            module_name.0,
            module_name.1
        )]
    );
    assert_eq!(
        config.compiler_option_value_locations("module"),
        [tsc_program::ProgramConfigSpan::new(
            module_value.0,
            module_value.1
        )]
    );
    assert_eq!(
        config.compiler_option_name_locations("noImplicitUseStrict"),
        [tsc_program::ProgramConfigSpan::new(
            removed_name.0,
            removed_name.1
        )]
    );
}

#[test]
fn ts6_module_option_relationship_diagnostics_match_the_effective_kinds() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let codes = |compiler_options: &str| {
        let text = format!(
            r#"{{"compilerOptions":{{"noEmit":true,"noLib":true,{compiler_options}}},"files":["main.ts"]}}"#
        );
        let plan = parse_config_root_plan(&adapter, request(&text))
            .expect("parse module option relationship plan");
        let mut codes = plan
            .option_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>();
        codes.sort_unstable();
        codes
    };

    assert_eq!(
        codes(r#""module":"node16","moduleResolution":"node10""#),
        [5107, 5109]
    );
    assert_eq!(codes(r#""moduleResolution":"node16""#), [5110]);
    assert_eq!(
        codes(r#""module":"amd","moduleResolution":"bundler""#),
        [5095, 5107]
    );
    assert_eq!(
        codes(r#""resolvePackageJsonExports":true,"moduleResolution":"classic""#),
        [5098, 5107]
    );
    assert_eq!(
        codes(r#""customConditions":[],"moduleResolution":"classic""#),
        [5098, 5107]
    );
    assert_eq!(
        codes(r#""verbatimModuleSyntax":true,"module":"amd""#),
        [5105, 5107]
    );
}

#[test]
fn compiler_option_relationship_diagnostics_use_tsc_config_spans() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let cases = [
        (
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"strict":false,"exactOptionalPropertyTypes":true},"files":["main.ts"]}"#,
            "\"exactOptionalPropertyTypes\"",
            5052,
            "Option 'exactOptionalPropertyTypes' cannot be specified without specifying option 'strictNullChecks'.",
        ),
        (
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"jsxFactory":"Element.createElement="},"files":["main.ts"]}"#,
            "\"Element.createElement=\"",
            5067,
            "Invalid value for 'jsxFactory'. 'Element.createElement=' is not a valid identifier or qualified-name.",
        ),
    ];

    for (text, located_text, expected_code, expected_message) in cases {
        let plan = parse_config_root_plan(&adapter, request(text))
            .expect("parse compiler option relationship plan");
        let [diagnostic] = plan.option_diagnostics() else {
            panic!(
                "expected one option diagnostic for {located_text}, got {:#?}",
                plan.option_diagnostics()
            );
        };
        let start = u32::try_from(text.find(located_text).expect("located option syntax"))
            .expect("config offset fits u32");

        assert_eq!(diagnostic.code(), expected_code);
        assert_eq!(diagnostic.message_text(), expected_message);
        assert_eq!(
            diagnostic.file_name.as_deref(),
            Some("/project/tsconfig.json")
        );
        assert_eq!(diagnostic.start, Some(start));
        assert_eq!(diagnostic.length, Some(located_text.len() as u32));
    }

    let text = r#"{"compilerOptions":{"noEmit":true,"noLib":true,"strictNullChecks":false,"exactOptionalPropertyTypes":true},"files":["main.ts"]}"#;
    let plan = parse_config_root_plan(&adapter, request(text))
        .expect("parse two-location compiler option relationship plan");
    let diagnostics = plan.option_diagnostics();
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5052, 5052]
    );
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.start)
            .collect::<Vec<_>>(),
        [
            Some(text.find("\"strictNullChecks\"").unwrap() as u32),
            Some(text.find("\"exactOptionalPropertyTypes\"").unwrap() as u32),
        ]
    );
}

#[test]
fn allow_importing_ts_extensions_requires_no_emit_unless_overridden() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noLib":true,"allowImportingTsExtensions":true},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse allowImportingTsExtensions plan");
    assert_eq!(
        plan.option_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5096]
    );
    let error = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect_err("allowImportingTsExtensions needs a noEmit setting");
    assert_eq!(error.options_diagnostics()[0].code(), 5096);

    let prepared = load_config_program_with_no_emit_override(
        &host,
        &plan,
        &LibraryCatalog::typescript_6_0_3("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("the command-line noEmit override satisfies TS5096");
    assert_eq!(prepared.compiler_options().no_emit, Some(true));
}
