Don't add the `!declaration_syntax` gate: in a bundle it is the ungated type-end phase that makes d.ts match TS. I withdraw the r106 suggestion. No code change is needed; add a few bundle and per-file d.ts controls instead.

## What TS does (vendored 6.0.3, `/tmp/r80/t.mjs`, declaration on, JSDoc comments)

The sources are uninitialized `let x /** name */: number /** type */;` and initialized `let y … = 1;` in a module file, plus the same pair as script variables `s` and `t`:

| Emit | Uninitialized (`x`, `s`) | Initialized (`y`, `t`) |
|---|---|---|
| **Bundle** (AMD ES5, System ES2017, script `outFile` ES2017) | `let x /** name */: number /** type */;` | `let y /** name */ /** type */: number /** type */;` |
| **Per-file** CommonJS ES5 | `x /** name */: number /** type */` | `y /** name */: number /** type */` (no extra) |

So in a bundle the name's type node survives into d.ts printing and TS's trailing phase at the type's end stays live. The extra `/** type */` before `:` is correct TS output there.

## Why TS behaves this way

1. **Every typed declaration gets the type node.** The TS transform's `visitVariableDeclaration` (`_tsc.js:95093-95106`) calls `setTypeNode(updated.name, node.type)` whether or not there is an initializer.
2. **Only per-file emit discards it.** Per-file emit disposes those emit nodes after the JS transform. In a bundle they reach d.ts printing, which matches native's snapshot/restore path (`execute.rs:1057-1063`, `orchestration.rs:178-190`, `parsed_metadata.rs`).
3. **The container rule decides the printed difference, not whether metadata exists.**
   - The d.ts declaration is updated from the source declaration, so it keeps the source range.
   - Uninitialized: the declaration's end equals the type's end, so `end === containerEnd` and the type-end phase is suppressed.
   - Initialized: the declaration's end is after `= 1`, so the phase fires, giving the extra `/** type */`.
4. **d.ts declarations have no initializer.** Native's `data.initializer.is_none()` branch is therefore the path every d.ts variable takes, including ones initialized in the source.

## What this means for e80dfc18f

The current ungated branch reproduces TS in all four cells:
- **Bundle:** the type node is restored and read.
  - Uninitialized: `retains_end` is true, so the phase is suppressed.
  - Initialized: `retains_end` is false, so it emits.
- **Per-file:** the d.ts arena is fresh with no type node, so nothing extra is printed.

A `!declaration_syntax` gate would drop the bundle `/** type */` for `y` and `t`, which is a real regression. It also isn't needed for the JS side.

That rests on two assumptions I haven't confirmed in native source:
- the restored `type_node` sits on the same name node the d.ts tree prints;
- the d.ts declaration keeps the source declaration's full range, including the initializer.

Both are part of the earlier bundle work you mention. If either is wrong, the new controls below will show it.

## Smallest action

- No code change.
- Add read-only TS observations as controls. The ten current cases only cover the ambient per-file d.ts, so they can't distinguish these cells. Use exactly the probe's inputs, uninitialized and initialized, module and script, typed with JSDoc comments on name and type:
  1. a bundle with a module, via `outFile`, AMD or System;
  2. a script bundle (`module` None with `outFile`);
  3. per-file non-ambient CommonJS.
- For JS output, the probe also shows `exports.y /** type */ /** name */ = 1;` (AMD and per-file) and `exports_1("y", y /** name */ /** type */ = 1)` (System). If you include the JS side of the same commands, those rows test the existing export-assignment paths too, which this change doesn't touch.

The frozen census `context.rs` identity is noted; nothing more is needed there.
