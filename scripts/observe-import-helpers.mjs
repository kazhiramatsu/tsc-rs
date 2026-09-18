// Complete commands for source-owned external helper imports.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/import-helpers.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const helperNames = ["__awaiter", "__generator", "__importDefault", "__importStar", "__exportStar", "__createBinding", "__setModuleDefault"];
const ambient = 'declare module "dep" { const value: number; export default value; export const item: number; }\n'
  + 'declare module "tslib" { const value: number; export default value; export const item: number; '
  + helperNames.map(name => `export const ${name}: any;`).join(' ') + ' }\n';
const defaults = { strict: true, skipDefaultLibCheck: true, noErrorTruncation: true,
  sourceMap: true, outDir: "/project/out", esModuleInterop: true, importHelpers: true };
const shapes = [
  ["async-helper", 'export async function read() { return 1; }\n'],
  ["default-import", 'import value from "dep"; export const result = value;\n'],
  ["namespace-import", 'import * as library from "dep"; export const result = library.item;\n'],
  ["mixed-default-named", 'import value, { item } from "dep"; export const result = value + item;\n'],
  ["named-default-import", 'import { default as value } from "dep"; export const result = value;\n'],
  ["export-star", 'export * from "dep";\n'],
  ["export-namespace", 'export * as library from "dep";\n'],
  ["default-reexport", 'export { default as result } from "dep";\n'],
  ["user-tslib", 'import value from "tslib"; export async function read() { return value; }\n'],
  ["occupied-tslib", 'import value from "tslib"; const tslib_1 = 1; export async function read() { return value + tslib_1; }\n'],
  ["source-helper-name", 'const __awaiter = 1; export async function read() { return __awaiter; }\n'],
  ["no-demand", 'export const result = 1;\n'],
];
const inputs = [];
function add(group, shape, text, options, extra = {}) {
  const extension = extra.extension ?? "ts";
  const main = `/project/main.${extension}`;
  const files = extra.files ?? [{ path: main, text }, { path: "/project/ambient.d.ts", text: ambient }];
  const roots = extra.roots ?? [main, "/project/ambient.d.ts"];
  // Both compilers enter their ordinary config-loading/Program command route.
  // Keep the real configuration intact, including options outside old test adapters.
  const compilerOptions = { ...defaults, ...options };
  if (extension === "js") Object.assign(compilerOptions, { allowJs: true, checkJs: true });
  inputs.push({ case_id: `import-helpers/${group}/${shape}`, roots, files, options: {},
    config: JSON.stringify({ compilerOptions, files: roots.map(name => name.slice('/project/'.length)) }),
    witness: { group, shape, compiler_options: compilerOptions } });
}
for (const module of ["commonjs", "amd", "umd"])
  for (const target of ["es5", "es2015"])
    for (const [shape, text] of shapes) add(`${module}/${target}`, shape, text, { module, target });
for (const module of ["commonjs", "amd", "umd"]) {
  add(`${module}/options`, "import-helpers-off", shapes[0][1], { module, target: "es2015", importHelpers: false });
  add(`${module}/options`, "no-emit-helpers", shapes[0][1], { module, target: "es2015", noEmitHelpers: true });
  add(`${module}/options`, "interop-off", shapes[1][1], { module, target: "es2015", esModuleInterop: false });
  add(`${module}/options`, "error-blocked", 'export async function read() { return missing; }\n', { module, target: "es2015", noEmitOnError: true });
}
for (const [shape, text] of [
  ["script", 'async function read() { return 1; }\n'],
  ["commonjs-exports", 'exports.read = async function read() { return 1; };\n'],
  ["commonjs-require", 'const library = require("dep"); async function read() { return library.item; }\n'],
]) for (const importHelpers of [true, false])
  add(`commonjs/javascript`, `${shape}/${importHelpers ? 'imported' : 'inline'}`, text,
    { module: "commonjs", target: "es2015", importHelpers }, { extension: "js" });
for (const module of ["amd", "umd"])
  for (const importHelpers of [true, false])
    add(`${module}/javascript`, `commonjs-script/${importHelpers ? 'imported-option' : 'inline'}`,
      'exports.read = async function read() { return 1; };\n',
      { module, target: "es2015", importHelpers }, { extension: "js" });
