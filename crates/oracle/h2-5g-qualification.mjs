import { spawn } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "./vfs-directory-overlay.mjs";
import {
  PIN_INDEX_RELATIVE_PATH,
  consumerRows,
  normalizedSha256,
  pinIndexRowsSha256,
} from "./pin-normalize.mjs";
import {
  collectStaleJournals,
  openJournalWriter,
  readJournal,
  receiptKey,
  receiptMissTerm,
  removeJournals,
  renderReceipt,
} from "./h2-5g-check-resume.mjs";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const MODE = process.argv[2];
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5g-qualification.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5g-qualification.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-5g-qualification.schema.json";
const H2_5F_PROFILE_RELATIVE_PATH = "ratchets/h2-5f-profile.v1.json";
const GLOBAL_DISPOSITIONS_RELATIVE_PATH =
  "ratchets/h2-candidate-dispositions.v1.json";
const OWNER_RELATIVE_PATH = "ratchets/h2-owner-inventory.v1.json";
const COMPILER_CLASSIFICATION =
  "vendor/typescript-6.0.3/compiler-profile-classification.v1.json";
const CONFORMANCE_CLASSIFICATION =
  "vendor/typescript-6.0.3/conformance-profile-classification.v1.json";
const COMPILER_EXPANSION =
  "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const COMPILER_CONFIG_PLANS =
  "vendor/typescript-6.0.3/compiler-config-plans.v1.json";
const CONFORMANCE_EXPANSION =
  "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json";
const VFS_DIRECTORY_OVERLAY = "crates/oracle/vfs-directory-overlay.mjs";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const NODE_VERSION_PATH = ".node-version";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const H2_5F_MERGE_COMMIT = "11f5d0abb93fed4b109bdb1dc552721ceb05e707";
const H2_5F_PROFILE_SHA256 =
  "921888fdc2fd032d1cd578d4c1fda28a47af39ffd757746d694ab17e3142397b";
const EXPANDED_RUN_LAYOUT_GENERATOR_SHA256 =
  "5255ec1656d1cbbf812736054e297828fd3ff51f7a8159068afcb87bedfaf453";
const COMPACT_RUN_LAYOUT_GENERATOR_SHA256 =
  "941b5f659c84c8c5c3a4e02fcfa5871a6ba3b2b7afebd644abd2e2d9f7c5cc81";
const EXPECTED_NODE = "25.2.1";
const VIRTUAL_SOURCE_ROOT = "/.src";
const MAX_TRANSFORM_DEPTH = 256;
const PROGRESS_INTERVAL = 256;
const CHECK_SHARDS_ENV = "TSRS_H2_5G_CHECK_SHARDS";
const DEFAULT_CHECK_SHARDS = 4;
const MAX_CHECK_SHARDS = 8;
const INTERNAL_CHECK_SHARD_MODE = "--internal-check-shard";
const CHECK_RECEIPT_RELATIVE_PATH = "target/h2-5g/check-receipt.v1.json";
const CHECK_OUTCOME_RELATIVE_PATH = "target/h2-5g/check-outcome.v1.json";
const CHECK_RESUME_DIRECTORY = "target/h2-5g/check-resume";
const PIN_NORMALIZE_RELATIVE_PATH = "crates/oracle/pin-normalize.mjs";
const FRESH_ENV = "TSRS_H2_5G_FRESH";
const TYPESCRIPT_LIB_DIRECTORY = "vendor/typescript-6.0.3/lib";
const OPTION_LINE_PATTERN = /^\/{2}\s*@(\w+)\s*:\s*([^\r\n]*)/;
const LINK_LINE_PATTERN =
  /^\/{2}\s*@link\s*:\s*([^\r\n]*)\s*->\s*([^\r\n]*)/;
const UTF8_BOM = Buffer.from([0xef, 0xbb, 0xbf]);

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
const OWNER_KEYS = Object.freeze(["transform-es2016"]);
const SLICE_ORDER = Object.freeze([
  "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b",
  "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c", "H2.3d", "H2.4a",
  "H2.4b", "H2.5a", "H2.5b", "H2.5c", "H2.5d", "H2.5e", "H2.5f",
  "H2.5g", "H2.5h", "H2.6a", "H2.6b", "H2.6c", "H2.7a", "H2.7b",
  "H2.7c", "H2.7d", "H2.7e", "H2.8a", "H2.8b", "H2.8c", "H2.8d",
  "H2.8e", "H2.9",
]);
const SLICE_RANK = new Map(SLICE_ORDER.map((slice, index) => [slice, index]));
const CLOSED_SLICES = new Set([
  "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b",
  "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c", "H2.3d", "H2.4a",
  "H2.4b", "H2.5a", "H2.5b", "H2.5c", "H2.5d", "H2.5e", "H2.5f",
  "H2.5g",
]);
const EXPECTED_TARGET_STATES = Object.freeze([
  Object.freeze({ value: "ES2015(2)", cases: 9_027 }),
]);
const EXPECTED_MODULE_STATES = Object.freeze([
  Object.freeze({ value: "absent", cases: 7_730 }),
  Object.freeze({ value: "CommonJS(1)", cases: 872 }),
  Object.freeze({ value: "AMD(2)", cases: 163 }),
  Object.freeze({ value: "System(4)", cases: 78 }),
  Object.freeze({ value: "ESNext(99)", cases: 67 }),
  Object.freeze({ value: "ES2015(5)", cases: 44 }),
  Object.freeze({ value: "UMD(3)", cases: 32 }),
  Object.freeze({ value: "NodeNext(199)", cases: 15 }),
  Object.freeze({ value: "Preserve(200)", cases: 9 }),
  Object.freeze({ value: "ES2020(6)", cases: 7 }),
  Object.freeze({ value: "None(0)", cases: 5 }),
  Object.freeze({ value: "ES2022(7)", cases: 2 }),
  Object.freeze({ value: "Node16(100)", cases: 1 }),
  Object.freeze({ value: "Node18(101)", cases: 1 }),
  Object.freeze({ value: "Node20(102)", cases: 1 }),
]);

const HARNESS_ONLY_OPTIONS = new Set(
  [
    "useCaseSensitiveFileNames", "baselineFile", "fileName",
    "suppressOutputPathCheck", "noImplicitReferences", "currentDirectory",
    "symlink", "link", "noTypesAndSymbols", "fullEmitPaths",
    "reportDiagnostics", "captureSuggestions", "typeScriptVersion",
  ].map((name) => name.toLowerCase()),
);
const OPTION_INDEX = new Map(
  ts.optionDeclarations.map((option) => [option.name.toLowerCase(), option]),
);
let observedCases = 0;

// --check sharding: the freshness proof still observes every one of the
// 9,027 cases and byte-compares the complete rendered artifact; shards only
// change which OS process performs each observation. A child process
// (shardAssignment) observes the cases whose enumeration ordinal is
// congruent to its shard index and streams the records back; the parent
// (shardAdoption) replays every child record through the unchanged per-case
// guards and the unchanged whole-artifact byte comparison. --write is
// untouched and stays serial with per-case observation reuse. The
// gate-tax 3 receipt attempt runs before any shard child is spawned;
// shards only ever perform full observations.
let shardAssignment = null;
let shardAdoption = null;
let shardOrdinal = 0;

// --check receipt attempt (gate-tax 3): while set, buildSuite adopts
// stored records through the per-case guards and a CheckReceiptMiss
// abort happens strictly before any TypeScript observation.
let checkReceiptAttempt = false;

class CheckReceiptMiss extends Error {}

// gate-tax 5-B: the per-case resume journal for the one legitimate
// full re-observation. Active only in the observing check processes
// (single-mode --check after a receipt miss, and every shard child);
// journaled cases are adopted through the unchanged per-case guards and
// only the remainder is observed, so a kill loses at most one case.
let resumeJournal = null;

// gate-tax 5-C: TSRS_H2_5G_FRESH=1 bypasses the receipt attempt,
// clears the journals, and forces the full observation — a RECORDED
// approval (outcome record + stderr), never a silent exemption.
const FRESH = process.env[FRESH_ENV] === "1";

// Machine-readable --check outcome record (gate-tax 5-C): the walk
// driver's enforcement reads this, never the prose lines.
let outcomeState = null;
let lastKeyRefusals = [];

