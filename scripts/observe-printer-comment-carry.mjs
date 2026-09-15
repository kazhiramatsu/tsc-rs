// A-INT3-CS: integer comment-container comparisons across source files.
// Reuse the submitted C03 observer's pinned hook adapter, without cleanup.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
const source = fs.readFileSync(new URL('./observe-printer-failures.mjs', import.meta.url), 'utf8');
const submittedSha = '491f5e6f0bcab4fc0894fc6c654a09f1401bc284e9b0e579eb2ddf0a211aac3c';
assert.equal(crypto.createHash('sha256').update(source).digest('hex'), submittedSha);
const vendor = new URL('../vendor/typescript-6.0.3/lib/typescript.js', import.meta.url).href;
const prefix = source.split('const hookCases = hookSpecs.map')[0]
  .replace("'../vendor/typescript-6.0.3/lib/typescript.js'", JSON.stringify(vendor));
const controls = String.raw`
const layouts = [
  ['equal-ascii', '', '', '/*old*/ f(x); //tail\n', '/*new*/ g(y); //next\n', 2],
  ['equal-utf16-different-bytes', '"😀";\n', '"ab";\n', '/*old*/ f(x); //tail\n', '/*new*/ g(y); //next\n', 2],
  ['equal-bytes-different-utf16', '"😀";\n', '"abcd";\n', '/*old*/ f(x); //tail\n', '/*new*/ g(y); //next\n', 2],
  ['different-offset', '"old prefix";\n', '"b";\n', '/*old*/ f(x); //tail\n', '/*new*/ g(y); //next\n', 2],
  ['short-source-then-original', '"a long prefix before the retained position";\n', '', '/*old*/ f(x); //tail\n', 'g(y);\n', 2],
  ['declaration-list-end', '', '', '/*old*/ const a = f(x); //tail\n', '/*new*/ const b = g(y); //next\n', 3],
];
const specs = [];
for (const [layout, leftPrefix, rightPrefix, leftBody, rightBody, occurrence] of layouts) {
  for (const newLine of ['lf','crlf']) {
    for (const phase of ['before','after']) {
      const target_specs = {s0:{source:0,index:leftPrefix?1:0},o0:{source:1,index:rightPrefix?1:0}};
      specs.push(printNodeSpec('comment-carry/' + layout + '/' + newLine + '/' + phase, {
        newLine, target_specs,
        sources:[{name:'main.ts',text:leftPrefix+leftBody+'k(w);\n'},
          {name:'other.ts',text:rightPrefix+rightBody+'q(v);\n'}],
        tracked:[K.VariableStatement,K.ExpressionStatement,K.Identifier],
        targets:files=>Object.fromEntries(Object.entries(target_specs).map(([name,spec])=>[name,
          {node:files[spec.source].statements[spec.index],sourceFile:files[spec.source]}])),
        ops:[{target:'o0',printer:'fresh'}, {target:'s0',fault:identifierFault(phase,occurrence)},
          {target:'o0'}, {target:'s0'}, {target:'o0'}],
      }));
    }
  }
}
const cases = specs.map(spec=>{
  const observed=runHooks(spec);
  assert.deepEqual(observed,runHooks(spec),spec.case_id);
  const {targets,...input}=spec;
  return {...input,typescript_observation:observed};
});
assert.equal(cases.length,24);
const output='crates/emitter/tests/fixtures/printer-comment-carry.json';
const artifact={version:1,typescript:ts.version,repetitions:2,route:'printer-comment-container-reuse',
  compiler_sha256:sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),
  submitted_observer_sha256:SUBMITTED_SHA,cases};
const rendered=JSON.stringify(artifact,null,2)+'\n';
if(process.argv[2]==='--write')fs.writeFileSync(output,rendered,{flag:'wx'});
else assert.equal(fs.readFileSync(output,'utf8'),rendered);
console.log(JSON.stringify({output,cases:cases.length,sha256:sha256(rendered)}));
`.replace('SUBMITTED_SHA', JSON.stringify(submittedSha));
await import('data:text/javascript;base64,' + Buffer.from(prefix + controls).toString('base64'));
