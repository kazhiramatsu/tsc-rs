// Pure getBasePaths observations; these do not qualify host lookup spelling.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const inputPath = "crates/program/tests/fixtures/h2-8b-config-discovery-spelling-inputs.json";
const outputPath = "crates/program/tests/fixtures/h2-8b-config-discovery-spelling.json";
const inputs = JSON.parse(fs.readFileSync(path.join(root, inputPath), "utf8"));
assert.equal(ts.version, "6.0.3");
assert.equal(inputs.cases.length, 6);
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") assert.ok(!fs.existsSync(path.join(root, outputPath)), "retain frozen observations");
const cases = inputs.cases.map(input => {
  const observe = () => ts.getFileMatcherPatterns(input.directory, undefined, input.includes ?? undefined,
    input.case_sensitive, input.directory).basePaths;
  const base_paths = observe();
  assert.deepEqual(observe(), base_paths);
  return {...input, base_paths};
});
const artifact = {version: 1, slice: inputs.slice, typescript: ts.version, repetitions: 2, program_executions: 0,
  inputs: {path: inputPath, sha256: sha(fs.readFileSync(path.join(root, inputPath)))},
  observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  source_sha256: sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/_tsc.js"))),
  compiler_sha256: sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))), cases};
const destination = path.join(root, outputPath);
if (process.argv[2] === "--write") fs.writeFileSync(destination, JSON.stringify(artifact, null, 2) + "\n");
else assert.deepEqual(JSON.parse(fs.readFileSync(destination, "utf8")), artifact);
console.log(JSON.stringify({cases: cases.length, repetitions: 2, sha256: sha(fs.readFileSync(destination))}));
