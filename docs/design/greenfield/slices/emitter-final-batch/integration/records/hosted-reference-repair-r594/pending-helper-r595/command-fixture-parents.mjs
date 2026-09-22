// Reconcile one enumerated command-reference family after all oracle rungs.
// These five fixtures are profile inputs, never qualification inputs. Keeping
// that boundary lets the ordinary next walk round converge without an oracle
// cycle. This metadata check is not a substitute for native/TypeScript replay.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const SELECTION = "docs/design/greenfield/slices/h2-5h-parameter-temporaries-selection.v1.json";
const FIXTURES = "crates/compiler/tests/fixtures/";
const DISPOSITIONS = "ratchets/h2-candidate-dispositions.v1.json";
const OUTPUT_INPUTS = "ratchets/h2-8a-candidate-inputs.v1.json";
const FIVE_G = "ratchets/h2-5g-qualification.v1.json";
const FIVE_H = "ratchets/h2-5h-qualification.v1.json";
const SEVEN_B = "ratchets/h2-7b-qualification.v1.json";
const PROFILE = "ratchets/h2-5g-profile.v1.json";
const TYPESCRIPT = "vendor/typescript-6.0.3/lib/typescript.js";
const HEX = /^[a-f0-9]{64}$/u;
const MAX_BUFFER = 256 * 1024 * 1024;
export const sha = value => crypto.createHash("sha256").update(value).digest("hex");
const pair = (pointer, parent) => Object.freeze({ pointer: Object.freeze(pointer), parent });

export const SPECS = Object.freeze([
  { name: "ef7-217", file: FIXTURES + "emitter-final-universe.json", count: 217,
    observer: "scripts/observe-emitter-final-universe.mjs", args: ["--set", "217"],
    parents: [pair(["inputs", 1, "sha256"], DISPOSITIONS), pair(["inputs", 2, "sha256"], OUTPUT_INPUTS)] },
  { name: "ef7-plan-base", file: FIXTURES + "emitter-final-universe-plan-base.json.zst", count: 1798,
    observer: "scripts/observe-emitter-final-universe.mjs", args: ["--set", "plan-base"],
    parents: [pair(["inputs", 1, "sha256"], DISPOSITIONS), pair(["inputs", 2, "sha256"], OUTPUT_INPUTS)] },
  { name: "jsdoc", file: FIXTURES + "emitter-jsdoc-original-command.json", count: 1,
    observer: "scripts/observe-emitter-jsdoc-original-command.mjs", args: [], exclusive: true,
    parents: [pair(["selection", 0, "artifact_sha256"], SEVEN_B)] },
  { name: "parameters", file: FIXTURES + "h2-5h-parameter-temporaries.json", count: 68,
    observer: "scripts/observe-h2-5h-parameter-temporaries.mjs", args: [], exclusive: true,
    parents: [pair(["parents", 0, "sha256"], FIVE_H), pair(["parents", 1, "sha256"], FIVE_G)] },
  { name: "utf16", file: FIXTURES + "utf16-original-rows-complete.json", count: 4,
    observer: "scripts/observe-utf16-original-rows-complete.mjs", args: [], exclusive: true,
    parents: [pair(["parent", "sha256"], FIVE_H)] },
].map(spec => Object.freeze({ ...spec, args: Object.freeze(spec.args), parents: Object.freeze(spec.parents) })));

export const PARAMETER_IDS = Object.freeze(["es5", "es2015"].flatMap(target =>
  ["nullishCoalescingOperator", "optionalChaining"].flatMap(family =>
    ["BindingPattern", "Initializer"].map(form =>
      `typescript-6.0.3/conformance/expressions/${family}/${family}InParameter${form}.ts#target%3D${target}`))));

function read(root, relative) {
  const file = path.join(root, relative);
  assert.ok(fs.lstatSync(file).isFile(), `not a regular file: ${relative}`);
  return fs.readFileSync(file);
}

function decode(file, bytes) {
  if (!file.endsWith(".zst")) return bytes;
  const result = spawnSync("zstd", ["-q", "-d", "--stdout"], { input: bytes, maxBuffer: MAX_BUFFER });
  assert.equal(result.error, undefined, `zstd failed: ${result.error?.message}`);
  assert.equal(result.status, 0, "zstd failed to decode the complete fixture");
  return result.stdout;
}

const get = (document, pointer) => pointer.reduce((value, key) => value[key], document);
function set(document, pointer, value) {
  get(document, pointer.slice(0, -1))[pointer.at(-1)] = value;
}

