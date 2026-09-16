// C01 / A40-LITERAL-UPDATE: fixed TypeScript 6.0.3 observations for literal value
// updates and their propagation into the printer and the tagged-template lowering.
//
//   node scripts/observe-literal-update.mjs <group> --list|--write|--check [destination]
//
// Groups:
//   factory   direct factory + standalone printer (5 literal kinds × origins × update operations)
//   transform direct transform routes: a `before` transformer updates parsed template fragments,
//             then the ordinary ES5 / ES2015 / ESNext script pipeline emits the file
//   pipeline  complete Program commands reaching the update owners from source input
//   lifetime  session lifetime controls (re-print, dispose)
//
// Every case is observed twice in-process and must agree; `--check` re-renders the
// artifact from a fresh process and requires byte identity with the stored file.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compiler = fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"));
assert.equal(sha256(compiler), "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
assert.equal(ts.version, "6.0.3");

const GROUPS = {
  factory: "crates/emitter/tests/fixtures/literal-update-factory.json",
  transform: "crates/emitter/tests/fixtures/literal-update-transform.json",
  pipeline: "crates/compiler/tests/fixtures/literal-update-pipeline.json",
  lifetime: "crates/emitter/tests/fixtures/literal-update-lifetime.json",
};
const group = process.argv[2];
const mode = process.argv[3];
assert.ok(Object.hasOwn(GROUPS, group), "unknown group");
assert.ok(["--list", "--write", "--check"].includes(mode), "mode must be --list, --write or --check");
const destination = path.resolve(root, process.argv[4] ?? GROUPS[group]);

const units = text => Array.from({ length: text.length }, (_, index) => text.charCodeAt(index));
const text = value => value === null || value === undefined ? undefined : String.fromCharCode(...value);
const spell = value => value.map(unit => "\\u" + unit.toString(16).toUpperCase().padStart(4, "0")).join("");
const label = (candidate, labels) => {
  for (const [name, node] of labels) if (node !== undefined && candidate === node) return name;
  return candidate === undefined ? null : "other";
};
const printed = (output) => {
  const bytes = Buffer.from(output);
  const starts = ts.computeLineStarts(output);
  return { text_utf16: units(output), utf8_base64: bytes.toString("base64"), utf8_bytes: bytes.length,
    end_utf16: { position: output.length, line: starts.length - 1, column: output.length - starts.at(-1) } };
};

// Value pairs (before → after) as UTF-16 code units. `lone-high` and `lone-low`
// change one unpaired surrogate into another (identical lossy UTF-8); `fffd`
// distinguishes a real U+FFFD from an unpaired surrogate.
const VALUES = {
  ascii: { before: units("x"), after: units("y") },
  bmp: { before: [0x00e9], after: [0x2028] },
  astral: { before: [0xd83d, 0xde00], after: [0xd83d, 0xde01] },
  "lone-high": { before: [0xd800], after: [0xd801] },
  "lone-low": { before: [0xdc00], after: [0xdc01] },
  fffd: { before: [0xfffd], after: [0xd800] },
  mixed: { before: [0x61, 0xd800, 0xdc00], after: [0x61, 0xdc00, 0xd800] },
};
const ALL_VALUES = Object.keys(VALUES);
const REDUCED_VALUES = ["ascii", "lone-high", "astral"];
const TEMPLATE_KINDS = ["NoSubstitutionTemplateLiteral", "TemplateHead", "TemplateMiddle", "TemplateTail"];
const STRING_KIND = "StringLiteral";
const CONTAINS_INVALID_ESCAPE = 2048;

function parsedSource(kind, spelling) {
  switch (kind) {
    case STRING_KIND: return `"${spelling}";`;
    case "NoSubstitutionTemplateLiteral": return `\`${spelling}\`;`;
    case "TemplateHead": return `\`${spelling}\${x}\`;`;
    case "TemplateMiddle": return `\`\${x}${spelling}\${y}\`;`;
    case "TemplateTail": return `\`\${x}${spelling}\`;`;
    default: throw new Error(kind);
  }
}
function findKind(sourceFile, kind) {
  let found;
  const walk = node => { if (!found && node.kind === ts.SyntaxKind[kind]) found = node; if (!found) node.forEachChild(walk); };
  sourceFile.forEachChild(walk);
  assert.ok(found, `${kind} not parsed`);
  return found;
}
// The factory's `update(updated, original)` helper (_tsc.js:24995-25001).
const update = (fresh, node) => ts.setOriginalNode(ts.setTextRange(fresh, node), node);

function factoryState(node, kind, labels) {
  const isString = kind === STRING_KIND;
  return { kind: node.kind, pos: node.pos, end: node.end, flags: node.flags, transform_flags: node.transformFlags,
    emit_flags: ts.getEmitFlags(node), parent: node.parent !== undefined,
    text_utf16: units(node.text), raw_text_utf16: isString ? null : node.rawText === undefined ? null : units(node.rawText),
    template_flags: isString ? null : node.templateFlags ?? 0,
    single_quote: isString ? node.singleQuote ?? null : null,
    has_extended_unicode_escape: isString ? node.hasExtendedUnicodeEscape ?? null : null,
    text_source: isString ? (node.textSourceNode ? "identifier" : null) : null,
    original: label(node.original, labels) };
}

// ---------------------------------------------------------------- factory group
function factoryInputs() {
  const rows = [];
  const push = (kind, origin, operation, value, policy) => rows.push({
    case_id: `literal-update/${kind}/${origin}/${operation}/${value}${policy === "ascii" ? "" : "-" + policy}`,
    group: "factory", kind, origin, operation, value, policy,
    before: VALUES[value].before, after: VALUES[value].after,
    source: origin === "parsed" || origin === "set-original" ? parsedSource(kind, spell(VALUES[value].before)) : null });
  for (const kind of TEMPLATE_KINDS) {
    const full = ["same", "cooked", "raw"], reduced = ["raw-absent", "raw-empty", "flags"];
    for (const origin of ["synthetic", "parsed", "clone", "set-original"]) {
      for (const operation of full) for (const value of ALL_VALUES) push(kind, origin, operation, value, "ascii");
      for (const operation of reduced) for (const value of REDUCED_VALUES) push(kind, origin, operation, value, "ascii");
    }
    for (const operation of ["same", "raw"]) for (const value of ALL_VALUES) push(kind, "synthetic-raw-absent", operation, value, "ascii");
    for (const value of REDUCED_VALUES) push(kind, "synthetic-raw-absent", "raw-empty", value, "ascii");
    for (const operation of ["same", "raw"]) for (const value of ALL_VALUES) push(kind, "synthetic-raw-empty", operation, value, "ascii");
    for (const value of REDUCED_VALUES) push(kind, "synthetic-raw-empty", "raw-absent", value, "ascii");
    for (const operation of full) for (const value of ALL_VALUES) push(kind, "cross-source", operation, value, "ascii");
    for (const origin of ["synthetic", "parsed"]) for (const operation of full) for (const value of ALL_VALUES) push(kind, origin, operation, value, "node-no-ascii");
  }
  for (const origin of ["synthetic", "parsed", "clone", "set-original"]) {
    for (const operation of ["same", "cooked"]) for (const value of ALL_VALUES) push(STRING_KIND, origin, operation, value, "ascii");
    for (const value of REDUCED_VALUES) push(STRING_KIND, origin, "quote", value, "ascii");
  }
  push(STRING_KIND, "text-source", "same", "ascii", "ascii");
  for (const value of ALL_VALUES) push(STRING_KIND, "text-source", "cooked", value, "ascii");
  push(STRING_KIND, "text-source", "quote", "ascii", "ascii");
  for (const operation of ["same", "cooked"]) for (const value of ALL_VALUES) push(STRING_KIND, "cross-source", operation, value, "ascii");
  for (const origin of ["synthetic", "parsed"]) for (const operation of ["same", "cooked"]) for (const value of ALL_VALUES) push(STRING_KIND, origin, operation, value, "node-no-ascii");
  return rows;
}

function observeFactory(input) {
  const { kind, origin, operation } = input;
  const isString = kind === STRING_KIND;
  const kindValue = ts.SyntaxKind[kind];
  const sourceFile = ts.createSourceFile("main.ts", input.source ?? "", ts.ScriptTarget.Latest, true);
  const before = text(input.before), after = text(input.after);
  const factory = ts.factory;
  const create = (value, raw, flags) => isString
    ? factory.createStringLiteral(value, false)
    : factory.createTemplateLiteralLikeNode(kindValue, value, raw, flags);
  let base, node;
  switch (origin) {
    case "synthetic": node = create(before, before, 0); break;
    case "synthetic-raw-absent": node = create(before, undefined, 0); break;
    case "synthetic-raw-empty": node = create(before, "", 0); break;
    case "parsed": node = findKind(sourceFile, kind); break;
    case "clone": base = create(before, before, 0); node = factory.cloneNode(base); break;
    case "set-original": base = findKind(sourceFile, kind); node = create(before, before, 0); ts.setOriginalNode(node, base); break;
    // The Rust arena clones the node into a second mounted source; TypeScript's
    // single node pool has only cloneNode. Compared against cloneNode semantics.
    case "cross-source": base = create(before, before, 0); node = factory.cloneNode(base); break;
    case "text-source": {
      base = factory.createIdentifier("abc");
      node = factory.createStringLiteralFromNode(base);
      break;
    }
    default: throw new Error(origin);
  }
  if (input.policy === "node-no-ascii") ts.setEmitFlags(node, ts.EmitFlags.NoAsciiEscaping);
  let updated;
  switch (operation) {
    case "same": updated = node; break;
    case "cooked": updated = update(isString ? factory.createStringLiteral(after, node.singleQuote)
      : factory.createTemplateLiteralLikeNode(kindValue, after, node.rawText, node.templateFlags), node); break;
    case "quote": updated = update(factory.createStringLiteral(node.text, !node.singleQuote), node); break;
    case "raw": updated = update(factory.createTemplateLiteralLikeNode(kindValue, node.text, after, node.templateFlags), node); break;
    case "raw-absent": updated = update(factory.createTemplateLiteralLikeNode(kindValue, node.text, undefined, node.templateFlags), node); break;
    case "raw-empty": updated = update(factory.createTemplateLiteralLikeNode(kindValue, node.text, "", node.templateFlags), node); break;
    case "flags": updated = update(factory.createTemplateLiteralLikeNode(kindValue, node.text, node.rawText,
      node.templateFlags ^ CONTAINS_INVALID_ESCAPE), node); break;
    default: throw new Error(operation);
  }
  const labels = [["base", base], ["node", node], ["updated", updated]];
  const printer = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed });
  const first = printer.printNode(ts.EmitHint.Unspecified, updated, sourceFile);
  const second = printer.printNode(ts.EmitHint.Unspecified, updated, sourceFile);
  assert.equal(first, second, `${input.case_id}: re-print drift`);
  return { node: factoryState(node, kind, labels), updated: factoryState(updated, kind, labels),
    identity: updated === node, printed: printed(first) };
}

