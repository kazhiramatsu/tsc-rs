import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const ORACLE_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(ORACLE_PATH), "../..");
const ORACLE_RELATIVE_PATH = "crates/oracle/h1-conformance-expansion.mjs";
const MANIFEST_RELATIVE_PATH =
  "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json";
const SOURCE_ROOT_RELATIVE_PATH = "ts-tests/tests/cases/conformance";
const SOURCE_ROOT = path.join(WORKSPACE, SOURCE_ROOT_RELATIVE_PATH);
const TYPESCRIPT_BUNDLE_RELATIVE_PATH =
  "vendor/typescript-6.0.3/lib/typescript.js";
const NODE_VERSION_PATH = path.join(WORKSPACE, ".node-version");
const TYPESCRIPT_VERSION = "6.0.3";
const TYPESCRIPT_BUNDLE_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const SOURCE_INVENTORY_SHA256 =
  "8dd4be94d28c32e953c5931daed512f6f1e4bca13eb0edf550c71b1db4a8c598";
const NOT_ENUMERATED_PATH =
  "parser/ecmascript5/Statements/ReturnStatements/parserReturnStatement4.js";
const OBSERVATION_INDEXES = [0, 1, 2, 3, 4, 5];
const OPTION_PATTERN = /^\/{2}\s*@(\w+)\s*:\s*([^\r\n]*)/gm;
const OPTION_LINE_PATTERN = /^\/{2}\s*@(\w+)\s*:\s*([^\r\n]*)/;
const LINK_LINE_PATTERN = /^\/{2}\s*@link\s*:\s*([^\r\n]*)\s*->\s*([^\r\n]*)/;

const EXPECTED_SUMMARY = {
  source_files: 5908,
  source_bytes: 3825804,
  unique_blobs: 5862,
  enumerated_fixtures: 5907,
  not_enumerated_sources: 1,
  default_fixtures: 4809,
  matrix_fixtures: 1098,
  cases: 7697,
  normal_units: 8055,
  virtual_configs: 27,
  present_empty_units: 14,
  missing_content_units: 0,
  link_directives: 0,
  document_symlink_directives: 0,
  document_symlink_paths: 0,
  runner_observations: 6,
  case_observations: 46182,
  not_run_cases: 7697,
  not_run_case_observations: 46182,
  execution_results_recorded: 0,
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
  const nodeVersion = fs.readFileSync(NODE_VERSION_PATH, "utf8").trim();
  const runningVersion = process.version.startsWith("v")
    ? process.version.slice(1)
    : process.version;
  requireCondition(
    runningVersion === nodeVersion,
    `H1 conformance oracle requires Node ${nodeVersion}; running ${process.version}`,
  );
  requireCondition(ts.version === TYPESCRIPT_VERSION, "unexpected TypeScript version");
  requireCondition(
    pathHash(TYPESCRIPT_BUNDLE_RELATIVE_PATH).sha256 === TYPESCRIPT_BUNDLE_SHA256,
    "vendored TypeScript bundle hash changed",
  );
}

function compareDirectoryEntries(left, right) {
  return Buffer.compare(Buffer.from(left.name), Buffer.from(right.name));
}

function walkFiles(directory = SOURCE_ROOT, output = []) {
  const entries = fs
    .readdirSync(directory, { withFileTypes: true })
    .sort(compareDirectoryEntries);
  requireCondition(entries.length > 0, `empty conformance directory ${directory}`);
  for (const entry of entries) {
    const candidate = path.join(directory, entry.name);
    requireCondition(!entry.isSymbolicLink(), `symlink in conformance suite: ${candidate}`);
    if (entry.isDirectory()) {
      walkFiles(candidate, output);
    } else {
      requireCondition(entry.isFile(), `unsupported conformance entry: ${candidate}`);
      const relative = path.relative(SOURCE_ROOT, candidate).split(path.sep).join("/");
      requireCondition(
        relative.length > 0 &&
          relative === path.posix.normalize(relative) &&
          !relative.startsWith("../") &&
          !relative.startsWith("/"),
        `unsafe conformance path ${JSON.stringify(relative)}`,
      );
      output.push(relative);
    }
  }
  return output;
}

