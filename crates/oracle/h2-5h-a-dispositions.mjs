// H2.5h-a prerequisite-transition step 5 (manifest half): the
// architecture-row disposition manifest. Every row in the current
// emitter architecture map receives exactly one applicability
// disposition from the closed enum; the row inventory is DERIVED from
// the architecture document and compared against the reviewed table, so
// an added, renamed, or removed row makes this manifest go stale
// (undispositioned rows are structurally impossible while the check is
// green). Dispositions reference pinned evidence in the frozen owner
// graph and local-gap matrix. This manifest authorizes no production
// edit and activates nothing.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5h-a-dispositions.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5h-a-dispositions.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-5h-a-dispositions.schema.json";
const OWNER_GRAPH_RELATIVE_PATH = "ratchets/h2-5h-a-owner-graph.v1.json";
const GAP_MATRIX_RELATIVE_PATH = "ratchets/h2-5h-a-gap-matrix.v1.json";
const ARCHITECTURE_DOC_RELATIVE_PATH =
  "docs/design/greenfield/emitter-architecture.md";
const SLICE = "H2.5h-a";

const DISPOSITIONS = Object.freeze([
  "premise-unchanged",
  "modified-requalify",
  "activate",
  "future-owned-fail-closed",
  "proven-unreachable",
]);

