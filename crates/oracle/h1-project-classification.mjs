import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h1-project-classification.mjs";
const TARGET_RELATIVE_PATH =
  "vendor/typescript-6.0.3/project-profile-classification.v1.json";
const TARGET_PATH = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h1-project-classification.schema.json";
const EXPANSION_RELATIVE_PATH =
  "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const PROFILE_RELATIVE_PATH = "ratchets/h1-emit-profile.v1.json";
const TYPESCRIPT_BUNDLE_RELATIVE_PATH =
  "vendor/typescript-6.0.3/lib/typescript.js";
const FOCUSED_ORACLE_RELATIVE_PATH =
  "vendor/typescript-6.0.3/project-node-modules-search.v1.json";
const DESCRIPTOR_ROOT = path.join(WORKSPACE, "ts-tests/tests/cases/project");
const PROJECTS_ROOT = path.join(WORKSPACE, "ts-tests/tests/cases/projects");
const VIRTUAL_ROOT = "/.src";
const VIRTUAL_PROJECTS_ROOT = "/.src/tests/cases/projects";
const NODE_VERSION_PATH = path.join(WORKSPACE, ".node-version");

const TYPESCRIPT_VERSION = "6.0.3";
const SOURCE_REPOSITORY = "https://github.com/microsoft/TypeScript.git";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE_VERSION = "25.2.1";
const EXPANSION_SHA256 =
  "9c6e991103b571f7a8800dc5e1ef66088017689f3769de8fcdb408b2dc125188";
const PROFILE_SHA256 =
  "d7a7d212780ef94cb9675c104ec8d2ca28af95764fa78f8aeb8c7c25885fa7db";
const TYPESCRIPT_BUNDLE_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const FOCUSED_ORACLE_SHA256 =
  "daa2afdd72235612b0a6d27ab50de709a9f62095c226dfcb0020222e005ed2c1";
const REFERENCE_BASELINE_STATE = "content-not-vendored-or-compared";

