// Independent option-projection witnesses; never rewrites qualification inputs.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const workspace = fileURLToPath(new URL("../../", import.meta.url));
const require = createRequire(import.meta.url);
const ts = require(path.join(workspace, "vendor/typescript-6.0.3/lib/typescript.js"));
assert.equal(ts.version, "6.0.3");
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const bytes = (text) => ({ utf8_base64: Buffer.from(text).toString("base64"), utf8_sha256: hash(text), utf8_bytes: Buffer.byteLength(text) });
const mapNames = ["sourceMap", "inlineSourceMap", "inlineSources", "sourceRoot", "mapRoot"];
const fixturePath = path.join(workspace, "crates/compiler/tests/fixtures/h2-6a-map-option-projection.json");
const inventoryPath = path.join(workspace, "docs/design/greenfield/slices/h2-6a-map-option-inventory.json");
const qualificationPath = "ratchets/h2-6a-qualification.v1.json";
const qualificationBytes = fs.readFileSync(path.join(workspace, qualificationPath));
const qualification = JSON.parse(qualificationBytes);

function inventory() {
  const cases = qualification.cases.map((row) => ({
    case_id: row.case_id,
    execution_route: row.execution_route,
    disposition: row.disposition,
    directive_map_options: Object.fromEntries(row.input.settings.filter(({ name }) => mapNames.some((key) => key.toLowerCase() === name.toLowerCase())).map(({ name, value }) => [name.toLowerCase(), value])),
    virtual_config: row.input.virtual_config,
    frozen_typescript_runs: row.typescript_run_fingerprints,
  }));
  assert.equal(cases.length, 177);
  assert.equal(cases.filter((row) => row.virtual_config !== null).length, 0);
  const extra = cases.filter((row) => Object.keys(row.directive_map_options).some((key) => key !== "sourcemap"));
  assert.equal(extra.length, 3);
  return { schema: 1, purpose: "Input inventory; no native parity claim", qualification: qualificationPath, qualification_sha256: hash(qualificationBytes), count: cases.length, additional_map_option_rows: extra.map((row) => row.case_id), cases };
}

const variants = [
  ["external", { sourceMap: true }],
  ["external-sources", { sourceMap: true, inlineSources: true }],
  ["relative-map-root", { sourceMap: true, inlineSources: true, mapRoot: "local" }],
  ["relative-source-root", { sourceMap: true, inlineSources: true, sourceRoot: "local" }],
  ["empty-roots", { sourceMap: true, inlineSources: true, mapRoot: "", sourceRoot: "" }],
  ["false-inline-sources", { sourceMap: true, inlineSources: false }],
  ["false-inline-map", { sourceMap: true, inlineSourceMap: false }],
  ["all-false", { sourceMap: false, inlineSourceMap: false, inlineSources: false, mapRoot: "", sourceRoot: "" }],
  ["inline", { sourceMap: false, inlineSourceMap: true, inlineSources: true }],
  ["inline-empty-roots", { inlineSourceMap: true, inlineSources: true, mapRoot: "", sourceRoot: "" }],
  ["conflicting-maps", { sourceMap: true, inlineSourceMap: true }],
  ["sources-without-map", { inlineSources: true }],
  ["map-root-without-map", { mapRoot: "local" }],
  ["source-root-without-map", { sourceRoot: "local" }],
  ["map-root-with-inline", { inlineSourceMap: true, mapRoot: "local" }],
];

