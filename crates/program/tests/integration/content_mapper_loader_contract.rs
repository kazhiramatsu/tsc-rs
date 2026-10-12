//! Content-mapped files in a program (tsgo compiler/fileloader.go
//! `parseContentMappedFile`, contentmapper_test.go): a file of a mapper's
//! extension is its mapper's virtual output, its supplemental outputs join
//! the program after it, and a transform that cannot be used leaves an
//! empty stub with a diagnostic, within the mapper's failure budget.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tsc_diagnostics::spanmap::SpanMap;
use tsc_diagnostics::{JsStr, JsString};
use tsc_host::MemoryCompilerHost;
use tsc_program::content_mapper::errors::{
    InitializeError, InitializeErrorKind, ProjectFailure, TransformFailure,
};
use tsc_program::content_mapper::host::{OptionDiagnostic, Request};
use tsc_program::content_mapper::protocol::{MappedResult, TransformOutput};
use tsc_program::content_mapper::{Mapper, Project};
use tsc_program::{
    command_line_option_bag, load_config_program_with_no_emit_override,
    parse_config_root_plan_with_command_line, CompilerConfigHost, ConfigExtendedCache,
    ConfigRootPlanRequest, ContentMapperProjectHandle, JsonValue, LibraryCatalog, PreparedProgram,
    ProgramLoadLimits, ResolutionMode, SourceInclusionReason,
};

const LIMITS: ProgramLoadLimits = ProgramLoadLimits::new(128, 512, 32, 1 << 20, 1 << 22);

type Transform =
    dyn Fn(&str, &str) -> Result<TransformOutput, TransformFailure> + Send + Sync + 'static;

/// tsgo `fakeContentMapperHost`: every mapper's identity is `test`.
struct FakeProject {
    transform: Box<Transform>,
    calls: Mutex<Vec<String>>,
}

impl Project for FakeProject {
    fn refresh(&self) -> Result<(), String> {
        Ok(())
    }

    fn identities(&self) -> Result<Vec<String>, ProjectFailure> {
        Ok(Vec::new())
    }

    fn identity(&self, _mapper: &Arc<Mapper>) -> Result<String, ProjectFailure> {
        Ok("test".to_owned())
    }

    fn watched_files(&self) -> Result<Vec<String>, ProjectFailure> {
        Ok(Vec::new())
    }

    fn diagnostics(&self) -> Vec<OptionDiagnostic> {
        Vec::new()
    }

    fn transform(
        &self,
        _mapper: &Arc<Mapper>,
        request: Request<'_>,
    ) -> Result<TransformOutput, TransformFailure> {
        self.calls
            .lock()
            .unwrap()
            .push(request.file_name.to_owned());
        (self.transform)(request.file_name, request.content)
    }

    fn close(&self) -> Result<(), String> {
        Ok(())
    }
}

fn result(text: &str, extension: &str) -> MappedResult {
    MappedResult {
        text: text.to_owned(),
        virtual_extension: extension.to_owned(),
        mappings: SpanMap::new(Vec::new()),
        diagnostic_directives: Vec::new(),
    }
}

fn output(text: &str, extension: &str, supplemental: Vec<MappedResult>) -> TransformOutput {
    TransformOutput {
        canonical: result(text, extension),
        diagnostics: Vec::new(),
        supplemental,
    }
}

const CONFIG: &str = r#"{
    "compilerOptions": { "noLib": true, "types": [], "module": "nodenext" },
    "contentMappers": [{ "package": "vue", "extensions": [".vue"] }]
}"#;

const PACKAGE: &str = r#"{ "name": "vue-mapper", "version": "1.0.0", "typescript": { "contentMapper": { "exec": ["vue-mapper"] } } }"#;

fn load(
    files: &[(&str, &str)],
    project: Option<Arc<FakeProject>>,
) -> (PreparedProgram, Option<Arc<FakeProject>>) {
    load_with(CONFIG, files, project)
}

fn load_with(
    config: &str,
    files: &[(&str, &str)],
    project: Option<Arc<FakeProject>>,
) -> (PreparedProgram, Option<Arc<FakeProject>>) {
    let host = files
        .iter()
        .fold(
            MemoryCompilerHost::builder("/p")
                .case_sensitive(true)
                .file("/p/tsconfig.json", config.as_bytes())
                .file("/p/node_modules/vue/package.json", PACKAGE.as_bytes()),
            |builder, (path, text)| builder.file(path, text.as_bytes()),
        )
        .build()
        .unwrap();
    let options = command_line_option_bag(
        &[("runExternalCode".to_owned(), JsonValue::Bool(true))],
        JsStr::from("/p"),
    );
    let mut plan = parse_config_root_plan_with_command_line(
        &CompilerConfigHost::new(&host),
        ConfigRootPlanRequest {
            file_name: JsString::from("/p/tsconfig.json"),
            text: config.to_owned(),
            base_path: JsString::from("/p"),
        },
        &options,
        &mut ConfigExtendedCache::default(),
    )
    .unwrap();
    assert!(plan.errors().is_empty(), "{:?}", plan.errors());
    if let Some(project) = &project {
        plan = plan.with_content_mapper_project(ContentMapperProjectHandle(
            Arc::clone(project) as Arc<dyn Project>
        ));
    }
    let prepared = load_config_program_with_no_emit_override(
        &host,
        &plan,
        &LibraryCatalog::typescript_7_1(PathBuf::from("/p/lib")),
        LIMITS,
    )
    .unwrap();
    (prepared, project)
}

