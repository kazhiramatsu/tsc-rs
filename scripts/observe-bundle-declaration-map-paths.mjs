// Bundle declaration map root/path facets; full TS emit observations, no admission changes.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const observerPath = "scripts/observe-bundle-declaration-map-paths.mjs";
const fixturePath = "crates/emitter/tests/fixtures/bundle-declaration-map-paths.json";
const sha256 = value => crypto.createHash("sha256").update(value).digest("hex");
const identity = name => ({ path: name, sha256: sha256(fs.readFileSync(path.join(root, name))) });
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
assert.ok(["--write", "--check"].includes(process.argv[2]));
const options = { target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.None, outFile: "/project/dist/bundle.js",
  declaration: true, declarationMap: true, emitDeclarationOnly: true, sourceMap: false, strict: false,
  newLine: ts.NewLineKind.LineFeed, noErrorTruncation: true, skipDefaultLibCheck: true, listEmittedFiles: true };
const files = [
  { path: "/project/src/nested/a.ts", text: '/** 文😀 */\nconst a: string = "文😀";\n' },
  { path: "/project/src/deep/b.ts", text: 'const b: number = 2;\n' },
];
const variants = [
  ["default", {}],
  ["source-root-relative", { sourceRoot: "../source root" }],
  ["source-root-url", { sourceRoot: "https://sources.test/源" }],
  ["map-root-relative", { mapRoot: "maps/nested" }],
  ["map-root-absolute", { mapRoot: "/external maps" }],
  ["map-root-url", { mapRoot: "https://maps.test/mapped path" }],
  ["source-and-map-roots", { sourceRoot: "https://sources.test/src", mapRoot: "maps/nested" }],
  ["inline-sources", { inlineSources: true }],
  ["inline-map", { inlineSourceMap: true }],
  ["relative-unicode-bom", { outFile: "dist/名 bundle.js", newLine: ts.NewLineKind.CarriageReturnLineFeed, emitBOM: true }],
];
const inputs = variants.map(([name, extra]) => ({ case_id: "bundle-declaration-map-path/" + name,
  current_directory: "/project", use_case_sensitive_file_names: true, files, options: { ...options, ...extra } }));
function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function resultRecord(result) {
  return { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
    emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps?.map(entry => ({
      input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap) })) ?? null };
}
function writeRecord(args, index) {
  const [fileName, text, bom, onError, sourceFiles, data] = args;
  assert.ok(!text.includes(root));
  assert.ok(data === undefined || Object.keys(data).every(key => ["sourceMapUrlPos", "diagnostics", "buildInfo"].includes(key)));
  const bytes = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239,187,191]), bytes]) : bytes;
  return { index, path: fileName, kind: ts.isDeclarationFileName(fileName) ? "declaration" : fileName.endsWith(".map") ? "source-map" : "javascript",
    callback_utf8_base64: bytes.toString("base64"), callback_utf8_bytes: bytes.length, write_byte_order_mark: bom,
    materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sourceFiles?.map(file => file.fileName) ?? null,
    data_present: data !== undefined, data_keys: data === undefined ? null : Object.keys(data),
    data_source_map_url_pos: data?.sourceMapUrlPos ?? null, data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null,
    data_build_info: data?.buildInfo ?? null };
}
function makeProgram(input) {
  const cwd = input.current_directory;
  const canonical = name => { const absolute = ts.getNormalizedAbsolutePath(name, cwd); return input.use_case_sensitive_file_names ? absolute : absolute.toLowerCase(); };
  const files = new Map(input.files.map(file => [canonical(file.path), file.text]));
  const libraryRoot = path.join(root, "vendor/typescript-6.0.3/lib");
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name) && fs.existsSync(path.join(libraryRoot, path.basename(name)));
  const read = name => files.get(canonical(name)) ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(input.files.map(file => file.path), { currentDirectory: cwd,
    useCaseSensitiveFileNames: input.use_case_sensitive_file_names,
    fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  const host = { ...ts.createCompilerHost(input.options, true), ...overlay, getCurrentDirectory: () => cwd,
    getCanonicalFileName: canonical, useCaseSensitiveFileNames: () => input.use_case_sensitive_file_names,
    getDefaultLibFileName: () => "/lib/" + ts.getDefaultLibFileName(input.options), getDefaultLibLocation: () => "/lib",
    readFile: read, fileExists: name => read(name) !== undefined, realpath: ts.normalizePath,
    writeFile() { assert.fail("unrecorded write"); },
    getSourceFile(name, version) { const text = read(name); return text === undefined ? undefined
      : ts.createSourceFile(name, text, version, true, ts.getScriptKindFromFileName(name)); } };
  const program = ts.createProgram(input.roots ?? input.files.map(file => file.path), input.options, host);
  for (const file of program.getSourceFiles()) assert.ok(files.has(canonical(file.fileName)) || library(file.fileName), file.fileName);
  return { program, library };
}
function observe(input) {
  const { program, library } = makeProgram(input);
  const writes = [];
  let result = null, exception = null;
  try { result = resultRecord(program.emit(undefined, (...args) => writes.push(writeRecord(args, writes.length)))); }
  catch (error) { exception = { name: error.name, message: error.message }; }
  return { program_source_order: program.getSourceFiles().filter(file => !library(file.fileName)).map(file => file.fileName),
    common_source_directory: program.getCommonSourceDirectory(), writes, emit_result: result, exception,
    options_diagnostics: program.getOptionsDiagnostics().map(diagnostic),
    syntactic_diagnostics: program.getSyntacticDiagnostics().map(diagnostic),
    global_diagnostics: program.getGlobalDiagnostics().map(diagnostic),
    semantic_diagnostics: program.getSemanticDiagnostics().map(diagnostic) };
}
const cases = inputs.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  assert.equal(first.exception, null);
  assert.equal(first.writes.length, 2);
  assert.deepEqual(first.writes.map(write => write.kind), ["source-map", "declaration"]);
  assert.equal(first.emit_result.source_maps.length, 1);
  for (const write of first.writes) {
    assert.equal(Buffer.from(write.callback_utf8_base64, "base64").length, write.callback_utf8_bytes);
    assert.equal(Buffer.from(write.materialized_utf8_base64, "base64").length, write.materialized_utf8_bytes);
    assert.deepEqual(write.source_files, input.files.map(file => file.path));
  }
  return { ...input, input_sha256: sha256(JSON.stringify(input)), typescript_observation: first };
});
const artifact = { schema: 1, kind: "bundle-declaration-map-paths", status: "internal-path-reference", typescript: ts.version,
  source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8", repetitions: 2, observer: identity(observerPath),
  inputs: ["vendor/typescript-6.0.3/lib/typescript.js", "vendor/typescript-6.0.3/lib/_tsc.js", "crates/oracle/vfs-directory-overlay.mjs", ".node-version"].map(identity),
  contract: "Complete ordinary Program.emit tuples on two fresh Programs. Rust root/URL lane replay is a helper facet; it does not assert printer mapping positions, public bundle admission or sourceRoot/mapRoot option admission.",
  cases, summary: { sequences: cases.length, writes_per_repetition: cases.reduce((sum, row) => sum + row.typescript_observation.writes.length, 0), runtime_admitted: 0 } };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
assert.ok(!rendered.includes(root));
if (process.argv[2] === "--write") fs.writeFileSync(path.join(root, fixturePath), rendered);
else assert.equal(fs.readFileSync(path.join(root, fixturePath), "utf8"), rendered, "Bundle declaration map paths fixture is stale");
console.log(JSON.stringify(artifact.summary));
