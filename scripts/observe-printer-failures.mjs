// Failure order and continuation of the fixed TypeScript printer.
//
// Part 1 (hooks fixture): public PrintHandlers faults on printNode / printFile /
// printBundle, each followed by recovery prints on the SAME printer and by fresh
// controls. No adapter adds try/finally: an `after` hook that never runs is the
// observation. The Rust contract replays every row of this fixture.
//
// Part 2 (probes fixture): upstream-only surfaces that Rust cannot reach through
// its public printer API (internal writeNode/writeFile with a caller-owned
// writer or source-map generator, node-array/token hooks, printList, and the
// compiler command with a throwing custom transformer). Recorded as evidence.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
const hooksOutput = 'crates/emitter/tests/fixtures/printer-failure-hooks.json';
const probesOutput = 'crates/emitter/tests/fixtures/printer-failure-probes.json';
const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.ok(['--write', '--check'].includes(process.argv[2]));
const K = ts.SyntaxKind;
const measure = text => {
  const bytes = Buffer.from(text), starts = ts.computeLineStarts(text);
  return { text, utf8_base64: bytes.toString('base64'), utf8_bytes: bytes.length,
    end_utf16: { position: text.length, line: starts.length - 1, column: text.length - starts.at(-1) } };
};
const parse = (name, text) => {
  const file = ts.createSourceFile(name, text, ts.ScriptTarget.ESNext, true);
  assert.equal(file.parseDiagnostics.length, 0, name);
  return file;
};
const uniqueStatement = () => ts.factory.createVariableStatement(undefined, ts.factory.createVariableDeclarationList([
  ts.factory.createVariableDeclaration(ts.factory.createUniqueName('x'), undefined, undefined, ts.factory.createNumericLiteral('1'))],
  ts.NodeFlags.Const));
const printerOptions = spec => ({ newLine: spec.newLine === 'lf' ? ts.NewLineKind.LineFeed : ts.NewLineKind.CarriageReturnLineFeed,
  removeComments: !!spec.removeComments });

// ---------------------------------------------------------------- part 1: hooks
function runHooks(spec) {
  const files = spec.sources.map(source => parse(source.name, source.text));
  const targets = spec.targets(files);
  const events = [], results = [];
  let current = null;
  const tracked = node => spec.tracked.includes(node.kind);
  const count = (phase, node) => {
    const key = `${phase}:${node.kind}`, n = (current.counts.get(key) ?? 0) + 1;
    current.counts.set(key, n);
    return n;
  };
  const record = (phase, hint, node) => events.push({ op: current.index, phase, hint: ts.EmitHint[hint], kind: node.kind, pos: node.pos, end: node.end });
  const fault = (phase, node, n) => {
    const f = current.op.fault;
    if (f && f.phase === phase && f.kind === node.kind && f.occurrence === n) throw new Error(`printer-failure:${phase}:${ts.SyntaxKind[node.kind]}:${n}`);
  };
  const handlers = {
    isEmitNotificationEnabled: tracked,
    substituteNode(hint, node) {
      if (!tracked(node)) return node;
      const n = count('substitute', node);
      record('substitute', hint, node);
      fault('substitute', node, n);
      const s = current.op.substitution;
      if (s && s.kind === node.kind && s.occurrence === n) return ts.factory.createIdentifier(s.replacement);
      return node;
    },
    onEmitNode(hint, node, emit) {
      const n = count('before', node);
      record('before', hint, node);
      fault('before', node, n);
      emit(hint, node);
      const m = count('after', node);
      record('after', hint, node);
      fault('after', node, m);
    },
  };
  const options = printerOptions(spec);
  const shared = ts.createPrinter(options, handlers);
  spec.ops.forEach((op, index) => {
    current = { index, op, counts: new Map() };
    const printer = op.printer === 'fresh' ? ts.createPrinter(options, handlers) : shared;
    const target = targets[op.target];
    let text;
    try {
      if (spec.entry === 'printFile') text = printer.printFile(target.node);
      else if (spec.entry === 'printBundle') text = printer.printBundle(target.node);
      else text = printer.printNode(ts.EmitHint[op.hint ?? 'Unspecified'], target.node, target.sourceFile);
    } catch (error) {
      results.push({ op: index, status: 'threw', error: error.message });
      return;
    }
    results.push({ op: index, status: 'returned', ...measure(text) });
  });
  return { events, results };
}
const MAIN = '/*a*/ f(x); //t\r\ng(y);\r\n';
const OTHER = '/*b*/ h(z); //u\r\nk(w);\r\n';
const statementTargets = files => ({
  s0: { node: files[0].statements[0], sourceFile: files[0] },
  s1: { node: files[0].statements[1], sourceFile: files[0] },
  o0: { node: files[1]?.statements[0], sourceFile: files[1] },
});
const printNodeSpec = (id, extra) => ({ case_id: `printer-failure/printNode/${id}`, entry: 'printNode', newLine: 'crlf',
  sources: [{ name: 'main.ts', text: MAIN }], tracked: [K.ExpressionStatement, K.Identifier], targets: statementTargets,
  rust_counterpart: 'StandaloneNode', ...extra });
