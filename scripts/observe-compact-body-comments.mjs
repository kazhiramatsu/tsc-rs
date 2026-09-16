// A-PC1: list-owned comments after synthetic and retained function statements.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const destination = new URL("../crates/emitter/tests/fixtures/compact-body-comments.json", import.meta.url);
const sources = [
  ["first", "function f() { /* body */ return x; }\n"],
  ["middle", "function f() { a(); /* body */ return x; }\n"],
  ["nonbmp", 'function f() { use("😀"); /* body */ return x; }\n'],
];
const prefixes = ["none", "synthetic", "comment-range", "break-comment-range"];
const variants = ["None", "TargetNoLeading", "PreviousNoTrailing", "TargetNoComments", "BodyNoNested"];
function observe(input) {
  const file = ts.createSourceFile("compact.ts", input.source, ts.ScriptTarget.Latest, true);
  let fn = file.statements[0];
  let body = fn.body;
  const target = body.statements.at(-1);
  if (input.variant === "TargetNoLeading") ts.setEmitFlags(target, ts.EmitFlags.NoLeadingComments);
  if (input.variant === "TargetNoComments") ts.setEmitFlags(target, ts.EmitFlags.NoComments);
  let statements = [...body.statements];
  if (input.prefix !== "none") {
    const prefix = [0, 1].map(i => ts.factory.createExpressionStatement(ts.factory.createIdentifier(`prefix${i}`)));
    if (input.prefix.endsWith("comment-range")) {
      assert.equal(prefix[1].pos, -1);
      ts.setCommentRange(prefix[1], {pos:target.pos, end:target.end});
      ts.setEmitFlags(prefix[1], ts.EmitFlags.NoComments);
      if (input.prefix === "break-comment-range") ts.setStartsOnNewLine(prefix[1], true);
    }
    statements.unshift(...prefix);
  }
  const previous = statements[statements.indexOf(target) - 1];
  if (input.variant === "PreviousNoTrailing" && previous) ts.setEmitFlags(previous, ts.getEmitFlags(previous) | ts.EmitFlags.NoTrailingComments);
  if (input.prefix !== "none") {
    const array = ts.setTextRange(ts.factory.createNodeArray(statements), body.statements);
    body = ts.factory.updateBlock(body, array);
    fn = ts.factory.updateFunctionDeclaration(fn, fn.modifiers, fn.asteriskToken, fn.name, fn.typeParameters, fn.parameters, fn.type, body);
  }
  if (input.variant === "BodyNoNested") ts.setEmitFlags(body, ts.getEmitFlags(body) | ts.EmitFlags.NoNestedComments);
  if (input.prefix === "break-comment-range") ts.setEmitFlags(body, ts.getEmitFlags(body) | ts.EmitFlags.SingleLine);
  const text = ts.createPrinter({newLine:ts.NewLineKind[input.newline], removeComments:input.remove_comments}).printNode(ts.EmitHint.Unspecified, fn, file);
  return {text, utf8: [...Buffer.from(text)], utf16_length:text.length};
}
const cases = [];
for (const [shape, source] of sources) for (const prefix of prefixes) for (const variant of variants)
  for (const newline of ["LineFeed", "CarriageReturnLineFeed"]) for (const remove_comments of [false, true]) {
    const input = {source:source.replaceAll("\n", newline === "LineFeed" ? "\n" : "\r\n"), prefix, variant, newline, remove_comments};
    const output = observe(input);
    assert.deepEqual(observe(input), output);
    cases.push({case_id:`${shape}/${prefix}/${variant}/${newline}/${remove_comments ? "removed" : "retained"}`, ...input, output});
  }
assert.equal(cases.length, 240);
const artifact = {version:1, typescript:ts.version, repetitions:2,
  compiler_sha256:sha256(fs.readFileSync(new URL("../vendor/typescript-6.0.3/lib/typescript.js", import.meta.url))),
  observer_sha256:sha256(fs.readFileSync(import.meta.filename)), cases};
const bytes = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, bytes);
else assert.equal(fs.readFileSync(destination, "utf8"), bytes);
console.log(`Compact body comments: ${cases.length} printer observations, each identical twice`);
