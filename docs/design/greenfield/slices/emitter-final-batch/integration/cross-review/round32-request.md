Independent read-only continuation. No edits, Rust builds/tests, or agents. Fable remains available; report limits explicitly.

Before implementing MissingDeclaration, I found a concrete correction to round28. `visitEachChild` (_tsc.js91318) returns the original node when no table operator exists; MissingDeclaration has NO operator in visitEachChildTable. `createMissingDeclaration`23670 also has zero transform flags, and parseDeclarationWorker attaches modifiers without recomputing flags. A tiny transpileModule custom `after` visitor on both original shapes confirms modifiers RETAIN their Decorator with Identifier/CallExpression after the complete built-in pipeline, in both decorator modes. Thus your round28 explanation that modifierVisitor drops them is wrong, and explicitly eliding modifiers would introduce an unnecessary divergence.

The probe was:
source=`declare let g: <T>(...args: any[]) => any;\n{ @g<number> class C {} }\n{ @g()<number> class C {} }\n`;
transpileModule target ESNext/module CommonJS, experimentalDecorators false/true, after traverses MissingDeclaration and records modifiers. Both results: MissingDeclaration[44,47] retains Decorator Identifier, [70,75] retains Decorator CallExpression; output has the two class-expression statements and no decorator text. Please independently reproduce/read this and revise your proposal.

Rust:
- syntax::try_visit_each_child is a generated structural NodeData mapper, explicitly counterpart to for_each_child, and DOES visit MissingDeclaration.modifiers. Also used by checker/type builder; changing this global structural helper would broaden impact.
- emitter initialize/compute_transform_flags uses `propagates_transform_child_flags` in builtins.rs. Its exclusions currently cover JSDoc recovery type families. TS MissingDeclaration starts at zero with no propagated modifier flags; adding MissingDeclaration to this exclusion may be the small owner-level fix.
- printer has missing statement and expression arms; those must emit no token while leaving ordinary comments/maps phases.

Please compare: (A) correct factory/initialization flag propagation only + explicit printer no-op, retaining modifiers; (B) emitter-local transform-child visitor wrapper that treats MissingDeclaration as opaque across passes; (C) explicit early return per relevant pass. Trace which downstream pass could still visit retained modifiers even when parent flags are zero (module/System traversals, standard/legacy decorators, async/generator). Recommend the smallest correct change, including transformed NodeData/factory flag recomputation. Consider decorator expressions with async arrows, optional chains, helper-requiring spreads that must never request a helper because the missing declaration emits nothing. Do not change immutable parsed syntax or treat as NotEmittedStatement.

The 72-case complete-command MissingDeclaration oracle in scripts/observe-emitter-missing-declaration.mjs and fixtures/emitter-missing-declaration.json was independently captured twice; native admission is not yet enabled. Stage B is committed as d46539938 merged to457fe8ffa and is awaiting the ordinary r30 queue; do not infer B passed.

Round31 G1→G2→G3 ordering makes sense; defer G3 deep design until the MissingDeclaration correction is settled. Need a correct minimal proposal first.
