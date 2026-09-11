// H2.8a local package output-to-input mapping; fresh complete TS Programs.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const destination = path.join(root, "crates/compiler/tests/fixtures/package-output-inputs.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const defaults = { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.NodeNext,
  declaration: true, emitDeclarationOnly: true, skipDefaultLibCheck: true,
  noErrorTruncation: true, newLine: ts.NewLineKind.CarriageReturnLineFeed };
const inputs = [];
function add(case_id, settings = {}) {
  const { map = "exports", extension = "ts", output = "d.ts", target = "./types/src/value." + output,
    source = true, configured = false, package_root = "/project", entry = map === "imports" ? "#value" : "local",
    extra_files = {}, package_fields = {}, rootDir = "/project", options = {}, config_path = "/project/tsconfig.json",
    ...extra } = settings;
  const rootName = "/project/test/main.ts";
  const files = {
    [rootName]: `import { value } from '${entry}'; export const main = value;\n`,
    [package_root + "/package.json"]: JSON.stringify({ name: "local", version: "1.0.0", type: "module",
      [map]: map === "imports" ? { "#value": target } : { ".": target }, ...package_fields }),
    [package_root + target.slice(1)]: "export declare const value: number;\n",
    ...(source ? { ["/project/src/value." + extension]: ["js", "jsx", "mjs", "cjs"].includes(extension)
      ? "export const value = 1;\n" : "export const value: number = 1;\n" } : {}),
    ...extra_files,
  };
  const actualOptions = { ...defaults, rootDir: Object.hasOwn(settings, "rootDir") ? settings.rootDir : rootDir, declarationDir: "/project/types", ...options };
  let config;
  if (configured) {
    config = JSON.stringify({ compilerOptions: { ...actualOptions, target: "esnext", module: "nodenext", newLine: "crlf" }, files: [rootName] });
  }
  inputs.push({ case_id, files: Object.entries(files).map(([path, text]) => ({path, text})),
    roots: [rootName], options: configured ? defaults : actualOptions,
    ...(config ? { config, config_path } : {}), ...extra });
}
for (const map of ["exports", "imports"]) {
  add(`${map}/explicit`, { map });
  add(`${map}/ambiguous-success`, { map, rootDir: undefined });
  add(`${map}/ambiguous-fallback`, { map, rootDir: undefined, source: false });
  add(`${map}/ambiguous-blocked`, { map, rootDir: undefined, options: {noEmitOnError: true} });
  add(`${map}/config-default`, { map, rootDir: undefined, configured: true });
  add(`${map}/config-path-only`, { map, rootDir: undefined, config_file_path: "/project/tsconfig.json" });
  add(`${map}/missing-input`, { map, source: false });
}
for (const extension of ["tsx", "js", "jsx", "mts", "mjs", "cts", "cjs"]) {
  const output = ["mts", "mjs"].includes(extension) ? "d.mts" : ["cts", "cjs"].includes(extension) ? "d.cts" : "d.ts";
  add(`extension/${extension}`, { extension, output, options: {allowJs: true} });
}
add("extension/tsx-priority", { extra_files: {"/project/src/value.tsx": "export const value: string = 'tsx';\n"} });
add("directory/javascript-output", { target: "./out/src/value.js", options: {declarationDir: undefined, outDir: "/project/out"} });
add("directory/declaration-before-javascript", { target: "./types/src/value.d.ts", options: {outDir: "/project/types/src"},
  extra_files: {"/project/value.ts": "export const value: string = 'outer';\n"} });
add("directory/javascript-after-declaration-miss", { target: "./out/src/value.js", options: {outDir: "/project/out"} });
add("directory/relative-without-config", { rootDir: "/project/src", target: "./src/types/value.d.ts", options: {declarationDir: "types"} });
add("directory/relative-outside-common", { rootDir: "/project/src", target: "./types/value.d.ts", options: {declarationDir: "types"} });
add("directory/same-output-options", { options: {outDir: "/project/types"} });
add("directory/no-output-options", { options: {declarationDir: undefined} });
add("directory/empty-output-options", { options: {declarationDir: "", outDir: ""} });
add("directory/component-boundary", { target: "./types-other/src/value.d.ts" });
add("directory/case-insensitive", { target: "./TYPES/src/value.d.ts", use_case_sensitive_file_names: false });
add("directory/trailing-slash", { options: {declarationDir: "/project/types/"} });
add("scope/config-outside", { configured: true, config_path: "/else/tsconfig.json" });
add("scope/node-modules", { entry: "local", package_root: "/project/node_modules/local" });
add("imports/bare-alias-drops-inner-diagnostic", { map: "imports", entry: "#alias", rootDir: undefined,
  package_fields: {imports: {"#alias": "#value", "#value": "./types/src/value.d.ts"}} });
