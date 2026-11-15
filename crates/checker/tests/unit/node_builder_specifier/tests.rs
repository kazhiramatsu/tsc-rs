use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use tsc_emitter::EmitImportIncludeReason;

use super::*;
use crate::state::test_support::with_program_state;

#[test]
fn declaration_module_name_views_keep_the_uncached_order() {
    let files = [(
        "/project/main.ts",
        "type Early = import('early').Type;\n\
         import value from 'static';\n\
         export * from 'last-static';\n\
         const later = await import('late');\n\
         declare module 'augmentation' { interface Added {} }",
    )];
    with_program_state(&files, &CompilerOptions::default(), |state| {
        let uncached = module_name_literals(state, 0);
        assert!(matches!(uncached, ModuleNameLiterals::Collected(_)));
        assert!(state.declaration_emit_inputs.is_none());
        assert_eq!(
            uncached
                .lists()
                .imports
                .iter()
                .chain(&uncached.lists().augmentations)
                .map(|&node| literal_text(state, node)
                    .expect("module literal")
                    .as_str()
                    .expect("scalar fixture")
                    .to_owned())
                .collect::<Vec<_>>(),
            ["static", "last-static", "early", "late", "augmentation"]
        );

        state
            .emit_precalculate_declaration_emit_visibility(state.binder.source(0).root)
            .expect("declaration preparation");
        let cold = module_name_literals(state, 0);
        let warm = module_name_literals(state, 0);
        assert_eq!(cold.lists().imports, uncached.lists().imports);
        assert_eq!(cold.lists().augmentations, uncached.lists().augmentations);
        let (ModuleNameLiterals::Retained(cold), ModuleNameLiterals::Retained(warm)) = (cold, warm)
        else {
            panic!("declaration preparation must retain the input lists");
        };
        assert!(Arc::ptr_eq(&cold, &warm));
    });
}

#[test]
fn retained_module_name_views_survive_other_files_and_cache_replacement() {
    let files = [
        ("/project/first.ts", "import 'first'; export {};"),
        ("/project/second.ts", "import 'second'; export {};"),
    ];
    with_program_state(&files, &CompilerOptions::default(), |state| {
        state
            .emit_precalculate_declaration_emit_visibility(state.binder.source(0).root)
            .expect("declaration preparation");
        // Insert in reverse file order; the selected lists depend on file identity.
        let second = module_name_literals(state, 1);
        let first = module_name_literals(state, 0);
        let (ModuleNameLiterals::Retained(first), ModuleNameLiterals::Retained(second)) =
            (first, second)
        else {
            panic!("prepared views");
        };
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(literal_text(state, first.imports[0]), Some("first".into()));
        assert_eq!(
            literal_text(state, second.imports[0]),
            Some("second".into())
        );

        state
            .declaration_emit_inputs
            .as_ref()
            .expect("prepared inputs")
            .module_names
            .borrow_mut()
            .clear();
        let ModuleNameLiterals::Retained(replacement) = module_name_literals(state, 0) else {
            panic!("replacement view");
        };
        assert!(!Arc::ptr_eq(&first, &replacement));
        assert_eq!(first.imports, replacement.imports);
        assert_eq!(first.augmentations, replacement.augmentations);
    });
}

struct TestModuleSpecifierHost {
    current_directory: String,
    files: BTreeSet<String>,
    reasons: BTreeMap<String, Vec<EmitImportIncludeReason>>,
    default_mode: EmitResolutionMode,
    index_modes: BTreeMap<u32, EmitResolutionMode>,
    include_reason_calls: Cell<usize>,
    directories: Vec<(JsString, JsString)>,
    case_sensitive: bool,
    global_cache: Option<JsString>,
    reference_redirect: Option<JsString>,
    redirects: Vec<JsString>,
}

impl Default for TestModuleSpecifierHost {
    fn default() -> Self {
        Self {
            current_directory: "/project".to_owned(),
            files: BTreeSet::new(),
            reasons: BTreeMap::new(),
            default_mode: EmitResolutionMode::CommonJs,
            index_modes: BTreeMap::new(),
            include_reason_calls: Cell::new(0),
            directories: Vec::new(),
            case_sensitive: true,
            global_cache: None,
            reference_redirect: None,
            redirects: Vec::new(),
        }
    }
}

impl EmitModuleSpecifierHost for TestModuleSpecifierHost {
    fn get_current_directory(&self) -> JsString {
        self.current_directory.clone().into()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive
    }

