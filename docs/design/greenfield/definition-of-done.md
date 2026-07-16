# tsrs2 definition of done

One page, normative. If any other doc disagrees with this one about
WHAT "done" means, this doc wins; file a doc fix. (Milestone HOW
lives in [greenfield.md](../greenfield.md) §§7-8 and the steps docs.)
Adopted 2026-07-14 after the external full-project review; the
review's checkpoint table is folded in below.

## What tsrs2 is

A batch **diagnostics checker** for TypeScript, byte-compatible with
**TypeScript 6.0.3 exactly** (the vendored
`tsrs2/vendor/typescript-6.0.3/lib/_tsc.js` bundle and its lib set —
the same artifact the oracle runs). "tsc compatibility" in any tsrs2
context means THIS bundle, not tsc-at-large.

## Done means

On the versioned conformance corpus (`ts-tests/tests/cases/conformance`,
matrix-expanded by the harness — 5,908 fixtures at adoption), fixed for
each gate run and growable only through A1's append-only reviewed
universe transition, using the
two-view contract in [m8-readiness.md](m8-readiness.md):

1. **Supported-scope T3 = 100%** for the syntactic AND semantic oracle passes:
   every diagnostic matches on file/code/line/col (T0), category
   (T1), full span + top message text (T2), and message chain +
   relatedInformation (T3). The full-corpus FN residue remains
   visible, and **all-corpus FP = 0 is absolute at every point in
   between** (the standing invariant, not just at the end). Scope
   exclusions are exact reviewed diagnostic identities; no
   fixture/code/glob exclusion exists.
2. **Supported-scope T4 per case**: the rendered-output hash equals the oracle's
   (`tsrs_cli_hash == oracle_cli_hash` in the goldens) — ordering
   and dedupe included.
3. **Suggestion-pass rows are in scope** insofar as they are
   emit-free (the M7 band: unused/grammar/suggestion). They already
   sit in the All-band denominator (band filtering is code-based);
   emit-DEPENDENT suggestions (if any surface) get exact
   `emit-dependent` scope dispositions plus the corresponding
   not-applicable function-ledger disposition instead of silent
   omission.
4. **Zero escapes**: no `Unsupported` containment sites remain
   (`cargo xtask escapes` reports sites=0), the site manifest
   (`tsrs2/escapes.toml`) is empty, and every ledger entry is
   hash-fresh against the vendored bundle. Every checker
   pub/pub(crate) function carries a DISPOSITION (the tsc-port
   header family / tsrs-native / tsc-deferred / tsc-not-applicable
   — the `fn-dispositions.toml` backlog allowlist admits ONLY
   deletions and empties before M8 starts); the tsc-side converse
   (emitter inventory + dependency closure over the SAME exact
   tsc-span/tsc-hash declaration identities; function names are aliases,
   not keys) is the M8-start checkpoint. Parse-recovery guards may
   be separately ratcheted through M7, but must move off the
   `Unsupported` channel before this final gate.
5. **Determinism invariants green** at full scope: idempotence,
   jobs-independence, prefix-determinism, encodings,
   matrix-independence.
6. **Corpus-external confidence** is CLAIMED ONLY as: the
   differential fuzzer introduced by the M8-start gate (generator +
   oracle comparison + reducer + signature dedupe) reaches M9's CI
   steady state — new divergence signatures < 1/night — with no
   known-open divergence class.
   This is an engineering bar, not a formal guarantee.

## Explicitly out of scope

- **Emitter** (no JS/d.ts output; emit-dependent diagnostics get
  dispositions, per [2xxx-first-order.md](2xxx-first-order.md)).
- **Module HOST resolution**: node_modules, package.json,
  `paths`/baseUrl, project references, `/// <reference>` redirects
  ([program-and-modules.md](program-and-modules.md)) — the checker
  resolves only in-program files + ambient/pattern-ambient modules
  (m4-58 §9). Exact affected oracle diagnostics receive
  `host-resolution` scope dispositions. They stay FN in the
  all-corpus visibility metric and are not chased.
- **LSP / watch / incremental** ([lsp-and-incremental.md](
  lsp-and-incremental.md) is design-only). Preconditions if ever
  started: owned lib cache (no `Box::leak`), collision-safe keys.
- **Public TypeChecker API** surface.
- **JS-file checking depth**: plain-JS files check to the
  plainJSErrors allowlist; JSDoc-driven semantics are out until a
  future decision revisits this line. Exact affected diagnostics use
  `jsdoc-semantics` dispositions; non-JSDoc assignment-declaration
  semantics are not excluded by this rule.
- **Upstream tracking** (>6.0.3): a separate project with its own
  re-vendor + goldens-regeneration + ledger-refresh loop; nothing
  here promises forward compatibility.

## Performance / memory bounds

- Full-corpus conformance (lib-loaded, 5,908 fixtures) stays under
  **60 s wall** on the reference dev machine (current: ~15 s;
  ratchet the regression, not the number).
- Peak RSS for the corpus run is currently UNMEASURED; measure and
  record a ceiling before M8's mining loop starts (the leaked lib
  bundles are an accepted batch-mode cost until the LSP
  precondition above).

## Go / no-go checkpoints (external review, 2026-07-14)

| Gate | Bar |
|---|---|
| M4 close | T0 ≥ 35% (All band), untagged escapes 0, stale 0 — first real go/no-go |
| M5 close | flow landed with idempotence + jobs-independence still green |
| M6 start | speculation scoped-transaction API + failed-candidate rollback tests exist ([m6-inference-calls-steps.md](m6-inference-calls-steps.md) precondition) |
| M8 start | `cargo xtask m8 readiness --require-ready`: M7 gate, globally identity-anchored frozen exact scope, T1-T3 shadow metrics, declaration-identity all-band emitter inventory + dependency closure + runtime coverage, current-fingerprint fuzzer evidence, and current performance/RSS evidence on an approved reference runner |
| Done | this page's §"Done means", all six clauses |

Shadow T1/T2/T3 rates are measured (non-gating) from pre-5.8a
onward; a fixture family that reaches completion may ratchet its
tier early, but the GATES stay T0+FP=0 until M8 activates the
higher tiers corpus-wide.
