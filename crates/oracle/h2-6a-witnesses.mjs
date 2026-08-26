// W-H2.6A — frozen source-map witness machine for the H2.6a slice
// (docs/design/greenfield/slices/h2-6a.md §9).
//
// Every observation is captured from the pinned TypeScript 6.0.3 runtime
// in a fresh Node process, twice, exactly like the frozen H2.5h-a witness
// machines. Beyond those serializers this machine records the write
// callback's sixth (`data`) argument (WriteFileCallbackData:
// sourceMapUrlPos + diagnostics presence), `emitResult.sourceMaps`
// (raw input source file names + the canonical JSON.stringify of each
// map object), `emitResult.emittedFiles` where listed, and the write
// ORDER as the array order. Expected bytes are never hand-authored:
// structural expectations (write paths, diagnostic codes, emitSkipped)
// are machine-validated inputs, and the oracle bytes are the authority.

import crypto from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-6a-witnesses.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-6a-witnesses.v1.json";
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-6a-witnesses.schema.json";
const PACKET_RELATIVE_PATH = "docs/design/greenfield/slices/h2-6a.md";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const TYPESCRIPT_LIB_DIRECTORY = "vendor/typescript-6.0.3/lib";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const SLICE = "H2.6a";
const SUB_PACKET = "W-H2.6A";
const TRUSTED_BASE = "752803c74d24acb33b5675f767e8c7b4fda50d50";
const INTERNAL_OBSERVE_MODE = "--internal-observe-witness";

const ROLES = Object.freeze([
  "positive",
  "adjacent-negative-control",
  "composition",
  "fault",
]);

// parity: the m-2/m-3 fixture gates replay the case byte-exact in Rust.
// refusal-control: the option shape is H2.6b-owned; Rust asserts the
// typed emit refusal until that slice, and the frozen oracle bytes are
// the 6b inheritance.
const RUST_LANES = Object.freeze(["parity", "refusal-control"]);

// Composition facets a composition-role case must cite; the census
// asserts the full set is covered so transform interplay under the
// recorder cannot silently narrow.
const COMPOSITION_FACETS = Object.freeze([
  "lowered-class",
  "generators-machine",
  "helper-prologue",
  "destructuring",
  "spread",
  "json-unit",
  "module-wrapper",
  "multi-unit",
]);

function baseOptions(extra = {}) {
  return {
    target: ts.ScriptTarget.ES5,
    module: ts.ModuleKind.CommonJS,
    newLine: ts.NewLineKind.LineFeed,
    sourceMap: true,
    ignoreDeprecations: "6.0",
    ...extra,
  };
}

function witnessCase(caseSlug, role, rustLane, description, files, extra = {}) {
  return {
    case_slug: caseSlug,
    role,
    rust_lane: rustLane,
    description,
    files,
    roots: extra.roots ?? files.map((file) => file.path).filter((p) => !p.endsWith(".json")),
    option_overrides: extra.option_overrides ?? {},
    expected_reported_codes: extra.expected_reported_codes ?? [],
    expected_emit_skipped: extra.expected_emit_skipped ?? false,
    expected_write_paths: extra.expected_write_paths,
    composition_facets: extra.composition_facets ?? [],
    markers: extra.markers ?? [],
  };
}

function file(filePath, lines) {
  return { path: filePath, text: lines.join("\n") };
}

const PLAIN_SOURCE = [
  "declare function sink(value: unknown): void;",
  "const answer = 42;",
  "function pick(flag: boolean): number {",
  "  if (flag) {",
  "    return answer;",
  "  }",
  "  return 0;",
  "}",
  "sink(pick(true));",
  "",
];

const TEMPLATE_SOURCE = [
  "declare function sink(value: unknown): void;",
  "const stitched = `first line",
  "second ${1 + 2} line",
  "third line`;",
  "sink(stitched);",
  "",
];

const COMMENT_SOURCE = [
  "// detached header comment",
  "",
  "declare function sink(value: unknown): void;",
  "/* leading block */ const first = 1; // trailing line",
  "/**",
  " * multi-line leading doc",
  " */",
  "const second = first + 1; /* trailing block */",
  "sink(second);",
  "",
];

const CLASS_SOURCE = [
  "declare function sink(value: unknown): void;",
  "class Base {",
  "  greet(): string {",
  "    return \"base\";",
  "  }",
  "}",
  "class Derived extends Base {",
  "  label = \"derived\";",
  "  constructor() {",
  "    super();",
  "  }",
  "  greet(): string {",
  "    return this.label + super.greet();",
  "  }",
  "}",
  "sink(new Derived().greet());",
  "",
];

const SHEBANG_SOURCE = [
  "#!/usr/bin/env node",
  "declare function sink(value: unknown): void;",
  "const started = true;",
  "sink(started);",
  "",
];

const NEWLINE_SOURCE = [
  "declare function sink(value: unknown): void;",
  "const left = 1;",
  "const right = 2;",
  "sink(left + right);",
  "",
];

