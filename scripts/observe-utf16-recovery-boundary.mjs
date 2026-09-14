// Repeated syntax API observations for B's recovery controls. Admission is a
// reviewed Rust policy, not a TypeScript API observation or command verdict.
// node scripts/observe-utf16-recovery-boundary.mjs --write|--check
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
  ["string-octal", String.raw`const x = "\1";`, true],
  ["string-decimal", String.raw`const x = "\8";`, true],
  ["string-hex", String.raw`const x = "\xG";`, true],
  ["string-newline", "const x = 'unterminated\n;", true],
  ["template-rescans", String.raw`const x = \`\8\${a}\9\${b}\xG\`;`.replaceAll("\\`", "`").replaceAll("\\$", "$"), true],
  ["tagged-no-report", "const x = tag`\\unicode`;", true],
  ["optional-grammar", "function f(a?: number) {};; const a = [,];", true],
  ["await-reparse", "export {}; await f();", true],
  ["await-retains-literals", String.raw`export {}; const a = "\8"; await f(); const b = "\9";`, true],
  ["numeric-and-string", String.raw`const x = 1__0; const y = "\8";`, false],
  ["identifier", String.raw`const \u00G0 = 1;`, false],
  ["regexp", "const x = /unterminated", false],
  ["comment", "/* unterminated", false],
  ["invalid-binding", String.raw`const "\8" = 1;`, false],
  ["structural-before-await", "export {}; const x = ; await f();", false],
  ["structural-after-await", "export {}; await f(); const x = ;", false],
  ["structural-in-await", "export {}; await f(;", false],
  ["reference", "/// <reference path=oops />\nconst x = 1;", false],
  ["jsdoc", "/** @type {Array<} */ const x = 1;", true, "main.js"],
  ["jsx-text", "const x = <div>}</div>;", false, "main.tsx"],
];
const units = text => Array.from({length: text.length}, (_, i) => text.charCodeAt(i));
const diagnostics = rows => (rows ?? []).map(d => ({code: d.code, start: d.start, length: d.length,
  message_utf16: units(ts.flattenDiagnosticMessageText(d.messageText, "\n"))}));
const cases = inputs.map(([case_id, source, literal_only_contract, file_name = "main.ts"]) => {
  function observe() {
    const parsed = ts.createSourceFile(file_name, source, ts.ScriptTarget.ES2025, true);
    return {diagnostics: diagnostics(parsed.parseDiagnostics), jsdoc_diagnostics: diagnostics(parsed.jsDocDiagnostics)};
  }
  const expected = observe();
  assert.deepEqual(observe(), expected, case_id);
  return {case_id, file_name, source, source_sha256: sha(Buffer.from(source)), literal_only_contract, expected};
});
const artifact = {version: 1, scope: "Syntax diagnostics only. literal_only_contract is reviewed policy, not TS observation. No command qualification.",
  typescript: ts.version, compiler_sha256: compilerSha, observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  repetitions: 2, parser_executions: cases.length * 2, cases};
const output = path.join(root, "crates/syntax/tests/fixtures/utf16-recovery-boundary.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)), artifact);
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length, executions: cases.length * 2}));
