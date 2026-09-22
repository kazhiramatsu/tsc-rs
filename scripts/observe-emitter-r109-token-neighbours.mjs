// Complete commands for the r109 token grammar and trailing-comment boundaries.
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
function add(group, shape, text, target, module, removeComments, extra = {}) {
  const main = "/project/main.ts";
  inputs.push({case_id: `emitter-r109-token-neighbours/${group}/${target}/${module}/remove-${removeComments}/${shape}`,
    roots: [main], files: [{path: main, text: "declare const a: any, x: any, y: any, key: any; declare function g(): IterableIterator<any>;\n" + text + "\n"}, {path: "/project/dep.ts", text: "export const item: any = 1; export default item;"}], options: {},
    config: JSON.stringify({compilerOptions: {target, module, removeComments, lib: ["esnext"],
      strict: false, skipDefaultLibCheck: true, noErrorTruncation: true, sourceMap: true,
      ignoreDeprecations: "6.0", outDir: "/project/out", ...extra}, files: ["main.ts"]})});
}
const shapes = [];
for (const [mode, gap] of [["same", " /*尾😀*/ "], ["newline", "\n/*行🍀*/ "], ["both", " /*尾😀*/\n/*行🍀*/ "]]) {
  shapes.push([`import-from-${mode}`, `import { item }${gap}from "./dep"; item;`]);
  shapes.push([`export-from-${mode}`, `export { item }${gap}from "./dep";`]);
  shapes.push([`specifier-as-${mode}`, `import { item${gap}as renamed } from "./dep"; renamed;`]);
  shapes.push([`do-block-${mode}`, `do {}${gap}while (y);`]);
  shapes.push([`do-nonblock-${mode}`, `do x();${gap}while (y);`]);
}
shapes.push(
  ["default-from", 'import item /*尾*/ from "./dep"; item;'],
  ["type-from", 'import type { item } /*尾*/ from "./dep"; export {};'],
  ["namespace-from", 'import * as ns /*尾*/ from "./dep"; ns;'],
  ["star-from", 'export * /*尾*/ from "./dep";'],
  ["chain-typeof", 'export const value = typeof a?.b.c;'],
  ["chain-void", 'export const value = void a?.b;'],
  ["chain-statement", 'a?.b.c();'],
  ["chain-new", 'export const value = new (a?.b)();'],
  ["chain-nonnull", 'export const value = a?.b!.c;'],
  ["chain-comments", 'export const value = a /*1*/ ?. /*2*/ b.c;'],
  ["chain-element", 'export const value = a?.[key]?.[key].x;'],
  ["chain-optional-call", 'export const value = a?.b?.(x).c;'],
  ["yield-access", 'export function* gen() { return (yield* g()).x; }'],
  ["yield-comments", 'export function* gen() { yield /*a*/ * /*b*/ x; }'],
);
assert.equal(shapes.length, 29);
for (const remove of [false, true])
  for (const [shape, text] of shapes)
    add("token-neighbours", shape, text, "es2020", "preserve", remove, {verbatimModuleSyntax: true});
assert.equal(inputs.length, 58);

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
const destination = path.join(root, "crates/compiler/tests/fixtures/emitter-r109-token-neighbours.json");
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered, {flag: "wx"});
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`r109 token neighbours: ${cases.length} cases, two identical complete observations each`);
