// Complete pinned TS6 Bundle command observations for sink/listing asymmetry.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const observerPath = "scripts/observe-bundle-sinks.mjs";
const fixturePath = "crates/compiler/tests/fixtures/bundle-sinks.json";
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const identity = name => ({ path: name, sha256: sha256(fs.readFileSync(path.join(root, name))) });
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
const mode = process.argv[2];
assert.ok(["--write", "--check"].includes(mode), "use --write or --check");

const outputPaths = ["/project/bundle.js.map", "/project/bundle.js", "/project/bundle.d.ts.map", "/project/bundle.d.ts"];
const listingPaths = [outputPaths[1], outputPaths[0], outputPaths[3], outputPaths[2]];
const base = {
  current_directory: "/project", use_case_sensitive_file_names: true,
  files: [{ path: "/project/a.ts", text: "const a = 1;" }, { path: "/project/b.ts", text: "const b = 2;" }],
  roots: ["/project/a.ts", "/project/b.ts"],
  options: { target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.AMD, outFile: "/project/bundle.js",
    sourceMap: true, declaration: true, declarationMap: true, listEmittedFiles: true,
    skipDefaultLibCheck: true, newLine: ts.NewLineKind.LineFeed },
  kind: "ordinary-command", target_source: null,
};
const inputs = [{ case_id: "bundle/sink#normal", ...base, sink_rules: [] }];
for (const [index, kind] of ["javascript-map", "javascript", "declaration-map", "declaration"].entries()) {
  inputs.push({ case_id: `bundle/sink#${kind}-skip-unchanged`, ...base,
    sink_rules: [{ path: outputPaths[index], action: "skip-unchanged" }] });
}
inputs.push({ case_id: "bundle/sink#all-on-error", ...base,
  sink_rules: outputPaths.map(path => ({ path, action: "on-error" })) });
for (const [index, kind] of ["javascript-map", "javascript", "declaration-map", "declaration"].entries()) {
  inputs.push({ case_id: `bundle/sink#${kind}-throw`, ...base,
    sink_rules: [{ path: outputPaths[index], action: "throw" }] });
}
assert.equal(inputs.length, 10);

function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function metadata(data) {
  assert.ok(data === undefined || Object.keys(data).every(name => ["sourceMapUrlPos", "diagnostics", "skippedDtsWrite"].includes(name)));
  return { present: data !== undefined, keys: data === undefined ? null : Object.keys(data),
    source_map_url_pos: data?.sourceMapUrlPos ?? null, diagnostics: data?.diagnostics?.map(diagnostic) ?? null,
    skipped_dts_write: data?.skippedDtsWrite ?? null };
}
function writeRecord(args, index) {
  const [fileName, text, bom, onError, sourceFiles, data] = args;
  assert.ok(!text.includes(root));
  const callback = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), callback]) : callback;
  return { index, path: fileName, kind: fileName.endsWith(".map") ? "source-map" : ts.isDeclarationFileName(fileName) ? "declaration" : "javascript",
    callback_utf8_base64: callback.toString("base64"), callback_utf8_bytes: callback.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sourceFiles?.map(source => source.fileName) ?? null,
    data_before: metadata(data), data_after: null, sink_action: null, sink_materialized: false, on_error_messages: [] };
}
function resultRecord(result) {
  return { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
    emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps?.map(entry => {
      assert.deepEqual(Object.keys(entry).sort(), ["inputSourceFileNames", "sourceMap"]);
      return { input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap) };
    }) ?? null };
}


function observe(input) {
  const files = new Map(input.files.map(file => [file.path, file.text]));
  const libraryRoot = path.join(root, "vendor/typescript-6.0.3/lib");
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name)
    && fs.existsSync(path.join(libraryRoot, path.basename(name)));
  const read = name => files.get(ts.normalizePath(name))
    ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(files.keys(), { currentDirectory: input.current_directory,
    useCaseSensitiveFileNames: input.use_case_sensitive_file_names,
    fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  const system = { ...ts.sys, ...overlay, useCaseSensitiveFileNames: input.use_case_sensitive_file_names,
    getCurrentDirectory: () => input.current_directory, getExecutingFilePath: () => "/lib/typescript.js",
    readFile: read, fileExists: name => files.has(ts.normalizePath(name)) || library(name),
    write() { assert.fail("unexpected System status write"); },
    writeFile() { assert.fail("callback transport must own every write"); } };
  const host = ts.createCompilerHostWorker(input.options, true, system);
  const call = { kind: input.kind, target_source: input.target_source, writes: [], reported_diagnostics: [],
    status_writes: [], exit_code: null, emit_result: null, exception: null };
  const materialized = new Map();
  const sink = (...args) => {
    const record = writeRecord(args, call.writes.length), data = args[5];
    call.writes.push(record);
    record.sink_action = input.sink_rules.find(rule => rule.path === args[0])?.action ?? "write";
    try {
      if (record.sink_action === "on-error") {
        const message = "H2.7 bundle controlled callback failure";
        record.on_error_messages.push(message); assert.ok(args[3]); args[3](message);
      } else if (record.sink_action === "throw") throw new Error("H2.7 bundle controlled callback exception");
      else if (record.sink_action === "skip-unchanged") {
        // Both text callbacks have data, but JS ignores the returned skip signal.
        // Map callbacks have no data and cannot set skippedDtsWrite at all.
        if (args[0].endsWith(".map")) assert.equal(data, undefined);
        else { assert.ok(data); data.skippedDtsWrite = true; }
      } else {
        materialized.set(args[0], Buffer.from(record.materialized_utf8_base64, "base64"));
        record.sink_materialized = true;
      }
    } finally { record.data_after = metadata(data); }
  };
  host.writeFile = sink;
  const program = ts.createProgram(input.roots, input.options, host);
  const emit = program.emit;
  let result;
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  try {
    call.exit_code = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => call.reported_diagnostics.push(diagnostic(d)),
      text => call.status_writes.push(text), undefined, sink);
    assert.ok(result); call.emit_result = resultRecord(result);
  } catch (error) {
    assert.equal(error.name, "Error");
    assert.equal(error.message, "H2.7 bundle controlled callback exception");
    assert.equal(call.writes.at(-1)?.sink_action, "throw");
    call.exception = { name: error.name, message: error.message };
  } finally { program.emit = emit; }
  call.materialized_write_indices = call.writes.filter(write => write.sink_materialized).map(write => write.index);
  call.materialized_files = [...materialized].map(([path, bytes]) => ({ path,
    utf8_base64: bytes.toString("base64"), utf8_bytes: bytes.length }));
  return { program_source_order: program.getSourceFiles().filter(source => !library(source.fileName)).map(source => source.fileName),
    standard_libraries: program.getSourceFiles().filter(source => library(source.fileName)).map(source => path.basename(source.fileName)),
    call };
}

