// H2.7c: original corpus membership, independently checked input projections,
// and complete repeated TypeScript observations. No Rust outputs are read.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "./vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "../..");
const generator = "crates/oracle/h2-7c-qualification.mjs";
const contract = ".github/ci/contracts/h2-7c-qualification.schema.json";
const target = "ratchets/h2-7c-qualification.v1.json";
const inputPath = "crates/compiler/tests/fixtures/h2-7c-corpus-inputs.json";
const parentPath = "ratchets/h2-7b-qualification.v1.json";
const globalPath = "ratchets/h2-candidate-dispositions.v1.json";
const expansionPaths = ["vendor/typescript-6.0.3/test-suite-expansion.v1.json",
  "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json"];
const configPath = "vendor/typescript-6.0.3/compiler-config-plans.v1.json";
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const bytes = relative => fs.readFileSync(path.join(root, relative));
const read = relative => JSON.parse(bytes(relative));
const identity = relative => ({ path: relative, sha256: sha256(bytes(relative)) });
const mode = process.argv[2];
assert.ok(mode === "--write" || mode === "--check", "use --write or --check");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
assert.equal(ts.version, "6.0.3");

const prior = read(parentPath).cases.filter(row => row.first_owner === "H2.7c");
const global = read(globalPath).cases.filter(row => row.required_slices.includes("H2.7c"));
assert.equal(prior.length, 26);
assert.equal(global.length, 16);
const selected = [
  ...prior.map(row => ({ case_id: row.case_id, source: row.source,
    origin: "h2-7b-first-owner", remaining_slices: row.required_slices.filter(s => s > "H2.7c") })),
  ...global.map(row => ({ case_id: row.id, source: row.source,
    origin: "global-required-slice", remaining_slices: row.required_slices.filter(s => s > "H2.7c") })),
];
assert.equal(new Set(selected.map(row => row.case_id)).size, 42);
const inputs = read(inputPath);
assert.equal(inputs.typescript, ts.version);
assert.equal(inputs.source_commit, "050880ce59e30b356b686bd3144efe24f875ebc8");
assert.deepEqual(inputs.cases.map(row => row.case_id),
  selected.filter(row => row.remaining_slices.length === 0).map(row => row.case_id));
assert.equal(inputs.cases.length, 32);
const inputById = new Map(inputs.cases.map(row => [row.case_id, row]));
const expansions = expansionPaths.map(read);
const plans = read(configPath).fixtures;
const optionsByName = new Map(ts.optionDeclarations.map(option => [option.name.toLowerCase(), option]));

function setOption(options, name, raw) {
  const option = optionsByName.get(name.toLowerCase());
  if (!option) {
    assert.ok(["filename", "noimplicitreferences", "currentdirectory", "notypesandsymbols"].includes(name.toLowerCase()), name);
    return;
  }
  const errors = [];
  options[option.name] = option.type === "boolean" ? String(raw).toLowerCase() === "true"
    : option.type === "string" ? String(raw)
    : option.type === "number" ? Number.parseInt(raw, 10)
    : ts.parseCustomTypeOption(option, raw, errors);
  assert.deepEqual(errors, [], name);
}

function parseConfig(input) {
  if (!input.config) return null;
  const files = [...input.files, input.config];
  const host = {
    useCaseSensitiveFileNames: false,
    fileExists: name => files.some(file => file.path.toLowerCase() === name.toLowerCase()),
    readFile: name => files.find(file => file.path.toLowerCase() === name.toLowerCase())?.text,
    readDirectory(directory, extensions, excludes, includes, depth) {
      return ts.matchFiles(directory, extensions, excludes, includes, false, "", depth, dir => {
        const names = [], directories = new Set();
        const prefix = dir.endsWith("/") ? dir : dir + "/";
        for (const file of files) {
          if (!file.path.toLowerCase().startsWith(prefix.toLowerCase())) continue;
          const relative = file.path.slice(prefix.length), separator = relative.indexOf("/");
          if (separator < 0) names.push(relative);
          else directories.add(relative.slice(0, separator));
        }
        return { files: names, directories: [...directories] };
      }, ts.identity);
    },
  };
  const source = ts.parseJsonText(input.config.path, input.config.text);
  const parsed = ts.parseJsonSourceFileConfigFileContent(source, host,
    ts.getDirectoryPath(input.config.path), undefined, input.config.path);
  assert.deepEqual(source.parseDiagnostics, []);
  assert.deepEqual(parsed.errors, []);
  assert.deepEqual(parsed.fileNames, input.roots);
  return parsed;
}

