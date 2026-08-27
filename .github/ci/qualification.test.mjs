import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";

// gate-tax 5 unit battery (pin grammar / normalizer / index / repin /
// receipt terms): importing registers its node:test cases here, so the
// walk tail and the gate structural preflight both run them.
import "./gate-tax-5.test.mjs";

import {
  ARTIFACT_SCHEMA_CONTRACTS,
  classifyPaths,
  loadPolicy,
  pathsDigest,
  qualificationResultHash,
  receiptResultHash,
  sha256,
  validateFailureArtifact,
  validateLaneSelection,
  validateMergeReceipt,
  validatePolicy,
  validateFciReadinessChain,
  validateJsonSchemaSubset,
  validateQualificationResult,
  validateBoundReceipt,
  validateRustOwnerControlBoundaries,
} from "./qualification.mjs";

const HEAD = "1".repeat(40);
const BASE = "2".repeat(40);
const HASH = "a".repeat(64);

function clone(value) {
  return structuredClone(value);
}

const SUBSET_SCHEMA = {
  type: "object",
  additionalProperties: false,
  required: ["kind", "payload", "choice"],
  properties: {
    kind: { const: "sample" },
    payload: { $ref: "#/$defs/payload" },
    choice: { oneOf: [{ const: "left" }, { const: "right" }] },
  },
  allOf: [
    { properties: { choice: { type: "string" } } },
    {
      if: { properties: { kind: { const: "sample" } } },
      then: { properties: { payload: { properties: { count: { minimum: 1 } } } } },
    },
  ],
  $defs: {
    payload: {
      type: "object",
      additionalProperties: false,
      required: ["count", "tags", "name", "optional"],
      properties: {
        count: { type: "integer", minimum: 0, maximum: 3 },
        tags: {
          type: "array",
          minItems: 1,
          maxItems: 2,
          uniqueItems: true,
          items: { enum: ["a", "b"] },
        },
        name: { type: "string", minLength: 1, maxLength: 3, pattern: "^[a-z😀]+$" },
        optional: { type: ["string", "null"] },
      },
    },
  },
};

const SUBSET_VALUE = {
  kind: "sample",
  payload: { count: 2, tags: ["a", "b"], name: "😀a", optional: null },
  choice: "left",
};

const ROOTLESS_CASE_ID =
  "typescript-6.0.3/compiler/jsFileCompilationWithoutJsExtensions.ts#default";

const HOSTED_MODULE_PATHS = [
  "crates/xtask/src/bounded_pipeline.rs",
  "crates/xtask/src/h1_emit_acceptance.rs",
  "crates/xtask/src/h2_1a_acceptance.rs",
  "crates/xtask/src/h2_1b_acceptance.rs",
  "crates/xtask/src/h2_1c_acceptance.rs",
  "crates/xtask/src/h2_1d_acceptance.rs",
  "crates/xtask/src/h2_1e_acceptance.rs",
  "crates/xtask/src/h2_2a_acceptance.rs",
  "crates/xtask/src/h2_2b_acceptance.rs",
  "crates/xtask/src/h2_2c_acceptance.rs",
  "crates/xtask/src/h2_2d_acceptance.rs",
  "crates/xtask/src/h2_3a_acceptance.rs",
  "crates/xtask/src/h2_3b_acceptance.rs",
  "crates/xtask/src/h2_3c_acceptance.rs",
  "crates/xtask/src/h2_3d_acceptance.rs",
];

const OWNER_MODULE_PATHS = [
  "crates/xtask/src/h2_3b_acceptance.rs",
  "crates/xtask/src/h2_3c_acceptance.rs",
  "crates/xtask/src/h2_3d_acceptance.rs",
];

const LOCAL_OWNER_CALLS = [
  "h2_3b_acceptance::run_owner_controls",
  "h2_3c_acceptance::run_owner_controls",
  "h2_3d_acceptance::run_owner_controls",
  "h2_3d_acceptance::run_h2_4a_owner_controls",
  "h2_3d_acceptance::run_h2_4b_owner_controls",
  "h2_3d_acceptance::run_h2_5a_owner_controls",
  "h2_3d_acceptance::run_h2_5b_owner_controls",
  "h2_3d_acceptance::run_h2_5c_owner_controls",
  "h2_3d_acceptance::run_h2_5d_owner_controls",
  "h2_3d_acceptance::run_h2_5e_owner_controls",
  "h2_3d_acceptance::run_h2_5f_owner_controls",
  "h2_3d_acceptance::run_h2_5g_owner_controls",
];

function rustSourceSha256(xtaskSource, moduleSources) {
  return Object.fromEntries([
    ["crates/xtask/src/main.rs", sha256(xtaskSource)],
    ...Object.entries(moduleSources).map(([sourcePath, source]) => [sourcePath, sha256(source)]),
  ]);
}

function repinRustFixture(fixture) {
  fixture.sourceSha256 = rustSourceSha256(fixture.xtaskSource, fixture.moduleSources);
  return fixture;
}