function sourceInventorySha256(sources) {
  const digest = crypto.createHash("sha256");
  const field = (value) => {
    const bytes = Buffer.from(value);
    const length = Buffer.alloc(8);
    length.writeBigUInt64BE(BigInt(bytes.length));
    digest.update(length);
    digest.update(bytes);
  };
  for (const source of sources) {
    field(source.path);
    field(source.mode);
    const byteCount = Buffer.alloc(8);
    byteCount.writeBigUInt64BE(BigInt(source.bytes));
    digest.update(byteCount);
    field(source.sha256);
    field(source.git_blob_sha1);
  }
  return digest.digest("hex");
}

function collectSources() {
  return walkFiles().map((relative) => {
    const raw = fs.readFileSync(path.join(SOURCE_ROOT, ...relative.split("/")));
    return {
      identity: {
        path: relative,
        mode: "100644",
        bytes: raw.length,
        sha256: sha256(raw),
        git_blob_sha1: gitBlobSha1(raw),
      },
      raw,
    };
  });
}

function decodeSource(raw) {
  if (raw.subarray(0, 3).equals(Buffer.from([0xef, 0xbb, 0xbf]))) {
    return { encoding: "utf-8-bom", text: raw.subarray(3).toString("utf8") };
  }
  return { encoding: "utf-8", text: raw.toString("utf8") };
}

function extractSettings(text) {
  const settings = {};
  OPTION_PATTERN.lastIndex = 0;
  for (let match; (match = OPTION_PATTERN.exec(text)) !== null; ) {
    settings[match[1]] = match[2].trim();
  }
  OPTION_PATTERN.lastIndex = 0;
  return settings;
}

function optionValues(varyBy) {
  const declaration = ts.optionDeclarations.find(
    (option) => option.name.toLowerCase() === varyBy.toLowerCase(),
  );
  if (!declaration) return undefined;
  if (typeof declaration.type === "object") return declaration.type;
  if (declaration.type === "boolean") {
    return new Map([
      ["true", 1],
      ["false", 0],
    ]);
  }
  return undefined;
}

function splitVaryBySettingValue(text, varyBy) {
  if (!text) return undefined;
  let star = false;
  const includes = [];
  const excludes = [];
  for (let value of text.split(",")) {
    value = value.trim().toLowerCase();
    if (!value) continue;
    if (value === "*") star = true;
    else if (value.startsWith("-") || value.startsWith("!")) {
      excludes.push(value.slice(1));
    } else includes.push(value);
  }
  if (includes.length <= 1 && !star && excludes.length === 0) return undefined;

  const values = optionValues(varyBy);
  const variations = [];
  const equivalent = (variation, key, value) =>
    variation.key === key || (value !== undefined && variation.value === value);
  for (const include of includes) {
    const value = values?.get(include);
    if (!variations.some((variation) => equivalent(variation, include, value))) {
      variations.push({ key: include, value });
    }
  }
  if (star && values) {
    for (const [key, value] of values.entries()) {
      if (
        !variations.some(
          (variation) => variation.key === key || variation.value === value,
        )
      ) {
        variations.push({ key, value });
      }
    }
  }
  for (const exclude of excludes) {
    const value = values?.get(exclude);
    for (let index = variations.length - 1; index >= 0; index -= 1) {
      if (equivalent(variations[index], exclude, value)) variations.splice(index, 1);
    }
  }
  requireCondition(variations.length > 0, `empty @${varyBy} variation`);
  return variations.map((variation) => variation.key);
}

function runnerVaryBy() {
  return ts.optionDeclarations
    .filter(
      (option) =>
        !option.isCommandLineOnly &&
        (option.type === "boolean" || typeof option.type === "object") &&
        (option.affectsProgramStructure ||
          option.affectsEmit ||
          option.affectsModuleResolution ||
          option.affectsBindDiagnostics ||
          option.affectsSemanticDiagnostics ||
          option.affectsSourceFile ||
          option.affectsDeclarationPath ||
          option.affectsBuildInfo),
    )
    .map((option) => option.name)
    .concat(["noEmit", "isolatedModules"]);
}

