import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5g-owner-controls.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5g-owner-controls.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-5g-owner-controls.schema.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const DISPOSABLE_GLOBALS = "interface Disposable {}\ninterface AsyncDisposable {}\n";
const DISPOSABLE_MODULE_GLOBALS =
  "declare global { interface Disposable {} interface AsyncDisposable {} }\n";
const ASYNC_GENERATOR_GLOBALS =
  "interface AsyncIterableIterator<T, TReturn = any, TNext = any> {}\n";

function options(extra = {}) {
  return {
    target: ts.ScriptTarget.ES2015,
    module: ts.ModuleKind.ESNext,
    alwaysStrict: true,
    useDefineForClassFields: true,
    useUnknownInCatchVariables: false,
    newLine: ts.NewLineKind.LineFeed,
    ignoreDeprecations: "6.0",
    ...extra,
  };
}

function control(controlId, source, extra = {}) {
  let ownedSource = /\busing\b/.test(source)
    ? `${/\b(?:export|import)\b/.test(source) ? DISPOSABLE_MODULE_GLOBALS : DISPOSABLE_GLOBALS}${source}`
    : source;
  if (/async\s+(?:function\s*)?\*/.test(source)) {
    ownedSource = `${ASYNC_GENERATOR_GLOBALS}${ownedSource}`;
  }
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

// These controls freeze transformES2016's semantic branches and their
// composition with the already-closed target ladder. Each program makes
// evaluation order, lexical temp ownership, binding identity, or the adjacent
// target boundary observable in exact bytes.
const CONTROLS = Object.freeze([
  control(
    "basic-exponentiation",
    "declare const left: number, right: number;\nexport const value = left ** right;\n",
  ),
  control(
    "right-associativity-and-unary-precedence",
    "declare const left: number, middle: number, right: number;\nexport const nested = left ** middle ** right;\nexport const unary = (-left) ** right;\n",
  ),
  control(
    "identifier-exponentiation-assignment",
    "let value = 2;\nvalue **= 3;\n",
  ),
  control(
    "property-exponentiation-assignment",
    "declare function object(): { value: number };\ndeclare function exponent(): number;\nobject().value **= exponent();\n",
  ),
  control(
    "element-exponentiation-assignment",
    "declare function object(): Record<string, number>;\ndeclare function key(): string;\ndeclare function exponent(): number;\nobject()[key()] **= exponent();\n",
  ),
  control(
    "nested-exponentiation-assignment",
    "declare function object(): Record<string, number>;\ndeclare function key(): string;\ndeclare const left: number, right: number;\nobject()[key()] **= left ** right;\n",
  ),
  control(
    "directive-and-exponentiation-comments",
    "\"custom\";\ndeclare const left: number, right: number;\n// before\nconst value = left ** /* exponent */ right;\n",
  ),
  control(
    "function-scope-hoists",
    "declare function object(): { value: number };\nfunction update(exponent: number) { return object().value **= exponent; }\n",
  ),
  control(
    "concise-arrow-hoists",
    "declare function object(): Record<string, number>;\ndeclare function key(): string;\nconst update = (exponent: number) => object()[key()] **= exponent;\n",
  ),
  control(
    "parameter-default-hoists",
    "declare function object(): Record<string, number>;\ndeclare function key(): string;\ndeclare const exponent: number;\nfunction read(value = object()[key()] **= exponent) { return value; }\n",
  ),
  control(
    "generated-name-collisions",
    "let _a: any, _b: any, _c: any;\ndeclare function object(): Record<string, number>;\ndeclare function key(): string;\nobject()[key()] **= 2;\nfunction nested() { let _a: any; return object()[key()] **= 3; }\n",
  ),
  control(
    "super-property-composition",
    "class Base { get value() { return 1; } set value(next: number) {} }\nclass Derived extends Base { update(exponent: number) { super.value **= exponent; return super.value ** exponent; } }\n",
  ),
  control(
    "class-field-composition",
    "declare const base: number, exponent: number;\nclass Value { field = base ** exponent; static shared = base ** exponent; update() { this.field **= exponent; } }\n",
  ),
  control(
    "computed-class-field-composition",
    "declare function key(): number;\ndeclare function value(): number;\ndeclare const exponent: number;\nclass Value { static [key() ** exponent] = value() ** exponent; }\n",
    { expected_diagnostic_codes: [1166] },
  ),
  control(
    "standard-decorator-composition",
    "declare const dec: any, base: number, exponent: number;\ndeclare const before: unique symbol, after: unique symbol;\n@dec class Value { [before] = base ** exponent; #method() {} [after] = base ** exponent; @dec field = base ** exponent; @dec update() { this.field **= exponent; } }\n",
  ),
  control(
    "legacy-decorator-composition",
    "declare const dec: any, base: number, exponent: number;\n@dec class Value { @dec field = base ** exponent; }\n",
    { compiler_options: { experimentalDecorators: true } },
  ),
  control(
    "async-function-composition",
    "async function read(value: Promise<number>, exponent: number) { return (await value) ** exponent; }\n",
  ),
  control(
    "object-rest-async-generator-composition",
    "async function* stream({ base, ...rest }: any) { yield base ** rest.exponent; }\n",
  ),
  control(
    "using-composition",
    "declare function acquire(): Disposable & { value: number };\ndeclare const exponent: number;\nusing resource = acquire();\nresource.value **= exponent;\n",
  ),
  control(
    "commonjs-export-composition",
    "export let value = 2;\nvalue **= 3;\nexport const squared = value ** 2;\n",
    { compiler_options: { module: ts.ModuleKind.CommonJS } },
  ),
  control(
    "adjacent-es2016-preserves-exponentiation",
    "export let value = 2;\nvalue **= 3;\nexport const squared = value ** 2;\n",
    { compiler_options: { target: ts.ScriptTarget.ES2016 } },
  ),
  control(
    "no-emit-on-error-stops-before-write",
    "let value = 2;\nvalue **= missing;\n",
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
        h2_5d_sources: first.emit_skipped ||
          control_.compiler_options.target >= ts.ScriptTarget.ES2019
          ? 0
          : control_.files.length,
        h2_5e_sources: first.emit_skipped ||
          control_.compiler_options.target >= ts.ScriptTarget.ES2018
          ? 0
          : control_.files.length,
        h2_5f_sources: first.emit_skipped ||
          control_.compiler_options.target >= ts.ScriptTarget.ES2017
          ? 0
          : control_.files.length,
        h2_5g_sources: first.emit_skipped ||
          control_.compiler_options.target >= ts.ScriptTarget.ES2016
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
      phase: "H2.5g-es2016-target-owner-controls",
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
        es2015_controls: controls.filter(
          (item) => item.input.compiler_options.target === ts.ScriptTarget.ES2015,
        ).length,
        es2016_controls: controls.filter(
          (item) => item.input.compiler_options.target === ts.ScriptTarget.ES2016,
        ).length,
        exponentiation_controls: CONTROLS.filter((item) =>
          item.files.some((file) => file.text.includes("**")),
        ).length,
        exponentiation_assignment_controls: CONTROLS.filter((item) =>
          item.files.some((file) => file.text.includes("**=")),
        ).length,
        property_assignment_controls: controls.filter((item) =>
          ["property-exponentiation-assignment", "function-scope-hoists",
           "super-property-composition", "class-field-composition",
           "standard-decorator-composition", "using-composition"].includes(
            item.control_id,
          ),
        ).length,
        element_assignment_controls: controls.filter((item) =>
          ["element-exponentiation-assignment", "nested-exponentiation-assignment",
           "concise-arrow-hoists", "parameter-default-hoists",
           "generated-name-collisions"].includes(item.control_id),
        ).length,
        parameter_controls: controls.filter(
          (item) => item.control_id === "parameter-default-hoists",
        ).length,
        collision_controls: controls.filter((item) =>
          item.control_id.includes("collision"),
        ).length,
        super_controls: controls.filter((item) =>
          item.control_id.includes("super"),
        ).length,
        precedence_controls: controls.filter(
          (item) => item.control_id === "right-associativity-and-unary-precedence",
        ).length,
        comment_controls: controls.filter(
          (item) => item.control_id === "directive-and-exponentiation-comments",
        ).length,
        class_composition_controls: controls.filter((item) =>
          item.control_id.includes("class-field") ||
          item.control_id.includes("decorator") ||
          item.control_id.includes("super"),
        ).length,
        commonjs_controls: controls.filter(
          (item) => item.input.compiler_options.module === ts.ModuleKind.CommonJS,
        ).length,
        async_composition_controls: controls.filter((item) =>
          item.control_id.includes("async"),
        ).length,
        using_controls: controls.filter((item) =>
          item.control_id.includes("using"),
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
        h2_5d_active_controls: controls.filter(
          (item) => item.runtime_expectation.h2_5d_sources !== 0,
        ).length,
        h2_5e_active_controls: controls.filter(
          (item) => item.runtime_expectation.h2_5e_sources !== 0,
        ).length,
        h2_5f_active_controls: controls.filter(
          (item) => item.runtime_expectation.h2_5f_sources !== 0,
        ).length,
        h2_5g_active_controls: controls.filter(
          (item) => item.runtime_expectation.h2_5g_sources !== 0,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-5g-owner-controls.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5g owner controls are fresh: outputs=${artifact.summary.exact_outputs}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-5g-owner-controls.mjs [--write|--check]");
}
