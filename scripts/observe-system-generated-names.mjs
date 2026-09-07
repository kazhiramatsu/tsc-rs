// Complete ordinary System observations; no Rust runtime admission.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const observerPath = "scripts/observe-system-generated-names.mjs";
const fixturePath = "crates/emitter/tests/fixtures/system-generated-names.json";
const originalPath = "crates/emitter/tests/fixtures/bundle-module-identities.json";
const originalHash = "965c6ab4b187ac8488b63e572544ea0924afd230af43077092ce57ed69bb91bb";
const sha256 = value => crypto.createHash("sha256").update(value).digest("hex");
const identity = name => ({ path: name, sha256: sha256(fs.readFileSync(path.join(root, name))) });
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
assert.ok(["--write", "--check"].includes(process.argv[2]), "use --write or --check");
assert.equal(identity(originalPath).sha256, originalHash);
const original = JSON.parse(fs.readFileSync(path.join(root, originalPath), "utf8"));
assert.equal(original.cases.length, 24);
assert.equal(original.repetitions, 2);
assert.equal(original.compiler_sha256, identity("vendor/typescript-6.0.3/lib/typescript.js").sha256);
const baseline = original.cases.find(row => row.case_id === "system/relative");
assert.ok(baseline);
const cases = [];
function add(case_id, sources, standalone = false) {
  const options = structuredClone(baseline.options);
  if (standalone) { delete options.outFile; options.outDir = "/project/dist"; }
  const files = Object.entries(sources).map(([name, text]) => ({ path: "/project/src/" + name, text }));
  cases.push({ case_id, current_directory: baseline.current_directory, api_reference: false,
    use_case_sensitive_file_names: true, options, files, roots: files.map(file => file.path),
    origin: { kind: "focused-control", baseline_case_id: baseline.case_id,
      option_delta: standalone ? { removed: ["outFile"], added: { outDir: options.outDir } } : { removed: [], added: {} } } });
}
const pair = { "a.ts": "export const a: number = 1;\n", "b.ts": "export const b: number = 2;\n" };
const triple = { ...pair, "c.ts": "export const c: number = 3;\n" };
add("bundle/two-modules", pair);
add("bundle/three-modules", triple);
add("standalone/three-modules", triple, true);
const parameters = suffix => `const exports_${suffix}: number = 10; const context_${suffix}: number = 20;\n`;
add("bundle/parsed-parameters-before", {
  "a.ts": parameters(1) + "export const a = exports_1 + context_1;\n", "b.ts": pair["b.ts"],
});
add("bundle/parsed-parameters-after", {
  "a.ts": pair["a.ts"], "b.ts": parameters(1) + "export const b = exports_1 + context_1;\n",
});
add("bundle/parsed-next-suffixes", {
  "a.ts": pair["a.ts"], "b.ts": parameters(2) + "export const b = exports_2 + context_2;\n",
  "c.ts": parameters(3) + "export const c = exports_3 + context_3;\n",
});
const star = {
  "a.ts": pair["a.ts"],
  "b.ts": "import { a } from './a'; export * from './a'; export const b: number = a + 1;\n",
  "c.ts": "import { a } from './a'; import { b } from './b'; export * from './a'; export * from './b'; export const c: number = a + b;\n",
};
const helperCollisions = "const exportStar_1: number = 10; const exportedNames_1: number = 20;\n"
  + "const exportStar_2: number = 11; const exportedNames_2: number = 21;\n"
  + "const a_1: number = 30; const a_1_1: number = 40; const a_2_1: number = 41;\n"
  + "const b_1: number = 50; const b_1_1: number = 60;\n";
const useCollisions = "export const userNames = exportStar_1 + exportedNames_1 + exportStar_2 + exportedNames_2 + a_1 + a_1_1 + a_2_1 + b_1 + b_1_1;\n";
add("bundle/star-dependency-chain", star);
add("bundle/parsed-helper-names-before", { ...star, "a.ts": helperCollisions + pair["a.ts"] + useCollisions });
add("bundle/parsed-helper-names-after", { ...star, "c.ts": helperCollisions + star["c.ts"] + useCollisions });
add("standalone/star-dependency-chain", star, true);
add("bundle/empty-between-modules", { "a.ts": pair["a.ts"], "empty.ts": "", "b.ts": pair["b.ts"] });
add("bundle/global-between-modules", {
  "a.ts": pair["a.ts"], "global.ts": parameters(1), "b.ts": pair["b.ts"],
});
assert.equal(cases.length, 12);
assert.equal(new Set(cases.map(row => row.case_id)).size, cases.length);

