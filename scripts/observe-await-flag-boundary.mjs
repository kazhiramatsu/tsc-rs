// Observe the parser's top-level await reparse boundary independently of emit.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const shapes = [
  [
    "original-1",
    "interface I extends await<string> {}"
  ],
  [
    "original-2",
    "interface await {}"
  ],
  [
    "original-3",
    "interface I { [await]: any; }"
  ],
  [
    "original-4",
    "type T = await<string>;"
  ],
  [
    "original-5",
    "type await = number;"
  ],
  [
    "original-6",
    "let a: await<string>;"
  ],
  [
    "original-7",
    "function f<T extends await>() {}"
  ],
  [
    "original-8",
    "const f = <T extends await>() => 1;"
  ],
  [
    "original-9",
    "class C<T extends await> {}"
  ],
  [
    "original-10",
    "class C { p: await<string>; }"
  ],
  [
    "original-11",
    "class C { await() {} }"
  ],
  [
    "original-12",
    "const x = { await: 1 };"
  ],
  [
    "original-13",
    "const x = { [await]: 1 };"
  ],
  [
    "original-14",
    "const x = source.await;"
  ],
  [
    "original-15",
    "const x = source[await];"
  ],
  [
    "original-16",
    "class C extends await<string> {}"
  ],
  [
    "original-17",
    "class C implements await<string> {}"
  ],
  [
    "computed-method",
    "class C { [await]() {} }"
  ],
  [
    "method-parameter",
    "class C { m(a = await) {} }"
  ],
  [
    "field-initializer",
    "class C { p = await; }"
  ],
  [
    "static-body",
    "class C { static { await; } }"
  ],
  [
    "class-decorator",
    "@await class C {}"
  ],
  [
    "binding-computed-name",
    "const { [await]: a } = o;"
  ],
  [
    "shorthand-default",
    "({ a = await } = o);"
  ],
  [
    "label",
    "await: ;"
  ],
  [
    "arrow-parameter",
    "const f = (a = await) => 1;"
  ],
  [
    "constructor-parameter",
    "class C { constructor(a = await) {} }"
  ],
  [
    "shorthand-name",
    "const x = { await };"
  ],
  [
    "binding-property-name",
    "const { await: a } = o;"
  ],
  [
    "function-parameter",
    "function f(a = await) {}"
  ],
  [
    "ambient-variable",
    "declare let x: typeof await;"
  ],
  [
    "ambient-function",
    "declare function f(a?: await): void;"
  ],
  [
    "overload",
    "function f(a = await): void; function f(a) {}"
  ],
  [
    "this-parameter",
    "function f(this: await) {}"
  ],
  [
    "missing-declaration",
    "@await;"
  ],
  [
    "static-modifier",
    "class C { @await static {} }"
  ],
  [
    "import-equals",
    "import x = await.y;"
  ],
  [
    "namespace-export",
    "export as namespace await;"
  ],
  [
    "namespace-body",
    "namespace N { await; }"
  ],
  [
    "function-expression",
    "const f = function(a = await) {};"
  ],
  [
    "function-name",
    "function await() {}"
  ],
  [
    "class-name",
    "class await {}"
  ],
  [
    "class-expression-name",
    "const C = class await {};"
  ],
  [
    "getter-name",
    "class C { get await() { return 1; } }"
  ],
  [
    "setter-name",
    "class C { set await(a) {} }"
  ],
  [
    "getter-computed",
    "class C { get [await]() { return 1; } }"
  ],
  [
    "setter-computed",
    "class C { set [await](a) {} }"
  ],
  [
    "getter-parameter",
    "class C { get x(a = await) {} }"
  ],
  [
    "setter-parameter",
    "class C { set x(a = await) {} }"
  ],
  [
    "method-body",
    "class C { m() { await; } }"
  ],
  [
    "method-decorator",
    "class C { @await m() {} }"
  ],
  [
    "property-decorator",
    "class C { @await p: number; }"
  ],
  [
    "parameter-decorator",
    "class C { m(@await a) {} }"
  ],
  [
    "ambient-class",
    "declare class C { [await](): void; }"
  ],
  [
    "method-signature",
    "class C { [await](): void; }"
  ],
  [
    "interface-signatures",
    "interface I { [await]: any; m(a: await): await; }"
  ],
  [
    "variable-name",
    "let await = 1;"
  ],
  [
    "binding-name",
    "const { a: await } = o;"
  ],
  [
    "array-binding-name",
    "const [await] = o;"
  ],
  [
    "binding-default",
    "const { a = await } = o;"
  ],
  [
    "type-assertion",
    "const x = <await>o;"
  ],
  [
    "as-type",
    "const x = o as await;"
  ],
  [
    "satisfies-type",
    "const x = o satisfies await;"
  ],
  [
    "generic-call",
    "f<await>();"
  ],
  [
    "generic-new",
    "new C<await>();"
  ],
  [
    "enum-member",
    "enum E { A = await }"
  ],
  [
    "export-assignment",
    "export = await;"
  ],
  [
    "export-default",
    "export default await;"
  ],
  [
    "import-specifier",
    "import {await} from \"m\";"
  ],
  [
    "export-specifier",
    "export {await};"
  ],
  [
    "property-post-modifier",
    "const o = { @await a: 1 };"
  ],
  [
    "shorthand-post-modifier",
    "const o = { @await a };"
  ],
  [
    "heritage-trailing-comma",
    "class C extends await<A>, {}"
  ],
  [
    "unicode-property",
    "const o = { aw\\u0061it: 1 };"
  ],
  [
    "private-name",
    "class C { #await = 1; }"
  ],
  [
    "break-label",
    "while (o) { break await; }"
  ],
  [
    "continue-label",
    "while (o) { continue await; }"
  ]
