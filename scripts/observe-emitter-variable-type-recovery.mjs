// Complete commands for the r129 direct variable type-recovery boundaries.
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
const inputs = [];
const shapes = [
  ["export-let-initialized", "export let x: = 1;"],
  ["export-let-uninitialized", "export let x: ;"],
  ["local-module", "let x: = 1; export {};"],
  ["export-const", "export const x: = 1;"],
  ["export-var", "export var x: ;"],
  ["two-types", "export let a: , b: = 2;"],
  ["block-comment", "export let x: /*c*/ = 1;"],
  ["line-comment", "export let x: //c\n = 1;"],
  ["name-comment", "export let x /*n*/: /*t*/ = 1; /*end*/"],
  ["namespace", "export namespace N { export let x: = 1; }"],
  ["script", "let x: = 1;"],
  ["uninitialized-comment", "export let x: /*c*/ ;"],
  ["using", "export {}; declare const f: () => Disposable; { using x: = f(); }"],
  ["await-using", "declare const f: () => AsyncDisposable; export async function h() { await using x: = f(); }"],
  ["ambient", "declare let d: ; export declare let e: ;"],
  ["definite", "export let x!: = 1;"],
  ["generator", "export function* g() { let x: = 1; yield x; }"],
  ["async", "export async function h() { let x: = 1; await 0; return x; }"],
  ["newline-block-comment", "export let x:\n/*c*/ = 1;"],
  ["newline-line-comment", "export let x:\n//c\n = 1;"],
  ["mixed-type-comments", "export let x: /*a*/\n/*b*/ = 1;"],
  ["initializer-comment", "export let x: = /*v*/ 1;"],
  ["unicode-name-comment", "export let é /*n*/: /*t*/ = 1;"],
  ["two-local-comments", "let x: /*c*/ = 1, y: /*d*/ = 2; export {};"],
  ["valid-type-comment", "export let x: number /*c*/ = 1;"],
  ["valid-type-newline-comment", "export let x: number\n/*c*/ = 1;"],
  ["for-await-container-newline", "export async function f(xs: AsyncIterable<{ x: number }>) { for await (const {\n/*c*/ x\n} of xs) {} }"],
  ["valid-type-line-comment", "export let x: number //c\n = 1;"],
  ["valid-type-mixed-comments", "export let x: number /*a*/\n/*b*/ = 1;"],
  ["valid-type-several-comments", "export let x: number /*a*/ //b\n/*c*/ = 1;"],
  ["untyped-newline-comment", "export let x\n/*c*/ = 1;"],
  ["untyped-inline-comment", "export let x /*c*/ = 1;"],
  ["untyped-line-comment", "export let x //c\n = 1;"],
  ["untyped-mixed-comments", "export let x /*a*/\n/*b*/ = 1;"],
  ["name-valid-type-newline", "export let x /*n*/: number\n/*c*/ = 1;"],
  ["declaration-literal-comments", "export const z /** n */\n/** c */ = 1;"],
  ["es5-block-rename", "let x = 1; { let x: number /*a*/\n/*c*/ = 2; (() => x)(); } export {};"],
  ["es5-converted-loop", "export function f() { for (let x = 0; x < 2; x++) { let y\n/*c*/ = () => x; y(); } }"],
  ["multiple-declaration-comments", "export let a /*x*/ = 1, b\n/*y*/ = 2;"],
  ["for-of-binding-comments", "export function f(arr: number[]) { for (const x /*a*/\n/*b*/ of arr) { (() => x)(); } }"],
];
const configurations = [
  ["es5", "commonjs", false],
  ["es5", "system", true],
  ["es2022", "esnext", false],
  ["es2022", "commonjs", true],
];
const commentShapes = new Set(["block-comment", "line-comment", "name-comment", "uninitialized-comment",
  "newline-block-comment", "newline-line-comment", "mixed-type-comments", "initializer-comment",
  "unicode-name-comment", "two-local-comments", "valid-type-comment", "valid-type-newline-comment",
  "for-await-container-newline", "valid-type-line-comment", "valid-type-mixed-comments", "valid-type-several-comments", "untyped-newline-comment", "untyped-inline-comment", "untyped-line-comment", "untyped-mixed-comments", "name-valid-type-newline", "declaration-literal-comments", "es5-block-rename", "es5-converted-loop", "multiple-declaration-comments", "for-of-binding-comments"]);
