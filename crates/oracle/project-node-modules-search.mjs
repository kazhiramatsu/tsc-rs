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
  "vendor/typescript-6.0.3/project-node-modules-search.v1.json";
const ARTIFACT_PATH = path.join(WORKSPACE, ARTIFACT_RELATIVE_PATH);
const TYPESCRIPT_BUNDLE_RELATIVE_PATH =
  "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_BUNDLE_PATH = path.join(
  WORKSPACE,
  TYPESCRIPT_BUNDLE_RELATIVE_PATH,
);
const PROJECT_RUNNER_SOURCE_PATH = "src/testRunner/projectsRunner.ts";
const PROJECT_RUNNER_GIT_BLOB = "5befdf497dff2accd67e08c3c51100b66f1b14b5";
const PROJECT_DESCRIPTOR_ROOT = path.join(
  WORKSPACE,
  "ts-tests/tests/cases/project",
);
const PROJECTS_MOUNT_ROOT = path.join(
  WORKSPACE,
  "ts-tests/tests/cases/projects",
);
const PROJECT_TREE_ROOT = path.join(PROJECTS_MOUNT_ROOT, "NodeModulesSearch");
const LIBRARY_ROOT = path.join(WORKSPACE, "vendor/typescript-6.0.3/lib");

const EXPECTED_NODE_VERSION = "25.2.1";
const EXPECTED_TYPESCRIPT_VERSION = "6.0.3";
const EXPECTED_SOURCE_COMMIT =
  "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_MANIFEST_SHA256 =
  "9c6e991103b571f7a8800dc5e1ef66088017689f3769de8fcdb408b2dc125188";