const WITNESS_FAMILY_SPECS = Object.freeze([
  {
    family_id: "plain-program",
    rust_boundary: false,
    description:
      "canonical node-boundary recording over plain statements: skip-trivia leading positions, range ends before trailing comments, writeToken keywords and braces, the map-then-js write order, the js-then-map emittedFiles order, and the sourceMapUrlPos callback metadata",
    cases: [
      witnessCase(
        "positive-statements",
        "positive",
        "parity",
        "multi-statement program with nested blocks and keywords; listEmittedFiles captures the js-then-map listing order against the map-then-js write order",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { listEmittedFiles: true },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "positive-expression-density",
        "positive",
        "parity",
        "dense expression forms: calls, property chains, binary/conditional operators, array and object literals",
        [
          file("/project/input.ts", [
            "declare function sink(value: unknown): void;",
            "const box = { width: 3, height: 4 };",
            "const flags = [true, false, box.width > box.height];",
            "const area = box.width * box.height + (flags[0] ? 1 : 0);",
            "sink({ area, flags, deep: { ref: box } });",
            "",
          ]),
        ],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-nomap",
        "adjacent-negative-control",
        "parity",
        "identical program without sourceMap: single write, no URL comment, no sourceMaps result, no sourceMapUrlPos",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { sourceMap: false },
          expected_write_paths: ["/project/input.js"],
        },
      ),
      witnessCase(
        "fault-noemitonerror",
        "fault",
        "parity",
        "noEmitOnError with a type error under sourceMap: zero writes, emitSkipped, no sourceMaps entries",
        [
          file("/project/input.ts", [
            "const wrong: number = \"text\";",
            "export const keep = wrong;",
            "",
          ]),
        ],
        {
          option_overrides: { noEmitOnError: true },
          expected_reported_codes: [2322],
          expected_emit_skipped: true,
          expected_write_paths: [],
        },
      ),
    ],
  },
  {
    family_id: "multiline-literals",
    rust_boundary: false,
    description:
      "writer line/column tracking through multi-line literals: native template at ES2015 versus the ES5 lowering, with the mapping AFTER the literal proving the generated line advance",
    cases: [
      witnessCase(
        "positive-template-native",
        "positive",
        "parity",
        "multi-line template kept native at target ES2015; generated lines advance inside one literal write",
        [file("/project/input.ts", TEMPLATE_SOURCE)],
        {
          option_overrides: { target: ts.ScriptTarget.ES2015 },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "positive-template-lowered",
        "positive",
        "parity",
        "the same template lowered at ES5 into concatenation with escaped newlines; generated text is single-line",
        [file("/project/input.ts", TEMPLATE_SOURCE)],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-singleline",
        "adjacent-negative-control",
        "parity",
        "single-line string literal control",
        [
          file("/project/input.ts", [
            "declare function sink(value: unknown): void;",
            "const flat = \"one line only\";",
            "sink(flat);",
            "",
          ]),
        ],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
    ],
  },
  {
    family_id: "comment-interaction",
    rust_boundary: false,
    description:
      "comment writers record positions around every written comment (leading, trailing, detached); removeComments deletes those mappings with the bytes",
    cases: [
      witnessCase(
        "positive-comments",
        "positive",
        "parity",
        "detached header, leading block, trailing line and block comments, multi-line doc comment",
        [file("/project/input.ts", COMMENT_SOURCE)],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-remove-comments",
        "adjacent-negative-control",
        "parity",
        "identical source under removeComments: comment mappings vanish with the comment bytes",
        [file("/project/input.ts", COMMENT_SOURCE)],
        {
          option_overrides: { removeComments: true },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "composition-class-comments",
        "composition",
        "parity",
        "comments attached to class members surviving the ES5 class lowering while their owners move",
        [
          file("/project/input.ts", [
            "declare function sink(value: unknown): void;",
            "class Carrier {",
            "  // leading member comment",
            "  value = 1; // trailing member comment",
            "  /** doc for method */",
            "  read(): number {",
            "    return this.value;",
            "  }",
            "}",
            "sink(new Carrier().read());",
            "",
          ]),
        ],
        {
          composition_facets: ["lowered-class"],
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
    ],
  },
  {
    family_id: "transform-composition",
    rust_boundary: false,
    description:
      "synthetic nodes produced by the live H2.1-H2.5h transform stack carry original sourceMapRange provenance; helper prologues are unmapped generated text",
    cases: [
      witnessCase(
        "positive-class-es5",
        "positive",
        "parity",
        "heritage class lowered at ES5: extends helper, prototype methods, constructor with super",
        [file("/project/input.ts", CLASS_SOURCE)],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-native-class",
        "adjacent-negative-control",
        "parity",
        "the same class kept native at target ES2022: no lowering, direct token mappings",
        [file("/project/input.ts", CLASS_SOURCE)],
        {
          option_overrides: { target: ts.ScriptTarget.ES2022 },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "composition-generator-loop",
        "composition",
        "parity",
        "generator with yield inside a captured for-of loop at ES5: the Generators state machine plus loop conversion under the recorder",
        [
          file("/project/input.ts", [
            "declare function sink(value: unknown): void;",
            "declare const items: number[];",
            "function* walk() {",
            "  for (const element of items) {",
            "    sink(() => element);",
            "    yield element;",
            "  }",
            "}",
            "sink(walk);",
            "",
          ]),
        ],
        {
          option_overrides: { downlevelIteration: true },
          composition_facets: ["generators-machine", "helper-prologue"],
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
          markers: [{ token: "__generator", expectation: "present" }],
        },
      ),
      witnessCase(
        "composition-destructuring-spread",
        "composition",
        "parity",
        "object rest destructuring and array spread lowered at ES5",
        [
          file("/project/input.ts", [
            "declare function sink(value: unknown): void;",
            "declare const source: { a: number; b?: number; c: number };",
            "const { a, b = 5, ...rest } = source;",
            "const gathered = [a, b, ...[rest.c, a]];",
            "sink(gathered);",
            "",
          ]),
        ],
        {
          option_overrides: { downlevelIteration: true },
          composition_facets: ["destructuring", "spread"],
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
    ],
  },
  {
    family_id: "path-shapes",
    rust_boundary: false,
    description:
      "sources[] relativization against the map directory (the js output directory in the H2.6a lane), and encodeURI on the URL comment but never on sources[]",
    cases: [
      witnessCase(
        "positive-outdir-nested",
        "positive",
        "parity",
        "nested source with outDir: sources[] climbs with ../ segments from the output directory",
        [file("/project/src/nested/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { outDir: "/project/out" },
          expected_write_paths: [
            "/project/out/input.js.map",
            "/project/out/input.js",
          ],
        },
      ),
      witnessCase(
        "adjacent-negative-no-outdir",
        "adjacent-negative-control",
        "parity",
        "the same nested source without outDir: map beside the js beside the source, sources[] is the bare basename",
        [file("/project/src/nested/input.ts", PLAIN_SOURCE)],
        {
          expected_write_paths: [
            "/project/src/nested/input.js.map",
            "/project/src/nested/input.js",
          ],
        },
      ),
      witnessCase(
        "positive-percent-name",
        "positive",
        "parity",
        "a file name needing URL escaping: the sourceMappingURL comment is encodeURI-escaped while sources[] stays raw",
        [file("/project/input space.ts", PLAIN_SOURCE)],
        {
          expected_write_paths: [
            "/project/input space.js.map",
            "/project/input space.js",
          ],
        },
      ),
    ],
  },
  {
    family_id: "json-exclusion",
    rust_boundary: false,
    description:
      "JSON source files never receive a map: the plan forces an undefined map path for JSON sources and the printer arm records nothing, while TypeScript units in the same program map normally",
    cases: [
      witnessCase(
        "positive-json-in-program",
        "positive",
        "parity",
        "ts root importing json without outDir: the ts unit maps; the json output would overwrite its input and is suppressed entirely",
        [
          file("/project/input.ts", [
            "import data from \"./data.json\";",
            "declare function sink(value: unknown): void;",
            "sink(data.value);",
            "",
          ]),
          file("/project/data.json", ["{ \"value\": 1 }", ""]),
        ],
        {
          option_overrides: { resolveJsonModule: true, esModuleInterop: true },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-json-self",
        "adjacent-negative-control",
        "parity",
        "a bare json root whose output path equals its input: nothing is written and nothing is mapped",
        [file("/project/data.json", ["{ \"value\": 1 }", ""])],
        {
          roots: ["/project/data.json"],
          option_overrides: { resolveJsonModule: true },
          expected_write_paths: [],
        },
      ),
      witnessCase(
        "composition-json-outdir",
        "composition",
        "parity",
        "ts + json under outDir: the ts unit writes map-then-js, the json unit writes its copy with no map, and emittedFiles lists all three",
        [
          file("/project/src/input.ts", [
            "import data from \"./data.json\";",
            "declare function sink(value: unknown): void;",
            "sink(data.value);",
            "",
          ]),
          file("/project/src/data.json", ["{ \"value\": 1 }", ""]),
        ],
        {
          option_overrides: {
            resolveJsonModule: true,
            esModuleInterop: true,
            outDir: "/project/out",
            listEmittedFiles: true,
          },
          composition_facets: ["json-unit", "module-wrapper", "multi-unit"],
          expected_write_paths: [
            "/project/out/data.json",
            "/project/out/input.js.map",
            "/project/out/input.js",
          ],
        },
      ),
    ],
  },
  {
    family_id: "newline-variants",
    rust_boundary: false,
    description:
      "newLine polarity: CRLF versus LF changes the emitted bytes and the URL-append newline but the mappings string is identical (positions count lines, not bytes)",
    cases: [
      witnessCase(
        "positive-crlf",
        "positive",
        "parity",
        "CRLF output",
        [file("/project/input.ts", NEWLINE_SOURCE)],
        {
          option_overrides: {
            newLine: ts.NewLineKind.CarriageReturnLineFeed,
          },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-lf",
        "adjacent-negative-control",
        "parity",
        "LF output of the same program; the census asserts its mappings string equals the CRLF case's",
        [file("/project/input.ts", NEWLINE_SOURCE)],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
    ],
  },
  {
    family_id: "edge-shapes",
    rust_boundary: false,
    description:
      "degenerate shapes: empty file, comments-only file, shebang, BOM polarity between the js and map writes, parse-error emit",
    cases: [
      witnessCase(
        "positive-empty",
        "positive",
        "parity",
        "empty source file: the source is still registered and the URL comment still lands",
        [file("/project/input.ts", [""])],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "positive-comments-only",
        "positive",
        "parity",
        "file containing only a comment",
        [file("/project/input.ts", ["// the only line", ""])],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "positive-shebang",
        "positive",
        "parity",
        "shebang line raw-written before prologue with mappings following it",
        [file("/project/input.ts", SHEBANG_SOURCE)],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "positive-bom",
        "positive",
        "parity",
        "emitBOM: the js write carries the byte-order mark while the map write NEVER does",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { emitBOM: true },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-shebang-nomap",
        "adjacent-negative-control",
        "parity",
        "shebang without sourceMap: single write, no URL comment",
        [file("/project/input.ts", SHEBANG_SOURCE)],
        {
          option_overrides: { sourceMap: false },
          expected_write_paths: ["/project/input.js"],
        },
      ),
      witnessCase(
        "fault-parse-error",
        "fault",
        "parity",
        "a parse error still emits js and map (no noEmitOnError): recovery nodes map from their recovered positions",
        [
          file("/project/input.ts", [
            "declare function sink(value: unknown): void;",
            "const = 5;",
            "sink(0);",
            "",
          ]),
        ],
        {
          expected_reported_codes: [1134, 1134],
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
    ],
  },
  {
    family_id: "token-surface",
    rust_boundary: false,
    description:
      "m-3 witness extension (m-2 review F9 residue): the remaining upstream writeToken map sites — switch case-block braces, BOTH single-statement clause-colon polarities (same-line inline statement vs indented list), and the debugger keyword; MetaProperty stays deferred to the ca-1 burn-down",
    cases: [
      witnessCase(
        "positive-switch-colons",
        "positive",
        "parity",
        "switch with one same-line single-statement clause, one next-line clause list, and a same-line default: case-block braces plus both clause-colon polarities",
        [
          file("/project/input.ts", [
            "declare function sink(value: unknown): void;",
            "declare const pick: number;",
            "switch (pick) {",
            "    case 0: sink(\"zero\");",
            "    case 1:",
            "        sink(\"one\");",
            "        break;",
            "    default: sink(\"rest\");",
            "}",
            "sink(pick);",
            "",
          ]),
        ],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "positive-debugger",
        "positive",
        "parity",
        "debugger statement inside a function body: the debugger keyword token record between plain statement boundaries",
        [
          file("/project/input.ts", [
            "declare function sink(value: unknown): void;",
            "function probe(): number {",
            "    debugger;",
            "    return 7;",
            "}",
            "sink(probe());",
            "",
          ]),
        ],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-switch-nomap",
        "adjacent-negative-control",
        "parity",
        "identical switch program without sourceMap: single write, no URL comment, no sourceMaps result",
        [
          file("/project/input.ts", [
            "declare function sink(value: unknown): void;",
            "declare const pick: number;",
            "switch (pick) {",
            "    case 0: sink(\"zero\");",
            "    case 1:",
            "        sink(\"one\");",
            "        break;",
            "    default: sink(\"rest\");",
            "}",
            "sink(pick);",
            "",
          ]),
        ],
        {
          option_overrides: { sourceMap: false },
          expected_write_paths: ["/project/input.js"],
        },
      ),
    ],
  },
  {
    family_id: "boundary-controls",
    rust_boundary: true,
    description:
      "H2.6b-owned option shapes frozen as inheritance: Rust keeps the typed emit refusal for every case in this family until H2.6b, and these oracle bytes are that slice's starting expectations",
    cases: [
      witnessCase(
        "control-sourceroot",
        "adjacent-negative-control",
        "refusal-control",
        "sourceRoot with sourceMap: the map's sourceRoot field is populated and sources relativize against the common source directory",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { sourceRoot: "src/" },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "control-maproot",
        "adjacent-negative-control",
        "refusal-control",
        "mapRoot with sourceMap: the URL comment points into the map root while the map is still written beside the js",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { mapRoot: "/project/maps" },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "control-inline-map",
        "adjacent-negative-control",
        "refusal-control",
        "inlineSourceMap alone: no .map write; the URL comment is the base64 data URI; emitResult.sourceMaps still carries the map object",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { sourceMap: false, inlineSourceMap: true },
          expected_write_paths: ["/project/input.js"],
        },
      ),
      witnessCase(
        "control-inline-sources-with-map",
        "adjacent-negative-control",
        "refusal-control",
        "inlineSources with an external map: sourcesContent joins the map JSON as the eighth field position",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { inlineSources: true },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "fault-inline-sources-alone",
        "fault",
        "refusal-control",
        "inlineSources without any map option: TS5051 and an unmapped emit",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { sourceMap: false, inlineSources: true },
          expected_reported_codes: [5051],
          expected_write_paths: ["/project/input.js"],
        },
      ),
    ],
  },
]);

function fail(message) {
  process.stderr.write(`h2-6a-witnesses: ${message}\n`);
  process.exit(1);
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

function fingerprintIsValid(record, field) {
  if (record === null || typeof record !== "object") return false;
  const { [field]: storedFingerprint, ...rest } = record;
  return (
    typeof storedFingerprint === "string" &&
    storedFingerprint === sha256(Buffer.from(canonical(rest), "utf8"))
  );
}

function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function readText(relativePath) {
  return readBytes(relativePath).toString("utf8");
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

function serializeOptions(options) {
  return Object.fromEntries(
    Object.entries(options).sort(([left], [right]) => left.localeCompare(right)),
  );
}

function libraryInventoryRecord() {
  const directory = path.join(WORKSPACE, TYPESCRIPT_LIB_DIRECTORY);
  const names = fs
    .readdirSync(directory)
    .filter((name) => name.startsWith("lib.") && name.endsWith(".d.ts"))
    .sort();
  requireCondition(names.length > 0, "vendored TypeScript lib inventory is empty");
  const hash = crypto.createHash("sha256");
  for (const name of names) {
    hash.update(name);
    hash.update("\u0000");
    hash.update(fs.readFileSync(path.join(directory, name)));
    hash.update("\u0000");
  }
  return {
    path: TYPESCRIPT_LIB_DIRECTORY,
    default_libraries: names.length,
    sha256: hash.digest("hex"),
  };
}

function typescriptRecord() {
  return {
    version: ts.version,
    source_commit: SOURCE_COMMIT,
    bundle: pathHash(TYPESCRIPT_BUNDLE),
    implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
    lib: libraryInventoryRecord(),
  };
}

function writeFileAtomic(absolutePath, contents) {
  const temporary = path.join(
    path.dirname(absolutePath),
    `.${path.basename(absolutePath)}.tmp`,
  );
  fs.writeFileSync(temporary, contents);
  fs.renameSync(temporary, absolutePath);
}

let adoptedCases = 0;

// Write-side observation adoption exactly as the frozen 5h witness
// machines: all-or-nothing key over this generator's bytes and the full
// typescript record (bundle + implementation + lib inventory). Only the
// fresh-process observations are adopted; every structural expectation
// and census below re-executes on every write. --check never adopts.
function reusableStoredObservations(currentTypescriptRecord) {
  if (mode !== "--write") return null;
  const targetPath = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
  if (!fs.existsSync(targetPath)) return null;
  let stored;
  try {
    stored = JSON.parse(fs.readFileSync(targetPath, "utf8"));
  } catch {
    return null;
  }
  if (
    stored === null ||
    typeof stored !== "object" ||
    stored.schema !== 1 ||
    stored.kind !== "h2-source-map-witnesses" ||
    !fingerprintIsValid(stored, "witnesses_fingerprint_sha256") ||
    canonical(stored.generator) !== canonical(pathHash(GENERATOR_RELATIVE_PATH)) ||
    canonical(stored.typescript) !== canonical(currentTypescriptRecord)
  ) {
    return null;
  }
  const observations = new Map();
  for (const family of stored.families ?? []) {
    for (const storedCase of family.cases ?? []) {
      if (
        typeof storedCase.case_id === "string" &&
        fingerprintIsValid(storedCase.observation, "observation_fingerprint_sha256")
      ) {
        observations.set(storedCase.case_id, storedCase.observation);
      }
    }
  }
  return observations;
}

function validateRuntime() {
  const node = readText(".node-version").trim();
  requireCondition(node === EXPECTED_NODE, "unexpected Node pin");
  requireCondition(process.version === `v${node}`, `requires Node ${node}`);
  requireCondition(ts.version === "6.0.3", "unexpected TypeScript runtime");
  for (const name of [
    "createProgram",
    "createCompilerHost",
    "createSourceFile",
    "getPreEmitDiagnostics",
    "normalizePath",
    "getScriptKindFromFileName",
  ]) {
    requireCondition(
      typeof ts[name] === "function",
      `pinned TypeScript does not expose ${name}`,
    );
  }
}

function hasDirectory(files, directory) {
  const prefix = `${ts.normalizePath(directory).replace(/\/$/, "")}/`;
  return [...files.keys()].some((fileName) => fileName.startsWith(prefix));
}

function createVirtualProgram(control) {
  const files = new Map(control.files.map((entry) => [entry.path, entry.text]));
  const baseHost = ts.createCompilerHost(control.compiler_options, true);
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
      return (
        hasDirectory(files, directory) ||
        (baseHost.directoryExists?.(directory) ?? false)
      );
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
      if (text === undefined) {
        return baseHost.getSourceFile(fileName, languageVersion);
      }
      return ts.createSourceFile(
        normalized,
        text,
        languageVersion,
        true,
        ts.getScriptKindFromFileName(normalized),
      );
    },
  };
  return ts.createProgram(control.roots, control.compiler_options, host);
}

function serializeDiagnostic(diagnostic, phase) {
  return {
    phase,
    code: diagnostic.code,
    category: ts.DiagnosticCategory[diagnostic.category],
    file: diagnostic.file ? ts.normalizePath(diagnostic.file.fileName) : null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
  };
}

// The full six-argument write callback, including the sixth
// WriteFileCallbackData argument that H2.5h never needed: the js write
// of a mapped unit carries { sourceMapUrlPos, diagnostics } while the
// map write passes no data argument at all.
function serializeWrite(arguments_, index) {
  const [fileName, text, writeByteOrderMark, onError, sourceFiles, data] =
    arguments_;
  const callback = Buffer.from(text, "utf8");
  const materialized = writeByteOrderMark
    ? Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), callback])
    : callback;
  return {
    index,
    path: ts.normalizePath(fileName),
    callback_utf8_base64: callback.toString("base64"),
    callback_utf8_sha256: sha256(callback),
    callback_utf8_bytes: callback.length,
    write_byte_order_mark: writeByteOrderMark,
    on_error_callback_present: typeof onError === "function",
    source_files: (sourceFiles ?? []).map((source) =>
      ts.normalizePath(source.fileName),
    ),
    materialized_utf8_sha256: sha256(materialized),
    materialized_utf8_bytes: materialized.length,
    data_present: data !== undefined,
    data_source_map_url_pos:
      data && typeof data.sourceMapUrlPos === "number"
        ? data.sourceMapUrlPos
        : null,
    data_diagnostics_count:
      data && Array.isArray(data.diagnostics) ? data.diagnostics.length : null,
  };
}

function countOccurrences(text, token) {
  let count = 0;
  let cursor = text.indexOf(token);
  while (cursor !== -1) {
    count += 1;
    cursor = text.indexOf(token, cursor + token.length);
  }
  return count;
}

function witnessControl(caseSpec) {
  return {
    files: caseSpec.files,
    roots: caseSpec.roots,
    compiler_options: baseOptions(caseSpec.option_overrides),
  };
}

function observeWitnessCase(caseSpec) {
  const control = witnessControl(caseSpec);
  const program = createVirtualProgram(control);
  const reportedDiagnostics = ts.getPreEmitDiagnostics(program);
  const writes = [];
  const emitResult = program.emit(
    undefined,
    (...arguments_) => writes.push(arguments_),
    undefined,
    false,
    {},
  );
  const observedCodes = reportedDiagnostics
    .map((diagnostic) => diagnostic.code)
    .sort((left, right) => left - right);
  requireCondition(
    canonical(observedCodes) === canonical(caseSpec.expected_reported_codes),
    `${caseSpec.case_id} reported [${observedCodes.join(", ")}], expected [${caseSpec.expected_reported_codes.join(", ")}]`,
  );
  requireCondition(
    emitResult.emitSkipped === caseSpec.expected_emit_skipped,
    `${caseSpec.case_id} emitSkipped=${emitResult.emitSkipped}, expected ${caseSpec.expected_emit_skipped}`,
  );
  const serializedWrites = writes.map(serializeWrite);
  const writePaths = serializedWrites.map((write) => write.path);
  requireCondition(
    canonical(writePaths) === canonical(caseSpec.expected_write_paths),
    `${caseSpec.case_id} wrote [${writePaths.join(", ")}], expected [${caseSpec.expected_write_paths.join(", ")}]`,
  );
  const sourceMaps =
    emitResult.sourceMaps === undefined
      ? null
      : emitResult.sourceMaps.map((entry) => ({
          input_source_file_names: entry.inputSourceFileNames.map((name) =>
            ts.normalizePath(name),
          ),
          source_map_json: JSON.stringify(entry.sourceMap),
        }));
  const emittedFiles =
    emitResult.emittedFiles === undefined
      ? null
      : emitResult.emittedFiles.map((name) => ts.normalizePath(name));
  // One-toJSON-authority invariant: every externally written .js.map
  // byte-equals the ordinal emitResult.sourceMaps stringification.
  const mapWrites = serializedWrites.filter((write) =>
    write.path.endsWith(".js.map"),
  );
  if (mapWrites.length > 0) {
    requireCondition(
      sourceMaps !== null && sourceMaps.length === mapWrites.length,
      `${caseSpec.case_id} has ${mapWrites.length} map writes but ${sourceMaps?.length ?? 0} sourceMaps entries`,
    );
    mapWrites.forEach((write, ordinal) => {
      const mapText = Buffer.from(write.callback_utf8_base64, "base64").toString(
        "utf8",
      );
      requireCondition(
        mapText === sourceMaps[ordinal].source_map_json,
        `${caseSpec.case_id} map write ${write.path} diverges from emitResult.sourceMaps[${ordinal}]`,
      );
      requireCondition(
        write.write_byte_order_mark === false && write.data_present === false,
        `${caseSpec.case_id} map write ${write.path} carries a BOM or a data argument`,
      );
    });
  }
  // sourceMapUrlPos self-consistency: where present, the js text at that
  // UTF-16 offset begins the sourceMappingURL comment.
  for (const write of serializedWrites) {
    if (write.data_source_map_url_pos !== null) {
      const text = Buffer.from(write.callback_utf8_base64, "base64").toString(
        "utf8",
      );
      requireCondition(
        text.slice(write.data_source_map_url_pos).startsWith("//# sourceMappingURL="),
        `${caseSpec.case_id} sourceMapUrlPos ${write.data_source_map_url_pos} does not address the URL comment in ${write.path}`,
      );
    }
  }
  const jsWrites = serializedWrites.filter((write) => write.path.endsWith(".js"));
  const combinedOutput = jsWrites
    .map((write) =>
      Buffer.from(write.callback_utf8_base64, "base64").toString("utf8"),
    )
    .join("\n");
  const markerOccurrences = caseSpec.markers.map(({ token }) => ({
    token,
    occurrences: countOccurrences(combinedOutput, token),
  }));
  return withFingerprint(
    {
      reported_diagnostics: reportedDiagnostics.map((diagnostic) =>
        serializeDiagnostic(diagnostic, "pre-emit"),
      ),
      emit_diagnostics: emitResult.diagnostics.map((diagnostic) =>
        serializeDiagnostic(diagnostic, "emit"),
      ),
      emit_skipped: emitResult.emitSkipped,
      writes: serializedWrites,
      source_maps: sourceMaps,
      emitted_files: emittedFiles,
      marker_occurrences: markerOccurrences,
    },
    "observation_fingerprint_sha256",
  );
}

function observeWitnessCaseInFreshProcess(caseSpec) {
  const stdout = execFileSync(
    process.execPath,
    [GENERATOR_PATH, INTERNAL_OBSERVE_MODE, caseSpec.case_id],
    {
      cwd: WORKSPACE,
      encoding: "utf8",
      maxBuffer: 128 * 1024 * 1024,
    },
  );
  const observation = JSON.parse(stdout);
  requireCondition(
    observation !== null &&
      typeof observation === "object" &&
      typeof observation.observation_fingerprint_sha256 === "string",
    `${caseSpec.case_id} fresh TypeScript observation is invalid`,
  );
  return observation;
}

function buildWitnessCase(caseSpec, adoptedObservations) {
  const adopted = adoptedObservations?.get(caseSpec.case_id) ?? null;
  let first;
  if (adopted !== null) {
    adoptedCases += 1;
    first = adopted;
  } else {
    first = observeWitnessCaseInFreshProcess(caseSpec);
    const second = observeWitnessCaseInFreshProcess(caseSpec);
    requireCondition(
      first.observation_fingerprint_sha256 ===
        second.observation_fingerprint_sha256,
      `${caseSpec.case_id} TypeScript observation is nondeterministic`,
    );
  }
  for (const markerSpec of caseSpec.markers) {
    const observed = first.marker_occurrences.find(
      (entry) => entry.token === markerSpec.token,
    );
    requireCondition(
      observed !== undefined,
      `${caseSpec.case_id} marker ${markerSpec.token} was not observed`,
    );
    if (markerSpec.expectation === "present") {
      requireCondition(
        observed.occurrences > 0,
        `${caseSpec.case_id} marker ${markerSpec.token} is absent from the oracle output`,
      );
    } else {
      requireCondition(
        markerSpec.expectation === "absent" && observed.occurrences === 0,
        `${caseSpec.case_id} marker ${markerSpec.token} expectation failed`,
      );
    }
  }
  const control = witnessControl(caseSpec);
  return withFingerprint(
    {
      case_id: caseSpec.case_id,
      role: caseSpec.role,
      rust_lane: caseSpec.rust_lane,
      description: caseSpec.description,
      option_overrides: serializeOptions(caseSpec.option_overrides),
      expected_reported_codes: caseSpec.expected_reported_codes,
      expected_emit_skipped: caseSpec.expected_emit_skipped,
      expected_write_paths: caseSpec.expected_write_paths,
      composition_facets: caseSpec.composition_facets,
      input: {
        current_directory: "/project",
        roots: control.roots,
        files: control.files.map((entry) => ({
          path: entry.path,
          root: control.roots.includes(entry.path),
          ...bytesRecord(entry.text),
        })),
        compiler_options: serializeOptions(control.compiler_options),
      },
      markers: caseSpec.markers,
      repetitions: 2,
      observation: first,
    },
    "case_fingerprint_sha256",
  );
}

function flattenCaseSpecs() {
  const specs = [];
  for (const family of WITNESS_FAMILY_SPECS) {
    for (const caseSpec of family.cases) {
      specs.push({
        ...caseSpec,
        case_id: `${family.family_id}--${caseSpec.case_slug}`,
        family_id: family.family_id,
        rust_boundary: family.rust_boundary,
      });
    }
  }
  const ids = new Set(specs.map((spec) => spec.case_id));
  requireCondition(ids.size === specs.length, "witness case ids collide");
  for (const spec of specs) {
    requireCondition(
      ROLES.includes(spec.role),
      `${spec.case_id} has unknown role ${spec.role}`,
    );
    requireCondition(
      RUST_LANES.includes(spec.rust_lane),
      `${spec.case_id} has unknown rust lane ${spec.rust_lane}`,
    );
    requireCondition(
      (spec.rust_lane === "refusal-control") === spec.rust_boundary,
      `${spec.case_id} rust lane disagrees with its family boundary marker`,
    );
    requireCondition(
      Array.isArray(spec.expected_write_paths),
      `${spec.case_id} does not declare its expected write paths`,
    );
    requireCondition(
      (spec.role === "fault") === (spec.expected_reported_codes.length > 0),
      `${spec.case_id} fault role and expected diagnostics disagree`,
    );
    requireCondition(
      (spec.role === "composition") === (spec.composition_facets.length > 0),
      `${spec.case_id} composition role and cited facets disagree`,
    );
    for (const facet of spec.composition_facets) {
      requireCondition(
        COMPOSITION_FACETS.includes(facet),
        `${spec.case_id} cites unknown composition facet ${facet}`,
      );
    }
    const sortedCodes = [...spec.expected_reported_codes].sort(
      (left, right) => left - right,
    );
    requireCondition(
      canonical(sortedCodes) === canonical(spec.expected_reported_codes),
      `${spec.case_id} expected diagnostics are not sorted`,
    );
    const tokens = spec.markers.map((entry) => entry.token);
    requireCondition(
      new Set(tokens).size === tokens.length,
      `${spec.case_id} marker tokens collide`,
    );
  }
  return specs;
}

function mappingsOf(builtCase) {
  const mapWrite = builtCase.observation.writes.find((write) =>
    write.path.endsWith(".js.map"),
  );
  requireCondition(
    mapWrite !== undefined,
    `${builtCase.case_id} has no map write for the mappings cross-check`,
  );
  const parsed = JSON.parse(
    Buffer.from(mapWrite.callback_utf8_base64, "base64").toString("utf8"),
  );
  return parsed.mappings;
}

function buildFamilies(adoptedObservations) {
  const specs = flattenCaseSpecs();
  const casesById = new Map(
    specs.map((spec) => [spec.case_id, buildWitnessCase(spec, adoptedObservations)]),
  );
  return WITNESS_FAMILY_SPECS.map((family) => {
    const cases = family.cases.map((caseSpec) =>
      casesById.get(`${family.family_id}--${caseSpec.case_slug}`),
    );
    const roles = cases.map((item) => item.role);
    if (!family.rust_boundary) {
      requireCondition(
        roles.includes("positive") && roles.includes("adjacent-negative-control"),
        `${family.family_id} is missing a positive or adjacent-negative case`,
      );
    } else {
      requireCondition(
        cases.every((item) => item.rust_lane === "refusal-control"),
        `${family.family_id} must contain only refusal-control cases`,
      );
    }
    // Vacuity check over the whole write set (the map bytes of a
    // newline pair are deliberately identical; the js bytes differ).
    const writeSetHashes = cases.map((item) =>
      sha256(
        Buffer.from(
          item.observation.writes
            .map((write) => `${write.path}\u0000${write.callback_utf8_sha256}`)
            .join(""),
          "utf8",
        ),
      ),
    );
    requireCondition(
      new Set(writeSetHashes).size === writeSetHashes.length,
      `${family.family_id} contains byte-identical case write sets - the family is vacuous`,
    );
    return withFingerprint(
      {
        family_id: family.family_id,
        rust_boundary: family.rust_boundary,
        description: family.description,
        cases,
      },
      "family_fingerprint_sha256",
    );
  });
}

function buildArtifact() {
  validateRuntime();
  const typescript = typescriptRecord();
  const families = buildFamilies(reusableStoredObservations(typescript));
  const cases = families.flatMap((family) => family.cases);
  const roleCount = (role) => cases.filter((item) => item.role === role).length;
  const laneCount = (lane) =>
    cases.filter((item) => item.rust_lane === lane).length;
  const citedFacets = [
    ...new Set(cases.flatMap((item) => item.composition_facets)),
  ].sort();
  requireCondition(
    canonical(citedFacets) === canonical([...COMPOSITION_FACETS].sort()),
    "composition facet coverage changed",
  );
  requireCondition(
    families.length === 10 && cases.length === 36,
    "witness census changed",
  );
  requireCondition(
    roleCount("positive") === 16 &&
      roleCount("adjacent-negative-control") === 13 &&
      roleCount("composition") === 4 &&
      roleCount("fault") === 3,
    "witness role census changed",
  );
  requireCondition(
    laneCount("parity") === 31 && laneCount("refusal-control") === 5,
    "witness rust-lane census changed",
  );
  // Cross-case invariant: the newline pair proves mappings are
  // byte-independent of the emitted newline sequence.
  const byId = new Map(cases.map((item) => [item.case_id, item]));
  requireCondition(
    mappingsOf(byId.get("newline-variants--positive-crlf")) ===
      mappingsOf(byId.get("newline-variants--adjacent-negative-lf")),
    "CRLF/LF mappings diverged - the newline invariant is broken",
  );
  return withFingerprint(
    {
      schema: 1,
      kind: "h2-source-map-witnesses",
      status: "frozen-source-map-witnesses",
      phase: SLICE,
      slice_id: SLICE,
      sub_packet: SUB_PACKET,
      plan_step: "packet-witness-freeze",
      typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      lineage: {
        packet: PACKET_RELATIVE_PATH,
        trusted_base: TRUSTED_BASE,
        interpretation:
          "W-H2.6A freezes oracle-captured external single-file source-map bytes, callback metadata, and write/list order for the h2-6a ladder; it authorizes no production edit and activates nothing",
      },
      composition_facets: COMPOSITION_FACETS,
      families,
      summary: {
        families: families.length,
        cases: cases.length,
        positive_cases: roleCount("positive"),
        adjacent_negative_controls: roleCount("adjacent-negative-control"),
        composition_cases: roleCount("composition"),
        fault_cases: roleCount("fault"),
        parity_cases: laneCount("parity"),
        refusal_control_cases: laneCount("refusal-control"),
        composition_facets_covered: citedFacets,
        typescript_oracle_runs: cases.reduce(
          (sum, item) => sum + item.repetitions,
          0,
        ),
        rust_runs: 0,
        runtime_admissions_delta: 0,
      },
    },
    "witnesses_fingerprint_sha256",
  );
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

const mode = process.argv[2];
if (mode === INTERNAL_OBSERVE_MODE) {
  requireCondition(
    process.argv.length === 4,
    "internal witness observation requires one case id",
  );
  validateRuntime();
  const caseSpec = flattenCaseSpecs().find(
    (item) => item.case_id === process.argv[3],
  );
  requireCondition(
    caseSpec !== undefined,
    `unknown internal witness case ${process.argv[3]}`,
  );
  process.stdout.write(render(observeWitnessCase(caseSpec)));
} else {
  requireCondition(
    mode === "--write" || mode === "--check",
    "usage: h2-6a-witnesses.mjs [--write|--check]",
  );
  const artifact = buildArtifact();
  const rendered = render(artifact);
  if (mode === "--write") {
    writeFileAtomic(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
    process.stdout.write(
      `wrote ${TARGET_RELATIVE_PATH}: families=${artifact.summary.families} cases=${artifact.summary.cases} oracle_runs=${artifact.summary.typescript_oracle_runs} adopted_cases=${adoptedCases} oracle_runs_saved=${adoptedCases * 2}\n`,
    );
  } else {
    requireCondition(
      fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
        fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") ===
          rendered,
      `stale ${TARGET_RELATIVE_PATH}; run h2-6a-witnesses.mjs --write and review`,
    );
    process.stdout.write(
      `H2.6a source-map witnesses are fresh: families=${artifact.summary.families} cases=${artifact.summary.cases}\n`,
    );
  }
}
