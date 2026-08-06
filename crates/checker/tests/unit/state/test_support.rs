use tsc_binder::Binder;
use tsc_syntax::{parse_source_file, LanguageVariant, ParseOptions, SourceFile};
use tsc_types::CompilerOptions;

use super::CheckerState;

/// Multi-file program construction mirroring check_program's parse/
/// bind base chaining (M4 5.0).
fn parse_program_with_target(
    files: &[(&str, &str)],
    script_target: tsc_types::ScriptTarget,
    require_clean_parse: bool,
) -> Vec<SourceFile> {
    let mut sources: Vec<SourceFile> = Vec::new();
    for (name, text) in files {
        let (node_id_base, node_array_id_base) = match sources.last() {
            Some(previous) => (previous.arena.node_end(), previous.arena.array_end()),
            None => (0, 0),
        };
        let javascript_file = name.ends_with(".js") || name.ends_with(".jsx");
        let jsx_file = name.ends_with(".tsx") || name.ends_with(".jsx");
        let source = parse_source_file(
            (*name).to_owned(),
            (*text).to_owned(),
            ParseOptions {
                script_target,
                language_variant: if javascript_file || jsx_file {
                    LanguageVariant::Jsx
                } else {
                    LanguageVariant::Standard
                },
                javascript_file,
                node_id_base,
                node_array_id_base,
                js_doc_parsing_mode: tsc_syntax::JSDocParsingMode::ParseAll,
                ..ParseOptions::default()
            },
            None,
        );
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
    let sources =
        parse_program_with_target(files, options.emit_script_target(), require_clean_parse);
    let mut binders: Vec<Binder<'_>> = Vec::new();
    for source in &sources {
        let (seed, base) = match binders.last() {
            Some(previous) => (previous.next_symbol_id(), previous.symbols.next_id().0),
            None => (1, 0),
        };
        let mut binder = Binder::with_bases(source, options, seed, base);
        binder.bind_source_file();
        binders.push(binder);
    }
    let binder_refs: Vec<&Binder<'_>> = binders.iter().collect();
    let mut state = CheckerState::from_program(binder_refs, options);
    // Mirror the driver (lib.rs): the augmentation passes and the
    // amalgamated-duplicates flush run between construction and
    // the file checks (A8 moved the flush there).
    state.merge_module_augmentations();
    run(&mut state)
}
