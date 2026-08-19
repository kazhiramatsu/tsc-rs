import crypto from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH =
  "crates/oracle/h2-5h-a-es2015-generators-witnesses.mjs";
const TARGET_RELATIVE_PATH =
  "ratchets/h2-5h-a-es2015-generators-witnesses.v1.json";
const CONTRACT_RELATIVE_PATH =
  ".github/ci/contracts/h2-5h-a-es2015-generators-witnesses.schema.json";
const FOUNDATION_RELATIVE_PATH = "ratchets/h2-5h-a-foundation.v1.json";
const OWNER_GRAPH_RELATIVE_PATH = "ratchets/h2-5h-a-owner-graph.v1.json";
const HANDOFF_RELATIVE_PATH = "docs/design/greenfield/slices/h2-5h-a.md";
const TYPESCRIPT_BUNDLE = "vendor/typescript-6.0.3/lib/typescript.js";
const TYPESCRIPT_IMPLEMENTATION = "vendor/typescript-6.0.3/lib/_tsc.js";
const TYPESCRIPT_LIB_DIRECTORY = "vendor/typescript-6.0.3/lib";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";
const EXPECTED_NODE = "25.2.1";
const SLICE = "H2.5h-a";
const SUB_PACKET = "W-H2.5H";
const INTERNAL_OBSERVE_MODE = "--internal-observe-witness";

const ROLES = Object.freeze([
  "positive",
  "adjacent-negative-control",
  "composition",
  "fault",
]);

// The five composition edges frozen in the owner graph. Every composition
// case cites at least one and the set must be covered exactly.
const EDGE_IDS = Object.freeze([
  "pass-order",
  "yield-star-synthesis",
  "substitution-chain",
  "destructuring-shared-module",
  "tagged-template-shared-module",
]);

// Exact partition of the owner graph's seventeen census surfaces: the
// fourteen required surfaces must be covered by the family union, and the
// three excluded surfaces carry the owning authority instead of a witness.
const REQUIRED_SURFACES = Object.freeze([
  "class-lowering-reach",
  "destructuring-module",
  "factory-construction",
  "helper-factory",
  "hook-composition",
  "lexical-environment",
  "loop-partition-machinery",
  "name-generation",
  "resolver-collision-capture-queries",
  "resolver-node-check-flags",
  "syntax-guards",
  "tagged-template-module",
  "transform-flag-recomputation",
  "yield-star-synthesis",
]);

const EXCLUDED_SURFACES = Object.freeze([
  Object.freeze({
    surface_id: "comment-apis",
    reason:
      "comment relocation semantics are owned by the frozen E-COMMENT-SCOPE-H witness artifact, not re-witnessed here",
  }),
  Object.freeze({
    surface_id: "source-map-apis",
    reason:
      "source-map provenance is future-owned-fail-closed under EA-GAP-MAPS-DECLS (H2.6/H2.7) per the disposition manifest",
  }),
  Object.freeze({
    surface_id: "outer-expression-wrappers",
    reason:
      "printer position/comment anchoring is not independently distinguishable through pure-source emit-level witnesses; its rows are revalidated by the comment-scope study and E-POSITIONS lineage",
  }),
]);

// The pinned yield* synthesis sites (owner-relative offsets frozen in the
// owner graph). Enclosing function names are re-derived from the pinned
// implementation bytes at build time and must match these records exactly.
const YIELD_STAR_SITES = Object.freeze([
  Object.freeze({
    site_index: 0,
    enclosing_function: "generateCallToConvertedLoopInitializer",
    covering_case_id: "loop-conversion-capture--composition-initializer-call",
  }),
  Object.freeze({
    site_index: 1,
    enclosing_function: "generateCallToConvertedLoop",
    covering_case_id: "loop-conversion-capture--composition-body-call",
  }),
]);

function compilerOptions(extra = {}) {
  return {
    target: ts.ScriptTarget.ES5,
    module: ts.ModuleKind.ESNext,
    alwaysStrict: true,
    downlevelIteration: true,
    importHelpers: false,
    noEmitHelpers: false,
    newLine: ts.NewLineKind.LineFeed,
    useDefineForClassFields: false,
    useUnknownInCatchVariables: false,
    ignoreDeprecations: "6.0",
    ...extra,
  };
}

function witnessCase(caseSlug, role, description, source, extra = {}) {
  return {
    case_slug: caseSlug,
    role,
    description,
    source,
    option_overrides: extra.option_overrides ?? {},
    markers: extra.markers ?? [],
    expected_reported_codes: extra.expected_reported_codes ?? [],
    composition_edges: extra.composition_edges ?? [],
    foundation_control: extra.foundation_control ?? null,
  };
}

function marker(token, expectation) {
  return { token, expectation };
}

// Foundation direct-control sources, copied byte-exact. Build-time
// validation compares them (and the emitted bytes) against the frozen
// foundation artifact, so any drift here fails the mint.
const FOUNDATION_COLLISION_SOURCE = [
  "declare function use(value: unknown): void;",
  "function collisionScope() {",
  "  var collisionValue = 0;",
  "  {",
  "    let collisionValue = 1;",
  "    use(collisionValue);",
  "  }",
  "}",
  "",
].join("\n");

const FOUNDATION_CAPTURED_SOURCE = [
  "declare function use(value: unknown): void;",
  "for (let capturedValue = 0; capturedValue < 2; capturedValue++) {",
  "  use(() => capturedValue);",
  "}",
  "",
].join("\n");

const FOUNDATION_ARGUMENTS_SOURCE = [
  "function localArguments(arguments: number) { return arguments; }",
  "function lexicalArguments() { return () => arguments; }",
  "function* catches() {",
  "  try { yield 1; }",
  "  catch (caughtValue) { yield caughtValue; }",
  "}",
  "",
].join("\n");

