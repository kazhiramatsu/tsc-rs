// Complete commands for System using export ownership and disposal temporary scope.
// The host and tuple match the established import-helper command observer.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const shapes = [
 ["using-object-pattern", "using r = source; export let {x,y} = source;"],
 ["using-array-pattern", "using r = source; export let [p,q] = source;"],
 ["using-nested-rest-pattern", "using r = source; export let {a: {b}, ...rest} = source;"],
 ["using-local-pattern", "using r = source; let {m} = source; source(m); export {};"],
 ["namespace-first", "namespace N { export const x = 1; } export {N};"],
 ["namespace-ambient", "declare const ambient: any; export namespace N { export const x = 1; }"],
 ["enum-first", "export enum E { A }"],
 ["namespace-doc", "/** namespace doc */\nexport namespace N { export const x = 1; }"],
 ["capture-this", "export const f = () => this;"],
 ["capture-this-and-pattern", "export const f = () => this; export let [a] = source;"],
 ["optional-property-comment", "export const v = source /*a*/ ?. /*b*/ x;"],
 ["optional-element-before-comment", "export const v = source /*a*/ ?. /*b*/ ['x'];"],
 ["optional-element-inside-comment", "export const v = source ?.[ /*b*/ 'x' /*c*/ ];"],
 ["optional-call-comment", "export const v = source /*a*/ ?. /*b*/ ();"],
 ["optional-property-clean", "export const v = source?.x;"],
 ["optional-element-clean", "export const v = source?.['x'];"],
 ["optional-call-clean", "export const v = source?.();"],
 ["optional-property-before-newline", "export const v = source\n /*a*/ ?.x;"],
 ["optional-property-after-newline", "export const v = source?.\n /*b*/ x;"],
 ["optional-element-newline", "export const v = source\n /*a*/ ?.[ /*b*/ 'x' ];"],
 ["optional-call-newline", "export const v = source\n /*a*/ ?.\n /*b*/ ();"],
 ["optional-element-empty-comment", "export const v = source?. /**/ [/**/ 'x'];"],
 ["using-declarations-comments", "export using /*c*/ r = source, /*d*/ s = source;"],

 ["destructure-rest-only", "export let x: any; function g(o: any) { ({...x} = o); } source(g);"],
 ["destructure-alias-only", "let z; export {z as w}; function g(o: any) { [z] = o; } source(g);"],
 ["destructure-mixed-alias", "let z; export {z as w}; export let x; [x,z] = source;"],
 ["destructure-for-of-head", "export let x; for ([x] of source) { source(x); }"],
 ["destructure-chained", "export let x,y; [x] = [y] = source;"],
 ["destructure-call-value", "export let x; source([x] = source());"],
 ["import-destructure", "import {value} from \"./dep\"; export {value}; [value] = source;"],
 ["destructure-property-target", "export let x; [source.p,x] = source;"],
 ["destructure-postfix-default", "export let x,y=0; [x = y++] = source;"],
 ["destructure-local-temporaries", "export let x=0,y=0; function g(o: any) { const v = x++; [x,y] = o; return [v,([x] = o)]; } source(g);"],
 ["destructure-swap", "export let x: any=0; let a: any; [x,a] = [a,x];"],
 ["destructure-key", "export let x; ({[source()]:x} = source);"],
 ["destructure-comments", "export let x; [/* first */ x] = source; ({ /* second */ x } = source);"],
 ["scope-comma-update", "export let x=0; const f = () => { (source(), x++); }; source(f);"],

 ["scope-export-vs-local", "export let x = 0; export function f() { return x++; } function g() { return x++; } source(f,g);"],
 ["scope-parameter-patterns", "export let x = 0; const h = function (a = x++, {b} = {}, c = 1, ...r) { return [a,b,c,r,x++]; }; source(h);"],
 ["scope-computed-getter", "export let x = 0; class C { get [x++]() { return x++; } } source(C);"],
 ["scope-object-method", "export let x = 0; const o = { [x++]: 1, m() { return x++; } }; source(o);"],
 ["scope-static-block", "export let x = 0; class C { static { source(x++); } } source(C);"],
 ["scope-field", "export let x = 0; class C { p = x++; } source(C);"],
 ["scope-local-collision", "export let x = 0; function g() { const _a = 1; return [_a,x++]; } source(g);"],
 ["scope-discarded", "export let x = 0; const f = () => { x++; (x++); for (x++; source; x++) { x++; } }; const g = () => void x++; source(f,g);"],
 ["scope-earlier-temp", "export let x = 0; const g = (o: any) => o.value ?? x++; source(g);"],
 ["scope-module-shadow", "export let x = 0; const v = x++; const g = () => x++; source(v,g);"],
 ["scope-default-export", "export let x = 0; export default function () { return x++; }"],
 ["scope-prologue-function", "export let x = 0; function g() { \"use strict\"; function i() {} return [i,x++]; } source(g);"],
 ["destructure-nested-array", "export let x = 0, y = 0; function g(o: any) { [x,y] = o; } source(g);"],
 ["destructure-nested-object", "export let x = 0, y = 0; function g(o: any) { ({a:x,b:y=1} = o); } source(g);"],
 ["destructure-value", "export let x = 0, y = 0; const g = (o: any) => ([x,y] = o); source(g);"],
 ["destructure-shadow", "export let x = 0; const g = (o: any) => { let x; [x] = o; return x; }; source(g);"],
 ["destructure-computed", "export let x = 0, y = 0; const g = (o: any) => ({[source()]:x,...y} = o); source(g);"],
 ["destructure-alias", "export let x = 0, y = 0; export {x as a,y as b}; const g = (o: any) => ([x,y] = o); source(g);"],
 ["destructure-rest", "export let x = 0, y = 0; function g(o: any) { [x,...y] = o; } source(g);"],
 ["destructure-collision", "export let x: any = 0; function g() { [x] = x; } source(g);"],

 ["scope-siblings", "export let x = 0; const g = () => x++; const h = function () { return x++; }; function k() { return x++; } source(g, h, k);"],
 ["scope-nested", "export let x = 0; function outer() { function inner() { return x++; } return [x++, inner, () => x++]; } source(outer);"],
 ["scope-parameters", "export let x = 0; const f = (p = x++) => p + x++; source(f);"],
 ["scope-methods", "export let x = 0; export class C { [x++]() { return x++; } get value() { return x++; } set value(v: number) { source(x++); } constructor(p = x++) { source(x++); } }"],
 ["scope-collision", "export let x = 0; const _a = source; const f = () => [_a, x++]; const g = () => x++; source(f, g);"],
 ["scope-prologue", 'export let x = 0; const f = function () { "use strict"; /* head */ return x++; }; source(f);'],
 ["scope-generator", "export let x = 0; const g = function* () { yield x++; }; source(g);"],
 ["scope-async", "export let x = 0; const f = async () => x++; source(f);"],
 ["switch-case-postfix", "export let x = 0; switch (x++) { case x++: source(x); }"],
 ["for-of-postfix", "export let x = 0; for (const a of x++ as any) source(a);"],
 ["mixed-module-temps", "export let { x = 1 } = source; const value = x++; source(value);"],
 ["scope-top-function-parameters", "export let x = 0; export function f(p = x++) { return p + x++; }"],
 ["scope-shadow", "export let x = 0; const f = (x = 1) => x++; const g = function () { let x = 2; return x++; }; source(f, g);"],
 ["scope-arrow-object", "export let x = 0; const f = () => ({ value: x++ }); source(f);"],
 ["import-named-update", 'import { value } from "./dep"; export { value as exposed }; value = 1; const v = value++; source(v);'],
 ["import-namespace-update", 'import * as ns from "./dep"; export { ns }; ns = source; const v = ns++; source(v);'],
 ["import-default-update", 'import value from "./dep"; export { value }; value = 1; const v = value++; source(v);'],
 ["import-equals-alias-update", 'export import a = require("./dep"); export { a as b }; a = source; const v = a++; source(v);'],
 ["import-equals-export", 'export import a = require("./dep"); a = source;'],
 ["import-named-reexport", 'import { value } from "./dep"; export { value as exposed }; value = source;'],
 ["import-namespace-reexport", 'import * as ns from "./dep"; export { ns }; ns = source;'],
 ["import-default-reexport", 'import value from "./dep"; export { value }; value = source;'],
 ["ambient-export", "export declare let x: any; x = 1; x++;"],
 ["escaped-export", String.raw`export let \u0061 = 1; \u0061 = 2; export { \u0061 as b };`],
 ["postfix-value", "export let x = 0; const v = x++; source(v);"],
 ["postfix-alias-before", "export { x as y }; export let x = 0; const v = x++; source(v);"],
 ["postfix-alias-after", "export let x = 0; export { x as y }; const v = x++; source(v);"],
 ["postfix-call", "export let x = 1; source(x--, x++);"],
 ["postfix-comments", "export let x = 1; const v = x /* a */ ++ /* b */; source(v);"],
 ["postfix-object", "export let x = 0; source({ value: x++ });"],
 ["postfix-return", "export let x = 0; export function f() { return x++; }"],
 ["compound-pattern", "export let x = 0; export { x as y }; x += 1; [x] = [2];"],
 ["nested-enum", "export {}; { enum E { A } source(E.A); }"],
 ["nested-namespace", "export {}; { namespace N { export const k = 1; } source(N.k); }"],
 ["nested-class", "export {}; { class C {} source(C); }"],
 ["for-of-pattern", "export {}; for (const [a] of source) source(a);"],
 ["merged-export", "var x; export var x = 1; x = 2; export { x as y };"],
 ["export-import-assignment", "declare namespace N { const x: number; } export import a = N; a = N;"],
 ["namespace-member-update", "export namespace N { export let x = 0; export function f() { return x++; } }"],
];
const inputs = [];
for (const target of ["es5", "es2015", "esnext"])
  for (const removeComments of [false, true])
    for (const [shape, text] of shapes) {
      const main = "/project/main.ts";
      inputs.push({case_id: `system-binding-publication/${target}/remove-${removeComments}/${shape}`,
        roots: [main], files: [{path: main, text: (["namespace-first", "enum-first", "namespace-doc"].includes(shape) ? "" : "declare const source: any;\n") + text + "\n"}, ...(shape.startsWith("import-") ? [{path: "/project/dep.ts", text: "export let value: any; export default value;\n"}] : [])], options: {},
        config: JSON.stringify({compilerOptions: {target, module: "system", removeComments, lib: ["esnext"],
          strict: false, skipDefaultLibCheck: true, noErrorTruncation: true, sourceMap: true,
          ignoreDeprecations: "6.0", outDir: "/project/out"}, files: [main.slice(9)]})});
    }