add('esnext/control', 'named-helper-import', shapes[0][1], { module: "esnext", target: "es2015" });
add('esnext/control', 'no-demand', shapes[11][1], { module: "esnext", target: "es2015" });
add('esnext/control', 'helper-name-collision', shapes[10][1], { module: "esnext", target: "es2015" });
add('esnext/control', 'import-helpers-off', shapes[0][1], { module: "esnext", target: "es2015", importHelpers: false });
for (const order of ["helper-first", "helper-last", "both-helpers"]) {
  const a = { path: '/project/a.ts', text: shapes[9][1] };
  const b = { path: '/project/b.ts', text: order === 'both-helpers' ? shapes[8][1] : shapes[11][1] };
  const ordered = order === 'helper-last' ? [b, a] : [a, b];
  add('commonjs/source-isolation', order, '', { module: "commonjs", target: "es2015" },
    { files: [...ordered, { path: '/project/ambient.d.ts', text: ambient }], roots: [...ordered.map(file => file.path), '/project/ambient.d.ts'] });
}
for (const module of ['node16', 'node18', 'node20', 'nodenext']) {
  add('implied-format', module, '', { module, target: 'es2015' }, {
    files: [
      { path: '/project/package.json', text: '{"type":"module"}' },
      { path: '/project/main.ts', text: shapes[0][1] },
      { path: '/project/sub/package.json', text: '{"type":"commonjs"}' },
      { path: '/project/sub/other.ts', text: shapes[8][1] },
      { path: '/project/ambient.d.ts', text: ambient },
    ], roots: ['/project/main.ts', '/project/sub/other.ts', '/project/ambient.d.ts'] });
}
for (const [shape, text, options] of [
  ['declaration-map', shapes[8][1], { declaration: true, declarationMap: true }],
  ['declaration-only', shapes[8][1], { declaration: true, declarationMap: true, emitDeclarationOnly: true }],
  ['import-equals-binding', 'import library = require("tslib"); export async function read() { return library.item; }\n', {}],
]) add('commonjs/adjacent', shape, text, { module: 'commonjs', target: 'es2015', ...options });
add('system/control', 'helper-demand', shapes[0][1], { module: 'system', target: 'es2015' });
add('system/control', 'no-demand', shapes[11][1], { module: 'system', target: 'es2015' });
add('bundle/control', 'import-helpers', shapes[0][1], { module: 'amd', target: 'es2015', outDir: undefined, outFile: '/project/out.js' });
assert.equal(inputs.length, 111);
// Integration audit: generated aliases must avoid the complete parsed-name
// census, including names inside functions and already numbered spellings.
for (const module of ["es2015", "esnext", "preserve"])
  for (const target of ["es5", "es2015"])
    for (const [shape, text] of [
      ["numbered", 'const __awaiter = 1, __awaiter_1 = 2; export async function read() { return __awaiter + __awaiter_1; }\n'],
      ["nested", 'export function local(__awaiter: number) { return __awaiter; } export async function read() { return 1; }\n'],
      ["multiple", 'const __awaiter = 1, __generator = 2, __generator_1 = 3; export async function read() { return __awaiter + __generator + __generator_1; }\n'],
      ["two-files", 'const __awaiter = 1; export async function read() { return __awaiter; }\n'],
    ]) {
      const extra = shape === "two-files" ? {
        files: [{ path: "/project/main.ts", text },
                { path: "/project/other.ts", text: 'const __awaiter = 1, __awaiter_1 = 2; export async function read() { return __awaiter_1; }\n' },
                { path: "/project/ambient.d.ts", text: ambient }],
        roots: ["/project/main.ts", "/project/other.ts", "/project/ambient.d.ts"],
      } : {};
      add(`${module}/${target}/alias-audit`, shape, text, { module, target, ignoreDeprecations: "6.0" }, extra);
    }
assert.equal(inputs.length, 135);
// Ordinary-source controls for transforms preceding standard decorators.
// They exercise the API shared-node boundary without a custom transformer.
const prelude = 'function dec(...args: any[]): any { return args[0]; }\nclass Base { static x = 1; }\n';
for (const [shape, text, experimentalDecorators] of [
  ["parameter-property", '@dec export class Derived extends Base { constructor(public value: number) { super(); } static result = super.x++; }\n', false],
  ["namespace", 'export namespace N { @dec export class Derived extends Base { static result = super.x++; } }\n', false],
  ["enum", 'export enum E { Value = (() => { @dec class Derived extends Base { static result = super.x++; } return Derived.result; })() }\n', false],
  ["legacy-decorators", '@dec export class Derived extends Base { static result = super.x++; }\n', true],
]) add('ordinary-transform-boundary', shape, prelude + text,
       { module: "esnext", target: "es2015", importHelpers: false, experimentalDecorators });
