// H2.8a-A-RES-POST-T1: adjacent controls for the five residual children left
// after C02 / T1 (handoff `h2-8a-post-t1-residuals-claude-handoff.md` §4):
//
//   r9                   anonymous default class names against the global
//                        name oracle and the bundle's generated-name table
//   r12                  the System transform's hoisted class statement end
//                        source map (`A = class A ... };`)
//   receiver-map         the class-fields copiable-receiver temp of a private
//                        compound assignment / update
//   private-set-comments comment ownership at the right operand of a lowered
//                        private assignment
//   decorator-comments   comment suppression on transformed decorator
//                        expressions (`transformDecorator` NoComments)
//
// IDs are fixed here before any observation; expected outputs are observed by
// `scripts/observe-post-t1-residuals.mjs`, never written by hand. The frozen
// target rows of `decorator-binding-inputs.json` / `bundle-metadata-t1-inputs.json`
// are not reused or rewritten: every row here is a new ID and a new artifact.
//
// usage: node scripts/generate-post-t1-residuals-inputs.mjs
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/post-t1-residuals-inputs.json");

const common = { strict: true, allowJs: false, checkJs: false, declaration: true, declarationMap: true,
  sourceMap: true, skipDefaultLibCheck: true, noErrorTruncation: true, newLine: 0, removeComments: false };
const targets = { es2015: 2, es2022: 9, esnext: 99 };
const MODULE = { commonjs: 1, amd: 2, umd: 3, system: 4, esnext: 99 };
// The frozen pipeline tuple (`decorator-binding-inputs.json`): standalone
// ESNext modules under outDir, or a System outFile bundle.
const outDir = { outDir: "/project/out" };
const bundle = module => ({ module, outFile: "/project/out/bundle.js", ignoreDeprecations: "6.0" });

const cases = [];
const file = (name, text) => ({ path: `/project/${name}`, text });
const add = (family, target, variant, files, options, roots = null) => {
  const case_id = `post-t1-residuals/${family}/${target}/${variant}`;
  assert.ok(!cases.some(c => c.case_id === case_id), case_id);
  const merged = { ...common, target: targets[target], ...options };
  for (const key of Object.keys(merged)) if (merged[key] === undefined) delete merged[key];
  cases.push({ case_id, family, variant, roots: roots ?? files.map(f => f.path), files, options: merged });
};

