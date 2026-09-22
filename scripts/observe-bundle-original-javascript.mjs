// Fresh two-pass checks for the four original JavaScript bundle recorder inputs.
// The observation helpers below retain the pinned h2-7de-observations.mjs bodies.
// Existing input and observation ratchets are read-only; --write creates only
// the bounded selection/identity manifest, never replacement expected output.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
import { root, inputPath, sha256, identity, parseConfig } from "../crates/oracle/h2-7de-candidates.mjs";

const mode = process.argv[2];
assert.ok(["--write", "--check"].includes(mode), "use --write or --check");
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
const read = name => JSON.parse(fs.readFileSync(path.join(root, name)));
const observationPath = "ratchets/h2-7de-observations.v1.json";
const manifestPath = "docs/design/greenfield/slices/witness-coverage/compiler-module-facets/original-javascript-inputs.v2.json";
const inputs = read(inputPath), observations = read(observationPath);
// The predecessor artifact binds its generator, inputs, compiler and host.
for (const dependency of [observations.generator, ...observations.inputs]) {
  assert.deepEqual(identity(dependency.path), dependency, dependency.path);
}
const libraryRoot = ts.normalizePath(path.join(root, "vendor/typescript-6.0.3/lib"));
const isLibrary = name => ts.normalizePath(name).startsWith(libraryRoot + "/");

function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [fileName, text, bom, onError, sourceFiles, data] = args;
  assert.ok(!text.includes(root), "local workspace path escaped into callback bytes");
  assert.ok(data === undefined || Object.keys(data).every(key => ["sourceMapUrlPos", "diagnostics", "buildInfo"].includes(key)), fileName);
  const callback = Buffer.from(text, "utf8"), materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), callback]) : callback;
  return { index, path: fileName, kind: ts.isDeclarationFileName(fileName) ? "declaration"
    : fileName.endsWith(".map") ? "source-map" : fileName.endsWith(".tsbuildinfo") ? "build-info" : "javascript",
    callback_utf8_base64: callback.toString("base64"), callback_utf8_bytes: callback.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sourceFiles?.map(file => file.fileName) ?? null,
    data_present: data !== undefined, data_keys: data === undefined ? null : Object.keys(data),
    data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null, data_build_info: data?.buildInfo ?? null };
}
function sourceMaps(maps) {
  return maps?.map(entry => {
    assert.deepEqual(Object.keys(entry).sort(), ["inputSourceFileNames", "sourceMap"]);
    return { input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap) };
  }) ?? null;
}

