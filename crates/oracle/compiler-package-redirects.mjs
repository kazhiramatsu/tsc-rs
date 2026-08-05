import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

let ts;

const DRIVER_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE = path.resolve(DRIVER_DIRECTORY, "../..");
const MANIFEST_RELATIVE_PATH =
  "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const MANIFEST_PATH = path.join(WORKSPACE, MANIFEST_RELATIVE_PATH);
const ARTIFACT_RELATIVE_PATH =
  "vendor/typescript-6.0.3/compiler-package-redirects.v1.json";
const ARTIFACT_PATH = path.join(WORKSPACE, ARTIFACT_RELATIVE_PATH);
const TYPESCRIPT_BUNDLE_RELATIVE_PATH =
  "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_BUNDLE_PATH = path.join(
  WORKSPACE,
  TYPESCRIPT_BUNDLE_RELATIVE_PATH,
);
const COMPILER_ROOT = path.join(
  WORKSPACE,
  "ts-tests/tests/cases/compiler",
);

const EXPECTED_NODE_VERSION = "25.2.1";
const EXPECTED_TYPESCRIPT_VERSION = "6.0.3";
const EXPECTED_SOURCE_COMMIT =
  "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_MANIFEST_SHA256 =
  "9c6e991103b571f7a8800dc5e1ef66088017689f3769de8fcdb408b2dc125188";
const EXPECTED_TYPESCRIPT_BUNDLE_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const FIRST_FIXTURE_INDEX = 2064;
const LAST_FIXTURE_INDEX = 2071;
const EXPECTED_FIXTURES = [
  "duplicatePackage.ts",
  "duplicatePackage_globalMerge.ts",
  "duplicatePackage_packageIdIncludesSubModule.ts",
  "duplicatePackage_referenceTypes.ts",
  "duplicatePackage_relativeImportWithinPackage.ts",
  "duplicatePackage_relativeImportWithinPackage_scoped.ts",
  "duplicatePackage_subModule.ts",
  "duplicatePackage_withErrors.ts",
];
const EXPECTED_SUMMARY = {
  fixture_total: 8,
  source_total: 38,
  redirect_total: 7,
  diagnostic_total: 5,
};

const OPTION_REGEX = /^\/{2}\s*@(\w+)\s*:\s*([^\r\n]*)/gm;

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

