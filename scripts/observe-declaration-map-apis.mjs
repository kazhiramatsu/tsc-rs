// H2.7e API and sink intersections. Expectations come only from pinned TS 6.0.3.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const observerPath = "scripts/observe-declaration-map-apis.mjs";
const fixturePath = "crates/compiler/tests/fixtures/declaration-map-apis.json";
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const identity = name => ({ path: name, sha256: sha256(fs.readFileSync(path.join(root, name))) });
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
const mode = process.argv[2];
assert.ok(["--write", "--check"].includes(mode), "use --write or --check");

const defaults = { target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.CommonJS, declaration: true, declarationMap: true,
  listEmittedFiles: true, strict: true, skipDefaultLibCheck: true, noErrorTruncation: true, newLine: ts.NewLineKind.CarriageReturnLineFeed };
const getter = (target = null) => ({ kind: "declaration-diagnostics", target_source: target });
const forced = (target = null) => ({ kind: "forced-declarations", target_source: target });
const command = () => ({ kind: "ordinary-command", target_source: null });
const good = { "a.ts": "export const a: number = 1;\n", "b.ts": "export const b: string = 'b';\n" };
const declarationErrors = { "a.ts": good["a.ts"], "b.ts": "export const b = class { private field: number = 1; };\n" };
const javascript = { "a.js": "/** @param {number} value */\nexport function f(value) { return value; }\n" };
const json = { "data.json": '{"value":1,"nested":{"name":"😀"}}\n' };
const declarationInput = { "types.d.ts": "export declare const existing: number;\n" };
const inputs = [];
function add(case_id, files, options, calls, extra = {}) {
  inputs.push({ case_id, current_directory: "/project", files: Object.entries(files).map(([name, text]) => ({ path: "/project/" + name, text })),
    options: { ...defaults, ...options }, calls: calls.map(call => ({ ...call,
      owner: call.kind === "ordinary-target-declarations" ? "H2.8d" : call.kind === "ordinary-command" && options.noEmit === true ? "H2.9" : "H2.7e" })),
    sink_rules: [], transport: "callback", owner: "H2.7e", ...extra });
}
const getterCalls = [getter(), getter("/project/a.ts"), getter("/project/b.ts"), getter(), getter("/project/b.ts")];
for (const declaration of [false, true]) for (const declarationMap of [false, true]) {
  add(`getter/options#declaration-${declaration}-map-${declarationMap}`, declarationErrors, { declaration, declarationMap }, getterCalls);
}
for (const name of ["noEmit", "noEmitOnError", "isolatedDeclarations"]) add("getter/gate#" + name, declarationErrors,
  { [name]: true }, [...getterCalls, command(), getter()]);
add("getter/before-and-after-force", declarationErrors, {}, [getter(), forced(), getter("/project/b.ts"), getter(), forced()]);
add("getter/empty", {}, {}, [getter(), command(), getter()]);
add("getter/declaration-only-input", declarationInput, {}, [getter(), getter("/project/types.d.ts"), command(), getter(), getter("/project/types.d.ts")]);
add("getter/javascript", javascript, { allowJs: true, checkJs: true }, [getter(), getter("/project/a.js"), getter()]);

function forceCalls(files) {
  const targets = Object.keys(files);
  return targets.length ? [forced(), ...targets.map(target => forced("/project/" + target)), getter(), forced()]
    : [forced(), getter(), command(), forced()];
}
for (const declaration of [false, true]) for (const declarationMap of [false, true]) {
  const suffix = `#declaration-${declaration}-map-${declarationMap}`;
  add("force/typescript" + suffix, good, { declaration, declarationMap }, forceCalls(good));
  add("force/json" + suffix, json, { declaration, declarationMap, resolveJsonModule: true }, forceCalls(json));
  add("force/empty" + suffix, {}, { declaration, declarationMap }, forceCalls({}));
}
for (const declaration of [false, true]) add("force/javascript#declaration-" + declaration, javascript,
  { declaration, allowJs: true, checkJs: true }, forceCalls(javascript));
