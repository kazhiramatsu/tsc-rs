// H2.5h-a prerequisite-transition step 3: the current Rust local-gap
// matrix. Reviewed capability rows verified mechanically against both
// sides: every requirement surface must exist in the frozen owner graph,
// every architecture row in the architecture map, every positive Rust
// anchor must be present (file pinned by hash), and every negative anchor
// must be absent - so landing an implementation makes this matrix go
// stale and forces a reviewed re-disposition. No production code is
// edited or executed.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5h-a-gap-matrix.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5h-a-gap-matrix.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-5h-a-gap-matrix.schema.json";
const OWNER_GRAPH_RELATIVE_PATH = "ratchets/h2-5h-a-owner-graph.v1.json";
const ARCHITECTURE_DOC_RELATIVE_PATH =
  "docs/design/greenfield/emitter-architecture.md";
const SLICE = "H2.5h-a";

// state: exists = the capability is present and H2.5h consumes it as-is
// (modified-requalify vs premise-unchanged is the step-5 disposition);
// partial = a related mechanism exists but does not cover the tsc
// requirement; missing = no Rust counterpart exists today.
const CAPABILITY_ROWS = Object.freeze([
  Object.freeze({
    capability_id: "pass-registration-boundary",
    state: "exists",
    requirement:
      "joint transformES2015+transformGenerators registration for target below ES2015; today the boundary is a typed fail-closed rejection",
    surfaces: Object.freeze(["hook-composition"]),
    architecture_rows: Object.freeze(["E-ORDER-H", "EA-GAP-COMPOSITION"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins.rs",
        symbol: "older targets belong to later target-ladder slices",
      }),
    ]),
    absences: Object.freeze([]),
    note: "the dormant seam this packet eventually activates; activation stays with H2.5h-b+",
  }),
  Object.freeze({
    capability_id: "lexical-environment",
    state: "exists",
    requirement:
      "start/resume/end lexical environment with hoisted variable/function declarations and suspension, as both owners destructure from the context",
    surfaces: Object.freeze(["lexical-environment"]),
    architecture_rows: Object.freeze(["E-CONTEXT"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/transform.rs",
        symbol: "pub struct LexicalEnvironment",
      }),
      Object.freeze({
        path: "crates/emitter/src/transform.rs",
        symbol: "lexical_environment_stack",
      }),
      Object.freeze({
        path: "crates/emitter/src/transform.rs",
        symbol: "lexical_environment_suspended",
      }),
    ]),
    absences: Object.freeze([]),
    note: null,
  }),
  Object.freeze({
    capability_id: "node-construction",
    state: "exists",
    requirement:
      "synthetic node construction for the 98 distinct factory methods the owners call; the Rust model is generic arena construction, so the mapping is per capability, not per tsc factory name",
    surfaces: Object.freeze(["factory-construction"]),
    architecture_rows: Object.freeze(["E-ARENA"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/factory.rs",
        symbol: "pub fn create_node",
      }),
      Object.freeze({
        path: "crates/emitter/src/factory.rs",
        symbol: "pub fn create_node_array",
      }),
    ]),
    absences: Object.freeze([]),
    note: null,
  }),
  Object.freeze({
    capability_id: "transform-flag-recomputation",
    state: "partial",
    requirement:
      "full postorder transform-flag classification usable for freshly synthesized ES2015/Generators output; EA-GAP-FLAGS bans inheriting old ES2015/Generator/Yield bits through a partial mask",
    surfaces: Object.freeze(["transform-flag-recomputation"]),
    architecture_rows: Object.freeze(["EA-GAP-FLAGS", "E-METADATA-BASE"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/factory.rs",
        symbol: "pub fn propagate_child_flags",
      }),
    ]),
    absences: Object.freeze([]),
    note: "propagation exists; the shared full classifier for changed nodes is the outstanding EA-GAP-FLAGS deliverable",
  }),
  Object.freeze({
    capability_id: "substitution-notification-hooks",
    state: "partial",
    requirement:
      "chained onSubstituteNode for both owners and chained onEmitNode for ES2015 (Generators registers substitution only)",
    surfaces: Object.freeze(["hook-composition"]),
    architecture_rows: Object.freeze(["E-ORDER-H"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/transform.rs",
        symbol: "pub fn substitution_factory",
      }),
    ]),
    absences: Object.freeze([]),
    note: "substitution machinery exists; ES2015-grade notification chaining parity is dispositioned by the step-5 manifest under E-ORDER-H",
  }),
  Object.freeze({
    capability_id: "helper-emission",
    state: "partial",
    requirement:
      "the five owner helper factories: extends, values, read, spreadArray, generator",
    surfaces: Object.freeze(["helper-factory"]),
    architecture_rows: Object.freeze(["E-HELPERS-BASE", "E-HELPERS-H"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins/helpers.rs",
        symbol: "typescript:read",
      }),
    ]),
    absences: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins/helpers.rs",
        symbol: "typescript:extends",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/helpers.rs",
        symbol: "__extends",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/helpers.rs",
        symbol: "typescript:spreadArray",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/helpers.rs",
        symbol: "typescript:generator",
      }),
    ]),
    note: "read helper exists from the H2.5g ladder; extends/values/spreadArray/generator helper texts are absent (asyncValues/asyncGenerator are different helpers)",
  }),
  Object.freeze({
    capability_id: "name-generation-deferred",
    state: "partial",
    requirement:
      "createUniqueName/createTempVariable/createLoopVariable and getGeneratedNameForNode/getInternalName/getLocalName semantics (deferred resolution at print time)",
    surfaces: Object.freeze(["name-generation"]),
    architecture_rows: Object.freeze(["E-NAMES-BASE", "E-NAMES-H"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins/generated_bindings.rs",
        symbol: "GeneratedBindingScopes",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/generated_bindings.rs",
        symbol: "allocate_temp",
      }),
    ]),
    absences: Object.freeze([]),
    note: "the Rust model is eager scoped reservation, not tsc's deferred resolution; E-NAMES-H owns the equivalence argument the step-5 manifest must disposition",
  }),
  Object.freeze({
    capability_id: "comment-scope-threading",
    state: "partial",
    requirement:
      "the containerPos/containerEnd/declarationListContainerEnd triple as an immutable threaded scope (frozen study + ten-family witnesses)",
    surfaces: Object.freeze(["comment-apis"]),
    architecture_rows: Object.freeze(["E-COMMENT-SCOPE-H", "E-COMMENTS-H"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/comment_cursor.rs",
        symbol: "CommentCursor",
      }),
      Object.freeze({
        path: "crates/emitter/src/printer.rs",
        symbol: "token_owned_comment_phase_prefix",
      }),
      Object.freeze({
        path: "crates/emitter/src/comment_cursor.rs",
        symbol: "struct CommentEmissionScope",
      }),
      Object.freeze({
        path: "crates/emitter/src/comment_cursor.rs",
        symbol: "fn claim_sides",
      }),
      Object.freeze({
        path: "crates/emitter/src/printer.rs",
        symbol: "fn established_container_sides",
      }),
      Object.freeze({
        path: "crates/emitter/src/printer.rs",
        symbol: "struct EmitContext",
      }),
      Object.freeze({
        path: "crates/emitter/src/printer.rs",
        symbol: "fn file_root",
      }),
    ]),
    absences: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/printer.rs",
        symbol: "claim_declaration_list_container",
      }),
      Object.freeze({
        path: "crates/emitter/src/comment_cursor.rs",
        symbol: "claim_declaration_list_container",
      }),
    ]),
    note: "CS-2 landed the threaded scope at the root and core pipeline and CS-3 landed the per-side claim predicates on the expression/list routes; the declaration-list writer, the statement-family flag-aware migration, the contextless-API deletion, and the final audit remain with packets CS-4..CS-6 before any ES2015/Generators production work",
  }),
  Object.freeze({
    capability_id: "resolver-collision-capture-queries",
    state: "partial",
    requirement:
      "the six emit-resolver queries the owners call: getReferencedDeclarationWithCollidingName, isDeclarationWithCollidingName, isArgumentsLocalBinding, isBindingCapturedByNode, getReferencedValueDeclaration, hasNodeCheckFlag(loop set)",
    surfaces: Object.freeze([
      "resolver-collision-capture-queries",
      "resolver-node-check-flags",
    ]),
    architecture_rows: Object.freeze([
      "E-RESOLVER-CAPTURE-H",
      "E-CHECKER-FACTS-H",
    ]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/resolver.rs",
        symbol: "fn is_arguments_local_binding",
      }),
      Object.freeze({
        path: "crates/emitter/src/resolver.rs",
        symbol: "fn get_referenced_value_declaration",
      }),
      Object.freeze({
        path: "crates/emitter/src/resolver.rs",
        symbol: "fn has_node_check_flag",
      }),
    ]),
    absences: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/resolver.rs",
        symbol: "colliding_name",
      }),
      Object.freeze({
        path: "crates/emitter/src/resolver.rs",
        symbol: "binding_captured",
      }),
    ]),
    note: "the EmitResolver trait already declares three of the six (typed fail-closed defaults; is_arguments_local_binding is implemented for async capture); the collision/capture pair is absent and checker-side answer parity for the loop NodeCheckFlags set must be dispositioned against the foundation's six direct controls",
  }),
  Object.freeze({
    capability_id: "loop-conversion-capture",
    state: "missing",
    requirement:
      "converted-loop extraction with captured block-scoped bindings, out-parameters, this/arguments/new.target capture, and yield* re-emission for generator-containing bodies",
    surfaces: Object.freeze([
      "loop-partition-machinery",
      "yield-star-synthesis",
    ]),
    architecture_rows: Object.freeze(["EA-GAP-CAPTURE", "E-CAPTURE-BASE"]),
    anchors: Object.freeze([]),
    absences: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins.rs",
        symbol: "converted_loop",
      }),
    ]),
    note: "the yield* composition edge makes this inseparable from the Generators consumer (SCC evidence)",
  }),
  Object.freeze({
    capability_id: "generator-state-machine",
    state: "missing",
    requirement:
      "the transformGenerators state machine (labels, try/catch protocol, instruction encoding via createGeneratorHelper)",
    surfaces: Object.freeze(["yield-star-synthesis", "helper-factory"]),
    architecture_rows: Object.freeze(["EA-GAP-COMPOSITION", "E-ORDER-H"]),
    anchors: Object.freeze([]),
    absences: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins",
        module: "generators.rs",
      }),
    ]),
    note: null,
  }),
  Object.freeze({
    capability_id: "destructuring-flattener-es2015",
    state: "partial",
    requirement:
      "the 18-function shared flattener family for binding patterns and destructuring assignments at the ES5 boundary (FlattenLevel All)",
    surfaces: Object.freeze(["destructuring-module"]),
    architecture_rows: Object.freeze(["EA-GAP-COMPOSITION", "E-CAPTURE-BASE"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins/es2018.rs",
        symbol: "flatten_destructuring_assignment",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/es2018.rs",
        symbol: "flatten_destructuring_binding",
      }),
    ]),
    absences: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins.rs",
        symbol: "flatten_destructuring",
      }),
    ]),
    note: "the ObjectRestSpread flatten level is ported inside the ES2018 lowering; the shared ES5-level family (FlattenLevel All: binding patterns, array patterns, defaults) and its extraction as a shared module remain outstanding",
  }),
  Object.freeze({
    capability_id: "tagged-template-lowering",
    state: "missing",
    requirement:
      "processTaggedTemplateExpression + createTemplateCooked for target below ES2015",
    surfaces: Object.freeze(["tagged-template-module"]),
    architecture_rows: Object.freeze(["EA-GAP-COMPOSITION", "E-STRINGS"]),
    anchors: Object.freeze([]),
    absences: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins",
        module: "tagged_template.rs",
      }),
    ]),
    note: null,
  }),
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

