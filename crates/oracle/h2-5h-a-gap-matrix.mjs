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
      "joint transformES2015+transformGenerators registration for target below ES2015",
    surfaces: Object.freeze(["hook-composition"]),
    architecture_rows: Object.freeze(["E-ORDER-H", "EA-GAP-COMPOSITION"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins.rs",
        symbol: "es2015::transform_es2015(options, resolver)",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins.rs",
        symbol: "generators::transform_generators(target, resolver)",
      }),
      Object.freeze({
        path: "crates/emitter/src/activity.rs",
        symbol: "pub const fn h2_5h_profile()",
      }),
    ]),
    absences: Object.freeze([]),
    note: "B-5 landed the live registration: the ES5 admission floor, the H2.5h activity profile, and the joint push in the upstream order (_tsc.js:115942-115945), verified end-to-end by the 32-case witness gate",
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
    state: "exists",
    requirement:
      "full postorder transform-flag classification usable for freshly synthesized ES2015/Generators output; EA-GAP-FLAGS bans inheriting old ES2015/Generator/Yield bits through a partial mask",
    surfaces: Object.freeze(["transform-flag-recomputation"]),
    architecture_rows: Object.freeze(["EA-GAP-FLAGS", "E-METADATA-BASE"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/factory.rs",
        symbol: "pub fn propagate_child_flags",
      }),
      Object.freeze({
        path: "crates/emitter/src/factory.rs",
        symbol: "fn classify_created_node_flags",
      }),
      Object.freeze({
        path: "crates/emitter/src/factory.rs",
        symbol: "fn classify_created_token_flags",
      }),
      Object.freeze({
        path: "crates/emitter/tests/unit/factory_classifier/tests.rs",
        symbol: "fn yield_and_generator_rows_classify_and_exclude_at_the_function_boundary",
      }),
    ]),
    absences: Object.freeze([]),
    note: "B-1 landed the EA-GAP-FLAGS postorder classifier: per-created-kind facet tables ported from the owner-called creators, aggregated through propagate_child_flags; the eight TransformFlags facets of the nine-facet qualification surface carry table contracts (the ninth is the resolver-side NodeCheckFlags fact)",
  }),
  Object.freeze({
    capability_id: "substitution-notification-hooks",
    state: "exists",
    requirement:
      "chained onSubstituteNode for both owners and chained onEmitNode for ES2015 (Generators registers substitution only)",
    surfaces: Object.freeze(["hook-composition"]),
    architecture_rows: Object.freeze(["E-ORDER-H"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/transform.rs",
        symbol: "pub fn substitution_factory",
      }),
      Object.freeze({
        path: "crates/emitter/src/transform.rs",
        symbol: "pub fn substitute_node",
      }),
      Object.freeze({
        path: "crates/emitter/src/transform.rs",
        symbol: "pub fn before_emit_node",
      }),
      Object.freeze({
        path: "crates/emitter/src/transform.rs",
        symbol: "pub fn after_emit_node",
      }),
      Object.freeze({
        path: "crates/emitter/tests/unit/hook_chaining/tests.rs",
        symbol: "fn substitution_delegates_previous_first_in_registration_order",
      }),
    ]),
    absences: Object.freeze([]),
    note: "B-1 pinned the chain: previous-first onSubstituteNode delegation with registration order [transformES2015, transformGenerators] is the forward substitution walk, notification is the forward-before/reverse-after wrap with the first-registered transformer outermost, and the order contracts prove the enablement split (ES2015 substitution+notification, Generators substitution only)",
  }),
  Object.freeze({
    capability_id: "helper-emission",
    state: "exists",
    requirement:
      "the five owner helper factories: extends, values, read, spreadArray, generator",
    surfaces: Object.freeze(["helper-factory"]),
    architecture_rows: Object.freeze(["E-HELPERS-BASE", "E-HELPERS-H"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins/helpers.rs",
        symbol: "typescript:read",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/helpers.rs",
        symbol: "typescript:extends",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/helpers.rs",
        symbol: "typescript:values",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/helpers.rs",
        symbol: "typescript:spreadArray",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/helpers.rs",
        symbol: "typescript:generator",
      }),
      Object.freeze({
        path: "crates/emitter/tests/unit/helpers/tests.rs",
        symbol: "fn helper_texts_match_the_vendored_declarations",
      }),
    ]),
    absences: Object.freeze([]),
    note: "B-1 landed the four absent texts byte-pinned to the vendored declarations by the proven typescript:read dedent recipe (ledger d2 hashes + the byte-parity unit suite that proves the recipe against read first); metadata mirrors upstream (extends priority 0, generator priority 6)",
  }),
  Object.freeze({
    capability_id: "name-generation-deferred",
    state: "exists",
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
      Object.freeze({
        path: "crates/emitter/src/builtins/generated_bindings.rs",
        symbol: "fn allocate_loop_variable",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/generated_bindings.rs",
        symbol: "fn allocate_source_numbered_for_node",
      }),
      Object.freeze({
        path: "crates/emitter/tests/unit/generated_bindings/tests.rs",
        symbol: "fn sibling_scopes_reuse_the_loop_variable_spelling",
      }),
    ]),
    absences: Object.freeze([]),
    note: "B-1 completed the eager scoped-reservation model (the TempFlags._i loop-variable preference and the generateNameCached-equivalent node-keyed memo) and recorded the reviewed E-NAMES-H deferred-vs-eager equivalence argument (packet h2-5h-b-b-1.md item 12.3: universe, scope policy, allocation order) with per-policy-arm contracts; empirical closure rides the B-5 byte gate",
  }),
  Object.freeze({
    capability_id: "comment-scope-threading",
    state: "exists",
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
      Object.freeze({
        path: "crates/emitter/src/comment_cursor.rs",
        symbol: "fn claim_declaration_list_sides",
      }),
      Object.freeze({
        path: "crates/emitter/tests/integration/comment_scope_witness_contract.rs",
        symbol: "fn drive_case",
      }),
    ]),
    absences: Object.freeze([]),
    note: "CS-2 landed the threaded scope at the root and core pipeline, CS-3 landed the per-side claim predicates on the expression/list routes, and CS-4 landed the statement/declaration-family migration with the declaration-list writer (claim_declaration_list_sides replaces the planned container claim; the statement-paired claim helper is deleted); CS-5 deleted the five contextless shims and the detached_transitional constructor, leaving file_root as the printer's single zero-scope entry; CS-6 landed the 30-case witness-driven fixture gate (byte parity against the frozen artifact, both removeComments polarities, six transforms) and the permanent zero-contextless workspace audit — the sub-packet is closed, requalified at 6acd5d43, and ES2015/Generators production (H2.5h-b) is unblocked",
  }),
  Object.freeze({
    capability_id: "resolver-collision-capture-queries",
    state: "exists",
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
      Object.freeze({
        path: "crates/emitter/src/resolver.rs",
        symbol: "fn get_referenced_declaration_with_colliding_name",
      }),
      Object.freeze({
        path: "crates/emitter/src/resolver.rs",
        symbol: "fn is_declaration_with_colliding_name",
      }),
      Object.freeze({
        path: "crates/emitter/src/resolver.rs",
        symbol: "fn is_binding_captured_by_node",
      }),
      Object.freeze({
        path: "crates/checker/src/modules.rs",
        symbol: "fn emit_get_referenced_declaration_with_colliding_name",
      }),
      Object.freeze({
        path: "crates/checker/src/emit.rs",
        symbol: "fn is_binding_captured_by_node",
      }),
      Object.freeze({
        path: "crates/checker/tests/unit/emit/tests.rs",
        symbol: "fn resolver_queries_replay_the_foundation_direct_controls",
      }),
    ]),
    absences: Object.freeze([]),
    note: "B-1 landed the collision/capture trio as typed fail-closed trait defaults with production implementations at the checker bridge (CheckerSession in emit.rs delegating to the modules.rs ports; checkNestedBlockScopedBinding's capturedBlockScopeBindings list materialized for isBindingCapturedByNode); all 43 resolver queries recorded by the foundation's three checker+resolver direct controls replay equal through the production bridge",
  }),
  Object.freeze({
    capability_id: "loop-conversion-capture",
    state: "exists",
    requirement:
      "converted-loop extraction with captured block-scoped bindings, out-parameters, this/arguments/new.target capture, and yield* re-emission for generator-containing bodies",
    surfaces: Object.freeze([
      "loop-partition-machinery",
      "yield-star-synthesis",
    ]),
    architecture_rows: Object.freeze(["EA-GAP-CAPTURE", "E-CAPTURE-BASE"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins/es2015.rs",
        symbol: "fn transform_es2015",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/es2015.rs",
        symbol: "fn convert_iteration_statement_body_if_necessary",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/es2015.rs",
        symbol: "fn generate_call_to_converted_loop_snapshot",
      }),
    ]),
    absences: Object.freeze([]),
    note: "B-4 landed the complete owner (171 pinned local functions) as the dormant Es2015Transformer behind transform_es2015 - class lowering lanes, captured this/arguments/new.target, parameters, block-scoped bindings, loop conversion with BOTH pinned yield* synthesis sites (the EmitFlags Iterator producer half), spread, templates, object-literal chunking, for-of both modes, the first production FlattenHost - qualified by 123 byte-equal focused oracle projections through the real [transformES2015, transformGenerators] chain; registration and tagged-template lowering stay with B-5",
  }),
  Object.freeze({
    capability_id: "generator-state-machine",
    state: "exists",
    requirement:
      "the transformGenerators state machine (labels, try/catch protocol, instruction encoding via createGeneratorHelper)",
    surfaces: Object.freeze(["yield-star-synthesis", "helper-factory"]),
    architecture_rows: Object.freeze(["EA-GAP-COMPOSITION", "E-ORDER-H"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins/generators.rs",
        symbol: "fn transform_generators",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/generators.rs",
        symbol: "fn transform_generator_function_body",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/generators.rs",
        symbol: "fn build_statements",
      }),
    ]),
    absences: Object.freeze([]),
    note: "B-3 landed the complete owner (129 pinned local functions) as the dormant GeneratorsTransformer behind transform_generators, qualified by 72 byte-equal focused oracle projections; registration stays with the B-5 runtime flip; the yield-star consumer obligations (EmitFlags Iterator skip + the YieldStar opcode) landed and the producer sites arrive with B-4",
  }),
  Object.freeze({
    capability_id: "destructuring-flattener-es2015",
    state: "exists",
    requirement:
      "the 18-function shared flattener family for binding patterns and destructuring assignments at the ES5 boundary (FlattenLevel All)",
    surfaces: Object.freeze(["destructuring-module"]),
    architecture_rows: Object.freeze(["EA-GAP-COMPOSITION", "E-CAPTURE-BASE"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins/flatten_destructuring.rs",
        symbol: "fn flatten_destructuring_binding",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/flatten_destructuring.rs",
        symbol: "fn flatten_destructuring_assignment",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins.rs",
        symbol: "mod flatten_destructuring;",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/es2018.rs",
        symbol: "flatten_destructuring_assignment",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/es2018.rs",
        symbol: "flatten_destructuring_binding",
      }),
    ]),
    absences: Object.freeze([]),
    note: "B-2 landed the 18-function shared family (both FlattenLevel arms behind the FlattenHost consumer seam) as crates/emitter/src/builtins/flatten_destructuring.rs, qualified by focused oracle projections; the module is dormant until the B-4/B-5 owners consume it; the ObjectRestSpread PRODUCTION path deliberately remains the independent plan-based lowering inside es2018.rs (re-basing it onto the shared family is a byte-identity-gated H2.5h-b-closure concern, B-2 packet section 12.3)",
  }),
  Object.freeze({
    capability_id: "tagged-template-lowering",
    state: "exists",
    requirement:
      "processTaggedTemplateExpression + createTemplateCooked for target below ES2015",
    surfaces: Object.freeze(["tagged-template-module"]),
    architecture_rows: Object.freeze(["EA-GAP-COMPOSITION", "E-STRINGS"]),
    anchors: Object.freeze([
      Object.freeze({
        path: "crates/emitter/src/builtins/tagged_template.rs",
        symbol: "pub(super) fn process_tagged_template_expression",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/tagged_template.rs",
        symbol: "fn create_template_cooked",
      }),
      Object.freeze({
        path: "crates/emitter/src/builtins/helpers.rs",
        symbol: "pub(super) fn make_template_object()",
      }),
    ]),
    absences: Object.freeze([]),
    note: "B-5 landed the shared module (ProcessLevel::All consumer in the ES2015 owner; the invalid-escape recomputation reads raw fragment bytes) with the __makeTemplateObject helper text byte-pinned; the es2018 LiftRestriction consumer stays the corpus-adoption deferral",
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
