import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import {
  M9RequestError,
  compilerOptionsFromOrderedSettings,
  createMapOnlyHost,
  decodeCanonicalUtf8Base64,
  decodeInlineProgram,
} from "./m9-program-host.mjs";
import {
  M9ObservationError,
  MAX_MESSAGE_CHAIN_DEPTH,
  MAX_MESSAGE_CHAIN_NODES,
  buildRendererObservation,
  collectPassOccurrences,
  formatRawDiagnostic,
  serializeDiagnosticOccurrence,
  serializeMessageChainBounded,
  snapshotCanonicalHead,
} from "./m9-observation.mjs";
import {
  rejectedResult,
  terminalResult,
} from "./m9-driver.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DRIVER = path.join(HERE, "m9-driver.mjs");
const CASE_SHA256 = "a".repeat(64);
const NODE_VERSION_PIN = fs
  .readFileSync(path.join(HERE, "../../.node-version"), "utf8")
  .trim();

function base64(text) {
  return Buffer.from(text, "utf8").toString("base64");
}

function file(ordinal, name, text) {
  return {
    ordinal,
    name,
    text_base64: base64(text),
  };
}

function program(source = "let = ;\n") {
  return {
    cwd: "/work",
    options: [],
    libs: [],
    files: [file(0, "main.ts", source)],
  };
}

function request(programValue = program()) {
  return {
    schema: 1,
    id: "1",
    op: "execute-case",
    case_sha256: CASE_SHA256,
    program: programValue,
  };
}

function globalDiagnostic(
  code,
  messageText,
  category = ts.DiagnosticCategory.Error,
) {
  return { code, category, messageText };
}

function runDriver(value) {
  return runDriverRaw(`${JSON.stringify(value)}\n`);
}

function runDriverRaw(input) {
  return spawnSync(
    process.execPath,
    ["--single-threaded", DRIVER],
    {
      input,
      encoding: "utf8",
    },
  );
}

function parsedFrames(output) {
  return output
    .split("\n")
    .filter((line) => line.length > 0)
    .map((line) => JSON.parse(line));
}

function requestFrames(output) {
  const frames = parsedFrames(output);
  assert.deepEqual(Object.keys(frames[0]), [
    "schema",
    "frame",
    "implementation",
    "version",
  ]);
  assert.equal(frames[0].schema, 1);
  assert.equal(frames[0].frame, "hello");
  assert.equal(frames[0].implementation, "oracle-node");
  assert.equal(frames[0].version, `v${NODE_VERSION_PIN}`);
  return frames.slice(1);
}

test("canonical base64 is strict and fatal UTF-8 preserves BOM", () => {
  assert.equal(decodeCanonicalUtf8Base64("", "empty"), "");
  assert.equal(
    decodeCanonicalUtf8Base64("77u/YQ==", "bom"),
    "\ufeffa",
  );
  for (const invalid of ["YQ", "YQ==\n", "YR==", "YQ===", "YQ-_"]) {
    assert.throws(
      () => decodeCanonicalUtf8Base64(invalid, "invalid"),
      M9RequestError,
    );
  }
  assert.throws(
    () => decodeCanonicalUtf8Base64("/w==", "invalid-utf8"),
    /not UTF-8/u,
  );
});

