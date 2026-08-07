import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const tscPath = path.join(workspace, "vendor/typescript-6.0.3/lib/_tsc.js");
const typesPath = path.join(workspace, "crates/types/src/options.rs");

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function read(relativePath) {
  return fs.readFileSync(path.join(workspace, relativePath), "utf8");
}

function lineOf(text, needle) {
  const offset = text.indexOf(needle);
  if (offset < 0) throw new Error(`missing audited source fragment: ${needle}`);
  if (text.indexOf(needle, offset + 1) >= 0) {
    throw new Error(`audited source fragment is not unique: ${needle}`);
  }
  return text.slice(0, offset).split("\n").length;
}

function owner(relativePath, needle) {
  const text = read(relativePath);
  return {
    path: relativePath,
    line: lineOf(text, needle),
    source_sha256: sha256(text),
    evidence: needle,
  };
}

const tscText = fs.readFileSync(tscPath);
const expectedTscHash = "1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3";
if (sha256(tscText) !== expectedTscHash) {
  throw new Error("vendored _tsc.js differs from the reviewed TypeScript 6.0.3 pin");
}
if (ts.version !== "6.0.3") throw new Error(`unexpected TypeScript version ${ts.version}`);

const rustFieldByOption = {
  target: "target",
  module: "module",
  jsx: "jsx",
  importHelpers: "import_helpers",
  alwaysStrict: "always_strict",
  noFallthroughCasesInSwitch: "no_fallthrough_cases_in_switch",
  moduleResolution: "module_resolution",
  jsxImportSource: "jsx_import_source",
  allowUnusedLabels: "allow_unused_labels",
  allowUnreachableCode: "allow_unreachable_code",
  moduleDetection: "module_detection",
};

const optionsText = fs.readFileSync(typesPath, "utf8");
const structStart = optionsText.indexOf("pub struct CompilerOptions {");
const structEnd = optionsText.indexOf("\n}\n\nimpl CompilerOptions", structStart);
if (structStart < 0 || structEnd < 0) throw new Error("cannot locate Rust CompilerOptions");
const rustFields = [...optionsText.slice(structStart, structEnd).matchAll(/^\s*pub\s+([a-z0-9_]+):/gmu)]
  .map((match) => match[1])
  .sort();

const sourceFileOptions = ts.sourceFileAffectingCompilerOptions.map((option) => {
  const rustField = rustFieldByOption[option.name];
  if (!rustField || !rustFields.includes(rustField)) {
    throw new Error(`source/bind-affecting option ${option.name} has no Rust field mapping`);
  }
  return {
    name: option.name,
    rust_field: rustField,
    affects_source_file: option.affectsSourceFile === true,
    affects_bind_diagnostics: option.affectsBindDiagnostics === true,
    affects_semantic_diagnostics: option.affectsSemanticDiagnostics === true,
    affects_module_resolution: option.affectsModuleResolution === true,
    strict_flag: option.strictFlag === true,
    allow_js_flag: option.allowJsFlag === true,
  };
});

const expectedSourceFileOptions = [
  "target",
  "module",
  "jsx",
  "importHelpers",
  "alwaysStrict",
  "noFallthroughCasesInSwitch",
  "moduleResolution",
  "jsxImportSource",
  "allowUnusedLabels",
  "allowUnreachableCode",
  "moduleDetection",
];
if (JSON.stringify(sourceFileOptions.map((option) => option.name)) !== JSON.stringify(expectedSourceFileOptions)) {
  throw new Error("TypeScript sourceFileAffectingCompilerOptions changed from the reviewed 6.0.3 set");
}

function optionNames(options) {
  return options.map((option) => option.name);
}

