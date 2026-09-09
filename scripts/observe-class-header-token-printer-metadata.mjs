// Direct printer metadata controls; these do not claim Program/command parity.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import crypto from 'node:crypto';
import path from 'node:path';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const root=path.resolve(import.meta.dirname,'..');
const destination=path.join(root,'crates/emitter/tests/fixtures/class-header-token-printer-metadata.json');
const sha256=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
assert.equal(ts.version,'6.0.3');assert.ok(['--write','--check'].includes(process.argv[2]));
const shapes=[
 ['named','/* 😀 outer */ class /* keyword */ Named /* name end */ {}\n'],
 ['anonymous','/* outer */ class /* keyword */ {}\n'],
 ['default','/* outer */ export default class /* keyword */ {}\n'],
 ['expression','const Value = /* outer */ class /* keyword */ Inner /* name end */ {};\n'],
];
const cases=[];
function observe(input){
 const source=ts.createSourceFile('main.ts',input.text,ts.ScriptTarget.ESNext,true,ts.ScriptKind.TS);
 const selected=[];function visit(n){if(ts.isClassDeclaration(n)||ts.isClassExpression(n))selected.push(n);ts.forEachChild(n,visit);}visit(source);
 assert.equal(selected.length,1);ts.setEmitFlags(selected[0],input.emit_flags);
 const printer=ts.createPrinter({newLine:ts.NewLineKind.CarriageReturnLineFeed,removeComments:input.remove_comments});
 const text=printer.printFile(source),bytes=Buffer.from(text),lineStarts=ts.computeLineStarts(text);
 return {text,utf8_base64:bytes.toString('base64'),utf8_bytes:bytes.length,
  end_utf16:{position:text.length,line:lineStarts.length-1,column:text.length-lineStarts.at(-1)}};
}
for(const [shape,text] of shapes)for(const [flagName,emit_flags] of [['none',0],['no-nested',ts.EmitFlags.NoNestedComments],['no-own',ts.EmitFlags.NoComments],['no-own-or-nested',ts.EmitFlags.NoComments|ts.EmitFlags.NoNestedComments]])for(const remove_comments of [false,true]){
 const input={case_id:`class-header-token-printer-metadata/${shape}/${flagName}/${remove_comments?'remove':'retain'}`,text,emit_flags,remove_comments};
 const first=observe(input);assert.deepEqual(observe(input),first,input.case_id);cases.push({...input,typescript_observation:first});
}
assert.equal(cases.length,32);
const artifact={version:1,typescript:ts.version,source_commit:'050880ce59e30b356b686bd3144efe24f875ebc8',route:'direct-printer-metadata',repetitions:2,
 compiler_sha256:sha256(fs.readFileSync(path.join(root,'vendor/typescript-6.0.3/lib/typescript.js'))),observer_sha256:sha256(fs.readFileSync(import.meta.filename)),cases};
const rendered=JSON.stringify(artifact,null,2)+'\n';if(process.argv[2]==='--write')fs.writeFileSync(destination,rendered);else assert.equal(fs.readFileSync(destination,'utf8'),rendered);
console.log(`Class header printer metadata: ${cases.length} direct observations, two identical prints each`);
