import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-1e-owner-controls.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-1e-owner-controls.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-1e-owner-controls.schema.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const UTF8_BOM = Buffer.from([0xef, 0xbb, 0xbf]);

const MODULE_KINDS = Object.freeze([
  ["Node16(100)", ts.ModuleKind.Node16],
  ["Node18(101)", ts.ModuleKind.Node18],
  ["Node20(102)", ts.ModuleKind.Node20],
  ["NodeNext(199)", ts.ModuleKind.NodeNext],
]);

const EXISTING_MODULE_KINDS = Object.freeze([
  ["CommonJS(1)", ts.ModuleKind.CommonJS],
  ["AMD(2)", ts.ModuleKind.AMD],
  ["UMD(3)", ts.ModuleKind.UMD],
  ["System(4)", ts.ModuleKind.System],
  ["ESNext(99)", ts.ModuleKind.ESNext],
  ["Preserve(200)", ts.ModuleKind.Preserve],
]);

const MIXED_FILES = Object.freeze([
  {
    path: "/project/package.json",
    text: '{"type":"module"}\n',
    root: false,
  },
  {
    path: "/project/input.ts",
    text: [
      "import { value } from './dep.ts';",
      'export { value as renamed } from "./dep.ts";',
      'export * from "./star.mts";',
      "export async function literal() { return import('./lazy.cts'); }",
      'export async function computed(specifier: string) { return import(specifier); }',
      "export const result = value;",
      "",
    ].join("\n"),
    root: true,
  },
  {
    path: "/project/dep.ts",
    text: "export const value = 1;\n",
    root: true,
  },
  {
    path: "/project/star.mts",
    text: "export const star = 2;\n",
    root: true,
  },
  {
    path: "/project/lazy.cts",
    text: "export const lazy = 3;\n",
    root: true,
  },
  {
    path: "/project/cjs/package.json",
    text: '{"type":"commonjs"}\n',
    root: false,
  },
  {
    path: "/project/cjs/input.ts",
    text: [
      'import { value } from "./dep.ts";',
      'export async function literal() { return import("./lazy.mts"); }',
      'export async function computed(specifier: string) { return import(specifier); }',
      "export const result = value;",
      "",
    ].join("\n"),
    root: true,
  },
  {
    path: "/project/cjs/dep.ts",
    text: "export const value = 4;\n",
    root: true,
  },
  {
    path: "/project/cjs/lazy.mts",
    text: "export const lazy = 5;\n",
    root: true,
  },
  {
    path: "/project/explicit.mts",
    text: "export const explicitMts = 6;\n",
    root: true,
  },
  {
    path: "/project/explicit.cts",
    text: "export const explicitCts = 7;\n",
    root: true,
  },
]);

const ATTRIBUTE_FILES = Object.freeze([
  {
    path: "/attributes/package.json",
    text: '{"type":"module"}\n',
    root: false,
  },
  {
    path: "/attributes/input.ts",
    text: [
      'import { value } from "./dep.ts" with { type: "javascript" };',
      'export { value as renamed } from "./dep.ts" with { type: "javascript" };',
      'export * from "./star.mts" with {',
      '  type: "javascript"',
      '};',
      'export async function load() { return import("./lazy.cts", { with: { type: "javascript" } }); }',
      "export const result = value;",
      "",
    ].join("\n"),
    root: true,
  },
  {
    path: "/attributes/dep.ts",
    text: "export const value = 8;\n",
    root: true,
  },
  {
    path: "/attributes/star.mts",
    text: "export const star = 9;\n",
    root: true,
  },
  {
    path: "/attributes/lazy.cts",
    text: "export const lazy = 10;\n",
    root: true,
  },
]);

