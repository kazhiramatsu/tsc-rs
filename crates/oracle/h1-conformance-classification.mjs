import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH =
  "crates/oracle/h1-conformance-classification.mjs";
const TARGET_RELATIVE_PATH =
  "vendor/typescript-6.0.3/conformance-profile-classification.v1.json";
const TARGET_PATH = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h1-conformance-classification.schema.json";
const EXPANSION_RELATIVE_PATH =
  "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json";
const PROFILE_RELATIVE_PATH = "ratchets/h1-emit-profile.v1.json";
const TYPESCRIPT_BUNDLE_RELATIVE_PATH =
  "vendor/typescript-6.0.3/lib/typescript.js";
const SOURCE_ROOT = path.join(
  WORKSPACE,
  "ts-tests/tests/cases/conformance",
);
const NODE_VERSION_PATH = path.join(WORKSPACE, ".node-version");
const TYPESCRIPT_VERSION = "6.0.3";
const SOURCE_REPOSITORY = "https://github.com/microsoft/TypeScript.git";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE_VERSION = "25.2.1";
const EXPANSION_SHA256 =
  "924d4007b3ac93a3ee57032ea6089b649bab2902e30ee64cff02f4c9404b7bbd";
const PROFILE_SHA256 =
  "91e05db331a090e180e9cda7fc8eaa505d795b229a49d78d62d1e086c8602991";
const TYPESCRIPT_BUNDLE_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const VIRTUAL_SOURCE_ROOT = "/.src";
const JAVASCRIPT_OBSERVATION_INDEX = 3;
const REFERENCE_BASELINE_STATE = "content-not-vendored-or-compared";

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
];

const HARNESS_ONLY_OPTIONS = new Set(
  [
    "useCaseSensitiveFileNames",
    "baselineFile",
    "fileName",
    "suppressOutputPathCheck",
    "noImplicitReferences",
    "currentDirectory",
    "symlink",
    "link",
    "noTypesAndSymbols",
    "fullEmitPaths",
    "reportDiagnostics",
    "captureSuggestions",
  ].map((name) => name.toLowerCase()),
);

