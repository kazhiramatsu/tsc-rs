//! Configuration diagnostics as tsgo reports them: each expectation is the
//! output of the tsgo binary at the vendored commit (`tsc -p . --noEmit` on
//! the same `tsconfig.json`, with `a.ts` beside it), as a code, a UTF-16
//! offset and a message. The plan's parse diagnostics and errors together
//! are those rows, sorted and deduplicated as the command reports them.

use tsc_program::{
    parse_config_root_plan, ConfigHostError, ConfigOptionValueState, ConfigParseHost,
    ConfigRootPlanRequest,
};

struct ProjectHost;

impl ConfigParseHost for ProjectHost {
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn file_exists(&self, path: tsc_diagnostics::JsStr<'_>) -> Result<bool, ConfigHostError> {
        Ok(path == "/project/a.ts")
    }

    fn read_file(
        &self,
        _path: tsc_diagnostics::JsStr<'_>,
    ) -> Result<Option<String>, ConfigHostError> {
        Ok(None)
    }

    fn read_directory(
        &self,
        directory: tsc_diagnostics::JsStr<'_>,
        _extensions: &[&str],
        _excludes: Option<&[tsc_diagnostics::JsString]>,
        _includes: Option<&[tsc_diagnostics::JsString]>,
        _depth: Option<usize>,
    ) -> Result<Vec<tsc_diagnostics::JsString>, ConfigHostError> {
        Ok(if directory == "/project" {
            vec!["/project/a.ts".to_owned().into()]
        } else {
            Vec::new()
        })
    }
}

