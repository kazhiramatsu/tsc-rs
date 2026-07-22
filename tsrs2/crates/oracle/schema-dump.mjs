// Node half of the schema-audit gate (impl-nodes.md field contract):
// parse typescript.d.ts with the VENDORED TypeScript itself and dump,
// per SyntaxKind, the interface fields tsc declares. xtask compares
// this against crates/syntax/nodes.schema.json — the Rust generator's
// line-based d.ts extraction — so a generator parsing bug (the
// readonly_* field class) cannot survive unnoticed.
//
// usage:  node schema-dump.mjs <path-to-typescript.d.ts>
// stdout: {"schema":1,"kinds":{K:{"interface":N,"fields":[
//          {"name","type","optional"}]}},"conflicts":[...]}

import fs from "node:fs";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const dtsPath = process.argv[2];
if (!dtsPath) {
  console.error("usage: node schema-dump.mjs <typescript.d.ts>");
  process.exit(2);
}
const text = fs.readFileSync(dtsPath, "utf8");
const sourceFile = ts.createSourceFile(
  "typescript.d.ts",
  text,
  ts.ScriptTarget.ESNext,
  /*setParentNodes*/ false,
  ts.ScriptKind.TS
);

// name -> { bases: string[], members: Map(name -> {typeText, optional}) }
// (declaration merging: repeated interface names merge member-wise)
const interfaces = new Map();
// single-declaration type aliases, name -> right-hand-side text
const aliases = new Map();

function typeText(node) {
  return node ? text.slice(node.pos, node.end).trim() : "";
}

function collect(node) {
  if (ts.isInterfaceDeclaration(node)) {
    const name = node.name.text;
    let decl = interfaces.get(name);
    if (!decl) {
      decl = { bases: [], members: new Map(), ownKind: undefined };
      interfaces.set(name, decl);
    }
    for (const heritage of node.heritageClauses ?? []) {
      for (const type of heritage.types) {
        const base = typeText(type.expression).split("<")[0].trim();
        if (base && !decl.bases.includes(base)) decl.bases.push(base);
      }
    }
    for (const member of node.members) {
      if (!ts.isPropertySignature(member)) continue;
      if (!member.name || !ts.isIdentifier(member.name)) continue;
      const memberName = member.name.text;
      if (memberName.startsWith("_")) continue;
      const memberType = typeText(member.type);
      const optional = member.questionToken !== undefined || /\bundefined\b/.test(memberType);
      decl.members.set(memberName, { typeText: memberType, optional });
      if (memberName === "kind") decl.ownKind = memberType;
    }
  } else if (ts.isTypeAliasDeclaration(node) && ts.isIdentifier(node.name)) {
    if (!aliases.has(node.name.text)) {
      aliases.set(node.name.text, typeText(node.type));
    }
  }
  ts.forEachChild(node, collect);
}
collect(sourceFile);

function kindLiterals(kindTypeText) {
  const kinds = [];
  const re = /SyntaxKind\.([A-Za-z0-9_]+)/g;
  let match;
  while ((match = re.exec(kindTypeText)) !== null) kinds.push(match[1]);
  return kinds;
}

// Mirrors xtask rust_field_type / resolve_alias_type so categories
// compare 1:1; the independent part of this cross-check is the FIELD
// LIST extraction, done here by the real TypeScript parser.
function fieldCategory(rawTypeText) {
  let t = rawTypeText.trim();
  const bare = t
    .replace(/^undefined\s*\|\s*/, "")
    .replace(/\s*\|\s*undefined$/, "")
    .trim();
  if (aliases.has(bare)) t = aliases.get(bare);
  if (t.includes("NodeArray<")) return "NodeArray";
  // Token<SyntaxKind.X> (and PunctuationToken/KeywordToken/ModifierToken)
  // are token NODES — classify before the bare-SyntaxKind payload check.
  if (t.includes("Token<")) return "Node";
  if (t.includes("boolean")) return "Bool";
  if (t.includes("string") || t.includes("__String")) return "String";
  if (t.includes("number")) return "Number";
  if (t.includes("SyntaxKind")) return "SyntaxKind";
  for (const marker of [
    "Node", "Expression", "Declaration", "Identifier", "Token", "Type",
    "Statement", "Clause", "Element", "Literal", "Name",
  ]) {
    if (t.includes(marker)) return "Node";
  }
  // Interface-named types the marker list misses (Block, CaseBlock, …):
  // the real parse knows every interface, so use membership directly,
  // expanding one alias level for union members.
  const parts = [];
  for (const raw of t.split("|")) {
    const part = raw.trim().split("<")[0].trim();
    const rhs = aliases.get(part);
    if (rhs) {
      for (const expanded of rhs.split("|")) {
        parts.push(expanded.trim().split("<")[0].trim());
      }
    } else {
      parts.push(part);
    }
  }
  if (parts.some((part) => interfaces.has(part))) {
    return "Node";
  }
  return "Payload";
}

// kind -> [{interface, singleLiteral}]
const claimants = new Map();
for (const [name, decl] of interfaces) {
  if (!decl.ownKind) continue;
  const kinds = kindLiterals(decl.ownKind);
  for (const kind of kinds) {
    if (!claimants.has(kind)) claimants.set(kind, []);
    claimants.get(kind).push({ name, singleLiteral: kinds.length === 1 });
  }
}

function mergedFields(name, stack = []) {
  if (stack.includes(name)) return new Map();
  const decl = interfaces.get(name);
  if (!decl) return new Map();
  stack.push(name);
  const fields = new Map();
  for (const base of decl.bases) {
    for (const [fieldName, field] of mergedFields(base, stack)) {
      fields.set(fieldName, field);
    }
  }
  for (const [fieldName, field] of decl.members) {
    fields.set(fieldName, field);
  }
  stack.pop();
  return fields;
}

const kinds = {};
const conflicts = [];
for (const [kind, list] of claimants) {
  let chosen;
  if (list.length === 1) {
    chosen = list[0].name;
  } else {
    const exact = list.find((claim) => claim.name === kind);
    if (exact) {
      chosen = exact.name;
    } else {
      const singles = list.filter((claim) => claim.singleLiteral);
      if (singles.length === 1) {
        chosen = singles[0].name;
      } else {
        conflicts.push({ kind, interfaces: list.map((claim) => claim.name) });
        continue;
      }
    }
  }
  const fields = [];
  for (const [fieldName, field] of mergedFields(chosen)) {
    if (fieldName === "kind") continue;
    fields.push({
      name: fieldName,
      type: fieldCategory(field.typeText),
      optional: field.optional,
    });
  }
  kinds[kind] = { interface: chosen, fields };
}

process.stdout.write(JSON.stringify({ schema: 1, kinds, conflicts }));
