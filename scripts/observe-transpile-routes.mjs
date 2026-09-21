#!/usr/bin/env node
// H2.8c source oracle: observe TypeScript 6.0.3's three no-check emit routes.
//
//   transpile-js      ts.transpileModule(input, options)        (typescript.js:145985)
//   transpile-dts     ts.transpileDeclaration(input, options)   (typescript.js:145993)
//   program-no-check  createProgram + emitFilesAndReportErrorsAndGetExitStatus with noCheck
//
// Usage:
//   node scripts/observe-transpile-routes.mjs --write-inputs <inputs.json>
//   node scripts/observe-transpile-routes.mjs --inputs <inputs.json> --out <expected.json>
//
// The public observation of each route is recorded separately from the
// internal evidence (host request trace, emit-resolver requests, semantic
// getter results) which is gathered by an instrumented replica Program and
// never mixed into the public comparison.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { fileURLToPath } from "node:url";

const SCRIPT = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(SCRIPT), "..");
const TS_PATH = path.join(WORKSPACE, "vendor/typescript-6.0.3/lib/typescript.js");
const LIB_DIR = path.join(WORKSPACE, "vendor/typescript-6.0.3/lib");
const INVENTORY = path.join(WORKSPACE, "vendor/typescript-6.0.3/transpile-suite-inventory.v1.json");
const TRANSPILE_SOURCES = path.join(WORKSPACE, "ts-tests/tests/cases/transpile");
const EXPECTED_BUNDLE_SHA256 =
  "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39";

const ts = (await import(TS_PATH)).default;

function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}
function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}
requireCondition(sha256(fs.readFileSync(TS_PATH)) === EXPECTED_BUNDLE_SHA256, "typescript.js hash drift");
requireCondition(ts.version === "6.0.3", `unexpected TypeScript version ${ts.version}`);

// ---------------------------------------------------------------------------
// Serialization (public observation schema)
// ---------------------------------------------------------------------------

function flattenChain(text, indent, out) {
  if (typeof text === "string") {
    if (indent !== 0) out.push("\n" + "  ".repeat(indent));
    out.push(text);
    return;
  }
  if (indent !== 0) out.push("\n" + "  ".repeat(indent));
  out.push(text.messageText);
  for (const next of text.next ?? []) flattenChain(next, indent + 1, out);
}
function categoryName(category) {
  switch (category) {
    case ts.DiagnosticCategory.Warning: return "Warning";
    case ts.DiagnosticCategory.Error: return "Error";
    case ts.DiagnosticCategory.Suggestion: return "Suggestion";
    case ts.DiagnosticCategory.Message: return "Message";
    default: throw new Error(`unknown category ${category}`);
  }
}
function serializeDiagnostic(diagnostic) {
  const out = [];
  flattenChain(diagnostic.messageText, 0, out);
  const related = diagnostic.relatedInformation;
  return {
    code: diagnostic.code,
    category: categoryName(diagnostic.category),
    file: diagnostic.file ? diagnostic.file.fileName : null,
    start: diagnostic.start === undefined ? null : diagnostic.start,
    length: diagnostic.length === undefined ? null : diagnostic.length,
    message: out.join(""),
    related_information: related === undefined
      ? null
      : related.map((entry) => {
          const text = [];
          flattenChain(entry.messageText, 0, text);
          return {
            code: entry.code,
            category: categoryName(entry.category),
            file: entry.file ? entry.file.fileName : null,
            start: entry.start === undefined ? null : entry.start,
            length: entry.length === undefined ? null : entry.length,
            message: text.join(""),
            related_information: null,
          };
        }),
  };
}
function utf8(text) {
  return Buffer.from(text, "utf8");
}
function textRecord(text) {
  // JavaScript strings are UTF-16; the UTF-8 materialization is recorded
  // separately (lone surrogates become U+FFFD there, the unit count does not).
  if (text === undefined) return { present: false, utf16_units: null, utf8_base64: null, utf8_sha256: null };
  const bytes = utf8(text);
  return {
    present: true,
    utf16_units: text.length,
    utf8_base64: bytes.toString("base64"),
    utf8_sha256: sha256(bytes),
  };
}
function outputKind(fileName) {
  if (/\.d\.[cm]?ts$/.test(fileName)) return "declaration";
  if (fileName.endsWith(".map")) return "source-map";
  if (fileName.endsWith(".tsbuildinfo")) return "build-info";
  if (/\.(js|jsx|mjs|cjs)$/.test(fileName)) return "javascript";
  throw new Error(`unknown emitted output kind for ${fileName}`);
}
function serializeWrite(args, index, sinkAction, onErrorMessages) {
  const [fileName, text, bom, onError, sourceFiles, data] = args;
  const callbackBytes = utf8(text);
  const materialized = bom ? Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), callbackBytes]) : callbackBytes;
  const known = ["sourceMapUrlPos", "diagnostics", "skippedDtsWrite", "differsOnlyInMap", "buildInfo"];
  let dataRecord = { data_present: false, data_keys: null, data_source_map_url_pos: null, data_diagnostics: null, data_build_info: null };
  if (data !== undefined) {
    const keys = Reflect.ownKeys(data).map(String);
    requireCondition(keys.every((key) => known.includes(key)), `unknown write metadata key in ${fileName}`);
    dataRecord = {
      data_present: true,
      data_keys: keys,
      data_source_map_url_pos: data.sourceMapUrlPos === undefined ? null : data.sourceMapUrlPos,
      data_diagnostics: (data.diagnostics ?? []).map(serializeDiagnostic),
      data_build_info: data.buildInfo === undefined ? null : JSON.parse(JSON.stringify(data.buildInfo)),
    };
  }
  return {
    index,
    path: ts.normalizePath(fileName),
    kind: outputKind(fileName),
    callback_utf8_base64: callbackBytes.toString("base64"),
    callback_utf8_bytes: callbackBytes.length,
    write_byte_order_mark: bom,
    materialized_utf8_base64: materialized.toString("base64"),
    materialized_utf8_bytes: materialized.length,
    on_error_callback_present: onError !== undefined,
    source_files: (sourceFiles ?? []).map((source) => ts.normalizePath(source.fileName)),
    ...dataRecord,
    sink_action: sinkAction,
    on_error_messages: onErrorMessages,
  };
}
function serializeEmitResult(result) {
  return {
    emit_skipped: result.emitSkipped,
    diagnostics: result.diagnostics.map(serializeDiagnostic),
    emitted_files: result.emittedFiles === undefined ? null : result.emittedFiles.map(ts.normalizePath),
    source_maps: result.sourceMaps === undefined
      ? null
      : result.sourceMaps.map((map) => ({
          input_source_file_names: map.inputSourceFileNames.map(ts.normalizePath),
          source_map_json: JSON.stringify(map.sourceMap),
        })),
  };
}
function exceptionRecord(error) {
  return { message: String(error && error.message !== undefined ? error.message : error) };
}

// ---------------------------------------------------------------------------
// Route: transpileModule / transpileDeclaration (public API observation)
// ---------------------------------------------------------------------------

function cloneOptions(options) {
  return options === undefined ? undefined : JSON.parse(JSON.stringify(options));
}
function transpileCall(input) {
  const api = input.route === "transpile-dts" ? ts.transpileDeclaration : ts.transpileModule;
  const transpileOptions = {};
  if (input.compilerOptions !== undefined) transpileOptions.compilerOptions = cloneOptions(input.compilerOptions);
  if (input.fileName !== undefined) transpileOptions.fileName = input.fileName;
  if (input.reportDiagnostics !== undefined) transpileOptions.reportDiagnostics = input.reportDiagnostics;
  if (input.moduleName !== undefined) transpileOptions.moduleName = input.moduleName;
  if (input.renamedDependencies !== undefined) transpileOptions.renamedDependencies = { ...input.renamedDependencies };
  if (input.jsDocParsingMode !== undefined) transpileOptions.jsDocParsingMode = ts.JSDocParsingMode[input.jsDocParsingMode];
  try {
    const result = api(input.text, transpileOptions);
    const keys = Object.keys(result).sort();
    requireCondition(
      keys.join(",") === "diagnostics,outputText,sourceMapText",
      `unexpected transpile result keys ${keys.join(",")}`,
    );
    return {
      exception: null,
      outputText: textRecord(result.outputText),
      diagnostics_present: result.diagnostics !== undefined,
      diagnostics: (result.diagnostics ?? []).map(serializeDiagnostic),
      sourceMapText: textRecord(result.sourceMapText),
    };
  } catch (error) {
    return {
      exception: exceptionRecord(error),
      outputText: null,
      diagnostics_present: null,
      diagnostics: null,
      sourceMapText: null,
    };
  }
}

