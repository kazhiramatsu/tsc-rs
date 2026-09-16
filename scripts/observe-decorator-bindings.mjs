// A41-BINDING (C02) observer: generated-name collision domains of the
// standard-decorator transform.
//
//   pipeline  complete TypeScript Program commands for the manifest written by
//             generate-decorator-binding-inputs.mjs (same host/lib/diagnostic/
//             callback contract as observe-decorator-super.mjs; two matching
//             executions per case) → crates/compiler/tests/fixtures/decorator-binding.json.zst
//   direct    direct API controls (JavaScript text or printer text only, two
//             matching runs per case) → crates/emitter/tests/fixtures/decorator-binding-direct.json
//             groups: synthetic (a `customTransformers.before` transformer
//             inserts a plain identifier spelled like a generated name into
//             the parsed tree of a complete emit), global (the printer's
//             `hasGlobalName` handler answering hit / hit-chain / miss / error
//             for every generated-name domain), lifecycle (re-print, clone,
//             a hook failure before/after the first statement followed by a
//             second generated name on the same printer, dispose).
//
// usage: node scripts/observe-decorator-bindings.mjs pipeline|direct --write|--check [--case <substr>]... [--out <path>]
// `--case`/`--out` are scratch selections (never the frozen destination).
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
const [, , mode, action] = process.argv;
assert.ok(["pipeline", "direct"].includes(mode), "mode: pipeline|direct");
assert.ok(["--write", "--check"].includes(action), "action: --write|--check");
const args = process.argv.slice(4);
const needles = [];
let scratchOut;
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--case") needles.push(args[++i]);
  else if (args[i] === "--out") scratchOut = args[++i];
  else assert.fail(`unknown argument ${args[i]}`);
}
assert.ok(!(needles.length && !scratchOut) || action === "--check", "a --case selection writes only to --out");
const selected = id => !needles.length || needles.some(needle => id.includes(needle));
const K = ts.SyntaxKind;
const CRLF = { newLine: ts.NewLineKind.CarriageReturnLineFeed };

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
// Program construction shared by the pipeline and the synthetic direct group.
function createProgram(input) {
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
  const roots = input.roots ?? input.files.map(file => file.path);
  return ts.createProgram({ rootNames: roots, options: input.options, host, configFileParsingDiagnostics: [] });
}
function observeCommand(input) {
  assert.ok(!input.config, "manifest cases use direct options");
  const program = createProgram(input);
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  let exit;
  try {
    exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)), s => status.push(s), undefined,
      (...args) => { writes.push(write(args, writes.length)); });
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

