// Local complete-command evidence for the exact captured Established projection.
// This is not an assertion that Established equals the original upstream options.
import assert from "node:assert/strict";
import {execFileSync} from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import {pathToFileURL} from "node:url";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import {sha256, documentPool, prepare} from "./recovery-command-input.mjs";

const codeRoot = path.resolve(import.meta.dirname, "..");
const optionFields = JSON.parse(fs.readFileSync(new URL("./recovery-command-options.json", import.meta.url)));
export const value = text => ({utf16: Array.from({length: text.length}, (_, i) => text.charCodeAt(i)), utf8_base64: Buffer.from(text).toString("base64")});
export const diagnostic = d => ({code: d.code, category: d.category, file: d.file ? value(d.file.fileName) : null,
  start: d.start ?? null, length: d.length ?? null, message: value(ts.flattenDiagnosticMessageText(d.messageText, "\n")),
  related_information: d.relatedInformation?.map(diagnostic) ?? null});

export function optionSnapshot(options) {
  const result = {};
  for (const {name, type} of optionFields) {
    const raw = options[name];
    if (type === "bool") result[name] = name === "allowJs" ? (raw ?? options.checkJs ?? false) : (raw ?? false);
    else if (type === "Option<JsString>") result[name] = raw == null ? null : value(raw);
    else if (["Option<Vec<JsString>>", "Option<Vec<ModuleSuffix>>"].includes(type)) result[name] = raw == null ? null : raw.map(v => v == null ? null : value(v));
    else result[name] = raw ?? null;
  }
  for (const name of ["noLib", "preserveSymlinks"]) result[name] = options[name] ?? null;
  for (const name of ["types", "typeRoots", "rootDirs"]) result[name] = options[name]?.map(value) ?? null;
  result.configFilePath = options.configFilePath == null ? null : value(options.configFilePath);
  return result;
}

export function complete(program) {
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined, "multiple emit calls"); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program,
    d => reported.push(diagnostic(d)), text => status.push(value(text)), undefined,
    (name, text, bom, onError, sources, data) => {
      if (data !== undefined) assert.deepEqual(Object.keys(data), ["sourceMapUrlPos", "diagnostics"], "unsupported callback metadata");
      writes.push({index: writes.length, path: value(name), callback: value(text), write_byte_order_mark: bom,
        materialized_utf8_base64: Buffer.from((bom ? "\uFEFF" : "") + text).toString("base64"),
        on_error_callback_present: onError !== undefined, source_files: sources?.map(source => value(source.fileName)) ?? null,
        data_present: data !== undefined, data_keys: data === undefined ? null : Object.keys(data),
        data_source_map_url_pos: data?.sourceMapUrlPos ?? null, data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null});
    });
  assert.ok(result, "complete command never invoked emit");
  return {writes, reported_diagnostics: reported, status_writes: status, exit_code: exit,
    emit_result: {emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      emitted_files: result.emittedFiles?.map(value) ?? null,
      source_maps: result.sourceMaps?.map(map => {
        assert.deepEqual(Object.keys(map).sort(), ["inputSourceFileNames", "sourceMap"]);
        return {input_source_file_names: map.inputSourceFileNames.map(value), source_map_json: value(JSON.stringify(map.sourceMap))};
      }) ?? null}};
}

export function observe(row, pool, libraryRoot) {
  const input = prepare(row, pool, libraryRoot);
  const program = ts.createProgram({rootNames: input.layout.roots, options: input.options, host: input.host,
    configFileParsingDiagnostics: input.errors});
  const actual = program.getSourceFiles().map(source => ({path: source.fileName, sha256: sha256(Buffer.from(source.text))}));
  const expected = row.command_input.prepared.source_files.map(({path, sha256}) => ({path, sha256}));
  const sorted = entries => [...entries].sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
  assert.deepEqual(sorted(actual), sorted(expected), `${row.case_id}: TypeScript loaded source paths/bytes differ; input reconstruction must be resolved before emitter comparison`);
  const options = optionSnapshot(program.getCompilerOptions());
  let command;
  try { command = complete(program); }
  catch (error) { command = {observer_unsupported: String(error)}; }
  return {options, decisions: input.decisions, loaded_sources: actual, command};
}