function rustOwnerBoundaryFixture() {
  const ownerStatements = LOCAL_OWNER_CALLS.map((callee, index) =>
    index + 1 === LOCAL_OWNER_CALLS.length
      ? `    ${callee}(workspace)`
      : `    ${callee}(workspace)?;`,
  ).join("\n");
  const hostedStatements = [
    ...HOSTED_MODULE_PATHS.filter((modulePath) => !modulePath.endsWith("/bounded_pipeline.rs")).map((modulePath) =>
      `${modulePath.slice(modulePath.lastIndexOf("/") + 1, -3)}::run(&workspace)?;`,
    ),
    "h2_2c_acceptance::run_h2_4a(&workspace)?;",
    "h2_2c_acceptance::run_h2_4b(&workspace)?;",
    "h2_2c_acceptance::run_h2_5a(&workspace)?;",
    "h2_2c_acceptance::run_h2_5b(&workspace)?;",
    "h2_2c_acceptance::run_h2_5c(&workspace)?;",
    "h2_2c_acceptance::run_h2_5d(&workspace)?;",
    "h2_2c_acceptance::run_h2_5e(&workspace)?;",
    "h2_2c_acceptance::run_h2_5f(&workspace)?;",
    "h2_2c_acceptance::run_h2_5g(&workspace)?;",
    "h2_2c_acceptance::run_h2_5h(&workspace)?;",
    "h2_2c_acceptance::run_h2_6a(&workspace)",
  ].map((statement) => `    ${statement}`).join("\n");
  const moduleDeclarations = HOSTED_MODULE_PATHS.map((modulePath) =>
    `mod ${modulePath.slice(modulePath.lastIndexOf("/") + 1, -3)};`,
  ).join("\n");
  const xtaskSource = `
${moduleDeclarations}

fn acceptance(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    if let Some(argument) = args.next() {
        return Err(format!(r#"run_owner_controls {argument}"#).into());
    }
    conformance(std::iter::empty())?;
    let workspace = find_workspace_root()?;
    // h2_3d_acceptance::run_h2_5g_owner_controls(&workspace)?;
${hostedStatements}
}

fn ci_h2_owner_controls(workspace: &Path) -> Result<(), Box<dyn Error>> {
${ownerStatements}
}
`;
  const moduleSources = Object.fromEntries(
    HOSTED_MODULE_PATHS.map((modulePath) => {
      const ownerFunction = OWNER_MODULE_PATHS.includes(modulePath)
        ? `
const OWNER_CONTROLS_RELATIVE_PATH: &str = "owner.json";
pub fn run_owner_controls(workspace: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}
`
        : "";
      const targetFunctions = modulePath.endsWith("/h2_2c_acceptance.rs")
        ? ["run_h2_4a", "run_h2_4b", "run_h2_5a", "run_h2_5b", "run_h2_5c", "run_h2_5d", "run_h2_5e", "run_h2_5f", "run_h2_5g", "run_h2_5h", "run_h2_6a"]
          .map((functionName) => `
pub fn ${functionName}(workspace: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}
`)
          .join("")
        : "";
      return [
        modulePath,
        `
pub fn run(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let decoy = "OWNER_CONTROLS_RELATIVE_PATH run_owner_controls(workspace)";
    /* run_owner_controls(workspace)?; */
    Ok(())
}
${ownerFunction}${targetFunctions}`,
      ];
    }),
  );
  return repinRustFixture({ xtaskSource, moduleSources });
}

test("artifact-to-schema mapping is fixed and immutable", () => {
  assert.equal(Object.isFrozen(ARTIFACT_SCHEMA_CONTRACTS), true);
  assert.deepEqual(
    ARTIFACT_SCHEMA_CONTRACTS.map(({ schema, artifact }) => [schema, artifact]),
    [
      [
        ".github/ci/contracts/h2-5g-qualification.schema.json",
        "ratchets/h2-5g-qualification.v1.json",
      ],
      [
        ".github/ci/contracts/h2-5g-owner-controls.schema.json",
        "ratchets/h2-5g-owner-controls.v1.json",
      ],
      [
        ".github/ci/contracts/h2-5g-profile.schema.json",
        "ratchets/h2-5g-profile.v1.json",
      ],
      [
        ".github/ci/contracts/h2-5h-qualification.schema.json",
        "ratchets/h2-5h-qualification.v1.json",
      ],
      [
        ".github/ci/contracts/h2-6a-qualification.schema.json",
        "ratchets/h2-6a-qualification.v1.json",
      ],
      [
        ".github/ci/contracts/h2-5h-a-foundation.schema.json",
        "ratchets/h2-5h-a-foundation.v1.json",
      ],
      [
        ".github/ci/contracts/h2-5h-a-comment-scope-witnesses.schema.json",
        "ratchets/h2-5h-a-comment-scope-witnesses.v1.json",
      ],
      [
        ".github/ci/contracts/h2-5h-a-owner-graph.schema.json",
        "ratchets/h2-5h-a-owner-graph.v1.json",
      ],
      [
        ".github/ci/contracts/h2-5h-a-gap-matrix.schema.json",
        "ratchets/h2-5h-a-gap-matrix.v1.json",
      ],
      [
        ".github/ci/contracts/h2-5h-a-dispositions.schema.json",
        "ratchets/h2-5h-a-dispositions.v1.json",
      ],
      [
        ".github/ci/contracts/h2-5h-a-es2015-generators-witnesses.schema.json",
        "ratchets/h2-5h-a-es2015-generators-witnesses.v1.json",
      ],
      [
        ".github/ci/contracts/h2-6a-witnesses.schema.json",
        "ratchets/h2-6a-witnesses.v1.json",
      ],
      [
        ".github/ci/contracts/h2-6b-witnesses.schema.json",
        "ratchets/h2-6b-witnesses.v1.json",
      ],
      [
        ".github/ci/contracts/h2-6b-qualification.schema.json",
        "ratchets/h2-6b-qualification.v1.json",
      ],
    ],
  );
});

