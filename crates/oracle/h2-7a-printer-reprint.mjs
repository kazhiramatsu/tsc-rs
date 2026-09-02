import crypto from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { validateJsonSchemaSubset } from "../../.github/ci/qualification.mjs";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const MODE = process.argv[2];

const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-7a-printer-reprint.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-7a-printer-reprint.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-7a-printer-reprint.schema.json";
const WITNESSES_RELATIVE_PATH = "ratchets/h2-7a-witnesses.v1.json";
const PACKET_RELATIVE_PATH = "docs/design/greenfield/slices/h2-7a-m-3.5.md";
const PARENT_PACKET_RELATIVE_PATH = "docs/design/greenfield/slices/h2-7a.md";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const TYPESCRIPT_LIB_DIRECTORY = "vendor/typescript-6.0.3/lib";
const FIXTURE_ROOT = "ts-tests/tests/cases";
const FIXTURE_ROOTS = Object.freeze([
  "ts-tests/tests/cases/compiler",
  "ts-tests/tests/cases/conformance",
]);
const CHECK_RECEIPT_RELATIVE_PATH =
  "target/h2-7a-printer-reprint/check-receipt.v1.json";
const SELFTEST_RELATIVE_PATH =
  "target/h2-7a-printer-reprint/selftest.v1.json";
const CONTROL_RELATIVE_PATH =
  "target/h2-7a-printer-reprint/observation-control.v1.json";
const CONTROL_FILE_ENV = "TSRS_H2_7A_PRINTER_REPRINT_CONTROL";
const INTERNAL_OBSERVE_MODE = "--internal-observe";
const BATCH_SIZE = 64;
const EXPECTED_NODE = "25.2.1";
const EXPECTED_TYPESCRIPT = "6.0.3";
const PHASE = "H2.7a-printer-reprint";
const ARTIFACT_KIND = "h2-7a-printer-reprint";
const ARTIFACT_STATUS = "qualified-typescript-oracle";
const TOP_LEVEL_KEYS = [
  "schema",
  "kind",
  "status",
  "phase",
  "typescript",
  "generator",
  "contract",
  "inputs",
  "printer_options",
  "strata",
  "rows",
  "excluded",
  "p4_inputs_sha256",
  "summary",
  "reprint_content_roll_sha256",
  "printer_reprint_fingerprint_sha256",
];
const PRINTER_OPTIONS = {
  newLine_default: "lf",
  removeComments_default: false,
  noEmitHelpers: true,
  onlyPrintJsDocStyle: true,
  omitBraceSourceMapPositions: true,
  target_default: null,
};
const STRATA = Object.freeze([
  Object.freeze({
    id: "P1",
    description: "Every declaration write materialized by the frozen W-H2.7A witness observations.",
    selection_contract:
      "One row for every witness observation write whose kind is declaration; decode and sha-verify declaration_materialized_base64.",
  }),
  Object.freeze({
    id: "P2",
    description: "Declaration-file units selected from the compiler and conformance fixture corpus.",
    selection_contract:
      "Walk *.ts and *.tsx fixtures, split on case-insensitive @FileName directives, and retain parse-clean units whose names end in .d.ts.",
  }),
  Object.freeze({
    id: "P3",
    description: "All declaration files in the pinned TypeScript 6.0.3 lib directory.",
    selection_contract:
      "Select every vendor/typescript-6.0.3/lib/*.d.ts file, parse-clean under ScriptKind.TS, with source bytes pinned by path and sha256.",
  }),
  Object.freeze({
    id: "P4",
    description: "Frozen hand-authored declaration-file inputs covering the printer worker obligations.",
    selection_contract:
      "Use the embedded P4_INPUTS roster verbatim; every input must parse cleanly and is observed as <id>.d.ts.",
  }),
]);