    fn file_exists(&self, file_name: JsStr<'_>) -> bool {
        self.files.iter().any(|name| file_name == name.as_str())
    }

    fn read_file(&self, _file_name: JsStr<'_>) -> Option<String> {
        None
    }

    fn get_common_source_directory(&self) -> JsString {
        self.current_directory.clone().into()
    }

    fn get_default_resolution_mode_for_file(&self, _file: EmitResolverNode) -> EmitResolutionMode {
        self.default_mode
    }

    fn get_mode_for_resolution_at_index(
        &self,
        _file: EmitResolverNode,
        index: u32,
    ) -> EmitResolutionMode {
        self.index_modes
            .get(&index)
            .copied()
            .unwrap_or(EmitResolutionMode::None)
    }

    fn import_include_reasons(&self, imported_path: JsStr<'_>) -> Vec<EmitImportIncludeReason> {
        self.include_reason_calls
            .set(self.include_reason_calls.get() + 1);
        self.reasons
            .iter()
            .find(|(name, _)| imported_path == name.as_str())
            .map(|(_, reasons)| reasons.clone())
            .unwrap_or_default()
    }

    fn symlinked_directories(&self) -> Vec<(JsString, JsString)> {
        self.directories.clone()
    }

    fn get_global_typings_cache_location(&self) -> Option<JsString> {
        self.global_cache.clone()
    }

    fn project_reference_output_of_source(&self, _source: JsStr<'_>) -> Option<JsString> {
        self.reference_redirect.clone()
    }

    fn redirect_targets(&self, _file_path: JsStr<'_>) -> Vec<JsString> {
        self.redirects.clone()
    }
}

#[test]
fn directory_inputs_preserve_ancestor_and_duplicate_link_order() {
    let host = TestModuleSpecifierHost {
        directories: [
            ("/physical/pkg", "/project/node_modules/pkg-a"),
            ("\\physical\\pkg\\.", "/project/node_modules/pkg-b"),
            ("../physical/pkg/lib/..", "/project/node_modules/pkg-c"),
            ("/physical", "/project/node_modules/root"),
            ("/PHYSICAL/PKG", "/project/node_modules/pkg-λ"),
        ]
        .map(|(real, link)| (real.into(), link.into()))
        .to_vec(),
        case_sensitive: false,
        ..TestModuleSpecifierHost::default()
    };
    let mut paths = Vec::new();
    let _: Option<()> = for_each_file_name_of_module(
        "/project/src/main.ts",
        "/physical/pkg/lib/value.ts",
        &host,
        true,
        |path, redirect| {
            paths.push((path.to_owned(), redirect));
            None
        },
    );
    assert_eq!(
        paths,
        [
            "/project/node_modules/pkg-a/lib/value.ts",
            "/project/node_modules/pkg-b/lib/value.ts",
            "/project/node_modules/pkg-c/lib/value.ts",
            "/project/node_modules/pkg-λ/lib/value.ts",
            "/project/node_modules/root/pkg/lib/value.ts",
            "/physical/pkg/lib/value.ts",
        ]
        .map(|path| (JsString::from(path), false))
    );
}

#[test]
fn directory_inputs_keep_self_package_and_global_cache_stops() {
    let mut host = TestModuleSpecifierHost {
        directories: [
            ("/physical/pkg/lib", "/project/node_modules/inner"),
            ("/physical/pkg", "/project/node_modules/pkg"),
            ("/physical", "/project/node_modules/root"),
        ]
        .map(|(real, link)| (real.into(), link.into()))
        .to_vec(),
        ..TestModuleSpecifierHost::default()
    };
    let collect = |importer, host: &TestModuleSpecifierHost| {
        let mut paths = Vec::new();
        let _: Option<()> = for_each_file_name_of_module(
            importer,
            "/physical/pkg/lib/value.ts",
            host,
            true,
            |path, _| {
                paths.push(path.to_owned());
                None
            },
        );
        paths
    };
    assert_eq!(
        collect("/physical/pkg/lib/src/main.ts", &host),
        [JsString::from("/physical/pkg/lib/value.ts")]
    );
    host.global_cache = Some("/physical/pkg".into());
    assert_eq!(
        collect("/project/src/main.ts", &host),
        [
            "/project/node_modules/inner/value.ts",
            "/project/node_modules/pkg/lib/value.ts",
            "/physical/pkg/lib/value.ts",
        ]
        .map(JsString::from)
    );
}

