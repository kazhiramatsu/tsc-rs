// H2.6c m-1: the census-authoritative breadth map-observation machine.
//
// This is intentionally an authoring-side machine.  The frozen census is the
// only source of band membership: this file resolves a row's fixture and
// effective options, verifies the row facet vector, and (for --write or
// --probe) observes the pinned TypeScript implementation.  It never edits a
// production crate and --probe never writes a ratchet.

import { spawn } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "./vfs-directory-overlay.mjs";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const MODE = process.argv[2];

const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-6c-qualification.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-6c-qualification.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-6c-qualification.schema.json";
const CENSUS_RELATIVE_PATH = "ratchets/h2-6c-census.v1.json";
const CENSUS_SHA256 =
  "ffea234acbff0cbe5e85f3750cd18361d5ea2bea51cf43ca8bab36a2333726bf";
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
const OWNER_RELATIVE_PATH = "ratchets/h2-owner-inventory.v1.json";
const VFS_DIRECTORY_OVERLAY = "crates/oracle/vfs-directory-overlay.mjs";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const TYPESCRIPT_LIB_DIRECTORY = "vendor/typescript-6.0.3/lib";
const NODE_VERSION_PATH = ".node-version";

const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const VIRTUAL_SOURCE_ROOT = "/.src";
const PROJECT_DESCRIPTOR_ROOT = "project";
const PROJECT_TREE_ROOT = "projects";
const PROJECT_VIRTUAL_PREFIX = "/.src/tests/cases/projects";
const MAX_TRANSFORM_DEPTH = 256;
const PROGRESS_INTERVAL = 128;

const CHECK_SHARDS_ENV = "TSRS_H2_6C_CHECK_SHARDS";
const DEFAULT_CHECK_SHARDS = 4;
const MAX_CHECK_SHARDS = 8;
const INTERNAL_CHECK_SHARD_MODE = "--internal-check-shard";
const CHECK_RECEIPT_RELATIVE_PATH = "target/h2-6c/check-receipt.v1.json";
const RECEIPT_DEBUG = process.env.TSRS_H2_6C_RECEIPT_DEBUG === "1";

const MAP_OPTION_NAMES = Object.freeze([
  "sourceMap",
  "inlineSourceMap",
  "inlineSources",
  "sourceRoot",
  "mapRoot",
]);
const BOOLEAN_MAP_OPTIONS = new Set([
  "sourcemap",
  "inlinesourcemap",
  "inlinesources",
]);
const MAP_OPTION_BY_LOWER = new Map(
  MAP_OPTION_NAMES.map((name) => [name.toLowerCase(), name]),
);
const OPTION_INDEX = new Map(
  ts.optionDeclarations.map((option) => [option.name.toLowerCase(), option]),
);
const HARNESS_ONLY_OPTIONS = new Set(
  [
    "useCaseSensitiveFileNames", "baselineFile", "fileName", "filename",
    "suppressOutputPathCheck", "noImplicitReferences", "currentDirectory",
    "symlink", "link", "noTypesAndSymbols", "fullEmitPaths",
    "reportDiagnostics", "captureSuggestions", "typeScriptVersion",
  ].map((name) => name.toLowerCase()),
);
const UTF8_BOM = Buffer.from([0xef, 0xbb, 0xbf]);

const EXPECTED_CORPUS = Object.freeze({
  total: 15642,
  compiler: 7276,
  conformance: 7697,
  project: 632,
  transpile: 37,
});
const EXPECTED_CENSUS = Object.freeze({
  total: 691,
  positive: 643,
  negative: 48,
  unique_fixture_ids: 396,
  compiler: Object.freeze({ cases: 241, positive: 199, negative: 42 }),
  conformance: Object.freeze({ cases: 36, positive: 32, negative: 4 }),
  project: Object.freeze({ cases: 410, positive: 410, negative: 0 }),
  transpile: Object.freeze({ cases: 4, positive: 2, negative: 2 }),
});
const EXPECTED_FACET_COUNTS = Object.freeze({
  sourceMap: Object.freeze({
    literal_cases: 676, true_cases: 629, false_cases: 47,
    set_cases: 0, nonempty_cases: 0, empty_cases: 0,
  }),
  inlineSourceMap: Object.freeze({
    literal_cases: 12, true_cases: 11, false_cases: 1,
    set_cases: 0, nonempty_cases: 0, empty_cases: 0,
  }),
  inlineSources: Object.freeze({
    literal_cases: 7, true_cases: 7, false_cases: 0,
    set_cases: 0, nonempty_cases: 0, empty_cases: 0,
  }),
  sourceRoot: Object.freeze({
    literal_cases: 212, true_cases: 0, false_cases: 0,
    set_cases: 212, nonempty_cases: 212, empty_cases: 0,
  }),
  mapRoot: Object.freeze({
    literal_cases: 210, true_cases: 0, false_cases: 0,
    set_cases: 210, nonempty_cases: 210, empty_cases: 0,
  }),
});
const BAND_COUNTS = Object.freeze({
  total: 643,
  compiler: 199,
  conformance: 32,
  project: 410,
  transpile: 2,
});

const FEATURE_SLICES = Object.freeze({
  decorators: "H2.4b",
  "export-equals": "H2.2d",
  "import-equals": "H2.2d",
  jsx: "H2.3b",
  "parameter-properties": "H2.2c",
  "runtime-enums": "H2.2a",
  "runtime-namespaces": "H2.2b",
});
const FEATURE_ORDER = Object.freeze(Object.keys(FEATURE_SLICES));
const SLICE_ORDER = Object.freeze([
  "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b",
  "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c", "H2.3d", "H2.4a",
  "H2.4b", "H2.5a", "H2.5b", "H2.5c", "H2.5d", "H2.5e", "H2.5f",
  "H2.5g", "H2.5h", "H2.6a", "H2.6b", "H2.6c", "H2.7a", "H2.7b",
  "H2.7c", "H2.7d", "H2.7e", "H2.8a", "H2.8b", "H2.8c", "H2.8d",
  "H2.8e", "H2.9",
]);
const SLICE_RANK = new Map(SLICE_ORDER.map((slice, index) => [slice, index]));
const CLOSED_SLICES = new Set(
  SLICE_ORDER.slice(0, SLICE_ORDER.indexOf("H2.6c")),
);
const OWNER_KEYS = Object.freeze([
  "source-map-generator",
  "source-map-output-path",
]);

let observedCases = 0;
let reusedObservations = 0;
let shardAssignment = null;
let shardAdoption = null;
let shardOrdinal = 0;
let checkReceiptAttempt = false;

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

