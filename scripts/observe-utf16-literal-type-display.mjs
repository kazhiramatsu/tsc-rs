// Checker/printer/diagnostic API values, independent of output-file encoding.
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
function observe(value) {
  const fileName = "/main.ts";
  const source = `type T = ${JSON.stringify(value)}; const value: T = 0;`;
  const file = ts.createSourceFile(fileName, source, ts.ScriptTarget.ESNext, true);
  const host = {
    getSourceFile: name => name === fileName ? file : undefined,
    getDefaultLibFileName: () => "", getCurrentDirectory: () => "/",
    getDirectories: () => [], fileExists: name => name === fileName,
    readFile: name => name === fileName ? source : undefined,
    getCanonicalFileName: name => name, useCaseSensitiveFileNames: () => true,
    getNewLine: () => "\n", writeFile() {},
  };
  const program = ts.createProgram([fileName], { noLib: true, target: ts.ScriptTarget.ESNext }, host);
  const checker = program.getTypeChecker();
  const type = checker.getTypeFromTypeNode(file.statements[0].type);
  const literal = ts.factory.createStringLiteral(value);
  ts.setEmitFlags(literal, ts.EmitFlags.NoAsciiEscaping);
  const node = ts.factory.createLiteralTypeNode(literal);
  return {
    value: units(value), source, display: units(checker.typeToString(type)),
    printed: units(ts.createPrinter().printNode(ts.EmitHint.Unspecified, node, file)),
    diagnostics: program.getSemanticDiagnostics(file).map(diagnostic => ({
      code: diagnostic.code, start: diagnostic.start, length: diagnostic.length,
      message: units(ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")),
    })),
  };
}
const cases = ["\ud800", "\ud801", "\udc00", "\ufffd", "\u{10000}", "\uE000", "a\0" + "1\n\"\\", "\u0085\u2028\u2029"].map(value => {
  const first = observe(value); assert.deepEqual(observe(value), first); return first;
});
const artifact = {
  version: 1, scope: "typeToString, NoAsciiEscaping factory printer, and semantic diagnostic UTF16 values; no complete-command qualification",
  typescript: ts.version, compiler_sha256: compilerSha,
  observer_sha256: hash(fs.readFileSync(import.meta.filename)), repetitions: 2, cases,
};
const output = path.join(root, "crates/checker/tests/fixtures/utf16-literal-type-display.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", { flag: "wx" });
else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(JSON.stringify({ output, sha256: hash(fs.readFileSync(output)), cases: cases.length, repetitions: 2 }));
