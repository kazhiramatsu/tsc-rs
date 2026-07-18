# Non-2XXX quick wins: implementation steps

Companion to `non-2xxx-blockers.md` phase N0. Five INDEPENDENT
workstreams (QW-A..QW-E), any order, each its own gated commit on
main (single-gated-commit convention, EXECUTION-GUIDE.md). Unlike the
scaffold steps docs these are BEHAVIOR changes: the gate expectation
is `0 NEW_FP / 0 NEW_FN` with standing FPs removed and adds gained —
each workstream states its expected movement. All anchors below were
verified at `2bf9ec0`; re-grep before editing (line numbers drift).

Shared baseline before ANY workstream:

```sh
git status                      # clean; note HEAD
cargo build --release
cargo test --release            # first suite: 98 passed (or current baseline)
./verify.sh golden-save         # golden must correspond to HEAD
```

After each workstream's final gate: commit, then `./verify.sh
golden-save` before starting the next.

---

## QW-A: enum constant evaluation (kills 18033 FP x43, 1066 FP x<=10)

Fixtures (probe all four before and after):

```sh
python3 scripts/probe.py ts-tests/tests/cases/conformance/enums/enumConstantMemberWithTemplateLiterals.ts
python3 scripts/probe.py ts-tests/tests/cases/conformance/enums/enumConstantMemberWithTemplateLiteralsEmitDeclaration.ts
python3 scripts/probe.py ts-tests/tests/cases/conformance/enums/enumConstantMemberWithString.ts
python3 scripts/probe.py ts-tests/tests/cases/conformance/enums/enumConstantMemberWithStringEmitDeclaration.ts
```

Root cause (verified): `const_eval_enum_init` (`src/checker/stmts.rs`
near line 3278) classifies template literals and string
concatenations as non-constant, so the caller treats them as computed
members (18033 via `is_assignable_to(string, number)`) and, in
`declare` enums, ALSO emits 1066 — tsc emits at most ONE of those per
member (else-if chain).

tsc anchors (read them first): `createEvaluator`/`evaluate`
(`oracle/node_modules/typescript/lib/_tsc.js` near 19382),
`evaluateTemplateExpression` (near 19476),
`computeConstantEnumMemberValue` (near 85632).

### Step A1: extend the evaluator [M]

In `const_eval_enum_init` (`stmts.rs` ~3278), add a `Template` arm
(place it before the final `_ => None`):

```rust
// tsc evaluate: NoSubstitutionTemplateLiteral and TemplateExpression
// with constant spans are string constants (evaluateTemplateExpression)
Expr::Template { parts, .. } => {
    let mut out = String::new();
    for p in parts {
        match p {
            TemplatePart::Str(s) => out.push_str(&s.to_str_lossy()),
            TemplatePart::Expr(e2) => match self.const_eval_enum_init(e2)? {
                EnumValue::Str(s) => out.push_str(&s),
                EnumValue::Number(n) => out.push_str(&crate::js_num::to_js_string(n)),
                EnumValue::Computed => return None,
            },
        }
    }
    Some(EnumValue::Str(out))
}
```

(`TemplatePart` is `ast.rs` ~539: `Str(JsString) | Expr(Expr)`; a
no-substitution template is a `Template` whose parts are all `Str`.
Number formatting MUST go through `crate::js_num::to_js_string` —
never `format!("{}", f64)`.)

Then REPLACE the whole `Expr::Binary` arm body with (current body
handles only Number x Number for Add/Sub/Mul/Shl/BitOr):

```rust
Expr::Binary { op, left, right, .. } => {
    let l = self.const_eval_enum_init(left)?;
    let r = self.const_eval_enum_init(right)?;
    if let (EnumValue::Number(a), EnumValue::Number(b)) = (&l, &r) {
        let (a, b) = (*a, *b);
        let v = match op {
            BinOp::Add => a + b,
            BinOp::Sub => a - b,
            BinOp::Mul => a * b,
            BinOp::Shl => ((a as i64) << (b as i64)) as f64,
            BinOp::BitOr => ((a as i64) | (b as i64)) as f64,
            _ => return None,
        };
        return Some(EnumValue::Number(v));
    }
    // tsc evaluate: `+` concatenates when both sides are string|number
    // (the both-number case is handled above)
    if *op == BinOp::Add {
        let as_str = |v: &EnumValue| match v {
            EnumValue::Str(s) => Some(s.clone()),
            EnumValue::Number(n) => Some(crate::js_num::to_js_string(*n)),
            EnumValue::Computed => None,
        };
        if let (Some(a), Some(b)) = (as_str(&l), as_str(&r)) {
            return Some(EnumValue::Str(a + &b));
        }
    }
    None
}
```

Deliberately OUT of scope (record in notes, do not add here): the
remaining tsc numeric operators (`/ % ** & ^ >> >>>`, unary `+`/`~`)
need JS ToInt32/ToUint32 semantics; identifier/member references
(`evaluateEntityNameExpression`) are a separate leaf.

### Step A2: mirror the else-if chain [M]

In the caller (`stmts.rs` ~3236-3270), the
`Some(EnumValue::Computed) | None =>` arm currently emits 1066 AND
the const-enum error AND the 18033 check cumulatively. Replace the
body so it mirrors `computeConstantEnumMemberValue` exactly — ONE
branch only, const-enum first, and `check_expr` runs ONLY in the
final branch:

```rust
Some(EnumValue::Computed) | None => {
    if e.is_const {
        self.error_at(init.span(),
            &gen::const_enum_member_initializers_must_be_constant_expressions, &[]);
    } else if has_modifier(&e.modifiers, ModifierKind::Declare) {
        self.error_at(init.span(),
            &gen::In_ambient_enum_declarations_member_initializer_must_be_constant_expression, &[]);
    } else {
        let t = self.check_expr(init, None);
        let r = self.types.regular(t);
        let num = self.types.number;
        if !self.types.is_any_or_error(r) && !self.is_assignable_to(r, num) {
            let d = self.display_type(r);
            self.error_at(init.span(),
                &gen::Type_0_is_not_assignable_to_type_1_as_required_for_computed_enum_member_values,
                &[d, "number".to_string()]);
        }
    }
    prev_numeric = None;
    if let Some(msym) = msym {
        self.enums.enum_member_values.insert(msym, EnumValue::Computed);
    }
}
```

Known preserved approximation: tsc keys the ambient branch on
inherited `NodeFlags.Ambient`; the existing direct-modifier check is
kept as-is (note it, don't fix here).

### Verify and commit

```sh
cargo build --release 2>&1 | grep -E "^error" -A5    # nothing
cargo test --release 2>&1 | grep "test result:"       # all ok
# all four probes: zero one-sided (*) main.ts lines
./verify.sh golden-check
```

Expect: `0 NEW_FP / 0 NEW_FN`; standing FPs removed: 18033 x43,
1066 x(3..10); the four `enumConstantMemberWith*` fixtures flip to
exact match. Commit:
`enum-eval 1: template literals and string concat are enum constants`.

| Symptom | Diagnosis | Fix |
|---|---|---|
| Movement in unrelated enum-typed fixtures | Members that were `Computed` are now `Str` — literal member types now flow (intended, mirrors tsc) | Probe the fixture; if the oracle agrees with the new output it is a bonus fix; if not, the divergence is in downstream enum-literal handling — stop-note, do not tweak the evaluator |
| A `"1" + 1`-style member differs from oracle | Number formatting divergence | Must use `js_num::to_js_string` |
| 1066 lines remain in T7 | Else-if chain not applied, or ambient detection missed | Re-check Step A2 ordering |

---

## QW-B: wire the regex validator (adds 1501/1508/1125/1198 family, ~19 files)

Root cause (verified): `src/checker/regex.rs` is a complete port of
tsc's `scanRegularExpressionWorker` pipeline, but its entry point
`check_grammar_regex_literal` (regex.rs ~104) has ZERO callers — the
module is dead code. Even `options.language_version()`
(`src/options.rs` ~257) was pre-built for it.

Canaries:

```sh
python3 scripts/probe.py ts-tests/tests/cases/conformance/es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions01.ts
# expect BEFORE: tsc-only line `main.ts:1:17 TS1501` (flag `u` under es5)
```

### Step B1: call it from the RegexLit check [M]

`src/checker/exprs.rs` ~344, current arm:

```rust
Expr::RegexLit { .. } => self
    .global_type_symbol("RegExp")
    .map(|s| self.types.intern_kind(TypeKind::Iface(s)))
    .unwrap_or(self.types.any),