add("imports/bare-self-name-drops-inner-diagnostic", { map: "exports", entry: "#alias", rootDir: undefined,
  package_fields: {imports: {"#alias": "local"}} });
add("imports/bare-fallback-restores-request", { map: "imports", entry: "#alias", rootDir: undefined,
  package_fields: {imports: {"#alias": ["#missing", "./types/src/value.d.ts"]}} });
add("exports/ambiguous-miss", { rootDir: undefined, source: false });
inputs.at(-1).files = inputs.at(-1).files.filter(file => !file.path.endsWith("/value.d.ts"));
add("extension/javascript-not-loaded", { extension: "js" });
add("extension/no-dts-resolution", { options: {noDtsResolution: true} });
add("extension/json-target", { target: "./out/src/value.json", options: {outDir: "/project/out", resolveJsonModule: true} });
for (const rootDir of ["/project", undefined]) {
  const declarationDir = "/project/typeRoots/local/types";
  const rootName = "/project/test/main.ts";
  const case_id = `type-reference/${rootDir ?? "ambiguous"}`;
  inputs.push({case_id, options: {...defaults, rootDir, declarationDir, typeRoots: ["/project/typeRoots"], types: []}, roots: [rootName],
    files: [
      {path: rootName, text: '/// <reference types="local" />\nexport const main: number = 1;\n'},
      {path: "/project/package.json", text: '{"type":"module"}'},
      {path: "/project/typeRoots/local/package.json", text: '{"name":"local","version":"1.0.0","exports":{".":"./types/src/value.d.ts"}}'},
      {path: "/project/typeRoots/local/types/src/value.d.ts", text: "export declare const value: number;\n"},
      {path: "/project/src/value.ts", text: "export const value: number = 1;\n"},
    ]});
}

for (const rootDir of ["/project", undefined]) {
  const base = inputs.find(input => input.case_id === `type-reference/${rootDir ?? "ambiguous"}`);
  const next = structuredClone(base);
  next.case_id += "/node-modules-secondary";
  next.options.typeRoots = [];
  next.options.declarationDir = "/project/node_modules/local/types";
  for (const file of next.files) file.path = file.path.replace("/typeRoots/", "/node_modules/");
  inputs.push(next);
}
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
  let options = { ...input.options, ...(input.config_file_path ? {configFilePath: input.config_file_path} : {}) }, roots = input.roots ?? input.files.map(file => file.path), errors = [];
  if (input.config) {
    const configPath = input.config_path ?? "/project/tsconfig.json";
    const parsed = ts.parseJsonSourceFileConfigFileContent(ts.parseJsonText(configPath, input.config),
      { ...host, readDirectory: () => roots }, ts.getDirectoryPath(configPath), undefined, configPath);
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
  const loaded_files = program.getSourceFiles().filter(source => !program.isSourceFileDefaultLibrary(source)).map(source => source.fileName);
  const resolutions = [];
  for (const source of program.getSourceFiles()) {
    if (program.isSourceFileDefaultLibrary(source)) continue;
    for (const request of source.imports ?? []) {
      const mode = program.getModeForUsageLocation(source, request);
      const result = program.getResolvedModule(source, request.text, mode);
      resolutions.push({ source: source.fileName, specifier: request.text, mode: mode ?? null,
        resolved_file: result?.resolvedModule?.resolvedFileName ?? null,
        extension: result?.resolvedModule?.extension ?? null,
        diagnostics: result?.resolutionDiagnostics?.map(diagnostic) ?? [] });
    }
  }
  const type_resolutions = [];
  for (const source of program.getSourceFiles()) {
    if (program.isSourceFileDefaultLibrary(source)) continue;
    for (const request of source.typeReferenceDirectives) {
      const result = program.getResolvedTypeReferenceDirectiveFromTypeReferenceDirective(request, source);
      type_resolutions.push({ source: source.fileName, specifier: request.fileName,
        resolved_file: result?.resolvedTypeReferenceDirective?.resolvedFileName ?? null,
        primary: result?.resolvedTypeReferenceDirective?.primary ?? null,
        diagnostics: result?.resolutionDiagnostics?.map(diagnostic) ?? [] });
    }
  }
  return { loaded_files, resolutions, type_resolutions, writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: null },
    status_writes: status, exit_code: exit };
}
const observations = inputs.map(input => {
  const first = observe(input); assert.deepEqual(observe(input), first, input.case_id);
  console.log(input.case_id, first.resolutions.map(r => r.resolved_file), first.reported_diagnostics.map(d => d.code));
  return { ...input, typescript_observation: first };
});
const artifact = {version: 1, typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), repetitions: 2, cases: observations};
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered);
else assert.equal(fs.readFileSync(destination, "utf8"), rendered);
console.log(`package output inputs: ${observations.length} cases, two identical complete observations each`);
