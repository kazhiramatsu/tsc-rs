// Source files whose statements are all prologue directives: tsc emits the
// prologues first, then emitSourceFile still runs emitBodyWithDetachedComments
// for the file, so a detached comment prefix is written again after them with
// nothing else to follow (the synthesized "use strict" makes statements[0]
// synthesized). Observed from pinned TypeScript through the complete program
// emit; the fixture pins the emitted JavaScript text, exit code and codes.
// node scripts/observe-prologue-only-detached-comments.mjs --write|--check
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");

const inputs = [
  {id: "detached-two-lines-custom-only", source: '// A\n// B\n\n"custom";\n'},
  {id: "detached-one-line-custom-only", source: '// A\n\n"custom";\n'},
  {id: "detached-original-use-strict-only", source: '// A\n// B\n\n"use strict";\n'},
  {id: "detached-two-custom-prologues", source: '// A\n// B\n\n"custom";\n"other";\n'},
  {id: "detached-custom-then-statement", source: '// A\n// B\n\n"custom";\nvar x = 1;\n'},
  {id: "attached-header-custom-only", source: '// A\n"custom";\n'},
  {id: "block-detached-custom-only", source: '/* A */\n\n"custom";\n'},
  {id: "detached-custom-invalid-escape-only", source: '// A\n// B\n\n"\\u000G";\n'},
];
const options = {target: ts.ScriptTarget.ES2015, newLine: ts.NewLineKind.LineFeed, skipDefaultLibCheck: true, noErrorTruncation: true};

function observe(input) {
  const fileName = "/project/main.ts";
  const base = ts.createCompilerHost(options, true);
  const host = {...base, getCurrentDirectory: () => "/project",
    getSourceFile: (name, version) => name === fileName ? ts.createSourceFile(name, input.source, version, true) : base.getSourceFile(name, version),
    fileExists: name => name === fileName || base.fileExists(name),
    readFile: name => name === fileName ? input.source : base.readFile(name),
    writeFile: () => assert.fail("writes must use the captured callback")};
  const program = ts.createProgram([fileName], options, host);
  const writes = [], diagnostics = [];
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => diagnostics.push(d.code), () => {}, undefined,
    (name, text) => writes.push({path: name, text}));
  assert.equal(writes.length, 1);
  assert.equal(writes[0].path, "/project/main.js");
  return {js_text: writes[0].text, exit_code: exit, diagnostic_codes: diagnostics};
}

const cases = inputs.map(input => {
  const runs = [observe(input), observe(input)];
  assert.deepEqual(runs[0], runs[1], input.id);
  return {...input, source_sha256: sha(Buffer.from(input.source)), ...runs[0]};
});
const artifact = {version: 1,
  scope: "prologue-only source files: detached comment prefix emitted after the prologue directives; emitted JavaScript text, exit code and diagnostic codes from the pinned compiler",
  typescript: ts.version, compiler_sha256: compilerSha, observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  repetitions: 2, options: {target: "es2015", newLine: "lf"}, cases};
const output = path.join(root, "crates/compiler/tests/fixtures/prologue-only-detached-comments.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)), artifact);
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length}));
