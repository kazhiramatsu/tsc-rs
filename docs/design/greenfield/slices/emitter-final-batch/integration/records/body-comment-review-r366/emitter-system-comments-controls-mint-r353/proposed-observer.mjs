// Complete commands for await-context recovery owners and clean script neighbours.
// The host and tuple match the established import-helper command observer.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const malformed = [
  ["comma-operand", "await (1,);"],
  ["assertion-call", "await <number, string>(1);"],
  ["assertion-template", "await <number, string> ``;"],
  ["heritage-extends", "class C extends await<string> {}"],
  ["heritage-implements", "class C implements await<string> {}"],
  ["decorator-direct", "@await class C {}"],
  ["decorator-call", "@await(x) class C {}"],
  ["decorator-await-expression", "@(await) class C {}"],
  ["decorator-comment", "@ /*😀*/await(x) class C {}"],
  ["member-decorators", "class C { @await ['a']() {} @await(1) ['b']() {} @(await) ['c']() {} }"],
  ["parameter-decorators", "class C { m(@await x: any) {} n(@await(1) x: any) {} p(@(await) x: any) {} }"],
  ["multiple-reparse-runs", "await (1,); const stable = 1; @await class C {} const after = 2;"],
];
const shapes = malformed.map(([shape, text]) => [shape, "export {};\n" + text, true]);
for (const shape of ["decorator-direct", "decorator-call", "decorator-await-expression"]) {
  shapes.push([shape + "-standard", "export {};\n" + malformed.find(row => row[0] === shape)[1], false]);
}
for (const experimentalDecorators of [false, true]) {
  for (const [shape, text] of [
    ["script-decorator-identifier", "@await class C {}"],
    ["script-heritage-identifier", "class C extends await<string> {}"],
  ]) shapes.push([shape + (experimentalDecorators ? "-legacy" : "-standard"), text, experimentalDecorators]);
}
shapes.push(["script-comma-operand", "(1,);", false]);
const wrappers = [
  ["decorator-non-null", "@await! class C {}"],
  ["decorator-tagged", "@await`x` class C {}"],
  ["decorator-optional-property", "@await?.x class C {}"],
  ["decorator-optional-element", "@await?.[x] class C {}"],
  ["decorator-generic-call", "@await<T>() class C {}"],
  ["decorator-generic-tagged", "@await<T>`x` class C {}"],
];
for (const experimentalDecorators of [false, true])
  for (const [shape, text] of wrappers)
    shapes.push([shape + (experimentalDecorators ? "-legacy" : "-standard"), "export {};\n" + text, experimentalDecorators]);
for (const [shape, text] of [
  ["heritage-trivia", "export {}; class C extends /*😀*/await<string /*😀*/> {}"],
  ["comma-trivia", "(1, /*😀*/);"],
  ["assertion-trivia", "export {}; await <number /*😀*/, string>(1);"],
  ["interface-heritage-clean", "export {}; interface I extends await<string> {}"],
]) shapes.push([shape, text, true]);
for (const [shape, text] of [
  ["heritage-following-comma", "export {}; class C extends await<A>, B {}"],
  ["heritage-preceding-comma", "export {}; class C implements A, await<B> {}"],
  ["unparenthesized-comma", "export {}; 1, ;"],
  ["missing-closer", "export {}; await (1,"],
  ["reparse-overshoot", "export {}; let a = await /1; b; c; x/;"],
  ["reparse-overshoot-followed", "export {}; let a = await /1; b; c; x/; await (1,);"],
]) shapes.push(["refused-" + shape, text, true]);
const isRefusedControl = input => input.case_id.includes("/refused-");
const inputs = [];
for (const target of ["es5", "es2015", "esnext"])
  for (const module of ["commonjs", "esnext"])
    for (const removeComments of [false, true])
      for (const [shape, text, experimentalDecorators] of shapes) {
        const main = "/project/main.ts";
        inputs.push({case_id: `emitter-context-recovery/${target}/${module}/remove-${removeComments}/${shape}`,
          roots: [main], files: [{path: main, text: text + "\n"}], options: {},
          config: JSON.stringify({compilerOptions: {target, module, removeComments, experimentalDecorators,
            strict: false, skipDefaultLibCheck: true, noErrorTruncation: true, sourceMap: true,
            ignoreDeprecations: "6.0", outDir: "/project/out"}, files: [main.slice(9)]})});
      }
assert.equal(inputs.length, 504);

