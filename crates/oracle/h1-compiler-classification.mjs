import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h1-compiler-classification.mjs";
const TARGET_RELATIVE_PATH =
  "vendor/typescript-6.0.3/compiler-profile-classification.v1.json";
const TARGET_PATH = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h1-compiler-classification.schema.json";
const EXPANSION_RELATIVE_PATH =
  "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const CONFIG_PLANS_RELATIVE_PATH =
  "vendor/typescript-6.0.3/compiler-config-plans.v1.json";
const PROFILE_RELATIVE_PATH = "ratchets/h1-emit-profile.v1.json";
const TYPESCRIPT_BUNDLE_RELATIVE_PATH =
  "vendor/typescript-6.0.3/lib/typescript.js";
const SOURCE_ROOT = path.join(WORKSPACE, "ts-tests/tests/cases/compiler");
const NODE_VERSION_PATH = path.join(WORKSPACE, ".node-version");
const VIRTUAL_SOURCE_ROOT = "/.src";

const TYPESCRIPT_VERSION = "6.0.3";
const SOURCE_REPOSITORY = "https://github.com/microsoft/TypeScript.git";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE_VERSION = "25.2.1";
const EXPANSION_SHA256 =
  "9c6e991103b571f7a8800dc5e1ef66088017689f3769de8fcdb408b2dc125188";
const CONFIG_PLANS_SHA256 =
  "d19356ed235fd32579f8688be44ee2f57dd7965cf45ccf172e7f01cd95050453";
const PROFILE_SHA256 =
  "d7a7d212780ef94cb9675c104ec8d2ca28af95764fa78f8aeb8c7c25885fa7db";
const TYPESCRIPT_BUNDLE_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const REFERENCE_BASELINE_STATE = "content-not-vendored-or-compared";

const IMPLEMENTATION_SOURCES = [
  {
    source_path: "src/testRunner/compilerRunner.ts",
    git_blob_sha1: "aed00f47656b316f3f20c913e2408a128d4671cb",
  },
  {
    source_path: "src/harness/harnessIO.ts",
    git_blob_sha1: "a06bde1c95182ea1bfad0ddf9af73053501a6dc7",
  },
  {
    source_path: "src/harness/harnessUtils.ts",
    git_blob_sha1: "f768325897167ad793eeff9ced7763e12f9aa154",
  },
  {
    source_path: "src/harness/vfsUtil.ts",
    git_blob_sha1: "b217fb57bba950c13d5d2e821b0652eacce0e65f",
  },
];

const HARNESS_ONLY_OPTIONS = new Set(
  [
    "captureSuggestions",
    "currentDirectory",
    "fileName",
    "fullEmitPaths",
    "link",
    "noImplicitReferences",
    "noTypesAndSymbols",
    "symlink",
    "typeScriptVersion",
    "useCaseSensitiveFilenames",
    "useCaseSensitiveFileNames",
  ].map((name) => name.toLowerCase()),
);

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

const REJECTED_FEATURE_ROOTS = [
  "decorators",
  "export-equals",
  "import-equals",
  "jsx",
  "parameter-properties",
  "runtime-enums",
  "runtime-namespaces",
];

const OPTION_LINE_PATTERN = /^\/{2}\s*@(\w+)\s*:\s*([^\r\n]*)/;
const LINK_LINE_PATTERN =
  /^\/{2}\s*@link\s*:\s*([^\r\n]*)\s*->\s*([^\r\n]*)/;
