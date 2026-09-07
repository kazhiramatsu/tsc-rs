// Complete H2.7d/e Bundle map references. Public outFile admission is separate.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const observerPath = "scripts/observe-bundle-maps.mjs";
const fixturePath = "crates/emitter/tests/fixtures/bundle-maps.json";
const baselinePath = "crates/emitter/tests/fixtures/bundle-declarations.json";
const sha256 = value => crypto.createHash("sha256").update(value).digest("hex");
const identity = name => ({ path: name, sha256: sha256(fs.readFileSync(path.join(root, name))) });
const baseline = JSON.parse(fs.readFileSync(path.join(root, baselinePath), "utf8"));
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
assert.ok(["--write", "--check"].includes(process.argv[2]));
const defaults = { target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.None, outFile: "/project/dist/bundle.js",
  declaration: true, declarationMap: true, sourceMap: true, strict: false, alwaysStrict: false,
  newLine: ts.NewLineKind.LineFeed, noErrorTruncation: true, skipDefaultLibCheck: true, listEmittedFiles: true };
const inputs = [];
function add(case_id, files, options = {}, extra = {}) {
  inputs.push({ case_id, current_directory: "/project", use_case_sensitive_file_names: true,
    files: Object.entries(files).map(([name, text]) => ({ path: "/project/" + name, text })),
    options: { ...defaults, ...options }, origin: { kind: "new-control" }, ...extra });
}
add("registration/prologue-second", { "src/a.ts": "const a: number = 1;\n", "src/b.ts": '"second";\nconst b: number = 2;\n', "src/c.ts": '"second";\nconst c: number = 3;\n' });
add("registration/duplicates-and-empty", { "src/a.ts": '"one";\n', "src/b.ts": "", "src/c.ts": '"one";\n' });
add("registration/erased-first", { "src/a.ts": "interface A { value: number }\n", "src/b.ts": '"two";\nconst b: number = 2;\n' });
add("comments/unicode-crlf-bom", { "src/a.ts": '/** 文😀 */\nconst a: string = "文😀"; // a\n', "src/b.ts": '/** 二🌸 */\nconst b: number = 2; // b\n' }, { newLine: ts.NewLineKind.CarriageReturnLineFeed, emitBOM: true });
add("references/shared-header", { "src/a.ts": '/// <reference path="../types/shared.d.ts" preserve="true" />\nconst a: Shared = { n: 1 };\n', "src/b.ts": '/// <reference path="../types/shared.d.ts" preserve="true" />\nconst b: Shared = { n: 2 };\n', "types/shared.d.ts": "interface Shared { n: number }\n" }, {}, { roots: ["/project/src/a.ts", "/project/src/b.ts"] });
add("registration/empty-declaration-only", { "src/a.ts": "", "src/b.ts": "" }, { emitDeclarationOnly: true });
const two = { "src/a.ts": "const a: number = 1;\n", "src/b.ts": "const b: number = 2;\n" };
add("options/inline-sources", two, { inlineSources: true });
add("options/inline-map", two, { sourceMap: false, inlineSourceMap: true });
add("helpers/shared-es5", { "src/a.ts": "declare const value: { n: number };\nconst a = { ...value };\n", "src/b.ts": "const b = { ...value };\n" }, { target: ts.ScriptTarget.ES5 });
assert.equal(inputs.length, 9);
for (const original of baseline.cases.filter(row => row.origin.kind === "original-corpus")) {
  inputs.push({ case_id: original.case_id, current_directory: original.current_directory,
    use_case_sensitive_file_names: original.use_case_sensitive_file_names, files: original.files,
    roots: original.roots, options: original.options, origin: original.origin });
}
assert.equal(inputs.length, 12);
function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function resultRecord(result) {
  return { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
    emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps?.map(entry => ({
      input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap) })) ?? null };
}
function writeRecord(args, index) {
  const [fileName, text, bom, onError, sourceFiles, data] = args;
  assert.ok(!text.includes(root));
  assert.ok(data === undefined || Object.keys(data).every(key => ["sourceMapUrlPos", "diagnostics", "buildInfo"].includes(key)));
  const bytes = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239,187,191]), bytes]) : bytes;
  return { index, path: fileName, kind: ts.isDeclarationFileName(fileName) ? "declaration" : fileName.endsWith(".map") ? "source-map" : "javascript",
    callback_utf8_base64: bytes.toString("base64"), callback_utf8_bytes: bytes.length, write_byte_order_mark: bom,
    materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sourceFiles?.map(file => file.fileName) ?? null,
    data_present: data !== undefined, data_keys: data === undefined ? null : Object.keys(data),
    data_source_map_url_pos: data?.sourceMapUrlPos ?? null, data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null,
    data_build_info: data?.buildInfo ?? null };
}
function makeProgram(input) {
  const cwd = input.current_directory;
  const canonical = name => { const absolute = ts.getNormalizedAbsolutePath(name, cwd); return input.use_case_sensitive_file_names ? absolute : absolute.toLowerCase(); };
  const files = new Map(input.files.map(file => [canonical(file.path), file.text]));
  const libraryRoot = path.join(root, "vendor/typescript-6.0.3/lib");
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name) && fs.existsSync(path.join(libraryRoot, path.basename(name)));
  const read = name => files.get(canonical(name)) ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(input.files.map(file => file.path), { currentDirectory: cwd,
    useCaseSensitiveFileNames: input.use_case_sensitive_file_names,
    fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  const host = { ...ts.createCompilerHost(input.options, true), ...overlay, getCurrentDirectory: () => cwd,
    getCanonicalFileName: canonical, useCaseSensitiveFileNames: () => input.use_case_sensitive_file_names,
    getDefaultLibFileName: () => "/lib/" + ts.getDefaultLibFileName(input.options), getDefaultLibLocation: () => "/lib",
    readFile: read, fileExists: name => read(name) !== undefined, realpath: ts.normalizePath,
    writeFile() { assert.fail("unrecorded write"); },
    getSourceFile(name, version) { const text = read(name); return text === undefined ? undefined
      : ts.createSourceFile(name, text, version, true, ts.getScriptKindFromFileName(name)); } };
  const program = ts.createProgram(input.roots ?? input.files.map(file => file.path), input.options, host);
  for (const file of program.getSourceFiles()) assert.ok(files.has(canonical(file.fileName)) || library(file.fileName), file.fileName);
  return { program, library };
}
function observe(input, probeMode = null) {
  const { program, library } = makeProgram(input);
  const writes = [];
  const parseNodes = new Map();
  for (const file of program.getSourceFiles().filter(file => !library(file.fileName))) {
    let index = 0;
    const visit = node => { parseNodes.set(node, { file: file.fileName, index: index++ }); ts.forEachChild(node, visit); };
    visit(file);
  }
  const describe = node => ({ kind: ts.SyntaxKind[node.kind], pos: node.pos, end: node.end,
    flags: ts.getEmitFlags(node), parsed_identity: parseNodes.get(node) ?? null,
    original_parsed_identity: parseNodes.get(node.original) ?? null,
    source_map_range: { pos: ts.getSourceMapRange(node).pos, end: ts.getSourceMapRange(node).end },
    type_node: node.emitNode?.typeNode ? { kind: ts.SyntaxKind[node.emitNode.typeNode.kind],
      parsed_identity: parseNodes.get(node.emitNode.typeNode) ?? null } : null });
  const parsedMetadata = () => [...parseNodes].filter(([node]) => ts.getEmitFlags(node) || node.emitNode?.typeNode)
    .map(([node]) => describe(node));
  const phases = [];
  const hook = phase => () => node => {
    const nodes = [];
    const visit = current => {
      if (ts.isParameter(current)) nodes.push({ node: describe(current), name: describe(current.name) });
      if (ts.isCallExpression(current)) nodes.push({ node: describe(current), callee_kind: ts.SyntaxKind[current.expression.kind] });
      if (ts.isBundle(current)) current.sourceFiles.forEach(visit); else ts.forEachChild(current, visit);
    };
    visit(node);
    phases.push({ phase, nodes, parsed_metadata: parsedMetadata() });
    return node;
  };
  let result = null, exception = null;
  try {
    const force = probeMode === "fresh-forced";
    result = resultRecord(program.emit(undefined, (...args) => writes.push(writeRecord(args, writes.length)),
      undefined, force, probeMode ? { after: [hook("after-js")], afterDeclarations: [hook("after-declarations")] } : undefined, force));
  }
  catch (error) { exception = { name: error.name, message: error.message }; }
  return { program_source_order: program.getSourceFiles().filter(file => !library(file.fileName)).map(file => file.fileName),
    common_source_directory: program.getCommonSourceDirectory(), writes, emit_result: result, exception,
    options_diagnostics: program.getOptionsDiagnostics().map(diagnostic),
    syntactic_diagnostics: program.getSyntacticDiagnostics().map(diagnostic),
    global_diagnostics: program.getGlobalDiagnostics().map(diagnostic),
    semantic_diagnostics: program.getSemanticDiagnostics().map(diagnostic),
    ...(probeMode ? { phases } : {}) };
}
const cases = inputs.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  assert.equal(first.exception, null, input.case_id);
  for (const write of first.writes) {
    assert.equal(Buffer.from(write.callback_utf8_base64, "base64").length, write.callback_utf8_bytes);
    assert.equal(Buffer.from(write.materialized_utf8_base64, "base64").length, write.materialized_utf8_bytes);
  }
  if (input.origin.kind === "original-corpus") {
    const original = baseline.cases.find(row => row.case_id === input.case_id);
    for (const key of ["files", "roots", "options", "current_directory", "use_case_sensitive_file_names"]) assert.deepEqual(input[key], original[key]);
    assert.deepEqual(first.writes, original.ordinary_declaration_tree_reference.writes);
    assert.deepEqual(first.emit_result, original.ordinary_declaration_tree_reference.emit_result);
  }
  return { ...input, input_sha256: sha256(JSON.stringify(input)), typescript_observation: first };
});
const ordered = cases.find(row => row.case_id === "registration/prologue-second").typescript_observation;
assert.deepEqual(ordered.emit_result.source_maps[0].input_source_file_names, ["/project/src/b.ts", "/project/src/a.ts", "/project/src/c.ts"]);
assert.deepEqual(ordered.writes[0].source_files, ["/project/src/a.ts", "/project/src/b.ts", "/project/src/c.ts"]);
assert.deepEqual(ordered.emit_result.source_maps[1].input_source_file_names, ["/project/src/a.ts", "/project/src/b.ts", "/project/src/c.ts"]);
const producerInputs = inputs.filter(input => input.origin.kind === "original-corpus" || input.case_id === "helpers/shared-es5");
for (const target of [ts.ScriptTarget.ES5, ts.ScriptTarget.ES2015]) {
  producerInputs.push({ ...inputs[0], case_id: `metadata/parameter-property/${target}`,
    files: [{ path: "/project/src/a.ts", text: "class A { constructor(public value: number) {} }\n" },
      { path: "/project/src/b.ts", text: "class B { constructor(public name: string) {} }\n" }],
    options: { ...defaults, target }, origin: { kind: "new-producer-control" } });
}
const metadataLifetimeReferences = producerInputs.map(input => {
  const modes = Object.fromEntries(["ordinary", "fresh-forced"].map(mode => {
    const first = observe(input, mode);
    assert.deepEqual(observe(input, mode), first, `${input.case_id} ${mode}`);
    assert.equal(first.exception, null);
    const old = cases.find(row => row.case_id === input.case_id);
    if (mode === "ordinary" && old) {
      assert.deepEqual(first.writes, old.typescript_observation.writes);
      assert.deepEqual(first.emit_result, old.typescript_observation.emit_result);
    }
    return [mode, first];
  }));
  return { ...input, owner: "API1 identity-hook reference only", modes };
});
for (const row of metadataLifetimeReferences.filter(row => row.origin.kind === "original-corpus")) {
  const params = mode => row.modes[mode].phases.filter(phase => phase.phase === "after-declarations")
    .flatMap(phase => phase.nodes).filter(node => node.node.kind === "Parameter");
  assert.ok(params("ordinary").some(node => node.name.flags === 64 && node.name.parsed_identity !== null));
  assert.ok(params("fresh-forced").every(node => node.name.flags === 0));
}
const artifact = { schema: 1, kind: "bundle-maps", status: "internal-printer-reference", typescript: ts.version,
  source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8", repetitions: 2, observer: identity(observerPath),
  inputs: [baselinePath, "vendor/typescript-6.0.3/lib/typescript.js", "vendor/typescript-6.0.3/lib/_tsc.js", "crates/oracle/vfs-directory-overlay.mjs", ".node-version"].map(identity),
  contract: "Complete ordinary Program.emit tuples on two fresh Programs. The three original compound inputs and ordinary write/result tuples join the frozen declaration observations unchanged. Internal Rust recorder comparison is separate from public outFile, cold getter/forced APIs, full command acceptance, and root-option owner admission.",
  cases, metadata_lifetime_references: metadataLifetimeReferences,
  summary: { new_controls: 9, unchanged_original_compounds: 3, fresh_programs: 24,
    producer_inputs: 6, producer_fresh_programs: 24,
    writes_per_repetition: cases.reduce((sum, row) => sum + row.typescript_observation.writes.length, 0), runtime_admitted: 0 } };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
assert.ok(!rendered.includes(root));
if (process.argv[2] === "--write") fs.writeFileSync(path.join(root, fixturePath), rendered);
else assert.equal(fs.readFileSync(path.join(root, fixturePath), "utf8"), rendered, "Bundle map fixture is stale");
console.log(JSON.stringify(artifact.summary));
