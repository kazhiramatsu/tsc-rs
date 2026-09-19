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
  ["arrow-newline", "x;\n// c\narg => 2;"],
  ["arrow-same-line-replay", "x; /*c*/ arg => 2;"],
  ["arrow-source-start", "/*c*/ arg => 2;"],
  ["arrow-call", "f(/*c*/ arg => 2);"],
  ["arrow-parenthesized", "(/*c*/ arg) => 2;"],
  ["arrow-async", "async /*c*/ arg => 2;"],
  ["arrow-initializer", "var f = /*c*/ arg => 2;"],
  ["reference-detached-block", "/* license */\n\n///<reference path='./dep.d.ts'/>\n\ndeclare var x: Dep; function f() {}", {dependency: true}],
  ["reference-same-line", "/* license */ ///<reference path='./dep.d.ts'/>\n\ndeclare var x: Dep; function f() {}", {dependency: true}],
  ["reference-unrecognized", "// license\n\n/// plain\n\ndeclare var x: number; function f() {}"],
  ["reference-emitted-first", "// license\n\n///<reference path='./dep.d.ts'/>\n\nvar x: Dep; function f() {}", {dependency: true}],
  ["reference-crlf", "// license\r\n\r\n///<reference path='./dep.d.ts'/>\r\n\r\ndeclare var x: Dep; function f() {}", {dependency: true}],
  ["reference-pinned", "/*! license */\n\n///<reference path='./dep.d.ts'/>\n\ndeclare var x: Dep; function f() {}", {dependency: true}],
  ["escaped-if", "\\u0069f (true) {}"],
  ["escaped-speculative-async", "(\\u0061sync x => x);"],
  ["escaped-identifier-async", "let \\u0061sync = 1;"],
  ["escaped-extended-var", "\\u{0076}ar x = 'hello';"],
  ["escaped-type", "type typ\\u0065 = 12; typ\\u0065 notok = 0; export {};"],
  ["escaped-await-yield", "var \\u0061wait = 12; async function main() { \\u0061wait 12; } var \\u0079ield = 12; function* gen() { \\u0079ield 12; } export {};"],
  ["escaped-extended-await-yield", "var \\u{0061}wait = 12; async function main() { \\u{0061}wait 12; } var \\u{0079}ield = 12; function* gen() { \\u{0079}ield 12; } export {};"],
  ["invalid-export-variable-decorator", "declare function dec(...args: any[]): any; @dec export const x = 1;"],
  ["missing-body-export", "export function f(p: number) => p;"],
  ["escaped-async-function", "\\u0061sync function f() {}"],
  ["escaped-static", "class C { \\u0073tatic x = 1; }"],
  ["escaped-export", "\\u0065xport const x = 1;"],
  ["escaped-declare", "\\u0064eclare var y: number;"],
  ["escaped-identifier-property", "var \\u0061sync = 1; var x: any; x.\\u0069f;"],
  ["detached-two-blocks", "/* a */ /* b */\n\ndeclare var x: number;"],
  ["detached-jsdoc-block", "/** a */ /* b */\n\ndeclare var x: number;"],
  ["detached-single-block", "/* a */\n\ndeclare var x: number;"],
  ["detached-lines", "// a\n// b\n\ndeclare var x: number;"],
  ["detached-separated-reference", "/* a */\n///<reference path='./dep.d.ts'/>\n\ndeclare var x: Dep;", {"dependency": true}],
  ["detached-jsdoc-reference", "/** a */ ///<reference path='./dep.d.ts'/>\n\ndeclare var x: Dep;", {"dependency": true}],
  ["detached-namespace", "namespace N {\n/* a */ /* b */\n\nexport var x = 1;\n}"],
  ["detached-unicode-lines", "/* a */\u2028/* b */\u2028\u2028declare var x: number;"],
  ["detached-newline-crlf", "/* a */\r\n/* b */\r\n\r\ndeclare var x: number; var y = 1;"],
  ["detached-newline-cr", "/* a */\r/* b */\r\rdeclare var x: number; var y = 1;"],
  ["detached-newline-u2029", "/* a */\u2029/* b */\u2029\u2029declare var x: number; var y = 1;"],
  ["detached-newline-mixed", "/* a */\u2028\ndeclare var x: number; var y = 1;"],
  ["ordinary-single-crlf", "/* a */\r\ndeclare var x: number; var y = 1;"],
  ["detached-unicode-line-comment", "// a\u2028\u2028declare var x: number; var y = 1;"],
  ["detached-nbsp-blank", "/* a */\n\u00a0\ndeclare var x: number; var y = 1;"],
  ["detached-namespace-unicode", "namespace N {\u2028/* a */\u2028\u2028export var v = 1; }"],
  ["detached-function-unicode", "function f() {\u2028/* a */\u2028\u2028return 1; }"],
  ["detached-pinned-unicode", "/*! a */\u2028\u2028var y = 1;"],
  ["ordinary-unicode-leading", "x;\n/* a */\u2028/* b */\u2028y;"],
  ["detached-bom-unicode", "\ufeff/* a */\u2028\u2028declare var x: number; var y = 1;"],
  ["detached-shebang-unicode", "#!/usr/bin/env node\u2028/* a */\u2028\u2028declare var x: number; var y = 1;"],
  ["detached-newline-lf", "/* a */\n/* b */\n\ndeclare var x: number; var y = 1;"],
  ["ordinary-call-unicode", "f(/* a */\u2028/* b */\u2028x);"],
  ["detached-namespace-after-trailing-block", "namespace N { /* same */\n\n/* b */\n\nexport var x = 1; }"],
  ["detached-after-trailing-block", "function f() { /* same */\n\n/* b */\n\nreturn 1; }"],
  ["escaped-default-empty-line", "var x = 1; switch (x) { def\\u0061ult: // c\n}"],
  ["escaped-default-empty-block", "var x = 1; switch (x) { def\\u0061ult: /* c */ }"],
  ["escaped-default-statements", "var x = 1; switch (x) { def\\u0061ult: // c\n x++; }"],
  ["escaped-default-single-statement", "var x = 1; switch (x) { def\\u0061ult: x++; // c\n}"],
  ["escaped-case-empty-line", "var x = 1; switch (x) { c\\u0061se 1: // c\n}"],
  ["escaped-default-intervening", "var x = 1; switch (x) { def\\u0061ult /* a */ : // c\n}"],
  ["ordinary-default-empty-line", "var x = 1; switch (x) { default: // c\n}"],
  ["ordinary-case-empty-line", "var x = 1; switch (x) { case 1: // c\n}"],
  ["ordinary-default-statements", "var x = 1; switch (x) { default: // c\n x++; }"],
  ["ordinary-case-statements", "var x = 1; switch (x) { case 1: // c\n x++; }"],
];
const inputs = [];
for (const [target, module, noCheck] of [["es2015", "commonjs", false], ["es2015", "esnext", true], ["es5", "system", false], ["esnext", "commonjs", true]])
  for (const removeComments of [false, true])
    for (const [shape, text, extra = {}] of shapes) {
      const main = "/project/main.ts";
      const files = [{path: main, text: text + "\n"}];
      if (extra.dependency) files.push({path: "/project/dep.d.ts", text: "interface Dep { value: number; }\n"});
      inputs.push({case_id: `emitter-r168-corpus-controls/${target}/${module}/nocheck-${noCheck}/remove-${removeComments}/${shape}`,
        roots: [main], files, options: {},
        config: JSON.stringify({compilerOptions: {target, module, noCheck, removeComments,
          experimentalDecorators: extra.experimentalDecorators ?? false,
          lib: ["esnext"], strict: false, skipDefaultLibCheck: true, noErrorTruncation: true,
          sourceMap: true, declaration: true, declarationMap: true, ignoreDeprecations: "6.0",
          outDir: "/project/out"}, files: ["main.ts"]})});
    }
assert.equal(inputs.length, 496);

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
const destination = path.join(root, "crates/compiler/tests/fixtures/emitter-r168-corpus-controls.json");
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered, {flag: "wx"});
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`r168 corpus controls: ${cases.length} cases, two identical complete observations each`);
