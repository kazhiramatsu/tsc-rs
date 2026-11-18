//! Offline compile-fail harness: actual rustc diagnostics, no network or cargo
//! subprocesses. trybuild is unavailable in this lane's dependency cache.
use std::path::{Path, PathBuf};
use std::process::Command;

/// The newest cargo-built `tsc_arena` library and every directory of Cargo's
/// build directory that holds compiled libraries. Up to Cargo 1.99 all of them
/// lie beside the test executable (`<profile>/deps`); from 1.100 every package
/// has a directory of its own (`<profile>/build/<package>/<hash>/out`). Both
/// are found from the build directory's root, the one with `CACHEDIR.TAG`.
fn libraries(executable: &Path) -> (PathBuf, Vec<PathBuf>) {
    let root = executable
        .ancestors()
        .skip(1)
        .find(|directory| {
            directory
                .parent()
                .is_some_and(|parent| parent.join("CACHEDIR.TAG").is_file())
        })
        .unwrap_or_else(|| executable.parent().unwrap())
        .to_path_buf();
    let mut directories = Vec::new();
    let mut arena: Option<(std::time::SystemTime, PathBuf)> = None;
    let mut pending = vec![(root, 0usize)];
    while let Some((directory, depth)) = pending.pop() {
        // Other test binaries may remove their scratch directories meanwhile.
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        let mut holds_library = false;
        for path in entries.filter_map(|entry| entry.ok().map(|entry| entry.path())) {
            if path.is_dir() {
                if depth < 5 && path.file_name().is_some_and(|name| name != "incremental") {
                    pending.push((path, depth + 1));
                }
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rlib") {
                continue;
            }
            holds_library = true;
            let is_arena = path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("libtsc_arena-"));
            if !is_arena {
                continue;
            }
            let Ok(modified) = path.metadata().and_then(|metadata| metadata.modified()) else {
                continue;
            };
            if arena.as_ref().is_none_or(|(newest, _)| modified > *newest) {
                arena = Some((modified, path));
            }
        }
        if holds_library {
            directories.push(directory);
        }
    }
    (arena.expect("cargo-built tsc_arena rlib").1, directories)
}

#[test]
fn ui_contracts() {
    let executable = std::env::current_exe().unwrap();
    let (library, directories) = libraries(&executable);
    let destination = std::env::temp_dir().join(format!("tsc-arena-ui-{}", std::process::id()));
    std::fs::create_dir_all(&destination).unwrap();
    let cases = [
        (
            "cell_in_shared.rs",
            false,
            "cannot be shared between threads safely",
        ),
        ("drop_field.rs", false, "String: Record"),
        ("custom_drop.rs", false, "Record must not have Drop glue"),
        ("padded_record.rs", false, "Record must not have padding"),
        ("scratch_escape.rs", false, "does not live long enough"),
        ("scratch_cell_escape.rs", false, "does not live long enough"),
        (
            "scratch_early_drop.rs",
            false,
            "cannot move out of `scope` because it is borrowed",
        ),
        (
            "ref_cell_send.rs",
            false,
            "cannot be shared between threads safely",
        ),
        ("pass_records.rs", true, ""),
        ("facade_contract_is_not_enforced.rs", true, ""),
    ];
    for (case, success, diagnostic) in cases {
        let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg("--edition=2021")
            .arg("--crate-type=bin")
            .arg("--emit=metadata")
            .arg("--extern")
            .arg(format!("tsc_arena={}", library.display()))
            .args(directories.iter().flat_map(|directory| {
                [
                    "-L".to_owned(),
                    format!("dependency={}", directory.display()),
                ]
            }))
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/ui")
                    .join(case),
            )
            .arg("-o")
            .arg(destination.join(format!("{case}.rmeta")))
            .output()
            .expect("run rustc");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.success(), success, "{case}: {stderr}");
        assert!(
            stderr.contains(diagnostic),
            "{case}: expected {diagnostic:?}, got {stderr}"
        );
        println!("{case}: {}", if success { "compiled" } else { diagnostic });
    }
    std::fs::remove_dir_all(destination).unwrap();
}
