Stage C candidate, before native validation

The parser predicate permits a uniquely matched retained MissingDeclaration only at statement-list positions, in addition to Stage B missing-await recovery. It still refuses skipped-token/reparse actions, missing provenance, duplicate matches, and unhandled zero-width identifiers. The earlier literal-only predicate remains unchanged.

TypeScript retains MissingDeclaration modifiers, binds their reachable locals, and collects dynamic imports. Semantic transforms treat the node as opaque and the printer emits no node body. We therefore preserve structural cloning, identifier collection, module discovery and namespace-local reservation traversal, while stopping semantic visitor and transform-effect scan descent. Named class/function expressions do not reserve their own name through container locals; descendant locals still count. Claude r34 agreed after direct upstream probes.

The System top-level-await scan is unchanged: retained decorator awaits entail Reparsed actions and remain outside Stage C admission.

Frozen complete TypeScript controls (two identical executions each): 72 adjacent declarations, 144 transform effects, 150 scripts/EOF/relative imports, and 60 local-name controls. Script dynamic imports intentionally include dep.js output; import.meta intentionally makes the file a module. None of these fixtures is a native pass claim. Whole-corpus census, native complete-command comparison, original-row retirement, hosted registration, inventory and final regression remain pending.