// -------------------------------------------------------------- transform group
const TRANSFORM_VALUES = {
  ascii: { spelling: "\\u0078", before: units("x"), after: units("y") },
  "lone-high": { spelling: "\\uD800", before: [0xd800], after: [0xd801] },
  astral: { spelling: "\\uD83D\\uDE00", before: [0xd83d, 0xde00], after: [0xd83d, 0xde01] },
  // Real CRLF inside the template: the cooked text normalizes it, the raw keeps it.
  crlf: { spelling: "a\r\nb", before: units("a\nb"), after: units("c\r\nd") },
  // An invalid escape: the cooked value is the raw text and templateFlags carry 2048.
  invalid: { spelling: "\\unicode", before: units("\\unicode"), after: units("y") },
};
const SHAPES = {
  "tagged-spans": s => `tag\`${s}\${x}${s}\${y}${s}\`;`,
  "tagged-nosub": s => `tag\`${s}\`;`,
  "untagged-spans": s => `\`${s}\${x}${s}\${y}${s}\`;`,
  "untagged-nosub": s => `\`${s}\`;`,
};
const TARGETS = { es5: ts.ScriptTarget.ES5, es2015: ts.ScriptTarget.ES2015, esnext: ts.ScriptTarget.ESNext };
const transformSource = (shape, value, module) =>
  `declare function tag(...a: any[]): any;\ndeclare const x: any, y: any;\n${module === "module" ? "export {};\n" : ""}${SHAPES[shape](TRANSFORM_VALUES[value].spelling)}\n`;