const OPTION_LINE_PATTERN = /^\/{2}\s*@(\w+)\s*:\s*([^\r\n]*)/;
const LINK_LINE_PATTERN = /^\/{2}\s*@link\s*:/;
const EXPECTED_SUMMARY = {
  fixtures: 5907,
  virtual_config_fixtures: 27,
  virtual_config_diagnostic_fixtures: 2,
  cases: 7697,
  javascript_observation_applicable_cases: 7655,
  required_target_module_matches: 3,
  cases_with_target_blocker: 7152,
  cases_with_module_blocker: 7678,
  cases_with_use_define_for_class_fields_blocker: 91,
  cases_with_no_emit_route: 547,
  cases_with_rejected_effective_options: 2483,
  rejected_option_cases: [
    { name: "allowImportingTsExtensions", cases: 10 },
    { name: "allowJs", cases: 694 },
    { name: "composite", cases: 0 },
    { name: "declaration", cases: 861 },
    { name: "declarationDir", cases: 1 },
    { name: "declarationMap", cases: 5 },
    { name: "emitDeclarationOnly", cases: 49 },
    { name: "experimentalDecorators", cases: 291 },
    { name: "importHelpers", cases: 74 },
    { name: "incremental", cases: 0 },
    { name: "inlineSourceMap", cases: 0 },
    { name: "isolatedModules", cases: 17 },
    { name: "jsx", cases: 257 },
    { name: "noCheck", cases: 0 },
    { name: "noEmitHelpers", cases: 630 },
    { name: "outDir", cases: 431 },
    { name: "outFile", cases: 26 },
    { name: "rewriteRelativeImportExtensions", cases: 16 },
    { name: "resolveJsonModule", cases: 24 },
    { name: "sourceMap", cases: 32 },
    { name: "tsBuildInfoFile", cases: 0 },
    { name: "verbatimModuleSyntax", cases: 22 },
  ],
  decisive_blockers: [
    { value: "required-option:target=ES2015(2)", cases: 5485 },
    { value: "required-option:target=ES5(1)", cases: 883 },
    { value: "required-option:target=ES2022(9)", cases: 579 },
    { value: "required-option:module=absent", cases: 321 },
    { value: "required-option:module=ESNext(99)", cases: 111 },
    { value: "required-option:target=ES2017(4)", cases: 80 },
    { value: "required-option:target=ES2020(7)", cases: 55 },
    { value: "required-option:module=CommonJS(1)", cases: 49 },
    { value: "required-option:target=ES2018(5)", cases: 31 },
    { value: "required-option:module=System(4)", cases: 30 },
    { value: "required-option:target=ES2021(8)", cases: 18 },
    { value: "required-option:module=ES2022(7)", cases: 14 },
    { value: "required-option:target=ES2019(6)", cases: 10 },
    { value: "required-option:module=AMD(2)", cases: 7 },
    { value: "required-option:module=UMD(3)", cases: 5 },
    { value: "required-option:target=ES2023(10)", cases: 5 },
    { value: "required-option:target=ES2016(3)", cases: 4 },
    { value: "required-option:module=NodeNext(199)", cases: 3 },
    { value: "rejected-option:allowJs", cases: 2 },
    { value: "required-option:module=ES2015(5)", cases: 1 },
    { value: "required-option:module=ES2020(6)", cases: 1 },
    { value: "required-option:target=ES2024(11)", cases: 1 },
    { value: "required-option:target=ES2025(12)", cases: 1 },
    { value: "route:noEmit=true", cases: 1 },
  ],
  target_states: [
    { value: "ES2015(2)", cases: 5485 },
    { value: "ES5(1)", cases: 883 },
    { value: "ES2022(9)", cases: 579 },
    { value: "ESNext(99)", cases: 545 },
    { value: "ES2017(4)", cases: 80 },
    { value: "ES2020(7)", cases: 55 },
    { value: "ES2018(5)", cases: 31 },
    { value: "ES2021(8)", cases: 18 },
    { value: "ES2019(6)", cases: 10 },
    { value: "ES2023(10)", cases: 5 },
    { value: "ES2016(3)", cases: 4 },
    { value: "ES2024(11)", cases: 1 },
    { value: "ES2025(12)", cases: 1 },
  ],
  module_states: [
    { value: "absent", cases: 6005 },
    { value: "CommonJS(1)", cases: 625 },
    { value: "ESNext(99)", cases: 330 },
    { value: "System(4)", cases: 114 },
    { value: "NodeNext(199)", cases: 105 },
    { value: "Node20(102)", cases: 101 },
    { value: "Node18(101)", cases: 97 },
    { value: "Node16(100)", cases: 95 },
    { value: "AMD(2)", cases: 78 },
    { value: "ES2015(5)", cases: 56 },
    { value: "UMD(3)", cases: 28 },
    { value: "ES2020(6)", cases: 22 },
    { value: "ES2022(7)", cases: 21 },
    { value: "Preserve(200)", cases: 19 },
    { value: "None(0)", cases: 1 },
  ],
  bootstrap_profile_admitted_cases: 0,
  deferred_effective_option_cases: 7697,
  not_run_cases: 7697,
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
    `H1 conformance classification requires Node ${EXPECTED_NODE_VERSION}; running ${process.version}`,
  );
  requireCondition(ts.version === TYPESCRIPT_VERSION, "TypeScript version changed");
  requireCondition(
    pathHash(TYPESCRIPT_BUNDLE_RELATIVE_PATH).sha256 ===
      TYPESCRIPT_BUNDLE_SHA256,
    "vendored TypeScript bundle changed",
  );
}

