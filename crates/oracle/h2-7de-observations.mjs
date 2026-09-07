// Complete emit tuples for the original H2.7d/e one-shot candidate inputs.
// This is a TypeScript reference packet, not a Rust or hosted qualification.
import assert from "node:assert/strict";
import path from "node:path";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "./vfs-directory-overlay.mjs";
import { root, sourceCommit, inputPath, inventoryPath, sha256, identity, prepare, persist, parseConfig } from "./h2-7de-candidates.mjs";

const mode = process.argv[2];
assert.ok(["--write", "--check"].includes(mode), "use --write or --check");
const { inputs, inventory } = prepare();
persist(inputPath, inputs, "--check");
persist(inventoryPath, inventory, "--check");
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

const cases = [];
for (const [index, input] of inputs.cases.entries()) {
  if (input.input.route !== "whole-program") continue;
  const observation = observe(input);
  assert.deepEqual(observe(input), observation, input.case_id);
  const facts = inventory.cases[index].source_facts;
  for (const file of [...facts.parse_diagnostic_units, ...facts.excessive_depth_units]) {
    assert.ok(observation.program_source_order.includes(file.path), "source boundary must be reached: " + file.path);
  }
  cases.push({ case_id: input.case_id, input_sha256: sha256(JSON.stringify(input)),
    required_slices: inventory.cases[index].required_slices, disposition: "typescript-reference-only",
    typescript_observation: observation, repetitions: 2 });
  console.log(`${cases.length}/323 ${input.case_id}`);
}
assert.equal(cases.length, 323);
const artifact = { schema: 1, kind: "h2-7de-observations", status: "typescript-reference-only", typescript: ts.version,
  source_commit: sourceCommit, repetitions: 2, generator: identity("crates/oracle/h2-7de-observations.mjs"),
  inputs: [inputPath, inventoryPath, "crates/oracle/h2-7de-candidates.mjs", "crates/oracle/vfs-directory-overlay.mjs",
    "vendor/typescript-6.0.3/lib/typescript.js", "vendor/typescript-6.0.3/lib/_tsc.js", ".node-version"].map(identity),
  execution_contract: "323 original one-shot candidates, fresh whole Program emitFilesAndReportErrorsAndGetExitStatus twice, one serial worker. Complete emit diagnostics, writes/order/bytes/callback metadata, emittedFiles/sourceMaps absence versus empty, status and exit. Source order excludes separately named pinned libraries. Host resolution traces and project declaration recheck are not emit observations. All later intersections remain references; no Rust qualification, ownership activation or inherited success.",
  cases, summary: { whole_program_candidates: 323, repetitions: 2, deterministic_cases: 323, typescript_runs: 646,
    transpile_controls_not_executed: 2, runtime_admitted: 0,
    writes: cases.reduce((count, row) => count + row.typescript_observation.writes.length, 0),
    emit_skipped: cases.filter(row => row.typescript_observation.emit_result.emit_skipped).length } };
persist("ratchets/h2-7de-observations.v1.json", artifact, mode);
console.log(JSON.stringify(artifact.summary));
