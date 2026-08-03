use std::path::Path;

use tsc_host::MemoryCompilerHost;
use tsc_program::{
    decode_host_text, CompilerOptions, HostResolvedModule, HostTextEncoding, ModuleResolver,
    ResolutionMode, ResolutionOutcome,
};

fn decoded(bytes: &[u8]) -> String {
    decode_host_text(bytes.to_vec()).expect("decode representable host text")
}

fn utf16_bytes(text: &str, encoding: HostTextEncoding, odd_tail: Option<u8>) -> Vec<u8> {
    let mut bytes = match encoding {
        HostTextEncoding::Utf16Le => vec![0xFF, 0xFE],
        HostTextEncoding::Utf16Be => vec![0xFE, 0xFF],
    };
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&match encoding {
            HostTextEncoding::Utf16Le => unit.to_le_bytes(),
            HostTextEncoding::Utf16Be => unit.to_be_bytes(),
        });
    }
    bytes.extend(odd_tail);
    bytes
}

#[test]
fn decodes_utf8_bom_and_short_inputs_without_guessing_utf16() {
    assert_eq!(decoded(&[]), "");
    assert_eq!(decoded(b"plain \xF0\x9F\x98\x80"), "plain 😀");
    assert_eq!(decoded(&[0xEF, 0xBB, 0xBF, b'a']), "a");
    assert_eq!(
        decoded(&[0xEF, 0xBB, 0xBF, 0xEF, 0xBB, 0xBF, b'a']),
        "\u{feff}a",
        "only the leading byte-order mark is consumed"
    );
    assert_eq!(decoded(&[0xEF, 0xBB]), "\u{fffd}");
    assert_eq!(decoded(&[b'A', 0]), "A\0", "BOM-less UTF-16 is not guessed");
}

#[test]
fn replaces_invalid_utf8_with_node_compatible_maximal_subparts() {
    for (bytes, expected) in [
        (&[b'a', 0x80, b'b'][..], "a\u{fffd}b"),
        (&[0xC0, 0x80], "\u{fffd}\u{fffd}"),
        (&[0xE2, 0x82], "\u{fffd}"),
        (&[0xE2, 0x28, 0xA1], "\u{fffd}(\u{fffd}"),
        (&[0xED, 0xA0, 0x80], "\u{fffd}\u{fffd}\u{fffd}"),
        (
            &[0xF4, 0x90, 0x80, 0x80],
            "\u{fffd}\u{fffd}\u{fffd}\u{fffd}",
        ),
    ] {
        assert_eq!(decoded(bytes), expected, "bytes={bytes:02X?}");
    }
}

#[test]
fn decodes_bom_selected_utf16_and_discards_an_odd_trailing_byte() {
    for encoding in [HostTextEncoding::Utf16Le, HostTextEncoding::Utf16Be] {
        assert_eq!(decoded(&utf16_bytes("", encoding, None)), "");
        assert_eq!(
            decoded(&utf16_bytes("A😀", encoding, Some(0x7F))),
            "A😀",
            "encoding={encoding:?}"
        );
        assert_eq!(
            decoded(&utf16_bytes("\u{feff}A", encoding, None)),
            "\u{feff}A",
            "a second decoded BOM remains text"
        );
    }
}

#[test]
fn rejects_unpaired_utf16_surrogates_without_changing_source_identity() {
    let high = decode_host_text(vec![0xFF, 0xFE, b'A', 0, 0x00, 0xD8])
        .expect_err("an unpaired high surrogate is not UTF-8 representable");
    assert_eq!(high.encoding(), HostTextEncoding::Utf16Le);
    assert_eq!(high.code_unit_index(), 1);
    assert_eq!(high.unpaired_surrogate(), 0xD800);
    assert!(high.to_string().contains("U+D800"));

    let low = decode_host_text(vec![0xFE, 0xFF, 0xDC, 0x00])
        .expect_err("an unpaired low surrogate is not UTF-8 representable");
    assert_eq!(low.encoding(), HostTextEncoding::Utf16Be);
    assert_eq!(low.code_unit_index(), 0);
    assert_eq!(low.unpaired_surrogate(), 0xDC00);

    let mut after_astral = utf16_bytes("😀", HostTextEncoding::Utf16Le, None);
    after_astral.extend_from_slice(&0xD800_u16.to_le_bytes());
    let after_astral = decode_host_text(after_astral)
        .expect_err("the error offset counts both units of an astral character");
    assert_eq!(after_astral.code_unit_index(), 2);
}

