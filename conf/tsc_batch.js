const fs = require('fs');
const path = require('path');

function loadTypescript() {
  const root = path.resolve(__dirname, '..');
  const oracle = process.env.TSRS_ORACLE || path.join(root, 'oracle');
  const candidates = [
    process.env.TSRS_TYPESCRIPT,
    path.join(oracle, 'node_modules', 'typescript'),
    'typescript',
  ].filter(Boolean);
  for (const candidate of candidates) {
    try { return require(candidate); } catch (_) {}
  }
  console.error('tsc_batch.js: missing TypeScript module; set TSRS_TYPESCRIPT or TSRS_ORACLE');
  process.exit(2);
}

const ts = loadTypescript();
const ROOT = process.env.TSRS_ROOT || path.resolve(__dirname, '..');
const LIB = process.env.TSRS_LIB || path.join(ROOT, 'lib', 'lib.tsrs.d.ts');
const libText = fs.readFileSync(LIB, 'utf8');
const libSF = ts.createSourceFile('lib.d.ts', libText, ts.ScriptTarget.ES2020, true);
const cases = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const opts = { noEmit: true, noLib: true, strict: true, target: ts.ScriptTarget.ES2020 };
const out = {};
for (const c of cases) {
  const sf = ts.createSourceFile('main.ts', c.src, ts.ScriptTarget.ES2020, true);
  const host = {
    getSourceFile: (n) => n === 'lib.d.ts' ? libSF : n === 'main.ts' ? sf : undefined,
    getDefaultLibFileName: () => 'lib.d.ts',
    writeFile: () => {}, getCurrentDirectory: () => '/', getDirectories: () => [],
    fileExists: (n) => n === 'lib.d.ts' || n === 'main.ts',
    readFile: (n) => n === 'lib.d.ts' ? libText : n === 'main.ts' ? c.src : undefined,
    getCanonicalFileName: (n) => n, useCaseSensitiveFileNames: () => true, getNewLine: () => '\n',
  };
  const prog = ts.createProgram(['lib.d.ts', 'main.ts'], opts, host);
  let ds = ts.getPreEmitDiagnostics(prog).filter(d => d.file && d.file.fileName === 'main.ts');
  let sg = [];
  try { sg = prog.getSuggestionDiagnostics(sf); } catch (e) {}
  out[c.name] = [...ds, ...sg].map(d => [d.code, d.start, d.category]);
}
const output = JSON.stringify(out);
if (process.argv[3] === '-') {
  process.stdout.write(output);
} else {
  fs.writeFileSync(process.argv[3], output);
}