function readInputs() {
  const expansionBytes = fs.readFileSync(
    path.join(WORKSPACE, EXPANSION_RELATIVE_PATH),
  );
  requireCondition(
    sha256(expansionBytes) === EXPANSION_SHA256,
    "conformance expansion hash changed",
  );
  const expansion = JSON.parse(expansionBytes.toString("utf8"));
  requireCondition(
    expansion.schema === 1 &&
      expansion.status === "expanded-not-run" &&
      expansion.phase === "H1.0a-conformance-runner-expansion" &&
      expansion.summary.source_files === 5908 &&
      expansion.summary.enumerated_fixtures === 5907 &&
      expansion.summary.cases === 7697 &&
      expansion.summary.not_run_cases === 7697,
    "conformance expansion header or frozen counts changed",
  );

  const profileBytes = fs.readFileSync(
    path.join(WORKSPACE, PROFILE_RELATIVE_PATH),
  );
  requireCondition(
    sha256(profileBytes) === PROFILE_SHA256,
    "H1 bootstrap profile hash changed",
  );
  const profile = JSON.parse(profileBytes.toString("utf8"));
  requireCondition(
    profile.schema === 1 &&
      profile.status === "frozen" &&
      profile.phase === "H1.0a-bootstrap-profile",
    "H1 bootstrap profile header changed",
  );
  requireJsonEqual(
    profile.emit_active_options.required,
    [
      { name: "target", accepted: [{ name: "ESNext", value: 99 }] },
      { name: "module", accepted: [{ name: "Preserve", value: 200 }] },
    ],
    "required bootstrap options",
  );
  return { expansion, profile };
}

function decodeSource(raw, recordedEncoding) {
  if (recordedEncoding === "utf-8-bom") {
    requireCondition(
      raw.subarray(0, 3).equals(Buffer.from([0xef, 0xbb, 0xbf])),
      "recorded UTF-8 BOM is absent",
    );
    return raw.subarray(3).toString("utf8");
  }
  requireCondition(recordedEncoding === "utf-8", "unexpected conformance encoding");
  return raw.toString("utf8");
}

function orderedSettings(map) {
  return [...map].map(([name, value]) => ({ name, value }));
}

