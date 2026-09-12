// CFG1e: ordered config host calls, optional extended-config cache, and reuse.
// node scripts/observe-h2-8b-config-reuse.mjs --write|--check
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const root=path.resolve(import.meta.dirname,'..');
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
const inputPath='crates/program/tests/fixtures/h2-8b-config-reuse-inputs.json';
const outputPath='crates/program/tests/fixtures/h2-8b-config-reuse.json';
const inputs=JSON.parse(fs.readFileSync(path.join(root,inputPath),'utf8'));
assert.equal(ts.version,'6.0.3');assert.equal(inputs.cases.length,26);
assert.ok(['--write','--check'].includes(process.argv[2]));
if(process.argv[2]==='--write')assert.ok(!fs.existsSync(path.join(root,outputPath)),'retain frozen observations');
function diagnostic(d){return {code:d.code,category:ts.DiagnosticCategory[d.category].toLowerCase(),file:d.file?.fileName??null,start:d.start??null,length:d.length??null,message:ts.flattenDiagnosticMessageText(d.messageText,'\n'),related_information:(d.relatedInformation??[]).map(diagnostic)};}
function observe(input){
 const base=ts.getDirectoryPath(input.config_path),canonical=input.use_case_sensitive_file_names?s=>s:ts.toFileNameLowerCase;
 const key=name=>canonical(ts.getNormalizedAbsolutePath(name,base));
 const files=new Map(input.files.map(f=>[key(f.path),f.text]));files.set(key(input.config_path),input.config);
 const cache=input.cache?new Map():undefined;let calls=[];
 function call(operation,name,args={}){
  calls.push({operation,path:name,...args});const fault=input.fault;
  if(fault&&fault.operation===operation&&key(fault.path)===key(name)){
   if(fault.mode==='absent')return true;
   throw Object.assign(new Error('synthetic fault'),{config_host_fault:{operation,path:name,detail:'synthetic fault'}});
  }return false;
 }
 const host={useCaseSensitiveFileNames:input.use_case_sensitive_file_names,
  fileExists:name=>call('fileExists',name)?false:files.has(key(name)),
  readFile:name=>call('readFile',name)?undefined:files.get(key(name)),
  readDirectory:(name,extensions,excludes,includes,depth)=>{call('readDirectory',name,{extensions,excludes:excludes??null,includes:includes??null,depth:depth??null});return [];}};
 return input.steps.map(step=>{
  for(const update of step.updates??[])files.set(key(update.path),update.text);
  if(step.clear_cache)cache?.clear();calls=[];
  const source=ts.parseJsonText(input.config_path,input.config);
  try{
   const parsed=ts.parseJsonSourceFileConfigFileContent(source,host,base,undefined,input.config_path,undefined,undefined,cache);
   const options={...parsed.options};delete options.configFile;
   return {calls,raw:JSON.parse(JSON.stringify(parsed.raw)),options:JSON.parse(JSON.stringify(options)),watch_options:parsed.watchOptions??null,type_acquisition:parsed.typeAcquisition,compile_on_save:parsed.compileOnSave,file_names:parsed.fileNames,extended_source_files:parsed.options.configFile?.extendedSourceFiles??[],root_parse_diagnostics:source.parseDiagnostics.map(diagnostic),parsed_errors:parsed.errors.map(diagnostic),config_diagnostics:ts.getConfigFileParsingDiagnostics(parsed).map(diagnostic)};
  }catch(e){assert.ok(e.config_host_fault,e.stack);return {calls,host_failure:e.config_host_fault};}
 });
}
const cases=inputs.cases.map(input=>{const first=observe(input);assert.deepEqual(observe(input),first,input.case_id);return {case_id:input.case_id,typescript_observation:first};});
const sourcePath='vendor/typescript-6.0.3/lib/_tsc.js',sourceBytes=fs.readFileSync(path.join(root,sourcePath)),lines=sourceBytes.toString('utf8').split('\n');
const anchors=[['getExtendedConfig',39460,39500],['performCompilation',132860,132899],['executeCommandLineWorker-config-cache',132652,132654]].map(([symbol,start_line,end_line])=>({symbol,start_line,end_line,sha256:sha(lines.slice(start_line-1,end_line).join('\n')+'\n')}));
const artifact={version:1,slice:inputs.slice,typescript:ts.version,source:{path:sourcePath,sha256:sha(sourceBytes),anchors},compiler_sha256:sha(fs.readFileSync(path.join(root,'vendor/typescript-6.0.3/lib/typescript.js'))),observer_sha256:sha(fs.readFileSync(import.meta.filename)),inputs:{path:inputPath,sha256:sha(fs.readFileSync(path.join(root,inputPath)))},repetitions:2,program_executions:0,config_parse_attempts:inputs.cases.reduce((n,c)=>n+c.steps.length*2,0),cases};
if(process.argv[2]==='--write')fs.writeFileSync(path.join(root,outputPath),JSON.stringify(artifact,null,2)+'\n');else assert.deepEqual(JSON.parse(fs.readFileSync(path.join(root,outputPath),'utf8')),artifact);
console.log(JSON.stringify({cases:cases.length,config_parse_attempts:artifact.config_parse_attempts,sha256:sha(fs.readFileSync(path.join(root,outputPath)))}));
