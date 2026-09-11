#!/usr/bin/env node
// Source census for the private retained comma-factory design amendment.
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const sha = text => crypto.createHash('sha256').update(text).digest('hex');
const file = 'vendor/typescript-6.0.3/lib/_tsc.js';
const text = fs.readFileSync(file, 'utf8');
const source = ts.createSourceFile(file, text, ts.ScriptTarget.ESNext, true, ts.ScriptKind.JS);
assert.equal(source.parseDiagnostics.length, 0);
const nodes = [];
function collect(node) { nodes.push(node); ts.forEachChild(node, collect); }
collect(source);
const line = node => source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
const reference = node => ({start: line(node), end: source.getLineAndCharacterOfPosition(node.end - 1).line + 1,
  start_utf16: node.getStart(source), end_utf16: node.end, sha256: sha(node.getText(source))});
const names = ['sameFlatMap', 'isParseTreeNode', 'nodeIsSynthesized', 'positionIsSynthesized',
  'flattenCommaElements', 'createCommaListExpression', 'getNodeId', 'getOriginalNodeId', 'getScriptTransformers',
  'isHoistedVariable', 'hoistFunctionDeclaration', 'getSuperCallFromStatement',
  'findSuperStatementIndexPathWorker', 'formatIdentifier', 'formatIdentifierWorker'];
const owners = names.map(name => {
  const selected = nodes.filter(n => ts.isFunctionLike(n) && n.body && n.name?.text === name);
  assert.equal(selected.length, 1, name);
  return {name, ...reference(selected[0]), body: selected[0].getText(source)};
});
const nearestOwner = node => {
  for (let current = node.parent; current; current = current.parent) {
    if (ts.isFunctionLike(current) && current.body) return {name: current.name?.getText(source) ?? '<callback>', ...reference(current)};
  }
  return null;
};
const directIdCalls = nodes.filter(n => ts.isCallExpression(n) && ts.isIdentifier(n.expression) && n.expression.text === 'getNodeId').map(node => {
  const at = line(node);
  let domain;
  if ([21661, 21701].includes(at)) domain = 'generated-name-source-key';
  else if (at >= 42290 && at <= 80910) domain = 'binder-checker-identity';
  else if (at === 92737) domain = 'original-declaration-key';
  else if (at === 92980) domain = 'generated-identifier-name-map-key';
  else if (at >= 101248 && at <= 102810) domain = 'later-async-transform-or-emit-hook';
  else if (at >= 110924 && at <= 113714) domain = 'later-module-transform-or-substitution';
  else if (at === 120634) domain = 'later-printer-generated-name';
  else assert.fail('Unclassified source identity call ' + at);
  return {...reference(node), text: node.getText(source), owner: nearestOwner(node), domain};
});
assert.equal(directIdCalls.length, 32);
const preRetainedRoots = new Set(['transformTypeScript', 'transformLegacyDecorators', 'transformJsx',
  'transformESNext', 'transformESDecorators', 'transformClassFields']);
const requests = nodes.filter(n => ts.isCallExpression(n) &&
  ['getGeneratedNameForNode', 'getGeneratedPrivateNameForNode', 'getOriginalNodeId'].includes(
    ts.isIdentifier(n.expression) ? n.expression.text : n.expression.name?.text)).flatMap(node => {
  let root = null;
  for (let parent = node.parent; parent; parent = parent.parent) {
    if (ts.isFunctionLike(parent) && preRetainedRoots.has(parent.name?.text)) root = parent.name.text;
  }
  if (!root && line(node) >= 94000) return [];
  return [{...reference(node), text: node.getText(source), owner: nearestOwner(node),
    pipeline_root: root ?? 'shared-helper', arguments: node.arguments.map(n => n.getText(source))}];
});
const record = {version: 1, slice: 'H2.8a-A6-40',
  status: 'Source supplement for comma design and helper closure; not whole A40 readiness',
  source: {path: file, sha256: sha(text)},
  observer: {path: 'scripts/observe-retained-comma-sources.mjs', sha256: sha(fs.readFileSync(import.meta.filename))},
  owners, direct_id_calls: directIdCalls, pre_retained_identity_requests: requests,
  policy: 'Every direct getNodeId call is enumerated. Domain labels are source call ownership; the design amendment supplies the argument/provenance and pass-order proof. Whole new helper bodies supplement the prior220-owner inventory.',
  counts: {owners: owners.length, direct_id_calls: directIdCalls.length, pre_retained_identity_requests: requests.length}};
const output = 'ratchets/h2-8a-retained-comma-sources.v1.json';
const bytes = JSON.stringify(record, null, 2) + '\n';
assert.ok(process.argv.length === 3 && ['--write', '--check'].includes(process.argv[2]));
if (process.argv[2] === '--check') assert.equal(fs.readFileSync(output, 'utf8'), bytes);
else { assert.ok(!fs.existsSync(output)); fs.writeFileSync(output, bytes); }
console.log(JSON.stringify({...record.counts, output, program_commands: 0}));