// ---------------------------------------------------------------- pipeline
function runPipeline() {
  const inputPath = "crates/compiler/tests/fixtures/decorator-binding-inputs.json";
  const destination = scratchOut ?? path.join(root, "crates/compiler/tests/fixtures/decorator-binding.json.zst");
  const encode = text => zlib.zstdCompressSync(Buffer.from(text), { params: { [zlib.constants.ZSTD_c_compressionLevel]: 19 } });
  const decode = bytes => zlib.zstdDecompressSync(bytes).toString("utf8");
  const manifest = JSON.parse(fs.readFileSync(path.join(root, inputPath), "utf8"));
  const inputs = manifest.cases.filter(input => selected(input.case_id));
  assert.equal(manifest.variants * manifest.cases_per_variant, manifest.cases.length);
  assert.equal(manifest.cases.length, 768);
  assert.equal(new Set(manifest.cases.map(input => input.case_id)).size, manifest.cases.length);
  assert.ok(inputs.length, "selection matched no case");
  if (action === "--write" && !scratchOut) assert.ok(!fs.existsSync(destination), "retain existing observations");
  const started = Date.now();
  const outcomes = inputs.map(input => {
    const first = observeCommand(input);
    assert.deepEqual(observeCommand(input), first, input.case_id);
    const exception = first.outcome === "exception";
    console.log(JSON.stringify({ case_id: input.case_id, attempts: 2,
      complete_observations: exception ? 0 : 2, exception_observations: exception ? 2 : 0,
      diagnostics: first.reported_diagnostics.length, writes: first.writes.length, exit_code: first.exit_code }));
    return exception ? { ...input, typescript_failure: first } : { ...input, typescript_observation: first };
  });
  const cases = outcomes.filter(row => row.typescript_observation !== undefined);
  const upstreamFailures = outcomes.filter(row => row.typescript_failure !== undefined);
  const artifact = {
    version: 1, status: "Complete upstream observations; native qualification is recorded separately",
    typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
    compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
    observer_sha256: sha256(fs.readFileSync(import.meta.filename)),
    inputs: { path: inputPath, sha256: sha256(fs.readFileSync(path.join(root, inputPath))) },
    selection: needles.length ? needles : null,
    repetitions: 2, program_attempts: inputs.length * 2, program_executions: cases.length * 2,
    exception_attempts: upstreamFailures.length * 2, cases, upstream_failures: upstreamFailures,
  };
  const json = JSON.stringify(artifact, null, 2) + "\n";
  if (action === "--write") fs.writeFileSync(destination, destination.endsWith(".zst") ? encode(json) : json);
  else assert.deepEqual(JSON.parse(destination.endsWith(".zst") ? decode(fs.readFileSync(destination)) : fs.readFileSync(destination, "utf8")), artifact);
  console.log(JSON.stringify({ destination, cases: cases.length, upstream_failure_cases: upstreamFailures.length,
    complete_program_executions: cases.length * 2, elapsed_seconds: (Date.now() - started) / 1000,
    sha256: sha256(fs.readFileSync(destination)) }));
}

// ---------------------------------------------------------------- direct
const targets = [["es2015", 2], ["es2022", 9], ["esnext", 99]];
const modes = [["set", false], ["define", true]];
const PRELUDE = `const events: unknown[] = [];
function dec(value: any, context: any): any { events.push(["dec", String(context.name)]); return value; }
function key(): "x" { events.push("key"); return "x"; }
function record(value: unknown) { events.push(value); }
`;
const TAIL = `export const tail = events.length;\n`;
const FULL_CLASS = `@dec
export class C extends Object {
    @dec x = 1;
    @dec static m() { return 1; }
    @dec n() { return 2; }
}
record(C.name);
`;
const OUTER_THIS_CLASS = `const holder = { dec, make() { return @dec class extends Object { @(this.dec) static n() {} }; } };
const D = holder.make();
record(D.name);
`;
const COMPUTED_CLASS = `@dec
export class K {
    @dec static [key()] = 1;
}
record(K.name);
`;
const CLASS_EXPRESSION = `const E = @dec class { @dec static m() {} };
record(E.name);
`;

// synthetic: a plain `let <name> = 0;` (factory.createIdentifier, no
// autoGenerate) prepended to the parsed statements by a `before` transformer.
// tsc's isFileLevelUniqueName / isUniqueName consult only the parsed
// identifier table, reserved names and the printer's generated names, so the
// synthetic spelling never shifts a generated name. The `parsed-*` rows put
// the same declaration into the source text instead (the census shifts it).
const syntheticRows = [];
const syntheticNames = [["_metadata", FULL_CLASS], ["_classThis", FULL_CLASS], ["_x_decorators", FULL_CLASS],
  ["_a", COMPUTED_CLASS], ["_outerThis", OUTER_THIS_CLASS], ["class_1", CLASS_EXPRESSION]];