// Instrumented replica of transpileWorker (typescript.js:146022-146133): the
// same host, options fixup and Program, with request tracing. Its public
// result must equal the real API call; only the trace is recorded as
// internal evidence.
const BAREBONES_LIB = `interface Boolean {}
interface Function {}
interface CallableFunction {}
interface NewableFunction {}
interface IArguments {}
interface Number {}
interface Object {}
interface RegExp {}
interface String {}
interface Array<T> { length: number; [n: number]: T; }
interface SymbolConstructor {
    (desc?: string | number): symbol;
    for(name: string): symbol;
    readonly toStringTag: symbol;
}
declare var Symbol: SymbolConstructor;
interface Symbol {
    readonly [Symbol.toStringTag]: string;
}`;
const TRANSPILE_FORCED = ts.optionDeclarations.filter((option) => Object.prototype.hasOwnProperty.call(option, "transpileOptionValue"));

function replicaTranspile(input) {
  const declaration = input.route === "transpile-dts";
  const trace = { host: [], emit_resolver_requests: [], checker_get_diagnostics: 0 };
  const diagnostics = [];
  let options = input.compilerOptions ? ts.fixupCompilerOptions(cloneOptions(input.compilerOptions), diagnostics) : {};
  const fixupDiagnostics = diagnostics.length;
  const defaults = { target: ts.ScriptTarget.LatestStandard ?? 12, jsx: ts.JsxEmit.Preserve };
  for (const key of Object.keys(defaults)) if (options[key] === undefined) options[key] = defaults[key];
  const forced = [];
  for (const option of TRANSPILE_FORCED) {
    if (options.verbatimModuleSyntax && option.name === "isolatedModules") continue;
    options[option.name] = option.transpileOptionValue;
    forced.push([option.name, option.transpileOptionValue === undefined ? null : option.transpileOptionValue]);
  }
  options.suppressOutputPathCheck = true;
  options.allowNonTsExtensions = true;
  if (declaration) {
    options.declaration = true;
    options.emitDeclarationOnly = true;
    options.isolatedDeclarations = true;
  } else {
    options.declaration = false;
    options.declarationMap = false;
  }
  options.noLib = !declaration;
  const newLine = ts.getNewLineCharacter(options);
  const inputFileName = input.fileName || (input.compilerOptions && input.compilerOptions.jsx ? "module.tsx" : "module.ts");
  const libName = "lib.d.ts";
  let outputText;
  let sourceMapText;
  let sourceFile;
  const libFile = ts.createSourceFile(libName, BAREBONES_LIB, { languageVersion: ts.ScriptTarget.Latest });
  const host = {
    getSourceFile: (fileName) => {
      trace.host.push(["getSourceFile", fileName]);
      return fileName === ts.normalizePath(inputFileName) ? sourceFile : fileName === ts.normalizePath(libName) ? libFile : undefined;
    },
    writeFile: (name, text) => {
      trace.host.push(["writeFile", name]);
      if (ts.fileExtensionIs(name, ".map")) sourceMapText = text; else outputText = text;
    },
    getDefaultLibFileName: () => libName,
    useCaseSensitiveFileNames: () => false,
    getCanonicalFileName: (fileName) => fileName,
    getCurrentDirectory: () => "",
    getNewLine: () => newLine,
    fileExists: (fileName) => {
      trace.host.push(["fileExists", fileName]);
      return fileName === inputFileName || (!!declaration && fileName === libName);
    },
    readFile: (fileName) => {
      trace.host.push(["readFile", fileName]);
      return "";
    },
    directoryExists: (d) => {
      trace.host.push(["directoryExists", d]);
      return true;
    },
    getDirectories: () => [],
  };
  const impliedNodeFormat = ts.getImpliedNodeFormatForFile(
    ts.toPath(inputFileName, "", host.getCanonicalFileName),
    undefined,
    host,
    options,
  );
  sourceFile = ts.createSourceFile(inputFileName, input.text, {
    languageVersion: ts.getEmitScriptTarget(options),
    impliedNodeFormat,
    setExternalModuleIndicator: ts.getSetExternalModuleIndicator(options),
    jsDocParsingMode: input.jsDocParsingMode === undefined ? ts.JSDocParsingMode.ParseAll : ts.JSDocParsingMode[input.jsDocParsingMode],
  });
  if (input.moduleName) sourceFile.moduleName = input.moduleName;
  if (input.renamedDependencies) sourceFile.renamedDependencies = new Map(Object.entries(input.renamedDependencies));
  const program = ts.createProgram([inputFileName], options, host);
  const checker = program.getTypeChecker();
  const originalGetEmitResolver = checker.getEmitResolver;
  checker.getEmitResolver = function (file, token, skip) {
    trace.emit_resolver_requests.push({ source_file: file ? file.fileName : null, skip_diagnostics: !!skip });
    return originalGetEmitResolver.call(this, file, token, skip);
  };
  const originalGetDiagnostics = checker.getDiagnostics;
  checker.getDiagnostics = function (...args) {
    trace.checker_get_diagnostics += 1;
    return originalGetDiagnostics.apply(this, args);
  };
  const syntactic = program.getSyntacticDiagnostics(sourceFile);
  const optionsDiagnostics = program.getOptionsDiagnostics();
  if (input.reportDiagnostics) {
    diagnostics.push(...syntactic, ...optionsDiagnostics);
  }
  let exception = null;
  let result;
  try {
    result = program.emit(undefined, undefined, undefined, declaration, undefined, declaration);
    diagnostics.push(...result.diagnostics);
    if (outputText === undefined) throw new Error("Debug Failure. Output generation failed");
  } catch (error) {
    exception = exceptionRecord(error);
  }
  // Evidence-only getters (never part of the public result): whole-Program
  // semantic and global diagnostics under the forced noCheck.
  const semantic = program.getSemanticDiagnostics();
  const global = program.getGlobalDiagnostics();
  return {
    public: {
      exception,
      outputText: exception ? null : textRecord(outputText),
      diagnostics_present: exception ? null : true,
      diagnostics: exception ? null : diagnostics.map(serializeDiagnostic),
      sourceMapText: exception ? null : textRecord(sourceMapText),
    },
    internal: {
      input_file_name: inputFileName,
      implied_node_format: impliedNodeFormat === undefined ? null : ts.ModuleKind[impliedNodeFormat],
      script_kind: ts.ScriptKind[sourceFile.scriptKind],
      is_declaration_file: sourceFile.isDeclarationFile,
      external_module: !!sourceFile.externalModuleIndicator,
      fixup_diagnostics: fixupDiagnostics,
      forced_options: forced,
      effective_options: effectiveOptions(options),
      syntactic_diagnostics: syntactic.map(serializeDiagnostic),
      options_diagnostics: optionsDiagnostics.map(serializeDiagnostic),
      semantic_diagnostics_evidence: semantic.map(serializeDiagnostic),
      global_diagnostics_evidence: global.map(serializeDiagnostic),
      emit_result: result ? serializeEmitResult(result) : null,
      source_files: program.getSourceFiles().map((file) => file.fileName),
      trace,
    },
  };
}
function effectiveOptions(options) {
  const out = {};
  for (const key of Object.keys(options).sort()) {
    const value = options[key];
    out[key] = value === undefined ? null : value;
  }
  return out;
}

// ---------------------------------------------------------------------------
// Route: Program noCheck complete command
// ---------------------------------------------------------------------------

