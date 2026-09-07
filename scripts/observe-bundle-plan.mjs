// H2.7d planning foundation. Complete emit observations are references, not
// runtime-admission claims: Rust still refuses outFile before any sink write.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const target = path.join(root, "crates/emitter/tests/fixtures/bundle-plan.json");
const sha256 = value => crypto.createHash("sha256").update(value).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
const defaults = { target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.None,
  outFile: "/project/bundle.js", newLine: ts.NewLineKind.LineFeed,
  skipDefaultLibCheck: true, noErrorTruncation: true };
const inputs = [];
function add(case_id, files, options = {}, extra = {}) {
  inputs.push({ case_id, current_directory: "/project", use_case_sensitive_file_names: true,
    files: Object.entries(files).map(([name, text]) => ({ path: name.startsWith("/") ? name : "/project/" + name, text })),
    options: { ...defaults, ...options }, ...extra });
}
const scripts = { "z.ts": "const z: number = 1;\n", "a.ts": "const a: string = 'a';\n" };
add("adjacent/scripts#program-order", scripts);
add("adjacent/scripts#reverse-roots", scripts, {}, { roots: ["/project/a.ts", "/project/z.ts"] });
add("adjacent/scripts#target-first", scripts, {}, { target_source: "/project/z.ts" });
add("adjacent/scripts#target-last", scripts, {}, { target_source: "/project/a.ts" });
const mixed = { "z.ts": scripts["z.ts"], "mod.ts": "export const value: number = 2;\n",
  "types.d.ts": "declare const ambient: number;\n", "a.ts": scripts["a.ts"] };
for (const [name, module] of [["none", 0], ["commonjs", 1], ["amd", 2], ["umd", 3], ["system", 4], ["es2015", 5], ["preserve", 200]]) {
  add("adjacent/module-selection#" + name, mixed, { module, declaration: true });
}
add("adjacent/module-selection#declaration-only", mixed, { module: 1, declaration: true, emitDeclarationOnly: true });
add("adjacent/module-selection#declaration-only-without-declaration", mixed, { module: 1, declaration: false, emitDeclarationOnly: true });
add("adjacent/module-selection#target-excluded-module", mixed, {}, { target_source: "/project/mod.ts" });
add("adjacent/module-selection#target-declaration", mixed, {}, { target_source: "/project/types.d.ts" });
add("adjacent/module-selection#only-external-module", { "mod.ts": mixed["mod.ts"] });
add("adjacent/declaration-only-input", { "types.d.ts": mixed["types.d.ts"] }, { declaration: true });
add("adjacent/empty-program", {}, { declaration: true });
for (const outFile of ["bundle.js", "./bundle.js", "out/../bundle.js", "../bundle.js", "bundle.d.ts", "bundle.mjs", "bundle.json", "bundle.output", "bundle.JS", "bundle", ".ts", ".d.ts"]) {
  add("adjacent/output-spelling#" + outFile, scripts, { outFile, declaration: true });
}
add("adjacent/empty-out-file", scripts, { outFile: "", declaration: true });
for (const [name, options] of Object.entries({ external: { sourceMap: true }, inline: { inlineSourceMap: true },
  both: { sourceMap: true, inlineSourceMap: true }, declaration: { declaration: true, declarationMap: true },
  only: { declaration: true, emitDeclarationOnly: true, sourceMap: true, declarationMap: true },
  "forced-without-declaration": { declaration: false, declarationMap: true },
  "declaration-dir-ignored": { declaration: true, declarationDir: "/project/types" },
  "out-dir-ignored": { declaration: true, outDir: "/project/js" },
  "emit-bom-and-list": { declaration: true, emitBOM: true, listEmittedFiles: true } })) {
  add("adjacent/output-members#" + name, scripts, options);
}
for (const noEmitOnError of [false, true]) {
  add("adjacent/declaration-collision#" + noEmitOnError,
    { ...scripts, "bundle.d.ts": "declare const existing: string;\n" }, { declaration: true, noEmitOnError });
}
add("adjacent/javascript-collision", { "bundle.js": "const existing = 1;\n", "a.ts": scripts["a.ts"] }, { allowJs: true, declaration: true });
add("adjacent/same-js-declaration-path", scripts, { outFile: "/project/bundle.d.ts", declaration: true });
add("adjacent/relative-collision", { ...scripts, "bundle.d.ts": "declare const existing: string;\n" }, { outFile: "./bundle.js", declaration: true });
add("adjacent/case-insensitive-collision", { ...scripts, "BUNDLE.d.ts": "declare const existing: string;\n" }, { declaration: true }, { use_case_sensitive_file_names: false });
add("adjacent/no-emit", scripts, { noEmit: true, declaration: true });
add("adjacent/javascript-selection", { "one.js": "var one = 1;\n", ...scripts }, { allowJs: true, declaration: true });
add("adjacent/javascript-selection#disabled", { "one.js": "var one = 1;\n", ...scripts }, { allowJs: true, noEmitForJsFiles: true, declaration: true });
add("adjacent/json-selection", { "data.json": '{"value":1}\n', ...scripts }, { resolveJsonModule: true, moduleResolution: 2, module: 2, declaration: true });
add("adjacent/json-selection#empty-out-file", { "data.json": '{"value":1}\n', ...scripts }, { outFile: "", resolveJsonModule: true, moduleResolution: 2, module: 2, declaration: true });
add("adjacent/write-failure", scripts, { declaration: true, listEmittedFiles: true }, { fail_write_path: "/project/bundle.js" });

