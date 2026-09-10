// Notification is entered after the next pipeline phase selects substitution.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const output = 'crates/emitter/tests/fixtures/emit-pipeline-phases.json';
const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
function observe(input) {
  const source = input.route === 'json' ? ts.parseJsonText('main.json', input.text)
    : ts.createSourceFile('main.ts', input.text, ts.ScriptTarget.ESNext, true);
  assert.equal(source.parseDiagnostics.length, 0);
  const tracked = node => ts.isSourceFile(node) || ts.isExpressionStatement(node)
    || input.route === 'standalone' && ts.isIdentifier(node);
  const events = [];
  const record = (phase, hint, node) => events.push({ phase, hint: ts.EmitHint[hint], kind: node.kind,
    pos: node.pos, end: node.end });
  const printer = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed }, {
    isEmitNotificationEnabled: tracked,
    substituteNode(hint, node) { if (tracked(node)) record('substitute', hint, node); return node; },
    onEmitNode(hint, node, emit) { record('before', hint, node); emit(hint, node); record('after', hint, node); },
  });
  const text = input.route === 'standalone' ? printer.printNode(ts.EmitHint.Unspecified, source.statements[0], source)
    : printer.printFile(source);
  const bytes = Buffer.from(text), starts = ts.computeLineStarts(text);
  return { events, text, utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: text.length, line: starts.length - 1, column: text.length - starts.at(-1) } };
}
const cases = [];
for (const route of ['canonical', 'preserved', 'json', 'standalone']) for (const unicode of [false, true]) {
  const input = { case_id: `emit-pipeline-phases/${route}/${unicode}`, route,
    text: route === 'json' ? `{ "${unicode ? '😀' : 'x'}": 1 }\r\n` : `${unicode ? 'é' : 'value'};\r\n` };
  const observed = observe(input);
  assert.deepEqual(observe(input), observed, input.case_id);
  assert.equal(observed.events[0].phase, 'substitute');
  cases.push({ ...input, typescript_observation: observed });
}
const artifact = { version: 1, typescript: ts.version, repetitions: 2, route: 'direct-factory-and-printer',
  compiler_sha256: sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, cases: cases.length, sha256: sha256(rendered) }));