const libCache = new Map();
function readLib(name) {
  if (!libCache.has(name)) {
    const file = path.join(LIB_DIR, name);
    libCache.set(name, fs.existsSync(file) ? fs.readFileSync(file, "utf8") : undefined);
  }
  return libCache.get(name);
}
function programCommand(input) {
  const files = new Map(input.files.map((file) => [file.path, file.text]));
  // tsc parses tsconfig/CLI option strings into typed values before
  // createProgram; the manifest stores the JSON spelling and converts here.
  const converted = ts.convertCompilerOptionsFromJson(cloneOptions(input.options), input.current_directory);
  requireCondition(converted.errors.length === 0, `${input.id}: option conversion failed ${JSON.stringify(converted.errors.map((e) => e.messageText))}`);
  const options = converted.options;
  const caseSensitive = input.use_case_sensitive_file_names;
  const canonical = (name) => (caseSensitive ? name : name.toLowerCase());
  // Program's internal getTypeChecker closure is not interceptable from the
  // public surface; only host requests are traced for this route.
  const trace = { host: [] };
  const writes = [];
  const reported = [];
  const status = [];
  const sinkRules = input.sink_rules ?? [];
  const sourceText = (name) => {
    if (files.has(name)) return files.get(name);
    if (name.startsWith("/lib/")) return readLib(name.slice("/lib/".length));
    return undefined;
  };
  const host = {
    getSourceFile: (fileName, languageVersionOrOptions) => {
      trace.host.push(["getSourceFile", fileName]);
      const text = sourceText(fileName);
      return text === undefined ? undefined : ts.createSourceFile(fileName, text, languageVersionOrOptions);
    },
    getDefaultLibLocation: () => "/lib",
    getDefaultLibFileName: (o) => "/lib/" + ts.getDefaultLibFileName(o),
    writeFile: () => { throw new Error("host writeFile must not be used; the command supplies its own"); },
    getCurrentDirectory: () => input.current_directory,
    getCanonicalFileName: canonical,
    useCaseSensitiveFileNames: () => caseSensitive,
    getNewLine: () => ts.getNewLineCharacter(options),
    fileExists: (fileName) => {
      trace.host.push(["fileExists", fileName]);
      return sourceText(fileName) !== undefined;
    },
    readFile: (fileName) => {
      trace.host.push(["readFile", fileName]);
      return sourceText(fileName);
    },
    directoryExists: (d) => {
      const normalized = ts.normalizePath(d).replace(/\/$/, "");
      const exists = normalized === "" || normalized === "/" || normalized === "/lib" ||
        [...files.keys()].some((f) => f.startsWith(normalized + "/")) || input.current_directory.startsWith(normalized);
      trace.host.push(["directoryExists", d]);
      return exists;
    },
    getDirectories: () => [],
    realpath: (p) => p,
  };
  let exception = null;
  let exitCode = null;
  let emitResult;
  let semanticEvidence = [];
  let globalEvidence = [];
  try {
    const program = ts.createProgram(input.roots, options, host);
    const originalEmit = program.emit;
    program.emit = function (...args) {
      requireCondition(emitResult === undefined, "command emitted more than once");
      emitResult = originalEmit.apply(this, args);
      return emitResult;
    };
    exitCode = ts.emitFilesAndReportErrorsAndGetExitStatus(
      program,
      (diagnostic) => reported.push(diagnostic),
      (text) => status.push(text),
      undefined,
      (...args) => {
        const [fileName, , , onError] = args;
        const rule = sinkRules.find((r) => r.path === fileName);
        const action = rule ? rule.action : "write";
        let messages = null;
        if (action === "on-error") {
          messages = ["H2.8c controlled sink failure"];
          onError(messages[0]);
        }
        writes.push(serializeWrite(args, writes.length, action, messages));
      },
    );
    semanticEvidence = program.getSemanticDiagnostics().map(serializeDiagnostic);
    globalEvidence = program.getGlobalDiagnostics().map(serializeDiagnostic);
  } catch (error) {
    exception = exceptionRecord(error);
  }
  return {
    public: {
      kind: "ordinary-command",
      target_source: null,
      writes,
      reported_diagnostics: reported.map(serializeDiagnostic),
      status_writes: status,
      exit_code: exitCode,
      emit_result: emitResult ? serializeEmitResult(emitResult) : null,
      exception,
    },
    internal: {
      semantic_diagnostics_evidence: semanticEvidence,
      global_diagnostics_evidence: globalEvidence,
      trace,
    },
  };
}

// ---------------------------------------------------------------------------
// Case manifest
// ---------------------------------------------------------------------------

function inventoryCases() {
  const inventory = JSON.parse(fs.readFileSync(INVENTORY, "utf8"));
  requireCondition(inventory.summary.source_files === 22, "inventory drift: expected 22 source files");
  const cases = [];
  for (const entry of inventory.cases) {
    const fixture = inventory.fixtures[entry.source];
    const source = inventory.sources[entry.source];
    const bytes = fs.readFileSync(path.join(TRANSPILE_SOURCES, source.path));
    requireCondition(sha256(bytes) === source.sha256, `source drift ${source.path}`);
    const text = bytes.toString("utf8");
    const configuration = fixture.configurations[entry.configuration];
    // harnessIO.makeUnitsFromTest: split on @filename directives; options are
    // the @-settings of the whole fixture (comma lists narrowed by the
    // configuration override) as raw strings. transpileRunner hands every
    // setting to transpileModule as compilerOptions strings, so fixup
    // (fixupCompilerOptions) converts the enum-typed ones.
    const settings = {};
    for (const setting of fixture.settings) settings[setting.name] = setting.value;
    for (const override of configuration.overrides) settings[override.name] = override.value;
    const units = splitUnits(text, source.path);
    requireCondition(units.length === fixture.units.length, `unit drift ${source.path}`);
    const compilerOptions = {};
    for (const [name, value] of Object.entries(settings)) {
      if (name.toLowerCase() === "filename") continue;
      compilerOptions[name] = harnessOptionValue(name, value);
    }
    units.forEach((unit, unitIndex) => {
      requireCondition(unit.name === fixture.units[unitIndex].name, `unit name drift ${source.path}`);
      cases.push({
        id: `h2-8c/${entry.kind === "module" ? "transpile-js" : "transpile-dts"}/inventory/${entry.id.replace(/^transpile:/, "").replace(/#/g, "~")}~${unit.name}`,
        route: entry.kind === "module" ? "transpile-js" : "transpile-dts",
        inventory_case: entry.id,
        source_path: source.path,
        text: unit.text,
        fileName: unit.name,
        reportDiagnostics: entry.report_diagnostics,
        compilerOptions,
      });
    });
  }
  return cases;
}
function harnessOptionValue(name, value) {
  // Utils.setCompilerOptionsFromHarnessSetting: boolean settings parse to
  // booleans, enum/list settings stay strings for fixupCompilerOptions.
  const option = ts.optionDeclarations.find((o) => o.name.toLowerCase() === name.toLowerCase());
  if (option && option.type === "boolean") {
    requireCondition(value === "true" || value === "false", `non-boolean harness setting ${name}=${value}`);
    return value === "true";
  }
  if (option && option.type === "number") return Number(value);
  return value;
}
function splitUnits(text, sourcePath) {
  const lines = text.split(/\r?\n/);
  const units = [];
  let current = null;
  let currentLines = [];
  const defaultName = path.basename(sourcePath);
  const flush = () => {
    if (current === null && currentLines.every((line) => /^\s*$/.test(line) || /^\/\/\s*@/.test(line))) {
      currentLines = [];
      return;
    }
    units.push({ name: current ?? defaultName, text: currentLines.join("\n") + (currentLines.length ? "\n" : "") });
    currentLines = [];
  };
  for (const line of lines) {
    const match = /^\/\/\s*@(\w+)\s*:\s*(.*?)\s*$/.exec(line);
    if (match && match[1].toLowerCase() === "filename") {
      flush();
      current = match[2];
      continue;
    }
    if (match) continue;
    currentLines.push(line);
  }
  // drop the trailing empty line produced by the final newline
  if (currentLines.length && currentLines[currentLines.length - 1] === "") currentLines.pop();
  flush();
  return units;
}

function tj(id, text, extra = {}) {
  return { id: `h2-8c/transpile-js/${id}`, route: "transpile-js", text, ...extra };
}
function td(id, text, extra = {}) {
  return { id: `h2-8c/transpile-dts/${id}`, route: "transpile-dts", text, ...extra };
}
function pn(id, files, options, extra = {}) {
  return {
    id: `h2-8c/program-no-check/${id}`,
    route: "program-no-check",
    current_directory: "/project",
    use_case_sensitive_file_names: true,
    files: Object.entries(files).map(([p, text]) => ({ path: p, text })),
    roots: Object.keys(files),
    options: { noCheck: true, newLine: "lf", ...options },
    ...extra,
  };
}

const IMPORT_ELISION = `import { A, B, type C } from "./dep";
import * as ns from "./dep";
import D = require("./dep");
export const value = new A();
let onlyType: B;
let alsoType: C;
export function useNs(): ns.T { return null as any; }
`;
const ES5_FLAGS = `class Base { m() { return 1; } }
class Derived extends Base {
  async m2() { return super.m() + arguments.length; }
  static { this.x = super.toString(); }
}
for (let i = 0; i < 3; i++) { setTimeout(() => console.log(i)); }
function f() { return () => arguments[0]; }
`;
const CLASS_FIELDS = `class C { x = 1; static y = 2; #z = 3; declare d: number; accessor a = 4; }
`;
const DECORATORS = `function dec(...args: any[]) {}
@dec class A { @dec m(@dec p: string) {} @dec prop = 1; @dec static s() {} }
`;
const ENUM_NS = `const enum E { A = 1, B = A << 1, C = "x".length }
enum F { X, Y = E.B }
namespace N { export const v = E.B; export namespace M { export let w = F.Y; } }
declare namespace D { const q: number; }
export const u = E.C + N.v;
`;
const JSX = `const el = <div className="a">{1 + 1}<span /></div>;
export default el;
`;