#[test]
fn directory_inputs_preserve_redirect_order_and_callback_results() {
    let host = TestModuleSpecifierHost {
        directories: [
            ("/physical/pkg", "/project/node_modules/first"),
            ("/physical/pkg/.", "/project/node_modules/second"),
        ]
        .map(|(real, link)| (real.into(), link.into()))
        .to_vec(),
        reference_redirect: Some("/physical/pkg/lib/value.d.ts".into()),
        redirects: vec!["/physical/pkg/lib/other.d.ts".into()],
        ..TestModuleSpecifierHost::default()
    };
    let mut paths = Vec::new();
    let _: Option<()> = for_each_file_name_of_module(
        "/project/src/main.ts",
        "/physical/pkg/lib/value.ts",
        &host,
        true,
        |path, redirect| {
            paths.push((path.to_owned(), redirect));
            None
        },
    );
    assert_eq!(
        paths,
        [
            ("/project/node_modules/first/lib/value.d.ts", true),
            ("/project/node_modules/first/lib/value.ts", false),
            ("/project/node_modules/first/lib/other.d.ts", false),
            ("/project/node_modules/second/lib/value.d.ts", true),
            ("/project/node_modules/second/lib/value.ts", false),
            ("/project/node_modules/second/lib/other.d.ts", false),
            ("/physical/pkg/lib/value.d.ts", true),
            ("/physical/pkg/lib/value.ts", false),
            ("/physical/pkg/lib/other.d.ts", false),
        ]
        .map(|(path, redirect)| (JsString::from(path), redirect))
    );
    for (prefer_symlinks, expected) in [
        (true, "/project/node_modules/first/lib/value.d.ts"),
        (false, "/physical/pkg/lib/value.d.ts"),
    ] {
        let mut calls = 0;
        let result = for_each_file_name_of_module(
            "/project/src/main.ts",
            "/physical/pkg/lib/value.ts",
            &host,
            prefer_symlinks,
            |path, redirect| {
                calls += 1;
                Some((path.to_owned(), redirect))
            },
        );
        assert_eq!(result, Some((JsString::from(expected), true)));
        assert_eq!(calls, 1);
    }
}

fn external_module_symbol(state: &CheckerState<'_>, file_index: usize) -> SymbolId {
    state
        .binder
        .node_symbol(state.binder.source(file_index).root)
        .expect("source file is an external module")
}

#[test]
fn computes_same_directory_and_subdirectory_relative_specifiers() {
    let files = [
        ("/project/src/main.ts", "export {};"),
        ("/project/src/value.ts", "export const value = 1;"),
        ("/project/src/sub/nested.ts", "export const nested = 1;"),
    ];
    with_program_state(&files, &CompilerOptions::default(), |state| {
        let host = TestModuleSpecifierHost::default();
        let enclosing = state.binder.source(0).root;
        let same_directory = get_specifier_for_module_symbol(
            state,
            external_module_symbol(state, 1),
            Some(&host),
            Some(enclosing),
            None,
            None,
        )
        .expect("same-directory specifier")
        .specifier;
        let subdirectory = get_specifier_for_module_symbol(
            state,
            external_module_symbol(state, 2),
            Some(&host),
            Some(enclosing),
            None,
            None,
        )
        .expect("subdirectory specifier")
        .specifier;

        assert_eq!(same_directory, "./value");
        assert_eq!(subdirectory, "./sub/nested");
    });
}

#[test]
fn computes_visible_node_modules_package_root_specifier() {
    let files = [
        (
            "/project/node_modules/pkg/index.d.ts",
            "export const value: number;",
        ),
        ("/project/src/main.ts", "export {};"),
    ];
    with_program_state(&files, &CompilerOptions::default(), |state| {
        let specifier = get_specifier_for_module_symbol(
            state,
            external_module_symbol(state, 0),
            Some(&TestModuleSpecifierHost::default()),
            Some(state.binder.source(1).root),
            None,
            None,
        )
        .expect("node_modules package specifier")
        .specifier;

        assert_eq!(specifier, "pkg");
    });
}

#[test]
fn returns_declared_ambient_module_name() {
    let files = [
        (
            "/project/ambient.d.ts",
            "declare module \"ambient-pkg\" { export const value: number; }",
        ),
        ("/project/main.ts", "export {};"),
    ];
    with_program_state(&files, &CompilerOptions::default(), |state| {
        let declaration = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .find(|&node| state.kind_of(node) == SyntaxKind::ModuleDeclaration)
            .expect("ambient module declaration");
        let symbol = state
            .binder
            .node_symbol(declaration)
            .expect("ambient module symbol");
        let name = get_specifier_for_module_symbol(
            state,
            symbol,
            Some(&TestModuleSpecifierHost::default()),
            Some(state.binder.source(1).root),
            None,
            None,
        )
        .expect("ambient module name")
        .specifier;

        assert_eq!(name, "ambient-pkg");
    });
}

