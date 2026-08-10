import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5c-owner-controls.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5c-owner-controls.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-5c-owner-controls.schema.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";

function options(extra = {}) {
  return {
    target: ts.ScriptTarget.ES2019,
    module: ts.ModuleKind.ESNext,
    alwaysStrict: true,
    useDefineForClassFields: true,
    newLine: ts.NewLineKind.LineFeed,
    ignoreDeprecations: "6.0",
    ...extra,
  };
}

function control(controlId, source, extra = {}) {
  return {
    control_id: controlId,
    files: [{ path: "/project/input.ts", text: source }],
    roots: ["/project/input.ts"],
    compiler_options: options(extra.compiler_options),
    expected_paths: extra.expected_paths ?? ["/project/input.js"],
    expected_diagnostic_codes: extra.expected_diagnostic_codes ?? [],
    expected_emit_diagnostic_codes: extra.expected_emit_diagnostic_codes ?? [],
    runtime_expectation: extra.runtime_expectation ?? {},
  };
}

// These controls are deliberately owner-shaped rather than corpus-shaped.
// They freeze optional-chain receiver/evaluation behavior, nullish ordering,
// generated-name ownership, parameter-hoist conversion, and composition with
// every already-closed transform that runs before transformES2020.
const CONTROLS = Object.freeze([
  control(
    "identifier-property-element-and-nullish",
    "declare let value: any, key: any, fallback: any;\nvalue?.property;\nvalue?.[key];\nvalue ?? fallback;\n",
  ),
  control(
    "effectful-chain-roots-and-keys",
    "declare function getObject(): any; declare function getKey(): any;\ngetObject()?.value;\ngetObject()?.[getKey()];\ngetObject()[getKey()]?.value;\n",
  ),
  control(
    "optional-call-receiver-matrix",
    "declare let object: any; declare function getObject(): any; declare function getKey(): any;\nobject.method?.(1);\nobject[getKey()]?.();\ngetObject().method?.();\nobject?.method();\n",
  ),
  control(
    "nested-optional-chain-receiver-state",
    "declare let object: any;\nobject?.method?.();\nobject?.method().nested?.();\nobject.property?.().nested?.value;\n",
  ),
  control(
    "parenthesized-optional-chain-calls",
    "declare let object: any; declare function fallback(): any;\n(object?.method)(fallback());\n(object?.method.nested)();\n(object?.[\"method\"])();\n",
  ),
  control(
    "super-property-and-element-optional-calls",
    "declare function getKey(): string;\nclass Base { method?(): void {} [key: string]: any; }\nclass Derived extends Base { run() { super.method?.(); super[getKey()]?.(); } }\n",
  ),
  control(
    "optional-delete-chain-matrix",
    "declare let object: any; declare function getKey(): any;\ndelete object?.property;\ndelete (object?.property.nested);\ndelete object.property?.[getKey()];\n",
  ),
  control(
    "typescript-outer-expression-restoration",
    "declare let object: any;\n(object.method as any)?.();\n(<any>object.method)?.();\nobject!?.method?.();\n(object?.method as any).length;\n",
  ),
  control(
    "nullish-coalescing-order-and-nesting",
    "declare let value: any, fallback: any; declare function getValue(): any; declare function getFallback(): any;\nvalue ?? fallback;\ngetValue() ?? getFallback();\n(value ?? getValue()) ?? getFallback();\nvalue?.property ?? getFallback();\n",
  ),
  control(
    "generated-name-collisions-and-reserved-loop-slots",
    "declare function getValue(): any;\nlet _a = 1, _b = 2;\ngetValue() ?? 0; getValue() ?? 1; getValue() ?? 2; getValue() ?? 3; getValue() ?? 4; getValue() ?? 5; getValue() ?? 6; getValue() ?? 7; getValue() ?? 8; getValue() ?? 9; getValue() ?? 10; getValue() ?? 11; getValue() ?? 12; getValue() ?? 13; getValue() ?? 14; getValue() ?? 15;\n",
  ),
  control(
    "source-and-function-scope-temp-reuse",
    "declare function getObject(): any;\ngetObject()?.value;\nfunction first() { return getObject()?.value; }\nfunction second() { return getObject() ?? 0; }\n",
  ),
  control(
    "concise-arrow-function-hoist",
    "declare function getObject(): any;\nconst read = () => getObject()?.value;\n",
  ),
  control(
    "identifier-and-binding-pattern-parameter-hoists",
    "declare function getObject(): any; declare function getFallback(): any;\nfunction first(value = getObject()?.property) { return value; }\nconst second = ({ value } = getObject()?.property, other = getFallback() ?? 0) => value;\n",
  ),
  control(
    "nested-binding-default-owns-function-temps",
    "declare function getObject(): any;\nfunction read({ value = getObject()?.property }) { return value; }\n",
  ),
  control(
    "nested-function-in-parameter-keeps-outer-default",
    "declare function getObject(): any;\nfunction read(value = (() => getObject()?.property)()) { return value; }\n",
  ),
  control(
    "es2021-logical-assignment-composition",
    "declare let object: any; declare function getObject(): any; declare function rhs(): any;\nobject.property ??= getObject()?.value ?? rhs();\ngetObject().property ||= object?.value;\n",
  ),
  control(
    "class-field-and-method-composition",
    "declare function getObject(): any;\nclass Value { field = getObject()?.value ?? 0; read() { return this.field?.value; } }\n",
  ),
  control(
    "private-field-composition",
    "class Value { #field: any; read() { return this.#field?.value ?? 0; } }\n",
  ),
  control(
    "standard-decorator-and-field-composition",
    "declare function decorate(value: any, context: any): any; declare function getObject(): any;\nclass Value { @decorate field = getObject()?.value ?? 0; }\n",
  ),
  control(
    "legacy-decorator-and-field-composition",
    "declare function decorate(...values: any[]): any; declare function getObject(): any;\n@decorate class Value { @decorate field = getObject()?.value ?? 0; }\n",
    { compiler_options: { experimentalDecorators: true } },
  ),
  control(
    "enum-namespace-and-parameter-property-composition",
    "declare function getObject(): any;\nenum Kind { First }\nnamespace State { export const value = getObject()?.value ?? 0; }\nclass Value { constructor(public field = getObject()?.value ?? 0) {} }\n",
    {
      runtime_expectation: {
        h2_2a_sources: 1,
        h2_2b_sources: 1,
        h2_2c_sources: 1,
      },
    },
  ),
  control(
    "commonjs-export-composition",
    "declare function getObject(): any;\nexport const value = getObject()?.value ?? 0;\nexport function read() { return getObject()?.nested?.value; }\n",
    { compiler_options: { module: ts.ModuleKind.CommonJS } },
  ),
  control(
    "parenthesized-and-commented-chain",
    "declare function getObject(): any; declare function getKey(): any;\n((getObject() /* receiver */))?.value;\ngetObject()?.[/* key */ getKey()];\n(getObject()?.value /* result */)?.nested;\n",
  ),
  control(
    "adjacent-es2020-preserves-optional-and-nullish",
    "declare let object: any, fallback: any;\nobject?.value;\nobject ?? fallback;\n",
    { compiler_options: { target: ts.ScriptTarget.ES2020 } },
  ),
  control(
    "ordinary-es2019-source-is-stable",
    "export const value = 1;\n",
  ),
  control(
    "no-emit-on-error-stops-before-write",
    "missing?.value;\n",
    {
      compiler_options: { noEmitOnError: true },
      expected_paths: [],
      expected_diagnostic_codes: [2304],
      expected_emit_diagnostic_codes: [2304],
    },
  ),
]);