function makeUnits(text, fixturePath) {
  const units = [];
  let currentContent;
  let currentOptions = new Map();
  let currentName;
  for (const line of text.split(/\r?\n/)) {
    requireCondition(!LINK_LINE_PATTERN.test(line), `${fixturePath} unexpectedly uses @link`);
    const metadata = OPTION_LINE_PATTERN.exec(line);
    if (metadata) {
      currentOptions.set(metadata[1], metadata[2].trim());
      if (metadata[1].toLowerCase() !== "filename") continue;
      if (currentName) {
        units.push({
          name: currentName,
          file_options: orderedSettings(currentOptions),
          text: currentContent,
        });
        currentContent = undefined;
        currentOptions = new Map();
        currentName = metadata[2].trim();
      } else {
        currentName = metadata[2].trim();
        if (currentContent) {
          requireCondition(
            ts.skipTrivia(currentContent, 0, false, false) ===
              currentContent.length,
            `${fixturePath} has non-comment content before first @filename`,
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
    file_options: orderedSettings(currentOptions),
    text: currentContent || "",
  });
  return units;
}

function isConfigName(fileName) {
  const basename = path.posix.basename(fileName).toLowerCase();
  return basename === "tsconfig.json" || basename === "jsconfig.json";
}

function recordedContentIdentity(unit) {
  const bytes = Buffer.from(unit.text ?? "");
  return {
    state: unit.text === undefined ? "missing" : "present",
    ...(unit.text === undefined
      ? {}
      : { utf8_bytes: bytes.length, sha256: sha256(bytes) }),
  };
}

function verifyAndPartitionUnits(parsedUnits, fixture, fixturePath) {
  const configIndex = parsedUnits.findIndex((unit) => isConfigName(unit.name));
  const configUnit =
    configIndex >= 0 ? parsedUnits.splice(configIndex, 1)[0] : undefined;
  requireCondition(
    Boolean(configUnit) === Boolean(fixture.virtual_config),
    `${fixturePath} virtual config partition changed`,
  );
  const verifyUnit = (actual, recorded, description) => {
    requireCondition(recorded !== undefined, `${description} is absent from expansion`);
    requireCondition(actual.name === recorded.name, `${description} name changed`);
    requireJsonEqual(
      actual.file_options,
      recorded.file_options,
      `${description} file options`,
    );
    requireJsonEqual(
      recordedContentIdentity(actual),
      recorded.content,
      `${description} content identity`,
    );
  };
  requireCondition(
    parsedUnits.length === fixture.normal_units.length,
    `${fixturePath} normal unit count changed`,
  );
  parsedUnits.forEach((unit, index) =>
    verifyUnit(unit, fixture.normal_units[index], `${fixturePath} unit ${index}`),
  );
  if (configUnit) {
    verifyUnit(configUnit, fixture.virtual_config, `${fixturePath} virtual config`);
  }
  return { normalUnits: parsedUnits, configUnit };
}

function parseVirtualConfig(normalUnits, configUnit) {
  if (!configUnit) {
    return {
      options: { noResolve: false },
      diagnostic_codes: [],
      file_names: [],
    };
  }
  const allUnits = [...normalUnits, configUnit];
  const parseConfigHost = {
    useCaseSensitiveFileNames: false,
    readDirectory: (directory, extensions, excludes, includes, depth) =>
      ts.matchFiles(
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
          for (const unit of allUnits) {
            const fileName = ts.getNormalizedAbsolutePath(
              unit.name,
              VIRTUAL_SOURCE_ROOT,
            );
            if (fileName.toLowerCase().startsWith(dir.toLowerCase())) {
              let suffix = fileName.substring(dir.length);
              if (suffix.startsWith("/")) suffix = suffix.substring(1);
              if (suffix.includes("/")) {
                directories.add(suffix.substring(0, suffix.indexOf("/")));
              } else files.push(suffix);
            }
          }
          return { files, directories: ts.arrayFrom(directories) };
        },
        ts.identity,
      ),
    fileExists: (fileName) =>
      allUnits.some(
        (unit) => unit.name.toLowerCase() === fileName.toLowerCase(),
      ),
    readFile: (fileName) =>
      allUnits.find(
        (unit) => unit.name.toLowerCase() === fileName.toLowerCase(),
      )?.text,
  };
  const configJson = ts.parseJsonText(configUnit.name, configUnit.text);
  const configFileName = ts.getNormalizedAbsolutePath(
    configUnit.name,
    VIRTUAL_SOURCE_ROOT,
  );
  const configDirectory = ts.getDirectoryPath(configFileName);
  const parsed = ts.parseJsonSourceFileConfigFileContent(
    configJson,
    parseConfigHost,
    configDirectory,
    undefined,
    configFileName,
  );
  return {
    options: ts.cloneCompilerOptions(parsed.options),
    diagnostic_codes: [
      ...new Set(parsed.errors.map((diagnostic) => diagnostic.code)),
    ].sort((left, right) => left - right),
    file_names: [...parsed.fileNames],
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
  else if (option.type === "number") {
    value = Number.parseInt(raw, 10);
    requireCondition(Number.isFinite(value), `invalid numeric @${option.name}`);
  } else if (option.type === "list" || option.type === "listOrElement") {
    value = ts.parseListTypeOption(option, raw, errors);
  } else value = ts.parseCustomTypeOption(option, raw, errors);
  requireCondition(errors.length === 0, `invalid value ${raw} for @${option.name}`);
  return value;
}

function mergedSettings(baseSettings, overrides) {
  const settings = new Map(
    baseSettings.map((setting) => [setting.name, setting.value]),
  );
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

function enumProjection(value, canonicalNames, origin) {
  if (value === undefined) return { state: "absent" };
  return {
    state: "set",
    name: canonicalNames.get(value) ?? `unknown-${value}`,
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
  requireCondition(
    blockers.length > 0,
    "conformance case unexpectedly requires syntax/output classification for admission",
  );
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

function countBy(values) {
  const counts = new Map();
  for (const value of values) counts.set(value, (counts.get(value) ?? 0) + 1);
  return [...counts]
    .map(([value, cases]) => ({ value, cases }))
    .sort((left, right) =>
      right.cases - left.cases ||
      (left.value < right.value ? -1 : left.value > right.value ? 1 : 0),
    );
}

function buildSummary(fixtures, cases, profile) {
  const rejectedOptionCases = profile.emit_active_options.rejected_when_effective
    .map((name) => ({
      name,
      cases: cases.filter((entry) =>
        entry.effective_profile.rejected_when_effective.some(
          (rejected) => rejected.name === name,
        ),
      ).length,
    }));
  return {
    fixtures: fixtures.length,
    virtual_config_fixtures: fixtures.filter(
      (fixture) => fixture.virtual_config.present,
    ).length,
    virtual_config_diagnostic_fixtures: fixtures.filter(
      (fixture) => fixture.virtual_config.diagnostic_codes.length > 0,
    ).length,
    cases: cases.length,
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
    rejected_option_cases: rejectedOptionCases,
    decisive_blockers: countBy(cases.map((entry) => entry.decisive_blocker)),
    target_states: countBy(
      cases.map((entry) => optionDisplay(entry.effective_profile.target)),
    ),
    module_states: countBy(
      cases.map((entry) => optionDisplay(entry.effective_profile.module)),
    ),
    bootstrap_profile_admitted_cases: cases.filter(
      (entry) => entry.bootstrap_profile_admitted,
    ).length,
    deferred_effective_option_cases: cases.filter(
      (entry) => entry.disposition === "deferred-effective-options",
    ).length,
    not_run_cases: cases.filter(
      (entry) => entry.javascript_observation.execution_state === "not-run",
    ).length,
    reference_baselines_compared: 0,
  };
}

function buildArtifact(expansion, profile) {
  const fixtures = [];
  const cases = [];
  let caseOffset = 0;
  for (const fixture of expansion.fixtures) {
    const source = expansion.sources[fixture.source];
    const raw = fs.readFileSync(
      path.join(SOURCE_ROOT, ...source.path.split("/")),
    );
    requireCondition(raw.length === source.bytes, `${source.path} byte count changed`);
    requireCondition(sha256(raw) === source.sha256, `${source.path} hash changed`);
    const text = decodeSource(raw, fixture.encoding);
    requireCondition(
      Buffer.byteLength(text) === fixture.decoded_utf8_bytes &&
        sha256(Buffer.from(text)) === fixture.decoded_sha256,
      `${source.path} decoded identity changed`,
    );
    const partition = verifyAndPartitionUnits(
      makeUnits(text, source.path),
      fixture,
      source.path,
    );
    const config = parseVirtualConfig(
      partition.normalUnits,
      partition.configUnit,
    );
    const javascriptApplicable = partition.normalUnits.some(
      (unit) => !ts.fileExtensionIs(unit.name, ts.Extension.Dts),
    );
    fixtures.push({
      source: fixture.source,
      javascript_observation_applicable: javascriptApplicable,
      virtual_config: {
        present: Boolean(partition.configUnit),
        diagnostic_codes: config.diagnostic_codes,
        file_names: config.file_names,
      },
    });

    for (const [configurationIndex, configuration] of fixture.configurations.entries()) {
      const expansionCase = expansion.cases[caseOffset];
      requireCondition(
        expansionCase.source === fixture.source &&
          expansionCase.configuration === configurationIndex &&
          expansionCase.initial_execution_state === "not-run" &&
          expansionCase.reference_baseline_state === REFERENCE_BASELINE_STATE &&
          expansionCase.observations[JAVASCRIPT_OBSERVATION_INDEX] ===
            JAVASCRIPT_OBSERVATION_INDEX,
        `expansion case ${caseOffset} changed`,
      );
      const settings = mergedSettings(
        fixture.settings,
        configuration.settings,
      );
      const classification = classifyOptions(config.options, settings, profile);
      cases.push({
        expansion_case: caseOffset,
        id: expansionCase.id,
        source: fixture.source,
        configuration: configurationIndex,
        effective_profile: classification.projection,
        javascript_observation: {
          index: JAVASCRIPT_OBSERVATION_INDEX,
          applicable: javascriptApplicable,
          execution_state: "not-run",
          reference_baseline_state: REFERENCE_BASELINE_STATE,
        },
        bootstrap_profile_admitted: false,
        disposition: "deferred-effective-options",
        decisive_blocker: classification.blockers[0],
        profile_blockers: classification.blockers,
      });
      caseOffset += 1;
    }
  }
  requireCondition(
    caseOffset === expansion.cases.length,
    "not every conformance expansion case was classified",
  );
  const summary = buildSummary(fixtures, cases, profile);
  if (EXPECTED_SUMMARY !== undefined) {
    requireJsonEqual(summary, EXPECTED_SUMMARY, "conformance classification summary");
  }
  return {
    schema: 1,
    status: "classified-not-run",
    phase: "H1.0a-conformance-profile-classification",
    typescript: {
      version: TYPESCRIPT_VERSION,
      source_repository: SOURCE_REPOSITORY,
      source_commit: SOURCE_COMMIT,
    },
    generator: pathHash(GENERATOR_RELATIVE_PATH),
    contract: pathHash(CONTRACT_RELATIVE_PATH),
    inputs: {
      conformance_expansion: {
        path: EXPANSION_RELATIVE_PATH,
        sha256: EXPANSION_SHA256,
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
      admission_proof:
        "every case has at least one effective-option blocker before source reachability or syntax classification",
      required_options: ["target=ESNext(99)", "module=Preserve(200)"],
      javascript_observation_index: JAVASCRIPT_OBSERVATION_INDEX,
      non_javascript_observations:
        "remain deferred and not-run outside bootstrap JavaScript acceptance",
      syntax_classification:
        "not required for the zero-admission proof and not claimed by this artifact",
      reference_baseline_state: REFERENCE_BASELINE_STATE,
    },
    fixtures,
    cases,
    summary,
  };
}

function validateArtifact(artifact) {
  requireCondition(
    artifact.schema === 1 &&
      artifact.status === "classified-not-run" &&
      artifact.phase === "H1.0a-conformance-profile-classification",
    "invalid conformance classification header",
  );
  requireCondition(
    artifact.summary.cases === artifact.cases.length &&
      artifact.summary.fixtures === artifact.fixtures.length &&
      artifact.summary.bootstrap_profile_admitted_cases === 0 &&
      artifact.summary.deferred_effective_option_cases === artifact.cases.length &&
      artifact.summary.not_run_cases === artifact.cases.length &&
      artifact.summary.reference_baselines_compared === 0,
    "invalid conformance classification closure",
  );
}

validateRuntime();
const { expansion, profile } = readInputs();
const artifact = buildArtifact(expansion, profile);
validateArtifact(artifact);
const rendered = `${JSON.stringify(artifact, null, 2)}\n`;
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(TARGET_PATH, rendered);
  process.stdout.write(`wrote ${TARGET_RELATIVE_PATH}\n`);
} else if (mode === "--check") {
  requireCondition(
    fs.existsSync(TARGET_PATH) &&
      fs.readFileSync(TARGET_PATH, "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run with --write and review`,
  );
  process.stdout.write(
    `H1 conformance profile classification is fresh: cases=${artifact.summary.cases} admitted=0 deferred=${artifact.summary.deferred_effective_option_cases} status=not-run\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h1-conformance-classification.mjs [--write|--check]");
}
