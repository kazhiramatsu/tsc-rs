//! Offline compile-fail harness: actual rustc diagnostics, no network or cargo
//! subprocesses. trybuild is unavailable in this lane's dependency cache.
use std::path::{Path, PathBuf};
use std::process::Command;

fn library(deps: &Path) -> PathBuf {
    std::fs::read_dir(deps)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("libtsc_arena-")
                && p.extension().is_some_and(|s| s == "rlib")
        })
        .max_by_key(|p| p.metadata().unwrap().modified().unwrap())
        .expect("cargo-built tsc_arena rlib")
}

#[test]
fn ui_contracts() {
    let executable = std::env::current_exe().unwrap();
    let deps = executable.parent().unwrap();
    let library = library(deps);
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
            .arg("-L")
            .arg(format!("dependency={}", deps.display()))
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
