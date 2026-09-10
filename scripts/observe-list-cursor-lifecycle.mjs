// Observable printer-instance list cursor: nested lists and repeated calls.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');
const output = 'ratchets/h2-8a-list-cursor-lifecycle.v1.json';
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
const recipes = ['first', 'last', 'empty', 'nested-first', 'nested-last', 'comma-first',
  'call-first', 'object-first', 'object-scalar-after', 'same-position-clone', 'unicode-other-source'];
function observe(recipe) {
  const source = ts.createSourceFile('main.ts', 'target(\n a, b\n);\n', ts.ScriptTarget.ESNext, true);
  const call = source.statements[0].expression, [first, last] = call.arguments;
  const array = nodes => ts.factory.createArrayLiteralExpression(nodes, false);
  const target = ts.setOriginalNode(ts.setTextRange(array(call.arguments), call), call);
  let seed, seedSource = source;
  switch (recipe) {
    case 'first': seed = array([first]); break;
    case 'last': seed = array([last]); break;
    case 'empty': seed = array([]); break;
    case 'nested-first': seed = array([array([first])]); break;
    case 'nested-last': seed = array([array([first]), last]); break;
    case 'comma-first': seed = ts.factory.createCommaListExpression([first]); break;
    case 'call-first': seed = ts.factory.createCallExpression(ts.factory.createIdentifier('f'), undefined, [first]); break;
    case 'object-first': seed = ts.factory.createObjectLiteralExpression([
      ts.factory.createPropertyAssignment('x', array([first]))], false); break;
    case 'object-scalar-after': seed = ts.factory.createObjectLiteralExpression([
      ts.factory.createPropertyAssignment('x', array([first])),
      ts.factory.createPropertyAssignment('y', first)], false); break;
    case 'same-position-clone': seed = array([ts.setTextRange(ts.factory.cloneNode(first), first)]); break;
    case 'unicode-other-source': {
      seedSource = ts.createSourceFile('other.ts', 'f("😀",a);\n', ts.ScriptTarget.ESNext, true);
      const item = seedSource.statements[0].expression.arguments[1];
      assert.equal(item.pos, first.pos);
      assert.notEqual(Buffer.byteLength(seedSource.text.slice(0, item.pos)), Buffer.byteLength(source.text.slice(0, first.pos)));
      seed = array([item]);
      break;
    }
    default: assert.fail(recipe);
  }
  const printer = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed });
  const print = (node, file) => {
    const text = printer.printNode(ts.EmitHint.Expression, node, file);
    const bytes = Buffer.from(text), starts = ts.computeLineStarts(text);
    return { text, utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
      end_utf16: { position: text.length, line: starts.length - 1, column: text.length - starts.at(-1) } };
  };
  const fresh = print(target, source);
  const seed_output = print(seed, seedSource);
  const after_seed = print(target, source);
  const after_target = print(target, source);
  assert.deepEqual(after_target, fresh);
  const suppresses = ['first', 'nested-first', 'comma-first', 'call-first', 'object-first', 'same-position-clone', 'unicode-other-source'].includes(recipe);
  assert.equal(after_seed.text !== fresh.text, suppresses, recipe);
  return { target_source: source.text, seed_source: seedSource.text, target_first_pos_utf16: first.pos,
    fresh, seed_output, after_seed, after_target };
}
const cases = recipes.map(recipe => {
  const observation = observe(recipe);
  assert.deepEqual(observe(recipe), observation, recipe);
  return { case_id: `list-cursor/${recipe}`, recipe, typescript_observation: observation };
});
const artifact = { version: 1, typescript: ts.version, repetitions: 2, native_executions: 0,
  status: 'Fresh source evidence; printer cursor lifecycle remains unimplemented and unqualified',
  compiler_sha256: sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js')),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, cases: cases.length, sha256: sha256(rendered) }));
