import fs from 'node:fs';
import ts from '/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/vendor/typescript-6.0.3/lib/typescript.js';
const prefixes=['export','declare','default','public','async','export default','declare export'];
const bodies=["import './dep';","import value from './dep';","export { value };","export * from './dep';","export default 1;","export = value;"];
const cases=[];
for(const prefix of prefixes)for(const body of bodies){
 const text=`${prefix} /* a */ ${body}\n`,seen=[],writes=[];
 const options={target:ts.ScriptTarget.ESNext,module:ts.ModuleKind.ESNext,sourceMap:true,noLib:true};
 const host={getSourceFile:(f,v)=>f==='/project/main.ts'?ts.createSourceFile(f,text,v,true):undefined,getDefaultLibFileName:()=>'',writeFile:(path,text)=>writes.push({path,text}),getCurrentDirectory:()=>'/project',getDirectories:()=>[],fileExists:f=>f==='/project/main.ts',readFile:f=>f==='/project/main.ts'?text:undefined,getCanonicalFileName:f=>f,useCaseSensitiveFileNames:()=>true,getNewLine:()=> '\n'};
 const program=ts.createProgram(['/project/main.ts'],options,host);
 const result=program.emit(undefined,undefined,undefined,false,{after:[()=>source=>{for(const n of source.statements)seen.push({kind:ts.SyntaxKind[n.kind],modifiers:n.modifiers?.map(m=>ts.SyntaxKind[m.kind])??[],pos:n.pos,modifierEnd:n.modifiers?.end});return source;}]});
 cases.push({text,seen,writes,skipped:result.emitSkipped});
}
fs.mkdirSync('/tmp/emitter-retained-modifier-probe-r407',{recursive:false});
fs.writeFileSync('/tmp/emitter-retained-modifier-probe-r407/oracle.json',JSON.stringify({scope:'Read-only after-transform inspection, noLib probe only; not full command qualification',cases},null,2)+'\n');
console.log(JSON.stringify({cases:cases.length,retained:cases.filter(c=>c.seen.some(n=>n.modifiers.length)).map(c=>({text:c.text,seen:c.seen,javascript:c.writes.find(w=>w.path.endsWith('.js'))?.text}))},null,2));
