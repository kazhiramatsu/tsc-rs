// Syntax provenance for the original 36 emit refusals. This is not an emit
// compatibility verdict; complete commands are replayed by emitter_final_universe.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const inputPath = "docs/design/greenfield/slices/emitter-final-batch/integration/cross-review/recovery-inputs-r20.json";
const inputs = JSON.parse(fs.readFileSync(path.join(root, inputPath)));
assert.equal(inputs.length, 36);
assert.equal(ts.version, "6.0.3");
const units = text => Array.from({length: text.length}, (_, i) => text.charCodeAt(i));
function observe(file, target) {
  const source = ts.createSourceFile(file.path, file.text, {
    languageVersion: target,
    jsDocParsingMode: ts.JSDocParsingMode.ParseForTypeErrors,
  }, true);
  const missing = [];
  function visit(node, parent) {
    if (node.kind === ts.SyntaxKind.MissingDeclaration ||
        node.pos === node.end && node.kind !== ts.SyntaxKind.EndOfFileToken &&
        node.kind !== ts.SyntaxKind.SourceFile) {
      missing.push({kind: node.kind, pos: node.pos, end: node.end, parent: parent?.kind ?? null});
    }
    ts.forEachChild(node, child => { visit(child, node); });
  }
  visit(source, undefined);
  return {
    missing,
    diagnostics: source.parseDiagnostics.map(d => ({code: d.code, start: d.start, length: d.length,
      message_utf16: units(ts.flattenDiagnosticMessageText(d.messageText, "\n"))})),
  };
}
const cases = inputs.map(input => ({
  case_id: input.case_id,
  target: input.options.target ?? ts.ScriptTarget.ES5,
  files: input.input.files.map(file => {
    const expected = observe(file, input.options.target ?? ts.ScriptTarget.ES5);
    assert.deepEqual(observe(file, input.options.target ?? ts.ScriptTarget.ES5), expected, input.case_id);
    return {...file, expected};
  }),
}));
const artifact = {
  version: 1, typescript: ts.version, repetitions: 2,
  purpose: "syntax-provenance-only-not-emit-compatibility",
  compiler_sha256: sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  inputs: {path: inputPath, sha256: sha(fs.readFileSync(path.join(root, inputPath)))},
  cases,
};
const output = path.join(root, "crates/syntax/tests/fixtures/emitter-recovery.json");
const rendered = JSON.stringify(artifact, null, 2) + "\n";
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, rendered, {flag: "wx"});
else assert.equal(fs.readFileSync(output, "utf8"), rendered);
console.log("Emitter recovery syntax: 36 inputs, two identical TypeScript parses each");
