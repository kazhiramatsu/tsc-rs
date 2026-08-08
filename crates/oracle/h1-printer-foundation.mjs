import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h1-printer-foundation.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h1-printer-foundation.v1.json";
const TARGET_PATH = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
const SCHEMA_RELATIVE_PATH =
  ".github/ci/contracts/h1-printer-foundation.schema.json";
const DESIGN_RELATIVE_PATH = "docs/design/greenfield/h1-emit.md";
const TYPESCRIPT_RELATIVE_PATH = "vendor/typescript-6.0.3/lib/typescript.js";
const TSC_RELATIVE_PATH = "vendor/typescript-6.0.3/lib/_tsc.js";

const EXPECTED_NODE_VERSION = "25.2.1";
const EXPECTED_TYPESCRIPT_VERSION = "6.0.3";
const EXPECTED_SOURCE_COMMIT =
  "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_TYPESCRIPT_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const EXPECTED_TSC_SHA256 =
  "1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3";

function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function fileBytes(relative) {
  return fs.readFileSync(path.join(WORKSPACE, relative));
}

function pathHash(relative) {
  return { path: relative, sha256: sha256(fileBytes(relative)) };
}

function exactKeys(value, required) {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    required.every((key) => Object.hasOwn(value, key)) &&
    Object.keys(value).every((key) => required.includes(key))
  );
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function validateRuntime() {
  const nodePin = fileBytes(".node-version").toString("utf8").trim();
  requireCondition(nodePin === EXPECTED_NODE_VERSION, "unexpected .node-version pin");
  requireCondition(
    process.version === `v${nodePin}`,
    `H1 printer oracle requires Node ${nodePin}; running ${process.version}`,
  );
  requireCondition(
    ts.version === EXPECTED_TYPESCRIPT_VERSION,
    `unexpected TypeScript runtime ${ts.version}`,
  );
  requireCondition(
    pathHash(TYPESCRIPT_RELATIVE_PATH).sha256 === EXPECTED_TYPESCRIPT_SHA256,
    "vendored typescript.js differs from the reviewed TypeScript 6.0.3 pin",
  );
  requireCondition(
    pathHash(TSC_RELATIVE_PATH).sha256 === EXPECTED_TSC_SHA256,
    "vendored _tsc.js differs from the reviewed TypeScript 6.0.3 pin",
  );
  for (const [name, value] of [
    ["createTextWriter", ts.createTextWriter],
    ["computeLineStarts", ts.computeLineStarts],
    ["computeLineAndCharacterOfPosition", ts.computeLineAndCharacterOfPosition],
    ["createPrinter", ts.createPrinter],
    ["createSourceFile", ts.createSourceFile],
    ["createScanner", ts.createScanner],
  ]) {
    requireCondition(typeof value === "function", `vendored ${name} export is unavailable`);
  }
}

const writerSpecs = [
  {
    id: "lf-unicode-indent-comment-reset",
    newLine: "lf",
    operations: [
      { kind: "write", text: "A😀e\u0301" },
      { kind: "write", text: "\r\n雪\u2028x" },
      { kind: "write", text: "\u0085" },
      { kind: "writeLine", force: false },
      { kind: "increaseIndent" },
      { kind: "writeKeyword", text: "const" },
      { kind: "writeComment", text: "/*c*/" },
      { kind: "writeSpace", text: " " },
      { kind: "rawWrite", text: "" },
      { kind: "writeLiteral", text: '"\\u{1F600}"' },
      { kind: "writeLine", force: true },
      { kind: "clear" },
    ],
  },
  {
    id: "crlf-unicode-mixed-line-breaks",
    newLine: "crlf",
    operations: [
      { kind: "increaseIndent" },
      { kind: "write", text: "😀" },
      { kind: "writeLine", force: false },
      { kind: "rawWrite", text: "雪\rZ\nQ\u2029R" },
      { kind: "writeLine", force: true },
      { kind: "decreaseIndent" },
      { kind: "writeComment", text: "//e\u0301" },
      { kind: "writeLine", force: false },
    ],
  },
];