const EXPECTED_TYPESCRIPT_BUNDLE_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const EXPECTED_PROJECT_MOUNT_FILES = 233;
const DESCRIPTORS = [
  "nodeModulesImportHigher.json",
  "nodeModulesMaxDepthExceeded.json",
  "nodeModulesMaxDepthIncreased.json",
];
const MODULE_VARIANTS = [
  { name: "commonjs", value: 1, baseline_folder: "node" },
  { name: "amd", value: 2, baseline_folder: "amd" },
];
const EXPECTED_SUMMARY = {
  fixture_total: 3,
  case_total: 6,
  config_root_total: 10,
  source_file_total: 44,
  library_file_total: 18,
  pre_emit_diagnostic_total: 17,
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

function normalizeSlashes(value) {
  return value.replaceAll("\\", "/");
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

const exactPathCache = new Map();
const projectMountFiles = new Set();
const projectMountDirectories = new Map();

function projectDirectoryEntries(directory) {
  const normalized = path.normalize(directory);
  let entries = projectMountDirectories.get(normalized);
  if (entries === undefined) {
    entries = { files: new Set(), directories: new Set() };
    projectMountDirectories.set(normalized, entries);
  }
  return entries;
}

function isExactPath(root, candidate, kind) {
  const normalized = path.normalize(candidate);
  if (!isWithin(root, normalized)) return false;
  const cacheKey = `${kind}\0${normalized}`;
  const cached = exactPathCache.get(cacheKey);
  if (cached !== undefined) return cached;
  let current = root;
  const relative = path.relative(root, normalized);
  try {
    for (const component of relative.split(path.sep).filter(Boolean)) {
      if (!fs.readdirSync(current).includes(component)) {
        exactPathCache.set(cacheKey, false);
        return false;
      }
      current = path.join(current, component);
    }
    const stats = fs.statSync(current);
    const matches = kind === "file" ? stats.isFile() : stats.isDirectory();
    exactPathCache.set(cacheKey, matches);
    return matches;
  } catch {
    exactPathCache.set(cacheKey, false);
    return false;
  }
}

function admittedProjectPath(fileName, kind) {
  const physical = path.normalize(physicalProjectPath(fileName));
  const admitted =
    kind === "file"
      ? projectMountFiles.has(physical)
      : projectMountDirectories.has(physical);
  return admitted ? physical : undefined;
}

function admittedCompilerPath(fileName, kind) {
  const projectPath = admittedProjectPath(fileName, kind);
  if (projectPath !== undefined) return projectPath;
  const physical = path.isAbsolute(fileName)
    ? path.normalize(fileName)
    : physicalProjectPath(fileName);
  return isExactPath(LIBRARY_ROOT, physical, kind)
    ? physical
    : undefined;
}

function normalizeVersion(version) {
  return version.startsWith("v") ? version.slice(1) : version;
}

function requireJsonEqual(actual, expected, label) {
  requireCondition(
    JSON.stringify(actual) === JSON.stringify(expected),
    `${label} mismatch:\nactual: ${JSON.stringify(actual)}\nexpected: ${JSON.stringify(expected)}`,
  );
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
    `project oracle requires Node ${recordedNodeVersion}; running ${runningNodeVersion}`,
  );
  const bundleHash = sha256(fs.readFileSync(TYPESCRIPT_BUNDLE_PATH));
  requireCondition(
    bundleHash === EXPECTED_TYPESCRIPT_BUNDLE_SHA256,
    `${TYPESCRIPT_BUNDLE_RELATIVE_PATH} SHA-256 is ${bundleHash}; expected ${EXPECTED_TYPESCRIPT_BUNDLE_SHA256}`,
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
    "manifest TypeScript version does not match the project oracle",
  );
  requireCondition(
    manifest.source_commit === EXPECTED_SOURCE_COMMIT,
    "manifest source commit does not match the project oracle",
  );
  const projectRunner = manifest.implementation_sources.find(
    (source) => source.source_path === PROJECT_RUNNER_SOURCE_PATH,
  );
  requireCondition(
    projectRunner?.git_blob_sha1 === PROJECT_RUNNER_GIT_BLOB,
    "projectsRunner.ts Git blob does not match the project oracle",
  );
  return manifest;
}

function sourceEntry(manifest, suite, relativePath) {
  const entry = manifest.sources.find(
    (source) => source.suite === suite && source.path === relativePath,
  );
  requireCondition(entry, `manifest is missing ${suite}/${relativePath}`);
  return entry;
}

function verifyPinnedFile(fileName, entry, label) {
  const bytes = fs.readFileSync(fileName);
  requireCondition(bytes.length === entry.bytes, `${label} byte count drifted`);
  requireCondition(sha256(bytes) === entry.sha256, `${label} SHA-256 drifted`);
  requireCondition(
    gitBlobSha1(bytes) === entry.git_blob_sha1,
    `${label} Git blob drifted`,
  );
  return bytes;
}

function verifyProjectMount(manifest) {
  const entries = manifest.sources.filter((source) => source.suite === "projects");
  requireCondition(
    entries.length === EXPECTED_PROJECT_MOUNT_FILES,
    `projects mount has ${entries.length} files; expected ${EXPECTED_PROJECT_MOUNT_FILES}`,
  );
  requireCondition(
    projectMountFiles.size === 0 && projectMountDirectories.size === 0,
    "projects mount inventory must be initialized exactly once",
  );
  projectDirectoryEntries(PROJECTS_MOUNT_ROOT);
  for (const entry of entries) {
    const physical = path.normalize(path.join(PROJECTS_MOUNT_ROOT, entry.path));
    requireCondition(
      isWithin(PROJECTS_MOUNT_ROOT, physical),
      `projects mount path ${entry.path} escapes its root`,
    );
    verifyPinnedFile(
      physical,
      entry,
      `projects/${entry.path}`,
    );
    requireCondition(
      !projectMountFiles.has(physical),
      `projects mount contains duplicate path ${entry.path}`,
    );
    projectMountFiles.add(physical);
    let child = physical;
    let directory = path.dirname(child);
    projectDirectoryEntries(directory).files.add(path.basename(child));
    while (directory !== PROJECTS_MOUNT_ROOT) {
      requireCondition(
        isWithin(PROJECTS_MOUNT_ROOT, directory),
        `projects mount path ${entry.path} escapes its root`,
      );
      child = directory;
      directory = path.dirname(directory);
      projectDirectoryEntries(directory).directories.add(path.basename(child));
    }
  }
}

function readProjectDirectory(
  directory,
  extensions,
  excludes,
  includes,
  depth,
) {
  const physical = admittedProjectPath(directory, "directory");
  if (physical === undefined) return [];
  return ts.matchFiles(
    physical,
    extensions,
    excludes,
    includes,
    true,
    process.cwd(),
    depth,
    (candidate) => {
      const admitted = admittedProjectPath(candidate, "directory");
      if (admitted === undefined) return { files: [], directories: [] };
      const entries = projectMountDirectories.get(admitted);
      return {
        files: [...entries.files],
        directories: [...entries.directories],
      };
    },
    ts.identity,
  );
}

function createRunnerOptions(testCase, moduleKind) {
  const compilerOptions = {
    noErrorTruncation: false,
    skipDefaultLibCheck: false,
    moduleResolution: ts.ModuleResolutionKind.Classic,
    module: moduleKind,
    newLine: ts.NewLineKind.CarriageReturnLineFeed,
    mapRoot:
      testCase.resolveMapRoot && testCase.mapRoot
        ? path.resolve(PROJECT_TREE_ROOT, testCase.mapRoot)
        : testCase.mapRoot,
    sourceRoot:
      testCase.resolveSourceRoot && testCase.sourceRoot
        ? path.resolve(PROJECT_TREE_ROOT, testCase.sourceRoot)
        : testCase.sourceRoot,
  };
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
    compilerOptions[option.name] = value;
  }
  return compilerOptions;
}

