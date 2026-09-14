// NoAsciiEscaping under a synthetic .d.ts type alias, including wire bytes.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = hash(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const units = value => Array.from({length: value.length}, (_, i) => value.charCodeAt(i));
function observe() {
  const cases = [];
  for (const [id, value] of [["high", [0xd800]], ["other-high", [0xd801]], ["low", [0xdc00]], ["pair", [0xd800, 0xdc00]]]) {
    for (const emit_flags of [0, ts.EmitFlags.NoAsciiEscaping]) {
      const source = ts.createSourceFile("main.d.ts", "", ts.ScriptTarget.Latest, true);
      const literal = ts.factory.createStringLiteral(String.fromCharCode(...value));
      ts.setEmitFlags(literal, emit_flags);
      const node = ts.factory.createTypeAliasDeclaration(undefined, "T", undefined, ts.factory.createLiteralTypeNode(literal));
      const text = ts.createPrinter({newLine: ts.NewLineKind.CarriageReturnLineFeed}).printNode(ts.EmitHint.Unspecified, node, source);
      const bytes = Buffer.from(text);
      cases.push({id: id + "/" + emit_flags, value, emit_flags, text_utf16: units(text), utf8_base64: bytes.toString("base64"), utf8_bytes: bytes.length});
    }
  }
  return cases;
}
const cases = observe(); assert.deepEqual(observe(), cases);
const artifact = {version: 1, scope: "synthetic declaration type alias printer: literal type child with/without NoAsciiEscaping; output UTF-16 and UTF-8 bytes, not a separate full compiler command", typescript: ts.version, compiler_sha256: compilerSha, observer_sha256: hash(fs.readFileSync(import.meta.filename)), repetitions: 2, cases};
const output = path.join(root, "crates/emitter/tests/fixtures/utf16-declaration-literal-printer.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(JSON.stringify({output, sha256: hash(fs.readFileSync(output)), cases: cases.length, repetitions: 2}));