function jsonEqual(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function requireJsonEqual(actual, expected, label) {
  requireCondition(
    jsonEqual(actual, expected),
    `${label} mismatch:\nactual: ${JSON.stringify(actual)}\nexpected: ${JSON.stringify(expected)}`,
  );
}

function validateRuntime() {
  const recordedNodeVersion = fs
    .readFileSync(path.join(WORKSPACE, ".node-version"), "utf8")
    .trim();
  requireCondition(
    recordedNodeVersion === EXPECTED_NODE_VERSION,
    `.node-version must remain ${EXPECTED_NODE_VERSION}`,
  );
  requireCondition(
    process.version.replace(/^v/, "") === recordedNodeVersion,
    `compiler package-redirect oracle requires Node ${recordedNodeVersion}; running ${process.version}`,
  );
  requireCondition(
    sha256(fs.readFileSync(TYPESCRIPT_BUNDLE_PATH)) ===
      EXPECTED_TYPESCRIPT_BUNDLE_SHA256,
    `${TYPESCRIPT_BUNDLE_RELATIVE_PATH} changed`,
  );
}

function readManifest() {
  const bytes = fs.readFileSync(MANIFEST_PATH);
  requireCondition(
    sha256(bytes) === EXPECTED_MANIFEST_SHA256,
    `${MANIFEST_RELATIVE_PATH} changed`,
  );
  const manifest = JSON.parse(bytes.toString("utf8"));
  requireCondition(manifest.schema === 1, "manifest schema must remain 1");
  requireCondition(
    manifest.typescript_version === EXPECTED_TYPESCRIPT_VERSION &&
      manifest.source_commit === EXPECTED_SOURCE_COMMIT,
    "manifest TypeScript identity changed",
  );
  return manifest;
}

function matchOnce(regex, line) {
  const match = regex.exec(line);
  regex.lastIndex = 0;
  return match;
}

function makeUnitsFromTest(code) {
  const units = [];
  let currentFileContent;
  let currentFileOptions = {};
  let currentFileName;
  for (const line of code.split(/\r?\n/)) {
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
  return units;
}

function normalizeAbsolute(fileName) {
  return ts.getNormalizedAbsolutePath(fileName, "/");
}

function hasVirtualDirectory(files, directory) {
  const normalized = normalizeAbsolute(directory).replace(/\/$/, "");
  const prefix = `${normalized}/`;
  return [...files.keys()].some((fileName) => fileName.startsWith(prefix));
}

function virtualDirectories(files, directory) {
  const normalized = normalizeAbsolute(directory).replace(/\/$/, "");
  const prefix = `${normalized}/`;
  const names = new Set();
  for (const fileName of files.keys()) {
    if (!fileName.startsWith(prefix)) continue;
    const tail = fileName.slice(prefix.length);
    const separator = tail.indexOf("/");
    if (separator >= 0) names.add(tail.slice(0, separator));
  }
  return [...names].sort().map((name) => `${normalized}/${name}`);
}

function createHost(units, options) {
  const files = new Map(
    units.map((unit) => [normalizeAbsolute(unit.name), unit.content]),
  );
  const host = ts.createCompilerHost(options, true);
  const systemFileExists = host.fileExists.bind(host);
  const systemReadFile = host.readFile.bind(host);
  const systemDirectoryExists = host.directoryExists?.bind(host);
  const systemGetDirectories = host.getDirectories?.bind(host);
  const systemRealpath = host.realpath?.bind(host);
  host.trace = () => {};
  host.getCurrentDirectory = () => "/";
  host.useCaseSensitiveFileNames = () => true;
  host.getCanonicalFileName = (fileName) => fileName;
  host.fileExists = (fileName) =>
    files.has(normalizeAbsolute(fileName)) || systemFileExists(fileName);
  host.readFile = (fileName) =>
    files.get(normalizeAbsolute(fileName)) ?? systemReadFile(fileName);
  host.directoryExists = (directory) =>
    hasVirtualDirectory(files, directory) ||
    (systemDirectoryExists?.(directory) ?? false);
  host.getDirectories = (directory) => {
    const virtual = virtualDirectories(files, directory);
    return virtual.length > 0
      ? virtual
      : (systemGetDirectories?.(directory) ?? []);
  };
  host.realpath = (fileName) =>
    files.has(normalizeAbsolute(fileName))
      ? normalizeAbsolute(fileName)
      : (systemRealpath?.(fileName) ?? fileName);
  return host;
}

function messageChainRecord(message, fallbackCode, fallbackCategory) {
  if (typeof message === "string") {
    return {
      code: fallbackCode,
      category: ts.DiagnosticCategory[fallbackCategory].toLowerCase(),
      text: message,
      next: null,
    };
  }
  return {
    code: message.code,
    category: ts.DiagnosticCategory[message.category].toLowerCase(),
    text: message.messageText,
    next: message.next
      ? message.next.map((child) =>
          messageChainRecord(child, child.code, child.category),
        )
      : null,
  };
}

function diagnosticRecord(diagnostic) {
  return {
    file: diagnostic.file?.fileName ?? null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    code: diagnostic.code,
    category: ts.DiagnosticCategory[diagnostic.category].toLowerCase(),
    message: messageChainRecord(
      diagnostic.messageText,
      diagnostic.code,
      diagnostic.category,
    ),
  };
}

function buildFixture(manifest, fixtureIndex) {
  const recorded = manifest.compiler_fixtures[fixtureIndex];
  requireCondition(recorded?.source === fixtureIndex, "fixture index drifted");
  const inventory = manifest.sources[recorded.source];
  const expectedPath = EXPECTED_FIXTURES[fixtureIndex - FIRST_FIXTURE_INDEX];
  requireCondition(
    inventory?.suite === "compiler" && inventory.path === expectedPath,
    `fixture ${fixtureIndex} is no longer ${expectedPath}`,
  );
  const sourcePath = path.join(COMPILER_ROOT, inventory.path);
  const raw = fs.readFileSync(sourcePath);
  requireCondition(
    raw.length === inventory.bytes &&
      sha256(raw) === inventory.sha256 &&
      gitBlobSha1(raw) === inventory.git_blob_sha1,
    `${inventory.path} source identity changed`,
  );
  const decoded = ts.sys.readFile(sourcePath);
  requireCondition(
    typeof decoded === "string" &&
      sha256(Buffer.from(decoded, "utf8")) === recorded.decoded_sha256,
    `${inventory.path} decoded identity changed`,
  );
  const units = makeUnitsFromTest(decoded);
  requireCondition(
    units.length === recorded.normal_units.length,
    `${inventory.path} unit count changed`,
  );
  for (const [unit, expected] of units.map((unit, index) => [
    unit,
    recorded.normal_units[index],
  ])) {
    requireCondition(unit.name === expected.name, `${inventory.path} unit order changed`);
  }

  const rawOptions = {};
  for (const setting of recorded.settings) {
    const normalizedName = setting.name.toLowerCase();
    if (
      normalizedName === "noimplicitreferences" ||
      normalizedName === "filename"
    ) {
      continue;
    }
    rawOptions[setting.name] =
      setting.value === "true"
        ? true
        : setting.value === "false"
          ? false
          : setting.value;
  }
  const converted = ts.convertCompilerOptionsFromJson(rawOptions, "/");
  requireCondition(
    converted.errors.length === 0,
    `${inventory.path} options no longer convert cleanly: ${converted.errors
      .map((diagnostic) => ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"))
      .join("; ")}`,
  );
  const options = { ...converted.options, noEmit: true };
  const root = normalizeAbsolute(units.at(-1).name);
  const program = ts.createProgram([root], options, createHost(units, options));
  const defaultLibraryDirectory = `${ts.getDirectoryPath(
    ts.getDefaultLibFilePath(options),
  )}/`;
  const sources = program
    .getSourceFiles()
    .filter((source) => !source.fileName.startsWith(defaultLibraryDirectory))
    .map((source) => ({
      file: source.fileName,
      redirect_target: source.redirectInfo?.redirectTarget?.fileName ?? null,
    }));
  const diagnostics = ts.getPreEmitDiagnostics(program).map(diagnosticRecord);
  return {
    fixture_index: fixtureIndex,
    case_id: `typescript-6.0.3/compiler/${inventory.path}#default`,
    source: {
      path: inventory.path,
      sha256: inventory.sha256,
      git_blob_sha1: inventory.git_blob_sha1,
      decoded_sha256: recorded.decoded_sha256,
    },
    root,
    sources,
    diagnostics,
  };
}

function summarize(fixtures) {
  return {
    fixture_total: fixtures.length,
    source_total: fixtures.reduce(
      (total, fixture) => total + fixture.sources.length,
      0,
    ),
    redirect_total: fixtures.reduce(
      (total, fixture) =>
        total + fixture.sources.filter((source) => source.redirect_target).length,
      0,
    ),
    diagnostic_total: fixtures.reduce(
      (total, fixture) => total + fixture.diagnostics.length,
      0,
    ),
  };
}

function renderArtifact() {
  const manifest = readManifest();
  const fixtures = [];
  for (
    let fixtureIndex = FIRST_FIXTURE_INDEX;
    fixtureIndex <= LAST_FIXTURE_INDEX;
    fixtureIndex += 1
  ) {
    fixtures.push(buildFixture(manifest, fixtureIndex));
  }
  const summary = summarize(fixtures);
  requireJsonEqual(summary, EXPECTED_SUMMARY, "package redirect summary");
  return `${JSON.stringify(
    {
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
    },
    null,
    2,
  )}\n`;
}

function checkRecordedArtifact(rendered) {
  const expected = Buffer.from(rendered, "utf8");
  const recorded = fs.readFileSync(ARTIFACT_PATH);
  requireCondition(
    recorded.equals(expected),
    `${ARTIFACT_RELATIVE_PATH} is stale; regenerate from its pinned producer`,
  );
}

const arguments_ = process.argv.slice(2);
requireCondition(
  arguments_.length === 0 ||
    (arguments_.length === 1 && arguments_[0] === "--check"),
  "usage: node crates/oracle/compiler-package-redirects.mjs [--check]",
);
validateRuntime();
const typescriptModule = await import(
  pathToFileURL(TYPESCRIPT_BUNDLE_PATH).href
);
ts = typescriptModule.default;
requireCondition(
  ts.version === EXPECTED_TYPESCRIPT_VERSION,
  `vendored TypeScript reports ${ts.version}`,
);
const rendered = renderArtifact();
if (arguments_[0] === "--check") {
  checkRecordedArtifact(rendered);
  process.stdout.write(`${ARTIFACT_RELATIVE_PATH} is current\n`);
} else {
  process.stdout.write(rendered);
}
