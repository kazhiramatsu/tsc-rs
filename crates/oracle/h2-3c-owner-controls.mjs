import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-3c-owner-controls.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-3c-owner-controls.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-3c-owner-controls.schema.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const UTF8_BOM = Buffer.from([0xef, 0xbb, 0xbf]);

const RUNTIME_TYPES = [
  "declare namespace JSX { interface IntrinsicElements { [name: string]: any; } }",
  "declare module \"react/jsx-runtime\" { export const jsx: any, jsxs: any, Fragment: any; }",
  "declare module \"react/jsx-dev-runtime\" { export const jsxDEV: any, Fragment: any; }",
  "declare module \"react\" { export const createElement: any; }",
  "declare module \"preact/jsx-runtime\" { export const jsx: any, jsxs: any, Fragment: any; }",
  "declare module \"custom/jsx-runtime\" { export const jsx: any, jsxs: any, Fragment: any; }",
  "declare const require: any, exports: any;",
  "",
].join("\n");

const AUTOMATIC_SOURCE = [
  "declare const Comp: any, value: any, props: any, items: any[];",
  "export const view = <><div id=\"a\">{value}</div><Comp {...props}>one<span />{...items}</Comp></>;",
  "",
].join("\n");

const DEVELOPMENT_SOURCE = [
  "declare const Comp: any;",
  "const _jsxDEV = 1, _jsxFileName = 2;",
  "export const view = <><div key=\"k\">x</div><Comp /></>;",
  "",
].join("\n");

const SIMPLE_SOURCE = [
  "export const view = <section data-mode=\"automatic\" />;",
  "",
].join("\n");

const PRAGMA_IMPORT_SOURCE = [
  "/** @jsxImportSource custom */",
  "export const view = <article />;",
  "",
].join("\n");

const CLASSIC_OVERRIDE_SOURCE = [
  "/** @jsxRuntime classic */",
  "/** @jsx Local.h */",
  "/** @jsxFrag Local.Fragment */",
  "declare const Local: any;",
  "export const view = <><main /></>;",
  "",
].join("\n");

const KEY_SOURCE = [
  "declare const Comp: any, props: any;",
  "export const first = <Comp key=\"before\" {...props} />;",
  "export const fallback = <Comp {...props} key=\"after\" />;",
  "export const flattened = <Comp {...{ value: 1 }} key=\"flat\" />;",
  "",
].join("\n");

const COMMONJS_SOURCE = [
  "export const view = <><div /><span /></>;",
  "",
].join("\n");

const COMMONJS_INDICATOR_SOURCE = [
  "exports.view = <div />;",
  "",
].join("\n");

const AUTOMATIC_PRAGMA_SOURCE = [
  "/** @jsxRuntime automatic */",
  "export const view = <aside />;",
  "",
].join("\n");

function options(jsx, extra = {}) {
  return {
    target: ts.ScriptTarget.ESNext,
    module: ts.ModuleKind.Preserve,
    jsx,
    strict: false,
    alwaysStrict: false,
    newLine: ts.NewLineKind.LineFeed,
    ignoreDeprecations: "6.0",
    ...extra,
  };
}