// ------------------------------------------------------------------- r9
// `transformTypeScript.visitClassDeclaration` names an anonymous class that
// needs a declaration name (member decorators or static initialized
// properties) with `getGeneratedNameForNode(node)`; the printer resolves it
// through `generateNameForExportDefault` -> `makeUniqueName("default")`
// against `hasGlobalName`, the source's parsed identifiers and the
// bundle-wide `generatedNames` (_tsc.js:94434-94460, 120638-120667,
// 120741-120786, 120831-120845). Module transforms reference the same
// generated identifier (`getLocalName` / `getDeclarationName`).
const DEC = `function dec(value: any, context: any): any { return value; }\n`;
const DEFAULT_DECORATED = `${DEC}export default @dec class { @dec static m() {} }\nexport const tail = 1;\n`;
const DEFAULT_NAMED = `${DEC}export default @dec class Named { @dec static m() {} }\nexport const tail = 1;\n`;
const DEFAULT_STATIC_FIELD = `export default class { static f = 1; }\nexport const tail = 1;\n`;
const DEFAULT_PLAIN = `export default class { m() {} }\nexport const tail = 1;\n`;
const DEFAULT_FUNCTION = `export default function () { return 1; }\nexport const tail = 1;\n`;
const SAME_FILE_CENSUS = `const default_1 = 0;\n${DEFAULT_DECORATED}`;
const GLOBAL_ONE = `let default_1 = 0;\n`;
const GLOBAL_TWO = `let default_1 = 0;\nlet default_2 = 0;\n`;
const esnextDefine = { module: MODULE.esnext, useDefineForClassFields: true, ...outDir };
const systemDefine = { useDefineForClassFields: true, ...bundle(MODULE.system) };
add("r9", "esnext", "define-module-global-let-collision", [file("main.ts", DEFAULT_DECORATED), file("globals.ts", GLOBAL_ONE)], esnextDefine);
add("r9", "esnext", "define-module-no-collision", [file("main.ts", DEFAULT_DECORATED)], esnextDefine);
add("r9", "esnext", "define-module-two-globals", [file("main.ts", DEFAULT_DECORATED), file("globals.ts", GLOBAL_TWO)], esnextDefine);
add("r9", "esnext", "define-module-same-file-census", [file("main.ts", SAME_FILE_CENSUS), file("globals.ts", GLOBAL_ONE)], esnextDefine);
add("r9", "esnext", "define-module-named-default", [file("main.ts", DEFAULT_NAMED), file("globals.ts", GLOBAL_ONE)], esnextDefine);
add("r9", "esnext", "define-module-static-field-no-decorator", [file("main.ts", DEFAULT_STATIC_FIELD), file("globals.ts", GLOBAL_ONE)], esnextDefine);
add("r9", "esnext", "define-module-no-name-needed", [file("main.ts", DEFAULT_PLAIN), file("globals.ts", GLOBAL_ONE)], esnextDefine);
add("r9", "esnext", "define-system-two-files", [file("main.ts", DEFAULT_DECORATED), file("second.ts", DEFAULT_DECORATED)], systemDefine);
add("r9", "esnext", "define-system-two-files-reversed", [file("main.ts", DEFAULT_DECORATED), file("second.ts", DEFAULT_DECORATED)], systemDefine, ["/project/second.ts", "/project/main.ts"]);
add("r9", "esnext", "define-system-global-collision", [file("main.ts", DEFAULT_DECORATED), file("globals.ts", GLOBAL_ONE), file("second.ts", DEFAULT_DECORATED)], systemDefine);
add("r9", "esnext", "define-system-static-field-two-files", [file("main.ts", DEFAULT_STATIC_FIELD), file("second.ts", DEFAULT_STATIC_FIELD)], systemDefine);
add("r9", "esnext", "define-system-function-default-collision", [file("main.ts", DEFAULT_FUNCTION), file("globals.ts", GLOBAL_ONE)], systemDefine);
add("r9", "esnext", "define-commonjs-global-let-collision", [file("main.ts", DEFAULT_DECORATED), file("globals.ts", GLOBAL_ONE)], { module: MODULE.commonjs, useDefineForClassFields: true, ...outDir });
add("r9", "esnext", "define-commonjs-function-default-collision", [file("main.ts", DEFAULT_FUNCTION), file("globals.ts", GLOBAL_ONE)], { module: MODULE.commonjs, useDefineForClassFields: true, ...outDir });
add("r9", "esnext", "define-amd-global-let-collision", [file("main.ts", DEFAULT_DECORATED), file("globals.ts", GLOBAL_ONE)], { module: MODULE.amd, useDefineForClassFields: true, ...outDir });
add("r9", "esnext", "define-umd-global-let-collision", [file("main.ts", DEFAULT_DECORATED), file("globals.ts", GLOBAL_ONE)], { module: MODULE.umd, useDefineForClassFields: true, ...outDir });
// Lowered decorator routes (the class name comes through transformESDecorators).
add("r9", "esnext", "set-module-global-let-collision", [file("main.ts", DEFAULT_DECORATED), file("globals.ts", GLOBAL_ONE)], { module: MODULE.esnext, useDefineForClassFields: false, ...outDir });
add("r9", "es2022", "set-module-global-let-collision", [file("main.ts", DEFAULT_DECORATED), file("globals.ts", GLOBAL_ONE)], { module: MODULE.esnext, useDefineForClassFields: false, ...outDir });
add("r9", "es2015", "set-module-global-let-collision", [file("main.ts", DEFAULT_DECORATED), file("globals.ts", GLOBAL_ONE)], { module: MODULE.esnext, useDefineForClassFields: false, ...outDir });
add("r9", "es2022", "set-system-two-files", [file("main.ts", DEFAULT_DECORATED), file("second.ts", DEFAULT_DECORATED)], { useDefineForClassFields: false, ...bundle(MODULE.system) });
add("r9", "es2015", "set-commonjs-global-let-collision", [file("main.ts", DEFAULT_DECORATED), file("globals.ts", GLOBAL_ONE)], { module: MODULE.commonjs, useDefineForClassFields: false, ...outDir });

