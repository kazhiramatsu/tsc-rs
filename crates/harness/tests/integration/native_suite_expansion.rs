//! The native (TypeScript 7.x) harness rules reproduce the reference
//! baselines' configuration set of the vendored profile: every configuration
//! with a baseline is one the port predicts to run, and every configuration
//! the port predicts to run has a baseline unless it produces no output.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};
use tsc_harness::upstream_suites::execution::{
    native_compiler_fixture, native_compiler_plan, CompilerRootSelection, CompilerUnitId,
};
use tsc_harness::upstream_suites::native::{
    baseline_stems, expand_case, NativeCase, NativeConfiguration, NativeProfile, NativeSkip,
    NativeSuite,
};

const PROFILE: &str = "7.1.0-dev-19dadef8";

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn native_expansion_reproduces_the_reference_baseline_configurations() {
    let profile = NativeProfile::load(&workspace(), PROFILE).expect("native profile");
    let cases = profile.cases().expect("native cases");
    assert_eq!(
        cases.len(),
        6_838 + 5_910,
        "TestLocal enumerates every .ts/.tsx case"
    );

    let mut file_names = BTreeSet::new();
    for case in &cases {
        assert!(
            file_names.insert(case.file_name().to_owned()),
            "{}: duplicate basename",
            case.relative_path
        );
    }

    let names = profile.baseline_names().expect("baseline names");
    let mut run = BTreeMap::new();
    let mut quiet_allowed = BTreeSet::new();
    let mut skipped = BTreeMap::<String, usize>::new();
    let mut failures = Vec::new();
    for case in &cases {
        let expansion = match expand_case(&profile, case) {
            Ok(expansion) => expansion,
            Err(error) => {
                failures.push(format!("{}: {error}", case.relative_path));
                continue;
            }
        };
        for (configuration, stem, skip) in expansion.configurations {
            match skip {
                None => {
                    if configuration
                        .settings
                        .get("notypesandsymbols")
                        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
                    {
                        quiet_allowed.insert((case.suite, stem.clone()));
                    }
                    run.insert((case.suite, stem), case.relative_path.clone());
                }
                Some(NativeSkip::Fatal(rule)) => {
                    failures.push(format!("{} ({stem}): fatal {rule}", case.relative_path));
                }
                Some(NativeSkip::UnknownDirective(name)) => {
                    failures.push(format!(
                        "{} ({stem}): unknown directive {name}",
                        case.relative_path
                    ));
                }
                Some(other) => *skipped.entry(format!("{other:?}")).or_default() += 1,
            }
        }
    }

    let mut observed = BTreeSet::new();
    for suite in NativeSuite::ALL {
        for stem in baseline_stems(&names, suite).into_keys() {
            observed.insert((suite, stem));
        }
    }
    let unexpected: Vec<_> = observed
        .iter()
        .filter(|key| !run.contains_key(*key))
        .collect();
    let silent: Vec<_> = run
        .keys()
        .filter(|key| !observed.contains(*key) && !quiet_allowed.contains(*key))
        .collect();
    println!(
        "native expansion: {} cases, {} configurations run, {} observed, skipped {:?}",
        cases.len(),
        run.len(),
        observed.len(),
        skipped
    );
    for line in failures.iter().take(20) {
        println!("failure: {line}");
    }
    for key in unexpected.iter().take(20) {
        println!("baseline without a predicted run: {:?}", key);
    }
    for key in silent.iter().take(20) {
        println!("predicted run without baselines: {:?} ({})", key, run[*key]);
    }
    assert!(
        failures.is_empty(),
        "{} cases failed to expand",
        failures.len()
    );
    assert!(
        unexpected.is_empty(),
        "{} baseline configurations were not predicted",
        unexpected.len()
    );
    assert!(
        silent.is_empty(),
        "{} predicted configurations have no baseline",
        silent.len()
    );
}

/// Every vendored file, with its Git blob id and mode, under `root/path`
/// (only `*.errors.txt` when the set is filtered; the file itself when the
/// set vendors one file), sorted by upstream path.
/// The vendored baseline kinds, longest suffix first so `.js.map` is not
/// taken for `.js`.
const BASELINE_SUFFIXES: [&str; 7] = [
    ".sourcemap.txt",
    ".trace.json",
    ".errors.txt",
    ".symbols",
    ".js.map",
    ".types",
    ".js",
];

fn baseline_kind(name: &str) -> Option<&'static str> {
    BASELINE_SUFFIXES
        .into_iter()
        .find(|suffix| name.ends_with(suffix))
}

