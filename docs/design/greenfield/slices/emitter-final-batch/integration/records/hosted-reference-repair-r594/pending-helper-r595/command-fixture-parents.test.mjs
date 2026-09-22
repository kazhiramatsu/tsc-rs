import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { SPECS, SELECTION, PARAMETER_IDS, ef7Inputs, inspect, refresh, sha } from "./command-fixture-parents.mjs";

const G = "ratchets/h2-5g-qualification.v1.json";
const H = "ratchets/h2-5h-qualification.v1.json";
const B = "ratchets/h2-7b-qualification.v1.json";
const D = "ratchets/h2-candidate-dispositions.v1.json";
const O = "ratchets/h2-8a-candidate-inputs.v1.json";
const PROFILE = "ratchets/h2-5g-profile.v1.json";
const TS = "vendor/typescript-6.0.3/lib/typescript.js";
const OWNED = [...SPECS.map(spec => spec.file), SELECTION];
const json = value => JSON.stringify(value, null, 2) + "\n";

function environment(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "command-parent-controls-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const put = (name, bytes) => {
    fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
    fs.writeFileSync(path.join(root, name), bytes);
  };
  const read = name => fs.readFileSync(path.join(root, name));
  const writeJson = (name, value) => {
    const bytes = Buffer.from(json(value));
    if (name.endsWith(".zst")) {
      const result = spawnSync("zstd", ["-q", "--stdout"], { input: bytes, maxBuffer: 8 * 1024 * 1024 });
      assert.equal(result.status, 0);
      put(name, result.stdout);
    } else put(name, bytes);
  };
  for (const name of [...ef7Inputs("217"), ...ef7Inputs("plan-base"), ...SPECS.map(spec => spec.observer)]) put(name, "unchanged input\n");
  put(".node-version", process.version.slice(1) + "\n");
  const rows = PARAMETER_IDS.map(case_id => ({ case_id, execution_route: "qualified-vfs",
    input: { text: "unicode: \ud800 and 𐐀", settings: [] }, typescript_observation: { diagnostics: [], result: true } }));
  for (const [name, cases] of [[G, rows.slice(4)], [H, rows.slice(0, 4)], [B, []], [D, []], [O, []]]) writeJson(name, { metadata: 0, cases });
  writeJson(PROFILE, { runtime_inputs: SPECS.map(spec => ({ path: spec.file, sha256: "a".repeat(64) })) });
  const selection = { schema: "h2-5h-parameter-temporaries-preparation/v1", historical: "retain this",
    files: { [G]: sha(read(G)), [H]: sha(read(H)), [TS]: sha(read(TS)), "old/source.rs": "b".repeat(64) },
    cases: rows.map((row, index) => ({ case_id: row.case_id, artifact: index < 4 ? H : G,
      role: index < 4 ? "repair-candidate" : "frozen-adjacent-control", sha256_json_stringify: sha(JSON.stringify(row)) })) };
  writeJson(SELECTION, selection);
  const originals = new Map();
  for (const spec of SPECS) {
    const document = { version: 1, typescript: "6.0.3", repetitions: 2, source_commit: "preserved",
      observer_sha256: sha(read(spec.observer)), compiler_sha256: sha(read(TS)),
      cases: Array.from({ length: spec.count }, (_, index) => ({ case_id: `${spec.name}-${index}`, text: "kept", value: true })) };
    if (spec.name.startsWith("ef7-")) {
      delete document.version;
      delete document.observer_sha256;
      delete document.compiler_sha256;
      Object.assign(document, { schema: 1, kind: "emitter-final-universe", id_set: spec.args[1],
        generator: { path: spec.observer, sha256: sha(read(spec.observer)) },
        inputs: ef7Inputs(spec.args[1]).map(name => ({ path: name, sha256: sha(read(name)) })) });
    } else if (spec.name === "jsdoc") document.selection = [{ universe: B, artifact_sha256: sha(read(B)) }];
    else if (spec.name === "parameters") Object.assign(document, { selection_sha256: sha(read(SELECTION)),
      parents: [{ path: H, sha256: sha(read(H)) }, { path: G, sha256: sha(read(G)) }] });
    else document.parent = { path: H, sha256: sha(read(H)) };
    originals.set(spec.name, document);
    writeJson(spec.file, document);
  }
  const before = () => new Map(OWNED.map(name => [name, { bytes: read(name), mtime: fs.statSync(path.join(root, name), { bigint: true }).mtimeNs }]));
  const unchanged = snapshot => {
    for (const [name, saved] of snapshot) {
      assert.deepStrictEqual(read(name), saved.bytes, name);
      assert.equal(fs.statSync(path.join(root, name), { bigint: true }).mtimeNs, saved.mtime, name);
    }
  };
  const move = (names = [G, H, B, D, O]) => {
    for (const name of names) {
      const value = JSON.parse(read(name));
      value.metadata += 1;
      writeJson(name, value);
    }
  };
  const calls = [];
  const writer = (cwd, spec, log) => {
    assert.equal(cwd, root);
    if (spec.exclusive) assert.ok(!fs.existsSync(path.join(root, spec.file)), "wx writer needs a new destination");
    const value = structuredClone(originals.get(spec.name));
    if (spec.name.startsWith("ef7-")) {
      value.inputs[1].sha256 = sha(read(D));
      value.inputs[2].sha256 = sha(read(O));
    } else if (spec.name === "jsdoc") value.selection[0].artifact_sha256 = sha(read(B));
    else if (spec.name === "parameters") {
      value.parents[0].sha256 = sha(read(H));
      value.parents[1].sha256 = sha(read(G));
      value.selection_sha256 = sha(read(SELECTION));
    } else value.parent.sha256 = sha(read(H));
    writeJson(spec.file, value);
    fs.writeFileSync(log, "injected writer, no TypeScript execution\n");
    calls.push(spec.name);
    return { status: 0 };
  };
  return { root, put, read, writeJson, move, calls, writer, before, unchanged, originals,
    recordDir: path.join(root, "target", "unit-evidence") };
}