,

  [
    "declare-variable-initializer",
    "declare let x = await;"
  ],
  [
    "declare-class-initializer",
    "declare class C { p = await; }"
  ],
  [
    "declare-function-body",
    "declare function f(a = await) {}"
  ],
  [
    "this-parameter-decorator",
    "class C { m(@await this) {} }"
  ],
  [
    "this-parameter-initializer",
    "class C { m(this = await) {} }"
  ],
  [
    "bodyless-accessors",
    "abstract class A { abstract get [await](): number; abstract set [await](v); }"
  ],
  [
    "parameter-name",
    "class C { m(await) {} }"
  ],
  [
    "parameter-binding-name",
    "class C { m({ [await]: a }) {} }"
  ],
  [
    "escaped-reference",
    "const x = aw\\u0061it;"
  ],
  [
    "declare-property",
    "class C { declare [await]: number; }"
  ],
  [
    "auto-accessor-name",
    "class C { accessor [await] = 1; }"
  ],
  [
    "optional-access-name",
    "const x = source?.await;"
  ]

];
const cases = [];
function observe(file, text) {
 const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
 return {
  diagnostics: source.parseDiagnostics.map(d => ({code:d.code,start:d.start,length:d.length,message:ts.flattenDiagnosticMessageText(d.messageText,"\n")})),
  statements: source.statements.map(s => ({kind:s.kind,pos:s.pos,end:s.end,await_context:!!(s.flags & ts.NodeFlags.AwaitContext)})),
 };
}
function add(id,file,text) {
 const first = observe(file,text); assert.deepEqual(first,observe(file,text),id);
 cases.push({case_id:id,file,text,expected:first});
}
for (const [id, body] of shapes) {
 for (const [context,prefix] of [["script",""],["module","export {}; "],["run","export {}; await; "]]) {
  add(`${id}/${context}`,"main.ts",prefix+body);
 }
}
for (const [id,body] of [
 ["attribute-name", "const e = <a await={1} />;"],
 ["namespaced-name", "const e = <a await:x={1} />;"],
 ["attribute-expression", "const e = <a x={await} />;"],
 ["element-name", "const e = <await />;"],
 ["qualified-element", "const e = <o.await />;"],
 ["child-expression", "const e = <a>{await}</a>;"],
]) for (const [context,prefix] of [["script",""],["module","export {}; "],["run","export {}; await; "]]) add(`${id}/${context}`,"main.tsx",prefix+body);
const result={version:1,typescript:ts.version,repetitions:2,compiler_sha256:sha256(fs.readFileSync(path.join(root,"vendor/typescript-6.0.3/lib/typescript.js"))),observer_sha256:sha256(fs.readFileSync(import.meta.filename)),cases};
const output=path.join(root,"crates/syntax/tests/fixtures/await-flag-boundary.json");
if (process.argv[2]==="--write") fs.writeFileSync(output,JSON.stringify(result,null,2)+"\n");
else assert.deepEqual(result,JSON.parse(fs.readFileSync(output,"utf8")));
console.log(`Await flag boundary: ${cases.length} cases, two identical syntax observations each`);
