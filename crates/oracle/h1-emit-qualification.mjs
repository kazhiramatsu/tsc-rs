import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import { serializeMessageChainBounded } from "./m9-observation.mjs";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h1-emit-qualification.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h1-emit-qualification.v1.json";
const TARGET_PATH = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h1-emit-qualification.schema.json";
const SOURCE_ROOT = path.join(WORKSPACE, "ts-tests/tests/cases/compiler");
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const EXPANSION = "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const PROFILE = "ratchets/h1-emit-profile.v1.json";
const CALLBACK_ORACLE = "ratchets/h1-emit-oracle.v1.json";
const OWNER_INVENTORY = "ratchets/h1-owner-inventory.v1.json";
const RUST_OMISSIONS = "ratchets/h1-rust-omissions.v1.json";
const NOEMIT_PERFORMANCE = "ratchets/h1-noemit-performance.v1.json";
const EMIT_PERFORMANCE = "ratchets/h1-emit-performance.v1.json";
const CLASSIFICATIONS = Object.freeze([
  {
    suite: "compiler",
    path: "vendor/typescript-6.0.3/compiler-profile-classification.v1.json",
    cases: 7_276,
    admitted: 1,
  },
  {
    suite: "conformance",
    path: "vendor/typescript-6.0.3/conformance-profile-classification.v1.json",
    cases: 7_697,
    admitted: 0,
  },
  {
    suite: "project",
    path: "vendor/typescript-6.0.3/project-profile-classification.v1.json",
    cases: 632,
    admitted: 0,
  },
  {
    suite: "transpile",
    path: "vendor/typescript-6.0.3/transpile-suite-inventory.v1.json",
    cases: 37,
    admitted: 0,
  },
  {
    suite: "fourslash",
    path: "vendor/typescript-6.0.3/fourslash-whole-program-equivalence.v1.json",
    cases: 38,
    admitted: 0,
  },
]);

const EXPECTED_NODE = "25.2.1";
const EXPECTED_TYPESCRIPT = "6.0.3";
const EXPECTED_SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const CASE_ID =
  "typescript-6.0.3/compiler/esmNoSynthesizedDefault.ts#module%3Dpreserve";
const VIRTUAL_CURRENT_DIRECTORY = "/.src";
const OPTION_LINE_PATTERN = /^\/{2}\s*@(\w+)\s*:\s*([^\r\n]*)/;
const LINK_LINE_PATTERN =
  /^\/{2}\s*@link\s*:\s*([^\r\n]*)\s*->\s*([^\r\n]*)/;
const UTF8_BOM = Buffer.from([0xef, 0xbb, 0xbf]);

const CONTROL_FAILURES = Object.freeze({
  "runtime-enum-control": {
    kind: "unsupported-transform-feature",
    feature: "runtime-enum",
  },
  "runtime-namespace-control": {
    kind: "unsupported-transform-feature",
    feature: "runtime-namespace",
  },
  "parameter-property-control": {
    kind: "unsupported-transform-feature",
    feature: "parameter-property",
  },
  "jsx-control": {
    kind: "unsupported-compiler-option",
    option: "jsx",
  },
  "source-map-control": {
    kind: "unsupported-compiler-option",
    option: "sourceMap",
  },
  "declaration-control": {
    kind: "unsupported-compiler-option",
    option: "declaration",
  },
  "mts-output-control": {
    kind: "unsupported-source-extension",
    extension: ".mts",
  },
});

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

function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function readJson(relativePath) {
  return JSON.parse(readBytes(relativePath).toString("utf8"));
}

function pathHash(relativePath) {
  return { path: relativePath, sha256: sha256(readBytes(relativePath)) };
}

function exactKeys(value, required, optional = []) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const allowed = new Set([...required, ...optional]);
  return (
    required.every((key) => Object.hasOwn(value, key)) &&
    Object.keys(value).every((key) => allowed.has(key))
  );
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

function requireJsonEqual(actual, expected, label) {
  requireCondition(canonical(actual) === canonical(expected), `${label} changed`);
}