test("current metadata is read-only and refresh is a true no-op", t => {
  const e = environment(t), before = e.before();
  assert.deepEqual(inspect(e.root).stale, []);
  assert.equal(refresh(e).changed, false);
  assert.deepEqual(e.calls, []);
  assert.ok(!fs.existsSync(e.recordDir));
  e.unchanged(before);
});

test("all five original-command slots refresh only parent metadata", t => {
  const e = environment(t);
  e.move();
  assert.equal(inspect(e.root).stale.length, 5);
  const result = refresh(e);
  assert.equal(result.changed, true);
  assert.deepEqual(e.calls, SPECS.map(spec => spec.name));
  assert.deepEqual(inspect(e.root).stale, []);
  const selection = JSON.parse(e.read(SELECTION));
  assert.equal(selection.historical, "retain this");
  assert.equal(selection.files["old/source.rs"], "b".repeat(64));
  const before = e.before();
  assert.equal(refresh(e).changed, false);
  e.unchanged(before);
});

test("a later 7b-only change runs only the JSDoc writer", t => {
  const e = environment(t), before = e.before();
  e.move([B]);
  assert.deepEqual(inspect(e.root).stale, ["jsdoc"]);
  refresh(e);
  assert.deepEqual(e.calls, ["jsdoc"]);
  before.delete(SPECS.find(spec => spec.name === "jsdoc").file);
  e.unchanged(before);
});

test("row hash or membership drift refuses before any writer", t => {
  const e = environment(t), before = e.before();
  const parent = JSON.parse(e.read(H));
  parent.cases[0].input.text = "changed input";
  e.writeJson(H, parent);
  assert.throws(() => refresh(e), /selected complete row changed/u);
  assert.deepEqual(e.calls, []);
  e.unchanged(before);
});

