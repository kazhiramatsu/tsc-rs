// Complete AMD/System trailing-underscore observations; no Rust runtime admission.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const observerPath = "scripts/observe-module-alias-underscores.mjs";
const fixturePath = "crates/emitter/tests/fixtures/module-alias-underscores.json";
const originalPath = "crates/emitter/tests/fixtures/bundle-module-identities.json";
const originalHash = "965c6ab4b187ac8488b63e572544ea0924afd230af43077092ce57ed69bb91bb";
const systemPath = "crates/emitter/tests/fixtures/system-generated-names.json";
const systemHash = "ee524599b64d4d01287ee8ff1a1ff5bfa42ddefadfc0bccd9f1f8b07ae513112";
const sha256 = value => crypto.createHash("sha256").update(value).digest("hex");
const identity = name => ({ path: name, sha256: sha256(fs.readFileSync(path.join(root, name))) });
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
assert.ok(["--write", "--check"].includes(process.argv[2]), "use --write or --check");
assert.equal(identity(originalPath).sha256, originalHash);
assert.equal(identity(systemPath).sha256, systemHash);
const original = JSON.parse(fs.readFileSync(path.join(root, originalPath), "utf8"));
assert.equal(original.cases.length, 24);
assert.equal(original.repetitions, 2);
assert.equal(original.compiler_sha256, identity("vendor/typescript-6.0.3/lib/typescript.js").sha256);
const cases = [];
for (const kind of ["amd", "system"]) {
  const baseline = original.cases.find(row => row.case_id === `${kind}/relative`);
  assert.ok(baseline);
  for (const [base, standalone] of [["dep_", false], ["dep__", false], ["dep__", true]]) {
    const options = structuredClone(baseline.options);
    if (standalone) { delete options.outFile; options.outDir = "/project/dist"; }
    const sources = {
      [`${base}.ts`]: "export const value: number = 1;\n",
      "bridge.ts": `import { value } from './${base}'; export * from './${base}'; export const bridge: number = value + 1;\n`,
      "main.ts": `import { value } from './${base}'; import { bridge } from './bridge'; export * from './bridge'; export const main: number = value + bridge;\n`,
    };
    const files = Object.entries(sources).map(([name, text]) => ({ path: "/project/src/" + name, text }));
    cases.push({ case_id: `${kind}/${standalone ? "standalone" : "bundle"}/${base}`, current_directory: baseline.current_directory,
      api_reference: false, use_case_sensitive_file_names: true, options, files, roots: files.map(file => file.path),
      origin: { kind: "focused-control", baseline_case_id: baseline.case_id,
        option_delta: standalone ? { removed: ["outFile"], added: { outDir: options.outDir } } : { removed: [], added: {} } } });
  }
}
assert.equal(cases.length, 6);
assert.equal(new Set(cases.map(row => row.case_id)).size, cases.length);

