// H2.7d's internal chainBundle/source-helper contract. This does not admit
// outFile execution: complete Program tuples remain in bundle-plan.json.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import ts from '../vendor/typescript-6.0.3/lib/typescript.js';

const root = path.resolve(import.meta.dirname, '..');
const target = path.join(root, 'crates/emitter/tests/fixtures/bundle-transform.json');
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
assert.equal(ts.version, '6.0.3');
assert.equal(process.versions.node, fs.readFileSync(path.join(root, '.node-version'), 'utf8').trim());
const inputs = [
  ['bundle', [['a.ts', 'b.ts']], false],
  ['sources', ['a.ts', 'b.ts'], false],
  ['empty-bundle', [[]], false],
  ['empty-roots', [], false],
  ['bundle-declarations', [['a.ts', 'types.d.ts', 'b.ts']], false],
  ['bundle-declarations-allowed', [['a.ts', 'types.d.ts', 'b.ts']], true],
  ['source-declarations', ['a.ts', 'types.d.ts', 'b.ts'], false],
  ['source-declarations-allowed', ['a.ts', 'types.d.ts', 'b.ts'], true],
  ['mixed-roots', [['a.ts', 'b.ts'], 'c.ts', ['d.ts']], false],
].map(([case_id, roots, allow_declaration_files]) => ({ case_id, roots, allow_declaration_files }));
for (const replace_phase of ['first', 'second']) inputs.push({ case_id: 'replacement-source-' + replace_phase, roots: [['a.ts', 'b.ts']], allow_declaration_files: false, replace_source: 'a.ts', replace_phase });
function source(name, text = 'const value: number = 1;\n') {
  return ts.createSourceFile(name, name.endsWith('.d.ts') ? 'declare const value: number;\n' : text, ts.ScriptTarget.ESNext, true);
}
function helper(name, dependencies = []) { return { name, scoped: false, text: `var ${name.replaceAll(/[^a-z0-9]/gi, '_')} = 0;`, dependencies }; }
function helperNames(node) { return ts.getEmitHelpers(node)?.map(helper => helper.name) ?? []; }
function observe(input) {
  const log = [];
  const roots = input.roots.map(node => Array.isArray(node) ? ts.factory.createBundle(node.map(name => source(name))) : source(node));
  // Built-in helpers are shared singleton records, including dependencies.
  const shared = helper('shared', [helper('dependency')]);
  const factories = ['first', 'second'].map(label => context => {
    log.push(`${label}:initialize`);
    return ts.chainBundle(context, file => {
      log.push(`${label}:${file.fileName}:${helperNames(file).join(',')}`);
      context.requestEmitHelper(shared);
      context.requestEmitHelper(helper(`${label}:${file.fileName}`));
      const output = label === input.replace_phase && file.fileName === input.replace_source ? ts.factory.cloneNode(file) : file;
      if (output !== file) output.fileName = 'replacement.ts';
      ts.addEmitHelpers(output, context.readEmitHelpers());
      return output;
    });
  });
  const result = ts.transformNodes(undefined, undefined, ts.factory, {}, roots, factories, input.allow_declaration_files);
  const projection = file => ({ path: file.fileName, helpers: helperNames(file), is_declaration_file: file.isDeclarationFile });
  const transformed = result.transformed.map(node => ts.isBundle(node)
    ? { kind: 'bundle', sources: node.sourceFiles.map(projection) }
    : { kind: 'source-file', sources: [projection(node)] });
  const observation = { log, roots: transformed, diagnostics: result.diagnostics };
  result.dispose();
  return observation;
}
const builtins = [
  { case_id: 'assign-separated', files: [['a.ts', 'const a = {...value};\n'], ['b.ts', 'const b = 1;\n']] },
  { case_id: 'assign-shared', files: [['a.ts', 'const a = {...value};\n'], ['b.ts', 'const b = {...value};\n']] },
  { case_id: 'rest-separated', files: [['a.ts', 'const {a, ...rest} = value;\n'], ['b.ts', 'const b = 1;\n']] },
  { case_id: 'mixed-helpers', files: [['a.ts', 'const a = {...value};\n'], ['b.ts', 'const {b, ...rest} = value;\n']] },
].map(input => ({ ...input, options: { target: ts.ScriptTarget.ES5, module: ts.ModuleKind.None, outFile: '/bundle.js', alwaysStrict: false, strict: false } }));
function collisionQueries(input) {
  const files = new Map(input.files);
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name);
  const read = name => files.get(name) ?? files.get(name.replace(/^\//, '')) ?? (library(name)
    ? fs.readFileSync(path.join(root, 'vendor/typescript-6.0.3/lib', path.basename(name)), 'utf8') : undefined);
  const host = { ...ts.createCompilerHost(input.options, true), getCurrentDirectory: () => '/',
    getDefaultLibFileName: () => '/lib/' + ts.getDefaultLibFileName(input.options), getDefaultLibLocation: () => '/lib',
    getDirectories: () => [], directoryExists: name => name === '/' || name === '/lib',
    fileExists: name => read(name) !== undefined, readFile: read,
    getSourceFile(name, version) { const text = read(name); return text === undefined ? undefined : ts.createSourceFile(name, text, version, true); } };
  const program = ts.createProgram([...files.keys()], input.options, host);
  const resolver = program.getTypeChecker().getEmitResolver();
  const queries = [];
  for (const file of program.getSourceFiles().filter(file => files.has(file.fileName))) {
    function walk(node) {
      if (ts.isVariableDeclaration(node) || ts.isBindingElement(node)) {
        const result = resolver.isDeclarationWithCollidingName(node);
        queries.push({ path: file.fileName, kind: ts.SyntaxKind[node.kind], pos: node.pos, end: node.end,
          value_present: result !== undefined, value: result ?? null, truthy: !!result });
      }
      ts.forEachChild(node, walk);
    }
    walk(file);
  }
  assert.ok(queries.length > 0);
  return queries;
}
function observeBuiltins(input) {
  const files = input.files.map(([name, text]) => source(name, text));
  const resolver = new Proxy({}, { get: (_, name) => () => { throw Error(`unexpected resolver query: ${String(name)}`); } });
  const result = ts.transformNodes(resolver, { getCompilerOptions: () => input.options }, ts.factory, input.options,
    [ts.factory.createBundle(files)], ts.getTransformers(input.options).scriptTransformers, false);
  const bundle = result.transformed[0];
  const observation = { roots: bundle.sourceFiles.map(file => ({ path: file.fileName, helpers: helperNames(file) })),
    collision_queries: collisionQueries(input),
    diagnostics: result.diagnostics,
    printed_bundle_utf8_base64: Buffer.from(ts.createPrinter({ ...input.options, newLine: ts.NewLineKind.LineFeed }).printBundle(bundle)).toString('base64') };
  result.dispose();
  return observation;
}
const cases = inputs.map(input => {
  const observation = observe(input);
  assert.deepEqual(observe(input), observation, input.case_id);
  return { ...input, observation };
});
const builtin_cases = builtins.map(input => {
  const observation = observeBuiltins(input);
  assert.deepEqual(observeBuiltins(input), observation, input.case_id);
  return { ...input, observation };
});
const artifact = { version: 1, typescript: ts.version, source_commit: '050880ce59e30b356b686bd3144efe24f875ebc8',
  compiler_sha256: sha256(fs.readFileSync(path.join(root, 'vendor/typescript-6.0.3/lib/typescript.js'))), repetitions: 2,
  program_observations: { path: 'crates/emitter/tests/fixtures/bundle-plan.json', sha256: sha256(fs.readFileSync(path.join(root, 'crates/emitter/tests/fixtures/bundle-plan.json'))) },
  contract: 'Rust compares internal transform order, declaration-root gating, retained roots and source-specific helpers. Printed bundle bytes are future printer references; Program outFile stays refused.',
  cases, builtin_cases };
const rendered = JSON.stringify(artifact, null, 2) + '\n';
if (process.argv[2] === '--write') fs.writeFileSync(target, rendered);
else { assert.ok(process.argv[2] === undefined || process.argv[2] === '--check'); assert.equal(fs.readFileSync(target, 'utf8'), rendered); }
console.log(`bundle transform: ${cases.length} traces and ${builtin_cases.length} built-in helper observations, each repeated twice`);
