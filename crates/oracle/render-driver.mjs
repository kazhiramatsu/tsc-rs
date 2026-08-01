// A3-only renderer producer. Normal oracle collection stays in driver.mjs.
import readline from "node:readline";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import {
  absoluteProgramFileName,
  categoryNames,
  createProgramFromJsonPath,
  normalizeFileName,
  publicFileName,
} from "./program-host.mjs";

function collectDiagnosticBundle(programJsonPath) {
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

  return {
    cwd,
    publicNames,
    diagnostics: ts.sortAndDeduplicateDiagnostics(diagnostics),
  };
}

function collectDiagnostics(programJsonPath) {
  const { diagnostics, publicNames } = collectDiagnosticBundle(programJsonPath);
  return diagnostics.map((diagnostic) => serializeDiagnostic(diagnostic, publicNames));
}

function renderDiagnostics(programJsonPath) {
  const { diagnostics, publicNames, cwd } = collectDiagnosticBundle(programJsonPath);
  return {
    diagnostics: diagnostics.map((diagnostic) =>
      serializeDiagnostic(diagnostic, publicNames)
    ),
    rendered: renderDiagnosticSequence(diagnostics, cwd),
  };
}

function renderDiagnosticRecords(programJsonPath, records, recordsAlreadySorted) {
  const { program, cwd } = createProgramFromJsonPath(programJsonPath);
  const diagnostics = records.map((record) =>
    deserializeDiagnostic(record, program, cwd)
  );
  return renderDiagnosticSequence(
    recordsAlreadySorted
      ? diagnostics
      : ts.sortAndDeduplicateDiagnostics(diagnostics),
    cwd,
  );
}

function renderDiagnosticSequence(diagnostics, cwd) {
  const host = {
    getCurrentDirectory() {
      return cwd;
    },
    getCanonicalFileName(fileName) {
      return normalizeFileName(fileName);
    },
    getNewLine() {
      return "\n";
    },
  };

  // `getCategoryFormat` deliberately rejects Suggestion because tsc's
  // normal batch CLI never asks the color formatter to print that pass.
  // tsc-rs's emit-free conformance contract does include suggestions.
  // Send each suggestion through the same vendored formatter as Message
  // (the only affected bytes are the ANSI style and category word), strip
  // ANSI, then restore the real category word.
  let rendered = "";
  for (const diagnostic of diagnostics) {
    if (diagnostic.category === ts.DiagnosticCategory.Suggestion) {
      const messageDiagnostic = {
        ...diagnostic,
        category: ts.DiagnosticCategory.Message,
      };
      // Keep the genuine formatter path while avoiding a textual
      // search that can mistake a marker-shaped file name for the
      // formatter-owned header. diagnosticCategoryName reads this
      // reverse enum entry, whereas getCategoryFormat still switches
      // on the numeric Message category. The worker is single-threaded,
      // and the mutation is restored even if the formatter throws.
      const messageName = ts.DiagnosticCategory[ts.DiagnosticCategory.Message];
      let formatted;
      try {
        ts.DiagnosticCategory[ts.DiagnosticCategory.Message] = "Suggestion";
        formatted = ts.formatDiagnosticsWithColorAndContext(
          [messageDiagnostic],
          host,
        );
      } finally {
        ts.DiagnosticCategory[ts.DiagnosticCategory.Message] = messageName;
      }
      rendered += stripAnsi(formatted);
    } else {
      rendered += stripAnsi(
        ts.formatDiagnosticsWithColorAndContext([diagnostic], host),
      );
    }
  }
  return normalizeNewlines(rendered);
}

function stripAnsi(text) {
  return text.replace(/\x1B\[[0-9;]*m/g, "");
}

function normalizeNewlines(text) {
  return text.replace(/\r\n?/g, "\n");
}

function deserializeDiagnostic(record, program, cwd) {
  const relatedInformation = (record.related ?? []).map((related) =>
    deserializeRelated(related, program, cwd)
  );
  const relatedInformationPresent =
    record.relatedInformationPresent === true || relatedInformation.length > 0;
  return {
    file: sourceFileForRecord(record.file, program, cwd),
    start: record.start ?? undefined,
    length: record.length ?? undefined,
    code: record.code,
    category: categoryValue(record.category),
    messageText: deserializeMessageText(record.chain),
    relatedInformation: relatedInformationPresent ? relatedInformation : undefined,
    reportsUnnecessary: record.reportsUnnecessary || undefined,
    reportsDeprecated: record.reportsDeprecated || undefined,
    source: record.source ?? undefined,
  };
}

function deserializeRelated(record, program, cwd) {
  return {
    file: sourceFileForRecord(record.file, program, cwd),
    start: record.start ?? undefined,
    length: record.length ?? undefined,
    code: record.code,
    category: categoryValue(record.category),
    messageText: deserializeMessageText(record.chain),
  };
}

function deserializeMessageText(chain) {
  return {
    messageText: chain.text,
    code: chain.code,
    category: categoryValue(chain.category),
    next: (chain.next ?? []).map(deserializeMessageText),
  };
}

function sourceFileForRecord(fileName, program, cwd) {
  if (fileName === null || fileName === undefined) return undefined;
  const sourceFile = program.getSourceFile(absoluteProgramFileName(fileName, cwd));
  if (!sourceFile) {
    throw new Error(`render record source file is unavailable: ${JSON.stringify(fileName)}`);
  }
  return sourceFile;
}

function categoryValue(name) {
  const category = categoryNames.indexOf(name);
  if (category < 0) {
    throw new Error(`unknown render record diagnostic category: ${JSON.stringify(name)}`);
  }
  return category;
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
    relatedInformationPresent: diagnostic.relatedInformation !== undefined,
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
    if (request.versionProbe) {
      // Launch-time producer verification: the harness compares the
      // LAUNCHED process's version against the pinned .node-version
      // before writing goldens (the file alone is a declaration).
      process.stdout.write(`${JSON.stringify({ id, version: process.version })}\n`);
      continue;
    }
    if (request.renderDiagnostics === true) {
      const { diagnostics, rendered } = renderDiagnostics(request.programJsonPath);
      process.stdout.write(`${JSON.stringify({ id, diagnostics, rendered })}\n`);
      continue;
    }
    if (Array.isArray(request.renderRecords)) {
      const rendered = renderDiagnosticRecords(
        request.programJsonPath,
        request.renderRecords,
        request.recordsAlreadySorted === true,
      );
      process.stdout.write(`${JSON.stringify({ id, rendered })}\n`);
      continue;
    }
    const diagnostics = collectDiagnostics(request.programJsonPath);
    process.stdout.write(`${JSON.stringify({ id, diagnostics })}\n`);
  } catch (error) {
    process.stdout.write(`${JSON.stringify({ id, error: String(error?.stack ?? error) })}\n`);
  }
}
