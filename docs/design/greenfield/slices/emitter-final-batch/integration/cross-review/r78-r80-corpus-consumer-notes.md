# Exact parser-change corpus replay consumers (provisional until native checks)

Census capture remains frozen at 67df86615. The selector propagates its full input manifest. Consumers never locate alternative fixtures by case ID: recorded plans use a unique source/configuration key followed by a case-ID assertion, and artifact routes replay the captured inline input. The native loader input is reserialized through the same observable_input code and compared byte-for-byte.

Opus rounds78/79 compared loader extraction with a separate TypeScript mirror. We chose the mirror to keep production loaders unchanged. It is pinned against execution.rs, project.rs, the existing directory overlay, and CompilerOptions. The recorded compiler config host uses insensitive raw-unit lookups under /.src, independently of the final host's case mode; this corrects the initial review suggestion to use the final host mode.

Code and census input workspaces are separate: current native code uses the immutable census data workspace so the original absolute library mount is preserved. Both vendor trees, census ancestry, data cleanliness and artifact bytes are verified. Evidence is local; absolute library paths are retained, not normalized by basename.

All 101 CompilerOptions fields and 6 public ProgramOptions fields are compared before command results. The Rust snapshot uses an exhaustive struct pattern; TypeScript's schema guard detects field additions. Callback text, paths and messages retain raw UTF-16 alongside the UTF-8 projection. NoEmit commands run twice for diagnostic evidence and never count as emit qualification. Build-info observer gaps receive an explicit unsupported disposition.

Validation so far: selector8 tests pass; input/observer16 tests pass including8 pre-existing EF7 complete-command self-check cases; comparator4 mutation tests pass. Rust compile and3 native consumer unit tests are pending the running r77 battery, then the census runs sequentially. These files do not retire any KNOWN row.
