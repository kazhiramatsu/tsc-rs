import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { validateJsonSchemaSubset } from "../../.github/ci/qualification.mjs";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const MODE = process.argv[2];

const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-7a-close.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-7a-close.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-7a-close.schema.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-5g-profile.v1.json";
const OWNER_INVENTORY_RELATIVE_PATH =
  "ratchets/h2-7a-owner-inventory.v1.json";
const WITNESSES_RELATIVE_PATH = "ratchets/h2-7a-witnesses.v1.json";
const PROBE_TRACES_RELATIVE_PATH = "ratchets/h2-7a-probe-traces.v1.json";
const PRINTER_REPRINT_RELATIVE_PATH =
  "ratchets/h2-7a-printer-reprint.v1.json";
const CANDIDATE_DISPOSITIONS_RELATIVE_PATH =
  "ratchets/h2-candidate-dispositions.v1.json";
const H2_6C_CLOSE_CASES_ROLL_SHA256 =
  "ed0036eb9d22227c3fba7980d852509a6aba42566c9a39329c761d1b4c61a79b";
const PLAN_RELATIVE_PATH = "crates/emitter/src/plan.rs";
const EXPECTED_PLAN_SHA256 =
  "ac36b2be2a480b0fce8b25a3983a9ec4e0c63661f138b45fcf31db1124c4fa3e";
const EXECUTE_RELATIVE_PATH = "crates/emitter/src/execute.rs";
const EXPECTED_EXECUTE_SHA256 =
  "e4b1e42504e5af8fc49433b4e2bf2ee3255f6e40e264e38e7743f01170f7a1ae";
const ACTIVITY_RELATIVE_PATH = "crates/emitter/src/activity.rs";
const EXPECTED_ACTIVITY_SHA256 =
  "3542317425e47a3fa90d657d419008feb9eb32864144c7356c00b54fd35a09b3";
const PRINTER_RELATIVE_PATH = "crates/emitter/src/printer.rs";
const PRINTER_REFUSAL_SPAN =
  "PrintRequest::Declaration(_) => Err(PrinterError::Unsupported(\n                UnsupportedEmitFeature::Declaration,\n            ))";

const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_TYPESCRIPT = "6.0.3";
const ARTIFACT_KIND = "h2-7a-close";
const ARTIFACT_STATUS = "qualified-typescript-oracle";
const PHASE = "H2.7a-close";
const ACTIVE_RUNTIME_SLICES = Object.freeze([
  "H2.1a",
  "H2.1b",
  "H2.1c",
  "H2.1d",
  "H2.1e",
  "H2.2a",
  "H2.2b",
  "H2.2c",
  "H2.2d",
  "H2.3a",
  "H2.3b",
  "H2.3c",
  "H2.3d",
  "H2.4a",
  "H2.4b",
  "H2.5a",
  "H2.5b",
  "H2.5c",
  "H2.5d",
  "H2.5e",
  "H2.5f",
  "H2.5g",
  "H2.5h",
  "H2.6a",
  "H2.6b",
  "H2.6c",
]);
const EXCLUDED_CASE_IDS = Object.freeze([
  "h2-7a/F6/references-first",
  "h2-7a/S2/entityname-1",
  "h2-7a/S3/typeofexpr-1",
  "h2-7a/S2/latebound-1",
]);
const TOP_LEVEL_KEYS = Object.freeze([
  "schema",
  "kind",
  "status",
  "phase",
  "typescript",
  "generator",
  "contract",
  "inputs",
  "runtime_contract",
  "candidate_band",
  "owner_closure",
  "evidence",
  "refusal_surfaces",
  "transition_landing",
  "summary",
  "close_fingerprint_sha256",
]);

const EXPECTED_OWNER_CLOSURE = Object.freeze({
  unresolved_candidates: 0,
  total_rows: 1039,
  surface_rows: Object.freeze({
    declarations_module: 184,
    selection_seam: 2,
    node_builder: 334,
    syntactic_builder: 139,
    resolver_declaration_subset: 54,
    printer_subgraph: 184,
    factory_parenthesizer: 124,
    option_owner_closure: 18,
  }),
  declarations_module: Object.freeze({
    function_rows: 68,
    owner_rows: 1,
    nested_function_rows: 60,
    sibling_function_rows: 7,
  }),
  node_builder_function_rows: 149,
  syntactic_builder_function_rows: 54,
  resolver: Object.freeze({
    consumed_members: 19,
    member_rows: 20,
    declarations_module_call_sites: 28,
    orchestration_use_sites: 6,
  }),
  printer: Object.freeze({
    function_rows: 370,
    seed_workers: 58,
    type_dispatch_workers: 24,
    subgraph_rows: 184,
  }),
  factory_parenthesizer: Object.freeze({
    factory_members: 112,
    factory_calls: 477,
    parenthesizer_members: 12,
    parenthesizer_calls: 14,
  }),
  audit: Object.freeze({
    already_exact: 308,
    foundation_needed: 0,
    pending: 0,
  }),
  m4_anchors: Object.freeze({ rows: 68, anchored: 68, header_verified: 68 }),
  partition: Object.freeze({ m_3_head: 7, m_2: 12 }),
  reached_rows: 386,
});