// The empty declaration list and its following numeric statement have separate
// retained syntax owners. Keep declaration/maps and comment modes in the tuple.
const emptyVariables = [
  ["comment-before-equals", "const /*before*/ = 5;"],
  ["comment-after-equals", "const = /*after*/ 5;"],
  ["comment-before-semicolon", "const = 5 /*tail*/;"],
  ["comment-after-semicolon", "const = 5; /*after*/"],
  ["jsdoc-empty-var", "/** @type {number} */ var /*c*/ = 1;", true],
  ["witness", "declare function sink(value: unknown): void;\nconst = 5;\nsink(0);"],
  ["first-const", "const = 5;"],
  ["first-var", "var = 5;"],
  ["unicode-prefix", "/* 😀 */ const = 5;"],
  ["comments", "const /*a*/ = /*b*/ 5;"],
  ["two-statements", "const = 5; const = 6;"],
  ["external-module", "export {};\nconst = 5;"],
  ["leading-binding-composition", "const = 5;\nexport const \\u{10400} = 1;"],
];
for (const target of ["es5", "es2015"])
  for (const module of ["commonjs", "system", "esnext"])
    for (const removeComments of [false, true])
      for (const [shape, text, javascript] of emptyVariables) {
        const main = javascript ? "/project/main.js" : "/project/main.ts";
        inputs.push({case_id: `emitter-context-recovery/empty-variable/${target}/${module}/remove-${removeComments}/${shape}`,
          roots: [main], files: [{path: main, text: text + "\n"}], options: {},
          config: JSON.stringify({compilerOptions: {target, module, removeComments,
            ...(javascript ? {allowJs: true, checkJs: true} : {}),
            strict: false, skipDefaultLibCheck: true, noErrorTruncation: true,
            sourceMap: true, declaration: true, declarationMap: true, alwaysStrict: shape === "first-const",
            ignoreDeprecations: "6.0", outDir: "/project/out"}, files: [main.slice(9)]})});
      }
assert.equal(inputs.length, 660);

// System visitNodes2 preserves the incoming array range only for a full visit.
const systemRanges = [
  ["plain", "export const x = 5;\n"],
  ["empty", "export {};\n"],
  ["directive", "\"use strict\";\nexport const x = 5;\n"],
  ["erased-declaration", "declare const source: number;\nexport const x = 5;\n"],
  ["class-prologue", "export class C { static x = 1; }\n"],
  ["async-helper", "export async function f() { return 1; }\n"],
  ["async-execute", "export {};\nawait 0;\n"],
  ["export-star", "export * from './dep';\n"],
  ["leading-trivia", "/* 😀 */\n\nexport const x = 5;\n"],
  ["trailing-trivia", "export const x = 5; /* tail */\n// last\n"],
  ["crlf", "export const x = 5;\r\n"],
  ["no-eol", "export const x = 5;"],
];
for (const target of ["es5", "es2015"])
  for (const alwaysStrict of [false, true])
    for (const removeComments of [false, true])
      for (const [shape, text] of systemRanges) {
        const main = "/project/main.ts";
        inputs.push({case_id: `emitter-context-recovery/system-range/${target}/strict-${alwaysStrict}/remove-${removeComments}/${shape}`,
          roots: [main], files: [{path: main, text}, ...(shape === "export-star"
            ? [{path: "/project/dep.ts", text: "export const y = 1;\n"}] : [])], options: {},
          config: JSON.stringify({compilerOptions: {target, module: "system", alwaysStrict, removeComments,
            strict: false, skipDefaultLibCheck: true, noErrorTruncation: true,
            sourceMap: true, declaration: true, declarationMap: true,
            ignoreDeprecations: "6.0", outDir: "/project/out"}, files: [main.slice(9)]})});
      }
assert.equal(inputs.length, 756);

