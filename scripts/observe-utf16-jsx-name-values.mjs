// Pinned JSX option/value emit observations, not complete-command evidence.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = hash(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const units = value => Array.from({ length: value.length }, (_, index) => value.charCodeAt(index));
const inputs = [
  ...[0xd800, 0xd801, 0xdc00, 0xfffd].map(unit => ({
    case_id: `namespace-${unit.toString(16)}`, settings: { reactNamespace: `R${String.fromCharCode(unit)}` },
  })),
  { case_id: "factory-comment", settings: { jsxFactory: "R/*\ud800*/.createElement" } },
  { case_id: "factory-escaped-identifier", settings: { jsxFactory: String.raw`R.\u0063reateElement` } },
  { case_id: "fragment-factory-comment", source: "export const x = <></>;", settings: { jsxFragmentFactory: "R/*\ud801*/.Fragment" } },
  { case_id: "automatic-import", settings: { jsx: ts.JsxEmit.ReactJSX, jsxImportSource: "./\ud800" } },
  { case_id: "development-import", settings: { jsx: ts.JsxEmit.ReactJSXDev, jsxImportSource: "./\ud801" } },
  { case_id: "development-file-name", file_name: "/project/\ud800.tsx", settings: { jsx: ts.JsxEmit.ReactJSXDev } },
  { case_id: "attribute-entities", source: 'export const x = <tag a="&#xD800;" b="&#xD801;" c="&#xFFFD;" d="&#xD800;&#xDC00;" />;', settings: {} },
];
function observe(input) {
  const fileName = input.file_name ?? "/project/input.tsx";
  const source = input.source ?? "export const x = <tag />;";
  const settings = { jsx: ts.JsxEmit.React, ...input.settings };
  const options = { target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.ESNext, noLib: true, alwaysStrict: false, newLine: ts.NewLineKind.LineFeed, ...settings };
  const outputs = [];
  const host = {
    getSourceFile: name => name === fileName ? ts.createSourceFile(name, source, options.target, true, ts.ScriptKind.TSX) : undefined,
    getDefaultLibFileName: () => "lib.d.ts", getCurrentDirectory: () => "/project", getDirectories: () => [],
    fileExists: name => name === fileName, readFile: name => name === fileName ? source : undefined,
    getCanonicalFileName: name => name, useCaseSensitiveFileNames: () => true, getNewLine: () => "\n",
    writeFile: (name, text) => outputs.push({ name: units(name), text: units(text) }),
  };
  assert.equal(ts.createProgram([fileName], options, host).emit().emitSkipped, false);
  assert.equal(outputs.length, 1);
  return { case_id: input.case_id, file_name: units(fileName), source,
    settings: Object.fromEntries(Object.entries(settings).map(([key, value]) => [key, typeof value === "string" ? units(value) : value])),
    output: outputs[0] };
}
const cases = inputs.map(input => { const first = observe(input); assert.deepEqual(observe(input), first); return first; });
const artifact = { version: 1, scope: "focused JSX JavaScript emit and callback filenames; no diagnostics/declarations/full-command comparison", typescript: ts.version, compiler_sha256: compilerSha, observer_sha256: hash(fs.readFileSync(import.meta.filename)), repetitions: 2, cases };
const output = path.join(root, "crates/emitter/tests/fixtures/utf16-jsx-name-values.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", { flag: "wx" });
else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(JSON.stringify({ output, sha256: hash(fs.readFileSync(output)), cases: cases.length, repetitions: 2 }));
