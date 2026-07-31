// m2-binder-steps.md stage 3.0: the oracle side of the symbol spot-audit.
//
// For a program.json, print a deterministic symbol summary: for each
// statement-level declaration name (top level + ONE nesting level),
// resolve via checker.getSymbolAtLocation and emit
//
//   pos \t end \t escapedName \t flags \t declarations.length \t
//   sorted(members keys) \t sorted(exports keys)
//
// where pos/end are the NAME node's [pos, end) in UTF-16 code units and
// member/export key lists are comma-joined. Unresolved names emit
// "pos \t end \t <no-symbol>".
//
// THE WALK CONTRACT (mirrored in crates/xtask/src/symbol_audit.rs —
// change BOTH sides together):
//  - top-level statements contribute their declaration names:
//    function/class/interface/type-alias/enum/module names, variable
//    statement binding names (recursing through binding patterns),
//    import-equals name, import clause default/namespace/named names,
//    export clause namespace/named names;
//  - one nesting level: class/interface member names and enum member
//    names (skipping computed names), and for modules the dotted name
//    chain plus the final ModuleBlock's statements (names only, no
//    deeper member walk);
//  - name nodes count only when their kind is Identifier, StringLiteral,
//    NumericLiteral, or PrivateIdentifier.

import fs from "node:fs";
import readline from "node:readline";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import {
  absoluteProgramFileName,
  createProgramFromJsonPath,
  normalizeFileName,
} from "./program-host.mjs";

function isAuditNameKind(node) {
  return (
    ts.isIdentifier(node) ||
    ts.isStringLiteral(node) ||
    ts.isNumericLiteral(node) ||
    ts.isPrivateIdentifier(node)
  );
}

function pushName(name, out) {
  if (name && isAuditNameKind(name)) out.push(name);
}

function pushBindingNames(name, out) {
  if (!name) return;
  if (ts.isIdentifier(name)) {
    out.push(name);
  } else if (ts.isObjectBindingPattern(name) || ts.isArrayBindingPattern(name)) {
    for (const element of name.elements) {
      if (ts.isBindingElement(element)) pushBindingNames(element.name, out);
    }
  }
}

function pushMemberNames(members, out) {
  for (const member of members ?? []) {
    pushName(member.name, out);
  }
}

function visitStatement(statement, depth, out) {
  switch (statement.kind) {
    case ts.SyntaxKind.FunctionDeclaration:
    case ts.SyntaxKind.TypeAliasDeclaration:
      pushName(statement.name, out);
      break;
    case ts.SyntaxKind.ClassDeclaration:
    case ts.SyntaxKind.InterfaceDeclaration:
      pushName(statement.name, out);
      if (depth === 0) pushMemberNames(statement.members, out);
      break;
    case ts.SyntaxKind.EnumDeclaration:
      pushName(statement.name, out);
      if (depth === 0) pushMemberNames(statement.members, out);
      break;
    case ts.SyntaxKind.ModuleDeclaration: {
      // Dotted names parse as nested ModuleDeclarations: emit every
      // segment, then walk the final ModuleBlock one level deep.
      let current = statement;
      let block = undefined;
      while (current) {
        pushName(current.name, out);
        if (current.body && ts.isModuleDeclaration(current.body)) {
          current = current.body;
        } else {
          if (current.body && ts.isModuleBlock(current.body)) block = current.body;
          current = undefined;
        }
      }
      if (depth === 0 && block) {
        for (const inner of block.statements) visitStatement(inner, 1, out);
      }
      break;
    }
    case ts.SyntaxKind.VariableStatement:
      for (const declaration of statement.declarationList.declarations) {
        pushBindingNames(declaration.name, out);
      }
      break;
    case ts.SyntaxKind.ImportEqualsDeclaration:
      pushName(statement.name, out);
      break;
    case ts.SyntaxKind.ImportDeclaration: {
      const clause = statement.importClause;
      if (!clause) break;
      pushName(clause.name, out);
      const bindings = clause.namedBindings;
      if (bindings && ts.isNamespaceImport(bindings)) {
        pushName(bindings.name, out);
      } else if (bindings && ts.isNamedImports(bindings)) {
        for (const element of bindings.elements) pushName(element.name, out);
      }
      break;
    }
    case ts.SyntaxKind.ExportDeclaration: {
      const clause = statement.exportClause;
      if (clause && ts.isNamespaceExport(clause)) {
        pushName(clause.name, out);
      } else if (clause && ts.isNamedExports(clause)) {
        for (const element of clause.elements) pushName(element.name, out);
      }
      break;
    }
    default:
      break;
  }
}

function tableKeys(table) {
  return table ? [...table.keys()].map(String).sort().join(",") : "";
}

function lineForName(checker, name) {
  const symbol = checker.getSymbolAtLocation(name);
  if (!symbol) return `${name.pos}\t${name.end}\t<no-symbol>`;
  return [
    name.pos,
    name.end,
    String(symbol.escapedName),
    symbol.flags,
    symbol.declarations ? symbol.declarations.length : 0,
    tableKeys(symbol.members),
    tableKeys(symbol.exports),
  ].join("\t");
}

function symbolDump(programJsonPath) {
  const { program, programJson, cwd } = createProgramFromJsonPath(programJsonPath);
  const checker = program.getTypeChecker();
  const files = [];

  for (const file of programJson.files ?? []) {
    const name = normalizeFileName(file.name);
    const sourceFile = program.getSourceFile(absoluteProgramFileName(file.name, cwd));
    if (!sourceFile) {
      // Roots with unsupported extensions never join the program.
      files.push({ name, inProgram: false, parseErrors: 0, lines: [] });
      continue;
    }
    const names = [];
    for (const statement of sourceFile.statements) visitStatement(statement, 0, names);
    files.push({
      name,
      inProgram: true,
      parseErrors: sourceFile.parseDiagnostics.length,
      lines: names.map((nameNode) => lineForName(checker, nameNode)),
    });
  }

  return { files };
}

function runServerJsonl() {
  const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
  rl.on("line", (line) => {
    if (!line.trim()) return;
    let id = null;
    try {
      const request = JSON.parse(line);
      id = request.id === undefined ? null : request.id;
      process.stdout.write(JSON.stringify({
        id,
        ok: true,
        result: symbolDump(request.programJsonPath),
      }) + "\n");
    } catch (error) {
      process.stdout.write(JSON.stringify({
        id,
        ok: false,
        error: error && error.stack ? String(error.stack) : String(error),
      }) + "\n");
    }
  });
}

if (process.argv[2] === "--server-jsonl") {
  runServerJsonl();
} else {
  const programJsonPath = process.argv[2];

  if (!programJsonPath || !fs.existsSync(programJsonPath)) {
    console.error("usage: node symbol-dump.mjs <program.json>");
    console.error("   or: node symbol-dump.mjs --server-jsonl");
    process.exit(2);
  }

  const { files } = symbolDump(programJsonPath);
  for (const file of files) {
    console.log(`== ${file.name} inProgram=${file.inProgram} parseErrors=${file.parseErrors}`);
    for (const line of file.lines) console.log(line);
  }
}