// Witness inputs. Expected output bytes are never written here: every
// observation is captured from the pinned TypeScript runtime in a fresh
// process, twice. Marker expectations assert only non-vacuity of the
// mechanism under witness (helper identities, converted-loop artifacts,
// renames); the frozen bytes are the authority.
const WITNESS_FAMILY_SPECS = Object.freeze([
  {
    family_id: "loop-conversion-capture",
    surfaces: [
      "loop-partition-machinery",
      "yield-star-synthesis",
      "lexical-environment",
    ],
    description:
      "captured block-scoped loop bindings force loop conversion; inside a generator the converted initializer/body calls are re-emitted through the two pinned yield* synthesis sites and lowered by the Generators state machine; with downlevelIteration off the same surface degrades to the indexed loop under a typed diagnostic. The initializer-call output is upstream-faithful including its unassigned out_index_1 read: bytes are the pinned oracle's, never a hand-derived correction",
    cases: [
      witnessCase(
        "positive-captured-forof",
        "positive",
        "for-of over a declared array with a closure capturing the loop binding extracts the loop body into a converted loop function",
        [
          "declare function sink(value: unknown): void;",
          "declare const items: number[];",
          "for (const element of items) {",
          "  sink(() => element);",
          "}",
          "",
        ].join("\n"),
        {
          markers: [marker("__values", "present"), marker("_loop_1", "present")],
        },
      ),
      witnessCase(
        "adjacent-negative-uncaptured",
        "adjacent-negative-control",
        "the same for-of without a capturing closure keeps the body inline: no converted loop function is created",
        [
          "declare function sink(value: unknown): void;",
          "declare const items: number[];",
          "for (const element of items) {",
          "  sink(element);",
          "}",
          "",
        ].join("\n"),
        {
          markers: [marker("__values", "present"), marker("_loop_1", "absent")],
        },
      ),
      witnessCase(
        "composition-body-call",
        "composition",
        "a generator for-loop with a captured binding and yield in the body synthesizes yield* around the converted body call (pinned site generateCallToConvertedLoop) and the Generators state machine lowers the delegation",
        [
          "declare function sink(value: unknown): void;",
          "function* sequence() {",
          "  for (let cursor = 0; cursor < 3; cursor++) {",
          "    sink(() => cursor);",
          "    yield cursor;",
          "  }",
          "}",
          "",
        ].join("\n"),
        {
          markers: [
            marker("__generator", "present"),
            marker("_loop_1", "present"),
          ],
          composition_edges: ["yield-star-synthesis", "pass-order"],
        },
      ),
      witnessCase(
        "composition-initializer-call",
        "composition",
        "a generator for-loop whose initializer both captures its own binding and yields converts the initializer into a nested generator called through yield* (pinned site generateCallToConvertedLoopInitializer) with loop out-parameters",
        [
          "declare function sink(value: unknown): void;",
          "function* seeded(): Generator<number, void, number> {",
          "  for (let seed = (sink(() => seed), yield 1), index = 0; index < 3; index++) {",
          "    yield index;",
          "  }",
          "}",
          "",
        ].join("\n"),
        {
          markers: [
            marker("_loop_init_1", "present"),
            marker("out_seed_1", "present"),
          ],
          composition_edges: ["yield-star-synthesis", "pass-order"],
        },
      ),
      witnessCase(
        "fault-downlevel-iteration-off",
        "fault",
        "for-of over a Set without downlevelIteration reports the typed diagnostic and still emits the indexed fallback loop",
        [
          "declare const bag: Set<number>;",
          "for (const entry of bag) {",
          "  void entry;",
          "}",
          "",
        ].join("\n"),
        {
          option_overrides: { downlevelIteration: false },
          markers: [marker("__values", "absent")],
          expected_reported_codes: [2802],
        },
      ),
    ],
  },
  {
    family_id: "class-lowering-lanes",
    surfaces: ["class-lowering-reach"],
    description:
      "class lowering lanes: heritage via the extends helper, accessor pairs through defineProperty, static members, and super member calls; the prototype-pattern control takes none of these lanes; a generator method routes the method body through the Generators state machine inside the class IIFE; an unresolved base still lowers under the checker fault",
    cases: [
      witnessCase(
        "positive-lanes",
        "positive",
        "derived class with static property, get/set accessor pair, prototype method, and super call exercises the class lowering lanes",
        [
          "class Base {",
          "  static origin = 0;",
          "  get size(): number { return 1; }",
          "  set size(next: number) { void next; }",
          '  describe(): string { return "base"; }',
          "}",
          "class Derived extends Base {",
          "  describe(): string { return `derived ${super.describe()}`; }",
          "}",
          "",
        ].join("\n"),
        {
          markers: [
            marker("__extends", "present"),
            marker("extendStatics", "present"),
          ],
        },
      ),
      witnessCase(
        "adjacent-negative-prototype",
        "adjacent-negative-control",
        "an ES5 constructor-function/prototype pattern is byte-preserved without any class lane",
        [
          "function Shape(this: { edges: number }, edges: number) {",
          "  this.edges = edges;",
          "}",
          "Shape.prototype.count = function (this: { edges: number }) {",
          "  return this.edges;",
          "};",
          "",
        ].join("\n"),
        {
          markers: [marker("__extends", "absent")],
        },
      ),
      witnessCase(
        "composition-generator-method",
        "composition",
        "a class generator method lowers the method body through the Generators state machine inside the ES2015 class IIFE",
        [
          "class Streamer {",
          "  *chunks(limit: number) {",
          "    for (let index = 0; index < limit; index++) {",
          "      yield index;",
          "    }",
          "  }",
          "}",
          "",
        ].join("\n"),
        {
          markers: [
            marker("__generator", "present"),
            marker("__extends", "absent"),
          ],
          composition_edges: ["pass-order"],
        },
      ),
      witnessCase(
        "fault-missing-base",
        "fault",
        "extending an unresolved identifier reports the checker fault and still emits the extends-helper lowering against the missing name",
        [
          "class Orphan extends MissingBase {",
          "  tag(): number { return 1; }",
          "}",
          "",
        ].join("\n"),
        {
          markers: [marker("__extends", "present")],
          expected_reported_codes: [2304],
        },
      ),
    ],
  },
  {
    family_id: "destructuring-flattener",
    surfaces: ["destructuring-module", "factory-construction"],
    description:
      "the shared destructuring flattener: binding patterns with defaults and nesting, assignment-pattern swaps, and object rest through the read/rest helpers; the flattener's temps interleave with generator hoisting when the pattern consumes a yielded value; a non-iterable right-hand side keeps the flattened emit under the typed diagnostic",
    cases: [
      witnessCase(
        "positive-patterns",
        "positive",
        "tuple binding with default and nested object member, array assignment swap, and object rest flatten through the read/rest helpers",
        [
          "declare const pair: [number, { depth: number }];",
          "const [first = 5, { depth }] = pair;",
          "declare let left: number, right: number;",
          "[left, right] = [right, left];",
          "const { a: renamed, ...rest } = { a: 1, b: 2 };",
          "void first; void depth; void renamed; void rest;",
          "",
        ].join("\n"),
        {
          markers: [marker("__read", "present"), marker("__rest", "present")],
        },
      ),
      witnessCase(
        "adjacent-negative-plain",
        "adjacent-negative-control",
        "plain indexed element reads take no flattener path",
        [
          "declare const pair: [number, number];",
          "const firstPlain = pair[0];",
          "const secondPlain = pair[1];",
          "void firstPlain; void secondPlain;",
          "",
        ].join("\n"),
        {
          markers: [marker("__read", "absent")],
        },
      ),
      witnessCase(
        "composition-inside-generator",
        "composition",
        "destructuring the value received from yield routes the flattener through the Generators state machine (read helper applied to the resumed value)",
        [
          "function* splitter(): Generator<number, void, [number, number]> {",
          "  const [head, tail] = yield 1;",
          "  yield head + tail;",
          "}",
          "",
        ].join("\n"),
        {
          markers: [
            marker("__read", "present"),
            marker("__generator", "present"),
          ],
          composition_edges: ["destructuring-shared-module", "pass-order"],
        },
      ),
      witnessCase(
        "fault-not-iterable",
        "fault",
        "array-destructuring a number reports the typed diagnostic and still emits the flattened read-helper call",
        ["const [broken] = 0;", "void broken;", ""].join("\n"),
        {
          markers: [marker("__read", "present")],
          expected_reported_codes: [2548],
        },
      ),
    ],
  },
  {
    family_id: "tagged-template-lowering",
    surfaces: ["tagged-template-module"],
    description:
      "the shared tagged-template module: cooked/raw divergence through the template-object helper, the untagged concat control, hoisted tag/template temps across a yield inside a generator, and an uncallable tag still lowering under the checker fault",
    cases: [
      witnessCase(
        "positive-raw-cooked",
        "positive",
        "a tagged template whose literal contains an escape sequence pins the cooked/raw divergence in the template-object helper call",
        [
          "declare function tag(strings: TemplateStringsArray, ...values: number[]): string;",
          "declare const amount: number;",
          "const rendered = tag`prefix\\n${amount}suffix`;",
          "void rendered;",
          "",
        ].join("\n"),
        {
          markers: [marker("__makeTemplateObject", "present")],
        },
      ),
      witnessCase(
        "adjacent-negative-untagged",
        "adjacent-negative-control",
        "the same untagged template lowers to string concatenation without the template-object helper",
        [
          "declare const amount: number;",
          "const rendered = `prefix\\n${amount}suffix`;",
          "void rendered;",
          "",
        ].join("\n"),
        {
          markers: [marker("__makeTemplateObject", "absent")],
        },
      ),
      witnessCase(
        "composition-yield-substitution",
        "composition",
        "a tagged template with yield in a substitution hoists the tag callee and template object into temps spread across the state-machine labels",
        [
          "declare function tag(strings: TemplateStringsArray, ...values: number[]): string;",
          "function* renderer(): Generator<number, string, number> {",
          "  return tag`value ${yield 1} end`;",
          "}",
          "",
        ].join("\n"),
        {
          markers: [
            marker("__makeTemplateObject", "present"),
            marker("__generator", "present"),
          ],
          composition_edges: ["tagged-template-shared-module", "pass-order"],
        },
      ),
      witnessCase(
        "fault-uncallable-tag",
        "fault",
        "tagging with a non-callable value reports the checker fault and still emits the template-object lowering",
        [
          "declare const notCallable: number;",
          "const rendered = notCallable`broken`;",
          "void rendered;",
          "",
        ].join("\n"),
        {
          markers: [marker("__makeTemplateObject", "present")],
          expected_reported_codes: [2349],
        },
      ),
    ],
  },
  {
    family_id: "helper-graph",
    surfaces: ["helper-factory"],
    description:
      "the five-helper graph of the foundation's factory control observed at emit level: all helper bodies inline exactly once in dependency order; noEmitHelpers suppresses the bodies while the references remain; importHelpers on a module without a resolvable tslib reports the typed helper fault and emits the tslib import instead of inline bodies",
    cases: [
      witnessCase(
        "positive-all-five",
        "positive",
        "extends + for-of + spread + tuple read + generator delegation inline all five helper bodies once",
        [
          "class HelperBase {}",
          "class HelperDerived extends HelperBase {}",
          "declare const iterable: Iterable<number>;",
          "for (const item of iterable) { void item; }",
          "const copied = [...iterable];",
          "const [firstCopied] = copied;",
          "function* produced() { yield* iterable; }",
          "void firstCopied; void produced;",
          "",
        ].join("\n"),
        {
          markers: [
            marker("extendStatics", "present"),
            marker("__values", "present"),
            marker("__read", "present"),
            marker("__spreadArray", "present"),
            marker("__generator", "present"),
          ],
        },
      ),
      witnessCase(
        "adjacent-negative-no-emit-helpers",
        "adjacent-negative-control",
        "noEmitHelpers keeps every helper reference while suppressing the inline helper bodies",
        [
          "class HelperBase {}",
          "class HelperDerived extends HelperBase {}",
          "declare const iterable: Iterable<number>;",
          "for (const item of iterable) { void item; }",
          "",
        ].join("\n"),
        {
          option_overrides: { noEmitHelpers: true },
          markers: [
            marker("__extends", "present"),
            marker("extendStatics", "absent"),
          ],
        },
      ),
      witnessCase(
        "fault-import-helpers-missing-tslib",
        "fault",
        "importHelpers on a module without tslib reports the typed helper fault and emits the tslib import in place of inline bodies",
        [
          "export class HelperBase {}",
          "export class HelperDerived extends HelperBase {}",
          "",
        ].join("\n"),
        {
          option_overrides: { importHelpers: true },
          markers: [
            marker("tslib", "present"),
            marker("extendStatics", "absent"),
          ],
          expected_reported_codes: [2354],
        },
      ),
    ],
  },
  {
    family_id: "name-generation",
    surfaces: ["name-generation", "factory-construction"],
    description:
      "deferred generated-name allocation observed at emit level: source-occupied _a/_b/_i/_super push the allocator to later names for error temps and the class-expression binding, while the free control uses the first names; the cross-owner case interleaves ES2015 loop-conversion names with Generators state temps in one allocation sequence",
    cases: [
      witnessCase(
        "positive-occupied-allocator",
        "positive",
        "source declarations occupying _a/_b/_i/_super force the allocator past them for the class-expression temp and iteration error temps",
        [
          "let _a = 1, _b = 2, _i = 3, _super = 4;",
          "const Holder = class { static marker = _a + _b + _i + _super; };",
          "declare const entries: number[];",
          "for (const entry of entries) { void (() => entry); }",
          "void Holder;",
          "",
        ].join("\n"),
        {
          markers: [marker("_super", "present")],
        },
      ),
      witnessCase(
        "adjacent-negative-free-allocator",
        "adjacent-negative-control",
        "the same shapes without occupied names let the allocator use the first generated names",
        [
          "const Holder = class { static marker = 0; };",
          "declare const entries: number[];",
          "for (const entry of entries) { void (() => entry); }",
          "void Holder;",
          "",
        ].join("\n"),
        {
          markers: [marker("_super", "absent")],
        },
      ),
      witnessCase(
        "composition-cross-owner-allocation",
        "composition",
        "a generator for-of with capture and occupied _a/_b interleaves loop-conversion names, iteration temps, and state-machine temps in one cross-owner allocation order (including the for-of yield* synthesis site)",
        [
          "let _a = 1, _b = 2;",
          "declare const entries: number[];",
          "function* interleaved() {",
          "  for (const entry of entries) {",
          "    void (() => entry);",
          "    yield entry;",
          "  }",
          "}",
          "void _a; void _b;",
          "",
        ].join("\n"),
        {
          markers: [
            marker("_loop_1", "present"),
            marker("__generator", "present"),
          ],
          composition_edges: ["pass-order", "yield-star-synthesis"],
        },
      ),
    ],
  },
  {
    family_id: "resolver-foundation-controls",
    surfaces: ["resolver-collision-capture-queries", "resolver-node-check-flags"],
    description:
      "emit-level converses of the foundation's resolver direct controls, input-byte-identical to the frozen controls with the emitted bytes proven equal to the foundation's stored writes: colliding block-scoped names rename, captured loop bindings extract the converted loop, and the strict-mode arguments/catch control emits under its recorded grammar faults; the calm control renames nothing",
    cases: [
      witnessCase(
        "positive-collision-rename",
        "positive",
        "the foundation collision control's shadowing let is renamed in the emitted bytes (isDeclarationWithCollidingName converse)",
        FOUNDATION_COLLISION_SOURCE,
        {
          markers: [marker("collisionValue_1", "present")],
          foundation_control: "checker-colliding-block-scope",
        },
      ),
      witnessCase(
        "positive-captured-loop",
        "positive",
        "the foundation captured-loop control extracts the converted loop function (isBindingCapturedByNode/hasNodeCheckFlag converse)",
        FOUNDATION_CAPTURED_SOURCE,
        {
          markers: [marker("_loop_1", "present")],
          foundation_control: "checker-captured-loop-bindings",
        },
      ),
      witnessCase(
        "adjacent-negative-no-collision",
        "adjacent-negative-control",
        "a non-shadowing block and an uncaptured var loop emit without renames or loop extraction",
        [
          "declare function use(value: unknown): void;",
          "function calmScope() {",
          "  var calmValue = 0;",
          "  {",
          "    use(calmValue);",
          "  }",
          "}",
          "for (var plainIndex = 0; plainIndex < 2; plainIndex++) {",
          "  use(plainIndex);",
          "}",
          "",
        ].join("\n"),
        {
          markers: [
            marker("calmValue", "present"),
            marker("_loop_1", "absent"),
          ],
        },
      ),
      witnessCase(
        "fault-arguments-and-catch",
        "fault",
        "the foundation arguments/catch control emits its generator catch delegation under the recorded strict-mode grammar faults",
        FOUNDATION_ARGUMENTS_SOURCE,
        {
          markers: [marker("caughtValue", "present")],
          expected_reported_codes: [1100, 2496],
          foundation_control: "checker-arguments-and-catch-reference",
        },
      ),
    ],
  },
  {
    family_id: "hook-chains",
    surfaces: ["hook-composition"],
    description:
      "chained onSubstituteNode/onEmitNode effects observed at emit level: generator var hoisting splits an assignment across state labels; an ES2015 block-scope rename survives into the Generators state machine only because both owners' hooks delegate in the pinned order; the plain-function control renames in place without a state machine",
    cases: [
      witnessCase(
        "positive-generator-hoisting",
        "positive",
        "a generator-local var is hoisted out of the state machine and a compound assignment across yield splits into a pre-yield temp and post-resume write",
        [
          "function* accumulate(): Generator<number, number, number> {",
          "  var runningTotal = 0;",
          "  runningTotal += yield runningTotal;",
          "  return runningTotal;",
          "}",
          "",
        ].join("\n"),
        {
          markers: [
            marker("runningTotal", "present"),
            marker("__generator", "present"),
          ],
        },
      ),
      witnessCase(
        "adjacent-negative-blockscope-rename-plain",
        "adjacent-negative-control",
        "the same shadowing rename in a plain function stays in place without any state machine",
        [
          "function walker(): number {",
          "  var shadowed = 0;",
          "  {",
          "    let shadowed = 10;",
          "    void shadowed;",
          "  }",
          "  return shadowed;",
          "}",
          "",
        ].join("\n"),
        {
          markers: [
            marker("shadowed_1", "present"),
            marker("__generator", "absent"),
          ],
        },
      ),
      witnessCase(
        "composition-blockscope-rename-in-generator",
        "composition",
        "an ES2015 block-scope rename inside a generator body survives into the hoisted state-machine bytes through the chained substitution order",
        [
          "function* walker(): Generator<number, number, void> {",
          "  var shadowed = 0;",
          "  {",
          "    let shadowed = 10;",
          "    yield shadowed;",
          "  }",
          "  return shadowed;",
          "}",
          "",
        ].join("\n"),
        {
          markers: [
            marker("shadowed_1", "present"),
            marker("__generator", "present"),
          ],
          composition_edges: ["substitution-chain", "pass-order"],
        },
      ),
    ],
  },
  {
    family_id: "enum-pair-guards",
    surfaces: ["transform-flag-recomputation", "syntax-guards"],
    description:
      "transform-flag guards select subtrees: in a mixed file only the ES2015-flagged arrow is rewritten while the ES5 subtrees are preserved, and a fully-ES5 file passes through with no helper or generated temp at all",
    cases: [
      witnessCase(
        "positive-mixed-subtrees",
        "positive",
        "a mixed file rewrites the arrow subtree and preserves the var/function subtrees",
        [
          "var legacyCounter = 0;",
          "function legacyIncrement() { legacyCounter += 1; return legacyCounter; }",
          "const modernIncrement = () => legacyIncrement();",
          "void modernIncrement;",
          "",
        ].join("\n"),
        {
          markers: [marker("legacyIncrement", "present")],
        },
      ),
      witnessCase(
        "adjacent-negative-plain-es5",
        "adjacent-negative-control",
        "a fully-ES5 file passes through with the prologue only: no double-underscore helper or temp appears",
        [
          "var plainValue = 0;",
          "function plainIncrement() { plainValue += 1; return plainValue; }",
          "plainIncrement();",
          "",
        ].join("\n"),
        {
          markers: [
            marker("plainIncrement", "present"),
            marker("__", "absent"),
          ],
        },
      ),
    ],
  },
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

function fingerprintIsValid(record, field) {
  if (record === null || typeof record !== "object") return false;
  const { [field]: storedFingerprint, ...rest } = record;
  return (
    typeof storedFingerprint === "string" &&
    storedFingerprint === sha256(Buffer.from(canonical(rest), "utf8"))
  );
}

function libraryInventoryRecord() {
  // The fresh-process observations resolve default libraries from disk
  // through the base compiler host; those .d.ts bytes drive the type
  // check but are not covered by the bundle/implementation hashes, so
  // the record pins the whole vendored lib inventory (gate-tax 2 R3-2).
  const directory = path.join(WORKSPACE, TYPESCRIPT_LIB_DIRECTORY);
  const names = fs
    .readdirSync(directory)
    .filter((name) => name.startsWith("lib.") && name.endsWith(".d.ts"))
    .sort();
  requireCondition(
    names.length > 0,
    "vendored TypeScript lib inventory is empty",
  );
  const hash = crypto.createHash("sha256");
  for (const name of names) {
    hash.update(name);
    hash.update("\u0000");
    hash.update(fs.readFileSync(path.join(directory, name)));
    hash.update("\u0000");
  }
  return {
    path: TYPESCRIPT_LIB_DIRECTORY,
    default_libraries: names.length,
    sha256: hash.digest("hex"),
  };
}

function typescriptRecord() {
  return {
    version: ts.version,
    source_commit: SOURCE_COMMIT,
    bundle: pathHash(TYPESCRIPT_BUNDLE),
    implementation: pathHash(TYPESCRIPT_IMPLEMENTATION),
    lib: libraryInventoryRecord(),
  };
}

function writeFileAtomic(absolutePath, contents) {
  // Same-directory temp + rename: a kill mid-write can never truncate
  // the artifact, which doubles as the adoption store (gate-tax 2 R4-1).
  const temporary = path.join(
    path.dirname(absolutePath),
    `.${path.basename(absolutePath)}.tmp`,
  );
  // The name is deterministic (no pid): artifact writes are
  // single-writer by walk discipline, and a stray temp left by a kill
  // is overwritten by the next successful write instead of
  // accumulating as untracked residue.
  fs.writeFileSync(temporary, contents);
  fs.renameSync(temporary, absolutePath);
}

let adoptedCases = 0;

// Write-side observation adoption (gate-tax 2). The adoption key is
// all-or-nothing and deliberately stricter than the 5g per-case
// fallback: the stored generator sha must byte-match this file (the
// case specs and transforms live here, and unlike 5g there is no
// per-gate --check backstop, only the once-per-slice packet checker),
// and the stored typescript record must byte-match the current one
// including the library inventory. Only the fresh-process oracle
// observations are adopted; every derivation, marker expectation,
// census guard, and lineage pin re-executes against current inputs on
// every write. --check never adopts: the packet checker's full
// re-observation remains the slice-boundary backstop.
function reusableStoredObservations(currentTypescriptRecord) {
  if (mode !== "--write") return null;
  const targetPath = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
  if (!fs.existsSync(targetPath)) return null;
  let stored;
  try {
    stored = JSON.parse(fs.readFileSync(targetPath, "utf8"));
  } catch {
    return null;
  }
  if (
    stored === null ||
    typeof stored !== "object" ||
    stored.schema !== 1 ||
    stored.kind !== "h2-es2015-generators-witnesses" ||
    !fingerprintIsValid(stored, "witnesses_fingerprint_sha256") ||
    canonical(stored.generator) !==
      canonical(pathHash(GENERATOR_RELATIVE_PATH)) ||
    canonical(stored.typescript) !== canonical(currentTypescriptRecord)
  ) {
    return null;
  }
  const observations = new Map();
  for (const family of stored.families ?? []) {
    for (const storedCase of family.cases ?? []) {
      if (
        typeof storedCase.case_id === "string" &&
        fingerprintIsValid(
          storedCase.observation,
          "observation_fingerprint_sha256",
        )
      ) {
        observations.set(storedCase.case_id, storedCase.observation);
      }
    }
  }
  return observations;
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

function bytesRecord(text) {
  const bytes = Buffer.from(text, "utf8");
  return {
    utf8_base64: bytes.toString("base64"),
    utf8_sha256: sha256(bytes),
    utf8_bytes: bytes.length,
  };
}

function serializeOptions(options) {
  return Object.fromEntries(
    Object.entries(options).sort(([left], [right]) => left.localeCompare(right)),
  );
}

function validateRuntime() {
  const node = readText(".node-version").trim();
  requireCondition(node === EXPECTED_NODE, "unexpected Node pin");
  requireCondition(process.version === `v${node}`, `requires Node ${node}`);
  requireCondition(ts.version === "6.0.3", "unexpected TypeScript runtime");
  for (const name of [
    "createProgram",
    "createCompilerHost",
    "createSourceFile",
    "getPreEmitDiagnostics",
    "normalizePath",
    "getScriptKindFromFileName",
  ]) {
    requireCondition(
      typeof ts[name] === "function",
      `pinned TypeScript does not expose ${name}`,
    );
  }
}

function hasDirectory(files, directory) {
  const prefix = `${ts.normalizePath(directory).replace(/\/$/, "")}/`;
  return [...files.keys()].some((fileName) => fileName.startsWith(prefix));
}

function createVirtualProgram(control) {
  const files = new Map(control.files.map((file) => [file.path, file.text]));
  const baseHost = ts.createCompilerHost(control.compiler_options, true);
  const host = {
    ...baseHost,
    getCurrentDirectory: () => "/project",
    useCaseSensitiveFileNames: () => true,
    getCanonicalFileName: (fileName) => fileName,
    trace() {},
    fileExists(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.has(normalized) || baseHost.fileExists(normalized);
    },
    readFile(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.get(normalized) ?? baseHost.readFile(normalized);
    },
    directoryExists(directory) {
      return (
        hasDirectory(files, directory) ||
        (baseHost.directoryExists?.(directory) ?? false)
      );
    },
    getDirectories(directory) {
      return baseHost.getDirectories?.(directory) ?? [];
    },
    realpath(fileName) {
      const normalized = ts.normalizePath(fileName);
      return files.has(normalized)
        ? normalized
        : (baseHost.realpath?.(normalized) ?? normalized);
    },
    getSourceFile(fileName, languageVersion) {
      const normalized = ts.normalizePath(fileName);
      const text = files.get(normalized);
      if (text === undefined) {
        return baseHost.getSourceFile(fileName, languageVersion);
      }
      return ts.createSourceFile(
        normalized,
        text,
        languageVersion,
        true,
        ts.getScriptKindFromFileName(normalized),
      );
    },
  };
  return ts.createProgram(control.roots, control.compiler_options, host);
}

function serializeDiagnostic(diagnostic, phase) {
  return {
    phase,
    code: diagnostic.code,
    category: ts.DiagnosticCategory[diagnostic.category],
    file: diagnostic.file ? ts.normalizePath(diagnostic.file.fileName) : null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
  };
}

function serializeWrite(arguments_, index) {
  const [fileName, text, writeByteOrderMark, onError, sourceFiles] = arguments_;
  const callback = Buffer.from(text, "utf8");
  const materialized = writeByteOrderMark
    ? Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), callback])
    : callback;
  return {
    index,
    path: ts.normalizePath(fileName),
    callback_utf8_base64: callback.toString("base64"),
    callback_utf8_sha256: sha256(callback),
    callback_utf8_bytes: callback.length,
    write_byte_order_mark: writeByteOrderMark,
    materialized_utf8_sha256: sha256(materialized),
    materialized_utf8_bytes: materialized.length,
    on_error_callback_present: typeof onError === "function",
    source_files: (sourceFiles ?? []).map((source) =>
      ts.normalizePath(source.fileName),
    ),
  };
}

