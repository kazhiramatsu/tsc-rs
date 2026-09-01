use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use tsc_emitter::EmitImportIncludeReason;

use super::*;
use crate::state::test_support::with_program_state;

struct TestModuleSpecifierHost {
    current_directory: String,
    files: BTreeSet<String>,
    reasons: BTreeMap<String, Vec<EmitImportIncludeReason>>,
    default_mode: EmitResolutionMode,
    index_modes: BTreeMap<u32, EmitResolutionMode>,
    include_reason_calls: Cell<usize>,
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
        }
    }
}

impl EmitModuleSpecifierHost for TestModuleSpecifierHost {
    fn get_current_directory(&self) -> String {
        self.current_directory.clone()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn file_exists(&self, file_name: &str) -> bool {
        self.files.contains(file_name)
    }

    fn read_file(&self, _file_name: &str) -> Option<String> {
        None
    }

    fn get_common_source_directory(&self) -> String {
        self.current_directory.clone()
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

    fn import_include_reasons(&self, imported_path: &str) -> Vec<EmitImportIncludeReason> {
        self.include_reason_calls
            .set(self.include_reason_calls.get() + 1);
        self.reasons.get(imported_path).cloned().unwrap_or_default()
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
            false,
            None,
        )
        .expect("same-directory specifier");
        let subdirectory = get_specifier_for_module_symbol(
            state,
            external_module_symbol(state, 2),
            Some(&host),
            Some(enclosing),
            None,
            false,
            None,
        )
        .expect("subdirectory specifier");

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
            false,
            None,
        )
        .expect("node_modules package specifier");

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
            false,
            None,
        )
        .expect("ambient module name");

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

        let name = get_specifier_for_module_symbol(state, symbol, None, None, None, false, None)
            .expect("AMD module name");

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
            &[ModuleSpecifierEnding::Minimal],
            &options,
            Some(&host),
        )
        .as_deref(),
        Some("./pkg")
    );
    host.files.insert("./pkg.js".to_owned());
    assert_eq!(
        process_ending(
            "./pkg/index.ts",
            &[ModuleSpecifierEnding::Minimal],
            &options,
            Some(&host),
        )
        .as_deref(),
        Some("./pkg/index")
    );
    assert_eq!(
        process_ending(
            "./pkg/index.ts",
            &[ModuleSpecifierEnding::Index],
            &options,
            None,
        )
        .as_deref(),
        Some("./pkg/index")
    );
    assert_eq!(
        process_ending(
            "./value.ts",
            &[ModuleSpecifierEnding::JsExtension],
            &options,
            None,
        )
        .as_deref(),
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
        assert_eq!(literal_text(state, literal), Some("chosen-package/subpath"));

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
            false,
            None,
        )
        .expect("reused specifier");

        assert_eq!(specifier, "chosen-package/subpath");

        host.index_modes.insert(0, EmitResolutionMode::EsNext);
        let declaration_mode_specifier = get_specifier_for_module_symbol(
            state,
            external_module_symbol(state, 0),
            Some(&host),
            Some(state.binder.source(1).root),
            Some(import_declaration),
            false,
            None,
        )
        .expect("enclosing-declaration resolution mode");
        assert_eq!(declaration_mode_specifier, "./target.js");
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
                .map(|index| get_module_name_string_literal_at(state, 1, index))
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
            path: "/project/src/target.ts".to_owned(),
            is_redirect: false,
            is_in_node_modules: false,
        }];
        let compiler_options = SpecifierCompilerOptions::new(state.options);
        let user_preferences = ModuleSpecifierUserPreferences {
            import_module_specifier_preference: Some(ImportModuleSpecifierPreference::NonRelative),
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
            false,
            None,
        )
        .expect("first computation");
        let first_reason_calls = host.include_reason_calls.get();
        let hit = get_specifier_for_module_symbol(
            state,
            symbol,
            Some(&host),
            Some(enclosing),
            None,
            false,
            None,
        )
        .expect("cache hit");
        let mode_miss = get_specifier_for_module_symbol(
            state,
            symbol,
            Some(&host),
            Some(enclosing),
            None,
            false,
            Some(EmitResolutionMode::EsNext),
        )
        .expect("mode-keyed cache miss");

        assert_eq!(first, "./target");
        assert_eq!(hit, first);
        assert_eq!(host.include_reason_calls.get(), first_reason_calls + 1);
        assert_eq!(mode_miss, "./target.js");
        assert_eq!(
            state
                .links
                .symbol(symbol)
                .specifier_cache
                .as_ref()
                .map(BTreeMap::len),
            Some(2)
        );
    });
}
