// Direct factory/printer values; this is not full declaration emit evidence.
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
const inputs = ["\ud800", "\ud801", "\udc00", "\ufffd", "__name", "\u{10000}", "default"];
function observe(value, clone) {
  const source = ts.createSourceFile("main.ts", "", ts.ScriptTarget.ESNext, false);
  const name = ts.factory.createIdentifier(value);
  const specifier = ts.factory.createExportSpecifier(false, "_name", clone ? ts.factory.cloneNode(name) : name);
  const declaration = ts.factory.createExportDeclaration(undefined, false, ts.factory.createNamedExports([specifier]));
  const text = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed }).printNode(ts.EmitHint.Unspecified, declaration, source);
  return { name: units(value), clone, kind: ts.SyntaxKind[specifier.name.kind], text: units(text) };
}
const cases = inputs.flatMap(value => [false, true].map(clone => {
  const first = observe(value, clone);
  assert.deepEqual(observe(value, clone), first);
  return first;
}));
const artifact = {
  version: 1, scope: "direct synthetic export factory and printer UTF16 values; no complete-command qualification",
  typescript: ts.version, compiler_sha256: compilerSha,
  observer_sha256: hash(fs.readFileSync(import.meta.filename)), repetitions: 2, cases,
};
const output = path.join(root, "crates/emitter/tests/fixtures/utf16-synthetic-export-names.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", { flag: "wx" });
else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(JSON.stringify({ output, sha256: hash(fs.readFileSync(output)), cases: cases.length, repetitions: 2 }));
