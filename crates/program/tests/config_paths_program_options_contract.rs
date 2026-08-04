use std::collections::BTreeMap;
use std::path::Path;

use tsc_host::MemoryCompilerHost;
use tsc_program::{
    parse_config_root_plan, ConfigHostError, ConfigModuleResolutionOptions, ConfigParseHost,
    ConfigRootPlanRequest, ModuleResolver, ProgramOptions, ResolutionMode, ResolutionOutcome,
};

#[derive(Default)]
struct MemoryConfigHost {
    files: BTreeMap<String, String>,
    directory_files: Vec<String>,
}

impl MemoryConfigHost {
    fn with_file(mut self, path: &str, text: &str) -> Self {
        self.files.insert(path.to_owned(), text.to_owned());
        self
    }

    fn with_directory_files(mut self, files: &[&str]) -> Self {
        self.directory_files = files.iter().map(|file| (*file).to_owned()).collect();
        self
    }
}

impl ConfigParseHost for MemoryConfigHost {
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn file_exists(&self, path: &str) -> Result<bool, ConfigHostError> {
        Ok(self.files.contains_key(path))
    }

    fn read_file(&self, path: &str) -> Result<Option<String>, ConfigHostError> {
        Ok(self.files.get(path).cloned())
    }

    fn read_directory(
        &self,
        _directory: &str,
        extensions: &[&str],
        _excludes: Option<&[String]>,
        _includes: Option<&[String]>,
        _depth: Option<usize>,
    ) -> Result<Vec<String>, ConfigHostError> {
        Ok(self
            .directory_files
            .iter()
            .filter(|file| extensions.iter().any(|extension| file.ends_with(extension)))
            .cloned()
            .collect())
    }
}

fn request(file_name: &str, text: &str) -> ConfigRootPlanRequest {
    ConfigRootPlanRequest {
        file_name: file_name.to_owned(),
        text: text.to_owned(),
        base_path: "/".to_owned(),
    }
}

fn option_codes(text: &str) -> Vec<u32> {
    parse_config_root_plan(
        &MemoryConfigHost::default().with_directory_files(&["/project/a.ts"]),
        request("/project/tsconfig.json", text),
    )
    .expect("paths config returns a partial plan")
    .option_diagnostics()
    .iter()
    .map(|diagnostic| diagnostic.code())
    .collect()
}

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn immutable_config_and_program_option_snapshots_are_worker_shareable() {
    assert_send_sync::<ConfigModuleResolutionOptions>();
    assert_send_sync::<ProgramOptions>();
}

#[test]
fn config_projection_carries_the_currently_modeled_resolver_options() {
    let text = r#"{
        "compilerOptions": {
            "checkJs": true,
            "module": "preserve",
            "moduleResolution": "bundler",
            "resolvePackageJsonExports": false,
            "resolvePackageJsonImports": true,
            "customConditions": ["development", "browser"],
            "allowArbitraryExtensions": true,
            "allowImportingTsExtensions": true,
            "rewriteRelativeImportExtensions": true,
            "resolveJsonModule": false,
            "noLib": true,
            "preserveSymlinks": true,
            "types": ["node"],
            "typeRoots": ["./types"],
            "rootDirs": ["./src", "./generated"]
        },
        "files": ["a.ts"]
    }"#;
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", text),
    )
    .expect("resolver-facing config projection");
    assert!(plan.errors().is_empty());
    let projected = plan.module_resolution_options();
    let compiler = projected.compiler_options();
    assert!(compiler.allow_js);
    assert_eq!(compiler.module, Some(200));
    assert_eq!(compiler.module_resolution, Some(100));
    assert_eq!(compiler.resolve_package_json_exports, Some(false));
    assert_eq!(compiler.resolve_package_json_imports, Some(true));
    assert_eq!(
        compiler.custom_conditions.as_deref(),
        Some(["development".to_owned(), "browser".to_owned()].as_slice())
    );
    assert_eq!(compiler.allow_arbitrary_extensions, Some(true));
    assert_eq!(compiler.allow_importing_ts_extensions, Some(true));
    assert_eq!(compiler.rewrite_relative_import_extensions, Some(true));
    assert_eq!(compiler.resolve_json_module, Some(false));

    let program = projected.program_options();
    assert_eq!(program.no_lib(), Some(true));
    assert_eq!(program.preserve_symlinks(), Some(true));
    assert_eq!(program.types(), Some(["node".to_owned()].as_slice()));
    assert_eq!(
        program
            .type_roots()
            .unwrap()
            .iter()
            .map(|path| path.display())
            .collect::<Vec<_>>(),
        [Path::new("/project/types")]
    );
    assert_eq!(
        program
            .root_dirs()
            .unwrap()
            .iter()
            .map(|path| path.display())
            .collect::<Vec<_>>(),
        [Path::new("/project/src"), Path::new("/project/generated")]
    );
    assert_eq!(
        program.config_file_path().unwrap().display(),
        Path::new("/project/tsconfig.json")
    );
}

