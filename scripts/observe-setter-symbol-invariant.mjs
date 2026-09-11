import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
const hash=b=>crypto.createHash('sha256').update(b).digest('hex');
assert.equal(ts.version,'6.0.3');
assert.ok(['--write','--check'].includes(process.argv[2]));
const text='export class Foo { set x(supplied) {} }';
const rows=[];
function observe(corrupt){
 const opts={noLib:true,allowJs:true,checkJs:true,declaration:true,target:ts.ScriptTarget.ES2015};
 const host=ts.createCompilerHost(opts,true);
 host.getSourceFile=(name,lang)=>name==='/main.js'?ts.createSourceFile(name,text,lang,true):undefined;
 host.fileExists=name=>name==='/main.js';host.readFile=name=>name==='/main.js'?text:undefined;
 const p=ts.createProgram(['/main.js'],opts,host),c=p.getTypeChecker(),sf=p.getSourceFile('/main.js');
 const cls=sf.statements[0],symbol=c.getSymbolAtLocation(cls.name);
 const prop=c.getPropertyOfType(c.getDeclaredTypeOfSymbol(symbol),'x');
 if(corrupt){prop.declarations=[];symbol.members.get('x').declarations=[];}
 try{const nodes=c.getEmitResolver().symbolToDeclarations(symbol,ts.SymbolFlags.Type,ts.NodeBuilderFlags.NoTruncation);return {output:nodes.map(n=>ts.createPrinter().printNode(ts.EmitHint.Unspecified,n,sf)),error:null};}
 catch(e){return {output:null,error:String(e)};}
}
for(const corrupt of [false,true]){const first=observe(corrupt);assert.deepEqual(observe(corrupt),first);rows.push({case_id:corrupt?'absent-declarations':'bound-setter',corrupt,observation:first});}
const artifact={version:1,typescript:ts.version,source_commit:'050880ce59e30b356b686bd3144efe24f875ebc8',scope:'internal-symbol-invariant; not complete compiler commands or admitted source inputs',repetitions:2,source_text:text,compiler_sha256:hash(fs.readFileSync(new URL('../vendor/typescript-6.0.3/lib/typescript.js',import.meta.url))),observer_sha256:hash(fs.readFileSync(import.meta.filename)),rows};
const rendered=JSON.stringify(artifact,null,2)+'\n';
const output=new URL('../crates/compiler/tests/fixtures/setter-symbol-invariant.json',import.meta.url);
if(process.argv[2]==='--write')fs.writeFileSync(output,rendered);else assert.equal(fs.readFileSync(output,'utf8'),rendered);
console.log('Setter symbol invariant: two internal observations twice; one success and one Debug Failure');