const CONTROLS = Object.freeze([
  {
    control_id: "automatic-esm-helper-dedup-order-fragment-and-static-children",
    roots: ["/project/runtime.d.ts", "/project/automatic.tsx"],
    files: [
      { path: "/project/runtime.d.ts", text: RUNTIME_TYPES },
      { path: "/project/automatic.tsx", text: AUTOMATIC_SOURCE },
    ],
    compiler_options: options(ts.JsxEmit.ReactJSX),
    expected_paths: ["/project/automatic.js"],
  },
  {
    control_id: "development-runtime-source-metadata-utf16-and-name-collisions",
    roots: ["/project/runtime.d.ts", "/project/emoji-😀.tsx"],
    files: [
      { path: "/project/runtime.d.ts", text: RUNTIME_TYPES },
      { path: "/project/emoji-😀.tsx", text: DEVELOPMENT_SOURCE },
    ],
    compiler_options: options(ts.JsxEmit.ReactJSXDev),
    expected_paths: ["/project/emoji-😀.js"],
  },
  {
    control_id: "automatic-jsx-input-option-import-source-and-js-output",
    roots: ["/project/runtime.d.ts", "/project/input.jsx"],
    files: [
      { path: "/project/runtime.d.ts", text: RUNTIME_TYPES },
      { path: "/project/input.jsx", text: SIMPLE_SOURCE },
    ],
    compiler_options: options(ts.JsxEmit.ReactJSX, {
      allowJs: true,
      checkJs: false,
      jsxImportSource: "preact",
      outDir: "/project/dist",
    }),
    expected_paths: ["/project/dist/input.js"],
  },
  {
    control_id: "leading-import-source-pragma-overrides-option",
    roots: ["/project/runtime.d.ts", "/project/pragma.tsx"],
    files: [
      { path: "/project/runtime.d.ts", text: RUNTIME_TYPES },
      { path: "/project/pragma.tsx", text: PRAGMA_IMPORT_SOURCE },
    ],
    compiler_options: options(ts.JsxEmit.ReactJSX, {
      jsxImportSource: "preact",
    }),
    expected_paths: ["/project/pragma.js"],
  },
  {
    control_id: "classic-runtime-pragma-overrides-automatic-mode",
    roots: ["/project/runtime.d.ts", "/project/classic.tsx"],
    files: [
      { path: "/project/runtime.d.ts", text: RUNTIME_TYPES },
      { path: "/project/classic.tsx", text: CLASSIC_OVERRIDE_SOURCE },
    ],
    compiler_options: options(ts.JsxEmit.ReactJSX),
    expected_paths: ["/project/classic.js"],
  },
  {
    control_id: "key-extraction-spread-fallback-and-import-order",
    roots: ["/project/runtime.d.ts", "/project/key.tsx"],
    files: [
      { path: "/project/runtime.d.ts", text: RUNTIME_TYPES },
      { path: "/project/key.tsx", text: KEY_SOURCE },
    ],
    compiler_options: options(ts.JsxEmit.ReactJSX),
    expected_paths: ["/project/key.js"],
  },
  {
    control_id: "commonjs-module-lowers-synthetic-runtime-import-reference",
    roots: ["/project/runtime.d.ts", "/project/commonjs.tsx"],
    files: [
      { path: "/project/runtime.d.ts", text: RUNTIME_TYPES },
      { path: "/project/commonjs.tsx", text: COMMONJS_SOURCE },
    ],
    compiler_options: options(ts.JsxEmit.ReactJSX, {
      module: ts.ModuleKind.CommonJS,
    }),
    expected_paths: ["/project/commonjs.js"],
  },
  {
    control_id: "commonjs-indicator-uses-direct-destructuring-require",
    roots: ["/project/runtime.d.ts", "/project/indicator.jsx"],
    files: [
      { path: "/project/runtime.d.ts", text: RUNTIME_TYPES },
      { path: "/project/indicator.jsx", text: COMMONJS_INDICATOR_SOURCE },
    ],
    compiler_options: options(ts.JsxEmit.ReactJSX, {
      allowJs: true,
      checkJs: false,
      moduleDetection: ts.ModuleDetectionKind.Legacy,
    }),
    expected_paths: ["/project/indicator.js"],
  },
  {
    control_id: "automatic-runtime-pragma-activates-under-classic-jsx-option",
    roots: ["/project/runtime.d.ts", "/project/automatic-pragma.tsx"],
    files: [
      { path: "/project/runtime.d.ts", text: RUNTIME_TYPES },
      { path: "/project/automatic-pragma.tsx", text: AUTOMATIC_PRAGMA_SOURCE },
    ],
    compiler_options: options(ts.JsxEmit.React),
    expected_paths: ["/project/automatic-pragma.js"],
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
  const lower = fileName.toLowerCase();
  if (lower.endsWith(".jsx")) return "jsx";
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
  requireCondition(
    first.run_fingerprint_sha256 === second.run_fingerprint_sha256,
    control.control_id + " owner control is nondeterministic",
  );
  requireCondition(
    first.reported_diagnostics.length === 0 &&
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
  requireCondition(
    first.writes.length === control.expected_paths.length,
    control.control_id + " output count changed",
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
      phase: "H2.3c-automatic-jsx-owner-controls",
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
      "; run h2-3c-owner-controls.mjs --write and review",
  );
  process.stdout.write(
    "H2.3c owner controls are fresh: outputs=" +
      artifact.summary.exact_outputs + "\n",
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-3c-owner-controls.mjs [--write|--check]");
}
