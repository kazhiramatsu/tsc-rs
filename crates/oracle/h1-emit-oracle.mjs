import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import { serializeMessageChainBounded } from "./m9-observation.mjs";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const PROFILE_RELATIVE_PATH = "ratchets/h1-emit-profile.v1.json";
const OBSERVATION_RELATIVE_PATH = "ratchets/h1-emit-oracle.v1.json";
const PROFILE_PATH = path.join(WORKSPACE, PROFILE_RELATIVE_PATH);
const OBSERVATION_PATH = path.join(WORKSPACE, OBSERVATION_RELATIVE_PATH);
const PROFILE_SCHEMA_RELATIVE_PATH =
  ".github/ci/contracts/h1-emit-profile.schema.json";
const OBSERVATION_SCHEMA_RELATIVE_PATH =
  ".github/ci/contracts/h1-emit-observation.schema.json";
const TYPESCRIPT_RELATIVE_PATH =
  "vendor/typescript-6.0.3/lib/typescript.js";
const TSC_RELATIVE_PATH = "vendor/typescript-6.0.3/lib/_tsc.js";
const H0_PROFILE_RELATIVE_PATH = "ratchets/h0-qualification.v1.json";
const OWNER_INVENTORY_RELATIVE_PATH = "ratchets/h1-owner-inventory.v1.json";

const EXPECTED_NODE_VERSION = "25.2.1";
const EXPECTED_TYPESCRIPT_VERSION = "6.0.3";
const EXPECTED_SOURCE_COMMIT =
  "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_TYPESCRIPT_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const EXPECTED_TSC_SHA256 =
  "1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3";
const VIRTUAL_ROOT = "/__h1";
const PROJECT_ROOT = `${VIRTUAL_ROOT}/project`;
const LIB_FILE = `${VIRTUAL_ROOT}/lib.d.ts`;
const UTF8_BOM = Buffer.from([0xef, 0xbb, 0xbf]);

const ORACLE_LIBRARY = [
  "interface Array<T> { readonly length: number; [n: number]: T; }",
  "interface Boolean {}",
  "interface CallableFunction extends Function {}",
  "interface Function {}",
  "interface IArguments { readonly length: number; [n: number]: any; }",
  "interface NewableFunction extends Function {}",
  "interface Number {}",
  "interface Object {}",
  "interface RegExp {}",
  "interface String {}",
  "",
].join("\n");

const BASE_OPTIONS = Object.freeze({
  target: ts.ScriptTarget.ESNext,
  module: ts.ModuleKind.Preserve,
  useDefineForClassFields: true,
  noLib: true,
  listEmittedFiles: true,
  newLine: ts.NewLineKind.LineFeed,
});

const fixtures = [
  {
    id: "erasable-typescript",
    classification: "admitted",
    options: {},
    files: {
      "src/main.ts": [
        "export interface Shape { value: number }",
        "export type Boxed<T> = { value: T };",
        "export const answer: number = 41 as number;",
        "export function inc(value: number): number { return value + 1; }",
        "export class Box<T> {",
        "    readonly value: T;",
        "    constructor(value: T) { this.value = value; }",
        "    get(): T { return this.value; }",
        "}",
        "export const boxed = new Box(answer satisfies number);",
        "",
      ].join("\n"),
    },
  },
  {
    id: "ordered-multi-file-bom-crlf",
    classification: "admitted",
    options: {
      emitBOM: true,
      newLine: ts.NewLineKind.CarriageReturnLineFeed,
    },
    files: {
      "src/zeta.ts": "export const zeta: string = \"雪\";\n",
      "src/alpha.ts": "export const alpha: number = 1;\n",
    },
  },
  {
    id: "diagnostics-with-output",
    classification: "admitted",
    options: { noEmitOnError: false },
    files: {
      "src/error.ts": "export const count: number = \"not a number\";\n",
    },
  },
  {
    id: "no-emit-on-error",
    classification: "admitted",
    options: { noEmitOnError: true },
    files: {
      "src/error.ts": "export const count: number = \"not a number\";\n",
    },
  },
  {
    id: "emitted-files-observation-absent",
    classification: "admitted",
    options: { listEmittedFiles: false },
    files: {
      "src/absent.ts": "export const active: boolean = true;\n",
    },
  },
  {
    id: "runtime-enum-control",
    classification: "adjacent-unsupported",
    expectedReasons: ["syntax.runtime-enum"],
    options: {},
    files: {
      "src/enum.ts": "export enum Direction { Up, Down }\n",
    },
  },
  {
    id: "runtime-namespace-control",
    classification: "adjacent-unsupported",
    expectedReasons: ["syntax.runtime-namespace"],
    options: {},
    files: {
      "src/namespace.ts":
        "export namespace Runtime { export const value: number = 1; }\n",
    },
  },
  {
    id: "parameter-property-control",
    classification: "adjacent-unsupported",
    expectedReasons: ["syntax.parameter-property"],
    options: {},
    files: {
      "src/parameter-property.ts":
        "export class Service { constructor(public value: number) {} }\n",
    },
  },
  {
    id: "jsx-control",
    classification: "adjacent-unsupported",
    expectedReasons: ["extension.tsx", "option.jsx", "syntax.jsx"],
    options: { jsx: ts.JsxEmit.Preserve },
    files: {
      "src/view.tsx": "export const view = <div />;\n",
    },
  },
  {
    id: "source-map-control",
    classification: "adjacent-unsupported",
    expectedReasons: ["option.sourceMap"],
    options: { sourceMap: true },
    files: {
      "src/mapped.ts": "export const mapped: number = 1;\n",
    },
  },
  {
    id: "declaration-control",
    classification: "adjacent-unsupported",
    expectedReasons: ["option.declaration"],
    options: { declaration: true },
    files: {
      "src/declaration.ts": "export const declared: number = 1;\n",
    },
  },
  {
    id: "mts-output-control",
    classification: "adjacent-unsupported",
    expectedReasons: ["extension.mts"],
    options: {},
    files: {
      "src/module.mts": "export const moduleValue: number = 1;\n",
    },
  },
];

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}

