# h2-7b-w5 — experiment register

Packet: [h2-7b-w5.md](h2-7b-w5.md), revision 2. Trusted base:
`main @3f90839e`. Train / PR: `h2/7b-w5`, #509.

## Result and validation

The candidate removes 16 of the 18 H2.7b divergence rows. Every removal has a
passing control comparing the complete frozen TypeScript observation, including
diagnostics, writes and their metadata, emit result, source maps and exit code.
The two survivors retain exactly the same mismatch vectors and fingerprints.
Full-corpus regression validation runs once in unchanged hosted
`cargo xtask acceptance`; its final-head result is recorded in the PR body.
The duplicate local H2.7b sweep was deliberately terminated before completion,
and is not a passing result.

| local check | result | execution time |
|---|---|---|
| compiler focused + CLI / emit-session / filesystem / H1 I/O contracts | 85 passed, 10 existing Node-oracle audits ignored | 35.83 s |
| checker library | 1,705 passed | 1.14 s |
| emitter library | 460 passed | 0.28 s |
| types library | 30 passed | 0.00 s |
| source-comment topology contracts | 18 passed | 0.00 s |
| diagnostic conformance | 5,908 fixtures / 7,691 cases; 49,024 / 49,024 matched; T0/T1/T2/T3=100%; FP=0, FN=0 | not separately timed |
| formatting / whitespace | passed | — |

The added alias-resolution unit control initially included an unrelated unused
local suggestion. Its input was corrected and oracle-checked; the final full
checker run above is green. No implementation diagnostic was suppressed.

## Workflow findings

- The edit loop uses direct Cargo focused tests. `cargo xtask test <role>`
  delegates to the same Cargo test operation (`workspace_maintenance.rs:67`)
  after rebuilding the dev-profile launcher and its dependencies.
- First reproduction: 2 expected failures, 1.57 s execution, preceded by
  2m37s dev and 1m49s test builds. The final compiler contract build took
  2m30s. The final diagnostic launcher build took 7m41s including an earlier
  artifact-lock wait. Build time dominates these local test bodies.
- Preparation commits `8ac859fe` / `24b6205e` from the stopped concurrent
  session are retained. Codex owns the implementation in this checkout.
- The preparation head's successful hosted check does not validate these
  implementation bytes. The PR must pass hosted acceptance after this commit.
- No historical source/profile certificate walk or full developer
  `cargo xtask ci --baseline 3f90839e` was run for this user-authorized pilot.
  Skipped checks are not represented as passing; hosted case coverage and
  upstream expected results are unchanged.

## Remaining investigations

`checkingObjectWithThisInNamePositionNoCrash`: the upstream minimal-source trace
shows that correct diagnostic display comes from syntactic return-expression
inference while the signature return cache itself still becomes `any`.
The old cache-order hypothesis is incomplete; the missing diagnostic-renderer
route is described in the packet.

`recursiveConditionalTypes`: upstream's minimal recursive-box getter first
resolves its symbol type during `inferFromProperties`, then subsequent accesses
reuse it. The first differing Rust resolution frame is still unproved.
Neither remaining diagnostic is suppressed or removed from the exception set.
