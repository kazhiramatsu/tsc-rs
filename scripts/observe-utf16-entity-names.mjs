// Isolated entity-name validity, preserving arbitrary UTF-16 values.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const units = text => Array.from({length: text.length}, (_, i) => text.charCodeAt(i));
const inputs = ["", "a", "a.b", "a..b", "default", "a.default", String.raw`\u0061.b`,
  String.raw`\uD800.b`, "\ud800a", "a\ud800", "a\ud801", "a.\udc00", "\ud801\udc00.x",
  "a/*\ud800*/", "a/*\ud801*/.b", "a/*\udc00*/.b", "/*\ud800*/a",
  "a//\ud800\n", "a//\ud800\r.b", "a//\ud800\u2028.b", "a//\ud800\u2029.b",
  "a//\ud800\u2028\ud801", "a/*\ud800*/b", "a/*\ud800", "a/**\ud800*/.b",
  "a/*/\ud800*/.b", "a/*\ud800*//*.*/b", "a/*\ud800*/./*\udc00*/b",
  "a/*\ud800\udc00*/.b", "a//\ud800", "'/*\ud800*/'", "`/*\ud800*/`",
  "/\ud800/", "a/*\ud800*/['x']", "a/*\ud800*/;", "a\ud800/*x*/",
  "a/*\ud800*/\\u002e.b", "a/*\ud800*/\\u0062", "a/*\ud800*/.\\u0062",
  "a/*\ud800*/.\\uD800", "a//\ud800\n.b", "a/*\ud800*/\u0000", "a/*\ud800*/\uFEFF.b"];
const cases = [];
for (const [index, value] of inputs.entries()) for (const target of [ts.ScriptTarget.ES5, ts.ScriptTarget.ES2015, ts.ScriptTarget.ESNext]) {
  const observe = () => ({entity: !!ts.parseIsolatedEntityName(value, target), identifier: ts.isIdentifierText(value, target)});
  const expected = observe();
  assert.deepEqual(observe(), expected);
  cases.push({case_id: `${index}/${target}`, value_utf16: units(value), target, expected});
}
const artifact = {version: 1, typescript: ts.version, compiler_sha256: compilerSha,
  observer_sha256: sha(fs.readFileSync(import.meta.filename)), repetitions: 2, cases};
const output = path.join(root, "crates/syntax/tests/fixtures/utf16-entity-names.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)), artifact);
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length}));