assert.equal(inputs.length, 576);
for (const target of ["es5", "es2015"])
  for (const removeComments of [false, true])
    for (const [shape] of shapes.filter(([name]) => name.startsWith("import-") || name === "export-import-assignment")) {
      const input = structuredClone(inputs.find(row => row.case_id === `system-binding-publication/${target}/remove-${removeComments}/${shape}`));
      input.case_id += "/commonjs-control";
      const config = JSON.parse(input.config); config.compilerOptions.module = "commonjs";
      input.config = JSON.stringify(config); inputs.push(input);
    }
assert.equal(inputs.length, 616);
for (const remove of [false, true]) {
  const input = structuredClone(inputs.find(row => row.case_id === `system-binding-publication/es2015/remove-${remove}/scope-earlier-temp`));
  input.case_id = input.case_id.replace("/es2015/", "/es2019/");
  const config = JSON.parse(input.config); config.compilerOptions.target = "es2019";
  input.config = JSON.stringify(config); inputs.push(input);
}
assert.equal(inputs.length, 618);
for (const target of ["es2015", "esnext"])
  for (const importHelpers of [false, true])
    for (const [shape, text, read] of [
      ["late-read-binding", "export let [x] = source;", true],
      ["late-rest-binding", "export let {x,...rest} = source;", false],
      ["late-read-assignment", "export let x; [x] = source;", true],
      ["late-rest-assignment", "export let x,rest; ({x,...rest} = source);", false],
    ])
      for (const secondFile of [false, true]) {
        const main = "/project/main.ts", after = "/project/after.ts";
        const roots = secondFile ? [main, after] : [main];
        inputs.push({case_id: `system-binding-publication/${target}/helpers-${importHelpers}/${shape}/second-${secondFile}`,
          roots, files: [{path: main, text: "declare const source: any;\n" + text + "\n"}, ...(secondFile ? [{path: after, text: "export const after = 1;\n"}] : [])], options: {},
          config: JSON.stringify({compilerOptions: {target, module: "system", importHelpers, downlevelIteration: read,
            lib: ["esnext"], strict: false, skipDefaultLibCheck: true, noErrorTruncation: true, sourceMap: true,
            ignoreDeprecations: "6.0", outDir: "/project/out"}, files: roots.map(root => root.slice(9))})});
      }
assert.equal(inputs.length, 650);
for (const target of ["es5", "es2015"])
  for (const remove of [false, true]) {
    const input = structuredClone(inputs.find(row => row.case_id === `system-binding-publication/${target}/remove-${remove}/using-object-pattern`));
    input.case_id += "/commonjs-control";
    const config = JSON.parse(input.config); config.compilerOptions.module = "commonjs";
    input.config = JSON.stringify(config); inputs.push(input);
  }
for (const remove of [false, true]) {
  const input = structuredClone(inputs.find(row => row.case_id === `system-binding-publication/es5/remove-${remove}/capture-this`));
  input.case_id += "/import-helpers-control";
  const config = JSON.parse(input.config); config.compilerOptions.importHelpers = true;
  input.config = JSON.stringify(config); inputs.push(input);
}
assert.equal(inputs.length, 656);
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
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  return {...input, typescript_observation: first};
});
const artifact = {version: 1, typescript: ts.version, repetitions: 2,
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases};
const destination = path.join(root, "crates/compiler/tests/fixtures/system-binding-publication.json");
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered, {flag: "wx"});
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`System binding publication: ${cases.length} cases, two identical complete observations each`);