function verifyInput(row, rawSource) {
  const input = row.replay_input;
  assert.deepEqual(input.vfs_symlinks, []);
  const parsed = parseConfig(input);
  let options;
  if (row.project_input) {
    const descriptor = JSON.parse(rawSource);
    assert.equal(input.current_directory, "/.src/" + descriptor.projectRoot);
    assert.deepEqual(input.roots, descriptor.inputFiles.map(name => ts.combinePaths(input.current_directory, name)));
    assert.equal(input.default_library_file_name, "lib.es5.d.ts");
    assert.equal(input.config, null);
    const directory = path.join(root, "ts-tests", descriptor.projectRoot);
    const files = fs.readdirSync(directory, { recursive: true }).filter(name =>
      fs.statSync(path.join(directory, name)).isFile()).sort();
    assert.deepEqual(input.files, files.map(name => ({
      path: ts.combinePaths(input.current_directory, name),
      text: fs.readFileSync(path.join(directory, name), "utf8"),
    })));
    options = { moduleResolution: ts.ModuleResolutionKind.Classic,
      noErrorTruncation: false, skipDefaultLibCheck: false, newLine: ts.NewLineKind.CarriageReturnLineFeed };
    for (const [name, value] of Object.entries(descriptor)) {
      if (!["scenario", "projectRoot", "inputFiles", "baselineCheck"].includes(name)) setOption(options, name, value);
    }
    const variant = decodeURIComponent(row.case_id.split("#")[1]);
    assert.ok(["module=amd", "module=commonjs"].includes(variant));
    setOption(options, "module", variant.slice("module=".length));
  } else {
    const compiler = row.case_id.startsWith("typescript-6.0.3/compiler/");
    const expansion = expansions[compiler ? 0 : 1];
    const recordedCase = expansion.cases.find(entry => entry.id === row.case_id);
    assert.ok(recordedCase, row.case_id);
    const fixture = (compiler ? expansion.compiler_fixtures : expansion.fixtures)
      .find(entry => entry.source === recordedCase.source);
    const index = compiler ? recordedCase.configuration.configuration : recordedCase.configuration;
    const settings = new Map(fixture.settings.map(setting => [setting.name, setting.value]));
    for (const setting of fixture.configurations[index].settings) settings.set(setting.name, setting.value);
    assert.deepEqual(row.settings, [...settings]);
    assert.equal(input.current_directory, settings.get("currentDirectory") ?? "/.src");
    assert.equal(input.default_library_file_name, null);
    assert.deepEqual(fixture.links, []);
    const normal = row.files.filter(file => file.name !== fixture.virtual_config?.name);
    assert.equal(normal.length, fixture.normal_units.length);
    for (const [id, recorded] of fixture.normal_units.entries()) {
      assert.equal(normal[id].name, recorded.name);
      assert.equal(sha256(normal[id].text), recorded.content.sha256);
      assert.equal(Buffer.byteLength(normal[id].text), recorded.content.utf8_bytes);
      assert.deepEqual(recorded.document_symlinks, []);
    }
    if (fixture.virtual_config) {
      assert.equal(input.config.path, fixture.virtual_config.name);
      assert.equal(sha256(input.config.text), fixture.virtual_config.content.sha256);
      if (compiler) {
        const plan = plans.find(plan => plan.source.index === recordedCase.source);
        assert.deepEqual(input.roots, plan.parsed_file_names);
      }
    } else assert.equal(input.config, null);
    options = parsed ? ts.cloneCompilerOptions(parsed.options) : { noResolve: false };
    Object.assign(options, { newLine: ts.NewLineKind.CarriageReturnLineFeed,
      noErrorTruncation: true, skipDefaultLibCheck: true });
    for (const [name, value] of settings) setOption(options, name, value);
    const candidates = [...new Map(normal.map((file, id) =>
      [ts.getNormalizedAbsolutePath(file.name, input.current_directory), id])).values()].sort((a, b) => a - b);
    let roots;
    if (parsed) roots = candidates.filter(id => parsed.fileNames.includes(
      ts.getNormalizedAbsolutePath(normal[id].name, input.current_directory)));
    else {
      const last = candidates.at(-1), text = normal[last].text;
      const implicit = settings.has("noImplicitReferences") || text.includes("require(") || /reference\s+path/.test(text);
      roots = implicit ? [last] : candidates;
    }
    const other = candidates.filter(id => !roots.includes(id));
    const programRoots = roots.filter(id => !normal[id].name.endsWith(".json") &&
      ts.isSupportedSourceFileName(normal[id].name, options));
    assert.deepEqual(row.selection, { root_unit_ids: roots, other_unit_ids: other,
      program_root_unit_ids: programRoots, vfs_write_order: [...roots, ...other] });
    assert.deepEqual(input.roots, programRoots.map(id => ts.getNormalizedAbsolutePath(normal[id].name, input.current_directory)));
    assert.deepEqual(input.files, row.selection.vfs_write_order.map(id => ({
      path: ts.getNormalizedAbsolutePath(normal[id].name, input.current_directory), text: normal[id].text,
    })));
  }
  const effective = Object.fromEntries(Object.entries(options).filter(([name, value]) =>
    !["configFilePath", "configFile"].includes(name) && value !== undefined).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0));
  assert.deepEqual(effective, row.effective_declaration_options, row.case_id);
  const parent = prior.find(entry => entry.case_id === row.case_id);
  if (parent) assert.deepEqual(effective, parent.effective_declaration_options);
  return options;
}