const EXPECTED_SUMMARY = {
  fixtures: 6537,
  virtual_config_fixtures: 103,
  cases: 7276,
  required_target_module_matches: 7,
  effective_option_clear_cases: 2,
  source_analyzed_cases: 2,
  source_profile_blocked_cases: 1,
  cases_with_target_blocker: 7056,
  cases_with_module_blocker: 7255,
  cases_with_use_define_for_class_fields_blocker: 13,
  cases_with_no_emit_route: 510,
  cases_with_rejected_effective_options: 2094,
  rejected_option_cases: [
    { name: "allowImportingTsExtensions", cases: 0 },
    { name: "allowJs", cases: 346 },
    { name: "composite", cases: 9 },
    { name: "declaration", cases: 1030 },
    { name: "declarationDir", cases: 9 },
    { name: "declarationMap", cases: 6 },
    { name: "emitDeclarationOnly", cases: 88 },
    { name: "experimentalDecorators", cases: 117 },
    { name: "importHelpers", cases: 110 },
    { name: "incremental", cases: 7 },
    { name: "inlineSourceMap", cases: 10 },
    { name: "isolatedModules", cases: 71 },
    { name: "jsx", cases: 191 },
    { name: "noCheck", cases: 3 },
    { name: "noEmitHelpers", cases: 114 },
    { name: "outDir", cases: 185 },
    { name: "outFile", cases: 115 },
    { name: "rewriteRelativeImportExtensions", cases: 1 },
    { name: "resolveJsonModule", cases: 39 },
    { name: "sourceMap", cases: 192 },
    { name: "tsBuildInfoFile", cases: 3 },
    { name: "verbatimModuleSyntax", cases: 12 },
  ],
  rejected_feature_cases: [
    { name: "decorators", cases: 0 },
    { name: "export-equals", cases: 1 },
    { name: "import-equals", cases: 1 },
    { name: "jsx", cases: 0 },
    { name: "parameter-properties", cases: 0 },
    { name: "runtime-enums", cases: 0 },
    { name: "runtime-namespaces", cases: 0 },
  ],
  decisive_blockers: [
    { value: "required-option:target=ES2015(2)", cases: 6394 },
    { value: "required-option:target=ES5(1)", cases: 475 },
    { value: "required-option:module=absent", cases: 175 },
    { value: "required-option:target=ES2017(4)", cases: 83 },
    { value: "required-option:target=ES2020(7)", cases: 27 },
    { value: "required-option:target=ES2022(9)", cases: 25 },
    { value: "required-option:target=absent", cases: 17 },
    { value: "required-option:target=ES2018(5)", cases: 14 },
    { value: "required-option:module=ESNext(99)", cases: 13 },
    { value: "required-option:module=CommonJS(1)", cases: 8 },
    { value: "required-option:module=NodeNext(199)", cases: 6 },
    { value: "required-option:target=ES2019(6)", cases: 6 },
    { value: "required-option:target=ES2016(3)", cases: 5 },
    { value: "required-option:target=ES3(0)", cases: 5 },
    { value: "required-option:module=AMD(2)", cases: 4 },
    { value: "required-option:module=System(4)", cases: 4 },
    { value: "required-option:target=ES2021(8)", cases: 4 },
    { value: "route:noEmit=true", cases: 4 },
    { value: "required-option:module=ES2015(5)", cases: 3 },
    { value: "rejected-feature:export-equals", cases: 1 },
    { value: "rejected-option:declaration", cases: 1 },
    { value: "required-option:target=ES2024(11)", cases: 1 },
  ],
  target_states: [
    { value: "ES2015(2)", cases: 6394 },
    { value: "ES5(1)", cases: 475 },
    { value: "ESNext(99)", cases: 220 },
    { value: "ES2017(4)", cases: 83 },
    { value: "ES2020(7)", cases: 27 },
    { value: "ES2022(9)", cases: 25 },
    { value: "absent", cases: 17 },
    { value: "ES2018(5)", cases: 14 },
    { value: "ES2019(6)", cases: 6 },
    { value: "ES2016(3)", cases: 5 },
    { value: "ES3(0)", cases: 5 },
    { value: "ES2021(8)", cases: 4 },
    { value: "ES2024(11)", cases: 1 },
  ],
  module_states: [
    { value: "absent", cases: 5470 },
    { value: "CommonJS(1)", cases: 1179 },
    { value: "AMD(2)", cases: 245 },
    { value: "System(4)", cases: 111 },
    { value: "ESNext(99)", cases: 62 },
    { value: "NodeNext(199)", cases: 57 },
    { value: "ES2015(5)", cases: 52 },
    { value: "UMD(3)", cases: 30 },
    { value: "Preserve(200)", cases: 21 },
    { value: "ES2020(6)", cases: 15 },
    { value: "None(0)", cases: 15 },
    { value: "Node16(100)", cases: 7 },
    { value: "Node18(101)", cases: 5 },
    { value: "Node20(102)", cases: 5 },
    { value: "ES2022(7)", cases: 2 },
  ],
  dispositions: [
    { value: "deferred-profile", cases: 7273 },
    { value: "h0-no-emit", cases: 2 },
    { value: "bootstrap-candidate-not-run", cases: 1 },
  ],
  bootstrap_profile_admitted_cases: 1,
  not_run_cases: 7276,
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

function jsonEqual(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function requireJsonEqual(actual, expected, description) {
  if (!jsonEqual(actual, expected)) {
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
    `H1 compiler classification requires Node ${EXPECTED_NODE_VERSION}; running ${process.version}`,
  );
  requireCondition(ts.version === TYPESCRIPT_VERSION, "TypeScript version changed");
  requireCondition(
    pathHash(TYPESCRIPT_BUNDLE_RELATIVE_PATH).sha256 ===
      TYPESCRIPT_BUNDLE_SHA256,
    "vendored TypeScript bundle changed",
  );
}

function readJsonInput(relativePath, expectedHash, description) {
  const bytes = fs.readFileSync(path.join(WORKSPACE, relativePath));
  requireCondition(
    sha256(bytes) === expectedHash,
    `${description} hash changed`,
  );
  return JSON.parse(bytes.toString("utf8"));
}

function readInputs() {
  const expansion = readJsonInput(
    EXPANSION_RELATIVE_PATH,
    EXPANSION_SHA256,
    "test-suite expansion",
  );
  requireCondition(
    expansion.schema === 1 &&
      expansion.typescript_version === TYPESCRIPT_VERSION &&
      expansion.source_commit === SOURCE_COMMIT &&
      expansion.virtual_source_root === VIRTUAL_SOURCE_ROOT &&
      expansion.summary.compiler_sources === 6537 &&
      expansion.summary.compiler_cases === 7276 &&
      expansion.summary.compiler_virtual_configs === 103 &&
      expansion.summary.not_run_cases === 7908,
    "compiler expansion header or frozen counts changed",
  );

  const configPlans = readJsonInput(
    CONFIG_PLANS_RELATIVE_PATH,
    CONFIG_PLANS_SHA256,
    "compiler config plans",
  );
  requireCondition(
    configPlans.schema === 1 &&
      configPlans.typescript_version === TYPESCRIPT_VERSION &&
      configPlans.source_commit === SOURCE_COMMIT &&
      configPlans.summary.config_plans.fixture_total === 103 &&
      configPlans.summary.config_plans.case_total === 106,
    "compiler config-plan header or frozen counts changed",
  );

  const profile = readJsonInput(
    PROFILE_RELATIVE_PATH,
    PROFILE_SHA256,
    "H1 bootstrap profile",
  );
  requireCondition(
    profile.schema === 1 &&
      profile.status === "frozen" &&
      profile.phase === "H1.0a-bootstrap-profile",
    "H1 bootstrap profile header changed",
  );
  requireJsonEqual(
    profile.source_profile.rejected_feature_roots,
    REJECTED_FEATURE_ROOTS,
    "rejected feature roots",
  );
  return { expansion, configPlans, profile };
}

function safeSourcePath(relativePath) {
  requireCondition(
    typeof relativePath === "string" &&
      relativePath.length > 0 &&
      relativePath === path.posix.normalize(relativePath) &&
      !relativePath.startsWith("/") &&
      !relativePath.startsWith("../"),
    `unsafe compiler source path ${JSON.stringify(relativePath)}`,
  );
  const absolute = path.resolve(SOURCE_ROOT, ...relativePath.split("/"));
  requireCondition(
    absolute.startsWith(`${path.resolve(SOURCE_ROOT)}${path.sep}`),
    `compiler source escaped suite root: ${relativePath}`,
  );
  return absolute;
}

function readVerifiedSource(expansion, fixture) {
  const source = expansion.sources[fixture.source];
  requireCondition(source?.suite === "compiler", "compiler source is absent");
  const absolute = safeSourcePath(source.path);
  const raw = fs.readFileSync(absolute);
  requireCondition(
    raw.length === source.bytes &&
      sha256(raw) === source.sha256 &&
      gitBlobSha1(raw) === source.git_blob_sha1,
    `${source.path} source identity changed`,
  );
  const decoded = ts.sys.readFile(absolute);
  requireCondition(typeof decoded === "string", `cannot decode ${source.path}`);
  requireCondition(
    Buffer.byteLength(decoded, "utf8") === fixture.decoded_utf8_bytes &&
      sha256(Buffer.from(decoded, "utf8")) === fixture.decoded_sha256,
    `${source.path} decoded identity changed`,
  );
  return { source, decoded };
}

function orderedSettings(object) {
  return Object.entries(object).map(([name, value]) => ({ name, value }));
}

function makeUnits(text, fixturePath) {
  const units = [];
  const links = [];
  let currentContent;
  let currentOptions = {};
  let currentName;
  for (const line of text.split(/\r?\n/)) {
    const link = LINK_LINE_PATTERN.exec(line);
    if (link) {
      links.push({ target: link[1].trim(), link_path: link[2].trim() });
      continue;
    }
    const metadata = OPTION_LINE_PATTERN.exec(line);
    if (metadata) {
      const name = metadata[1];
      const value = metadata[2].trim();
      currentOptions[name] = value;
      if (name.toLowerCase() !== "filename") continue;
      if (currentName) {
        units.push({
          name: currentName,
          text: currentContent,
          file_options: orderedSettings(currentOptions),
        });
        currentContent = undefined;
        currentOptions = {};
        currentName = value;
      } else {
        currentName = value;
        if (currentContent) {
          requireCondition(
            ts.skipTrivia(currentContent, 0, false, false) ===
              currentContent.length,
            `${fixturePath} has content before its first @filename`,
          );
        }
        currentContent = "";
      }
      continue;
    }
    if (currentContent === undefined) currentContent = "";
    else if (currentContent !== "") currentContent += "\n";
    currentContent += line;
  }
  currentName =
    units.length > 0 || currentName
      ? currentName
      : path.posix.basename(fixturePath);
  units.push({
    name: currentName,
    text: currentContent || "",
    file_options: orderedSettings(currentOptions),
  });
  return { units, links };
}

function isConfigName(fileName) {
  const basename = path.posix.basename(fileName).toLowerCase();
  return basename === "tsconfig.json" || basename === "jsconfig.json";
}

function contentIdentity(unit) {
  if (unit.text === undefined) return { state: "missing" };
  const bytes = Buffer.from(unit.text, "utf8");
  return { state: "present", utf8_bytes: bytes.length, sha256: sha256(bytes) };
}

function documentSymlinks(fileOptions) {
  const setting = fileOptions.find((entry) => entry.name === "symlink");
  if (!setting || setting.value === "") return [];
  return setting.value.split(",").map((entry) => entry.trim());
}

function loadFixture(expansion, fixture) {
  const { source, decoded } = readVerifiedSource(expansion, fixture);
  const parsed = makeUnits(decoded, source.path);
  const configIndex = parsed.units.findIndex((unit) => isConfigName(unit.name));
  requireCondition(
    (configIndex >= 0) === (fixture.virtual_config !== null),
    `${source.path} config partition changed`,
  );
  let normalIndex = 0;
  for (const [unitIndex, unit] of parsed.units.entries()) {
    const expected =
      unitIndex === configIndex
        ? fixture.virtual_config
        : fixture.normal_units[normalIndex++];
    requireCondition(expected !== undefined, `${source.path} unit is absent`);
    requireCondition(unit.name === expected.name, `${source.path} unit name changed`);
    requireJsonEqual(
      unit.file_options,
      expected.file_options,
      `${source.path} unit file options`,
    );
    requireJsonEqual(
      contentIdentity(unit),
      expected.content,
      `${source.path} unit content`,
    );
    requireJsonEqual(
      documentSymlinks(unit.file_options),
      expected.document_symlinks,
      `${source.path} unit symlinks`,
    );
  }
  requireCondition(
    normalIndex === fixture.normal_units.length,
    `${source.path} normal-unit count changed`,
  );
  requireJsonEqual(parsed.links, fixture.links, `${source.path} links`);
  return { source, units: parsed.units, links: parsed.links, configIndex };
}

function createParseConfigHost(units) {
  const log = [];
  return {
    useCaseSensitiveFileNames: false,
    readDirectory(directory, extensions, excludes, includes, depth) {
      const result = ts.matchFiles(
        directory,
        extensions,
        excludes,
        includes,
        false,
        "",
        depth,
        (dir) => {
          const files = [];
          const directories = new Set();
          for (const unit of units) {
            const fileName = ts.getNormalizedAbsolutePath(
              unit.name,
              VIRTUAL_SOURCE_ROOT,
            );
            if (fileName.toLowerCase().startsWith(dir.toLowerCase())) {
              let relative = fileName.substring(dir.length);
              if (relative.startsWith("/")) relative = relative.substring(1);
              const separator = relative.indexOf("/");
              if (separator >= 0) {
                directories.add(relative.substring(0, separator));
              } else files.push(relative);
            }
          }
          return { files, directories: ts.arrayFrom(directories) };
        },
        ts.identity,
      );
      log.push({
        operation: "read_directory",
        directory,
        extensions: [...extensions],
        excludes: excludes === undefined ? null : [...excludes],
        includes: includes === undefined ? null : [...includes],
        depth: depth ?? null,
        result: [...result],
      });
      return result;
    },
    fileExists(fileName) {
      const result = units.some(
        (unit) => unit.name.toLowerCase() === fileName.toLowerCase(),
      );
      log.push({ operation: "file_exists", path: fileName, result });
      return result;
    },
    readFile(fileName) {
      const result = ts.forEach(units, (unit) =>
        unit.name.toLowerCase() === fileName.toLowerCase()
          ? unit.text
          : undefined,
      );
      log.push({
        operation: "read_file",
        path: fileName,
        result: result === undefined ? "missing" : "text",
      });
      return result;
    },
    log,
  };
}

function jsonValue(value, label, ancestors = new Set()) {
  if (
    value === null ||
    typeof value === "string" ||
    typeof value === "boolean"
  ) {
    return value;
  }
  if (typeof value === "number") {
    requireCondition(Number.isFinite(value), `${label} has non-finite number`);
    return value;
  }
  requireCondition(value !== undefined, `${label} has undefined`);
  requireCondition(typeof value === "object", `${label} is not JSON data`);
  requireCondition(!ancestors.has(value), `${label} contains a cycle`);
  ancestors.add(value);
  let result;
  if (Array.isArray(value)) {
    result = value.map((entry, index) =>
      jsonValue(entry, `${label}[${index}]`, ancestors),
    );
  } else {
    const prototype = Object.getPrototypeOf(value);
    requireCondition(
      prototype === Object.prototype || prototype === null,
      `${label} contains a non-plain object`,
    );
    result = {};
    for (const key of Object.keys(value)) {
      result[key] = jsonValue(value[key], `${label}.${key}`, ancestors);
    }
  }
  ancestors.delete(value);
  return result;
}

function diagnosticRecord(diagnostic) {
  return {
    code: diagnostic.code,
    file: diagnostic.file?.fileName ?? null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    message: ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
  };
}

function parseConfigContext(loaded, recordedPlan) {
  const config = loaded.units[loaded.configIndex];
  const source = ts.parseJsonText(config.name, config.text);
  const conversionDiagnostics = [];
  const rawConfig = ts.convertToObject(source, conversionDiagnostics);
  requireCondition(
    conversionDiagnostics.length === 0,
    `${loaded.source.path} raw config conversion diagnosed`,
  );
  const configFileName = ts.getNormalizedAbsolutePath(
    config.name,
    VIRTUAL_SOURCE_ROOT,
  );
  const host = createParseConfigHost(loaded.units);
  const parsed = ts.parseJsonSourceFileConfigFileContent(
    source,
    host,
    ts.getDirectoryPath(configFileName),
    undefined,
    configFileName,
  );
  const candidates = loaded.units
    .map((unit, id) => ({ id, name: unit.name }))
    .filter((unit) => unit.id !== loaded.configIndex);
  const rootUnitIds = [];
  const otherUnitIds = [];
  for (const candidate of candidates) {
    const absolute = ts.getNormalizedAbsolutePath(
      candidate.name,
      VIRTUAL_SOURCE_ROOT,
    );
    if (parsed.fileNames.includes(absolute)) rootUnitIds.push(candidate.id);
    else otherUnitIds.push(candidate.id);
  }
  const rootSet = new Set(rootUnitIds);
  const programRootUnitIds = candidates
    .filter(
      (candidate) =>
        rootSet.has(candidate.id) &&
        !ts.fileExtensionIs(candidate.name, ts.Extension.Json),
    )
    .map((candidate) => candidate.id);
  const extendedSources = (
    parsed.options.configFile?.extendedSourceFiles ?? []
  ).map((fileName) => {
    const unitId = loaded.units.findIndex(
      (unit) => unit.name.toLowerCase() === fileName.toLowerCase(),
    );
    requireCondition(unitId >= 0, `${loaded.source.path} extended config absent`);
    return {
      file_name: fileName,
      unit_id: unitId,
      content: contentIdentity(loaded.units[unitId]),
    };
  });
  const actualPlan = {
    source: { index: recordedPlan.source.index, path: loaded.source.path },
    configuration_count: recordedPlan.configuration_count,
    config_unit: { id: loaded.configIndex, name: config.name },
    candidate_units: candidates,
    parsed_file_names: [...parsed.fileNames],
    root_unit_ids: rootUnitIds,
    other_unit_ids: otherUnitIds,
    program_root_unit_ids: programRootUnitIds,
    raw_config: jsonValue(rawConfig, `${loaded.source.path} raw config`),
    extended_sources: extendedSources,
    discovery_options: {
      allow_js: ts.getAllowJSCompilerOption(parsed.options),
      resolve_json_module: ts.getResolveJsonModule(parsed.options),
      out_dir: parsed.options.outDir ?? null,
      declaration_dir: parsed.options.declarationDir ?? null,
    },
    diagnostics: parsed.errors.map(diagnosticRecord),
    host_log: host.log,
  };
  requireJsonEqual(actualPlan, recordedPlan, `${loaded.source.path} config plan`);
  return {
    options: ts.cloneCompilerOptions(parsed.options),
    root_unit_ids: rootUnitIds,
    other_unit_ids: otherUnitIds,
    program_root_unit_ids: programRootUnitIds,
    diagnostic_codes: parsed.errors.map((diagnostic) => diagnostic.code),
  };
}

const optionIndex = new Map(
  ts.optionDeclarations.map((option) => [option.name.toLowerCase(), option]),
);

function optionValue(option, raw) {
  const errors = [];
  let value;
  if (option.type === "boolean") value = raw.toLowerCase() === "true";
  else if (option.type === "string") value = raw;
  else if (option.type === "number") value = Number.parseInt(raw, 10);
  else if (option.type === "list" || option.type === "listOrElement") {
    value = ts.parseListTypeOption(option, raw, errors);
  } else value = ts.parseCustomTypeOption(option, raw, errors);
  requireCondition(
    errors.length === 0 &&
      (option.type !== "number" || Number.isFinite(value)),
    `invalid value ${raw} for @${option.name}`,
  );
  return value;
}

function mergedSettings(base, overrides) {
  const settings = new Map(base.map((setting) => [setting.name, setting.value]));
  for (const setting of overrides) settings.set(setting.name, setting.value);
  return settings;
}

function effectiveCompilerOptions(baseOptions, settings) {
  const options = ts.cloneCompilerOptions(baseOptions);
  options.newLine = options.newLine || ts.NewLineKind.CarriageReturnLineFeed;
  options.noErrorTruncation = true;
  options.skipDefaultLibCheck =
    options.skipDefaultLibCheck === undefined
      ? true
      : options.skipDefaultLibCheck;
  const harnessAssignments = new Set();
  for (const [name, raw] of settings) {
    if (name === "typeScriptVersion") continue;
    const option = optionIndex.get(name.toLowerCase());
    if (option) {
      options[option.name] = optionValue(option, raw);
      harnessAssignments.add(option.name);
      continue;
    }
    requireCondition(
      HARNESS_ONLY_OPTIONS.has(name.toLowerCase()),
      `unknown harness/compiler option @${name}`,
    );
  }
  return { options, harnessAssignments };
}

function optionOrigin(name, baseOptions, harnessAssignments) {
  if (harnessAssignments.has(name)) return "harness-setting";
  if (baseOptions[name] !== undefined) return "virtual-config";
  return "absent";
}

function enumProjection(value, names, origin) {
  if (value === undefined) return { state: "absent" };
  return {
    state: "set",
    name: names.get(value) ?? `unknown-${value}`,
    value,
    origin,
  };
}

function scalarProjection(value, origin) {
  if (value === undefined) return { state: "absent" };
  return { state: "set", value, origin };
}

function normalizeOptionValue(value) {
  if (Array.isArray(value)) return value.map(normalizeOptionValue);
  if (value instanceof Map) {
    return [...value].map(([key, entry]) => [key, normalizeOptionValue(entry)]);
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, normalizeOptionValue(value[key])]),
    );
  }
  return value;
}

