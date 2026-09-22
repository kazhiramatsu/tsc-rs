const fs = require('node:fs');
const assert = require('node:assert/strict');
const path = require('node:path');
const {execFileSync} = require('node:child_process');
const base = '/tmp/emitter-hosted-reference-repair-r578';
const root = '/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep';
const read = p => JSON.parse(p.endsWith('.zst') ? execFileSync('zstd',['-q','-d','--stdout',p],{maxBuffer:64*1024*1024}) : fs.readFileSync(p));
const specs = [
 ['emitter-final-universe.json',217,['inputs/1/sha256']],
 ['emitter-final-universe-plan-base.json.zst',1798,['inputs/1/sha256']],
 ['emitter-jsdoc-original-command.json',1,['selection/0/artifact_sha256']],
 ['h2-5h-parameter-temporaries.json',68,['selection_sha256','parents/0/sha256','parents/1/sha256']],
 ['utf16-original-rows-complete.json',4,['parent/sha256']],
 ['emitter-heritage-boundaries.json',132,['observer_sha256']],
];
const results=[];
for(const [name,count,pointers] of specs){
 const rel='crates/compiler/tests/fixtures/'+name;
 const old=read(path.join(base,'before',rel)),current=read(path.join(root,rel));
 assert.equal(old.cases.length,count);
 assert.deepStrictEqual(current.cases.slice(0,count),old.cases,rel);
 assert.equal(JSON.stringify(current.cases.slice(0,count)),JSON.stringify(old.cases),rel);
 assert.equal(current.cases.length,count+(name==='emitter-heritage-boundaries.json'?8:0));
 const projected=structuredClone(old);
 for(const pointer of pointers){
  const keys=pointer.split('/');let a=projected,b=current;
  for(const key of keys.slice(0,-1)){a=a[key];b=b[key];}
  const key=keys.at(-1);assert.notEqual(a[key],b[key],pointer);a[key]=b[key];
 }
 if(name==='emitter-heritage-boundaries.json')projected.cases.push(...current.cases.slice(count));
 assert.deepStrictEqual(projected,current,rel);
 results.push({fixture:rel,original_cases:count,old_case_payloads_strictly_identical:true,permitted_changes:pointers,new_cases:current.cases.length-count});
}
fs.writeFileSync(path.join(base,'strict-fixture-payload-proof-r591.json'),JSON.stringify({scope:'Node deepStrictEqual and case JSON.stringify equality; complete old case payloads and every other field outside explicitly changed metadata/eight new cases',results},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({fixtures:results.length,original_case_memberships:results.reduce((n,x)=>n+x.original_cases,0),new_case_memberships:8,all_strictly_identical:true}));