function validateRuntime() {
  const node = fs.readFileSync(path.join(WORKSPACE, ".node-version"), "utf8").trim();
  requireCondition(node === EXPECTED_NODE, "unexpected Node pin");
  requireCondition(process.version === `v${node}`, `requires Node ${node}`);
  requireCondition(ts.version === EXPECTED_TYPESCRIPT, "unexpected TypeScript runtime");
  requireCondition(
    typeof ts.emitFilesAndReportErrorsAndGetExitStatus === "function",
    "vendored emit/report orchestration export is unavailable",
  );
}

function safeCompilerSource(relativePath) {
  requireCondition(
    relativePath.length > 0 &&
      !path.posix.isAbsolute(relativePath) &&
      !relativePath.split("/").includes(".."),
    `unsafe compiler source path ${JSON.stringify(relativePath)}`,
  );
  const absolute = path.resolve(SOURCE_ROOT, ...relativePath.split("/"));
  requireCondition(
    absolute.startsWith(`${path.resolve(SOURCE_ROOT)}${path.sep}`),
    `compiler source escaped suite root: ${relativePath}`,
  );
  return absolute;
}

function orderedSettings(object) {
  return Object.entries(object).map(([name, value]) => ({ name, value }));
}

// CompilerBaselineRunner's makeUnits contract. This intentionally retains
// its metadata association and LF reconstruction instead of inventing a new
// H1 fixture format.
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
  const bytes = Buffer.from(text, "utf8");
  return { state: "present", utf8_bytes: bytes.length, sha256: sha256(bytes) };
}

function documentSymlinks(fileOptions) {
  const setting = fileOptions.find((entry) => entry.name === "symlink");
  if (!setting || setting.value === "") return [];
  return setting.value.split(",").map((entry) => entry.trim());
}

function loadCandidate(expansion, classification) {
  const admitted = classification.cases.filter(
    (entry) => entry.bootstrap_profile_admitted,
  );
  requireCondition(admitted.length === 1, "compiler admission count changed");
  const row = admitted[0];
  requireCondition(row.id === CASE_ID, "compatible compiler case changed");
  const fixture = expansion.compiler_fixtures[row.source];
  const source = expansion.sources[row.source];
  requireCondition(source?.suite === "compiler", "candidate source is absent");
  const sourcePath = safeCompilerSource(source.path);
  const raw = fs.readFileSync(sourcePath);
  requireCondition(
    raw.length === source.bytes &&
      sha256(raw) === source.sha256 &&
      gitBlobSha1(raw) === source.git_blob_sha1,
    "candidate source identity changed",
  );
  const decoded = ts.sys.readFile(sourcePath);
  requireCondition(typeof decoded === "string", "candidate source cannot be decoded");
  requireCondition(
    Buffer.byteLength(decoded, "utf8") === fixture.decoded_utf8_bytes &&
      sha256(Buffer.from(decoded, "utf8")) === fixture.decoded_sha256,
    "candidate decoded identity changed",
  );
  const parsed = makeUnits(decoded, source.path);
  requireCondition(fixture.virtual_config === null, "candidate acquired a virtual config");
  requireCondition(parsed.links.length === 0 && fixture.links.length === 0, "candidate links changed");
  requireCondition(
    parsed.units.length === fixture.normal_units.length,
    "candidate unit count changed",
  );
  for (const [index, unit] of parsed.units.entries()) {
    const expected = fixture.normal_units[index];
    requireCondition(unit.name === expected.name, `candidate unit ${index} name changed`);
    requireJsonEqual(unit.file_options, expected.file_options, `candidate unit ${index} options`);
    requireJsonEqual(contentIdentity(unit.text), expected.content, `candidate unit ${index} content`);
    requireJsonEqual(
      documentSymlinks(unit.file_options),
      expected.document_symlinks,
      `candidate unit ${index} symlinks`,
    );
  }
  const configuration = fixture.configurations[row.configuration];
  requireCondition(configuration?.variant === "module=preserve", "candidate variant changed");
  return { row, fixture, source, units: parsed.units, configuration };
}

const optionIndex = new Map(
  ts.optionDeclarations.map((option) => [option.name.toLowerCase(), option]),
);

