// Nonbundle preserved source references: forced paths are independent of declaration.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const observerPath = "scripts/observe-declaration-reference-paths.mjs";
const fixturePath = "crates/compiler/tests/fixtures/declaration-reference-paths.json";
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const identity = name => ({ path: name, sha256: sha256(fs.readFileSync(path.join(root, name))) });
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
const mode = process.argv[2];
assert.ok(["--write", "--check"].includes(mode));
const inputs = [];
for (const declaration of [false, true]) for (const declarationMap of [false, true]) for (const included of [false, true]) {
  inputs.push({
    case_id: `source-reference#declaration-${declaration}-map-${declarationMap}-included-${included}`,
    current_directory: "/project", roots: ["/project/src/main.ts"],
    files: [
      { path: "/project/src/main.ts", text: '/// <reference path="./dep/b.ts" preserve="true" />\nexport const main: number = 1;\n' },
      { path: "/project/src/dep/b.ts", text: "export const b: string = 'b';\n" },
    ],
    options: { target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.CommonJS, declaration, declarationMap,
      noResolve: !included, outDir: "out", listEmittedFiles: true, strict: true,
      skipDefaultLibCheck: true, noErrorTruncation: true, newLine: ts.NewLineKind.CarriageReturnLineFeed },
    calls: [
      { kind: "forced-declarations", target_source: null },
      { kind: "declaration-diagnostics", target_source: null },
      { kind: "declaration-diagnostics", target_source: "/project/src/main.ts" },
      { kind: "forced-declarations", target_source: "/project/src/main.ts" },
      { kind: "ordinary-command", target_source: null },
      { kind: "declaration-diagnostics", target_source: null },
      { kind: "forced-declarations", target_source: null },
    ].map(call => ({ ...call, owner: "H2.7e" })),
    sink_rules: [], transport: "callback", owner: "H2.7e",
  });
}
// Retain the initial outDir matrix unchanged: its Rust request boundary is
// H2.8a outside its admitted JS/JSON relocation families. Use distinct IDs for the base matrix.
const adjacentInputs = inputs.slice();
inputs.splice(0, inputs.length, ...adjacentInputs.map(input => {
  const primary = structuredClone(input);
  primary.case_id = primary.case_id.replace("source-reference#", "source-reference-in-place#");
  delete primary.options.outDir;
  return primary;
}));
const supplementalInputs = [
  { name: "json", path: "/project/src/dep/data.json", text: '{"value":1}\n', options: { resolveJsonModule: true } },
  { name: "suppressed-javascript", path: "/project/src/dep/b.js", text: 'export const b = 1;\n', options: { allowJs: true, noEmitForJsFiles: true } },
  { name: "declaration-file", path: "/project/src/dep/b.d.ts", text: 'export declare const b: number;\n', options: {} },
].map(target => {
  const input = structuredClone(inputs.find(input => !input.options.declaration && !input.options.declarationMap && !input.options.noResolve));
  input.case_id = "source-reference-target#" + target.name;
  input.files[0].text = input.files[0].text.replace("./dep/b.ts", "./dep/" + path.basename(target.path));
  input.files[1] = { path: target.path, text: target.text };
  Object.assign(input.options, target.options);
  return input;
});
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
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name) && fs.existsSync(path.join(libraryRoot, path.basename(name)));
  const read = name => files.get(ts.normalizePath(name)) ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(files.keys(), { currentDirectory: input.current_directory,
    useCaseSensitiveFileNames: true, fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  const system = { ...ts.sys, ...overlay, useCaseSensitiveFileNames: true, getCurrentDirectory: () => input.current_directory,
    getExecutingFilePath: () => "/lib/typescript.js", readFile: read,
    fileExists: name => files.has(ts.normalizePath(name)) || library(name),
    write() { assert.fail("unexpected status write"); }, writeFile() { assert.fail("unexpected System write"); } };
  const host = ts.createCompilerHostWorker(input.options, true, system);
  let current;
  const sink = (name, text, bom, onError, sources, data) => {
    assert.ok(current); assert.ok(!text.includes(root));
    const callback = Buffer.from(text);
    const materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), callback]) : callback;
    current.writes.push({ index: current.writes.length, path: name,
      kind: name.endsWith(".map") ? "source-map" : ts.isDeclarationFileName(name) ? "declaration" : "javascript",
      callback_utf8_base64: callback.toString("base64"), callback_utf8_bytes: callback.length,
      write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
      on_error_callback_present: onError !== undefined, source_files: sources?.map(source => source.fileName) ?? null,
      data_before: metadata(data), data_after: metadata(data), sink_action: "write", sink_materialized: true, on_error_messages: [] });
  };
  host.writeFile = sink;
  const program = ts.createProgram(input.roots, input.options, host);
  const resolverRequests = [];
  const checker = program.getTypeChecker(), getEmitResolver = checker.getEmitResolver.bind(checker);
  checker.getEmitResolver = (source, token, skip) => {
    resolverRequests.push({ source_file: source?.fileName ?? null, skip_diagnostics: skip ?? null });
    return getEmitResolver(source, token, skip);
  };
  const calls = [];
  for (const call of input.calls) {
    current = { ...call, writes: [], system_write_attempts: [], diagnostics: null, reported_diagnostics: null,
      status_writes: null, exit_code: null, emit_result: null, exception: null };
    const source = call.target_source === null ? undefined : program.getSourceFile(call.target_source);
    if (call.target_source !== null) assert.ok(source);
    const start = resolverRequests.length;
    try {
      if (call.kind === "declaration-diagnostics") current.diagnostics = program.getDeclarationDiagnostics(source).map(diagnostic);
      else if (call.kind === "ordinary-command") {
        current.reported_diagnostics = []; current.status_writes = [];
        const emit = program.emit;
        let result;
        program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
        try {
          current.exit_code = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => current.reported_diagnostics.push(diagnostic(d)),
            text => current.status_writes.push(text), undefined, sink);
          assert.ok(result); current.emit_result = resultRecord(result);
        } finally { program.emit = emit; }
      } else current.emit_result = resultRecord(program.emit(source, sink, undefined, true, undefined, true));
    } catch (error) {
      assert.equal(error.name, "Error"); assert.equal(error.message, "Debug Failure.");
      assert.equal(call.kind, "forced-declarations");
      assert.equal(input.options.declaration, false); assert.equal(input.options.declarationMap, true);
      current.exception = { name: error.name, message: error.message };
    }
    current.resolver_requests = resolverRequests.slice(start);
    current.materialized_write_indices = current.writes.map(write => write.index);
    calls.push(current); current = undefined;
  }
  const programDiagnostics = { options: program.getOptionsDiagnostics().map(diagnostic), syntactic: program.getSyntacticDiagnostics().map(diagnostic),
    global: program.getGlobalDiagnostics().map(diagnostic), semantic: program.getSemanticDiagnostics().map(diagnostic) };
  return { program_source_order: program.getSourceFiles().filter(source => !library(source.fileName)).map(source => source.fileName),
    standard_libraries: program.getSourceFiles().filter(source => library(source.fileName)).map(source => path.basename(source.fileName)),
    calls, program_diagnostics_after_calls: programDiagnostics };
}
const rows = [...inputs, ...supplementalInputs, ...adjacentInputs].map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  const included = !input.options.noResolve;
  assert.deepEqual(first.program_source_order, included ? [input.files[1].path, "/project/src/main.ts"] : input.roots);
  for (const call of first.calls) for (const write of call.writes.filter(write => write.kind === "declaration")) {
    const text = Buffer.from(write.callback_utf8_base64, "base64").toString();
    if (write.source_files.includes("/project/src/main.ts")) {
      const reference = path.basename(input.files[1].path) === "data.json" ? "dep/data.d.json.ts" : "dep/b.d.ts";
      assert.equal(text.includes(`/// <reference path="${reference}" preserve="true" />`), included, `${input.case_id}: ${text}`);
    }
  }
  console.log(input.case_id);
  return { ...input, input_sha256: sha256(JSON.stringify(input)), typescript_observation: first };
});
const cases = rows.slice(0, inputs.length), supplemental = rows.slice(inputs.length, inputs.length + supplementalInputs.length),
  adjacent = rows.slice(inputs.length + supplementalInputs.length);