#[test]
fn returns_amd_module_name_through_export_equals_equivalent_file() {
    let files = [(
        "/project/named.ts",
        "/// <amd-module name=\"amd-name\" />\nconst value = 1; export = value;",
    )];
    with_program_state(&files, &CompilerOptions::default(), |state| {
        let declaration = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .find(|&node| state.kind_of(node) == SyntaxKind::VariableDeclaration)
            .expect("export-equals value declaration");
        let symbol = state
            .binder
            .node_symbol(declaration)
            .expect("export-equals value symbol");

        let name = get_specifier_for_module_symbol(state, symbol, None, None, None, None)
            .expect("AMD module name")
            .specifier;

        assert_eq!(name, "amd-name");
    });
}

#[test]
fn process_ending_honors_js_minimal_and_index_preferences() {
    let options = SpecifierCompilerOptions::new(&CompilerOptions::default());
    let mut host = TestModuleSpecifierHost::default();

    assert_eq!(
        process_ending(
            "./pkg/index.ts",
            Some("./pkg/index.ts".into()),
            &[ModuleSpecifierEnding::Minimal],
            &options,
            Some(&host),
        )
        .as_ref()
        .map(|value| value.as_js().as_str().expect("scalar name observation")),
        Some("./pkg")
    );
    host.files.insert("./pkg.js".to_owned());
    assert_eq!(
        process_ending(
            "./pkg/index.ts",
            Some("./pkg/index.ts".into()),
            &[ModuleSpecifierEnding::Minimal],
            &options,
            Some(&host),
        )
        .as_ref()
        .map(|value| value.as_js().as_str().expect("scalar name observation")),
        Some("./pkg/index")
    );
    assert_eq!(
        process_ending(
            "./pkg/index.ts",
            None,
            &[ModuleSpecifierEnding::Index],
            &options,
            None,
        )
        .as_ref()
        .map(|value| value.as_js().as_str().expect("scalar name observation")),
        Some("./pkg/index")
    );
    assert_eq!(
        process_ending(
            "./value.ts",
            None,
            &[ModuleSpecifierEnding::JsExtension],
            &options,
            None,
        )
        .as_ref()
        .map(|value| value.as_js().as_str().expect("scalar name observation")),
        Some("./value.js")
    );
}

#[test]
fn reuses_existing_parse_tree_specifier_when_reason_and_modes_match() {
    let files = [
        ("/project/src/target.ts", "export const value = 1;"),
        (
            "/project/src/main.ts",
            "import { value } from \"chosen-package/subpath\"; export { value };",
        ),
    ];
    with_program_state(&files, &CompilerOptions::default(), |state| {
        let import_declaration = state
            .binder
            .source(1)
            .arena
            .node_ids()
            .find(|&node| state.kind_of(node) == SyntaxKind::ImportDeclaration)
            .expect("import declaration");
        assert!(can_have_module_specifier(state, import_declaration));
        let literal = try_get_module_specifier_from_declaration(state, import_declaration)
            .expect("existing specifier literal");
        assert_eq!(
            literal_text(state, literal),
            Some("chosen-package/subpath".into())
        );

        let mut host = TestModuleSpecifierHost::default();
        host.index_modes.insert(0, EmitResolutionMode::CommonJs);
        host.reasons.insert(
            "/project/src/target.ts".to_owned(),
            vec![EmitImportIncludeReason {
                importing_file: SourceFileId::from_raw(1),
                index: 0,
            }],
        );
        let specifier = get_specifier_for_module_symbol(
            state,
            external_module_symbol(state, 0),
            Some(&host),
            Some(state.binder.source(1).root),
            Some(import_declaration),
            None,
        )
        .expect("reused specifier")
        .specifier;

        assert_eq!(specifier, "chosen-package/subpath");

        // tsgo getSpecifierForModuleSymbol takes the enclosing declaration's
        // mode from Program.GetModeForUsageLocation, which the checker
        // answers: the host's mode at the import's index does not change it,
        // so the existing specifier is still reused.
        host.index_modes.insert(0, EmitResolutionMode::EsNext);
        let declaration_mode_specifier = get_specifier_for_module_symbol(
            state,
            external_module_symbol(state, 0),
            Some(&host),
            Some(state.binder.source(1).root),
            Some(import_declaration),
            None,
        )
        .expect("enclosing-declaration resolution mode")
        .specifier;
        assert_eq!(declaration_mode_specifier, "chosen-package/subpath");
    });
}

