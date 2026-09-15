// A-INT3 focused review controls. Reuse the submitted observer's pinned hook
// adapter; add per-operation entry selection, without changing cleanup behavior.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
const source = fs.readFileSync(new URL('./observe-printer-failures.mjs', import.meta.url), 'utf8');
const submittedSha = '491f5e6f0bcab4fc0894fc6c654a09f1401bc284e9b0e579eb2ddf0a211aac3c';
assert.equal(crypto.createHash('sha256').update(source).digest('hex'), submittedSha);
const vendorUrl = new URL('../vendor/typescript-6.0.3/lib/typescript.js', import.meta.url).href;
const prefix = source.split('const hookCases = hookSpecs.map')[0]
  .replace("'../vendor/typescript-6.0.3/lib/typescript.js'", JSON.stringify(vendorUrl))
  .replace('return { text, utf8_base64:', "return { text: bytes.toString('utf8'), text_utf16: Array.from({length:text.length},(_,i)=>text.charCodeAt(i)), utf8_base64:")
  .replace('removeComments: !!spec.removeComments', 'module: spec.moduleKind, removeComments: !!spec.removeComments')
  .replaceAll("spec.entry === 'printFile'", "(op.entry ?? spec.entry) === 'printFile'")
  .replaceAll("spec.entry === 'printBundle'", "(op.entry ?? spec.entry) === 'printBundle'");
const controls = String.raw`
const targets = files => ({ ...statementTargets(files),
  file: { node: files[0] }, other: { node: files[1] }, bundle: { node: ts.factory.createBundle(files) } });
const select = entry => ({ entry, target: {printNode: 's0', printFile: 'file', printBundle: 'bundle'}[entry] });
const reviewSpecs = [];
for (const from of ['printNode', 'printFile', 'printBundle']) {
  for (const to of ['printNode', 'printFile', 'printBundle']) {
    if (from === to) continue;
    for (const phase of ['before', 'after']) {
      reviewSpecs.push(printNodeSpec('review/' + from + '-to-' + to + '/' + phase, {
        sources: [{name:'main.ts', text: MAIN}, {name:'other.ts', text:'h(z);\r\nk(w);\r\n'}],
        tracked: [K.SourceFile, K.ExpressionStatement, K.Identifier], targets,
        ops: [{...select(to), printer:'fresh'}, {...select(from), fault:identifierFault(phase, 2)}, select(to), select(to)]
      }));
    }
  }
}
// A successful print on the shared printer precedes the fault; a second fault
// happens before any successful recovery can clear the owned writer.
reviewSpecs.push(printNodeSpec('review/success-failure-failure-success', {
  ops: [{target:'s0'}, {target:'s0',fault:identifierFault('before',2)},
    {target:'s1',fault:identifierFault('after',1)}, {target:'s0'}, {target:'s1'}]
}));
// Failure inside a NoNestedComments extent, followed by a DIFFERENT entry.
for (const to of ['printFile', 'printBundle']) {
  reviewSpecs.push(printNodeSpec('review/sticky-comments-to-' + to, {
    sources: [{name:'main.ts',text:MAIN},{name:'other.ts',text:OTHER}],
    tracked: [K.SourceFile,K.ExpressionStatement,K.Identifier],
    targets: files => {ts.setEmitFlags(files[0].statements[0].expression,ts.EmitFlags.NoNestedComments);return targets(files);},
    emit_flags:[{target:'s0',path:'expression',flags:ts.EmitFlags.NoNestedComments}],
    ops:[{...select(to),printer:'fresh'},{target:'s0',fault:identifierFault('before',2)},select(to),select(to)]
  }));
}
// Standalone subtree suppression must also apply to the root's source comments.
reviewSpecs.push(printNodeSpec('review/nested-comments-with-trivia', {
  sources:[{name:'main.ts',text:'/*a*/ f(/*arg*/ x); //t\r\ng(y);\r\n'}],
  targets:files=>{ts.setEmitFlags(files[0].statements[0].expression,ts.EmitFlags.NoNestedComments);return statementTargets(files);},
  emit_flags:[{target:'s0',path:'expression',flags:ts.EmitFlags.NoNestedComments}],
  ops:[{target:'s0',printer:'fresh'},{target:'s0',fault:identifierFault('before',2)},{target:'s0'},{target:'s1'}]
}));
for (const phase of ['success', 'before']) {
  reviewSpecs.push(printNodeSpec('review/bundled-helpers/' + phase, {
    sources:[{name:'main.ts',text:'a(1);\r\nb(2);\r\n'},{name:'other.ts',text:'c(3);\r\nd(4);\r\n'}],
    moduleKind: ts.ModuleKind.AMD, helper: {name:'review:helper',text:'var __reviewHelper = 1;',scoped:false},
    tracked:[K.SourceFile,K.ExpressionStatement,K.Identifier],
    targets:files=>{for(const file of files)ts.addEmitHelper(file,{name:'review:helper',text:'var __reviewHelper = 1;',scoped:false});return targets(files);},
    ops:[{...select('printBundle'),printer:'fresh'},
      {...select('printBundle'),...(phase==='before'?{fault:identifierFault('before',2)}:{})},
      select('printBundle'),select('printFile'),select('printBundle')]
  }));
}
for (const [label, first, second] of [['high',0xd800,0x79],['low',0xdc00,0x79],['pair-across-failure',0xd83d,0xde00]]) {
  reviewSpecs.push(printNodeSpec('review/surrogate/' + label, {
    synthetic_identifiers:{rawA:[first],rawB:[second]},
    targets:files=>({...statementTargets(files),
      rawA:{node:ts.factory.createExpressionStatement(ts.factory.createIdentifier(String.fromCharCode(first))),sourceFile:files[0]},
      rawB:{node:ts.factory.createExpressionStatement(ts.factory.createIdentifier(String.fromCharCode(second))),sourceFile:files[0]}}),
    ops:[{target:'rawB',printer:'fresh'},{target:'rawA',fault:identifierFault('after',1)},{target:'rawB'},{target:'rawB'}]
  }));
}
const cases = reviewSpecs.map(spec => {
  const first = runHooks(spec), second = runHooks(spec);
  assert.deepEqual(second,first,spec.case_id);
  const {targets,...input} = spec;
  return {...input,typescript_observation:first};
});
assert.equal(cases.length,21);
const output = 'crates/emitter/tests/fixtures/printer-failure-review.json';
const rendered = JSON.stringify({version:1,typescript:ts.version,repetitions:2,
  route:'printer-failure-review-controls',submitted_observer_sha256:SUBMITTED_SHA,
  compiler_sha256:sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),cases},null,2)+'\n';
if(process.argv[2]==='--write')fs.writeFileSync(output,rendered,{flag:'wx'});
else assert.equal(fs.readFileSync(output,'utf8'),rendered);
console.log(JSON.stringify({output,cases:cases.length,sha256:sha256(rendered)}));
`.replace('SUBMITTED_SHA', JSON.stringify(submittedSha));
await import('data:text/javascript;base64,' + Buffer.from(prefix + controls).toString('base64'));
