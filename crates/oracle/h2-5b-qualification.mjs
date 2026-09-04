import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5b-qualification.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5b-qualification.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-5b-qualification.schema.json";
const H2_5A_PROFILE_RELATIVE_PATH = "ratchets/h2-5a-profile.v1.json";
const GLOBAL_DISPOSITIONS_RELATIVE_PATH =
  "ratchets/h2-candidate-dispositions.v1.json";
const OWNER_RELATIVE_PATH = "ratchets/h2-owner-inventory.v1.json";
const COMPILER_CLASSIFICATION =
  "vendor/typescript-6.0.3/compiler-profile-classification.v1.json";
const CONFORMANCE_CLASSIFICATION =
  "vendor/typescript-6.0.3/conformance-profile-classification.v1.json";
const COMPILER_EXPANSION =
  "vendor/typescript-6.0.3/test-suite-expansion.v1.json";
const CONFORMANCE_EXPANSION =
  "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const NODE_VERSION_PATH = ".node-version";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const H2_5A_MERGE_COMMIT = "1676080a3efa759464a17fe91c96bbc807f38fc5";
const H2_5A_PROFILE_SHA256 =
  "7e96f229ef0d738eac41a854026a18e8acf703f3176c8dc0182a0865e7859ed8";
const EXPECTED_NODE = "25.2.1";
const VIRTUAL_SOURCE_ROOT = "/.src";
const MAX_TRANSFORM_DEPTH = 256;
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
const OWNER_KEYS = Object.freeze(["transform-es2021"]);
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
  "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c", "H2.3d", "H2.4a", "H2.4b", "H2.5a", "H2.5b",
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
  let virtualConfig = null;
  if (fixture.virtual_config !== null) {
    const parsedConfig = parsed.units.pop();
    requireCondition(
      parsedConfig?.name === fixture.virtual_config.name &&
        canonical(parsedConfig.file_options) ===
          canonical(fixture.virtual_config.file_options) &&
        canonical(contentIdentity(parsedConfig.text)) ===
          canonical(fixture.virtual_config.content) &&
        fixture.virtual_config.document_symlinks.length === 0,
      `${suite}/${source.path} virtual config changed`,
    );
    virtualConfig = parsedConfig;
  }
  requireCondition(parsed.links.length === 0, `${suite}/${source.path} gained global links`);
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
        expected.document_symlinks.length === 0,
      `${suite}/${source.path} unit ${index} changed`,
    );
  });
  return { source, units: parsed.units, virtualConfig };
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