```

becomes:

```rust
Expr::RegexLit { text, span } => {
    // tsc gates checkGrammarRegularExpressionLiteral on
    // !hasParseDiagnostics(sourceFile); the once-guard keeps the
    // validation single-shot per literal (and, via `quiet`,
    // exploratory-safe) if this expression is ever re-checked.
    if !self.parse_error_files.contains(&self.current_file)
        && self.report_once_node(1501, node_key(e))
    {
        let lv = self.options.language_version();
        crate::checker::regex::check_grammar_regex_literal(
            text, span.start, self.current_file, lv, &mut self.diags,
        );
    }
    self.global_type_symbol("RegExp")
        .map(|s| self.types.intern_kind(TypeKind::Iface(s)))
        .unwrap_or(self.types.any)
}
```

Notes: `1501` here is only the once-guard KEY (`(code, node)` set
entry), chosen from the family so it cannot collide; `node_key` is
already imported throughout the checker; `e` is the match scrutinee
of `check_expr_uncached`.

### Step B2: fix the stale module docblock [M]

`regex.rs`'s header claims "tsrs never runs the checker when any
parse diagnostic exists" — stale since `5412cb1` (statement-level
gate). Reword that sentence to describe the actual gate now in force:
the `parse_error_files` check at the new call site.

### Verify and commit

Standard build/test, then:

```sh
python3 scripts/probe.py ts-tests/tests/cases/conformance/es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions01.ts
# expect AFTER: the 1501 line has no `*` (both sides emit it)
./verify.sh golden-check
```

Expect: `0 NEW_FP / 0 NEW_FN`, adds across
`unicodeExtendedEscapesInRegularExpressions*` (19 files) and possibly
other regex fixtures. THE RISK of this workstream: the validator has
never been exercised against the corpus — NEW_FP in any
regex-bearing fixture means a divergence inside regex.rs itself.
Triage per EXECUTION-GUIDE (micro-fixture with just that literal);
fix inside regex.rs against its cited tsc anchors; two failed rounds
= stop-note. Commit:
`regex-wire 1: run check_grammar_regex_literal at RegexLit`.

---

## QW-C: scanner same-start dedup (kills 1199 FP x~9)

Root cause (verified): for `"\u{r}"` tsc emits 1125 then suppresses
the follow-up 1199 at the SAME position — `parseErrorAtPosition`
(`_tsc.js` ~29467) drops any parse error whose `start` equals the
LAST recorded error's start. tsrs's scanner sink has no such rule, so
`unicodeExtendedEscapesInStrings14/17/20` and
`...InTemplates14/17` show 1125 (matched) + 1199 (FP) pairs.
Ironically `regex.rs` already implements this rule for itself
(`last_error_start`).

### Step C1: mirror the rule in the scanner sink [M]

`src/scanner.rs` `fn error_at` (~589) — prepend:

```rust
// tsc parseErrorAtPosition: consecutive parse errors at the same
// start position collapse to the first
if self
    .diags
    .last()
    .is_some_and(|d| d.start == start as u32 && d.file == Some(self.file))
{
    return;
}
```

Known narrower-than-tsc scope (record in notes): tsc compares against
the last of the MERGED parser+scanner stream; this compares within
the scanner's own sink. The corpus cases at hand are
scanner-consecutive, so behavior matches.

### Step C2: probe the leftover [P]

`unicodeExtendedEscapesInStrings24.ts` (FP 1002 at 1:27,
unterminated string + escape) — probe it after C1. If its FP is gone,
done; if not, its divergence is position-shaped (where the
unterminated-string error lands), NOT dedup-shaped: record it in the
notes as a separate leaf and leave it standing.

### Verify and commit

Standard build/test, probes of the five fixtures above (expect the
1199 FPs gone, the matched 1125 lines still present), then
`./verify.sh golden-check`. Expect `0 NEW_FP / 0 NEW_FN`, roughly
-9 standing FPs. A NEW_FN here means the dedup swallowed a
second-diagnostic that tsc DOES emit — that only happens when tsc's
two diagnostics are non-consecutive; find the fixture, stop-note.
Commit: `scanner 1: same-start parse-error dedup (parseErrorAtPosition)`.

---

## QW-D: static-block statement grammar (adds 18037 x13, 18041, 1163; kills 1375/1378/1108 FPs)

Fixtures: `classes/classStaticBlock/classStaticBlock{6,7,26}.ts`.
Probe classStaticBlock7 first — current shape at 3:9: tsrs emits
1375+1378 (FPs), tsc emits 18037 (FN); at 5:9 tsrs 1108 (FP), tsc
18041 (FN); at 24:13 tsc 1163 (FN, `yield` in a static block nested
in a generator).

tsc anchors: `checkAwaitGrammar` (`_tsc.js` ~79338 — the nearest
function-like-or-static-block container being a static block wins
over every other await rule), `checkReturnStatement` (~84522),
`checkYieldExpression` (a class static block IS a function-like
container, so a yield directly inside one is "not in a generator" →
1163).

Mechanism: tsrs's `fn_stack` does NOT get a frame for static blocks
(verified: `await` directly in a top-level static block takes the
`None`/top-level branch today). "Directly inside a static block" =
the fn_stack has not grown since the innermost static block was
entered.

### Step D1: the tracking stack [M]

`TraversalStacks` (`src/checker/mod.rs` ~310, `#[derive(Default)]` —
no construction-site edits needed): add