const identifierFault = (phase, occurrence) => ({ phase, kind: K.Identifier, occurrence });
const hookSpecs = [
  // Nested before-fault after "/*a*/ f(" was written; the same statement re-printed twice.
  printNodeSpec('before/identifier-2/recover-same', { ops: [
    { target: 's0', printer: 'fresh' }, { target: 's0', fault: identifierFault('before', 2) },
    { target: 's0' }, { target: 's0' }] }),
  printNodeSpec('before/identifier-2/recover-same-lf', { newLine: 'lf', ops: [
    { target: 's0', printer: 'fresh' }, { target: 's0', fault: identifierFault('before', 2) },
    { target: 's0' }, { target: 's0' }] }),
  printNodeSpec('before/identifier-2/recover-other-statement', { ops: [
    { target: 's1', printer: 'fresh' }, { target: 's0', fault: identifierFault('before', 2) },
    { target: 's1' }, { target: 's1' }] }),
  // The other file has the same layout, so its positions equal the carried container.
  printNodeSpec('before/identifier-2/recover-other-source-same-positions', {
    sources: [{ name: 'main.ts', text: MAIN }, { name: 'other.ts', text: OTHER }], ops: [
      { target: 'o0', printer: 'fresh' }, { target: 's0', fault: identifierFault('before', 2) },
      { target: 'o0' }, { target: 'o0' }] }),
  // The root's own before-fault: nothing is written before the notification phase.
  printNodeSpec('before/statement-1/recover-same', { ops: [
    { target: 's0', printer: 'fresh' }, { target: 's0', fault: { phase: 'before', kind: K.ExpressionStatement, occurrence: 1 } },
    { target: 's0' }, { target: 's0' }] }),
  printNodeSpec('substitute/identifier-2/recover-same', { ops: [
    { target: 's0', printer: 'fresh' }, { target: 's0', fault: identifierFault('substitute', 2) },
    { target: 's0' }, { target: 's0' }] }),
  printNodeSpec('after/identifier-1/recover-same', { ops: [
    { target: 's0', printer: 'fresh' }, { target: 's0', fault: identifierFault('after', 1) },
    { target: 's0' }, { target: 's0' }] }),
  // The statement's own after-fault: its comment phases completed, so only the writer carries.
  printNodeSpec('after/statement-1/recover-same', { ops: [
    { target: 's0', printer: 'fresh' }, { target: 's0', fault: { phase: 'after', kind: K.ExpressionStatement, occurrence: 1 } },
    { target: 's0' }, { target: 's0' }] }),
  // NoNestedComments on the call: the fault leaves commentsDisabled set.
  printNodeSpec('before/nested-comments/identifier-2/recover-same', {
    targets: files => { ts.setEmitFlags(files[0].statements[0].expression, ts.EmitFlags.NoNestedComments); return statementTargets(files); },
    emit_flags: [{ target: 's0', path: 'expression', flags: ts.EmitFlags.NoNestedComments }], ops: [
      { target: 's0', printer: 'fresh' }, { target: 's0', fault: identifierFault('before', 2) },
      { target: 's0' }, { target: 's0' }, { target: 's1' }] }),
  printNodeSpec('before/remove-comments/identifier-2/recover-same', { removeComments: true, ops: [
    { target: 's0', printer: 'fresh' }, { target: 's0', fault: identifierFault('before', 2) },
    { target: 's0' }, { target: 's0' }] }),
  // Substitution x -> y, then the after-fault of the ORIGINAL x.
  printNodeSpec('substitution-y/after/identifier-2/recover-same', { ops: [
    { target: 's0', printer: 'fresh' },
    { target: 's0', substitution: { kind: K.Identifier, occurrence: 2, replacement: 'y' }, fault: identifierFault('after', 2) },
    { target: 's0' }, { target: 's0' }] }),
  // Substitution control without any fault: the substitute prints, the hooks see the original.
  printNodeSpec('substitution-y/success/recover-same', { ops: [
    { target: 's0', printer: 'fresh' }, { target: 's0', substitution: { kind: K.Identifier, occurrence: 2, replacement: 'y' } },
    { target: 's0' }] }),
  // Non-BMP text precedes the fault: UTF-16 column carry.
  printNodeSpec('before/non-bmp/identifier-2/recover-same', { sources: [{ name: 'main.ts', text: 'f("😀", x); //t\r\ng(y);\r\n' }], ops: [
    { target: 's0', printer: 'fresh' }, { target: 's0', fault: identifierFault('before', 2) },
    { target: 's0' }, { target: 's0' }] }),
  // Block bodies: the writer indent and line state carry into the next print.
  ...['crlf', 'lf'].map(newLine => printNodeSpec(`before/block-indent/identifier-2/recover-other-block-${newLine}`, { newLine,
    sources: [{ name: 'main.ts', text: '{\r\n    f(x);\r\n}\r\n{\r\n    g(y);\r\n}\r\n' }], tracked: [K.Block, K.Identifier], ops: [
      { target: 's1', printer: 'fresh' }, { target: 's0', fault: identifierFault('before', 2) },
      { target: 's1' }, { target: 's1' }] })),
  // List item fault: nextListElementPos keeps the third item's position (A6-40
  // cursor lifecycle recipe: a call-ranged array whose first element starts on
  // a later line than the call; the cursor suppresses that leading newline).
  printNodeSpec('before/list-item-3/cursor-array', { sources: [{ name: 'main.ts', text: 'f(\r\n a, /*c1*/ b, c\r\n);\r\n' }], tracked: [K.Identifier],
    targets: files => {
      const call = files[0].statements[0].expression, [, b, c] = call.arguments;
      const array = ts.setOriginalNode(ts.setTextRange(ts.factory.createArrayLiteralExpression([b, c], false), call), call);
      return { s0: { node: files[0].statements[0], sourceFile: files[0] }, array_bc: { node: array, sourceFile: files[0] } };
    }, ops: [
      { target: 'array_bc', hint: 'Expression', printer: 'fresh' }, { target: 's0', fault: identifierFault('before', 3) },
      { target: 'array_bc', hint: 'Expression' }, { target: 'array_bc', hint: 'Expression' }] }),
  // Generated names: a fault after x_1 was generated, then a new unique name on the same printer.
  printNodeSpec('unique-name/after/statement-1/recover-new-unique', { tracked: [K.VariableStatement],
    targets: files => ({ u1: { node: uniqueStatement(), sourceFile: files[0] }, u2: { node: uniqueStatement(), sourceFile: files[0] } }), ops: [
      { target: 'u1', printer: 'fresh' }, { target: 'u1', fault: { phase: 'after', kind: K.VariableStatement, occurrence: 1 } },
      { target: 'u2' }, { target: 'u2', printer: 'fresh' }] }),
  printNodeSpec('unique-name/success/twice', { tracked: [K.VariableStatement],
    targets: files => ({ u1: { node: uniqueStatement(), sourceFile: files[0] }, u2: { node: uniqueStatement(), sourceFile: files[0] } }), ops: [
      { target: 'u1', printer: 'fresh' }, { target: 'u1' }, { target: 'u2' }] }),
  // printFile: source-file roots.
  { case_id: 'printer-failure/printFile/before/statement-2/recover-same', entry: 'printFile', newLine: 'crlf', rust_counterpart: 'SourceFile',
    sources: [{ name: 'main.ts', text: MAIN }], tracked: [K.SourceFile, K.ExpressionStatement],
    targets: files => ({ file: { node: files[0] } }), ops: [
      { target: 'file', printer: 'fresh' }, { target: 'file', fault: { phase: 'before', kind: K.ExpressionStatement, occurrence: 2 } },
      { target: 'file' }, { target: 'file' }] },
  // A fault INSIDE the first statement carries its call container into the next printFile.
  { case_id: 'printer-failure/printFile/before/identifier-2/recover-same', entry: 'printFile', newLine: 'crlf', rust_counterpart: 'SourceFile',
    sources: [{ name: 'main.ts', text: MAIN }], tracked: [K.SourceFile, K.ExpressionStatement, K.Identifier],
    targets: files => ({ file: { node: files[0] } }), ops: [
      { target: 'file', printer: 'fresh' }, { target: 'file', fault: identifierFault('before', 2) },
      { target: 'file' }, { target: 'file' }] },
  // Shebang and prologue precede the SourceFile notification (writeFile order).
  { case_id: 'printer-failure/printFile/before/source-file-1/prologue/recover-same', entry: 'printFile', newLine: 'crlf', rust_counterpart: 'SourceFile',
    sources: [{ name: 'main.ts', text: '#!/usr/bin/env node\r\n"use strict";\r\n/*a*/ f(x); //t\r\n' }], tracked: [K.SourceFile, K.ExpressionStatement],
    targets: files => ({ file: { node: files[0] } }), ops: [
      { target: 'file', printer: 'fresh' }, { target: 'file', fault: { phase: 'before', kind: K.SourceFile, occurrence: 1 } },
      { target: 'file' }, { target: 'file' }] },
  { case_id: 'printer-failure/printFile/after/source-file-1/recover-same', entry: 'printFile', newLine: 'crlf', rust_counterpart: 'SourceFile',
    sources: [{ name: 'main.ts', text: MAIN }], tracked: [K.SourceFile, K.ExpressionStatement],
    targets: files => ({ file: { node: files[0] } }), ops: [
      { target: 'file', printer: 'fresh' }, { target: 'file', fault: { phase: 'after', kind: K.SourceFile, occurrence: 1 } },
      { target: 'file' }, { target: 'file' }] },
  { case_id: 'printer-failure/printFile/before/statement-2/recover-other-file', entry: 'printFile', newLine: 'crlf', rust_counterpart: 'SourceFile',
    sources: [{ name: 'main.ts', text: MAIN }, { name: 'other.ts', text: OTHER }], tracked: [K.SourceFile, K.ExpressionStatement],
    targets: files => ({ file: { node: files[0] }, other: { node: files[1] } }), ops: [
      { target: 'other', printer: 'fresh' }, { target: 'file', fault: { phase: 'before', kind: K.ExpressionStatement, occurrence: 2 } },
      { target: 'other' }, { target: 'other' }] },
  // printBundle: two sources, fault in the second.
  { case_id: 'printer-failure/printBundle/before/statement-2/recover-same', entry: 'printBundle', newLine: 'crlf', rust_counterpart: 'Bundle',
    sources: [{ name: 'a.ts', text: 'a(1);\r\n' }, { name: 'b.ts', text: '/*b*/ b(2);\r\n' }], tracked: [K.SourceFile, K.ExpressionStatement],
    targets: files => ({ bundle: { node: ts.factory.createBundle(files) } }), ops: [
      { target: 'bundle', printer: 'fresh' }, { target: 'bundle', fault: { phase: 'before', kind: K.ExpressionStatement, occurrence: 2 } },
      { target: 'bundle' }, { target: 'bundle' }] },
  { case_id: 'printer-failure/printBundle/success/twice', entry: 'printBundle', newLine: 'crlf', rust_counterpart: 'Bundle',
    sources: [{ name: 'a.ts', text: 'a(1);\r\n' }, { name: 'b.ts', text: '/*b*/ b(2);\r\n' }], tracked: [K.SourceFile, K.ExpressionStatement],
    targets: files => ({ bundle: { node: ts.factory.createBundle(files) } }), ops: [
      { target: 'bundle', printer: 'fresh' }, { target: 'bundle' }, { target: 'bundle' }] },
];
const hookCases = hookSpecs.map(spec => {
  const observation = runHooks(spec);
  assert.deepEqual(runHooks(spec), observation, spec.case_id);
  const { targets, ...rest } = spec;
  return { ...rest, typescript_observation: observation };
});
assert.equal(hookCases.length, 25);
assert.equal(new Set(hookCases.map(c => c.case_id)).size, hookCases.length);

