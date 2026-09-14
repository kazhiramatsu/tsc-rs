// Pinned standard-decorator runtime names and helper stems, not full commands.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = hash(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const inputs = [
  ["member-names", String.raw`class C { @dec "\uD800" = 1; @dec "\uD801"() {} @dec "\uFFFD"() {} @dec "\uDC00"() {} @dec "\uD800\uDC00"() {} @dec "ok"() {} @dec 7() {} @dec #x() {} }`],
  ["assigned-class-names", String.raw`const x = { "\uD800": @dec class {}, "\uD801": @dec class {}, "\uFFFD": @dec class {}, "valid": @dec class {} };`],
  ["assigned-static-accessor", String.raw`const x = { "\uD800": class { @dec static accessor #x = 1; }, "valid": class { @dec static accessor #x = 1; } };`],
];
function observe(source, target) {
  const fileName = "/project/input.ts";
  const options = { target, noLib: true, alwaysStrict: false, newLine: ts.NewLineKind.LineFeed, useDefineForClassFields: true };
  const outputs = [];
  const host = {
    getSourceFile: name => name === fileName ? ts.createSourceFile(name, source, target, true) : undefined,
    getDefaultLibFileName: () => "lib.d.ts", getCurrentDirectory: () => "/project", getDirectories: () => [],
    fileExists: name => name === fileName, readFile: name => name === fileName ? source : undefined,
    getCanonicalFileName: name => name, useCaseSensitiveFileNames: () => true, getNewLine: () => "\n",
    writeFile: (name, text) => outputs.push({ name, text }),
  };
  assert.equal(ts.createProgram([fileName], options, host).emit().emitSkipped, false);
  assert.equal(outputs.length, 1);
  assert.equal(outputs[0].name, "/project/input.js");
  assert.equal(outputs[0].text.isWellFormed(), true);
  return outputs[0].text;
}
const cases = [ts.ScriptTarget.ES2021, ts.ScriptTarget.ES2022].flatMap(target => inputs.map(([name, source]) => {
  const expected = observe(source, target); assert.equal(observe(source, target), expected);
  return { case_id: `${ts.ScriptTarget[target]}-${name}`, target, source, expected };
}));
const artifact = { version: 1, scope: "focused standard-decorator JavaScript emit only; no diagnostics/declarations/full-command comparison", typescript: ts.version, compiler_sha256: compilerSha, observer_sha256: hash(fs.readFileSync(import.meta.filename)), repetitions: 2, cases };
const output = path.join(root, "crates/emitter/tests/fixtures/utf16-decorator-name-values.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", { flag: "wx" });
else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(JSON.stringify({ output, sha256: hash(fs.readFileSync(output)), cases: cases.length, repetitions: 2 }));
