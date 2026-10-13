//! tsgo transform_test.go (`ParseResult` up to its parse) and the file
//! loader's content mapper diagnostics.

use super::*;
use crate::content_mapper::errors::DiagnosticDirectiveError;
use crate::content_mapper::{Definition, Manifest};
use tsc_diagnostics::spanmap::{MappingError, Segment, FEATURES_ALL, KIND_VERBATIM};

fn mapper() -> Mapper {
    Mapper {
        definition: Definition {
            package: "pkg".to_owned(),
            extensions: Some(vec![".astro".to_owned()]),
            options: None,
        },
        manifest: Manifest {
            name: "mapper".to_owned(),
            ..Manifest::default()
        },
        ..Mapper::default()
    }
}

fn output(extension: &str, supplemental: &[&str]) -> TransformOutput {
    let result = |extension: &str, text: &str| MappedResult {
        text: text.to_owned(),
        virtual_extension: extension.to_owned(),
        mappings: SpanMap::new(Vec::new()),
        diagnostic_directives: Vec::new(),
    };
    TransformOutput {
        canonical: result(extension, "export {};"),
        diagnostics: Vec::new(),
        supplemental: supplemental
            .iter()
            .map(|extension| result(extension, "export const value = 1;"))
            .collect(),
    }
}

/// tsgo `TestParseResultSupplementalFileExtensions`.
#[test]
fn supplemental_outputs_are_named_by_index_and_extension() {
    let extensions = [".js", ".jsx", ".ts", ".tsx", ".mts", ".cts", ".json"];
    let sources = virtual_sources(
        JsStr::from("/component.astro"),
        "",
        &mapper(),
        "transform-identity",
        output(".ts", &extensions),
    )
    .unwrap();
    assert_eq!(sources.canonical.file_name, "/component.astro");
    assert_eq!(
        sources.canonical.info.virtual_file_name,
        "/component.astro.ts"
    );
    assert_eq!(
        sources.canonical.info.transform_identity,
        "transform-identity"
    );
    assert_eq!(sources.canonical.info.content_mapper, "mapper");
    let expected = [
        "/component.astro.0.js",
        "/component.astro.1.jsx",
        "/component.astro.2.ts",
        "/component.astro.3.tsx",
        "/component.astro.4.mts",
        "/component.astro.5.cts",
        "/component.astro.6.json",
    ];
    assert_eq!(sources.canonical.info.supplemental, expected);
    assert_eq!(sources.supplemental.len(), expected.len());
    for (supplemental, name) in sources.supplemental.iter().zip(expected) {
        assert_eq!(supplemental.file_name, name);
        assert_eq!(supplemental.info.virtual_file_name, name);
        assert_eq!(supplemental.info.transform_identity, "transform-identity");
        assert_eq!(
            supplemental.info.canonical.as_ref().map(JsString::as_js),
            Some(JsStr::from("/component.astro"))
        );
        assert!(supplemental.info.is_supplemental());
        assert!(!supplemental.info.is_failure_stub());
    }
}

#[test]
fn outputs_are_checked_against_the_original_content() {
    let mut invalid = output(".ts", &[]);
    invalid.canonical.mappings = SpanMap::new(vec![Segment {
        virtual_start: 0,
        virtual_end: 3,
        original_start: 0,
        original_end: 3,
        kind: KIND_VERBATIM,
        features: FEATURES_ALL,
    }]);
    match virtual_sources(JsStr::from("/a.box"), "xyz", &mapper(), "", invalid) {
        Err(TransformFailure::Mappings(MappingError {
            kind: MappingErrorKind::VerbatimMismatch,
            ..
        })) => {}
        result => panic!("expected a verbatim mismatch, got {result:?}"),
    }
    let mut coffee = output(".ts", &[]);
    coffee.supplemental.push(MappedResult {
        text: String::new(),
        virtual_extension: ".coffee".to_owned(),
        mappings: SpanMap::new(Vec::new()),
        diagnostic_directives: Vec::new(),
    });
    assert_eq!(
        virtual_sources(JsStr::from("/a.box"), "", &mapper(), "", coffee).unwrap_err(),
        TransformFailure::Response(ResponseFailure::InvalidVirtualExtension(
            ".coffee".to_owned()
        ))
    );
}

#[test]
fn a_mapper_s_diagnostics_keep_their_source_and_original_range() {
    let mut with_diagnostic = output(".ts", &[]);
    with_diagnostic.diagnostics.push(MapperDiagnostic {
        start: 2,
        end: 5,
        code: 1000,
        message_text: "Unclosed interpolation.".to_owned(),
        source: "box".to_owned(),
    });
    let sources = virtual_sources(
        JsStr::from("/a.box"),
        "ab{{cd",
        &mapper(),
        "",
        with_diagnostic,
    )
    .unwrap();
    let [diagnostic] = sources.canonical.info.diagnostics.as_slice() else {
        panic!("one diagnostic");
    };
    assert_eq!(
        (diagnostic.start, diagnostic.length, diagnostic.code()),
        (Some(2), Some(3), 1000)
    );
    assert_eq!(diagnostic.source.as_deref(), Some("box"));
    assert_eq!(diagnostic.message_text(), "Unclosed interpolation.");
    assert_eq!(diagnostic.reports_unnecessary, None);
}

