use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tsc_host::{CompilerHost, FsCompilerHost, MemoryCompilerHost};
use tsc_program::{
    command_line_option_bag, decode_host_text, load_config_program,
    load_config_program_with_no_emit_override, load_emitting_config_program,
    load_emitting_config_program_with_no_emit_override, parse_config_root_plan,
    parse_config_root_plan_with_command_line, validate_config_plan, CompilerConfigHost,
    ConfigExtendedCache, ConfigHostError, ConfigHostOperation, ConfigParseHost,
    ConfigProgramLoadError, ConfigRootPlanRequest, JsonValue, LibraryCatalog, PreparedProgramMode,
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

    // A config with `references` loads (the referenced projects are
    // resolved, see project_references_contract); a reference that names no
    // config is TS6053 among the program diagnostics, as tsgo's
    // verifyProjectReferences reports it.
    let references = parse_config_root_plan(
        &adapter,
        ConfigRootPlanRequest {
            file_name: "/work/tsconfig.json".to_owned().into(),
            text: r#"{"files":["main.ts"],"references":[{"path":"other"}]}"#.to_owned(),
            base_path: "/work".to_owned().into(),
        },
    )
    .expect("project-reference config remains observable as a partial plan");
    let prepared = load_config_program_with_no_emit_override(
        &host,
        &references,
        &LibraryCatalog::typescript_7_1(PathBuf::from("/work/lib")),
        LIMITS,
    )
    .expect("a config with project references loads");
    assert!(
        prepared
            .diagnostics()
            .program()
            .iter()
            .any(|diagnostic| diagnostic.code() == 6053),
        "{:?}",
        prepared.diagnostics().program()
    );

    let emit = parse_config_root_plan(
        &adapter,
        ConfigRootPlanRequest {
            file_name: "/work/tsconfig.json".to_owned().into(),
            text: r#"{"files":["main.ts"],"compilerOptions":{"declaration":true}}"#.to_owned(),
            base_path: "/work".to_owned().into(),
        },
    )
    .expect("declaration config remains observable as a partial plan");
    // tsc reports a --noEmit command's declaration diagnostics when
    // getEmitDeclarations(options) holds, so the no-emit loader admits the
    // declaration-product options (composite, declaration, isolatedDeclarations...).
    let prepared = load_config_program_with_no_emit_override(
        &host,
        &emit,
        &LibraryCatalog::typescript_7_1(PathBuf::from("/work/lib")),
        LIMITS,
    )
    .expect("declaration output is admitted for a no-emit check");
    assert_eq!(prepared.compiler_options().declaration, Some(true));
    assert_eq!(prepared.compiler_options().no_emit, Some(true));
}

#[test]
fn incremental_is_admitted_for_a_no_emit_check() {
    // tsgo checks an incremental project like any other (the build info it
    // writes is an emit product); the no-emit loader keeps the option on the
    // program instead of refusing the config.
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"incremental":true,"tsBuildInfoFile":"cache/main.tsbuildinfo"},"files":["main.ts"]}"#,
        ),
    )
    .expect("incremental is a recognized partial-plan option");

    let prepared = load_config_program_with_no_emit_override(
        &host,
        &plan,
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("an incremental project is checked");
    assert_eq!(prepared.compiler_options().incremental, Some(true));
    assert_eq!(
        prepared
            .compiler_options()
            .ts_build_info_file
            .as_ref()
            .map(|file| file.as_js().to_owned()),
        Some("/project/cache/main.tsbuildinfo".into())
    );
}

#[test]
fn inherited_root_scopes_respect_acquisition_noninheritance() {
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
        let loaded = load_config_program_with_no_emit_override(
            &host,
            &plan,
            &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
            LIMITS,
        );
        if scope == "typeAcquisition" {
            assert!(
                loaded.is_ok(),
                "typeAcquisition does not inherit into this config"
            );
            assert_eq!(plan.unsupported_root_scopes().next(), None);
            continue;
        }
        // An inherited watchOptions and a truthy compileOnSave reach the
        // child's config, and a command that neither watches nor serves an
        // editor never reads them: the plan observes the scope and the
        // no-emit loader admits it.
        assert!(loaded.is_ok(), "{scope} is inert for a tsc command");
        assert_eq!(plan.unsupported_root_scopes().collect::<Vec<_>>(), [scope]);
    }
}

