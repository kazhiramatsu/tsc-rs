// Complete-command promotion of the immutable v1/v2 C witnesses and added boundaries.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
function parent(version, digest) {
  const bytes = fs.readFileSync(path.join(root, `crates/compiler/tests/fixtures/utf16-tagged-template-review-v${version}.json`));
  assert.equal(sha(bytes), digest);
  return JSON.parse(bytes);
}
const v1Sha = "92fbb23eafa07bfbf8c33de981453f32ee9c7abe8589886f288a3819c35537ca";
const v2Sha = "28b126f461adf71aa07a199d9cfb21713a78d4e297a7094806f767aba69d0df9";
const v1 = parent(1, v1Sha), v2 = parent(2, v2Sha);
const units = text => Array.from({length: text.length}, (_, index) => text.charCodeAt(index));
const value = text => ({utf16: units(text), utf8_base64: Buffer.from(text).toString("base64")});
const diagnostic = d => ({code: d.code, category: d.category, file: d.file?.fileName ?? null,
  start: d.start ?? null, length: d.length ?? null,
  message: value(ts.flattenDiagnosticMessageText(d.messageText, "\n")),
  related_information: d.relatedInformation?.map(diagnostic) ?? null});
const baseOptions = {...v2.options};
const inputs = [...v1.cases.map(c => ({id:c.id, source:c.source, target:c.target, parent_version:1})),
  ...v2.cases.map(c => ({id:c.id, source:c.source, target:baseOptions.target, parent_version:2}))];
function complete(source, options) {
  const fileName = "/project/main.ts";
  const base = ts.createCompilerHost(options, true);
  const host = {...base, getCurrentDirectory: () => "/project",
    getSourceFile: (name, version) => name === fileName
      ? ts.createSourceFile(name, source, version, true) : base.getSourceFile(name, version),
    fileExists: name => name === fileName || base.fileExists(name),
    readFile: name => name === fileName ? source : base.readFile(name),
    directoryExists: name => name === "/project" || name === "/project/out" || base.directoryExists(name),
    writeFile: () => assert.fail("writes must use the captured callback")};
  const program = ts.createProgram([fileName], options, host);
  const writes = [], diagnostics = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => {assert.equal(result, undefined); return result = emit(...args);};
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
  return {writes, reported_diagnostics: diagnostics, status_writes: status, exit_code: exit,
    emit_result: {emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps?.map(entry => ({
        input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap)})) ?? null}};
}

inputs.push(
  {id:"function-rest-valid", source:"export function g() { return ({a, ...r} = f(), r).f`ok`; }", target:4},
  {id:"function-nested-invalid", source:"export function g() { return a`\\unicode``ok`; }", target:4},
  {id:"es2015-mixed", source:"export {}; a`first`; a`\\unicode`; a`last`;", target:2},
  {id:"es2018-retained-invalid", source:"export {}; a`\\unicode`;", target:5},
  {id:"script-invalid", source:"a`\\unicode`;", target:4},
  {id:"raw-cooked-units", source:"export {}; tag`\\uD800${x}\\unicode`;", target:4},
  {id:"escaped-valid-rest-tag", source:"export {}; ({a, ...r} = f(), r).f`\\x61`;", target:4},
  {id:"nested-four-visits", source:"export {}; a`\\unicode``ok``end`;", target:4},
  {id:"function-expression-tag", source:"export {}; (() => ({a, ...r} = f(), r))`ok`;", target:4},
  {id:"namespace-mixed", source:"export namespace N { a`first`; a`\\unicode`; a`last`; }", target:1},
);
const cases = inputs.map(input => {
  const options = {...baseOptions, target:input.target};
  const direct = () => ts.transpileModule(input.source, {fileName:"main.ts", compilerOptions:{
    target:input.target, module:options.module, newLine:ts.NewLineKind.LineFeed}}).outputText;
  const directRuns = [direct(), direct()];
  assert.equal(directRuns[0], directRuns[1], input.id);
  const commandRuns = [complete(input.source, options), complete(input.source, options)];
  assert.deepEqual(commandRuns[0], commandRuns[1], input.id);
  if (input.parent_version === 1) {
    const old = v1.cases.find(c => c.id === input.id);
    assert.equal(directRuns[0], old.runs[0].output_text);
  } else if (input.parent_version === 2) {
    const old = v2.cases.find(c => c.id === input.id);
    assert.deepEqual(directRuns, old.direct_runs);
    assert.deepEqual(commandRuns, old.complete_command_runs);
  }
  return {...input, source_sha256:sha(Buffer.from(input.source)), options,
    direct_runs:directRuns, complete_command_runs:commandRuns};
});
const artifact = {version:3, status:"Upstream command observations only; native comparison pending",
  typescript:ts.version, compiler_sha256:compilerSha, parent_sha256:{v1:v1Sha,v2:v2Sha},
  observer_sha256:sha(fs.readFileSync(import.meta.filename)), repetitions:2,
  direct_executions:cases.length*2, complete_command_executions:cases.length*2, cases};
const output = path.join(root, "crates/compiler/tests/fixtures/utf16-tagged-template-controls.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact,null,2)+"\n", {flag:"wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)), artifact);
console.log(JSON.stringify({output, sha256:sha(fs.readFileSync(output)), cases:cases.length,
  direct_executions:cases.length*2, complete_command_executions:cases.length*2}));
