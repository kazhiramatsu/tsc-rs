use super::*;

fn settings(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn directives_are_read_from_the_whole_file_lower_cased_and_trimmed() {
    let content = "// @Target: ES2015;\r\n// @filename: a.ts\nlet a;\n//@strict:true\n// @target:  esnext  \n";
    assert_eq!(
        extract_settings(content),
        settings(&[
            ("filename", "a.ts"),
            ("strict", "true"),
            ("target", "esnext")
        ])
    );
}

#[test]
fn a_directive_value_can_swallow_the_next_line_like_go_findall() {
    // `\s*` after the colon crosses the line break, so the second line is
    // the first directive's value and is not matched again.
    let content = "// @a:\n// @b: 2\n";
    assert_eq!(extract_settings(content), settings(&[("a", "// @b: 2")]));
}

#[test]
fn variations_deduplicate_by_value_and_keep_the_first_spelling() {
    let configurations = configurations(&settings(&[("target", "es6, es2015, esnext")])).unwrap();
    let names: Vec<_> = configurations.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["target=es6", "target=esnext"]);
}

#[test]
fn star_and_exclusions_expand_enum_and_boolean_options() {
    let strict = configurations(&settings(&[("strict", "*, -true")])).unwrap();
    assert_eq!(strict.len(), 1);
    assert_eq!(strict[0].name, "");
    assert_eq!(strict[0].settings["strict"], "false");

    let modules = configurations(&settings(&[("module", "*, -amd, -system, -umd")])).unwrap();
    let mut names: Vec<_> = modules.iter().map(|c| c.name.clone()).collect();
    names.sort();
    assert_eq!(names.len(), 10);
    assert!(names.contains(&"module=es6".to_owned()));
    assert!(!names.contains(&"module=es2015".to_owned()));
    // An excluded value the option no longer knows is ignored.
    let targets = configurations(&settings(&[("target", "es5, es2015, -es3")])).unwrap();
    assert_eq!(targets.len(), 2);
}

#[test]
fn configuration_names_sort_keys_and_lower_case_values() {
    let configurations = configurations(&settings(&[
        ("strict", "true, false"),
        ("module", "CommonJS, ESNext"),
        ("filename", "a.ts"),
    ]))
    .unwrap();
    let names: BTreeSet<_> = configurations.iter().map(|c| c.name.clone()).collect();
    assert!(names.contains("module=commonjs,strict=true"));
    assert_eq!(configurations[0].settings["filename"], "a.ts");
    assert_eq!(baseline_stem("x.tsx", "jsx=react"), "x(jsx=react)");
    assert_eq!(baseline_stem("x.ts", ""), "x");
}

#[test]
fn more_than_twenty_five_variations_is_fatal() {
    let result = configurations(&settings(&[
        ("target", "es2015, es2016, es2017"),
        ("module", "commonjs, es2015, es2020"),
        ("strict", "true, false"),
        ("jsx", "react, preserve"),
    ]));
    assert!(result.is_err());
}

#[test]
fn unsupported_rules_read_the_tsconfig_under_the_directives() {
    let tsconfig = r#"{ "compilerOptions": { "module": "System", /* old */ "strict": true, }, }"#;
    let options = EffectiveOptions::from_configuration(Some(tsconfig), &BTreeMap::new());
    assert_eq!(
        options.unsupported(),
        Some(NativeSkip::Unsupported("module=system"))
    );
    let options =
        EffectiveOptions::from_configuration(Some(tsconfig), &settings(&[("module", "commonjs")]));
    assert_eq!(options.unsupported(), None);
    let options = EffectiveOptions::from_configuration(None, &settings(&[("module", "AMD")]));
    assert_eq!(options.unsupported(), Some(NativeSkip::Fatal("module=amd")));
    let options =
        EffectiveOptions::from_configuration(None, &settings(&[("alwaysstrict", "false")]));
    assert_eq!(
        options.unsupported(),
        Some(NativeSkip::Unsupported("alwaysStrict=false"))
    );
}

#[test]
fn a_commented_out_tsconfig_entry_after_a_trailing_comma_still_parses() {
    let tsconfig = "{\r\n    \"compilerOptions\": {\r\n        \"baseUrl\": \".\",\r\n        \"paths\": {\r\n            \"@shared/*\": [\"../shared/*\"]\r\n        }\r\n    },\r\n   //\"files\": [\"src/app.ts\"]\r\n}";
    let options = EffectiveOptions::from_configuration(Some(tsconfig), &BTreeMap::new());
    assert_eq!(
        options.unsupported(),
        Some(NativeSkip::Unsupported("baseUrl"))
    );
}

#[test]
fn an_unsupported_option_inherited_through_extends_skips_the_configuration() {
    // tsgo's harness parses the test's tsconfig with the compiler's parser,
    // so `SkipUnsupportedCompilerOptions` sees the inherited `baseUrl`
    // (compiler/pathMappingInheritedBaseUrl has no baseline).
    let units = [
        (
            "/other/tsconfig.base.json",
            "{ \"compilerOptions\": { \"baseUrl\": \".\" } }",
        ),
        (
            "/project/tsconfig.json",
            "{ \"extends\": \"../other/tsconfig.base.json\", \"compilerOptions\": { \"module\": \"commonjs\" } }",
        ),
    ];
    let options =
        EffectiveOptions::from_units(Some("/project/tsconfig.json"), &units, &BTreeMap::new());
    assert_eq!(
        options.unsupported(),
        Some(NativeSkip::Unsupported("baseUrl"))
    );
    // The config's own option wins over an inherited one, a list is
    // followed in order, and `.json` is added to a name without it.
    let units = [
        (
            "/a.json",
            "{ \"compilerOptions\": { \"module\": \"system\" } }",
        ),
        (
            "/b.json",
            "{ \"compilerOptions\": { \"module\": \"umd\" } }",
        ),
        ("/tsconfig.json", "{ \"extends\": [\"./a\", \"./b.json\"] }"),
    ];
    let options = EffectiveOptions::from_units(Some("/tsconfig.json"), &units, &BTreeMap::new());
    assert_eq!(
        options.unsupported(),
        Some(NativeSkip::Unsupported("module=umd"))
    );
    let units = [
        (
            "/a.json",
            "{ \"compilerOptions\": { \"module\": \"system\" } }",
        ),
        (
            "/tsconfig.json",
            "{ \"extends\": \"./a.json\", \"compilerOptions\": { \"module\": \"commonjs\" } }",
        ),
    ];
    let options = EffectiveOptions::from_units(Some("/tsconfig.json"), &units, &BTreeMap::new());
    assert_eq!(options.unsupported(), None);
}

#[test]
fn the_tsconfig_unit_of_the_package_id_case_is_found() {
    let content = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/testdata/tests/cases/compiler/moduleResolutionPackageIdWithRelativeAndAbsolutePath.ts"
    ))
    .unwrap();
    let (units, _) = compiler::make_units_from_test(&content, "x.ts").unwrap();
    let names: Vec<_> = units.iter().map(|unit| unit.name.clone()).collect();
    let config = units
        .iter()
        .find(|unit| compiler::is_config_file_name(&unit.name));
    assert!(config.is_some(), "{names:?}");
    let text = config.unwrap().content.clone();
    assert!(text.is_some(), "{names:?}");
    let options = EffectiveOptions::from_configuration(text.as_deref(), &BTreeMap::new());
    assert_eq!(
        options.unsupported(),
        Some(NativeSkip::Unsupported("baseUrl")),
        "{:?}",
        text
    );
}