function transformInputs() {
  const rows = [];
  const push = (shape, operation, value, target, module) => rows.push({
    case_id: `literal-update/transform/${shape}/${operation}/${value}/${target}${module === "module" ? "/module" : ""}`,
    group: "transform", shape, operation, value, target, module,
    before: TRANSFORM_VALUES[value].before, after: TRANSFORM_VALUES[value].after,
    source: transformSource(shape, value, module) });
  // `\unicode` is only admitted inside tagged templates (untagged: TS1125).
  const valuesFor = shape => Object.keys(TRANSFORM_VALUES).filter(value => value !== "invalid" || shape.startsWith("tagged"));
  for (const shape of Object.keys(SHAPES)) for (const operation of ["same", "cooked", "raw", "raw-absent", "raw-empty", "flags"])
    for (const value of valuesFor(shape)) for (const target of Object.keys(TARGETS)) push(shape, operation, value, target, "script");
  for (const shape of ["tagged-spans", "untagged-spans"]) for (const value of valuesFor(shape))
    for (const target of Object.keys(TARGETS)) push(shape, "children", value, target, "script");
  for (const shape of ["tagged-spans", "tagged-nosub"]) for (const operation of ["same", "cooked", "raw", "flags"])
    for (const value of ["lone-high", "invalid"]) for (const target of Object.keys(TARGETS)) push(shape, operation, value, target, "module");
  return rows;
}