const FRESH_SOURCE_PATH = "/Fresh/Entry.ts";
const FRESH_SOURCE = 'export const format = "fresh";\n';
const EXISTING_MODULE_REWRITE_FILES = Object.freeze([
  {
    path: "/rewrite/input.ts",
    text: [
      'const specifier = "./runtime.ts";',
      "export const loaded = import(specifier);",
      "",
    ].join("\n"),
    root: true,
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

function createVirtualProgram(files, roots, options, currentDirectory) {
  const fileMap = new Map(
    files.map((file) => [ts.normalizePath(file.path), file.text]),
  );
  const baseHost = ts.createCompilerHost(options, true);
  const host = {
    ...baseHost,
    getCurrentDirectory: () => currentDirectory,
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
  return ts.createProgram(roots, options, host);
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
  if (lower.endsWith(".map")) return "source-map";
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
    source_files: (sourceFiles ?? []).map((source) => ts.normalizePath(source.fileName)),
  };
}

function impliedFormatName(value) {
  if (value === ts.ModuleKind.ESNext) return "ESModule(99)";
  if (value === ts.ModuleKind.CommonJS) return "CommonJS(1)";
  return "None";
}

function observe(files, roots, options, currentDirectory) {
  const program = createVirtualProgram(files, roots, options, currentDirectory);
  const rootFacts = roots.map((root) => {
    const sourceFile = program.getSourceFile(root);
    requireCondition(sourceFile !== undefined, `missing root ${root}`);
    return {
      path: sourceFile.fileName,
      implied_module_format: impliedFormatName(sourceFile.impliedNodeFormat),
      emit_module_format: program.getEmitModuleFormatOfFile(sourceFile),
    };
  });
  const writes = [];
  const preEmitDiagnostics = ts.getPreEmitDiagnostics(program);
  const emitResult = program.emit(
    undefined,
    (...arguments_) => writes.push(arguments_),
  );
  return withFingerprint(
    {
      root_facts: rootFacts,
      diagnostics: preEmitDiagnostics.map(serializeDiagnostic),
      emit_diagnostics: emitResult.diagnostics.map(serializeDiagnostic),
      emit_skipped: emitResult.emitSkipped,
      writes: writes.map(serializeWrite),
    },
    "run_fingerprint_sha256",
  );
}

function nodeOptions(moduleKind) {
  return {
    target: ts.ScriptTarget.ESNext,
    module: moduleKind,
    rewriteRelativeImportExtensions: true,
    newLine: ts.NewLineKind.LineFeed,
    ignoreDeprecations: "6.0",
  };
}

function buildAttributeControl() {
  const roots = ATTRIBUTE_FILES.filter((file) => file.root).map((file) => file.path);
  const runs = MODULE_KINDS.filter(([, moduleValue]) => moduleValue !== ts.ModuleKind.Node16)
    .map(([moduleState, moduleValue]) => {
      const repeated = repeatedObservation(
        () => observe(ATTRIBUTE_FILES, roots, nodeOptions(moduleValue), "/attributes"),
        `${moduleState} import-attributes owner control is nondeterministic`,
      );
      requireCondition(
        repeated.observation.diagnostics.length === 0 &&
          repeated.observation.emit_diagnostics.length === 0,
        `${moduleState} import-attributes owner control gained diagnostics: ${canonical(repeated.observation.diagnostics)}`,
      );
      return {
        module_state: moduleState,
        module_value: moduleValue,
        ...repeated,
      };
    });
  requireCondition(
    new Set(runs.map((run) => run.observation.run_fingerprint_sha256)).size === 1,
    "Node18/20/Next import-attributes emits diverged",
  );
  return withFingerprint(
    {
      control_id: "node-format-import-attributes-owner-closure",
      input: {
        current_directory: "/attributes",
        roots,
        files: ATTRIBUTE_FILES.map((file) => ({
          path: file.path,
          root: file.root,
          ...bytesRecord(file.text),
        })),
        target: "ESNext(99)",
        modules: runs.map((run) => run.module_state),
        node16_disposition: "diagnostic-2823-not-an-emission-owner-control",
        rewrite_relative_import_extensions: true,
        ignore_deprecations: "6.0",
      },
      runs,
    },
    "control_fingerprint_sha256",
  );
}

function repeatedObservation(makeRun, message) {
  const first = makeRun();
  const second = makeRun();
  requireCondition(
    first.run_fingerprint_sha256 === second.run_fingerprint_sha256,
    message,
  );
  return { repetitions: 2, observation: first };
}

function buildMixedControl() {
  const roots = MIXED_FILES.filter((file) => file.root).map((file) => file.path);
  const runs = MODULE_KINDS.map(([moduleState, moduleValue]) => {
    const repeated = repeatedObservation(
      () => observe(MIXED_FILES, roots, nodeOptions(moduleValue), "/project"),
      `${moduleState} mixed-project owner control is nondeterministic`,
    );
    requireCondition(
      repeated.observation.diagnostics.length === 0 &&
        repeated.observation.emit_diagnostics.length === 0,
      `${moduleState} mixed-project owner control gained diagnostics: ${canonical(repeated.observation.diagnostics)}`,
    );
    return {
      module_state: moduleState,
      module_value: moduleValue,
      ...repeated,
    };
  });
  requireCondition(
    new Set(runs.map((run) => run.observation.run_fingerprint_sha256)).size === 1,
    "Node16/18/20/Next mixed-project emits diverged",
  );
  return withFingerprint(
    {
      control_id: "node-format-mixed-project-owner-closure",
      input: {
        current_directory: "/project",
        roots,
        files: MIXED_FILES.map((file) => ({
          path: file.path,
          root: file.root,
          ...bytesRecord(file.text),
        })),
        target: "ESNext(99)",
        modules: MODULE_KINDS.map(([state]) => state),
        rewrite_relative_import_extensions: true,
        ignore_deprecations: "6.0",
      },
      runs,
    },
    "control_fingerprint_sha256",
  );
}

function buildFreshPackageControl() {
  const variants = [
    ["module", "ESModule(99)"],
    ["commonjs", "CommonJS(1)"],
  ].map(([packageType, expectedFormat]) => {
    const files = [
      {
        path: "/Fresh/package.json",
        text: `${JSON.stringify({ type: packageType })}\n`,
      },
      { path: FRESH_SOURCE_PATH, text: FRESH_SOURCE },
    ];
    const repeated = repeatedObservation(
      () =>
        observe(
          files,
          [FRESH_SOURCE_PATH],
          nodeOptions(ts.ModuleKind.NodeNext),
          "/Fresh",
        ),
      `${packageType} fresh-package owner control is nondeterministic`,
    );
    requireCondition(
      repeated.observation.diagnostics.length === 0 &&
        repeated.observation.emit_diagnostics.length === 0,
      `${packageType} fresh-package owner control gained diagnostics`,
    );
    requireCondition(
      repeated.observation.root_facts[0].implied_module_format === expectedFormat,
      `${packageType} package type did not own the implied format`,
    );
    return {
      package_type: packageType,
      package_json: bytesRecord(files[0].text),
      expected_implied_module_format: expectedFormat,
      ...repeated,
    };
  });
  requireCondition(
    variants[0].observation.writes[0].path === "/Fresh/Entry.js" &&
      variants[1].observation.writes[0].path === "/Fresh/Entry.js",
    "fresh-package path casing changed",
  );
  requireCondition(
    variants[0].observation.writes[0].callback_utf8_sha256 !==
      variants[1].observation.writes[0].callback_utf8_sha256,
    "fresh package type did not change the output format",
  );
  return withFingerprint(
    {
      control_id: "node-format-fresh-package-and-path-casing",
      input: {
        current_directory: "/Fresh",
        root: FRESH_SOURCE_PATH,
        source: bytesRecord(FRESH_SOURCE),
        target: "ESNext(99)",
        module: "NodeNext(199)",
        rewrite_relative_import_extensions: true,
        ignore_deprecations: "6.0",
      },
      variants,
    },
    "control_fingerprint_sha256",
  );
}

function buildExistingModuleRewriteControl() {
  const roots = EXISTING_MODULE_REWRITE_FILES.filter((file) => file.root)
    .map((file) => file.path);
  const runs = EXISTING_MODULE_KINDS.map(([moduleState, moduleValue]) => {
    const repeated = repeatedObservation(
      () =>
        observe(
          EXISTING_MODULE_REWRITE_FILES,
          roots,
          nodeOptions(moduleValue),
          "/rewrite",
        ),
      `${moduleState} relative-extension owner control is nondeterministic`,
    );
    requireCondition(
      repeated.observation.diagnostics.length === 0 &&
        repeated.observation.emit_diagnostics.length === 0,
      `${moduleState} relative-extension owner control gained diagnostics: ${canonical(repeated.observation.diagnostics)}`,
    );
    return {
      module_state: moduleState,
      module_value: moduleValue,
      ...repeated,
    };
  });
  return withFingerprint(
    {
      control_id: "relative-extension-existing-module-owner-closure",
      input: {
        current_directory: "/rewrite",
        roots,
        files: EXISTING_MODULE_REWRITE_FILES.map((file) => ({
          path: file.path,
          root: file.root,
          ...bytesRecord(file.text),
        })),
        target: "ESNext(99)",
        modules: EXISTING_MODULE_KINDS.map(([state]) => state),
        system_disposition: "typescript-does-not-rewrite-system-module-imports",
        rewrite_relative_import_extensions: true,
        ignore_deprecations: "6.0",
      },
      runs,
    },
    "control_fingerprint_sha256",
  );
}

function buildArtifact() {
  validateRuntime();
  const controls = [
    buildMixedControl(),
    buildAttributeControl(),
    buildFreshPackageControl(),
    buildExistingModuleRewriteControl(),
  ];
  const exactOutputs = controls.reduce((sum, control) => {
    if ("runs" in control) {
      return sum + control.runs.reduce(
        (subtotal, run) => subtotal + run.observation.writes.length,
        0,
      );
    }
    return sum + control.variants.reduce(
      (subtotal, variant) => subtotal + variant.observation.writes.length,
      0,
    );
  }, 0);
  return withFingerprint(
    {
      schema: 1,
      phase: "H2.1e-node-format-owner-controls",
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
        exact_outputs: exactOutputs,
        typescript_runs: controls.reduce((sum, control) => {
          const observations = "runs" in control ? control.runs : control.variants;
          return sum + observations.reduce(
            (subtotal, observation) => subtotal + observation.repetitions,
            0,
          );
        }, 0),
        diagnostics: 0,
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
    `wrote ${TARGET_RELATIVE_PATH}: controls=${artifact.summary.controls} outputs=${artifact.summary.exact_outputs}\n`,
  );
} else if (mode === "--check") {
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-1e-owner-controls.mjs --write and review`,
  );
  process.stdout.write(
    `H2.1e owner controls are fresh: outputs=${artifact.summary.exact_outputs}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-1e-owner-controls.mjs [--write|--check]");
}
