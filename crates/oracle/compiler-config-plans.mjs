import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

let ts;

const DRIVER_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE = path.resolve(DRIVER_DIRECTORY, "../..");
const NODE_VERSION_PATH = path.join(WORKSPACE, ".node-version");
const MANIFEST_RELATIVE_PATH =
  "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const MANIFEST_PATH = path.join(WORKSPACE, MANIFEST_RELATIVE_PATH);
const ARTIFACT_RELATIVE_PATH =
  "vendor/typescript-6.0.3/compiler-config-plans.v1.json";
const ARTIFACT_PATH = path.join(WORKSPACE, ARTIFACT_RELATIVE_PATH);
const TYPESCRIPT_BUNDLE_RELATIVE_PATH =
  "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_BUNDLE_PATH = path.join(
  WORKSPACE,
  TYPESCRIPT_BUNDLE_RELATIVE_PATH,
);

const EXPECTED_NODE_VERSION = "25.2.1";
const EXPECTED_TYPESCRIPT_VERSION = "6.0.3";
const EXPECTED_SOURCE_COMMIT =
  "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_MANIFEST_SHA256 =
  "9c6e991103b571f7a8800dc5e1ef66088017689f3769de8fcdb408b2dc125188";
const EXPECTED_TYPESCRIPT_BUNDLE_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const EXPECTED_COMPILER_VENDOR_PATH = "ts-tests/tests/cases/compiler";
const VIRTUAL_SOURCE_ROOT = "/.src";

const EXPECTED_SUMMARY = {
  config_plans: { fixture_total: 103, case_total: 106 },
  candidate_units: { fixture_total: 300, case_total: 306 },
  parsed_file_names: { fixture_total: 167, case_total: 170 },
  extended_sources: { fixture_total: 5, case_total: 5 },
  root_units: { fixture_total: 167, case_total: 170 },
  other_units: { fixture_total: 133, case_total: 136 },
  program_root_units: { fixture_total: 167, case_total: 170 },
  parsed_diagnostics: { fixture_total: 0, case_total: 0 },
  distributions: {
    configurations_per_fixture: [
      { configurations: 1, fixtures: 102, cases: 102 },
      { configurations: 4, fixtures: 1, cases: 4 },
    ],
    config_occurrence: [
      { occurrence: 0, fixtures: 66, cases: 69 },
      { occurrence: 1, fixtures: 9, cases: 9 },
      { occurrence: 2, fixtures: 11, cases: 11 },
      { occurrence: 3, fixtures: 10, cases: 10 },
      { occurrence: 4, fixtures: 1, cases: 1 },
      { occurrence: 5, fixtures: 2, cases: 2 },
      { occurrence: 8, fixtures: 2, cases: 2 },
      { occurrence: 9, fixtures: 2, cases: 2 },
    ],
    candidate_units_per_fixture: [
      { units: 1, fixtures: 27, cases: 27 },
      { units: 2, fixtures: 22, cases: 25 },
      { units: 3, fixtures: 28, cases: 28 },
      { units: 4, fixtures: 7, cases: 7 },
      { units: 5, fixtures: 10, cases: 10 },
      { units: 6, fixtures: 4, cases: 4 },
      { units: 8, fixtures: 2, cases: 2 },
      { units: 9, fixtures: 3, cases: 3 },
    ],
    root_units_per_fixture: [
      { units: 1, fixtures: 59, cases: 62 },
      { units: 2, fixtures: 27, cases: 27 },
      { units: 3, fixtures: 14, cases: 14 },
      { units: 4, fixtures: 3, cases: 3 },
    ],
    other_units_per_fixture: [
      { units: 0, fixtures: 50, cases: 50 },
      { units: 1, fixtures: 20, cases: 23 },
      { units: 2, fixtures: 13, cases: 13 },
      { units: 3, fixtures: 7, cases: 7 },
      { units: 4, fixtures: 7, cases: 7 },
      { units: 5, fixtures: 1, cases: 1 },
      { units: 6, fixtures: 3, cases: 3 },
      { units: 7, fixtures: 1, cases: 1 },
      { units: 8, fixtures: 1, cases: 1 },
    ],
    program_root_units_per_fixture: [
      { units: 1, fixtures: 59, cases: 62 },
      { units: 2, fixtures: 27, cases: 27 },
      { units: 3, fixtures: 14, cases: 14 },
      { units: 4, fixtures: 3, cases: 3 },
    ],
  },
};

