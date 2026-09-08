// H2.8a root/common-source-directory witnesses. Expectations are TS-produced.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/output-roots.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const defaults = { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.CommonJS,
  strict: true, skipDefaultLibCheck: true, noErrorTruncation: true, newLine: ts.NewLineKind.CarriageReturnLineFeed };
const inputs = [];
const nested = { "src/a.ts": "export const a: number = 1;\n", "src/nested/b.ts": "export const b: string = 'b';\n" };
function add(case_id, files, options = {}, extra = {}) {
  inputs.push({ case_id, files: Object.entries(files).map(([name, text]) => ({ path: "/project/" + name, text })), options: { ...defaults, ...options }, ...extra });
}
for (const rootDir of ["/project", "/project/src", "src", "./src/../src", "", "/project/other", "/project/sr", "/project/sr/"]) {
  for (const noEmitOnError of [false, true]) add(`root/${rootDir}/${noEmitOnError}`, nested, { rootDir, outDir: "out", declaration: true, noEmitOnError });
}
add("root/ignored-declaration", { ...nested, "external/a.d.ts": "export declare const other: number;\n" }, { rootDir: "src", outDir: "out" });
add("root/no-output-directory", nested, { rootDir: "other" });
add("root/empty-program", {}, { rootDir: "src", outDir: "out" });
for (const rootDir of [undefined, "src", ".", "other"]) for (const noEmitOnError of [false, true]) {
  const config = JSON.stringify({ compilerOptions: { target: "esnext", module: "commonjs", declaration: true,
    outDir: "out", rootDir, noEmitOnError, strict: true, skipDefaultLibCheck: true, noErrorTruncation: true, newLine: "crlf" },
    files: Object.keys(nested) }, null, 2) + "\n";
  add(`config/${rootDir}/${noEmitOnError}`, nested, {}, { config });
}
const baseCount = inputs.length;
assert.equal(baseCount, 27);
for (const kind of ["import", "reference"]) for (const alsoRoot of [false, true]) {
  const main = kind === "import" ? "import { b } from '../external/b'; export const a: number = b;\n"
    : "/// <reference path='../external/b.ts' />\nexport const a: number = 1;\n";
  add(`reason/${kind}/${alsoRoot}`, { "src/a.ts": main, "external/b.ts": "export const b: number = 1;\n" },
    { rootDir: "src", outDir: "out" }, { roots: alsoRoot ? ["/project/src/a.ts", "/project/external/b.ts"] : ["/project/src/a.ts"] });
}
add("reason/two-imports", {
  "src/a.ts": "import { b } from '../external/b'; export const a = b;\n",
  "src/c.ts": "import { b } from '../external/b'; export const c = b;\n",
  "external/b.ts": "export const b: number = 1;\n",
}, { rootDir: "src", outDir: "out" }, { roots: ["/project/src/a.ts", "/project/src/c.ts"] });
for (const noEmitForJsFiles of [false, true]) for (const extension of ["js", "json"]) {
  add(`eligibility/${extension}/${noEmitForJsFiles}`, {
    "src/a.ts": nested["src/a.ts"], ["external/b." + extension]: extension === "js" ? "exports.b = 1;\n" : '{"b":1}\n',
  }, { rootDir: "src", outDir: "out", noEmitForJsFiles, allowJs: true, resolveJsonModule: true });
}
for (const rootDir of [undefined, "src", "SRC"]) {
  add(`case-fold/${rootDir}`, { "src/a.ts": nested["src/a.ts"], "src2/b.ts": nested["src/nested/b.ts"] },
    { rootDir, outDir: "out" }, { use_case_sensitive_file_names: false });
}
add("case-fold/protected-unicode", { "İ/a.ts": nested["src/a.ts"], "i̇/b.ts": nested["src/nested/b.ts"] },
  { outDir: "out" }, { use_case_sensitive_file_names: false });

function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [name, text, bom, onError, sources, data] = args;
  const bytes = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), bytes]) : bytes;
  return { index, path: name, kind: ts.isDeclarationFileName(name) ? "declaration" : "javascript",
    callback_utf8_base64: bytes.toString("base64"), callback_utf8_bytes: bytes.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"), materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined, source_files: sources?.map(source => source.fileName) ?? null,
    data_present: data !== undefined, data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null };
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
  assert.equal(result.sourceMaps, undefined);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: null },
    status_writes: status, exit_code: exit };
}
const observations = inputs.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  return { ...input, typescript_observation: first };
});
const cases = observations.slice(0, baseCount), supplemental_cases = observations.slice(baseCount);
assert.equal(sha256(JSON.stringify(cases)), "3a7b08b0e6e373f9071cbb1a962ab0cd78d6b7d4c78fb386c5b8ec21585d728b", "original 27 cases stay frozen");
const artifact = { version: 1, typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), repetitions: 2, cases, supplemental_cases };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered);
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`output roots: ${cases.length} original + ${supplemental_cases.length} supplemental cases, two identical complete observations each`);