test("ordered compiler options reject unknown, wrong, and folded duplicates", () => {
  const options = compilerOptionsFromOrderedSettings([
    {
      ordinal: 0,
      name: "strict",
      value: { kind: "boolean", value: true },
    },
    {
      ordinal: 1,
      name: "target",
      value: { kind: "text", value: "es2022" },
    },
  ]);
  assert.equal(options.strict, true);
  assert.equal(options.target, ts.ScriptTarget.ES2022);
  assert.equal(options.noLib, true);
  assert.equal(
    compilerOptionsFromOrderedSettings([
      {
        ordinal: 0,
        name: "target",
        value: { kind: "number", value: 9 },
      },
    ]).target,
    ts.ScriptTarget.ES2022,
  );
  assert.deepEqual(
    compilerOptionsFromOrderedSettings([
      {
        ordinal: 0,
        name: "lib",
        value: { kind: "text", value: " ES5, DOM " },
      },
    ]).lib,
    ["es5", "dom"],
  );
  const absent = compilerOptionsFromOrderedSettings([
    {
      ordinal: 0,
      name: "strict",
      value: { kind: "null" },
    },
  ]);
  assert.equal(Object.hasOwn(absent, "strict"), false);

  assert.throws(
    () =>
      compilerOptionsFromOrderedSettings([
        {
          ordinal: 0,
          name: "notAnOption",
          value: { kind: "boolean", value: true },
        },
      ]),
    /unsupported compiler option/u,
  );
  assert.throws(
    () =>
      compilerOptionsFromOrderedSettings([
        {
          ordinal: 0,
          name: "declaration",
          value: { kind: "boolean", value: true },
        },
      ]),
    /unsupported compiler option/u,
  );
  assert.throws(
    () =>
      compilerOptionsFromOrderedSettings([
        {
          ordinal: 0,
          name: "strict",
          value: { kind: "text", value: "true" },
        },
      ]),
    /must be boolean/u,
  );
  assert.throws(
    () =>
      compilerOptionsFromOrderedSettings([
        {
          ordinal: 0,
          name: "strict",
          value: { kind: "boolean", value: true },
        },
        {
          ordinal: 1,
          name: "Strict",
          value: { kind: "boolean", value: true },
        },
      ]),
    /case-insensitive duplicate/u,
  );
  assert.throws(
    () =>
      compilerOptionsFromOrderedSettings([
        {
          ordinal: 0,
          name: "target",
          value: { kind: "number", value: 13 },
        },
      ]),
    /not a valid numeric/u,
  );
  assert.throws(
    () =>
      compilerOptionsFromOrderedSettings([
        {
          ordinal: 0,
          name: "target",
          value: { kind: "text", value: "es2099" },
        },
      ]),
    /not a valid/u,
  );
  assert.throws(
    () =>
      compilerOptionsFromOrderedSettings([
        {
          ordinal: 0,
          name: "target",
          value: { kind: "text", value: "es2022" },
        },
        {
          ordinal: 1,
          name: "strict",
          value: { kind: "boolean", value: true },
        },
      ]),
    /strictly sorted/u,
  );
  assert.throws(
    () =>
      compilerOptionsFromOrderedSettings([
        {
          ordinal: 0,
          name: "baseUrl",
          value: { kind: "text", value: "bad\u0000path" },
        },
      ]),
    /control characters/u,
  );
  assert.throws(
    () =>
      compilerOptionsFromOrderedSettings([
        {
          ordinal: 0,
          name: "noLib",
          value: { kind: "boolean", value: false },
        },
      ]),
    /must be true/u,
  );
});

test("inline host uses only exact inline libs and files", () => {
  const decoded = decodeInlineProgram({
    cwd: "/work",
    options: [],
    libs: [file(0, "lib.d.ts", "declare const inlineOnly: 1;\n")],
    files: [file(0, "main.ts", "inlineOnly;\n")],
  });
  const host = createMapOnlyHost(decoded);
  assert.equal(host.readFile("/work/lib.d.ts"), "declare const inlineOnly: 1;\n");
  assert.equal(host.fileExists("/etc/passwd"), false);
  assert.equal(host.readFile("/etc/passwd"), undefined);

  assert.throws(
    () =>
      decodeInlineProgram({
        cwd: "/work",
        options: [],
        libs: [file(0, "main.ts", "")],
        files: [file(0, "/work/main.ts", "")],
      }),
    /duplicate resolved file name/u,
  );
});

