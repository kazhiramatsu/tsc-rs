# Step-by-step: parse-error gate (companion to parse-error-gate.md)

Read parse-error-gate.md for WHY; this file is the HOW, sized for a
low-context implementer. Follow docs/design/EXECUTION-GUIDE.md's loop.
Work on branch: `git checkout -b parse-gate`.

Difficulty labels: [M] mechanical, [P] probe-first (pin oracle behavior
before coding), [T] triage-heavy (expect gate iterations; a weaker
model should stop at the listed stop-points and leave notes).

---

## STAGE 0 — inert plumbing (zero behavior change) [M]

### Step 0.1 — collect per-statement parse-error marks

File: `src/lib.rs`, inside `check_program_core`, right after the parse
loop that fills `parsed` and `syntactic` (the loop is at ~line 647 and
pushes `(name.clone(), st, file_ast)`).

Add a helper at module level (bottom of lib.rs, near other free fns):

```rust
/// node_keys of the innermost STATEMENTS whose span contains a syntactic
/// diagnostic — the tsrs projection of tsc's ThisNodeOrAnySubNodesHasError.
fn collect_parse_error_stmts(
    parsed: &[(String, SourceText, ast::SourceFileAst)],
    syntactic: &[diagnostics::Diagnostic],
) -> std::collections::HashSet<usize> {
    use crate::ast::Stmt;
    // gather (node_key, span, file) for every statement, recursively
    fn walk(stmts: &[Stmt], file: usize, out: &mut Vec<(usize, crate::ast::Span, usize)>) {
        for s in stmts {
            out.push((crate::ast::node_key(s), s.span(), file));
            // recurse into bodies that contain statements
            match s {
                Stmt::Block { stmts, .. } => walk(stmts, file, out),
                Stmt::If { then_branch, else_branch, .. } => {
                    walk(std::slice::from_ref(then_branch), file, out);
                    if let Some(e) = else_branch { walk(std::slice::from_ref(e), file, out); }
                }
                // ... add the remaining compound-statement variants of the
                // actual Stmt enum (While/DoWhile/For/ForIn/ForOf/Switch
                // cases/Try blocks/Labeled/Namespace bodies/Function &
                // class bodies are NOT needed — statement granularity at
                // the top/function level is enough for stage 1).
                _ => {}
            }
        }
    }
    let mut all: Vec<(usize, crate::ast::Span, usize)> = Vec::new();
    for (i, (_, _, ast)) in parsed.iter().enumerate() {
        walk(&ast.statements, i, &mut all);
    }
    let mut marks = std::collections::HashSet::new();
    for d in syntactic {
        // pick the SMALLEST enclosing statement span in the same file
        let mut best: Option<(usize, u32)> = None; // (key, span_len)
        for (key, sp, f) in &all {
            if *f == d.file_index && sp.start as u32 <= d.start && d.start < sp.end as u32 {
                let len = sp.end as u32 - sp.start as u32;
                if best.map_or(true, |(_, l)| len < l) { best = Some((*key, len)); }
            }
        }
        if let Some((key, _)) = best { marks.insert(key); }
    }
    marks
}
```

VERIFIED anchors (checked @3242fdc — adjust only if drifted):
- `diagnostics::Diagnostic` fields: `file: Option<usize>` (index into
  the program file list; use `d.file == Some(i)`), `start: u32`,
  `length: u32` (src/diagnostics/mod.rs:107).
- `Stmt` variants (src/ast.rs:1090+): `Var(VarStmt)`,
  `Func(Box<FunctionLike>)`, `Class`, `Interface`, `TypeAlias`, `Enum`,
  `Namespace`, `With{..}`, `Return{..}`, `If{..}`, `While{..}`,
  `DoWhile{..}`, `For{..}`, `ForIn{..}`, `ForOf{..}`, `Block(Block)`,
  `Expr{..}`, `Empty{..}`, `Break{..}`, `Continue{..}`, `Throw{..}`,
  `Try{..}`, `Switch{..}`, `Labeled{..}`, `Import(..)`,
  `ExportNamed(..)` (+ a few export/import-equals variants — read the
  enum). For stage 0, recursing into `Block(b)` (`b.stmts`), `If`,
  `While/DoWhile/For*` bodies, `Try` blocks, `Switch` case stmt lists,
  `Labeled` and `Namespace` bodies is sufficient; the exact field names
  are one `Read` of the enum away.
- Statement-list root: check the actual field name on the parsed file
  AST (`ast::SourceFileAst`) — grep `statements` in src/ast.rs.

