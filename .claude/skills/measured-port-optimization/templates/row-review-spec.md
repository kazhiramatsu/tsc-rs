# Row review (read-only): named functions where one Rust port of the TypeScript compiler spends more cycles than another

<!-- A brief for one read-only review of ONE service (or of the load phase). Fill the angle brackets; keep every path
     RELATIVE to the review's working tree (copy the tables into its untracked `target/account/`, link the other port's
     source as `target/other-port` and the Go reference as `target/reference-go`). No personal details, no home paths. -->

You are looking at two Rust ports of the TypeScript 7.1 compiler. Both are ports of the Go compiler `tsgo` and keep its
function names in snake case.
- OURS: this repository (the working tree is the code of the measured binary). Checker `crates/checker/src`, type
  tables `crates/types/src`, binder `crates/binder/src`, scanner and parser `crates/syntax/src`, program loader and
  module resolution `crates/program/src`, record stores `crates/store/src`.
- THE OTHER PORT: `target/other-port` (the source of its measured binary). Its `notes/` directory records its own changes
  with their measured effects.
- THE REFERENCE: `target/reference-go/internal/checker` (and its sibling packages): the Go source both are ported from.

## What was measured
Cycle-weighted samples with whole call stacks of both binaries, one thread, on a performance core, <projects>. Every
sample of the type check was put on its DEEPEST stack frame whose function belongs to a SERVICE (a list of the
reference's function names, the same list for both ports); a helper with a port's own name is charged to the service
that called it. The services partition the check, and the differences add up to the whole gap. Units: million cycles
(M) of one one-thread run.

<the service table: ours minus theirs by service and project, and the whole check>

Files (relative to the repository root, plain text):
- `target/account/services-<project>.txt`: the service table of a project and, under each service, the functions that
  hold its cycles on each side and the service functions that only one port shows as stack frames.
- `target/account/join-<service>.txt`: one service's functions JOINED BY NAME over the projects. A line:
  `function  ours|theirs diff` per project, then the owning type on each side (`-` = that port has no frame of that name
  there). A function is "the sample's nearest own function" = where the cycles are spent.
  READ A LINE WITH CARE: the two ports cut their functions differently. A name on one side only is usually the same
  work under another name or inlined into its caller on the other side - find the counterpart before calling it an
  excess, and say which lines you paired.

## Your task
For the service named below, explain the rows of its work list: for each row, what OUR code does that costs the
cycles, what the other port does instead, what the reference does, and the change to OUR code that removes the
difference. Then rank the changes.

## Hard rules
- READ-ONLY. Do NOT run cargo, rustc, tests or any build. Do NOT modify, create or delete files. Allowed: file reads,
  `rg` / `grep`, `git` read commands<, and the read-only reader named below>.
- Every proposal must keep every output byte-identical (diagnostics text and order, emitted files, exit codes) at
  every thread count and must keep the work counters Types / Instantiations / Symbols unchanged. The order in which
  types, symbols, signatures, lists and names are created must not move. If a difference you find would change any
  of these (the other port leaves out work the reference does on some paths), label it ELISION and do not propose it.
- A proposal of the kind "remember a pure answer" (a memo) is allowed when the remembered value cannot differ from
  recomputing it: say exactly what the answer depends on, when that becomes final, and what would invalidate it.
- Do NOT copy code or comments from the other port or from the Go reference into a proposal. Describe the mechanism
  in your own words and say how it maps onto OUR structures (ids into tables, not pointers).
- No general advice, no style notes, no "consider". A finding counts only with `file:line` on OUR side, `file:line` on
  the other port's side and in the reference, the mechanism, and the row's numbers it explains.
- A row's numbers are an UPPER bound of what its change can give. Do not promise more.

## Already known - do not re-propose
<what is level or cheaper in ours, what is already written or was tried and failed, what is out of scope>

## Output (markdown on stdout, in English)
1. ROWS: one section a row of the work list, in the list's order. Each: the lines of the join table you paired;
   OUR `file:line`; the other port's `file:line`; the reference's `file:line`; what differs (the mechanism, in your
   words); the proposed change to OUR code, concrete enough to implement (which function, which data, where it is
   written, read and invalidated); why outputs and counters cannot change; the expected gain per project in M
   (bounded by the row) with your confidence (high / medium / low); the unit test that would pin it.
   If a row is not an excess after pairing (the same work under two names), say so and stop there.
2. RANKING: the proposals by expected gain summed over the projects, each with a one-line risk.
3. NOT VERIFIED: what you assumed without reading it.
4. FOUND BESIDE THE LIST: at most five further rows of this service's join table that you believe are real, each
   with the same fields.

## The service: <NAME> (`target/account/join-<service>.txt`; ours `<files>`)
Ours | theirs in M, per project <order>. The service: <totals>.

Work list:
1. `<function>` (ours `<owner::name>`, theirs `<owner::name>`): <ours | theirs per project>. <the question to answer>
2. …
<Note what is cheaper in ours and must not be touched.>
