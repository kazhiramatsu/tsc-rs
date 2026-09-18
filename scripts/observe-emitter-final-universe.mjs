// EF7 (H2.8a-A-RES-EMITTER-FINAL): fresh complete TypeScript 6.0.3 observations for the
// 217 PLAN-BASE case IDs that no emit profile ever selected (`universe:H2.0a`, ledger state RM).
// Inputs are built exactly like the H2.8a original candidates (the directive-input builder
// below is a verbatim copy of crates/oracle/h2-8a-candidates.mjs and is self-checked against
// the frozen ratchets/h2-8a-candidate-inputs.v1.json rows); each Program is observed twice
// through emitFilesAndReportErrorsAndGetExitStatus. This is a TypeScript reference packet,
// not a Rust result: the Rust replay lives in crates/compiler/tests/emitter_final_universe.rs.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
import { root, sha256, read, identity, parseConfig, persist } from "../crates/oracle/h2-8a-candidates.mjs";

const mode = process.argv[2];
assert.ok(["--write", "--check", "--self-check"].includes(mode), "use --write, --check or --self-check");
// Two id sets share this observer: the 217 previously-unobserved IDs (default) and the
// PLAN-BASE IDs never observed by any profile (`--set plan-base`, compiler/conformance rows).
const set = process.argv[3] === "--set" ? process.argv[4] : "217";
assert.ok(["217", "plan-base"].includes(set), "use --set 217|plan-base");
const idsPath = set === "217" ? "docs/design/greenfield/slices/emitter-final-batch/ef7/universe-217.v1.json"
  : "docs/design/greenfield/slices/emitter-final-batch/ef7/universe-plan-base.v1.json";
const destination = set === "217" ? "crates/compiler/tests/fixtures/emitter-final-universe.json"
  : "crates/compiler/tests/fixtures/emitter-final-universe-plan-base.json";

// ---- verbatim from crates/oracle/h2-8a-candidates.mjs (not exported there) ----
const ordered = value => Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b, "en")));
const unique = values => [...new Set(values)].sort();
const optionsByName = new Map(ts.optionDeclarations.map(option => [option.name.toLowerCase(), option]));
const harnessNames = new Set(["usecasesensitivefilenames", "baselinefile", "filename", "suppressoutputpathcheck",
  "noimplicitreferences", "currentdirectory", "symlink", "link", "notypesandsymbols", "fullemitpaths",
  "reportdiagnostics", "capturesuggestions", "typescriptversion"]);
function setOption(options, name, raw) {
  const option = optionsByName.get(name.toLowerCase());
  if (!option) { assert.ok(harnessNames.has(name.toLowerCase()), name); return; }
  const errors = [];
  options[option.name] = option.type === "boolean" ? String(raw).toLowerCase() === "true"
    : option.type === "string" ? String(raw) : option.type === "number" ? Number.parseInt(raw, 10)
    : ["list", "listOrElement"].includes(option.type) ? ts.parseListTypeOption(option, String(raw), errors)
    : ts.parseCustomTypeOption(option, String(raw), errors);
  assert.deepEqual(errors, [], name);
}
function serialOptions(options) {
  return ordered(Object.fromEntries(Object.entries(options).filter(([name, value]) =>
    !["configFile", "configFilePath"].includes(name) && value !== undefined)));
}
function pinnedSource(suite, source) {
  const name = `ts-tests/tests/cases/${suite}/${source.path}`;
  const bytes = fs.readFileSync(path.join(root, name));
  assert.equal(bytes.length, source.bytes, name);
  assert.equal(sha256(bytes), source.sha256, name);
  assert.equal(crypto.createHash("sha1").update(`blob ${bytes.length}\0`).update(bytes).digest("hex"), source.git_blob_sha1, name);
  return ts.sys.readFile(path.join(root, name));
}