// These are the two regular expressions used by harnessIO.ts. Both are global,
// so every single-line probe must restore lastIndex before the next probe.
const OPTION_REGEX = /^\/{2}\s*@(\w+)\s*:\s*([^\r\n]*)/gm;
const LINK_REGEX = /^\/{2}\s*@link\s*:\s*([^\r\n]*)\s*->\s*([^\r\n]*)/gm;

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

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function jsonEqual(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function requireJsonEqual(actual, expected, label) {
  requireCondition(
    jsonEqual(actual, expected),
    `${label} mismatch:\nactual: ${JSON.stringify(actual)}\nexpected: ${JSON.stringify(expected)}`,
  );
}

function normalizeVersion(version) {
  return version.startsWith("v") ? version.slice(1) : version;
}

function validateRuntime() {
  const recordedNodeVersion = fs.readFileSync(NODE_VERSION_PATH, "utf8").trim();
  requireCondition(
    recordedNodeVersion === EXPECTED_NODE_VERSION,
    `.node-version is ${JSON.stringify(recordedNodeVersion)}; expected ${EXPECTED_NODE_VERSION}`,
  );
  const runningNodeVersion = normalizeVersion(process.version);
  requireCondition(
    runningNodeVersion === recordedNodeVersion,
    `compiler config oracle requires Node ${recordedNodeVersion}; running ${runningNodeVersion}`,
  );
  const bundleHash = sha256(fs.readFileSync(TYPESCRIPT_BUNDLE_PATH));
  requireCondition(
    bundleHash === EXPECTED_TYPESCRIPT_BUNDLE_SHA256,
    `${TYPESCRIPT_BUNDLE_RELATIVE_PATH} SHA-256 is ${bundleHash}; expected ${EXPECTED_TYPESCRIPT_BUNDLE_SHA256}`,
  );
}

function validateTypeScriptRuntime() {
  requireCondition(
    ts?.version === EXPECTED_TYPESCRIPT_VERSION,
    `vendored TypeScript reports ${ts?.version}; expected ${EXPECTED_TYPESCRIPT_VERSION}`,
  );
}

function readManifest() {
  const bytes = fs.readFileSync(MANIFEST_PATH);
  const actualHash = sha256(bytes);
  requireCondition(
    actualHash === EXPECTED_MANIFEST_SHA256,
    `${MANIFEST_RELATIVE_PATH} SHA-256 is ${actualHash}; expected ${EXPECTED_MANIFEST_SHA256}`,
  );
  const manifest = JSON.parse(bytes.toString("utf8"));
  requireCondition(manifest.schema === 1, "test suite expansion manifest schema must be 1");
  requireCondition(
    manifest.typescript_version === EXPECTED_TYPESCRIPT_VERSION,
    "test suite expansion TypeScript version does not match the oracle pin",
  );
  requireCondition(
    manifest.source_commit === EXPECTED_SOURCE_COMMIT,
    "test suite expansion source commit does not match the oracle pin",
  );
  requireCondition(
    manifest.virtual_source_root === VIRTUAL_SOURCE_ROOT,
    `test suite expansion virtual root must be ${VIRTUAL_SOURCE_ROOT}`,
  );
  requireCondition(
    Array.isArray(manifest.sources) && Array.isArray(manifest.compiler_fixtures),
    "test suite expansion manifest is missing compiler inventories",
  );
  requireCondition(
    manifest.summary?.compiler_virtual_configs === 103,
    "test suite expansion manifest must record 103 virtual compiler configs",
  );

  const compilerSuites = manifest.corpus_pin?.suites?.filter(
    (suite) => suite.name === "compiler",
  );
  requireCondition(
    compilerSuites?.length === 1,
    "test suite expansion manifest must pin exactly one compiler suite",
  );
  requireCondition(
    compilerSuites[0].vendored_path === EXPECTED_COMPILER_VENDOR_PATH,
    `compiler suite vendor path must be ${EXPECTED_COMPILER_VENDOR_PATH}`,
  );
  return {
    manifest,
    compilerRoot: path.join(WORKSPACE, compilerSuites[0].vendored_path),
  };
}

function safeSourcePath(compilerRoot, relativePath) {
  requireCondition(
    typeof relativePath === "string" && relativePath.length > 0,
    "compiler source path must be a non-empty string",
  );
  requireCondition(
    relativePath === path.posix.normalize(relativePath) &&
      !relativePath.startsWith("/") &&
      !relativePath.startsWith("../"),
    `compiler source path is not normalized and relative: ${JSON.stringify(relativePath)}`,
  );
  const resolved = path.resolve(compilerRoot, relativePath);
  requireCondition(
    resolved.startsWith(`${path.resolve(compilerRoot)}${path.sep}`),
    `compiler source path escaped the vendored suite: ${JSON.stringify(relativePath)}`,
  );
  return resolved;
}

function readVerifiedSource(manifest, compilerRoot, recorded) {
  requireCondition(
    Number.isSafeInteger(recorded.source) && recorded.source >= 0,
    "compiler fixture source index must be a non-negative safe integer",
  );
  const inventory = manifest.sources[recorded.source];
  requireCondition(
    isObject(inventory) && inventory.suite === "compiler",
    `source ${recorded.source} is absent from the compiler inventory`,
  );
  const sourcePath = safeSourcePath(compilerRoot, inventory.path);
  const raw = fs.readFileSync(sourcePath);
  requireCondition(
    raw.length === inventory.bytes &&
      sha256(raw) === inventory.sha256 &&
      gitBlobSha1(raw) === inventory.git_blob_sha1,
    `compiler source ${inventory.path} does not match its byte/blob inventory`,
  );
  const decoded = ts.sys.readFile(sourcePath);
  requireCondition(
    typeof decoded === "string",
    `TypeScript could not decode vendored compiler source ${inventory.path}`,
  );
  requireCondition(
    Buffer.byteLength(decoded, "utf8") === recorded.decoded_utf8_bytes &&
      sha256(Buffer.from(decoded, "utf8")) === recorded.decoded_sha256,
    `decoded compiler source ${inventory.path} does not match the expansion manifest`,
  );
  return { index: recorded.source, path: inventory.path, decoded };
}

function matchOnce(regex, line) {
  const match = regex.exec(line);
  regex.lastIndex = 0;
  return match;
}

function makeUnitsFromTest(code, fixturePath) {
  const units = [];
  const links = [];
  let currentFileContent;
  let currentFileOptions = {};
  let currentFileName;

  for (const line of code.split(/\r?\n/)) {
    const link = matchOnce(LINK_REGEX, line);
    if (link) {
      links.push({ target: link[1].trim(), link_path: link[2].trim() });
      continue;
    }

    const option = matchOnce(OPTION_REGEX, line);
    if (option) {
      const name = option[1];
      const value = option[2].trim();
      currentFileOptions[name] = value;
      if (name.toLowerCase() !== "filename") continue;

      if (currentFileName) {
        units.push({
          name: currentFileName,
          content: currentFileContent,
          fileOptions: currentFileOptions,
        });
        currentFileContent = undefined;
        currentFileOptions = {};
        currentFileName = value;
      } else {
        currentFileName = value;
        if (
          currentFileContent &&
          ts.skipTrivia(currentFileContent, 0, false, false) !==
            currentFileContent.length
        ) {
          fail(
            `compiler fixture ${JSON.stringify(fixturePath)} contains non-comment content before its first @filename directive`,
          );
        }
        currentFileContent = "";
      }
      continue;
    }

    if (currentFileContent === undefined) currentFileContent = "";
    else if (currentFileContent !== "") currentFileContent += "\n";
    currentFileContent += line;
  }

  currentFileName =
    units.length > 0 || currentFileName
      ? currentFileName
      : ts.getBaseFileName(fixturePath);
  units.push({
    name: currentFileName,
    content: currentFileContent || "",
    fileOptions: currentFileOptions,
  });
  return { units, links };
}

function isConfigFileName(fileName) {
  const baseName = ts.getBaseFileName(fileName).toLowerCase();
  return baseName === "tsconfig.json" || baseName === "jsconfig.json";
}

function recordedUnitContent(unit) {
  if (unit.content === undefined) return { state: "missing" };
  const bytes = Buffer.from(unit.content, "utf8");
  return {
    state: "present",
    utf8_bytes: bytes.length,
    sha256: sha256(bytes),
  };
}

function recordedDocumentSymlinks(fileOptions) {
  const symlink = Object.entries(fileOptions).find(([name]) => name === "symlink");
  if (!symlink || symlink[1] === "") return [];
  return symlink[1].split(",").map((entry) => entry.trim());
}

function verifyUnit(unit, expected, label) {
  requireCondition(isObject(expected), `${label} is absent from the manifest`);
  requireCondition(unit.name === expected.name, `${label} name does not match the manifest`);
  const fileOptions = Object.entries(unit.fileOptions).map(([name, value]) => ({
    name,
    value,
  }));
  requireJsonEqual(fileOptions, expected.file_options, `${label} file options`);
  requireJsonEqual(recordedUnitContent(unit), expected.content, `${label} content`);
  requireJsonEqual(
    recordedDocumentSymlinks(unit.fileOptions),
    expected.document_symlinks,
    `${label} document symlinks`,
  );
}

function verifyFixtureExpansion(recorded, source, units, links, configIndex) {
  requireCondition(
    units.length === recorded.normal_units.length + 1,
    `compiler fixture ${source.path} unit count does not match the manifest`,
  );
  let normalIndex = 0;
  for (const [unitIndex, unit] of units.entries()) {
    const expected =
      unitIndex === configIndex
        ? recorded.virtual_config
        : recorded.normal_units[normalIndex++];
    verifyUnit(unit, expected, `${source.path} unit occurrence ${unitIndex}`);
  }
  requireCondition(
    normalIndex === recorded.normal_units.length,
    `compiler fixture ${source.path} normal unit partition does not match the manifest`,
  );
  requireJsonEqual(links, recorded.links, `${source.path} @link directives`);
}

function createParseConfigHost(units) {
  return {
    useCaseSensitiveFileNames: false,
    readDirectory(directory, extensions, excludes, includes, depth) {
      return ts.matchFiles(
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
              if (separator >= 0) directories.add(relative.substring(0, separator));
              else files.push(relative);
            }
          }
          return { files, directories: ts.arrayFrom(directories) };
        },
        ts.identity,
      );
    },
    fileExists(fileName) {
      return units.some(
        (unit) => unit.name.toLowerCase() === fileName.toLowerCase(),
      );
    },
    readFile(fileName) {
      return ts.forEach(units, (unit) =>
        unit.name.toLowerCase() === fileName.toLowerCase()
          ? unit.content
          : undefined,
      );
    },
  };
}