const artifact = { schema: 1, kind: "declaration-reference-paths", typescript: ts.version,
  source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8", repetitions: 2,
  observer: identity(observerPath), inputs: ["vendor/typescript-6.0.3/lib/typescript.js", "vendor/typescript-6.0.3/lib/_tsc.js",
    ".node-version", "crates/oracle/vfs-directory-overlay.mjs"].map(identity),
  execution_contract: "Only main.ts is a root. The referenced source remains mounted in both inclusion variants; noResolve controls Program inclusion. Seven calls share one checker, with final Program diagnostics afterward. Fresh forced emits precede getters. TypeScript exceptions and all partial callback writes remain observations.",
  cases, supplemental_reference_targets: supplemental,
  adjacent_out_dir_owner: "H2.8a", adjacent_out_dir_observations: adjacent,
  summary: { sequences: cases.length, adjacent_out_dir_sequences: adjacent.length,
    supplemental_sequences: supplemental.length,
    calls_per_repetition: [...cases, ...supplemental].reduce((sum, row) => sum + row.calls.length, 0),
    exception_calls_per_repetition: cases.reduce((sum, row) => sum + row.typescript_observation.calls.filter(call => call.exception).length, 0), runtime_admitted: 0 } };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
assert.ok(!rendered.includes(root));
if (mode === "--write") fs.writeFileSync(path.join(root, fixturePath), rendered);
else assert.equal(fs.readFileSync(path.join(root, fixturePath), "utf8"), rendered, "reference paths fixture is stale");
console.log(JSON.stringify(artifact.summary));