function buildMatrix() {
  const ownerGraph = readJson(OWNER_GRAPH_RELATIVE_PATH);
  requireCondition(
    ownerGraph.schema === 1 &&
      ownerGraph.kind === "h2-owner-graph" &&
      ownerGraph.status === "frozen-owner-graph" &&
      ownerGraph.phase === SLICE &&
      typeof ownerGraph.owner_graph_fingerprint_sha256 === "string",
    "owner-graph lineage is not closed",
  );
  const surfaceIds = new Set(
    ownerGraph.surface_row_assignments.map((surface) => surface.surface_id),
  );
  const architectureText = readText(ARCHITECTURE_DOC_RELATIVE_PATH);
  const pinnedFiles = new Map();
  const pinFile = (relativePath) => {
    if (!pinnedFiles.has(relativePath)) {
      pinnedFiles.set(relativePath, pathHash(relativePath));
    }
    return pinnedFiles.get(relativePath);
  };
  const rows = CAPABILITY_ROWS.map((row, index) => {
    for (const surface of row.surfaces) {
      requireCondition(
        surfaceIds.has(surface),
        `${row.capability_id} references unknown owner-graph surface ${surface}`,
      );
    }
    for (const architectureRow of row.architecture_rows) {
      requireCondition(
        architectureText.includes("`" + architectureRow + "`"),
        `${row.capability_id} names unknown architecture row ${architectureRow}`,
      );
    }
    const anchors = row.anchors.map((anchor) => {
      const text = readText(anchor.path);
      requireCondition(
        text.includes(anchor.symbol),
        `${row.capability_id} anchor ${anchor.symbol} disappeared from ${anchor.path}`,
      );
      return { ...anchor, file: pinFile(anchor.path) };
    });
    const absences = row.absences.map((absence) => {
      if (absence.module !== undefined) {
        const absolute = path.join(WORKSPACE, absence.path, absence.module);
        requireCondition(
          !fs.existsSync(absolute),
          `${row.capability_id} absence violated: ${absence.path}/${absence.module} now exists - re-disposition this matrix`,
        );
        return { path: absence.path, module: absence.module };
      }
      const text = readText(absence.path);
      requireCondition(
        !text.includes(absence.symbol),
        `${row.capability_id} absence violated: ${absence.symbol} now appears in ${absence.path} - re-disposition this matrix`,
      );
      return { path: absence.path, symbol: absence.symbol, file: pinFile(absence.path) };
    });
    requireCondition(
      (row.state === "missing") === (anchors.length === 0),
      `${row.capability_id} state/anchor mismatch`,
    );
    return withFingerprint(
      {
        index,
        capability_id: row.capability_id,
        state: row.state,
        requirement: row.requirement,
        surfaces: [...row.surfaces],
        architecture_rows: [...row.architecture_rows],
        anchors,
        absences,
        note: row.note,
      },
      "capability_fingerprint_sha256",
    );
  });
  const states = { exists: 0, partial: 0, missing: 0 };
  for (const row of rows) states[row.state] += 1;
  return withFingerprint(
    {
      schema: 1,
      kind: "h2-local-gap-matrix",
      status: "frozen-local-gap-matrix",
      phase: SLICE,
      slice_id: SLICE,
      plan_step: "prerequisite-transition-step-3",
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      lineage: {
        owner_graph: pathHash(OWNER_GRAPH_RELATIVE_PATH),
        owner_graph_fingerprint_sha256:
          readJson(OWNER_GRAPH_RELATIVE_PATH).owner_graph_fingerprint_sha256,
        architecture_map: pathHash(ARCHITECTURE_DOC_RELATIVE_PATH),
        interpretation:
          "prerequisite-transition step 3 records the current Rust capability states against the frozen owner graph; a landed implementation breaks its pinned absence and forces re-disposition; it edits no production code and activates nothing",
      },
      capabilities: rows,
      summary: {
        capabilities: rows.length,
        exists: states.exists,
        partial: states.partial,
        missing: states.missing,
        pinned_rust_files: pinnedFiles.size,
        rust_runs: 0,
        runtime_admissions_delta: 0,
      },
    },
    "gap_matrix_fingerprint_sha256",
  );
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

const mode = process.argv[2];
requireCondition(
  mode === "--write" || mode === "--check",
  "usage: h2-5h-a-gap-matrix.mjs [--write|--check]",
);
const artifact = buildMatrix();
const rendered = render(artifact);
if (mode === "--write") {
  fs.writeFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
  process.stdout.write(
    `wrote ${TARGET_RELATIVE_PATH}: capabilities=${artifact.summary.capabilities} exists=${artifact.summary.exists} partial=${artifact.summary.partial} missing=${artifact.summary.missing}\n`,
  );
} else {
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") ===
        rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-5h-a-gap-matrix.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5h-a local-gap matrix is fresh: capabilities=${artifact.summary.capabilities} exists=${artifact.summary.exists} partial=${artifact.summary.partial} missing=${artifact.summary.missing}\n`,
  );
}
