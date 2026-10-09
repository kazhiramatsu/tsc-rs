use tsc_program::{
    parse_config_root_plan, ConfigHostError, ConfigOptionValueState, ConfigParseErrorKind,
    ConfigParseHost, ConfigRootPlanRequest,
};

struct EmptyConfigHost;

impl ConfigParseHost for EmptyConfigHost {
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn file_exists(&self, _path: tsc_diagnostics::JsStr<'_>) -> Result<bool, ConfigHostError> {
        let _path = _path.as_str().expect("scalar config fixture query");

        Ok(false)
    }

    fn read_file(
        &self,
        _path: tsc_diagnostics::JsStr<'_>,
    ) -> Result<Option<String>, ConfigHostError> {
        let _path = _path.as_str().expect("scalar config fixture query");

        Ok(None)
    }

    fn read_directory(
        &self,
        _directory: tsc_diagnostics::JsStr<'_>,
        _extensions: &[&str],
        _excludes: Option<&[tsc_diagnostics::JsString]>,
        _includes: Option<&[tsc_diagnostics::JsString]>,
        _depth: Option<usize>,
    ) -> Result<Vec<tsc_diagnostics::JsString>, ConfigHostError> {
        let scalar_paths: Result<Vec<String>, ConfigHostError> = { Ok(Vec::new()) };
        scalar_paths.map(|paths| paths.into_iter().map(Into::into).collect())
    }
}

fn request(text: String) -> ConfigRootPlanRequest {
    ConfigRootPlanRequest {
        file_name: "/project/tsconfig.json".to_owned().into(),
        text,
        base_path: "/".to_owned().into(),
    }
}

fn codes(diagnostics: &[tsc_diagnostics::Diagnostic]) -> Vec<u32> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect()
}

/// tsgo's parse of a value JSON has no form for (TS1328) and its
/// conversion against the `strict` option (TS5024): the option stays unset.
fn assert_recovered_strict(text: &str) {
    let plan = parse_config_root_plan(
        &EmptyConfigHost,
        request(format!(
            r#"{{"compilerOptions":{{"strict":{text}}},"files":["x.ts"]}}"#
        )),
    )
    .unwrap_or_else(|error| panic!("{text}: {error:?}"));
    assert_eq!(codes(plan.root_parse_diagnostics()), [1328], "{text}");
    assert_eq!(codes(plan.errors()), [5024], "{text}");
    assert_eq!(
        plan.options().typed_value_state("strict"),
        ConfigOptionValueState::Undefined,
        "{text}"
    );
}

// Each expectation below is tsgo's (`tsc -p . --noEmit` on the same text).
#[test]
fn keyword_leaf_values_recover_as_typescript_undefined_options() {
    for keyword in ["module", "any", "string"] {
        assert_recovered_strict(keyword);
    }
}

#[test]
fn recursive_expression_keywords_remain_available_as_property_names() {
    let plan = parse_config_root_plan(
        &EmptyConfigHost,
        request(r#"{delete:true,files:["x.ts"]}"#.to_owned()),
    )
    .expect("a keyword followed by ':' is a property name");

    // The parser reports both unquoted names (TS1327); the conversion reads
    // them as names.
    assert_eq!(codes(plan.root_parse_diagnostics()), [1327, 1327]);
    assert!(plan.errors().is_empty());
    assert_eq!(plan.file_names(), ["/project/x.ts"]);
}

#[test]
fn recursive_keyword_and_type_expressions_recover_as_typescript_does() {
    for expression in [
        "delete delete module",
        "typeof typeof module",
        "void void module",
        "await await module",
        "new new module",
        "module as keyof string",
        "module as asserts value is string",
        "class Derived extends Base {}",
    ] {
        assert_recovered_strict(expression);
    }
}

/// A configuration is parsed as deep as the stack allows, as tsgo parses
/// one (on the compiler's stack reservation, like every source file); only
/// the nesting of objects, arrays and parentheses has a limit.
#[test]
fn deep_recursive_expressions_parse_on_the_compiler_stack() {
    std::thread::Builder::new()
        .stack_size(tsc_program::WORKER_STACK_BYTES)
        .spawn(|| {
            for text in [
                format!("{}module", "delete ".repeat(10_000)),
                format!("module as {}string", "keyof ".repeat(257)),
                format!("module as {}string", "infer T extends ".repeat(257)),
                format!("module as {}string", "asserts value is ".repeat(257)),
            ] {
                assert_recovered_strict(&text);
            }
            // A class heritage chain also misses its closing braces: the
            // parser records that error (TS1005) before it validates the
            // value (TS1328), as tsgo's parseJSONText does.
            let heritage = format!("{}Base {{}}", "class C extends ".repeat(257));
            let plan = parse_config_root_plan(
                &EmptyConfigHost,
                request(format!(
                    r#"{{"compilerOptions":{{"strict":{heritage}}},"files":["x.ts"]}}"#
                )),
            )
            .expect("a deep class heritage chain is recoverable config syntax");
            assert_eq!(codes(plan.root_parse_diagnostics()), [1005, 1328]);
            assert_eq!(codes(plan.errors()), [5024]);
        })
        .expect("spawn a thread with the compiler's stack")
        .join()
        .expect("deep config expressions parse");
}

#[test]
fn structural_nesting_keeps_its_resource_limit() {
    let nested = |depth: usize| {
        format!(
            r#"{{"compilerOptions":{{"types":{}"x"{}}},"files":["x.ts"]}}"#,
            "[".repeat(depth),
            "]".repeat(depth)
        )
    };
    // The root object and `compilerOptions` count toward the 256 levels.
    parse_config_root_plan(&EmptyConfigHost, request(nested(254)))
        .expect("nesting within the limit parses");
    let error = parse_config_root_plan(&EmptyConfigHost, request(nested(255)))
        .expect_err("nesting above the limit is a resource limit");
    assert_eq!(error.kind(), ConfigParseErrorKind::ResourceLimit);
    // A closing token closes its own kind and what was opened inside it.
    parse_config_root_plan(
        &EmptyConfigHost,
        request(format!(
            r#"{{"compilerOptions":{{"types":{}"x"}}}},"files":["x.ts"]}}"#,
            "[".repeat(100)
        )),
    )
    .expect("a closing brace closes the unclosed arrays inside it");
}
