// gate-tax 5 acceptance battery, unit tier (design packet
// docs/design/greenfield/gate-tax-5.md §Acceptance battery items 1/5 and
// the journal/receipt-term mechanics of item 2/4). Registered from
// qualification.test.mjs so every walk tail and gate structural
// preflight runs it. No TypeScript observations happen here.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  PLACEHOLDER,
  PIN_INDEX_RELATIVE_PATH,
  extractPins,
  classifyPins,
  normalizeText,
  normalizedSha256,
  consumerRows,
  pinIndexRowsSha256,
} from "../../crates/oracle/pin-normalize.mjs";

const WORKSPACE = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../..",
);

const FIXTURE = `const rows = [
  ["k1", "ratchets/t.json", "0000000000000000000000000000000000000000000000000000000000000000"],
  ["k2", "vendor/v.js",
    "1111111111111111111111111111111111111111111111111111111111111111"],
];
const map = {
  "ratchets/t.json":
    "2222222222222222222222222222222222222222222222222222222222222222",
  "crates/oracle/o.mjs": "3333333333333333333333333333333333333333333333333333333333333333",
};
const U_RELATIVE_PATH = "ratchets/t.json";
const U_SHA256 =
  "4444444444444444444444444444444444444444444444444444444444444444";
const EXPECTED_U_SHA256 = "5555555555555555555555555555555555555555555555555555555555555555";
const CPATH = ".github/ci/c.json";
const pins = {
  [CPATH]:
    "6666666666666666666666666666666666666666666666666666666666666666",
};
const KEEP_ME = "7777777777777777777777777777777777777777777777777777777777777777";
`;

const FIXTURE_ROWS = {
  pins: [
    { path: ".github/ci/c.json", grammar: "E", count: 1 },
    { path: "crates/oracle/o.mjs", grammar: "C", count: 1 },
    { path: "ratchets/t.json", grammar: "A", count: 1 },
    { path: "ratchets/t.json", grammar: "B", count: 1 },
    { path: "ratchets/t.json", grammar: "D", count: 2 },
    { path: "vendor/v.js", grammar: "A", count: 1 },
  ],
  semantic: [],
};

test("gt5: fixture extraction sees all five grammars and only them", () => {
  const pins = extractPins(FIXTURE);
  assert.equal(pins.length, 7);
  const byGrammar = {};
  for (const pin of pins) byGrammar[pin.grammar] = (byGrammar[pin.grammar] ?? 0) + 1;
  assert.deepEqual(byGrammar, { A: 2, B: 1, C: 1, D: 2, E: 1 });
  // the pathless 64-hex const is not extracted
  assert.ok(!pins.some((pin) => FIXTURE.slice(pin.start, pin.end).startsWith("7777")));
});

test("gt5: normalizer is invariant under pin value swaps (the incident class)", () => {
  const base = normalizedSha256(FIXTURE, FIXTURE_ROWS);
  assert.equal(base.refusals.length, 0);
  let swapped = FIXTURE;
  for (const digit of ["0", "1", "2", "3", "4", "5", "6"]) {
    swapped = swapped.replaceAll(digit.repeat(64), "abc123".repeat(10) + "beef".repeat(1));
  }
  assert.notEqual(swapped, FIXTURE);
  const swappedHash = normalizedSha256(swapped, FIXTURE_ROWS);
  assert.equal(swappedHash.refusals.length, 0);
  assert.equal(swappedHash.sha256, base.sha256);
});

test("gt5: normalizer re-keys on logic changes and pin retargets", () => {
  const base = normalizedSha256(FIXTURE, FIXTURE_ROWS);
  const logic = normalizedSha256(FIXTURE.replace("KEEP_ME", "KEEP_YOU"), FIXTURE_ROWS);
  assert.equal(logic.refusals.length, 0);
  assert.notEqual(logic.sha256, base.sha256);
  // a retarget (path swapped, hash values kept) can never silently HIT:
  // the moved pair is unclassified and the vacated pair is
  // classified-but-absent — a refusal, strictly stronger than a re-key
  const retargeted = FIXTURE.replaceAll("crates/oracle/o.mjs", "crates/oracle/other.mjs");
  const outcome = normalizedSha256(retargeted, FIXTURE_ROWS);
  assert.equal(outcome.sha256, null);
  assert.deepEqual(
    outcome.refusals.map((row) => row.reason).sort(),
    ["classified-but-absent", "unclassified"],
  );
});