test("FCI readiness chain is valid and every ready packet passes the checker", () => {
  const summary = validateFciReadinessChain();
  assert.ok(summary.envelopes >= 18, "all packet envelopes are enumerated");
  assert.ok(summary.ready >= 1, "at least one packet is machine-checked ready");
  assert.ok(summary.ready <= summary.envelopes);
});

test("slice-readiness schema rejects malformed envelopes fail-closed", () => {
  const schema = JSON.parse(
    fs.readFileSync(new URL("./contracts/slice-readiness.v1.schema.json", import.meta.url), "utf8"),
  );
  const valid = {
    schemaVersion: 1,
    packetId: "fci-0x",
    status: "design",
    trustedBase: "a".repeat(40),
    packet: { path: "docs/x.md", sha256: "b".repeat(64) },
    predecessors: [],
    allowedPaths: ["docs/x.md"],
    forbiddenPrefixes: ["crates"],
    proof: { commands: ["true"], evidencePath: "ratchets/x.json" },
  };
  assert.equal(validateJsonSchemaSubset(schema, valid, "fixture envelope"), valid);
  assert.throws(
    () => validateJsonSchemaSubset(schema, { ...valid, status: "shipped" }, "fixture envelope"),
    /fixture envelope/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset(schema, { ...valid, blessed: true }, "fixture envelope"),
    /fixture envelope/u,
  );
  assert.throws(
    () =>
      validateJsonSchemaSubset(
        schema,
        { ...valid, trustedBase: "not-a-commit" },
        "fixture envelope",
      ),
    /fixture envelope/u,
  );
});

test("JSON schema subset validates local refs and every H2.5g keyword family", () => {
  assert.equal(validateJsonSchemaSubset(SUBSET_SCHEMA, SUBSET_VALUE, "subset fixture"), SUBSET_VALUE);
  const reordered = { nested: [1, { value: true }], name: "fixture" };
  assert.equal(
    validateJsonSchemaSubset(
      { const: { name: "fixture", nested: [1, { value: true }] } },
      reordered,
      "deep const fixture",
    ),
    reordered,
  );
  assert.equal(
    validateJsonSchemaSubset(
      {
        $schema: "https://json-schema.org/draft/2020-12/schema",
        $id: "https://example.invalid/root.schema.json",
        type: "null",
      },
      null,
    ),
    null,
  );
});

test("H2.5g rootless exception is exact and cannot take the nonempty branch", () => {
  const qualificationSchema = JSON.parse(
    fs.readFileSync(new URL("./contracts/h2-5g-qualification.schema.json", import.meta.url), "utf8"),
  );
  const caseRules = qualificationSchema.$defs.case.allOf;
  const exactEmpty = caseRules.find((rule) => rule.if?.properties?.case_id?.const === ROOTLESS_CASE_ID);
  const emptyOrNonempty = caseRules.find((rule) => Array.isArray(rule.oneOf));
  assert.ok(exactEmpty);
  assert.ok(emptyOrNonempty);
  const schema = { allOf: [exactEmpty, emptyOrNonempty] };
  const special = { case_id: ROOTLESS_CASE_ID, input: { roots: [] }, files: [] };
  const ordinary = { case_id: "typescript-6.0.3/compiler/ordinary.ts#default", input: { roots: ["ordinary.ts"] }, files: [{}] };
  assert.equal(validateJsonSchemaSubset(schema, special), special);
  assert.equal(validateJsonSchemaSubset(schema, ordinary), ordinary);

  const specialWithRoot = clone(special);
  specialWithRoot.input.roots.push("decoy.ts");
  assert.throws(() => validateJsonSchemaSubset(schema, specialWithRoot), /maxItems/u);
  const specialWithFile = clone(special);
  specialWithFile.files.push({});
  assert.throws(() => validateJsonSchemaSubset(schema, specialWithFile), /maxItems/u);
  const ordinaryWithoutInputs = clone(ordinary);
  ordinaryWithoutInputs.input.roots = [];
  ordinaryWithoutInputs.files = [];
  assert.throws(() => validateJsonSchemaSubset(schema, ordinaryWithoutInputs), /oneOf matched 0/u);
});

test("JSON schema subset rejects invalid artifact values fail-closed", () => {
  const mutations = [
    ["missing required property", (value) => delete value.payload.name],
    ["additional property", (value) => { value.extra = true; }],
    ["const", (value) => { value.kind = "other"; }],
    ["union type", (value) => { value.payload.optional = 1; }],
    ["integer type", (value) => { value.payload.count = 1.5; }],
    ["minimum through if/then", (value) => { value.payload.count = 0; }],
    ["maximum", (value) => { value.payload.count = 4; }],
    ["minItems", (value) => { value.payload.tags = []; }],
    ["maxItems", (value) => { value.payload.tags = ["a", "b", "a"]; }],
    ["uniqueItems", (value) => { value.payload.tags = ["a", "a"]; }],
    ["enum", (value) => { value.payload.tags = ["c"]; }],
    ["pattern", (value) => { value.payload.name = "A"; }],
    ["maxLength", (value) => { value.payload.name = "abcd"; }],
    ["oneOf", (value) => { value.choice = "middle"; }],
  ];
  for (const [name, mutate] of mutations) {
    const value = clone(SUBSET_VALUE);
    mutate(value);
    assert.throws(
      () => validateJsonSchemaSubset(SUBSET_SCHEMA, value, name),
      /violates JSON schema/u,
      name,
    );
  }
  assert.throws(
    () => validateJsonSchemaSubset({ type: "array", uniqueItems: true }, [{ a: 1 }, { a: 1 }]),
    /uniqueItems/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({ oneOf: [{ type: "integer" }, { type: "integer" }] }, 1),
    /matched 2 branches/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({ allOf: [{ type: "integer" }, { minimum: 2 }] }, 1),
    /minimum/u,
  );
});

