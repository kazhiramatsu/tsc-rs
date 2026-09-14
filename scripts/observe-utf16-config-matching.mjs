// Pure wildcard matching and filename case normalization, with no host I/O.
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
const specs = ["*.ts", "?.ts", "??.ts", "\ud800.ts", "\ud801.ts", "\udc00.ts", "\ud800?.ts", "?\udc00.ts", "\ud800\udc00.ts",
  "\ud800/*.ts", "\ud800/**/*.ts", "\ud800", "\ud800/**", "", "../project/\ud801.ts", "C:/\ud800/*.ts",
  "//\ud800/server/*.ts", "https://\ud800/*.ts", "*.js", "*.min.js", "Ο?.ts"];
const paths = ["a.ts", "\ud800.ts", "\ud801.ts", "\udc00.ts", "\ufffd.ts", "\ud800\udc00.ts", "\ud800x.ts", "x\udc00.ts",
  "\ud800/a.ts", "\ud801/a.ts", "\ud800/nested/a.ts", "\ud800/node_modules/a.ts", "\ud800/.hidden.ts", "\ud800/A.TS",
  "\ud800.min.js", "\ud800.js", "\ud801/../\ud800.ts", "ΟΣ.ts", "οσ.ts"].map(p => "/project/" + p)
  .concat(["C:/\ud800/a.ts", "//\ud800/server/a.ts", "https://\ud800/a.ts"]);
const matching = [];
for (const [index, spec] of specs.entries()) for (const case_sensitive of [true, false]) {
  const observe = () => {
    const regexp = ts.getRegularExpressionForWildcard([spec], "/project", "files");
    return {compiled: regexp !== undefined, matches: paths.map(p => regexp !== undefined && new RegExp(regexp, case_sensitive ? "" : "i").test(ts.normalizePath(p)))};
  };
  const expected = observe(); assert.deepEqual(observe(), expected);
  matching.push({case_id: `${index}/${case_sensitive}`, spec_utf16: units(spec), base: "/project", case_sensitive, expected});
}
const folding = ["A\ud800B", "A\ud801B", "A\udc00B", "A\ufffdB", "ΟΣ", "ΟΣ\ud800", "Ο\ud800Σ", "\ud800ΟΣ", "ΟΣ\ud800Α",
  "ΟΣa", "ΟΣ\u0130A", "İıßΣ", "\ud801\udc00", "\ud801\udc28", "\ud800/Σ", "A\ud800\ud801B", "A\udc00\udc01B"]
  .map(value => {const expected = units(ts.toFileNameLowerCase(value)); assert.deepEqual(units(ts.toFileNameLowerCase(value)), expected); return {value_utf16: units(value), expected};});
const artifact = {version: 1, typescript: ts.version, compiler_sha256: compilerSha,
  observer_sha256: sha(fs.readFileSync(import.meta.filename)), repetitions: 2, paths_utf16: paths.map(units), matching, folding};
const output = path.join(root, "crates/program/tests/fixtures/utf16-config-matching.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)), artifact);
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), matching: matching.length * paths.length, folding: folding.length}));