// row_id -> [disposition, rationale, {surfaces?, capabilities?}]
const ROW_TABLE = Object.freeze([
  ["E-ENTRY", "premise-unchanged", "the no-emit/emit typed entries are unchanged; widening target admission happens at the transformer-registration guard owned by E-ORDER-H", {}],
  ["E-PROTOCOL", "premise-unchanged", "host/resolver/artifact/sink/outcome ownership boundaries are unchanged; resolver query growth is owned by the E-RESOLVER-* rows", {}],
  ["E-PLAN-SCRIPT", "premise-unchanged", "selection/root/mode/path planning is target-independent", {}],
  ["E-PLAN-FUTURE", "future-owned-fail-closed", "H2.6/H2.7 own future planning products; no H2.5h reachability", {}],
  ["E-OUTPUT-SCRIPT", "premise-unchanged", "the JavaScript write path is unchanged by downlevel output", {}],
  ["E-OUTPUT-FUTURE", "future-owned-fail-closed", "H2.6/H2.7 own future output products; no H2.5h reachability", {}],
  ["E-MAPS", "future-owned-fail-closed", "the owners set source-map ranges on synthesized nodes but range storage already lives in E-METADATA-BASE; map emission stays dormant under its H2.6 owner", { surfaces: ["source-map-apis"] }],
  ["E-ARENA", "premise-unchanged", "generic arena construction covers the owners' 98 factory methods", { surfaces: ["factory-construction"], capabilities: ["node-construction"] }],
  ["E-RESOLVER-IDENTITY-G", "premise-unchanged", "the identity-anchoring rule is unchanged; every new resolver consumer follows it and is enumerated by its implementation packet", {}],
  ["E-CONTEXT", "premise-unchanged", "the lexical environment (start/resume/end, hoisting, suspension) exists and is consumed as-is", { surfaces: ["lexical-environment"], capabilities: ["lexical-environment"] }],
  ["E-SYNTAX-FACTS", "modified-requalify", "template token flags and extended-Unicode identifier facts must persist through clone/incremental reuse for the EA-GAP-FLAGS classifier", { surfaces: ["syntax-guards", "transform-flag-recomputation"] }],
  ["E-METADATA-BASE", "modified-requalify", "the new transforms write comment/source-map ranges and original-node chains onto synthesized nodes; the row extends rather than infers from text", { surfaces: ["transform-flag-recomputation", "source-map-apis"] }],
  ["E-METADATA-G", "premise-unchanged", "the H2.5g typed metadata facts are consumed unchanged", {}],
  ["E-METADATA-G-CLASS", "modified-requalify", "ES2015 class lowering must preserve the PropertyDeclaration->Parameter provenance chain and class_this/assigned_name transport across its wrappers", { surfaces: ["class-lowering-reach"] }],
  ["E-JSX-FACTORY-G", "premise-unchanged", "JSX is lowered upstream of ES2015; the owner census contains zero JSX factory calls", {}],
  ["E-CAPTURE-BASE", "modified-requalify", "converted-loop capture plans extend the existing scoped local plans", { surfaces: ["loop-partition-machinery", "destructuring-module"], capabilities: ["loop-conversion-capture"] }],
  ["E-CAPTURE-CLASS-G", "modified-requalify", "every ES2015 class/wrapper consumer must preserve the three independent ordered lanes", { surfaces: ["class-lowering-reach"] }],
  ["E-CLASS-PENDING-G", "modified-requalify", "class-definition pending effects must survive ES2015 wrapper placement", { surfaces: ["class-lowering-reach"] }],
  ["E-DECORATOR-INITIALIZERS-G", "modified-requalify", "member addInitializer queues must survive ES2015 wrapper placement", { surfaces: ["class-lowering-reach"] }],
  ["E-DECORATOR-CLASS-INITIALIZERS-G", "modified-requalify", "the exactly-once class-decorator finalizer must survive ES2015 wrapper placement", { surfaces: ["class-lowering-reach"] }],
  ["E-DECORATOR-PARAMETER-PROPERTY-G", "modified-requalify", "constructor-local parameter-property materialization must hold in the ES5 target route", { surfaces: ["class-lowering-reach"] }],
  ["E-ORDER-G", "premise-unchanged", "the closed pass order before ES2015 is unchanged", {}],
  ["E-ORDER-H", "activate", "the joint ES2015->Generators registration, hook chains, and pass-order edges become live production structure", { surfaces: ["hook-composition", "yield-star-synthesis"], capabilities: ["pass-registration-boundary", "substitution-notification-hooks"] }],
  ["E-RESOLVER-BASE", "premise-unchanged", "the emit-resolver protocol boundary is unchanged", {}],
  ["E-RESOLVER-CAPTURE-BASE", "modified-requalify", "existing capture queries gain converted-loop consumers", { surfaces: ["resolver-collision-capture-queries"] }],
  ["E-RESOLVER-CAPTURE-H", "activate", "the six owner queries become live (three are declared today as typed fail-closed defaults; answers are gated by the foundation's direct controls)", { surfaces: ["resolver-collision-capture-queries"], capabilities: ["resolver-collision-capture-queries"] }],
  ["E-CHECKER-FACTS-BASE", "premise-unchanged", "the checker-fact transport boundary is unchanged", {}],
  ["E-CHECKER-FACTS-H", "activate", "the loop NodeCheckFlags set must be answered exactly against the frozen direct-control observations", { surfaces: ["resolver-node-check-flags"], capabilities: ["resolver-collision-capture-queries"] }],
  ["E-NAMES-BASE", "modified-requalify", "loop variables and unique names gain new producers", { surfaces: ["name-generation"], capabilities: ["name-generation-deferred"] }],
  ["E-NAMES-CLASS-G", "modified-requalify", "class-name lowering at the ES5 boundary consumes the class-name facts", { surfaces: ["class-lowering-reach"] }],
  ["E-NAMES-H", "activate", "the deferred-resolution vs eager-reservation equivalence argument becomes a live obligation", { surfaces: ["name-generation"], capabilities: ["name-generation-deferred"] }],
  ["E-HELPERS-BASE", "modified-requalify", "four helper texts (extends/values/spreadArray/generator) join the existing read helper", { surfaces: ["helper-factory"], capabilities: ["helper-emission"] }],
  ["E-HELPERS-PROVENANCE-G", "premise-unchanged", "the helper provenance rule is unchanged", {}],
  ["E-HELPERS-H", "activate", "the owner helper graph becomes live", { surfaces: ["helper-factory"], capabilities: ["helper-emission"] }],
  ["E-PRINTER-BASE", "modified-requalify", "downlevel output uses ES5 shapes the printer already prints, but the row's named private context carrier is reshaped into the threaded EmitContext by the comment-scope packets (CS-2 landed the root and core pipeline); its hook-composition, no-checker-dependency, and immutable-planning invariants are preserved and requalify at the CS-6 final validation ref", { surfaces: ["comment-apis"], capabilities: ["comment-scope-threading"] }],
  ["E-PRINTER-G", "modified-requalify", "the H2.5g printer facts are consumed unchanged, but the row's named ExpressionEmissionContext additions are reshaped by the same comment-scope packets and requalify with them", { surfaces: ["comment-apis"], capabilities: ["comment-scope-threading"] }],
  ["E-COMMENTS-G", "premise-unchanged", "the qualified comment projections keep their H2.5g scope", {}],
  ["E-COMMENT-SCOPE-H", "activate", "the threaded comment-scope triple is implemented by comment-scope packets 2-6 before any ES2015/Generators production work; the study and ten-family witnesses are frozen", { surfaces: ["comment-apis"], capabilities: ["comment-scope-threading"] }],
  ["E-COMMENTS-H", "activate", "comment relocation for synthesized wrappers rides the threaded scope", { surfaces: ["comment-apis"], capabilities: ["comment-scope-threading"] }],
  ["E-POSITIONS", "modified-requalify", "PartiallyEmittedExpression and outer-expression wrappers gain new producers whose position preservation must requalify", { surfaces: ["outer-expression-wrappers"] }],
  ["E-STRINGS", "modified-requalify", "createTemplateCooked and tagged-template lowering extend the string-spelling obligations", { surfaces: ["tagged-template-module"], capabilities: ["tagged-template-lowering"] }],
  ["EA-GAP-FLAGS", "activate", "the shared full transform-flag classifier is a named deliverable", { surfaces: ["transform-flag-recomputation"], capabilities: ["transform-flag-recomputation"] }],
  ["EA-GAP-CAPTURE", "activate", "the complete ES2015 capture model (hierarchy, this/arguments/new.target/super, converted loops, catch scopes, captured bindings) is a named deliverable", { surfaces: ["loop-partition-machinery", "resolver-collision-capture-queries"], capabilities: ["loop-conversion-capture", "resolver-collision-capture-queries"] }],
  ["EA-GAP-COMPOSITION", "activate", "the owner graph resolved the SCC question: one joint H2.5h-b runtime slice; hook composition, generated-binding finalization, wrapper provenance, comment transitions, and typed helper IR are named deliverables", { surfaces: ["yield-star-synthesis", "hook-composition"], capabilities: ["generator-state-machine", "destructuring-flattener-es2015"] }],
  ["EA-GAP-MAPS-DECLS", "future-owned-fail-closed", "the pinned graph shows range-setting only; no shared provenance fact forces an H2.5h requalification, so the row stays with H2.6/H2.7", { surfaces: ["source-map-apis"] }],
]);

