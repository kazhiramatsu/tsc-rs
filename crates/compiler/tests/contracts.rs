// Share the unchanged scalar path observer once per test crate.
#[path = "../../host/tests/support/scalar_path.rs"]
mod utf16_scalar_path;

#[path = "integration/automatic_type_directive_session_contract.rs"]
mod automatic_type_directive_session_contract;
#[path = "integration/cli_contract.rs"]
mod cli_contract;
#[path = "integration/declaration_emit_resolver_members.rs"]
mod declaration_emit_resolver_members;
#[path = "integration/declaration_import_attributes.rs"]
mod declaration_import_attributes;
#[path = "integration/emit_session_contract.rs"]
mod emit_session_contract;
#[path = "integration/filesystem_loader_contract.rs"]
mod filesystem_loader_contract;
#[path = "integration/h2_7a_ca_controls.rs"]
mod h2_7a_ca_controls;
#[path = "integration/h2_7a_m4_controls.rs"]
mod h2_7a_m4_controls;
#[path = "integration/library_loader_session_contract.rs"]
mod library_loader_session_contract;
#[path = "integration/no_resolve_session_contract.rs"]
mod no_resolve_session_contract;
#[path = "integration/original_path_session_contract.rs"]
mod original_path_session_contract;
#[path = "integration/preparsed_adoption_contract.rs"]
mod preparsed_adoption_contract;
#[path = "integration/preserve_symlinks_session_contract.rs"]
mod preserve_symlinks_session_contract;
#[path = "integration/program_session_contract.rs"]
mod program_session_contract;
