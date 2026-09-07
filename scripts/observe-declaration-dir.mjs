// Focused H2.7c observations; the historical declaration admission band is unchanged.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";

const root = path.resolve(import.meta.dirname, "..");
const target = path.join(root, "crates/compiler/tests/fixtures/declaration-dir.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const defaults = { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.CommonJS, declaration: true, strict: true,
  newLine: ts.NewLineKind.CarriageReturnLineFeed, skipDefaultLibCheck: true, noErrorTruncation: true };
const inputs = [];
function add(case_id, files, options, extra = {}) {
  inputs.push({ case_id, files: Object.entries(files).map(([name, text]) => ({ path: path.isAbsolute(name) ? name : "/project/" + name, text })),
    options: { ...defaults, ...options }, ...extra });
}
function official(name, hash, options) {
  const raw = fs.readFileSync(path.join(root, "ts-tests/tests/cases/compiler", name));
  assert.equal(sha256(raw), hash, name);
  const files = {};
  let filename = name, lines = [];
  const flush = () => {
    const text = lines.join("\n").replace(/^\n+/, "");
    if (text.trim()) files[filename] = text;
    lines = [];
  };
  for (const line of raw.toString().split(/\r?\n/)) {
    const match = line.match(/^\/\/\s*@filename:\s*(.*)/i);
    if (match) { flush(); filename = match[1].trim(); }
    else if (!/^\/\/\s*@\w+\s*:/.test(line)) lines.push(line);
  }
  flush();
  add("compiler/" + name, files, options, { source_sha256: hash });
}

function officialConfig(name, expectedHash) {
  const raw = fs.readFileSync(path.join(root, "ts-tests/tests/cases/compiler", name));
  assert.equal(sha256(raw), expectedHash, name);
  const files = {};
  let filename, lines = [];
  const flush = () => {
    if (filename) files[filename] = lines.join("\n").replace(/^\n+/, "");
    lines = [];
  };
  for (const line of raw.toString().split(/\r?\n/)) {
    const match = line.match(/^\/\/\s*@filename:\s*(.*)/i);
    if (match) { flush(); filename = match[1].trim(); }
    else if (!/^\/\/\s*@\w+\s*:/.test(line)) lines.push(line);
  }
  flush();
  const config_path = Object.keys(files).find(name => name.endsWith("tsconfig.json"));
  assert.ok(config_path);
  const original_config = files[config_path]; delete files[config_path];
  add("compiler/" + name + "#original", files, {}, { config: original_config, config_path, source_sha256: expectedHash });
  const config = JSON.parse(original_config); config.compilerOptions.noEmitOnError = true;
  add("compiler/" + name + "#no-emit-on-error", files, {}, { config: JSON.stringify(config, null, 2) + "\n", config_path, original_config, source_sha256: expectedHash });
}
officialConfig("declarationEmitToDeclarationDirWithDeclarationOption.ts", "713ebabb6f73997e0891b999f637a27bfba093bb7e865f190466128c76abb543");
officialConfig("declarationEmitToDeclarationDirWithoutCompositeAndDeclarationOptions.ts", "d2c79337a36e8401b19e67a125ac77f580255d13a206eb51edf9d0a28b20950f");
const nested = { "src/a.ts": "export const a: number = 1;\n", "src/nested/b.ts": "export const b: string = 'b';\n" };
for (const [label, options] of [
  ["declaration-dir", { declarationDir: "/project/types" }],
  ["separate-out-dir", { declarationDir: "/project/types", outDir: "/project/js" }],
  ["declaration-only", { declarationDir: "/project/types", emitDeclarationOnly: true }],
  ["blocking-enabled", { declarationDir: "/project/types", noEmitOnError: true }],
  ["disabled-dir", { outDir: "/project/js" }],
]) add("adjacent/nested#" + label, nested, options);
for (const noEmitOnError of [false, true]) {
  add("adjacent/missing-declaration#" + noEmitOnError, { "a.ts": "export const a: number = 1;\n" }, { declaration: false, declarationDir: "/project/types", noEmitOnError });
  add("adjacent/declaration-only-input#" + noEmitOnError, { "a.d.ts": "export declare const a: number;\n" }, { declaration: false, declarationDir: "/project/types", noEmitOnError });
  add("adjacent/partial-isolated#" + noEmitOnError, { "good.ts": "export const good: number = 1;\n", "bad.ts": "export const bad = 1 + 1;\n" }, { declarationDir: "/project/types", isolatedDeclarations: true, noEmitOnError });
}
add("adjacent/empty-directory#out-dir-fallback", nested, { declarationDir: "", outDir: "/project/js" });
add("adjacent/empty-directory#without-declaration", { "a.ts": "export const a: number = 1;\n" }, { declaration: false, declarationDir: "" });
for (const noEmitOnError of [false, true]) {
  const config = JSON.stringify({ compilerOptions: { declarationDir: "types", declaration: false, composite: false, emitDeclarationOnly: true, noEmitOnError, target: "es2015", module: "commonjs", skipDefaultLibCheck: true }, files: ["a.ts"] }, null, 2) + "\n";
  add("adjacent/config-option-locations#" + noEmitOnError, { "a.ts": "export const a: number = 1;\n" }, {}, { config });
}
add("adjacent/empty-directory#beside-source", nested, { declarationDir: "" });

