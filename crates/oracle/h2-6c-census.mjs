// H2.6c m-1: the effective source-map applicability census.
//
// This is an authoring-side census, not an observation machine.  It walks the
// same pinned compiler, conformance, project, and transpile case corpus used
// by the H2.6a/H2.6b qualification machines, resolves option matrices and
// virtual/project configs, and records only cases whose effective option
// vector mentions the source-map family.  It never invokes a TypeScript emit
// path and never writes an observation.

import { spawn } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const MODE = process.argv[2];

const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-6c-census.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-6c-census.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-6c-census.schema.json";
const TEST_SUITE_EXPANSION =
  "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const CONFORMANCE_EXPANSION =
  "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json";
const COMPILER_CLASSIFICATION =
  "vendor/typescript-6.0.3/compiler-profile-classification.v1.json";
const CONFORMANCE_CLASSIFICATION =
  "vendor/typescript-6.0.3/conformance-profile-classification.v1.json";
const COMPILER_CONFIG_PLANS =
  "vendor/typescript-6.0.3/compiler-config-plans.v1.json";
const PROJECT_CLASSIFICATION =
  "vendor/typescript-6.0.3/project-profile-classification.v1.json";
const TRANSPILE_INVENTORY =
  "vendor/typescript-6.0.3/transpile-suite-inventory.v1.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const NODE_VERSION_PATH = ".node-version";

const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const VIRTUAL_SOURCE_ROOT = "/.src";
const PROJECT_DESCRIPTOR_ROOT = "project";
const PROJECT_TREE_ROOT = "projects";
const PROJECT_VIRTUAL_PREFIX = "/.src/tests/cases/projects";
const PROJECT_STRUCTURAL_KEYS = new Set([
  "scenario",
  "projectRoot",
  "inputFiles",
  "baselineCheck",
  "runTest",
  "project",
  "emittedFiles",
  "resolveMapRoot",
  "resolveSourceRoot",
]);

const MAP_OPTION_NAMES = Object.freeze([
  "sourceMap",
  "inlineSourceMap",
  "inlineSources",
  "sourceRoot",
  "mapRoot",
]);
const MAP_OPTION_BY_LOWER = new Map(
  MAP_OPTION_NAMES.map((name) => [name.toLowerCase(), name]),
);
const BOOLEAN_MAP_OPTIONS = new Set([
  "sourcemap",
  "inlinesourcemap",
  "inlinesources",
]);
const OPTION_INDEX = new Map(
  ts.optionDeclarations.map((option) => [option.name.toLowerCase(), option]),
);

// H2.6c m-1 freezes the current effective census, including the two compiler
// virtual-config cases which are absent from the packet's annotation-only
// estimate.  A changed corpus must cause a loud re-census, never a silently
// resized denominator.
const EXPECTED_CORPUS = Object.freeze({
  compiler: 7276,
  conformance: 7697,
  project: 632,
  transpile: 37,
  total: 15642,
});
const EXPECTED_CENSUS = Object.freeze({
  compiler: Object.freeze({ cases: 241, positive: 199, negative: 42 }),
  conformance: Object.freeze({ cases: 36, positive: 32, negative: 4 }),
  project: Object.freeze({ cases: 410, positive: 410, negative: 0 }),
  transpile: Object.freeze({ cases: 4, positive: 2, negative: 2 }),
  total: 691,
  positive: 643,
  negative: 48,
  unique_fixture_ids: 396,
});
const EXPECTED_FACET_COUNTS = Object.freeze({
  sourceMap: Object.freeze({
    literal_cases: 676,
    true_cases: 629,
    false_cases: 47,
    set_cases: 0,
    nonempty_cases: 0,
    empty_cases: 0,
  }),
  inlineSourceMap: Object.freeze({
    literal_cases: 12,
    true_cases: 11,
    false_cases: 1,
    set_cases: 0,
    nonempty_cases: 0,
    empty_cases: 0,
  }),
  inlineSources: Object.freeze({
    literal_cases: 7,
    true_cases: 7,
    false_cases: 0,
    set_cases: 0,
    nonempty_cases: 0,
    empty_cases: 0,
  }),
  sourceRoot: Object.freeze({
    literal_cases: 212,
    true_cases: 0,
    false_cases: 0,
    set_cases: 212,
    nonempty_cases: 212,
    empty_cases: 0,
  }),
  mapRoot: Object.freeze({
    literal_cases: 210,
    true_cases: 0,
    false_cases: 0,
    set_cases: 210,
    nonempty_cases: 210,
    empty_cases: 0,
  }),
});

const CHECK_SHARDS_ENV = "TSRS_H2_6C_CHECK_SHARDS";
const DEFAULT_CHECK_SHARDS = 4;
const MAX_CHECK_SHARDS = 8;
const INTERNAL_CHECK_SHARD_MODE = "--internal-check-shard";
const CHECK_RECEIPT_RELATIVE_PATH = "target/h2-6c/check-receipt.v1.json";

const OPTION_LINE_PATTERN = /^\/{2}\s*@(\w+)\s*:\s*([^\r\n]*)/;
const LINK_LINE_PATTERN =
  /^\/{2}\s*@link\s*:\s*([^\r\n]*)\s*->\s*([^\r\n]*)/;

class CheckReceiptMiss extends Error {}

