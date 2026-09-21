//! Original literal-recovery corpus, preserving the strict complete-command comparator.
#[path = "support/complete_command_corpus.rs"]
mod complete_command_corpus;

#[test]
fn newly_admitted_literal_recovery_rows_match_complete_commands_twice() {
    complete_command_corpus::assert_complete_commands_twice(include_bytes!(
        "fixtures/utf16-literal-recovery-corpus.json"
    ));
}
