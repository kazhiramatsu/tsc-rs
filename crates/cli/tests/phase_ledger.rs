//! A real CLI/probe smoke test, independent of the benchmark corpora.
#![cfg(all(feature = "phase-ledger", target_os = "macos"))]

use std::{collections::BTreeSet, fs, process::Command};

#[test]
fn six_phases_are_monotonic_and_preserve_cli_output() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("phase-ledger-smoke-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let _cleanup = Cleanup(root.clone());
    fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"noLib":true,"noEmit":true},"files":["tiny.ts"]}"#,
    )
    .unwrap();
    fs::write(
        root.join("tiny.ts"),
        r#"
interface Array<T> { length: number; [n: number]: T; }
interface Boolean {} interface Function {} interface IArguments {}
interface Number {} interface Object {} interface RegExp {} interface String {}
interface CallableFunction extends Function {} interface NewableFunction extends Function {}
const value: number = 1;
"#,
    )
    .unwrap();
    let run = |sidecar: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tsc-rs"));
        command
            .current_dir(&root)
            .args(["-p", "tsconfig.json", "--pretty", "false"])
            .env("TSRS_WORKERS", "1")
            .env("TSRS_CHECKERS", "1")
            .env_remove("TSRS_PHASE_LEDGER")
            .env_remove("TSRS_PHASE_LEDGER_CALIBRATE");
        if sidecar {
            command
                .env("TSRS_PHASE_LEDGER", root.join("ledger.jsonl"))
                .env("TSRS_PHASE_LEDGER_CALIBRATE", "4");
        }
        command.output().unwrap()
    };
    let control = run(false);
    let observed = run(true);
    assert!(
        control.status.success(),
        "{}",
        String::from_utf8_lossy(&control.stdout)
    );
    assert_eq!(control.status.code(), observed.status.code());
    assert_eq!(control.stdout, observed.stdout);
    assert_eq!(control.stderr, observed.stderr);
    let text = fs::read_to_string(root.join("ledger.jsonl")).unwrap();
    let rows: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(rows.iter().all(|row| row.get("error").is_none()), "{text}");
    assert!(rows
        .iter()
        .any(|row| row["kind"] == "calibration" && row["boundaries"] == 8));
    assert!(rows
        .iter()
        .any(|row| row["kind"] == "metadata" && row["serialized"] == true));
    let mut phases = BTreeSet::new();
    let mut previous = [0; 5];
    let keys = [
        "ri_instructions",
        "ri_user_time",
        "ri_system_time",
        "wall_ns",
        "ri_lifetime_max_phys_footprint",
    ];
    for row in rows.iter().filter(|row| row.get("phase").is_some()) {
        let phase = row["phase"].as_str().unwrap();
        phases.insert(phase);
        for (index, key) in keys.iter().enumerate() {
            let end = row[key].as_u64().unwrap();
            if phase != "total" {
                let start = row["start"][key].as_u64().unwrap();
                assert!(start >= previous[index] && end >= start, "{text}");
            }
            assert!(end >= previous[index], "{text}");
            previous[index] = end;
        }
    }
    assert_eq!(
        phases,
        BTreeSet::from([
            "config",
            "load",
            "bind",
            "checker-init",
            "semantic",
            "finalization",
            "total"
        ])
    );
    assert_eq!(rows.last().unwrap()["phase"], "total");
    assert!(previous[0] > 0);
}

struct Cleanup(std::path::PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