#[test]
fn parse_tree_indices_drive_mode_and_nonrelative_reuse_gates() {
    let files = [
        ("/project/src/target.ts", "export const value = 1;"),
        (
            "/project/src/main.ts",
            "import { value } from \"chosen-package/subpath\";\n\
             type Dynamic = import(\"dynamic-name\").Type;\n\
             export * from \"./stale\";\n\
             const later = import(\"dynamic-later\");\n\
             export { value, later };",
        ),
    ];
    with_program_state(&files, &CompilerOptions::default(), |state| {
        assert_eq!(
            (0..4)
                .map(|index| get_module_name_string_literal_at(state, 1, index)
                    .map(|value| value.as_str().expect("scalar value observation").to_owned()))
                .collect::<Vec<_>>(),
            [
                Some("chosen-package/subpath".to_owned()),
                Some("./stale".to_owned()),
                Some("dynamic-name".to_owned()),
                Some("dynamic-later".to_owned()),
            ]
        );

        let mut host = TestModuleSpecifierHost::default();
        let importing_file = state.binder.source(1).root;
        let importing_node = EmitResolverNode::new(SourceFileId::from_raw(1), importing_file);
        let module_paths = [ModulePath {
            path: ("/project/src/target.ts".to_owned()).into(),
            is_redirect: false,
            is_in_node_modules: false,
        }];
        let compiler_options = SpecifierCompilerOptions::new(state.options);
        let user_preferences = ModuleSpecifierUserPreferences {
            import_module_specifier_preference: ImportModuleSpecifierPreference::NON_RELATIVE,
            ..ModuleSpecifierUserPreferences::default()
        };
        let options = ModuleSpecifierOptions::default();

        host.reasons.insert(
            "/project/src/target.ts".to_owned(),
            vec![EmitImportIncludeReason {
                importing_file: SourceFileId::from_raw(1),
                index: 0,
            }],
        );
        host.index_modes.insert(0, EmitResolutionMode::EsNext);
        let mode_mismatch = compute_module_specifiers(
            state,
            &module_paths,
            &compiler_options,
            importing_file,
            importing_node,
            &host,
            &user_preferences,
            &options,
            false,
        )
        .expect("mode mismatch computation");
        assert_eq!(mode_mismatch.module_specifiers, ["./target"]);

        host.index_modes.insert(0, EmitResolutionMode::CommonJs);
        let matching_bare = compute_module_specifiers(
            state,
            &module_paths,
            &compiler_options,
            importing_file,
            importing_node,
            &host,
            &user_preferences,
            &options,
            false,
        )
        .expect("matching existing bare specifier");
        assert_eq!(matching_bare.module_specifiers, ["chosen-package/subpath"]);

        host.reasons.insert(
            "/project/src/target.ts".to_owned(),
            vec![EmitImportIncludeReason {
                importing_file: SourceFileId::from_raw(1),
                index: 1,
            }],
        );
        host.index_modes.insert(1, EmitResolutionMode::CommonJs);
        let rejected_relative = compute_module_specifiers(
            state,
            &module_paths,
            &compiler_options,
            importing_file,
            importing_node,
            &host,
            &user_preferences,
            &options,
            false,
        )
        .expect("relative existing specifier rejection");
        assert_eq!(rejected_relative.module_specifiers, ["./target"]);
    });
}

