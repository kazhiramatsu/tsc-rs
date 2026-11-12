use super::ConfigFilePattern;

#[test]
fn arbitrary_utf16_specs_and_candidates_match_typescript() {
    use tsc_diagnostics::JsString;
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../fixtures/utf16-config-matching.json")).unwrap();
    let value = |units: &serde_json::Value| -> JsString {
        units
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
            .collect()
    };
    let paths: Vec<_> = fixture["paths_utf16"]
        .as_array()
        .unwrap()
        .iter()
        .map(value)
        .collect();
    for case in fixture["matching"].as_array().unwrap() {
        let spec = value(&case["spec_utf16"]);
        let pattern = ConfigFilePattern::new(
            &spec,
            case["base"].as_str().unwrap(),
            case["case_sensitive"].as_bool().unwrap(),
        )
        .unwrap();
        assert_eq!(
            pattern.is_some(),
            case["expected"]["compiled"].as_bool().unwrap(),
            "{}",
            case["case_id"]
        );
        for (path, expected) in paths
            .iter()
            .zip(case["expected"]["matches"].as_array().unwrap())
        {
            assert_eq!(
                pattern
                    .as_ref()
                    .is_some_and(|pattern| pattern.matches(path)),
                expected.as_bool().unwrap(),
                "{}: spec={:?} path={:?}",
                case["case_id"],
                spec,
                path
            );
        }
    }
}

#[test]
fn filename_case_folding_preserves_surrogates_and_context_boundaries() {
    use tsc_diagnostics::JsString;
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../fixtures/utf16-config-matching.json")).unwrap();
    for case in fixture["folding"].as_array().unwrap() {
        let value: JsString = case["value_utf16"]
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
            .collect();
        let expected: Vec<u16> = case["expected"]
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
            .collect();
        assert_eq!(
            crate::js_path::file_name_lower_case(value.as_js()).to_utf16(),
            expected,
            "{:?}",
            value
        );
    }
}

fn pattern(spec: &str) -> ConfigFilePattern {
    ConfigFilePattern::new(spec, "/work", true)
        .expect("valid pattern")
        .expect("usable files pattern")
}

#[test]
fn normalizes_paths_and_expands_implicit_directory_globs() {
    let pattern = pattern("./src/../src");
    assert!(pattern.matches("/work/src/index.ts"));
    assert!(pattern.matches("/work/src/nested/index.ts"));
    assert!(!pattern.matches("/work/index.ts"));

    let drive = ConfigFilePattern::new("../src/*.TS", "C:/Project/config", false)
        .expect("valid drive pattern")
        .expect("usable drive pattern");
    assert!(drive.matches("c:\\project\\SRC\\main.ts"));
}

#[test]
fn recursive_wildcard_excludes_implicit_directories() {
    let selection = pattern("src/**/*.ts");
    assert!(selection.matches("/work/src/nested/index.ts"));
    assert!(!selection.matches("/work/src/.cache/index.ts"));
    assert!(!selection.matches("/work/src/node_modules/pkg/index.ts"));

    let explicit = pattern("src/node_modules/**/*.ts");
    assert!(explicit.matches("/work/src/node_modules/pkg/index.ts"));
}

#[test]
fn directory_pruning_preserves_explicit_package_includes() {
    let implicit = pattern("**/*.ts");
    assert!(!implicit.could_match_descendant("/work/node_modules"));
    assert!(!implicit.could_match_descendant("/work/.cache"));
    assert!(implicit.could_match_descendant("/work/src"));

    let explicit = pattern("node_modules/**/*.ts");
    assert!(explicit.could_match_descendant("/work/node_modules"));
    assert!(explicit.could_match_descendant("/work/node_modules/pkg"));
    assert!(!explicit.could_match_descendant("/work/src"));
}

#[test]
fn component_wildcards_preserve_dot_package_and_min_js_rules() {
    let javascript = pattern("src/*.js");
    assert!(javascript.matches("/work/src/main.js"));
    assert!(!javascript.matches("/work/src/.hidden.js"));
    assert!(!javascript.matches("/work/src/main.min.js"));

    assert!(pattern("src/.*.js").matches("/work/src/.hidden.js"));
    assert!(pattern("src/*.min.js").matches("/work/src/main.min.js"));
    assert!(pattern("src/*.*").matches("/work/src/.hidden.js"));
    assert!(pattern("src/*.*").matches("/work/src/main.min.js"));
    assert!(!pattern("src/*/*.ts").matches("/work/src/node_modules/index.ts"));

    let implicit = pattern("src");
    assert!(!implicit.matches("/work/src/.hidden.ts"));
    assert!(!implicit.matches("/work/src/main.min.js"));
    assert!(pattern(".dir/**/*.ts").matches("/work/.dir/main.ts"));
}

#[test]
fn only_a_whole_component_double_star_is_recursive() {
    assert!(ConfigFilePattern::new("src/**", "/work", true)
        .expect("valid pattern")
        .is_none());

    let ordinary = pattern("src/**name.ts");
    assert!(ordinary.matches("/work/src/long-name.ts"));
    assert!(!ordinary.matches("/work/src/nested/long-name.ts"));
}

