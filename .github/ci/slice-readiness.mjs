#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const SCHEMA_PATH = path.join(ROOT, ".github/ci/contracts/slice-readiness.v1.schema.json");
const BOOTSTRAP_PATH = path.join(ROOT, "ratchets/fci-packet-bootstrap.v1.json");
const READINESS_DIR = path.join(ROOT, "ratchets/fci-readiness");

const fail = (message) => {
    throw new Error(`slice-readiness: ${message}`);
};

const readJson = (filePath) => {
    try {
        return JSON.parse(readFileSync(filePath, "utf8"));
    } catch (error) {
        fail(`${path.relative(ROOT, filePath)} is not valid JSON: ${error.message}`);
    }
};

const sha256File = (filePath) =>
    createHash("sha256").update(readFileSync(filePath)).digest("hex");

const isSha256 = (value) => typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
const isCommit = (value) => typeof value === "string" && /^[0-9a-f]{40}$/.test(value);

const assertRelativePath = (value, label) => {
    if (typeof value !== "string" || value.length === 0 || value.startsWith("/")) {
        fail(`${label} must be a non-empty relative path`);
    }
    if (value.includes("\\") || value.includes("\0") || value.split("/").some((part) => part === ".." || part === "")) {
        fail(`${label} must use normalized workspace-relative components`);
    }
};

const assertExactKeys = (value, keys, label) => {
    const actual = Object.keys(value).sort();
    const expected = [...keys].sort();
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
        fail(`${label} has unexpected or missing fields`);
    }
};

const validateEnvelope = (envelope) => {
    if (envelope === null || typeof envelope !== "object" || Array.isArray(envelope)) {
        fail("readiness envelope must be an object");
    }
    assertExactKeys(
        envelope,
        [
            "schemaVersion",
            "packetId",
            "status",
            "trustedBase",
            "packet",
            "predecessors",
            "allowedPaths",
            "forbiddenPrefixes",
            "proof",
        ],
        "readiness envelope",
    );
    if (envelope.schemaVersion !== 1) fail("schemaVersion must be 1");
    if (typeof envelope.packetId !== "string" || !/^[a-z0-9][a-z0-9.-]*$/.test(envelope.packetId)) {
        fail("packetId is not canonical");
    }
    if (!["design", "ready", "closed"].includes(envelope.status)) {
        fail(`invalid status for ${envelope.packetId}`);
    }
    if (!isCommit(envelope.trustedBase)) fail(`${envelope.packetId} trustedBase is not a commit`);

    if (envelope.packet === null || typeof envelope.packet !== "object" || Array.isArray(envelope.packet)) {
        fail(`${envelope.packetId} packet must be an object`);
    }
    assertExactKeys(envelope.packet, ["path", "sha256"], `${envelope.packetId}.packet`);
    assertRelativePath(envelope.packet.path, `${envelope.packetId}.packet.path`);
    if (!envelope.packet.path.endsWith(".md") || !isSha256(envelope.packet.sha256)) {
        fail(`${envelope.packetId} packet path/digest is invalid`);
    }

    if (!Array.isArray(envelope.predecessors)) fail(`${envelope.packetId}.predecessors must be an array`);
    for (const [index, predecessor] of envelope.predecessors.entries()) {
        if (predecessor === null || typeof predecessor !== "object" || Array.isArray(predecessor)) {
            fail(`${envelope.packetId}.predecessors[${index}] must be an object`);
        }
        assertExactKeys(predecessor, ["packetId", "receiptSha256"], `${envelope.packetId}.predecessors[${index}]`);
        if (typeof predecessor.packetId !== "string" || !isSha256(predecessor.receiptSha256)) {
            fail(`${envelope.packetId}.predecessors[${index}] is invalid`);
        }
    }

    for (const field of ["allowedPaths", "forbiddenPrefixes"]) {
        if (!Array.isArray(envelope[field]) || envelope[field].length === 0) {
            fail(`${envelope.packetId}.${field} must be a non-empty array`);
        }
        const normalized = new Set();
        for (const [index, item] of envelope[field].entries()) {
            assertRelativePath(item, `${envelope.packetId}.${field}[${index}]`);
            if (normalized.has(item)) fail(`${envelope.packetId}.${field} contains a duplicate`);
            normalized.add(item);
        }
    }

    if (envelope.proof === null || typeof envelope.proof !== "object" || Array.isArray(envelope.proof)) {
        fail(`${envelope.packetId}.proof must be an object`);
    }
    assertExactKeys(envelope.proof, ["commands", "evidencePath"], `${envelope.packetId}.proof`);
    if (!Array.isArray(envelope.proof.commands) || envelope.proof.commands.length === 0 || envelope.proof.commands.some((command) => typeof command !== "string" || command.length === 0)) {
        fail(`${envelope.packetId}.proof.commands is invalid`);
    }
    assertRelativePath(envelope.proof.evidencePath, `${envelope.packetId}.proof.evidencePath`);
    return envelope;
};

