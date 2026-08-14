use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn vendor_lib_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/typescript-6.0.3/lib")
        .canonicalize()
        .expect("vendored TypeScript lib exists")
}

#[test]
fn harness_reaches_checker_api() {
    assert!(check_empty_program().diagnostics.is_empty());
}

#[test]
fn compiler_option_projection_keeps_aliases_lists_and_null() {
    let options = BTreeMap::from([
        ("allowJs".to_owned(), OptionValue::Null),
        ("checkJs".to_owned(), OptionValue::Bool(true)),
        ("target".to_owned(), OptionValue::String("ES6".to_owned())),
        (
            "module".to_owned(),
            OptionValue::String("es2015".to_owned()),
        ),
        (
            "moduleResolution".to_owned(),
            OptionValue::String("node".to_owned()),
        ),
        ("moduleDetection".to_owned(), OptionValue::Number(2)),
        ("maxNodeModuleJsDepth".to_owned(), OptionValue::Number(3)),
        (
            "jsx".to_owned(),
            OptionValue::String("react-jsx".to_owned()),
        ),
        (
            "customConditions".to_owned(),
            OptionValue::StringList(vec!["browser".to_owned(), "development".to_owned()]),
        ),
        (
            "moduleSuffixes".to_owned(),
            OptionValue::String(" .Native , ".to_owned()),
        ),
        (
            "lib".to_owned(),
            OptionValue::String(" ES5, DOM ".to_owned()),
        ),
        ("libReplacement".to_owned(), OptionValue::Bool(true)),
    ]);

    let permissive = compiler_options_from_options(&options);
    let closed = try_compiler_options_from_options(&options)
        .expect("all values belong to the production projection");
    assert_eq!(permissive, closed);
    assert!(closed.allow_js);
    assert_eq!(closed.target, Some(2));
    assert_eq!(closed.module, Some(5));
    assert_eq!(closed.module_resolution, Some(2));
    assert_eq!(closed.module_detection, Some(2));
    assert_eq!(
        closed
            .max_node_module_js_depth
            .map(tsc_program::CompilerOptionNumber::value),
        Some(3.0)
    );
    assert_eq!(closed.jsx, Some(4));
    assert_eq!(
        closed.module_suffixes.as_deref(),
        Some([ModuleSuffix::value(".Native "), ModuleSuffix::value("")].as_slice())
    );
    assert_eq!(
        closed.custom_conditions,
        Some(vec!["browser".to_owned(), "development".to_owned()])
    );
    assert_eq!(closed.lib, Some(vec!["es5".to_owned(), "dom".to_owned()]));
    assert_eq!(closed.lib_replacement, Some(true));
}

#[test]
fn closed_compiler_option_projection_rejects_invalid_inputs() {
    for options in [
        BTreeMap::from([("futureOption".to_owned(), OptionValue::Bool(true))]),
        BTreeMap::from([("strict".to_owned(), OptionValue::String("true".to_owned()))]),
        BTreeMap::from([(
            "target".to_owned(),
            OptionValue::String("future".to_owned()),
        )]),
        BTreeMap::from([("jsx".to_owned(), OptionValue::Number(99))]),
    ] {
        assert!(
            try_compiler_options_from_options(&options).is_err(),
            "{options:?}"
        );
    }
}

#[test]
fn corpus_projection_retains_h1_printer_options_without_expanding_m9() {
    let options = BTreeMap::from([
        ("newLine".to_owned(), OptionValue::String("CRLF".to_owned())),
        ("removeComments".to_owned(), OptionValue::Bool(true)),
        ("noImplicitUseStrict".to_owned(), OptionValue::Bool(true)),
        ("noEmitHelpers".to_owned(), OptionValue::Bool(true)),
    ]);

    let projected = compiler_options_from_options(&options);
    assert_eq!(projected.new_line, Some(0));
    assert_eq!(projected.remove_comments, Some(true));
    assert_eq!(projected.no_implicit_use_strict, Some(true));
    assert_eq!(projected.no_emit_helpers, Some(true));
    assert!(
        try_compiler_options_from_options(&options).is_err(),
        "H1 emit options do not broaden the closed M9 checker projection"
    );
}

