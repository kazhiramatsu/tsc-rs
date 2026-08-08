import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h1-rust-omission-inventory.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h1-rust-omissions.v1.json";
const TARGET_PATH = path.join(WORKSPACE, TARGET_RELATIVE_PATH);
const SCHEMA_RELATIVE_PATH =
  ".github/ci/contracts/h1-rust-omissions.schema.json";
const DESIGN_RELATIVE_PATH =
  "docs/design/greenfield/compiler-compatibility-residual.md";
const H1_DESIGN_RELATIVE_PATH = "docs/design/greenfield/h1-emit.md";
const PROFILE_RELATIVE_PATH = "ratchets/h1-emit-profile.v1.json";
const OWNER_RELATIVE_PATH = "ratchets/h1-owner-inventory.v1.json";
const TYPESCRIPT_VERSION = "6.0.3";
const SOURCE_COMMIT = "050880ce59e30b356b686bd3144efe24f875ebc8";

function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function relativePath(value) {
  return path.relative(WORKSPACE, value).split(path.sep).join("/");
}

function fileBytes(relative) {
  return fs.readFileSync(path.join(WORKSPACE, relative));
}

function fileText(relative) {
  return fileBytes(relative).toString("utf8");
}

function pathHash(relative) {
  return { path: relative, sha256: sha256(fileBytes(relative)) };
}

function compareDirectoryEntries(left, right) {
  if (left.name < right.name) return -1;
  if (left.name > right.name) return 1;
  return 0;
}

function walkFiles(directory, accept, output = []) {
  for (const entry of fs
    .readdirSync(directory, { withFileTypes: true })
    .sort(compareDirectoryEntries)) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      walkFiles(entryPath, accept, output);
    } else if (entry.isFile() && accept(entryPath)) {
      output.push(relativePath(entryPath));
    }
  }
  return output;
}

function productionInventory() {
  const files = ["Cargo.toml"];
  const crates = path.join(WORKSPACE, "crates");
  for (const entry of fs
    .readdirSync(crates, { withFileTypes: true })
    .filter((candidate) => candidate.isDirectory())
    .sort(compareDirectoryEntries)) {
    const crateRoot = path.join(crates, entry.name);
    const manifest = path.join(crateRoot, "Cargo.toml");
    if (fs.existsSync(manifest)) files.push(relativePath(manifest));
    const sourceRoot = path.join(crateRoot, "src");
    if (fs.existsSync(sourceRoot)) {
      walkFiles(sourceRoot, (candidate) => candidate.endsWith(".rs"), files);
    }
  }
  files.sort();
  requireCondition(files.length > 100, "production Rust inventory is unexpectedly small");
  requireCondition(new Set(files).size === files.length, "production inventory has duplicates");
  const rows = files.map((file) => {
    const bytes = fileBytes(file);
    return { path: file, bytes: bytes.length, sha256: sha256(bytes) };
  });
  const digestInput = rows
    .map((row) => `${row.path}\0${row.bytes}\0${row.sha256}\n`)
    .join("");
  return {
    files,
    summary: {
      file_count: rows.length,
      rust_source_files: rows.filter((row) => row.path.endsWith(".rs")).length,
      cargo_manifests: rows.filter((row) => row.path.endsWith("Cargo.toml")).length,
      bytes: rows.reduce((total, row) => total + row.bytes, 0),
      tree_sha256: sha256(digestInput),
    },
  };
}

const inventory = productionInventory();
const sourceCache = new Map(
  inventory.files.map((file) => [file, fileText(file)]),
);

function source(relative) {
  const cached = sourceCache.get(relative);
  if (cached !== undefined) return cached;
  const text = fileText(relative);
  sourceCache.set(relative, text);
  return text;
}

function anchor(id, relative, needle) {
  const text = source(relative);
  const offset = text.indexOf(needle);
  requireCondition(offset >= 0, `missing anchor ${id} in ${relative}`);
  requireCondition(
    text.indexOf(needle, offset + 1) < 0,
    `anchor ${id} is not unique in ${relative}`,
  );
  const before = text.slice(0, offset);
  const line = before.split("\n").length;
  const lastBreak = before.lastIndexOf("\n");
  const column = offset - lastBreak;
  return {
    id,
    path: relative,
    line,
    column,
    text: needle,
    text_sha256: sha256(needle),
    file_sha256: sha256(text),
  };
}