function expandConfigurations(fixturePath, settings, varyBy) {
  const dimensions = [];
  let count = 1;
  for (const key of varyBy) {
    if (!Object.hasOwn(settings, key)) continue;
    const entries = splitVaryBySettingValue(settings[key], key);
    if (!entries) continue;
    count *= entries.length;
    requireCondition(count <= 25, `${fixturePath} exceeds 25 configurations`);
    dimensions.push([key, entries]);
  }
  if (dimensions.length === 0) {
    return [
      {
        variant: "default",
        description: "",
        upstream_name: path.posix.basename(fixturePath),
        settings: [],
      },
    ];
  }

  const states = [];
  const visit = (offset, state) => {
    if (offset === dimensions.length) {
      states.push([...state]);
      return;
    }
    const [key, entries] = dimensions[offset];
    for (const value of entries) {
      state.push({ name: key, value });
      visit(offset + 1, state);
      state.pop();
    }
  };
  visit(0, []);
  const basename = path.posix.basename(fixturePath);
  const extension = basename.endsWith(".tsx") ? ".tsx" : ".ts";
  const stem = basename.slice(0, -extension.length);
  return states.map((settingsForCase) => {
    const sorted = [...settingsForCase].sort((left, right) =>
      left.name < right.name ? -1 : left.name > right.name ? 1 : 0,
    );
    const variant = sorted
      .map((setting) => `${setting.name.toLowerCase()}=${setting.value.toLowerCase()}`)
      .join(",");
    return {
      variant,
      description: sorted
        .map((setting) => `@${setting.name}: ${setting.value}`)
        .join(", "),
      upstream_name: `${stem}(${variant})${extension}`,
      settings: settingsForCase,
    };
  });
}

function setOrdered(settings, name, value) {
  const existing = settings.find((setting) => setting.name === name);
  if (existing) existing.value = value;
  else settings.push({ name, value });
}

function contentIdentity(content) {
  if (content === undefined) return { state: "missing" };
  const bytes = Buffer.from(content);
  return { state: "present", utf8_bytes: bytes.length, sha256: sha256(bytes) };
}

function makeUnit(name, fileOptions, content) {
  const symlink = fileOptions.find((setting) => setting.name === "symlink");
  return {
    name,
    file_options: fileOptions,
    content: contentIdentity(content),
    document_symlinks:
      symlink && symlink.value
        ? symlink.value.split(",").map((value) => value.trim())
        : [],
  };
}

