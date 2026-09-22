// Printer-phase controls for H2.8a A6-11. Expected bytes come only from tsc.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const destination = new URL("../crates/emitter/tests/fixtures/list-comment-flags.json", import.meta.url);
const sources = [
  ["call-inline", "f(/* FIRST */ x /* AFTER */, /* SECOND */ y);\n"],
  ["call-newline", "f(\n/* FIRST */ x /* AFTER */,\n/* SECOND */ y);\n"],
  ["array-inline", "[/* FIRST */ x /* AFTER */, /* SECOND */ y];\n"],
  ["array-newline", "[\n/* FIRST */ x /* AFTER */,\n/* SECOND */ y];\n"],
  ["variable-newline", "let x\n/* NEXT */ = 1;\n"],
  ["variable-inline", "let x /* TAIL */ = 1;\n"],
  ["variable-mixed", "let x /* A */\n/* B */ = 1;\n"],
  ["variable-line", "let x // A\n/* B */ = 1;\n"],
  ["variable-list", "let x /* A */ = 1, y\n/* B */ = 2;\n"],
  ["variable-jsdoc", "let x /** A */\n/** B */ = 1;\n"],
  ["variable-erased-type", "let x /* NAME */: number /* TYPE */\n/* NEXT */ = 1;\n"],
  ["variable-empty-type", "let x /* NAME */: /* TYPE */\n/* NEXT */ = 1;\n"],
];
const variants = ["None", "NoLeadingComments", "NoTrailingComments", "NoComments", "NoNestedComments", "ParentNoNestedComments"];
function observe(source, variant, removeComments, bodyRange) {
  let file = ts.createSourceFile("list-comments.ts", source, ts.ScriptTarget.Latest, true);
  let statement = file.statements[0];
  if (ts.isFunctionDeclaration(statement)) {
    if (bodyRange) {
      const original = statement.body.statements;
      const statements = ts.factory.createNodeArray([...original]);
      ts.setTextRange(statements, {
        pos: bodyRange === "EndOnly" || bodyRange === "Synthesized" ? -1 : original.pos,
        end: bodyRange === "StartOnly" || bodyRange === "Synthesized" ? -1 : original.end,
      });
      const body = ts.factory.updateBlock(statement.body, statements);
      statement = ts.factory.updateFunctionDeclaration(statement, statement.modifiers,
        statement.asteriskToken, statement.name, statement.typeParameters,
        statement.parameters, statement.type, body);
      file = ts.factory.updateSourceFile(file, [statement, ...file.statements.slice(1)]);
    }
    if (variant === "ParentNoNestedComments" || variant === "ParentAndBodyNoNestedComments")
      ts.setEmitFlags(statement, ts.EmitFlags.NoNestedComments);
    if (variant !== "ParentNoNestedComments")
      ts.setEmitFlags(statement.body, variant === "ParentAndBodyNoNestedComments"
        ? ts.EmitFlags.NoNestedComments : ts.EmitFlags[variant]);
    return ts.createPrinter({newLine:ts.NewLineKind.LineFeed, removeComments}).printFile(file);
  }
  const parent = statement.expression ?? statement.declarationList;
  if (parent.declarations) for (const declaration of parent.declarations) {
    if (declaration.type) {
      ts.setTypeNode(declaration.name, declaration.type);
      declaration.type = undefined;
    }
  }
  if (variant === "CloneName") {
    const declarations = parent.declarations.map(d => ts.factory.updateVariableDeclaration(d, ts.factory.cloneNode(d.name), d.exclamationToken, d.type, d.initializer));
    const list = ts.factory.updateVariableDeclarationList(parent, declarations);
    file = ts.factory.updateSourceFile(file, [ts.factory.updateVariableStatement(statement, statement.modifiers, list)]);
  } else if (variant === "ParentNoNestedComments") ts.setEmitFlags(parent, ts.EmitFlags.NoNestedComments);
  else for (const child of parent.arguments ?? parent.elements ?? parent.declarations.map(d => d.name)) ts.setEmitFlags(child, ts.EmitFlags[variant]);
  return ts.createPrinter({newLine:ts.NewLineKind.LineFeed, removeComments}).printFile(file);
}
const cases = [];
for (const [shape, source] of sources) for (const variant of variants) for (const remove_comments of [false, true]) {
  const output = observe(source, variant, remove_comments);
  assert.equal(observe(source, variant, remove_comments), output);
  cases.push({case_id:`${shape}/${variant}/${remove_comments ? "removed" : "retained"}`, source, variant, remove_comments, output});
}
for (const [shape, source] of sources.filter(([shape]) => ["variable-newline", "variable-inline", "variable-mixed"].includes(shape))) {
  const variant = "CloneName", remove_comments = false;
  const output = observe(source, variant, remove_comments);
  assert.equal(observe(source, variant, remove_comments), output);
  cases.push({case_id:`${shape}/${variant}/retained`, source, variant, remove_comments, output});
}
const bodies = [
  ["function-body", "function f() {\n// head\n\nx(/* inner */);\n// tail\n}\n"],
  ["empty-function-body", "function f() {\n// head\n\n// tail\n}\n"],
];
for (const [shape, source] of bodies) for (const variant of variants) for (const remove_comments of [false, true]) {
  const output = observe(source, variant, remove_comments);
  assert.equal(observe(source, variant, remove_comments), output);
  cases.push({case_id:`${shape}/${variant}/${remove_comments ? "removed" : "retained"}`, source, variant, remove_comments, output});
}
// Both extents must leave the parent's suppression active until its own exit.
{
  const [shape, source] = bodies[0], variant = "ParentAndBodyNoNestedComments", remove_comments = false;
  const output = observe(source, variant, remove_comments);
  assert.equal(observe(source, variant, remove_comments), output);
  cases.push({case_id:`${shape}/${variant}/retained`, source, variant, remove_comments, output});
}
// Factory-only endpoint controls: EndOnly mirrors dotted namespace arrays;
// StartOnly and Synthesized exercise the public factory/printer boundary without
// claiming that ordinary command transforms currently produce StartOnly bodies.
for (const [shape, source] of bodies) for (const body_range of ["StartOnly", "EndOnly", "Synthesized"])
  for (const variant of variants) for (const remove_comments of [false, true]) {
    const output = observe(source, variant, remove_comments, body_range);
    assert.equal(observe(source, variant, remove_comments, body_range), output);
    cases.push({case_id:`${shape}/${body_range}/${variant}/${remove_comments ? "removed" : "retained"}`,
      source, variant, body_range, remove_comments, output});
  }
assert.equal(cases.length, 244);
const artifact = {version:1, typescript:ts.version, repetitions:2,
  compiler_sha256:sha256(fs.readFileSync(new URL("../vendor/typescript-6.0.3/lib/typescript.js", import.meta.url))),
  observer_sha256:sha256(fs.readFileSync(import.meta.filename)), cases};
const bytes = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, bytes);
else assert.equal(fs.readFileSync(destination, "utf8"), bytes);
console.log(`List comment flags: ${cases.length} printer observations, each identical twice`);