const anchorSpecs = [
  [
    "workspace-members",
    "Cargo.toml",
    '    "crates/program",\n    "crates/emitter",\n    "crates/compiler",',
  ],
  [
    "emitter-manifest-owner",
    "crates/emitter/Cargo.toml",
    '[package.metadata.tsc-rs]\nrole = "emitter"',
  ],
  [
    "emitter-artifact-protocol",
    "crates/emitter/src/artifact.rs",
    "pub struct EmitArtifact {\n    path: PathBuf,\n    callback_text: Box<str>,\n    write_byte_order_mark: bool,\n    kind: EmitArtifactKind,\n    source_files: Option<Box<[PathBuf]>>,\n    metadata: Option<EmitWriteMetadata>,\n}",
  ],
  [
    "emitter-output-plan-shape",
    "crates/emitter/src/plan.rs",
    "pub struct EmitOutputPaths {\n    javascript: Option<PathBuf>,\n    javascript_map: Option<PathBuf>,\n    declaration: Option<PathBuf>,\n    declaration_map: Option<PathBuf>,\n    build_info: Option<PathBuf>,\n}",
  ],
  [
    "emitter-output-sink-protocol",
    "crates/emitter/src/sink.rs",
    "pub trait OutputSink {\n    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError>;\n}",
  ],
  [
    "emitter-memory-sink",
    "crates/emitter/src/sink.rs",
    "pub struct MemoryOutputSink {\n    writes: Vec<EmitArtifact>,\n}",
  ],
  [
    "emitter-outcome-protocol",
    "crates/emitter/src/outcome.rs",
    "pub struct EmitOutcome {\n    diagnostics: DiagnosticList,\n    emit_skipped: bool,\n    emitted_files: Option<Box<[PathBuf]>>,\n    source_maps: Option<Box<[SourceMapObservation]>>,\n}",
  ],
  [
    "emitter-transform-arena",
    "crates/emitter/src/factory.rs",
    "pub struct TransformArena {",
  ],
  [
    "emitter-node-factory",
    "crates/emitter/src/factory.rs",
    "pub struct NodeFactory<'arena> {",
  ],
  [
    "emitter-transform-flags",
    "crates/emitter/src/transform.rs",
    "pub struct TransformFlags(i32);",
  ],
  [
    "emitter-emit-metadata",
    "crates/emitter/src/metadata.rs",
    "pub struct EmitMetadata {",
  ],
  [
    "emitter-transformation-context",
    "crates/emitter/src/transform.rs",
    "pub struct TransformationContext {",
  ],
  [
    "emitter-transform-nodes",
    "crates/emitter/src/transform.rs",
    "pub fn transform_nodes<'transformers>(\n    arena: TransformArena,",
  ],
  [
    "emitter-text-writer",
    "crates/emitter/src/writer.rs",
    "pub struct TextWriter {",
  ],
  [
    "emitter-printer-factory",
    "crates/emitter/src/printer.rs",
    "pub const fn create_printer(options: PrinterOptions) -> Printer {",
  ],
  [
    "emitter-position-domains",
    "crates/emitter/src/position.rs",
    "pub enum SourceRange {",
  ],
  [
    "emitter-source-map-recorder",
    "crates/emitter/src/printer.rs",
    "pub trait SourceMapRecorder {",
  ],
  [
    "emitter-resolver-protocol",
    "crates/emitter/src/resolver.rs",
    "pub trait EmitResolver {",
  ],
  [
    "emitter-script-transformer-selection",
    "crates/emitter/src/builtins.rs",
    "pub fn get_script_transformers<'resolver>(",
  ],
  [
    "emitter-transform-typescript",
    "crates/emitter/src/builtins.rs",
    "pub fn transform_type_script<'resolver>(",
  ],
  [
    "emitter-transform-class-fields",
    "crates/emitter/src/builtins.rs",
    "pub fn transform_class_fields(options: &CompilerOptions) -> Box<dyn Transformer> {",
  ],
  [
    "emitter-transform-ecmascript-module",
    "crates/emitter/src/builtins.rs",
    "pub fn transform_ecmascript_module(options: &CompilerOptions) -> Box<dyn Transformer> {",
  ],
  [
    "emitter-changed-import-printer",
    "crates/emitter/src/printer.rs",
    "NodeData::ImportDeclaration(data) => {",
  ],
  [
    "checker-scoped-emit-session",
    "crates/checker/src/emit.rs",
    "pub struct CheckerSession<'program> {",
  ],
  [
    "checker-emit-resolver-adapter",
    "crates/checker/src/emit.rs",
    "impl EmitResolver for CheckerSession<'_> {",
  ],
  [
    "checker-value-alias-producer",
    "crates/checker/src/modules.rs",
    "pub(crate) fn emit_is_value_alias_declaration(",
  ],
  [
    "checker-referenced-alias-producer",
    "crates/checker/src/modules.rs",
    "pub(crate) fn emit_is_referenced_alias_declaration(",
  ],
  [
    "active-transform-oracle",
    "ratchets/h1-active-transform.v1.json",
    '"phase": "H1.3-active-transform-resolver"',
  ],
  [
    "prepared-program-emitting-builder",
    "crates/program/src/prepared.rs",
    "pub fn emitting_builder(\n        path_context: PathContext,\n        compiler_options: CompilerOptions,\n    ) -> PreparedProgramBuilder {",
  ],
  [
    "program-session-owner",
    "crates/compiler/src/lib.rs",
    "pub struct ProgramSession {\n    prepared: PreparedProgram,\n}",
  ],
  [
    "program-session-run",
    "crates/compiler/src/lib.rs",
    "pub fn run(self) -> Result<NoEmitOutcome, DriverError> {\n        self.require_mode(PreparedProgramMode::NoEmit)?;\n        let mut no_emit_canary = no_emit_canary::NoEmitCanary::new();\n        self.run_with_no_emit_canary(false, &mut no_emit_canary)\n    }",
  ],
  [
    "program-session-emit",
    "crates/compiler/src/lib.rs",
    "pub fn emit(self, sink: &mut dyn OutputSink) -> Result<EmitOutcome, DriverError> {",
  ],
  [
    "program-session-emit-fail-before-sink",
    "crates/compiler/src/lib.rs",
    "Err(DriverError::Emit(EmitFailure::StageUnavailable(\n            EmitStage::TransformAndPrint,\n        )))",
  ],
  [
    "h1-no-emit-canary",
    "crates/compiler/src/no_emit_canary.rs",
    "pub(crate) struct NoEmitCanary;",
  ],
  [
    "checker-session-local-state",
    "crates/checker/src/lib.rs",
    "let mut state = state::CheckerState::from_snapshot(&snapshot, options);",
  ],
  [
    "checker-state-collapse",
    "crates/checker/src/lib.rs",
    "partial_checks = state.partial_check_records.clone();\n        authoritative_failure = state.take_authoritative_module_failure();",
  ],
  [
    "display-clone-not-emitter",
    "crates/checker/src/display_clone.rs",
    "This is deliberately not an emitter. It prints the synthesized face of",
  ],
  [
    "display-clone-exclusions",
    "crates/checker/src/display_clone.rs",
    "It does not copy\n//! source text, comments, source maps, substitutions, or declaration emit\n//! state",
  ],
  [
    "prepared-program-current-shape",
    "crates/program/src/prepared.rs",
    "pub struct PreparedProgram {\n    mode: PreparedProgramMode,\n    path_context: PathContext,\n    compiler_options: CompilerOptions,\n    program_options: ProgramOptions,",
  ],
  [
    "prepared-source-emit-facts",
    "crates/program/src/prepared.rs",
    "may_be_emitted: bool,\n    implied_node_format: Option<ResolutionMode>,\n    implied_node_format_for_emit: Option<ResolutionMode>,",
  ],
  [
    "checker-authoritative-emit-facts",
    "crates/checker/src/lib.rs",
    "pub may_be_emitted: bool,\n    /// Raw `SourceFile.impliedNodeFormat`",
  ],
  [
    "compiler-options-current-shape",
    "crates/types/src/options.rs",
    "pub struct CompilerOptions {",
  ],
  [
    "compiler-options-bootstrap-scalars",
    "crates/types/src/options.rs",
    "pub target: Option<i32>,\n    /// tsc ModuleKind value; None when the option is absent.",
  ],
  [
    "compiler-options-class-fields",
    "crates/types/src/options.rs",
    "pub use_define_for_class_fields: Option<bool>,",
  ],
  [
    "compiler-options-no-emit",
    "crates/types/src/options.rs",
    "pub no_emit: Option<bool>,",
  ],
  [
    "compiler-options-new-line",
    "crates/types/src/options.rs",
    "pub new_line: Option<i32>,",
  ],
  [
    "compiler-options-remove-comments",
    "crates/types/src/options.rs",
    "pub remove_comments: Option<bool>,",
  ],
  [
    "compiler-options-no-implicit-use-strict",
    "crates/types/src/options.rs",
    "pub no_implicit_use_strict: Option<bool>,",
  ],
  [
    "compiler-options-no-emit-helpers",
    "crates/types/src/options.rs",
    "pub no_emit_helpers: Option<bool>,",
  ],
  [
    "binder-source-aggregate-emit-flags",
    "crates/binder/src/containers.rs",
    "flags = NodeFlags::from_bits(flags.bits() | self.emit_flags);",
  ],
  [
    "loader-no-emit-gate",
    "crates/program/src/loader.rs",
    "if compiler_options.no_emit != Some(true) {\n        return Err(reject_input(\n            \"compilerOptions.noEmit must be explicitly true\",\n        ));\n    }",
  ],
  [
    "config-no-emit-gate",
    "crates/program/src/config.rs",
    "A config without an explicit\n/// `noEmit: true` is rejected before `load_program`",
  ],
  [
    "cli-explicit-root-no-emit-gate",
    "crates/compiler/src/cli.rs",
    "explicit source files require --noEmit; H0 never invokes an emitter",
  ],
  [
    "cli-false-no-emit-gate",
    "crates/compiler/src/cli.rs",
    '"--noEmit=false" => {\n                return Err(CliError::Usage(\n                    "--noEmit=false is outside the mandatory no-emit driver"',
  ],
  [
    "config-transient-output-discovery",
    "crates/program/src/config.rs",
    "out_dir: Option<String>,\n    declaration_dir: Option<String>,",
  ],
  [
    "program-snapshot-owner",
    "crates/checker/src/program.rs",
    "pub struct ProgramSnapshot {",
  ],
  [
    "captured-block-scope-bindings-elided",
    "crates/checker/src/expr.rs",
    "links.capturedBlockScopeBindings pushIfUnique",
  ],
  [
    "computed-property-loop-capture-elided",
    "crates/checker/src/literals.rs",
    "class-expression-property loop-capture side-effect",
  ],
  [
    "private-scope-loop-capture-elided",
    "crates/checker/src/functions.rs",
    "The class-expression-in-iteration arm sets emit-only flags",
  ],
  [
    "constructor-capture-this-elided",
    "crates/checker/src/functions.rs",
    "captureLexicalThis is emit-only (no-op)",
  ],
  [
    "async-linked-references-elided",
    "crates/checker/src/functions.rs",
    "`markLinkedReferences` is emit-only.",
  ],
  [
    "decorator-linked-references-elided",
    "crates/checker/src/calls.rs",
    "markLinkedReferences is declaration-emit bookkeeping",
  ],
  [
    "import-equals-linked-references-elided",
    "crates/checker/src/modules.rs",
    "markLinkedReferences' declaration-emit\n    /// traversal remains outside this checker slice",
  ],
  [
    "export-linked-aliases-elided",
    "crates/checker/src/modules.rs",
    "collectLinkedAliases remains declaration-emit bookkeeping",
  ],
  [
    "property-access-linked-references-elided",
    "crates/checker/src/access.rs",
    "markLinkedReferences' non-alias bookkeeping",
  ],
  [
    "inaccessible-this-elided",
    "crates/checker/src/check.rs",
    "inaccessible-this tracking is declaration-emit band",
  ],
  [
    "binding-object-rest-helper-elided",
    "crates/checker/src/statements.rs",
    "Object-rest emit-helper probe elided",
  ],
  [
    "binding-array-helper-elided",
    "crates/checker/src/statements.rs",
    "downlevelIteration emit-helper probe is elided",
  ],
  [
    "for-await-helper-elided",
    "crates/checker/src/statements.rs",
    "ForAwaitOf emit-helper probe — elided",
  ],
  [
    "for-of-helper-elided",
    "crates/checker/src/statements.rs",
    "downlevelIteration ForOf emit-helper probe — elided",
  ],
  [
    "signature-helper-probes-elided",
    "crates/checker/src/functions.rs",
    "Emit-helper probes are importHelpers-gated (no-op)",
  ],
  [
    "yield-helper-probes-elided",
    "crates/checker/src/functions.rs",
    "Grammar closure runs eager (the 5.4 addLazyDiagnostic\n    /// decision), as does the noImplicitAny 7057 closure. Emit-helper\n    /// probes are importHelpers-gated (no-op)",
  ],
  [
    "class-extends-helper-elided",
    "crates/checker/src/class.rs",
    "Elision: the ES5 Extends emit-helper probe",
  ],
  [
    "private-field-in-helper-elided",
    "crates/checker/src/operators.rs",
    "ClassPrivateFieldIn emit-helper rows are",
  ],
  [
    "object-rest-helper-elided",
    "crates/checker/src/operators.rs",
    "ObjectSpreadRest emit-helper row is importHelpers-gated",
  ],
  [
    "destructuring-helper-elided",
    "crates/checker/src/operators.rs",
    "DestructuringAssignment emit-helper row is",
  ],
  [
    "private-field-set-helper-elided",
    "crates/checker/src/operators.rs",
    "ClassPrivateFieldSet emit-helper row is importHelpers-gated",
  ],
  [
    "object-assign-helper-inactive",
    "crates/checker/src/literals.rs",
    "languageVersion ObjectAssign emit-helper gate is dead at",
  ],
  [
    "template-helper-inactive",
    "crates/checker/src/calls.rs",
    "MakeTemplateObject emit-helper check is dead at ES2025",
  ],
  [
    "export-assignment-linked-aliases-elided",
    "crates/checker/src/modules.rs",
    "erasableSyntaxOnly, collectLinkedAliases (emit), and the JSDoc",
  ],
  [
    "standard-class-fields-profile-check",
    "crates/checker/src/resolve.rs",
    "useDefineForClassFields is true, so getEmitStandardClassFields reduces\n    /// exactly to target >= ES2022",
  ],
  [
    "external-helper-producer-present",
    "crates/checker/src/modules.rs",
    "pub(crate) fn check_external_emit_helpers(",
  ],
  [
    "jsx-alias-producer-present",
    "crates/checker/src/jsx.rs",
    "fn mark_jsx_alias_referenced(&mut self, node: NodeId) -> CheckResult<()> {",
  ],
];

