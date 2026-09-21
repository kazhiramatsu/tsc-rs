// Independent raw TypeScript commands for the supplemental project loader run.
// This artifact does not replace a parser census or its selected input set.
import assert from "node:assert/strict";
import {execFileSync} from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import ts from "../vendor/typescript-6.0.3/lib/typescript.js";
import {documentPool, sha256} from "./recovery-command-input.mjs";
import {observeSelectedRow} from "./observe-recovery-selected-corpus.mjs";

const [nativePath, rosterPath, output, ...extra] = process.argv.slice(2);
assert.ok(nativePath && rosterPath && output && !extra.length,
  "usage: node scripts/observe-project-command-supplement.mjs NATIVE ROSTER OUTPUT");
assert.ok(!fs.existsSync(output), "refusing to overwrite project observations");
const root = path.resolve(import.meta.dirname, "..");
const git = (dir, args) => execFileSync("git", args, {cwd: dir, encoding: "utf8"}).trim();
const clean = dir => {
  assert.equal(git(dir, ["diff", "HEAD", "--name-only"]), "", "project source/data changed");
  assert.equal(git(dir, ["ls-files", "--others", "--exclude-standard", "--", "crates", "scripts"]), "", "untracked project code");
  return git(dir, ["rev-parse", "HEAD"]);
};
const bytes = fs.readFileSync(nativePath), native = JSON.parse(bytes);
assert.equal(native.schema, 1); assert.equal(native.kind, "emitter-project-projection-native");
assert.equal(native.repetitions, 2);
assert.equal(sha256(fs.readFileSync(rosterPath)), native.roster_sha256);
const roster = JSON.parse(fs.readFileSync(rosterPath));
assert.equal(roster.schema, 1); assert.equal(roster.kind, "emitter-project-projection-roster");
assert.deepEqual(roster.case_ids, native.case_ids);
assert.equal(clean(root), native.head);
assert.equal(clean(native.input_workspace), native.input_head);
const pins = JSON.parse(fs.readFileSync(path.join(root, "scripts/recovery-command-pins.json")));
for (const [name, hash] of Object.entries(pins)) assert.equal(sha256(fs.readFileSync(path.join(root, name))), hash, `review mirror pin: ${name}`);
for (const [name, hash] of Object.entries(native.dependencies)) assert.equal(sha256(fs.readFileSync(path.join(root, name))), hash, name);
for (const library of [native.library_root, path.join(root, "vendor/typescript-6.0.3/lib")]) {
  assert.equal(sha256(fs.readFileSync(path.join(library, "typescript.js"))), native.compiler_sha256);
}
const manifest = path.join(native.input_workspace, "vendor/typescript-6.0.3/test-suite-expansion.v1.json");
assert.equal(sha256(fs.readFileSync(manifest)), native.plan_manifest_sha256);
assert.equal(git(native.input_workspace, ["rev-parse", "HEAD:vendor/typescript-6.0.3"]), native.vendor_tree_hash);
const pool = documentPool(native.documents), seen = new Set(), cases = [];
assert.deepEqual(native.cases.map(c => c.case_id), native.case_ids);
for (const row of native.cases) {
  assert.ok(!seen.has(row.case_id)); seen.add(row.case_id);
  if (row.load_error !== undefined) {
    assert.equal(row.disposition, "not-loaded; emit-not-qualified");
    cases.push({case_id: row.case_id, disposition: "native-input-unavailable; emit-not-qualified"});
  } else {
    assert.equal(row.row.case_id, row.case_id);
    assert.equal(row.row.loader, "load_project_emit");
    cases.push(observeSelectedRow(row.row, pool, native.library_root));
  }
  console.log(`${cases.length}/${native.cases.length} ${row.case_id}: ${cases.at(-1).disposition}`);
}
assert.equal(clean(root), native.head); assert.equal(clean(native.input_workspace), native.input_head);
assert.equal(sha256(fs.readFileSync(manifest)), native.plan_manifest_sha256);
assert.equal(sha256(fs.readFileSync(path.join(native.library_root, "typescript.js"))), native.compiler_sha256);
const dependencies = Object.fromEntries(["scripts/observe-project-command-supplement.mjs", "scripts/recovery-command-input.mjs", "scripts/observe-recovery-selected-corpus.mjs", "scripts/recovery-command-options.json", "scripts/recovery-command-pins.json"]
  .map(name => [name, sha256(fs.readFileSync(path.join(root, name)))]));
fs.mkdirSync(path.dirname(output), {recursive: true});
fs.writeFileSync(output, JSON.stringify({schema: 1, kind: "emitter-project-projection-typescript", typescript: ts.version,
  head: native.head, input_head: native.input_head, native_sha256: sha256(bytes), roster_sha256: native.roster_sha256,
  compiler_sha256: native.compiler_sha256, library_root: native.library_root, input_workspace: native.input_workspace,
  repetitions: 2, dependencies, cases}) + "\n", {flag: "wx"});
