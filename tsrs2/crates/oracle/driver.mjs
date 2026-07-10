import readline from "node:readline";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import {
  absoluteProgramFileName,
  categoryNames,
  createProgramFromJsonPath,
  publicFileName,
} from "./program-host.mjs";

function collectDiagnostics(programJsonPath) {
  const { program, programJson, cwd, publicNames } = createProgramFromJsonPath(programJsonPath);
  const diagnostics = [];

  for (const file of programJson.files ?? []) {
    const sourceFile = program.getSourceFile(absoluteProgramFileName(file.name, cwd));
    if (!sourceFile) continue;
    for (const [pass, passDiagnostics] of [
      ["syntactic", program.getSyntacticDiagnostics(sourceFile)],
      ["semantic", program.getSemanticDiagnostics(sourceFile)],
      ["suggestion", program.getSuggestionDiagnostics(sourceFile)],
    ]) {
      for (const diagnostic of passDiagnostics) {
        diagnostic.tsrsPass ??= pass;
        diagnostics.push(diagnostic);
      }
    }
  }

  return ts.sortAndDeduplicateDiagnostics(diagnostics).map((diagnostic) =>
    serializeDiagnostic(diagnostic, publicNames)
  );
}

function serializeDiagnostic(diagnostic, publicNames) {
  return {
    file: publicFileName(diagnostic.file, publicNames),
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    code: diagnostic.code,
    pass: diagnostic.tsrsPass ?? null,
    category: categoryNames[diagnostic.category] ?? "message",
    chain: serializeMessageText(diagnostic.messageText, diagnostic.code, diagnostic.category),
    related: (diagnostic.relatedInformation ?? []).map((related) =>
      serializeRelated(related, publicNames)
    ),
    reportsUnnecessary: !!diagnostic.reportsUnnecessary,
    reportsDeprecated: !!diagnostic.reportsDeprecated,
    source: diagnostic.source ?? null,
  };
}

function serializeRelated(diagnostic, publicNames) {
  return {
    file: publicFileName(diagnostic.file, publicNames),
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    code: diagnostic.code,
    category: categoryNames[diagnostic.category] ?? "message",
    chain: serializeMessageText(diagnostic.messageText, diagnostic.code, diagnostic.category),
  };
}

function serializeMessageText(messageText, fallbackCode, fallbackCategory) {
  if (typeof messageText === "string") {
    return {
      text: messageText,
      code: fallbackCode,
      category: categoryNames[fallbackCategory] ?? "message",
      next: [],
    };
  }

  return {
    text: messageText.messageText,
    code: messageText.code,
    category: categoryNames[messageText.category] ?? "message",
    next: (messageText.next ?? []).map((next) =>
      serializeMessageText(next, next.code, next.category)
    ),
  };
}

const rl = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
});

for await (const line of rl) {
  if (!line.trim()) continue;
  let id = null;
  try {
    const request = JSON.parse(line);
    id = request.id;
    const diagnostics = collectDiagnostics(request.programJsonPath);
    process.stdout.write(`${JSON.stringify({ id, diagnostics })}\n`);
  } catch (error) {
    process.stdout.write(`${JSON.stringify({ id, error: String(error?.stack ?? error) })}\n`);
  }
}