function optionDisplay(projection) {
  if (projection.state === "absent") return "absent";
  return `${projection.name}(${projection.value})`;
}

function classifyOptions(baseOptions, settings, profile) {
  const { options, harnessAssignments } = effectiveCompilerOptions(
    baseOptions,
    settings,
  );
  const target = enumProjection(
    options.target,
    TARGET_NAMES,
    optionOrigin("target", baseOptions, harnessAssignments),
  );
  const module = enumProjection(
    options.module,
    MODULE_NAMES,
    optionOrigin("module", baseOptions, harnessAssignments),
  );
  const useDefineForClassFields = scalarProjection(
    options.useDefineForClassFields,
    optionOrigin("useDefineForClassFields", baseOptions, harnessAssignments),
  );
  const noEmit = scalarProjection(
    options.noEmit,
    optionOrigin("noEmit", baseOptions, harnessAssignments),
  );
  const rejectedWhenEffective = [];
  for (const name of profile.emit_active_options.rejected_when_effective) {
    const value = options[name];
    if (value !== undefined && value !== false) {
      rejectedWhenEffective.push({
        name,
        value: normalizeOptionValue(value),
        origin: optionOrigin(name, baseOptions, harnessAssignments),
      });
    }
  }
  const blockers = [];
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
    options,
    projection: {
      target,
      module,
      use_define_for_class_fields: useDefineForClassFields,
      no_emit: noEmit,
      rejected_when_effective: rejectedWhenEffective,
    },
    blockers,
  };
}

