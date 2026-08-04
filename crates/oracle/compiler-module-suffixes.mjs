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
  "vendor/typescript-6.0.3/compiler-module-suffixes.v1.json";
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
const FIRST_FIXTURE_INDEX = 4293;
const LAST_FIXTURE_INDEX = 4308;

const EXPECTED_FIXTURES = [
  "moduleResolutionWithSuffixes_empty.ts",
  "moduleResolutionWithSuffixes_notSpecified.ts",
  "moduleResolutionWithSuffixes_one.ts",
  "moduleResolutionWithSuffixes_oneBlank.ts",
  "moduleResolutionWithSuffixes_oneNotFound.ts",
  "moduleResolutionWithSuffixes_one_dirModuleWithIndex.ts",
  "moduleResolutionWithSuffixes_one_externalModule.ts",
  "moduleResolutionWithSuffixes_one_externalModulePath.ts",
  "moduleResolutionWithSuffixes_one_externalModule_withPaths.ts",
  "moduleResolutionWithSuffixes_one_externalTSModule.ts",
  "moduleResolutionWithSuffixes_one_jsModule.ts",
  "moduleResolutionWithSuffixes_one_jsonModule.ts",
  "moduleResolutionWithSuffixes_threeLastIsBlank1.ts",
  "moduleResolutionWithSuffixes_threeLastIsBlank2.ts",
  "moduleResolutionWithSuffixes_threeLastIsBlank3.ts",
  "moduleResolutionWithSuffixes_threeLastIsBlank4.ts",
];

// Filled from the first verified production run, then held constant so the
// producer cannot silently widen or shrink this focused contract.
const EXPECTED_SUMMARY = {
  fixture_total: 16,
  request_total: 18,
  resolved_total: 16,
  unresolved_total: 2,
  file_probe_total: 78,
  failed_lookup_total: 95,
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
    `compiler module-suffix oracle requires Node ${recordedNodeVersion}; running ${runningNodeVersion}`,
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
  requireCondition(manifest.schema === 1, "expansion manifest schema must be 1");
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

function readVerifiedSource(manifest, compilerRoot, recorded, fixtureIndex) {
  requireCondition(
    recorded.source === fixtureIndex,
    `fixture ${fixtureIndex} must retain its source index`,
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
  return {
    index: recorded.source,
    path: inventory.path,
    bytes: inventory.bytes,
    sha256: inventory.sha256,
    git_blob_sha1: inventory.git_blob_sha1,
    decoded_sha256: recorded.decoded_sha256,
    decoded,
  };
}

function matchOnce(regex, line) {
  const match = regex.exec(line);
  regex.lastIndex = 0;
  return match;
}

function makeUnitsFromTest(code) {
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
        currentFileContent = "";
      }
      continue;
    }
    if (currentFileContent === undefined) currentFileContent = "";
    else if (currentFileContent !== "") currentFileContent += "\n";
    currentFileContent += line;
  }
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
  const bytes = Buffer.from(unit.content, "utf8");
  return {
    state: "present",
    utf8_bytes: bytes.length,
    sha256: sha256(bytes),
  };
}

function verifyUnit(unit, expected, label) {
  requireCondition(isObject(expected), `${label} is absent from the manifest`);
  requireCondition(unit.name === expected.name, `${label} name does not match`);
  const fileOptions = Object.entries(unit.fileOptions).map(([name, value]) => ({
    name,
    value,
  }));
  requireJsonEqual(fileOptions, expected.file_options, `${label} file options`);
  requireJsonEqual(recordedUnitContent(unit), expected.content, `${label} content`);
}

function verifyFixtureExpansion(recorded, source, units, links, configIndex) {
  requireCondition(
    units.length === recorded.normal_units.length + 1,
    `${source.path} unit count does not match the manifest`,
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
    `${source.path} normal-unit partition does not match`,
  );
  requireJsonEqual(links, recorded.links, `${source.path} @link directives`);
}

function normalizeAbsolute(fileName) {
  return ts.getNormalizedAbsolutePath(fileName, "/");
}

function createUnitIndex(units) {
  const files = new Map();
  for (const unit of units) {
    const normalized = normalizeAbsolute(unit.name);
    requireCondition(
      !files.has(normalized.toLowerCase()),
      `duplicate case-insensitive unit ${normalized}`,
    );
    files.set(normalized.toLowerCase(), { ...unit, normalized });
  }
  return files;
}