test("JSON schema subset rejects unsafe or unsupported schemas", () => {
  assert.throws(
    () => validateJsonSchemaSubset({ $ref: "https://example.invalid/schema.json" }, {}),
    /local JSON Pointer/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({ $ref: "#/$defs/missing", $defs: {} }, {}),
    /unresolved local/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({ $ref: "#/$defs/loop", $defs: { loop: { $ref: "#/$defs/loop" } } }, {}),
    /cyclic local/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({ type: "string", pattern: "[" }, "value"),
    /invalid pattern/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({ type: "integer", not: { const: 0 } }, 1),
    /unsupported keyword not/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({ if: { const: true } }, true),
    /if and then must appear together/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({ properties: { nested: { $id: "nested" } } }, {}),
    /\$id is supported only on the root schema/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({ $schema: "http://json-schema.org/draft-07/schema#" }, {}),
    /\$schema must select JSON Schema draft 2020-12/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({ $defs: { nested: { $schema: "nested" } } }, {}),
    /\$schema is supported only on the root schema/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({ $ref: "#/$defs/%64ecoy", $defs: { decoy: {} } }, {}),
    /percent-encoding is not supported/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({ const: { nested: [Number.NaN] } }, { nested: [null] }),
    /non-finite numbers are not JSON-compatible/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({ enum: [{ nested: Number.POSITIVE_INFINITY }] }, {}),
    /non-finite numbers are not JSON-compatible/u,
  );
  assert.throws(
    () => validateJsonSchemaSubset({}, { nested: [Number.NEGATIVE_INFINITY] }),
    /instance \/nested\/0.*non-finite numbers/u,
  );
});

test("Rust owner-control boundary ignores comments and string literals", () => {
  const fixture = rustOwnerBoundaryFixture();
  assert.equal(validateRustOwnerControlBoundaries(fixture), true);

  const hostedStringDecoy = rustOwnerBoundaryFixture();
  hostedStringDecoy.xtaskSource = hostedStringDecoy.xtaskSource.replace(
    "    h2_2c_acceptance::run_h2_5g(&workspace)",
    '    let missing_runner = "h2_2c_acceptance::run_h2_5g(&workspace)";\n    Ok(())',
  );
  assert.throws(
    () => validateRustOwnerControlBoundaries(hostedStringDecoy),
    /body differs from the pinned canonical ts-tests entrypoint/u,
  );

  const localCommentDecoy = rustOwnerBoundaryFixture();
  localCommentDecoy.xtaskSource = localCommentDecoy.xtaskSource.replace(
    "    h2_3b_acceptance::run_owner_controls(workspace)?;",
    "    // h2_3b_acceptance::run_owner_controls(workspace)?;",
  );
  assert.throws(
    () => validateRustOwnerControlBoundaries(localCommentDecoy),
    /body differs from the exact complete local owner-control statements/u,
  );

  const localStringDecoy = rustOwnerBoundaryFixture();
  localStringDecoy.xtaskSource = localStringDecoy.xtaskSource.replace(
    "    h2_3c_acceptance::run_owner_controls(workspace)?;",
    '    let missing_control = "h2_3c_acceptance::run_owner_controls(workspace)?;";',
  );
  assert.throws(
    () => validateRustOwnerControlBoundaries(localStringDecoy),
    /body differs from the exact complete local owner-control statements/u,
  );

  const unreachableLocalCalls = rustOwnerBoundaryFixture();
  unreachableLocalCalls.xtaskSource = unreachableLocalCalls.xtaskSource.replace(
    "    h2_3b_acceptance::run_owner_controls(workspace)?;",
    "    return Ok(());\n    h2_3b_acceptance::run_owner_controls(workspace)?;",
  );
  assert.throws(
    () => validateRustOwnerControlBoundaries(unreachableLocalCalls),
    /body differs from the exact complete local owner-control statements/u,
  );
});

test("Rust owner-control boundary rejects direct hosted owner calls", () => {
  const hosted = rustOwnerBoundaryFixture();
  hosted.xtaskSource = hosted.xtaskSource.replace(
    "    h2_2c_acceptance::run_h2_5g(&workspace)",
    "    h2_3d_acceptance::run_h2_5g_owner_controls(&workspace)?;\n" +
      "    h2_2c_acceptance::run_h2_5g(&workspace)",
  );
  assert.throws(
    () => validateRustOwnerControlBoundaries(hosted),
    /body differs from the pinned canonical ts-tests entrypoint/u,
  );

  const module = rustOwnerBoundaryFixture();
  const modulePath = OWNER_MODULE_PATHS[0];
  module.moduleSources[modulePath] = module.moduleSources[modulePath].replace(
    "    let decoy =",
    "    h2_3b_acceptance::run_owner_controls(workspace)?;\n    let decoy =",
  );
  assert.throws(
    () => validateRustOwnerControlBoundaries(module),
    /hosted run function contains a direct owner-control call/u,
  );
});