#[test]
fn compile_on_save_is_admitted_for_a_no_emit_check() {
    // tsc parses compileOnSave into ParsedCommandLine.compileOnSave
    // (convertCompileOnSaveOptionFromJson) for editors and executeCommandLine
    // never reads it, so a --noEmit check of a project that sets it
    // (Playwright's root tsconfig) loads and reports exactly what tsc reports.
    let host = host();
    let plan = parse_config_root_plan(
        &ConfigHostAdapter::new(&host),
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true},"files":["main.ts"],"compileOnSave":true}"#,
        ),
    )
    .expect("compileOnSave remains a partial plan");
    assert_eq!(
        plan.unsupported_root_scopes().collect::<Vec<_>>(),
        ["compileOnSave"]
    );
    let prepared = load_config_program_with_no_emit_override(
        &host,
        &plan,
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("compileOnSave is inert for a no-emit check");
    assert_eq!(prepared.compiler_options().no_emit, Some(true));
}

#[test]
fn language_service_plugins_are_admitted_for_every_command() {
    // tsc declares `plugins` under Editor Support ("A list of plugins to load
    // in the language service") and no code path outside the service reads
    // options.plugins, so a --noEmit check or an emit of a project that lists
    // service plugins (VS Code's tsec, Effect's language service) loads and
    // reports exactly what tsc reports.
    let host = host();
    let catalog = LibraryCatalog::typescript_7_1("/vendor/typescript/lib");
    let plan = parse_config_root_plan(
        &ConfigHostAdapter::new(&host),
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"plugins":[{"name":"tsec","exemptionConfig":"./tsec.exemptions.json"}]},"files":["main.ts"]}"#,
        ),
    )
    .expect("plugins remain a partial plan");
    let prepared = load_config_program_with_no_emit_override(&host, &plan, &catalog, LIMITS)
        .expect("language service plugins are inert for a no-emit check");
    assert_eq!(prepared.compiler_options().no_emit, Some(true));

    let emitting = parse_config_root_plan(
        &ConfigHostAdapter::new(&host),
        request(
            r#"{"compilerOptions":{"noLib":true,"plugins":[{"name":"tsec"}]},"files":["main.ts"]}"#,
        ),
    )
    .expect("plugins remain a partial plan");
    load_emitting_config_program(&host, &emitting, &catalog, LIMITS)
        .expect("language service plugins are inert for an emitting command");
}

#[test]
fn emit_decorator_metadata_is_admitted_for_a_no_emit_check() {
    // emitDecoratorMetadata selects checker behaviour the checker implements
    // (decorator metadata type references count as referenced), so a
    // --noEmit check of a project that sets it (zod's base tsconfig) loads
    // and reports exactly what tsc reports, including its
    // experimentalDecorators requirement.
    let host = host();
    let catalog = LibraryCatalog::typescript_7_1("/vendor/typescript/lib");
    let plan = parse_config_root_plan(
        &ConfigHostAdapter::new(&host),
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"experimentalDecorators":true,"emitDecoratorMetadata":true},"files":["main.ts"]}"#,
        ),
    )
    .expect("emitDecoratorMetadata remains a partial plan");
    let prepared = load_config_program_with_no_emit_override(&host, &plan, &catalog, LIMITS)
        .expect("emitDecoratorMetadata is admitted for a no-emit check");
    assert_eq!(prepared.compiler_options().no_emit, Some(true));
    assert_eq!(
        prepared.compiler_options().emit_decorator_metadata,
        Some(true)
    );
}

