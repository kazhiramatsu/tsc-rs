Case 2 is confirmed, with an exact fix. For case 1 I narrowed the owner to the ES5-only lowering path and ruled out `await`, but I could not pin the exact line from maps alone; the capture run will settle it. This was code reading, the r71 log, the frozen fixtures, and Node probes against the vendored 6.0.3 compiler; nothing was built.

## 1. ES5 `method-decorator/script`: missing statement end mapping (2 rows)
**Evidence:**
- The only map difference is TS-only `(53,4)→0:25`: the end of `var C = function () { … }();`, mapped to the end of the class.
- The start `(35,0)→0:0` is present in both TS and native, and every other segment matches.

**What the probes show** (`transpileModule`, CommonJS, `sourceMap: true`):

| Input | ES5 | ES2015 | ES2022 |
|---|---|---|---|
| `class C { @await m() {} }` | statement mapped `0:0 … 0:25` (start and end) | only the name, `(35,4)→0:6` | same as ES2015 |
| `class C { @dec m() {} }` | identical pattern, ending at `0:23` | same | same |

**Conclusions:**
- It isn't specific to `await`. It's a general ES5 member-decorated standard-decorator class.
- The standard-decorator pass's `let C = (() => …)()` statement carries no source map in TS (the ES2015/ES2022 columns). The class-range map on the `var` statement therefore comes from the ES5-only lowering (the ES2015 transform, plus the printer path for that statement), not from `standard_decorators.rs`.
- Native gets the start from some source and loses the end. Typical causes are a merged `NO_TRAILING_SOURCE_MAP` flag, an invalid range end, or the start coming from a different node than the end.
- I didn't find the upstream line that attaches the class range at ES5. ES2015 `visitClassDeclaration` (105144-105165) ranges its own `var` statement to the class, but that is the inner class wrapper, not this outer statement. So I'm not proposing a patch without the native bytes.

**Next steps:**
- In the capture run, identify which native node emits `(35,0)`: statement, declaration list, or declaration. Then compare its emit flags and range end against the upstream statement.
- **Controls to add:** `class C { @dec m() {} }` at ES5 and ES2015, script and module, plus a decorated getter, setter and field. These separate "any decorated member at ES5" from the method-specific case.
- As you said, this is almost certainly not caused by the new factory rules. The factory touches parentheses, not statement map ranges.

## 2. ESNext `static-modifier` (4 rows): the fix is confirmed
- **Upstream:** `emitClassStaticBlockDeclaration` (117915-117920) is `writeKeyword("static")` plus the body. It never emits modifiers, which the parser assigns after the factory call (34055).
- **Rust:** the `ClassStaticBlockDeclaration` arm in printer.rs (around 5758-5778) calls `emit_modifiers` first, so `@await` is printed. That explains the extra `(2,5)`/`(2,10)` segments and the shifted body segments.
- **Fix:** delete the `emit_modifiers` block (and its trailing space) in that arm, keeping `static`, a space, and the body. There's no need to touch the AST. Comments inside the decorator's text range become leading trivia of the static-block node, as they are upstream; the four rows will confirm that.
- **Neighbouring printer paths:** PropertyAssignment and ShorthandPropertyAssignment already omit their post-assigned modifiers, matching upstream `emitPropertyAssignment`/`emitShorthandPropertyAssignment` (119516-119535). Nothing to change there.

**System `await-static-decorator`:** your proposed change is correct, if native confirms the failure. In `source_contains_top_level_await`, walk only `data.body` for `ClassStaticBlockDeclaration`. That matches `createClassStaticBlockDeclaration`, whose flags come from the body alone (21961), and the one-bit parser projection. Don't skip static blocks wholesale: an `await` in the body really does make `execute` async.

## Controls
- **Case 1:** the four decorator shapes above at ES5 and ES2015, script and module, together with the capture comparison.
- **Case 2:** the existing four rows plus `class C { /*c*/ @dec static { } }` at ESNext, with comments on and off, to pin where the comment goes once the modifiers are no longer printed.
- **System:** the pending `await-static-decorator` rows. Change `source_contains_top_level_await` only if they fail.