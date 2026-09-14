use tsc_diagnostics::JsString;
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    parse_config_root_plan_with_cache, CompilerConfigHost, ConfigExtendedCache,
    ConfigRootPlanRequest,
};

fn path(prefix: &str, unit: u16, suffix: &str) -> JsString {
    let mut path = JsString::from(prefix);
    path.push_code_unit(unit);
    path.push_str(suffix);
    path
}

#[test]
fn extends_cache_and_inherited_option_origins_keep_distinct_js_directories() {
    let first = path("/work/", 0xd800, "/base.json");
    let second = path("/work/", 0xd801, "/base.json");
    let source = path("/work/", 0xfffd, "/child.ts");
    let first_text = r#"{"files":["a.ts"],"compilerOptions":{"paths":{"first":["./a"]}}}"#;
    let second_text =
        r#"{"files":["b.ts"],"compilerOptions":{"paths":{"second":["./b"]},"outDir":"./dist"}}"#;
    let host = MemoryCompilerHost::builder("/work")
        .file_js(first.clone(), first_text.as_bytes().to_vec())
        .file_js(second.clone(), second_text.as_bytes().to_vec())
        .file_js(source, Vec::new())
        .build()
        .unwrap();
    let config_host = CompilerConfigHost::new(&host);
    let mut cache = ConfigExtendedCache::default();
    for _ in 0..2 {
        let plan = parse_config_root_plan_with_cache(
            &config_host,
            ConfigRootPlanRequest {
                file_name: "/work/tsconfig.json".into(),
                text: r#"{"extends":["./\ud800/base.json","./\ud801/base.json"]}"#.into(),
                base_path: "/work".into(),
            },
            &mut cache,
        )
        .unwrap();
        assert_eq!(
            plan.extended_source_files(),
            &[first.clone(), second.clone()]
        );
        assert_eq!(plan.extended_sources()[0].file_name, first);
        assert_eq!(plan.extended_sources()[1].file_name, second);
        assert_eq!(plan.file_names(), &[path("/work/", 0xd801, "/b.ts")]);
        assert_eq!(
            plan.options().stored_paths_base_path(),
            Some(path("/work/", 0xd801, "").as_js())
        );
        assert_eq!(
            plan.compiler_options().out_dir,
            Some(path("/work/", 0xd801, "/dist"))
        );
        assert_eq!(
            plan.program_options().paths_base_path(),
            Some(path("/work/", 0xd801, "").as_js())
        );
    }
}

#[test]
fn circular_extends_diagnostic_keeps_each_js_filename() {
    let file = path("/work/", 0xd800, ".json");
    let contents = r#"{"extends":"./\ud800.json","files":[]}"#;
    let host = MemoryCompilerHost::builder("/work")
        .file_js(file.clone(), contents.as_bytes().to_vec())
        .build()
        .unwrap();
    let mut cache = ConfigExtendedCache::default();
    let plan = parse_config_root_plan_with_cache(
        &CompilerConfigHost::new(&host),
        ConfigRootPlanRequest {
            file_name: file.clone(),
            text: contents.into(),
            base_path: "/work".into(),
        },
        &mut cache,
    )
    .unwrap();
    let cycle = plan
        .errors()
        .iter()
        .find(|diagnostic| diagnostic.code() == 18000)
        .unwrap();
    let message = cycle.message_text();
    assert!(message.code_units().any(|unit| unit == 0xd800));
    assert!(!message.code_units().any(|unit| unit == 0xfffd));
}
