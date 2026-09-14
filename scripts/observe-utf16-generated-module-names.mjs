// Module-name basename and ASCII generated-name projection, no host I/O.
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
const inputs = ["", "module", "@scope/package", "./1-x.ts", "./$name", "./_name", "./\ud800", "./\ud801", "./\udc00", "./\ufffd",
  "./\ud800\udc00", "./\ud800x\udc00", "./\ud800/", "./\ud800//", "./\ud800\\x", "/", "//server", "//\ud800", "//\ud800/", "//\ud800/x",
  "C:", "C:/", "C:/\ud800/", "C:\ud800", "https://host", "https://\ud800", "https://\ud800/", "https://\ud800/\ud801", "file:///c:", "file:///c:/", "file:///c:/\ud800", "Ο.Σ", "./a\u0000b"];
const cases = inputs.map((value, index) => {
  const observe = () => ({base_name_utf16: units(ts.getBaseFileName(value)), generated_name: ts.makeIdentifierFromModuleName(value)});
  const expected = observe(); assert.deepEqual(observe(), expected);
  return {case_id: String(index), value_utf16: units(value), expected};
});
const artifact = {version: 1, typescript: ts.version, compiler_sha256: compilerSha,
  observer_sha256: sha(fs.readFileSync(import.meta.filename)), repetitions: 2, cases};
const output = path.join(root, "crates/program/tests/fixtures/utf16-generated-module-names.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)), artifact);
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length}));
