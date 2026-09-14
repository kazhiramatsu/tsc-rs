// H2.8a G5c controls: JSDoc return annotation ownership through complete
// TypeScript commands. Each row is one whole createProgram +
// emitFilesAndReportErrorsAndGetExitStatus observation from the pinned
// compiler, run twice; the callback value is kept as UTF-16 units so an
// unpaired unit is never projected to U+FFFD. Every function-like node also
// records the upstream JSDoc return resolution (owned tags, getJSDocReturnType,
// getEffectiveReturnTypeNode, the single return candidate) through the public
// API so the native syntactic lookup can be compared node for node.
// The native outcome is recorded separately (crates/compiler/tests/h2_8a_jsdoc_return.rs).
// node scripts/observe-h2-8a-jsdoc-return.mjs --write|--check
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const compilerSha = sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js")));
assert.equal(ts.version, "6.0.3");
assert.equal(compilerSha, "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39");
const units = text => Array.from({length: text.length}, (_, index) => text.charCodeAt(index));
const value = text => ({utf16: units(text), utf8_base64: Buffer.from(text).toString("base64")});
const diagnostic = d => ({code: d.code, category: d.category, file: d.file?.fileName ?? null,
  start: d.start ?? null, length: d.length ?? null,
  message: value(ts.flattenDiagnosticMessageText(d.messageText, "\n")),
  related_information: d.relatedInformation?.map(diagnostic) ?? null});
const FUNCTION_LIKE = new Set([ts.SyntaxKind.FunctionDeclaration, ts.SyntaxKind.FunctionExpression,
  ts.SyntaxKind.ArrowFunction, ts.SyntaxKind.MethodDeclaration, ts.SyntaxKind.GetAccessor,
  ts.SyntaxKind.SetAccessor, ts.SyntaxKind.Constructor]);
// ts.SyntaxKind[value] returns the last alias declared for a value (ImportType
// prints as LastTypeNode); the canonical kind name is the first non-marker key.
const KIND_NAMES = new Map();
for (const [name, value] of Object.entries(ts.SyntaxKind)) {
  if (typeof value === "number" && !/^(First|Last)/.test(name) && !KIND_NAMES.has(value)) KIND_NAMES.set(value, name);
}
const kindName = kind => KIND_NAMES.get(kind) ?? ts.SyntaxKind[kind];
function identity(node, file) {
  return node ? {kind: kindName(node.kind), pos: node.pos, end: node.end, file} : null;
}
// Upstream resolution of every function-like node, recorded through the public
// API (getJSDocTags / getJSDocReturnType / getEffectiveReturnTypeNode). The
// single return candidate mirrors typeFromSingleReturnExpression's selection
// (_tsc.js:134407-134441) for reporting only; it is not an oracle for its use.
function returnTrace(sourceFile) {
  const file = sourceFile.fileName, rows = [];
  const visit = node => {
    if (FUNCTION_LIKE.has(node.kind)) {
      const tags = ts.getJSDocTags(node);
      let candidate;
      if (node.body && !ts.nodeIsMissing(node.body)) {
        if (ts.isBlock(node.body)) {
          ts.forEachReturnStatement(node.body, statement => {
            if (statement.parent !== node.body) { candidate = undefined; return true; }
            if (!candidate) { candidate = statement.expression; } else { candidate = undefined; return true; }
          });
        } else candidate = node.body;
      }
      const effective = ts.getEffectiveReturnTypeNode(node);
      rows.push({host: identity(node, file), name: node.name && ts.isIdentifier(node.name) ? node.name.text : null,
        direct_jsdoc_count: node.jsDoc?.length ?? 0,
        owned_tags: tags.map(tag => ({kind: kindName(tag.kind), pos: tag.pos, end: tag.end,
          attached_to: kindName(tag.parent.parent.kind)})),
        jsdoc_return_type: identity(ts.isInJSFile(node) ? ts.getJSDocReturnType(node) : undefined, file),
        effective_return_type_node: identity(effective, file),
        single_return_candidate: identity(candidate, file),
        candidate_is_jsdoc_type_assertion: candidate ? ts.isJSDocTypeAssertion(candidate) : null});
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return rows;
}
function complete(files, roots, options) {
  const input = new Map(files.map(file => [file.path, file.text]));
  const base = ts.createCompilerHost(options, true);
  const host = {...base, getCurrentDirectory: () => "/project",
    getSourceFile: (name, version) => input.has(name)
      ? ts.createSourceFile(name, input.get(name), version, true) : base.getSourceFile(name, version),
    fileExists: name => input.has(name) || base.fileExists(name),
    readFile: name => input.has(name) ? input.get(name) : base.readFile(name),
    directoryExists: name => name === "/project" || name === "/project/out" || base.directoryExists(name),
    writeFile: () => assert.fail("writes must use the captured callback")};
  const program = ts.createProgram(roots, options, host);
  const trace = program.getSourceFiles().filter(file => !program.isSourceFileDefaultLibrary(file))
    .flatMap(returnTrace);
  const writes = [], diagnostics = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => {assert.equal(result, undefined); return result = emit(...args);};
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program,
    d => diagnostics.push(diagnostic(d)), text => status.push(value(text)), undefined,
    (name, text, bom, onError, sources, data) => writes.push({
      index: writes.length, path: name, callback: value(text), write_byte_order_mark: bom,
      materialized_utf8_base64: Buffer.from((bom ? "﻿" : "") + text).toString("base64"),
      on_error_callback_present: onError !== undefined,
      source_files: sources?.map(s => s.fileName) ?? null,
      data_present: data !== undefined, data_keys: data === undefined ? null : Object.keys(data),
      data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
      data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null}));
  assert.ok(result);
  return {observation: {writes, reported_diagnostics: diagnostics, status_writes: status, exit_code: exit,
    emit_result: {emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps?.map(entry => ({
        input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap)})) ?? null}},
    return_trace: trace};
}
// The original G5c tuple: target es2015, allowJs, checkJs, declaration, outDir
// (ratchets/h2-8a-candidate-inputs.v1.json); module stays unset as there.
const baseOptions = {target: 2, allowJs: true, checkJs: true, declaration: true, newLine: 0,
  skipDefaultLibCheck: true, noErrorTruncation: true, outDir: "/project/out"};
