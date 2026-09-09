// H2.8a Class field alias map positions. Expectations are complete TS commands.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/class-field-alias-map-positions.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const defaults = { strict:true, allowJs:true, checkJs:true, declaration:true, declarationMap:true, sourceMap:true,
  target:ts.ScriptTarget.ES2015, skipDefaultLibCheck:true, noErrorTruncation:true,
  newLine:ts.NewLineKind.CarriageReturnLineFeed, outDir:"/project/out" };
const shapes = [
  [
    "anonymous-field",
    "const Foo = class { static self: any = this; }; export { Foo };"
  ],
  [
    "named-field",
    "const Foo = class Inner { static self: any = this; }; export { Foo };"
  ],
  [
    "declaration-field",
    "export class Foo { static self: any = this; }"
  ],
  [
    "field-arrow",
    "const Foo = class { static self: any = () => this; }; export { Foo };"
  ],
  [
    "field-function",
    "const Foo = class { static self: any = function (this: unknown) { return this; }; }; export { Foo };"
  ],
  [
    "private-field",
    "const Foo = class { static #self: any = this; static get self(): any { return this.#self; } }; export { Foo };"
  ],
  [
    "auto-accessor",
    "const Foo = class { static accessor self: any = this; }; export { Foo };"
  ],
  [
    "static-block",
    "const Foo = class { static self: any; static { this.self = this; } }; export { Foo };"
  ],
  [
    "static-block-arrow",
    "const Foo = class { static self: any; static { this.self = () => this; } }; export { Foo };"
  ],
  [
    "static-block-function",
    "const Foo = class { static self: any; static { this.self = function (this: unknown) { return this; }; } }; export { Foo };"
  ],
  [
    "mixed-field-block",
    "const Foo = class { static self: any = this; static { this.self = this; } static last: any = this; }; export { Foo };"
  ],
  [
    "existing-parentheses",
    "const Foo = (class { static self: any = this; }); export { Foo };"
  ],
  [
    "return-class",
    "export function Foo() { return class { static self: any = this; }; }"
  ],
  [
    "concise-arrow",
    "export const Foo = () => class { static self: any = this; };"
  ],
  [
    "assignment-class",
    "export let Foo: any; Foo = class { static self: any = this; };"
  ],
  [
    "comma-class",
    "const Foo = (0, class { static self: any = this; }); export { Foo };"
  ],
  [
    "call-argument",
    "function keep<T>(value: T): T { return value; } export const Foo = keep(class { static self: any = this; });"
  ],
  [
    "conditional-class",
    "declare const condition: boolean; const Foo = condition ? class { static self: any = this; } : class {}; export { Foo };"
  ],
  [
    "object-property",
    "export const Foo = { value: class { static self: any = this; } };"
  ],
  [
    "array-element",
    "export const Foo = [class { static self: any = this; }];"
  ],
  [
    "wrapper-comments",
    "const Foo = /* before */ (/* inside */ class { static self: any = this; } /* tail */); export { Foo };"
  ],
  [
    "field-comments",
    "const Foo = class { /* field */ static self: any = /* value */ this /* end */; }; export { Foo };"
  ],
  [
    "many-inline-operands",
    "const Foo = class { static a = 1; static b = 2; static c = 3; static d = 4; static e = 5; static f = 6; static g = 7; static h = 8; static i = 9; static j = 10; static self: any = this; }; export { Foo };"
  ],
  [
    "plain-field-adjacent",
    "const Foo = class { static value = 1; }; export { Foo };"
  ],
  [
    "instance-this-adjacent",
    "const Foo = class { self: any = this; }; export { Foo };"
  ],
  [
    "method-this-adjacent",
    "const Foo = class { static self(): any { return this; } }; export { Foo };"
  ],
  [
    "nested-class-field",
    "const Foo = class { static nested = class { static self: any = this; }; static self: any = this; }; export { Foo };"
  ],
  [
    "nested-computed-name",
    "const Foo = class { static key = 'member'; static nested = class { [this.key]() {} }; }; export { Foo };"
  ],
  [
    "no-map-adjacent",
    "const Foo = class { static self: any = this; }; export { Foo };",
    {
      "sourceMap": false,
      "declarationMap": false
    }
  ],
  [
    "legacy-invalid-this",
    "function dec(value: any) { return value; } @dec export class Foo { static self: any = this; }",
    {
      "experimentalDecorators": true
    }
  ],
  [
    "legacy-bound-this",
    "function dec(value: any) { return value; } @dec export default class { static self: any = this; }",
    {
      "experimentalDecorators": true
    }
  ],
  [
    "legacy-static-block",
    "function dec(value: any) { return value; } @dec export class Foo { static self: any; static { this.self = this; } }",
    {
      "experimentalDecorators": true
    }
  ]
];
const inputs=[];
for (const [targetName,target] of [["es5",ts.ScriptTarget.ES5],["es2015",ts.ScriptTarget.ES2015],["es2022",ts.ScriptTarget.ES2022]]) {
 for (const [moduleName,module] of [["commonjs",ts.ModuleKind.CommonJS],["esnext",ts.ModuleKind.ESNext]]) {
  for (const useDefineForClassFields of [false,true]) for (const [shape,body,extra={}] of shapes) {
   const main="/project/main.ts", mode=useDefineForClassFields?"define":"set";
   inputs.push({case_id:`class-field-alias-map-positions/${targetName}/${moduleName}/${mode}/${shape}`, roots:[main],
    files:[{path:main,text:"interface I { method(): number; }\n"+body+"\nexport const tail = 1;\n"}],
    options:{...defaults,target,module,allowJs:false,checkJs:false,useDefineForClassFields,...extra}});
  }
 }
}
assert.equal(inputs.length,384);
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
  const first = observe(input); assert.deepEqual(observe(input), first, input.case_id);
  return {...input, typescript_observation:first};
});
const artifact = {version:1,typescript:ts.version,source_commit:"050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256:sha256(fs.readFileSync(path.join(root,"vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256:sha256(fs.readFileSync(import.meta.filename)),repetitions:2,cases};
const rendered = JSON.stringify(artifact,null,2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination,rendered);
else assert.equal(fs.readFileSync(destination,"utf8"),rendered);
console.log(`Class field alias map positions: ${cases.length} cases, two identical complete observations each`);
