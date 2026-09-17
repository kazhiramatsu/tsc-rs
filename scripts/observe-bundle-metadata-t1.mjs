// H2.8a-A-RES-BUNDLE-METADATA-T1 observer: complete TypeScript 6.0.3 commands
// for the adjacent controls of `generate-bundle-metadata-t1-inputs.mjs` (same
// host / library / diagnostic / callback contract as
// `observe-decorator-bindings.mjs pipeline`; two matching executions per
// case), plus a separate fresh-Program probe of every parse node that carries
// an `emitNode` after the JavaScript transform (`customTransformers.after`)
// and after the declaration transform (`afterDeclarations`), and a fresh
// Program that emits twice (same-Program lifetime of bundle emit nodes).
//
//   → crates/compiler/tests/fixtures/bundle-metadata-t1.json
//
// usage: node scripts/observe-bundle-metadata-t1.mjs --write|--check [--case <substr>]... [--out <path>]
// `--case`/`--out` are scratch selections (never the frozen destination).
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
const [, , action] = process.argv;
assert.ok(["--write", "--check"].includes(action), "action: --write|--check");
const args = process.argv.slice(3);
const needles = [];
let scratchOut;
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--case") needles.push(args[++i]);
  else if (args[i] === "--out") scratchOut = args[++i];
  else assert.fail(`unknown argument ${args[i]}`);
}
assert.ok(!(needles.length && !scratchOut) || action === "--check", "a --case selection writes only to --out");
const selected = id => !needles.length || needles.some(needle => id.includes(needle));
const inputPath = "crates/compiler/tests/fixtures/bundle-metadata-t1-inputs.json";
const destination = scratchOut ?? path.join(root, "crates/compiler/tests/fixtures/bundle-metadata-t1.json");

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

// ------------------------------------------------------------------ probes
// Every parse node of the Program's own sources that carries an `emitNode`,
// projected to the fields the Rust parsed-metadata packet covers plus the
// remaining defined keys (`other_keys`), so a non-portable upstream field on a
// parse node is visible rather than silently absent.
const PORTABLE = new Set(["flags", "internalFlags", "commentRange", "typeNode", "constantValue", "annotatedNodes"]);
const sourceFileOf = node => { while (node && node.kind !== ts.SyntaxKind.SourceFile) node = node.parent; return node; };
function projectEmitNodes(program, input) {
  const own = new Set(input.files.map(file => file.path));
  const rows = [];
  for (const file of program.getSourceFiles()) {
    if (!own.has(file.fileName)) continue;
    const walk = node => {
      const emitNode = node.emitNode;
      if (emitNode) {
        const defined = Object.keys(emitNode).filter(key => emitNode[key] !== undefined);
        const range = emitNode.commentRange;
        rows.push({ file: file.fileName, kind: ts.SyntaxKind[node.kind], pos: node.pos, end: node.end,
          flags: emitNode.flags ?? 0, internal_flags: emitNode.internalFlags ?? 0,
          comment_range: range === undefined ? null : { pos: range.pos, end: range.end,
            node_kind: range.kind === undefined ? null : ts.SyntaxKind[range.kind],
            node_file: range.kind === undefined ? null : sourceFileOf(range)?.fileName ?? null, same_node: range === node },
          type_node: emitNode.typeNode ? ts.SyntaxKind[emitNode.typeNode.kind] : null,
          constant_value: emitNode.constantValue !== undefined,
          annotated_nodes: emitNode.annotatedNodes === undefined ? null : emitNode.annotatedNodes.length,
          other_keys: defined.filter(key => !PORTABLE.has(key)).sort() });
      }
      ts.forEachChild(node, walk);
    };
    walk(file);
  }
  return rows;
}
function probeCommand(input) {
  const program = createProgram(input);
  const phases = {};
  // A plain custom transformer is chained per source (`chainBundle`): for a
  // Bundle root every built-in transform of that phase has already run over
  // the whole bundle, so each call projects the same complete state; for
  // SourceFile roots each file is transformed, printed and disposed on its
  // own, so successive calls show that per-file lifetime. Every call's
  // projection is kept in order.
  const hook = name => () => node => { (phases[name] ??= []).push(projectEmitNodes(program, input)); return node; };
  const writes = [];
  const result = program.emit(undefined, (...args) => writes.push(write(args, writes.length)), undefined, undefined,
    { after: [hook("after_javascript")], afterDeclarations: [hook("after_declarations")] });
  const afterEmit = projectEmitNodes(program, input);
  // Same-Program lifetime: a second ordinary emit on a fresh Program that
  // already emitted once (no hooks on either emit).
  const twice = createProgram(input);
  const first = [], second = [];
  twice.emit(undefined, (...args) => first.push(write(args, first.length)));
  twice.emit(undefined, (...args) => second.push(write(args, second.length)));
  return { hooked_emit: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      writes: writes.map(w => ({ path: w.path, sha256: sha256(Buffer.from(w.callback_utf8_base64, "base64")) })) },
    after_javascript: phases.after_javascript ?? null, after_declarations: phases.after_declarations ?? null,
    after_emit: afterEmit,
    second_ordinary_emit: { first_writes: first.map(w => ({ path: w.path, sha256: sha256(Buffer.from(w.callback_utf8_base64, "base64")) })),
      identical: JSON.stringify(first) === JSON.stringify(second) } };
}