// ------------------------------------------------------------------ r12
// `transformSystemModule.visitClassDeclaration` (_tsc.js:112605-112633)
// hoists a class declaration into `setTextRange(createExpressionStatement(
// createAssignment(name, setTextRange(createClassExpression(...), node))), node)`:
// two fresh nodes ranged to the declaration and never linked to it, so
// `emitSourceMapsAfterNode` maps the class end twice on the `};` line
// regardless of the flags the class declaration carries (a static initialized
// property adds NoTrailingSourceMap to the declaration, _tsc.js:94464-94467).
const PLAIN = `class A extends Object {\n    static f = 1;\n}\nexport const tailA = A.name;\n`;
const EXPORTED_PLAIN = PLAIN.replace("class A", "export class A");
const STATIC_GET = `class B extends Object {\n    static #p = 1;\n    static readP() { return B.#p; }\n}\nexport const tailB = B.readP();\n`;
const NO_STATIC = `export class A extends Object {\n    m() { return 1; }\n}\nexport const tailA = A.name;\n`;
const TRAILING_COMMENT = `export class A extends Object {\n    static f = 1;\n} // end of A\n\n/* between */\nexport const tailA = A.name;\n`;
const NON_BMP = `const s = "😀😀";\nexport class A extends Object {\n    static f = 1;\n}\nexport const tailA = A.name + s;\n`;
const DEFAULT_STATIC = `export default class {\n    static f = 1;\n}\nexport const tail = 1;\n`;
const CLASS_EXPRESSION = `export const A = class {\n    static f = 1;\n};\nexport const tailA = A.name;\n`;
const NESTED_CLASS = `export function make() {\n    class A extends Object {\n        static f = 1;\n    }\n    return A.name;\n}\nexport const tailA = make();\n`;
const DECORATED_STATIC_INIT = `${DEC}const keys = { get x(): "x" { return "x"; } };\n@dec\nexport class K {\n    @dec static [keys.x] = 1;\n}\nexport const tail = K.name;\n`;
const DECORATED_NO_STATIC = `${DEC}@dec\nexport class K {\n    @dec m() { return 1; }\n}\nexport const tail = K.name;\n`;
const es2015Set = { useDefineForClassFields: false };
add("r12", "es2015", "system-exported-static-field", [file("main.ts", EXPORTED_PLAIN), file("second.ts", STATIC_GET)], { ...es2015Set, ...bundle(MODULE.system) });
add("r12", "es2015", "system-unexported-static-field", [file("main.ts", PLAIN), file("second.ts", STATIC_GET)], { ...es2015Set, ...bundle(MODULE.system) });
add("r12", "es2015", "system-no-declaration", [file("main.ts", EXPORTED_PLAIN), file("second.ts", STATIC_GET)], { ...es2015Set, ...bundle(MODULE.system), declaration: false, declarationMap: false });
add("r12", "es2015", "system-no-static", [file("main.ts", NO_STATIC), file("second.ts", STATIC_GET)], { ...es2015Set, ...bundle(MODULE.system) });
add("r12", "es2015", "system-trailing-comment", [file("main.ts", TRAILING_COMMENT), file("second.ts", STATIC_GET)], { ...es2015Set, ...bundle(MODULE.system) });
add("r12", "es2015", "system-non-bmp-before-class", [file("main.ts", NON_BMP), file("second.ts", STATIC_GET)], { ...es2015Set, ...bundle(MODULE.system) });
add("r12", "es2015", "system-default-anonymous-static-field", [file("main.ts", DEFAULT_STATIC), file("second.ts", STATIC_GET)], { ...es2015Set, ...bundle(MODULE.system) });
add("r12", "es2015", "system-class-expression-variable", [file("main.ts", CLASS_EXPRESSION), file("second.ts", STATIC_GET)], { ...es2015Set, ...bundle(MODULE.system) });
add("r12", "es2015", "system-nested-class", [file("main.ts", NESTED_CLASS), file("second.ts", STATIC_GET)], { ...es2015Set, ...bundle(MODULE.system) });
add("r12", "es2015", "system-static-private-both-files", [file("main.ts", STATIC_GET.replace(/\bB\b/g, "A")), file("second.ts", STATIC_GET)], { ...es2015Set, ...bundle(MODULE.system) });
add("r12", "es2022", "system-exported-static-field", [file("main.ts", EXPORTED_PLAIN), file("second.ts", STATIC_GET)], { ...es2015Set, ...bundle(MODULE.system) });
add("r12", "esnext", "define-system-decorated-static-init", [file("main.ts", DECORATED_STATIC_INIT), file("second.ts", DECORATED_STATIC_INIT)], { useDefineForClassFields: true, ...bundle(MODULE.system) });
add("r12", "esnext", "define-system-decorated-no-static", [file("main.ts", DECORATED_NO_STATIC), file("second.ts", DECORATED_NO_STATIC)], { useDefineForClassFields: true, ...bundle(MODULE.system) });
add("r12", "esnext", "define-system-static-field-no-decorator", [file("main.ts", EXPORTED_PLAIN), file("second.ts", PLAIN)], { useDefineForClassFields: true, ...bundle(MODULE.system) });
// Module formats that do not hoist the class (protection).
add("r12", "es2015", "amd-exported-static-field", [file("main.ts", EXPORTED_PLAIN), file("second.ts", STATIC_GET)], { ...es2015Set, ...bundle(MODULE.amd) });
add("r12", "es2015", "umd-exported-static-field", [file("main.ts", EXPORTED_PLAIN)], { ...es2015Set, module: MODULE.umd, ...outDir });
add("r12", "es2015", "commonjs-exported-static-field", [file("main.ts", EXPORTED_PLAIN)], { ...es2015Set, module: MODULE.commonjs, ...outDir });
add("r12", "es2015", "esnext-module-exported-static-field", [file("main.ts", EXPORTED_PLAIN)], { ...es2015Set, module: MODULE.esnext, ...outDir });