export function ef7Inputs(idSet) {
  return [
    `docs/design/greenfield/slices/emitter-final-batch/ef7/universe-${idSet === "217" ? "217" : "plan-base"}.v1.json`,
    DISPOSITIONS, OUTPUT_INPUTS,
    "vendor/typescript-6.0.3/test-suite-expansion.v1.json",
    "vendor/typescript-6.0.3/conformance-suite-expansion.v1.json",
    "vendor/typescript-6.0.3/compiler-config-plans.v1.json",
    "crates/oracle/h2-8a-candidates.mjs", "crates/oracle/vfs-directory-overlay.mjs",
    TYPESCRIPT, ".node-version",
  ];
}

export function inspect(root = ROOT) {
  const guards = new Map();
  const guarded = relative => {
    if (!guards.has(relative)) guards.set(relative, read(root, relative));
    return guards.get(relative);
  };
  const digest = relative => sha(guarded(relative));
  assert.equal(process.version, `v${guarded(".node-version").toString().trim()}`, "Node version mismatch");
  const profile = JSON.parse(guarded(PROFILE));
  for (const spec of SPECS) {
    assert.equal(profile.runtime_inputs.filter(row => row.path === spec.file).length, 1,
      `profile must contain exactly one input for ${spec.file}`);
  }
  const selectionBytes = read(root, SELECTION);
  const selection = JSON.parse(selectionBytes);
  assert.equal(selection.schema, "h2-5h-parameter-temporaries-preparation/v1");
  assert.deepEqual(selection.cases.map(row => row.case_id), PARAMETER_IDS, "parameter selection identities changed");
  assert.equal(selection.files[TYPESCRIPT], digest(TYPESCRIPT), "parameter TypeScript pin changed");
  for (const [index, row] of selection.cases.entries()) {
    assert.equal(row.artifact, index < 4 ? FIVE_H : FIVE_G);
    assert.equal(row.role, index < 4 ? "repair-candidate" : "frozen-adjacent-control");
    assert.match(row.sha256_json_stringify, HEX);
  }
  const selectionUpdates = [FIVE_H, FIVE_G].map(parent => {
    assert.match(selection.files[parent], HEX);
    return { parent, before: selection.files[parent], after: digest(parent) };
  });
  const fixtures = SPECS.map(spec => {
    const bytes = read(root, spec.file);
    const document = JSON.parse(decode(spec.file, bytes));
    assert.equal(document.typescript, "6.0.3", spec.file);
    assert.equal(document.repetitions, 2, spec.file);
    assert.equal(document.cases.length, spec.count, `${spec.file}: case count changed`);
    if (spec.name.startsWith("ef7-")) {
      assert.equal(document.schema, 1);
      assert.equal(document.kind, "emitter-final-universe");
      assert.equal(document.id_set, spec.args[1]);
      assert.equal(document.generator.path, spec.observer);
      assert.equal(document.generator.sha256, digest(spec.observer), "EF7 observer changed");
      assert.deepEqual(document.inputs.map(input => input.path), ef7Inputs(spec.args[1]), "EF7 input roster changed");
      for (const [index, input] of document.inputs.entries()) {
        assert.match(input.sha256, HEX);
        if (index === 1 || index === 2) {
          assert.equal(input.path, index === 1 ? DISPOSITIONS : OUTPUT_INPUTS);
        } else {
          assert.equal(input.sha256, digest(input.path), `non-parent EF7 input changed: ${input.path}`);
        }
      }
    } else {
      assert.equal(document.version, 1);
      assert.equal(document.observer_sha256, digest(spec.observer), `${spec.name}: observer changed`);
      assert.equal(document.compiler_sha256, digest(TYPESCRIPT), `${spec.name}: TypeScript changed`);
      if (spec.name === "jsdoc") assert.equal(document.selection.length, 1);
      if (spec.name === "parameters") assert.equal(document.parents.length, 2);
    }
    const updates = spec.parents.map(({ pointer, parent }) => {
      const reference = get(document, pointer.slice(0, -1));
      assert.equal(reference.path ?? reference.universe, parent, `${spec.name}: parent identity changed`);
      const before = get(document, pointer);
      assert.match(before, HEX);
      return { pointer, parent, before, after: digest(parent) };
    });
    if (spec.name === "parameters") {
      assert.equal(document.selection_sha256, sha(selectionBytes), "selection changed outside the parent refresh");
      for (const update of updates) assert.equal(update.before, selection.files[update.parent]);
    }
    return { spec, bytes, document, updates, stale: updates.some(update => update.before !== update.after) };
  });
  return { fixtures, selection, selectionBytes, selectionUpdates, guards,
    stale: fixtures.filter(fixture => fixture.stale).map(fixture => fixture.spec.name) };
}