test("public getter seam preserves file-pass occurrences and never mutates diagnostics", () => {
  const shared = globalDiagnostic(2000, "shared");
  shared.canonicalHead = { code: 1999, messageText: "canonical" };
  const suggestionHigh = globalDiagnostic(
    9000,
    "suggestion-high",
    ts.DiagnosticCategory.Suggestion,
  );
  const suggestionLow = globalDiagnostic(
    100,
    "suggestion-low",
    ts.DiagnosticCategory.Suggestion,
  );
  const calls = [];
  const fakeSource = {};
  const state = {
    files: [{ publicName: "main.ts", resolvedName: "/work/main.ts" }],
    publicNames: new Map([["/work/main.ts", "main.ts"]]),
    program: {
      getSourceFile(name) {
        calls.push(`source:${name}`);
        return fakeSource;
      },
      getSyntacticDiagnostics(source) {
        assert.equal(source, fakeSource);
        calls.push("syntactic");
        return [];
      },
      getSemanticDiagnostics(source) {
        assert.equal(source, fakeSource);
        calls.push("semantic");
        return [shared];
      },
      getSuggestionDiagnostics(source) {
        assert.equal(source, fakeSource);
        calls.push("suggestion");
        // Deliberately not code-sorted: the public suggestion getter return
        // must not receive an adapter-owned sort.
        return [suggestionHigh, shared, suggestionLow];
      },
    },
  };

  const occurrences = collectPassOccurrences(state);
  assert.deepEqual(calls, [
    "source:/work/main.ts",
    "syntactic",
    "semantic",
    "suggestion",
  ]);
  assert.deepEqual(
    occurrences.map(({ assembled }) => [
      assembled.diagnostic.pass,
      assembled.diagnostic.code,
    ]),
    [
      ["semantic", 2000],
      ["suggestion", 9000],
      ["suggestion", 2000],
      ["suggestion", 100],
    ],
  );
  assert.equal(Object.hasOwn(shared, "tsrsPass"), false);
  assert.deepEqual(occurrences[0].assembled.canonical_head, {
    kind: "present",
    code: 1999,
    message_text: "canonical",
  });

  const renderer = buildRendererObservation(occurrences, "/work");
  assert.equal(renderer.deduped_indices.includes(0), true);
  assert.equal(renderer.deduped_indices.includes(2), false);
  assert.equal(
    renderer.aggregate_text,
    renderer.segments.map(({ raw_text }) => raw_text).join(""),
  );
  renderer.segments.forEach((segment, index) => {
    assert.equal(segment.assembled_index, renderer.deduped_indices[index]);
  });
});

test("host-only or extension-filtered inputs have no public diagnostic getter", () => {
  const occurrences = collectPassOccurrences({
    program: {
      getSourceFile() {
        return undefined;
      },
    },
    files: [
      {
        publicName: "package.json",
        resolvedName: "/package.json",
      },
    ],
    publicNames: new Map(),
  });
  assert.deepEqual(occurrences, []);
});

test("global sort/dedupe joins returned object identity to the first assembled occurrence", () => {
  const first = globalDiagnostic(1234, "same");
  const equalButDistinct = globalDiagnostic(1234, "same");
  const publicNames = new Map();
  const occurrence = (diagnosticObject, pass) => ({
    diagnosticObject,
    assembled: {
      diagnostic: serializeDiagnosticOccurrence(
        diagnosticObject,
        pass,
        publicNames,
      ),
      canonical_head: snapshotCanonicalHead(diagnosticObject),
    },
  });
  const renderer = buildRendererObservation(
    [
      occurrence(first, "semantic"),
      occurrence(equalButDistinct, "semantic"),
      occurrence(first, "suggestion"),
    ],
    "/",
  );
  assert.deepEqual(renderer.deduped_indices, [0]);
});

function chainWithDepth(depth) {
  const root = {
    messageText: "0",
    code: 1,
    category: ts.DiagnosticCategory.Error,
    next: [],
  };
  let current = root;
  for (let index = 1; index < depth; index += 1) {
    const child = {
      messageText: String(index),
      code: 1,
      category: ts.DiagnosticCategory.Error,
      next: [],
    };
    current.next = [child];
    current = child;
  }
  return root;
}

function wideChain(nodes) {
  return {
    messageText: "root",
    code: 1,
    category: ts.DiagnosticCategory.Error,
    next: Array.from({ length: nodes - 1 }, (_, index) => ({
      messageText: String(index),
      code: 1,
      category: ts.DiagnosticCategory.Error,
    })),
  };
}

