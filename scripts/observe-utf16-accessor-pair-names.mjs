// Focused ES5 JavaScript emit observations; no complete-command qualification.
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
  ["underscore-identifier-literal", 'class C { get __x() { return 1; } set "__x"(value) {} }'],
  ["surrogate-accessor-pairs", String.raw`class C { get "\ud800"() { return 1; } set ["\ud800"](value) {} get "\ud801"() { return 2; } set ["\ud801"](value) {} get "\ufffd"() { return 3; } set ["\ufffd"](value) {} }`],
];
function observe(source) {
  const fileName = "/project/input.ts";
  const options = { target: ts.ScriptTarget.ES5, alwaysStrict: false, newLine: ts.NewLineKind.LineFeed, noLib: true };
  const outputs = [];
  const host = {
    getSourceFile: name => name === fileName ? ts.createSourceFile(name, source, options.target, true) : undefined,
    getDefaultLibFileName: () => "lib.d.ts",
    writeFile: (name, text) => outputs.push({ name, text }),
    getCurrentDirectory: () => "/project",
    getDirectories: () => [],
    fileExists: name => name === fileName,
    readFile: name => name === fileName ? source : undefined,
    getCanonicalFileName: name => name,
    useCaseSensitiveFileNames: () => true,
    getNewLine: () => "\n",
  };
  const result = ts.createProgram([fileName], options, host).emit();
  assert.equal(result.emitSkipped, false);
  assert.equal(outputs.length, 1);
  assert.equal(outputs[0].name, "/project/input.js");
  return outputs[0].text;
}
const cases = inputs.map(([case_id, source]) => {
  const expected = observe(source);
  assert.equal(observe(source), expected);
  return { case_id, source, expected };
});
const artifact = {
  version: 1, scope: "focused ES5 JavaScript emit only; no diagnostics/declaration/complete-command comparison",
  typescript: ts.version, compiler_sha256: compilerSha,
  observer_sha256: hash(fs.readFileSync(import.meta.filename)), repetitions: 2, cases,
};
const output = path.join(root, "crates/emitter/tests/fixtures/utf16-accessor-pair-names.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", { flag: "wx" });
else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(JSON.stringify({ output, sha256: hash(fs.readFileSync(output)), cases: cases.length, repetitions: 2 }));