function verifySelectedRows(state) {
  const parents = new Map([FIVE_H, FIVE_G].map(parent => [parent, JSON.parse(state.guards.get(parent))]));
  for (const pinned of state.selection.cases) {
    const matches = parents.get(pinned.artifact).cases.filter(row => row.case_id === pinned.case_id);
    assert.equal(matches.length, 1, `selected row missing or duplicated: ${pinned.case_id}`);
    assert.equal(sha(JSON.stringify(matches[0])), pinned.sha256_json_stringify,
      `selected complete row changed: ${pinned.case_id}`);
  }
}

function snapshot(root, relative) {
  const stat = fs.statSync(path.join(root, relative), { bigint: true });
  return { relative, bytes: read(root, relative), mode: Number(stat.mode & 0o777n),
    atimeNs: stat.atimeNs.toString(), mtimeNs: stat.mtimeNs.toString() };
}

function restore(root, snapshots) {
  for (const saved of snapshots) {
    fs.writeFileSync(path.join(root, saved.relative), saved.bytes);
    fs.chmodSync(path.join(root, saved.relative), saved.mode);
  }
  // Node's Date/double timestamp API cannot restore arbitrary nanoseconds.
  // Python's integer-nanosecond API preserves the original stability markers.
  const result = spawnSync("python3", ["-c",
    "import json,os,sys\nfor p,a,m in json.load(sys.stdin): os.utime(p,ns=(int(a),int(m)))\n"],
  { input: JSON.stringify(snapshots.map(saved =>
      [path.join(root, saved.relative), saved.atimeNs, saved.mtimeNs])), maxBuffer: MAX_BUFFER });
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, "could not restore exact file timestamps");
}

function originalWriter(root, spec, log) {
  const fd = fs.openSync(log, "wx");
  try {
    return spawnSync(process.execPath, [spec.observer, "--write", ...spec.args],
      { cwd: root, stdio: ["ignore", fd, fd] });
  } finally {
    fs.closeSync(fd);
  }
}