function fail(message) {
  throw new Error(message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function withFingerprint(value, field) {
  return { ...value, [field]: sha256(Buffer.from(canonical(value), "utf8")) };
}

function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function pathHash(relativePath) {
  return { path: relativePath, sha256: sha256(readBytes(relativePath)) };
}

function bytesRecord(text) {
  const bytes = Buffer.from(text, "utf8");
  return {
    utf8_base64: bytes.toString("base64"),
    utf8_sha256: sha256(bytes),
    utf8_bytes: bytes.length,
  };
}

function validateRuntime() {
  const node = readBytes(".node-version").toString("utf8").trim();
  requireCondition(node === EXPECTED_NODE, "unexpected Node pin");
  requireCondition(process.version === `v${node}`, `requires Node ${node}`);
  requireCondition(ts.version === "6.0.3", "unexpected TypeScript runtime");
}

function hasDirectory(files, directory) {
  const prefix = `${ts.normalizePath(directory).replace(/\/$/, "")}/`;
  return [...files.keys()].some((fileName) => fileName.startsWith(prefix));
}

function createVirtualProgram(control_) {
  const files = new Map(control_.files.map((file) => [file.path, file.text]));
  const baseHost = ts.createCompilerHost(control_.compiler_options, true);
  const host = {
    ...baseHost,
    getCurrentDirectory: () => "/project",
    useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: (fileName) => fileName,
    trace() {},
    fileExists(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.has(normalized) || baseHost.fileExists(normalized);
    },
    readFile(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.get(normalized) ?? baseHost.readFile(normalized);
    },
    directoryExists(directory) {
      return hasDirectory(files, directory) ||
        (baseHost.directoryExists?.(directory) ?? false);
    },
    getDirectories(directory) {
      return baseHost.getDirectories?.(directory) ?? [];
    },
    realpath(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.has(normalized)
        ? normalized
        : (baseHost.realpath?.(normalized) ?? normalized);
    },
    getSourceFile(fileName, languageVersion) {
      const normalized = ts.normalizePath(fileName);
      const text = files.get(normalized);
      if (text === undefined) return baseHost.getSourceFile(fileName, languageVersion);
      return ts.createSourceFile(
        normalized,
        text,
        languageVersion,
        true,
        ts.getScriptKindFromFileName(normalized),
      );
    },
  };
  return ts.createProgram(control_.roots, control_.compiler_options, host);
}

function flattenMessage(messageText, indent = 0) {
  if (typeof messageText === "string") return messageText;
  const children = messageText.next ?? [];
  return messageText.messageText + children
    .map((child) => `\n${"  ".repeat(indent + 1)}${flattenMessage(child, indent + 1)}`)
    .join("");
}

function serializeDiagnostic(diagnostic) {
  return {
    code: diagnostic.code,
    category: ts.DiagnosticCategory[diagnostic.category],
    file: diagnostic.file?.fileName ?? null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    message: flattenMessage(diagnostic.messageText),
  };
}

function serializeWrite(arguments_) {
  const [fileName, text, writeByteOrderMark, onError, sourceFiles] = arguments_;
  const callback = Buffer.from(text, "utf8");
  const materialized = writeByteOrderMark
    ? Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), callback])
    : callback;
  return {
    index: 0,
    path: ts.normalizePath(fileName),
    kind: fileName.endsWith(".js") ? "javascript" : "other",
    callback_utf8_base64: callback.toString("base64"),
    callback_utf8_sha256: sha256(callback),
    callback_utf8_bytes: callback.length,
    write_byte_order_mark: writeByteOrderMark,
    materialized_utf8_sha256: sha256(materialized),
    materialized_utf8_bytes: materialized.length,
    on_error_callback_present: typeof onError === "function",
    source_files: (sourceFiles ?? []).map((source) => source.fileName),
  };
}

