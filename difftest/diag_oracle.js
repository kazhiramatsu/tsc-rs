// Phase 2 diagnostic oracle (comprehensive).
//
// Dumps EVERY diagnostic tsc currently produces for a program, with the full
// field set, so tsrs's checker/emit output can be diffed against the exact tsc
// shape. Runs with noEmit:false (emit + declaration emit enabled) so that
// emit-related diagnostics are captured too; emitted file contents are
// discarded (only the diagnostics and the would-be-emitted file names kept).
//
// Sources gathered (the union tsc reports under a normal compile):
//   getOptionsDiagnostics, getGlobalDiagnostics, getConfigFileParsingDiagnostics,
//   per-file getSyntacticDiagnostics / getSemanticDiagnostics /
//   getDeclarationDiagnostics, program.emit().diagnostics, and per-file
//   getSuggestionDiagnostics (category 2).
//
// Per diagnostic the complete tsc `Diagnostic` surface is serialized:
//   code, category, source, reportsUnnecessary, reportsDeprecated,
//   file, start, length, full span ({line,col} for both ends),
//   messageText (recursive DiagnosticMessageChain), relatedInformation.
//
// Usage:
//   node diag_oracle.js <main.ts> [otherFile.ts ...] [--all-files]
//   node diag_oracle.js --options-json opts.json --all-files <root.ts> ...
//   Mirrors difftest/cmp.sh (--noLib --strict). files[0] scopes which
//   diagnostics are reported unless --all-files is given.
//   Emits: { emittedFiles, emitSkipped, diagnostics: [...] }.
const fs = require("fs");
const path = require("path");

function loadTypescript() {
  const root = path.resolve(__dirname, "..");
  const oracle = process.env.TSRS_ORACLE || path.join(root, "oracle");
  const candidates = [
    process.env.TSRS_TYPESCRIPT,
    path.join(oracle, "node_modules", "typescript"),
    "typescript",
  ].filter(Boolean);
  for (const candidate of candidates) {
    try { return require(candidate); } catch (_) {}
  }
  console.error("diag_oracle.js: missing TypeScript module; set TSRS_TYPESCRIPT or TSRS_ORACLE");
  process.exit(2);
}

const ts = loadTypescript();

let allFiles = false;
let optionsJson = null;
const files = [];
for (let i = 2; i < process.argv.length; i++) {
  const arg = process.argv[i];
  if (arg === "--all-files") {
    allFiles = true;
  } else if (arg === "--options-json") {
    optionsJson = process.argv[++i];
  } else if (arg.startsWith("--")) {
    console.error(`diag_oracle.js: unknown option ${arg}`);
    process.exit(2);
  } else {
    files.push(path.resolve(arg));
  }
}
if (files.length === 0) {
  console.error("diag_oracle.js: expected at least one input file");
  process.exit(2);
}
const mainBase = path.basename(files[0]);

function enumValue(map, raw, fallback) {
  if (raw === null || raw === undefined) return fallback;
  if (typeof raw === "number") return raw;
  const key = String(raw).toLowerCase();
  if (!Object.prototype.hasOwnProperty.call(map, key)) return fallback;
  return map[key] === undefined ? fallback : map[key];
}

function coerceCompilerOptions(raw) {
  const target = {
    es3: ts.ScriptTarget.Latest,
    es5: ts.ScriptTarget.ES5,
    es6: ts.ScriptTarget.ES2015,
    es2015: ts.ScriptTarget.ES2015,
    es2016: ts.ScriptTarget.ES2016,
    es2017: ts.ScriptTarget.ES2017,
    es2018: ts.ScriptTarget.ES2018,
    es2019: ts.ScriptTarget.ES2019,
    es2020: ts.ScriptTarget.ES2020,
    es2021: ts.ScriptTarget.ES2021,
    es2022: ts.ScriptTarget.ES2022,
    es2023: ts.ScriptTarget.ES2023,
    es2024: ts.ScriptTarget.ES2024,
    es2025: ts.ScriptTarget.ES2025,
    esnext: ts.ScriptTarget.ESNext,
    latest: ts.ScriptTarget.Latest,
  };
  const moduleKind = {
    none: ts.ModuleKind.None,
    commonjs: ts.ModuleKind.CommonJS,
    amd: ts.ModuleKind.AMD,
    umd: ts.ModuleKind.UMD,
    system: ts.ModuleKind.System,
    es6: ts.ModuleKind.ES2015,
    es2015: ts.ModuleKind.ES2015,
    es2020: ts.ModuleKind.ES2020,
    es2022: ts.ModuleKind.ES2022,
    esnext: ts.ModuleKind.ESNext,
    node16: ts.ModuleKind.Node16,
    node18: ts.ModuleKind.Node18,
    node20: ts.ModuleKind.Node20,
    nodenext: ts.ModuleKind.NodeNext,
    preserve: ts.ModuleKind.Preserve,
  };
  const moduleResolution = {
    classic: ts.ModuleResolutionKind.Classic,
    node: ts.ModuleResolutionKind.Node10 || ts.ModuleResolutionKind.NodeJs,
    node10: ts.ModuleResolutionKind.Node10 || ts.ModuleResolutionKind.NodeJs,
    node16: ts.ModuleResolutionKind.Node16,
    nodenext: ts.ModuleResolutionKind.NodeNext,
    bundler: ts.ModuleResolutionKind.Bundler,
  };
  const jsx = {
    preserve: ts.JsxEmit.Preserve,
    react: ts.JsxEmit.React,
    "react-native": ts.JsxEmit.ReactNative,
    "react-jsx": ts.JsxEmit.ReactJSX,
    "react-jsxdev": ts.JsxEmit.ReactJSXDev,
  };
  const importsNotUsedAsValues = {
    remove: ts.ImportsNotUsedAsValues.Remove,
    preserve: ts.ImportsNotUsedAsValues.Preserve,
    error: ts.ImportsNotUsedAsValues.Error,
  };
  const out = { ...raw };
  if ("target" in out) out.target = enumValue(target, out.target, ts.ScriptTarget.Latest);
  if ("module" in out) {
    if (out.module === null) delete out.module;
    else out.module = enumValue(moduleKind, out.module, ts.ModuleKind.ESNext);
  }
  if ("moduleResolution" in out) {
    out.moduleResolution = enumValue(moduleResolution, out.moduleResolution, ts.ModuleResolutionKind.Bundler);
  }
  if ("jsx" in out) out.jsx = enumValue(jsx, out.jsx, ts.JsxEmit.Preserve);
  if ("importsNotUsedAsValues" in out) {
    out.importsNotUsedAsValues = enumValue(
      importsNotUsedAsValues,
      out.importsNotUsedAsValues,
      ts.ImportsNotUsedAsValues.Remove,
    );
  }
  return out;
}