#[test]
fn symbol_cache_hits_by_context_and_misses_by_resolution_mode() {
    let files = [
        ("/project/src/target.ts", "export const value = 1;"),
        ("/project/src/main.ts", "export {};"),
    ];
    with_program_state(&files, &CompilerOptions::default(), |state| {
        let host = TestModuleSpecifierHost::default();
        let symbol = external_module_symbol(state, 0);
        let enclosing = state.binder.source(1).root;

        let first = get_specifier_for_module_symbol(
            state,
            symbol,
            Some(&host),
            Some(enclosing),
            None,
            None,
        )
        .expect("first computation")
        .specifier;
        let first_reason_calls = host.include_reason_calls.get();
        let hit = get_specifier_for_module_symbol(
            state,
            symbol,
            Some(&host),
            Some(enclosing),
            None,
            None,
        )
        .expect("cache hit")
        .specifier;
        let mode_miss = get_specifier_for_module_symbol(
            state,
            symbol,
            Some(&host),
            Some(enclosing),
            None,
            Some(EmitResolutionMode::EsNext),
        )
        .expect("mode-keyed cache miss")
        .specifier;

        assert_eq!(first, "./target");
        assert_eq!(hit, first);
        assert_eq!(host.include_reason_calls.get(), first_reason_calls + 1);
        assert_eq!(mode_miss, "./target.js");
        assert_eq!(
            state
                .links
                .symbol_cold()
                .specifier_cache
                .get(symbol)
                .as_ref()
                .map(BTreeMap::len),
            Some(2)
        );
    });
}

#[test]
fn supported_exclusion_regexes_match_javascript_utf16_and_unicode_values() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../fixtures/utf16-specifier-regex-values.json"
    ))
    .unwrap();
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 12);
    for case in fixture["cases"].as_array().unwrap() {
        let value: JsString = case["value"]
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
            .collect();
        let pattern = case["pattern"].as_str().unwrap();
        assert!(
            matches!(
                string_to_regex(pattern),
                Some(CompiledExcludeRegex::Supported(_))
            ),
            "{}: supported subset",
            case["id"]
        );
        assert_eq!(
            is_excluded_by_regex(value.as_js(), &[pattern.to_owned()]),
            case["matched"].as_bool().unwrap(),
            "{}",
            case["id"]
        );
    }
}

/// tsgo `IsExcludedByRegex`: a lazy quantifier (`*?`) is a quantifier, and a
/// pattern whose only slash is its first is no `/pattern/flags` literal
/// (TestAutoImportSpecifierExcludeRegexes1's `/.*?$` excludes
/// `ambient/utils`, not `ambient`).
#[test]
fn exclusion_regexes_accept_lazy_quantifiers() {
    let excluded = |specifier: &str, pattern: &str| {
        is_excluded_by_regex(JsStr::from_str(specifier), &[pattern.to_owned()])
    };
    assert!(excluded("ambient/utils", "/.*?$"));
    assert!(!excluded("ambient", "/.*?$"));
    assert!(excluded("ambient/utils", "ut+?ils"));
    assert!(excluded("ambient", "^amb??ient$"));
    assert!(!excluded("ambient/utils", "oops("));
}

/// tsgo `tryGetModuleNameFromPaths`: a pattern is resolved against the base
/// directory (`resolvePathPatternIfInSameVolume`), so `..\x` from a base at
/// the root is `x`; a case-insensitive host compares the star's prefix and
/// suffix ignoring case.
#[test]
fn paths_patterns_resolve_against_the_base_directory() {
    let options = SpecifierCompilerOptions::new(&CompilerOptions::default());
    let host = TestModuleSpecifierHost::default();
    let mapping = |key: &str, pattern: &str| ModulePathMapping {
        key: key.into(),
        patterns: vec![pattern.into()],
    };
    let specifier = |relative_to_base_url: &str,
                     mappings: &[ModulePathMapping],
                     base_directory: &str,
                     case_sensitive: bool| {
        try_get_module_name_from_paths(
            relative_to_base_url,
            None,
            mappings,
            &[ModuleSpecifierEnding::Minimal],
            base_directory,
            case_sensitive,
            &host,
            &options,
        )
        .map(|value| {
            value
                .as_js()
                .as_str()
                .expect("scalar name observation")
                .to_owned()
        })
    };
    assert_eq!(
        specifier(
            "thisHasPathMapping.ts",
            &[mapping("foo", "..\\thisHasPathMapping")],
            "/",
            true,
        ),
        Some("foo".to_owned())
    );
    assert_eq!(
        specifier(
            "Lib/Util.ts",
            &[mapping("@lib/*", "lib/*")],
            "/project",
            false
        ),
        Some("@lib/Util".to_owned())
    );
    assert_eq!(
        specifier(
            "Lib/Util.ts",
            &[mapping("@lib/*", "lib/*")],
            "/project",
            true
        ),
        None
    );
}

