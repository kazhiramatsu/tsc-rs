#!/usr/bin/env node
// A40 source inventory, independent of production qualification or readiness.
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';

const seedsPath = 'ratchets/h2-8a-retained-lexical-source-seeds.v1.json';
const outputPath = 'ratchets/h2-8a-retained-lexical-source-inventory.v1.json';
const sha = value => crypto.createHash('sha256').update(value).digest('hex');
const seedBytes = fs.readFileSync(seedsPath);
const spec = JSON.parse(seedBytes);
const sourceText = fs.readFileSync(spec.source.path, 'utf8');
assert.equal(sha(sourceText), spec.source.sha256);
const source = ts.createSourceFile(spec.source.path, sourceText, ts.ScriptTarget.ESNext, true, ts.ScriptKind.JS);
assert.equal(source.parseDiagnostics.length, 0);
const lines = sourceText.split(/(?<=\n)/);
const nodes = [];
function collect(node) { nodes.push(node); ts.forEachChild(node, collect); }
collect(source);
const lineOf = node => source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
const endLineOf = node => source.getLineAndCharacterOfPosition(node.end - 1).line + 1;
const positionKey = node => `${node.getStart(source)}:${node.end}`;
const nodesByPosition = new Map(nodes.map(node => [positionKey(node), node]));
const selected = new Map();
function register(node, group, name, seed) {
  const key = positionKey(node);
  const existing = selected.get(key);
  if (existing) return existing;
  const location = source.getLineAndCharacterOfPosition(node.getStart(source));
  const item = {node, group, name, seed, id: `${group}/${name}@${location.line + 1}:${location.character + 1}`};
  selected.set(key, item);
  return item;
}
for (const seed of spec.seeds) {
  const node = nodesByPosition.get(`${seed.start_utf16}:${seed.end_utf16}`);
  assert.ok(node, seed.name);
  assert.equal(ts.SyntaxKind[node.kind], seed.kind);
  assert.equal(lineOf(node), seed.start);
  assert.equal(sha(node.getText(source)), seed.sha256);
  assert.ok((ts.isFunctionLike(node) && node.body) || ts.isVariableDeclaration(node));
  assert.ok(!selected.has(positionKey(node)), seed.name);
  register(node, seed.group, seed.name, true);
}
function registerNested(node, owner) {
  ts.forEachChild(node, child => {
    let next = owner;
    if (ts.isFunctionLike(child) && child.body) {
      next = register(child, owner.group, child.name?.getText(source) ?? '<callback>', false);
    }
    registerNested(child, next);
  });
}
for (const owner of [...selected.values()]) registerNested(owner.node, owner);
// Determine containment only after all registrations. Earlier inventories
// accidentally left a selected nested function parentless due to seed order.
function selectedParent(node) {
  for (let parent = node.parent; parent; parent = parent.parent) {
    const owner = selected.get(positionKey(parent));
    if (owner) return owner.id;
  }
  return null;
}
const owners = [], predicates = [], calls = [];
for (const item of [...selected.values()].sort((a, b) => a.node.getStart(source) - b.node.getStart(source))) {
  const {node} = item;
  const start = lineOf(node), end = endLineOf(node);
  owners.push({id: item.id, group: item.group, name: item.name,
    parent: selectedParent(node), selected_seed: item.seed, kind: ts.SyntaxKind[node.kind],
    start, end, start_utf16: node.getStart(source), end_utf16: node.end,
    sha256: sha(node.getText(source)), line_span_sha256: sha(lines.slice(start - 1, end).join(''))});
  let callOrder = 0;
  function scan(child, guards = []) {
    if (child !== node && ts.isFunctionLike(child) && child.body) return;
    let expression;
    if (ts.isIfStatement(child)) expression = child.expression;
    else if (ts.isConditionalExpression(child)) expression = child.condition;
    else if (ts.isBinaryExpression(child) && [ts.SyntaxKind.AmpersandAmpersandToken, ts.SyntaxKind.BarBarToken, ts.SyntaxKind.QuestionQuestionToken].includes(child.operatorToken.kind)) expression = child;
    else if (ts.isCaseClause(child)) expression = child.expression;
    else if (ts.isDefaultClause(child)) expression = child;
    else if (ts.isForOfStatement(child) || ts.isForInStatement(child) || ts.isWhileStatement(child) || ts.isDoStatement(child)) expression = child.expression;
    else if (ts.isForStatement(child)) expression = child.condition ?? child;
    if (expression) {
      const text = ts.isDefaultClause(child) ? 'default' : expression.getText(source);
      const id = `${item.id}/predicate@${expression.getStart(source)}:${expression.end}:${ts.SyntaxKind[child.kind]}`;
      predicates.push({id, owner: item.id, line: lineOf(expression), kind: ts.SyntaxKind[child.kind],
        start_utf16: expression.getStart(source), end_utf16: expression.end, text, sha256: sha(text)});
      guards = [...guards, id];
    }
    if (ts.isCallExpression(child) || ts.isNewExpression(child)) {
      calls.push({id: `${item.id}/call@${child.getStart(source)}:${child.end}`, owner: item.id,
        line: lineOf(child), start_utf16: child.getStart(source), end_utf16: child.end,
        syntax_order: callOrder++, callee: child.expression.getText(source),
        arguments: child.arguments?.length ?? 0, enclosing_predicates: guards,
        text_sha256: sha(child.getText(source))});
    }
    ts.forEachChild(child, c => scan(c, guards));
  }
  scan(node);
}
const ids = new Set(owners.map(o => o.id));
assert.equal(ids.size, owners.length);
assert.ok(owners.every(o => o.parent === null || ids.has(o.parent)));
assert.equal(new Set(predicates.map(p => p.id)).size, predicates.length);
assert.equal(new Set(calls.map(c => c.id)).size, calls.length);
const record = {version: 1, slice: spec.slice,
  status: 'Whole selected bodies and nested callbacks inventoried; semantic disposition and transitive helper closure are separate readiness requirements',
  source: spec.source, seeds: {path: seedsPath, sha256: sha(seedBytes)},
  observer: {path: 'scripts/inventory-retained-lexical-owners.mjs', sha256: sha(fs.readFileSync(import.meta.filename))},
  policy: {
    body_coverage: 'All selected bodies, with every nested function or callback represented once under its nearest registered AST ancestor',
    predicate_coverage: 'if, conditional, short-circuit logical, case/default, and loop predicates',
    call_order: 'AST preorder within each owner; syntax_order is not an assertion about execution order. Enclosing predicates identify syntax containment, not evaluated truth or branch polarity.',
    closure: 'No inferred callee resolution or readiness; every call still requires an explicit semantic disposition, frozen premise, or source-backed reachability guard',
  },
  counts: {seeds: spec.seeds.length, owners: owners.length, predicates: predicates.length, calls: calls.length},
  owners, predicates, calls};
const output = JSON.stringify(record, null, 2) + '\n';
assert.ok(process.argv.length === 3 && ['--check', '--write'].includes(process.argv[2]), 'Use --check or --write');
if (process.argv[2] === '--check') assert.equal(fs.readFileSync(outputPath, 'utf8'), output);
else {
  assert.ok(!fs.existsSync(outputPath), 'Do not overwrite a frozen inventory');
  fs.writeFileSync(outputPath, output);
}
console.log(JSON.stringify({output: outputPath, ...record.counts, program_commands: 0, readiness_claim: false}));
