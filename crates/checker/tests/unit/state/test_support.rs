use tsc_binder::Binder;
use tsc_diagnostics::{DocumentVersion, TextSnapshot};
use tsc_syntax::{parse_source_file_from_snapshot_in_identity_domain, ParseOptions, SourceFile};
use tsc_types::{CompilerOptions, IdentityDomain};

use super::CheckerState;

/// Multi-file program construction mirroring check_program's parse/
/// bind base chaining (M4 5.0).
fn parse_program(
    files: &[(&str, &str)],
    require_clean_parse: bool,
    domain: &IdentityDomain,
) -> Vec<SourceFile> {
    let mut sources: Vec<SourceFile> = Vec::new();
    for (name, text) in files {
        let source = parse_source_file_from_snapshot_in_identity_domain(
            (*name).to_owned(),
            TextSnapshot::new((*text).to_owned(), DocumentVersion::default()),
            ParseOptions {
                script_kind: tsc_syntax::ScriptKind::ensure_from_file_name(name),
                js_doc_parsing_mode: tsc_syntax::JSDocParsingMode::ParseAll,
                ..ParseOptions::default()
            },
            None,
            domain,
        )
        .expect("test source identities");
        if require_clean_parse {
            assert!(
                source.parse_diagnostics.is_empty(),
                "test source must parse cleanly: {:?}",
                source.parse_diagnostics
            );
        }
        sources.push(source);
    }
    sources
}

/// tsrs-native: checker unit-test harness for constructing an
/// in-memory Program and borrowing its CheckerState.
pub(crate) fn with_program_state<R>(
    files: &[(&str, &str)],
    options: &CompilerOptions,
    run: impl FnOnce(&mut CheckerState) -> R,
) -> R {
    with_program_state_impl(files, options, true, run)
}

/// tsrs-native: test-only checker construction for grammar
/// suppression canaries whose source intentionally has parse errors.
pub(crate) fn with_program_state_allow_parse_diagnostics<R>(
    files: &[(&str, &str)],
    options: &CompilerOptions,
    run: impl FnOnce(&mut CheckerState) -> R,
) -> R {
    with_program_state_impl(files, options, false, run)
}

fn with_program_state_impl<R>(
    files: &[(&str, &str)],
    options: &CompilerOptions,
    require_clean_parse: bool,
    run: impl FnOnce(&mut CheckerState) -> R,
) -> R {
    let domain = IdentityDomain::new();
    let mut sources = parse_program(files, require_clean_parse, &domain);
    let binders: Vec<Binder<'_>> = sources
        .iter_mut()
        .map(|source| {
            Binder::bind_in_identity_domain(source, &domain).expect("test bind identities")
        })
        .collect();
    let binder_refs: Vec<&Binder<'_>> = binders.iter().collect();
    let mut state = CheckerState::from_program(binder_refs, options);
    // Mirror the driver (lib.rs): the augmentation passes and the
    // amalgamated-duplicates flush run between construction and
    // the file checks (A8 moved the flush there).
    state.merge_module_augmentations();
    run(&mut state)
}