/// tsgo `GetNodeModulePathParts`' package facts: the innermost package's
/// name (a scoped one's with its scope), the path below its root, a nested
/// `node_modules`, and a file directly in `node_modules`.
#[test]
fn node_module_package_path_matches_tsgo() {
    let package = |path: &str| {
        node_module_package_path(path).map(|parts| {
            (
                parts.package_name,
                parts.package_relative_path,
                parts.has_nested_node_modules,
                parts.is_direct_node_modules_file,
            )
        })
    };
    assert_eq!(
        package("/p/node_modules/pkg/dist/index.d.ts"),
        Some(("pkg".to_owned(), "dist/index.d.ts".to_owned(), false, false))
    );
    assert_eq!(
        package("/p/node_modules/@types/node/fs.d.ts"),
        Some(("@types/node".to_owned(), "fs.d.ts".to_owned(), false, false))
    );
    assert_eq!(
        package("/p/node_modules/a/node_modules/b/index.d.ts"),
        Some(("b".to_owned(), "index.d.ts".to_owned(), true, false))
    );
    assert_eq!(
        package("/p/node_modules/umd.d.ts"),
        Some(("umd.d.ts".to_owned(), String::new(), false, true))
    );
    assert_eq!(package("/p/src/index.ts"), None);
}

#[test]
fn normalized_slash_spans_preserve_wtf8() {
    use tsc_types::JsString;
    for units in [
        vec![],
        vec![0x5c, 0x5c, 0x61, 0x5c],
        vec![0xD800, 0x5c, 0xDC00, 0x5c, 0xD83D, 0xDE00, 0x754C],
        vec![0xD800, 0xDC00, 0xD800],
    ] {
        let input = JsString::from_code_units(&units);
        let expected: Vec<u16> = units
            .iter()
            .map(|&unit| if unit == 0x5c { 0x2f } else { unit })
            .collect();
        assert_eq!(super::normalized_slashes(&input).to_utf16(), expected);
    }
}

struct PackageInputHost {
    text: String,
    prepared: Option<Arc<Value>>,
    reads: Cell<usize>,
}

impl EmitModuleSpecifierHost for PackageInputHost {
    fn get_current_directory(&self) -> JsString {
        "/project".into()
    }
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }
    fn file_exists(&self, _name: JsStr<'_>) -> bool {
        true
    }
    fn read_file(&self, _name: JsStr<'_>) -> Option<String> {
        self.reads.set(self.reads.get() + 1);
        Some(self.text.clone())
    }
    fn read_package_json_input(
        &self,
        name: JsStr<'_>,
    ) -> Option<tsc_emitter::EmitPackageJsonInput> {
        Some(match &self.prepared {
            Some(value) => tsc_emitter::EmitPackageJsonInput::Parsed(Arc::clone(value)),
            None => tsc_emitter::EmitPackageJsonInput::Text(self.read_file(name)?),
        })
    }
    fn get_common_source_directory(&self) -> JsString {
        "/project".into()
    }
    fn get_default_resolution_mode_for_file(&self, _file: EmitResolverNode) -> EmitResolutionMode {
        EmitResolutionMode::None
    }
    fn get_mode_for_resolution_at_index(
        &self,
        _file: EmitResolverNode,
        _index: u32,
    ) -> EmitResolutionMode {
        EmitResolutionMode::None
    }
}