function exactKeys(value, required, optional = []) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const allowed = new Set([...required, ...optional]);
  return (
    required.every((key) => Object.hasOwn(value, key)) &&
    Object.keys(value).every((key) => allowed.has(key))
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

function fileSha256(relativePath) {
  return sha256(fs.readFileSync(path.join(WORKSPACE, relativePath)));
}

function validateRuntime() {
  const nodePin = fs.readFileSync(path.join(WORKSPACE, ".node-version"), "utf8").trim();
  requireCondition(nodePin === EXPECTED_NODE_VERSION, "unexpected .node-version pin");
  requireCondition(
    process.version === `v${nodePin}`,
    `H1 emit oracle requires Node ${nodePin}; running ${process.version}`,
  );
  requireCondition(
    ts.version === EXPECTED_TYPESCRIPT_VERSION,
    `unexpected TypeScript runtime ${ts.version}`,
  );
  requireCondition(
    fileSha256(TYPESCRIPT_RELATIVE_PATH) === EXPECTED_TYPESCRIPT_SHA256,
    "vendored typescript.js differs from the reviewed TypeScript 6.0.3 pin",
  );
  requireCondition(
    fileSha256(TSC_RELATIVE_PATH) === EXPECTED_TSC_SHA256,
    "vendored _tsc.js differs from the reviewed TypeScript 6.0.3 pin",
  );
  requireCondition(
    typeof ts.emitFilesAndReportErrorsAndGetExitStatus === "function",
    "vendored callback/exit orchestration export is unavailable",
  );
  requireCondition(
    typeof ts.getTransformers === "function",
    "vendored transformer-selection export is unavailable",
  );
}

function normalizeVirtualPath(fileName) {
  const normalized = path.posix.normalize(fileName.replaceAll("\\", "/"));
  if (normalized === LIB_FILE) return "/__oracle__/lib.d.ts";
  if (normalized === PROJECT_ROOT) return "/project";
  if (normalized.startsWith(`${PROJECT_ROOT}/`)) {
    return `/project/${normalized.slice(PROJECT_ROOT.length + 1)}`;
  }
  throw new Error(`path escaped the H1 virtual root: ${fileName}`);
}

function normalizeObservableText(text) {
  return text
    .replaceAll(PROJECT_ROOT, "/project")
    .replaceAll(LIB_FILE, "/__oracle__/lib.d.ts");
}

function optionalValue(value) {
  return value === undefined
    ? { present: false, value: null }
    : { present: true, value };
}

function diagnosticCategory(category, context) {
  const names = ["warning", "error", "suggestion", "message"];
  requireCondition(
    Number.isInteger(category) && category >= 0 && category < names.length,
    `${context} has unknown diagnostic category ${category}`,
  );
  return names[category];
}

function serializeRelatedDiagnostic(diagnostic, context) {
  const file = diagnostic.file
    ? normalizeVirtualPath(diagnostic.file.fileName)
    : null;
  const start = optionalValue(diagnostic.start);
  const length = optionalValue(diagnostic.length);
  requireCondition(
    start.present === length.present,
    `${context} diagnostic span has mismatched presence`,
  );
  return {
    file: optionalValue(file === null ? undefined : file),
    start,
    length,
    code: diagnostic.code,
    category: diagnosticCategory(diagnostic.category, context),
    chain: serializeMessageChainBounded(
      diagnostic.messageText,
      diagnostic.code,
      diagnostic.category,
      `${context}.chain`,
    ),
  };
}

function serializeDiagnostic(diagnostic, context) {
  requireCondition(
    diagnostic !== null && typeof diagnostic === "object" && !Array.isArray(diagnostic),
    `${context} is not a diagnostic object`,
  );
  const file = diagnostic.file
    ? normalizeVirtualPath(diagnostic.file.fileName)
    : null;
  const start = optionalValue(diagnostic.start);
  const length = optionalValue(diagnostic.length);
  requireCondition(
    start.present === length.present,
    `${context} diagnostic span has mismatched presence`,
  );
  let line = optionalValue(undefined);
  let column = optionalValue(undefined);
  if (diagnostic.file && diagnostic.start !== undefined) {
    const location = diagnostic.file.getLineAndCharacterOfPosition(diagnostic.start);
    line = optionalValue(location.line);
    column = optionalValue(location.character);
  }
  const related = diagnostic.relatedInformation;
  requireCondition(
    related === undefined || Array.isArray(related),
    `${context}.relatedInformation is not an array`,
  );
  return {
    file: optionalValue(file === null ? undefined : file),
    start,
    length,
    line,
    column,
    code: diagnostic.code,
    category: diagnosticCategory(diagnostic.category, context),
    chain: serializeMessageChainBounded(
      diagnostic.messageText,
      diagnostic.code,
      diagnostic.category,
      `${context}.chain`,
    ),
    related_information_present: related !== undefined,
    related: (related ?? []).map((entry, index) =>
      serializeRelatedDiagnostic(entry, `${context}.related[${index}]`),
    ),
    reports_unnecessary: optionalValue(diagnostic.reportsUnnecessary),
    reports_deprecated: optionalValue(diagnostic.reportsDeprecated),
    source: optionalValue(diagnostic.source),
  };
}

function outputKind(fileName) {
  if (fileName.endsWith(".d.ts") || fileName.endsWith(".d.mts") || fileName.endsWith(".d.cts")) {
    return "declaration";
  }
  if (fileName.endsWith(".map")) return "source-map";
  if (fileName.endsWith(".tsbuildinfo")) return "build-info";
  if ([".js", ".jsx", ".mjs", ".cjs"].some((extension) => fileName.endsWith(extension))) {
    return "javascript";
  }
  throw new Error(`unknown emitted output kind for ${fileName}`);
}

function normalizeMetadata(data, context) {
  if (data === undefined) {
    return { present: false, value: null };
  }
  requireCondition(
    data !== null && typeof data === "object" && !Array.isArray(data),
    `${context} metadata is not an object`,
  );
  const known = new Set([
    "sourceMapUrlPos",
    "diagnostics",
    "skippedDtsWrite",
    "differsOnlyInMap",
    "buildInfo",
  ]);
  const ownKeys = Reflect.ownKeys(data);
  requireCondition(
    ownKeys.every((key) => typeof key === "string" && known.has(key)),
    `${context} metadata contains an unknown key`,
  );
  const diagnostics = data.diagnostics;
  requireCondition(
    diagnostics === undefined || Array.isArray(diagnostics),
    `${context}.diagnostics is not an array`,
  );
  let buildInfo = optionalValue(data.buildInfo);
  if (buildInfo.present) {
    buildInfo = {
      present: true,
      value: JSON.parse(JSON.stringify(buildInfo.value)),
    };
  }
  return {
    present: true,
    value: {
      own_keys: ownKeys.map(String).sort(),
      source_map_url_position_utf16: optionalValue(data.sourceMapUrlPos),
      diagnostics_present: diagnostics !== undefined,
      diagnostics: (diagnostics ?? []).map((diagnostic, index) =>
        serializeDiagnostic(diagnostic, `${context}.diagnostics[${index}]`),
      ),
      skipped_dts_write: optionalValue(data.skippedDtsWrite),
      differs_only_in_map: optionalValue(data.differsOnlyInMap),
      build_info: buildInfo,
    },
  };
}

function serializeWrite(arguments_, index) {
  const [fileName, text, writeByteOrderMark, onError, sourceFiles, data] = arguments_;
  requireCondition(typeof fileName === "string", `write ${index} has no path`);
  requireCondition(typeof text === "string", `write ${index} has no text`);
  requireCondition(
    typeof writeByteOrderMark === "boolean",
    `write ${index} has no BOM decision`,
  );
  requireCondition(
    onError === undefined || typeof onError === "function",
    `write ${index} has invalid onError callback`,
  );
  requireCondition(
    sourceFiles === undefined || Array.isArray(sourceFiles),
    `write ${index} has invalid source provenance`,
  );
  const callbackBytes = Buffer.from(text, "utf8");
  const materializedBytes = writeByteOrderMark
    ? Buffer.concat([UTF8_BOM, callbackBytes])
    : callbackBytes;
  return {
    index,
    path: normalizeVirtualPath(fileName),
    kind: outputKind(fileName),
    callback_text: text,
    callback_utf8_base64: callbackBytes.toString("base64"),
    callback_utf8_sha256: sha256(callbackBytes),
    callback_utf8_bytes: callbackBytes.length,
    write_byte_order_mark: writeByteOrderMark,
    materialized_utf8_base64: materializedBytes.toString("base64"),
    materialized_utf8_sha256: sha256(materializedBytes),
    materialized_utf8_bytes: materializedBytes.length,
    on_error_callback_present: onError !== undefined,
    source_files_present: sourceFiles !== undefined,
    source_files: (sourceFiles ?? []).map((sourceFile) =>
      normalizeVirtualPath(sourceFile.fileName),
    ),
    metadata: normalizeMetadata(data, `writes[${index}]`),
    sink_disposition: "written",
  };
}

function normalizeSourceMapObservation(observation, context) {
  requireCondition(
    exactKeys(observation, ["inputSourceFileNames", "sourceMap"]),
    `${context} has an unknown source-map shape`,
  );
  requireCondition(
    Array.isArray(observation.inputSourceFileNames),
    `${context}.inputSourceFileNames is not an array`,
  );
  return {
    input_source_file_names: observation.inputSourceFileNames.map((fileName) =>
      normalizeVirtualPath(fileName),
    ),
    source_map: JSON.parse(JSON.stringify(observation.sourceMap)),
  };
}

function createCompilerHost(fileMap, options) {
  const host = ts.createCompilerHost(options, true);
  return {
    ...host,
    getCurrentDirectory: () => PROJECT_ROOT,
    getDefaultLibFileName: () => LIB_FILE,
    getCanonicalFileName: (fileName) => fileName,
    useCaseSensitiveFileNames: () => true,
    getNewLine: () =>
      options.newLine === ts.NewLineKind.CarriageReturnLineFeed ? "\r\n" : "\n",
    fileExists: (fileName) => fileMap.has(fileName),
    readFile: (fileName) => fileMap.get(fileName),
    directoryExists: (directoryName) =>
      [...fileMap.keys()].some((fileName) => fileName.startsWith(`${directoryName}/`)),
    getDirectories: () => [],
    realpath: (fileName) => fileName,
    getSourceFile(fileName, languageVersion) {
      const text = fileMap.get(fileName);
      if (text === undefined) return undefined;
      return ts.createSourceFile(fileName, text, languageVersion, true);
    },
    writeFile() {
      throw new Error("H1 oracle host writeFile reached without the callback capture");
    },
  };
}

function fixtureFileMap(fixture) {
  const fileMap = new Map([[LIB_FILE, ORACLE_LIBRARY]]);
  for (const [relativeName, text] of Object.entries(fixture.files)) {
    requireCondition(
      !path.posix.isAbsolute(relativeName) && !relativeName.split("/").includes(".."),
      `${fixture.id} has an invalid relative file name`,
    );
    fileMap.set(`${PROJECT_ROOT}/${relativeName}`, text);
  }
  return fileMap;
}

function hasModifier(node, kind) {
  return node.modifiers?.some((modifier) => modifier.kind === kind) ?? false;
}

function profileGate(fixture, fileMap, options) {
  const reasons = new Set();
  if (options.target !== ts.ScriptTarget.ESNext) reasons.add("option.target");
  if (options.module !== ts.ModuleKind.Preserve) reasons.add("option.module");
  if (options.useDefineForClassFields === false) {
    reasons.add("option.useDefineForClassFields");
  }
  if (options.noEmit === true) reasons.add("route.h0-no-emit");
  for (const option of [
    "sourceMap",
    "inlineSourceMap",
    "declaration",
    "declarationMap",
    "emitDeclarationOnly",
    "incremental",
    "composite",
    "allowJs",
    "experimentalDecorators",
    "importHelpers",
    "noEmitHelpers",
    "noCheck",
    "isolatedModules",
    "verbatimModuleSyntax",
    "allowImportingTsExtensions",
    "rewriteRelativeImportExtensions",
    "resolveJsonModule",
  ]) {
    if (options[option] === true) reasons.add(`option.${option}`);
  }
  for (const option of ["outFile", "outDir", "declarationDir", "tsBuildInfoFile"]) {
    if (options[option] !== undefined) reasons.add(`option.${option}`);
  }
  if (options.jsx !== undefined) reasons.add("option.jsx");
  if (fixture.customTransformers === true) reasons.add("request.custom-transformers");

  for (const [fileName, text] of fileMap) {
    if (fileName === LIB_FILE) continue;
    const extension = path.posix.extname(fileName).slice(1).toLowerCase();
    if (extension !== "ts") reasons.add(`extension.${extension}`);
    const sourceFile = ts.createSourceFile(fileName, text, options.target, true);
    function visit(node) {
      if (ts.isEnumDeclaration(node)) reasons.add("syntax.runtime-enum");
      if (ts.isModuleDeclaration(node)) reasons.add("syntax.runtime-namespace");
      if (ts.isImportEqualsDeclaration(node)) reasons.add("syntax.import-equals");
      if (ts.isExportAssignment(node) && node.isExportEquals) {
        reasons.add("syntax.export-equals");
      }
      if (
        ts.isParameter(node) &&
        [
          ts.SyntaxKind.PublicKeyword,
          ts.SyntaxKind.PrivateKeyword,
          ts.SyntaxKind.ProtectedKeyword,
          ts.SyntaxKind.ReadonlyKeyword,
          ts.SyntaxKind.OverrideKeyword,
        ].some((kind) => hasModifier(node, kind))
      ) {
        reasons.add("syntax.parameter-property");
      }
      if (
        node.kind === ts.SyntaxKind.JsxElement ||
        node.kind === ts.SyntaxKind.JsxSelfClosingElement ||
        node.kind === ts.SyntaxKind.JsxFragment
      ) {
        reasons.add("syntax.jsx");
      }
      if (ts.canHaveDecorators(node) && (ts.getDecorators(node)?.length ?? 0) > 0) {
        reasons.add("syntax.decorators");
      }
      ts.forEachChild(node, visit);
    }
    visit(sourceFile);
  }
  const orderedReasons = [...reasons].sort();
  return {
    disposition:
      orderedReasons.length === 0
        ? "admitted"
        : orderedReasons.length === 1 && orderedReasons[0] === "route.h0-no-emit"
          ? "h0-no-emit"
          : "unsupported-before-first-write",
    reasons: orderedReasons,
  };
}

function observeFixture(fixture) {
  const options = { ...BASE_OPTIONS, ...fixture.options };
  const fileMap = fixtureFileMap(fixture);
  const rootNames = [
    ...Object.keys(fixture.files).map((relativeName) =>
      `${PROJECT_ROOT}/${relativeName}`,
    ),
    LIB_FILE,
  ];
  const host = createCompilerHost(fileMap, options);
  const program = ts.createProgram({ rootNames, options, host });
  const gate = profileGate(fixture, fileMap, options);
  const writes = [];
  const diagnostics = [];
  const statusWrites = [];
  let emitResult;
  const originalEmit = program.emit;
  program.emit = function captureEmitResult(...arguments_) {
    requireCondition(emitResult === undefined, `${fixture.id} emitted more than once`);
    emitResult = originalEmit.apply(this, arguments_);
    return emitResult;
  };
  const exitStatus = ts.emitFilesAndReportErrorsAndGetExitStatus(
    program,
    (diagnostic) => diagnostics.push(diagnostic),
    (text) => statusWrites.push(normalizeObservableText(text)),
    undefined,
    (...arguments_) => writes.push(arguments_),
  );
  requireCondition(emitResult !== undefined, `${fixture.id} did not produce an EmitResult`);
  const emittedFilesPresent = emitResult.emittedFiles !== undefined;
  const sourceMapsPresent = emitResult.sourceMaps !== undefined;
  const exitNames = [
    "success",
    "diagnostics-present-outputs-skipped",
    "diagnostics-present-outputs-generated",
  ];
  requireCondition(exitNames[exitStatus] !== undefined, `${fixture.id} has unknown exit status`);
  const observation = {
    profile_gate: gate,
    rust_expectation: {
      outcome:
        gate.disposition === "admitted"
          ? "exact-typescript-observation"
          : gate.disposition === "h0-no-emit"
            ? "existing-h0-route"
            : "typed-unsupported-before-first-write",
      sink_write_count:
        gate.disposition === "admitted" ? writes.length : 0,
    },
    writes: writes.map(serializeWrite),
    reported_diagnostics: diagnostics.map((diagnostic, index) =>
      serializeDiagnostic(diagnostic, `reported_diagnostics[${index}]`),
    ),
    emit_result: {
      emit_skipped: emitResult.emitSkipped,
      emit_diagnostics: emitResult.diagnostics.map((diagnostic, index) =>
        serializeDiagnostic(diagnostic, `emit_diagnostics[${index}]`),
      ),
      emitted_files_present: emittedFilesPresent,
      emitted_files: (emitResult.emittedFiles ?? []).map(normalizeVirtualPath),
      source_maps_present: sourceMapsPresent,
      source_maps: (emitResult.sourceMaps ?? []).map((entry, index) =>
        normalizeSourceMapObservation(entry, `source_maps[${index}]`),
      ),
    },
    status_writes: statusWrites,
    process_exit: { code: exitStatus, name: exitNames[exitStatus] },
  };
  return observation;
}

function fixtureInput(fixture) {
  return {
    id: fixture.id,
    classification: fixture.classification,
    compiler_options: Object.fromEntries(
      Object.entries({ ...BASE_OPTIONS, ...fixture.options }).sort(([left], [right]) =>
        left.localeCompare(right),
      ),
    ),
    root_files: Object.entries(fixture.files).map(([relativeName, text]) => ({
      path: `/project/${relativeName}`,
      utf8_base64: Buffer.from(text, "utf8").toString("base64"),
      utf8_sha256: sha256(Buffer.from(text, "utf8")),
      utf8_bytes: Buffer.byteLength(text, "utf8"),
    })),
  };
}

function buildProfile() {
  const transformerNames = ts
    .getTransformers(BASE_OPTIONS, undefined, false)
    .scriptTransformers.map((transformer) => transformer.name);
  requireCondition(
    JSON.stringify(transformerNames) ===
      JSON.stringify([
        "transformTypeScript",
        "transformClassFields",
        "transformECMAScriptModule",
      ]),
    `bootstrap transformer order drifted: ${transformerNames.join(", ")}`,
  );
  const h0Profile = JSON.parse(
    fs.readFileSync(path.join(WORKSPACE, H0_PROFILE_RELATIVE_PATH), "utf8"),
  );
  const narrowedH0Options = new Set([
    "allowImportingTsExtensions",
    "allowJs",
    "declarationDir",
    "experimentalDecorators",
    "importHelpers",
    "isolatedModules",
    "jsx",
    "module",
    "noEmit",
    "outDir",
    "resolveJsonModule",
    "rewriteRelativeImportExtensions",
    "target",
    "useDefineForClassFields",
    "verbatimModuleSyntax",
  ]);
  const retainedH0Options = h0Profile.option_profile.config_options.filter(
    (name) => !narrowedH0Options.has(name),
  );
  requireCondition(
    [...narrowedH0Options].every((name) =>
      h0Profile.option_profile.config_options.includes(name),
    ),
    "H1 narrowed-option inventory contains an option outside the frozen H0 profile",
  );
  requireCondition(
    retainedH0Options.length + narrowedH0Options.size ===
      h0Profile.option_profile.config_options.length,
    "H1 emit option inventory does not partition the frozen H0 option profile",
  );
  const profile = {
    schema: 1,
    status: "frozen",
    phase: "H1.0a-bootstrap-profile",
    typescript: {
      version: EXPECTED_TYPESCRIPT_VERSION,
      source_commit: EXPECTED_SOURCE_COMMIT,
      implementation: TSC_RELATIVE_PATH,
      implementation_sha256: EXPECTED_TSC_SHA256,
      oracle_bundle: TYPESCRIPT_RELATIVE_PATH,
      oracle_bundle_sha256: EXPECTED_TYPESCRIPT_SHA256,
    },
    toolchain: { node: EXPECTED_NODE_VERSION },
    generator: {
      path: "crates/oracle/h1-emit-oracle.mjs",
      sha256: sha256(fs.readFileSync(GENERATOR_PATH)),
    },
    schemas: {
      profile: {
        path: PROFILE_SCHEMA_RELATIVE_PATH,
        sha256: fileSha256(PROFILE_SCHEMA_RELATIVE_PATH),
      },
      observation: {
        path: OBSERVATION_SCHEMA_RELATIVE_PATH,
        sha256: fileSha256(OBSERVATION_SCHEMA_RELATIVE_PATH),
      },
    },
    base_program_profile: {
      path: H0_PROFILE_RELATIVE_PATH,
      sha256: fileSha256(H0_PROFILE_RELATIVE_PATH),
      retained_scope: "H0 single-project program/config/host/checker options unless narrowed below",
    },
    owner_inventory: {
      path: OWNER_INVENTORY_RELATIVE_PATH,
      sha256: fileSha256(OWNER_INVENTORY_RELATIVE_PATH),
      status: "draft/report-only",
    },
    execution: {
      project: "single",
      lifetime: "one-shot",
      selection: "whole-program",
      emitted_root: "source-file",
      output_products: ["javascript"],
      unsupported_policy: "typed-fail-closed-before-first-sink-write",
    },
    source_profile: {
      emit_eligible_extensions: [".ts"],
      non_emitting_dependencies: [".d.ts"],
      admitted_language:
        "ESNext JavaScript plus erasable TypeScript syntax accepted by the H0 parser",
      rejected_feature_roots: [
        "decorators",
        "export-equals",
        "import-equals",
        "jsx",
        "parameter-properties",
        "runtime-enums",
        "runtime-namespaces",
      ],
    },
    emit_active_options: {
      required: [
        { name: "target", accepted: [{ name: "ESNext", value: 99 }] },
        { name: "module", accepted: [{ name: "Preserve", value: 200 }] },
      ],
      admitted_optional: [
        { name: "useDefineForClassFields", accepted: ["absent", true] },
        { name: "noEmitOnError", accepted: ["absent", false, true] },
        { name: "emitBOM", accepted: ["absent", false, true] },
        {
          name: "newLine",
          accepted: [
            "absent",
            { name: "CarriageReturnLineFeed", value: 0 },
            { name: "LineFeed", value: 1 },
          ],
        },
        { name: "listEmittedFiles", accepted: ["absent", false, true] },
      ],
      h0_route: [{ name: "noEmit", accepted: [true] }],
      rejected_when_effective: [
        "allowImportingTsExtensions",
        "allowJs",
        "composite",
        "declaration",
        "declarationDir",
        "declarationMap",
        "emitDeclarationOnly",
        "experimentalDecorators",
        "importHelpers",
        "incremental",
        "inlineSourceMap",
        "isolatedModules",
        "jsx",
        "noCheck",
        "noEmitHelpers",
        "outDir",
        "outFile",
        "rewriteRelativeImportExtensions",
        "resolveJsonModule",
        "sourceMap",
        "tsBuildInfoFile",
        "verbatimModuleSyntax",
      ],
      retained_h0_options: retainedH0Options,
      unlisted_option_policy: "unsupported-before-first-write",
      request_only_rejections: ["custom-transformers", "plugins"],
    },
    transformer_order: transformerNames.map((name, index) => ({ index, name })),
    output_contract: {
      callback_text_encoding: "UTF-8 without BOM",
      bom: "independent callback boolean; sink prepends EFBBBF when true",
      path_profile: "case-sensitive canonical POSIX virtual root",
      source_provenance: "presence plus ordered canonical source paths",
      metadata:
        "presence plus own keys, generated UTF-16 source-map URL position, transform diagnostics, and dormant builder fields",
      callback_order_independent_from_emitted_files: true,
      sink_dispositions: ["written"],
      source_maps: "typed absent slot in admitted profile",
    },
    dormant_axes: {
      selections: ["target-source-file"],
      emitted_roots: ["bundle"],
      products: ["javascript-map", "declaration", "declaration-map", "build-info"],
      modes: ["declaration-only", "builder-signature", "build-info-only"],
    },
    oracle_cases: {
      admitted: fixtures
        .filter((fixture) => fixture.classification === "admitted")
        .map((fixture) => fixture.id),
      adjacent_unsupported: fixtures
        .filter((fixture) => fixture.classification === "adjacent-unsupported")
        .map((fixture) => fixture.id),
    },
  };
  profile.profile_fingerprint_sha256 = sha256(canonical(profile));
  return profile;
}

function validateProfile(profile) {
  requireCondition(
    exactKeys(profile, [
      "schema",
      "status",
      "phase",
      "typescript",
      "toolchain",
      "generator",
      "schemas",
      "base_program_profile",
      "owner_inventory",
      "execution",
      "source_profile",
      "emit_active_options",
      "transformer_order",
      "output_contract",
      "dormant_axes",
      "oracle_cases",
      "profile_fingerprint_sha256",
    ]),
    "H1 emit profile has missing or unknown top-level fields",
  );
  requireCondition(profile.schema === 1 && profile.status === "frozen", "invalid profile header");
  const semantic = { ...profile };
  delete semantic.profile_fingerprint_sha256;
  requireCondition(
    profile.profile_fingerprint_sha256 === sha256(canonical(semantic)),
    "H1 emit profile fingerprint mismatch",
  );
  requireCondition(
    profile.execution.output_products.length === 1 &&
      profile.execution.output_products[0] === "javascript",
    "bootstrap profile must admit JavaScript only",
  );
}

function validateObservationArtifact(artifact, profileBytes) {
  requireCondition(
    exactKeys(artifact, [
      "schema",
      "status",
      "phase",
      "typescript_version",
      "source_commit",
      "profile",
      "oracle_environment",
      "comparison",
      "cases",
    ]),
    "H1 emit observation has missing or unknown top-level fields",
  );
  requireCondition(artifact.schema === 1 && artifact.status === "frozen", "invalid oracle header");
  requireCondition(artifact.profile.sha256 === sha256(profileBytes), "oracle/profile binding mismatch");
  requireCondition(artifact.cases.length === fixtures.length, "oracle fixture set is incomplete");
  for (const [index, entry] of artifact.cases.entries()) {
    const fixture = fixtures[index];
    requireCondition(entry.input.id === fixture.id, `oracle case ${index} order drifted`);
    requireCondition(entry.determinism.repeat_runs === 2, `${fixture.id} repeat count drifted`);
    requireCondition(entry.determinism.exact_match, `${fixture.id} is nondeterministic`);
    const gate = entry.observation.profile_gate;
    if (fixture.classification === "admitted") {
      requireCondition(gate.disposition === "admitted", `${fixture.id} unexpectedly left the profile`);
      requireCondition(gate.reasons.length === 0, `${fixture.id} has profile rejection reasons`);
    } else {
      requireCondition(
        gate.disposition === "unsupported-before-first-write",
        `${fixture.id} is not an adjacent unsupported control`,
      );
      requireCondition(
        JSON.stringify(gate.reasons) === JSON.stringify([...fixture.expectedReasons].sort()),
        `${fixture.id} rejection reasons drifted`,
      );
      requireCondition(
        entry.observation.rust_expectation.sink_write_count === 0,
        `${fixture.id} unsupported expectation permits a sink write`,
      );
    }
    for (const [writeIndex, write] of entry.observation.writes.entries()) {
      requireCondition(write.index === writeIndex, `${fixture.id} write order drifted`);
      const callbackBytes = Buffer.from(write.callback_utf8_base64, "base64");
      const materializedBytes = Buffer.from(write.materialized_utf8_base64, "base64");
      requireCondition(
        callbackBytes.toString("utf8") === write.callback_text &&
          sha256(callbackBytes) === write.callback_utf8_sha256 &&
          callbackBytes.length === write.callback_utf8_bytes,
        `${fixture.id} callback bytes are not exact`,
      );
      const expectedMaterialized = write.write_byte_order_mark
        ? Buffer.concat([UTF8_BOM, callbackBytes])
        : callbackBytes;
      requireCondition(
        materializedBytes.equals(expectedMaterialized) &&
          sha256(materializedBytes) === write.materialized_utf8_sha256 &&
          materializedBytes.length === write.materialized_utf8_bytes,
        `${fixture.id} materialized bytes are not exact`,
      );
    }
  }
}

validateRuntime();
const profile = buildProfile();
validateProfile(profile);
const profileRendered = `${JSON.stringify(profile, null, 2)}\n`;
const cases = fixtures.map((fixture) => {
  const first = observeFixture(fixture);
  const second = observeFixture(fixture);
  return {
    input: fixtureInput(fixture),
    determinism: { repeat_runs: 2, exact_match: canonical(first) === canonical(second) },
    observation: first,
  };
});
const observationArtifact = {
  schema: 1,
  status: "frozen",
  phase: "H1.0a-callback-oracle",
  typescript_version: EXPECTED_TYPESCRIPT_VERSION,
  source_commit: EXPECTED_SOURCE_COMMIT,
  profile: {
    path: PROFILE_RELATIVE_PATH,
    sha256: sha256(Buffer.from(profileRendered, "utf8")),
  },
  oracle_environment: {
    virtual_root: "/project",
    case_sensitive: true,
    library: {
      path: "/__oracle__/lib.d.ts",
      utf8_base64: Buffer.from(ORACLE_LIBRARY, "utf8").toString("base64"),
      utf8_sha256: sha256(Buffer.from(ORACLE_LIBRARY, "utf8")),
      utf8_bytes: Buffer.byteLength(ORACLE_LIBRARY, "utf8"),
    },
  },
  comparison: {
    output_bytes: "exact",
    text_normalization: "none",
    path_mapping: `${VIRTUAL_ROOT}/project -> /project`,
    repeated_runs: 2,
  },
  cases,
};
validateObservationArtifact(observationArtifact, Buffer.from(profileRendered, "utf8"));
const observationRendered = `${JSON.stringify(observationArtifact, null, 2)}\n`;

const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(PROFILE_PATH, profileRendered);
  fs.writeFileSync(OBSERVATION_PATH, observationRendered);
  process.stdout.write(`wrote ${PROFILE_RELATIVE_PATH} and ${OBSERVATION_RELATIVE_PATH}\n`);
} else if (mode === "--check") {
  requireCondition(fs.existsSync(PROFILE_PATH), `missing ${PROFILE_RELATIVE_PATH}`);
  requireCondition(fs.existsSync(OBSERVATION_PATH), `missing ${OBSERVATION_RELATIVE_PATH}`);
  requireCondition(
    fs.readFileSync(PROFILE_PATH, "utf8") === profileRendered,
    `stale ${PROFILE_RELATIVE_PATH}; run with --write and review`,
  );
  requireCondition(
    fs.readFileSync(OBSERVATION_PATH, "utf8") === observationRendered,
    `stale ${OBSERVATION_RELATIVE_PATH}; run with --write and review`,
  );
  process.stdout.write(
    `H1 emit profile/oracle are fresh: admitted=${profile.oracle_cases.admitted.length} controls=${profile.oracle_cases.adjacent_unsupported.length}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(observationRendered);
} else {
  throw new Error("usage: h1-emit-oracle.mjs [--write|--check]");
}
