use std::path::{Path, PathBuf};

use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_no_lib_program, plan_source_requests, CompilerOptions, ModuleExtension, ModuleResolution,
    ModuleSuffix, PathContext, PreparationErrorKind, PreparedProgram, PreparedSourceFile,
    ProgramLoadLimits, ProgramOptions, ProgramPath, ResolutionKey, ResolutionMode,
    ResolutionOutcome, ResolvedModule, ResolvedModuleTarget,
};

fn path(display: &str, canonical: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(display, canonical).expect("trusted program path")
}

fn compiler_options(module_suffix: ModuleSuffix) -> CompilerOptions {
    CompilerOptions {
        no_emit: Some(true),
        module: Some(1),
        module_resolution: Some(2),
        allow_arbitrary_extensions: Some(true),
        module_suffixes: Some(vec![module_suffix]),
        ..CompilerOptions::default()
    }
}

fn program_options() -> ProgramOptions {
    ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new())
}

#[test]
fn recursive_loader_accepts_suffixes_inside_nominal_arbitrary_extensions() {
    for (module_suffix, physical_path) in [
        (ModuleSuffix::value(".ios"), "/theme.d.css.ios.ts"),
        (ModuleSuffix::Undefined, "/theme.d.cssundefined.ts"),
    ] {
        let host = MemoryCompilerHost::builder("/")
            .file(
                "/index.ts",
                b"import theme from './theme.css'; theme;".to_vec(),
            )
            .file(
                physical_path,
                b"declare const theme: string; export default theme;".to_vec(),
            )
            .build()
            .expect("arbitrary-extension moduleSuffixes host");
        let program = load_no_lib_program(
            &host,
            &[PathBuf::from("/index.ts")],
            compiler_options(module_suffix),
            program_options(),
            ProgramLoadLimits::new(16, 16, 8, 1 << 20, 1 << 20),
        )
        .expect("load suffix-selected arbitrary declaration twin");

        assert_eq!(
            program
                .source_files()
                .iter()
                .map(|source| source.path().display().to_path_buf())
                .collect::<Vec<_>>(),
            [PathBuf::from(physical_path), PathBuf::from("/index.ts")]
        );
        let root = program
            .source_files()
            .iter()
            .find(|source| source.path().display() == Path::new("/index.ts"))
            .expect("root source is loaded");
        let key = plan_source_requests(root, program.compiler_options())
            .expect("re-plan root module requests")
            .module_requests()[0]
            .clone();
        let ResolutionOutcome::Resolved(module) = program
            .resolutions()
            .require_module(&key)
            .expect("arbitrary module resolution row")
            .outcome()
        else {
            panic!("arbitrary declaration twin must resolve");
        };
        assert_eq!(
            module.extension(),
            &ModuleExtension::Arbitrary(".d.css.ts".to_owned())
        );
    }
}

#[test]
fn prepared_validation_folds_inserted_suffixes_only_for_case_insensitive_hosts() {
    let add_resolution = |case_sensitive: bool| {
        let root_path = path(
            "/Work/index.ts",
            if case_sensitive {
                "/Work/index.ts"
            } else {
                "/work/index.ts"
            },
        );
        let target_path = path(
            "/Work/theme.d.css.IOS.ts",
            if case_sensitive {
                "/Work/theme.d.css.IOS.ts"
            } else {
                "/work/theme.d.css.ios.ts"
            },
        );
        let key = ResolutionKey::new(
            root_path.canonical().clone(),
            "./theme.CSS",
            ResolutionMode::Unspecified,
        );
        let mut builder = PreparedProgram::builder(
            PathContext::new(
                path("/Work", if case_sensitive { "/Work" } else { "/work" }),
                case_sensitive,
            ),
            compiler_options(ModuleSuffix::value(".ios")),
        );
        let root = builder
            .add_source_file(PreparedSourceFile::new(root_path, "import './theme.CSS';"))
            .expect("add root source");
        let target = builder
            .add_source_file(PreparedSourceFile::new(
                target_path.clone(),
                "declare const theme: string; export default theme;",
            ))
            .expect("add arbitrary target");
        builder.add_root_file(root).expect("add root membership");
        builder.add_module_resolution(
            key,
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: target,
                    resolved_file: target_path,
                },
                ModuleExtension::Arbitrary(".d.CSS.ts".to_owned()),
            ))),
        )
    };

    add_resolution(false).expect("case-insensitive validation folds extension and suffix");
    let error = add_resolution(true).expect_err("case-sensitive validation preserves spelling");
    assert_eq!(error.kind(), PreparationErrorKind::InvalidData);
}

#[test]
fn prepared_validation_accepts_a_suffix_that_forms_a_declaration_ending() {
    let root_path = path("/index.ts", "/index.ts");
    let target_path = path("/dep.d.ts", "/dep.d.ts");
    let key = ResolutionKey::new(
        root_path.canonical().clone(),
        "./dep",
        ResolutionMode::Unspecified,
    );
    let mut builder = PreparedProgram::builder(
        PathContext::new(path("/", "/"), true),
        compiler_options(ModuleSuffix::value(".d")),
    );
    let root = builder
        .add_source_file(PreparedSourceFile::new(root_path, "import './dep';"))
        .expect("add root source");
    let target = builder
        .add_source_file(PreparedSourceFile::new(
            target_path.clone(),
            "export const value = 1;",
        ))
        .expect("add suffix-selected source");
    builder.add_root_file(root).expect("add root membership");
    builder
        .add_module_resolution(
            key,
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: target,
                    resolved_file: target_path,
                },
                ModuleExtension::Ts,
            ))),
        )
        .expect("the physical .d.ts ending came from suffix insertion before logical .ts");
}