// H2.8a owns these outDir combinations. Keep the full TypeScript observation
// beside the explicit Rust boundary, without counting it as compatible emit.
const outDirBoundaryWindows = new Set([
  "adjacent/nested#separate-out-dir",
  "adjacent/nested#disabled-dir",
  "adjacent/empty-directory#out-dir-fallback",
]);
for (const input of inputs) {
  if (outDirBoundaryWindows.has(input.case_id)) input.rust_expected_unsupported_option = "outDir";
}
const windows = inputs;

function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
function write(args, index) {
  const [fileName, text, bom, onError, sourceFiles, data] = args;
  const callback = Buffer.from(text, "utf8");
  const materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), callback]) : callback;
  return { index, path: fileName, kind: fileName.endsWith(".d.ts") ? "declaration" : "javascript",
    callback_utf8_base64: callback.toString("base64"), callback_utf8_bytes: callback.length,
    write_byte_order_mark: bom, materialized_utf8_base64: materialized.toString("base64"),
    materialized_utf8_bytes: materialized.length, on_error_callback_present: onError !== undefined,
    source_files: sourceFiles?.map(f => f.fileName) ?? null, data_present: data !== undefined,
    data_source_map_url_pos: data?.sourceMapUrlPos ?? null,
    data_diagnostics: data?.diagnostics?.map(diagnostic) ?? null };
}
function observe(input) {
  const files = new Map(input.files.map(file => [file.path, file.text]));
  const host = ts.createCompilerHost(input.options, true);
  const readFile = host.readFile.bind(host), fileExists = host.fileExists.bind(host);
  host.readFile = name => files.get(name) ?? readFile(name);
  host.fileExists = name => files.has(name) || fileExists(name);
  const directoryExists = host.directoryExists.bind(host);
  const directories = new Set();
  for (const file of files.keys()) {
    for (let directory = path.dirname(file);; directory = path.dirname(directory)) {
      directories.add(directory);
      if (directory === path.dirname(directory)) break;
    }
  }
  host.directoryExists = name => directories.has(name) || directoryExists(name);
  host.getCurrentDirectory = () => "/project";
  let options = input.options, roots = input.files.map(file => file.path), errors = [];
  if (input.config) {
    const configPath = input.config_path ?? "/project/tsconfig.json";
    const parsed = ts.parseJsonSourceFileConfigFileContent(ts.parseJsonText(configPath, input.config),
      { ...host, readDirectory: () => roots }, path.dirname(configPath), undefined, configPath);
    options = parsed.options; roots = parsed.fileNames; errors = parsed.errors;
  }
  const program = ts.createProgram({ rootNames: roots, options, host, configFileParsingDiagnostics: errors });
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program,
    d => reported.push(diagnostic(d)), s => status.push(s), undefined,
    (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  assert.ok(result.sourceMaps === undefined || result.sourceMaps.length === 0, "this packet only observes suppressed map output");
  assert.deepEqual(status, []);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps ?? null },
    status_writes: status, exit_code: exit };
}
const cases = windows.map(input => {
  const first = observe(input);
  assert.deepEqual(observe(input), first, input.case_id);
  return { ...input, typescript_observation: first };
});
const artifact = { version: 1, typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))), repetitions: 2, cases };
const rendered = JSON.stringify(artifact, null, 2) + "\n";
if (process.argv[2] === "--write") fs.writeFileSync(target, rendered);
else {
  assert.ok(process.argv[2] === undefined || process.argv[2] === "--check");
  assert.equal(fs.readFileSync(target, "utf8"), rendered);
}
console.log(`declaration directory: ${cases.length} cases, 2 identical observations each`);
