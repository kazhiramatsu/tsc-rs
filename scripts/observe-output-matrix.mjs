// EF8 (H2.8a-A-RES-EMITTER-FINAL): the minimum additional complete-command inputs P1–P8 for the
// output axes no existing control observes (ef8/README.md §4–§5, U1–U10). Expectations are
// TS-produced twice per row. Two fixtures: output-matrix.json (P1–P7, memory sink, the
// h2_7c_declaration_blocking comparator shape) and output-matrix-filesystem.json (P8, the
// output-filesystem fault-injection shape).
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

const root = path.resolve(import.meta.dirname, "..");
const matrixDestination = path.join(root, "crates/compiler/tests/fixtures/output-matrix.json");
const filesystemDestination = path.join(root, "crates/compiler/tests/fixtures/output-matrix-filesystem.json");
const sha256 = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
assert.equal(ts.version, "6.0.3");
assert.ok(["--write", "--check"].includes(process.argv[2]));
const defaults = { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.CommonJS,
  strict: true, skipDefaultLibCheck: true, noErrorTruncation: true, newLine: ts.NewLineKind.CarriageReturnLineFeed };
const inputs = [];
function add(case_id, files, options = {}, extra = {}) {
  inputs.push({ case_id, files: Object.entries(files).map(([name, text]) => ({ path: "/project/" + name, text })),
    options: { ...defaults, ...options }, ...extra });
}

// P1 — U1/U2: the three targets with no complete-command tuple, under two module families.
const targetSensitive = {
  "src/a.ts": [
    "export class C {",
    "    #p = 1;",
    "    static s = 2;",
    "    static { C.s = 3; }",
    "    accessor v = 4;",
    "    get q(): number { return this.#p ?? 0; }",
    "    has(o: object): boolean { return #p in o; }",
    "}",
    "export async function* g(a?: { b?: number[] }): AsyncGenerator<number> {",
    "    for await (const x of [1, 2]) { yield a?.b?.[x] ?? x ** 2; }",
    "}",
    "export const o = { ...{ a: 1 }, b: 2 };",
    "export let n: number | null = null;",
    "n ??= 5;",
    "export function f(x: number = 1, ...rest: number[]): number { return x + rest.length; }",
    "",
  ].join("\n"),
  "src/b.ts": "import { C, o, f } from \"./a\";\nexport const c = new C();\nexport const d = o.b + f(2, 3);\nexport default c;\n",
};
for (const [targetName, target] of [["es2018", ts.ScriptTarget.ES2018], ["es2021", ts.ScriptTarget.ES2021], ["es2023", ts.ScriptTarget.ES2023]]) {
  for (const [moduleName, module] of [["commonjs", ts.ModuleKind.CommonJS], ["esnext", ts.ModuleKind.ESNext]]) {
    add(`target-module/${targetName}/${moduleName}`, targetSensitive, { target, module, declaration: true, outDir: "out" });
  }
}

// P2 — U3 (+ the BOM half of U9): BOM on every map product, both newline kinds, with and without outDir.
const nestedComments = { "src/a.ts": "/** doc */\nexport const a: number = 1; // trailing\n", "src/nested/b.ts": "export const b: string = 'b';\n" };
for (const [layoutName, layout] of [["outDir", { outDir: "out" }], ["neither", {}]]) {
  for (const [newLineName, newLine] of [["crlf", ts.NewLineKind.CarriageReturnLineFeed], ["lf", ts.NewLineKind.LineFeed]]) {
    add(`bom-maps/${layoutName}/${newLineName}`, nestedComments, { ...layout, newLine, emitBOM: true, sourceMap: true, declaration: true, declarationMap: true });
  }
}

