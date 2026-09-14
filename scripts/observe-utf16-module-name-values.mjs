// Focused module-name JavaScript emit observations; no complete-command qualification.
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
  ["quoted-re-export-identities", String.raw`export { "\ud800" as "\ud800", "\ud801" as "\ud801", "\ufffd" as "\ufffd" } from "./dep";`],
  ["dependency-group-identities", String.raw`export { x as a } from "./\ud800"; export { x as b } from "./\ud801"; export { x as c } from "./\ufffd"; export { y as d } from "./\ud800";`],
  ["local-export-alias-identities", String.raw`const x = 1; export { x as "\ud800", x as "\ud801", x as "\ufffd" };`],
  ["rewritten-relative-identities", String.raw`export { x as a } from "./\ud800.ts"; export { x as b } from "./\ud801.ts"; export { x as c } from "./\ufffd.ts"; export { x as d } from "./\udc00.mts";`],
];
const formats = [
  ["common-js", ts.ModuleKind.CommonJS],
  ["amd", ts.ModuleKind.AMD],
  ["system", ts.ModuleKind.System],
];
function observe(source, module) {
  const fileName = "/project/input.ts";
  const options = { target: ts.ScriptTarget.ES2015, module, rewriteRelativeImportExtensions: true, alwaysStrict: false, newLine: ts.NewLineKind.LineFeed, noLib: true };
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
const cases = formats.flatMap(([format, module]) => inputs.map(([name, source]) => {
  const expected = observe(source, module);
  assert.equal(observe(source, module), expected);
  return { case_id: `${format}-${name}`, module, source, expected };
}));
const artifact = {
  version: 1, scope: "focused ES2015 CommonJS/AMD/System JavaScript emit only; no diagnostics/declaration/complete-command comparison",
  typescript: ts.version, compiler_sha256: compilerSha,
  observer_sha256: hash(fs.readFileSync(import.meta.filename)), repetitions: 2, cases,
};
const output = path.join(root, "crates/emitter/tests/fixtures/utf16-module-name-values.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", { flag: "wx" });
else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(JSON.stringify({ output, sha256: hash(fs.readFileSync(output)), cases: cases.length, repetitions: 2 }));