// These corpus inputs are joined from the independently rebuilt candidate
// census. Keep effective options, virtual bytes, root order, and cwd intact.
const candidatePath = "ratchets/h2-7de-candidate-inputs.v1.json";
const checked = spawnSync(process.execPath, ["crates/oracle/h2-7de-candidates.mjs", "--check"], { cwd: root, encoding: "utf8" });
assert.equal(checked.status, 0, checked.stdout + checked.stderr);
const candidateBytes = fs.readFileSync(path.join(root, candidatePath));
const candidates = JSON.parse(candidateBytes);
for (const name of ["blockScopedClassDeclarationAcrossFiles.ts", "constDeclarations-useBeforeDefinition2.ts", "declarationFileOverwriteErrorWithOut.ts"]) {
  const case_id = "typescript-6.0.3/compiler/" + name + "#default";
  const row = candidates.cases.find(row => row.case_id === case_id);
  assert.ok(row);
  assert.equal(row.input.config, null);
  assert.equal(row.input.shared_mount, null);
  assert.deepEqual(row.input.vfs_symlinks, []);
  const bytes = fs.readFileSync(path.join(root, "ts-tests/tests/cases/compiler", row.source.path));
  assert.equal(bytes.length, row.source.bytes);
  assert.equal(sha256(bytes), row.source.sha256);
  inputs.push({ case_id, ...row.input, options: row.effective_options, source: row.source });
}
assert.equal(inputs.length, 55);

