//! The project references of a `-p` compile (tsgo compiler/
//! projectreferenceparser.go, projectreferencefilemapper.go): the referenced
//! configs are parsed, their sources map to their output declaration files,
//! and the loader loads the output in place of the source.

use std::path::PathBuf;

use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_config_program_with_no_emit_override, parse_config_root_plan,
    resolve_config_file_name_of_project_reference, resolve_project_references, CanonicalPath,
    CompilerConfigHost, ConfigRootPlanRequest, LibraryCatalog, ProgramLoadLimits,
    ResolutionOutcome, ResolvedModuleTarget, UnloadedModuleReason,
};

const LIMITS: ProgramLoadLimits = ProgramLoadLimits::new(128, 512, 32, 1 << 20, 1 << 22);

const APP_CONFIG: &str = r#"{"compilerOptions":{"module":"esnext","moduleResolution":"bundler","target":"es2022","noLib":true,"types":[],"strict":true,"noEmit":true},"files":["src/main.ts"],"references":[{"path":"../lib"},{"path":"../missing"}]}"#;

/// `lib` is a composite project with `outDir` `dist`; `app` references it
/// and imports one of its sources.
fn host(built: bool) -> MemoryCompilerHost {
    let mut builder = MemoryCompilerHost::builder("/work")
        .file(
            "/work/lib/tsconfig.json",
            br#"{"compilerOptions":{"composite":true,"outDir":"dist","module":"esnext","moduleResolution":"bundler","target":"es2022","lib":["es2022"],"types":[]},"include":["src"],"references":[{"path":"../base"}]}"#.to_vec(),
        )
        .file(
            "/work/lib/src/index.ts",
            b"export function add(a: number, b: number): number { return a + b; }\n".to_vec(),
        )
        .file("/work/lib/src/types.d.ts", b"export type Pair = [number, number];\n".to_vec())
        .file(
            "/work/base/tsconfig.json",
            br#"{"compilerOptions":{"composite":true,"declarationDir":"types","module":"esnext","moduleResolution":"bundler","target":"es2022","lib":["es2022"],"types":[]},"files":["util.mts"]}"#.to_vec(),
        )
        .file("/work/base/util.mts", b"export const util = 1;\n".to_vec())
        .file("/work/app/tsconfig.json", APP_CONFIG.as_bytes().to_vec())
        .file(
            "/work/app/src/main.ts",
            b"import { add } from \"../../lib/src/index\";\nexport const s: string = add(1, 2);\n"
                .to_vec(),
        );
    if built {
        builder = builder.file(
            "/work/lib/dist/src/index.d.ts",
            b"export declare function add(a: number, b: number): number;\n".to_vec(),
        );
    }
    builder.build().expect("build project reference host")
}

fn canonical(path: &str) -> CanonicalPath {
    CanonicalPath::from_js_normalized(path.into()).expect("canonical test path")
}

#[test]
fn the_config_file_name_of_a_reference_is_its_path_or_its_tsconfig() {
    // tsgo core.ResolveConfigFileNameOfProjectReference.
    assert_eq!(
        resolve_config_file_name_of_project_reference("/work/lib".into()),
        "/work/lib/tsconfig.json"
    );
    assert_eq!(
        resolve_config_file_name_of_project_reference("/work/lib/tsconfig.build.json".into()),
        "/work/lib/tsconfig.build.json"
    );
}