function hasDirectory(files, directory) {
  const normalized = normalizeAbsolute(directory).replace(/\/$/, "").toLowerCase();
  if (normalized === "") return true;
  const prefix = `${normalized}/`;
  return [...files.keys()].some((fileName) => fileName.startsWith(prefix));
}

function createConfigHost(units) {
  const files = createUnitIndex(units);
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
          const names = [];
          const directories = new Set();
          for (const unit of files.values()) {
            if (unit.normalized.toLowerCase().startsWith(dir.toLowerCase())) {
              let relative = unit.normalized.substring(dir.length);
              if (relative.startsWith("/")) relative = relative.substring(1);
              const separator = relative.indexOf("/");
              if (separator >= 0) directories.add(relative.substring(0, separator));
              else names.push(relative);
            }
          }
          return { files: names, directories: ts.arrayFrom(directories) };
        },
        ts.identity,
      );
    },
    fileExists(fileName) {
      return files.has(normalizeAbsolute(fileName).toLowerCase());
    },
    readFile(fileName) {
      return files.get(normalizeAbsolute(fileName).toLowerCase())?.content;
    },
  };
}

function createResolutionHost(units) {
  const files = createUnitIndex(units);
  const fileProbes = [];
  const directoryProbes = [];
  const readProbes = [];
  const realpathProbes = [];
  return {
    fileProbes,
    directoryProbes,
    readProbes,
    realpathProbes,
    fileExists(fileName) {
      const normalized = normalizeAbsolute(fileName);
      const result = files.has(normalized.toLowerCase());
      fileProbes.push({ path: normalized, result });
      return result;
    },
    readFile(fileName) {
      const normalized = normalizeAbsolute(fileName);
      const result = files.get(normalized.toLowerCase())?.content;
      readProbes.push({
        path: normalized,
        result: result === undefined ? "missing" : "text",
      });
      return result;
    },
    directoryExists(directory) {
      const normalized = normalizeAbsolute(directory).replace(/\/$/, "") || "/";
      const result = hasDirectory(files, normalized);
      directoryProbes.push({ path: normalized, result });
      return result;
    },
    realpath(fileName) {
      const normalized = normalizeAbsolute(fileName);
      const actual = files.get(normalized.toLowerCase())?.normalized ?? normalized;
      realpathProbes.push({ path: normalized, result: actual });
      return actual;
    },
    getCurrentDirectory() {
      return "/";
    },
    useCaseSensitiveFileNames() {
      return false;
    },
  };
}

function packageIdRecord(packageId) {
  if (!packageId) return null;
  return {
    name: packageId.name,
    sub_module_name: packageId.subModuleName,
    version: packageId.version,
    peer_dependencies: packageId.peerDependencies ?? null,
  };
}

function resolutionRecord(resolved) {
  if (!resolved) return { state: "not_found" };
  return {
    state: "resolved",
    resolved_file_name: resolved.resolvedFileName,
    original_path: resolved.originalPath ?? null,
    extension: resolved.extension,
    is_external_library_import: resolved.isExternalLibraryImport === true,
    package_id: packageIdRecord(resolved.packageId),
  };
}

function importedRequests(units) {
  const requests = [];
  for (const unit of units) {
    if (isConfigFileName(unit.name) || unit.content === undefined) continue;
    const imports = ts.preProcessFile(unit.content, true, true).importedFiles;
    for (const imported of imports) {
      requests.push({
        containing_file: normalizeAbsolute(unit.name),
        specifier: imported.fileName,
      });
    }
  }
  return requests;
}