const validateSchemaDocument = () => {
    const schema = readJson(SCHEMA_PATH);
    assertExactKeys(schema, ["$schema", "$id", "title", "type", "additionalProperties", "required", "properties"], "slice schema");
    if (schema.type !== "object" || schema.additionalProperties !== false || schema.properties?.schemaVersion?.const !== 1) {
        fail("slice schema does not freeze the v1 object boundary");
    }
    const required = new Set(schema.required ?? []);
    for (const field of ["schemaVersion", "packetId", "status", "trustedBase", "packet", "predecessors", "allowedPaths", "forbiddenPrefixes", "proof"]) {
        if (!required.has(field)) fail(`slice schema omits required field ${field}`);
    }
    return schema;
};

const validateBootstrap = () => {
    const bootstrap = readJson(BOOTSTRAP_PATH);
    assertExactKeys(
        bootstrap,
        ["schemaVersion", "packetId", "status", "trustedBase", "schemaPath", "schemaSha256", "checkerPath", "checkerSha256", "allowedPacketIds", "forbiddenTransitions", "proof"],
        "bootstrap ratchet",
    );
    if (bootstrap.schemaVersion !== 1 || bootstrap.packetId !== "packet-control-bootstrap" || bootstrap.status !== "ready") {
        fail("bootstrap ratchet is not the ready v1 record");
    }
    if (!isCommit(bootstrap.trustedBase) || !isSha256(bootstrap.schemaSha256) || !isSha256(bootstrap.checkerSha256)) {
        fail("bootstrap ratchet identity is invalid");
    }
    assertRelativePath(bootstrap.schemaPath, "bootstrap schemaPath");
    assertRelativePath(bootstrap.checkerPath, "bootstrap checkerPath");
    if (sha256File(path.join(ROOT, bootstrap.schemaPath)) !== bootstrap.schemaSha256) fail("schema digest does not match bootstrap ratchet");
    if (sha256File(path.join(ROOT, bootstrap.checkerPath)) !== bootstrap.checkerSha256) fail("checker digest does not match bootstrap ratchet");
    if (!Array.isArray(bootstrap.allowedPacketIds) || bootstrap.allowedPacketIds.length === 0) fail("bootstrap allowedPacketIds is empty");
    if (!Array.isArray(bootstrap.forbiddenTransitions) || bootstrap.forbiddenTransitions.length === 0) fail("bootstrap forbiddenTransitions is empty");
    return bootstrap;
};

const checkPacket = (packetId) => {
    const bootstrap = validateBootstrap();
    if (!bootstrap.allowedPacketIds.includes(packetId)) fail(`${packetId} is not authorized by bootstrap`);
    const envelopePath = path.join(READINESS_DIR, `${packetId}.v1.json`);
    const envelope = validateEnvelope(readJson(envelopePath));
    if (envelope.packetId !== packetId || envelope.status !== "ready") fail(`${packetId} is not ready`);
    const packetPath = path.join(ROOT, envelope.packet.path);
    if (sha256File(packetPath) !== envelope.packet.sha256) fail(`${packetId} packet digest is stale`);
    console.log(`slice readiness: ${packetId} ready; authoritative=false`);
};

const runSelfTest = () => {
    const valid = {
        schemaVersion: 1,
        packetId: "fixture",
        status: "design",
        trustedBase: "0".repeat(40),
        packet: { path: "docs/fixture.md", sha256: "0".repeat(64) },
        predecessors: [],
        allowedPaths: ["docs/fixture.md"],
        forbiddenPrefixes: ["crates"],
        proof: { commands: ["true"], evidencePath: "ratchets/fixture.json" },
    };
    validateEnvelope(valid);
    for (const mutate of [
        (value) => ({ ...value, trustedBase: "../unsafe" }),
        (value) => ({ ...value, allowedPaths: ["/absolute"] }),
        (value) => ({ ...value, packet: { ...value.packet, sha256: "not-a-digest" } }),
    ]) {
        let rejected = false;
        try {
            validateEnvelope(mutate(valid));
        } catch {
            rejected = true;
        }
        if (!rejected) fail("self-test accepted an invalid readiness fixture");
    }
    console.log("slice readiness: self-test passed");
};

const main = () => {
    const [command, value] = process.argv.slice(2);
    if (command === "--schema-check") {
        validateSchemaDocument();
        console.log("slice readiness: schema v1 valid");
        return;
    }
    if (command === "--test") {
        validateSchemaDocument();
        runSelfTest();
        return;
    }
    if (command === "--bootstrap-check") {
        validateSchemaDocument();
        validateBootstrap();
        console.log("slice readiness: bootstrap v1 valid");
        return;
    }
    if (command === "--check" && value) {
        validateSchemaDocument();
        checkPacket(value);
        return;
    }
    fail("usage: --schema-check | --test | --bootstrap-check | --check <packet-id>");
};

try {
    main();
} catch (error) {
    console.error(error.message);
    process.exitCode = 1;
}