// ---------------------------------------------------------- receiver-map
// `createPrivateIdentifierAssignment` (_tsc.js:96795-96840): a compound
// assignment routes the visited receiver through `createCopiableReceiverExpr`
// (96567-96578): `cloneNode(receiver)` is synthesized (no positions), and an
// identifier receiver is not `isSimpleInlineableExpression`, so the clone is
// stored in a hoisted temp (`_b = <clone>`) that reads back as `_b`. Neither
// the temp nor the clone maps to the source receiver.
const RECEIVER_MAIN = `class A extends Object {\n    static f = 1;\n}\nexport const tailA = A.name;\n`;
const rcv = body => `class B extends Object {\n    static #p = 1;\n    #q = 1;\n${body}}\nexport const tailB = B.name;\n`;
const COMPOUND = rcv(`    static bump() { B.#p += 1; }\n`);
const COMPOUND_USED = rcv(`    static bump() { return B.#p += 1; }\n`);
const COMPOUND_STRING = rcv(`    static bump() { B.#p *= 2; }\n`);
const POSTFIX = rcv(`    static bump() { B.#p++; }\n`);
const POSTFIX_USED = rcv(`    static bump() { return B.#p++; }\n`);
const PREFIX = rcv(`    static bump() { ++B.#p; }\n`);
const PREFIX_USED = rcv(`    static bump() { return ++B.#p; }\n`);
const READ = rcv(`    static read() { return B.#p; }\n`);
const SIMPLE_SET = rcv(`    static set(v: number) { B.#p = v; }\n`);
const INSTANCE_THIS = rcv(`    bump() { this.#q += 1; }\n`);
const INSTANCE_THIS_USED = rcv(`    bump() { return this.#q += 1; }\n`);
const INSTANCE_SIDE_EFFECT = rcv(`    static make(): B { return new B(); }\n    static bump() { B.make().#q += 1; }\n`);
const STATIC_SIDE_EFFECT = rcv(`    static pick(): typeof B { return B; }\n    static bump() { B.pick().#p += 1; }\n`);
const PAREN_RECEIVER = rcv(`    static bump() { (B).#p += 1; }\n`);
const NESTED = `export function make() {\n    class B extends Object {\n        static #p = 1;\n        static bump() { B.#p += 1; }\n    }\n    return B.name;\n}\nexport const tailB = make();\n`;
const amdBundle = { ...es2015Set, ...bundle(MODULE.amd) };
add("receiver-map", "es2015", "static-compound-second-file", [file("main.ts", RECEIVER_MAIN), file("second.ts", COMPOUND)], amdBundle);
add("receiver-map", "es2015", "static-compound-first-file", [file("main.ts", COMPOUND), file("second.ts", RECEIVER_MAIN)], amdBundle);
add("receiver-map", "es2015", "static-compound-outdir", [file("main.ts", COMPOUND)], { ...es2015Set, module: MODULE.commonjs, ...outDir });
add("receiver-map", "es2015", "static-compound-value-used", [file("main.ts", RECEIVER_MAIN), file("second.ts", COMPOUND_USED)], amdBundle);
add("receiver-map", "es2015", "static-compound-multiply", [file("main.ts", RECEIVER_MAIN), file("second.ts", COMPOUND_STRING)], amdBundle);
add("receiver-map", "es2015", "static-postfix-discarded", [file("main.ts", RECEIVER_MAIN), file("second.ts", POSTFIX)], amdBundle);
add("receiver-map", "es2015", "static-postfix-used", [file("main.ts", RECEIVER_MAIN), file("second.ts", POSTFIX_USED)], amdBundle);
add("receiver-map", "es2015", "static-prefix-discarded", [file("main.ts", RECEIVER_MAIN), file("second.ts", PREFIX)], amdBundle);
add("receiver-map", "es2015", "static-prefix-used", [file("main.ts", RECEIVER_MAIN), file("second.ts", PREFIX_USED)], amdBundle);
add("receiver-map", "es2015", "static-read", [file("main.ts", RECEIVER_MAIN), file("second.ts", READ)], amdBundle);
add("receiver-map", "es2015", "static-simple-set", [file("main.ts", RECEIVER_MAIN), file("second.ts", SIMPLE_SET)], amdBundle);
add("receiver-map", "es2015", "instance-this-compound", [file("main.ts", RECEIVER_MAIN), file("second.ts", INSTANCE_THIS)], amdBundle);
add("receiver-map", "es2015", "instance-this-compound-used", [file("main.ts", RECEIVER_MAIN), file("second.ts", INSTANCE_THIS_USED)], amdBundle);
add("receiver-map", "es2015", "instance-side-effect-compound", [file("main.ts", RECEIVER_MAIN), file("second.ts", INSTANCE_SIDE_EFFECT)], amdBundle);
add("receiver-map", "es2015", "static-side-effect-compound", [file("main.ts", RECEIVER_MAIN), file("second.ts", STATIC_SIDE_EFFECT)], amdBundle);
add("receiver-map", "es2015", "static-paren-receiver-compound", [file("main.ts", RECEIVER_MAIN), file("second.ts", PAREN_RECEIVER)], amdBundle);
add("receiver-map", "es2015", "static-nested-class-compound", [file("main.ts", RECEIVER_MAIN), file("second.ts", NESTED)], amdBundle);
add("receiver-map", "es2022", "static-compound-second-file", [file("main.ts", RECEIVER_MAIN), file("second.ts", COMPOUND)], { ...es2015Set, ...bundle(MODULE.amd) });

