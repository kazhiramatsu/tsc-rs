// Complete bundle-declaration observations; no Rust runtime admission.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const observerPath = "scripts/observe-bundle-declarations.mjs";
const fixturePath = "crates/emitter/tests/fixtures/bundle-declarations.json";
const sha256 = value => crypto.createHash("sha256").update(value).digest("hex");
const identity = name => ({ path: name, sha256: sha256(fs.readFileSync(path.join(root, name))) });
const readJson = name => JSON.parse(fs.readFileSync(path.join(root, name), "utf8"));
const planPath = "crates/emitter/tests/fixtures/bundle-plan.json";
const modulesPath = "crates/emitter/tests/fixtures/bundle-module-identities.json";
const originalInputsPath = "ratchets/h2-7de-candidate-inputs.v1.json";
const originalObservationsPath = "ratchets/h2-7de-observations.v1.json";
const plans = readJson(planPath), modules = readJson(modulesPath), originalInputs = readJson(originalInputsPath), originalObservations = readJson(originalObservationsPath);
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
assert.ok(["--write", "--check"].includes(process.argv[2]));
assert.equal(plans.cases.length, 55); assert.equal(modules.cases.length, 24);
for (const baseline of [plans, modules]) { assert.equal(baseline.repetitions, 2); assert.equal(baseline.compiler_sha256, identity("vendor/typescript-6.0.3/lib/typescript.js").sha256); }
const defaults = { target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.AMD, moduleResolution: ts.ModuleResolutionKind.Node10,
  declaration: true, outFile: "/project/dist/bundle.js", listEmittedFiles: true, ignoreDeprecations: "6.0",
  strict: true, skipDefaultLibCheck: true, noErrorTruncation: true, newLine: ts.NewLineKind.LineFeed };
const command = () => ({ kind: "ordinary-command", target_source: null });
const getter = (target = null) => ({ kind: "declaration-diagnostics", target_source: target });
const forced = (target = null) => ({ kind: "forced-declarations", target_source: target });
const sequence = target => [command(), getter(), getter(target), getter(), forced(), forced(target)];
const inputs = [];
function add(case_id, files, options = {}, extra = {}) {
  inputs.push({ case_id, current_directory: "/project", use_case_sensitive_file_names: true,
    files: Object.entries(files).map(([name, text]) => ({ path: "/project/" + name, text })),
    options: { ...defaults, ...options }, calls: [command()], owners: ["H2.7d"], origin: { kind: "new-control" }, ...extra });
}
const refs = {
  "src/a.ts": '/// <reference path="./inside.ts" preserve="true" />\n/// <reference path="../types/shared.d.ts" preserve="true" />\n/// <reference path="../types/shared.d.ts" preserve="true" />\n/// <reference path="../types/dropped.d.ts" />\nconst a: Shared = { n: inside };\n',
  "src/b.ts": '/// <reference path="../types/shared.d.ts" preserve="true" />\n/// <reference path="../types/other.d.ts" preserve="true" />\nconst b: Shared = { n: 2 };\n',
  "src/inside.ts": 'const inside: number = 1;\n', "types/shared.d.ts": 'interface Shared { n: number }\n', "types/dropped.d.ts": 'interface Dropped {}\n', "types/other.d.ts": 'interface Other {}\n',
};
const roots = ["/project/src/a.ts", "/project/src/b.ts"];
add("references/path-shared", refs, {}, { roots, calls: sequence(roots[0]) });
add("references/path-reverse", refs, {}, { roots: [...roots].reverse() });
add("references/path-relocated", refs, { outFile: "/project/out/nested/bundle.js", declarationDir: "/project/ignored", emitBOM: true, newLine: 0 }, { roots });
add("references/path-excluded-module", { "src/main.ts": '/// <reference path="./external.ts" preserve="true" />\nconst main: number = 1;\n',
  "src/external.ts": 'export interface External { n: number }\n' }, { module: ts.ModuleKind.None }, { roots: ["/project/src/main.ts"], calls: sequence("/project/src/external.ts") });
