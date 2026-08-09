import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-3d-owner-controls.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-3d-owner-controls.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-3d-owner-controls.schema.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const UTF8_BOM = Buffer.from([0xef, 0xbb, 0xbf]);

const MULTILINE_JSON = [
  "{",
  '  "a":1,',
  '  "same": [true,{"emoji":"😀"},],',
  "}",
  "",
].join("\n");

const COMPACT_JSON = '{"escaped":"x\\u0041","nested":[1,{"ok":true}]}';
const FORMAT_JSON = '{"format":"stable","items":[1,true,null]}';

function options(module, moduleResolution, extra = {}) {
  return {
    target: ts.ScriptTarget.ESNext,
    module,
    ...(moduleResolution === undefined ? {} : { moduleResolution }),
    resolveJsonModule: true,
    newLine: ts.NewLineKind.LineFeed,
    ignoreDeprecations: "6.0",
    ...extra,
  };
}

function diagnosticCodesForModule(module) {
  return module === ts.ModuleKind.UMD || module === ts.ModuleKind.System
    ? [5071]
    : [];
}

function jsonControl(controlId, module, moduleResolution) {
  return {
    control_id: controlId,
    roots: ["/project/format.json"],
    files: [{ path: "/project/format.json", text: FORMAT_JSON }],
    compiler_options: options(module, moduleResolution, {
      outDir: "/project/dist",
    }),
    expected_diagnostic_codes: diagnosticCodesForModule(module),
    expected_paths: ["/project/dist/format.json"],
  };
}

