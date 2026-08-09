import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-1d-owner-controls.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-1d-owner-controls.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-1d-owner-controls.schema.json";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";

const FILES = Object.freeze([
  {
    path: "/project/input.ts",
    text: [
      'import def, { value as renamed } from "./dep";',
      'import * as ns from "./ns";',
      'import "./side";',
      'export { remote as alias } from "./remote";',
      'export * from "./star";',
      "export let counter = def + renamed + ns.value;",
      "counter++;",
      "counter += 2;",
      "export function fn() { return counter; }",
      "export default class Named {}",
      'export async function load() { return import("./lazy"); }',
      "export const url = import.meta.url;",
      "",
    ].join("\n"),
    emit_eligible: true,
  },
  {
    path: "/project/dep.ts",
    text: "export default 1;\nexport const value = 2;\n",
    emit_eligible: false,
  },
  {
    path: "/project/ns.ts",
    text: "export const value = 3;\n",
    emit_eligible: false,
  },
  {
    path: "/project/side.ts",
    text: "export {};\n",
    emit_eligible: false,
  },
  {
    path: "/project/remote.ts",
    text: "export const remote = 4;\n",
    emit_eligible: false,
  },
  {
    path: "/project/star.ts",
    text: "export const extra = 5;\n",
    emit_eligible: false,
  },
  {
    path: "/project/lazy.ts",
    text: "export const lazy = 6;\n",
    emit_eligible: false,
  },
]);

const MODULE_RESOLUTIONS = Object.freeze(
  ["dep", "ns", "side", "remote", "star", "lazy"].map((name) => ({
    origin: "/project/input.ts",
    specifier: `./${name}`,
    target: `/project/${name}.ts`,
  })),
);

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

function observe() {
  const options = {
    target: ts.ScriptTarget.ESNext,
    module: ts.ModuleKind.System,
    newLine: ts.NewLineKind.LineFeed,
    ignoreDeprecations: "6.0",
    esModuleInterop: true,
  };
  const runs = Array.from({ length: 2 }, () =>
    ts.transpileModule(FILES[0].text, {
      compilerOptions: options,
      fileName: FILES[0].path,
      reportDiagnostics: true,
    }),
  );
  const expected = runs[0];
  requireCondition(
    runs.every(
      (run) =>
        run.outputText === expected.outputText &&
        canonical((run.diagnostics ?? []).map((diagnostic) => diagnostic.code)) ===
          canonical((expected.diagnostics ?? []).map((diagnostic) => diagnostic.code)),
    ),
    "System owner control is nondeterministic",
  );
  const diagnosticCodes = (expected.diagnostics ?? []).map(
    (diagnostic) => diagnostic.code,
  );
  requireCondition(
    diagnosticCodes.length === 0,
    "System owner control gained diagnostics",
  );
  return withFingerprint(
    {
      module_state: "System(4)",
      module_value: ts.ModuleKind.System,
      repetitions: 2,
      diagnostic_codes: diagnosticCodes,
      output: bytesRecord(expected.outputText),
    },
    "run_fingerprint_sha256",
  );
}

function buildArtifact() {
  validateRuntime();
  const control = withFingerprint(
    {
      control_id: "system-register-owner-closure",
      input: {
        current_directory: "/project",
        root: FILES[0].path,
        files: FILES.map((file) => ({
          path: file.path,
          emit_eligible: file.emit_eligible,
          ...bytesRecord(file.text),
        })),
        module_resolutions: MODULE_RESOLUTIONS,
        target: "ESNext(99)",
        module: "System(4)",
        ignore_deprecations: "6.0",
        es_module_interop: true,
      },
      runs: [observe()],
    },
    "control_fingerprint_sha256",
  );
  return withFingerprint(
    {
      schema: 1,
      phase: "H2.1d-system-owner-controls",
      status: "qualified",
      typescript: {
        version: ts.version,
        source_commit: SOURCE_COMMIT,
        bundle: pathHash(TYPESCRIPT_BUNDLE),
        implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
      },
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      controls: [control],
      summary: {
        controls: 1,
        exact_outputs: 1,
        typescript_runs: 2,
        diagnostics: 0,
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
    `stale ${TARGET_RELATIVE_PATH}; run h2-1d-owner-controls.mjs --write and review`,
  );
  process.stdout.write(
    `H2.1d owner controls are fresh: outputs=${artifact.summary.exact_outputs}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-1d-owner-controls.mjs [--write|--check]");
}
