// §10.4 separately scoped raw UTF-16 source limitation, not an emit witness.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const hash = value => crypto.createHash("sha256").update(value).digest("hex");
const compilerSha = hash(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const units = value => Array.from({length: value.length}, (_, i) => value.charCodeAt(i));
function observe() {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "tsc-raw-utf16-boundary-"));
  const rows = [];
  try {
    for (const encoding of ["utf16le", "utf16be"]) {
      for (const [kind, values] of [["different-high", ["\ud800", "\ud801"]], ["low", ["\udc00"]], ["paired", ["\ud800\udc00"]]]) {
        const source = "export const values = [" + values.map(value => '"' + value + '"').join(", ") + "];\n";
        const bytes = Buffer.from("\ufeff" + source, "utf16le");
        if (encoding === "utf16be") bytes.swap16();
        const name = path.join(temp, "input.ts");
        fs.writeFileSync(name, bytes);
        const read = ts.sys.readFile(name);
        assert.equal(read, source);
        const tree = ts.createSourceFile("/input.ts", read, ts.ScriptTarget.Latest, true);
        const literals = [];
        function visit(node) { if (ts.isStringLiteral(node)) literals.push(units(node.text)); ts.forEachChild(node, visit); }
        visit(tree);
        rows.push({id: encoding + "-" + kind, encoding, kind, bytes: [...bytes], source_units: units(read), literal_values: literals, parse_diagnostic_codes: tree.parseDiagnostics.map(d => d.code)});
      }
    }
  } finally { fs.rmSync(temp, {recursive: true}); }
  return rows;
}
const cases = observe(); assert.deepEqual(observe(), cases);
const artifact = {version: 1, scope: "TypeScript sys.readFile and parser on raw UTF-16 bytes; Rust lone-surrogate source rejection is a separately scoped limitation (§10.4)", typescript: ts.version, compiler_sha256: compilerSha, observer_sha256: hash(fs.readFileSync(import.meta.filename)), repetitions: 2, cases};
const output = path.join(root, "crates/program/tests/fixtures/utf16-raw-source-boundary.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output, "utf8")), artifact);
console.log(JSON.stringify({output, sha256: hash(fs.readFileSync(output)), cases: cases.length, repetitions: 2}));
