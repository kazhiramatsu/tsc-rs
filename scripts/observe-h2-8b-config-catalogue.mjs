// Frozen conversion schemas; no Program or native execution.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
const output='crates/program/tests/fixtures/h2-8b-config-catalogue.json';
assert.equal(ts.version,'6.0.3');assert.ok(['--write','--check'].includes(process.argv[2]));
function shape(option){
 const type=option.type instanceof Map?{kind:'named',values:[...option.type]}:{kind:option.type};
 if(option.type==='list')Object.assign(type,{element:{name:option.element.name,...shape(option.element)},preserve_falsy:!!option.listPreserveFalsyValues,config_dir:!!option.allowConfigDirTemplateSubstitution});
 if(option.type==='object')type.config_dir=!!option.allowConfigDirTemplateSubstitution;
 return {type,file_path:!!option.isFilePath,command_line_only:!!option.isCommandLineOnly,tsconfig_only:!!option.isTSConfigOnly,extra_validation:!!option.extraValidation};
}
const groups={compiler:ts.optionDeclarations,watch:ts.optionsForWatch,acquisition:ts.typeAcquisitionDeclarations};
const artifact={version:1,typescript:ts.version,source_sha256:sha(fs.readFileSync('vendor/typescript-6.0.3/lib/_tsc.js')),compiler_sha256:sha(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),observer_sha256:sha(fs.readFileSync(import.meta.filename)),groups:Object.fromEntries(Object.entries(groups).map(([key,rows])=>[key,rows.map(row=>({name:row.name,...shape(row)}))]))};
if(process.argv[2]==='--write'){assert.ok(!fs.existsSync(output));fs.writeFileSync(output,JSON.stringify(artifact,null,2)+'\n');}else assert.deepEqual(JSON.parse(fs.readFileSync(output)),artifact);
console.log(JSON.stringify({declarations:Object.fromEntries(Object.entries(groups).map(([name,rows])=>[name,rows.length])),sha256:sha(fs.readFileSync(output))}));