#[test]
fn package_object_transport_preserves_reader_recovery_and_property_order() {
    let name = JsString::from("/project/package.json");
    for text in [
        r#"{"exports":{"2":"two","1":"one","types":"types","default":"js"},"nul":"a\u0000b","surrogate":"\ud800"}"#,
        r#"{/* comment */ "exports":{"types":"types","default":"js",},"duplicate":1,"duplicate":2,}"#,
        r#"{/* comment */ "__proto__":{"inherited":"value"},"own":"own","\u0000key":"\udfff"}"#,
        r#"{"__proto__":{"own":"value"},"duplicate":1,"duplicate":2}"#,
        r#"[1,2,3]"#,
        r#"{"broken": "#,
    ] {
        let snapshot = tsc_diagnostics::TextSnapshot::new(
            text.to_owned(),
            tsc_diagnostics::DocumentVersion::default(),
        );
        let prepared = Arc::new(Value::Object(
            tsc_program::read_package_json_object_from_snapshot(name.as_js(), &snapshot),
        ));
        let fallback = PackageInputHost {
            text: text.to_owned(),
            prepared: None,
            reads: Cell::new(0),
        };
        let source = PackageInputHost {
            text: "this text must never be read".to_owned(),
            prepared: Some(Arc::clone(&prepared)),
            reads: Cell::new(0),
        };
        let owned = read_package_json(&fallback, name.as_js()).expect("fallback package");
        let shared = read_package_json(&source, name.as_js()).expect("prepared package");
        assert_eq!(&*owned, &*shared, "{text}");
        let PackageJsonValue::Prepared(value) = &shared else {
            panic!("a structured input must retain its owner");
        };
        assert!(Arc::ptr_eq(value, &prepared));
        assert_eq!(fallback.reads.get(), 1);
        assert_eq!(source.reads.get(), 0);
        for property in [
            "exports",
            "duplicate",
            "__proto__",
            "inherited",
            "own",
            "nul",
            "surrogate",
            "\0key",
        ] {
            assert_eq!(
                package_json_property(&owned, property),
                package_json_property(&shared, property),
                "{property} in {text}"
            );
        }
        let keys = |value: &Value| {
            package_json_own_entries(value.as_object().expect("package object"))
                .into_iter()
                .map(|(key, _)| key.to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(keys(&owned), keys(&shared));
        if let Some(exports) = package_json_property(&owned, "exports") {
            let shared_exports = package_json_property(&shared, "exports").expect("exports");
            assert_eq!(keys(exports), keys(shared_exports));
        }
    }
}

#[test]
fn full_path_equality_preserves_ascii_and_javascript_unit_equivalence() {
    let cases: &[(&[u16], &[u16], bool)] = &[
        (&[], &[], true),
        (&[0x41, 0x2f, 0x5a], &[0x61, 0x2f, 0x7a], true),
        (&[0x61, 0, 0x42], &[0x41, 0, 0x62], true),
        (&[0x40], &[0x60], false),
        (&[0x5b], &[0x7b], false),
        (&[0x5c], &[0x7c], false),
        (&[0x5d], &[0x7d], false),
        (&[0x5e], &[0x7e], false),
        (&[0x391], &[0x3b1], false),
        (&[0x130], &[0x69], false),
        (&[0x212a], &[0x6b], false),
        (&[0xdf], &[0x53, 0x53], false),
        (&[0xe9], &[0x20ac], false),
        (&[0xd800], &[0xfffd], false),
        (&[0xd800, 0x41], &[0xd800, 0x61], true),
        (&[0xdc00, 0x5a], &[0xdc00, 0x7a], true),
        (&[0xd83d, 0xde00, 0x41], &[0xd83d, 0xde00, 0x61], true),
        (&[0xd83d, 0xde00], &[0xd83d], false),
        (&[0xd83d, 0xde00], &[0xde00, 0xd83d], false),
        (&[0xd801, 0xdc00], &[0xd801, 0xdc28], false),
        (&[0xd800, 0, 0xdc00, 0x41], &[0xd800, 0, 0xdc00, 0x61], true),
    ];
    for &(left, right, expected) in cases {
        let left = JsString::from_code_units(left);
        let right = JsString::from_code_units(right);
        assert_eq!(
            paths_equal(&left, &right, true),
            expected,
            "{left:?}, {right:?}"
        );
        assert_eq!(
            paths_equal(&right, &left, true),
            expected,
            "{left:?}, {right:?}"
        );
        assert_eq!(paths_equal(&left, &right, false), left == right);
        assert!(paths_equal(&left, &left, true));
        assert!(paths_equal(&right, &right, false));
    }
}

#[test]
fn path_prefix_and_suffix_keep_half_surrogate_matches() {
    let pair = JsString::from_code_units(&[0xd83d, 0xde00]);
    let high = JsString::from_code_units(&[0xd83d]);
    let low = JsString::from_code_units(&[0xde00]);
    for ignore_case in [false, true] {
        assert!(starts_with(&pair, &high, ignore_case));
        assert!(ends_with(&pair, &low, ignore_case));
        assert!(!paths_equal(&pair, &high, ignore_case));
        assert!(!paths_equal(&pair, &low, ignore_case));
        assert!(!starts_with(&pair, &low, ignore_case));
        assert!(!ends_with(&pair, &high, ignore_case));
    }
}

#[test]
fn package_export_patterns_require_disjoint_prefix_and_suffix() {
    for ignore_case in [false, true] {
        assert_eq!(
            match_path_pattern("abc", "ab".into(), "bc".into(), ignore_case),
            None
        );
        assert_eq!(
            match_path_pattern("abbc", "ab".into(), "bc".into(), ignore_case),
            Some(JsString::new())
        );
        assert_eq!(
            match_path_pattern("abXbc", "ab".into(), "bc".into(), ignore_case),
            Some("X".into())
        );
    }
}