test("Rust hosted call graph rejects transitive owner-control helpers and aliases", () => {
  for (const [name, call] of [
    ["same-module helper", "    run_extra(workspace)?;"],
    ["qualified same-module helper", "    self::run_extra(workspace)?;"],
    ["local alias", "    let neutral = run_extra;\n    neutral(workspace)?;"],
  ]) {
    const fixture = rustOwnerBoundaryFixture();
    const modulePath = OWNER_MODULE_PATHS[0];
    fixture.moduleSources[modulePath] = fixture.moduleSources[modulePath]
      .replace("    let decoy =", `${call}\n    let decoy =`)
      .replace(
        "pub fn run_owner_controls",
        `fn run_extra(workspace: &Path) -> Result<(), Box<dyn Error>> {
    run_owner_controls(workspace)
}
pub fn run_owner_controls`,
      );
    assert.throws(
      () => validateRustOwnerControlBoundaries(fixture),
      /hosted call graph.*reachable function.*run_extra.*owner-control symbol/u,
      name,
    );
  }

  const groupedAlias = rustOwnerBoundaryFixture();
  const groupedAliasModule = OWNER_MODULE_PATHS[0];
  groupedAlias.moduleSources[groupedAliasModule] = `use self::{run_extra as neutral};\n${groupedAlias.moduleSources[groupedAliasModule]}`
    .replace("    let decoy =", "    neutral(workspace)?;\n    let decoy =")
    .replace(
      "pub fn run_owner_controls",
      `fn run_extra(workspace: &Path) -> Result<(), Box<dyn Error>> {
    run_owner_controls(workspace)
}
pub fn run_owner_controls`,
    );
  assert.throws(
    () => validateRustOwnerControlBoundaries(groupedAlias),
    /hosted call graph.*run_extra.*owner-control symbol/u,
  );

  const unresolvedAlias = rustOwnerBoundaryFixture();
  const unresolvedModule = OWNER_MODULE_PATHS[0];
  unresolvedAlias.moduleSources[unresolvedModule] = `use self::missing as neutral;\n${unresolvedAlias.moduleSources[unresolvedModule]}`
    .replace("    let decoy =", "    neutral(workspace)?;\n    let decoy =");
  assert.throws(
    () => validateRustOwnerControlBoundaries(unresolvedAlias),
    /unresolved local Rust function path self::missing/u,
  );

  const crossModule = rustOwnerBoundaryFixture();
  crossModule.moduleSources[OWNER_MODULE_PATHS[0]] = crossModule.moduleSources[
    OWNER_MODULE_PATHS[0]
  ].replace(
    "    let decoy =",
    "    h2_3c_acceptance::run_extra(workspace)?;\n    let decoy =",
  );
  crossModule.moduleSources[OWNER_MODULE_PATHS[1]] = crossModule.moduleSources[
    OWNER_MODULE_PATHS[1]
  ].replace(
    "pub fn run_owner_controls",
    `fn run_extra(workspace: &Path) -> Result<(), Box<dyn Error>> {
    run_owner_controls(workspace)
}
pub fn run_owner_controls`,
  );
  assert.throws(
    () => validateRustOwnerControlBoundaries(crossModule),
    /hosted call graph.*h2_3c_acceptance::run_extra.*owner-control symbol/u,
  );

  const mainHelper = rustOwnerBoundaryFixture();
  mainHelper.xtaskSource = mainHelper.xtaskSource
    .replace(
      "    h2_2c_acceptance::run_h2_5g(&workspace)",
      "    hosted_extra(&workspace)?;\n    h2_2c_acceptance::run_h2_5g(&workspace)",
    )
    .replace(
      "fn ci_h2_owner_controls",
      `fn hosted_extra(workspace: &Path) -> Result<(), Box<dyn Error>> {
    h2_3d_acceptance::run_owner_controls(workspace)
}

fn ci_h2_owner_controls`,
    );
  assert.throws(
    () => validateRustOwnerControlBoundaries(mainHelper),
    /body differs from the pinned canonical ts-tests entrypoint/u,
  );

  const unresolvedLocalModule = rustOwnerBoundaryFixture();
  unresolvedLocalModule.xtaskSource = `mod neutral;\n${unresolvedLocalModule.xtaskSource}`
    .replace(
      "    h2_2c_acceptance::run_h2_5g(&workspace)",
      "    hosted_extra(&workspace)?;\n    h2_2c_acceptance::run_h2_5g(&workspace)",
    )
    .replace(
      "fn ci_h2_owner_controls",
      `fn hosted_extra(workspace: &Path) -> Result<(), Box<dyn Error>> {
    neutral::run(workspace)
}

fn ci_h2_owner_controls`,
    );
  assert.throws(
    () => validateRustOwnerControlBoundaries(unresolvedLocalModule),
    /body differs from the pinned canonical ts-tests entrypoint/u,
  );
});

