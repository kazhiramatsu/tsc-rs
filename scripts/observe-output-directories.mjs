// H2.8a direct Program output-directory witnesses. Expectations are TS-produced.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/output-directories.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const defaults = { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.CommonJS,
  strict: true, skipDefaultLibCheck: true, noErrorTruncation: true, newLine: ts.NewLineKind.CarriageReturnLineFeed };
const inputs = [];
const nested = { "src/a.ts": "export const a: number = 1;\n", "src/nested/b.ts": "export const b: string = 'b';\n" };
function add(case_id, files, options = {}) {
  inputs.push({ case_id, files: Object.entries(files).map(([name, text]) => ({ path: "/project/" + name, text })), options: { ...defaults, ...options } });
}
for (const outDir of ["/project/out", "out", "./out", "out/../dist", "../dist", ".", "", "/", "out/"]) {
  add("directory/" + outDir, nested, { outDir });
}
add("directory/absent", nested);
for (const declaration of [false, true]) for (const emitDeclarationOnly of [false, true]) {
  add(`products/${declaration}/${emitDeclarationOnly}`, nested,
    { outDir: "out", declarationDir: "types", declaration, emitDeclarationOnly, listEmittedFiles: true });
}
for (const emitBOM of [false, true]) for (const newLine of [0, 1]) for (const removeComments of [false, true]) {
  add(`text/${emitBOM}/${newLine}/${removeComments}`, { "src/名.ts": "#!/usr/bin/env node\n/** 文😀 */\nexport const 名: string = '😀'; // trailing\n" },
    { outDir: "./out", emitBOM, newLine, removeComments, declaration: true });
}
for (const noEmitOnError of [false, true]) {
  add(`collision/input/${noEmitOnError}`, { "src/a.ts": nested["src/a.ts"], "src/a.js": "exports.original = 1;\n" },
    { outDir: "src", allowJs: true, noEmitOnError, listEmittedFiles: true });
  add(`collision/multiple/${noEmitOnError}`, { "src/a.ts": nested["src/a.ts"], "src/a.tsx": "export const b = 2;\n" },
    { outDir: "out", noEmitOnError });
  add(`collision/declaration/${noEmitOnError}`, { "src/a.ts": nested["src/a.ts"], "out/a.d.ts": "export declare const original: string;\n" },
    { outDir: "out", declaration: true, noEmitOnError });
  add(`diagnostic/semantic/${noEmitOnError}`, { "src/a.ts": "export const a: number = '';\n" }, { outDir: "out", noEmitOnError });
}
add("directory/ignored-declaration-root", { ...nested, "external/types.d.ts": "export declare const external: number;\n" }, { outDir: "out", listEmittedFiles: true });
for (const outDir of ["", "./out"]) add("javascript/" + outDir, { "src/a.js": "exports.a = 1;\n" }, { outDir, allowJs: true });
add("directory/no-emit-sources", { "external/types.d.ts": "export declare const a: number;\n" }, { outDir: "out", listEmittedFiles: true });

function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [name, text, bom, onError, sources, data] = args;
  const bytes = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), bytes]) : bytes;
  return { index, path: name, kind: ts.isDeclarationFileName(name) ? "declaration" : "javascript",
    callback_utf8_base64: bytes.toString("base64"), callback_utf8_bytes: bytes.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sources?.map(source => source.fileName) ?? null,
    data_present: data !== undefined, data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null };
}
function observe(input) {
  const files = new Map(input.files.map(file => [file.path, file.text]));
  const libraryRoot = path.join(root, "vendor/typescript-6.0.3/lib");
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name) && fs.existsSync(path.join(libraryRoot, path.basename(name)));
  const read = name => files.get(ts.normalizePath(name)) ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(files.keys(), { currentDirectory: "/project", useCaseSensitiveFileNames: true,
    fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  const host = { ...ts.createCompilerHost(input.options, true), ...overlay,
    getCurrentDirectory: () => "/project", getDefaultLibFileName: options => "/lib/" + ts.getDefaultLibFileName(options),
    getDefaultLibLocation: () => "/lib", useCaseSensitiveFileNames: () => true, getCanonicalFileName: name => name,
    readFile: read, fileExists: name => files.has(ts.normalizePath(name)) || library(name),
    getSourceFile: (name, options) => { const text = read(name); return text === undefined ? undefined : ts.createSourceFile(name, text, options, true); },
    writeFile: () => assert.fail("unexpected host write") };
  const program = ts.createProgram({ rootNames: input.files.map(file => file.path), options: input.options, host });
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)), s => status.push(s), undefined,
    (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  assert.equal(result.sourceMaps, undefined);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: null },
    status_writes: status, exit_code: exit };
}
const cases = inputs.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  return { ...input, typescript_observation: first };
});
const artifact = { version: 1, typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), repetitions: 2, cases };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered);
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`output directories: ${cases.length} cases, two identical complete observations each`);