function applyOperation(factory, node, operation, after) {
  const kind = node.kind, flags = node.templateFlags;
  switch (operation) {
    case "same": return node;
    case "children": return node;
    case "cooked": return update(factory.createTemplateLiteralLikeNode(kind, after, node.rawText, flags), node);
    case "raw": return update(factory.createTemplateLiteralLikeNode(kind, node.text, after, flags), node);
    case "raw-absent": return update(factory.createTemplateLiteralLikeNode(kind, node.text, undefined, flags), node);
    case "raw-empty": return update(factory.createTemplateLiteralLikeNode(kind, node.text, "", flags), node);
    case "flags": return update(factory.createTemplateLiteralLikeNode(kind, node.text, node.rawText, flags ^ CONTAINS_INVALID_ESCAPE), node);
    default: throw new Error(operation);
  }
}
function beforeTransformer(operation, after) {
  return context => sourceFile => {
    const factory = context.factory;
    const visit = node => {
      if (ts.isTemplateLiteralToken(node)) return applyOperation(factory, node, operation, after);
      if (operation === "children" && ts.isTemplateSpan(node)) return factory.updateTemplateSpan(node, factory.createIdentifier("z"), node.literal);
      return ts.visitEachChild(node, visit, context);
    };
    return ts.visitEachChild(sourceFile, visit, context);
  };
}
function makeHost(files, options, sensitive = true) {
  const canonical = ts.createGetCanonicalFileName(sensitive);
  const map = new Map([...files].map(([name, content]) => [canonical(name), content]));
  const libraryRoot = path.join(root, "vendor/typescript-6.0.3/lib");
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name) && fs.existsSync(path.join(libraryRoot, path.basename(name)));
  const read = name => map.get(canonical(ts.normalizePath(name))) ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(map.keys(), { currentDirectory: "/project", useCaseSensitiveFileNames: sensitive,
    fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  return { ...ts.createCompilerHost(options, true), ...overlay,
    getCurrentDirectory: () => "/project", getDefaultLibFileName: options => "/lib/" + ts.getDefaultLibFileName(options),
    getDefaultLibLocation: () => "/lib", useCaseSensitiveFileNames: () => sensitive, getCanonicalFileName: canonical,
    readFile: read, fileExists: name => map.has(canonical(ts.normalizePath(name))) || library(name),
    getSourceFile: (name, languageVersion) => { const content = read(name); return content === undefined ? undefined : ts.createSourceFile(name, content, languageVersion, true); },
    writeFile: () => assert.fail("unexpected host write") };
}
// One checked Program per (source, target); every case still emits through its
// own transformNodes call, twice.
const programs = new Map();
function transformProgram(input) {
  // TypeScript 6.0 reports TS5107 for target ES5 unless deprecations are
  // acknowledged; the acknowledgement does not change emit.
  // module Preserve: the port selects its script transformers without an
  // emit host for this kind; the ESM shapes here do not depend on it.
  const options = { strict: true, target: TARGETS[input.target], module: ts.ModuleKind.Preserve, newLine: ts.NewLineKind.LineFeed,
    outDir: "/project/out", skipDefaultLibCheck: true, sourceMap: false, declaration: false, removeComments: false,
    ignoreDeprecations: "6.0" };
  const key = JSON.stringify([input.source, options]);
  if (!programs.has(key)) {
    const host = makeHost([["/project/main.ts", input.source]], options);
    const program = ts.createProgram({ rootNames: ["/project/main.ts"], options, host });
    const diagnostics = ts.getPreEmitDiagnostics(program).map(d => ({ code: d.code, message: ts.flattenDiagnosticMessageText(d.messageText, "\n") }));
    assert.deepEqual(diagnostics, [], input.case_id);
    programs.set(key, program);
  }
  return programs.get(key);
}
function observeTransform(input) {
  const program = transformProgram(input);
  const writes = [];
  const result = program.emit(undefined, (name, content) => writes.push({ path: name, content }), undefined, false,
    { before: [beforeTransformer(input.operation, text(input.after))] });
  assert.equal(result.emitSkipped, false);
  const js = writes.find(w => w.path === "/project/out/main.js");
  assert.ok(js, "main.js written");
  assert.equal(writes.length, 1);
  return { js: printed(js.content) };
}

// --------------------------------------------------------------- pipeline group
const PIPELINE_BASE = { strict: true, module: ts.ModuleKind.ESNext, newLine: ts.NewLineKind.CarriageReturnLineFeed,
  declaration: true, declarationMap: true, sourceMap: true, skipDefaultLibCheck: true, noErrorTruncation: true, outDir: "/project/out" };
const TAG = "export declare function t(s: TemplateStringsArray, ...v: any[]): string;\n";
const AB = "declare const a: string | undefined, b: string;\n";
const PIPELINE_SOURCES = {
  "untagged-nullish": `${AB}export const s = \`\\uD800\${a ?? b}\\uD800\`;\n`,
  "tagged-nullish": `${TAG}${AB}export const s = t\`\\uD800\${a ?? b}\\uD800\`;\n`,
  "tagged-invalid-nullish": `${TAG}${AB}export const s = t\`\\unicode\${a ?? b}\\uD800\`;\n`,
  "tagged-nested-nullish": `${TAG}${AB}export const s = t\`\\uD800\${t\`\\uD83D\\uDE00\${a ?? b}\\uDC00\`}\\uD800\`;\n`,
  "tagged-crlf-nullish": `${TAG}${AB}export const s = t\`a\r\nb\${a ?? b}c\r\n\\uD800\`;\r\n`,
  "untagged-crlf-nullish": `${AB}export const s = \`a\r\nb\${a ?? b}c\r\n\\uD800\`;\r\n`,
};
const REWRITE_FILES = [
  ["/project/main.ts", "import { a } from './x.ts';\nimport { b } from \"./y\\u{79}.ts\";\nexport * from \"./z.ts\";\nexport const c = a + b;\nexport const d = import(\"./w.ts\");\n"],
  ["/project/x.ts", "export const a = 1;\n"],
  ["/project/yy.ts", "export const b = 2;\n"],
  ["/project/z.ts", "export const z = 3;\n"],
  ["/project/w.ts", "export const w = 4;\n"],
];
function pipelineInputs() {
  const rows = [];
  for (const [name, source] of Object.entries(PIPELINE_SOURCES)) for (const target of Object.keys(TARGETS)) rows.push({
    case_id: `literal-update/pipeline/children/${target}/${name}`, group: "pipeline", route: "children",
    files: [{ path: "/project/main.ts", text: source }], roots: ["/project/main.ts"],
    options: { ...PIPELINE_BASE, target: TARGETS[target] } });
  for (const [moduleName, module] of [["esnext", ts.ModuleKind.ESNext], ["commonjs", ts.ModuleKind.CommonJS]])
    for (const target of ["es5", "es2015"]) rows.push({
      case_id: `literal-update/pipeline/rewrite/${target}/${moduleName}`, group: "pipeline", route: "rewrite",
      files: REWRITE_FILES.map(([path, text]) => ({ path, text })), roots: ["/project/main.ts"],
      options: { ...PIPELINE_BASE, target: TARGETS[target], module, rewriteRelativeImportExtensions: true } });
  return rows;
}
const value = content => ({ utf16: units(content), utf8_base64: Buffer.from(content).toString("base64") });
const diagnostic = d => ({ code: d.code, category: d.category, file: d.file?.fileName ?? null,
  start: d.start ?? null, length: d.length ?? null,
  message: value(ts.flattenDiagnosticMessageText(d.messageText, "\n")),
  related_information: d.relatedInformation?.map(diagnostic) ?? null });
function observePipeline(input) {
  const host = makeHost(input.files.map(file => [file.path, file.text]), input.options);
  const program = ts.createProgram(input.roots, input.options, host);
  const writes = [], diagnostics = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program,
    d => diagnostics.push(diagnostic(d)), content => status.push(value(content)), undefined,
    (name, content, bom, onError, sources, data) => writes.push({
      index: writes.length, path: name, callback: value(content), write_byte_order_mark: bom,
      materialized_utf8_base64: Buffer.from((bom ? "﻿" : "") + content).toString("base64"),
      on_error_callback_present: onError !== undefined,
      source_files: sources?.map(s => s.fileName) ?? null,
      data_present: data !== undefined, data_keys: data === undefined ? null : Object.keys(data),
      data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
      data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null }));
  assert.ok(result);
  return { writes, reported_diagnostics: diagnostics, status_writes: status, exit_code: exit,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps?.map(entry => ({
        input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap) })) ?? null } };
}