```rust
/// fn_stack depth at each active class-static-block entry; the last
/// entry equals the current fn_stack len exactly when the nearest
/// function-like container is a static block (tsc
/// getContainingFunctionOrClassStaticBlock landing on the block).
pub static_block_fn_depth: Vec<usize>,
```

Helper next to other small helpers in `mod.rs`:

```rust
fn directly_in_static_block(&self) -> bool {
    self.stacks
        .static_block_fn_depth
        .last()
        .is_some_and(|&base| self.stacks.fn_stack.len() == base)
}
```

`src/checker/classes.rs` static-block arm (~944-949) — wrap the
existing body exactly where `in_class_static_block` is maintained:

```rust
this.cflags.in_class_static_block += 1;
this.stacks.static_block_fn_depth.push(this.stacks.fn_stack.len());
this.check_statements(&b.stmts, bscope);
this.stacks.static_block_fn_depth.pop();
this.cflags.in_class_static_block -= 1;
```

### Step D2: await → 18037 [M]

`src/checker/exprs.rs` `Expr::Await` arm (~579): the static-block
branch comes FIRST and suppresses the async/top-level checks (tsc
sets `hasError` and skips them); operand checking and the 80007
suggestion continue unchanged:

```rust
Expr::Await { expr, span } => {
    if self.directly_in_static_block() {
        self.error_at(*span,
            &gen::await_expression_cannot_be_used_inside_a_class_static_block, &[]);
    } else {
        match self.stacks.fn_stack.last() {
            /* ...existing three arms, byte-identical... */
        }
    }
    /* ...existing operand + 80007 code unchanged... */
```