const anchors = anchorSpecs.map((spec) => anchor(...spec));
const anchorById = new Map(anchors.map((entry) => [entry.id, entry]));
requireCondition(anchorById.size === anchors.length, "anchor identifiers are not unique");

function scopedFiles(scope) {
  switch (scope) {
    case "production-rust":
      return inventory.files.filter((file) => file.endsWith(".rs"));
    case "cargo-manifests":
      return inventory.files.filter((file) => file.endsWith("Cargo.toml"));
    case "compiler-production":
      return inventory.files.filter(
        (file) => file.startsWith("crates/compiler/src/") && file.endsWith(".rs"),
      );
    case "checker-production":
      return inventory.files.filter(
        (file) => file.startsWith("crates/checker/src/") && file.endsWith(".rs"),
      );
    case "emitter-production":
      return inventory.files.filter(
        (file) => file.startsWith("crates/emitter/src/") && file.endsWith(".rs"),
      );
    default:
      throw new Error(`unknown omission search scope ${scope}`);
  }
}

function regexMatches(scope, expression) {
  const matches = [];
  for (const relative of scopedFiles(scope)) {
    const text = source(relative);
    const regex = new RegExp(expression, "gmu");
    for (const match of text.matchAll(regex)) {
      const before = text.slice(0, match.index);
      matches.push({
        path: relative,
        line: before.split("\n").length,
        text: match[0].trim(),
      });
    }
  }
  return matches;
}

