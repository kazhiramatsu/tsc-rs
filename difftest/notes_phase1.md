
## Phase 1 parser-conformance session (continued)

False-positives (tsc parse-clean, tsrs parse-error): 1268 → 211 across 5907 conformance files.
No panics/hangs. Checker regression held: corpus1 30/30, corpus2 24/30.

Parser fixes this session (src/parser.rs unless noted):
- Ambient/string-named modules: `declare module "x" {…}`, bodyless `declare module "x";`,
  dotted `namespace A.B.C`, and `declare global {…}` (parse_namespace + both dispatch sites).
- `using x = …` and `await using x = …` declarations (statement dispatch → parse_var_stmt, VarKind::Const).
- import types in type position: `import("m").Foo<Args>` and `typeof import("m")` (parse_primary_type, →any).
- JSX names: hyphen/namespace/dot/`this`/reserved-keyword tag+attr names, closing tags, spread
  attributes already done; added spread CHILDREN `<div>{...x}</div>` and empty container `{}`.
- Function expressions: `async function(){}`, `async function*(){}` (parse_primary_expr KAsync arm).
- Decorator grammar (parse_decorator_expr): identifier/`(expr)`/`new X` base, `.name`/`(call)`/`!`
  postfix; element access `[...]` excluded (so `@dec ["x"]()` reads `["x"]` as member name).
- Decorated class expressions `(@dec class {})`, `export @dec default class`, `export default @dec class`.
- export specifiers: `export import a = require(…)|A.B`, `export * as default`, string-named
  `export {x as "s"}` / `export {"s" as x}`; import string-named source `import {"s" as x}`.
- import-equals entity-name refs: `import a = M.x` (parse_module_reference helper).
- import/export attributes: `with {…}` / `assert {…}` (parse_import_attributes helper).
- strict-reserved words as identifiers: `interface`/`implements`/`private`/`protected`/`public`/`yield`
  in binding/ref position (scanner is_strict_reserved_word; parse_ident; let-dispatch lookahead).
- `<const>` angle const assertion (parse_unary_expr) + `typeof this.member` (parse_entity_name accepts `this`).
- TS1206 (decorators not valid here) deferred to checker — removed parse-time emission (statement + export paths).
- **Shift bug**: `a << 2 >> 1` dropped because inner rescan_greater() widened `>` to GtGt without
  consuming; added Tok::GtGt/GtGtGt/GtGtEq/GtGtGtEq arms to binary_op_for_token.
- **new + tagged template**: ``new f`...` `` — parse_new_expr template branch had copy-pasted `[...]`
  body (parse_expression + expect CloseBracket); replaced with parse_template_expr + Call wrap.

Remaining FP long-tail (211): regex `\u{…}` w/ u-flag (1501, ~20, deferred), parse-deferred-to-checker
cases tsc tolerates (`var;` empty decl 1003; `#x` private in object literal 1005), and assorted one-offs.
MISSED rose 66→70 from intended parse-leniency (keyword-as-namespace-name, 1206 deferral, `@g<number>`)
— these are Phase-2 grammar-check territory.