(`await using` has its own message, 18038-family — out of scope,
note it.)

### Step D3: return → 18041 [M]

`src/checker/stmts.rs` `check_return` (~3748): replace the
`invalid_return` computation and emission with

```rust
let in_static_block = self.directly_in_static_block();
let invalid_return = self.stacks.fn_stack.is_empty() || in_static_block;
if invalid_return && !self.parse_error_files.contains(&self.current_file) {
    self.error_at(
        span,
        if in_static_block {
            &gen::A_return_statement_cannot_be_used_inside_a_class_static_block
        } else {
            &gen::A_return_statement_can_only_be_used_within_a_function_body
        },
        &[],
    );
}
```

Everything downstream (`invalid_return_expr_depth`, `ret_ctx`) keeps
using `invalid_return` unchanged — a static-block `return` must NOT
check against the enclosing function's return type (tsc treats it as
foreign to the function).

### Step D4: yield → 1163 [M]

`src/checker/exprs.rs` yield arm (~641): the generator test becomes

```rust
let in_generator = !self.directly_in_static_block()
    && self.stacks.fn_stack.last().map(|f| f.is_generator).unwrap_or(false);
```

(the existing 1163 emission below it is unchanged; the delegate/async
code path below is untouched).

### Verify and commit

Standard build/test; probe all three fixtures — at every position
listed above the `*` must flip sides correctly (tsrs now emits
18037/18041/1163, no longer 1375/1378/1108 there); `./verify.sh mf`
(await/yield sit near flow-checked code); full gate. Expect
`0 NEW_FP / 0 NEW_FN`, adds ~13(18037)+~6(18041)+~2(1163), standing
FPs -5-ish, three fixtures flip. Recommended pin (strings from the
probe, per EXECUTION-GUIDE): one test with a top-level static block
asserting `TS18037` present and `TS1375` absent. Commit:
`static-block 1: await/return/yield grammar inside class static blocks`.

| Symptom | Diagnosis | Fix |
|---|---|---|
| 18037 fires inside a function DEFINED in a static block | fn_stack grew, so `directly_in_static_block` must be false — the helper compares len, check the push/pop placement | Push/pop must wrap ONLY `check_statements` |
| 1375/1378 still present at 3:9 | static-block branch not first, or `else` dropped | Re-apply D2 shape |
| Return-type errors appear on static-block returns | `invalid_return` no longer covers the static-block case downstream | Keep D3's exact computation |

---

## QW-E: `new.target` meta-property → 17013 (adds x34, 2 files)

Fixtures: `es6/newTarget/invalidNewTarget.es5.ts` / `.es6.ts`
(17 FN each, zero FPs — tsrs parses the construct cleanly today).

