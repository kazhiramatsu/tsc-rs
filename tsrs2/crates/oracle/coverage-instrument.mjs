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
const direct = new Map(
  inventory.functions
    .filter((declaration) => declaration.direct_emitter)
    .map((declaration) => [declaration.source_range.start.offset, declaration]),
);
const edits = [];
const seen = new Set();

function hitExpression(id) {
  return `__tsrsM8Hit(${JSON.stringify(id)})`;
}

function visit(node) {
  const start = node.getStart(source);
  const declaration = direct.get(start);
  if (declaration && declaration.kind !== "SourceFile") {
    const body = node.body;
    if (!body) {
      throw new Error(`direct emitter ${declaration.id} has no executable body`);
    }
    if (ts.isBlock(body)) {
      edits.push({
        start: body.getStart(source) + 1,
        end: body.getStart(source) + 1,
        text: `${hitExpression(declaration.id)};`,
      });
    } else if (ts.isArrowFunction(node)) {
      edits.push({
        start: body.getStart(source),
        end: body.getStart(source),
        text: `(${hitExpression(declaration.id)},`,
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

const top = inventory.functions.filter(
  (declaration) => declaration.direct_emitter && declaration.kind === "SourceFile",
);
if (top.length !== 1) {
  throw new Error(`expected one direct <top> declaration, found ${top.length}`);
}
seen.add(top[0].id);

const expected = inventory.functions.filter((declaration) => declaration.direct_emitter);
if (seen.size !== expected.length) {
  const missing = expected.filter((declaration) => !seen.has(declaration.id));
  throw new Error(
    `instrumentation resolved ${seen.size}/${expected.length} direct emitters; first missing ${missing[0]?.id}`,
  );
}

const counterPrelude = `
var __tsrsM8Counts = Object.create(null);
function __tsrsM8Hit(id) {
  __tsrsM8Counts[id] = (__tsrsM8Counts[id] || 0) + 1;
}
`;
const strictMarker = '"use strict";';
const strictOffset = sourceText.indexOf(strictMarker);
if (strictOffset < 0) {
  throw new Error("instrumentation could not find the _tsc.js strict-mode marker");
}
edits.push({
  start: strictOffset + strictMarker.length,
  end: strictOffset + strictMarker.length,
  text: `${counterPrelude}${hitExpression(top[0].id)};`,
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
  __tsrsM8Counts
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
