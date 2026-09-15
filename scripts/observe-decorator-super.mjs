// Complete TypeScript Program commands for the A6-41-SUPER decorator static
// super witnesses. Same host/lib/diagnostic/callback contract as
// observe-decorator-receiver-context.mjs; the input set, destination and
// count assertions are specific to this manifest. A case carrying
// `write_failure_index` reports that write through the host onError callback
// (tsc adds TS5033 and continues), so the failure boundary and partial writes
// are observed rather than simulated.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
// `--write|--check [--extra]`: the primary manifest (672 cases) or the extra
// control manifest (24 cases), each with its own destination and count.
// `--followup`: the 2026-09-15 follow-up manifest (156 cases: arrow parameter
// defaults, unicode-escaped names, the lexical-this declaration shape).
const extra = process.argv.includes("--extra");
const followup = process.argv.includes("--followup");
// `--followup2`: the second follow-up manifest (162 cases: named evaluation of
// anonymous decorated class expressions, unicode-escaped private names).
const followup2 = process.argv.includes("--followup2");
// `--followup3`: the third follow-up manifest (48 cases: emit-helper request
// order of relocated statics below ES2022 and the synthetic member order).
const followup3 = process.argv.includes("--followup3");
assert.ok([extra, followup, followup2, followup3].filter(Boolean).length <= 1, "one manifest per invocation");
const inputPath = followup3
  ? "crates/compiler/tests/fixtures/decorator-super-followup3-inputs.json"
  : followup2
  ? "crates/compiler/tests/fixtures/decorator-super-followup2-inputs.json"
  : followup
  ? "crates/compiler/tests/fixtures/decorator-super-followup-inputs.json"
  : extra
  ? "crates/compiler/tests/fixtures/decorator-super-extra-inputs.json"
  : "crates/compiler/tests/fixtures/decorator-super-inputs.json";
// The observation fixtures are stored zstd-compressed (`<name>.json.zst`;
// the Rust replay decodes them at run time and every SHA-256 it records is
// the decoded JSON's), which keeps the 670-case set at ~1 MB in the tree.
const destination = path.resolve(root, followup3
  ? "crates/compiler/tests/fixtures/decorator-super-followup3.json.zst"
  : followup2
  ? "crates/compiler/tests/fixtures/decorator-super-followup2.json.zst"
  : followup
  ? "crates/compiler/tests/fixtures/decorator-super-followup.json.zst"
  : extra
  ? "crates/compiler/tests/fixtures/decorator-super-extra.json.zst"
  : "crates/compiler/tests/fixtures/decorator-super.json.zst");
const encode = text => zlib.zstdCompressSync(Buffer.from(text), { params: { [zlib.constants.ZSTD_c_compressionLevel]: 19 } });
const decode = bytes => zlib.zstdDecompressSync(bytes).toString("utf8");
const expectedCases = followup3 ? 48 : followup2 ? 162 : followup ? 156 : extra ? 42 : 672;
if (process.argv[2] === "--write") assert.ok(!fs.existsSync(destination), "retain existing observations");
const manifest = JSON.parse(fs.readFileSync(path.join(root, inputPath), "utf8"));
const inputs = manifest.cases;
assert.equal(manifest.variants * manifest.cases_per_variant, inputs.length);
assert.equal(inputs.length, expectedCases);
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
  const failureIndex = input.write_failure_index;
  try {
    exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)), s => status.push(s), undefined,
      (...args) => {
        const index = writes.length;
        const observed = write(args, index);
        if (failureIndex === index) {
          observed.write_failed = true;
          writes.push(observed);
          assert.equal(typeof args[3], "function", "onError callback present for the failed write");
          args[3]("simulated output callback failure");
          return;
        }
        writes.push(observed);
      });
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
const started = Date.now();
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
const json = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, encode(json));
else assert.deepEqual(JSON.parse(decode(fs.readFileSync(destination))), artifact);
console.log(JSON.stringify({destination, cases: cases.length, upstream_failure_cases: upstreamFailures.length,
  complete_program_executions: cases.length * 2, program_attempts: inputs.length * 2,
  exception_attempts: upstreamFailures.length * 2, native_executions: 0, elapsed_seconds: (Date.now() - started) / 1000,
  sha256: sha256(fs.readFileSync(destination)), decoded_sha256: sha256(decode(fs.readFileSync(destination)))}));