function physicalProjectPath(fileName) {
  return path.isAbsolute(fileName)
    ? path.normalize(fileName)
    : path.resolve(PROJECT_TREE_ROOT, fileName);
}

function projectRelativePath(fileName) {
  const physical = physicalProjectPath(fileName);
  const relative = normalizeSlashes(path.relative(PROJECT_TREE_ROOT, physical));
  return relative || ".";
}

function createParseHost() {
  return {
    useCaseSensitiveFileNames: true,
    fileExists(fileName) {
      return admittedProjectPath(fileName, "file") !== undefined;
    },
    readFile(fileName) {
      const physical = admittedProjectPath(fileName, "file");
      return physical === undefined ? undefined : ts.sys.readFile(physical);
    },
    readDirectory(directory, extensions, excludes, includes, depth) {
      return readProjectDirectory(
        directory,
        extensions,
        excludes,
        includes,
        depth,
      ).map(projectRelativePath);
    },
  };
}

function createCompilerHost(options) {
  const host = ts.createCompilerHost(options, true);
  const readFile = (fileName) => {
    const physical = admittedCompilerPath(fileName, "file");
    return physical === undefined ? undefined : ts.sys.readFile(physical);
  };
  const fileExists = (fileName) =>
    admittedCompilerPath(fileName, "file") !== undefined;
  host.getCurrentDirectory = () => PROJECT_TREE_ROOT;
  host.useCaseSensitiveFileNames = () => true;
  host.getCanonicalFileName = (fileName) => fileName;
  host.getDefaultLibFileName = () => path.join(LIBRARY_ROOT, "lib.es5.d.ts");
  host.readFile = readFile;
  host.fileExists = fileExists;
  host.directoryExists = (directory) =>
    admittedCompilerPath(directory, "directory") !== undefined;
  host.getDirectories = (directory) => {
    const projectDirectory = admittedProjectPath(directory, "directory");
    if (projectDirectory !== undefined) {
      return [
        ...projectMountDirectories.get(projectDirectory).directories,
      ].map((name) => path.join(projectDirectory, name));
    }
    const physical = admittedCompilerPath(directory, "directory");
    if (physical === undefined) return [];
    return ts.sys
      .getDirectories(physical)
      .filter(
        (candidate) =>
          admittedCompilerPath(candidate, "directory") !== undefined,
      );
  };
  host.readDirectory = (directory, extensions, excludes, includes, depth) => {
    if (admittedProjectPath(directory, "directory") !== undefined) {
      return readProjectDirectory(
        directory,
        extensions,
        excludes,
        includes,
        depth,
      );
    }
    const physical = admittedCompilerPath(directory, "directory");
    if (physical === undefined) return [];
    return ts.sys
      .readDirectory(physical, extensions, excludes, includes, depth)
      .filter(
        (candidate) => admittedCompilerPath(candidate, "file") !== undefined,
      );
  };
  host.realpath = (fileName) => {
    const physical =
      admittedCompilerPath(fileName, "file") ??
      admittedCompilerPath(fileName, "directory");
    return physical === undefined ? fileName : ts.sys.realpath(physical);
  };
  host.getSourceFile = (
    fileName,
    languageVersionOrOptions,
    onError,
    shouldCreateNewSourceFile,
  ) => {
    const text = readFile(fileName);
    if (text === undefined) {
      onError?.(`Cannot read file ${fileName}`);
      return undefined;
    }
    return ts.createSourceFile(
      fileName,
      text,
      languageVersionOrOptions,
      true,
      ts.getScriptKindFromFileName(fileName),
    );
  };
  return host;
}