fn fake(
    transform: impl Fn(&str, &str) -> Result<TransformOutput, TransformFailure> + Send + Sync + 'static,
) -> Option<Arc<FakeProject>> {
    Some(Arc::new(FakeProject {
        transform: Box::new(transform),
        calls: Mutex::new(Vec::new()),
    }))
}

fn names(prepared: &PreparedProgram) -> Vec<String> {
    prepared
        .source_files()
        .iter()
        .map(|source| source.path().display().to_string_lossy().into_owned())
        .collect()
}

/// tsgo collects a task's subtasks before its file, so a supplemental
/// output precedes its canonical file in the program.
#[test]
fn a_mapped_file_is_its_virtual_output_with_its_supplemental_outputs_as_subtasks() {
    let (prepared, _) = load(
        &[("/p/src/Component.vue", "<template />")],
        fake(|_, _| {
            Ok(output(
                "export const a = 1;",
                ".ts",
                vec![result("export const b = 2;", ".mts")],
            ))
        }),
    );
    assert_eq!(
        names(&prepared),
        ["/p/src/Component.vue.0.mts", "/p/src/Component.vue"]
    );
    let canonical = &prepared.source_files()[1];
    assert_eq!(canonical.snapshot().text(), "export const a = 1;");
    let info = canonical.content_mapped().unwrap();
    assert_eq!(info.content_mapper, "vue-mapper@1.0.0");
    assert_eq!(info.transform_identity, "test");
    assert_eq!(&*info.original_text, "<template />");
    assert_eq!(info.virtual_file_name, "/p/src/Component.vue.ts");
    assert_eq!(info.supplemental, ["/p/src/Component.vue.0.mts"]);
    assert!(!info.is_failure_stub());

    let supplemental = &prepared.source_files()[0];
    assert_eq!(supplemental.snapshot().text(), "export const b = 2;");
    assert_eq!(
        supplemental
            .content_mapped()
            .and_then(|info| info.canonical.as_ref())
            .map(JsString::as_js),
        Some(JsStr::from("/p/src/Component.vue"))
    );
    assert_eq!(
        supplemental.inclusion_reasons(),
        [SourceInclusionReason::ContentMapperSupplemental {
            canonical: JsString::from("/p/src/Component.vue")
        }]
    );
    // tsgo TestContentMapperVirtualExtensionSetsImpliedNodeFormat: the
    // virtual `.mts` output is an ES module whatever the file's name.
    assert_eq!(
        supplemental.implied_node_format(),
        Some(ResolutionMode::EsNext)
    );
}

#[test]
fn a_failed_transform_leaves_a_stub_with_its_diagnostic() {
    let (prepared, _) = load(
        &[("/p/src/fail.vue", "original")],
        fake(|_, _| Err(TransformFailure::Request("failed".to_owned()))),
    );
    let [file] = prepared.source_files() else {
        panic!("one file: {:?}", names(&prepared));
    };
    assert_eq!(file.snapshot().text(), "");
    let info = file.content_mapped().unwrap();
    assert!(info.is_failure_stub());
    assert_eq!(&*info.original_text, "original");
    assert_eq!(info.content_mapper, "vue-mapper@1.0.0");
    let [diagnostic] = info.diagnostics.as_slice() else {
        panic!("one diagnostic: {:?}", info.diagnostics);
    };
    assert_eq!(
        diagnostic.message_text(),
        "The content mapper 'vue-mapper' failed to transform this file."
    );
    assert!(prepared.diagnostics().content_mapper().is_empty());
}

#[test]
fn a_mapper_failing_five_times_is_not_used_again() {
    let files: Vec<(String, &str)> = (0..7)
        .map(|index| (format!("/p/src/c{index}.vue"), "x"))
        .collect();
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(name, text)| (name.as_str(), *text))
        .collect();
    let (prepared, project) = load(
        &files,
        fake(|_, _| Err(TransformFailure::Request("failed".to_owned()))),
    );
    let reported = prepared
        .source_files()
        .iter()
        .filter(|file| !file.content_mapped().unwrap().diagnostics.is_empty())
        .count();
    assert_eq!(reported, 5);
    assert_eq!(project.unwrap().calls.lock().unwrap().len(), 5);
    let texts: Vec<_> = prepared
        .diagnostics()
        .content_mapper()
        .iter()
        .map(|diagnostic| diagnostic.message_text().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        texts,
        ["The content mapper 'vue-mapper' failed 5 times and will not be used."]
    );
}

