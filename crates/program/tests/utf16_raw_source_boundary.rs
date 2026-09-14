//! Observe the separately scoped raw-source limitation from design §10.4.
//! These are not passing emit-parity claims for raw lone-surrogate files.
use std::path::PathBuf;
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    decode_host_text, load_no_lib_program, CompilerOptions, HostTextEncoding, ProgramLoadError,
    ProgramLoadLimits, ProgramLoadOperation, ProgramOptions,
};

struct Artifact {
    cases: Vec<Case>,
}
struct Case {
    id: String,
    encoding: String,
    kind: String,
    bytes: Vec<u8>,
    source_units: Vec<u16>,
    literal_values: Vec<Vec<u16>>,
    parse_diagnostic_codes: Vec<u32>,
}

#[test]
fn raw_utf16_source_preservation_in_typescript_and_explicit_rust_decode_limit() {
    let value: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/utf16-raw-source-boundary.json")).unwrap();
    let units = |value: &serde_json::Value| {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
            .collect::<Vec<_>>()
    };
    let artifact = Artifact {
        cases: value["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|case| Case {
                id: case["id"].as_str().unwrap().into(),
                encoding: case["encoding"].as_str().unwrap().into(),
                kind: case["kind"].as_str().unwrap().into(),
                bytes: case["bytes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|byte| u8::try_from(byte.as_u64().unwrap()).unwrap())
                    .collect(),
                source_units: units(&case["source_units"]),
                literal_values: case["literal_values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(units)
                    .collect(),
                parse_diagnostic_codes: case["parse_diagnostic_codes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|code| u32::try_from(code.as_u64().unwrap()).unwrap())
                    .collect(),
            })
            .collect(),
    };
    assert_eq!(artifact.cases.len(), 6);
    for case in artifact.cases {
        assert!(
            case.parse_diagnostic_codes.is_empty(),
            "{}: upstream parses",
            case.id
        );
        let host = MemoryCompilerHost::builder("/")
            .file("/input.ts", case.bytes.clone())
            .build()
            .unwrap();
        let loaded = load_no_lib_program(
            &host,
            &[PathBuf::from("/input.ts")],
            CompilerOptions {
                no_emit: Some(true),
                ..Default::default()
            },
            ProgramOptions::default()
                .with_no_lib(true)
                .with_types(Vec::new()),
            ProgramLoadLimits::new(8, 32, 8, 4096, 16384),
        );
        if case.kind == "paired" {
            assert_eq!(case.literal_values, vec![vec![0xd800, 0xdc00]]);
            let text = decode_host_text(case.bytes).unwrap();
            assert_eq!(
                text.encode_utf16().collect::<Vec<_>>(),
                case.source_units,
                "{}",
                case.id
            );
            assert!(
                loaded.is_ok(),
                "{}: scalar source loads: {loaded:?}",
                case.id
            );
        } else {
            let expected_unit = if case.kind == "low" { 0xdc00 } else { 0xd800 };
            if case.kind == "different-high" {
                assert_eq!(case.literal_values, vec![vec![0xd800], vec![0xd801]]);
            } else {
                assert_eq!(case.literal_values, vec![vec![0xdc00]]);
            }
            let error = decode_host_text(case.bytes).unwrap_err();
            assert_eq!(
                error.encoding(),
                if case.encoding == "utf16le" {
                    HostTextEncoding::Utf16Le
                } else {
                    HostTextEncoding::Utf16Be
                }
            );
            assert_eq!(error.unpaired_surrogate(), expected_unit);
            assert_eq!(
                error.code_unit_index(),
                case.source_units
                    .iter()
                    .position(|unit| *unit == expected_unit)
                    .unwrap()
            );
            match loaded {
                Err(ProgramLoadError::Decode {
                    operation, source, ..
                }) => {
                    assert_eq!(operation, ProgramLoadOperation::DecodeSource);
                    assert_eq!(source, error);
                }
                other => panic!(
                    "{}: raw unpaired source remains an explicit decode error: {other:?}",
                    case.id
                ),
            }
        }
    }
}
