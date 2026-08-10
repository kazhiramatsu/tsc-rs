import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-4a-owner-controls.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-4a-owner-controls.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-4a-owner-controls.schema.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";

function options(extra = {}) {
  return {
    target: ts.ScriptTarget.ESNext,
    module: ts.ModuleKind.ESNext,
    alwaysStrict: true,
    experimentalDecorators: true,
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
  };
}

const CONTROLS = Object.freeze([
  control(
    "named-exported-decorated-class",
    "declare const dec: any;\n@dec export class C {}\n",
  ),
  control(
    "default-exported-decorated-class",
    "declare const dec: any;\n@dec export default class C {}\n",
  ),
  control(
    "anonymous-default-exported-decorated-class",
    "declare const dec: any;\n@dec export default class {}\n",
  ),
  control(
    "anonymous-default-exported-member-decoration",
    "declare const dec: any;\nexport default class { @dec method() {} }\n",
  ),
  control(
    "constructor-reference-static-initializer",
    "declare const dec: any;\n@dec class C { static value = C; }\n",
  ),
  control(
    "computed-name-evaluated-once",
    "declare const dec: any;\ndeclare function make(): string;\nclass C { @dec [make()]() {} }\n",
    { compiler_options: { emitDecoratorMetadata: true } },
  ),
  control(
    "primitive-metadata-matrix",
    "declare const dec: any;\nclass C { @dec a!: string; @dec b!: number; @dec c!: boolean; @dec d!: any[]; @dec e!: (() => void); }\n",
    { compiler_options: { emitDecoratorMetadata: true } },
  ),
  control(
    "constructor-method-and-accessor-metadata",
    'declare const dec: any, paramDec: any;\n@dec class C { constructor(value: string, count: number) {} @dec method(flag: boolean): string { return ""; } @dec get item(): number { return 0; } set item(@paramDec value: number) {} }\n',
    { compiler_options: { emitDecoratorMetadata: true } },
  ),
  control(
    "type-alias-metadata-kinds",
    "declare const dec: any;\ntype AliasString = string; type AliasNumber = number; type AliasBoolean = boolean; type AliasArray = unknown[]; type AliasFunction = () => void;\nclass C { @dec text!: AliasString; @dec count!: AliasNumber; @dec flag!: AliasBoolean; @dec values!: AliasArray; @dec callback!: AliasFunction; }\n",
    { compiler_options: { emitDecoratorMetadata: true } },
  ),
  control(
    "unknown-type-reference-metadata-fallback",
    "declare const dec: any;\nclass C { @dec value!: Missing; }\n",
    {
      compiler_options: { emitDecoratorMetadata: true },
      expected_diagnostic_codes: [2304],
    },
  ),
  control(
    "metadata-serializer-edge-kinds",
    'declare const dec: any;\nclass C { @dec same!: string | `x`; @dec mixed!: string | number; @dec literal!: 1; @dec template!: `x${string}`; @dec readonlyValues!: readonly string[]; @dec method(this: C, ...args: string[]): void {} @dec async asyncMethod() {} }\n',
    { compiler_options: { emitDecoratorMetadata: true } },
  ),
  control(
    "class-value-reference-metadata",
    "declare const dec: any;\nclass Value {}\nclass C { @dec value!: Value; }\n",
    { compiler_options: { emitDecoratorMetadata: true } },
  ),
  control(
    "qualified-value-reference-metadata",
    "declare const dec: any;\nnamespace N { export class Value {} }\nclass C { @dec value!: N.Value; }\n",
    { compiler_options: { emitDecoratorMetadata: true } },
  ),
  control(
    "type-only-reference-metadata-fallback",
    "declare const dec: any;\ninterface Shape { value: number; }\nclass C { @dec value!: Shape; }\n",
    { compiler_options: { emitDecoratorMetadata: true } },
  ),
  control(
    "no-emit-helpers-retains-helper-calls",
    "declare const dec: any;\n@dec class C {}\n",
    { compiler_options: { noEmitHelpers: true } },
  ),
  control(
    "helper-deduplication-and-member-before-class-order",
    "declare const dec: any;\n@dec class A { @dec method() {} }\n@dec class B { @dec field = 1; }\n",
  ),
  control(
    "commonjs-exported-decorated-class",
    "declare const dec: any;\n@dec export class C {}\n",
    { compiler_options: { module: ts.ModuleKind.CommonJS } },
  ),
  control(
    "system-exported-decorated-class",
    "declare const dec: any;\n@dec export class C {}\n",
    { compiler_options: { module: ts.ModuleKind.System } },
  ),
  control(
    "no-emit-on-error-stops-before-write",
    "declare const dec: any;\n@dec class C { static value = this; }\n",
    {
      compiler_options: { noEmitOnError: true },
      expected_paths: [],
      expected_diagnostic_codes: [2816],
      expected_emit_diagnostic_codes: [2816],
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

function outputKind(fileName) {
  return fileName.endsWith(".js") ? "javascript" : "other";
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
    kind: outputKind(fileName),
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
      phase: "H2.4a-legacy-decorator-owner-controls",
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-4a-owner-controls.mjs --write and review`,
  );
  process.stdout.write(
    `H2.4a owner controls are fresh: outputs=${artifact.summary.exact_outputs}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-4a-owner-controls.mjs [--write|--check]");
}