#[test]
fn question_mark_matches_one_character_with_the_host_case_profile() {
    let sensitive = pattern("src/file?.ts");
    assert!(sensitive.matches("/work/src/file1.ts"));
    assert!(!sensitive.matches("/work/src/file10.ts"));
    assert!(!sensitive.matches("/WORK/src/file1.ts"));

    let insensitive = ConfigFilePattern::new("src/file?.TS", "/work", false)
        .expect("valid pattern")
        .expect("usable files pattern");
    assert!(insensitive.matches("/WORK/SRC/FILE1.ts"));

    let protected = ConfigFilePattern::new("İ/*.TS", "/work", false)
        .expect("valid protected-case pattern")
        .expect("usable protected-case pattern");
    assert!(protected.matches("/WORK/İ/FILE.ts"));
    assert!(!protected.matches("/work/i/file.ts"));

    let insensitive_component = |component: &str| {
        ConfigFilePattern::new(&format!("{component}/*.ts"), "/work", false)
            .expect("valid Unicode case pattern")
            .expect("usable Unicode case pattern")
    };
    assert!(!insensitive_component("K").matches("/work/k/file.ts"));
    assert!(insensitive_component("Σ").matches("/work/ς/file.ts"));
    assert!(!insensitive_component("ẞ").matches("/work/ß/file.ts"));

    assert!(!pattern("src/?.ts").matches("/work/src/💩.ts"));
    assert!(pattern("src/??.ts").matches("/work/src/💩.ts"));
    assert!(pattern("src/💩.ts").matches("/work/src/💩.ts"));
}

#[test]
fn relative_patterns_can_select_files_outside_the_config_base() {
    let outside = pattern("../shared/**/*.ts");
    assert!(outside.matches("/shared/nested/main.ts"));
    assert!(!outside.matches("/work/shared/main.ts"));
}

