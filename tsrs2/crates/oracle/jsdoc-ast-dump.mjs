// Deterministic TypeScript 6.0.3 JSDoc AST oracle.
//
// The ordinary TypeScript child walk intentionally does not visit `node.jsDoc`.
// This oracle records that ordinary AST first, then walks every attached JSDoc
// subtree explicitly. Parsed string/boolean/node/node-array fields are emitted
// in name order; NodeArray source boundaries and attachment ownership are kept.
//
// usage:
//   node crates/oracle/jsdoc-ast-dump.mjs <file.js|file.jsx>
//   node crates/oracle/jsdoc-ast-dump.mjs --server-jsonl
//   node crates/oracle/jsdoc-ast-dump.mjs --self-test

import assert from "node:assert/strict";
import fs from "node:fs";
import * as readline from "node:readline";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const BASE_NODE_FIELDS = new Set([
  "end",
  "flags",
  "jsDoc",
  "kind",
  "original",
  "parent",
  "pos",
]);

const kindNames = new Map();
for (const [name, value] of Object.entries(ts.SyntaxKind)) {
  if (typeof value !== "number") continue;
  const names = kindNames.get(value) ?? [];
  names.push(name);
  kindNames.set(value, names);
}

function syntaxKindName(kind) {
  const names = kindNames.get(kind) ?? [];
  const concrete = names.filter(
    (name) => !name.startsWith("First") && !name.startsWith("Last") && name !== "Count"
  );
  const candidates = concrete.length === 0 ? names : concrete;
  return candidates.length === 0 ? `SyntaxKind(${kind})` : candidates[candidates.length - 1];
}

function scriptKindForFileName(fileName) {
  const detected = ts.getScriptKindFromFileName(fileName);
  return detected === ts.ScriptKind.Unknown ? ts.ScriptKind.TS : detected;
}

function isNode(value) {
  return (
    value !== null &&
    typeof value === "object" &&
    Number.isInteger(value.kind) &&
    typeof value.pos === "number" &&
    typeof value.end === "number"
  );
}

function isNodeArray(value, allowEmpty = false) {
  if (!Array.isArray(value)) return false;
  if (typeof value.pos === "number" && typeof value.end === "number") {
    return value.every(isNode);
  }
  return (allowEmpty || value.length > 0) && value.every(isNode);
}

function diagnosticDump(diagnostic) {
  return {
    code: diagnostic.code,
    category: diagnostic.category,
    categoryName: ts.DiagnosticCategory[diagnostic.category],
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    message: ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
  };
}

function jsDocAstDump(fileName, text) {
  const scriptKind = scriptKindForFileName(fileName);
  const sourceFile = ts.createSourceFile(
    fileName,
    text,
    {
      languageVersion: ts.ScriptTarget.ESNext,
      jsDocParsingMode: ts.JSDocParsingMode.ParseAll,
    },
    /*setParentNodes*/ true,
    scriptKind
  );

  const astEntries = [];
  const jsDocEntries = [];
  const entriesByNode = new Map();

  function addNode(node, domain, depth) {
    const existing = entriesByNode.get(node);
    if (existing) return existing;

    const entries = domain === "ast" ? astEntries : jsDocEntries;
    const entry = {
      id: `${domain === "ast" ? "a" : "j"}${entries.length}`,
      domain,
      node,
      depth,
      children: [],
    };
    entries.push(entry);
    entriesByNode.set(node, entry);

    ts.forEachChild(node, (child) => {
      const childEntry = addNode(child, domain, depth + 1);
      entry.children.push(childEntry.id);
    });
    return entry;
  }

  addNode(sourceFile, "ast", 0);

  const attachments = [];
  const attachmentOwners = new Set();

  function collectAttachment(entry) {
    const documents = entry.node.jsDoc;
    if (!isNodeArray(documents) || attachmentOwners.has(entry.node)) return;
    attachmentOwners.add(entry.node);
    for (const document of documents) addNode(document, "jsDoc", 0);
    attachments.push({ owner: entry, documents });
  }

  for (const entry of astEntries) collectAttachment(entry);
  for (let index = 0; index < jsDocEntries.length; index += 1) {
    collectAttachment(jsDocEntries[index]);
  }

  function nodeRef(node) {
    const entry = entriesByNode.get(node);
    return {
      id: entry?.id ?? null,
      kind: node.kind,
      kindName: syntaxKindName(node.kind),
      pos: node.pos,
      end: node.end,
    };
  }

  function nodeArrayDump(array) {
    return {
      pos: typeof array.pos === "number" ? array.pos : null,
      end: typeof array.end === "number" ? array.end : null,
      hasTrailingComma:
        typeof array.hasTrailingComma === "boolean" ? array.hasTrailingComma : null,
      elements: array.map(nodeRef),
    };
  }

  function observableFields(node, allowEmptyNodeArrays) {
    const fields = [];
    for (const name of Object.keys(node).sort()) {
      if (BASE_NODE_FIELDS.has(name)) continue;
      const value = node[name];
      if (typeof value === "string") {
        fields.push({ name, type: "string", value });
      } else if (typeof value === "boolean") {
        fields.push({ name, type: "boolean", value });
      } else if (isNode(value)) {
        fields.push({ name, type: "node", value: nodeRef(value) });
      } else if (isNodeArray(value, allowEmptyNodeArrays)) {
        fields.push({ name, type: "nodeArray", value: nodeArrayDump(value) });
      }
    }
    return fields;
  }

  function nodeDump(entry) {
    const { node } = entry;
    return {
      id: entry.id,
      kind: node.kind,
      kindName: syntaxKindName(node.kind),
      pos: node.pos,
      end: node.end,
      flags: node.flags,
      parent: node.parent ? nodeRef(node.parent) : null,
      depth: entry.depth,
      children: entry.children.map((id) => ({ id })),
      fields: observableFields(node, entry.domain === "jsDoc"),
    };
  }

  return {
    schema: 1,
    fileName,
    scriptKind: {
      value: scriptKind,
      name: ts.ScriptKind[scriptKind],
    },
    sourceFile: nodeRef(sourceFile),
    ast: astEntries.map(nodeDump),
    jsDocAttachments: attachments.map(({ owner, documents }) => ({
      owner: nodeRef(owner.node),
      property: "jsDoc",
      value: nodeArrayDump(documents),
    })),
    jsDocNodes: jsDocEntries.map(nodeDump),
    parseDiagnostics: sourceFile.parseDiagnostics.map(diagnosticDump),
    jsDocDiagnostics: (sourceFile.jsDocDiagnostics ?? []).map(diagnosticDump),
  };
}

