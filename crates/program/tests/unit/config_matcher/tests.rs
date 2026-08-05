use super::ConfigFilePattern;

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