const absenceSpecs = [
  {
    id: "emitter-host-protocol",
    kind: "regex-zero",
    scope: "emitter-production",
    expression:
      "^[ \\t]*(?:pub(?:\\([^)]*\\))?[ \\t]+)?trait[ \\t]+EmitHost\\b",
  },
  {
    id: "filesystem-output-sink",
    kind: "regex-zero",
    scope: "emitter-production",
    expression:
      "^[ \\t]*(?:pub(?:\\([^)]*\\))?[ \\t]+)?struct[ \\t]+FsOutputSink\\b",
  },
  {
    id: "active-output-functions",
    kind: "regex-zero",
    scope: "production-rust",
    expression:
      "^[ \\t]*(?:pub(?:\\([^)]*\\))?[ \\t]+)?fn[ \\t]+(?:get_source_files_to_emit|get_output_paths_for|for_each_emitted_file|emit_files)[ \\t]*\\(",
  },
  {
    id: "linked-reference-producers",
    kind: "regex-zero",
    scope: "checker-production",
    expression:
      "^[ \\t]*(?:pub(?:\\([^)]*\\))?[ \\t]+)?fn[ \\t]+(?:mark_linked_references|collect_linked_aliases)[ \\t]*\\(",
  },
];

const absenceProofs = absenceSpecs.map((spec) => {
  if (spec.kind === "path-absent") {
    const exists = fs.existsSync(path.join(WORKSPACE, spec.path));
    requireCondition(!exists, `expected omission path now exists: ${spec.path}`);
    return { ...spec, exists };
  }
  const matches = regexMatches(spec.scope, spec.expression);
  requireCondition(
    matches.length === 0,
    `${spec.id} omission closed or collided: ${JSON.stringify(matches)}`,
  );
  return { ...spec, match_count: matches.length, matches };
});
const absenceById = new Map(absenceProofs.map((proof) => [proof.id, proof]));

