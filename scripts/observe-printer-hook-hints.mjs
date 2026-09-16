// API1.2-HINT: declaration names and initializer expressions use different
// printer hints, including substitution, failures, and subsequent calls.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';

const source = fs.readFileSync(new URL('./observe-printer-failures.mjs', import.meta.url), 'utf8');
const submittedSha = '491f5e6f0bcab4fc0894fc6c654a09f1401bc284e9b0e579eb2ddf0a211aac3c';
assert.equal(crypto.createHash('sha256').update(source).digest('hex'), submittedSha);
const vendor = new URL('../vendor/typescript-6.0.3/lib/typescript.js', import.meta.url).href;
const originalPredicate = 's && s.kind === node.kind && s.occurrence === n';
assert.equal(source.split(originalPredicate).length, 2);
const prefix = source.split('const hookCases = hookSpecs.map')[0]
  .replace("'../vendor/typescript-6.0.3/lib/typescript.js'", JSON.stringify(vendor))
  .replace(originalPredicate, originalPredicate + ' && s.hint === ts.EmitHint[hint]');
const controls = String.raw`
const specs = [];
for (const [shape, left, right] of [
  ['const', 'const a = f(x);', 'const b = g(y);'],
  ['let-multiple', 'let a = f(x), c = h(z);', 'let b = g(y), d = k(w);'],
  ['array-binding', 'const [a] = f(x);', 'const [b] = g(y);'],
]) {
  for (const newLine of ['lf', 'crlf']) {
    for (const [site, kind, hint] of [
      ['binding-name', K.Identifier, 'Unspecified'],
      ['initializer', K.CallExpression, 'Expression'],
    ]) {
      for (const action of ['success', 'before', 'substitute', 'after', 'replace', 'replace-after']) {
        const op = {target:'s0'};
        if (['before', 'substitute', 'after'].includes(action)) {
          op.fault = {phase:action,kind,occurrence:1};
        }
        if (action.startsWith('replace')) {
          op.substitution = {kind,occurrence:1,hint,replacement:'replacement'};
          if (action === 'replace-after') op.fault = {phase:'after',kind,occurrence:1};
        }
        specs.push(printNodeSpec('hook-hints/' + [shape,newLine,site,action].join('/'), {
          newLine,
          sources:[{name:'main.ts',text:'/*left*/ '+left+' //tail\nk(w);\n'},
            {name:'other.ts',text:'/*right*/ '+right+' //next\nq(v);\n'}],
          tracked:[K.VariableStatement,K.VariableDeclarationList,K.VariableDeclaration,
            K.Identifier,K.CallExpression],
          ops:[{target:'s0',printer:'fresh'}, {target:'s0'}, op,
            {target:'s0'}, {target:'o0'}, {target:'s0'}],
        }));
      }
    }
  }
}
const cases = specs.map(spec => {
  const observed = runHooks(spec);
  assert.deepEqual(observed, runHooks(spec), spec.case_id);
  const {targets, ...input} = spec;
  return {...input, typescript_observation:observed};
});
assert.equal(cases.length, 72);
assert.equal(new Set(cases.map(c => c.case_id)).size, cases.length);
const output = 'crates/emitter/tests/fixtures/printer-hook-hints.json';
const artifact = {version:1, typescript:ts.version, repetitions:2,
  route:'printer-declaration-hook-hints',
  compiler_sha256:sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),
  submitted_observer_sha256:SUBMITTED_SHA, cases};
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, {flag:'wx'});
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({output,cases:cases.length,sha256:sha256(rendered)}));
`.replace('SUBMITTED_SHA', JSON.stringify(submittedSha));
await import('data:text/javascript;base64,' + Buffer.from(prefix + controls).toString('base64'));
