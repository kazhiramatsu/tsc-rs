import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5a-owner-controls.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5a-owner-controls.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-5a-owner-controls.schema.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const DISPOSABLE_GLOBALS = "interface Disposable {}\ninterface AsyncDisposable {}\n";
const DISPOSABLE_MODULE_GLOBALS =
  "declare global { interface Disposable {} interface AsyncDisposable {} }\n";

function options(extra = {}) {
  return {
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.ESNext,
    alwaysStrict: true,
    useDefineForClassFields: true,
    newLine: ts.NewLineKind.LineFeed,
    ignoreDeprecations: "6.0",
    ...extra,
  };
}

function control(controlId, source, extra = {}) {
  const ownedSource = /\busing\b/.test(source)
    ? `${/\b(?:export|import)\b/.test(source) ? DISPOSABLE_MODULE_GLOBALS : DISPOSABLE_GLOBALS}${source}`
    : source;
  return {
    control_id: controlId,
    files: [{ path: "/project/input.ts", text: ownedSource }],
    roots: ["/project/input.ts"],
    compiler_options: options(extra.compiler_options),
    expected_paths: extra.expected_paths ?? ["/project/input.js"],
    expected_diagnostic_codes: extra.expected_diagnostic_codes ?? [],
    expected_emit_diagnostic_codes: extra.expected_emit_diagnostic_codes ?? [],
    runtime_expectation: extra.runtime_expectation ?? {},
  };
}