function countOccurrences(text, token) {
  let count = 0;
  let cursor = text.indexOf(token);
  while (cursor !== -1) {
    count += 1;
    cursor = text.indexOf(token, cursor + token.length);
  }
  return count;
}

function witnessControl(caseSpec) {
  return {
    files: [{ path: "/project/input.ts", text: caseSpec.source }],
    roots: ["/project/input.ts"],
    compiler_options: compilerOptions(caseSpec.option_overrides),
  };
}

function observeWitnessCase(caseSpec) {
  const control = witnessControl(caseSpec);
  const program = createVirtualProgram(control);
  const reportedDiagnostics = ts.getPreEmitDiagnostics(program);
  const writes = [];
  const emitResult = program.emit(
    undefined,
    (...arguments_) => writes.push(arguments_),
    undefined,
    false,
    {},
  );
  const observedCodes = reportedDiagnostics
    .map((diagnostic) => diagnostic.code)
    .sort((left, right) => left - right);
  requireCondition(
    canonical(observedCodes) === canonical(caseSpec.expected_reported_codes),
    `${caseSpec.case_id} reported [${observedCodes.join(", ")}], expected [${caseSpec.expected_reported_codes.join(", ")}]`,
  );
  requireCondition(
    emitResult.diagnostics.length === 0 && emitResult.emitSkipped === false,
    `${caseSpec.case_id} emit did not complete`,
  );
  requireCondition(
    writes.length === 1,
    `${caseSpec.case_id} produced ${writes.length} writes`,
  );
  const serializedWrites = writes.map(serializeWrite);
  requireCondition(
    serializedWrites[0].path === "/project/input.js",
    `${caseSpec.case_id} wrote ${serializedWrites[0].path}`,
  );
  const output = Buffer.from(
    serializedWrites[0].callback_utf8_base64,
    "base64",
  ).toString("utf8");
  const markerOccurrences = caseSpec.markers.map(({ token }) => ({
    token,
    occurrences: countOccurrences(output, token),
  }));
  return withFingerprint(
    {
      reported_diagnostics: reportedDiagnostics.map((diagnostic) =>
        serializeDiagnostic(diagnostic, "pre-emit"),
      ),
      emit_diagnostics: emitResult.diagnostics.map((diagnostic) =>
        serializeDiagnostic(diagnostic, "emit"),
      ),
      emit_skipped: emitResult.emitSkipped,
      writes: serializedWrites,
      marker_occurrences: markerOccurrences,
    },
    "observation_fingerprint_sha256",
  );
}

