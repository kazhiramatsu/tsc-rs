// Complete direct list entry sites in the pinned printer, including non-list
// prologue emission and the state that deliberately survives reset.
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
const startLine = node => source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
const reference = node => ({ start: startLine(node),
  end: source.getLineAndCharacterOfPosition(node.end - 1).line + 1, sha256: sha256(node.getText(source)) });
const entries = new Set(['emitList', 'emitExpressionList', 'emitNodeList', 'emitNodeListItems']);
const calls = nodes.filter(node => ts.isCallExpression(node) && startLine(node) >= 116912
  && startLine(node) <= 121378 && entries.has(node.expression.getText(source)));
assert.equal(calls.length, 51);
const functions = new Set();
const sites = calls.map(node => {
  let owner = node.parent;
  while (owner && !ts.isFunctionDeclaration(owner)) owner = owner.parent;
  assert.ok(owner?.name);
  functions.add(owner);
  return { ...reference(node), owner: owner.name.text, text: node.getText(source) };
});
const supplements = ['getLeadingLineTerminatorCount', 'getEmitListItem', 'emitPrologueDirectives',
  'emitPrologueDirectivesIfNeeded', 'shouldEmitBlockFunctionBodyOnSingleLine', 'writeNode',
  'setSourceFile', 'reset', 'getIdentifierTypeArguments', 'setIdentifierTypeArguments',
  'emitListItemNoParenthesizer', 'emitListItemWithParenthesizerRuleSelector', 'emitListItemWithParenthesizerRule'];
for (const name of supplements) {
  const matches = nodes.filter(node => ts.isFunctionDeclaration(node) && node.name?.text === name
    && (name.includes('IdentifierTypeArguments') || name.includes('ListItem')
      || startLine(node) >= 116912 && startLine(node) <= 121378));
  assert.equal(matches.length, 1, name);
  functions.add(matches[0]);
}
const owners = [...functions].sort((a, b) => a.pos - b.pos).map(node => {
  const predicates = [];
  function inspect(part) {
    if (ts.isIfStatement(part) || ts.isConditionalExpression(part)) {
      const test = ts.isIfStatement(part) ? part.expression : part.condition;
      predicates.push({ ...reference(test), text: test.getText(source) });
    }
    ts.forEachChild(part, inspect);
  }
  inspect(node.body);
  return { name: node.name.text, ...reference(node), body: node.getText(source), predicates };
});
const output = 'ratchets/h2-8a-list-cursor-sources.v1.json';
const artifact = { version: 1, slice: 'H2.8a-A6-40',
  status: 'Source census for list cursor ownership; not a readiness disposition or qualified runtime claim',
  source: { path: file, sha256: sha256(text) },
  observer: { path: 'scripts/observe-list-cursor-sources.mjs', sha256: sha256(fs.readFileSync(import.meta.filename)) },
  sites, owners };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
assert.ok(['--write', '--check'].includes(process.argv[2]));
if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
console.log(JSON.stringify({ output, sites: sites.length, owners: owners.length, sha256: sha256(rendered) }));
