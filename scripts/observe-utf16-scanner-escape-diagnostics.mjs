// Scanner escape diagnostics for the implementation-review fix round (A-7,
// lane A2 F1–F3): end of text after a backslash inside a tagged template,
// extended escapes whose value overflows, and identifier escapes that are
// peeked but not consumed. Parse diagnostics, identifier texts in source
// order and the statement count from pinned TypeScript; syntax API evidence only.
// node scripts/observe-utf16-scanner-escape-diagnostics.mjs --write|--check
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const units = text => Array.from({length: text.length}, (_, index) => text.charCodeAt(index));

const inputs = [
  // F1: scanEscapeSequence reports Unexpected_end_of_text unconditionally (_tsc.js:9066-9072).
  {case_id: "tagged-template-eof-backslash", source: "declare function tag(t: TemplateStringsArray): unknown; tag`\\"},
  {case_id: "untagged-template-eof-backslash", source: "const x = `\\"},
  {case_id: "string-eof-backslash", source: "const x = \"\\"},
  {case_id: "tagged-template-head-eof-backslash", source: "declare function tag(t: TemplateStringsArray, ...v: unknown[]): unknown; tag`a${1}\\"},
  // F2: an overflowing extended escape is a value above 0x10FFFF at escapedStart (_tsc.js:9221-9226).
  {case_id: "string-extended-escape-overflow", source: "const x = \"\\u{FFFFFFFFFF}\";"},
  {case_id: "template-extended-escape-overflow", source: "const x = `\\u{FFFFFFFFFF}`;"},
  {case_id: "tagged-template-extended-escape-overflow", source: "declare function tag(t: TemplateStringsArray): unknown; tag`\\u{FFFFFFFFFF}`;"},
  {case_id: "string-extended-escape-just-over", source: "const x = \"\\u{110000}\";"},
  {case_id: "string-extended-escape-max", source: "const x = \"\\u{10FFFF}\";"},
  {case_id: "string-extended-escape-no-digits", source: "const x = \"\\u{}\";"},
  {case_id: "string-extended-escape-unterminated", source: "const x = \"\\u{41\";"},
  // F3: a peeked identifier escape that is not consumed leaves pos and the
  // token flags untouched (_tsc.js:9247-9308, 9751-9770).
  {case_id: "identifier-part-extended-escape-overflow", source: "const a = 1; a\\u{FFFFFFFFFF};"},
  {case_id: "identifier-start-extended-escape-overflow", source: "\\u{FFFFFFFFFF} = 1;"},
  {case_id: "keyword-then-rejected-escape", source: "if\\u0020(x) {}"},
  {case_id: "identifier-then-rejected-escape", source: "const ab = 1; ab\\u0020;"},
  {case_id: "identifier-then-rejected-extended-escape", source: "const ab = 1; ab\\u{20};"},
  {case_id: "rejected-escape-at-start", source: "\\u0020x = 1;"},
  {case_id: "private-name-extended-escape-overflow", source: "class C { #\\u{FFFFFFFFFF} = 1; }"},
  // Regression controls: consumed identifier escapes keep their cooked text.
  {case_id: "identifier-start-escape", source: "const \\u0041bc = 1;"},
  {case_id: "identifier-part-escape", source: "const a\\u0062c = 1;"},
  {case_id: "identifier-extended-escape", source: "const \\u{62}cd = 1;"},
  {case_id: "keyword-escape", source: "\\u0069f (x) {}"},
  {case_id: "private-name-escape", source: "class C { #\\u0061 = 1; m() { return this.#a; } }"},
];

function observe(input) {
  const source = ts.createSourceFile("a.ts", input.source, ts.ScriptTarget.ESNext, true, ts.ScriptKind.TS);
  const diagnostics = source.parseDiagnostics.map(d => {
    assert.equal(typeof d.messageText, "string");
    return {code: d.code, start: d.start, length: d.length, message_utf16: units(d.messageText)};
  });
  const identifiers = [];
  let statements = 0;
  const walk = node => {
    if (ts.isIdentifier(node) || ts.isPrivateIdentifier(node)) {
      identifiers.push({text_utf16: units(ts.idText(node)), pos: node.pos, end: node.end, missing: node.pos === node.end});
    }
    ts.forEachChild(node, walk);
  };
  walk(source);
  statements = source.statements.length;
  return {diagnostics, identifiers, statements};
}

const cases = inputs.map(input => {
  const runs = [observe(input), observe(input)];
  assert.deepEqual(runs[0], runs[1], input.case_id);
  return {...input, source_sha256: sha(Buffer.from(input.source)), expected: runs[0]};
});
const artifact = {version: 1,
  scope: "scanner escape diagnostics: end of text after a backslash in tagged templates, overflowing extended escapes, peeked-but-rejected identifier escapes; parse diagnostics, identifier texts and statement counts; syntax API evidence only",
  typescript: ts.version, compiler_sha256: compilerSha, observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  repetitions: 2, cases};
const output = path.join(root, "crates/syntax/tests/fixtures/utf16-scanner-escape-diagnostics.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)), artifact);
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length, executions: cases.length * 2}));