for (const [target, module, noCheck] of configurations)
  for (const [shape, text] of shapes)
    for (const removeComments of commentShapes.has(shape) ? [false, true] : [false]) {
      const main = "/project/main.ts";
      inputs.push({case_id: `emitter-r129-variable-type-controls/${target}/${module}/nocheck-${noCheck}/remove-${removeComments}/${shape}`,
        roots: [main], files: [{path: main, text: text + "\n"}], options: {},
        config: JSON.stringify({compilerOptions: {target, module, noCheck, removeComments,
          lib: ["esnext"], strict: false, skipDefaultLibCheck: true, noErrorTruncation: true,
          sourceMap: true, declaration: true, declarationMap: true, ignoreDeprecations: "6.0",
          outDir: "/project/out"}, files: ["main.ts"]})});
    }
for (const [target, module, noCheck] of configurations) {
  const main = "/project/main.js";
  inputs.push({case_id: `emitter-r129-variable-type-controls/${target}/${module}/nocheck-${noCheck}/javascript`,
    roots: [main], files: [{path: main, text: "export let x: = 1;\n"}], options: {},
    config: JSON.stringify({compilerOptions: {target, module, noCheck, allowJs: true,
      lib: ["esnext"], strict: false, skipDefaultLibCheck: true, noErrorTruncation: true,
      sourceMap: true, declaration: true, declarationMap: true, ignoreDeprecations: "6.0",
      outDir: "/project/out"}, files: ["main.js"]})});
}
const es5ProducerShapes = [
  ["rename-export-alias", "let x = 1; export { x }; { let x /*a*/ = 2; (() => x)(); }"],
  ["rename-untyped-mixed", "let x = 1; { let x /*a*/\n/*b*/ = 2; (() => x)(); } export {};"],
  ["for-of-pattern-assignment", "export function f(arr: number[][]) { let x; for ([x] /*a*/ of arr) { x; } }"],
  ["rename-untyped", "let x = 1; { let x = 2; (() => x)(); } export {};"],
  ["rename-untyped-inline", "let x = 1; { let x /*n*/ = 2; (() => x)(); } export {};"],
  ["rename-typed-inline", "let x = 1; { let x: number /*t*/ = 2; (() => x)(); } export {};"],
  ["rename-typed-newline", "let x = 1; { let x: number\n/*t*/ = 2; (() => x)(); } export {};"],
  ["rename-name-and-type", "let x = 1; { let x /*n*/: number /*t*/\n/*next*/ = 2; (() => x)(); } export {};"],
  ["rename-shadow-export", "export let x = 1; { let x /*n*/ = 2; (() => x)(); }"],
  ["rename-destructure", "let x = 1; { let {a: x /*n*/} = {a: 2}; (() => x)(); } export {};"],
  ["for-of-inline", "export function f(arr: number[]) { for (const x /*a*/ of arr) { x; } }"],
  ["for-of-newline", "export function f(arr: number[]) { for (const x\n/*a*/ of arr) { x; } }"],
  ["for-of-leading", "export function f(arr: number[]) { for (/*l*/ const x of arr) { x; } }"],
  ["for-of-assignment", "export function f(arr: number[]) { let x; for (x /*a*/ of arr) { x; } }"],
  ["for-of-pattern", "export function f(arr: number[][]) { for (const [x] /*a*/ of arr) { (() => x)(); } }"],
  ["for-of-unicode", "export function f(arr: number[]) { for (const é /*a*/ of arr) { (() => é)(); } }"],
  ["for-of-escaped-name", "export function f(arr: number[]) { for (const \\u{61} /*a*/ of arr) { (() => a)(); } }"],
];
for (const module of ["commonjs", "system"])
  for (const noCheck of [false, true])
    for (const removeComments of [false, true])
      for (const [shape, text] of es5ProducerShapes) {
        const main = "/project/main.ts";
        inputs.push({case_id: `emitter-r154-variable-producers/es5/${module}/nocheck-${noCheck}/remove-${removeComments}/${shape}`,
          roots: [main], files: [{path: main, text: text + "\n"}], options: {},
          config: JSON.stringify({compilerOptions: {target: "es5", module, noCheck, removeComments,
            lib: ["esnext"], strict: false, skipDefaultLibCheck: true, noErrorTruncation: true,
            sourceMap: true, declaration: true, declarationMap: true, ignoreDeprecations: "6.0",
            outDir: "/project/out"}, files: ["main.ts"]})});
      }
assert.equal(inputs.length, 404);

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
const destination = path.join(root, "crates/compiler/tests/fixtures/emitter-r129-variable-type-controls.json");
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered, {flag: "wx"});
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`r129 variable-type controls: ${cases.length} cases, two identical complete observations each`);
