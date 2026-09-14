// Printer API witness for declaration pre-scan, including an unlowered ModuleBlock.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const body = "secondBinding(); firstBinding(); var firstBinding, secondBinding;";
const inputs = [
  {id:"source-file", source:body},
  {id:"module-block", source:`namespace N { ${body} }`},
  {id:"function-body", source:`function f() { ${body} }`},
];
function observe(input) {
  const source = ts.createSourceFile("main.ts", input.source, ts.ScriptTarget.Latest, true);
  const first = ts.factory.createUniqueName("templateObject"), second = ts.factory.createUniqueName("templateObject");
  const transformed = ts.transform(source, [context => root => {
    const visit = node => ts.isIdentifier(node) && node.text === "firstBinding" ? first
      : ts.isIdentifier(node) && node.text === "secondBinding" ? second
      : ts.visitEachChild(node, visit, context);
    return ts.visitNode(root, visit);
  }]);
  try {
    return ts.createPrinter({newLine:ts.NewLineKind.LineFeed}).printFile(transformed.transformed[0]);
  } finally { transformed.dispose(); }
}
const cases = inputs.map(input => {
  const runs = [observe(input), observe(input)];
  assert.equal(runs[0], runs[1]);
  assert.ok(runs[0].includes("templateObject_2();"));
  assert.ok(runs[0].includes("var templateObject_1, templateObject_2;"));
  return {...input, expected:runs[0]};
});
const artifact = {version:1, scope:"Synthetic generated identifier printer API only; not command qualification",
  typescript:ts.version, compiler_sha256:compilerSha, observer_sha256:sha(fs.readFileSync(import.meta.filename)),
  repetitions:2, printer_executions:cases.length*2, cases};
const output = path.join(root,"crates/emitter/tests/fixtures/utf16-generated-declaration-order.json");
assert.ok(["--write","--check"].includes(process.argv[2]));
if(process.argv[2]==="--write") fs.writeFileSync(output,JSON.stringify(artifact,null,2)+"\n",{flag:"wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)),artifact);
console.log(JSON.stringify({output,sha256:sha(fs.readFileSync(output)),cases:cases.length,executions:cases.length*2}));
