import crypto from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH =
  "crates/oracle/h2-5h-a-comment-scope-witnesses.mjs";
const TARGET_RELATIVE_PATH =
  "ratchets/h2-5h-a-comment-scope-witnesses.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-5h-a-comment-scope-witnesses.schema.json";
const FOUNDATION_RELATIVE_PATH = "ratchets/h2-5h-a-foundation.v1.json";
const SCOPE_STUDY_RELATIVE_PATH =
  "docs/design/greenfield/slices/h2-5h-a-comment-scope.md";
const HANDOFF_RELATIVE_PATH = "docs/design/greenfield/slices/h2-5h-a.md";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const TYPESCRIPT_LIB_DIRECTORY = "vendor/typescript-6.0.3/lib";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const SLICE = "H2.5h-a";
const SUB_PACKET = "E-COMMENT-SCOPE-H";
const INTERNAL_OBSERVE_MODE = "--internal-observe-witness";

// The complete pinned occurrence set of the scoped comment triple in the
// vendored implementation bundle. The study's grep-completeness claim is
// machine-checked: a scan of every line for the same alternation must
// produce exactly these 1-based line numbers.
const SCOPE_GRAPH_PATTERN =
  "containerPos|containerEnd|declarationListContainerEnd";
const SCOPE_GRAPH_OCCURRENCE_LINES = Object.freeze([
  116957, 116958, 116959, 118404, 118405, 118406, 118417, 118418, 118419,
  118456, 118457, 118458, 120980, 120981, 120982, 121021, 121024, 121026,
  121038, 121039, 121040, 121220, 121235,
]);

// The six site clusters of the frozen scope-graph study, pinned as exact
// line spans with slice hashes. Anchors are complete definition lines and
// must be unique in the bundle at exactly the claimed start line.
const SCOPE_GRAPH_SPANS = Object.freeze([
  Object.freeze({
    span_id: "printer-scope-state",
    role: "state",
    start_line: 116957,
    end_line: 116959,
    anchor: "  var containerPos = -1;",
  }),
  Object.freeze({
    span_id: "binary-trampoline-carrier",
    role: "save-restore-carrier-iterative",
    start_line: 118390,
    end_line: 118468,
    anchor: "  function createEmitBinaryExpression() {",
  }),
  Object.freeze({
    span_id: "pipeline-emit-with-comments",
    role: "save-restore-carrier-recursive",
    start_line: 120978,
    end_line: 120986,
    anchor: "  function pipelineEmitWithComments(hint, node) {",
  }),
  Object.freeze({
    span_id: "emit-leading-comments-of-node",
    role: "set-site",
    start_line: 121007,
    end_line: 121032,
    anchor: "  function emitLeadingCommentsOfNode(node, emitFlags, pos, end) {",
  }),
  Object.freeze({
    span_id: "emit-trailing-comments-of-node",
    role: "restore-site",
    start_line: 121033,
    end_line: 121052,
    anchor:
      "  function emitTrailingCommentsOfNode(node, emitFlags, pos, end, savedContainerPos, savedContainerEnd, savedDeclarationListContainerEnd) {",
  }),
  Object.freeze({
    span_id: "for-each-leading-comment-to-emit",
    role: "guarded-reader-leading",
    start_line: 121219,
    end_line: 121233,
    anchor: "  function forEachLeadingCommentToEmit(pos, cb) {",
  }),
  Object.freeze({
    span_id: "for-each-trailing-comment-to-emit",
    role: "guarded-reader-trailing",
    start_line: 121234,
    end_line: 121238,
    anchor: "  function forEachTrailingCommentToEmit(end, cb) {",
  }),
]);

const TRANSFORM_IDS = Object.freeze([
  "identity",
  "wrap-expression-statements-in-synthetic-arrow",
  "apply-comment-emit-flags",
  "add-synthetic-comments-to-marked",
  "append-synthetic-statement-with-synthetic-comments",
  "replace-not-emitted-and-zero-width",
]);

function witnessCase(role, source, extra = {}) {
  return {
    role,
    transform: extra.transform ?? "identity",
    remove_comments: extra.remove_comments ?? false,
    source,
    markers: extra.markers ?? [],
  };
}

function marker(token, expectation) {
  return { token, expectation };
}