function observedSourceName(fileName) {
  const physical = path.isAbsolute(fileName)
    ? path.normalize(fileName)
    : physicalProjectPath(fileName);
  const libraryRelative = path.relative(LIBRARY_ROOT, physical);
  if (
    libraryRelative !== "" &&
    libraryRelative !== ".." &&
    !libraryRelative.startsWith(`..${path.sep}`) &&
    !path.isAbsolute(libraryRelative)
  ) {
    return normalizeSlashes(libraryRelative);
  }
  return projectRelativePath(physical);
}

function diagnosticRecord(diagnostic) {
  return {
    code: diagnostic.code,
    category: diagnostic.category,
    file: diagnostic.file ? observedSourceName(diagnostic.file.fileName) : null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    message: ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
  };
}

function optionNumber(value) {
  return value === undefined ? null : value;
}

function buildCase(manifest, descriptorName, moduleVariant) {
  const descriptorEntry = sourceEntry(manifest, "project", descriptorName);
  const descriptorBytes = verifyPinnedFile(
    path.join(PROJECT_DESCRIPTOR_ROOT, descriptorName),
    descriptorEntry,
    `project/${descriptorName}`,
  );
  const testCase = JSON.parse(descriptorBytes.toString("utf8"));
  requireCondition(
    testCase.projectRoot === "tests/cases/projects/NodeModulesSearch",
    `${descriptorName} projectRoot drifted`,
  );
  const existingOptions = createRunnerOptions(testCase, moduleVariant.value);
  requireCondition(
    typeof existingOptions.project === "string" && existingOptions.project !== "",
    `${descriptorName} must select an explicit project config`,
  );
  const configFileName = ts.normalizePath(
    ts.combinePaths(existingOptions.project, "tsconfig.json"),
  );
  const configRelativePath = `NodeModulesSearch/${configFileName}`;
  const configEntry = sourceEntry(manifest, "projects", configRelativePath);
  verifyPinnedFile(
    physicalProjectPath(configFileName),
    configEntry,
    `projects/${configRelativePath}`,
  );
  const parseHost = createParseHost();
  const configSource = ts.readJsonConfigFile(configFileName, parseHost.readFile);
  const parsed = ts.parseJsonSourceFileConfigFileContent(
    configSource,
    parseHost,
    ts.getDirectoryPath(configFileName),
    existingOptions,
  );
  requireCondition(
    configSource.parseDiagnostics.length === 0 && parsed.errors.length === 0,
    `${descriptorName}/${moduleVariant.name} config produced diagnostics`,
  );

  const rawNoEmit = parsed.options.noEmit ?? null;
  parsed.options.noEmit = true;
  const compilerHost = createCompilerHost(parsed.options);
  const program = ts.createProgram(parsed.fileNames, parsed.options, compilerHost);
  const preEmitDiagnostics = ts.getPreEmitDiagnostics(program).map(diagnosticRecord);
  const sourceFiles = program.getSourceFiles().map(({ fileName }) =>
    observedSourceName(fileName),
  );
  const libraryFiles = program
    .getSourceFiles()
    .filter((sourceFile) => program.isSourceFileDefaultLibrary(sourceFile))
    .map(({ fileName }) => observedSourceName(fileName));
  const caseId = `typescript-6.0.3/project/${descriptorName}#module%3D${moduleVariant.name}`;
  const manifestCase = manifest.cases.find((entry) => entry.id === caseId);
  requireCondition(manifestCase, `manifest is missing ${caseId}`);
  requireCondition(
    manifestCase.initial_execution_state === "not-run",
    `${caseId} must remain not-run`,
  );
  requireCondition(
    manifestCase.configuration.baseline_folder === moduleVariant.baseline_folder,
    `${caseId} baseline folder drifted`,
  );
  return {
    case_id: caseId,
    initial_execution_state: manifestCase.initial_execution_state,
    descriptor: {
      path: descriptorName,
      sha256: descriptorEntry.sha256,
      git_blob_sha1: descriptorEntry.git_blob_sha1,
    },
    module: moduleVariant,
    current_directory: "/.src/tests/cases/projects/NodeModulesSearch",
    config: {
      path: configFileName,
      sha256: configEntry.sha256,
      git_blob_sha1: configEntry.git_blob_sha1,
      root_names: parsed.fileNames.map(projectRelativePath),
      diagnostics: [],
    },
    effective_options: {
      allow_js: parsed.options.allowJs ?? false,
      max_node_module_js_depth: optionNumber(
        parsed.options.maxNodeModuleJsDepth,
      ),
      module: parsed.options.module ?? null,
      module_resolution: parsed.options.moduleResolution ?? null,
      declaration: parsed.options.declaration ?? null,
      no_error_truncation: parsed.options.noErrorTruncation ?? null,
      skip_default_lib_check: parsed.options.skipDefaultLibCheck ?? null,
      out_dir:
        parsed.options.outDir === undefined
          ? null
          : projectRelativePath(parsed.options.outDir),
      config_file_path: parsed.options.configFilePath ?? null,
      raw_no_emit: rawNoEmit,
      execution_no_emit_adapter: true,
      host_default_library: "lib.es5.d.ts",
    },
    source_files: sourceFiles,
    library_files: libraryFiles,
    pre_emit_diagnostics: preEmitDiagnostics,
  };
}

