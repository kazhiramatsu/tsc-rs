//! H2.6a / m-3 emit witness gate (packet §4): every W-H2.6A witness
//! case reproduces through the PRODUCTION emit path —
//! `ProgramSession::emit` over a memory sink, no harness bridge and no
//! test-local URL assembly. Parity-lane cases compare every write's
//! path/bytes/BOM/source-files/callback-`data`, the write ORDER (map
//! before js per mapped unit), `emitResult.sourceMaps`,
//! `emitted_files` (js before map), and `emitSkipped`; refusal-lane
//! cases (the H2.6b boundary controls) stay typed-refused with zero
//! writes; the parse-error fault stays the typed H2.9 deferral.

use serde_json::Value;
use tsc_compiler::ProgramSession;
use tsc_emitter::MemoryOutputSink;

use super::source_map_recording_witness_contract::{
    decode_base64, prepare_case_program, sha256_hex, vendored_library_files, WITNESSES,
};

/// The option name each refusal-control case must be refused on (the
/// execute.rs preflight row that owns it while H2.6b is unlanded).
fn expected_refusal_option(case_id: &str) -> &'static str {
    match case_id {
        "boundary-controls--control-sourceroot" => "sourceRoot",
        "boundary-controls--control-maproot" => "mapRoot",
        "boundary-controls--control-inline-map" => "inlineSourceMap",
        "boundary-controls--control-inline-sources-with-map" => "inlineSources",
        "boundary-controls--fault-inline-sources-alone" => "inlineSources",
        other => panic!("unmapped refusal-control case {other}"),
    }
}

#[test]
fn every_witness_case_reproduces_through_the_production_emit_path() {
    let artifact: Value = serde_json::from_slice(WITNESSES).expect("witness artifact JSON");
    let library_files = vendored_library_files();
    let mut parity_cases = 0_usize;
    let mut refusal_cases = 0_usize;
    let mut deferred_cases = 0_usize;
    for family in artifact["families"].as_array().expect("families") {
        for case in family["cases"].as_array().expect("cases") {
            let case_id = case["case_id"].as_str().expect("case id");
            let lane = case["rust_lane"].as_str().expect("rust lane");
            match lane {
                "refusal-control" => {
                    let prepared = prepare_case_program(case_id, case, &library_files);
                    let mut sink = MemoryOutputSink::new();
                    let error = ProgramSession::new(prepared)
                        .emit(&mut sink)
                        .expect_err("boundary-control options are refused until H2.6b");
                    let rendered = format!("{error:?}");
                    let option = expected_refusal_option(case_id);
                    assert!(
                        rendered.contains("UnsupportedCompilerOption") && rendered.contains(option),
                        "{case_id}: expected a typed {option} refusal, got {rendered}"
                    );
                    assert!(
                        sink.writes().is_empty(),
                        "{case_id}: a refused emit must write nothing"
                    );
                    refusal_cases += 1;
                }
                "parity" => {
                    if case_id == "path-shapes--positive-outdir-nested" {
                        // TS-source `outDir` stays H2.8a-owned (the
                        // execute.rs relocation gate: H2.3a JavaScript-only
                        // + H2.3d JSON relocation are the admitted arms).
                        // This case's byte parity is proven at the PRINT
                        // level by the m-2 replay suite; the production
                        // EMIT stays a typed refusal until H2.8a lands.
                        let prepared = prepare_case_program(case_id, case, &library_files);
                        let mut sink = MemoryOutputSink::new();
                        let error = ProgramSession::new(prepared)
                            .emit(&mut sink)
                            .expect_err("TS-source outDir is H2.8a-deferred");
                        let rendered = format!("{error:?}");
                        assert!(
                            rendered.contains("UnsupportedCompilerOption")
                                && rendered.contains("outDir"),
                            "{case_id}: expected the typed outDir refusal, got {rendered}"
                        );
                        assert!(
                            sink.writes().is_empty(),
                            "{case_id}: refusal writes nothing"
                        );
                        deferred_cases += 1;
                        continue;
                    }
                    if case_id == "edge-shapes--fault-parse-error" {
                        let prepared = prepare_case_program(case_id, case, &library_files);
                        let mut sink = MemoryOutputSink::new();
                        let error = ProgramSession::new(prepared)
                            .emit(&mut sink)
                            .expect_err("parse-diagnostic files are typed-deferred");
                        let rendered = format!("{error:?}");
                        assert!(
                            rendered.contains("ParseDiagnosticsDeferred")
                                && rendered.contains("H2.9"),
                            "{case_id}: unexpected deferral shape: {rendered}"
                        );
                        assert!(
                            sink.writes().is_empty(),
                            "{case_id}: deferral writes nothing"
                        );
                        deferred_cases += 1;
                        continue;
                    }
                    assert_parity_case(case_id, case, &library_files);
                    parity_cases += 1;
                }
                other => panic!("{case_id}: unknown rust_lane {other}"),
            }
        }
    }
    assert_eq!(
        (parity_cases, refusal_cases, deferred_cases),
        (29, 5, 2),
        "witness emit census changed"
    );
}

