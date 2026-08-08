import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const OUTPUT_RELATIVE_PATH = "ratchets/h1-active-transform.v1.json";
const OUTPUT_PATH = path.join(WORKSPACE, OUTPUT_RELATIVE_PATH);
const PROFILE_RELATIVE_PATH = "ratchets/h1-emit-profile.v1.json";
const EMIT_ORACLE_RELATIVE_PATH = "ratchets/h1-emit-oracle.v1.json";
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

const files = new Map([
  [
    "/project/missing.ts",
    [
      "export default function used() {}",
      "export const unused = 1;",
      "export type TypeOnly = string;",
      "",
    ].join("\n"),
  ],
  [
    "/project/main.ts",
    [
      "import used, { unused, type TypeOnly } from './missing';",
      "used();",
      "const runtime = 1;",
      "type Shape = { value: number };",
      "export { runtime, Shape };",
      "",
    ].join("\n"),
  ],
]);

const rootNames = [...files.keys()];
const options = Object.freeze({
  target: ts.ScriptTarget.ESNext,
  module: ts.ModuleKind.Preserve,
  useDefineForClassFields: true,
  noLib: true,
  listEmittedFiles: true,
  newLine: ts.NewLineKind.LineFeed,
});

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function fileSha256(relativePath) {
  return sha256(fs.readFileSync(path.join(WORKSPACE, relativePath)));
}

function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}

function normalize(fileName) {
  return path.posix.normalize(fileName.replaceAll("\\", "/"));
}

function textRecord(filePath, text) {
  const bytes = Buffer.from(text, "utf8");
  return {
    path: filePath,
    text,
    utf8_sha256: sha256(bytes),
    utf8_bytes: bytes.length,
  };
}

function makeHost(writes) {
  const host = ts.createCompilerHost(options, true);
  host.getCurrentDirectory = () => "/project";
  host.getCanonicalFileName = (fileName) => normalize(fileName);
  host.useCaseSensitiveFileNames = () => true;
  host.getNewLine = () => "\n";
  host.fileExists = (fileName) => files.has(normalize(fileName));
  host.readFile = (fileName) => files.get(normalize(fileName));
  host.directoryExists = (directory) => {
    const normalized = normalize(directory);
    return normalized === "/" || normalized === "/project";
  };
  host.getDirectories = () => [];
  host.realpath = normalize;
  host.getSourceFile = (fileName, languageVersion) => {
    const normalized = normalize(fileName);
    const text = files.get(normalized);
    return text === undefined
      ? undefined
      : ts.createSourceFile(
          normalized,
          text,
          languageVersion,
          true,
          ts.ScriptKind.TS,
        );
  };
  host.writeFile = (
    fileName,
    text,
    writeByteOrderMark,
    _onError,
    sourceFiles,
  ) => {
    writes.push({
      ...textRecord(normalize(fileName), text),
      write_byte_order_mark: writeByteOrderMark,
      source_files: (sourceFiles ?? []).map((source) => normalize(source.fileName)),
    });
  };
  return host;
}

function observe() {
  const writes = [];
  const program = ts.createProgram({
    rootNames,
    options,
    host: makeHost(writes),
  });
  const semanticDiagnostics = program.getSemanticDiagnostics();
  const result = program.emit();
  return {
    semantic_diagnostic_codes: semanticDiagnostics.map((diagnostic) => diagnostic.code),
    emit_skipped: result.emitSkipped,
    emit_diagnostic_codes: result.diagnostics.map((diagnostic) => diagnostic.code),
    emitted_files: result.emittedFiles ?? [],
    writes,
  };
}

