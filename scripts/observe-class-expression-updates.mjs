// Auxiliary TS factory transitions; complete expression commands are in class-transform-flags.json.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname,"..");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version,"6.0.3");
assert.ok(["--write","--check"].includes(process.argv[2]));
const text = 'const key = "value";\nconst empty = class Empty {};\nconst withStatic = class Static { static value = 1; };\nconst withComputed = class Computed { [key] = 1; };\n';
function observe(extension) {
  const file_name = "/project/expression-updates." + extension;
  const source = ts.createSourceFile(file_name,text,ts.ScriptTarget.ES2015,true);
  const classes = new Map();
  function visit(node) { if (ts.isClassExpression(node)) classes.set(node.name.text,node); ts.forEachChild(node,visit); }
  visit(source);
  return [["add-static","Empty","Static"],["remove-static","Static","Empty"],["replace-computed","Static","Computed"]].map(([name,original_name,members_from]) => {
    const original = classes.get(original_name);
    const updated = ts.factory.updateClassExpression(original,original.modifiers,original.name,original.typeParameters,
      original.heritageClauses,classes.get(members_from).members);
    return {case_id:extension+"/"+name,file_name,text,original_name,members_from,
      expected:{transform_flags:updated.transformFlags}};
  });
}
const cases = [];
for (const extension of ["js","ts"]) {
  const first = observe(extension); assert.deepEqual(observe(extension),first); cases.push(...first);
}
const artifact = {version:1,typescript:ts.version,source_commit:"050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256:sha256(fs.readFileSync(path.join(root,"vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256:sha256(fs.readFileSync(import.meta.filename)),repetitions:2,cases};
const destination = path.join(root,"crates/emitter/tests/fixtures/class-expression-updates.json");
const rendered = JSON.stringify(artifact,null,2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination,rendered);
else assert.equal(fs.readFileSync(destination,"utf8"),rendered);
console.log("Class expression factory updates: 6 transitions, two identical observations each");