fn assert_parity_case(case_id: &str, case: &Value, library_files: &[(String, Vec<u8>)]) {
    let observation = &case["observation"];
    let prepared = prepare_case_program(case_id, case, library_files);
    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(prepared)
        .emit(&mut sink)
        .unwrap_or_else(|error| panic!("{case_id}: production emit failed: {error:?}"));

    // Write sequence: exact path order, bytes, BOM, source files, data.
    let frozen_writes = observation["writes"].as_array().expect("frozen writes");
    let produced = sink.writes();
    let produced_paths: Vec<String> = produced
        .iter()
        .map(|artifact| artifact.path().to_string_lossy().into_owned())
        .collect();
    let frozen_paths: Vec<&str> = frozen_writes
        .iter()
        .map(|write| write["path"].as_str().expect("write path"))
        .collect();
    assert_eq!(
        produced_paths, frozen_paths,
        "{case_id}: write order/paths diverge"
    );
    for (artifact, frozen) in produced.iter().zip(frozen_writes) {
        let path = frozen["path"].as_str().expect("write path");
        let frozen_bytes = decode_base64(frozen["callback_utf8_base64"].as_str().expect("bytes"));
        assert_eq!(
            sha256_hex(artifact.callback_text().as_bytes()),
            sha256_hex(&frozen_bytes),
            "{case_id}: {path} bytes diverge"
        );
        assert_eq!(
            artifact.write_byte_order_mark(),
            frozen["write_byte_order_mark"].as_bool().expect("bom"),
            "{case_id}: {path} BOM diverges"
        );
        let frozen_sources: Vec<&str> = frozen["source_files"]
            .as_array()
            .expect("source files")
            .iter()
            .map(|value| value.as_str().expect("source file"))
            .collect();
        let produced_sources: Vec<String> = artifact
            .source_files()
            .unwrap_or_default()
            .iter()
            .map(|source| source.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            produced_sources, frozen_sources,
            "{case_id}: {path} source files diverge"
        );
        let frozen_url_pos = frozen["data_source_map_url_pos"].as_u64();
        let produced_url_pos = artifact.metadata().and_then(|metadata| match metadata {
            tsc_emitter::EmitWriteMetadata::Text(text) => text
                .source_map_url_position()
                .map(|position| u64::from(position.value())),
            tsc_emitter::EmitWriteMetadata::BuildInfo(_) => None,
        });
        assert_eq!(
            produced_url_pos, frozen_url_pos,
            "{case_id}: {path} sourceMapUrlPos diverges"
        );
    }

    // emitResult.sourceMaps: raw input names + canonical map JSON.
    let frozen_maps = observation["source_maps"].as_array();
    match (outcome.source_maps(), frozen_maps) {
        (None, None) => {}
        (Some(produced_maps), Some(frozen_maps)) => {
            assert_eq!(
                produced_maps.len(),
                frozen_maps.len(),
                "{case_id}: sourceMaps entry count diverges"
            );
            for (produced_map, frozen_map) in produced_maps.iter().zip(frozen_maps) {
                let frozen_names: Vec<&str> = frozen_map["input_source_file_names"]
                    .as_array()
                    .expect("input names")
                    .iter()
                    .map(|value| value.as_str().expect("input name"))
                    .collect();
                let produced_names: Vec<String> = produced_map
                    .input_source_files()
                    .iter()
                    .map(|name| name.to_string_lossy().into_owned())
                    .collect();
                assert_eq!(
                    produced_names, frozen_names,
                    "{case_id}: sourceMaps input names diverge"
                );
                assert_eq!(
                    produced_map.canonical_json(),
                    frozen_map["source_map_json"].as_str().expect("map json"),
                    "{case_id}: sourceMaps JSON diverges"
                );
            }
        }
        (produced_maps, frozen_maps) => panic!(
            "{case_id}: sourceMaps presence diverges (produced {} frozen {})",
            produced_maps.is_some(),
            frozen_maps.is_some()
        ),
    }

    // emitted_files (present exactly when the case listed them).
    let frozen_listing = observation["emitted_files"].as_array();
    match (outcome.emitted_files(), frozen_listing) {
        (None, None) => {}
        (Some(produced_listing), Some(frozen_listing)) => {
            let produced_listing: Vec<String> = produced_listing
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            let frozen_listing: Vec<&str> = frozen_listing
                .iter()
                .map(|value| value.as_str().expect("emitted file"))
                .collect();
            assert_eq!(
                produced_listing, frozen_listing,
                "{case_id}: emittedFiles diverges"
            );
        }
        (produced_listing, frozen_listing) => panic!(
            "{case_id}: emittedFiles presence diverges (produced {} frozen {})",
            produced_listing.is_some(),
            frozen_listing.is_some()
        ),
    }

    assert_eq!(
        outcome.emit_skipped(),
        observation["emit_skipped"].as_bool().expect("emitSkipped"),
        "{case_id}: emitSkipped diverges"
    );
}

#[test]
fn a_mapped_emit_is_deterministic_across_two_runs() {
    let artifact: Value = serde_json::from_slice(WITNESSES).expect("witness artifact JSON");
    let library_files = vendored_library_files();
    let case = artifact["families"]
        .as_array()
        .expect("families")
        .iter()
        .flat_map(|family| family["cases"].as_array().expect("cases"))
        .find(|case| case["case_id"] == "plain-program--positive-statements")
        .expect("representative case");
    let run = || {
        let prepared =
            prepare_case_program("plain-program--positive-statements", case, &library_files);
        let mut sink = MemoryOutputSink::new();
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("deterministic mapped emit");
        (
            sink.writes()
                .iter()
                .map(|artifact| {
                    (
                        artifact.path().to_path_buf(),
                        artifact.callback_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>(),
            outcome
                .source_maps()
                .map(|maps| {
                    maps.iter()
                        .map(|map| map.canonical_json().to_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        )
    };
    assert_eq!(run(), run(), "mapped emit is not deterministic");
}
