// Complete commands for the r77 metadata, assignment-reference and comment boundaries.
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
const inputs = [];
function add(group, shape, text, target, module, removeComments, extra = {}) {
  const main = "/project/main.ts";
  inputs.push({case_id: `emitter-recovery-boundaries/${group}/${target}/${module}/remove-${removeComments}/${shape}`,
    roots: [main], files: [{path: main, text: "declare const dec: any, x: any, y: any, k: any, source: any;\n" + text + "\n"}], options: {},
    config: JSON.stringify({compilerOptions: {target, module, removeComments, lib: ["esnext"],
      strict: false, skipDefaultLibCheck: true, noErrorTruncation: true, sourceMap: true,
      ignoreDeprecations: "6.0", outDir: "/project/out", ...extra}, files: ["main.ts"]})});
}
const metadata = [
  ["empty-constructor", "@dec class C { constructor() {} } export {C};"],
  ["no-constructor", "@dec class C {} export {C};"],
  ["parameter-only", "class C { constructor(@dec x: any) {} } export {C};"],
  ["abstract", "@dec abstract class C { constructor() {} } export {C};"],
  ["decorator-export", "@dec export class C { constructor() {} }"],
  ["export-decorator", "export @dec class C { constructor() {} }"],
  ["decorator-export-default", "@dec export default class C { constructor() {} }"],
  ["export-default-decorator", "export default @dec class C { constructor() {} }"],
  ["getter-only", "export class C { @dec get p(): any { return x; } }"],
  ["setter-only", "export class C { @dec set p(v: any) {} }"],
  ["pair-first", "export class C { @dec get p(): any { return x; } set p(v: any) {} }"],
  ["pair-second", "export class C { get p(): any { return x; } @dec set p(v: any) {} }"],
  ["static-getter", "export class C { @dec static get p(): any { return x; } }"],
  ["public-getter", "export class C { @dec public get p(): any { return x; } }"],
  ["override-getter", "declare class B { get p(): any; } export class C extends B { @dec override get p(): any { return x; } }"],
  ["method", "export class C { @dec m(x: any): any { return x; } }"],
  ["property", "export class C { @dec p: any; }"],
];
for (const target of ["es5", "es2015", "es2022"])
 for (const module of ["commonjs", "system"])
  for (const remove of [false,true]) {
   for (const [shape,text] of metadata)
    add("metadata",shape,text,target,module,remove,{experimentalDecorators:true,emitDecoratorMetadata:true});
   for (const [shape,text] of metadata.filter(([shape]) => ["empty-constructor","pair-second","public-getter","method"].includes(shape)))
    add("metadata-off",shape,text,target,module,remove,{experimentalDecorators:true,emitDecoratorMetadata:false});
  }
assert.equal(inputs.length,252);
const receivers = [["property","x?.y"],["element","x?.[k]"],["nested","x?.y.z"],["this","this?.x"]];
for (const target of ["es5","es2015","es2022"])
 for (const module of ["commonjs","esnext"])
  for (const remove of [false,true])
   for (const [shape,expr] of receivers) {
    add("standard-class",shape,`@(${expr}) class C {} export {C};`,target,module,remove);
    add("standard-member",shape,`export class C { @(${expr}) m() {} }`,target,module,remove);
   }
assert.equal(inputs.length,348);
const comments = [
 ["type-assertion", "export const z = <any>/*😀*/y;"],
 ["as", "export const z = y /*😀*/ as any;"],
 ["as-newline", "export const z = y\n /*😀*/ as any;"],
 ["non-null", "export const z = y /*😀*/ !;"],
 ["satisfies", "export const z = y /*😀*/ satisfies any;"],
 ["nested", "export const z = (<any>/*😀*/y) /*after*/ as any;"],
];
for (const target of ["es5","es2015","es2022"])
 for (const module of ["commonjs","esnext"])
  for (const remove of [false,true])
   for (const [shape,text] of comments) add("comments",shape,text,target,module,remove);
assert.equal(inputs.length,420);
const assignments = [
 ["using-renamed", "using r = source; export let {x: y} = source;"],
 ["using-array-rest", "using r = source; export let [x,...r2] = source;"],
 ["using-default", "using r = source; export let {x = y} = source;"],
 ["using-rest", "using r = source; export let {...r2} = source;"],
 ["parsed-rest", "export let a: any, r: any; ({a,...r} = source);"],
 ["parsed-renamed", "export let {x: y} = source; export let [a,...r] = source;"],
];
for (const target of ["es5","es2015","es2022"])
 for (const module of ["commonjs","system","esnext"])
  for (const remove of [false,true])
   for (const [shape,text] of assignments) add("assignments",shape,text,target,module,remove);
assert.equal(inputs.length,528);
for (const target of ["es5","es2015","es2022"])
 for (const module of ["commonjs","system"])
  for (const remove of [false,true]) {
    const text = "@dec export class C { constructor(public x: any) {} }";
    add("metadata-extra","parameter-property",text,target,module,remove,{experimentalDecorators:true,emitDecoratorMetadata:true});
    add("metadata-off-extra","parameter-property",text,target,module,remove,{experimentalDecorators:true,emitDecoratorMetadata:false});
    add("metadata-off-extra","override-getter",metadata.find(([name]) => name === "override-getter")[1],target,module,remove,{experimentalDecorators:true,emitDecoratorMetadata:false});
  }
assert.equal(inputs.length,564);
const commentOwners = [
 ["assertion-call-missing","export {}; source(<number /*c*/, 2);"],
 ["assertion-array-missing","export {}; [<number /*c*/, 1];"],
 ["assertion-binary-missing","export {}; 1 + <number /*c*/;"],
 ["paren-line","export const z = (x //c\n);"],
 ["paren-block","export const z = (x /*c*/);"],
 ["paren-leading","export const z = (x\n/*c*/);"],
 ["paren-missing","export const z = (x /*c*/\n as number);"],
 ["paren-missing-nested","export const z = ((x //c\n as number));"],
 ["paren-optional-leading","export const z = (x?.y\n// c\n);"],
 ["heritage-before-types","export class C extends x /*c*/<any> {}"],
 ["heritage-inside-types","export class C extends x<any /*c*/> {}"],
 ["heritage-after-types","export class C extends x<any> /*c*/ {}"],
 ["heritage-parenthesized","export class C extends (x) {}"],
 ["heritage-qualified","export class C extends x.y<any> {}"],
 ["heritage-optional","export class C extends x?.y {}"],
 ["heritage-optional-trivia","export class C extends x?.y /*c*/ {}"],
];
for (const target of ["es5","es2015","es2022"])
 for (const module of ["commonjs","esnext"])
  for (const remove of [false,true])
   for (const [shape,text] of commentOwners) add("comment-owners",shape,text,target,module,remove);
assert.equal(inputs.length,756);
for (const target of ["es5","es2015","es2022"])
 for (const module of ["commonjs","system","esnext"])
  for (const remove of [false,true])
   add("assignment-role","using-shorthand","using r = source; export let {x,y} = source;",target,module,remove);
assert.equal(inputs.length,774);

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
const cases = inputs.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  return {...input, typescript_observation: first};
});
const artifact = {version: 1, typescript: ts.version, repetitions: 2,
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), cases};
const destination = path.join(root, "crates/compiler/tests/fixtures/emitter-recovery-boundaries.json");
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered, {flag: "wx"});
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`Recovery boundary neighbours: ${cases.length} cases, two identical complete observations each`);
