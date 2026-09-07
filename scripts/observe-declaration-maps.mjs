// H2.7e non-bundle declaration maps: complete, repeated TS 6.0.3 observations.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const runtime = process.argv.includes("--runtime");
const disabled = process.argv.includes("--disabled-declaration");
assert.ok(!(runtime && disabled));
const output = path.join(root, "crates/compiler/tests/fixtures", disabled ? "declaration-maps-disabled-declaration.json" : runtime ? "declaration-maps-runtime.json" : "declaration-maps.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.equal(process.versions.node, fs.readFileSync(path.join(root, ".node-version"), "utf8").trim());
const inputs = [];
function add(id, files, options = {}, extra = {}) {
  inputs.push({ case_id: id, current_directory: "/project", files: Object.entries(files).map(([name,text]) => ({ path: "/project/" + name, text })),
    options: { target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.CommonJS, declaration: true, declarationMap: true,
      emitDeclarationOnly: true, listEmittedFiles: true, newLine: ts.NewLineKind.CarriageReturnLineFeed,
      strict: false, skipDefaultLibCheck: true, noErrorTruncation: true, ...options }, ...extra });
}
const syntax = {
  variables: "export const constant = 1;\nexport let value: number = 2;\nexport const text = 'hello';\n",
  functions: "export function f<T>(value: T, count = 1): T { return value; }\nexport function g(a: string, ...rest: number[]): void {}\n",
  types: "export interface I<T> { x: T; readonly y?: string; f(arg: T): number; }\nexport type Pair<T> = [T, number];\n",
  classes: "export class C { readonly value: number = 1; constructor(public name: string) {} method(x: number): string { return this.name; } }\n",
  enums: "export enum E { A, B = 4, C = 'c' }\n",
  namespaces: "export namespace N { export const value: number = 1; export interface I { x: number } }\n",
  comments: "/** 文😀 */\nexport const 名: string = '😀'; // trailing\n/** function */\nexport function f(): void {}\n",
  stripped: "/** @internal */\nexport const hidden: number = 1;\nexport const visible: number = 2;\n",
  empty: "",
  erased: "const hidden = 1;\nexport {};\n",
};
for (const [name, text] of Object.entries(syntax)) add("syntax/" + name, { "a.ts": text }, name === "stripped" ? {stripInternal: true} : {});
const nested = { "src/a.ts": syntax.variables, "src/nested/b.ts": syntax.types };
for (const [name, options] of [
  ["default", {}], ["declaration-dir", {declarationDir: "/project/types"}],
  ["relative-declaration-dir", {declarationDir: "types"}], ["source-root", {sourceRoot: "https://sources.test/root"}],
  ["relative-map-root", {mapRoot: "maps"}], ["absolute-map-root", {mapRoot: "/maps"}],
  ["url-map-root", {mapRoot: "https://maps.test/maps"}],
  ["both-roots", {mapRoot: "maps", sourceRoot: "../sources"}],
  ["inline-options", {inlineSourceMap: true, inlineSources: true}],
  ["bom-lf", {emitBOM: true, newLine: ts.NewLineKind.LineFeed}],
  ["remove-comments", {removeComments: true}],
]) add("options/" + name, nested, options);
add("paths/encoded-name", {"has space/名.ts": syntax.comments});
add("syntax/javascript", {"a.js": "/** @param {number} x */\nexport function f(x) { return x; }\n"}, {allowJs: true});
const sourcePath = "ts-tests/tests/cases/compiler/declarationMaps.ts";
const raw = fs.readFileSync(path.join(root, sourcePath));
const sourceText = raw.toString().split(/\r?\n/).filter(line => !/^\/\/\s*@\w+\s*:/.test(line)).join("\r\n");
add("compiler/declarationMaps.ts#declaration-only-control", {"declarationMaps.ts": sourceText}, {}, {source: {path: sourcePath, sha256: sha256(raw)}});
function diagnostic(d) {
  return {code:d.code, category:ts.DiagnosticCategory[d.category], file:d.file?.fileName ?? null, start:d.start ?? null,
    length:d.length ?? null, message:ts.flattenDiagnosticMessageText(d.messageText,"\n"), related_information:d.relatedInformation?.map(diagnostic) ?? null};
}
function write(args, index) {
  const [name,text,bom,onError,sourceFiles,data] = args, callback = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239,187,191]),callback]) : callback;
  return {index,path:name,kind:name.endsWith(".d.ts.map") ? "declaration-map" : name.endsWith(".map") ? "javascript-map" : name.endsWith(".d.ts") ? "declaration" : "javascript", callback_utf8_base64:callback.toString("base64"), callback_utf8_bytes:callback.length,
    write_byte_order_mark:bom,materialized_utf8_base64:materialized.toString("base64"),materialized_utf8_bytes:materialized.length,
    on_error_callback_present:onError!==undefined,source_files:sourceFiles?.map(f=>f.fileName) ?? null,data_present:data!==undefined,
    data_source_map_url_pos:data?.sourceMapUrlPos ?? null,data_diagnostics:data?.diagnostics?.map(diagnostic) ?? null};
}
function observe(input) {
  const files = new Map(input.files.map(f=>[f.path,f.text]));
  const base = ts.createCompilerHost(input.options,true);
  const libraryRoot = ts.normalizePath(path.join(root,"vendor/typescript-6.0.3/lib"));
  const isLibrary = name => ts.normalizePath(name).startsWith(libraryRoot + "/");
  const overlay = createHermeticDirectoryOverlay(files.keys(), {currentDirectory:input.current_directory,useCaseSensitiveFileNames:true,
    fallbackHost:{directoryExists:name=>isLibrary(name+"/")&&base.directoryExists(name),getDirectories:name=>isLibrary(name+"/")?base.getDirectories(name):[]}});
  const host = {...base,...overlay,getCurrentDirectory:()=>input.current_directory,useCaseSensitiveFileNames:()=>true,getCanonicalFileName:n=>n,
    fileExists:name=>files.has(ts.normalizePath(name))||(isLibrary(name)&&base.fileExists(name)),
    readFile:name=>files.get(ts.normalizePath(name))??(isLibrary(name)?base.readFile(name):undefined),
    getSourceFile(name,version) { const text=files.get(ts.normalizePath(name)); return text===undefined?(isLibrary(name)?base.getSourceFile(name,version):undefined):ts.createSourceFile(ts.normalizePath(name),text,version,true,ts.getScriptKindFromFileName(name)); }};
  const program=ts.createProgram(input.files.map(f=>f.path),input.options,host), writes=[],reported=[],status=[];
  let result; const emit=program.emit.bind(program); program.emit=(...args)=>{assert.equal(result,undefined);return result=emit(...args);};
  const exit=ts.emitFilesAndReportErrorsAndGetExitStatus(program,d=>reported.push(diagnostic(d)),text=>status.push(text),undefined,(...args)=>writes.push(write(args,writes.length)));
  return {common_source_directory:program.getCommonSourceDirectory(),writes,reported_diagnostics:reported,emit_refused:result.emitSkipped,
    emit_result:{emit_skipped:result.emitSkipped,diagnostics:result.diagnostics.map(diagnostic),emitted_files:result.emittedFiles??null,source_maps:result.sourceMaps??null},status_writes:status,exit_code:exit};
}
const runtimeInputs = [];
for (const [name, options] of [
  ["javascript-and-declaration", {}],
  ["both-external-maps", {sourceMap: true}],
  ["inline-javascript-external-declaration", {inlineSourceMap: true, inlineSources: true}],
]) {
  add("runtime/" + name, {"a.ts": syntax.variables, "b.ts": syntax.functions}, {emitDeclarationOnly: false, ...options});
  runtimeInputs.push(inputs.pop());
}
const originalInputs = disabled ? JSON.parse(fs.readFileSync(path.join(root, "ratchets/h2-7de-candidate-inputs.v1.json"), "utf8")) : undefined;
const originalWithoutDeclaration = originalInputs?.cases.find(c => c.case_id === "typescript-6.0.3/compiler/declarationMapsWithoutDeclaration.ts#default");
const disabledInputs = disabled ? [undefined, false].map(declaration => ({
  case_id: declaration === undefined ? originalWithoutDeclaration.case_id : "options/declarationMap-with-explicit-false",
  current_directory: originalWithoutDeclaration.input.current_directory,
  files: originalWithoutDeclaration.input.files,
  options: {...originalWithoutDeclaration.effective_options, ...(declaration === undefined ? {} : {declaration})},
  source: originalWithoutDeclaration.source,
})) : [];
const cases=(disabled ? disabledInputs : runtime ? runtimeInputs : inputs).map(input=>{const first=observe(input);assert.deepEqual(observe(input),first,input.case_id);return {...input,typescript_observation:first};});
const artifact={version:1,typescript:ts.version,source_commit:"050880ce59e30b356b686bd3144efe24f875ebc8",compiler_sha256:sha256(fs.readFileSync(path.join(root,"vendor/typescript-6.0.3/lib/typescript.js"))),repetitions:2,cases};
const rendered=JSON.stringify(artifact,null,2)+"\n";
const mode = process.argv.slice(2).filter(arg => arg !== "--runtime" && arg !== "--disabled-declaration");
assert.ok(mode.length <= 1 && (mode[0] === undefined || mode[0] === "--write" || mode[0] === "--check"));
if(mode[0] === "--write")fs.writeFileSync(output,rendered);else assert.equal(fs.readFileSync(output,"utf8"),rendered);
console.log(`H2.7e non-bundle declaration map observations: ${cases.length}, twice each`);