/// The walk matches each entry from its directory's automaton states; those
/// answers are exactly the whole-path answers of `matches` and
/// `could_match_descendant`.
#[test]
fn per_directory_states_agree_with_whole_path_matching() {
    use super::{InputComponent, MatchInput};

    let patterns = [
        pattern("src/**/*.ts"),
        pattern("**/*.test.ts"),
        pattern("lib/*/index.ts"),
        pattern("node_modules/**/*.ts"),
        pattern("src/**/*.min.js"),
        pattern("src/exact.ts"),
        pattern("*/a?c.ts"),
    ];
    let paths = [
        "/work/src/index.ts",
        "/work/src/exact.ts",
        "/work/src/nested/deep/x.test.ts",
        "/work/src/node_modules/pkg/index.ts",
        "/work/lib/a/index.ts",
        "/work/lib/a/b/index.ts",
        "/work/lib/index.ts",
        "/work/node_modules/pkg/index.ts",
        "/work/src/.cache/y.ts",
        "/work/src/vendor/lib.min.js",
        "/work/src/app.min.js",
        "/work/src/abc.ts",
        "/work/other/abc.ts",
        "/elsewhere/src/index.ts",
    ];
    for pattern in &patterns {
        for path in paths {
            let (directory, name) = path.rsplit_once('/').expect("absolute path");
            let states = MatchInput::new(directory.into(), true)
                .and_then(|input| pattern.directory_states(&input));
            let entry = InputComponent::new(name.into(), true);
            let by_states = states
                .as_ref()
                .is_some_and(|states| pattern.accepts_entry(states, &entry));
            assert_eq!(by_states, pattern.matches(path), "{path}");

            if let Some((parent, directory_name)) = directory.rsplit_once('/') {
                if parent.is_empty() {
                    continue;
                }
                let stepped = MatchInput::new(parent.into(), true)
                    .and_then(|input| pattern.directory_states(&input))
                    .and_then(|states| {
                        pattern.advance_directory(
                            &states,
                            &InputComponent::new(directory_name.into(), true),
                        )
                    });
                assert_eq!(
                    stepped.is_some(),
                    pattern.could_match_descendant(directory),
                    "{directory}"
                );
            }
        }
    }
}
#[test]
fn directory_entry_acceptance_matches_the_whole_path_matcher() {
    use super::{InputComponent, MatchInput};
    for case_sensitive in [false, true] {
        for base in ["/project", "c:/project", "file://host/project"] {
            for spec in [
                "**/*.ts",
                "**/**/foo*.min.js",
                "node_modules/pkg/*.d.ts",
                "a/**/b/*",
                ".hidden/*.ts",
                "**/.dot.ts",
            ] {
                let pattern = ConfigFilePattern::new(spec, base, case_sensitive)
                    .unwrap()
                    .unwrap();
                for directory in ["", "/a", "/a/b", "/a/b/c", "/.hidden", "/node_modules/pkg"] {
                    let directory = format!("{base}{directory}");
                    let input = MatchInput::new(directory.as_str().into(), case_sensitive).unwrap();
                    let states = pattern.directory_states(&input);
                    for name in [
                        "index.ts",
                        "INDEX.TS",
                        ".dot.ts",
                        "foo.min.js",
                        "é.ts",
                        "node_modules",
                    ] {
                        let path = format!("{directory}/{name}");
                        let name = InputComponent::new(name.into(), case_sensitive);
                        let actual = states
                            .as_ref()
                            .is_some_and(|states| pattern.accepts_entry(states, &name));
                        assert_eq!(
                            actual,
                            pattern.matches(path.as_str()),
                            "{spec} {path} {case_sensitive}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn wildcard_cursor_matches_the_original_component_dp() {
    use super::{min_js_dot_index, regex_code_unit_eq, GlobComponent, GlobToken, InputComponent};
    fn old(glob: &GlobComponent, input: &InputComponent, last: bool, case_sensitive: bool) -> bool {
        if glob.has_wildcard && input.common_package_folder {
            return false;
        }
        let input_count = input.characters.len();
        let min_js_dot = last
            .then(|| min_js_dot_index(&input.characters, case_sensitive))
            .flatten();
        // The DP rows live on the stack for every ordinary component name.
        const INLINE_ROW: usize = 128;
        let mut inline_rows = [[false; INLINE_ROW]; 2];
        let mut heap_rows = Vec::new();
        let (mut previous, mut current): (&mut [bool], &mut [bool]) = if input_count < INLINE_ROW {
            let [first, second] = &mut inline_rows;
            (&mut first[..=input_count], &mut second[..=input_count])
        } else {
            heap_rows.resize(2 * (input_count + 1), false);
            heap_rows.split_at_mut(input_count + 1)
        };
        previous[0] = true;

        for (token_index, token) in glob.tokens.iter().enumerate() {
            current.fill(false);
            match token {
                GlobToken::Literal(literal) => {
                    for input_index in 1..=input_count {
                        current[input_index] = previous[input_index - 1]
                            && regex_code_unit_eq(
                                *literal,
                                input.characters[input_index - 1],
                                case_sensitive,
                            );
                    }
                }
                GlobToken::Question => {
                    for input_index in 1..=input_count {
                        let character = input.characters[input_index - 1];
                        current[input_index] = previous[input_index - 1]
                            && character != u16::from(b'/')
                            && !(token_index == 0 && character == u16::from(b'.'));
                    }
                }
                GlobToken::Star => {
                    current[0] = previous[0];
                    for input_index in 1..=input_count {
                        let character_index = input_index - 1;
                        let may_consume = input.characters[character_index] != u16::from(b'/')
                            && !(token_index == 0
                                && character_index == 0
                                && input.characters[character_index] == u16::from(b'.'))
                            && min_js_dot != Some(character_index);
                        current[input_index] =
                            previous[input_index] || (current[input_index - 1] && may_consume);
                    }
                }
            }
            std::mem::swap(&mut previous, &mut current);
        }

        previous[input_count]
    }
    let mut patterns = vec![String::new()];
    for _ in 0..4 {
        let previous = patterns.clone();
        patterns.extend(previous.into_iter().flat_map(|prefix| {
            ['a', '.', '/', '*', '?']
                .into_iter()
                .map(move |c| format!("{prefix}{c}"))
        }));
    }
    patterns.sort();
    patterns.dedup();
    patterns.extend(
        [
            "*.min.js", "*.js", "*.*", "**a", "?*.js", "K*", "𐀀?*", "*İ*", "file://*",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    let inputs = [
        "",
        "a",
        ".",
        "a.a",
        ".a",
        "aa",
        "/a",
        "a/b",
        "a/",
        "aa.min.js",
        "a.a.min.js",
        "a.min.js",
        "A.MIN.JS",
        ".min.js",
        "a.min.JSX",
        "a..min.js",
        "node_modules",
        "NODE_MODULES",
        "İx",
        "Kx",
        "kx",
        "𐀀a",
        "file://host",
        "áÁ",
    ];
    for pattern in patterns {
        let glob = GlobComponent::compile(pattern.as_str().into());
        for name in inputs {
            for case_sensitive in [false, true] {
                let input = InputComponent::new(name.into(), case_sensitive);
                for last in [false, true] {
                    assert_eq!(
                        glob.matches(&input, last, case_sensitive),
                        old(&glob, &input, last, case_sensitive),
                        "{pattern:?} {name:?} last={last} case={case_sensitive}"
                    );
                }
            }
        }
    }
}

#[test]
fn package_folders_keep_non_unicode_regexp_case_rules() {
    use super::{regex_canonicalize_code_unit, regex_code_unit_eq, InputComponent};
    for left in 0_u16..128 {
        for right in 0_u16..128 {
            assert_eq!(
                regex_code_unit_eq(left, right, false),
                regex_canonicalize_code_unit(left) == regex_canonicalize_code_unit(right)
            );
        }
    }
    for name in [
        "node_modules",
        "NODE_MODULES",
        "bower_components",
        "JSPM_PACKAGES",
    ] {
        assert!(InputComponent::new(name.into(), false).common_package_folder);
    }
    for name in ["node_moduleſ", "jspm_pacKages"] {
        assert!(!InputComponent::new(name.into(), false).common_package_folder);
    }
    assert!(!InputComponent::new("NODE_MODULES".into(), true).common_package_folder);
}