#[test]
fn closed_compiler_option_projection_rejects_casefold_duplicates() {
    let options = BTreeMap::from([
        ("Strict".to_owned(), OptionValue::Bool(true)),
        ("strict".to_owned(), OptionValue::Bool(false)),
    ]);
    let error = try_compiler_options_from_options(&options)
        .expect_err("tsc option names are case-insensitive");
    assert!(
        error.to_string().contains("ASCII-case-insensitively"),
        "{error}"
    );
}

#[test]
fn expands_single_file_snapshot() {
    let programs = expand_fixture_text(
        "plain.ts",
        "// @noLib: true\n\nlet x = 1;\n",
        &vendor_lib_dir(),
    )
    .expect("fixture expands");

    assert_eq!(programs.len(), 1);
    assert_eq!(
        programs[0].to_json(),
        "{\n  \"schema\": 1,\n  \"cwd\": \"/\",\n  \"options\": {\n    \"noLib\": true\n  },\n  \"libs\": [],\n  \"files\": [\n    {\n      \"name\": \"plain.ts\",\n      \"textB64\": \"bGV0IHggPSAxOwo=\"\n    }\n  ],\n  \"matrixKey\": \"\"\n}\n"
    );
}

#[test]
fn no_error_truncation_directive_reaches_program_options() {
    let programs = expand_fixture_text(
        "display.ts",
        "// @noErrorTruncation: true\nlet x = 1;\n",
        &vendor_lib_dir(),
    )
    .expect("fixture expands");

    assert_eq!(
        programs[0].options.get("noErrorTruncation"),
        Some(&OptionValue::Bool(true))
    );
}

#[test]
fn strips_bom_and_preserves_crlf_snapshot() {
    let programs = expand_fixture_text(
        "bom.ts",
        "\u{feff}// @noLib: true\r\n\r\nlet x = 1;\r\n",
        &vendor_lib_dir(),
    )
    .expect("fixture expands");

    assert_eq!(programs[0].files[0].text_b64, "bGV0IHggPSAxOw0K");
}

#[test]
fn splits_multi_file_snapshot() {
    let programs = expand_fixture_text(
        "multi.ts",
        "// @noLib: true\n// @filename: a.ts\nexport const a = 1;\n// @filename: b.ts\nimport { a } from \"./a\";\na;\n",
        &vendor_lib_dir(),
    )
    .expect("fixture expands");

    assert_eq!(programs.len(), 1);
    assert_eq!(programs[0].files.len(), 2);
    assert_eq!(programs[0].files[0].name, "a.ts");
    assert_eq!(
        programs[0].files[0].text_b64,
        "ZXhwb3J0IGNvbnN0IGEgPSAxOwo="
    );
    assert_eq!(programs[0].files[1].name, "b.ts");
    assert_eq!(
        programs[0].files[1].text_b64,
        "aW1wb3J0IHsgYSB9IGZyb20gIi4vYSI7CmE7Cg=="
    );
}

#[test]
fn expands_target_matrix_snapshot() {
    let programs = expand_fixture_text(
        "matrix.ts",
        "// @noLib: true\n// @target: es5, es2015\nlet x = 1;\n",
        &vendor_lib_dir(),
    )
    .expect("fixture expands");

    assert_eq!(programs.len(), 2);
    assert_eq!(programs[0].matrix_key, "target=es5");
    assert_eq!(
        programs[0].options.get("target"),
        Some(&OptionValue::String("es5".to_owned()))
    );
    assert_eq!(programs[1].matrix_key, "target=es2015");
    assert_eq!(
        programs[1].options.get("target"),
        Some(&OptionValue::String("es2015".to_owned()))
    );
}

#[test]
fn resolves_default_and_explicit_libs() {
    let default_programs = expand_fixture_text(
        "default.ts",
        "// @target: es2015\nlet x = new Promise(() => {});\n",
        &vendor_lib_dir(),
    )
    .expect("fixture expands");
    assert!(default_programs[0]
        .libs
        .contains(&"lib.es6.d.ts".to_owned()));
    assert!(default_programs[0]
        .libs
        .contains(&"lib.es5.d.ts".to_owned()));
    assert!(default_programs[0]
        .libs
        .contains(&"lib.es2015.promise.d.ts".to_owned()));

    let explicit_programs = expand_fixture_text(
        "lib.ts",
        "// @lib: es5,dom\nlet documentTitle = document.title;\n",
        &vendor_lib_dir(),
    )
    .expect("fixture expands");
    assert_eq!(
        explicit_programs[0].options.get("lib"),
        Some(&OptionValue::StringList(vec![
            "es5".to_owned(),
            "dom".to_owned(),
        ]))
    );
    assert!(explicit_programs[0]
        .libs
        .contains(&"lib.es5.d.ts".to_owned()));
    assert!(explicit_programs[0]
        .libs
        .contains(&"lib.dom.d.ts".to_owned()));
}

