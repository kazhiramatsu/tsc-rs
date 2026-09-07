// Focused H2.7c observations; the historical declaration admission band is unchanged.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const root = path.resolve(import.meta.dirname, "..");
const target = path.join(root, "crates/compiler/tests/fixtures/isolated-declaration-expando-augmentation.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const defaults = { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.CommonJS, declaration: true, strict: true,
  newLine: ts.NewLineKind.CarriageReturnLineFeed, skipDefaultLibCheck: true, noErrorTruncation: true };
const inputs = [];
function add(case_id, files, options, extra = {}) {
  inputs.push({ case_id, files: Object.entries(files).map(([name, text]) => ({ path: "/project/" + name, text })),
    options: { ...defaults, ...options }, ...extra });
}
function official(name, hash, options) {
  const raw = fs.readFileSync(path.join(root, "ts-tests/tests/cases/compiler", name));
  assert.equal(sha256(raw), hash, name);
  const files = {};
  let filename = name, lines = [];
  const flush = () => {
    const text = lines.join("\n").replace(/^\n+/, "");
    if (text.trim()) files[filename] = text;
    lines = [];
  };
  for (const line of raw.toString().split(/\r?\n/)) {
    const match = line.match(/^\/\/\s*@filename:\s*(.*)/i);
    if (match) { flush(); filename = match[1].trim(); }
    else if (!/^\/\/\s*@\w+\s*:/.test(line)) lines.push(line);
  }
  flush();
  add("compiler/" + name, files, options, { source_sha256: hash });
}
official("isolatedDeclarationErrorsExpandoFunctions.ts", "c5c88b532c942b2f71aeb0774931567b5a85dfbbabb70093c2837f435f76efff", { strict: false });
official("isolatedDeclarationErrorsAugmentation.ts", "1c37b28e1ab1cfa23752251c994d97cedd8ce1b39922bd5dd7e85ad6fb689d35", {});
inputs.push({ ...inputs[1], case_id: inputs[1].case_id + "-reversed-roots", files: [...inputs[1].files].reverse() });
add("adjacent/expando-property-targets", { "properties.ts": `
export function named(): void {}
named["value"] = 1;
named.value = 2;
export const arrow = (): void => {};
arrow.extra = 1;
` }, {});
add("adjacent/declared-and-expando-properties", { "mixed.ts": `
export function declared(): void {}
export namespace declared { export let value: number; }
declared.value = 1;
export function mixed(): void {}
export namespace mixed { export let declared: number; }
mixed.declared = 1;
mixed.extra = 2;
` }, {});
for (const stripInternal of [true, false]) {
  add("adjacent/expando-internal-property-" + stripInternal, { "internal.ts": `
export function f(): void {}
/** @internal */
f.hidden = 1;
f.visible = 2;
` }, { stripInternal });
}
add("adjacent/erased-import", { "value.ts": "export const value: number = 1;\n", "parent.ts": "import { value } from './value'; export const p: number = 1;\n" }, {});
add("adjacent/explicit-side-effect-import", { "value.ts": "export const value: number = 1;\n", "parent.ts": "import './value'; export const p: number = 1;\n" }, {});
const variants = [
  ["enabled", { isolatedDeclarations: true }],
  ["disabled", { isolatedDeclarations: false }],
  ["no-emit-on-error", { isolatedDeclarations: true, noEmitOnError: true }],
];
const windows = inputs.flatMap(input => variants.map(([name, options]) => ({
  ...input, case_id: input.case_id + "#" + name, options: { ...input.options, ...options },
})));

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
  const directoryExists = host.directoryExists.bind(host);
  const directories = new Set();
  for (const file of files.keys()) {
    for (let directory = path.dirname(file);; directory = path.dirname(directory)) {
      directories.add(directory);
      if (directory === path.dirname(directory)) break;
    }
  }
  host.directoryExists = name => directories.has(name) || directoryExists(name);
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
const cases = windows.map(input => {
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
console.log(`isolated declaration expando/augmentation draft: ${cases.length} cases, 2 identical observations each`);