function fail(message) {
  throw new Error("h2-7a-printer-reprint: " + message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function stableStringify(value) {
  if (Array.isArray(value)) {
    return "[" + value.map(stableStringify).join(",") + "]";
  }
  if (value !== null && typeof value === "object") {
    return (
      "{" +
      Object.keys(value)
        .sort()
        .map((key) => JSON.stringify(key) + ":" + stableStringify(value[key]))
        .join(",") +
      "}"
    );
  }
  return JSON.stringify(value);
}

function withFingerprint(value, field) {
  return {
    ...value,
    [field]: sha256(Buffer.from(stableStringify(value), "utf8")),
  };
}

function fingerprintIsValid(record, field) {
  if (record === null || typeof record !== "object" || Array.isArray(record)) {
    return false;
  }
  const stored = record[field];
  const payload = { ...record };
  delete payload[field];
  return (
    typeof stored === "string" &&
    stored === sha256(Buffer.from(stableStringify(payload), "utf8"))
  );
}

function render(value) {
  return JSON.stringify(value, null, 2) + "\n";
}

function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function readJson(relativePath) {
  try {
    return JSON.parse(readBytes(relativePath).toString("utf8"));
  } catch (error) {
    fail(relativePath + " is not valid JSON: " + error.message);
  }
}

function pathHash(relativePath) {
  return {
    path: relativePath,
    sha256: sha256(readBytes(relativePath)),
  };
}

function compareBytes(left, right) {
  return Buffer.compare(Buffer.from(left), Buffer.from(right));
}

function isSha256(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function writeFileAtomic(absolutePath, contents) {
  fs.mkdirSync(path.dirname(absolutePath), { recursive: true });
  const temporary = path.join(
    path.dirname(absolutePath),
    "." + path.basename(absolutePath) + ".tmp",
  );
  fs.writeFileSync(temporary, contents);
  fs.renameSync(temporary, absolutePath);
}

function validateRuntime() {
  const node = readBytes(".node-version").toString("utf8").trim();
  requireCondition(node === EXPECTED_NODE, "unexpected Node pin");
  requireCondition(process.version === "v" + node, "requires Node " + node);
  requireCondition(ts.version === EXPECTED_TYPESCRIPT, "unexpected TypeScript runtime");
  for (const name of ["createSourceFile", "createPrinter", "forEachChild"]) {
    requireCondition(
      typeof ts[name] === "function",
      "pinned TypeScript does not expose " + name,
    );
  }
  requireCondition(ts.ScriptKind && ts.ScriptKind.TS !== undefined, "ScriptKind.TS is unavailable");
  requireCondition(
    ts.ScriptTarget && ts.ScriptTarget.Latest !== undefined,
    "ScriptTarget.Latest is unavailable",
  );
}

function assertPathHash(record, relativePath, label) {
  requireCondition(
    record !== null &&
      typeof record === "object" &&
      record.path === relativePath &&
      isSha256(record.sha256) &&
      record.sha256 === pathHash(relativePath).sha256,
    label + " path/hash pin changed",
  );
}

function normalizeFixtureRelativePath(suite, relativePath) {
  requireCondition(
    typeof relativePath === "string" &&
      relativePath.length > 0 &&
      relativePath === path.posix.normalize(relativePath) &&
      !relativePath.startsWith("/") &&
      !relativePath.split("/").includes(".."),
    "unsafe " + suite + " fixture path " + JSON.stringify(relativePath),
  );
  return suite + "/" + relativePath;
}

function fixtureAbsolutePath(fixtureRelativePath) {
  requireCondition(
    typeof fixtureRelativePath === "string" &&
      FIXTURE_ROOTS.some((root) => fixtureRelativePath === root.slice(FIXTURE_ROOT.length + 1) ||
        fixtureRelativePath.startsWith(root.slice(FIXTURE_ROOT.length + 1) + "/")),
    "fixture path is outside the pinned roots: " + fixtureRelativePath,
  );
  return path.join(WORKSPACE, FIXTURE_ROOT, fixtureRelativePath);
}

function decodeUtf8Fixture(bytes, fixtureRelativePath) {
  if (
    (bytes[0] === 0xff && bytes[1] === 0xfe) ||
    (bytes[0] === 0xfe && bytes[1] === 0xff)
  ) {
    return null;
  }
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

function utf8Bytes(text) {
  return Buffer.from(text, "utf8");
}

function sourceFileFor(row, input) {
  const target = row.options.target;
  return ts.createSourceFile(
    row.file_name,
    input,
    target === null ? ts.ScriptTarget.Latest : target,
    true,
    ts.ScriptKind.TS,
  );
}

function diagnosticCodes(sourceFile) {
  return sourceFile.parseDiagnostics.map((diagnostic) => diagnostic.code);
}

function addKindCoverage(sourceFile, counts) {
  const visit = (node) => {
    const name = SYNTAX_KIND_NAMES.get(node.kind);
    requireCondition(
      name !== undefined,
      "unknown SyntaxKind numeric value " + node.kind,
    );
    counts.set(name, (counts.get(name) ?? 0) + 1);
    ts.forEachChild(node, visit);
  };
  ts.forEachChild(sourceFile, visit);
}

const SYNTAX_KIND_NAMES = (() => {
  const names = new Map();
  for (const [name, value] of Object.entries(ts.SyntaxKind)) {
    if (!Number.isInteger(value)) continue;
    const current = names.get(value);
    if (
      current === undefined ||
      (/^(First|Last)/u.test(current) && !/^(First|Last)/u.test(name))
    ) {
      names.set(value, name);
    }
  }
  return names;
})();

function kindCoverageObject(counts) {
  return Object.fromEntries(
    [...counts.entries()].sort(([left], [right]) => compareBytes(left, right)),
  );
}

function parseAndCover(row, input, counts, label) {
  const sourceFile = sourceFileFor(row, input);
  addKindCoverage(sourceFile, counts);
  requireCondition(
    sourceFile.parseDiagnostics.length === 0,
    label + " has parse diagnostics: " + diagnosticCodes(sourceFile).join(","),
  );
  return sourceFile;
}

function optionsFromCase(caseEntry) {
  const record = caseEntry.option_record ?? {};
  let newLine;
  if (record.newLine === undefined) newLine = "lf";
  else if (record.newLine === 0) newLine = "crlf";
  else if (record.newLine === 1) newLine = "lf";
  else fail(caseEntry.case_id + " has an unsupported newLine option");
  const removeComments =
    record.removeComments === undefined ? false : record.removeComments;
  requireCondition(
    typeof removeComments === "boolean",
    caseEntry.case_id + " has a non-boolean removeComments option",
  );
  const target = record.target === undefined ? null : record.target;
  requireCondition(
    target === null || Number.isInteger(target),
    caseEntry.case_id + " has a non-integer target option",
  );
  return { newLine, removeComments, target };
}

function makeSelectionRow({
  id,
  stratum,
  source,
  fileName,
  options,
  input,
  inputBytes,
}) {
  const row = {
    id,
    stratum,
    source,
    file_name: fileName,
    options,
  };
  if (input !== undefined) row.input_utf8 = input;
  row.input_sha256 = sha256(inputBytes);
  row.input_bytes = inputBytes.length;
  return row;
}

function selectP1(witness, counts) {
  requireCondition(
    Array.isArray(witness.case_manifest?.cases) &&
      Array.isArray(witness.observations),
    "witness artifact lacks case manifest or observations",
  );
  const cases = new Map(
    witness.case_manifest.cases.map((entry) => [entry.case_id, entry]),
  );
  const seenCases = new Set();
  const rows = [];
  for (const observationEntry of witness.observations) {
    const caseEntry = cases.get(observationEntry.case_id);
    requireCondition(
      caseEntry !== undefined && !seenCases.has(observationEntry.case_id),
      "witness observation case IDs are not a unique manifest projection",
    );
    seenCases.add(observationEntry.case_id);
    for (const write of observationEntry.observation?.writes ?? []) {
      if (write.kind !== "declaration") continue;
      requireCondition(
        typeof write.path === "string" &&
          Number.isInteger(write.index) &&
          typeof write.declaration_materialized_base64 === "string" &&
          typeof write.materialized_utf8_sha256 === "string" &&
          Number.isInteger(write.materialized_utf8_bytes),
        observationEntry.case_id + " has an invalid declaration write",
      );
      const inputBytes = Buffer.from(
        write.declaration_materialized_base64,
        "base64",
      );
      requireCondition(
        inputBytes.length === write.materialized_utf8_bytes &&
          sha256(inputBytes) === write.materialized_utf8_sha256 &&
          Buffer.compare(Buffer.from(inputBytes.toString("utf8"), "utf8"), inputBytes) === 0,
        observationEntry.case_id + " declaration materialized bytes are not pinned",
      );
      const input = inputBytes.toString("utf8");
      const options = optionsFromCase(caseEntry);
      const row = {
        id:
          "h2-7a-p/P1/" +
          observationEntry.case_id +
          "#" +
          String(write.index),
        stratum: "P1",
        source: {
          case_id: observationEntry.case_id,
          write_index: write.index,
          path: write.path,
          materialized_utf8_sha256: write.materialized_utf8_sha256,
        },
        file_name: write.path,
        options,
        input_utf8: input,
        input_sha256: sha256(inputBytes),
        input_bytes: inputBytes.length,
      };
      parseAndCover(row, input, counts, row.id);
      rows.push(row);
    }
  }
  requireCondition(seenCases.size === witness.case_manifest.cases.length, "witness manifest has unobserved cases");
  requireCondition(rows.length === 202, "P1 selection count drifted: " + rows.length);
  return rows;
}

function splitFixtureUnits(text, fixtureRelativePath) {
  const basename = path.posix.basename(fixtureRelativePath);
  const lines = text.split(/\r?\n/);
  if (/\.d\.ts$/iu.test(basename)) {
    return [{ name: basename, body: lines.join("\n") }];
  }
  const directive = /^\/\/\s*@[Ff]ile[Nn]ame:\s*(.+?)\s*$/u;
  const units = [];
  let current = null;
  const finish = () => {
    if (current !== null) {
      units.push({ name: current.name, body: current.lines.join("\n") });
    }
  };
  for (const line of lines) {
    const match = directive.exec(line);
    if (match !== null) {
      finish();
      current = { name: match[1].trim(), lines: [] };
    } else if (current !== null) {
      current.lines.push(line);
    }
  }
  finish();
  return units;
}

function walkFixtureFiles(suite) {
  const root = path.join(WORKSPACE, FIXTURE_ROOT, suite);
  const paths = [];
  const walk = (directory) => {
    const entries = fs.readdirSync(directory, { withFileTypes: true }).sort((left, right) =>
      compareBytes(left.name, right.name),
    );
    for (const entry of entries) {
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        walk(absolute);
      } else if (
        entry.isFile() &&
        (entry.name.endsWith(".ts") || entry.name.endsWith(".tsx"))
      ) {
        paths.push(
          path.relative(root, absolute).split(path.sep).join("/"),
        );
      }
    }
  };
  walk(root);
  return paths.sort(compareBytes);
}

function selectP2(counts) {
  const rows = [];
  const excluded = [];
  let units = 0;
  let parseExcluded = 0;
  let nonUtf8Fixtures = 0;
  for (const suite of ["compiler", "conformance"]) {
    for (const relativePath of walkFixtureFiles(suite)) {
      const fixture = normalizeFixtureRelativePath(suite, relativePath);
      const absolute = fixtureAbsolutePath(fixture);
      const bytes = fs.readFileSync(absolute);
      const fixtureSha = sha256(bytes);
      const text = decodeUtf8Fixture(bytes, fixture);
      if (text === null) {
        nonUtf8Fixtures += 1;
        excluded.push({
          id: "h2-7a-p/P2/" + fixture,
          stratum: "P2",
          source: { fixture, fixture_sha256: fixtureSha },
          reason: "non-utf8-fixture",
        });
        continue;
      }
      for (const unit of splitFixtureUnits(text, fixture)) {
        if (!/\.d\.ts$/iu.test(unit.name)) continue;
        units += 1;
        const row = {
          id: "h2-7a-p/P2/" + fixture + "#" + unit.name,
          stratum: "P2",
          source: {
            fixture,
            fixture_sha256: fixtureSha,
            unit: unit.name,
          },
          file_name: unit.name,
          options: { newLine: "lf", removeComments: false, target: null },
          input_utf8: unit.body,
          input_sha256: sha256(Buffer.from(unit.body, "utf8")),
          input_bytes: Buffer.byteLength(unit.body, "utf8"),
        };
        const sourceFile = sourceFileFor(row, unit.body);
        addKindCoverage(sourceFile, counts);
        const codes = diagnosticCodes(sourceFile);
        if (codes.length > 0) {
          parseExcluded += 1;
          excluded.push({
            id: row.id,
            stratum: "P2",
            source: row.source,
            reason: "parse-diagnostic",
            diagnostic_codes: codes,
          });
          continue;
        }
        rows.push(row);
      }
    }
  }
  requireCondition(units === 997, "P2 unit count drifted: " + units);
  requireCondition(parseExcluded === 4, "P2 parse exclusion count drifted: " + parseExcluded);
  requireCondition(rows.length === 993, "P2 row count drifted: " + rows.length);
  return { rows, excluded, units, parseExcluded, nonUtf8Fixtures };
}

function selectP3(counts) {
  const directory = path.join(WORKSPACE, TYPESCRIPT_LIB_DIRECTORY);
  const names = fs
    .readdirSync(directory)
    .filter((name) => name.endsWith(".d.ts"))
    .sort(compareBytes);
  requireCondition(names.length === 109, "P3 lib file count drifted: " + names.length);
  const rows = [];
  for (const name of names) {
    const relativePath = TYPESCRIPT_LIB_DIRECTORY + "/" + name;
    const inputBytes = readBytes(relativePath);
    const input = decodeUtf8Fixture(inputBytes, relativePath);
    requireCondition(input !== null, "P3 source is not valid UTF-8: " + name);
    const row = {
      id: "h2-7a-p/P3/" + name,
      stratum: "P3",
      source: { path: relativePath, sha256: sha256(inputBytes) },
      file_name: name,
      options: { newLine: "lf", removeComments: false, target: null },
      input_sha256: sha256(inputBytes),
      input_bytes: inputBytes.length,
    };
    const sourceFile = sourceFileFor(row, input);
    addKindCoverage(sourceFile, counts);
    requireCondition(
      sourceFile.parseDiagnostics.length === 0,
      row.id + " has parse diagnostics: " + diagnosticCodes(sourceFile).join(","),
    );
    rows.push(row);
  }
  return rows;
}

function selectP4(counts) {
  requireCondition(Array.isArray(P4_INPUTS) && P4_INPUTS.length === 46, "P4 roster count drifted");
  const crlf = P4_INPUTS.find((entry) => entry.id === "crlf-and-bom");
  requireCondition(
    crlf !== undefined &&
      typeof crlf.text === "string" &&
      crlf.text.charCodeAt(0) === 0xfeff,
    "P4 crlf-and-bom row lost its UTF-8 BOM",
  );
  const rows = [];
  const ids = new Set();
  for (const entry of P4_INPUTS) {
    requireCondition(
      typeof entry.id === "string" &&
        !ids.has(entry.id) &&
        typeof entry.text === "string" &&
        Array.isArray(entry.targets),
      "P4 roster row is malformed",
    );
    ids.add(entry.id);
    const inputBytes = Buffer.from(entry.text, "utf8");
    const row = makeSelectionRow({
      id: "h2-7a-p/P4/" + entry.id,
      stratum: "P4",
      source: { id: entry.id, targets: [...entry.targets] },
      fileName: entry.id + ".d.ts",
      options: { newLine: "lf", removeComments: entry.options !== undefined && entry.options.removeComments === true, target: null },
      input: entry.text,
      inputBytes,
    });
    parseAndCover(row, entry.text, counts, row.id);
    rows.push(row);
  }
  return rows;
}

function loadContext(strataToSelect) {
  const witness = readJson(WITNESSES_RELATIVE_PATH);
  requireCondition(
    witness.schema === 1 &&
      witness.kind === "h2-7a-public-observable-witnesses" &&
      isSha256(witness.case_manifest?.case_manifest_fingerprint) &&
      isSha256(witness.observation_content_roll_sha256),
    "witness artifact identity or pins are invalid",
  );
  const typescript = {
    version: ts.version,
    bundle: pathHash(TYPESCRIPT_BUNDLE),
    implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
  };
  const inputs = {
    packet: pathHash(PACKET_RELATIVE_PATH),
    parent_packet: pathHash(PARENT_PACKET_RELATIVE_PATH),
    witnesses: {
      ...pathHash(WITNESSES_RELATIVE_PATH),
      case_manifest_fingerprint: witness.case_manifest.case_manifest_fingerprint,
      observation_content_roll_sha256: witness.observation_content_roll_sha256,
    },
    lib_directory: TYPESCRIPT_LIB_DIRECTORY,
    fixture_roots: [...FIXTURE_ROOTS],
  };
  const counts = new Map();
  const selected = [];
  let p2 = { rows: [], excluded: [], units: 0, parseExcluded: 0, nonUtf8Fixtures: 0 };
  if (strataToSelect.includes("P1")) selected.push(...selectP1(witness, counts));
  if (strataToSelect.includes("P2")) {
    p2 = selectP2(counts);
    selected.push(...p2.rows);
  }
  if (strataToSelect.includes("P3")) selected.push(...selectP3(counts));
  if (strataToSelect.includes("P4")) selected.push(...selectP4(counts));
  const excluded = strataToSelect.includes("P2") ? p2.excluded : [];
  const rows = selected.sort((left, right) => compareBytes(left.id, right.id));
  const sortedExcluded = excluded.sort((left, right) => compareBytes(left.id, right.id));
  const context = {
    witness,
    typescript,
    inputs,
    strata: STRATA,
    selection: {
      rows,
      excluded: sortedExcluded,
      p2Units: p2.units,
      p2ParseExcluded: p2.parseExcluded,
      p2NonUtf8Fixtures: p2.nonUtf8Fixtures,
      kindCoverage: kindCoverageObject(counts),
    },
  };
  if (strataToSelect.length === 4) {
    requireCondition(rows.filter((row) => row.stratum === "P1").length === 202, "P1 row count drifted");
    requireCondition(rows.filter((row) => row.stratum === "P2").length === 993, "P2 gating row count drifted");
    requireCondition(rows.filter((row) => row.stratum === "P3").length === 109, "P3 row count drifted");
    requireCondition(rows.filter((row) => row.stratum === "P4").length === 46, "P4 row count drifted");
  }
  return context;
}

function inputBytesForRow(row) {
  if (row.stratum === "P3") {
    const bytes = readBytes(row.source.path);
    requireCondition(
      sha256(bytes) === row.input_sha256 &&
        bytes.length === row.input_bytes,
      row.id + " P3 input path/hash drifted",
    );
    return bytes;
  }
  const bytes = Buffer.from(row.input_utf8, "utf8");
  requireCondition(
    sha256(bytes) === row.input_sha256 &&
      bytes.length === row.input_bytes,
    row.id + " embedded input hash drifted",
  );
  return bytes;
}

function printerFor(row) {
  return ts.createPrinter({
    removeComments: row.options.removeComments,
    newLine:
      row.options.newLine === "crlf"
        ? ts.NewLineKind.CarriageReturnLineFeed
        : ts.NewLineKind.LineFeed,
    noEmitHelpers: true,
    target: row.options.target === null ? undefined : row.options.target,
    onlyPrintJsDocStyle: true,
    omitBraceSourceMapPositions: true,
  });
}

function printRow(row) {
  const inputBytes = inputBytesForRow(row);
  const input = inputBytes.toString("utf8");
  const sourceFile = sourceFileFor(row, input);
  requireCondition(
    sourceFile.parseDiagnostics.length === 0,
    row.id + " has parse diagnostics before printing: " +
      diagnosticCodes(sourceFile).join(","),
  );
  const output = printerFor(row).printFile(sourceFile);
  requireCondition(typeof output === "string", row.id + " printer returned a non-string");
  const outputBytes = Buffer.from(output, "utf8");
  return {
    id: row.id,
    expected_utf8: output,
    expected_sha256: sha256(outputBytes),
    expected_bytes: outputBytes.length,
  };
}

function readInternalControl(stratum, batchIndex) {
  const configured = process.env[CONTROL_FILE_ENV];
  requireCondition(configured !== undefined, CONTROL_FILE_ENV + " is required");
  let record;
  try {
    record = JSON.parse(fs.readFileSync(configured, "utf8"));
  } catch (error) {
    fail("observation control is not valid JSON: " + error.message);
  }
  requireCondition(
    record.schema === 1 &&
      record.stratum === stratum &&
      record.batch_index === batchIndex &&
      Array.isArray(record.rows) &&
      fingerprintIsValid(record, "control_fingerprint_sha256"),
    "observation control is invalid",
  );
  return record;
}

function observeFreshBatch(stratum, batchIndex, rows) {
  const controlPath = path.join(WORKSPACE, CONTROL_RELATIVE_PATH);
  const control = withFingerprint(
    {
      schema: 1,
      stratum,
      batch_index: batchIndex,
      rows,
    },
    "control_fingerprint_sha256",
  );
  writeFileAtomic(controlPath, render(control));
  const stdout = execFileSync(
    process.execPath,
    [
      GENERATOR_PATH,
      INTERNAL_OBSERVE_MODE,
      stratum,
      String(batchIndex),
    ],
    {
      cwd: WORKSPACE,
      encoding: "utf8",
      env: { ...process.env, [CONTROL_FILE_ENV]: controlPath },
      maxBuffer: 256 * 1024 * 1024,
    },
  );
  try {
    return JSON.parse(stdout);
  } catch (error) {
    fail("fresh observer returned invalid JSON: " + error.message);
  }
}

function validateExpectedBatch(result, rows, label) {
  requireCondition(Array.isArray(result) && result.length === rows.length, label + " result length changed");
  const expectedIds = rows.map((row) => row.id);
  const actualIds = result.map((entry) => entry.id);
  requireCondition(
    stableStringify(actualIds) === stableStringify(expectedIds),
    label + " result IDs changed",
  );
  for (const entry of result) {
    requireCondition(
      typeof entry.id === "string" &&
        typeof entry.expected_utf8 === "string" &&
        isSha256(entry.expected_sha256) &&
        Number.isInteger(entry.expected_bytes) &&
        sha256(Buffer.from(entry.expected_utf8, "utf8")) === entry.expected_sha256 &&
        Buffer.byteLength(entry.expected_utf8, "utf8") === entry.expected_bytes,
      label + " has an invalid expected output record",
    );
  }
}

function observeRows(selection) {
  const expected = new Map();
  const batches = [];
  for (const stratum of ["P1", "P2", "P3", "P4"]) {
    const stratumRows = selection.rows.filter((row) => row.stratum === stratum);
    for (let start = 0; start < stratumRows.length; start += BATCH_SIZE) {
      batches.push(stratumRows.slice(start, start + BATCH_SIZE));
    }
  }
  let completed = 0;
  for (const batch of batches) {
    const stratum = batch[0]?.stratum;
    requireCondition(stratum !== undefined, "empty observation batch");
    requireCondition(batch.every((row) => row.stratum === stratum), "observation batch crosses strata");
    const batchIndex = completed;
    const first = observeFreshBatch(stratum, batchIndex, batch);
    validateExpectedBatch(first, batch, "first fresh " + stratum + " batch " + batchIndex);
    const second = observeFreshBatch(stratum, batchIndex, batch);
    validateExpectedBatch(second, batch, "second fresh " + stratum + " batch " + batchIndex);
    requireCondition(
      stableStringify(first) === stableStringify(second),
      stratum + " batch " + batchIndex + " printer output is nondeterministic",
    );
    for (const entry of first) {
      requireCondition(!expected.has(entry.id), "duplicate observed row " + entry.id);
      expected.set(entry.id, entry);
    }
    completed += 1;
    process.stderr.write(
      "H2.7a printer reprint fresh batches: " +
        completed +
        "/" +
        batches.length +
        " (" +
        stratum +
        ")\n",
    );
  }
  requireCondition(expected.size === selection.rows.length, "fresh observation row count changed");
  return selection.rows.map((row) => expected.get(row.id));
}

function materializeRows(selection, expected) {
  const byId = new Map(expected.map((entry) => [entry.id, entry]));
  requireCondition(byId.size === expected.length, "expected output IDs are not unique");
  const rows = [];
  for (const selectionRow of selection.rows) {
    const observed = byId.get(selectionRow.id);
    requireCondition(observed !== undefined, "expected output is missing for " + selectionRow.id);
    const inputBytes = inputBytesForRow(selectionRow);
    const outputBytes = Buffer.from(observed.expected_utf8, "utf8");
    requireCondition(
      sha256(outputBytes) === observed.expected_sha256 &&
        outputBytes.length === observed.expected_bytes,
      selectionRow.id + " expected output bytes are not pinned",
    );
    const row = {
      id: selectionRow.id,
      stratum: selectionRow.stratum,
      source: selectionRow.source,
      file_name: selectionRow.file_name,
      options: selectionRow.options,
    };
    if (selectionRow.stratum !== "P3") row.input_utf8 = selectionRow.input_utf8;
    row.input_sha256 = selectionRow.input_sha256;
    row.input_bytes = selectionRow.input_bytes;
    if (selectionRow.stratum !== "P3") row.expected_utf8 = observed.expected_utf8;
    row.expected_sha256 = observed.expected_sha256;
    row.expected_bytes = observed.expected_bytes;
    row.fixed_point = Buffer.compare(inputBytes, outputBytes) === 0;
    row.repetitions = 2;
    rows.push(row);
  }
  return rows;
}

function p4InputSha256() {
  return sha256(Buffer.from(stableStringify(P4_INPUTS), "utf8"));
}

function contentRoll(rows) {
  return sha256(
    Buffer.from(
      stableStringify(rows.map((row) => [row.id, row.input_sha256, row.expected_sha256])),
      "utf8",
    ),
  );
}

function summaryFor(rows, selection) {
  const rowsByStratum = { P1: 0, P2: 0, P3: 0, P4: 0 };
  for (const row of rows) rowsByStratum[row.stratum] += 1;
  return {
    rows_by_stratum: rowsByStratum,
    gating_rows: rows.length,
    excluded_rows: selection.excluded.filter((entry) => entry.reason === "parse-diagnostic").length,
    fixed_point_rows: {
      P1: rows.filter((row) => row.stratum === "P1" && row.fixed_point).length,
    },
    input_bytes: rows.reduce((total, row) => total + row.input_bytes, 0),
    expected_bytes: rows.reduce((total, row) => total + row.expected_bytes, 0),
    kind_coverage: selection.kindCoverage,
    typescript_oracle_runs: rows.length * 2,
  };
}

function selectionProjection(row) {
  const projection = {
    id: row.id,
    stratum: row.stratum,
    source: row.source,
    file_name: row.file_name,
    options: row.options,
  };
  if (row.stratum !== "P3") projection.input_utf8 = row.input_utf8;
  projection.input_sha256 = row.input_sha256;
  projection.input_bytes = row.input_bytes;
  return projection;
}

function loadContract() {
  const contract = readJson(CONTRACT_RELATIVE_PATH);
  requireCondition(
    contract.$schema === "https://json-schema.org/draft/2020-12/schema" &&
      contract.type === "object" &&
      contract.additionalProperties === false &&
      Array.isArray(contract.required) &&
      stableStringify(contract.required) ===
        stableStringify(TOP_LEVEL_KEYS),
    "printer reprint schema top-level contract drifted",
  );
  return contract;
}

function validateRows(artifact, selection) {
  requireCondition(
    Array.isArray(artifact.rows) &&
      artifact.rows.length === selection.rows.length &&
      stableStringify(artifact.rows.map(selectionProjection)) ===
        stableStringify(selection.rows.map(selectionProjection)),
    "row selection, IDs, or input pins changed",
  );
  requireCondition(
    stableStringify(artifact.rows.map((row) => row.id)) ===
      stableStringify([...artifact.rows].sort((left, right) => compareBytes(left.id, right.id)).map((row) => row.id)),
    "rows are not sorted by id",
  );
  for (let index = 0; index < selection.rows.length; index += 1) {
    const row = artifact.rows[index];
    const selectionRow = selection.rows[index];
    const inputBytes = inputBytesForRow(selectionRow);
    requireCondition(
      row.repetitions === 2 &&
        isSha256(row.input_sha256) &&
        row.input_sha256 === sha256(inputBytes) &&
        row.input_bytes === inputBytes.length &&
        isSha256(row.expected_sha256) &&
        Number.isInteger(row.expected_bytes) &&
        row.expected_bytes ===
          (row.stratum === "P3"
            ? row.expected_bytes
            : Buffer.byteLength(row.expected_utf8, "utf8")) &&
        typeof row.fixed_point === "boolean" &&
        row.fixed_point ===
          (row.stratum === "P3"
            ? row.input_sha256 === row.expected_sha256 &&
              row.input_bytes === row.expected_bytes
            : Buffer.compare(inputBytes, Buffer.from(row.expected_utf8, "utf8")) === 0),
      row.id + " row byte fields are invalid",
    );
    if (row.stratum === "P3") {
      requireCondition(
        !Object.hasOwn(row, "input_utf8") &&
          !Object.hasOwn(row, "expected_utf8"),
        row.id + " P3 embeds text bytes",
      );
      const expectedPath = row.source.path;
      requireCondition(
        typeof expectedPath === "string" &&
          expectedPath.startsWith(TYPESCRIPT_LIB_DIRECTORY + "/"),
        row.id + " P3 source path is invalid",
      );
    } else {
      requireCondition(
        typeof row.input_utf8 === "string" &&
          typeof row.expected_utf8 === "string" &&
          Buffer.compare(Buffer.from(row.input_utf8, "utf8"), inputBytes) === 0 &&
          sha256(Buffer.from(row.expected_utf8, "utf8")) === row.expected_sha256,
        row.id + " embedded text bytes are invalid",
      );
    }
  }
}

function validateArtifact(artifact, context, full) {
  const contract = loadContract();
  validateJsonSchemaSubset(contract, artifact, "printer reprint artifact");
  requireCondition(
    stableStringify(Object.keys(artifact)) === stableStringify(TOP_LEVEL_KEYS),
    "artifact top-level key order or set changed",
  );
  requireCondition(
    artifact.schema === 1 &&
      artifact.kind === ARTIFACT_KIND &&
      artifact.status === ARTIFACT_STATUS &&
      artifact.phase === PHASE &&
      fingerprintIsValid(artifact, "printer_reprint_fingerprint_sha256"),
    "artifact identity or fingerprint is invalid",
  );
  assertPathHash(artifact.typescript.bundle, TYPESCRIPT_BUNDLE, "TypeScript bundle");
  assertPathHash(
    artifact.typescript.implementation,
    TYPESCRIPT_IMPLEMENTATION,
    "TypeScript implementation",
  );
  assertPathHash(artifact.generator, GENERATOR_RELATIVE_PATH, "generator");
  assertPathHash(artifact.contract, CONTRACT_RELATIVE_PATH, "contract");
  assertPathHash(artifact.inputs.packet, PACKET_RELATIVE_PATH, "packet");
  assertPathHash(
    artifact.inputs.parent_packet,
    PARENT_PACKET_RELATIVE_PATH,
    "parent packet",
  );
  assertPathHash(
    artifact.inputs.witnesses,
    WITNESSES_RELATIVE_PATH,
    "witness artifact",
  );
  requireCondition(
    stableStringify(artifact.typescript) === stableStringify(context.typescript) &&
      stableStringify(artifact.generator) === stableStringify(pathHash(GENERATOR_RELATIVE_PATH)) &&
      stableStringify(artifact.contract) === stableStringify(pathHash(CONTRACT_RELATIVE_PATH)) &&
      stableStringify(artifact.inputs) === stableStringify(context.inputs) &&
      stableStringify(artifact.printer_options) === stableStringify(PRINTER_OPTIONS) &&
      stableStringify(artifact.strata) === stableStringify(STRATA) &&
      artifact.p4_inputs_sha256 === p4InputSha256(),
    "artifact path, input, printer-option, or stratum pin changed",
  );
  requireCondition(
    stableStringify(artifact.excluded) === stableStringify(context.selection.excluded),
    "excluded P2 selection changed",
  );
  validateRows(artifact, context.selection);
  const expectedSummary = summaryFor(artifact.rows, context.selection);
  requireCondition(
    stableStringify(artifact.summary) === stableStringify(expectedSummary),
    "artifact summary changed",
  );
  requireCondition(
    artifact.reprint_content_roll_sha256 === contentRoll(artifact.rows) &&
      isSha256(artifact.reprint_content_roll_sha256),
    "reprint content roll changed",
  );
  if (full) {
    requireCondition(context.selection.p2Units === 997, "P2 unit prediction failed");
    requireCondition(context.selection.p2ParseExcluded === 4, "P2 exclusion prediction failed");
    requireCondition(artifact.rows.length === 1350, "gating row count prediction failed");
    requireCondition(
      stableStringify(artifact.summary.rows_by_stratum) ===
        stableStringify({ P1: 202, P2: 993, P3: 109, P4: 46 }),
      "stratum row predictions failed",
    );
    requireCondition(
      artifact.summary.fixed_point_rows.P1 === 197,
      "P1 fixed-point prediction failed",
    );
    requireCondition(artifact.summary.excluded_rows === 4, "P2 parse exclusion prediction failed");
    requireCondition(artifact.summary.typescript_oracle_runs === 2700, "oracle run prediction failed");
  }
  return artifact;
}

function buildArtifact(context, rows) {
  return withFingerprint(
    {
      schema: 1,
      kind: ARTIFACT_KIND,
      status: ARTIFACT_STATUS,
      phase: PHASE,
      typescript: context.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      inputs: context.inputs,
      printer_options: PRINTER_OPTIONS,
      strata: context.strata,
      rows,
      excluded: context.selection.excluded,
      p4_inputs_sha256: p4InputSha256(),
      summary: summaryFor(rows, context.selection),
      reprint_content_roll_sha256: contentRoll(rows),
    },
    "printer_reprint_fingerprint_sha256",
  );
}

function compareWholeArtifact(artifact) {
  const targetPath = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
  requireCondition(
    fs.existsSync(targetPath) &&
      fs.readFileSync(targetPath, "utf8") === render(artifact),
    "stale " + TARGET_RELATIVE_PATH + "; run " + GENERATOR_RELATIVE_PATH + " --write and review",
  );
}

class CheckReceiptMiss extends Error {}

function receiptKey(artifact) {
  return {
    generator_sha256: artifact.generator.sha256,
    contract_sha256: artifact.contract.sha256,
    artifact_fingerprint_sha256: artifact.printer_reprint_fingerprint_sha256,
    input_roll_sha256: artifact.reprint_content_roll_sha256,
  };
}

function loadReceipt() {
  let receipt;
  try {
    receipt = readJson(CHECK_RECEIPT_RELATIVE_PATH);
  } catch {
    throw new CheckReceiptMiss("absent-or-invalid");
  }
  if (
    receipt === null ||
    typeof receipt !== "object" ||
    receipt.schema !== 1 ||
    receipt.kind !== "h2-7a-printer-reprint-check-receipt" ||
    receipt.minted_by !== "successful-full-reobservation-check" ||
    receipt.workspace !== fs.realpathSync(WORKSPACE) ||
    receipt.node !== process.version ||
    !fingerprintIsValid(receipt, "receipt_fingerprint_sha256")
  ) {
    throw new CheckReceiptMiss("receipt-shape");
  }
  return receipt;
}

function attemptReceiptHit(context, artifact) {
  validateArtifact(artifact, context, true);
  compareWholeArtifact(artifact);
  const receipt = loadReceipt();
  const key = receiptKey(artifact);
  for (const [name, value] of Object.entries(key)) {
    if (receipt[name] !== value) throw new CheckReceiptMiss(name);
  }
  return artifact;
}

function mintCheckReceipt(artifact) {
  const receipt = withFingerprint(
    {
      schema: 1,
      kind: "h2-7a-printer-reprint-check-receipt",
      minted_by: "successful-full-reobservation-check",
      workspace: fs.realpathSync(WORKSPACE),
      node: process.version,
      ...receiptKey(artifact),
    },
    "receipt_fingerprint_sha256",
  );
  writeFileAtomic(
    path.join(WORKSPACE, CHECK_RECEIPT_RELATIVE_PATH),
    render(receipt),
  );
}

function printSummary(artifact, context, receiptState) {
  const by = artifact.summary.rows_by_stratum;
  const nonUtf8 = context.selection.p2NonUtf8Fixtures;
  const lines = [
    "wrote " + TARGET_RELATIVE_PATH + ":",
    "summary:",
    "  P1 rows=" + by.P1 + " fixed_point=" + artifact.summary.fixed_point_rows.P1,
    "  P2 units=" +
      context.selection.p2Units +
      " parse_excluded=" +
      artifact.summary.excluded_rows +
      " rows=" +
      by.P2 +
      " non_utf8_fixtures=" +
      nonUtf8,
    "  P3 rows=" + by.P3,
    "  P4 rows=" + by.P4,
    "  gating_rows=" + artifact.summary.gating_rows,
    "  input_bytes=" + artifact.summary.input_bytes,
    "  expected_bytes=" + artifact.summary.expected_bytes,
    "  kind_coverage OptionalType=" +
      (artifact.summary.kind_coverage.OptionalType ?? 0) +
      " NamedTupleMember=" +
      (artifact.summary.kind_coverage.NamedTupleMember ?? 0) +
      " ImportType=" +
      (artifact.summary.kind_coverage.ImportType ?? 0) +
      " TypePredicate=" +
      (artifact.summary.kind_coverage.TypePredicate ?? 0),
    "  typescript_oracle_runs=" + artifact.summary.typescript_oracle_runs,
    "  artifact_bytes=" + Buffer.byteLength(render(artifact), "utf8"),
    "  check_receipt=" + receiptState,
  ];
  process.stdout.write(lines.join("\n") + "\n");
}

function runInternalObserve() {
  requireCondition(process.argv.length === 5, "internal observation requires stratum and batch index");
  validateRuntime();
  const stratum = process.argv[3];
  const batchIndex = Number(process.argv[4]);
  requireCondition(Number.isInteger(batchIndex) && batchIndex >= 0, "invalid observation batch index");
  const control = readInternalControl(stratum, batchIndex);
  const results = control.rows.map((row) => printRow(row));
  process.stdout.write(JSON.stringify(results));
}

function runSelftest() {
  validateRuntime();
  const context = loadContext(["P4"]);
  const expected = observeRows(context.selection);
  const artifact = buildArtifact(context, materializeRows(context.selection, expected));
  validateArtifact(artifact, context, false);
  writeFileAtomic(path.join(WORKSPACE, SELFTEST_RELATIVE_PATH), render(artifact));
  const stored = readJson(SELFTEST_RELATIVE_PATH);
  validateArtifact(stored, context, false);
  process.stdout.write(
    "H2.7a printer reprint selftest is green: rows=" +
      stored.summary.gating_rows +
      " P4=" +
      stored.summary.rows_by_stratum.P4 +
      " double_observation=identical\n",
  );
}

function runWrite() {
  validateRuntime();
  const context = loadContext(["P1", "P2", "P3", "P4"]);
  const expected = observeRows(context.selection);
  const artifact = buildArtifact(context, materializeRows(context.selection, expected));
  validateArtifact(artifact, context, true);
  writeFileAtomic(path.join(WORKSPACE, TARGET_RELATIVE_PATH), render(artifact));
  const stored = readJson(TARGET_RELATIVE_PATH);
  validateArtifact(stored, context, true);
  compareWholeArtifact(stored);
  mintCheckReceipt(stored);
  printSummary(stored, context, "minted");
}

function runCheck() {
  validateRuntime();
  const context = loadContext(["P1", "P2", "P3", "P4"]);
  const tracked = readJson(TARGET_RELATIVE_PATH);
  try {
    attemptReceiptHit(context, tracked);
    process.stdout.write(
      "H2.7a printer reprint is fresh: rows=" +
        tracked.summary.gating_rows +
        " check_receipt=hit\n",
    );
    return;
  } catch (error) {
    if (!(error instanceof CheckReceiptMiss)) throw error;
    process.stderr.write(
      "H2.7a printer reprint check receipt: miss (" +
        error.message +
        "); running full fresh-process double observation\n",
    );
  }
  const expected = observeRows(context.selection);
  const artifact = buildArtifact(context, materializeRows(context.selection, expected));
  validateArtifact(artifact, context, true);
  compareWholeArtifact(artifact);
  mintCheckReceipt(artifact);
  process.stdout.write(
    "H2.7a printer reprint is fresh: rows=" +
      artifact.summary.gating_rows +
      " check_receipt=minted\n",
  );
}

// W-H2.7A-P stratum P4: hand-authored declaration-file INPUT texts, authored
// 2026-09-02 BEFORE any observation (packet §7.3). Expected bytes are only
// ever produced by the pinned printer at mint time. Each row names the print
// workers it targets. Never edit a row after its observation exists — retire
// the id and add a new one.
export const P4_INPUTS = Object.freeze([
  {
    id: "tuple-optional-named-rest",
    targets: ["emitTupleType", "emitOptionalType", "emitNamedTupleMember", "emitRestOrJSDocVariadicType"],
    text: "declare const t1: [string, number?, ...boolean[]];\ndeclare const t2: [a: string, b?: number, ...rest: boolean[]];\ndeclare const t3: [];\ndeclare const t4: [first: string];\ndeclare function f(...args: [x: number, y?: string]): void;\n",
  },
  {
    id: "type-predicates",
    targets: ["emitTypePredicate"],
    text: "declare function isString(x: unknown): x is string;\ndeclare function assertIsString(x: unknown): asserts x is string;\ndeclare function assertDefined(x: unknown): asserts x;\ndeclare class Box {\n    isEmpty(): this is Empty;\n}\ndeclare class Empty extends Box {\n}\n",
  },
  {
    id: "import-types",
    targets: ["emitImportTypeNode", "emitImportTypeNodeAttributes", "emitTypeArguments"],
    text: "declare const a: import(\"./mod\");\ndeclare const b: import(\"./mod\").Foo;\ndeclare const c: import(\"./mod\").Ns.Bar<string, number>;\ndeclare const d: typeof import(\"./mod\");\ndeclare const e: typeof import(\"./mod\").value;\ndeclare const f: import(\"./data.json\", { with: { \"resolution-mode\": \"import\" } });\ndeclare const g: import(\"./data.json\", { assert: { type: \"json\" } }).Data;\n",
  },
  {
    id: "mapped-types",
    targets: ["emitMappedType", "emitMappedTypeParameter", "emitTypeOperator"],
    text: "type A<T> = { [K in keyof T]: T[K] };\ntype B<T> = { readonly [K in keyof T]?: T[K] };\ntype C<T> = { -readonly [K in keyof T]-?: T[K] };\ntype D<T> = { +readonly [K in keyof T]+?: T[K] };\ntype E<T> = { [K in keyof T as `get${Capitalize<string & K>}`]: () => T[K] };\ntype F<T> = { [K in keyof T as Exclude<K, \"kind\">]: T[K] };\ntype G = { [K in \"a\" | \"b\"]: K };\n",
  },
  {
    id: "template-literal-types",
    targets: ["emitTemplateType", "emitTemplateTypeSpan", "emitLiteralType"],
    text: "type T1 = `prefix-${string}`;\ntype T2 = `${number}px`;\ntype T3<A extends string, B extends string> = `${A}.${B}`;\ntype T4 = `${\"a\" | \"b\"}-${1 | 2}`;\ntype T5 = ``;\ntype T6 = `only`;\n",
  },
  {
    id: "type-operators-unique-keyof-readonly",
    targets: ["emitTypeOperator", "emitIndexedAccessType", "emitArrayType"],
    text: "declare const sym: unique symbol;\ntype K<T> = keyof T;\ntype R = readonly string[];\ntype RR = readonly (string | number)[];\ntype RT = readonly [string, number];\ntype IA<T> = T[keyof T];\ntype IA2 = Array<string>[number][\"length\"];\ntype KK = keyof typeof sym;\n",
  },
  {
    id: "conditional-infer",
    targets: ["emitConditionalType", "emitInferType", "emitParenthesizedType"],
    text: "type C1<T> = T extends string ? \"s\" : \"n\";\ntype C2<T> = T extends (infer U)[] ? U : never;\ntype C3<T> = T extends [infer H, ...infer R] ? [H, R] : never;\ntype C4<T> = T extends (...args: any[]) => infer R ? R : never;\ntype C5<T> = T extends infer U extends string ? U : never;\ntype C6<T> = (T extends string ? 1 : 2) extends 1 ? \"yes\" : \"no\";\ntype C7<T> = T extends (T extends 1 ? 2 : 3) ? 4 : 5;\ntype C8<T> = T extends ((x: string) => void) ? true : false;\n",
  },
  {
    id: "unions-intersections-parens",
    targets: ["emitUnionType", "emitIntersectionType", "emitParenthesizedType", "emitFunctionType", "emitConstructorType"],
    text: "type U1 = string | number | undefined;\ntype U2 = (string | number)[];\ntype I1 = { a: 1 } & { b: 2 };\ntype I2 = (A | B) & C;\ntype F1 = ((x: number) => string) | null;\ntype F2 = (new (x: number) => object) | (() => void);\ntype F3 = <T>(x: T) => T;\ntype F4 = abstract new () => object;\ntype F5 = new <T>(x: T) => T;\ndeclare const u: | string | number;\ninterface A {\n}\ninterface B {\n}\ninterface C {\n}\n",
  },
  {
    id: "function-type-heads",
    targets: ["emitFunctionType", "emitFunctionTypeHead", "emitFunctionTypeBody", "emitParameter"],
    text: "type G1 = (this: Window, ev: Event) => any;\ntype G2 = (x?: number, ...rest: string[]) => void;\ntype G3 = ({ a, b }: {\n    a: string;\n    b: number;\n}) => void;\ntype G4 = ([x, y]: [number, number]) => void;\ntype G5 = (cb: (err: Error | null, value?: string) => void) => void;\n",
  },
  {
    id: "interface-forms",
    targets: ["emitInterfaceDeclaration", "emitPropertySignature", "emitMethodSignature", "emitCallSignature", "emitConstructSignature", "emitIndexSignature", "emitTypeParameters", "emitHeritageClause"],
    text: "export interface Empty {\n}\nexport interface Props<T = unknown, U extends T = T> extends Base<T>, Other {\n    readonly id: string;\n    name?: string;\n    \"quoted-key\": number;\n    42: boolean;\n    [Symbol.iterator](): Iterator<T>;\n    method<V>(arg: V): V;\n    optionalMethod?(): void;\n    get accessor(): T;\n    set accessor(value: T);\n    (call: number): string;\n    new (ctor: string): Props<T, U>;\n    [index: string]: unknown;\n    readonly [index: number]: T;\n}\ninterface Base<T> {\n}\ninterface Other {\n}\n",
  },
  {
    id: "class-forms",
    targets: ["emitClassDeclaration", "emitPropertyDeclaration", "emitMethodDeclaration", "emitConstructor", "emitAccessorDeclaration", "emitIndexSignature", "emitDecoratorsAndModifiers", "emitTypeParameters"],
    text: "export declare abstract class Shape<T extends object = {}> extends Base implements Drawable, Sized<T> {\n    #private;\n    static readonly kind: string;\n    private id;\n    protected name?: string;\n    public readonly size: number;\n    declare tag: string;\n    override label: string;\n    accessor count: number;\n    static accessor total: number;\n    definite!: string;\n    abstract area(): number;\n    protected abstract get bounds(): T;\n    private static helper(x: number): void;\n    method?<V>(v: V): V;\n    constructor(id: string);\n    constructor(id: number, name?: string);\n    get name2(): string;\n    set name2(value: string);\n    static [Symbol.hasInstance](value: unknown): boolean;\n    [key: string]: unknown;\n}\ndeclare class Base {\n}\ninterface Drawable {\n}\ninterface Sized<T> {\n}\nexport default class {\n    x: number;\n}\n",
  },
  {
    id: "enum-forms",
    targets: ["emitEnumDeclaration", "emitEnumMember", "emitLiteralType"],
    text: "export declare enum Color {\n    Red = 0,\n    Green = 1,\n    Blue = 2\n}\nexport declare const enum Flags {\n    None = 0,\n    A = 1,\n    B = 2,\n    AB = 3\n}\ndeclare enum Strings {\n    A = \"a\",\n    B = \"b\",\n    \"quoted key\" = \"q\",\n    Neg = -1,\n    Big = 1e21,\n    Computed = \"a\".length\n}\ndeclare enum Empty {\n}\ntype L = -1 | 1n | -2n | true | false | null | \"s\" | 'single';\n",
  },
  {
    id: "module-namespace-global",
    targets: ["emitModuleDeclaration", "emitModuleBlock", "emitNamespaceExportDeclaration"],
    text: "declare namespace A.B.C {\n    const x: number;\n    function f(): void;\n    namespace D {\n        type T = string;\n    }\n}\ndeclare module \"quoted\" {\n    export = A;\n}\ndeclare module \"wildcard/*\";\ndeclare global {\n    interface Window {\n        custom: string;\n    }\n}\ndeclare namespace Empty {\n}\nexport as namespace MyLib;\nexport {};\n",
  },
  {
    id: "import-export-forms",
    targets: ["emitImportEqualsDeclaration", "emitImportDeclaration", "emitExportDeclaration", "emitExportAssignment", "emitNamespaceImport", "emitNamespaceExport"],
    text: "import x = require(\"x\");\nimport type y = require(\"y\");\nimport z = A.B.C;\nimport type { T1, T2 as U2 } from \"./types\";\nimport { type T3, V3 } from \"./mixed\";\nimport * as ns from \"./ns\";\nimport def, { named } from \"./def\";\nimport type def2 from \"./def2\";\nimport attr from \"./data.json\" with { type: \"json\" };\nexport type { T1 };\nexport { type T3, V3 as W3 };\nexport * from \"./all\";\nexport * as star from \"./star\";\nexport type * from \"./types-only\";\nexport type * as tns from \"./types-only\";\nexport { default } from \"./reexport\";\nexport default def;\nexport = ns;\ndeclare namespace A.B {\n    const C: number;\n}\n",
  },
  {
    id: "variable-function-overloads",
    targets: ["emitVariableStatement", "emitFunctionDeclaration", "emitSignatureAndBody", "emitEmptyFunctionBody", "emitParameter", "emitTypeParameter"],
    text: "export declare const c1: string, c2: number;\nexport declare let l1: string | undefined;\ndeclare var v1: any;\nexport declare function overloaded(x: string): string;\nexport declare function overloaded(x: number): number;\nexport declare function overloaded<T extends object = {}>(x: T, ...rest: T[]): T;\ndeclare function withThis(this: Window, ev?: Event): void;\ndeclare function generic<const T extends readonly unknown[]>(x: T): T;\ndeclare function variance<in I, out O, in out IO>(i: I): O;\nexport declare function destructured({ a, b: [c, d] }: {\n    a: string;\n    b: [number, number];\n}, [e, , f]?: (string | undefined)[]): void;\nexport default function (): void;\n",
  },
  {
    id: "jsdoc-comments-retained",
    targets: ["shouldWriteComment", "emitLeadingComment", "emitTrailingComment", "emitComment"],
    text: "/**\n * File-level doc comment.\n */\n/** Doc for A. */\nexport interface A {\n    /** Doc for x. */\n    x: string;\n    /**\n     * Multi-line doc\n     * for y.\n     * @deprecated use x\n     */\n    y?: number;\n    // line comment dropped\n    /* block comment dropped */\n    z: boolean;\n}\n/** Doc for f. */\nexport declare function f(/** param doc */ a: string, b: /** type-arg doc */ Array</** inner */ string>): void;\nexport declare const v: /** before type */ string;\n/*! pinned comment kept */\nexport declare const p: number;\n/** trailing doc after last statement */\n",
  },
  {
    id: "triple-slash-directives",
    targets: ["emitTripleSlashDirectives", "emitNonTripleSlashLeadingComment"],
    text: "/// <reference no-default-lib=\"true\"/>\n/// <reference path=\"./other.d.ts\" />\n/// <reference types=\"node\" />\n/// <reference types=\"react\" resolution-mode=\"require\" />\n/// <reference types=\"vite/client\" resolution-mode=\"import\" preserve=\"true\" />\n/// <reference lib=\"es2015\" />\n/// <reference lib=\"dom\" preserve=\"true\" />\n/// <amd-module name=\"MyModule\" />\n/// <amd-dependency path=\"legacy/moduleA\" name=\"moduleA\" />\n/// <amd-dependency path=\"legacy/moduleB\" />\n/** doc after directives */\nexport declare const x: number;\n",
  },
  {
    id: "crlf-and-bom",
    targets: ["emitSourceFile", "emitLeadingComments"],
    text: "﻿/// <reference types=\"node\" />\r\n/** doc */\r\nexport declare const a: string;\r\nexport interface I {\r\n    /** m */\r\n    m(): void;\r\n}\r\n",
  },
  {
    id: "empty-file-and-only-comments",
    targets: ["emitSourceFile", "emitLeadingComments"],
    text: "/** only a doc comment */\n",
  },
  {
    id: "empty-file",
    targets: ["emitSourceFile"],
    text: "",
  },
  {
    id: "type-alias-forms",
    targets: ["emitTypeAliasDeclaration", "emitTypeLiteral", "emitTypeQuery", "emitThisType"],
    text: "export type Alias<T> = T;\ntype Empty = {};\ntype Single = {\n    a: 1;\n};\ntype Nested = {\n    inner: {\n        deep: {\n            value: string;\n        };\n    };\n    fn(): void;\n    [k: string]: unknown;\n};\ntype Q1 = typeof globalThis;\ntype Q2 = typeof x.y.z;\ntype Q3 = typeof f<string>;\ndeclare const x: {\n    y: {\n        z: number;\n    };\n};\ndeclare function f<T>(): T;\ninterface Fluent {\n    self(): this;\n    chain(): this | undefined;\n}\n",
  },
  {
    id: "keyword-types",
    targets: ["pipelineEmitWithHintWorker"],
    text: "declare const k1: any;\ndeclare const k2: unknown;\ndeclare const k3: number;\ndeclare const k4: bigint;\ndeclare const k5: object;\ndeclare const k6: boolean;\ndeclare const k7: string;\ndeclare const k8: symbol;\ndeclare const k9: void;\ndeclare const k10: undefined;\ndeclare const k11: never;\ndeclare const k12: null;\ndeclare const k13: intrinsic;\n",
  },
  {
    id: "computed-and-private-names",
    targets: ["emitComputedPropertyName", "emitPropertyDeclaration", "emitMethodDeclaration"],
    text: "declare const key: unique symbol;\nexport declare class K {\n    #private;\n    [key]: string;\n    [Symbol.iterator](): Iterator<number>;\n    static [Symbol.species]: typeof K;\n    [\"literal-name\"]: number;\n    get [key2](): string;\n}\ndeclare const key2: \"k\";\nexport interface KI {\n    [key]: string;\n    readonly [Symbol.toStringTag]: string;\n}\n",
  },
  {
    id: "generics-and-defaults",
    targets: ["emitTypeParameter", "emitTypeArguments", "emitTypeReference", "emitExpressionWithTypeArguments"],
    text: "export declare class Container<T, U extends keyof T = keyof T, V = T[U]> extends Base<T, U> implements I1<T>, I2<[U, V]> {\n    value: Map<T, Set<U>>;\n    nested: Promise<Array<Record<string, T>>>;\n}\ndeclare class Base<A, B> {\n}\ninterface I1<X> {\n}\ninterface I2<Y> {\n}\ntype LeadingFn = Array<<T>() => T>;\ntype Nested = Foo<Bar<Baz<1>>>;\ninterface Foo<T> {\n}\ninterface Bar<T> {\n}\ninterface Baz<T> {\n}\n",
  },
  {
    id: "declare-and-export-modifier-orders",
    targets: ["emitDecoratorsAndModifiers", "emitModifierList"],
    text: "export declare const a: number;\ndeclare const b: number;\nexport declare abstract class C {\n}\nexport default abstract class D {\n}\nexport declare namespace N {\n    export const inner: string;\n    export import Alias = N.Sub;\n    namespace Sub {\n    }\n}\ndeclare module M {\n    export = N;\n}\n",
  },
  {
    id: "string-literal-forms",
    targets: ["emitLiteralType", "getLiteralTextOfNode"],
    text: "declare const s1: \"double\";\ndeclare const s2: 'single';\ndeclare const s3: \"esc\\\"aped\";\ndeclare const s4: \"uni\\u00e9code\";\ndeclare const s5: \"\\n\\t\\\\\";\ndeclare const s6: \"日本語\";\ndeclare const s7: \"emoji😀\";\ndeclare module \"quoted-module\" {\n}\ndeclare module 'single-quoted' {\n}\nimport q from 'single-quoted-import';\nexport interface Q {\n    'single': 1;\n    \"double\": 2;\n}\n",
  },
  {
    id: "numeric-literal-forms",
    targets: ["emitLiteralType", "emitEnumMember"],
    text: "declare const n1: 1;\ndeclare const n2: 1.5;\ndeclare const n3: 1e10;\ndeclare const n4: 0x1F;\ndeclare const n5: 0o17;\ndeclare const n6: 0b101;\ndeclare const n7: 1_000_000;\ndeclare const n8: -0;\ndeclare const n9: 123n;\ndeclare const n10: 0x1Fn;\ndeclare enum E {\n    A = 1_000,\n    B = 0x10,\n    C = 1e3\n}\n",
  },
  {
    id: "object-and-array-binding-params",
    targets: ["emitParameter", "emitObjectBindingPattern", "emitArrayBindingPattern", "emitBindingElement"],
    text: "export declare function f({ a, b: { c }, ...rest }: {\n    a: string;\n    b: {\n        c: number;\n    };\n}, [x, [y], ...zs]: [number, [string], ...boolean[]]): void;\nexport declare function g({}: {}, []: []): void;\nexport declare function h({ a }?: {\n    a?: string;\n}): void;\n",
  },
  {
    id: "long-single-line-and-indent",
    targets: ["emitNodeListItems", "getSeparatingLineTerminatorCount"],
    text: "export declare function longSignature(parameterNumberOne: string, parameterNumberTwo: number, parameterNumberThree: boolean, parameterNumberFour: Array<string | number | boolean | null | undefined>): Promise<Map<string, Array<Record<string, Set<number>>>>>;\nexport declare const deep: {\n    a: {\n        b: {\n            c: {\n                d: {\n                    e: string;\n                };\n            };\n        };\n    };\n};\n",
  },
  {
    id: "accessors-in-interfaces-and-types",
    targets: ["emitAccessorDeclaration", "emitPropertySignature"],
    text: "export interface Acc {\n    get value(): number;\n    set value(v: number);\n    get readonlyValue(): string;\n}\nexport type AccT = {\n    get x(): 1;\n    set x(v: 1);\n};\nexport declare class AccC {\n    get a(): string;\n    set a(v: string);\n    static get b(): number;\n    protected set c(v: number);\n}\n",
  },
  {
    id: "abstract-constructor-types-and-new",
    targets: ["emitConstructorType", "emitConstructSignature", "emitTypeQuery"],
    text: "type Ctor<T> = new (...args: any[]) => T;\ntype AbstractCtor<T> = abstract new (...args: any[]) => T;\ntype GenericCtor = new <T>(value: T) => Box<T>;\ndeclare class Box<T> {\n    value: T;\n}\ndeclare const BoxCtor: typeof Box;\ndeclare function mixin<TBase extends abstract new (...args: any) => any>(base: TBase): TBase;\ninterface HasNew {\n    new (): HasNew;\n    new <T>(x: T): T;\n}\n",
  },
  {
    id: "jsdoc-in-type-positions",
    targets: ["emitLeadingComment", "emitTrailingComment", "emitNodeListItems"],
    text: "export type Mixed = /** first */ string | /** second */ number;\nexport type Tup = [/** a */ string, /** b */ number];\nexport interface Doc {\n    /** before */ x: /** after colon */ string /** trailing */;\n    m(/** p1 */ a: string, /** p2 */ b: number): /** ret */ void;\n}\nexport declare function fn<T /** tp */>(): T;\n",
  },
  {
    id: "index-signature-modifiers",
    targets: ["emitIndexSignature", "emitParametersForIndexSignature"],
    text: "export declare class IS {\n    [key: string]: unknown;\n    static [key: number]: string;\n    readonly [key: symbol]: boolean;\n    static readonly [key: `data-${string}`]: string;\n}\nexport interface ISI {\n    readonly [key: string]: unknown;\n    [key: number]: string;\n    [key: symbol]: boolean;\n    [key: `data-${string}`]: string;\n}\n",
  },
  {
    id: "optional-chaining-free-expressions-in-decls",
    targets: ["emitExportAssignment", "emitExpressionWithTypeArguments", "emitPropertyAccessExpression"],
    text: "declare const Base: {\n    new (): {\n        x: number;\n    };\n};\nexport declare class D extends Base {\n}\nexport declare class E extends Ns.Inner.Deep<string> {\n}\ndeclare namespace Ns.Inner {\n    class Deep<T> {\n    }\n}\nexport default Ns.Inner.Deep;\n",
  },
  {
    id: "nested-module-blocks-and-empty-blocks",
    targets: ["emitModuleBlock", "emitBlockStatements"],
    text: "declare namespace Outer {\n    namespace Inner {\n    }\n    namespace Inner2 {\n        namespace Inner3 {\n            const x: 1;\n        }\n    }\n    class Empty {\n    }\n    interface EmptyI {\n    }\n    enum EmptyE {\n    }\n    type EmptyT = {};\n    function noParams(): void;\n}\n",
  },
  {
    id: "this-types-and-fluent",
    targets: ["emitThisType", "emitTypePredicate", "emitFunctionType"],
    text: "export declare class Fluent {\n    self(): this;\n    map<U>(f: (this: this, value: this) => U): U;\n    is(): this is Fluent;\n    prop: this[];\n    opt?: this | null;\n}\ntype ThisFn = (this: void) => void;\n",
  },
  {
    id: "type-only-and-attributes-import-forms",
    targets: ["emitImportDeclaration", "emitImportAttributes", "emitImportAttribute"],
    text: "import json from \"./a.json\" with { type: \"json\" };\nimport json2 from \"./b.json\" assert { type: \"json\" };\nimport { x } from \"./c\" with { type: \"json\", \"other-key\": \"v\" };\nexport { y } from \"./d\" with { type: \"json\" };\nexport * from \"./e\" with { type: \"json\" };\nimport type Only from \"./f\";\nimport type * as NS from \"./g\";\nimport type { A, B } from \"./h\";\ndeclare const x: number, y: number;\n",
  },
  {
    id: "readonly-arrays-and-array-of-functions",
    targets: ["emitArrayType", "emitParenthesizedType", "emitTypeOperator"],
    text: "type A1 = (() => void)[];\ntype A2 = readonly (() => void)[];\ntype A3 = (string | number)[][];\ntype A4 = (keyof Foo)[];\ntype A5 = (typeof x)[];\ntype A6 = (infer U)[] extends never ? 1 : 2;\ntype A7 = (T extends string ? 1 : 2)[];\ninterface Foo {\n}\ndeclare const x: number;\ntype T = string;\n",
  },
  {
    id: "satisfies-free-literal-const-forms",
    targets: ["emitVariableDeclaration", "emitLiteralType"],
    text: "export declare const tuple: readonly [\"a\", \"b\"];\nexport declare const obj: {\n    readonly a: \"x\";\n    readonly b: 1;\n    readonly c: readonly [true, false];\n};\nexport declare const neg: -1;\nexport declare const big: 100n;\nexport declare const tpl: `t-${string}`;\n",
  },
  {
    id: "prologue-like-and-empty-statements",
    targets: ["emitSourceFileWorker", "emitEmptyStatement"],
    text: "\"use strict\";\n;\nexport declare const a: 1;\n;\n",
  },
  {
    id: "declare-function-and-class-in-module-with-export-default",
    targets: ["emitModuleDeclaration", "emitExportAssignment", "emitFunctionDeclaration"],
    text: "declare module \"lib\" {\n    function helper(): void;\n    class Impl {\n    }\n    export default Impl;\n    export { helper };\n}\ndeclare module \"lib2\" {\n    const value: number;\n    export = value;\n}\n",
  },
  {
    id: "unicode-identifiers-and-escapes",
    targets: ["emitIdentifierName", "getTextOfNode"],
    text: "export declare const caf\\u00e9: number;\nexport declare const 日本: string;\nexport declare const $_1: boolean;\nexport interface Ünïcode {\n    ñ: string;\n}\ndeclare namespace \\u0041bc {\n    const x: 1;\n}\n",
  },
  {
    id: "jsx-like-generic-arrow-types",
    targets: ["emitFunctionType", "emitTypeParameters"],
    text: "type Arrow1 = <T,>(x: T) => T;\ntype Arrow2 = <T extends string>(x: T) => T;\ntype Arrow3 = <T, U = T>(x: T, y?: U) => [T, U];\ntype Arrow4 = <const T>(x: T) => T;\n",
  },
  {
    // packet §5.5 control: intervening comments reach the UNFILTERED
    // position writers (emitTrailingCommentOfPosition) and survive
    // onlyPrintJsDocStyle; leading/trailing node comments do not.
    id: "unfiltered-intervening-comments",
    targets: ["emitNodeListItems", "emitTrailingCommentOfPosition", "emitTrailingCommentOfPositionNoNewline", "shouldWriteComment"],
    text: "declare const a: Array</* intervening block */ string>;\ndeclare const b: Map<// intervening line\nstring, number>;\ndeclare function f(/* p0 */ x: string, // after x\ny: number): void;\ndeclare function g(x: string /* before comma */, y: number): void;\ndeclare const t: [/* first */ string, /* second */ number];\ndeclare const u: /* leading dropped */ string | /* between dropped */ number;\n// line comment dropped\nexport {};\n",
  },
  {
    // packet §5.5 control: removeComments keeps the position-0 pinned
    // comment (emitDetachedComments) and drops everything else.
    id: "remove-comments-pinned-detached",
    targets: ["emitDetachedComments", "shouldWriteComment", "emitLeadingComment"],
    options: { removeComments: true },
    text: "/*! pinned position-zero comment */\n/** doc dropped under removeComments */\n// line dropped\nexport declare const a: string;\n/*! pinned but not at position zero — dropped */\nexport declare const b: number;\n",
  },
  {
    // packet §7.3: lone-surrogate string-literal SOURCE spellings print
    // from the parse tree (the printer copies source spelling).
    id: "lone-surrogate-source-spellings",
    targets: ["emitLiteralType", "getLiteralTextOfNode"],
    text: "declare const s1: \"\\uD800\";\ndeclare const s2: \"\\uDC00tail\";\ndeclare const s3: 'lead\\uD83D';\ndeclare const s4: \"\\uD83D\\uDE00\";\ndeclare module \"\\uD800mod\" {\n}\nexport interface L {\n    \"\\uDBFF\": 1;\n}\ntype T = `\\uD800${string}`;\n",
  },
]);

try {
  if (MODE === INTERNAL_OBSERVE_MODE) runInternalObserve();
  else if (MODE === "--selftest") runSelftest();
  else if (MODE === "--write") runWrite();
  else if (MODE === "--check") runCheck();
  else {
    fail(
      "usage: h2-7a-printer-reprint.mjs [--selftest|--write|--check]",
    );
  }
} catch (error) {
  process.stderr.write((error.stack ?? error.message ?? String(error)) + "\n");
  process.exitCode = 1;
}
