import crypto from "node:crypto";
import fs from "node:fs";

const [bundlePath, inventoryPath, codesPath, outputPath] = process.argv.slice(2);
if (!bundlePath || !inventoryPath || !codesPath || !outputPath) {
  throw new Error(
    "usage: trace-instrument.mjs <_tsc.js> <m8-emitter-inventory.json> <codes.json> <output.cjs>",
  );
}

const sourceText = fs.readFileSync(bundlePath, "utf8");
const inventory = JSON.parse(fs.readFileSync(inventoryPath, "utf8"));
const codes = JSON.parse(fs.readFileSync(codesPath, "utf8"));
if (inventory.schema !== 2 || inventory.band !== "all") {
  throw new Error("diagnostic trace requires the schema-2 all-band D2 inventory");
}
if (
  !Array.isArray(codes) ||
  codes.length === 0 ||
  codes.some((code) => !Number.isSafeInteger(code) || code < 0)
) {
  throw new Error("diagnostic trace codes must be a non-empty array of integers");
}
const normalizedCodes = [...new Set(codes)].sort((left, right) => left - right);
if (JSON.stringify(codes) !== JSON.stringify(normalizedCodes)) {
  throw new Error("diagnostic trace codes must be sorted and unique");
}

const selectedCodes = new Set(normalizedCodes);
const sites = inventory.functions
  .flatMap((declaration) =>
    (declaration.sites ?? [])
      .filter((site) => selectedCodes.has(site.code))
      .map((site) => ({
        id: site.id,
        declaration: declaration.id,
        code: site.code,
        name: site.name,
        line: site.line,
        character: site.character,
        offset: site.offset,
      })),
  )
  .sort((left, right) => left.offset - right.offset || left.id.localeCompare(right.id));
if (sites.length === 0) {
  throw new Error(
    `no D2 diagnostic reference site carries requested codes ${normalizedCodes.join(",")}`,
  );
}

const edits = [];
for (const [index, site] of sites.entries()) {
  const expression = `Diagnostics.${site.name}`;
  if (sourceText.slice(site.offset, site.offset + expression.length) !== expression) {
    throw new Error(
      `stale D2 diagnostic site ${site.id}: expected ${expression} at ${site.offset}`,
    );
  }
  edits.push({
    start: site.offset,
    end: site.offset + expression.length,
    text: `(__tsrsM8TraceReference(${index}),${expression})`,
    kind: "diagnostic-reference",
  });
}

const cliTail = "executeCommandLine(sys, noop, sys.args);";
const tailOffset = sourceText.lastIndexOf(cliTail);
if (tailOffset < 0) {
  throw new Error("diagnostic trace could not find the _tsc.js CLI entrypoint");
}
const siteMetadata = sites;
const traceTail = `
var __tsrsM8TraceSites=${JSON.stringify(siteMetadata)};
var __tsrsM8TraceEvents=[];
var __tsrsM8TracePass=null;
function __tsrsM8TraceReference(index){
  var previousPrepare=Error.prepareStackTrace;
  var previousLimit=Error.stackTraceLimit;
  var holder={};
  try {
    Error.stackTraceLimit=64;
    Error.prepareStackTrace=function(_error,frames){return frames;};
    Error.captureStackTrace(holder,__tsrsM8TraceReference);
    var frames=holder.stack.map(function(frame){
      return {
        function_name:frame.getFunctionName()||frame.getMethodName()||null,
        file:frame.getFileName()||null,
        line:frame.getLineNumber()||null,
        column:frame.getColumnNumber()||null
      };
    });
    __tsrsM8TraceEvents.push({
      site:__tsrsM8TraceSites[index],
      pass:__tsrsM8TracePass,
      frames:frames
    });
  } finally {
    Error.prepareStackTrace=previousPrepare;
    Error.stackTraceLimit=previousLimit;
  }
}
function __tsrsM8ResetTrace(){__tsrsM8TraceEvents.length=0;__tsrsM8TracePass=null;}
function __tsrsM8SetTracePass(pass){__tsrsM8TracePass=pass;}
function __tsrsM8TakeTrace(){var events=__tsrsM8TraceEvents.slice();__tsrsM8TraceEvents.length=0;return events;}
module.exports={
  optionDeclarations,
  ScriptTarget:{Latest:99},
  createSourceFile,
  createProgram,
  sortAndDeduplicateDiagnostics,
  getKeyForCompilerOptions,
  sourceFileAffectingCompilerOptions,
  __tsrsM8TraceSites,
  __tsrsM8ResetTrace,
  __tsrsM8SetTracePass,
  __tsrsM8TakeTrace
};`;
edits.push({
  start: tailOffset,
  end: tailOffset + cliTail.length,
  text: traceTail,
  kind: "cli-export",
});

const ascendingEdits = edits
  .slice()
  .sort((left, right) => left.start - right.start || left.end - right.end);
let generatedDelta = 0;
const offsetMap = ascendingEdits.map((edit) => {
  const generatedStart = edit.start + generatedDelta;
  const generatedEnd = generatedStart + edit.text.length;
  const mapping = {
    kind: edit.kind,
    original_start: edit.start,
    original_end: edit.end,
    generated_start: generatedStart,
    generated_end: generatedEnd,
  };
  generatedDelta += edit.text.length - (edit.end - edit.start);
  return mapping;
});

edits.sort((left, right) => right.start - left.start || right.end - left.end);
let instrumented = sourceText;
let lastStart = sourceText.length + 1;
for (const edit of edits) {
  if (edit.end > lastStart) {
    throw new Error(`overlapping diagnostic trace edit at ${edit.start}`);
  }
  instrumented =
    instrumented.slice(0, edit.start) + edit.text + instrumented.slice(edit.end);
  lastStart = edit.start;
}
fs.writeFileSync(outputPath, instrumented);

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

process.stdout.write(
  `${JSON.stringify({
    schema: 1,
    strategy: "exact-d2-site-offsets/no-ast-visit",
    source_sha256: sha256(sourceText),
    output_sha256: sha256(instrumented),
    codes: normalizedCodes,
    selected_sites: sites.length,
    source_declarations_visited: 0,
    offset_map: offsetMap,
  })}\n`,
);