assert.equal(inputs.length, 139);
// A fresh Program has no builder affected-file traversal. This valid flag
// must preserve both JavaScript and declaration/map callback observations.
for (const assumeChangesOnlyAffectDirectDependencies of [false, true])
  for (const declaration of [false, true])
    add('fresh-program-options', `direct-dependencies-${assumeChangesOnlyAffectDirectDependencies}/declaration-${declaration}`,
        shapes[0][1], { module: 'esnext', target: 'es2015', declaration,
          declarationMap: declaration, assumeChangesOnlyAffectDirectDependencies });
assert.equal(inputs.length, 143);
// Audit the remaining raw-text comment refusal against real syntax and
// lookalike literal/comment text, with both comment-output polarities.
const commentShapes = [
  ['string-private', 'export const value = "#x /* comment */ in";\n'],
  ['string-optional', 'export const value = "a?.b /* comment */ as T";\n'],
  ['template-private', 'export const value = `#x /* comment */ in`;\n'],
  ['comment-only', '// #x /* comment */ in; a?.b /* comment */ as T\nexport const value = 1;\n'],
  ['private-in-inline', 'export class C { #x = 1; has(value: object) { return #x /* brand */ in value; } }\n'],
  ['private-in-lines', 'export class C { #x = 1; has(value: object) { return #x /* first */\n /* second */ in value; } }\n'],
  ['optional-as', 'declare const object: { x: number } | undefined; export const value = (object?.x /* value */ as number);\n'],
  ['optional-angle', 'declare const object: { x: number } | undefined; export const value = <number> /* boundary */ object?.x;\n'],
  ['private-in-line-comment', 'export class C { #x = 1; has(value: object) { return #x // brand\n in value; } }\n'],
  ['private-in-jsdoc', 'export class C { #x = 1; has(value: object) { return #x /** brand */ in value; } }\n'],
  ['optional-non-null', 'declare const object: { x: number } | undefined; export const value = object?.x /* value */ !;\n'],
  ['optional-satisfies', 'declare const object: { x: number } | undefined; export const value = (object?.x /* value */ satisfies number | undefined);\n'],
  ['optional-two-comments', 'declare const object: { x: number } | undefined; export const value = (object?.x /* first */ /* second */ as number);\n'],
  ['optional-line-comment', 'declare const object: { x: number } | undefined; export const value = (object?.x // value\n as number);\n'],
  ['optional-own-line', 'declare const object: { x: number } | undefined; export const value = (object?.x\n /* value */ as number);\n'],
];
for (const target of ['es2015', 'es2022', 'esnext'])
  for (const removeComments of [false, true])
    for (const [shape, text] of commentShapes)
      add(`comment-boundary/${target}/remove-${removeComments}`, shape, text,
          { module: 'esnext', target, importHelpers: false, removeComments });
assert.equal(inputs.length, 233);
// Cross-review controls for converting a concise arrow to a function block:
// parsed parentheses survive, object wrapping changes at the factory boundary,
// and an arrow without hoisted temporaries stays concise.
for (const target of ['es2015', 'es2022', 'esnext'])
  for (const useDefineForClassFields of [false, true])
    for (const [shape, body] of [
      ['parsed-comma', '(super.x++, 1)'],
      ['object-body', '({ value: super.x++ })'],
      ['no-temporaries', 'super.x'],
    ]) add(`arrow-environment/${target}/define-${useDefineForClassFields}`, shape,
      prelude + `@dec export class Derived extends Base { static f = () => ${body}; }\n`,
      { module: 'esnext', target, importHelpers: false, useDefineForClassFields });
assert.equal(inputs.length, 251);
// PEE ownership under surrounding token/statement contexts: retaining the
// no-ASI forwarding lane must also retain comments and exact map boundaries.
const commentContexts = [
  ['return-as', 'export function f() { return (object?.x /* c */ as number); }'],
  ['return-angle', 'export function f() { return <number> /* b */ object?.x /* c */; }'],
  ['throw-as', 'export function f() { throw (object?.x /* c */ as number); }'],
  ['paren-outer', 'export const value = (object?.x as number) /* c */;'],
  ['call-argument', 'declare function f(a: number, b: number): number; export const value = f(object?.x as number /* c */, 1);'],
  ['default-parameter', 'export const f = (a = object?.x /* c */ as number) => a;'],
  ['no-asi-return', 'export function f() { return\n(object?.x as number); }'],
  ['no-asi-return-nested', 'export function f() { return (\n/* a */ object?.x as number) /* c */ satisfies unknown; }'],
  ['no-asi-throw-nested', 'export function f() { throw (\n/* a */ object?.x as number) /* c */ satisfies unknown; }'],
  ['no-asi-yield-nested', 'export function* f() { yield (\n/* a */ object?.x as number) /* c */ as unknown; }'],
  ['no-asi-non-null-nested', 'export function f() { return (\n/* a */ object?.x /* c */ as number)!; }'],
  ['call-angle-leading', 'declare function f(value: number): number; declare const x: number; export const value = f(<number>/*c*/x);'],
  ['call-as-leading', 'declare function f(value: number): number; declare const x: number; export const value = f((/*c*/ x as number));'],
];
for (const target of ['es2015', 'es2022', 'esnext'])
  for (const [shape, text] of commentContexts)
    add(`comment-context/${target}`, shape,
      `declare const object: { x: number } | undefined;\n${text}\n`,
      { module: 'esnext', target, importHelpers: false });
