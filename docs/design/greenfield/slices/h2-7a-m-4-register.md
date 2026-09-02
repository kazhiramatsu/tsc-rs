# h2-7a-m-4 — Phase-E register (skeleton; values recorded at E2/E3/E4)

## Witness artifact succession (parent-packet pin move, E3)
- whole-file fingerprint before / after: __ / __
- case-manifest fingerprint: 81d5b13a639f5cc8e74bc82dea62aef8c2b54c70419cd1c575225a0ec24f3ab1 (must reproduce)
- observation-content roll: 4d2e6f6dc52cf5e43356d3e8fd707bf4926638057d60ae98c49581cfb6a42cad (must reproduce)

## L1/L2 domain table (E2, scripted from the frozen artifacts)
| class | cases | windows | owner |
| --- | --- | --- | --- |
| eligible | 116 | 202 (199 unblocked + 3 blocked) | m-4 |
| outFile (F6/references-first, S2/entityname-1, S3/typeofexpr-1) | 3 | __ | H2.7d |
| isolatedDeclarations (S2/latebound-1) | 1 | __ | H2.7c |
Gating denominators (E2 re-measure of the reviewer-measured values): top-level `.changed` 742 / subtree `.changed` 496 / declBlocked 202 / trackSymbol 533 / reportInferenceFallback 362 / singleton tracker lanes 1 + 1 / eligible emit diagnostics 3 / expected declaration writes 199.

## Factory-face census (E2)
| upstream face | Rust face | status (exists / additive-P2) |
| --- | --- | --- |
| getGeneratedNameForNode | get_generated_name_for_node | additive-P2 |
| updateExportAssignment | update_export_assignment | additive-P2 |
| (remaining ~66 faces) | __ | __ |

## Reached-helper closure (E2; the 116 reached rows of the module)
| upstream helper | disposition (ported-here / reused / owned-later(slice)) |
| --- | --- |

## Message-name lookup (E2): 19 module names + 115 diagnostics.ts names against crates/diagnostics/src/gen.rs — misses: __ (expected 0)

## Counting-grammar note (E2)
Non-row items ported under the owner header: pushErrorFallbackNode :114292-114300, popErrorFallbackNode :114301-114303, throwDiagnostic :114266, restoreFallbackNode :114279, the three getSymbolAccessibilityDiagnostic lambdas :114433/:115289/:115617.

## Resolver partition succession (P-head)
m-3-head partition rows: 7 → 10 (create_type_of_declaration_in_expando_scope, is_last_bodiless_overload_of_symbol, is_first_declaration_of_symbol) — TO-VERIFY at the inventory re-mint.

## E4 surprise-trigger assessment (operator)
| trigger | fired? | disposition |
| --- | --- | --- |
