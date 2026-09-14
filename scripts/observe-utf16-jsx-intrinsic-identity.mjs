// JSX intrinsic literal tag identity, independent of output-file encoding.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = hash(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const units = text => Array.from({ length: text.length }, (_, i) => text.charCodeAt(i));
function observe() {
  const fileName = "/a.tsx";
  const source = String.raw`declare namespace JSX { interface Element {} interface IntrinsicElements { "\uD800": {a: number}; "\uD801": {b: string}; "\uDC00": {c: boolean}; "�": {d: number}; } }
declare const High: "\uD800", Other: "\uD801", Low: "\uDC00", Replacement: "�";
<High a={1}/>; <Other b="ok"/>; <Low c={true}/>; <Replacement d={2}/>;
<High b="bad"/>; <Other a={1}/>; <Low a={1}/>;`;
  const file = ts.createSourceFile(fileName, source, ts.ScriptTarget.ESNext, true);
  const host = {
    getSourceFile: name => name === fileName ? file : undefined,
    getDefaultLibFileName: () => "", getCurrentDirectory: () => "/",
    getDirectories: () => [], fileExists: name => name === fileName,
    readFile: name => name === fileName ? source : undefined,
    getCanonicalFileName: name => name, useCaseSensitiveFileNames: () => true,
    getNewLine: () => "\n", writeFile() {},
  };
  const program = ts.createProgram([fileName], { noLib: true, jsx: ts.JsxEmit.Preserve }, host);
  return { source, diagnostics: program.getSemanticDiagnostics(file).map(diagnostic => ({
    code: diagnostic.code, start: diagnostic.start, length: diagnostic.length,
    message: units(ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")),
  })) };
}
const first = observe(); assert.deepEqual(observe(), first); const cases = [first];
const artifact = {
  version: 1, scope: "JSX intrinsic literal tag identity and full semantic diagnostic values; no complete-command qualification",
  typescript: ts.version, compiler_sha256: compilerSha,
  observer_sha256: hash(fs.readFileSync(import.meta.filename)), repetitions: 2, cases,
};
const output = path.join(root, "crates/checker/tests/fixtures/utf16-jsx-intrinsic-identity.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", { flag: "wx" });
else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(JSON.stringify({ output, sha256: hash(fs.readFileSync(output)), cases: cases.length, repetitions: 2 }));
