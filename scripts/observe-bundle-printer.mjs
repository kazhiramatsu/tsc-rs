// Focused H2.7d shared-writer references. Program observations are complete;
// initial Rust qualification is the internal global-script bundle printer.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';
import { createHermeticDirectoryOverlay } from '../crates/oracle/vfs-directory-overlay.mjs';

const root = path.resolve(import.meta.dirname, '..');
const target = path.join(root, 'crates/emitter/tests/fixtures/bundle-printer.json');
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.equal(process.versions.node, fs.readFileSync(path.join(root, '.node-version'), 'utf8').trim());
const defaults = { target: ts.ScriptTarget.ES2015, module: ts.ModuleKind.None, outFile: '/project/bundle.js',
  strict: false, alwaysStrict: false, newLine: ts.NewLineKind.LineFeed, noErrorTruncation: true, skipDefaultLibCheck: true };
const inputs = [];
function add(case_id, files, options = {}, extra = {}) {
  inputs.push({ case_id, files: files.map(([name, text]) => ({ path: '/project/' + name, text })),
    options: { ...defaults, ...options }, current_directory: '/project', use_case_sensitive_file_names: true, ...extra });
}
add('source-order', [['z.ts', 'const z: number = 1;\n'], ['a.ts', "const a: string = 'a';\n"]]);
add('reverse-roots', [['z.ts', 'const z: number = 1;\n'], ['a.ts', "const a: string = 'a';\n"]], {}, { roots: ['/project/a.ts', '/project/z.ts'] });
add('distinct-prologues', [['a.ts', '"one";\nconst a = 1;\n'], ['b.ts', '"two";\nconst b = 2;\n']]);
add('duplicate-prologue-comments', [['a.ts', '// first\n"use strict";\nconst a = 1;\n'], ['b.ts', "// second\n'use strict';\nconst b = 2;\n"]]);
add('detached-before-prologues', [['a.ts', '/* detached */\n\n"use strict";\nconst a = 1;\n'], ['b.ts', '/* second */\n\n"use strict";\nconst b = 2;\n']]);
add('all-prologues', [['a.ts', '"one";\n"two";\n'], ['b.ts', '"two";\n"three";\n']]);
add('escaped-prologue-dedup', [['a.ts', '"u\\u0073e strict";\n"🌸";\nconst a = 1;\n'], ['b.ts', '"use strict";\n"\\uD83C\\uDF38";\nconst b = 2;\n']]);
add('surrogate-prologue-identity', [['a.ts', '"\\uD800";\n"�";\nconst a = 1;\n'], ['b.ts', '"\\ud800";\n"\\uFFFD";\nconst b = 2;\n']]);
add('same-line-prologue-comments', [['a.ts', '"use strict"; /* first */ const a = 1;\n'], ['b.ts', '"use strict"; /* second */ const b = 2;\n']]);
add('prologue-only-eof-comments', [['a.ts', '"one"; // a\n/* eof a */\n'], ['b.ts', '"one"; // b\n/* eof b */\n']]);
add('later-shebang', [['a.ts', 'const a = 1;\n'], ['b.ts', '#!/usr/bin/env node\nconst b = 2;\n']]);
add('first-shebang-wins', [['a.ts', '#!/first\nconst a = 1;\n'], ['b.ts', '#!/second\nconst b = 2;\n']]);
add('empty-source-comments', [['a.ts', '/* a */\n'], ['b.ts', '// b\n']]);
add('empty-sources', [['a.ts', ''], ['b.ts', '']]);
add('erased-sources', [['a.ts', 'interface A { value: number }\n'], ['b.ts', 'type B = string;\n']]);
add('comment-topology', [['a.ts', '/* file a */\n\n// leading\nconst a: number = 1; // tail\n/* eof a */\n'], ['b.ts', '/* file b */\n\nconst b: number = 2;\n/* eof b */\n']]);
add('always-strict', [['a.ts', '// first\nconst a = 1;\n'], ['b.ts', '// second\nconst b = 2;\n']], { alwaysStrict: true });
add('crlf', [['a.ts', '"one";\nconst a = 1; // a\n'], ['b.ts', '"two";\nconst b = 2; // b\n']], { newLine: ts.NewLineKind.CarriageReturnLineFeed });
add('remove-comments', [['a.ts', '/* first */\n"use strict";\nconst a = 1; // tail\n'], ['b.ts', '#!/second\n/* second */\n"use strict";\nconst b = 2;\n']], { removeComments: true });
const helpers = [['a.ts', 'const a = 1;\n'], ['b.ts', 'const b = {...value};\n'], ['c.ts', 'const {c, ...rest} = value;\n']];
for (const [name, module] of [['none', 0], ['commonjs', 1], ['amd', 2], ['system', 4]]) add('helper-placement-' + name, helpers, { target: ts.ScriptTarget.ES5, module });
add('shared-helper', [['a.ts', 'const a = {...value};\n'], ['b.ts', 'const b = {...value};\n']], { target: ts.ScriptTarget.ES5 });
add('no-emit-helpers', helpers, { target: ts.ScriptTarget.ES5, noEmitHelpers: true });
add('import-helpers-global', helpers, { target: ts.ScriptTarget.ES5, importHelpers: true });
add('javascript-input', [['a.js', '"one";\nconst a = 1;\n'], ['b.ts', '"two";\nconst b: number = 2;\n']], { allowJs: true });
add('bom-and-list', [['a.ts', 'const a = 1;\n'], ['b.ts', 'const b = 2;\n']], { emitBOM: true, listEmittedFiles: true });
function diagnostic(value) {
  return { code: value.code, category: ts.DiagnosticCategory[value.category], file: value.file?.fileName ?? null,
    start: value.start ?? null, length: value.length ?? null, message: ts.flattenDiagnosticMessageText(value.messageText, '\n'),
    related_information: value.relatedInformation?.map(diagnostic) ?? null };
}
function observation(input) {
  const files = new Map(input.files.map(file => [file.path, file.text]));
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name);
  const read = name => files.get(ts.normalizePath(name)) ?? (library(name)
    ? fs.readFileSync(path.join(root, 'vendor/typescript-6.0.3/lib', path.basename(name)), 'utf8') : undefined);
  const overlay = createHermeticDirectoryOverlay(input.files.map(file => file.path), { currentDirectory: input.current_directory,
    useCaseSensitiveFileNames: true, fallbackHost: { directoryExists: name => name === '/lib', getDirectories: () => [] } });
  const host = { ...ts.createCompilerHost(input.options, true), ...overlay, getCurrentDirectory: () => input.current_directory,
    useCaseSensitiveFileNames: () => true, getCanonicalFileName: ts.normalizePath,
    getDefaultLibFileName: () => '/lib/' + ts.getDefaultLibFileName(input.options), getDefaultLibLocation: () => '/lib',
    readFile: read, fileExists: name => read(name) !== undefined,
    getSourceFile(name, version) { const text = read(name); return text === undefined ? undefined : ts.createSourceFile(name, text, version, true, ts.getScriptKindFromFileName(name)); } };
  const program = ts.createProgram(input.roots ?? [...files.keys()], input.options, host);
  const writes = [];
  const result = program.emit(undefined, (name, text, bom, onError, sources, data) => {
    const callback = Buffer.from(text, 'utf8');
    const materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), callback]) : callback;
    const measured = ts.createTextWriter(input.options.newLine === ts.NewLineKind.CarriageReturnLineFeed ? '\r\n' : '\n');
    measured.rawWrite(text);
    writes.push({ index: writes.length, path: name, end: { line: measured.getLine(), character: measured.getColumn(), position: measured.getTextPos() }, callback_utf8_base64: callback.toString('base64'), callback_utf8_bytes: callback.length,
      write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString('base64'), materialized_utf8_bytes: materialized.length,
      on_error_callback_present: onError !== undefined, source_files: sources?.map(file => file.fileName) ?? null,
      data_present: data !== undefined, data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
      data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null });
  });
  const resolver = program.getTypeChecker().getEmitResolver();
  const collision_queries = [];
  const referenced_collision_queries = [];
  for (const file of program.getSourceFiles().filter(file => files.has(file.fileName))) {
    function walk(node) {
      if (ts.isVariableDeclaration(node) || ts.isBindingElement(node)) {
        const value = resolver.isDeclarationWithCollidingName(node);
        collision_queries.push({ path: file.fileName, kind: ts.SyntaxKind[node.kind], pos: node.pos, end: node.end,
          value_present: value !== undefined, value: value ?? null, truthy: !!value });
      }
      if (ts.isIdentifier(node)) {
        const value = resolver.getReferencedDeclarationWithCollidingName(node);
        referenced_collision_queries.push({ path: file.fileName, kind: ts.SyntaxKind[node.kind], pos: node.pos, end: node.end,
          value_present: value !== undefined, value: value ? { path: value.getSourceFile().fileName,
            kind: ts.SyntaxKind[value.kind], pos: value.pos, end: value.end } : null });
      }
      ts.forEachChild(node, walk);
    }
    walk(file);
  }
  return { source_files: program.getSourceFiles().filter(file => files.has(file.fileName)).map(file => file.fileName),
    common_source_directory: program.getCommonSourceDirectory(), collision_queries, referenced_collision_queries, writes,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null,
      source_maps: result.sourceMaps ?? null }, options_diagnostics: program.getOptionsDiagnostics().map(diagnostic),
    syntactic_diagnostics: program.getSyntacticDiagnostics().map(diagnostic), global_diagnostics: program.getGlobalDiagnostics().map(diagnostic),
    semantic_diagnostics: program.getSemanticDiagnostics().map(diagnostic) };
}
const cases = inputs.map(input => {
  const first = observation(input);
  assert.deepEqual(observation(input), first, input.case_id);
  assert.equal(first.writes.length, 1, input.case_id);
  return { ...input, typescript_observation: first };
});
const artifact = { version: 1, typescript: ts.version, source_commit: '050880ce59e30b356b686bd3144efe24f875ebc8',
  compiler_sha256: sha256(fs.readFileSync(path.join(root, 'vendor/typescript-6.0.3/lib/typescript.js'))), repetitions: 2,
  contract: 'Complete fresh Program.emit tuples repeat twice. Rust initially compares the internal global-script bundle printer bytes; runtime outFile admission stays closed.', cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(target, rendered);
else { assert.ok(process.argv[2] === undefined || process.argv[2] === '--check'); assert.equal(fs.readFileSync(target, 'utf8'), rendered); }
console.log(`bundle printer: ${cases.length} complete Program observations, each repeated twice`);