#[test]
fn official_paths_validation_shapes_are_options_diagnostics_not_parse_errors() {
    let cases = [
        (
            r#"{"compilerOptions":{"baseUrl":".","paths":{"*":"*"}}}"#,
            vec![5063],
        ),
        (
            r#"{"compilerOptions":{"baseUrl":".","paths":{"*":[1]}}}"#,
            vec![5064],
        ),
        (
            r#"{"compilerOptions":{"baseUrl":".","paths":{"foo":[]}}}"#,
            vec![5066],
        ),
        (
            r#"{"compilerOptions":{"baseUrl":"./src","paths":{"@interface/**/*":["./src/interface/*"],"@service/**/*":["./src/service/**/*"],"@controller/*":["controller/*"]}}}"#,
            vec![5061, 5061, 5062],
        ),
        (
            r#"{"compilerOptions":{"paths":{"@interface/*":["src/interface/*"],"@blah":["blah"],"@humbug/*":["*/generated"]}}}"#,
            vec![5090, 5090, 5090],
        ),
    ];

    for (text, mut expected) in cases {
        let plan = parse_config_root_plan(
            &MemoryConfigHost::default().with_directory_files(&["/project/a.ts"]),
            request("/project/tsconfig.json", text),
        )
        .expect("official validation shape returns a partial plan");
        assert!(plan.errors().is_empty(), "{text}");
        let mut actual = plan
            .option_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>();
        actual.sort_unstable();
        expected.sort_unstable();
        assert_eq!(actual, expected, "{text}");
    }
}