function evidence(anchorIds = [], absenceIds = []) {
  for (const id of anchorIds) requireCondition(anchorById.has(id), `unknown anchor ${id}`);
  for (const id of absenceIds) requireCondition(absenceById.has(id), `unknown absence ${id}`);
  return { anchors: anchorIds, absence_proofs: absenceIds };
}

const boundaryOmissions = [
  {
    id: "workspace-emitter-owner",
    planned_phase: "H1.4",
    current:
      "The acyclic emitter workspace owner contains the H1.1 execution spine, H1.2 transform/printer foundation, and H1.3 active built-in transformer, resolver, and changed-node printer slice.",
    missing:
      "The read-only EmitHost projection and executable output planning.",
    evidence: evidence(
      [
        "workspace-members",
        "emitter-manifest-owner",
        "emitter-artifact-protocol",
        "emitter-output-plan-shape",
        "emitter-output-sink-protocol",
        "emitter-memory-sink",
        "emitter-outcome-protocol",
        "emitter-transform-arena",
        "emitter-node-factory",
        "emitter-transformation-context",
        "emitter-transform-nodes",
        "emitter-text-writer",
        "emitter-printer-factory",
        "emitter-resolver-protocol",
        "emitter-script-transformer-selection",
        "emitter-transform-typescript",
        "emitter-transform-class-fields",
        "emitter-transform-ecmascript-module",
        "emitter-changed-import-printer",
      ],
      ["emitter-host-protocol", "active-output-functions"],
    ),
  },
  {
    id: "output-plan-artifacts-sinks",
    planned_phase: "H1.4-H1.5",
    current:
      "H1.1 owns the complete dormant-axis output shape, exact callback artifact identity, typed sink feedback/I/O errors, independent outcome observations, and ordered MemoryOutputSink.",
    missing:
      "Executable source/path planning, collision preflight, callback error continuation, partial-failure behavior, and FsOutputSink.",
    evidence: evidence(
      [
        "prepared-program-current-shape",
        "emitter-artifact-protocol",
        "emitter-output-plan-shape",
        "emitter-output-sink-protocol",
        "emitter-memory-sink",
        "emitter-outcome-protocol",
      ],
      ["active-output-functions", "filesystem-output-sink"],
    ),
  },
  {
    id: "emitting-loader-and-cli-route",
    planned_phase: "H1.4-H1.5",
    current:
      "H0 loader/config/CLI gates require noEmit=true and reject the emitting route before source execution.",
    missing:
      "A separate emitting option projection, config/explicit-root loader, preflight, CLI dispatch, filesystem sink connection, and exact exit behavior.",
    evidence: evidence([
      "loader-no-emit-gate",
      "config-no-emit-gate",
      "cli-explicit-root-no-emit-gate",
      "cli-false-no-emit-gate",
    ]),
  },
];

const compilerOptionsSource = source("crates/types/src/options.rs");
const compilerOptionsStart = compilerOptionsSource.indexOf("pub struct CompilerOptions {");
const compilerOptionsEnd = compilerOptionsSource.indexOf("\n}\n\nimpl CompilerOptions", compilerOptionsStart);
requireCondition(
  compilerOptionsStart >= 0 && compilerOptionsEnd > compilerOptionsStart,
  "failed to isolate CompilerOptions",
);
const compilerOptionsBlock = compilerOptionsSource.slice(
  compilerOptionsStart,
  compilerOptionsEnd,
);

