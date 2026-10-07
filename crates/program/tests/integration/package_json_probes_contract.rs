//! The package.json files a load probes (`PreparedProgram::package_json_probes`,
//! the build info's `packageJsons`/`missingPackageJsons` of tsgo
//! execute/incremental/program.go ensurePackageJsonsForState): the resolver's
//! package.json cache entries, and nothing the dependency symlink walk of
//! program.go GetSymlinkCache tests (ResolvePackageDirectory looks for
//! package directories without reading a package.json).

use std::path::PathBuf;

use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_config_program_with_no_emit_override, parse_config_root_plan, CompilerConfigHost,
    ConfigRootPlanRequest, LibraryCatalog, ProgramLoadLimits,
};

const LIMITS: ProgramLoadLimits = ProgramLoadLimits::new(128, 512, 32, 1 << 20, 1 << 22);

const CONFIG: &str = r#"{"compilerOptions":{"module":"esnext","moduleResolution":"bundler","target":"es2022","noLib":true,"types":[],"noEmit":true,"incremental":true},"files":["src/main.ts"]}"#;

#[test]
fn the_probes_are_the_resolved_package_jsons_without_the_dependency_walk() {
    // tsgo on this layout (scratchpad fixture p36b/fx5/probes, `--noEmit`):
    // packageJsons ["./node_modules/dep/package.json", "./package.json"],
    // no missingPackageJsons — the runtime dependencies `absent` and
    // `@scope/peer` of the package scope are never looked up as
    // package.json files.
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/tsconfig.json", CONFIG.as_bytes().to_vec())
        .file(
            "/work/package.json",
            br#"{"name":"work","dependencies":{"dep":"*","absent":"*"},"peerDependencies":{"@scope/peer":"*"}}"#.to_vec(),
        )
        .file(
            "/work/node_modules/dep/package.json",
            br#"{"name":"dep","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/dep/index.d.ts",
            b"export declare const dep: number;\n".to_vec(),
        )
        .file(
            "/work/src/main.ts",
            b"import { dep } from \"dep\";\nexport const value: number = dep;\n".to_vec(),
        )
        .build()
        .expect("build the probe host");
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

    let mut found = prepared
        .package_json_probes()
        .iter()
        .filter(|probe| probe.exists)
        .map(|probe| probe.path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    found.sort();
    assert_eq!(
        found,
        ["/work/node_modules/dep/package.json", "/work/package.json"]
    );
    let missing_under_node_modules = prepared
        .package_json_probes()
        .iter()
        .filter(|probe| !probe.exists && probe.path.as_js().contains("/node_modules/"))
        .map(|probe| probe.path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(
        missing_under_node_modules.is_empty(),
        "{missing_under_node_modules:?}"
    );
    assert!(prepared.package_json_probes().iter().all(|probe| {
        !probe.path.as_js().contains("absent") && !probe.path.as_js().contains("@scope")
    }));
}

#[test]
fn a_type_reference_probes_the_candidate_of_every_existing_type_root() {
    // tsgo on this layout (scratchpad fixture p36b/fx5/typeroot): packageJsons
    // ["./node_modules/@types/other/package.json"], missingPackageJsons
    // ["./node_modules/@types/absent/package.json",
    // "./node_modules/absent/package.json"] — the primary lookup asks the
    // package.json cache for `<typeRoot>/absent` although that directory
    // does not exist (loadNodeModuleFromDirectory → getPackageJsonInfo), and
    // the secondary lookup for `node_modules/absent`.
    const CONFIG: &str = r#"{"compilerOptions":{"module":"esnext","moduleResolution":"bundler","target":"es2022","noLib":true,"types":[],"noEmit":true,"incremental":true},"files":["src/main.ts"]}"#;
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/tsconfig.json", CONFIG.as_bytes().to_vec())
        .file(
            "/work/node_modules/@types/other/package.json",
            br#"{"name":"@types/other","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@types/other/index.d.ts",
            b"declare const other: number;\n".to_vec(),
        )
        .file(
            "/work/src/main.ts",
            b"/// <reference types=\"absent\" />\n/// <reference types=\"other\" />\nexport const value = 1;\n".to_vec(),
        )
        .build()
        .expect("build the type-root host");
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

    let mut found = prepared
        .package_json_probes()
        .iter()
        .filter(|probe| probe.exists)
        .map(|probe| probe.path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    found.sort();
    assert_eq!(found, ["/work/node_modules/@types/other/package.json"]);
    let mut missing = prepared
        .package_json_probes()
        .iter()
        .filter(|probe| !probe.exists && probe.path.as_js().contains("/node_modules/"))
        .map(|probe| probe.path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    missing.sort();
    assert_eq!(
        missing,
        [
            "/work/node_modules/@types/absent/package.json",
            "/work/node_modules/absent/package.json"
        ]
    );
}