// P3 — U4: removeComments with declaration maps under outFile.
const commented = {
  "a.ts": "/** A doc */\nexport class A {\n    /* inner */\n    m(): number { return 1; } // m trailing\n}\n",
  "b.ts": "import { A } from \"./a\";\n/** b doc */\nexport const b: A = new A(); // b trailing\n",
};
for (const [targetName, target] of [["es2015", ts.ScriptTarget.ES2015], ["es2022", ts.ScriptTarget.ES2022]]) {
  add(`remove-comments-outfile-maps/${targetName}`, commented,
    { target, module: ts.ModuleKind.AMD, outFile: "out/bundle.js", removeComments: true, sourceMap: true, declaration: true, declarationMap: true });
}

// P4 — U5 / U10: both layouts set; noEmitOnError skipping an outFile program with maps.
add("layout/outfile-and-outdir", commented,
  { module: ts.ModuleKind.AMD, outFile: "out/bundle.js", outDir: "out", sourceMap: true, declaration: true, declarationMap: true });
add("skip/noemitonerror-outfile-maps", { ...commented, "c.ts": "export const c: number = 'not a number';\n" },
  { module: ts.ModuleKind.AMD, outFile: "out/bundle.js", noEmitOnError: true, sourceMap: true, declaration: true, declarationMap: true });

// P5 — U6: reversed root order under outDir (multi-file, non-bundle), with and without rootDir.
const chain = {
  "src/c.ts": "import { b } from \"./b\";\nexport const c: number = b + 1;\n",
  "src/b.ts": "import { a } from \"./a\";\nexport const b: number = a + 1;\n",
  "src/a.ts": "export const a: number = 1;\n",
};
for (const [rootName, rootDir] of [["rootDir", { rootDir: "/project/src" }], ["none", {}]]) {
  add(`reversed-roots/${rootName}`, chain, { ...rootDir, outDir: "out", declaration: true },
    { roots: ["/project/src/c.ts", "/project/src/b.ts", "/project/src/a.ts"] });
}

// P6 — U7: escaped and non-BMP class/export names flowing into d.ts and d.ts.map under outDir.
const escapedOptions = { module: ts.ModuleKind.CommonJS, outDir: "/project/out", sourceMap: true, declaration: true, declarationMap: true };
add("escaped-declaration-maps/escaped/es5", { "main.ts": "class \\u0046oo { method() { return 1; } }\nexport { Foo as Bar };\n" },
  { ...escapedOptions, target: ts.ScriptTarget.ES5 });
add("escaped-declaration-maps/direct-escaped/es5", { "main.ts": "export class \\u0046oo { method() { return 1; } }\n" },
  { ...escapedOptions, target: ts.ScriptTarget.ES5 });
add("escaped-declaration-maps/nonbmp-class/es2015", { "main.ts": "export class \u{1D49C} { method() { return 1; } }\nexport const \u{1D4B7} = new \u{1D49C}();\n" },
  { ...escapedOptions, target: ts.ScriptTarget.ES2015 });
add("escaped-declaration-maps/nonbmp-export/es2015", { "main.ts": "const \u{1D49E} = 1;\nexport { \u{1D49E} as \u{1D49F} };\n" },
  { ...escapedOptions, target: ts.ScriptTarget.ES2015 });

// P7 — U8: a declarationDir that folds onto an input on a case-insensitive host, and the same
// program on a case-sensitive host.
const folding = { "src/a.ts": "export const a: number = 1;\n", "types/a.d.ts": "export declare const a: number;\n" };
add("case-insensitive-declaration-dir/collision", folding, { declaration: true, declarationDir: "/project/Types", outDir: "/project/out" },
  { use_case_sensitive_file_names: false });
add("case-insensitive-declaration-dir/distinct", folding, { declaration: true, declarationDir: "/project/Types", outDir: "/project/out" },
  { use_case_sensitive_file_names: true });
assert.equal(inputs.length, 22);