// ---------------------------------------------------------------- part 2: probes
const WRITER_METHODS = ['write', 'rawWrite', 'writeLiteral', 'writeLine', 'increaseIndent', 'decreaseIndent', 'writeKeyword', 'writeOperator',
  'writeParameter', 'writeProperty', 'writePunctuation', 'writeSpace', 'writeStringLiteral', 'writeSymbol', 'writeTrailingSemicolon', 'writeComment'];
function faultingWriter(calls, fault, newLine) {
  const inner = ts.createTextWriter(newLine);
  const counts = new Map();
  const wrapped = { ...inner };
  for (const method of WRITER_METHODS) wrapped[method] = (...args) => {
    const n = (counts.get(method) ?? 0) + 1;
    counts.set(method, n);
    calls.push({ method, text: typeof args[0] === 'string' ? args[0] : null });
    if (fault && fault.method === method && fault.occurrence === n) throw new Error(`writer-fault:${method}:${n}`);
    return inner[method](...args);
  };
  return wrapped;
}
const WRITER_TEXT = '/*a*/ const c = "s"; //t\r\ng(y);\r\n';
function writerProbe(id, fault) {
  const file = parse('main.ts', WRITER_TEXT);
  const printer = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed });
  const calls = [];
  const writer = faultingWriter(calls, fault, '\r\n');
  const results = [];
  try {
    printer.writeNode(ts.EmitHint.Unspecified, file.statements[0], file, writer);
    results.push({ op: 0, entry: 'writeNode', status: 'returned', ...measure(writer.getText()) });
  } catch (error) {
    results.push({ op: 0, entry: 'writeNode', status: 'threw', error: error.message, partial: measure(writer.getText()) });
  }
  results.push({ op: 1, entry: 'printNode', status: 'returned', ...measure(printer.printNode(ts.EmitHint.Unspecified, file.statements[1], file)) });
  results.push({ op: 2, entry: 'printNode', status: 'returned', ...measure(printer.printNode(ts.EmitHint.Unspecified, file.statements[0], file)) });
  return { case_id: `printer-failure/writeNode/writer-fault/${id}`, entry: 'writeNode', rust_counterpart: 'absent',
    reason: 'Rust TextWriter writes are infallible and printer-owned; no caller-owned writer entry exists',
    source: WRITER_TEXT, fault, typescript_observation: { writer_calls: calls, results } };
}
function faultingGenerator(calls, fault, file) {
  const host = { getCanonicalFileName: name => name, useCaseSensitiveFileNames: () => true, getCurrentDirectory: () => '/' };
  const inner = ts.createSourceMapGenerator(host, file, '', '', {});
  const counts = new Map();
  const wrapped = { ...inner };
  for (const method of ['addSource', 'setSourceContent', 'addName', 'addMapping', 'appendSourceMap']) wrapped[method] = (...args) => {
    const n = (counts.get(method) ?? 0) + 1;
    counts.set(method, n);
    calls.push({ method, args: args.map(a => typeof a === 'object' && a !== null ? '[object]' : a) });
    if (fault && fault.method === method && fault.occurrence === n) throw new Error(`map-fault:${method}:${n}`);
    return inner[method](...args);
  };
  wrapped.inner = inner;
  return wrapped;
}
function mapProbe(id, fault) {
  const file = parse('main.ts', WRITER_TEXT);
  const printer = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed });
  const results = [];
  for (let op = 0; op < 2; op++) {
    const calls = [];
    const writer = ts.createTextWriter('\r\n');
    const generator = faultingGenerator(calls, op === 0 ? fault : null, 'main.js');
    try {
      printer.writeFile(file, writer, generator);
      results.push({ op, entry: 'writeFile', status: 'returned', ...measure(writer.getText()), generator_calls: calls, map: generator.inner.toJSON() });
    } catch (error) {
      results.push({ op, entry: 'writeFile', status: 'threw', error: error.message, partial: measure(writer.getText()), generator_calls: calls, map: generator.inner.toJSON() });
    }
  }
  return { case_id: `printer-failure/writeFile/map-fault/${id}`, entry: 'writeFile', rust_counterpart: 'absent',
    reason: 'Rust source-map recording is print-owned and infallible; the second generator observes the mostRecentlyAddedSourceMapSource fast path',
    source: WRITER_TEXT, fault, typescript_observation: { results } };
}
function arrayTokenProbe(id, handlerName, occurrence, text, hint) {
  const file = parse('main.ts', text);
  let count = 0;
  const events = [];
  const handlers = { [handlerName]: node => {
    count += 1;
    events.push({ handler: handlerName, occurrence: count, kind: node ? node.kind : null, pos: node ? node.pos : null, end: node ? node.end : null });
    if (count === occurrence) throw new Error(`${handlerName}:${count}`);
  } };
  const printer = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed }, handlers);
  const results = [];
  try { results.push({ op: 0, status: 'returned', ...measure(printer.printNode(ts.EmitHint[hint], file.statements[0], file)) }); }
  catch (error) { results.push({ op: 0, status: 'threw', error: error.message }); }
  count = 1e9;
  results.push({ op: 1, status: 'returned', ...measure(printer.printNode(ts.EmitHint.Unspecified, file.statements[1], file)) });
  return { case_id: `printer-failure/printNode/${id}`, entry: 'printNode', rust_counterpart: 'absent',
    reason: 'Rust Transformer has no node-array or token hooks', source: text, handler: handlerName, occurrence,
    typescript_observation: { events, results } };
}
function printListProbe() {
  const file = parse('main.ts', 'f(a, /*c1*/ b, c);\r\ng(y);\r\n');
  let count = 0;
  const events = [];
  const printer = ts.createPrinter({ newLine: ts.NewLineKind.CarriageReturnLineFeed }, {
    isEmitNotificationEnabled: node => node.kind === K.Identifier,
    onEmitNode(hint, node, emit) {
      count += 1;
      events.push({ phase: 'before', hint: ts.EmitHint[hint], kind: node.kind, pos: node.pos, end: node.end });
      if (count === 2) throw new Error('printList:before:Identifier:2');
      emit(hint, node);
      events.push({ phase: 'after', hint: ts.EmitHint[hint], kind: node.kind, pos: node.pos, end: node.end });
    } });
  const results = [];
  try { results.push({ op: 0, status: 'returned', ...measure(printer.printList(ts.ListFormat.CommaListElements, file.statements[0].expression.arguments, file)) }); }
  catch (error) { results.push({ op: 0, status: 'threw', error: error.message }); }
  count = 1e9;
  results.push({ op: 1, status: 'returned', ...measure(printer.printNode(ts.EmitHint.Unspecified, file.statements[1], file)) });
  return { case_id: 'printer-failure/printList/before/identifier-2/then-printNode', entry: 'printList', rust_counterpart: 'absent',
    reason: 'PrintRequest::NodeList is a typed Unsupported request in Rust', source: file.text, typescript_observation: { events, results } };
}
function compilerProbe(id, options, files, armFault) {
  const writes = [];
  const sources = new Map(Object.entries(files));
  const host = {
    getSourceFile: (name, language) => sources.has(name) ? ts.createSourceFile(name, sources.get(name), language, true) : undefined,
    getDefaultLibFileName: () => '/lib.d.ts', writeFile: (name, data) => writes.push({ fileName: name, data }),
    getCurrentDirectory: () => '/', getCanonicalFileName: name => name, useCaseSensitiveFileNames: () => true, getNewLine: () => '\n',
    fileExists: name => sources.has(name), readFile: name => sources.get(name), directoryExists: () => true, getDirectories: () => [],
  };
  const program = ts.createProgram([...sources.keys()], options, host);
  const diagnostics = ts.getPreEmitDiagnostics(program).map(d => ({ code: d.code, file: d.file?.fileName ?? null }));
  const runs = [];
  for (let op = 0; op < 2; op++) {
    writes.length = 0;
    const armed = armFault && op === 0;
    const transformers = armFault ? { before: [context => {
      context.enableEmitNotification(K.ExpressionStatement);
      const previous = context.onEmitNode;
      context.onEmitNode = (hint, node, emit) => {
        if (armed && ts.isExpressionStatement(node) && node.getSourceFile()?.fileName === '/b.ts') throw new Error('compiler-hook-fault:/b.ts');
        previous(hint, node, emit);
      };
      return file => file;
    }] } : undefined;
    try {
      const result = program.emit(undefined, undefined, undefined, false, transformers);
      runs.push({ op, status: 'returned', emit_skipped: result.emitSkipped, emitted_files: result.emittedFiles ?? null,
        emit_diagnostics: result.diagnostics.length, writes: writes.map(w => ({ ...w })) });
    } catch (error) {
      runs.push({ op, status: 'threw', error: error.message, writes: writes.map(w => ({ ...w })) });
    }
  }
  return { case_id: `printer-failure/compiler/${id}`, entry: 'program.emit', rust_counterpart: 'absent',
    reason: 'Rust admits no custom transformers; E-OUTPUT-SCRIPT constructs every artifact before the first sink callback',
    options, files, typescript_observation: { pre_emit_diagnostics: diagnostics, runs } };
}
const probeSpecs = [
  () => writerProbe('comment-1', { method: 'writeComment', occurrence: 1 }),
  () => writerProbe('keyword-1', { method: 'writeKeyword', occurrence: 1 }),
  () => writerProbe('string-1', { method: 'writeStringLiteral', occurrence: 1 }),
  () => writerProbe('line-1', { method: 'writeLine', occurrence: 1 }),
  () => writerProbe('none', null),
  () => mapProbe('addMapping-1', { method: 'addMapping', occurrence: 1 }),
  () => mapProbe('addMapping-3', { method: 'addMapping', occurrence: 3 }),
  () => mapProbe('none-reuse-new-generator', null),
  () => arrayTokenProbe('array-hook/before-array-1/then-printNode', 'onBeforeEmitNodeArray', 1, 'f(a, b);\r\ng(y);\r\n', 'Unspecified'),
  () => arrayTokenProbe('array-hook/after-array-1/then-printNode', 'onAfterEmitNodeArray', 1, 'f(a, b);\r\ng(y);\r\n', 'Unspecified'),
  () => arrayTokenProbe('token-hook/before-token-1/then-printNode', 'onBeforeEmitToken', 1, 'f(a + b);\r\ng(y);\r\n', 'Unspecified'),
  () => printListProbe(),
  () => compilerProbe('hook-fault/second-file', { noLib: true, target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.CommonJS },
    { '/a.ts': 'const a = 1;\na;\n', '/b.ts': 'const b = 2;\nb;\n' }, true),
  () => compilerProbe('noEmitOnError/type-error', { noLib: true, noEmitOnError: true, target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.CommonJS },
    { '/a.ts': 'const a: string = 1;\na;\n', '/b.ts': 'const b = 2;\nb;\n' }, false),
];
const probeCases = probeSpecs.map(make => {
  const first = make(), second = make();
  assert.deepEqual(second, first, first.case_id);
  return first;
});
assert.equal(probeCases.length, 14);

const compilerSha = sha256(fs.readFileSync('vendor/typescript-6.0.3/lib/typescript.js'));
const observerSha = sha256(fs.readFileSync(import.meta.filename));
const write = (output, cases, route) => {
  const artifact = { version: 1, typescript: ts.version, repetitions: 2, route, compiler_sha256: compilerSha, observer_sha256: observerSha, cases };
  const rendered = JSON.stringify(artifact, null, 2) + '\n';
  if (process.argv[2] === '--write') fs.writeFileSync(output, rendered, { flag: 'wx' });
  else assert.equal(fs.readFileSync(output, 'utf8'), rendered);
  console.log(JSON.stringify({ output, cases: cases.length, sha256: sha256(rendered) }));
};
write(hooksOutput, hookCases, 'direct-printer-hook-faults');
write(probesOutput, probeCases, 'upstream-only-printer-probes');
