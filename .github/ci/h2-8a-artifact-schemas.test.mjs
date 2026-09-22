import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { validateJsonSchemaSubset } from "./qualification.mjs";

const read = relative => JSON.parse(fs.readFileSync(new URL(relative, import.meta.url), "utf8"));
const artifacts = new Map(["candidates", "candidate-inputs", "observations"].map(kind => [kind, {
  schema: read(`./contracts/h2-8a-${kind}.schema.json`),
  value: read(`../../ratchets/h2-8a-${kind}.v1.json`),
}]));

function rejects(kind, mutate) {
  const { schema, value } = artifacts.get(kind);
  const changed = structuredClone(value);
  mutate(changed);
  assert.throws(() => validateJsonSchemaSubset(schema, changed), /violates JSON schema/u);
}

test("H2.8a schemas accept the original three artifacts without re-minting", () => {
  for (const { schema, value } of artifacts.values()) {
    assert.equal(validateJsonSchemaSubset(schema, value), value);
  }
});

test("H2.8a candidate schemas reject qualification claims and missing provenance", () => {
  rejects("candidates", value => { value.status = "qualified"; });
  rejects("candidates", value => { value.cases[0].runtime_admitted = true; });
  rejects("candidates", value => { value.summary.runtime_admitted = 1; });
  rejects("candidates", value => { value.cases[0].disposition = "admitted"; });
  rejects("candidates", value => { delete value.cases[0].input_sha256; });
  rejects("candidates", value => { value.cases[0].input_sha256 = "wrong"; });
  rejects("candidates", value => { value.generator.path = "unreviewed.mjs"; });
  for (const kind of artifacts.keys()) {
    rejects(kind, value => { value.cases.pop(); });
    rejects(kind, value => { value.extra = true; });
  }
});

test("H2.8a command inputs retain route-specific shape and the project mount", () => {
  rejects("candidate-inputs", value => { value.shared_mounts.projects.pop(); });
  rejects("candidate-inputs", value => { delete value.cases[0].input.roots; });
  rejects("candidate-inputs", value => { value.cases[0].input.route = "transpile-api"; });
  rejects("candidate-inputs", value => {
    value.cases[0].input = { route: "transpile-api", api: "transpileModule", report_diagnostics: true };
  });
});

test("H2.8a reference observations require both runs and the complete command tuple", () => {
  rejects("observations", value => { value.repetitions = 1; });
  rejects("observations", value => { value.cases[0].repetitions = 1; });
  rejects("observations", value => { value.cases[0].disposition = "admitted"; });
  rejects("observations", value => { value.summary.runtime_admitted = 1; });
  rejects("observations", value => { value.generator.path = "unreviewed.mjs"; });
  rejects("observations", value => { delete value.cases[0].typescript_observation.emit_result; });
  rejects("observations", value => {
    delete value.cases[0].typescript_observation.writes[0].on_error_callback_present;
  });
  rejects("observations", value => { delete value.cases[0].typescript_observation.exit_code; });
});