#[test]
fn erasable_syntax_only_is_admitted_for_a_no_emit_check() {
    // erasableSyntaxOnly selects only the checker's TS1294 rows, which the
    // checker implements, so a --noEmit check of a project that sets it
    // (Effect's base tsconfig) loads and reports exactly what tsc reports.
    let host = host();
    let catalog = LibraryCatalog::typescript_7_1("/vendor/typescript/lib");
    let plan = parse_config_root_plan(
        &ConfigHostAdapter::new(&host),
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"erasableSyntaxOnly":true},"files":["main.ts"]}"#,
        ),
    )
    .expect("erasableSyntaxOnly remains a partial plan");
    let prepared = load_config_program_with_no_emit_override(&host, &plan, &catalog, LIMITS)
        .expect("erasableSyntaxOnly is admitted for a no-emit check");
    assert_eq!(prepared.compiler_options().no_emit, Some(true));
    assert_eq!(prepared.compiler_options().erasable_syntax_only, Some(true));
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

    fn file_exists(&self, path: tsc_diagnostics::JsStr<'_>) -> Result<bool, ConfigHostError> {
        let path = path.as_str().expect("scalar config fixture query");

        self.host.file_exists(Path::new(path)).map_err(|error| {
            Self::host_error(ConfigHostOperation::FileExists, path, &error.to_string())
        })
    }

    fn read_file(
        &self,
        path: tsc_diagnostics::JsStr<'_>,
    ) -> Result<Option<String>, ConfigHostError> {
        let path = path.as_str().expect("scalar config fixture query");

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
        directory: tsc_diagnostics::JsStr<'_>,
        _extensions: &[&str],
        _excludes: Option<&[tsc_diagnostics::JsString]>,
        _includes: Option<&[tsc_diagnostics::JsString]>,
        _depth: Option<usize>,
    ) -> Result<Vec<tsc_diagnostics::JsString>, ConfigHostError> {
        let directory = directory.as_str().expect("scalar config fixture directory");
        let scalar_paths: Result<Vec<String>, ConfigHostError> = {
            Err(Self::host_error(
                ConfigHostOperation::ReadDirectory,
                directory,
                "the files-only contract does not enumerate directories",
            ))
        };
        scalar_paths.map(|paths| paths.into_iter().map(Into::into).collect())
    }
}

fn request(text: &str) -> ConfigRootPlanRequest {
    request_at(Path::new("/project"), text)
}

fn request_at(base: &Path, text: &str) -> ConfigRootPlanRequest {
    let base = base.to_str().expect("test path is Unicode");
    ConfigRootPlanRequest {
        file_name: format!("{base}/tsconfig.json").into(),
        text: text.to_owned(),
        base_path: base.to_owned().into(),
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
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("load config program");
    assert_eq!(prepared.roots().len(), 1);
    assert_eq!(
        prepared.roots()[0].path().display().scalar_test_path(),
        "/project/main.ts"
    );
    assert_eq!(prepared.compiler_options().no_emit, Some(true));
    assert_eq!(prepared.program_options().no_lib(), Some(true));
}

#[test]
fn conflicting_lib_and_no_lib_report_at_the_first_of_the_two_names() {
    // TypeScript 7.1 reports the row once, at whichever of `lib`/`noLib`
    // comes first in the config (tsc 6.0 reported it at both).
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
        vec![(5053, Some(34), Some(7))]
    );
    assert_eq!(
        plan.option_diagnostics()[0]
            .message_text()
            .as_str()
            .expect("scalar diagnostic observation"),
        "Option 'lib' cannot be specified with option 'noLib'."
    );
}

#[test]
fn composite_project_reports_an_unlisted_file_beside_its_root_dir_violation() {
    // tsc verifyCompilerOptions: a composite project lists every file it
    // would emit (TS6307), reported at the import that pulled the file in
    // like the rootDir violation (TS6059) that precedes it.
    let host = MemoryCompilerHost::builder("/project")
        .file(
            "/project/src/a.ts",
            b"import { b } from \"../other/b\";\nexport const a: number = b;\n".to_vec(),
        )
        .file("/project/other/b.ts", b"export const b = 1;\n".to_vec())
        .build()
        .expect("memory compiler host");
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"composite":true,"rootDir":"src"},"files":["src/a.ts"]}"#,
        ),
    )
    .expect("parse composite plan");
    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("a --noEmit check of a composite project loads");
    let diagnostics = prepared.diagnostics().program();
    let codes = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect::<Vec<_>>();
    assert_eq!(codes, [6059, 6307]);
    let import_specifier = "import { b } from ".len() as u32;
    for diagnostic in diagnostics {
        assert_eq!(
            diagnostic.file_name.as_ref().and_then(|name| name.as_str()),
            Some("/project/src/a.ts")
        );
        assert_eq!(diagnostic.start, Some(import_specifier));
    }
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
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
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
    assert_eq!(
        related
            .file_name
            .as_ref()
            .map(|value| value.as_str().expect("scalar legacy option observation")),
        Some("/project/tsconfig.json")
    );
    let literal_byte = text.find("\"missing\"").expect("types literal span");
    let literal_utf16 = text[..literal_byte].encode_utf16().count() as u32;
    assert_eq!(related.start, Some(literal_utf16));
    assert_eq!(related.length, Some("\"missing\"".len() as u32));
    let auxiliary = prepared.auxiliary_files().collect::<Vec<_>>();
    assert_eq!(auxiliary.len(), 1);
    assert_eq!(
        auxiliary[0].path().display().scalar_test_path(),
        "/project/tsconfig.json"
    );
    assert_eq!(auxiliary[0].text(), text);
}

