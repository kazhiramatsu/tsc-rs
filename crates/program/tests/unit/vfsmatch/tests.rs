use super::*;

#[test]
fn directory_selections_preserve_full_exclusion_matches() {
    let specs = [
        "dist/**/*",
        "*.d.ts",
        "future/*.d.ts",
        "image-types/global.d.ts",
        "src/**/test?.ts",
        "**/*.generated.ts",
        "literal.json",
        "K-dir/*.ts",
        "σ-dir/*.ts",
        "𐐀-dir/*.ts",
        "src-*/?/only.ts",
        "short?/deep/*.ts",
    ];
    let directories = [
        "/work",
        "/work/src",
        "/work/src/deep",
        "/work/dist",
        "/work/dist/deep",
        "/work/image-types",
        "/work/other",
        "/work/k-dir",
        "/work/ς-dir",
        "/work/𐐨-dir",
        "/work/src-a/δ",
        "/work/src-a/ab",
        "/work/shortδ/deep",
        "/work/one.d.ts/deep",
    ];
    let names = [
        "index.ts",
        "index.d.ts",
        "global.d.ts",
        "testA.ts",
        "x.generated.ts",
        ".hidden.ts",
        "x.min.js",
        "literal.json",
        "only.ts",
    ];
    for case_sensitive in [false, true] {
        let matcher = SpecMatcher::new(&specs, "/work", Usage::Exclude, case_sensitive).unwrap();
        for directory in directories {
            let mut selection = matcher.relevant_to_directory("/", None);
            let mut prefix = String::new();
            for component in directory.split('/').filter(|part| !part.is_empty()) {
                prefix.push('/');
                prefix.push_str(component);
                selection = matcher.relevant_to_directory(&prefix, Some(&selection));
            }
            for name in names {
                let file = format!("{directory}/{name}");
                assert_eq!(
                    matcher.match_relevant(&file, &selection),
                    matcher.match_string(&file),
                    "{file}, case_sensitive={case_sensitive}"
                );
            }
        }
    }
}

#[test]
fn completed_directory_selections_share_their_indices() {
    let matcher = SpecMatcher::new(
        &["src/**/only.ts", "src/**/*.test.ts", "unrelated/*.ts"],
        "/work",
        Usage::Exclude,
        true,
    )
    .unwrap();
    let root = matcher.relevant_to_directory("/work", None);
    assert!(root.can_narrow());
    let source = matcher.relevant_to_directory("/work/src", Some(&root));
    assert!(!source.can_narrow());
    assert!(!source.is_empty());
    let child = matcher.relevant_to_directory("/work/src/deep", Some(&source));
    assert!(Arc::ptr_eq(
        source.indices.as_ref().unwrap(),
        child.indices.as_ref().unwrap()
    ));
    let other = matcher.relevant_to_directory("/work/other", Some(&root));
    assert!(other.is_empty());
    assert!(!other.can_narrow());
    assert!(matcher
        .relevant_to_directory("/work/other/deep", Some(&other))
        .is_empty());
}

#[test]
fn unrestricted_exclusions_need_no_selection_storage() {
    let matcher = SpecMatcher::new(
        &["**/*.test.ts", "**/test/**"],
        "/work",
        Usage::Exclude,
        true,
    )
    .unwrap();
    let root = matcher.relevant_to_directory("/work", None);
    assert!(root.all);
    assert!(root.indices.is_none());
    assert!(!root.is_empty());
    assert!(!root.can_narrow());
    let child = matcher.relevant_to_directory("/work/src/deep", Some(&root));
    assert!(child.all);
    assert!(child.indices.is_none());
    for file in [
        "/work/src/index.ts",
        "/work/src/index.test.ts",
        "/work/test/a.ts",
    ] {
        assert_eq!(
            matcher.match_relevant(file, &child),
            matcher.match_string(file)
        );
    }
}

#[test]
fn dynamic_directory_selections_keep_the_case_sensitive_root() {
    let base = "^/~ts-uri~/custom/ts-nul-authority";
    let spec = format!("{base}/Foo/**/*.ts");
    let matcher = SpecMatcher::new(&[spec], "/dev", Usage::Exclude, false).unwrap();
    assert!(matcher.match_string(&format!("{base}/Foo/a.ts")));
    assert!(!matcher.match_string(&format!("{base}/foo/a.ts")));
    let selection = matcher.relevant_to_directory(base, None);
    for directory in [format!("{base}/Foo"), format!("{base}/foo")] {
        let child = matcher.relevant_to_directory(&directory, Some(&selection));
        let file = format!("{directory}/a.ts");
        assert_eq!(
            matcher.match_relevant(&file, &child),
            matcher.match_string(&file)
        );
    }
}