function diagnostic(d) {
  return { code: d.code, category: ts.DiagnosticCategory[d.category], file: d.file?.fileName ?? null, start: d.start ?? null, length: d.length ?? null, message: ts.flattenDiagnosticMessageText(d.messageText, "\n") };
}
function observe(input) {
  const files = new Map(input.files.map((file) => [file.path, Buffer.from(file.utf8_base64, "base64").toString("utf8")]));
  if (input.virtual_config) files.set(input.virtual_config.path, Buffer.from(input.virtual_config.utf8_base64, "base64").toString("utf8"));
  const cwd = input.current_directory;
  const normalize = (name) => ts.getNormalizedAbsolutePath(name, cwd);
  const readFile = (name) => files.get(normalize(name)) ?? ts.sys.readFile(name);
  const fileExists = (name) => files.has(normalize(name)) || ts.sys.fileExists(name);
  let options = { noResolve: false };
  if (input.virtual_config) {
    const configPath = input.virtual_config.path;
    const config = ts.parseJsonText(configPath, readFile(configPath));
    const parsed = ts.parseJsonSourceFileConfigFileContent(config, { useCaseSensitiveFileNames: true, readFile, fileExists, readDirectory: () => input.roots }, path.posix.dirname(configPath), undefined, configPath);
    assert.deepEqual(parsed.errors, []);
    options = parsed.options;
  }
  Object.assign(options, { newLine: ts.NewLineKind.CarriageReturnLineFeed, noErrorTruncation: true, skipDefaultLibCheck: true });
  for (const { name, value } of input.settings) {
    const option = ts.optionDeclarations.find((option) => option.name.toLowerCase() === name.toLowerCase());
    assert.ok(option, name);
    const errors = [];
    options[option.name] = option.type === "boolean" ? value.toLowerCase() === "true" : option.type === "string" ? value : ts.parseCustomTypeOption(option, value, errors);
    assert.deepEqual(errors, []);
  }
  const base = ts.createCompilerHost(options, true);
  const host = { ...base, getCurrentDirectory: () => cwd, getCanonicalFileName: (name) => name, useCaseSensitiveFileNames: () => true, readFile, fileExists,
    directoryExists: (dir) => [...files.keys()].some((name) => name.startsWith(`${normalize(dir).replace(/\/$/, "")}/`)) || base.directoryExists(dir),
    getSourceFile(name, version) { const text = files.get(normalize(name)); return text === undefined ? base.getSourceFile(name, version) : ts.createSourceFile(name, text, version, true); },
  };
  const program = ts.createProgram(input.roots, options, host);
  const writes = [], reported = [], status = [];
  let result;
  const original = program.emit;
  program.emit = function (...args) { result = original.apply(this, args); return result; };
  const exit = ts.emitFilesAndReportErrorsAndGetExitStatus(program, (d) => reported.push(diagnostic(d)), (text) => status.push(text), undefined,
    (name, text, bom, onError, sourceFiles, data) => {
      const callback = Buffer.from(text), materialized = bom ? Buffer.concat([Buffer.from([239, 187, 191]), callback]) : callback;
      writes.push({ index: writes.length, path: name, kind: name.endsWith(".map") ? "source-map" : "javascript", callback_utf8_base64: callback.toString("base64"), callback_utf8_sha256: hash(callback), callback_utf8_bytes: callback.length, write_byte_order_mark: bom, materialized_utf8_sha256: hash(materialized), materialized_utf8_bytes: materialized.length, on_error_callback_present: onError !== undefined, source_files: (sourceFiles ?? []).map((source) => source.fileName), data_present: data !== undefined, data_source_map_url_pos: data?.sourceMapUrlPos ?? null, data_diagnostics_count: data?.diagnostics?.length ?? null });
    });
  return { effective_map_options: Object.fromEntries(mapNames.map((name) => [name, options[name] ?? null])), observation: { writes, reported_diagnostics: reported, emit_result: { emit_skipped: result.emitSkipped, diagnostics: result.diagnostics.map(diagnostic), emitted_files: result.emittedFiles ?? null, source_maps: result.sourceMaps?.map((entry) => ({ input_source_file_names: entry.inputSourceFileNames, source_map_json: JSON.stringify(entry.sourceMap) })) ?? null }, status_writes: status, exit_code: exit } };
}

function build() {
  const cases = [];
  for (const route of ["directives", "virtual-config"]) {
    for (const [slug, mapOptions] of variants) {
      const config = route === "virtual-config" ? { compilerOptions: { target: "es2015", ...mapOptions }, files: ["input.ts"] } : null;
      const input = { current_directory: "/.src", roots: ["/.src/input.ts"], files: [{ path: "/.src/input.ts", ...bytes("var a = 10;") }], settings: config ? [] : Object.entries({ target: "es2015", ...mapOptions }).map(([name, value]) => ({ name, value: String(value) })), virtual_config: config ? { path: "/.src/tsconfig.json", ...bytes(JSON.stringify(config, null, 2)) } : null };
      const first = observe(input), second = observe(input);
      assert.deepEqual(first, second);
      cases.push({ case_id: `${route}/${slug}`, route, input, repetitions: 2, ...first });
    }
  }
  // A higher directive layer must retain explicit false and empty strings.
  const inherited = structuredClone(cases.find((row) => row.case_id === "virtual-config/relative-map-root").input);
  inherited.settings = [{ name: "inlineSources", value: "false" }, { name: "mapRoot", value: "" }, { name: "sourceRoot", value: "" }];
  const first = observe(inherited);
  assert.deepEqual(first, observe(inherited));
  cases.push({ case_id: "virtual-config/directive-overrides", route: "virtual-config", input: inherited, repetitions: 2, ...first });
  return { schema: 1, typescript: ts.version, generator_sha256: hash(fs.readFileSync(fileURLToPath(import.meta.url))), typescript_sha256: hash(fs.readFileSync(path.join(workspace, "vendor/typescript-6.0.3/lib/typescript.js"))), cases };
}
const mode = process.argv[2];
assert.ok(["--write", "--check", "--inventory"].includes(mode), "usage: --write | --check | --inventory");
const render = (value) => `${JSON.stringify(value, null, 2)}\n`;
if (mode === "--inventory") {
  fs.writeFileSync(inventoryPath, render(inventory()));
  console.log("inventoried 177 H2.6a inputs; 3 additional map option rows; 0 virtual configs");
} else {
  const result = render(build());
  if (mode === "--write") fs.writeFileSync(fixturePath, result);
  else assert.equal(fs.readFileSync(fixturePath, "utf8"), result);
  console.log(`${mode}: 31 option witnesses, two TypeScript observations each`);
}
