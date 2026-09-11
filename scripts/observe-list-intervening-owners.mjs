// Direct transformed-list comment ownership controls; no Program admission.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const root = path.resolve(import.meta.dirname, '..');
const destination = path.join(root, 'crates/emitter/tests/fixtures/list-intervening-owners.json');
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
if (process.argv[2] === '--write') assert.ok(!fs.existsSync(destination));
const inputs = [];
const texts = {
  inline: 'sink(/* first */ a /* tail a */, /* between */ b, /* third */ c /* tail c */);\n',
  comments: 'sink(/* first */ a /* tail a */, /* between1 */ /* between2 */ b, /* third1 */ /* third2 */ c /* tail c */);\n',
  lines: 'sink(/* first */ a /* tail a */, // between\n b, // third\n c /* tail c */);\n',
};
const recipes = {
  'skip-middle': [0, 2],
  'reverse': [2, 0],
  'synthetic': [0, 'synthetic'],
  'synthetic-comment-range': [0, 'synthetic-comment-range'],
};
for (const container of ['call', 'optional-call', 'new', 'array']) {
  for (const [shape, text] of Object.entries(texts)) for (const [name, recipe] of Object.entries(recipes)) for (const ranged_list of [false, true]) {
    inputs.push({ case_id: `list-intervening/${container}/${shape}/${name}/${ranged_list ? 'ranged-list' : 'synthetic-list'}`,
      container, list_mode: 'selection', recipe, ranged_list, text });
  }
}
function observe(input) {
  const source = ts.createSourceFile('main.ts', input.text, ts.ScriptTarget.ESNext, true, ts.ScriptKind.TS);
  const statement = source.statements[0], call = statement.expression, parsed = call.arguments;
  let supplied;
  assert.equal(input.list_mode, 'selection');
  supplied = ts.factory.createNodeArray(input.recipe.map(entry => {
    if (typeof entry === 'number') return parsed[entry];
    const created = ts.factory.createIdentifier('generated');
    if (entry === 'synthetic-comment-range') ts.setCommentRange(created, parsed[1]);
    else assert.equal(entry, 'synthetic');
    return created;
  }));
  if (input.ranged_list) ts.setTextRange(supplied, parsed);
  let expression;
  switch (input.container) {
    case 'call': expression = ts.factory.createCallExpression(call.expression, undefined, supplied); break;
    case 'optional-call': expression = ts.factory.createCallChain(call.expression, ts.factory.createToken(ts.SyntaxKind.QuestionDotToken), undefined, supplied); break;
    case 'new': expression = ts.factory.createNewExpression(call.expression, undefined, supplied); break;
    case 'array': expression = ts.factory.createArrayLiteralExpression(supplied, false); break;
    default: assert.fail(input.container);
  }
  const array = input.container === 'array' ? expression.elements : expression.arguments;
  const array_state = array === undefined ? null : {
    same_input: array === supplied, pos: array.pos, end: array.end, has_trailing_comma: array.hasTrailingComma,
    elements: array.map(node => ({ kind: ts.SyntaxKind[node.kind], pos: node.pos, end: node.end,
      flags: node.flags, emit_flags: ts.getEmitFlags(node), original_present: node.original !== undefined,
      inner_is_supplied: ts.isParenthesizedExpression(node) ? supplied.includes(node.expression) : null })),
  };
  const updated = ts.factory.updateSourceFile(source, [ts.factory.updateExpressionStatement(statement, expression)]);
  const printed = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed }).printFile(updated);
  const bytes = Buffer.from(printed), starts = ts.computeLineStarts(printed);
  return { array_state, text: printed, utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: printed.length, line: starts.length - 1, column: printed.length - starts.at(-1) } };
}
const cases = inputs.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  return { ...input, typescript_observation: first };
});
assert.equal(cases.length, 96);
const artifact = { version: 1, typescript: ts.version, route: 'direct-factory-and-printer', repetitions: 2,
  source_commit: '050880ce59e30b356b686bd3144efe24f875ebc8',
  compiler_sha256: sha256(fs.readFileSync(path.join(root, 'vendor/typescript-6.0.3/lib/typescript.js'))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(destination, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(destination, 'utf8'), rendered);
console.log(JSON.stringify({ cases: cases.length, repetitions: 2, native_executions: 0, sha256: sha256(rendered) }));