function fail(message) {
  throw new Error(message);
}
function requireCondition(condition, message) {
  if (!condition) fail(message);
}
function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}
function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}
function withFingerprint(value, field) {
  return { ...value, [field]: sha256(Buffer.from(canonical(value), "utf8")) };
}
function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}
function readText(relativePath) {
  return readBytes(relativePath).toString("utf8");
}
function readJson(relativePath) {
  return JSON.parse(readText(relativePath));
}
function pathHash(relativePath) {
  return { path: relativePath, sha256: sha256(readBytes(relativePath)) };
}

// Derive the complete architecture row inventory from the document:
// table rows `| \`E-...\`` plus gap sections `### \`EA-GAP-...\``.
function deriveArchitectureRows(architectureText) {
  const rows = new Set();
  for (const match of architectureText.matchAll(/^\| `((?:E|EA)-[A-Z0-9-]+)`/gmu)) {
    rows.add(match[1]);
  }
  for (const match of architectureText.matchAll(/^### `(EA-GAP-[A-Z-]+)`/gmu)) {
    rows.add(match[1]);
  }
  return [...rows].sort();
}

function buildManifest() {
  const architectureText = readText(ARCHITECTURE_DOC_RELATIVE_PATH);
  const derivedRows = deriveArchitectureRows(architectureText);
  const tableRows = ROW_TABLE.map(([rowId]) => rowId).sort();
  requireCondition(
    canonical(derivedRows) === canonical(tableRows),
    `architecture row inventory drifted: derived=${derivedRows.length} table=${tableRows.length}; ` +
      `missing=${derivedRows.filter((row) => !tableRows.includes(row)).join(",") || "none"} ` +
      `extra=${tableRows.filter((row) => !derivedRows.includes(row)).join(",") || "none"}`,
  );
  const ownerGraph = readJson(OWNER_GRAPH_RELATIVE_PATH);
  const gapMatrix = readJson(GAP_MATRIX_RELATIVE_PATH);
  requireCondition(
    ownerGraph.kind === "h2-owner-graph" &&
      ownerGraph.status === "frozen-owner-graph" &&
      gapMatrix.kind === "h2-local-gap-matrix" &&
      gapMatrix.status === "frozen-local-gap-matrix",
    "step-2/step-3 lineage is not closed",
  );
  const surfaceIds = new Set(
    ownerGraph.surface_row_assignments.map((surface) => surface.surface_id),
  );
  const capabilityById = new Map(
    gapMatrix.capabilities.map((capability) => [
      capability.capability_id,
      capability,
    ]),
  );
  const rows = ROW_TABLE.map(([rowId, disposition, rationale, evidence], index) => {
    requireCondition(
      DISPOSITIONS.includes(disposition),
      `${rowId} has an unknown disposition ${disposition}`,
    );
    const surfaces = evidence.surfaces ?? [];
    const capabilities = evidence.capabilities ?? [];
    for (const surface of surfaces) {
      requireCondition(
        surfaceIds.has(surface),
        `${rowId} cites unknown owner-graph surface ${surface}`,
      );
    }
    for (const capabilityId of capabilities) {
      const capability = capabilityById.get(capabilityId);
      requireCondition(
        capability !== undefined,
        `${rowId} cites unknown gap-matrix capability ${capabilityId}`,
      );
      requireCondition(
        !(disposition === "premise-unchanged" && capability.state !== "exists"),
        `${rowId} is premise-unchanged but cites non-exists capability ${capabilityId}`,
      );
    }
    return withFingerprint(
      {
        index,
        row_id: rowId,
        disposition,
        rationale,
        surfaces,
        capabilities,
      },
      "row_fingerprint_sha256",
    );
  });
  const counts = Object.fromEntries(DISPOSITIONS.map((name) => [name, 0]));
  for (const row of rows) counts[row.disposition] += 1;
  return withFingerprint(
    {
      schema: 1,
      kind: "h2-architecture-dispositions",
      status: "frozen-architecture-dispositions",
      phase: SLICE,
      slice_id: SLICE,
      plan_step: "prerequisite-transition-step-5-manifest",
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      lineage: {
        owner_graph: pathHash(OWNER_GRAPH_RELATIVE_PATH),
        owner_graph_fingerprint_sha256:
          ownerGraph.owner_graph_fingerprint_sha256,
        gap_matrix: pathHash(GAP_MATRIX_RELATIVE_PATH),
        gap_matrix_fingerprint_sha256:
          gapMatrix.gap_matrix_fingerprint_sha256,
        architecture_map: pathHash(ARCHITECTURE_DOC_RELATIVE_PATH),
        interpretation:
          "prerequisite-transition step 5 (manifest half): every architecture row dispositioned exactly once against the frozen owner graph and local-gap matrix; a row added to or removed from the map makes this manifest stale; it authorizes no production edit and activates nothing",
      },
      rows,
      summary: {
        rows: rows.length,
        premise_unchanged: counts["premise-unchanged"],
        modified_requalify: counts["modified-requalify"],
        activate: counts["activate"],
        future_owned_fail_closed: counts["future-owned-fail-closed"],
        proven_unreachable: counts["proven-unreachable"],
        undispositioned: 0,
        rust_runs: 0,
        runtime_admissions_delta: 0,
      },
    },
    "dispositions_fingerprint_sha256",
  );
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

const mode = process.argv[2];
requireCondition(
  mode === "--write" || mode === "--check",
  "usage: h2-5h-a-dispositions.mjs [--write|--check]",
);
const artifact = buildManifest();
const rendered = render(artifact);
if (mode === "--write") {
  fs.writeFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
  process.stdout.write(
    `wrote ${TARGET_RELATIVE_PATH}: rows=${artifact.summary.rows} activate=${artifact.summary.activate} undispositioned=${artifact.summary.undispositioned}\n`,
  );
} else {
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") ===
        rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-5h-a-dispositions.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5h-a dispositions are fresh: rows=${artifact.summary.rows} activate=${artifact.summary.activate} undispositioned=${artifact.summary.undispositioned}\n`,
  );
}