test("gt5: unclassified sites, count drift, and mixed classification refuse", () => {
  const unclassified = classifyPins(FIXTURE, {
    pins: FIXTURE_ROWS.pins.filter((row) => row.grammar !== "E"),
    semantic: [],
  });
  assert.deepEqual(
    unclassified.refusals,
    [{ path: ".github/ci/c.json", grammar: "E", reason: "unclassified" }],
  );
  const drifted = classifyPins(FIXTURE, {
    pins: FIXTURE_ROWS.pins.map((row) =>
      row.grammar === "D" ? { ...row, count: 1 } : row,
    ),
    semantic: [],
  });
  assert.equal(drifted.refusals.length, 1);
  assert.match(drifted.refusals[0].reason, /count-drift \(1 classified, 2 present\)/);
  const mixed = classifyPins(FIXTURE, {
    pins: FIXTURE_ROWS.pins,
    semantic: [{ path: "vendor/v.js", grammar: "A" }],
  });
  assert.ok(mixed.refusals.some((row) => row.reason === "both-pin-and-semantic"));
});

test("gt5: semantic exclusions stay unmasked without refusal", () => {
  const rows = {
    pins: FIXTURE_ROWS.pins.filter((row) => row.grammar !== "D"),
    semantic: [{ path: "ratchets/t.json", grammar: "D" }],
  };
  const { normalized, refusals } = normalizeText(FIXTURE, rows);
  assert.equal(refusals.length, 0);
  assert.ok(normalized.includes("4".repeat(64)));
  assert.ok(normalized.includes("5".repeat(64)));
  assert.equal(normalized.split(PLACEHOLDER).length - 1, 5);
});

test("gt5: normalized text holds no residual grammar hits (idempotence)", () => {
  const { normalized, refusals } = normalizeText(FIXTURE, FIXTURE_ROWS);
  assert.equal(refusals.length, 0);
  assert.equal(normalized.split(PLACEHOLDER).length - 1, 7);
  assert.deepEqual(extractPins(normalized), []);
  assert.ok(normalized.includes("7".repeat(64)));
});

test("gt5: python and mjs normalizers agree on every indexed consumer", () => {
  const indexPath = path.join(WORKSPACE, PIN_INDEX_RELATIVE_PATH);
  const index = JSON.parse(fs.readFileSync(indexPath, "utf8"));
  const consumers = Object.keys(index.consumers);
  assert.ok(consumers.length >= 60, `only ${consumers.length} indexed consumers`);
  const python = JSON.parse(
    execFileSync(
      "python3",
      ["scripts/pin-grammar.py", "--normalize-all", indexPath],
      { cwd: WORKSPACE, encoding: "utf8" },
    ),
  );
  for (const consumer of consumers) {
    const text = fs.readFileSync(path.join(WORKSPACE, consumer), "utf8");
    const { sha256, refusals } = normalizedSha256(
      text,
      consumerRows(index, consumer),
    );
    assert.equal(refusals.length, 0, `${consumer}: mjs refusals ${JSON.stringify(refusals)}`);
    assert.equal(python[consumer], sha256, `${consumer}: python/mjs divergence`);
  }
});

test("gt5: the checked-in pin-index is clean", () => {
  const report = execFileSync("python3", ["scripts/pin-index.py", "--check"], {
    cwd: WORKSPACE,
    encoding: "utf8",
  });
  assert.match(report, /pin-index: clean/);
});

test("gt5: pinIndexRowsSha256 keys the masking-relevant rows only", () => {
  const base = pinIndexRowsSha256(FIXTURE_ROWS);
  assert.equal(
    pinIndexRowsSha256({ ...FIXTURE_ROWS, unmatched: [{ literal: "x", note: "y" }] }),
    base,
  );
  assert.notEqual(
    pinIndexRowsSha256({
      pins: FIXTURE_ROWS.pins.slice(1),
      semantic: FIXTURE_ROWS.semantic,
    }),
    base,
  );
});

