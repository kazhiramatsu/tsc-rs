// Pure path API values, preserving UTF-16 names before host encoding.
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
const paths = ["", ".", "..", "a/", "./a/", "a/./b", "a/../b", "a//b//", "../a/../../b", "/", "/../a", "C:", "C:a", "C:/a/../b/", "\\\\server\\share\\..\\a", "\\\\server/part/a/..", "//server/share/../a", "file:///C%3a/a/../b", "file://localhost/C:/a/..", "file://host\\part/a/..", "https://host/a/../b", "https://host", "x:/a/..", "//\uD800/\uDC00/../x", "./\uD800", "./\uD801", "./�", "./\\uD800", "./\uD83D\uDE00/../\uD800", "\uD800/../\uDC00", "./\uD800/./\uDC00/", "https://\uD800/\uDC00/..", "a\0b/../\uD800"];
const bases = ["", "/root", "C:/root/", "//server/root", "file:///C:/root", "/\uD800/root", "\uD800/\uDC00"];
const cases = [];
for (const [pi, p] of paths.entries()) for (const [bi, b] of bases.entries()) {
  const observe = () => ({root_length: ts.getRootLength(p), normalized_utf16: units(ts.getNormalizedAbsolutePath(p, b)), combined_utf16: units(ts.combinePaths(b, p))});
  const expected = observe(); assert.deepEqual(observe(), expected);
  cases.push({case_id: `path-${pi}-base-${bi}`, path_utf16: units(p), base_utf16: units(b), ...expected});
}
const artifact = {version: 1, scope: "getRootLength/getNormalizedAbsolutePath/combinePaths values only; no host or module resolution qualification", typescript: ts.version,
  compiler_sha256: compilerSha, observer_sha256: sha(fs.readFileSync(import.meta.filename)), repetitions: 2, executions: cases.length * 2, cases};
const output = path.join(root, "crates/program/tests/fixtures/utf16-lexical-paths.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)), artifact);
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length, executions: cases.length * 2}));
