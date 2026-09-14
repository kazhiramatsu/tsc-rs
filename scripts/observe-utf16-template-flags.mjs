// Parser-owned template flags and factory transform facets from pinned TypeScript.
// This is API evidence; command lowering and native emitter qualification are separate.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const bodies = [
  ["plain", "ok"], ["empty", ""], ["unicode", String.raw`\u0061`],
  ["extended", String.raw`\u{10000}`], ["hex", String.raw`\x61`],
  ["lone-surrogate", String.raw`\uD800`], ["extended-surrogate", String.raw`\u{D800}`],
  ["combined", String.raw`\u0061\x62\u{63}`],
  ["identity-and-control", String.raw`\z\n\0`],
  ["invalid-unicode", String.raw`\unicode`], ["invalid-hex", String.raw`\xQ`],
  ["out-of-range", String.raw`\u{110000}`], ["octal", String.raw`\01`],
  ["decimal", String.raw`\8`], ["line-continuation", "a\\\r\nb"],
  ["crlf", "a\r\nb"],
];
const inputs = bodies.flatMap(([name, body]) => [false, true].map(tagged => ({
  case_id: `${tagged ? "tagged" : "untagged"}/${name}`,
  source: `const value = ${tagged ? "tag" : ""}\`${body}\`;`,
})));
inputs.push(
  {case_id: "tagged/all-fragments", source: "tag`\\uD800${x}\\x61${y}\\u{10000}`;"},
  {case_id: "tagged/invalid-fragments", source: "tag`\\unicode${x}\\xQ${y}\\01`;"},
  {case_id: "untagged/invalid-rescans", source: "`\\unicode${x}\\xQ${y}\\01`;"},
  {case_id: "tagged/middle-only", source: "tag`a${x}\\unicode${y}c`;"},
  {case_id: "tagged/tail-only", source: "tag`a${x}b${y}\\unicode`;"},
  {case_id: "tagged/unterminated", source: "tag`abc"},
  {case_id: "untagged/unterminated", source: "`abc"},
  {case_id: "tagged/trivia", source: "/**doc*/\ntag\n`ok`;"},
  {case_id: "tagged/type-arguments", source: "tag<string>`\\unicode`;"},
  {case_id: "await/reparse", source: "export {}; await tag`\\unicode`; tag`\\u0061`;"},
  {case_id: "non-template/flags-isolation", source: String.raw`const a = 0b1; const b = "\u{61}"; tag` + "`ok`;"},
);
const units = text => Array.from({length: text.length}, (_, i) => text.charCodeAt(i));
function observe(input) {
  const source = ts.createSourceFile("main.ts", input.source, ts.ScriptTarget.ES2017, true);
  const fragments = [], transforms = [];
  function visit(node) {
    if (ts.isTemplateLiteralToken(node)) {
      fragments.push({kind: node.kind, pos: node.pos, end: node.end, value_utf16: units(node.text),
        raw_text: node.rawText ?? null, template_flags: node.templateFlags});
      transforms.push({kind: node.kind, pos: node.pos, end: node.end, flags: node.transformFlags});
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
  return {syntax: {fragments, diagnostics: source.parseDiagnostics.map(d => ({
    code: d.code, start: d.start, length: d.length,
    message_utf16: units(ts.flattenDiagnosticMessageText(d.messageText, "\n"))}))},
    transforms: {root_flags: source.transformFlags, fragments: transforms}};
}
const cases = inputs.map(input => {
  const first = observe(input), second = observe(input);
  assert.deepEqual(second, first, input.case_id);
  return {...input, source_sha256: sha(Buffer.from(input.source)), expected: first};
});
const artifact = {version: 1, scope: "Parser flags/values/diagnostics and transform flags only; not native or command qualification",
  typescript: ts.version, compiler_sha256: compilerSha, observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  target: ts.ScriptTarget.ES2017, repetitions: 2, parser_executions: cases.length * 2, cases};
const output = path.join(root, "crates/syntax/tests/fixtures/utf16-template-flags.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)), artifact);
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length, executions: cases.length * 2}));
