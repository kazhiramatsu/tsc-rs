use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tsc_host::{CompilerHost, FsCompilerHost, MemoryCompilerHost};
use tsc_program::{
    decode_host_text, load_config_program, load_config_program_with_no_emit_override,
    parse_config_root_plan, CompilerConfigHost, ConfigHostError, ConfigHostOperation,
    ConfigParseHost, ConfigProgramLoadError, ConfigRootPlanRequest, LibraryCatalog,
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
