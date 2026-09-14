// Complete-command controls for the implementation-review fix round (review
// response §2–§4: M-2/B-2, A-2, A-3, A-4, C-1). Each row is one whole
// createProgram + emitFilesAndReportErrorsAndGetExitStatus observation from
// the pinned compiler, run twice; the native outcome is compared separately.
// The identity-recovery observer's harness is repeated here on purpose so
// the frozen utf16-identity-recovery-controls.json is never rewritten.
// node scripts/observe-utf16-review-fix-controls.mjs --write|--check
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
      materialized_utf8_base64: Buffer.from((bom ? "﻿" : "") + text).toString("base64"),
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
const baseOptions = {module:99, newLine:0, strict:true, declaration:true,
  declarationMap:true, sourceMap:true, skipDefaultLibCheck:true,
  noErrorTruncation:true, outDir:"/project/out"};
const inputs = [];
function add(id, finding, source, options = {}) {
  const files = typeof source === "string" ? [{path:"/project/main.ts",text:source}] : source;
  inputs.push({id, finding, files, roots:files.map(file => file.path), options:{...baseOptions,target:2,...options}, native:"exact"});
}
// M-2 / B-2: emitFilesAndReportErrors adds getDeclarationDiagnostics only for
// noEmit + getEmitDeclarations(options) and only while nothing beyond the
// config-file parsing diagnostics was reported (_tsc.js:129433-129440).
add("m2-noemit-declaration-ts4094", "B-2", "export const C = class { private x = 1; };", {noEmit:true});
add("m2-noemit-declaration-clean", "B-2", "export const C = class { x = 1; };", {noEmit:true});
add("m2-noemit-declaration-semantic-first", "B-2", "export const C = class { private x: string = 1; };", {noEmit:true});
add("m2-noemit-declaration-syntactic-first", "B-2", "export const C = class { private x = 1; };\nconst y = ;", {noEmit:true});
add("m2-noemit-without-declaration", "B-2", "export const C = class { private x = 1; };", {noEmit:true, declaration:false, declarationMap:false});
add("m2-noemit-declaration-two-files", "B-2", [
  {path:"/project/a.ts", text:"export const A = class { private x = 1; };"},
  {path:"/project/main.ts", text:"import {A} from \"./a\"; export const B = class { protected y = new A(); };"},
], {noEmit:true});
// A-2: "Did you mean" suggestions render symbolName(suggestion) (_tsc.js:75518-75521,
// 11452-11458), never the written (quoted / escaped) face.
add("a2-element-access-quoted-candidate", "A-2", "export const o = { [\"abcd-\"]: 1 }; export const e = o[\"abcde\"];");
add("a2-property-access-quoted-candidate", "A-2", "export const o = { [\"abcd-\"]: 1 }; export const e = o.abcde;");
add("a2-excess-property-quoted-candidate", "A-2", "declare let t: { [\"abcd-\"]: number }; t = { abcde: 1 };");
add("a2-private-name-candidate", "A-2", "export class C { #hidden = 1; m() { return this.hidden; } }");
add("a2-leading-underscore-candidates", "A-2", "export const p = { [\"__protox\"]: 1 }; export const e3 = p[\"__protoy\"]; export const e4 = p.__protoy; declare let u: { [\"__protox\"]: number }; u = { __protoy: 1 };");
add("a2-lone-surrogate-candidate", "A-2", String.raw`const q = { ["abcdef\uD800"]: 1 }; export const y = q["abcdeg\uD800"];`);
// A-3: declarationless (mapped) property symbols display through
// getNameOfSymbolFromNameType before symbolName (_tsc.js:55541-55557, 55586-55588).
add("a3-record-quoted-name", "A-3", "export const r: Record<\"a-b\", number> = {};");
add("a3-record-lone-surrogate-name", "A-3", String.raw`export const r: Record<"\uD800", number> = {};`);
add("a3-record-identifier-name", "A-3", "export const r: Record<\"ab\", number> = {};");
add("a3-record-negative-numeric-name", "A-3", "export const r: Record<\"-1\", number> = {};");
add("a3-record-numeric-name", "A-3", "export const r: Record<\"1\", number> = {};");
// A-4: the writeFile callback carries the raw JavaScript string; a declaration
// literal type built by typeToTypeNode keeps an unpaired unit there
// (NoAsciiEscaping) while the sink projects it to U+FFFD.
add("a4-declaration-callback-lone-unit", "A-4", String.raw`export const k = "\uD800" as const; export const k2 = k;`);
add("a4-declaration-callback-lone-unit-es5", "A-4", String.raw`export const k = "\uD800" as const; export const k2 = k;`, {target:1});
// C-1: createCallExpression parenthesizes the callee (parenthesizeLeftSideOfAccess,
// _tsc.js:22579-22585, 20466-20471); an optional-chain tag lowered to a
// conditional reaches the ES2018 / ES2015 tagged-template call producers.
add("c1-es2017-optional-property-invalid", "C-1", "export {}; declare const a: any; a?.b`\\unicode`;", {target:4});
add("c1-es5-optional-property-valid", "C-1", "export {}; declare const a: any; a?.b`ok`;", {target:1});
add("c1-es2015-optional-element-invalid", "C-1", "export {}; declare const a: any; a?.[0]`\\unicode`;");
add("c1-es2015-optional-call-invalid", "C-1", "export {}; declare const a: any; a?.()`\\unicode`;");
add("c1-es2017-optional-chain-long-invalid", "C-1", "export {}; declare const a: any; a?.b.c`\\unicode`;", {target:4});
add("c1-es2015-parenthesized-tag-valid", "C-1", "export {}; declare const a: any, b: any; (a || b)`ok`; (a, b)`ok`;", {target:1});
const cases=[];
for (const input of inputs) {
  const runs=[complete(input.files,input.roots,input.options),complete(input.files,input.roots,input.options)];
  assert.deepEqual(runs[0],runs[1],input.id);
  cases.push({...input,files:input.files.map(file=>({...file,sha256:sha(Buffer.from(file.text))})),complete_command_runs:runs});
}
const artifact={version:1,scope:"Implementation-review fix-round controls (B-2 noEmit declaration diagnostics, A-2 suggestion names, A-3 declarationless names, A-4 callback units, C-1 callee parenthesization); upstream full command observations, native outcome separate",typescript:ts.version,compiler_sha256:compilerSha,observer_sha256:sha(fs.readFileSync(import.meta.filename)),repetitions:2,complete_command_executions:cases.length*2,cases};
const output=path.join(root,"crates/compiler/tests/fixtures/utf16-review-fix-controls.json");
assert.ok(["--write","--check"].includes(process.argv[2]));
if(process.argv[2]==="--write") fs.writeFileSync(output,JSON.stringify(artifact,null,2)+"\n",{flag:"wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)),artifact);
console.log(JSON.stringify({output,sha256:sha(fs.readFileSync(output)),cases:cases.length,complete_command_executions:cases.length*2}));
