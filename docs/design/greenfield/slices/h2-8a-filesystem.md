# H2.8a A3: filesystem retry paths and emitted-file status

Kind: runtime; status: focused implementation verified, 2026-09-08. A1/A2 supply ordinary
output-directory admission and common-directory facts. This packet covers the
portable callback filesystem worker used by `FsOutputSink`, plus the shared
command's `TSFILE` path projection. Native CompilerHost/System caches, nested
System write wrappers and optional capabilities retain H2.8b ownership.

The 24 fresh TS6 observations cross six output directories (relative, dot
segments, absolute POSIX, drive, UNC and empty) with ordinary writes, forced
initial failures, create failures and permanent write failures. Sources,
effective options and complete command observations repeat twice. The tuple
includes callback bytes, materialized bytes/BOM, metadata and source lists,
diagnostic streams, emitSkipped, optional result lists/maps, status and exit,
each filesystem operation and error, directory insertion order and actual
materialized files. Six successful controls also replay on MemoryOutputSink.
The VFS models only controlled input/output operations and cannot access the
real workspace. The source libraries come exclusively from the pinned vendor.

`target/h2-8a-filesystem-before.log` records 12 exact and 12 divergent windows.
Four dot-segment and four drive cases first fail status spelling; four UNC
cases first fail the filesystem operation trace. The callback/result comparator
was extracted without changing its checks so both real sinks can consume it.
No expected path or operation is normalized by the comparator. Any later
failure after the first one must also be fixed on the unchanged tuple.

| Step | Fresh upstream owner | Rust action / lifecycle | Exact witness |
| --- | --- | --- | --- |
| A3-1 | `writeFileEnsuringDirectories`, `_tsc.js:16663–16670`; `ensureDirectoriesExist`, 16656–16662 | `FsOutputSink::ensure_parent_directories` normalizes the output only for retry-parent discovery, then walks string directory prefixes to the platform-neutral root. Query from child upward, stop at an existing directory or root, create missing parents from outermost inward; return the first create error without retrying the file. Original callback path and bytes are retained for both writes. | All 24 operation traces; root, dot and drive controls plus errors and the second source after the first failure. |
| A3-2 | `normalizePath`, 5568–5592; `getDirectoryPath`, 5391–5397; `getRootLength`, 5387–5390 | Reuse existing `source_map::paths::{get_normalized_absolute_path,get_root_length}` with an empty base for the reduced emitted filename. Add a private sink `directory_path` string helper with root-aware slicing. Every emitted artifact here has a filename; trailing-directory-only public API inputs are not introduced. The existing source-map components preserve leading relative parents and clamp rooted parents, so no OS-native `Path::parent` is used to infer drive/UNC roots. | Twelve POSIX/relative controls and twelve drive/UNC/dot windows; no extra root create/query. |
| A3-3 | `emitFilesAndReportErrors`, 129452–129459; `getNormalizedAbsolutePath`, 5493–5596 | `compiler::cli::emit_command_status` uses shared `program::canonical_emit_path(path,cwd,true)` for each status entry. The `true` argument preserves spelling/case while normalizing absolute path syntax. Keep callback and result-list filenames untouched and keep existing diagnostic/exit ordering. | Dot segments disappear only from TSFILE text; drive names remain rooted on every host OS; all 24 command observations. |

Allowed production paths: `crates/emitter/src/sink.rs` and
`crates/compiler/src/cli.rs`, restricted to these owners. The integrator is the
single writer. Existing source-map path helpers and Program canonicalization
are read/hash-pinned dependencies; a behavioral change there requires an
amendment. No transform, printer, source-map recorder, source host I/O, new
output product, option admission or custom callback API is added. Artifacts
remain immutable owned values, and filesystem errors retain the write-only
boundary's existing stable string representation.

`E-OUTPUT-SCRIPT` is modified-requalify. `E-PLAN-SCRIPT` is premise-unchanged,
rechecked on current source; `E-OUTPUT-FUTURE` is future-owned-fail-closed for
remaining API/build products. The old architecture qualification refs are
research inputs, never substitute evidence for this packet. A3's architecture
and final qualification are recorded with the full H2.8a close.

Before either runtime edit, run
`python3 scripts/check-output-filesystem-readiness.py`. The manifest pins the
packet, observer/fixture, upstream owners, concrete Rust dependencies, baseline
owners and tests. Readiness requires three traceable steps, six owner spans,
three architecture dispositions, all 24 cases and zero unresolved/undispositioned
rows. A2's retained-position-index refinement is an independent pending native
implementation detail; its existing A2 gate remains the owner and the final
combined root regression validates it.

After implementation, the complete A3 comparison and A1/A2 controls must pass.
Run the emitter artifact-sink/whole emitter contracts, the real filesystem CLI
controls, and the changed CLI unit tests. H2.7c and map/reference regressions
remain required by the enclosing train, followed by all-target Clippy and
hosted acceptance before landing. No certificate walk or historical full
developer gate is claimed. Cargo build jobs remain two, oracle generation one
process, and ordinary Program comparisons one test thread. Slow OS executable
startup is observed to completion, never restarted as a source failure.

```sh
taskpolicy -b nice -n 15 node scripts/observe-output-filesystem.mjs --check
python3 scripts/check-output-filesystem-readiness.py
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test contracts h2_8a -- --nocapture --test-threads=1
```

This packet does not close H2.8a: full corpus migrations, additional map
intersections, root diagnostic package-format context, output collision axes
and final inventory/architecture/canary admission remain in the same train.

The corrected A3 comparison passes all 24 filesystem tuples twice, including
six additional successful Memory/Fs pairs. The combined A1/A2/A3 run passes
116 cases (six tests, 244 complete commands, 212.17s after compilation), in
`target/h2-8a-filesystem-after.log`. This run also validates A2's retained-index
refinement and the extracted command comparator. The filesystem oracle
`--check` reproduces all 24 tuples. Program/whole-emitter, adjacent declaration
and map/reference, CLI and Clippy checks continue before final qualification.
Before logs are also retained outside the worktree at
`tsc-rs-h2-8a-a2-a3-before-lt1qxren` under the system temporary directory.
