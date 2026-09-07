// Focused public getter and owner observations; the frozen declaration band is unchanged.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const root=path.resolve(import.meta.dirname,'..');
const output=path.join(root,'crates/compiler/tests/fixtures/declaration-getters.json');
const sha256=x=>crypto.createHash('sha256').update(x).digest('hex');
const defaults={target:ts.ScriptTarget.ESNext,module:ts.ModuleKind.CommonJS,declaration:true,strict:true,skipDefaultLibCheck:true,newLine:ts.NewLineKind.CarriageReturnLineFeed,noErrorTruncation:true};
const inputs=[];
function add(case_id,files,options={}) {inputs.push({case_id,files:Object.entries(files).map(([name,text])=>({path:'/project/'+name,text})),options:{...defaults,...options}});}
const pair={'good.ts':'export const good: number = 1;\n','bad.ts':'export const bad = class { private field: number = 1; };\n'};
for(const [name,options] of Object.entries({enabled:{},disabled:{declaration:false},'no-emit':{noEmit:true},'no-emit-on-error':{noEmitOnError:true},'no-check':{noCheck:true},isolated:{isolatedDeclarations:true},'isolated-disabled-declaration':{declaration:false,isolatedDeclarations:true}})) add('adjacent/getter-options#'+name,pair,options);
add('adjacent/reversed-roots',Object.fromEntries(Object.entries(pair).reverse()));
add('adjacent/target-good-first',pair); inputs.at(-1).call_order=['/project/good.ts','/project/bad.ts',null,'/project/good.ts',null];
add('adjacent/target-bad-first',pair); inputs.at(-1).call_order=['/project/bad.ts','/project/good.ts',null,'/project/bad.ts',null];
add('adjacent/multiple-diagnostics-reversed-roots',{'z.ts':'export const z = class { private field: number = 1; };\n','a.ts':'export const a = class { protected field: number = 1; };\nexport const b = class { private field: string = \'\'; };\n'});

add('adjacent/semantic-error',{'source.ts':"export const value: number = '';\n"});
add('adjacent/syntax-error',{'source.ts':'export const value: number = ;\n'});
add('adjacent/empty-program',{});
add('adjacent/declaration-only-input',{'source.d.ts':'export declare const value: number;\n'});
add('adjacent/javascript',{'source.js':'export function fn(value) { return value; }\n'},{allowJs:true,checkJs:true});
add('adjacent/json',{'source.ts':'export const value: number = 1;\n','data.json':'{"value":1}'},{resolveJsonModule:true});
add('adjacent/imported-typescript',{'source.ts':"import { value } from './node_modules/pkg/source'; export const exported = value;\n",'node_modules/pkg/source.ts':'export const value = class { private field: number = 1; };\n'});
inputs.at(-1).roots=['/project/source.ts'];
const officialName='declarationEmitIsolatedDeclarationErrorNotEmittedForNonEmittedFile.ts';
const raw=fs.readFileSync(path.join(root,'ts-tests/tests/cases/compiler',officialName));
assert.equal(sha256(raw),'77953360c15e7fedd9f7944adac4158b3332650ce8e2601bd340cdda3a98bf96');
const officialFiles={}; let filename=officialName,lines=[];
const flush=()=>{const text=lines.join('\n').replace(/^\n+/,'');if(text.trim())officialFiles[filename]=text;lines=[];};
for(const line of raw.toString().split(/\r?\n/)){const match=line.match(/^\/\/\s*@filename:\s*(.*)/i);if(match){flush();filename=match[1].trim();}else if(!/^\/\/\s*@\w+\s*:/.test(line))lines.push(line);}
flush();add('compiler/'+officialName,officialFiles,{target:ts.ScriptTarget.ES2015,strict:false,isolatedDeclarations:true});inputs.at(-1).source_sha256=sha256(raw);inputs.at(-1).roots=['/project/index.ts'];
const diagnostic=d=>({code:d.code,category:ts.DiagnosticCategory[d.category],file:d.file?.fileName??null,start:d.start??null,length:d.length??null,message:ts.flattenDiagnosticMessageText(d.messageText,'\n'),related_information:d.relatedInformation?.map(diagnostic)??null});
function observe(input){
 const files=new Map(input.files.map(f=>[f.path,f.text]));
 const host=ts.createCompilerHost(input.options,true);
 const read=host.readFile.bind(host),exists=host.fileExists.bind(host),directoryExists=host.directoryExists.bind(host);
 host.readFile=name=>files.get(name)??read(name);host.fileExists=name=>files.has(name)||exists(name);host.getCurrentDirectory=()=>'/project';
 const directories=new Set();for(const file of files.keys()){for(let dir=path.dirname(file);;dir=path.dirname(dir)){directories.add(dir);if(dir===path.dirname(dir))break;}}
 host.directoryExists=name=>directories.has(name)||directoryExists(name);
 const writes=[];host.writeFile=(...args)=>writes.push(args);
 const program=ts.createProgram({rootNames:input.roots??[...files.keys()],options:input.options,host});
 const resolverRequests=[];
 const checker=program.getTypeChecker();const getEmitResolver=checker.getEmitResolver.bind(checker);
 checker.getEmitResolver=(source,token,skip)=>{
  resolverRequests.push({source_file:source?.fileName??null,skip_diagnostics:skip??null});
  return getEmitResolver(source,token,skip);
 };
 const resolver_requests_by_call=[];
 const selected=input.call_order??[null,...files.keys(),null,...[...files.keys()].reverse()];
 const calls=selected.map(file=>{
  const source=file===null?undefined:program.getSourceFile(file);
  if(file!==null)assert.ok(source,`selected source must be in Program: ${file}`);
  const start=resolverRequests.length;
  const diagnostics=program.getDeclarationDiagnostics(source).map(diagnostic);
  resolver_requests_by_call.push(resolverRequests.slice(start));
  return {source_file:file,diagnostics};
 });
 assert.deepEqual(writes,[]);
 return {typescript_observation:{calls,writes},owner_observation:{resolver_requests_by_call}};
}
const cases=inputs.map(input=>{const first=observe(input);assert.deepEqual(observe(input),first,input.case_id);return {...input,...first};});
const artifact={version:1,typescript:ts.version,source_commit:'050880ce59e30b356b686bd3144efe24f875ebc8',compiler_sha256:sha256(fs.readFileSync(path.join(root,'vendor/typescript-6.0.3/lib/typescript.js'))),repetitions:2,cases};
const rendered=JSON.stringify(artifact,null,2)+'\n';
if(process.argv[2]==='--write')fs.writeFileSync(output,rendered);else{assert.ok(process.argv[2]===undefined||process.argv[2]==='--check');assert.equal(fs.readFileSync(output,'utf8'),rendered);}
console.log(`declaration getters: ${cases.length} complete call sequences, two identical observations each`);