Root cause (verified): the parser folds `new.target` into the
catch-all `Expr::ImportMeta` and DISCARDS the name
(`src/parser/expr.rs` ~815: `let _name = self.parse_ident_name();`),
so the checker cannot see it; `ImportMeta` checks to `any`
(`src/checker/exprs.rs` ~635) with no grammar check.

tsc anchors: `checkNewTargetMetaProperty` (`_tsc.js` ~78086),
`getNewTargetContainer` (~14541): the container is the nearest
NON-ARROW this-container; only `Constructor`, `FunctionDeclaration`,
`FunctionExpression` are legal — methods, accessors, property
initializers, computed names, and top level all error 17013.

### Step E1: AST variant [M]

`src/ast.rs`, next to `ImportMeta` (~713):

```rust
/// `new.target` meta-property.
NewTarget {
    span: Span,
},
```

Add `| Expr::NewTarget { span }` to the big `span()` match arm chain
(~789, same group as `ImportMeta`). Then
`grep -n "ImportMeta" src/ src/checker -r` and mirror membership in
every list-style match that is about EXPRESSION SHAPE, not
import-specific behavior — at `2bf9ec0` that is exactly two more
sites: `src/parser/expr.rs` ~36 (LHS-expression predicate; matters
for the parse-gate recovery — `new.target` IS a valid LHS-expression
kind in tsc's `isLeftHandSideExpression`) and `src/checker/exprs.rs`
~128 (the not-a-narrowable-reference list). The checker's `=> any`
arm (~635) is import-SPECIFIC — do not add NewTarget there; E3 gives
it its own arm.

### Step E2: parser [M]

`src/parser/expr.rs` ~815, current:

```rust
if self.token() == Tok::Dot {
    self.next();
    let _name = self.parse_ident_name();
    return Expr::ImportMeta { span: Span::new(start, self.prev_end()) };
}
```

becomes:

```rust
if self.token() == Tok::Dot {
    self.next();
    let name = self.parse_ident_name();
    let span = Span::new(start, self.prev_end());
    if name.name == "target" {
        return Expr::NewTarget { span };
    }
    // other `new.<name>` forms keep the old catch-all (tsc 17012 is
    // a separate leaf — record, don't add here)
    return Expr::ImportMeta { span };
}
```

### Step E3: checker [M]

`src/checker/exprs.rs`, next to the `ImportMeta` arm (~635):

```rust
Expr::NewTarget { span } => {
    // tsc checkNewTargetMetaProperty / getNewTargetContainer: nearest
    // non-arrow container must be a constructor, function declaration,
    // or function expression
    let valid = self
        .stacks
        .fn_stack
        .iter()
        .rev()
        .find(|f| f.kind != FuncKind::Arrow)
        .is_some_and(|f| matches!(
            f.kind,
            FuncKind::Declaration | FuncKind::Expression | FuncKind::Constructor
        ));
    if !valid {
        self.error_at(*span,
            &gen::Meta_property_0_is_only_allowed_in_the_body_of_a_function_declaration_function_expression_or_constructor,
            &["new.target".to_string()]);
    }
    self.types.any
}
```

Returning `any` matches today's `ImportMeta` typing so the VALID
`new.target` fixtures stay byte-identical; the tsc-faithful type
(`getTypeOfSymbol` of the container) is follow-up fidelity, not this
commit. Known edge left as-is: tsc reports 17013 for `new.target`
directly inside a class static block (the block is a this-container);
with QW-D's stack in place this could be added later — record it.

### Verify and commit

Standard build/test; probe both fixtures — every previously-starred
17013 line must lose its star and NO new tsrs-only lines may appear
(especially none in `es6/newTarget/newTarget.es5.ts`/`.es6.ts`, the
valid-usage fixtures — probe those too); full gate. Expect
`0 NEW_FP / 0 NEW_FN`, +34 adds, two files flip. Commit:
`new-target 1: parse new.target as a meta-property and check 17013`.

| Symptom | Diagnosis | Fix |
|---|---|---|
| Valid fixtures gain 17013 | fn_stack walk classified a legal container as invalid (check FuncKind of the frame the walk lands on) | The walk must SKIP arrows and test only the first non-arrow frame |
| Parse errors appear | E2 changed recovery for `new.<other>` | Only `target` may divert; the fallback must stay byte-identical |
| 17013 missing at computed-name position (5:6) | Computed names check under a fn frame | Verify with the micro `class C { [new.target]() {} }` — expected container walk lands on no frame / a Method frame, both invalid |