test("message chains enforce iterative depth, node, cycle, and next presence", () => {
  assert.doesNotThrow(() =>
    serializeMessageChainBounded(
      chainWithDepth(MAX_MESSAGE_CHAIN_DEPTH),
      1,
      ts.DiagnosticCategory.Error,
    ),
  );
  assert.throws(
    () =>
      serializeMessageChainBounded(
        chainWithDepth(MAX_MESSAGE_CHAIN_DEPTH + 1),
        1,
        ts.DiagnosticCategory.Error,
      ),
    /depth 32/u,
  );
  assert.doesNotThrow(() =>
    serializeMessageChainBounded(
      wideChain(MAX_MESSAGE_CHAIN_NODES),
      1,
      ts.DiagnosticCategory.Error,
    ),
  );
  assert.throws(
    () =>
      serializeMessageChainBounded(
        wideChain(MAX_MESSAGE_CHAIN_NODES + 1),
        1,
        ts.DiagnosticCategory.Error,
      ),
    /node count 4096/u,
  );

  const cyclic = chainWithDepth(2);
  cyclic.next[0].next = [cyclic];
  assert.throws(
    () =>
      serializeMessageChainBounded(
        cyclic,
        1,
        ts.DiagnosticCategory.Error,
      ),
    /active-ancestor cycle/u,
  );
  assert.equal(
    serializeMessageChainBounded(
      globalDiagnostic(1, "unused").messageText,
      1,
      ts.DiagnosticCategory.Error,
    ).next_present,
    false,
  );
  assert.equal(
    serializeMessageChainBounded(
      chainWithDepth(1),
      1,
      ts.DiagnosticCategory.Error,
    ).next_present,
    true,
  );

  const shared = {
    messageText: "shared",
    code: 1,
    category: ts.DiagnosticCategory.Error,
  };
  const dag = {
    messageText: "root",
    code: 1,
    category: ts.DiagnosticCategory.Error,
    next: [
      {
        messageText: "left",
        code: 1,
        category: ts.DiagnosticCategory.Error,
        next: [shared],
      },
      {
        messageText: "right",
        code: 1,
        category: ts.DiagnosticCategory.Error,
        next: [shared],
      },
    ],
  };
  const serializedDag = serializeMessageChainBounded(
    dag,
    1,
    ts.DiagnosticCategory.Error,
  );
  assert.equal(serializedDag.next[0].next[0].text, "shared");
  assert.equal(serializedDag.next[1].next[0].text, "shared");
});

test("unknown diagnostic path/category are rejected and raw formatter keeps CRLF", () => {
  assert.throws(
    () =>
      serializeDiagnosticOccurrence(
        globalDiagnostic(1, "bad", 99),
        "semantic",
        new Map(),
      ),
    M9ObservationError,
  );
  const unknownFile = {
    fileName: "/outside.ts",
    getLineAndCharacterOfPosition() {
      return { line: 0, character: 0 };
    },
  };
  assert.throws(
    () =>
      serializeDiagnosticOccurrence(
        {
          file: unknownFile,
          start: 0,
          length: 1,
          code: 1,
          category: ts.DiagnosticCategory.Error,
          messageText: "bad",
        },
        "semantic",
        new Map(),
      ),
    /outside the inline program/u,
  );

  const rendered = formatRawDiagnostic(
    globalDiagnostic(1, "left\r\nright"),
    "/",
  );
  assert.equal(rendered.includes("left\r\nright"), true);
  assert.equal(rendered.includes("\u001b["), false);
});

test("JSONL driver emits bound ordered phases and index-joined completed result", () => {
  const output = runDriver(request());
  assert.equal(output.status, 0, output.stderr);
  assert.equal(output.stderr, "");
  for (const line of output.stdout.split("\n").filter(Boolean)) {
    assert.equal(line, JSON.stringify(JSON.parse(line)));
  }
  const frames = requestFrames(output.stdout);
  assert.deepEqual(
    frames.map((entry) =>
      entry.frame === "phase"
        ? `phase:${entry.phase}`
        : `result:${entry.result.status}`,
    ),
    [
      "phase:parse",
      "phase:bind",
      "phase:check",
      "phase:format",
      "result:completed",
    ],
  );
  for (const entry of frames) {
    assert.equal(entry.schema, 1);
    assert.equal(entry.id, "1");
    assert.equal(entry.case_sha256, CASE_SHA256);
  }
  const result = frames.at(-1).result;
  assert.equal(result.assembled.length > 0, true);
  assert.equal(result.deduped_indices.length, result.segments.length);
  result.segments.forEach((segment, index) => {
    assert.equal(segment.assembled_index, result.deduped_indices[index]);
    assert.ok(result.assembled[segment.assembled_index]);
  });
  assert.equal(
    result.aggregate_text,
    result.segments.map(({ raw_text }) => raw_text).join(""),
  );
});