// -------------------------------------------------------------------- main
const manifest = JSON.parse(fs.readFileSync(path.join(root, inputPath), "utf8"));
assert.equal(manifest.version, 1);
assert.equal(new Set(manifest.cases.map(input => input.case_id)).size, manifest.cases.length);
const inputs = manifest.cases.filter(input => selected(input.case_id));
assert.ok(inputs.length, "selection matched no case");
if (action === "--write" && !scratchOut) assert.ok(!fs.existsSync(destination), "retain existing observations");
const started = Date.now();
const cases = inputs.map(input => {
  const first = observeCommand(input);
  assert.deepEqual(observeCommand(input), first, input.case_id);
  assert.notEqual(first.outcome, "exception", `${input.case_id}: upstream exception`);
  const probe = probeCommand(input);
  assert.deepEqual(probeCommand(input), probe, `${input.case_id} probe`);
  assert.deepEqual(probe.hooked_emit.writes.map(w => w.path).sort(), first.writes.map(w => w.path).sort(), `${input.case_id}: hooked emit writes the same paths`);
  console.log(JSON.stringify({ case_id: input.case_id, attempts: 2, complete_observations: 2, diagnostics: first.reported_diagnostics.length,
    writes: first.writes.length, exit_code: first.exit_code,
    after_javascript_calls: probe.after_javascript?.length ?? 0,
    annotated_after_javascript: probe.after_javascript?.map(rows => rows.length) ?? null,
    parsed_comment_ranges: probe.after_javascript?.map(rows => rows.filter(row => row.comment_range).length) ?? null,
    second_emit_identical: probe.second_ordinary_emit.identical }));
  return { ...input, typescript_observation: first, probe };
});
const artifact = {
  version: 1, status: "Complete upstream observations with parse-node emitNode probes; native qualification is recorded separately",
  typescript: ts.version,
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)),
  inputs: { path: inputPath, sha256: sha256(fs.readFileSync(path.join(root, inputPath))) },
  selection: needles.length ? needles : null,
  repetitions: 2, program_executions: cases.length * 2, probe_program_executions: cases.length * 4,
  probe_contract: "Per case, two fresh Programs emit with identity `after` / `afterDeclarations` hooks; each hook call (one per transformed source, `chainBundle`) projects every own-source parse node carrying an emitNode (flags, internalFlags, commentRange endpoints, typeNode kind, constantValue presence, remaining defined keys) in call order; `after_emit` is the same projection after the hooked emit returns. Two further fresh Programs emit ordinarily twice; `identical` compares both write tuples. Probes are upstream references and confer no runtime admission.",
  cases,
};
const json = JSON.stringify(artifact, null, 1) + "\n";
if (action === "--write") fs.writeFileSync(destination, json);
else {
  const frozen = JSON.parse(fs.readFileSync(destination, "utf8"));
  const compare = { ...artifact, selection: frozen.selection, program_executions: frozen.program_executions, probe_program_executions: frozen.probe_program_executions,
    cases: needles.length ? frozen.cases.filter(row => selected(row.case_id)) : frozen.cases };
  assert.deepEqual(frozen, { ...compare, cases: frozen.cases });
  for (const row of cases) assert.deepEqual(frozen.cases.find(f => f.case_id === row.case_id), row, row.case_id);
}
console.log(JSON.stringify({ destination: path.relative(root, destination), cases: cases.length,
  elapsed_seconds: (Date.now() - started) / 1000, sha256: sha256(json) }));