const optionSpecs = [
  ["listEmittedFiles", "list_emitted_files", "bootstrap", "H1.4"],
  ["emitBOM", "emit_bom", "bootstrap", "H1.4"],
  ["noEmitOnError", "no_emit_on_error", "bootstrap", "H1.4-H1.5"],
  ["outDir", "out_dir", "output-planning", "H1.4"],
  ["rootDir", "root_dir", "output-planning", "H1.4"],
  ["noCheck", "no_check", "diagnostic-gate-control", "H1.4"],
  ["erasableSyntaxOnly", "erasable_syntax_only", "profile-preflight-control", "H1.4"],
  ["importsNotUsedAsValues", "imports_not_used_as_values", "legacy-transform-control", "typed-deferred"],
  ["preserveValueImports", "preserve_value_imports", "legacy-transform-control", "typed-deferred"],
  ["emitDecoratorMetadata", "emit_decorator_metadata", "adjacent-decorator", "profile-control"],
  ["sourceMap", "source_map", "dormant-source-map", "typed-slot-only"],
  ["inlineSourceMap", "inline_source_map", "dormant-source-map", "typed-slot-only"],
  ["inlineSources", "inline_sources", "dormant-source-map", "typed-slot-only"],
  ["sourceRoot", "source_root", "dormant-source-map", "typed-slot-only"],
  ["mapRoot", "map_root", "dormant-source-map", "typed-slot-only"],
  ["declaration", "declaration", "dormant-declaration", "typed-slot-only"],
  ["declarationMap", "declaration_map", "dormant-declaration", "typed-slot-only"],
  ["emitDeclarationOnly", "emit_declaration_only", "dormant-declaration", "typed-slot-only"],
  ["isolatedDeclarations", "isolated_declarations", "dormant-declaration", "typed-slot-only"],
  ["stableTypeOrdering", "stable_type_ordering", "dormant-declaration", "typed-slot-only"],
  ["declarationDir", "declaration_dir", "dormant-declaration", "typed-slot-only"],
  ["stripInternal", "strip_internal", "dormant-declaration", "typed-slot-only"],
  ["outFile", "out_file", "dormant-bundle", "typed-slot-only"],
  ["out", "out", "dormant-bundle", "typed-slot-only"],
  ["incremental", "incremental", "dormant-build-info", "typed-slot-only"],
  ["composite", "composite", "dormant-build-info", "typed-slot-only"],
  ["assumeChangesOnlyAffectDirectDependencies", "assume_changes_only_affect_direct_dependencies", "dormant-build-info", "typed-slot-only"],
  ["tsBuildInfoFile", "ts_build_info_file", "dormant-build-info", "typed-slot-only"],
];

for (const [option, rustField] of [
  ["newLine", "new_line"],
  ["removeComments", "remove_comments"],
  ["noImplicitUseStrict", "no_implicit_use_strict"],
  ["noEmitHelpers", "no_emit_helpers"],
]) {
  requireCondition(
    new RegExp(`\\bpub[ \\t]+${rustField}[ \\t]*:`).test(compilerOptionsBlock),
    `H1.2 CompilerOptions projection lost ${option}`,
  );
}

const optionProjectionOmissions = optionSpecs.map(
  ([option, rustField, disposition, plannedPhase]) => {
    const fieldExpression = new RegExp(`\\bpub[ \\t]+${rustField}[ \\t]*:`);
    requireCondition(
      !fieldExpression.test(compilerOptionsBlock),
      `CompilerOptions now retains ${option}; update its omission disposition`,
    );
    const catalog = source("crates/program/src/config_options.rs");
    const optionNeedle = `\"${option}\"`;
    requireCondition(catalog.includes(optionNeedle), `option catalog no longer contains ${option}`);
    return {
      option,
      rust_field: rustField,
      current_retention:
        option === "outDir" || option === "declarationDir"
          ? "config-root-discovery-only; absent from CompilerOptions and PreparedProgram emit projection"
          : "recognized by the config option catalog; absent from CompilerOptions",
      disposition,
      planned_phase: plannedPhase,
    };
  },
);

const checkerElisionSpecs = [
  ["captured-block-scope-bindings", "captured-block-scope-bindings-elided", "inactive-at-ESNext", "profile-control"],
  ["computed-property-loop-capture", "computed-property-loop-capture-elided", "inactive-at-ESNext", "profile-control"],
  ["private-scope-loop-capture", "private-scope-loop-capture-elided", "adjacent-class-field", "profile-control"],
  ["constructor-capture-lexical-this", "constructor-capture-this-elided", "adjacent-class-field", "profile-control"],
  ["async-mark-linked-references", "async-linked-references-elided", "dormant-declaration", "typed-deferred"],
  ["decorator-mark-linked-references", "decorator-linked-references-elided", "dormant-declaration/decorator", "typed-deferred"],
  ["import-equals-mark-linked-references", "import-equals-linked-references-elided", "adjacent-import-equals", "profile-control"],
  ["export-collect-linked-aliases", "export-linked-aliases-elided", "dormant-declaration", "typed-deferred"],
  ["property-access-mark-linked-references", "property-access-linked-references-elided", "dormant-declaration-non-alias-bookkeeping", "typed-deferred"],
  ["inaccessible-this-tracking", "inaccessible-this-elided", "dormant-declaration", "typed-deferred"],
  ["binding-object-rest-helper", "binding-object-rest-helper-elided", "adjacent-helper", "profile-control"],
  ["binding-array-downlevel-helper", "binding-array-helper-elided", "adjacent-helper", "profile-control"],
  ["for-await-helper", "for-await-helper-elided", "adjacent-helper", "profile-control"],
  ["for-of-downlevel-helper", "for-of-helper-elided", "adjacent-helper", "profile-control"],
  ["signature-helper-probes", "signature-helper-probes-elided", "adjacent-helper", "profile-control"],
  ["yield-helper-probes", "yield-helper-probes-elided", "adjacent-helper", "profile-control"],
  ["class-extends-helper", "class-extends-helper-elided", "adjacent-helper", "profile-control"],
  ["class-private-field-in-helper", "private-field-in-helper-elided", "adjacent-helper", "profile-control"],
  ["object-rest-helper", "object-rest-helper-elided", "adjacent-helper", "profile-control"],
  ["destructuring-helper", "destructuring-helper-elided", "adjacent-helper", "profile-control"],
  ["class-private-field-set-helper", "private-field-set-helper-elided", "adjacent-helper", "profile-control"],
  ["object-assign-helper", "object-assign-helper-inactive", "inactive-at-ESNext", "generated-canary"],
  ["template-object-helper", "template-helper-inactive", "inactive-at-ESNext", "generated-canary"],
  ["export-assignment-collect-linked-aliases", "export-assignment-linked-aliases-elided", "dormant-declaration", "typed-deferred"],
];

