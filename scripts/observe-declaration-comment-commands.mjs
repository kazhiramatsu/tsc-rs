// Replay the ordinary command recipe from h2-8a-declaration-comment-range-observations.md.
// Frozen inputs/expectations are read-only. Internal prefix/parameter traces are
// historical evidence, not part of this complete-command replay claim.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import { createHermeticDirectoryOverlay } from "../crates/oracle/vfs-directory-overlay.mjs";

assert.deepEqual(process.argv.slice(2), ["--check"]);
assert.equal(ts.version, "6.0.3");
const root = path.resolve(import.meta.dirname, "..");
const read = name => fs.readFileSync(path.join(root, name));
const pins = {
  "vendor/typescript-6.0.3/lib/typescript.js": "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39",
  "scripts/observe-jsdoc-block-scope-container.mjs": "cee190bee2cdca35a5fc9c6702e98cd5a089a53dd43c542723b588ce8fc2201f",
  "crates/oracle/vfs-directory-overlay.mjs": "2868391e75941127f8eb3a352232190fd98c6d794235913e288432e5626aab76",
  "crates/compiler/tests/fixtures/declaration-comment-ranges.json": "d5b0d846188dbe37061e852d62b15e510a26edb252bfcceab3333f0c2daf6b9f",
  "crates/compiler/tests/fixtures/declaration-comment-detached-prefixes.json": "73247175cf3b94d0b1f42dab0982119bb0509651e9b655d47e9024db68b0cce6",
  "crates/compiler/tests/fixtures/declaration-comment-parameter-tags.json": "7065e771ef7e3686f21e6f81e998bae4afe2b731824caa15740eb0f6b4076ad5"
};
for (const [name, expected] of Object.entries(pins)) {
  assert.equal(crypto.createHash("sha256").update(read(name)).digest("hex"), expected, name);
}
// The notebook used this same complete observer. Keep its source immutable and
// guard both extraction anchors, rather than maintaining a second comparator.
const source = read("scripts/observe-jsdoc-block-scope-container.mjs").toString("utf8");
const start = "function diagnostic(d) {";
const end = "const cases = inputs.map(input => {";
assert.equal(source.split(start).length, 2);
assert.equal(source.split(end).length, 2);
const functions = start + source.split(start)[1].split(end)[0];
const observe = new Function("root", "ts", "fs", "path", "assert", "createHermeticDirectoryOverlay",
  functions + "\nreturn observe;")(root, ts, fs, path, assert, createHermeticDirectoryOverlay);
let commands = 0;
for (const [group, count] of [["ranges", 17], ["detached-prefixes", 12], ["parameter-tags", 12]]) {
  const fixture = JSON.parse(read(`crates/compiler/tests/fixtures/declaration-comment-${group}.json`));
  assert.equal(fixture.typescript, "6.0.3");
  assert.equal(fixture.repetitions, 2);
  assert.equal(fixture.cases.length, count);
  assert.equal(new Set(fixture.cases.map(row => row.case_id)).size, count);
  for (const row of fixture.cases) {
    // Construct the input explicitly; the observer never receives expected output.
    const input = {case_id: row.case_id, roots: row.roots, files: row.files, options: row.options};
    for (let repetition = 0; repetition < 2; repetition++) {
      assert.deepEqual(observe(input), row.typescript_observation, `${row.case_id} repetition=${repetition}`);
      commands++;
    }
    console.log(JSON.stringify({case_id: row.case_id, complete_command_executions: 2}));
  }
}
assert.equal(commands, 82);
console.log(JSON.stringify({cases: 41, complete_command_executions: commands, internal_traces_replayed: false}));