function applyWriterOperation(writer, operation) {
  switch (operation.kind) {
    case "write":
      writer.write(operation.text);
      break;
    case "rawWrite":
      writer.rawWrite(operation.text);
      break;
    case "writeLiteral":
      writer.writeLiteral(operation.text);
      break;
    case "writeKeyword":
      writer.writeKeyword(operation.text);
      break;
    case "writeComment":
      writer.writeComment(operation.text);
      break;
    case "writeSpace":
      writer.writeSpace(operation.text);
      break;
    case "writeLine":
      writer.writeLine(operation.force);
      break;
    case "increaseIndent":
      writer.increaseIndent();
      break;
    case "decreaseIndent":
      writer.decreaseIndent();
      break;
    case "clear":
      writer.clear();
      break;
    default:
      throw new Error(`unknown writer operation ${operation.kind}`);
  }
}

function writerState(writer) {
  const text = writer.getText();
  return {
    text,
    text_utf8_bytes: Buffer.byteLength(text, "utf8"),
    text_utf8_sha256: sha256(Buffer.from(text, "utf8")),
    text_position_utf16: writer.getTextPos(),
    line: writer.getLine(),
    column_utf16: writer.getColumn(),
    indent: writer.getIndent(),
    at_start_of_line: writer.isAtStartOfLine(),
    has_trailing_comment: writer.hasTrailingComment(),
    has_trailing_whitespace: writer.hasTrailingWhitespace(),
  };
}

function writerCases() {
  return writerSpecs.map((spec) => {
    const newLine =
      spec.newLine === "lf"
        ? ts.NewLineKind.LineFeed
        : ts.NewLineKind.CarriageReturnLineFeed;
    const writer = ts.createTextWriter(newLine === 1 ? "\n" : "\r\n");
    return {
      id: spec.id,
      new_line: spec.newLine,
      new_line_text: newLine === 1 ? "\n" : "\r\n",
      steps: spec.operations.map((operation, index) => {
        applyWriterOperation(writer, operation);
        return { index, operation, state: writerState(writer) };
      }),
    };
  });
}

function isScalarBoundary(text, position) {
  if (position <= 0 || position >= text.length) return true;
  const previous = text.charCodeAt(position - 1);
  const current = text.charCodeAt(position);
  return !(previous >= 0xd800 && previous <= 0xdbff && current >= 0xdc00 && current <= 0xdfff);
}

function utf8ByteOffset(text, utf16Position) {
  requireCondition(
    Number.isInteger(utf16Position) && utf16Position >= 0 && utf16Position <= text.length,
    `invalid UTF-16 position ${utf16Position}`,
  );
  requireCondition(
    isScalarBoundary(text, utf16Position),
    `position ${utf16Position} splits a surrogate pair`,
  );
  return Buffer.byteLength(text.slice(0, utf16Position), "utf8");
}

const escapedSource =
  'const \\u0061 = "\\u{1F600}";\nconst café = "😀";\n';

const sourcePositionSpecs = [
  {
    id: "astral-combining-crlf-ls-ps",
    text: "a😀\r\ne\u0301\u2028雪\u2029z",
    positions: [
      ["start", 0],
      ["after-a", 1],
      ["after-astral", 3],
      ["after-cr", 4],
      ["after-crlf", 5],
      ["after-combining-base", 6],
      ["after-combining-mark", 7],
      ["after-ls", 8],
      ["after-snow", 9],
      ["after-ps", 10],
      ["end", 11],
    ],
  },
  {
    id: "nel-control-lf-cr",
    text: "N\u0085Q\nR\rS",
    positions: [
      ["start", 0],
      ["after-n", 1],
      ["after-nel", 2],
      ["after-q", 3],
      ["after-lf", 4],
      ["after-r", 5],
      ["after-cr", 6],
      ["end", 7],
    ],
  },
  {
    id: "escaped-and-unescaped-identifiers-literals",
    text: escapedSource,
    positions: [
      ["escaped-identifier-start", escapedSource.indexOf("\\u0061")],
      ["escaped-identifier-end", escapedSource.indexOf("\\u0061") + 6],
      ["escaped-literal-start", escapedSource.indexOf('"\\u{1F600}"')],
      [
        "escaped-literal-end",
        escapedSource.indexOf('"\\u{1F600}"') + '"\\u{1F600}"'.length,
      ],
      ["unescaped-identifier-start", escapedSource.indexOf("café")],
      ["unescaped-identifier-end", escapedSource.indexOf("café") + "café".length],
      ["astral-literal-start", escapedSource.indexOf('"😀"') + 1],
      ["astral-literal-end", escapedSource.indexOf('"😀"') + 3],
      ["end", escapedSource.length],
    ],
  },
];

