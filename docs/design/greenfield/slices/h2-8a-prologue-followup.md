# Parameter prologue and System helper follow-up

Start: `7ad75593e` (production `8884fcc05`). Existing fixed exporters and receipts remain immutable.

New ordinary-source inputs are observed twice with the existing TypeScript 6.0.3 observer before production changes. Complete tuples include diagnostics, JavaScript/maps, declarations/maps, write callbacks and emit status.

| Group | Commands | Baseline exact twice | Diagnosis |
| --- | ---: | ---: | --- |
| parameter-binding | 36 | 36 | Object/array patterns with defaults, patterns without defaults alongside a hoisting sibling, and non-hoisting controls at ES2015/ES2022/ESNext × set/define. Existing cause 6 implementation matches; no production change required. |
| parameter-class-fields | 24 | 20 | Ordinary function and concise-arrow parameters initialized by a class with a static field fail only at ES2015 set/define; object-pattern and empty-class controls match. Dedicated class-fields owner reached without decorators. |
| system-map-followup | 4 | 2 | Two existing decorated System cases fail maps; undecorated constant/function controls match. System TS5107 diagnostics are retained and compared. |

Baseline receipts: `ratchets/h2-8a-prologue-followup-baseline-*.v1.json`; captures, archives and binaries: `target/decorator-prologue-followup-runs/baseline-*/`.

## Source owners and repair plan

- Parameter binding: `visitParameterList` → `addDefaultValueAssignmentForBindingPattern` (`_tsc.js:91168–91238`) → `standard_decorators.rs`. The four hoisting source forms exercise alias/default/no-default branches; ESNext define and the two non-hoisting source forms provide controls.
- Class-fields downlevel: `addDefaultValueAssignmentForInitializer` (`_tsc.js:91239–91276`) → `class_fields/downlevel.rs::lower_parameter_default`. Clone condition/assignment names, suppress the assignment target and initializer maps, and map the assignment/block to the parameter. This is already implemented by the retained class-fields and decorator owners. Investigate any concise-arrow remainder independently.
- System helper ownership: `transformSystemModule` moves source helpers onto `moduleBodyBlock` (`_tsc.js:112093–112145`); the printer emits helpers after directives while writing that block. Rust instead inserts helper text into finished output, rejects map recording for that route, and `execute.rs` emits an empty fallback map when declarations are enabled. The simple controls disprove a blanket absence of System mappings. Emit helpers at their structural body location before recording subsequent generated positions.

This follow-up does not claim all decorator paths or all H2.8. CFG integration is tracked independently in PR #514.

## Class-fields repair

The parameter map repair and `convertToFunctionBlock` return/block ranges (`_tsc.js:20665–20671`) make all 24 dedicated class-fields commands exact twice, exit 0. The concise-arrow difference was separately visible in the baseline; its return and block previously had synthesized ranges. Focused evidence is `ratchets/h2-8a-prologue-followup-class-fields.v1.json` (the recorded dirty input hashes, not a relabelled later commit). No expected output changed.

## System repair and integration

The printer now locates the transformed `System.register` function body and emits its helpers after its directive prologue, while the writer is recording generated positions. The finished-output splice and its map refusal are removed, as is the splice-only `GeneratedText::insert_at_utf8_boundary` method. The original System group is 4/4 exact twice, including the two formerly failing maps; see `ratchets/h2-8a-prologue-followup-system-focused.v1.json`.

Four additional helper controls cover directives plus a shebang, noEmitHelpers, no declarations, and no maps. All were observed twice upstream. The first native attempt completed three cases exactly; the noEmitHelpers case stopped in the harness option adapter before creating a Program (actual test exit 101). Its failed receipt/log is preserved; the adapter now copies that existing option to `CompilerOptions::no_emit_helpers`. Comparison semantics and all observations remain unchanged. Final verification must cover all four.

CFG branch `15b6a6038` was merged without conflicts into this candidate at `9f60a6547`. Its tree is identical to the previously measured/documented `083b3160e` tree. The CFG completion checker passes after integration; final command regression checks run on the combined production.