/// A configuration text and tsgo's diagnostics for it: code, UTF-16
/// offset, message.
type Case = (&'static str, &'static [(u32, u32, &'static str)]);

fn plan(text: &str) -> tsc_program::ConfigRootPlan {
    parse_config_root_plan(
        &ProjectHost,
        ConfigRootPlanRequest {
            file_name: "/project/tsconfig.json".to_owned().into(),
            text: text.to_owned(),
            base_path: "/project".to_owned().into(),
        },
    )
    .unwrap_or_else(|error| panic!("{text}: {error:?}"))
}

#[test]
fn configuration_diagnostics_match_tsgo() {
    let cases: &[Case] = &[
        (
            r#"{ broken"#,
            &[
                (1136, 1, r#"Property assignment expected."#),
                (1136, 2, r#"Property assignment expected."#),
                (1005, 8, r#"'}' expected."#),
            ][..],
        ),
        (
            r#"{ "compilerOptions": { "strict": true, } x }"#,
            &[
                (1136, 40, r#"Property assignment expected."#),
                (1005, 41, r#"',' expected."#),
                (1136, 41, r#"Property assignment expected."#),
            ][..],
        ),
        (
            r#"{ "files": ["a.ts" }"#,
            &[(1005, 19, r#"',' expected."#)][..],
        ),
        (
            r#"{ "compilerOptions": { "strict": true }}}"#,
            &[(1012, 40, r#"Unexpected token."#)][..],
        ),
        (
            r#"{ "compilerOptions": { "strict": true } } extra"#,
            &[
                (1012, 42, r#"Unexpected token."#),
                (1136, 42, r#"Property assignment expected."#),
                (1005, 47, r#"'}' expected."#),
            ][..],
        ),
        (r#"}"#, &[(1005, 0, r#"'{' expected."#)][..]),
        (
            r#"{ "compilerOptions": { tru } }"#,
            &[
                (1136, 22, r#"Property assignment expected."#),
                (1136, 23, r#"Property assignment expected."#),
            ][..],
        ),
        (
            r#"{ "compilerOptions": { "strict": tru }, "files": ["a.ts"] }"#,
            &[
                (
                    5024,
                    32,
                    r#"Compiler option 'strict' requires a value of type boolean."#,
                ),
                (
                    1328,
                    33,
                    r#"Property value can only be string literal, numeric literal, 'true', 'false', 'null', object literal or array literal."#,
                ),
            ][..],
        ),
        (
            r#"{ "compilerOptions": { "strict"?: true }, "files": ["a.ts"] }"#,
            &[(
                8009,
                31,
                r#"The '?' modifier can only be used in TypeScript files."#,
            )][..],
        ),
        (
            r#"{ "compilerOptions": { ["strict"]: true }, "files": ["a.ts"] }"#,
            &[(1327, 23, r#"String literal with double quotes expected."#)][..],
        ),
        (
            r#"{ "compilerOptions": { "target": 5 }, "files": ["a.ts"] }"#,
            &[(
                5024,
                33,
                r#"Compiler option 'target' requires a value of type enum."#,
            )][..],
        ),
        (
            r#"{ "compilerOptions": { "lib": [5] }, "files": ["a.ts"] }"#,
            &[(
                5024,
                31,
                r#"Compiler option 'lib' requires a value of type enum."#,
            )][..],
        ),
        (
            r#"{ "compilerOptions": { "lib": [tru] }, "files": ["a.ts"] }"#,
            &[
                (
                    1328,
                    31,
                    r#"Property value can only be string literal, numeric literal, 'true', 'false', 'null', object literal or array literal."#,
                ),
                (
                    5024,
                    31,
                    r#"Compiler option 'lib' requires a value of type Array."#,
                ),
            ][..],
        ),
        (
            r#"{ "compilerOptions": { "strict": [foo] }, "files": ["a.ts"] }"#,
            &[
                (
                    5024,
                    33,
                    r#"Compiler option 'strict' requires a value of type boolean."#,
                ),
                (
                    1328,
                    34,
                    r#"Property value can only be string literal, numeric literal, 'true', 'false', 'null', object literal or array literal."#,
                ),
                (
                    5024,
                    34,
                    r#"Compiler option 'strict' requires a value of type boolean."#,
                ),
            ][..],
        ),
        (
            r#"{ "extends": tru, "files": ["a.ts"] }"#,
            &[
                (
                    5024,
                    12,
                    r#"Compiler option 'extends' requires a value of type string or Array."#,
                ),
                (
                    1328,
                    13,
                    r#"Property value can only be string literal, numeric literal, 'true', 'false', 'null', object literal or array literal."#,
                ),
                (
                    5024,
                    13,
                    r#"Compiler option 'extends' requires a value of type string or Array."#,
                ),
            ][..],
        ),
        (
            r#"{ "watchOptions": { "watchFile": 5, "bogus": 1 }, "files": ["a.ts"] }"#,
            &[][..],
        ),
        (
            r#"{ "compilerOptions": { "STRICT": null, "Strict": tru }, "files": ["a.ts"] }"#,
            &[
                (
                    1328,
                    48,
                    r#"Property value can only be string literal, numeric literal, 'true', 'false', 'null', object literal or array literal."#,
                ),
                (
                    1328,
                    49,
                    r#"Property value can only be string literal, numeric literal, 'true', 'false', 'null', object literal or array literal."#,
                ),
            ][..],
        ),
        (
            r#"{ "compilerOptions": { "help": true }, "files": ["a.ts"] }"#,
            &[(
                6266,
                23,
                r#"Option 'help' can only be specified on command line."#,
            )][..],
        ),
        (
            r#"{ "references": 5, "files": [] }"#,
            &[
                (
                    5024,
                    16,
                    r#"Compiler option 'references' requires a value of type Array."#,
                ),
                (
                    18002,
                    28,
                    r#"The 'files' list in config file '/project/tsconfig.json' is empty."#,
                ),
            ][..],
        ),
        (
            r#"{ "extends": null, "files": [] }"#,
            &[
                (
                    5024,
                    13,
                    r#"Compiler option 'extends' requires a value of type string or Array."#,
                ),
                (
                    18002,
                    28,
                    r#"The 'files' list in config file '/project/tsconfig.json' is empty."#,
                ),
            ][..],
        ),
        (
            r#"{ "compilerOptions": [], "files": ["a.ts"] }"#,
            &[(
                5024,
                21,
                r#"Compiler option 'compilerOptions' requires a value of type object."#,
            )][..],
        ),
        (
            r#"{ "compilerOptions": { "paths": [] }, "files": ["a.ts"] }"#,
            &[(
                5024,
                32,
                r#"Compiler option 'paths' requires a value of type object."#,
            )][..],
        ),
    ];
    for (text, expected) in cases {
        let plan = plan(text);
        let mut actual = plan
            .root_parse_diagnostics()
            .iter()
            .chain(plan.errors())
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic
                        .start
                        .expect("a located configuration diagnostic"),
                    diagnostic
                        .message_text()
                        .as_str()
                        .expect("scalar message")
                        .to_owned(),
                )
            })
            .collect::<Vec<_>>();
        actual.sort();
        actual.dedup();
        let mut expected = expected
            .iter()
            .map(|(code, start, message)| (*code, *start, (*message).to_owned()))
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(actual, expected, "{text}");
    }
}

/// tsgo drops a command-line-only option's value after TS6266, and checks
/// no option name for a nil value (`null`, or a value JSON has no form for).
#[test]
fn rejected_values_set_no_option() {
    let help = plan(r#"{ "compilerOptions": { "help": true }, "files": ["a.ts"] }"#);
    assert!(help.options().get("help").is_none());
    let strict = plan(r#"{ "compilerOptions": { "strict": [foo] }, "files": ["a.ts"] }"#);
    assert_eq!(
        strict.options().typed_value_state("strict"),
        ConfigOptionValueState::Undefined
    );
}