let options = {
  noEmit: false,
  noLib: true,
  strict: true,
  declaration: true,
  target: ts.ScriptTarget.Latest,
  module: ts.ModuleKind.ESNext,
  moduleResolution: ts.ModuleResolutionKind.Bundler,
};
if (optionsJson) {
  options = coerceCompilerOptions(JSON.parse(fs.readFileSync(optionsJson, "utf8")));
}

const program = ts.createProgram(files, options);

const diagSet = [];
const seen = new Set();
function add(list) {
  for (const d of list || []) {
    const key =
      d.code + "|" + (d.file ? d.file.fileName : "") + "|" + d.start + "|" +
      d.length + "|" +
      (typeof d.messageText === "string" ? d.messageText : d.messageText.messageText);
    if (seen.has(key)) continue;
    seen.add(key);
    diagSet.push(d);
  }
}

add(program.getConfigFileParsingDiagnostics && program.getConfigFileParsingDiagnostics());
add(program.getOptionsDiagnostics());
add(program.getGlobalDiagnostics());
for (const sf of program.getSourceFiles()) {
  add(program.getSyntacticDiagnostics(sf));
  add(program.getSemanticDiagnostics(sf));
  add(program.getDeclarationDiagnostics(sf));
}

let emittedFiles = [];
let emitSkipped = false;
try {
  const writeDiscard = (fileName) => { emittedFiles.push(path.basename(fileName)); };
  const emitResult = program.emit(undefined, writeDiscard);
  emitSkipped = !!emitResult.emitSkipped;
  add(emitResult.diagnostics);
} catch (e) {}

for (const sf of program.getSourceFiles()) {
  try { add(program.getSuggestionDiagnostics(sf)); } catch (e) {}
}

function chain(mt) {
  if (typeof mt === "string") return { text: mt };
  return { text: mt.messageText, code: mt.code, category: mt.category, next: (mt.next || []).map(chain) };
}

function span(d) {
  if (d.file && typeof d.start === "number") {
    const s = d.file.getLineAndCharacterOfPosition(d.start);
    const e = d.file.getLineAndCharacterOfPosition(d.start + (d.length || 0));
    // tsc positions are UTF-16 (JS string indices). Provide the UTF-8 byte
    // offsets too (Buffer.byteLength of the UTF-16 prefix) for native consumers.
    const txt = d.file.text;
    const byteStart = Buffer.byteLength(txt.slice(0, d.start), "utf8");
    const byteEnd = Buffer.byteLength(txt.slice(0, d.start + (d.length || 0)), "utf8");
    return {
      file: path.basename(d.file.fileName),
      start: d.start, length: d.length,
      byteStart, byteLength: byteEnd - byteStart,
      startLine: s.line + 1, startCol: s.character + 1,
      endLine: e.line + 1, endCol: e.character + 1,
    };
  }
  return { file: null, start: null, length: null, byteStart: null, byteLength: null, startLine: null, startCol: null, endLine: null, endCol: null };
}

function related(r) {
  return { code: r.code, category: r.category, ...span(r), message: chain(r.messageText) };
}

function serialize(d) {
  return {
    code: d.code,
    category: d.category,
    source: d.source || null,
    reportsUnnecessary: d.reportsUnnecessary ? true : false,
    reportsDeprecated: d.reportsDeprecated ? true : false,
    ...span(d),
    message: chain(d.messageText),
    related: (d.relatedInformation || []).map(related),
  };
}

const reported = diagSet
  .filter((d) => allFiles || !d.file || path.basename(d.file.fileName) === mainBase)
  .map(serialize)
  .sort((a, b) => (a.start || 0) - (b.start || 0) || a.code - b.code || a.category - b.category);

process.stdout.write(JSON.stringify({ emittedFiles, emitSkipped, diagnostics: reported }, null, 2) + "\n");
