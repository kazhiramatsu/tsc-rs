# EF3-CASE-CANONICAL — oracle host fix proposal, re-mint and verification procedure

Status: **proposal + evidence only**. `crates/oracle/*` and `ratchets/*` are single-writer
(integrator) surfaces; nothing here is applied. The two rows stay **unresolved** in this batch
until the re-mint and the Rust comparison below are complete.

## 1. Rows and cause

| row | frozen (6c oracle) | tsc-rs (final bytes) | cause |
| --- | --- | --- | --- |
| `compiler/sourceMapWithNonCaseSensitiveFileNamesAndOutDir.ts#default` (H2.6c-map-observation; also H2.8a global) | map `sources: ["../testFiles/app.ts"]`, exit 0 | map `sources: ["app.ts"]`, exit 0 | 6c host ignores `@useCaseSensitiveFileNames: false` |
| `compiler/sourceMapWithNonCaseSensitiveFileNames.ts#default` (H2.6c; also H2.7de) | 2 writes, `sources: ["../testFiles/app.ts", …]`, TS5101 (`outFile` deprecation), exit 2 | typed refusal `useCaseSensitiveFileNames` (`execute.rs validate_emit_request`, `outFile` + case-insensitive host guard) | same, plus the guard |

`crates/oracle/h2-6c-qualification.mjs` (`createProgramCase`, 720-731; `createProjectProgramCase`,
1102-1111) hard-codes `useCaseSensitiveFileNames: () => true` and `getCanonicalFileName: identity`
and treats `@useCaseSensitiveFileNames` as harness-only (`HARNESS_ONLY_OPTIONS`, dropped). The
compiler-runner harness applies that directive to its virtual file system host, and the
directive-honoring sibling oracles already do so:
`h2-8a-observations.mjs` / `h2-7de-observations.mjs` read `input.use_case_sensitive_file_names`
and canonicalize by lower-casing (frozen H2.8a observation of the same `…AndOutDir` test:
`sources: ["app.ts"]`, exit 0; frozen H2.7de observation of the `outFile` test:
`sources: ["app.ts", "app2.ts"]`, TS5101, exit 2).

## 2. Proposed host change (option 1)

Patch (unified diff, not applied): [`h2-6c-qualification.case-canonical.patch`](h2-6c-qualification.case-canonical.patch).

- `useCaseSensitiveFileNamesSetting(settings)` reads the harness directive (default `true`).
- `getCanonicalFileName = ts.createGetCanonicalFileName(useCaseSensitiveFileNames)` and
  `useCaseSensitiveFileNames()` report the directive.
- The VFS keeps its original path spellings (`unit.name` → SourceFile name, emitted paths,
  `realpath`) and is looked up through canonical keys (`vfsByCanonicalPath`,
  `symlinkByCanonicalPath`); `createHermeticDirectoryOverlay` receives the same flag (it already
  canonicalizes with `ts.createGetCanonicalFileName` and preserves child spellings).
- `createProjectProgramCase` (project route) needs the identical change; the patch shows the
  program route, the project route mirrors it line for line. `createParseConfigHost` (361) already
  reports `useCaseSensitiveFileNames: false` for config parsing — the integrator should decide
  whether it, too, follows the directive (it does not affect these two rows).
- Other hosts with the same hard-coded pattern (`grep -l "getCanonicalFileName: (fileName) => fileName" crates/oracle/*.mjs`,
  ~50 scripts: h1-*, h2-1a … h2-7b) are out of scope here; §5 lists the frozen sets whose cases
  carry the directive.

## 3. Old/new observation diff (standalone mirror of the 6c host; `remint/remint.mjs`)

The mirror reproduces the frozen 6c writes **byte-exactly in its "old" shape** (validation of
fidelity; `newLine: CarriageReturnLineFeed`, `noErrorTruncation`, `skipDefaultLibCheck` as the
oracle sets them), then re-runs with the proposed shape ("new"). sha256 prefixes of the write
callbacks (`remint/*.json` hold the full texts):

| case | write | old (= frozen) | new | change |
| --- | --- | --- | --- | --- |
| sourceMapWithNonCaseSensitiveFileNames | testfiles/fooResult.js.map | `3a57600186c2` `sources: ["../testFiles/app.ts","../testFiles/app2.ts"]` | `083fcce15b19` `sources: ["app.ts","app2.ts"]` | map only |
| | testfiles/fooResult.js | `c509c0df0dc2` | `c509c0df0dc2` | none |
| | diagnostics / exit | TS5101 / 2 | TS5101 / 2 | none |
| sourceMapWithNonCaseSensitiveFileNamesAndOutDir | testfiles/app.js.map | `1f654066f84d` `["../testFiles/app.ts"]` | `662f9daa8fe3` `["app.ts"]` | map only |
| | testfiles/app.js | `d679070b9688` | `d679070b9688` | none |
| | testfiles/app2.js.map | `3cc5edef8d3e` `["../testFiles/app2.ts"]` | `0ed148ada5c5` `["app2.ts"]` | map only |
| | testfiles/app2.js | `cb5ea6d51023` | `cb5ea6d51023` | none |
| | diagnostics / exit | none / 0 | none / 0 | none |
| sourceMapWithCaseSensitiveFileNames (control, directive `true`) | fooResult.js.map / .js | `b09e22e60234` / `a2c5b449f266`, TS5101, exit 2 | identical | none |
| sourceMapWithCaseSensitiveFileNamesAndOutDir (control, directive `true`) | app.js.map / app.js / app2.js.map / app2.js | `1a956f29a6a4` / `e2e1f559f8e5` / `3cc5edef8d3e` / `cb5ea6d51023`, exit 0 | identical | none |