test("terminal and rejected result variants have closed model-compatible shapes", () => {
  const expected = [
    ["parse", "parser-invariant"],
    ["bind", "phase-invariant"],
    ["check", "phase-invariant"],
    ["format", "renderer-invariant"],
  ];
  for (const [phase, boundary] of expected) {
    const result = terminalResult(phase, new Error("boom"));
    assert.deepEqual(Object.keys(result), [
      "status",
      "phase",
      "kind",
      "boundary_id",
      "detail",
    ]);
    assert.equal(result.status, "terminal");
    assert.equal(result.kind, "panic");
    assert.equal(result.phase, phase);
    assert.equal(result.boundary_id, boundary);
    assert.match(result.detail, /boom/u);
  }
  const rejected = rejectedResult(
    "malformed-observation",
    new Error("bad"),
  );
  assert.deepEqual(Object.keys(rejected), [
    "status",
    "kind",
    "detail",
  ]);
  assert.deepEqual(
    [rejected.status, rejected.kind],
    ["rejected", "malformed-observation"],
  );
});

test("inline program preserves Node16 implied format through the map-only host", () => {
  const node16 = {
    cwd: "/",
    options: [
      {
        ordinal: 0,
        name: "module",
        value: { kind: "text", value: "node16" },
      },
      {
        ordinal: 1,
        name: "moduleResolution",
        value: { kind: "text", value: "node16" },
      },
      {
        ordinal: 2,
        name: "target",
        value: { kind: "text", value: "es2022" },
      },
    ],
    libs: [],
    files: [
      file(0, "/foo.ts", "export const x = 1;\n"),
      file(1, "/main.mts", 'import { x } from "./foo";\nx;\n'),
    ],
  };
  const output = runDriver(request(node16));
  assert.equal(output.status, 0, output.stderr);
  const result = requestFrames(output.stdout).at(-1).result;
  assert.equal(result.status, "completed");
  assert.equal(
    result.assembled.some(
      ({ diagnostic }) =>
        diagnostic.pass === "semantic" && diagnostic.code === 2835,
    ),
    true,
  );
});

test("bound malformed requests are rejected and unbound requests terminate the session", () => {
  const invalidProgram = program();
  invalidProgram.files[0].text_base64 = "YR==";
  const bound = runDriver(request(invalidProgram));
  assert.equal(bound.status, 0, bound.stderr);
  const boundFrames = requestFrames(bound.stdout);
  assert.equal(boundFrames.length, 1);
  assert.equal(boundFrames[0].frame, "result");
  assert.equal(boundFrames[0].result.status, "rejected");
  assert.equal(boundFrames[0].result.kind, "malformed-request");

  const unbound = request();
  unbound.id = 1;
  const failed = runDriver(unbound);
  assert.equal(failed.status, 2);
  assert.equal(requestFrames(failed.stdout).length, 0);
  assert.match(failed.stderr, /cannot be bound/u);
});

test("wire input requires canonical compact JSON, exact key order, and no unknown fields", () => {
  const spaced = JSON.stringify(request()).replace(
    '"schema":1',
    '"schema": 1',
  );
  const pretty = runDriverRaw(`${spaced}\n`);
  assert.equal(pretty.status, 0, pretty.stderr);
  assert.equal(
    requestFrames(pretty.stdout)[0].result.kind,
    "malformed-request",
  );

  const reordered = {
    id: "1",
    schema: 1,
    op: "execute-case",
    case_sha256: CASE_SHA256,
    program: program(),
  };
  const reorderedOutput = runDriver(reordered);
  assert.equal(reorderedOutput.status, 0, reorderedOutput.stderr);
  assert.equal(
    requestFrames(reorderedOutput.stdout)[0].result.kind,
    "malformed-request",
  );

  const unknown = request();
  unknown.extra = true;
  const unknownOutput = runDriver(unknown);
  assert.equal(unknownOutput.status, 0, unknownOutput.stderr);
  assert.equal(
    requestFrames(unknownOutput.stdout)[0].result.kind,
    "malformed-request",
  );

  const nestedUnknown = request();
  nestedUnknown.program.files[0].extra = true;
  const nestedOutput = runDriver(nestedUnknown);
  assert.equal(nestedOutput.status, 0, nestedOutput.stderr);
  assert.equal(
    requestFrames(nestedOutput.stdout)[0].result.kind,
    "malformed-request",
  );
});