function positionObservation(text, label, utf16Position) {
  const lineStarts = ts.computeLineStarts(text);
  const location = ts.computeLineAndCharacterOfPosition(lineStarts, utf16Position);
  return {
    label,
    source_byte_position: utf8ByteOffset(text, utf16Position),
    source_utf16_position: utf16Position,
    line: location.line,
    column_utf16: location.character,
  };
}

function sourcePositionCases() {
  return sourcePositionSpecs.map((spec) => ({
    id: spec.id,
    text: spec.text,
    text_utf8_bytes: Buffer.byteLength(spec.text, "utf8"),
    text_utf16_units: spec.text.length,
    positions: spec.positions.map(([label, position]) =>
      positionObservation(spec.text, label, position),
    ),
  }));
}

const printerSpecs = [
  {
    id: "lf-comments-escapes-astral-combining",
    fileName: "unicode.js",
    newLine: "lf",
    source:
      '// lead 😀\nconst \\u0061 = "\\u{1F600}";\nconst café = "😀e\u0301";\nconst high = "\\uD800";\nconst low = "\\uDC00";\nconst escapedBackslash = "\\\\uD800";\n',
  },
  {
    id: "crlf-astral-combining",
    fileName: "unicode-crlf.js",
    newLine: "crlf",
    source: 'const astral = "😀";\r\nconst combining = "e\u0301";\r\n',
  },
];

function tokenRows(source) {
  const scanner = ts.createScanner(
    ts.ScriptTarget.ESNext,
    true,
    ts.LanguageVariant.Standard,
    source,
  );
  const rows = [];
  for (;;) {
    const kind = scanner.scan();
    if (kind === ts.SyntaxKind.EndOfFileToken) break;
    const start = scanner.getTokenPos();
    const end = scanner.getTextPos();
    const value = scanner.getTokenValue();
    rows.push({
      index: rows.length,
      kind,
      kind_name: ts.SyntaxKind[kind],
      text: source.slice(start, end),
      value_utf16_units:
        value === undefined
          ? null
          : Array.from({ length: value.length }, (_, index) => value.charCodeAt(index)),
      start: positionObservation(source, "start", start),
      end: positionObservation(source, "end", end),
    });
  }
  return rows;
}

function printerCases() {
  return printerSpecs.map((spec) => {
    const newLine =
      spec.newLine === "lf"
        ? ts.NewLineKind.LineFeed
        : ts.NewLineKind.CarriageReturnLineFeed;
    const sourceFile = ts.createSourceFile(
      spec.fileName,
      spec.source,
      ts.ScriptTarget.ESNext,
      true,
      ts.ScriptKind.JS,
    );
    requireCondition(
      sourceFile.parseDiagnostics.length === 0,
      `${spec.id} unexpectedly has parse diagnostics`,
    );
    const output = ts
      .createPrinter({ newLine, removeComments: false, noEmitHelpers: false })
      .printFile(sourceFile);
    requireCondition(
      output === spec.source,
      `${spec.id} must remain on H1.2's proven whole-source raw-copy route`,
    );
    const end = positionObservation(output, "end", output.length);
    return {
      id: spec.id,
      file_name: spec.fileName,
      new_line: spec.newLine,
      source: spec.source,
      source_utf8_sha256: sha256(Buffer.from(spec.source, "utf8")),
      output,
      output_utf8_sha256: sha256(Buffer.from(output, "utf8")),
      output_utf16_units: output.length,
      output_end: {
        position_utf16: output.length,
        line: end.line,
        column_utf16: end.column_utf16,
      },
      tokens: tokenRows(spec.source),
    };
  });
}