fn inventory(upstream: &Path, path: &str, filter: Option<&str>) -> Vec<(String, String, String)> {
    let mut files = Vec::new();
    let mut stack = Vec::new();
    if upstream.join(path).is_file() {
        files.push(upstream.join(path));
    } else {
        stack.push(upstream.join(path));
    }
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("vendored directory") {
            let entry = entry.expect("directory entry");
            let full = entry.path();
            if full.is_dir() {
                stack.push(full);
            } else if filter.is_none_or(|filter| {
                baseline_kind(&full.to_string_lossy()) == Some(filter.trim_start_matches('*'))
            }) {
                files.push(full);
            }
        }
    }
    let mut hash = Command::new("git")
        .args(["hash-object", "--no-filters", "--stdin-paths"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("git hash-object");
    let paths: String = files
        .iter()
        .map(|file| format!("{}\n", file.display()))
        .collect();
    let mut stdin = hash.stdin.take().expect("stdin");
    let writer = std::thread::spawn(move || stdin.write_all(paths.as_bytes()));
    let output = hash.wait_with_output().expect("git hash-object output");
    writer.join().expect("writer").expect("write paths");
    assert!(output.status.success());
    let blobs = String::from_utf8(output.stdout).expect("blob ids");
    let mut rows: Vec<_> = files
        .iter()
        .zip(blobs.lines())
        .map(|(file, blob)| {
            let mode = if file.metadata().expect("metadata").permissions().mode() & 0o111 != 0 {
                "100755"
            } else {
                "100644"
            };
            let name = file
                .strip_prefix(upstream)
                .expect("under the upstream root")
                .to_string_lossy()
                .into_owned();
            (mode.to_owned(), blob.to_owned(), name)
        })
        .collect();
    rows.sort_by(|left, right| left.2.cmp(&right.2));
    rows
}

#[test]
fn native_vendored_inputs_match_the_manifest() {
    let profile = NativeProfile::load(&workspace(), PROFILE).expect("native profile");
    let manifest = &profile.manifest;
    assert_eq!(manifest.schema, 1);
    assert_eq!(
        manifest.repository,
        "https://github.com/microsoft/TypeScript.git"
    );
    assert_eq!(manifest.commit, "19dadef8888ba5b27d8b9f622480745cf623e020");
    assert_eq!(manifest.profile, PROFILE);
    let upstream = profile.upstream_root();
    let mut vendored = 0;
    for set in &manifest.sets {
        let rows = inventory(&upstream, &set.path, set.filter.as_deref());
        let text: String = rows
            .iter()
            .map(|(mode, blob, name)| format!("{mode} {blob} {name}\n"))
            .collect();
        assert_eq!(rows.len() as u64, set.files, "{}: file count", set.path);
        assert_eq!(
            format!("{:x}", Sha256::digest(text.as_bytes())),
            set.blob_inventory_sha256,
            "{}: blob inventory",
            set.path
        );
        if let Some(blob) = &set.git_blob_sha1 {
            assert_eq!(
                rows.iter()
                    .map(|(_, blob, _)| blob.as_str())
                    .collect::<Vec<_>>(),
                [blob.as_str()],
                "{}: single-file set",
                set.path
            );
        }
        vendored += rows.len();
    }
    // 32,680 inputs of the first profile plus the 12,779 `.types` and 12,779
    // `.symbols` baselines vendored for the type and symbol comparison, the
    // 148 `.trace.json` baselines of the trace comparison, the 25 cases and
    // 41 baselines of the transpile suite, and the 80 command-line and 87
    // tsconfig parsing baselines.
    assert_eq!(vendored, 58_619);
    assert!(profile.diagnostic_messages_path().is_file());
    assert!(profile
        .bundled_libraries_root()
        .join("lib.es2026.d.ts")
        .is_file());
    let names = profile.baseline_names().expect("baseline names");
    assert_eq!(names.len() as u64, manifest.baseline_names.entries);
    let text: String = names.iter().map(|name| format!("{name}\n")).collect();
    assert_eq!(
        format!("{:x}", Sha256::digest(text.as_bytes())),
        manifest.baseline_names.sha256
    );
}

#[test]
fn a_unit_named_like_the_last_one_is_written_after_it_like_tsgo() {
    // Go's parser keeps every unit's text in a string builder, so the first
    // `file3.ts` of augmentExportEquals2 is an empty file
    // (test_case_parser.go:199-216), and the compiler runner compiles the
    // last unit while every other one, also a unit of the same name, is
    // written after it (compiler_runner.go:320-323, harnessutil.go:194-205).
    let profile = NativeProfile::load(&workspace(), PROFILE).expect("native profile");
    let case = NativeCase {
        suite: NativeSuite::Compiler,
        relative_path: "augmentExportEquals2.ts".to_owned(),
    };
    let fixture = native_compiler_fixture(&profile, &case).expect("fixture");
    let units: Vec<_> = fixture
        .units
        .iter()
        .map(|unit| {
            (
                unit.name.as_ref().to_owned(),
                unit.content.as_deref().map(str::len),
            )
        })
        .collect();
    assert_eq!(units[2], ("file3.ts".to_owned(), Some(0)));
    assert_eq!(units[3].0, "file3.ts");
    let configuration = NativeConfiguration {
        name: String::new(),
        settings: BTreeMap::new(),
    };
    let plan = native_compiler_plan(fixture, &configuration).expect("plan");
    let CompilerRootSelection::Explicit {
        root_units,
        other_units,
        ..
    } = &plan.root_selection
    else {
        panic!("augmentExportEquals2 has no tsconfig");
    };
    assert_eq!(root_units.as_ref(), [CompilerUnitId(3)]);
    assert_eq!(
        other_units.as_ref(),
        [CompilerUnitId(0), CompilerUnitId(1), CompilerUnitId(2)]
    );
}