// P8 — U9: multi-product write sequences under the pinned directory/retry worker with faults.
const filesystemInputs = [];
for (const outDir of ["out/deep", "/project/new/deep"]) {
  for (const fault of ["first-write", "create-directory"]) {
    filesystemInputs.push({ case_id: `products/${outDir}/${fault}`, current_directory: "/project", fault,
      initial_directories: ["/", "/project", "/project/src"],
      files: [{ path: "/project/src/a.ts", text: "// before\nexport const a: number = 1;\n" },
        { path: "/project/src/b.ts", text: "export const b: string = 'b';\n" }],
      options: { ...defaults, outDir, emitBOM: true, listEmittedFiles: true, declaration: true, sourceMap: true, declarationMap: true } });
  }
}
assert.equal(filesystemInputs.length, 4);

function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null,
    start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    related_information: d.relatedInformation?.map(diagnostic) ?? null };
}
const kindOf = name => name.endsWith(".d.ts.map") ? "declaration-map" : name.endsWith(".map") ? "javascript-map"
  : ts.isDeclarationFileName(name) ? "declaration" : "javascript";
function write(args, index) {
  const [name, text, bom, onError, sources, data] = args;
  const bytes = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), bytes]) : bytes;
  return { index, path: name, kind: kindOf(name), callback_utf8_base64: bytes.toString("base64"), callback_utf8_bytes: bytes.length,
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
const libraryRoot = path.join(root, "vendor/typescript-6.0.3/lib");
const library = name => /^\/lib\/lib(?:\.[a-z0-9.-]+)?\.d\.ts$/i.test(name) && fs.existsSync(path.join(libraryRoot, path.basename(name)));

function observeMatrix(input) {
  const sensitive = input.use_case_sensitive_file_names ?? true;
  const canonical = ts.createGetCanonicalFileName(sensitive);
  const files = new Map(input.files.map(file => [canonical(file.path), file.text]));
  const read = name => files.get(canonical(ts.normalizePath(name))) ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(files.keys(), { currentDirectory: "/project", useCaseSensitiveFileNames: sensitive,
    fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  const host = { ...ts.createCompilerHost(input.options, true), ...overlay,
    getCurrentDirectory: () => "/project", getDefaultLibFileName: options => "/lib/" + ts.getDefaultLibFileName(options),
    getDefaultLibLocation: () => "/lib", useCaseSensitiveFileNames: () => sensitive, getCanonicalFileName: canonical,
    readFile: read, fileExists: name => files.has(canonical(ts.normalizePath(name))) || library(name),
    getSourceFile: (name, options) => { const text = read(name); return text === undefined ? undefined : ts.createSourceFile(name, text, options, true); },
    writeFile: () => assert.fail("unexpected host write") };
  const roots = input.roots ?? input.files.map(file => file.path);
  const program = ts.createProgram({ rootNames: roots, options: input.options, host });
  const writes = [], reported = [], status = [];
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)), s => status.push(s), undefined,
    (...args) => writes.push(write(args, writes.length)));
  assert.ok(result);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic),
      emitted_files: result.emittedFiles ?? null, source_maps: sourceMaps(result.sourceMaps) },
    status_writes: status, exit_code: exit };
}

