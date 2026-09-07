// H2.7d module-identity references. No Rust runtime admission is claimed.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const outputPath = "crates/emitter/tests/fixtures/bundle-module-identities.json";
const sha256 = value => crypto.createHash("sha256").update(value).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
assert.ok(["--write", "--check"].includes(process.argv[2]), "use --write or --check");
const cases = [];
const dep = "export const value: number = 1;\n";
const main = "import { value } from './dep'; export const answer: number = value;\n";
for (const [name, module] of [["amd", ts.ModuleKind.AMD], ["system", ts.ModuleKind.System]]) {
  const add = (id, files, options = {}, extra = {}) => cases.push({
    case_id: `${name}/${id}`, current_directory: "/project", api_reference: false,
    options: { target: ts.ScriptTarget.ES2015, module, moduleResolution: ts.ModuleResolutionKind.Node10,
      outFile: "/project/dist/bundle.js", declaration: true, listEmittedFiles: true,
      strict: true, skipDefaultLibCheck: true, noErrorTruncation: true, newLine: ts.NewLineKind.LineFeed, ...options },
    files: Object.entries(files).map(([name, text]) => ({ path: "/project/" + name, text })), ...extra,
  });
  add("relative", { "src/main.ts": main, "src/dep.ts": dep });
  add("nested-reexport", { "src/app/main.ts": "export { value } from '../lib/dep';\n", "src/lib/dep.ts": dep });
  add("explicit-module-names", {
    "src/main.ts": '/// <amd-module name="public/main" />\n' + main,
    "src/dep.ts": '/// <amd-module name="public/dependency" />\n' + dep,
  });
  add("empty-module-names", {
    "src/main.ts": '/// <amd-module name="" />\n' + main,
    "src/dep.ts": '/// <amd-module name="" />\n' + dep,
  });
  add("import-equals", { "src/main.ts": "import dep = require('./dep'); export const answer = dep.value;\n", "src/dep.ts": dep });
  add("ambient-package", { "src/main.ts": "import { value } from 'pkg'; export const answer = value;\n",
    "node_modules/pkg/index.d.ts": "export declare const value: number;\n" }, {}, { roots: ["/project/src/main.ts"] });
  add("named-declaration-package", { "src/main.ts": "import { value } from 'pkg'; export const answer = value;\n",
    "node_modules/pkg/index.d.ts": '/// <amd-module name="public/package" />\nexport declare const value: number;\n' },
  {}, { roots: ["/project/src/main.ts"] });
  add("unresolved", { "src/main.ts": "import { value } from 'missing'; export const answer = value;\n" });
  add("resolved-before-rename", { "src/main.ts": main, "src/dep.ts": dep }, {}, {
    api_reference: true, renamed_dependencies: { "/project/src/main.ts": [["./dep", "ignored/rename"]] },
  });
  add("unresolved-rename", { "src/main.ts": "import { value } from 'missing'; export const answer = value;\n" }, {}, {
    api_reference: true, renamed_dependencies: { "/project/src/main.ts": [["missing", "runtime/missing"]] },
  });
  add("resolved-path-alias", { "src/main.ts": "import { value } from '@local/dep'; export const answer = value;\n",
    "src/lib/dep.ts": dep }, { baseUrl: "/project/src", paths: { "@local/*": ["lib/*"] } });
  add("duplicate-resolved-specifiers", { "src/main.ts": "import { value as a } from './dep'; import { value as b } from '@local/dep'; export const answer = a + b;\n",
    "src/dep.ts": dep }, { baseUrl: "/project/src", paths: { "@local/*": ["*"] } });
}

function diagnostic(value) {
  return { code: value.code, category: ts.DiagnosticCategory[value.category], file: value.file?.fileName ?? null,
    start: value.start ?? null, length: value.length ?? null, message: ts.flattenDiagnosticMessageText(value.messageText, "\n"),
    related_information: value.relatedInformation?.map(diagnostic) ?? null };
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
    writeFile() { throw Error("emit must use the recording callback"); },
    getSourceFile(name, languageVersion) {
      const text = read(name);
      if (text === undefined) return undefined;
      const source = ts.createSourceFile(name, text, languageVersion, true, ts.getScriptKindFromFileName(name));
      if (input.renamed_dependencies?.[name]) source.renamedDependencies = new Map(input.renamed_dependencies[name]);
      return source;
    },
  };
  const program = ts.createProgram(input.roots ?? input.files.map(file => file.path), input.options, host);
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
  const writes = [];
  const result = program.emit(undefined, (fileName, text, bom, onError, sources, data) => {
    const bytes = Buffer.from(text, "utf8");
    const materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), bytes]) : bytes;
    writes.push({ index: writes.length, path: fileName, callback_utf8_base64: bytes.toString("base64"),
      callback_utf8_bytes: bytes.length, write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"),
      on_error_callback_present: onError !== undefined, source_files: sources?.map(source => source.fileName) ?? null,
      data_present: data !== undefined, data_keys: data ? Object.keys(data) : null,
      data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null, data_source_map_url_pos: data?.sourceMapUrlPos ?? null });
  });
  return { common_source_directory: program.getCommonSourceDirectory(), source_order: program.getSourceFiles().map(file => file.fileName),
    identities, pre_emit_diagnostics: preEmit, writes, emit_result: { emit_skipped: result.emitSkipped,
      diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps ?? null } };
}

const pathCases = [
  ...["a.ts", "a.tsx", "a.mts", "a.cts", "a.d.ts", "a.d.mts", "a.js", "a.json", "a.TS"].map(name => ({
    case_id: `extension/${name}`, file: `/project/src/${name}`, reference: null, common: "/project/src/", cwd: "/project", case_sensitive: true,
  })),
  ...["/project/src/main.ts", "/project/src/nested/main.ts", "/project/other/main.ts"].map(reference => ({
    case_id: `reference/${reference}`, file: "/project/src/a.ts", reference, common: "/project/", cwd: "/project", case_sensitive: true,
  })),
  { case_id: "relative-spelling", file: "src/../src/a.ts", reference: null, common: "/project/src/", cwd: "/project", case_sensitive: true },
  { case_id: "case-insensitive", file: "/PROJECT/SRC/A.ts", reference: null, common: "/project/src/", cwd: "/project", case_sensitive: false },
];
function observePath(input) {
  const host = { getCurrentDirectory: () => input.cwd, getCommonSourceDirectory: () => input.common,
    getCanonicalFileName: name => input.case_sensitive ? name : name.toLowerCase() };
  return ts.getExternalModuleNameFromPath(host, input.file, input.reference ?? undefined);
}
for (const input of cases) {
  input.observation = observe(input);
  assert.deepEqual(observe(input), input.observation, input.case_id);
}
for (const input of pathCases) {
  input.observation = observePath(input);
  assert.equal(observePath(input), input.observation, input.case_id);
}
const artifact = { version: 1, typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))), repetitions: 2,
  dependencies: ["scripts/observe-bundle-module-identities.mjs", "crates/oracle/vfs-directory-overlay.mjs", ".node-version"]
    .map(name => ({ path: name, sha256: sha256(fs.readFileSync(path.join(root, name))) })),
  contract: "Program.emit tuples and internal module-identity helpers; no CLI status, Rust execution or runtime admission. Renamed-dependency mutations are separate API references.",
  cases, path_cases: pathCases };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(path.join(root, outputPath), rendered);
else assert.equal(fs.readFileSync(path.join(root, outputPath), "utf8"), rendered);
console.log(`bundle module identities: ${cases.length} Program tuples (${cases.filter(input => input.api_reference).length} API references), ${pathCases.length} path helpers; each twice`);
