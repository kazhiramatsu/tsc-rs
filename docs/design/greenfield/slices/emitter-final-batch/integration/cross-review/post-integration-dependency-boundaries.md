# Follow-up: isolate emit and diagnostic responsibilities

Status: post-integration design investigation, not a refactoring authorization for the current closure train. Requested by the user on 2026-09-19 after the r101 boundary audit. Current work stays limited to demonstrated compatibility defects and final qualification.

The r101 audit demonstrates coupling between transformation provenance, comment ownership, text emission, temporary-name assignment and source-map recording. A valid text range does not imply a parsed original; an original node does not donate every comment/map flag; a synthesized parenthesis owns boundaries independently of its child. These distinct meanings are carried through shared node metadata and printer paths, making small corrections require broad regression checks.

A future design should make those contracts explicit before moving code. Candidate boundaries are a documented transformed-node interface (including original identity, raw/comment/map ranges and generated binding identity), a text/token output interface, and independently testable comment/map consumers. Output locations remain a necessary shared fact between emitted text and maps. Shared mutable ownership and implicit phase ordering are candidates for removal, subject to measured compatibility and performance.

The checker/emitter dependency requires a separate source-level inventory. The current audit does not prove that the printer fixes changed checker diagnostic behavior; running checker regressions is a verification obligation, not evidence of such a change. Record actual call and data dependencies before deciding which interface should move.

Use the frozen complete-command cases and source-map suppression controls as migration contracts. Compare text, maps, diagnostics, callback metadata and failure behavior independently as well as together. Introduce one boundary at a time behind unchanged public behavior; retain the full integration gate. Do not combine that migration with current source corrections or weaken exact comparisons.

Concrete starting evidence: `round103-opus-response.md`, `round104-opus-response.md`, `native-r101-final-differences.json`, and the r104 complete-command controls. No implementation choice or milestone closure is claimed here.