// An injected writer is used only by tests with temporary roots; the CLI always
// invokes the original fixed commands in the canonical checkout.
export function refresh({ root = ROOT, recordDir, writer = originalWriter } = {}) {
  const state = inspect(root);
  if (state.stale.length === 0) return { changed: false, stale: [], writers: [] };
  verifySelectedRows(state);
  assert.equal(typeof recordDir, "string", "--fix requires a new evidence directory");
  const directory = path.resolve(recordDir);
  const target = path.resolve(root, "target") + path.sep;
  assert.ok(directory.startsWith(target), "evidence must be under target/");
  assert.ok(!fs.existsSync(directory), "evidence directory already exists");
  const snapshots = [...SPECS.map(spec => spec.file), SELECTION].map(file => snapshot(root, file));
  const store = (prefix, file, bytes) => {
    const destination = path.join(directory, prefix, file + ".gz");
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.writeFileSync(destination, gzipSync(bytes), { flag: "wx" });
    return { path: path.relative(directory, destination), sha256: sha(bytes), bytes: bytes.length };
  };
  fs.mkdirSync(directory, { recursive: true });
  const report = { status: "prepared", writer_kind: writer === originalWriter ? "original" : "injected-test",
    scope: "Only enumerated current-parent metadata may change; native verification is separate",
    started_at: new Date().toISOString(), stale: state.stale, writers: [], before: [], guards: [], after: [] };
  const record = () => fs.writeFileSync(path.join(directory, "result.json"), JSON.stringify(report, null, 2) + "\n");
  for (const saved of snapshots) report.before.push({ ...store("before", saved.relative, saved.bytes),
    file: saved.relative, mode: saved.mode, atime_ns: saved.atimeNs, mtime_ns: saved.mtimeNs });
  for (const [file, bytes] of state.guards) report.guards.push({ file, ...store("inputs", file, bytes) });
  record();
  let changed = false;
  const validatedOutputs = new Map();
  try {
    // These are the only two selection leaves this helper owns. Byte-level
    // replacement plus a full parsed comparison protects its historical data.
    const expectedSelection = structuredClone(state.selection);
    let selectionText = state.selectionBytes.toString();
    for (const update of state.selectionUpdates) {
      if (update.before === update.after) continue;
      const needle = `${JSON.stringify(update.parent)}: ${JSON.stringify(update.before)}`;
      assert.equal(selectionText.split(needle).length, 2, "selection parent field is not unique");
      selectionText = selectionText.replace(needle,
        `${JSON.stringify(update.parent)}: ${JSON.stringify(update.after)}`);
      expectedSelection.files[update.parent] = update.after;
    }
    assert.deepStrictEqual(JSON.parse(selectionText), expectedSelection, "selection changed outside its two parent leaves");
    if (selectionText !== state.selectionBytes.toString()) {
      changed = true;
      fs.writeFileSync(path.join(root, SELECTION), selectionText);
    }
    for (const fixture of state.fixtures) {
      const { spec } = fixture;
      if (!fixture.stale) continue;
      const expected = structuredClone(fixture.document);
      for (const update of fixture.updates) set(expected, update.pointer, update.after);
      if (spec.name === "parameters") expected.selection_sha256 = sha(selectionText);
      const log = path.join(directory, spec.name + ".log");
      report.status = "writing";
      report.active_writer = spec.name;
      record();
      changed = true;
      if (spec.exclusive) fs.unlinkSync(path.join(root, spec.file));
      const started = performance.now();
      const result = writer(root, spec, log);
      const logBytes = fs.existsSync(log) ? fs.readFileSync(log) : Buffer.alloc(0);
      report.writers.push({ name: spec.name, argv: [process.execPath, spec.observer, "--write", ...spec.args],
        exit: result.status, signal: result.signal ?? null, error: result.error?.message ?? null,
        seconds: (performance.now() - started) / 1000, log_sha256: sha(logBytes), log_bytes: logBytes.length });
      record();
      assert.equal(result.error, undefined, `${spec.name}: writer failed`);
      assert.equal(result.status, 0, `${spec.name}: writer exited unsuccessfully`);
      const bytes = read(root, spec.file);
      const actual = JSON.parse(decode(spec.file, bytes));
      report.after.push({ file: spec.file, ...store("after", spec.file, bytes) });
      assert.deepStrictEqual(actual, expected, `${spec.name}: writer changed a non-parent field`);
      assert.equal(JSON.stringify(actual), JSON.stringify(expected), `${spec.name}: field or row order changed`);
      assert.equal(JSON.stringify(actual.cases), JSON.stringify(fixture.document.cases), `${spec.name}: complete case payload changed`);
      validatedOutputs.set(spec.file, bytes);
    }
    for (const [file, bytes] of validatedOutputs) {
      assert.deepStrictEqual(read(root, file), bytes, `writer modified a previously validated output: ${file}`);
    }
    for (const [file, bytes] of state.guards) {
      assert.equal(sha(read(root, file)), sha(bytes), `input changed during refresh: ${file}`);
    }
    assert.equal(read(root, SELECTION).toString(), selectionText, "writer changed the selection document");
    for (const fixture of state.fixtures.filter(candidate => !candidate.stale)) {
      assert.deepStrictEqual(read(root, fixture.spec.file), fixture.bytes, "writer touched an unselected fixture");
    }
    assert.deepEqual(inspect(root).stale, [], "parent metadata is still stale after refresh");
    report.status = "passed";
    delete report.active_writer;
    report.finished_at = new Date().toISOString();
    record();
    return { changed: true, stale: state.stale, writers: report.writers.map(row => row.name) };
  } catch (error) {
    const captureErrors = [];
    for (const saved of snapshots) {
      try {
        const file = path.join(root, saved.relative);
        if (fs.existsSync(file)) store("failed", saved.relative, fs.readFileSync(file));
      } catch (captureError) {
        captureErrors.push(`${saved.relative}: ${captureError.message}`);
      }
    }
    let restoreError;
    if (changed) {
      try { restore(root, snapshots); } catch (failure) { restoreError = failure; }
    }
    report.status = "refused";
    report.error = String(error);
    report.restored = changed && !restoreError;
    report.capture_errors = captureErrors;
    if (restoreError) report.restore_error = String(restoreError);
    try { record(); } catch (failure) { console.error(`could not record refusal: ${failure.message}`); }
    if (restoreError) throw new AggregateError([error, restoreError], "refresh failed and output recovery is incomplete; use the preserved before snapshots");
    throw error;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [mode, flag, recordDir, ...rest] = process.argv.slice(2);
    assert.ok(mode === "--check" ? flag === undefined :
      mode === "--fix" && flag === "--record-dir" && recordDir && rest.length === 0, "use --check or --fix --record-dir <new target directory>");
    const result = mode === "--check" ? { stale: inspect().stale } : refresh({ recordDir });
    console.log(JSON.stringify({ family: "command-fixture-parents", ...result }));
    if (mode === "--check" && result.stale.length) process.exitCode = 1;
  } catch (error) {
    console.error(`command-fixture-parents REFUSED: ${error.message}`);
    process.exitCode = 2;
  }
}