const inventory = {
  schema: 3,
  status: "owned-bind-state-complete",
  typescript: {
    version: ts.version,
    source_commit: "050880ce59e30b356b686bd3144efe24f875ebc8",
    tsc_path: "vendor/typescript-6.0.3/lib/_tsc.js",
    tsc_sha256: expectedTscHash,
    option_declarations: ts.optionDeclarations.length,
  },
  document_address: {
    registry_namespace: [
      "typescript_pin",
      "jsdoc_parsing_mode",
      "canonicalization_profile",
      "rust_parser_policy",
    ],
    bucket: ["source_file_affecting_options", "implied_node_format"],
    path: "canonical_path",
    variant: "script_kind",
    excluded_version_facts: ["document_version", "text_hash", "snapshot_lineage"],
  },
  option_sets: {
    source_file_affecting: sourceFileOptions,
    source_file_only: optionNames(ts.optionDeclarations.filter((option) => option.affectsSourceFile === true)),
    bind_diagnostics: optionNames(ts.optionDeclarations.filter((option) => option.affectsBindDiagnostics === true)),
    semantic_diagnostics: optionNames(ts.semanticDiagnosticsOptionDeclarations),
    module_resolution: optionNames(ts.moduleResolutionOptionDeclarations),
    program_structure: optionNames(ts.optionsAffectingProgramStructure),
  },
  rust_source_ownership: {
    representations: [
      {
        owner: "PreparedSourceFile.snapshot",
        storage: "Arc<TextSnapshot>",
        ...owner("crates/program/src/prepared.rs", "pub struct PreparedSourceFile {"),
      },
      {
        owner: "InputFile.snapshot",
        storage: "Arc<TextSnapshot>",
        ...owner("crates/checker/src/lib.rs", "pub struct InputFile {"),
      },
      {
        owner: "SourceFile.snapshot",
        storage: "Arc<TextSnapshot>",
        authority: "parsed-document-single-snapshot-authority",
        ...owner("crates/syntax/src/lib.rs", "pub struct SourceFile {"),
      },
      {
        owner: "ConfigSourceText.snapshot",
        storage: "Arc<TextSnapshot>",
        ...owner("crates/program/src/config.rs", "pub struct ConfigSourceText {"),
      },
      {
        owner: "ProgramConfigFile.snapshot",
        storage: "Arc<TextSnapshot>",
        ...owner("crates/program/src/prepared.rs", "pub struct ProgramConfigFile {"),
      },
      {
        owner: "PreparedAuxiliaryFile.snapshot",
        storage: "Arc<TextSnapshot>",
        ...owner("crates/program/src/prepared.rs", "pub struct PreparedAuxiliaryFile {"),
      },
      {
        owner: "PackageMetadata.snapshot",
        storage: "Arc<TextSnapshot>",
        ...owner("crates/program/src/prepared.rs", "pub struct PackageMetadata {"),
      },
    ],
    shared_snapshot_edges: [
      {
        from: "PreparedSourceFile.snapshot",
        to: "InputFile.snapshot",
        ...owner(
          "crates/compiler/src/lib.rs",
          "        InputFile::from_snapshot(name, Arc::clone(source.snapshot())),",
        ),
      },
      {
        from: "InputFile.snapshot",
        to: "SourceFile.snapshot",
        branch: "json",
        ...owner(
          "crates/checker/src/lib.rs",
          "            let source_file = tsc_syntax::parse_json_text_from_snapshot_in_identity_domain(",
        ),
      },
      {
        from: "InputFile.snapshot",
        to: "SourceFile.snapshot",
        branch: "ordinary",
        ...owner(
          "crates/checker/src/lib.rs",
          "        let source_file = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(",
        ),
      },
      {
        from: "PreparedSourceFile.snapshot",
        to: "module-request SourceFile.snapshot",
        ...owner(
          "crates/program/src/module_requests.rs",
          "    let parsed = parse_source_file_from_snapshot(",
        ),
      },
      {
        from: "ConfigSourceText.snapshot",
        to: "ProgramConfigFile.snapshot",
        ...owner(
          "crates/program/src/config.rs",
          "    let mut config_file = ProgramConfigFile::from_snapshot(path, Arc::clone(source.snapshot()));",
        ),
      },
      {
        from: "package-json ingress snapshot",
        to: "JSON SourceFile.snapshot and PackageMetadata.snapshot",
        ...owner("crates/program/src/json.rs", "    let source = parse_json_text_from_snapshot("),
      },
      {
        from: "prepared/config/auxiliary snapshots",
        to: "CLI diagnostic snapshot map",
        ...owner(
          "crates/compiler/src/cli.rs",
          "    let host = FormatDiagnosticsHost::from_snapshots(current_directory, source_texts);",
        ),
      },
    ],
    full_text_copy_edges: [],
    zero_copy_counter_contract: owner(
      "crates/compiler/tests/integration/program_session_contract.rs",
      "fn l0_work_counters_and_sorted_batch_syntax_diagnostics() {",
    ),
    position_index: {
      static_h0: {
        representation: "private dense byte/UTF-16 tables",
        ...owner("crates/diagnostics/src/text.rs", "struct DensePositionIndex {"),
      },
      edited: {
        representation: "persistent line tree with subtree byte/UTF-16/line counts",
        ...owner("crates/diagnostics/src/text.rs", "struct PersistentLineTree {"),
      },
      accessor_contract: owner(
        "crates/diagnostics/src/text.rs",
        "    pub fn byte_to_utf16(&self, position: u32) -> Option<u32> {",
      ),
      versioned_store: owner("crates/diagnostics/src/text.rs", "pub struct VersionedTextStore {"),
    },
    identity_ownership: {
      domain_allocator: {
        policies: ["ephemeral-bump", "reclaiming-interval"],
        spaces: ["node", "node-array", "persistent-symbol", "private-name-serial"],
        ...owner("crates/types/src/identity.rs", "pub struct IdentityDomain {"),
      },
      lease_lifetime: {
        storage: "Arc lease; last-owner Drop releases the exact interval",
        ...owner("crates/types/src/identity.rs", "pub struct IdentityLease {"),
      },
      syntax_owner: {
        storage: "NodeArena node/array leases",
        ...owner("crates/syntax/src/arena.rs", "    node_lease: Option<IdentityLease>,"),
      },
      generated_syntax_relocation: {
        source: "nodes.schema.json field model",
        ...owner("crates/xtask/src/main.rs", "fn render_relocate_rs(schemas: &[NodeSchema])"),
      },
      bind_owner: {
        storage: "BinderWorker publication moves symbol/private-name leases into owned BindData",
        ...owner("crates/binder/src/declare.rs", "    private_name_serial_lease: Option<IdentityLease>,"),
      },
      program_snapshot_owner: {
        storage: "Arc-owned ParsedDocument/BoundDocument handles ordered by ProgramSnapshot",
        ...owner("crates/checker/src/program.rs", "pub struct ProgramSnapshot {")
      },
      program_owner_intervals: {
        contract: "base-sorted non-contiguous and non-overlapping",
        ...owner("crates/checker/src/program.rs", "    symbol_owners: Vec<ArenaOwner>,"),
      },
      checker_transient_partition: {
        high_bit: 2147483648,
        ...owner("crates/types/src/identity.rs", "pub const TRANSIENT_SYMBOL_BIT: u32 = 1 << 31;"),
      },
    },
    compiler_options_owner: {
      path: "crates/types/src/options.rs",
      source_sha256: sha256(optionsText),
      fields: rustFields,
    },
  },
};

const rendered = `${JSON.stringify(inventory, null, 2)}\n`;
const target = path.join(workspace, "ratchets/l0-source-options.v1.json");
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(target, rendered);
  process.stdout.write(`wrote ${path.relative(workspace, target)}\n`);
} else if (mode === "--check") {
  if (!fs.existsSync(target) || fs.readFileSync(target, "utf8") !== rendered) {
    throw new Error("stale ratchets/l0-source-options.v1.json; run with --write and review");
  }
  process.stdout.write("L0 source/options inventory is fresh\n");
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  throw new Error("usage: l0-option-inventory.mjs [--write|--check]");
}
