// Read-only design probe of the TypeScript diagnostic string boundary.
// These three API calls are not complete-command or native qualification.
// node scripts/observe-utf16-diagnostic-values.mjs --write|--check
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const sha = value => crypto.createHash("sha256").update(value).digest("hex");
const compiler = fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"));
assert.equal(sha(compiler), "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const options = {target: ts.ScriptTarget.ES2015, strict: true, skipDefaultLibCheck: true,
  noEmit: true, noErrorTruncation: true, newLine: ts.NewLineKind.CarriageReturnLineFeed};
const inputs = [
  {id: "duplicate-class-name", text: String.raw`export class C { "\u{D800}"=1; "\uD800"=2; }`, code: 2300, surrogate_units: []},
  {id: "missing-index-name", text: String.raw`export const o = {}; export const x = o["\uD800"];`, code: 7053, surrogate_units: [0xD800, 0xD800]},
  {id: "literal-annotation-mismatch", text: String.raw`export const x: "\uD800" = "\uDC00";`, code: 2322, surrogate_units: [0xDC00, 0xD800]},
];
function diagnostic(d) {
  const text = ts.flattenDiagnosticMessageText(d.messageText, "\n");
  return {code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null,
    message_utf16: Array.from({length: text.length}, (_, i) => text.charCodeAt(i)),
    message_json_spelling: JSON.stringify(text),
    message_node_utf8_base64: Buffer.from(text, "utf8").toString("base64"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null};
}
function observe(text) {
  const name = "/.src/main.ts", base = ts.createCompilerHost(options, true);
  const host = {...base, getCurrentDirectory: () => "/.src",
    getSourceFile: (f, v) => f === name ? ts.createSourceFile(f, text, v, true) : base.getSourceFile(f, v),
    fileExists: f => f === name || base.fileExists(f), readFile: f => f === name ? text : base.readFile(f),
    writeFile: () => assert.fail("design probe must not emit")};
  return ts.getPreEmitDiagnostics(ts.createProgram([name], options, host)).map(diagnostic);
}
const cases = inputs.map(input => {
  const first = observe(input.text), second = observe(input.text);
  assert.deepEqual(second, first);
  assert.deepEqual(first.map(d => d.code), [input.code]);
  const surrogates = first[0].message_utf16.filter(u => u >= 0xD800 && u <= 0xDFFF);
  assert.deepEqual(surrogates, input.surrogate_units);
  console.log(`${input.id}: code ${input.code}, ${surrogates.length} surrogate units in the diagnostic value, repeated x2`);
  return {case_id: input.id, file: "/.src/main.ts", source: input.text,
    source_sha256: sha(Buffer.from(input.text)), diagnostics: first};
});
const artifact = {version: 1, status: "design API observations only; no native or complete-command qualification",
  typescript: ts.version, compiler_sha256: sha(compiler), observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  api: "getPreEmitDiagnostics / flattenDiagnosticMessageText", options, repetitions: 2, api_executions: 6, cases};
const output = path.join(root, "ratchets/h2-8a-utf16-diagnostic-values-design.v1.json");
if (process.argv[2] === "--write") {
  assert.ok(!fs.existsSync(output), "retain existing observations"); fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n");
} else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(`${output}: ${sha(fs.readFileSync(output))}`);