#[test]
fn missing_default_library_has_no_target_related_information_like_tsgo() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let text = r#"{"note":"😀","compilerOptions":{"noEmit":true,"target":"es5","types":[]},"files":["main.ts"]}"#;
    let plan = parse_config_root_plan(&adapter, request(text)).expect("parse target config");
    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
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
        "Default library for target 'ES5'"
    );
    // tsgo's lib-file related information matches array elements only, so a
    // string `target` points nowhere.
    assert!(!diagnostic.related_information_present);
    assert!(diagnostic.related.is_empty());
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
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
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
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
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
    assert_eq!(
        related
            .file_name
            .as_ref()
            .map(|value| value.as_str().expect("scalar legacy option observation")),
        Some("/project/tsconfig.json")
    );
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
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
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
    assert_eq!(
        related
            .file_name
            .as_ref()
            .map(|value| value.as_str().expect("scalar legacy option observation")),
        Some("/project/tsconfig.json")
    );
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
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
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
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("load both case-sensitive files");

    assert_eq!(
        prepared
            .source_files()
            .iter()
            .map(|source| source.path().display().scalar_test_path())
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
        assert_eq!(
            related
                .file_name
                .as_ref()
                .map(|value| value.as_str().expect("scalar legacy option observation")),
            Some("/project/tsconfig.json")
        );
        let byte = text.find(spec).expect("files entry span");
        assert_eq!(related.start, Some(byte as u32));
        assert_eq!(related.length, Some(spec.len() as u32));
    }
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
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
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
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"newLine":"crlf","removeComments":true,"noEmitHelpers":true},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse H1 printer option projection plan");

    assert_eq!(plan.compiler_options().new_line, Some(0));
    assert_eq!(plan.compiler_options().remove_comments, Some(true));
    assert_eq!(plan.compiler_options().no_emit_helpers, Some(true));

    // Emitter-only options change no diagnostic of a no-emit check, so the
    // loader retains them without constructing an emitter (as tsc ignores
    // them under --noEmit).
    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("the no-emit loader retains emitter-only options");
    assert_eq!(prepared.compiler_options().new_line, Some(0));
    assert_eq!(prepared.compiler_options().remove_comments, Some(true));
    assert_eq!(prepared.compiler_options().no_emit_helpers, Some(true));
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
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
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

    let catalog = LibraryCatalog::typescript_7_1("/vendor/typescript/lib");
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
        (filesystem_plan.file_names())
            .iter()
            .map(|name| name.as_str().expect("scalar name observation").to_owned())
            .collect::<Vec<_>>()
            .as_slice(),
        std::slice::from_ref(&main_name)
    );

    let catalog = LibraryCatalog::typescript_7_1("/vendor/typescript/lib");
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
        .read_directory("/project".into(), &[".ts"], None, None, None)
        .expect("unfiltered recursive directory listing");
    assert_eq!(
        (all_files)
            .iter()
            .map(|name| name.as_str().expect("scalar name observation").to_owned())
            .collect::<Vec<_>>()
            .as_slice(),
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
    assert_eq!(
        (implicit.file_names())
            .iter()
            .map(|name| name.as_str().expect("scalar name observation").to_owned())
            .collect::<Vec<_>>()
            .as_slice(),
        &["/project/main.ts".to_owned()]
    );

    let explicit = parse_config_root_plan(
        &CompilerConfigHost::new(&host),
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true},"include":["node_modules/**/*.ts"]}"#,
        ),
    )
    .expect("explicit package include plan");
    assert_eq!(
        (explicit.file_names())
            .iter()
            .map(|name| name.as_str().expect("scalar name observation").to_owned())
            .collect::<Vec<_>>()
            .as_slice(),
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
            "/project".into(),
            &[".ts"],
            None,
            Some(&["b/**/*.ts".to_owned().into(), "a/**/*.ts".to_owned().into()]),
            None,
        )
        .expect("multiple include directory listing");
    assert_eq!(
        (files)
            .iter()
            .map(|name| name.as_str().expect("scalar name observation").to_owned())
            .collect::<Vec<_>>()
            .as_slice(),
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
            "/project".into(),
            &[".ts"],
            None,
            Some(&["**/*.ts".to_owned().into()]),
            None,
        )
        .expect("realpath-cycle directory listing");
    assert_eq!(
        (files)
            .iter()
            .map(|name| name.as_str().expect("scalar name observation").to_owned())
            .collect::<Vec<_>>()
            .as_slice(),
        vec!["/project/main.ts".to_owned()]
    );
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
            &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
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
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("command-line noEmit override");
    assert_eq!(prepared.compiler_options().no_emit, Some(true));
}