function observeWitnessCaseInFreshProcess(caseSpec) {
  const stdout = execFileSync(
    process.execPath,
    [GENERATOR_PATH, INTERNAL_OBSERVE_MODE, caseSpec.case_id],
    {
      cwd: WORKSPACE,
      encoding: "utf8",
      maxBuffer: 128 * 1024 * 1024,
    },
  );
  const observation = JSON.parse(stdout);
  requireCondition(
    observation !== null &&
      typeof observation === "object" &&
      typeof observation.observation_fingerprint_sha256 === "string",
    `${caseSpec.case_id} fresh TypeScript observation is invalid`,
  );
  return observation;
}

function buildWitnessCase(caseSpec, foundationControls, adoptedObservations) {
  // Generated-name allocation and helper priority are process-global
  // printer state: every repetition runs in a fresh Node isolate, exactly
  // like the foundation's direct controls and the comment-scope witnesses.
  const adopted = adoptedObservations?.get(caseSpec.case_id) ?? null;
  let first;
  if (adopted !== null) {
    // Adoption skips only the two fresh-process oracle runs; the stored
    // observation fingerprint stands for the repetitions=2 determinism
    // proof, exactly as the 5g reuse does. Every marker expectation and
    // foundation-control cross-check below still executes against the
    // adopted record and the CURRENT foundation artifact.
    adoptedCases += 1;
    first = adopted;
  } else {
    first = observeWitnessCaseInFreshProcess(caseSpec);
    const second = observeWitnessCaseInFreshProcess(caseSpec);
    requireCondition(
      first.observation_fingerprint_sha256 ===
        second.observation_fingerprint_sha256,
      `${caseSpec.case_id} TypeScript observation is nondeterministic`,
    );
  }
  for (const markerSpec of caseSpec.markers) {
    const observed = first.marker_occurrences.find(
      (entry) => entry.token === markerSpec.token,
    );
    requireCondition(
      observed !== undefined,
      `${caseSpec.case_id} marker ${markerSpec.token} was not observed`,
    );
    if (markerSpec.expectation === "present") {
      requireCondition(
        observed.occurrences > 0,
        `${caseSpec.case_id} marker ${markerSpec.token} is absent from the oracle output`,
      );
    } else if (markerSpec.expectation === "absent") {
      requireCondition(
        observed.occurrences === 0,
        `${caseSpec.case_id} marker ${markerSpec.token} appears ${observed.occurrences} times in the oracle output`,
      );
    } else {
      requireCondition(
        markerSpec.expectation === "recorded",
        `${caseSpec.case_id} marker ${markerSpec.token} has unknown expectation`,
      );
    }
  }
  const control = witnessControl(caseSpec);
  if (caseSpec.foundation_control !== null) {
    const foundationControl = foundationControls.get(
      caseSpec.foundation_control,
    );
    requireCondition(
      foundationControl !== undefined,
      `${caseSpec.case_id} cites unknown foundation control ${caseSpec.foundation_control}`,
    );
    requireCondition(
      foundationControl.input.files.length === 1 &&
        foundationControl.input.files[0].utf8_sha256 ===
          bytesRecord(caseSpec.source).utf8_sha256,
      `${caseSpec.case_id} input drifted from foundation control ${caseSpec.foundation_control}`,
    );
    requireCondition(
      canonical(foundationControl.input.compiler_options) ===
        canonical(serializeOptions(control.compiler_options)),
      `${caseSpec.case_id} compiler options drifted from foundation control ${caseSpec.foundation_control}`,
    );
    requireCondition(
      foundationControl.observation.writes.length === 1 &&
        foundationControl.observation.writes[0].callback_utf8_sha256 ===
          first.writes[0].callback_utf8_sha256,
      `${caseSpec.case_id} emitted bytes diverge from foundation control ${caseSpec.foundation_control}`,
    );
  }
  return withFingerprint(
    {
      case_id: caseSpec.case_id,
      role: caseSpec.role,
      description: caseSpec.description,
      option_overrides: serializeOptions(caseSpec.option_overrides),
      expected_reported_codes: caseSpec.expected_reported_codes,
      composition_edges: caseSpec.composition_edges,
      foundation_control: caseSpec.foundation_control,
      input: {
        current_directory: "/project",
        roots: control.roots,
        files: control.files.map((file) => ({
          path: file.path,
          root: control.roots.includes(file.path),
          ...bytesRecord(file.text),
        })),
        compiler_options: serializeOptions(control.compiler_options),
      },
      markers: caseSpec.markers,
      repetitions: 2,
      observation: first,
    },
    "case_fingerprint_sha256",
  );
}

