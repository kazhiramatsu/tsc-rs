// Replay the exact vendored priority owner against normalized physical paths.
// node scripts/observe-h2-8b-library-priority.mjs --write|--check
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const sourcePath = "vendor/typescript-6.0.3/lib/_tsc.js";
const source = fs.readFileSync(path.join(root, sourcePath), "utf8");
const owner = source.split("\n").slice(123123, 123138).join("\n") + "\n";
const ownerSha = sha(owner);
assert.equal(ownerSha, "76ba34e95562034f7cf2bde179f09ddac57adf36e403e26a589c8575d3759ae5");
const inputPath = "crates/program/tests/fixtures/h2-8b-library-priority-inputs.json";
const inputs = JSON.parse(fs.readFileSync(path.join(root, inputPath), "utf8"));
const destination = path.join(root, "crates/program/tests/fixtures/h2-8b-library-priority.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") assert.ok(!fs.existsSync(destination), "retain prior observations");
assert.equal(inputs.cases.length, 15);
const makeOwner = new Function("containsPath", "getBaseFileName", "removeSuffix", "removePrefix", "libs", "defaultLibraryPath", `return (${owner});`);
const cases = inputs.cases.map(input => {
  const priority = makeOwner(ts.containsPath, ts.getBaseFileName, ts.removeSuffix, ts.removePrefix, ts.libs, input.directory);
  const first = priority({fileName: input.file});
  assert.equal(priority({fileName: input.file}), first);
  return {...input, priority: first};
});
const artifact = {version: 1, typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  source: {path: sourcePath, sha256: sha(source), start_line: 123124, end_line: 123138, owner_sha256: ownerSha},
  compiler_sha256: sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  inputs: {path: inputPath, sha256: sha(fs.readFileSync(path.join(root, inputPath)))},
  repetitions: 2, program_executions: 0, cases};
if (process.argv[2] === "--write") fs.writeFileSync(destination, JSON.stringify(artifact, null, 2) + "\n");
else assert.deepEqual(JSON.parse(fs.readFileSync(destination, "utf8")), artifact);
console.log(JSON.stringify({cases: cases.length, repetitions: 2, program_executions: 0, sha256: sha(fs.readFileSync(destination)), priorities: cases.map(c => c.priority)}));
