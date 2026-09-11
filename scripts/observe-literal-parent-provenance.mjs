// Original literal spelling requires a positioned node and an actual parent.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';

const output = 'crates/emitter/tests/fixtures/literal-parent-provenance.json';
const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
function observe(input) {
  const source = input.route === 'json'
    ? ts.parseJsonText('main.json', input.token)
    : ts.createSourceFile('main.ts', input.token + ';', ts.ScriptTarget.ESNext, input.parents);
  if (input.route === 'json' && input.parents) ts.setParentRecursive(source, true);
  assert.equal(source.parseDiagnostics.length, 0);
  const parsed = source.statements[0].expression;
  assert.ok(ts.isStringLiteral(parsed));
  assert.equal(!!parsed.parent, input.parents);
  if (input.operation === 'parsed-synthesized-flag') parsed.flags |= ts.NodeFlags.Synthesized;
  let node = parsed;
  if (input.operation.startsWith('clone')) node = ts.factory.cloneNode(parsed);
  if (input.operation === 'clone-ranged') ts.setTextRange(node, parsed);
  const state = node => ({ kind: node.kind, pos: node.pos, end: node.end, flags: node.flags,
    emit_flags: ts.getEmitFlags(node), original_present: !!node.original,
    parent: node.parent ? { kind: node.parent.kind, pos: node.parent.pos, end: node.parent.end } : null,
    value_utf16: Array.from({ length: node.text.length }, (_, index) => node.text.charCodeAt(index)) });
  const tree_state = { parsed: state(parsed), node: state(node), same_parsed: node === parsed };
  const text = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed,
    neverAsciiEscape: input.never_ascii_escape })
    .printNode(ts.EmitHint.Unspecified, node, source);
  const bytes = Buffer.from(text), starts = ts.computeLineStarts(text);
  return { tree_state, text, utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: text.length, line: starts.length - 1, column: text.length - starts.at(-1) } };
}
const tokens = ['"\\u0061"', '"é😀"', '"\\u00e9\\uD83D\\uDE00"', '"\\uD800"'];
const cases = [];
for (const route of ['typescript', 'json']) for (const parents of [false, true])
for (const [index, token] of tokens.entries())
for (const operation of ['parsed', 'parsed-synthesized-flag', 'clone', 'clone-ranged'])
for (const never_ascii_escape of [false, true]) {
  const input = { case_id: `literal-parent/${route}/${parents}/${index}/${operation}/${never_ascii_escape}`,
    route, parents, token, operation, never_ascii_escape };
  const observed = observe(input);
  assert.deepEqual(observe(input), observed, input.case_id);
  cases.push({ ...input, typescript_observation: observed });
}
assert.equal(cases.length, 128);
// Retain the setup failure discovered by the initial observer recipe. A
// parsed, unparented node cannot acquire emit metadata through setEmitFlags.
// These are source-only failure observations, separate from the print rows.
const setup_failure_observations = [];
for (const route of ['typescript', 'json']) for (const flags of [0, ts.EmitFlags.NoAsciiEscaping]) {
  const failure = () => {
    const source = route === 'json' ? ts.parseJsonText('main.json', '"é"')
      : ts.createSourceFile('main.ts', '"é";', ts.ScriptTarget.ESNext, false);
    try { ts.setEmitFlags(source.statements[0].expression, flags); }
    catch (error) { return { name: error.name, message: error.message }; }
    assert.fail('setEmitFlags on an unparented parse node must fail');
  };
  const error = failure();
  assert.deepEqual(failure(), error);
  setup_failure_observations.push({ route, flags, error });
}
const artifact = { version: 1, typescript: ts.version, repetitions: 2, route: 'direct-factory-and-printer',
  compiler_sha256: sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), setup_failure_observations, cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, cases: cases.length, sha256: sha256(rendered) }));
