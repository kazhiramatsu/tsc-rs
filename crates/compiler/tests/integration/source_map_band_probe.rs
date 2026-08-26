//! H2.6a band burn-down probe (ca-2 tooling, the M2_CASE idiom): run one
//! qualification row through the production emit with the SourceMap
//! settings floor and dump produced vs frozen bytes for offline
//! alignment. `#[ignore]`d; select a row with
//! `TSRS_H2_6A_PROBE_CASE=<case-id substring>` and run with
//! `cargo test -p tsc-rs-compiler --test contracts source_map_band_probe -- --ignored`.
//! Dumps land under `/tmp/h2-6a-probe/`.

use std::path::PathBuf;

use base64::Engine as _;
use serde_json::Value;
use tsc_compiler::ProgramSession;
use tsc_emitter::MemoryOutputSink;
use tsc_harness::upstream_suites::execution::{
    load_qualified_compiler_emit_with_option_floor, EmitOptionFloor,
};
use tsc_program::ProgramLoadLimits;

#[test]
#[ignore = "burn-down probe; select a row with TSRS_H2_6A_PROBE_CASE"]
fn probe_one_band_row() {
    let Some(filter) = std::env::var("TSRS_H2_6A_PROBE_CASE").ok() else {
        panic!("set TSRS_H2_6A_PROBE_CASE to a case-id substring");
    };
    let artifact: Value = serde_json::from_slice(
        &std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../ratchets/h2-6a-qualification.v1.json"),
        )
        .expect("qualification artifact"),
    )
    .expect("qualification JSON");
    let case = artifact["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| {
            case["case_id"]
                .as_str()
                .is_some_and(|case_id| case_id.contains(&filter))
        })
        .unwrap_or_else(|| panic!("no case id contains {filter}"));
    let case_id = case["case_id"].as_str().expect("case id");
    let input = &case["input"];
    let current_directory = input["current_directory"].as_str().expect("cwd");
    let mut files = Vec::new();
    for file in input["files"].as_array().expect("files") {
        files.push((
            PathBuf::from(file["path"].as_str().expect("path")),
            base64::engine::general_purpose::STANDARD
                .decode(file["utf8_base64"].as_str().expect("bytes"))
                .expect("base64"),
        ));
    }
    if let Some(config) = input["virtual_config"].as_object() {
        files.push((
            PathBuf::from(config["path"].as_str().expect("config path")),
            base64::engine::general_purpose::STANDARD
                .decode(config["utf8_base64"].as_str().expect("config bytes"))
                .expect("config base64"),
        ));
    }
    let roots: Vec<PathBuf> = input["roots"]
        .as_array()
        .expect("roots")
        .iter()
        .map(|root| PathBuf::from(root.as_str().expect("root")))
        .collect();
    let settings: Vec<(String, String)> = input["settings"]
        .as_array()
        .expect("settings")
        .iter()
        .map(|setting| {
            (
                setting["name"].as_str().expect("name").to_owned(),
                setting["value"].as_str().expect("value").to_owned(),
            )
        })
        .collect();
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let prepared = load_qualified_compiler_emit_with_option_floor(
        &workspace,
        current_directory,
        &files,
        &roots,
        &settings,
        ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024),
        EmitOptionFloor::SourceMap,
    )
    .expect("prepare probe program");
    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(prepared).emit(&mut sink);
    let out_dir = PathBuf::from("/tmp/h2-6a-probe");
    std::fs::create_dir_all(&out_dir).expect("probe out dir");
    let slug = case_id.replace(['/', '#'], "_");
    for write in sink.writes() {
        let name = write
            .path()
            .file_name()
            .expect("write basename")
            .to_string_lossy()
            .into_owned();
        std::fs::write(
            out_dir.join(format!("{slug}.produced.{name}")),
            write.callback_text(),
        )
        .expect("dump produced");
    }
    for frozen in case["typescript_observation"]["writes"]
        .as_array()
        .expect("frozen writes")
    {
        let path = frozen["path"].as_str().expect("frozen path");
        let name = path.rsplit('/').next().expect("frozen basename");
        std::fs::write(
            out_dir.join(format!("{slug}.frozen.{name}")),
            base64::engine::general_purpose::STANDARD
                .decode(
                    frozen["callback_utf8_base64"]
                        .as_str()
                        .expect("frozen bytes"),
                )
                .expect("frozen base64"),
        )
        .expect("dump frozen");
    }
    println!("probe {case_id}: outcome {:?}", outcome.map(|_| "ok"));
    println!("dumps under {}", out_dir.display());
}