function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category],
    file: d.file?.fileName ?? null, start: d.start ?? null, length: d.length ?? null,
    message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [fileName, text, bom, onError, sourceFiles, data] = args;
  const callback = Buffer.from(text, "utf8");
  const materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), callback]) : callback;
  return { index, path: fileName, kind: ts.isDeclarationFileName(fileName) ? "declaration"
      : fileName.endsWith(".map") ? "source-map" : "javascript",
    callback_utf8_base64: callback.toString("base64"), callback_utf8_bytes: callback.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"),
    materialized_utf8_bytes: materialized.length, on_error_callback_present: onError !== undefined,
    source_files: sourceFiles?.map(file => file.fileName) ?? null,
    data_present: data !== undefined, data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null };
}
function observe(row, options) {
  const input = row.replay_input, cwd = input.current_directory;
  const files = new Map(input.files.map(file => [file.path, file.text]));
  const base = ts.createCompilerHost(options, true);
  // Physical fallback is confined to the pinned standard libraries.
  const libraryRoot = ts.normalizePath(path.join(root, "vendor/typescript-6.0.3/lib"));
  const isLibrary = name => ts.normalizePath(name).startsWith(libraryRoot + "/");
  const fallback = { directoryExists: name => isLibrary(name + "/") && base.directoryExists(name),
    getDirectories: name => isLibrary(name + "/") ? base.getDirectories(name) : [] };
  const overlay = createHermeticDirectoryOverlay(files.keys(), {
    currentDirectory: cwd, useCaseSensitiveFileNames: true, fallbackHost: fallback });
  const host = { ...base, ...overlay,
    getCurrentDirectory: () => cwd, useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: name => name, trace() {},
    fileExists: name => files.has(ts.normalizePath(name)) || (isLibrary(name) && base.fileExists(name)),
    readFile: name => files.get(ts.normalizePath(name)) ?? (isLibrary(name) ? base.readFile(name) : undefined),
    realpath: name => ts.normalizePath(name),
    getSourceFile(name, languageVersion) {
      const text = files.get(ts.normalizePath(name));
      return text === undefined ? (isLibrary(name) ? base.getSourceFile(name, languageVersion) : undefined)
        : ts.createSourceFile(ts.normalizePath(name), text, languageVersion, true, ts.getScriptKindFromFileName(name));
    },
  };
  if (input.default_library_file_name) host.getDefaultLibFileName = () => ts.combinePaths(libraryRoot, input.default_library_file_name);
  const program = ts.createProgram(input.roots, options, host);
  for (const source of program.getSourceFiles()) {
    assert.ok(files.has(source.fileName) || isLibrary(source.fileName), "unrecorded Program source " + source.fileName);
  }
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program,
    d => reported.push(diagnostic(d)), text => status.push(text), undefined,
    (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  assert.ok(result.sourceMaps === undefined || result.sourceMaps.length === 0, "maps remain a later slice");
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps ?? null },
    status_writes: status, exit_code: exit };
}