function exactSetting(settings, name) {
  return [...settings].find(([candidate]) => candidate === name)?.[1];
}

function currentDirectory(settings) {
  const configured = exactSetting(settings, "currentDirectory");
  return configured === undefined
    ? VIRTUAL_SOURCE_ROOT
    : ts.getNormalizedAbsolutePath(configured, VIRTUAL_SOURCE_ROOT);
}

function containsReferencePath(text) {
  return [...text.matchAll(/reference/g)].some((match) => {
    const suffix = text.slice(match.index + "reference".length);
    return /^\s+path/.test(suffix);
  });
}

function explicitRootSelection(loaded, settings) {
  const candidates = loaded.units.map((_unit, id) => id);
  const last = candidates.at(-1);
  requireCondition(last !== undefined, `${loaded.source.path} has no unit`);
  const lastUnit = loaded.units[last];
  const implicitReferences =
    exactSetting(settings, "noImplicitReferences") !== undefined ||
    (lastUnit.text ?? "").includes("require(") ||
    containsReferencePath(lastUnit.text ?? "");
  const rootUnitIds = implicitReferences ? [last] : candidates;
  const otherUnitIds = implicitReferences
    ? candidates.filter((id) => loaded.units[id].name !== lastUnit.name)
    : [];
  const programRootUnitIds = rootUnitIds.filter(
    (id) => !ts.fileExtensionIs(loaded.units[id].name, ts.Extension.Json),
  );
  return {
    kind: "explicit",
    reason: implicitReferences ? "last-unit-implicit-references" : "all-units",
    root_unit_ids: rootUnitIds,
    other_unit_ids: otherUnitIds,
    program_root_unit_ids: programRootUnitIds,
    vfs_write_order: [...rootUnitIds, ...otherUnitIds],
  };
}

