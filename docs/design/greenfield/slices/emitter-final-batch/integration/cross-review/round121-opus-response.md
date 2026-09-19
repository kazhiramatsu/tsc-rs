Keep the original run (PID 56011) as the authority and don't start a second census yet. First run two cheap read-only measurements: a longer series of `sample`s of the running process, and a small standalone sha2 benchmark. A second run is only worth it if those show it would finish well before the original, and even then it adds evidence alongside the original rather than replacing it.

## What I measured and what I'm estimating

**Checked directly:**
- The 1-second sample has 144/144 stacks ending in `run` → `Census::qualified_artifacts` → `program_facts` → `recovery_parse_snapshot::input` (`recovery_parse_snapshot.rs:41`) → `sha256` (`:15`) → `digest` → `block_buffer::digest_blocks` → `sha2::sha256::soft::compress`.
- `Cargo.lock` pins `sha2 0.10.9`.
- The software implementation is used because of the target, not the build profile. In sha2 0.10.9, `src/sha256.rs` only selects the hardware aarch64 backend when the `asm` feature is enabled, and no workspace crate enables it. Enabling it would be a feature change, which you've excluded; a profile override does not change which implementation runs.
- The workspace profile has no sha2 entry, so sha2 builds at dev opt-level 0. The generated block sets opt-level 3 for binder, checker, conformance, diagnostics, emitter, harness, syntax and types.
- Crates that depend on sha2 as a normal dependency: harness, conformance, fuzz and xtask. compiler only has it as a dev-dependency.

**Estimates, not measured:**
- how much faster sha2 is at opt-level 3;
- what fraction of the remaining run is spent hashing;
- how long the remaining run will take.

One 1-second window can't establish any of these.

## 1. Constraints on a profile-only second run

**Same inputs:**
- the same HEAD 67df86615;
- `--locked` with the unchanged `Cargo.lock`;
- the same toolchain file, target triple and feature set;
- the same census command and inputs;
- a new, separate output path.

**Only change:** `--config profile.dev.package.sha2.opt-level=3`.
- Dev `debug-assertions` and `overflow-checks` stay on, including for sha2. Its compression code uses wrapping arithmetic, so the checks cost time but can't panic.
- The generic `digest` and `block_buffer` wrappers are instantiated inside xtask, so they stay at opt-level 0. Only sha2's own non-generic `compress` gains.

**Why results should match:** SHA-256 in safe Rust is deterministic, so every digest and the snapshot should be byte-identical to the original's. That is an expectation to verify, not something already established.

**Provenance:** this is a different executable from the frozen census receipt, which records inode and hash. Record separately:
- the exact `--config` override;
- the rustc version, `Cargo.lock` hash and target directory;
- the new binary's hash.

Any use as a replacement has to state that the executable changed, differing only in the sha2 build profile, and must also show the snapshot matches the original byte-for-byte.

**Don't reuse or write anything under the original `target/debug`**, including the running `xtask` binary.

## 2. Rebuild scope

- **With a warm copy of the cache (estimate):** the profile setting is part of Cargo's per-crate build hash. sha2 and everything that depends on it would rebuild: sha2, harness, conformance, fuzz and xtask, then the final link. syntax, binder, checker, emitter, program, compiler and the other crates shouldn't rebuild. Roughly minutes, not verified.
- **With a fresh separate target directory:** everything rebuilds, including checker at opt-level 3. That's much longer, but it is the cleanest provenance.
- **With an APFS clone (`cp -c`) of the existing cache:** it only writes to the new copy, but whether Cargo treats the moved cache as up to date is unproven.
  - Build-script outputs and dependency-info files can contain absolute paths back to the original target directory.
  - That can force rebuilds, or quietly read files from the original location (read-only, but it muddies provenance).
  - Proving it's usable takes a real build with `-v` and inspecting the Fresh/Compiling lines, which is not something to do now.
  - If you ever do it: clone only while no build is running, build at low priority, confirm Fresh/Compiling covers only the five expected crates, and check that no absolute path in the new cache points at the original.

## 3. Evidence to gather first

**a. A longer read-only profile of PID 56011.** Run several `sample` windows over a few minutes, for example five 20–30-second samples. This estimates what fraction of time `soft::compress` takes in the current h2-5g stage. The result applies only to this stage, not to earlier stages or the whole run.

**b. A standalone sha2 0.10.9 benchmark.**
- Use a scratch crate outside the workspace, with its own target directory and the same toolchain.
- Pin `sha2 = "=0.10.9"` and enable no extra features.
- Hash input sizes representative of what the census hashes.
- Compare dev at opt-level 0 against dev with only sha2 at opt-level 3.

This gives the speed-up of `compress` alone. It takes seconds to build and touches nothing shared.

**How to decide.** A second run starts from zero across all 14,329 IDs, while the original is already about 5h45 of wall time in (about 173 CPU minutes). To be worth it, the second run must finish well before the original's remaining time, after also paying its own build time. Estimate the second run's length from the hashing fraction measured in (a) and the speed-up from (b). If (a) shows hashing dominating throughout, the case is strong; if it's mostly one stage, it's weak. Treat both numbers as diagnostic evidence only, never as a timing qualification.

## 4. Safest choice

- **Keep the original running unchanged.** It remains the evidence of record.
- **Do (3a) and (3b) now.** They are cheap and isolated.
- **Start a second run only if** those numbers show a large margin. If so:
  - prefer a fresh separate target directory, or a clone that has been verified up to date;
  - run it at low priority on otherwise idle cores;
  - note that both runs use about 1.2 GiB each, so check memory headroom.
- **Treat any second run's snapshot as a cross-check.** It should match the original byte-for-byte. It becomes a replacement only if the original fails, and only with the executable change disclosed and complete output equality shown. If the original finishes first, discard the second run's evidence or keep it just as a consistency check.