// Preserve the pinned compiler runner's directive removal and newline rules.
function unitsFromSource(text, name) {
  const units = [], links = [];
  let currentName, content, settings = {};
  const flush = () => units.push({ name: currentName, text: content || "", file_options: Object.entries(settings).map(([name, value]) => ({ name, value })) });
  for (const line of text.split(/\r?\n/)) {
    const link = /^\/{2}\s*@link\s*:\s*([^\r\n]*)\s*->\s*([^\r\n]*)/.exec(line);
    if (link) { links.push({ target: link[1].trim(), link_path: link[2].trim() }); continue; }
    const option = /^\/{2}\s*@([\w]+)\s*:\s*([^\r\n]*)/.exec(line);
    if (option) {
      settings[option[1]] = option[2].trim();
      if (option[1].toLowerCase() !== "filename") continue;
      if (currentName !== undefined) { flush(); content = undefined; settings = {}; }
      else { assert.ok(!content || ts.skipTrivia(content, 0, false, false) === content.length); content = ""; }
      currentName = option[2].trim();
      continue;
    }
    if (content === undefined) content = "";
    else if (content !== "") content += "\n";
    content += line;
  }
  currentName ??= path.posix.basename(name);
  flush();
  return { units, links };
}
function verifyUnits(units, recorded) {
  assert.equal(units.length, recorded.length);
  for (const [index, unit] of units.entries()) {
    const expected = recorded[index];
    assert.equal(unit.name, expected.name);
    assert.deepEqual(unit.file_options, expected.file_options);
    assert.equal(Buffer.byteLength(unit.text), expected.content.utf8_bytes);
    assert.equal(sha256(unit.text), expected.content.sha256);
  }
}

function directiveInput(row, expansion, configPlans) {
  const entry = expansion.cases.find(entry => entry.id === row.id);
  assert.ok(entry, row.id);
  const fixture = (row.suite === "compiler" ? expansion.compiler_fixtures : expansion.fixtures).find(f => f.source === entry.source);
  const source = expansion.sources[entry.source];
  for (const key of Object.keys(row.source)) assert.deepEqual(source[key], row.source[key]);
  const text = pinnedSource(row.suite, row.source);
  assert.equal(sha256(text), fixture.decoded_sha256);
  const { units, links } = unitsFromSource(text, row.source.path);
  assert.deepEqual(links, fixture.links);
  const configUnit = fixture.virtual_config ? units.find(unit => unit.name === fixture.virtual_config.name) : null;
  if (configUnit) verifyUnits([configUnit], [fixture.virtual_config]);
  const normal = units.filter(unit => unit !== configUnit);
  verifyUnits(normal, fixture.normal_units);
  const index = row.suite === "compiler" ? entry.configuration.configuration : entry.configuration;
  const settings = new Map(fixture.settings.map(s => [s.name, s.value]));
  for (const setting of fixture.configurations[index].settings) settings.set(setting.name, setting.value);
  const cwd = ts.getNormalizedAbsolutePath(settings.get("currentDirectory") ?? "/.src", "/.src");
  const config = configUnit ? { path: ts.getNormalizedAbsolutePath(configUnit.name, cwd), text: configUnit.text } : null;
  const allFiles = units.map(unit => ({ path: ts.getNormalizedAbsolutePath(unit.name, cwd), text: unit.text }));
  const parsed = config ? parseConfig(config, allFiles, cwd, false) : null;
  const options = parsed ? ts.cloneCompilerOptions(parsed.options) : { noResolve: false };
  Object.assign(options, { newLine: ts.NewLineKind.CarriageReturnLineFeed, noErrorTruncation: true, skipDefaultLibCheck: true });
  for (const [name, raw] of settings) setOption(options, name, raw);
  const candidates = [...new Map(normal.map((unit, id) => [ts.getNormalizedAbsolutePath(unit.name, cwd), id])).values()].sort((a, b) => a - b);
  const last = candidates.at(-1);
  const implicit = settings.has("noImplicitReferences") || normal[last].text.includes("require(") || /reference\s+path/.test(normal[last].text);
  const roots = parsed ? candidates.filter(id => parsed.fileNames.includes(ts.getNormalizedAbsolutePath(normal[id].name, cwd))) : implicit ? [last] : candidates;
  const other = candidates.filter(id => !roots.includes(id));
  const programRoots = roots.filter(id => !normal[id].name.endsWith(".json") && ts.isSupportedSourceFileName(normal[id].name, options));
  if (parsed && row.suite === "compiler") {
    const plan = configPlans.fixtures.find(plan => plan.source.index === entry.source);
    assert.deepEqual(parsed.fileNames, plan.parsed_file_names);
    assert.deepEqual(programRoots.map(id => units.indexOf(normal[id])), plan.program_root_unit_ids);
  }
  const files = [...roots, ...other].map(id => ({ path: ts.getNormalizedAbsolutePath(normal[id].name, cwd), text: normal[id].text }));
  const symlinks = new Map();
  for (const link of links) {
    const target = ts.getNormalizedAbsolutePath(link.target, cwd), destination = ts.getNormalizedAbsolutePath(link.link_path, cwd);
    const matches = files.filter(file => file.path === target || file.path.startsWith(target + "/"));
    assert.ok(matches.length, link.target);
    for (const file of matches) symlinks.set(destination + file.path.slice(target.length), file.path);
  }
  for (const [id, unit] of normal.entries()) for (const link of fixture.normal_units[id].document_symlinks) symlinks.set(ts.getNormalizedAbsolutePath(link, cwd), ts.getNormalizedAbsolutePath(unit.name, cwd));
  return { settings: [...settings], selection: { root_unit_ids: roots, other_unit_ids: other,
    program_root_unit_ids: programRoots, vfs_write_order: [...roots, ...other] },
    effective_options: serialOptions(options), input: { route: "whole-program", current_directory: cwd,
      use_case_sensitive_file_names: settings.get("useCaseSensitiveFileNames")?.toLowerCase() !== "false",
      roots: programRoots.map(id => ts.getNormalizedAbsolutePath(normal[id].name, cwd)), files, config,
      vfs_symlinks: [...symlinks].map(([link_path, target_path]) => ({ link_path, target_path })),
      shared_mount: null, default_library_file_name: null }, project_descriptor: null };
}

