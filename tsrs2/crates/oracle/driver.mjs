import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath } from "node:url";
import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const vendorLibDir = path.resolve(__dirname, "../../vendor/typescript-6.0.3/lib");

const categoryNames = ["warning", "error", "suggestion", "message"];

function decodeProgram(programJsonPath) {
  const program = JSON.parse(fs.readFileSync(programJsonPath, "utf8"));
  const files = new Map();

  for (const lib of program.libs ?? []) {
    const libPath = path.join(vendorLibDir, lib);
    files.set(normalizeFileName(lib), fs.readFileSync(libPath, "utf8"));
  }

  for (const file of program.files ?? []) {
    files.set(normalizeFileName(file.name), Buffer.from(file.textB64, "base64").toString("utf8"));
  }

  return { program, files };
}

function normalizeFileName(fileName) {
  return fileName.replace(/\\/g, "/");
}

function compilerOptionsFromProgram(program) {
  const converted = ts.convertCompilerOptionsFromJson(program.options ?? {}, program.cwd ?? "/", "program.json");
  if (converted.errors?.length) {
    throw new Error(ts.flattenDiagnosticMessageText(converted.errors[0].messageText, "\n"));
  }

  return {
    ...converted.options,
    noLib: true,
  };
}

function createHost(options, files, cwd) {
  const languageVersion = options.target ?? ts.ScriptTarget.Latest;
  return {
    getSourceFile(fileName) {
      const text = files.get(normalizeFileName(fileName));
      return text === undefined
        ? undefined
        : ts.createSourceFile(normalizeFileName(fileName), text, languageVersion, true);
    },
    getDefaultLibFileName() {
      return "lib.d.ts";
    },
    getCurrentDirectory() {
      return cwd ?? "/";
    },
    getDirectories() {
      return [];
    },
    getCanonicalFileName(fileName) {
      return normalizeFileName(fileName);
    },
    useCaseSensitiveFileNames() {
      return true;
    },
    getNewLine() {
      return "\n";
    },
    fileExists(fileName) {
      return files.has(normalizeFileName(fileName));
    },
    readFile(fileName) {
      return files.get(normalizeFileName(fileName));
    },
    writeFile() {},
  };
}

function collectDiagnostics(programJsonPath) {
  const { program: programJson, files } = decodeProgram(programJsonPath);
  const options = compilerOptionsFromProgram(programJson);
  const rootNames = [
    ...(programJson.libs ?? []).map(normalizeFileName),
    ...(programJson.files ?? []).map((file) => normalizeFileName(file.name)),
  ];
  const host = createHost(options, files, programJson.cwd);
  const program = ts.createProgram(rootNames, options, host);
  const diagnostics = [];

  for (const file of programJson.files ?? []) {
    const sourceFile = program.getSourceFile(normalizeFileName(file.name));
    if (!sourceFile) continue;
    diagnostics.push(...program.getSyntacticDiagnostics(sourceFile));
    diagnostics.push(...program.getSemanticDiagnostics(sourceFile));
    diagnostics.push(...program.getSuggestionDiagnostics(sourceFile));
  }

  return ts.sortAndDeduplicateDiagnostics(diagnostics).map(serializeDiagnostic);
}

function serializeDiagnostic(diagnostic) {
  return {
    file: diagnostic.file?.fileName ?? null,
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    code: diagnostic.code,
    category: categoryNames[diagnostic.category] ?? "message",
    chain: serializeMessageText(diagnostic.messageText, diagnostic.code, diagnostic.category),
    related: (diagnostic.relatedInformation ?? []).map(serializeRelated),
    reportsUnnecessary: !!diagnostic.reportsUnnecessary,
    reportsDeprecated: !!diagnostic.reportsDeprecated,
    source: diagnostic.source ?? null,
  };
}

function serializeRelated(diagnostic) {
  return {
    file: diagnostic.file?.fileName ?? null,
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
