// Exact catalog lookup and spelling-suggestion inputs; config parsing and
// complete command diagnostics remain separate qualification targets.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const sha = data => crypto.createHash("sha256").update(data).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const names = ["stric\ud800", "stric\ud801", "stric\udc00", "stric\ufffd", "stric\\uD800", "\ud800strict", "stric😀", "strict", "Strict", "target"];
const cases = names.map(name => {
  const observe = () => ({
    exact: ts.optionDeclarations.findLast(candidate => candidate.name === name)?.name ?? null,
    suggestion: ts.getSpellingSuggestion(name, ts.optionDeclarations, candidate => candidate.name)?.name ?? null,
  });
  const expected = observe();
  assert.deepEqual(observe(), expected);
  return {units: Array.from({length: name.length}, (_, i) => name.charCodeAt(i)), ...expected};
});
const content = JSON.stringify({
  scope: "Exact option lookup and getSpellingSuggestion API only",
  typescript_version: ts.version, compiler_sha256: compilerSha,
  observer_sha256: sha(fs.readFileSync(import.meta.filename)), repetitions: 2, cases,
}, null, 2) + "\n";
const output = path.join(root, "crates/program/tests/fixtures/utf16-config-names.json");
if (process.argv[2] === "--write") fs.writeFileSync(output, content, {flag: "wx"});
else if (process.argv[2] === "--check") assert.equal(fs.readFileSync(output, "utf8"), content);
else throw new Error("Expected --write or --check");
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length, repetitions: 2}));
