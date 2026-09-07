# TypeScript 7 and native LSP direction

Status: user-directed roadmap amendment, 2026-09-06. The user delegated where
to consult the Go implementation and included TypeScript 7.0 and new features
from the Go implementation in the intended follow-on scope. This records that
direction; it does not claim any TypeScript 7 feature is implemented yet.

This amendment supersedes the mandatory tsserver product / separate LSP
adapter sequence in the older post-H1 and L2-L5 plans, and their requirement
for separate approval merely to begin post-6.0.3 work. Routine selection and
sequencing within this direction belong to the implementer.

## Reference selection

The current compiler acceptance baseline remains the vendored TypeScript
6.0.3 artifact until an explicit profile transition. TypeScript 7 is the next
compatibility target, including its new features. Consulting its Go source
may begin now; adopting an observable behavior uses the corresponding pinned
upstream tests and an explicit version transition.

| Area | Primary implementation reference and adoption rule |
| --- | --- |
| Language Service and LSP | Use the TypeScript 7 Go implementation from the outset. Rust provides LSP directly, with typed service operations and LSP request/result contracts. |
| Editor projects, Program reuse, caches and cancellation | Use the Go session/snapshot/project organization to inform Rust ownership and scheduling. Preserve the already established source identity and lifetime contracts; qualify reuse against fresh computation. |
| New build/watch implementation | Prefer the Go builder, restart and watch design, selecting the matching upstream observations for the version being admitted. Existing shared compiler behavior stays covered by its accepted tests. |
| Existing parser, checker, resolver and emitter | Consult both versions when changing a producer. Reuse applicable Go algorithms without replacing working Rust code solely to mirror Go. Intentional TypeScript 7 semantic changes are admitted together with their tests. |
| New Go-era features | Maintain a worklist of upstream features and intentional changes, their dependencies, reference commits, tests and adoption status. Introduce dependency-closed groups; do not wait for a complete legacy tsserver product. |

## Native LSP product

The intended structure is:

```text
editor <-> Rust LSP server <-> language services / editor projects
                                      <-> versioned Program / compiler
```

The legacy tsserver wire protocol, compatible Session commands, and a
tsserver-to-LSP bridge are outside the required product. Project selection,
open-document overlays, snapshots, watches, cancellation, stale-result
suppression and resource bounds remain necessary internal functionality.
L3-L5 implementation packets will regroup those responsibilities around the
native server. Public TypeScript Compiler API compatibility is a separate
product, not a blanket prerequisite for the LSP's internal interfaces.

## Introducing TypeScript 7 behavior

The first follow-on task is the [native test/build/debug workflow](typescript-7-workflow.md),
ahead of further H2 implementation, as directed by the user on 2026-09-06.
Run compiler fixtures and native FourSlash tests at a fixed commit and use
filtered print debugging as the ordinary investigation loop. Stack capture
and, when necessary, Delve support deeper investigations.

Next, map the existing H2 remainder onto the selected TypeScript 7 tests:
retained behavior, intentional changes, removed options, and new features.
The native runner skips ES5/System and rejects outFile at the reviewed pin;
do not equate that with completion of the corresponding 6.0.3 rows, or
automatically implement all legacy rows before adopting TypeScript 7.
Record any retirement when transitioning the accepted profile. Shared
compiler fixes remain covered by the current accepted baseline in the meantime.
Use the native design for the subsequent subsystems; TypeScript 7 adoption
does not require first implementing every TypeScript 6 tooling product.

For each admitted feature, record the upstream commit, observable change,
shared-producer dependencies and complete upstream observations. Retain the
6.0.3 evidence as the previous baseline and identify intentional differences
when transitioning the relevant accepted profile. Do not regenerate existing
expectations simply to conceal a regression. This direction does not require
two permanent runtime modes or two independent compiler implementations.

The upstream change list already documents intentional differences in
JavaScript/JSDoc checking and JavaScript declaration emit. Those are version
changes to account for, not interchangeable implementation details. LSP
capabilities likewise need their own admitted operation set; a feature that
depends on a new compiler behavior must bring that dependency with it.

Keep the lightweight edit loop: complete observations for the affected
features and adjacent regressions while editing, grouped product checks,
and the full acceptance boundary for the admitted profile before landing.
Updating an upstream reference alone does not change CI, vendored inputs,
accepted sets, or a completion claim.

## Confirmed source locations

Development moved from the temporary `microsoft/typescript-go` repository
back to `microsoft/TypeScript`. This review used commit
`1f70213d4922b434345f639b441681e470c7cfc1` of the latter. Future work records
its own selected commit rather than treating moving `main` as a test oracle.
The binary built from this pin reports `Version 7.1.0-dev`; this is a native
7-series development reference, not a claim about a released 7.0 artifact.

- [Native LSP server](https://github.com/microsoft/TypeScript/blob/1f70213d4922b434345f639b441681e470c7cfc1/tsc/internal/lsp/server.go#L2093): definition and completion handlers directly call the Language Service and return LSP types; the server owns a project session.
- [Language Service](https://github.com/microsoft/TypeScript/tree/1f70213d4922b434345f639b441681e470c7cfc1/tsc/internal/ls) and [project/session implementation](https://github.com/microsoft/TypeScript/tree/1f70213d4922b434345f639b441681e470c7cfc1/tsc/internal/project).
- [Intentional Go-port changes](https://github.com/microsoft/TypeScript/blob/1f70213d4922b434345f639b441681e470c7cfc1/tsc/CHANGES.md), read through the official GitHub API at that commit.
- [Repository-move notice](https://github.com/microsoft/typescript-go/blob/89d5d5b2849a0db0957065889ca58536fa6d2e4a/README.md).

The detailed future feature denominator and implementation packets remain
work to do. This amendment changes the roadmap and reference policy, not
the already merged W5 code or its validation evidence.