Call it right after the parse loop and keep it unused for now:

```rust
let parse_error_stmts = collect_parse_error_stmts(&parsed, &syntactic);
let _ = &parse_error_stmts; // consumed in stage 2
```

Verify: `cargo build --release` clean; `cargo test --release` green;
`./verify.sh golden-check` → `changed: 0 files` (byte-identical).
Commit: `parse-gate 0.1: collect per-statement parse-error marks (inert)`.

---

## STAGE 1 — non-LHS `=` recovery [P]

### Step 1.1 — pin the oracle behavior FIRST

Create these micro fixtures under `/tmp/scratch/` and run
`python3 scripts/probe.py` on each. Record the ORACLE side (codes +
line:col) in a comment block you keep for step 1.3's tests:

```
// f1.ts
// @target: es2015
declare var a: number; declare var b: number; declare var c: number;
a + b = c;

// f2.ts
// @target: es2015
declare var x: boolean;
!x = false;

// f3.ts
// @target: es2015
declare var p: Promise<number>;
async function f() { await p = 3; }

// f4.ts
// @target: es2015
declare var n: number;
n++ = 3;
```

Expected (from tsc source reading — VERIFY, this is the pin): the
oracle emits **TS1005 ';' expected** (or TS1128/TS1109 per context) at
the `=` token, and the assignment is NOT built (so no TS2364 and the
RHS parses as its own expression statement). If a probe shows a
DIFFERENT code/position, the oracle wins — update the expectations.

### Step 1.2 — the parser change

File: `src/parser/expr.rs`, `parse_assignment_expr` (~line 30). The
operator match starts `Tok::Eq => Some(BinOp::Assign)`.

Add above `parse_assignment_expr`:

```rust
/// tsc isLeftHandSideExpressionKind (_tsc.js:12210): the only expression
/// forms allowed to the left of an assignment operator.
fn is_left_hand_side_expr(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Ident(_)
            | Expr::NumLit { .. } | Expr::StrLit { .. } | Expr::BigIntLit { .. }
            | Expr::BoolLit { .. } | Expr::NullLit { .. } | Expr::RegexLit { .. }
            | Expr::Template { .. }
            | Expr::Array { .. } | Expr::Object { .. }
            | Expr::FunctionExpr(_) | Expr::ClassExpr(_)
            | Expr::Call { .. } | Expr::PropAccess { .. } | Expr::ElemAccess { .. }
            | Expr::Paren { .. } | Expr::NonNull { .. }
            | Expr::This { .. } | Expr::Super { .. }
    )
}
```

(Deliberately excluded: `Unary`, `Update`, `Binary`, `Cond`, `Await`,
`Yield`, `Arrow`, `Assertion`, `Spread`. NOTE `Assertion`: tsc treats
`<T>x` as a UnaryExpression — not LHS — but `x as T` parses as... probe
`(<any>y) = 3` and `y as any = 3` in step 1.1 and follow the oracle;
add/keep `Assertion` accordingly.)

In `parse_assignment_expr`, wrap the existing `if let Some(op) = op`:

```rust
if let Some(op) = op {
    if !is_left_hand_side_expr(&left) {
        // tsc parseAssignmentExpressionOrHigher (_tsc.js:31788) only
        // builds an assignment when the LHS is a LeftHandSideExpression;
        // otherwise the operator token is LEFT UNCONSUMED and statement-
        // level recovery reports 1005 at it.
        return left;
    }
    ...existing body unchanged...
}
```

### Step 1.3 — check what recovery emits and align

Build, then probe f1–f4 again with the TSRS side. The statement parser
will now hit the dangling `=`. Three possible outcomes per fixture:

- tsrs emits the SAME code+position as the oracle pin → done.
- tsrs emits a DIFFERENT code (e.g. 1128) at the same position → find
  the statement-recovery diagnostic site in `src/parser/stmt.rs`
  (grep `expected` / `1005`) and align the code choice with tsc's
  context rules (probe more variants if needed).
- tsrs loops or consumes the whole rest of the file → the statement
  loop failed to make progress on `=`. Fix: ensure the recovery path
  SKIPS the unconsumed operator token (one `self.next()` in the
  error branch of the expression-statement parser) — mirror how other
  unexpected tokens are skipped there.

Add unit tests pinning f1–f4's full diagnostic sets (follow the pattern
of existing parser tests in `src/lib.rs`'s `#[cfg(test)]` block; exact
strings). Tests must encode the ORACLE outputs from step 1.1.