const CONTROLS = Object.freeze([
  {
    control_id: "multiline-lf-formatting-and-trailing-comma-rules",
    roots: ["/project/data.json"],
    files: [{ path: "/project/data.json", text: MULTILINE_JSON }],
    compiler_options: options(ts.ModuleKind.Preserve, ts.ModuleResolutionKind.Bundler, {
      outDir: "/project/dist",
    }),
    expected_paths: ["/project/dist/data.json"],
  },
  {
    control_id: "crlf-input-bom-output-and-object-trailing-comma",
    roots: ["/project/bom.json"],
    files: [{ path: "/project/bom.json", text: "\ufeff{\r\n\t\"a\":1,\r\n}\r\n" }],
    compiler_options: options(ts.ModuleKind.CommonJS, ts.ModuleResolutionKind.Node10, {
      outDir: "/project/dist",
      newLine: ts.NewLineKind.CarriageReturnLineFeed,
      emitBOM: true,
    }),
    expected_paths: ["/project/dist/bom.json"],
  },
  {
    control_id: "compact-text-escapes-and-nested-values",
    roots: ["/project/compact.json"],
    files: [{ path: "/project/compact.json", text: COMPACT_JSON }],
    compiler_options: options(ts.ModuleKind.ESNext, ts.ModuleResolutionKind.Bundler, {
      outDir: "/project/dist",
    }),
    expected_paths: ["/project/dist/compact.json"],
  },
  jsonControl("amd-copy-is-module-invariant", ts.ModuleKind.AMD, ts.ModuleResolutionKind.Node10),
  jsonControl("umd-copy-is-module-invariant", ts.ModuleKind.UMD, ts.ModuleResolutionKind.Node10),
  jsonControl("system-copy-is-module-invariant", ts.ModuleKind.System, ts.ModuleResolutionKind.Node10),
  jsonControl("node16-copy-is-module-invariant", ts.ModuleKind.Node16, ts.ModuleResolutionKind.Node16),
  jsonControl("node18-copy-is-module-invariant", ts.ModuleKind.Node18, ts.ModuleResolutionKind.NodeNext),
  jsonControl("node20-copy-is-module-invariant", ts.ModuleKind.Node20, ts.ModuleResolutionKind.NodeNext),
  jsonControl("nodenext-copy-is-module-invariant", ts.ModuleKind.NodeNext, ts.ModuleResolutionKind.NodeNext),
  {
    control_id: "json-without-outdir-is-not-emit-eligible",
    roots: ["/project/data.json"],
    files: [{ path: "/project/data.json", text: FORMAT_JSON }],
    compiler_options: options(ts.ModuleKind.Preserve, ts.ModuleResolutionKind.Bundler),
    expected_paths: [],
  },
  {
    control_id: "same-location-json-output-is-suppressed",
    roots: ["/project/data.json"],
    files: [{ path: "/project/data.json", text: FORMAT_JSON }],
    compiler_options: options(ts.ModuleKind.Preserve, ts.ModuleResolutionKind.Bundler, {
      outDir: "/project",
    }),
    expected_paths: [],
  },
  {
    control_id: "empty-json-emits-empty-callback",
    roots: ["/project/empty.json"],
    files: [{ path: "/project/empty.json", text: "" }],
    compiler_options: options(ts.ModuleKind.Preserve, ts.ModuleResolutionKind.Bundler, {
      outDir: "/project/dist",
    }),
    expected_paths: ["/project/dist/empty.json"],
  },
  {
    control_id: "mixed-typescript-json-relocation-and-write-order",
    roots: ["/project/data.json", "/project/main.ts"],
    files: [
      { path: "/project/data.json", text: '{"value":1}' },
      { path: "/project/main.ts", text: "export const marker: number = 1;\n" },
    ],
    compiler_options: options(ts.ModuleKind.Preserve, ts.ModuleResolutionKind.Bundler, {
      outDir: "/project/dist",
    }),
    expected_paths: ["/project/dist/data.json", "/project/dist/main.js"],
  },
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
  if (Array.isArray(value)) return "[" + value.map(canonical).join(",") + "]";
  if (value !== null && typeof value === "object") {
    return "{" + Object.keys(value)
      .sort()
      .map((key) => JSON.stringify(key) + ":" + canonical(value[key]))
      .join(",") + "}";
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
  requireCondition(process.version === "v" + node, "requires Node " + node);
  requireCondition(ts.version === "6.0.3", "unexpected TypeScript runtime");
}

function hasDirectory(files, directory) {
  const prefix = ts.normalizePath(directory).replace(/\/$/, "") + "/";
  return [...files.keys()].some((fileName) => fileName.startsWith(prefix));
}

function createVirtualProgram(control) {
  const fileMap = new Map(control.files.map((file) => [file.path, file.text]));
  const baseHost = ts.createCompilerHost(control.compiler_options, true);
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
  return ts.createProgram(control.roots, control.compiler_options, host);
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
  return fileName.toLowerCase().endsWith(".json") ? "json" : "javascript";
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

function observe(control) {
  const program = createVirtualProgram(control);
  const writes = [];
  const reported = ts.getPreEmitDiagnostics(program);
  const emitResult = program.emit(
    undefined,
    (...arguments_) => writes.push(arguments_),
  );
  return withFingerprint(
    {
      reported_diagnostics: reported.map(serializeDiagnostic),
      emit_diagnostics: emitResult.diagnostics.map(serializeDiagnostic),
      emit_skipped: emitResult.emitSkipped,
      writes: writes.map(serializeWrite),
    },
    "run_fingerprint_sha256",
  );
}

function serializeOptions(options_) {
  return Object.fromEntries(
    Object.entries(options_).sort(([left], [right]) => left.localeCompare(right)),
  );
}

function buildControl(control) {
  const first = observe(control);
  const second = observe(control);
  const expectedDiagnosticCodes = control.expected_diagnostic_codes ?? [];
  requireCondition(
    first.run_fingerprint_sha256 === second.run_fingerprint_sha256,
    control.control_id + " owner control is nondeterministic",
  );
  requireCondition(
    canonical(first.reported_diagnostics.map((diagnostic) => diagnostic.code)) ===
      canonical(expectedDiagnosticCodes) &&
      first.emit_diagnostics.length === 0 &&
      !first.emit_skipped,
    control.control_id + " diagnostics or emit result changed: " +
      JSON.stringify({
        reported: first.reported_diagnostics,
        emit: first.emit_diagnostics,
        skipped: first.emit_skipped,
      }),
  );
  requireCondition(
    canonical(first.writes.map((write) => write.path)) ===
      canonical(control.expected_paths),
    control.control_id + " output paths changed",
  );
  return withFingerprint(
    {
      control_id: control.control_id,
      input: {
        current_directory: "/project",
        roots: control.roots,
        files: control.files.map((file) => ({
          path: file.path,
          root: control.roots.includes(file.path),
          ...bytesRecord(file.text),
        })),
        compiler_options: serializeOptions(control.compiler_options),
      },
      repetitions: 2,
      observation: first,
    },
    "control_fingerprint_sha256",
  );
}

function buildArtifact() {
  validateRuntime();
  const controls = CONTROLS.map(buildControl);
  return withFingerprint(
    {
      schema: 1,
      phase: "H2.3d-json-source-owner-controls",
      status: "qualified",
      typescript: {
        version: ts.version,
        source_commit: SOURCE_COMMIT,
        bundle: pathHash(TYPESCRIPT_BUNDLE),
        implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
      },
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      controls,
      summary: {
        controls: controls.length,
        exact_outputs: controls.reduce(
          (sum, control) => sum + control.observation.writes.length,
          0,
        ),
        typescript_runs: controls.reduce(
          (sum, control) => sum + control.repetitions,
          0,
        ),
        reported_diagnostics: controls.reduce(
          (sum, control) =>
            sum + control.observation.reported_diagnostics.length,
          0,
        ),
      },
    },
    "controls_fingerprint_sha256",
  );
}

function render(value) {
  return JSON.stringify(value, null, 2) + "\n";
}

const artifact = buildArtifact();
const rendered = render(artifact);
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
  process.stdout.write(
    "wrote " + TARGET_RELATIVE_PATH +
      ": controls=" + artifact.summary.controls +
      " outputs=" + artifact.summary.exact_outputs + "\n",
  );
} else if (mode === "--check") {
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") === rendered,
    "stale " + TARGET_RELATIVE_PATH +
      "; run h2-3d-owner-controls.mjs --write and review",
  );
  process.stdout.write(
    "H2.3d owner controls are fresh: outputs=" +
      artifact.summary.exact_outputs + "\n",
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-3d-owner-controls.mjs [--write|--check]");
}