// Witness inputs. Expected output bytes are never written here: every
// observation is captured from the pinned TypeScript runtime in a fresh
// process, twice. Marker expectations assert only non-vacuity
// (presence/absence of a comment token somewhere in the emitted JS);
// "recorded" markers are captured without assertion.
const WITNESS_FAMILY_SPECS = Object.freeze([
  {
    family_id: "synthetic-wrapper-relocation",
    study_item: 1,
    description:
      "the handoff counterexample: a statement with trailing trivia is moved into a synthetic arrow wrapper; the trailing walk runs under the restored parent scope and the comment emits with the relocated statement inside the wrapper",
    positive: witnessCase(
      "positive",
      "declare const x: number;\nx /*TAIL*/\n",
      {
        transform: "wrap-expression-statements-in-synthetic-arrow",
        markers: [marker("TAIL", "present")],
      },
    ),
    remove_comments: witnessCase(
      "remove-comments-control",
      "declare const x: number;\nx /*TAIL*/\n",
      {
        transform: "wrap-expression-statements-in-synthetic-arrow",
        remove_comments: true,
        markers: [marker("TAIL", "absent")],
      },
    ),
    adjacent_negative: witnessCase(
      "adjacent-negative-control",
      "declare const x: number;\nx /*TAIL*/\n",
      {
        markers: [marker("TAIL", "present")],
      },
    ),
  },
  {
    family_id: "container-start-shared-prefix",
    study_item: 2,
    description:
      "a direct child sharing the container's start position must not re-claim the same leading prefix (containerPos guard)",
    positive: witnessCase(
      "positive",
      "declare const x: number;\n/*LEAD*/ x;\n",
      {
        markers: [marker("LEAD", "present")],
      },
    ),
    remove_comments: witnessCase(
      "remove-comments-control",
      "declare const x: number;\n/*LEAD*/ x;\n",
      {
        remove_comments: true,
        markers: [marker("LEAD", "absent")],
      },
    ),
    adjacent_negative: witnessCase(
      "adjacent-negative-control",
      "declare const x: number;\n( /*LEAD*/ x);\n",
      {
        markers: [marker("LEAD", "present")],
      },
    ),
  },
  {
    family_id: "delimited-list-starts",
    study_item: 3,
    description:
      "leading comments at ordinary and multiline delimited-list element starts (list phase vs ordinary phase split)",
    positive: witnessCase(
      "positive",
      [
        "declare function f(a: number, b: number): void;",
        "f(/*a1*/ 1, /*a2*/ 2);",
        "f(",
        "  /*m1*/ 1,",
        "  /*m2*/ 2",
        ");",
        "",
      ].join("\n"),
      {
        markers: [
          marker("a1", "present"),
          marker("a2", "present"),
          marker("m1", "present"),
          marker("m2", "present"),
        ],
      },
    ),
    remove_comments: witnessCase(
      "remove-comments-control",
      [
        "declare function f(a: number, b: number): void;",
        "f(/*a1*/ 1, /*a2*/ 2);",
        "f(",
        "  /*m1*/ 1,",
        "  /*m2*/ 2",
        ");",
        "",
      ].join("\n"),
      {
        remove_comments: true,
        markers: [
          marker("a1", "absent"),
          marker("a2", "absent"),
          marker("m1", "absent"),
          marker("m2", "absent"),
        ],
      },
    ),
    adjacent_negative: witnessCase(
      "adjacent-negative-control",
      [
        "declare function f(a: number, b: number): void;",
        "f(1 /*t1*/, 2 /*t2*/);",
        "",
      ].join("\n"),
      {
        markers: [marker("t1", "present"), marker("t2", "present")],
      },
    ),
  },
  {
    family_id: "declaration-list-trailing-dedupe",
    study_item: 4,
    description:
      "declarationListContainerEnd dedupe: trailing trivia at the shared declarator/list end emits once at the list boundary",
    positive: witnessCase(
      "positive",
      "var a = 1, b = 2 /*T1*/;\nvar c = 3, d = 4; /*T2*/\n",
      {
        markers: [marker("T1", "present"), marker("T2", "present")],
      },
    ),
    remove_comments: witnessCase(
      "remove-comments-control",
      "var a = 1, b = 2 /*T1*/;\nvar c = 3, d = 4; /*T2*/\n",
      {
        remove_comments: true,
        markers: [marker("T1", "absent"), marker("T2", "absent")],
      },
    ),
    adjacent_negative: witnessCase(
      "adjacent-negative-control",
      "var g = 6 /*MID*/, h = 7;\n",
      {
        markers: [marker("MID", "present")],
      },
    ),
  },
  {
    family_id: "binary-trampoline-parity",
    study_item: 5,
    description:
      "comment scope across the iterative binary-expression trampoline stack must match the recursive carrier",
    positive: witnessCase(
      "positive",
      [
        "declare const a: number, b: number, c: number, d: number, e: number;",
        "const chain = /*c0*/ a /*c1*/ + /*c2*/ b /*c3*/ + /*c4*/ c /*c5*/ + /*c6*/ d /*c7*/ + /*c8*/ e /*c9*/;",
        "const nested = a + ( /*n1*/ b + ( /*n2*/ c + d /*n3*/ ) /*n4*/ );",
        "",
      ].join("\n"),
      {
        markers: [
          marker("c0", "present"),
          marker("c5", "present"),
          marker("c9", "present"),
          marker("n1", "present"),
          marker("n2", "present"),
          marker("n4", "present"),
        ],
      },
    ),
    remove_comments: witnessCase(
      "remove-comments-control",
      [
        "declare const a: number, b: number, c: number, d: number, e: number;",
        "const chain = /*c0*/ a /*c1*/ + /*c2*/ b /*c3*/ + /*c4*/ c /*c5*/ + /*c6*/ d /*c7*/ + /*c8*/ e /*c9*/;",
        "const nested = a + ( /*n1*/ b + ( /*n2*/ c + d /*n3*/ ) /*n4*/ );",
        "",
      ].join("\n"),
      {
        remove_comments: true,
        markers: [
          marker("c0", "absent"),
          marker("c9", "absent"),
          marker("n2", "absent"),
        ],
      },
    ),
    adjacent_negative: witnessCase(
      "adjacent-negative-control",
      [
        "declare const a: number, b: number, c: number, d: number, e: number;",
        "const chain = a + b + c + d + e;",
        "const nested = a + (b + (c + d));",
        "",
      ].join("\n"),
    ),
  },
  {
    family_id: "emit-flag-suppression",
    study_item: 6,
    description:
      "NoLeadingComments/NoTrailingComments/NoNestedComments interaction with the flag-suppressed container claim",
    positive: witnessCase(
      "positive",
      [
        "declare function setup(): void;",
        "declare function suppressLead(): void;",
        "declare function suppressTrail(): void;",
        "declare function nested(): void;",
        "setup();",
        "/*L1*/ suppressLead(); /*T1*/",
        "/*L2*/ suppressTrail(); /*T2*/",
        "{",
        "  /*B1*/ nested(); /*B2*/",
        "}",
        "setup();",
        "",
      ].join("\n"),
      {
        transform: "apply-comment-emit-flags",
        markers: [
          marker("L1", "absent"),
          marker("T1", "present"),
          marker("L2", "present"),
          marker("T2", "absent"),
          marker("B1", "absent"),
          marker("B2", "absent"),
        ],
      },
    ),
    remove_comments: witnessCase(
      "remove-comments-control",
      [
        "declare function setup(): void;",
        "declare function suppressLead(): void;",
        "declare function suppressTrail(): void;",
        "declare function nested(): void;",
        "setup();",
        "/*L1*/ suppressLead(); /*T1*/",
        "/*L2*/ suppressTrail(); /*T2*/",
        "{",
        "  /*B1*/ nested(); /*B2*/",
        "}",
        "setup();",
        "",
      ].join("\n"),
      {
        transform: "apply-comment-emit-flags",
        remove_comments: true,
        markers: [
          marker("L1", "absent"),
          marker("T1", "absent"),
          marker("L2", "absent"),
          marker("T2", "absent"),
          marker("B1", "absent"),
          marker("B2", "absent"),
        ],
      },
    ),
    adjacent_negative: witnessCase(
      "adjacent-negative-control",
      [
        "declare function setup(): void;",
        "declare function suppressLead(): void;",
        "declare function suppressTrail(): void;",
        "declare function nested(): void;",
        "setup();",
        "/*L1*/ suppressLead(); /*T1*/",
        "/*L2*/ suppressTrail(); /*T2*/",
        "{",
        "  /*B1*/ nested(); /*B2*/",
        "}",
        "setup();",
        "",
      ].join("\n"),
      {
        markers: [
          marker("L1", "present"),
          marker("T1", "present"),
          marker("L2", "present"),
          marker("T2", "present"),
          marker("B1", "present"),
          marker("B2", "present"),
        ],
      },
    ),
  },
  {
    family_id: "synthetic-comments-alongside-source",
    study_item: 7,
    description:
      "synthetic leading/trailing comments bypass the triple and interleave with source trivia in the pinned order",
    positive: witnessCase(
      "positive",
      "declare function markMe(): void;\n/*SRC-L*/ markMe(); /*SRC-T*/\n",
      {
        transform: "add-synthetic-comments-to-marked",
        markers: [
          marker("SRC-L", "present"),
          marker("SRC-T", "present"),
          marker("SYN-LEAD", "present"),
          marker("SYN-TRAIL", "present"),
        ],
      },
    ),
    remove_comments: witnessCase(
      "remove-comments-control",
      "declare function markMe(): void;\n/*SRC-L*/ markMe(); /*SRC-T*/\n",
      {
        transform: "add-synthetic-comments-to-marked",
        remove_comments: true,
        markers: [
          marker("SRC-L", "absent"),
          marker("SRC-T", "absent"),
          marker("SYN-LEAD", "absent"),
          marker("SYN-TRAIL", "absent"),
        ],
      },
    ),
    adjacent_negative: witnessCase(
      "adjacent-negative-control",
      "declare function markMe(): void;\nmarkMe();\n",
      {
        transform: "append-synthetic-statement-with-synthetic-comments",
        markers: [
          marker("SYN-LEAD", "present"),
          marker("SYN-TRAIL", "present"),
        ],
      },
    ),
  },
  {
    family_id: "detached-comment-reroute",
    study_item: 8,
    description:
      "detached comment ranges reroute the leading walk (hasDetachedComments adjacency) without touching the triple",
    positive: witnessCase(
      "positive",
      [
        "/*! FILE-HEADER */",
        "",
        "declare function body(): void;",
        "function wrapper() {",
        "  // DETACHED-1",
        "  // DETACHED-2",
        "",
        "  body();",
        "  return 1;",
        "}",
        "",
      ].join("\n"),
      {
        markers: [
          marker("FILE-HEADER", "present"),
          marker("DETACHED-1", "present"),
          marker("DETACHED-2", "present"),
        ],
      },
    ),
    remove_comments: witnessCase(
      "remove-comments-control",
      [
        "/*! FILE-HEADER */",
        "",
        "declare function body(): void;",
        "function wrapper() {",
        "  // DETACHED-1",
        "  // DETACHED-2",
        "",
        "  body();",
        "  return 1;",
        "}",
        "",
      ].join("\n"),
      {
        remove_comments: true,
        markers: [
          marker("FILE-HEADER", "present"),
          marker("DETACHED-1", "absent"),
          marker("DETACHED-2", "absent"),
        ],
      },
    ),
    adjacent_negative: witnessCase(
      "adjacent-negative-control",
      [
        "declare function body(): void;",
        "function wrapper() {",
        "  // ATTACHED-1",
        "  body();",
        "  return 1;",
        "}",
        "",
      ].join("\n"),
      {
        markers: [marker("ATTACHED-1", "present")],
      },
    ),
  },
  {
    family_id: "type-node-trailing-repetition",
    study_item: 9,
    description:
      "emitCommentsAfterNode repeats the trailing phase over getTypeNode(node)'s range so trivia trailing an erased annotation survives",
    positive: witnessCase(
      "positive",
      [
        "const typed: number /*TYPE-T*/ = 1;",
        "function h(): number /*RET-T*/ {",
        "  return 2;",
        "}",
        "",
      ].join("\n"),
      {
        markers: [marker("TYPE-T", "present"), marker("RET-T", "absent")],
      },
    ),
    remove_comments: witnessCase(
      "remove-comments-control",
      [
        "const typed: number /*TYPE-T*/ = 1;",
        "function h(): number /*RET-T*/ {",
        "  return 2;",
        "}",
        "",
      ].join("\n"),
      {
        remove_comments: true,
        markers: [marker("TYPE-T", "absent"), marker("RET-T", "absent")],
      },
    ),
    adjacent_negative: witnessCase(
      "adjacent-negative-control",
      "const untyped /*NO-T*/ = 1;\n",
      {
        markers: [marker("NO-T", "present")],
      },
    ),
  },
  {
    family_id: "zero-width-and-not-emitted",
    study_item: 10,
    description:
      "pos===end ranges fail the outer claim gate and NotEmittedStatement suppresses the trailing phase",
    positive: witnessCase(
      "positive",
      [
        "declare function keep(): void;",
        "declare function dropMe(): void;",
        "declare function shrinkMe(): void;",
        "keep(); /*KEEP-T*/",
        "dropMe(); /*DROPPED-T*/",
        "shrinkMe(); /*SHRUNK-T*/",
        "keep();",
        "",
      ].join("\n"),
      {
        transform: "replace-not-emitted-and-zero-width",
        markers: [
          marker("KEEP-T", "present"),
          marker("DROPPED-T", "absent"),
          marker("SHRUNK-T", "absent"),
        ],
      },
    ),
    remove_comments: witnessCase(
      "remove-comments-control",
      [
        "declare function keep(): void;",
        "declare function dropMe(): void;",
        "declare function shrinkMe(): void;",
        "keep(); /*KEEP-T*/",
        "dropMe(); /*DROPPED-T*/",
        "shrinkMe(); /*SHRUNK-T*/",
        "keep();",
        "",
      ].join("\n"),
      {
        transform: "replace-not-emitted-and-zero-width",
        remove_comments: true,
        markers: [
          marker("KEEP-T", "absent"),
          marker("DROPPED-T", "absent"),
          marker("SHRUNK-T", "absent"),
        ],
      },
    ),
    adjacent_negative: witnessCase(
      "adjacent-negative-control",
      [
        "declare function keep(): void;",
        "declare function dropMe(): void;",
        "declare function shrinkMe(): void;",
        "keep(); /*KEEP-T*/",
        "dropMe(); /*DROPPED-T*/",
        "shrinkMe(); /*SHRUNK-T*/",
        "keep();",
        "",
      ].join("\n"),
      {
        markers: [
          marker("KEEP-T", "present"),
          marker("DROPPED-T", "present"),
          marker("SHRUNK-T", "present"),
        ],
      },
    ),
  },
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

function fingerprintIsValid(record, field) {
  if (record === null || typeof record !== "object") return false;
  const { [field]: storedFingerprint, ...rest } = record;
  return (
    typeof storedFingerprint === "string" &&
    storedFingerprint === sha256(Buffer.from(canonical(rest), "utf8"))
  );
}

function libraryInventoryRecord() {
  // The fresh-process observations resolve default libraries from disk
  // through the base compiler host; those .d.ts bytes drive the type
  // check but are not covered by the bundle/implementation hashes, so
  // the record pins the whole vendored lib inventory (gate-tax 2 R3-2).
  const directory = path.join(WORKSPACE, TYPESCRIPT_LIB_DIRECTORY);
  const names = fs
    .readdirSync(directory)
    .filter((name) => name.startsWith("lib.") && name.endsWith(".d.ts"))
    .sort();
  requireCondition(
    names.length > 0,
    "vendored TypeScript lib inventory is empty",
  );
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
  // Same-directory temp + rename: a kill mid-write can never truncate
  // the artifact, which doubles as the adoption store (gate-tax 2 R4-1).
  const temporary = path.join(
    path.dirname(absolutePath),
    `.${path.basename(absolutePath)}.${process.pid}.tmp`,
  );
  fs.writeFileSync(temporary, contents);
  fs.renameSync(temporary, absolutePath);
}

let adoptedCases = 0;

// Write-side observation adoption (gate-tax 2). The adoption key is
// all-or-nothing and deliberately stricter than the 5g per-case
// fallback: the stored generator sha must byte-match this file (the
// case specs and transforms live here, and unlike 5g there is no
// per-gate --check backstop, only the once-per-slice packet checker),
// and the stored typescript record must byte-match the current one
// including the library inventory. Only the fresh-process oracle
// observations are adopted; every derivation, marker expectation,
// census guard, and lineage pin re-executes against current inputs on
// every write. --check never adopts: the packet checker's full
// re-observation remains the slice-boundary backstop.
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
    stored.kind !== "h2-comment-scope-witnesses" ||
    !fingerprintIsValid(stored, "witnesses_fingerprint_sha256") ||
    canonical(stored.generator) !==
      canonical(pathHash(GENERATOR_RELATIVE_PATH)) ||
    canonical(stored.typescript) !== canonical(currentTypescriptRecord)
  ) {
    return null;
  }
  const observations = new Map();
  for (const family of stored.families ?? []) {
    for (const storedCase of family.cases ?? []) {
      if (
        typeof storedCase.case_id === "string" &&
        fingerprintIsValid(
          storedCase.observation,
          "observation_fingerprint_sha256",
        )
      ) {
        observations.set(storedCase.case_id, storedCase.observation);
      }
    }
  }
  return observations;
}