function hasValidFingerprint(value, field) {
  const payload = { ...value };
  const expected = payload[field];
  delete payload[field];
  return expected === sha256(Buffer.from(canonical(payload), "utf8"));
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
  requireCondition(
    typeof ts.emitFilesAndReportErrorsAndGetExitStatus === "function" &&
      typeof ts.sourceFileMayBeEmitted === "function" &&
      typeof ts.transpileModule === "function" &&
      typeof ts.transpileDeclaration === "function",
    "vendored compiler observation exports are unavailable",
  );
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

function orderedSettings(object) {
  return Object.entries(object).map(([name, value]) => ({ name, value }));
}

function contentIdentity(text) {
  if (text === undefined) return { state: "missing" };
  const bytes = Buffer.from(text, "utf8");
  return { state: "present", utf8_bytes: bytes.length, sha256: sha256(bytes) };
}

function documentSymlinks(fileOptions) {
  const setting = fileOptions.find((entry) => entry.name === "symlink");
  if (!setting || setting.value === "") return [];
  return setting.value.split(",").map((entry) => entry.trim());
}

function makeUnits(text, fixturePath) {
  const units = [];
  const links = [];
  let currentContent;
  let currentOptions = {};
  let currentName;
  const optionPattern = /^\/{2}\s*@([\w]+)\s*:\s*([^\r\n]*)/;
  const linkPattern = /^\/{2}\s*@link\s*:\s*([^\r\n]*)\s*->\s*([^\r\n]*)/;
  for (const line of text.split(/\r?\n/)) {
    const link = linkPattern.exec(line);
    if (link) {
      links.push({ target: link[1].trim(), link_path: link[2].trim() });
      continue;
    }
    const metadata = optionPattern.exec(line);
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
            if (!fileName.toLowerCase().startsWith(dir.toLowerCase())) continue;
            let relative = fileName.substring(dir.length);
            if (relative.startsWith("/")) relative = relative.substring(1);
            const separator = relative.indexOf("/");
            if (separator >= 0) directories.add(relative.substring(0, separator));
            else files.push(relative);
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
    category: ts.DiagnosticCategory[diagnostic.category],
    file: diagnostic.file?.fileName ?? null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    message: ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
  };
}

function parseConfigContext(loaded, recordedPlan) {
  const config = loaded.virtualConfig;
  requireCondition(config !== null, `${loaded.source.path} config is absent`);
  requireCondition(
    recordedPlan.source.path === loaded.source.path &&
      recordedPlan.source.index === loaded.sourceIndex &&
      recordedPlan.config_unit.id === loaded.configIndex &&
      recordedPlan.config_unit.name === config.name,
    `${loaded.source.path} recorded config identity changed`,
  );
  const source = ts.parseJsonText(config.name, config.text);
  const configFileName = ts.getNormalizedAbsolutePath(
    config.name,
    VIRTUAL_SOURCE_ROOT,
  );
  const host = createParseConfigHost(loaded.allUnits);
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
  requireCondition(
    canonical(parsed.fileNames) === canonical(recordedPlan.parsed_file_names) &&
      canonical(diagnostics) === canonical(recordedPlan.diagnostics) &&
      canonical(host.log) === canonical(recordedPlan.host_log),
    `${loaded.source.path} config plan changed`,
  );
  const normalIndexByOriginalId = new Map(
    loaded.units.map((unit, index) => [unit.original_id, index]),
  );
  const mapUnitIds = (ids, label) => ids.map((id) => {
    const index = normalIndexByOriginalId.get(id);
    requireCondition(index !== undefined, `${loaded.source.path} ${label} unit ${id} is absent`);
    return index;
  });
  const rootUnitIds = mapUnitIds(recordedPlan.root_unit_ids, "root");
  const otherUnitIds = mapUnitIds(recordedPlan.other_unit_ids, "other");
  const programRootUnitIds = mapUnitIds(
    recordedPlan.program_root_unit_ids,
    "program root",
  );
  return {
    options: ts.cloneCompilerOptions(parsed.options),
    selection: {
      root_unit_ids: rootUnitIds,
      other_unit_ids: otherUnitIds,
      program_root_unit_ids: programRootUnitIds,
      vfs_write_order: [...rootUnitIds, ...otherUnitIds],
    },
  };
}

function validateDirectiveFixture(loaded) {
  const parsed = makeUnits(loaded.text, loaded.source.path);
  requireCondition(
    canonical(parsed.links) === canonical(loaded.fixture.links),
    `${loaded.source.path} global links changed`,
  );
  const normalUnits = [...parsed.units];
  if (loaded.fixture.virtual_config !== null) {
    const configIndex = normalUnits.findIndex(
      (unit) => unit.name === loaded.fixture.virtual_config.name,
    );
    requireCondition(configIndex >= 0, `${loaded.source.path} virtual config is absent`);
    normalUnits.splice(configIndex, 1);
  }
  requireCondition(
    normalUnits.length === loaded.fixture.normal_units.length,
    `${loaded.source.path} unit count changed`,
  );
  normalUnits.forEach((unit, index) => {
    const expected = loaded.fixture.normal_units[index];
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

function loadDirectiveContext(suite, row, input, configPlans, cache) {
  const cacheKey = `${suite}:${row.source_index}`;
  if (cache.has(cacheKey)) return cache.get(cacheKey);
  const expansionCase = input.expansion.cases[row.expansion_case];
  requireCondition(
    expansionCase?.id === row.case_id &&
      expansionCase.source === row.source_index,
    `${row.case_id} expansion identity changed`,
  );
  const fixture = suite === "compiler"
    ? input.testExpansion.compiler_fixtures[expansionCase.source]
    : input.conformanceExpansion.fixtures.find(
        (candidate) => candidate.source === expansionCase.source,
      );
  requireCondition(fixture !== undefined, `${row.case_id} fixture is absent`);
  const source = input.expansion.sources[row.source_index];
  requireCondition(source !== undefined, `${row.case_id} source is absent`);
  requireCondition(
    canonical({
      path: source.path,
      bytes: source.bytes,
      sha256: source.sha256,
      git_blob_sha1: source.git_blob_sha1,
    }) === canonical(row.source),
    `${row.case_id} census source identity changed`,
  );
  if (suite === "compiler") {
    requireCondition(source.suite === "compiler", "compiler source suite changed");
  }
  const absolute = safeSourcePath(suite, source.path);
  const raw = fs.readFileSync(absolute);
  requireCondition(
    raw.length === source.bytes &&
      sha256(raw) === source.sha256 &&
      gitBlobSha1(raw) === source.git_blob_sha1,
    `${suite}/${source.path} source identity changed`,
  );
  const decoded = ts.sys.readFile(absolute);
  requireCondition(typeof decoded === "string", `cannot decode ${suite}/${source.path}`);
  requireCondition(
    Buffer.byteLength(decoded, "utf8") === fixture.decoded_utf8_bytes &&
      sha256(Buffer.from(decoded, "utf8")) === fixture.decoded_sha256,
    `${suite}/${source.path} decoded identity changed`,
  );
  const loaded = {
    suite,
    sourceIndex: row.source_index,
    source: row.source,
    fixture,
    text: decoded,
    units: null,
    allUnits: null,
    links: null,
    virtualConfig: null,
    configIndex: null,
    configContext: null,
  };
  loaded.units = validateDirectiveFixture(loaded);
  loaded.allUnits = [...loaded.units];
  if (fixture.virtual_config !== null) {
    const configIndex = loaded.units.findIndex(
      (unit) => unit.name === fixture.virtual_config.name,
    );
    requireCondition(configIndex >= 0, `${source.path} virtual config is absent`);
    const [configUnit] = loaded.units.splice(configIndex, 1);
    loaded.configIndex = configUnit.original_id;
    requireCondition(
      configUnit.name === fixture.virtual_config.name &&
        canonical(configUnit.file_options) === canonical(fixture.virtual_config.file_options) &&
        canonical(contentIdentity(configUnit.text)) === canonical(fixture.virtual_config.content) &&
        canonical(documentSymlinks(configUnit.file_options)) ===
          canonical(fixture.virtual_config.document_symlinks),
      `${source.path} virtual config changed`,
    );
    loaded.virtualConfig = configUnit;
    if (suite === "compiler") {
      const plan = configPlans.get(row.source_index);
      requireCondition(
        plan?.configuration_count === fixture.configurations.length,
        `${source.path} compiler config plan is absent`,
      );
      loaded.configContext = parseConfigContext(loaded, plan);
    }
  }
  loaded.links = fixture.links;
  cache.set(cacheKey, loaded);
  return loaded;
}

function mergedSettings(base, overrides) {
  const settings = new Map(base.map((setting) => [setting.name, setting.value]));
  for (const setting of overrides ?? []) settings.set(setting.name, setting.value);
  return settings;
}

function optionValue(option, raw) {
  const errors = [];
  let value;
  if (option.type === "boolean") value = String(raw).toLowerCase() === "true";
  else if (option.type === "string") value = String(raw);
  else if (option.type === "number") value = Number.parseInt(raw, 10);
  else if (option.type === "list" || option.type === "listOrElement") {
    value = ts.parseListTypeOption(option, raw, errors);
  } else value = ts.parseCustomTypeOption(option, raw, errors);
  requireCondition(errors.length === 0, `invalid @${option.name}: ${raw}`);
  return value;
}

function effectiveCompilerOptions(settings, baseOptions = { noResolve: false }) {
  const options = ts.cloneCompilerOptions(baseOptions);
  options.newLine = ts.NewLineKind.CarriageReturnLineFeed;
  options.noErrorTruncation = true;
  options.skipDefaultLibCheck = true;
  for (const [name, raw] of settings) {
    if (name === "typeScriptVersion") continue;
    const option = OPTION_INDEX.get(name.toLowerCase());
    if (option) {
      options[option.name] = optionValue(option, raw);
      continue;
    }
    requireCondition(
      HARNESS_ONLY_OPTIONS.has(name.toLowerCase()),
      `unknown harness/compiler option @${name}`,
    );
  }
  return options;
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
  return [...text.matchAll(/reference/g)].some((match) =>
    /^\s+path/.test(text.slice(match.index + "reference".length)),
  );
}

function explicitRootSelection(loaded, settings, options) {
  const cwd = currentDirectory(settings);
  const lastUnitByPath = new Map();
  loaded.units.forEach((unit, id) => {
    lastUnitByPath.set(ts.getNormalizedAbsolutePath(unit.name, cwd), id);
  });
  const candidates = [...lastUnitByPath.values()].sort((left, right) => left - right);
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
  return {
    root_unit_ids: rootUnitIds,
    other_unit_ids: otherUnitIds,
    program_root_unit_ids: rootUnitIds.filter(
      (id) =>
        !ts.fileExtensionIs(loaded.units[id].name, ts.Extension.Json) &&
        ts.isSupportedSourceFileName(loaded.units[id].name, options),
    ),
    vfs_write_order: [...rootUnitIds, ...otherUnitIds],
  };
}

function createProgramCase(loaded, selection, settings, options) {
  const cwd = currentDirectory(settings);
  const unitByPath = new Map();
  for (const id of selection.vfs_write_order) {
    const unit = loaded.units[id];
    requireCondition(unit.text !== undefined, `${loaded.source.path} has missing content`);
    unitByPath.set(ts.getNormalizedAbsolutePath(unit.name, cwd), { id, unit });
  }
  const vfsByPath = new Map(unitByPath);
  const symlinkByPath = new Map();
  for (const [target, fixture] of unitByPath) {
    for (const rawLink of documentSymlinks(fixture.unit.file_options)) {
      const link = ts.getNormalizedAbsolutePath(rawLink, cwd);
      if (!vfsByPath.has(link)) vfsByPath.set(link, fixture);
      symlinkByPath.set(link, target);
    }
  }
  const baseHost = ts.createCompilerHost(options, true);
  const directoryOverlay = createHermeticDirectoryOverlay(vfsByPath.keys(), {
    currentDirectory: cwd,
    useCaseSensitiveFileNames: true,
    fallbackHost: baseHost,
  });
  const host = {
    ...baseHost,
    getCurrentDirectory: () => cwd,
    useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: (fileName) => fileName,
    trace() {},
    fileExists(fileName) {
      const normalized = ts.normalizePath(fileName);
      return vfsByPath.has(normalized) || baseHost.fileExists(normalized);
    },
    readFile(fileName) {
      const normalized = ts.normalizePath(fileName);
      return vfsByPath.get(normalized)?.unit.text ?? baseHost.readFile(normalized);
    },
    directoryExists(directory) {
      return directoryOverlay.directoryExists(directory);
    },
    getDirectories(directory) {
      return directoryOverlay.getDirectories(directory);
    },
    realpath(fileName) {
      const normalized = ts.normalizePath(fileName);
      if (symlinkByPath.has(normalized)) return symlinkByPath.get(normalized);
      return vfsByPath.has(normalized)
        ? normalized
        : (baseHost.realpath?.(normalized) ?? normalized);
    },
    getSourceFile(fileName, languageVersion) {
      const normalized = ts.normalizePath(fileName);
      const fixture = vfsByPath.get(normalized);
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
  const roots = selection.program_root_unit_ids.map((id) =>
    ts.getNormalizedAbsolutePath(loaded.units[id].name, cwd),
  );
  return {
    program: ts.createProgram(roots, options, host),
    roots,
    cwd,
    unitByPath: vfsByPath,
    vfsSymlinks: [...symlinkByPath].map(([link_path, target_path]) => ({
      link_path,
      target_path,
    })),
  };
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
  facets[canonicalName] = {
    state: boolean ? (value ? "true" : "false") : "set",
    value,
    raw: raw === null ? null : String(raw),
    origin,
  };
}

function effectiveMapFacets(baseOptions, fixtureSettings, matrixSettings, origins) {
  const facets = {};
  for (const name of MAP_OPTION_NAMES) {
    facets[name] = { state: "absent", value: null, raw: null, origin: "absent" };
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
    if (name === undefined) continue;
    if (
      selectedMatrixNames.has(setting.name.toLowerCase()) &&
      String(setting.value).includes(",")
    ) continue;
    setFacet(facets, name, typedMapValue(name, setting.value), origins.fixture, setting.value);
  }
  for (const setting of matrixSettings ?? []) {
    const name = MAP_OPTION_BY_LOWER.get(setting.name.toLowerCase());
    if (name !== undefined) {
      setFacet(facets, name, typedMapValue(name, setting.value), origins.matrix, setting.value);
    }
  }
  return facets;
}

function assertFacetAgreement(row, facets, options) {
  requireCondition(
    canonical(facets) === canonical(row.option_facets),
    `${row.case_id} effective map facets diverge from census`,
  );
  for (const name of MAP_OPTION_NAMES) {
    const option = OPTION_INDEX.get(name.toLowerCase());
    const actual = options[option.name];
    const expected = row.option_facets[name];
    if (expected.state === "absent") {
      requireCondition(actual === undefined, `${row.case_id} ${name} became effective`);
    } else {
      requireCondition(
        actual === expected.value,
        `${row.case_id} effective ${name} differs from census facet`,
      );
    }
  }
}

function moduleStateName(moduleKind) {
  if (moduleKind === undefined) return "absent";
  const names = new Map([
    [ts.ModuleKind.None, "None(0)"], [ts.ModuleKind.CommonJS, "CommonJS(1)"],
    [ts.ModuleKind.AMD, "AMD(2)"], [ts.ModuleKind.UMD, "UMD(3)"],
    [ts.ModuleKind.System, "System(4)"], [ts.ModuleKind.ES2015, "ES2015(5)"],
    [ts.ModuleKind.ES2020, "ES2020(6)"], [ts.ModuleKind.ES2022, "ES2022(7)"],
    [ts.ModuleKind.ESNext, "ESNext(99)"], [ts.ModuleKind.Node16, "Node16(100)"],
    [ts.ModuleKind.Node18, "Node18(101)"], [ts.ModuleKind.Node20, "Node20(102)"],
    [ts.ModuleKind.NodeNext, "NodeNext(199)"], [ts.ModuleKind.Preserve, "Preserve(200)"],
  ]);
  const result = names.get(moduleKind);
  requireCondition(result !== undefined, `unexpected H2.6c module kind ${moduleKind}`);
  return result;
}

function targetStateName(target) {
  if (target === undefined) return "absent";
  const names = new Map([
    [ts.ScriptTarget.ES3, "ES3(0)"], [ts.ScriptTarget.ES5, "ES5(1)"],
    [ts.ScriptTarget.ES2015, "ES2015(2)"], [ts.ScriptTarget.ES2016, "ES2016(3)"],
    [ts.ScriptTarget.ES2017, "ES2017(4)"], [ts.ScriptTarget.ES2018, "ES2018(5)"],
    [ts.ScriptTarget.ES2019, "ES2019(6)"], [ts.ScriptTarget.ES2020, "ES2020(7)"],
    [ts.ScriptTarget.ES2021, "ES2021(8)"], [ts.ScriptTarget.ES2022, "ES2022(9)"],
    [ts.ScriptTarget.ES2023, "ES2023(10)"], [ts.ScriptTarget.ES2024, "ES2024(11)"],
    [ts.ScriptTarget.ESNext, "ESNext(99)"],
  ]);
  const result = names.get(target);
  requireCondition(result !== undefined, `unexpected H2.6c target ${target}`);
  return result;
}

// The project lane is the same hermetic whole-tree mount used by the closed
// H2.5h qualification lane.  It is deliberately kept in this authoring
// machine rather than delegated to a production crate: the mount is a
// TypeScript observation input, not a Rust implementation shortcut.
const PROJECT_STRUCTURAL_KEYS = new Set([
  "scenario", "projectRoot", "inputFiles", "baselineCheck", "runTest",
  "project", "emittedFiles", "resolveMapRoot", "resolveSourceRoot",
]);
const PROJECT_MODULE_VARIANTS = Object.freeze({
  amd: Object.freeze({ name: "amd", value: ts.ModuleKind.AMD }),
  commonjs: Object.freeze({ name: "commonjs", value: ts.ModuleKind.CommonJS }),
});

function walkProjectTree(testExpansion) {
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
  const expectedBacking = new Map(
    testExpansion.sources
      .filter((source) => source.suite === "projects")
      .map((source) => [source.path, source]),
  );
  const tree = files.map((absolute) => {
    const relative = path
      .relative(root, absolute)
      .split(path.sep)
      .join("/");
    const expected = expectedBacking.get(relative);
    requireCondition(expected !== undefined, `project mount has an unpinned file ${relative}`);
    const raw = fs.readFileSync(absolute);
    const text = ts.sys.readFile(absolute);
    requireCondition(typeof text === "string", `cannot decode project mount file ${relative}`);
    requireCondition(
      raw.length === expected.bytes &&
        sha256(raw) === expected.sha256 &&
        gitBlobSha1(raw) === expected.git_blob_sha1,
      `project mount file ${relative} identity changed`,
    );
    return {
      relative_path: relative,
      virtual_path: `${PROJECT_VIRTUAL_PREFIX}/${relative}`,
      text,
      bytes: raw.length,
      sha256: expected.sha256,
      git_blob_sha1: expected.git_blob_sha1,
    };
  });
  requireCondition(tree.length === expectedBacking.size, "project mount denominator changed");
  return tree;
}

function projectMountInventory(tree) {
  return withFingerprint({
    files: tree.map((entry) => ({
      path: entry.virtual_path,
      bytes: entry.bytes,
      sha256: entry.sha256,
      git_blob_sha1: entry.git_blob_sha1,
    })),
  }, "mount_fingerprint_sha256");
}

function loadProjectDescriptor(row) {
  const absolute = safeSourcePath(PROJECT_DESCRIPTOR_ROOT, row.source.path);
  const raw = fs.readFileSync(absolute);
  requireCondition(
    raw.length === row.source.bytes &&
      sha256(raw) === row.source.sha256 &&
      gitBlobSha1(raw) === row.source.git_blob_sha1,
    `project descriptor ${row.source.path} identity changed`,
  );
  const descriptor = JSON.parse(raw.toString("utf8"));
  for (const key of Object.keys(descriptor)) {
    if (!PROJECT_STRUCTURAL_KEYS.has(key) && !OPTION_INDEX.has(key.toLowerCase())) {
      fail(`project descriptor ${row.source.path} carries unknown key ${key}`);
    }
  }
  requireCondition(
    typeof descriptor.scenario === "string" &&
      typeof descriptor.projectRoot === "string",
    `project descriptor ${row.source.path} is missing scenario/projectRoot`,
  );
  return descriptor;
}

function projectRootSelectionRecord(descriptor, cwd, mountByPath) {
  if (Array.isArray(descriptor.inputFiles)) {
    return {
      state: "explicit-inputs",
      roots: descriptor.inputFiles.map((requested) => {
        const absolute = ts.getNormalizedAbsolutePath(requested, cwd);
        return { requested, path: absolute, present: mountByPath.has(absolute) };
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

function projectEffectiveOptions(descriptor, variant, configOptions) {
  const options = {
    ...configOptions,
    moduleResolution:
      configOptions.moduleResolution ?? ts.ModuleResolutionKind.Classic,
    noErrorTruncation: false,
    skipDefaultLibCheck: false,
    newLine: ts.NewLineKind.CarriageReturnLineFeed,
  };
  for (const [key, raw] of Object.entries(descriptor)) {
    if (PROJECT_STRUCTURAL_KEYS.has(key)) continue;
    const option = OPTION_INDEX.get(key.toLowerCase());
    requireCondition(option !== undefined, `project descriptor option ${key} is not a compiler option`);
    options[option.name] = optionValue(option, String(raw));
  }
  options.module = variant.value;
  delete options.noEmit;
  return options;
}

function createProjectProgramCase(mountByPath, cwd, rootPaths, options) {
  const baseHost = ts.createCompilerHost(options, true);
  const defaultLibraryFileName = ts.combinePaths(
    baseHost.getDefaultLibLocation(),
    "lib.es5.d.ts",
  );
  const directoryOverlay = createHermeticDirectoryOverlay(mountByPath.keys(), {
    currentDirectory: cwd,
    useCaseSensitiveFileNames: true,
    fallbackHost: baseHost,
  });
  const host = {
    ...baseHost,
    getCurrentDirectory: () => cwd,
    useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: (fileName) => fileName,
    getDefaultLibFileName: () => defaultLibraryFileName,
    trace() {},
    fileExists(fileName) {
      const normalized = ts.normalizePath(fileName);
      return mountByPath.has(normalized) || baseHost.fileExists(normalized);
    },
    readFile(fileName) {
      const normalized = ts.normalizePath(fileName);
      return mountByPath.get(normalized)?.text ?? baseHost.readFile(normalized);
    },
    directoryExists(directory) {
      return directoryOverlay.directoryExists(directory);
    },
    getDirectories(directory) {
      return directoryOverlay.getDirectories(directory);
    },
    realpath(fileName) {
      const normalized = ts.normalizePath(fileName);
      return mountByPath.has(normalized)
        ? normalized
        : (baseHost.realpath?.(normalized) ?? normalized);
    },
    getSourceFile(fileName, languageVersion) {
      const normalized = ts.normalizePath(fileName);
      const mounted = mountByPath.get(normalized);
      if (!mounted) return baseHost.getSourceFile(fileName, languageVersion);
      return ts.createSourceFile(
        normalized,
        mounted.text,
        languageVersion,
        true,
        ts.getScriptKindFromFileName(normalized),
      );
    },
  };
  return {
    program: ts.createProgram(rootPaths, options, host),
    roots: rootPaths,
    cwd,
    unitByPath: mountByPath,
    vfsSymlinks: [],
  };
}

function compareProjectSelection(actual, recorded, caseId) {
  requireCondition(actual.state === recorded.state, `${caseId} root-selection arm changed`);
  if (actual.state === "explicit-inputs") {
    requireCondition(
      canonical(actual.roots) === canonical(recorded.roots.map((root) => ({
        requested: root.requested,
        path: root.path,
        present: root.presence.state === "present",
      }))),
      `${caseId} explicit roots changed`,
    );
  } else {
    requireCondition(actual.config_path === recorded.config?.path, `${caseId} config path changed`);
  }
}

function resolveProjectRow(row, input, projectState) {
  const classified = projectState.classificationById.get(row.case_id);
  requireCondition(classified !== undefined, `${row.case_id} project classification is absent`);
  requireCondition(
    classified.source === row.source_index &&
      classified.expansion_case === row.expansion_case &&
      classified.descriptor_path === row.source.path,
    `${row.case_id} project classification identity changed`,
  );
  const expansionCase = input.testExpansion.cases[row.expansion_case];
  requireCondition(
    expansionCase?.id === row.case_id &&
      expansionCase.source === row.source_index &&
      expansionCase.configuration.kind === "project",
    `${row.case_id} project expansion identity changed`,
  );
  const descriptor = loadProjectDescriptor(row);
  const cwd = ts.normalizePath(`${VIRTUAL_SOURCE_ROOT}/${descriptor.projectRoot}`);
  requireCondition(cwd === classified.current_directory, `${row.case_id} current directory changed`);
  const variant = PROJECT_MODULE_VARIANTS[classified.module_variant.name];
  requireCondition(
    variant !== undefined &&
      variant.value === classified.module_variant.value &&
      variant.name === row.configuration_variant &&
      expansionCase.configuration.module === variant.name,
    `${row.case_id} project module variant changed`,
  );
  const selection = projectRootSelectionRecord(descriptor, cwd, projectState.mountByPath);
  compareProjectSelection(selection, classified.root_selection, row.case_id);
  requireCondition(selection.state === row.root_mode, `${row.case_id} root mode changed`);
  let configOptions = {};
  let configFileNames = null;
  let configDiagnostics = [];
  if (selection.state !== "explicit-inputs") {
    const parsed = parseProjectConfig(selection.config_path, cwd, projectState.mountByPath);
    configOptions = parsed.options;
    configFileNames = parsed.file_names;
    configDiagnostics = parsed.diagnostics;
    requireCondition(row.project_config_path === selection.config_path, `${row.case_id} config path changed`);
    requireCondition(canonical(row.reached_files) === canonical(configFileNames), `${row.case_id} reached files changed`);
  } else {
    const reached = selection.roots.filter((root) => root.present).map((root) => root.path);
    requireCondition(row.project_config_path === null, `${row.case_id} unexpected project config`);
    requireCondition(canonical(row.reached_files) === canonical(reached), `${row.case_id} reached files changed`);
  }
  requireCondition(canonical(configDiagnostics) === canonical(row.config_diagnostics), `${row.case_id} config diagnostics changed`);
  const options = projectEffectiveOptions(descriptor, variant, configOptions);
  const facets = {};
  for (const name of MAP_OPTION_NAMES) {
    facets[name] = { state: "absent", value: null, raw: null, origin: "absent" };
  }
  for (const name of MAP_OPTION_NAMES) {
    const option = OPTION_INDEX.get(name.toLowerCase());
    if (Object.hasOwn(configOptions, option.name) && configOptions[option.name] !== undefined) {
      setFacet(facets, name, configOptions[option.name], "project-config", configOptions[option.name]);
    }
  }
  for (const [key, raw] of Object.entries(descriptor)) {
    const name = MAP_OPTION_BY_LOWER.get(key.toLowerCase());
    if (name !== undefined) setFacet(facets, name, typedMapValue(name, raw), "descriptor", raw);
  }
  assertFacetAgreement(row, facets, options);
  const rootPaths = selection.state === "explicit-inputs"
    ? selection.roots.map((root) => root.path)
    : configFileNames;
  const projectInput = {
    descriptor: {
      path: row.source.path,
      scenario: descriptor.scenario,
      project_root: descriptor.projectRoot,
    },
    current_directory: cwd,
    root_selection: selection,
    module_variant: { name: variant.name, value: variant.value },
    config_diagnostics: configDiagnostics,
    mount_fingerprint: projectState.mountInventory.mount_fingerprint_sha256,
  };
  return {
    row,
    suite: "project",
    route: "project-mount",
    source: row.source,
    options,
    facets,
    target_state: targetStateName(options.target),
    module_state: moduleStateName(options.module),
    projectInput,
    identity: sha256(Buffer.from(canonical({
      row: row.case_id,
      source: row.source,
      facets,
      project_input: projectInput,
      roots: rootPaths,
    }), "utf8")),
    makeProgram: () => createProjectProgramCase(
      projectState.mountByPath,
      cwd,
      rootPaths,
      options,
    ),
  };
}

function scriptKindName(fileName) {
  const names = ["Unknown", "JS", "JSX", "TS", "TSX", "External", "JSON", "Deferred"];
  return names[ts.getScriptKindFromFileName(fileName)] ?? "Unknown";
}

function impliedFormatName(value) {
  if (value === ts.ModuleKind.CommonJS) return "CommonJS";
  if (value === ts.ModuleKind.ESNext) return "ESModule";
  return "None";
}

function outputSlice(fileName) {
  const lower = fileName.toLowerCase();
  if (lower.endsWith(".d.ts") || lower.endsWith(".d.mts") || lower.endsWith(".d.cts")) return null;
  if (lower.endsWith(".ts")) return null;
  if (lower.endsWith(".mts") || lower.endsWith(".cts")) return "H2.1e";
  if (lower.endsWith(".js") || lower.endsWith(".mjs") || lower.endsWith(".cjs")) return "H2.3a";
  if (lower.endsWith(".tsx") || lower.endsWith(".jsx")) return "H2.3b";
  if (lower.endsWith(".json")) return "H2.3d";
  return "H2.9";
}

function featureRoots(sourceFile) {
  const roots = [];
  function record(feature, node) {
    roots.push({
      feature,
      start: node.getStart(sourceFile),
      end: node.end,
      kind: ts.SyntaxKind[node.kind],
    });
  }
  const stack = [sourceFile];
  while (stack.length > 0) {
    const node = stack.pop();
    if (ts.isEnumDeclaration(node)) record("runtime-enums", node);
    if (
      ts.isModuleDeclaration(node) &&
      !ts.isAmbientModule(node) &&
      (node.flags & ts.NodeFlags.Ambient) === 0
    ) record("runtime-namespaces", node);
    if (ts.isImportEqualsDeclaration(node)) record("import-equals", node);
    if (ts.isExportAssignment(node) && node.isExportEquals) record("export-equals", node);
    if (ts.isJsxElement(node) || ts.isJsxFragment(node) || ts.isJsxSelfClosingElement(node)) {
      record("jsx", node);
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
    ) record("parameter-properties", node);
    if (ts.canHaveDecorators(node) && (ts.getDecorators(node)?.length ?? 0) > 0) {
      record("decorators", node);
    }
    ts.forEachChild(node, (child) => stack.push(child));
  }
  return roots.sort(
    (left, right) =>
      left.start - right.start ||
      left.end - right.end ||
      FEATURE_ORDER.indexOf(left.feature) - FEATURE_ORDER.indexOf(right.feature),
  );
}

function maximumAstDepth(root) {
  let maximum = 0;
  const stack = [[root, 1]];
  while (stack.length !== 0) {
    const [node, depth] = stack.pop();
    maximum = Math.max(maximum, depth);
    ts.forEachChild(node, (child) => stack.push([child, depth + 1]));
  }
  return maximum;
}

function hasImportAttributes(root) {
  const stack = [root];
  while (stack.length !== 0) {
    const node = stack.pop();
    if (
      (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) &&
      node.attributes !== undefined
    ) return true;
    ts.forEachChild(node, (child) => stack.push(child));
  }
  return false;
}

function hasCommentedOptionalChainTypeAssertion(text) {
  return text.split(/\r?\n/).some(
    (line) =>
      line.includes("/*") &&
      line.includes("?.") &&
      (/\bas\b/.test(line) || /<[^>]+>/.test(line)),
  );
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
  if (lower.endsWith(".js")) return "javascript";
  if (lower.endsWith(".jsx")) return "jsx";
  if (lower.endsWith(".mjs")) return "mjs";
  if (lower.endsWith(".cjs")) return "cjs";
  if (lower.endsWith(".d.ts") || lower.endsWith(".d.mts") || lower.endsWith(".d.cts")) return "declaration";
  if (lower.endsWith(".map")) return "source-map";
  return "other";
}

function inlineMapPayload(text) {
  const match = text.match(/sourceMappingURL=data:application\/json;base64,([A-Za-z0-9+/=]+)/);
  if (!match) return null;
  const encoded = match[1];
  const decoded = Buffer.from(encoded, "base64");
  return {
    data_uri_base64: encoded,
    decoded_utf8_base64: decoded.toString("base64"),
    decoded_utf8_bytes: decoded.length,
    decoded_utf8_sha256: sha256(decoded),
  };
}

function serializeWrite(arguments_, index) {
  const [fileName, text, bom, onError, sourceFiles, data] = arguments_;
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
    materialized_utf8_base64: materialized.toString("base64"),
    materialized_utf8_sha256: sha256(materialized),
    materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined,
    source_files: (sourceFiles ?? []).map((source) => ts.normalizePath(source.fileName)),
    data_present: data !== undefined,
    data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics_count: data?.diagnostics === undefined ? null : data.diagnostics.length,
    inline_source_map_payload: inlineMapPayload(text),
  };
}

function serializeSourceMaps(sourceMaps) {
  if (sourceMaps === undefined) return null;
  return sourceMaps.map((entry) => {
    // This is the one JSON.stringify authority for an upstream SourceMap
    // object.  The exact string is reused for map-write/inline comparisons.
    const sourceMapJson = JSON.stringify(entry.sourceMap);
    return {
      input_source_file_names: entry.inputSourceFileNames.map((name) => ts.normalizePath(name)),
      source_map_json: sourceMapJson,
      source_map_json_utf8_base64: Buffer.from(sourceMapJson, "utf8").toString("base64"),
      source_map_json_utf8_sha256: sha256(Buffer.from(sourceMapJson, "utf8")),
    };
  });
}

function observeTypeScript(makeProgram) {
  const { program } = makeProgram();
  const writes = [];
  const reported = [];
  const statusWrites = [];
  let emitResult;
  const originalEmit = program.emit;
  program.emit = function capture(...arguments_) {
    requireCondition(emitResult === undefined, "TypeScript emitted more than once");
    emitResult = originalEmit.apply(this, arguments_);
    return emitResult;
  };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(
    program,
    (diagnostic) => reported.push(diagnostic),
    (text) => statusWrites.push(text),
    undefined,
    (...arguments_) => writes.push(arguments_),
  );
  requireCondition(emitResult !== undefined, "TypeScript did not call Program.emit");
  const emitRefused = emitResult.emitSkipped;
  return withFingerprint(
    {
      writes: writes.map(serializeWrite),
      reported_diagnostics: reported.map(serializeDiagnostic),
      emit_refused: emitRefused,
      emit_result: {
        emit_skipped: emitResult.emitSkipped,
        emit_refused: emitRefused,
        diagnostics: emitResult.diagnostics.map(serializeDiagnostic),
        emitted_files: emitResult.emittedFiles === undefined
          ? null
          : emitResult.emittedFiles.map((fileName) => ts.normalizePath(fileName)),
        source_maps: serializeSourceMaps(emitResult.sourceMaps),
      },
      status_writes: statusWrites,
      exit_code: exit,
    },
    "run_fingerprint_sha256",
  );
}

function serializeTranspileOutput(unit, output) {
  const outputBytes = Buffer.from(output.outputText ?? "", "utf8");
  const sourceMapBytes = output.sourceMapText === undefined || output.sourceMapText === null
    ? null
    : Buffer.from(output.sourceMapText, "utf8");
  return {
    unit: unit.original_id,
    path: ts.normalizePath(unit.name),
    output_utf8_base64: outputBytes.toString("base64"),
    output_utf8_sha256: sha256(outputBytes),
    output_utf8_bytes: outputBytes.length,
    source_map_json: sourceMapBytes === null ? null : sourceMapBytes.toString("utf8"),
    source_map_json_utf8_base64: sourceMapBytes === null ? null : sourceMapBytes.toString("base64"),
    source_map_json_utf8_sha256: sourceMapBytes === null ? null : sha256(sourceMapBytes),
    inline_source_map_payload: inlineMapPayload(output.outputText ?? ""),
    diagnostics: (output.diagnostics ?? []).map(serializeDiagnostic),
  };
}

function observeTranspile(resolved) {
  const api = resolved.transpileApi;
  const outputs = resolved.loaded.units.map((unit) => {
    const result = ts[api](unit.text, {
      compilerOptions: resolved.options,
      fileName: unit.name,
      reportDiagnostics: true,
    });
    return serializeTranspileOutput(unit, result);
  });
  const diagnostics = outputs.flatMap((output) => output.diagnostics);
  const sourceMaps = outputs
    .filter((output) => output.source_map_json !== null)
    .map((output) => ({
      input_source_file_names: [output.path],
      source_map_json: output.source_map_json,
      source_map_json_utf8_base64: output.source_map_json_utf8_base64,
      source_map_json_utf8_sha256: output.source_map_json_utf8_sha256,
    }));
  return withFingerprint(
    {
      api,
      writes: [],
      reported_diagnostics: diagnostics,
      emit_refused: false,
      emit_result: {
        emit_skipped: false,
        emit_refused: false,
        diagnostics,
        emitted_files: null,
        source_maps: sourceMaps.length === 0 ? null : sourceMaps,
      },
      status_writes: [],
      exit_code: 0,
      transpile_outputs: outputs,
    },
    "run_fingerprint_sha256",
  );
}

function observationFor(resolved) {
  return resolved.route === "transpile-api"
    ? observeTranspile(resolved)
    : observeTypeScript(resolved.makeProgram);
}

function sourceTextForMounted(mounted) {
  return mounted?.unit?.text ?? mounted?.text;
}

function analyzeProgram(resolved) {
  const { program, roots, cwd, unitByPath, vfsSymlinks } = resolved.makeProgram();
  const files = [];
  const requiredSlices = new Set();
  for (const sourceFile of program.getSourceFiles()) {
    const normalized = ts.normalizePath(sourceFile.fileName);
    const mounted = unitByPath.get(normalized);
    if (!mounted) continue;
    if (resolved.suite === "project" && !normalized.startsWith(`${PROJECT_VIRTUAL_PREFIX}/`)) continue;
    const text = sourceTextForMounted(mounted);
    const emitEligible = ts.sourceFileMayBeEmitted(sourceFile, program, false);
    const rootsFound = featureRoots(sourceFile);
    const emitFormat = program.getEmitModuleFormatOfFile(sourceFile);
    const parseDiagnosticCodes = [
      ...new Set(sourceFile.parseDiagnostics.map((diagnostic) => diagnostic.code)),
    ].sort((left, right) => left - right);
    const maxAst = maximumAstDepth(sourceFile);
    const importAttributes = hasImportAttributes(sourceFile);
    const advancedCommentPlacement =
      /\.\.\.[\t \r\n]*\/(?:\*|\/)/.test(sourceFile.text) ||
      /#[A-Za-z_$][\w$]*[\t ]*\/\*.*?\*\/(?:[\t \r\n]|\/\*.*?\*\/)*\bin\b/s.test(sourceFile.text) ||
      hasCommentedOptionalChainTypeAssertion(sourceFile.text);
    if (emitEligible) {
      const slice = outputSlice(sourceFile.fileName);
      if (slice) requiredSlices.add(slice);
      for (const root of rootsFound) requiredSlices.add(FEATURE_SLICES[root.feature]);
      if (parseDiagnosticCodes.length !== 0) requiredSlices.add("H2.9");
      if (maxAst > MAX_TRANSFORM_DEPTH) requiredSlices.add("H2.9");
      if (importAttributes) requiredSlices.add("H2.1e");
      if (advancedCommentPlacement) requiredSlices.add("H2.8a");
    }
    const file = {
      path: normalized,
      script_kind: scriptKindName(sourceFile.fileName),
      declaration_file: sourceFile.isDeclarationFile,
      emit_eligible: emitEligible,
      implied_module_format: impliedFormatName(sourceFile.impliedNodeFormat),
      emit_module_format: emitFormat,
      feature_roots: rootsFound,
      parse_diagnostic_codes: parseDiagnosticCodes,
      max_ast_depth: maxAst,
      import_attributes: importAttributes,
      advanced_comment_placement: advancedCommentPlacement,
      text_sha256: sha256(Buffer.from(text ?? sourceFile.text, "utf8")),
    };
    if (resolved.suite !== "project") file.unit = mounted.id;
    files.push(file);
  }
  if (resolved.suite === "project") {
    for (const root of resolved.projectInput.root_selection.roots ?? []) {
      if (!root.present) continue;
      requireCondition(files.some((file) => file.path === root.path), `${resolved.row.case_id} did not reach present root ${root.path}`);
    }
  } else {
    requireCondition(
      resolved.selection.program_root_unit_ids.every((id) => files.some((file) => file.unit === id)),
      `${resolved.row.case_id} did not reach every root`,
    );
  }
  const orderedSlices = [...requiredSlices].sort(
    (left, right) => SLICE_RANK.get(left) - SLICE_RANK.get(right),
  );
  const remainingSlices = orderedSlices.filter((slice) => !CLOSED_SLICES.has(slice));
  const first = observationFor(resolved);
  const second = observationFor(resolved);
  requireCondition(
    first.run_fingerprint_sha256 === second.run_fingerprint_sha256,
    `${resolved.row.case_id} TypeScript observation is not deterministic`,
  );
  const analysis = {
    files,
    owner_reachability: OWNER_KEYS,
    disposition: remainingSlices.length === 0 ? "admitted-for-execution" : "deferred-to-slices",
    required_slices: remainingSlices,
    diagnostic_disposition: remainingSlices.length === 0
      ? { state: "exact-required" }
      : { state: "not-observed-source-deferred" },
    typescript_observation: first,
    typescript_run_fingerprints: [first.run_fingerprint_sha256, second.run_fingerprint_sha256],
  };
  if (resolved.suite === "project") {
    analysis.project_input = { ...resolved.projectInput, analyzed_files: files };
    delete analysis.files;
  } else {
    analysis.input = {
      current_directory: cwd,
      roots,
      vfs_symlinks: vfsSymlinks,
      settings: [...resolved.settings].map(([name, value]) => ({ name, value })),
      virtual_config: resolved.loaded.virtualConfig === null
        ? null
        : (() => {
            const bytes = Buffer.from(resolved.loaded.virtualConfig.text, "utf8");
            return {
              path: ts.getNormalizedAbsolutePath(resolved.loaded.virtualConfig.name, cwd),
              utf8_base64: bytes.toString("base64"),
              utf8_sha256: sha256(bytes),
              utf8_bytes: bytes.length,
            };
          })(),
      files: resolved.selection.vfs_write_order.map((id) => {
        const unit = resolved.loaded.units[id];
        const bytes = Buffer.from(unit.text, "utf8");
        return {
          unit: id,
          path: ts.getNormalizedAbsolutePath(unit.name, cwd),
          utf8_base64: bytes.toString("base64"),
          utf8_sha256: sha256(bytes),
          utf8_bytes: bytes.length,
        };
      }),
    };
  }
  return analysis;
}

function assertCensus(census, censusBytes) {
  requireCondition(
    sha256(censusBytes) === CENSUS_SHA256,
    `census file hash changed: expected ${CENSUS_SHA256}`,
  );
  requireCondition(
    census.schema === 1 &&
      census.kind === "h2-6c-map-applicability-census" &&
      census.status === "measured-not-observed" &&
      census.phase === "H2.6c-map-applicability" &&
      hasValidFingerprint(census, "census_fingerprint_sha256"),
    "invalid H2.6c census authority",
  );
  const summary = census.summary;
  requireCondition(summary.corpus_cases === EXPECTED_CORPUS.total, "census corpus denominator changed");
  requireCondition(
    summary.census_cases === EXPECTED_CENSUS.total &&
      summary.literal_cases === EXPECTED_CENSUS.total &&
      summary.positive_cases === EXPECTED_CENSUS.positive &&
      summary.negative_cases === EXPECTED_CENSUS.negative &&
      summary.unique_fixture_ids === EXPECTED_CENSUS.unique_fixture_ids &&
      summary.explicit_false_cases === EXPECTED_CENSUS.negative,
    "census total counts changed",
  );
  requireCondition(census.cases.length === EXPECTED_CENSUS.total, "census row count changed");
  requireCondition(
    new Set(census.cases.map((row) => row.case_id)).size === census.cases.length,
    "census case IDs are not unique",
  );
  for (const suite of ["compiler", "conformance", "project", "transpile"]) {
    const rows = census.cases.filter((row) => row.suite === suite);
    const expected = EXPECTED_CENSUS[suite];
    requireCondition(
      rows.length === expected.cases &&
        rows.filter((row) => row.positive).length === expected.positive &&
        rows.filter((row) => !row.positive).length === expected.negative &&
        summary[`${suite}_census_cases`] === expected.cases &&
        summary[`${suite}_positive_cases`] === expected.positive &&
        summary[`${suite}_negative_cases`] === expected.negative,
      `census ${suite} counts changed`,
    );
  }
  const facetCounts = {};
  for (const name of MAP_OPTION_NAMES) {
    const count = {
      literal_cases: 0,
      true_cases: 0,
      false_cases: 0,
      set_cases: 0,
      nonempty_cases: 0,
      empty_cases: 0,
    };
    for (const row of census.cases) {
      const facet = row.option_facets[name];
      requireCondition(facet !== undefined, `${row.case_id} lacks census facet ${name}`);
      if (facet.state === "absent") continue;
      count.literal_cases += 1;
      if (facet.state === "true") count.true_cases += 1;
      if (facet.state === "false") count.false_cases += 1;
      if (facet.state === "set") {
        count.set_cases += 1;
        if (facet.value.length > 0) count.nonempty_cases += 1;
        else count.empty_cases += 1;
      }
    }
    facetCounts[name] = count;
  }
  requireCondition(
    canonical(facetCounts) === canonical(EXPECTED_FACET_COUNTS) &&
      canonical(summary.facet_counts) === canonical(EXPECTED_FACET_COUNTS),
    "census facet counts changed",
  );
  const positiveRows = census.cases.filter((row) => row.positive === true);
  requireCondition(positiveRows.length === BAND_COUNTS.total, "positive census band changed");
  for (const suite of Object.keys(BAND_COUNTS).filter((key) => key !== "total")) {
    requireCondition(
      positiveRows.filter((row) => row.suite === suite).length === BAND_COUNTS[suite],
      `positive ${suite} band count changed`,
    );
  }
  return positiveRows;
}

function readCensus() {
  const bytes = readBytes(CENSUS_RELATIVE_PATH);
  const census = JSON.parse(bytes.toString("utf8"));
  assertCensus(census, bytes);
  return census;
}

function loadInputs() {
  const testExpansion = readJson(TEST_SUITE_EXPANSION);
  const conformanceExpansion = readJson(CONFORMANCE_EXPANSION);
  const compilerClassification = readJson(COMPILER_CLASSIFICATION);
  const conformanceClassification = readJson(CONFORMANCE_CLASSIFICATION);
  const compilerConfigPlans = readJson(COMPILER_CONFIG_PLANS);
  const projectClassification = readJson(PROJECT_CLASSIFICATION);
  const transpileInventory = readJson(TRANSPILE_INVENTORY);
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
  return {
    testExpansion,
    conformanceExpansion,
    compilerClassification,
    conformanceClassification,
    compilerConfigPlans,
    projectClassification,
    transpileInventory,
    typescript: {
      version: ts.version,
      source_commit: SOURCE_COMMIT,
      bundle: pathHash(TYPESCRIPT_BUNDLE),
      implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
    },
    inputs: {
      census: pathHash(CENSUS_RELATIVE_PATH),
      test_suite_expansion: pathHash(TEST_SUITE_EXPANSION),
      conformance_expansion: pathHash(CONFORMANCE_EXPANSION),
      compiler_classification: pathHash(COMPILER_CLASSIFICATION),
      conformance_classification: pathHash(CONFORMANCE_CLASSIFICATION),
      compiler_config_plans: pathHash(COMPILER_CONFIG_PLANS),
      project_classification: pathHash(PROJECT_CLASSIFICATION),
      transpile_inventory: pathHash(TRANSPILE_INVENTORY),
      vfs_directory_overlay: pathHash(VFS_DIRECTORY_OVERLAY),
      owner_inventory: pathHash(OWNER_RELATIVE_PATH),
    },
  };
}

function loadDirectiveInputs(input, row, configPlans, cache) {
  const suite = row.suite;
  const expansion = suite === "compiler"
    ? input.testExpansion
    : input.conformanceExpansion;
  return loadDirectiveContext(
    suite,
    row,
    { ...input, expansion },
    configPlans,
    cache,
  );
}

function resolveDirectiveRow(row, input, configPlanBySource, cache) {
  const suite = row.suite;
  const loaded = loadDirectiveInputs(input, row, configPlanBySource, cache);
  requireCondition(loaded.links.length === 0, `${row.case_id} requires global-link topology support`);
  const configuration = loaded.fixture.configurations[row.configuration_index];
  requireCondition(configuration !== undefined, `${row.case_id} configuration is absent`);
  requireCondition(
    configuration.variant === row.configuration_variant,
    `${row.case_id} configuration variant changed`,
  );
  const settings = mergedSettings(loaded.fixture.settings, configuration.settings);
  const options = effectiveCompilerOptions(
    settings,
    loaded.configContext?.options ?? { noResolve: false },
  );
  const facets = effectiveMapFacets(
    loaded.configContext?.options ?? {},
    loaded.fixture.settings,
    configuration.settings,
    { base: "virtual-config", fixture: "fixture", matrix: "matrix" },
  );
  assertFacetAgreement(row, facets, options);
  const selection = loaded.configContext?.selection ?? explicitRootSelection(loaded, settings, options);
  const identity = sha256(Buffer.from(canonical({
    case_id: row.case_id,
    source: row.source,
    facets,
    settings: [...settings],
    selection,
    units: loaded.units.map((unit) => ({
      id: unit.original_id,
      name: unit.name,
      text_sha256: sha256(Buffer.from(unit.text, "utf8")),
    })),
  }), "utf8"));
  return {
    row,
    suite,
    route: suite === "compiler" ? "recorded-compiler-plan" : "qualified-vfs",
    source: row.source,
    loaded,
    settings,
    options,
    facets,
    selection,
    target_state: targetStateName(options.target),
    module_state: moduleStateName(options.module),
    identity,
    makeProgram: () => createProgramCase(loaded, selection, settings, options),
  };
}

function resolveTranspileRow(row, input) {
  const inventory = input.transpileInventory;
  const entry = inventory.cases[row.case_index];
  requireCondition(entry?.id === row.case_id && entry.source === row.source_index, `${row.case_id} transpile identity changed`);
  const fixture = inventory.fixtures.find((candidate) => candidate.source === entry.source);
  requireCondition(fixture !== undefined, `${row.case_id} transpile fixture is absent`);
  const source = inventory.sources[entry.source];
  requireCondition(
    canonical({
      path: source.path,
      bytes: source.bytes,
      sha256: source.sha256,
      git_blob_sha1: source.git_blob_sha1,
    }) === canonical(row.source),
    `${row.case_id} transpile source identity changed`,
  );
  const absolute = safeSourcePath("transpile", source.path);
  const raw = fs.readFileSync(absolute);
  requireCondition(
    raw.length === source.bytes && sha256(raw) === source.sha256 && gitBlobSha1(raw) === source.git_blob_sha1,
    `${row.case_id} transpile source bytes changed`,
  );
  const text = ts.sys.readFile(absolute);
  requireCondition(
    typeof text === "string" &&
      Buffer.byteLength(text, "utf8") === fixture.decoded_utf8_bytes &&
      sha256(Buffer.from(text, "utf8")) === fixture.decoded_sha256,
    `${row.case_id} transpile decoded identity changed`,
  );
  const parsed = makeUnits(text, source.path);
  requireCondition(parsed.links.length === 0, `${row.case_id} transpile links are unsupported`);
  requireCondition(parsed.units.length === fixture.units.length, `${row.case_id} transpile unit count changed`);
  parsed.units.forEach((unit, index) => {
    const expected = fixture.units[index];
    requireCondition(
      unit.name === expected.name &&
        canonical(unit.file_options) === canonical(expected.file_options) &&
        unit.text !== undefined &&
        contentIdentity(unit.text).utf8_bytes === expected.content.utf8_bytes &&
        contentIdentity(unit.text).sha256 === expected.content.sha256,
      `${row.case_id} transpile unit ${index} changed`,
    );
  });
  const configuration = fixture.configurations[row.configuration_index];
  requireCondition(configuration !== undefined && configuration.variant === row.configuration_variant, `${row.case_id} transpile configuration changed`);
  const settings = mergedSettings(fixture.settings, configuration.overrides);
  const options = effectiveCompilerOptions(settings);
  const facets = effectiveMapFacets(
    {},
    fixture.settings,
    configuration.overrides,
    { base: "virtual-config", fixture: "fixture", matrix: "matrix" },
  );
  assertFacetAgreement(row, facets, options);
  return {
    row,
    suite: "transpile",
    route: "transpile-api",
    source: row.source,
    loaded: { ...fixture, text, units: parsed.units },
    settings,
    options,
    facets,
    transpileApi: entry.api,
    transpileKind: entry.kind,
    target_state: targetStateName(options.target),
    module_state: moduleStateName(options.module),
    identity: sha256(Buffer.from(canonical({
      case_id: row.case_id,
      source: row.source,
      facets,
      settings: [...settings],
      units: parsed.units.map((unit) => ({ name: unit.name, text_sha256: sha256(Buffer.from(unit.text, "utf8")) })),
    }), "utf8")),
  };
}

function buildProjectState(input) {
  const tree = walkProjectTree(input.testExpansion);
  const mountInventory = projectMountInventory(tree);
  const mountByPath = new Map(tree.map((entry) => [entry.virtual_path, entry]));
  const classificationById = new Map(
    input.projectClassification.cases.map((entry) => [entry.id, entry]),
  );
  requireCondition(
    classificationById.size === input.projectClassification.cases.length,
    "project classification IDs are not unique",
  );
  return { tree, mountInventory, mountByPath, classificationById };
}

function ownerClosure() {
  const owner = readJson(OWNER_RELATIVE_PATH);
  return OWNER_KEYS.map((key) => {
    const row = owner.owners.find((entry) => entry.key === key);
    requireCondition(row?.owner_slice === "H2.6a", `missing source-map owner ${key}`);
    return {
      key,
      declaration: row.declaration,
      disposition_before_h2_6c: row.disposition,
    };
  });
}

function prepareRows(rows) {
  const census = readCensus();
  const input = loadInputs();
  const positiveRows = census.cases.filter((row) => row.positive === true);
  requireCondition(positiveRows.length === BAND_COUNTS.total, "positive band selection changed");
  const requested = rows ?? positiveRows;
  const requestedIds = new Set(requested.map((row) => row.case_id));
  requireCondition(
    requested.every((row) =>
      row.positive === true && census.cases.some((candidate) => candidate.case_id === row.case_id),
    ),
    "requested row is outside the census positive band",
  );
  const configPlanBySource = new Map(
    input.compilerConfigPlans.fixtures.map((entry) => [entry.source.index, entry]),
  );
  const projectRows = requested.filter((row) => row.suite === "project");
  const projectState = projectRows.length === 0 ? null : buildProjectState(input);
  const directiveCache = new Map();
  const resolved = requested.map((row) => {
    if (row.suite === "project") return resolveProjectRow(row, input, projectState);
    if (row.suite === "transpile") return resolveTranspileRow(row, input);
    return resolveDirectiveRow(row, input, configPlanBySource, directiveCache);
  });
  requireCondition(
    new Set(resolved.map((entry) => entry.row.case_id)).size === resolved.length &&
      resolved.every((entry) => requestedIds.has(entry.row.case_id)),
    "resolved band rows are not unique",
  );
  return {
    census,
    input,
    positiveRows,
    resolved,
    projectState,
    agreement: resolved.length,
  };
}

function transpileAnalysis(resolved) {
  const first = observationFor(resolved);
  const second = observationFor(resolved);
  requireCondition(
    first.run_fingerprint_sha256 === second.run_fingerprint_sha256,
    `${resolved.row.case_id} TypeScript observation is not deterministic`,
  );
  const bytes = resolved.loaded.units.map((unit) => Buffer.from(unit.text, "utf8"));
  return {
    transpile_input: {
      api: resolved.transpileApi,
      kind: resolved.transpileKind,
      current_directory: VIRTUAL_SOURCE_ROOT,
      settings: [...resolved.settings].map(([name, value]) => ({ name, value })),
      files: resolved.loaded.units.map((unit, index) => ({
        unit: unit.original_id,
        path: ts.normalizePath(unit.name),
        utf8_base64: bytes[index].toString("base64"),
        utf8_sha256: sha256(bytes[index]),
        utf8_bytes: bytes[index].length,
      })),
    },
    owner_reachability: OWNER_KEYS,
    disposition: "deferred-to-slices",
    required_slices: ["H2.8c"],
    diagnostic_disposition: { state: "not-observed-source-deferred" },
    typescript_observation: first,
    typescript_run_fingerprints: [first.run_fingerprint_sha256, second.run_fingerprint_sha256],
  };
}

function analyzeResolved(resolved) {
  if (resolved.route === "transpile-api") return transpileAnalysis(resolved);
  return analyzeProgram(resolved);
}

function makeCaseRecord(resolved) {
  const analysis = analyzeResolved(resolved);
  return withFingerprint(
    {
      suite: resolved.suite,
      case_id: resolved.row.case_id,
      fixture_id: resolved.row.fixture_id,
      matrix_key: resolved.row.matrix_key,
      selection_origin: "census-positive-band",
      execution_route: resolved.route,
      expansion_case: resolved.suite === "transpile" || resolved.suite === "project"
        ? null
        : resolved.row.expansion_case,
      configuration_index: resolved.suite === "project"
        ? null
        : resolved.row.configuration_index,
      source: resolved.source,
      option_facets: resolved.facets,
      positive_options: resolved.row.positive_options,
      explicit_false_options: resolved.row.explicit_false_options,
      target_state: resolved.target_state,
      module_state: resolved.module_state,
      observation_input_sha256: resolved.identity,
      ...analysis,
      rust_expectation:
        analysis.disposition === "admitted-for-execution"
          ? "two-deterministic-exact-runs"
          : "typed-failure-before-first-sink-write",
    },
    "case_fingerprint_sha256",
  );
}

function countBy(values) {
  const counts = new Map();
  for (const value of values) counts.set(value, (counts.get(value) ?? 0) + 1);
  return [...counts]
    .map(([value, cases]) => ({ value, cases }))
    .sort((left, right) => right.cases - left.cases || left.value.localeCompare(right.value));
}

function countObservationField(cases, field) {
  return cases.reduce(
    (total, entry) => total + (entry.typescript_observation?.[field]?.length ?? 0),
    0,
  );
}

function buildSummary(cases, census) {
  const admitted = cases.filter((entry) => entry.disposition === "admitted-for-execution");
  const deferred = cases.filter((entry) => entry.disposition === "deferred-to-slices");
  const projectCases = cases.filter((entry) => entry.suite === "project");
  const directiveCases = cases.filter((entry) => entry.input !== undefined);
  const noEmitCases = cases.filter((entry) => {
    const files = entry.files ?? entry.project_input?.analyzed_files ?? [];
    return !files.some((file) => file.emit_eligible);
  });
  const summary = {
    census_cases: census.summary.census_cases,
    literal_cases: census.summary.literal_cases,
    positive_cases: census.summary.positive_cases,
    negative_cases: census.summary.negative_cases,
    candidates: BAND_COUNTS.total,
    observed_candidates: cases.length,
    compiler_candidates: cases.filter((entry) => entry.suite === "compiler").length,
    conformance_candidates: cases.filter((entry) => entry.suite === "conformance").length,
    project_candidates: projectCases.length,
    transpile_candidates: cases.filter((entry) => entry.suite === "transpile").length,
    deferred_project_mount: 0,
    project_mount_cases: projectCases.length,
    recorded_compiler_plan_cases: cases.filter((entry) => entry.execution_route === "recorded-compiler-plan").length,
    qualified_vfs_cases: cases.filter((entry) => entry.execution_route === "qualified-vfs").length,
    transpile_api_cases: cases.filter((entry) => entry.execution_route === "transpile-api").length,
    virtual_config_cases: directiveCases.filter((entry) => entry.input.virtual_config !== null).length,
    vfs_symlink_cases: directiveCases.filter((entry) => entry.input.vfs_symlinks.length > 0).length,
    vfs_symlink_paths: directiveCases.reduce((sum, entry) => sum + entry.input.vfs_symlinks.length, 0),
    admitted_cases: admitted.length,
    deferred_cases: deferred.length,
    no_emit_control_cases: noEmitCases.length,
    target_states: countBy(cases.map((entry) => entry.target_state)),
    module_states: countBy(cases.map((entry) => entry.module_state)),
    dispositions: countBy(cases.map((entry) => entry.disposition)),
    first_deferred_slices: countBy(deferred.map((entry) => entry.required_slices[0])),
    typescript_runs: cases.reduce((sum, entry) => sum + entry.typescript_run_fingerprints.length, 0),
    deterministic_typescript_cases: cases.filter((entry) => entry.typescript_run_fingerprints[0] === entry.typescript_run_fingerprints[1]).length,
    emit_refused_cases: cases.filter((entry) => entry.typescript_observation.emit_refused).length,
    typescript_writes: countObservationField(cases, "writes"),
    typescript_reported_diagnostics: countObservationField(cases, "reported_diagnostics"),
    typescript_emit_diagnostics: cases.reduce((total, entry) => total + (entry.typescript_observation.emit_result?.diagnostics.length ?? 0), 0),
    source_map_result_entries: cases.reduce((total, entry) => total + (entry.typescript_observation.emit_result?.source_maps?.length ?? 0), 0),
    facet_agreement_cases: cases.length,
    unexecuted_candidates: 0,
    undispositioned_candidates: cases.filter((entry) => !entry.disposition || (entry.disposition === "deferred-to-slices" && entry.required_slices.length === 0)).length,
  };
  requireCondition(summary.candidates === BAND_COUNTS.total && summary.observed_candidates === BAND_COUNTS.total, "qualification band partition is incomplete");
  for (const suite of ["compiler", "conformance", "project", "transpile"]) {
    requireCondition(summary[`${suite}_candidates`] === BAND_COUNTS[suite], `${suite} qualification band partition changed`);
  }
  requireCondition(summary.facet_agreement_cases === BAND_COUNTS.total, "not every band row passed facet agreement");
  requireCondition(summary.typescript_runs === BAND_COUNTS.total * 2, "TypeScript repetition count changed");
  requireCondition(summary.deterministic_typescript_cases === BAND_COUNTS.total, "TypeScript determinism is open");
  requireCondition(summary.undispositioned_candidates === 0, "H2.6c retained an undispositioned case");
  return summary;
}

function admissionContract() {
  return "the 643 census-positive rows are the complete band (199 compiler, 32 conformance, 410 project, 2 transpile); no applicability is re-derived for selection; each row's effective sourceMap/inlineSourceMap/inlineSources/sourceRoot/mapRoot facet vector must byte-equal the frozen census vector; compiler/conformance/project rows capture exact TypeScript writes, callback data, inline payloads, source-map records, emitted files, diagnostics, emitSkipped/emit_refused, and order; transpile rows capture the API output and map payload with the API route explicit; project rows use the hermetic whole-tree mount and deferred_project_mount is empty";
}

function executionContract() {
  return {
    source_reachability: "fixture VFS roots plus module-resolved fixture dependencies in a vendored TypeScript Program; project rows use the pinned whole-tree mount",
    module_selection: "Program.getEmitModuleFormatOfFile for every reached fixture SourceFile; transpile rows retain the inventory API/kind",
    admission: admissionContract(),
    typescript_repetitions: 2,
    rust_repetitions: 2,
    normalization: "none",
    deferred_boundary: "typed failure before first sink write",
  };
}

function libraryInventoryRecord() {
  const directory = path.join(WORKSPACE, TYPESCRIPT_LIB_DIRECTORY);
  const names = fs
    .readdirSync(directory)
    .filter((name) => name.startsWith("lib.") && name.endsWith(".d.ts"))
    .sort();
  requireCondition(names.length > 0, "vendored TypeScript lib inventory is empty");
  const hash = crypto.createHash("sha256");
  for (const name of names) {
    hash.update(name);
    hash.update("\u0000");
    hash.update(fs.readFileSync(path.join(directory, name)));
    hash.update("\u0000");
  }
  return {
    path: TYPESCRIPT_LIB_DIRECTORY,
    default_libraries: names.length,
    sha256: hash.digest("hex"),
  };
}

// `owner_inventory` and `global_candidate_dispositions` are pin-carrying
// ratchet artifacts: a pin-only rebind changes their file bytes without
// changing anything an observation can see. Compare their observation-
// relevant projections instead; owner closure and every per-case identity
// remain explicit receipt guards below.
function observationInputs(inputs) {
  const {
    owner_inventory: _ownerInventory,
    global_candidate_dispositions: _globalDispositions,
    ...rest
  } = inputs;
  return rest;
}

function globalRecordsSha(context, contract, owners, phase) {
  const terms = {
    typescript: context.input.typescript,
    observation_inputs: observationInputs(context.input.inputs),
    execution_contract: contract,
    owner_closure: owners,
    census_fingerprint_sha256: context.census.census_fingerprint_sha256,
    project_mount_fingerprint_sha256: context.projectState?.mountInventory.mount_fingerprint_sha256 ?? null,
    library_inventory: libraryInventoryRecord(),
  };
  if (RECEIPT_DEBUG) {
    process.stderr.write(
      `H2.6c receipt debug (${phase}): ${JSON.stringify({
        ...terms,
        inputs: context.input.inputs,
      })}\n`,
    );
  }
  return sha256(Buffer.from(canonical(terms), "utf8"));
}

function loadCheckReceipt(context, contract, owners) {
  let receipt;
  try {
    receipt = JSON.parse(readBytes(CHECK_RECEIPT_RELATIVE_PATH).toString("utf8"));
  } catch {
    throw new CheckReceiptMiss("absent-or-invalid");
  }
  const globalRecords = globalRecordsSha(context, contract, owners, "check");
  if (
    receipt === null ||
    typeof receipt !== "object" ||
    receipt.schema !== 1 ||
    receipt.kind !== "h2-6c-qualification-check-receipt" ||
    !hasValidFingerprint(receipt, "receipt_fingerprint_sha256") ||
    receipt.workspace !== fs.realpathSync(WORKSPACE) ||
    receipt.node !== process.version ||
    receipt.generator_sha256 !== pathHash(GENERATOR_RELATIVE_PATH).sha256 ||
    receipt.global_records_sha256 !== globalRecords
  ) throw new CheckReceiptMiss("stale");
  return receipt;
}

function casesObservationSha(caseFingerprints) {
  const hash = crypto.createHash("sha256");
  for (const fingerprint of [...caseFingerprints].sort()) {
    hash.update(fingerprint);
    hash.update("\u0000");
  }
  return hash.digest("hex");
}

function storedCaseReusable(stored, resolved) {
  return (
    stored !== null &&
    typeof stored === "object" &&
    stored.case_id === resolved.row.case_id &&
    stored.observation_input_sha256 === resolved.identity &&
    canonical(stored.option_facets) === canonical(resolved.facets) &&
    hasValidFingerprint(stored, "case_fingerprint_sha256")
  );
}

function reusableStoredCases(context, contract, owners) {
  if (MODE !== "--write" && !checkReceiptAttempt) return null;
  const receipt = checkReceiptAttempt
    ? loadCheckReceipt(context, contract, owners)
    : null;
  let stored;
  try {
    stored = JSON.parse(readBytes(TARGET_RELATIVE_PATH).toString("utf8"));
  } catch {
    if (checkReceiptAttempt) throw new CheckReceiptMiss("stored-artifact");
    return null;
  }
  if (
    stored.schema !== 1 ||
    stored.status !== "qualified-typescript-oracle" ||
    stored.phase !== "H2.6c-map-observation" ||
    !hasValidFingerprint(stored, "qualification_fingerprint_sha256") ||
    canonical(stored.typescript) !== canonical(context.input.typescript) ||
    canonical(observationInputs(stored.inputs)) !==
      canonical(observationInputs(context.input.inputs)) ||
    canonical(stored.execution_contract) !== canonical(contract) ||
    canonical(stored.owner_closure) !== canonical(owners) ||
    stored.selection_contract?.census_file_sha256 !== CENSUS_SHA256 ||
    stored.selection_contract?.census_fingerprint_sha256 !== context.census.census_fingerprint_sha256 ||
    stored.project_mount?.mount_fingerprint_sha256 !==
      (context.projectState?.mountInventory.mount_fingerprint_sha256 ?? null)
  ) {
    if (checkReceiptAttempt) throw new CheckReceiptMiss("global-records");
    return null;
  }
  if (!Array.isArray(stored.cases) || stored.cases.length !== BAND_COUNTS.total) {
    if (checkReceiptAttempt) throw new CheckReceiptMiss("stored-cases");
    return null;
  }
  if (receipt !== null) {
    const fingerprints = stored.cases.map((entry) => entry?.case_fingerprint_sha256);
    if (
      fingerprints.some((value) => typeof value !== "string") ||
      receipt.cases_observation_sha256 !== casesObservationSha(fingerprints)
    ) throw new CheckReceiptMiss("observation-content");
  }
  return new Map(stored.cases.map((entry) => [entry.case_id, entry]));
}

function reportObservationProgress(caseId) {
  observedCases += 1;
  const target = shardAssignment === null ? BAND_COUNTS.total : shardAssignment.assigned;
  if (observedCases % PROGRESS_INTERVAL === 0 || observedCases === target) {
    const suffix = shardAssignment === null
      ? ""
      : ` [shard ${shardAssignment.index + 1}/${shardAssignment.count}]`;
    process.stderr.write(`H2.6c TypeScript observations${suffix}: ${observedCases}/${target} (${caseId})\n`);
  }
}

function buildCaseRecords(prepared, contract, owners) {
  const reuseByCaseId = reusableStoredCases(prepared, contract, owners);
  const records = [];
  for (const resolved of prepared.resolved) {
    const row = resolved.row;
    const stored = reuseByCaseId?.get(row.case_id);
    if (stored !== undefined && storedCaseReusable(stored, resolved)) {
      reusedObservations += 1;
      records.push(stored);
      reportObservationProgress(row.case_id);
      continue;
    }
    if (checkReceiptAttempt) throw new CheckReceiptMiss(`case ${row.case_id}`);
    if (shardAdoption !== null) {
      const adopted = shardAdoption.get(row.case_id);
      requireCondition(adopted !== undefined, `${row.case_id} is missing from shard observations`);
      records.push(adopted);
      reportObservationProgress(row.case_id);
      continue;
    }
    if (shardAssignment !== null) {
      const ordinal = shardOrdinal;
      shardOrdinal += 1;
      if (ordinal % shardAssignment.count !== shardAssignment.index) continue;
    }
    const record = makeCaseRecord(resolved);
    records.push(record);
    reportObservationProgress(row.case_id);
  }
  return records;
}

function buildArtifact() {
  const prepared = prepareRows(null);
  const owners = ownerClosure();
  const contract = executionContract();
  const records = buildCaseRecords(prepared, contract, owners);
  if (shardAssignment !== null) {
    requireCondition(
      records.length === shardAssignment.assigned && shardOrdinal === BAND_COUNTS.total,
      `shard ${shardAssignment.index} observed ${records.length}/${shardAssignment.assigned} of ${shardOrdinal} enumerated cases`,
    );
    return { shard_cases: records };
  }
  requireCondition(records.length === BAND_COUNTS.total, `unexpected H2.6c observed denominator ${records.length}`);
  requireCondition(new Set(records.map((entry) => entry.case_id)).size === records.length, "duplicate H2.6c case");
  const artifact = withFingerprint(
    {
      schema: 1,
      status: "qualified-typescript-oracle",
      phase: "H2.6c-map-observation",
      typescript: prepared.input.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        census: {
          path: CENSUS_RELATIVE_PATH,
          sha256: CENSUS_SHA256,
          fingerprint_sha256: prepared.census.census_fingerprint_sha256,
        },
        project_mount_decision: "mounted-all-project-rows",
      },
      selection_contract: {
        census_file_sha256: CENSUS_SHA256,
        census_fingerprint_sha256: prepared.census.census_fingerprint_sha256,
        candidate_definition: "the rows selected by the frozen census positive=true field; no applicability is re-derived by this machine",
        census_literal_denominator: EXPECTED_CENSUS.total,
        census_positive_denominator: EXPECTED_CENSUS.positive,
        census_negative_denominator: EXPECTED_CENSUS.negative,
        global_candidate_denominator: BAND_COUNTS.total,
        observed_candidate_denominator: records.length,
        suite_counts: {
          compiler: BAND_COUNTS.compiler,
          conformance: BAND_COUNTS.conformance,
          project: BAND_COUNTS.project,
          transpile: BAND_COUNTS.transpile,
        },
        deferred_project_mount: [],
      },
      inputs: prepared.input.inputs,
      execution_contract: contract,
      owner_closure: owners,
      project_mount: prepared.projectState?.mountInventory ?? null,
      cases: records,
      summary: buildSummary(records, prepared.census),
    },
    "qualification_fingerprint_sha256",
  );
  return artifact;
}

function checkShardCount() {
  const raw = process.env[CHECK_SHARDS_ENV];
  if (raw === undefined) return DEFAULT_CHECK_SHARDS;
  const value = Number(raw);
  requireCondition(Number.isInteger(value) && value >= 1 && value <= MAX_CHECK_SHARDS, `${CHECK_SHARDS_ENV} must be an integer from 1 to ${MAX_CHECK_SHARDS}`);
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
  return {
    index,
    count,
    assigned: Math.floor((BAND_COUNTS.total - 1 - index) / count) + 1,
  };
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
        reject(new Error(`observation shard ${index} exited with ${code}`));
        return;
      }
      try {
        const payload = JSON.parse(Buffer.concat(chunks).toString("utf8"));
        requireCondition(
          payload?.schema === 1 &&
            payload.shard_index === index &&
            payload.shard_count === count &&
            Array.isArray(payload.shard_cases),
          `observation shard ${index} returned an invalid payload`,
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
    Array.from({ length: count }, (_, index) => observeShardInChildProcess(index, count)),
  );
  const adoption = new Map();
  for (const cases of shardCases) {
    for (const record of cases) {
      requireCondition(
        record !== null && typeof record === "object" &&
          typeof record.case_id === "string" && !adoption.has(record.case_id),
        "shard observations overlap or are malformed",
      );
      adoption.set(record.case_id, record);
    }
  }
  requireCondition(adoption.size === BAND_COUNTS.total, `sharded check observed ${adoption.size}/${BAND_COUNTS.total} cases`);
  shardAdoption = adoption;
  const artifact = buildArtifact();
  const rendered = render(artifact);
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-6c-qualification.mjs --write and review`,
  );
  mintCheckReceipt(artifact);
  process.stdout.write(`H2.6c qualification is fresh: positive=${artifact.summary.positive_cases} observed=${artifact.summary.observed_candidates} admitted=${artifact.summary.admitted_cases} deferred=${artifact.summary.deferred_cases} check_shards=${count} check_receipt=minted\n`);
}

function mintCheckReceipt(artifact) {
  const context = {
    input: { typescript: artifact.typescript, inputs: artifact.inputs },
    census: { census_fingerprint_sha256: artifact.selection_contract.census_fingerprint_sha256 },
    projectState: artifact.project_mount === null ? null : { mountInventory: artifact.project_mount },
  };
  const contract = artifact.execution_contract;
  const owners = artifact.owner_closure;
  const receipt = withFingerprint({
    schema: 1,
    kind: "h2-6c-qualification-check-receipt",
    minted_by: "full-re-observation-check",
    workspace: fs.realpathSync(WORKSPACE),
    node: process.version,
    generator_sha256: artifact.generator.sha256,
    global_records_sha256: globalRecordsSha(context, contract, owners, "mint"),
    cases_observation_sha256: casesObservationSha(artifact.cases.map((entry) => entry.case_fingerprint_sha256)),
  }, "receipt_fingerprint_sha256");
  const absolute = path.join(WORKSPACE, CHECK_RECEIPT_RELATIVE_PATH);
  fs.mkdirSync(path.dirname(absolute), { recursive: true });
  writeFileAtomic(absolute, render(receipt));
}

function attemptReceiptCheck() {
  checkReceiptAttempt = true;
  try {
    const artifact = buildArtifact();
    requireCondition(reusedObservations === BAND_COUNTS.total, `check receipt adopted ${reusedObservations}/${BAND_COUNTS.total} cases`);
    process.stderr.write(`H2.6c check receipt: hit; adopted ${BAND_COUNTS.total} stored observations under the full per-case guards\n`);
    return artifact;
  } catch (error) {
    if (!(error instanceof CheckReceiptMiss)) throw error;
    observedCases = 0;
    reusedObservations = 0;
    shardOrdinal = 0;
    process.stderr.write(`H2.6c check receipt: miss (${error.message}); running the full re-observation\n`);
    return null;
  } finally {
    checkReceiptAttempt = false;
  }
}

function parseProbeCount(argv) {
  requireCondition(argv.length === 4, "--probe requires a case count");
  const count = Number(argv[3]);
  requireCondition(Number.isInteger(count) && count >= 1 && count <= 5, "--probe count must be an integer from 1 to 5");
  return count;
}

function printBandPreflight(prepared) {
  const summary = prepared.census.summary;
  const suiteCounts = Object.fromEntries(
    ["compiler", "conformance", "project", "transpile"].map((suite) => [
      suite,
      prepared.positiveRows.filter((row) => row.suite === suite).length,
    ]),
  );
  const projectRows = prepared.positiveRows.filter((row) => row.suite === "project").length;
  const projectDecision = prepared.projectState === null
    ? "not-needed-for-selected-sample"
    : `mounted ${projectRows}/${BAND_COUNTS.project}`;
  process.stdout.write(
    `H2.6c census authority asserted: file_sha256=${CENSUS_SHA256} fingerprint=${prepared.census.census_fingerprint_sha256} counts=positive:${summary.positive_cases} negative:${summary.negative_cases} literal:${summary.literal_cases}\n`,
  );
  process.stdout.write(
    `H2.6c positive band selected: total=${BAND_COUNTS.total} compiler=${suiteCounts.compiler} conformance=${suiteCounts.conformance} project=${suiteCounts.project} transpile=${suiteCounts.transpile}\n`,
  );
  process.stdout.write(
    `H2.6c project mount decision: ${projectDecision}; deferred_project_mount=[] (denominator=${BAND_COUNTS.project})\n`,
  );
  process.stdout.write(
    `H2.6c option facet agreement verified: ${prepared.agreement}/${prepared.resolved.length} selected rows\n`,
  );
  process.stdout.write(
    `H2.6c expected-shape stats: ${canonical({
      facet_counts: EXPECTED_FACET_COUNTS,
      target_states: countBy(prepared.resolved.map((entry) => entry.target_state)),
      module_states: countBy(prepared.resolved.map((entry) => entry.module_state)),
    })}\n`,
  );
}

function runPreflight() {
  const prepared = prepareRows(null);
  printBandPreflight(prepared);
  process.stdout.write("H2.6c qualification fixture/config/project/facet preflight passed\n");
}

function runProbe(count) {
  const census = readCensus();
  const compilerRows = census.cases.filter((row) => row.suite === "compiler" && row.positive === true);
  const selected = compilerRows.slice(0, count);
  const prepared = prepareRows(selected);
  requireCondition(prepared.resolved.length === count, `probe resolved ${prepared.resolved.length}/${count} compiler rows`);
  const results = [];
  for (const resolved of prepared.resolved) {
    const analysis = analyzeResolved(resolved);
    requireCondition(
      analysis.typescript_run_fingerprints[0] === analysis.typescript_run_fingerprints[1],
      `${resolved.row.case_id} probe observation is not deterministic`,
    );
    results.push({
      case_id: resolved.row.case_id,
      fingerprint: analysis.typescript_run_fingerprints[0],
      writes: analysis.typescript_observation.writes.length,
      emit_refused: analysis.typescript_observation.emit_refused,
      source_maps: analysis.typescript_observation.emit_result.source_maps?.length ?? 0,
    });
  }
  process.stdout.write(
    `H2.6c probe passed: cases=${count} suite=compiler facet_agreement=${prepared.agreement}/${count} deterministic=${results.filter((entry) => entry.fingerprint !== undefined).length}/${count} tree_writes=0\n`,
  );
  for (const result of results) {
    process.stdout.write(
      `  ${result.case_id}: fingerprint=${result.fingerprint} writes=${result.writes} source_maps=${result.source_maps} emit_refused=${result.emit_refused}\n`,
    );
  }
}

validateRuntime();
if (MODE === INTERNAL_CHECK_SHARD_MODE) {
  shardAssignment = parseShardArguments(process.argv);
  const artifact = buildArtifact();
  process.stdout.write(render({
    schema: 1,
    shard_index: shardAssignment.index,
    shard_count: shardAssignment.count,
    shard_cases: artifact.shard_cases,
  }));
} else if (MODE === "--preflight") {
  runPreflight();
} else if (MODE === "--probe") {
  runProbe(parseProbeCount(process.argv));
} else if (MODE === "--check") {
  const receiptArtifact = attemptReceiptCheck();
  if (receiptArtifact !== null) {
    const rendered = render(receiptArtifact);
    requireCondition(
      fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
        fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") === rendered,
      `stale ${TARGET_RELATIVE_PATH}; run h2-6c-qualification.mjs --write and review`,
    );
    process.stdout.write(`H2.6c qualification is fresh: positive=${receiptArtifact.summary.positive_cases} observed=${receiptArtifact.summary.observed_candidates} admitted=${receiptArtifact.summary.admitted_cases} deferred=${receiptArtifact.summary.deferred_cases} check_receipt=hit reused_observations=${reusedObservations}\n`);
  } else if (checkShardCount() > 1) {
    await runShardedCheck(checkShardCount());
  } else {
    const artifact = buildArtifact();
    const rendered = render(artifact);
    requireCondition(
      fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
        fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") === rendered,
      `stale ${TARGET_RELATIVE_PATH}; run h2-6c-qualification.mjs --write and review`,
    );
    mintCheckReceipt(artifact);
    process.stdout.write(`H2.6c qualification is fresh: positive=${artifact.summary.positive_cases} observed=${artifact.summary.observed_candidates} admitted=${artifact.summary.admitted_cases} deferred=${artifact.summary.deferred_cases} check_receipt=minted\n`);
  }
} else if (MODE === "--write") {
  const artifact = buildArtifact();
  writeFileAtomic(path.join(WORKSPACE, TARGET_RELATIVE_PATH), render(artifact));
  process.stdout.write(`wrote ${TARGET_RELATIVE_PATH}: positive=${artifact.summary.positive_cases} observed=${artifact.summary.observed_candidates} admitted=${artifact.summary.admitted_cases} deferred=${artifact.summary.deferred_cases} reused_observations=${reusedObservations}\n`);
} else if (MODE === undefined) {
  process.stdout.write(render(buildArtifact()));
} else {
  fail("usage: h2-6c-qualification.mjs [--preflight|--write|--check|--probe N]");
}