// ---- verbatim from crates/oracle/h2-8a-observations.mjs (not exported there) ----
const libraryRoot = ts.normalizePath(path.join(root, "vendor/typescript-6.0.3/lib"));
const isLibrary = name => ts.normalizePath(name).startsWith(libraryRoot + "/");
// EF7: unlike the 809 H2.8a candidates, several universe rows report diagnostics located in
// (or naming) a standard library file. The Rust memory host mounts those files under /lib/,
// so the vendored library root is projected to that same mount here; every other path is the
// original virtual /.src path.
const projectLibraryPath = name => name === undefined || name === null ? null
  : isLibrary(name) ? "/lib/" + path.posix.basename(ts.normalizePath(name)) : name;
const projectLibraryText = text => text.split(libraryRoot + "/").join("/lib/");
function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: projectLibraryPath(d.file?.fileName),
    start: d.start ?? null, length: d.length ?? null, message: projectLibraryText(ts.flattenDiagnosticMessageText(d.messageText, "\n")),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [fileName, text, bom, onError, sourceFiles, data] = args;
  assert.ok(!text.includes(root), "local workspace path escaped into callback bytes");
  assert.ok(data === undefined || Object.keys(data).every(key => ["sourceMapUrlPos", "diagnostics", "buildInfo"].includes(key)), fileName);
  const callback = Buffer.from(text, "utf8"), materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), callback]) : callback;
  return { index, path: fileName, kind: ts.isDeclarationFileName(fileName) ? "declaration"
    : fileName.endsWith(".map") ? "source-map" : fileName.endsWith(".tsbuildinfo") ? "build-info" : "javascript",
    callback_utf8_base64: callback.toString("base64"), callback_utf8_bytes: callback.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sourceFiles?.map(file => file.fileName) ?? null,
    data_present: data !== undefined, data_keys: data === undefined ? null : Object.keys(data),
    data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null, data_build_info: data?.buildInfo ?? null };
}
function sourceMaps(maps) {
  return maps?.map(entry => {
    assert.deepEqual(Object.keys(entry).sort(), ["inputSourceFileNames", "sourceMap"]);
    return { input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap) };
  }) ?? null;
}