Impact audit of the other corpus cases carrying `@useCaseSensitiveFileNames: false`
(`remint/out-audit/`): `caseInsensitiveFileSystemWithCapsImportTypeDeclarations`,
`symbolLinkDeclarationEmitModuleNamesImportRef`, `missingMemberErrorHasShortPath` — no write /
diagnostic / exit difference between the shapes in the mirror (the mirror does not model `@link`
symlinks or `@currentDirectory`; the real oracle must confirm). `commonSourceDir3` differs
(old: TS5009, exit 2, `A:/foo/bar.js` + `a:/foo/baz.js`; new: exit 0, `A:/bar.js` + `A:/baz.js`),
but it is only frozen in the H2.8a sets, whose oracle already honors the directive.

## 4. Rust comparison against the proposed observations

- `…AndOutDir`: the tsc-rs writes at the final bytes (`capture/ef2-ef3-r8b`, `actual-*`) equal the
  proposed "new" observation **byte for byte on all four writes**
  (`662f9daa8fe3` / `d679070b9688` / `0ed148ada5c5` / `cb5ea6d51023`), with no diagnostics and
  exit 0 — a complete match (writes, map, diagnostics, exit).
- `…NonCaseSensitiveFileNames` (`outFile`): tsc-rs used to refuse the option combination
  (`validate_emit_request`, `outFile` + case-insensitive host). The release candidate lifts that
  guard, and its replay (`capture/ef2-ef3-r8c`, `compare-outfile-candidate.py`) is a **complete
  match** against the proposed "new" observation: 2 writes (`fooResult.js` `c509c0df0dc2…`,
  `fooResult.js.map` `083fcce15b19…`, map `sources: ["app.ts","app2.ts"]`), diagnostics
  `[TS5101]`, exit 2. The guard removal therefore stays in the candidate (`crates/emitter/src/
  execute.rs`). The row is still recorded unresolved until the integrator re-mints the observation
  (§5) and the Rust comparison runs against the minted bytes (§6 steps 5–6).

## 5. Re-mint scope (integrator)

1. Apply the patch to `crates/oracle/h2-6c-qualification.mjs` (both routes).
2. Re-mint H2.6c for the four cases in §3 (two change, two controls must not):
   `ratchets/h2-6c-qualification.v1.json` observations (run fingerprints of the two rows change:
   `75878861f980…`, `e7095dbea59a…`), `ratchets/h2-6c-census.v1.json`,
   `ratchets/h2-6c-known-divergences.v1.json` (the two rows and their mismatch vectors; the
   `…AndOutDir` row is expected to drop out once tsc-rs replays exact ×2),
   `ratchets/h2-7a-w5-stratum-pool.v1.json` and `ratchets/h2-candidate-dispositions.v1.json`
   where the case fingerprints are referenced. H2.7de and H2.8a already hold directive-honoring
   observations for these tests (no change expected; verify by fingerprint comparison).
3. Other sets whose cases carry the directive but whose oracles hard-code the identity canonical
   (`h2-7b-qualification.v1.json`: `caseInsensitiveFileSystemWithCapsImportTypeDeclarations`,
   `symbolLinkDeclarationEmitModuleNamesImportRef`; `h2-5g-qualification.v1.json`:
   `missingMemberErrorHasShortPath`) are a separate decision: §3's audit predicts no byte change,
   but only the real oracle (symlinks, currentDirectory) can confirm.

## 6. Verification procedure

1. Before applying: `node crates/oracle/h2-6c-qualification.mjs --check` (or the current check
   entrypoint) must be green at the current ratchets.
2. Apply the patch; re-mint the four cases; confirm the two controls are byte-identical to before
   (writes, diagnostics, exit, fingerprints) and the two rows changed exactly as §3 predicts.
3. Replay tsc-rs (`cargo test -p tsc-rs-compiler --test emitter_final_rows`) against the re-minted
   observations: `…AndOutDir` must be exact ×2 → retire it from `KNOWN` and propose retirement from
   `h2-6c-known-divergences`.
4. `outFile` row: build the release candidate with the guard lifted (delete the
   `!host.use_case_sensitive_file_names()` arm in `validate_emit_request`, keep the `importHelpers`
   arm), replay; the row must be exact ×2 on writes, map, diagnostics (TS5101) and exit (2).
   Only then commit the guard removal; otherwise keep the guard and the row stays known.
   **Done in the candidate against the mirror observation** (§4: complete match, capture
   `ef2-ef3-r8c`); the integrator repeats this replay against the minted bytes before retiring the
   row from `KNOWN` / `h2-6c-known-divergences`.
5. Run the adjacent H2.6c/H2.6a witness and qualification checks the integrator uses for map
   observations, plus `printer --all` / `compact-body-comments --all` for the emitter surface.