function hasDirectory(files, directory) {
  const normalized = ts.normalizePath(directory);
  const prefix = normalized.endsWith("/") ? normalized : `${normalized}/`;
  return [...files.keys()].some((fileName) => fileName.startsWith(prefix));
}

function syntaxFeatures(sourceFile) {
  const found = new Set();
  function visit(node) {
    if (ts.isEnumDeclaration(node)) found.add("runtime-enums");
    if (ts.isModuleDeclaration(node)) found.add("runtime-namespaces");
    if (ts.isImportEqualsDeclaration(node)) found.add("import-equals");
    if (ts.isExportAssignment(node) && node.isExportEquals) {
      found.add("export-equals");
    }
    if (
      ts.isJsxElement(node) ||
      ts.isJsxFragment(node) ||
      ts.isJsxSelfClosingElement(node)
    ) {
      found.add("jsx");
    }
    if (
      ts.isParameter(node) &&
      node.modifiers?.some((modifier) =>
        [
          ts.SyntaxKind.PublicKeyword,
          ts.SyntaxKind.PrivateKeyword,
          ts.SyntaxKind.ProtectedKeyword,
          ts.SyntaxKind.ReadonlyKeyword,
          ts.SyntaxKind.OverrideKeyword,
        ].includes(modifier.kind),
      )
    ) {
      found.add("parameter-properties");
    }
    if (ts.canHaveDecorators(node) && (ts.getDecorators(node)?.length ?? 0) > 0) {
      found.add("decorators");
    }
    ts.forEachChild(node, visit);
  }
  visit(sourceFile);
  return REJECTED_FEATURE_ROOTS.filter((feature) => found.has(feature));
}