#[test]
fn a_mapper_that_cannot_start_is_reported_once_without_a_file() {
    let (prepared, project) = load(
        &[("/p/src/a.vue", "x"), ("/p/src/b.vue", "y")],
        fake(|_, _| {
            Err(TransformFailure::Initialize(InitializeError {
                kind: InitializeErrorKind::ProcessExit { exit_code: 3 },
                mapper_name: "vue-mapper".to_owned(),
            }))
        }),
    );
    assert_eq!(project.unwrap().calls.lock().unwrap().len(), 1);
    assert!(prepared.source_files().iter().all(|file| file
        .content_mapped()
        .unwrap()
        .diagnostics
        .is_empty()
        && file.content_mapped().unwrap().is_failure_stub()));
    let [diagnostic] = prepared.diagnostics().content_mapper() else {
        panic!("one diagnostic");
    };
    assert_eq!(diagnostic.file_name, None);
    assert_eq!(
        diagnostic.message_text(),
        "The content mapper 'vue-mapper' could not be initialized."
    );
}

#[test]
fn without_a_project_each_file_reports_the_failure_with_the_mapper_s_own_identity() {
    let (prepared, _) = load(&[("/p/src/a.vue", "x")], None);
    let [file] = prepared.source_files() else {
        panic!("one file");
    };
    let info = file.content_mapped().unwrap();
    assert!(info.is_failure_stub());
    // tsgo getContentMapperTransformIdentity without a project: the
    // mapper's own transform identity in hexadecimal.
    assert_eq!(info.transform_identity.len(), 32);
    assert_eq!(
        info.diagnostics[0].message_text(),
        "The content mapper 'vue-mapper' failed to transform this file."
    );
}

/// tsgo GetResolutionDiagnostic returns nothing for a file found by a
/// mapper's extension (`ResolvedUsingExtraExtensions`), so an import of it
/// loads the mapped file without `allowArbitraryExtensions` (Astro's
/// `components/index.ts` re-exports its `.astro` components).
#[test]
fn an_import_of_a_mapper_s_extension_loads_the_mapped_file() {
    let (prepared, _) = load_with(
        r#"{
            "compilerOptions": { "noLib": true, "types": [], "module": "esnext", "moduleResolution": "bundler" },
            "files": ["src/main.ts"],
            "contentMappers": [{ "package": "vue", "extensions": [".vue"] }]
        }"#,
        &[
            (
                "/p/src/main.ts",
                "export { default } from \"./Component.vue\";",
            ),
            ("/p/src/Component.vue", "<template />"),
        ],
        fake(|_, _| Ok(output("export default 1;", ".ts", Vec::new()))),
    );
    assert_eq!(names(&prepared), ["/p/src/Component.vue", "/p/src/main.ts"]);
    assert!(matches!(
        prepared.source_files()[0].inclusion_reasons(),
        [SourceInclusionReason::Import { .. }]
    ));
}

/// tsgo resolveImportsAndModuleAugmentations adds the JSX runtime import by
/// the script kind, which a mapped file takes from its virtual name: a
/// `.tsx` output imports the runtime, explained as tsgo explains a
/// synthesized import.
#[test]
fn a_mapped_tsx_output_imports_the_jsx_runtime() {
    let (prepared, _) = load_with(
        r#"{
            "compilerOptions": { "noLib": true, "types": [], "module": "esnext", "moduleResolution": "bundler", "jsx": "react-jsx" },
            "files": ["src/App.vue"],
            "contentMappers": [{ "package": "vue", "extensions": [".vue"] }]
        }"#,
        &[
            ("/p/src/App.vue", "<template><div /></template>"),
            (
                "/p/node_modules/react/package.json",
                r#"{ "name": "react", "version": "18.0.0", "types": "index.d.ts" }"#,
            ),
            ("/p/node_modules/react/index.d.ts", "export {};"),
            (
                "/p/node_modules/react/jsx-runtime.d.ts",
                "export declare function jsx(...args: any[]): any;",
            ),
        ],
        fake(|_, _| Ok(output("export const el = <div />;", ".tsx", Vec::new()))),
    );
    assert_eq!(
        names(&prepared),
        ["/p/node_modules/react/jsx-runtime.d.ts", "/p/src/App.vue"]
    );
    assert_eq!(
        prepared.source_files()[0].inclusion_reasons(),
        [SourceInclusionReason::SyntheticImport {
            parent: JsString::from("/p/src/App.vue"),
            specifier: JsString::from("react/jsx-runtime"),
            import_helpers: false,
            package_id: Some(JsString::from("react/jsx-runtime.d.ts@18.0.0")),
        }]
    );
}
