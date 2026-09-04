//! h2-7a-m-4 L3 dormancy and refusal-retention controls.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use tsc_checker::CompilerOptions;
use tsc_emitter::{
    transform_nodes, DeclarationPathResolver, DeclarationTransformer, EmitBundle, EmitFailure,
    EmitHost, EmitMode, EmitOutputPaths, EmitOutputPlan, EmitOutputUnit, EmitRoot, SourceFileId,
    TransformArena, TransformBundle, TransformError, TransformRoot, UnsupportedEmitFeature,
};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("readable directory") {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn declaration_transformer_production_call_sites_are_exactly_allowlisted() {
    let workspace = workspace();
    let mut files = Vec::new();
    collect_rs_files(&workspace.join("crates"), &mut files);
    let symbols = [
        (
            "DeclarationTransformer::new(",
            "fn new(",
            BTreeSet::from(["crates/emitter/src/declarations/selection.rs".to_owned()]),
        ),
        (
            "get_declaration_transformers(",
            "fn get_declaration_transformers(",
            BTreeSet::from(["crates/emitter/src/declarations/orchestration.rs".to_owned()]),
        ),
        (
            "transform_declaration_unit_for_harness(",
            "fn transform_declaration_unit_for_harness(",
            BTreeSet::new(),
        ),
    ];
    let mut actual = symbols
        .iter()
        .map(|(symbol, _, _)| ((*symbol).to_owned(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();

    for path in files {
        let relative = path
            .strip_prefix(&workspace)
            .expect("inside workspace")
            .to_string_lossy()
            .replace('\\', "/");
        if relative.contains("/tests/") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("readable Rust source");
        for line in source.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
                continue;
            }
            for (symbol, definition, _) in &symbols {
                if line.contains(symbol) && !line.contains(definition) {
                    actual
                        .get_mut(*symbol)
                        .expect("symbol row exists")
                        .insert(relative.clone());
                }
            }
        }
    }

    for (symbol, _, expected) in symbols {
        assert_eq!(
            actual.remove(symbol).expect("symbol was scanned"),
            expected,
            "production callers of {symbol} changed"
        );
    }
}

struct ControlHost {
    options: CompilerOptions,
    sources: [SourceFileId; 1],
}

impl EmitHost for ControlHost {
    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
    }

    fn current_directory(&self) -> &Path {
        Path::new("/control")
    }

    fn common_source_directory(&self) -> &Path {
        Path::new("/control")
    }

    fn config_file_path(&self) -> Option<&Path> {
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
    fn declaration_file_path(&self, _source: SourceFileId) -> Option<PathBuf> {
        None
    }

    fn reference_target_path(&self, _source: SourceFileId) -> Option<PathBuf> {
        None
    }
}

#[test]
fn declaration_bundle_root_remains_refused_at_the_transform_seam() {
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
        Err(TransformError::Unsupported(
            UnsupportedEmitFeature::BundleRoot
        ))
    ));
}

#[test]
fn declaration_plan_execute_and_printer_refusals_are_retained() {
    let source = SourceFileId::from_raw(0);
    let bundle = EmitOutputPlan::whole_program(vec![EmitOutputUnit::new(
        EmitRoot::Bundle(EmitBundle::new(vec![source])),
        EmitOutputPaths::javascript("/control/out.js"),
        EmitMode::Script,
    )]);
    assert_eq!(
        bundle.validate_bootstrap_shape(),
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
        printer.contains("PrintRequest::Declaration(source) => self.print_declaration("),
        "PrintRequest::Declaration must route through the activated declaration entry"
    );
    let execute = include_str!("../../../emitter/src/execute.rs");
    assert!(
        execute.contains(
            "let EmitRoot::SourceFile(source_id) = unit.root() else {\n            return Err(EmitFailure::Unsupported(\n                crate::UnsupportedEmitFeature::BundleRoot,\n            ));\n        };"
        ),
        "execute must retain the bundle-root refusal before unit execution"
    );
    assert!(
        !execute.contains("transform_declaration_unit_for_harness"),
        "the dormant declaration seam must not be activated from execute"
    );
}

#[test]
fn production_declaration_transform_requires_exactly_one_source_root() {
    let orchestration = include_str!("../../../emitter/src/declarations/orchestration.rs");
    assert!(orchestration.contains("if result.roots().len() != 1 {"));
    assert!(
        orchestration.contains("detail: \"declaration transform must produce exactly one root\"")
    );
    assert!(orchestration.contains("detail: \"declaration transform root must be a source file\""));
}
