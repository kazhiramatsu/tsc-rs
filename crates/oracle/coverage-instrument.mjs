import fs from "node:fs";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const [bundlePath, inventoryPath, outputPath] = process.argv.slice(2);
if (!bundlePath || !inventoryPath || !outputPath) {
  throw new Error(
    "usage: coverage-instrument.mjs <_tsc.js> <m8-emitter-inventory.json> <output.cjs>",
  );
}

const sourceText = fs.readFileSync(bundlePath, "utf8");
const inventory = JSON.parse(fs.readFileSync(inventoryPath, "utf8"));
if (inventory.schema !== 2 || inventory.band !== "all") {
  throw new Error("runtime coverage requires the schema-2 all-band D2 inventory");
}

const source = ts.createSourceFile(bundlePath, sourceText, ts.ScriptTarget.Latest, true);
const expected = inventory.functions.filter(
  (declaration) => declaration.direct_emitter,
);
const direct = new Map(
  expected.map((declaration, index) => [
    declaration.source_range.start.offset,
    { declaration, index },
  ]),
);
const edits = [];
const seen = new Set();

function hitExpression(index) {
  return `__tsrsM8Hits[${index}]=1`;
}

function visit(node) {
  const start = node.getStart(source);
  const match = direct.get(start);
  if (match && match.declaration.kind !== "SourceFile") {
    const { declaration, index } = match;
    const body = node.body;
    if (!body) {
      throw new Error(`direct emitter ${declaration.id} has no executable body`);
    }
    if (ts.isBlock(body)) {
      edits.push({
        start: body.getStart(source) + 1,
        end: body.getStart(source) + 1,
        text: `${hitExpression(index)};`,
      });
    } else if (ts.isArrowFunction(node)) {
      edits.push({
        start: body.getStart(source),
        end: body.getStart(source),
        text: `(${hitExpression(index)},`,
      });
      edits.push({
        start: body.end,
        end: body.end,
        text: ")",
      });
    } else {
      throw new Error(
        `direct emitter ${declaration.id} has unsupported non-block body ${ts.SyntaxKind[body.kind]}`,
      );
    }
    seen.add(declaration.id);
  }
  ts.forEachChild(node, visit);
}
visit(source);

const top = expected
  .map((declaration, index) => ({ declaration, index }))
  .filter(({ declaration }) => declaration.kind === "SourceFile");
if (top.length !== 1) {
  throw new Error(`expected one direct <top> declaration, found ${top.length}`);
}
seen.add(top[0].declaration.id);

if (seen.size !== expected.length) {
  const missing = expected.filter((declaration) => !seen.has(declaration.id));
  throw new Error(
    `instrumentation resolved ${seen.size}/${expected.length} direct emitters; first missing ${missing[0]?.id}`,
  );
}

const counterPrelude = `
var __tsrsM8HitIds = ${JSON.stringify(expected.map((declaration) => declaration.id))};
var __tsrsM8Hits = new Uint8Array(__tsrsM8HitIds.length);
`;
const strictMarker = '"use strict";';
const strictOffset = sourceText.indexOf(strictMarker);
if (strictOffset < 0) {
  throw new Error("instrumentation could not find the _tsc.js strict-mode marker");
}
edits.push({
  start: strictOffset + strictMarker.length,
  end: strictOffset + strictMarker.length,
  text: `${counterPrelude}${hitExpression(top[0].index)};`,
});

const cliTail = "executeCommandLine(sys, noop, sys.args);";
const tailOffset = sourceText.lastIndexOf(cliTail);
if (tailOffset < 0) {
  throw new Error("instrumentation could not find the _tsc.js CLI entrypoint");
}
edits.push({
  start: tailOffset,
  end: tailOffset + cliTail.length,
  text: `module.exports = {
  optionDeclarations,
  ScriptTarget: { Latest: 99 },
  createSourceFile,
  createProgram,
  sortAndDeduplicateDiagnostics,
  getKeyForCompilerOptions,
  sourceFileAffectingCompilerOptions,
  __tsrsM8HitIds,
  __tsrsM8Hits
};`,
});

edits.sort((left, right) => right.start - left.start || right.end - left.end);
let instrumented = sourceText;
let lastStart = sourceText.length + 1;
for (const edit of edits) {
  if (edit.end > lastStart) {
    throw new Error(`overlapping instrumentation edit at ${edit.start}`);
  }
  instrumented =
    instrumented.slice(0, edit.start) + edit.text + instrumented.slice(edit.end);
  lastStart = edit.start;
}
fs.writeFileSync(outputPath, instrumented);
process.stdout.write(
  `${JSON.stringify({
    schema: 1,
    instrumented: expected.length,
    output: outputPath,
  })}\n`,
);
