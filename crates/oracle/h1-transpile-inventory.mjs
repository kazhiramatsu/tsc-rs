import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { TextDecoder } from "node:util";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h1-transpile-inventory.mjs";
const TARGET_RELATIVE_PATH =
  "vendor/typescript-6.0.3/transpile-suite-inventory.v1.json";
const TARGET_PATH = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h1-transpile-inventory.schema.json";
const SUITE_PIN_RELATIVE_PATH =
  "vendor/typescript-6.0.3/test-suites-pin.v2.json";
const PROFILE_RELATIVE_PATH = "ratchets/h1-emit-profile.v1.json";
const TYPESCRIPT_BUNDLE_RELATIVE_PATH =
  "vendor/typescript-6.0.3/lib/typescript.js";
const SOURCE_ROOT_RELATIVE_PATH = "ts-tests/tests/cases/transpile";
const SOURCE_ROOT = path.join(WORKSPACE, SOURCE_ROOT_RELATIVE_PATH);
const NODE_VERSION_PATH = path.join(WORKSPACE, ".node-version");

const EXPECTED_NODE_VERSION = "25.2.1";
const TYPESCRIPT_VERSION = "6.0.3";
const SOURCE_REPOSITORY = "https://github.com/microsoft/TypeScript.git";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const SUITE_PIN_SHA256 =
  "83f8edbb6f4535a19e61cf872532a46722f8cedbd2d746a0922dc507addc0879";
const PROFILE_SHA256 =
  "2edf0ec23a59cef953bf3322397c642fb5e38b5a33bd98310349ca16188ee6be";
const TYPESCRIPT_BUNDLE_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";
const TRANSPILE_SUITE = {
  name: "transpile",
  source_path: "tests/cases/transpile",
  vendored_path: SOURCE_ROOT_RELATIVE_PATH,
  git_tree_sha1: "e457f4923a084d10e9902ab311f640f02467e20d",
  blob_inventory_sha256:
    "d07d1ac154da492d5d1d5a01fd00eea830f9d372aff03215eda1baad8b2c12ac",
  files: 22,
  bytes: 13480,
  unique_blobs: 22,
  executable_paths: [],
};
const IMPLEMENTATION_SOURCES = [
  {
    source_path: "src/testRunner/transpileRunner.ts",
    git_blob_sha1: "3926aa9b7d88e953163ed1fee843d273783be131",
  },
  {
    source_path: "src/harness/harnessIO.ts",
    git_blob_sha1: "a06bde1c95182ea1bfad0ddf9af73053501a6dc7",
  },
  {
    source_path: "src/harness/harnessUtils.ts",
    git_blob_sha1: "f768325897167ad793eeff9ced7763e12f9aa154",
  },
];
const VARY_BY = ["declarationMap", "sourceMap", "inlineSourceMap"];
const REJECTED_FEATURE_ROOTS = [
  "decorators",
  "export-equals",
  "import-equals",
  "jsx",
  "parameter-properties",
  "runtime-enums",
  "runtime-namespaces",
];
const EXPECTED_SUMMARY = {
  source_files: 22,
  source_bytes: 13480,
  unique_blobs: 22,
  fixtures: 22,
  configurations: 25,
  fixture_units: 42,
  cases: 37,
  module_cases: 16,
  declaration_cases: 21,
  unit_operations: 79,
  javascript_transform_printer_controls: 14,
  deferred_source_map_controls: 2,
  deferred_declaration_controls: 20,
  deferred_declaration_map_controls: 1,
  report_diagnostics_cases: 2,
  bootstrap_profile_admitted_cases: 0,
  not_run_cases: 37,
  reference_baselines_compared: 0,
};

const UTF8 = new TextDecoder("utf-8", { fatal: true });
const OPTION_PATTERN = /^\/{2}\s*@(\w+)\s*:\s*([^\r\n]*)/gm;
const OPTION_LINE_PATTERN = /^\/{2}\s*@(\w+)\s*:\s*([^\r\n]*)/;
const LINK_LINE_PATTERN = /^\/{2}\s*@link\s*:/;

