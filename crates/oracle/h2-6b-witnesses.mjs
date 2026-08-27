// W-H2.6B — frozen source-map witness machine for the H2.6b slice
// (docs/design/greenfield/slices/h2-6b.md §9).
//
// Every observation is captured from the pinned TypeScript 6.0.3 runtime
// in a fresh Node process, twice, exactly like the frozen W-H2.6A
// machine this generator descends from. Beyond those serializers it
// cross-checks the INLINE lane (the data-URI payload base64-decodes to
// exactly the emitResult.sourceMaps stringification and no `.js.map`
// write occurs) and drives the sanctioned map sink-fault route (the
// write callback invokes its onError argument for a selected path —
// upstream's writeFile helper 16644-16655 converts exactly that into
// TS5033; a throwing host would escape emit entirely). Expected bytes
// are never hand-authored: structural expectations (write paths,
// diagnostic codes, emitSkipped) are machine-validated inputs, and the
// oracle bytes are the authority.
//
// Every case is `parity`: the h2-6b-m-1/m-2 fixture gates replay each
// one byte-exact in Rust once the four option refusals lift. The
// refusal-control lane lives in W-H2.6A `boundary-controls`, whose five
// frozen cases the m-2 packet flips.

import crypto from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-6b-witnesses.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-6b-witnesses.v1.json";
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-6b-witnesses.schema.json";
const PACKET_RELATIVE_PATH = "docs/design/greenfield/slices/h2-6b.md";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const TYPESCRIPT_LIB_DIRECTORY = "vendor/typescript-6.0.3/lib";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const SLICE = "H2.6b";
const SUB_PACKET = "W-H2.6B";
const TRUSTED_BASE = "cd27f25a6f2f310878650a70705aacbd763ca2e7";
const INTERNAL_OBSERVE_MODE = "--internal-observe-witness";
const SINK_FAULT_MESSAGE = "W-H2.6B simulated sink failure";
const INLINE_URL_PREFIX = "//# sourceMappingURL=data:application/json;base64,";

const ROLES = Object.freeze([
  "positive",
  "adjacent-negative-control",
  "composition",
  "fault",
]);

// Every W-H2.6B case is parity (packet §9); the lane field is kept so
// the artifact shape stays homologous with W-H2.6A.
const RUST_LANES = Object.freeze(["parity"]);

