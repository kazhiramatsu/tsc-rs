import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-3a-owner-controls.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-3a-owner-controls.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-3a-owner-controls.schema.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const UTF8_BOM = Buffer.from([0xef, 0xbb, 0xbf]);

const FILES = Object.freeze([
  {
    path: "/project/plain.js",
    text: [
      "#!/usr/bin/env node",
      '"use strict";',
      "/** @type {number} */",
      "// retained leading comment",
      "const answer = 42; // retained trailing comment",
      "function checked() { return 5 || true; }",
      "",
    ].join("\n"),
  },
  {
    path: "/project/module.mjs",
    text: "export const moduleValue = 1;\n",
  },
  {
    path: "/project/common.cjs",
    text: "module.exports = { value: 2 };\n",
  },
]);

const CHECK_JS_STATES = Object.freeze([
  ["absent", undefined],
  ["false", false],
  ["true", true],
]);

function fail(message) {
  throw new Error(message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
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

function withFingerprint(value, field) {
  return { ...value, [field]: sha256(Buffer.from(canonical(value), "utf8")) };
}

function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function pathHash(relativePath) {
  return { path: relativePath, sha256: sha256(readBytes(relativePath)) };
}

function bytesRecord(text) {
  const bytes = Buffer.from(text, "utf8");
  return {
    utf8_base64: bytes.toString("base64"),
    utf8_sha256: sha256(bytes),
    utf8_bytes: bytes.length,
  };
}

function validateRuntime() {
  const node = readBytes(".node-version").toString("utf8").trim();
  requireCondition(node === EXPECTED_NODE, "unexpected Node pin");
  requireCondition(process.version === `v${node}`, `requires Node ${node}`);
  requireCondition(ts.version === "6.0.3", "unexpected TypeScript runtime");
}

function hasDirectory(files, directory) {
  const prefix = `${ts.normalizePath(directory).replace(/\/$/, "")}/`;
  return [...files.keys()].some((fileName) => fileName.startsWith(prefix));
}

function createVirtualProgram(options) {
  const fileMap = new Map(FILES.map((file) => [file.path, file.text]));
  const baseHost = ts.createCompilerHost(options, true);
  const host = {
    ...baseHost,
    getCurrentDirectory: () => "/project",
    useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: (fileName) => fileName,
    trace() {},
    fileExists(fileName) {
      const normalized = ts.normalizePath(fileName);
      return fileMap.has(normalized) || baseHost.fileExists(normalized);
    },
    readFile(fileName) {
      const normalized = ts.normalizePath(fileName);
      return fileMap.get(normalized) ?? baseHost.readFile(normalized);
    },
    directoryExists(directory) {
      return hasDirectory(fileMap, directory) ||
        (baseHost.directoryExists?.(directory) ?? false);
    },
    getDirectories(directory) {
      return baseHost.getDirectories?.(directory) ?? [];
    },
    realpath(fileName) {
      const normalized = ts.normalizePath(fileName);
      return fileMap.has(normalized)
        ? normalized
        : (baseHost.realpath?.(normalized) ?? normalized);
    },
    getSourceFile(fileName, languageVersion) {
      const normalized = ts.normalizePath(fileName);
      const text = fileMap.get(normalized);
      if (text === undefined) return baseHost.getSourceFile(fileName, languageVersion);
      return ts.createSourceFile(
        normalized,
        text,
        languageVersion,
        true,
        ts.getScriptKindFromFileName(normalized),
      );
    },
  };
  return ts.createProgram(FILES.map((file) => file.path), options, host);
}

function serializeDiagnostic(diagnostic) {
  return {
    code: diagnostic.code,
    category: ts.DiagnosticCategory[diagnostic.category],
    file: diagnostic.file?.fileName ?? null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    message: ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
  };
}

function outputKind(fileName) {
  const lower = fileName.toLowerCase();
  if (lower.endsWith(".mjs")) return "mjs";
  if (lower.endsWith(".cjs")) return "cjs";
  if (lower.endsWith(".js")) return "javascript";
  return "other";
}

function serializeWrite(arguments_, index) {
  const [fileName, text, bom, onError, sourceFiles] = arguments_;
  const callback = Buffer.from(text, "utf8");
  const materialized = bom ? Buffer.concat([UTF8_BOM, callback]) : callback;
  return {
    index,
    path: ts.normalizePath(fileName),
    kind: outputKind(fileName),
    callback_utf8_base64: callback.toString("base64"),
    callback_utf8_sha256: sha256(callback),
    callback_utf8_bytes: callback.length,
    write_byte_order_mark: bom,
    materialized_utf8_sha256: sha256(materialized),
    materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined,
    source_files: (sourceFiles ?? []).map((source) =>
      ts.normalizePath(source.fileName)
    ),
  };
}

function optionsFor(checkJs) {
  const options = {
    allowJs: true,
    target: ts.ScriptTarget.ESNext,
    module: ts.ModuleKind.Preserve,
    outDir: "/project/dist",
    newLine: ts.NewLineKind.LineFeed,
    ignoreDeprecations: "6.0",
  };
  if (checkJs !== undefined) options.checkJs = checkJs;
  return options;
}

function observe(checkJs) {
  const program = createVirtualProgram(optionsFor(checkJs));
  const writes = [];
  const diagnostics = ts.getPreEmitDiagnostics(program);
  const emitResult = program.emit(
    undefined,
    (...arguments_) => writes.push(arguments_),
  );
  return withFingerprint(
    {
      reported_diagnostics: diagnostics.map(serializeDiagnostic),
      emit_diagnostics: emitResult.diagnostics.map(serializeDiagnostic),
      emit_skipped: emitResult.emitSkipped,
      writes: writes.map(serializeWrite),
    },
    "run_fingerprint_sha256",
  );
}

function repeatedObservation(checkJs, state) {
  const first = observe(checkJs);
  const second = observe(checkJs);
  requireCondition(
    first.run_fingerprint_sha256 === second.run_fingerprint_sha256,
    `checkJs=${state} owner control is nondeterministic`,
  );
  return {
    check_js_state: state,
    repetitions: 2,
    observation: first,
  };
}

function buildArtifact() {
  validateRuntime();
  const variants = CHECK_JS_STATES.map(([state, value]) =>
    repeatedObservation(value, state)
  );
  const expectedCodes = [[], [], [2872]];
  const expectedPaths = [
    "/project/dist/plain.js",
    "/project/dist/module.mjs",
    "/project/dist/common.cjs",
  ];
  for (const [index, variant] of variants.entries()) {
    const observation = variant.observation;
    requireCondition(
      canonical(observation.reported_diagnostics.map((diagnostic) => diagnostic.code)) ===
        canonical(expectedCodes[index]),
      `checkJs=${variant.check_js_state} diagnostic set changed`,
    );
    requireCondition(
      observation.emit_diagnostics.length === 0 && !observation.emit_skipped,
      `checkJs=${variant.check_js_state} emit result changed`,
    );
    requireCondition(
      canonical(observation.writes.map((write) => write.path)) ===
        canonical(expectedPaths),
      `checkJs=${variant.check_js_state} output paths changed`,
    );
    for (const [writeIndex, write] of observation.writes.entries()) {
      requireCondition(
        write.callback_utf8_sha256 === sha256(Buffer.from(FILES[writeIndex].text, "utf8")) &&
          canonical(write.source_files) === canonical([FILES[writeIndex].path]),
        `checkJs=${variant.check_js_state} output ${writeIndex} changed`,
      );
    }
  }
  requireCondition(
    variants.every(
      (variant) =>
        canonical(variant.observation.writes) ===
        canonical(variants[0].observation.writes),
    ),
    "checkJs changed JavaScript source routing or output bytes",
  );

  const control = withFingerprint(
    {
      control_id: "javascript-family-relocation-and-checking",
      input: {
        current_directory: "/project",
        roots: FILES.map((file) => file.path),
        files: FILES.map((file) => ({
          path: file.path,
          root: true,
          ...bytesRecord(file.text),
        })),
        target: "ESNext(99)",
        module: "Preserve(200)",
        out_dir: "/project/dist",
        allow_js: true,
        check_js_states: CHECK_JS_STATES.map(([state]) => state),
        new_line: "LF(1)",
        ignore_deprecations: "6.0",
      },
      variants,
    },
    "control_fingerprint_sha256",
  );

  return withFingerprint(
    {
      schema: 1,
      phase: "H2.3a-javascript-source-owner-controls",
      status: "qualified",
      typescript: {
        version: ts.version,
        source_commit: SOURCE_COMMIT,
        bundle: pathHash(TYPESCRIPT_BUNDLE),
        implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
      },
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      controls: [control],
      summary: {
        controls: 1,
        variants: 3,
        exact_outputs: 9,
        typescript_runs: 6,
        reported_diagnostics: 1,
      },
    },
    "controls_fingerprint_sha256",
  );
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

const artifact = buildArtifact();
const rendered = render(artifact);
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
  process.stdout.write(
    `wrote ${TARGET_RELATIVE_PATH}: variants=${artifact.summary.variants} outputs=${artifact.summary.exact_outputs}\n`,
  );
} else if (mode === "--check") {
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-3a-owner-controls.mjs --write and review`,
  );
  process.stdout.write(
    `H2.3a owner controls are fresh: outputs=${artifact.summary.exact_outputs}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-3a-owner-controls.mjs [--write|--check]");
}