function diagnostic(value) {
  return { code: value.code, category: ts.DiagnosticCategory[value.category], file: value.file?.fileName ?? null,
    start: value.start ?? null, length: value.length ?? null, message: ts.flattenDiagnosticMessageText(value.messageText, "\n"),
    related_information: value.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [fileName, text, bom, onError, sourceFiles, data] = args;
  const callback = Buffer.from(text, "utf8");
  const materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), callback]) : callback;
  return { index, path: fileName, kind: fileName.endsWith(".map") ? "source-map"
    : ts.isDeclarationFileName(fileName) ? "declaration" : "javascript",
    callback_utf8_base64: callback.toString("base64"), callback_utf8_bytes: callback.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sourceFiles?.map(file => file.fileName) ?? null,
    data_present: data !== undefined, data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null };
}
function observe(input) {
  const canonical = name => input.use_case_sensitive_file_names ? ts.normalizePath(name) : ts.normalizePath(name).toLowerCase();
  const files = new Map(input.files.map(file => [canonical(file.path), file.text]));
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name);
  const read = name => files.get(canonical(name)) ?? (library(name)
    ? fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib", path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(input.files.map(file => file.path), {
    currentDirectory: input.current_directory, useCaseSensitiveFileNames: input.use_case_sensitive_file_names,
    fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  const host = { ...ts.createCompilerHost(input.options, true), ...overlay,
    getCurrentDirectory: () => input.current_directory, useCaseSensitiveFileNames: () => input.use_case_sensitive_file_names,
    getCanonicalFileName: canonical, getDefaultLibFileName: () => "/lib/" + (input.default_library_file_name ?? ts.getDefaultLibFileName(input.options)), getDefaultLibLocation: () => "/lib", readFile: read,
    fileExists: name => files.has(canonical(name)) || library(name), realpath: ts.normalizePath,
    getSourceFile(name, languageVersion) { const text = read(name); return text === undefined ? undefined
      : ts.createSourceFile(name, text, languageVersion, true, ts.getScriptKindFromFileName(name)); } };
  const program = ts.createProgram(input.roots ?? input.files.map(file => file.path), input.options, host);
  const target = input.target_source ? program.getSourceFile(input.target_source) : undefined;
  if (input.target_source) assert.ok(target);
  const emitHost = { ...program, getCanonicalFileName: canonical, useCaseSensitiveFileNames: () => input.use_case_sensitive_file_names };
  const paths = value => ({ javascript: value.jsFilePath ?? null, javascript_map: value.sourceMapFilePath ?? null,
    declaration: value.declarationFilePath ?? null, declaration_map: value.declarationMapPath ?? null, build_info: value.buildInfoPath ?? null });
  const plan = force => {
    const units = [];
    ts.forEachEmittedFile(emitHost, (output, root) => { units.push({ root_kind: ts.isBundle(root) ? "bundle" : "source-file",
      source_files: ts.isBundle(root) ? root.sourceFiles.map(source => source.fileName) : [root.fileName], paths: paths(output) }); }, target, force);
    return units;
  };
  const planObservation = { source_files: ts.getSourceFilesToEmit(emitHost, target).map(source => source.fileName), units: plan(false),
    forced_source_files: ts.getSourceFilesToEmit(emitHost, target, true).map(source => source.fileName), forced_units: plan(true),
    preflight_diagnostics: program.getOptionsDiagnostics().filter(value => [5055, 5056].includes(value.code)).map(diagnostic) };
  const emit = force => {
    const writes = [];
    try {
      const result = program.emit(target, (...args) => { writes.push(write(args, writes.length));
        if (args[0] === input.fail_write_path) args[3]("H2.7d controlled write failure"); }, undefined, force || undefined, undefined, force || undefined);
      return { writes, exception: null, emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
        emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps ?? null } };
    } catch (error) {
      assert.equal(input.case_id, "adjacent/output-members#forced-without-declaration");
      assert.ok(force);
      assert.equal(error.message, "Debug Failure.");
      return { writes, exception: { name: error.name, message: error.message }, emit_result: null };
    }
  };
  const ordinary = emit(false);
  const forced = emit(true);
  return { program_sources: program.getSourceFiles().map(source => ({ path: source.fileName,
    is_external_module: ts.isExternalModule(source), may_be_emitted: !source.isDeclarationFile && !program.isSourceFileFromExternalLibrary(source)
      && !program.isSourceOfProjectReferenceRedirect(source.fileName),
    may_emit_forced_declaration: !source.isDeclarationFile && !program.isSourceFileFromExternalLibrary(source) })),
    common_source_directory: program.getCommonSourceDirectory(), planning_observation: planObservation,
    typescript_observation: { ...ordinary, options_diagnostics: program.getOptionsDiagnostics().map(diagnostic),
      syntactic_diagnostics: program.getSyntacticDiagnostics().map(diagnostic), global_diagnostics: program.getGlobalDiagnostics().map(diagnostic),
      semantic_diagnostics: program.getSemanticDiagnostics().map(diagnostic) }, forced_typescript_observation: forced };
}
const cases = inputs.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  return { ...input, ...first };
});
const artifact = { version: 1, typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))), repetitions: 2,
  candidate_inputs: { path: candidatePath, sha256: sha256(candidateBytes) }, focused_cases: 52, corpus_cases: 3,
  call_sequence: "Each fresh Program performs ordinary Program.emit followed by forced declaration-only Program.emit, then diagnostic getters. All 55 sequences repeat in another fresh Program.",
  contract: "Rust compares planning only. Complete TypeScript emit observations remain future bundle-runtime references; all outFile runtime requests retain their typed refusal.", cases };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(target, rendered);
else { assert.ok(process.argv[2] === undefined || process.argv[2] === "--check"); assert.equal(fs.readFileSync(target, "utf8"), rendered); }
console.log(`bundle planning: ${cases.length} complete repeated TypeScript observations; Rust runtime admission remains closed`);