function fail(message) {
  throw new Error(message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function sha1(bytes) {
  return crypto.createHash("sha1").update(bytes).digest();
}

function gitBlobSha1(bytes) {
  return crypto
    .createHash("sha1")
    .update(`blob ${bytes.length}\0`)
    .update(bytes)
    .digest("hex");
}

function compareBytes(left, right) {
  return Buffer.compare(Buffer.from(left), Buffer.from(right));
}

function normalizeVersion(version) {
  return version.startsWith("v") ? version.slice(1) : version;
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

function requireJsonEqual(actual, expected, label) {
  requireCondition(
    jsonEqual(actual, expected),
    `${label} mismatch:\nactual=${JSON.stringify(actual)}\nexpected=${JSON.stringify(expected)}`,
  );
}

function exactKeys(value, keys) {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    keys.every((key) => Object.hasOwn(value, key)) &&
    Object.keys(value).every((key) => keys.includes(key))
  );
}

function validateRuntime() {
  const recorded = fs.readFileSync(NODE_VERSION_PATH, "utf8").trim();
  requireCondition(recorded === EXPECTED_NODE_VERSION, "unexpected .node-version");
  requireCondition(
    normalizeVersion(process.version) === recorded,
    `H1 transpile inventory requires Node ${recorded}; running ${process.version}`,
  );
  requireCondition(ts.version === TYPESCRIPT_VERSION, "unexpected TypeScript runtime version");
  requireCondition(
    pathHash(TYPESCRIPT_BUNDLE_RELATIVE_PATH).sha256 === TYPESCRIPT_BUNDLE_SHA256,
    "vendored TypeScript bundle hash changed",
  );
}

function validateInputs() {
  const pinBytes = fs.readFileSync(path.join(WORKSPACE, SUITE_PIN_RELATIVE_PATH));
  requireCondition(sha256(pinBytes) === SUITE_PIN_SHA256, "suite pin v2 hash changed");
  const pin = JSON.parse(pinBytes.toString("utf8"));
  requireCondition(
    pin.schema === 2 &&
      pin.typescript_version === TYPESCRIPT_VERSION &&
      pin.source_repository === SOURCE_REPOSITORY &&
      pin.source_commit === SOURCE_COMMIT,
    "suite pin v2 header changed",
  );
  const suites = pin.suites.filter((suite) => suite.name === "transpile");
  requireCondition(suites.length === 1, "suite pin must contain one transpile tree");
  requireJsonEqual(suites[0], TRANSPILE_SUITE, "transpile suite identity");
  requireCondition(
    pin.implementation_sources.some(
      (source) =>
        source.source_path === IMPLEMENTATION_SOURCES[0].source_path &&
        source.git_blob_sha1 === IMPLEMENTATION_SOURCES[0].git_blob_sha1,
    ),
    "suite pin does not preserve transpileRunner identity",
  );

  const profileBytes = fs.readFileSync(path.join(WORKSPACE, PROFILE_RELATIVE_PATH));
  requireCondition(sha256(profileBytes) === PROFILE_SHA256, "H1 profile hash changed");
  const profile = JSON.parse(profileBytes.toString("utf8"));
  requireCondition(
    profile.schema === 1 &&
      profile.status === "frozen" &&
      profile.phase === "H1.0a-bootstrap-profile",
    "H1 profile header changed",
  );
  requireJsonEqual(
    profile.source_profile.rejected_feature_roots,
    REJECTED_FEATURE_ROOTS,
    "H1 rejected feature roots",
  );
  const required = new Map(
    profile.emit_active_options.required.map((option) => [option.name, option.accepted]),
  );
  requireCondition(
    required.get("target")?.some((value) => value.name === "ESNext" && value.value === 99) &&
      required
        .get("module")
        ?.some((value) => value.name === "Preserve" && value.value === 200),
    "H1 required target/module profile changed",
  );
  return profile;
}

function normalizedRelativePath(root, candidate) {
  const relative = path.relative(root, candidate).split(path.sep).join("/");
  requireCondition(
    relative.length > 0 &&
      relative === path.posix.normalize(relative) &&
      !relative.startsWith("../") &&
      !relative.startsWith("/"),
    `unsafe transpile source path ${JSON.stringify(relative)}`,
  );
  return relative;
}

function walkSources(root, directory = root, output = []) {
  const entries = fs
    .readdirSync(directory, { withFileTypes: true })
    .sort((left, right) => compareBytes(left.name, right.name));
  requireCondition(entries.length > 0, `empty transpile directory ${directory}`);
  for (const entry of entries) {
    const candidate = path.join(directory, entry.name);
    requireCondition(!entry.isSymbolicLink(), `unexpected symlink ${candidate}`);
    if (entry.isDirectory()) {
      walkSources(root, candidate, output);
      continue;
    }
    requireCondition(entry.isFile(), `unsupported transpile entry ${candidate}`);
    requireCondition(
      (fs.statSync(candidate).mode & 0o111) === 0,
      `transpile source must not be executable: ${candidate}`,
    );
    const raw = fs.readFileSync(candidate);
    output.push({
      path: normalizedRelativePath(root, candidate),
      mode: "100644",
      bytes: raw.length,
      sha256: sha256(raw),
      git_blob_sha1: gitBlobSha1(raw),
      raw,
    });
  }
  return output;
}

function directoryNode() {
  return { directories: new Map(), files: new Map() };
}

function buildTree(files) {
  const root = directoryNode();
  for (const file of files) {
    const components = file.path.split("/");
    const name = components.pop();
    let current = root;
    for (const component of components) {
      requireCondition(!current.files.has(component), `tree collision at ${file.path}`);
      if (!current.directories.has(component)) {
        current.directories.set(component, directoryNode());
      }
      current = current.directories.get(component);
    }
    requireCondition(
      !current.files.has(name) && !current.directories.has(name),
      `duplicate source path ${file.path}`,
    );
    current.files.set(name, file);
  }
  return root;
}

function treeEntries(node) {
  const entries = [
    ...[...node.files].map(([name, file]) => ({ name, kind: "file", file })),
    ...[...node.directories].map(([name, child]) => ({
      name,
      kind: "directory",
      child,
    })),
  ];
  entries.sort((left, right) =>
    compareBytes(
      `${left.name}${left.kind === "directory" ? "/" : ""}`,
      `${right.name}${right.kind === "directory" ? "/" : ""}`,
    ),
  );
  return entries;
}

function hashTree(node) {
  const chunks = [];
  for (const entry of treeEntries(node)) {
    const mode = entry.kind === "directory" ? "40000" : "100644";
    const object =
      entry.kind === "directory"
        ? hashTree(entry.child)
        : Buffer.from(entry.file.git_blob_sha1, "hex");
    chunks.push(Buffer.from(`${mode} ${entry.name}\0`), object);
  }
  const body = Buffer.concat(chunks);
  return sha1(Buffer.concat([Buffer.from(`tree ${body.length}\0`), body]));
}

function flattenTree(node, prefix = "", output = []) {
  for (const entry of treeEntries(node)) {
    if (entry.kind === "directory") {
      flattenTree(entry.child, `${prefix}${entry.name}/`, output);
    } else {
      output.push({ path: `${prefix}${entry.name}`, file: entry.file });
    }
  }
  return output;
}

function treeIdentity(files) {
  const tree = buildTree(files);
  const inventory = Buffer.concat(
    flattenTree(tree).map(({ path: relative, file }) =>
      Buffer.from(
        `100644 blob ${file.git_blob_sha1} ${file.bytes}\t${relative}\0`,
      ),
    ),
  );
  return {
    git_tree_sha1: hashTree(tree).toString("hex"),
    blob_inventory_sha256: sha256(inventory),
    files: files.length,
    bytes: files.reduce((total, file) => total + file.bytes, 0),
    unique_blobs: new Set(files.map((file) => file.git_blob_sha1)).size,
    executable_paths: [],
  };
}

function orderedSettings(map) {
  return [...map].map(([name, value]) => ({ name, value }));
}

function extractSettings(content) {
  const settings = new Map();
  for (const match of content.matchAll(OPTION_PATTERN)) {
    settings.set(match[1], match[2].trim());
  }
  return settings;
}

function makeUnits(content, fixturePath) {
  const units = [];
  let currentContent;
  let currentOptions = new Map();
  let currentName;
  for (const line of content.split(/\r?\n/)) {
    requireCondition(!LINK_LINE_PATTERN.test(line), `${fixturePath} unexpectedly uses @link`);
    const metadata = OPTION_LINE_PATTERN.exec(line);
    if (metadata) {
      currentOptions.set(metadata[1], metadata[2].trim());
      if (metadata[1].toLowerCase() !== "filename") continue;
      if (currentName) {
        requireCondition(
          typeof currentContent === "string",
          `${fixturePath} has an undefined intermediate unit`,
        );
        units.push({
          name: currentName,
          file_options: orderedSettings(currentOptions),
          text: currentContent,
        });
        currentContent = undefined;
        currentOptions = new Map();
        currentName = metadata[2].trim();
      } else {
        currentName = metadata[2].trim();
        if (currentContent) {
          requireCondition(
            ts.skipTrivia(currentContent, 0, false, false) === currentContent.length,
            `${fixturePath} has non-comment content before its first @filename`,
          );
        }
        currentContent = "";
      }
    } else {
      if (currentContent === undefined) currentContent = "";
      else if (currentContent !== "") currentContent += "\n";
      currentContent += line;
    }
  }
  currentName = units.length > 0 || currentName ? currentName : path.posix.basename(fixturePath);
  units.push({
    name: currentName,
    file_options: orderedSettings(currentOptions),
    text: currentContent || "",
  });
  return units;
}

function splitVariation(value) {
  if (!value) return undefined;
  const entries = value
    .split(",")
    .map((entry) => entry.trim().toLowerCase())
    .filter((entry) => entry.length > 0);
  return entries.length > 1 ? entries : undefined;
}

function configurations(settings, fixturePath) {
  const dimensions = [];
  let count = 1;
  for (const name of VARY_BY) {
    if (!settings.has(name)) continue;
    const entries = splitVariation(settings.get(name));
    if (!entries) continue;
    count *= entries.length;
    requireCondition(count <= 25, `${fixturePath} exceeds the runner variation limit`);
    dimensions.push([name, entries]);
  }
  if (dimensions.length === 0) return [new Map()];
  let states = [new Map()];
  for (const [name, entries] of dimensions) {
    const next = [];
    for (const state of states) {
      for (const entry of entries) {
        const candidate = new Map(state);
        candidate.set(name, entry);
        next.push(candidate);
      }
    }
    states = next;
  }
  return states;
}

function mergedSettings(settings, overrides) {
  const merged = new Map(settings);
  for (const [name, value] of overrides) merged.set(name, value);
  return merged;
}

const optionIndex = new Map(
  ts.optionDeclarations.map((option) => [option.name.toLowerCase(), option]),
);

function compilerOptions(settings) {
  const options = {};
  for (const [name, raw] of settings) {
    const lower = name.toLowerCase();
    if (lower === "filename" || lower === "reportdiagnostics") continue;
    const option = optionIndex.get(lower);
    requireCondition(option !== undefined, `unknown compiler option @${name}`);
    let value;
    if (option.type === "boolean") {
      value = raw.toLowerCase() === "true";
    } else if (option.type === "string") {
      value = raw;
    } else if (option.type === "number") {
      value = Number.parseInt(raw, 10);
      requireCondition(Number.isFinite(value), `invalid numeric option @${name}: ${raw}`);
    } else if (option.type instanceof Map) {
      value = option.type.get(raw.toLowerCase());
      requireCondition(value !== undefined, `invalid custom option @${name}: ${raw}`);
    } else {
      fail(`transpile inventory does not admit list option @${name}`);
    }
    options[option.name] = value;
  }
  return options;
}

function scriptKind(fileName) {
  return ts.getScriptKindFromFileName(fileName) || ts.ScriptKind.TS;
}

function unitSyntax(text, fileName) {
  const source = ts.createSourceFile(
    fileName,
    text,
    ts.ScriptTarget.Latest,
    true,
    scriptKind(fileName),
  );
  const found = new Set();
  function visit(node) {
    if (ts.isEnumDeclaration(node)) found.add("runtime-enums");
    if (ts.isModuleDeclaration(node) && !ts.isStringLiteral(node.name)) {
      found.add("runtime-namespaces");
    }
    if (ts.isImportEqualsDeclaration(node)) found.add("import-equals");
    if (ts.isExportAssignment(node) && node.isExportEquals) found.add("export-equals");
    if (
      ts.isJsxElement(node) ||
      ts.isJsxFragment(node) ||
      ts.isJsxSelfClosingElement(node)
    ) {
      found.add("jsx");
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
      found.add("parameter-properties");
    }
    if (node.modifiers?.some((modifier) => modifier.kind === ts.SyntaxKind.Decorator)) {
      found.add("decorators");
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
  return {
    rejected_feature_roots: REJECTED_FEATURE_ROOTS.filter((feature) => found.has(feature)),
    parse_diagnostic_codes: [
      ...new Set(source.parseDiagnostics.map((diagnostic) => diagnostic.code)),
    ].sort((left, right) => left - right),
  };
}

function fixtureUnit(unit) {
  return {
    name: unit.name,
    file_options: unit.file_options,
    content: {
      utf8_bytes: Buffer.byteLength(unit.text, "utf8"),
      sha256: sha256(unit.text),
    },
    ...unitSyntax(unit.text, unit.name),
  };
}

function stemAndExtension(fileName) {
  const extension = path.posix.extname(fileName);
  return { stem: fileName.slice(0, fileName.length - extension.length), extension };
}

function outputExtension(fileName, settings) {
  return ts.getOutputExtension(fileName, {
    jsx: settings.get("jsx") === "preserve" ? ts.JsxEmit.Preserve : undefined,
  });
}

function profileBlockers(kind, api, settings, options, units, profile) {
  const blockers = [`api:component-only:${api}`];
  if (options.target !== ts.ScriptTarget.ESNext) {
    blockers.push(`required-option:target=${settings.get("target") ?? "absent"}`);
  }
  if (options.module !== ts.ModuleKind.Preserve) {
    blockers.push(`required-option:module=${settings.get("module") ?? "absent"}`);
  }
  for (const name of profile.emit_active_options.rejected_when_effective) {
    const value = options[name];
    if (value !== undefined && value !== false) blockers.push(`rejected-option:${name}`);
  }
  const features = new Set(units.flatMap((unit) => unit.rejected_feature_roots));
  for (const feature of REJECTED_FEATURE_ROOTS) {
    if (features.has(feature)) blockers.push(`rejected-feature:${feature}`);
  }
  if (kind === "declaration") blockers.push("product:declaration");
  return [...new Set(blockers)];
}

function unitOutputs(kind, units, settings, options) {
  return units.map((unit) => {
    if (kind === "module") {
      const outputPath = ts.changeExtension(
        unit.name,
        outputExtension(unit.name, settings),
      );
      const products = ["javascript"];
      if (options.sourceMap) products.push("javascript-map");
      if (options.inlineSourceMap) products.push("javascript-inline-map");
      return { unit: unit.name, output_path: outputPath, products };
    }
    const outputPath = ts.changeExtension(
      unit.name,
      ts.getDeclarationEmitExtensionForPath(unit.name),
    );
    const products = ["declaration"];
    if (options.declarationMap) products.push("declaration-map");
    return { unit: unit.name, output_path: outputPath, products };
  });
}

function componentDisposition(kind, options) {
  if (kind === "module") {
    return options.sourceMap || options.inlineSourceMap
      ? "deferred-source-map-control"
      : "javascript-transform-printer-control";
  }
  return options.declarationMap
    ? "deferred-declaration-map-control"
    : "deferred-declaration-control";
}

function sourceInventoryDigest(sources) {
  const records = sources
    .map(
      (source) =>
        `${source.path}\0${source.mode}\0${source.bytes}\0${source.sha256}\0${source.git_blob_sha1}\n`,
    )
    .join("");
  return sha256(records);
}

function buildArtifact(profile) {
  const collected = walkSources(SOURCE_ROOT);
  requireJsonEqual(
    {
      name: TRANSPILE_SUITE.name,
      source_path: TRANSPILE_SUITE.source_path,
      vendored_path: TRANSPILE_SUITE.vendored_path,
      ...treeIdentity(collected),
    },
    TRANSPILE_SUITE,
    "vendored transpile Git tree",
  );
  const sources = collected.map(({ raw: _raw, ...source }) => source);
  const fixtures = [];
  const cases = [];

  for (const [sourceIndex, source] of collected.entries()) {
    requireCondition(/\.[cm]?[tj]sx?$/i.test(source.path), `runner would not enumerate ${source.path}`);
    let decoded;
    try {
      decoded = UTF8.decode(source.raw);
    } catch (error) {
      fail(`${source.path} is not UTF-8: ${error.message}`);
    }
    requireCondition(!decoded.startsWith("\uFEFF"), `${source.path} unexpectedly has a BOM`);
    const settings = extractSettings(decoded);
    const parsedUnits = makeUnits(decoded, source.path);
    const units = parsedUnits.map(fixtureUnit);
    const { stem, extension } = stemAndExtension(path.posix.basename(source.path));
    const fixtureConfigurations = configurations(settings, source.path).map((overrides) => {
      const variant = overrides.size
        ? [...overrides].map(([name, value]) => `${name}=${value}`).join(",")
        : "default";
      return {
        variant,
        runner_name: overrides.size ? `${stem}(${variant})` : stem,
        overrides: orderedSettings(overrides),
      };
    });
    fixtures.push({
      source: sourceIndex,
      encoding: "utf-8",
      decoded_utf8_bytes: Buffer.byteLength(decoded, "utf8"),
      decoded_sha256: sha256(decoded),
      settings: orderedSettings(settings),
      units,
      configurations: fixtureConfigurations,
    });

    for (const [configurationIndex, configuration] of fixtureConfigurations.entries()) {
      const overrides = new Map(
        configuration.overrides.map((setting) => [setting.name, setting.value]),
      );
      const effectiveSettings = mergedSettings(settings, overrides);
      const options = compilerOptions(effectiveSettings);
      const kinds = [];
      if (!effectiveSettings.has("emitDeclarationOnly")) kinds.push("module");
      if (effectiveSettings.has("declaration")) kinds.push("declaration");
      requireCondition(kinds.length > 0, `${source.path} configuration ${configuration.variant} has no runner kind`);
      for (const kind of kinds) {
        const api = kind === "module" ? "transpileModule" : "transpileDeclaration";
        const baselineExtension =
          kind === "module"
            ? outputExtension(`${configuration.runner_name}${extension}`, effectiveSettings)
            : ts.getDeclarationEmitExtensionForPath(`${configuration.runner_name}${extension}`);
        const baselinePath = `tests/baselines/reference/transpile/${configuration.runner_name}${baselineExtension}`;
        const blockers = profileBlockers(
          kind,
          api,
          effectiveSettings,
          options,
          units,
          profile,
        );
        requireCondition(blockers.length > 0, "component API blocker must remain explicit");
        cases.push({
          id: `transpile:${source.path}#${configuration.variant}#${kind}`,
          source: sourceIndex,
          configuration: configurationIndex,
          kind,
          api,
          baseline_path: baselinePath,
          unit_outputs: unitOutputs(kind, units, effectiveSettings, options),
          report_diagnostics: effectiveSettings.get("reportDiagnostics") === "true",
          execution_state: "not-run",
          reference_baseline_state: "path-pinned-content-not-vendored-or-compared",
          component_disposition: componentDisposition(kind, options),
          whole_program_equivalence: "unproven",
          bootstrap_profile_admitted: false,
          profile_blockers: blockers,
        });
      }
    }
  }

  const countDisposition = (disposition) =>
    cases.filter((entry) => entry.component_disposition === disposition).length;
  const summary = {
    source_files: sources.length,
    source_bytes: sources.reduce((total, source) => total + source.bytes, 0),
    unique_blobs: new Set(sources.map((source) => source.git_blob_sha1)).size,
    fixtures: fixtures.length,
    configurations: fixtures.reduce(
      (total, fixture) => total + fixture.configurations.length,
      0,
    ),
    fixture_units: fixtures.reduce((total, fixture) => total + fixture.units.length, 0),
    cases: cases.length,
    module_cases: cases.filter((entry) => entry.kind === "module").length,
    declaration_cases: cases.filter((entry) => entry.kind === "declaration").length,
    unit_operations: cases.reduce((total, entry) => total + entry.unit_outputs.length, 0),
    javascript_transform_printer_controls: countDisposition(
      "javascript-transform-printer-control",
    ),
    deferred_source_map_controls: countDisposition("deferred-source-map-control"),
    deferred_declaration_controls: countDisposition("deferred-declaration-control"),
    deferred_declaration_map_controls: countDisposition(
      "deferred-declaration-map-control",
    ),
    report_diagnostics_cases: cases.filter((entry) => entry.report_diagnostics).length,
    bootstrap_profile_admitted_cases: cases.filter(
      (entry) => entry.bootstrap_profile_admitted,
    ).length,
    not_run_cases: cases.filter((entry) => entry.execution_state === "not-run").length,
    reference_baselines_compared: 0,
  };
  requireJsonEqual(summary, EXPECTED_SUMMARY, "transpile runner summary");

  const ids = new Set(cases.map((entry) => entry.id));
  const baselines = new Set(cases.map((entry) => entry.baseline_path));
  requireCondition(ids.size === cases.length, "transpile case IDs are not unique");
  requireCondition(baselines.size === cases.length, "transpile baseline paths are not unique");
  requireCondition(
    fixtures.every((fixture, index) => fixture.source === index),
    "transpile fixtures are not in source order",
  );

  return {
    schema: 1,
    status: "classified-not-run",
    phase: "H1.0a-transpile-runner-inventory",
    typescript: {
      version: TYPESCRIPT_VERSION,
      source_repository: SOURCE_REPOSITORY,
      source_commit: SOURCE_COMMIT,
    },
    generator: pathHash(GENERATOR_RELATIVE_PATH),
    contract: pathHash(CONTRACT_RELATIVE_PATH),
    inputs: {
      suite_pin: { path: SUITE_PIN_RELATIVE_PATH, sha256: SUITE_PIN_SHA256 },
      transpile_suite: TRANSPILE_SUITE,
      h1_profile: { path: PROFILE_RELATIVE_PATH, sha256: PROFILE_SHA256 },
      typescript_bundle: {
        path: TYPESCRIPT_BUNDLE_RELATIVE_PATH,
        sha256: TYPESCRIPT_BUNDLE_SHA256,
      },
      implementation_sources: IMPLEMENTATION_SOURCES,
    },
    runner_contract: {
      enumeration: "recursive files matching /\\.[cm]?[tj]sx?/i in tests/cases/transpile",
      vary_by: VARY_BY,
      variation_limit: 25,
      configuration_order: "vary_by order, then each comma-separated value order",
      unit_partition: "harnessIO.makeUnitsFromTest with CRLF/LF line splitting and @filename metadata",
      unit_execution: "each configuration runs each parsed unit independently in source order",
      run_kinds: ["module", "declaration"],
      reference_baseline_state: "path-pinned-content-not-vendored-or-compared",
    },
    source_inventory_sha256: sourceInventoryDigest(sources),
    sources,
    fixtures,
    cases,
    summary,
  };
}

function validateArtifact(artifact) {
  requireCondition(
    exactKeys(artifact, [
      "schema",
      "status",
      "phase",
      "typescript",
      "generator",
      "contract",
      "inputs",
      "runner_contract",
      "source_inventory_sha256",
      "sources",
      "fixtures",
      "cases",
      "summary",
    ]),
    "transpile inventory has missing or unknown top-level fields",
  );
  requireCondition(
    artifact.schema === 1 &&
      artifact.status === "classified-not-run" &&
      artifact.phase === "H1.0a-transpile-runner-inventory",
    "invalid transpile inventory header",
  );
  requireJsonEqual(artifact.summary, EXPECTED_SUMMARY, "validated summary");
}

validateRuntime();
const profile = validateInputs();
const artifact = buildArtifact(profile);
validateArtifact(artifact);
const rendered = `${JSON.stringify(artifact, null, 2)}\n`;
const mode = process.argv[2];

if (mode === "--write") {
  fs.writeFileSync(TARGET_PATH, rendered);
  process.stdout.write(`wrote ${TARGET_RELATIVE_PATH}\n`);
} else if (mode === "--check") {
  requireCondition(fs.existsSync(TARGET_PATH), `missing ${TARGET_RELATIVE_PATH}`);
  requireCondition(
    fs.readFileSync(TARGET_PATH, "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run with --write and review`,
  );
  process.stdout.write(
    `H1 transpile inventory is fresh: fixtures=${EXPECTED_SUMMARY.fixtures} configurations=${EXPECTED_SUMMARY.configurations} cases=${EXPECTED_SUMMARY.cases} status=not-run\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h1-transpile-inventory.mjs [--write|--check]");
}
