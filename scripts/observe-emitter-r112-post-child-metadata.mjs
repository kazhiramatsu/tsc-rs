// Direct printer metadata controls; no claim of full Program/command parity.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import crypto from 'node:crypto';
import path from 'node:path';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const root=path.resolve(import.meta.dirname,'..');
const sha256=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
assert.equal(ts.version,'6.0.3');
assert.ok(['--write','--check'].includes(process.argv[2]));
const text='import { foo /*p😀*/ as bar /*ib🍀*/ } /*c🌊*/ from /*s🧭*/ "m";\nexport { a /*q🔥*/ as b /*eb🌙*/ } /*e🪐*/ from "m";\ndo { x; /*db🎋*/ } /*d💠*/ while (y);\n';
const byte=offset=>Buffer.byteLength(text.slice(0,offset));
const source=()=>ts.createSourceFile('main.ts',text,ts.ScriptTarget.ESNext,true,ts.ScriptKind.TS);
function selection(sf,shape){
 const [i,e,d]=sf.statements;
 switch(shape){
  case 'import-clause': return [i.importClause,i.importClause.namedBindings.elements[0].name.end];
  case 'export-clause': return [e.exportClause,e.exportClause.elements[0].name.end];
  case 'specifier-property': return [i.importClause.namedBindings.elements[0].propertyName,text.indexOf(' as bar')+3];
  case 'do-body': return [d.statement,d.statement.statements[0].end];
  default: assert.fail(shape);
 }
}
function observe(input){
 const sf=source(),[node,end]=selection(sf,input.shape);
 ts.setEmitFlags(node,input.emit_flags);
 if(input.mode==='range')ts.setCommentRange(node,{pos:node.pos,end});
 const output=ts.createPrinter({newLine:ts.NewLineKind.CarriageReturnLineFeed,removeComments:input.remove_comments}).printFile(sf);
 const bytes=Buffer.from(output),lines=ts.computeLineStarts(output);
 return {text:output,utf8_base64:bytes.toString('base64'),utf8_bytes:bytes.length,
  end_utf16:{position:output.length,line:lines.length-1,column:output.length-lines.at(-1)}};
}
const cases=[];
for(const shape of ['import-clause','export-clause','specifier-property','do-body'])
 for(const mode of ['none','no-trailing','range'])for(const remove_comments of [false,true]){
  const [node,end]=selection(source(),shape);
  const input={case_id:`emitter-r112-post-child-metadata/${shape}/${mode}/${remove_comments?'remove':'retain'}`,
   text,shape,mode,selection_kind:ts.SyntaxKind[node.kind],selected_byte_range:{start:byte(node.pos),end:byte(node.end)},
   emit_flags:mode==='no-trailing'?ts.EmitFlags.NoTrailingComments:0,remove_comments,
   comment_range_bytes:mode==='range'?{start:byte(node.pos),end:byte(end)}:null};
  const first=observe(input);assert.deepEqual(observe(input),first,input.case_id);
  cases.push({...input,typescript_observation:first});
 }
assert.equal(cases.length,24);
const artifact={version:1,typescript:ts.version,route:'direct-printer-metadata',repetitions:2,
 compiler_sha256:sha256(fs.readFileSync(path.join(root,'vendor/typescript-6.0.3/lib/typescript.js'))),
 observer_sha256:sha256(fs.readFileSync(import.meta.filename)),cases};
const destination=path.join(root,'crates/emitter/tests/fixtures/emitter-r112-post-child-metadata.json');
const rendered=JSON.stringify(artifact,null,2)+'\n';
if(process.argv[2]==='--write')fs.writeFileSync(destination,rendered,{flag:'wx'});
else assert.equal(fs.readFileSync(destination,'utf8'),rendered);
console.log(`r112 post-child metadata: ${cases.length} cases, two identical direct prints each`);