assert.equal(inputs.length, 290);
// Checker audit controls compare complete commands, including negative gates.
for (const module of ["commonjs", "es2015"])
  for (const extension of ["ts", "js"])
    for (const esModuleInterop of [false, true]) {
      const main = `/project/main.${extension}`;
      add('checker-audit/import-equals', `${module}/${extension}/${esModuleInterop}`, '',
          { module, target: 'es2015', esModuleInterop, noEmit: true }, {
        extension, files: [{ path: '/project/a.ts', text: 'class Foo {}\nexport = Foo;\n' },
          { path: main, text: 'import { Foo } from "./a";\n' }], roots: ['/project/a.ts', main] });
    }
for (const [shape, constructor] of [
  ['bare', 'constructor();'], ['comment', '/* 😀 */ constructor();'], ['public', 'public constructor();'],
]) add('checker-audit/constructor', shape, `class A { ${constructor} }\n`,
       { module: 'commonjs', target: 'es2015', noEmit: true }, { extension: 'js' });
for (const [shape, declaration] of [
  ['direct', 'export declare const __awaiter: any;'],
  ['reexport', 'export { __awaiter } from "./helper";'],
  ['star', 'export * from "./helper";'],
  ['type-only-value-absent', 'export interface __awaiter {}'],
]) add('checker-audit/tslib-alias', shape, '', { module: 'commonjs', target: 'es2015' }, {
  files: [{ path: '/project/main.ts', text: shapes[0][1] },
    { path: '/project/node_modules/tslib/index.d.ts', text: declaration + '\n' },
    { path: '/project/node_modules/tslib/helper.d.ts', text: 'export declare const __awaiter: any;\n' }],
  roots: ['/project/main.ts'],
});
for (const target of ['es2017', 'es2018'])
  for (const importHelpers of [false, true])
    for (const [shape, text] of [
      ['parameter', 'export function f({a, ...rest}: {a: number, b: number}) { return rest; }'],
      ['variable', 'const o = {a: 1, b: 2}; export const {a, ...rest} = o;'],
      ['assignment', 'const o = {a: 1, b: 2}; let a = 0, rest = {}; ({a, ...rest} = o); export {rest};'],
    ]) add('checker-audit/rest', `${target}/${importHelpers}/${shape}`, text + '\n',
           { module: 'commonjs', target, importHelpers });
assert.equal(inputs.length, 317);
for (const isolatedModules of [false, true])
  for (const verbatimModuleSyntax of [false, true])
    for (const [shape, text, extraFiles] of [
      ['local-value', 'import { T } from "./types"; const T = 1; export default T;', []],
      ['global-type-use', 'import { Date } from "./types"; export type D = Date;', []],
      ['namespace-types', 'namespace Types { export type T = number; } declare namespace Ambient { const x: number; }', []],
      ['same-file-enum', 'enum E { A = 1, B = A }', []],
      ['value-export-import', 'import { N } from "./types"; export import Alias = N;', []],
    ]) add('checker-audit/isolated-controls', `${isolatedModules}/${verbatimModuleSyntax}/${shape}`, '',
        { module: 'esnext', target: 'es2015', isolatedModules, verbatimModuleSyntax, noEmit: true }, {
      files: [{ path: '/project/main.ts', text: text + '\n' },
        { path: '/project/types.ts', text: 'export type T = number; export interface Date { year: number; } export namespace N { export const x = 1; }\n' },
        ...extraFiles], roots: ['/project/main.ts'],
    });
const adjacentHelpers = [
  ['assign', 'const o = {a: 1}; export const result = {...o};'],
  ['read-variable', 'export const [first, ...rest] = [1, 2];'],
  ['read-parameter', 'export function f([first, ...rest]: number[]) { return rest; }'],
  ['read-assignment', 'let first = 0, rest: number[] = []; [first, ...rest] = [1, 2]; export {rest};'],
];
for (const target of ['es5', 'es2015'])
  for (const importHelpers of [false, true])
    for (const [shape, text] of adjacentHelpers)
      add('checker-audit/adjacent-helper', `${target}/${importHelpers}/${shape}`, text + '\n',
          { module: 'commonjs', target, importHelpers, downlevelIteration: true, ignoreDeprecations: '6.0' });