test("hosted acceptance body is pinned as one canonical executable shape", () => {
  const mutations = [
    ["leading return", (source) => source.replace(
      "    if let Some(argument) = args.next() {",
      "    return Ok(());\n    if let Some(argument) = args.next() {",
    )],
    ["changed argument guard", (source) => source.replace(
      "if let Some(argument) = args.next()",
      "if args.next().is_some()",
    )],
    ["extra unqualified statement", (source) => source.replace(
      "    conformance(std::iter::empty())?;",
      "    benign()?;\n    conformance(std::iter::empty())?;",
    )],
    ["reordered setup", (source) => source.replace(
      "    conformance(std::iter::empty())?;\n    let workspace = find_workspace_root()?;",
      "    let workspace = find_workspace_root()?;\n    conformance(std::iter::empty())?;",
    )],
    ["wrong argument", (source) => source.replace(
      "h2_3b_acceptance::run(&workspace)?;",
      "h2_3b_acceptance::run(workspace)?;",
    )],
    ["wrong suffix", (source) => source.replace(
      "h2_3c_acceptance::run(&workspace)?;",
      "h2_3c_acceptance::run(&workspace);",
    )],
    ["nested call", (source) => source.replace(
      "    h2_3d_acceptance::run(&workspace)?;",
      "    { h2_3d_acceptance::run(&workspace)?; }",
    )],
    ["reordered calls", (source) => source.replace(
      "    h2_1a_acceptance::run(&workspace)?;\n    h2_1b_acceptance::run(&workspace)?;",
      "    h2_1b_acceptance::run(&workspace)?;\n    h2_1a_acceptance::run(&workspace)?;",
    )],
  ];
  for (const [name, mutate] of mutations) {
    const fixture = rustOwnerBoundaryFixture();
    fixture.xtaskSource = mutate(fixture.xtaskSource);
    repinRustFixture(fixture);
    assert.throws(
      () => validateRustOwnerControlBoundaries(fixture),
      /body differs from the pinned canonical ts-tests entrypoint/u,
      name,
    );
  }
});

test("hosted module declarations reject path, cfg, inline, duplicate, and nested alternatives", () => {
  const moduleName = "h1_emit_acceptance";
  const declaration = `mod ${moduleName};`;
  const mutations = [
    ["path", `#[path = "alternate.rs"]\n${declaration}`],
    ["cfg", `#[cfg(unix)]\n${declaration}`],
    ["inline", `mod ${moduleName} {}`],
    ["duplicate", `${declaration}\n${declaration}`],
    ["cfg alternatives", `#[cfg(unix)]\n${declaration}\n#[cfg(not(unix))]\n${declaration}`],
    ["nested", `mod wrapper { ${declaration} }`],
  ];
  for (const [name, replacement] of mutations) {
    const fixture = rustOwnerBoundaryFixture();
    fixture.xtaskSource = fixture.xtaskSource.replace(declaration, replacement);
    repinRustFixture(fixture);
    assert.throws(
      () => validateRustOwnerControlBoundaries(fixture),
      /module declaration|must use the canonical|declaration must/u,
      name,
    );
  }
});

test("raw source pins close Rust constructs outside the bounded call-graph grammar", () => {
  const modulePath = OWNER_MODULE_PATHS[0];
  const directOwnerVariants = [
    ["tuple destructuring", "    let (neutral,) = (run_owner_controls,);\n    neutral(workspace)?;"],
    ["excess parentheses", "    (run_owner_controls)(workspace)?;"],
    ["block", "    { run_owner_controls(workspace)?; }"],
    ["as fn alias", "    let neutral = run_owner_controls as fn(&Path) -> _;\n    neutral(workspace)?;"],
  ];
  for (const [name, statement] of directOwnerVariants) {
    const fixture = rustOwnerBoundaryFixture();
    fixture.moduleSources[modulePath] = fixture.moduleSources[modulePath].replace(
      "    let decoy =",
      `${statement}\n    let decoy =`,
    );
    assert.throws(
      () => validateRustOwnerControlBoundaries(fixture),
      /hosted run function contains a direct owner-control call or input/u,
      name,
    );
  }

  const contentAddressedVariants = [
    [
      "nested grouped import",
      "mod holder { pub use super::run_owner_controls; }\n" +
        "use self::{holder::{run_owner_controls as neutral}};\n",
      "    neutral(workspace)?;",
    ],
    [
      "macro_rules",
      "macro_rules! neutral { () => { run_owner_controls(workspace)?; }; }\n",
      "    neutral!();",
    ],
    [
      "impl method",
      "struct Neutral;\nimpl Neutral { fn call(workspace: &Path) -> Result<(), Box<dyn Error>> { run_owner_controls(workspace) } }\n",
      "    Neutral::call(workspace)?;",
    ],
    [
      "nested module",
      "mod neutral { pub fn call(workspace: &Path) -> Result<(), Box<dyn Error>> { super::run_owner_controls(workspace) } }\n",
      "    neutral::call(workspace)?;",
    ],
  ];
  for (const [name, prefix, statement] of contentAddressedVariants) {
    const fixture = rustOwnerBoundaryFixture();
    fixture.moduleSources[modulePath] = `${prefix}${fixture.moduleSources[modulePath]}`.replace(
      "    let decoy =",
      `${statement}\n    let decoy =`,
    );
    assert.throws(
      () => validateRustOwnerControlBoundaries(fixture),
      /h2_3b_acceptance\.rs content hash drifted/u,
      name,
    );
  }

  const rawByteFixture = rustOwnerBoundaryFixture();
  rawByteFixture.rawSourceBytes = Object.fromEntries([
    ["crates/xtask/src/main.rs", Buffer.from(rawByteFixture.xtaskSource)],
    ...Object.entries(rawByteFixture.moduleSources).map(([sourcePath, source]) => [
      sourcePath,
      Buffer.from(source),
    ]),
  ]);
  rawByteFixture.rawSourceBytes[modulePath] = Buffer.concat([
    rawByteFixture.rawSourceBytes[modulePath],
    Buffer.from("\n"),
  ]);
  assert.throws(
    () => validateRustOwnerControlBoundaries(rawByteFixture),
    /h2_3b_acceptance\.rs content hash drifted/u,
  );
});