function extraCases() {
  const cases = [];
  // --- transpile-js
  cases.push(tj("empty/no-input", ""));
  cases.push(tj("empty/whitespace-only", "  \n\n"));
  cases.push(tj("kinds/ts-basic", "let x: number = 1;\nexport {};\n"));
  cases.push(tj("kinds/tsx-jsx-react", JSX, { compilerOptions: { jsx: "react" } }));
  cases.push(tj("kinds/tsx-jsx-preserve-default", JSX, { compilerOptions: { jsx: 1 } }));
  cases.push(tj("kinds/js-input", "export const a = 1;\nvar b = a + 2;\n", { fileName: "module.js" }));
  cases.push(tj("kinds/js-input-jsx", "const x = <a/>;\nexport {x};\n", { fileName: "module.jsx", compilerOptions: { jsx: "react" } }));
  cases.push(tj("filename/absent", "export const a: string = 'a';\n"));
  cases.push(tj("filename/explicit-nested", "export const a = 1;\n", { fileName: "src/lib/a.ts" }));
  cases.push(tj("filename/nonstandard-extension", "export const a: number = 1;\n", { fileName: "module.txt" }));
  cases.push(tj("filename/declaration-input-throws", "declare const a: number;\nexport {};\n", { fileName: "module.d.ts" }));
  cases.push(tj("filename/mts", "export const a = 1;\n", { fileName: "module.mts", compilerOptions: { module: "node20" } }));
  cases.push(tj("filename/cts", "export const a = 1;\n", { fileName: "module.cts", compilerOptions: { module: "node20" } }));
  cases.push(tj("filename/json-input", '{"a": 1}\n', { fileName: "data.json" }));
  cases.push(tj("module-name/amd", "export const a = 1;\n", { moduleName: "my/mod", compilerOptions: { module: "amd" } }));
  cases.push(tj("module-name/system", "export const a = 1;\n", { moduleName: "my/mod", compilerOptions: { module: "system" } }));
  cases.push(tj("module-name/commonjs-ignored", "export const a = 1;\n", { moduleName: "my/mod", compilerOptions: { module: "commonjs" } }));
  cases.push(tj("renamed/commonjs", "import { x } from './a';\nimport './side';\nexport const y = x;\n", { renamedDependencies: { "./a": "./renamed-a", "./side": "./renamed-side" }, compilerOptions: { module: "commonjs" } }));
  cases.push(tj("renamed/esnext", "import { x } from './a';\nexport const y = x;\n", { renamedDependencies: { "./a": "./renamed-a" }, compilerOptions: { module: "esnext" } }));
  cases.push(tj("jsdoc/parse-none", "/** @deprecated */\nexport function f() {}\n", { jsDocParsingMode: "ParseNone" }));
  cases.push(tj("diag/syntax-error-report-on", "let x: = 1;\nexport {};\n", { reportDiagnostics: true }));
  cases.push(tj("diag/syntax-error-report-off", "let x: = 1;\nexport {};\n", { reportDiagnostics: false }));
  cases.push(tj("diag/syntax-error-report-absent", "let x: = 1;\nexport {};\n"));
  cases.push(tj("diag/option-invalid-enum-string", "export const a = 1;\n", { reportDiagnostics: true, compilerOptions: { target: "es9999" } }));
  cases.push(tj("diag/option-invalid-enum-number", "export const a = 1;\n", { reportDiagnostics: true, compilerOptions: { target: 1234 } }));
  cases.push(tj("diag/option-invalid-enum-report-off", "export const a = 1;\n", { reportDiagnostics: false, compilerOptions: { target: "es9999" } }));
  cases.push(tj("diag/option-conflict-verbatim-commonjs", "import { A } from './a';\nlet v: A;\nexport {};\n", { reportDiagnostics: true, compilerOptions: { module: "commonjs", verbatimModuleSyntax: true } }));
  cases.push(tj("diag/option-conflict-maps", "export const a = 1;\n", { reportDiagnostics: true, compilerOptions: { sourceMap: true, inlineSourceMap: true } }));
  cases.push(tj("diag/semantic-only-error", "const x: number = 'string';\nexport const y: string = x;\n", { reportDiagnostics: true }));
  cases.push(tj("diag/clean-report-on", "export const a = 1;\n", { reportDiagnostics: true }));
  cases.push(tj("diag/target-es5-deprecated-report-on", "export const a = () => 1;\n", { reportDiagnostics: true, compilerOptions: { target: "es5" } }));
  cases.push(tj("diag/target-es5-ignore-deprecations", "export const a = () => 1;\n", { reportDiagnostics: true, compilerOptions: { target: "es5", ignoreDeprecations: "6.0" } }));
  cases.push(tj("diag/isolated-modules-reexport-type", "import { T } from './t';\nexport { T };\n", { reportDiagnostics: true, compilerOptions: { module: "esnext" } }));
  cases.push(tj("modules/import-elision", IMPORT_ELISION, { compilerOptions: { module: "commonjs" } }));
  cases.push(tj("modules/import-elision-esnext", IMPORT_ELISION, { compilerOptions: { module: "esnext" } }));
  cases.push(tj("modules/import-elision-verbatim", IMPORT_ELISION.replace('import D = require("./dep");\n', ""), { compilerOptions: { module: "esnext", verbatimModuleSyntax: true } }));
  cases.push(tj("modules/export-star-and-default", "export * from './a';\nexport * as b from './b';\nexport default function () {}\nexport { c as default2 } from './c';\n", { compilerOptions: { module: "commonjs" } }));
  cases.push(tj("modules/export-equals", "import fs = require('fs');\nexport = fs.readFileSync;\n", { compilerOptions: { module: "commonjs" } }));
  cases.push(tj("modules/dynamic-import-es5", "export async function f() { return (await import('./a')).x; }\n", { compilerOptions: { module: "commonjs", target: "es5" } }));
  cases.push(tj("modules/type-only-export", "import type { A } from './a';\nexport type { A };\nexport type B = A;\n", { compilerOptions: { module: "commonjs" } }));
  cases.push(tj("modules/script-no-exports", "const a = 1;\nnamespace N { export const b = a; }\n", { compilerOptions: { module: "commonjs" } }));
  cases.push(tj("lang/enum-namespace", ENUM_NS, { compilerOptions: { module: "esnext" } }));
  cases.push(tj("lang/enum-namespace-es5", ENUM_NS, { compilerOptions: { module: "commonjs", target: "es5" } }));
  cases.push(tj("lang/decorators-experimental", DECORATORS, { compilerOptions: { experimentalDecorators: true, target: "es2017" } }));
  cases.push(tj("lang/decorators-experimental-metadata", DECORATORS, { compilerOptions: { experimentalDecorators: true, emitDecoratorMetadata: true, target: "es2017" } }));
  cases.push(tj("lang/decorators-standard-es2022", DECORATORS, { compilerOptions: { target: "es2022" } }));
  cases.push(tj("lang/decorators-standard-esnext", DECORATORS, { compilerOptions: { target: "esnext" } }));
  for (const target of ["es5", "es2015", "es2017", "es2021", "es2022", "esnext"]) {
    cases.push(tj(`lang/class-fields-${target}`, CLASS_FIELDS, { compilerOptions: { target } }));
  }
  cases.push(tj("lang/class-fields-es2022-no-define", CLASS_FIELDS, { compilerOptions: { target: "es2022", useDefineForClassFields: false } }));
  cases.push(tj("lang/es5-flags", ES5_FLAGS, { compilerOptions: { target: "es5" } }));
  cases.push(tj("lang/es2015-flags", ES5_FLAGS, { compilerOptions: { target: "es2015" } }));
  cases.push(tj("lang/es2017-flags", ES5_FLAGS, { compilerOptions: { target: "es2017" } }));
  cases.push(tj("lang/generators-es5", "export function* g() { yield 1; const x = yield* g(); }\nasync function* ag() { for await (const x of []) yield x; }\n", { compilerOptions: { target: "es5" } }));
  cases.push(tj("lang/object-rest-es2017", "const { a, ...rest } = { a: 1, b: 2 };\nconst o = { ...rest, a };\nexport { o };\n", { compilerOptions: { target: "es2017" } }));
  cases.push(tj("lang/optional-chaining-es2019", "declare const o: any;\nexport const v = o?.a?.[0]?.() ?? 1;\nlet w = 0; w ??= 2; w ||= 3; w &&= 4;\n", { compilerOptions: { target: "es2019" } }));
  cases.push(tj("lang/using-es2022", "export {};\n{ using x = null; await using y = null; }\n", { compilerOptions: { target: "es2022", module: "esnext" } }));
  cases.push(tj("lang/private-in-es2020", "class C { #p = 1; static has(o: object) { return #p in o; } }\nexport {C};\n", { compilerOptions: { target: "es2020" } }));
  cases.push(tj("lang/import-helpers", "export class A extends Object {}\nexport const {a, ...r} = {a: 1};\n", { compilerOptions: { target: "es5", importHelpers: true, module: "commonjs" } }));
  cases.push(tj("maps/source-map", "export const a: number = 1;\nexport function f(x: string) { return x; }\n", { compilerOptions: { sourceMap: true } }));
  cases.push(tj("maps/inline-source-map", "export const a: number = 1;\n", { compilerOptions: { inlineSourceMap: true } }));
  cases.push(tj("maps/inline-sources", "export const a: number = 1;\n", { compilerOptions: { sourceMap: true, inlineSources: true } }));
  cases.push(tj("maps/source-map-nested-filename", "export const a = 1;\n", { fileName: "src/deep/a.ts", compilerOptions: { sourceMap: true, sourceRoot: "/root", mapRoot: "/maps" } }));
  cases.push(tj("maps/declaration-map-forced-off", "export const a = 1;\n", { compilerOptions: { declarationMap: true, declaration: true } }));
  cases.push(tj("text/crlf", "export const a = 1;\r\nexport const b = 2;\r\n"));
  cases.push(tj("text/crlf-newline-option", "export const a = 1;\nexport const b = 2;\n", { compilerOptions: { newLine: "crlf" } }));
  cases.push(tj("text/unicode", "export const s = 'é😀漢字';\nconst \\u{1F600}x = `\\u{1F600}${s}`;\nexport { \\u{1F600}x as 😀 };\n", { compilerOptions: { target: "es5" } }));
  cases.push(tj("text/bom", "﻿export const a = 1;\n"));
  cases.push(tj("text/shebang-and-prologue", "#!/usr/bin/env node\n'use strict';\nexport const a = 1;\n", { compilerOptions: { module: "commonjs" } }));
  cases.push(tj("options/target-string", "export const a = () => 1;\n", { compilerOptions: { target: "ES5" } }));
  cases.push(tj("options/module-string-case", "export const a = 1;\n", { compilerOptions: { module: "CommonJS" } }));
  cases.push(tj("options/unknown-option", "export const a = 1;\n", { reportDiagnostics: true, compilerOptions: { noSuchOption: true } }));
  cases.push(tj("options/no-emit-forced-off", "export const a = 1;\n", { reportDiagnostics: true, compilerOptions: { noEmit: true } }));
  cases.push(tj("options/no-emit-on-error-forced-off", "const x: number = 'a';\nexport {x};\n", { reportDiagnostics: true, compilerOptions: { noEmitOnError: true } }));
  cases.push(tj("options/declaration-forced-off", "export const a = 1;\n", { reportDiagnostics: true, compilerOptions: { declaration: true, emitDeclarationOnly: true } }));
  cases.push(tj("options/out-file-forced-off", "export const a = 1;\n", { reportDiagnostics: true, compilerOptions: { outFile: "bundle.js", module: "amd" } }));
  cases.push(tj("options/no-check-false-forced-true", "const x: number = 'a';\nexport {x};\n", { reportDiagnostics: true, compilerOptions: { noCheck: false } }));
  cases.push(tj("options/isolated-modules-false-forced-true", "import { T } from './t';\nexport { T };\n", { reportDiagnostics: true, compilerOptions: { isolatedModules: false, module: "esnext" } }));
  cases.push(tj("options/lib-forced-off", "export const a = 1;\n", { reportDiagnostics: true, compilerOptions: { lib: ["es2015"] } }));
  cases.push(tj("options/no-lib-false-forced-true", "export const a: Array<number> = [];\n", { reportDiagnostics: true, compilerOptions: { noLib: false } }));
  cases.push(tj("options/remove-comments", "// c1\n/* c2 */ export const a = 1; // c3\n", { compilerOptions: { removeComments: true } }));
  cases.push(tj("options/preserve-const-enums-false", "const enum E { A = 1 }\nexport const v = E.A;\n", { compilerOptions: { preserveConstEnums: false, module: "esnext" } }));
  cases.push(tj("options/emit-bom", "export const a = 1;\n", { compilerOptions: { emitBOM: true } }));
  cases.push(tj("options/module-detection-force", "const a = 1;\n", { compilerOptions: { moduleDetection: "force", module: "commonjs" } }));
  cases.push(tj("options/jsx-factory", JSX, { compilerOptions: { jsx: "react", jsxFactory: "h", jsxFragmentFactory: "F" } }));
  cases.push(tj("options/jsx-react-jsx", JSX, { compilerOptions: { jsx: "react-jsx", module: "esnext" } }));
  cases.push(tj("options/jsx-react-jsxdev-import-source", JSX, { compilerOptions: { jsx: "react-jsxdev", jsxImportSource: "preact", module: "commonjs" } }));
  cases.push(tj("repeat/first", "export const first = 1;\n"));
  cases.push(tj("repeat/second", "export const second = 2;\n"));
  // stage C producers (moduleName / renamedDependencies / jsDocParsingMode /
  // allowNonTsExtensions) — added after the first candidate.
  cases.push(tj("filename/extensionless", "export const a: number = 1;\n", { fileName: "module" }));
  cases.push(tj("filename/nonstandard-extension-source-map", "export const a: number = 1;\n", { fileName: "dir/module.txt", compilerOptions: { sourceMap: true } }));
  cases.push(tj("module-name/amd-with-imports", "import { x } from './a';\nexport const y = x;\nexport const p = import('./b');\n", { moduleName: "my/mod", compilerOptions: { module: "amd" } }));
  cases.push(tj("module-name/umd", "export const a = 1;\n", { moduleName: "my/mod", compilerOptions: { module: "umd" } }));
  cases.push(tj("module-name/empty-string-ignored", "export const a = 1;\n", { moduleName: "", compilerOptions: { module: "amd" } }));
  cases.push(tj("module-name/pragma-overridden", "/// <amd-module name=\"pragma/name\" />\nexport const a = 1;\n", { moduleName: "api/name", compilerOptions: { module: "amd" } }));
  cases.push(tj("renamed/amd", "import { x } from './a';\nexport const y = x;\n", { renamedDependencies: { "./a": "./renamed-a" }, compilerOptions: { module: "amd" } }));
  cases.push(tj("renamed/system", "import { x } from './a';\nexport const y = x;\n", { renamedDependencies: { "./a": "./renamed-a" }, compilerOptions: { module: "system" } }));
  cases.push(tj("renamed/umd", "import { x } from './a';\nexport const y = x;\n", { renamedDependencies: { "./a": "./renamed-a" }, compilerOptions: { module: "umd" } }));
  cases.push(tj("renamed/dynamic-import", "export const p = import('./a');\nexport const q = import('./unrenamed');\n", { renamedDependencies: { "./a": "./renamed-a" }, compilerOptions: { module: "commonjs" } }));
  cases.push(tj("renamed/dynamic-import-system", "export const p = import('./a');\n", { renamedDependencies: { "./a": "./renamed-a" }, compilerOptions: { module: "system" } }));
  cases.push(tj("renamed/export-from", "export { x } from './a';\nexport * from './b';\nimport c = require('./c');\nexport const d = c;\n", { renamedDependencies: { "./a": "./renamed-a", "./b": "./renamed-b", "./c": "./renamed-c" }, compilerOptions: { module: "commonjs" } }));
  cases.push(tj("renamed/type-only-elided", "import type { T } from './a';\nexport const v: T = null!;\n", { renamedDependencies: { "./a": "./renamed-a" }, compilerOptions: { module: "commonjs" } }));
  cases.push(tj("jsdoc/parse-none-js", "/** @type {number} */\nexport const a = 1;\n", { fileName: "module.js", jsDocParsingMode: "ParseNone" }));
  cases.push(tj("jsdoc/parse-for-type-info-ts", "/** @deprecated */\nexport const a = 1;\n", { jsDocParsingMode: "ParseForTypeInfo" }));
  // --- transpile-dts
  cases.push(td("empty/no-input", ""));
  cases.push(td("basic/exports", "export const a = 1;\nexport function f(x: number): string { return ''; }\nexport class C { m(): void {} private p = 1; }\nexport interface I { x: number }\nexport type T = I | C;\n"));
  cases.push(td("basic/script-globals", "const a = 1;\nfunction f(): void {}\ndeclare const g: number;\n"));
  cases.push(td("diag/isolated-inference-error", "export function f() { return 1; }\nexport const o = { a: 1, b() { return 2; } };\n", { reportDiagnostics: true }));
  cases.push(td("diag/isolated-inference-error-report-off", "export function f() { return 1; }\n", { reportDiagnostics: false }));
  cases.push(td("diag/isolated-inference-error-report-absent", "export function f() { return 1; }\n"));
  cases.push(td("diag/syntax-error-report-on", "export const a: = 1;\n", { reportDiagnostics: true }));
  cases.push(td("diag/semantic-only-error", "export const x: number = 'string';\n", { reportDiagnostics: true }));
  cases.push(td("diag/option-invalid-enum-string", "export const a = 1;\n", { reportDiagnostics: true, compilerOptions: { target: "es9999" } }));
  cases.push(td("diag/option-conflict-verbatim-commonjs", "import { A } from './a';\nexport let v: A;\n", { reportDiagnostics: true, compilerOptions: { module: "commonjs", verbatimModuleSyntax: true } }));
  cases.push(td("maps/declaration-map", "export const a: number = 1;\nexport function f(x: string): string { return x; }\n", { compilerOptions: { declarationMap: true } }));
  cases.push(td("maps/declaration-map-nested-filename", "export const a = 1;\n", { fileName: "src/deep/a.ts", compilerOptions: { declarationMap: true } }));
  cases.push(td("maps/source-map-ignored", "export const a = 1;\n", { compilerOptions: { sourceMap: true, inlineSourceMap: true } }));
  cases.push(td("kinds/js-input", "export const a = 1;\n/** @type {number} */\nexport let b = 2;\n", { fileName: "module.js" }));
  cases.push(td("kinds/js-input-allow-js", "export const a = 1;\n/** @type {number} */\nexport let b = 2;\n", { fileName: "module.js", compilerOptions: { allowJs: true } }));
  cases.push(td("kinds/tsx", JSX, { compilerOptions: { jsx: "react" } }));
  cases.push(td("filename/absent", "export const a = 1;\n"));
  cases.push(td("filename/nonstandard-extension", "export const a: number = 1;\n", { fileName: "module.txt" }));
  cases.push(td("filename/declaration-input-throws", "export declare const a: number;\n", { fileName: "module.d.ts" }));
  cases.push(td("filename/mts", "export const a = 1;\n", { fileName: "module.mts", compilerOptions: { module: "node20" } }));
  cases.push(td("globals/barebones-lib", "export const arr: Array<string> = [];\nexport const sym: symbol = Symbol();\nexport const len = arr.length;\nexport const p: Promise<number> = null!;\n", { reportDiagnostics: true }));
  cases.push(td("modules/imports", IMPORT_ELISION, { compilerOptions: { module: "commonjs" } }));
  cases.push(td("modules/export-star-and-default", "export * from './a';\nexport * as b from './b';\nexport default function (): void {}\n"));
  cases.push(td("modules/import-type-and-typeof", "import type { A } from './a';\nimport { B } from './b';\nexport type X = A;\nexport const y: typeof B = B;\nexport const z: import('./c').C = null!;\n"));
  cases.push(td("lang/enum-namespace", ENUM_NS));
  cases.push(td("lang/class-fields", CLASS_FIELDS, { compilerOptions: { target: "es2022" } }));
  cases.push(td("lang/class-heritage-and-accessors", "export class B<T> { get v(): T { return null!; } set v(x: T) {} }\nexport class D extends B<string> implements I { constructor(public x: number, private readonly y?: string) { super(); } }\ninterface I {}\n"));
  cases.push(td("lang/decorators", DECORATORS, { compilerOptions: { experimentalDecorators: true } }));
  cases.push(td("lang/overloads-and-generics", "export function f(x: string): string;\nexport function f(x: number): number;\nexport function f(x: any): any { return x; }\nexport function g<T extends object = {}>(x: T): T { return x; }\n"));
  cases.push(td("lang/const-assertions", "export const t = [1, 'a'] as const;\nexport const o = { a: 1 } as const;\nexport const n = 1 as number;\n"));
  cases.push(td("lang/expando-function", "export function f() {}\nf.prop = 1;\n"));
  cases.push(td("text/crlf", "export const a: number = 1;\r\nexport const b: string = '';\r\n"));
  cases.push(td("text/crlf-newline-option", "export const a = 1;\n", { compilerOptions: { newLine: "crlf" } }));
  cases.push(td("text/unicode", "export const s = 'é😀漢字';\nexport const \\u{1F600}x: '😀' = '😀';\n"));
  cases.push(td("text/comments", "/** doc */\nexport const a = 1; // trailing\n/* internal */ export const b = 2;\n"));
  cases.push(td("options/strip-internal", "/** @internal */ export const a = 1;\nexport const b = 2;\n", { compilerOptions: { stripInternal: true } }));
  cases.push(td("options/remove-comments", "/** doc */\nexport const a = 1;\n", { compilerOptions: { removeComments: true } }));
  cases.push(td("options/declaration-false-forced-true", "export const a = 1;\n", { reportDiagnostics: true, compilerOptions: { declaration: false, emitDeclarationOnly: false, isolatedDeclarations: false } }));
  cases.push(td("options/no-lib-true-forced-false", "export const a: Array<number> = [];\n", { reportDiagnostics: true, compilerOptions: { noLib: true } }));
  cases.push(td("options/no-check-false-forced-true", "export const x: number = 'a';\n", { reportDiagnostics: true, compilerOptions: { noCheck: false } }));
  cases.push(td("options/unknown-option", "export const a = 1;\n", { reportDiagnostics: true, compilerOptions: { noSuchOption: true } }));
  cases.push(td("repeat/first", "export const first = 1;\n"));
  cases.push(td("repeat/second", "export const second = 2;\n"));
  cases.push(td("filename/extensionless", "export const a: number = 1;\n", { fileName: "module" }));
  cases.push(td("module-name/dts-ignored", "export const a = 1;\n", { moduleName: "my/mod", compilerOptions: { module: "amd" } }));
  cases.push(td("renamed/dts-ignored", "import { x } from './a';\nexport const y: typeof x = x;\n", { renamedDependencies: { "./a": "./renamed-a" }, compilerOptions: { module: "commonjs" } }));
  cases.push(td("jsdoc/parse-none-js-dts", "/** @type {number} */\nexport const a = 1;\n/** @param {string} s */\nexport function f(s) { return s; }\n", { fileName: "module.js", compilerOptions: { allowJs: true }, jsDocParsingMode: "ParseNone" }));
  cases.push(td("jsdoc/parse-all-js-dts", "/** @type {number} */\nexport const a = 1;\n/** @param {string} s */\nexport function f(s) { return s; }\n", { fileName: "module.js", compilerOptions: { allowJs: true }, jsDocParsingMode: "ParseAll" }));
  cases.push(td("jsdoc/parse-for-type-errors-ts", "/** @deprecated {@link f} */\nexport const a = 1;\n", { jsDocParsingMode: "ParseForTypeErrors" }));
  // --- program-no-check
  const SEMANTIC = { "/project/src/a.ts": "const x: number = 'string';\nexport const y: string = x;\n" };
  const SYNTAX = { "/project/src/a.ts": "let x: = 1;\nexport {};\n" };
  cases.push(pn("semantic-error/emits", SEMANTIC, {}));
  cases.push(pn("semantic-error/control-checked", SEMANTIC, { noCheck: false }));
  cases.push(pn("semantic-error/control-checked-absent", SEMANTIC, { noCheck: undefined }));
  cases.push(pn("syntax-error/still-emits", SYNTAX, {}));
  cases.push(pn("no-emit-on-error/semantic", SEMANTIC, { noEmitOnError: true }));
  cases.push(pn("no-emit-on-error/syntax", SYNTAX, { noEmitOnError: true }));
  cases.push(pn("no-emit-on-error/declaration-error", { "/project/src/a.ts": "class Private {}\nexport const v = new Private();\nexport function f() { return new Private(); }\n" }, { noEmitOnError: true, declaration: true }));
  cases.push(pn("no-emit/plain", SEMANTIC, { noEmit: true }));
  cases.push(pn("no-emit/declaration-getter", { "/project/src/a.ts": "class Private {}\nexport const v = new Private();\n" }, { noEmit: true, declaration: true }));
  cases.push(pn("declaration/clean", { "/project/src/a.ts": "export const a: number = 1;\nexport function f(x: string): string { return x; }\n" }, { declaration: true }));
  cases.push(pn("declaration/inferred-types", { "/project/src/a.ts": "export const a = 1;\nexport function f(x: string) { return { x, n: [1, 'a'] }; }\nexport class C { m() { return this; } }\n" }, { declaration: true }));
  cases.push(pn("declaration/declaration-error-blocks-dts", { "/project/src/a.ts": "class Private {}\nexport const v = new Private();\n" }, { declaration: true }));
  cases.push(pn("declaration/declaration-map", { "/project/src/a.ts": "export const a: number = 1;\n" }, { declaration: true, declarationMap: true }));
  cases.push(pn("isolated-declarations/error", { "/project/src/a.ts": "export function f() { return 1; }\nexport const ok: number = 1;\n" }, { declaration: true, isolatedDeclarations: true }));
  cases.push(pn("isolated-declarations/clean", { "/project/src/a.ts": "export function f(): number { return 1; }\n" }, { declaration: true, isolatedDeclarations: true }));
  cases.push(pn("js-declaration/allow-js", { "/project/src/a.js": "export const a = 1;\n/** @param {number} x */\nexport function f(x) { return x; }\n" }, { allowJs: true, declaration: true, outDir: "/project/out" }));
  cases.push(pn("js-declaration/check-js-error", { "/project/src/a.js": "// @ts-check\nexport const a = 1;\n/** @type {number} */\nexport const b = 'string';\n" }, { allowJs: true, checkJs: true, declaration: true, outDir: "/project/out" }));
  cases.push(pn("import-elision/cross-file", { "/project/src/dep.ts": "export class A {}\nexport interface B {}\nexport type C = number;\nexport type T = string;\nexport = A;\n".replace("export = A;\n", ""), "/project/src/main.ts": IMPORT_ELISION }, { module: "commonjs" }));
  cases.push(pn("import-elision/cross-file-esnext", { "/project/src/dep.ts": "export class A {}\nexport interface B {}\nexport type C = number;\nexport type T = string;\n", "/project/src/main.ts": IMPORT_ELISION.replace('import D = require("./dep");\n', "") }, { module: "esnext" }));
  cases.push(pn("import-elision/cross-file-control-checked", { "/project/src/dep.ts": "export class A {}\nexport interface B {}\nexport type C = number;\nexport type T = string;\n", "/project/src/main.ts": IMPORT_ELISION }, { module: "commonjs", noCheck: false }));
  cases.push(pn("import-elision/ts-nocheck-directive", { "/project/src/dep.ts": "export class A {}\nexport interface B {}\n", "/project/src/main.ts": "// @ts-nocheck\nimport { A, B } from './dep';\nlet t: B;\nexport const v = new A();\n" }, { module: "commonjs", noCheck: false }));
  cases.push(pn("import-elision/unresolved", { "/project/src/main.ts": "import { A } from 'missing';\nimport { B } from './also-missing';\nlet t: A;\nexport const v = new B();\n" }, { module: "commonjs" }));
  cases.push(pn("import-elision/reexports", { "/project/src/dep.ts": "export class A {}\nexport interface B {}\n", "/project/src/main.ts": "import { A, B } from './dep';\nexport { A, B };\nexport { A as A2 } from './dep';\nexport * from './dep';\nexport type { B as B2 } from './dep';\n" }, { module: "commonjs" }));
  cases.push(pn("import-elision/import-equals-export", { "/project/src/dep.ts": "export namespace N { export const x = 1; export type T = string; }\n", "/project/src/main.ts": "import { N } from './dep';\nimport X = N.x;\nimport T = N.T;\nexport import Y = N.x;\nexport const v: T = '' + X;\n" }, { module: "commonjs" }));
  cases.push(pn("enum/const-enum-cross-file", { "/project/src/e.ts": "export const enum E { A = 1, B = A * 2, S = 'str' }\n", "/project/src/main.ts": "import { E } from './e';\nexport const v = E.B + E.A;\nexport const s = E.S;\nexport const w = E['A'];\n" }, { module: "commonjs" }));
  cases.push(pn("enum/const-enum-same-file-es5", { "/project/src/main.ts": ENUM_NS }, { module: "commonjs", target: "es5", ignoreDeprecations: "6.0" }));
  cases.push(pn("flags/es5-loop-capture", { "/project/src/main.ts": ES5_FLAGS }, { target: "es5", ignoreDeprecations: "6.0" }));
  cases.push(pn("flags/es5-loop-capture-commonjs", { "/project/src/main.ts": ES5_FLAGS + "export {};\n" }, { target: "es5", module: "commonjs", ignoreDeprecations: "6.0" }));
  cases.push(pn("flags/es2015-async-super", { "/project/src/main.ts": ES5_FLAGS }, { target: "es2015" }));
  cases.push(pn("flags/es2017-static-block", { "/project/src/main.ts": ES5_FLAGS }, { target: "es2017" }));
  cases.push(pn("flags/es5-class-fields-decorators", { "/project/src/main.ts": CLASS_FIELDS + DECORATORS }, { target: "es5", experimentalDecorators: true, ignoreDeprecations: "6.0" }));
  cases.push(pn("decorators/metadata-cross-file", { "/project/src/dep.ts": "export class Dep {}\nexport interface I {}\nexport type U = string | number;\nexport enum En { A }\n", "/project/src/main.ts": "import { Dep, I, U, En } from './dep';\nfunction dec(...a: any[]) {}\n@dec export class C {\n  constructor(private d: Dep, i: I, u: U, e: En, n: number, s?: string, f?: () => void) {}\n  @dec m(@dec p: Dep): Promise<I> { return null!; }\n  @dec prop: En = En.A;\n}\n" }, { experimentalDecorators: true, emitDecoratorMetadata: true, target: "es2017", module: "commonjs" }));
  cases.push(pn("decorators/standard-esnext", { "/project/src/main.ts": DECORATORS }, { target: "esnext" }));
  cases.push(pn("jsx/react", { "/project/src/main.tsx": JSX }, { jsx: "react", module: "commonjs" }));
  cases.push(pn("jsx/react-jsx", { "/project/src/main.tsx": JSX }, { jsx: "react-jsx", module: "esnext" }));
  cases.push(pn("maps/source-map", { "/project/src/a.ts": "export const a: number = 1;\n" }, { sourceMap: true, outDir: "/project/out" }));
  cases.push(pn("maps/inline-source-map", { "/project/src/a.ts": "export const a: number = 1;\n" }, { inlineSourceMap: true }));
  cases.push(pn("no-lib/global-diagnostics", { "/project/src/a.ts": "export const a: number = 1;\n" }, { noLib: true }));
  cases.push(pn("no-lib/global-diagnostics-control-checked", { "/project/src/a.ts": "export const a: number = 1;\n" }, { noLib: true, noCheck: false }));
  cases.push(pn("options/conflict-emit-declaration-only", { "/project/src/a.ts": "export const a = 1;\n" }, { emitDeclarationOnly: true }));
  cases.push(pn("options/list-emitted-files", { "/project/src/a.ts": "export const a = 1;\n" }, { listEmittedFiles: true, declaration: true }));
  cases.push(pn("sink/on-error-javascript", { "/project/src/a.ts": "export const a = 1;\n" }, { declaration: true }, { sink_rules: [{ path: "/project/src/a.js", action: "on-error" }] }));
  cases.push(pn("sink/on-error-declaration", { "/project/src/a.ts": "export const a = 1;\n" }, { declaration: true }, { sink_rules: [{ path: "/project/src/a.d.ts", action: "on-error" }] }));
  // checked controls for rows whose noCheck output differed natively:
  // the same inputs under the ordinary Program route separate inherited
  // emitter differences from H2.8c-specific ones.
  cases.push(pn("control-checked/es5-flags", { "/project/src/main.ts": ES5_FLAGS }, { target: "es5", ignoreDeprecations: "6.0", noCheck: false }));
  cases.push(pn("control-checked/es2015-flags", { "/project/src/main.ts": ES5_FLAGS }, { target: "es2015", noCheck: false }));
  cases.push(pn("control-checked/import-helpers", { "/project/src/main.ts": "export class A extends Object {}\nexport const {a, ...r} = {a: 1};\n" }, { target: "es5", module: "commonjs", importHelpers: true, ignoreDeprecations: "6.0", noCheck: false }));
  cases.push(pn("control-checked/namespace-source-map", { "/project/src/namespace.ts": "export namespace ns {\n    namespace internal {\n        export class Foo {}\n    }\n    export namespace nested {\n        export import inner = internal;\n    }\n}\n" }, { target: "es2015", sourceMap: true, noCheck: false }));
  cases.push(pn("no-check/import-helpers", { "/project/src/main.ts": "export class A extends Object {}\nexport const {a, ...r} = {a: 1};\n" }, { target: "es5", module: "commonjs", importHelpers: true, ignoreDeprecations: "6.0" }));
  cases.push(pn("no-check/namespace-source-map", { "/project/src/namespace.ts": "export namespace ns {\n    namespace internal {\n        export class Foo {}\n    }\n    export namespace nested {\n        export import inner = internal;\n    }\n}\n" }, { target: "es2015", sourceMap: true }));
  cases.push(pn("text/crlf-unicode", { "/project/src/a.ts": "export const s = 'é😀漢字';\r\nexport const t: number = 1;\r\n" }, { target: "es5", newLine: "crlf" }));
  cases.push(pn("multi/three-files-mixed", { "/project/src/a.ts": "export const a: number = 'x' as any;\n", "/project/src/b.ts": "import { a } from './a';\nexport const b = a + 1;\n", "/project/src/c.ts": "import type { b } from './b';\nexport {};\n" }, { module: "commonjs", declaration: true }));
  const numericSource = "export class C { #value = 1; value: number = this.#value; }\nexport const read = async (value?: C) => value?.value ?? 0;\n";
  for (const target of [99, 100, 1234])
    cases.push(tj(`numeric-target/transform-${target}`,
      target === 100 ? `declare const dec: any;\n${numericSource.replace('export class', '@dec export class')}` : numericSource,
      { reportDiagnostics: true, compilerOptions: { target, module: "esnext", sourceMap: true,
        ...(target === 100 ? { useDefineForClassFields: false } : {}) } }));
  cases.push(td("numeric-target/declaration-1234", "export const value: number = 1;\nexport class C { value: number = 1; }\n",
    { reportDiagnostics: true, compilerOptions: { target: 1234, declarationMap: true } }));
  return cases;
}