function jsonValue(value, label, ancestors = new Set()) {
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    requireCondition(Number.isFinite(value), `${label} contains a non-finite number`);
    return value;
  }
  requireCondition(value !== undefined, `${label} contains undefined`);
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
    result = Object.create(null);
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

function buildFixturePlan(manifest, compilerRoot, recorded) {
  requireCondition(
    isObject(recorded.virtual_config),
    "buildFixturePlan requires a virtual config fixture",
  );
  requireCondition(
    Array.isArray(recorded.configurations) && recorded.configurations.length > 0,
    `compiler fixture source ${recorded.source} has no configurations`,
  );
  const source = readVerifiedSource(manifest, compilerRoot, recorded);
  const { units, links } = makeUnitsFromTest(source.decoded, source.path);
  const configOccurrences = units
    .map((unit, id) => ({ id, name: unit.name }))
    .filter((unit) => isConfigFileName(unit.name));
  requireCondition(
    configOccurrences.length === 1,
    `compiler fixture ${source.path} must contain exactly one tsconfig/jsconfig occurrence`,
  );
  const configUnit = configOccurrences[0];
  verifyFixtureExpansion(
    recorded,
    source,
    units,
    links,
    configUnit.id,
  );

  const config = units[configUnit.id];
  const configSource = ts.parseJsonText(config.name, config.content);
  requireCondition(
    configSource.endOfFileToken !== undefined,
    `TypeScript did not produce a complete JSON source for ${source.path}`,
  );
  const conversionDiagnostics = [];
  const rawConfig = ts.convertToObject(configSource, conversionDiagnostics);
  requireCondition(
    conversionDiagnostics.length === 0,
    `raw config conversion unexpectedly diagnosed ${source.path}`,
  );
  const configFileName = ts.getNormalizedAbsolutePath(
    config.name,
    VIRTUAL_SOURCE_ROOT,
  );
  const parsed = ts.parseJsonSourceFileConfigFileContent(
    configSource,
    createParseConfigHost(units),
    ts.getDirectoryPath(configFileName),
    undefined,
    configFileName,
  );
  requireJsonEqual(rawConfig, parsed.raw, `${source.path} parsed raw config`);
  const extendedSourceFiles = parsed.options.configFile?.extendedSourceFiles ?? [];
  const extendedSources = extendedSourceFiles.map((fileName, index) => {
    const unitId = units.findIndex(
      (unit) => unit.name.toLowerCase() === fileName.toLowerCase(),
    );
    requireCondition(
      unitId >= 0,
      `${source.path} extended source ${index} ${JSON.stringify(fileName)} is not a unit`,
    );
    return {
      file_name: fileName,
      unit_id: unitId,
      content: recordedUnitContent(units[unitId]),
    };
  });

  const candidates = units
    .map((unit, id) => ({ id, name: unit.name }))
    .filter((unit) => unit.id !== configUnit.id);
  const rootUnitIds = [];
  const otherUnitIds = [];
  for (const candidate of candidates) {
    const absoluteName = ts.getNormalizedAbsolutePath(
      candidate.name,
      VIRTUAL_SOURCE_ROOT,
    );
    if (parsed.fileNames.includes(absoluteName)) rootUnitIds.push(candidate.id);
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

  return {
    plan: {
      source: { index: source.index, path: source.path },
      configuration_count: recorded.configurations.length,
      config_unit: configUnit,
      candidate_units: candidates,
      parsed_file_names: [...parsed.fileNames],
      root_unit_ids: rootUnitIds,
      other_unit_ids: otherUnitIds,
      program_root_unit_ids: programRootUnitIds,
      raw_config: jsonValue(rawConfig, `${source.path} raw config`),
      extended_sources: extendedSources,
      discovery_options: {
        allow_js: ts.getAllowJSCompilerOption(parsed.options),
        resolve_json_module: ts.getResolveJsonModule(parsed.options),
        out_dir: parsed.options.outDir ?? null,
        declaration_dir: parsed.options.declarationDir ?? null,
      },
      diagnostics: parsed.errors.map(diagnosticRecord),
    },
    configurationCount: recorded.configurations.length,
    configOccurrence: configUnit.id,
  };
}

function addDistributionEntry(distribution, value, cases) {
  const entry = distribution.get(value) ?? { fixtures: 0, cases: 0 };
  entry.fixtures += 1;
  entry.cases += cases;
  distribution.set(value, entry);
}

function renderDistribution(distribution, key) {
  return [...distribution.entries()]
    .sort(([left], [right]) => left - right)
    .map(([value, counts]) => ({ [key]: value, ...counts }));
}

function summarize(rows) {
  const totals = {
    cases: 0,
    candidates: 0,
    weightedCandidates: 0,
    parsedFileNames: 0,
    weightedParsedFileNames: 0,
    extendedSources: 0,
    weightedExtendedSources: 0,
    roots: 0,
    weightedRoots: 0,
    others: 0,
    weightedOthers: 0,
    programRoots: 0,
    weightedProgramRoots: 0,
    diagnostics: 0,
    weightedDiagnostics: 0,
  };
  const configurations = new Map();
  const configOccurrences = new Map();
  const candidates = new Map();
  const roots = new Map();
  const others = new Map();
  const programRoots = new Map();

  for (const row of rows) {
    const cases = row.configurationCount;
    const plan = row.plan;
    totals.cases += cases;
    totals.candidates += plan.candidate_units.length;
    totals.weightedCandidates += plan.candidate_units.length * cases;
    totals.parsedFileNames += plan.parsed_file_names.length;
    totals.weightedParsedFileNames += plan.parsed_file_names.length * cases;
    totals.extendedSources += plan.extended_sources.length;
    totals.weightedExtendedSources += plan.extended_sources.length * cases;
    totals.roots += plan.root_unit_ids.length;
    totals.weightedRoots += plan.root_unit_ids.length * cases;
    totals.others += plan.other_unit_ids.length;
    totals.weightedOthers += plan.other_unit_ids.length * cases;
    totals.programRoots += plan.program_root_unit_ids.length;
    totals.weightedProgramRoots += plan.program_root_unit_ids.length * cases;
    totals.diagnostics += plan.diagnostics.length;
    totals.weightedDiagnostics += plan.diagnostics.length * cases;
    addDistributionEntry(configurations, cases, cases);
    addDistributionEntry(configOccurrences, row.configOccurrence, cases);
    addDistributionEntry(candidates, plan.candidate_units.length, cases);
    addDistributionEntry(roots, plan.root_unit_ids.length, cases);
    addDistributionEntry(others, plan.other_unit_ids.length, cases);
    addDistributionEntry(programRoots, plan.program_root_unit_ids.length, cases);
  }

  return {
    config_plans: { fixture_total: rows.length, case_total: totals.cases },
    candidate_units: {
      fixture_total: totals.candidates,
      case_total: totals.weightedCandidates,
    },
    parsed_file_names: {
      fixture_total: totals.parsedFileNames,
      case_total: totals.weightedParsedFileNames,
    },
    extended_sources: {
      fixture_total: totals.extendedSources,
      case_total: totals.weightedExtendedSources,
    },
    root_units: {
      fixture_total: totals.roots,
      case_total: totals.weightedRoots,
    },
    other_units: {
      fixture_total: totals.others,
      case_total: totals.weightedOthers,
    },
    program_root_units: {
      fixture_total: totals.programRoots,
      case_total: totals.weightedProgramRoots,
    },
    parsed_diagnostics: {
      fixture_total: totals.diagnostics,
      case_total: totals.weightedDiagnostics,
    },
    distributions: {
      configurations_per_fixture: renderDistribution(
        configurations,
        "configurations",
      ),
      config_occurrence: renderDistribution(configOccurrences, "occurrence"),
      candidate_units_per_fixture: renderDistribution(candidates, "units"),
      root_units_per_fixture: renderDistribution(roots, "units"),
      other_units_per_fixture: renderDistribution(others, "units"),
      program_root_units_per_fixture: renderDistribution(programRoots, "units"),
    },
  };
}

function generateArtifact() {
  validateRuntime();
  validateTypeScriptRuntime();
  const { manifest, compilerRoot } = readManifest();
  const rows = manifest.compiler_fixtures
    .filter((fixture) => fixture.virtual_config !== null)
    .map((fixture) => buildFixturePlan(manifest, compilerRoot, fixture));
  const summary = summarize(rows);
  requireJsonEqual(summary, EXPECTED_SUMMARY, "compiler config oracle summary");
  return {
    schema: 1,
    typescript_version: EXPECTED_TYPESCRIPT_VERSION,
    source_commit: EXPECTED_SOURCE_COMMIT,
    node_version: EXPECTED_NODE_VERSION,
    producer: {
      path: TYPESCRIPT_BUNDLE_RELATIVE_PATH,
      sha256: EXPECTED_TYPESCRIPT_BUNDLE_SHA256,
    },
    manifest: {
      path: MANIFEST_RELATIVE_PATH,
      sha256: EXPECTED_MANIFEST_SHA256,
    },
    summary,
    fixtures: rows.map((row) => row.plan),
  };
}

function renderArtifact() {
  return `${JSON.stringify(generateArtifact(), null, 2)}\n`;
}

function checkRecordedArtifact(rendered) {
  const expected = Buffer.from(rendered, "utf8");
  const recorded = fs.readFileSync(ARTIFACT_PATH);
  if (!recorded.equals(expected)) {
    let firstDifference = 0;
    while (
      firstDifference < recorded.length &&
      firstDifference < expected.length &&
      recorded[firstDifference] === expected[firstDifference]
    ) {
      firstDifference += 1;
    }
    fail(
      `${ARTIFACT_RELATIVE_PATH} is stale at byte ${firstDifference} ` +
        `(recorded sha256 ${sha256(recorded)}, generated sha256 ${sha256(expected)}); ` +
        `regenerate with: node crates/oracle/compiler-config-plans.mjs > ${ARTIFACT_RELATIVE_PATH}`,
    );
  }
}

const arguments_ = process.argv.slice(2);
requireCondition(
  arguments_.length === 0 ||
    (arguments_.length === 1 && arguments_[0] === "--check"),
  "usage: node crates/oracle/compiler-config-plans.mjs [--check]",
);
validateRuntime();
const typescriptModule = await import(pathToFileURL(TYPESCRIPT_BUNDLE_PATH).href);
ts = typescriptModule.default;
validateTypeScriptRuntime();
const rendered = renderArtifact();
if (arguments_[0] === "--check") {
  checkRecordedArtifact(rendered);
  process.stdout.write(`${ARTIFACT_RELATIVE_PATH} is current\n`);
} else {
  process.stdout.write(rendered);
}