test("policy and every qualification schema boundary are valid", () => {
  const policy = validatePolicy(loadPolicy());
  assert.equal(policy.status, "active");
  assert.equal(policy.aggregate_check, "gates");
  assert.equal(policy.hosted_acceptance.authority_job, "gates");
  assert.equal(policy.hosted_acceptance.test_root, "ts-tests/");
  assert.deepEqual(policy.hosted_acceptance.authoritative_command, [
    "cargo",
    "xtask",
    "acceptance",
  ]);
  assert.equal(policy.hosted_acceptance.only_acceptance_tests, true);
  assert.equal(
    Object.keys(policy.hosted_acceptance.rust_source_sha256).length,
    16,
  );
  assert.match(
    policy.hosted_acceptance.rust_source_sha256["crates/xtask/src/main.rs"],
    /^[0-9a-f]{64}$/u,
  );
  assert.deepEqual(policy.local_full_gate.authoritative_command, [
    "cargo",
    "xtask",
    "ci",
    "--baseline",
    "<trusted-base>",
  ]);
  assert.equal(policy.approved_performance.authority_job, "qualify");
  assert.equal(policy.approved_performance.l1_authority_job, "qualify");
  assert.equal(policy.approved_performance.h1_authority_job, "qualify");
  assert.equal(
    policy.approved_performance.l1_h0_evidence,
    "ratchets/l1-h0-performance.v1.json",
  );
  assert.equal(
    policy.approved_performance.h1_evidence,
    "ratchets/h1-noemit-performance.v1.json",
  );

  const frozen = clone(policy);
  frozen.status = "frozen";
  assert.throws(() => validatePolicy(frozen), /policy header/u);

  const broadenedHostedCommand = clone(policy);
  broadenedHostedCommand.hosted_acceptance.authoritative_command = ["cargo", "xtask", "ci"];
  assert.throws(
    () => validatePolicy(broadenedHostedCommand),
    /hosted ts-tests acceptance/u,
  );

  const driftedHostedSource = clone(policy);
  driftedHostedSource.hosted_acceptance.rust_source_sha256[
    "crates/xtask/src/main.rs"
  ] = "0".repeat(64);
  assert.throws(
    () => validatePolicy(driftedHostedSource),
    /main\.rs content hash drifted/u,
  );
});

test("documentation-only changes select no execution lane", () => {
  const selection = classifyPaths({
    paths: ["docs/design/greenfield/lsp-and-incremental.md"],
    headSha: HEAD,
    baseSha: BASE,
    statusBlockEqual: true,
    policy: loadPolicy(),
  });
  assert.equal(selection.docs_only, true);
  assert.deepEqual(selection.selected, {
    static: false,
    host_platform: false,
    program_path: false,
    tracks: [],
  });
});

test("generated-status drift and unknown paths fail closed", () => {
  const statusDrift = classifyPaths({
    paths: ["README.md"],
    headSha: HEAD,
    baseSha: BASE,
    statusBlockEqual: false,
    policy: loadPolicy(),
  });
  assert.equal(statusDrift.docs_only, false);
  assert.deepEqual(statusDrift.selected.tracks, ["common", "h1", "l0-l1"]);
  assert.equal(statusDrift.selected.host_platform, true);
  assert.equal(statusDrift.selected.program_path, true);

  const unknown = classifyPaths({
    paths: ["new-runtime/owner.rs"],
    headSha: HEAD,
    baseSha: BASE,
    statusBlockEqual: true,
    policy: loadPolicy(),
  });
  assert.deepEqual(unknown.selected.tracks, ["common", "h1", "l0-l1"]);
  assert.equal(unknown.selected.host_platform, true);
});

test("known compiler and program owners select their focused tracks", () => {
  const compiler = classifyPaths({
    paths: ["crates/compiler/src/lib.rs"],
    headSha: HEAD,
    baseSha: BASE,
    statusBlockEqual: true,
    policy: loadPolicy(),
  });
  assert.deepEqual(compiler.selected.tracks, ["common", "h1", "l0-l1"]);
  assert.equal(compiler.selected.host_platform, false);

  const program = classifyPaths({
    paths: ["crates/program/src/lib.rs"],
    headSha: HEAD,
    baseSha: BASE,
    statusBlockEqual: true,
    policy: loadPolicy(),
  });
  assert.equal(program.selected.host_platform, true);
  assert.equal(program.selected.program_path, true);
  assert.ok(program.selected.tracks.includes("l0-l1"));
});

test("local evidence classification remains fail-closed for policy inputs", () => {
  for (const changedPath of [
    ".github/workflows/ci.yml",
    ".github/workflows/l1-performance.yml",
    ".github/workflows/h1-noemit-performance.yml",
    ".github/ci/qualification-policy.v2.json",
    ".github/ci/contracts/qualification-result.schema.json",
    "Cargo.lock",
    ".node-version",
  ]) {
    const selection = classifyPaths({
      paths: [changedPath],
      headSha: HEAD,
      baseSha: BASE,
      statusBlockEqual: true,
      policy: loadPolicy(),
    });
    assert.deepEqual(selection.selected.tracks, ["common", "h1", "l0-l1"], changedPath);
  }
});