for (const [targetName, target] of targets) {
  for (const [modeName, define] of modes) {
    // module Preserve (200) and LF as in decorator-super-direct: the host-free
    // Rust transformer entry cannot resolve an implied module format.
    const options = { target, module: 200, useDefineForClassFields: define, strict: true, newLine: 1, outDir: "/project/out",
      skipDefaultLibCheck: true, noErrorTruncation: true };
    for (const [name, body] of syntheticNames) {
      syntheticRows.push({ case_id: `decorator-binding-direct/synthetic/${targetName}/${modeName}/injected-${name}`, group: "synthetic",
        text: PRELUDE + body + TAIL, inject: name, options });
    }
    for (const [name, body] of [["_metadata", FULL_CLASS], ["_x_decorators", FULL_CLASS]]) {
      syntheticRows.push({ case_id: `decorator-binding-direct/synthetic/${targetName}/${modeName}/parsed-${name}`, group: "synthetic",
        text: PRELUDE + `let ${name} = 0;\nrecord(${name});\n` + body + TAIL, inject: null, options });
    }
  }
}
function plainLet(factory, name) {
  return factory.createVariableStatement(undefined, factory.createVariableDeclarationList([
    factory.createVariableDeclaration(factory.createIdentifier(name), undefined, undefined, factory.createNumericLiteral("0"))], ts.NodeFlags.Let));
}
function observeSynthetic(row) {
  const program = createProgram({ files: [{ path: "/project/main.ts", text: row.text }], options: row.options });
  const before = row.inject === null ? [] : [context => sourceFile =>
    context.factory.updateSourceFile(sourceFile, [plainLet(context.factory, row.inject), ...sourceFile.statements])];
  const writes = new Map();
  const result = program.emit(undefined, (name, text) => writes.set(name, text), undefined, undefined, { before });
  const diagnostics = ts.getPreEmitDiagnostics(program).map(diagnostic);
  assert.ok(writes.has("/project/out/main.js"), "main.js emitted");
  return { js_text: writes.get("/project/out/main.js"), emit_skipped: result.emitSkipped, diagnostics };
}

// global: generated names of every domain printed through printFile with an
// explicit hasGlobalName handler. Identifiers are created with the public
// factory using exactly the flags the standard-decorator transform uses.
const GLOBAL_SOURCE = `let keep = 1;\nconst o = { [keep]: 1 };\n`;
const globalKinds = [
  ["file-level", "_metadata", ["_metadata", "_metadata_1"]],
  ["scoped", "_x_decorators", ["_x_decorators", "_x_decorators_1"]],
  ["file-wide", "_outerThis", ["_outerThis", "_outerThis_1"]],
  ["numbered", "default", ["default_1", "default_2"]],
  ["temp", null, ["_a", "_b"]],
];
const oracles = ["miss", "hit", "hit-chain", "error"];
const globalRows = [];
for (const [kind, base, chain] of globalKinds) {
  for (const oracle of oracles) {
    globalRows.push({ case_id: `decorator-binding-direct/global/${kind}/${oracle}`, group: "global", kind, base, chain, oracle,
      source: GLOBAL_SOURCE });
  }
}
globalRows.push({ case_id: "decorator-binding-direct/global/file-level/parsed-and-hit", group: "global", kind: "file-level", base: "_metadata",
  chain: ["_metadata_1"], oracle: "hit", source: `let _metadata = 1;\n` + GLOBAL_SOURCE });
globalRows.push({ case_id: "decorator-binding-direct/global/scoped/parsed-and-hit", group: "global", kind: "scoped", base: "_x_decorators",
  chain: ["_x_decorators_1"], oracle: "hit", source: `let _x_decorators = 1;\n` + GLOBAL_SOURCE });