function observe(row) {
  const input = row.input, cwd = input.current_directory, caseSensitive = input.use_case_sensitive_file_names;
  const canonical = name => caseSensitive ? ts.getNormalizedAbsolutePath(name, cwd) : ts.getNormalizedAbsolutePath(name, cwd).toLowerCase();
  const originalFiles = [...(input.shared_mount ? inputs.shared_mounts[input.shared_mount] : []), ...input.files];
  if (input.config && !originalFiles.some(file => file.path === input.config.path)) originalFiles.push(input.config);
  const files = new Map(originalFiles.map(file => [canonical(file.path), file.text]));
  const symlinks = new Map(input.vfs_symlinks.map(link => [canonical(link.link_path), link.target_path]));
  for (const link of input.vfs_symlinks) { assert.ok(files.has(canonical(link.target_path))); files.set(canonical(link.link_path), files.get(canonical(link.target_path))); }
  const parsed = input.config ? parseConfig(input.config, originalFiles, cwd, row.suite === "project") : null;
  const options = { ...parsed?.options, ...row.effective_options };
  const base = ts.createCompilerHost(options, true);
  const fallback = { directoryExists: name => isLibrary(name + "/") && base.directoryExists(name),
    getDirectories: name => isLibrary(name + "/") ? base.getDirectories(name) : [] };
  const overlay = createHermeticDirectoryOverlay([...originalFiles.map(file => file.path), ...input.vfs_symlinks.map(link => link.link_path)],
    { currentDirectory: cwd, useCaseSensitiveFileNames: caseSensitive, fallbackHost: fallback });
  const host = { ...base, ...overlay, getCurrentDirectory: () => cwd, useCaseSensitiveFileNames: () => caseSensitive,
    getCanonicalFileName: name => caseSensitive ? name : name.toLowerCase(), trace() {},
    fileExists: name => files.has(canonical(name)) || (isLibrary(name) && base.fileExists(name)),
    readFile: name => files.get(canonical(name)) ?? (isLibrary(name) ? base.readFile(name) : undefined),
    realpath: name => symlinks.get(canonical(name)) ?? ts.normalizePath(name),
    getSourceFile(name, languageVersion) {
      const text = files.get(canonical(name));
      return text === undefined ? (isLibrary(name) ? base.getSourceFile(name, languageVersion) : undefined)
        : ts.createSourceFile(ts.normalizePath(name), text, languageVersion, true, ts.getScriptKindFromFileName(name));
    } };
  if (input.default_library_file_name) host.getDefaultLibFileName = () => ts.combinePaths(libraryRoot, input.default_library_file_name);
  const program = ts.createProgram(input.roots, options, host);
  for (const source of program.getSourceFiles()) assert.ok(files.has(canonical(source.fileName)) || isLibrary(source.fileName), source.fileName);
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)), text => status.push(text), undefined,
    (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  return { program_source_order: program.getSourceFiles().filter(source => !isLibrary(source.fileName)).map(source => source.fileName),
    standard_libraries: program.getSourceFiles().filter(source => isLibrary(source.fileName)).map(source => path.posix.basename(source.fileName)),
    writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      emitted_files: result.emittedFiles ?? null, source_maps: sourceMaps(result.sourceMaps) }, status_writes: status, exit_code: exit };
}

// ---- EF7 driver ----
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
const ids = read(idsPath);
const expectedCount = ids.count;
assert.equal(ids.cases.length, expectedCount);
const global = read("ratchets/h2-candidate-dispositions.v1.json");
const test = read("vendor/typescript-6.0.3/test-suite-expansion.v1.json");
const conformance = read("vendor/typescript-6.0.3/conformance-suite-expansion.v1.json");
const plans = read("vendor/typescript-6.0.3/compiler-config-plans.v1.json");
const byId = new Map(global.cases.map(row => [row.id, row]));

// Self-check: the copied builder reproduces every frozen compiler/conformance H2.8a input byte for byte.
const frozenInputs = read("ratchets/h2-8a-candidate-inputs.v1.json");
let verified = 0;
for (const frozen of frozenInputs.cases) {
  if (!["compiler", "conformance"].includes(frozen.suite)) continue;
  const row = byId.get(frozen.case_id);
  assert.ok(row, frozen.case_id);
  const built = { case_id: row.id, suite: row.suite, source: row.source, ...directiveInput(row, row.suite === "compiler" ? test : conformance, plans) };
  assert.deepEqual(built, frozen, frozen.case_id);
  verified++;
}
console.log(`builder self-check: ${verified} frozen compiler/conformance inputs reproduced`);
if (mode === "--self-check") process.exit(0);