for (const declarationMap of [false, true]) add("force/declaration-only-input#map-" + declarationMap, declarationInput,
  { declarationMap }, forceCalls(declarationInput));
for (const jsonFirst of [false, true]) {
  const files = jsonFirst ? { ...json, ...good } : { ...good, ...json };
  add("force/partial-before-debug-failure#json-first-" + jsonFirst, files,
    { declaration: false, resolveJsonModule: true }, [forced(), getter(), forced()]);
}
add("force/gate#noEmit", good, { noEmit: true }, forceCalls(good));
add("force/gate#noEmitOnError-declaration-error", declarationErrors, { noEmitOnError: true }, forceCalls(declarationErrors));
add("force/gate#noEmitOnError-semantic-error", { "a.ts": "export const a: number = '';\n" }, { noEmitOnError: true }, forceCalls({ "a.ts": "" }));
add("force/gate#isolatedDeclarations", declarationErrors, { isolatedDeclarations: true }, forceCalls(declarationErrors));
add("force/gate#disabled-declaration-noEmitOnError", good, { declaration: false, noEmitOnError: true }, [...forceCalls(good), command()]);
for (const name of ["sourceMap", "inlineSourceMap"]) add("force/empty-allocation#" + name, {}, { declarationMap: false, [name]: true }, forceCalls({}));
add("force/inline-options-still-external-declaration-map", good, { inlineSourceMap: true, inlineSources: true }, forceCalls(good));
const unicode = { "src/名.ts": "/** 文😀 */\nexport const 名: string = '😀';\n", "src/nested/b.ts": good["b.ts"] };
add("force/source-root-bom", unicode, { sourceRoot: "https://sources.test/root", declarationDir: "types", emitBOM: true, inlineSources: true }, forceCalls(unicode));

for (const route of ["ordinary-command", "forced-declarations"]) {
  for (const kind of ["map", "declaration"]) for (const action of ["on-error", "throw", "skip-unchanged"]) {
    add(`sink/${route}#${kind}-${action}`, good, { emitDeclarationOnly: true }, [{ kind: route, target_source: null }],
      { sink_rules: [{ path: "/project/a.d.ts" + (kind === "map" ? ".map" : ""), action }] });
  }
  add(`sink/${route}#both-on-error`, good, { emitDeclarationOnly: true }, [{ kind: route, target_source: null }],
    { sink_rules: ["/project/a.d.ts.map", "/project/a.d.ts"].map(path => ({ path, action: "on-error" })) });
}
for (const kind of ["map", "declaration"]) add("sink/compiler-host-system#" + kind + "-throw", good,
  { emitDeclarationOnly: true }, [command()], { transport: "compiler-host-system",
    sink_rules: [{ path: "/project/a.d.ts" + (kind === "map" ? ".map" : ""), action: "system-throw" }] });
