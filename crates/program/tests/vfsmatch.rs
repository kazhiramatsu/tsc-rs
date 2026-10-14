//! tsgo `vfs/vfsmatch`'s `SpecMatcher` (cases of tsgo's vfsmatch_test.go,
//! and the absolute exclude patterns of `autoImportFileExcludePatterns`).

use tsc_program::vfsmatch::{SpecMatcher, Usage};

fn matcher(specs: &[&str], base_path: &str, usage: Usage, case_sensitive: bool) -> SpecMatcher {
    SpecMatcher::new(specs, base_path, usage, case_sensitive).expect("a spec compiles")
}

#[test]
fn files_specs_match_their_directory_or_below() {
    let simple = matcher(&["*.ts"], "/project", Usage::Files, true);
    assert!(simple.match_string("/project/a.ts"));
    assert!(!simple.match_string("/project/sub/a.ts"));
    assert!(!simple.match_string("/project/a.js"));
    let recursive = matcher(&["**/*.ts"], "/project", Usage::Files, true);
    assert!(recursive.match_string("/project/a.ts"));
    assert!(recursive.match_string("/project/sub/deep/a.ts"));
    assert!(!recursive.match_string("/project/a.js"));
    let insensitive = matcher(&["*.ts"], "/project", Usage::Files, false);
    assert!(insensitive.match_string("/project/A.TS"));
    assert!(insensitive.match_string("/project/B.Ts"));
}

#[test]
fn exclude_specs_match_what_is_below_a_directory() {
    let exclude = matcher(
        &["node_modules", "bower_components"],
        "/project",
        Usage::Exclude,
        true,
    );
    assert_eq!(exclude.match_index("/project/node_modules"), None);
    assert_eq!(exclude.match_index("/project/node_modules/foo"), Some(0));
    assert_eq!(
        exclude.match_index("/project/bower_components/bar"),
        Some(1)
    );
    assert_eq!(exclude.match_index("/project/src"), None);
    // A trailing `**` compiles only for excludes.
    assert!(SpecMatcher::new(&["**"], "/project", Usage::Files, true).is_none());
    let everything = matcher(&["**"], "/project", Usage::Exclude, true);
    assert!(everything.match_string("/project/anything"));
    assert!(everything.match_string("/project/deep/path"));
}

#[test]
fn index_lookup_prefers_the_first_spec() {
    let specs = matcher(&["*.ts", "*.tsx", "*.js"], "/project", Usage::Files, true);
    assert_eq!(specs.match_index("/project/a.ts"), Some(0));
    assert_eq!(specs.match_index("/project/b.tsx"), Some(1));
    assert_eq!(specs.match_index("/project/c.js"), Some(2));
    assert_eq!(specs.match_index("/project/d.css"), None);
    assert!(SpecMatcher::new::<&str>(&[], "/project", Usage::Files, true).is_none());
}

#[test]
fn dynamic_roots_match_case_sensitively() {
    let root = "^/~ts-uri~/custom/ts-nul-authority";
    let spec = format!("{root}/Foo/**/*.ts");
    let dynamic = matcher(&[spec.as_str()], "/dev", Usage::Files, false);
    assert!(dynamic.match_string(&format!("{root}/Foo/a.ts")));
    assert!(!dynamic.match_string(&format!("{root}/foo/a.ts")));
}

/// `autoImportFileExcludePatterns` compile against no base: an absolute
/// pattern's `/` root is the path's empty root component.
#[test]
fn auto_import_exclude_patterns_without_a_base() {
    let index_files = matcher(&["/**/index.*"], "", Usage::Exclude, true);
    assert!(index_files.match_string("/lib/components/index.ts"));
    assert!(index_files.match_string("/index.ts"));
    assert!(!index_files.match_string("/lib/main.ts"));
    let ambient = matcher(&["/**/ambient1.d.ts"], "", Usage::Exclude, true);
    assert!(ambient.match_string("/ambient1.d.ts"));
    assert!(!ambient.match_string("/ambient2.d.ts"));
    // A relative pattern's root is empty too: it matches at the top only.
    let relative = matcher(&["*ignoreme.ts"], "", Usage::Exclude, true);
    assert!(relative.match_string("/ignoreme.ts"));
    assert!(!relative.match_string("/src/ignoreme.ts"));
}
