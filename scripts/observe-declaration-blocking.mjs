// Focused H2.7c observations; the historical declaration admission band is unchanged.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const root = path.resolve(import.meta.dirname, "..");
const target = path.join(root, "crates/compiler/tests/fixtures/declaration-blocking.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const defaults = { target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.CommonJS,
  newLine: ts.NewLineKind.CarriageReturnLineFeed, skipDefaultLibCheck: true, noErrorTruncation: true };
const inputs = [];
function add(case_id, files, options, extra = {}) {
  inputs.push({ case_id, files: Object.entries(files).map(([name, text]) => ({ path: "/project/" + name, text })),
    options: { ...defaults, ...options }, ...extra });
}
function official(name, hash, options) {
  const raw = fs.readFileSync(path.join(root, "ts-tests/tests/cases/compiler", name));
  assert.equal(sha256(raw), hash, name);
  const filename = raw.toString().match(/^\/\/\s*@filename:\s*(.*)/mi)?.[1].trim() ?? name;
  const text = raw.toString().split(/\r?\n/).filter(line => !/^\/\/\s*@\w+\s*:/.test(line)).join("\n").replace(/^\n+/, "");
  add("compiler/" + name, { [filename]: text }, options, { source_sha256: hash });
}
official("declFileEmitDeclarationOnlyError1.ts", "0de1fd32efd6603d6abb7eacc99398ded503d9fdebd331d141ee73de27fb94b0", { emitDeclarationOnly: true });
official("DeclarationErrorsNoEmitOnError.ts", "8baed7e57b0a218620301c111e75e95d5957048469eba5242c8e5e263c3cf3d1", { declaration: true, noEmitOnError: true });
official("noEmitOnError.ts", "902bb6eaaaf7fa1efd68e57acab83d797887b99b814a3101ad3871f80604854e", { declaration: true, noEmitOnError: true, sourceMap: true });

const valid = "export const value = 1;\n";
const bad = "export const value = class { private p = 1; };\n";
for (const noEmitOnError of [false, true]) {
  add(`invalid-option#blocked-${noEmitOnError}`, { "a.ts": valid },
    { emitDeclarationOnly: true, declaration: false, sourceMap: true, noEmitOnError, listEmittedFiles: true });
  add(`invalid-option#dts-only-${noEmitOnError}`, { "a.d.ts": "export declare const value: number;\n" },
    { emitDeclarationOnly: true, noEmitOnError });
  for (const reverse of [false, true]) {
    const files = reverse ? { "bad.ts": bad, "good.ts": valid } : { "good.ts": valid, "bad.ts": bad };
    add(`declaration-error#blocked-${noEmitOnError}-reverse-${reverse}`, files,
      { declaration: true, noEmitOnError, listEmittedFiles: noEmitOnError, ...(noEmitOnError ? { sourceMap: true } : {}) });
  }
  add(`declaration-error#semantic-precedence-${noEmitOnError}`,
    { "bad.ts": bad, "semantic.ts": "export const semantic: number = '';\n" },
    { declaration: true, noEmitOnError });
  add(`declaration-error#sorted-${noEmitOnError}`, { "z.ts": bad, "a.ts": bad },
    { declaration: true, noEmitOnError });
}
add("declaration-error#declaration-only", { "good.ts": valid, "bad.ts": bad },
  { declaration: true, emitDeclarationOnly: true, noEmitOnError: true });
add("declaration-error#js-only", { "bad.ts": bad }, { declaration: false, noEmitOnError: true });
add("declaration-error#strip-internal", { "bad.ts": "/** @internal */\n" + bad, "good.ts": valid },
  { declaration: true, noEmitOnError: true, stripInternal: true });
add("valid#declaration-only", { "a.ts": valid }, { declaration: true, emitDeclarationOnly: true, noEmitOnError: true });
add("valid#disabled-declaration-only", { "a.ts": valid }, { declaration: false, emitDeclarationOnly: false, noEmitOnError: true });
for (const noEmitOnError of [false, true]) {
  const config = JSON.stringify({ compilerOptions: { target: "es2015", module: "commonjs", newLine: "crlf",
    skipDefaultLibCheck: true, noErrorTruncation: true, emitDeclarationOnly: true, declaration: false, noEmitOnError }, files: ["a.ts"] }, null, 2) + "\n";
  add(`invalid-option#config-${noEmitOnError}`, { "a.ts": valid }, {}, { config });
}

function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [fileName, text, bom, onError, sourceFiles, data] = args;
  const callback = Buffer.from(text, "utf8");
  const materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), callback]) : callback;
  return { index, path: fileName, kind: fileName.endsWith(".d.ts") ? "declaration" : "javascript",
    callback_utf8_base64: callback.toString("base64"), callback_utf8_bytes: callback.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"),
    materialized_utf8_bytes: materialized.length, on_error_callback_present: onError !== undefined,
    source_files: sourceFiles?.map(f => f.fileName) ?? null, data_present: data !== undefined,
    data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null };
}
function observe(input) {
  const files = new Map(input.files.map(file => [file.path, file.text]));
  const host = ts.createCompilerHost(input.options, true);
  const readFile = host.readFile.bind(host), fileExists = host.fileExists.bind(host);
  host.readFile = name => files.get(name) ?? readFile(name);
  host.fileExists = name => files.has(name) || fileExists(name);
  host.getCurrentDirectory = () => "/project";
  let options = input.options, roots = input.files.map(file => file.path), errors = [];
  if (input.config) {
    const parsed = ts.parseJsonSourceFileConfigFileContent(ts.parseJsonText("/project/tsconfig.json", input.config),
      { ...host, readDirectory: () => roots }, "/project", undefined, "/project/tsconfig.json");
    options = parsed.options; roots = parsed.fileNames; errors = parsed.errors;
  }
  const program = ts.createProgram({ rootNames: roots, options, host, configFileParsingDiagnostics: errors });
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program,
    d => reported.push(diagnostic(d)), s => status.push(s), undefined,
    (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  assert.ok(result.sourceMaps === undefined || result.sourceMaps.length === 0, "this packet only observes suppressed map output");
  assert.deepEqual(status, []);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps ?? null },
    status_writes: status, exit_code: exit };
}
const cases = inputs.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  return { ...input, typescript_observation: first };
});
const artifact = { version: 1, typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))), repetitions: 2, cases };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(target, rendered);
else {
  assert.ok(process.argv[2] === undefined || process.argv[2] === "--check");
  assert.equal(fs.readFileSync(target, "utf8"), rendered);
}
console.log(`declaration blocking: ${cases.length} cases, 2 identical observations each`);
