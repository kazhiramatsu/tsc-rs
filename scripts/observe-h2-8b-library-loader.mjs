// Re-observe the existing config-anchored loader contract without emitting.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";
const root = path.resolve(import.meta.dirname, "..");
const sha = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const replacement = "/somepath/node_modules/@typescript/lib-dom/iterable.d.ts";
const input = {case_id: "config-directory-package-subpaths", current_directory: "/workspace",
  roots: ["/somepath/index.ts"], options: {target: 2, noEmit: true, types: [], libReplacement: true, configFilePath: "/somepath/tsconfig.json"},
  library_directory: "/typescript/lib", files: [
    {path: "/somepath/index.ts", text: '/// <reference lib="dom.iterable" />\nconst value: DOMIterable = {};\n'},
    {path: replacement, text: "interface DOMIterable {}\n"},
    {path: "/somepath/node_modules/@typescript/lib-dom/index.d.ts", text: "// replacement DOM\n"},
    {path: "/typescript/lib/lib.dom.iterable.d.ts", text: "interface CatalogDOMIterable { fallback: true }\n"},
    {path: "/typescript/lib/lib.es6.d.ts", text: '/// <reference lib="dom" />\n/// <reference lib="dom.iterable" />\n'}
  ]};
function observe() {
  const files = new Map(input.files.map(f => [f.path, f.text]));
  const host = {...ts.createCompilerHost(input.options, true),
    ...createHermeticDirectoryOverlay(files.keys(), {currentDirectory: input.current_directory, useCaseSensitiveFileNames: true}),
    getCurrentDirectory: () => input.current_directory, useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: name => name, getDefaultLibLocation: () => input.library_directory,
    getDefaultLibFileName: options => `${input.library_directory}/${ts.getDefaultLibFileName(options)}`,
    readFile: name => files.get(name), fileExists: name => files.has(name),
    getSourceFile: (name, options) => files.has(name) ? ts.createSourceFile(name, files.get(name), options, true) : undefined};
  const program = ts.createProgram({rootNames: input.roots, options: input.options, host});
  return {source_files: program.getSourceFiles().map(f => f.fileName), library_files: program.getSourceFiles().filter(f => program.isSourceFileDefaultLibrary(f)).map(f => f.fileName), root_names: program.getRootFileNames()};
}
const first = observe(); assert.deepEqual(observe(), first);
const artifact = {version: 1, typescript: ts.version, source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8", repetitions: 2,
  compiler_sha256: sha(fs.readFileSync(path.join(root, "vendor/typescript-6.0.3/lib/typescript.js"))), observer_sha256: sha(fs.readFileSync(import.meta.filename)), input, typescript_observation: first};
const destination = path.join(root, "crates/program/tests/fixtures/h2-8b-library-loader-order.json");
assert.ok(["--write", "--check"].includes(process.argv[2]));
if (process.argv[2] === "--write") { assert.ok(!fs.existsSync(destination)); fs.writeFileSync(destination, JSON.stringify(artifact, null, 2) + "\n"); }
else assert.deepEqual(JSON.parse(fs.readFileSync(destination, "utf8")), artifact);
console.log(JSON.stringify({observation: first, repetitions: 2, sha256: sha(fs.readFileSync(destination))}));
