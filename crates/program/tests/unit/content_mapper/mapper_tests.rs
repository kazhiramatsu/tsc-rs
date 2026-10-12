//! Mapper identities and transform identities, pinned to tsgo
//! `contentmapper` aa8149273 (computed by its own code).

use super::*;

fn hex(bytes: [u8; 16]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn full() -> Mapper {
    Mapper {
        definition: Definition {
            package: "mapper".to_owned(),
            extensions: Some(vec![".box".to_owned()]),
            options: Some(r#"{"flavor":"x"}"#.to_owned()),
        },
        manifest: Manifest {
            name: "mapper".to_owned(),
            version: "1.0.0".to_owned(),
            exec: Some(vec!["compiler-test-mapper".to_owned()]),
            compiler_options: Some(vec![
                "target".to_owned(),
                "jsx".to_owned(),
                "strict".to_owned(),
            ]),
            dynamic_config: false,
        },
        ..Mapper::default()
    }
}

/// `target: ES2020` and `jsx: preserve` as tsgo marshals its
/// `CompilerOptions` fields; `strict` is unset.
fn option_json(name: &str) -> Option<String> {
    match name {
        "target" => Some("7".to_owned()),
        "jsx" => Some("1".to_owned()),
        _ => None,
    }
}

#[test]
fn transform_identities_hash_identity_options_and_declared_compiler_options_like_tsgo() {
    let full = full();
    assert_eq!(full.identity(), "mapper@1.0.0");
    let declared = full.declared_options_json(option_json);
    assert_eq!(declared, r#"{"target":7,"jsx":1}"#);
    assert_eq!(
        hex(full.transform_identity(&declared)),
        "18e47b8226368ad2ccb572acbfb9c3c6"
    );

    let bare = Mapper {
        definition: Definition {
            package: "vue".to_owned(),
            ..Definition::default()
        },
        manifest: Manifest {
            name: "vue-ts-mapper".to_owned(),
            ..Manifest::default()
        },
        ..Mapper::default()
    };
    assert_eq!(bare.identity(), "vue-ts-mapper");
    let declared = bare.declared_options_json(option_json);
    assert_eq!(declared, "{}");
    assert_eq!(
        hex(bare.transform_identity(&declared)),
        "1b193d8172fff05a3784073732182b67"
    );
}

#[test]
fn identities_and_diagnostic_names_follow_what_is_known() {
    let contributed = Mapper {
        manifest: Manifest {
            name: "astro".to_owned(),
            version: "2.0.0".to_owned(),
            ..Manifest::default()
        },
        contribution_id: "ext.astro".to_owned(),
        ..Mapper::default()
    };
    assert_eq!(contributed.identity(), "ext.astro (astro@2.0.0)");
    assert_eq!(contributed.diagnostic_name(), "astro");

    let unresolved = Mapper {
        definition: Definition {
            package: "vue-mapper".to_owned(),
            ..Definition::default()
        },
        ..Mapper::default()
    };
    assert_eq!(unresolved.identity(), "");
    assert_eq!(unresolved.diagnostic_name(), "vue-mapper");
}

#[test]
fn virtual_extensions_are_the_compiler_s_own() {
    for extension in [
        ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".json",
    ] {
        assert!(is_supported_virtual_extension(extension), "{extension}");
    }
    for extension in [".d.ts", ".vue", "ts", ""] {
        assert!(!is_supported_virtual_extension(extension), "{extension}");
    }
}