for (const importHelpers of [false, true])
  for (const [shape, text] of adjacentHelpers.slice(1))
    add('checker-audit/adjacent-helper', `iteration-off/${importHelpers}/${shape}`, text + '\n',
        { module: 'commonjs', target: 'es5', importHelpers, downlevelIteration: false, ignoreDeprecations: '6.0' });
assert.equal(inputs.length, 359);
for (const [shape, tag, parameter] of [
  ['bracket', '@param [b]', 'b'], ['optional-type', '@param {number=} b', 'b'],
  ['jsdoc-default', '@param [b=1]', 'b'], ['initializer', '@param [b]', 'b = 1'],
]) add('checker-audit/js-optional', shape,
  `/** @param {number} n */ function accept(n) {}\n/** @typedef {(a: string, b: number) => void} Fn */\n/** @type {Fn} */ const fn = /** ${tag} */ function self(a, ${parameter}) { accept(b); self(""); self("", undefined); };\n`,
  { module: 'commonjs', target: 'es2015', noEmit: true }, { extension: 'js' });
add('checker-audit/js-optional', 'typescript',
  'function accept(n: number) {}\nconst fn: (a: string, b: number) => void = function self(a, b?: number) { accept(b); self(""); self("", undefined); };\n',
  { module: 'commonjs', target: 'es2015', noEmit: true });
for (const [shape, extension, comment] of [
  ['javascript', 'js', ''], ['augments', 'js', '/** @augments A<number> */'],
  ['excess', 'js', '/** @augments A<number, number> */'], ['typescript', 'ts', ''],
]) {
  const main = `/project/main.${extension}`;
  add('checker-audit/js-base', shape, '', { module: 'commonjs', target: 'es2015', noEmit: true }, {
    extension, files: [{ path: main, text: `${comment}\nclass B extends A {}\nnew B().x;\n` },
      { path: '/project/base.d.ts', text: 'declare class A<T> { x: T; }\n' }], roots: [main, '/project/base.d.ts'] });
}
for (const [shape, expression] of [
  ['paren', '(2 * 2);'], ['binary', '(2 * 2) + 1;'], ['nested', '((2 * 2));'],
  ['iife', '(function(){})();'], ['assignment', 'x = (1);'], ['label', 'a: (1);'],
]) add('checker-audit/jsdoc-host', shape,
       `let x = 0;\n/** @typedef {number} T */\n${expression}\n/** @param {T} value */ function accept(value) {}\naccept("wrong");\n`,
       { module: 'commonjs', target: 'es2015', noEmit: true }, { extension: 'js' });
for (const declarationPath of ['/project/types.d.ts', '/project/lib.es5.d.ts'])
  add('checker-audit/library-related', declarationPath.endsWith('lib.es5.d.ts') ? 'user-lib-looking-name' : 'user', '',
      { module: 'commonjs', target: 'es2015', noEmit: true }, {
    files: [{ path: '/project/main.ts', text: 'const box: Box = {value: "wrong"}; const dict: Dict = {key: "wrong"};\n' },
      { path: declarationPath, text: 'interface Box { value: number; } interface Dict { [key: string]: number; }\n' }],
    roots: ['/project/main.ts', declarationPath],
  });
const relatedSource = 'const record: Record<string, number> = {key: "wrong"}; const array: number[] = ["wrong"];\n';
add('checker-audit/library-related', 'default-library', relatedSource,
    { module: 'commonjs', target: 'es2015', noEmit: true });
add('checker-audit/library-related', 'reference-library', '/// <reference lib="es5" />\n' + relatedSource,
    { module: 'commonjs', target: 'es2015', lib: [], noEmit: true });
