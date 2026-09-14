// Package identity name operation only; no module/host resolution evidence.
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
const names = ["", "plain", "@scope/name", "@scope", "@scope/name/more", "@/name", "\uD800", "\uD801", "@\uD800/\uDC00", "@scope/\uD800", "@😀/name", "\\uD800"];
const cases = names.map((name, i) => {
  const first = units(ts.getTypesPackageName(name));
  assert.deepEqual(units(ts.getTypesPackageName(name)), first);
  return {case_id: `package-${i}`, name_utf16: units(name), types_package_utf16: first};
});
const artifact = {version: 1, scope: "getTypesPackageName API only; no module resolution qualification", typescript: ts.version,
  compiler_sha256: compilerSha, observer_sha256: sha(fs.readFileSync(import.meta.filename)), repetitions: 2, executions: cases.length * 2, cases};
const output = path.join(root, "crates/program/tests/fixtures/utf16-package-names.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)), artifact);
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length, executions: cases.length * 2}));
