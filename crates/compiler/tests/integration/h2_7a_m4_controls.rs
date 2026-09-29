//! Declaration bundle-transform boundary control.

use std::path::Path;

use tsc_checker::CompilerOptions;
use tsc_emitter::{
    transform_nodes, DeclarationPathResolver, DeclarationTransformer, EmitHost, SourceFileId,
    TransformArena, TransformBundle, TransformError, TransformRoot,
};

struct ControlHost {
    options: CompilerOptions,
    sources: [SourceFileId; 1],
}

impl EmitHost for ControlHost {
    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
    }

    fn current_directory(&self) -> tsc_diagnostics::JsStr<'_> {
        (Path::new("/control"))
            .to_str()
            .expect("scalar mock host directory")
            .into()
    }

    fn common_source_directory(&self) -> tsc_diagnostics::JsStr<'_> {
        (Path::new("/control"))
            .to_str()
            .expect("scalar mock host directory")
            .into()
    }

    fn config_file_path(&self) -> Option<tsc_diagnostics::JsStr<'_>> {
        None
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.sources
    }

    fn source_file(&self, _id: SourceFileId) -> Option<tsc_emitter::EmitSource<'_>> {
        None
    }
}

struct NoDeclarationPaths;

impl DeclarationPathResolver for NoDeclarationPaths {
    fn declaration_file_path(&self, _source: SourceFileId) -> Option<tsc_diagnostics::JsString> {
        None
    }

    fn reference_target_path(&self, _source: SourceFileId) -> Option<tsc_diagnostics::JsString> {
        None
    }
}

#[test]
fn declaration_bundle_transform_requires_a_bundle_output_path() {
    let source = SourceFileId::from_raw(0);
    let host = ControlHost {
        options: CompilerOptions::default(),
        sources: [source],
    };
    let resolver = tsc_emitter::UnavailableEmitResolver;
    let paths = NoDeclarationPaths;
    let transformer = DeclarationTransformer::new(&host.options, &resolver, &host, &paths);
    let result = transform_nodes(
        TransformArena::new(),
        vec![TransformRoot::Bundle(TransformBundle::new(Vec::new()))],
        vec![Box::new(transformer)],
        false,
    );
    assert!(matches!(
        result,
        Err(TransformError::UnsupportedCompilerOption {
            option: "declaration transformer contract",
            detail: "bundle declaration output path is required",
        })
    ));
}
