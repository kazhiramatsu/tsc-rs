// Direct factory/printer controls, independent of Program admission.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const root = path.resolve(import.meta.dirname, '..');
const destination = path.join(root, 'crates/emitter/tests/fixtures/comma-argument-factory.json');
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
if (process.argv[2] === '--write') assert.ok(!fs.existsSync(destination));
const text = 'sink(/* first */ a /* tail a */, /* between */ b, c /* tail c */);\n';
const inputs = [];
for (const container of ['call', 'optional-call', 'new', 'array']) {
  for (const expression of ['comma-list', 'binary-comma']) for (const flags of [0, ts.EmitFlags.NoTrailingComments]) for (const ranged of [false, true]) {
    inputs.push({ case_id: `comma-argument-factory/${container}/${expression}/${flags}/${ranged ? 'ranged' : 'synthetic'}`,
      container, expression, flags, ranged, list_mode: 'comma', text });
  }
  for (const list_mode of ['plain', 'hole', 'absent']) {
    inputs.push({ case_id: `comma-argument-factory/${container}/${list_mode}`, container, list_mode, text });
  }
}
function observe(input) {
  const source = ts.createSourceFile('main.ts', input.text, ts.ScriptTarget.ESNext, true, ts.ScriptKind.TS);
  const statement = source.statements[0], call = statement.expression, parsed = call.arguments;
  let supplied;
  switch (input.list_mode) {
    case 'plain': supplied = parsed; break;
    case 'hole':
      supplied = ts.factory.createNodeArray([parsed[0], ts.factory.createOmittedExpression()], false);
      ts.setTextRange(supplied, parsed);
      break;
    case 'absent': supplied = undefined; break;
    case 'comma': {
      const expression = input.expression === 'comma-list' ? ts.factory.createCommaListExpression(parsed)
        : parsed.slice(1).reduce((left, right) => ts.factory.createBinaryExpression(left, ts.SyntaxKind.CommaToken, right), parsed[0]);
      if (input.ranged) ts.setTextRange(expression, { pos: parsed[0].pos, end: parsed.at(-1).end });
      if (input.flags) ts.setEmitFlags(expression, input.flags);
      supplied = ts.factory.createNodeArray([expression]);
      break;
    }
    default: assert.fail(input.list_mode);
  }
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
assert.equal(cases.length, 44);
const artifact = { version: 1, typescript: ts.version, route: 'direct-factory-and-printer', repetitions: 2,
  source_commit: '050880ce59e30b356b686bd3144efe24f875ebc8',
  compiler_sha256: sha256(fs.readFileSync(path.join(root, 'vendor/typescript-6.0.3/lib/typescript.js'))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(destination, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(destination, 'utf8'), rendered);
console.log(JSON.stringify({ cases: cases.length, repetitions: 2, native_executions: 0, sha256: sha256(rendered) }));