function flattenCaseSpecs() {
  const specs = [];
  for (const family of WITNESS_FAMILY_SPECS) {
    for (const caseSpec of family.cases) {
      specs.push({
        ...caseSpec,
        case_id: `${family.family_id}--${caseSpec.case_slug}`,
        family_id: family.family_id,
      });
    }
  }
  const ids = new Set(specs.map((spec) => spec.case_id));
  requireCondition(ids.size === specs.length, "witness case ids collide");
  for (const spec of specs) {
    requireCondition(
      ROLES.includes(spec.role),
      `${spec.case_id} has unknown role ${spec.role}`,
    );
    requireCondition(
      (spec.role === "fault") === (spec.expected_reported_codes.length > 0),
      `${spec.case_id} fault role and expected diagnostics disagree`,
    );
    requireCondition(
      (spec.role === "composition") === (spec.composition_edges.length > 0),
      `${spec.case_id} composition role and cited edges disagree`,
    );
    const sortedCodes = [...spec.expected_reported_codes].sort(
      (left, right) => left - right,
    );
    requireCondition(
      canonical(sortedCodes) === canonical(spec.expected_reported_codes),
      `${spec.case_id} expected diagnostics are not sorted`,
    );
    const tokens = spec.markers.map((entry) => entry.token);
    requireCondition(
      new Set(tokens).size === tokens.length,
      `${spec.case_id} marker tokens collide`,
    );
    for (const left of tokens) {
      for (const right of tokens) {
        requireCondition(
          left === right || !right.includes(left),
          `${spec.case_id} marker token ${left} is a substring of ${right} - occurrence counts would be polluted`,
        );
      }
    }
  }
  return specs;
}