add('checker-audit/library-related', 'explicit-root-no-lib', '',
    { module: 'commonjs', target: 'es2015', noLib: true, noEmit: true }, {
  files: [{ path: '/project/main.ts', text: relatedSource },
    { path: '/project/lib.es5.d.ts', text: fs.readFileSync(path.join(root, 'vendor/typescript-6.0.3/lib/lib.es5.d.ts'), 'utf8') }],
  roots: ['/project/main.ts', '/project/lib.es5.d.ts'],
});
assert.equal(inputs.length, 379);
// CommonJS augmentation and late-member clones, with file order and negative gates.
for (const checkJs of [false, true])
  for (const shape of ['typescript', 'declaration', 'type-only']) {
    const main = shape === 'declaration' ? '/project/main.d.ts' : '/project/main.ts';
    const member = shape === 'type-only' ? 'interface a { value: number }' : 'const a: number;';
    add('checker-audit/cjs-augmentation', `${checkJs}/${shape}`, '',
        { module: 'commonjs', target: 'es2015', noEmit: true, allowJs: true, checkJs }, {
      files: [{ path: '/project/test.js', text: 'module.exports = { a: "ok" };\n' },
        { path: main, text: `import { a } from "./test";\ndeclare module "./test" { export ${member} }\n${shape === 'declaration' ? '' : 'a.toFixed();\n'}` }],
      roots: ['/project/test.js', main],
    });
  }
for (const shape of ['typedef-computed', 'no-typedef', 'no-computed', 'static', 'extra-export'])
  for (const reverse of [false, true]) {
    const files = [
      { path: '/project/main.js', text: 'const LazySet = require("./LazySet");\n/** @type {LazySet} */ const set = undefined;\nset.addAll(set);\n' },
      { path: '/project/LazySet.js', text:
        (shape === 'no-typedef' ? '' : '/** @typedef {Object} SomeObject */\n') +
        'class LazySet {\n/** @param {LazySet} iterable */\n' +
        (shape === 'static' ? 'static ' : '') + 'addAll(iterable) {}\n' +
        (shape === 'no-computed' ? '' : '[Symbol.iterator]() {}\n') + '}\nmodule.exports = LazySet;\n' +
        (shape === 'extra-export' ? 'module.exports.extra = 1;\n' : '') },
    ];
    add('checker-audit/cjs-late-members', `${shape}/${reverse}`, '',
        { module: 'commonjs', target: 'es2015', noEmit: true, allowJs: true, checkJs: true },
        { files, roots: (reverse ? [...files].reverse() : files).map(file => file.path) });
  }
for (const [shape, value, targetType] of [
  ['quoted', '"smth"', "'literal'"], ['number', '{10}', "'literal'"],
  ['widened', '{"smth" as string}', "'literal'"], ['const', '{"smth" as const}', "'literal'"],
  ['union', '"smth"', "'literal' | 'other'"],
]) add('checker-audit/jsx-display', shape,
       `declare namespace JSX { interface IntrinsicElements { [k: \`foo\${string}\`]: { prop: ${targetType} }; } }\n<foobaz prop=${value} />;\n`,
       { module: 'commonjs', target: 'es2015', jsx: 'preserve', noEmit: true }, { extension: 'tsx' });
for (const [shape, text] of [
  ['tuple-substitution', 'type Must<T extends any[]> = T; type H<T extends any[]> = T extends number[] ? Must<{[I in keyof T]: 1}> : never; declare const x: H<[3,4,5]>; const y: [1,1,1] = x;'],
  ['object-substitution', 'type H<T> = T extends {x:string} ? {[I in keyof T]: 1} : never; declare const x: H<{x:string, y:number}>; const y: {x:1,y:1} = x;'],
  ['name-remapping', 'type H<T extends any[]> = T extends number[] ? {[I in keyof T as `p${I & string}`]: 1} : never; declare const x: H<[3,4]>; const y: {p0:1,p1:1} = x;'],
  ['mixed-intersection', 'type Box<T> = {[K in keyof T]: {value:T[K]}}; declare const x: Box<string[] & {x:string}>; const y: {value:string} = x.x;'],
]) add('checker-audit/mapped-substitution', shape, text + '\n',
       { module: 'commonjs', target: 'es2015', noEmit: true });
for (const [shape, text] of [
  ['object-intersection', 'type E<T> = T extends infer O ? {[K in keyof O]: O[K]} : never; declare const x: E<{a:1} & {b:2}>; const y: {a:1,b:3} = x;'],
  ['array-intersection', 'type E<T> = {[K in keyof T]: {value:T[K]}}; declare const x: E<string[] & number[]>; const y: {value:string} = x[0];'],
  ['tuple-intersection', 'type E<T> = {[K in keyof T]: {value:T[K]}}; declare const x: E<[1] & [2]>; const y: {value:1} = x[0];'],
  ['recursive-any', 'type Deep<T> = {[K in keyof T]: Deep<T[K]>}; declare const x: Deep<any>; const y: Deep<{a:{b:number}}> = x;'],
]) add('checker-audit/mapped-recursion', shape, text + '\n',
       { module: 'commonjs', target: 'es2015', noEmit: true });
