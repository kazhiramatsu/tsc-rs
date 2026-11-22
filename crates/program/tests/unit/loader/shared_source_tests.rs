use super::*;

#[test]
fn shared_source_text_survives_decoding_and_snapshot_construction() {
    static TEXT: tsc_diagnostics::StaticText<4> = tsc_diagnostics::StaticText::new("😀");
    let text = TEXT.shared();
    let snapshot = decode_source_text(SourceRead::Text(text.clone()))
        .unwrap()
        .into_snapshot();
    assert!(snapshot.shared_text().ptr_eq(&text));
    assert!(!snapshot.shared_text().in_text_heap());
    assert_eq!(snapshot.positions().byte_to_utf16(4), Some(2));
    let owned = decode_source_text(SourceRead::Bytes(vec![0xff, 0xfe, 0x3d, 0xd8, 0, 0xde]))
        .unwrap()
        .into_snapshot();
    assert_eq!(owned.text(), "😀");
    assert!(owned.shared_text().in_text_heap());
}

#[test]
fn implied_format_pairs_use_the_selected_extension_scope_and_emit_mode() {
    use ResolutionMode::{CommonJs, EsNext};
    for package_type in [
        None,
        Some(PackageJsonType::CommonJs),
        Some(PackageJsonType::Module),
        Some(PackageJsonType::Other),
    ] {
        let scope = package_type.map(|ty| {
            PackageMetadata::from_trusted_parsed(
                ProgramPath::from_trusted_parts("/package.json", "/package.json").unwrap(),
                "{}",
                None,
                None,
                ty,
            )
        });
        for module in [99, 199] {
            for resolution in [99, 100] {
                let options = CompilerOptions {
                    module: Some(module),
                    module_resolution: Some(resolution),
                    ..Default::default()
                };
                for dir in ["/src", "/node_modules/pkg"] {
                    let consulted = resolution == 99 || dir == "/node_modules/pkg";
                    let esm = consulted && package_type == Some(PackageJsonType::Module);
                    let implied = if esm { EsNext } else { CommonJs };
                    let emit = (module == 199
                        || esm
                        || (consulted && package_type == Some(PackageJsonType::CommonJs)))
                    .then_some(implied);
                    for (ext, expected) in [
                        ("mts", (Some(EsNext), Some(EsNext))),
                        ("cts", (Some(CommonJs), Some(CommonJs))),
                        ("ts", (Some(implied), emit)),
                        ("tsx", (Some(implied), emit)),
                        ("vue", (None, None)),
                    ] {
                        let file = format!("{dir}/component.{ext}");
                        assert_eq!(
                            implied_node_formats(file.as_str().into(), scope.as_ref(), &options),
                            expected,
                            "{file} {module} {resolution} {package_type:?}"
                        );
                    }
                }
            }
        }
    }
}