const cases = selected.map((selectedRow, index) => {
  const suite = selectedRow.case_id.split("/")[1];
  const raw = bytes(`ts-tests/tests/cases/${suite}/${selectedRow.source.path}`);
  assert.equal(raw.length, selectedRow.source.bytes);
  assert.equal(sha256(raw), selectedRow.source.sha256);
  const row = inputById.get(selectedRow.case_id);
  if (!row) return { ...selectedRow, disposition: "deferred", replay_input: null, typescript_observation: null };
  assert.deepEqual(row.source, selectedRow.source);
  assert.equal(row.origin, selectedRow.origin);
  const options = verifyInput(row, raw.toString("utf8"));
  const remaining = [...selectedRow.remaining_slices];
  if (options.rootDir !== undefined) remaining.push("H2.8a");
  assert.deepEqual(row.original_deferred_slices, ["H2.7c", ...remaining]);
  assert.equal(row.rust_expected_unsupported_option, remaining.length ? "rootDir" : undefined);
  const first = observe(row, options);
  assert.deepEqual(observe(row, options), first, row.case_id);
  console.log(`${index + 1}/42 H2.7c ${row.case_id}: ${remaining.length ? "boundary reference" : "exact candidate"}`);
  return { ...row, remaining_slices: remaining,
    disposition: remaining.length ? "deferred" : "exact", typescript_observation: first };
});
assert.equal(cases.filter(row => row.disposition === "exact").length, 31);
assert.equal(cases.filter(row => row.disposition === "deferred").length, 11);

const focused = [
  ["strip-internal", 15], ["declaration-blocking", 22], ["isolated-declaration-inference", 12],
  ["isolated-declaration-parameters", 21], ["isolated-declaration-accessors", 18],
  ["isolated-declaration-enums", 21], ["isolated-declaration-expando-augmentation", 27],
  ["isolated-declaration-private-types", 18], ["declaration-dir", 34],
  ["declaration-getters", 19], ["forced-declarations", 37],
].map(([name, count]) => {
  const observer = `scripts/observe-${name}.mjs`;
  const fixture = `crates/compiler/tests/fixtures/${name}.json`;
  const result = spawnSync(process.execPath, [observer, "--check"], { cwd: root, stdio: "inherit" });
  assert.equal(result.status, 0, `${observer}: ${result.error ?? result.signal ?? "failed"}`);
  const observation = read(fixture);
  assert.equal(observation.cases.length, count);
  assert.equal(observation.repetitions, 2);
  return { observer: identity(observer), fixture: identity(fixture), cases: count,
    typed_refusals: observation.cases.filter(row => row.rust_expected_unsupported_option).length };
});
const artifact = { schema: 1, kind: "h2-7c-qualification", status: "qualified-typescript-oracle",
  typescript: ts.version, source_commit: inputs.source_commit, repetitions: 2,
  generator: identity(generator), contract: identity(contract),
  inputs: [inputPath, parentPath, globalPath, ...expansionPaths, configPath,
    "crates/oracle/vfs-directory-overlay.mjs", ".node-version",
    "vendor/typescript-6.0.3/lib/typescript.js", "vendor/typescript-6.0.3/lib/_tsc.js"].map(identity),
  selection_contract: "26 H2.7b first-owner H2.7c rows plus 16 global required-slice H2.7c rows. Effective rootDir adds one later intersection. Focused controls and ordinary force=false references are separate denominators.",
  execution_contract: "Whole Program emitFilesAndReportErrorsAndGetExitStatus, original options/roots/config metadata, two complete observations. Rust compares 31 exact corpus rows and one typed rootDir refusal; ten other later intersections are count-only. Focused API calls retain their own observers. This artifact does not certify a Rust run or hosted admission.",
  cases, focused,
  summary: { corpus: 42, exact: 31, deferred: 11, boundary_probes: 1,
    focused: 244, focused_exact: 241, focused_typed_refusals: 3,
    adjacent_ordinary_api_references: 37, adjacent_ordinary_api_owner: "H2.8d" } };
artifact.qualification_fingerprint_sha256 = sha256(JSON.stringify(artifact));
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (mode === "--write") fs.writeFileSync(path.join(root, target), rendered);
else assert.equal(fs.readFileSync(path.join(root, target), "utf8"), rendered, "H2.7c artifact is stale");
console.log("H2.7c oracle: 42 corpus (31 exact candidates, 11 deferred), 244 focused, repetitions=2");