function unitSourceKind(fileName) {
  const lower = fileName.toLowerCase();
  if (lower.endsWith(".d.ts")) return "declaration-dependency";
  if (lower.endsWith(".ts")) return "javascript-emit-input";
  return "unsupported-extension";
}

function analyzeProgram(loaded, selection, settings, options) {
  requireCondition(
    loaded.source && loaded.units.every((unit) => unit.text !== undefined),
    `${loaded.source.path} analysis does not support missing content`,
  );
  requireCondition(
    loaded.units.every((unit) => documentSymlinks(unit.file_options).length === 0),
    `${loaded.source.path} analysis requires document-symlink support`,
  );
  requireCondition(
    loaded.links.length === 0,
    `${loaded.source.path} analysis requires global-link support`,
  );
  const cwd = currentDirectory(settings);
  const unitByPath = new Map();
  for (const id of selection.vfs_write_order) {
    const unit = loaded.units[id];
    const fileName = ts.getNormalizedAbsolutePath(unit.name, cwd);
    unitByPath.set(fileName, { id, unit });
  }
  const baseHost = ts.createCompilerHost(options, true);
  const host = {
    ...baseHost,
    getCurrentDirectory: () => cwd,
    useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: (fileName) => fileName,
    fileExists(fileName) {
      const normalized = ts.normalizePath(fileName);
      return unitByPath.has(normalized) || baseHost.fileExists(normalized);
    },
    readFile(fileName) {
      const normalized = ts.normalizePath(fileName);
      return unitByPath.get(normalized)?.unit.text ?? baseHost.readFile(normalized);
    },
    directoryExists(directory) {
      return (
        hasDirectory(unitByPath, directory) ||
        (baseHost.directoryExists?.(directory) ?? false)
      );
    },
    getDirectories(directory) {
      return baseHost.getDirectories?.(directory) ?? [];
    },
    realpath(fileName) {
      const normalized = ts.normalizePath(fileName);
      return unitByPath.has(normalized)
        ? normalized
        : (baseHost.realpath?.(normalized) ?? normalized);
    },
    getSourceFile(fileName, languageVersion) {
      const normalized = ts.normalizePath(fileName);
      const fixture = unitByPath.get(normalized);
      if (!fixture) return baseHost.getSourceFile(fileName, languageVersion);
      return ts.createSourceFile(
        normalized,
        fixture.unit.text,
        languageVersion,
        true,
        ts.getScriptKindFromFileName(normalized),
      );
    },
  };
  const rootNames = selection.program_root_unit_ids.map((id) =>
    ts.getNormalizedAbsolutePath(loaded.units[id].name, cwd),
  );
  const program = ts.createProgram(rootNames, options, host);
  const reached = [];
  for (const sourceFile of program.getSourceFiles()) {
    const fixture = unitByPath.get(ts.normalizePath(sourceFile.fileName));
    if (!fixture) continue;
    reached.push({
      unit: fixture.id,
      name: fixture.unit.name,
      source_kind: unitSourceKind(fixture.unit.name),
      rejected_feature_roots: syntaxFeatures(sourceFile),
      parse_diagnostic_codes: [
        ...new Set(sourceFile.parseDiagnostics.map((diagnostic) => diagnostic.code)),
      ].sort((left, right) => left - right),
    });
  }
  const reachedIds = new Set(reached.map((entry) => entry.unit));
  requireCondition(
    selection.program_root_unit_ids.every((id) => reachedIds.has(id)),
    `${loaded.source.path} program did not include every root`,
  );
  const blockers = [];
  for (const entry of reached) {
    if (entry.source_kind === "unsupported-extension") {
      blockers.push(`unsupported-extension:${path.posix.extname(entry.name) || "none"}`);
    }
  }
  if (!reached.some((entry) => entry.source_kind === "javascript-emit-input")) {
    blockers.push("source:no-javascript-emit-input");
  }
  const features = new Set(
    reached.flatMap((entry) => entry.rejected_feature_roots),
  );
  for (const feature of REJECTED_FEATURE_ROOTS) {
    if (features.has(feature)) blockers.push(`rejected-feature:${feature}`);
  }
  return {
    current_directory: cwd,
    root_unit_ids: selection.root_unit_ids,
    other_unit_ids: selection.other_unit_ids,
    program_root_unit_ids: selection.program_root_unit_ids,
    reached_units: reached,
    profile_blockers: [...new Set(blockers)],
  };
}