function makeUnitsFromTest(text, fixturePath) {
  const parsedUnits = [];
  const links = [];
  let currentContent;
  let currentOptions = [];
  let currentName;

  for (const line of text.split(/\r?\n/)) {
    const link = LINK_LINE_PATTERN.exec(line);
    if (link) {
      links.push({ target: link[1].trim(), link_path: link[2].trim() });
      continue;
    }
    const option = OPTION_LINE_PATTERN.exec(line);
    if (option) {
      const name = option[1];
      const value = option[2].trim();
      setOrdered(currentOptions, name, value);
      if (name.toLowerCase() !== "filename") continue;
      if (currentName) {
        parsedUnits.push(makeUnit(currentName, currentOptions, currentContent));
        currentContent = undefined;
        currentOptions = [];
        currentName = value;
      } else {
        currentName = value;
        if (
          currentContent &&
          ts.skipTrivia(currentContent, 0, false, false) !== currentContent.length
        ) {
          fail(`${fixturePath} has non-comment content before first @filename`);
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
    parsedUnits.length > 0 || currentName
      ? currentName
      : path.posix.basename(fixturePath);
  parsedUnits.push(makeUnit(currentName, currentOptions, currentContent || ""));

  const configIndex = parsedUnits.findIndex((unit) => {
    const name = path.posix.basename(unit.name).toLowerCase();
    return name === "tsconfig.json" || name === "jsconfig.json";
  });
  const virtualConfig = configIndex >= 0 ? parsedUnits.splice(configIndex, 1)[0] : null;
  return { normal_units: parsedUnits, virtual_config: virtualConfig, links };
}

function percentEncode(value, preserveSlash) {
  const unreserved = /^[A-Za-z0-9._~-]$/;
  let output = "";
  for (const byte of Buffer.from(value)) {
    const character = String.fromCharCode(byte);
    if (unreserved.test(character) || (preserveSlash && character === "/")) {
      output += character;
    } else output += `%${byte.toString(16).toUpperCase().padStart(2, "0")}`;
  }
  return output;
}

function caseId(fixturePath, variant) {
  return `typescript-${TYPESCRIPT_VERSION}/conformance/${percentEncode(fixturePath, true)}#${percentEncode(variant, false)}`;
}

function verifyManifest(manifest) {
  requireCondition(manifest.schema === 1, "unexpected manifest schema");
  requireCondition(manifest.status === "expanded-not-run", "unexpected manifest status");
  requireCondition(
    manifest.phase === "H1.0a-conformance-runner-expansion",
    "unexpected manifest phase",
  );
  requireJsonEqual(
    manifest.independent_oracle,
    pathHash(ORACLE_RELATIVE_PATH),
    "independent oracle identity",
  );

  const varyBy = runnerVaryBy();
  requireCondition(varyBy.length === 77, "unexpected runner varyBy count");
  requireJsonEqual(manifest.runner_contract.vary_by, varyBy, "runner varyBy order");
  requireCondition(manifest.runner_contract.emit_enabled === true, "runner emit must be enabled");
  requireCondition(
    manifest.runner_contract.observations.length === 6 &&
      manifest.runner_contract.observations.every(
        (observation) =>
          observation.initial_execution_state === "not-run" &&
          observation.reference_baseline_state === "content-not-vendored-or-compared",
      ),
    "runner observations must all remain not-run and uncompared",
  );

  const collected = collectSources();
  const sourceIdentities = collected.map((source) => source.identity);
  requireJsonEqual(manifest.sources, sourceIdentities, "complete source inventory");
  requireCondition(
    sourceInventorySha256(sourceIdentities) === SOURCE_INVENTORY_SHA256 &&
      manifest.source_inventory_sha256 === SOURCE_INVENTORY_SHA256,
    "source inventory hash changed",
  );

  const notEnumeratedIndex = sourceIdentities.findIndex(
    (source) => source.path === NOT_ENUMERATED_PATH,
  );
  requireJsonEqual(
    manifest.not_enumerated_sources,
    [
      {
        source: notEnumeratedIndex,
        reason: "extension-does-not-match-/\\.tsx?$/",
      },
    ],
    "runner non-enumerated source",
  );

  let fixtureOffset = 0;
  let caseOffset = 0;
  for (let sourceIndex = 0; sourceIndex < collected.length; sourceIndex += 1) {
    const { identity, raw } = collected[sourceIndex];
    if (!/\.tsx?$/.test(identity.path)) continue;
    const decoded = decodeSource(raw);
    const settings = extractSettings(decoded.text);
    const units = makeUnitsFromTest(decoded.text, identity.path);
    const configurations = expandConfigurations(identity.path, settings, varyBy);
    const expectedFixture = {
      source: sourceIndex,
      encoding: decoded.encoding,
      decoded_utf8_bytes: Buffer.byteLength(decoded.text),
      decoded_sha256: sha256(Buffer.from(decoded.text)),
      settings: Object.entries(settings).map(([name, value]) => ({ name, value })),
      normal_units: units.normal_units,
      virtual_config: units.virtual_config,
      links: units.links,
      configurations,
    };
    requireJsonEqual(
      manifest.fixtures[fixtureOffset],
      expectedFixture,
      `fixture ${identity.path}`,
    );
    for (let configuration = 0; configuration < configurations.length; configuration += 1) {
      const expectedCase = {
        id: caseId(identity.path, configurations[configuration].variant),
        source: sourceIndex,
        configuration,
        observations: OBSERVATION_INDEXES,
        initial_execution_state: "not-run",
        reference_baseline_state: "content-not-vendored-or-compared",
      };
      requireJsonEqual(
        manifest.cases[caseOffset],
        expectedCase,
        `case ${expectedCase.id}`,
      );
      caseOffset += 1;
    }
    fixtureOffset += 1;
  }
  requireCondition(fixtureOffset === manifest.fixtures.length, "unreferenced fixture rows");
  requireCondition(caseOffset === manifest.cases.length, "unreferenced case rows");
  requireJsonEqual(manifest.summary, EXPECTED_SUMMARY, "frozen summary");
}

if (process.argv.length !== 3 || process.argv[2] !== "--check") {
  fail("usage: node crates/oracle/h1-conformance-expansion.mjs --check");
}
validateRuntime();
const manifest = JSON.parse(
  fs.readFileSync(path.join(WORKSPACE, MANIFEST_RELATIVE_PATH), "utf8"),
);
verifyManifest(manifest);
process.stdout.write(
  `H1 conformance expansion oracle matched: sources=${manifest.summary.source_files} fixtures=${manifest.summary.enumerated_fixtures} cases=${manifest.summary.cases} observations=${manifest.summary.case_observations}\n`,
);