add("references/path-missing", { ...refs, "src/b.ts": '/// <reference path="../types/missing.d.ts" preserve="true" />\n' + refs["src/b.ts"] }, {}, { roots });
const types = {
  "src/a.ts": '/// <reference types="shared" resolution-mode="import" preserve="true" />\n/// <reference lib="es2015" preserve="true" />\nconst a: SharedType = { n: 1 };\n',
  "src/b.ts": '/// <reference types="shared" resolution-mode="require" preserve="true" />\n/// <reference types="shared" resolution-mode="import" preserve="true" />\n/// <reference lib="es2015" preserve="true" />\n/// <reference lib="es2016" />\nconst b: SharedType = { n: 2 };\n',
  "node_modules/@types/shared/index.d.ts": 'interface SharedType { n: number }\n',
};
add("references/type-lib-shared", types, {}, { roots });
add("references/type-lib-reverse", types, {}, { roots: [...roots].reverse() });
add("references/amd-no-default-lib", { ...types,
  "src/a.ts": '/// <amd-module name="public/a" />\n/// <reference no-default-lib="true" />\n' + types["src/a.ts"] + 'export { a };\n',
  "src/b.ts": '/// <amd-module name="public/b" />\n' + types["src/b.ts"] + 'export { b };\n' }, {}, { roots });
const mixed = { "src/global.ts": 'const globalValue: number = 1;\n', "src/module.ts": 'export const moduleValue: number = 2;\n',
  "src/common.js": '/** @param {number} value */\nfunction make(value) { return { value }; }\nexports.make = make;\n',
  "src/data.json": '{"value":1,"name":"文😀"}\n' };
add("wrappers/mixed-amd", mixed, { allowJs: true, checkJs: true, resolveJsonModule: true },
  { calls: [command(), getter(), forced(), forced("/project/src/data.json")] });
add("wrappers/mixed-system", mixed, { allowJs: true, checkJs: true, resolveJsonModule: true, module: ts.ModuleKind.System },
  { roots: Object.keys(mixed).reverse().map(name => "/project/" + name) });
add("wrappers/ambient-augmentation", { "src/ambient.ts": 'declare module "pkg" { export interface Shape { size: number } }\n',
  "src/dep.ts": 'export interface Shape { value: number }\n',
  "src/main.ts": 'import { Shape } from "./dep"; declare module "./dep" { interface Shape { extra: string } } export const value: Shape = { value: 1, extra: "x" };\n' });
add("wrappers/strip-all", { "src/module.ts": '/** @internal */ export const removed: number = 1;\n',
  "src/global.ts": '/** @internal */ const removedGlobal: number = 2;\n' }, { stripInternal: true });
const late = { "src/painted.ts": 'class Hidden { value: number = 1; }\nexport function make(): Hidden { return new Hidden(); }\n',
  "src/plain.ts": 'export interface Visible { value: number }\n', "src/global.ts": 'class Global { value: number = 3; }\n' };
add("state/late-painted-first", late, {}, { calls: sequence("/project/src/painted.ts") });
add("state/late-painted-last", late, {}, { roots: Object.keys(late).reverse().map(name => "/project/" + name), calls: sequence("/project/src/plain.ts") });
add("state/scope-marker-isolation", { "src/marker.ts": 'class Local {}\nexport const value: Local = new Local();\nexport {};\n',
  "src/erased.ts": 'export {};\n', "src/plain.ts": 'export interface Public { n: number }\n' }, {}, { calls: sequence("/project/src/plain.ts") });
const errors = { "src/good.ts": 'export const good: number = 1;\n',
  "src/zbad.ts": 'export const zbad = class { private field: number = 1; };\n',
  "src/abad.ts": 'export const abad = class { protected field: number = 2; };\n' };