#[test]
fn paths_diagnostic_locations_follow_root_syntax_fallback_and_compacted_indices() {
    let text = r#"{"compilerOptions":{"paths":{"x":[missing,"bare"]}}}"#;
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default().with_directory_files(&["/project/a.ts"]),
        request("/project/tsconfig.json", text),
    )
    .expect("compacted paths config");
    let [diagnostic] = plan.option_diagnostics() else {
        panic!("one compacted substitution diagnostic expected")
    };
    assert_eq!(diagnostic.code(), 5090);
    assert_eq!(diagnostic.message_text(), "Non-relative paths are not allowed when 'baseUrl' is not set. Did you forget a leading './'?");
    assert_eq!(diagnostic.start, Some(text.find("missing").unwrap() as u32));
    assert_eq!(diagnostic.length, Some("missing".len() as u32));

    let inherited = r#"{"compilerOptions":{"paths":{"bad**":["bare"]}}}"#;
    let root = r#"{"extends":"../base.json","compilerOptions":{},"files":["a.ts"]}"#;
    let inherited_plan = parse_config_root_plan(
        &MemoryConfigHost::default().with_file("/base.json", inherited),
        request("/project/tsconfig.json", root),
    )
    .expect("inherited paths diagnostics use root fallback");
    assert_eq!(
        inherited_plan
            .option_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5061, 5090]
    );
    for diagnostic in inherited_plan.option_diagnostics() {
        assert_eq!(
            diagnostic.file_name.as_deref(),
            Some("/project/tsconfig.json")
        );
        assert_eq!(
            diagnostic.start,
            Some(root.find("\"compilerOptions\"").unwrap() as u32)
        );
        assert_eq!(diagnostic.length, Some("\"compilerOptions\"".len() as u32));
    }

    let no_compiler_options = r#"{"extends":"../base.json","files":["a.ts"]}"#;
    let global = parse_config_root_plan(
        &MemoryConfigHost::default().with_file("/base.json", inherited),
        request("/project/tsconfig.json", no_compiler_options),
    )
    .expect("missing compilerOptions uses fileless diagnostics");
    assert!(global
        .option_diagnostics()
        .iter()
        .all(|diagnostic| diagnostic.file_name.is_none()
            && diagnostic.start.is_none()
            && diagnostic.length.is_none()));

    let repeated_inherited = r#"{"compilerOptions":{"paths":{"a":["bare"],"b":["bare"]}}}"#;
    let deduped = parse_config_root_plan(
        &MemoryConfigHost::default().with_file("/base.json", repeated_inherited),
        request("/project/tsconfig.json", root),
    )
    .expect("identical inherited fallback diagnostics are deduplicated");
    assert_eq!(
        deduped
            .option_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5090]
    );

    let astral = r#"{"note":"😀","compilerOptions":{"paths":{"x":["bare"]}},"files":["a.ts"]}"#;
    let astral_plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", astral),
    )
    .expect("paths location after an astral scalar");
    let [diagnostic] = astral_plan.option_diagnostics() else {
        panic!("one astral-offset diagnostic expected")
    };
    let byte_start = astral.find("\"bare\"").expect("bare substitution token");
    assert_eq!(
        diagnostic.start,
        Some(astral[..byte_start].encode_utf16().count() as u32)
    );
    assert_eq!(diagnostic.length, Some("\"bare\"".len() as u32));
}

#[test]
fn duplicate_paths_syntax_uses_effective_values_and_typescript_location_walks() {
    let text = r#"{"compilerOptions":{"baseUrl":".","paths":{"bad**":["first**"],"bad**":["second**"],"wrong":[],"wrong":[]},"paths":{"bad**":["effective**"],"wrong":"effective"}},"files":["a.ts"]}"#;
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", text),
    )
    .expect("duplicate paths config");

    let key_diagnostics = plan
        .option_diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.code() == 5061)
        .collect::<Vec<_>>();
    assert_eq!(key_diagnostics.len(), 3);
    let element_diagnostics = plan
        .option_diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.code() == 5062)
        .collect::<Vec<_>>();
    assert_eq!(element_diagnostics.len(), 3);
    assert!(element_diagnostics
        .iter()
        .all(|diagnostic| diagnostic.message_text().contains("effective")));
    assert_eq!(
        plan.option_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.code() == 5063)
            .count(),
        3,
        "value diagnostics visit every duplicate key in every paths property"
    );
}

#[test]
fn config_dir_substitution_is_validated_after_final_merge() {
    let text = r#"{"compilerOptions":{"paths":{"x":["${configDir}/a**"]}},"files":["a.ts"]}"#;
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", text),
    )
    .expect("configDir paths config");
    let [diagnostic] = plan.option_diagnostics() else {
        panic!("one multi-star substitution diagnostic expected")
    };
    assert_eq!(diagnostic.code(), 5062);
    assert!(diagnostic.message_text().contains("/project/a**"));
}