function generatedName(factory, kind, base, sourceFile) {
  switch (kind) {
    case "file-level": return factory.createUniqueName(base, ts.GeneratedIdentifierFlags.Optimistic | ts.GeneratedIdentifierFlags.FileLevel);
    case "scoped": return factory.createUniqueName(base, ts.GeneratedIdentifierFlags.Optimistic | ts.GeneratedIdentifierFlags.ReservedInNestedScopes);
    case "file-wide": return factory.createUniqueName(base, ts.GeneratedIdentifierFlags.Optimistic);
    case "numbered": return factory.createUniqueName(base);
    // getGeneratedNameForNode of a non-member node: the printer's default
    // arm (makeTempVariableName(Auto), not reserved in nested scopes).
    case "temp": return factory.getGeneratedNameForNode(sourceFile.statements[0]);
    // getGeneratedNameForNode of a parsed identifier: cached per node id.
    case "node-derived": return factory.getGeneratedNameForNode(sourceFile.statements[0].declarationList.declarations[0].name);
    default: assert.fail(kind);
  }
}
function declaration(factory, name) {
  return factory.createVariableStatement(undefined, factory.createVariableDeclarationList([
    factory.createVariableDeclaration(name, undefined, undefined, factory.createNumericLiteral("1"))], ts.NodeFlags.Const));
}
function oracleHandler(row, queries) {
  const set = new Set(row.oracle === "hit" ? row.chain.slice(0, 1) : row.oracle === "hit-chain" ? row.chain : []);
  return name => {
    queries.push(name);
    if (row.oracle === "error") throw new Error("global-name-oracle-failure");
    return set.has(name);
  };
}
function printed(text) {
  return { text, utf8_base64: Buffer.from(text).toString("base64"), utf8_bytes: Buffer.byteLength(text) };
}
function observeGlobal(row) {
  const sourceFile = ts.createSourceFile("main.ts", row.source, ts.ScriptTarget.ESNext, true);
  const result = ts.transform(sourceFile, [context => file => {
    const name = generatedName(context.factory, row.kind, row.base, file);
    return context.factory.updateSourceFile(file, [declaration(context.factory, name)]);
  }]);
  const queries = [];
  const printer = ts.createPrinter(CRLF, { hasGlobalName: oracleHandler(row, queries) });
  let outcome;
  try { outcome = { status: "returned", ...printed(printer.printFile(result.transformed[0])) }; }
  catch (error) { outcome = { status: "threw", error: error.message }; }
  result.dispose();
  return { ...outcome, queries };
}

