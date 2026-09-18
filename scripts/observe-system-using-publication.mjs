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
 ["direct-using-alias", "export using r = source; export { r as s };"],
 ["direct-await-using-alias", "export await using r = source; export { r as s };"],
 ["multiple-using", "export using a = source, b = source;"],
 ["inner-using-shadow", "export let r = 1; { using r = source; source(r); } r = 2;"],
 ["nested-using", "export {}; { using r = source; source(r); }"],
 ["nested-await-using", "export {}; { await using r = source; source(r); }"],
 ["const-pattern", "export {}; { const { a } = source; source(a); }"],
 ["for-of-const", "export {}; for (const x of source) source(x);"],
 ["for-in-let", "export {}; for (let x in source) source(x);"],
 ["loop-capture", "export {}; { for (let x = 0; x < 2; x++) { source(() => x); } }"],
 ["switch-let", "export {}; switch (source) { case 1: let x = source; source(x); break; }"],
 ["top-let-alias", "let a = 1; export { a }; a = 2;"],
 ["alias-before-direct", "export { y as z }; export let y = 1; y = 2; ++y; y++;"],
 ["alias-after-direct", "export let y = 1; export { y as z }; y = 2; ++y; y++;"],
 ["namespace-export", "export namespace N { export let y = 1; y = 2; }"],
 ["nested-let", "export {}; { let x = 1; source(x); }"],
 ["nested-const", "export {}; { const x = source; source(x); }"],
 ["for-let", "export {}; for (let x = 0; x < 2; x++) source(x);"],
 ["for-var", "export {}; for (var x = 0; x < 2; x++) source(x);"],
 ["try-let", "export {}; try { let x = 1; source(x); } finally {}"],
 ["catch-binding", "export {}; try { source(); } catch (e) { let x = e; source(x); }"],
 ["top-assignment", "export let y = 1; y = 2;"],
 ["block-assignment", "export let y = 1; { y = 2; }"],
 ["try-assignment", "export let y = 1; try { y = 2; } finally {}"],
 ["direct-using", "export using r = source;"],
 ["direct-await-using", "export await using r = source;"],
 ["using-alias", "using r = source; export { r as s };"],
 ["await-using-alias", "await using r = source; export { r as s };"],
 ["local-await-using", "export {}; await using r = source;"],
 ["shadowed-assignment", "export let r = 0; using resource = source; { let r = 1; r = 2; } r = 3;"],
 ["export-after-using", "using resource = source; export let x = 1; x = 2;"],
 ["pattern-after-using", "using resource = source; export let { x, y } = source; x = 1;"],
 ["await-using-in-function", "export async function f() { await using r = source; }"],
];
const inputs = [];
for (const target of ["es5", "es2015", "esnext"])
  for (const removeComments of [false, true])
    for (const [shape, text] of shapes) {
      const main = "/project/main.ts";
      inputs.push({case_id: `system-using-publication/${target}/remove-${removeComments}/${shape}`,
        roots: [main], files: [{path: main, text: "declare const source: any;\n" + text + "\n"}], options: {},
        config: JSON.stringify({compilerOptions: {target, module: "system", removeComments, lib: ["esnext"],
          strict: false, skipDefaultLibCheck: true, noErrorTruncation: true, sourceMap: true,
          ignoreDeprecations: "6.0", outDir: "/project/out"}, files: [main.slice(9)]})});
    }
assert.equal(inputs.length, 198);
for (const target of ["es5", "es2015"])
  for (const removeComments of [false, true])
    for (const shape of ["direct-using", "direct-await-using"]) {
      const input = structuredClone(inputs.find(row => row.case_id === `system-using-publication/${target}/remove-${removeComments}/${shape}`));
      input.case_id += "/commonjs-control";
      const config = JSON.parse(input.config); config.compilerOptions.module = "commonjs";
      input.config = JSON.stringify(config); inputs.push(input);
    }
assert.equal(inputs.length, 206);
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
const destination = path.join(root, "crates/compiler/tests/fixtures/system-using-publication.json");
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered, {flag: "wx"});
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`System using publication: ${cases.length} cases, two identical complete observations each`);