function countBy(values) {
  const counts = new Map();
  for (const value of values) counts.set(value, (counts.get(value) ?? 0) + 1);
  return [...counts]
    .map(([value, cases]) => ({ value, cases }))
    .sort(
      (left, right) =>
        right.cases - left.cases ||
        (left.value < right.value ? -1 : left.value > right.value ? 1 : 0),
    );
}

function buildSummary(cases, analyses, profile) {
  return {
    fixtures: 6537,
    virtual_config_fixtures: 103,
    cases: cases.length,
    required_target_module_matches: cases.filter(
      (entry) =>
        entry.effective_profile.target.state === "set" &&
        entry.effective_profile.target.value === ts.ScriptTarget.ESNext &&
        entry.effective_profile.module.state === "set" &&
        entry.effective_profile.module.value === ts.ModuleKind.Preserve,
    ).length,
    effective_option_clear_cases: cases.filter(
      (entry) => entry.source_analysis.state === "analyzed",
    ).length,
    source_analyzed_cases: analyses.length,
    source_profile_blocked_cases: cases.filter(
      (entry) =>
        entry.source_analysis.state === "analyzed" &&
        entry.profile_blockers.length > 0,
    ).length,
    cases_with_target_blocker: cases.filter((entry) =>
      entry.profile_blockers.some((blocker) =>
        blocker.startsWith("required-option:target="),
      ),
    ).length,
    cases_with_module_blocker: cases.filter((entry) =>
      entry.profile_blockers.some((blocker) =>
        blocker.startsWith("required-option:module="),
      ),
    ).length,
    cases_with_use_define_for_class_fields_blocker: cases.filter((entry) =>
      entry.profile_blockers.includes(
        "required-option:useDefineForClassFields=false",
      ),
    ).length,
    cases_with_no_emit_route: cases.filter((entry) =>
      entry.profile_blockers.includes("route:noEmit=true"),
    ).length,
    cases_with_rejected_effective_options: cases.filter(
      (entry) => entry.effective_profile.rejected_when_effective.length > 0,
    ).length,
    rejected_option_cases: profile.emit_active_options.rejected_when_effective.map(
      (name) => ({
        name,
        cases: cases.filter((entry) =>
          entry.effective_profile.rejected_when_effective.some(
            (rejected) => rejected.name === name,
          ),
        ).length,
      }),
    ),
    rejected_feature_cases: REJECTED_FEATURE_ROOTS.map((name) => ({
      name,
      cases: cases.filter((entry) =>
        entry.profile_blockers.includes(`rejected-feature:${name}`),
      ).length,
    })),
    decisive_blockers: countBy(
      cases
        .filter((entry) => entry.decisive_blocker !== null)
        .map((entry) => entry.decisive_blocker),
    ),
    target_states: countBy(
      cases.map((entry) => optionDisplay(entry.effective_profile.target)),
    ),
    module_states: countBy(
      cases.map((entry) => optionDisplay(entry.effective_profile.module)),
    ),
    dispositions: countBy(cases.map((entry) => entry.disposition)),
    bootstrap_profile_admitted_cases: cases.filter(
      (entry) => entry.bootstrap_profile_admitted,
    ).length,
    not_run_cases: cases.filter((entry) => entry.execution_state === "not-run")
      .length,
    reference_baselines_compared: 0,
  };
}

