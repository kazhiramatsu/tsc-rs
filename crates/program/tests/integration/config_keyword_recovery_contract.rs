use tsc_program::{
    parse_config_root_plan, ConfigHostError, ConfigOptionValueState, ConfigParseErrorKind,
    ConfigParseHost, ConfigRootPlanRequest,
};

struct EmptyConfigHost;

impl ConfigParseHost for EmptyConfigHost {
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn file_exists(&self, _path: &str) -> Result<bool, ConfigHostError> {
        Ok(false)
    }

    fn read_file(&self, _path: &str) -> Result<Option<String>, ConfigHostError> {
        Ok(None)
    }

    fn read_directory(
        &self,
        _directory: &str,
        _extensions: &[&str],
        _excludes: Option<&[String]>,
        _includes: Option<&[String]>,
        _depth: Option<usize>,
    ) -> Result<Vec<String>, ConfigHostError> {
        Ok(Vec::new())
    }
}

fn request(text: String) -> ConfigRootPlanRequest {
    ConfigRootPlanRequest {
        file_name: "/project/tsconfig.json".to_owned(),
        text,
        base_path: "/".to_owned(),
    }
}

#[test]
fn keyword_leaf_values_recover_as_typescript_undefined_options() {
    for keyword in ["module", "any", "string"] {
        let plan = parse_config_root_plan(
            &EmptyConfigHost,
            request(format!(
                r#"{{"compilerOptions":{{"strict":{keyword}}},"files":["x.ts"]}}"#
            )),
        )
        .expect("a non-recursive keyword leaf is recoverable config syntax");

        assert!(plan.root_parse_diagnostics().is_empty(), "{keyword}");
        assert_eq!(
            plan.errors()
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [5024],
            "{keyword}"
        );
        assert_eq!(
            plan.options().typed_value_state("strict"),
            ConfigOptionValueState::Undefined,
            "{keyword}"
        );
    }
}

#[test]
fn recursive_expression_keywords_remain_available_as_property_names() {
    let plan = parse_config_root_plan(
        &EmptyConfigHost,
        request(r#"{delete:true,files:["x.ts"]}"#.to_owned()),
    )
    .expect("a recursive-expression keyword followed by ':' is a bounded property name");

    assert!(plan.root_parse_diagnostics().is_empty());
    assert_eq!(
        plan.errors()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [1327, 1327]
    );
    assert_eq!(plan.file_names(), ["/project/x.ts"]);
}

#[test]
fn bounded_recursive_keyword_chains_keep_typescripts_recovery() {
    for keyword in ["delete", "typeof", "void", "await", "yield", "new"] {
        let plan = parse_config_root_plan(
            &EmptyConfigHost,
            request(format!(
                r#"{{"compilerOptions":{{"strict":{keyword} {keyword} module}},"files":["x.ts"]}}"#
            )),
        )
        .expect("a bounded recursive keyword expression is recoverable config syntax");
        assert!(plan.root_parse_diagnostics().is_empty(), "{keyword}");
        assert_eq!(
            plan.errors()
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [5024],
            "{keyword}"
        );
        assert_eq!(
            plan.options().typed_value_state("strict"),
            ConfigOptionValueState::Undefined,
            "{keyword}"
        );
    }
}

#[test]
fn bounded_type_and_class_recovery_remains_available() {
    for expression in [
        "module as keyof string",
        "module as asserts value is string",
        "class Derived extends Base {}",
    ] {
        let plan = parse_config_root_plan(
            &EmptyConfigHost,
            request(format!(
                r#"{{"compilerOptions":{{"strict":{expression}}},"files":["x.ts"]}}"#
            )),
        )
        .expect("a bounded TypeScript recovery expression remains available");

        assert!(plan.root_parse_diagnostics().is_empty(), "{expression}");
        assert_eq!(
            plan.errors()
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [5024],
            "{expression}"
        );
        assert_eq!(
            plan.options().typed_value_state("strict"),
            ConfigOptionValueState::Undefined,
            "{expression}"
        );
    }
}

#[test]
fn recursive_keyword_depth_is_bounded_before_the_recursive_parser() {
    let bounded = "delete ".repeat(256);
    let plan = parse_config_root_plan(
        &EmptyConfigHost,
        request(format!(
            r#"{{"compilerOptions":{{"strict":{bounded}module}},"files":["x.ts"]}}"#
        )),
    )
    .expect("the documented recursive keyword boundary remains recoverable");
    assert_eq!(
        plan.errors()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5024]
    );
    assert_eq!(
        plan.options().typed_value_state("strict"),
        ConfigOptionValueState::Undefined
    );

    let over_limit = "delete ".repeat(257);
    let error = parse_config_root_plan(
        &EmptyConfigHost,
        request(format!(
            r#"{{"compilerOptions":{{"strict":{over_limit}module}},"files":["x.ts"]}}"#
        )),
    )
    .expect_err("recursive keyword depth above the documented boundary must fail early");
    assert_eq!(error.kind(), ConfigParseErrorKind::ResourceLimit);

    let chain = "delete ".repeat(100_000);
    let error = parse_config_root_plan(
        &EmptyConfigHost,
        request(format!(
            r#"{{"compilerOptions":{{"strict":{chain}module}},"files":["x.ts"]}}"#
        )),
    )
    .expect_err("a long unary-keyword chain must not reach the recursive syntax parser");
    assert_eq!(error.kind(), ConfigParseErrorKind::ResourceLimit);
}

