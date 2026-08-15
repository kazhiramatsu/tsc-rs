#!/usr/bin/env node

/**
 * Pure FCI-5c.1 membership shadow.
 *
 * The exported function consumes only the fixed plan and its qualified case
 * source.  It deliberately does not read the workspace, spawn a candidate,
 * inspect the clock, or manufacture an observation.  The next packet owns
 * the source-snapshot/runner boundary needed for semantic execution.
 */

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const PLAN_PATH = path.join(ROOT, ".github/ci/plans/h2-5g.v1.json");
const QUALIFICATION_PATH = path.join(ROOT, "ratchets/h2-5g-qualification.v1.json");

const EXPECTED = Object.freeze({
    denominator: 9027,
    admitted: 8511,
    h2_8a_deferred: 6,
    h2_9_deferred: 510,
    profile: "h2-5g-es2016",
    suite: "h2",
});

const fail = (message) => {
    throw new Error(`fci-h2-5g-membership: ${message}`);
};

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

const canonical = (value) => {
    if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
    if (value !== null && typeof value === "object") {
        return `{${Object.keys(value)
            .sort()
            .map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`)
            .join(",")}}`;
    }
    return JSON.stringify(value);
};

const parseJson = (bytes, label) => {
    try {
        return JSON.parse(bytes.toString("utf8"));
    } catch (error) {
        fail(`${label} is not valid JSON: ${error.message}`);
    }
};

const requireCondition = (condition, message) => {
    if (!condition) fail(message);
};

const classify = (entry, index) => {
    const id = entry?.case_id;
    requireCondition(typeof id === "string" && id.length > 0, `case ${index} has no case_id`);
    if (entry.disposition === "admitted-for-execution") return "admitted";
    requireCondition(
        entry.disposition === "deferred-to-slices" &&
            entry.diagnostic_disposition?.state === "not-observed-source-deferred" &&
            entry.rust_expectation === "typed-failure-before-first-sink-write",
        `${id} has an invalid deferred disposition`,
    );
    const owner = entry.required_slices?.[0];
    if (owner === "H2.8a") return "h2_8a_deferred";
    if (owner === "H2.9") return "h2_9_deferred";
    fail(`${id} has an unknown deferred owner ${JSON.stringify(owner)}`);
};

const validateHeader = (qualification) => {
    requireCondition(qualification.schema === 1, "qualification schema changed");
    requireCondition(qualification.status === "qualified-typescript-oracle", "qualification status changed");
    requireCondition(qualification.phase === "H2.5g-es2016-target", "qualification phase changed");
    requireCondition(qualification.selection_contract?.global_h2_5g_rows === 11910, "global H2.5g rows changed");
    requireCondition(qualification.selection_contract?.global_candidate_denominator === EXPECTED.denominator, "global denominator changed");
    requireCondition(qualification.selection_contract?.candidate_denominator === EXPECTED.denominator, "candidate denominator changed");
    requireCondition(qualification.summary?.candidates === EXPECTED.denominator, "summary denominator changed");
    requireCondition(qualification.summary?.admitted_cases === EXPECTED.admitted, "admitted count changed");
    requireCondition(qualification.summary?.deferred_cases === 516, "deferred count changed");
    requireCondition(qualification.summary?.source_deferred_cases === 516, "source-deferred count changed");
    requireCondition(qualification.cases?.length === EXPECTED.denominator, "qualification case list changed");
};

const validatePlan = (plan, qualificationBytes) => {
    requireCondition(plan.schema_version === 1, "plan schema changed");
    requireCondition(plan.profile === EXPECTED.profile, "plan profile changed");
    requireCondition(plan.suite === EXPECTED.suite, "plan suite changed");
    requireCondition(plan.denominator === EXPECTED.denominator, "plan denominator changed");
    requireCondition(
        plan.case_id_source === "ratchets/h2-5g-qualification.v1.json",
        "plan case source changed",
    );
    requireCondition(
        plan.case_id_source_sha256 === sha256(qualificationBytes),
        "plan case source digest is stale",
    );
    requireCondition(
        typeof plan.membership_digest === "string" && /^[0-9a-f]{64}$/.test(plan.membership_digest),
        "plan membership digest is invalid",
    );
    requireCondition(
        JSON.stringify(plan.policy_ids) === JSON.stringify(["h2-5g-hosted", "h2-5g-local"]),
        "plan policy ids changed",
    );
    requireCondition(Array.isArray(plan.shards) && plan.shards.length === 4, "plan shard count changed");
    let next = 0;
    for (const [index, shard] of plan.shards.entries()) {
        requireCondition(typeof shard.id === "string" && shard.id === `h2-5g-${String(index).padStart(2, "0")}`, `shard ${index} id changed`);
        requireCondition(Number.isInteger(shard.start) && Number.isInteger(shard.end), `shard ${index} range is not integral`);
        requireCondition(shard.start === next && shard.start < shard.end, `shard ${index} is not contiguous`);
        requireCondition(typeof shard.case_ids_digest === "string" && /^[0-9a-f]{64}$/.test(shard.case_ids_digest), `shard ${index} digest is invalid`);
        next = shard.end;
    }
    requireCondition(next === EXPECTED.denominator, "plan shard coverage changed");
};

/**
 * Build the byte-stable, non-authoritative membership observation.
 *
 * `planBytes` and `qualificationBytes` are already immutable input bytes at
 * this boundary.  The plan's existing digest fields remain bound evidence;
 * the explicit case-id digests below make the source order observable and
 * give the later adapter packet a concrete re-verification input.
 */
export const buildMembershipShadow = (planBytes, qualificationBytes) => {
    const plan = parseJson(Buffer.from(planBytes), "plan");
    const qualification = parseJson(Buffer.from(qualificationBytes), "qualification");
    validatePlan(plan, Buffer.from(qualificationBytes));
    validateHeader(qualification);

    const counts = { admitted: 0, h2_8a_deferred: 0, h2_9_deferred: 0 };
    const caseIds = [];
    const seen = new Set();
    for (const [index, entry] of qualification.cases.entries()) {
        const disposition = classify(entry, index);
        requireCondition(!seen.has(entry.case_id), `duplicate case_id ${entry.case_id}`);
        seen.add(entry.case_id);
        caseIds.push(entry.case_id);
        counts[disposition] += 1;
    }
    requireCondition(counts.admitted === EXPECTED.admitted, "admitted membership changed");
    requireCondition(counts.h2_8a_deferred === EXPECTED.h2_8a_deferred, "H2.8a membership changed");
    requireCondition(counts.h2_9_deferred === EXPECTED.h2_9_deferred, "H2.9 membership changed");

    const shardReports = plan.shards.map((shard) => {
        const ids = caseIds.slice(shard.start, shard.end);
        return {
            bound_digest: shard.case_ids_digest,
            case_id_sha256: sha256(Buffer.from(canonical(ids), "utf8")),
            end: shard.end,
            id: shard.id,
            start: shard.start,
        };
    });
    const report = {
        case_id_sha256: sha256(Buffer.from(canonical(caseIds), "utf8")),
        counts,
        denominator: caseIds.length,
        observation_policy: {
            admitted_repetitions: 2,
            status: "deferred-until-source-snapshot-runner",
        },
        plan: {
            membership_digest: plan.membership_digest,
            qualification_sha256: plan.case_id_source_sha256,
            profile: plan.profile,
            suite: plan.suite,
        },
        schema: 1,
        shards: shardReports,
        status: "shadow-membership-only",
        authoritative: false,
    };
    return canonical(report);
};

const main = () => {
    const [command] = process.argv.slice(2);
    if (command !== "--check" && command !== "--repeat-check") {
        fail("usage: --check | --repeat-check");
    }
    const planBytes = readFileSync(PLAN_PATH);
    const qualificationBytes = readFileSync(QUALIFICATION_PATH);
    const first = buildMembershipShadow(planBytes, qualificationBytes);
    if (command === "--repeat-check") {
        const second = buildMembershipShadow(planBytes, qualificationBytes);
        requireCondition(first === second, "repeated membership evaluation is not byte-identical");
    }
    process.stdout.write(`${first}\n`);
};

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    try {
        main();
    } catch (error) {
        console.error(error.message);
        process.exitCode = 1;
    }
}