// Range-owned function-body comment boundaries, including empty and erased lists.
const systemCommentBoundaries = [
  ["pinned-prefix", "/*! keep */\n\nexport const x = 5;\n"],
  ["line-prefix", "// detached\n\nexport const x = 5;\n"],
  ["attached-prefix", "/* attached */\nexport const x = 5;\n"],
  ["brace-in-trivia", "export const x = '}';\n// } last\n"],
  ["removed-tail", "export const x = 5;\n// before type\ninterface Gone {}\n// after type\n"],
  ["empty-execute", "/*! keep */\n\nexport {};\n// end\n"],
  ["erased-prologue", "/*! keep */\n\ndeclare const marker: number;\nexport const x = 5;\n// end\n"],
  ["ordinary-function", "export function f() {/* inner */\n\nreturn 1;\n// last\n}\n"],
];
for (const target of ["es5", "es2015"])
  for (const alwaysStrict of [false, true])
    for (const removeComments of [false, true])
      for (const [shape, text] of systemCommentBoundaries) {
        inputs.push({case_id: `emitter-context-recovery/system-comment-boundary/${target}/strict-${alwaysStrict}/remove-${removeComments}/${shape}`,
          roots: ["/project/main.ts"], files: [{path: "/project/main.ts", text}], options: {},
          config: JSON.stringify({compilerOptions: {target, module: "system", alwaysStrict, removeComments,
            strict: false, skipDefaultLibCheck: true, noErrorTruncation: true,
            sourceMap: true, declaration: true, declarationMap: true,
            ignoreDeprecations: "6.0", outDir: "/project/out"}, files: ["main.ts"]})});
      }
assert.equal(inputs.length, 820);



function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [name, text, bom, onError, sources, data] = args;
  const bytes = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), bytes]) : bytes;
  return { index, path: name, kind: ts.isDeclarationFileName(name) ? "declaration" : name.endsWith(".map") && ts.isDeclarationFileName(name.slice(0, -4)) ? "declaration-map" : name.endsWith(".map") ? "source-map" : name.endsWith(".mjs") ? "mjs" : name.endsWith(".cjs") ? "cjs" : "javascript",
    callback_utf8_base64: bytes.toString("base64"), callback_utf8_bytes: bytes.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sources?.map(source => source.fileName) ?? null,
    data_present: data !== undefined, data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null };
}
function sourceMaps(maps) {
  return maps?.map(entry => {
    assert.deepEqual(Object.keys(entry).sort(), ["inputSourceFileNames", "sourceMap"]);
    return { input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap) };
  }) ?? null;
}
function observe(input) {
  const sensitive = input.use_case_sensitive_file_names ?? true;
  const canonical = ts.createGetCanonicalFileName(sensitive);
  const files = new Map(input.files.map(file => [canonical(file.path), file.text]));
  const libraryRoot = path.join(root, "vendor/typescript-6.0.3/lib");
  const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name) && fs.existsSync(path.join(libraryRoot, path.basename(name)));
  const read = name => files.get(canonical(ts.normalizePath(name))) ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(files.keys(), { currentDirectory: "/project", useCaseSensitiveFileNames: sensitive,
    fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  const host = { ...ts.createCompilerHost(input.options, true), ...overlay,
    getCurrentDirectory: () => "/project", getDefaultLibFileName: options => "/lib/" + ts.getDefaultLibFileName(options),
    getDefaultLibLocation: () => "/lib", useCaseSensitiveFileNames: () => sensitive, getCanonicalFileName: canonical,
    readFile: read, fileExists: name => files.has(canonical(ts.normalizePath(name))) || library(name),
    getSourceFile: (name, options) => { const text = read(name); return text === undefined ? undefined : ts.createSourceFile(name, text, options, true); },
    writeFile: () => assert.fail("unexpected host write") };
  let options = input.options, roots = input.roots ?? input.files.map(file => file.path), errors = [];
  if (input.config) {
    const configPath = "/project/tsconfig.json";
    const parsed = ts.parseJsonSourceFileConfigFileContent(ts.parseJsonText(configPath, input.config),
      { ...host, readDirectory: () => roots }, "/project", undefined, configPath);
    options = parsed.options; roots = parsed.fileNames; errors = parsed.errors;
  }
  const program = ts.createProgram({ rootNames: roots, options, host, configFileParsingDiagnostics: errors });
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)), s => status.push(s), undefined,
    (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: sourceMaps(result.sourceMaps) },
    status_writes: status, exit_code: exit };
}
const observations = inputs.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  if (isRefusedControl(input)) {
    assert.equal(first.emit_refused, false, input.case_id);
    assert.ok(first.writes.some(write => write.kind === "javascript"), input.case_id);
  }
  return {...input, typescript_observation: first};
});
const cases = observations.filter(input => !isRefusedControl(input));
const refused_cases = observations.filter(isRefusedControl);
assert.equal(cases.length, 748);
assert.equal(refused_cases.length, 72);
const artifact = {version: 1, typescript: ts.version, repetitions: 2,
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases, refused_cases};
const destination = path.join(root, "crates/compiler/tests/fixtures/emitter-context-recovery.json");
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered, {flag: "wx"});
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`Context recovery: ${cases.length} supported controls + ${refused_cases.length} refused neighbours, two identical complete observations each`);