function diagnostic(value) {
  return { code: value.code, category: ts.DiagnosticCategory[value.category], file: value.file?.fileName ?? null,
    start: value.start ?? null, length: value.length ?? null, message: ts.flattenDiagnosticMessageText(value.messageText, "\n"),
    related_information: value.relatedInformation?.map(diagnostic) ?? null };
}
// These names are decoded from the complete emitted JS, not inferred from input
// spelling or a replacement generation algorithm. Full callback bytes are primary.
function moduleRegistrations(fileName, text) {
  const source = ts.createSourceFile(fileName, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  assert.deepEqual(source.parseDiagnostics, []);
  const registrations = [];
  for (const statement of source.statements) {
    if (!ts.isExpressionStatement(statement) || !ts.isCallExpression(statement.expression)) continue;
    const call = statement.expression;
    const amd = ts.isIdentifier(call.expression) && call.expression.text === "define";
    const system = ts.isPropertyAccessExpression(call.expression) && call.expression.expression.getText(source) === "System"
      && call.expression.name.text === "register";
    if (!amd && !system) continue;
    const body = call.arguments.at(-1), dependencies = call.arguments.at(-2);
    assert.ok(ts.isFunctionExpression(body)); assert.ok(ts.isArrayLiteralExpression(dependencies));
    const variables = body.body.statements.filter(ts.isVariableStatement).flatMap(node => [...node.declarationList.declarations]);
    let setters = null;
    if (system) {
      const returned = body.body.statements.find(ts.isReturnStatement)?.expression;
      assert.ok(returned && ts.isObjectLiteralExpression(returned));
      const array = returned.properties.find(node => ts.isPropertyAssignment(node) && node.name.getText(source) === "setters")?.initializer;
      assert.ok(array && ts.isArrayLiteralExpression(array));
      setters = array.elements.map(node => { assert.ok(ts.isFunctionExpression(node)); return node.parameters.map(parameter => parameter.name.getText(source)); });
    }
    registrations.push({ kind: amd ? "amd" : "system", module_name: call.arguments.length === 3 ? call.arguments[0].text : null,
      dependencies: dependencies.elements.map(node => { assert.ok(ts.isStringLiteral(node)); return node.text; }),
      factory_parameters: body.parameters.map(parameter => parameter.name.getText(source)), setter_parameters: setters,
      local_variables: variables.map(node => node.name.getText(source)),
      export_star_functions: body.body.statements.filter(node => ts.isFunctionDeclaration(node) && /^exportStar_\d+$/.test(node.name?.text ?? "")).map(node => node.name.text),
      exported_names_variables: variables.filter(node => ts.isIdentifier(node.name) && /^exportedNames_\d+$/.test(node.name.text) && node.initializer && ts.isObjectLiteralExpression(node.initializer)).map(node => node.name.text) });
  }
  return registrations;
}
function observe(input) {
  const baseline = original.cases.find(row => row.case_id === input.origin.baseline_case_id); assert.ok(baseline);
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
    if (fileName.endsWith(".js")) registers.push({ path: fileName, registrations: moduleRegistrations(fileName, text) });
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
    program_diagnostics: programDiagnostics, exception: null, module_registrations: registers,
    libraries: program.getSourceFiles().filter(file => libraryPath(file.fileName)).map(file => ({ path: file.fileName, sha256: sha256(file.text) })) };
}
for (const input of cases) {
  input.observation = observe(input);
  assert.deepEqual(observe(input), input.observation, input.case_id);
}
const sourcePath = "vendor/typescript-6.0.3/lib/_tsc.js";
const sourceLines = fs.readFileSync(path.join(root, sourcePath), "utf8").split("\n");
const references = [
  ["AMD generated dependency factory parameters", 110442, 110500],
  ["External import local-name identity", 27696, 27718],
  ["Module basename identifier spelling", 13703, 13710],
  ["System generated exports/context", 112079, 112092],
  ["System generated exportedNames/exportStar", 112292, 112319],
  ["System generated dependency setter parameters", 112403, 112410],
  ["Bundle vs standalone writer reset", 117058, 117081],
  ["Printer generated-name reset", 117117, 117141],
  ["Cached generation and current-source/generated-name uniqueness", 120624, 120667],
  ["makeUniqueName numeric suffix allocation", 120741, 120779],
  ["Module-derived name generation", 120813, 120830],
].map(([owner, first_line, last_line]) => ({ owner, path: sourcePath, first_line, last_line,
  sha256: sha256(sourceLines.slice(first_line - 1, last_line).join("\n") + "\n") }));
const artifact = { version: 1, typescript: ts.version, source_commit: original.source_commit,
  compiler_sha256: original.compiler_sha256, repetitions: 2,
  dependencies: [observerPath, originalPath, systemPath, sourcePath, "crates/oracle/vfs-directory-overlay.mjs", ".node-version"].map(identity),
  contract: "Complete ordinary Program.emit tuples plus lexical names decoded from emitted JavaScript; no Rust execution, CLI status, forced API or runtime admission. The original module-identity and System-generated-name fixtures are unchanged.",
  upstream_references: references, cases };
assert.equal(identity(originalPath).sha256, originalHash);
assert.equal(identity(systemPath).sha256, systemHash);
const rendered = JSON.stringify(artifact, null, 2) + "\n";
assert.ok(!rendered.includes(root));
if (process.argv[2] === "--write") fs.writeFileSync(path.join(root, fixturePath), rendered);
else assert.equal(fs.readFileSync(path.join(root, fixturePath), "utf8"), rendered);
console.log(`AMD/System underscore aliases: ${cases.length} complete Program tuples (4 bundle, 2 standalone), each twice; original24/System12 unchanged`);
