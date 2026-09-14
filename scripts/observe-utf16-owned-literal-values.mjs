// Parser-owned literal values, independently observed from pinned TypeScript.
// This is syntax API evidence, not complete-command qualification.
// node scripts/observe-utf16-owned-literal-values.mjs --write|--check
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const inputPath = path.join(root, "crates/compiler/tests/fixtures/utf16-literals-adjacent-probes-inputs.json");
const inputBytes = fs.readFileSync(inputPath);
assert.equal(sha(inputBytes), "44672cd7192c2897f25cf93f0c04892a2c3bcabd00589de83ac32bfff2ee6b69");
const inputs = JSON.parse(inputBytes).cases.map(input => {
  assert.equal(input.files.length, 1);
  return {case_id: input.case_id, source: input.files[0].text, target: input.options.target};
});
inputs.push(
  {case_id: "ownership/distinct-values", source: String.raw`const a = ["\uD800", "\uD801", "\uDC00", "\uFFFD", "\\uD800"];`, target: ts.ScriptTarget.ES2015},
  {case_id: "ownership/pair-spellings", source: String.raw`const a = ["\uD83D\uDE00", "\u{1F600}", "😀"];`, target: ts.ScriptTarget.ES2015},
  {case_id: "ownership/template-fragments", source: "const a = `\\uD800${x}\\uDC00${y}\\uD83D\\uDE00`;", target: ts.ScriptTarget.ES2015},
  {case_id: "ownership/invalid-binding-name", source: String.raw`const "\uD800" = 1;`, target: ts.ScriptTarget.ES2015},
);
const units = text => Array.from({length: text.length}, (_, i) => text.charCodeAt(i));
const literalKinds = new Set([ts.SyntaxKind.StringLiteral, ts.SyntaxKind.NoSubstitutionTemplateLiteral,
  ts.SyntaxKind.TemplateHead, ts.SyntaxKind.TemplateMiddle, ts.SyntaxKind.TemplateTail]);
function observe(input) {
  const source = ts.createSourceFile("main.ts", input.source, input.target, true);
  const literals = [];
  function visit(node) {
    if (literalKinds.has(node.kind)) literals.push({kind: node.kind, pos: node.pos, end: node.end,
      value_utf16: units(node.text), raw_text: node.rawText ?? null});
    ts.forEachChild(node, visit);
  }
  visit(source);
  return {literals, diagnostics: source.parseDiagnostics.map(d => ({
    code: d.code, start: d.start, length: d.length,
    message_utf16: units(ts.flattenDiagnosticMessageText(d.messageText, "\n"))}))};
}
const cases = inputs.map(input => {
  const first = observe(input), second = observe(input);
  assert.deepEqual(second, first, input.case_id);
  return {...input, source_sha256: sha(Buffer.from(input.source)), expected: first};
});
const artifact = {version: 1, scope: "Parser literal values and diagnostics only; not native or command qualification",
  typescript: ts.version, compiler_sha256: compilerSha, input_sha256: sha(inputBytes),
  observer_sha256: sha(fs.readFileSync(import.meta.filename)), repetitions: 2,
  parser_executions: cases.length * 2, cases};
const output = path.join(root, "crates/syntax/tests/fixtures/utf16-owned-literal-values.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") {
  fs.mkdirSync(path.dirname(output), {recursive: true});
  fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
} else assert.deepEqual(JSON.parse(fs.readFileSync(output)), artifact);
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length, executions: cases.length * 2}));