function observe(control_) {
  const program = createVirtualProgram(control_);
  const writes = [];
  const reported = ts.getPreEmitDiagnostics(program);
  const emitResult = program.emit(
    undefined,
    (...arguments_) => writes.push(arguments_),
  );
  return withFingerprint(
    {
      reported_diagnostics: reported.map(serializeDiagnostic),
      emit_diagnostics: emitResult.diagnostics.map(serializeDiagnostic),
      emit_skipped: emitResult.emitSkipped,
      writes: writes.map((write, index) => ({ ...serializeWrite(write), index })),
    },
    "run_fingerprint_sha256",
  );
}

function serializeOptions(options_) {
  return Object.fromEntries(
    Object.entries(options_).sort(([left], [right]) => left.localeCompare(right)),
  );
}

function sourceContainsDecorator(file) {
  const source = ts.createSourceFile(
    file.path,
    file.text,
    ts.ScriptTarget.ESNext,
    true,
    ts.ScriptKind.TS,
  );
  const stack = [source];
  while (stack.length !== 0) {
    const node = stack.pop();
    if (ts.canHaveDecorators(node) && (ts.getDecorators(node)?.length ?? 0) !== 0) {
      return true;
    }
    ts.forEachChild(node, (child) => {
      stack.push(child);
    });
  }
  return false;
}

