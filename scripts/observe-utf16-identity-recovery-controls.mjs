// Additional A identity and B recovery commands from the accepted review, without changing the original 23.
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
const baseOptions = {module:99, newLine:0, strict:true, declaration:true,
  declarationMap:true, sourceMap:true, skipDefaultLibCheck:true,
  noErrorTruncation:true, outDir:"/project/out"};
const inputs = [];
function add(id, source, options = {}, native = "exact") {
  const files = typeof source === "string" ? [{path:"/project/main.ts",text:source}] : source;
  inputs.push({id, files, roots:files.map(file => file.path), options:{...baseOptions,target:2,...options},native});
}
const a = [
  ["a1-distinct-and-pair", String.raw`export const o = {"\uD800":1,"\uD801":2,"\uDC00":3,"\uFFFD":4,"\\uD800":5,"\uD83D\uDE00":6,"\u{1F600}":7};`],
  ["a2-object-equivalent", 'export const o = {"\\uD800":1,"\\u{D800}":2,[`\\uD800`]:3};'],
  ["a2-class-equivalent", 'export class C { "\\uD800" = 1; "\\u{D800}" = 2; [`\\uD800`] = 3; }'],
  ["a2-interface-equivalent", String.raw`export interface I { "\uD800": number; "\u{D800}": string; }`],
  ["a3-lookup-mapped", 'export const o = {"\\uD800":1,"\\uDC00":2}; export const a=o["\\uD800"]; export const b=o[`\\uDC00`]; export const {"\\uD800": c}=o; export type K = keyof typeof o; export type P=Pick<typeof o,"\\uD800">; export type O=Omit<typeof o,"\\uD800">; export type R=Record<"\\uD800",number>; export type M={[P in keyof typeof o]:P}; export const m:M={"\\uD800":"\\uD800","\\uDC00":"\\uDC00"};'],
  ["a3-narrowing-context", String.raw`export function f(x: {kind:"\uD800", "\uD800":1}|{kind:"\uDC00", "\uDC00":2}) { if ("\uD800" in x) return x["\uD800"]; if (x.kind === "\uDC00") return x["\uDC00"]; } export const o: {"\uD800":number} = {"\uD800":1};`],
  ["a3-reverse-mapped", String.raw`declare function infer<T>(o:{[P in keyof T]:{value:T[P]}}):T; export const result = infer({"\uD800":{value:1},"\uDC00":{value:"x"},"__call":{value:true}});`],
  ["a4-late-bound", String.raw`export const k="\uD800" as const; export class C { [k]=1; } export const c=new C(); export const value=c[k];`],
  ["a5-enum-pair", String.raw`export enum E { "\uD800"=1, B="\uD800", C="\uD800"+"\uDC00" } const enum F { A="\uD800", B="\uD800"+"\uDC00" } export const a=F.A; export const b=F.B;`],
  ["a6-template-types", 'export type T = `${"\\uD800"}${"\\uDC00"}`; export type U=Uppercase<"\\uD800">; export const t:T="\\uD800\\uDC00"; export const u:U="\\uD800"; export const s:`a${string}`="a\\uD800";'],
  ["a8-name-kinds", String.raw`export const o={__proto__:1,"__call":1,"___x":1,"1":1,1:2,"01":1,"-1":1,[-1]:1}; export const a=o["__call"]; export const b=o["___x"]; export class C { static readonly "\uD800":unique symbol; }`],
  ["a8-leading-expando", String.raw`export function f(){} f["__call"]=1; f["___x"]=2; f["\uD800"]=3; export const a=f["__call"]; export const b=f["___x"]; export const c=f["\uD800"];`],
  ["a9-diagnostic-values", String.raw`const o={"\uD800":1}; export const x=o["\uD801"]; export const y:"\uD800"="\uDC00"; export const z:{"\uD800":number}={"\uDC00":1};`],
  ["a9-spelling-suggestion", String.raw`const o={"abc\uD800":1}; export const x=o["abd\uD800"];`],
  ["a10-literal-type", String.raw`export const k="\uD800" as const;`],
];
for (const target of [1,2]) for (const [id,text] of a) add(`${id}/target-${target}`,text,{target});
for (const module of [1,99]) {
  add(`a7-cross-file/module-${module}`,[
    {path:"/project/a.ts",text:String.raw`export const o={"\uD800":1}; const x="\uDC00"; export {x as "\uD800"};`},
    {path:"/project/main.ts",text:String.raw`import {o,"\uD800" as y} from "./a"; export const a=o["\uD800"]; export const b=y;`},
  ],{module});
  add(`a7-ambient-module/module-${module}`,[
    {path:"/project/a.d.ts",text:String.raw`declare module "\uD800" {export const a:number;} declare module "\uD801" {export const a:string;}`},
    {path:"/project/main.ts",text:String.raw`import {a} from "\uD800"; import {a as b} from "\uD801"; export {a,b};`},
  ],{module});
}
add("a8-private-static-instance", "export class C { #x=1; static #x=2; }", {target:9});
const upstreamLimitations = [];
const privateSource = "export class C { #x=1; static #x=2; }";
const privateOptions = {...baseOptions,target:1};
const privateRuns = [0,1].map(() => {
  try { return {command:complete([{path:"/project/main.ts",text:privateSource}],["/project/main.ts"],privateOptions)}; }
  catch(error) { return {error_message:error.message}; }
});
assert.deepEqual(privateRuns[0], privateRuns[1]);
assert.match(privateRuns[0].error_message, /Node PropertyDeclaration was unexpected/);
upstreamLimitations.push({id:"private-static-instance-es5-upstream-internal-error",source:privateSource,options:privateOptions,runs:privateRuns});
const oldBytes=fs.readFileSync(path.join(root,"crates/compiler/tests/fixtures/utf16-literals-adjacent-probes-inputs.json"));
assert.equal(sha(oldBytes),"44672cd7192c2897f25cf93f0c04892a2c3bcabd00589de83ac32bfff2ee6b69");
const old=JSON.parse(oldBytes);
const bRows=old.cases.filter(row=> !/declaration-|collision|tagged-invalid/.test(row.case_id));
assert.equal(bRows.length,14);
for (const row of bRows) add(`b1-noEmitOnError/${row.case_id}`,row.files,{...row.options,noEmitOnError:true});
add("b2-noEmit-declaration",String.raw`export const x="\8\uD800";`,{noEmit:true});
for (const [id,text] of [
  ["missing-initializer","export const x = ;"],["missing-class","class {"],
  ["mixed-literal-structure",String.raw`export const x="\uD800" +`],
  ["numeric-separator","export const x=1__0;"],["hex-digits","export const x=0x;"],
  ["regexp-eof","export const x=/abc"],["comment-eof","export const x=1; /* ..."],
  ["conflict-marker","<<<<<<< HEAD\nexport const x=1;"],
  ["keyword-escape",String.raw`\u0063lass C {}`],
]) add(`b3-b4-rejected/${id}`,text,{},"typed-recovery-refusal");
add("b5-tagged-no-diagnostic",'declare function tag(t:TemplateStringsArray):unknown; export const x=tag`\\unicode`;',{target:4});
add("b6-eof-backslash",'export const x="\\');
add("b6-value-eof-backslash",'export const x="\\u{D800}\\');
// Complete the C target and substitution matrix beyond the frozen v3 receipt.
for (const target of [2,3,4]) add(`c1-multiple-spans/target-${target}`,
  'export {}; declare const x:unknown; declare function tag(t:TemplateStringsArray,...v:unknown[]):unknown; tag`a${x}\\unicode${x}z`;',{target});
const cases=[];
for (const input of inputs) {
  const runs=[complete(input.files,input.roots,input.options),complete(input.files,input.roots,input.options)];
  assert.deepEqual(runs[0],runs[1],input.id);
  cases.push({...input,files:input.files.map(file=>({...file,sha256:sha(Buffer.from(file.text))})),complete_command_runs:runs});
}
const artifact={version:1,scope:"Accepted review A1-A10/B1-B6 plus C1 target and substitution controls; upstream full command observations, native outcome separate",typescript:ts.version,compiler_sha256:compilerSha,observer_sha256:sha(fs.readFileSync(import.meta.filename)),repetitions:2,complete_command_executions:cases.length*2,upstream_limitations:upstreamLimitations,cases};
const output=path.join(root,"crates/compiler/tests/fixtures/utf16-identity-recovery-controls.json");
assert.ok(["--write","--check"].includes(process.argv[2]));
if(process.argv[2]==="--write") fs.writeFileSync(output,JSON.stringify(artifact,null,2)+"\n",{flag:"wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)),artifact);
console.log(JSON.stringify({output,sha256:sha(fs.readFileSync(output)),cases:cases.length,complete_command_executions:cases.length*2}));