function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function readText(relativePath) {
  return readBytes(relativePath).toString("utf8");
}

function readJson(relativePath) {
  return JSON.parse(readText(relativePath));
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

function compilerOptions(removeComments) {
  return {
    target: ts.ScriptTarget.ES2016,
    module: ts.ModuleKind.ESNext,
    newLine: ts.NewLineKind.LineFeed,
    removeComments,
  };
}

function serializeOptions(options) {
  return Object.fromEntries(
    Object.entries(options).sort(([left], [right]) => left.localeCompare(right)),
  );
}

function validateRuntime() {
  const node = readText(".node-version").trim();
  requireCondition(node === EXPECTED_NODE, "unexpected Node pin");
  requireCondition(process.version === `v${node}`, `requires Node ${node}`);
  requireCondition(ts.version === "6.0.3", "unexpected TypeScript runtime");
  for (const name of ["NoLeadingComments", "NoTrailingComments", "NoNestedComments"]) {
    requireCondition(
      typeof ts.EmitFlags?.[name] === "number",
      `pinned TypeScript does not expose EmitFlags.${name}`,
    );
  }
  for (const name of [
    "setEmitFlags",
    "setTextRange",
    "addSyntheticLeadingComment",
    "addSyntheticTrailingComment",
    "normalizePath",
    "getScriptKindFromFileName",
  ]) {
    requireCondition(
      typeof ts[name] === "function",
      `pinned TypeScript does not expose ${name}`,
    );
  }
}

function pinScopeGraph() {
  const implementationText = readText(TYPESCRIPT_IMPLEMENTATION);
  const lines = implementationText.split(/\r?\n/u);
  requireCondition(
    !implementationText.includes("\r"),
    "pinned bundle line endings changed",
  );
  const pattern = new RegExp(SCOPE_GRAPH_PATTERN, "u");
  const occurrences = [];
  for (let index = 0; index < lines.length; index += 1) {
    if (pattern.test(lines[index])) {
      occurrences.push(index + 1);
    }
  }
  requireCondition(
    canonical(occurrences) === canonical(SCOPE_GRAPH_OCCURRENCE_LINES),
    `scope-graph occurrence set changed: [${occurrences.join(", ")}]`,
  );
  const spans = SCOPE_GRAPH_SPANS.map((span) => {
    const anchorLines = [];
    for (let index = 0; index < lines.length; index += 1) {
      if (lines[index] === span.anchor) anchorLines.push(index + 1);
    }
    requireCondition(
      anchorLines.length === 1 && anchorLines[0] === span.start_line,
      `${span.span_id} anchor moved: [${anchorLines.join(", ")}]`,
    );
    requireCondition(
      span.end_line >= span.start_line && span.end_line <= lines.length,
      `${span.span_id} span bounds changed`,
    );
    const sliceText = implementationText
      .split(/(?<=\n)/u)
      .slice(span.start_line - 1, span.end_line)
      .join("");
    return {
      span_id: span.span_id,
      role: span.role,
      start_line: span.start_line,
      end_line: span.end_line,
      anchor: span.anchor,
      slice_sha256: sha256(sliceText),
    };
  });
  return {
    source: TYPESCRIPT_IMPLEMENTATION,
    grep_pattern: SCOPE_GRAPH_PATTERN,
    occurrence_lines: SCOPE_GRAPH_OCCURRENCE_LINES.map((line) => ({
      line,
      text: lines[line - 1],
    })),
    spans,
  };
}

function hasDirectory(files, directory) {
  const prefix = `${ts.normalizePath(directory).replace(/\/$/, "")}/`;
  return [...files.keys()].some((fileName) => fileName.startsWith(prefix));
}

function createVirtualProgram(control) {
  const files = new Map(control.files.map((file) => [file.path, file.text]));
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

function isCallStatementOf(statement, calleeName) {
  return (
    ts.isExpressionStatement(statement) &&
    ts.isCallExpression(statement.expression) &&
    ts.isIdentifier(statement.expression.expression) &&
    statement.expression.expression.text === calleeName
  );
}

const TRANSFORMS = {
  identity(_context, sourceFile) {
    return sourceFile;
  },
  "wrap-expression-statements-in-synthetic-arrow"(context, sourceFile) {
    const factory = context.factory;
    let wrapped = 0;
    const statements = sourceFile.statements.map((statement) => {
      if (
        !ts.isExpressionStatement(statement) ||
        !ts.isIdentifier(statement.expression) ||
        statement.expression.text !== "x"
      ) {
        return statement;
      }
      wrapped += 1;
      return factory.createExpressionStatement(
        factory.createArrowFunction(
          undefined,
          undefined,
          [],
          undefined,
          factory.createToken(ts.SyntaxKind.EqualsGreaterThanToken),
          factory.createBlock([statement], false),
        ),
      );
    });
    requireCondition(wrapped === 1, "synthetic wrapper target disappeared");
    return factory.updateSourceFile(sourceFile, statements);
  },
  "apply-comment-emit-flags"(_context, sourceFile) {
    let flagged = 0;
    for (const statement of sourceFile.statements) {
      if (isCallStatementOf(statement, "suppressLead")) {
        ts.setEmitFlags(statement, ts.EmitFlags.NoLeadingComments);
        flagged += 1;
      } else if (isCallStatementOf(statement, "suppressTrail")) {
        ts.setEmitFlags(statement, ts.EmitFlags.NoTrailingComments);
        flagged += 1;
      } else if (ts.isBlock(statement)) {
        ts.setEmitFlags(statement, ts.EmitFlags.NoNestedComments);
        flagged += 1;
      }
    }
    requireCondition(flagged === 3, "emit-flag targets disappeared");
    return sourceFile;
  },
  "add-synthetic-comments-to-marked"(_context, sourceFile) {
    let annotated = 0;
    for (const statement of sourceFile.statements) {
      if (!isCallStatementOf(statement, "markMe")) continue;
      ts.addSyntheticLeadingComment(
        statement,
        ts.SyntaxKind.MultiLineCommentTrivia,
        " SYN-LEAD ",
        false,
      );
      ts.addSyntheticTrailingComment(
        statement,
        ts.SyntaxKind.MultiLineCommentTrivia,
        " SYN-TRAIL ",
        false,
      );
      annotated += 1;
    }
    requireCondition(annotated === 1, "synthetic-comment target disappeared");
    return sourceFile;
  },
  "append-synthetic-statement-with-synthetic-comments"(context, sourceFile) {
    const factory = context.factory;
    const synthetic = factory.createExpressionStatement(
      factory.createCallExpression(
        factory.createIdentifier("syntheticMarker"),
        undefined,
        [],
      ),
    );
    ts.addSyntheticLeadingComment(
      synthetic,
      ts.SyntaxKind.MultiLineCommentTrivia,
      " SYN-LEAD ",
      false,
    );
    ts.addSyntheticTrailingComment(
      synthetic,
      ts.SyntaxKind.MultiLineCommentTrivia,
      " SYN-TRAIL ",
      false,
    );
    return factory.updateSourceFile(sourceFile, [
      ...sourceFile.statements,
      synthetic,
    ]);
  },
  "replace-not-emitted-and-zero-width"(context, sourceFile) {
    const factory = context.factory;
    let dropped = 0;
    let shrunk = 0;
    const statements = sourceFile.statements.map((statement) => {
      if (isCallStatementOf(statement, "dropMe")) {
        dropped += 1;
        return factory.createNotEmittedStatement(statement);
      }
      if (isCallStatementOf(statement, "shrinkMe")) {
        shrunk += 1;
        const replacement = factory.createExpressionStatement(
          factory.createCallExpression(
            factory.createIdentifier("shrunkMarker"),
            undefined,
            [],
          ),
        );
        return ts.setTextRange(replacement, {
          pos: statement.pos,
          end: statement.pos,
        });
      }
      return statement;
    });
    requireCondition(
      dropped === 1 && shrunk === 1,
      "not-emitted/zero-width targets disappeared",
    );
    return factory.updateSourceFile(sourceFile, statements);
  },
};

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

function serializeWrite(arguments_, index) {
  const [fileName, text, writeByteOrderMark, onError, sourceFiles] = arguments_;
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
    materialized_utf8_sha256: sha256(materialized),
    materialized_utf8_bytes: materialized.length,
    on_error_callback_present: typeof onError === "function",
    source_files: (sourceFiles ?? []).map((source) =>
      ts.normalizePath(source.fileName),
    ),
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
    files: [{ path: "/project/input.ts", text: caseSpec.source }],
    roots: ["/project/input.ts"],
    compiler_options: compilerOptions(caseSpec.remove_comments),
  };
}

function observeWitnessCase(caseSpec) {
  const control = witnessControl(caseSpec);
  const transform = TRANSFORMS[caseSpec.transform];
  requireCondition(
    typeof transform === "function",
    `unknown transform ${caseSpec.transform}`,
  );
  const program = createVirtualProgram(control);
  const reportedDiagnostics = ts.getPreEmitDiagnostics(program);
  const writes = [];
  let visited = 0;
  const before = (context) => (sourceFile) => {
    if (!control.roots.includes(sourceFile.fileName)) return sourceFile;
    visited += 1;
    return transform(context, sourceFile);
  };
  const emitResult = program.emit(
    undefined,
    (...arguments_) => writes.push(arguments_),
    undefined,
    false,
    { before: [before] },
  );
  requireCondition(
    visited === 1,
    `${caseSpec.case_id} was visited ${visited} times`,
  );
  requireCondition(
    reportedDiagnostics.length === 0,
    `${caseSpec.case_id} input does not type-check clean`,
  );
  requireCondition(
    emitResult.diagnostics.length === 0 && emitResult.emitSkipped === false,
    `${caseSpec.case_id} emit did not complete clean`,
  );
  requireCondition(
    writes.length === 1,
    `${caseSpec.case_id} produced ${writes.length} writes`,
  );
  const serializedWrites = writes.map(serializeWrite);
  requireCondition(
    serializedWrites[0].path === "/project/input.js",
    `${caseSpec.case_id} wrote ${serializedWrites[0].path}`,
  );
  const output = Buffer.from(
    serializedWrites[0].callback_utf8_base64,
    "base64",
  ).toString("utf8");
  const markerOccurrences = caseSpec.markers.map(({ token }) => ({
    token,
    occurrences: countOccurrences(output, token),
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
  // Comment interval and generated-name caches are printer/process state:
  // every repetition runs in a fresh Node isolate, exactly like the
  // foundation's direct controls.
  const adopted = adoptedObservations?.get(caseSpec.case_id) ?? null;
  let first;
  if (adopted !== null) {
    // Adoption skips only the two fresh-process oracle runs; the stored
    // observation fingerprint stands for the repetitions=2 determinism
    // proof, exactly as the 5g reuse does. Every marker expectation and
    // derivation below still executes against the adopted record.
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
    } else if (markerSpec.expectation === "absent") {
      requireCondition(
        observed.occurrences === 0,
        `${caseSpec.case_id} marker ${markerSpec.token} appears ${observed.occurrences} times in the oracle output`,
      );
    } else {
      requireCondition(
        markerSpec.expectation === "recorded",
        `${caseSpec.case_id} marker ${markerSpec.token} has unknown expectation`,
      );
    }
  }
  const control = witnessControl(caseSpec);
  return withFingerprint(
    {
      case_id: caseSpec.case_id,
      role: caseSpec.role,
      transform: caseSpec.transform,
      remove_comments: caseSpec.remove_comments,
      input: {
        current_directory: "/project",
        roots: control.roots,
        files: control.files.map((file) => ({
          path: file.path,
          root: control.roots.includes(file.path),
          ...bytesRecord(file.text),
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
    for (const [roleKey, roleId] of [
      ["positive", "positive"],
      ["remove_comments", "remove-comments"],
      ["adjacent_negative", "adjacent-negative"],
    ]) {
      const caseSpec = family[roleKey];
      specs.push({
        ...caseSpec,
        case_id: `${family.family_id}--${roleId}`,
        family_id: family.family_id,
      });
    }
  }
  const ids = new Set(specs.map((spec) => spec.case_id));
  requireCondition(ids.size === specs.length, "witness case ids collide");
  for (const spec of specs) {
    const tokens = spec.markers.map((entry) => entry.token);
    requireCondition(
      new Set(tokens).size === tokens.length,
      `${spec.case_id} marker tokens collide`,
    );
    for (const left of tokens) {
      for (const right of tokens) {
        requireCondition(
          left === right || !right.includes(left),
          `${spec.case_id} marker token ${left} is a substring of ${right} - occurrence counts would be polluted`,
        );
      }
    }
  }
  return specs;
}

function buildFamilies(adoptedObservations) {
  const specs = flattenCaseSpecs();
  const casesById = new Map(
    specs.map((spec) => [
      spec.case_id,
      buildWitnessCase(spec, adoptedObservations),
    ]),
  );
  return WITNESS_FAMILY_SPECS.map((family) => {
    const positive = casesById.get(`${family.family_id}--positive`);
    const removeComments = casesById.get(
      `${family.family_id}--remove-comments`,
    );
    const adjacentNegative = casesById.get(
      `${family.family_id}--adjacent-negative`,
    );
    requireCondition(
      positive.observation.writes[0].callback_utf8_sha256 !==
        removeComments.observation.writes[0].callback_utf8_sha256,
      `${family.family_id} positive and remove-comments outputs are identical - the witness is comment-vacuous`,
    );
    return withFingerprint(
      {
        family_id: family.family_id,
        study_item: family.study_item,
        study_anchor: `${SCOPE_STUDY_RELATIVE_PATH}#3-witness-set-to-freeze-step-1-remainder`,
        description: family.description,
        positive_vs_remove_comments_distinct: true,
        cases: [positive, removeComments, adjacentNegative],
      },
      "family_fingerprint_sha256",
    );
  });
}

function validateFoundationLineage() {
  const foundation = readJson(FOUNDATION_RELATIVE_PATH);
  requireCondition(
    foundation.schema === 1 &&
      foundation.kind === "h2-dormant-semantic-foundation" &&
      foundation.status === "frozen-dormant-semantic-foundation" &&
      foundation.phase === SLICE &&
      foundation.slice_id === SLICE &&
      typeof foundation.foundation_fingerprint_sha256 === "string" &&
      foundation.typescript.version === ts.version &&
      foundation.typescript.source_commit === SOURCE_COMMIT &&
      canonical(foundation.typescript.bundle) ===
        canonical(pathHash(TYPESCRIPT_BUNDLE)) &&
      canonical(foundation.typescript.implementation) ===
        canonical(pathHash(TYPESCRIPT_IMPLEMENTATION)),
    "H2.5h-a foundation lineage is not closed",
  );
  return {
    artifact: pathHash(FOUNDATION_RELATIVE_PATH),
    foundation_fingerprint_sha256: foundation.foundation_fingerprint_sha256,
  };
}

function buildArtifact() {
  validateRuntime();
  const scopeGraph = pinScopeGraph();
  const foundation = validateFoundationLineage();
  const typescript = typescriptRecord();
  const families = buildFamilies(reusableStoredObservations(typescript));
  const cases = families.flatMap((family) => family.cases);
  const transformsUsed = [...new Set(cases.map((item) => item.transform))].sort();
  requireCondition(
    canonical(transformsUsed) === canonical([...TRANSFORM_IDS].sort()),
    "witness transform coverage changed",
  );
  requireCondition(
    families.length === 10 && cases.length === 30,
    "witness census changed",
  );
  return withFingerprint(
    {
      schema: 1,
      kind: "h2-comment-scope-witnesses",
      status: "frozen-comment-scope-witnesses",
      phase: SLICE,
      slice_id: SLICE,
      sub_packet: SUB_PACKET,
      plan_step: "step-1-witness-freeze",
      typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      lineage: {
        foundation: foundation.artifact,
        foundation_fingerprint_sha256:
          foundation.foundation_fingerprint_sha256,
        scope_study: pathHash(SCOPE_STUDY_RELATIVE_PATH),
        handoff: pathHash(HANDOFF_RELATIVE_PATH),
        interpretation:
          "E-COMMENT-SCOPE-H step 1 freezes oracle-captured comment-scope bytes; it authorizes no production edit and activates nothing",
      },
      scope_graph: scopeGraph,
      families,
      summary: {
        families: families.length,
        cases: cases.length,
        positive_cases: cases.filter((item) => item.role === "positive").length,
        remove_comments_controls: cases.filter(
          (item) => item.role === "remove-comments-control",
        ).length,
        adjacent_negative_controls: cases.filter(
          (item) => item.role === "adjacent-negative-control",
        ).length,
        typescript_oracle_runs: cases.reduce(
          (sum, item) => sum + item.repetitions,
          0,
        ),
        transforms_used: transformsUsed,
        scope_graph_occurrence_lines: scopeGraph.occurrence_lines.length,
        scope_graph_spans: scopeGraph.spans.length,
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
    "usage: h2-5h-a-comment-scope-witnesses.mjs [--write|--check]",
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
      `stale ${TARGET_RELATIVE_PATH}; run h2-5h-a-comment-scope-witnesses.mjs --write and review`,
    );
    process.stdout.write(
      `H2.5h-a comment-scope witnesses are fresh: families=${artifact.summary.families} cases=${artifact.summary.cases}\n`,
    );
  }
}
