**Decision: approve to test, with four small additions and one optional hardening.** I found no ownership hole or logic error in the gate for the variable-statement shapes it targets. Before native execution, add controls for the admitted variable-statement shapes the 64 inputs don't yet cover. This is from reading `git diff d0edc1a74 HEAD` at 01a2dcdbe plus TS probes (`/tmp/r80/ee.mjs`); I made no edits or builds.

Your correction is accepted: the missing node keeps its full start (13). Neither the node nor the `=` anchor should move to the diagnostic start (20).

## The gate

- **Pairing is unchanged.** The new conjunct only widens which parents are accepted. The existing `missing_positions.remove(&position) != Some(Identifier)` check still runs, so each zero-width name must still consume exactly one recorded parser missing-node event at its own position with a retained diagnostic index. Your tampered position, index and duplicate tests exercise this.
- **Slot conditions:**
  - the parent is a zero-width `TypeReference` with no type arguments, whose `type_name` is this identifier and which starts where the name starts;
  - that reference is the `VariableDeclaration`'s `r#type` field;
  - the declaration's name is an `Identifier`, so destructuring is excluded;
  - the parent is a `VariableDeclarationList` owned by a `VariableStatement` through `declaration_list`, so `for`/`for-in`/`for-of` heads are excluded.

  Parameters, properties, nested type, union and qualified slots, and expression positions all stay closed. Your negative syntax tests cover each of these.
- **Two missing types in one list** each consume their own event, so admitting both is consistent with the ownership invariant.
- **Code.** `parents.as_ref().unwrap()` is safe today. `parents` is `Some` whenever `allow_statement_gaps` is true, and the only caller that enables context recovery (`is_supported_for_emit`, via `has_supported_emit_recovery`) passes every flag as true.
  - *Optional hardening:* `parents.as_ref().is_some_and(|p| Self::is_missing_variable_type_slot(source, p, id))` removes a latent panic if a future profile ever enabled context recovery without statement gaps.
  - The rest compiles by inspection. It reuses existing `NodeData` fields and `reachable_parents` output, and the `NodeData::VariableStatement(data) if …` match is well-formed.

## Admitted shapes the controls don't cover yet

The 64 cases are 12 sources covering exported, local, script, const, var, uninitialized, multiple declarations, namespace and comment cases, each across module systems, targets, checked/noCheck and emit outputs. The gate also admits these `VariableStatement` shapes:

1. **`using` / `await using` declarations.** These are `VariableStatement` lists with the `USING` flag, and the ESNext lowering rebuilds the declarations.
   - TS for `export {}; declare const f: () => Disposable; { using x: = f(); }` at ES2022: JS goes through `__addDisposableResource`, d.ts is `export {};`, diagnostic 1110.
   - Either add this control (plus `await using` in an async context), or exclude them in the gate with a list-flags check. Excluding is the smaller choice if you don't want to qualify the `using` lowering now.
2. **Ambient `declare`.** `declare let d: ;` and `export declare let e: ;`.
   - TS: JS is `export {};`, d.ts is `export declare let e: ;`. The non-exported ambient `d` is dropped. Diagnostics: 1110 twice.
   - This goes through a different d.ts path.
3. **Definite assignment.** `export let x!: = 1;`. The gate doesn't look at `exclamation_token`, so this is admitted. Observe TS's JS, d.ts and maps.
4. **Function-local declarations lowered by hoisting at ES5.**
   - `export function* g() { let x: = 1; yield x; }`
   - `export async function h() { let x: = 1; await 0; return x; }`

   The zero-width type is copied onto the hoisted or cloned name through native `merge_from`. The r104/r116 type-comment phases should then do nothing because the range is empty, but that's unmeasured.

Optional: an `allowJs` `.js` input `let x: = 1; export {};`. TS emits `let x = 1;` with diagnostics 8010 and 1110. The parse is identical, but the JS-source emit path differs.

The h2-8c route fixtures and the eight known rows are correctly left unchanged, and their rerun will show the effect. Transpile routes are covered by that rerun, not by the 64 ordinary commands, which is fine.