const checkerEmitElisions = checkerElisionSpecs.map(
  ([id, anchorId, disposition, closure]) => ({
    id,
    anchor: anchorId,
    disposition,
    closure,
  }),
);

const existingPrerequisites = [
  {
    id: "h1-factory-transform-printer-foundation",
    treatment:
      "reuse-and-extend-with-output-planning-without-mutating-published-syntax",
    evidence: [
      "emitter-transform-arena",
      "emitter-node-factory",
      "emitter-transform-flags",
      "emitter-emit-metadata",
      "emitter-transformation-context",
      "emitter-transform-nodes",
      "emitter-position-domains",
      "emitter-text-writer",
      "emitter-source-map-recorder",
      "emitter-printer-factory",
      "compiler-options-new-line",
      "compiler-options-remove-comments",
      "compiler-options-no-implicit-use-strict",
      "compiler-options-no-emit-helpers",
    ],
  },
  {
    id: "h1-active-transformer-runtime",
    treatment:
      "reuse-exact-frozen-order-and-extend-only-with-oracle-backed-syntax-owners",
    evidence: [
      "emitter-script-transformer-selection",
      "emitter-transform-typescript",
      "emitter-transform-class-fields",
      "emitter-transform-ecmascript-module",
      "emitter-changed-import-printer",
      "standard-class-fields-profile-check",
      "active-transform-oracle",
    ],
  },
  {
    id: "h1-scoped-checker-resolver",
    treatment:
      "reuse-live-checker-alias-producers-through-the-consumer-owned-fail-closed-protocol",
    evidence: [
      "program-snapshot-owner",
      "emitter-resolver-protocol",
      "checker-scoped-emit-session",
      "checker-emit-resolver-adapter",
      "checker-value-alias-producer",
      "checker-referenced-alias-producer",
      "active-transform-oracle",
      "checker-session-local-state",
      "checker-state-collapse",
    ],
  },
  {
    id: "h1-typed-execution-spine",
    treatment: "reuse-and-extend-without-routing-no-emit-through-emitter",
    evidence: [
      "prepared-program-emitting-builder",
      "program-session-run",
      "program-session-emit",
      "program-session-emit-fail-before-sink",
      "emitter-artifact-protocol",
      "emitter-output-plan-shape",
      "emitter-output-sink-protocol",
      "emitter-memory-sink",
      "emitter-outcome-protocol",
    ],
  },
  {
    id: "persistent-program-snapshot",
    treatment: "reuse",
    evidence: ["program-snapshot-owner"],
  },
  {
    id: "source-emit-eligibility",
    treatment: "verify-and-project",
    evidence: ["prepared-source-emit-facts", "checker-authoritative-emit-facts"],
  },
  {
    id: "implied-node-format-for-emit",
    treatment: "verify-and-project",
    evidence: ["prepared-source-emit-facts", "checker-authoritative-emit-facts"],
  },
  {
    id: "bootstrap-target-module-class-field-options",
    treatment: "reuse-scalars-through-separate-emit-projection",
    evidence: ["compiler-options-bootstrap-scalars", "compiler-options-class-fields"],
  },
  {
    id: "h0-no-emit-option",
    treatment: "preserve-separate-route",
    evidence: ["compiler-options-no-emit", "loader-no-emit-gate"],
  },
  {
    id: "h1-no-emit-constructor-write-canary",
    treatment: "preserve-and-thread-through-no-emit-route",
    evidence: ["program-session-run", "h1-no-emit-canary"],
  },
  {
    id: "sourcefile-aggregate-binder-emit-flags",
    treatment: "do-not-confuse-with-transform-flags",
    evidence: ["binder-source-aggregate-emit-flags"],
  },
  {
    id: "external-helper-semantic-producer",
    treatment: "reuse-only-when-profile-reachability-opens",
    evidence: ["external-helper-producer-present"],
  },
  {
    id: "jsx-alias-semantic-producer",
    treatment: "existing-but-JSX-remains-out-of-profile",
    evidence: ["jsx-alias-producer-present"],
  },
];

