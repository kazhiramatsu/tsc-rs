// H2.8a-A-RES-BUNDLE-METADATA-T1: adjacent controls for the parse-node
// `commentRange` carried from a System bundle's JavaScript transform into the
// same command's declaration transform (tsc never disposes a Bundle root's
// annotated parse nodes: `_tsc.js:25302-25310` with `getParseTreeNode(bundle)`
// undefined). The only parse-node `commentRange` producer reachable in the
// JavaScript transforms of TypeScript 6.0.3 is the class-fields private
// receiver (`setCommentRange(receiver, moveRangePos(receiver, -1))`,
// `_tsc.js:96407` / `96808`), so every family here places that producer on a
// different source, endpoint kind, mount order, text encoding or lifetime.
//
// IDs are fixed here before any observation; expected outputs are observed by
// `scripts/observe-bundle-metadata-t1.mjs`, never written by hand. The frozen
// T1 rows of `decorator-binding-inputs.json` are not reused or rewritten.
//
// usage: node scripts/generate-bundle-metadata-t1-inputs.mjs
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/bundle-metadata-t1-inputs.json");

// The frozen T1 rows' option tuple (lifecycle family, System outFile with
// JavaScript, declaration and both maps, CRLF, set semantics).
const base = { strict: true, allowJs: false, checkJs: false, declaration: true, declarationMap: true,
  sourceMap: true, skipDefaultLibCheck: true, noErrorTruncation: true, newLine: 0, target: 2, module: 4,
  useDefineForClassFields: false, removeComments: false, outFile: "/project/out/bundle.js",
  ignoreDeprecations: "6.0" };
const targets = { es2015: 2, es2022: 9, esnext: 99 };

// The adjacent families use an AMD outFile: a System bundle hoists every
// top-level class into `A = class A ... };` and today maps that closing `};`
// differently from tsc (the R12 family, recorded by the two `residual` rows
// below, JavaScript map only). The packet lifetime is the same for every
// bundle root; the frozen T1 rows themselves stay System.
const PLAIN = `class A extends Object {
    static f = 1;
}
export const tailA = A.name;
`;
const EXPORTED_PLAIN = PLAIN.replace("class A", "export class A");
// Parsed Identifier receiver of a lowered static private field read: the
// class-fields transform annotates the parsed `B` with `{pos: -1, end}`.
const STATIC_GET = `class B extends Object {
    static #p = 1;
    static readP() { return B.#p; }
}
export const tailB = B.readP();
`;
const STATIC_GET_A = STATIC_GET.replace(/\bB\b/g, "A");
const STATIC_SET = `class B extends Object {
    static #p = 1;
    static writeP(v: number) { B.#p = v; }
}
B.writeP(2);
export const tailB = B.name;
`;
// A compound assignment clones an identifier receiver before annotating it
// (`createCopiableReceiverExpr`): the parsed `B` keeps no comment range.
const STATIC_COMPOUND = `class B extends Object {
    static #p = 1;
    static bump() { B.#p += 1; }
}
B.bump();
export const tailB = B.name;
`;
// Parsed ThisKeyword receivers (static and instance private fields).
const STATIC_THIS = `class B extends Object {
    static #p = 1;
    static readP() { return this.#p; }
}
export const tailB = B.readP();
`;
const INSTANCE_THIS = `class B extends Object {
    #q = 1;
    readQ() { return this.#q; }
}
export const tailB = new B().readQ();
`;
// The parsed receiver inside a nested class scope.
const NESTED = `export function make() {
    class B extends Object {
        static #p = 1;
        static readP() { return B.#p; }
    }
    return B.readP();
}
export const tailB = make();
`;
// The frozen T1 shape's decorator route: a class decorator makes ESNext /
// ES2022 lowering reach the same producer through
// InternalEmitFlags.TransformPrivateStaticElements (cause one, PR #549).
const DEC_PRELUDE = `function dec(value: any, context: any): any { return value; }
`;
const DECORATED_GET = `${DEC_PRELUDE}@dec
class B extends Object {
    static #p = 1;
    static readP() { return B.#p; }
}
export const tailB = B.readP();
`;
// Non-BMP text before the receiver: byte offsets and UTF-16 offsets differ.
const UTF16_GET = `const s = "😀😀";
class B extends Object {
    static #p = 1;
    static readP() { return /* 😀 */ B.#p; }
}
export const tailB = B.readP();
`;
const JSON_DATA = `{ "n": 1 }\n`;
const JSON_IMPORT = `import * as data from "./data.json";
export const n = data.n;
`;

const cases = [];
const add = (family, target, variant, files, options = {}, roots = null) => {
  const case_id = `bundle-metadata-t1/${family}/${target}/${variant}`;
  assert.ok(!cases.some(c => c.case_id === case_id), case_id);
  cases.push({ case_id, family, variant, roots: roots ?? files.map(f => f.path),
    files, options: { ...base, module: 2, target: targets[target], ...options } });
};
const file = (name, text) => ({ path: `/project/${name}`, text });