function validateFoundationLineage() {
  const foundation = readJson(FOUNDATION_RELATIVE_PATH);
  requireCondition(
    foundation.schema === 1 &&
      foundation.kind === "h2-dormant-semantic-foundation" &&
      foundation.status === "frozen-dormant-semantic-foundation" &&
      foundation.phase === SLICE &&
      foundation.slice_id === SLICE &&
      typeof foundation.foundation_fingerprint_sha256 === "string" &&
      foundation.typescript.version === ts.version &&
      foundation.typescript.source_commit === SOURCE_COMMIT &&
      canonical(foundation.typescript.bundle) ===
        canonical(pathHash(TYPESCRIPT_BUNDLE)) &&
      canonical(foundation.typescript.implementation) ===
        canonical(pathHash(TYPESCRIPT_IMPLEMENTATION)) &&
      Array.isArray(foundation.direct_controls),
    "H2.5h-a foundation lineage is not closed",
  );
  return {
    artifact: pathHash(FOUNDATION_RELATIVE_PATH),
    foundation_fingerprint_sha256: foundation.foundation_fingerprint_sha256,
    controls: new Map(
      foundation.direct_controls.map((control) => [
        control.control_id,
        control,
      ]),
    ),
  };
}

function validateOwnerGraphLineage() {
  const graph = readJson(OWNER_GRAPH_RELATIVE_PATH);
  requireCondition(
    graph.schema === 1 &&
      graph.kind === "h2-owner-graph" &&
      graph.status === "frozen-owner-graph" &&
      graph.phase === SLICE &&
      graph.slice_id === SLICE &&
      typeof graph.owner_graph_fingerprint_sha256 === "string" &&
      graph.typescript.version === ts.version &&
      graph.typescript.source_commit === SOURCE_COMMIT &&
      canonical(graph.typescript.bundle) ===
        canonical(pathHash(TYPESCRIPT_BUNDLE)) &&
      canonical(graph.typescript.implementation) ===
        canonical(pathHash(TYPESCRIPT_IMPLEMENTATION)),
    "H2.5h-a owner-graph lineage is not closed",
  );
  const edgeIds = graph.composition_edges.map((edge) => edge.edge_id).sort();
  requireCondition(
    canonical(edgeIds) === canonical([...EDGE_IDS].sort()),
    "owner-graph composition edge census changed",
  );
  const surfaceIds = graph.surface_row_assignments
    .map((surface) => surface.surface_id)
    .sort();
  requireCondition(
    canonical(surfaceIds) ===
      canonical(
        [
          ...REQUIRED_SURFACES,
          ...EXCLUDED_SURFACES.map((entry) => entry.surface_id),
        ].sort(),
      ),
    "owner-graph surface census is not exactly partitioned by this witness set",
  );
  const yieldStarEdge = graph.composition_edges.find(
    (edge) => edge.edge_id === "yield-star-synthesis",
  );
  requireCondition(
    Array.isArray(yieldStarEdge.owner_relative_offsets) &&
      yieldStarEdge.owner_relative_offsets.length === 2,
    "pinned yield* synthesis site count changed",
  );
  const es2015Owner = graph.owners.find(
    (owner) => owner.key === "transform-es2015",
  );
  requireCondition(
    es2015Owner !== undefined &&
      typeof es2015Owner.declaration?.source_range?.start?.offset === "number",
    "owner-graph transform-es2015 declaration pin changed",
  );
  return {
    artifact: pathHash(OWNER_GRAPH_RELATIVE_PATH),
    owner_graph_fingerprint_sha256: graph.owner_graph_fingerprint_sha256,
    yield_star_relative_offsets: yieldStarEdge.owner_relative_offsets,
    es2015_declaration_start_offset:
      es2015Owner.declaration.source_range.start.offset,
  };
}

