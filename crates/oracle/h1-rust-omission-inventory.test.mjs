import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import test from "node:test";
import { collectAnchorFailures } from "./h1-rust-omission-inventory.mjs";

test("omission references report all missing and non-unique anchors", () => {
  const failures = collectAnchorFailures([
    ["missing", "source.rs", "absent"],
    ["ambiguous", "source.rs", "repeat"],
    ["valid", "source.rs", "unique"],
    ["unreadable", "missing.rs", "needle"],
  ], file => {
    if (file === "missing.rs") throw new Error("missing source");
    return "unique repeat repeat";
  });
  assert.deepEqual(failures, [
    "missing anchor missing in source.rs",
    "anchor ambiguous is not unique in source.rs",
    "cannot read anchor unreadable in missing.rs: missing source",
  ]);
});

test("omission references reject duplicate identities even with unique text", () => {
  assert.deepEqual(collectAnchorFailures([
    ["same", "one.rs", "one"], ["same", "two.rs", "two"],
  ], file => file.slice(0, -3)), ["duplicate anchor identifier same"]);
});

test("reference-only validation checks current sources without minting an artifact", () => {
  const root = path.resolve(import.meta.dirname, "../..");
  const artifact = path.join(root, "ratchets/h1-rust-omissions.v1.json");
  const original = fs.readFileSync(artifact);
  const result = spawnSync(process.execPath,
    [path.join(import.meta.dirname, "h1-rust-omission-inventory.mjs"), "--check-anchors"],
    {cwd: root, encoding: "utf8"});
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /references valid:/);
  assert.match(result.stdout, /artifact freshness NOT asserted/);
  assert.deepEqual(fs.readFileSync(artifact), original);
});

// Import also validates the current repository references. The injected
// reader tests above exercise aggregation independently of those sources.
test("symlinked generator entries execute their requested validation", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "h1-omission-entry-"));
  try {
    const entry = path.join(directory, "inventory.mjs");
    fs.symlinkSync(path.join(import.meta.dirname, "h1-rust-omission-inventory.mjs"), entry);
    const checked = spawnSync(process.execPath, [entry, "--check-anchors"], {encoding: "utf8"});
    assert.equal(checked.status, 0, checked.stderr);
    assert.match(checked.stdout, /artifact freshness NOT asserted/);
    const invalid = spawnSync(process.execPath, [entry, "--unknown-mode"], {encoding: "utf8"});
    assert.notEqual(invalid.status, 0);
    assert.match(invalid.stderr, /usage: h1-rust-omission-inventory/);
  } finally {
    fs.rmSync(directory, {recursive: true, force: true});
  }
});
