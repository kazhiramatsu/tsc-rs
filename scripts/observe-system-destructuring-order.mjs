// Complete commands for exported destructuring assignment comment ranges.
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
const prefix = "declare const source: any; declare function get(): any; declare const key: string;\n";
const shapes = [
  ["single-default", "export let {\n/** leaf */\nx = 1 } = source; export {x as y};"],
  ["multiple-defaults", "export let {\n/** x */\nx = 1,\n/** y */\ny = 2} = source;"],
  ["prior-declaration", "export var x; export var {\n/** leaf */\nx = 1} = source;"],
  ["prior-initializer", "export var x = 0; export var {\n/** leaf */\nx = 1} = source;"],
  ["surrounding-names", "export let before = 0; export let {\n/** leaf */\nx = 1} = source; export let after = 2;"],
  ["root-call", "export let {\n/** x */\nx = 1, y = 2} = get();"],
  ["nested-default", "export let {inner: {\n/** leaf */\nx = 1} = {}} = source;"],
  ["array-default", "export let [\n/** leaf */\nx = 1,, y = 2] = source;"],
  ["computed-default", "export let {\n/** leaf */\n[key]: x = 1, y} = source;"],
  ["rest-default", "export let {\n/** leaf */\nx = 1, ...rest} = source;"],
  ["for-initializer", "for (var {\n/** leaf */\nx = 1} = source; x < 2; x++) {} export {x};"],
  ["nested-var", "if (source) { var {\n/** leaf */\nx = 1} = source; } export {x};"],
];
shapes.push(
  ["later-uninitialized", "export let {x = 1} = source; export var later;"],
  ["later-root-temp", "export let before = 0; export let {x, y} = get();"],
  ["later-class", "export let {x = 1} = source; export class C {}"],
  ["mixed-declarations", "export let before = 0, {x, y} = get(), after;"],
  ["import-before", "import {value} from './dep'; export let {x = value} = source;"],
  ["import-after", "export let {x = 1} = source; import {value} from './dep'; export {value};"],
  ["using-after", "export let {x = 1} = source; export using resource = source;"],
  ["for-of-head", "for (var {x = 1} of source) {} export {x};"],
  ["uninitialized-pattern", "export var {x};"],
  ["duplicate-plain", "var x; var x = source; export {x};"],
  ["duplicate-export", "export var x; export var x = source;"],
  ["for-of-plain", "for (var {a} of source) {} export {a};"],
  ["mixed-for", "for (var j = 0, {b = 1} = source; j < 2; j++) {} export {j, b};"],
  ["empty-patterns", "export let {} = source; export let [] = source;"],
  ["nested-sequence", "{ var y = source.y; } if (source) { var {z = 2} = source; } export {y, z};"],
  ["default-class", "export let {x = 1} = source; export default class {}"],
  ["import-namespace-after", "export let {x = 1} = source; import * as ns from './dep'; export {ns};"],
  ["nested-function", "export let {x = 1} = source; export function f() { var {local = 2} = source; return local; }"],
  ["all-omitted", "export let [,] = get();"],
  ["computed-rest", "export let {[key]: x = 1, ...rest} = source;"],
  ["nested-default-call", "export let {inner: {x = 1} = get()} = source;"],
);
const inputs = [];
for (const target of ["es5", "es2015", "esnext"])
  for (const removeComments of [false, true])
    for (const [shape, fragment] of shapes) {
      const main = "/project/main.ts", text = prefix + fragment + "\n";
      assert.equal(ts.createSourceFile(main, text, ts.ScriptTarget.Latest, true).parseDiagnostics.length, 0);
      inputs.push({case_id: `system-destructuring-order/${target}/remove-${removeComments}/${shape}`,
        roots: [main], files: [{path: main, text}, ...(shape.startsWith("import-")
          ? [{path: "/project/dep.ts", text: "export const value = 1;\n"}] : [])], options: {},
        config: JSON.stringify({compilerOptions: {target, module: "system", removeComments,
          strict: false, skipDefaultLibCheck: true, noErrorTruncation: true, sourceMap: true,
          declaration: true, declarationMap: true,
          ignoreDeprecations: "6.0", outDir: "/project/out"}, files: [main.slice(9)]})});
    }
assert.equal(inputs.length, 198);
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
const destination = path.join(root, "crates/compiler/tests/fixtures/system-destructuring-order.json");
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered, {flag: "wx"});
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`System destructuring order: ${cases.length} cases, two identical complete observations each`);