function observeStructuralTransform() {
  const emitOracle = JSON.parse(
    fs.readFileSync(path.join(WORKSPACE, EMIT_ORACLE_RELATIVE_PATH), "utf8"),
  );
  const fixture = emitOracle.cases.find(
    (entry) => entry.input.id === "erasable-typescript",
  );
  requireCondition(fixture !== undefined, "missing erasable TypeScript oracle fixture");
  const input = fixture.input.root_files[0];
  const text = Buffer.from(input.utf8_base64, "base64").toString("utf8");
  const source = ts.createSourceFile(
    input.path,
    text,
    ts.ScriptTarget.ESNext,
    true,
    ts.ScriptKind.TS,
  );
  const result = ts.transform(
    source,
    [
      ts.transformTypeScript,
      ts.transformClassFields,
      ts.transformECMAScriptModule,
    ],
    options,
  );
  const transformed = result.transformed[0];
  const statementKinds = transformed.statements.map(
    (statement) => ts.SyntaxKind[statement.kind],
  );
  const transformedRootTransformFlags = transformed.transformFlags;
  result.dispose();
  return {
    source_path: input.path,
    source_utf8_sha256: input.utf8_sha256,
    parsed_root_transform_flags: source.transformFlags,
    transformed_root_transform_flags: transformedRootTransformFlags,
    transformed_statement_kinds: statementKinds,
    emitted_statement_count: statementKinds.filter(
      (kind) => kind !== "NotEmittedStatement",
    ).length,
  };
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
  const nodeVersion = fs
    .readFileSync(path.join(WORKSPACE, ".node-version"), "utf8")
    .trim();
  requireCondition(nodeVersion === EXPECTED_NODE_VERSION, "unexpected Node pin");
  requireCondition(
    process.version === `v${EXPECTED_NODE_VERSION}`,
    `H1.3 transform oracle requires Node ${EXPECTED_NODE_VERSION}; running ${process.version}`,
  );
  requireCondition(
    ts.version === EXPECTED_TYPESCRIPT_VERSION,
    `unexpected TypeScript runtime ${ts.version}`,
  );
  requireCondition(
    fileSha256(TYPESCRIPT_RELATIVE_PATH) === EXPECTED_TYPESCRIPT_SHA256,
    "vendored typescript.js differs from the reviewed pin",
  );
  requireCondition(
    fileSha256(TSC_RELATIVE_PATH) === EXPECTED_TSC_SHA256,
    "vendored _tsc.js differs from the reviewed pin",
  );
}

validateRuntime();
const first = observe();
const second = observe();
requireCondition(canonical(first) === canonical(second), "H1.3 oracle is nondeterministic");
requireCondition(first.semantic_diagnostic_codes.length === 0, "alias fixture has diagnostics");
requireCondition(!first.emit_skipped, "alias fixture unexpectedly skipped emit");
requireCondition(first.emit_diagnostic_codes.length === 0, "alias fixture has emit diagnostics");
requireCondition(first.writes.length === 2, "alias fixture must write two JavaScript files");

const profileBytes = fs.readFileSync(path.join(WORKSPACE, PROFILE_RELATIVE_PATH));
const emitOracleBytes = fs.readFileSync(
  path.join(WORKSPACE, EMIT_ORACLE_RELATIVE_PATH),
);
const artifact = {
  schema: 1,
  status: "frozen",
  phase: "H1.3-active-transform-resolver",
  typescript_version: EXPECTED_TYPESCRIPT_VERSION,
  source_commit: EXPECTED_SOURCE_COMMIT,
  authority: {
    typescript_js: {
      path: TYPESCRIPT_RELATIVE_PATH,
      sha256: EXPECTED_TYPESCRIPT_SHA256,
    },
    tsc_js: {
      path: TSC_RELATIVE_PATH,
      sha256: EXPECTED_TSC_SHA256,
    },
    profile: {
      path: PROFILE_RELATIVE_PATH,
      sha256: sha256(profileBytes),
    },
    emit_oracle: {
      path: EMIT_ORACLE_RELATIVE_PATH,
      sha256: sha256(emitOracleBytes),
    },
  },
  compiler_options: {
    target: options.target,
    module: options.module,
    useDefineForClassFields: options.useDefineForClassFields,
    noLib: options.noLib,
    listEmittedFiles: options.listEmittedFiles,
    newLine: options.newLine,
  },
  inputs: [...files].map(([filePath, text]) => textRecord(filePath, text)),
  determinism: {
    repeat_runs: 2,
    exact_match: true,
  },
  structural_probe: observeStructuralTransform(),
  observation: first,
};
const rendered = `${JSON.stringify(artifact, null, 2)}\n`;

const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(OUTPUT_PATH, rendered);
  process.stdout.write(`wrote ${OUTPUT_RELATIVE_PATH}\n`);
} else if (mode === "--check") {
  requireCondition(fs.existsSync(OUTPUT_PATH), `missing ${OUTPUT_RELATIVE_PATH}`);
  requireCondition(
    fs.readFileSync(OUTPUT_PATH, "utf8") === rendered,
    `stale ${OUTPUT_RELATIVE_PATH}; run with --write and review`,
  );
  process.stdout.write("H1.3 active-transform oracle is fresh: writes=2\n");
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  throw new Error("usage: h1-active-transform.mjs [--write|--check]");
}