#[test]
fn rejects_unknown_directives() {
    let err = expand_fixture_text(
        "bad.ts",
        "// @definitelyUnknown: true\nlet x = 1;\n",
        &vendor_lib_dir(),
    )
    .expect_err("unknown directives are hard errors");
    assert!(err.to_string().contains("unknown fixture directive"));
}

#[test]
fn hand_picked_fixture_set_expands() {
    let fixtures = [
        ("plain.ts", "// @noLib: true\nlet x = 1;\n", 1),
        (
            "strict.ts",
            "// @noLib: true\n// @strict: true\nlet x = 1;\n",
            1,
        ),
        (
            "target.ts",
            "// @noLib: true\n// @target: es5, es2015\nlet x = 1;\n",
            2,
        ),
        (
            "module.ts",
            "// @noLib: true\n// @module: commonjs, esnext\nexport {};\n",
            2,
        ),
        (
            "both.ts",
            "// @noLib: true\n// @target: es5, es2015\n// @module: commonjs, esnext\nexport {};\n",
            4,
        ),
        ("crlf.ts", "// @noLib: true\r\nlet x = 1;\r\n", 1),
        ("bom.ts", "\u{feff}// @noLib: true\nlet x = 1;\n", 1),
        (
            "multi.ts",
            "// @noLib: true\n// @filename: a.ts\nlet a = 1;\n// @filename: b.ts\nlet b = 2;\n",
            1,
        ),
        ("lib.ts", "// @lib: es5\nlet x: string;\n", 1),
        (
            "jsx.tsx",
            "// @noLib: true\n// @jsx: react-jsx\n<div />;\n",
            1,
        ),
        (
            "allowjs.ts",
            "// @noLib: true\n// @allowJs: true\nlet x = 1;\n",
            1,
        ),
        (
            "checkjs.ts",
            "// @noLib: true\n// @checkJs: true\nlet x = 1;\n",
            1,
        ),
        (
            "decl.ts",
            "// @noLib: true\n// @declaration: true\nlet x = 1;\n",
            1,
        ),
        (
            "unused.ts",
            "// @noLib: true\n// @noUnusedLocals: true\nlet x = 1;\n",
            1,
        ),
        (
            "moduleResolution.ts",
            "// @noLib: true\n// @moduleResolution: node16\nexport {};\n",
            1,
        ),
        (
            "outdir.ts",
            "// @noLib: true\n// @outdir: built\nlet x = 1;\n",
            1,
        ),
        (
            "types.ts",
            "// @noLib: true\n// @types: node,jest\nlet x = 1;\n",
            1,
        ),
        (
            "cwd.ts",
            "// @noLib: true\n// @currentDirectory: /src\nlet x = 1;\n",
            1,
        ),
        (
            "filename-case.ts",
            "// @noLib: true\n// @Filename: main.ts\nlet x = 1;\n",
            1,
        ),
        (
            "null-module.ts",
            "// @noLib: true\n// @module: undefined\nexport {};\n",
            1,
        ),
    ];
    assert_eq!(fixtures.len(), 20);

    for (name, text, expected_count) in fixtures {
        let programs = expand_fixture_text(name, text, &vendor_lib_dir())
            .unwrap_or_else(|err| panic!("{name} should expand: {err}"));
        assert_eq!(programs.len(), expected_count, "{name}");
    }
}

#[test]
fn writes_program_json_files() {
    let programs = expand_fixture_text(
        "matrix.ts",
        "// @noLib: true\n// @target: es5, es2015\nlet x = 1;\n",
        &vendor_lib_dir(),
    )
    .expect("fixture expands");
    let temp = std::env::temp_dir().join(format!(
        "tsc-rs-harness-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));

    let paths = write_program_jsons(&programs, &temp).expect("programs write");
    assert_eq!(paths.len(), 2);
    assert!(paths[0]
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .contains("target=es5"));
    assert!(paths[1]
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .contains("target=es2015"));

    fs::remove_dir_all(temp).expect("remove temp dir");
}