for (const [shape, text] of [
  ['circular-alias', 'type C = C extends string ? 1 : 0;'],
  ['missing-name', 'type U = Missing extends string ? 1 : 0;'],
]) add('checker-audit/conditional-error', shape, text + '\n',
       { module: 'commonjs', target: 'es2015', noEmit: true });
for (const [shape, assignment] of [
  ['alias-origin', 'function f(x: T1, y: T1 & T2) { x = y; }'],
  ['nullable-target', 'function f(x: T1 | null, y: T1 & T2) { x = y; }'],
  ['incompatible-target', 'function f(x: {a:number} | null, y: T1 & T2) { x = y; }'],
]) add('checker-audit/relation-origin', shape,
       "type T1 = '00' | '01' | '10' | '11' | undefined; type T2 = {a:string} | {b:number};\n" + assignment + '\n',
       { module: 'commonjs', target: 'es2015', noEmit: true });
assert.equal(inputs.length, 413);
for (const target of ['es5', 'es2018'])
  for (const [shape, text] of [
    ['local-rest', 'const o = {a:1,b:2}; const {a, ...rest} = o; export {};'],
    ['default-array', 'export let [x = 1, ...ys] = [1,2];'],
    ['computed-rest', 'const k = "a"; const o = {a:1,b:2}; export const {[k]: v, ...r} = o;'],
    ['imported-read', 'import value from "dep"; export let [x, ...ys] = [value,2];'],
    ['imported-rest', 'import value from "dep"; const o = {a:value,b:2}; export const {a, ...rest} = o;'],
    ['alias-rest', 'const o = {a:1,b:2}; export const {a, ...rest} = o; export {rest as other};'],
  ]) add('checker-audit/module-destructuring', `${target}/${shape}`, text + '\n',
         { module: 'commonjs', target, downlevelIteration: true, ignoreDeprecations: '6.0' });
for (const module of ['amd', 'umd'])
  for (const [shape, text] of [
    ['array', 'export const [x, ...ys] = [1,2];'],
    ['rest', 'const o = {a:1,b:2}; export const {a, ...rest} = o;'],
  ]) add('checker-audit/module-destructuring', `${module}/${shape}`, text + '\n',
         { module, target: 'es2018', downlevelIteration: true });
assert.equal(inputs.length, 429);
for (const [shape, text] of [
  ['early-alias', 'const o = {a:1}; export {a as b}; export const {a} = o;'],
  ['empty-object', 'const o = {a:1}; export const {} = o;'],
  ['empty-array', 'const o = [1]; export const [] = o;'],
]) add('checker-audit/module-destructuring', shape, text + '\n',
       { module: 'commonjs', target: 'es2018', downlevelIteration: false });
assert.equal(inputs.length, 432);
for (const kind of ['intrinsic', 'component'])
  for (const exactOptionalPropertyTypes of [false, true])
    for (const [shape, property, value] of [
      ['quoted', "prop: 'literal'", 'prop="smth"'],
      ['expression', "prop: 'literal'", 'prop={"smth"}'],
      ['nested-object', "prop: { value: 'literal' }", 'prop={{ value: "smth" }}'],
      ['valid-literal', "prop: 'literal'", 'prop="literal"'],
      ['valid-nested', "prop: { value: 'literal' }", 'prop={{ value: "literal" }}'],
      ['optional-missing', "prop?: 'literal'", ''],
      ['optional-undefined', "prop?: 'literal'", 'prop={undefined}'],
    ]) {
      const tag = kind === 'intrinsic' ? 'foo' : 'Comp';
      const declarations = kind === 'intrinsic'
        ? `declare namespace JSX { interface Element {} interface IntrinsicElements { foo: { ${property} }; } }`
        : `declare namespace JSX { interface Element {} } declare function Comp(props: { ${property} }): JSX.Element;`;
      add('checker-audit/jsx-context', `${kind}/${exactOptionalPropertyTypes}/${shape}`,
          `${declarations}\n<${tag} ${value} />;\n`,
          { module: 'commonjs', target: 'es2015', jsx: 'preserve', noEmit: true, exactOptionalPropertyTypes },
          { extension: 'tsx' });
    }
for (const target of ['es5', 'es2018'])
  for (const [shape, text] of [
    ['alias-nested', 'const o = {x:{y:1}}; export const {x:{y}} = o; export {y as z};'],
    ['alias-array-rest', 'export const [p, ...q] = [1,2]; export {q as r};'],
    ['alias-multiple', 'const o = {a:1}; export const {a} = o; export {a as b, a as c};'],
    ['alias-local-control', 'const o = {a:1}; const {a} = o; export {a as b};'],
  ]) add('checker-audit/module-destructuring', `${target}/${shape}`, text + '\n',
         { module: 'commonjs', target, downlevelIteration: true, ignoreDeprecations: '6.0' });