function fail(message) {
  throw new Error(message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function gitBlobSha1(value) {
  const bytes = Buffer.isBuffer(value) ? value : Buffer.from(value);
  return crypto
    .createHash("sha1")
    .update(`blob ${bytes.length}\0`)
    .update(bytes)
    .digest("hex");
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

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function writeFileAtomic(absolutePath, contents) {
  const temporary = path.join(
    path.dirname(absolutePath),
    `.${path.basename(absolutePath)}.tmp`,
  );
  fs.writeFileSync(temporary, contents);
  fs.renameSync(temporary, absolutePath);
}

function hasValidFingerprint(value, field) {
  const payload = { ...value };
  const expected = payload[field];
  delete payload[field];
  return expected === sha256(Buffer.from(canonical(payload), "utf8"));
}

function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function readJson(relativePath) {
  return JSON.parse(readBytes(relativePath).toString("utf8"));
}

function pathHash(relativePath) {
  return { path: relativePath, sha256: sha256(readBytes(relativePath)) };
}

function compareBytes(left, right) {
  return Buffer.compare(Buffer.from(left), Buffer.from(right));
}

function validateRuntime() {
  const node = readBytes(NODE_VERSION_PATH).toString("utf8").trim();
  requireCondition(node === EXPECTED_NODE, "unexpected Node pin");
  requireCondition(process.version === `v${node}`, `requires Node ${node}`);
  requireCondition(ts.version === "6.0.3", "unexpected TypeScript runtime");
}

function safeSourcePath(suite, relativePath) {
  requireCondition(
    typeof relativePath === "string" &&
      relativePath.length > 0 &&
      relativePath === path.posix.normalize(relativePath) &&
      !relativePath.startsWith("/") &&
      !relativePath.startsWith("../"),
    `unsafe ${suite} source path ${JSON.stringify(relativePath)}`,
  );
  const root = path.join(WORKSPACE, "ts-tests/tests/cases", suite);
  const absolute = path.resolve(root, ...relativePath.split("/"));
  requireCondition(
    absolute.startsWith(`${path.resolve(root)}${path.sep}`),
    `${suite} source escaped its suite root: ${relativePath}`,
  );
  return absolute;
}

function verifySource(suite, source) {
  requireCondition(source !== null && typeof source === "object", `${suite} source is malformed`);
  const absolute = safeSourcePath(suite, source.path);
  const stat = fs.lstatSync(absolute);
  requireCondition(stat.isFile() && !stat.isSymbolicLink(), `${suite} source is not a regular file: ${source.path}`);
  const raw = fs.readFileSync(absolute);
  requireCondition(
    raw.length === source.bytes &&
      sha256(raw) === source.sha256 &&
      gitBlobSha1(raw) === source.git_blob_sha1,
    `${suite}/${source.path} source identity changed`,
  );
  return {
    path: source.path,
    bytes: source.bytes,
    sha256: source.sha256,
    git_blob_sha1: source.git_blob_sha1,
  };
}

function verifyTestSuiteSources(expansion) {
  const records = expansion.sources.map((source) => {
    const suite = source.suite;
    requireCondition(
      suite === "compiler" || suite === "project" || suite === "projects",
      `unknown test-suite source suite ${suite}`,
    );
    return verifySource(suite, source);
  });
  return records;
}

function verifyConformanceSources(expansion) {
  return expansion.sources.map((source) => verifySource("conformance", source));
}

function verifyTranspileSources(inventory) {
  return inventory.sources.map((source) => verifySource("transpile", source));
}

function orderedSettings(object) {
  return Object.entries(object).map(([name, value]) => ({ name, value }));
}

function documentSymlinks(fileOptions) {
  const setting = fileOptions.find((entry) => entry.name === "symlink");
  if (!setting || setting.value === "") return [];
  return setting.value.split(",").map((entry) => entry.trim());
}

function contentIdentity(text) {
  if (text === undefined) return { state: "missing" };
  const bytes = Buffer.from(text, "utf8");
  return { state: "present", utf8_bytes: bytes.length, sha256: sha256(bytes) };
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
      if (currentName !== undefined) {
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
            ts.skipTrivia(currentContent, 0, false, false) === currentContent.length,
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
    units.length > 0 || currentName !== undefined
      ? currentName
      : path.posix.basename(fixturePath);
  units.push({
    name: currentName,
    text: currentContent || "",
    file_options: orderedSettings(currentOptions),
  });
  units.forEach((unit, index) => {
    unit.original_id = index;
  });
  return { units, links };
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
              if (separator >= 0) directories.add(relative.substring(0, separator));
              else files.push(relative);
            }
          }
          return { files, directories: [...directories] };
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

function configDiagnostic(diagnostic) {
  return {
    code: diagnostic.code,
    file: diagnostic.file?.fileName ?? null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    message: ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
  };
}

function parseVirtualConfig(loaded, recordedPlan) {
  const config = loaded.fixture.virtual_config;
  if (config === null) return {};
  const configIndex = loaded.units.findIndex((unit) => unit.name === config.name);
  requireCondition(configIndex >= 0, `${loaded.source.path} virtual config is absent`);
  const configUnit = loaded.units[configIndex];
  requireCondition(
    canonical(configUnit.file_options) === canonical(config.file_options) &&
      canonical(contentIdentity(configUnit.text)) === canonical(config.content) &&
      canonical(documentSymlinks(configUnit.file_options)) ===
        canonical(config.document_symlinks),
    `${loaded.source.path} virtual config changed`,
  );
  const configFileName = ts.getNormalizedAbsolutePath(
    config.name,
    VIRTUAL_SOURCE_ROOT,
  );
  const source = ts.parseJsonText(config.name, configUnit.text);
  const host = createParseConfigHost(loaded.units);
  const parsed = ts.parseJsonSourceFileConfigFileContent(
    source,
    host,
    ts.getDirectoryPath(configFileName),
    undefined,
    configFileName,
  );
  const diagnostics = [
    ...(source.parseDiagnostics ?? []),
    ...parsed.errors,
  ].map(configDiagnostic);
  requireCondition(diagnostics.length === 0, `${loaded.source.path} virtual config has diagnostics`);
  if (recordedPlan !== undefined) {
    requireCondition(
      recordedPlan.source.index === loaded.sourceIndex &&
        recordedPlan.source.path === loaded.source.path &&
        recordedPlan.configuration_count === loaded.fixture.configurations.length &&
        recordedPlan.config_unit.id === configUnit.original_id &&
        recordedPlan.config_unit.name === configUnit.name &&
        canonical(parsed.fileNames) === canonical(recordedPlan.parsed_file_names) &&
        canonical(diagnostics) === canonical(recordedPlan.diagnostics) &&
        canonical(host.log) === canonical(recordedPlan.host_log),
      `${loaded.source.path} compiler config plan changed`,
    );
  }
  return parsed.options;
}

function validateDirectiveFixture(loaded, expansionFixture) {
  const parsed = makeUnits(loaded.text, loaded.source.path);
  requireCondition(
    canonical(parsed.links) === canonical(expansionFixture.links),
    `${loaded.source.path} global links changed`,
  );
  const normalUnits = [...parsed.units];
  if (expansionFixture.virtual_config !== null) {
    const configIndex = normalUnits.findIndex(
      (unit) => unit.name === expansionFixture.virtual_config.name,
    );
    requireCondition(configIndex >= 0, `${loaded.source.path} virtual config is absent`);
    normalUnits.splice(configIndex, 1);
  }
  requireCondition(
    normalUnits.length === expansionFixture.normal_units.length,
    `${loaded.source.path} unit count changed`,
  );
  normalUnits.forEach((unit, index) => {
    const expected = expansionFixture.normal_units[index];
    requireCondition(
      unit.name === expected.name &&
        canonical(unit.file_options) === canonical(expected.file_options) &&
        canonical(contentIdentity(unit.text)) === canonical(expected.content) &&
        canonical(documentSymlinks(unit.file_options)) ===
          canonical(expected.document_symlinks),
      `${loaded.source.path} unit ${index} changed`,
    );
  });
  return parsed.units;
}

function loadDirectiveFixtures(suite, expansion, sourceRecords, configPlans) {
  const fixtures = suite === "compiler"
    ? expansion.compiler_fixtures
    : expansion.fixtures;
  const bySource = new Map();
  for (const fixture of fixtures) {
    const sourceIndex = fixture.source;
    const source = expansion.sources[sourceIndex];
    requireCondition(source !== undefined, `${suite} fixture source is absent`);
    if (suite === "compiler") {
      requireCondition(source.suite === "compiler", "compiler source suite changed");
    }
    const absolute = safeSourcePath(suite, source.path);
    const decoded = ts.sys.readFile(absolute);
    requireCondition(typeof decoded === "string", `cannot decode ${suite}/${source.path}`);
    requireCondition(
      Buffer.byteLength(decoded, "utf8") === fixture.decoded_utf8_bytes &&
        sha256(Buffer.from(decoded, "utf8")) === fixture.decoded_sha256,
      `${suite}/${source.path} decoded identity changed`,
    );
    const loaded = {
      suite,
      sourceIndex,
      source: sourceRecords[sourceIndex],
      fixture,
      text: decoded,
      units: null,
      configOptions: {},
    };
    loaded.units = validateDirectiveFixture(loaded, fixture);
    const plan = suite === "compiler" && fixture.virtual_config !== null
      ? configPlans.get(sourceIndex)
      : undefined;
    if (suite === "compiler" && fixture.virtual_config !== null) {
      requireCondition(plan !== undefined, `${source.path} compiler config plan is absent`);
    }
    // The conformance harness records a virtual tsconfig-shaped file as a
    // fixture input, but (unlike compiler-suite project/config cases) does
    // not execute it as a TypeScript config.  Preserve it as a fixture unit;
    // only compiler virtual configs participate in effective compiler-option
    // resolution here.
    loaded.configOptions = suite === "compiler"
      ? parseVirtualConfig(loaded, plan)
      : {};
    bySource.set(sourceIndex, loaded);
  }
  return bySource;
}

function typedMapValue(name, raw) {
  const option = OPTION_INDEX.get(name.toLowerCase());
  requireCondition(option !== undefined, `unknown map option ${name}`);
  if (BOOLEAN_MAP_OPTIONS.has(name.toLowerCase())) {
    if (typeof raw === "boolean") return raw;
    const value = String(raw).trim().toLowerCase();
    requireCondition(value === "true" || value === "false", `invalid @${name}: ${raw}`);
    return value === "true";
  }
  requireCondition(typeof raw === "string", `invalid @${name}: ${raw}`);
  return raw;
}

function setFacet(facets, name, value, origin, raw = value) {
  const canonicalName = MAP_OPTION_BY_LOWER.get(name.toLowerCase());
  if (canonicalName === undefined || value === undefined) return;
  const boolean = BOOLEAN_MAP_OPTIONS.has(canonicalName.toLowerCase());
  const state = boolean ? (value ? "true" : "false") : "set";
  facets[canonicalName] = {
    state,
    value,
    raw: raw === null ? null : String(raw),
    origin,
  };
}

function effectiveMapFacets(baseOptions, fixtureSettings, matrixSettings, origins) {
  const facets = {};
  for (const name of MAP_OPTION_NAMES) {
    facets[name] = {
      state: "absent",
      value: null,
      raw: null,
      origin: "absent",
    };
  }
  for (const name of MAP_OPTION_NAMES) {
    const option = OPTION_INDEX.get(name.toLowerCase());
    if (Object.hasOwn(baseOptions, option.name)) {
      const value = baseOptions[option.name];
      if (value !== undefined) setFacet(facets, name, value, origins.base, value);
    }
  }
  const selectedMatrixNames = new Set(
    (matrixSettings ?? []).map((setting) => setting.name.toLowerCase()),
  );
  for (const setting of fixtureSettings ?? []) {
    const name = MAP_OPTION_BY_LOWER.get(setting.name.toLowerCase());
    if (name !== undefined) {
      // The transpile inventory preserves the harness selector (for example
      // `true,false`) in fixture settings and supplies the selected scalar in
      // the configuration override.  It is not itself an effective value.
      if (selectedMatrixNames.has(setting.name.toLowerCase()) &&
          String(setting.value).includes(",")) continue;
      setFacet(facets, name, typedMapValue(name, setting.value), origins.fixture, setting.value);
    }
  }
  for (const setting of matrixSettings ?? []) {
    const name = MAP_OPTION_BY_LOWER.get(setting.name.toLowerCase());
    if (name !== undefined) {
      setFacet(facets, name, typedMapValue(name, setting.value), origins.matrix, setting.value);
    }
  }
  return facets;
}

function facetSummary(facets) {
  const result = {};
  for (const name of MAP_OPTION_NAMES) {
    const facet = facets[name];
    result[name] = facet;
  }
  return result;
}

function classifyFacets(facets) {
  const positiveOptions = MAP_OPTION_NAMES.filter((name) => {
    const facet = facets[name];
    return facet.state === "true" ||
      (facet.state === "set" && facet.value.length > 0);
  });
  const explicitFalseOptions = MAP_OPTION_NAMES.filter(
    (name) => facets[name].state === "false",
  );
  const emptyOptions = MAP_OPTION_NAMES.filter(
    (name) => facets[name].state === "set" && facets[name].value.length === 0,
  );
  const classification = positiveOptions.length > 0 ? "positive" : "negative";
  return {
    positiveOptions,
    explicitFalseOptions,
    emptyOptions,
    classification,
  };
}

function caseParts(caseId) {
  const marker = caseId.indexOf("#");
  return {
    fixtureId: marker < 0 ? caseId : caseId.slice(0, marker),
    matrixKey: marker < 0 ? "default" : caseId.slice(marker + 1),
  };
}

function makeCensusRow({
  suite,
  caseId,
  sourceIndex,
  expansionCase,
  configurationIndex,
  configurationVariant,
  route,
  source,
  facets,
  extra = {},
}) {
  const { fixtureId, matrixKey } = caseParts(caseId);
  const classification = classifyFacets(facets);
  return {
    suite,
    case_id: caseId,
    fixture_id: fixtureId,
    matrix_key: matrixKey,
    source_index: sourceIndex,
    expansion_case: expansionCase,
    configuration_index: configurationIndex,
    configuration_variant: configurationVariant,
    route,
    source,
    option_facets: facetSummary(facets),
    positive_options: classification.positiveOptions,
    explicit_false_options: classification.explicitFalseOptions,
    empty_root_options: classification.emptyOptions,
    classification: classification.classification,
    positive: classification.classification === "positive",
    ...extra,
  };
}

function compareExpansionClassification(suite, expansion, classification) {
  const expansionCases = expansion.cases.filter(
    (entry) => suite === "compiler" ? entry.suite === "compiler" : suite === "conformance",
  );
  requireCondition(
    classification.cases.length === expansionCases.length,
    `${suite} classification denominator changed`,
  );
  const byId = new Map(classification.cases.map((entry) => [entry.id, entry]));
  requireCondition(byId.size === classification.cases.length, `${suite} classification IDs are not unique`);
  for (const [expansionCase, expansionIndex] of expansion.cases.entries()) {
    if (suite === "compiler" && expansionCase.suite !== "compiler") continue;
    if (suite === "conformance") continue;
    const classified = byId.get(expansionCase.id);
    requireCondition(
      classified !== undefined &&
        classified.expansion_case === expansionIndex &&
        classified.source === expansionCase.source &&
        classified.configuration === expansionCase.configuration.configuration,
      `${suite} classification identity changed for ${expansionCase.id}`,
    );
  }
  if (suite === "conformance") {
    for (const [expansionIndex, expansionCase] of expansion.cases.entries()) {
      const classified = byId.get(expansionCase.id);
      requireCondition(
        classified !== undefined &&
          classified.expansion_case === expansionIndex &&
          classified.source === expansionCase.source &&
          classified.configuration === expansionCase.configuration,
        `${suite} classification identity changed for ${expansionCase.id}`,
      );
    }
  }
  return byId;
}

function directiveRows(
  suite,
  expansion,
  classification,
  loadedBySource,
) {
  const classById = compareExpansionClassification(suite, expansion, classification);
  const rows = [];
  for (const [expansionIndex, expansionCase] of expansion.cases.entries()) {
    if (suite === "compiler" && expansionCase.suite !== "compiler") continue;
    if (suite === "conformance") {
      // The conformance expansion has no suite field; every case is a
      // conformance case.
    }
    const classified = classById.get(expansionCase.id);
    const configurationIndex = suite === "compiler"
      ? expansionCase.configuration.configuration
      : expansionCase.configuration;
    const loaded = loadedBySource.get(expansionCase.source);
    requireCondition(loaded !== undefined, `${expansionCase.id} fixture is absent`);
    const configuration = loaded.fixture.configurations[configurationIndex];
    requireCondition(configuration !== undefined, `${expansionCase.id} configuration is absent`);
    const facets = effectiveMapFacets(
      loaded.configOptions,
      loaded.fixture.settings,
      configuration.settings,
      {
        base: "virtual-config",
        fixture: "fixture",
        matrix: "matrix",
      },
    );
    if (MAP_OPTION_NAMES.every((name) => facets[name].state === "absent")) continue;
    rows.push(makeCensusRow({
      suite,
      caseId: expansionCase.id,
      sourceIndex: expansionCase.source,
      expansionCase: expansionIndex,
      configurationIndex,
      configurationVariant: configuration.variant,
      route: suite === "compiler" ? "recorded-compiler-plan" : "qualified-vfs",
      source: loaded.source,
      facets,
      extra: {
        virtual_config: loaded.fixture.virtual_config !== null,
        selection_origin: "full-corpus",
        classified_disposition: classified.disposition,
      },
    }));
  }
  return rows;
}

function walkProjectTree() {
  const root = path.join(WORKSPACE, "ts-tests/tests/cases", PROJECT_TREE_ROOT);
  const files = [];
  const visit = (directory) => {
    const entries = fs
      .readdirSync(directory, { withFileTypes: true })
      .sort((left, right) => compareBytes(left.name, right.name));
    for (const entry of entries) {
      const absolute = path.join(directory, entry.name);
      requireCondition(!entry.isSymbolicLink(), `unsupported project tree symlink ${absolute}`);
      if (entry.isDirectory()) visit(absolute);
      else {
        requireCondition(entry.isFile(), `unsupported project tree entry ${absolute}`);
        files.push(absolute);
      }
    }
  };
  visit(root);
  return files.map((absolute) => {
    const relative = path
      .relative(root, absolute)
      .split(path.sep)
      .join("/");
    const raw = fs.readFileSync(absolute);
    const decoded = ts.sys.readFile(absolute);
    requireCondition(typeof decoded === "string", `cannot decode project mount file ${relative}`);
    return {
      relative_path: relative,
      virtual_path: `${PROJECT_VIRTUAL_PREFIX}/${relative}`,
      text: decoded,
      bytes: raw.length,
      sha256: sha256(raw),
      git_blob_sha1: gitBlobSha1(raw),
    };
  });
}

function projectRootSelectionRecord(descriptor, cwd, mountByPath) {
  if (Array.isArray(descriptor.inputFiles)) {
    return {
      state: "explicit-inputs",
      roots: descriptor.inputFiles.map((requested) => {
        const absolute = ts.getNormalizedAbsolutePath(requested, cwd);
        return {
          requested,
          path: absolute,
          present: mountByPath.has(absolute),
        };
      }),
    };
  }
  if (typeof descriptor.project === "string") {
    const configured = ts.getNormalizedAbsolutePath(descriptor.project, cwd);
    const configPath = mountByPath.has(configured)
      ? configured
      : ts.normalizePath(`${configured}/tsconfig.json`);
    requireCondition(
      mountByPath.has(configPath),
      `project descriptor config ${descriptor.project} is absent from the mount`,
    );
    return { state: "project-config", config_path: configPath };
  }
  const discovered = ts.normalizePath(`${cwd}/tsconfig.json`);
  requireCondition(
    mountByPath.has(discovered),
    `discovered project config ${discovered} is absent from the mount`,
  );
  return { state: "discovered-config", config_path: discovered };
}

function parseProjectConfig(configPath, cwd, mountByPath) {
  const configText = mountByPath.get(configPath)?.text;
  requireCondition(typeof configText === "string", `project config ${configPath} is unreadable`);
  const parsed = ts.parseJsonText(configPath, configText);
  const host = {
    useCaseSensitiveFileNames: true,
    fileExists: (fileName) =>
      mountByPath.has(ts.getNormalizedAbsolutePath(fileName, cwd)),
    readFile: (fileName) =>
      mountByPath.get(ts.getNormalizedAbsolutePath(fileName, cwd))?.text,
    readDirectory(rootDirectory, extensions, excludes, includes, depth) {
      return ts.matchFiles(
        rootDirectory,
        extensions,
        excludes,
        includes,
        true,
        cwd,
        depth,
        (directory) => {
          const normalized = ts.normalizePath(directory).replace(/\/+$/, "");
          const prefix = normalized === "" ? "" : `${normalized}/`;
          const files = [];
          const directories = new Set();
          for (const filePath of mountByPath.keys()) {
            if (!filePath.startsWith(prefix)) continue;
            const remainder = filePath.slice(prefix.length);
            const slash = remainder.indexOf("/");
            if (slash === -1) files.push(remainder);
            else directories.add(remainder.slice(0, slash));
          }
          return { files, directories: [...directories] };
        },
        (fileName) => ts.getNormalizedAbsolutePath(fileName, cwd),
      );
    },
  };
  const content = ts.parseJsonSourceFileConfigFileContent(
    parsed,
    host,
    ts.getDirectoryPath(configPath),
    {},
    configPath,
  );
  const diagnostics = [
    ...(parsed.parseDiagnostics ?? []),
    ...content.errors,
  ].map(configDiagnostic);
  requireCondition(diagnostics.length === 0, `project config ${configPath} has diagnostics`);
  return {
    options: content.options,
    file_names: content.fileNames.map((fileName) => ts.normalizePath(fileName)),
    diagnostics,
  };
}

function loadProjectDescriptor(source, descriptorPath) {
  const absolute = safeSourcePath(PROJECT_DESCRIPTOR_ROOT, descriptorPath);
  const raw = fs.readFileSync(absolute);
  requireCondition(
    raw.length === source.bytes &&
      sha256(raw) === source.sha256 &&
      gitBlobSha1(raw) === source.git_blob_sha1,
    `project descriptor ${descriptorPath} identity changed`,
  );
  const descriptor = JSON.parse(raw.toString("utf8"));
  for (const key of Object.keys(descriptor)) {
    if (!PROJECT_STRUCTURAL_KEYS.has(key) && !OPTION_INDEX.has(key.toLowerCase())) {
      fail(`project descriptor ${descriptorPath} carries unknown key ${key}`);
    }
  }
  requireCondition(
    typeof descriptor.scenario === "string" &&
      typeof descriptor.projectRoot === "string",
    `project descriptor ${descriptorPath} is missing scenario/projectRoot`,
  );
  return descriptor;
}

function compareProjectSelection(actual, recorded, caseId) {
  requireCondition(actual.state === recorded.state, `${caseId} root-selection arm changed`);
  if (actual.state === "explicit-inputs") {
    requireCondition(
      canonical(actual.roots.map((root) => ({
        requested: root.requested,
        path: root.path,
        present: root.present,
      }))) === canonical(recorded.roots.map((root) => ({
        requested: root.requested,
        path: root.path,
        present: root.presence.state === "present",
      }))),
      `${caseId} explicit roots changed`,
    );
  } else {
    requireCondition(
      actual.config_path === recorded.config?.path,
      `${caseId} config path changed`,
    );
  }
}

function projectFacets(configOptions, descriptor) {
  const facets = {};
  for (const name of MAP_OPTION_NAMES) {
    facets[name] = {
      state: "absent",
      value: null,
      raw: null,
      origin: "absent",
    };
  }
  for (const name of MAP_OPTION_NAMES) {
    const option = OPTION_INDEX.get(name.toLowerCase());
    if (Object.hasOwn(configOptions, option.name) && configOptions[option.name] !== undefined) {
      setFacet(facets, name, configOptions[option.name], "project-config", configOptions[option.name]);
    }
  }
  for (const [key, raw] of Object.entries(descriptor)) {
    const name = MAP_OPTION_BY_LOWER.get(key.toLowerCase());
    if (name !== undefined) {
      setFacet(facets, name, typedMapValue(name, raw), "descriptor", raw);
    }
  }
  return facets;
}

function projectRows(testExpansion, projectClassification, sourceRecords) {
  const projectExpansionCases = testExpansion.cases.filter(
    (entry) => entry.suite === "project",
  );
  requireCondition(
    projectExpansionCases.length === projectClassification.cases.length,
    "project expansion/classification denominator changed",
  );
  const expansionById = new Map(
    projectExpansionCases.map((entry, index) => [entry.id, { entry, index: testExpansion.cases.indexOf(entry) }]),
  );
  requireCondition(expansionById.size === projectExpansionCases.length, "project expansion IDs are not unique");

  const tree = walkProjectTree();
  const expectedBacking = new Map(
    testExpansion.sources
      .filter((source) => source.suite === "projects")
      .map((source) => [source.path, source]),
  );
  const mountByPath = new Map();
  for (const entry of tree) {
    const expected = expectedBacking.get(entry.relative_path);
    requireCondition(expected !== undefined, `project mount has an unpinned file ${entry.relative_path}`);
    requireCondition(
      entry.bytes === expected.bytes &&
        entry.sha256 === expected.sha256 &&
        entry.git_blob_sha1 === expected.git_blob_sha1,
      `project mount file ${entry.relative_path} identity changed`,
    );
    mountByPath.set(entry.virtual_path, entry);
  }
  requireCondition(tree.length === expectedBacking.size, "project mount denominator changed");

  const contexts = new Map();
  for (const row of projectClassification.cases) {
    const expansion = expansionById.get(row.id);
    requireCondition(
      expansion !== undefined &&
        expansion.entry.source === row.source &&
        expansion.entry.configuration.kind === "project" &&
        expansion.entry.configuration.module === row.module_variant.name &&
        expansion.index === row.expansion_case,
      `project classification identity changed for ${row.id}`,
    );
    const existing = contexts.get(row.source);
    if (existing !== undefined) {
      requireCondition(
        existing.descriptor_path === row.descriptor_path &&
          existing.current_directory === row.current_directory,
        `project descriptor matrix identity changed for ${row.id}`,
      );
      continue;
    }
    const source = testExpansion.sources[row.source];
    requireCondition(source?.suite === "project" && source.path === row.descriptor_path, `project source identity changed for ${row.id}`);
    const descriptor = loadProjectDescriptor(source, row.descriptor_path);
    const cwd = ts.normalizePath(`${VIRTUAL_SOURCE_ROOT}/${descriptor.projectRoot}`);
    requireCondition(cwd === row.current_directory, `${row.id} current directory changed`);
    const selection = projectRootSelectionRecord(descriptor, cwd, mountByPath);
    compareProjectSelection(selection, row.root_selection, row.id);
    let configOptions = {};
    let configPath = null;
    let configDiagnostics = [];
    let reachedFiles;
    if (selection.state !== "explicit-inputs") {
      configPath = selection.config_path;
      const recordedConfig = row.root_selection.config;
      const mountedConfig = mountByPath.get(configPath);
      requireCondition(
        mountedConfig !== undefined &&
          mountedConfig.sha256 === recordedConfig.sha256 &&
          mountedConfig.git_blob_sha1 === recordedConfig.git_blob_sha1,
        `${row.id} project config identity changed`,
      );
      const parsed = parseProjectConfig(configPath, cwd, mountByPath);
      configOptions = parsed.options;
      configDiagnostics = parsed.diagnostics;
      reachedFiles = parsed.file_names;
      requireCondition(
        canonical(reachedFiles) ===
          canonical(row.root_selection.roots.map((root) => root.path)),
        `${row.id} reached-file selection changed`,
      );
    } else {
      reachedFiles = selection.roots
        .filter((root) => root.present)
        .map((root) => root.path);
    }
    contexts.set(row.source, {
      descriptor_path: row.descriptor_path,
      current_directory: cwd,
      selection,
      config_path: configPath,
      config_diagnostics: configDiagnostics,
      reached_files: reachedFiles,
      source: {
        path: source.path,
        bytes: source.bytes,
        sha256: source.sha256,
        git_blob_sha1: source.git_blob_sha1,
      },
      facets: projectFacets(configOptions, descriptor),
    });
  }
  const rows = [];
  for (const row of projectClassification.cases) {
    const context = contexts.get(row.source);
    requireCondition(context !== undefined, `${row.id} project context is absent`);
    if (MAP_OPTION_NAMES.every((name) => context.facets[name].state === "absent")) continue;
    rows.push(makeCensusRow({
      suite: "project",
      caseId: row.id,
      sourceIndex: row.source,
      expansionCase: row.expansion_case,
      configurationIndex: null,
      configurationVariant: row.module_variant.name,
      route: "project-mount",
      source: context.source,
      facets: context.facets,
      extra: {
        project_case: row.project_case,
        root_mode: context.selection.state,
        project_config_path: context.config_path,
        config_diagnostics: context.config_diagnostics,
        reached_files: context.reached_files,
        selection_origin: "full-corpus",
      },
    }));
  }
  return rows;
}

function transpileRows(inventory, sourceRecords) {
  const fixtures = new Map(inventory.fixtures.map((fixture) => [fixture.source, fixture]));
  requireCondition(fixtures.size === inventory.fixtures.length, "transpile fixture IDs are not unique");
  const rows = [];
  for (const [caseIndex, entry] of inventory.cases.entries()) {
    const fixture = fixtures.get(entry.source);
    requireCondition(fixture !== undefined, `${entry.id} transpile fixture is absent`);
    const source = sourceRecords[entry.source];
    const absolute = safeSourcePath("transpile", source.path);
    const decoded = ts.sys.readFile(absolute);
    requireCondition(
      typeof decoded === "string" &&
        Buffer.byteLength(decoded, "utf8") === fixture.decoded_utf8_bytes &&
        sha256(Buffer.from(decoded, "utf8")) === fixture.decoded_sha256,
      `${entry.id} transpile decoded identity changed`,
    );
    const configuration = fixture.configurations[entry.configuration];
    requireCondition(configuration !== undefined, `${entry.id} transpile configuration is absent`);
    const facets = effectiveMapFacets(
      {},
      fixture.settings,
      configuration.overrides,
      { base: "virtual-config", fixture: "fixture", matrix: "matrix" },
    );
    if (MAP_OPTION_NAMES.every((name) => facets[name].state === "absent")) continue;
    rows.push(makeCensusRow({
      suite: "transpile",
      caseId: entry.id,
      sourceIndex: entry.source,
      expansionCase: null,
      configurationIndex: entry.configuration,
      configurationVariant: configuration.variant,
      route: "transpile-api",
      source,
      facets,
      extra: {
        transpile_kind: entry.kind,
        api: entry.api,
        case_index: caseIndex,
        selection_origin: "full-corpus",
      },
    }));
  }
  return rows;
}

function countBySuite(rows, suite, field) {
  return rows.filter((row) => row.suite === suite && row[field]).length;
}

function facetCounts(rows) {
  const counts = {};
  for (const name of MAP_OPTION_NAMES) {
    const record = {
      literal_cases: 0,
      true_cases: 0,
      false_cases: 0,
      set_cases: 0,
      nonempty_cases: 0,
      empty_cases: 0,
    };
    for (const row of rows) {
      const facet = row.option_facets[name];
      if (facet.state === "absent") continue;
      record.literal_cases += 1;
      if (facet.state === "true") record.true_cases += 1;
      if (facet.state === "false") record.false_cases += 1;
      if (facet.state === "set") {
        record.set_cases += 1;
        if (facet.value.length > 0) record.nonempty_cases += 1;
        else record.empty_cases += 1;
      }
    }
    counts[name] = record;
  }
  return counts;
}

function assertFrozenCensus(corpus, summary, rows) {
  for (const suite of ["compiler", "conformance", "project", "transpile"]) {
    requireCondition(
      corpus.suites.find((entry) => entry.suite === suite).cases === EXPECTED_CORPUS[suite],
      `unexpected ${suite} corpus case count`,
    );
    requireCondition(
      summary[`${suite}_census_cases`] === EXPECTED_CENSUS[suite].cases &&
        summary[`${suite}_positive_cases`] === EXPECTED_CENSUS[suite].positive &&
        summary[`${suite}_negative_cases`] === EXPECTED_CENSUS[suite].negative,
      `unexpected ${suite} H2.6c census counts`,
    );
  }
  requireCondition(summary.corpus_cases === EXPECTED_CORPUS.total, "unexpected full corpus denominator");
  requireCondition(
    summary.census_cases === EXPECTED_CENSUS.total &&
      summary.literal_cases === EXPECTED_CENSUS.total &&
      summary.positive_cases === EXPECTED_CENSUS.positive &&
      summary.negative_cases === EXPECTED_CENSUS.negative &&
      summary.unique_fixture_ids === EXPECTED_CENSUS.unique_fixture_ids &&
      rows.length === EXPECTED_CENSUS.total,
    "unexpected frozen H2.6c census denominator",
  );
  const actualFacetCounts = facetCounts(rows);
  requireCondition(
    canonical(actualFacetCounts) === canonical(EXPECTED_FACET_COUNTS),
    `unexpected H2.6c option facet counts: ${canonical(actualFacetCounts)}`,
  );
}

function buildSummary(rows, corpusCases) {
  const summary = {
    corpus_cases: corpusCases.total,
    census_cases: rows.length,
    literal_cases: rows.length,
    positive_cases: rows.filter((row) => row.classification === "positive").length,
    negative_cases: rows.filter((row) => row.classification === "negative").length,
    explicit_false_cases: rows.filter((row) => row.explicit_false_options.length > 0).length,
    unique_fixture_ids: new Set(rows.map((row) => row.fixture_id)).size,
    compiler_census_cases: rows.filter((row) => row.suite === "compiler").length,
    compiler_positive_cases: countBySuite(rows, "compiler", "positive"),
    compiler_negative_cases: rows.filter((row) => row.suite === "compiler" && !row.positive).length,
    conformance_census_cases: rows.filter((row) => row.suite === "conformance").length,
    conformance_positive_cases: countBySuite(rows, "conformance", "positive"),
    conformance_negative_cases: rows.filter((row) => row.suite === "conformance" && !row.positive).length,
    project_census_cases: rows.filter((row) => row.suite === "project").length,
    project_positive_cases: countBySuite(rows, "project", "positive"),
    project_negative_cases: rows.filter((row) => row.suite === "project" && !row.positive).length,
    transpile_census_cases: rows.filter((row) => row.suite === "transpile").length,
    transpile_positive_cases: countBySuite(rows, "transpile", "positive"),
    transpile_negative_cases: rows.filter((row) => row.suite === "transpile" && !row.positive).length,
    facet_counts: facetCounts(rows),
  };
  requireCondition(
    summary.positive_cases + summary.negative_cases === summary.census_cases,
    "H2.6c positive/negative partition is incomplete",
  );
  requireCondition(summary.explicit_false_cases === summary.negative_cases, "H2.6c negative controls changed");
  return summary;
}

function inputRecords() {
  return {
    test_suite_expansion: pathHash(TEST_SUITE_EXPANSION),
    conformance_expansion: pathHash(CONFORMANCE_EXPANSION),
    compiler_classification: pathHash(COMPILER_CLASSIFICATION),
    conformance_classification: pathHash(CONFORMANCE_CLASSIFICATION),
    compiler_config_plans: pathHash(COMPILER_CONFIG_PLANS),
    project_classification: pathHash(PROJECT_CLASSIFICATION),
    transpile_inventory: pathHash(TRANSPILE_INVENTORY),
  };
}

function buildCorpus() {
  const testExpansion = readJson(TEST_SUITE_EXPANSION);
  const conformanceExpansion = readJson(CONFORMANCE_EXPANSION);
  const compilerClassification = readJson(COMPILER_CLASSIFICATION);
  const conformanceClassification = readJson(CONFORMANCE_CLASSIFICATION);
  const compilerConfigPlans = readJson(COMPILER_CONFIG_PLANS);
  const projectClassification = readJson(PROJECT_CLASSIFICATION);
  const transpileInventory = readJson(TRANSPILE_INVENTORY);

  const testSourceRecords = verifyTestSuiteSources(testExpansion);
  const conformanceSourceRecords = verifyConformanceSources(conformanceExpansion);
  const transpileSourceRecords = verifyTranspileSources(transpileInventory);
  requireCondition(
    testExpansion.cases.filter((entry) => entry.suite === "compiler").length === EXPECTED_CORPUS.compiler &&
      conformanceExpansion.cases.length === EXPECTED_CORPUS.conformance &&
      testExpansion.cases.filter((entry) => entry.suite === "project").length === EXPECTED_CORPUS.project &&
      transpileInventory.cases.length === EXPECTED_CORPUS.transpile,
    "full fixture corpus denominator changed",
  );
  requireCondition(
    compilerClassification.cases.length === EXPECTED_CORPUS.compiler &&
      conformanceClassification.cases.length === EXPECTED_CORPUS.conformance &&
      projectClassification.cases.length === EXPECTED_CORPUS.project,
    "qualification corpus classification denominator changed",
  );

  const configPlans = new Map(
    compilerConfigPlans.fixtures.map((entry) => [entry.source.index, entry]),
  );
  const compilerFixtures = loadDirectiveFixtures(
    "compiler",
    testExpansion,
    testSourceRecords,
    configPlans,
  );
  const conformanceFixtures = loadDirectiveFixtures(
    "conformance",
    conformanceExpansion,
    conformanceSourceRecords,
    new Map(),
  );
  const rows = [
    ...directiveRows(
      "compiler",
      testExpansion,
      compilerClassification,
      compilerFixtures,
    ),
    ...directiveRows(
      "conformance",
      conformanceExpansion,
      conformanceClassification,
      conformanceFixtures,
    ),
    ...projectRows(testExpansion, projectClassification, testSourceRecords),
    ...transpileRows(transpileInventory, transpileSourceRecords),
  ].sort((left, right) =>
    compareBytes(left.suite, right.suite) || compareBytes(left.case_id, right.case_id),
  );
  requireCondition(
    new Set(rows.map((row) => row.case_id)).size === rows.length,
    "duplicate H2.6c census case ID",
  );

  const corpusCases = {
    compiler: EXPECTED_CORPUS.compiler,
    conformance: EXPECTED_CORPUS.conformance,
    project: EXPECTED_CORPUS.project,
    transpile: EXPECTED_CORPUS.transpile,
    total: EXPECTED_CORPUS.total,
  };
  const corpus = {
    root: "ts-tests/tests/cases",
    cases: corpusCases.total,
    source_files: testExpansion.sources.length +
      conformanceExpansion.sources.length +
      transpileInventory.sources.length,
    suites: [
      {
        suite: "compiler",
        relative_root: "compiler",
        source_files: testExpansion.sources.filter((source) => source.suite === "compiler").length,
        cases: corpusCases.compiler,
      },
      {
        suite: "conformance",
        relative_root: "conformance",
        source_files: conformanceExpansion.sources.length,
        cases: corpusCases.conformance,
      },
      {
        suite: "project",
        relative_root: "project",
        source_files: testExpansion.sources.filter((source) => source.suite === "project").length,
        backing_files: testExpansion.sources.filter((source) => source.suite === "projects").length,
        cases: corpusCases.project,
      },
      {
        suite: "transpile",
        relative_root: "transpile",
        source_files: transpileInventory.sources.length,
        cases: corpusCases.transpile,
      },
    ],
  };
  const summary = buildSummary(rows, corpusCases);
  assertFrozenCensus(corpus, summary, rows);
  return {
    testExpansion,
    conformanceExpansion,
    transpileInventory,
    compilerConfigPlans,
    corpus,
    rows,
    summary,
  };
}

function enumerationContract() {
  return {
    corpus_root: "ts-tests/tests/cases",
    suites: ["compiler", "conformance", "project", "transpile"],
    case_id: "the pinned expansion/inventory case id, including its matrix suffix",
    matrix_key: "the exact case-id suffix after the first #; default when absent",
    effective_option_order:
      "virtual tsconfig/project config options, then fixture/descriptor settings, then matrix overrides",
    map_options: [...MAP_OPTION_NAMES],
    positive_definition:
      "sourceMap, inlineSourceMap, or inlineSources is explicitly true, or sourceRoot/mapRoot is explicitly nonempty",
    negative_definition:
      "the case has a map-family option but no positive facet; explicit false boolean values remain controls",
    observations: "none; this m-1 artifact is authoring-side census data only",
  };
}

function buildArtifact() {
  const built = buildCorpus();
  const corpus = built.corpus;
  const typescript = {
    version: ts.version,
    source_commit: SOURCE_COMMIT,
    bundle: pathHash(TYPESCRIPT_BUNDLE),
    implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
  };
  return withFingerprint(
    {
      schema: 1,
      kind: "h2-6c-map-applicability-census",
      status: "measured-not-observed",
      phase: "H2.6c-map-applicability",
      typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        packet: "docs/design/greenfield/slices/h2-6c.md",
        static_estimate: {
          positive_cases: 641,
          literal_cases: 689,
        },
        measurement: "effective options after virtual/project config and matrix expansion",
      },
      inputs: inputRecords(),
      enumeration_contract: enumerationContract(),
      corpus,
      cases: built.rows,
      summary: built.summary,
    },
    "census_fingerprint_sha256",
  );
}

function checkShardCount() {
  const raw = process.env[CHECK_SHARDS_ENV];
  if (raw === undefined) return DEFAULT_CHECK_SHARDS;
  const value = Number(raw);
  requireCondition(
    Number.isInteger(value) && value >= 1 && value <= MAX_CHECK_SHARDS,
    `${CHECK_SHARDS_ENV} must be an integer from 1 to ${MAX_CHECK_SHARDS}`,
  );
  return value;
}

function parseShardArguments(argv) {
  requireCondition(argv.length === 5, "internal shard mode requires a shard index and count");
  const index = Number(argv[3]);
  const count = Number(argv[4]);
  requireCondition(
    Number.isInteger(index) && index >= 0 &&
      Number.isInteger(count) && count >= 2 && count <= MAX_CHECK_SHARDS && index < count,
    "invalid internal shard selection",
  );
  return { index, count };
}

function observeShardInChildProcess(index, count) {
  return new Promise((resolve, reject) => {
    const child = spawn(
      process.execPath,
      [GENERATOR_PATH, INTERNAL_CHECK_SHARD_MODE, String(index), String(count)],
      { cwd: WORKSPACE, stdio: ["ignore", "pipe", "inherit"] },
    );
    const chunks = [];
    child.stdout.on("data", (chunk) => chunks.push(chunk));
    child.on("error", reject);
    child.on("close", (code) => {
      if (code !== 0) {
        reject(new Error(`census shard ${index} exited with ${code}`));
        return;
      }
      try {
        const payload = JSON.parse(Buffer.concat(chunks).toString("utf8"));
        requireCondition(
          payload?.schema === 1 &&
            payload.shard_index === index &&
            payload.shard_count === count &&
            Array.isArray(payload.shard_cases),
          `census shard ${index} returned an invalid payload`,
        );
        resolve(payload.shard_cases);
      } catch (error) {
        reject(error);
      }
    });
  });
}

async function runShardedCheck(count) {
  const shardCases = await Promise.all(
    Array.from({ length: count }, (_, index) =>
      observeShardInChildProcess(index, count),
    ),
  );
  const adoption = new Map();
  for (const cases of shardCases) {
    for (const row of cases) {
      requireCondition(
        row !== null && typeof row === "object" &&
          typeof row.case_id === "string" && !adoption.has(row.case_id),
        "census shards overlap or are malformed",
      );
      adoption.set(row.case_id, row);
    }
  }
  const artifact = buildArtifact();
  requireCondition(adoption.size === artifact.cases.length, `sharded census covered ${adoption.size}/${artifact.cases.length} cases`);
  for (const row of artifact.cases) {
    requireCondition(
      canonical(adoption.get(row.case_id)) === canonical(row),
      `census shard record changed for ${row.case_id}`,
    );
  }
  finishCheck(artifact, count);
}

function receiptGlobalSha(artifact) {
  return sha256(Buffer.from(canonical({
    typescript: artifact.typescript,
    inputs: artifact.inputs,
    enumeration_contract: artifact.enumeration_contract,
    corpus: artifact.corpus,
  }), "utf8"));
}

function receiptState(artifact) {
  let receipt;
  try {
    receipt = JSON.parse(
      fs.readFileSync(path.join(WORKSPACE, CHECK_RECEIPT_RELATIVE_PATH), "utf8"),
    );
  } catch {
    return "miss (absent-or-invalid)";
  }
  if (
    receipt === null || typeof receipt !== "object" ||
    receipt.schema !== 1 ||
    receipt.kind !== "h2-6c-census-check-receipt" ||
    !hasValidFingerprint(receipt, "receipt_fingerprint_sha256") ||
    receipt.workspace !== fs.realpathSync(WORKSPACE) ||
    receipt.node !== process.version ||
    receipt.generator_sha256 !== artifact.generator.sha256 ||
    receipt.global_records_sha256 !== receiptGlobalSha(artifact) ||
    receipt.census_fingerprint_sha256 !== artifact.census_fingerprint_sha256
  ) {
    return "miss (stale)";
  }
  return "hit";
}

function mintCheckReceipt(artifact) {
  const absolute = path.join(WORKSPACE, CHECK_RECEIPT_RELATIVE_PATH);
  fs.mkdirSync(path.dirname(absolute), { recursive: true });
  writeFileAtomic(
    absolute,
    render(withFingerprint({
      schema: 1,
      kind: "h2-6c-census-check-receipt",
      minted_by: "full-census-check",
      workspace: fs.realpathSync(WORKSPACE),
      node: process.version,
      generator_sha256: artifact.generator.sha256,
      global_records_sha256: receiptGlobalSha(artifact),
      census_fingerprint_sha256: artifact.census_fingerprint_sha256,
    }, "receipt_fingerprint_sha256")),
  );
}

function finishCheck(artifact, checkShards) {
  const rendered = render(artifact);
  const target = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
  requireCondition(
    fs.existsSync(target) && fs.readFileSync(target, "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-6c-census.mjs --write after review`,
  );
  const previousReceipt = receiptState(artifact);
  mintCheckReceipt(artifact);
  process.stderr.write(`H2.6c census check receipt: ${previousReceipt}; minted fresh receipt\n`);
  process.stdout.write(
    `H2.6c census is fresh: positive=${artifact.summary.positive_cases} negative=${artifact.summary.negative_cases} literal=${artifact.summary.literal_cases} check_shards=${checkShards}\n`,
  );
}

validateRuntime();
if (MODE === INTERNAL_CHECK_SHARD_MODE) {
  const shard = parseShardArguments(process.argv);
  const artifact = buildArtifact();
  const cases = artifact.cases.filter((_, index) => index % shard.count === shard.index);
  process.stdout.write(render({
    schema: 1,
    shard_index: shard.index,
    shard_count: shard.count,
    shard_cases: cases,
  }));
} else if (MODE === "--check") {
  if (checkShardCount() > 1) await runShardedCheck(checkShardCount());
  else finishCheck(buildArtifact(), 1);
} else if (MODE === "--preflight") {
  const artifact = buildArtifact();
  const positiveDelta = artifact.summary.positive_cases - 641;
  const literalDelta = artifact.summary.literal_cases - 689;
  process.stdout.write(
    `H2.6c census preflight passed: positive=${artifact.summary.positive_cases} negative=${artifact.summary.negative_cases} literal=${artifact.summary.literal_cases} (packet static positive=641 literal=689; effective delta=${positiveDelta >= 0 ? "+" : ""}${positiveDelta}/${literalDelta >= 0 ? "+" : ""}${literalDelta}, two virtual tsconfig cases account for the increase)\n`,
  );
} else if (MODE === "--write") {
  const artifact = buildArtifact();
  writeFileAtomic(path.join(WORKSPACE, TARGET_RELATIVE_PATH), render(artifact));
  process.stdout.write(
    `wrote ${TARGET_RELATIVE_PATH}: positive=${artifact.summary.positive_cases} negative=${artifact.summary.negative_cases} literal=${artifact.summary.literal_cases}\n`,
  );
} else if (MODE === undefined) {
  process.stdout.write(render(buildArtifact()));
} else {
  fail("usage: h2-6c-census.mjs [--preflight|--write|--check]");
}