// --------------------------------------------------------------- lifetime group
function lifetimeInputs() {
  const rows = [];
  for (const kind of [STRING_KIND, ...TEMPLATE_KINDS]) for (const origin of ["parsed", "synthetic"]) rows.push({
    case_id: `literal-update/lifetime/${kind}/${origin}/dispose/lone-high-no-ascii`, group: "lifetime", kind, origin,
    value: [0xd800], source: origin === "parsed" ? parsedSource(kind, "\\uD800") : null });
  return rows;
}
function observeLifetime(input) {
  const isString = input.kind === STRING_KIND;
  const sourceFile = ts.createSourceFile("main.ts", input.source ?? "", ts.ScriptTarget.Latest, true);
  const node = input.origin === "parsed" ? findKind(sourceFile, input.kind)
    : isString ? ts.factory.createStringLiteral(text(input.value), false)
    : ts.factory.createTemplateLiteralLikeNode(ts.SyntaxKind[input.kind], text(input.value), text(input.value), 0);
  ts.setEmitFlags(node, ts.EmitFlags.NoAsciiEscaping);
  const printer = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed });
  const before = printer.printNode(ts.EmitHint.Unspecified, node, sourceFile);
  const again = printer.printNode(ts.EmitHint.Unspecified, node, sourceFile);
  assert.equal(before, again);
  const result = ts.transform(sourceFile, []);
  result.dispose();
  const after = printer.printNode(ts.EmitHint.Unspecified, node, sourceFile);
  return { emit_flags_before_dispose: ts.EmitFlags.NoAsciiEscaping, emit_flags_after_dispose: ts.getEmitFlags(node),
    text_utf16: units(node.text), raw_text_utf16: isString ? null : node.rawText === undefined ? null : units(node.rawText),
    single_quote: isString ? node.singleQuote ?? null : null,
    printed_before_dispose: printed(before), printed_after_dispose: printed(after) };
}