function optionValue(option, raw) {
  const diagnostics = [];
  let value;
  if (option.type === "boolean") value = raw.toLowerCase() === "true";
  else if (option.type === "string") value = raw;
  else if (option.type === "number") value = Number.parseInt(raw, 10);
  else if (option.type === "list" || option.type === "listOrElement") {
    value = ts.parseListTypeOption(option, raw, diagnostics);
  } else value = ts.parseCustomTypeOption(option, raw, diagnostics);
  requireCondition(diagnostics.length === 0, `invalid @${option.name}: ${raw}`);
  return value;
}

function effectiveOptions(fixture, configuration) {
  const settings = new Map(fixture.settings.map((entry) => [entry.name, entry.value]));
  for (const entry of configuration.settings) settings.set(entry.name, entry.value);
  const options = {};
  for (const [name, raw] of settings) {
    const option = optionIndex.get(name.toLowerCase());
    if (option) options[option.name] = optionValue(option, raw);
  }
  // Exact Compiler.compileFiles harness defaults, after directive/config
  // projection. These differ from production command-line host defaults.
  options.newLine = options.newLine || ts.NewLineKind.CarriageReturnLineFeed;
  options.noErrorTruncation = true;
  options.skipDefaultLibCheck =
    options.skipDefaultLibCheck === undefined ? true : options.skipDefaultLibCheck;
  return options;
}

function hasDirectory(files, directory) {
  const normalized = ts.normalizePath(directory).replace(/\/$/u, "");
  return [...files.keys()].some((fileName) => fileName.startsWith(`${normalized}/`));
}

function createProgram(candidate, options) {
  const files = new Map();
  for (const unit of candidate.units) {
    const fileName = ts.getNormalizedAbsolutePath(unit.name, VIRTUAL_CURRENT_DIRECTORY);
    files.set(fileName, unit.text);
  }
  const base = ts.createCompilerHost(options, true);
  const host = {
    ...base,
    getCurrentDirectory: () => VIRTUAL_CURRENT_DIRECTORY,
    useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: (fileName) => fileName,
    fileExists(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.has(normalized) || base.fileExists(normalized);
    },
    readFile(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.get(normalized) ?? base.readFile(normalized);
    },
    directoryExists(directory) {
      return hasDirectory(files, directory) || (base.directoryExists?.(directory) ?? false);
    },
    getDirectories(directory) {
      return base.getDirectories?.(directory) ?? [];
    },
    realpath(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.has(normalized)
        ? normalized
        : (base.realpath?.(normalized) ?? normalized);
    },
    getSourceFile(fileName, languageVersion) {
      const normalized = ts.normalizePath(fileName);
      const text = files.get(normalized);
      if (text === undefined) return base.getSourceFile(fileName, languageVersion);
      return ts.createSourceFile(
        normalized,
        text,
        languageVersion,
        true,
        ts.getScriptKindFromFileName(normalized),
      );
    },
    writeFile() {
      fail("candidate host writeFile reached without callback capture");
    },
  };
  const roots = candidate.units
    .filter((unit) => !ts.fileExtensionIs(unit.name, ts.Extension.Json))
    .map((unit) => ts.getNormalizedAbsolutePath(unit.name, VIRTUAL_CURRENT_DIRECTORY));
  return { program: ts.createProgram({ rootNames: roots, options, host }), roots };
}

function normalizePath(fileName) {
  return ts.normalizePath(fileName);
}

function optionalValue(value) {
  return value === undefined
    ? { present: false, value: null }
    : { present: true, value };
}

function diagnosticCategory(category, context) {
  const names = ["warning", "error", "suggestion", "message"];
  requireCondition(
    Number.isInteger(category) && category >= 0 && category < names.length,
    `${context} has unknown diagnostic category`,
  );
  return names[category];
}

function serializeRelatedDiagnostic(diagnostic, context) {
  return {
    file: optionalValue(diagnostic.file?.fileName && normalizePath(diagnostic.file.fileName)),
    start: optionalValue(diagnostic.start),
    length: optionalValue(diagnostic.length),
    code: diagnostic.code,
    category: diagnosticCategory(diagnostic.category, context),
    chain: serializeMessageChainBounded(
      diagnostic.messageText,
      diagnostic.code,
      diagnostic.category,
      `${context}.chain`,
    ),
  };
}

