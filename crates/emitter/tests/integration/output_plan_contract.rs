use tsc_emitter::{
    EmitBundle, EmitContractViolation, EmitFailure, EmitMode, EmitOutputPaths, EmitOutputPlan,
    EmitOutputUnit, EmitRoot, EmitSelection, UnsupportedEmitFeature,
};
use tsc_program::SourceFileId;

fn source(raw: u32) -> SourceFileId {
    SourceFileId::from_raw(raw)
}

fn script_unit(raw: u32, paths: EmitOutputPaths) -> EmitOutputUnit {
    EmitOutputUnit::new(EmitRoot::SourceFile(source(raw)), paths, EmitMode::Script)
}

#[test]
fn bootstrap_shape_is_whole_program_source_file_javascript_only() {
    let plan = EmitOutputPlan::whole_program(vec![script_unit(
        3,
        EmitOutputPaths::javascript("/project/out.js"),
    )]);

    assert_eq!(plan.selection(), EmitSelection::WholeProgram);
    assert_eq!(plan.units().len(), 1);
    assert_eq!(plan.units()[0].mode(), EmitMode::Script);
    assert_eq!(
        plan.units()[0].paths().javascript_path(),
        Some(std::path::Path::new("/project/out.js"))
    );
    assert_eq!(plan.validate_bootstrap_shape(), Ok(()));
}

#[test]
fn every_dormant_axis_is_typed_and_rejected() {
    let javascript = || EmitOutputPaths::javascript("/project/out.js");
    let targeted = EmitOutputPlan::targeted(source(1), vec![script_unit(1, javascript())]);
    assert_eq!(
        targeted.validate_bootstrap_shape(),
        Err(EmitFailure::Unsupported(
            UnsupportedEmitFeature::TargetedSelection
        ))
    );

    let bundle = EmitOutputPlan::whole_program(vec![EmitOutputUnit::new(
        EmitRoot::Bundle(EmitBundle::new(vec![source(1), source(2)])),
        javascript(),
        EmitMode::Script,
    )]);
    assert_eq!(
        bundle.validate_bootstrap_shape(),
        Err(EmitFailure::Unsupported(UnsupportedEmitFeature::BundleRoot))
    );

    for (mode, feature) in [
        (
            EmitMode::DeclarationOnly,
            UnsupportedEmitFeature::DeclarationOnlyMode,
        ),
        (
            EmitMode::BuilderSignature,
            UnsupportedEmitFeature::BuilderSignatureMode,
        ),
        (
            EmitMode::BuildInfoOnly,
            UnsupportedEmitFeature::BuildInfoOnlyMode,
        ),
    ] {
        let plan = EmitOutputPlan::whole_program(vec![EmitOutputUnit::new(
            EmitRoot::SourceFile(source(1)),
            javascript(),
            mode,
        )]);
        assert_eq!(
            plan.validate_bootstrap_shape(),
            Err(EmitFailure::Unsupported(feature))
        );
    }

    for (paths, feature) in [
        (
            javascript().with_javascript_map("/project/out.js.map"),
            UnsupportedEmitFeature::JavaScriptMap,
        ),
        (
            javascript().with_declaration("/project/out.d.ts"),
            UnsupportedEmitFeature::Declaration,
        ),
        (
            javascript().with_declaration_map("/project/out.d.ts.map"),
            UnsupportedEmitFeature::DeclarationMap,
        ),
        (
            javascript().with_build_info("/project/tsconfig.tsbuildinfo"),
            UnsupportedEmitFeature::BuildInfo,
        ),
    ] {
        let plan = EmitOutputPlan::whole_program(vec![script_unit(1, paths)]);
        assert_eq!(
            plan.validate_bootstrap_shape(),
            Err(EmitFailure::Unsupported(feature))
        );
    }
}

#[test]
fn malformed_active_slot_is_a_contract_failure() {
    let plan = EmitOutputPlan::whole_program(vec![script_unit(1, EmitOutputPaths::empty())]);
    assert_eq!(
        plan.validate_bootstrap_shape(),
        Err(EmitFailure::Contract(
            EmitContractViolation::ScriptOutputMissingJavaScriptPath
        ))
    );
}