assert.equal(inputs.length, 54);
for (const declaration of [false, true]) add("ordinary-target-reference#declaration-" + declaration, good, { declaration },
  [{ kind: "ordinary-target-declarations", target_source: "/project/a.ts" }], { owner: "H2.8d" });

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
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name) && fs.existsSync(path.join(libraryRoot, path.basename(name)));
  const read = name => files.get(ts.normalizePath(name)) ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const createdDirectories = new Set();
  const overlay = createHermeticDirectoryOverlay(files.keys(), { currentDirectory: input.current_directory,
    useCaseSensitiveFileNames: true, fallbackHost: { directoryExists: name => name === "/lib" || createdDirectories.has(name), getDirectories: () => [] } });
  let current, activeWrite;
  const system = { ...ts.sys, ...overlay, useCaseSensitiveFileNames: true, getCurrentDirectory: () => input.current_directory,
    getExecutingFilePath: () => "/lib/typescript.js", readFile: read, fileExists: name => files.has(ts.normalizePath(name)) || library(name),
    write() { assert.fail("unexpected System status write"); }, createDirectory: name => createdDirectories.add(name),
    writeFile(name, text, bom) {
      assert.ok(current && activeWrite);
      current.system_write_attempts.push({ path: name, callback_utf8_base64: Buffer.from(text).toString("base64"), write_byte_order_mark: bom });
      if (input.sink_rules.some(rule => rule.path === name && rule.action === "system-throw")) throw new Error("H2.7e controlled system failure");
      activeWrite.sink_materialized = true;
    } };
  const host = ts.createCompilerHostWorker(input.options, true, system);
  const hostWrite = host.writeFile.bind(host);
  const sink = (...args) => {
    assert.ok(current, "write outside a recorded API call");
    const record = writeRecord(args, current.writes.length), data = args[5];
    current.writes.push(record);
    record.sink_action = input.sink_rules.find(rule => rule.path === args[0])?.action ?? "write";
    const reportError = message => { record.on_error_messages.push(message); assert.ok(args[3]); args[3](message); };
    try {
      if (input.transport === "compiler-host-system") {
        activeWrite = record;
        hostWrite(args[0], args[1], args[2], reportError, args[4], data);
      } else if (record.sink_action === "on-error") reportError("H2.7e controlled callback failure");
      else if (record.sink_action === "throw") throw new Error("H2.7e controlled callback exception");
      else if (record.sink_action === "skip-unchanged") {
        // Maps have no callback data and no skip-return protocol. The d.ts
        // signal is the mutation used by the upstream builder write adapter.
        if (ts.isDeclarationFileName(args[0])) { assert.ok(data); data.skippedDtsWrite = true; }
        else assert.equal(data, undefined);
      } else record.sink_materialized = true;
    } finally { record.data_after = metadata(data); activeWrite = undefined; }
  };
  host.writeFile = sink;
  const program = ts.createProgram(input.roots ?? input.files.map(file => file.path), input.options, host);
  const resolverRequests = [];
  const checker = program.getTypeChecker(), getEmitResolver = checker.getEmitResolver.bind(checker);
  checker.getEmitResolver = (source, token, skip) => {
    resolverRequests.push({ source_file: source?.fileName ?? null, skip_diagnostics: skip ?? null });
    return getEmitResolver(source, token, skip);
  };
  const calls = [];
  for (const call of input.calls) {
    current = { kind: call.kind, owner: call.owner, target_source: call.target_source, writes: [], system_write_attempts: [],
      diagnostics: null, reported_diagnostics: null, status_writes: null, exit_code: null, emit_result: null, exception: null };
    const source = call.target_source === null ? undefined : program.getSourceFile(call.target_source);
    if (call.target_source !== null) assert.ok(source, call.target_source);
    const requestStart = resolverRequests.length;
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
      } else {
        assert.ok(["forced-declarations", "ordinary-target-declarations"].includes(call.kind));
        current.emit_result = resultRecord(program.emit(source, sink, undefined, true, undefined, call.kind === "forced-declarations"));
      }
    } catch (error) {
      assert.equal(error.name, "Error");
      assert.ok(["Debug Failure.", "H2.7e controlled callback exception"].includes(error.message), error.stack);
      if (error.message === "Debug Failure.") {
        assert.equal(call.kind, "forced-declarations");
        assert.equal(input.options.declaration, false);
        assert.equal(input.options.declarationMap, true);
      } else assert.equal(current.writes.at(-1)?.sink_action, "throw");
      current.exception = { name: error.name, message: error.message };
    }
    if (call.kind === "declaration-diagnostics") assert.deepEqual(current.writes, []);
    current.resolver_requests = resolverRequests.slice(requestStart);
    current.materialized_write_indices = current.writes.filter(write => write.sink_materialized).map(write => write.index);
    calls.push(current); current = undefined;
  }
  // Keep this after the API sequence so a diagnostic snapshot cannot prewarm
  // declaration/semantic caches before the first forced or getter operation.
  const programDiagnostics = { options: program.getOptionsDiagnostics().map(diagnostic), syntactic: program.getSyntacticDiagnostics().map(diagnostic),
    global: program.getGlobalDiagnostics().map(diagnostic), semantic: program.getSemanticDiagnostics().map(diagnostic) };
  return { program_source_order: program.getSourceFiles().filter(source => !library(source.fileName)).map(source => source.fileName),
    standard_libraries: program.getSourceFiles().filter(source => library(source.fileName)).map(source => path.basename(source.fileName)),
    calls, program_diagnostics_after_calls: programDiagnostics };
}