#[test]
fn referenced_projects_map_their_sources_to_their_outputs() {
    // tsoptions ParseInputOutputNames over every project reachable through
    // `references` (the root's and the referenced projects' own): a `.ts`
    // source maps to its declaration file under `outDir` (or
    // `declarationDir`) by its path relative to the project's common source
    // directory (the config's directory for a composite project), a
    // declaration file maps to no output, and a reference whose config does
    // not exist resolves to no project.
    let host = host(true);
    let adapter = CompilerConfigHost::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        ConfigRootPlanRequest {
            file_name: "/work/app/tsconfig.json".to_owned().into(),
            text: APP_CONFIG.to_owned(),
            base_path: "/work".to_owned().into(),
        },
    )
    .expect("parse app config");
    let references =
        resolve_project_references(&adapter, &plan, "/work".into()).expect("resolve references");

    let entries = references.root_references();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].config_file_name(), "/work/lib/tsconfig.json");
    let lib = entries[0].project().expect("lib is parsed");
    assert_eq!(lib.config_file_name(), "/work/lib/tsconfig.json");
    assert_eq!(lib.compiler_options().composite, Some(true));
    assert_eq!(
        lib.build_info_file_name(),
        Some("/work/lib/dist/tsconfig.tsbuildinfo".into())
    );
    assert_eq!(entries[1].config_file_name(), "/work/missing/tsconfig.json");
    assert!(entries[1].project().is_none());

    let nested = references.references_in_config(lib.canonical());
    assert_eq!(nested.len(), 1);
    let base = nested[0].project().expect("base is parsed through lib");
    assert_eq!(base.config_file_name(), "/work/base/tsconfig.json");
    assert_eq!(
        base.build_info_file_name(),
        Some("/work/base/tsconfig.tsbuildinfo".into())
    );

    let output = references
        .output_for_source(&canonical("/work/lib/src/index.ts"))
        .expect("lib's source maps to its output");
    assert_eq!(output.source(), "/work/lib/src/index.ts");
    assert_eq!(
        output.output_dts(),
        Some("/work/lib/dist/src/index.d.ts".into())
    );
    assert!(std::sync::Arc::ptr_eq(output.project(), lib));
    assert!(std::sync::Arc::ptr_eq(
        references
            .source_for_output(&canonical("/work/lib/dist/src/index.d.ts"))
            .expect("the output maps back")
            .project(),
        lib
    ));
    let declaration = references
        .output_for_source(&canonical("/work/lib/src/types.d.ts"))
        .expect("a declaration source is mapped");
    assert_eq!(declaration.output_dts(), None);
    let util = references
        .output_for_source(&canonical("/work/base/util.mts"))
        .expect("a nested project's source is mapped");
    assert_eq!(
        util.output_dts(),
        Some("/work/base/types/util.d.mts".into())
    );
    assert_eq!(references.projects().count(), 2);
}

#[test]
fn a_referenced_source_resolves_to_its_output_or_reports_it_unbuilt() {
    // tsgo getParseFileRedirect for the command: the import of a referenced
    // project's source loads the output declaration file (the resolution
    // keeps the source as its file name); without the output nothing is
    // loaded and the resolution records it (the checker's TS6305). The
    // missing reference is TS6053 among the program diagnostics.
    for built in [true, false] {
        let host = host(built);
        let adapter = CompilerConfigHost::new(&host);
        let plan = parse_config_root_plan(
            &adapter,
            ConfigRootPlanRequest {
                file_name: "/work/app/tsconfig.json".to_owned().into(),
                text: APP_CONFIG.to_owned(),
                base_path: "/work".to_owned().into(),
            },
        )
        .expect("parse app config");
        let prepared = load_config_program_with_no_emit_override(
            &host,
            &plan,
            &LibraryCatalog::typescript_7_1(PathBuf::from("/work/lib-files")),
            LIMITS,
        )
        .expect("load the app");
        assert!(prepared.compiler_options().no_emit == Some(true));
        let (_, resolution) = prepared
            .resolutions()
            .modules()
            .find(|(key, _)| key.specifier() == "../../lib/src/index")
            .expect("main.ts's import is resolved");
        let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
            panic!("the import resolves to lib's source: {resolution:?}");
        };
        match module.target() {
            ResolvedModuleTarget::Source {
                source,
                resolved_file,
            } => {
                assert!(built, "an unbuilt output loads nothing");
                assert_eq!(resolved_file.display(), "/work/lib/src/index.ts");
                let loaded = prepared
                    .source_file(*source)
                    .expect("the target is a source");
                assert_eq!(loaded.path().display(), "/work/lib/dist/src/index.d.ts");
                assert_eq!(
                    loaded
                        .project_reference_source_paths()
                        .iter()
                        .map(|path| path.display().to_owned())
                        .collect::<Vec<_>>(),
                    ["/work/lib/src/index.ts"]
                );
            }
            ResolvedModuleTarget::Unloaded {
                resolved_file,
                reason,
            } => {
                assert!(!built, "a built output is loaded");
                assert_eq!(resolved_file.display(), "/work/lib/src/index.ts");
                assert_eq!(
                    *reason,
                    UnloadedModuleReason::ProjectReferenceOutputNotBuilt
                );
            }
        }
        assert!(prepared
            .source_files()
            .iter()
            .all(|source| source.path().display() != "/work/lib/src/index.ts"));
        assert_eq!(
            prepared
                .diagnostics()
                .program()
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [6053]
        );
        // The referenced configs are auxiliary files: a diagnostic located
        // in one of them renders with its text.
        assert!(prepared
            .auxiliary_files()
            .any(|file| file.path().display() == "/work/lib/tsconfig.json"));
    }
}
