use crate::state::test_support::with_program_state;
use tsc_types::CompilerOptions;

#[test]
fn captured_diagnostic_attempts_reach_the_enclosing_walk() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let before = state.diagnostics.attempts;
        let (_, captured) = state
            .with_collected_diagnostics(|state| {
                for _ in 0..2 {
                    state.error_at(
                        None,
                        &tsc_diagnostics::gen::Cannot_find_name_0,
                        &["missing"],
                    );
                }
                Ok(())
            })
            .expect("captured diagnostic scope");
        assert_eq!(captured.len(), 1);
        assert!(state.diagnostics.is_empty());
        assert_eq!(state.diagnostics.attempts - before, 2);
    });
}
