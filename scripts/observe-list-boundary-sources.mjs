// Whole boundary owners plus printer-lifetime list cursor references.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const sha256 = text => crypto.createHash('sha256').update(text).digest('hex');
const file = 'vendor/typescript-6.0.3/lib/_tsc.js';
const text = fs.readFileSync(file, 'utf8');
const source = ts.createSourceFile(file, text, ts.ScriptTarget.ESNext, true, ts.ScriptKind.JS);
assert.equal(source.parseDiagnostics.length, 0);
const nodes = [];
function walk(node) { nodes.push(node); ts.forEachChild(node, walk); }
walk(source);
const names = ['getLeadingLineTerminatorCount', 'getClosingLineTerminatorCount', 'rangeStartPositionsAreOnSameLine', 'rangeEndPositionsAreOnSameLine', 'getStartPositionOfRange', 'positionsAreOnSameLine', 'getLinesBetweenPositions', 'computeLineOfPosition', 'nodeIsSynthesized', 'synthesizedNodeStartsOnNewLine', 'getStartsOnNewLine'];
const owners = names.map(name => {
  const matches = nodes.filter(node => ts.isFunctionLike(node) && node.body && node.name?.text === name);
  assert.equal(matches.length, 1, name);
  const node = matches[0];
  const reference = part => ({ start: source.getLineAndCharacterOfPosition(part.getStart(source)).line + 1,
    end: source.getLineAndCharacterOfPosition(part.end - 1).line + 1, sha256: sha256(part.getText(source)) });
  const calls = [], predicates = [];
  function inspect(part) {
    if (ts.isCallExpression(part)) calls.push({ ...reference(part), text: part.getText(source) });
    if (ts.isIfStatement(part) || ts.isConditionalExpression(part)) {
      const predicate = ts.isIfStatement(part) ? part.expression : part.condition;
      predicates.push({ ...reference(predicate), text: predicate.getText(source) });
    }
    ts.forEachChild(part, inspect);
  }
  inspect(node.body);
  return { name, ...reference(node), body: node.getText(source), calls, predicates };
});
const cursor_references = nodes.filter(node => ts.isIdentifier(node) && node.text === 'nextListElementPos').map(node => ({
  line: source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1,
  parent_kind: ts.SyntaxKind[node.parent.kind], text: node.parent.getText(source),
}));
assert.deepEqual(cursor_references.map(row => row.line), [116946, 120125, 120276]);
const reset = nodes.find(node => ts.isFunctionDeclaration(node) && node.name?.text === 'reset'
  && source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1 === 117117);
assert.ok(reset);
assert.ok(!reset.getText(source).includes('nextListElementPos'));
const artifact = { version: 1, slice: 'H2.8a-A6-40',
  status: 'Source supplement; no whole A40 readiness or production activation',
  source: { path: file, sha256: sha256(text) },
  observer: { path: 'scripts/observe-list-boundary-sources.mjs', sha256: sha256(fs.readFileSync(import.meta.filename)) },
  owners, cursor_references, reset: { start: 117117, body: reset.getText(source), sha256: sha256(reset.getText(source)) } };
const output = 'ratchets/h2-8a-list-boundary-sources.v1.json';
const rendered = JSON.stringify(artifact, null, 2) + '\n';
assert.ok(['--write', '--check'].includes(process.argv[2]));
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, owners: owners.length, sha256: sha256(rendered) }));