#[test]
fn separated_type_and_class_recursion_is_bounded_per_value() {
    let type_operators = "keyof ".repeat(257);
    let error = parse_config_root_plan(
        &EmptyConfigHost,
        request(format!(
            r#"{{"compilerOptions":{{"strict":module as {type_operators}string}},"files":["x.ts"]}}"#
        )),
    )
    .expect_err("a deep type-operator recovery expression must fail before parsing");
    assert_eq!(error.kind(), ConfigParseErrorKind::ResourceLimit);

    let infer_constraints = "infer T extends ".repeat(257);
    let error = parse_config_root_plan(
        &EmptyConfigHost,
        request(format!(
            r#"{{"compilerOptions":{{"strict":module as {infer_constraints}string}},"files":["x.ts"]}}"#
        )),
    )
    .expect_err("identifiers must not reset the recursive type budget");
    assert_eq!(error.kind(), ConfigParseErrorKind::ResourceLimit);

    let asserted_types = "asserts value is ".repeat(257);
    let error = parse_config_root_plan(
        &EmptyConfigHost,
        request(format!(
            r#"{{"compilerOptions":{{"strict":module as {asserted_types}string}},"files":["x.ts"]}}"#
        )),
    )
    .expect_err("deep assertion predicates must fail before recursive type parsing");
    assert_eq!(error.kind(), ConfigParseErrorKind::ResourceLimit);

    let mut nested_type = "module as ".to_owned();
    for _ in 0..128 {
        nested_type.push_str("{value:keyof readonly ");
    }
    nested_type.push_str("string");
    nested_type.push_str(&"}".repeat(128));
    let error = parse_config_root_plan(
        &EmptyConfigHost,
        request(format!(
            r#"{{"compilerOptions":{{"strict":{nested_type}}},"files":["x.ts"]}}"#
        )),
    )
    .expect_err("nested value separators must retain their parent recursive budget");
    assert_eq!(error.kind(), ConfigParseErrorKind::ResourceLimit);

    let class_heritage = "class C extends ".repeat(257);
    let error = parse_config_root_plan(
        &EmptyConfigHost,
        request(format!(
            r#"{{"compilerOptions":{{"strict":{class_heritage}Base {{}}}},"files":["x.ts"]}}"#
        )),
    )
    .expect_err("deep class heritage must fail before recursive expression parsing");
    assert_eq!(error.kind(), ConfigParseErrorKind::ResourceLimit);
}