function buildArtifact(expansion, configPlans, profile) {
  const configPlanBySource = new Map(
    configPlans.fixtures.map((plan) => [plan.source.index, plan]),
  );
  const loadedFixtures = new Map();
  const configContexts = new Map();
  const load = (fixture) => {
    let loaded = loadedFixtures.get(fixture.source);
    if (!loaded) {
      loaded = loadFixture(expansion, fixture);
      loadedFixtures.set(fixture.source, loaded);
    }
    return loaded;
  };
  for (const fixture of expansion.compiler_fixtures) {
    if (fixture.virtual_config === null) continue;
    const loaded = load(fixture);
    const recordedPlan = configPlanBySource.get(fixture.source);
    requireCondition(recordedPlan !== undefined, "config plan row is absent");
    requireCondition(
      recordedPlan.configuration_count === fixture.configurations.length,
      `${loaded.source.path} config case count changed`,
    );
    configContexts.set(
      fixture.source,
      parseConfigContext(loaded, recordedPlan),
    );
  }
  requireCondition(
    configContexts.size === 103 && configPlanBySource.size === 103,
    "not every virtual config was reconstructed",
  );

  const cases = [];
  const analyses = [];
  let expansionCase = 0;
  for (const fixture of expansion.compiler_fixtures) {
    const config = configContexts.get(fixture.source);
    const baseOptions = config?.options ?? { noResolve: false };
    for (const [configurationIndex, configuration] of fixture.configurations.entries()) {
      const recorded = expansion.cases[expansionCase];
      requireCondition(
        recorded.suite === "compiler" &&
          recorded.source === fixture.source &&
          recorded.configuration.kind === "compiler" &&
          recorded.configuration.configuration === configurationIndex &&
          recorded.initial_execution_state === "not-run",
        `compiler expansion case ${expansionCase} changed`,
      );
      const settings = mergedSettings(fixture.settings, configuration.settings);
      const classification = classifyOptions(baseOptions, settings, profile);
      let sourceAnalysis = { state: "not-required-effective-options" };
      let blockers = classification.blockers;
      if (blockers.length === 0) {
        const loaded = load(fixture);
        const selection = config
          ? {
              kind: "config",
              root_unit_ids: config.root_unit_ids,
              other_unit_ids: config.other_unit_ids,
              program_root_unit_ids: config.program_root_unit_ids,
              vfs_write_order: [
                ...config.root_unit_ids,
                ...config.other_unit_ids,
              ],
            }
          : explicitRootSelection(loaded, settings);
        const analysis = analyzeProgram(
          loaded,
          selection,
          settings,
          classification.options,
        );
        const analysisIndex = analyses.length;
        analyses.push({
          expansion_case: expansionCase,
          source: fixture.source,
          configuration: configurationIndex,
          ...analysis,
        });
        sourceAnalysis = { state: "analyzed", analysis: analysisIndex };
        blockers = [...blockers, ...analysis.profile_blockers];
      }
      const admitted = blockers.length === 0;
      const disposition = admitted
        ? "bootstrap-candidate-not-run"
        : blockers.length === 1 && blockers[0] === "route:noEmit=true"
          ? "h0-no-emit"
          : "deferred-profile";
      cases.push({
        expansion_case: expansionCase,
        id: recorded.id,
        source: fixture.source,
        configuration: configurationIndex,
        effective_profile: classification.projection,
        source_analysis: sourceAnalysis,
        bootstrap_profile_admitted: admitted,
        disposition,
        decisive_blocker: blockers[0] ?? null,
        profile_blockers: blockers,
        execution_state: "not-run",
        reference_baseline_state: REFERENCE_BASELINE_STATE,
      });
      expansionCase += 1;
    }
  }
  requireCondition(
    expansionCase === 7276 && expansion.cases[expansionCase].suite === "project",
    "compiler case boundary changed",
  );
  const summary = buildSummary(cases, analyses, profile);
  if (EXPECTED_SUMMARY !== undefined) {
    requireJsonEqual(summary, EXPECTED_SUMMARY, "compiler classification summary");
  }
  return {
    schema: 1,
    status: "classified-not-run",
    phase: "H1.0a-compiler-profile-classification",
    typescript: {
      version: TYPESCRIPT_VERSION,
      source_repository: SOURCE_REPOSITORY,
      source_commit: SOURCE_COMMIT,
    },
    generator: pathHash(GENERATOR_RELATIVE_PATH),
    contract: pathHash(CONTRACT_RELATIVE_PATH),
    inputs: {
      suite_expansion: {
        path: EXPANSION_RELATIVE_PATH,
        sha256: EXPANSION_SHA256,
      },
      compiler_config_plans: {
        path: CONFIG_PLANS_RELATIVE_PATH,
        sha256: CONFIG_PLANS_SHA256,
      },
      h1_profile: { path: PROFILE_RELATIVE_PATH, sha256: PROFILE_SHA256 },
      typescript_bundle: {
        path: TYPESCRIPT_BUNDLE_RELATIVE_PATH,
        sha256: TYPESCRIPT_BUNDLE_SHA256,
      },
      implementation_sources: IMPLEMENTATION_SOURCES,
    },
    classification_contract: {
      effective_option_order:
        "virtual tsconfig parse, Compiler.compileFiles defaults, then harness settings with matrix overrides",
      source_analysis_gate:
        "construct a vendored TypeScript Program only when effective options have no bootstrap blocker",
      source_analysis_scope:
        "fixture VFS program roots plus module-resolved fixture source dependencies",
      required_options: ["target=ESNext(99)", "module=Preserve(200)"],
      admitted_products: ["javascript"],
      execution_state: "not-run",
      reference_baseline_state: REFERENCE_BASELINE_STATE,
    },
    config_classification: {
      fixtures: configContexts.size,
      cases: [...configContexts.keys()].reduce(
        (total, source) =>
          total + expansion.compiler_fixtures[source].configurations.length,
        0,
      ),
      diagnostics: [...configContexts.values()].filter(
        (context) => context.diagnostic_codes.length > 0,
      ).length,
    },
    analyses,
    cases,
    summary,
  };
}

function validateArtifact(artifact) {
  requireCondition(
    artifact.schema === 1 &&
      artifact.status === "classified-not-run" &&
      artifact.phase === "H1.0a-compiler-profile-classification",
    "invalid compiler classification header",
  );
  requireCondition(
    artifact.summary.cases === artifact.cases.length &&
      artifact.summary.source_analyzed_cases === artifact.analyses.length &&
      artifact.summary.not_run_cases === artifact.cases.length &&
      artifact.summary.reference_baselines_compared === 0,
    "invalid compiler classification closure",
  );
}

validateRuntime();
const { expansion, configPlans, profile } = readInputs();
const artifact = buildArtifact(expansion, configPlans, profile);
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
    `H1 compiler profile classification is fresh: cases=${artifact.summary.cases} analyzed=${artifact.summary.source_analyzed_cases} admitted=${artifact.summary.bootstrap_profile_admitted_cases} status=not-run\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h1-compiler-classification.mjs [--write|--check]");
}