#[test]
fn emitting_config_loader_is_distinct_and_rejects_effective_no_emit() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let catalog = LibraryCatalog::typescript_7_1("/vendor/typescript/lib");
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
    // tsgo merges the command line's options over the config's before the
    // program is created; the plan carries the merged options.
    let command_line = command_line_option_bag(
        &[
            ("target".to_owned(), JsonValue::String("esnext".into())),
            ("module".to_owned(), JsonValue::String("preserve".into())),
            ("emitBOM".to_owned(), JsonValue::Bool(true)),
            ("newLine".to_owned(), JsonValue::String("crlf".into())),
            ("listEmittedFiles".to_owned(), JsonValue::Bool(true)),
        ],
        "/project".into(),
    );
    let plan = parse_config_root_plan_with_command_line(
        &adapter,
        request(
            r#"{"compilerOptions":{"target":"es2025","module":"esnext","lib":["es5"]},"files":["main.ts"]}"#,
        ),
        &command_line,
        &mut ConfigExtendedCache::default(),
    )
    .expect("parse the plan with the command line's options");
    let prepared = load_emitting_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
        LIMITS,
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
fn config_diagnostics_remain_separate_and_do_not_stop_program_construction() {
    // tsgo creates the Program whatever the config reports: the config
    // parsing diagnostics stay apart from the option diagnostics
    // (GetConfigFileParsingDiagnostics, compiler/program.go:2010-2065). The
    // public validation gate still names both lists.
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"notAnOption":true},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse diagnostic plan");
    let error = validate_config_plan(&plan).expect_err("the validation gate reports TS5023");
    let ConfigProgramLoadError::Diagnostics { config, options } = error else {
        panic!("expected separated config/option diagnostics");
    };
    assert_eq!(config.len(), plan.diagnostics().count());
    assert_eq!(options.len(), plan.option_diagnostics().len());
    assert!(options.is_empty());
    assert_eq!(config[0].code(), 5023);

    let prepared = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("a config diagnostic does not stop program construction");
    let diagnostics = prepared.diagnostics();
    assert_eq!(
        diagnostics
            .config()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5023]
    );
    assert!(diagnostics.options().is_empty());
}

