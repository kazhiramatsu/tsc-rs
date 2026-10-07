//! Package deduplication decided when the files are collected (tsgo
//! compiler/filesparser.go:293-295, 448-481): a path's package identity is
//! the first one any task reaching it carried, and the collect walk keeps
//! the first file with an identity in program order, redirecting later
//! copies and not walking their subtasks.

use std::path::PathBuf;

use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_config_program_with_no_emit_override, parse_config_root_plan, CompilerConfigHost,
    ConfigRootPlanRequest, LibraryCatalog, ProgramLoadLimits, ResolutionOutcome,
    ResolvedModuleTarget,
};

const LIMITS: ProgramLoadLimits = ProgramLoadLimits::new(128, 512, 32, 1 << 20, 1 << 22);

const CONFIG: &str = r#"{"compilerOptions":{"module":"esnext","moduleResolution":"bundler","target":"es2022","noLib":true,"types":[],"noEmit":true},"files":["a/src/a.ts","b/src/b.ts","a/src/a2.ts"]}"#;

/// Two copies of `pkg@1.0.0`. Copy A's `sub.d.ts` enters through copy A's
/// `index.d.ts` path reference (no package identity); copy B's `sub.d.ts`
/// enters through `b.ts`'s `pkg/sub` import with the identity
/// `pkg@1.0.0/sub.d.ts`; `a2.ts`'s `pkg/sub` import later reaches copy A's
/// `sub.d.ts` with the same identity.
fn host() -> MemoryCompilerHost {
    let mut builder = MemoryCompilerHost::builder("/work")
        .file("/work/tsconfig.json", CONFIG.as_bytes().to_vec());
    for copy in ["a", "b"] {
        builder = builder
            .file(
                format!("/work/{copy}/node_modules/pkg/package.json"),
                br#"{"name":"pkg","version":"1.0.0"}"#.to_vec(),
            )
            .file(
                format!("/work/{copy}/node_modules/pkg/index.d.ts"),
                b"/// <reference path=\"./sub.d.ts\" />\nexport declare const main: number;\n"
                    .to_vec(),
            )
            .file(
                format!("/work/{copy}/node_modules/pkg/sub.d.ts"),
                b"export declare const sub: number;\n".to_vec(),
            );
    }
    builder
        .file(
            "/work/a/src/a.ts",
            b"import { main } from \"pkg\";\nexport const a = main;\n".to_vec(),
        )
        .file(
            "/work/b/src/b.ts",
            b"import { sub } from \"pkg/sub\";\nexport const b = sub;\n".to_vec(),
        )
        .file(
            "/work/a/src/a2.ts",
            b"import { sub } from \"pkg/sub\";\nexport const a2 = sub;\n".to_vec(),
        )
        .build()
        .expect("build the dedupe host")
}

#[test]
fn a_copy_first_reached_without_an_identity_still_owns_it_at_collect_time() {
    // tsgo on this layout (scratchpad fixture p36b/fx6/collect, `--noEmit
    // --incremental`): fileNames ["./a/node_modules/pkg/sub.d.ts",
    // "./a/node_modules/pkg/index.d.ts", "./a/src/a.ts", "./b/src/b.ts",
    // "./a/src/a2.ts"], referencedMap [[2,1],[3,2],[5,1],[4,1]]: copy B's
    // `sub.d.ts` is a redirect to copy A's and `b.ts` resolves to it.
    let host = host();
    let adapter = CompilerConfigHost::new(&host);
    let plan = parse_config_root_plan(
        &adapter,
        ConfigRootPlanRequest {
            file_name: "/work/tsconfig.json".to_owned().into(),
            text: CONFIG.to_owned(),
            base_path: "/work".to_owned().into(),
        },
    )
    .expect("parse the config");
    let prepared = load_config_program_with_no_emit_override(
        &host,
        &plan,
        &LibraryCatalog::typescript_7_1(PathBuf::from("/work/lib-files")),
        LIMITS,
    )
    .expect("load the program");
    assert_eq!(
        prepared
            .source_files()
            .iter()
            .map(|source| source.path().display().to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        [
            "/work/a/node_modules/pkg/sub.d.ts",
            "/work/a/node_modules/pkg/index.d.ts",
            "/work/a/src/a.ts",
            "/work/b/src/b.ts",
            "/work/a/src/a2.ts",
        ]
    );
    let owner = &prepared.source_files()[0];
    assert_eq!(
        owner
            .package_redirect_paths()
            .iter()
            .map(|path| path.display().to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["/work/b/node_modules/pkg/sub.d.ts"]
    );
    let (_, resolution) = prepared
        .resolutions()
        .modules()
        .find(|(key, _)| {
            key.specifier() == "pkg/sub" && key.source().as_js().contains("/b/src/b.ts")
        })
        .expect("b.ts's import is resolved");
    let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
        panic!("b.ts's import resolves: {resolution:?}");
    };
    let ResolvedModuleTarget::Source { source, .. } = module.target() else {
        panic!("b.ts's import loads a source: {resolution:?}");
    };
    assert_eq!(
        prepared
            .source_file(*source)
            .expect("the target is a source")
            .path()
            .display(),
        "/work/a/node_modules/pkg/sub.d.ts"
    );
}
