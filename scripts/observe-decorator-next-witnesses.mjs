// Complete TypeScript Program commands for the H2.8a decorator-next witness groups
// (transform-order, super-paths, name-owners, source-followup, source-followup-top-level,
// literal-member-kinds, literal-key-spelling, lexical-prologue, lexical-prologue-readers,
// lexical-prologue-readers-v2). One group per invocation:
//   node scripts/observe-decorator-next-witnesses.mjs <group> --write|--check [destination]
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
const GROUPS = { "transform-order": 60, "super-paths": 42, "name-owners": 24, "source-followup": 48, "source-followup-top-level": 18,
  "literal-member-kinds": 48, "literal-key-spelling": 48, "lexical-prologue": 24, "lexical-prologue-readers": 18,
  "lexical-prologue-readers-v2": 16 };
const group = process.argv[2];
assert.ok(Object.hasOwn(GROUPS, group), "unknown witness group");
assert.ok(["--write", "--check"].includes(process.argv[3]));
const inputPath = `crates/compiler/tests/fixtures/decorator-${group}-inputs.json`;
const destination = path.resolve(root, process.argv[4] ?? `crates/compiler/tests/fixtures/decorator-${group}.json`);
if (process.argv[3] === "--write") assert.ok(!fs.existsSync(destination), "retain existing observations");
const inputs = JSON.parse(fs.readFileSync(path.join(root, inputPath), "utf8")).cases;
assert.equal(inputs.length, GROUPS[group]);
assert.equal(new Set(inputs.map(input => input.case_id)).size, inputs.length);
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
  let exit;
  try {
    exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)), s => status.push(s), undefined,
      (...args) => writes.push(write(args, writes.length)));
  } catch (error) {
    return { outcome: "exception", error: { name: error.name, message: error.message,
      stack: error.stack.split(root).join("<repository>") },
      writes, reported_diagnostics: reported, status_writes: status,
      emit_result: result === undefined ? null : { emit_skipped: result.emitSkipped,
        diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null,
        source_maps: sourceMaps(result.sourceMaps) }, exit_code: null };
  }
  assert.ok(result);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: sourceMaps(result.sourceMaps) },
    status_writes: status, exit_code: exit };
}
const outcomes = inputs.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  const exception = first.outcome === "exception";
  console.log(JSON.stringify({case_id: input.case_id, attempts: 2,
    complete_observations: exception ? 0 : 2, exception_observations: exception ? 2 : 0,
    diagnostics: first.reported_diagnostics.length, writes: first.writes.length, exit_code: first.exit_code}));
  return exception ? {...input, typescript_failure: first} : {...input, typescript_observation: first};
});
const cases = outcomes.filter(row => row.typescript_observation !== undefined);
const upstreamFailures = outcomes.filter(row => row.typescript_failure !== undefined);
const artifact = {
  version: 1, status: "Complete upstream observations; native qualification is recorded separately",
  typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)),
  inputs: {path: inputPath, sha256: sha256(fs.readFileSync(path.join(root, inputPath)))},
  repetitions: 2, program_attempts: inputs.length * 2, program_executions: cases.length * 2,
  exception_attempts: upstreamFailures.length * 2, cases, upstream_failures: upstreamFailures,
};
if (process.argv[3] === "--write") fs.writeFileSync(destination, JSON.stringify(artifact, null, 2) + "\n");
else assert.deepEqual(JSON.parse(fs.readFileSync(destination, "utf8")), artifact);
console.log(JSON.stringify({destination, cases: cases.length, upstream_failure_cases: upstreamFailures.length,
  complete_program_executions: cases.length * 2, program_attempts: inputs.length * 2,
  exception_attempts: upstreamFailures.length * 2, native_executions: 0, sha256: sha256(fs.readFileSync(destination))}));
