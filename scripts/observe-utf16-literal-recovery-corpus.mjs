// Complete-command observations for corpus rows newly admitted by the parser-owned
// literal-only recovery predicate (handoff §6 step 4). Inputs come from the census
// row's own artifact (the qualified VFS the acceptance loader rebuilds), options
// from the same directive projection and established floor the Rust harness applies,
// and the program from a hermetic VFS host; two repetitions must agree.
// node scripts/observe-utf16-literal-recovery-corpus.mjs --census <census.json> --write|--check
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const args = process.argv.slice(2);
const censusIndex = args.indexOf("--census");
assert.ok(censusIndex >= 0 && args[censusIndex + 1], "--census <path> is required");
const mode = args.find(arg => arg === "--write" || arg === "--check");
assert.ok(mode, "--write or --check is required");
const censusPath = path.resolve(root, args[censusIndex + 1]);
const censusBytes = fs.readFileSync(censusPath);
const census = JSON.parse(censusBytes);
assert.equal(census.kind, "utf16-literal-recovery-admission-census");

const units = text => Array.from({length: text.length}, (_, index) => text.charCodeAt(index));
const value = text => ({utf16: units(text), utf8_base64: Buffer.from(text).toString("base64")});
const diagnostic = d => ({code: d.code, category: d.category, file: d.file?.fileName ?? null,
  start: d.start ?? null, length: d.length ?? null,
  message: value(ts.flattenDiagnosticMessageText(d.messageText, "\n")),
  related_information: d.relatedInformation?.map(diagnostic) ?? null});

// Harness directive projection (crates/oracle/h2-5g-qualification.mjs effectiveCompilerOptions).
const HARNESS_ONLY_OPTIONS = new Set(["useCaseSensitiveFileNames", "baselineFile", "fileName",
  "suppressOutputPathCheck", "noImplicitReferences", "currentDirectory", "symlink", "link",
  "noTypesAndSymbols", "fullEmitPaths", "reportDiagnostics", "captureSuggestions", "typeScriptVersion"]
  .map(name => name.toLowerCase()));
const OPTION_INDEX = new Map(ts.optionDeclarations.map(option => [option.name.toLowerCase(), option]));
// EmitOptionFloor::Established (crates/harness/src/upstream_suites/execution.rs
// apply_compiler_setting / apply_emit_option_floor_to_config): dropped names.
const ESTABLISHED_FLOOR_DROPS = ["sourceMap", "inlineSourceMap", "inlineSources", "sourceRoot", "mapRoot",
  "emitBOM", "emitDeclarationOnly", "declarationMap", "outFile", "noEmitHelpers", "outDir", "declarationDir",
  "incremental", "assumeChangesOnlyAffectDirectDependencies", "stripInternal", "disableSizeLimit", "out",
  "rootDir", "tsBuildInfoFile", "pretty", "traceResolution", "listFilesOnly", "captureSuggestions",
  "stableTypeOrdering", "noCheck"];
function optionValue(option, raw) {
  const errors = [];
  let parsed;
  if (option.type === "boolean") parsed = raw.toLowerCase() === "true";
  else if (option.type === "string") parsed = raw;
  else if (option.type === "number") parsed = Number.parseInt(raw, 10);
  else if (option.type === "list" || option.type === "listOrElement") parsed = ts.parseListTypeOption(option, raw, errors);
  else parsed = ts.parseCustomTypeOption(option, raw, errors);
  assert.equal(errors.length, 0, `invalid @${option.name}: ${raw}`);
  return parsed;
}
function effectiveCompilerOptions(settings) {
  const options = ts.cloneCompilerOptions({noResolve: false});
  options.newLine = ts.NewLineKind.CarriageReturnLineFeed;
  options.noErrorTruncation = true;
  options.skipDefaultLibCheck = true;
  for (const [name, raw] of settings) {
    if (name === "typeScriptVersion") continue;
    const option = OPTION_INDEX.get(name.toLowerCase());
    if (option) { options[option.name] = optionValue(option, raw); continue; }
    assert.ok(HARNESS_ONLY_OPTIONS.has(name.toLowerCase()), `unknown harness/compiler option @${name}`);
  }
  for (const name of ESTABLISHED_FLOOR_DROPS) delete options[name];
  return options;
}
const serialOptions = options => Object.fromEntries(Object.entries(options)
  .filter(([name, v]) => !["configFile", "configFilePath"].includes(name) && v !== undefined)
  .sort(([a], [b]) => a.localeCompare(b, "en")));