fn resolve_encoded_package(package_json: Vec<u8>) -> HostResolvedModule {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.mts", b"export {};".to_vec())
        .file("/node_modules/pkg/package.json", package_json)
        .file(
            "/node_modules/pkg/index.d.ts",
            b"export const value: true;".to_vec(),
        )
        .build()
        .expect("build encoded package host");
    let options = CompilerOptions {
        module: Some(199),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let outcome = resolver
        .resolve(Path::new("/index.mts"), "pkg", ResolutionMode::EsNext)
        .expect("resolve encoded package");
    let ResolutionOutcome::Resolved(resolved) = outcome else {
        panic!("encoded package must resolve");
    };
    resolved
}

#[test]
fn package_resolution_uses_the_central_host_text_decoder() {
    let json = r#"{"name":"pkg","types":"./index.d.ts"}"#;
    for encoding in [HostTextEncoding::Utf16Le, HostTextEncoding::Utf16Be] {
        let resolved = resolve_encoded_package(utf16_bytes(json, encoding, Some(0x7F)));
        let metadata = resolved
            .package_metadata()
            .expect("encoded package retains metadata");
        assert_eq!(metadata.text(), json);
        assert_eq!(metadata.name(), Some("pkg"));
    }

    let mut lossy_json = br#"{"name":"pXkg","types":"./index.d.ts"}"#.to_vec();
    let marker = lossy_json
        .iter()
        .position(|byte| *byte == b'X')
        .expect("test marker");
    lossy_json[marker] = 0x80;
    let resolved = resolve_encoded_package(lossy_json);
    let metadata = resolved
        .package_metadata()
        .expect("lossily decoded package retains metadata");
    assert_eq!(metadata.name(), Some("p\u{fffd}kg"));
    assert!(metadata.text().contains("p\u{fffd}kg"));

    let mut bom_json = vec![0xEF, 0xBB, 0xBF];
    bom_json.extend_from_slice(json.as_bytes());
    let resolved = resolve_encoded_package(bom_json);
    assert!(
        resolved
            .package_metadata()
            .expect("BOM package retains metadata")
            .text()
            .starts_with('{'),
        "retained package text must use the same BOM-free central decode"
    );
}

#[test]
fn raw_unpaired_utf16_is_invalid_but_escaped_surrogates_follow_read_json() {
    let mut json = utf16_bytes(
        r#"{"name":"pkg","types":"./index.d.ts"}"#,
        HostTextEncoding::Utf16Le,
        None,
    );
    json.extend_from_slice(&0xD800_u16.to_le_bytes());
    let host = MemoryCompilerHost::builder("/")
        .file("/index.mts", b"export {};".to_vec())
        .file("/node_modules/pkg/package.json", json)
        .file("/node_modules/pkg/index.d.ts", b"export {};".to_vec())
        .build()
        .expect("build unrepresentable package host");
    let options = CompilerOptions {
        module: Some(199),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let error = resolver
        .resolve(Path::new("/index.mts"), "pkg", ResolutionMode::EsNext)
        .expect_err("unpaired surrogate must fail closed");
    assert_eq!(error.kind(), tsc_program::ResolutionErrorKind::InvalidData);
    assert!(error.to_string().contains("U+D800"));
    assert!(error.to_string().contains("package.json"));

    let escaped_host = MemoryCompilerHost::builder("/")
        .file("/index.mts", b"export {};".to_vec())
        .file(
            "/node_modules/pkg/package.json",
            br#"{"name":"p\ud800kg","types":"./index.d.ts"}"#.to_vec(),
        )
        .file("/node_modules/pkg/index.d.ts", b"export {};".to_vec())
        .build()
        .expect("build escaped-surrogate package host");
    let mut resolver =
        ModuleResolver::new(&escaped_host, &options).expect("create escaped resolver");
    let resolved = resolver
        .resolve(Path::new("/index.mts"), "pkg", ResolutionMode::EsNext)
        .expect("an escaped surrogate is valid JSON text");
    let ResolutionOutcome::Resolved(resolved) = resolved else {
        panic!("escaped-surrogate package must resolve");
    };
    let metadata = resolved
        .package_metadata()
        .expect("escaped-surrogate package retains metadata");
    assert_eq!(metadata.name(), Some("p\u{fffd}kg"));
    assert_eq!(
        metadata.text(),
        r#"{"name":"p\ud800kg","types":"./index.d.ts"}"#
    );
}