const IMPLEMENTATION_SOURCES = [
  {
    source_path: "src/testRunner/projectsRunner.ts",
    git_blob_sha1: "5befdf497dff2accd67e08c3c51100b66f1b14b5",
  },
  {
    source_path: "src/compiler/commandLineParser.ts",
    git_blob_sha1: "c17cc4ef9ca01cedd915a7040efb248aa19d2e18",
  },
  {
    source_path: "src/harness/harnessIO.ts",
    git_blob_sha1: "a06bde1c95182ea1bfad0ddf9af73053501a6dc7",
  },
  {
    source_path: "src/harness/vfsUtil.ts",
    git_blob_sha1: "b217fb57bba950c13d5d2e821b0652eacce0e65f",
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

const MODULE_VARIANTS = [
  {
    name: "commonjs",
    value: ts.ModuleKind.CommonJS,
    baseline_folder: "node",
  },
  { name: "amd", value: ts.ModuleKind.AMD, baseline_folder: "amd" },
];

const EXPECTED_SUMMARY = {
  fixtures: 316,
  explicit_input_fixtures: 285,
  project_config_fixtures: 16,
  discovered_config_fixtures: 15,
  cases: 632,
  explicit_input_cases: 570,
  config_cases: 62,
  explicit_declared_roots: 604,
  explicit_missing_roots: 6,
  config_roots: 74,
  javascript_observation_applicable_cases: 572,
  required_target_module_matches: 0,
  effective_option_clear_cases: 0,
  cases_with_target_blocker: 632,
  cases_with_module_blocker: 632,
  cases_with_use_define_for_class_fields_blocker: 0,
  cases_with_no_emit_route: 0,
  cases_with_rejected_effective_options: 556,
  rejected_option_cases: [
    { name: "allowImportingTsExtensions", cases: 0 },
    { name: "allowJs", cases: 26 },
    { name: "composite", cases: 0 },
    { name: "declaration", cases: 528 },
    { name: "declarationDir", cases: 6 },
    { name: "declarationMap", cases: 0 },
    { name: "emitDeclarationOnly", cases: 0 },
    { name: "experimentalDecorators", cases: 10 },
    { name: "importHelpers", cases: 0 },
    { name: "incremental", cases: 0 },
    { name: "inlineSourceMap", cases: 0 },
    { name: "isolatedModules", cases: 8 },
    { name: "jsx", cases: 0 },
    { name: "noCheck", cases: 0 },
    { name: "noEmitHelpers", cases: 0 },
    { name: "outDir", cases: 188 },
    { name: "outFile", cases: 174 },
    { name: "rewriteRelativeImportExtensions", cases: 0 },
    { name: "resolveJsonModule", cases: 0 },
    { name: "sourceMap", cases: 404 },
    { name: "tsBuildInfoFile", cases: 0 },
    { name: "verbatimModuleSyntax", cases: 0 },
  ],
  decisive_blockers: [
    { value: "required-option:target=absent", cases: 620 },
    { value: "required-option:target=ES5(1)", cases: 12 },
  ],
  target_states: [
    { value: "absent", cases: 620 },
    { value: "ES5(1)", cases: 12 },
  ],
  module_states: [
    { value: "AMD(2)", cases: 316 },
    { value: "CommonJS(1)", cases: 316 },
  ],
  root_modes: [
    { value: "explicit-inputs", cases: 570 },
    { value: "project-config", cases: 32 },
    { value: "discovered-config", cases: 30 },
  ],
  dispositions: [{ value: "deferred-profile", cases: 632 }],
  config_diagnostic_cases: 0,
  bootstrap_profile_admitted_cases: 0,
  not_run_cases: 632,
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

function isWithin(root, candidate) {
  const relative = path.relative(root, candidate);
  return (
    relative === "" ||
    (relative !== ".." &&
      !relative.startsWith(`..${path.sep}`) &&
      !path.isAbsolute(relative))
  );
}

function safePath(root, relativePath, label) {
  requireCondition(
    typeof relativePath === "string" &&
      relativePath.length > 0 &&
      !path.isAbsolute(relativePath),
    `unsafe ${label} path ${JSON.stringify(relativePath)}`,
  );
  const absolute = path.resolve(root, ...relativePath.split("/"));
  requireCondition(isWithin(root, absolute), `${label} path escaped its root`);
  return absolute;
}

function validateRuntime() {
  const recorded = fs.readFileSync(NODE_VERSION_PATH, "utf8").trim();
  const running = process.version.startsWith("v")
    ? process.version.slice(1)
    : process.version;
  requireCondition(recorded === EXPECTED_NODE_VERSION, ".node-version changed");
  requireCondition(
    running === EXPECTED_NODE_VERSION,
    `H1 project classification requires Node ${EXPECTED_NODE_VERSION}; running ${process.version}`,
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
      expansion.virtual_source_root === VIRTUAL_ROOT &&
      expansion.summary.project_descriptors === 316 &&
      expansion.summary.project_backing_files === 233 &&
      expansion.summary.project_cases === 632 &&
      expansion.summary.not_run_cases === 7908,
    "project expansion header or frozen counts changed",
  );
  requireJsonEqual(
    IMPLEMENTATION_SOURCES.map(({ source_path, git_blob_sha1 }) => ({
      source_path,
      git_blob_sha1,
    })),
    IMPLEMENTATION_SOURCES.map(({ source_path, git_blob_sha1 }) => {
      const actual = expansion.implementation_sources.find(
        (entry) => entry.source_path === source_path,
      );
      requireCondition(actual !== undefined, `${source_path} pin is absent`);
      return { source_path: actual.source_path, git_blob_sha1: actual.git_blob_sha1 };
    }),
    "project implementation source pins",
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

  const focusedOracle = readJsonInput(
    FOCUSED_ORACLE_RELATIVE_PATH,
    FOCUSED_ORACLE_SHA256,
    "focused project oracle",
  );
  requireCondition(
    focusedOracle.schema === 1 &&
      focusedOracle.typescript_version === TYPESCRIPT_VERSION &&
      focusedOracle.source_commit === SOURCE_COMMIT &&
      focusedOracle.summary.case_total === 6,
    "focused project oracle header changed",
  );
  return { expansion, profile, focusedOracle };
}

function verifySource(root, source) {
  const absolute = safePath(root, source.path, source.suite);
  const raw = fs.readFileSync(absolute);
  requireCondition(
    raw.length === source.bytes &&
      sha256(raw) === source.sha256 &&
      gitBlobSha1(raw) === source.git_blob_sha1,
    `${source.suite}/${source.path} source identity changed`,
  );
  return { absolute, raw };
}

function sourceIndexes(expansion) {
  const bySuitePath = new Map();
  for (const [index, source] of expansion.sources.entries()) {
    bySuitePath.set(`${source.suite}\0${source.path}`, { index, source });
  }
  const projectsByPhysicalPath = new Map();
  for (const [index, source] of expansion.sources.entries()) {
    if (source.suite !== "projects") continue;
    const { absolute } = verifySource(PROJECTS_ROOT, source);
    projectsByPhysicalPath.set(path.normalize(absolute), { index, source });
  }
  requireCondition(
    projectsByPhysicalPath.size === 233,
    "projects mount file count changed",
  );
  return { bySuitePath, projectsByPhysicalPath };
}

function virtualProjectPath(absolutePath) {
  const normalized = path.normalize(absolutePath);
  requireCondition(
    isWithin(PROJECTS_ROOT, normalized),
    `project path escaped the pinned mount: ${absolutePath}`,
  );
  const relative = path.relative(PROJECTS_ROOT, normalized).split(path.sep).join("/");
  return relative === ""
    ? VIRTUAL_PROJECTS_ROOT
    : `${VIRTUAL_PROJECTS_ROOT}/${relative}`;
}

function stableOptionValue(value) {
  if (Array.isArray(value)) return value.map(stableOptionValue);
  if (value instanceof Map) {
    return [...value].map(([key, entry]) => [key, stableOptionValue(entry)]);
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, stableOptionValue(value[key])]),
    );
  }
  if (typeof value === "string" && path.isAbsolute(value)) {
    const normalized = path.normalize(value);
    if (isWithin(PROJECTS_ROOT, normalized)) return virtualProjectPath(normalized);
    return value.split(path.sep).join("/");
  }
  return value;
}

function readDescriptor(expansion, indexes, fixture) {
  const source = expansion.sources[fixture.source];
  requireCondition(source?.suite === "project", "project descriptor source is absent");
  const indexed = indexes.bySuitePath.get(`project\0${source.path}`);
  requireCondition(indexed?.index === fixture.source, "project source index changed");
  const { raw } = verifySource(DESCRIPTOR_ROOT, source);
  requireCondition(fixture.encoding === "utf-8", `${source.path} encoding changed`);
  const testCase = JSON.parse(raw.toString("utf8"));
  requireCondition(
    testCase.scenario === fixture.scenario &&
      testCase.projectRoot === fixture.project_root,
    `${source.path} descriptor identity changed`,
  );
  if (fixture.input_files.state === "absent") {
    requireCondition(testCase.inputFiles === undefined, `${source.path} inputs changed`);
  } else {
    requireJsonEqual(
      testCase.inputFiles,
      fixture.input_files.inputs.map((input) => input.path),
      `${source.path} input files`,
    );
  }
  return { source, testCase };
}

function createRunnerOptions(testCase, moduleVariant) {
  const options = {
    noErrorTruncation: false,
    skipDefaultLibCheck: false,
    moduleResolution: ts.ModuleResolutionKind.Classic,
    module: moduleVariant.value,
    newLine: ts.NewLineKind.CarriageReturnLineFeed,
    mapRoot:
      testCase.resolveMapRoot && testCase.mapRoot
        ? ts.getNormalizedAbsolutePath(testCase.mapRoot, VIRTUAL_ROOT)
        : testCase.mapRoot,
    sourceRoot:
      testCase.resolveSourceRoot && testCase.sourceRoot
        ? ts.getNormalizedAbsolutePath(testCase.sourceRoot, VIRTUAL_ROOT)
        : testCase.sourceRoot,
  };
  const origins = new Map(
    [
      "noErrorTruncation",
      "skipDefaultLibCheck",
      "moduleResolution",
      "module",
      "newLine",
      "mapRoot",
      "sourceRoot",
    ].map((name) => [name, "runner-default"]),
  );
  const optionNameMap = new Map(
    ts.optionDeclarations.map((option) => [option.name, option]),
  );
  for (const name in testCase) {
    if (name === "mapRoot" || name === "sourceRoot") continue;
    const option = optionNameMap.get(name);
    if (!option) continue;
    let value = testCase[name];
    if (typeof option.type !== "string") {
      const converted = option.type.get(value.toLowerCase());
      if (converted) value = converted;
    }
    options[option.name] = value;
    origins.set(option.name, "descriptor");
  }
  if (testCase.mapRoot !== undefined) origins.set("mapRoot", "descriptor");
  if (testCase.sourceRoot !== undefined) origins.set("sourceRoot", "descriptor");
  return { options, origins };
}

function optionOrigin(name, options, origins) {
  if (origins.has(name)) return origins.get(name);
  if (options[name] !== undefined) return "virtual-config";
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

function optionDisplay(projection) {
  if (projection.state === "absent") return "absent";
  return `${projection.name}(${projection.value})`;
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
  const useDefineForClassFields = scalarProjection(
    options.useDefineForClassFields,
    optionOrigin("useDefineForClassFields", options, origins),
  );
  const noEmit = scalarProjection(
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

function sourceRecord(indexes, absolutePath) {
  const indexed = indexes.projectsByPhysicalPath.get(path.normalize(absolutePath));
  requireCondition(indexed !== undefined, `unpinned project path ${absolutePath}`);
  return {
    path: virtualProjectPath(absolutePath),
    source: indexed.index,
    sha256: indexed.source.sha256,
    git_blob_sha1: indexed.source.git_blob_sha1,
  };
}

function explicitRootSelection(fixture) {
  requireCondition(fixture.input_files.state === "present", "explicit inputs absent");
  return {
    state: "explicit-inputs",
    roots: fixture.input_files.inputs.map((input) => ({
      requested: input.path,
      path: `${VIRTUAL_PROJECTS_ROOT}/${input.resolved_backing_path}`,
      presence:
        input.presence.state === "present"
          ? { state: "present", source: input.presence.source }
          : { state: "missing" },
    })),
  };
}

function configRootSelection(
  indexes,
  fixture,
  testCase,
  existingOptions,
  origins,
  state,
) {
  const projectRootPrefix = "tests/cases/projects/";
  requireCondition(
    fixture.project_root.startsWith(projectRootPrefix),
    `${fixture.project_root} is outside projects`,
  );
  const currentPhysical = safePath(
    PROJECTS_ROOT,
    fixture.project_root.slice(projectRootPrefix.length),
    "project root",
  );
  const configRelative =
    state === "project-config"
      ? ts.normalizePath(ts.combinePaths(existingOptions.project, "tsconfig.json"))
      : "tsconfig.json";
  const configPhysical = path.resolve(currentPhysical, configRelative);
  requireCondition(
    isWithin(PROJECTS_ROOT, configPhysical),
    "project config escaped pinned mount",
  );
  const configSourceRecord = sourceRecord(indexes, configPhysical);
  const configSource = ts.readJsonConfigFile(configPhysical, ts.sys.readFile);
  const parsed = ts.parseJsonSourceFileConfigFileContent(
    configSource,
    ts.sys,
    path.dirname(configPhysical),
    existingOptions,
  );
  const diagnostics = [...configSource.parseDiagnostics, ...parsed.errors].map(
    (diagnostic) => diagnostic.code,
  );
  for (const name of Object.keys(parsed.options)) {
    if (!origins.has(name)) origins.set(name, "virtual-config");
  }
  const roots = parsed.fileNames.map((fileName) => {
    const record = sourceRecord(indexes, fileName);
    return { path: record.path, source: record.source };
  });
  return {
    selection: {
      state,
      config: configSourceRecord,
      roots,
      diagnostic_codes: diagnostics,
    },
    options: parsed.options,
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

function rootMode(selection) {
  return selection.state;
}

function buildSummary(cases, fixtureModes, profile) {
  const explicitCases = cases.filter(
    (entry) => entry.root_selection.state === "explicit-inputs",
  );
  const configCases = cases.filter(
    (entry) => entry.root_selection.state !== "explicit-inputs",
  );
  return {
    fixtures: 316,
    explicit_input_fixtures: fixtureModes.filter(
      (state) => state === "explicit-inputs",
    ).length,
    project_config_fixtures: fixtureModes.filter(
      (state) => state === "project-config",
    ).length,
    discovered_config_fixtures: fixtureModes.filter(
      (state) => state === "discovered-config",
    ).length,
    cases: cases.length,
    explicit_input_cases: explicitCases.length,
    config_cases: configCases.length,
    explicit_declared_roots: explicitCases.reduce(
      (total, entry) => total + entry.root_selection.roots.length,
      0,
    ),
    explicit_missing_roots: explicitCases.reduce(
      (total, entry) =>
        total +
        entry.root_selection.roots.filter(
          (root) => root.presence.state === "missing",
        ).length,
      0,
    ),
    config_roots: configCases.reduce(
      (total, entry) => total + entry.root_selection.roots.length,
      0,
    ),
    javascript_observation_applicable_cases: cases.filter(
      (entry) => entry.javascript_observation.applicable,
    ).length,
    required_target_module_matches: cases.filter(
      (entry) =>
        entry.effective_profile.target.state === "set" &&
        entry.effective_profile.target.value === ts.ScriptTarget.ESNext &&
        entry.effective_profile.module.state === "set" &&
        entry.effective_profile.module.value === ts.ModuleKind.Preserve,
    ).length,
    effective_option_clear_cases: cases.filter(
      (entry) => entry.profile_blockers.length === 0,
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
    decisive_blockers: countBy(cases.map((entry) => entry.decisive_blocker)),
    target_states: countBy(
      cases.map((entry) => optionDisplay(entry.effective_profile.target)),
    ),
    module_states: countBy(
      cases.map((entry) => optionDisplay(entry.effective_profile.module)),
    ),
    root_modes: countBy(cases.map((entry) => rootMode(entry.root_selection))),
    dispositions: countBy(cases.map((entry) => entry.disposition)),
    config_diagnostic_cases: configCases.filter(
      (entry) => entry.root_selection.diagnostic_codes.length > 0,
    ).length,
    bootstrap_profile_admitted_cases: cases.filter(
      (entry) => entry.bootstrap_profile_admitted,
    ).length,
    not_run_cases: cases.filter((entry) => entry.execution_state === "not-run")
      .length,
    reference_baselines_compared: 0,
  };
}

function relativeToCurrent(currentDirectory, absolutePath) {
  return path.posix.relative(currentDirectory, absolutePath);
}

function crossCheckFocusedOracle(builtCases, focusedOracle) {
  const byId = new Map(builtCases.map((entry) => [entry.record.id, entry]));
  for (const focused of focusedOracle.cases) {
    const built = byId.get(focused.case_id);
    requireCondition(built !== undefined, `focused case ${focused.case_id} absent`);
    const selection = built.record.root_selection;
    requireCondition(selection.state === "project-config", "focused root mode changed");
    requireCondition(
      relativeToCurrent(built.record.current_directory, selection.config.path) ===
        focused.config.path,
      `${focused.case_id} config path changed`,
    );
    requireCondition(
      selection.config.sha256 === focused.config.sha256 &&
        selection.config.git_blob_sha1 === focused.config.git_blob_sha1,
      `${focused.case_id} config identity changed`,
    );
    requireJsonEqual(
      selection.roots.map((root) =>
        relativeToCurrent(built.record.current_directory, root.path),
      ),
      focused.config.root_names,
      `${focused.case_id} config roots`,
    );
    requireJsonEqual(
      selection.diagnostic_codes,
      focused.config.diagnostics.map((diagnostic) => diagnostic.code),
      `${focused.case_id} config diagnostics`,
    );
    const options = built.options;
    requireCondition(
      (options.allowJs ?? false) === focused.effective_options.allow_js &&
        (options.maxNodeModuleJsDepth ?? null) ===
          focused.effective_options.max_node_module_js_depth &&
        (options.module ?? null) === focused.effective_options.module &&
        (options.moduleResolution ?? null) ===
          focused.effective_options.module_resolution &&
        (options.declaration ?? false) ===
          focused.effective_options.declaration &&
        (options.noErrorTruncation ?? null) ===
          focused.effective_options.no_error_truncation &&
        (options.skipDefaultLibCheck ?? null) ===
          focused.effective_options.skip_default_lib_check &&
        (options.noEmit ?? null) === focused.effective_options.raw_no_emit,
      `${focused.case_id} focused effective options changed`,
    );
    const outDir =
      options.outDir === undefined
        ? null
        : relativeToCurrent(
            built.record.current_directory,
            virtualProjectPath(options.outDir),
          );
    requireCondition(
      outDir === focused.effective_options.out_dir,
      `${focused.case_id} outDir changed`,
    );
  }
}

function buildArtifact(expansion, profile, focusedOracle) {
  const indexes = sourceIndexes(expansion);
  const cases = [];
  const builtCases = [];
  const fixtureModes = [];
  let projectCase = 0;
  for (const fixture of expansion.project_fixtures) {
    const { source, testCase } = readDescriptor(expansion, indexes, fixture);
    const fixtureMode =
      typeof testCase.project === "string" && testCase.project.length > 0
        ? "project-config"
        : Array.isArray(testCase.inputFiles) && testCase.inputFiles.length > 0
          ? "explicit-inputs"
          : "discovered-config";
    fixtureModes.push(fixtureMode);
    for (const moduleVariant of MODULE_VARIANTS) {
      const expansionCase = 7276 + projectCase;
      const recorded = expansion.cases[expansionCase];
      requireCondition(
        recorded.suite === "project" &&
          recorded.source === fixture.source &&
          recorded.configuration.kind === "project" &&
          recorded.configuration.module === moduleVariant.name &&
          recorded.configuration.baseline_folder ===
            moduleVariant.baseline_folder &&
          recorded.initial_execution_state === "not-run",
        `project expansion case ${expansionCase} changed`,
      );
      const runner = createRunnerOptions(testCase, moduleVariant);
      let options = runner.options;
      let rootSelection;
      if (fixtureMode === "explicit-inputs") {
        rootSelection = explicitRootSelection(fixture);
      } else {
        const config = configRootSelection(
          indexes,
          fixture,
          testCase,
          options,
          runner.origins,
          fixtureMode,
        );
        rootSelection = config.selection;
        options = config.options;
      }
      const classification = classifyOptions(options, runner.origins, profile);
      requireCondition(
        classification.blockers.some((blocker) =>
          blocker.startsWith("required-option:module="),
        ),
        `${recorded.id} lost its project-runner module blocker`,
      );
      const admitted = classification.blockers.length === 0;
      const record = {
        project_case: projectCase,
        expansion_case: expansionCase,
        id: recorded.id,
        source: fixture.source,
        descriptor_path: source.path,
        module_variant: moduleVariant,
        current_directory: `${VIRTUAL_ROOT}/${fixture.project_root}`,
        root_selection: rootSelection,
        effective_profile: classification.projection,
        source_analysis: { state: "not-required-effective-options" },
        javascript_observation: {
          applicable: testCase.baselineCheck === true,
          execution_state: "not-run",
          reference_baseline_state: REFERENCE_BASELINE_STATE,
        },
        bootstrap_profile_admitted: admitted,
        disposition: admitted ? "bootstrap-candidate-not-run" : "deferred-profile",
        decisive_blocker: classification.blockers[0] ?? null,
        profile_blockers: classification.blockers,
        execution_state: "not-run",
        reference_baseline_state: REFERENCE_BASELINE_STATE,
      };
      cases.push(record);
      builtCases.push({ record, options });
      projectCase += 1;
    }
  }
  requireCondition(projectCase === 632, "project case count changed");
  crossCheckFocusedOracle(builtCases, focusedOracle);
  const summary = buildSummary(cases, fixtureModes, profile);
  requireJsonEqual(summary, EXPECTED_SUMMARY, "project classification summary");
  return {
    schema: 1,
    status: "classified-not-run",
    phase: "H1.0a-project-profile-classification",
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
      h1_profile: { path: PROFILE_RELATIVE_PATH, sha256: PROFILE_SHA256 },
      typescript_bundle: {
        path: TYPESCRIPT_BUNDLE_RELATIVE_PATH,
        sha256: TYPESCRIPT_BUNDLE_SHA256,
      },
      focused_project_oracle: {
        path: FOCUSED_ORACLE_RELATIVE_PATH,
        sha256: FOCUSED_ORACLE_SHA256,
      },
      implementation_sources: IMPLEMENTATION_SOURCES,
    },
    classification_contract: {
      runner_option_order:
        "ProjectRunner defaults, descriptor compiler options, then virtual tsconfig with existing runner options winning",
      root_selection_order:
        "project option, else nonempty inputFiles, else discover tsconfig.json",
      admission_proof:
        "every CommonJS/AMD project-runner row has required target and module blockers before source reachability",
      required_options: ["target=ESNext(99)", "module=Preserve(200)"],
      admitted_products: ["javascript"],
      source_analysis: "not required by the zero-admission option proof",
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
      artifact.phase === "H1.0a-project-profile-classification",
    "invalid project classification header",
  );
  requireCondition(
    artifact.summary.cases === artifact.cases.length &&
      artifact.summary.not_run_cases === artifact.cases.length &&
      artifact.summary.bootstrap_profile_admitted_cases === 0 &&
      artifact.summary.reference_baselines_compared === 0,
    "invalid project classification closure",
  );
}

validateRuntime();
const { expansion, profile, focusedOracle } = readInputs();
const artifact = buildArtifact(expansion, profile, focusedOracle);
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
    `H1 project profile classification is fresh: cases=${artifact.summary.cases} configs=${artifact.summary.config_cases} admitted=${artifact.summary.bootstrap_profile_admitted_cases} status=not-run\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h1-project-classification.mjs [--write|--check]");
}
