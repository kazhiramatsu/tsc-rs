// Focused H2.7c observations. This does not change any frozen admission band.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const ROOT = path.resolve(import.meta.dirname, "..");
const target = path.join(ROOT, "crates/compiler/tests/fixtures/strip-internal.json");
const sha256 = (bytes) => crypto.createHash("sha256").update(bytes).digest("hex");
const sources = [
  ["compiler/stripInternal1.ts", "cd734f20be3e4819705f0ee5a4d0ec9b65bfd443ac0e91bdd5d1cd92ff19d624"],
  ["conformance/declarationEmit/declarationEmitWorkWithInlineComments.ts", "31daa1ecc08ece9dc5413581c2dff2e96c168f72ae8d6141c99caa8460e0cc5a"],
];
function fixture(relative, expected) {
  const raw = fs.readFileSync(path.join(ROOT, "ts-tests/tests/cases", relative));
  assert.equal(sha256(raw), expected, relative);
  let text = "";
  for (const line of raw.toString("utf8").split(/\r?\n/)) {
    if (/^\/{2}\s*@([\w]+)\s*:/.test(line)) continue;
    if (text !== "") text += "\n";
    text += line;
  }
  return { source: relative, source_sha256: expected, text };
}
const inputs = sources.map(([relative, expected]) => fixture(relative, expected));
// Adjacent source controls, with expectations exclusively produced by tsc.
inputs.push({ source: "adjacent/comment-ownership.ts", text: `
/** @internal */
export interface Hidden { value: string; }
export interface Public {
  /** @internal */
  hidden: string;
  visible: string;
}
/** @internal */
export const hidden = 1;
export const visible = 2;
export enum E {
  /** @internal */
  Hidden,
  Visible,
}
export class C {
  /** @internal */
  get hidden(): string { return ""; }
  set hidden(value: string) {}
  /** @internal */ /* last ordinary comment */
  method(): void {}
  ordinary(): void {} // @internal
  retained(): void {}
  /** @Internal */
  caseSensitive(): void {}
}
export function parameter(/** @internal */ value: string): void {}
` });

function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category],
    file: d.file?.fileName ?? null, start: d.start ?? null, length: d.length ?? null,
    message: ts.flattenDiagnosticMessageText(d.messageText, "\n") };
}
function write(args, index) {
  const [fileName, text, bom, onError, sourceFiles, data] = args;
  const callback = Buffer.from(text, "utf8");
  const materialized = bom ? Buffer.concat([Buffer.from([239,187,191]), callback]) : callback;
  return { index, path: fileName, kind: fileName.endsWith(".d.ts") ? "declaration" : "javascript",
    callback_utf8_base64: callback.toString("base64"), callback_utf8_bytes: callback.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"),
    materialized_utf8_bytes: materialized.length, on_error_callback_present: onError !== undefined,
    source_files: sourceFiles?.map(f => f.fileName) ?? [], data_present: data !== undefined,
    data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null };
}
function observe(input, options) {
  const filename = "/project/" + path.posix.basename(input.source);
  const host = ts.createCompilerHost(options, true);
  const readFile = host.readFile.bind(host);
  const fileExists = host.fileExists.bind(host);
  host.readFile = name => name === filename ? input.text : readFile(name);
  host.fileExists = name => name === filename || fileExists(name);
  host.getCurrentDirectory = () => "/project";
  const program = ts.createProgram([filename], options, host);
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program,
    d => reported.push(diagnostic(d)), s => status.push(s), undefined,
    (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  // All frozen inputs are valid; any unexpected diagnostic fails observation.
  assert.deepEqual(reported, []);
  assert.deepEqual(result.diagnostics, []);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: [],
      emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps ?? null },
    status_writes: status, exit_code: exit };
}
const variants = [
  ["enabled", { stripInternal: true }],
  ["disabled", { stripInternal: false }],
  ["declaration-only", { stripInternal: true, emitDeclarationOnly: true }],
  ["remove-comments", { stripInternal: true, removeComments: true }],
  ["js-only", { stripInternal: true, declaration: false }],
];
const cases = inputs.flatMap(input => variants.map(([variant, changes]) => {
  const options = { target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.CommonJS,
    declaration: true, newLine: ts.NewLineKind.CarriageReturnLineFeed,
    skipDefaultLibCheck: true, noErrorTruncation: true, ...changes };
  const first = observe(input, options);
  assert.deepEqual(observe(input, options), first, input.source + "#" + variant);
  return { case_id: input.source + "#" + variant, input, options,
    typescript_observation: first };
}));
const artifact = { version: 1, typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(ROOT,"vendor/typescript-6.0.3/lib/typescript.js"))),
  repetitions: 2, cases };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
assert.ok(cases.length === 15);
if (process.argv[2] === "--write") {
  fs.mkdirSync(path.dirname(target), { recursive: true }); fs.writeFileSync(target, rendered);
} else {
  assert.ok(process.argv[2] === undefined || process.argv[2] === "--check");
  assert.equal(fs.readFileSync(target, "utf8"), rendered);
}
console.log(`stripInternal: ${cases.length} cases, 2 identical observations each`);