assert.equal(inputs.length, 468);
// Preserve the original malformed inputs and their complete TypeScript recovery
// output. Native refusal is a separate negative boundary, never an exact emit.
for (const input of inputs) {
  if (/^import-helpers\/comment-boundary\/[^/]+\/remove-(false|true)\/optional-(line-comment|own-line)$/.test(input.case_id)) {
    input.rust_expected_parse_recovery = {
      count: 3, recovery_events: 4, owner_slice: 'H2.9', partial_writes: [],
      cause: 'newline before as terminates the expression; missing/skip recovery facts and comment ownership are not admitted by the transform',
    };
  }
}
assert.equal(inputs.filter(input => input.rust_expected_parse_recovery).length, 12);
for (const target of ['es2015', 'es2022', 'esnext'])
  for (const removeComments of [false, true])
    for (const [shape, expression] of [
      ['optional-line-comment', '(object?.x as // value\n number)'],
      ['optional-own-line', '(object?.x as\n /* value */ number)'],
    ]) add(`comment-valid/${target}/remove-${removeComments}`, shape,
      `declare const object: { x: number } | undefined;\nexport const value = ${expression};\n`,
      { module: 'esnext', target, importHelpers: false, removeComments });
assert.equal(inputs.length, 480);
function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [name, text, bom, onError, sources, data] = args;
  const bytes = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), bytes]) : bytes;
  return { index, path: name, kind: ts.isDeclarationFileName(name) ? "declaration" : name.endsWith(".map") && ts.isDeclarationFileName(name.slice(0, -4)) ? "declaration-map" : name.endsWith(".map") ? "source-map" : name.endsWith(".mjs") ? "mjs" : name.endsWith(".cjs") ? "cjs" : "javascript",
    callback_utf8_base64: bytes.toString("base64"), callback_utf8_bytes: bytes.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sources?.map(source => source.fileName) ?? null,
    data_present: data !== undefined, data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null };
}
function sourceMaps(maps) {
  return maps?.map(entry => {
    assert.deepEqual(Object.keys(entry).sort(), ["inputSourceFileNames", "sourceMap"]);
    return { input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap) };
  }) ?? null;
}
function observe(input) {
  const sensitive = input.use_case_sensitive_file_names ?? true;
  const canonical = ts.createGetCanonicalFileName(sensitive);
  const files = new Map(input.files.map(file => [canonical(file.path), file.text]));
  const libraryRoot = path.join(root, "vendor/typescript-6.0.3/lib");
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name) && fs.existsSync(path.join(libraryRoot, path.basename(name)));
  const read = name => files.get(canonical(ts.normalizePath(name))) ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(files.keys(), { currentDirectory: "/project", useCaseSensitiveFileNames: sensitive,
    fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  const host = { ...ts.createCompilerHost(input.options, true), ...overlay,
    getCurrentDirectory: () => "/project", getDefaultLibFileName: options => "/lib/" + ts.getDefaultLibFileName(options),
    getDefaultLibLocation: () => "/lib", useCaseSensitiveFileNames: () => sensitive, getCanonicalFileName: canonical,
    readFile: read, fileExists: name => files.has(canonical(ts.normalizePath(name))) || library(name),
    getSourceFile: (name, options) => { const text = read(name); return text === undefined ? undefined : ts.createSourceFile(name, text, options, true); },
    writeFile: () => assert.fail("unexpected host write") };
  let options = input.options, roots = input.roots ?? input.files.map(file => file.path), errors = [];
  if (input.config) {
    const configPath = "/project/tsconfig.json";
    const parsed = ts.parseJsonSourceFileConfigFileContent(ts.parseJsonText(configPath, input.config),
      { ...host, readDirectory: () => roots }, "/project", undefined, configPath);
    options = parsed.options; roots = parsed.fileNames; errors = parsed.errors;
  }
  const program = ts.createProgram({ rootNames: roots, options, host, configFileParsingDiagnostics: errors });
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)), s => status.push(s), undefined,
    (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: sourceMaps(result.sourceMaps) },
    status_writes: status, exit_code: exit };
}
const cases = inputs.map(input => {
  const first = observe(input); assert.deepEqual(observe(input), first, input.case_id);
  return { ...input, typescript_observation: first };
});
const artifact = { version: 1, typescript: ts.version,
  source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), repetitions: 2, cases };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered);
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`External helper imports: ${cases.length} cases, two identical complete observations each`);