fn texts(diagnostic: &Diagnostic) -> Vec<String> {
    let mut texts = vec![diagnostic.message.text.to_string_lossy().into_owned()];
    let mut next = &diagnostic.message.next;
    while let Some(entry) = next.first() {
        texts.push(entry.text.to_string_lossy().into_owned());
        next = &entry.next;
    }
    texts
}

#[test]
fn failures_are_reported_at_the_start_of_the_file_as_tsgo_words_them() {
    let diagnostic = |failure: TransformFailure| {
        let diagnostic = transform_diagnostic(JsStr::from("/a.box"), "box", &failure);
        assert_eq!((diagnostic.start, diagnostic.length), (Some(0), Some(0)));
        texts(&diagnostic)
    };
    assert_eq!(
        diagnostic(TransformFailure::Request("broken pipe".to_owned())),
        [
            "The content mapper 'box' failed to transform this file.",
            "The content mapper process failed while handling the transform request."
        ]
    );
    assert_eq!(
        diagnostic(TransformFailure::Project(ProjectFailure::Project(
            ProjectErrorKind::MalformedResponse
        ))),
        [
            "The content mapper 'box' failed to transform this file.",
            "The content mapper returned a project response that could not be decoded."
        ]
    );
    assert_eq!(
        diagnostic(TransformFailure::Response(
            ResponseFailure::DiagnosticDirective(DiagnosticDirectiveError {
                kind: DiagnosticDirectiveErrorKind::InvalidRange,
                index: 3,
                supplemental_index: Some(1),
                policy: 0,
            })
        )),
        [
            "The content mapper 'box' failed to transform this file.",
            "The invalid diagnostic directive is in supplemental output 1 returned by the content mapper.",
            "Diagnostic directive 3 returned by the content mapper has an invalid range."
        ]
    );
    assert_eq!(
        diagnostic(TransformFailure::Mappings(MappingError {
            kind: MappingErrorKind::Overlap,
            virtual_pos: 5,
            original_pos: 0,
        })),
        ["The content mapper 'box' produced overlapping or out-of-order position mappings (near virtual offset 5)."]
    );
    assert_eq!(
        diagnostic(TransformFailure::Host(
            "content mapper project is closed".to_owned()
        )),
        ["The content mapper 'box' failed to transform this file."]
    );
}

#[test]
fn initialization_and_budget_diagnostics_have_no_file() {
    let diagnostic = initialization_diagnostic(
        "",
        &InitializeError {
            kind: InitializeErrorKind::ProcessExit { exit_code: 42 },
            mapper_name: "vm".to_owned(),
        },
    );
    assert_eq!(diagnostic.file_name, None);
    assert_eq!(
        texts(&diagnostic),
        [
            "The content mapper 'vm' could not be initialized.",
            "The content mapper process exited before responding to the 'initialize' request (exit code 42)."
        ]
    );
    assert_eq!(
        texts(&disabled_diagnostic("vm")),
        ["The content mapper 'vm' failed 5 times and will not be used."]
    );
}

/// tsgo `TestContentMappedParseCacheBundleLifetime`: the cache keeps a
/// transform's outputs as one entry, found again with every output's
/// identity, and forgets it when no program holds the outputs.
#[test]
fn the_cache_finds_a_transforms_outputs_while_a_program_holds_them() {
    let cache = ContentMappedCache::default();
    let file_name = JsStr::from("/component.vue");
    let sources = virtual_sources(
        file_name,
        "<template />",
        &mapper(),
        "transform-identity",
        output(".ts", &[".ts"]),
    )
    .unwrap();
    cache.insert(&sources);
    let found = cache
        .get(file_name, "<template />", "transform-identity")
        .expect("the held outputs are found");
    assert!(Arc::ptr_eq(&found.canonical.text, &sources.canonical.text));
    assert!(Arc::ptr_eq(&found.canonical.info, &sources.canonical.info));
    assert!(Arc::ptr_eq(
        &found.supplemental[0].info,
        &sources.supplemental[0].info
    ));
    // Another content or transform identity is another key.
    assert!(cache
        .get(file_name, "<template></template>", "transform-identity")
        .is_none());
    assert!(cache
        .get(file_name, "<template />", "other-identity")
        .is_none());
    drop(found);
    assert_eq!(cache.len(), 1);
    // The outputs and their entry go with the last program holding them.
    drop(sources);
    assert!(cache
        .get(file_name, "<template />", "transform-identity")
        .is_none());
    assert!(cache.is_empty());
    cache.purge();
    assert!(cache.lock().is_empty());
}