for (const prerequisite of existingPrerequisites) {
  for (const id of prerequisite.evidence) {
    requireCondition(anchorById.has(id), `unknown prerequisite evidence ${id}`);
  }
}
for (const elision of checkerEmitElisions) {
  requireCondition(anchorById.has(elision.anchor), `unknown checker elision ${elision.anchor}`);
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

function exactKeys(value, required) {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    required.every((key) => Object.hasOwn(value, key)) &&
    Object.keys(value).every((key) => required.includes(key))
  );
}

const artifact = {
  schema: 1,
  status: "frozen-current-rust-baseline",
  phase: "H1.3-rust-omission-inventory",
  typescript: { version: TYPESCRIPT_VERSION, source_commit: SOURCE_COMMIT },
  generator: pathHash(GENERATOR_RELATIVE_PATH),
  contract: pathHash(SCHEMA_RELATIVE_PATH),
  authorities: {
    compatibility_residual: pathHash(DESIGN_RELATIVE_PATH),
    h1_execution: pathHash(H1_DESIGN_RELATIVE_PATH),
    emit_profile: pathHash(PROFILE_RELATIVE_PATH),
    owner_inventory: pathHash(OWNER_RELATIVE_PATH),
  },
  scope: {
    description:
      "workspace Cargo manifests plus every Rust file under each workspace crate src tree; tests/benches/examples are excluded unless embedded in src",
    inclusion: ["Cargo.toml", "crates/*/Cargo.toml", "crates/*/src/**/*.rs"],
    ...inventory.summary,
  },
  summary: {
    production_boundary_omissions: boundaryOmissions.length,
    absence_proofs: absenceProofs.length,
    option_projection_omissions: optionProjectionOmissions.length,
    explicit_checker_emit_elisions: checkerEmitElisions.length,
    existing_prerequisites: existingPrerequisites.length,
  },
  existing_prerequisites: existingPrerequisites,
  production_boundary_omissions: boundaryOmissions,
  option_projection_omissions: optionProjectionOmissions,
  checker_emit_elisions: checkerEmitElisions,
  evidence: { anchors, absence_proofs: absenceProofs },
};
artifact.inventory_fingerprint_sha256 = sha256(canonical(artifact));

function validateArtifact(value) {
  requireCondition(
    exactKeys(value, [
      "schema",
      "status",
      "phase",
      "typescript",
      "generator",
      "contract",
      "authorities",
      "scope",
      "summary",
      "existing_prerequisites",
      "production_boundary_omissions",
      "option_projection_omissions",
      "checker_emit_elisions",
      "evidence",
      "inventory_fingerprint_sha256",
    ]),
    "H1 Rust omission artifact has missing or unknown top-level fields",
  );
  requireCondition(
    value.schema === 1 &&
      value.status === "frozen-current-rust-baseline" &&
      value.phase === "H1.3-rust-omission-inventory",
    "invalid H1 Rust omission artifact header",
  );
  const semantic = { ...value };
  delete semantic.inventory_fingerprint_sha256;
  requireCondition(
    value.inventory_fingerprint_sha256 === sha256(canonical(semantic)),
    "H1 Rust omission artifact fingerprint mismatch",
  );
  requireCondition(
    value.summary.production_boundary_omissions ===
      value.production_boundary_omissions.length &&
      value.summary.absence_proofs === value.evidence.absence_proofs.length &&
      value.summary.option_projection_omissions ===
        value.option_projection_omissions.length &&
      value.summary.explicit_checker_emit_elisions ===
        value.checker_emit_elisions.length &&
      value.summary.existing_prerequisites === value.existing_prerequisites.length,
    "H1 Rust omission artifact summary mismatch",
  );
}

validateArtifact(artifact);

requireCondition(
  boundaryOmissions.length === 3,
  "the reviewed missing-production-boundary table must remain exhaustive",
);
requireCondition(
  new Set(boundaryOmissions.map((entry) => entry.id)).size === boundaryOmissions.length,
  "production boundary omission identifiers are not unique",
);
requireCondition(
  optionProjectionOmissions.length === 28 &&
    new Set(optionProjectionOmissions.map((entry) => entry.option)).size ===
      optionProjectionOmissions.length,
  "emit option projection omission set drifted or contains duplicates",
);
requireCondition(
  optionProjectionOmissions.filter((entry) => entry.disposition === "bootstrap").length === 3,
  "bootstrap emit option omission set drifted",
);
requireCondition(
  checkerEmitElisions.length === checkerElisionSpecs.length &&
    new Set(checkerEmitElisions.map((entry) => entry.id)).size === checkerEmitElisions.length,
  "checker emit elision set drifted or contains duplicates",
);

const rendered = `${JSON.stringify(artifact, null, 2)}\n`;
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(TARGET_PATH, rendered);
  process.stdout.write(`wrote ${TARGET_RELATIVE_PATH}\n`);
} else if (mode === "--check") {
  requireCondition(fs.existsSync(TARGET_PATH), `missing ${TARGET_RELATIVE_PATH}`);
  requireCondition(
    fs.readFileSync(TARGET_PATH, "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run with --write and review`,
  );
  process.stdout.write(
    `H1 Rust omission inventory is fresh: boundaries=${boundaryOmissions.length} options=${optionProjectionOmissions.length} checker_elisions=${checkerEmitElisions.length}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  throw new Error("usage: h1-rust-omission-inventory.mjs [--write|--check]");
}
