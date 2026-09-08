// H2.8a Synthetic namespace export modifiers. Expectations are complete TS commands.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/synthetic-namespace-export-modifiers.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const defaults = { strict:true, allowJs:true, checkJs:true, declaration:true, sourceMap:true,
  target:ts.ScriptTarget.ES2015, skipDefaultLibCheck:true, noErrorTruncation:true,
  newLine:ts.NewLineKind.CarriageReturnLineFeed, outDir:"/project/out" };
const shapes = [
 ["function-empty", 'function foo() {}\n'],
 ["function-normal", 'function foo() {}\nfoo.normal = 1;\n'],
 ["function-reserved", 'function foo() {}\nfoo.default = 1;\n'],
 ["function-mixed", 'function foo() {}\nfoo.normal = 1;\nfoo.default = 2;\n'],
 ["function-methods", 'function foo() {}\nfoo.normal = function () { return 1; };\nfoo.default = function () { return 2; };\n'],
 ["object-normal", 'const foo = { normal() { return 1; } };\n'],
 ["object-mixed", 'const foo = { normal() { return 1; }, default() { return 2; } };\n'],
 ["object-constructor", 'const foo = { constructor: function Foo() {}, normal() { return 1; } };\n'],
 ["nested-function", 'function foo() {}\nfoo.nested = function () {};\nfoo.nested.normal = 1;\nfoo.nested.default = 2;\nfoo.normal = 3;\n'],
 ["typedef", '/** @typedef {number} Value */\nfunction foo() {}\n/** @type {Value} */\nfoo.normal = 1;\nfoo.default = 2;\n'],
];
shapes.push(
 ["overloaded-constructor", 'const foo = {\n/** @overload Example(value)\n * @param value [String]\n */\nconstructor: function Example(value, options) {},\nnormal: 1\n};\n'],
 ["namespace-class-fallback", 'var foo = {};\nfoo.C1 = class { method() { return 1; } };\nvar C5;\nfoo.C5 = C5 || class { method() { return 2; } };\n'],
);
const inputs=[];
for (const [targetName,target] of [["es5",ts.ScriptTarget.ES5],["es2015",ts.ScriptTarget.ES2015]]) {
 for (const [route,tail] of [["script",""],["commonjs","module.exports = foo;\n"],["esm","export { foo };\n"]]) {
  for (const [shape,text] of shapes) {
   const main="/project/main.js";
   inputs.push({case_id:`synthetic-namespace/${targetName}/${route}/${shape}`,
    roots:[main],files:[{path:main,text:text+tail}],options:{...defaults,target,module:ts.ModuleKind.CommonJS}});
  }
 }
}
const typedShapes = [
 ["parsed-namespace", 'namespace foo { export const normal = 1; }\n'],
 ["parsed-nested-namespace", 'namespace foo { export namespace nested { export const normal = 1; } }\n'],
 ["parsed-ambient-namespace", 'declare namespace foo { const normal: number; }\n'],
];
for (const [targetName,target] of [["es5",ts.ScriptTarget.ES5],["es2015",ts.ScriptTarget.ES2015]]) {
 for (const [route,tail] of [["script",""],["esm","export { foo };\n"]]) {
  for (const [shape,text] of typedShapes) {
   const main="/project/main.ts";
   inputs.push({case_id:`synthetic-namespace/${targetName}/${route}/${shape}`,
    roots:[main],files:[{path:main,text:text+tail}],options:{...defaults,allowJs:false,checkJs:false,target,module:ts.ModuleKind.CommonJS}});
  }
 }
}
assert.equal(inputs.length,84);
function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [name, text, bom, onError, sources, data] = args;
  const bytes = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), bytes]) : bytes;
  return { index, path: name, kind: ts.isDeclarationFileName(name) ? "declaration" : name.endsWith(".map") ? "source-map" : name.endsWith(".mjs") ? "mjs" : name.endsWith(".cjs") ? "cjs" : "javascript",
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
function observe(input) {
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
  let options = input.options, roots = input.roots ?? input.files.map(file => file.path), errors = [];
  if (input.config) {
    const configPath = "/project/tsconfig.json";
    const parsed = ts.parseJsonSourceFileConfigFileContent(ts.parseJsonText(configPath, input.config),
      { ...host, readDirectory: () => roots }, "/project", undefined, configPath);
    options = parsed.options; roots = parsed.fileNames; errors = parsed.errors;
  }
  const program = ts.createProgram({ rootNames: roots, options, host, configFileParsingDiagnostics: errors });
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)), s => status.push(s), undefined,
    (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: sourceMaps(result.sourceMaps) },
    status_writes: status, exit_code: exit };
}
const cases = inputs.map(input => {
  const first = observe(input); assert.deepEqual(observe(input), first, input.case_id);
  return {...input, typescript_observation:first};
});
const artifact = {version:1,typescript:ts.version,source_commit:"050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256:sha256(fs.readFileSync(path.join(root,"vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256:sha256(fs.readFileSync(import.meta.filename)),repetitions:2,cases};
const rendered = JSON.stringify(artifact,null,2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination,rendered);
else assert.equal(fs.readFileSync(destination,"utf8"),rendered);
console.log(`Synthetic namespace export modifiers: ${cases.length} cases, two identical complete observations each`);