// ------------------------------------------------------------------- driver
const DRIVERS = {
  factory: [factoryInputs, observeFactory, "direct-factory-and-printer"],
  transform: [transformInputs, observeTransform, "direct-transform-custom-before"],
  pipeline: [pipelineInputs, observePipeline, "complete-program-command"],
  lifetime: [lifetimeInputs, observeLifetime, "direct-session-lifetime"],
};
const [inputs, observe, route] = DRIVERS[group];
const rows = inputs();
assert.equal(new Set(rows.map(row => row.case_id)).size, rows.length, "duplicate case ids");
if (mode === "--list") {
  console.log(JSON.stringify({ group, route, cases: rows.length, ids: rows.map(row => row.case_id) }, null, 2));
  process.exit(0);
}
const cases = rows.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  return { ...input, typescript_observation: first };
});
const artifact = { version: 1, typescript: ts.version, group, route, repetitions: 2,
  compiler_sha256: sha256(compiler), observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (mode === "--write") fs.writeFileSync(destination, rendered, { flag: "wx" });
else assert.equal(fs.readFileSync(destination, "utf8"), rendered, `${group}: stored observations differ`);
console.log(JSON.stringify({ group, output: path.relative(root, destination), cases: cases.length, repetitions: 2, sha256: sha256(rendered) }));