const errorCalls = [command(), getter(), getter("/project/src/zbad.ts"), getter(), forced(), forced("/project/src/good.ts"), forced("/project/src/abad.ts"), getter()];
add("diagnostics/multiple-files", errors, {}, { calls: errorCalls });
add("diagnostics/no-emit-on-error", errors, { noEmitOnError: true }, { calls: errorCalls });
add("diagnostics/semantic-gate", { "src/good.ts": 'export const good: number = "wrong";\n' }, { noEmitOnError: true }, { calls: sequence("/project/src/good.ts") });
assert.equal(inputs.length, 18);
function fromPlan(id, owners = ["H2.7d"]) {
  const row = plans.cases.find(row => row.case_id === id); assert.ok(row); assert.equal(row.target_source, undefined);
  return { case_id: "sequence/" + id, current_directory: row.current_directory, use_case_sensitive_file_names: row.use_case_sensitive_file_names,
    files: row.files, options: row.options, roots: row.roots ?? row.files.map(file => file.path),
    calls: [command(), getter(), getter(), forced(), ...(row.files.length ? [forced(row.files[0].path)] : [])],
    owners, origin: { kind: "bundle-plan", case_id: id } };
}
for (const id of ["adjacent/empty-program", "adjacent/declaration-only-input", "adjacent/declaration-collision#false", "adjacent/declaration-collision#true"]) inputs.push(fromPlan(id));
for (const name of ["declarationMapsOutFile.ts", "declarationMapsOutFile2.ts", "declarationMapsWithSourceMap.ts"]) {
  const case_id = "typescript-6.0.3/compiler/" + name + "#default";
  const row = originalInputs.cases.find(row => row.case_id === case_id); assert.ok(row);
  assert.equal(row.input.config, null); assert.equal(row.input.shared_mount, null); assert.deepEqual(row.input.vfs_symlinks, []);
  inputs.push({ case_id, current_directory: row.input.current_directory, use_case_sensitive_file_names: row.input.use_case_sensitive_file_names,
    files: row.input.files, roots: row.input.roots, options: row.effective_options,
    calls: [command(), getter(), forced(), forced(row.input.roots[0])], owners: ["H2.7d", "H2.7e"],
    origin: { kind: "original-corpus", case_id, original_input_sha256: sha256(JSON.stringify(row)) } });
}
assert.equal(inputs.length, 25);
inputs.push(fromPlan("adjacent/no-emit", ["H2.9"]));
const targetReference = structuredClone(inputs[0]); targetReference.case_id = "reference/ordinary-target";
 targetReference.owners = ["H2.8d"]; targetReference.origin = { kind: "new-control-reference", source_case_id: inputs[0].case_id };
 targetReference.calls = [{ kind: "ordinary-target", target_source: "/project/src/a.ts" }]; inputs.push(targetReference);
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
function observeSequence(input) {
  const { program, library } = makeProgram(input);
  const resolverRequests = [], checker = program.getTypeChecker(), getResolver = checker.getEmitResolver.bind(checker);
  checker.getEmitResolver = (source, token, skip) => {
    resolverRequests.push({ source_file: source?.fileName ?? null, skip_diagnostics: skip ?? null });
    return getResolver(source, token, skip);
  };
  const calls = [];
  for (const call of input.calls) {
    const result = { ...call, writes: [], diagnostics: null, reported_diagnostics: null, status_writes: null,
      exit_code: null, emit_result: null, exception: null };
    const target = call.target_source ? program.getSourceFile(call.target_source) : undefined;
    if (call.target_source) assert.ok(target, call.target_source);
    const start = resolverRequests.length, sink = (...args) => result.writes.push(writeRecord(args, result.writes.length));
    try {
      if (call.kind === "declaration-diagnostics") result.diagnostics = program.getDeclarationDiagnostics(target).map(diagnostic);
      else if (call.kind === "ordinary-command") {
        const emit = program.emit; let emitted;
        program.emit = (...args) => { assert.equal(emitted, undefined); return emitted = emit(...args); };
        result.reported_diagnostics = []; result.status_writes = [];
        try { result.exit_code = ts.emitFilesAndReportErrorsAndGetExitStatus(program,
          d => result.reported_diagnostics.push(diagnostic(d)), text => result.status_writes.push(text), undefined, sink);
          assert.ok(emitted); result.emit_result = resultRecord(emitted);
        } finally { program.emit = emit; }
      } else {
        assert.ok(["forced-declarations", "ordinary-target"].includes(call.kind));
        const force = call.kind === "forced-declarations";
        result.emit_result = resultRecord(program.emit(target, sink, undefined, force || undefined, undefined, force || undefined));
      }
    } catch (error) {
      // A controlled case must name an expected upstream exception explicitly;
      // unexpected host/observer failures cannot silently become a baseline.
      assert.deepEqual({ name: error.name, message: error.message }, input.expected_exception, error.stack);
      result.exception = { name: error.name, message: error.message };
    }
    result.resolver_requests = resolverRequests.slice(start);
    if (call.kind === "declaration-diagnostics") assert.deepEqual(result.writes, []);
    calls.push(result);
  }
  const programDiagnostics = { options: program.getOptionsDiagnostics().map(diagnostic), syntactic: program.getSyntacticDiagnostics().map(diagnostic),
    global: program.getGlobalDiagnostics().map(diagnostic), semantic: program.getSemanticDiagnostics().map(diagnostic) };
  return { program_source_order: program.getSourceFiles().filter(file => !library(file.fileName)).map(file => file.fileName),
    standard_libraries: program.getSourceFiles().filter(file => library(file.fileName)).map(file => path.basename(file.fileName)),
    source_facts: program.getSourceFiles().filter(file => !library(file.fileName)).map(file => ({ path: file.fileName,
      is_declaration_file: file.isDeclarationFile, is_external_module: ts.isExternalModule(file),
      has_common_js_module_indicator: file.commonJsModuleIndicator !== undefined,
      is_javascript: (file.flags & ts.NodeFlags.JavaScriptFile) !== 0, is_json: ts.isJsonSourceFile(file),
      module_name: file.moduleName ?? null, has_no_default_lib: file.hasNoDefaultLib ?? null,
      referenced_files: file.referencedFiles, type_reference_directives: file.typeReferenceDirectives, lib_reference_directives: file.libReferenceDirectives })),
    calls, program_diagnostics_after_calls: programDiagnostics };
}
const references = value => value === undefined ? null : value.map(reference => ({ ...reference }));
function statement(node) {
  return { kind: ts.SyntaxKind[node.kind], pos: node.pos, end: node.end,
    name: node.name?.text ?? null, modifiers: node.modifiers?.map(modifier => ts.SyntaxKind[modifier.kind]) ?? null,
    declaration_names: node.declarationList?.declarations.map(declaration => declaration.name.text ?? ts.SyntaxKind[declaration.name.kind]) ?? null,
    module_specifier: node.moduleSpecifier?.text ?? null,
    empty_export: ts.isExportDeclaration(node) && node.exportClause !== undefined && ts.isNamedExports(node.exportClause) && node.exportClause.elements.length === 0,
    body: ts.isModuleDeclaration(node) && node.body && ts.isModuleBlock(node.body) ? {
      pos: node.body.statements.pos, end: node.body.statements.end, statements: node.body.statements.map(statement) } : null };
}
function sourceShape(source) {
  return { file_name: source.fileName, is_declaration_file: source.isDeclarationFile, module_name: source.moduleName ?? null,
    has_no_default_lib: source.hasNoDefaultLib ?? null, referenced_files: references(source.referencedFiles),
    type_reference_directives: references(source.typeReferenceDirectives), lib_reference_directives: references(source.libReferenceDirectives),
    statements: { pos: source.statements.pos, end: source.statements.end, nodes: source.statements.map(statement) } };
}
function observeDeclarationTree(input) {
  const { program } = makeProgram(input), roots = [], writes = [];
  const result = program.emit(undefined, (...args) => writes.push(writeRecord(args, writes.length)), undefined, true,
    { afterDeclarations: [() => node => { roots.push(ts.isBundle(node) ? { kind: "Bundle",
      synthetic_file_references: references(node.syntheticFileReferences), synthetic_type_references: references(node.syntheticTypeReferences),
      synthetic_lib_references: references(node.syntheticLibReferences), source_files: node.sourceFiles.map(sourceShape) }
      : { kind: "SourceFile", source_file: sourceShape(node) }); return node; }] }, true);
  return { owner: "API1", route: "separate-fresh-Program-forced-afterDeclarations-identity-hook", roots,
    writes, emit_result: resultRecord(result), exception: null };
}
function baselineJoin(input, observation) {
  const first = observation.calls[0];
  if (input.origin.kind === "bundle-plan") {
    const reference = plans.cases.find(row => row.case_id === input.origin.case_id).typescript_observation;
    assert.deepEqual(first.emit_result, { ...reference.emit_result, source_maps: reference.emit_result.source_maps?.map(entry => ({
      input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap) })) ?? null });
    assert.equal(first.writes.length, reference.writes.length);
    for (const [index, write] of reference.writes.entries()) for (const [key, value] of Object.entries(write)) assert.deepEqual(first.writes[index][key], value);
  } else if (input.origin.kind === "original-corpus") {
    const reference = originalObservations.cases.find(row => row.case_id === input.case_id);
    assert.equal(input.origin.original_input_sha256, reference.input_sha256);
    const original = { program_source_order: observation.program_source_order, standard_libraries: observation.standard_libraries,
      writes: first.writes, reported_diagnostics: first.reported_diagnostics, emit_refused: first.emit_result.emit_skipped,
      emit_result: first.emit_result, status_writes: first.status_writes, exit_code: first.exit_code };
    assert.deepEqual(original, reference.typescript_observation, input.case_id + " original input tuple changed");
  }
}
const rows = [];
for (const [index, input] of inputs.entries()) {
  const observation = observeSequence(input); assert.deepEqual(observeSequence(input), observation, input.case_id);
  baselineJoin(input, observation);
  const tree = observeDeclarationTree(input); assert.deepEqual(observeDeclarationTree(input), tree, input.case_id + " tree reference");
  const force = observation.calls.find(call => call.kind === "forced-declarations" && call.target_source === null);
  if (force) { assert.deepEqual(tree.writes, force.writes); assert.deepEqual(tree.emit_result, force.emit_result); }
  for (const call of observation.calls) {
    if (call.exception) assert.equal(call.emit_result, null);
    for (const name of ["source_maps", "emitted_files"]) if (call.emit_result) assert.ok(call.emit_result[name] === null || Array.isArray(call.emit_result[name]));
    for (const write of call.writes) { assert.equal(Buffer.from(write.callback_utf8_base64, "base64").length, write.callback_utf8_bytes);
      assert.equal(Buffer.from(write.materialized_utf8_base64, "base64").length, write.materialized_utf8_bytes); }
  }
  rows.push({ ...input, input_sha256: sha256(JSON.stringify(input)), typescript_observation: observation, declaration_tree_reference: tree });
  console.log(`${index + 1}/${inputs.length} ${input.case_id}`);
}
const cases = rows.filter(row => row.owners.includes("H2.7d")), adjacent = rows.filter(row => !row.owners.includes("H2.7d"));
const artifact = { schema: 1, kind: "bundle-declarations", status: "typescript-reference-only", typescript: ts.version,
  source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8", repetitions: 2, observer: identity(observerPath),
  inputs: [planPath, modulesPath, originalInputsPath, originalObservationsPath, "vendor/typescript-6.0.3/lib/typescript.js",
    "vendor/typescript-6.0.3/lib/_tsc.js", "crates/oracle/vfs-directory-overlay.mjs", ".node-version"].map(identity),
  existing_coverage: { bundle_plan_sequences: 55, module_identity_programs: 24, module_identity_api_references: 4,
    module_identity_path_helpers: 14, note: "Existing witnesses retain their original inputs/observations; new sequences reuse four plan inputs and three compound corpus inputs without changes. Module identities are referenced, not recounted as new controls." },
  execution_contract: "Every main sequence repeats on a fresh Program; calls share caches within a sequence. The diagnostic snapshot follows calls. Complete command/emit/getter results preserve bytes, BOM, order, metadata, sourceMaps/list absence, diagnostics, exceptions and partial writes. Declaration tree metadata comes from two additional fresh Programs with a forced identity afterDeclarations hook, remains API1 reference, and matches the main forced tuple wherever present. No TypeScript success becomes Rust admission. Ordinary noEmit and targeted APIs remain H2.9/H2.8d references.",
  cases, adjacent_owner_references: adjacent,
  summary: { new_controls: 18, reused_plan_sequences: 4, original_compound_sequences: 3, main_sequences: cases.length,
    adjacent_sequences: adjacent.length, calls_per_repetition: rows.reduce((sum,row) => sum + row.calls.length,0),
    exception_calls_per_repetition: rows.reduce((sum,row) => sum + row.typescript_observation.calls.filter(call => call.exception).length,0),
    fresh_sequence_programs: rows.length * 2, fresh_tree_reference_programs: rows.length * 2, runtime_admitted: 0 } };
assert.equal(cases.length,25); assert.equal(adjacent.length,2);
const rendered = JSON.stringify(artifact, null, 2) + "\n"; assert.ok(!rendered.includes(root));
if (process.argv[2] === "--write") fs.writeFileSync(path.join(root,fixturePath),rendered);
else assert.equal(fs.readFileSync(path.join(root,fixturePath),"utf8"),rendered,"bundle declaration fixture is stale");
console.log(JSON.stringify(artifact.summary));