const artifacts = new Map();
function artifact(relative) {
  if (!artifacts.has(relative)) artifacts.set(relative, JSON.parse(fs.readFileSync(path.join(root, relative))));
  return artifacts.get(relative);
}
// Qualification artifacts that embed the exact qualified VFS (same list as the census).
const QUALIFIED_INPUT_ARTIFACTS = ["h2-1a", "h2-1b", "h2-1c", "h2-1d", "h2-1e", "h2-2a", "h2-2b", "h2-2c", "h2-2d",
  "h2-3a", "h2-3b", "h2-3c", "h2-4a", "h2-4b", "h2-5a", "h2-5b", "h2-5c", "h2-5d", "h2-5e", "h2-5f", "h2-5g", "h2-5h",
  "h2-6a", "h2-6b", "h2-6c", "h2-7b"].map(name => `ratchets/${name}-qualification.v1.json`);
function locate(row) {
  if (row.universe !== "recorded-execution-plans") {
    const found = artifact(row.universe).cases.find(entry => entry.case_id === row.case_id);
    assert.ok(found, `${row.universe}: ${row.case_id}`);
    return {universe: row.universe, found};
  }
  // A recorded-plan row was claimed by the census before its artifact; the
  // embedded qualified input of the same case id is the observation input.
  for (const relative of QUALIFIED_INPUT_ARTIFACTS) {
    const found = artifact(relative).cases.find(entry => entry.case_id === row.case_id && entry.input?.files?.length);
    if (found) return {universe: relative, found};
  }
  return null;
}
function inputFor(row) {
  const located = locate(row);
  if (!located) return null;
  const {universe, found} = located;
  row = {...row, input_universe: universe};
  const input = found.input;
  const files = [];
  if (universe.includes("candidate-inputs")) {
    assert.equal(input.route, "whole-program");
    assert.equal(input.shared_mount, null, `${row.case_id}: project mounts are outside this observer`);
    for (const file of input.files) files.push({path: file.path, text: file.text});
    if (input.config && !files.some(file => file.path === input.config.path)) files.push({path: input.config.path, text: input.config.text});
    return {current_directory: input.current_directory, roots: input.roots, files,
      settings: found.settings.map(([name, raw]) => [name, raw]), vfs_symlinks: input.vfs_symlinks ?? []};
  }
  const decode = file => {
    const bytes = Buffer.from(file.utf8_base64, "base64");
    assert.equal(bytes.length, file.utf8_bytes, file.path);
    assert.equal(sha(bytes), file.utf8_sha256, file.path);
    return {path: file.path, text: bytes.toString("utf8")};
  };
  for (const file of input.files) files.push(decode(file));
  if (input.virtual_config) files.push(decode(input.virtual_config));
  return {current_directory: input.current_directory, roots: input.roots, files,
    settings: input.settings.map(setting => [setting.name, setting.value]),
    vfs_symlinks: (input.vfs_symlinks ?? []).map(link => ({link_path: link.link_path, target_path: link.target_path}))};
}