// --------------------------------------------------- private-set-comments
// `createPrivateIdentifierAssignment` passes the visited right operand into
// the `__classPrivateFieldSet` helper call; the call is ranged and
// original-linked to the source assignment (`visitBinaryExpression`), so the
// upstream printer's `containerEnd` and `emitNodeListItems`
// (`previousSibling.end !== parentNode.end`) decide which node owns the
// comments after `v`. The Rust producer adds NoTrailingComments to `v`.
const psc = body => `class B extends Object {\n    static #p = 1;\n    #q = 1;\n${body}}\nB.writeP(2);\nexport const tailB = B.name;\n`;
const PSC_NONE = psc(`    static writeP(v: number) { B.#p = v; }\n`);
const PSC_TRAILING = psc(`    static writeP(v: number) { B.#p = v /* t */; }\n`);
const PSC_LEADING = psc(`    static writeP(v: number) { B.#p = /* l */ v; }\n`);
const PSC_EOL = psc(`    static writeP(v: number) {\n        B.#p = v // eol\n        ;\n    }\n`);
const PSC_PAREN = psc(`    static writeP(v: number) { B.#p = (v /* in */); }\n`);
const PSC_COMMA = psc(`    static writeP(v: number) { (B.#p = v /* t1 */, B.#p = v /* t2 */); }\n`);
const PSC_ARGUMENT = psc(`    static id(x: number, y: number) { return x + y; }\n    static writeP(v: number) { B.id(B.#p = v /* arg */, 1); }\n`);
const PSC_INSTANCE = psc(`    static writeP(v: number) { }\n    writeQ(v: number) { this.#q = v /* t */; }\n`);
const PSC_COMPOUND = psc(`    static writeP(v: number) { B.#p += v /* t */; }\n`);
const PSC_MULTILINE = psc(`    static writeP(v: number) {\n        B.#p =\n            v /* t */\n            ;\n    }\n`);
const PSC_LEADING_LINE = psc(`    static writeP(v: number) {\n        B.#p = // c\n            v;\n    }\n`);
const PSC_USED = psc(`    static writeP(v: number) { const r = (B.#p = v /* t */); return r; }\n`);
const PSC_BOTH = psc(`    static writeP(v: number) { B.#p = /* l */ v /* t */; }\n`);
const PSC_TRAILING_LINE_END = psc(`    static writeP(v: number) { B.#p = v; /* after */ }\n`);
add("private-set-comments", "es2015", "no-comment", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_NONE)], amdBundle);
add("private-set-comments", "es2015", "trailing-block", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_TRAILING)], amdBundle);
add("private-set-comments", "es2015", "leading-block", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_LEADING)], amdBundle);
add("private-set-comments", "es2015", "leading-and-trailing", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_BOTH)], amdBundle);
add("private-set-comments", "es2015", "end-of-line", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_EOL)], amdBundle);
add("private-set-comments", "es2015", "parenthesized", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_PAREN)], amdBundle);
add("private-set-comments", "es2015", "comma-sequence", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_COMMA)], amdBundle);
add("private-set-comments", "es2015", "argument-boundary", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_ARGUMENT)], amdBundle);
add("private-set-comments", "es2015", "instance-this-trailing", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_INSTANCE)], amdBundle);
add("private-set-comments", "es2015", "compound-trailing", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_COMPOUND)], amdBundle);
add("private-set-comments", "es2015", "multiline-trailing", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_MULTILINE)], amdBundle);
add("private-set-comments", "es2015", "leading-line-comment", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_LEADING_LINE)], amdBundle);
add("private-set-comments", "es2015", "value-used-trailing", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_USED)], amdBundle);
add("private-set-comments", "es2015", "after-statement", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_TRAILING_LINE_END)], amdBundle);
add("private-set-comments", "es2015", "trailing-block-remove-comments", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_TRAILING)], { ...amdBundle, removeComments: true });
add("private-set-comments", "es2015", "trailing-block-outdir", [file("main.ts", PSC_TRAILING)], { ...es2015Set, module: MODULE.commonjs, ...outDir });
add("private-set-comments", "es2022", "trailing-block", [file("main.ts", RECEIVER_MAIN), file("second.ts", PSC_TRAILING)], amdBundle);

