import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h1-fourslash-equivalence.mjs";
const TARGET_RELATIVE_PATH =
  "vendor/typescript-6.0.3/fourslash-whole-program-equivalence.v1.json";
const TARGET_PATH = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h1-fourslash-equivalence.schema.json";
const SUITE_PIN_RELATIVE_PATH =
  "vendor/typescript-6.0.3/test-suites-pin.v3.json";
const PROJECTION_RELATIVE_PATH =
  "vendor/typescript-6.0.3/fourslash-emit-projection.v1.json";
const PROFILE_RELATIVE_PATH = "ratchets/h1-emit-profile.v1.json";
const TYPESCRIPT_BUNDLE_RELATIVE_PATH =
  "vendor/typescript-6.0.3/lib/typescript.js";
const SOURCE_ROOT_RELATIVE_PATH = "ts-tests/tests/cases/fourslash";
const SOURCE_ROOT = path.join(WORKSPACE, SOURCE_ROOT_RELATIVE_PATH);
const NODE_VERSION_PATH = path.join(WORKSPACE, ".node-version");

const TYPESCRIPT_VERSION = "6.0.3";
const SOURCE_REPOSITORY = "https://github.com/microsoft/TypeScript.git";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE_VERSION = "25.2.1";
const SUITE_PIN_SHA256 =
  "5f7aee7d434066017c5cd115fb2195ff4959e5203eddc7ed9dafaf705cb38b34";
const PROJECTION_SHA256 =
  "d652d0e0ad1a6195cb3d74e97cb241f3da6a55b6811bd4770fb1ec56a2843c46";
const PROFILE_SHA256 =
  "91e05db331a090e180e9cda7fc8eaa505d795b229a49d78d62d1e086c8602991";
const TYPESCRIPT_BUNDLE_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const REFERENCE_BASELINE_STATE = "content-not-vendored-or-compared";
const VIRTUAL_BASE_PATH = "/tests/cases/fourslash";

const IMPLEMENTATION_SOURCES = [
  {
    source_path: "src/harness/fourslashImpl.ts",
    git_blob_sha1: "dc6341ef018f79e5d55a1d59aeafaeae2932c3d6",
  },
  {
    source_path: "src/harness/fourslashInterfaceImpl.ts",
    git_blob_sha1: "6178b2723f13e86f78261d848f48af8f4a998e18",
  },
];

const BUNDLE_DECLARATIONS = [
  {
    name: "getFileEmitOutput",
    start_offset: 6118484,
    end_offset: 6118964,
    start_line: 130904,
    end_line: 130911,
    sha256: "b8dc847c489382197ad6b85f3a109cdb1d54a050d805ff46c986bd601e7ab421",
  },
  {
    name: "getEmitOutput",
    start_offset: 7102303,
    end_offset: 7102668,
    start_line: 154020,
    end_line: 154025,
    sha256: "ea72a701544cbbf9389898d750a700a7e532f21df4d75dd85f45921d526ac9bd",
  },
];

const TARGET_NAMES = new Map([
  [ts.ScriptTarget.ES3, "ES3"],
  [ts.ScriptTarget.ES5, "ES5"],
  [ts.ScriptTarget.ES2015, "ES2015"],
  [ts.ScriptTarget.ES2016, "ES2016"],
  [ts.ScriptTarget.ES2017, "ES2017"],
  [ts.ScriptTarget.ES2018, "ES2018"],
  [ts.ScriptTarget.ES2019, "ES2019"],
  [ts.ScriptTarget.ES2020, "ES2020"],
  [ts.ScriptTarget.ES2021, "ES2021"],
  [ts.ScriptTarget.ES2022, "ES2022"],
  [ts.ScriptTarget.ES2023, "ES2023"],
  [ts.ScriptTarget.ES2024, "ES2024"],
  [ts.ScriptTarget.ES2025, "ES2025"],
  [ts.ScriptTarget.ESNext, "ESNext"],
  [ts.ScriptTarget.JSON, "JSON"],
]);

const MODULE_NAMES = new Map([
  [ts.ModuleKind.None, "None"],
  [ts.ModuleKind.CommonJS, "CommonJS"],
  [ts.ModuleKind.AMD, "AMD"],
  [ts.ModuleKind.UMD, "UMD"],
  [ts.ModuleKind.System, "System"],
  [ts.ModuleKind.ES2015, "ES2015"],
  [ts.ModuleKind.ES2020, "ES2020"],
  [ts.ModuleKind.ES2022, "ES2022"],
  [ts.ModuleKind.ESNext, "ESNext"],
  [ts.ModuleKind.Node16, "Node16"],
  [ts.ModuleKind.Node18, "Node18"],
  [ts.ModuleKind.Node20, "Node20"],
  [ts.ModuleKind.NodeNext, "NodeNext"],
  [ts.ModuleKind.Preserve, "Preserve"],
]);