#[test]
fn paths_and_declaring_base_project_atomically_into_the_resolver() {
    let base = r#"{"compilerOptions":{"paths":{"p1":["./lib/p1"]}}}"#;
    let root = r#"{"extends":"../other/tsconfig.base.json","compilerOptions":{"module":"commonjs"},"files":["index.ts"]}"#;
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default().with_file("/other/tsconfig.base.json", base),
        request("/project/tsconfig.json", root),
    )
    .expect("inherited paths projection");
    let projected = plan.module_resolution_options();
    assert_eq!(
        projected.program_options().paths_base_path(),
        Some("/other")
    );
    assert_eq!(
        projected.program_options().paths().unwrap()[0].pattern(),
        "p1"
    );

    let host = MemoryCompilerHost::builder("/project")
        .file("/project/index.ts", b"import 'p1';".to_vec())
        .file("/other/lib/p1/index.ts", b"export const p1 = 0;".to_vec())
        .build()
        .expect("build inherited paths resolver host");
    let mut resolver = ModuleResolver::new_with_program_options(
        &host,
        projected.compiler_options(),
        projected.program_options(),
    )
    .expect("create inherited paths resolver");
    let outcome = resolver
        .resolve(
            Path::new("/project/index.ts"),
            "p1",
            ResolutionMode::CommonJs,
        )
        .expect("resolve inherited mapping");
    let ResolutionOutcome::Resolved(module) = outcome else {
        panic!("inherited paths mapping must resolve")
    };
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/other/lib/p1/index.ts")
    );
}

#[test]
fn inherited_base_url_wins_and_masked_paths_drop_the_stale_base() {
    let base = r#"{"compilerOptions":{"baseUrl":".","paths":{"old":["./old"]}}}"#;
    let root = r#"{"extends":"../other/tsconfig.base.json","compilerOptions":{"paths":{"p1":["./lib/p1"]}},"files":["index.ts"]}"#;
    let host = MemoryConfigHost::default().with_file("/other/tsconfig.base.json", base);
    let plan = parse_config_root_plan(&host, request("/project/tsconfig.json", root))
        .expect("inherited baseUrl projection");
    let projected = plan.module_resolution_options();
    assert_eq!(
        projected.compiler_options().base_url.as_deref(),
        Some("/other")
    );
    assert_eq!(
        projected.program_options().paths_base_path(),
        Some("/project")
    );

    let masked_root = r#"{"extends":"../other/tsconfig.base.json","compilerOptions":{"paths":null},"files":["index.ts"]}"#;
    let masked = parse_config_root_plan(&host, request("/project/tsconfig.json", masked_root))
        .expect("masked paths projection");
    assert!(masked
        .module_resolution_options()
        .program_options()
        .paths()
        .is_none());
    assert!(masked
        .module_resolution_options()
        .program_options()
        .paths_base_path()
        .is_none());
}

#[test]
fn absolute_and_relative_substitutions_do_not_require_base_url() {
    let text = r#"{"compilerOptions":{"paths":{"x":[".","..","./x","..\\x","/x","\\x","C:","C:/x","scheme://host/x","scheme:\\\\host\\x","C:relative","node:x","bare",""]}},"files":["a.ts"]}"#;
    assert_eq!(option_codes(text), [5090, 5090, 5090, 5090, 5090]);
}

#[test]
fn non_string_substitutions_remain_diagnostic_instead_of_panicking() {
    let text = r#"{"compilerOptions":{"paths":{"x":[null,true,{"nested":1},["a",null,"b"]]}},"files":["a.ts"]}"#;
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", text),
    )
    .expect("non-string substitutions remain a partial plan");
    assert_eq!(
        plan.option_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5064, 5064, 5064, 5064]
    );
    assert_eq!(
        plan.option_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.message_text())
            .collect::<Vec<_>>(),
        [
            "Substitution 'null' for pattern 'x' has incorrect type, expected 'string', got 'object'.",
            "Substitution 'true' for pattern 'x' has incorrect type, expected 'string', got 'boolean'.",
            "Substitution '[object Object]' for pattern 'x' has incorrect type, expected 'string', got 'object'.",
            "Substitution 'a,,b' for pattern 'x' has incorrect type, expected 'string', got 'object'.",
        ]
    );
}
