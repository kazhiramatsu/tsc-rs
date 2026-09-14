// `new.<name>` meta-property names, independently observed from pinned TypeScript.
// tsc parses the name with parseIdentifierName(): keywords are accepted and any
// other token (a string or template token carrying an invalid escape included)
// yields a missing identifier plus TS1003. This is syntax API evidence only.
// node scripts/observe-utf16-new-meta-property-name.mjs --write|--check
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
  {case_id: "lone-surrogate-string-name", file_name: "a.ts", source: String.raw`const a = new."\uD800";`},
  {case_id: "lone-surrogate-template-name", file_name: "a.ts", source: "const a = new.`\\uD800`;"},
  {case_id: "surrogate-pair-string-name", file_name: "a.ts", source: String.raw`const a = new."😀";`},
  {case_id: "scalar-string-name", file_name: "a.ts", source: String.raw`const a = new."x";`},
  {case_id: "keyword-name", file_name: "a.ts", source: "const a = new.if;"},
  {case_id: "target-in-function", file_name: "a.ts", source: "function f() { return new.target; }"},
  {case_id: "numeric-after-new-is-not-a-meta-property", file_name: "a.ts", source: "const a = new.1;"},
  {case_id: "lone-surrogate-string-name-js", file_name: "a.js", source: String.raw`const a = new."\uD800";`},
  {case_id: "lone-surrogate-string-name-tsx", file_name: "a.tsx", source: String.raw`const a = new."\uD800";`},
  {case_id: "eof-after-dot", file_name: "a.ts", source: "const a = new."},
];

function observe(input) {
  const source = ts.createSourceFile(input.file_name, input.source, ts.ScriptTarget.ESNext, true,
    ts.getScriptKindFromFileName(input.file_name));
  const diagnostics = source.parseDiagnostics.map(d => {
    assert.equal(typeof d.messageText, "string");
    return {code: d.code, start: d.start, length: d.length, message_utf16: units(d.messageText)};
  });
  const metaProperties = [];
  const walk = node => {
    if (ts.isMetaProperty(node)) {
      assert.equal(node.keywordToken, ts.SyntaxKind.NewKeyword);
      assert.ok(ts.isIdentifier(node.name));
      metaProperties.push({pos: node.pos, end: node.end, name_pos: node.name.pos, name_end: node.name.end,
        name_utf16: units(ts.idText(node.name)), name_missing: node.name.pos === node.name.end});
    }
    ts.forEachChild(node, walk);
  };
  walk(source);
  return {diagnostics, meta_properties: metaProperties,
    new_expressions: (() => { let count = 0; const visit = n => { if (ts.isNewExpression(n)) count++; ts.forEachChild(n, visit); }; visit(source); return count; })()};
}

const cases = inputs.map(input => {
  const runs = [observe(input), observe(input)];
  assert.deepEqual(runs[0], runs[1], input.case_id);
  return {...input, expected: runs[0]};
});
const artifact = {version: 1,
  scope: "new.<name> meta-property names through parseIdentifierName: keyword names, missing names with TS1003 for string/template tokens (lone surrogate values included); syntax API evidence only",
  typescript: ts.version, compiler_sha256: compilerSha, observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  repetitions: 2, cases};
const output = path.join(root, "crates/syntax/tests/fixtures/utf16-new-meta-property-name.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") {
  fs.mkdirSync(path.dirname(output), {recursive: true});
  fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
} else assert.deepEqual(JSON.parse(fs.readFileSync(output)), artifact);
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length, executions: cases.length * 2}));