add("receiver", "es2015", "static-get-second-file", [file("main.ts", PLAIN), file("second.ts", STATIC_GET)]);
add("receiver", "es2015", "static-get-first-file", [file("main.ts", STATIC_GET), file("second.ts", PLAIN)]);
add("receiver", "es2015", "static-get-both-files", [file("main.ts", STATIC_GET_A), file("second.ts", STATIC_GET)]);
add("receiver", "es2015", "static-set-second-file", [file("main.ts", PLAIN), file("second.ts", STATIC_SET)]);
add("receiver", "es2015", "static-compound-second-file", [file("main.ts", PLAIN), file("second.ts", STATIC_COMPOUND)]);
add("receiver", "es2015", "static-this-second-file", [file("main.ts", PLAIN), file("second.ts", STATIC_THIS)]);
add("receiver", "es2015", "instance-this-second-file", [file("main.ts", PLAIN), file("second.ts", INSTANCE_THIS)]);
add("receiver", "es2015", "nested-class-second-file", [file("main.ts", PLAIN), file("second.ts", NESTED)]);
add("decorated", "es2022", "static-get-second-file", [file("main.ts", PLAIN), file("second.ts", DECORATED_GET)]);
add("decorated", "esnext", "static-get-second-file", [file("main.ts", PLAIN), file("second.ts", DECORATED_GET)]);
// Program order places an imported JSON module before its importer; the
// declaration bundle excludes JSON sources, so the annotated source's mount
// index differs between the JavaScript and declaration arenas. JSON modules
// are refused for System / UMD / None bundles (TS5071) and need a non-classic
// resolution (TS5070): these two rows use an AMD outFile with Node10.
add("mount-order", "es2015", "json-before-annotated", [file("main.ts", JSON_IMPORT), file("data.json", JSON_DATA), file("second.ts", STATIC_GET)],
  { resolveJsonModule: true, moduleResolution: 2, module: 2 }, ["/project/main.ts", "/project/second.ts"]);
add("mount-order", "es2015", "json-between-annotated", [file("main.ts", STATIC_GET_A), file("data.json", JSON_DATA), file("second.ts", JSON_IMPORT + STATIC_GET)],
  { resolveJsonModule: true, moduleResolution: 2, module: 2 }, ["/project/main.ts", "/project/second.ts"]);
add("utf16", "es2015", "non-bmp-before-receiver", [file("main.ts", PLAIN), file("second.ts", UTF16_GET)]);
// Lifetimes that never carry a packet: SourceFile roots, no declaration
// output, and declaration-only emission without a JavaScript transform.
add("lifetime", "es2015", "source-file-roots", [file("main.ts", PLAIN), file("second.ts", STATIC_GET)],
  { outFile: undefined, outDir: "/project/out", module: 5 });
add("lifetime", "es2015", "no-declaration", [file("main.ts", PLAIN), file("second.ts", STATIC_GET)],
  { declaration: false, declarationMap: false });
add("lifetime", "es2015", "emit-declaration-only", [file("main.ts", PLAIN), file("second.ts", STATIC_GET)],
  { emitDeclarationOnly: true });
// Recorded residual outside this slice: undecorated classes in a System
// bundle (exported or not) are hoisted into `A = class A ... };` and tsc maps
// that closing `};` line twice (columns 13 and 14 to the class end); the
// Rust System transform emits neither segment while every byte of the
// JavaScript, declaration and declaration map matches. The same
// JavaScript-only difference exists without any declaration output, so the
// packet cannot cause it; both rows are frozen as known native divergences.
add("residual", "es2015", "system-export-class-map", [file("main.ts", EXPORTED_PLAIN), file("second.ts", STATIC_GET)], { module: 4 });
add("residual", "es2015", "system-hoisted-class-map", [file("main.ts", PLAIN), file("second.ts", STATIC_GET)], { module: 4 });
for (const c of cases) for (const key of Object.keys(c.options)) if (c.options[key] === undefined) delete c.options[key];

const manifest = { version: 1,
  description: "H2.8a-A-RES-BUNDLE-METADATA-T1 adjacent controls: parse-node commentRange carried from a bundle's JavaScript transform to its declaration transform (receiver source, endpoint kind, mount order, UTF-16 text, lifetimes).",
  cases };
assert.equal(new Set(cases.map(c => c.case_id)).size, cases.length);
assert.equal(cases.length, 18);
fs.writeFileSync(destination, JSON.stringify(manifest, null, 1) + "\n");
console.log(JSON.stringify({ destination: path.relative(root, destination), cases: cases.length }));