const inputs = [];
function add(id, group, source, options = {}, roots) {
  const files = typeof source === "string" ? [{path: "/project/main.js", text: source}] : source;
  inputs.push({id, group, files, roots: roots ?? files.map(file => file.path), options: {...baseOptions, ...options}});
}
const STAR = "/** @type {*} */(null)";
const D = `/**\n * @param {number} a\n * @param {number} b\n * @return {string} \n */\nmodule.exports.d = function d(a, b) { return ${STAR}; }\n`;
const E = `/**\n * @template T,U\n * @param {T} a\n * @param {U} b\n * @return {T & U} \n */\nmodule.exports.e = function e(a, b) { return ${STAR}; }\n`;
// R1: the minimal cause controls around the original d / e statements.
add("r1-original-d-string", "R1", D);
add("r1-original-e-template-intersection", "R1", E);
add("r1-returns-alias", "R1", `/**\n * @returns {string}\n */\nmodule.exports.d = function d() { return ${STAR}; }\n`);
add("r1-body-star-without-annotation", "R1", `module.exports.d = function d() { return ${STAR}; }\n`);
add("r1-plain-body-without-annotation", "R1", `module.exports.p = function p() { return 1; }\n`);
// R2: ownership — which attachment the function-like host reaches.
add("r2-exports-property", "R2", `/** @return {string} */\nexports.f = function (a) { return ${STAR}; };\n`);
add("r2-variable-initializer", "R2", `/** @return {string} */\nconst v = function () { return ${STAR}; };\nmodule.exports.v = v;\n`);
add("r2-function-declaration", "R2", `/** @return {string} */\nfunction g() { return ${STAR}; }\nmodule.exports.g = g;\n`);
add("r2-arrow-expression-body", "R2", `/** @return {string} */\nmodule.exports.arrow = () => ${STAR};\n`);
add("r2-parenthesized-function", "R2", `/** @return {string} */\nmodule.exports.paren = (function () { return ${STAR}; });\n`);
add("r2-prototype-method", "R2", `function C() {}\n/** @return {string} */\nC.prototype.m = function () { return ${STAR}; };\nmodule.exports.C = C;\n`);
add("r2-ordinary-property", "R2", `const o = {};\n/** @return {string} */\no.m = function () { return ${STAR}; };\nmodule.exports.o = o;\n`);
add("r2-object-literal-method", "R2", `module.exports.obj = {\n  /** @return {string} */\n  m() { return ${STAR}; },\n};\n`);
add("r2-property-assignment-function", "R2", `module.exports.obj = {\n  /** @return {string} */\n  n: function () { return ${STAR}; },\n};\n`);
add("r2-class-method", "R2", `class K {\n  /** @return {string} */\n  m() { return ${STAR}; }\n}\nmodule.exports.K = K;\n`);
// R3: order and stopping — competing blocks, untyped tags, callable @type, siblings.
add("r3-inner-tag-before-outer", "R3", `/** @return {string} */\nmodule.exports.x = /** @return {number} */ function () { return ${STAR}; };\n`);
add("r3-last-block-owns-tags", "R3", `/** @return {number} */\n/** @return {string} */\nmodule.exports.y = function () { return ${STAR}; };\n`);
add("r3-last-block-owns-tags-declaration", "R3", `/** @return {number} */\n/** @return {string} */\nfunction y() { return ${STAR}; }\nmodule.exports.y = y;\n`);
add("r3-untyped-return-tag", "R3", `/** @return */\nmodule.exports.z = function () { return ${STAR}; };\n`);
add("r3-untyped-then-typed-return-tags", "R3", `/**\n * @return\n * @return {string}\n */\nfunction z() { return ${STAR}; }\nmodule.exports.z = z;\n`);
add("r3-return-tag-over-callable-type-tag", "R3", `/**\n * @type {() => number}\n * @return {string}\n */\nmodule.exports.t = function () { return ${STAR}; };\n`);
add("r3-callable-type-tag-only", "R3", `/** @type {() => number} */\nmodule.exports.t = function () { return ${STAR}; };\n`);
add("r3-sibling-jsdoc-not-inherited", "R3", `/** @return {string} */\nmodule.exports.s1 = function () { return 1; };\nmodule.exports.s2 = function () { return ${STAR}; };\n`);
add("r3-type-assertion-parenthesized-owner", "R3", `module.exports.q = /** @type {() => string} */ (function () { return ${STAR}; });\n`);
// R4: generic identity — template parameters, shadowing, typedef/import returns, other sources.
add("r4-template-constraint-default", "R4", `/**\n * @template {string} T\n * @template [U=number]\n * @param {T} a\n * @return {T | U}\n */\nmodule.exports.cd = function cd(a) { return ${STAR}; };\n`);
add("r4-template-shadows-typedef", "R4", `/** @typedef {number} T */\n/**\n * @template T\n * @param {T} a\n * @return {T}\n */\nmodule.exports.s = function s(a) { return ${STAR}; };\n/** @return {T} */\nmodule.exports.n = function n() { return ${STAR}; };\n`);
add("r4-typedef-return", "R4", `/** @typedef {{ x: number }} Point */\n/** @return {Point} */\nmodule.exports.mk = function mk() { return ${STAR}; };\n`);
add("r4-import-type-return", "R4", [
  {path: "/project/main.js", text: `/** @return {import("./other").Thing} */\nmodule.exports.mk = function mk() { return ${STAR}; };\n`},
  {path: "/project/other.d.ts", text: "export interface Thing { id: number; }\n"},
], {}, ["/project/main.js"]);
add("r4-same-name-template-two-sources", "R4", [
  {path: "/project/a.js", text: `/**\n * @template T\n * @param {T} a\n * @return {T}\n */\nmodule.exports.id = function id(a) { return ${STAR}; };\n`},
  {path: "/project/b.js", text: `/**\n * @template T\n * @param {T} a\n * @return {T[]}\n */\nmodule.exports.wrap = function wrap(a) { return ${STAR}; };\n`},
]);
// R5: the return route per annotation shape and body shape.
add("r5-literal-annotation", "R5", `/** @return {"lit"} */\nmodule.exports.l = function l() { return ${STAR}; };\n`);
add("r5-union-annotation", "R5", `/** @return {string | number} */\nmodule.exports.u = function u() { return ${STAR}; };\n`);
add("r5-callable-type-literal", "R5", `/** @return {{ (): void; x: number }} */\nmodule.exports.c = function c() { return ${STAR}; };\n`);
add("r5-jsdoc-function-type", "R5", `/** @return {function(number): string} */\nmodule.exports.jf = function jf() { return ${STAR}; };\n`);
add("r5-async-promise", "R5", `/** @return {Promise<string>} */\nmodule.exports.as = async function () { return ${STAR}; };\n`);
add("r5-generator", "R5", `/** @return {Generator<number>} */\nmodule.exports.gen = function* () { yield 1; };\n`);
add("r5-async-generator-without-annotation", "R5", `module.exports.ag = async function* () { yield 1; };\n`);
add("r5-single-return-without-annotation", "R5", `module.exports.one = function () { return "a"; };\n`);
add("r5-multiple-returns-without-annotation", "R5", `module.exports.two = function (b) { if (b) return 1; return 2; };\n`);
// R6: priority and negative controls.
add("r6-ts-direct-type-ignores-jsdoc", "R6", [{path: "/project/main.ts", text: "/** @return {string} */\nexport function t(): number { return 1; }\n"}]);
add("r6-accessor-pair", "R6", `module.exports.acc = {\n  /** @return {string} */\n  get g() { return ${STAR}; },\n  set g(v) {},\n};\n`);
add("r6-jsdoc-construct-signature", "R6", `/** @typedef {{ x: number }} Point */\n/** @type {function(new: Point, number)} */\nmodule.exports.ctor = function (n) {};\n`);
add("r6-annotation-body-mismatch", "R6", `/** @return {number} */\nmodule.exports.mis = function mis(b) { if (b) return 1; return "x"; };\n`);
add("r6-unresolved-annotation", "R6", `/** @return {NotDefined} */\nmodule.exports.bad = function bad() { return ${STAR}; };\n`);
// Semantic fallback route: several asserted returns keep their JSDoc type
// assertions in checkAndAggregateReturnExpressionTypes (skipParentheses with
// excludeJSDocTypeAssertions, _tsc.js:78959-79008).
add("r6-asserted-returns-semantic-fallback", "R6", `function c(x) {\n  if (x) return ${STAR};\n  return ${STAR};\n}\nmodule.exports.c = c;\n`);
add("r6-asserted-await-returns-semantic-fallback", "R6", `/** @param {Promise<number>} p */\nmodule.exports.aw = async function (p) {\n  if (p) return /** @type {*} */(await p);\n  return await /** @type {*} */(p);\n};\n`);
// R7: command variations over the original d / e statements.
add("r7-allowjs-without-checkjs", "R7", D + E, {checkJs: false});
add("r7-emit-declaration-only", "R7", D + E, {emitDeclarationOnly: true});
add("r7-noemitonerror-blocked", "R7", D + E + `/** @return {number} */\nmodule.exports.mis = function mis(b) { if (b) return 1; return "x"; };\n`, {noEmitOnError: true});
add("r7-noemit-with-declaration", "R7", D + E, {noEmit: true});
add("r7-declaration-map", "R7", D + E, {declarationMap: true});
add("r7-remove-comments", "R7", D + E, {removeComments: true});
// R8: UTF-16 literal returns and the previously repaired comment/parameter paths.
add("r8-lone-surrogate-escape-literal", "R8", `/** @return {"\\uD800"} */\nmodule.exports.lone = function lone() { return ${STAR}; };\n`);
add("r8-astral-pair-literal", "R8", `/** @return {"\u{1F600}"} */\nmodule.exports.pair = function pair() { return ${STAR}; };\n`);
add("r8-replacement-character-literal", "R8", `/** @return {"�"} */\nmodule.exports.rep = function rep() { return ${STAR}; };\n`);
add("r8-g4a-function-parameter-and-return", "R8", `/** @typedef {number} Num */\n\n/**\n * @param {Num} a\n * @return {string}\n */\nfunction g(a) { return ${STAR}; }\nmodule.exports.g = g;\n`);
add("r8-g4b-class-method-owned-comment", "R8", `class C {\n  /**\n   * @param {number} a\n   * @return {string}\n   */\n  m(a) { return ${STAR}; }\n}\nmodule.exports.C = C;\n`);
add("r8-detached-prefix-with-return", "R8", `// detached prefix\n\n/** @return {string} */\nmodule.exports.d = function d() { return ${STAR}; };\n`);
add("r8-parameter-tag-binding-with-return", "R8", `/**\n * @param {{ x: number }} item\n * @return {string}\n */\nexports.f = function({x}) { return ${STAR}; };\n`);
assert.equal(new Set(inputs.map(input => input.id)).size, inputs.length);
const cases = [];
for (const input of inputs) {
  const runs = [complete(input.files, input.roots, input.options), complete(input.files, input.roots, input.options)];
  assert.deepEqual(runs[0], runs[1], input.id);
  cases.push({...input, files: input.files.map(file => ({...file, sha256: sha(Buffer.from(file.text))})),
    complete_command_runs: runs.map(run => run.observation), return_trace: runs[0].return_trace, native: "exact"});
}
const artifact = {version: 1,
  scope: "H2.8a G5c JSDoc return annotation controls (R1 cause, R2 ownership, R3 order, R4 generic identity, R5 route, R6 priority, R7 commands, R8 UTF-16 and repaired paths); upstream complete command observations twice plus the public-API return trace; native outcome separate",
  typescript: ts.version, compiler_sha256: compilerSha, observer_sha256: sha(fs.readFileSync(import.meta.filename)),
  repetitions: 2, complete_command_executions: cases.length * 2, cases};
const output = path.join(root, "crates/compiler/tests/fixtures/h2-8a-jsdoc-return.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") fs.writeFileSync(output, JSON.stringify(artifact, null, 2) + "\n", {flag: "wx"});
else assert.deepEqual(JSON.parse(fs.readFileSync(output)), artifact);
console.log(JSON.stringify({output, sha256: sha(fs.readFileSync(output)), cases: cases.length,
  complete_command_executions: cases.length * 2, traced_functions: cases.reduce((n, c) => n + c.return_trace.length, 0)}));
