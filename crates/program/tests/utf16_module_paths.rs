use tsc_diagnostics::{JsStr, JsString};
use tsc_host::{HostError, HostErrorKind, HostOperation, MemoryCompilerHost};
use tsc_program::{
    CompilerOptions, ModuleExtension, ModuleResolver, ResolutionMode, ResolutionOutcome,
};
use tsc_types::ModuleSuffix;

fn named(prefix: &str, unit: u16, suffix: &str) -> JsString {
    let mut value = JsString::from(prefix);
    value.push_code_unit(unit);
    value.push_str(suffix);
    value
}

fn append(parent: JsStr<'_>, suffix: &str) -> JsString {
    let mut value = parent.to_owned();
    value.push_str(suffix);
    value
}

#[test]
fn package_queries_and_manifest_cache_keep_distinct_js_names() {
    for sensitive in [true, false] {
        let cwd = named("/Work/", 0xdc00, "");
        let containing = append(cwd.as_js(), "/main.ts");
        let mut builder = MemoryCompilerHost::builder_js(cwd.clone()).case_sensitive(sensitive);
        let names = [0xd800, 0xd801, 0xfffd].map(|unit| named("pkg", unit, ""));
        let mut expected = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let mut root = append(cwd.as_js(), "/node_modules/");
            root.push_js(name.as_js());
            let manifest = append(root.as_js(), "/package.json");
            let file = append(root.as_js(), "/index.d.ts");
            let unit = [0xd800, 0xd801, 0xfffd][index];
            let text = format!(
                r#"{{"name":"pkg\u{unit:04x}","version":"1.0.0","exports":"./index.d.ts"}}"#
            );
            builder = builder
                .file_js(manifest.clone(), text.into_bytes())
                .file_js(file.clone(), b"export declare const value: 1;".to_vec());
            expected.push((manifest, file));
        }
        let host = builder.build().unwrap();
        let options = CompilerOptions {
            module_resolution: Some(99),
            ..Default::default()
        };
        let mut resolver = ModuleResolver::new(&host, &options).unwrap();
        for index in [0, 1, 2, 2, 0, 1] {
            let ResolutionOutcome::Resolved(module) = resolver
                .resolve(&containing, &names[index], ResolutionMode::EsNext)
                .unwrap()
            else {
                panic!("missing package {index}")
            };
            assert_eq!(module.resolved_file().display(), expected[index].1.as_js());
            assert_eq!(module.extension(), &ModuleExtension::Dts);
            assert_eq!(module.package_id().unwrap().name(), names[index].as_js());
            assert_eq!(
                module.package_metadata().unwrap().package_json().display(),
                expected[index].0.as_js()
            );
        }
        assert_eq!(resolver.observed_package_metadata().count(), 3);
    }
}

#[test]
fn suffix_candidate_and_realpath_keep_their_separate_js_spellings() {
    let suffix = named(".", 0xd800, "");
    let lexical = named("/work/entry.", 0xd800, ".d.ts");
    let physical = named("/physical/", 0xd801, ".d.ts");
    let host = MemoryCompilerHost::builder("/work")
        .file_js(lexical.clone(), b"export const value = 1;".to_vec())
        .file_js(physical.clone(), b"export const value = 2;".to_vec())
        .realpath_js(lexical.clone(), physical.clone())
        .build()
        .unwrap();
    let options = CompilerOptions {
        module_resolution: Some(99),
        module_suffixes: Some(vec![ModuleSuffix::value(suffix)]),
        ..Default::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).unwrap();
    // Type-reference resolution follows realpath for local file candidates.
    let ResolutionOutcome::Resolved(module) = resolver
        .resolve_type_reference(
            "/work/main.ts",
            "./entry",
            ResolutionMode::Unspecified,
            None,
        )
        .unwrap()
    else {
        panic!("missing suffixed type reference")
    };
    assert_eq!(module.resolved_file().display(), physical.as_js());
    assert_eq!(module.original_path().unwrap().display(), lexical.as_js());
}

#[test]
fn arbitrary_extension_and_typed_host_failure_preserve_the_query_units() {
    let request = named("./style.", 0xd800, "");
    let file = named("/work/style.d.", 0xd800, ".ts");
    let extension = named(".d.", 0xd800, ".ts");
    let host = MemoryCompilerHost::builder("/work")
        .file_js(file.clone(), b"export declare const css: string;".to_vec())
        .build()
        .unwrap();
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..Default::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).unwrap();
    let ResolutionOutcome::Resolved(module) = resolver
        .resolve("/work/main.ts", &request, ResolutionMode::Unspecified)
        .unwrap()
    else {
        panic!("missing arbitrary-extension declaration")
    };
    assert_eq!(module.resolved_file().display(), file.as_js());
    assert_eq!(module.extension().as_js(), extension.as_js());

    let failure = HostError::new_js(
        HostErrorKind::PermissionDenied,
        HostOperation::FileExists,
        Some(file.as_js()),
        "query denied",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file_js(file.clone(), Vec::new())
        .failure(failure.clone())
        .build()
        .unwrap();
    let mut resolver = ModuleResolver::new(&host, &options).unwrap();
    let error = resolver
        .resolve("/work/main.ts", &request, ResolutionMode::Unspecified)
        .unwrap_err();
    assert_eq!(error.js_path(), Some(file.as_js()));
    assert_eq!(error, failure.into());
}
