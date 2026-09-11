// Direct final-comma raw/original/comment-range ownership controls; no Program admission.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const root = path.resolve(import.meta.dirname, '..');
const destination = path.join(root, 'crates/emitter/tests/fixtures/list-trailing-token-owners.json');
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
if (process.argv[2] === '--write') assert.ok(!fs.existsSync(destination));
const inputs = [];
const texts = {
  inline: 'sink(a, /* last */ b /* before comma */, /* after comma */);\n',
  lines: 'sink(a, /* last */ b\n/* before comma */ ,\n/* after comma */);\n',
  header: '/* file */ sink(a, /* last */ b /* before comma */, /* after comma */);\n',
};
for (const [shape, text] of Object.entries(texts)) for (const last_provenance of ['parsed', 'clone', 'range-only', 'comment-only']) {
  for (const flags of [0, ts.EmitFlags.NoTrailingComments]) for (const ranged_list of [false, true]) for (const has_trailing_comma of [false, true]) {
    const input = { case_id: `list-trailing-token/${shape}/${last_provenance}/${flags}/${ranged_list ? 'ranged-list' : 'synthetic-list'}/${has_trailing_comma ? 'comma' : 'no-comma'}`,
      container: 'array', list_mode: 'trailing-token', last_provenance, flags, ranged_list, has_trailing_comma, text };
    inputs.push(input);
    if (shape !== 'header' && last_provenance === 'parsed' && ranged_list) {
      inputs.push({ ...input, case_id: input.case_id + '/only-jsdoc', only_print_js_doc_style: true });
    }
  }
}
function observe(input) {
  const source = ts.createSourceFile('main.ts', input.text, ts.ScriptTarget.ESNext, true, ts.ScriptKind.TS);
  const statement = source.statements[0], call = statement.expression, parsed = call.arguments;
  let supplied;
  assert.equal(input.list_mode, 'trailing-token');
  assert.equal(parsed.length, 2);
  assert.equal(parsed.hasTrailingComma, true);
  let last;
  switch (input.last_provenance) {
    case 'parsed': last = parsed[1]; break;
    case 'clone': last = ts.factory.cloneNode(parsed[1]); break;
    case 'range-only': last = ts.setTextRange(ts.factory.createIdentifier('generated'), parsed[1]); break;
    case 'comment-only': last = ts.setCommentRange(ts.factory.createIdentifier('generated'), parsed[1]); break;
    default: assert.fail(input.last_provenance);
  }
  if (input.flags) ts.setEmitFlags(last, input.flags);
  supplied = ts.factory.createNodeArray([last], input.has_trailing_comma);
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
  const printed = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed, onlyPrintJsDocStyle: input.only_print_js_doc_style === true }).printFile(updated);
  const bytes = Buffer.from(printed), starts = ts.computeLineStarts(printed);
  return { array_state, text: printed, utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: printed.length, line: starts.length - 1, column: printed.length - starts.at(-1) } };
}
const cases = inputs.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  return { ...input, typescript_observation: first };
});
assert.equal(cases.length, 104);
const artifact = { version: 1, typescript: ts.version, route: 'direct-factory-and-printer', repetitions: 2,
  source_commit: '050880ce59e30b356b686bd3144efe24f875ebc8',
  compiler_sha256: sha256(fs.readFileSync(path.join(root, 'vendor/typescript-6.0.3/lib/typescript.js'))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(destination, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(destination, 'utf8'), rendered);
console.log(JSON.stringify({ cases: cases.length, repetitions: 2, native_executions: 0, sha256: sha256(rendered) }));
