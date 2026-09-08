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
];
const variants = ["None", "NoLeadingComments", "NoTrailingComments", "NoComments", "NoNestedComments", "ParentNoNestedComments"];
function observe(source, variant, removeComments) {
  const file = ts.createSourceFile("list-comments.ts", source, ts.ScriptTarget.Latest, true);
  const parent = file.statements[0].expression;
  if (variant === "ParentNoNestedComments") ts.setEmitFlags(parent, ts.EmitFlags.NoNestedComments);
  else for (const child of parent.arguments ?? parent.elements) ts.setEmitFlags(child, ts.EmitFlags[variant]);
  return ts.createPrinter({newLine:ts.NewLineKind.LineFeed, removeComments}).printFile(file);
}
const cases = [];
for (const [shape, source] of sources) for (const variant of variants) for (const remove_comments of [false, true]) {
  const output = observe(source, variant, remove_comments);
  assert.equal(observe(source, variant, remove_comments), output);
  cases.push({case_id:`${shape}/${variant}/${remove_comments ? "removed" : "retained"}`, source, variant, remove_comments, output});
}
const artifact = {version:1, typescript:ts.version, repetitions:2,
  compiler_sha256:sha256(fs.readFileSync(new URL("../vendor/typescript-6.0.3/lib/typescript.js", import.meta.url))),
  observer_sha256:sha256(fs.readFileSync(import.meta.filename)), cases};
const bytes = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, bytes);
else assert.equal(fs.readFileSync(destination, "utf8"), bytes);
console.log(`List comment flags: ${cases.length} printer observations, each identical twice`);