const git = (root, args) => execFileSync("git", args, {cwd: root, encoding: "utf8"}).trim();
function validateCode() {
  assert.equal(git(codeRoot, ["diff", "--name-only", "HEAD"]), "", "observer sources must be committed");
  assert.equal(git(codeRoot, ["ls-files", "--others", "--exclude-standard", "--", "crates", "scripts"]), "", "observer code/fixtures must be tracked");
  const pins = JSON.parse(fs.readFileSync(path.join(codeRoot, "scripts/recovery-command-pins.json")));
  for (const [name, hash] of Object.entries(pins)) assert.equal(sha256(fs.readFileSync(path.join(codeRoot, name))), hash, `${name}: review input mirror after loader changes`);
  return git(codeRoot, ["rev-parse", "HEAD"]);
}
function validateData(selection, inputRoot) {
  git(inputRoot, ["merge-base", "--is-ancestor", selection.head, "HEAD"]);
  const paths = ["vendor/typescript-6.0.3", "ts-tests", ...Object.keys(selection.input_manifest).filter(p => p.startsWith("ratchets/"))];
  paths.push(selection.input_manifest.recorded_execution_plans.manifest);
  for (const prefix of [["diff", "--name-only", selection.head, "HEAD", "--"], ["diff", "--name-only", "HEAD", "--"], ["ls-files", "--others", "--exclude-standard", "--"]]) {
    assert.equal(git(inputRoot, [...prefix, ...paths]), "", "input workspace data changed");
  }
  for (const root of [codeRoot, inputRoot]) assert.equal(git(root, ["rev-parse", "HEAD:vendor/typescript-6.0.3"]), selection.vendor_tree_hash);
  assert.equal(sha256(fs.readFileSync(path.join(inputRoot, selection.input_manifest.recorded_execution_plans.manifest))), selection.plan_manifest_sha256);
  for (const [name, pin] of Object.entries(selection.input_manifest)) {
    if (["recorded_execution_plans", "typescript_parity"].includes(name)) continue;
    assert.ok(/^ratchets\/h2-[a-z0-9-]+\.v1\.json$/.test(name), "unexpected artifact input path");
    assert.equal(sha256(fs.readFileSync(path.join(inputRoot, name))), pin.sha256, name);
  }
}

function main() {
  const [selectionPath, out, dataWorkspace, ...extra] = process.argv.slice(2);
  assert.ok(selectionPath && out && dataWorkspace && !extra.length,
    "usage: node scripts/observe-recovery-selected-corpus.mjs SELECTION OUTPUT INPUT_WORKSPACE");
  assert.ok(!fs.existsSync(out), "refusing to overwrite TypeScript observations");
  const bytes = fs.readFileSync(selectionPath), selection = JSON.parse(bytes);
  const head = validateCode();
  assert.equal(selection.schema, 1); assert.equal(selection.kind, "emitter-recovery-corpus-selection");
  const inputRoot = fs.realpathSync(dataWorkspace);
  const libraryRoot = fs.realpathSync(path.join(inputRoot, "vendor/typescript-6.0.3/lib"));
  assert.equal(sha256(fs.readFileSync(path.join(libraryRoot, "typescript.js"))), sha256(fs.readFileSync(path.join(codeRoot, "vendor/typescript-6.0.3/lib/typescript.js"))));
  validateData(selection, inputRoot);
  assert.equal(validateCode(), head, "observer HEAD changed during execution");
  const pool = documentPool(selection.documents), cases = [], ids = new Set();
  for (const row of selection.cases) {
    assert.ok(!ids.has(row.case_id), "duplicate selected case"); ids.add(row.case_id);
    const fallback = ["load_compiler_no_emit", "load_project_no_emit"].includes(row.loader);
    assert.equal(row.emit_disposition, fallback ? "parse-admission-only; emit-not-qualified" : "pending-complete-command-comparison");
    assert.ok(fallback ? typeof row.emit_load_error === "string" && row.emit_load_error.length : row.emit_load_error === null);
    const first = observe(row, pool, libraryRoot), second = observe(row, pool, libraryRoot);
    assert.deepEqual(first, second, `${row.case_id}: TypeScript command repetition differs`);
    // Preserve canonical serde JSON order for the native input hash.
    const canonical = v => Array.isArray(v) ? v.map(canonical) : v && typeof v === "object" ? Object.fromEntries(Object.keys(v).sort().map(k => [k, canonical(v[k])])) : v;
    cases.push({case_id: row.case_id, input_sha256: sha256(JSON.stringify(canonical(row.command_input))),
      disposition: fallback ? "parse-admission-only; emit-not-qualified" : first.command.observer_unsupported ? "observer-unsupported; emit-not-qualified" : "observed-twice; pending-native-comparison",
      options: first.options, directive_decisions: first.decisions, loaded_sources: first.loaded_sources,
      complete_command_runs: [first.command, second.command]});
    console.log(`${cases.length}/${selection.cases.length} ${row.case_id}: ${cases.at(-1).disposition}`);
  }
  validateData(selection, inputRoot);
  assert.equal(validateCode(), head, "observer HEAD changed during execution");
  const dependencies = ["scripts/recovery-command-input.mjs", "scripts/recovery-command-options.json", "scripts/observe-recovery-selected-corpus.mjs", "crates/harness/src/upstream_suites/execution.rs", "crates/harness/src/upstream_suites/execution/project.rs", "crates/oracle/vfs-directory-overlay.mjs"];
  const result = {schema: 1, kind: "emitter-recovery-typescript-observations", typescript: ts.version, head,
    scope: "Exact captured Established projection; local evidence with the census library mount",
    input_workspace: inputRoot, library_root: libraryRoot, selection_sha256: sha256(bytes), repetitions: 2,
    dependencies: Object.fromEntries(dependencies.map(name => [name, sha256(fs.readFileSync(path.join(codeRoot, name)))])),
    compiler_sha256: sha256(fs.readFileSync(path.join(libraryRoot, "typescript.js"))),
    load_failures: selection.load_failures, cases};
  fs.mkdirSync(path.dirname(out), {recursive: true});
  fs.writeFileSync(out, JSON.stringify(result) + "\n", {flag: "wx"});
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) main();