const cases = [];
for (const entry of ids.cases) {
  const row = byId.get(entry.case_id);
  assert.ok(row, entry.case_id);
  assert.deepEqual(row.source, entry.source, entry.case_id);
  assert.deepEqual(row.profile_blockers, entry.profile_blockers, entry.case_id);
  const prepared = { case_id: row.id, suite: row.suite, source: row.source, ...directiveInput(row, row.suite === "compiler" ? test : conformance, plans) };
  assert.equal(prepared.input.route, "whole-program");
  const observation = observe(prepared);
  assert.deepEqual(observe(prepared), observation, prepared.case_id);
  cases.push({ ...prepared, profile_blockers: row.profile_blockers, required_slices: row.required_slices,
    ledger_owner: entry.ledger_owner, ledger_bucket: entry.ledger_bucket, expected_kinds: entry.expected_kinds,
    input_sha256: sha256(JSON.stringify(prepared)), repetitions: 2, typescript_observation: observation });
  console.log(`${cases.length}/${expectedCount} ${prepared.case_id} writes=${observation.writes.length} exit=${observation.exit_code}`);
}
assert.equal(cases.length, expectedCount);
for (const row of cases) assert.ok(!JSON.stringify(row).includes(root), "local workspace path escaped: " + row.case_id);
const optionNames = [...new Set(cases.flatMap(row => Object.keys(row.effective_options)))].sort();
const artifact = { schema: 1, kind: "emitter-final-universe", status: "typescript-reference-only", typescript: ts.version,
  source_commit: global.inputs?.find?.(i => i.path?.includes("test-suite-expansion"))?.source_commit ?? test.source_commit,
  repetitions: 2, generator: identity("scripts/observe-emitter-final-universe.mjs"),
  inputs: [idsPath, "ratchets/h2-candidate-dispositions.v1.json", "ratchets/h2-8a-candidate-inputs.v1.json",
    "vendor/typescript-6.0.3/test-suite-expansion.v1.json", "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json",
    "vendor/typescript-6.0.3/compiler-config-plans.v1.json", "crates/oracle/h2-8a-candidates.mjs",
    "crates/oracle/vfs-directory-overlay.mjs", "vendor/typescript-6.0.3/lib/typescript.js", ".node-version"].map(identity),
  id_set: set, execution_contract: `${expectedCount} ${set === "217" ? "previously unobserved" : "never-observed PLAN-BASE"} compiler/conformance IDs, fresh whole Program emitFilesAndReportErrorsAndGetExitStatus twice, one serial worker; complete diagnostics, writes/order/bytes/callback metadata, emittedFiles/sourceMaps absence versus empty, status and exit. noEmit rows record diagnostics, status and exit with an empty write set.`,
  cases, summary: { cases: expectedCount, repetitions: 2, typescript_runs: expectedCount * 2,
    writes: cases.reduce((count, row) => count + row.typescript_observation.writes.length, 0),
    no_emit_rows: cases.filter(row => row.effective_options.noEmit === true).length,
    emit_skipped: cases.filter(row => row.typescript_observation.emit_result.emit_skipped).length,
    nonzero_exit: cases.filter(row => row.typescript_observation.exit_code !== 0).length,
    effective_option_names: optionNames } };
// The plan-base set (≈12 MB) is stored zstd-compressed like the other multi-megabyte
// complete-command fixtures; every pin refers to the decoded bytes.
if (set === "plan-base") {
  const text = JSON.stringify(artifact, null, 2) + "\n";
  assert.ok(!text.includes(root), "local workspace path escaped into artifact");
  const compressed = path.join(root, destination + ".zst");
  if (mode === "--write") {
    const plain = path.join(root, destination);
    fs.writeFileSync(plain, text);
    execFileSync("zstd", ["-19", "-q", "-f", "--rm", "-o", compressed, plain]);
  } else {
    assert.equal(execFileSync("zstd", ["-d", "-c", compressed], { maxBuffer: 1 << 28 }).toString("utf8"), text, `${destination}.zst is stale`);
  }
} else {
  persist(destination, artifact, mode);
}
console.log(JSON.stringify(artifact.summary));
