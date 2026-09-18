// Observe the upstream after-transform AST without modifying it.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from '../../../../../../../vendor/typescript-6.0.3/lib/typescript.js';
const source='declare let g: <T>(...args: any[]) => any;\n{ @g<number> class C {} }\n{ @g()<number> class C {} }\n';
function observe(experimentalDecorators) {
  const nodes=[];
  const result=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ESNext,module:ts.ModuleKind.CommonJS,experimentalDecorators},transformers:{after:[()=>root=>{
    function visit(node) {
      if(node.kind===ts.SyntaxKind.MissingDeclaration) nodes.push({kind:ts.SyntaxKind[node.kind],pos:node.pos,end:node.end,transformFlags:node.transformFlags,modifiers:node.modifiers?.map(m=>({kind:ts.SyntaxKind[m.kind],expression:ts.SyntaxKind[m.expression.kind]}))});
      ts.forEachChild(node,child=>{visit(child);});
    }
    visit(root);return root;
  }]}});
  return {experimentalDecorators,nodes,output:result.outputText};
}
const results=[false,true].map(mode=>{const result=observe(mode);assert.deepEqual(observe(mode),result);return result;});
for(const result of results) {
  assert.equal(result.nodes.length,2);
  assert.ok(result.nodes.every(node=>node.transformFlags===0&&node.modifiers.length===1&&node.modifiers[0].kind==='Decorator'));
}
const data={typescript:ts.version,repetitions:2,purpose:'after-transform structural observation; not native emit compatibility',source,results};
const destination=new URL('./missing-declaration-transform-probe.json',import.meta.url);
const rendered=JSON.stringify(data,null,2)+'\n';
if(process.argv[2]==='--write')fs.writeFileSync(destination,rendered,{flag:'wx'});
else{assert.equal(process.argv[2],'--check');assert.equal(fs.readFileSync(destination,'utf8'),rendered);}
console.log('MissingDeclaration retains decorator children and zero transform flags in both decorator modes');