const cases = inputs.map((input, index) => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  const call = first.call;
  assert.deepEqual(call.writes.map(write => write.path), outputPaths.slice(0, call.writes.length));
  for (const write of call.writes) {
    assert.deepEqual(write.source_files, base.roots);
    assert.equal(Buffer.from(write.callback_utf8_base64, "base64").length, write.callback_utf8_bytes);
    assert.equal(Buffer.from(write.materialized_utf8_base64, "base64").length, write.materialized_utf8_bytes);
    assert.equal(write.data_before.present, !write.path.endsWith(".map"));
  }
  const throwingRule = input.sink_rules.find(rule => rule.action === "throw");
  if (throwingRule) {
    const index = outputPaths.indexOf(throwingRule.path);
    assert.equal(call.writes.length, index + 1);
    assert.equal(call.materialized_files.length, index);
    assert.equal(call.emit_result, null); assert.equal(call.exit_code, null);
    assert.deepEqual(call.reported_diagnostics, []); assert.deepEqual(call.status_writes, []);
  } else {
    assert.equal(call.exception, null); assert.equal(call.emit_result.emit_skipped, false);
    assert.equal(call.emit_result.source_maps.length, 2);
    const dtsSkipped = input.sink_rules.some(rule => rule.path === outputPaths[3] && rule.action === "skip-unchanged");
    assert.deepEqual(call.emit_result.emitted_files, listingPaths.filter(name => !dtsSkipped || name !== outputPaths[3]));
    assert.deepEqual(call.status_writes, call.emit_result.emitted_files.map(name => `TSFILE: ${name}`));
    assert.equal(call.exit_code, 2); // Real outFile/AMD option diagnostics stay present.
    if (input.sink_rules.some(rule => rule.action === "on-error")) {
      assert.equal(call.materialized_files.length, 0);
      assert.equal(call.emit_result.diagnostics.filter(d => d.code === 5033).length, 4);
    }
  }
  console.log(`${index + 1}/${inputs.length} ${input.case_id}`);
  return { ...input, input_sha256: sha256(JSON.stringify(input)), typescript_observation: first };
});
const artifact = { schema: 1, kind: "bundle-sinks", status: "typescript-reference-only", typescript: ts.version,
  source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8", repetitions: 2,
  observer: identity(observerPath), inputs: ["vendor/typescript-6.0.3/lib/typescript.js", "vendor/typescript-6.0.3/lib/_tsc.js",
    ".node-version", "crates/oracle/vfs-directory-overlay.mjs"].map(identity),
  execution_contract: "Each ordinary full-Program command is repeated twice on a fresh Program and empty in-memory sink. Callback attempts, before/after metadata, actual materialized files, diagnostics, status writes, exit and complete sourceMaps remain separate facets. Rust sink Err corresponds to onError/TS5033; controlled panic corresponds to a direct callback throw with absent command result and retained partial writes. JS and map skips retain listing entries; only skipped DTS text is omitted. No runtime admission or closure is inferred.",
  cases, summary: { cases: cases.length, command_observations_per_repetition: cases.length,
    callback_attempts_per_repetition: cases.reduce((n, row) => n + row.typescript_observation.call.writes.length, 0),
    materialized_files_per_repetition: cases.reduce((n, row) => n + row.typescript_observation.call.materialized_files.length, 0),
    successful_returns_per_repetition: cases.filter(row => !row.typescript_observation.call.exception).length,
    direct_exceptions_per_repetition: cases.filter(row => row.typescript_observation.call.exception).length,
    fresh_program_repetitions: 2, runtime_admitted: 0 } };
assert.equal(artifact.summary.callback_attempts_per_repetition, 34);
assert.equal(artifact.summary.materialized_files_per_repetition, 22);
const rendered = JSON.stringify(artifact, null, 2) + "\n";
assert.ok(!rendered.includes(root));
if (mode === "--write") fs.writeFileSync(path.join(root, fixturePath), rendered);
else assert.equal(fs.readFileSync(path.join(root, fixturePath), "utf8"), rendered, "bundle sink fixture is stale");
console.log(JSON.stringify(artifact.summary));