test("unsupported parent paths and profile membership refuse", t => {
  const e = environment(t);
  const profile = JSON.parse(e.read(PROFILE));
  profile.runtime_inputs[0].path = "crates/compiler/tests/fixtures/emitter-heritage-boundaries.json";
  e.writeJson(PROFILE, profile);
  assert.throws(() => inspect(e.root), /exactly one input/u);
  assert.equal(SPECS.length, 5);
  assert.ok(SPECS.every(spec => !spec.file.includes("heritage")));
});

for (const [name, mutation] of [
  ["boolean-to-number", value => { value.cases[0].value = 1; }],
  ["output string", value => { value.cases[0].text = "different output"; }],
  ["row order", value => { [value.cases[0], value.cases[1]] = [value.cases[1], value.cases[0]]; }],
]) {
  test(`${name} mutation restores all six files and mtimes`, t => {
    const e = environment(t), before = e.before();
    e.move();
    const writer = (root, spec, log) => {
      const result = e.writer(root, spec, log);
      if (spec.name === "utf16") {
        const value = JSON.parse(e.read(spec.file));
        mutation(value);
        e.writeJson(spec.file, value);
      }
      return result;
    };
    assert.throws(() => refresh({ ...e, writer }), /changed a non-parent field/u);
    e.unchanged(before);
    assert.equal(JSON.parse(fs.readFileSync(path.join(e.recordDir, "result.json"))).restored, true);
  });
}

test("a changed source_commit is not a permitted metadata refresh", t => {
  const e = environment(t), before = e.before();
  e.move([D]);
  const writer = (root, spec, log) => {
    const result = e.writer(root, spec, log);
    const value = JSON.parse(e.read(spec.file));
    value.source_commit = "different";
    e.writeJson(spec.file, value);
    return result;
  };
  assert.throws(() => refresh({ ...e, writer }), /changed a non-parent field/u);
  e.unchanged(before);
});

test("writer failure after earlier successful writers restores the whole set", t => {
  const e = environment(t), before = e.before();
  e.move();
  const writer = (root, spec, log) => {
    const result = e.writer(root, spec, log);
    return spec.name === "utf16" ? { status: 7 } : result;
  };
  assert.throws(() => refresh({ ...e, writer }), /writer exited unsuccessfully/u);
  assert.equal(e.calls.length, 5);
  e.unchanged(before);
});

test("a thrown writer restores an absent wx destination", t => {
  const e = environment(t), before = e.before();
  e.move();
  const writer = (root, spec, log) => {
    if (spec.name === "utf16") throw new Error("injected I/O failure");
    return e.writer(root, spec, log);
  };
  assert.throws(() => refresh({ ...e, writer }), /injected I\/O failure/u);
  e.unchanged(before);
});

test("a parent moving during observation refuses and restores only owned outputs", t => {
  const e = environment(t), before = e.before();
  e.move();
  const parentBefore = e.read(G);
  const writer = (root, spec, log) => {
    const result = e.writer(root, spec, log);
    if (spec.name === "utf16") e.move([G]);
    return result;
  };
  assert.throws(() => refresh({ ...e, writer }), /input changed during refresh/u);
  e.unchanged(before);
  assert.notDeepEqual(e.read(G), parentBefore, "helper must not overwrite an externally changed parent");
});

test("selection changes outside the two hash leaves refuse", t => {
  const e = environment(t), before = e.before();
  e.move();
  const writer = (root, spec, log) => {
    const result = e.writer(root, spec, log);
    if (spec.name === "utf16") {
      const selection = JSON.parse(e.read(SELECTION));
      selection.historical = "rewritten";
      e.writeJson(SELECTION, selection);
    }
    return result;
  };
  assert.throws(() => refresh({ ...e, writer }), /writer changed the selection document/u);
  e.unchanged(before);
});

test("an unexplained pre-existing selection mismatch refuses", t => {
  const e = environment(t);
  const selection = JSON.parse(e.read(SELECTION));
  selection.historical = "unexpected";
  e.writeJson(SELECTION, selection);
  assert.throws(() => inspect(e.root), /selection changed outside the parent refresh/u);
  assert.deepEqual(e.calls, []);
});