test("lane selection rejects ambiguity, traversal, and missing lanes", () => {
  const selection = {
    schema: 1,
    kind: "lane-selection",
    head_sha: HEAD,
    base_sha: BASE,
    changed_paths: ["crates/compiler/src/lib.rs"],
    paths_sha256: pathsDigest(["crates/compiler/src/lib.rs"]),
    docs_only: false,
    selected: {
      static: true,
      host_platform: false,
      program_path: false,
      tracks: ["common", "h1", "l0-l1"],
    },
  };
  assert.equal(validateLaneSelection(selection), selection);
  const extra = clone(selection);
  extra.silent_skip = true;
  assert.throws(() => validateLaneSelection(extra), /unknown fields/u);
  const traversal = clone(selection);
  traversal.changed_paths = ["../outside"];
  traversal.paths_sha256 = pathsDigest(traversal.changed_paths);
  assert.throws(() => validateLaneSelection(traversal), /invalid repository path/u);
  const missing = clone(selection);
  missing.selected.tracks = ["h1", "l0-l1"];
  assert.throws(() => validateLaneSelection(missing), /static\/common/u);
  const programWithoutHost = clone(selection);
  programWithoutHost.selected.program_path = true;
  assert.throws(() => validateLaneSelection(programWithoutHost), /host-platform/u);
});

function validReceipt() {
  const receipt = {
    schema: 1,
    kind: "exact-merge-qualification",
    head_sha: HEAD,
    base_sha: BASE,
    inputs: {
      rust_toolchain_sha256: HASH,
      node_version_sha256: HASH,
      cargo_lock_sha256: HASH,
      vendor_inventory_sha256: HASH,
      suite_inventory_sha256: HASH,
      qualification_profile_sha256: HASH,
      lane_selection_sha256: HASH,
    },
    lanes: [{ name: "full", status: "success", result_sha256: HASH }],
    commands: [{ argv: ["cargo", "xtask", "ci"], exit_code: 0, stdout_sha256: HASH, stderr_sha256: HASH }],
    result_sha256: "0".repeat(64),
    authentication: {
      kind: "trusted-runner-oidc",
      issuer: "https://token.actions.githubusercontent.com",
      subject: "repo:kazhiramatsu/tsc-rs:ref:refs/heads/main",
      attestation_sha256: HASH,
    },
  };
  receipt.result_sha256 = receiptResultHash(receipt);
  return receipt;
}

test("exact receipt binds successful commands, immutable inputs, and authentication", () => {
  const receipt = validReceipt();
  assert.equal(validateMergeReceipt(receipt), receipt);
  const movedBase = clone(receipt);
  movedBase.base_sha = "3".repeat(40);
  assert.throws(() => validateMergeReceipt(movedBase), /digest mismatch/u);
  const failed = clone(receipt);
  failed.commands[0].exit_code = 1;
  assert.throws(() => validateMergeReceipt(failed), /command result/u);
  const unsigned = clone(receipt);
  unsigned.authentication.kind = "unsigned";
  assert.throws(() => validateMergeReceipt(unsigned), /authentication/u);
  const unknown = clone(receipt);
  unknown.inputs.extra = HASH;
  assert.throws(() => validateMergeReceipt(unknown), /input binding/u);
});

test("OIDC receipt remains bound to the exact attested qualification result", () => {
  const receipt = validReceipt();
  const result = clone(receipt);
  delete result.authentication;
  result.kind = "exact-merge-qualification-result";
  result.result_sha256 = qualificationResultHash(result);
  assert.equal(validateQualificationResult(result), result);

  const bundle = Buffer.from('{"mediaType":"application/vnd.dev.sigstore.bundle.v0.3+json"}\n');
  receipt.authentication.attestation_sha256 = sha256(bundle);
  receipt.result_sha256 = receiptResultHash(receipt);
  assert.equal(validateBoundReceipt(receipt, result, bundle, HEAD, BASE), receipt);

  const movedResult = clone(result);
  movedResult.base_sha = "3".repeat(40);
  movedResult.result_sha256 = qualificationResultHash(movedResult);
  assert.throws(
    () => validateBoundReceipt(receipt, movedResult, bundle, HEAD, BASE),
    /expected HEAD\/base/u,
  );
  assert.throws(
    () => validateBoundReceipt(receipt, result, Buffer.from("{}"), HEAD, BASE),
    /verified attestation bundle/u,
  );
});

test("failure artifacts are bounded, relative, and content addressed", () => {
  const payload = Buffer.from("bounded reproducer\n");
  const artifact = {
    schema: 1,
    kind: "failure-artifact",
    head_sha: HEAD,
    base_sha: BASE,
    track: "l0-l1",
    payload_path: "failure/reproducer.json",
    content_type: "application/json",
    bytes: payload.length,
    payload_sha256: sha256(payload),
    truncated: false,
    reproducer: { seed: "42", fixture: "ratchets/l0-fixtures.v1.json" },
  };
  assert.equal(validateFailureArtifact(artifact, payload), artifact);
  const oversized = clone(artifact);
  oversized.bytes = 10_485_761;
  assert.throws(() => validateFailureArtifact(oversized), /bound/u);
  const traversal = clone(artifact);
  traversal.payload_path = "../secrets";
  assert.throws(() => validateFailureArtifact(traversal), /payload metadata/u);
  const tampered = clone(artifact);
  tampered.payload_sha256 = HASH;
  assert.throws(() => validateFailureArtifact(tampered, payload), /binding mismatch/u);
});