function manifestCases() {
  const cases = [...inventoryCases(), ...extraCases()];
  const ids = new Set();
  for (const entry of cases) {
    requireCondition(!ids.has(entry.id), `duplicate case id ${entry.id}`);
    ids.add(entry.id);
  }
  return cases;
}

function inputRecord(entry) {
  const record = { ...entry };
  if (record.route !== "program-no-check") {
    record.text_sha256 = sha256(utf8(record.text));
    record.text_utf16_units = record.text.length;
  } else {
    record.files = record.files.map((file) => ({ ...file, text_sha256: sha256(utf8(file.text)) }));
  }
  return record;
}

// Review metadata may contain isolated UTF-16 units in internal host traces.
function losslessRecord(value) {
  if (typeof value === "string" && !value.isWellFormed()) {
    return { utf16: Array.from({ length: value.length }, (_, index) => value.charCodeAt(index)) };
  }
  if (Array.isArray(value)) return value.map(losslessRecord);
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, losslessRecord(item)]));
  return value;
}

function observe(entry) {
  entry = { ...entry };
  if (entry.moduleNameUtf16) entry.moduleName = String.fromCharCode(...entry.moduleNameUtf16);
  if (entry.renamedDependenciesUtf16) {
    entry.renamedDependencies = Object.fromEntries(entry.renamedDependenciesUtf16.map(
      ([from, to]) => [String.fromCharCode(...from), String.fromCharCode(...to)]));
  }
  if (entry.route === "program-no-check") {
    const { public: pub, internal } = programCommand(entry);
    return { id: entry.id, route: entry.route, observation: pub, internal };
  }
  const pub = transpileCall(entry);
  const replica = replicaTranspile(entry);
  const replicaMatches = JSON.stringify(replica.public) === JSON.stringify(pub);
  return {
    id: entry.id,
    route: entry.route,
    observation: pub,
    internal: losslessRecord({ ...replica.internal, replica_public_result_matches: replicaMatches }),
  };
}

