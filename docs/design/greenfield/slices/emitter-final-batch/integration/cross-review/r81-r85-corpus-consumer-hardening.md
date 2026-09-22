# Selected-corpus consumer review

The original native consumer at `8f63a3651` compiled and passed all three focused tests (1,153.228 seconds). The updated native consumer has not compiled yet; its five focused tests remain pending.

Actual Claude Opus reviews 82, 84 and 85 are archived alongside this note. Review 82 identified the canonical JSON hash ordering problem. Review 84 approved the consumers after fixing config syntax diagnostics and adding numeric guards. Its B1 config identity objection was withdrawn in review 85: local inspection showed that both `with_config_file_path` and `without_config_file_path` clear the config source. The TypeScript snapshot correctly retains `null` for project config sources.

The consumers now sort canonical JSON keys explicitly and reject floats or unsafe integers. They reproduce Established list trimming, preserve raw document pool bytes, decode compiler VFS BOMs once, and retain the project mount's already-decoded text. Project config conversion preserves located errors while eliding the config source passed to the program. Root JSON syntax diagnostics precede conversion diagnostics on both routes.

Option comparisons cover all 101 CompilerOptions fields and the relevant ProgramOptions paths, default library, config identity, diagnostics and ownership fields. Config auxiliary sources are covered indirectly by mounted bytes, options and config diagnostics, with source pins on prepared.rs and config.rs. This is not an exhaustive structural comparison of ProgramOptions. Reconstruction failures and TypeScript repetition failures block their rows separately; source order equality is recorded. No KNOWN rows have been retired.

Validation: 24 Node tests passed in 9.081 seconds; nine selector tests and six comparator tests passed. The selector test's initial helper-name error was corrected before its successful run. The eight existing EF7 self-checks only validate serialization, not the full recorded corpus reconstruction. The original native preflight's unused Result warning was fixed by explicitly discarding the removed row in the existing test mutation; this does not change behavior.