function diagnostic(value) {
  return { code: value.code, category: ts.DiagnosticCategory[value.category], file: value.file?.fileName ?? null,
    start: value.start ?? null, length: value.length ?? null, message: ts.flattenDiagnosticMessageText(value.messageText, "\n"),
    related_information: value.relatedInformation?.map(diagnostic) ?? null };
}
// These names are decoded from the complete emitted JS, not inferred from input
// spelling or a replacement generation algorithm. Full callback bytes are primary.
function systemRegisters(fileName, text) {
  const source = ts.createSourceFile(fileName, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  assert.deepEqual(source.parseDiagnostics, []);
  const registers = [];
  for (const statement of source.statements) {
    if (!ts.isExpressionStatement(statement) || !ts.isCallExpression(statement.expression)) continue;
    const call = statement.expression;
    if (!ts.isPropertyAccessExpression(call.expression) || call.expression.expression.getText(source) !== "System"
      || call.expression.name.text !== "register") continue;
    const body = call.arguments.at(-1);
    assert.ok(ts.isFunctionExpression(body));
    const dependencies = call.arguments.at(-2);
    assert.ok(ts.isArrayLiteralExpression(dependencies));
    const returned = body.body.statements.find(ts.isReturnStatement)?.expression;
    assert.ok(returned && ts.isObjectLiteralExpression(returned));
    const setters = returned.properties.find(node => ts.isPropertyAssignment(node) && node.name.getText(source) === "setters")?.initializer;
    assert.ok(setters && ts.isArrayLiteralExpression(setters));
    const variables = body.body.statements.filter(ts.isVariableStatement).flatMap(node => [...node.declarationList.declarations]);
    registers.push({ module_name: call.arguments.length === 3 ? call.arguments[0].text : null,
      dependencies: dependencies.elements.map(node => { assert.ok(ts.isStringLiteral(node)); return node.text; }),
      export_parameter: body.parameters[0].name.getText(source), context_parameter: body.parameters[1].name.getText(source),
      setter_parameters: setters.elements.map(node => { assert.ok(ts.isFunctionExpression(node)); return node.parameters.map(parameter => parameter.name.getText(source)); }),
      export_star_functions: body.body.statements.filter(node => ts.isFunctionDeclaration(node) && /^exportStar_\d+$/.test(node.name?.text ?? "")).map(node => node.name.text),
      exported_names_variables: variables.filter(node => ts.isIdentifier(node.name) && /^exportedNames_\d+$/.test(node.name.text) && node.initializer && ts.isObjectLiteralExpression(node.initializer)).map(node => node.name.text) });
  }
  return registers;
}
function observe(input) {
  const files = new Map(input.files.map(file => [ts.normalizePath(file.path), file.text]));
  const libraryPath = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name)
    ? path.join(root, "vendor/typescript-6.0.3/lib", path.basename(name)) : undefined;
  const read = name => files.get(ts.normalizePath(name)) ?? (libraryPath(name) && fs.existsSync(libraryPath(name))
    ? fs.readFileSync(libraryPath(name), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(input.files.map(file => file.path), {
    currentDirectory: input.current_directory, useCaseSensitiveFileNames: true,
    fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] },
  });
  const host = { ...ts.createCompilerHost(input.options, true), ...overlay,
    getCurrentDirectory: () => input.current_directory, useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: ts.normalizePath, getDefaultLibFileName: () => "/lib/" + ts.getDefaultLibFileName(input.options),
    getDefaultLibLocation: () => "/lib", readFile: read, fileExists: name => read(name) !== undefined, realpath: ts.normalizePath,
    writeFile() { assert.fail("emit must use the recording callback"); },
    getSourceFile(name, languageVersion) {
      const text = read(name);
      return text === undefined ? undefined : ts.createSourceFile(name, text, languageVersion, true, ts.getScriptKindFromFileName(name));
    },
  };
  const program = ts.createProgram(input.roots, input.options, host);
  for (const file of program.getSourceFiles()) assert.ok(files.has(file.fileName) || libraryPath(file.fileName), file.fileName);
  const preEmit = ts.getPreEmitDiagnostics(program).map(diagnostic);
  const resolver = program.getTypeChecker().getEmitResolver();
  const emitHost = { ...program, getCanonicalFileName: ts.normalizePath, useCaseSensitiveFileNames: () => true };
  const identities = program.getSourceFiles().filter(file => !libraryPath(file.fileName)).map(file => ({
    path: file.fileName, declaration_file: file.isDeclarationFile, explicit_module_name: file.moduleName ?? null,
    resolved_name: ts.getResolvedExternalModuleName(emitHost, file),
    emitted_module_name: ts.tryGetModuleNameFromFile(ts.factory, file, emitHost, input.options)?.text ?? null,
    imports: file.statements.filter(node => ts.isImportDeclaration(node) || ts.isExportDeclaration(node) || ts.isImportEqualsDeclaration(node))
      .map(node => ({ kind: ts.SyntaxKind[node.kind], original_text: ts.getExternalModuleName(node)?.text ?? null,
        resolved_file: resolver.getExternalModuleFileFromDeclaration(node)?.fileName ?? null,
        literal: ts.getExternalModuleNameLiteral(ts.factory, node, file, emitHost, resolver, input.options)?.text ?? null,
        declaration_name: ts.getExternalModuleNameFromDeclaration(emitHost, resolver, node) ?? null })),
  }));
  const writes = [], registers = [];
  const result = program.emit(undefined, (fileName, text, bom, onError, sources, data) => {
    assert.ok(!text.includes(root));
    const bytes = Buffer.from(text, "utf8");
    const materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), bytes]) : bytes;
    assert.ok(data === undefined || Object.keys(data).every(key => ["diagnostics", "sourceMapUrlPos"].includes(key)));
    writes.push({ index: writes.length, path: fileName, callback_utf8_base64: bytes.toString("base64"),
      callback_utf8_bytes: bytes.length, write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"),
      on_error_callback_present: onError !== undefined, source_files: sources?.map(source => source.fileName) ?? null,
      data_present: data !== undefined, data_keys: data ? Object.keys(data) : null,
      data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null, data_source_map_url_pos: data?.sourceMapUrlPos ?? null });
    if (fileName.endsWith(".js")) registers.push({ path: fileName, registers: systemRegisters(fileName, text) });
  });
  const programDiagnostics = { options: program.getOptionsDiagnostics().map(diagnostic), syntactic: program.getSyntacticDiagnostics().map(diagnostic),
    global: program.getGlobalDiagnostics().map(diagnostic), semantic: program.getSemanticDiagnostics().map(diagnostic),
    declaration: program.getDeclarationDiagnostics().map(diagnostic) };
  assert.deepEqual(programDiagnostics.syntactic, []); assert.deepEqual(programDiagnostics.global, []);
  assert.deepEqual(programDiagnostics.semantic, []); assert.deepEqual(programDiagnostics.declaration, []);
  assert.deepEqual(programDiagnostics.options,
    baseline.observation.pre_emit_diagnostics.filter(d => input.options.outFile || d.code !== 5101), input.case_id);
  assert.equal(result.emitSkipped, false); assert.deepEqual(result.diagnostics, []);
  assert.equal(result.sourceMaps, undefined);
  assert.equal(writes.filter(write => write.path.endsWith(".js")).length, input.options.outFile ? 1 : input.files.length);
  return { common_source_directory: program.getCommonSourceDirectory(), source_order: program.getSourceFiles().map(file => file.fileName),
    identities, pre_emit_diagnostics: preEmit, writes, emit_result: { emit_skipped: result.emitSkipped,
      diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps ?? null },
    program_diagnostics: programDiagnostics, exception: null, system_registers: registers,
    libraries: program.getSourceFiles().filter(file => libraryPath(file.fileName)).map(file => ({ path: file.fileName, sha256: sha256(file.text) })) };
}
for (const input of cases) {
  input.observation = observe(input);
  assert.deepEqual(observe(input), input.observation, input.case_id);
}
const sourcePath = "vendor/typescript-6.0.3/lib/_tsc.js";
const sourceLines = fs.readFileSync(path.join(root, sourcePath), "utf8").split("\n");
const references = [
  ["System generated exports/context", 112079, 112092],
  ["System generated exportedNames/exportStar", 112292, 112319],
  ["System generated dependency setter parameters", 112403, 112410],
  ["Bundle vs standalone writer reset", 117058, 117081],
  ["Printer generated-name reset", 117117, 117141],
  ["Cached generation and current-source/generated-name uniqueness", 120624, 120667],
  ["makeUniqueName numeric suffix allocation", 120741, 120779],
].map(([owner, first_line, last_line]) => ({ owner, path: sourcePath, first_line, last_line,
  sha256: sha256(sourceLines.slice(first_line - 1, last_line).join("\n") + "\n") }));
const artifact = { version: 1, typescript: ts.version, source_commit: original.source_commit,
  compiler_sha256: original.compiler_sha256, repetitions: 2,
  dependencies: [observerPath, originalPath, sourcePath, "crates/oracle/vfs-directory-overlay.mjs", ".node-version"].map(identity),
  contract: "Complete ordinary Program.emit tuples plus lexical names decoded from emitted JavaScript; no Rust execution, CLI status, forced API or runtime admission. The original module-identity fixture is unchanged.",
  upstream_references: references, cases };
assert.equal(identity(originalPath).sha256, originalHash);
const rendered = JSON.stringify(artifact, null, 2) + "\n";
assert.ok(!rendered.includes(root));
if (process.argv[2] === "--write") fs.writeFileSync(path.join(root, fixturePath), rendered);
else assert.equal(fs.readFileSync(path.join(root, fixturePath), "utf8"), rendered);
console.log(`System generated names: ${cases.length} complete Program tuples (${cases.filter(input => input.options.outFile).length} bundle, ${cases.filter(input => !input.options.outFile).length} standalone), each twice; original24 unchanged`);