function main(argv) {
  if (argv.length === 1 && argv[0] === "--check") {
    requireCondition(process.versions.node === fs.readFileSync(path.join(WORKSPACE, ".node-version"), "utf8").trim(), "Node version drift");
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), "transpile-routes-"));
    try {
      for (const prefix of ["", "review-"]) {
        const fixtures = path.join(WORKSPACE, "crates/compiler/tests/fixtures/h2_8c_transpile");
        const expected = fs.readFileSync(path.join(fixtures, `${prefix}expected.v1.json`));
        for (let run = 1; run <= 2; run++) {
          const output = path.join(directory, `${prefix}${run}.json`);
          main(["--inputs", path.join(fixtures, `${prefix}inputs.v1.json`), "--out", output]);
          requireCondition(expected.equals(fs.readFileSync(output)), `${prefix || "original-"}oracle bytes differ (run ${run})`);
        }
      }
    } finally {
      fs.rmSync(directory, { recursive: true, force: true });
    }
    console.log("transpile routes: 291 original + 14 review cases matched twice");
    return;
  }
  const args = new Map();
  for (let i = 0; i < argv.length; i += 2) args.set(argv[i], argv[i + 1]);
  if (args.has("--write-inputs")) {
    const cases = manifestCases().map(inputRecord);
    const summary = {};
    for (const entry of cases) summary[entry.route] = (summary[entry.route] ?? 0) + 1;
    const manifest = {
      schema: "h2-8c-transpile-inputs.v1",
      typescript: { version: "6.0.3", bundle: "vendor/typescript-6.0.3/lib/typescript.js", bundle_sha256: EXPECTED_BUNDLE_SHA256 },
      generator: "scripts/observe-transpile-routes.mjs",
      case_count: cases.length,
      route_counts: summary,
      cases,
    };
    fs.writeFileSync(args.get("--write-inputs"), JSON.stringify(manifest, null, 1) + "\n");
    console.log(`wrote ${cases.length} inputs`, summary);
    return;
  }
  const inputs = JSON.parse(fs.readFileSync(args.get("--inputs"), "utf8"));
  requireCondition(inputs.schema === "h2-8c-transpile-inputs.v1", "unknown inputs schema");
  requireCondition(inputs.cases.length === inputs.case_count && inputs.cases.length > 0, "case count drift");
  requireCondition(new Set(inputs.cases.map((entry) => entry.id)).size === inputs.cases.length, "duplicate case id");
  const results = [];
  for (const entry of inputs.cases) {
    results.push(observe(entry));
  }
  // second-call state separation: repeat/* cases are observed twice more in
  // sequence and must match their own first observation.
  const repeats = inputs.cases.filter((entry) => entry.id.includes("/repeat/"));
  const repeatMatches = repeats.map((entry) => {
    const first = results.find((r) => r.id === entry.id).observation;
    const again = observe(entry).observation;
    return { id: entry.id, matches: JSON.stringify(first) === JSON.stringify(again) };
  });
  const expected = {
    schema: "h2-8c-transpile-expected.v1",
    inputs_sha256: sha256(fs.readFileSync(args.get("--inputs"))),
    typescript: inputs.typescript,
    node: process.version,
    case_count: results.length,
    repeat_state_separation: repeatMatches,
    cases: results,
    ...(inputs.cases.some(entry => entry.id.includes('/numeric-target/')) ? {
      numeric_target_default_libraries: [99, 100, 1234].map(target => ({ target, name: ts.getDefaultLibFileName({ target }) }))
    } : {}),
  };
  requireCondition(repeatMatches.every((entry) => entry.matches), "repeat state separation failed");
  requireCondition(results.every((entry) => entry.internal.replica_public_result_matches !== false), "replica/public mismatch");
  fs.writeFileSync(args.get("--out"), JSON.stringify(expected, null, 1) + "\n");
  const exceptions = results.filter((r) => r.observation.exception).length;
  const replicaMismatch = results.filter((r) => r.internal.replica_public_result_matches === false).map((r) => r.id);
  console.log(`observed ${results.length} cases; exceptions=${exceptions}; replica mismatches=${replicaMismatch.length}`, replicaMismatch);
}

main(process.argv.slice(2));