test("a later writer cannot alter an earlier validated fixture", t => {
  const e = environment(t), before = e.before();
  e.move();
  const writer = (root, spec, log) => {
    const result = e.writer(root, spec, log);
    if (spec.name === "utf16") {
      const name = SPECS[0].file, value = JSON.parse(e.read(name));
      value.cases[0].text = "changed after its validation";
      e.writeJson(name, value);
    }
    return result;
  };
  assert.throws(() => refresh({ ...e, writer }), /previously validated output/u);
  e.unchanged(before);
});

test("the CLI cannot add a sixth fixture to the closed roster", () => {
  const result = spawnSync(process.execPath,
    [new URL("./command-fixture-parents.mjs", import.meta.url).pathname, "--check", "--fixture", "emitter-heritage-boundaries.json"],
    { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /use --check or --fix/u);
});

test("an unexpected parent identity refuses before reading that path", t => {
  const e = environment(t), spec = SPECS.find(candidate => candidate.name === "utf16");
  const value = JSON.parse(e.read(spec.file));
  value.parent.path = "unlisted-parent.json";
  e.writeJson(spec.file, value);
  assert.throws(() => inspect(e.root), /parent identity changed/u);
});

test("the actual hook follows every producer and forces an ordinary next pass", t => {
  const e = environment(t);
  const source = fs.readFileSync(new URL("./chain-walk.sh", import.meta.url), "utf8");
  const begin = source.indexOf("  # command-fixture-parents hook begin\n");
  const end = source.indexOf("  # command-fixture-parents hook end\n", begin);
  const clean = source.indexOf("  if [ ${#stale[@]} -eq 0 ]; then", end);
  const cleanEnd = source.indexOf("\n  fi", clean) + "\n  fi".length;
  assert.ok(begin > source.indexOf('for name in "${ORDER[@]}"; do'));
  assert.ok(begin > source.lastIndexOf("  done\n", begin), "hook is outside the ORDER for-loop");
  assert.match(source.slice(source.lastIndexOf("  done\n", begin), begin),
    /^  done\n(?:  #[^\n]*\n)+$/u, "only the hook's comments separate it from the completed ORDER loop");
  assert.ok(begin >= 0 && end > begin && clean > end);
  const order = source.match(/^ORDER=\(\n([\s\S]*?)^\)\n/mu)[1].match(/[a-z0-9-]+/gu);
  for (const parent of ["h2-transition", "h2-5g-qualification", "h2-5h-qualification", "h2-7b-qualification", "h2-8a-candidates"]) assert.ok(order.includes(parent));
  assert.ok(!order.includes("command-fixture-parents"));
  const hook = source.slice(begin, end);
  for (const [initial, expected, status] of [[0, "2 1", 0], [1, "1 0", 0], [2, null, 1], [3, null, 1]]) {
    const run = path.join(e.root, "target", `hook-${initial}`);
    fs.mkdirSync(run, { recursive: true });
    const program = `set -u
RUN_DIR=${JSON.stringify(run)}
round=1
refreshed=${initial}
fixes=0
taskpolicy() { shift; "$@"; }
nice() { shift 2; "$@"; }
summary() { :; }
node() {
  if [ "$2" = --check ]; then
    [ "$refreshed" -eq 2 ] && return 2
    [ "$refreshed" -eq 1 ] && return 0
    return 1
  fi
  [ "$2" = --fix ] || return 2
  [ "$refreshed" -eq 3 ] && return 7
  refreshed=1
  fixes=$((fixes+1))
  return 0
}
while [ "$round" -le 3 ]; do
  stale=()
${hook}
${source.slice(clean, cleanEnd)}
  round=$((round+1))
done
echo "$round $fixes"
`;
    const result = spawnSync("bash", ["-c", program], { encoding: "utf8" });
    assert.equal(result.status, status, result.stderr + result.stdout);
    if (expected) assert.equal(result.stdout.trim(), expected);
    else assert.match(result.stdout, initial === 2 ? /PARENT METADATA INVALID/u : /PARENT REFRESH REFUSED/u);
  }
});
