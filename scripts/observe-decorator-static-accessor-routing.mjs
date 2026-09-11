// Fresh source-owned print controls for the static-accessor regression test.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/emitter/tests/fixtures/decorator-static-accessor-routing.json");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const source = "const dec = (_value, _context) => {};\nclass C { @dec static accessor value = 1; }\n";
const fileName = "decorator-static-accessor.ts";
const cases = [];
for (const target of [ts.ScriptTarget.ES2022, ts.ScriptTarget.ESNext]) {
  for (const define of [false, true]) {
    const options = { target, module: ts.ModuleKind.Preserve,
      useDefineForClassFields: define, alwaysStrict: false,
      newLine: ts.NewLineKind.LineFeed };
    const observe = () => {
      const result = ts.transpileModule(source, { compilerOptions: options, fileName,
        reportDiagnostics: true });
      assert.equal(result.sourceMapText, undefined);
      return { text: result.outputText, diagnostics: result.diagnostics.map(diagnostic => ({
        code: diagnostic.code, category: ts.DiagnosticCategory[diagnostic.category],
        message: ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n") })) };
    };
    const observed = observe();
    assert.deepEqual(observe(), observed);
    cases.push({ case_id: `${target === ts.ScriptTarget.ESNext ? "esnext" : "es2022"}/${define ? "define" : "set"}`,
      target, use_define_for_class_fields: define,
      expected: observed.text, source_diagnostics: observed.diagnostics });
  }
}
const result = { version: 1, status: "source-owned printed output; not complete program command observations",
  typescript: ts.version, compiler_sha256: sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  file_name: fileName, source, module: ts.ModuleKind.Preserve, repetitions: 2, cases };
if (process.argv[2] === "--write") {
  fs.writeFileSync(destination, JSON.stringify(result, null, 2) + "\n", { flag: "wx" });
} else {
  assert.deepEqual(JSON.parse(fs.readFileSync(destination, "utf8")), result);
}
console.log(JSON.stringify({ cases: cases.length, repetitions: 2, destination,
  sha256: sha(fs.readFileSync(destination)),
  routing: cases.map(c => ({ case_id: c.case_id,
    retains_static_accessor: c.expected.includes("static accessor value"),
    lowers_to_static_getter: c.expected.includes("static get value()") })) }));