function validateArtifact(value) {
  requireCondition(
    exactKeys(value, [
      "schema",
      "status",
      "phase",
      "typescript",
      "generator",
      "contract",
      "authorities",
      "summary",
      "writer_cases",
      "source_position_cases",
      "printer_cases",
      "oracle_fingerprint_sha256",
    ]),
    "H1 printer artifact has missing or unknown top-level fields",
  );
  requireCondition(
    value.schema === 1 &&
      value.status === "frozen-h1.2-printer-foundation" &&
      value.phase === "H1.2",
    "invalid H1 printer artifact header",
  );
  requireCondition(
    value.writer_cases.length === 2 &&
      value.source_position_cases.length === 3 &&
      value.printer_cases.length === 2,
    "H1 printer oracle case set drifted",
  );
  requireCondition(
    value.summary.writer_steps === 20 &&
      value.summary.source_positions === 28 &&
      value.summary.printer_tokens ===
        value.printer_cases.reduce((total, entry) => total + entry.tokens.length, 0),
    "H1 printer oracle summary drifted",
  );
  const semantic = { ...value };
  delete semantic.oracle_fingerprint_sha256;
  requireCondition(
    value.oracle_fingerprint_sha256 === sha256(canonical(semantic)),
    "H1 printer oracle fingerprint mismatch",
  );
}

validateRuntime();
const writers = writerCases();
const sources = sourcePositionCases();
const printers = printerCases();
const losslessTokens = new Map(
  printers[0].tokens.map((token) => [token.text, token.value_utf16_units]),
);
requireCondition(
  canonical(losslessTokens.get('"\\u{1F600}"')) === canonical([0xd83d, 0xde00]),
  "paired astral escape lost its UTF-16 code units",
);
requireCondition(
  canonical(losslessTokens.get('"\\uD800"')) === canonical([0xd800]),
  "lone high surrogate escape was replaced",
);
requireCondition(
  canonical(losslessTokens.get('"\\uDC00"')) === canonical([0xdc00]),
  "lone low surrogate escape was replaced",
);
requireCondition(
  canonical(losslessTokens.get('"\\\\uD800"')) ===
    canonical([0x5c, 0x75, 0x44, 0x38, 0x30, 0x30]),
  "escaped backslash was decoded as a surrogate escape",
);
const artifact = {
  schema: 1,
  status: "frozen-h1.2-printer-foundation",
  phase: "H1.2",
  typescript: {
    version: EXPECTED_TYPESCRIPT_VERSION,
    source_commit: EXPECTED_SOURCE_COMMIT,
  },
  generator: pathHash(GENERATOR_RELATIVE_PATH),
  contract: pathHash(SCHEMA_RELATIVE_PATH),
  authorities: {
    design: pathHash(DESIGN_RELATIVE_PATH),
    typescript: pathHash(TYPESCRIPT_RELATIVE_PATH),
    tsc: pathHash(TSC_RELATIVE_PATH),
  },
  summary: {
    writer_cases: writers.length,
    writer_steps: writers.reduce((total, entry) => total + entry.steps.length, 0),
    source_position_cases: sources.length,
    source_positions: sources.reduce(
      (total, entry) => total + entry.positions.length,
      0,
    ),
    printer_cases: printers.length,
    printer_tokens: printers.reduce((total, entry) => total + entry.tokens.length, 0),
  },
  writer_cases: writers,
  source_position_cases: sources,
  printer_cases: printers,
};
artifact.oracle_fingerprint_sha256 = sha256(canonical(artifact));
validateArtifact(artifact);

const rendered = `${JSON.stringify(artifact, null, 2)}\n`;
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(TARGET_PATH, rendered);
  process.stdout.write(`wrote ${TARGET_RELATIVE_PATH}\n`);
} else if (mode === "--check") {
  requireCondition(fs.existsSync(TARGET_PATH), `missing ${TARGET_RELATIVE_PATH}`);
  requireCondition(
    fs.readFileSync(TARGET_PATH, "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run with --write and review`,
  );
  process.stdout.write(
    `H1 printer foundation oracle is fresh: writer_steps=${artifact.summary.writer_steps} source_positions=${artifact.summary.source_positions} printer_tokens=${artifact.summary.printer_tokens}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  throw new Error("usage: h1-printer-foundation.mjs [--write|--check]");
}