// lifecycle
const LIFECYCLE_SOURCE = `let x = 1;\nx;\nconst o = { [x]: 1 };\n`;
// node-derived-same-node: both statements name the same parsed identifier
// (nodeIdToGeneratedName cache); node-derived-other-node: a second parsed
// identifier with the same text (a fresh cache entry against generatedNames).
const lifecycleKinds = [["numbered", "x"], ["file-level", "_m"], ["scoped", "_s"], ["file-wide", "_o"], ["temp", null], ["node-derived-same-node", null], ["node-derived-other-node", null]];
const lifecycleRows = [];
for (const [kind, base] of lifecycleKinds) {
  if (!kind.startsWith("node-derived")) lifecycleRows.push({ case_id: `decorator-binding-direct/lifecycle/reprint/${kind}`, group: "lifecycle", op: "reprint", kind, base });
  if (!kind.startsWith("node-derived")) lifecycleRows.push({ case_id: `decorator-binding-direct/lifecycle/clone/${kind}`, group: "lifecycle", op: "clone", kind, base });
  for (const phase of ["before", "after"]) {
    lifecycleRows.push({ case_id: `decorator-binding-direct/lifecycle/failure/${kind}/${phase}-statement-1`, group: "lifecycle", op: "failure", kind, base, phase });
  }
  if (kind === "numbered" || kind === "file-level") {
    lifecycleRows.push({ case_id: `decorator-binding-direct/lifecycle/dispose/${kind}`, group: "lifecycle", op: "dispose", kind, base });
  }
}
// Failure-carry identity controls (integration review F1 / F2). The printer
// whose print threw keeps `autoGeneratedIdToGeneratedName`, so the same
// binding printed again keeps its spelling (reprint-after-failure), and
// `nodeIdToGeneratedName` keys on the process-wide node id, so a binding of
// another transformation never resolves through the first one's cache
// (cross-arena: the second transformation's node-derived `x` advances to
// `x_2`, its `y` is `y_1`). A repeated failure of the same binding consumes no
// second ordinal / set entry (double-failure: the next temp is still `_b`).
const lifecycleBase = Object.fromEntries(lifecycleKinds.map(([kind, base]) => [kind, base]));
const FAILURE_CARRY_OPS = new Set(["reprint-after-failure", "cross-arena", "double-failure"]);
for (const kind of ["numbered", "file-level", "scoped", "file-wide", "temp", "node-derived-same-node"]) {
  for (const phase of ["before", "after"]) {
    lifecycleRows.push({ case_id: `decorator-binding-direct/lifecycle/reprint-after-failure/${kind}/${phase}-statement-1`, group: "lifecycle", op: "reprint-after-failure", kind, base: lifecycleBase[kind] ?? null, phase });
  }
}
for (const [kind, second] of [["node-derived-same-node", "same-text"], ["node-derived-same-node", "other-text"], ["numbered", "same-text"], ["numbered", "other-text"], ["scoped", "same-text"], ["file-wide", "same-text"], ["file-level", "same-text"], ["temp", "same-text"]]) {
  lifecycleRows.push({ case_id: `decorator-binding-direct/lifecycle/cross-arena/${kind}/${second}`, group: "lifecycle", op: "cross-arena", kind, base: lifecycleBase[kind] ?? null, second });
}
for (const kind of ["numbered", "scoped", "file-wide", "temp", "node-derived-other-node"]) {
  lifecycleRows.push({ case_id: `decorator-binding-direct/lifecycle/double-failure/${kind}`, group: "lifecycle", op: "double-failure", kind, base: lifecycleBase[kind] ?? null });
}
// Scope controls of the carried tables. scope-fault: a source-file print
// holding two root bindings of the kind, a function whose body holds a third
// and a fourth root binding after it; the hook throws inside the function
// (after its inner statement), after the function, or after the trailing root
// statement. The next standalone print on the same printer shows which
// scope's tables survived: tsc pops a scope's tempFlags / reservedNames when
// it closes, keeps the innermost open scope's tempFlags as the stale value,
// and consults the whole stale reservedNames stack. file-after-failure: a
// standalone print fails, then a source-file print follows on the same
// printer; emitSourceFileWorker pushes a fresh scope (temps restart at `_a`)
// while generatedNames and the stale reservedNames stack continue.
const scopeKinds = [["temp", null], ["scoped", "_s"], ["file-wide", "_o"], ["numbered", "x"], ["file-level", "_m"]];
for (const [kind, base] of scopeKinds) {
  for (const fault of ["inside-nested", "after-nested", "after-tail"]) {
    lifecycleRows.push({ case_id: `decorator-binding-direct/lifecycle/scope-fault/${kind}/${fault}`, group: "lifecycle", op: "scope-fault", kind, base, fault });
  }
  lifecycleRows.push({ case_id: `decorator-binding-direct/lifecycle/file-after-failure/${kind}`, group: "lifecycle", op: "file-after-failure", kind, base });
}
// bundle-fault: two transformed sources printed as one bundle; the hook
// throws after the first source's statement, so the second source's
// emitSourceFileWorker never runs: its root declaration names are not yet
// generated (tsc names them per source at that worker's entry) while the
// first source's scope stays open.
for (const [kind, base] of scopeKinds) {
  lifecycleRows.push({ case_id: `decorator-binding-direct/lifecycle/bundle-fault/${kind}`, group: "lifecycle", op: "bundle-fault", kind, base });
}
FAILURE_CARRY_OPS.add("scope-fault");
FAILURE_CARRY_OPS.add("file-after-failure");
FAILURE_CARRY_OPS.add("bundle-fault");
function scopeBinding(row, factory, sourceFile) {
  // Distinct temps need distinct non-member nodes (getGeneratedNameForNode
  // caches per node): a synthesized empty statement each.
  return declaration(factory, row.kind === "temp" ? factory.getGeneratedNameForNode(factory.createEmptyStatement()) : generatedName(factory, row.kind, row.base, sourceFile));
}
function observeScopeControl(row) {
  const factory = ts.factory;
  const sf = ts.createSourceFile("main.ts", LIFECYCLE_SOURCE, ts.ScriptTarget.ESNext, true);
  const state = { target: null, fault: null };
  const handlers = {
    isEmitNotificationEnabled: node => node.kind === K.VariableStatement || node.kind === K.FunctionDeclaration,
    onEmitNode(hint, node, callback) {
      callback(hint, node);
      if (state.fault === "after" && node === state.target) { state.fault = null; throw new Error("printer-failure:after:target"); }
    },
  };
  const shared = ts.createPrinter(CRLF, handlers);
  const results = [];
  const op = (index, run) => {
    try { results.push({ op: index, status: "returned", ...printed(run()) }); }
    catch (error) { results.push({ op: index, status: "threw", error: error.message }); }
  };
  if (row.op === "scope-fault") {
    const d = [0, 1, 2, 3].map(() => scopeBinding(row, factory, sf));
    const f = factory.createFunctionDeclaration(undefined, undefined, "f", undefined, [], undefined, factory.createBlock([d[2]], true));
    const file = factory.updateSourceFile(sf, [d[0], d[1], f, d[3]]);
    state.target = row.fault === "inside-nested" ? d[2] : row.fault === "after-nested" ? f : d[3];
    state.fault = "after";
    op(0, () => shared.printFile(file));
    assert.equal(state.fault, null, "fault armed and consumed");
    assert.equal(results[0].status, "threw", "the armed fault fired");
    const next = scopeBinding(row, factory, sf);
    op(1, () => shared.printNode(ts.EmitHint.Unspecified, next, sf));
    op(2, () => ts.createPrinter(CRLF, handlers).printNode(ts.EmitHint.Unspecified, next, sf));
    return { results };
  }
  if (row.op === "bundle-fault") {
    const sf2 = ts.createSourceFile("second.ts", LIFECYCLE_SOURCE.replaceAll("x", "y"), ts.ScriptTarget.ESNext, true);
    let first;
    const result = ts.transform([sf, sf2], [context => file => {
      const statement = scopeBinding(row, context.factory, file);
      if (file === sf) first = statement;
      return context.factory.updateSourceFile(file, [statement]);
    }]);
    assert.ok(first, "the first source's statement was created");
    const bundle = factory.createBundle(result.transformed);
    state.target = first; state.fault = "after";
    op(0, () => shared.printBundle(bundle));
    assert.equal(state.fault, null, "fault armed and consumed");
    assert.equal(results[0].status, "threw", "the armed fault fired");
    const next = scopeBinding(row, factory, sf);
    op(1, () => shared.printNode(ts.EmitHint.Unspecified, next, sf));
    op(2, () => ts.createPrinter(CRLF, handlers).printNode(ts.EmitHint.Unspecified, next, sf));
    return { results };
  }
  assert.equal(row.op, "file-after-failure");
  const u1 = scopeBinding(row, factory, sf);
  state.target = u1; state.fault = "after";
  op(0, () => shared.printNode(ts.EmitHint.Unspecified, u1, sf));
  assert.equal(state.fault, null, "fault armed and consumed");
  assert.equal(results[0].status, "threw", "the armed fault fired");
  const file = factory.updateSourceFile(sf, [scopeBinding(row, factory, sf)]);
  op(1, () => shared.printFile(file));
  op(2, () => ts.createPrinter(CRLF, handlers).printFile(file));
  return { results };
}
function failureHandlers(state) {
  return {
    isEmitNotificationEnabled: node => node.kind === K.VariableStatement,
    onEmitNode(hint, node, callback) {
      if (state.fault === "before" && node === state.target) { state.fault = null; throw new Error("printer-failure:before:FirstStatement:1"); }
      callback(hint, node);
      if (state.fault === "after" && node === state.target) { state.fault = null; throw new Error("printer-failure:after:FirstStatement:1"); }
    },
  };
}
function observeFailureCarry(row) {
  const factory = ts.factory;
  const sf1 = ts.createSourceFile("main.ts", LIFECYCLE_SOURCE, ts.ScriptTarget.ESNext, true);
  // cross-arena: the second binding belongs to another source file (another
  // transformation on the Rust side); other-text renames its identifiers.
  const sf2 = row.op === "cross-arena" ? ts.createSourceFile("main.ts", row.second === "other-text" ? LIFECYCLE_SOURCE.replaceAll("x", "y") : LIFECYCLE_SOURCE, ts.ScriptTarget.ESNext, true) : sf1;
  const nameFor = (sourceFile, index, base) => {
    const identifiers = [sourceFile.statements[0].declarationList.declarations[0].name, sourceFile.statements[1].expression];
    return row.kind === "node-derived-same-node" ? factory.getGeneratedNameForNode(identifiers[0])
      : row.kind === "node-derived-other-node" ? factory.getGeneratedNameForNode(identifiers[index])
      : row.kind === "temp" ? factory.getGeneratedNameForNode(sourceFile.statements[index])
      : generatedName(factory, row.kind, base, sourceFile);
  };
  const u1 = declaration(factory, nameFor(sf1, 0, row.base));
  const u2 = row.op === "reprint-after-failure" ? u1
    : row.op === "cross-arena" ? declaration(factory, nameFor(sf2, 0, row.second === "other-text" && row.kind === "numbered" ? "y" : row.base))
    : declaration(factory, nameFor(sf1, 1, row.base));
  const state = { target: u1, fault: null };
  const shared = ts.createPrinter(CRLF, failureHandlers(state));
  const results = [];
  const op = (printer, node, sourceFile, index) => {
    try { results.push({ op: index, status: "returned", ...printed(printer.printNode(ts.EmitHint.Unspecified, node, sourceFile)) }); }
    catch (error) { results.push({ op: index, status: "threw", error: error.message }); }
  };
  const fail = (index) => {
    state.fault = row.phase ?? "after";
    op(shared, u1, sf1, index);
    assert.equal(state.fault, null, "fault armed and consumed");
    assert.equal(results.at(-1).status, "threw", "the armed fault fired");
  };
  fail(0);
  if (row.op === "double-failure") fail(1);
  const next = results.length;
  op(shared, u2, sf2, next);
  op(ts.createPrinter(CRLF, failureHandlers(state)), u2, sf2, next + 1);
  return { results };
}
function observeLifecycle(row) {
  const sourceFile = ts.createSourceFile("main.ts", LIFECYCLE_SOURCE, ts.ScriptTarget.ESNext, true);
  if (row.op === "reprint" || row.op === "clone" || row.op === "dispose") {
    const result = ts.transform(sourceFile, [context => file => {
      const name = generatedName(context.factory, row.kind, row.base, file);
      const statements = [declaration(context.factory, name)];
      if (row.op === "clone") statements.push(context.factory.createExpressionStatement(context.factory.cloneNode(name)));
      return context.factory.updateSourceFile(file, statements);
    }]);
    const printer = ts.createPrinter(CRLF);
    const first = printed(printer.printFile(result.transformed[0]));
    if (row.op === "dispose") {
      result.dispose();
      const second = printed(ts.createPrinter(CRLF).printFile(result.transformed[0]));
      return { first, after_dispose: second };
    }
    const second = printed(printer.printFile(result.transformed[0]));
    return { first, second };
  }
  if (row.op === "scope-fault" || row.op === "file-after-failure" || row.op === "bundle-fault") return observeScopeControl(row);
  if (FAILURE_CARRY_OPS.has(row.op)) return observeFailureCarry(row);
  assert.equal(row.op, "failure");
  // Two standalone statements with fresh generated names of the same kind
  // (node-derived: two parsed identifiers of the same text `x`), printed
  // through printNode with the hook failure of the C03 observer shape.
  const factory = ts.factory;
  const identifierNodes = [sourceFile.statements[0].declarationList.declarations[0].name, sourceFile.statements[1].expression];
  assert.ok(ts.isIdentifier(identifierNodes[0]) && ts.isIdentifier(identifierNodes[1]) && identifierNodes[0] !== identifierNodes[1]);
  const make = index => {
    const name = row.kind === "node-derived-same-node" ? factory.getGeneratedNameForNode(identifierNodes[0])
      : row.kind === "node-derived-other-node" ? factory.getGeneratedNameForNode(identifierNodes[index])
      : row.kind === "temp" ? factory.getGeneratedNameForNode(sourceFile.statements[index])
      : generatedName(factory, row.kind, row.base, sourceFile);
    return declaration(factory, name);
  };
  const u1 = make(0), u2 = make(1);
  let fault = null;
  const handlers = {
    isEmitNotificationEnabled: node => node.kind === K.VariableStatement,
    onEmitNode(hint, node, callback) {
      if (fault === "before" && node === u1) { fault = null; throw new Error("printer-failure:before:FirstStatement:1"); }
      callback(hint, node);
      if (fault === "after" && node === u1) { fault = null; throw new Error("printer-failure:after:FirstStatement:1"); }
    },
  };
  const shared = ts.createPrinter(CRLF, handlers);
  const results = [];
  const op = (printer, node, index) => {
    try { results.push({ op: index, status: "returned", ...printed(printer.printNode(ts.EmitHint.Unspecified, node, sourceFile)) }); }
    catch (error) { results.push({ op: index, status: "threw", error: error.message }); }
  };
  op(ts.createPrinter(CRLF, handlers), u1, 0);
  fault = row.phase;
  op(shared, u1, 1);
  assert.equal(fault, null, "fault armed and consumed");
  op(shared, u2, 2);
  op(ts.createPrinter(CRLF, handlers), u2, 3);
  return { results };
}

