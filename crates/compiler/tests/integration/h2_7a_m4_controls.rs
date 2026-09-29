//! Declaration bundle-transform and output-plan boundary controls.
//! H2.7d admits bundle roots.

use std::path::Path;

use tsc_checker::CompilerOptions;
use tsc_emitter::{
    transform_nodes, DeclarationPathResolver, DeclarationTransformer, EmitBundle, EmitFailure,
    EmitHost, EmitMode, EmitOutputPaths, EmitOutputPlan, EmitOutputUnit, EmitRoot, SourceFileId,
    TransformArena, TransformBundle, TransformError, TransformRoot, UnsupportedEmitFeature,
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

#[test]
fn declaration_plan_admits_nonempty_bundles_and_retains_boundary_refusals() {
    let source = SourceFileId::from_raw(0);
    let bundle = EmitOutputPlan::whole_program(vec![EmitOutputUnit::new(
        EmitRoot::Bundle(EmitBundle::new(vec![source])),
        EmitOutputPaths::javascript("/control/out.js"),
        EmitMode::Script,
    )]);
    assert_eq!(bundle.validate_bootstrap_shape(), Ok(()));
    let empty_bundle = EmitOutputPlan::whole_program(vec![EmitOutputUnit::new(
        EmitRoot::Bundle(EmitBundle::new(Vec::new())),
        EmitOutputPaths::javascript("/control/out.js"),
        EmitMode::Script,
    )]);
    assert_eq!(
        empty_bundle.validate_bootstrap_shape(),
        Err(EmitFailure::Unsupported(UnsupportedEmitFeature::BundleRoot))
    );

    let declaration = EmitOutputPlan::whole_program(vec![EmitOutputUnit::new(
        EmitRoot::SourceFile(source),
        EmitOutputPaths::javascript("/control/out.js").with_declaration("/control/out.d.ts"),
        EmitMode::Script,
    )]);
    assert_eq!(declaration.validate_bootstrap_shape(), Ok(()));

    let printer = include_str!("../../../emitter/src/printer.rs");
    assert!(
        printer.contains(
            "PrintRequest::Declaration(source) => self.print_declaration_with_recording("
        ),
        "PrintRequest::Declaration must route through the activated declaration entry"
    );
    let execute = include_str!("../../../emitter/src/execute.rs");
    assert!(
        !execute.contains("transform_declaration_unit_for_harness"),
        "the dormant declaration seam must not be activated from execute"
    );
}
