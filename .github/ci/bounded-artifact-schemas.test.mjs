import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { sha256, validateJsonSchemaSubset } from "./qualification.mjs";

const read = relative => fs.readFileSync(new URL(relative, import.meta.url), "utf8");
const contracts = new Map(["h2-7c-qualification", "h2-5h-a-dispositions"].map(kind => {
  const source = read(`./contracts/${kind}.schema.json`);
  return [kind, {
    source,
    schema: JSON.parse(source),
    value: JSON.parse(read(`../../ratchets/${kind}.v1.json`)),
  }];
}));
const qualification = contracts.get("h2-7c-qualification");
const dispositions = contracts.get("h2-5h-a-dispositions");
const diagnosticSchema = { $ref: "#/$defs/diagnostic", $defs: qualification.schema.$defs };
const leaf = {
  code: 1, category: "Message", file: null, start: null, length: null,
  message: "related diagnostic", related_information: null,
};

test("bounded contracts validate both real artifacts and their current schema digests", () => {
  for (const [kind, { source, schema, value }] of contracts) {
    assert.equal(validateJsonSchemaSubset(schema, value), value);
    assert.equal(value.contract.path, `.github/ci/contracts/${kind}.schema.json`);
    assert.equal(value.contract.sha256, sha256(source));
  }
});

test("H2.7c accepts terminal related diagnostics without tightening existing scalar fields", () => {
  for (const related_information of [null, [], [leaf], [{ ...leaf, file: "", start: 0, length: 0 }]]) {
    const value = { ...leaf, related_information };
    assert.equal(validateJsonSchemaSubset(diagnosticSchema, value), value);
  }
});

test("H2.7c rejects malformed children and unqualified deeper related information", () => {
  const mutations = [
    child => { delete child.message; },
    child => { child.extra = true; },
    child => { child.code = "1"; },
    child => { child.start = -1; },
    // The recorded children all terminate at null. Even an empty second-level
    // array is outside this bounded contract, pending explicit qualification.
    child => { child.related_information = []; },
    child => { child.related_information = [structuredClone(leaf)]; },
  ];
  for (const mutate of mutations) {
    const child = structuredClone(leaf);
    mutate(child);
    assert.throws(() => validateJsonSchemaSubset(diagnosticSchema, {
      ...leaf, related_information: [child],
    }), /violates JSON schema/u);
  }
});

function rejectsDisposition(mutate) {
  const changed = structuredClone(dispositions.value);
  mutate(changed);
  assert.throws(() => validateJsonSchemaSubset(dispositions.schema, changed), /violates JSON schema/u);
}

test("H2.5h-a rejects the old roster, missing or extra rows, and out-of-range indices", () => {
  rejectsDisposition(value => { value.rows.length = 45; });
  rejectsDisposition(value => { value.rows.pop(); });
  rejectsDisposition(value => { value.rows.push(structuredClone(value.rows.at(-1))); });
  rejectsDisposition(value => { value.rows.at(-1).index = 58; });
  rejectsDisposition(value => { value.summary.rows = 45; });
});

test("H2.5h-a keeps every summary count exact and preserves the runtime non-admission boundary", () => {
  assert.equal(dispositions.value.summary.activate, 10);
  for (const key of ["proven_unreachable", "undispositioned", "rust_runs", "runtime_admissions_delta"]) {
    assert.equal(dispositions.value.summary[key], 0);
  }
  for (const key of Object.keys(dispositions.value.summary)) {
    rejectsDisposition(value => { value.summary[key] += 1; });
  }
  // This exercises the closed enum; fingerprint integrity belongs to the generator.
  rejectsDisposition(value => { value.rows.at(-1).disposition = "runtime-admitted"; });
});