function runDirect() {
  const destination = scratchOut ?? path.join(root, "crates/emitter/tests/fixtures/decorator-binding-direct.json");
  if (action === "--write" && !scratchOut) assert.ok(!fs.existsSync(destination), "retain existing observations");
  const rows = [...syntheticRows, ...globalRows, ...lifecycleRows].filter(row => selected(row.case_id));
  assert.equal(new Set(rows.map(row => row.case_id)).size, rows.length);
  assert.ok(rows.length, "selection matched no case");
  const observe = row => row.group === "synthetic" ? observeSynthetic(row) : row.group === "global" ? observeGlobal(row) : observeLifecycle(row);
  const cases = rows.map(row => {
    const first = observe(row);
    assert.deepEqual(observe(row), first, row.case_id);
    console.log(JSON.stringify({ case_id: row.case_id, attempts: 2 }));
    return { ...row, typescript_observation: first };
  });
  const artifact = { version: 1, route: "direct-generated-name-controls", typescript: ts.version,
    source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
    compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
    observer_sha256: sha256(fs.readFileSync(import.meta.filename)), selection: needles.length ? needles : null,
    repetitions: 2, groups: { synthetic: syntheticRows.length, global: globalRows.length, lifecycle: lifecycleRows.length }, cases };
  const json = JSON.stringify(artifact, null, 2) + "\n";
  if (action === "--write") fs.writeFileSync(destination, json);
  else assert.deepEqual(JSON.parse(fs.readFileSync(destination, "utf8")), artifact);
  console.log(JSON.stringify({ destination, cases: cases.length, groups: artifact.groups, sha256: sha256(fs.readFileSync(destination)) }));
}

if (mode === "pipeline") runPipeline(); else runDirect();