const EXPECTED_ELIGIBLE_DOMAIN = Object.freeze({
  excluded_case_ids: EXCLUDED_CASE_IDS,
  eligible_cases: 116,
  transform_seeds: 202,
  decl_blocked: 202,
  transform_top_level_declaration_changed: 742,
  visit_declaration_subtree_changed: 496,
  track_symbol: 533,
  report_inference_fallback: 362,
  report_inaccessible_unique_symbol_error: 1,
  report_likely_unsafe_import_required_error: 1,
});

function fail(message) {
  throw new Error("h2-7a-close: " + message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function canonical(value) {
  if (Array.isArray(value)) return "[" + value.map(canonical).join(",") + "]";
  if (value !== null && typeof value === "object") {
    return (
      "{" +
      Object.keys(value)
        .sort()
        .map((key) => JSON.stringify(key) + ":" + canonical(value[key]))
        .join(",") +
      "}"
    );
  }
  return JSON.stringify(value);
}

function pythonJsonString(value) {
  let result = '"';
  for (let index = 0; index < value.length; index += 1) {
    const codePoint = value.codePointAt(index);
    if (codePoint > 0xffff) index += 1;
    if (codePoint === 0x08) result += "\\b";
    else if (codePoint === 0x09) result += "\\t";
    else if (codePoint === 0x0a) result += "\\n";
    else if (codePoint === 0x0c) result += "\\f";
    else if (codePoint === 0x0d) result += "\\r";
    else if (codePoint === 0x22) result += '\\"';
    else if (codePoint === 0x5c) result += "\\\\";
    else if (codePoint < 0x20 || codePoint > 0x7f) {
      if (codePoint <= 0xffff) {
        result += "\\u" + codePoint.toString(16).padStart(4, "0");
      } else {
        const adjusted = codePoint - 0x10000;
        const high = 0xd800 + (adjusted >> 10);
        const low = 0xdc00 + (adjusted & 0x3ff);
        result +=
          "\\u" +
          high.toString(16).padStart(4, "0") +
          "\\u" +
          low.toString(16).padStart(4, "0");
      }
    } else {
      result += String.fromCodePoint(codePoint);
    }
  }
  return result + '"';
}

// Python's json.dumps(..., sort_keys=True, separators=(',', ':')) is the
// frozen H2.6c candidate-roll authority. The cases contain only safe integer
// numbers today; strings are encoded with ensure_ascii=True here so the
// implementation keeps the Python contract if a future case contains Unicode.
function pythonCanonical(value) {
  if (Array.isArray(value)) return "[" + value.map(pythonCanonical).join(",") + "]";
  if (value !== null && typeof value === "object") {
    return (
      "{" +
      Object.keys(value)
        .sort()
        .map((key) => pythonJsonString(key) + ":" + pythonCanonical(value[key]))
        .join(",") +
      "}"
    );
  }
  if (typeof value === "string") return pythonJsonString(value);
  if (typeof value === "number") {
    requireCondition(Number.isFinite(value), "candidate cases contain a non-finite number");
    requireCondition(Number.isSafeInteger(value), "candidate cases contain an unsafe number");
    return String(value);
  }
  return JSON.stringify(value);
}

function withFingerprint(value, field) {
  return {
    ...value,
    [field]: sha256(Buffer.from(canonical(value), "utf8")),
  };
}

function fingerprintIsValid(record, field) {
  if (record === null || typeof record !== "object" || Array.isArray(record)) {
    return false;
  }
  const stored = record[field];
  const payload = { ...record };
  delete payload[field];
  return (
    isSha256(stored) &&
    stored === sha256(Buffer.from(canonical(payload), "utf8"))
  );
}

function render(value) {
  return JSON.stringify(value, null, 2) + "\n";
}

function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function readText(relativePath) {
  return readBytes(relativePath).toString("utf8");
}

function readJson(relativePath) {
  try {
    return JSON.parse(readText(relativePath));
  } catch (error) {
    fail(relativePath + " is not valid JSON: " + error.message);
  }
}

function pathHash(relativePath) {
  return {
    path: relativePath,
    sha256: sha256(readBytes(relativePath)),
  };
}

function isSha256(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function assertPathHash(record, relativePath, label) {
  requireCondition(
    record !== null &&
      typeof record === "object" &&
      record.path === relativePath &&
      isSha256(record.sha256) &&
      record.sha256 === pathHash(relativePath).sha256,
    label + " path/hash pin changed",
  );
}

function assertFingerprint(record, field, label) {
  requireCondition(
    fingerprintIsValid(record, field),
    label + " fingerprint is invalid",
  );
}

function loadParentProfile() {
  const parent = readJson(PARENT_PROFILE_RELATIVE_PATH);
  requireCondition(
    parent.schema === 1 &&
      parent.kind === "h2-runtime-profile" &&
      parent.status === "qualified" &&
      parent.phase === "H2.5g" &&
      parent.transition.completed_slice === "H2.5g" &&
      canonical(parent.transition.active_runtime_slices) ===
        canonical(ACTIVE_RUNTIME_SLICES) &&
      parent.transition.inactive_runtime_slice_count === 11 &&
      parent.transition.next_runtime_activation_slice === "H2.7b" &&
      parent.summary.completed_runtime_slices === 25 &&
      parent.summary.next_slice_runtime_slice_delta === 0 &&
      parent.summary.runtime_admissions === 9_196 &&
      parent.summary.executed_candidates === 9_715 &&
      parent.summary.unexecuted_candidates === 0 &&
      parent.summary.undispositioned_candidates === 0,
    "H2.5g parent profile content is not the closed H2.6c state",
  );
  const preLanding =
    parent.transition.next_slice === "H2.7a" &&
    parent.transition.next_slice_scope ===
      "declaration-owner-inventory-and-dormant-foundation";
  const landed =
    parent.transition.next_slice === "H2.7b" &&
    parent.transition.next_slice_scope === "non-bundle-declaration-output";
  requireCondition(preLanding || landed, "parent transition has an unknown landing state");
  return parent;
}

function loadInputs() {
  const ownerInventory = readJson(OWNER_INVENTORY_RELATIVE_PATH);
  requireCondition(
    ownerInventory.schema === 1 &&
      ownerInventory.status === "measured" &&
      ownerInventory.phase === "H2.7a-owner-inventory",
    "owner inventory identity changed",
  );
  assertFingerprint(
    ownerInventory,
    "inventory_fingerprint_sha256",
    "owner inventory",
  );

  const witnesses = readJson(WITNESSES_RELATIVE_PATH);
  requireCondition(
    witnesses.schema === 1 &&
      witnesses.kind === "h2-7a-public-observable-witnesses" &&
      witnesses.status === "qualified-typescript-oracle" &&
      witnesses.phase === "H2.7a-witnesses" &&
      witnesses.case_manifest !== null &&
      isSha256(witnesses.case_manifest?.case_manifest_fingerprint) &&
      isSha256(witnesses.observation_content_roll_sha256),
    "witness artifact identity or pins changed",
  );
  assertFingerprint(witnesses, "witnesses_fingerprint_sha256", "witnesses");

  const probeTraces = readJson(PROBE_TRACES_RELATIVE_PATH);
  requireCondition(
    probeTraces.schema === 3 &&
      probeTraces.phase === "H2.7a-probe-traces" &&
      isSha256(probeTraces.case_manifest_fingerprint) &&
      probeTraces.case_manifest_fingerprint ===
        witnesses.case_manifest.case_manifest_fingerprint,
    "probe trace identity or case-manifest pin changed",
  );
  assertFingerprint(
    probeTraces,
    "probe_traces_fingerprint_sha256",
    "probe traces",
  );

  const printerReprint = readJson(PRINTER_REPRINT_RELATIVE_PATH);
  requireCondition(
    printerReprint.schema === 1 &&
      printerReprint.kind === "h2-7a-printer-reprint" &&
      printerReprint.status === "qualified-typescript-oracle" &&
      printerReprint.phase === "H2.7a-printer-reprint",
    "printer reprint identity changed",
  );
  assertFingerprint(
    printerReprint,
    "printer_reprint_fingerprint_sha256",
    "printer reprint",
  );

  const candidateDispositions = readJson(CANDIDATE_DISPOSITIONS_RELATIVE_PATH);
  requireCondition(
    candidateDispositions.schema === 1 &&
      candidateDispositions.status === "frozen" &&
      candidateDispositions.phase === "H2.0a-runner-candidate-dispositions" &&
      Array.isArray(candidateDispositions.cases) &&
      candidateDispositions.cases.length === 15_642,
    "candidate dispositions identity or case count changed",
  );

  return {
    ownerInventory,
    witnesses,
    probeTraces,
    printerReprint,
    candidateDispositions,
    inputs: {
      owner_inventory: {
        ...pathHash(OWNER_INVENTORY_RELATIVE_PATH),
        inventory_fingerprint_sha256:
          ownerInventory.inventory_fingerprint_sha256,
      },
      witnesses: {
        ...pathHash(WITNESSES_RELATIVE_PATH),
        witnesses_fingerprint_sha256:
          witnesses.witnesses_fingerprint_sha256,
        observation_content_roll_sha256:
          witnesses.observation_content_roll_sha256,
        case_manifest_fingerprint:
          witnesses.case_manifest.case_manifest_fingerprint,
      },
      probe_traces: {
        ...pathHash(PROBE_TRACES_RELATIVE_PATH),
        probe_traces_fingerprint_sha256:
          probeTraces.probe_traces_fingerprint_sha256,
      },
      printer_reprint: {
        ...pathHash(PRINTER_REPRINT_RELATIVE_PATH),
        printer_reprint_fingerprint_sha256:
          printerReprint.printer_reprint_fingerprint_sha256,
      },
      candidate_dispositions: pathHash(CANDIDATE_DISPOSITIONS_RELATIVE_PATH),
    },
  };
}

function typescriptRecord() {
  return {
    version: EXPECTED_TYPESCRIPT,
    source_commit: SOURCE_COMMIT,
    bundle: pathHash("vendor/typescript-6.0.3/lib/typescript.js"),
    implementation: pathHash("vendor/typescript-6.0.3/lib/_tsc.js"),
  };
}

function ownerClosureFrom(inventory) {
  const summary = inventory.summary;
  const result = {
    unresolved_candidates: summary.unresolved_candidates,
    total_rows: summary.total_rows,
    surface_rows: {
      declarations_module: summary.surface_rows.declarations_module,
      selection_seam: summary.surface_rows.selection_seam,
      node_builder: summary.surface_rows.node_builder,
      syntactic_builder: summary.surface_rows.syntactic_builder,
      resolver_declaration_subset:
        summary.surface_rows.resolver_declaration_subset,
      printer_subgraph: summary.surface_rows.printer_subgraph,
      factory_parenthesizer: summary.surface_rows.factory_parenthesizer,
      option_owner_closure: summary.surface_rows.option_owner_closure,
    },
    declarations_module: {
      function_rows: summary.declarations_module.function_rows,
      owner_rows: summary.declarations_module.owner_rows,
      nested_function_rows: summary.declarations_module.nested_function_rows,
      sibling_function_rows: summary.declarations_module.sibling_function_rows,
    },
    node_builder_function_rows: summary.node_builder.function_rows,
    syntactic_builder_function_rows: summary.syntactic_builder.function_rows,
    resolver: {
      consumed_members: summary.resolver.consumed_members,
      member_rows: summary.resolver.member_rows,
      declarations_module_call_sites: summary.resolver.declarations_module_call_sites,
      orchestration_use_sites: summary.resolver.orchestration_use_sites,
    },
    printer: {
      function_rows: summary.printer.function_rows,
      seed_workers: summary.printer.seed_workers,
      type_dispatch_workers: summary.printer.type_dispatch_workers,
      subgraph_rows: summary.printer.subgraph_rows,
    },
    factory_parenthesizer: {
      factory_members: summary.factory_parenthesizer.factory_members,
      factory_calls: summary.factory_parenthesizer.factory_calls,
      parenthesizer_members: summary.factory_parenthesizer.parenthesizer_members,
      parenthesizer_calls: summary.factory_parenthesizer.parenthesizer_calls,
    },
    audit: {
      already_exact: summary.audit.already_exact,
      foundation_needed: summary.audit.foundation_needed,
      pending: summary.audit.pending,
    },
    m4_anchors: {
      rows: summary.m4_anchors.rows,
      anchored: summary.m4_anchors.anchored,
      header_verified: summary.m4_anchors.header_verified,
    },
    partition: {
      m_3_head: summary.partition.m_3_head,
      m_2: summary.partition.m_2,
    },
    reached_rows: summary.reached_rows,
  };
  requireCondition(
    Array.isArray(inventory.unresolved_candidates) &&
      inventory.unresolved_candidates.length === 0 &&
      canonical(result) === canonical(EXPECTED_OWNER_CLOSURE),
    "owner closure projections changed",
  );
  return result;
}

function candidateBandFrom(candidateDispositions) {
  const cases = candidateDispositions.cases;
  const casesRoll = sha256(Buffer.from(pythonCanonical(cases), "utf8"));
  const h2_7aFirstBlockerRows = cases.filter(
    (row) => row.next_slice === "H2.7a",
  ).length;
  const h2_7aChainRows = cases.filter(
    (row) => Array.isArray(row.required_slices) && row.required_slices.includes("H2.7a"),
  ).length;
  const nextSliceForecast = {
    slice: "H2.7b",
    first_blocker_rows: cases.filter((row) => row.next_slice === "H2.7b").length,
    chain_rows: cases.filter(
      (row) => Array.isArray(row.required_slices) && row.required_slices.includes("H2.7b"),
    ).length,
  };
  requireCondition(
    casesRoll === H2_6C_CLOSE_CASES_ROLL_SHA256,
    "candidate dispositions cases roll changed from the H2.6c close",
  );
  requireCondition(h2_7aFirstBlockerRows === 0, "H2.7a first-blocker band is nonzero");
  requireCondition(h2_7aChainRows === 0, "H2.7a chain-membership band is nonzero");
  return {
    h2_7a_first_blocker_rows: h2_7aFirstBlockerRows,
    h2_7a_chain_rows: h2_7aChainRows,
    cases_roll_sha256: casesRoll,
    h2_6c_close_cases_roll_sha256: H2_6C_CLOSE_CASES_ROLL_SHA256,
    roll_matches_h2_6c_close: casesRoll === H2_6C_CLOSE_CASES_ROLL_SHA256,
    next_slice_forecast: nextSliceForecast,
  };
}

function evidenceFrom(inputs) {
  const witnessSummary = inputs.witnesses.summary;
  const probeSummary = inputs.probeTraces.summary;
  const printerSummary = inputs.printerReprint.summary;
  const evidence = {
    witnesses: {
      cases: witnessSummary.cases,
      typescript_oracle_runs: witnessSummary.typescript_oracle_runs,
      rust_runs: witnessSummary.rust_runs,
      declaration_writes: witnessSummary.declaration_writes,
      deterministic_cases: witnessSummary.deterministic_cases,
      frozen_families: witnessSummary.frozen_families,
      emit_diagnostics: witnessSummary.emit_diagnostics,
    },
    probe_traces: {
      cases: probeSummary.cases,
      events: inputs.probeTraces.cases.reduce(
        (total, current) => total + current.trace_events.length,
        0,
      ),
    },
    printer_reprint: {
      gating_rows: printerSummary.gating_rows,
    },
  };
  requireCondition(
    canonical(evidence.witnesses) ===
      canonical({
        cases: 120,
        typescript_oracle_runs: 240,
        rust_runs: 0,
        declaration_writes: 202,
        deterministic_cases: 120,
        frozen_families: 14,
        emit_diagnostics: 13,
      }) &&
      canonical(evidence.probe_traces) === canonical({ cases: 120, events: 16_925 }) &&
      canonical(evidence.printer_reprint) === canonical({ gating_rows: 1_350 }),
    "standing evidence projections changed",
  );
  return evidence;
}

function eligibleDomainFrom(probeTraces) {
  const excluded = new Set(EXCLUDED_CASE_IDS);
  const counts = new Map();
  let eligibleCases = 0;
  requireCondition(Array.isArray(probeTraces.cases), "probe trace cases are absent");
  for (const current of probeTraces.cases) {
    requireCondition(
      typeof current.case_id === "string" && Array.isArray(current.trace_events),
      "probe trace case shape changed",
    );
    if (excluded.has(current.case_id)) continue;
    eligibleCases += 1;
    for (const event of current.trace_events) {
      requireCondition(typeof event.site_id === "string", "probe trace event site_id is absent");
      counts.set(event.site_id, (counts.get(event.site_id) ?? 0) + 1);
    }
  }
  const result = {
    excluded_case_ids: [...EXCLUDED_CASE_IDS],
    eligible_cases: eligibleCases,
    transform_seeds: counts.get("probe.transformSeed") ?? 0,
    decl_blocked: counts.get("declarations.declBlocked") ?? 0,
    transform_top_level_declaration_changed:
      counts.get("declarations.transformTopLevelDeclaration.changed") ?? 0,
    visit_declaration_subtree_changed:
      counts.get("declarations.visitDeclarationSubtree.changed") ?? 0,
    track_symbol: counts.get("tracker.trackSymbol") ?? 0,
    report_inference_fallback:
      counts.get("tracker.reportInferenceFallback") ?? 0,
    report_inaccessible_unique_symbol_error:
      counts.get("tracker.reportInaccessibleUniqueSymbolError") ?? 0,
    report_likely_unsafe_import_required_error:
      counts.get("tracker.reportLikelyUnsafeImportRequiredError") ?? 0,
  };
  requireCondition(
    probeTraces.cases.length === 120 &&
      canonical(result) === canonical(EXPECTED_ELIGIBLE_DOMAIN),
    "eligible probe-domain projections changed",
  );
  return result;
}

function refusalSurfaces() {
  const sources = [
    [PLAN_RELATIVE_PATH, EXPECTED_PLAN_SHA256],
    [EXECUTE_RELATIVE_PATH, EXPECTED_EXECUTE_SHA256],
    [ACTIVITY_RELATIVE_PATH, EXPECTED_ACTIVITY_SHA256],
  ];
  const result = sources.map(([relativePath, expected]) => {
    const current = sha256(readBytes(relativePath));
    requireCondition(current === expected, relativePath + " left the H2.6c-close baseline");
    return {
      path: relativePath,
      sha256_now: current,
      sha256_at_h2_6c_close: expected,
      retained: true,
    };
  });
  const printerPresent = readText(PRINTER_RELATIVE_PATH).includes(PRINTER_REFUSAL_SPAN);
  requireCondition(printerPresent, "printer declaration refusal span is absent");
  result.push({
    path: PRINTER_RELATIVE_PATH,
    refusal_span: "PrintRequest::Declaration",
    present: printerPresent,
    retained: true,
  });
  return result;
}

function transitionLandingRecord() {
  return {
    next_slice: "H2.7b",
    next_slice_scope: "non-bundle-declaration-output",
    next_runtime_activation_slice: "H2.7b",
    completed_slice: "H2.5g",
    inactive_runtime_slice_count: 11,
    completed_runtime_slices: 25,
    next_slice_runtime_slice_delta: 0,
    runtime_admissions: 9_196,
    executed_candidates: 9_715,
    unexecuted_candidates: 0,
    undispositioned_candidates: 0,
  };
}

function assertParentLanding(parent) {
  requireCondition(
    parent.transition.next_slice === "H2.7b" &&
      parent.transition.next_slice_scope === "non-bundle-declaration-output" &&
      parent.transition.next_runtime_activation_slice === "H2.7b" &&
      parent.transition.completed_slice === "H2.5g" &&
      parent.transition.inactive_runtime_slice_count === 11 &&
      parent.summary.completed_runtime_slices === 25 &&
      parent.summary.next_slice_runtime_slice_delta === 0 &&
      parent.summary.runtime_admissions === 9_196 &&
      parent.summary.executed_candidates === 9_715 &&
      parent.summary.unexecuted_candidates === 0 &&
      parent.summary.undispositioned_candidates === 0,
    "transition_landing: parent has not landed H2.7b",
  );
}

function summaryFrom(candidateBand, eligibleDomain) {
  const result = {
    h2_7a_activity: 0,
    rust_runs: 0,
    runtime_admissions_delta: 0,
    executed_candidates_delta: 0,
    unresolved_owners: 0,
    refusal_surfaces_retained: 4,
    eligible_cases: eligibleDomain.eligible_cases,
    h2_7a_first_blocker_rows: candidateBand.h2_7a_first_blocker_rows,
  };
  requireCondition(
    canonical(result) ===
      canonical({
        h2_7a_activity: 0,
        rust_runs: 0,
        runtime_admissions_delta: 0,
        executed_candidates_delta: 0,
        unresolved_owners: 0,
        refusal_surfaces_retained: 4,
        eligible_cases: 116,
        h2_7a_first_blocker_rows: 0,
      }),
    "close summary projections changed",
  );
  return result;
}

function buildContext() {
  const parent = loadParentProfile();
  const inputState = loadInputs();
  const ownerClosure = ownerClosureFrom(inputState.ownerInventory);
  const candidateBand = candidateBandFrom(inputState.candidateDispositions);
  const evidence = evidenceFrom(inputState);
  const eligibleDomain = eligibleDomainFrom(inputState.probeTraces);
  const refusalSurfaceRecords = refusalSurfaces();
  const typescript = typescriptRecord();
  const context = {
    parent,
    ...inputState,
    typescript,
    ownerClosure,
    candidateBand,
    evidence,
    eligibleDomain,
    refusalSurfaces: refusalSurfaceRecords,
    runtimeContract: {
      foundation_slice_id: "H2.7a",
      runtime_activation_slice_id: "H2.7b",
      production_state: "dormant",
      transformer_registration: "not-registered",
      active_runtime_slices: [...ACTIVE_RUNTIME_SLICES],
      h2_7a_runtime_active: false,
      h2_7a_activity: 0,
      candidate_execution_state: "not-run",
      candidate_typescript_runs: 0,
      rust_runs: 0,
      parent_completed_runtime_slices: 25,
      runtime_admissions_before: 9_196,
      runtime_admissions_after: 9_196,
      runtime_admissions_delta: 0,
      executed_candidates_before: 9_715,
      executed_candidates_after: 9_715,
      executed_candidates_delta: 0,
    },
  };
  context.transitionLanding = transitionLandingRecord();
  context.summary = summaryFrom(candidateBand, eligibleDomain);
  return context;
}

function buildArtifact(context) {
  return withFingerprint(
    {
      schema: 1,
      kind: ARTIFACT_KIND,
      status: ARTIFACT_STATUS,
      phase: PHASE,
      typescript: context.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      inputs: context.inputs,
      runtime_contract: context.runtimeContract,
      candidate_band: context.candidateBand,
      owner_closure: context.ownerClosure,
      evidence: {
        ...context.evidence,
        eligible_domain: context.eligibleDomain,
      },
      refusal_surfaces: context.refusalSurfaces,
      transition_landing: context.transitionLanding,
      summary: context.summary,
    },
    "close_fingerprint_sha256",
  );
}

function loadContract() {
  const contract = readJson(CONTRACT_RELATIVE_PATH);
  requireCondition(
    contract.$schema === "https://json-schema.org/draft/2020-12/schema" &&
      contract.$id ===
        "https://tsc-rs.invalid/contracts/h2-7a-close.schema.json" &&
      contract.type === "object" &&
      contract.additionalProperties === false &&
      Array.isArray(contract.required) &&
      JSON.stringify(contract.required) === JSON.stringify(TOP_LEVEL_KEYS),
    "close schema top-level contract drifted",
  );
  return contract;
}

function validateArtifact(artifact, context, strictParent) {
  const contract = loadContract();
  validateJsonSchemaSubset(contract, artifact, "h2-7a close artifact");
  requireCondition(
    JSON.stringify(Object.keys(artifact)) === JSON.stringify(TOP_LEVEL_KEYS),
    "artifact top-level key order or set changed",
  );
  requireCondition(
    artifact.schema === 1 &&
      artifact.kind === ARTIFACT_KIND &&
      artifact.status === ARTIFACT_STATUS &&
      artifact.phase === PHASE &&
      fingerprintIsValid(artifact, "close_fingerprint_sha256"),
    "artifact identity or fingerprint is invalid",
  );
  requireCondition(
    canonical(artifact.typescript) === canonical(context.typescript),
    "TypeScript record changed",
  );
  assertPathHash(artifact.generator, GENERATOR_RELATIVE_PATH, "generator");
  assertPathHash(artifact.contract, CONTRACT_RELATIVE_PATH, "contract");
  requireCondition(
    canonical(artifact.inputs) === canonical(context.inputs),
    "close input pins changed",
  );
  requireCondition(
    canonical(artifact.runtime_contract) === canonical(context.runtimeContract),
    "runtime contract changed",
  );
  requireCondition(
    canonical(artifact.candidate_band) === canonical(context.candidateBand),
    "candidate band changed",
  );
  requireCondition(
    canonical(artifact.owner_closure) === canonical(context.ownerClosure),
    "owner closure changed",
  );
  requireCondition(
    canonical(artifact.evidence) ===
      canonical({ ...context.evidence, eligible_domain: context.eligibleDomain }),
    "evidence projections changed",
  );
  requireCondition(
    canonical(artifact.refusal_surfaces) === canonical(context.refusalSurfaces),
    "refusal surfaces changed",
  );
  requireCondition(
    canonical(artifact.transition_landing) === canonical(context.transitionLanding),
    "transition landing record changed",
  );
  requireCondition(
    canonical(artifact.summary) === canonical(context.summary),
    "summary changed",
  );
  if (strictParent) assertParentLanding(context.parent);
  return artifact;
}

function compareWholeArtifact(artifact) {
  const targetPath = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
  requireCondition(
    fs.existsSync(targetPath) &&
      fs.readFileSync(targetPath, "utf8") === render(artifact),
    "stale " + TARGET_RELATIVE_PATH + "; run --write and review",
  );
}

function writeFileAtomic(absolutePath, contents) {
  fs.mkdirSync(path.dirname(absolutePath), { recursive: true });
  const temporary = path.join(
    path.dirname(absolutePath),
    "." + path.basename(absolutePath) + ".tmp",
  );
  fs.writeFileSync(temporary, contents);
  fs.renameSync(temporary, absolutePath);
}

function printSelftest(context) {
  const parent = context.parent;
  const lines = [
    "H2.7a close selftest:",
    "inputs:",
    ...Object.entries(context.inputs).map(
      ([name, value]) => "  " + name + "=" + JSON.stringify(value),
    ),
    "candidate_band:",
    "  cases_roll_sha256=" +
      context.candidateBand.cases_roll_sha256 +
      " h2_6c_close=" +
      context.candidateBand.h2_6c_close_cases_roll_sha256 +
      " equal=" +
      context.candidateBand.roll_matches_h2_6c_close,
    "  h2_7a_first_blocker_rows=" +
      context.candidateBand.h2_7a_first_blocker_rows +
      " h2_7a_chain_rows=" +
      context.candidateBand.h2_7a_chain_rows,
    "  next_slice_forecast=" + JSON.stringify(context.candidateBand.next_slice_forecast),
    "eligible_domain=" + JSON.stringify(context.eligibleDomain),
    "source_baselines:",
    ...context.refusalSurfaces.slice(0, 3).map(
      (surface) =>
        "  " +
        surface.path +
        " now=" +
        surface.sha256_now +
        " expected=" +
        surface.sha256_at_h2_6c_close +
        " equal=" +
        (surface.sha256_now === surface.sha256_at_h2_6c_close),
    ),
    "printer_refusal_span=PrintRequest::Declaration present=" +
      context.refusalSurfaces[3].present,
    "owner_closure=" + JSON.stringify(context.ownerClosure),
    "parent_transition=" +
      JSON.stringify({
        completed_slice: parent.transition.completed_slice,
        next_slice: parent.transition.next_slice,
        next_slice_scope: parent.transition.next_slice_scope,
        next_runtime_activation_slice:
          parent.transition.next_runtime_activation_slice,
        active_runtime_slices: parent.transition.active_runtime_slices,
        inactive_runtime_slice_count: parent.transition.inactive_runtime_slice_count,
        completed_runtime_slices: parent.summary.completed_runtime_slices,
        runtime_admissions: parent.summary.runtime_admissions,
        executed_candidates: parent.summary.executed_candidates,
      }),
    parent.transition.next_slice === "H2.7a"
      ? "transition_landing: parent not yet landed (next_slice=H2.7a)"
      : "transition_landing: parent landed (next_slice=H2.7b)",
    "H2.7a close selftest is green",
  ];
  process.stdout.write(lines.join("\n") + "\n");
}

function runSelftest() {
  const context = buildContext();
  const artifact = buildArtifact(context);
  validateArtifact(artifact, context, false);
  printSelftest(context);
}

function runWrite() {
  const context = buildContext();
  assertParentLanding(context.parent);
  const artifact = buildArtifact(context);
  validateArtifact(artifact, context, true);
  writeFileAtomic(path.join(WORKSPACE, TARGET_RELATIVE_PATH), render(artifact));
  const stored = readJson(TARGET_RELATIVE_PATH);
  validateArtifact(stored, context, true);
  compareWholeArtifact(stored);
  process.stdout.write(
    "wrote " + TARGET_RELATIVE_PATH + ": close_fingerprint_sha256=" +
      stored.close_fingerprint_sha256 +
      " h2_7a_first_blocker_rows=" +
      stored.summary.h2_7a_first_blocker_rows +
      " runtime_admissions_delta=" +
      stored.summary.runtime_admissions_delta +
      "\n",
  );
}

function runCheck() {
  const context = buildContext();
  const targetPath = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
  requireCondition(
    fs.existsSync(targetPath),
    "missing " + TARGET_RELATIVE_PATH + "; run --write and review",
  );
  const tracked = readJson(TARGET_RELATIVE_PATH);
  validateArtifact(tracked, context, true);
  const expected = buildArtifact(context);
  validateArtifact(expected, context, true);
  requireCondition(
    fs.readFileSync(targetPath, "utf8") === render(expected),
    "stale " + TARGET_RELATIVE_PATH + "; run --write and review",
  );
  process.stdout.write(
    "H2.7a close is fresh: close_fingerprint_sha256=" +
      tracked.close_fingerprint_sha256 +
      " h2_7a_first_blocker_rows=" +
      tracked.summary.h2_7a_first_blocker_rows +
      "\n",
  );
}

try {
  if (MODE === "--selftest") runSelftest();
  else if (MODE === "--write") runWrite();
  else if (MODE === "--check") runCheck();
  else fail("usage: h2-7a-close.mjs [--selftest|--write|--check]");
} catch (error) {
  process.stderr.write((error.stack ?? error.message ?? String(error)) + "\n");
  process.exitCode = 1;
}
