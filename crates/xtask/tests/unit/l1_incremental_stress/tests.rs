use super::*;

#[test]
fn internal_child_flag_is_hidden_and_single_use() {
    let arguments = parse_arguments(
        [
            "--internal-stress-child",
            "--fixture",
            "large.ts",
            "--edits",
            "1",
        ]
        .into_iter()
        .map(str::to_owned),
    )
    .unwrap();
    assert!(arguments.internal_stress_child);
    assert!(parse_arguments(
        [
            "--internal-stress-child",
            "--internal-stress-child",
            "--fixture",
            "large.ts",
        ]
        .into_iter()
        .map(str::to_owned)
    )
    .is_err());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn supported_platform_has_a_high_water_rss_measurement() {
    assert!(peak_rss_bytes().is_some());
}
