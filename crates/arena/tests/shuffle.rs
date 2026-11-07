#![cfg(feature = "arena-shuffle")]

use std::process::Command;
use tsc_arena::{owner_of, SharedArena, BLOCK_BYTES};

#[test]
fn shuffle_probe() {
    if std::env::var_os("TSRS_ARENA_SHUFFLE_PROBE").is_none() {
        return;
    }
    let arena = SharedArena::new(41);
    let mut handles = Vec::new();
    for _ in 0..8 {
        let r = arena.alloc([9u64; BLOCK_BYTES / 8]);
        assert_eq!(owner_of(r.raw()), Some(41));
        assert_eq!(r.get()[0], 9);
        handles.push(r.raw());
    }
    println!("handles={handles:?}");
}

#[test]
fn two_seed_processes_issue_different_handles() {
    let mut sequences = Vec::new();
    for seed in [17, 89] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "shuffle_probe",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("TSRS_ARENA_SHUFFLE_PROBE", "1")
            .env("TSRS_ARENA_SHUFFLE_SEED", seed.to_string())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let handles = stdout
            .lines()
            .find_map(|s| s.split_once("handles=").map(|(_, h)| h.to_owned()))
            .unwrap();
        println!("seed {seed}: {handles}");
        sequences.push(handles);
    }
    assert_ne!(sequences[0], sequences[1]);
}