#[test]
fn removed_options_are_reported_without_blocking_no_emit_loading() {
    let host = host();
    let adapter = ConfigHostAdapter::new(&host);
    let catalog = LibraryCatalog::typescript_7_1("/vendor/typescript/lib");
    let plan = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"moduleResolution":"node"},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse removed-option plan");
    let [diagnostic] = plan.option_diagnostics() else {
        panic!(
            "expected one removed-option row: {:?}",
            plan.option_diagnostics()
        );
    };
    assert_eq!(diagnostic.code(), 5108);
    assert_eq!(
        diagnostic
            .message_text()
            .as_str()
            .expect("scalar diagnostic observation"),
        "Option 'moduleResolution=node10' has been removed. Please remove it from your configuration."
    );
    let prepared = load_config_program(&host, &plan, &catalog, LIMITS)
        .expect("a removed-option row must not prevent source loading");
    assert_eq!(prepared.compiler_options().ignore_deprecations, None);

    // TypeScript 7.1 parses `ignoreDeprecations` but neither validates it
    // nor lets it silence a row.
    for value in ["6.0", "5.0", "5.1"] {
        let plan = parse_config_root_plan(
            &adapter,
            request(&format!(
                r#"{{"compilerOptions":{{"noEmit":true,"noLib":true,"moduleResolution":"node","ignoreDeprecations":"{value}"}},"files":["main.ts"]}}"#
            )),
        )
        .expect("parse ignoreDeprecations plan");
        assert_eq!(
            plan.option_diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [5108],
            "ignoreDeprecations {value}"
        );
        assert_eq!(
            plan.compiler_options()
                .ignore_deprecations
                .as_ref()
                .map(|value| value.as_str().expect("scalar option observation")),
            Some(value)
        );
    }

    // `es3` is not a target value in TypeScript 7.1: a config error (TS6046
    // listing the values without the deprecated `es5`), not an option row.
    let removed = parse_config_root_plan(
        &adapter,
        request(
            r#"{"compilerOptions":{"noEmit":true,"noLib":true,"target":"ES3"},"files":["main.ts"]}"#,
        ),
    )
    .expect("parse es3 target plan");
    assert!(removed.option_diagnostics().is_empty());
    let [diagnostic] = removed.diagnostics().collect::<Vec<_>>()[..] else {
        panic!("expected one config diagnostic");
    };
    assert_eq!(diagnostic.code(), 6046);
    assert_eq!(
        diagnostic
            .message_text()
            .as_str()
            .expect("scalar diagnostic observation"),
        "Argument for '--target' option must be: 'es6', 'es2015', 'es2016', 'es2017', 'es2018', 'es2019', 'es2020', 'es2021', 'es2022', 'es2023', 'es2024', 'es2025', 'es2026', 'esnext'."
    );
    let prepared = load_config_program(&host, &removed, &catalog, LIMITS)
        .expect("an invalid option value does not stop program construction");
    let diagnostics = prepared.diagnostics();
    assert_eq!(
        diagnostics
            .config()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [6046]
    );
    assert!(diagnostics.options().is_empty());
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
fn module_option_relationship_diagnostics_match_the_effective_kinds() {
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

    // tsgo maps a removed classic/node10 value to the module's default
    // resolution (`GetModuleResolutionKind`), so neither TS5109 nor TS5098
    // follows the TS5108 row; TS5098 needs a resolution other than node16,
    // nodenext or bundler, which TypeScript 7.1 no longer has.
    assert_eq!(
        codes(r#""module":"node16","moduleResolution":"node10""#),
        [5108]
    );
    assert_eq!(codes(r#""moduleResolution":"node16""#), [5110]);
    assert_eq!(
        codes(r#""module":"amd","moduleResolution":"bundler""#),
        [5095, 5108]
    );
    assert_eq!(
        codes(r#""resolvePackageJsonExports":true,"moduleResolution":"classic""#),
        [5108]
    );
    assert_eq!(
        codes(r#""customConditions":[],"moduleResolution":"classic""#),
        [5108]
    );
    // TypeScript 7.1 has no verbatimModuleSyntax/amd row; the default
    // moduleResolution for amd is bundler, whose relationship row remains.
    assert_eq!(
        codes(r#""verbatimModuleSyntax":true,"module":"amd""#),
        [5095, 5108]
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
        assert_eq!(
            diagnostic
                .message_text()
                .as_str()
                .expect("scalar diagnostic observation"),
            expected_message
        );
        assert_eq!(
            diagnostic
                .file_name
                .as_ref()
                .map(|value| value.as_str().expect("scalar legacy option observation")),
            Some("/project/tsconfig.json")
        );
        assert_eq!(diagnostic.start, Some(start));
        assert_eq!(diagnostic.length, Some(located_text.len() as u32));
    }

    // TypeScript 7.1 reports a two-name row once, at the first of the two
    // properties in document order (tsoptions.ForEachPropertyAssignment).
    let text = r#"{"compilerOptions":{"noEmit":true,"noLib":true,"strictNullChecks":false,"exactOptionalPropertyTypes":true},"files":["main.ts"]}"#;
    let plan = parse_config_root_plan(&adapter, request(text))
        .expect("parse two-name compiler option relationship plan");
    let [diagnostic] = plan.option_diagnostics() else {
        panic!("expected one row: {:?}", plan.option_diagnostics());
    };
    assert_eq!(diagnostic.code(), 5052);
    assert_eq!(
        diagnostic.start,
        Some(text.find("\"strictNullChecks\"").unwrap() as u32)
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
    // TS5096 does not stop the Program (tsgo reports it among the Program
    // diagnostics); the no-emit route itself still needs a noEmit setting.
    let error = load_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect_err("the no-emit route needs a noEmit setting");
    assert!(
        matches!(
            error,
            ConfigProgramLoadError::NoEmitRequired { value: None }
        ),
        "{error:?}"
    );
    let prepared = load_emitting_config_program(
        &host,
        &plan,
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("TS5096 does not stop an emitting program");
    assert_eq!(prepared.compiler_options().no_emit, None);

    let prepared = load_config_program_with_no_emit_override(
        &host,
        &plan,
        &LibraryCatalog::typescript_7_1("/vendor/typescript/lib"),
        LIMITS,
    )
    .expect("the command-line noEmit override satisfies TS5096");
    assert_eq!(prepared.compiler_options().no_emit, Some(true));
}

use super::utf16_scalar_path::ScalarTestPath as _;