test("gt5: repin rewrites exactly the classified grammar shapes", () => {
  const workspace = fs.mkdtempSync(path.join(os.tmpdir(), "gt5-repin-"));
  try {
    for (const directory of ["ratchets", "vendor", "crates/oracle", ".github/ci"]) {
      fs.mkdirSync(path.join(workspace, directory), { recursive: true });
    }
    fs.writeFileSync(path.join(workspace, "ratchets/t.json"), "x\n");
    fs.writeFileSync(path.join(workspace, "vendor/v.js"), "v\n");
    fs.writeFileSync(path.join(workspace, "crates/oracle/o.mjs"), "o\n");
    fs.writeFileSync(path.join(workspace, ".github/ci/c.json"), "c\n");
    fs.writeFileSync(path.join(workspace, "script.mjs"), FIXTURE);
    const output = execFileSync(
      "python3",
      [path.join(WORKSPACE, "scripts/chain-walk-repin.py"), "script.mjs"],
      { cwd: workspace, encoding: "utf8" },
    );
    const rewritten = fs.readFileSync(path.join(workspace, "script.mjs"), "utf8");
    // every classified site now carries the current disk hash
    for (const digit of ["0", "1", "2", "3", "4", "5", "6"]) {
      assert.ok(!rewritten.includes(digit.repeat(64)), `stale ${digit} pin left`);
    }
    // the pathless semantic const is untouched
    assert.ok(rewritten.includes("7".repeat(64)));
    assert.equal((output.match(/re-pinned/g) ?? []).length, 7);
    // the rewrite preserves classification shape (all seven spans still
    // extract with the same paths and grammars) …
    const before = extractPins(FIXTURE).map(({ path: p, grammar }) => ({ path: p, grammar }));
    const after = extractPins(rewritten).map(({ path: p, grammar }) => ({ path: p, grammar }));
    assert.deepEqual(after, before);
    // … and repin is normalized-hash-invariant EXCEPT for the known
    // multi-line pattern-A collapse (pre-gt5 repin behavior, preserved
    // byte-identically; 0 such sites exist in the corpus — checked by
    // the walk-preflight index check whenever counts move). The fixture
    // has one multi-line A site (vendor/v.js), so assert invariance on
    // the collapsed form: a SECOND repin is a no-op and normalization is
    // stable across it.
    const secondOutput = execFileSync(
      "python3",
      [path.join(WORKSPACE, "scripts/chain-walk-repin.py"), "script.mjs"],
      { cwd: workspace, encoding: "utf8" },
    );
    assert.match(secondOutput, /no stale pins found/);
    const rewrittenTwice = fs.readFileSync(path.join(workspace, "script.mjs"), "utf8");
    assert.equal(rewrittenTwice, rewritten);
    const normalizedOnce = normalizedSha256(rewritten, FIXTURE_ROWS);
    const normalizedTwice = normalizedSha256(rewrittenTwice, FIXTURE_ROWS);
    assert.equal(normalizedOnce.refusals.length, 0);
    assert.equal(normalizedTwice.sha256, normalizedOnce.sha256);
    // pin-value invariance on layout-stable sites: swap every rewritten
    // hash back to a synthetic value without touching layout
    const currentHashes = new Set(
      extractPins(rewritten).map((pin) => rewritten.slice(pin.start, pin.end)),
    );
    let swapped = rewritten;
    for (const value of currentHashes) {
      swapped = swapped.replaceAll(value, "0123456789abcdef".repeat(4));
    }
    const normalizedSwapped = normalizedSha256(swapped, FIXTURE_ROWS);
    assert.equal(normalizedSwapped.sha256, normalizedOnce.sha256);
  } finally {
    fs.rmSync(workspace, { recursive: true, force: true });
  }
});

test("gt5: repin skips semantic-classified sites when the index says so", () => {
  const workspace = fs.mkdtempSync(path.join(os.tmpdir(), "gt5-repin-sem-"));
  try {
    fs.mkdirSync(path.join(workspace, "ratchets"), { recursive: true });
    fs.mkdirSync(path.join(workspace, "vendor"), { recursive: true });
    fs.mkdirSync(path.join(workspace, ".github/ci"), { recursive: true });
    fs.writeFileSync(path.join(workspace, "ratchets/t.json"), "x\n");
    fs.writeFileSync(path.join(workspace, "vendor/v.js"), "v\n");
    fs.writeFileSync(
      path.join(workspace, "script.mjs"),
      'const U_RELATIVE_PATH = "ratchets/t.json";\n' +
        'const U_SHA256 =\n  "4444444444444444444444444444444444444444444444444444444444444444";\n' +
        'const V_RELATIVE_PATH = "vendor/v.js";\n' +
        'const V_SHA256 = "8888888888888888888888888888888888888888888888888888888888888888";\n',
    );
    fs.writeFileSync(
      path.join(workspace, ".github/ci/pin-index.v1.json"),
      JSON.stringify({
        schema: 1,
        kind: "pin-index",
        consumers: {
          "script.mjs": {
            family: "oracle-script",
            pins: [{ path: "vendor/v.js", grammar: "D", count: 1 }],
            semantic: [{ path: "ratchets/t.json", grammar: "D" }],
            unmatched: [],
          },
        },
      }),
    );
    execFileSync(
      "python3",
      [path.join(WORKSPACE, "scripts/chain-walk-repin.py"), "script.mjs"],
      { cwd: workspace, encoding: "utf8" },
    );
    const rewritten = fs.readFileSync(path.join(workspace, "script.mjs"), "utf8");
    assert.ok(rewritten.includes("4".repeat(64)), "semantic const was rewritten");
    assert.ok(!rewritten.includes("8".repeat(64)), "classified pin was not refreshed");
  } finally {
    fs.rmSync(workspace, { recursive: true, force: true });
  }
});