const rows = inputs.map((input, index) => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  console.log(`${index + 1}/${inputs.length} ${input.case_id}`);
  return { ...input, input_sha256: sha256(JSON.stringify(input)), typescript_observation: first };
});
const cases = rows.filter(row => row.owner === "H2.7e"), references = rows.filter(row => row.owner === "H2.8d");
for (const row of rows) {
  assert.equal(row.calls.length, row.typescript_observation.calls.length);
  for (const call of row.typescript_observation.calls) {
    if (call.exception) assert.equal(call.emit_result, null);
    if (call.emit_result) {
      assert.equal(typeof call.emit_result.emit_skipped, "boolean");
      for (const name of ["emitted_files", "source_maps"]) assert.ok(call.emit_result[name] === null || Array.isArray(call.emit_result[name]));
    }
    for (const write of call.writes) {
      const callback = Buffer.from(write.callback_utf8_base64, "base64");
      assert.equal(callback.length, write.callback_utf8_bytes);
      const materialized = write.write_byte_order_mark ? Buffer.concat([Buffer.from([239, 187, 191]), callback]) : callback;
      assert.equal(materialized.length, write.materialized_utf8_bytes);
      assert.equal(materialized.toString("base64"), write.materialized_utf8_base64);
    }
  }
}
const artifact = { schema: 1, kind: "declaration-map-apis", status: "typescript-reference-only", typescript: ts.version,
  source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8", repetitions: 2,
  observer: identity(observerPath), inputs: ["vendor/typescript-6.0.3/lib/typescript.js", "vendor/typescript-6.0.3/lib/_tsc.js",
    ".node-version", "crates/oracle/vfs-directory-overlay.mjs"].map(identity),
  execution_contract: "Each complete API sequence repeats on a fresh Program. Calls within each sequence share Program/cache state. Program diagnostics are captured only afterward. Ordinary command and forced emit are separate routes. Direct callback exceptions, compiler-host System exceptions, onError reports and skippedDtsWrite mutations retain distinct outcomes and partial writes. No Rust success is inferred from a TypeScript exception. General ordinary targeted APIs retain H2.8d ownership; one ordinary noEmit command retains H2.9 ownership within its getter sequence.",
  cases, adjacent_ordinary_api_owner: "H2.8d", adjacent_ordinary_api_observations: references,
  summary: { h2_7e_sequences: cases.length, ordinary_target_reference_sequences: references.length,
    fresh_program_repetitions: 2, calls_per_repetition: cases.reduce((sum, row) => sum + row.calls.length, 0),
    exception_calls_per_repetition: cases.reduce((sum, row) => sum + row.typescript_observation.calls.filter(call => call.exception).length, 0),
    embedded_h2_9_command_references: cases.reduce((sum, row) => sum + row.calls.filter(call => call.owner === "H2.9").length, 0),
    runtime_admitted: 0 } };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
assert.ok(!rendered.includes(root));
if (mode === "--write") fs.writeFileSync(path.join(root, fixturePath), rendered);
else assert.equal(fs.readFileSync(path.join(root, fixturePath), "utf8"), rendered, "declaration map API fixture is stale");
console.log(JSON.stringify(artifact.summary));