function observe(row) {
  const input = row.input, cwd = input.current_directory, caseSensitive = input.use_case_sensitive_file_names;
  const canonical = name => caseSensitive ? ts.getNormalizedAbsolutePath(name, cwd) : ts.getNormalizedAbsolutePath(name, cwd).toLowerCase();
  const originalFiles = [...(input.shared_mount ? inputs.shared_mounts[input.shared_mount] : []), ...input.files];
  if (input.config && !originalFiles.some(file => file.path === input.config.path)) originalFiles.push(input.config);
  const files = new Map(originalFiles.map(file => [canonical(file.path), file.text]));
  const symlinks = new Map(input.vfs_symlinks.map(link => [canonical(link.link_path), link.target_path]));
  for (const link of input.vfs_symlinks) { assert.ok(files.has(canonical(link.target_path))); files.set(canonical(link.link_path), files.get(canonical(link.target_path))); }
  const parsed = input.config ? parseConfig(input.config, originalFiles, cwd, row.suite === "project") : null;
  const options = { ...parsed?.options, ...row.effective_options };
  const base = ts.createCompilerHost(options, true);
  const fallback = { directoryExists: name => isLibrary(name + "/") && base.directoryExists(name),
    getDirectories: name => isLibrary(name + "/") ? base.getDirectories(name) : [] };
  const overlay = createHermeticDirectoryOverlay([...originalFiles.map(file => file.path), ...input.vfs_symlinks.map(link => link.link_path)],
    { currentDirectory: cwd, useCaseSensitiveFileNames: caseSensitive, fallbackHost: fallback });
  const host = { ...base, ...overlay, getCurrentDirectory: () => cwd, useCaseSensitiveFileNames: () => caseSensitive,
    getCanonicalFileName: name => caseSensitive ? name : name.toLowerCase(), trace() {},
    fileExists: name => files.has(canonical(name)) || (isLibrary(name) && base.fileExists(name)),
    readFile: name => files.get(canonical(name)) ?? (isLibrary(name) ? base.readFile(name) : undefined),
    realpath: name => symlinks.get(canonical(name)) ?? ts.normalizePath(name),
    getSourceFile(name, languageVersion) {
      const text = files.get(canonical(name));
      return text === undefined ? (isLibrary(name) ? base.getSourceFile(name, languageVersion) : undefined)
        : ts.createSourceFile(ts.normalizePath(name), text, languageVersion, true, ts.getScriptKindFromFileName(name));
    } };
  if (input.default_library_file_name) host.getDefaultLibFileName = () => ts.combinePaths(libraryRoot, input.default_library_file_name);
  const program = ts.createProgram(input.roots, options, host);
  for (const source of program.getSourceFiles()) assert.ok(files.has(canonical(source.fileName)) || isLibrary(source.fileName), source.fileName);
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)), text => status.push(text), undefined,
    (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  return { program_source_order: program.getSourceFiles().filter(source => !isLibrary(source.fileName)).map(source => source.fileName),
    standard_libraries: program.getSourceFiles().filter(source => isLibrary(source.fileName)).map(source => path.posix.basename(source.fileName)),
    writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      emitted_files: result.emittedFiles ?? null, source_maps: sourceMaps(result.sourceMaps) }, status_writes: status, exit_code: exit };
}


const names = ["jsDeclarationsImportTypeBundled", "jsdocAccessibilityTagsDeclarations",
  "jsdocReadonlyDeclarations", "uniqueSymbolsDeclarationsInJs"];
const selected = names.map(name => {
  const rows = inputs.cases.filter(row => row.case_id.endsWith(`/${name}.ts#default`));
  assert.equal(rows.length, 1, name);
  const row = rows[0];
  assert.equal(row.input.route, "whole-program");
  assert.equal(row.input.config, null);
  assert.equal(row.input.shared_mount, null);
  assert.deepEqual(row.input.vfs_symlinks, []);
  const expected = observations.cases.filter(value => value.case_id === row.case_id);
  assert.equal(expected.length, 1, row.case_id);
  assert.equal(expected[0].repetitions, 2);
  assert.equal(expected[0].input_sha256, sha256(JSON.stringify(row)));
  return {row, expected: expected[0]};
});
const manifest = {
  version: 1, kind: "original-javascript-bundle-selection", typescript: ts.version,
  source_commit: observations.source_commit, repetitions: 2,
  dependencies: [inputPath, observationPath, "crates/oracle/h2-7de-observations.mjs",
    "crates/oracle/h2-7de-candidates.mjs", "crates/oracle/vfs-directory-overlay.mjs",
    "vendor/typescript-6.0.3/lib/typescript.js", "vendor/typescript-6.0.3/lib/_tsc.js", ".node-version"].map(identity),
  cases: selected.map(({row, expected}) => ({case_id: row.case_id,
    input_sha256: expected.input_sha256, observation_sha256: sha256(JSON.stringify(expected.typescript_observation))})),
};
const rendered = JSON.stringify(manifest, null, 2) + "\n";
if (mode === "--check") assert.equal(fs.readFileSync(path.join(root, manifestPath), "utf8"), rendered);
for (const {row, expected} of selected) {
  for (let pass = 1; pass <= 2; pass++) {
    assert.deepEqual(observe(row), expected.typescript_observation, `${row.case_id} pass ${pass}`);
  }
  console.log(`${row.case_id}: unchanged complete TypeScript command x2`);
}
if (mode === "--write") fs.writeFileSync(path.join(root, manifestPath), rendered, {flag: "wx"});
console.log("Original JavaScript bundle oracle: 4 inputs, 8 fresh Programs; native recorder scope is separate");