function buildControl(control_) {
  const first = observe(control_);
  const second = observe(control_);
  requireCondition(
    first.run_fingerprint_sha256 === second.run_fingerprint_sha256,
    `${control_.control_id} owner control is nondeterministic`,
  );
  requireCondition(
    canonical(first.reported_diagnostics.map((diagnostic) => diagnostic.code)) ===
      canonical(control_.expected_diagnostic_codes) &&
      canonical(first.emit_diagnostics.map((diagnostic) => diagnostic.code)) ===
        canonical(control_.expected_emit_diagnostic_codes) &&
      first.emit_skipped === (control_.expected_paths.length === 0),
    `${control_.control_id} diagnostics or emit result changed: ${JSON.stringify(first)}`,
  );
  requireCondition(
    canonical(first.writes.map((write) => write.path)) ===
      canonical(control_.expected_paths),
    `${control_.control_id} output paths changed`,
  );
  return withFingerprint(
    {
      control_id: control_.control_id,
      input: {
        current_directory: "/project",
        roots: control_.roots,
        files: control_.files.map((file) => ({
          path: file.path,
          root: control_.roots.includes(file.path),
          ...bytesRecord(file.text),
        })),
        compiler_options: serializeOptions(control_.compiler_options),
      },
      runtime_expectation: {
        h2_2a_sources: first.emit_skipped
          ? 0
          : (control_.runtime_expectation.h2_2a_sources ?? 0),
        h2_2b_sources: first.emit_skipped
          ? 0
          : (control_.runtime_expectation.h2_2b_sources ?? 0),
        h2_2c_sources: first.emit_skipped
          ? 0
          : (control_.runtime_expectation.h2_2c_sources ?? 0),
        h2_4a_sources: first.emit_skipped
          ? 0
          : control_.files.filter(
              (file) =>
                control_.compiler_options.experimentalDecorators === true &&
                sourceContainsDecorator(file),
            ).length,
        h2_4b_sources: first.emit_skipped
          ? 0
          : control_.files.filter(
              (file) =>
                control_.compiler_options.useDefineForClassFields === false ||
                (control_.compiler_options.experimentalDecorators !== true &&
                  sourceContainsDecorator(file)),
            ).length,
        h2_5a_sources: first.emit_skipped ||
          control_.compiler_options.target >= ts.ScriptTarget.ESNext
          ? 0
          : control_.files.length,
        h2_5b_sources: first.emit_skipped ||
          control_.compiler_options.target >= ts.ScriptTarget.ES2021
          ? 0
          : control_.files.length,
        h2_5c_sources: first.emit_skipped ||
          control_.compiler_options.target >= ts.ScriptTarget.ES2020
          ? 0
          : control_.files.length,
      },
      repetitions: 2,
      observation: first,
    },
    "control_fingerprint_sha256",
  );
}

function buildArtifact() {
  validateRuntime();
  const controls = CONTROLS.map(buildControl);
  return withFingerprint(
    {
      schema: 1,
      phase: "H2.5c-es2020-target-owner-controls",
      status: "qualified",
      typescript: {
        version: ts.version,
        source_commit: SOURCE_COMMIT,
        bundle: pathHash(TYPESCRIPT_BUNDLE),
        implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
      },
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      controls,
      summary: {
        controls: controls.length,
        exact_outputs: controls.reduce(
          (sum, item) => sum + item.observation.writes.length,
          0,
        ),
        typescript_runs: controls.reduce((sum, item) => sum + item.repetitions, 0),
        reported_diagnostics: controls.reduce(
          (sum, item) => sum + item.observation.reported_diagnostics.length,
          0,
        ),
        emit_diagnostics: controls.reduce(
          (sum, item) => sum + item.observation.emit_diagnostics.length,
          0,
        ),
        no_emit_on_error_controls: controls.filter(
          (item) => item.input.compiler_options.noEmitOnError === true,
        ).length,
        es2019_controls: controls.filter(
          (item) => item.input.compiler_options.target === ts.ScriptTarget.ES2019,
        ).length,
        es2020_controls: controls.filter(
          (item) => item.input.compiler_options.target === ts.ScriptTarget.ES2020,
        ).length,
        optional_chain_controls: CONTROLS.filter((item) =>
          item.files.some((file) => /\?\./.test(file.text)),
        ).length,
        nullish_coalescing_controls: CONTROLS.filter((item) =>
          item.files.some((file) => /\?\?(?!=)/.test(file.text)),
        ).length,
        parameter_hoist_controls: controls.filter((item) =>
          [
            "identifier-and-binding-pattern-parameter-hoists",
            "nested-binding-default-owns-function-temps",
            "nested-function-in-parameter-keeps-outer-default",
          ].includes(item.control_id),
        ).length,
        standard_decorator_controls: CONTROLS.filter((item) =>
          item.compiler_options.experimentalDecorators !== true &&
          item.files.some(sourceContainsDecorator),
        ).length,
        legacy_decorator_controls: CONTROLS.filter(
          (item) => item.compiler_options.experimentalDecorators === true,
        ).length,
        commonjs_controls: controls.filter(
          (item) => item.input.compiler_options.module === ts.ModuleKind.CommonJS,
        ).length,
        h2_5a_active_controls: controls.filter(
          (item) => item.runtime_expectation.h2_5a_sources !== 0,
        ).length,
        h2_5b_active_controls: controls.filter(
          (item) => item.runtime_expectation.h2_5b_sources !== 0,
        ).length,
        h2_5c_active_controls: controls.filter(
          (item) => item.runtime_expectation.h2_5c_sources !== 0,
        ).length,
      },
    },
    "controls_fingerprint_sha256",
  );
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

const artifact = buildArtifact();
const rendered = render(artifact);
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
  process.stdout.write(
    `wrote ${TARGET_RELATIVE_PATH}: controls=${artifact.summary.controls} outputs=${artifact.summary.exact_outputs}\n`,
  );
} else if (mode === "--check") {
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-5c-owner-controls.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5c owner controls are fresh: outputs=${artifact.summary.exact_outputs}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-5c-owner-controls.mjs [--write|--check]");
}
