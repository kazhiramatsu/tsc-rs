//! Logical module-table census; runs the loader only, never the checker.
use std::path::PathBuf;
use tsc_diagnostics::JsStr;
use tsc_host::{CompilerHost, FsCompilerHost};
use tsc_program::{
    load_config_program_with_no_emit_override, parse_config_root_plan, CompilerConfigHost,
    ConfigRootPlanRequest, LibraryCatalog, ModuleExtension, ProgramLoadLimits, ProgramPath,
    ResolutionOutcome,
};

// Payload bytes of strings exceeding JsString's 15-byte inline capacity.
// This counts retained text, excluding allocator capacity/slack.
fn text_bytes(text: JsStr<'_>) -> usize {
    let len = text.as_bytes().len();
    if len > 15 {
        len
    } else {
        0
    }
}
fn path_bytes(path: &ProgramPath) -> usize {
    text_bytes(path.display()) + text_bytes(path.canonical().as_js())
}

fn main() {
    let config = std::fs::canonicalize(std::env::args().nth(1).expect("config path"))
        .expect("canonical config");
    let base = config.parent().unwrap();
    let host = FsCompilerHost::new(
        base,
        FsCompilerHost::from_process()
            .unwrap()
            .use_case_sensitive_file_names(),
    )
    .unwrap();
    let plan = parse_config_root_plan(
        &CompilerConfigHost::new(&host),
        ConfigRootPlanRequest {
            file_name: config.to_str().unwrap().into(),
            text: std::fs::read_to_string(&config).unwrap(),
            base_path: base.to_str().unwrap().into(),
        },
    )
    .unwrap();
    let catalog = LibraryCatalog::typescript_7_1(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs",
    ));
    let program = load_config_program_with_no_emit_override(
        &host,
        &plan,
        &catalog,
        ProgramLoadLimits::new(1_000_000, 10_000_000, 10_000, 1 << 30, 16 << 30),
    )
    .unwrap();
    let (
        mut own,
        mut different,
        mut original,
        mut package,
        mut alternate,
        mut diagnostics,
        mut arbitrary,
    ) = (0, 0, 0, 0, 0, 0, 0);
    let (
        key_source_bytes,
        mut specifier_bytes,
        mut loaded_bytes,
        mut unloaded_bytes,
        mut other_bytes,
    ) = (0, 0, 0, 0, 0);
    for (key, row) in program.resolutions().modules() {
        specifier_bytes += text_bytes(key.specifier());
        if let Some(path) = row.alternate_result() {
            alternate += 1;
            other_bytes += path_bytes(path);
        }
        diagnostics += usize::from(!row.diagnostics().is_empty());
        if let ResolutionOutcome::Resolved(module) = row.outcome() {
            let target = module.target();
            if let Some(id) = target.source() {
                if target.resolved_file() == program.source_file(id).unwrap().path() {
                    own += 1;
                } else {
                    different += 1;
                    loaded_bytes += path_bytes(target.resolved_file());
                }
            } else {
                unloaded_bytes += path_bytes(target.resolved_file());
            }
            if let Some(path) = module.original_path() {
                original += 1;
                other_bytes += path_bytes(path);
            }
            // Retained package strings are counted once in the side table below.
            package += usize::from(module.package_id().is_some());
            if let ModuleExtension::Arbitrary(text) = module.extension() {
                arbitrary += 1;
                other_bytes += text_bytes(text.as_js());
            }
        }
    }
    let storage = program.resolutions().module_storage();
    other_bytes += storage.package_string_bytes;
    println!("row_size={} ranges={} rare_boxes={} rare_bytes={} unloaded_boxes={} unloaded_box_bytes={} package_ids={} package_id_bytes={}",
        storage.row_size, storage.range_bytes, storage.rare_boxes, storage.rare_bytes, storage.unloaded_paths, storage.unloaded_path_bytes,
        storage.package_ids, storage.package_id_bytes);
    println!("rows={} inline={} key_source={} specifier={} loaded={} unloaded={} other={} own={} differing={} original={} package={} alternate={} diagnostics={} arbitrary={}",
        program.resolutions().module_len(), storage.row_bytes,
        key_source_bytes, specifier_bytes, loaded_bytes, unloaded_bytes, other_bytes, own, different, original, package, alternate, diagnostics, arbitrary);
}
