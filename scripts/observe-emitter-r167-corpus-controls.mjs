// Complete commands for the remaining r162 source-corpus emitter boundaries.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const shapes = [
  ["missing-body-typed", "function f(p: number) => p;"],
  ["missing-body-untyped", "function f(p) => p;"],
  ["missing-body-namespace", "namespace N { export function f(p: number) => p; }"],
  ["bodyless-overload", "function f(p: number): number; function f(p: number) { return p; }"],
  ["missing-body-generic", "function f<T>(p) => p;"],
  ["real-empty-body", "function f(p: number) {}"],
  ["function-name-inline", "function f /* <T> */(p: number) { return p; }"],
  ["function-name-newline", "function f\n/*c*/(p: number) { return p; }"],
  ["function-expression-name", "const f = function g /*c*/(p: number) { return p; };"],
  ["generator-name", "function* g /*c*/(p: number) { yield p; }"],
  ["async-function-name", "async function f /*c*/(p: number) { return p; }"],
  ["decorator-using", "declare function dec<T>(target: T): T;\n@dec\nusing 1;\n@dec\nusing x;"],
  ["decorator-const", "declare function dec<T>(target: T): T;\n@dec\nconst x = 1;"],
  ["decorator-await-using", "declare function dec<T>(target: T): T; export async function f() { @dec\nawait using x = null; }"],
  ["reference-detached-header", "// license\n// second line\n\n///<reference path='./dep.d.ts'/>\n\ndeclare var x: Dep;\nfunction f() {}", {dependency: true}],
  ["reference-no-header", "///<reference path='./dep.d.ts'/>\ndeclare var x: Dep;\nfunction f() {}", {dependency: true}],
  ["reference-bom-header", "//\uFEFF\n// license\n\n///<reference path='./dep.d.ts'/>\n\n/** erased */ declare var x: Dep;\nfunction f() {}", {dependency: true}],
  ["arrow-bare-leading", "// first\n(arg) => 2;\n// second\narg => 2;"],
  ["arrow-parenthesized-leading", "// first\n(arg) => 2;\n// second\n(arg) => 2;"],
  ["arrow-bare-block-leading", "/* first */\n(arg) => 2;\n/* second */\narg => 2;"],
  ["valid-standard-decorators", "function dec(...args: any[]): any {}\n@dec class C { @dec field = 1; @dec m() {} }"],
  ["valid-legacy-decorators", "function dec(...args: any[]): any {}\n@dec class C { @dec field = 1; @dec m() {} }", {experimentalDecorators: true}],
];
const inputs = [];
for (const [target, module, noCheck] of [["es2015", "commonjs", false], ["es2015", "esnext", true], ["es5", "system", false], ["esnext", "commonjs", true]])
  for (const removeComments of [false, true])
    for (const [shape, text, extra = {}] of shapes) {
      const main = "/project/main.ts";
      const files = [{path: main, text: text + "\n"}];
      if (extra.dependency) files.push({path: "/project/dep.d.ts", text: "interface Dep { value: number; }\n"});
      inputs.push({case_id: `emitter-r167-corpus-controls/${target}/${module}/nocheck-${noCheck}/remove-${removeComments}/${shape}`,
        roots: [main], files, options: {},
        config: JSON.stringify({compilerOptions: {target, module, noCheck, removeComments,
          experimentalDecorators: extra.experimentalDecorators ?? false,
          lib: ["esnext"], strict: false, skipDefaultLibCheck: true, noErrorTruncation: true,
          sourceMap: true, declaration: true, declarationMap: true, ignoreDeprecations: "6.0",
          outDir: "/project/out"}, files: ["main.ts"]})});
    }
assert.equal(inputs.length, 176);

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
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  return {...input, typescript_observation: first};
});
const artifact = {version: 1, typescript: ts.version, repetitions: 2,
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases};
const destination = path.join(root, "crates/compiler/tests/fixtures/emitter-r167-corpus-controls.json");
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered, {flag: "wx"});
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`r167 corpus controls: ${cases.length} cases, two identical complete observations each`);