Verify: build + tests green. Then run
`./verify.sh golden-check`. EXPECTED at this point: NEW_FN = 0,
NEW_FP = 0, but many `OK_RM`/`OK_ADD` lines in files from the 1005/2364
families (improvements). If NEW_FP appear ONLY in files that previously
relied on the mis-built Assign for semantic output, note them — they
should disappear after stage 2; keep them listed in the branch notes.
Commit: `parse-gate 1: non-LHS assignment recovery (tsc :31788)`.

STOP-POINT for weaker models: if the golden-check delta exceeds ~120
changed files or NEW_FP > ~15, stop and leave notes; a stronger model
should review before stage 2.

---

## STAGE 2 — semantic un-gating [T]

### Step 2.1 — flip the gate

File: `src/lib.rs:742`. Replace:

```rust
let diags: Vec<diagnostics::Diagnostic> = if !syntactic.is_empty() {
    syntactic
} else { ...semantic path... };
```

with: run the semantic path UNCONDITIONALLY (keep the options-diags
early-out INSIDE it exactly as today), then output
`syntactic ++ semantic` (keep the existing final sort — find where the
current single-source vector is sorted/deduped and apply the same to
the concatenation; if there is no sort today, sort by
`(file_index, start, code)` and dedup exact duplicates).

### Step 2.2 — panic sweep BEFORE looking at diagnostics

```
for ch in /tmp/chunk1.txt /tmp/chunk2.txt /tmp/chunk3.txt /tmp/chunk_tail.txt; do
  ./target/release/tsrs --check-batch $ch >/dev/null 2>>/tmp/panics.txt
done
grep -c panic /tmp/panics.txt
```

Every panic is a binder/checker hole on recovered ASTs. Fix each with
the SMALLEST local guard (e.g. a `let Some(..) else { return ... }`),
never by re-gating. Repeat until zero panics. Commit per fix-batch.

### Step 2.3 — statement-granularity skip

File: `src/checker/stmts.rs`, `check_statements` (line ~33). Thread
`parse_error_stmts` from stage 0 into the checker (simplest: a new
field on `Checker` set at construction in `checker::check`; find the
constructor call in `check_program_core`). In the loop:

```rust
for s in stmts {
    if self.parse_error_stmts.contains(&crate::ast::node_key(s)) {
        continue; // tsc containsParseError: no semantic checks inside
    }
    ...existing...
}
```

The BINDER still sees these statements (do NOT touch binder input) —
that is intentional and matches tsc.

Unused-locals interaction: symbols declared ONLY inside skipped
statements will now surface as "unused" (nothing marked them). Probe
first (fixture: a broken function body + an unused outer var) and pin
what tsc does; expected per tsc source: unused reporting still runs for
the file but tsc's own checks inside the errored node were skipped the
same way, so parity should hold WITHOUT extra work. If probes disagree,
add `parse_error_stmts` exemption to the unused main loop (grep
`unused_group_symbol_exempt` in checker/mod.rs for where exemptions
live).

### Step 2.4 — the big gate + cluster triage [T]

`./verify.sh golden-check`. This is the largest delta of the project.
Triage per EXECUTION-GUIDE (micro-fixture per NEW_FP file), and expect
these failure modes:

| Symptom | Likely cause | Fix |
|---|---|---|
| 2304 FPs on names declared in a skipped statement | skip granularity too coarse (decl swallowed) | mark the ERROR at finer granularity for declarations: if the diag lands inside a function BODY, mark the innermost statement INSIDE the body, not the whole declaration (extend the stage-0 walker into function/class bodies) |
| 2364/2322 FPs at recovered `=` sites | stage 1 recovery incomplete for a token context | add the context to step 1.3's alignment |
| cascade FPs inside a statement AFTER the errored one | tsc marks a WIDER region (its flag propagates to the containing list when the parser bailed) | mark BOTH the errored statement and its successor when the diag is `1005 ';' expected`-family AND the successor starts on the same line — pin with probes first |
| duplicate diagnostics (same code+span twice) | syntactic+semantic overlap | dedup exact (code,file,start,len) pairs at the concat site |

NEW_FN budget: 0. NEW_FP budget for landing: 0 (iterate; historical
workstreams needed ≤4 rounds).

### Step 2.5 — land

Squash-merge the branch to main as ONE commit (message template in
parse-error-gate.md), run `cargo fmt`, full gate on main, tests, mf,
`./verify.sh golden-save`, re-measure absolutes, update
`docs/design/README.md`'s status header and the conformance-sweep
memory file if you have access to it.