function serializeDiagnostic(diagnostic, context) {
  const start = optionalValue(diagnostic.start);
  const length = optionalValue(diagnostic.length);
  requireCondition(start.present === length.present, `${context} span presence differs`);
  let line = optionalValue(undefined);
  let column = optionalValue(undefined);
  if (diagnostic.file && diagnostic.start !== undefined) {
    const location = diagnostic.file.getLineAndCharacterOfPosition(diagnostic.start);
    line = optionalValue(location.line);
    column = optionalValue(location.character);
  }
  const related = diagnostic.relatedInformation;
  return {
    file: optionalValue(diagnostic.file?.fileName && normalizePath(diagnostic.file.fileName)),
    start,
    length,
    line,
    column,
    code: diagnostic.code,
    category: diagnosticCategory(diagnostic.category, context),
    chain: serializeMessageChainBounded(
      diagnostic.messageText,
      diagnostic.code,
      diagnostic.category,
      `${context}.chain`,
    ),
    related_information_present: related !== undefined,
    related: (related ?? []).map((entry, index) =>
      serializeRelatedDiagnostic(entry, `${context}.related[${index}]`),
    ),
    reports_unnecessary: optionalValue(diagnostic.reportsUnnecessary),
    reports_deprecated: optionalValue(diagnostic.reportsDeprecated),
    source: optionalValue(diagnostic.source),
  };
}

function outputKind(fileName) {
  if ([".js", ".jsx", ".mjs", ".cjs"].some((extension) => fileName.endsWith(extension))) {
    return "javascript";
  }
  if (fileName.endsWith(".map")) return "source-map";
  if (fileName.endsWith(".d.ts")) return "declaration";
  if (fileName.endsWith(".tsbuildinfo")) return "build-info";
  fail(`unknown output kind for ${fileName}`);
}

function normalizeMetadata(data, context) {
  if (data === undefined) return { present: false, value: null };
  requireCondition(
    data !== null && typeof data === "object" && !Array.isArray(data),
    `${context} metadata is invalid`,
  );
  const known = new Set([
    "sourceMapUrlPos",
    "diagnostics",
    "skippedDtsWrite",
    "differsOnlyInMap",
    "buildInfo",
  ]);
  const ownKeys = Reflect.ownKeys(data);
  requireCondition(
    ownKeys.every((key) => typeof key === "string" && known.has(key)),
    `${context} metadata key is unknown`,
  );
  const diagnostics = data.diagnostics;
  requireCondition(
    diagnostics === undefined || Array.isArray(diagnostics),
    `${context}.diagnostics is invalid`,
  );
  return {
    present: true,
    value: {
      own_keys: ownKeys.map(String).sort(),
      source_map_url_position_utf16: optionalValue(data.sourceMapUrlPos),
      diagnostics_present: diagnostics !== undefined,
      diagnostics: (diagnostics ?? []).map((entry, index) =>
        serializeDiagnostic(entry, `${context}.diagnostics[${index}]`),
      ),
      skipped_dts_write: optionalValue(data.skippedDtsWrite),
      differs_only_in_map: optionalValue(data.differsOnlyInMap),
      build_info: optionalValue(
        data.buildInfo === undefined
          ? undefined
          : JSON.parse(JSON.stringify(data.buildInfo)),
      ),
    },
  };
}

function serializeWrite(arguments_, index) {
  const [fileName, text, bom, onError, sourceFiles, data] = arguments_;
  requireCondition(typeof fileName === "string" && typeof text === "string", "invalid write");
  requireCondition(typeof bom === "boolean", "write has no BOM decision");
  requireCondition(onError === undefined || typeof onError === "function", "invalid onError");
  requireCondition(sourceFiles === undefined || Array.isArray(sourceFiles), "invalid sources");
  const callbackBytes = Buffer.from(text, "utf8");
  const materialized = bom
    ? Buffer.concat([UTF8_BOM, callbackBytes])
    : callbackBytes;
  return {
    index,
    path: normalizePath(fileName),
    kind: outputKind(fileName),
    callback_text: text,
    callback_utf8_base64: callbackBytes.toString("base64"),
    callback_utf8_sha256: sha256(callbackBytes),
    callback_utf8_bytes: callbackBytes.length,
    write_byte_order_mark: bom,
    materialized_utf8_base64: materialized.toString("base64"),
    materialized_utf8_sha256: sha256(materialized),
    materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined,
    source_files_present: sourceFiles !== undefined,
    source_files: (sourceFiles ?? []).map((source) => normalizePath(source.fileName)),
    metadata: normalizeMetadata(data, `writes[${index}]`),
    sink_disposition: "written",
  };
}