function field(node, name) {
  return node.fields.find((candidate) => candidate.name === name);
}

function selfTest() {
  const jsText = [
    "/**",
    " * Summary {@link Foo.bar label}",
    " * @template {object} T",
    " * @param {T} [value] Description {@linkcode Foo}",
    " * @returns {string}",
    " */",
    'function f(value) { return ""; }',
    "",
  ].join("\n");
  const first = jsDocAstDump("self-test.js", jsText);
  const second = jsDocAstDump("self-test.js", jsText);

  assert.deepEqual(first, second, "the same JS input must produce identical JSON");
  assert.equal(first.scriptKind.value, ts.ScriptKind.JS);
  assert.equal(first.jsDocAttachments.length, 1);
  assert.equal(first.parseDiagnostics.length, 0);
  assert.equal(first.jsDocDiagnostics.length, 0);

  const document = first.jsDocNodes.find(
    (node) => node.kind === ts.SyntaxKind.JSDocComment
  );
  assert.ok(document, "JSDocComment must be materialized");
  assert.equal(document.parent.id, first.jsDocAttachments[0].owner.id);
  assert.equal(first.jsDocAttachments[0].value.elements[0].id, document.id);

  const comment = field(document, "comment");
  const tags = field(document, "tags");
  assert.equal(comment?.type, "nodeArray");
  assert.equal(tags?.type, "nodeArray");
  assert.equal(typeof comment.value.pos, "number");
  assert.equal(typeof comment.value.end, "number");
  assert.equal(typeof tags.value.pos, "number");
  assert.equal(typeof tags.value.end, "number");

  const parameterTag = first.jsDocNodes.find(
    (node) => node.kind === ts.SyntaxKind.JSDocParameterTag
  );
  assert.ok(parameterTag, "JSDocParameterTag must be materialized");
  assert.deepEqual(field(parameterTag, "isBracketed"), {
    name: "isBracketed",
    type: "boolean",
    value: true,
  });
  assert.ok(
    first.jsDocNodes.some((node) => node.kind === ts.SyntaxKind.JSDocLinkCode),
    "structured comment links must be walked"
  );

  const jsx = jsDocAstDump(
    "self-test.jsx",
    "/** @type {number} */\nconst view = <div />;\n"
  );
  assert.equal(jsx.scriptKind.value, ts.ScriptKind.JSX);
  assert.ok(
    jsx.ast.some((node) => node.kind === ts.SyntaxKind.JsxSelfClosingElement),
    ".jsx input must be parsed with JSX grammar"
  );
  assert.equal(jsx.jsDocAttachments.length, 1);

  const broken = jsDocAstDump(
    "broken.js",
    "/** @type { */\nconst = ;\n"
  );
  assert.ok(broken.parseDiagnostics.length > 0, "parse diagnostics must be emitted");
  assert.ok(
    broken.jsDocDiagnostics.length > 0,
    "JSDoc parse diagnostics must be emitted separately"
  );

  process.stdout.write("jsdoc-ast-dump self-test: ok\n");
}

function runServerJsonl() {
  const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
  rl.on("line", (line) => {
    if (!line.trim()) return;
    let id = null;
    try {
      const request = JSON.parse(line);
      id = request.id === undefined ? null : request.id;
      const payload = request.payload ?? request;
      const text =
        payload.textBase64 === undefined
          ? (payload.text ?? "")
          : Buffer.from(payload.textBase64, "base64").toString("utf8");
      const fileName = payload.fileName ?? "a.js";
      process.stdout.write(
        JSON.stringify({
          id,
          ok: true,
          result: jsDocAstDump(fileName, text),
        }) + "\n"
      );
    } catch (error) {
      process.stdout.write(
        JSON.stringify({
          id,
          ok: false,
          error: error && error.stack ? String(error.stack) : String(error),
        }) + "\n"
      );
    }
  });
}

function printUsage() {
  console.error("usage: node jsdoc-ast-dump.mjs <file.js|file.jsx>");
  console.error("   or: node jsdoc-ast-dump.mjs --server-jsonl");
  console.error("   or: node jsdoc-ast-dump.mjs --self-test");
}

const argument = process.argv[2];
if (argument === "--server-jsonl") {
  runServerJsonl();
} else if (argument === "--self-test") {
  selfTest();
} else if (argument) {
  const text = fs.readFileSync(argument, "utf8");
  process.stdout.write(JSON.stringify(jsDocAstDump(argument, text), null, 2) + "\n");
} else {
  printUsage();
  process.exit(2);
}
