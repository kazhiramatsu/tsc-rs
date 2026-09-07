// Forced declaration-only APIs; adjacent ordinary emitOnly observations retain H2.8d ownership.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const target = path.join(root, "crates/compiler/tests/fixtures/forced-declarations.json");
const sha256 = x => crypto.createHash("sha256").update(x).digest("hex");
const defaults = {target:ts.ScriptTarget.ESNext,module:ts.ModuleKind.CommonJS,declaration:true,strict:true,skipDefaultLibCheck:true,newLine:ts.NewLineKind.CarriageReturnLineFeed,noErrorTruncation:true};
const inputs=[];
function add(case_id, files, options={}, extra={}) {
 inputs.push({case_id,files:Object.entries(files).map(([name,text])=>({path:'/project/'+name,text})),options:{...defaults,...options},...extra});
}
const pair={'good.ts':'export const good: number = 1;\n','bad.ts':'export const bad = class { private field: number = 1; };\n'};
for(const [name,options] of Object.entries({enabled:{},disabled:{declaration:false},'no-emit':{noEmit:true},'no-emit-on-error':{noEmitOnError:true},'no-check':{noCheck:true},isolated:{isolatedDeclarations:true},'isolated-disabled-declaration':{declaration:false,isolatedDeclarations:true},'list-emitted-files':{listEmittedFiles:true},'declaration-dir':{declarationDir:'/project/types'}})) {
 add('adjacent/force-options#'+name,pair,options);
}
add('adjacent/semantic-error',{'source.ts':"export const value: number = '';\n"},{noEmitOnError:true});
add('adjacent/syntax-error',{'source.ts':'export const value: number = ;\n'},{noEmitOnError:true});
add('adjacent/declaration-only-input',{'source.d.ts':'export declare const value: number;\n'});
add('adjacent/javascript',{'source.js':'export function fn(value) { return value; }\n'},{allowJs:true,checkJs:true});
add('adjacent/javascript-no-emit',{'source.js':'export function fn(value) { return value; }\n'},{allowJs:true,checkJs:true,noEmitForJsFiles:true});
add('adjacent/json',{'source.ts':'export const value: number = 1;\n','data.json':'{"value":1}'},{resolveJsonModule:true});
add('adjacent/imported-typescript',{'source.ts':"import { value } from './node_modules/pkg/source'; export const exported = value;\n",'node_modules/pkg/source.ts':pair['bad.ts'].replace('bad','value')},{},{roots:['/project/source.ts']});
add('adjacent/target-good',pair,{}, {target_source:'/project/good.ts'});
add('adjacent/target-bad',pair,{}, {target_source:'/project/bad.ts'});
add('adjacent/getter-first',pair,{}, {before:'declaration-diagnostics'});
add('adjacent/semantic-first',pair,{}, {before:'semantic-diagnostics'});
add('adjacent/linked-alias',{'dep.ts':'export class A {}\n','source.ts':"import { A } from './dep'; export const value: A = new A();\n"},{declaration:false});
add('adjacent/output-input-collision',{'source.ts':'export const value: number = 1;\n','source.d.ts':'export declare const original: string;\n'},{noEmitOnError:true});
for (const [name, options] of Object.entries({disabled:{declaration:false},'no-emit':{noEmit:true},'disabled-no-emit':{declaration:false,noEmit:true}})) {
 add('adjacent/output-input-collision#'+name,{'source.ts':'export const value: number = 1;\n','source.d.ts':'export declare const original: string;\n'},options);
}
for (const [name,text] of Object.entries({nested:'{"nested":{"value":1},"array":[1,2],"truth":true}',array:'[1,2,3]',string:'"hello"',null:'null'})) {
 add('adjacent/json-shape#'+name,{'data.json':text},{resolveJsonModule:true});
}
add('adjacent/imported-external-json',{'source.ts':"import data from './node_modules/pkg/data.json'; export const value = data.value;\n",'node_modules/pkg/data.json':'{"value":1}'},{resolveJsonModule:true,esModuleInterop:true},{roots:['/project/source.ts']});
add('adjacent/json-root-in-node-modules',{'node_modules/pkg/data.json':'{"value":1}'},{resolveJsonModule:true});
add('adjacent/json-no-emit-for-js',{'data.json':'{"value":1}'},{resolveJsonModule:true,noEmitForJsFiles:true});
add('adjacent/json-declaration-dir',{'data.json':'{"value":1}'},{resolveJsonModule:true,declarationDir:'/project/types'});
add('adjacent/empty-program',{});
const officialName='isolatedDeclarationErrorsExpressions.ts';
const raw=fs.readFileSync(path.join(root,'ts-tests/tests/cases/compiler',officialName));
assert.equal(sha256(raw),'c75c90821b1eddaab12483f9c4c2cc9f6a64c10b38fc6fab0dd22d278c1f63b6');
const officialFiles={};let filename=officialName,lines=[];
const flush=()=>{const text=lines.join('\n').replace(/^\n+/,'');if(text.trim())officialFiles[filename]=text;lines=[];};
for(const line of raw.toString().split(/\r?\n/)){const match=line.match(/^\/\/\s*@filename:\s*(.*)/i);if(match){flush();filename=match[1].trim();}else if(!/^\/\/\s*@\w+\s*:/.test(line))lines.push(line);}
flush();add('compiler/'+officialName,officialFiles,{target:ts.ScriptTarget.ES2015,strict:false,isolatedDeclarations:true},{source_sha256:sha256(raw)});
function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [fileName, text, bom, onError, sourceFiles, data] = args;
  const callback = Buffer.from(text, "utf8");
  const materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), callback]) : callback;
  return { index, path: fileName, kind: ts.isDeclarationFileName(fileName) ? "declaration" : "javascript",
    callback_utf8_base64: callback.toString("base64"), callback_utf8_bytes: callback.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"),
    materialized_utf8_bytes: materialized.length, on_error_callback_present: onError !== undefined,
    source_files: sourceFiles?.map(f => f.fileName) ?? null, data_present: data !== undefined,
    data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null };
}