function observeCandidate(candidate, options) {
  const { program, roots } = createProgram(candidate, options);
  const writes = [];
  const reported = [];
  const statusWrites = [];
  let emitResult;
  const originalEmit = program.emit;
  program.emit = function captureEmitResult(...arguments_) {
    requireCondition(emitResult === undefined, "candidate emitted more than once");
    emitResult = originalEmit.apply(this, arguments_);
    return emitResult;
  };
  const exitStatus = ts.emitFilesAndReportErrorsAndGetExitStatus(
    program,
    (diagnostic) => reported.push(diagnostic),
    (text) => statusWrites.push(text),
    undefined,
    (...arguments_) => writes.push(arguments_),
  );
  requireCondition(emitResult !== undefined, "candidate did not emit");
  return {
    roots,
    observation: {
      writes: writes.map(serializeWrite),
      reported_diagnostics: reported.map((diagnostic, index) =>
        serializeDiagnostic(diagnostic, `reported_diagnostics[${index}]`),
      ),
      emit_result: {
        emit_skipped: emitResult.emitSkipped,
        emit_diagnostics: emitResult.diagnostics.map((diagnostic, index) =>
          serializeDiagnostic(diagnostic, `emit_diagnostics[${index}]`),
        ),
        emitted_files_present: emitResult.emittedFiles !== undefined,
        emitted_files: (emitResult.emittedFiles ?? []).map(normalizePath),
        source_maps_present: emitResult.sourceMaps !== undefined,
        source_maps: emitResult.sourceMaps ?? [],
      },
      status_writes: statusWrites,
      process_exit: {
        code: exitStatus,
        name: [
          "success",
          "diagnostics-present-outputs-skipped",
          "diagnostics-present-outputs-generated",
        ][exitStatus],
      },
    },
  };
}

function suiteRows() {
  return CLASSIFICATIONS.map((expected) => {
    const artifact = readJson(expected.path);
    const cases = artifact.summary.cases ?? artifact.summary.fixtures;
    const admitted =
      artifact.summary.bootstrap_profile_admitted_cases ??
      artifact.summary.promotion_candidates ??
      0;
    requireCondition(cases === expected.cases, `${expected.suite} case count changed`);
    requireCondition(admitted === expected.admitted, `${expected.suite} admissions changed`);
    requireCondition(
      artifact.summary.not_run_cases === cases,
      `${expected.suite} classification is no longer the immutable not-run inventory`,
    );
    return {
      suite: expected.suite,
      classification: pathHash(expected.path),
      cases,
      compatible_cases: admitted,
      executed_cases: admitted,
      exact_observations: admitted,
    };
  });
}

function controlRows(callbackOracle) {
  const controls = callbackOracle.cases.filter(
    (entry) => entry.input.classification === "adjacent-unsupported",
  );
  requireCondition(controls.length === 7, "adjacent control count changed");
  return controls.map((entry) => {
    const id = entry.input.id;
    const expected = CONTROL_FAILURES[id];
    requireCondition(expected !== undefined, `control ${id} has no Rust disposition`);
    requireCondition(
      entry.observation.rust_expectation.outcome ===
        "typed-unsupported-before-first-write",
      `control ${id} oracle expectation changed`,
    );
    return {
      id,
      oracle_write_count: entry.observation.writes.length,
      expected_rust_failure: expected,
      expected_rust_sink_writes: 0,
    };
  });
}