function effectiveCompilerOptions(settings) {
  const options = { noResolve: false };
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

function hasDirectory(files, directory) {
  const normalized = ts.normalizePath(directory);
  const prefix = normalized.endsWith("/") ? normalized : `${normalized}/`;
  return [...files.keys()].some((fileName) => fileName.startsWith(prefix));
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
  const unitByPath = new Map();
  for (const id of selection.vfs_write_order) {
    const unit = loaded.units[id];
    requireCondition(unit.text !== undefined, `${loaded.source.path} has missing content`);
    unitByPath.set(ts.getNormalizedAbsolutePath(unit.name, cwd), { id, unit });
  }
  const baseHost = ts.createCompilerHost(options, true);
  const host = {
    ...baseHost,
    getCurrentDirectory: () => cwd,
    useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: (fileName) => fileName,
    trace() {},
    fileExists(fileName) {
      const normalized = ts.normalizePath(fileName);
      return unitByPath.has(normalized) || baseHost.fileExists(normalized);
    },
    readFile(fileName) {
      const normalized = ts.normalizePath(fileName);
      return unitByPath.get(normalized)?.unit.text ?? baseHost.readFile(normalized);
    },
    directoryExists(directory) {
      return hasDirectory(unitByPath, directory) || (baseHost.directoryExists?.(directory) ?? false);
    },
    getDirectories(directory) {
      return baseHost.getDirectories?.(directory) ?? [];
    },
    realpath(fileName) {
      const normalized = ts.normalizePath(fileName);
      return unitByPath.has(normalized) ? normalized : (baseHost.realpath?.(normalized) ?? normalized);
    },
    getSourceFile(fileName, languageVersion) {
      const normalized = ts.normalizePath(fileName);
      const fixture = unitByPath.get(normalized);
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
  return { program: ts.createProgram(roots, options, host), roots, cwd, unitByPath };
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

function analyzeCase(
  loaded,
  selection,
  settings,
  options,
  makeProgram,
  ownerReachability,
) {
  const { program, roots, cwd, unitByPath } = makeProgram();
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
    typescript_runs: [first, second],
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
  fail(`unexpected H2.5b module kind ${moduleKind}`);
}

function targetStateName(target) {
  if (target === ts.ScriptTarget.ES2020) return "ES2020(7)";
  fail(`unexpected H2.5b target ${target}`);
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

function buildSuite(suite, classification, expansion, selectionOrigins) {
  const rows = moduleCandidates(classification, selectionOrigins);
  const loadedBySource = new Map();
  return rows.map((row) => {
    const fixture = fixtureFor(suite, expansion, row);
    let loaded = loadedBySource.get(fixture.source);
    if (!loaded) {
      loaded = loadFixture(suite, expansion, fixture);
      loadedBySource.set(fixture.source, loaded);
    }
    const configuration = fixture.configurations[row.configuration];
    requireCondition(configuration !== undefined, `${row.id} configuration is absent`);
    const settings = mergedSettings(fixture.settings, configuration.settings);
    const options = effectiveCompilerOptions(settings);
    requireCondition(
      options.target === ts.ScriptTarget.ES2020 &&
        [
          undefined,
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
      `${row.id} is no longer an H2.5b option candidate`,
    );
    const selection = explicitRootSelection(loaded, settings, options);
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
    return withFingerprint(
      {
        suite,
        case_id: row.id,
        selection_origin: row.selection_origin,
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
  });
}

function countBy(values) {
  const counts = new Map();
  for (const value of values) counts.set(value, (counts.get(value) ?? 0) + 1);
  return [...counts]
    .map(([value, cases]) => ({ value, cases }))
    .sort((left, right) => right.cases - left.cases || left.value.localeCompare(right.value));
}

function buildArtifact() {
  const compilerClassification = readJson(COMPILER_CLASSIFICATION);
  const conformanceClassification = readJson(CONFORMANCE_CLASSIFICATION);
  const compilerExpansion = readJson(COMPILER_EXPANSION);
  const conformanceExpansion = readJson(CONFORMANCE_EXPANSION);
  const owner = readJson(OWNER_RELATIVE_PATH);
  const h2_5a_profile = pathHash(H2_5A_PROFILE_RELATIVE_PATH);
  requireCondition(
    h2_5a_profile.sha256 === H2_5A_PROFILE_SHA256,
    "reviewed H2.5a profile changed",
  );
  const globalDispositions = readJson(GLOBAL_DISPOSITIONS_RELATIVE_PATH);
  const closedBeforeH2_5b = new Set(
    [...CLOSED_SLICES].filter((slice) => slice !== "H2.5b"),
  );
  const globalRows = globalDispositions.cases.filter((entry) =>
    entry.required_slices.includes("H2.5b"),
  );
  const candidateRows = globalRows.filter((entry) =>
    entry.required_slices.every(
      (slice) => slice === "H2.5b" || closedBeforeH2_5b.has(slice),
    ),
  );
  const selectionOrigins = new Map(
    candidateRows.map((entry) => [entry.id, "global-h2-5b-candidate"]),
  );
  requireCondition(globalRows.length === 84, "unexpected global H2.5b row count");
  requireCondition(
    candidateRows.length === 72,
    `unexpected global H2.5b candidate denominator ${candidateRows.length}`,
  );
  requireCondition(
    selectionOrigins.size === 72,
    `unexpected H2.5b candidate denominator ${selectionOrigins.size}`,
  );
  const ownerRows = OWNER_KEYS.map((key) => {
    const row = owner.owners.find((entry) => entry.key === key);
    requireCondition(row?.owner_slice === "H2.5b", `missing transform owner ${key}`);
    return {
      key,
      declaration: row.declaration,
      disposition_before_h2_5b: row.disposition,
    };
  });
  const cases = [
    ...buildSuite(
      "compiler",
      compilerClassification,
      compilerExpansion,
      selectionOrigins,
    ),
    ...buildSuite(
      "conformance",
      conformanceClassification,
      conformanceExpansion,
      selectionOrigins,
    ),
  ].sort((left, right) => left.suite.localeCompare(right.suite) || left.case_id.localeCompare(right.case_id));
  requireCondition(cases.length === 72, `unexpected H2.5b denominator ${cases.length}`);
  requireCondition(new Set(cases.map((entry) => entry.case_id)).size === cases.length, "duplicate H2.5b case");
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
    typescript_runs: cases.reduce((sum, entry) => sum + entry.typescript_runs.length, 0),
    deterministic_typescript_cases: cases.filter(
      (entry) => entry.typescript_runs[0].run_fingerprint_sha256 === entry.typescript_runs[1].run_fingerprint_sha256,
    ).length,
    admitted_typescript_writes: admitted.reduce(
      (sum, entry) => sum + entry.typescript_runs[0].writes.length,
      0,
    ),
    diagnostic_control_typescript_writes: outputControls.reduce(
      (sum, entry) => sum + entry.typescript_runs[0].writes.length,
      0,
    ),
    admitted_typescript_diagnostics: admitted.reduce(
      (sum, entry) => sum + entry.typescript_runs[0].reported_diagnostics.length,
      0,
    ),
    unexecuted_candidates: 0,
    undispositioned_candidates: cases.filter(
      (entry) =>
        !entry.disposition ||
        (entry.disposition === "deferred-to-slices" && entry.required_slices.length === 0),
    ).length,
  };
  requireCondition(summary.compiler_candidates === 19, "compiler candidate count changed");
  requireCondition(summary.conformance_candidates === 53, "conformance candidate count changed");
  requireCondition(summary.admitted_cases === 68, "exact admission count changed");
  requireCondition(
    summary.diagnostic_deferred_output_control_cases === 0,
    "diagnostic control count changed",
  );
  requireCondition(summary.source_deferred_cases === 4, "source-deferred count changed");
  requireCondition(summary.no_emit_control_cases === 0, "no-emit control count changed");
  requireCondition(
    canonical(summary.target_states) ===
      canonical([{ value: "ES2020(7)", cases: 72 }]),
    "target distribution changed",
  );
  requireCondition(
    canonical(summary.module_states) ===
      canonical([
        { value: "absent", cases: 36 },
        { value: "ES2020(6)", cases: 11 },
        { value: "ESNext(99)", cases: 7 },
        { value: "CommonJS(1)", cases: 5 },
        { value: "NodeNext(199)", cases: 5 },
        { value: "ES2015(5)", cases: 2 },
        { value: "Node18(101)", cases: 2 },
        { value: "Node20(102)", cases: 2 },
        { value: "Preserve(200)", cases: 2 },
      ]),
    "module distribution changed",
  );
  requireCondition(
    canonical(summary.first_deferred_slices) ===
      canonical([{ value: "H2.9", cases: 4 }]),
    "deferred-slice distribution changed",
  );
  requireCondition(summary.admitted_typescript_writes === 93, "exact write count changed");
  requireCondition(
    summary.diagnostic_control_typescript_writes === 0,
    "diagnostic-control write count changed",
  );
  requireCondition(
    summary.admitted_typescript_diagnostics === 48,
    "exact diagnostic count changed",
  );
  requireCondition(summary.typescript_runs === 144, "TypeScript repetition count changed");
  requireCondition(summary.deterministic_typescript_cases === 72, "TypeScript determinism is open");
  requireCondition(summary.undispositioned_candidates === 0, "H2.5b retained an undispositioned case");
  return withFingerprint(
    {
      schema: 1,
      status: "qualified-typescript-oracle",
      phase: "H2.5b-es2021-target",
      typescript: {
        version: ts.version,
        source_commit: SOURCE_COMMIT,
        bundle: pathHash(TYPESCRIPT_BUNDLE),
        implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
      },
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        h2_5a_merge: H2_5A_MERGE_COMMIT,
        h2_5a_profile,
      },
      selection_contract: {
        global_h2_5b_rows: globalRows.length,
        candidate_definition:
          "the 72 global H2.5b rows whose complete required-slice set is closed through H2.5b",
        global_candidate_denominator: candidateRows.length,
        candidate_denominator: selectionOrigins.size,
        future_deferred_rows: globalRows.length - candidateRows.length,
      },
      inputs: {
        compiler_classification: pathHash(COMPILER_CLASSIFICATION),
        conformance_classification: pathHash(CONFORMANCE_CLASSIFICATION),
        compiler_expansion: pathHash(COMPILER_EXPANSION),
        conformance_expansion: pathHash(CONFORMANCE_EXPANSION),
        owner_inventory: pathHash(OWNER_RELATIVE_PATH),
        global_candidate_dispositions: pathHash(GLOBAL_DISPOSITIONS_RELATIVE_PATH),
      },
      execution_contract: {
        source_reachability: "fixture VFS roots plus module-resolved fixture dependencies in a vendored TypeScript Program",
        module_selection: "Program.getEmitModuleFormatOfFile for every reached fixture SourceFile",
        admission: `every selected global row has a required-slice set closed through H2.5b; every emit-eligible source targets ES2020, has no parse diagnostics, has AST depth <= ${MAX_TRANSFORM_DEPTH}, and requires no later source/output owner; transformES2021 runs after the already-closed transformESNext/class-field pipeline and before the module transformer, and diagnostics and writes are exact; declaration/package/config inputs remain non-emitted controls`,
        typescript_repetitions: 2,
        rust_repetitions: 2,
        normalization: "none",
        deferred_boundary: "typed failure before first sink write",
      },
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

validateRuntime();
const artifact = buildArtifact();
const rendered = render(artifact);
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
  process.stdout.write(
    `wrote ${TARGET_RELATIVE_PATH}: candidates=${artifact.summary.candidates} admitted=${artifact.summary.admitted_cases} deferred=${artifact.summary.deferred_cases}\n`,
  );
} else if (mode === "--check") {
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-5b-qualification.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5b qualification is fresh: candidates=${artifact.summary.candidates} admitted=${artifact.summary.admitted_cases} deferred=${artifact.summary.deferred_cases}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-5b-qualification.mjs [--write|--check]");
}
