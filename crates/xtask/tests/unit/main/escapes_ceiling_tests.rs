use super::*;

#[test]
fn parses_the_escapes_section() {
    let dir = std::env::temp_dir().join(format!("tsc-rs-ceiling-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("ratchet.toml"),
        "[t0]\nrate = 0.1\n\n[escapes]\n# comment\nmax_untagged = 178\nmax_recovery = 71\n",
    )
    .unwrap();
    assert_eq!(
        read_ratchet_ceiling(&dir, "escapes", "max_untagged").unwrap(),
        Some(178)
    );
    assert_eq!(
        read_ratchet_ceiling(&dir, "escapes", "max_recovery").unwrap(),
        Some(71)
    );
    fs::write(dir.join("ratchet.toml"), "[t0]\nrate = 0.1\n").unwrap();
    assert_eq!(
        read_ratchet_ceiling(&dir, "escapes", "max_untagged").unwrap(),
        None
    );
    assert_eq!(
        read_ratchet_ceiling(&dir, "escapes", "max_recovery").unwrap(),
        None
    );
    fs::remove_dir_all(&dir).ok();
}