function summarize(cases) {
  return {
    fixture_total: new Set(cases.map((entry) => entry.descriptor.path)).size,
    case_total: cases.length,
    config_root_total: cases.reduce(
      (total, entry) => total + entry.config.root_names.length,
      0,
    ),
    source_file_total: cases.reduce(
      (total, entry) => total + entry.source_files.length,
      0,
    ),
    library_file_total: cases.reduce(
      (total, entry) => total + entry.library_files.length,
      0,
    ),
    pre_emit_diagnostic_total: cases.reduce(
      (total, entry) => total + entry.pre_emit_diagnostics.length,
      0,
    ),
  };
}

function generateArtifact() {
  const manifest = readManifest();
  verifyProjectMount(manifest);
  const cases = [];
  for (const descriptorName of DESCRIPTORS) {
    for (const moduleVariant of MODULE_VARIANTS) {
      cases.push(buildCase(manifest, descriptorName, moduleVariant));
    }
  }
  const summary = summarize(cases);
  requireJsonEqual(summary, EXPECTED_SUMMARY, "project oracle summary");
  return {
    schema: 1,
    typescript_version: EXPECTED_TYPESCRIPT_VERSION,
    source_commit: EXPECTED_SOURCE_COMMIT,
    node_version: EXPECTED_NODE_VERSION,
    producer: {
      path: TYPESCRIPT_BUNDLE_RELATIVE_PATH,
      sha256: EXPECTED_TYPESCRIPT_BUNDLE_SHA256,
    },
    project_runner: {
      path: PROJECT_RUNNER_SOURCE_PATH,
      git_blob_sha1: PROJECT_RUNNER_GIT_BLOB,
    },
    manifest: {
      path: MANIFEST_RELATIVE_PATH,
      sha256: EXPECTED_MANIFEST_SHA256,
    },
    scope: {
      no_emit_adapter: true,
      emit_and_baselines: "not-run",
    },
    summary,
    cases,
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
        `regenerate with: node crates/oracle/project-node-modules-search.mjs > ${ARTIFACT_RELATIVE_PATH}`,
    );
  }
}

const arguments_ = process.argv.slice(2);
requireCondition(
  arguments_.length === 0 ||
    (arguments_.length === 1 && arguments_[0] === "--check"),
  "usage: node crates/oracle/project-node-modules-search.mjs [--check]",
);
validateRuntime();
const typescriptModule = await import(pathToFileURL(TYPESCRIPT_BUNDLE_PATH).href);
ts = typescriptModule.default;
requireCondition(
  ts?.version === EXPECTED_TYPESCRIPT_VERSION,
  `vendored TypeScript reports ${ts?.version}; expected ${EXPECTED_TYPESCRIPT_VERSION}`,
);
const rendered = renderArtifact();
if (arguments_[0] === "--check") {
  checkRecordedArtifact(rendered);
  process.stdout.write(`${ARTIFACT_RELATIVE_PATH} is current\n`);
} else {
  process.stdout.write(rendered);
}