const FILE_METADATA = new Set([
  "filename",
  "emitthisfile",
  "resolvereference",
  "symlink",
]);
const NON_COMPILER_GLOBAL_OPTIONS = new Set(["baselinefile"]);
const OPERATION_PATTERN =
  /^[ \t]*verify\.(baselineGetEmitOutput|getEmitOutput|verifyGetEmitOutputForCurrentFile|verifyGetEmitOutputContentsForCurrentFile)[ \t]*\(/gm;
const OPTION_LINE_PATTERN = /^\s*\/\/\s*@([A-Za-z0-9_]+):\s*(.*?)\s*\r?$/;
const MARKER_NAVIGATION_PATTERN = /goTo\.marker\(\s*["']([^"']+)["']\s*\)/g;

const EXPECTED_SUMMARY = {
  fixtures: 38,
  fixture_bytes: 31051,
  native_cases: 33,
  server_cases: 5,
  config_cases: 5,
  config_diagnostic_cases: 0,
  virtual_files: 94,
  emit_this_file_true: 47,
  emit_this_file_false: 2,
  targeted_program_emit_calls: 47,
  operation_methods: [
    { value: "baselineGetEmitOutput", cases: 31 },
    { value: "getEmitOutput", cases: 5 },
    { value: "verifyGetEmitOutputContentsForCurrentFile", cases: 1 },
    { value: "verifyGetEmitOutputForCurrentFile", cases: 1 },
  ],
  selection_modes: [
    { value: "emit-this-file-true", cases: 36 },
    { value: "active-file", cases: 2 },
  ],
  target_states: [{ value: "ES2025(12)", cases: 38 }],
  module_states: [
    { value: "absent", cases: 30 },
    { value: "CommonJS(1)", cases: 7 },
    { value: "AMD(2)", cases: 1 },
  ],
  cases_with_rejected_effective_options: 29,
  rejected_option_cases: [
    { name: "allowImportingTsExtensions", cases: 0 },
    { name: "allowJs", cases: 2 },
    { name: "composite", cases: 0 },
    { name: "declaration", cases: 17 },
    { name: "declarationDir", cases: 0 },
    { name: "declarationMap", cases: 5 },
    { name: "emitDeclarationOnly", cases: 0 },
    { name: "experimentalDecorators", cases: 0 },
    { name: "importHelpers", cases: 0 },
    { name: "incremental", cases: 0 },
    { name: "inlineSourceMap", cases: 2 },
    { name: "isolatedModules", cases: 0 },
    { name: "jsx", cases: 2 },
    { name: "noCheck", cases: 0 },
    { name: "noEmitHelpers", cases: 0 },
    { name: "outDir", cases: 7 },
    { name: "outFile", cases: 12 },
    { name: "rewriteRelativeImportExtensions", cases: 0 },
    { name: "resolveJsonModule", cases: 0 },
    { name: "sourceMap", cases: 8 },
    { name: "tsBuildInfoFile", cases: 0 },
    { name: "verbatimModuleSyntax", cases: 0 },
  ],
  baseline_path_observations: 31,
  inline_expectations: 7,
  targeted_api_blocked_cases: 38,
  required_target_module_matches: 0,
  promotion_candidates: 0,
  promoted_controls: 0,
  deferred_controls: 38,
  not_run_cases: 38,
  reference_baselines_compared: 0,
};

function fail(message) {
  throw new Error(message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function gitBlobSha1(bytes) {
  return crypto
    .createHash("sha1")
    .update(`blob ${bytes.length}\0`)
    .update(bytes)
    .digest("hex");
}

function pathHash(relative) {
  return {
    path: relative,
    sha256: sha256(fs.readFileSync(path.join(WORKSPACE, relative))),
  };
}

function compareBytes(left, right) {
  return Buffer.compare(Buffer.from(left), Buffer.from(right));
}

function requireJsonEqual(actual, expected, description) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    fail(
      `${description} mismatch\nactual=${JSON.stringify(actual)}\nexpected=${JSON.stringify(expected)}`,
    );
  }
}

function validateRuntime() {
  const recorded = fs.readFileSync(NODE_VERSION_PATH, "utf8").trim();
  const running = process.version.startsWith("v")
    ? process.version.slice(1)
    : process.version;
  requireCondition(recorded === EXPECTED_NODE_VERSION, ".node-version changed");
  requireCondition(
    running === EXPECTED_NODE_VERSION,
    `H1 FourSlash equivalence requires Node ${EXPECTED_NODE_VERSION}; running ${process.version}`,
  );
  requireCondition(ts.version === TYPESCRIPT_VERSION, "TypeScript version changed");
  requireCondition(
    pathHash(TYPESCRIPT_BUNDLE_RELATIVE_PATH).sha256 === TYPESCRIPT_BUNDLE_SHA256,
    "vendored TypeScript bundle changed",
  );
}

function readJsonInput(relative, expectedHash, description) {
  const bytes = fs.readFileSync(path.join(WORKSPACE, relative));
  requireCondition(sha256(bytes) === expectedHash, `${description} hash changed`);
  return JSON.parse(bytes.toString("utf8"));
}

function readInputs() {
  const suitePin = readJsonInput(
    SUITE_PIN_RELATIVE_PATH,
    SUITE_PIN_SHA256,
    "suite pin v3",
  );
  requireCondition(
    suitePin.schema === 3 &&
      suitePin.typescript_version === TYPESCRIPT_VERSION &&
      suitePin.source_commit === SOURCE_COMMIT,
    "suite pin v3 header changed",
  );
  for (const expected of IMPLEMENTATION_SOURCES) {
    const actual = suitePin.implementation_sources.find(
      (entry) => entry.source_path === expected.source_path,
    );
    requireJsonEqual(actual, expected, `${expected.source_path} identity`);
  }
  const projectionPin = suitePin.projections.find(
    (entry) => entry.name === "fourslash-batch-emit",
  );
  requireCondition(projectionPin !== undefined, "FourSlash projection pin absent");
  requireJsonEqual(
    projectionPin.manifest,
    { path: PROJECTION_RELATIVE_PATH, sha256: PROJECTION_SHA256 },
    "FourSlash projection manifest pin",
  );

  const projection = readJsonInput(
    PROJECTION_RELATIVE_PATH,
    PROJECTION_SHA256,
    "FourSlash emit projection",
  );
  requireCondition(
    projection.schema === 1 &&
      projection.status === "inventory-only-not-run" &&
      projection.typescript_version === TYPESCRIPT_VERSION &&
      projection.source_commit === SOURCE_COMMIT &&
      projection.projection.summary.fixture_files === 38 &&
      projection.qualification.executed_rows === 0,
    "FourSlash projection header changed",
  );

  const profile = readJsonInput(PROFILE_RELATIVE_PATH, PROFILE_SHA256, "H1 profile");
  requireCondition(
    profile.schema === 1 &&
      profile.status === "frozen" &&
      profile.phase === "H1.0a-bootstrap-profile",
    "H1 profile header changed",
  );
  requireJsonEqual(
    profile.emit_active_options.required,
    [
      { name: "target", accepted: [{ name: "ESNext", value: 99 }] },
      { name: "module", accepted: [{ name: "Preserve", value: 200 }] },
    ],
    "H1 required options",
  );
  return { suitePin, projection, profile };
}

function declarationRecord(sourceFile, text, expected) {
  const found = [];
  function visit(node) {
    if (ts.isFunctionDeclaration(node) && node.name?.text === expected.name) {
      found.push(node);
    }
    ts.forEachChild(node, visit);
  }
  visit(sourceFile);
  requireCondition(found.length === 1, `${expected.name} declaration count changed`);
  const declaration = found[0];
  const start = declaration.getStart(sourceFile);
  const end = declaration.end;
  const startPosition = sourceFile.getLineAndCharacterOfPosition(start);
  const endPosition = sourceFile.getLineAndCharacterOfPosition(end);
  const record = {
    name: expected.name,
    start_offset: start,
    end_offset: end,
    start_line: startPosition.line + 1,
    end_line: endPosition.line + 1,
    sha256: sha256(text.slice(start, end)),
  };
  requireJsonEqual(record, expected, `${expected.name} declaration`);
  return record;
}

function verifyBundleRoute() {
  const bundlePath = path.join(WORKSPACE, TYPESCRIPT_BUNDLE_RELATIVE_PATH);
  const text = fs.readFileSync(bundlePath, "utf8");
  const sourceFile = ts.createSourceFile(
    bundlePath,
    text,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.JS,
  );
  const records = BUNDLE_DECLARATIONS.map((expected) =>
    declarationRecord(sourceFile, text, expected),
  );
  const fileEmit = text.slice(records[0].start_offset, records[0].end_offset);
  const serviceEmit = text.slice(records[1].start_offset, records[1].end_offset);
  requireCondition(
    fileEmit.includes("program.emit(sourceFile") &&
      serviceEmit.includes("getFileEmitOutput(program, sourceFile"),
    "Language Service targeted-emit route changed",
  );
  return records;
}

function sourceLine(text, offset) {
  return text.slice(0, offset).split("\n").length;
}

function normalizeVirtualPath(fileName) {
  const normalized = fileName.replaceAll("\\", "/");
  return path.posix.isAbsolute(normalized)
    ? path.posix.normalize(normalized)
    : path.posix.join(VIRTUAL_BASE_PATH, normalized);
}

function parseFourSlashData(text, fixturePath) {
  const globalOptions = new Map();
  const files = [];
  let currentContent;
  let currentFileName = fixturePath;
  let currentFileOptions = new Map();

  function nextFile() {
    if (currentContent === undefined) return;
    files.push({
      path: normalizeVirtualPath(currentFileName),
      file_options: [...currentFileOptions].map(([name, value]) => ({ name, value })),
      dsl_content: currentContent,
    });
    currentContent = undefined;
    currentFileOptions = new Map();
    currentFileName = fixturePath;
  }

  for (let line of text.split("\n")) {
    if (line.endsWith("\r")) line = line.slice(0, -1);
    if (line.startsWith("////")) {
      const content = line.slice(4);
      currentContent =
        currentContent === undefined ? content : `${currentContent}\n${content}`;
      continue;
    }
    if (line.startsWith("///") && currentContent !== undefined) {
      fail(`${fixturePath} has a three-slash line inside FourSlash content`);
    }
    if (line.startsWith("//")) {
      const metadata = OPTION_LINE_PATTERN.exec(line);
      if (!metadata) continue;
      const key = metadata[1].toLowerCase();
      const value = metadata[2];
      if (!FILE_METADATA.has(key)) {
        requireCondition(
          !globalOptions.has(key),
          `${fixturePath} repeats global option ${key}`,
        );
        globalOptions.set(key, value);
      } else if (key === "filename") {
        nextFile();
        currentFileName = value;
        currentFileOptions.set(key, value);
      } else if (key !== "symlink") {
        currentFileOptions.set(key, value);
      }
      continue;
    }
    if (line !== "") nextFile();
  }
  nextFile();
  requireCondition(files.length > 0, `${fixturePath} has no virtual files`);
  return { globalOptions, files };
}

const optionIndex = new Map(
  ts.optionDeclarations.map((option) => [option.name.toLowerCase(), option]),
);

function convertOption(option, raw, label) {
  if (option.type === "boolean") {
    requireCondition(/^(true|false)$/i.test(raw), `${label} is not boolean`);
    return raw.toLowerCase() === "true";
  }
  if (option.type === "string") return raw;
  if (option.type === "number") {
    const value = Number.parseInt(raw, 10);
    requireCondition(Number.isFinite(value), `${label} is not numeric`);
    return value;
  }
  if (option.type instanceof Map) {
    const value = option.type.get(raw.toLowerCase());
    requireCondition(value !== undefined, `${label} has an unknown value ${raw}`);
    return value;
  }
  fail(`${label} uses an unsupported list/object option`);
}

function runnerOptions(globalOptions, fixturePath) {
  const options = {
    ...ts.getDefaultCompilerOptions(),
    jsx: undefined,
    newLine: ts.NewLineKind.CarriageReturnLineFeed,
  };
  const origins = new Map([
    ["target", "fourslash-default"],
    ["newLine", "fourslash-default"],
  ]);
  for (const [name, raw] of globalOptions) {
    if (NON_COMPILER_GLOBAL_OPTIONS.has(name)) continue;
    const option = optionIndex.get(name);
    requireCondition(option !== undefined, `${fixturePath} has unknown global @${name}`);
    options[option.name] = convertOption(option, raw, `${fixturePath} @${name}`);
    origins.set(option.name, "global-setting");
  }
  options.skipDefaultLibCheck = true;
  origins.set("skipDefaultLibCheck", "fourslash-default");
  return { options, origins };
}

function virtualHost(files) {
  const byCanonicalPath = new Map(
    files.map((file) => [file.path.toLowerCase(), file.dsl_content]),
  );
  const matchingFiles = (root, extensions) => {
    const normalizedRoot = path.posix.normalize(root).toLowerCase();
    return files
      .map((file) => file.path)
      .filter((fileName) => {
        const lower = fileName.toLowerCase();
        return (
          (lower === normalizedRoot || lower.startsWith(`${normalizedRoot}/`)) &&
          extensions.some((extension) => lower.endsWith(extension.toLowerCase()))
        );
      });
  };
  return {
    useCaseSensitiveFileNames: false,
    fileExists(fileName) {
      return byCanonicalPath.has(path.posix.normalize(fileName).toLowerCase());
    },
    readFile(fileName) {
      return byCanonicalPath.get(path.posix.normalize(fileName).toLowerCase());
    },
    readDirectory(root, extensions) {
      return matchingFiles(root, extensions);
    },
    trace() {},
  };
}

function configOptions(parsed, runner, fixturePath) {
  const configs = parsed.files.filter((file) => /\/(?:ts|js)config\.json$/i.test(file.path));
  if (configs.length === 0) {
    return { ...runner, config: null, roots: [], diagnostics: [] };
  }
  requireCondition(configs.length === 1, `${fixturePath} has multiple configs`);
  const config = configs[0];
  const parsedJson = ts.parseConfigFileTextToJson(config.path, config.dsl_content);
  requireCondition(parsedJson.config !== undefined, `${fixturePath} config JSON failed`);
  const baseDirectory = path.posix.dirname(config.path);
  const converted = ts.convertCompilerOptionsFromJson(
    parsedJson.config.compilerOptions ?? {},
    baseDirectory,
    config.path,
  );
  requireCondition(converted.errors.length === 0, `${fixturePath} config conversion failed`);
  const existing = { ...converted.options, ...runner.options };
  const origins = new Map(runner.origins);
  for (const name of Object.keys(converted.options)) {
    if (!origins.has(name)) origins.set(name, "virtual-config");
  }
  const sourceFile = ts.parseJsonText(config.path, config.dsl_content);
  const commandLine = ts.parseJsonSourceFileConfigFileContent(
    sourceFile,
    virtualHost(parsed.files),
    baseDirectory,
    existing,
    config.path,
  );
  for (const name of Object.keys(commandLine.options)) {
    if (!origins.has(name)) origins.set(name, "virtual-config");
  }
  return {
    options: commandLine.options,
    origins,
    config: config.path,
    roots: commandLine.fileNames.map((fileName) => path.posix.normalize(fileName)),
    diagnostics: commandLine.errors.map((diagnostic) => diagnostic.code),
  };
}

function enumProjection(value, names, origin) {
  if (value === undefined) return { state: "absent" };
  const name = names.get(value);
  requireCondition(name !== undefined, `unknown enum option value ${value}`);
  return { state: "set", name, value, origin };
}

function booleanProjection(value, origin) {
  return value === undefined
    ? { state: "absent" }
    : { state: "set", value, origin };
}

function stableOptionValue(value) {
  if (typeof value === "string") return value.replaceAll("\\", "/");
  return value;
}

function optionOrigin(name, options, origins) {
  if (origins.has(name)) return origins.get(name);
  return options[name] === undefined ? "absent" : "virtual-config";
}

function optionDisplay(projection) {
  return projection.state === "absent"
    ? "absent"
    : `${projection.name}(${projection.value})`;
}

function classifyOptions(options, origins, profile) {
  const target = enumProjection(
    options.target,
    TARGET_NAMES,
    optionOrigin("target", options, origins),
  );
  const module = enumProjection(
    options.module,
    MODULE_NAMES,
    optionOrigin("module", options, origins),
  );
  const useDefineForClassFields = booleanProjection(
    options.useDefineForClassFields,
    optionOrigin("useDefineForClassFields", options, origins),
  );
  const noEmit = booleanProjection(
    options.noEmit,
    optionOrigin("noEmit", options, origins),
  );
  const rejectedWhenEffective = [];
  for (const name of profile.emit_active_options.rejected_when_effective) {
    const value = options[name];
    if (value !== undefined && value !== false) {
      rejectedWhenEffective.push({
        name,
        value: stableOptionValue(value),
        origin: optionOrigin(name, options, origins),
      });
    }
  }
  const blockers = ["api:language-service-targeted-emit"];
  if (options.target !== ts.ScriptTarget.ESNext) {
    blockers.push(`required-option:target=${optionDisplay(target)}`);
  }
  if (options.module !== ts.ModuleKind.Preserve) {
    blockers.push(`required-option:module=${optionDisplay(module)}`);
  }
  if (options.useDefineForClassFields === false) {
    blockers.push("required-option:useDefineForClassFields=false");
  }
  if (options.noEmit === true) blockers.push("route:noEmit=true");
  for (const rejected of rejectedWhenEffective) {
    blockers.push(`rejected-option:${rejected.name}`);
  }
  return {
    profile: {
      target,
      module,
      use_define_for_class_fields: useDefineForClassFields,
      no_emit: noEmit,
      rejected_when_effective: rejectedWhenEffective,
    },
    blockers,
  };
}

function fileOption(file, name) {
  return file.file_options.find((option) => option.name === name)?.value;
}

function operationRecord(text, projected, parsed, fixturePath) {
  const matches = [...text.matchAll(OPERATION_PATTERN)];
  requireCondition(matches.length === 1, `${fixturePath} operation count changed`);
  const match = matches[0];
  requireCondition(
    match[1] === projected.operation.method &&
      sourceLine(text, match.index) === projected.operation.line,
    `${fixturePath} projected operation changed`,
  );
  const emitTrueFiles = parsed.files
    .filter((file) => fileOption(file, "emitthisfile") === "true")
    .map((file) => file.path);
  let selection;
  let selectedFiles;
  if (
    match[1] === "verifyGetEmitOutputForCurrentFile" ||
    match[1] === "verifyGetEmitOutputContentsForCurrentFile"
  ) {
    const prefix = text.slice(0, match.index);
    const navigations = [...prefix.matchAll(MARKER_NAVIGATION_PATTERN)];
    requireCondition(navigations.length > 0, `${fixturePath} has no active marker`);
    const marker = navigations.at(-1)[1];
    const owners = parsed.files.filter((file) =>
      file.dsl_content.includes(`/*${marker}*/`),
    );
    requireCondition(owners.length === 1, `${fixturePath} marker ${marker} is ambiguous`);
    selection = "active-file";
    selectedFiles = [owners[0].path];
  } else {
    requireCondition(emitTrueFiles.length > 0, `${fixturePath} has no emitThisFile=true`);
    selection = "emit-this-file-true";
    selectedFiles = emitTrueFiles;
  }
  return {
    method: match[1],
    line: sourceLine(text, match.index),
    language_service_method: "getEmitOutput(fileName)",
    selection,
    selected_files: selectedFiles,
    targeted_program_emit_calls: selectedFiles.length,
  };
}

function expectedObservation(globalOptions, fixturePath, method) {
  if (method === "baselineGetEmitOutput") {
    const baseline =
      globalOptions.get("baselinefile") ??
      `${path.posix.basename(fixturePath, ".ts")}.baseline`;
    return {
      state: "baseline-path-pinned-content-not-vendored-or-compared",
      baseline_path: `tests/baselines/reference/fourslash/${baseline}`,
    };
  }
  return { state: "inline-expectation-not-executed" };
}

function countValues(values) {
  const counts = new Map();
  for (const value of values) counts.set(value, (counts.get(value) ?? 0) + 1);
  return [...counts]
    .map(([value, cases]) => ({ value, cases }))
    .sort(
      (left, right) =>
        right.cases - left.cases || compareBytes(left.value, right.value),
    );
}

function buildSummary(cases, profile) {
  const rejectedOptionCases = profile.emit_active_options.rejected_when_effective.map(
    (name) => ({
      name,
      cases: cases.filter((entry) =>
        entry.effective_profile.rejected_when_effective.some(
          (option) => option.name === name,
        ),
      ).length,
    }),
  );
  return {
    fixtures: cases.length,
    fixture_bytes: cases.reduce((total, entry) => total + entry.fixture.bytes, 0),
    native_cases: cases.filter((entry) => entry.test_type === "native").length,
    server_cases: cases.filter((entry) => entry.test_type === "server").length,
    config_cases: cases.filter((entry) => entry.config !== null).length,
    config_diagnostic_cases: cases.filter((entry) => entry.config_diagnostic_codes.length > 0)
      .length,
    virtual_files: cases.reduce((total, entry) => total + entry.virtual_files.length, 0),
    emit_this_file_true: cases.reduce(
      (total, entry) =>
        total + entry.virtual_files.filter((file) => file.emit_this_file === true).length,
      0,
    ),
    emit_this_file_false: cases.reduce(
      (total, entry) =>
        total + entry.virtual_files.filter((file) => file.emit_this_file === false).length,
      0,
    ),
    targeted_program_emit_calls: cases.reduce(
      (total, entry) => total + entry.operation.targeted_program_emit_calls,
      0,
    ),
    operation_methods: countValues(cases.map((entry) => entry.operation.method)),
    selection_modes: countValues(cases.map((entry) => entry.operation.selection)),
    target_states: countValues(
      cases.map((entry) => optionDisplay(entry.effective_profile.target)),
    ),
    module_states: countValues(
      cases.map((entry) => optionDisplay(entry.effective_profile.module)),
    ),
    cases_with_rejected_effective_options: cases.filter(
      (entry) => entry.effective_profile.rejected_when_effective.length > 0,
    ).length,
    rejected_option_cases: rejectedOptionCases,
    baseline_path_observations: cases.filter(
      (entry) => entry.expected_observation.baseline_path !== undefined,
    ).length,
    inline_expectations: cases.filter(
      (entry) => entry.expected_observation.state === "inline-expectation-not-executed",
    ).length,
    targeted_api_blocked_cases: cases.filter((entry) =>
      entry.equivalence_blockers.includes("api:language-service-targeted-emit"),
    ).length,
    required_target_module_matches: cases.filter(
      (entry) =>
        entry.effective_profile.target.state === "set" &&
        entry.effective_profile.target.value === ts.ScriptTarget.ESNext &&
        entry.effective_profile.module.state === "set" &&
        entry.effective_profile.module.value === ts.ModuleKind.Preserve,
    ).length,
    promotion_candidates: cases.filter(
      (entry) => entry.whole_program_equivalence === "candidate-not-run",
    ).length,
    promoted_controls: cases.filter(
      (entry) => entry.whole_program_equivalence === "proven-equivalent",
    ).length,
    deferred_controls: cases.filter(
      (entry) => entry.whole_program_equivalence === "deferred",
    ).length,
    not_run_cases: cases.filter((entry) => entry.execution_state === "not-run").length,
    reference_baselines_compared: 0,
  };
}

function buildArtifact(projection, profile, bundleDeclarations) {
  const cases = [];
  for (const projected of projection.fixtures) {
    const absolute = path.join(SOURCE_ROOT, ...projected.path.split("/"));
    const bytes = fs.readFileSync(absolute);
    requireCondition(
      bytes.length === projected.bytes &&
        gitBlobSha1(bytes) === projected.git_blob_sha1,
      `${projected.path} identity changed`,
    );
    const text = bytes.toString("utf8");
    requireCondition(
      Buffer.from(text, "utf8").equals(bytes),
      `${projected.path} is not UTF-8`,
    );
    const parsed = parseFourSlashData(text, projected.path);
    const runner = runnerOptions(parsed.globalOptions, projected.path);
    const configured = configOptions(parsed, runner, projected.path);
    const classification = classifyOptions(
      configured.options,
      configured.origins,
      profile,
    );
    const operation = operationRecord(text, projected, parsed, projected.path);
    requireCondition(
      classification.blockers.includes("api:language-service-targeted-emit") &&
        classification.blockers.some((blocker) =>
          blocker.startsWith("required-option:target="),
        ) &&
        classification.blockers.some((blocker) =>
          blocker.startsWith("required-option:module="),
        ),
      `${projected.path} lost its zero-promotion proof`,
    );
    const virtualFiles = parsed.files.map((file) => {
      const raw = file.file_options.find((option) => option.name === "emitthisfile")
        ?.value;
      return {
        path: file.path,
        emit_this_file:
          raw === undefined ? null : raw.toLowerCase() === "true",
      };
    });
    requireJsonEqual(
      virtualFiles
        .filter((file) => file.emit_this_file !== null)
        .map((file) => file.emit_this_file),
      projected.emit_this_file_directives.map((directive) => directive.value),
      `${projected.path} emitThisFile directives`,
    );
    cases.push({
      case: cases.length,
      id: `typescript-6.0.3/fourslash/${projected.path}`,
      fixture: {
        path: projected.path,
        bytes: projected.bytes,
        sha256: sha256(bytes),
        git_blob_sha1: projected.git_blob_sha1,
      },
      test_type: projected.path.startsWith("server/") ? "server" : "native",
      global_settings: [...parsed.globalOptions].map(([name, value]) => ({
        name,
        value,
      })),
      virtual_files: virtualFiles,
      config: configured.config,
      config_roots: configured.roots,
      config_diagnostic_codes: configured.diagnostics,
      operation,
      effective_profile: classification.profile,
      source_analysis: { state: "not-required-effective-options-and-api-route" },
      expected_observation: expectedObservation(
        parsed.globalOptions,
        projected.path,
        operation.method,
      ),
      equivalence_decisive_blocker: classification.blockers[0],
      equivalence_blockers: classification.blockers,
      whole_program_equivalence: "deferred",
      promotion_state: "not-promoted",
      execution_state: "not-run",
      reference_baseline_state: REFERENCE_BASELINE_STATE,
    });
  }
  requireCondition(cases.length === 38, "FourSlash case count changed");
  const summary = buildSummary(cases, profile);
  requireJsonEqual(summary, EXPECTED_SUMMARY, "FourSlash equivalence summary");
  return {
    schema: 1,
    status: "classified-not-run",
    phase: "H1.0a-fourslash-whole-program-equivalence",
    typescript: {
      version: TYPESCRIPT_VERSION,
      source_repository: SOURCE_REPOSITORY,
      source_commit: SOURCE_COMMIT,
    },
    generator: pathHash(GENERATOR_RELATIVE_PATH),
    contract: pathHash(CONTRACT_RELATIVE_PATH),
    inputs: {
      suite_pin_v3: { path: SUITE_PIN_RELATIVE_PATH, sha256: SUITE_PIN_SHA256 },
      emit_projection: {
        path: PROJECTION_RELATIVE_PATH,
        sha256: PROJECTION_SHA256,
      },
      h1_profile: { path: PROFILE_RELATIVE_PATH, sha256: PROFILE_SHA256 },
      typescript_bundle: {
        path: TYPESCRIPT_BUNDLE_RELATIVE_PATH,
        sha256: TYPESCRIPT_BUNDLE_SHA256,
      },
      implementation_sources: IMPLEMENTATION_SOURCES,
      bundle_declarations: bundleDeclarations,
    },
    classification_contract: {
      fourslash_defaults:
        "getDefaultCompilerOptions, jsx cleared, CRLF newline, then global settings and skipDefaultLibCheck=true; virtual config options are parsed with existing options winning",
      operation_route:
        "every selected operation calls LanguageService.getEmitOutput(fileName), which calls getFileEmitOutput and Program.emit(sourceFile)",
      h1_request: "ProgramSession::emit is a whole-Program request with no target source",
      promotion_rule:
        "a control requires exact targeted-versus-whole-Program observation equivalence and the frozen H1 profile before promotion",
      required_options: ["target=ESNext(99)", "module=Preserve(200)"],
      execution_state: "not-run",
      reference_baseline_state: REFERENCE_BASELINE_STATE,
    },
    cases,
    summary,
  };
}

function validateArtifact(artifact) {
  requireCondition(
    artifact.schema === 1 &&
      artifact.status === "classified-not-run" &&
      artifact.phase === "H1.0a-fourslash-whole-program-equivalence" &&
      artifact.summary.fixtures === artifact.cases.length &&
      artifact.summary.targeted_api_blocked_cases === artifact.cases.length &&
      artifact.summary.promoted_controls === 0 &&
      artifact.summary.deferred_controls === artifact.cases.length &&
      artifact.summary.not_run_cases === artifact.cases.length &&
      artifact.summary.reference_baselines_compared === 0,
    "invalid FourSlash equivalence closure",
  );
}

validateRuntime();
const { projection, profile } = readInputs();
const bundleDeclarations = verifyBundleRoute();
const artifact = buildArtifact(projection, profile, bundleDeclarations);
validateArtifact(artifact);
const rendered = `${JSON.stringify(artifact, null, 2)}\n`;
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(TARGET_PATH, rendered);
  process.stdout.write(`wrote ${TARGET_RELATIVE_PATH}\n`);
} else if (mode === "--check") {
  requireCondition(
    fs.existsSync(TARGET_PATH) && fs.readFileSync(TARGET_PATH, "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run with --write and review`,
  );
  process.stdout.write(
    `H1 FourSlash equivalence classification is fresh: cases=${artifact.summary.fixtures} promoted=${artifact.summary.promoted_controls} deferred=${artifact.summary.deferred_controls} status=not-run\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h1-fourslash-equivalence.mjs [--write|--check]");
}
