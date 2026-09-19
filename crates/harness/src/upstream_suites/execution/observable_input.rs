//! Lossless command inputs shared by recovery census and native replay.
//! Serialization verifies existing harness planners; it does not decode options
//! or independently build a program. Document bytes remain owned by the caller.
use super::{CompilerRootSelection, CompilerSymlinkOperation, UpstreamExecutionInput};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_program::{PreparedProgram, ProgramLoadLimits};

fn sha256(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

pub fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024)
}

pub fn limits_input() -> Value {
    let limits = limits();
    json!({"max_source_files":limits.max_source_files(),
        "max_request_edges":limits.max_request_edges(),
        "max_source_depth":limits.max_source_depth(),
        "max_source_file_bytes":limits.max_source_file_bytes(),
        "max_total_source_bytes":limits.max_total_source_bytes()})
}

fn symlink_input(operation: &CompilerSymlinkOperation) -> Value {
    let CompilerSymlinkOperation {
        phase,
        raw_target,
        raw_link_path,
        anchor,
        normalized_target,
        normalized_link_path,
    } = operation;
    json!({"phase":format!("{phase:?}"),"raw_target":raw_target.as_ref(),"raw_link_path":raw_link_path.as_ref(),
        "anchor":anchor.as_ref(),"target":normalized_target.as_ref(),"link":normalized_link_path.as_ref()})
}

pub fn prepared_summary(program: &PreparedProgram) -> Value {
    json!({
        "current_directory":program.current_directory().display().to_string_lossy(),
        "roots":program.roots().iter().map(|root| root.path().display().to_string_lossy().into_owned()).collect::<Vec<_>>(),
        "source_files":program.source_files().iter().map(|source| json!({
            "path":source.path().display().to_string_lossy(),
            "sha256":sha256(source.snapshot().text()),
            "may_be_emitted":source.may_be_emitted(),
            "implied_node_format":format!("{:?}",source.implied_node_format())
        })).collect::<Vec<_>>(),
        // These are verification digests, not an alternative option decoder.
        "compiler_options_debug_sha256":sha256(format!("{:?}",program.compiler_options())),
        "program_options_debug_sha256":sha256(format!("{:?}",program.program_options()))
    })
}

pub fn artifact_input(
    route: &str,
    input: &Value,
    settings: &Value,
    program: &PreparedProgram,
) -> Value {
    json!({"route":route,"input":input,"settings":settings,"limits":limits_input(),
        "floor":"established","use_case_sensitive_file_names":program.path_context().use_case_sensitive_file_names(),
        "prepared":prepared_summary(program)})
}

pub fn plan_input(
    input: &UpstreamExecutionInput,
    program: &PreparedProgram,
    document: &mut dyn FnMut(&[u8]) -> String,
) -> Value {
    let mut result = match input {
        UpstreamExecutionInput::Compiler(plan) => {
            let (vfs, roots) = match &plan.root_selection {
                CompilerRootSelection::Explicit {
                    vfs_write_order,
                    program_root_units,
                    ..
                }
                | CompilerRootSelection::Config {
                    vfs_write_order,
                    program_root_units,
                    ..
                } => (vfs_write_order, program_root_units),
            };
            let units = plan.fixture.units.iter().map(|unit| json!({
                "id":unit.id.0,"name":unit.name.as_ref(),
                "content_sha256":unit.content.as_ref().map(|text|document(text.as_bytes())),
                "file_options":unit.file_options.iter().map(|s| json!([s.name.as_str(),s.value.as_str()])).collect::<Vec<_>>(),
                "original_fixture_path":unit.original_fixture_path.as_ref(),
                "document_symlinks":unit.document_symlinks.iter().map(symlink_input).collect::<Vec<_>>()
            })).collect::<Vec<_>>();
            let symlinks = plan
                .fixture
                .global_symlinks
                .iter()
                .chain(
                    plan.fixture
                        .units
                        .iter()
                        .flat_map(|unit| unit.document_symlinks.iter()),
                )
                .map(symlink_input)
                .collect::<Vec<_>>();
            json!({"route":"recorded-compiler","floor":"established",
                "fixture_path":plan.fixture.source.relative_path.as_ref(),"fixture_blob_sha1":plan.fixture.source.git_blob_sha1.as_ref(),
                "variant":{"configuration_index":plan.variant.configuration_index,"key":plan.variant.key.as_ref(),
                    "description":plan.variant.description.as_ref(),"upstream_name":plan.variant.upstream_name.as_ref(),
                    "overrides":plan.variant.overrides.iter().map(|s|json!([s.name.as_str(),s.value.as_str()])).collect::<Vec<_>>()},
                "current_directory":plan.current_directory.as_ref(),"use_case_sensitive_file_names":plan.use_case_sensitive_file_names,
                "settings":plan.effective_settings.iter().map(|s| json!([s.name.as_str(),s.value.as_str()])).collect::<Vec<_>>(),
                "units":units,"config_unit":plan.fixture.config_unit.map(|id|id.0),
                "vfs_write_order":vfs.iter().map(|id|id.0).collect::<Vec<_>>(),
                "program_root_units":roots.iter().map(|id|id.0).collect::<Vec<_>>(),"vfs_symlinks":symlinks,
                "global_symlink_directives":plan.fixture.global_symlink_directives.iter().map(symlink_input).collect::<Vec<_>>(),
                "global_symlinks":plan.fixture.global_symlinks.iter().map(symlink_input).collect::<Vec<_>>(),
                "root_selection_debug":format!("{:?}",plan.root_selection)})
        }
        UpstreamExecutionInput::Project(plan) => {
            let files = plan.fixture.mount.files.iter().map(|file| json!({
                "path":file.virtual_path.as_ref(),"content_sha256":document(file.source.decoded.as_bytes()),
                "upstream_blob_sha1":file.source.git_blob_sha1.as_ref()
            })).collect::<Vec<_>>();
            json!({"route":"recorded-project","floor":"established",
                "descriptor_utf8":{"content_sha256":document(plan.fixture.descriptor_text.as_bytes())},
                "descriptor_raw":{"content_sha256":document(&plan.fixture.descriptor_raw)},
                "descriptor_blob_sha1":plan.fixture.source.git_blob_sha1.as_ref(),
                "descriptor_path":plan.fixture.source.relative_path.as_ref(),
                "current_directory":plan.fixture.current_directory.as_ref(),"project_root":plan.fixture.project_root.as_ref(),
                "scenario":plan.fixture.scenario.as_ref(),"module_variant":format!("{:?}",plan.module_variant),
                "descriptor_module_override":plan.descriptor_module_override,
                "mount":{"virtual_path":plan.fixture.mount.virtual_path.as_ref(),"case_sensitive":plan.fixture.mount.case_sensitive,
                    "read_only":plan.fixture.mount.read_only,"files":files},
                "root_selection_debug":format!("{:?}",plan.fixture.root_selection)})
        }
    };
    result["limits"] = limits_input();
    result["prepared"] = prepared_summary(program);
    result
}
