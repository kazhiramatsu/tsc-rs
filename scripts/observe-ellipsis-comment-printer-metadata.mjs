// Direct printer metadata controls; these do not claim Program/command parity.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import crypto from 'node:crypto';
import path from 'node:path';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const root=path.resolve(import.meta.dirname,'..');
const destination=path.join(root,'crates/emitter/tests/fixtures/ellipsis-comment-printer-metadata.json');
const sha256=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
assert.equal(ts.version,'6.0.3');assert.ok(['--write','--check'].includes(process.argv[2]));
const shapes=[
 ['parameter-token','function collect(... /* rest */ values: number[]) {}\n','DotDotDotToken','main.ts'],
 ['parameter-parent','function collect(... /* rest */ values: number[]) {}\n','Parameter','main.ts'],
 ['array-binding-token','const [... /* rest */ values] = source;\n','DotDotDotToken','main.ts'],
 ['array-binding-parent','const [... /* rest */ values] = source;\n','BindingElement','main.ts'],
 ['object-binding-token','const {... /* rest */ value} = source;\n','DotDotDotToken','main.ts'],
 ['object-binding-parent','const {... /* rest */ value} = source;\n','BindingElement','main.ts'],
 ['named-tuple-token','type Values = [... /* rest */ values: number[]];\n','DotDotDotToken','main.ts'],
 ['named-tuple-parent','type Values = [... /* rest */ values: number[]];\n','NamedTupleMember','main.ts'],
 ['jsx-expression-token','const node = <div>{... /* rest */ values /* tail */}</div>;\n','DotDotDotToken','main.tsx'],
 ['jsx-expression-parent','const node = <div>{... /* rest */ values /* tail */}</div>;\n','JsxExpression','main.tsx'],
 ['rest-type-parent','type Values = [... /* rest */ number[] /* tail */];\n','RestType','main.ts'],
 ['jsx-attribute-parent','const node = <div {... /* rest */ value /* tail */} />;\n','JsxSpreadAttribute','main.tsx'],
];
const cases=[];
function observe(input){
 const source=ts.createSourceFile(input.file_name,input.text,ts.ScriptTarget.ESNext,true,input.file_name.endsWith('.tsx')?ts.ScriptKind.TSX:ts.ScriptKind.TS);
 const selected=[];function visit(n){if(ts.SyntaxKind[n.kind]===input.selection_kind)selected.push(n);ts.forEachChild(n,visit);}visit(source);
 assert.equal(selected.length,1);ts.setEmitFlags(selected[0],input.emit_flags);
 const printer=ts.createPrinter({newLine:ts.NewLineKind.CarriageReturnLineFeed,removeComments:input.remove_comments});
 const text=printer.printFile(source),bytes=Buffer.from(text),lineStarts=ts.computeLineStarts(text);
 return {text,utf8_base64:bytes.toString('base64'),utf8_bytes:bytes.length,
  end_utf16:{position:text.length,line:lineStarts.length-1,column:text.length-lineStarts.at(-1)}};
}
for(const [shape,text,selection_kind,file_name] of shapes)for(const [flagName,emit_flags] of [['none',0],['no-leading',ts.EmitFlags.NoLeadingComments],['no-trailing',ts.EmitFlags.NoTrailingComments],['no-nested',ts.EmitFlags.NoNestedComments],['no-own',ts.EmitFlags.NoComments],['no-own-or-nested',ts.EmitFlags.NoComments|ts.EmitFlags.NoNestedComments]])for(const remove_comments of [false,true]){
 const input={case_id:`ellipsis-comment-printer-metadata/${shape}/${flagName}/${remove_comments?'remove':'retain'}`,text,selection_kind,file_name,emit_flags,remove_comments,declaration_syntax:true};
 const first=observe(input);assert.deepEqual(observe(input),first,input.case_id);cases.push({...input,typescript_observation:first});
}
assert.equal(cases.length,144);
const artifact={version:1,typescript:ts.version,source_commit:'050880ce59e30b356b686bd3144efe24f875ebc8',route:'direct-printer-metadata',repetitions:2,
 compiler_sha256:sha256(fs.readFileSync(path.join(root,'vendor/typescript-6.0.3/lib/typescript.js'))),observer_sha256:sha256(fs.readFileSync(import.meta.filename)),cases};
const rendered=JSON.stringify(artifact,null,2)+'\n';if(process.argv[2]==='--write')fs.writeFileSync(destination,rendered);else assert.equal(fs.readFileSync(destination,'utf8'),rendered);
console.log(`Ellipsis comment printer metadata: ${cases.length} direct observations, two identical prints each`);
