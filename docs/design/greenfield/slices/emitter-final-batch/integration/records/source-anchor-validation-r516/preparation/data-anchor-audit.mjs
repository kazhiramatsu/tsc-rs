import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import crypto from 'node:crypto';
const root='/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep';
const out='/tmp/emitter-l0-anchor-repair-r511/data-anchor-audit.json';
const read=p=>fs.readFileSync(path.join(root,p),'utf8');
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
const failures=[];
const matrixPath='crates/oracle/h2-5h-a-gap-matrix.mjs';
const matrixSource=read(matrixPath);
const matrixDeclaration=matrixSource.slice(matrixSource.indexOf('const CAPABILITY_ROWS ='),matrixSource.indexOf('\nfunction fail('));
const rows=vm.runInNewContext(matrixDeclaration+'\nCAPABILITY_ROWS',{}, {timeout:1000});
let positives=0,negatives=0;
for(const row of rows){
 for(const a of row.anchors){positives++;if(!read(a.path).includes(a.symbol))failures.push({generator:matrixPath,capability:row.capability_id,kind:'missing',...a});}
 for(const a of row.absences){negatives++;const exists=a.module!==undefined?fs.existsSync(path.join(root,a.path,a.module)):read(a.path).includes(a.symbol);if(exists)failures.push({generator:matrixPath,capability:row.capability_id,kind:'unexpected-presence',...a});}
}
const closePath='crates/oracle/h2-7a-close.mjs';
const closeSource=read(closePath);
const declarations=closeSource.slice(closeSource.indexOf('const PLAN_RELATIVE_PATH ='),closeSource.indexOf('const SOURCE_COMMIT ='));
const functions=closeSource.slice(closeSource.indexOf('function splitLines('),closeSource.indexOf('function assertRetainedArmsFilled('));
const context={readText:read,sha256:sha,Buffer,requireCondition:(ok,message)=>{if(!ok)throw new Error(message);},fail:message=>{throw new Error(message);}};
const results=vm.runInNewContext(declarations+'\n'+functions+`\nRETAINED_SURFACE_SPECS.map(spec=>{try{return {path:spec.path,arms:retainedArmsFor(spec)}}catch(error){return {path:spec.path,error:error.message}}})`,context,{timeout:5000});
for(const result of results)if(result.error)failures.push({generator:closePath,...result});
const result={scope:'Read-only extraction of original constant tables and original close anchor functions, not a generator run or a mint.',sources:[matrixPath,closePath].map(p=>({path:p,sha256:sha(read(p))})),matrix_rows:rows.length,matrix_positive_anchors:positives,matrix_absences:negatives,close_surfaces:results,failures};
fs.writeFileSync(out,JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({matrix_rows:rows.length,matrix_positive_anchors:positives,matrix_absences:negatives,close_surfaces:results,failures},null,2));
process.exitCode=failures.length?1:0;