function buildArtifact() {
  const expansion = readJson(EXPANSION);
  const compilerClassification = readJson(CLASSIFICATIONS[0].path);
  const profile = readJson(PROFILE);
  const callbackOracle = readJson(CALLBACK_ORACLE);
  const owner = readJson(OWNER_INVENTORY);
  const omissions = readJson(RUST_OMISSIONS);
  const noemitPerformance = readJson(NOEMIT_PERFORMANCE);
  const emitPerformance = readJson(EMIT_PERFORMANCE);
  requireCondition(
    profile.status === "frozen" && profile.phase === "H1.0a-bootstrap-profile",
    "H1 profile changed",
  );
  requireCondition(
    owner.pending_h1_0a.length === 0 && owner.summary.undispositioned_calls === 0,
    "owner inventory is open",
  );
  requireCondition(
    omissions.summary.production_boundary_omissions === 0 &&
      omissions.summary.option_projection_omissions === 0,
    "current Rust production boundary is incomplete",
  );
  requireCondition(noemitPerformance.status === "qualified", "no-emit resource guard failed");
  requireCondition(
    emitPerformance.kind === "h1-emit-performance" &&
      emitPerformance.phase === "H1.6" &&
      emitPerformance.status === "qualified" &&
      emitPerformance.qualified === true,
    "emit resource guard failed",
  );

  const candidate = loadCandidate(expansion, compilerClassification);
  const options = effectiveOptions(candidate.fixture, candidate.configuration);
  requireCondition(
    options.target === ts.ScriptTarget.ESNext &&
      options.module === ts.ModuleKind.Preserve &&
      options.moduleResolution === ts.ModuleResolutionKind.Bundler &&
      options.newLine === ts.NewLineKind.CarriageReturnLineFeed,
    "candidate effective options changed",
  );
  const observed = observeCandidate(candidate, options);
  requireCondition(observed.observation.writes.length === 1, "candidate write count changed");
  requireCondition(
    observed.observation.reported_diagnostics.map((entry) => entry.code).join(",") ===
      "1192,2339",
    "candidate diagnostic set changed",
  );
  requireCondition(
    observed.observation.process_exit.code === 2 &&
      observed.observation.emit_result.emit_skipped === false,
    "candidate exit/emit result changed",
  );

  const suites = suiteRows();
  const totalCases = suites.reduce((sum, entry) => sum + entry.cases, 0);
  const compatibleCases = suites.reduce(
    (sum, entry) => sum + entry.compatible_cases,
    0,
  );
  const admittedOracleCases = callbackOracle.cases.filter(
    (entry) => entry.input.classification === "admitted",
  );
  requireCondition(admittedOracleCases.length === 5, "frozen oracle admission count changed");
  const frozenWrites = admittedOracleCases.flatMap(
    (entry) => entry.observation.writes,
  );
  const candidateWrites = observed.observation.writes;
  requireCondition(
    emitPerformance.workload.case_id === candidate.row.id &&
      emitPerformance.candidate_summary.output_files === candidateWrites.length &&
      emitPerformance.candidate_summary.output_utf8_bytes ===
        candidateWrites.reduce(
          (sum, write) => sum + write.materialized_utf8_bytes,
          0,
        ) &&
      emitPerformance.candidate_summary.output_sha256 ===
        candidateWrites[0].materialized_utf8_sha256,
    "emit performance workload changed",
  );
  const allWrites = [...frozenWrites, ...candidateWrites];
  const cliConfig = `${JSON.stringify(
    {
      compilerOptions: {
        target: "esnext",
        module: "preserve",
        moduleResolution: "bundler",
        newLine: "crlf",
        noErrorTruncation: true,
      },
      files: ["index.ts"],
    },
    null,
    2,
  )}\n`;

  return {
    schema: 1,
    kind: "h1-emit-qualification",
    status: "qualified",
    phase: "H1.6",
    typescript: {
      version: EXPECTED_TYPESCRIPT,
      source_repository: "https://github.com/microsoft/TypeScript.git",
      source_commit: EXPECTED_SOURCE_COMMIT,
    },
    generator: pathHash(GENERATOR_RELATIVE_PATH),
    contract: pathHash(CONTRACT_RELATIVE_PATH),
    authorities: {
      profile: pathHash(PROFILE),
      callback_oracle: pathHash(CALLBACK_ORACLE),
      owner_inventory: pathHash(OWNER_INVENTORY),
      rust_omissions: pathHash(RUST_OMISSIONS),
      suite_expansion: pathHash(EXPANSION),
      typescript_bundle: pathHash(TYPESCRIPT_BUNDLE),
      typescript_implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
    },
    owner_closure: {
      inventory_status: owner.status,
      active_roots: owner.summary.active_roots,
      declarations: owner.summary.closure_declarations,
      edges: owner.summary.static_edges,
      reviewed_call_sites: owner.summary.reviewed_call_sites,
      unresolved_or_undispositioned: owner.summary.undispositioned_calls,
      pending_rows: owner.pending_h1_0a.length,
      production_boundary_omissions:
        omissions.summary.production_boundary_omissions,
      option_projection_omissions: omissions.summary.option_projection_omissions,
      explicit_checker_emit_elisions:
        omissions.summary.explicit_checker_emit_elisions,
    },
    upstream_closure: {
      suites,
      total_cases: totalCases,
      compatible_cases: compatibleCases,
      executed_cases: compatibleCases,
      exact_observations: compatibleCases,
      unexecuted_compatible_cases: 0,
    },
    compatible_cases: [
      {
        id: candidate.row.id,
        suite: "compiler",
        expansion_case: candidate.row.expansion_case,
        source: {
          path: `ts-tests/tests/cases/compiler/${candidate.source.path}`,
          bytes: candidate.source.bytes,
          sha256: candidate.source.sha256,
          git_blob_sha1: candidate.source.git_blob_sha1,
        },
        configuration: {
          index: candidate.row.configuration,
          variant: candidate.configuration.variant,
          description: candidate.configuration.description,
          upstream_name: candidate.configuration.upstream_name,
        },
        effective_options: {
          target: options.target,
          module: options.module,
          module_resolution: options.moduleResolution,
          new_line: options.newLine,
          no_error_truncation: options.noErrorTruncation,
          skip_default_lib_check: options.skipDefaultLibCheck,
        },
        roots: observed.roots,
        virtual_files: candidate.units.map((unit) => {
          const bytes = Buffer.from(unit.text, "utf8");
          return {
            path: normalizePath(
              ts.getNormalizedAbsolutePath(unit.name, VIRTUAL_CURRENT_DIRECTORY),
            ),
            utf8_base64: bytes.toString("base64"),
            utf8_sha256: sha256(bytes),
            utf8_bytes: bytes.length,
          };
        }),
        observation: observed.observation,
        cli_projection: {
          config_utf8_base64: Buffer.from(cliConfig, "utf8").toString("base64"),
          config_utf8_sha256: sha256(Buffer.from(cliConfig, "utf8")),
          config_utf8_bytes: Buffer.byteLength(cliConfig, "utf8"),
          expected_exit_code: observed.observation.process_exit.code,
          expected_output_paths: candidateWrites.map((write) => write.path),
          expected_diagnostic_codes: observed.observation.reported_diagnostics.map(
            (diagnostic) => diagnostic.code,
          ),
        },
      },
    ],
    adjacent_controls: controlRows(callbackOracle),
    output_summary: {
      frozen_oracle_admitted_cases: admittedOracleCases.length,
      compatible_upstream_cases: compatibleCases,
      total_qualified_cases: admittedOracleCases.length + compatibleCases,
      callback_writes: allWrites.length,
      callback_utf8_bytes: allWrites.reduce(
        (sum, write) => sum + write.callback_utf8_bytes,
        0,
      ),
      materialized_utf8_bytes: allWrites.reduce(
        (sum, write) => sum + write.materialized_utf8_bytes,
        0,
      ),
      ordered_outputs_sha256: sha256(
        Buffer.from(
          canonical(
            allWrites.map((write) => ({
              path: write.path,
              sha256: write.materialized_utf8_sha256,
            })),
          ),
          "utf8",
        ),
      ),
    },
    resource_summary: {
      no_emit_performance: {
        artifact: pathHash(NOEMIT_PERFORMANCE),
        candidate_commit: noemitPerformance.candidate.commit,
        status: noemitPerformance.status,
        maximum_warm_median_wall_ratio: Math.max(
          ...noemitPerformance.workloads.map(
            (workload) => workload.ratios.warm_median_wall_ratio,
          ),
        ),
        maximum_peak_rss_ratio: Math.max(
          ...noemitPerformance.workloads.map((workload) => workload.ratios.peak_rss_ratio),
        ),
        executable_size_ratio: noemitPerformance.binary_size.ratio,
        zero_activity_canaries: noemitPerformance.workloads.every((workload) =>
          Object.values(workload.candidate_summary.h1_no_emit).every((value) => value === 0),
        ),
      },
      emit_performance: {
        artifact: pathHash(EMIT_PERFORMANCE),
        base_commit: emitPerformance.base.commit,
        candidate_commit: emitPerformance.candidate.commit,
        status: emitPerformance.status,
        warm_median_wall_ratio: emitPerformance.ratios.warm_median_wall_ratio,
        warm_p95_wall_ratio: emitPerformance.ratios.warm_p95_wall_ratio,
        peak_rss_ratio: emitPerformance.ratios.peak_rss_ratio,
        executable_size_ratio: emitPerformance.binary_size.ratio,
        peak_rss_bytes: emitPerformance.candidate_summary.peak_rss_bytes,
        output_utf8_bytes: emitPerformance.candidate_summary.output_utf8_bytes,
        output_sha256: emitPerformance.candidate_summary.output_sha256,
      },
      emit_workload: {
        case_id: candidate.row.id,
        source_files: candidate.units.length,
        source_utf8_bytes: candidate.units.reduce(
          (sum, unit) => sum + Buffer.byteLength(unit.text, "utf8"),
          0,
        ),
        output_files: candidateWrites.length,
        output_utf8_bytes: candidateWrites.reduce(
          (sum, write) => sum + write.materialized_utf8_bytes,
          0,
        ),
        deterministic_repetitions: 2,
        legal_worker_counts: [1, 2],
        local_max_rss_bytes: 268_435_456,
      },
    },
    release_contract: {
      hosted_entrypoint: "cargo xtask acceptance",
      hosted_test_root: "ts-tests/",
      hosted_jobs: ["gates"],
      local_full_gate: "cargo xtask ci --baseline <trusted-base>",
      production_cli_runtime_node_required: false,
      production_cli_runtime_vendor_lookup: false,
      unsupported_before_first_sink_write: true,
    },
  };
}