function buildFixture(manifest, compilerRoot, recorded, fixtureIndex) {
  requireCondition(
    isObject(recorded.virtual_config),
    `fixture ${fixtureIndex} must contain a virtual config`,
  );
  requireCondition(
    recorded.configurations?.length === 1 &&
      recorded.configurations[0].variant === "default",
    `fixture ${fixtureIndex} must contain one default configuration`,
  );
  const source = readVerifiedSource(
    manifest,
    compilerRoot,
    recorded,
    fixtureIndex,
  );
  requireCondition(
    source.path === EXPECTED_FIXTURES[fixtureIndex - FIRST_FIXTURE_INDEX],
    `fixture ${fixtureIndex} path is outside the frozen moduleSuffixes sequence`,
  );
  const { units, links } = makeUnitsFromTest(source.decoded);
  const configIndexes = units
    .map((unit, index) => ({ unit, index }))
    .filter(({ unit }) => isConfigFileName(unit.name))
    .map(({ index }) => index);
  requireCondition(
    configIndexes.length === 1,
    `${source.path} must contain exactly one config unit`,
  );
  const configIndex = configIndexes[0];
  verifyFixtureExpansion(recorded, source, units, links, configIndex);

  const configUnit = units[configIndex];
  const configFileName = normalizeAbsolute(configUnit.name);
  const configSource = ts.parseJsonText(configFileName, configUnit.content);
  const parsed = ts.parseJsonSourceFileConfigFileContent(
    configSource,
    createConfigHost(units),
    ts.getDirectoryPath(configFileName),
    undefined,
    configFileName,
  );
  requireCondition(
    parsed.errors.length === 0,
    `${source.path} config parse unexpectedly produced diagnostics`,
  );

  const requests = importedRequests(units).map((request) => {
    const host = createResolutionHost(units);
    const resolution = ts.resolveModuleName(
      request.specifier,
      request.containing_file,
      parsed.options,
      host,
    );
    return {
      ...request,
      resolution: resolutionRecord(resolution.resolvedModule),
      failed_lookup_locations: [...(resolution.failedLookupLocations ?? [])],
      file_probes: host.fileProbes,
      directory_probes: host.directoryProbes,
      read_probes: host.readProbes,
      realpath_probes: host.realpathProbes,
    };
  });

  return {
    fixture_index: fixtureIndex,
    case_id: `typescript-6.0.3/compiler/${source.path}#default`,
    source: {
      index: source.index,
      path: source.path,
      bytes: source.bytes,
      sha256: source.sha256,
      git_blob_sha1: source.git_blob_sha1,
      decoded_sha256: source.decoded_sha256,
    },
    config_unit: {
      name: configFileName,
      text: configUnit.content,
      utf8_bytes: Buffer.byteLength(configUnit.content, "utf8"),
      sha256: sha256(Buffer.from(configUnit.content, "utf8")),
    },
    units: units
      .filter((_, index) => index !== configIndex)
      .map((unit) => ({
        name: normalizeAbsolute(unit.name),
        text: unit.content,
        utf8_bytes: Buffer.byteLength(unit.content, "utf8"),
        sha256: sha256(Buffer.from(unit.content, "utf8")),
      })),
    requests,
  };
}

function summarize(fixtures) {
  const requests = fixtures.flatMap((fixture) => fixture.requests);
  return {
    fixture_total: fixtures.length,
    request_total: requests.length,
    resolved_total: requests.filter(
      (request) => request.resolution.state === "resolved",
    ).length,
    unresolved_total: requests.filter(
      (request) => request.resolution.state === "not_found",
    ).length,
    file_probe_total: requests.reduce(
      (total, request) => total + request.file_probes.length,
      0,
    ),
    failed_lookup_total: requests.reduce(
      (total, request) => total + request.failed_lookup_locations.length,
      0,
    ),
  };
}

function generateArtifact() {
  validateRuntime();
  validateTypeScriptRuntime();
  const { manifest, compilerRoot } = readManifest();
  const fixtures = [];
  for (
    let fixtureIndex = FIRST_FIXTURE_INDEX;
    fixtureIndex <= LAST_FIXTURE_INDEX;
    fixtureIndex += 1
  ) {
    fixtures.push(
      buildFixture(
        manifest,
        compilerRoot,
        manifest.compiler_fixtures[fixtureIndex],
        fixtureIndex,
      ),
    );
  }
  const summary = summarize(fixtures);
  requireJsonEqual(summary, EXPECTED_SUMMARY, "moduleSuffixes oracle summary");
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
    fixture_range: {
      first: FIRST_FIXTURE_INDEX,
      last: LAST_FIXTURE_INDEX,
    },
    summary,
    fixtures,
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
        `regenerate with: node crates/oracle/compiler-module-suffixes.mjs > ${ARTIFACT_RELATIVE_PATH}`,
    );
  }
}

const arguments_ = process.argv.slice(2);
requireCondition(
  arguments_.length === 0 ||
    (arguments_.length === 1 && arguments_[0] === "--check"),
  "usage: node crates/oracle/compiler-module-suffixes.mjs [--check]",
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
