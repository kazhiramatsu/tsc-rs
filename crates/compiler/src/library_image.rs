//! The host-generated command-mode images. Runtime adoption is a separate
//! change; this module currently only gives the ordinary test suite the
//! exact bytes that a compiler build generated and would embed.

include!(concat!(env!("OUT_DIR"), "/embedded_library_images.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use tsc_binder::library_image::BindingImage;
    use tsc_syntax::library_image::SyntaxImage;
    use tsc_syntax::{JSDocParsingMode, ParseOptions};

    #[test]
    fn host_built_catalog_equals_the_ordinary_target_frontend() {
        let started = std::time::Instant::now();
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs",
        );
        assert_eq!(EMBEDDED_LIBRARY_IMAGES.len(), 116);
        let mut bytes = 0;
        for entry in EMBEDDED_LIBRARY_IMAGES {
            let name = format!("bundled:///libs/{}", entry.name);
            let text = std::fs::read_to_string(directory.join(entry.name)).unwrap();
            let mut image = SyntaxImage::decode(entry.syntax, &text).unwrap();
            assert!(image.matches(&name, &text, JSDocParsingMode::ParseForTypeErrors));
            assert!(!image.matches(&name, &text, JSDocParsingMode::ParseAll));
            let mut source = tsc_syntax::parse_source_file(
                name.as_str(),
                text.as_str(),
                ParseOptions {
                    js_doc_parsing_mode: JSDocParsingMode::ParseForTypeErrors,
                    ..Default::default()
                },
                None,
            );
            assert_eq!(source, image.restore().unwrap(), "parsed {}", entry.name);
            let data = tsc_binder::Binder::bind_local(&mut source).into_bind_data();
            image.apply_binding_edits(entry.edits).unwrap();
            assert_eq!(
                source,
                image.restore().unwrap(),
                "bound tree {}",
                entry.name
            );
            assert_eq!(
                BindingImage::capture(&source, &data).unwrap(),
                BindingImage::decode(entry.binding).unwrap(),
                "binding {}",
                entry.name
            );
            bytes += entry.syntax.len() + entry.binding.len() + entry.edits.len();
        }
        assert!(bytes <= 12 * 1024 * 1024);
        eprintln!(
            "library image proof: 116 files, {bytes} bytes, {:?}",
            started.elapsed()
        );
    }
}