// These controls are deliberately owner-shaped rather than corpus-shaped.
// They freeze disposal-scope ownership, generated-name ordering, sync/async
// helper policy, synthesized TypeScript wrappers, every newly accepted target,
// and the class/decorator boundaries that run immediately after transformESNext.
const CONTROLS = Object.freeze([
  control(
    "using-source-block-function-and-namespace-scopes",
    "declare function acquire(): any;\nusing root = acquire();\nfunction nested() { using inner = acquire(); }\n{ using block = acquire(); }\nnamespace N { using member = acquire(); }\n",
    { runtime_expectation: { h2_2b_sources: 1 } },
  ),
  control(
    "generated-name-collisions-remain-scope-local",
    "declare function acquire(): any;\nconst env_1 = 1, e_1 = 2, result_1 = 3;\nusing root = acquire();\nfunction nested() { const env_2 = 4, e_2 = 5, result_2 = 6; using inner = acquire(); }\n",
  ),
  control(
    "await-using-in-async-function",
    "declare function acquire(): any;\nasync function consume() { await using value = acquire(); return value; }\n",
  ),
  control(
    "using-for-initializer",
    "declare function acquire(): any; declare function condition(): boolean; declare function step(): void; declare function consume(value: any): void;\nfor (using value = acquire(); condition(); step()) { consume(value); }\n",
  ),
  control(
    "using-for-of-binding",
    "declare const values: any; declare function consume(value: any): void;\nfor (using value of values) { consume(value); }\n",
  ),
  control(
    "await-using-for-await-of-binding",
    "declare const values: any; declare function consume(value: any): void;\nasync function run() { for await (await using value of values) { consume(value); } }\n",
  ),
  control(
    "no-emit-helpers-retains-disposal-calls",
    "declare function acquire(): any;\nusing value = acquire();\n",
    { compiler_options: { noEmitHelpers: true } },
  ),
  control(
    "commonjs-export-and-disposal-order",
    "declare function acquire(): any;\nexport const before = 1;\nusing value = acquire();\nexport const after = 2;\n",
    { compiler_options: { module: ts.ModuleKind.CommonJS } },
  ),
  control(
    "esnext-preserves-native-using",
    "declare function acquire(): any;\nusing value = acquire();\n",
    { compiler_options: { target: ts.ScriptTarget.ESNext } },
  ),
  control(
    "es2023-selects-esnext-lowering",
    "declare function acquire(): any;\nusing value = acquire();\n",
    { compiler_options: { target: ts.ScriptTarget.ES2023 } },
  ),
  control(
    "es2024-selects-esnext-lowering",
    "declare function acquire(): any;\nusing value = acquire();\n",
    { compiler_options: { target: ts.ScriptTarget.ES2024 } },
  ),
  control(
    "es2025-selects-esnext-lowering",
    "declare function acquire(): any;\nusing value = acquire();\n",
    { compiler_options: { target: ts.ScriptTarget.ES2025 } },
  ),
  control(
    "es2022-auto-accessor-with-native-fields",
    "class C { field = 1; accessor item = 2; #native = 3; static accessor total = 4; }\n",
  ),
  control(
    "es2021-class-fields-assignment-mode",
    "class Base {}\nclass C extends Base { field = 1; empty: unknown; #private = 2; static total = 3; }\n",
    {
      compiler_options: {
        target: ts.ScriptTarget.ES2021,
        useDefineForClassFields: false,
      },
    },
  ),
  control(
    "es2021-class-fields-define-mode",
    "class Base {}\nclass C extends Base { field = 1; empty: unknown; #private = 2; static total = 3; }\n",
    { compiler_options: { target: ts.ScriptTarget.ES2021 } },
  ),
  control(
    "decorator-expression-receiver-binding",
    "declare class Provider { decorate(value: any, context: any): any; }\ndeclare const instance: Provider;\nclass C { @instance.decorate method1() {} @(instance[\"decorate\"]) method2() {} @((instance.decorate)) method3() {} }\n",
  ),
  control(
    "decorator-super-binding-uses-lexical-this",
    "declare class Provider { decorate(value: any, context: any): any; }\nclass D extends Provider { method() { class Nested { @(super.decorate) method1() {} @(super[\"decorate\"]) method2() {} @((super.decorate)) method3() {} } } }\n",
  ),
  control(
    "es2021-static-this-and-super",
    "class Base { static value = 1; static method() {} }\nclass C extends Base { static extra: number; static value = super.value; static { super.method(); this.extra = 2; } }\n",
    { compiler_options: { target: ts.ScriptTarget.ES2021 } },
  ),
  control(
    "parameter-property-and-field-initializer-order",
    "class Base {}\nclass C extends Base { field = this.parameter; constructor(public parameter: number) { super(); } }\n",
    {
      compiler_options: {
        target: ts.ScriptTarget.ES2021,
        useDefineForClassFields: false,
      },
      runtime_expectation: { h2_2c_sources: 1 },
    },
  ),
  control(
    "no-emit-on-error-stops-before-write",
    "using value = missing;\n",
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
        h2_2b_sources: first.emit_skipped
          ? 0
          : (control_.runtime_expectation.h2_2b_sources ?? 0),
        h2_2c_sources: first.emit_skipped
          ? 0
          : (control_.runtime_expectation.h2_2c_sources ?? 0),
        h2_4b_sources: first.emit_skipped
          ? 0
          : control_.files.filter(
              (file) =>
                control_.compiler_options.useDefineForClassFields === false ||
                sourceContainsDecorator(file),
            ).length,
        h2_5a_sources: first.emit_skipped ||
          control_.compiler_options.target >= ts.ScriptTarget.ESNext
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
      phase: "H2.5a-esnext-target-owner-controls",
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
        no_emit_helpers_controls: controls.filter(
          (item) => item.input.compiler_options.noEmitHelpers === true,
        ).length,
        es2021_controls: controls.filter(
          (item) => item.input.compiler_options.target === ts.ScriptTarget.ES2021,
        ).length,
        es2022_controls: controls.filter(
          (item) => item.input.compiler_options.target === ts.ScriptTarget.ES2022,
        ).length,
        later_standard_controls: controls.filter(
          (item) => item.input.compiler_options.target >= ts.ScriptTarget.ES2023 &&
            item.input.compiler_options.target <= ts.ScriptTarget.ES2025,
        ).length,
        esnext_controls: controls.filter(
          (item) => item.input.compiler_options.target === ts.ScriptTarget.ESNext,
        ).length,
        using_controls: CONTROLS.filter((item) =>
          item.files.some((file) => /\busing\b/.test(file.text))
        ).length,
        await_using_controls: CONTROLS.filter((item) =>
          item.files.some((file) => /\bawait\s+using\b/.test(file.text))
        ).length,
        standard_decorator_controls: CONTROLS.filter((item) =>
          item.files.some(sourceContainsDecorator)
        ).length,
        h2_5a_active_controls: controls.filter(
          (item) => item.runtime_expectation.h2_5a_sources !== 0,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-5a-owner-controls.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5a owner controls are fresh: outputs=${artifact.summary.exact_outputs}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-5a-owner-controls.mjs [--write|--check]");
}