// Hermetic VFS host (crates/oracle/h2-5g-qualification.mjs createProgramCase), case-sensitive
// like the Rust qualified loader; the default library resolves through the base host.
function complete(input, options) {
  const cwd = input.current_directory;
  const vfsByPath = new Map(input.files.map(file => [ts.normalizePath(file.path), file.text]));
  const symlinkByPath = new Map();
  for (const link of input.vfs_symlinks) {
    const linkPath = ts.normalizePath(link.link_path), target = ts.normalizePath(link.target_path);
    assert.ok(vfsByPath.has(target), `${link.target_path} is absent from the VFS`);
    if (!vfsByPath.has(linkPath)) vfsByPath.set(linkPath, vfsByPath.get(target));
    symlinkByPath.set(linkPath, target);
  }
  const baseHost = ts.createCompilerHost(options, true);
  const overlay = createHermeticDirectoryOverlay(vfsByPath.keys(), {currentDirectory: cwd, useCaseSensitiveFileNames: true, fallbackHost: baseHost});
  const host = {...baseHost,
    getCurrentDirectory: () => cwd, useCaseSensitiveFileNames: () => true, getCanonicalFileName: name => name, trace() {},
    fileExists: name => vfsByPath.has(ts.normalizePath(name)) || baseHost.fileExists(ts.normalizePath(name)),
    readFile: name => vfsByPath.get(ts.normalizePath(name)) ?? baseHost.readFile(ts.normalizePath(name)),
    directoryExists: directory => overlay.directoryExists(directory),
    getDirectories: directory => overlay.getDirectories(directory),
    realpath(name) { const normalized = ts.normalizePath(name); if (symlinkByPath.has(normalized)) return symlinkByPath.get(normalized);
      return vfsByPath.has(normalized) ? normalized : (baseHost.realpath?.(normalized) ?? normalized); },
    getSourceFile(name, version) { const normalized = ts.normalizePath(name); const text = vfsByPath.get(normalized);
      if (text === undefined) return baseHost.getSourceFile(name, version);
      return ts.createSourceFile(normalized, text, version, true, ts.getScriptKindFromFileName(normalized)); },
    writeFile: () => assert.fail("writes must use the captured callback")};
  const program = ts.createProgram(input.roots, options, host);
  const writes = [], diagnostics = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...emitArgs) => {assert.equal(result, undefined); return result = emit(...emitArgs);};
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program,
    d => diagnostics.push(diagnostic(d)), text => status.push(value(text)), undefined,
    (name, text, bom, onError, sources, data) => writes.push({
      index: writes.length, path: name, callback: value(text), write_byte_order_mark: bom,
      materialized_utf8_base64: Buffer.from((bom ? "\uFEFF" : "") + text).toString("base64"),
      on_error_callback_present: onError !== undefined,
      source_files: sources?.map(s => s.fileName) ?? null,
      data_present: data !== undefined, data_keys: data === undefined ? null : Object.keys(data),
      data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
      data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null}));
  assert.ok(result);
  const parseDiagnosticUnits = program.getSourceFiles().filter(source => !source.isDeclarationFile && source.parseDiagnostics.length)
    .map(source => ({path: source.fileName, codes: source.parseDiagnostics.map(d => d.code)}));
  return {parse_diagnostic_units: parseDiagnosticUnits, writes, reported_diagnostics: diagnostics, status_writes: status, exit_code: exit,
    emit_result: {emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps?.map(entry => ({
        input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap)})) ?? null}};
}

const cases = [], skipped = [];
for (const row of census.newly_admitted) {
  const located = locate(row);
  const input = inputFor(row);
  if (!input) { skipped.push({case_id: row.case_id, universe: row.universe, reason: "no artifact embeds a qualified input for this case id"}); continue; }
  const options = effectiveCompilerOptions(input.settings);
  const runs = [complete(input, options), complete(input, options)];
  assert.deepEqual(runs[0], runs[1], row.case_id);
  assert.ok(runs[0].parse_diagnostic_units.length, `${row.case_id}: TypeScript retains a parse diagnostic on this row`);
  cases.push({case_id: row.case_id, suite: row.suite, universe: row.universe, input_universe: located.universe, census_units: row.units,
    input: {...input, files: input.files.map(file => ({...file, sha256: sha(Buffer.from(file.text))})), use_case_sensitive_file_names: true, floor: "established"},
    effective_options: serialOptions(options), complete_command_runs: runs});
}
const fixture = {version: 1,
  scope: "Corpus rows newly admitted by literal-only parser recovery: upstream complete command observations under the established qualified-loader floor; native outcome compared separately",
  typescript: ts.version, compiler_sha256: compilerSha, observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  census: {path: path.relative(root, censusPath), sha256: sha(censusBytes), newly_admitted: census.newly_admitted.length},
  repetitions: 2, complete_command_executions: cases.length * 2, skipped, cases};
const output = path.join(root, "crates/compiler/tests/fixtures/utf16-literal-recovery-corpus.json");
if (mode === "--write") fs.writeFileSync(output, JSON.stringify(fixture, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)), fixture);
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length, skipped: skipped.length, complete_command_executions: cases.length * 2}));