function writeCheckOutcome(update) {
  if (MODE !== "--check") return;
  outcomeState = {
    ...(outcomeState ?? {
      schema: 1,
      kind: "h2-5g-check-outcome",
      fresh: FRESH,
      check_shards: null,
      receipt_attempt: null,
      miss_term: null,
      refusals: [],
      full_observation_started: false,
      completed: false,
      observed_cases: null,
      journal_adopted: null,
      receipt_reused: null,
      minted: null,
    }),
    ...update,
  };
  const absolute = path.join(WORKSPACE, CHECK_OUTCOME_RELATIVE_PATH);
  fs.mkdirSync(path.dirname(absolute), { recursive: true });
  writeFileAtomic(
    absolute,
    render(withFingerprint({ ...outcomeState }, "outcome_fingerprint_sha256")),
  );
}

function observationTarget() {
  return shardAssignment === null ? 9_027 : shardAssignment.assigned;
}

function progressLabel() {
  return shardAssignment === null
    ? "H2.5g TypeScript observations"
    : `H2.5g TypeScript observations [shard ${shardAssignment.index + 1}/${shardAssignment.count}]`;
}

function reportObservationProgress(caseId) {
  observedCases += 1;
  const target = observationTarget();
  if (observedCases % PROGRESS_INTERVAL === 0 || observedCases === target) {
    process.stderr.write(
      `${progressLabel()}: ${observedCases}/${target} (${caseId})\n`,
    );
  }
}

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

