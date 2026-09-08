// H2.8a A6-14 printer dependency: StringLiteral.textSourceNode identifiers.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const destination = new URL("../crates/emitter/tests/fixtures/string-literal-identifier-source.json", import.meta.url);
const identifiers = [
 ["plain", "value"], ["underscores", "__name"], ["unicode", "名前"],
 ["escaped", "\\u0061"], ["extended-escape", "\\u{61}"],
 ["astral", "𐀀"], ["astral-escape", "\\u{10000}"], ["mixed", "n\\u0061me"],
];
const strings = [
 ["single-quote", "'a\\\"b'"], ["line-continuation", "'a\\\nb'"],
 ["string-unicode", '"名前"'], ["lone-surrogate", '"\\uD800"'],
];
function observe(source, origin, no_ascii_escaping) {
 const file = ts.createSourceFile("literal-source.ts", source, ts.ScriptTarget.Latest, true);
 assert.equal(file.parseDiagnostics.length, 0);
 let name = file.statements[0].expression;
 if (origin === "foreign") name = ts.createSourceFile("foreign.ts", source, ts.ScriptTarget.Latest, true).statements[0].expression;
 if (origin === "clone") name = ts.factory.cloneNode(name);
 if (origin === "synthetic") name = ts.factory.createIdentifier(ts.idText(name));
 const literal = ts.factory.createStringLiteralFromNode(name);
 if (no_ascii_escaping) ts.setEmitFlags(literal, ts.EmitFlags.NoAsciiEscaping);
 const updated = ts.factory.updateSourceFile(file, [ts.factory.createExpressionStatement(literal)]);
 return ts.createPrinter({newLine:ts.NewLineKind.LineFeed}).printFile(updated);
}
const cases = [];
for (const [shape, spelling] of [...identifiers, ...strings]) {
 for (const origin of (identifiers.some(([name]) => name === shape) ? ["parsed", "clone", "synthetic", "foreign"] : ["parsed"])) {
  for (const no_ascii_escaping of [false, true]) {
   const source = spelling + ";\n";
   const output = observe(source, origin, no_ascii_escaping);
   assert.equal(observe(source, origin, no_ascii_escaping), output);
   cases.push({case_id:`${shape}/${origin}/${no_ascii_escaping ? "unicode" : "ascii"}`,source,origin,no_ascii_escaping,output});
  }
 }
}
assert.equal(cases.length,72);
const artifact = {version:1,typescript:ts.version,repetitions:2,
 compiler_sha256:sha256(fs.readFileSync(new URL("../vendor/typescript-6.0.3/lib/typescript.js",import.meta.url))),
 observer_sha256:sha256(fs.readFileSync(import.meta.filename)),cases};
const bytes = JSON.stringify(artifact,null,2)+"\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination,bytes);
else assert.equal(fs.readFileSync(destination,"utf8"),bytes);
console.log(`String literal identifier source: ${cases.length} printer outputs, each identical twice`);
