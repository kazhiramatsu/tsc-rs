use tsc_program::ConfigFilePattern;

fn pattern(spec: &str, base: &str) -> ConfigFilePattern {
    ConfigFilePattern::new(spec, base, true)
        .expect("the lexical config path is supported")
        .expect("the files pattern is usable")
}

#[test]
fn config_patterns_match_unc_and_url_roots() {
    let unc = pattern("//server/share/src/**/*.ts", "/unused");
    assert!(unc.matches("//server/share/src/main.ts"));
    assert!(!unc.matches("//other/share/src/main.ts"));

    let url = pattern("src/**/*.json", "https://host/project");
    assert!(url.matches("https://host/project/src/data.json"));
    assert!(!url.matches("https://other/project/src/data.json"));

    let file_url = pattern("file:///c:/project/src/*.ts", "/unused");
    assert!(file_url.matches("file:///c:/project/src/main.ts"));
    assert!(!file_url.matches("file:///d:/project/src/main.ts"));

    // TypeScript compiles root component zero as part of the wildcard regexp,
    // so schemes and UNC server names retain `*`/`?` semantics.
    let wildcard_authority = pattern("https://ho?t/src/*.json", "/unused");
    assert!(wildcard_authority.matches("https://host/src/data.json"));
    assert!(!wildcard_authority.matches("https://ho/st/src/data.json"));

    let wildcard_server = pattern("//*/share/*.ts", "/unused");
    assert!(wildcard_server.matches("//server/share/main.ts"));
    assert!(!wildcard_server.matches("//server/other/main.ts"));
}

#[test]
fn config_patterns_treat_nul_as_a_lexical_character() {
    let nul = pattern("src/a\0*.ts", "/project");
    assert!(nul.matches("/project/src/a\0b.ts"));
    assert!(!nul.matches("/project/src/ab.ts"));
}

#[test]
fn relative_patterns_add_a_separator_to_separator_less_roots() {
    for (base, candidate) in [
        ("https://host", "https://host/src/data.json"),
        ("//server", "//server/src/data.json"),
        ("c:", "c:/src/data.json"),
    ] {
        let relative = pattern("src/*.json", base);
        assert!(relative.matches(candidate), "base {base:?}");
    }
}