function writeFileAtomic(absolutePath, contents) {
  // Same-directory temp + rename: a kill mid-write can never truncate
  // the artifact, which doubles as the reuse store (gate-tax 2 R4-1).
  const temporary = path.join(
    path.dirname(absolutePath),
    `.${path.basename(absolutePath)}.tmp`,
  );
  // The name is deterministic (no pid): artifact writes are
  // single-writer by walk discipline, and a stray temp left by a kill
  // is overwritten by the next successful write instead of
  // accumulating as untracked residue.
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

function validateRuntime() {
  const node = readBytes(NODE_VERSION_PATH).toString("utf8").trim();
  requireCondition(node === EXPECTED_NODE, "unexpected Node pin");
  requireCondition(process.version === `v${node}`, `requires Node ${node}`);
  requireCondition(ts.version === "6.0.3", "unexpected TypeScript runtime");
  requireCondition(
    typeof ts.emitFilesAndReportErrorsAndGetExitStatus === "function" &&
      typeof ts.sourceFileMayBeEmitted === "function",
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
    units.length > 0 || currentName ? currentName : path.posix.basename(fixturePath);
  units.push({
    name: currentName,
    text: currentContent || "",
    file_options: orderedSettings(currentOptions),
  });
  return { units, links };
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

function loadFixture(suite, expansion, fixture) {
  const source = expansion.sources[fixture.source];
  requireCondition(source !== undefined, `${suite} fixture source is absent`);
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
  const parsed = makeUnits(decoded, source.path);
  parsed.units.forEach((unit, index) => {
    unit.original_id = index;
  });
  const allUnits = [...parsed.units];
  let virtualConfig = null;
  let configIndex = null;
  if (fixture.virtual_config !== null) {
    const virtualConfigIndex = parsed.units.findIndex(
      (unit) => unit.name === fixture.virtual_config.name,
    );
    requireCondition(
      virtualConfigIndex >= 0,
      `${suite}/${source.path} virtual config is absent`,
    );
    const [parsedConfig] = parsed.units.splice(virtualConfigIndex, 1);
    configIndex = parsedConfig.original_id;
    requireCondition(
      parsedConfig?.name === fixture.virtual_config.name &&
        canonical(parsedConfig.file_options) ===
          canonical(fixture.virtual_config.file_options) &&
        canonical(contentIdentity(parsedConfig.text)) ===
          canonical(fixture.virtual_config.content) &&
        canonical(documentSymlinks(parsedConfig.file_options)) ===
          canonical(fixture.virtual_config.document_symlinks),
      `${suite}/${source.path} virtual config changed`,
    );
    virtualConfig = parsedConfig;
  }
  requireCondition(
    canonical(parsed.links) === canonical(fixture.links),
    `${suite}/${source.path} global links changed`,
  );
  requireCondition(
    parsed.units.length === fixture.normal_units.length,
    `${suite}/${source.path} unit count changed`,
  );
  parsed.units.forEach((unit, index) => {
    const expected = fixture.normal_units[index];
    requireCondition(
      unit.name === expected.name &&
        canonical(unit.file_options) === canonical(expected.file_options) &&
        canonical(contentIdentity(unit.text)) === canonical(expected.content) &&
        canonical(documentSymlinks(unit.file_options)) ===
          canonical(expected.document_symlinks),
      `${suite}/${source.path} unit ${index} changed`,
    );
  });
  return {
    source,
    units: parsed.units,
    allUnits,
    virtualConfig,
    configIndex,
    links: parsed.links,
  };
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

function configDiagnostic(diagnostic) {
  return {
    code: diagnostic.code,
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
  requireCondition(
    canonical(parsed.fileNames) === canonical(recordedPlan.parsed_file_names) &&
      canonical(parsed.errors.map(configDiagnostic)) ===
        canonical(recordedPlan.diagnostics) &&
      canonical(host.log) === canonical(recordedPlan.host_log),
    `${loaded.source.path} config plan changed`,
  );
  const normalIndexByOriginalId = new Map(
    loaded.units.map((unit, index) => [unit.original_id, index]),
  );
  const mapUnitIds = (ids, label) =>
    ids.map((id) => {
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

function mergedSettings(base, overrides) {
  const settings = new Map(base.map((setting) => [setting.name, setting.value]));
  for (const setting of overrides) settings.set(setting.name, setting.value);
  return settings;
}

function optionValue(option, raw) {
  const errors = [];
  let value;
  if (option.type === "boolean") value = raw.toLowerCase() === "true";
  else if (option.type === "string") value = raw;
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
  // The upstream runner materializes virtual files by name, so a repeated
  // @filename shadows the earlier unit. Canonicalize that last-write-wins VFS
  // before selecting roots; the Rust qualification loader intentionally
  // rejects duplicate paths.
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
    ) {
      record("runtime-namespaces", node);
    }
    if (ts.isImportEqualsDeclaration(node)) record("import-equals", node);
    if (ts.isExportAssignment(node) && node.isExportEquals) record("export-equals", node);
    if (
      ts.isJsxElement(node) ||
      ts.isJsxFragment(node) ||
      ts.isJsxSelfClosingElement(node)
    ) {
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
    ) {
      record("parameter-properties", node);
    }
    if (ts.canHaveDecorators(node) && (ts.getDecorators(node)?.length ?? 0) > 0) {
      record("decorators", node);
    }
    ts.forEachChild(node, (child) => {
      stack.push(child);
    });
  }
  return roots.sort(
    (left, right) =>
      left.start - right.start ||
      left.end - right.end ||
      FEATURE_ORDER.indexOf(left.feature) - FEATURE_ORDER.indexOf(right.feature),
  );
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
  if (lower.endsWith(".d.ts") || lower.endsWith(".d.mts") || lower.endsWith(".d.cts")) {
    return null;
  }
  if (lower.endsWith(".ts")) return null;
  if (lower.endsWith(".mts") || lower.endsWith(".cts")) return "H2.1e";
  if (lower.endsWith(".js") || lower.endsWith(".mjs") || lower.endsWith(".cjs")) {
    return "H2.3a";
  }
  if (lower.endsWith(".tsx") || lower.endsWith(".jsx")) return "H2.3b";
  if (lower.endsWith(".json")) return "H2.3d";
  return "H2.9";
}

function createProgramCase(loaded, selection, settings, options) {
  const cwd = currentDirectory(settings);
  requireCondition(
    loaded.links.length === 0,
    `${loaded.source.path} requires global-link topology support`,
  );
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
  if (lower.endsWith(".d.ts") || lower.endsWith(".d.mts") || lower.endsWith(".d.cts")) {
    return "declaration";
  }
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

function observeTypeScript(makeProgram) {
  const { program } = makeProgram();
  const writes = [];
  const reported = [];
  const statusWrites = [];
  let emitResult;
  const originalEmit = program.emit;
  program.emit = function capture(...arguments_) {
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
  return withFingerprint(
    {
      writes: writes.map(serializeWrite),
      reported_diagnostics: reported.map(serializeDiagnostic),
      emit_result: {
        emit_skipped: emitResult.emitSkipped,
        diagnostics: emitResult.diagnostics.map(serializeDiagnostic),
        emitted_files: emitResult.emittedFiles ?? null,
        source_maps: emitResult.sourceMaps ?? null,
      },
      status_writes: statusWrites,
      exit_code: exit,
    },
    "run_fingerprint_sha256",
  );
}

function maximumAstDepth(root) {
  let maximum = 0;
  const stack = [[root, 1]];
  while (stack.length !== 0) {
    const [node, depth] = stack.pop();
    maximum = Math.max(maximum, depth);
    ts.forEachChild(node, (child) => {
      stack.push([child, depth + 1]);
    });
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
    ) {
      return true;
    }
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

// --write observation reuse: pin-only envelope changes must not re-run
// 18,054 TypeScript observations. A stored case record is adopted verbatim
// only when every observation-driving input is byte-identical: the global
// records (vendored TypeScript bundle/implementation, the eight vendor
// inputs, the execution contract, the owner closure) and the per-case
// identity (fixture bytes, merged settings, selection, roots, per-unit
// text hashes). Anything else falls back to a fresh observation for that
// case.
//
// --check consults the gate-tax 3 receipt instead of always paying the
// full re-observation: a machine-local, self-fingerprinted record under
// target/ that only a green full-re-observation --check mints. On a hit
// the stored records are adopted through the same per-case guards and
// the unchanged assembly plus whole-artifact byte comparison still run;
// only the observeTypeScript runs are skipped. On ANY miss — receipt
// absent/invalid, workspace path, node version, platform/arch, the
// normalizer or pin-index terms, the NORMALIZED generator bytes
// (gate-tax 5-A: pin-index-enumerated pin literals are masked, so walk
// repins cannot break the key; raw generator_sha256 is informational
// provenance only), the vendored lib inventory, the global observation
// records, the observation-content roll, or one stale case — the
// attempt aborts before any observation and the full re-observation
// runs unchanged, minting a fresh schema-2 receipt on success. The
// gate-tax 2 keystone survives, amended by gate-tax-3.md §3 and
// gate-tax-5.md §B: observation content enters the trusted state only
// through local re-observation under a SINGLE receipt key, resumable
// across process restarts — the union of partial local runs under an
// identical key (the per-case resume journal) is one full
// re-observation.
let reusedObservations = 0;

// `owner_inventory` and `global_candidate_dispositions` are pin-carrying
// ratchet artifacts: an upstream pin-only rebind changes their file bytes
// without changing anything an observation can see. Their
// observation-relevant projections are compared exactly instead — the
// owner closure rows canonically, and the selection they drive through
// the per-case identity (case-id membership, the 9,027-count guards, and
// every per-case fixture/settings/selection hash). The vendored
// TypeScript records and the vendor expansion inputs stay byte-compared:
// they are the oracle itself and are never pin-rebound.
function observationInputs(inputs) {
  const {
    owner_inventory: _ownerInventory,
    global_candidate_dispositions: _globalDispositions,
    ...rest
  } = inputs;
  return rest;
}

function libraryInventoryRecord() {
  // The observations resolve default libraries from disk through the
  // real compiler host; those .d.ts bytes drive the type check but are
  // not covered by the bundle/implementation hashes (gate-tax 2 R3-2,
  // applied to the 5g receipt key by gate-tax 3).
  const directory = path.join(WORKSPACE, TYPESCRIPT_LIB_DIRECTORY);
  const names = fs
    .readdirSync(directory)
    .filter((name) => name.startsWith("lib.") && name.endsWith(".d.ts"))
    .sort();
  requireCondition(
    names.length > 0,
    "vendored TypeScript lib inventory is empty",
  );
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

function checkReceiptGlobalSha(
  typescriptRecord,
  inputsRecord,
  executionContract,
  ownerRows,
) {
  return sha256(
    Buffer.from(
      canonical({
        typescript: typescriptRecord,
        observation_inputs: observationInputs(inputsRecord),
        execution_contract: executionContract,
        owner_closure: ownerRows,
        library_inventory: libraryInventoryRecord(),
      }),
      "utf8",
    ),
  );
}

function casesObservationSha(caseFingerprints) {
  const hash = crypto.createHash("sha256");
  for (const fingerprint of [...caseFingerprints].sort()) {
    hash.update(fingerprint);
    hash.update("\u0000");
  }
  return hash.digest("hex");
}

// gate-tax 5-A: the schema-2 receipt key. The generator term is the
// NORMALIZED script hash — bytes with every pin-index-enumerated pin
// literal masked — so walk repins cannot break the key; a grammar hit
// the index does not enumerate is a refusal, never a silent mask. The
// normalizer bytes and the consumer's classification rows are their own
// key terms (a masking-rule change must re-key).
function computeCheckReceiptKey(
  typescriptRecord,
  inputsRecord,
  executionContract,
  ownerRows,
) {
  const generatorText = readBytes(GENERATOR_RELATIVE_PATH).toString("utf8");
  const rows = consumerRows(
    readJson(PIN_INDEX_RELATIVE_PATH),
    GENERATOR_RELATIVE_PATH,
  );
  const { sha256: normalizedGenerator, refusals } = normalizedSha256(
    generatorText,
    rows,
  );
  lastKeyRefusals = refusals;
  if (refusals.length !== 0) return { key: null, refusals };
  return {
    refusals: [],
    key: receiptKey({
      workspace: fs.realpathSync(WORKSPACE),
      node: process.version,
      platform: process.platform,
      arch: process.arch,
      normalizerSha256: pathHash(PIN_NORMALIZE_RELATIVE_PATH).sha256,
      pinIndexRowsSha256: pinIndexRowsSha256(rows),
      normalizedGeneratorSha256: normalizedGenerator,
      globalRecordsSha256: checkReceiptGlobalSha(
        typescriptRecord,
        inputsRecord,
        executionContract,
        ownerRows,
      ),
    }),
  };
}

// Validates every receipt key term except the observation-content roll,
// which reusableStoredCases owns (it holds the stored-artifact parse).
// Throws CheckReceiptMiss naming the first divergent term.
function loadCheckReceipt(keyComputation) {
  if (keyComputation.refusals.length !== 0) {
    throw new CheckReceiptMiss("normalizer-refusal");
  }
  let bytes;
  try {
    bytes = fs.readFileSync(
      path.join(WORKSPACE, CHECK_RECEIPT_RELATIVE_PATH),
      "utf8",
    );
  } catch {
    throw new CheckReceiptMiss("absent");
  }
  let receipt;
  try {
    receipt = JSON.parse(bytes);
  } catch {
    throw new CheckReceiptMiss("invalid");
  }
  const term = receiptMissTerm(receipt, keyComputation.key);
  if (term !== null) throw new CheckReceiptMiss(term);
  return receipt;
}

// Mints the schema-2 receipt, recomputing every key term fresh from the
// current disk state (external-review #5). On a normalizer refusal the
// mint is SKIPPED loudly — the full re-observation stays green, but no
// receipt exists until the pin sites are classified, and the journal is
// kept so the next check adopts instead of re-observing.
function mintCheckReceipt(artifact) {
  const computation = computeCheckReceiptKey(
    artifact.typescript,
    artifact.inputs,
    artifact.execution_contract,
    artifact.owner_closure,
  );
  if (computation.refusals.length !== 0) {
    process.stderr.write(
      `H2.5g check receipt: minting REFUSED — unclassified pin sites ${JSON.stringify(
        computation.refusals,
      )}; classify them in ${PIN_INDEX_RELATIVE_PATH} (scripts/pin-index.py)\n`,
    );
    return { minted: false, key: null };
  }
  const absolute = path.join(WORKSPACE, CHECK_RECEIPT_RELATIVE_PATH);
  fs.mkdirSync(path.dirname(absolute), { recursive: true });
  writeFileAtomic(
    absolute,
    render(
      renderReceipt(
        computation.key,
        artifact.generator.sha256,
        casesObservationSha(
          artifact.cases.map((entry) => entry.case_fingerprint_sha256),
        ),
      ),
    ),
  );
  return { minted: true, key: computation.key };
}

function reusableStoredCases(
  keyComputation,
  typescriptRecord,
  inputsRecord,
  executionContract,
  ownerRows,
) {
  if (MODE !== "--write" && !checkReceiptAttempt) return null;
  const receipt = checkReceiptAttempt ? loadCheckReceipt(keyComputation) : null;
  const targetPath = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
  if (!fs.existsSync(targetPath)) {
    if (checkReceiptAttempt) throw new CheckReceiptMiss("stored-artifact");
    return null;
  }
  let stored;
  try {
    stored = JSON.parse(fs.readFileSync(targetPath, "utf8"));
  } catch {
    if (checkReceiptAttempt) throw new CheckReceiptMiss("stored-artifact");
    return null;
  }
  if (
    stored.schema !== 1 ||
    stored.status !== "qualified-typescript-oracle" ||
    stored.phase !== "H2.5g-es2016-target" ||
    !hasValidFingerprint(stored, "qualification_fingerprint_sha256") ||
    canonical(stored.typescript) !== canonical(typescriptRecord) ||
    canonical(observationInputs(stored.inputs)) !==
      canonical(observationInputs(inputsRecord)) ||
    canonical(stored.execution_contract) !== canonical(executionContract) ||
    canonical(stored.owner_closure) !== canonical(ownerRows)
  ) {
    if (checkReceiptAttempt) throw new CheckReceiptMiss("global-records");
    return null;
  }
  if (receipt !== null) {
    const fingerprints = Array.isArray(stored.cases)
      ? stored.cases.map((entry) => entry?.case_fingerprint_sha256)
      : null;
    if (
      fingerprints === null ||
      fingerprints.some((value) => typeof value !== "string") ||
      receipt.cases_observation_sha256 !== casesObservationSha(fingerprints)
    ) {
      throw new CheckReceiptMiss("observation-content");
    }
  }
  return new Map(stored.cases.map((entry) => [entry.case_id, entry]));
}

function storedCaseReusable(stored, suite, row, loaded, settings, selection) {
  if (
    stored.suite !== suite ||
    stored.selection_origin !== row.selection_origin ||
    stored.expansion_case !== row.expansion_case ||
    stored.source.sha256 !== loaded.source.sha256 ||
    !hasValidFingerprint(stored, "case_fingerprint_sha256")
  ) {
    return false;
  }
  const cwd = currentDirectory(settings);
  const settingsRecord = [...settings].map(([name, value]) => ({ name, value }));
  const roots = selection.program_root_unit_ids.map((id) =>
    ts.getNormalizedAbsolutePath(loaded.units[id].name, cwd),
  );
  const virtualConfig =
    loaded.virtualConfig === null
      ? null
      : {
          path: ts.getNormalizedAbsolutePath(loaded.virtualConfig.name, cwd),
          utf8_sha256: sha256(Buffer.from(loaded.virtualConfig.text, "utf8")),
        };
  // Mirrors createProgramCase exactly: iteration in vfs_write_order,
  // last target wins per link path.
  const symlinkByPath = new Map();
  for (const id of selection.vfs_write_order) {
    const unit = loaded.units[id];
    const target = ts.getNormalizedAbsolutePath(unit.name, cwd);
    for (const rawLink of documentSymlinks(unit.file_options)) {
      symlinkByPath.set(ts.getNormalizedAbsolutePath(rawLink, cwd), target);
    }
  }
  const vfsSymlinks = [...symlinkByPath].map(([link_path, target_path]) => ({
    link_path,
    target_path,
  }));
  if (
    stored.input.current_directory !== cwd ||
    canonical(stored.input.settings) !== canonical(settingsRecord) ||
    canonical(stored.input.roots) !== canonical(roots) ||
    canonical(stored.input.vfs_symlinks) !== canonical(vfsSymlinks) ||
    (stored.input.virtual_config === null) !== (virtualConfig === null) ||
    (virtualConfig !== null &&
      (stored.input.virtual_config.path !== virtualConfig.path ||
        stored.input.virtual_config.utf8_sha256 !== virtualConfig.utf8_sha256)) ||
    stored.input.files.length !== selection.vfs_write_order.length
  ) {
    return false;
  }
  return selection.vfs_write_order.every((id, index) => {
    const unit = loaded.units[id];
    const record = stored.input.files[index];
    return (
      record.unit === id &&
      record.path === ts.getNormalizedAbsolutePath(unit.name, cwd) &&
      record.utf8_sha256 === sha256(Buffer.from(unit.text, "utf8"))
    );
  });
}

function analyzeCase(
  loaded,
  selection,
  settings,
  options,
  makeProgram,
  ownerReachability,
) {
  const { program, roots, cwd, unitByPath, vfsSymlinks } = makeProgram();
  const files = [];
  const requiredSlices = new Set();
  for (const sourceFile of program.getSourceFiles()) {
    const fixture = unitByPath.get(ts.normalizePath(sourceFile.fileName));
    if (!fixture) continue;
    const emitEligible = ts.sourceFileMayBeEmitted(sourceFile, program, false);
    const roots = featureRoots(sourceFile);
    const emitFormat = program.getEmitModuleFormatOfFile(sourceFile);
    const parseDiagnosticCodes = [
      ...new Set(sourceFile.parseDiagnostics.map((diagnostic) => diagnostic.code)),
    ].sort((left, right) => left - right);
    const maxAstDepth = maximumAstDepth(sourceFile);
    const importAttributes = hasImportAttributes(sourceFile);
    const advancedCommentPlacement =
      /\.\.\.[\t \r\n]*\/(?:\*|\/)/.test(sourceFile.text) ||
      /#[A-Za-z_$][\w$]*[\t ]*\/\*.*?\*\/(?:[\t \r\n]|\/\*.*?\*\/)*\bin\b/s.test(
        sourceFile.text,
      ) ||
      hasCommentedOptionalChainTypeAssertion(sourceFile.text);
    if (emitEligible) {
      const slice = outputSlice(sourceFile.fileName);
      if (slice) requiredSlices.add(slice);
      for (const root of roots) requiredSlices.add(FEATURE_SLICES[root.feature]);
      if (parseDiagnosticCodes.length !== 0) requiredSlices.add("H2.9");
      if (maxAstDepth > MAX_TRANSFORM_DEPTH) requiredSlices.add("H2.9");
      if (importAttributes) requiredSlices.add("H2.1e");
      if (advancedCommentPlacement) requiredSlices.add("H2.8a");
    }
    files.push({
      unit: fixture.id,
      path: ts.normalizePath(sourceFile.fileName),
      script_kind: scriptKindName(sourceFile.fileName),
      declaration_file: sourceFile.isDeclarationFile,
      emit_eligible: emitEligible,
      implied_module_format: impliedFormatName(sourceFile.impliedNodeFormat),
      emit_module_format: emitFormat,
      feature_roots: roots,
      parse_diagnostic_codes: parseDiagnosticCodes,
      max_ast_depth: maxAstDepth,
      import_attributes: importAttributes,
      advanced_comment_placement: advancedCommentPlacement,
      text_sha256: sha256(Buffer.from(fixture.unit.text, "utf8")),
    });
  }
  requireCondition(
    selection.program_root_unit_ids.every((id) => files.some((file) => file.unit === id)),
    `${loaded.source.path} did not reach every root`,
  );
  const orderedSlices = [...requiredSlices].sort(
    (left, right) => SLICE_RANK.get(left) - SLICE_RANK.get(right),
  );
  const remainingSlices = orderedSlices.filter((slice) => !CLOSED_SLICES.has(slice));
  const input = {
    current_directory: cwd,
    roots,
    vfs_symlinks: vfsSymlinks,
    settings: [...settings].map(([name, value]) => ({ name, value })),
    virtual_config:
      loaded.virtualConfig === null
        ? null
        : (() => {
            const bytes = Buffer.from(loaded.virtualConfig.text, "utf8");
            return {
              path: ts.getNormalizedAbsolutePath(loaded.virtualConfig.name, cwd),
              utf8_base64: bytes.toString("base64"),
              utf8_sha256: sha256(bytes),
              utf8_bytes: bytes.length,
            };
          })(),
    files: selection.vfs_write_order.map((id) => {
      const unit = loaded.units[id];
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
  const first = observeTypeScript(makeProgram);
  const second = observeTypeScript(makeProgram);
  requireCondition(
    first.run_fingerprint_sha256 === second.run_fingerprint_sha256,
    `${loaded.source.path} TypeScript observation is not deterministic`,
  );
  const disposition =
    remainingSlices.length === 0 ? "admitted-for-execution" : "deferred-to-slices";
  return {
    input,
    files,
    owner_reachability: ownerReachability,
    disposition,
    required_slices: remainingSlices,
    diagnostic_disposition:
      remainingSlices.length === 0
        ? { state: "exact-required" }
        : { state: "not-observed-source-deferred" },
    typescript_observation: first,
    typescript_run_fingerprints: [
      first.run_fingerprint_sha256,
      second.run_fingerprint_sha256,
    ],
  };
}

function moduleCandidates(classification, selectionOrigins) {
  return classification.cases
    .filter((entry) => selectionOrigins.has(entry.id))
    .map((entry) => ({
      ...entry,
      selection_origin: selectionOrigins.get(entry.id),
    }));
}

function moduleStateName(moduleKind) {
  if (moduleKind === undefined) return "absent";
  if (moduleKind === ts.ModuleKind.None) return "None(0)";
  if (moduleKind === ts.ModuleKind.CommonJS) return "CommonJS(1)";
  if (moduleKind === ts.ModuleKind.AMD) return "AMD(2)";
  if (moduleKind === ts.ModuleKind.UMD) return "UMD(3)";
  if (moduleKind === ts.ModuleKind.System) return "System(4)";
  if (moduleKind === ts.ModuleKind.ES2015) return "ES2015(5)";
  if (moduleKind === ts.ModuleKind.ES2020) return "ES2020(6)";
  if (moduleKind === ts.ModuleKind.ES2022) return "ES2022(7)";
  if (moduleKind === ts.ModuleKind.ESNext) return "ESNext(99)";
  if (moduleKind === ts.ModuleKind.Node16) return "Node16(100)";
  if (moduleKind === ts.ModuleKind.Node18) return "Node18(101)";
  if (moduleKind === ts.ModuleKind.Node20) return "Node20(102)";
  if (moduleKind === ts.ModuleKind.NodeNext) return "NodeNext(199)";
  if (moduleKind === ts.ModuleKind.Preserve) return "Preserve(200)";
  fail(`unexpected H2.5g module kind ${moduleKind}`);
}

function targetStateName(target) {
  if (target === ts.ScriptTarget.ES2015) return "ES2015(2)";
  fail(`unexpected H2.5g target ${target}`);
}

function fixtureFor(suite, expansion, row) {
  const expansionCase = expansion.cases[row.expansion_case];
  requireCondition(expansionCase.id === row.id, `${row.id} expansion identity changed`);
  if (suite === "compiler") {
    requireCondition(
      expansionCase.configuration.kind === "compiler" &&
        expansionCase.configuration.configuration === row.configuration,
      `${row.id} compiler configuration changed`,
    );
    return expansion.compiler_fixtures[expansionCase.source];
  }
  requireCondition(
    expansionCase.configuration === row.configuration,
    `${row.id} conformance configuration changed`,
  );
  const fixture = expansion.fixtures.find(
    (candidate) => candidate.source === expansionCase.source,
  );
  requireCondition(fixture !== undefined, `${row.id} conformance fixture is absent`);
  return fixture;
}

function preflightSuiteFixtures(
  suite,
  classification,
  expansion,
  selectionOrigins,
  configPlanBySource,
) {
  const loadedBySource = new Map();
  for (const row of moduleCandidates(classification, selectionOrigins)) {
    const fixture = fixtureFor(suite, expansion, row);
    if (!loadedBySource.has(fixture.source)) {
      const loaded = loadFixture(suite, expansion, fixture);
      if (suite === "compiler" && loaded.virtualConfig !== null) {
        const recordedPlan = configPlanBySource.get(fixture.source);
        requireCondition(
          recordedPlan?.configuration_count === fixture.configurations.length,
          `${loaded.source.path} compiler config plan is absent`,
        );
        loaded.configContext = parseConfigContext(loaded, recordedPlan);
      } else {
        loaded.configContext = null;
      }
      loadedBySource.set(fixture.source, loaded);
    }
  }
  return loadedBySource;
}

function buildSuite(
  suite,
  classification,
  expansion,
  selectionOrigins,
  loadedBySource,
  reuseByCaseId,
) {
  const rows = moduleCandidates(classification, selectionOrigins);
  const records = rows.map((row) => {
    const fixture = fixtureFor(suite, expansion, row);
    const loaded = loadedBySource.get(fixture.source);
    requireCondition(loaded !== undefined, `${row.id} missed fixture preflight`);
    const configuration = fixture.configurations[row.configuration];
    requireCondition(configuration !== undefined, `${row.id} configuration is absent`);
    const settings = mergedSettings(fixture.settings, configuration.settings);
    const options = effectiveCompilerOptions(
      settings,
      loaded.configContext?.options ?? { noResolve: false },
    );
    requireCondition(
      options.target === ts.ScriptTarget.ES2015 &&
        [
          undefined,
          ts.ModuleKind.None,
          ts.ModuleKind.CommonJS,
          ts.ModuleKind.AMD,
          ts.ModuleKind.UMD,
          ts.ModuleKind.System,
          ts.ModuleKind.ES2015,
          ts.ModuleKind.ES2020,
          ts.ModuleKind.ES2022,
          ts.ModuleKind.ESNext,
          ts.ModuleKind.Node16,
          ts.ModuleKind.Node18,
          ts.ModuleKind.Node20,
          ts.ModuleKind.NodeNext,
          ts.ModuleKind.Preserve,
        ].includes(options.module),
      `${row.id} is no longer an H2.5g option candidate`,
    );
    const selection =
      loaded.configContext?.selection ?? explicitRootSelection(loaded, settings, options);
    const stored = reuseByCaseId?.get(row.id);
    if (
      stored !== undefined &&
      storedCaseReusable(stored, suite, row, loaded, settings, selection)
    ) {
      reusedObservations += 1;
      reportObservationProgress(row.id);
      return stored;
    }
    if (checkReceiptAttempt) {
      // gate-tax 3: the receipt path never observes; one stale case
      // falls the whole check back to the full re-observation.
      throw new CheckReceiptMiss(`case ${row.id}`);
    }
    if (shardAdoption !== null) {
      const adopted = shardAdoption.get(row.id);
      requireCondition(
        adopted !== undefined,
        `${row.id} is missing from the shard observations`,
      );
      reportObservationProgress(row.id);
      return adopted;
    }
    if (shardAssignment !== null) {
      const ordinal = shardOrdinal;
      shardOrdinal += 1;
      if (ordinal % shardAssignment.count !== shardAssignment.index) {
        return null;
      }
    }
    if (resumeJournal !== null) {
      // gate-tax 5-B: a journaled case re-enters through the SAME
      // per-case guards as any stored record; only the remainder is
      // observed. Guard failure = observe fresh (never an error).
      const journaled = resumeJournal.map.get(row.id);
      if (
        journaled !== undefined &&
        storedCaseReusable(journaled, suite, row, loaded, settings, selection)
      ) {
        resumeJournal.adopted += 1;
        reportObservationProgress(row.id);
        return journaled;
      }
    }
    const makeProgram = () => createProgramCase(loaded, selection, settings, options);
    const ownerReachability = OWNER_KEYS;
    const analysis = analyzeCase(
      loaded,
      selection,
      settings,
      options,
      makeProgram,
      ownerReachability,
    );
    reportObservationProgress(row.id);
    const record = withFingerprint(
      {
        suite,
        case_id: row.id,
        selection_origin: row.selection_origin,
        execution_route:
          suite === "compiler" ? "recorded-compiler-plan" : "qualified-vfs",
        expansion_case: row.expansion_case,
        source: {
          path: loaded.source.path,
          bytes: loaded.source.bytes,
          sha256: loaded.source.sha256,
          git_blob_sha1: loaded.source.git_blob_sha1,
        },
        target_state: targetStateName(options.target),
        module_state: moduleStateName(options.module),
        ...analysis,
        rust_expectation:
          analysis.disposition === "admitted-for-execution"
            ? "two-deterministic-exact-runs"
            : "typed-failure-before-first-sink-write",
      },
      "case_fingerprint_sha256",
    );
    if (resumeJournal !== null) {
      resumeJournal.writer.append(row.id, record);
      resumeJournal.appended += 1;
    }
    return record;
  });
  return records.filter((record) => record !== null);
}

function suiteOptionStates(
  suite,
  classification,
  expansion,
  selectionOrigins,
  loadedBySource,
) {
  return moduleCandidates(classification, selectionOrigins).map((row) => {
    const fixture = fixtureFor(suite, expansion, row);
    const loaded = loadedBySource.get(fixture.source);
    requireCondition(loaded !== undefined, `${row.id} missed option preflight`);
    const configuration = fixture.configurations[row.configuration];
    requireCondition(configuration !== undefined, `${row.id} configuration is absent`);
    const settings = mergedSettings(fixture.settings, configuration.settings);
    const options = effectiveCompilerOptions(
      settings,
      loaded.configContext?.options ?? { noResolve: false },
    );
    return {
      target: targetStateName(options.target),
      module: moduleStateName(options.module),
    };
  });
}

function countBy(values) {
  const counts = new Map();
  for (const value of values) counts.set(value, (counts.get(value) ?? 0) + 1);
  return [...counts]
    .map(([value, cases]) => ({ value, cases }))
    .sort((left, right) => right.cases - left.cases || left.value.localeCompare(right.value));
}

function admissionContract() {
  return `all 9,027 selected global rows have a required-slice set closed through H2.5g at the option/owner inventory layer; a selected case is admitted for Rust execution only when every emit-eligible reached source targets ES2015, has no parse diagnostics, has AST depth <= ${MAX_TRANSFORM_DEPTH}, and requires no later source/output owner; transformES2016 runs after the already-closed transformESNext/class-field/ES2021/ES2020/ES2019/ES2018/ES2017 pipeline and before the module transformer, and diagnostics and writes are exact; deferred cases retain their first later owner and fail before the first Rust sink write`;
}

function buildArtifact() {
  const compilerClassification = readJson(COMPILER_CLASSIFICATION);
  const conformanceClassification = readJson(CONFORMANCE_CLASSIFICATION);
  const compilerExpansion = readJson(COMPILER_EXPANSION);
  const compilerConfigPlans = readJson(COMPILER_CONFIG_PLANS);
  const conformanceExpansion = readJson(CONFORMANCE_EXPANSION);
  const owner = readJson(OWNER_RELATIVE_PATH);
  const h2_5f_profile = pathHash(H2_5F_PROFILE_RELATIVE_PATH);
  requireCondition(
    h2_5f_profile.sha256 === H2_5F_PROFILE_SHA256,
    "reviewed H2.5f profile changed",
  );
  const globalDispositions = readJson(GLOBAL_DISPOSITIONS_RELATIVE_PATH);
  const closedBeforeH2_5g = new Set(
    [...CLOSED_SLICES].filter((slice) => slice !== "H2.5g"),
  );
  const globalRows = globalDispositions.cases.filter((entry) =>
    entry.required_slices.includes("H2.5g"),
  );
  const candidateRows = globalRows.filter((entry) =>
    entry.required_slices.every(
      (slice) => slice === "H2.5g" || closedBeforeH2_5g.has(slice),
    ),
  );
  const selectionOrigins = new Map(
    candidateRows.map((entry) => [entry.id, "global-h2-5g-candidate"]),
  );
  requireCondition(globalRows.length === 11_910, "unexpected global H2.5g row count");
  requireCondition(
    candidateRows.length === 9_027,
    `unexpected global H2.5g candidate denominator ${candidateRows.length}`,
  );
  requireCondition(
    selectionOrigins.size === 9_027,
    `unexpected H2.5g candidate denominator ${selectionOrigins.size}`,
  );
  const ownerRows = OWNER_KEYS.map((key) => {
    const row = owner.owners.find((entry) => entry.key === key);
    requireCondition(row?.owner_slice === "H2.5g", `missing transform owner ${key}`);
    return {
      key,
      declaration: row.declaration,
      disposition_before_h2_5g: row.disposition,
    };
  });
  const typescriptRecord = {
    version: ts.version,
    source_commit: SOURCE_COMMIT,
    bundle: pathHash(TYPESCRIPT_BUNDLE),
    implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
  };
  const inputsRecord = {
    compiler_classification: pathHash(COMPILER_CLASSIFICATION),
    conformance_classification: pathHash(CONFORMANCE_CLASSIFICATION),
    compiler_expansion: pathHash(COMPILER_EXPANSION),
    compiler_config_plans: pathHash(COMPILER_CONFIG_PLANS),
    conformance_expansion: pathHash(CONFORMANCE_EXPANSION),
    vfs_directory_overlay: pathHash(VFS_DIRECTORY_OVERLAY),
    owner_inventory: pathHash(OWNER_RELATIVE_PATH),
    global_candidate_dispositions: pathHash(GLOBAL_DISPOSITIONS_RELATIVE_PATH),
  };
  const executionContract = {
    source_reachability: "fixture VFS roots plus module-resolved fixture dependencies in a vendored TypeScript Program",
    module_selection: "Program.getEmitModuleFormatOfFile for every reached fixture SourceFile",
    admission: admissionContract(),
    typescript_repetitions: 2,
    rust_repetitions: 2,
    normalization: "none",
    deferred_boundary: "typed failure before first sink write",
  };
  const keyComputation =
    checkReceiptAttempt ||
    (MODE === "--check" && shardAdoption === null && shardAssignment === null) ||
    MODE === INTERNAL_CHECK_SHARD_MODE
      ? computeCheckReceiptKey(
          typescriptRecord,
          inputsRecord,
          executionContract,
          ownerRows,
        )
      : null;
  const reuseByCaseId = reusableStoredCases(
    keyComputation,
    typescriptRecord,
    inputsRecord,
    executionContract,
    ownerRows,
  );
  // gate-tax 5-B: the observing check processes journal each completed
  // case and adopt journaled cases through the unchanged per-case
  // guards. The sharded parent never observes (children own the
  // journal); the receipt attempt never observes (it aborts first).
  const journalEligible =
    (MODE === "--check" &&
      !checkReceiptAttempt &&
      shardAdoption === null &&
      shardAssignment === null) ||
    MODE === INTERNAL_CHECK_SHARD_MODE;
  if (journalEligible && keyComputation.refusals.length === 0) {
    const base = path.join(WORKSPACE, CHECK_RESUME_DIRECTORY);
    collectStaleJournals(base, keyComputation.key);
    const read = readJournal(base, keyComputation.key);
    resumeJournal = {
      key: keyComputation.key,
      map: read.cases,
      writer: openJournalWriter(
        base,
        keyComputation.key,
        shardAssignment === null
          ? "single.jsonl"
          : `shard-${shardAssignment.index}-of-${shardAssignment.count}.jsonl`,
      ),
      adopted: 0,
      appended: 0,
    };
    if (read.cases.size > 0) {
      process.stderr.write(
        `H2.5g check resume: ${read.cases.size} journaled cases available under key ${keyComputation.key.key_sha256.slice(0, 16)}${read.droppedTails > 0 ? ` (${read.droppedTails} torn tails dropped)` : ""}\n`,
      );
    }
  }
  const configPlanBySource = new Map(
    compilerConfigPlans.fixtures.map((entry) => [entry.source.index, entry]),
  );
  const compilerLoadedBySource = preflightSuiteFixtures(
    "compiler",
    compilerClassification,
    compilerExpansion,
    selectionOrigins,
    configPlanBySource,
  );
  const conformanceLoadedBySource = preflightSuiteFixtures(
    "conformance",
    conformanceClassification,
    conformanceExpansion,
    selectionOrigins,
    new Map(),
  );
  const optionStates = [
    ...suiteOptionStates(
      "compiler",
      compilerClassification,
      compilerExpansion,
      selectionOrigins,
      compilerLoadedBySource,
    ),
    ...suiteOptionStates(
      "conformance",
      conformanceClassification,
      conformanceExpansion,
      selectionOrigins,
      conformanceLoadedBySource,
    ),
  ];
  requireCondition(
    canonical(countBy(optionStates.map((entry) => entry.target))) ===
      canonical(EXPECTED_TARGET_STATES) &&
      canonical(countBy(optionStates.map((entry) => entry.module))) ===
        canonical(EXPECTED_MODULE_STATES),
    "H2.5g effective option distribution changed",
  );
  if (MODE === "--preflight") return null;
  const cases = [
    ...buildSuite(
      "compiler",
      compilerClassification,
      compilerExpansion,
      selectionOrigins,
      compilerLoadedBySource,
      reuseByCaseId,
    ),
    ...buildSuite(
      "conformance",
      conformanceClassification,
      conformanceExpansion,
      selectionOrigins,
      conformanceLoadedBySource,
      reuseByCaseId,
    ),
  ].sort((left, right) => left.suite.localeCompare(right.suite) || left.case_id.localeCompare(right.case_id));
  if (shardAssignment !== null) {
    requireCondition(
      cases.length === shardAssignment.assigned && shardOrdinal === 9_027,
      `shard ${shardAssignment.index} observed ${cases.length}/${shardAssignment.assigned} of ${shardOrdinal} enumerated cases`,
    );
    return { shard_cases: cases };
  }
  requireCondition(cases.length === 9_027, `unexpected H2.5g denominator ${cases.length}`);
  requireCondition(new Set(cases.map((entry) => entry.case_id)).size === cases.length, "duplicate H2.5g case");
  const admitted = cases.filter((entry) => entry.disposition === "admitted-for-execution");
  const outputControls = cases.filter(
    (entry) => entry.disposition === "diagnostic-deferred-output-control",
  );
  const sourceDeferred = cases.filter((entry) => entry.disposition === "deferred-to-slices");
  const deferred = [...outputControls, ...sourceDeferred];
  const summary = {
    candidates: cases.length,
    compiler_candidates: cases.filter((entry) => entry.suite === "compiler").length,
    conformance_candidates: cases.filter((entry) => entry.suite === "conformance").length,
    recorded_compiler_plan_cases: cases.filter(
      (entry) => entry.execution_route === "recorded-compiler-plan",
    ).length,
    qualified_vfs_cases: cases.filter(
      (entry) => entry.execution_route === "qualified-vfs",
    ).length,
    virtual_config_cases: cases.filter(
      (entry) => entry.input.virtual_config !== null,
    ).length,
    vfs_symlink_cases: cases.filter(
      (entry) => entry.input.vfs_symlinks.length > 0,
    ).length,
    vfs_symlink_paths: cases.reduce(
      (sum, entry) => sum + entry.input.vfs_symlinks.length,
      0,
    ),
    admitted_cases: admitted.length,
    deferred_cases: deferred.length,
    diagnostic_deferred_output_control_cases: outputControls.length,
    source_deferred_cases: sourceDeferred.length,
    no_emit_control_cases: admitted.filter(
      (entry) => !entry.files.some((file) => file.emit_eligible),
    ).length,
    target_states: countBy(cases.map((entry) => entry.target_state)),
    module_states: countBy(cases.map((entry) => entry.module_state)),
    dispositions: countBy(cases.map((entry) => entry.disposition)),
    first_deferred_slices: countBy(deferred.map((entry) => entry.required_slices[0])),
    typescript_runs: cases.reduce(
      (sum, entry) => sum + entry.typescript_run_fingerprints.length,
      0,
    ),
    deterministic_typescript_cases: cases.filter(
      (entry) =>
        entry.typescript_run_fingerprints[0] ===
        entry.typescript_run_fingerprints[1],
    ).length,
    admitted_typescript_writes: admitted.reduce(
      (sum, entry) => sum + entry.typescript_observation.writes.length,
      0,
    ),
    diagnostic_control_typescript_writes: outputControls.reduce(
      (sum, entry) => sum + entry.typescript_observation.writes.length,
      0,
    ),
    admitted_typescript_diagnostics: admitted.reduce(
      (sum, entry) => sum + entry.typescript_observation.reported_diagnostics.length,
      0,
    ),
    unexecuted_candidates: 0,
    undispositioned_candidates: cases.filter(
      (entry) =>
        !entry.disposition ||
        (entry.disposition === "deferred-to-slices" && entry.required_slices.length === 0),
    ).length,
  };
  requireCondition(summary.compiler_candidates === 4_712, "compiler candidate count changed");
  requireCondition(summary.conformance_candidates === 4_315, "conformance candidate count changed");
  requireCondition(
    summary.admitted_cases + summary.deferred_cases === summary.candidates,
    "admission partition is incomplete",
  );
  requireCondition(
    summary.diagnostic_deferred_output_control_cases === 0,
    "diagnostic control count changed",
  );
  requireCondition(
    summary.diagnostic_deferred_output_control_cases + summary.source_deferred_cases ===
      summary.deferred_cases,
    "deferred partition is incomplete",
  );
  requireCondition(
    canonical(summary.target_states) ===
      canonical(EXPECTED_TARGET_STATES),
    "target distribution changed",
  );
  requireCondition(
    canonical(summary.module_states) ===
      canonical(EXPECTED_MODULE_STATES),
    `module distribution changed: ${canonical(summary.module_states)}`,
  );
  requireCondition(
    summary.diagnostic_control_typescript_writes === 0,
    "diagnostic-control write count changed",
  );
  requireCondition(
    summary.typescript_runs === 18_054,
    `TypeScript repetition count changed: ${summary.typescript_runs}`,
  );
  requireCondition(summary.deterministic_typescript_cases === 9_027, "TypeScript determinism is open");
  requireCondition(summary.undispositioned_candidates === 0, "H2.5g retained an undispositioned case");
  const summaryContract = readJson(CONTRACT_RELATIVE_PATH).$defs.summary.properties;
  for (const [field, property] of Object.entries(summaryContract)) {
    if (Object.hasOwn(property, "const")) {
      requireCondition(
        canonical(summary[field]) === canonical(property.const),
        `contract summary ${field} changed: ${canonical(summary[field])}`,
      );
    }
  }
  return withFingerprint(
    {
      schema: 1,
      status: "qualified-typescript-oracle",
      phase: "H2.5g-es2016-target",
      typescript: typescriptRecord,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        h2_5f_merge: H2_5F_MERGE_COMMIT,
        h2_5f_profile,
      },
      selection_contract: {
        global_h2_5g_rows: globalRows.length,
        candidate_definition:
          "the 9,027 dependency-closed rows among the 11,910 global H2.5g rows whose complete required-slice set is closed through H2.5g",
        global_candidate_denominator: candidateRows.length,
        candidate_denominator: selectionOrigins.size,
        future_deferred_rows: globalRows.length - candidateRows.length,
      },
      inputs: inputsRecord,
      execution_contract: executionContract,
      owner_closure: ownerRows,
      cases,
      summary,
    },
    "qualification_fingerprint_sha256",
  );
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
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
  requireCondition(
    argv.length === 5,
    "internal shard mode requires a shard index and count",
  );
  const index = Number(argv[3]);
  const count = Number(argv[4]);
  requireCondition(
    Number.isInteger(count) &&
      count >= 2 &&
      count <= MAX_CHECK_SHARDS &&
      Number.isInteger(index) &&
      index >= 0 &&
      index < count,
    "invalid internal shard selection",
  );
  return { index, count, assigned: Math.floor((9_026 - index) / count) + 1 };
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
          payload !== null &&
            typeof payload === "object" &&
            payload.schema === 1 &&
            payload.shard_index === index &&
            payload.shard_count === count &&
            Array.isArray(payload.shard_cases),
          `observation shard ${index} returned an invalid payload`,
        );
        resolve(payload);
      } catch (error) {
        reject(error);
      }
    });
  });
}

async function runShardedCheck(count) {
  const payloads = await Promise.all(
    Array.from({ length: count }, (_, index) =>
      observeShardInChildProcess(index, count),
    ),
  );
  const adoption = new Map();
  let journalAdopted = 0;
  let observed = 0;
  for (const payload of payloads) {
    journalAdopted += payload.journal_adopted ?? 0;
    observed += payload.observed ?? payload.shard_cases.length;
    for (const record of payload.shard_cases) {
      requireCondition(
        record !== null &&
          typeof record === "object" &&
          typeof record.case_id === "string" &&
          !adoption.has(record.case_id),
        "shard observations overlap or are malformed",
      );
      adoption.set(record.case_id, record);
    }
  }
  requireCondition(
    adoption.size === 9_027,
    `sharded check observed ${adoption.size}/9027 cases`,
  );
  shardAdoption = adoption;
  const artifact = buildArtifact();
  const rendered = render(artifact);
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") ===
        rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-5g-qualification.mjs --write and review`,
  );
  const mint = mintCheckReceipt(artifact);
  if (mint.minted) {
    removeJournals(path.join(WORKSPACE, CHECK_RESUME_DIRECTORY), mint.key);
  }
  writeCheckOutcome({
    completed: true,
    observed_cases: observed,
    journal_adopted: journalAdopted,
    minted: mint.minted,
  });
  process.stdout.write(
    `H2.5g qualification is fresh: candidates=${artifact.summary.candidates} admitted=${artifact.summary.admitted_cases} deferred=${artifact.summary.deferred_cases} check_shards=${count} check_receipt=${mint.minted ? "minted" : "mint-refused"} journal_adopted=${journalAdopted}\n`,
  );
}

// The gate-tax 3 receipt attempt: adopt-and-verify without observation,
// or return null after printing the divergent key term so the caller
// runs the unchanged full re-observation.
function attemptReceiptCheck() {
  checkReceiptAttempt = true;
  try {
    const artifact = buildArtifact();
    requireCondition(
      reusedObservations === 9_027,
      `check receipt adopted ${reusedObservations}/9027 cases`,
    );
    process.stderr.write(
      "H2.5g check receipt: hit; adopted 9,027 stored observations under the full per-case guards\n",
    );
    writeCheckOutcome({
      receipt_attempt: "hit",
      receipt_reused: reusedObservations,
    });
    return artifact;
  } catch (error) {
    if (!(error instanceof CheckReceiptMiss)) throw error;
    observedCases = 0;
    reusedObservations = 0;
    shardOrdinal = 0;
    process.stderr.write(
      `H2.5g check receipt: miss (${error.message}); running the full re-observation\n`,
    );
    writeCheckOutcome({
      receipt_attempt: "miss",
      miss_term: error.message,
      refusals: lastKeyRefusals,
    });
    return null;
  } finally {
    checkReceiptAttempt = false;
  }
}

validateRuntime();
if (MODE === INTERNAL_CHECK_SHARD_MODE) {
  shardAssignment = parseShardArguments(process.argv);
  const shard = buildArtifact();
  requireCondition(
    shard !== null && Array.isArray(shard.shard_cases),
    "internal shard mode did not produce shard observations",
  );
  resumeJournal?.writer.close();
  process.stdout.write(
    render({
      schema: 1,
      shard_index: shardAssignment.index,
      shard_count: shardAssignment.count,
      shard_cases: shard.shard_cases,
      journal_adopted: resumeJournal?.adopted ?? 0,
      observed: resumeJournal?.appended ?? shard.shard_cases.length,
    }),
  );
} else if (MODE === "--check") {
  requireCondition(
    process.env[FRESH_ENV] === undefined ||
      ["0", "1"].includes(process.env[FRESH_ENV]),
    `${FRESH_ENV} must be 0 or 1`,
  );
  if (FRESH) {
    const resumeBase = path.join(WORKSPACE, CHECK_RESUME_DIRECTORY);
    // recursive removal is guarded: the resolved path must be the
    // journal store itself (user directive 2026-08-27 — never trust an
    // unchecked recursive rmSync)
    requireCondition(
      path.resolve(resumeBase) ===
        path.resolve(WORKSPACE, "target", "h2-5g", "check-resume"),
      `refusing to clear unexpected journal path ${resumeBase}`,
    );
    fs.rmSync(resumeBase, { recursive: true, force: true });
    process.stderr.write(
      `H2.5g check receipt: bypassed (${FRESH_ENV}=1) — journals cleared, full re-observation (recorded approval)\n`,
    );
    writeCheckOutcome({ receipt_attempt: "bypassed-fresh" });
  }
  const receiptArtifact = FRESH ? null : attemptReceiptCheck();
  if (receiptArtifact !== null) {
    const rendered = render(receiptArtifact);
    requireCondition(
      fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
        fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") ===
          rendered,
      `stale ${TARGET_RELATIVE_PATH}; run h2-5g-qualification.mjs --write and review`,
    );
    writeCheckOutcome({ completed: true, minted: false });
    process.stdout.write(
      `H2.5g qualification is fresh: candidates=${receiptArtifact.summary.candidates} admitted=${receiptArtifact.summary.admitted_cases} deferred=${receiptArtifact.summary.deferred_cases} check_receipt=hit reused_observations=${reusedObservations}\n`,
    );
  } else if (checkShardCount() > 1) {
    writeCheckOutcome({
      full_observation_started: true,
      check_shards: checkShardCount(),
    });
    await runShardedCheck(checkShardCount());
  } else {
    writeCheckOutcome({ full_observation_started: true, check_shards: 1 });
    const artifact = buildArtifact();
    resumeJournal?.writer.close();
    const rendered = render(artifact);
    requireCondition(
      fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
        fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") ===
          rendered,
      `stale ${TARGET_RELATIVE_PATH}; run h2-5g-qualification.mjs --write and review`,
    );
    const mint = mintCheckReceipt(artifact);
    if (mint.minted) {
      removeJournals(path.join(WORKSPACE, CHECK_RESUME_DIRECTORY), mint.key);
    }
    writeCheckOutcome({
      completed: true,
      observed_cases: resumeJournal?.appended ?? 9_027,
      journal_adopted: resumeJournal?.adopted ?? 0,
      minted: mint.minted,
    });
    process.stdout.write(
      `H2.5g qualification is fresh: candidates=${artifact.summary.candidates} admitted=${artifact.summary.admitted_cases} deferred=${artifact.summary.deferred_cases} check_receipt=${mint.minted ? "minted" : "mint-refused"} journal_adopted=${resumeJournal?.adopted ?? 0}\n`,
    );
  }
} else if (MODE === "--upgrade-observation-layout") {
  const artifact = readJson(TARGET_RELATIVE_PATH);
  requireCondition(
    artifact.generator.sha256 === EXPANDED_RUN_LAYOUT_GENERATOR_SHA256 &&
      hasValidFingerprint(artifact, "qualification_fingerprint_sha256") &&
      artifact.cases.every(
        (entry) =>
          hasValidFingerprint(entry, "case_fingerprint_sha256") &&
          entry.typescript_runs.length === 2 &&
          entry.typescript_runs.every((run) =>
            hasValidFingerprint(run, "run_fingerprint_sha256"),
          ) &&
          entry.typescript_runs[0].run_fingerprint_sha256 ===
            entry.typescript_runs[1].run_fingerprint_sha256,
      ),
    "cannot upgrade invalid expanded TypeScript observations",
  );
  artifact.cases = artifact.cases.map((entry) => {
    const {
      typescript_runs: runs,
      rust_expectation,
      case_fingerprint_sha256: _caseFingerprint,
      ...prefix
    } = entry;
    return withFingerprint(
      {
        ...prefix,
        typescript_observation: runs[0],
        typescript_run_fingerprints: runs.map(
          (run) => run.run_fingerprint_sha256,
        ),
        rust_expectation,
      },
      "case_fingerprint_sha256",
    );
  });
  artifact.generator = pathHash(GENERATOR_RELATIVE_PATH);
  artifact.contract = pathHash(CONTRACT_RELATIVE_PATH);
  delete artifact.qualification_fingerprint_sha256;
  writeFileAtomic(
    path.join(WORKSPACE, TARGET_RELATIVE_PATH),
    render(withFingerprint(artifact, "qualification_fingerprint_sha256")),
  );
  process.stdout.write(`upgraded ${TARGET_RELATIVE_PATH} observation layout\n`);
} else if (MODE === "--rebind-contract") {
  const artifact = readJson(TARGET_RELATIVE_PATH);
  const currentGenerator = pathHash(GENERATOR_RELATIVE_PATH);
  requireCondition(
    hasValidFingerprint(artifact, "qualification_fingerprint_sha256") &&
      artifact.cases.every((entry) => hasValidFingerprint(entry, "case_fingerprint_sha256")),
    "cannot rebind invalid qualification fingerprints",
  );
  if (artifact.generator.sha256 !== currentGenerator.sha256) {
    requireCondition(
      artifact.generator.sha256 === COMPACT_RUN_LAYOUT_GENERATOR_SHA256,
      "cannot rebind an artifact generated by unrelated code",
    );
    const {
      candidates,
      compiler_candidates,
      conformance_candidates,
      ...summaryTail
    } = artifact.summary;
    artifact.summary = {
      candidates,
      compiler_candidates,
      conformance_candidates,
      recorded_compiler_plan_cases: artifact.cases.filter(
        (entry) => entry.execution_route === "recorded-compiler-plan",
      ).length,
      qualified_vfs_cases: artifact.cases.filter(
        (entry) => entry.execution_route === "qualified-vfs",
      ).length,
      virtual_config_cases: artifact.cases.filter(
        (entry) => entry.input.virtual_config !== null,
      ).length,
      vfs_symlink_cases: artifact.cases.filter(
        (entry) => entry.input.vfs_symlinks.length > 0,
      ).length,
      vfs_symlink_paths: artifact.cases.reduce(
        (sum, entry) => sum + entry.input.vfs_symlinks.length,
        0,
      ),
      ...summaryTail,
    };
    artifact.execution_contract.admission = admissionContract();
    artifact.generator = currentGenerator;
  }
  artifact.contract = pathHash(CONTRACT_RELATIVE_PATH);
  delete artifact.qualification_fingerprint_sha256;
  writeFileAtomic(
    path.join(WORKSPACE, TARGET_RELATIVE_PATH),
    render(withFingerprint(artifact, "qualification_fingerprint_sha256")),
  );
  process.stdout.write(`rebound ${TARGET_RELATIVE_PATH} to the reviewed contract\n`);
} else {
  const artifact = buildArtifact();
  if (MODE === "--preflight") {
    requireCondition(artifact === null, "H2.5g preflight unexpectedly built an artifact");
    process.stdout.write("H2.5g qualification fixture/config preflight passed\n");
  } else {
  const rendered = render(artifact);
  if (MODE === "--write") {
  writeFileAtomic(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
  process.stdout.write(
    `wrote ${TARGET_RELATIVE_PATH}: candidates=${artifact.summary.candidates} admitted=${artifact.summary.admitted_cases} deferred=${artifact.summary.deferred_cases} reused_observations=${reusedObservations}\n`,
  );
  } else if (MODE === undefined) {
    process.stdout.write(rendered);
  } else {
    fail("usage: h2-5g-qualification.mjs [--preflight|--write|--check|--upgrade-observation-layout|--rebind-contract]");
  }
  }
}