function validateArtifact(artifact) {
  requireCondition(
    artifact.schema === 1 &&
      artifact.kind === "h1-emit-qualification" &&
      artifact.status === "qualified" &&
      artifact.phase === "H1.6",
    "invalid H1 qualification header",
  );
  requireCondition(
    artifact.upstream_closure.total_cases === 15_680 &&
      artifact.upstream_closure.compatible_cases === 1 &&
      artifact.upstream_closure.executed_cases === 1 &&
      artifact.upstream_closure.unexecuted_compatible_cases === 0,
    "upstream execution closure changed",
  );
  requireCondition(
    artifact.compatible_cases.length === 1 &&
      artifact.adjacent_controls.length === 7 &&
      artifact.owner_closure.pending_rows === 0,
    "H1 qualification closure is incomplete",
  );
  requireCondition(
    artifact.resource_summary.no_emit_performance.status === "qualified" &&
      artifact.resource_summary.emit_performance.status === "qualified",
    "H1 resource qualification is incomplete",
  );
  requireCondition(
    exactKeys(artifact.release_contract, [
      "hosted_entrypoint",
      "hosted_test_root",
      "hosted_jobs",
      "local_full_gate",
      "production_cli_runtime_node_required",
      "production_cli_runtime_vendor_lookup",
      "unsupported_before_first_sink_write",
    ]),
    "release contract shape changed",
  );
}

validateRuntime();
const artifact = buildArtifact();
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
    `H1 emit qualification is fresh: upstream=${artifact.upstream_closure.total_cases} compatible=1 executed=1 controls=7\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h1-emit-qualification.mjs [--write|--check]");
}