// ------------------------------------------------------ decorator-comments
// `transformDecorator` (_tsc.js:100554-100568): the visited decorator
// expression receives NoComments before it is bound (`createCallBinding`
// for access expressions) and stored in the `_classDecorators` /
// `_<member>_decorators` arrays.
const DEC2 = `${DEC}function dec2(value: any, context: any): any { return value; }\nconst ns = { dec };\nfunction key(): "x" { return "x"; }\n`;
const dc = (decorated) => `${DEC2}${decorated}export const tail = B.name;\n`;
const DC_BASE = dc(`@dec\nclass B extends Object {\n    static #p = 1;\n    static readP() { return B.#p; }\n}\n`);
const DC_CLASS_BLOCK = dc(`@/* a */ dec /* b */\nclass B extends Object {\n    static #p = 1;\n}\n`);
const DC_CLASS_EOL = dc(`@dec // eol\nclass B extends Object {\n    static #p = 1;\n}\n`);
const DC_MEMBER_BLOCK = dc(`class B extends Object {\n    @/* a */ dec /* b */ m() { return 1; }\n    @/* c */ dec /* d */ static f = 1;\n}\n`);
const DC_ACCESS = dc(`@/* a */ ns./* b */dec /* c */\nclass B extends Object {\n    @ns.dec /* d */ m() { return 1; }\n}\n`);
const DC_CALL = dc(`@dec(/* arg */ 1 as any) /* after */\nclass B extends Object {\n    @dec /* x */ (1 as any) m() { return 1; }\n}\n`);
const DC_PAREN = dc(`@(/* p */ dec /* q */)\nclass B extends Object {\n    @(/* r */ ns.dec /* s */) m() { return 1; }\n}\n`);
const DC_MULTIPLE = dc(`@dec /* a */ @dec2 /* b */\nclass B extends Object {\n    @dec /* c */ @dec2 /* d */ m() { return 1; }\n}\n`);
const DC_COMPUTED = dc(`class B extends Object {\n    @dec /* a */ [/* k */ key() /* l */] = 1;\n    @dec /* b */ static [/* m */ key()]() { return 1; }\n}\n`);
const DC_LINE_BETWEEN = dc(`@dec\n// between\nclass B extends Object {\n    @dec\n    // between member\n    m() { return 1; }\n}\n`);
const es2022Set = { useDefineForClassFields: false, ...bundle(MODULE.amd) };
const esnextSet = { useDefineForClassFields: false, ...bundle(MODULE.amd) };
const esnextDefineBundle = { useDefineForClassFields: true, ...bundle(MODULE.amd) };
for (const [target, options] of [["es2022", es2022Set], ["esnext", esnextSet]]) {
  add("decorator-comments", target, "set-static-get-second-file", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_BASE)], options);
  add("decorator-comments", target, "set-class-block", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_CLASS_BLOCK)], options);
  add("decorator-comments", target, "set-class-eol", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_CLASS_EOL)], options);
  add("decorator-comments", target, "set-member-block", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_MEMBER_BLOCK)], options);
  add("decorator-comments", target, "set-property-access", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_ACCESS)], options);
  add("decorator-comments", target, "set-call-arguments", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_CALL)], options);
  add("decorator-comments", target, "set-parenthesized", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_PAREN)], options);
  add("decorator-comments", target, "set-multiple", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_MULTIPLE)], options);
  add("decorator-comments", target, "set-computed-name", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_COMPUTED)], options);
  add("decorator-comments", target, "set-line-between", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_LINE_BETWEEN)], options);
}
add("decorator-comments", "es2022", "set-class-block-remove-comments", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_CLASS_BLOCK)], { ...es2022Set, removeComments: true });
add("decorator-comments", "es2022", "set-class-block-outdir", [file("main.ts", DC_CLASS_BLOCK)], { useDefineForClassFields: false, module: MODULE.commonjs, ...outDir });
add("decorator-comments", "es2015", "set-class-block", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_CLASS_BLOCK)], { useDefineForClassFields: false, ...bundle(MODULE.amd) });
add("decorator-comments", "es2015", "set-property-access", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_ACCESS)], { useDefineForClassFields: false, ...bundle(MODULE.amd) });
// Native decorators (ESNext x define): no transform, comments print as parsed.
add("decorator-comments", "esnext", "define-class-block", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_CLASS_BLOCK)], esnextDefineBundle);
add("decorator-comments", "esnext", "define-property-access", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_ACCESS)], esnextDefineBundle);
add("decorator-comments", "esnext", "define-member-block", [file("main.ts", RECEIVER_MAIN), file("second.ts", DC_MEMBER_BLOCK)], esnextDefineBundle);

const manifest = { version: 1,
  description: "H2.8a-A-RES-POST-T1 adjacent controls: anonymous default class names (r9), System hoisted class end maps (r12), private compound-assignment receiver temps (receiver-map), private assignment right-operand comments (private-set-comments), decorator expression comment suppression (decorator-comments).",
  cases };
assert.equal(new Set(cases.map(c => c.case_id)).size, cases.length);
const families = {};
for (const c of cases) families[c.family] = (families[c.family] ?? 0) + 1;
fs.writeFileSync(destination, JSON.stringify(manifest, null, 1) + "\n");
console.log(JSON.stringify({ destination: path.relative(root, destination), cases: cases.length, families }));
