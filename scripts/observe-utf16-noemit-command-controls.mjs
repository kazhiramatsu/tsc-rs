// Whole-program noEmit result controls: collection presence, reporting, and noEmitOnError precedence.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const units = text => Array.from({length: text.length}, (_, index) => text.charCodeAt(index));
const value = text => ({utf16: units(text), utf8_base64: Buffer.from(text).toString("base64")});
const diagnostic = d => ({code: d.code, category: d.category, file: d.file?.fileName ?? null,
  start: d.start ?? null, length: d.length ?? null,
  message: value(ts.flattenDiagnosticMessageText(d.messageText, "\n")),
  related_information: d.relatedInformation?.map(diagnostic) ?? null});
function complete(files, roots, options) {
  const input = new Map(files.map(file => [file.path, file.text]));
  const base = ts.createCompilerHost(options, true);
  const host = {...base, getCurrentDirectory: () => "/project",
    getSourceFile: (name, version) => input.has(name)
      ? ts.createSourceFile(name, input.get(name), version, true) : base.getSourceFile(name, version),
    fileExists: name => input.has(name) || base.fileExists(name),
    readFile: name => input.has(name) ? input.get(name) : base.readFile(name),
    directoryExists: name => name === "/project" || name === "/project/out" || base.directoryExists(name),
    writeFile: () => assert.fail("writes must use the captured callback")};
  const program = ts.createProgram(roots, options, host);
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

const cases = [];
for (const invalid of [false,true]) for (const variant of ["none","js-map","inline-map","declaration-map","inactive-declaration-map","listing","noEmitOnError"]) {
  const text = invalid ? 'export const k="\\8\\uD800";' : 'export const k="\\uD800" as const;';
  const options = {target:2,module:99,newLine:0,strict:true,skipDefaultLibCheck:true,noErrorTruncation:true,
    noEmit:true,declaration:false,declarationMap:false,sourceMap:false,outDir:"/project/out"};
  if(variant==="js-map") options.sourceMap=true;
  if(variant==="inline-map") options.inlineSourceMap=true;
  if(variant==="declaration-map") {options.declaration=true;options.declarationMap=true;}
  if(variant==="inactive-declaration-map") options.declarationMap=true;
  if(variant==="listing") options.listEmittedFiles=true;
  if(variant==="noEmitOnError") options.noEmitOnError=true;
  const files=[{path:"/project/main.ts",text,sha256:sha(Buffer.from(text))}];
  const roots=["/project/main.ts"];
  const runs=[complete(files,roots,options),complete(files,roots,options)];
  assert.deepEqual(runs[0],runs[1]);
  cases.push({id:`noEmit/${variant}/${invalid?"invalid":"valid"}`,files,roots,options,native:"exact",complete_command_runs:runs});
}
const artifact={version:1,scope:"Whole-program noEmit empty-build-info command and map/list collection presence; incremental/composite output remains outside scope",typescript:ts.version,compiler_sha256:compilerSha,observer_sha256:sha(fs.readFileSync(import.meta.filename)),repetitions:2,complete_command_executions:cases.length*2,cases};
const output=path.join(root,"crates/compiler/tests/fixtures/utf16-noemit-command-controls.json");
assert.ok(["--write","--check"].includes(process.argv[2]));
if(process.argv[2]==="--write") fs.writeFileSync(output,JSON.stringify(artifact,null,2)+"\n",{flag:"wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)),artifact);
console.log(JSON.stringify({output,sha256:sha(fs.readFileSync(output)),cases:cases.length,complete_command_executions:cases.length*2}));