const COMPOSITION_FACETS = Object.freeze([
  "lowered-class",
  "generators-machine",
  "helper-prologue",
  "module-wrapper",
  "multi-unit",
  "nested-sources",
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
    roots:
      extra.roots ??
      files.map((entry) => entry.path).filter((p) => !p.endsWith(".json")),
    option_overrides: extra.option_overrides ?? {},
    expected_reported_codes: extra.expected_reported_codes ?? [],
    expected_emit_codes: extra.expected_emit_codes ?? [],
    expected_emit_skipped: extra.expected_emit_skipped ?? false,
    expected_write_paths: extra.expected_write_paths,
    fault_sink_paths: extra.fault_sink_paths ?? [],
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

const NEWLINE_SOURCE = [
  "declare function sink(value: unknown): void;",
  "const left = 1;",
  "const right = 2;",
  "sink(left + right);",
  "",
];

const IMPORTING_MAIN_SOURCE = [
  "import { util } from \"../lib/util\";",
  "declare function sink(value: unknown): void;",
  "sink(util(3));",
  "",
];

const UTIL_SOURCE = [
  "export function util(value: number): number {",
  "  return value * 2;",
  "}",
  "",
];

const GENERATOR_LOOP_SOURCE = [
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
];

const WITNESS_FAMILY_SPECS = Object.freeze([
  {
    family_id: "inline-map",
    description:
      "inlineSourceMap alone: the URL comment is the base64 data URI whose payload byte-equals the emitResult map stringification, no .js.map write exists, emittedFiles lists the js only, and sourceMapUrlPos addresses the data-URI comment",
    cases: [
      witnessCase(
        "positive-inline-plain",
        "positive",
        "parity",
        "plain program under inlineSourceMap with listEmittedFiles: single js write carrying the data URI; the census proves its mappings equal the external lane's",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: {
            sourceMap: false,
            inlineSourceMap: true,
            listEmittedFiles: true,
          },
          expected_write_paths: ["/project/input.js"],
        },
      ),
      witnessCase(
        "composition-inline-lowered-class",
        "composition",
        "parity",
        "ES5 class lowering under the inline lane: extends helper and prototype methods map inside the data URI",
        [file("/project/input.ts", CLASS_SOURCE)],
        {
          option_overrides: { sourceMap: false, inlineSourceMap: true },
          composition_facets: ["lowered-class"],
          expected_write_paths: ["/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-external-plain",
        "adjacent-negative-control",
        "parity",
        "the identical program in the external H2.6a lane: map-then-js writes, basename URL",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "fault-noemitonerror-inline",
        "fault",
        "parity",
        "noEmitOnError with a type error under inlineSourceMap: zero writes, emitSkipped, no sourceMaps entries",
        [
          file("/project/input.ts", [
            "const wrong: number = \"text\";",
            "export const keep = wrong;",
            "",
          ]),
        ],
        {
          option_overrides: {
            sourceMap: false,
            inlineSourceMap: true,
            noEmitOnError: true,
          },
          expected_reported_codes: [2322],
          expected_emit_codes: [2322],
          expected_emit_skipped: true,
          expected_write_paths: [],
        },
      ),
    ],
  },
  {
    family_id: "inline-sources",
    description:
      "inlineSources populates sourcesContent (null-padded by registration index, key after mappings) under BOTH map modes; a registered source with zero mappings still contributes its full text",
    cases: [
      witnessCase(
        "positive-external-content",
        "positive",
        "parity",
        "external map with inlineSources: sourcesContent joins the map JSON",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { inlineSources: true },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "positive-inline-content",
        "positive",
        "parity",
        "inline map with inlineSources: the data-URI payload carries sourcesContent",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: {
            sourceMap: false,
            inlineSourceMap: true,
            inlineSources: true,
          },
          expected_write_paths: ["/project/input.js"],
        },
      ),
      witnessCase(
        "positive-comments-only-content",
        "positive",
        "parity",
        "a comments-only source under removeComments with inlineSources: the source registers with ZERO mappings yet sourcesContent carries its full text",
        [file("/project/input.ts", ["// the only line", ""])],
        {
          option_overrides: { inlineSources: true, removeComments: true },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-no-content",
        "adjacent-negative-control",
        "parity",
        "the identical program without inlineSources: the sourcesContent key is dropped by JSON.stringify",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
    ],
  },
  {
    family_id: "source-root-shapes",
    description:
      "sourceRoot value shapes: the map field carries the trailing-slash form verbatim and sources[] relativize against the COMMON SOURCE DIRECTORY instead of the js output directory",
    cases: [
      witnessCase(
        "positive-relative-root",
        "positive",
        "parity",
        "relative sourceRoot ('src' -> 'src/') with a nested source under outDir: sources[] become common-directory-relative",
        [file("/project/src/nested/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { sourceRoot: "src", outDir: "/project/out" },
          expected_write_paths: [
            "/project/out/input.js.map",
            "/project/out/input.js",
          ],
        },
      ),
      witnessCase(
        "positive-url-root",
        "positive",
        "parity",
        "URL sourceRoot passes through with a trailing slash appended",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { sourceRoot: "https://cdn.example.invalid/app" },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "positive-absolute-root",
        "positive",
        "parity",
        "absolute-path sourceRoot",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { sourceRoot: "/served/sources" },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-empty-root",
        "adjacent-negative-control",
        "parity",
        "no sourceRoot: the field stays the emitted empty string and sources relativize against the js output directory (the H2.6a lane)",
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
        "composition-multi-unit-roots",
        "composition",
        "parity",
        "two importing units under sourceRoot + outDir: per-unit maps, CommonJS wrappers, both units' sources common-directory-relative",
        [
          file("/project/src/nested/input.ts", IMPORTING_MAIN_SOURCE),
          file("/project/src/lib/util.ts", UTIL_SOURCE),
        ],
        {
          option_overrides: { sourceRoot: "src", outDir: "/project/out" },
          composition_facets: ["module-wrapper", "multi-unit"],
          expected_write_paths: [
            "/project/out/lib/util.js.map",
            "/project/out/lib/util.js",
            "/project/out/nested/input.js.map",
            "/project/out/nested/input.js",
          ],
        },
      ),
    ],
  },
  {
    family_id: "map-root-shapes",
    description:
      "mapRoot changes the URL the consumer follows (rooted = absolute encodeURI'd URL; relative = resolved against the common source directory then relativized FROM the js directory) and the sources relativization base, while the .js.map artifact stays beside the js",
    cases: [
      witnessCase(
        "positive-rooted",
        "positive",
        "parity",
        "rooted mapRoot: absolute URL in the comment, map still written beside the js",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { mapRoot: "/project/maps" },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "positive-relative",
        "positive",
        "parity",
        "relative mapRoot with outDir: resolves against the common source directory and the URL climbs from the js output directory",
        [file("/project/src/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { mapRoot: "maps", outDir: "/project/out" },
          expected_write_paths: [
            "/project/out/input.js.map",
            "/project/out/input.js",
          ],
        },
      ),
      witnessCase(
        "positive-nested",
        "positive",
        "parity",
        "two units in different directories under mapRoot + outDir: the per-file nesting arm (getSourceFilePathInNewDir) gives each unit its own map directory",
        [
          file("/project/src/nested/input.ts", IMPORTING_MAIN_SOURCE),
          file("/project/src/lib/util.ts", UTIL_SOURCE),
        ],
        {
          option_overrides: { mapRoot: "/project/maps", outDir: "/project/out" },
          expected_write_paths: [
            "/project/out/lib/util.js.map",
            "/project/out/lib/util.js",
            "/project/out/nested/input.js.map",
            "/project/out/nested/input.js",
          ],
        },
      ),
      witnessCase(
        "adjacent-negative-plain",
        "adjacent-negative-control",
        "parity",
        "no mapRoot: basename URL (the H2.6a lane)",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "composition-lowered-maproot",
        "composition",
        "parity",
        "ES5 class lowering under a rooted mapRoot",
        [file("/project/input.ts", CLASS_SOURCE)],
        {
          option_overrides: { mapRoot: "/project/maps" },
          composition_facets: ["lowered-class"],
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
    ],
  },
  {
    family_id: "root-composition",
    description:
      "sourceRoot and mapRoot together: sourceRoot wins the sources-directory selection (common source directory) while mapRoot still owns the URL",
    cases: [
      witnessCase(
        "positive-both-roots",
        "positive",
        "parity",
        "both roots on a flat program",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { sourceRoot: "src", mapRoot: "/project/maps" },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-sourceroot-only",
        "adjacent-negative-control",
        "parity",
        "sourceRoot alone as the adjacent control",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { sourceRoot: "src" },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "composition-generators-both-roots",
        "composition",
        "parity",
        "generator + captured for-of at ES5 under both roots, outDir, and a nested source: the state machine and helper prologue map under the root lanes",
        [file("/project/src/nested/input.ts", GENERATOR_LOOP_SOURCE)],
        {
          option_overrides: {
            sourceRoot: "src",
            mapRoot: "/project/maps",
            outDir: "/project/out",
            downlevelIteration: true,
          },
          composition_facets: [
            "generators-machine",
            "helper-prologue",
            "nested-sources",
          ],
          expected_write_paths: [
            "/project/out/input.js.map",
            "/project/out/input.js",
          ],
          markers: [{ token: "__generator", expectation: "present" }],
        },
      ),
    ],
  },
  {
    family_id: "url-placement",
    description:
      "URL comment placement polarities under the inline lane: CRLF vs LF change the emitted bytes but not the payload mappings; a percent-encodable file name affects the external URL (encodeURI) but never the data URI",
    cases: [
      witnessCase(
        "positive-inline-crlf",
        "positive",
        "parity",
        "CRLF inline output",
        [file("/project/input.ts", NEWLINE_SOURCE)],
        {
          option_overrides: {
            sourceMap: false,
            inlineSourceMap: true,
            newLine: ts.NewLineKind.CarriageReturnLineFeed,
          },
          expected_write_paths: ["/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-inline-lf",
        "adjacent-negative-control",
        "parity",
        "LF inline output of the same program; the census asserts its payload mappings equal the CRLF case's",
        [file("/project/input.ts", NEWLINE_SOURCE)],
        {
          option_overrides: { sourceMap: false, inlineSourceMap: true },
          expected_write_paths: ["/project/input.js"],
        },
      ),
      witnessCase(
        "positive-percent-inline",
        "positive",
        "parity",
        "a file name needing URL escaping under the inline lane: the data URI is unaffected by encodeURI",
        [file("/project/input space.ts", PLAIN_SOURCE)],
        {
          option_overrides: { sourceMap: false, inlineSourceMap: true },
          expected_write_paths: ["/project/input space.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-percent-external",
        "adjacent-negative-control",
        "parity",
        "the same percent-encodable name in the external lane: the URL comment is encodeURI-escaped while sources[] stays raw",
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
    family_id: "sink-fault",
    description:
      "map sink faults through the sanctioned onError route (writeFile helper 16644-16655): the failing path yields TS5033 in emitResult.diagnostics, both writes are still attempted in order, and emitSkipped stays false",
    cases: [
      witnessCase(
        "fault-map-sink",
        "fault",
        "parity",
        "the .js.map sink fails: map write attempted first, js write still follows, one TS5033",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          fault_sink_paths: ["/project/input.js.map"],
          expected_emit_codes: [5033],
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "fault-js-sink",
        "fault",
        "parity",
        "the .js sink fails: the map was already written cleanly, one TS5033 for the js path",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          fault_sink_paths: ["/project/input.js"],
          expected_emit_codes: [5033],
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-healthy",
        "adjacent-negative-control",
        "parity",
        "the identical program with a healthy sink",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
    ],
  },
  {
    family_id: "option-conflicts",
    description:
      "the §4.1 option lattice as observed context: TS5053 inlineSourceMap conflicts, TS5051 map-option requirements, TS5069 mapRoot requirement; emit behavior under each conflict is frozen (the inline lane wins the artifact shape when inlineSourceMap is set)",
    cases: [
      witnessCase(
        "fault-inline-with-sourcemap",
        "fault",
        "parity",
        "sourceMap + inlineSourceMap: TS5053; the emitted shape is the inline lane (no map write)",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { inlineSourceMap: true },
          expected_reported_codes: [5053],
          expected_write_paths: ["/project/input.js"],
        },
      ),
      witnessCase(
        "fault-inline-with-maproot",
        "fault",
        "parity",
        "inlineSourceMap + mapRoot: TS5053; inline shape",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: {
            sourceMap: false,
            inlineSourceMap: true,
            mapRoot: "/project/maps",
          },
          expected_reported_codes: [5053, 5069],
          expected_write_paths: ["/project/input.js"],
        },
      ),
      witnessCase(
        "fault-bare-inline-sources",
        "fault",
        "parity",
        "inlineSources without any map option: TS5051 and an unmapped emit",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { sourceMap: false, inlineSources: true },
          expected_reported_codes: [5051],
          expected_write_paths: ["/project/input.js"],
        },
      ),
      witnessCase(
        "fault-bare-sourceroot",
        "fault",
        "parity",
        "sourceRoot without any map option: TS5051 and an unmapped emit",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { sourceMap: false, sourceRoot: "src" },
          expected_reported_codes: [5051],
          expected_write_paths: ["/project/input.js"],
        },
      ),
      witnessCase(
        "fault-maproot-alone",
        "fault",
        "parity",
        "mapRoot without sourceMap or declarationMap: TS5069 and an unmapped emit",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: { sourceMap: false, mapRoot: "/project/maps" },
          expected_reported_codes: [5069],
          expected_write_paths: ["/project/input.js"],
        },
      ),
      witnessCase(
        "adjacent-negative-full-valid-combo",
        "adjacent-negative-control",
        "parity",
        "sourceMap + sourceRoot + mapRoot + inlineSources all together: the fully valid lattice corner, no diagnostics",
        [file("/project/input.ts", PLAIN_SOURCE)],
        {
          option_overrides: {
            sourceRoot: "src",
            mapRoot: "/project/maps",
            inlineSources: true,
          },
          expected_write_paths: ["/project/input.js.map", "/project/input.js"],
        },
      ),
    ],
  },
]);

function fail(message) {
  process.stderr.write(`h2-6b-witnesses: ${message}\n`);
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

// Write-side observation adoption exactly as W-H2.6A: all-or-nothing key
// over this generator's bytes and the full typescript record. Only the
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
    stored.kind !== "h2-6b-source-map-witnesses" ||
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
    // W-H2.6B extension over the 6a shape: the flattened message text is
    // itself a frozen observable — the sink-fault diagnostics carry the
    // failing path nowhere else.
    message: ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
  };
}

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

function inlinePayloadJson(caseId, write) {
  const text = Buffer.from(write.callback_utf8_base64, "base64").toString(
    "utf8",
  );
  requireCondition(
    write.data_source_map_url_pos !== null,
    `${caseId} inline write ${write.path} lacks sourceMapUrlPos`,
  );
  const comment = text.slice(write.data_source_map_url_pos);
  requireCondition(
    comment.startsWith(INLINE_URL_PREFIX),
    `${caseId} inline write ${write.path} does not carry a base64 data URI at sourceMapUrlPos`,
  );
  return Buffer.from(comment.slice(INLINE_URL_PREFIX.length), "base64").toString(
    "utf8",
  );
}

function observeWitnessCase(caseSpec) {
  const control = witnessControl(caseSpec);
  const faultPaths = new Set(
    caseSpec.fault_sink_paths.map((entry) => ts.normalizePath(entry)),
  );
  const program = createVirtualProgram(control);
  const reportedDiagnostics = ts.getPreEmitDiagnostics(program);
  const writes = [];
  const emitResult = program.emit(
    undefined,
    (...arguments_) => {
      writes.push(arguments_);
      // gate-tax-free sanctioned fault route (packet §4.8): the helper's
      // onError is argument four; invoking it is the ONLY way a sink
      // failure reaches emitterDiagnostics.
      const [fileName, , , onError] = arguments_;
      if (faultPaths.has(ts.normalizePath(fileName))) {
        requireCondition(
          typeof onError === "function",
          `${caseSpec.case_id} sink-fault path ${fileName} received no onError callback`,
        );
        onError(SINK_FAULT_MESSAGE);
      }
    },
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
  const observedEmitCodes = emitResult.diagnostics
    .map((diagnostic) => diagnostic.code)
    .sort((left, right) => left - right);
  requireCondition(
    canonical(observedEmitCodes) === canonical(caseSpec.expected_emit_codes),
    `${caseSpec.case_id} emit diagnostics [${observedEmitCodes.join(", ")}], expected [${caseSpec.expected_emit_codes.join(", ")}]`,
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
  const inlineLane = control.compiler_options.inlineSourceMap === true;
  const mapWrites = serializedWrites.filter((write) =>
    write.path.endsWith(".js.map"),
  );
  if (inlineLane) {
    // Inline-lane invariants (packet §9): no external map artifact; every
    // mapped js unit carries a data URI whose payload byte-equals the
    // ordinal emitResult.sourceMaps stringification.
    requireCondition(
      mapWrites.length === 0,
      `${caseSpec.case_id} wrote a .js.map under inlineSourceMap`,
    );
    const mappedJsWrites = serializedWrites.filter(
      (write) => write.data_source_map_url_pos !== null,
    );
    if (mappedJsWrites.length > 0 || (sourceMaps?.length ?? 0) > 0) {
      requireCondition(
        sourceMaps !== null && sourceMaps.length === mappedJsWrites.length,
        `${caseSpec.case_id} has ${mappedJsWrites.length} inline-mapped js writes but ${sourceMaps?.length ?? 0} sourceMaps entries`,
      );
      mappedJsWrites.forEach((write, ordinal) => {
        requireCondition(
          inlinePayloadJson(caseSpec.case_id, write) ===
            sourceMaps[ordinal].source_map_json,
          `${caseSpec.case_id} inline payload of ${write.path} diverges from emitResult.sourceMaps[${ordinal}]`,
        );
      });
    }
  } else if (mapWrites.length > 0) {
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
      expected_emit_codes: caseSpec.expected_emit_codes,
      expected_emit_skipped: caseSpec.expected_emit_skipped,
      expected_write_paths: caseSpec.expected_write_paths,
      fault_sink_paths: caseSpec.fault_sink_paths,
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
      Array.isArray(spec.expected_write_paths),
      `${spec.case_id} does not declare its expected write paths`,
    );
    requireCondition(
      (spec.role === "fault") ===
        (spec.expected_reported_codes.length > 0 ||
          spec.expected_emit_codes.length > 0),
      `${spec.case_id} fault role and expected diagnostics disagree`,
    );
    requireCondition(
      spec.fault_sink_paths.every((entry) =>
        spec.expected_write_paths.includes(entry),
      ),
      `${spec.case_id} declares a sink-fault path it never writes`,
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

function externalMappingsOf(builtCase) {
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

function inlineMappingsOf(builtCase) {
  const jsWrite = builtCase.observation.writes.find(
    (write) => write.data_source_map_url_pos !== null,
  );
  requireCondition(
    jsWrite !== undefined,
    `${builtCase.case_id} has no inline-mapped js write for the mappings cross-check`,
  );
  return JSON.parse(inlinePayloadJson(builtCase.case_id, jsWrite)).mappings;
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
    // Every family carries its adjacent-negative control plus at least
    // one non-negative case (positive, composition, or fault) — the
    // sink-fault and option-conflict families are fault-led by design.
    requireCondition(
      roles.includes("adjacent-negative-control") &&
        roles.some((role) => role !== "adjacent-negative-control"),
      `${family.family_id} is missing its adjacent-negative control or any exercising case`,
    );
    // Vacuity is judged over the write set AND the diagnostic codes: the
    // sink-fault cases write byte-identical outputs by design — their
    // discriminating observable is the TS5033 emit diagnostic the
    // onError route produced.
    const observableHashes = cases.map((item) =>
      sha256(
        Buffer.from(
          item.observation.writes
            .map((write) => `${write.path}\u0000${write.callback_utf8_sha256}`)
            .join("") +
            "\u0000codes\u0000" +
            [
              ...item.observation.reported_diagnostics,
              ...item.observation.emit_diagnostics,
            ]
              .map((diagnostic) => `${diagnostic.code}:${diagnostic.message}`)
              .join(","),
          "utf8",
        ),
      ),
    );
    requireCondition(
      new Set(observableHashes).size === observableHashes.length,
      `${family.family_id} contains observationally identical cases - the family is vacuous`,
    );
    return withFingerprint(
      {
        family_id: family.family_id,
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
  const citedFacets = [
    ...new Set(cases.flatMap((item) => item.composition_facets)),
  ].sort();
  requireCondition(
    canonical(citedFacets) === canonical([...COMPOSITION_FACETS].sort()),
    "composition facet coverage changed",
  );
  requireCondition(
    families.length === 8 && cases.length === 34,
    "witness census changed",
  );
  requireCondition(
    roleCount("positive") === 13 &&
      roleCount("adjacent-negative-control") === 9 &&
      roleCount("composition") === 4 &&
      roleCount("fault") === 8,
    "witness role census changed",
  );
  const byId = new Map(cases.map((item) => [item.case_id, item]));
  // Cross-case invariants: (1) the inline data-URI payload's mappings
  // equal the external lane's for the identical program; (2) CRLF vs LF
  // inline outputs share one mappings string.
  requireCondition(
    inlineMappingsOf(byId.get("inline-map--positive-inline-plain")) ===
      externalMappingsOf(byId.get("inline-map--adjacent-negative-external-plain")),
    "inline/external mappings diverged for the identical program",
  );
  requireCondition(
    inlineMappingsOf(byId.get("url-placement--positive-inline-crlf")) ===
      inlineMappingsOf(byId.get("url-placement--adjacent-negative-inline-lf")),
    "CRLF/LF inline mappings diverged - the newline invariant is broken",
  );
  return withFingerprint(
    {
      schema: 1,
      kind: "h2-6b-source-map-witnesses",
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
          "W-H2.6B freezes oracle-captured inline-map/inline-sources/root-lane/URL-lane/sink-fault source-map bytes, callback metadata, and write/list order for the h2-6b ladder; it authorizes no production edit and activates nothing",
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
        parity_cases: cases.length,
        refusal_control_cases: 0,
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
    "usage: h2-6b-witnesses.mjs [--write|--check]",
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
      `stale ${TARGET_RELATIVE_PATH}; run h2-6b-witnesses.mjs --write and review`,
    );
    process.stdout.write(
      `H2.6b source-map witnesses are fresh: families=${artifact.summary.families} cases=${artifact.summary.cases}\n`,
    );
  }
}
