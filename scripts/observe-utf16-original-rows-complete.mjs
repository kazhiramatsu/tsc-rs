// Supplemental complete commands for the four frozen H2.5h UTF-16 rows.
// Uses the qualification's exact inputs/defaults, retaining additional
// callback metadata and command fields without rewriting the old artifact.
// node scripts/observe-utf16-original-rows-complete.mjs --write|--check
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const parentPath = "ratchets/h2-5h-qualification.v1.json";
const destination = "crates/compiler/tests/fixtures/utf16-original-rows-complete.json";
const parentBytes = fs.readFileSync(path.join(root, parentPath));
const parent = JSON.parse(parentBytes);
const stems = ["Strings10", "Strings11", "Templates10", "Templates11"];
const ids = stems.map(stem => `typescript-6.0.3/conformance/es6/unicodeExtendedEscapes/unicodeExtendedEscapesIn${stem}.ts#target%3Des5`);
assert.equal(ts.version, "6.0.3");
assert.equal(sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
assert.ok(["--write", "--check"].includes(process.argv[2]));

function diagnostic(d) {
  return {code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null};
}
function write(args, index) {
  const [name, text, bom, onError, sources, data] = args;
  assert.ok(name.endsWith(".js"));
  if (data !== undefined) assert.deepEqual(Object.keys(data).sort(), ["diagnostics", "sourceMapUrlPos"]);
  const bytes = Buffer.from(text);
  const materialized = bom ? Buffer.concat([Buffer.from([239,187,191]), bytes]) : bytes;
  return {index, path: ts.normalizePath(name), kind: "javascript",
    callback_utf8_base64: bytes.toString("base64"), callback_utf8_sha256: sha(bytes), callback_utf8_bytes: bytes.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"),
    materialized_utf8_sha256: sha(materialized), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sources?.map(s => ts.normalizePath(s.fileName)) ?? null,
    data_present: data !== undefined, data_keys: data === undefined ? null : Object.keys(data),
    data_source_map_url_pos: data?.sourceMapUrlPos ?? null, data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null};
}
function observe(input, options) {
  const files = new Map(input.files.map(f => [f.path, Buffer.from(f.utf8_base64, "base64").toString("utf8")]));
  const base = ts.createCompilerHost(options, true);
  const dirs = createHermeticDirectoryOverlay(files.keys(), {currentDirectory: input.current_directory,
    useCaseSensitiveFileNames: true, fallbackHost: base});
  // Same createProgramCase host policy as h2-5h-qualification.mjs: exact
  // VFS spellings first, standard compiler host fallback for the libraries.
  const host = {...base, getCurrentDirectory: () => input.current_directory,
    useCaseSensitiveFileNames: () => true, getCanonicalFileName: name => name, trace() {},
    fileExists: name => files.has(ts.normalizePath(name)) || base.fileExists(ts.normalizePath(name)),
    readFile: name => files.get(ts.normalizePath(name)) ?? base.readFile(ts.normalizePath(name)),
    directoryExists: name => dirs.directoryExists(name), getDirectories: name => dirs.getDirectories(name),
    realpath: name => files.has(ts.normalizePath(name)) ? ts.normalizePath(name) : (base.realpath?.(ts.normalizePath(name)) ?? ts.normalizePath(name)),
    getSourceFile(name, languageVersion) {
      const normalized = ts.normalizePath(name), text = files.get(normalized);
      return text === undefined ? base.getSourceFile(name, languageVersion)
        : ts.createSourceFile(normalized, text, languageVersion, true, ts.getScriptKindFromFileName(normalized));
    }, writeFile: () => assert.fail("unexpected host write")};
  const program = ts.createProgram(input.roots, options, host);
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)),
    s => status.push(s), undefined, (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  const maps = result.sourceMaps?.map(entry => {
    assert.deepEqual(Object.keys(entry).sort(), ["inputSourceFileNames", "sourceMap"]);
    return {input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap)};
  }) ?? null;
  return {writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: {emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      emitted_files: result.emittedFiles ?? null, source_maps: maps}, status_writes: status, exit_code: exit};
}
// Select only the original artifact's fields, with no value normalization.
// Every old observed value must agree; the new artifact adds evidence.
function oldShape(actual, expected) {
  if (Array.isArray(expected)) {
    assert.ok(Array.isArray(actual)); assert.equal(actual.length, expected.length);
    return expected.map((value, i) => oldShape(actual[i], value));
  }
  if (expected !== null && typeof expected === "object") {
    assert.ok(actual !== null && typeof actual === "object");
    return Object.fromEntries(Object.entries(expected).map(([key, value]) => {
      assert.ok(Object.hasOwn(actual, key), key); return [key, oldShape(actual[key], value)];
    }));
  }
  return actual;
}
const cases = ids.map(id => {
  const row = parent.cases.find(c => c.case_id === id); assert.ok(row);
  assert.equal(row.execution_route, "qualified-vfs"); assert.equal(row.disposition, "admitted-for-execution");
  const input = row.input;
  assert.equal(input.current_directory, "/.src"); assert.equal(input.virtual_config, null);
  assert.deepEqual(input.vfs_symlinks, []); assert.deepEqual(input.settings, [{name: "target", value: "es5"}]);
  assert.equal(input.files.length, 1); assert.deepEqual(input.roots, [input.files[0].path]);
  for (const file of input.files) {
    const bytes = Buffer.from(file.utf8_base64, "base64");
    assert.equal(bytes.length, file.utf8_bytes); assert.equal(sha(bytes), file.utf8_sha256);
  }
  // effectiveCompilerOptions defaults at h2-5h-qualification.mjs:572-590,
  // plus the only setting present in each of these four immutable inputs.
  const options = {noResolve: false, newLine: ts.NewLineKind.CarriageReturnLineFeed,
    noErrorTruncation: true, skipDefaultLibCheck: true, target: ts.ScriptTarget.ES5};
  const first = observe(input, options), second = observe(input, options);
  assert.deepEqual(second, first, id);
  const {run_fingerprint_sha256: fingerprint, ...old} = row.typescript_observation;
  assert.ok(row.typescript_run_fingerprints.every(value => value === fingerprint));
  assert.deepEqual(oldShape(first, old), old, id);
  console.log(`${id}: complete x2; every frozen field unchanged`);
  return {case_id: id, input, options, typescript_observation: first};
});
const artifact = {version: 1, status: "Complete supplemental upstream commands; native evidence recorded separately",
  typescript: ts.version, compiler_sha256: sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  parent: {path: parentPath, sha256: sha(parentBytes)}, observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  repetitions: 2, complete_program_executions: 8, cases};
const output = path.join(root, destination);
if (process.argv[2] === "--write") {
  assert.ok(!fs.existsSync(output), "retain existing observations"); fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n");
} else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(`${destination}: ${sha(fs.readFileSync(output))}`);