function observeFilesystem(input) {
  const files = new Map(input.files.map(file => [file.path, file.text]));
  const read = name => files.get(ts.normalizePath(name)) ?? (library(name) ? fs.readFileSync(path.join(libraryRoot, path.basename(name)), "utf8") : undefined);
  const overlay = createHermeticDirectoryOverlay(files.keys(), { currentDirectory: input.current_directory,
    useCaseSensitiveFileNames: true, fallbackHost: { directoryExists: name => name === "/lib", getDirectories: () => [] } });
  const host = { ...ts.createCompilerHost(input.options, true), ...overlay,
    getCurrentDirectory: () => input.current_directory, getDefaultLibFileName: options => "/lib/" + ts.getDefaultLibFileName(options),
    getDefaultLibLocation: () => "/lib", useCaseSensitiveFileNames: () => true, getCanonicalFileName: name => name,
    readFile: read, fileExists: name => files.has(ts.normalizePath(name)) || library(name),
    getSourceFile: (name, options) => { const text = read(name); return text === undefined ? undefined : ts.createSourceFile(name, text, options, true); },
    writeFile: () => assert.fail("command callback owns filesystem transport") };
  const program = ts.createProgram({ rootNames: input.files.map(file => file.path), options: input.options, host });
  const absolute = name => ts.getNormalizedAbsolutePath(name, input.current_directory);
  const directories = new Set(input.initial_directories), materialized = new Map(), attempts = new Map();
  const operations = [], writes = [], reported = [], status = [];
  function rawWrite(name, text, bom) {
    const attempt = (attempts.get(name) ?? 0) + 1; attempts.set(name, attempt);
    const bytes = Buffer.from((bom ? "﻿" : "") + text);
    const error = input.fault === "all-writes" ? "H2.8 controlled write failure"
      : input.fault === "first-write" && attempt === 1 ? "H2.8 discarded first write failure"
      : !directories.has(ts.getDirectoryPath(absolute(name))) ? "H2.8 parent directory is missing" : null;
    operations.push({ operation: "write-file", path: name, utf8_base64: bytes.toString("base64"), error });
    if (error !== null) throw new Error(error);
    materialized.set(absolute(name), bytes);
  }
  function directoryExists(name) {
    const exists = directories.has(absolute(name));
    operations.push({ operation: "directory-exists", path: name, exists });
    return exists;
  }
  function createDirectory(name) {
    const error = input.fault === "create-directory" ? "H2.8 controlled create failure" : null;
    operations.push({ operation: "create-directory", path: name, error });
    if (error !== null) throw new Error(error);
    directories.add(absolute(name));
  }
  const sink = (...args) => {
    writes.push(write(args, writes.length));
    try { ts.writeFileEnsuringDirectories(args[0], args[1], args[2], rawWrite, createDirectory, directoryExists); }
    catch (error) { assert.ok(args[3]); args[3](error.message); }
  };
  let result;
  const emit = program.emit.bind(program);
  program.emit = (...args) => { assert.equal(result, undefined); return result = emit(...args); };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, d => reported.push(diagnostic(d)), s => status.push(s), undefined, sink);
  assert.ok(result);
  return { writes, reported_diagnostics: reported, emit_refused: result.emitSkipped,
    emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: sourceMaps(result.sourceMaps) },
    status_writes: status, exit_code: exit, filesystem: { operations,
      materialized_files: [...materialized].map(([name, bytes]) => ({ path: name, utf8_base64: bytes.toString("base64") })),
      directories: [...directories] } };
}

function mint(list, observe) {
  return list.map(input => {
    const first = observe(input);
    assert.deepEqual(observe(input), first, input.case_id);
    return { ...input, typescript_observation: first };
  });
}
const common = { version: 1, typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
  compiler_sha256: sha256(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))),
  observer_sha256: sha256(fs.readFileSync(import.meta.filename)), repetitions: 2 };
const matrix = { ...common, cases: mint(inputs, observeMatrix) };
const filesystem = { ...common, cases: mint(filesystemInputs, observeFilesystem) };
for (const [destination, artifact] of [[matrixDestination, matrix], [filesystemDestination, filesystem]]) {
  const rendered = JSON.stringify(artifact, null, 2) + "\n";
  assert.ok(!rendered.includes(root), "local workspace path escaped into artifact");
  if (process.argv[2] === "--write") fs.writeFileSync(destination, rendered);
  else assert.equal(fs.readFileSync(destination, "utf8"), rendered, destination);
}
for (const row of matrix.cases) console.log(`${row.case_id}: writes=${row.typescript_observation.writes.length} exit=${row.typescript_observation.exit_code} skipped=${row.typescript_observation.emit_result.emit_skipped} diags=${row.typescript_observation.reported_diagnostics.map(d => d.code).join(",")}`);
for (const row of filesystem.cases) console.log(`${row.case_id}: writes=${row.typescript_observation.writes.length} exit=${row.typescript_observation.exit_code} ops=${row.typescript_observation.filesystem.operations.length}`);
console.log(`output matrix: ${matrix.cases.length} + ${filesystem.cases.length} cases, two identical complete observations each`);