function pinYieldStarSites(ownerGraph, specs) {
  const implementationText = readText(TYPESCRIPT_IMPLEMENTATION);
  const specIds = new Set(specs.map((spec) => spec.case_id));
  return YIELD_STAR_SITES.map((site) => {
    const relativeOffset =
      ownerGraph.yield_star_relative_offsets[site.site_index];
    const absoluteOffset =
      ownerGraph.es2015_declaration_start_offset + relativeOffset;
    const preceding = implementationText.slice(
      Math.max(0, absoluteOffset - 2000),
      absoluteOffset,
    );
    const enclosingMatches = [
      ...preceding.matchAll(/function (generateCallToConvertedLoop\w*)\(/gu),
    ];
    requireCondition(
      enclosingMatches.length > 0 &&
        enclosingMatches[enclosingMatches.length - 1][1] ===
          site.enclosing_function,
      `yield* site ${site.site_index} is no longer inside ${site.enclosing_function}`,
    );
    requireCondition(
      implementationText
        .slice(absoluteOffset, absoluteOffset + 400)
        .includes("createYieldExpression("),
      `yield* site ${site.site_index} no longer synthesizes a yield expression`,
    );
    requireCondition(
      specIds.has(site.covering_case_id),
      `yield* site ${site.site_index} covering case ${site.covering_case_id} does not exist`,
    );
    return {
      site_index: site.site_index,
      owner_relative_offset: relativeOffset,
      enclosing_function: site.enclosing_function,
      covering_case_id: site.covering_case_id,
    };
  });
}

function buildFamilies(foundationControls, adoptedObservations) {
  const specs = flattenCaseSpecs();
  const casesById = new Map(
    specs.map((spec) => [
      spec.case_id,
      buildWitnessCase(spec, foundationControls, adoptedObservations),
    ]),
  );
  return WITNESS_FAMILY_SPECS.map((family) => {
    const cases = family.cases.map((caseSpec) =>
      casesById.get(`${family.family_id}--${caseSpec.case_slug}`),
    );
    const roles = cases.map((item) => item.role);
    requireCondition(
      roles.includes("positive") &&
        roles.includes("adjacent-negative-control"),
      `${family.family_id} is missing a positive or adjacent-negative case`,
    );
    const writeHashes = cases.map(
      (item) => item.observation.writes[0].callback_utf8_sha256,
    );
    requireCondition(
      new Set(writeHashes).size === writeHashes.length,
      `${family.family_id} contains byte-identical case outputs - the family is vacuous`,
    );
    for (const surface of family.surfaces) {
      requireCondition(
        REQUIRED_SURFACES.includes(surface),
        `${family.family_id} cites unlisted surface ${surface}`,
      );
    }
    return withFingerprint(
      {
        family_id: family.family_id,
        surfaces: [...family.surfaces].sort(),
        description: family.description,
        cases,
      },
      "family_fingerprint_sha256",
    );
  });
}

function buildArtifact() {
  validateRuntime();
  const foundation = validateFoundationLineage();
  const ownerGraph = validateOwnerGraphLineage();
  const specs = flattenCaseSpecs();
  const yieldStarSites = pinYieldStarSites(ownerGraph, specs);
  const typescript = typescriptRecord();
  const families = buildFamilies(
    foundation.controls,
    reusableStoredObservations(typescript),
  );
  const cases = families.flatMap((family) => family.cases);
  const roleCount = (role) =>
    cases.filter((item) => item.role === role).length;
  const citedEdges = [
    ...new Set(cases.flatMap((item) => item.composition_edges)),
  ].sort();
  requireCondition(
    canonical(citedEdges) === canonical([...EDGE_IDS].sort()),
    "composition edge coverage changed",
  );
  const citedSurfaces = [
    ...new Set(families.flatMap((family) => family.surfaces)),
  ].sort();
  requireCondition(
    canonical(citedSurfaces) === canonical([...REQUIRED_SURFACES].sort()),
    "required surface coverage changed",
  );
  requireCondition(
    families.length === 9 && cases.length === 32,
    "witness census changed",
  );
  requireCondition(
    roleCount("positive") === 10 &&
      roleCount("adjacent-negative-control") === 9 &&
      roleCount("composition") === 7 &&
      roleCount("fault") === 6,
    "witness role census changed",
  );
  const foundationControlCases = cases.filter(
    (item) => item.foundation_control !== null,
  );
  requireCondition(
    foundationControlCases.length === 3,
    "foundation control cross-check census changed",
  );
  return withFingerprint(
    {
      schema: 1,
      kind: "h2-es2015-generators-witnesses",
      status: "frozen-es2015-generators-witnesses",
      phase: SLICE,
      slice_id: SLICE,
      sub_packet: SUB_PACKET,
      plan_step: "step-6-witness-freeze",
      typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      lineage: {
        foundation: foundation.artifact,
        foundation_fingerprint_sha256:
          foundation.foundation_fingerprint_sha256,
        owner_graph: ownerGraph.artifact,
        owner_graph_fingerprint_sha256:
          ownerGraph.owner_graph_fingerprint_sha256,
        handoff: pathHash(HANDOFF_RELATIVE_PATH),
        interpretation:
          "W-H2.5H step 6 freezes oracle-captured ES2015/Generators lowering bytes; it authorizes no production edit and activates nothing",
      },
      yield_star_sites: yieldStarSites,
      excluded_surfaces: EXCLUDED_SURFACES,
      families,
      summary: {
        families: families.length,
        cases: cases.length,
        positive_cases: roleCount("positive"),
        adjacent_negative_controls: roleCount("adjacent-negative-control"),
        composition_cases: roleCount("composition"),
        fault_cases: roleCount("fault"),
        composition_edges_covered: citedEdges,
        required_surfaces_covered: citedSurfaces,
        excluded_surfaces: EXCLUDED_SURFACES.map((entry) => entry.surface_id),
        foundation_control_cross_checks: foundationControlCases.length,
        typescript_oracle_runs: cases.reduce(
          (sum, item) => sum + item.repetitions,
          0,
        ),
        rust_runs: 0,
        runtime_admissions_delta: 0,
      },
    },
    "witnesses_fingerprint_sha256",
  );
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

const mode = process.argv[2];
if (mode === INTERNAL_OBSERVE_MODE) {
  requireCondition(
    process.argv.length === 4,
    "internal witness observation requires one case id",
  );
  validateRuntime();
  const caseSpec = flattenCaseSpecs().find(
    (item) => item.case_id === process.argv[3],
  );
  requireCondition(
    caseSpec !== undefined,
    `unknown internal witness case ${process.argv[3]}`,
  );
  process.stdout.write(render(observeWitnessCase(caseSpec)));
} else {
  requireCondition(
    mode === "--write" || mode === "--check",
    "usage: h2-5h-a-es2015-generators-witnesses.mjs [--write|--check]",
  );
  const artifact = buildArtifact();
  const rendered = render(artifact);
  if (mode === "--write") {
    writeFileAtomic(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
    process.stdout.write(
      `wrote ${TARGET_RELATIVE_PATH}: families=${artifact.summary.families} cases=${artifact.summary.cases} oracle_runs=${artifact.summary.typescript_oracle_runs} adopted_cases=${adoptedCases} oracle_runs_saved=${adoptedCases * 2}\n`,
    );
  } else {
    requireCondition(
      fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
        fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") ===
          rendered,
      `stale ${TARGET_RELATIVE_PATH}; run h2-5h-a-es2015-generators-witnesses.mjs --write and review`,
    );
    process.stdout.write(
      `H2.5h-a ES2015/Generators witnesses are fresh: families=${artifact.summary.families} cases=${artifact.summary.cases}\n`,
    );
  }
}
