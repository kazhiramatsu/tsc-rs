import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import { buildMembershipShadow } from "./fci-h2-5g-membership.mjs";

const plan = readFileSync(".github/ci/plans/h2-5g.v1.json");
const qualification = readFileSync("ratchets/h2-5g-qualification.v1.json");

test("membership shadow is canonical and preserves the qualified partition", () => {
    const first = buildMembershipShadow(plan, qualification);
    const second = buildMembershipShadow(plan, qualification);
    assert.equal(first, second);
    const report = JSON.parse(first);
    assert.equal(report.schema, 1);
    assert.equal(report.authoritative, false);
    assert.equal(report.status, "shadow-membership-only");
    assert.equal(report.denominator, 9027);
    assert.deepEqual(report.counts, {
        admitted: 8511,
        h2_8a_deferred: 6,
        h2_9_deferred: 510,
    });
    assert.equal(report.shards.length, 4);
    assert.equal(report.shards.at(-1).end, 9027);
});

test("membership shadow rejects a stale source digest", () => {
    const stale = Buffer.from(qualification.toString().replace("H2.5g-es2016", "H2.5g-stale"));
    assert.throws(() => buildMembershipShadow(plan, stale), /plan case source digest is stale/);
});

test("membership shadow rejects duplicate case identity after source binding", () => {
    const value = JSON.parse(qualification);
    value.cases[1].case_id = value.cases[0].case_id;
    const mutated = Buffer.from(JSON.stringify(value));
    const boundPlan = JSON.parse(plan);
    boundPlan.case_id_source_sha256 = createHash("sha256").update(mutated).digest("hex");
    assert.throws(
        () => buildMembershipShadow(Buffer.from(JSON.stringify(boundPlan)), mutated),
        /duplicate case_id/,
    );
});