function observe(input, force) {
 const files=new Map(input.files.map(f=>[f.path,f.text]));
 const host=ts.createCompilerHost(input.options,true);
 const read=host.readFile.bind(host),exists=host.fileExists.bind(host),directoryExists=host.directoryExists.bind(host);
 host.readFile=name=>files.get(name)??read(name);host.fileExists=name=>files.has(name)||exists(name);host.getCurrentDirectory=()=>'/project';
 const directories=new Set();for(const file of files.keys()){for(let dir=path.dirname(file);;dir=path.dirname(dir)){directories.add(dir);if(dir===path.dirname(dir))break;}}
 host.directoryExists=name=>directories.has(name)||directoryExists(name);
 const program=ts.createProgram({rootNames:input.roots??[...files.keys()],options:input.options,host});
 const resolverRequests=[];
 const checker=program.getTypeChecker(),getEmitResolver=checker.getEmitResolver.bind(checker);
 checker.getEmitResolver=(source,token,skip)=>{
  resolverRequests.push({source_file:source?.fileName??null,skip_diagnostics:skip??null});
  return getEmitResolver(source,token,skip);
 };
 const before=input.before==='declaration-diagnostics'?program.getDeclarationDiagnostics():input.before==='semantic-diagnostics'?program.getSemanticDiagnostics():null;
 const source=input.target_source?program.getSourceFile(input.target_source):undefined;
 if(input.target_source)assert.ok(source);
 const before_resolver_requests=[...resolverRequests];
 const calls=[],resolver_requests_by_emit=[];
 for(let repetition=0;repetition<2;repetition++) {
  const writes=[];
  const requestStart=resolverRequests.length;
  const result=program.emit(source,(...args)=>writes.push(write(args,writes.length)),undefined,true,undefined,force);
  resolver_requests_by_emit.push(resolverRequests.slice(requestStart));
  assert.ok(result.sourceMaps===undefined||result.sourceMaps.length===0);
  calls.push({writes,emit_result:{emit_skipped:result.emitSkipped,diagnostics:result.diagnostics.map(diagnostic),emitted_files:result.emittedFiles??null,source_maps:result.sourceMaps??null}});
 }
 assert.deepEqual(calls[1],calls[0],input.case_id);
 return {typescript_observation:{before_diagnostics:before?.map(diagnostic)??null,calls},owner_observation:{before_resolver_requests,resolver_requests_by_emit}};
}
for (const noEmitForJsFiles of [false, true]) {
 add('adjacent/javascript-common-directory#'+noEmitForJsFiles,{'a/source.ts':'export const value: number = 1;\n','b/source.js':'export const other = 2;\n'},{allowJs:true,noEmitForJsFiles,declarationDir:'/project/types'});
}
const cases=inputs.flatMap(input=>[false,true].map(force=>{
 const first=observe(input,force);assert.deepEqual(observe(input,force),first,input.case_id);
 return {...input,case_id:input.case_id+'#force-'+force,force,...first};
}));
const artifact={version:1,typescript:ts.version,source_commit:'050880ce59e30b356b686bd3144efe24f875ebc8',compiler_sha256:sha256(fs.readFileSync(path.join(root,'vendor/typescript-6.0.3/lib/typescript.js'))),repetitions:2,cases:cases.filter(input=>input.force),adjacent_ordinary_api_owner:"H2.8d",adjacent_ordinary_api_observations:cases.filter(input=>!input.force)};
const rendered=JSON.stringify(artifact,null,2)+'\n';
if(process.argv[2]==='--write')fs.writeFileSync(target,rendered);else {assert.ok(process.argv[2]===undefined||process.argv[2]==='--check');assert.equal(fs.readFileSync(target,'utf8'),rendered);}
console.log(`forced declarations: ${artifact.cases.length} forced sequences plus ${artifact.adjacent_ordinary_api_observations.length} ordinary H2.8d reference sequences, two identical observations each`);
